//! Phase 5b §5.A — TDD seed for resolver coverage gap: utility-type
//! mapped/conditional evaluation does not currently distribute through
//! the macro path.
//!
//! Source: `phase-00-tier1-mismatches.md` row 1 (`mapped_exclude`,
//! line 28). TS spec §4.4: `Exclude<T,U> = T extends U ? never : T`
//! distributes over the union T and removes every member matching U.
//!
//! **Pre-Phase-5b behaviour (current tree):** the macro path surfaces
//! the unresolved `Exclude<>` utility as `Unknown { raw: "semanticMiss" }`
//! instead of evaluating the distributive conditional. The
//! `kind` prop's resolved `type_expr` is therefore an `Unknown`
//! variant, NOT a `Union` of `'a' | 'c'`.
//!
//! **Post-Phase-5b expected:** with the variant + dispatch helpers
//! landed (commits 4a/5/9 close the gap end-to-end), the resolver
//! emits a `Union` containing exactly `Literal::String("a")` and
//! `Literal::String("c")`.
//!
//! This seed remains RED through the end of Phase 5b — the close
//! happens in 5d/5e/5f via callsite migrations. The rule is: this
//! test must FAIL on the pre-Phase-5b tree (no `Union` of `'a' | 'c'`
//! surfaces) and must PASS once the migration lands.

use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

use crate::harness::{build_hermetic_host_with_lib, resolve_under_audit, STUB_LIB_ES5};

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
#[ignore = "Phase 5f §9 deferral to 5g: `Exclude<>` is a 'deferred utility' in `dispatch's build.rs:962-966` — its body lowers to `T extends U ? never : T` but the conditional reduction depends on the relation engine's ability to decide string-literal-extends-string-literal assignability. Phase 5f's commits 7+8 add open-Conditional empty-path distribution + IndexedAccess empty-path materialisation, but neither closes the `Exclude<'a'|'b'|'c', 'b'>` reduction because the conditional check (`'a'`, `'b'`, `'c'`) is bound to concrete string literals, not unbound, so distribution does NOT trigger (and would be wrong if it did — `Exclude` requires CONCRETE reduction to drop the matching literal, not Union both branches). Closes in 5g where the engine deletion + 7 fixture authoring lands a discriminating `Exclude` evaluation path that routes through the relation engine's literal-equality check. Verified FAIL pre-impl on commit 1, still FAIL after 5f commits 7+8."]
fn resolver_coverage_mapped_types_exclude_distributes() {
    let host = build_hermetic_host_with_lib(
        &[("/c.vue", MAPPED_EXCLUDE_VUE)],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/c.vue");

    let kind = analysis
        .props
        .iter()
        .find(|p| p.name == "kind")
        .expect("Source.kind must surface as a prop");

    // Discriminating: pre-fix, type_expr is Unknown; post-fix, it is a
    // Union of two string literals. assert_eq! over collected literals
    // would fail on an `Unknown` variant outright (no String literals
    // produced) AND on a wrong-arity Union (1 or 3 members).
    let literals = collect_string_literals(&kind.type_expr);
    let mut sorted = literals.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec!["a".to_string(), "c".to_string()],
        "Exclude<'a'|'b'|'c', 'b'> must evaluate to literal union 'a' | 'c'; got {:?} from {:#?}",
        literals,
        kind.type_expr
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
