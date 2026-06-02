#![deny(missing_docs)]
//! The Vue SFC typeinfo adapter.
//!
//! Turns a `.vue` single-file component into the shared typeinfo substrate's
//! value types, sourcing ALL meaning from the shared surface path
//! (`VerterHost::resolve_shallow_surface_for` + the lowering dispatch it
//! routes through) — there is NO Vue-specific type resolver here.
//!
//! Three concerns:
//! - [`store`] — the host-owned cache of a `.vue`'s extracted shallow macro
//!   metadata + public surface, materialized ONCE per `(canonical, content)`
//!   per the Shallow File Processing Core Invariant.
//! - [`public_type`] — build a `.vue`'s PUBLIC component type (the synthesized
//!   `$props` / `$emit` / `$slots` / expose instance surface) so a TS
//!   `import Foo from './Foo.vue'` resolves through typeinfo WITHOUT
//!   component-meta. ([`crate::typeinfo::types::TypeInfoQueryLevel::PublicType`].)
//! - [`surface`] — `resolve_vue_macro_surface` (FullMetadata) + the
//!   `props_from_typeinfo_surface` / `emits_from_typeinfo_surface` /
//!   `slots_from_typeinfo_surface` normalizers that produce the final
//!   `AnalyzedPropField` / `AnalyzedEmitField` / `AnalyzedSlotField` DTOs from
//!   the typeinfo surface + macro-analyzer facts.

pub mod public_type;
pub mod runtime_ctor;
pub mod store;
pub mod surface;

pub use surface::VueMacroSurface;
// The three macro-surface normalizers are consumed inside `surface.rs` itself
// (the `vue_macro_dtos_with_ctx` DTO core) and, via this re-export, by the
// in-crate vue-adapter tests. No production caller outside `surface` reaches
// them through this path, so the re-export is test-scoped.
#[cfg(test)]
pub(crate) use surface::{
    emits_from_typeinfo_surface, props_from_typeinfo_surface, slots_from_typeinfo_surface,
};
