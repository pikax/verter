//! The sealed normalizer-input contract shared by the framework-surface
//! resolved-surface tokens.
//!
//! [`ResolvedSurfaceAccess`] is the SOLE input the Vue per-kind NORMALIZERS
//! (`vue_exec::normalize`) accept — instead of a forgeable `&VueMacroSurface` /
//! `&TypeInfoSurfaceMember` pair. It is implemented in THIS file ONLY, for
//! EXACTLY the two framework-surface resolved-surface tokens
//! ([`vue_exec::ResolvedVueSurface`] and [`svelte_exec::SvelteResolvedSurface`]).
//!
//! The supertrait seal [`Sealed`] is PRIVATE to this file (a bare module-private
//! `trait`), so no module OUTSIDE this file can name or implement `Sealed`. A
//! `framework_surface` sibling that writes `impl Sealed for ItsOwnWrapper` is a
//! COMPILE ERROR (`E0603` — `Sealed` is private), so it cannot implement
//! [`ResolvedSurfaceAccess`] and drive the normalizers over a forged surface.
//! The trait is therefore STRUCTURALLY implemented only for the two tokens: the
//! seal is private here, both impls live here, and a sibling impl does not
//! compile.
//!
//! It lives at the `framework_surface` level (NOT inside `vue_exec`) because it
//! is a SHARED contract both the Vue and Svelte legs route through; nesting it
//! inside `vue_exec` would also place it inside the
//! `TypeinfoVueSurfaceOutputCap` mint scope, which the per-leaf mint-scope guard
//! correctly rejects (a non-sink helper inside a cap's mint scope).

use super::svelte_exec::SvelteResolvedSurface;
use super::vue_exec::{ResolvedVueSurface, VueMacroSurface};

/// Private supertrait seal for [`ResolvedSurfaceAccess`]. It is a bare
/// module-private `trait` in THIS file: no module outside
/// `resolved_surface_access.rs` can name it, so no module outside this file can
/// implement [`ResolvedSurfaceAccess`] (a sibling `impl Sealed for Foo` is
/// `E0603`). The two sanctioned impls below are the only `impl Sealed`s in the
/// crate.
trait Sealed {}

/// The SEALED INTERNAL accessor the Vue per-kind NORMALIZERS read — the sole
/// input they accept (instead of a forgeable `&VueMacroSurface` /
/// `&TypeInfoSurfaceMember` pair). It is implemented in this file ONLY, for
/// EXACTLY the two resolved-surface tokens; the file-private supertrait seal
/// [`Sealed`] makes any out-of-file impl a compile error.
///
/// It exposes the resolved surface data BY BORROW (no clone / no alloc): both
/// the Vue token and the Svelte token wrap a resolution-derived
/// [`VueMacroSurface`] carrier, so a single shared normalizer set drives off the
/// SAME borrowed accessor for both frameworks.
///
/// `#[allow(private_bounds)]`: the file-private [`Sealed`] supertrait is the
/// POINT — the trait is `pub(crate)` (the normalizers name it as a bound) but
/// the seal stays private so no out-of-file module can implement it. This is the
/// standard sealed-trait shape; the lint that flags a more-private supertrait is
/// exactly what the seal intends.
#[allow(private_bounds)]
pub(crate) trait ResolvedSurfaceAccess: Sealed {
    /// The resolution-derived Vue macro surface the normalizers read (members,
    /// signatures, macro kind, per-member scope helpers), by borrow.
    fn macro_surface(&self) -> &VueMacroSurface;
}

// ── The ONLY `impl Sealed` / `impl ResolvedSurfaceAccess` in the crate ──────
// Both token types are nameable here (`ResolvedVueSurface` is `pub(crate)`;
// `SvelteResolvedSurface` is `pub(in crate::typeinfo::framework_surface)`), but
// each keeps its constructor + private `_seal` field in its OWNING module, so
// only that module mints. The trait reads the inner carrier through each
// token's `pub(in …framework_surface)` `surface_carrier()` accessor — the seal
// field stays private to the owning module.

impl Sealed for ResolvedVueSurface {}

impl ResolvedSurfaceAccess for ResolvedVueSurface {
    fn macro_surface(&self) -> &VueMacroSurface {
        self.surface_carrier()
    }
}

impl Sealed for SvelteResolvedSurface {}

impl ResolvedSurfaceAccess for SvelteResolvedSurface {
    fn macro_surface(&self) -> &VueMacroSurface {
        self.surface_carrier()
    }
}
