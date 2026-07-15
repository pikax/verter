//! Compile-FAIL fixture: a field whose type does not spell `TypeExpr` but OWNS
//! one transitively. `verter_type_expr::ValueRef` carries a `Vec<TypeExpr>`
//! (its `type_args`), so it is not `NoTypeExpr` — and a carrier with a
//! `ValueRef` field therefore cannot derive `NoTypeExpr`.
//!
//! A spelling scanner allow-listing field names would have to enumerate every
//! transitive owner to catch this; the compiler resolves it for free.

#[derive(verter_no_typeexpr::NoTypeExpr)]
struct Bad {
    nested: verter_type_expr::ValueRef,
}

fn main() {
    let _ = std::mem::size_of::<Bad>();
}
