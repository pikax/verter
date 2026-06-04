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
use crate::project_semantic_dispatch::{omit_builtin_decl_identity, pick_builtin_decl_identity};
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

/// Upsert a minimal `defineProps` SFC at `canonical` and build a
/// `ResolveMacroPayload` owner [`DeclIdentity`] carrying that file's
/// REAL whole hash.
///
/// A `ResolveMacroPayload` memo entry self-roots on the owner SFC's
/// `FileWholeHash`; the strict warm-read validator rejects an entry
/// whose self-root canonical is untracked or hash-mismatched. The
/// owner must therefore be a tracked file with the real content
/// version threaded into the key identity.
fn tracked_macro_owner(host: &VerterHost, canonical: &str) -> crate::semantic_query::DeclKey {
    use crate::{FileKind, UpsertRequest};
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.to_string(),
            source: Arc::from("<script setup lang=\"ts\">defineProps<{ x: string }>()</script>\n"),
            file_kind: FileKind::from_path(canonical),
            aliases: Vec::new(),
        })
        .expect("owner SFC upsert succeeds");
    // Force the IndexedReady to materialise so the cold build can
    // re-source the live whole_hash.
    let _ = host
        .ensure_indexed_ready(canonical)
        .expect("owner SFC IndexedReady materialises");
    crate::semantic_query::DeclKey {
        canonical_id: Arc::from(canonical),
        decl_name: Arc::from("<sfc-script-setup>"),
    }
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
        mode: ProjectionMode::Expanded,
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
        owner: crate::semantic_query::DeclKey {
            canonical_id: Arc::from("<synthetic>"),
            decl_name: Arc::from("OtherOwner"),
        },
        macro_index: 1,
        macro_kind: AnalyzedMacroKind::DefineProps,
        type_args: Arc::from(vec![unrelated_arg].into_boxed_slice()),
        mode: ProjectionMode::Expanded,
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

/// 5b §5.D.1 — `materialize_surface` dispatch helper: repeated
/// identical `MaterializeStructureCacheKey` increments the live-entry
/// counter EXACTLY once across N calls (the warm peek path returns
/// the cached entry on every subsequent call).
#[test]
fn cache_discipline_materialize_surface_repeated_keys_warm() {
    use crate::component_meta_materialize::{MaterializationScope, MaterializeStructureCacheKey};
    use crate::{FileKind, UpsertRequest};

    let host = build_test_host();
    // Load both scope files so the materialiser's dispatch reads have
    // a real `IndexedReady` to walk. The `MaterializeStructureDb`
    // entry does NOT self-root the consumer materialise scope (R7
    // cross-owner reuse); a `Global`-origin `base` (here
    // `intern_empty_object`) yields a zero-self-root entry that is
    // always `Cacheable`.
    for scope in ["/test.vue", "/other.vue"] {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: scope.to_string(),
                source: Arc::from("<script setup lang=\"ts\">const x = 1;</script>\n"),
                file_kind: FileKind::from_path(scope),
                aliases: Vec::new(),
            })
            .expect("scope upsert succeeds");
        host.ensure_indexed_ready(scope)
            .expect("scope IndexedReady materialises");
    }
    let base = intern_empty_object(&host);
    let key = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from("/test.vue"),
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
    // alone does NOT make the key distinct. Use a different
    // `scope_axis` to force a distinct cache entry.
    let distinct_key = MaterializeStructureCacheKey {
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
/// warm N-1. The underlying dispatch is `Instantiate { pick_decl,
/// args, body_mode }` so we probe the counter against that key.
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
        base: pick_builtin_decl_identity(),
        args: Arc::from(vec![base, key_set].into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(mode),
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
        base: pick_builtin_decl_identity(),
        args: Arc::from(vec![base, unrelated_key_set].into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(mode),
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
        base: omit_builtin_decl_identity(),
        args: Arc::from(vec![base, key_set].into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(mode),
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
        base: omit_builtin_decl_identity(),
        args: Arc::from(vec![base, unrelated_key_set].into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(mode),
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
