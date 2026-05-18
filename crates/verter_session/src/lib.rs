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
mod audit_warm_cache_tests;
pub mod audited_request;
#[cfg(test)]
mod audited_request_tests;
mod cache;
pub mod cache_schema;
mod compile;
pub(crate) mod completion_fence;
pub mod component_meta_audit;
#[cfg(test)]
mod component_meta_cache_discipline_tests;
pub mod host_audit_runtime;
// tests/invalidation_perf.rs — InvalidationByCanonical impl on
// ImportedRegistryDb is exercised by the §12.A12 perf gate.
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
#[cfg(test)]
mod component_meta_field_reduction_lazy_probe_tests;
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
mod component_meta_read_once_tests;
#[cfg(test)]
mod component_meta_repo_first_pass_diagnosis_tests;
pub mod component_meta_resolution_policy;
pub mod component_meta_result_db;
#[cfg(test)]
mod component_meta_slot_binding_skip_tests;
#[cfg(test)]
mod component_meta_terminal_mode_tests;
pub mod cooperative_admission;
pub mod cross_file;
mod deps;
pub mod fact_emission;
pub(crate) mod fact_signature_helpers;
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
// Block 1.J.1 item 1 — content-pinned artifact-read discriminators.
#[cfg(test)]
mod artifact_reads_pinned_tests;
// Current-content-pinned `shallow_file_state` observed-read
// discriminators.
#[cfg(test)]
mod shallow_file_state_pinned_tests;
// Shared substrate for query-identity self-version rooting —
// current-content fact path, self-root signature helpers, strict
// self-root validation, and the skip-own-drain test hook.
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
// Block 1.J.1 item 2 — negative import-route reopen discriminators.
/// Selective component-meta surface API types and BFS bridge support
/// (Tier 1B / D102 / D125).
pub mod component_meta_payload;
#[cfg(not(target_arch = "wasm32"))]
pub mod host_analyze_audit;
#[cfg(not(target_arch = "wasm32"))]
pub mod host_audit_bridge;
pub mod host_compile;
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
#[cfg(test)]
mod host_test_seed;
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
pub mod meta;
pub mod meta_resolve;
#[cfg(test)]
mod negative_import_route_tests;
pub mod owned_artifacts;
pub mod owner_import_surface;
#[cfg(test)]
mod parity_tests;
mod parse;
#[cfg(test)]
mod project_global_cache_tests;
pub(crate) mod project_semantic_dispatch;
pub mod project_type_store;
#[cfg(test)]
mod project_type_store_tests;
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
pub(crate) mod spike_instrumentation;
pub(crate) mod template_convert;
pub mod typeinfo;
mod types;
mod upsert;

// Test harness module — defines the per-request `CaptureToken` API
// consumed by counter assertions across the verter_session test suite.
// The module is NOT `cfg(test)`-gated because integration tests in
// `crates/verter_session/tests/*.rs` build the lib WITHOUT `cfg(test)`
// set; the production-cost discipline is enforced via `pub(crate)` on
// the module itself plus the empty-thread-local fast path inside
// `with_active_capture` (no token bound → immediate return, no lock
// acquisition, no allocation).
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

// `for_tests` re-export shim for integration tests in
// `crates/verter_session/tests/*.rs`. Integration tests build the lib
// WITHOUT `cfg(test)` set, so a `cfg(test)`-gated `pub mod for_tests`
// would be invisible to them. The module is therefore gated by
// `cfg(any(test, debug_assertions))` — release builds do not extend
// the public surface because debug_assertions is OFF in release.
//
// The harness module itself stays `pub(crate)` so the production
// public crate surface is not extended; this re-export is a thin shim
// that routes test-only access through a name `for_tests` that callers
// can grep for.
#[cfg(any(test, debug_assertions))]
pub mod for_tests {
    //! Re-export shim for integration tests in
    //! `crates/verter_session/tests/*.rs` — those build as a separate
    //! crate target and cannot reach `pub(crate)` items directly.
    //!
    //! All re-exports are gated `cfg(any(test, debug_assertions))` so
    //! release builds do not extend the public surface — `debug_assertions`
    //! is OFF in `cargo build --release`, so the module is absent from
    //! release artifacts consumed downstream.
    pub use crate::capture_token::{
        assert_no_stack_overflow, with_active_capture, with_active_capture_returning, CacheId,
        CacheKeyFilter, CacheProvenance, CanonicalId, CaptureGuard, CaptureSnapshot, CaptureToken,
        DispatchEntry, EdgeIdentity, InternedId, KeyFamily, SignatureHash, StackOverflow,
    };
    /// Tier 1B: re-export the cooperative-batch primitive so the
    /// selective component-meta integration tests can probe its
    /// existence. Internal callers continue to reach
    /// `crate::semantic_query_memo::SemanticGraphStore` directly.
    pub use crate::semantic_query_memo::{BatchExpandError, SemanticGraphStore};

    /// Integration tests that drive the counter-helper dual-target
    /// write (`record_inflight_aborted_retry` /
    /// `record_cold_abort_swept`) need a `DepSignature` constructor
    /// to feed `execute_cooperative` and a guard struct to force the
    /// cold-abort branch. Both surfaces are `#[doc(hidden)]` and
    /// gated through this `for_tests` module so production callers
    /// never reach them.
    pub use crate::semantic_query_memo::{
        empty_signature_for_tests, test_trigger_inflight_abort, TestForceColdAbortGuard,
    };

    /// Carrier type for cache-entry dependency signatures. Integration
    /// tests construct `ReadSetSignature` directly when seeding
    /// fixtures into `ComponentMetaResultEntry` / `MaterializeStructureEntry` /
    /// `OwnerImportSurface` / `RefCycleEntry` / `MemoEntry`.
    pub use crate::fact_signature_helpers::ReadSetSignature;

    /// Constructs `ComputeAdmission::Failed` for the
    /// `compute_admission_failed_variant_is_constructible` discriminator
    /// in `tests/block_1_i_discriminators.rs`. The Failed variant is
    /// part of the codex three-variant contract for
    /// `cooperative_admit_with_post_publish`; this helper proves it is
    /// constructible so the variant cannot be silently dropped.
    pub fn cooperative_admission_failed_variant_for_tests(
    ) -> crate::cooperative_admission::ComputeAdmission<(), ()> {
        crate::cooperative_admission::ComputeAdmission::Failed
    }

    /// Fan `sig` into every active tracer on the current thread's TLS
    /// stack. Integration tests use this to verify that the multi-level
    /// fan-out delivers observations into all nested tracer scopes without
    /// going through `FactReadSetCell::observe` directly (which only writes
    /// to the cell it's called on, not the full stack).
    pub fn observe_fan_out_borrowed_for_tests(sig: &[crate::resolver_core::FactVersionRef]) {
        crate::resolver_core::resolver_context::observe_fan_out_borrowed(sig);
    }

    /// Bracket one cold-compute closure with a push-style fact tracer.
    ///
    /// Thin re-export of [`crate::fact_signature_helpers::install_fact_tracer`]
    /// for integration tests that need to verify tracer finalisation,
    /// overflow telemetry, and the returned `FactReadSetFinalise` variant.
    pub fn install_fact_tracer_for_tests<F, R>(
        host: &crate::VerterHost,
        f: F,
    ) -> (R, crate::resolver_core::FactReadSetFinalise)
    where
        F: FnOnce() -> R,
    {
        crate::fact_signature_helpers::install_fact_tracer(host, f)
    }

    /// Convert a legacy `DepSignature` into a `Vec<FactVersionRef>`.
    ///
    /// Re-export for integration tests verifying the bridge conversion.
    pub fn dep_signature_to_fact_signature_for_tests(
        sig: &crate::semantic_query::DepSignature,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        crate::fact_signature_helpers::dep_signature_to_fact_signature(sig)
    }

    /// Read the current overflow-at-install counter value.
    ///
    /// Integration tests use this to verify that `FactSignatureOverflow`
    /// telemetry fires and the counter increments on overflow.
    pub fn read_signature_overflow_at_install() -> u64 {
        crate::fact_signature_helpers::read_signature_overflow_at_install()
    }

    /// Arm the materialiser's test-only fact-injection knob with `n`
    /// synthetic `FileWholeHash` observations per cold-compute call.
    /// When `n > FACT_SIGNATURE_CAP` (1024), the cold compute's
    /// installed fact tracer finalises with `Overflow`, exercising the
    /// `materialize_structure_overflow_refusals` admission-refusal path
    /// without requiring a workspace fixture that organically emits
    /// thousands of facts. The returned guard zeroes the knob on drop.
    ///
    /// Integration tests in
    /// `crates/verter_session/tests/family_bcd_*_overflow*.rs` use this
    /// to discriminate the
    /// `MaterializeOutcome::Value` vs `Tainted` distinction on
    /// admission refusal.
    pub fn materialize_force_overflow_observations_for_tests(
        n: usize,
    ) -> crate::component_meta_materialize::MaterializeForceOverflowGuard {
        crate::component_meta_materialize::MaterializeForceOverflowGuard::arm(n)
    }

    /// Read the materialiser's `materialize_structure_overflow_refusals`
    /// counter for the given host. Test surface; production callers reach
    /// this through the `host_audit_runtime().snapshot()` provenance
    /// surface.
    pub fn read_materialize_structure_overflow_refusals(host: &crate::VerterHost) -> u64 {
        host.provenance
            .materialize_structure_overflow_refusals
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Arm the dispatch's test-only Parse-fact injection slot. The
    /// next cold build through `ProjectSemanticDispatch::execute()`
    /// observes the supplied `Parse(...)` fact onto every active
    /// tracer BEFORE the inner build runs. The returned guard clears
    /// the slot on drop. Integration tests in
    /// `crates/verter_session/tests/dispatch_*_fact_*.rs` use this to
    /// discriminate the cold-publish → warm-hit path-precise survival
    /// contract.
    pub fn dispatch_inject_parse_fact_for_tests(
        fact: crate::resolver_core::FactVersionRef,
    ) -> crate::project_semantic_dispatch::DispatchInjectParseFactGuard {
        crate::project_semantic_dispatch::DispatchInjectParseFactGuard::arm(fact)
    }

    /// Drive [`crate::project_semantic_dispatch::ProjectSemanticDispatch::execute`]
    /// from integration tests. Constructs a `ProjectSemanticDispatch`
    /// from the `host` (the standard internal pattern) and forwards
    /// the call. Used by tests that need to exercise the dispatch's
    /// `install_fact_tracer` wrapper directly — the dispatch is
    /// `pub(crate)` so integration test crates cannot reach it
    /// otherwise.
    pub fn dispatch_execute_for_tests(
        host: &crate::VerterHost,
        key: crate::semantic_query::SemanticQueryKey,
    ) -> crate::semantic_query::QueryResult<crate::semantic_query::SemanticNodeId> {
        use crate::semantic_query::SemanticQueryApi;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
        dispatch.execute(key)
    }

    /// Returns `true` iff `host.active_session_view()` returns `None`.
    ///
    /// This shim is needed because `ResolverContext` is sealed — integration
    /// tests cannot call the trait method directly. The shim calls through the
    /// sealed trait on the concrete `VerterHost` impl.
    pub fn active_session_view_is_none_for_tests(host: &crate::VerterHost) -> bool {
        use crate::resolver_core::ResolverContext;
        host.active_session_view().is_none()
    }

    /// Drive the AppConfigNoOverrideProofDb production producer
    /// from integration tests. The producer takes a `&dyn ResolverContext`
    /// internally (per the seal contract); this wrapper accepts a
    /// concrete `&VerterHost` reference so integration tests in
    /// `tests/family_bcd_*.rs` can drive it end-to-end.
    pub fn app_config_no_override_proof_get_or_compute_for_tests(
        host: &crate::VerterHost,
        key: &crate::app_config_proof_db::AppConfigNoOverrideProofKey,
    ) -> Option<std::sync::Arc<crate::app_config_proof_db::AppConfigNoOverrideProofEntry>> {
        crate::component_meta_caches::app_config_no_override_proof_get_or_compute(host, key)
    }

    /// Return the published `ComponentMetaResultEntry`'s
    /// `ReadSetSignature` for `owner_canonical`, or `None` when no
    /// entry is cached for the owner's current content.
    ///
    /// The lookup key is composed exactly as
    /// `publish_component_meta_cache_entry` composes it — owner
    /// canonical + current `IndexedReady` whole-hash + the default
    /// `ComponentMetaOptions` fingerprint — so it matches the
    /// published entry. Integration tests in
    /// `tests/component_meta_signature_is_tracer_owned.rs` and
    /// `tests/component_meta_family_producers_observe_cross_file_deps.rs`
    /// use it to inspect the tracer-owned `facts` rail of the
    /// published signature.
    pub fn component_meta_result_signature_for_owner(
        host: &crate::VerterHost,
        owner_canonical: &str,
    ) -> Option<crate::fact_signature_helpers::ReadSetSignature> {
        let whole_hash = host
            .ensure_indexed_ready(owner_canonical)
            .map(|ir| ir.whole_hash)?;
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: std::sync::Arc::from(owner_canonical),
            owner_whole_hash: whole_hash,
            options_fingerprint: crate::host_manage::component_meta_options_fingerprint(
                &crate::host_manage::ComponentMetaOptions::default(),
            ),
        };
        host.project_type_store()
            .component_meta_results()
            .get(&key)
            .map(|entry| entry.read_set_signature.clone())
    }

    /// Compute the FINALIZED fact-tracer read set for a cold
    /// `get_component_meta` of `owner_canonical`. Installs a fresh
    /// `with_fact_tracer` scope around `resolve_component_meta` +
    /// `extract_component_meta_from_resolved` (the exact body the
    /// production `get_component_meta` cold path traces) and returns
    /// `read_set.finalise()`.
    ///
    /// Returns `None` when the owner does not resolve to a component.
    /// Integration tests use this to assert the published
    /// `ComponentMetaResultEntry` signature EQUALS the finalized
    /// tracer read set (codex item 3 — tracer-owned signature).
    pub fn component_meta_cold_traced_read_set_for_tests(
        host: &crate::VerterHost,
        owner_canonical: &str,
    ) -> Option<crate::resolver_core::FactReadSetFinalise> {
        let canonical = host.resolve_alias_or_canonical(owner_canonical);
        let (resolved_opt, read_set) = host.with_fact_tracer(|| {
            host.resolve_component_meta(canonical.as_str(), crate::types::ProjectionMode::Expanded)
                .map(|resolved| {
                    let _ = crate::host_manage::extract_component_meta_from_resolved(
                        host,
                        canonical.as_str(),
                        &resolved,
                        true,
                    );
                })
        });
        resolved_opt?;
        Some(read_set.finalise())
    }

    /// Discriminating probes for the dispatch DepSignature→fact
    /// bridges (`accumulate_dispatch_dep_signature` /
    /// `observe_fence_entry`). Used by
    /// `tests/dispatch_bridges_convert_project_generation.rs`.
    pub use crate::tests::dispatch_bridges::{
        accumulate_dispatch_dep_signature_for_tests, observe_fence_entry_for_tests,
    };
}

pub use host_audit_runtime::{
    ActiveRegistration, AuditRequestRegistration, AuditRuntimeSnapshot, HostAuditRuntime,
};
pub use types::*;

// Re-export for the LSP: standalone @verter/types .d.ts content.
pub use verter_compiler::utils::oxc::vue::resolve_type::ResolvedMemberVisibility;
pub use verter_compiler::VERTER_TYPES_STANDALONE_DTS;

// Re-export CompileTarget so downstream crates (LSP, MCP, FFI) can use it
// without adding verter_compiler as a direct dependency.
pub use verter_compiler::compile::CompileTarget;

use std::rc::Rc;
use std::sync::Arc;

pub use id::resolve_external;
use rustc_hash::FxHashMap;
#[cfg(test)]
use shared::default_shared;
use shared::Shared;

type CachedEvalProgramAst<'a> = oxc_ast::ast::Program<'a>;
type CachedTypeResolutionContext<'a> =
    verter_compiler::utils::oxc::vue::resolve_type::TypeResolutionContext<'a, 'a>;

struct ParsedEvalProgramOwner {
    allocator: oxc_allocator::Allocator,
    source: Arc<str>,
    source_type: oxc_span::SourceType,
}

self_cell::self_cell!(
    pub(crate) struct ParsedEvalProgram {
        owner: ParsedEvalProgramOwner,

        #[covariant]
        dependent: CachedEvalProgramAst,
    }
);

self_cell::self_cell!(
    pub(crate) struct ParsedTypeResolutionContext {
        owner: Rc<ParsedEvalProgram>,

        #[covariant]
        dependent: CachedTypeResolutionContext,
    }
);

impl ParsedEvalProgram {
    pub(crate) fn parse(source: Arc<str>, source_type: oxc_span::SourceType) -> Option<Self> {
        let mut panicked = false;
        let parsed = Self::new(
            ParsedEvalProgramOwner {
                allocator: oxc_allocator::Allocator::new(),
                source,
                source_type,
            },
            |owner| {
                let result = oxc_parser::Parser::new(
                    &owner.allocator,
                    owner.source.as_ref(),
                    owner.source_type,
                )
                .with_options(oxc_parser::ParseOptions {
                    parse_regular_expression: false,
                    ..oxc_parser::ParseOptions::default()
                })
                .parse();
                panicked = result.panicked;
                result.program
            },
        );
        (!panicked).then_some(parsed)
    }

    pub(crate) fn empty(source_type: oxc_span::SourceType) -> Self {
        Self::parse(Arc::<str>::from(""), source_type)
            .expect("empty eval program should always parse")
    }

    pub(crate) fn source_bytes(&self) -> &[u8] {
        self.borrow_owner().source.as_bytes()
    }
}

pub(crate) fn next_host_instance_id() -> u64 {
    static NEXT_HOST_INSTANCE_ID: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(1);
    NEXT_HOST_INSTANCE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Consolidated resolver state: bundles the unified sub-node resolver runtime
/// with host-level top caches and singleflight groups.
///
/// Replaces the 4 individual cache/singleflight fields that were previously
/// scattered across `VerterHost`.
pub(crate) struct HostResolverState {
    /// Unified sub-node resolver runtime (symbol + fallthrough subsystems).
    pub runtime: crate::resolver_core::resolver_runtime::UnifiedResolverRuntime<
        crate::meta_resolve::ResolvedComponentMetaState,
        crate::types::FallthroughResolution,
    >,
}

impl HostResolverState {
    /// Construct a `HostResolverState` whose
    /// inner [`UnifiedResolverRuntime`](crate::resolver_core::resolver_runtime::UnifiedResolverRuntime)
    /// shares its `RouteDb` / `ImportedRootDb` authority with the host's
    /// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore)
    /// via `Arc` clones supplied by the host at construction time.
    fn new(
        routes: Arc<crate::resolver_core::RouteDb>,
        imported_roots: Arc<crate::resolver_core::ImportedRootDb>,
    ) -> Self {
        Self {
            runtime: crate::resolver_core::resolver_runtime::UnifiedResolverRuntime::new(
                routes,
                imported_roots,
            ),
        }
    }

    fn reset_all(&self) {
        self.runtime.clear_caches();
    }
}

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
    pub(crate) store_view_epoch: std::sync::atomic::AtomicU64,
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
    pub(crate) resolver: HostResolverState,
    /// Active per-host query profile — execution-policy decisions
    /// (prewarming, budgets, allowed query families). **Not a cache** — does
    /// not memoise query results. Different artifact type than anything in
    /// `ProjectTypeStore`.
    pub(crate) query_profile: parking_lot::Mutex<verter_semantic::profile::QueryProfile>,
    // The `external_type_analysis_cache` (F6) and
    // `route_owned_shallow_cache` (F7) host mutexes are not present here:
    // both halves are carried in
    // [`ProjectTypeStore.route_owned_shallow`](crate::project_type_store::ProjectTypeStore::route_owned_shallow)
    // as a single first-class artifact ([`RouteOwnedShallowEntry`]).
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
    /// Host-owned LRU cache for the typeinfo `evaluate_type_expression`
    /// scratch URIs. See `typeinfo::scratch_cache` for the LRU policy
    /// and §5.3 of the typeinfo plan for the deterministic-URI
    /// derivation. Capacity defaults to
    /// `typeinfo::scratch_cache::DEFAULT_CAPACITY` (64).
    pub(crate) typeinfo_scratch_cache:
        parking_lot::Mutex<crate::typeinfo::scratch_cache::ScratchCache>,
}

// Manual Debug impl because Arc<dyn WorkspaceAccess> doesn't implement Debug.
impl std::fmt::Debug for VerterHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerterHost")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// SourceLoader that delegates to the host's current workspace.
///
/// Holds a reference to the host's `RwLock<Arc<dyn WorkspaceAccess>>`
/// so it always reads through the latest workspace, even after
/// `set_workspace()` swaps it.
pub(crate) struct WorkspaceSourceLoader(
    pub(crate) Arc<parking_lot::RwLock<Arc<dyn verter_workspace::WorkspaceAccess>>>,
);

impl verter_scheduler::source_loader::SourceLoader for WorkspaceSourceLoader {
    fn load(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.0.read().read_file(canonical_id)
    }

    fn exists(&self, canonical_id: &str) -> bool {
        self.0.read().file_exists(canonical_id)
    }

    fn classify(&self, canonical_id: &str) -> verter_scheduler::source_loader::FileKind {
        match self.0.read().classify_file(canonical_id) {
            verter_workspace::FileKind::VueSfc => verter_scheduler::source_loader::FileKind::VueSfc,
            verter_workspace::FileKind::NonSfc => verter_scheduler::source_loader::FileKind::NonSfc,
        }
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.0.read().realpath(canonical_id)
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;
