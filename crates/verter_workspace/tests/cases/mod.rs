//! Private sorted manifest of the consolidated integration-test entries.
//! One `mod <entry>;` per former top-level `tests/<entry>.rs` target. Each
//! entry is its own module so per-entry helpers stay in disjoint scopes —
//! do NOT centralise shared helpers here, and keep this list sorted.

mod fact_value_ownership;
mod legacy_resolver_absence_compile_fail;
mod project_membership_old_path_compile_fail;
mod resolver_core_private_helpers_compile_fail;
mod resolver_stay_ownership;
mod workspace_audit_dep_graph_traverse;
mod workspace_audit_resolve;
