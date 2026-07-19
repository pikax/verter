//! OXC-based AST utilities for analyzing Vue SFC scripts and templates.
//!
//! This module provides helpers that operate on OXC-parsed ASTs to extract
//! binding information, resolve types, and handle Vue-specific syntax.
//!
//! # Submodules
//!
//! - [`bindings`] — Binding extraction from expressions (identifiers, functions, literals).
//!   Re-exported at this level for convenience.
//! - [`script`] — Framework-neutral script analysis: the raw pre-lowering
//!   statement surface, the local type-surface capture engine
//!   (`script::type_inventory`), and the generic import/decl binding inventory.
//! - [`vue`] — Vue-specific analysis: directive parsing (`v-for`, `v-slot`),
//!   script block parsing (Options API and `<script setup>`), and template helpers.

pub mod bindings;
pub mod script;
pub mod vue;

// Re-export everything from bindings at the oxc level for convenience
pub use bindings::*;
