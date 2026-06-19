//! Resolver coverage seed for the cross-package wildcard re-export ×
//! utility-wrapped instantiated generic gap (CP1).
//!
//! ## Bug pattern
//!
//! Applying a utility-type wrapper (`Omit`, `Pick`, …) over an
//! INSTANTIATED imported generic ref whose declaration canonical lives
//! in a cross-package source (regardless of whether the re-export hop
//! is wildcard or named) resolves to
//! `TypeExpr::Unknown { raw: "semanticMiss" }` instead of returning a
//! real shape. This characterises the per-prop `kind=unknown` /
//! `raw=semanticMiss` regression observed on nuxt-ui `Table.vue` (~19
//! of 45 published props).
//!
//! ## Discrimination table
//!
//! | # | name                                                              | composition                                                                | baseline expectation |
//! | - | ----------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------- |
//! | 1 | `bare_instantiated_generic_via_wildcard_resolves`                 | `export *` re-export + bare instantiated generic                          | PASS (sanity)        |
//! | 2 | `omit_wrapped_non_generic_via_wildcard_resolves`                  | `export *` re-export + utility-wrapped NON-generic interface              | PASS (sanity)        |
//! | 3 | `omit_wrapped_instantiated_generic_via_wildcard_resolves`         | `export *` re-export + utility-wrapped instantiated generic               | FAIL pre-fix         |
//! | 4 | `omit_wrapped_instantiated_generic_via_named_reexport_fails`      | named re-export (`export { Foo } from ...`) + utility × instantiated      | FAIL pre-fix         |
//! | 5 | `omit_wrapped_sfc_generic_param_via_wildcard_resolves`            | `export *` re-export + utility × instantiated using SFC `generic="T"`     | FAIL pre-fix         |
//!
//! Tests 1+2 prove the wildcard re-export hop is healthy and that
//! utility-wrap over a non-generic cross-package ref works. Tests 3–5
//! pin the COMPOSITION (utility × instantiation × cross-package) as
//! the failing axis.
//!
//! ## Hermetic constraint
//!
//! All fixtures resolve against `build_hermetic_host_with_lib` with
//! the ambient lib stub. The "cross-package" hop is modelled by
//! seating the source declaration under `/ws/node_modules/<pkg>/` so
//! the resolver's package-backed walk is exercised the same way it
//! is for a real `node_modules/` dependency.

use super::harness::{build_hermetic_host_with_lib, resolve_under_audit, STUB_LIB_ES5};

/// Returns a debug representation of the prop's resolved `type_expr`,
/// or `None` if the prop is missing. Used to discriminate
/// `TypeExpr::Unknown { raw: "semanticMiss" }` (the cross-package
/// utility-wrapped-generic give-up) from a real resolved shape.
fn prop_type_repr(
    analysis: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    name: &str,
) -> Option<String> {
    analysis
        .props
        .iter()
        .find(|p| p.name.as_str() == name)
        .map(|p| format!("{:?}", p.type_expr))
}

/// Assert the named prop exists AND resolved to a real shape — i.e.
/// the prop's `type_expr` is NOT `TypeExpr::Unknown { raw:
/// "semanticMiss" }` and not an `Unknown` shell. Either condition is
/// the cross-package utility-wrapped-generic give-up.
fn assert_prop_resolved(
    analysis: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    name: &str,
) {
    let repr = prop_type_repr(analysis, name).unwrap_or_else(|| {
        let all: Vec<&str> = analysis.props.iter().map(|p| p.name.as_str()).collect();
        panic!("prop `{name}` must surface; published prop names: {all:?}")
    });
    assert!(
        !repr.contains("semanticMiss") && !repr.contains("Unknown"),
        "prop `{name}` must resolve to a real type shape (not Unknown / semanticMiss), got: {repr}"
    );
}

// ── Sanity #1: bare instantiated generic via wildcard re-export ──────────────
//
// `Foo<Item[]>` (no utility wrap) imported through a cross-package
// `export *` barrel. The macro lowers to `Ref { name: "Foo", args:
// [Item[]] }`, the registry route is not taken (no Pick/Omit), and
// the prepared decl for `Foo` is recovered via the wildcard hop.
// Published prop names must include `label` (the bare member of the
// underlying `Foo<T>` interface).

const SANITY_BARE_VUE: &str = r#"<script setup lang="ts">
import type { Foo, Item } from 'pkg-types';
defineProps<Foo<Item[]>>();
</script>
<template><div /></template>
"#;

const SANITY_BARE_TS: &str = r#"export * from './foo';
export * from './item';
"#;

const SANITY_BARE_FOO_TS: &str = r#"export interface Foo<T extends Item[] = Item[]> {
  label?: string;
  items?: T;
}
"#;

const SANITY_BARE_ITEM_TS: &str = r#"export interface Item {
  id: string;
}
"#;

#[test]
fn bare_instantiated_generic_via_wildcard_resolves() {
    let host = build_hermetic_host_with_lib(
        &[
            ("/ws/src/c.vue", SANITY_BARE_VUE),
            ("/ws/node_modules/pkg-types/index.d.ts", SANITY_BARE_TS),
            ("/ws/node_modules/pkg-types/foo.d.ts", SANITY_BARE_FOO_TS),
            ("/ws/node_modules/pkg-types/item.d.ts", SANITY_BARE_ITEM_TS),
            (
                "/ws/node_modules/pkg-types/package.json",
                r#"{"name":"pkg-types","types":"./index.d.ts"}"#,
            ),
        ],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/ws/src/c.vue");

    // Discriminating positive: `label` is the explicit own-member of
    // `Foo<T>`. It MUST surface as a published prop with a real
    // (non-semanticMiss) kind.
    assert_prop_resolved(&analysis, "label");
}

// ── Sanity #2: utility-wrapped NON-generic interface via wildcard ────────────
//
// `Omit<Bar, 'omitted'>` where `Bar` is a NON-generic interface
// re-exported through `export *`. The registry's Omit route takes
// `Bar` as the root_symbol. Pre-fix this works; the bug is
// instantiation-specific.

const SANITY_NONGEN_VUE: &str = r#"<script setup lang="ts">
import type { Bar } from 'pkg-types';
defineProps<Omit<Bar, 'omitted'>>();
</script>
<template><div /></template>
"#;

const SANITY_NONGEN_TS: &str = r#"export * from './bar';
"#;

const SANITY_NONGEN_BAR_TS: &str = r#"export interface Bar {
  kept: string;
  omitted: number;
  also_kept: boolean;
}
"#;

#[test]
fn omit_wrapped_non_generic_via_wildcard_resolves() {
    let host = build_hermetic_host_with_lib(
        &[
            ("/ws/src/c.vue", SANITY_NONGEN_VUE),
            ("/ws/node_modules/pkg-types/index.d.ts", SANITY_NONGEN_TS),
            ("/ws/node_modules/pkg-types/bar.d.ts", SANITY_NONGEN_BAR_TS),
            (
                "/ws/node_modules/pkg-types/package.json",
                r#"{"name":"pkg-types","types":"./index.d.ts"}"#,
            ),
        ],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/ws/src/c.vue");
    let names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();

    // Discriminating positive: the two non-omitted members surface
    // with real shape.
    assert_prop_resolved(&analysis, "kept");
    assert_prop_resolved(&analysis, "also_kept");

    // Discriminating negative: the omitted member is NOT published.
    assert!(
        !names.iter().any(|n| n == "omitted"),
        "Omit<Bar,'omitted'> must drop `omitted`; got {names:?}"
    );
}

// ── Failure #3: utility × instantiated generic via wildcard re-export ────────
//
// `Omit<Foo<Item[]>, 'items'>`: utility wrap over an INSTANTIATED
// imported generic ref. The inner `Foo<Item[]>` is an Instantiate
// shell whose decl canonical lives across a wildcard re-export hop.
// Pre-fix: the prepared decl for `Foo` is not recovered via the
// utility-arg lowering path, the Instantiate base stays Opaque(Miss),
// and `label` publishes as `semanticMiss`.

const FAIL_WILDCARD_VUE: &str = r#"<script setup lang="ts">
import type { Foo, Item } from 'pkg-types';
defineProps<Omit<Foo<Item[]>, 'items'>>();
</script>
<template><div /></template>
"#;

const FAIL_WILDCARD_TS: &str = r#"export * from './foo';
export * from './item';
"#;

const FAIL_WILDCARD_FOO_TS: &str = r#"export interface Foo<T extends Item[] = Item[]> {
  label?: string;
  items?: T;
}
"#;

const FAIL_WILDCARD_ITEM_TS: &str = r#"export interface Item {
  id: string;
}
"#;

#[test]
fn omit_wrapped_instantiated_generic_via_wildcard_resolves() {
    let host = build_hermetic_host_with_lib(
        &[
            ("/ws/src/c.vue", FAIL_WILDCARD_VUE),
            ("/ws/node_modules/pkg-types/index.d.ts", FAIL_WILDCARD_TS),
            ("/ws/node_modules/pkg-types/foo.d.ts", FAIL_WILDCARD_FOO_TS),
            (
                "/ws/node_modules/pkg-types/item.d.ts",
                FAIL_WILDCARD_ITEM_TS,
            ),
            (
                "/ws/node_modules/pkg-types/package.json",
                r#"{"name":"pkg-types","types":"./index.d.ts"}"#,
            ),
        ],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/ws/src/c.vue");
    let names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();

    // Discriminating positive: the kept (non-omitted) member resolves
    // with a real shape — NOT `semanticMiss`.
    assert_prop_resolved(&analysis, "label");

    // Discriminating negative: the omitted key is absent.
    assert!(
        !names.iter().any(|n| n == "items"),
        "Omit<Foo<Item[]>,'items'> must drop `items`; got {names:?}"
    );
}

// ── Failure #4: utility × instantiated generic via NAMED re-export ───────────
//
// Same composition as #3 but the cross-package barrel uses
// `export { Foo, Item } from './foo'` instead of `export *`. The bug
// is wildcard-agnostic: the symmetry test confirms it lives upstream
// of the import-kind distinction and downstream of utility composition.

const FAIL_NAMED_VUE: &str = r#"<script setup lang="ts">
import type { Foo, Item } from 'pkg-types';
defineProps<Omit<Foo<Item[]>, 'items'>>();
</script>
<template><div /></template>
"#;

const FAIL_NAMED_TS: &str = r#"export { Foo } from './foo';
export { Item } from './item';
"#;

const FAIL_NAMED_FOO_TS: &str = r#"export interface Foo<T extends Item[] = Item[]> {
  label?: string;
  items?: T;
}
import type { Item } from './item';
"#;

const FAIL_NAMED_ITEM_TS: &str = r#"export interface Item {
  id: string;
}
"#;

#[test]
fn omit_wrapped_instantiated_generic_via_named_reexport_fails() {
    let host = build_hermetic_host_with_lib(
        &[
            ("/ws/src/c.vue", FAIL_NAMED_VUE),
            ("/ws/node_modules/pkg-types/index.d.ts", FAIL_NAMED_TS),
            ("/ws/node_modules/pkg-types/foo.d.ts", FAIL_NAMED_FOO_TS),
            ("/ws/node_modules/pkg-types/item.d.ts", FAIL_NAMED_ITEM_TS),
            (
                "/ws/node_modules/pkg-types/package.json",
                r#"{"name":"pkg-types","types":"./index.d.ts"}"#,
            ),
        ],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/ws/src/c.vue");
    let names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();

    // Discriminating positive: the kept member resolves under named
    // re-export too — the bug is NOT wildcard-specific.
    assert_prop_resolved(&analysis, "label");

    // Discriminating negative: the omitted key is absent.
    assert!(
        !names.iter().any(|n| n == "items"),
        "Omit<Foo<Item[]>,'items'> must drop `items` via named re-export; got {names:?}"
    );
}

// ── Failure #5: utility × instantiated generic with SFC `generic="T"` ────────
//
// The SFC uses `<script setup generic="T extends Item[]">` and binds
// `T` into the utility-wrapped instantiated generic. Pre-fix the
// inner `Foo<T>` Instantiate fails to recover the prepared decl
// across the wildcard re-export hop and publishes `label` as
// `semanticMiss`.

const FAIL_SFC_GENERIC_VUE: &str = r#"<script setup lang="ts" generic="T extends Item[]">
import type { Foo, Item } from 'pkg-types';
defineProps<Omit<Foo<T>, 'items'>>();
</script>
<template><div /></template>
"#;

const FAIL_SFC_GENERIC_TS: &str = r#"export * from './foo';
export * from './item';
"#;

const FAIL_SFC_GENERIC_FOO_TS: &str = r#"export interface Foo<T extends Item[] = Item[]> {
  label?: string;
  items?: T;
}
"#;

const FAIL_SFC_GENERIC_ITEM_TS: &str = r#"export interface Item {
  id: string;
}
"#;

#[test]
fn omit_wrapped_sfc_generic_param_via_wildcard_resolves() {
    let host = build_hermetic_host_with_lib(
        &[
            ("/ws/src/c.vue", FAIL_SFC_GENERIC_VUE),
            ("/ws/node_modules/pkg-types/index.d.ts", FAIL_SFC_GENERIC_TS),
            (
                "/ws/node_modules/pkg-types/foo.d.ts",
                FAIL_SFC_GENERIC_FOO_TS,
            ),
            (
                "/ws/node_modules/pkg-types/item.d.ts",
                FAIL_SFC_GENERIC_ITEM_TS,
            ),
            (
                "/ws/node_modules/pkg-types/package.json",
                r#"{"name":"pkg-types","types":"./index.d.ts"}"#,
            ),
        ],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/ws/src/c.vue");
    let names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();

    // Discriminating positive: SFC `generic="T"` parameter resolves
    // through the utility-wrap composition and publishes `label` as a
    // real shape.
    assert_prop_resolved(&analysis, "label");

    // Discriminating negative: the omitted key is absent.
    assert!(
        !names.iter().any(|n| n == "items"),
        "Omit<Foo<T>,'items'> must drop `items` with SFC generic; got {names:?}"
    );
}
