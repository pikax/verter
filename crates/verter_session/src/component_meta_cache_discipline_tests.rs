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

/// Hermetic host for §5.D.1 cache-discipline probes. No upserts —
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
    use crate::semantic_query::SurfaceView;
    host.project_type_store()
        .semantic_graph()
        .intern_node(SemanticNodeData::Object(SurfaceView {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }))
}

/// Upsert a file exporting `type {name} = { a: number }` and intern a
/// `DeclRef` to it with a `NodeScopeId::File` origin — a DECL-ROOTED
/// `base` the materialiser canonicalises (via
/// [`derive_materialization_subject`](crate::component_meta_materialize::derive_materialization_subject))
/// to `slot(canonical, name)` and publishes a warm
/// `MaterializeStructureDb` entry for. An anonymous `Object` base keys no
/// DB slot (it computes uncached), so it can no longer drive the
/// entry-count invariant — use this decl-rooted fixture instead.
fn intern_decl_ref_base(host: &VerterHost, canonical: &str, name: &str) -> SemanticNodeId {
    use crate::semantic_query::{DeclIdentity, NodeScopeId};
    use crate::UpsertRequest;
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: canonical.to_string(),
        source: Arc::from(format!("export type {name} = {{ a: number }};\n")),
        file_language: verter_language::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host.ensure_indexed_ready(canonical)
        .expect("decl-ref base fixture: canonical IndexedReady materialises");
    let whole_hash = host
        .shallow_file_state(canonical)
        .map(|s| s.whole_hash)
        .expect("decl-ref base fixture: canonical must be tracked with a whole hash");
    host.project_type_store()
        .semantic_graph()
        .intern_node_with_scope(
            SemanticNodeData::DeclRef {
                identity: DeclIdentity {
                    canonical_id: Arc::from(canonical),
                    whole_hash,
                    decl_name: Arc::from(name),
                },
            },
            NodeScopeId::File {
                canonical_id: Arc::from(canonical),
                whole_hash,
                local_scope: None,
            },
        )
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
        Arc::from("<sfc-script-setup>"),
    )
}

/// 5b §5.D.1 — `ResolveMacroPayload` repeated identical keys: cold
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

/// `materialize_surface` dispatch helper: repeated identical
/// `MaterializeRuntimeKey`s — which canonicalise to ONE content-free
/// `MaterializationCacheKey` subject — increment the live-entry counter
/// EXACTLY once across N calls (the warm peek path returns the cached
/// entry on every subsequent call).
#[test]
fn cache_discipline_materialize_surface_repeated_keys_warm() {
    use crate::component_meta_materialize::{MaterializationScope, MaterializeRuntimeKey};

    let host = build_test_host();
    // A DECL-ROOTED `base` (a `DeclRef` to `/props.ts:Props`) — the
    // materialiser canonicalises it to `slot(/props.ts, Props)` and
    // publishes a warm `MaterializeStructureDb` entry. (An anonymous
    // `Object` base now keys no DB slot — it computes uncached — so it
    // can no longer drive the cold-once / warm-N-1 entry-count invariant.)
    let base = intern_decl_ref_base(&host, "/props.ts", "Props");
    let key = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from("/props.ts"),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };

    // Drive a baseline call so any setup-time admission is paid up
    // front; the deltas measured below should reflect repeated
    // identical-key dispatch only.
    let dispatch = host.semantic_dispatch();
    let baseline_live = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();
    let _ = dispatch.materialize_surface(key.clone());
    let after_first_live = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    const N: usize = 8;
    for _ in 0..(N - 1) {
        let _ = dispatch.materialize_surface(key.clone());
    }
    let after_n_live = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    // Cold path published exactly one entry across the full run.
    assert_eq!(
        after_n_live - baseline_live,
        after_first_live - baseline_live,
        "live entry count must NOT grow across repeated identical materialize_surface calls (baseline={baseline_live}, after_first={after_first_live}, after_n={after_n_live})"
    );

    // Cross-owner reuse (R7): a different `scope_canonical_id`
    // alone does NOT make the key distinct (it is in neither the
    // recursion key nor the canonical `MaterializationCacheKey`). Use a
    // different `scope_axis` (a policy axis the canonical key DOES carry)
    // to force a distinct cache entry.
    let distinct_key = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from("/other.vue"),
        base,
        scope_axis: MaterializationScope::Nested,
        mode: ProjectionMode::Expanded,
    };
    let _ = dispatch.materialize_surface(distinct_key);
    let after_unrelated_live = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();
    assert!(
        after_unrelated_live > after_n_live,
        "distinct (scope_axis) cold key must publish a fresh live entry (after_n={after_n_live}, after_unrelated={after_unrelated_live})"
    );
}

/// 5b §5.D.1 — `execute_pick` repeated identical keys: cold once,
/// warm N-1. The underlying dispatch is `Instantiate { base: pick slot,
/// args, context }` so we probe the counter against that key.
#[test]
fn cache_discipline_execute_pick_repeated_keys_warm() {
    let host = build_test_host();
    let base = intern_empty_object(&host);
    let members: Vec<Arc<str>> = vec![Arc::from("a"), Arc::from("b")];
    let mode = ProjectionMode::Expanded;

    // Construct the same Instantiate key shape execute_pick uses
    // internally so we can read the cold/warm counters for it.
    let key_set = host
        .semantic_dispatch()
        .intern_string_literal_union(&members);
    let probe_key = SemanticQueryKey::Instantiate {
        base: host.semantic_dispatch().builtin_type_slot("Pick"),
        args: Arc::from(vec![base, key_set].into_boxed_slice()),
        context: host.semantic_dispatch().instantiate_context_for(
            "__builtin__",
            crate::semantic_query::ProjectionReductionContext::published(mode),
        ),
    };

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&probe_key);
    let baseline_warm = counter.family_warm(&probe_key);

    const N: usize = 8;
    let dispatch = host.semantic_dispatch();
    for _ in 0..N {
        let _ = dispatch.execute_pick(base, &members, mode);
    }

    let cold = counter.family_cold(&probe_key) - baseline_cold;
    let warm = counter.family_warm(&probe_key) - baseline_warm;
    assert_eq!(
        cold, 1,
        "execute_pick cold path should fire ONCE for repeated identical key (got {cold})"
    );
    assert_eq!(
        warm,
        N - 1,
        "execute_pick warm path should fire N-1 times for repeated identical key (got {warm})"
    );

    // Negative assertion: a different members set produces a
    // distinct key that cold-fires once.
    let unrelated_members: Vec<Arc<str>> = vec![Arc::from("z")];
    let unrelated_key_set = host
        .semantic_dispatch()
        .intern_string_literal_union(&unrelated_members);
    let unrelated_probe = SemanticQueryKey::Instantiate {
        base: host.semantic_dispatch().builtin_type_slot("Pick"),
        args: Arc::from(vec![base, unrelated_key_set].into_boxed_slice()),
        context: host.semantic_dispatch().instantiate_context_for(
            "__builtin__",
            crate::semantic_query::ProjectionReductionContext::published(mode),
        ),
    };
    let unrelated_baseline_cold = counter.family_cold(&unrelated_probe);
    let _ = dispatch.execute_pick(base, &unrelated_members, mode);
    let unrelated_cold = counter.family_cold(&unrelated_probe) - unrelated_baseline_cold;
    assert_eq!(
        unrelated_cold, 1,
        "unrelated execute_pick cold key should still cold-fire (got {unrelated_cold})"
    );
}

/// 5b §5.D.1 — `execute_omit` repeated identical keys: cold once,
/// warm N-1. Mirrors `execute_pick` shape. Underlying dispatch is
/// `Instantiate { omit_decl, ... }`.
#[test]
fn cache_discipline_execute_omit_repeated_keys_warm() {
    let host = build_test_host();
    let base = intern_empty_object(&host);
    let members: Vec<Arc<str>> = vec![Arc::from("x"), Arc::from("y")];
    let mode = ProjectionMode::Expanded;

    let key_set = host
        .semantic_dispatch()
        .intern_string_literal_union(&members);
    let probe_key = SemanticQueryKey::Instantiate {
        base: host.semantic_dispatch().builtin_type_slot("Omit"),
        args: Arc::from(vec![base, key_set].into_boxed_slice()),
        context: host.semantic_dispatch().instantiate_context_for(
            "__builtin__",
            crate::semantic_query::ProjectionReductionContext::published(mode),
        ),
    };

    let counter = DispatchCounter;
    let baseline_cold = counter.family_cold(&probe_key);
    let baseline_warm = counter.family_warm(&probe_key);

    const N: usize = 8;
    let dispatch = host.semantic_dispatch();
    for _ in 0..N {
        let _ = dispatch.execute_omit(base, &members, mode);
    }

    let cold = counter.family_cold(&probe_key) - baseline_cold;
    let warm = counter.family_warm(&probe_key) - baseline_warm;
    assert_eq!(
        cold, 1,
        "execute_omit cold path should fire ONCE for repeated identical key (got {cold})"
    );
    assert_eq!(
        warm,
        N - 1,
        "execute_omit warm path should fire N-1 times for repeated identical key (got {warm})"
    );

    // Negative assertion: distinct members set still cold-fires.
    let unrelated_members: Vec<Arc<str>> = vec![Arc::from("q")];
    let unrelated_key_set = host
        .semantic_dispatch()
        .intern_string_literal_union(&unrelated_members);
    let unrelated_probe = SemanticQueryKey::Instantiate {
        base: host.semantic_dispatch().builtin_type_slot("Omit"),
        args: Arc::from(vec![base, unrelated_key_set].into_boxed_slice()),
        context: host.semantic_dispatch().instantiate_context_for(
            "__builtin__",
            crate::semantic_query::ProjectionReductionContext::published(mode),
        ),
    };
    let unrelated_baseline_cold = counter.family_cold(&unrelated_probe);
    let _ = dispatch.execute_omit(base, &unrelated_members, mode);
    let unrelated_cold = counter.family_cold(&unrelated_probe) - unrelated_baseline_cold;
    assert_eq!(
        unrelated_cold, 1,
        "unrelated execute_omit cold key should still cold-fire (got {unrelated_cold})"
    );
}

/// 5b §5.D.1 — `execute_to_type_expr` repeated identical keys: cold
/// once, warm N-1. The helper wraps `execute_read(key.clone())` so
/// the counter probe targets the wrapped key directly.
#[test]
fn cache_discipline_execute_to_type_expr_repeated_keys_warm() {
    let host = build_test_host();
    let base = intern_empty_object(&host);
    // Use a ProjectMember key as the wrapped dispatch — it routes
    // through execute_read like every other variant. The
    // SemanticQueryKeyDigest canonicalises ProjectMember →
    // ProjectPath before hashing so the counter probe with the
    // pre-canonical key form sees the same digest as the warm cache.
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
        let _ = dispatch.execute_to_type_expr(&key);
    }

    let cold = counter.family_cold(&key) - baseline_cold;
    let warm = counter.family_warm(&key) - baseline_warm;
    assert_eq!(
        cold, 1,
        "execute_to_type_expr cold path should fire ONCE for repeated identical key (got {cold})"
    );
    assert_eq!(
        warm,
        N - 1,
        "execute_to_type_expr warm path should fire N-1 times for repeated identical key (got {warm})"
    );

    // Negative assertion: a different member name produces a
    // distinct key that cold-fires once.
    let unrelated_key = SemanticQueryKey::ProjectMember {
        base,
        member: Arc::from("z"),
        mode: ProjectionMode::Expanded,
    };
    let unrelated_baseline_cold = counter.family_cold(&unrelated_key);
    let _ = dispatch.execute_to_type_expr(&unrelated_key);
    let unrelated_cold = counter.family_cold(&unrelated_key) - unrelated_baseline_cold;
    assert_eq!(
        unrelated_cold, 1,
        "unrelated execute_to_type_expr cold key should still cold-fire (got {unrelated_cold})"
    );
}
