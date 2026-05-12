//! Semantic fact types derived from parser, compiler, and workspace inputs.
//!
//! Facts are immutable, cacheable values produced by semantic queries.
//! They represent the normalized semantic truth about components, bindings,
//! reactivity, routes, CSS flow, and other cross-cutting concerns.
//!
//! The [`registry`] sub-module hosts the parse-domain / resolve-domain
//! [`FactKey`] / [`Fact`] / [`FactRegistry`] schema that backs the
//! fact-based cache architecture (see
//! `.claude/skills/type-cache-architecture/SKILL.md`).

pub mod binding;
pub mod boundary;
pub mod component;
pub mod corender;
pub mod css;
pub mod reactivity;
pub mod registry;
pub mod route;
pub mod runtime_schema;
pub mod symbol;

pub use registry::{
    AugmentationTargetKindTag, Fact, FactDomain, FactHash, FactKey, FactLane, FactRegistry,
    MacroKind as FactMacroKind, MacroTargetKey, MemberKind, ObservedFact, SymbolSpace,
};
