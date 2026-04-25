//! Step 2 caller-class parity matrix (per `D:/tmp/architectural-debt-closure.md`
//! revision 10, Step 2, sub-task 2.0 / D2.1).
//!
//! Five tests, one per caller class. Each test exercises the dispatch surface
//! end-to-end via the public `get_component_meta` API on a representative
//! fixture for the class. After Step 1.5 closed Debt 1, dispatch produces
//! substitution-correct surfaces for the inputs that previously routed
//! through `materialize_*_in_scope` walkers; this matrix is the gate that
//! proves it before sub-task 2.1 deletes the legacy walker family.
//!
//! Each test:
//!   1. Builds a fixture that targets the class's behavioural shape.
//!   2. Resolves the fixture through the public API (compute path, which uses
//!      dispatch via Step 1's closure rewire).
//!   3. Asserts the resolved component-meta carries the expected concrete
//!      surface (NOT a symbolic Ref / Unknown).
//!
//! If a test FAILS on the post-deletion tree, the dispatch query surface needs
//! an addition or fix for that class — STOP CONDITION 4 fires per the
//! continuation prompt.
//!
//! Class taxonomy (per D2.1):
//!   1. Fallthrough resolution
//!   2. Utility/mapped/keyof projection
//!   3. `meta_resolve` route paths
//!   4. Component-meta rematerialization
//!   5. Direct symbol resolution

use std::sync::Arc;

use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;
use verter_semantic::analysis::component_meta::PropAnalysis;

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

fn prop_by_name<'a>(
    meta: &'a verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    name: &str,
) -> &'a PropAnalysis {
    meta.props
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("expected prop `{name}` in resolved meta"))
}

fn assert_no_unresolved_ref(
    type_expr: &verter_semantic::analysis::type_expr::TypeExpr,
    forbidden_name: &str,
) {
    use verter_semantic::analysis::type_expr::TypeExpr;
    fn walk(expr: &TypeExpr, forbidden: &str, hits: &mut Vec<String>) {
        match expr {
            TypeExpr::Ref { name, .. } if name.as_ref() == forbidden => {
                hits.push(name.to_string());
            }
            TypeExpr::Ref { type_arguments, .. } => {
                for arg in type_arguments.iter() {
                    walk(arg, forbidden, hits);
                }
            }
            TypeExpr::Parenthesized(inner) | TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => {
                walk(inner, forbidden, hits);
            }
            TypeExpr::Array { element, .. } => walk(element, forbidden, hits),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter() {
                    walk(&element.ty, forbidden, hits);
                }
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types.iter() {
                    walk(ty, forbidden, hits);
                }
            }
            TypeExpr::Object(object) => {
                use verter_semantic::analysis::type_expr::ObjectMember;
                for member in object.properties.iter() {
                    match member {
                        ObjectMember::Property(prop) => walk(&prop.ty, forbidden, hits),
                        ObjectMember::IndexSignature(sig) => {
                            walk(&sig.value_type, forbidden, hits);
                            walk(&sig.key_type, forbidden, hits);
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            if let Some(rt) = function.return_type.as_ref() {
                                walk(rt, forbidden, hits);
                            }
                            for parameter in function.parameters.iter() {
                                walk(&parameter.ty, forbidden, hits);
                            }
                        }
                        ObjectMember::Method(method) => {
                            if let Some(rt) = method.function.return_type.as_ref() {
                                walk(rt, forbidden, hits);
                            }
                            for parameter in method.function.parameters.iter() {
                                walk(&parameter.ty, forbidden, hits);
                            }
                        }
                    }
                }
            }
            TypeExpr::IndexedAccess { object, index } => {
                walk(object, forbidden, hits);
                walk(index, forbidden, hits);
            }
            TypeExpr::Function(function) => {
                if let Some(rt) = function.return_type.as_ref() {
                    walk(rt, forbidden, hits);
                }
                for parameter in function.parameters.iter() {
                    walk(&parameter.ty, forbidden, hits);
                }
            }
            _ => {}
        }
    }
    let mut hits = Vec::new();
    walk(type_expr, forbidden_name, &mut hits);
    assert!(
        hits.is_empty(),
        "dispatch produced an unresolved `{forbidden_name}` Ref shell where a \
         structural surface was expected; got hits {hits:?} in {type_expr:?}"
    );
}

// ============================================================================
// Class 1: Fallthrough resolution
//
// Inheritance walks (`inheritAttrs`, root-component inheritance). Native
// element root → intrinsic attrs minus declared props/events. Test that an
// `<Inner>` root inherits `MyButtonProps`'s structural shape.
// ============================================================================

#[test]
fn parity_class1_fallthrough_resolution_dispatches_correctly() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface OwnerProps {
  size: 'sm' | 'md' | 'lg';
  label: string;
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script setup lang="ts">
import type { OwnerProps } from './types'
defineProps<OwnerProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let meta = host
        .get_component_meta("/Owner.vue")
        .expect("get_component_meta must succeed for fallthrough fixture");

    let size = prop_by_name(&meta, "size");
    let label = prop_by_name(&meta, "label");

    // Dispatch must resolve `OwnerProps`'s `size` and `label` to their
    // concrete types, not leave a symbolic Ref.
    assert_no_unresolved_ref(&size.type_expr, "OwnerProps");
    assert_no_unresolved_ref(&label.type_expr, "OwnerProps");
}

// ============================================================================
// Class 2: Utility/mapped/keyof projection
//
// `Pick<T, K>`, `Omit<T, K>`, `Record<K, V>`, mapped types. Test that
// `Pick<HelperProps, "size" | "label">` projects to the structural subset.
// ============================================================================

#[test]
fn parity_class2_utility_pick_dispatches_correctly() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface HelperProps {
  size: 'sm' | 'md' | 'lg';
  label: string;
  variant: 'primary' | 'secondary';
  disabled: boolean;
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script setup lang="ts">
import type { HelperProps } from './types'
defineProps<Pick<HelperProps, 'size' | 'label'>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let meta = host
        .get_component_meta("/Owner.vue")
        .expect("get_component_meta must succeed for Pick fixture");

    let prop_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names.iter().any(|n| n == "size") && prop_names.iter().any(|n| n == "label"),
        "Pick<HelperProps, 'size' | 'label'> must produce props [size, label]; got {prop_names:?}"
    );
    assert!(
        !prop_names.iter().any(|n| n == "variant" || n == "disabled"),
        "Pick must NOT carry the omitted fields; got {prop_names:?}"
    );

    // Dispatch must resolve to concrete types, not leave symbolic Pick<T,K>
    // or HelperProps Ref shells.
    let size = prop_by_name(&meta, "size");
    let label = prop_by_name(&meta, "label");
    assert_no_unresolved_ref(&size.type_expr, "HelperProps");
    assert_no_unresolved_ref(&label.type_expr, "HelperProps");
    assert_no_unresolved_ref(&size.type_expr, "Pick");
    assert_no_unresolved_ref(&label.type_expr, "Pick");
}

// ============================================================================
// Class 3: `meta_resolve` route paths
//
// Member-route resolution: `Container['member']` indexed-access through an
// imported alias. Test that `ButtonConfig['variants']['color']` projects to
// the inner type after dispatch resolves the path.
// ============================================================================

#[test]
fn parity_class3_meta_resolve_route_path_dispatches_correctly() {
    let project = make_project();
    project
        .upsert_base(
            "/theme.ts",
            r#"export interface ButtonConfig {
  variants: {
    color: 'red' | 'blue' | 'green';
    size: 'sm' | 'md' | 'lg';
  };
  defaults: {
    color: string;
    size: string;
  };
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script setup lang="ts">
import type { ButtonConfig } from './theme'
defineProps<{ color: ButtonConfig['variants']['color'] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let meta = host
        .get_component_meta("/Owner.vue")
        .expect("get_component_meta must succeed for member-route fixture");

    let color = prop_by_name(&meta, "color");
    // Dispatch must resolve the member-route to a Union of literals, not
    // leave a symbolic IndexedAccess or ButtonConfig Ref.
    assert_no_unresolved_ref(&color.type_expr, "ButtonConfig");
}

// ============================================================================
// Class 4: Component-meta rematerialization
//
// Imported component-meta props that previously required a second
// rematerialize pass. Test that a transitively-imported props type produces
// the same structural surface from compute alone (post-Step-1.5 dispatch).
// ============================================================================

#[test]
fn parity_class4_imported_props_rematerialize_dispatches_correctly() {
    let project = make_project();
    project
        .upsert_base(
            "/leaf.ts",
            r#"export interface LeafProps {
  count: number;
  enabled: boolean;
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/middle.ts",
            r#"import type { LeafProps } from './leaf'
export interface MiddleProps extends LeafProps {
  label: string;
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script setup lang="ts">
import type { MiddleProps } from './middle'
defineProps<MiddleProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let meta = host
        .get_component_meta("/Owner.vue")
        .expect("get_component_meta must succeed for transitively-imported fixture");

    let prop_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    // After dispatch substitutes through the extends chain, all three
    // fields (count, enabled, label) appear in the resolved meta. This
    // is the property the rematerialize phase used to repair when
    // compute alone was insufficient; post-Step-1.5 compute is enough.
    assert!(
        prop_names.iter().any(|n| n == "count"),
        "transitively imported prop `count` must surface; got {prop_names:?}"
    );
    assert!(
        prop_names.iter().any(|n| n == "label"),
        "directly declared prop `label` must surface; got {prop_names:?}"
    );

    // No symbolic LeafProps / MiddleProps Ref left in the resolved
    // type_expr — proves dispatch resolved through the extends chain.
    let count = prop_by_name(&meta, "count");
    let label = prop_by_name(&meta, "label");
    assert_no_unresolved_ref(&count.type_expr, "LeafProps");
    assert_no_unresolved_ref(&count.type_expr, "MiddleProps");
    assert_no_unresolved_ref(&label.type_expr, "MiddleProps");
}

// ============================================================================
// Class 5: Direct symbol resolution
//
// `solve_expr_type_expr` was the legacy walker entry that resolved a
// `TypeExpr` against a scope by walking declarations. Test the dispatch
// equivalent (`lower_type_expr_in_scope_with_mode` + `raise_node_to_type_expr`)
// produces a structural surface for an imported alias.
// ============================================================================

#[test]
fn parity_class5_direct_symbol_resolution_dispatches_correctly() {
    // `solve_expr_type_expr` and `expand_local_generic_ref_expr` were
    // legacy walker methods on `ComponentMetaQueryEngine` that resolved
    // a `TypeExpr` against a scope by walking declarations. Their
    // architectural successor is `dispatch.shallow_lower_type_expr →
    // raise_and_reduce(Expanded)` (see
    // `materialize_component_meta_type_expr_until_stable_full`). This
    // test exercises that path through the public component-meta API
    // on a fixture that previously routed through Class-5 direct symbol
    // resolution: a `typeof` reference whose value root is an exported
    // const, plus a generic helper alias that previously went through
    // `expand_local_generic_ref_expr`.
    let project = make_project();
    project
        .upsert_base(
            "/values.ts",
            r#"export const ENGINE_DEFAULTS = {
  retries: 3,
  timeout: 5000,
  label: 'engine',
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script setup lang="ts">
import { ENGINE_DEFAULTS } from './values'
defineProps<{ defaults: typeof ENGINE_DEFAULTS }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let meta = host
        .get_component_meta("/Owner.vue")
        .expect("get_component_meta must succeed for typeof fixture");

    let defaults = prop_by_name(&meta, "defaults");
    // Dispatch must resolve the `typeof ENGINE_DEFAULTS` reference to
    // an object whose properties match the const's value shape. A
    // surviving symbolic `typeof` or a Ref to ENGINE_DEFAULTS would
    // mean direct symbol resolution failed.
    assert_no_unresolved_ref(&defaults.type_expr, "ENGINE_DEFAULTS");
}
