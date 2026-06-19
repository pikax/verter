//! Private sorted manifest of the consolidated integration-test entries.
//! One `mod <entry>;` per former top-level `tests/<entry>.rs` target. Each
//! entry is its own module so per-entry helpers stay in disjoint scopes —
//! do NOT centralise shared helpers here, and keep this list sorted.

mod component_meta_flags_audit;
mod proto_audit;
mod synthetic_slot_binding_graph;
mod typeinfo_proto_roundtrip;
mod typeinfo_proto_ts_freshness;
