#![deny(missing_docs)]
//! Framework adapters layered over the shared typeinfo surface.
//!
//! Adapters translate a framework-specific carrier (a `.vue` SFC today, other
//! component frameworks in future) into the typeinfo substrate's value types
//! (`TypeInfoSurface`, the component-meta DTOs) WITHOUT introducing a second
//! resolver. Every adapter sources its meaning from the shared typeinfo
//! surface path (`VerterHost::resolve_shallow_surface_for` and the lowering
//! dispatch it routes through) — never from a parallel reader.
//!
//! Sub-modules:
//! - [`vue`] — the Vue SFC adapter: a `.vue`'s public component type, the
//!   FullMetadata macro surfaces, and the prop / emit / slot normalizers that
//!   produce the final component-meta DTOs from the typeinfo surface.
//! - [`svelte`] — the Svelte carrier's blessed parse accessor (the surface
//!   adapter is a later vertical; the carrier registers `Deferred` until then).

pub mod svelte;
pub mod vue;
