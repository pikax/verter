//! Deterministic covering-array construction and verification for Svelte compiled-output conformance suites.

#[macro_use]
extern crate verter_debug_assert;

pub mod covering_array;
pub mod generate;
pub mod manifest;
pub mod model;
pub mod value_wrap;
