//! Block 6.i Round 11 — Chain X discriminator (fact-backed DeclRef
//! keyspace admission).
//!
//! Closes the residual Chain X leak that Round-10's Commit 3 did NOT
//! close. The Round-10 Commit 3 fixture used a SIMPLE
//! `defineProps<Pick<EmblaOptionsType, 'align' | 'loop'>>()` shape:
//! `Pick<X, K>` with K a literal-union narrows `mapper.key_space` to a
//! string-literal union. Tier 2 of the path-admission chain
//! (`keyspace_admits_literal_non_emitting` at `enumerate.rs:297`)
//! decides admission directly on the Literal / Union arms — the
//! keyspace never reaches a `DeclRef` / `InstantiationRef` base.
//!
//! The corpus Carousel (nuxt-ui-codex-bench `Carousel.vue`) leaks
//! because its macro payload `CarouselProps<T> extends
//! Omit<EmblaOptionsType, 'axis' | 'container' | 'slides' | 'direction'>`
//! lowers `Omit<DeclRef, K>` → `Pick<DeclRef, Exclude<keyof DeclRef, K>>`.
//! When the published surface is computed, the PathWalker visits
//! `CarouselProps['items']` (an OWN prop). To resolve the carrier
//! shell, the walker traverses the inherited Mapped admission with
//! `source = DeclRef(EmblaOptionsType)` and `mapper.key_space =
//! Exclude<keyof DeclRef, K>`.
//!
//! Tier 1 (`source` Object) does not match — `source` is a DeclRef.
//! Tier 2 (`keyspace_admits_literal_non_emitting`) returns `None` for
//! the `Exclude<keyof DeclRef, K>` shape (the `base_member_admission_non_emitting`
//! arm falls through on `DeclRef`/`InstantiationRef`, see
//! `enumerate.rs:455`). Tier 3 returns `false` for non-primitives.
//!
//! `key_admitted == Some(false)`, `can_narrow == false`, and the
//! walker falls through to the **whole-surface MappedType
//! resolution** at `walk.rs:1091-1108` which dispatches
//! `SemanticQueryKey::MappedType { context: Published(Expanded) }` —
//! `build_mapped_type` then enumerates the entire EmblaOptionsType
//! keyspace and emits one `ProjectMember` edge per key, even for the
//! `axis` / `direction` / `container` / `slides` keys the Pick
//! deliberately excluded.
//!
//! Per codex 6th-consult Q1-X (BINDING):
//!
//! > X needs `DeclRef`/`keyof` membership via `MemberPresence` facts
//! > without whole-keyspace publication. Route fact-backed
//! > `MemberPresence` membership into the walker, or leave the
//! > carrier unresolved.
//!
//! Post-Round-11 Commit 3 the non-emitting predicate consults
//! `FactKey::MemberPresence` facts for `DeclRef` / `InstantiationRef`
//! shapes directly. The walker NEVER falls through to the whole-
//! surface MappedType dispatch when admission is provable from
//! parse-fact membership.
//!
//! ## Hermetic shape
//!
//! Mirrors the corpus Carousel: a generic interface extending
//! `Omit<DeclRefLib, …>` consumed by `defineProps<X<T>>()`. The library
//! type's OMITTED members must not be emitted by the walker's
//! Mapped-admission fallback.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "component_meta_audit/harness.rs"]
mod harness;

use verter_audit::origin_graph::{OriginEdgeKind, OriginEdgeMetaDto};
use verter_session::audited_request::AuditedRequest;

// Hand-derived Embla `EmblaOptionsType`-shape. Wide enough that the
// whole-keyspace enumeration fans into many uniquely-named members.
// Each member name appears only here and as an inherited published-
// surface key — never as a Vue SFC reserved name — so any
// `ProjectMember` edge naming any of the OMITTED ones is a Chain-X
// Rule-5 leak.
const EMBLA_OPTIONS_TS: &str = r#"
export interface CarouselItem {
  id?: string;
}
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

// Mirrors the corpus Carousel: a generic interface extending
// `Omit<DeclRefLib, …>` consumed by `defineProps<X<T>>()` through
// `<script setup generic="…">`. The macro publication's surface
// composition then routes the OWN prop `items` through the PathWalker;
// the walker's Mapped admission visits `Pick<X, Exclude<keyof X, K>>`
// with `source = DeclRef(EmblaOptionsType)`, fails Tier 2 / Tier 3,
// falls through to `walk.rs:1091`'s whole-surface MappedType dispatch
// under `Published(Expanded)` — emitting per-key edges for EVERY
// EmblaOptionsType member including the OMITTED ones.
const CAROUSEL_VUE: &str = r#"<script lang="ts">
import type { EmblaOptionsType, CarouselItem } from './embla_options';

export interface CarouselProps<T extends CarouselItem = CarouselItem>
  extends Omit<EmblaOptionsType, 'axis' | 'direction'> {
  items?: T[];
}
</script>
<script setup lang="ts" generic="T extends CarouselItem">
defineProps<CarouselProps<T>>();
</script>
<template><div></div></template>
"#;

#[test]
fn chain_x_declref_keyspace_admission_does_not_enumerate_library_members() {
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

    // Members EXPLICITLY OMITTED on the inherited Omit<EmblaOptionsType,
    // 'axis' | 'direction'>. These names exist in EmblaOptionsType's body
    // but NOT on CarouselProps' published surface. Any `ProjectMember`
    // edge naming them is the Chain-X whole-keyspace enumeration leak.
    const OMITTED_MEMBERS: &[&str] = &["axis", "direction"];

    let mut omitted_edge_count = 0usize;
    let mut omitted_edge_names: Vec<String> = Vec::new();
    for edge in footprint.derivation_subgraph.edges.iter() {
        if !matches!(edge.kind, OriginEdgeKind::ProjectMember) {
            continue;
        }
        if let OriginEdgeMetaDto::ProjectMember { member_name } = &edge.meta {
            if OMITTED_MEMBERS.contains(&member_name.as_ref()) {
                omitted_edge_count += 1;
                omitted_edge_names.push(member_name.to_string());
            }
        }
    }

    let mut omitted_path_count = 0usize;
    for projection in footprint.projections.iter() {
        for seg in projection.path.iter() {
            if let verter_audit::origin_graph::ProjectPathSegment::Member { name } = seg {
                if OMITTED_MEMBERS.contains(&name.as_ref()) {
                    omitted_path_count += 1;
                }
            }
        }
    }

    let total = omitted_edge_count + omitted_path_count;

    assert_eq!(
        total, 0,
        "Block 6.i Round 11 Chain X — the PathWalker's Mapped \
         admission on a `Omit<DeclRefLib, K>` inherited carrier MUST \
         NOT emit `ProjectMember` edges for the OMITTED library \
         members. Tier 2's `keyspace_admits_literal_non_emitting` and \
         `base_member_admission_non_emitting` must consult \
         `FactKey::MemberPresence` facts for `DeclRef` / \
         `InstantiationRef` bases — admitting structurally when the \
         fact says the library has the needle; refuting otherwise. \
         The walker MUST NOT fall through to the whole-surface \
         MappedType dispatch (`walk.rs:1091`) for admission tests. \
         Got: leak_edges={omitted_edge_count} \
         (names={omitted_edge_names:?}), \
         projection_path_member_hits={omitted_path_count}. \
         Pre-Round-11 `base_member_admission_non_emitting` returned \
         `None` for `DeclRef` / `InstantiationRef` shapes \
         (`enumerate.rs:455`), Tier 3 returned `false` for non-\
         primitives, `key_admitted == Some(false)`, `can_narrow == \
         false`, and the walker fell through to whole-surface \
         MappedType under `Published(Expanded)` — emitting per-key \
         edges for EVERY EmblaOptionsType member including the OMITTED \
         `axis` / `direction`. See `D:/tmp/round10-review-codex-out.txt` \
         Q1-X (BINDING) and `D:/tmp/round11-implementer-brief.md` \
         Commit 3 spec."
    );
}
