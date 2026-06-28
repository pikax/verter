//! The Svelte carrier compiler — parser + neutral artifact bridge.
//!
//! This module owns the Svelte-specific compiler surface: the byte parser
//! ([`parser`]) producing a [`parser::ParsedSvelte`], and the
//! [`carrier::SvelteCarrierCompiler`] that lifts it into the framework-neutral
//! [`FrameworkParseArtifact`](verter_language::FrameworkParseArtifact) and
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
pub(crate) mod bind_contract;
mod bind_contract_data;
pub mod carrier;
pub mod ide;
pub mod parser;
pub mod runtime;
pub mod template_facts;

pub use carrier::SvelteCarrierCompiler;
pub use ide::{project_svelte_ide, SvelteIdeProjection};
pub use parser::{parse_svelte, ParsedSvelte};
