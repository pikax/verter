//! Compile-FAIL fixture: a closed-fact carrier (deriving BOTH the marker traits
//! a fact family must satisfy) with a `verter_type_expr::TypeExpr` field cannot
//! compile — the `NoTypeExpr` witness bound `TypeExpr: NoTypeExpr` is
//! unsatisfiable. A fact routes unsupported structure through a LOCATOR, never
//! an embedded `TypeExpr`; this fixture proves the marker forbids the embed even
//! on a carrier that also derives `NoStoredSpan`.

#[derive(
    verter_no_typeexpr::NoTypeExpr,
    verter_no_storedspan::NoStoredSpan,
)]
struct BadFact {
    body: verter_type_expr::TypeExpr,
}

fn main() {
    let _ = std::mem::size_of::<BadFact>();
}
