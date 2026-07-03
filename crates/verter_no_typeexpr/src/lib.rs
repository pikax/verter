//! `NoTypeExpr` — a compiler-enforced marker trait certifying that a type owns,
//! transitively, NO symbolic IR `verter_type_expr::TypeExpr`.
//!
//! # Motivation
//!
//! The session-internal hot prepared-declaration carriers store every type body
//! as an arena handle, never the symbolic `TypeExpr`. The first guard for that
//! invariant was a `syn` SOURCE SCANNER over field-type spellings; it was
//! empirically launderable (`use verter_type_expr::TypeExpr as HotBody; field:
//! HotBody` slips past a spelling check). The sound close is this marker trait:
//! the compiler resolves the ACTUAL field type, so an aliased `TypeExpr`-owner
//! — or a nested owner such as `ValueRef` / `TupleElement` — fails the bound.
//!
//! # What the marker is (and is not)
//!
//! `NoTypeExpr` is a FIRST-PARTY STRUCTURAL WITNESS — a compiler-enforced check
//! that a first-party type derived with `#[derive(NoTypeExpr)]` owns no
//! transitive `TypeExpr`. It is NOT an adversarial, downstream-proof security
//! boundary.
//!
//! [`NoTypeExpr`] is a thin public façade over the hidden witness supertrait
//! [`__private::NoTypeExprWitness`], plus a blanket bridge:
//!
//! ```ignore
//! pub trait NoTypeExpr: __private::NoTypeExprWitness {}
//! impl<T: __private::NoTypeExprWitness + ?Sized> NoTypeExpr for T {}
//! ```
//!
//! Because `NoTypeExpr` is blanket-impl'd for everything that is the witness, no
//! crate — first-party or downstream — can hand-write `impl NoTypeExpr for X`:
//! it would overlap the blanket (`E0119`). For a first-party carrier the only
//! sanctioned route to the witness is [`#[derive(NoTypeExpr)]`](NoTypeExpr),
//! which emits the witness field-recursively, so a derived carrier with a
//! `TypeExpr`-owning field (by alias or nesting) fails to compile.
//!
//! What this does NOT prevent: the witness supertrait
//! [`__private::NoTypeExprWitness`] is `pub` (it must be, so the derive's
//! generated `::verter_no_typeexpr::__private::…` path resolves in downstream
//! crates). A downstream crate could therefore DELIBERATELY FORGE the marker by
//! hand-writing `impl __private::NoTypeExprWitness for SpanOwner {}`. That is a
//! hostile act, not accidental drift, and the marker does not defend against it
//! — the guard is the first-party `#[derive]` discipline plus the narrow
//! in-crate hand-impl bans, not a cross-crate seal.
//!
//! The hand-written witness impls in this crate ARE the trusted foundational set
//! — the primitives, owned containers (each forwarding the bound), small tuples,
//! `HashMap` (with the hasher `S` bounded — it is owned state), and
//! [`verter_span::Span`] (this crate owns that leaf fact so `verter_span` stays
//! marker-free). Shared references, raw pointers, and function pointers are
//! deliberately NOT witnesses.

#![forbid(unsafe_code)]

// The derive emits absolute `::verter_no_typeexpr::...` paths so downstream
// crates resolve them. This self-alias lets those paths resolve INSIDE this
// crate too (its own `#[cfg(test)]` carriers derive `NoTypeExpr`) — a crate
// cannot otherwise name itself. (Standard idiom, as used by serde.)
extern crate self as verter_no_typeexpr;

/// Hidden witness machinery. By CONVENTION, do not name, import, or
/// hand-implement [`__private::NoTypeExprWitness`] outside the sanctioned
/// `#[derive(NoTypeExpr)]` route (and the single audited carrier exception in
/// `verter_session`). It is `pub` only so the derive's generated absolute paths
/// resolve downstream — it is NOT a sealed boundary, so a downstream crate CAN
/// deliberately forge this witness; the marker is a first-party structural
/// witness, not a security boundary. The blanket bridge still makes the PUBLIC
/// [`NoTypeExpr`] trait non-hand-implementable (E0119).
#[doc(hidden)]
pub mod __private {
    /// The hidden supertrait of [`NoTypeExpr`](crate::NoTypeExpr). A type is
    /// `NoTypeExpr` exactly when it is this witness. `#[derive(NoTypeExpr)]`
    /// emits this impl field-recursively; this crate hand-writes it for the
    /// trusted foundational types.
    pub trait NoTypeExprWitness {}
}

use __private::NoTypeExprWitness;

/// Marker trait: the implementing type owns NO transitive
/// `verter_type_expr::TypeExpr`. Bound a field on this to require it; derive it
/// (field-recursively) with `#[derive(NoTypeExpr)]`.
///
/// This trait cannot be hand-implemented downstream — see the module docs.
pub trait NoTypeExpr: NoTypeExprWitness {}

/// The blanket bridge: every witness is `NoTypeExpr`. This is what makes the
/// public trait non-hand-implementable downstream (a manual
/// `impl NoTypeExpr for X` overlaps this blanket).
impl<T: NoTypeExprWitness + ?Sized> NoTypeExpr for T {}

/// `#[derive(NoTypeExpr)]` — emits the hidden witness field-recursively. A
/// single `use verter_no_typeexpr::NoTypeExpr;` brings both the trait and the
/// derive into scope.
pub use verter_no_typeexpr_derive::NoTypeExpr;

// ===========================================================================
// Trusted foundational witness impls.
//
// These hand-written impls are the trust anchor: every derived witness bottoms
// out in them. The container impls FORWARD the witness bound, so the recursion
// is sound (`Vec<TypeExpr>` is not a witness because `TypeExpr` is not). The
// foundational impls use the witness bound directly to stay self-contained.
// ===========================================================================

/// Implement the hidden witness for a list of concrete, field-free leaf types.
macro_rules! witness_leaf {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl NoTypeExprWitness for $ty {}
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
impl<T: NoTypeExprWitness> NoTypeExprWitness for Option<T> {}
impl<T: NoTypeExprWitness> NoTypeExprWitness for Vec<T> {}
impl<T: NoTypeExprWitness + ?Sized> NoTypeExprWitness for Box<T> {}
impl<T: NoTypeExprWitness + ?Sized> NoTypeExprWitness for std::sync::Arc<T> {}
impl<T: NoTypeExprWitness> NoTypeExprWitness for [T] {}
impl<T: NoTypeExprWitness, const N: usize> NoTypeExprWitness for [T; N] {}
impl<T: NoTypeExprWitness + ?Sized> NoTypeExprWitness for std::marker::PhantomData<T> {}

// --- Small tuples (each element forwards the bound) ---
impl<A: NoTypeExprWitness> NoTypeExprWitness for (A,) {}
impl<A: NoTypeExprWitness, B: NoTypeExprWitness> NoTypeExprWitness for (A, B) {}
impl<A: NoTypeExprWitness, B: NoTypeExprWitness, C: NoTypeExprWitness> NoTypeExprWitness
    for (A, B, C)
{
}

// --- HashMap: the hasher `S` is OWNED STATE, so it MUST be bounded; an
//     unbounded-`S` impl would let a hasher smuggle a `TypeExpr`. ---
impl<K: NoTypeExprWitness, V: NoTypeExprWitness, S: NoTypeExprWitness> NoTypeExprWitness
    for std::collections::HashMap<K, V, S>
{
}

// The concrete hashers used across the workspace, so `FxHashMap<K, V>` and the
// std default `HashMap<K, V>` are covered.
impl NoTypeExprWitness for rustc_hash::FxBuildHasher {}
impl NoTypeExprWitness for std::collections::hash_map::RandomState {}

// --- The trusted leaf span impl. `verter_no_typeexpr` owns this fact so the
//     leaf `verter_span` crate stays free of the marker (no reverse dep). ---
impl NoTypeExprWitness for verter_span::Span {}

#[cfg(test)]
mod tests {
    use super::NoTypeExpr;
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    // A type that is NOT a witness — the negative control. (`fn()` is
    // deliberately excluded from the foundational impls.) The field is load-
    // bearing for type identity only; it is never read.
    #[allow(dead_code)]
    struct NotAWitness(fn());

    // Synthetic carriers exercising the blanket bridge + the derive over scalar
    // / container fields. Fields/variants exist to drive the field-recursion and
    // are never read.
    #[allow(dead_code)]
    #[derive(NoTypeExpr)]
    struct GoodScalars {
        a: u32,
        b: String,
        c: bool,
        d: Option<u64>,
        e: Vec<String>,
        f: std::sync::Arc<[u8]>,
        g: (char, u8),
        h: rustc_hash::FxHashMap<String, u32>,
        i: verter_span::Span,
    }

    #[allow(dead_code)]
    #[derive(NoTypeExpr)]
    enum GoodEnum {
        Unit,
        Tuple(u8, String),
        Named { x: Option<bool> },
    }

    // A synthetic POISON owner: it owns a non-witness field, so it must NOT be
    // `NoTypeExpr`. This discriminates — if the marker were trivially-satisfied,
    // this assert would not hold.
    struct PoisonOwner {
        #[allow(dead_code)]
        bad: NotAWitness,
    }
    // NOTE: `PoisonOwner` deliberately does NOT derive `NoTypeExpr` (deriving
    // would be a compile error, which is itself the proof — covered by the
    // verter_session trybuild suite). Here we assert the non-impl directly.

    #[test]
    fn good_carriers_implement_no_type_expr() {
        assert_impl_all!(GoodScalars: NoTypeExpr);
        assert_impl_all!(GoodEnum: NoTypeExpr);
        // The foundational leaves + containers themselves are witnesses.
        assert_impl_all!(u32: NoTypeExpr);
        assert_impl_all!(String: NoTypeExpr);
        assert_impl_all!(Option<u32>: NoTypeExpr);
        assert_impl_all!(Vec<String>: NoTypeExpr);
        assert_impl_all!(std::sync::Arc<[u8]>: NoTypeExpr);
        assert_impl_all!(verter_span::Span: NoTypeExpr);
        assert_impl_all!(rustc_hash::FxHashMap<String, u32>: NoTypeExpr);
    }

    #[test]
    fn non_witness_types_are_not_no_type_expr() {
        // Discriminating negatives: a bare non-witness, an owner of one, and the
        // deliberately-excluded reference / pointer / fn-pointer forms.
        assert_not_impl_any!(NotAWitness: NoTypeExpr);
        assert_not_impl_any!(PoisonOwner: NoTypeExpr);
        assert_not_impl_any!(&'static u32: NoTypeExpr);
        assert_not_impl_any!(*const u32: NoTypeExpr);
        assert_not_impl_any!(fn() -> u32: NoTypeExpr);
    }

    #[test]
    fn container_of_non_witness_is_not_no_type_expr() {
        // The bound FORWARDS: a container of a non-witness is itself not a
        // witness. This is the recursion-soundness proof.
        assert_not_impl_any!(Option<NotAWitness>: NoTypeExpr);
        assert_not_impl_any!(Vec<NotAWitness>: NoTypeExpr);
        assert_not_impl_any!(std::sync::Arc<NotAWitness>: NoTypeExpr);
        assert_not_impl_any!((u32, NotAWitness): NoTypeExpr);
        // The hasher position is bounded too: a map with a non-witness hasher is
        // not a witness.
        assert_not_impl_any!(
            std::collections::HashMap<String, u32, BadHasher>: NoTypeExpr
        );
    }

    // A non-witness hasher placeholder for the `S`-is-bounded discrimination.
    #[allow(dead_code)]
    struct BadHasher(fn());
}
