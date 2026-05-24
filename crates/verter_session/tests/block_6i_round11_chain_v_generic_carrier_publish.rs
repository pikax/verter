//! Block 6.i Round 11 — Chain V discriminator (generic-bearing carrier
//! per-prop publication).
//!
//! Closes the residual Chain V leak that Round-10's Commit 2 did NOT
//! close. The Round-10 Commit 2 fixture used a SIMPLE
//! `defineProps<{ editorOptions: Partial<EditorOptions> }>()` shape:
//! the per-prop publication path enters
//! `reduce_field_type_expr_with_mode(... Navigate)` carrying a single
//! Mapped TypeExpr for `Partial<EditorOptions>`. Commit 2's propagation
//! of `publish_mode` into `materialize_component_meta_type_expr_until_stable`'s
//! mode parameter closes that fixture: the `dispatch.shallow_lower_type_expr(...
//! Navigate)` + `dispatch.raise_and_reduce(_, Navigate)` legs both run
//! under `Published(Navigate)` (the legacy `mode → Published(mode)` shell),
//! which under the codex-hybrid demand axis IS reduction-permissive —
//! `may_reduce_operator(Published(_)) == true` regardless of mode.
//!
//! But the corpus Editor (nuxt-ui-codex-bench `Editor.vue`) carries a
//! **two-tier** macro payload:
//!
//! ```text
//! export interface EditorProps<
//!   T extends Content = Content,
//!   H extends EditorCustomHandlers = EditorCustomHandlers,
//! > extends Omit<Partial<EditorOptions>, 'content' | 'element'>
//! { /* own fields */ }
//! defineProps<EditorProps<T, H>>()
//! ```
//!
//! The per-prop publication path then iterates each inherited prop
//! (`editable`, `textDirection`, `tabindex`, …). Each inherited prop's
//! `field.r#type` enters `reduce_field_type_expr_with_mode(Navigate)`
//! as the substituted-then-projected value carrier — a generic-bearing
//! `Omit<Partial<EditorOptions>, ...>` expression after the macro-
//! publication layer routes `EditorProps<T, H>` to its declaration body.
//!
//! The materialiser then lowers the carrier under `Published(Navigate)`
//! and `build_mapped_type` reaches `may_reduce_operator(Published(_)) ==
//! true` for the Mapped's source, enumerating the source's keyspace and
//! emitting one `ProjectMember` edge per `EditorOptions` member.
//!
//! Per codex 6th-consult Q1-V (BINDING):
//!
//! > V needs a context-explicit TypeExpr materializer using
//! > `StructuralTransit(Navigate)` for carrier lowering and
//! > `Published(Navigate)` only at the terminal publication boundary.
//!
//! Post-Round-11 Commit 2 the materialiser carrier-lowers under
//! `StructuralTransit(Navigate)` — `may_reduce_operator` evaluates
//! `false` at every nested `Instantiate` / `KeyOf` / `MappedType`
//! dispatch — and the published shape stays as a structural carrier
//! that the per-prop publication boundary observes one-level shallow.
//!
//! ## Hermetic shape
//!
//! Mirrors the corpus Editor: a userland generic interface that extends
//! `Omit<Partial<Lib>, …>` and is consumed by `defineProps<X<T,H>>()`
//! through `<script setup generic="…">`. The leak member names live
//! ONLY in the library `EditorOptions` body — never on the SFC's
//! macro payload site — so any `ProjectMember` edge naming them on
//! the per-prop publication path is unambiguously a Chain-V Rule-5
//! leak from the per-field carrier materialiser.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "component_meta_audit/harness.rs"]
mod harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

// Hand-derived TipTap `EditorOptions`-shape (narrow enough to be
// hermetic, wide enough that any whole-keyspace publication of a
// derived Mapped fans into MANY uniquely-named members). Each member
// name appears only in this file and as an inherited published-surface
// key — never as a Vue SFC reserved name — so any ProjectMember edge
// naming any of these is unambiguously a Chain-V Rule-5 leak.
const EDITOR_OPTIONS_TS: &str = r#"
export interface EditorOptions {
  editable?: boolean;
  textDirection?: 'ltr' | 'rtl';
  tabindex?: number;
  clipboardTextSerializer?: (slice: unknown) => string;
  focusEvents?: { onFocus?: () => void };
  keymap?: Record<string, () => void>;
  paste?: (e: Event) => void;
  content?: string;
  element?: HTMLElement;
}
export interface ContentDescriptor {
  type: string;
}
export interface EditorHandlersConfig {
  beforeSend?: () => void;
}
"#;

// Mirrors the corpus Editor shape: a generic interface extending
// `Omit<Partial<EditorOptions>, …>` consumed by `defineProps<X<T,H>>()`
// through `<script setup generic>`. The per-prop publication path
// iterates each inherited member name and enters
// `reduce_field_type_expr_with_mode(... Navigate)` for the carrier-
// substituted value. Pre-Round-11 the materialiser lowered the
// carrier under `Published(Navigate)` and emitted per-member edges
// on the Mapped reduction.
const EDITOR_VUE: &str = r#"<script lang="ts">
import type { EditorOptions, ContentDescriptor, EditorHandlersConfig } from './editor_options';

export interface EditorProps<
  T extends ContentDescriptor = ContentDescriptor,
  H extends EditorHandlersConfig = EditorHandlersConfig,
> extends Omit<Partial<EditorOptions>, 'content' | 'element'> {
  as?: any;
  modelValue?: T;
  handlers?: H;
}
</script>
<script setup lang="ts" generic="T extends ContentDescriptor, H extends EditorHandlersConfig">
defineProps<EditorProps<T, H>>();
</script>
<template><div></div></template>
"#;

#[test]
fn chain_v_generic_carrier_does_not_leak_inherited_library_members_through_per_prop_publication() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/editor_options.ts", EDITOR_OPTIONS_TS),
            ("/Editor.vue", EDITOR_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let (_analysis, _resolved, audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/Editor.vue")
        .expect("hermetic resolve must succeed");

    let footprint = audit
        .footprint
        .as_ref()
        .expect("footprint_capture is enabled in this harness");

    // Inherited library members that MUST NOT appear as `ProjectMember`
    // edges through the per-prop publication path on a generic-bearing
    // `Omit<Partial<Lib>, …>` carrier. These names live ONLY in
    // `EditorOptions`' body — never on `EditorProps`' declared own
    // surface — so any `ProjectMember` edge naming them after Round 11
    // is a Chain-V Rule-5 leak from the carrier materialiser.
    const LEAK_MEMBERS: &[&str] = &[
        "editable",
        "textDirection",
        "tabindex",
        "clipboardTextSerializer",
        "focusEvents",
        "keymap",
        "paste",
    ];

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
        "Block 6.i Round 11 Chain V — a generic-bearing \
         `Omit<Partial<Lib>, ...>` carrier on the per-prop publication \
         path MUST NOT emit `ProjectMember` edges for the library's \
         inherited members. The TypeExpr materialiser must drive the \
         lowering under `StructuralTransit(Navigate)` (per codex 6th-\
         consult Q1-V) so `may_reduce_operator` evaluates `false` at \
         every nested `Instantiate` / `KeyOf` / `MappedType` dispatch — \
         the per-prop publication boundary observes the carrier shape \
         one-level shallow, never enumerating the source's keyspace. \
         Got: leak_edges={leak_edge_count} (names={leak_edge_names:?}), \
         projection_path_member_hits={leak_path_count}. \
         Pre-Round-11 the materialiser at \
         `meta_resolve/materialize/field_types.rs:62-225` ran \
         `dispatch.shallow_lower_type_expr(..., Navigate)` + \
         `dispatch.raise_and_reduce(_, Navigate)` under `Published(Navigate)` \
         (the legacy `mode → published(mode)` shell), which is still \
         reduction-permissive — `may_reduce_operator(Published(_)) == \
         true`. Round-11 Commit 2 routes the carrier lowering through \
         `StructuralTransit(Navigate)`. See \
         `D:/tmp/round10-review-codex-out.txt` Q1-V (BINDING) and \
         `D:/tmp/round11-implementer-brief.md` Commit 2 spec."
    );
}
