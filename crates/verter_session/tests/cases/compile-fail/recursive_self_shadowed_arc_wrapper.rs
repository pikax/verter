//! Compile-FAIL fixture: the tightened matcher accepts ONLY the standard `Arc`
//! wrapper (bare `Arc` / `std::sync::Arc` / `alloc::sync::Arc`). A shadowed /
//! custom `shadow::Arc` — which could OWN a `verter_type_expr::TypeExpr` — is
//! NOT the standard wrapper, so its `NoTypeExpr` bound is KEPT and fails.
//!
//! Pre-fix, the last-segment `== "Arc"` check matched `shadow::Arc` and OMITTED
//! the bound, so the custom wrapper's `TypeExpr` slipped through and this
//! COMPILED (a false witness).

use std::marker::PhantomData;

mod shadow {
    use super::PhantomData;

    // A custom `Arc` that owns a `TypeExpr` — the shadow the pre-fix last-segment
    // check could not distinguish from `std::sync::Arc`. `T: ?Sized` so the
    // `[Recipe]` slice element is well-formed; the failure is the KEPT
    // `NoTypeExpr` bound, not a sizing error.
    pub struct Arc<T: ?Sized> {
        pub bad: verter_type_expr::TypeExpr,
        pub _marker: PhantomData<*const T>,
    }
}

#[derive(verter_no_typeexpr::NoTypeExpr)]
#[no_typeexpr(recursive_self)]
enum Recipe {
    // The genuine fixed-point self-container via the standard `Arc` — its plain
    // witness bound is replaced by the `RecursiveSelfArc<Self>` proof-bound (so
    // the escape's at-least-one-`Arc<[Self]>` requirement holds).
    Rec(std::sync::Arc<[Recipe]>),
    // A custom `shadow::Arc<[Recipe]>` wrapper owning a `TypeExpr`. It is NOT the
    // standard `Arc`, so its `NoTypeExpr` bound is KEPT and unsatisfiable.
    Custom(shadow::Arc<[Recipe]>),
}

fn main() {
    let _ = std::mem::size_of::<Recipe>();
}
