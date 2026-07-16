//! Resolver coverage for utility-type mapped/conditional evaluation:
//! the macro path must distribute through utility types.
//!
//! TS spec §4.4: `Exclude<T,U> = T extends U ? never : T`
//! distributes over the union T and removes every member matching U.
//!
//! The `Extract` / `Exclude` arms of `build_builtin_utility`
//! (`crates/verter_session/src/project_semantic_dispatch/build.rs`)
//! distribute the source union, dispatch each member through
//! `relate_nodes` against the filter argument, and reconstitute the
//! survivors via `intern_normalized_union_or_intersection`. The
//! resolver therefore emits a `Union` containing exactly
//! `Literal::String("a")` and `Literal::String("c")` for
//! `Exclude<'a' | 'b' | 'c', 'b'>`.

use verter_type_expr::{LiteralValue, TypeExpr};

use super::harness::{build_hermetic_host_with_lib, resolve_under_audit, STUB_LIB_ES5};

/// Distributive `Exclude<>` over a string-literal union should drop
/// the matching member and surface the survivors. Here `'b'` is
/// excluded; `'a' | 'c'` survive and form the prop's resolved type.
const MAPPED_EXCLUDE_VUE: &str = r#"<script setup lang="ts">
type Source = { kind: Exclude<'a' | 'b' | 'c', 'b'> };
defineProps<Source>();
</script>
<template><div /></template>
"#;

#[test]
fn resolver_coverage_mapped_types_exclude_distributes() {
    let host = build_hermetic_host_with_lib(
        &[("/c.vue", MAPPED_EXCLUDE_VUE)],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) =
        resolve_under_audit(std::sync::Arc::clone(&host), "/c.vue");

    let kind = analysis
        .props
        .iter()
        .find(|p| p.name == "kind")
        .expect("Source.kind must surface as a prop");
    let kind_source = kind
        .type_source
        .present()
        .expect("Source.kind must publish a typed source");
    let kind_ty = verter_session::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/c.vue",
        kind_source,
    )
    .unwrap_or_else(|| panic!("`kind`'s published source must demand-materialize"));

    // Discriminating: pre-fix, the resolved type is Unknown; post-fix, it
    // is a Union of two string literals. assert_eq! over collected literals
    // would fail on an `Unknown` variant outright (no String literals
    // produced) AND on a wrong-arity Union (1 or 3 members).
    let literals = collect_string_literals(&kind_ty);
    let mut sorted = literals.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec!["a".to_string(), "c".to_string()],
        "Exclude<'a'|'b'|'c', 'b'> must evaluate to literal union 'a' | 'c'; got {literals:?} from {kind_ty:#?}"
    );

    // Negative: 'b' MUST NOT appear (not "filtered out" if still present).
    assert!(
        !literals.iter().any(|s| s == "b"),
        "'b' must not survive Exclude; got literals {:?}",
        literals
    );
}

/// Walks a `TypeExpr` collecting every string-literal payload reached
/// through `Union` / `Intersection` / `Alias`-equivalent shells. Returns
/// `Vec<String>` so callers can sort and compare. An `Unknown { raw }`
/// produces no literals — this is what discriminates pre-fix from
/// post-fix.
fn collect_string_literals(expr: &TypeExpr) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    walk(expr, &mut out);
    return out;

    fn walk(expr: &TypeExpr, out: &mut Vec<String>) {
        match expr {
            TypeExpr::Literal(LiteralValue::String(s)) => out.push(s.to_string()),
            TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
                for a in arms.iter() {
                    walk(a, out);
                }
            }
            _ => { /* Unknown / Primitive / other — no literal contribution */ }
        }
    }
}
