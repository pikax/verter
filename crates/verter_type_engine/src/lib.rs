//! Verter's semantic type engine: the semantic query vocabulary and its graph
//! store, the project semantic dispatch, the signature kernel, the cache
//! runtime, the request context and fact-tracing runtime, and the resolver
//! request contracts the host implements.
//!
//! The engine depends only on the shared query vocabulary, the execution
//! substrate and parser-free leaves. The host (`verter_session`) owns the
//! stores' instances, implements the request ports, and drives the engine.

#![allow(clippy::too_many_arguments)]
// Unsafe code is confined to the fact recorder stack in `fact_tracing::tracing`,
// which opts back in explicitly. The output and surface-claim authorities rely
// on safe visibility rules, which unsafe code elsewhere could bypass.
#![deny(unsafe_code)]

#[macro_use]
extern crate verter_debug_assert;

pub mod bounded_query_retention;
pub mod cache_runtime;
pub mod cache_schema;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod cancel_trace;
#[cfg(any(test, feature = "test-support"))]
pub mod capture_token;
pub mod component_meta_caches;
pub mod component_meta_result_db;
pub mod engine_provenance;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod engine_test_knobs;
pub mod fact_signature_helpers;
pub mod fact_tracing;
pub mod flow_return_audit;
pub mod graph_walk;
pub mod identity_interner;
pub mod instant;
pub mod intrinsic_registry;
pub mod invalidation_domain;
/// Session-side key identities for locator-backed body lowering
/// (`LocatorLoweringKey` + the sealed R6 key-dimension witness +
/// `SessionDemandIdentity`). Crate-private: the substrate is B1-internal;
/// only the two R6-witness compile-fail helpers below are re-exported.
pub mod locator_identity;
/// Loop-5 inner-dispatch instrumentation counters. Inert in production —
/// callers bump atomic counters at named call sites and dump aggregates
/// via `dump_loop5_instrumentation_counters()`.
pub mod loop5_instrumentation;
pub mod mapper_binder_registry;
pub mod project_semantic_dispatch;
pub mod request_budget;
pub mod request_context;
pub mod request_footprint;
pub mod request_observers;
pub mod request_route_memo;
#[cfg_attr(not(test), allow(dead_code))]
pub mod semantic_execution;
pub mod semantic_query;
pub mod semantic_query_memo;
pub mod signature_kernel;
pub mod structural_carrier_producer;

pub mod resolver_core {
    //! The engine's resolver request contracts: the resolver context and its
    //! capabilities, the request ports and fact-validation port, and the
    //! engine-side name, scope and ambient resolution over them.
    pub mod ambient_resolve;
    pub mod bare_name_resolve;
    pub mod dispatch_profile;
    pub mod fact_validation_port;
    pub mod fuses;
    pub mod request_ports;
    pub mod resolver_context;
    pub mod scope_shadowing;

    pub use fuses::{FuseBudgets, FuseState, FuseTrip};
    pub use resolver_context::{
        MaterializeScopeObservation, ProjectGenerationClock, RequestBoundResolverContext,
        RequestFlags, RequestSnapshot, ResolverCapabilities, ResolverContext,
    };
}

pub mod meta_resolve {
    //! Dispatch dependency-signature fact emission.
    pub mod dep_signature;

    pub use dep_signature::emit_dispatch_dep_signature_facts;
}
