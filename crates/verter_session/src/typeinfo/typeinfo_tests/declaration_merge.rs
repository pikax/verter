//! Discriminating oracles for SAME-FILE TypeScript declaration merging.
//!
//! TypeScript merges multiple same-name declarations in one file:
//!   * Two `interface Foo` blocks UNION their members.
//!   * Same-name interface METHODS accumulate into an ordered overload group
//!     (NOT a single shadowed signature — the Contest-2 killer case).
//!   * Function overload sets surface every bodiless overload signature with
//!     the trailing implementation hidden.
//!
//! Each oracle is written to FAIL on the pre-merge tree (last-wins drops
//! earlier contributors / overloads collapse) and PASS once the `MergedDecl`
//! carrier + peer-merge reducer + overload-projection land. Negative controls
//! assert unrelated symbols do NOT accidentally merge.

use super::support::*;
use verter_type_expr::{FunctionExpr, TypeExpr};

const PATH: &str = "/fixtures/declaration_merge.ts";

/// First-parameter primitive of an overload call signature.
fn first_param_primitive(f: &FunctionExpr) -> PrimitiveName {
    match &f
        .parameters
        .first()
        .expect("overload signature must have >=1 param")
        .ty
    {
        TypeExpr::Primitive(p) => *p,
        other => panic!("expected primitive first param, got {other:?}"),
    }
}

/// The ordered list of first-parameter primitives carried by a callable
/// member's type. A bare `Function` is a one-element overload group; an
/// intersection of functions is an ordered overload group of N — the canonical
/// structural encoding of an overloaded method. Anything else is a projection
/// bug (a shadowed single signature, or a non-callable).
fn overload_param_primitives(ty: &TypeExpr) -> Vec<PrimitiveName> {
    match ty {
        TypeExpr::Function(f) => vec![first_param_primitive(f)],
        TypeExpr::Intersection(arms) => arms
            .iter()
            .map(|arm| match arm {
                TypeExpr::Function(f) => first_param_primitive(f),
                other => panic!("expected function arm in overload group, got {other:?}"),
            })
            .collect(),
        other => panic!("expected callable member type, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 1. Two interface blocks UNION their members.
// ---------------------------------------------------------------------------

#[test]
fn same_file_interface_merge_unions_members() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Foo { a: string }\nexport interface Foo { b: number }\n",
    );

    let (expr, record) = resolve_expr(&host, PATH, "Foo", &[], ProjectionMode::Expanded);
    let props = object_props(&expr);
    let names = prop_names(&props);

    // Positive: BOTH members survive the merge.
    assert!(
        names.contains(&"a"),
        "merged Foo must expose `a`; got {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "merged Foo must expose `b`; got {names:?}"
    );
    // Negative: neither contributor is dropped (last-wins drops `a`).
    assert!(
        !names.is_empty() && names.contains(&"a") && names.contains(&"b"),
        "neither `a` nor `b` may be absent from the merged surface; got {names:?}"
    );
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 2. Adding a contributor invalidates the merged warm entry.
// ---------------------------------------------------------------------------

#[test]
fn same_file_interface_merge_invalidates_on_contributor_add() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Foo { a: string }\nexport interface Foo { b: number }\n",
    );

    let (expr, _) = resolve_expr(&host, PATH, "Foo", &[], ProjectionMode::Expanded);
    let before = prop_names(&object_props(&expr))
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert!(
        before.contains(&"a".to_string()) && before.contains(&"b".to_string()),
        "warm merged surface must start with a,b; got {before:?}"
    );

    // Add a third contributor to the same file.
    upsert_ts(
        &host,
        PATH,
        "export interface Foo { a: string }\nexport interface Foo { b: number }\nexport interface Foo { c: boolean }\n",
    );

    let (expr, _) = resolve_expr(&host, PATH, "Foo", &[], ProjectionMode::Expanded);
    let props = object_props(&expr);
    let names = prop_names(&props);
    assert!(
        names.contains(&"a"),
        "after add: `a` must remain; got {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "after add: `b` must remain; got {names:?}"
    );
    assert!(
        names.contains(&"c"),
        "after add: `c` must appear; got {names:?}"
    );
    assert_primitive(&props["c"].ty, PrimitiveName::Boolean);
}

// ---------------------------------------------------------------------------
// 3. Function overload group: bodiless overloads visible, impl hidden.
// ---------------------------------------------------------------------------

#[test]
fn same_file_function_overloads_surface_bodiless_signatures() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export function f(x: number): void;\nexport function f(x: string): void;\nexport function f(x: any): void {}\n",
    );

    let (expr, record) = evaluate_expr(&host, PATH, "typeof f", ProjectionMode::Expanded);
    let sigs = object_call_signatures(&expr);

    // The two bodiless overloads are visible; the implementation is hidden.
    assert_eq!(
        sigs.len(),
        2,
        "typeof f must expose exactly the TWO bodiless overloads (impl hidden); got {} signatures",
        sigs.len()
    );
    let params: Vec<PrimitiveName> = sigs.iter().map(first_param_primitive).collect();
    assert!(
        params.contains(&PrimitiveName::Number) && params.contains(&PrimitiveName::String),
        "overload params must be number and string; got {params:?}"
    );
    // Negative: not collapsed to one, not the impl `any` signature.
    assert!(
        !params.contains(&PrimitiveName::Any),
        "implementation `any` overload must be hidden; got {params:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 4. Interface METHOD overload-merge (the Contest-2 killer).
// ---------------------------------------------------------------------------

#[test]
fn same_file_interface_method_merge_accumulates_overload_group() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface I { m(x: number): void }\nexport interface I { m(x: string): void }\n",
    );

    let (expr, record) = resolve_expr(&host, PATH, "I", &[], ProjectionMode::Expanded);
    let props = object_props(&expr);
    let member = props.get("m").unwrap_or_else(|| {
        panic!(
            "merged I must expose member `m`; got {:?}",
            prop_names(&props)
        )
    });

    let overloads = overload_param_primitives(&member.ty);
    // Peer-merge accumulates BOTH method signatures; shadow would keep one.
    assert_eq!(
        overloads.len(),
        2,
        "I.m must be an ordered overload group of 2 signatures, not a shadowed single; got {overloads:?}"
    );
    assert!(
        overloads.contains(&PrimitiveName::Number) && overloads.contains(&PrimitiveName::String),
        "the two I.m overloads must take number and string; got {overloads:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 5. Negative controls — no accidental merge.
// ---------------------------------------------------------------------------

#[test]
fn unrelated_consts_do_not_merge() {
    let host = make_host_with_footprint();
    upsert_ts(&host, PATH, "export const p = 1;\nexport const q = 2;\n");

    let (p_expr, _) = evaluate_expr(&host, PATH, "typeof p", ProjectionMode::Expanded);
    let (q_expr, _) = evaluate_expr(&host, PATH, "typeof q", ProjectionMode::Expanded);
    assert_number_literal(&p_expr, 1.0);
    assert_number_literal(&q_expr, 2.0);
}

#[test]
fn distinct_interface_names_stay_distinct() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface A { a: string }\nexport interface B { b: number }\n",
    );

    let (a_expr, _) = resolve_expr(&host, PATH, "A", &[], ProjectionMode::Expanded);
    let (b_expr, _) = resolve_expr(&host, PATH, "B", &[], ProjectionMode::Expanded);
    assert_eq!(prop_names(&object_props(&a_expr)), vec!["a"]);
    assert_eq!(prop_names(&object_props(&b_expr)), vec!["b"]);
}
