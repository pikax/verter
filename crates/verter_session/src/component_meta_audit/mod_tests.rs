//! Inline tests for [`crate::component_meta_audit`]. Migrated out of
//! `mod.rs` to keep the production module focused on session-wiring
//! glue once the substrate move landed.

use std::sync::Arc;

use super::{
    AuditBuilder, AuditPhase, ComponentMetaPayload, DerivationEdgeRecord, DerivationSubgraph,
    IndexedReadyBuildRecord, NamedIdentity, NodeId, NodeRecord, OriginEdgeKind, OriginEdgeMetaDto,
    RequestAuditRecord, RequestFootprintAudit, RequestKind, RequestKindPayload, SemanticNodeKind,
    SharedLoadReuseRecord, VfsLayer, VfsReadRecord,
};
use crate::types::Hash16;

#[test]
fn audit_builder_captures_total_timing() {
    let builder = AuditBuilder::new(1, "test.vue".into());
    std::thread::sleep(std::time::Duration::from_millis(5));
    let record = builder.finish();
    assert!(record.timings.total_ms >= 4.0);
    assert_eq!(record.request_id, 1);
    assert_eq!(record.canonical_id, "test.vue");
    assert!(record.footprint.is_none());
}

#[test]
fn audit_builder_records_solver_steps() {
    let mut builder = AuditBuilder::new(2, "component.vue".into());
    builder.record_solver_steps(42);
    builder.record_solver_steps(100);
    let record = builder.finish();
    let cm = record
        .component_meta_payload()
        .expect("component-meta record must carry a component-meta payload");
    assert_eq!(cm.total_resolve_steps, 142);
    assert_eq!(cm.solve_count, 2);
}

#[test]
fn audit_builder_captures_phase_timings() {
    let mut builder = AuditBuilder::new(3, "phased.vue".into());
    std::thread::sleep(std::time::Duration::from_millis(2));
    builder.end_phase(AuditPhase::CaptureInputs);
    std::thread::sleep(std::time::Duration::from_millis(2));
    builder.end_phase(AuditPhase::Solver);
    let record = builder.finish();
    assert!(record.timings.capture_inputs_ms >= 1.0);
    assert!(record.timings.solver_ms >= 1.0);
    assert_eq!(record.timings.store_read_ms, 0.0);
}

#[test]
fn audit_default_host_config_is_off() {
    let config = crate::HostConfig::default();
    assert!(!config.audit_enabled);
}

#[test]
fn hash16_available_in_component_meta_audit_via_crate_types() {
    let h: Hash16 = [7u8; 16];
    let record = IndexedReadyBuildRecord {
        canonical_id: Arc::from("/a.ts"),
        whole_hash: h,
    };
    assert_eq!(record.whole_hash[0], 7);
}

#[test]
fn derivation_subgraph_serde_round_trips_nodes_and_edges_preserving_node_ids() {
    let graph = DerivationSubgraph {
        nodes: vec![
            NodeRecord {
                kind: SemanticNodeKind::DeclAnchor,
                named_identity: Some(NamedIdentity {
                    canonical_id: Arc::from("/x.ts"),
                    symbol_name: Arc::from("Foo"),
                    args_fingerprint: [0u8; 16],
                }),
                structural_hash: [1u8; 16],
                display_label: Arc::from("Foo"),
            },
            NodeRecord {
                kind: SemanticNodeKind::Primitive,
                named_identity: None,
                structural_hash: [2u8; 16],
                display_label: Arc::from("string"),
            },
        ],
        edges: vec![DerivationEdgeRecord {
            result: NodeId(1),
            kind: OriginEdgeKind::ProjectMember,
            sources: vec![NodeId(0)],
            meta: OriginEdgeMetaDto::ProjectMember {
                member_name: Arc::from("foo"),
                provenance: verter_audit::MemberEdgeProvenance::PathProjection,
            },
        }],
    };
    let json = serde_json::to_string(&graph).unwrap();
    let back: DerivationSubgraph = serde_json::from_str(&json).unwrap();
    assert_eq!(back.nodes.len(), 2);
    assert_eq!(back.edges.len(), 1);
    assert_eq!(back.edges[0].result, NodeId(1));
    assert_eq!(back.edges[0].sources[0], NodeId(0));
}

#[test]
fn node_id_stable_within_footprint_across_serialization_roundtrip() {
    let fp = RequestFootprintAudit {
        derivation_subgraph: DerivationSubgraph {
            nodes: vec![NodeRecord {
                kind: SemanticNodeKind::Alias,
                named_identity: None,
                structural_hash: [3u8; 16],
                display_label: Arc::from("Alias"),
            }],
            edges: vec![],
        },
        ..Default::default()
    };
    let json = serde_json::to_string(&fp).unwrap();
    let back: RequestFootprintAudit = serde_json::from_str(&json).unwrap();
    assert_eq!(back.derivation_subgraph.nodes.len(), 1);
    assert!(matches!(
        back.derivation_subgraph.nodes[0].kind,
        SemanticNodeKind::Alias
    ));
}

#[test]
fn semantic_node_kind_non_exhaustive_with_other_variant_accepts_unknown_names() {
    let k = SemanticNodeKind::Other {
        name: Arc::from("UnknownFutureVariant"),
    };
    let json = serde_json::to_string(&k).unwrap();
    let back: SemanticNodeKind = serde_json::from_str(&json).unwrap();
    match back {
        SemanticNodeKind::Other { name } => {
            assert_eq!(name.as_ref(), "UnknownFutureVariant");
        }
        other => panic!("expected Other, got {other:?}"),
    }
}

#[test]
fn loaded_files_unions_vfs_reads_and_shared_load_reuses() {
    let fp = RequestFootprintAudit {
        vfs_reads: vec![
            VfsReadRecord {
                canonical_id: Arc::from("/b.ts"),
                layer: VfsLayer::Overlay,
                cache_hit: true,
                bytes_read: 10,
                request_id: 1,
            },
            VfsReadRecord {
                canonical_id: Arc::from("/a.ts"),
                layer: VfsLayer::Disk,
                cache_hit: false,
                bytes_read: 20,
                request_id: 1,
            },
        ],
        shared_load_reuses: vec![
            SharedLoadReuseRecord {
                canonical_id: Arc::from("/c.ts"),
                winner_request_id: 1,
                winner_audited: true,
            },
            // Dup to prove dedup.
            SharedLoadReuseRecord {
                canonical_id: Arc::from("/a.ts"),
                winner_request_id: 2,
                winner_audited: false,
            },
        ],
        ..Default::default()
    };
    let files = fp.loaded_files();
    assert_eq!(
        files.iter().map(|s| s.as_ref()).collect::<Vec<_>>(),
        vec!["/a.ts", "/b.ts", "/c.ts"]
    );
}

#[test]
fn finish_populates_component_meta_kind_and_payload() {
    let builder = AuditBuilder::new(99, "/probe.vue".into());
    let record = builder.finish();
    assert_eq!(record.kind, RequestKind::ComponentMeta);
    match record.kind_payload {
        RequestKindPayload::ComponentMeta(_) => {}
        other => panic!("expected ComponentMeta payload, got {other:?}"),
    }
}

#[test]
fn component_meta_payload_accessor_returns_none_for_other_kinds() {
    let record = RequestAuditRecord {
        request_id: 1,
        canonical_id: String::new(),
        kind: RequestKind::TypeResolution,
        parent_request_id: None,
        from_cache: false,
        timings: Default::default(),
        memory: Default::default(),
        store: Default::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::TypeResolution(Default::default()),
        trace_id: String::new(),
    };
    assert!(record.component_meta_payload().is_none());
    assert!(record.type_resolution_payload().is_some());
}

// -----------------------------------------------------------------
// Audit-counter loss probe + smallest reproducer (D80 permanent
// regression smoke).
//
// Drives a real cold-resolver call through `MetaProject` and
// snapshots which `ComponentMetaPayload` materializer counters report
// 0 vs > 0 across a representative single-file resolution. The probe
// documents the EXPECTED state: every counter wired to a production
// code path that runs in the cold-resolver flow must increment.
// -----------------------------------------------------------------

/// Drive a small SFC + dependency through the cold resolver and
/// return the published `RequestAuditRecord`. Fixture exercises:
///   - cross-file imported `interface` (forces `imported_root_proof`),
///   - multiple props (forces `materialize_structure_calls`),
///   - a DIAMOND import graph — `Props` references two imported types
///     (`FromA`, `FromB`) declared in two separate modules that BOTH
///     re-export their declaration through the SAME shared base module
///     (`/base.ts`). The shared base is therefore a route participant
///     reached by two distinct import routes, so its whole-hash fact is
///     observed/merged into the completion fence twice — the second
///     merge is a redundant `(canonical, kind)` insert at the same
///     version (the production `dep_signature_intern_hits` "intern
///     hit"). The diamond makes the intern-hit a property of the import
///     graph itself, independent of any single cache's self-rooting.
///   - which together force the substrate to: intern semantic
///     nodes (NodeArena push_impl shard locks), walk origins
///     under the completion fence (dep_signature merges), and
///     re-merge an already-observed origin (intern hits).
fn run_probe_request() -> RequestAuditRecord {
    let host = crate::VerterHost::new_standalone(crate::types::HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        footprint_capture: true,
        ..crate::types::HostConfig::default()
    });
    let project = crate::meta::MetaProject::new(host);
    // Shared base of the diamond — both `/a.ts` and `/b.ts` re-export
    // their declaration through this module, so it is a route
    // participant reached by two distinct routes.
    project
        .upsert_base(
            "/base.ts",
            r#"export interface FromA {
  fromAMessage: string;
}
export interface FromB {
  fromBLevel: number;
}"#,
        )
        .unwrap();
    // Left arm of the diamond — re-exports `FromA` from the shared base.
    project
        .upsert_base("/a.ts", "export { FromA } from './base'\n")
        .unwrap();
    // Right arm of the diamond — re-exports `FromB` from the shared base.
    project
        .upsert_base("/b.ts", "export { FromB } from './base'\n")
        .unwrap();
    // `Props` pulls one type from each arm of the diamond, so resolving
    // it walks both routes — and both converge on `/base.ts`.
    project
        .upsert_base(
            "/types.ts",
            r#"import type { FromA } from './a'
import type { FromB } from './b'
export interface Props {
  fromA: FromA;
  fromB: FromB;
  optional?: boolean;
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("resolver must produce metadata for the probe fixture");
    host.take_audit_record(resolution.request_id)
        .expect("audit record must publish for the probe fixture")
}

/// Characterization probe (D80 permanent regression smoke). The
/// probe documents per-counter status for the cold-resolver path.
/// Should any of the wired counters silently drop back to 0, this
/// test regresses with a per-counter summary in the failure message.
#[test]
fn audit_counter_loss_reproduction() {
    let record = run_probe_request();
    let cm: &ComponentMetaPayload = record
        .component_meta_payload()
        .expect("probe fixture must publish a component-meta payload");

    let observed: Vec<(&'static str, u64)> = vec![
        (
            "node_arena_lock_acquisitions",
            cm.node_arena_lock_acquisitions,
        ),
        ("dep_signature_merges", cm.dep_signature_merges),
        ("dep_signature_intern_hits", cm.dep_signature_intern_hits),
    ];

    let zero: Vec<&'static str> = observed
        .iter()
        .filter_map(|(name, value)| (*value == 0).then_some(*name))
        .collect();

    assert!(
        zero.is_empty(),
        "audit_counter_loss_reproduction (D80 permanent smoke): the following \
         ComponentMetaPayload counters silently regressed back to 0 on a non-trivial \
         cold-resolver request — every counter listed below is wired to a \
         production code path exercised by the probe fixture and MUST report \
         > 0. Zero counters: {zero:?}. Observed values: {observed:?}.",
    );
}

/// Smallest reproducer (DISCRIMINATING). A minimal SFC + dep fixture
/// that exercises the substrate work expected to bump each of the
/// three previously-zero counters.
#[test]
fn audit_counter_smallest_reproducer() {
    let record = run_probe_request();
    let cm: &ComponentMetaPayload = record
        .component_meta_payload()
        .expect("probe fixture must publish a component-meta payload");

    assert!(
        cm.node_arena_lock_acquisitions > 0,
        "smallest reproducer: NodeArena shard-lock acquisitions must \
         increment on the cold-resolver path. Production `push_impl` \
         acquires a shard mutex on every interned semantic node — \
         observing 0 means the audit hook is no longer wired into the \
         production hot path. Counter: {}",
        cm.node_arena_lock_acquisitions,
    );
    assert!(
        cm.dep_signature_merges > 0,
        "smallest reproducer: dep_signature merges must increment when \
         the cold resolver folds a cached read's dep-signature into the \
         materialiser's per-frame local fence. Production \
         `merge_dep_signature_into_local_fence` bumps the counter at \
         every such fold — observing 0 means the audit hook is no longer \
         wired into the production merge site. Counter: {}",
        cm.dep_signature_merges,
    );
    assert!(
        cm.dep_signature_intern_hits > 0,
        "smallest reproducer: dep_signature intern-hits must increment \
         when `merge_signature` observes a `(canonical, kind)` pair \
         already present at the same version (redundant merge avoided). \
         Counter: {}",
        cm.dep_signature_intern_hits,
    );
}

/// Discriminating: the cold-path attribution sheet at
/// `crates/verter_session/tests/perf_bounds/cold-path-attribution-baseline.md`
/// must (a) identify a dominant cost arm per fixture and (b) record
/// the bridge max-depth column.
#[test]
fn chat_messages_attribution_sheet_has_dominant_cost_arm_and_bridge_max_depth_recorded() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set during cargo test");
    let path = std::path::Path::new(&manifest_dir)
        .join("tests")
        .join("perf_bounds")
        .join("cold-path-attribution-baseline.md");
    assert!(
        path.is_file(),
        "cold-path attribution sheet must exist at `{}`. Without this file the \
         corpus-wide cost attribution is not captured.",
        path.display(),
    );
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read attribution sheet at {}: {}", path.display(), e));

    assert!(
        body.contains("materialize_ms")
            || body.contains("dominant phase")
            || body.contains("dominant cost arm"),
        "attribution sheet must identify a dominant cost arm (e.g., \
         `materialize_ms` / `dominant cost arm`). Sheet contents \
         do not include those markers.",
    );
    assert!(
        body.contains("bridge max depth") || body.contains("bridge_max_depth_observed"),
        "attribution sheet must record the `bridge max depth` column. Sheet does not \
         include the column header.",
    );
    assert!(
        body.contains("bridge worst batch") || body.contains("bridge_worst_batch"),
        "attribution sheet must record the `bridge worst batch` column. Sheet does \
         not include the column header.",
    );
    assert!(
        body.contains("chat-components")
            || body.contains("ChatMessage")
            || body.contains("00b-deferred-baselines"),
        "attribution sheet must reference the chat-components deferred-baselines doc \
         OR name a chat fixture.",
    );
}

/// Drive a small SFC fixture exercising every macro-projector
/// publication boundary (`defineProps` / `defineEmits` / `defineSlots`
/// / `defineExpose` / `defineModel`) and return its
/// `RequestAuditRecord`.
fn run_macro_surface_probe_request() -> RequestAuditRecord {
    let host = crate::VerterHost::new_standalone(crate::types::HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        footprint_capture: true,
        ..crate::types::HostConfig::default()
    });
    let project = crate::meta::MetaProject::new(host);
    project
        .upsert_base(
            "/Surfaces.vue",
            r#"<script setup lang="ts">
defineProps<{ foo: string; bar: number }>()
defineEmits<{ baz: [string]; qux: [] }>()
defineSlots<{ default(): unknown; named(): unknown }>()
defineExpose<{ expoA: number; expoB: string }>()
defineModel<string>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution("/Surfaces.vue")
        .expect("resolver must produce metadata for the surfaces fixture");
    host.take_audit_record(resolution.request_id)
        .expect("audit record must publish for the surfaces fixture")
}

/// PublishedField non-vacuity gate (DISCRIMINATING).
///
/// Round 17 closed the Rule-5 validator's false-negative class
/// (`PublishedField` edges crossing the `fp.projections` lift), but
/// the validator's `PublishedField` discriminating branch was dead
/// code on real corpus data: zero production sites emitted
/// `MemberEdgeProvenance::PublishedField`. 179/179 corpus PASS was
/// vacuous via the structural-allowlist short-circuit.
///
/// This test pins the producer-side PublishedField wiring.
/// Every macro projector (`defineProps` / `defineEmits` /
/// `defineSlots` / `defineExpose` / `defineModel`) MUST emit one
/// `ProjectMember` origin edge with `MemberEdgeProvenance::PublishedField`
/// for every published member name. Pre-fix the test FAILS with
/// `published_field_edges` empty; post-fix it PASSES with one edge
/// per declared surface field.
#[test]
fn audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries() {
    let record = run_macro_surface_probe_request();
    let footprint = record
        .footprint
        .as_ref()
        .expect("footprint must publish when HostConfig::footprint_capture is enabled");

    let mut published_field_names: Vec<String> = footprint
        .derivation_subgraph
        .edges
        .iter()
        .filter(|e| e.kind == OriginEdgeKind::ProjectMember)
        .filter_map(|e| match &e.meta {
            OriginEdgeMetaDto::ProjectMember {
                member_name,
                provenance,
            } if *provenance == verter_audit::MemberEdgeProvenance::PublishedField => {
                Some(member_name.as_ref().to_string())
            }
            _ => None,
        })
        .collect();
    published_field_names.sort();
    published_field_names.dedup();

    // Every declared surface field across the five macro projectors
    // MUST appear as a `PublishedField` origin edge.
    let expected: &[&str] = &[
        "foo",        // defineProps
        "bar",        // defineProps
        "baz",        // defineEmits
        "qux",        // defineEmits
        "default",    // defineSlots
        "named",      // defineSlots
        "expoA",      // defineExpose
        "expoB",      // defineExpose
        "modelValue", // defineModel (default name)
    ];

    let missing: Vec<&&str> = expected
        .iter()
        .filter(|name| !published_field_names.iter().any(|seen| seen == **name))
        .collect();

    assert!(
        missing.is_empty(),
        "PublishedField non-vacuity gate: every macro projector \
         publication boundary MUST emit a `ProjectMember` origin edge \
         tagged `MemberEdgeProvenance::PublishedField` for every \
         declared surface field. Missing names: {missing:?}. Observed \
         PublishedField edge names: {published_field_names:?}. If this \
         test fails post-Block-6.j R18, a macro projector was added \
         WITHOUT wiring `dispatch.record_published_field_edge(...)` at \
         the publish boundary — see `crates/verter_session/src/\
         meta_resolve/projectors/{{props,emits,slots,exposed,model}}.rs`.",
    );

    // Discriminator: the validator branch must be live. Assert at
    // least one PublishedField edge was observed on this fixture.
    assert!(
        !published_field_names.is_empty(),
        "PublishedField origin edges must be emitted at the macro \
         publication boundary. Zero observed — the Rule-5 validator's \
         `PublishedField` discriminating branch is dead code, \
         reverting the PublishedField wiring.",
    );
}
