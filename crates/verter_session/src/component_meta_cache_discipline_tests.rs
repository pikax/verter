//! §5.D.1 cache-discipline tests for the §5.B-introduced variants and
//! dispatch helpers (Phase 5g-supplement backfill for 5b).
//!
//! Each test asserts that re-issuing the SAME dispatch key N times
//! triggers the cold path EXACTLY ONCE and the warm path N-1 times.
//! A negative assertion against an unrelated key confirms the warm
//! hits are not a wholesale "everything is warm" artifact.
//!
//! These are characterization tests — they fail if the warm cache is
//! either bypassed (cold > 1 for repeated identical keys) or
//! over-eager (warm > 0 for the unrelated cold-path probe). They use
//! the §5.D.0 r17 instrumentation surface (`dispatch_counter()`) which
//! lives behind bare `#[cfg(test)]` per r17/N12.
//!
//! Plan: §5.D.1 (Phase 5g-supplement.1.B for 5b backfill).

use std::sync::Arc;

use verter_semantic::analysis::AnalyzedMacroKind;

use crate::host_test_audit::DispatchCounter;
use crate::semantic_query::{
    DeclIdentity, ProjectionMode, SemanticNodeData, SemanticNodeId, SemanticQueryApi,
    SemanticQueryKey,
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

/// 5b §5.D.1 — `ResolveMacroPayload` repeated identical keys: cold
/// once, warm N-1 times. Negative assertion against an unrelated
/// `ResolveMacroPayload` proves the warm hits are key-specific.
///
/// Uses `DefineProps` with a single type_arg so the build returns
/// `QueryResult::Value(type_args[0])` (publishable) rather than the
/// `Error(Miss)` shape the snapshot-driven arms produce when the
/// owner is synthetic.
#[test]
fn cache_discipline_resolve_macro_payload_repeated_keys_warm() {
    let host = build_test_host();
    let arg = intern_empty_object(&host);
    let key = SemanticQueryKey::ResolveMacroPayload {
        owner: DeclIdentity::synthetic("TestOwner"),
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
        let _ = dispatch.execute(key.clone());
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
    let unrelated_arg = host
        .project_type_store()
        .semantic_graph()
        .intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
    let unrelated_key = SemanticQueryKey::ResolveMacroPayload {
        owner: DeclIdentity::synthetic("OtherOwner"),
        macro_index: 1,
        macro_kind: AnalyzedMacroKind::DefineProps,
        type_args: Arc::from(vec![unrelated_arg].into_boxed_slice()),
        mode: ProjectionMode::Expanded,
    };
    let unrelated_baseline_cold = counter.family_cold(&unrelated_key);
    let unrelated_baseline_warm = counter.family_warm(&unrelated_key);
    let _ = dispatch.execute(unrelated_key.clone());
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
