use std::sync::Arc;

use crate::component_meta_result_admission::ComponentMetaResultPublish;
use crate::component_meta_result_db::{ComponentMetaPublishDecision, ComponentMetaResultDb};
use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::fact_validation_port::FactValidation;

#[test]
fn captured_component_publication_fence_refuses_stale_and_unproven_bases() {
    for (is_current, edit_after_capture, admits) in [
        (true, true, false),
        (false, false, false),
        (true, false, true),
    ] {
        let host = VerterHost::new_standalone(HostConfig::default());
        let owner = "/src/Owner.vue";
        let dep = "/src/dep.ts";
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(owner.into()),
                input_id: owner.into(),
                source: Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>",
                ),
                file_language: crate::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let update_dep = |source: &str| {
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(dep.into()),
                    input_id: dep.into(),
                    source: Arc::from(source),
                    file_language: crate::FileLanguage::script_ts(),
                    aliases: Vec::new(),
                })
                .unwrap();
        };
        update_dep("export type Value = string;");
        let owner_hash = host.ensure_indexed_ready(owner).unwrap().whole_hash;
        let dep_hash = host.ensure_indexed_ready(dep).unwrap().whole_hash;
        let captured = host.resolver_store_view_read().into_owned_view();
        let ctx = HostResolverContext::<'_, verter_audit::observe::Uncaptured>::from_fixed_view(
            &host,
            &captured,
            Arc::new(CanonicalCompletionOverlay::new()),
            is_current,
        );
        if edit_after_capture {
            update_dep("export type Value = number;");
        }
        let dispatch = ProjectSemanticDispatch::new(&ctx);
        let db = ComponentMetaResultDb::<u32>::new();
        let facade = ComponentMetaResultPublish::new(&dispatch, &db);
        let key = host
            .component_meta_result_key(owner, &crate::host_manage::ComponentMetaOptions::default());
        let (value, admitted) = facade.compute_and_admit_with_entry(
            owner,
            "captured-publication-fence",
            || {
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    FactVersionRef::FileWholeHash {
                        canonical_id: owner.into(),
                        hash: owner_hash,
                    },
                );
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    FactVersionRef::FileWholeHash {
                        canonical_id: dep.into(),
                        hash: dep_hash,
                    },
                );
                45u32
            },
            |_, _evidence| {
                ComponentMetaPublishDecision::publish(
                    key.clone(),
                    owner_hash,
                    Arc::new(45),
                    dispatch.current_project_generation(),
                )
            },
        );
        assert_eq!(value, 45, "the caller retains its computed value");
        if admits {
            assert!(
                admitted.is_some(),
                "a current capture with an unchanged dependency publishes"
            );
            assert!(!db.is_empty());
        } else {
            assert!(
                admitted.is_none(),
                "stale or unproven captured bases must not publish"
            );
            assert!(db.is_empty());
        }
        if !is_current {
            assert!(
                ctx.publication_input_fingerprint().is_none(),
                "matching live inputs cannot vouch for an unproven base"
            );
        }
    }
}

#[test]
fn warm_native_component_read_replays_dependencies_and_rejected_read_replays_nothing() {
    use crate::component_meta_host::ComponentMetaHost;
    use crate::component_meta_result_admission::ComponentMetaResultRead;
    use verter_session_query::facts::fact_read_set::FactReadSetFinalise;

    let mh = ComponentMetaHost::new_standalone(HostConfig::default());
    let owner = "/src/Comp.vue";
    let dep = "/src/types.ts";
    mh.upsert_base(dep, "export interface Props { a: string; }")
        .unwrap();
    mh.upsert_base(
        owner,
        "<script setup lang=\"ts\">import type { Props } from './types'; defineProps<Props>();</script>",
    )
    .unwrap();
    let host = mh.host();
    let cold = host.get_component_meta(owner).expect("cold native result");
    assert_eq!(cold.props.len(), 1);
    assert_eq!(cold.props[0].name, "a");
    let key =
        host.component_meta_result_key(owner, &crate::host_manage::ComponentMetaOptions::default());
    let owner_hash = host.ensure_indexed_ready(owner).unwrap().whole_hash;
    let db = host.project_type_store().component_meta_results();
    let admitted = db.get(&key, owner_hash).expect("admitted cold result");
    assert!(admitted
        .read_set_signature
        .canonical_ids()
        .iter()
        .any(|canonical| canonical.as_ref() == dep));

    let current = host.resolver_store_view_read().current().unwrap();
    let ctx = HostResolverContext::<'_, verter_audit::observe::Uncaptured>::from_current(
        host,
        &current,
        Arc::new(CanonicalCompletionOverlay::new()),
    );
    let dispatch = ProjectSemanticDispatch::new(&ctx);
    let ((warm, inner_finalise), finalise) =
        dispatch.traced_compute(|| dispatch.traced_compute(|| host.get_component_meta(owner)));
    let warm = warm.expect("warm native result");
    assert_eq!(warm.props.len(), cold.props.len());
    assert_eq!(warm.props[0].name, cold.props[0].name);
    let FactReadSetFinalise::Ok(inner_facts) = inner_finalise else {
        panic!("a validated warm read must have cacheable evidence");
    };
    let FactReadSetFinalise::Ok(facts) = finalise else {
        panic!("a validated warm read must have cacheable evidence");
    };
    assert_eq!(
        inner_facts.to_vec(),
        admitted.read_set_signature.facts.to_vec()
    );
    assert_eq!(
        facts.to_vec(),
        admitted.read_set_signature.facts.to_vec(),
        "a consumer adopts the warm result's entire dependency signature"
    );

    mh.upsert_base(dep, "export interface Props { a: number; }")
        .unwrap();
    let current = host.resolver_store_view_read().current().unwrap();
    let ctx = HostResolverContext::<'_, verter_audit::observe::Uncaptured>::from_current(
        host,
        &current,
        Arc::new(CanonicalCompletionOverlay::new()),
    );
    let dispatch = ProjectSemanticDispatch::new(&ctx);
    let facade = ComponentMetaResultRead::new(&dispatch, db, host.provenance());
    let (rejected, finalise) = dispatch.traced_compute(|| facade.peek(&key, owner_hash));
    assert!(
        rejected.is_none(),
        "the edited dependency rejects the candidate"
    );
    let FactReadSetFinalise::Ok(facts) = finalise else {
        panic!("a rejected candidate leaves the consumer's trace untouched");
    };
    // Availability reads made while probing are the consumer's own current
    // observations (route interfaces only); the rejected candidate's
    // dependency facts must not have been replayed.
    let observed = facts.to_vec();
    assert!(
        observed.iter().all(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::Parse(parse)
                if matches!(parse.key, verter_session_query::facts::FactKey::SyntacticRouteInterface)
        )),
        "validation must precede dependency replay: {observed:?}"
    );
    assert!(
        admitted
            .read_set_signature
            .facts
            .to_vec()
            .iter()
            .any(|fact| !observed.contains(fact)),
        "the candidate carries dependency facts beyond the probe's own reads"
    );
}
