//! Compile-FAIL fixture: a carrier with a direct `verter_type_expr::TypeExpr`
//! field cannot derive `NoTypeExpr` — the derived witness bound
//! `TypeExpr: NoTypeExpr` is unsatisfiable, so the build fails.
//!
//! This is the baseline the launderable spelling scanner DID catch; the cases
//! it could NOT catch are the aliased and nested-owner fixtures.

#[derive(verter_no_typeexpr::NoTypeExpr)]
struct Bad {
    body: verter_type_expr::TypeExpr,
}

fn main() {
    let _ = std::mem::size_of::<Bad>();
}
