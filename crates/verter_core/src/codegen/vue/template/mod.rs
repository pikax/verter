//! Vue template code generation.
//!
//! This module provides code generation helpers for Vue template elements:
//! - Elements (native/components)
//! - Interpolations (`{{ expression }}`)
//! - Directives (v-bind, v-on, v-if, v-for, v-slot, etc.)
//! - Text nodes

pub mod directives;
pub mod element;
pub mod interpolation;
pub mod types;
