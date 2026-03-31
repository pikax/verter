//! Semantic fact types derived from parser, compiler, and workspace inputs.
//!
//! Facts are immutable, cacheable values produced by semantic queries.
//! They represent the normalized semantic truth about components, bindings,
//! reactivity, routes, CSS flow, and other cross-cutting concerns.

pub mod binding;
pub mod component;
pub mod reactivity;
