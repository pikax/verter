//! The Svelte carrier compiler — parser + neutral artifact bridge.
//!
//! This module owns the Svelte-specific compiler surface: the byte parser
//! ([`parser`]) producing a [`parser::ParsedSvelte`], and the
//! [`carrier::SvelteCarrierCompiler`] that lifts it into the framework-neutral
//! [`FrameworkParseArtifact`](verter_language::FrameworkParseArtifact) and
//! drives the four [`CarrierCompiler`](crate::framework_common::CarrierCompiler)
//! operations.
//!
//! It performs NO type lowering (D-o; the thin-adapters guard). The IDE TSX
//! projection (B8c) is NOT implemented here — `compile_ide` returns the typed
//! [`CompileUnsupported`](crate::framework_common::CompileUnsupported) answer
//! until that vertical lands.

pub mod carrier;
pub mod parser;

pub use carrier::SvelteCarrierCompiler;
pub use parser::{parse_svelte, ParsedSvelte};
