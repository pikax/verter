//! Compile-FAIL fixture — THE RED-PROOF: an ALIASED `TypeExpr` field.
//!
//! The launderable source-spelling scanner judged the WRITTEN field-type name,
//! so `use verter_type_expr::TypeExpr as Body; field: Body` slipped past it (the
//! spelling `Body` was not on its denylist). The compiler `NoTypeExpr` derive
//! resolves `Body` to its real type `TypeExpr` — which is NOT `NoTypeExpr` — so
//! the derived witness bound is unsatisfiable and the build FAILS. This is the
//! exact hole the new mechanism closes that the old one could not.

use verter_type_expr::TypeExpr as Body;

#[derive(verter_no_typeexpr::NoTypeExpr)]
struct Bad {
    body: Body,
}

fn main() {
    let _ = std::mem::size_of::<Bad>();
}
