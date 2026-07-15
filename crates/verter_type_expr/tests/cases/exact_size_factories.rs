//! Discrimination tests for the exact-size-iterator union/intersection
//! factories ([`TypeExpr::union_from_exact_iter`] /
//! [`TypeExpr::intersection_from_exact_iter`]).
//!
//! Contract: byte-identical semantics to the `Vec`-based [`TypeExpr::union`]
//! / [`TypeExpr::intersection`] — empty → `never` / `unknown`, single →
//! unwrap, multi → the compound variant with SOURCE ORDER preserved — while
//! collecting the arms straight into the `Arc<[TypeExpr]>` payload (no
//! intermediate `Vec` + copy on the exact-size path).

use verter_type_expr::{PrimitiveName, TypeExpr};

fn arms(n: usize) -> Vec<TypeExpr> {
    (0..n)
        .map(|i| TypeExpr::string_literal(format!("arm{i}")))
        .collect()
}

#[test]
fn union_from_exact_iter_empty_is_never() {
    let result = TypeExpr::union_from_exact_iter(std::iter::empty());
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::Never));
}

#[test]
fn intersection_from_exact_iter_empty_is_unknown() {
    let result = TypeExpr::intersection_from_exact_iter(std::iter::empty());
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::Unknown));
}

#[test]
fn union_from_exact_iter_single_unwraps() {
    let result = TypeExpr::union_from_exact_iter(arms(1));
    // The single arm is returned bare — NOT wrapped in a 1-arm Union.
    assert_eq!(result, TypeExpr::string_literal("arm0"));
    assert!(!matches!(result, TypeExpr::Union(_)));
}

#[test]
fn intersection_from_exact_iter_single_unwraps() {
    let result = TypeExpr::intersection_from_exact_iter(arms(1));
    assert_eq!(result, TypeExpr::string_literal("arm0"));
    assert!(!matches!(result, TypeExpr::Intersection(_)));
}

#[test]
fn union_from_exact_iter_multi_preserves_source_order() {
    let result = TypeExpr::union_from_exact_iter(arms(3));
    let TypeExpr::Union(parts) = &result else {
        panic!("expected Union, got {result:?}");
    };
    assert_eq!(parts.len(), 3);
    for (i, part) in parts.iter().enumerate() {
        assert_eq!(part, &TypeExpr::string_literal(format!("arm{i}")));
    }
}

#[test]
fn intersection_from_exact_iter_multi_preserves_source_order() {
    let result = TypeExpr::intersection_from_exact_iter(arms(4));
    let TypeExpr::Intersection(parts) = &result else {
        panic!("expected Intersection, got {result:?}");
    };
    assert_eq!(parts.len(), 4);
    for (i, part) in parts.iter().enumerate() {
        assert_eq!(part, &TypeExpr::string_literal(format!("arm{i}")));
    }
}

#[test]
fn exact_iter_factories_match_vec_factories_across_lengths() {
    for n in 0..5 {
        assert_eq!(
            TypeExpr::union_from_exact_iter(arms(n)),
            TypeExpr::union(arms(n)),
            "union divergence at {n} arms"
        );
        assert_eq!(
            TypeExpr::intersection_from_exact_iter(arms(n)),
            TypeExpr::intersection(arms(n)),
            "intersection divergence at {n} arms"
        );
    }
}

#[test]
fn exact_iter_factories_accept_mapped_slice_iterators() {
    // The intended call shape: a `Map` over a slice iterator (the OXC
    // lowering arms). The bound is `ExactSizeIterator`, so this must
    // compile and produce the same value as the Vec path.
    let source = ["a", "b", "c"];
    let result = TypeExpr::union_from_exact_iter(
        source
            .iter()
            .map(|s| TypeExpr::string_literal(s.to_string())),
    );
    let TypeExpr::Union(parts) = &result else {
        panic!("expected Union, got {result:?}");
    };
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], TypeExpr::string_literal("a"));
    assert_eq!(parts[2], TypeExpr::string_literal("c"));
}
