//! Cache-discipline tests for the SemanticQueryKey variants and
//! dispatch helpers.
//!
//! Each test asserts that re-issuing the SAME dispatch key N times
//! triggers the cold path EXACTLY ONCE and the warm path N-1 times.
//! A negative assertion against an unrelated key confirms the warm
//! hits are not a wholesale "everything is warm" artifact.
//!
//! These are characterization tests — they fail if the warm cache is
//! either bypassed (cold > 1 for repeated identical keys) or
//! over-eager (warm > 0 for the unrelated cold-path probe). They use
//! the test-only `dispatch_counter()` instrumentation surface which
//! lives behind bare `#[cfg(test)]`.

use std::sync::Arc;

use verter_semantic::analysis::AnalyzedMacroKind;

use crate::host_test_audit::DispatchCounter;
use crate::semantic_query::{
    ProjectionMode, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
};
use crate::types::HostConfig;
use crate::VerterHost;

/// Hermetic host for the cache-discipline probes. No upserts —
/// the dispatcher executes against an empty graph and produces
/// `Opaque(Miss)` results that are still cached per family/slot, so
/// the cold/warm split is measurable without a full file fixture.
fn build_test_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }))
}

/// Intern an arbitrary `Object` shell into the host's graph and
/// return its node id. Used as a stable `base` for keys that
/// otherwise need a `SemanticNodeId` argument.
fn intern_empty_object(host: &VerterHost) -> SemanticNodeId {
    host.project_type_store()
        .semantic_graph()
        .intern_node(SemanticNodeData::Object(crate::test_surface_view! {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }))
}

/// Upsert a minimal `defineProps` SFC at `canonical` and build the
/// content-free `ResolveMacroPayload` owner
/// [`ResolvedDeclSlotIdentity`] slot for that file.
///
/// A `ResolveMacroPayload` memo entry self-roots on the owner SFC's
/// `FileWholeHash`; the strict warm-read validator rejects an entry
/// whose self-root canonical is untracked or hash-mismatched. The
/// owner slot is content-free (no whole hash in the key) — the real
/// content version is re-sourced live at value-build time via
/// `ensure_indexed_ready`, so the owner must be a TRACKED file or the
/// cold build cannot self-root and the entry stays non-cacheable.
fn tracked_macro_owner(
    host: &VerterHost,
    canonical: &str,
) -> crate::semantic_query::ResolvedDeclSlotIdentity {
    use crate::UpsertRequest;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.to_string(),
            source: Arc::from("<script setup lang=\"ts\">defineProps<{ x: string }>()</script>\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("owner SFC upsert succeeds");
    // Force the IndexedReady to materialise so the cold build can
    // re-source the live whole_hash.
    let _ = host
        .ensure_indexed_ready(canonical)
        .expect("owner SFC IndexedReady materialises");
    crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
        Arc::from(canonical),
        verter_type_expr::TopLevelOwnerId::instance(0),
        Arc::from("<sfc-script-setup>"),
    )
}

/// `ResolveMacroPayload` repeated identical keys: cold
/// once, warm N-1 times. Negative assertion against an unrelated
/// `ResolveMacroPayload` proves the warm hits are key-specific.
///
/// Uses `DefineProps` with a single type_arg so the build returns
/// `QueryResult::Value(type_args[0])` (publishable). The owner is a
/// real tracked SFC so the entry's self-root `FileWholeHash` passes
/// strict warm-read validation.
#[test]
fn cache_discipline_resolve_macro_payload_repeated_keys_warm() {
    let host = build_test_host();
    let arg = intern_empty_object(&host);
    // The `ResolveMacroPayload` memo entry self-roots on the owner
    // SFC's `FileWholeHash`; the owner canonical must be a tracked
    // file so the strict warm-read validator can confirm the self-root
    // and the repeated-key warm hit lands.
    let owner = tracked_macro_owner(&host, "/cache_discipline_owner.vue");
    let key = SemanticQueryKey::ResolveMacroPayload {
        owner,
        macro_index: 0,
        macro_kind: AnalyzedMacroKind::DefineProps,
        type_args: Arc::from(vec![arg].into_boxed_slice()),
        context: crate::semantic_query::MacroPayloadContext::new(
            Default::default(),
            ProjectionMode::Expanded,
        ),
    };

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);

    const N: usize = 8;
    let dispatch = host.semantic_dispatch();
    for _ in 0..N {
        let _ = dispatch.execute_type_node(key.clone());
    }

    let cold = counter.family_cold(&key) - baseline_cold;
    let warm = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold, 1,
        "cold path should fire ONCE for repeated identical key (got {cold})"
    );
    assert_eq!(
        warm,
        N - 1,
        "warm path should fire N-1 times for repeated identical key (got {warm})"
    );

    // Negative assertion: an UNRELATED ResolveMacroPayload key cold-fires.
    let unrelated_arg =
        host.project_type_store()
            .semantic_graph()
            .intern_node(SemanticNodeData::Primitive(
                crate::semantic_query::PrimitiveKind::String,
            ));
    let unrelated_key = SemanticQueryKey::ResolveMacroPayload {
        owner: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from("<synthetic>"),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from("OtherOwner"),
        ),
        macro_index: 1,
        macro_kind: AnalyzedMacroKind::DefineProps,
        type_args: Arc::from(vec![unrelated_arg].into_boxed_slice()),
        context: crate::semantic_query::MacroPayloadContext::new(
            Default::default(),
            ProjectionMode::Expanded,
        ),
    };
    let unrelated_baseline_cold = counter.family_cold(&unrelated_key);
    let unrelated_baseline_warm = counter.family_warm(&unrelated_key);
    let _ = dispatch.execute_type_node(unrelated_key.clone());
    let unrelated_cold = counter.family_cold(&unrelated_key) - unrelated_baseline_cold;
    let unrelated_warm = counter.family_warm(&unrelated_key) - unrelated_baseline_warm;
    assert_eq!(
        unrelated_cold, 1,
        "unrelated cold key should still cold-fire (got {unrelated_cold})"
    );
    assert_eq!(
        unrelated_warm, 0,
        "unrelated key must NOT warm-fire on its first call (got {unrelated_warm})"
    );
}

/// The builtin Pick utility `Instantiate` family: cold once, warm N-1.
/// The canonical key is `Instantiate { base: pick slot, args, context }`
/// constructed directly — there is no wrapper entrance; every utility
/// route constructs the shared family key itself.
#[test]
fn cache_discipline_builtin_pick_instantiate_repeated_keys_warm() {
    let host = build_test_host();
    let base = intern_empty_object(&host);
    let members: Vec<Arc<str>> = vec![Arc::from("a"), Arc::from("b")];
    let mode = ProjectionMode::Expanded;

    // The canonical builtin Pick key shape, constructed directly.
    let key_set = host
        .semantic_dispatch()
        .intern_string_literal_union(&members);
    let probe_key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
        host.semantic_dispatch().builtin_type_slot("Pick"),
        Arc::from(vec![base, key_set].into_boxed_slice()),
        host.semantic_dispatch().instantiate_context_for(
            "__builtin__",
            crate::semantic_query::ProjectionReductionContext::published(mode),
        ),
    ));

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&probe_key);
    let baseline_warm = counter.family_warm(&probe_key);

    const N: usize = 8;
    let dispatch = host.semantic_dispatch();
    for _ in 0..N {
        let _ = dispatch.execute_type_node(probe_key.clone());
    }

    let cold = counter.family_cold(&probe_key) - baseline_cold;
    let warm = counter.family_warm(&probe_key) - baseline_warm;
    assert_eq!(
        cold, 1,
        "builtin Pick Instantiate cold path should fire ONCE for repeated identical key (got {cold})"
    );
    assert_eq!(
        warm,
        N - 1,
        "builtin Pick Instantiate warm path should fire N-1 times for repeated identical key (got {warm})"
    );

    // Negative assertion: a different members set produces a
    // distinct key that cold-fires once.
    let unrelated_members: Vec<Arc<str>> = vec![Arc::from("z")];
    let unrelated_key_set = host
        .semantic_dispatch()
        .intern_string_literal_union(&unrelated_members);
    let unrelated_probe =
        SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            host.semantic_dispatch().builtin_type_slot("Pick"),
            Arc::from(vec![base, unrelated_key_set].into_boxed_slice()),
            host.semantic_dispatch().instantiate_context_for(
                "__builtin__",
                crate::semantic_query::ProjectionReductionContext::published(mode),
            ),
        ));
    let unrelated_baseline_cold = counter.family_cold(&unrelated_probe);
    let _ = dispatch.execute_type_node(unrelated_probe.clone());
    let unrelated_cold = counter.family_cold(&unrelated_probe) - unrelated_baseline_cold;
    assert_eq!(
        unrelated_cold, 1,
        "unrelated builtin Pick cold key should still cold-fire (got {unrelated_cold})"
    );
}

/// The builtin Omit utility `Instantiate` family: cold once,
/// warm N-1. Mirrors the Pick test shape.
#[test]
fn cache_discipline_builtin_omit_instantiate_repeated_keys_warm() {
    let host = build_test_host();
    let base = intern_empty_object(&host);
    let members: Vec<Arc<str>> = vec![Arc::from("x"), Arc::from("y")];
    let mode = ProjectionMode::Expanded;

    let key_set = host
        .semantic_dispatch()
        .intern_string_literal_union(&members);
    let probe_key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
        host.semantic_dispatch().builtin_type_slot("Omit"),
        Arc::from(vec![base, key_set].into_boxed_slice()),
        host.semantic_dispatch().instantiate_context_for(
            "__builtin__",
            crate::semantic_query::ProjectionReductionContext::published(mode),
        ),
    ));

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&probe_key);
    let baseline_warm = counter.family_warm(&probe_key);

    const N: usize = 8;
    let dispatch = host.semantic_dispatch();
    for _ in 0..N {
        let _ = dispatch.execute_type_node(probe_key.clone());
    }

    let cold = counter.family_cold(&probe_key) - baseline_cold;
    let warm = counter.family_warm(&probe_key) - baseline_warm;
    assert_eq!(
        cold, 1,
        "builtin Omit Instantiate cold path should fire ONCE for repeated identical key (got {cold})"
    );
    assert_eq!(
        warm,
        N - 1,
        "builtin Omit Instantiate warm path should fire N-1 times for repeated identical key (got {warm})"
    );

    // Negative assertion: distinct members set still cold-fires.
    let unrelated_members: Vec<Arc<str>> = vec![Arc::from("q")];
    let unrelated_key_set = host
        .semantic_dispatch()
        .intern_string_literal_union(&unrelated_members);
    let unrelated_probe =
        SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            host.semantic_dispatch().builtin_type_slot("Omit"),
            Arc::from(vec![base, unrelated_key_set].into_boxed_slice()),
            host.semantic_dispatch().instantiate_context_for(
                "__builtin__",
                crate::semantic_query::ProjectionReductionContext::published(mode),
            ),
        ));
    let unrelated_baseline_cold = counter.family_cold(&unrelated_probe);
    let _ = dispatch.execute_type_node(unrelated_probe.clone());
    let unrelated_cold = counter.family_cold(&unrelated_probe) - unrelated_baseline_cold;
    assert_eq!(
        unrelated_cold, 1,
        "unrelated builtin Omit cold key should still cold-fire (got {unrelated_cold})"
    );
}

/// `execute_read` repeated identical keys: cold once, warm N-1.
/// The Kind-B sink adapters gate on `execute_read(key)` (the node read the
/// former `execute_to_type_expr` wrapped), so the counter probe targets the
/// dispatch key directly.
#[test]
fn cache_discipline_execute_read_repeated_keys_warm() {
    let host = build_test_host();
    let base = intern_empty_object(&host);
    // Use a ProjectMember key — it routes through execute_read like every
    // other variant. The SemanticQueryKeyDigest canonicalises ProjectMember →
    // ProjectPath before hashing so the counter probe with the pre-canonical
    // key form sees the same digest as the warm cache.
    let key = SemanticQueryKey::ProjectMember {
        base,
        member: Arc::from("a"),
        mode: ProjectionMode::Expanded,
    };

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&key);
    let baseline_warm = counter.family_warm(&key);

    const N: usize = 8;
    let dispatch = host.semantic_dispatch();
    for _ in 0..N {
        let _ = dispatch.execute_read(key.clone());
    }

    let cold = counter.family_cold(&key) - baseline_cold;
    let warm = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold, 1,
        "execute_read cold path should fire ONCE for repeated identical key (got {cold})"
    );
    assert_eq!(
        warm,
        N - 1,
        "execute_read warm path should fire N-1 times for repeated identical key (got {warm})"
    );

    // Negative assertion: a different member name produces a
    // distinct key that cold-fires once.
    let unrelated_key = SemanticQueryKey::ProjectMember {
        base,
        member: Arc::from("z"),
        mode: ProjectionMode::Expanded,
    };
    let unrelated_baseline_cold = counter.family_cold(&unrelated_key);
    let _ = dispatch.execute_read(unrelated_key.clone());
    let unrelated_cold = counter.family_cold(&unrelated_key) - unrelated_baseline_cold;
    assert_eq!(
        unrelated_cold, 1,
        "unrelated execute_read cold key should still cold-fire (got {unrelated_cold})"
    );
}

/// Upsert a tracked file at `canonical` with the language the registry
/// classifies for that path.
fn upsert_tracked(host: &VerterHost, canonical: &str, source: &str) {
    use crate::UpsertRequest;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("fixture upsert succeeds");
}

/// Cross-owner reuse on the shared route: N owner SFCs that each project
/// `Pick<Shared, 'id'>` from ONE shared declaration file resolve that
/// declaration through the same content-free slot keys — the cycle-gate
/// classification and the Skeleton-transit instantiation of `Shared` are
/// published as ONE candidate each by the first owner, and every later
/// owner serves them warm without a further cold dispatch. A route that
/// keyed the shared declaration on the consuming owner, or forked a
/// per-owner evaluator, would cold-dispatch once per owner and publish N
/// candidates. The per-owner prop assertion keeps the reuse assertion honest:
/// every owner really did project the shared declaration.
#[test]
fn cross_owner_shared_declaration_resolves_once_across_owners() {
    use crate::semantic_query::{DeclIdentity, InstantiateKey, ProjectionReductionContext};
    use crate::types::DependencyResolution;

    let host = build_test_host();
    let shared = "/cross_owner/shared.ts";
    upsert_tracked(
        &host,
        shared,
        "export interface Shared { id: string; label: string }\n",
    );
    const N: usize = 4;
    let owners: Vec<String> = (0..N)
        .map(|i| format!("/cross_owner/Owner{i}.vue"))
        .collect();
    for owner in &owners {
        upsert_tracked(
            &host,
            owner,
            "<script setup lang=\"ts\">\nimport type { Shared } from './shared'\n\
             defineProps<{ value: Pick<Shared, 'id'> }>()\n</script>\n\
             <template><div /></template>\n",
        );
        host.set_import_dependencies(
            owner,
            vec![DependencyResolution {
                specifier: "./shared".to_string(),
                resolved_canonical_id: Some(shared.to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
    }

    // A `.ts` module's top-level declarations are owned by the module owner.
    let owner_id = verter_type_expr::TopLevelOwnerId::module(0);
    let dispatch = host.semantic_dispatch();
    let shared_slot = || dispatch.type_slot_for(Arc::from(shared), owner_id, Arc::from("Shared"));
    // The two shared-declaration keys the `Pick` projection dispatches: the
    // cycle-gate classification of `Shared` and its Skeleton-transit
    // instantiation (the gate's own body demand).
    let gate_key = dispatch.materialization_cycle_gate_key_for(&DeclIdentity {
        canonical_id: Arc::from(shared),
        owner: owner_id,
        whole_hash: host
            .shallow_file_state(shared)
            .map(|state| state.whole_hash)
            .expect("the shared declaration file is tracked"),
        decl_name: Arc::from("Shared"),
    });
    let skeleton_key = SemanticQueryKey::Instantiate(InstantiateKey::new(
        shared_slot(),
        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        dispatch.instantiate_context_for(
            shared,
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Skeleton),
        ),
    ));
    let counter = DispatchCounter;
    let candidates = |key: &SemanticQueryKey| {
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(key)
    };
    let resolve = |owner: &str| {
        let meta = host
            .get_component_meta(owner)
            .unwrap_or_else(|| panic!("{owner} publishes component meta"));
        assert!(
            meta.props.iter().any(|prop| prop.name == "value"),
            "{owner} must publish the `value` prop projected from the shared declaration"
        );
    };

    // The first owner performs the shared declaration's cold work and
    // publishes exactly one candidate per key.
    resolve(&owners[0]);
    let gate_cold_after_first = counter.family_cold(&gate_key);
    let skeleton_cold_after_first = counter.family_cold(&skeleton_key);
    assert!(
        gate_cold_after_first >= 1 && skeleton_cold_after_first >= 1,
        "control: the first owner cold-dispatches the shared declaration's keys \
         (gate cold={gate_cold_after_first}, skeleton cold={skeleton_cold_after_first})"
    );
    assert_eq!(
        (candidates(&gate_key), candidates(&skeleton_key)),
        (1, 1),
        "the first owner publishes exactly one candidate per shared-declaration key"
    );
    let gate_warm_after_first = counter.family_warm(&gate_key);

    // Every later owner reuses those candidates: no further cold dispatch,
    // no candidate growth, and the gate serves warm.
    for owner in &owners[1..] {
        resolve(owner);
    }
    assert_eq!(
        counter.family_cold(&gate_key),
        gate_cold_after_first,
        "no owner after the first cold-dispatches the shared cycle-gate key"
    );
    assert_eq!(
        counter.family_cold(&skeleton_key),
        skeleton_cold_after_first,
        "no owner after the first cold-dispatches the shared Skeleton instantiation"
    );
    assert_eq!(
        (candidates(&gate_key), candidates(&skeleton_key)),
        (1, 1),
        "{N} owners share one candidate per shared-declaration key — never N per-owner entries"
    );
    assert!(
        counter.family_warm(&gate_key) - gate_warm_after_first >= N - 1,
        "each later owner serves the shared cycle-gate classification warm"
    );
}
