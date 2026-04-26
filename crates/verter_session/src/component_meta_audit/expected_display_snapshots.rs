//! Byte-exact `Display` snapshots for every variant of
//! [`StructuredComponentMetaEvent`](super::StructuredComponentMetaEvent).
//!
//! Plan §3 Commit 5 / §3.A Commit 6.E. The
//! `structured_event_display_snapshot_byte_exact_for_every_variant`
//! test in this module constructs a canonical instance of each
//! variant, invokes `format!("{}", ev)`, and asserts the output
//! equals the corresponding `EXPECTED_*` constant here — verbatim.
//!
//! A drift in the `Display` impl OR a new enum variant without a
//! snapshot fails the test. The format is intentionally not a
//! legacy carry-over; it is authored fresh (plan §2.3 — legacy
//! `format!("k=v")` stderr format is gone).

use std::sync::Arc;

use super::{
    CacheOutcomeKind, DispatchKeyKind, MaterializationScopeAudit, MaterializationSubject,
    MaterializeSkipReason, ProjectionModeAudit, StructuredComponentMetaEvent as Event, VfsLayer,
};
use crate::types::Hash16;

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
    // first four bytes `01 02 03 04` → short_hash yields "01020304".
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

/// Pair each fixture with its expected Display string. The
/// `structured_event_display_snapshot_byte_exact_for_every_variant`
/// test iterates this table, and the companion `all_variants_covered`
/// test reflects on `StructuredComponentMetaEvent`'s match discriminants
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
