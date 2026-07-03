//! Compile-FAIL fixture: the tightened `#[no_typeexpr(recursive_self)]` matcher
//! accepts ONLY `Arc<[Self]>` where the slice element is the container's OWN
//! single-segment type. A DIFFERENT type that merely shares the container's
//! LAST-segment name (`Arc<[some_mod::Recipe]>`) is NOT a self-container — its
//! `TypeExpr`-owning payload keeps its `NoTypeExpr` bound, which is
//! unsatisfiable.
//!
//! This is THE tightening red-proof: pre-fix, the last-segment matcher matched
//! `some_mod::Recipe` against the container ident and OMITTED the bound, so the
//! foreign type's `TypeExpr` slipped through and this COMPILED (a false witness).

use std::sync::Arc;

mod some_mod {
    // A DIFFERENT type that shares the container's last-segment name and owns a
    // `verter_type_expr::TypeExpr`.
    pub struct Recipe(pub verter_type_expr::TypeExpr);
}

#[derive(verter_no_typeexpr::NoTypeExpr)]
#[no_typeexpr(recursive_self)]
enum Recipe {
    // The genuine fixed-point self-container — its self-bound is omitted (so the
    // escape's at-least-one-`Arc<[Self]>` requirement is satisfied).
    Rec(Arc<[Recipe]>),
    // NOT the container's own type: a foreign `some_mod::Recipe` (multi-segment)
    // that owns a `TypeExpr`. Its `NoTypeExpr` bound is KEPT and unsatisfiable.
    Foreign(Arc<[some_mod::Recipe]>),
}

fn main() {
    let _ = std::mem::size_of::<Recipe>();
}
