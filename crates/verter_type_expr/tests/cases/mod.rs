//! Private sorted manifest of the consolidated integration-test entries.
//! One `mod <entry>;` per former top-level `tests/<entry>.rs` target. Each
//! entry is its own module so per-entry helpers stay in disjoint scopes —
//! do NOT centralise shared helpers here, and keep this list sorted.

mod deep_drop_is_iterative;
mod exact_size_factories;
mod hash_byte_stream_contract;
mod member_visibility_discrimination;
mod member_visibility_json_roundtrip;
mod source_self_anchoring;
mod synthetic_slot_binding_discrimination;
