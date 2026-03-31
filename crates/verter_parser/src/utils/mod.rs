//! Utility wrappers for OXC parsing and Vue-specific tag/attribute helpers.
//!
//! - [`oxc`] — OXC AST wrappers for expression parsing and script analysis
//! - [`vue`] — Vue tag detection (void tags, built-in components, directives)

pub mod oxc;
pub mod vue;
