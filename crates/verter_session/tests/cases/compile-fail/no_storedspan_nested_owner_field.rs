//! Compile-FAIL fixture: a field whose type does not spell `Span` but OWNS one
//! transitively. `verter_type_expr::MemberSpans` carries three `Option<Span>`
//! fields, so it is not `NoStoredSpan` — and a carrier with a `MemberSpans`
//! field therefore cannot derive `NoStoredSpan`.
//!
//! This is the nested-owner analogue for the span marker: it is exactly the
//! shape a fact family would take if it tried to STORE member spans directly
//! instead of recovering them via a producer-emitted origin locator.

#[derive(verter_no_storedspan::NoStoredSpan)]
struct Bad {
    spans: verter_type_expr::MemberSpans,
}

fn main() {
    let _ = std::mem::size_of::<Bad>();
}
