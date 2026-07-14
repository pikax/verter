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
pub mod hashing;
pub mod reactivity;
pub mod registry;
pub mod route;
pub mod route_closure;
pub mod route_facts;
pub mod runtime_schema;
pub mod symbol;

pub use hashing::{
    compute_member_presence_hash, compute_member_shape_hash, compute_semantic_hash,
    type_body_fingerprint, value_body_fingerprint, CrossDeclLens, CrossDeclRef, HashOutcome,
    TransientTypeBody, UnresolvedLens, ValueBodyFingerprintInput, MAX_HASH_DEPTH,
};
pub use registry::{
    AugmentationTargetKindTag, Fact, FactDomain, FactHash, FactKey, FactLane, FactRegistry,
    MacroKind as FactMacroKind, MacroTargetKey, MemberKind, ObservedFact, SymbolSpace,
};
pub use route_closure::{
    local_closure_over_facts, route_closure_over_facts, ClassifiedRouteDeps, FactClosureResult,
    FactClosureStatus, KeySourceLookup, RouteClosureProvider,
};
pub use route_facts::{
    produce_key_source_fact, produce_shallow_route_facts, EmptyRouteFactLens, ImportRouteTarget,
    RouteFactLens,
};
