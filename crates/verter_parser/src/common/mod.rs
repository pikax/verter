//! Shared span and position types used across the compiler pipeline.
//!
//! Provides [`Span`] for byte ranges and basic source location types.

pub mod html_entities;
pub mod span;
pub mod types;

pub use span::*;
pub use types::*;
