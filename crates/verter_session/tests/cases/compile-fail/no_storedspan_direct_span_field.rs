//! Compile-FAIL fixture: a carrier with a direct `verter_span::Span` field
//! cannot derive `NoStoredSpan` — the derived witness bound
//! `Span: NoStoredSpan` is unsatisfiable (the marker deliberately provides NO
//! leaf witness for `Span`), so the build fails.
//!
//! This is the inverse of `no_typeexpr_direct_field.rs`: the SAME `Span` field
//! that PASSES `NoTypeExpr` (which owns a `Span` leaf) FAILS `NoStoredSpan`.
//! That is the whole reason `NoStoredSpan` is a separate marker — it enforces
//! the "spans are recovered-via-locator, never stored on a fact" contract that
//! `NoTypeExpr` cannot.

#[derive(verter_no_storedspan::NoStoredSpan)]
struct Bad {
    s: verter_span::Span,
}

fn main() {
    let _ = std::mem::size_of::<Bad>();
}
