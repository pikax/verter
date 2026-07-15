//! Chain X discriminator (PathWalker Mapped admission non-emitting).
//!
//! Closes Chain X, a large residual emitter on the nuxt-ui corpus
//! of captured ProjectMember emissions. The chain enters
//! through the host's evaluated-types cold path:
//!
//! ```text
//! cold_resolver::resolve_component_meta_parts
//!   ⇧ jsdoc_resolve::HostComponentMetaResolver::build_eval_outputs
//!   ⇧ VerterHost::compute_evaluated_types_with_tracking_from_owner_context_with_ctx
//!   ⇧ host_manage::eval_env::compute_evaluated_types_from_owner_context_with_ctx
//!   ⇧ ProjectSemanticDispatch::build_project_path
//!   ⇧ PathWalker::advance_step / walk
//!   ⇧ evaluate_deferred_semantic_node
//!   ⇧ ProjectSemanticDispatch::key_names_from_keyspace_node  (enumerate.rs:221)
//!   ⇧ shallow_lower_type_expr_with_context
//!   ⇧ build_key_of / build_mapped_type  →  ProjectMember emit
//! ```
//!
//! A PathWalker whose Tier-2 Mapped admission called
//! `dispatch.key_names_from_keyspace_node(mapper.key_space)` to test
//! "does this Mapped contain `needle`?" leaks: that helper enumerates
//! the ENTIRE keyspace through `evaluate_deferred_semantic_node` +
//! `key_names_from_base_node` — and the enumerator routes
//! `Instantiate(Published(Expanded))` for deferred shells. Result:
//! `build_key_of` / `build_mapped_type` emit one `ProjectMember` edge
//! per key for every segment admission, even though the walker only
//! cares about ONE literal.
//!
//! The walker instead calls the non-emitting predicate
//! `keyspace_admits_literal_non_emitting` (in
//! `enumerate.rs`) which decides admission structurally without
//! ever calling `evaluate_deferred_semantic_node` on a deferred
//! shell, without ever calling `key_names_from_base_node`. When
//! the predicate cannot prove admission (`None`), the walker
//! falls through to Tier 3 (primitive keyspace) or accepts the
//! unresolved carrier — NEVER enumerating just to prove membership.
//!
//! ## Hermetic shape
//!
//! Carousel-like fixture (the Chain X dominant emitter). Carousel
//! emits 100% through Chain X — the cleanest empirical isolate for the
//! chain. The shape: `CarouselProps<T> extends
//! Omit<EmblaOptionsType, …>`.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use super::harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

// Hand-derived Embla `EmblaOptionsType`-shape — narrow enough to be
// hermetic, wide enough that path admission must walk a Mapped's
// `keyof` to discover each member. Each member name appears only
// here and as an inherited published-surface key — never as a Vue
// SFC reserved name — so any `ProjectMember` edge naming any of
// these is unambiguously a Chain-X Rule-5 leak.
const EMBLA_OPTIONS_TS: &str = r#"
export type EmblaOptionsType = {
  align?: 'start' | 'center' | 'end';
  axis?: 'x' | 'y';
  containScroll?: 'trimSnaps' | 'keepSnaps' | false;
  direction?: 'ltr' | 'rtl';
  dragFree?: boolean;
  dragThreshold?: number;
  loop?: boolean;
  skipSnaps?: boolean;
  slidesToScroll?: 'auto' | number;
  watchDrag?: boolean;
  watchResize?: boolean;
  watchSlides?: boolean;
};
"#;

// `defineProps<Pick<EmblaOptionsType, 'align' | 'loop'>>()`. The
// `Pick` lowers to a `Mapped { source: keyof EmblaOptionsType narrowed
// to 'align' | 'loop', value: EmblaOptionsType[K] }`. For each
// macro-declared field name, the cold evaluated-types path dispatches
// `ProjectPath { base: PickMapped, path: [Member(name)], context:
// Published(Expanded) }`. A PathWalker Tier-2 Mapped admission that
// calls `key_names_from_keyspace_node` enumerates
// the whole `keyof EmblaOptionsType` — emitting one `ProjectMember`
// edge per `EmblaOptionsType` member, even for the names that aren't
// in the Pick narrow.
const CAROUSEL_VUE: &str = r#"<script setup lang="ts">
import type { EmblaOptionsType } from './embla_options';
defineProps<Pick<EmblaOptionsType, 'align' | 'loop'>>();
</script>
<template><div></div></template>
"#;

#[test]
fn chain_x_path_admission_non_emitting_does_not_leak_inherited_library_members() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/embla_options.ts", EMBLA_OPTIONS_TS),
            ("/Carousel.vue", CAROUSEL_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let (_analysis, _resolved, audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/Carousel.vue")
        .expect("hermetic resolve must succeed");

    let footprint = audit
        .footprint
        .as_ref()
        .expect("footprint_capture is enabled in this harness");

    // Inherited library member names. These live ONLY in
    // `EmblaOptionsType`'s body — never on `CarouselProps`'
    // declared surface — so any `ProjectMember` edge naming them is
    // a Chain-X Rule-5 leak from `PathWalker`'s Mapped admission.
    // The Pick selects only `align` and `loop`. Every OTHER member
    // name of `EmblaOptionsType` (`axis`, `containScroll`,
    // `direction`, `dragFree`, …) MUST NOT appear in any
    // `ProjectMember` edge — those names are in the SOURCE
    // keyspace but not in the Pick-narrowed surface. A walker that
    // enumerates `keyof EmblaOptionsType` to test membership of
    // `align` / `loop` emits per-key edges for ALL members,
    // including the non-Picked ones. The non-emitting predicate
    // admits `align` / `loop` structurally without enumeration.
    const LEAK_MEMBERS: &[&str] = &[
        "axis",
        "containScroll",
        "direction",
        "dragFree",
        "dragThreshold",
        "skipSnaps",
        "slidesToScroll",
        "watchDrag",
        "watchResize",
        "watchSlides",
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
        "Chain X — `PathWalker`'s Mapped admission \
         MUST NOT emit `ProjectMember` edges for inherited library \
         members. The Tier-2 admission must use the non-emitting \
         membership predicate `keyspace_admits_literal_non_emitting`, \
         NOT `key_names_from_keyspace_node` which enumerates the full \
         keyspace through `evaluate_deferred_semantic_node` and \
         `key_names_from_base_node`. Got: leak_edges={leak_edge_count} \
         (names={leak_edge_names:?}), \
         projection_path_member_hits={leak_path_count}. \
         A walker that called `key_names_from_keyspace_node` for path \
         admission would emit one `ProjectMember` edge per enumerated \
         key; the non-emitting predicate avoids that."
    );
}
