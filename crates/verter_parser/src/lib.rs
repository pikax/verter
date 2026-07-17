//! # verter_parser — Vue SFC parser

pub mod ast;
pub mod common;
pub mod cursor;
pub mod diagnostics;
pub mod parser;
pub mod svelte_reactivity;
pub mod tokenizer;
pub mod types;
pub mod utils;

#[cfg(test)]
pub(crate) mod test_helpers;
