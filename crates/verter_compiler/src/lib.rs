//! # verter_compiler — Vue SFC compiler
//!
//! Compiler crate for the Verter Vue compiler. Handles codegen, lowering,
//! IDE output, style processing, and source maps.
//!
//! Parser-owned modules (tokenizer, parser, ast, cursor, types, common,
//! diagnostics) are re-exported from `verter_parser` for backward
//! compatibility. `verter_parser::utils` is crate-internal only: consumers
//! that need the parser's script/type-surface utilities depend on
//! `verter_parser` directly.

// ── Re-exports from verter_parser ──────────────────────────────────────────
pub use verter_parser::ast;
pub use verter_parser::common;
pub use verter_parser::cursor;
pub use verter_parser::diagnostics;
pub use verter_parser::parser;
pub use verter_parser::tokenizer;
pub use verter_parser::types;
pub(crate) use verter_parser::utils;

// ── Compiler-owned modules ─────────────────────────────────────────────────

#[cfg(feature = "bench")]
pub mod code_transform;
#[cfg(not(feature = "bench"))]
pub(crate) mod code_transform;

pub mod css;
pub mod js_number;
pub mod strip_types;

pub mod compile;

pub mod framework_common;

pub mod svelte;

/// Shared emitted-module semantic-comment oracle used by the Svelte
/// conformance comparator and golden-generation tooling.
#[doc(hidden)]
pub mod svelte_semantic_comments;

// Reusable Svelte golden topology-diff comparison engine (the normalized
// topology schema + the identity/structure/helper-topology diff). Gated behind
// `svelte-oracle` so the DEFAULT build never compiles it; every golden-diff
// consumer imports the SAME diff engine from here rather than its own fork.
// Today the consumer is the reference-drift gate (committed goldens vs the
// pinned Svelte compiler); a Verter-output conformance consumer is a follow-up
// for when the native Svelte codegen lands.
#[cfg(feature = "svelte-oracle")]
pub mod svelte_oracle;

#[cfg(feature = "bench")]
pub mod script;
#[cfg(not(feature = "bench"))]
pub(crate) mod script;

#[cfg(feature = "bench")]
pub mod style;
#[cfg(not(feature = "bench"))]
pub(crate) mod style;

#[cfg(feature = "bench")]
pub mod template;
#[cfg(not(feature = "bench"))]
pub(crate) mod template;

// IDE code generation (TSX for TS projects, JSX+JSDoc for JS projects)
pub(crate) mod ide;

// TSC code generation (vue-tsc replacement)
pub mod tsc;

// Re-export the @verter/types declarations for the LSP and verter-tsc
pub use ide::script::global_component_nav_probe_offset;
pub use ide::script::VERTER_TYPES_AMBIENT_MODULE;
pub use ide::script::VERTER_TYPES_STANDALONE_DTS;

#[cfg(test)]
mod compile_ported_tests;
#[cfg(test)]
mod sourcemap_e2e_tests;
#[cfg(test)]
pub(crate) mod test_helpers;
