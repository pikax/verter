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
pub mod framework_surface;
pub mod raise;
pub mod request_validation;
pub mod resolve_named_symbol;
pub(crate) mod scratch_cache;
pub mod shallow_surface;
pub mod surface;
pub mod symbol_inventory;
pub mod types;
pub(crate) mod vue_macro_codegen;

// The output-sink capabilities for this subtree are defined PER-SINK in the
// exact output-SINK modules that project — NOT subtree-wide:
// `TypeinfoRaiseOutputCap` in `raise.rs`, `TypeinfoVueSurfaceOutputCap` in
// `framework_surface/vue_exec/` (whose whole reachable scope — `vue_exec` +
// its `normalize` child — is output-only, so the single cap is correct), and
// `TypeinfoSvelteSurfaceOutputCap` in `framework_surface/svelte_exec.rs` (whose
// only submodule is a `#[cfg(test)]` test module). A subtree-wide cap
// (`pub(in crate::typeinfo)`) would let any `typeinfo` sibling (e.g. a future
// `framework_surface::executor` raise-then-decide site) mint it; terminal-sink
// minting (each mint scope's whole reachable production module tree is
// output-only) makes the output-materialization fence compiler-enforced.

/// Bounded number of times a typeinfo query-returner re-reads the base
/// store view trying to settle on a proven-[`crate::resolver_store::StoreViewRead::Current`]
/// snapshot under churn before reporting a non-current miss.
///
/// A typeinfo query-returner builds a request-bound dispatch context from
/// the base store view and RETURNS the resolved node — there is no outer
/// `run_stable_request` publish fence to suppress a stale answer. So it MUST
/// resolve against a CURRENT snapshot: a `ReturnOnly` read means the manager
/// could not prove the snapshot coherent, and computing the query against it
/// would return a result derived from already-superseded dependency state.
/// The retry budget mirrors the store-view manager's own bounded
/// no-torn-snapshot retries — a transient mutation burst settles within a
/// few rounds; sustained churn exhausts the budget and the query-returner
/// surfaces a miss (`None`) rather than a stale answer.
pub(crate) const TYPEINFO_CURRENT_VIEW_RETRY_ATTEMPTS: usize = 3;

/// Acquire a proven-[`crate::resolver_store::CurrentHostStoreView`] for a
/// typeinfo query-returner, retrying a bounded number of times when the
/// store-view manager hands back a known-stale
/// [`crate::resolver_store::StoreViewRead::ReturnOnly`] read under churn.
///
/// Returns `None` when every attempt within
/// [`TYPEINFO_CURRENT_VIEW_RETRY_ATTEMPTS`] observed a non-current read —
/// the host is under sustained churn and the query CANNOT be answered
/// against a coherent snapshot. `None` is the established typeinfo miss
/// signal ("could not be resolved"): the FFI surface already maps it to a
/// `null` payload, so the consumer re-queries on the next request once the
/// host settles. The loop is bounded, so it always terminates — it never
/// spins.
#[must_use]
pub(crate) fn current_store_view_for_query(
    host: &crate::VerterHost,
) -> Option<crate::resolver_store::CurrentHostStoreView> {
    for _ in 0..TYPEINFO_CURRENT_VIEW_RETRY_ATTEMPTS {
        if let Some(current) = host.resolver_store_view_read().current() {
            return Some(current);
        }
        // Non-current read: the manager could not prove this snapshot
        // coherent. Re-read — a transient mutation burst settles within
        // the bounded budget. No sleep / yield: the manager's own
        // build_coherent retry already absorbs the contention window, and
        // a tight bounded re-read keeps the path non-blocking.
    }
    None
}

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
// not the `#[cfg(test)]` `typeinfo_tests` tree). It declares no oracle rows
// itself: it is the shared foundation the `TypeExpr`-projection oracle rows
// consume. The tsgo driver lives behind
// `oracle-gen` only, so the default resolver build + default test gate stay
// tsgo-free (the `tsgo`-forbidden-at-runtime invariant, design §3 inv 1).
#[cfg(any(test, feature = "oracle-gen"))]
pub(crate) mod oracle_core;
