//! Block 6.i Round 10 — Chain V discriminator (TypeExpr field
//! materialiser carrier-stop).
//!
//! Closes the dominant residual emitter chain on the nuxt-ui corpus
//! Rule-5 leak (50.5% / 184 of 364 captured emissions per the round-10
//! diagnostic at `D:/tmp/round10-diagnostic-report.md`). The chain
//! enters through the projector pipeline's per-prop publication path:
//!
//! ```text
//! reduce_published_field_types
//!   ⇧ reduce_field_type_expr_with_mode(query_engine, scope, expr,
//!                                      ProjectionMode::Navigate)
//!   ⇧ materialize_component_meta_type_expr_until_stable(
//!         expr, scope, ProjectionMode::Expanded /* hardcoded! */,
//!         query_engine,
//!     )
//!   ⇧ shallow_lower_type_expr(..., Expanded)
//!   ⇧ build_key_of / build_mapped_type  →  ProjectMember emit
//! ```
//!
//! Pre-Commit-2 the projector's `reduce_field_type_expr_with_mode`
//! hardcoded `ProjectionMode::Expanded` when dispatching to the TypeExpr
//! materialiser, regardless of the caller's `publish_mode`. The
//! per-prop publication surface called this entry with `Navigate`, but
//! the materialiser's INTERNAL lowering ran at `Published(Expanded)` —
//! so `Partial<EditorOptions>` (a `Mapped { source: keyof EditorOptions,
//! … }`) reduced through `build_mapped_type`'s publication loop and
//! emitted one `ProjectMember` edge per enumerated keyspace name.
//!
//! Post-Commit-2 the materialiser propagates `publish_mode` verbatim
//! — a `Navigate` per-prop caller now drives `shallow_lower` and
//! `raise_and_reduce` under `Published(Navigate)`. The Mapped carrier
//! interior stays deferred at the macro-publication boundary; no
//! `ProjectMember` edge fires for inherited library members on the
//! per-prop chain.
//!
//! ## Hermetic shape
//!
//! Editor-like fixture (Round-10 diagnostic Chain V dominant emitter):
//! `EditorProps<T,H> extends Omit<Partial<EditorOptions>, 'content' |
//! 'element'>` then `withDefaults(defineProps<EditorProps<T,H>>(), …)`.
//! The two-tier indirection (a userland `EditorProps` intermediate
//! that extends a `Omit<Partial<Lib>, …>` chain) is what makes the
//! per-prop field materialiser walk into the Mapped publication
//! interior — the simpler `defineProps<Partial<EditorOptions>>()`
//! shape (round-9 Pattern A test fixture) does NOT trigger Chain V
//! because the round-9 path-precise transit-shallow swap closes it
//! at the macro publication layer. Chain V leaks specifically when
//! `evaluated_types.props` carries an inherited library member name
//! whose published `TypeExpr` body still contains a `Mapped` /
//! `KeyOf` interior — `reduce_published_field_types` then enters
//! the per-field materialiser under `Navigate`, but the materialiser
//! (pre-Commit-2) silently upgraded to `Expanded` and emitted the
//! per-key edges.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "component_meta_audit/harness.rs"]
mod harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

// Hand-derived TipTap `EditorOptions`-shape — narrow enough to be
// hermetic (no external dependencies), wide enough that round-9's
// macro-publication closure can't suppress every inherited member
// name through path-precision. Each member name appears only in this
// file and as an inherited published-surface key — never as a Vue
// SFC reserved name — so a ProjectMember edge naming any of these is
// unambiguously a Rule-5 leak.
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
"#;

// `defineProps<{ editorOptions: Partial<EditorOptions> }>()` — a
// per-prop field carrying a `Mapped { source: keyof EditorOptions,
// … }` body. The per-field publication path enters through
// `reduce_published_field_types` → `reduce_field_type_expr_with_mode(
// query_engine, scope, raised, ProjectionMode::Navigate)`. Pre-
// Commit-2 the materialiser silently upgraded to `Published(Expanded)`
// at `projectors/mod.rs:1429` and ran the Mapped publication loop
// — emitting one `ProjectMember` edge per enumerated `EditorOptions`
// member. Post-Commit-2 the materialiser propagates `publish_mode`
// (`Navigate`) verbatim so the Mapped reduction carrier-stops at
// the per-prop publication boundary and no inherited member name
// fires.
const EDITOR_VUE: &str = r#"<script setup lang="ts">
import type { EditorOptions } from './editor_options';
defineProps<{ editorOptions: Partial<EditorOptions> }>();
</script>
<template><div></div></template>
"#;

#[test]
fn chain_v_field_typeexpr_carrier_stop_does_not_leak_inherited_library_members() {
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

    // Inherited library members that MUST NOT appear as
    // `ProjectMember` edges through the per-prop projector
    // publication path. These names live ONLY in `EditorOptions`'
    // body — never on `EditorProps`' macro payload — so any edge
    // naming them is a Chain-V Rule-5 leak from the per-field
    // `reduce_field_type_expr_with_mode` materialiser.
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
        if let OriginEdgeMetaDto::ProjectMember { member_name } = &edge.meta {
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
        "Block 6.i Round 10 Chain V — per-prop publication MUST NOT \
         emit `ProjectMember` edges for inherited library members \
         (`editable`, `textDirection`, `tabindex`, …). The TypeExpr \
         field materialiser must propagate the caller's \
         `publish_mode` instead of hardcoding `ProjectionMode::Expanded`: \
         a per-prop `Navigate` caller drives `shallow_lower` + \
         `raise_and_reduce` under `Published(Navigate)` so the Mapped/\
         KeyOf carrier interior stays deferred at the per-prop publication \
         boundary. Got: leak_edges={leak_edge_count} \
         (names={leak_edge_names:?}), \
         projection_path_member_hits={leak_path_count}. \
         Pre-Commit-2 the projector's `reduce_field_type_expr_with_mode` \
         hardcoded `ProjectionMode::Expanded` at \
         `projectors/mod.rs:1429`; Commit 2 of Round 10 propagates \
         `publish_mode` so the per-prop publication respects \
         `Navigate`. See `D:/tmp/round10-diagnostic-report.md` Chain V \
         (50.5% / 184 of 364 captured emissions) and the codex 5th \
         consult Q1-V verdict at `D:/tmp/round10-codex-reconsult-out.txt`."
    );
}
