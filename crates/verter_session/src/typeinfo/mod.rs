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
//! 3. `evaluate_type_expression_with_audit` (a `test-support` harness) —
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
//! - `evaluate_type_expression` — the string-expression test harness (`test-support`).
//! - [`shallow_surface`] — `resolve_shallow_surface` impl.
//! - [`capabilities`] — the TypeInfo capability handshake (`typeinfo_capabilities`).
//! - [`surface`] — the span-rich [`surface::TypeInfoSurface`] projection.
//! - `scratch_cache` — the harness's LRU of scratch URIs (`test-support`).

pub mod adapters;
pub mod capabilities;
// The string-expression evaluator is a test harness: the public API no
// longer evaluates arbitrary expressions.
#[cfg(any(test, feature = "test-support"))]
pub mod evaluate_type_expression;
#[cfg(feature = "semantic-observe")]
pub(crate) mod footprint_attach;
pub mod framework_surface;
pub mod graph_query;
pub(crate) mod native_basis;
pub(crate) mod node_refs;
pub mod raise;
pub mod request_validation;
pub mod resolve_named_symbol;
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod scratch_cache;
pub(crate) mod selector;
pub mod shallow_surface;
pub mod surface;
pub(crate) mod surface_resolution;
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

/// The store-view manager could not prove a current view within its own
/// bounded handoff. Not a miss and not an eligible refusal: nothing is
/// published, warmed or issued under it. A request whose view never
/// settles has no input basis, so a TypeInfo
/// request answers it with the request-level `UnstableState` error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct UnsettledInput {
    reason: crate::resolver_store::StoreViewReturnOnlyReason,
}

impl UnsettledInput {
    /// Manager rounds spent: the manager's budget when every build was
    /// superseded or declined; 0 for a re-entrant build claim, where no
    /// settle round ran.
    #[must_use]
    pub fn attempts(self) -> u8 {
        use crate::resolver_store::StoreViewReturnOnlyReason;
        match self.reason {
            StoreViewReturnOnlyReason::PublishDeclined | StoreViewReturnOnlyReason::Superseded => {
                u8::try_from(crate::resolver_store::STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS)
                    .unwrap_or(u8::MAX)
            }
            StoreViewReturnOnlyReason::ReentrantBuildClaim => 0,
        }
    }
}

impl From<UnsettledInput> for verter_protocol::verter::v1::TypeInfoRequestError {
    fn from(unsettled: UnsettledInput) -> Self {
        Self {
            kind: Some(
                verter_protocol::verter::v1::type_info_request_error::Kind::UnstableState(
                    verter_protocol::typeinfo::graph::wire_error_unstable_state(u32::from(
                        unsettled.attempts(),
                    )),
                ),
            ),
        }
    }
}

impl From<UnsettledInput> for crate::host_resolve_type_audit::TypeResolutionRequestError {
    fn from(unsettled: UnsettledInput) -> Self {
        Self::UnstableState {
            attempts: unsettled.attempts(),
        }
    }
}

impl From<UnsettledInput> for crate::host_flow_return_audit::FlowReturnError {
    fn from(unsettled: UnsettledInput) -> Self {
        Self::UnstableState {
            attempts: unsettled.attempts(),
        }
    }
}

/// Take the proven-current store view a typeinfo query-returner resolves
/// against: exactly one manager read. The store-view manager is the single
/// retry authority (its handoff already runs the bounded settle rounds and
/// parks on another thread's build), so a non-current read is answered at
/// once as [`UnsettledInput`], never retried here and never reported as a
/// miss.
pub(crate) fn current_store_view_for_query(
    host: &crate::VerterHost,
) -> Result<crate::resolver_store::CurrentHostStoreView, UnsettledInput> {
    match host.resolver_store_view_read() {
        crate::resolver_store::StoreViewRead::Current(current) => Ok(current),
        crate::resolver_store::StoreViewRead::ReturnOnly { reason, .. } => {
            Err(UnsettledInput { reason })
        }
    }
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
pub(crate) mod typeinfo_tests;

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
