//! The Svelte carrier compiler — parser + neutral artifact bridge.
//!
//! This module owns the Svelte-specific compiler surface: the byte parser
//! ([`parser`]) producing a [`parser::ParsedSvelte`], and the
//! [`carrier::SvelteCarrierCompiler`] that lifts it into the framework-neutral
//! [`FrameworkParseArtifact`](verter_compiler::framework_common::FrameworkParseArtifact) and
//! drives the four [`CarrierCompiler`](crate::framework_common::CarrierCompiler)
//! operations.
//!
//! It performs NO type lowering (the thin-adapters guard). The IDE TSX
//! projection ([`ide`]) is a pure syntactic transform via `CodeTransform` —
//! never type resolution.

/// The shared Svelte `bind:` contract table — the SOURCE OF TRUTH for the wide
/// binding family, consumed by BOTH the IDE projection ([`ide`]) and the runtime
/// client codegen ([`runtime`]). Lives at the `svelte` module root (not under
/// [`ide`]) so the runtime backend depends on it without depending on the IDE
/// projection.
pub mod attribute_expressions;
pub(crate) mod bind_contract;
mod bind_contract_data;
#[cfg(test)]
mod bind_contract_tests;
pub mod carrier;
pub mod carrier_frontend;
pub mod ide;
pub mod parser;
pub mod runtime;
pub mod semantic_authority;
pub mod svelte_projection_backend;
pub mod svelte_runtime_backend;
pub mod template_facts;

pub use carrier::SvelteCarrierCompiler;
pub use carrier_frontend::{
    svelte_carrier_frontend_registration, SvelteCarrierFrontend, SvelteSfc5,
};
pub use ide::{project_svelte_ide, SvelteIdeProjection};
pub use parser::{parse_svelte, ParsedSvelte};
pub use semantic_authority::{svelte_semantic_authority_registration, SvelteSemanticAuthority};
pub use svelte_projection_backend::{
    svelte_projection_backend_registration, SvelteIdeCompanion, SvelteProjectionBackend,
    SvelteProjectionDiagnostic, SvelteProjectionError, SvelteProjectionInputs,
};
pub use svelte_runtime_backend::{
    svelte_runtime_backend_registration, SvelteRuntimeBackend, SvelteRuntimeError,
    SvelteRuntimeInputs,
};
