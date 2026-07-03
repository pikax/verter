//! `NoStoredSpan` — a compiler-enforced marker trait certifying that a type
//! owns, transitively, NO `verter_span::Span` (nor any span-owning carrier such
//! as `MemberSpans` / `FunctionSpans` / `IndexSignatureSpans`).
//!
//! # Motivation
//!
//! The closed semantic fact families are content-free: a fact stores no
//! `Span`. Span-bearing IR structs (`ObjectProperty`, `MethodSignature`,
//! `FunctionExpr`, `IndexSignature`, `FunctionParam`) put their span field(s)
//! in `Eq`/`Hash`, so member spans participate in node identity — a fact that
//! reconstructs such a node must RECOVER the exact spans from a retained parse
//! via a producer-emitted origin locator BEFORE any identity/interning, never
//! store a `Span` as a fact field. The sibling [`NoTypeExpr`] marker cannot
//! enforce that: `verter_span::Span` IS `NoTypeExpr`, so a stored span slips
//! past it.
//!
//! `NoStoredSpan` is the separate marker that forbids a stored span. It is the
//! deliberate INVERSE of `NoTypeExpr`: the SAME non-hand-implementable
//! machinery — hidden witness supertrait + blanket bridge + a trusted
//! foundational set of primitive / container / tuple / `HashMap` impls — with
//! ONE difference: there is NO leaf witness for `verter_span::Span`. Because no
//! impl makes `Span` a witness, `Span` / `Option<Span>` / `Vec<Span>` /
//! `MemberSpans` (which owns `Option<Span>` fields) all FAIL the bound, while
//! `String` / `bool` / `u32` / `Arc<str>` and their containers pass.
//!
//! [`NoTypeExpr`]: verter_no_storedspan  (see the sibling `verter_no_typeexpr` crate)
//!
//! # What the marker is (and is not)
//!
//! `NoStoredSpan` is a FIRST-PARTY STRUCTURAL WITNESS — a compiler-enforced
//! check that a first-party type derived with `#[derive(NoStoredSpan)]` owns no
//! transitive `Span`. It is NOT an adversarial, downstream-proof security
//! boundary.
//!
//! [`NoStoredSpan`] is a thin public façade over the hidden witness supertrait
//! [`__private::NoStoredSpanWitness`], plus a blanket bridge:
//!
//! ```ignore
//! pub trait NoStoredSpan: __private::NoStoredSpanWitness {}
//! impl<T: __private::NoStoredSpanWitness + ?Sized> NoStoredSpan for T {}
//! ```
//!
//! Because `NoStoredSpan` is blanket-impl'd for everything that is the witness,
//! no crate — first-party or downstream — can hand-write
//! `impl NoStoredSpan for X`: it would overlap the blanket (`E0119`). For a
//! first-party carrier the only sanctioned route to the witness is
//! [`#[derive(NoStoredSpan)]`](NoStoredSpan), which emits the witness
//! field-recursively, so a derived carrier with a span-owning field (by alias
//! or nesting) fails to compile.
//!
//! What this does NOT prevent: the witness supertrait
//! [`__private::NoStoredSpanWitness`] is `pub` (it must be, so the derive's
//! generated `::verter_no_storedspan::__private::…` path resolves downstream).
//! A downstream crate could therefore DELIBERATELY FORGE the marker by
//! hand-writing `impl __private::NoStoredSpanWitness for SpanOwner {}`. That is a
//! hostile act, not accidental drift, and the marker does not defend against it
//! — the guard is the first-party `#[derive]` discipline, not a cross-crate
//! seal.

#![forbid(unsafe_code)]

// The derive emits absolute `::verter_no_storedspan::...` paths so downstream
// crates resolve them. This self-alias lets those paths resolve INSIDE this
// crate too (its own `#[cfg(test)]` carriers derive `NoStoredSpan`) — a crate
// cannot otherwise name itself. (Standard idiom, as used by serde.)
extern crate self as verter_no_storedspan;

/// Hidden witness machinery. By CONVENTION, do not name, import, or
/// hand-implement [`__private::NoStoredSpanWitness`] outside the sanctioned
/// `#[derive(NoStoredSpan)]` route. It is `pub` only so the derive's generated
/// absolute paths resolve downstream — it is NOT a sealed boundary, so a
/// downstream crate CAN deliberately forge this witness; the marker is a
/// first-party structural witness, not a security boundary. The blanket bridge
/// still makes the PUBLIC [`NoStoredSpan`] trait non-hand-implementable (E0119).
#[doc(hidden)]
pub mod __private {
    /// The hidden supertrait of [`NoStoredSpan`](crate::NoStoredSpan). A type
    /// is `NoStoredSpan` exactly when it is this witness. `#[derive(NoStoredSpan)]`
    /// emits this impl field-recursively; this crate hand-writes it for the
    /// trusted foundational types — deliberately NOT for `verter_span::Span`.
    pub trait NoStoredSpanWitness {}
}

use __private::NoStoredSpanWitness;

/// Marker trait: the implementing type owns NO transitive `verter_span::Span`.
/// Bound a field on this to require it; derive it (field-recursively) with
/// `#[derive(NoStoredSpan)]`.
///
/// This trait cannot be hand-implemented downstream — see the module docs.
pub trait NoStoredSpan: NoStoredSpanWitness {}

/// The blanket bridge: every witness is `NoStoredSpan`. This is what makes the
/// public trait non-hand-implementable downstream (a manual
/// `impl NoStoredSpan for X` overlaps this blanket).
impl<T: NoStoredSpanWitness + ?Sized> NoStoredSpan for T {}

/// `#[derive(NoStoredSpan)]` — emits the hidden witness field-recursively. A
/// single `use verter_no_storedspan::NoStoredSpan;` brings both the trait and
/// the derive into scope.
pub use verter_no_storedspan_derive::NoStoredSpan;

// ===========================================================================
// Trusted foundational witness impls.
//
// These hand-written impls are the trust anchor: every derived witness bottoms
// out in them. The container impls FORWARD the witness bound, so the recursion
// is sound (`Vec<Span>` is not a witness because `Span` is not). The
// foundational impls use the witness bound directly to stay self-contained.
//
// NOTE — the deliberate omission: unlike `verter_no_typeexpr`, there is NO
// `impl NoStoredSpanWitness for verter_span::Span {}`. That omission is the
// whole marker: everything span-touching fails the bound automatically.
// ===========================================================================

/// Implement the hidden witness for a list of concrete, field-free leaf types.
macro_rules! witness_leaf {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl NoStoredSpanWitness for $ty {}
        )+
    };
}

// --- Primitives + owned string scalars ---
witness_leaf!(
    bool,
    char,
    (),
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64,
    str,
    String,
);

// --- Owned containers (each forwards the bound) ---
impl<T: NoStoredSpanWitness> NoStoredSpanWitness for Option<T> {}
impl<T: NoStoredSpanWitness> NoStoredSpanWitness for Vec<T> {}
impl<T: NoStoredSpanWitness + ?Sized> NoStoredSpanWitness for Box<T> {}
impl<T: NoStoredSpanWitness + ?Sized> NoStoredSpanWitness for std::sync::Arc<T> {}
impl<T: NoStoredSpanWitness> NoStoredSpanWitness for [T] {}
impl<T: NoStoredSpanWitness, const N: usize> NoStoredSpanWitness for [T; N] {}
impl<T: NoStoredSpanWitness + ?Sized> NoStoredSpanWitness for std::marker::PhantomData<T> {}

// --- Small tuples (each element forwards the bound) ---
impl<A: NoStoredSpanWitness> NoStoredSpanWitness for (A,) {}
impl<A: NoStoredSpanWitness, B: NoStoredSpanWitness> NoStoredSpanWitness for (A, B) {}
impl<A: NoStoredSpanWitness, B: NoStoredSpanWitness, C: NoStoredSpanWitness> NoStoredSpanWitness
    for (A, B, C)
{
}

// --- HashMap: the hasher `S` is OWNED STATE, so it MUST be bounded; an
//     unbounded-`S` impl would let a hasher smuggle a span. ---
impl<K: NoStoredSpanWitness, V: NoStoredSpanWitness, S: NoStoredSpanWitness> NoStoredSpanWitness
    for std::collections::HashMap<K, V, S>
{
}

// The concrete hashers used across the workspace, so `FxHashMap<K, V>` and the
// std default `HashMap<K, V>` are covered.
impl NoStoredSpanWitness for rustc_hash::FxBuildHasher {}
impl NoStoredSpanWitness for std::collections::hash_map::RandomState {}

#[cfg(test)]
mod tests {
    use super::NoStoredSpan;
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    // A type that is NOT a witness — the negative control. The field is load-
    // bearing for type identity only; it is never read.
    #[allow(dead_code)]
    struct NotAWitness(fn());

    // A span-shaped leaf that (like `verter_span::Span`) is NOT a witness: it
    // has no leaf impl and does not derive the marker, so a carrier owning it
    // must fail the bound. This is the in-crate analogue of `verter_span::Span`;
    // the on-point `verter_span::Span` assertion lives in the dev-dep test below.
    #[allow(dead_code)]
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    struct LocalSpanShape {
        start: u32,
        end: u32,
    }

    #[allow(dead_code)]
    #[derive(NoStoredSpan)]
    struct GoodScalars {
        a: u32,
        b: String,
        c: bool,
        d: Option<u64>,
        e: Vec<String>,
        f: std::sync::Arc<[u8]>,
        g: (char, u8),
        h: rustc_hash::FxHashMap<String, u32>,
    }

    #[allow(dead_code)]
    #[derive(NoStoredSpan)]
    enum GoodEnum {
        Unit,
        Tuple(u8, String),
        Named { x: Option<bool> },
    }

    // A synthetic POISON owner: it owns a span-shaped non-witness field, so it
    // must NOT be `NoStoredSpan`. Deliberately does NOT derive `NoStoredSpan`
    // (deriving would be a compile error — the proof lives in the trybuild
    // suite); here we assert the non-impl directly.
    struct SpanOwner {
        #[allow(dead_code)]
        span: LocalSpanShape,
    }

    #[test]
    fn good_carriers_implement_no_stored_span() {
        assert_impl_all!(GoodScalars: NoStoredSpan);
        assert_impl_all!(GoodEnum: NoStoredSpan);
        // The foundational leaves + containers themselves are witnesses.
        assert_impl_all!(u32: NoStoredSpan);
        assert_impl_all!(String: NoStoredSpan);
        assert_impl_all!(Option<u32>: NoStoredSpan);
        assert_impl_all!(Vec<String>: NoStoredSpan);
        assert_impl_all!(std::sync::Arc<[u8]>: NoStoredSpan);
        assert_impl_all!(rustc_hash::FxHashMap<String, u32>: NoStoredSpan);
    }

    #[test]
    fn span_shaped_types_are_not_no_stored_span() {
        // Discriminating: a span-shaped leaf (no leaf impl, not derived) and an
        // owner of one are NOT witnesses, and neither are the excluded
        // reference / pointer / fn-pointer forms.
        assert_not_impl_any!(LocalSpanShape: NoStoredSpan);
        assert_not_impl_any!(SpanOwner: NoStoredSpan);
        assert_not_impl_any!(NotAWitness: NoStoredSpan);
        assert_not_impl_any!(&'static u32: NoStoredSpan);
        assert_not_impl_any!(*const u32: NoStoredSpan);
        assert_not_impl_any!(fn() -> u32: NoStoredSpan);
    }

    #[test]
    fn container_of_span_shape_is_not_no_stored_span() {
        // The bound FORWARDS: a container of a span-shaped non-witness is itself
        // not a witness. This is the recursion-soundness proof — it is exactly
        // why `Option<Span>` / `Vec<Span>` fail.
        assert_not_impl_any!(Option<LocalSpanShape>: NoStoredSpan);
        assert_not_impl_any!(Vec<LocalSpanShape>: NoStoredSpan);
        assert_not_impl_any!(std::sync::Arc<LocalSpanShape>: NoStoredSpan);
        assert_not_impl_any!((u32, LocalSpanShape): NoStoredSpan);
    }

    // THE on-point discrimination: the real `verter_span::Span` (dev-dep) is NOT
    // `NoStoredSpan` — the leaf impl is deliberately omitted. `Option<Span>`
    // fails too, via the forwarding container bound. This is the inverse of
    // `verter_no_typeexpr`, where `Span` IS a witness.
    #[test]
    fn real_verter_span_is_not_no_stored_span() {
        assert_not_impl_any!(verter_span::Span: NoStoredSpan);
        assert_not_impl_any!(Option<verter_span::Span>: NoStoredSpan);
        assert_not_impl_any!(Vec<verter_span::Span>: NoStoredSpan);
    }
}
