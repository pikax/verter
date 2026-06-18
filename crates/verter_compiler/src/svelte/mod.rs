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

pub mod carrier;
pub mod ide;
pub mod parser;
pub mod runtime;
pub mod template_facts;

pub use carrier::SvelteCarrierCompiler;
pub use ide::{project_svelte_ide, SvelteIdeProjection};
pub use parser::{parse_svelte, ParsedSvelte};
