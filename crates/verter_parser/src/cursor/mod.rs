//! Position tracking and parser-owned script language classification.
//!
//! Provides [`Cursor`](cursor::Cursor) for byte-level position tracking during tokenization,
//! script dialect classification, and line offset computation for source maps.

#![allow(clippy::module_inception)]

pub mod cursor;
pub mod lang;
pub mod lines;
pub mod position;
pub use lang::ScriptLanguage;
