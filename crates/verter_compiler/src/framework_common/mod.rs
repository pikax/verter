//! Framework adapter plumbing owned by the compiler.
//!
//! Hosts the per-framework carrier bridges between the parser's typed
//! parse results and the framework-neutral
//! [`verter_language::FrameworkParseArtifact`]. The compiler is the one
//! crate BOTH producers (parse pipelines) and the session (carrier
//! consumers) can name without dependency cycles, so the concrete
//! `CarrierParse` wrappers live here rather than in `verter_parser`
//! (the wrapper is adapter plumbing, not parser data) or
//! `verter_session` (unnameable from compiler-side producers).
//!
//! On top of the carrier wrappers it owns the compiler-side carrier
//! framework substrate: the [`CarrierCompiler`] trait (parse / eval /
//! IDE / template), the [`CarrierCompilerRegistry`] the host's carrier
//! dispatch looks up, and the blessed [`CarrierCompilerCtx`] downcast
//! (D-m). Vue is the reference implementation
//! ([`vue_bridge::VueCarrierCompiler`]), delegating call-for-call to the
//! existing Vue pipeline with ZERO edits to any Vue parser/codegen
//! module.

pub mod carrier_compiler;
pub mod ctx;
pub mod registry;
pub mod vue_bridge;

/// Reusable framework IDE sourcemap end-to-end assertion helpers, shared
/// by every carrier vertical's `#[cfg(test)]` sourcemap suite.
#[cfg(test)]
pub mod sourcemap_e2e_helpers;

pub use carrier_compiler::{
    CarrierCompiler, CompileUnsupported, IdeCompileOptions, IdeOutput, ParseOptions, TemplateFacts,
};
pub use ctx::CarrierCompilerCtx;
pub use registry::CarrierCompilerRegistry;
