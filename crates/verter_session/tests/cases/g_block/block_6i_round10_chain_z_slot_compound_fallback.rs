//! Chain Z discriminator (slot compound-objects fallback
//! transit-shallow).
//!
//! Closes Chain Z, the residual slot-publication leak on ChatMessages
//! fresh-cold. The chain enters through the slot publication's
//! `Option::or_else` fallback in
//! `produce_one_macro_object_shape_for_slots`:
//!
//! ```text
//! project_expr_class_a_via_dispatch_transit_shallow(...)
//!     .or_else(|| project_expr_surface_expr_with_compound_objects_via_host_threaded(...))
//! ```
//!
//! An `.or_else` branch that called the Expanded helper
//! `project_expr_surface_expr_with_compound_objects_via_host_threaded`
//! leaks: it lowered the slot binding's TypeExpr in
//! `ProjectionMode::Expanded` and projected under
//! `Published(Expanded)`. The Expanded demand re-entered
//! `build_key_of` / `build_mapped_type` for the slot payload's
//! `Mapped<...>` body and emitted per-key `ProjectMember` edges for
//! the inherited library member names on the fresh-cold pass where
//! the transit-shallow primary path returned `None`.
//!
//! The slot fallback instead uses the sibling
//! `project_expr_surface_expr_with_compound_objects_transit_shallow_via_host_threaded`
//! which mirrors the transit-shallow Class A helper's demand
//! profile (`Navigate` lowering + `Published(Shallow)` terminal).
//! The slot publication boundary observes a one-level Object
//! surface with carrier-shaped member values per the
//! shallow-by-default rule; no per-key edge fires for inherited
//! library members.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use super::harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

// Hand-derived AI-SDK `UITools`-shape carrying canonical chain-Z
// leak surface members (`outputSchema`, `execute`). The fresh-cold
// pass on ChatMessages emits these exclusively through the slot
// compound-objects fallback per the diagnostic.
const UI_TOOLS_TS: &str = r#"
export type UITools = {
  toolA: { outputSchema: { kind: 'json' }; execute: (args: unknown) => unknown };
  toolB: { outputSchema: { kind: 'text' }; execute: (args: unknown) => unknown };
};

export type ChatMessagesSlots<TTools extends UITools = UITools> = {
  default: (props: { tools: TTools }) => unknown;
  item: (props: { tool: TTools[keyof TTools] }) => unknown;
};
"#;

// `defineSlots<ChatMessagesSlots<T>>()` — the slot payload is a
// generic-substituted `UITools` whose per-tool method bodies carry
// `outputSchema` / `execute`. The slot publication's primary
// transit-shallow Class A path may return `None` for this compound
// shape; the `.or_else` fallback then drives the compound-objects
// helper. The Expanded helper would emit the per-key edges; the
// transit-shallow sibling keeps the Mapped carrier deferred.
const CHAT_MESSAGES_VUE: &str = r#"<script setup lang="ts" generic="T extends import('./ui_tools').UITools = import('./ui_tools').UITools">
import type { ChatMessagesSlots } from './ui_tools';
defineSlots<ChatMessagesSlots<T>>();
</script>
<template><div></div></template>
"#;

#[test]
fn chain_z_slot_compound_fallback_does_not_leak_inherited_library_members() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/ui_tools.ts", UI_TOOLS_TS),
            ("/ChatMessages.vue", CHAT_MESSAGES_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let (_analysis, _resolved, audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/ChatMessages.vue")
        .expect("hermetic resolve must succeed");

    let footprint = audit
        .footprint
        .as_ref()
        .expect("footprint_capture is enabled in this harness");

    // Inherited generic-substituted library members that MUST NOT
    // appear in any `ProjectMember` edge. These live ONLY inside
    // `UITools`' per-tool method bodies — never on the slot
    // payload's declared surface — so any edge naming them is a
    // Chain-Z Rule-5 leak from the slot compound-objects fallback.
    const LEAK_MEMBERS: &[&str] = &["outputSchema", "execute"];

    let mut leak_edge_count = 0usize;
    let mut leak_edge_names: Vec<String> = Vec::new();
    for edge in footprint.derivation_subgraph.edges.iter() {
        if !matches!(edge.kind, OriginEdgeKind::ProjectMember) {
            continue;
        }
        if let OriginEdgeMetaDto::ProjectMember { member_name, .. } = &edge.meta {
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
        "Chain Z — slot publication's compound-objects \
         fallback MUST NOT emit `ProjectMember` edges for inherited \
         generic-substituted library members (`outputSchema`, `execute`). \
         The slot fallback in `produce_one_macro_object_shape_for_slots` \
         must call the transit-shallow \
         sibling `project_expr_surface_expr_with_compound_objects_transit_shallow_via_host_threaded`, \
         NOT the retired Expanded helper. Got: leak_edges={leak_edge_count} \
         (names={leak_edge_names:?}), \
         projection_path_member_hits={leak_path_count}."
    );
}
