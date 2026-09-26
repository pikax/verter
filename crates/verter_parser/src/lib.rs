//! # verter_parser — Vue SFC parser

#[macro_use]
extern crate verter_debug_assert;

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
