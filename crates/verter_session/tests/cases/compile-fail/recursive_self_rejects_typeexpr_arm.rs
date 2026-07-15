//! Compile-FAIL fixture: the `#[no_typeexpr(recursive_self)]` escape replaces
//! ONLY the `Arc<[Self]>` recursive field's plain witness bound with the
//! `RecursiveSelfArc<Self>` proof-bound — it does NOT weaken the derive for any
//! other arm. A carrier that uses the escape AND grows a NEW non-recursive arm
//! owning a `verter_type_expr::TypeExpr` still FAILS the derive: the non-recursive
//! arm keeps its `TypeExpr: NoTypeExpr` witness bound, which is unsatisfiable.
//!
//! This is the future-arm proof for `ClosednessRecipe`: the recursive-self
//! escape closes the fixpoint arm without opening a hole for a forbidden payload.

use std::sync::Arc;

#[derive(verter_no_typeexpr::NoTypeExpr)]
#[no_typeexpr(recursive_self)]
enum Bad {
    // The approved fixed-point self-container arm — its plain witness bound is
    // replaced by the `RecursiveSelfArc<Self>` proof-bound.
    Rec(Arc<[Bad]>),
    // A NEW non-recursive arm owning a `TypeExpr` — its bound is KEPT and fails.
    NonRecursive(verter_type_expr::TypeExpr),
}

fn main() {
    let _ = std::mem::size_of::<Bad>();
}
