//! Compile-PASS fixture: the recursive-self escape COMPILES for a clean
//! fixed-point carrier (the `ClosednessRecipe` shape). The `Arc<[Self]>` arm's
//! plain marker witness bound is REPLACED by the compiler-resolved
//! `RecursiveSelfArc<Self>` proof-bound (so the trait solver does not overflow),
//! which the genuine `std::sync::Arc<[Self]>` satisfies, while every
//! non-recursive arm's payload stays witnessed. This pins that the escape does
//! not over-reject a sound recursive fact carrier.

use std::sync::Arc;

#[derive(verter_no_typeexpr::NoTypeExpr, verter_no_storedspan::NoStoredSpan)]
#[no_typeexpr(recursive_self)]
#[no_storedspan(recursive_self)]
enum GoodRecipe {
    Leaf,
    // The fixed-point composition arm — the escape replaces its plain witness
    // bound with the `RecursiveSelfArc<Self>` proof-bound.
    All(Arc<[GoodRecipe]>),
    // Non-recursive content-free payloads stay fully witnessed.
    Named(String),
    Ordinal(u32),
}

fn assert_markers<T: verter_no_typeexpr::NoTypeExpr + verter_no_storedspan::NoStoredSpan>() {}

fn main() {
    assert_markers::<GoodRecipe>();
    let _ = std::mem::size_of::<GoodRecipe>();
}
