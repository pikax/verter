//! SFC tokenizer — produces a flat event stream from Vue source text.
//!
//! Uses [`byte`] for zero-copy byte-level tokenization, emitting [`Event`]
//! values consumed by the [`parser`](crate::parser).

pub mod byte;
pub mod helpers;
pub mod types;

pub use types::{Event, QuoteType};

#[cfg(test)]
mod byte_tests;
