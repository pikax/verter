//! Private sorted manifest of the consolidated integration-test entries.
//! One `mod <entry>;` per former top-level `tests/<entry>.rs` target. Each
//! entry is its own module so per-entry helpers stay in disjoint scopes —
//! do NOT centralise shared helpers here, and keep this list sorted.

mod fact_value_ownership;
mod resolver_stay_ownership;
// Drives the workspace audit producer, which exists only with optional capture.
#[cfg(feature = "semantic-observe")]
mod workspace_audit_dep_graph_traverse;
// Drives the workspace audit producer, which exists only with optional capture.
#[cfg(feature = "semantic-observe")]
mod workspace_audit_resolve;
