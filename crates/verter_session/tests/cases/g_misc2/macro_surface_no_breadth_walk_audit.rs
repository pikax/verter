//! Discriminating guard for the CLAUDE.md CRITICAL macro-traversal rule
//! ("Do not walk unrelated imports"): a `defineProps<T>()` macro surface must
//! follow ONLY the import graph reachable from the requested type's declaration
//! graph, never breadth-walk the owner SFC's OTHER (unrelated) imports.
//!
//! Unlike the `u3c_chatmessages_audit` gate — which polices breadth on
//! the real ChatMessages fixture whose package imports are unresolvable in the
//! hermetic setup (so they never produce a probe) — this is a MINIMAL,
//! controlled fixture where the unrelated import is a RESOLVABLE workspace file.
//!
//! Observable note: under the unified cold build, ANY file whose export
//! surface a request inspects owns exactly one `IndexedReady` — the owner's
//! direct imports are inspected at depth 1 (the imported-root registry +
//! dependency fact capture read their export surfaces), so an
//! `IndexedReady` build of a DIRECT import is not by itself a breadth
//! walk. What the macro-traversal rule forbids — and what this gate pins —
//! is TYPE-RESOLVING the unrelated declaration graph: no member of the
//! unrelated type may leak into the surface, no instantiation may root in
//! the unrelated file, and the build set must stay exactly the depth-1
//! inspected set (no transitive fan-out through the unrelated file).
//!
//! Mutation-probe (proves the test discriminates): change `defineProps<Props>`
//! to `defineProps<Props & Unrelated>` — `/unrelated.ts` then becomes part of
//! the requested type's declaration graph: `secret` enters the props surface
//! (firing the leak assertion) AND the resolver deepens through
//! `/unrelated.ts` into `/unrelated_dep.ts` (firing the build-set
//! never-build assertion INDEPENDENTLY — verified with the surface-leak and
//! instantiation asserts neutralised). Reverting restores green. Verified
//! during authoring (re-verified after the unified-cold-build re-aim).

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use super::harness::{build_hermetic_host, footprint_of, resolve_under_audit};

/// The owner SFC's props type lives in `/related.ts`. `/unrelated.ts` is
/// imported into the SFC scope but is NEVER referenced by any macro — it is
/// outside the `Props` declaration graph and must not be walked.
const RELATED_TS: &str = r#"export interface Props {
  label?: string
  count?: number
}
"#;

/// `/unrelated.ts` carries a TRANSITIVE dependency (`/unrelated_dep.ts`,
/// referenced by a member type) so the build-set axis is falsifiable: a
/// breadth walk that deepens THROUGH the unrelated import must build the
/// transitive dep, breaching the depth-1 closed set below.
const UNRELATED_TS: &str = r#"import type { Dep } from './unrelated_dep'

export interface Unrelated {
  secret?: string
  dep?: Dep
}
"#;

const UNRELATED_DEP_TS: &str = r#"export interface Dep {
  hidden?: number
}
"#;

const APP_VUE: &str = r#"<script setup lang="ts">
import type { Props } from './related'
import type { Unrelated } from './unrelated'

// `Unrelated` is imported into scope but is NOT referenced by any macro type
// argument (it is intentionally imported-but-unused for the props request).
// A correct resolver never walks it into, or deepens it for, the props surface.
defineProps<Props>()
</script>
<template><div /></template>"#;

#[test]
fn macro_surface_does_not_breadth_walk_unrelated_imports() {
    let host = build_hermetic_host(&[
        ("/App.vue", APP_VUE),
        ("/related.ts", RELATED_TS),
        ("/unrelated.ts", UNRELATED_TS),
        ("/unrelated_dep.ts", UNRELATED_DEP_TS),
    ]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/App.vue");

    // Sanity: the requested props surface resolved (the related-import IS
    // followed — this is NOT a "resolve nothing" false pass).
    let prop_names: std::collections::BTreeSet<_> =
        analysis.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains("label") && prop_names.contains("count"),
        "the requested Props surface (from the reachable /related.ts) must resolve, got {prop_names:?}",
    );
    // The unrelated type must NOT leak into the props surface.
    assert!(
        !prop_names.contains("secret"),
        "the unrelated import's member must NOT appear in the props surface, got {prop_names:?}",
    );

    let fp = footprint_of(&record);

    // Positive control for the instantiation axis: the request DOES
    // instantiate types (the Props surface resolves through the typed-IR
    // dispatch), so axis (a) below cannot be vacuously green from an
    // empty instantiation footprint. (The recorded decl provenance can be
    // empty for an instantiation whose source node carries no named
    // identity — a breadth-walk instantiation of the NAMED `Unrelated`
    // decl would carry `/unrelated.ts`, which axis (a) checks; the
    // named-identity extraction itself is pinned by the footprint-miner
    // unit tests in `component_meta_audit::assertions`.)
    assert!(
        !fp.instantiations.is_empty(),
        "positive control: resolving Props must record at least one \
         instantiation — an empty footprint would make the \
         no-unrelated-instantiation gate vacuous",
    );

    // CORE no-breadth-walk gate (post-unification observables):
    //
    // (a) No TYPE RESOLUTION may root in the unrelated file — `Unrelated`
    //     is outside the `Props` declaration graph, so no instantiation
    //     step may carry `/unrelated.ts` as its declaring canonical.
    assert!(
        !fp.instantiations.iter().any(|i| {
            i.decl_canonical_id.as_ref() == "/unrelated.ts"
                || i.decl_canonical_id.as_ref() == "/unrelated_dep.ts"
        }),
        "no-breadth-walk: resolving Props must NOT instantiate any type \
         declared in `/unrelated.ts` or its transitive dep — both are \
         outside the Props declaration graph. Instantiations observed: {:?}",
        fp.instantiations
            .iter()
            .map(|i| (i.decl_canonical_id.as_ref(), i.decl_symbol_name.as_ref()))
            .collect::<Vec<_>>(),
    );
    // (b) The build set stays exactly the depth-1 inspected set — the
    //     owner, plus its direct imports whose export surfaces the
    //     imported-root registry / dependency fact capture read. NO
    //     transitive fan-out: deepening THROUGH `/unrelated.ts` (into
    //     `/unrelated_dep.ts`, its transitive dep — present in the
    //     fixture, NEVER in the allowlist) breaches this closed set, so
    //     the subset assertion is falsifiable by construction.
    let irb: std::collections::BTreeSet<String> = fp
        .indexed_ready_builds
        .iter()
        .map(|b| b.canonical_id.as_ref().to_string())
        .collect();
    let allowed: std::collections::BTreeSet<String> = ["/App.vue", "/related.ts", "/unrelated.ts"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert!(
        !irb.contains("/unrelated_dep.ts"),
        "no-breadth-walk: `/unrelated_dep.ts` (the unrelated import's \
         transitive dep) must NEVER be built during the macro-surface \
         request — building it means the resolver deepened THROUGH the \
         unrelated import; observed build set {irb:?}",
    );
    assert!(
        irb.is_subset(&allowed),
        "no-breadth-walk: the IndexedReady build set must stay within the \
         depth-1 inspected set {allowed:?}; observed {irb:?}",
    );
    // Per-request build-counter bound: the depth-1 inspected set is the
    // whole budget — any breadth walk shows up as a fourth build even if
    // a future fixture edit accidentally widens the allowlist.
    assert!(
        fp.indexed_ready_builds.len() <= 3,
        "no-breadth-walk: at most 3 IndexedReady builds (owner + 2 direct \
         imports) per macro-surface request; observed {} ({irb:?})",
        fp.indexed_ready_builds.len(),
    );

    // Reachability sanity: the RELATED file (the one the requested type actually
    // lives in) was followed — this proves the no-breadth-walk gate above is
    // discriminating, not vacuously green from a resolver that walks nothing.
    // (The owner SFC + its reachable props-type file are the only files the
    // declaration graph reaches; the unrelated import is excluded.)
    assert!(
        analysis
            .props
            .iter()
            .any(|p| p.name == "label" || p.name == "count"),
        "the reachable props-type file (/related.ts) must have been followed",
    );
}
