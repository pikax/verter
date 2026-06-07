//! Discriminating guard for the CLAUDE.md CRITICAL macro-traversal rule
//! ("Do not walk unrelated imports"): a `defineProps<T>()` macro surface must
//! follow ONLY the import graph reachable from the requested type's declaration
//! graph, never breadth-walk the owner SFC's OTHER (unrelated) imports.
//!
//! Unlike the `u3c_chatmessages_audit` gate — which polices breadth on
//! the real ChatMessages fixture whose package imports are unresolvable in the
//! hermetic setup (so they never produce a probe) — this is a MINIMAL,
//! controlled fixture where the unrelated import is a RESOLVABLE workspace file.
//! That makes the test discriminating: if the resolver breadth-walks, the
//! unrelated file's IndexedReady WILL be built and the gate fails.
//!
//! Mutation-probe (proves the test discriminates): change `defineProps<Props>`
//! to `defineProps<Props & Unrelated>` — `/unrelated.ts` then becomes part of
//! the requested type's declaration graph, its IndexedReady is built, `secret`
//! enters the props surface, and BOTH assertions fire. Reverting restores
//! green. Verified during authoring (see the test-module comment on the probe).

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use crate::harness::{build_hermetic_host, footprint_of, resolve_under_audit};

/// The owner SFC's props type lives in `/related.ts`. `/unrelated.ts` is
/// imported into the SFC scope but is NEVER referenced by any macro — it is
/// outside the `Props` declaration graph and must not be walked.
const RELATED_TS: &str = r#"export interface Props {
  label?: string
  count?: number
}
"#;

const UNRELATED_TS: &str = r#"export interface Unrelated {
  secret?: string
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

    // CORE no-breadth-walk gate: `/unrelated.ts` is reachable (resolvable) but
    // outside the `Props` declaration graph, so resolving App.vue's
    // component-meta must NEVER build its IndexedReady. The related file
    // (`/related.ts`) and the owner (`/App.vue`) may build; `/unrelated.ts`
    // must not.
    let irb: Vec<String> = fp
        .indexed_ready_builds
        .iter()
        .map(|b| b.canonical_id.as_ref().to_string())
        .collect();
    assert!(
        !irb.iter().any(|c| c == "/unrelated.ts"),
        "no-breadth-walk: resolving Props must NOT build the unrelated import's \
         IndexedReady — `/unrelated.ts` is outside the Props declaration graph. \
         IndexedReady builds observed: {irb:?}",
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
