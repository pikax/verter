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
