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
//! **Post-Phase-5l behaviour (harness fix re-homed from §5.13 r15):**
//! the SFC is seated at `/ws/src/c.vue` so the unowned node_modules
//! walk has parent directories to traverse (`/ws/src` → `/ws`),
//! reaching `/ws/node_modules/pkg-types/`. The package's body is
//! shaped so `OuterProps` has a sibling object-typed member
//! (`nested: NestedExtras`) whose properties WOULD leak as flattened
//! members at the top level if the resolver did not enforce the
//! `FunctionPropertyAtNested` / nested-package-backed gate. The
//! function-typed sibling (`callback: InnerHandler`) exercises the
//! function-shape skip at the nested axis.

use crate::harness::{build_hermetic_host_with_lib, resolve_under_audit, STUB_LIB_ES5};

/// SFC seated at `/ws/src/c.vue` — `ancestor_dirs` returns
/// `["/ws/src", "/ws"]` so `resolve_node_modules_package` walks
/// `/ws/src/node_modules/` (miss) → `/ws/node_modules/` (hit at
/// `pkg-types`). Pre-harness-fix, the SFC was at workspace root
/// `/c.vue` and `parent_dir("/c.vue")` returned `""`, leaving the
/// ancestor list empty and resolution returning `None`.
const PACKAGE_BACKED_VUE: &str = r#"<script setup lang="ts">
import type { OuterProps } from 'pkg-types';
defineProps<OuterProps>();
</script>
<template><div /></template>
"#;

/// Discriminating `node_modules/pkg-types` body. `OuterProps` has
/// THREE members: a function-typed sibling (`callback: InnerHandler`),
/// an OBJECT-typed sibling (`nested: NestedExtras`), and a primitive
/// (`marker: string`). The OBJECT-typed sibling's body contains
/// fields (`leak_field`, `leak_event`) that WOULD merge into the
/// top-level `OuterProps` shape if the resolver descended into the
/// package-backed nested member instead of keeping it symbolic via
/// the gate. The function-typed sibling's call signature parameter
/// (`event`) provides the secondary negative assertion against
/// function-body flattening.
const PKG_TYPES_DTS: &str = r#"export interface OuterProps {
  callback: InnerHandler;
  nested: NestedExtras;
  marker: string;
}
export interface NestedExtras {
  leak_field: string;
  leak_event: number;
}
export interface InnerHandler {
  (event: string): void;
}
"#;

#[test]
fn resolver_coverage_package_backed_function_property_gate() {
    let host = build_hermetic_host_with_lib(
        &[
            ("/ws/src/c.vue", PACKAGE_BACKED_VUE),
            ("/ws/node_modules/pkg-types/index.d.ts", PKG_TYPES_DTS),
            (
                "/ws/node_modules/pkg-types/package.json",
                r#"{"name":"pkg-types","types":"./index.d.ts"}"#,
            ),
        ],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/ws/src/c.vue");

    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();

    // Discriminating positive: the THREE declared properties of
    // `OuterProps` (`callback`, `nested`, `marker`) surface as
    // top-level prop names. The gate restricts descent into nested
    // package-backed bodies, NOT the existence of the parent's own
    // members.
    for required in ["callback", "nested", "marker"] {
        assert!(
            prop_names.iter().any(|n| n == required),
            "package-backed prop `{required}` must surface; got {prop_names:?}"
        );
    }

    // Discriminating negative #1: the OBJECT-typed sibling
    // `nested: NestedExtras` has properties (`leak_field`,
    // `leak_event`) whose names WOULD appear at the top level if
    // the resolver flattened the package-backed nested body.
    // The package-ref gate keeps `NestedExtras` symbolic at the
    // nested axis; its members must NOT leak as siblings of
    // `nested`.
    for must_not in ["leak_field", "leak_event"] {
        assert!(
            !prop_names.iter().any(|n| n == must_not),
            "package-backed nested object member `{must_not}` must NOT leak as a top-level prop; got {prop_names:?}"
        );
    }

    // Discriminating negative #2: the function-typed sibling
    // `callback: InnerHandler` has a call-signature parameter
    // (`event`). The function-shape skip at the nested axis keeps
    // the function body symbolic; the parameter must NOT leak as
    // a top-level prop.
    assert!(
        !prop_names.iter().any(|n| n == "event"),
        "function parameter from package-backed nested function property must NOT leak as a top-level prop; got {prop_names:?}"
    );
}
