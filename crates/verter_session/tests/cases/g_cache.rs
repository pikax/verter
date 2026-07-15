//! Consolidated integration-test group `cache`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_cache/cache_invariant_migration.rs"]
mod cache_invariant_migration;
#[path = "g_cache/cache_key_invariants.rs"]
mod cache_key_invariants;
#[path = "g_cache/cache_layer_cold_concurrent_attribution.rs"]
mod cache_layer_cold_concurrent_attribution;
#[path = "g_cache/cache_layer_concurrent_attribution.rs"]
mod cache_layer_concurrent_attribution;
#[path = "g_cache/cache_layer_per_request_attribution.rs"]
mod cache_layer_per_request_attribution;
#[path = "g_cache/cache_layer_regression_per_layer.rs"]
mod cache_layer_regression_per_layer;
#[path = "g_cache/cache_reuse_invariants.rs"]
mod cache_reuse_invariants;
#[path = "g_cache/r6_r21_query_identity_keys.rs"]
mod r6_r21_query_identity_keys;
#[path = "g_cache/read_set_signature_carrier.rs"]
mod read_set_signature_carrier;
