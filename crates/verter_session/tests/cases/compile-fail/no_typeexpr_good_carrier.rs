//! Compile-PASS fixture: a synthetic hot-carrier-analogue using only
//! TypeExpr-free scalars + a local handle compiles cleanly under
//! `#[derive(verter_no_typeexpr::NoTypeExpr)]`.
//!
//! `HotTypeRef` is crate-private, so the fixture stands in a local
//! `#[derive(NoTypeExpr)] struct LocalHandle(u64);` for the "good handle"
//! position. Every field is a scalar, a container thereof, an `Arc<[...]>`
//! slice, a scalar enum, or a `verter_span::Span` — all `NoTypeExpr`, so the
//! derive succeeds. This pins that the marker does NOT over-reject sound
//! handle-native carriers.

use std::sync::Arc;

/// The "good handle" position — a `u64` ordinal analogue of `HotTypeRef`.
#[derive(verter_no_typeexpr::NoTypeExpr)]
struct LocalHandle(u64);

#[derive(verter_no_typeexpr::NoTypeExpr)]
enum ScalarKind {
    A,
    B,
    C,
}

#[derive(verter_no_typeexpr::NoTypeExpr)]
struct GoodCarrier {
    name: Arc<str>,
    handle: LocalHandle,
    maybe: Option<LocalHandle>,
    many: Vec<LocalHandle>,
    slice: Arc<[LocalHandle]>,
    kind: ScalarKind,
    span: verter_span::Span,
    flag: bool,
    count: u32,
}

fn main() {
    // The fixture only needs to compile; reference the type so it is not
    // dead-code-eliminated before the derive bound is checked.
    let _ = std::mem::size_of::<GoodCarrier>();
}
