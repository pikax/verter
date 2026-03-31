//! OXC-based AST utilities for analyzing Vue SFC scripts and templates.
//!
//! This module provides helpers that operate on OXC-parsed ASTs to extract
//! binding information, resolve types, and handle Vue-specific syntax.
//!
//! # Submodules
//!
//! - [`bindings`] — Binding extraction from expressions (identifiers, functions, literals).
//!   Re-exported at this level for convenience.
//! - [`vue`] — Vue-specific analysis: directive parsing (`v-for`, `v-slot`),
//!   script block parsing (Options API and `<script setup>`), and template helpers.
//!   - `vue::script::resolve_type` (2k+ LOC) handles cross-file type resolution
//!     for `defineProps<ExternalType>()` and similar macros. It lives here because
//!     it is called during compilation, not just static analysis.
//!
//! If the `vue::script` submodule grows further, consider extracting it into a
//! dedicated `verter_semantic::analysis` crate.

pub mod bindings;
pub mod vue;

// Re-export everything from bindings at the oxc level for convenience
pub use bindings::*;
