//! Byte-exact `Display` snapshots for every variant of
//! [`StructuredAuditEvent`](super::StructuredAuditEvent).
//!
//! / §3.A. The
//! `structured_event_display_snapshot_byte_exact_for_every_variant`
//! test in this module constructs a canonical instance of each
//! variant, invokes `format!("{}", ev)`, and asserts the output
//! equals the corresponding `EXPECTED_*` constant here — verbatim.
//!
//! A drift in the `Display` impl OR a new enum variant without a
//! snapshot fails the test. The format is intentionally not a
//! legacy carry-over; it is authored fresh ( — legacy
//! `format!("k=v")` stderr format is gone).

use std::sync::Arc;

use super::{
    CacheOutcomeKind, DispatchKeyKind, MaterializationScopeAudit, MaterializationSubject,
    MaterializeSkipReason, ProjectionModeAudit, StructuredAuditEvent as Event, VfsLayer,
};
use crate::types::Hash16;
use verter_audit::AugmentationTargetKindTag;

// ──────────────────────────────────────────────────────────────────
// Expected Display strings, authored to match the hand-written
// `Display` impl in `structured_event.rs` exactly.
// ──────────────────────────────────────────────────────────────────

pub const EXPECTED_REQUEST_START: &str = "RequestStart(/c.vue, #42)";
pub const EXPECTED_REQUEST_END_SUCCESS: &str = "RequestEnd(#42, success=true)";
pub const EXPECTED_REQUEST_END_FAILURE: &str = "RequestEnd(#42, success=false)";
pub const EXPECTED_INDEXED_READY_BUILT: &str = "IndexedReadyBuilt(/a.ts, hash=01020304)";
pub const EXPECTED_VFS_READ_OVERLAY_HIT: &str = "VfsRead(/a.ts, Overlay, hit=true, bytes=123)";
pub const EXPECTED_VFS_READ_MISS: &str = "VfsRead(/missing.ts, Missing, hit=false, bytes=0)";
pub const EXPECTED_SHARED_LOAD_REUSE_AUDITED: &str =
    "SharedLoadReuse(/b.ts, winner=#7, audited=true)";
pub const EXPECTED_SHARED_LOAD_REUSE_UNAUDITED: &str =
    "SharedLoadReuse(/b.ts, winner=#7, audited=false)";
pub const EXPECTED_DISPATCH_ENTER: &str = "DispatchEnter(ResolveDecl, depth=3)";
pub const EXPECTED_DISPATCH_EXIT: &str = "DispatchExit(ResolveDecl, Hit, 1500ns)";
pub const EXPECTED_MATERIALIZE_MEMBER_ROUTE_START: &str =
    "MaterializeMemberRouteStart(MemberRoute { owner: \"/x.vue\", member: \"foo\" })";
pub const EXPECTED_MATERIALIZE_MEMBER_ROUTE_END: &str =
    "MaterializeMemberRouteEnd(MemberRoute { owner: \"/x.vue\", member: \"foo\" }, 2500ns)";
pub const EXPECTED_REMATERIALIZE_PUBLIC_PROP_TYPE_START: &str =
    "RematerializePublicPropTypeStart(PublicPropType { owner: \"/x.vue\", prop: \"value\" })";
pub const EXPECTED_REMATERIALIZE_PUBLIC_PROP_TYPE_END: &str =
    "RematerializePublicPropTypeEnd(PublicPropType { owner: \"/x.vue\", prop: \"value\" }, 3750ns)";
pub const EXPECTED_MATERIALIZE_DEFINE_PROPS_MEMBER: &str =
    "MaterializeDefinePropsMember(DefinePropsMember { owner: \"/x.vue\", member: \"label\" })";
pub const EXPECTED_FALLTHROUGH_INHERITANCE_COMPUTED: &str =
    "FallthroughInheritanceComputed(FallthroughInheritance { owner: \"/x.vue\" })";
pub const EXPECTED_RESOLVE_IMPORTED_TYPE_ROOT: &str = "ResolveImportedTypeRoot(/types.ts::Props)";
pub const EXPECTED_CURRENT_EVAL_STATE: &str = "CurrentEvalState(/a.ts, 999ns)";
pub const EXPECTED_MATERIALIZE_STRUCTURE_ENTER: &str =
    "MaterializeStructureEnter(Object#7, TopLevel, Expanded, depth=1)";
pub const EXPECTED_MATERIALIZE_STRUCTURE_EXIT: &str =
    "MaterializeStructureExit(Object#7, TopLevel, Expanded, Hit, 1234ns)";
pub const EXPECTED_MATERIALIZE_STRUCTURE_POLICY_SKIP: &str =
    "MaterializeStructurePolicySkip(Object#7, Nested, FunctionPropertyAtNested)";
pub const EXPECTED_MATERIALIZE_STRUCTURE_CYCLE_DETECTED: &str =
    "MaterializeStructureCycleDetected(Object#7, Nested, Expanded, depth=3)";
pub const EXPECTED_MATERIALIZE_STRUCTURE_DEPTH_FUSE_TRIPPED: &str =
    "MaterializeStructureDepthFuseTripped(Object#7, Nested, Expanded, depth=4096)";
pub const EXPECTED_CUSTOM: &str = "Custom(test_name, key=value)";
pub const EXPECTED_CACHE_DRAINED_AT_UPSERT: &str =
    "CacheDrainedAtUpsert(resolved_type_cache, /probe.vue)";
pub const EXPECTED_FACT_SIGNATURE_OVERFLOW: &str = "FactSignatureOverflow(size=1100, cap=1024)";
pub const EXPECTED_FACT_SIGNATURE_ADMISSION_REFUSED: &str =
    "FactSignatureAdmissionRefused(materialize_structure, EmptySignature)";
pub const EXPECTED_MODULE_AUGMENTATION_STITCHED_EXTERNAL: &str =
    "ModuleAugmentationStitched(ext=vue, n=2, fp=01020304)";
pub const EXPECTED_MODULE_AUGMENTATION_INDEX_SHAPE_INSTALL: &str =
    "ModuleAugmentationIndexShape(ext=vue, install=05060708, n=1)";
pub const EXPECTED_MODULE_AUGMENTATION_INDEX_SHAPE_REFRESH: &str =
    "ModuleAugmentationIndexShape(ext=vue, prev=05060708, new=090a0b0c, n=2)";

pub const EXPECTED_FILE_ARTIFACT_CACHE_ADMIT: &str =
    "FileArtifactCache(/w/a.ts, Admit, ch=01020304, pe=05060708, n=1)";

pub const EXPECTED_FILE_ARTIFACT_CACHE_EVICT: &str =
    "FileArtifactCache(/w/a.ts, Evict, ch=01020304, pe=05060708, n=0)";

pub const EXPECTED_FACT_REGISTRY_WRITE: &str =
    "FactRegistryWrite(/w/a.ts, Export, Semantic, sem=01020304, disp=05060708)";

pub const EXPECTED_FACT_VALIDATION_SUMMARY: &str =
    "FactValidationSummary(#7, materialize_structure, n=100, warm=80, stale=15, archive=5)";

pub const EXPECTED_EXPORT_ROUTE_RESOLVED_AUGMENTED: &str =
    "ExportRouteResolved(/w/providers/index.ts::Foo -> /w/lib.ts::Foo, augmented=true)";

pub const EXPECTED_EXPORT_ROUTE_RESOLVED_PLAIN: &str =
    "ExportRouteResolved(/w/providers/index.ts::Foo -> /w/lib.ts::Foo, augmented=false)";

pub const EXPECTED_COMPILE_MODE_DOWNGRADE: &str =
    "CompileModeDowngrade(Content -> Stateless, reasons=[HasMacroTypeDeps])";

pub const EXPECTED_TYPEINFO_GRAPH_PUBLISHED: &str =
    "TypeInfoGraphPublished(typeinfo_graph_session, ResolveSymbol, nodes=5, roots=1, closure=OneLevel)";

pub const EXPECTED_TYPEINFO_GRAPH_DEGRADED: &str =
    "TypeInfoGraphDegraded(typeinfo_graph_session, ProjectPath, reason=BudgetExceededNodes, nodes=3)";

pub const EXPECTED_TYPEINFO_GRAPH_CACHE_HIT: &str =
    "TypeInfoGraphCacheHit(typeinfo_graph_session, EvaluateExpression)";

// ──────────────────────────────────────────────────────────────────
// Fixture constructors — exactly one canonical instance per variant.
// The DISPLAY_SNAPSHOTS table pairs each fixture with its expected
// string so the byte-exact test can iterate over every variant.
// ──────────────────────────────────────────────────────────────────

pub fn fixture_request_start() -> Event {
    Event::RequestStart {
        canonical_id: Arc::from("/c.vue"),
        request_id: 42,
    }
}

pub fn fixture_request_end_success() -> Event {
    Event::RequestEnd {
        request_id: 42,
        success: true,
    }
}

pub fn fixture_request_end_failure() -> Event {
    Event::RequestEnd {
        request_id: 42,
        success: false,
    }
}

pub fn fixture_indexed_ready_built() -> Event {
    // First four bytes `01 02 03 04` → short_hash yields "01020304".
    let mut h: Hash16 = [0u8; 16];
    h[0] = 1;
    h[1] = 2;
    h[2] = 3;
    h[3] = 4;
    Event::IndexedReadyBuilt {
        canonical_id: Arc::from("/a.ts"),
        whole_hash: h,
    }
}

pub fn fixture_vfs_read_overlay_hit() -> Event {
    Event::VfsRead {
        canonical_id: Arc::from("/a.ts"),
        layer: VfsLayer::Overlay,
        cache_hit: true,
        bytes_read: 123,
    }
}

pub fn fixture_vfs_read_miss() -> Event {
    Event::VfsRead {
        canonical_id: Arc::from("/missing.ts"),
        layer: VfsLayer::Missing,
        cache_hit: false,
        bytes_read: 0,
    }
}

pub fn fixture_shared_load_reuse_audited() -> Event {
    Event::SharedLoadReuse {
        canonical_id: Arc::from("/b.ts"),
        winner_request_id: 7,
        winner_audited: true,
    }
}

pub fn fixture_shared_load_reuse_unaudited() -> Event {
    Event::SharedLoadReuse {
        canonical_id: Arc::from("/b.ts"),
        winner_request_id: 7,
        winner_audited: false,
    }
}

pub fn fixture_dispatch_enter() -> Event {
    Event::DispatchEnter {
        key_kind: DispatchKeyKind::ResolveDecl,
        depth: 3,
    }
}

pub fn fixture_dispatch_exit() -> Event {
    Event::DispatchExit {
        key_kind: DispatchKeyKind::ResolveDecl,
        outcome: CacheOutcomeKind::Hit,
        duration_ns: 1500,
    }
}

pub fn fixture_materialize_member_route_start() -> Event {
    Event::MaterializeMemberRouteStart {
        subject: MaterializationSubject::MemberRoute {
            owner: Arc::from("/x.vue"),
            member: Arc::from("foo"),
        },
    }
}

pub fn fixture_materialize_member_route_end() -> Event {
    Event::MaterializeMemberRouteEnd {
        subject: MaterializationSubject::MemberRoute {
            owner: Arc::from("/x.vue"),
            member: Arc::from("foo"),
        },
        duration_ns: 2500,
    }
}

pub fn fixture_rematerialize_public_prop_type_start() -> Event {
    Event::RematerializePublicPropTypeStart {
        subject: MaterializationSubject::PublicPropType {
            owner: Arc::from("/x.vue"),
            prop: Arc::from("value"),
        },
    }
}

pub fn fixture_rematerialize_public_prop_type_end() -> Event {
    Event::RematerializePublicPropTypeEnd {
        subject: MaterializationSubject::PublicPropType {
            owner: Arc::from("/x.vue"),
            prop: Arc::from("value"),
        },
        duration_ns: 3750,
    }
}

pub fn fixture_materialize_define_props_member() -> Event {
    Event::MaterializeDefinePropsMember {
        subject: MaterializationSubject::DefinePropsMember {
            owner: Arc::from("/x.vue"),
            member: Arc::from("label"),
        },
    }
}

pub fn fixture_fallthrough_inheritance_computed() -> Event {
    Event::FallthroughInheritanceComputed {
        subject: MaterializationSubject::FallthroughInheritance {
            owner: Arc::from("/x.vue"),
        },
    }
}

pub fn fixture_resolve_imported_type_root() -> Event {
    Event::ResolveImportedTypeRoot {
        canonical_id: Arc::from("/types.ts"),
        symbol_name: Arc::from("Props"),
    }
}

pub fn fixture_current_eval_state() -> Event {
    Event::CurrentEvalState {
        canonical_id: Arc::from("/a.ts"),
        duration_ns: 999,
    }
}

pub fn fixture_materialize_structure_enter() -> Event {
    Event::MaterializeStructureEnter {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::TopLevel,
        mode: ProjectionModeAudit::Expanded,
        depth: 1,
    }
}

pub fn fixture_materialize_structure_exit() -> Event {
    Event::MaterializeStructureExit {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::TopLevel,
        mode: ProjectionModeAudit::Expanded,
        outcome: CacheOutcomeKind::Hit,
        duration_ns: 1234,
    }
}

pub fn fixture_materialize_structure_policy_skip() -> Event {
    Event::MaterializeStructurePolicySkip {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::Nested,
        reason: MaterializeSkipReason::FunctionPropertyAtNested,
    }
}

pub fn fixture_materialize_structure_cycle_detected() -> Event {
    Event::MaterializeStructureCycleDetected {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::Nested,
        mode: ProjectionModeAudit::Expanded,
        depth: 3,
    }
}

pub fn fixture_materialize_structure_depth_fuse_tripped() -> Event {
    Event::MaterializeStructureDepthFuseTripped {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::Nested,
        mode: ProjectionModeAudit::Expanded,
        depth: 4096,
    }
}

pub fn fixture_custom() -> Event {
    // Custom justified: canonical fixture for the Display snapshot test —
    // every variant needs coverage, including Custom.
    Event::Custom {
        name: Arc::from("test_name"),
        detail: Arc::from("key=value"),
    }
}

pub fn fixture_cache_drained_at_upsert() -> Event {
    Event::CacheDrainedAtUpsert {
        layer: Arc::from("resolved_type_cache"),
        canonical_id: Arc::from("/probe.vue"),
    }
}

pub fn fixture_fact_signature_overflow() -> Event {
    Event::FactSignatureOverflow {
        candidate_size: 1100,
        cap: 1024,
    }
}

pub fn fixture_fact_signature_admission_refused() -> Event {
    Event::FactSignatureAdmissionRefused {
        cache_kind: Arc::from("materialize_structure"),
        reason: verter_audit::AdmissionRefusalReason::EmptySignature,
    }
}

pub fn fixture_module_augmentation_stitched_external() -> Event {
    Event::ModuleAugmentationStitched {
        target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
        external_specifier: Some(Arc::from("vue")),
        resolved_relative_canonical: None,
        wildcard_pattern: None,
        augmenter_count: 2,
        fingerprint: [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    }
}

pub fn fixture_module_augmentation_index_shape_install() -> Event {
    Event::ModuleAugmentationIndexShape {
        target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
        external_specifier: Some(Arc::from("vue")),
        resolved_relative_canonical: None,
        wildcard_pattern: None,
        prev_fingerprint: None,
        new_fingerprint: [5, 6, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        augmenter_count: 1,
    }
}

pub fn fixture_module_augmentation_index_shape_refresh() -> Event {
    Event::ModuleAugmentationIndexShape {
        target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
        external_specifier: Some(Arc::from("vue")),
        resolved_relative_canonical: None,
        wildcard_pattern: None,
        prev_fingerprint: Some([5, 6, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        new_fingerprint: [9, 10, 11, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        augmenter_count: 2,
    }
}

pub fn fixture_file_artifact_cache_admit() -> Event {
    Event::FileArtifactCache {
        canonical_id: Arc::from("/w/a.ts"),
        action: verter_audit::FileArtifactCacheAction::Admit,
        content_hash: [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        parse_env_hash: [5, 6, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        entry_count_after: 1,
    }
}

pub fn fixture_file_artifact_cache_evict() -> Event {
    Event::FileArtifactCache {
        canonical_id: Arc::from("/w/a.ts"),
        action: verter_audit::FileArtifactCacheAction::Evict,
        content_hash: [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        parse_env_hash: [5, 6, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        entry_count_after: 0,
    }
}

pub fn fixture_fact_registry_write() -> Event {
    Event::FactRegistryWrite {
        canonical_id: Arc::from("/w/a.ts"),
        fact_key_kind: verter_audit::FactKeyKindTag::Export,
        lane: verter_audit::FactLaneTag::Semantic,
        semantic_hash: [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        display_hash: [5, 6, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    }
}

pub fn fixture_fact_validation_summary() -> Event {
    Event::FactValidationSummary {
        request_id: 7,
        cache_kind: Arc::from("materialize_structure"),
        validations_attempted: 100,
        warm_hits: 80,
        stale_misses: 15,
        archive_checks: 5,
    }
}

pub fn fixture_export_route_resolved_augmented() -> Event {
    Event::ExportRouteResolved {
        provider_canonical: Arc::from("/w/providers/index.ts"),
        exported_name: Arc::from("Foo"),
        resolved_canonical: Arc::from("/w/lib.ts"),
        resolved_source_name: Arc::from("Foo"),
        augmented: true,
    }
}

pub fn fixture_export_route_resolved_plain() -> Event {
    Event::ExportRouteResolved {
        provider_canonical: Arc::from("/w/providers/index.ts"),
        exported_name: Arc::from("Foo"),
        resolved_canonical: Arc::from("/w/lib.ts"),
        resolved_source_name: Arc::from("Foo"),
        augmented: false,
    }
}

pub fn fixture_compile_mode_downgrade() -> Event {
    Event::CompileModeDowngrade {
        requested: verter_audit::payloads::tags::CompileCacheModeTag::Content,
        actual: verter_audit::payloads::tags::CompileCacheModeTag::Stateless,
        reasons: vec![verter_audit::payloads::tags::DowngradeReasonTag::HasMacroTypeDeps],
    }
}

pub fn fixture_typeinfo_graph_published() -> Event {
    Event::TypeInfoGraphPublished {
        layer: Arc::from("typeinfo_graph_session"),
        operation: verter_audit::payloads::GraphOperationTag::ResolveSymbol,
        snapshot_node_count: 5,
        roots_count: 1,
        closure: verter_audit::payloads::GraphClosurePolicyTag::OneLevel,
    }
}

pub fn fixture_typeinfo_graph_degraded() -> Event {
    Event::TypeInfoGraphDegraded {
        layer: Arc::from("typeinfo_graph_session"),
        operation: verter_audit::payloads::GraphOperationTag::ProjectPath,
        reason: verter_audit::payloads::TypeInfoDegradationReasonTag::BudgetExceededNodes,
        snapshot_node_count: 3,
    }
}

pub fn fixture_typeinfo_graph_cache_hit() -> Event {
    Event::TypeInfoGraphCacheHit {
        layer: Arc::from("typeinfo_graph_session"),
        operation: verter_audit::payloads::GraphOperationTag::EvaluateExpression,
    }
}

/// Pair each fixture with its expected Display string. The
/// `structured_event_display_snapshot_byte_exact_for_every_variant`
/// test iterates this table, and the companion `all_variants_covered`
/// test reflects on `StructuredAuditEvent`'s match discriminants
/// to ensure no variant is missing.
pub fn all_snapshots() -> Vec<(Event, &'static str)> {
    vec![
        (fixture_request_start(), EXPECTED_REQUEST_START),
        (fixture_request_end_success(), EXPECTED_REQUEST_END_SUCCESS),
        (fixture_request_end_failure(), EXPECTED_REQUEST_END_FAILURE),
        (fixture_indexed_ready_built(), EXPECTED_INDEXED_READY_BUILT),
        (
            fixture_vfs_read_overlay_hit(),
            EXPECTED_VFS_READ_OVERLAY_HIT,
        ),
        (fixture_vfs_read_miss(), EXPECTED_VFS_READ_MISS),
        (
            fixture_shared_load_reuse_audited(),
            EXPECTED_SHARED_LOAD_REUSE_AUDITED,
        ),
        (
            fixture_shared_load_reuse_unaudited(),
            EXPECTED_SHARED_LOAD_REUSE_UNAUDITED,
        ),
        (fixture_dispatch_enter(), EXPECTED_DISPATCH_ENTER),
        (fixture_dispatch_exit(), EXPECTED_DISPATCH_EXIT),
        (
            fixture_materialize_member_route_start(),
            EXPECTED_MATERIALIZE_MEMBER_ROUTE_START,
        ),
        (
            fixture_materialize_member_route_end(),
            EXPECTED_MATERIALIZE_MEMBER_ROUTE_END,
        ),
        (
            fixture_rematerialize_public_prop_type_start(),
            EXPECTED_REMATERIALIZE_PUBLIC_PROP_TYPE_START,
        ),
        (
            fixture_rematerialize_public_prop_type_end(),
            EXPECTED_REMATERIALIZE_PUBLIC_PROP_TYPE_END,
        ),
        (
            fixture_materialize_define_props_member(),
            EXPECTED_MATERIALIZE_DEFINE_PROPS_MEMBER,
        ),
        (
            fixture_fallthrough_inheritance_computed(),
            EXPECTED_FALLTHROUGH_INHERITANCE_COMPUTED,
        ),
        (
            fixture_resolve_imported_type_root(),
            EXPECTED_RESOLVE_IMPORTED_TYPE_ROOT,
        ),
        (fixture_current_eval_state(), EXPECTED_CURRENT_EVAL_STATE),
        (
            fixture_materialize_structure_enter(),
            EXPECTED_MATERIALIZE_STRUCTURE_ENTER,
        ),
        (
            fixture_materialize_structure_exit(),
            EXPECTED_MATERIALIZE_STRUCTURE_EXIT,
        ),
        (
            fixture_materialize_structure_policy_skip(),
            EXPECTED_MATERIALIZE_STRUCTURE_POLICY_SKIP,
        ),
        (
            fixture_materialize_structure_cycle_detected(),
            EXPECTED_MATERIALIZE_STRUCTURE_CYCLE_DETECTED,
        ),
        (
            fixture_materialize_structure_depth_fuse_tripped(),
            EXPECTED_MATERIALIZE_STRUCTURE_DEPTH_FUSE_TRIPPED,
        ),
        (fixture_custom(), EXPECTED_CUSTOM),
        (
            fixture_cache_drained_at_upsert(),
            EXPECTED_CACHE_DRAINED_AT_UPSERT,
        ),
        (
            fixture_fact_signature_overflow(),
            EXPECTED_FACT_SIGNATURE_OVERFLOW,
        ),
        (
            fixture_fact_signature_admission_refused(),
            EXPECTED_FACT_SIGNATURE_ADMISSION_REFUSED,
        ),
        (
            fixture_module_augmentation_stitched_external(),
            EXPECTED_MODULE_AUGMENTATION_STITCHED_EXTERNAL,
        ),
        (
            fixture_module_augmentation_index_shape_install(),
            EXPECTED_MODULE_AUGMENTATION_INDEX_SHAPE_INSTALL,
        ),
        (
            fixture_module_augmentation_index_shape_refresh(),
            EXPECTED_MODULE_AUGMENTATION_INDEX_SHAPE_REFRESH,
        ),
        (
            fixture_file_artifact_cache_admit(),
            EXPECTED_FILE_ARTIFACT_CACHE_ADMIT,
        ),
        (
            fixture_file_artifact_cache_evict(),
            EXPECTED_FILE_ARTIFACT_CACHE_EVICT,
        ),
        (fixture_fact_registry_write(), EXPECTED_FACT_REGISTRY_WRITE),
        (
            fixture_fact_validation_summary(),
            EXPECTED_FACT_VALIDATION_SUMMARY,
        ),
        (
            fixture_export_route_resolved_augmented(),
            EXPECTED_EXPORT_ROUTE_RESOLVED_AUGMENTED,
        ),
        (
            fixture_export_route_resolved_plain(),
            EXPECTED_EXPORT_ROUTE_RESOLVED_PLAIN,
        ),
        (
            fixture_compile_mode_downgrade(),
            EXPECTED_COMPILE_MODE_DOWNGRADE,
        ),
        (
            fixture_typeinfo_graph_published(),
            EXPECTED_TYPEINFO_GRAPH_PUBLISHED,
        ),
        (
            fixture_typeinfo_graph_degraded(),
            EXPECTED_TYPEINFO_GRAPH_DEGRADED,
        ),
        (
            fixture_typeinfo_graph_cache_hit(),
            EXPECTED_TYPEINFO_GRAPH_CACHE_HIT,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_event_display_snapshot_byte_exact_for_every_variant() {
        for (ev, expected) in all_snapshots() {
            let actual = format!("{ev}");
            assert_eq!(
                actual, expected,
                "Display mismatch for {ev:?} — if you changed `Display`, update \
                 `expected_display_snapshots.rs` in lockstep."
            );
        }
    }

    #[test]
    fn structured_event_trace_span_catalogue_derived_from_enum_via_macro() {
        // Compile-time / runtime enumeration: every variant appearing
        // in the `Event` enum MUST have a fixture in
        // `all_snapshots()`. We pattern-match against every variant
        // to force a compile error if a new variant lands without
        // updating this table — the compiler's exhaustiveness check
        // is our "derived-from-enum" mechanism.
        let ev = fixture_request_start();
        let _: () = match ev {
            Event::RequestStart { .. }
            | Event::RequestEnd { .. }
            | Event::IndexedReadyBuilt { .. }
            | Event::VfsRead { .. }
            | Event::SharedLoadReuse { .. }
            | Event::DispatchEnter { .. }
            | Event::DispatchExit { .. }
            | Event::MaterializeMemberRouteStart { .. }
            | Event::MaterializeMemberRouteEnd { .. }
            | Event::RematerializePublicPropTypeStart { .. }
            | Event::RematerializePublicPropTypeEnd { .. }
            | Event::MaterializeDefinePropsMember { .. }
            | Event::FallthroughInheritanceComputed { .. }
            | Event::ResolveImportedTypeRoot { .. }
            | Event::CurrentEvalState { .. }
            | Event::MaterializeStructureEnter { .. }
            | Event::MaterializeStructureExit { .. }
            | Event::MaterializeStructurePolicySkip { .. }
            | Event::MaterializeStructureCycleDetected { .. }
            | Event::MaterializeStructureDepthFuseTripped { .. }
            | Event::CacheDrainedAtUpsert { .. }
            | Event::FactSignatureOverflow { .. }
            | Event::FactSignatureAdmissionRefused { .. }
            | Event::ModuleAugmentationStitched { .. }
            | Event::ModuleAugmentationIndexShape { .. }
            | Event::FileArtifactCache { .. }
            | Event::FactRegistryWrite { .. }
            | Event::FactValidationSummary { .. }
            | Event::ExportRouteResolved { .. }
            | Event::CompileModeDowngrade { .. }
            | Event::TypeInfoGraphPublished { .. }
            | Event::TypeInfoGraphDegraded { .. }
            | Event::TypeInfoGraphCacheHit { .. }
            | Event::Custom { .. } => (),
        };

        // Runtime coverage: every enum variant must appear in the
        // all_snapshots() list. We check by matching the string
        // representation of each variant discriminant.
        let expected_variants = [
            "RequestStart",
            "RequestEnd",
            "IndexedReadyBuilt",
            "VfsRead",
            "SharedLoadReuse",
            "DispatchEnter",
            "DispatchExit",
            "MaterializeMemberRouteStart",
            "MaterializeMemberRouteEnd",
            "RematerializePublicPropTypeStart",
            "RematerializePublicPropTypeEnd",
            "MaterializeDefinePropsMember",
            "FallthroughInheritanceComputed",
            "ResolveImportedTypeRoot",
            "CurrentEvalState",
            "MaterializeStructureEnter",
            "MaterializeStructureExit",
            "MaterializeStructurePolicySkip",
            "MaterializeStructureCycleDetected",
            "MaterializeStructureDepthFuseTripped",
            "Custom",
            "CacheDrainedAtUpsert",
            "FactSignatureOverflow",
            "FactSignatureAdmissionRefused",
            "ModuleAugmentationStitched",
            "ModuleAugmentationIndexShape",
            "FileArtifactCache",
            "FactRegistryWrite",
            "FactValidationSummary",
            "ExportRouteResolved",
            "CompileModeDowngrade",
            "TypeInfoGraphPublished",
            "TypeInfoGraphDegraded",
            "TypeInfoGraphCacheHit",
        ];
        let covered: Vec<&'static str> = all_snapshots()
            .iter()
            .map(|(ev, _)| match ev {
                Event::RequestStart { .. } => "RequestStart",
                Event::RequestEnd { .. } => "RequestEnd",
                Event::IndexedReadyBuilt { .. } => "IndexedReadyBuilt",
                Event::VfsRead { .. } => "VfsRead",
                Event::SharedLoadReuse { .. } => "SharedLoadReuse",
                Event::DispatchEnter { .. } => "DispatchEnter",
                Event::DispatchExit { .. } => "DispatchExit",
                Event::MaterializeMemberRouteStart { .. } => "MaterializeMemberRouteStart",
                Event::MaterializeMemberRouteEnd { .. } => "MaterializeMemberRouteEnd",
                Event::RematerializePublicPropTypeStart { .. } => {
                    "RematerializePublicPropTypeStart"
                }
                Event::RematerializePublicPropTypeEnd { .. } => "RematerializePublicPropTypeEnd",
                Event::MaterializeDefinePropsMember { .. } => "MaterializeDefinePropsMember",
                Event::FallthroughInheritanceComputed { .. } => "FallthroughInheritanceComputed",
                Event::ResolveImportedTypeRoot { .. } => "ResolveImportedTypeRoot",
                Event::CurrentEvalState { .. } => "CurrentEvalState",
                Event::MaterializeStructureEnter { .. } => "MaterializeStructureEnter",
                Event::MaterializeStructureExit { .. } => "MaterializeStructureExit",
                Event::MaterializeStructurePolicySkip { .. } => "MaterializeStructurePolicySkip",
                Event::MaterializeStructureCycleDetected { .. } => {
                    "MaterializeStructureCycleDetected"
                }
                Event::MaterializeStructureDepthFuseTripped { .. } => {
                    "MaterializeStructureDepthFuseTripped"
                }
                Event::Custom { .. } => "Custom",
                Event::CacheDrainedAtUpsert { .. } => "CacheDrainedAtUpsert",
                Event::FactSignatureOverflow { .. } => "FactSignatureOverflow",
                Event::FactSignatureAdmissionRefused { .. } => "FactSignatureAdmissionRefused",
                Event::ModuleAugmentationStitched { .. } => "ModuleAugmentationStitched",
                Event::ModuleAugmentationIndexShape { .. } => "ModuleAugmentationIndexShape",
                Event::FileArtifactCache { .. } => "FileArtifactCache",
                Event::FactRegistryWrite { .. } => "FactRegistryWrite",
                Event::FactValidationSummary { .. } => "FactValidationSummary",
                Event::ExportRouteResolved { .. } => "ExportRouteResolved",
                Event::CompileModeDowngrade { .. } => "CompileModeDowngrade",
                Event::TypeInfoGraphPublished { .. } => "TypeInfoGraphPublished",
                Event::TypeInfoGraphDegraded { .. } => "TypeInfoGraphDegraded",
                Event::TypeInfoGraphCacheHit { .. } => "TypeInfoGraphCacheHit",
            })
            .collect();
        for v in expected_variants.iter() {
            assert!(
                covered.iter().any(|c| c == v),
                "variant `{v}` has no fixture in all_snapshots() — add one in \
                 `expected_display_snapshots.rs`"
            );
        }
    }
}
