//! Compile-FAIL fixture (the bare-shadow hole — `NoStoredSpan` inverse): a
//! locally re-imported BARE `Arc` — one that OWNS a `verter_span::Span` — is
//! brought into scope via `use shadow::Arc;`. The SYNTACTIC matcher sees a bare
//! single-segment `Arc<[Recipe]>` and cannot tell it from `std::sync::Arc`; the
//! compiler-resolved `RecursiveSelfArc<Self>` proof-bound RESOLVES the field
//! type, and `shadow::Arc<[Recipe]>` does NOT implement `RecursiveSelfArc<Recipe>`,
//! so the derive fails to compile.
//!
//! DISCRIMINATING: under the OLD syntactic-omit escape the bare `Arc` matched and
//! its bound was OMITTED, so the stored `Span` slipped through and this COMPILED
//! (a false `NoStoredSpan` witness). Under the proof-bound it FAILS. This is the
//! red-proof the earlier `recursive_self_shadowed_arc_wrapper` (a MULTI-segment
//! `shadow::Arc`) could not cover — only a BARE re-imported `Arc` reaches this
//! hole.

use std::marker::PhantomData;

mod shadow {
    use super::PhantomData;

    // A custom `Arc` that owns a `Span`, exposed under the BARE name `Arc`.
    // `T: ?Sized` so the `[Recipe]` slice element is well-formed; the failure is
    // the unsatisfied `RecursiveSelfArc` proof-bound, not a sizing error.
    pub struct Arc<T: ?Sized> {
        pub bad: verter_span::Span,
        pub _marker: PhantomData<*const T>,
    }
}

// The BARE `Arc` name now resolves to the custom `shadow::Arc`, not std's.
use shadow::Arc;

#[derive(verter_no_storedspan::NoStoredSpan)]
#[no_storedspan(recursive_self)]
enum Recipe {
    // A BARE `Arc<[Recipe]>` — but `Arc` is `shadow::Arc` (owns a `Span`). The
    // syntactic matcher accepts the bare name; the proof-bound rejects the
    // resolved type.
    Rec(Arc<[Recipe]>),
}

fn main() {
    let _ = std::mem::size_of::<Recipe>();
}
