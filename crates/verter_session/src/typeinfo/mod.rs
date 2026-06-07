#![deny(missing_docs)]
//! Public host typeinfo substrate.
//!
//! Audited / non-audited host methods that expose the shallow-state
//! symbol inventory and the dispatch-backed type evaluation surface
//! to downstream consumers (the `@verter/typeinfo` TS package, MCP
//! tools, IDE integrations).
//!
//! Public methods (the published surface of the typeinfo
//! substrate):
//!
//! 1. [`crate::VerterHost::list_file_symbols`] — shallow read; no
//!    audit.
//! 2. [`crate::VerterHost::resolve_named_symbol`] /
//!    [`crate::VerterHost::resolve_named_symbol_with_audit`] —
//!    audited resolution of a named declaration with optional
//!    generic instantiation.
//! 3. [`crate::VerterHost::evaluate_type_expression_with_audit`] —
//!    audited evaluation of a synthetic type expression in a
//!    file scope, with an optional host-owned scratch cache.
//! 4. [`crate::VerterHost::resolve_shallow_surface`] — resolves a
//!    named declaration to its span-rich, one-level
//!    [`surface::TypeInfoSurface`] (members + call / construct / index
//!    signatures + the keyspace) WITHOUT expanding member bodies. The
//!    shallow-by-default rule holds: each member `value` is a
//!    reference-style node, never an eagerly expanded object. Every
//!    span on the returned surface is a [`surface::CanonicalSpan`]
//!    (byte offsets + the canonical declaration file), so a consumer
//!    slices the source on demand at the FFI / consumer boundary —
//!    the surface itself holds NO owned type / display strings.
//!
//! Sub-modules:
//! - [`types`] — public DTOs (`SymbolEntry`, `EvaluateTypeExpressionRequest`, …).
//! - [`symbol_inventory`] — `list_file_symbols` impl.
//! - [`resolve_named_symbol`] — `resolve_named_symbol_with_audit` impl.
//! - [`evaluate_type_expression`] — `evaluate_type_expression_with_audit` impl.
//! - [`shallow_surface`] — `resolve_shallow_surface` impl.
//! - [`surface`] — the span-rich [`surface::TypeInfoSurface`] projection.
//! - [`scratch_cache`] — host-owned LRU for typeinfo scratch URIs.

pub mod adapters;
pub mod evaluate_type_expression;
pub mod raise;
pub mod request_validation;
pub mod resolve_named_symbol;
pub(crate) mod scratch_cache;
pub mod shallow_surface;
pub mod surface;
pub mod symbol_inventory;
pub mod types;

pub use resolve_named_symbol::ResolveMode;
pub use surface::{
    CanonicalSpan, SurfaceMemberOrigin, TypeInfoIndexSignature, TypeInfoSurface,
    TypeInfoSurfaceMember, TypeInfoSurfaceSignature,
};
pub use types::{
    EvaluateTypeExpressionRequest, ImportSpec, NamedImport, ShallowSurfaceRequest, SymbolEntry,
    SymbolKind, TypeArgList, TypeInfoQueryLevel, VueMacroSurfaceRequest,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod typeinfo_tests;

// The TS7 `TypeExpr`-projection oracle harness core (normalizer, snapshot
// schema, identity derivation, admission gate, probe synthesis, hover
// extraction, source-side walk, pure-data registry, and — under `oracle-gen` —
// the tsgo-driving snapshot generator). Compiled in `test` mode (the lifted
// unit-test rows consume it) AND under the `oracle-gen` feature (the
// `src/bin/oracle_gen` generator reaches its `run_oracle_gen` entry — a
// `src/bin/*` is a separate crate that sees only `pub`/`pub(crate)` lib items,
// not the `#[cfg(test)]` `typeinfo_tests` tree). It lifts ZERO rows: it is the
// shared foundation that later row-lifts consume. The tsgo driver lives behind
// `oracle-gen` only, so the default resolver build + default test gate stay
// tsgo-free (the `tsgo`-forbidden-at-runtime invariant, design §3 inv 1).
#[cfg(any(test, feature = "oracle-gen"))]
pub(crate) mod oracle_core;
