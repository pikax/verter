//! Compile-PASS fixture: the recursive-self escape COMPILES for a clean
//! fixed-point carrier (the `ClosednessRecipe` shape). The `Arc<[Self]>` arm's
//! self-bound is omitted (so the trait solver does not overflow) while every
//! non-recursive arm's payload stays witnessed. This pins that the escape does
//! not over-reject a sound recursive fact carrier.

use std::sync::Arc;

#[derive(verter_no_typeexpr::NoTypeExpr, verter_no_storedspan::NoStoredSpan)]
#[no_typeexpr(recursive_self)]
#[no_storedspan(recursive_self)]
enum GoodRecipe {
    Leaf,
    // The fixed-point composition arm — self-bound omitted by the escape.
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
