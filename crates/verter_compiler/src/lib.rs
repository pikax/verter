//! # verter_compiler — Vue SFC compiler
//!
//! Compiler crate for the Verter Vue compiler. Handles codegen, lowering,
//! IDE output, style processing, and source maps.
//!
//! Parser-owned modules (tokenizer, parser, ast, cursor, types, utils, common,
//! diagnostics) are re-exported from `verter_parser` for backward compatibility.

// ── Re-exports from verter_parser ──────────────────────────────────────────
pub use verter_parser::ast;
pub use verter_parser::common;
pub use verter_parser::cursor;
pub use verter_parser::diagnostics;
pub use verter_parser::parser;
pub use verter_parser::tokenizer;
pub use verter_parser::types;
pub use verter_parser::utils;

// ── Compiler-owned modules ─────────────────────────────────────────────────

#[cfg(feature = "bench")]
pub mod code_transform;
#[cfg(not(feature = "bench"))]
pub(crate) mod code_transform;

pub mod css;
pub mod strip_types;

pub mod compile;

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
pub use ide::script::VERTER_TYPES_AMBIENT_MODULE;
pub use ide::script::VERTER_TYPES_STANDALONE_DTS;

#[cfg(test)]
mod compile_ported_tests;
#[cfg(test)]
mod sourcemap_e2e_tests;
#[cfg(test)]
pub(crate) mod test_helpers;
#[cfg(test)]
mod v5_process_ported_tests;

/// Temporary observability probe used by the TLS-propagation harness's
/// cross-crate self-test. Returns `true` when an audit observer is
/// installed in the calling thread's TLS slot, `false` otherwise.
///
/// Removed once production component-compile audit producers ship from
/// this crate. At that point a separate architecture guard requires
/// every production audited entry-point in this crate to carry its
/// own TLS-propagation test, replacing the role this probe plays.
#[doc(hidden)]
pub fn _audit_harness_probe() -> bool {
    verter_audit::current_observer().is_some()
}
