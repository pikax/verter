//! OXC-related utilities.
//!
//! This module contains utilities for working with OXC-parsed JavaScript/TypeScript:
//!
//! - `bindings`: Binding extraction from expressions (identifiers, functions, literals)
//! - `vue`: Vue-specific parsing utilities (v-for, v-slot)

pub mod bindings;
pub mod vue;

// Re-export everything from bindings at the oxc level for convenience
pub use bindings::*;
