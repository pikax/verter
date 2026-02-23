//! Position tracking and script language detection for the tokenizer.
//!
//! Provides [`Cursor`](cursor::Cursor) for byte-level position tracking during tokenization,
//! [`ScriptDetector`] for detecting `<script lang="...">` attributes, and line
//! offset computation for source map generation.

#![allow(clippy::module_inception)]

pub mod cursor;
pub mod lang;
pub mod lines;
pub mod position;
pub mod script_detector;

// Re-export the main detector for convenience
pub use script_detector::{DetectResult, ScriptDetector, ScriptLanguage};
