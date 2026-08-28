//! Private sorted manifest of the consolidated integration-test entries.
//! One `mod <entry>;` per former top-level `tests/<entry>.rs` target. Each
//! entry is its own module so per-entry helpers stay in disjoint scopes —
//! do NOT centralise shared helpers here, and keep this list sorted.

mod jsdoc_tag_type_payload_parity;
mod nested_special_pseudo_facts;
mod resolver_core_ownership;
mod resolver_observation_compile_fail;
