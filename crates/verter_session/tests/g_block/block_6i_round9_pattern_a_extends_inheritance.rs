//! Block 6.i Round 9 — Pattern A discriminator (non-slot props
//! Mapped/utility-route library inheritance leak).
//!
//! Closes the architectural class behind the corpus leak for `Editor`,
//! `Table`, `Carousel`, `EditorDragHandle` (and Pattern B singular
//! `ChatMessage`): a non-slot `defineProps<X>()` whose lowered macro
//! payload is rooted at a Mapped/utility-route carrier (or routes via
//! a library `extends` chain) walks the inherited interface body
//! structurally under `Published(Expanded)` and emits one
//! `ProjectMember` edge per inherited member name.
//!
//! Pre-Commit-2 the non-slot path in [`produce_one_macro_object_shape`]
//! routes through `project_expr_class_a_via_dispatch_threaded` under
//! `Published(Expanded)`. The Mapped-arm publication path
//! ([`build_mapped_type`] / [`intern_keyspace_names`]) reduces
//! `keyof T` and emits one `ProjectMember` edge per enumerated key.
//!
//! Post-Commit-2 the producer branches on the lowered root's semantic
//! shape: an Object/Intersection/Mapped/Ref/InstantiationRef root
//! dispatches via `project_expr_class_a_via_dispatch_transit_shallow_threaded`.
//! The transit-shallow Empty-path lowering at `Navigate` mode + the
//! terminal `Published(Shallow)` projection keeps the Mapped carrier
//! deferred at the macro-publication boundary — `keyof` never reduces
//! and no `ProjectMember` edges fire for the inherited library member
//! names. The `Conditional` carrier path stays on `Published(Expanded)`
//! for the inherited-emits branch-merge protocol — see
//! [`block_6i_round9_inherited_emits_branch_merge_survives`] for the
//! regression guard.
//!
//! ## Hermetic shape
//!
//! `defineProps<Partial<EditorOptions>>()` — a non-slot macro payload
//! rooted at a Mapped utility route over `keyof EditorOptions`. This
//! is the dispatch-level analogue of the corpus
//! `EditorProps extends Omit<Partial<EditorOptions>, …>` heritage
//! chain — both lower into the producer's non-slot Class A surface
//! projection where the publication boundary must keep the Mapped
//! reduction deferred.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "../component_meta_audit/harness.rs"]
mod harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

// Hand-derived TipTap `EditorOptions`-shape. Each member name appears
// only in this file and as a published-surface key — never as a Vue
// SFC reserved name — so a ProjectMember edge naming any of these is
// unambiguously a Rule-5 leak from the inherited library body.
const EDITOR_OPTIONS_TS: &str = r#"
export interface EditorOptions {
  editable?: boolean;
  textDirection?: 'ltr' | 'rtl';
  tabindex?: number;
  clipboardTextSerializer?: (slice: unknown) => string;
  focusEvents?: { onFocus?: () => void };
  keymap?: Record<string, () => void>;
  paste?: (e: Event) => void;
}
"#;

// `defineProps<Partial<EditorOptions>>()`. Lowered macro payload is
// `InstantiationRef { base: Partial, args: [EditorOptions] }` — root
// shape after Navigate-mode realization is `Mapped { source: keyof
// EditorOptions, … }`. NOT a Conditional → Commit 2's path-precise
// branch dispatches the transit-shallow helper.
const EDITOR_VUE: &str = r#"<script setup lang="ts">
import type { EditorOptions } from './editor_options';
defineProps<Partial<EditorOptions>>();
</script>
<template><div></div></template>
"#;

#[test]
fn pattern_a_non_slot_mapped_publication_does_not_leak_inherited_library_members() {
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

    // Inherited library members that MUST NOT appear as ProjectMember
    // edges through the macro-publication path. These names never
    // appear in the SFC's macro payload site — they are sourced ONLY
    // from EditorOptions' inherited body, so any edge naming them is
    // a Rule-5 leak.
    const LEAK_MEMBERS: &[&str] = &[
        "editable",
        "textDirection",
        "tabindex",
        "clipboardTextSerializer",
        "focusEvents",
        "keymap",
        "paste",
    ];

    // Block 6.j R18 — scope leak counter to intermediate provenances.
    // `MemberEdgeProvenance::PublishedField` is the producer-side
    // declaration of the user-visible surface and is OUT of the leak
    // domain: a `defineProps<Partial<EditorOptions>>()` publication
    // legitimately names every EditorOptions key as a published prop,
    // so a `PublishedField`-tagged edge for `editable` is not a leak.
    // The leak this test pins is intermediate enumeration via Mapped /
    // KeyOf / Path reduction (`MappedKeyEnumerated` /
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
        "Block 6.i Round 9 Pattern A — non-slot Mapped/utility-route macro \
         publication MUST publish 0 ProjectMember edges or projection-path \
         Member segments naming any inherited `EditorOptions` member \
         (`editable`, `textDirection`, `tabindex`, `clipboardTextSerializer`, \
         `focusEvents`, `keymap`, `paste`). The macro publication boundary \
         must keep the Mapped/keyof reduction deferred. \
         Got: leak_edges={leak_edge_count} (names={leak_edge_names:?}), \
         projection_path_member_hits={leak_path_count}. \
         At HEAD `23c866eb1` the non-slot path lowers at \
         Published(Expanded) and routes through \
         `project_expr_class_a_via_dispatch_threaded`. The Empty-path \
         Expanded lowering reduces `Partial<EditorOptions>` via the \
         Mapped publication path; `build_mapped_type` / \
         `intern_keyspace_names` then emits one `ProjectMember` edge per \
         enumerated key. Commit 2's path-precise transit-shallow swap \
         keeps the Mapped carrier deferred for Object/Intersection/Mapped/Ref/\
         InstantiationRef-rooted macro payloads (the Conditional-rooted \
         inherited-emits path retains Expanded for branch-merge — see \
         `block_6i_round9_inherited_emits_branch_merge_survives`)."
    );
}
