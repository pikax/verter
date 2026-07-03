//! Compile-FAIL fixture: an `Option<verter_span::Span>` field cannot derive
//! `NoStoredSpan`. The container impl FORWARDS the witness bound, so
//! `Option<Span>: NoStoredSpan` holds only if `Span: NoStoredSpan` does — and it
//! does not. This proves the marker catches a span wrapped in a container, not
//! only a bare `Span` field (the `MemberSpans`-style `Option<Span>` shape).

#[derive(verter_no_storedspan::NoStoredSpan)]
struct Bad {
    s: Option<verter_span::Span>,
}

fn main() {
    let _ = std::mem::size_of::<Bad>();
}
