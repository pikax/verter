//! Compile-PASS fixture: a span-free fact-analogue compiles cleanly under
//! `#[derive(verter_no_storedspan::NoStoredSpan)]`. Every field is a scalar, a
//! container thereof, an `Arc<[...]>` slice, a scalar enum, or a `String` — all
//! `NoStoredSpan`, so the derive succeeds. This pins that the marker does NOT
//! over-reject a sound content-free fact carrier (and, unlike `NoTypeExpr`, it
//! carries NO `verter_span::Span` field precisely because a span would fail).

use std::sync::Arc;

/// A content-free ordinal handle analogue.
#[derive(verter_no_storedspan::NoStoredSpan)]
struct LocalOrdinal(u32);

#[derive(verter_no_storedspan::NoStoredSpan)]
enum ScalarKind {
    A,
    B,
    C,
}

#[derive(verter_no_storedspan::NoStoredSpan)]
struct GoodFact {
    name: Arc<str>,
    ordinal: LocalOrdinal,
    maybe: Option<LocalOrdinal>,
    many: Vec<LocalOrdinal>,
    slice: Arc<[LocalOrdinal]>,
    kind: ScalarKind,
    flag: bool,
    count: u32,
    label: String,
}

fn main() {
    let _ = std::mem::size_of::<GoodFact>();
}
