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
    for (is_current, edit_after_capture) in [(true, true), (false, false)] {
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
        let ctx = HostResolverContext::from_fixed_view(
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
                        canonical_id: dep.into(),
                        hash: dep_hash,
                    },
                );
                45u32
            },
            |_| {
                ComponentMetaPublishDecision::publish(
                    key.clone(),
                    owner_hash,
                    Arc::new(45),
                    dispatch.current_project_generation(),
                )
            },
        );
        assert_eq!(value, 45, "the caller retains its computed value");
        assert!(
            admitted.is_none(),
            "stale or unproven captured bases must not publish"
        );
        assert!(db.is_empty());
        if !is_current {
            assert!(
                ctx.publication_input_fingerprint().is_none(),
                "matching live inputs cannot vouch for an unproven base"
            );
        }
    }
}
