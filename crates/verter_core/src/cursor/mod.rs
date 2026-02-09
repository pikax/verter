#![allow(clippy::module_inception)]

pub mod cursor;
pub mod lang;
pub mod lines;
pub mod position;
pub mod script_detector;

// Re-export the main detector for convenience
pub use script_detector::{DetectResult, ScriptDetector, ScriptLanguage};
