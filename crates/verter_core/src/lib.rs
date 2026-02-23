//! # verter_core — Vue SFC compiler
//!
//! Core Rust crate for the Verter Vue compiler. Handles the full compilation
//! pipeline from Vue Single File Components (SFCs) to compiled output:
//!
//! ```text
//! Vue SFC source
//!     ↓ tokenizer   — byte-level SFC tokenization
//!     ↓ parser      — builds arena-based template AST + extracts script/style blocks
//!     ↓ script      — macro processing, binding extraction, component wrapper
//!     ↓ template    — render function codegen (VDOM or Vapor backends)
//!     ↓ style/css   — scoped CSS, CSS Modules, v-bind() replacement
//!     ↓ compile     — orchestrates the above, applies CodeTransform, emits output
//! ```
//!
//! ## Crate boundaries
//!
//! - **Rust** (this crate): tokenization → parsing → AST → template/script/style codegen
//! - **TypeScript** (`@verter/core`): SFC-to-TSX transformation for IDE type checking
//! - **NAPI** (`verter_napi`): bridges this crate to Node.js for build-time use (unplugin)
//! - **WASM** (`verter_wasm`): bridges this crate to the browser for the playground
//!
//! ## Visibility
//!
//! Modules used only by `verter_bench` (`ast`, `code_transform`, `script`,
//! `style`, `template`) are feature-gated behind the `bench` feature.
//! They are `pub(crate)` by default and become `pub` when
//! `features = ["bench"]` is enabled.

// Shared infrastructure
#[cfg(feature = "bench")]
pub mod code_transform;
#[cfg(not(feature = "bench"))]
pub(crate) mod code_transform;

pub mod common;
pub mod css;
pub mod cursor;
pub mod strip_types;
pub mod tokenizer;
pub mod utils;

// Diagnostic infrastructure
pub mod diagnostics;

// Core compilation modules
#[cfg(feature = "bench")]
pub mod ast;
#[cfg(not(feature = "bench"))]
pub(crate) mod ast;

pub mod compile;
pub mod parser;

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

pub mod types;

#[cfg(test)]
mod compile_ported_tests;
#[cfg(test)]
pub(crate) mod test_helpers;
