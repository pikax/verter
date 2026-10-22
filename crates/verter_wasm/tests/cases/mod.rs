//! Private sorted manifest of the consolidated integration-test entries.
//! One `mod <entry>;` per former top-level `tests/<entry>.rs` target. Each
//! entry is its own module so per-entry helpers stay in disjoint scopes —
//! do NOT centralise shared helpers here, and keep this list sorted.

// Walks a captured audit record, which exists only with optional capture.
#[cfg(feature = "semantic-observe")]
mod audit;
mod one_parse_per_style_block;
mod selector_structure_boundary;
