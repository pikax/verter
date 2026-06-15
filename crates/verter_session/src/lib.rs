#![allow(clippy::too_many_arguments)]
//! # verter_session â€” In-memory virtual file host for Vue SFC compilation
//!
//! Manages the lifecycle of Vue Single File Components in a stateful,
//! in-memory store. Each `.vue` file (or non-SFC dependency) is parsed,
//! hashed, cached, and compiled on demand. The host is the primary API
//! surface consumed by both the Vite bundler plugin (via `verter_napi`)
//! and the browser playground (via `verter_wasm`).
//!
//! ## Resolution
//!
//! All import resolution goes through `verter_workspace::WorkspaceAccess`. The host
//! does NOT perform any heuristic resolution (no extension guessing, no alias
//! maps, no basename matching). `resolve_via_vfs()` is the sole resolution path.
//!
//! ## Dependencies
//!
//! - **`verter_workspace`** â€” sole authority for file access and import resolution
//! - **`verter_compiler`** â€” SFC tokenizer, parser, and template/script/style codegen
//! - **`verter_semantic::analysis`** â€” static analysis (imports, bindings, macros, style analysis)
//!
//! ## Key types
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`VerterHost`] | Main entry point â€” owns the file store and compile cache |
//! | [`HostConfig`] | Per-host configuration (dev mode, error policy, analysis level) |
//! | [`CompileProfile`] | Per-compilation variant (production, SSR, HMR strategy, etc.) |
//! | [`HostUpdateResult`] | Result of [`VerterHost::upsert`] â€” lists changed/removed virtual nodes |
//! | [`VirtualFileResponse`] | Result of [`VerterHost::get_virtual_file`] â€” compiled code + metadata |
//! | [`ResolvedId`] | Result of [`VerterHost::resolve`] â€” canonical + virtual IDs |
//!
//! ## Caching
//!
//! Each file stores per-profile compile slots keyed by a hash of the
//! [`CompileProfile`]. Slots are invalidated when the file's semantic hash
//! changes, and evicted LRU when the per-file profile cap is exceeded.
//! Smart dependency invalidation (tiered: Tier 1 full, Tier 2 export-level,
//! Tier 3 cross-file type resolution) minimizes unnecessary recompilation.
//!
//! ## Internal modules
//!
//! - [`cache`] â€” virtual node diffing, compile slot invalidation, LRU eviction
//! - [`compile`] â€” external source merging, main module assembly
//! - [`deps`] â€” dependency tracking, tiered smart invalidation
//! - [`hash`] â€” xxh3-based content hashing, profile hashing, semantic hashing
//! - [`id`] â€” canonical ID normalization, virtual ID rendering, import resolution
//! - [`parse`] â€” SFC tokenization â†’ [`ParseSnapshot`](types::ParseSnapshot), non-SFC hashing
//! - [`shared`] â€” feature-gated `RwLock`/`RefCell` abstraction
//! - [`upsert`] â€” change detection, result building, export signature diffing

pub mod app_config_proof_db;
#[cfg(test)]
mod audit_caps_truncation_tests;
#[cfg(test)]
mod audit_warm_cache_tests;
pub mod audited_request;
#[cfg(test)]
mod audited_request_tests;
#[cfg(test)]
mod block_6c_view_hoist_tests;
mod cache;
pub mod cache_schema;
#[cfg(test)]
mod cold_artifact_dedup_tests;
mod compile;
#[cfg(test)]
mod compile_blockers_snapshot_generation_tests;
#[cfg(test)]
mod compile_content_publish_fence_tests;
#[cfg(test)]
mod compile_session_fenced_serve_admission_tests;
#[cfg(test)]
mod compile_style_vbind_source_snapshot_tests;
#[cfg(test)]
mod compile_template_slot_admission_tests;
pub mod component_meta_audit;
#[cfg(test)]
mod component_meta_cache_discipline_tests;
pub mod host_audit_runtime;
#[cfg(test)]
mod host_lifecycle_cascade_tests;
#[cfg(test)]
mod lazy_decl_body_tests;
#[cfg(test)]
mod narrowed_scope_snapshot_generation_tests;
#[cfg(test)]
mod overlay_promotion_isolation_tests;
#[cfg(test)]
mod overlay_template_conversion_isolation_tests;
#[cfg(test)]
mod prepared_decl_import_route_hash_alignment_tests;
#[cfg(test)]
mod raw_snapshot_template_source_move_tests;
#[cfg(test)]
mod request_store_view_derived_hash_tests;
#[cfg(test)]
mod template_slot_generation_rail_tests;
// tests/invalidation_perf.rs — InvalidationByCanonical impl on
// ImportedRegistryDb is exercised by the §12.A12 perf gate.
pub(crate) mod bounded_query_retention;
pub(crate) mod cache_runtime;
pub(crate) mod compile_cache_mode;
pub(crate) mod compile_fact_emission;
pub mod component_meta_caches;
#[cfg(test)]
mod component_meta_caches_tests;
#[cfg(test)]
mod component_meta_canonical_reuse_tests;
#[cfg(test)]
mod component_meta_component_config_fast_path_tests;
#[cfg(test)]
mod component_meta_concurrency_tests;
pub mod component_meta_host;
#[cfg(test)]
mod component_meta_indexed_access_early_out_tests;
#[cfg(test)]
mod component_meta_invalidation_tests;
pub mod component_meta_materialize;
#[cfg(test)]
mod component_meta_no_cache_promotion_tests;
#[cfg(test)]
mod component_meta_owner_local_registry_route_tests;
#[cfg(test)]
mod component_meta_pathological_recursion_tests;
#[cfg(test)]
mod component_meta_pick_omit_tests;
#[cfg(test)]
mod component_meta_publication_demand_tests;
#[cfg(test)]
mod component_meta_read_once_tests;
#[cfg(test)]
mod component_meta_repo_first_pass_diagnosis_tests;
pub mod component_meta_resolution_policy;
#[cfg(test)]
mod component_meta_resolve_owner_route_strip_tests;
pub mod component_meta_result_db;
#[cfg(test)]
mod component_meta_slot_binding_skip_tests;
#[cfg(test)]
mod component_meta_terminal_mode_tests;
#[cfg(test)]
mod component_meta_warm_invalidation_oracle_tests;
pub mod cross_file;
pub(crate) mod decl_body_memo;
pub(crate) mod decl_lowering;
pub mod fact_emission;
// `fact_signature_helpers` is `pub(crate)`: the module's internals are
// implementation detail. The only externally-needed type is
// `ReadSetSignature` — the return type of the public inspector
// `compile_slot_fact_dep_signature` — selectively re-exported below.
pub(crate) mod fact_signature_helpers;
pub use crate::fact_signature_helpers::ReadSetSignature;
#[cfg(test)]
mod error_propagation_lattice_tests;
pub mod file_artifact_store;
mod hash;
pub(crate) mod instant;
pub mod member_display_fact_store;
pub mod member_semantic_fact_store;
pub mod parse_stable_hash;
#[cfg(test)]
mod project_semantic_dispatch_invariants_tests;
pub mod resolved_import_facts;
pub mod resolved_import_facts_producer;
// `host_compile` is the host-backed parallel SFC batch compile module.
// It is bundler/runtime-only and uses Rayon, which is not available on
// WASM, so the module is gated to native targets. WASM continues to use
// single-file `upsert` + `get_virtual_file`.
#[cfg(test)]
mod cache_identity_invariants_tests;
// Content-pinned artifact-read discriminators.
#[cfg(test)]
mod artifact_reads_pinned_tests;
// `SessionView::content_hash_for` is a view-authoritative current-
// content oracle, consistent with `source()` — base + overlay
// fallthrough route through the scheduler authority, never a
// content-agnostic `FileArtifactStore` scan; the overlay materialiser
// derives source + hash from the view itself.
#[cfg(test)]
mod session_view_current_content_tests;
// Byte-identical-overlay artifact-key isolation discriminators —
// a session-view overlay candidate is keyed off the base artifact.
#[cfg(test)]
mod overlay_artifact_key_isolation_tests;
// Current-content-pinned `shallow_file_state` observed-read
// discriminators.
#[cfg(test)]
mod shallow_file_state_pinned_tests;
// Shared substrate for query-identity self-version rooting —
// current-content fact path, self-root signature helpers, and strict
// self-root validation.
#[cfg(test)]
mod query_identity_self_root_substrate_tests;
// Per-cache self-version-root discriminators for the nine
// component-meta query-identity caches.
#[cfg(test)]
mod query_db_self_root_tests;
// Self-version-root discriminators for the `SemanticGraphStore`
// query-node memo: strict warm-read validation, the
// `semantic_graph_read_set_signature` producer, and per-node-kind
// same-canonical-edit rejection.
#[cfg(test)]
mod semantic_graph_self_root_tests;
// Negative import-route reopen discriminators.
/// Selective component-meta surface API types and BFS bridge support
/// (Tier 1B / D102 / D125).
pub mod component_meta_payload;
#[cfg(not(target_arch = "wasm32"))]
pub mod host_analyze_audit;
#[cfg(not(target_arch = "wasm32"))]
pub mod host_audit_bridge;
pub(crate) mod host_batch_coordinator;
mod host_cache_runtime;
pub mod host_compile;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod host_compile_atomic_upsert_tests;
pub mod host_compile_audit;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod host_compile_tests;
mod host_construction;
pub(crate) mod host_executor;
#[cfg(test)]
mod host_executor_lowering_tests;
mod host_lifecycle;
pub mod host_lsp_audit;
pub mod host_manage;
pub mod host_mcp_audit;
mod host_resolve;
pub mod host_resolve_type_audit;
mod host_semantic;
#[cfg(test)]
pub(crate) mod host_test_audit;
mod host_upsert;
mod host_views;
mod host_workspace_audit;
mod id;
pub(crate) mod intrinsic_registry;
pub mod invalidation_domain;
/// Loop-5 inner-dispatch instrumentation counters. Inert in production —
/// callers bump atomic counters at named call sites and dump aggregates
/// via `dump_loop5_instrumentation_counters()`.
pub mod loop5_instrumentation;
pub(crate) mod mapper_binder_registry;
pub mod meta;

pub mod meta_resolve;
#[cfg(test)]
mod negative_import_route_tests;
pub mod owned_artifacts;
pub mod owner_import_surface;
#[cfg(test)]
mod parity_tests;
mod parse;
mod parsed_eval_program;
#[cfg(test)]
mod project_global_cache_tests;
pub(crate) mod project_semantic_dispatch;
pub mod project_type_store;
#[cfg(test)]
mod project_type_store_tests;
mod request_budget;
pub mod request_context;
pub mod resolver_core;
mod resolver_store;
#[cfg(test)]
mod resolver_store_tests;
pub mod semantic_query;
pub(crate) mod semantic_query_memo;
pub(crate) mod session_runtime;
pub mod session_view;
mod shared;
pub(crate) mod source_map_remap;
#[cfg(test)]
mod store_view_manager_tests;
#[cfg(test)]
mod store_view_non_current_contract_tests;
pub(crate) mod template_convert;
/// Test-only re-exports for integration tests in `tests/`.
///
/// NOT a public API — hidden from documentation; production code MUST
/// NOT import from here. The architecture guard
/// `test_only_module_is_only_consumed_by_test_files` (see
/// `tests/architecture_guards.rs`) pins this contract. Body lives in
/// `test_only.rs`.
#[doc(hidden)]
pub mod test_only;
pub mod typeinfo;
mod types;
mod upsert;

// The TS7 oracle harness snapshot GENERATOR entry — `pub` ONLY under the
// `oracle-gen` feature, so the `src/bin/oracle_gen` binary (a separate crate that
// sees only non-test `pub` lib items) can invoke it. NEVER on the default build:
// the default closure has no tsgo (design §3 inv 1).
#[cfg(feature = "oracle-gen")]
pub use crate::typeinfo::oracle_core::gen::{run_oracle_gen, upgrade_snapshots_to_v3, GenError};

// Test harness module — defines the per-request `CaptureToken` API
// consumed by counter assertions across the verter_session test suite.
// Capture-token test/diagnosis instrumentation. Gated `cfg(any(test,
// debug_assertions))` — the same gate as `mod tests` / `mod for_tests`
// below — so it is wholly absent from `cargo build --release`
// (`debug_assertions` is OFF there). Integration tests in
// `crates/verter_session/tests/*.rs` build the lib WITHOUT `cfg(test)`
// but WITH `debug_assertions`, so the `debug_assertions` arm keeps the
// instrumentation reachable for them. Production hooks
// (`with_active_capture(..)`) at the recording sites carry the matching
// `#[cfg(any(test, debug_assertions))]` so release pays zero cost (the
// hook is not even compiled in) instead of the prior one-TLS-lookup.
#[cfg(any(test, debug_assertions))]
pub(crate) mod capture_token;

// Test-support submodules accessible to integration tests under
// `crates/verter_session/tests/*.rs`. Gated `cfg(any(test,
// debug_assertions))` so release builds do not extend the public
// surface (`debug_assertions` is OFF in `cargo build --release`).
// The submodules host reusable harnesses (e.g. the TLS observer
// propagation harness) that integration tests would otherwise need
// to copy-paste; routing them through one named module makes the
// test-only entry points easy to grep and audit.
#[cfg(any(test, debug_assertions))]
pub mod tests;

// `for_tests` re-export shim for integration tests under
// `crates/verter_session/tests/*.rs`. Same `cfg(any(test, debug_assertions))`
// gate as `mod tests`: invisible in release because `debug_assertions` is
// OFF in `cargo build --release`. Body lives in `for_tests.rs`.
#[cfg(any(test, debug_assertions))]
pub mod for_tests;

pub use host_audit_runtime::{
    ActiveRegistration, AuditRequestRegistration, AuditRuntimeSnapshot, HostAuditRuntime,
};
pub use types::*;

// Per-call-site instrumentation accessors. Production-on
// (the counter map is bumped on every `HostStoreView::from_host`
// invocation) so the bench can dump the attribution table at the end
// of each pass. The dump is keyed by `&'static Location` propagated
// through the `#[track_caller]` rail from the warm-hit validator
// down to `HostStoreView::from_host`.
pub use resolver_store::{dump_from_host_call_sites, reset_from_host_call_sites};
// Actual base-view sweep counter — a batch-saturation gate reads this
// (NOT the per-call `from_host` count, which also bumps on cheap
// token-stable Arc-clone hits) to assert a warm batch performs ~O(1)
// full-workspace sweeps.
pub use resolver_store::{
    reset_store_view_coherent_build_sweeps, store_view_coherent_build_sweeps,
};
// The session-overlay copy-on-write counter is deliberately NOT a
// process-global re-export: it lives per-host on
// `VerterHost::provenance().session_overlay_cows` (`types::MetaProvenance`)
// so the batch regression gate measures only its own host's overlay COWs.

// Re-export for the LSP: standalone @verter/types .d.ts content.
pub use verter_compiler::utils::oxc::vue::resolve_type::ResolvedMemberVisibility;
pub use verter_compiler::VERTER_TYPES_STANDALONE_DTS;

// Re-export CompileTarget so downstream crates (LSP, MCP, FFI) can use it
// without adding verter_compiler as a direct dependency.
pub use verter_compiler::compile::CompileTarget;

use std::sync::Arc;

pub use id::resolve_external;
pub(crate) use parsed_eval_program::{ParsedEvalProgram, ParsedTypeResolutionContext};
use rustc_hash::FxHashMap;
#[cfg(test)]
use shared::default_shared;
use shared::Shared;

/// Central file store and compile cache for Vue SFC compilation.
///
/// `VerterHost` owns all tracked files, their parse snapshots, and per-profile
/// compile slots. It is designed to be long-lived (one per Vite dev server or
/// WASM session) and provides the full upsert-resolve-load lifecycle:
///
/// 1. [`upsert`](Self::upsert) â€” parse and store a file, returning change info
/// 2. [`resolve`](Self::resolve) â€” map a raw import ID to canonical + virtual IDs
/// 3. [`get_virtual_file`](Self::get_virtual_file) â€” compile on demand (or cache hit) and return code
///
/// Internal state is protected by `RwLock` for thread-safe concurrent access.
pub struct VerterHost {
    pub(crate) instance_id: u64,
    pub(crate) config: HostConfig,
    /// VFS workspace providing file reads, import resolution, and edge recording.
    /// Wrapped in Arc<RwLock> so the scheduler's SourceLoader can share the same
    /// lock and always read through the latest workspace after `set_workspace()`.
    pub(crate) workspace: Arc<parking_lot::RwLock<Arc<dyn verter_workspace::WorkspaceAccess>>>,
    /// Caller-supplied virtual-alias map populated at upsert time from
    /// [`UpsertRequest`](crate::types::UpsertRequest)`.aliases`. Maps
    /// non-canonical paths (e.g. synthetic IDs from the unplugin or LSP layer)
    /// to canonical IDs for symbol-resolution stability across surfaces.
    /// Disjoint from VFS overlay storage (which keys by canonical) and from
    /// `verter_vfs::ProjectResolver` (which resolves import strings).
    /// Host-scoped authority with no equivalent in `ProjectTypeStore`.
    pub(crate) alias_to_canonical: Shared<FxHashMap<String, String>>,
    pub(crate) tick: std::sync::atomic::AtomicU64,
    /// Coarse semantic mutation epoch used for snapshot-coherent resolver views.
    ///
    /// Unlike `tick`, which tracks compile/access recency, this counter only
    /// advances after host mutations that can change semantic resolution inputs.
    /// It is an INPUT to the `StoreViewValidationToken`, not the validity
    /// oracle by itself (see [`crate::resolver_store::StoreViewValidationToken`]).
    pub(crate) store_view_epoch: std::sync::atomic::AtomicU64,
    /// First-time additive-load generation. Advances on every successful
    /// FIRST-TIME `ensure_loaded` — additive state the base view snapshots
    /// by value but that publishes into neither `FileArtifactStore` nor the
    /// epoch. A dedicated dimension (not a `store_view_epoch` bump) because
    /// the epoch bump clears the thread-local parsed-eval-program cache,
    /// and the publish fence must EXCLUDE a compute's own loads while the
    /// manager REUSE oracle includes them — full rationale on
    /// [`crate::resolver_store::StoreViewValidationToken::load_generation`].
    pub(crate) load_generation: std::sync::atomic::AtomicU64,
    /// Caches one Arc-shareable base `HostStoreView` keyed by the
    /// complete `StoreViewValidationToken`, so batch jobs reuse one
    /// workspace snapshot by cheap Arc clone instead of re-sweeping the
    /// workspace per job. See [`crate::resolver_store::StoreViewManager`].
    pub(crate) store_view_manager: crate::resolver_store::StoreViewManager,
    /// Last computed cross-file prop constness overrides for
    /// invalidation tracking. Stores the LAST computed values for
    /// diff-detection on re-computation. **NOT a cache** of resolution
    /// results — a state-diff record. Host-scoped authority with no
    /// equivalent in `ProjectTypeStore`.
    pub(crate) last_const_prop_overrides:
        Shared<rustc_hash::FxHashMap<String, rustc_hash::FxHashSet<String>>>,
    #[cfg(feature = "session_metrics")]
    pub(crate) metrics: HostMetrics,
    /// Scheduler for async per-file staging.
    ///
    /// The scheduler coordinates Sourceâ†’Analysisâ†’Artifact progression
    /// with generation tracking, priority queuing, and blocker management.
    /// It is the sole parser â€” upsert() delegates to the scheduler.
    pub(crate) scheduler: Arc<verter_scheduler::scheduler::Scheduler>,
    /// Provenance counters for component-meta observability.
    /// Shared with sessions via `Arc`.
    pub(crate) provenance: Arc<MetaProvenance>,
    /// Consolidated resolver state: sub-node caches (symbol + fallthrough),
    /// top-level host caches (meta + fallthrough), and singleflight groups.
    pub(crate) resolver: host_construction::HostResolverState,
    /// Active per-host query profile — execution-policy decisions
    /// (prewarming, budgets, allowed query families). **Not a cache** — does
    /// not memoise query results. Different artifact type than anything in
    /// `ProjectTypeStore`.
    pub(crate) query_profile: parking_lot::Mutex<verter_semantic::profile::QueryProfile>,
    /// Project-global type-resolution cache root. Owns `IndexedReady`,
    /// `AnalysisReady`, and the rehomed
    /// `RouteDb` / `ImportedRootDb`. See `project_type_store` module docs.
    pub(crate) project_type_store: Arc<crate::project_type_store::ProjectTypeStore>,
    /// Monotonic request-id generator for component-meta requests.
    /// Zero is reserved for "not populated"; the counter
    /// starts at 0 and `next_request_id()` returns pre-increment + 1.
    pub(crate) request_id_counter: std::sync::atomic::AtomicU64,
    /// Bounded insert-ordered store of finished audit records.
    ///
    /// Backing shape:
    /// `Mutex<IndexMap<u64, RequestAuditRecord>>` with capacity 256 and
    /// **FIFO eviction** via `shift_remove_index(0)` at capacity (verified
    /// at `audit_records_store.rs:23–26, 49–56`). Different artifact type
    /// than anything in `ProjectTypeStore`; the audit subsystem has its own
    /// per-request lifecycle. Per-request inserts happen in
    /// `emit_audit_trace`; consumers retrieve via
    /// `take_audit_record(request_id)`.
    pub(crate) audit_records: Arc<crate::component_meta_audit::AuditRecordsStore>,
    /// Host-owned audit runtime — wraps [`crate::component_meta_audit::AuditRecordsStore`],
    /// the [`verter_audit::AuditConfig`] snapshot, and the active-request
    /// registry that [`crate::host_audit_runtime::AuditRequestRegistration`]
    /// populates. The records store and this runtime share the same
    /// `Arc<AuditRecordsStore>`, so writes through either surface land in
    /// the same map.
    pub(crate) host_audit_runtime: Arc<crate::host_audit_runtime::HostAuditRuntime>,
    /// Cumulative host-level test audit state — accessible via
    /// [`Self::audit`] (test-only). Counters increment from
    /// `#[cfg(test)]` hooks at the production read / shallow-process
    /// sites; the lowering count is read from the graph store's
    /// existing `stats_snapshot`.
    #[cfg(test)]
    pub(crate) test_audit: Arc<crate::host_test_audit::HostTestAuditState>,
    /// Test-only observable: records the most recent priority
    /// passed to [`VerterHost::upsert_with_priority`]. Read by
    /// `compile_many_propagates_interactive_priority` and
    /// `compile_many_priority_default_is_background` to confirm that
    /// `compile_many` propagates the caller-configured priority into
    /// the scheduler submit site. **Compiled out in production builds.**
    #[cfg(test)]
    pub(crate) last_upsert_priority: parking_lot::Mutex<Option<verter_scheduler::stage::Priority>>,
    /// Test-only observable: incremented at the very top of
    /// `host_compile::compile_one_in_batch` (BEFORE the precomputed-error
    /// short-circuit so every invocation is counted). Read by
    /// `compile_many_compiles_each_canonical_once` to discriminate the
    /// "compile each unique canonical group exactly once" invariant.
    /// **Compiled out in production builds.**
    #[cfg(test)]
    pub(crate) compile_one_call_count: std::sync::atomic::AtomicUsize,
    /// Test-only observable: records the `CallerKind` reported by
    /// `CallerKind::current()` on each `compile_one_in_batch` worker.
    /// Stored as a `u8` tag so the field is lock-free and the discrete
    /// caller-kind discriminator is exposed without a `CallerKind`
    /// import in non-test code. Encoding: `0 = unobserved`, `1 = External`,
    /// `2 = Driver`, `3 = CpuWorker`, `4 = IoWorker`, `5 = Inline`.
    /// Read by `compile_many_workers_carry_host_cpu_pool_id`
    /// (secondary caller-kind canary, alongside the primary
    /// pool-id-token assertion read from
    /// `compile_one_host_cpu_pool_token`) to confirm the dual-pool
    /// isolation invariant: workers running `compile_one_in_batch`
    /// MUST report `External` (host pool) rather than `CpuWorker`
    /// (scheduler pool). **Compiled out in production builds.**
    #[cfg(test)]
    pub(crate) compile_one_caller_kind_tag: std::sync::atomic::AtomicU8,
    /// Test-only observable: records the host-CPU-pool identity token
    /// observed on the worker that ran `compile_one_in_batch`. Encoding:
    /// `usize::MAX` is the "unobserved / not on a host pool" sentinel
    /// (so the field stays lock-free and avoids `AtomicOption`);
    /// any other value is the worker's
    /// `verter_scheduler::host_cpu_pool_token()` reading, expected to
    /// equal `self.host_cpu_pool().pool_id()` on a properly host-owned
    /// `compile_many`. A regressed per-call Rayon pool (no
    /// `start_handler` installs the token) would report the sentinel
    /// even though `CallerKind::current() == External`. Read by
    /// `compile_many_workers_carry_host_cpu_pool_id`. **Compiled out
    /// in production builds.**
    #[cfg(test)]
    pub(crate) compile_one_host_cpu_pool_token: std::sync::atomic::AtomicUsize,
    /// Test-only seam fired inside an `IndexedReady` materialise flight
    /// (base materialise, edge refresh, and the overlay materialiser)
    /// AFTER the flight's generation stamps are captured and BEFORE the
    /// build + pre-publish fence. Concurrency tests install a barrier
    /// here to deterministically land a workspace / route mutation in
    /// the fence window and to admit singleflight followers while the
    /// leader is parked. **Compiled out in production builds.**
    #[cfg(test)]
    pub(crate) materialize_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// Test-only seam fired inside `ensure_indexed_ready_serve`'s bounded
    /// singleflight retry loop AFTER a follower records a fenced
    /// (non-adoptable) outcome and BEFORE its next attempt. Sustained-churn
    /// tests park here to interleave a fresh leader + mutation per
    /// attempt, driving the loop to its bounded ReturnOnly fallback
    /// deterministically. **Compiled out in production builds.**
    #[cfg(test)]
    pub(crate) flight_retry_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// Test-only seam fired inside `get_virtual_file`'s cold compile
    /// path AFTER the compile completed and BEFORE the mode-routed
    /// publish. Fence tests install an env / project mutation here to
    /// land deterministically in the compute→publish window and assert
    /// the publish declines (ReturnOnly) instead of stamping the
    /// old-input output under the moved identity. **Compiled out in
    /// production builds.**
    #[cfg(test)]
    pub(crate) compile_publish_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// Test-only seam fired inside `get_virtual_file`'s cold compile
    /// path AFTER the request's scheduler source snapshot is captured
    /// and BEFORE the compile input is assembled. Fence tests install
    /// a content upsert here to land deterministically in the
    /// snapshot→compile-input window and assert the content-addressed
    /// publish never stamps bytes under a content hash the compiled
    /// input did not have. **Compiled out in production builds.**
    #[cfg(test)]
    pub(crate) compile_input_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// Test-only seam fired inside `ensure_indexed_ready_serve`'s
    /// singleflight body AFTER the edge-refresh parse-env reuse gate
    /// passes and BEFORE the refresh flight runs. Fence tests install a
    /// parse-env-moving mutation here ([`Self::parse_env_override`] flip
    /// plus a `project_generation` bump) to land deterministically in
    /// the reuse-gate→publish window and assert the refresh publish
    /// declines (ReturnOnly) instead of stamping a current
    /// `project_generation` onto a payload parsed under the superseded
    /// env. **Compiled out in production builds.**
    #[cfg(test)]
    pub(crate) edge_refresh_gate_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// Test-only seam fired inside the raw-analysis-snapshot scheduler
    /// lane AFTER the lane's analysis snapshot is captured and BEFORE
    /// the template-analysis source join. Fence tests install a content
    /// upsert here to land deterministically in the capture→join window
    /// and assert a template derived from the moved bytes is never
    /// persisted into the rail-less `derived_raw_cache` slot.
    /// **Compiled out in production builds.**
    #[cfg(test)]
    pub(crate) raw_snapshot_template_join_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// Test-only seam fired inside the lazy template-analysis
    /// computation AFTER the by-value inputs produced the template and
    /// BEFORE the `derived_raw_cache` persist. Fence tests install a
    /// content upsert here to land deterministically in the
    /// compute→persist window and assert a coherently-captured but
    /// since-superseded template never serves as current after the
    /// racing upsert cleared the slot. **Compiled out in production
    /// builds.**
    #[cfg(test)]
    pub(crate) template_persist_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// Test-only seam fired inside `get_analysis_snapshot_internal`'s
    /// narrowed-scope serve branch AFTER the branch's source snapshot
    /// is captured and BEFORE the snapshot's products are assembled.
    /// Fence tests install a content upsert here to land
    /// deterministically in the capture→assembly window and assert the
    /// served snapshot stays single-generation — every product derives
    /// from the held source snapshot, never from an independent later
    /// read. **Compiled out in production builds.**
    #[cfg(test)]
    pub(crate) narrowed_scope_serve_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// Test-only seam fired inside `get_compile_blockers` AFTER the
    /// source snapshot is captured and BEFORE the snapshot's products
    /// are assembled. Fence tests install a content upsert here to
    /// land deterministically in the capture→assembly window and
    /// assert the served `CompileBlockersSnapshot` stays
    /// single-generation — every product derives from the held source
    /// snapshot, never from an independent later read. **Compiled out
    /// in production builds.**
    #[cfg(test)]
    pub(crate) compile_blockers_serve_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// Test-only override of the live parse-env dimension returned by
    /// `host_view_env_hashes` / `host_view_env_hashes_for`. The
    /// production parse dimension derives solely from the constant
    /// workspace parser flags today, so fence tests flip this override
    /// (always paired with a `project_generation` bump — every
    /// parse-env-moving mutation bumps `project_generation`) to emulate
    /// a parse-env-moving configuration change mid-flight. **Compiled
    /// out in production builds.**
    #[cfg(test)]
    pub(crate) parse_env_override: parking_lot::Mutex<Option<crate::types::Hash16>>,
    /// Host-owned LRU cache for the typeinfo `evaluate_type_expression`
    /// scratch URIs. See `typeinfo::scratch_cache` for the LRU policy
    /// and §5.3 of the typeinfo plan for the deterministic-URI
    /// derivation. Capacity defaults to
    /// `typeinfo::scratch_cache::DEFAULT_CAPACITY` (64).
    pub(crate) typeinfo_scratch_cache:
        parking_lot::Mutex<crate::typeinfo::scratch_cache::ScratchCache>,
    /// Host-owned cache of `.vue` SFC macro-surface normalized DTOs
    /// (the typeinfo Vue adapter's shallow-metadata home). Materialized
    /// once per `(canonical, content, macro, level)` per the Shallow File
    /// Processing Core Invariant. See
    /// `typeinfo::adapters::vue::store::VueShallowMetadataStore`.
    pub(crate) vue_shallow_metadata_store:
        crate::typeinfo::adapters::vue::store::VueShallowMetadataStore,
    /// Host-owned CPU pool for every host batch API's outer coordinator
    /// — both `compile_many` and the component-meta batch fan their
    /// outer wait out on it through the host batch coordinator. Distinct
    /// from the scheduler's own CPU pool — workers register as
    /// [`verter_scheduler::caller_kind::CallerKind::External`] so
    /// `wait_or_drive` parks instead of inline-executing scheduler CPU
    /// tasks. Built once at host construction and reused across every
    /// batch call (a regressed per-call rebuild would bump
    /// [`verter_scheduler::HostCpuPool::build_count`] on every batch).
    ///
    /// Not present on `wasm32` — `compile_many` is gated behind
    /// `#[cfg(not(target_arch = "wasm32"))]` and the host-pool field
    /// is gated alongside it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) host_cpu_pool: Arc<verter_scheduler::HostCpuPool>,
    /// Scheduler-side lazy declaration-lowering service: dedicated
    /// worker threads own the retained (!Send) eval-program parses per
    /// content generation and run pure per-declaration lowering jobs
    /// for the declaration-body memos. Built once at host construction;
    /// shared by every `IndexedReady` artifact's memo. Retention is
    /// LEASE-PINNED — a memo holds the snapshot for its content
    /// generation, so a live artifact reuses one parse instead of
    /// re-parsing per body demand. On `wasm32` there are no worker
    /// threads and the (`Rc`-backed, `!Send`) parse cannot be a service
    /// field, so the retained snapshot lives in a single-thread
    /// thread-local shard (`WASM_DECL_LOWERING_SHARD`) the job runs
    /// inline against — still lease-pinned, NOT a re-parse per demand.
    pub(crate) decl_lowering: Arc<crate::decl_lowering::DeclLoweringService>,
    /// Per-host test-injection knob for the compile-tier cold-build
    /// path. When set to `N > 0`, the `Session` cold-compute closure
    /// observes `N` synthetic `FileWholeHash` facts via `observe_fan_out`
    /// after the normal compile-tier observation step, deterministically
    /// forcing the installed fact tracer to either overflow (when `N >
    /// FACT_SIGNATURE_CAP`) or accumulate a large signature. Drives the
    /// refuse-publish-on-overflow tests without a pathological workspace
    /// fixture.
    ///
    /// Armed/cleared by the host-scoped RAII guard
    /// [`crate::host_resolve::CompileForceOverflowGuard`]. Per-host
    /// (not process-global) so a test arming it on one host never
    /// poisons concurrent compiles on a different host running on
    /// another test thread. Production reads it once per `Session` cold
    /// compute as a relaxed atomic load (~1 ns) on a path that already
    /// takes locks, so the cost is in the noise.
    pub(crate) compile_force_overflow_observations: std::sync::atomic::AtomicUsize,
    /// Per-host test-injection knob for the materialiser's cold-compute
    /// path — the structural-materialise analogue of
    /// [`Self::compile_force_overflow_observations`]. When set to `N >
    /// 0`, the cold-compute closure observes `N` synthetic
    /// `FileWholeHash` facts onto the active tracer, forcing the
    /// `materialize_structure_overflow_refusals` admission-refusal path.
    /// Armed/cleared by
    /// [`crate::for_tests::MaterializeForceOverflowGuard`].
    pub(crate) materialize_force_overflow_observations: std::sync::atomic::AtomicUsize,
    /// Per-host test-injection knob that forces a GENUINE in-scope
    /// PARTIAL inside the materialiser's cold compute. When non-zero,
    /// the cold-compute closure folds a partial into the active
    /// [`crate::request_context::ColdComputeCompletenessScope`] via the
    /// EXACT production rail a budget-tripped child read uses
    /// ([`crate::request_context::mark_request_materialization_cache_suppress`]),
    /// so the per-cold-compute completeness goes `Partial` and the
    /// `MaterializeStructureDb` admission gate
    /// (`refuse_result_cache_admission_if_partial`) must refuse the
    /// entry. This is NOT a side channel: it drives the same fold a
    /// real budget trip drives, mirroring production. Armed/cleared by
    /// [`crate::for_tests::MaterializeForceInScopePartialGuard`].
    pub(crate) materialize_force_in_scope_partial: std::sync::atomic::AtomicBool,
    /// Per-host test-injection knob modelling a project-shape mutation
    /// landing INSIDE the materialiser's cold window: when armed, the
    /// next `materialize_component_meta_structure` cold compute bumps
    /// the project generation once (a REAL bump through
    /// `ProjectTypeStore::bump_project_generation`), so the runtime's
    /// post-compute revalidation gate rejects the freshly-built entry —
    /// the exact production admission-refusal path, with a
    /// deterministic trigger. Self-disarms after one fire.
    pub(crate) materialize_force_mid_compute_generation_bump: std::sync::atomic::AtomicBool,
    /// Per-host test-injection knob for the relation engine's cold
    /// judgement path — the relation-memo analogue of
    /// [`Self::materialize_force_overflow_observations`]. When set to `N >
    /// 0`, the cold relation compute observes `N` synthetic `FileWholeHash`
    /// facts onto the active tracer, forcing the relation memo's
    /// `FactReadSetFinalise::Overflow` non-admission path (the judgement is
    /// returned to the caller but refused memo admission). Set directly in
    /// the inline relation tests.
    pub(crate) relation_force_overflow_observations: std::sync::atomic::AtomicUsize,
    /// Per-host invocation counter for
    /// [`VerterHost::prefetch_compile_tier_observation_targets`].
    /// Incremented once per actual call to the prefetch. The cold-compute
    /// path installs the prefetch ONLY for the `Session` cache mode, so a
    /// routing test resets this, runs one cold compute per requested
    /// mode, and asserts the counter stays `0` for `Content` /
    /// `Stateless` and increments for `Session`. Per-host so a `Session`
    /// compute on one host never increments the counter another host's
    /// routing test reads.
    pub(crate) compile_tier_prefetch_invocations: std::sync::atomic::AtomicUsize,
    /// Per-host counter for `FactReadSetFinalise::Overflow` hits at the
    /// `install_fact_tracer` boundary. Monotonically increasing for the
    /// host's lifetime; reset only when the host is dropped. Readable
    /// from tests via
    /// [`crate::fact_signature_helpers::read_signature_overflow_at_install`].
    /// Per-host so an overflow forced on one host's tracer never bumps
    /// the counter a different host's delta assertion reads.
    pub(crate) signature_overflow_at_install: std::sync::atomic::AtomicU64,
}

// Manual Debug impl because Arc<dyn WorkspaceAccess> doesn't implement Debug.
impl std::fmt::Debug for VerterHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerterHost")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;
