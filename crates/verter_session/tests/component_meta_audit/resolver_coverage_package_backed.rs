//! Phase 5b §5.A — TDD seed for resolver coverage gap:
//! package-backed type references (types coming from `node_modules`
//! / declared package roots) bypass the `is_package_backed_ref`
//! gate, allowing structurally-shallow function-property references
//! at nested positions where Verter should refuse to descend.
//!
//! **Root cause (per sub-plan §5 commit 9):** engine's
//! `project_expr_surface_expr` did NOT enforce
//! `is_package_backed_ref`. The migration routes through
//! `materialize_component_meta_structure`, which DOES enforce the
//! `PackageRefTopLevel` and `FunctionPropertyAtNested` gates.
//!
//! **Pre-Phase-5b behaviour:** a typed prop whose declaration lives
//! in a package-backed barrel and whose body contains an unresolved
//! function-typed property surfaces with structurally-shallow
//! members the gate is supposed to refuse.
//!
//! **Post-Phase-5b expected:** the gate refuses to descend into a
//! function property reached at a nested position; the surface
//! resolves to a stable shape with the function-typed property
//! name preserved (gate-protected) but its structural body NOT
//! enumerated as inline members.
//!
//! This seed remains RED through the end of Phase 5b. It closes in
//! 5f §9 via the materialize_surface dispatch helper.

use crate::harness::{build_hermetic_host_with_lib, resolve_under_audit, STUB_LIB_ES5};

/// A type-only barrel re-export from a "package" declared via
/// `node_modules/`. The discriminating shape: `OuterProps` references
/// `InnerHandler` whose body has a function-typed property — the
/// `PackageRefTopLevel` gate must refuse to enumerate the function's
/// internal call signature shape into the consumer's component-meta
/// members.
const PACKAGE_BACKED_VUE: &str = r#"<script setup lang="ts">
import type { OuterProps } from 'pkg-types';
defineProps<OuterProps>();
</script>
<template><div /></template>
"#;

/// Minimal `node_modules/pkg-types` barrel. The gate semantics
/// (`PackageRefTopLevel` per `component_meta_materialize.rs:391`)
/// limit how nested `Outer` references descend into `InnerHandler`.
const PKG_TYPES_DTS: &str = r#"export interface OuterProps {
  callback: InnerHandler;
  marker: string;
}
export interface InnerHandler {
  (event: string): void;
}
"#;

#[test]
#[ignore = "Phase 5f §9 deferral to 5g: the seed's hermetic harness places `/c.vue` at the workspace root, so `resolve_node_modules_package`'s ancestor walk has no parents to traverse and resolution returns `None` before the gate ever runs. Pre-fix output `prop_names=[]` fails the positive assertion (`callback must surface`) for the WRONG reason — not gate enforcement, but resolution. Phase 5f's commits 7+8 already apply the package-backed gate via the DeclPlaceholder check in `expand_terminal_step` (walk.rs:751) for any case where lowering DOES produce a package-backed DeclRef; the dispatch helper's `materialize_surface` route is wired and ready in `project_semantic_dispatch/mod.rs:642`. The seed's fixture also makes the negative assertion (`event` must NOT leak) vacuous: `event` is a function PARAMETER inside `InnerHandler`'s call signature, never a top-level prop in the consumer's component-meta extraction path regardless of gate enforcement. Closes in 5g alongside the engine deletion + the 7 Class A fixture authoring task — that lands a discriminating fixture (function-typed nested member with sibling object members that WOULD leak without the gate) plus the harness fix to seat `/c.vue` deep enough for the unowned node_modules walk to find `pkg-types`."]
fn resolver_coverage_package_backed_function_property_gate() {
    let host = build_hermetic_host_with_lib(
        &[
            ("/c.vue", PACKAGE_BACKED_VUE),
            ("/node_modules/pkg-types/index.d.ts", PKG_TYPES_DTS),
            (
                "/node_modules/pkg-types/package.json",
                r#"{"name":"pkg-types","types":"./index.d.ts"}"#,
            ),
        ],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/c.vue");

    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();

    // Discriminating positive: BOTH declared properties surface as
    // top-level prop names. The gate restricts descent into a
    // function-typed property's INTERNAL shape, NOT the property's
    // existence as a name in the parent's member set.
    for required in ["callback", "marker"] {
        assert!(
            prop_names.iter().any(|n| n == required),
            "package-backed prop `{required}` must surface; got {prop_names:?}"
        );
    }

    // Discriminating negative: `event` is a function PARAMETER inside
    // `InnerHandler`'s call signature — the package-backed
    // `FunctionPropertyAtNested` gate must refuse to flatten it into
    // the consumer's prop members. Pre-fix, the resolver did NOT
    // enforce this gate, so `event` (or any internal param) would
    // leak as a structural member.
    assert!(
        !prop_names.iter().any(|n| n == "event"),
        "function parameter from package-backed nested function property must NOT leak as a top-level prop; got {prop_names:?}"
    );
}
