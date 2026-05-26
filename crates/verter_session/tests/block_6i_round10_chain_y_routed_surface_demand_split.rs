//! Block 6.i Round 10 — Chain Y discriminator (routed surface demand split).
//!
//! Closes Chain Y, the smallest residual emitter on the nuxt-ui
//! corpus (9.9% / 36 of 364 captured ProjectMember emissions per
//! the round-10 diagnostic at `D:/tmp/round10-diagnostic-report.md`,
//! all attributed to EditorDragHandle). The chain enters through
//! the macro publication route fast-path:
//!
//! ```text
//! produce_one_macro_object_shape
//!   ⇧ project_type_surface_shape_via_host_threaded
//!   ⇧ engine.dispatch_projected_surface
//!   ⇧ engine.dispatch_root_instantiated
//!   ⇧ Instantiate(Published(Expanded))  →  build_key_of / build_mapped_type emit
//! ```
//!
//! Pre-Commit-5 the macro publication's `Ref { name, type_arguments:
//! [] }` fast-path in `produce_one_macro_object_shape` always
//! routed through `project_type_surface_shape_via_host_threaded`,
//! which instantiated the root's full structural body under
//! `Published(Expanded)` — re-entering `build_key_of` /
//! `build_mapped_type` for `extends Omit<DragHandleProps, …>` /
//! `extends Omit<ButtonProps, …>` heritage chains and emitting one
//! `ProjectMember` edge per inherited library member name.
//!
//! Post-Commit-5 the fast-path applies the SAME path-precision
//! predicate the round-9 non-fast-path uses
//! (`macro_payload_root_is_conditional_carrier`): a Conditional
//! macro payload root retains `Published(Expanded)` for the
//! inherited-emits branch-merge protocol; a non-Conditional root
//! (Object / Intersection / Mapped / Ref / InstantiationRef)
//! routes through the new transit-shallow sibling
//! `project_type_surface_shape_transit_shallow_via_host_threaded`
//! which carrier-lowers in `Navigate` mode and projects under
//! `Published(Shallow)`.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "component_meta_audit/harness.rs"]
mod harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

// Hand-derived TipTap `DragHandleProps`-shape. Each member name
// appears only in this file and as an inherited published-surface
// key — never as a Vue SFC reserved name — so any `ProjectMember`
// edge naming any of these is unambiguously a Chain-Y Rule-5 leak.
const DRAG_HANDLE_PROPS_TS: &str = r#"
export interface DragHandleProps {
  editor?: unknown;
  computePositionConfig?: { strategy?: 'absolute' };
  onElementDragEnd?: (e: Event) => void;
  nestedOptions?: { offset?: number };
  getReferencedVirtualElement?: () => unknown;
  onNodeChange?: (node: unknown) => void;
  pluginKey?: string;
  element?: HTMLElement;
  onElementDragStart?: (e: Event) => void;
}
"#;

// `EditorDragHandleProps extends Omit<DragHandleProps, 'element'>`
// + `defineProps<EditorDragHandleProps>()`. The macro payload
// lowers to `Ref { 'EditorDragHandleProps', [] }` which hits the
// fast path in `produce_one_macro_object_shape`. Pre-Commit-5 the
// fast path drives `dispatch_root_instantiated`'s
// `Instantiate(Published(Expanded))` which instantiates the root's
// structural body and emits per-key edges for every member of
// `DragHandleProps` (minus the Omit'd `element` key).
const EDITOR_DRAG_HANDLE_VUE: &str = r#"<script setup lang="ts">
import type { DragHandleProps } from './drag_handle_props';
interface EditorDragHandleProps extends Omit<DragHandleProps, 'element'> {
  variant?: 'solid' | 'outline';
}
defineProps<EditorDragHandleProps>();
</script>
<template><div></div></template>
"#;

#[test]
fn chain_y_routed_surface_demand_split_does_not_leak_inherited_library_members() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/drag_handle_props.ts", DRAG_HANDLE_PROPS_TS),
            ("/EditorDragHandle.vue", EDITOR_DRAG_HANDLE_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let (_analysis, _resolved, audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/EditorDragHandle.vue")
        .expect("hermetic resolve must succeed");

    let footprint = audit
        .footprint
        .as_ref()
        .expect("footprint_capture is enabled in this harness");

    // Inherited `DragHandleProps` member names. These appear ONLY
    // in `DragHandleProps`' body — never on the SFC's macro payload
    // site — so any `ProjectMember` edge naming them is a Chain-Y
    // Rule-5 leak from the route fast-path. `element` is excluded
    // by the `Omit<..., 'element'>` clause anyway; the others are
    // inherited and MUST stay shallow.
    const LEAK_MEMBERS: &[&str] = &[
        "editor",
        "computePositionConfig",
        "onElementDragEnd",
        "nestedOptions",
        "getReferencedVirtualElement",
        "onNodeChange",
        "pluginKey",
        "onElementDragStart",
    ];

    // Block 6.j R18 — scope leak counter to intermediate provenances.
    // `MemberEdgeProvenance::PublishedField` is the producer-side
    // declaration of the user-visible surface and is OUT of the leak
    // domain: a `defineProps<EditorDragHandleProps>()` publication
    // legitimately names every inherited `DragHandleProps` key as a
    // published prop, so a `PublishedField`-tagged edge for `editor`
    // is not a leak. The leak this test pins is intermediate enumeration
    // via Mapped / KeyOf / Path reduction (`MappedKeyEnumerated` /
    // `KeyOfEnumerated` / `PathProjection`).
    let mut leak_edge_count = 0usize;
    let mut leak_edge_names: Vec<String> = Vec::new();
    for edge in footprint.derivation_subgraph.edges.iter() {
        if !matches!(edge.kind, OriginEdgeKind::ProjectMember) {
            continue;
        }
        if let OriginEdgeMetaDto::ProjectMember {
            member_name,
            provenance,
        } = &edge.meta
        {
            if matches!(
                provenance,
                verter_audit::MemberEdgeProvenance::PublishedField
            ) {
                continue;
            }
            if LEAK_MEMBERS.contains(&member_name.as_ref()) {
                leak_edge_count += 1;
                leak_edge_names.push(member_name.to_string());
            }
        }
    }

    let mut leak_path_count = 0usize;
    for projection in footprint.projections.iter() {
        for seg in projection.path.iter() {
            if let verter_audit::origin_graph::ProjectPathSegment::Member { name } = seg {
                if LEAK_MEMBERS.contains(&name.as_ref()) {
                    leak_path_count += 1;
                }
            }
        }
    }

    let total = leak_edge_count + leak_path_count;

    assert_eq!(
        total, 0,
        "Block 6.i Round 10 Chain Y — macro publication route fast-path \
         MUST NOT emit `ProjectMember` edges for inherited library members \
         (`editor`, `computePositionConfig`, `onElementDragEnd`, …). The \
         macro fast-path in `produce_one_macro_object_shape` must apply \
         the path-precision predicate `macro_payload_root_is_conditional_carrier` \
         (same predicate as the round-9 non-fast-path) and route \
         non-Conditional roots through \
         `project_type_surface_shape_transit_shallow_via_host_threaded` \
         (carrier-lower in `Navigate` + project under \
         `Published(Shallow)`). Got: leak_edges={leak_edge_count} \
         (names={leak_edge_names:?}), \
         projection_path_member_hits={leak_path_count}. \
         See `D:/tmp/round10-diagnostic-report.md` Chain Y (9.9% / 36 of \
         364 captured emissions on EditorDragHandle) and the codex 5th \
         consult Q1-Y verdict at `D:/tmp/round10-codex-reconsult-out.txt`."
    );
}
