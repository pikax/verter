//! Compile-FAIL fixture (the SEAL red-proof, `NoStoredSpan` marker — the inverse
//! of `recursive_self_hand_impl_sealed_typeexpr`): `RecursiveSelfArc` is impl'd
//! ONLY inside the marker crate — the derive merely EMITS it as a bound, never
//! impls it downstream — so it is SEALED behind a PRIVATE supertrait
//! (`recursive_self_sealed::Sealed`) that is UNREACHABLE downstream. A hostile
//! downstream/shadow crate that owns its own `Arc` type therefore CANNOT
//! hand-write `impl RecursiveSelfArc<Recipe> for shadow::Arc<[Recipe]>` to FORGE
//! the proof-bound and re-open the bare-shadow hole: the private `Sealed<Recipe>`
//! supertrait is unsatisfiable and unnameable, so the impl fails to compile
//! (E0277).
//!
//! DISCRIMINATING: against the UNSEALED trait (no `Sealed` supertrait) this exact
//! hand-impl COMPILES and forges the proof-bound — the orphan rules let a
//! shadow-`Arc` owner masquerade as the approved self-container. Against the
//! SEALED trait it FAILS. Without the seal, `__private` is documentation, not a
//! compiler barrier.

use std::marker::PhantomData;

// A local stand-in for `ClosednessRecipe` (the sealed proof is independent of
// what the shadow `Arc` holds).
struct Recipe;

mod shadow {
    use super::PhantomData;

    // A custom `Arc` a hostile downstream crate owns — one that could hold a
    // `verter_span::Span`. `T: ?Sized` so the `[Recipe]` slice element is
    // well-formed.
    pub struct Arc<T: ?Sized>(pub PhantomData<*const T>);
}

// The FORGERY attempt: hand-impl the proof-trait for the shadow `Arc`. The trait
// is sealed, so this fails — the private `Sealed<Recipe>` supertrait is not (and
// cannot be) implemented for `shadow::Arc<[Recipe]>`.
impl verter_no_storedspan::__private::RecursiveSelfArc<Recipe> for shadow::Arc<[Recipe]> {}

fn main() {
    let _ = std::mem::size_of::<Recipe>();
}
