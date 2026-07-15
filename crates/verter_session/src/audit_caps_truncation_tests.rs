//! Discriminating tests for `RequestFootprintAccumulator`'s
//! cap-bounded push surface.
//!
//! ## Why this test exists
//!
//! Before this fix, every `Vec` lane inside the accumulator
//! (`structured_events`, `vfs_reads`, `derivation_edges_raw`,
//! `instantiations`, …) accumulated via unbounded `Vec::push`.
//! ChatMessages.vue produced an 8.4 GB audit JSON and a 20 GB RSS
//! delta on a single audited request — pathological component trees
//! would OOM a real LSP host with `audit_enabled = true`.
//!
//! The fix:
//! 1. Adds [`verter_audit::AuditCaps`] with `DEFAULT_*` constants
//!    (10_000 per category) and per-category overrides.
//! 2. Plumbs `audit_caps: AuditCaps` through `HostConfig`.
//! 3. The accumulator's push methods consult the matching cap; once
//!    the cap is reached, the item is dropped and the matching
//!    counter on
//!    [`verter_audit::RequestFootprintAudit::truncation_counters`]
//!    is incremented.
//!
//! ## Discriminating properties
//!
//! Each test below pushes `cap + N` items into one lane and verifies:
//! - The accumulator's drained `Vec` contains exactly `cap` items.
//! - The matching `*_truncated` counter equals `N`.
//!
//! Pre-fix every lane was unbounded `push` — `Vec::len()` would equal
//! `cap + N` and `*_truncated` would be `0`. The assertions below
//! fail. Post-fix the cap fires; assertions pass.

use std::sync::Arc;

use verter_audit::{AuditCaps, MaterializationSubject};

use crate::component_meta_audit::{
    accumulator::RequestFootprintAccumulator, AliasResolveRecord, ConditionalRecord,
    IndexedReadyBuildRecord, InstantiationRecord, MaterializationRecord, ProjectionRecord,
    StructuredAuditEvent, SubstitutionRecord, VfsLayer, VfsReadRecord,
};
use crate::semantic_query::SemanticNodeId;

/// Construct an `AuditCaps` with every category set to a tight `cap`
/// — useful for tests that need to exercise truncation behaviour at
/// small synthetic sizes.
fn tight_caps(cap: usize) -> AuditCaps {
    AuditCaps {
        structured_events: Some(cap),
        derivation_nodes: Some(cap),
        derivation_edges: Some(cap),
        vfs_reads: Some(cap),
        indexed_ready_builds: Some(cap),
        materializations: Some(cap),
        instantiations: Some(cap),
        substitutions: Some(cap),
        projections: Some(cap),
        conditional_decisions: Some(cap),
        alias_resolutions: Some(cap),
        shared_load_reuses: Some(cap),
    }
}

#[test]
fn accumulator_caps_structured_events_at_configured_limit() {
    const CAP: usize = 8;
    const EXCESS: u64 = 5;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_structured_event(StructuredAuditEvent::RequestStart {
            canonical_id: Arc::from(format!("/c{i}.vue").as_str()),
            request_id: i,
        });
    }

    let state = acc.drain();
    assert_eq!(
        state.structured_events.len(),
        CAP,
        "accumulator must cap structured_events at CAP (={CAP}); pre-fix \
         it accumulated all {} pushes unbounded",
        CAP + EXCESS as usize
    );
    assert_eq!(
        state.truncation_counters.structured_events_truncated, EXCESS,
        "structured_events_truncated must equal the dropped count \
         (={EXCESS}); pre-fix the counter did not exist"
    );
}

#[test]
fn accumulator_caps_vfs_reads_at_configured_limit() {
    const CAP: usize = 3;
    const EXCESS: u64 = 4;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_vfs_read(VfsReadRecord {
            canonical_id: Arc::from(format!("/r{i}.ts").as_str()),
            layer: VfsLayer::Disk,
            cache_hit: false,
            bytes_read: i,
            request_id: i,
        });
    }

    let state = acc.drain();
    assert_eq!(state.vfs_reads.len(), CAP);
    assert_eq!(state.truncation_counters.vfs_reads_truncated, EXCESS);
}

#[test]
fn accumulator_caps_indexed_ready_builds_at_configured_limit() {
    const CAP: usize = 2;
    const EXCESS: u64 = 6;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_indexed_ready_build(IndexedReadyBuildRecord {
            canonical_id: Arc::from(format!("/b{i}.ts").as_str()),
            whole_hash: [i as u8; 16],
        });
    }

    let state = acc.drain();
    assert_eq!(state.indexed_ready_builds.len(), CAP);
    assert_eq!(
        state.truncation_counters.indexed_ready_builds_truncated,
        EXCESS
    );
}

#[test]
fn accumulator_caps_materializations_at_configured_limit() {
    const CAP: usize = 5;
    const EXCESS: u64 = 3;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_materialization(MaterializationRecord {
            subject: MaterializationSubject::FallthroughInheritance {
                owner: Arc::from(format!("/m{i}.vue").as_str()),
            },
            duration_ms: 0.0,
        });
    }

    let state = acc.drain();
    assert_eq!(state.materializations.len(), CAP);
    assert_eq!(state.truncation_counters.materializations_truncated, EXCESS);
}

#[test]
fn accumulator_caps_instantiations_at_configured_limit() {
    const CAP: usize = 4;
    const EXCESS: u64 = 7;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_instantiation(InstantiationRecord {
            result: verter_audit::NodeId(i as u32),
            decl_canonical_id: Arc::from("/decl.ts"),
            decl_symbol_name: Arc::from("T"),
            args_fingerprint: [0u8; 16],
            args: Vec::new(),
        });
    }

    let state = acc.drain();
    assert_eq!(state.instantiations.len(), CAP);
    assert_eq!(state.truncation_counters.instantiations_truncated, EXCESS);
}

#[test]
fn accumulator_caps_substitutions_at_configured_limit() {
    const CAP: usize = 6;
    const EXCESS: u64 = 2;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_substitution(SubstitutionRecord {
            result: verter_audit::NodeId(i as u32),
            param_name: Arc::from(format!("T{i}").as_str()),
            substituted_with: verter_audit::NodeId(0),
        });
    }

    let state = acc.drain();
    assert_eq!(state.substitutions.len(), CAP);
    assert_eq!(state.truncation_counters.substitutions_truncated, EXCESS);
}

#[test]
fn accumulator_caps_projections_at_configured_limit() {
    const CAP: usize = 2;
    const EXCESS: u64 = 4;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_projection(ProjectionRecord {
            result: verter_audit::NodeId(i as u32),
            base: verter_audit::NodeId(0),
            path: Vec::new(),
        });
    }

    let state = acc.drain();
    assert_eq!(state.projections.len(), CAP);
    assert_eq!(state.truncation_counters.projections_truncated, EXCESS);
}

#[test]
fn accumulator_caps_conditional_decisions_at_configured_limit() {
    const CAP: usize = 3;
    const EXCESS: u64 = 8;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_conditional(ConditionalRecord {
            result: verter_audit::NodeId(i as u32),
            branch: verter_audit::ConditionalBranch::True,
        });
    }

    let state = acc.drain();
    assert_eq!(state.conditional_decisions.len(), CAP);
    assert_eq!(
        state.truncation_counters.conditional_decisions_truncated,
        EXCESS
    );
}

#[test]
fn accumulator_caps_alias_resolutions_at_configured_limit() {
    const CAP: usize = 4;
    const EXCESS: u64 = 5;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_alias_resolution(AliasResolveRecord {
            result: verter_audit::NodeId(i as u32),
            alias_name: Arc::from(format!("a{i}").as_str()),
        });
    }

    let state = acc.drain();
    assert_eq!(state.alias_resolutions.len(), CAP);
    assert_eq!(
        state.truncation_counters.alias_resolutions_truncated,
        EXCESS
    );
}

#[test]
fn accumulator_caps_shared_load_reuses_at_configured_limit() {
    const CAP: usize = 5;
    const EXCESS: u64 = 3;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        acc.push_shared_load_reuse(Arc::from(format!("/s{i}.ts").as_str()), i, false);
    }

    let state = acc.drain();
    assert_eq!(state.shared_load_reuses.len(), CAP);
    assert_eq!(
        state.truncation_counters.shared_load_reuses_truncated,
        EXCESS
    );
}

#[test]
fn accumulator_caps_derivation_edges_at_configured_limit() {
    use crate::semantic_query::{OriginEdge, OriginEdgeKind, OriginMeta};

    const CAP: usize = 3;
    const EXCESS: u64 = 5;
    let acc = RequestFootprintAccumulator::with_caps(tight_caps(CAP));

    for i in 0..(CAP as u64 + EXCESS) {
        let sources: Arc<[SemanticNodeId]> = vec![SemanticNodeId(i + 100)].into();
        let edge = OriginEdge {
            sources,
            meta: OriginMeta::None,
            edge_dep_signature: Arc::new(
                Arc::<[(Arc<str>, crate::semantic_query::DepVersion)]>::from(Vec::<(
                    Arc<str>,
                    crate::semantic_query::DepVersion,
                )>::new(
                )),
            ),
        };
        acc.push_derivation_edge(SemanticNodeId(i), OriginEdgeKind::AliasResolve, edge);
    }

    let state = acc.drain();
    assert_eq!(state.derivation_edges_raw.len(), CAP);
    assert_eq!(
        state.truncation_counters.derivation_edges_raw_truncated,
        EXCESS
    );
}

/// Default `AuditCaps` (10_000 per category) must NOT truncate
/// typical request volumes. Asserts that pushing well under the
/// default cap does not increment any truncation counter.
#[test]
fn default_caps_do_not_truncate_under_typical_volumes() {
    let acc = RequestFootprintAccumulator::new();
    for i in 0..50u64 {
        acc.push_structured_event(StructuredAuditEvent::RequestStart {
            canonical_id: Arc::from(format!("/c{i}.vue").as_str()),
            request_id: i,
        });
    }
    let state = acc.drain();
    assert_eq!(state.structured_events.len(), 50);
    assert_eq!(state.truncation_counters.structured_events_truncated, 0);
}
