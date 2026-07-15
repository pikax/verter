//! Host-method surface for component-meta on `VerterHost`.
//!
//! An inherent `impl VerterHost { ... }` block (Rust supports several across
//! files) that lives next to the materialization core. Owns ~18 host methods
//! including `resolve_component_meta`, `compute_component_meta_state`, the
//! `*_inner` audited variants, the registry-publication helpers
//! (`append_component_meta_registry_entries`,
//! `bridge_component_meta_registry_for_imported_macros`, ...), and the
//! request-id / fact-version / fact-key plumbing. Items owned by the parent
//! shell (registry / cycle / origin-graph predicates, the resolver adapter)
//! are reached via `super::*`.

use crate::host_manage::{
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom,
};
use crate::resolver_core::{run_component_meta_request, RequestSource, SingleflightRole};
use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};
use crate::VerterHost;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use crate::instant::Instant;

// File moved from `meta_resolve/host_methods.rs` to
// `host_manage/component_meta_methods.rs`. The original `super::X` paths
// resolved through `meta_resolve`'s private siblings; after the move,
// they rewrite to `crate::meta_resolve::X` (the parent module's
// re-exported `pub(crate)` surface).
use crate::meta_resolve::slot_binding_graph;
use crate::meta_resolve::STORE_VIEW_STABILITY_MAX_ATTEMPTS;
use crate::meta_resolve::{
    collect_define_props_root_names, slot_binding_targets_define_props_root,
    RegistryMaterialization, ResolvedComponentMetaState,
};
use crate::meta_resolve::{
    drain_dispatch_dep_signature_accumulator, reset_dispatch_dep_signature_accumulator,
};
use crate::meta_resolve::{
    next_component_meta_audit_request_id, request_source_performed_compute,
    should_skip_imported_registry_seed_refresh, trace_request_source, CapturedComponentMetaInputs,
    ResolvedComponentMetaComputeAudit, ResolvedTypeRegistryMeta,
};

// Items that live in the parent shell (`crate::meta_resolve`): the
// origin-graph builder. The `HostComponentMetaResolver` adapter lives
// in `host_manage/jsdoc_resolve.rs` (host-impl tier).
use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
use crate::meta_resolve::build_origin_graph;

use crate::resolver_core::component_meta_registry::{
    collect_component_meta_registry_public_field_refs, collect_component_meta_registry_refs_node,
    enqueue_component_meta_registry_ref, owner_component_meta_registry_import_root,
    upsert_component_meta_registry_entry, PendingComponentMetaRegistryRef,
};

// The sink-owned macro-output expansion demand API + its MODULE-PRIVATE
// node-domain artifact and materialiser live in the child sink module
// `macro_output_expansion` (a descendant of this cap's
// `pub(in crate::host_manage::component_meta_methods)` subtree. The eval_env
// expansion branches drive the re-exported `expand_*_output` demand methods.
pub(crate) mod macro_output_expansion;

pub(crate) use macro_output_expansion::{
    expand_define_model_output, expand_generic_project_path_output, expand_slot_binding_output,
    DefineModelOutputExpansion, MacroPathOutputExpansion,
};

impl VerterHost {
    /// Single host-backed resolver API for cross-file component-meta enrichment.
    ///
    /// This is the ONLY entry point for cross-file component-meta resolution.
    /// Mode is chosen explicitly by callers â€” never inferred.
    ///
    /// - `Type`: resolves symbol identity, canonical location, and attached JSDoc
    ///   without materializing expanded shapes.
    /// - `Expanded`: resolves the same way, then materializes
    ///   props/emits/slots/exposed (type-based `DefineExpose` rides the same
    ///   DTO/resolved-input path), populates the type registry, and computes
    ///   evaluated types.
    pub fn resolve_component_meta(
        &self,
        canonical_or_alias: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        // Base-only path: bind an overlay-free `HostViewRef` so the
        // shared with-view body sees a session view whose fingerprint
        // is `0` (no overlays) — this collapses to the historical
        // base-only request shape.
        let view = crate::session_view::HostViewRef::new(self);
        self.resolve_component_meta_with_view(canonical_or_alias, mode, &view)
    }

    pub(crate) fn resolve_component_meta_with_view(
        &self,
        canonical_or_alias: &str,
        mode: ProjectionMode,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<ResolvedComponentMetaState> {
        self.resolve_component_meta_with_view_and_fixed(canonical_or_alias, mode, view, None)
    }

    /// Install a per-request [`crate::request_context::RequestContext`]
    /// carrying `config.projection_op_budget` IFF none is active — the ONE
    /// shared install-if-none path that arms the projection-budget fuse AND
    /// the per-cold-compute completeness rail across EVERY component-meta
    /// surface (Shared Optimized Codebase). Each public surface installs this
    /// once around its FULL resolve + extract body so the fuse spans the
    /// fallthrough extract, not only the resolve; nested installs (the inner
    /// resolve, the fallthrough choke backstop) find the outer context and
    /// no-op, so the budget is NEVER double-installed. Returns `None` (a
    /// no-op guard) when an outer context already exists.
    #[must_use]
    pub(crate) fn install_request_budget_context_if_none(
        &self,
        request_id: u64,
        canonical_id: &str,
        timing_capture: bool,
    ) -> Option<crate::request_context::RequestContextGuard> {
        if crate::request_context::current_request_context().is_some() {
            return None;
        }
        Some(crate::request_context::RequestContextGuard::install(
            crate::request_context::RequestContext::with_kind_timing_and_projection_budget(
                request_id,
                std::sync::Arc::<str>::from(canonical_id),
                verter_audit::RequestKind::ComponentMeta,
                false,
                timing_capture,
                None,
                self.config.projection_op_budget,
            ),
        ))
    }

    /// [`Self::resolve_component_meta_with_view`] with an optional
    /// caller-captured FIXED base store view.
    ///
    /// When `fixed_store_view` is `Some((base_view, captured_fingerprint,
    /// captured_is_current))`, the request executor pins to that snapshot
    /// instead of taking its own per-attempt `resolver_store_view_read()`
    /// — the O(N)→O(1) warm-batch read collapse. The fixed view is
    /// ALREADY overlay-rooted once per batch by `capture_batch_fixed_view`
    /// (the executor consumes it directly, with no further per-job
    /// `with_session_overlay`). Promotion stays FENCED: the executor promotes the
    /// resolved-meta result into the shared cache only when the capture was
    /// current AND its captured fingerprint still equals the live host
    /// fingerprint (see [`run_component_meta_request`]).
    pub(crate) fn resolve_component_meta_with_view_and_fixed(
        &self,
        canonical_or_alias: &str,
        mode: ProjectionMode,
        view: &dyn crate::session_view::SessionView,
        fixed_store_view: Option<(&crate::resolver_store::HostStoreView, u64, bool)>,
    ) -> Option<ResolvedComponentMetaState> {
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let _ctx_guard = self.install_request_budget_context_if_none(
            next_component_meta_audit_request_id(),
            canonical.as_str(),
            self.config.audit_timing_capture && self.config.audit_enabled,
        );
        let audit = self.config.audit_enabled.then(|| {
            // Prefer the request_id stamped by
            // `get_component_meta_with_resolution` (via the installed
            // `RequestContext`). Falls back to the global static only
            // when no context is installed — e.g. direct callers of
            // `resolve_component_meta` outside the audited-request
            // path. Without this link `take_audit_record` would look
            // up the outer id while the record is stored under the
            // inner id, and every `AuditedRequest::resolve` would
            // fail with `AuditRecordMissing`.
            let request_id = crate::request_context::current_request_context()
                .map(|ctx| ctx.request_id)
                .unwrap_or_else(next_component_meta_audit_request_id);
            let (host_cache_before_bytes, workspace_before_bytes) =
                self.component_meta_audit_memory_bytes();
            (
                request_id,
                crate::component_meta_audit::begin_request_audit(request_id),
                crate::component_meta_audit::AuditBuilder::new(request_id, canonical.clone()),
                host_cache_before_bytes,
                workspace_before_bytes,
            )
        });
        component_meta_trace_custom!(
            "resolve_component_meta",
            format!("owner={} mode={mode:?}", canonical),
        );
        // Route through `ViewBoundRequestHost` so the view's fingerprint
        // discriminates the singleflight slot (R20) and overlay source
        // flows through `capture_component_meta_inputs_with_view` into
        // the cold compute path.
        //
        // Request-scoped overlay shape: the request-scoped
        // `CanonicalCompletionOverlay` is built ONCE here at the request
        // boundary and stored on the adapter struct. Every resolver
        // call inside the request shares this same `Arc` so promoted
        // canonicals accumulate across capture / try-get-cached /
        // compute boundaries.
        let request_host = crate::host_manage::component_meta_request_impl::ViewBoundRequestHost {
            host: self,
            view,
            overlay: std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        };
        let result = run_component_meta_request(
            &request_host,
            self.resolver_runtime().component_meta.singleflight(),
            &canonical,
            mode,
            fixed_store_view,
            STORE_VIEW_STABILITY_MAX_ATTEMPTS,
        );

        if matches!(result.source, RequestSource::Cache) {
            self.provenance
                .resolver_node_cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if !(matches!(result.source, RequestSource::Cache) && result.attempts == 1) {
            self.provenance
                .resolver_node_cache_misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let RequestSource::Flight { role, forked_lane } = result.source {
            if role == SingleflightRole::Follower {
                self.provenance
                    .resolver_singleflight_coalesced
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if forked_lane {
                self.provenance
                    .resolver_cross_view_lane_forks
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        if let Some(started) = started {
            match result.source {
                RequestSource::Cache => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} cached attempt={} took {:?}",
                    canonical,
                    mode,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Flight { role, .. } => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} role={:?} stable attempt={} total took {:?}",
                    canonical,
                    mode,
                    role,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Fallback => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} retries_exhausted total took {:?}",
                    canonical,
                    mode,
                    started.elapsed(),
                )),
            }
        }

        if let Some(resolved) = result.value.as_ref() {
            component_meta_trace_custom!(
                "resolve_component_meta_result",
                format!(
                    "owner={} mode={mode:?} source={} attempts={} macros={} resolved_types={} has_evaluated_types={} fact_versions={}",
                    canonical,
                    trace_request_source(result.source),
                    result.attempts,
                    resolved.resolved_macros.len(),
                    resolved.resolved_type_registry.len(),
                    resolved.evaluated_types.is_some(),
                    resolved.fact_versions.len(),
                ),
            );
        }

        if let Some((
            _request_id,
            request_audit_guard,
            mut audit_builder,
            host_cache_before_bytes,
            workspace_before_bytes,
        )) = audit
        {
            // Joiner-accounting: a request is a JOINER exactly when it
            // did NOT perform cold compute — i.e. every source other
            // than a compute-performing one. Two non-compute sources
            // reach here under concurrency:
            //
            //   - `Flight { Follower }` — the request dedup-joined an
            //     in-flight cold build and woke onto the leader's
            //     published result.
            //   - `Cache` — the request's resolver-node-cache peek
            //     served a result a concurrent (or just-completed)
            //     leader had already published. This is just as much a
            //     warm hit as a Follower join: no cold work was done.
            //     Before the cold-concurrent singleflight became a
            //     durable rendezvous, late callers in the leader's
            //     post-compute gap spawned fresh leaders; now they
            //     Follower-join or hit the node cache, and BOTH must
            //     attribute as joiners or the per-joiner contract
            //     (`from_cache=false` count <= 1) breaks under load.
            //
            // The joiner flips the speculative miss bumped by the
            // warm-cache check into a hit on the active TLS context and
            // marks `from_cache=true`. Only a compute-performing source
            // (`Flight { Leader }` / `Fallback`) stays the cold winner
            // (`from_cache=false`, miss recorded) and pays for the work
            // it actually performed.
            if !request_source_performed_compute(result.source) {
                audit_builder.mark_joined_inflight();
            }
            let (store_audit, cm_counters) = self.component_meta_audit_store_snapshot(None);
            audit_builder.record_store(store_audit);
            let (host_cache_after_bytes, workspace_after_bytes) =
                self.component_meta_audit_memory_bytes();
            audit_builder.record_memory_snapshots(
                host_cache_before_bytes,
                host_cache_after_bytes,
                workspace_before_bytes,
                workspace_after_bytes,
            );
            // Install the resolver-side solver counters FIRST, then
            // layer materializer counters on top. Order matters
            // because `record_component_meta_payload` replaces the
            // in-flight payload wholesale, while
            // `record_component_meta_store` mutates only the
            // materializer fields.
            if request_source_performed_compute(result.source) {
                if let Some(compute_audit) = result
                    .value
                    .as_ref()
                    .and_then(|resolved| resolved.compute_audit.as_ref())
                {
                    let mut timings = compute_audit.timings.clone();
                    timings.imported_root_proof_ms =
                        request_audit_guard.snapshot().imported_root_proof_ms;
                    audit_builder.record_timings(timings);
                    audit_builder.record_component_meta_payload(compute_audit.solver.clone());
                }
            }
            audit_builder.record_component_meta_store(
                cm_counters.materialize_structure_calls,
                cm_counters.materialize_structure_cache_hits,
                cm_counters.node_arena_lock_acquisitions,
                cm_counters.family_map_lock_acquisitions,
                cm_counters.dep_signature_merges,
                cm_counters.dep_signature_intern_hits,
            );
            // Project graph-native slot-binding synthesis state onto the
            // audit substrate. `diagnostics` projects via the
            // `host_audit_bridge` so consumers see one canonical
            // `AuditDiagnosticEntry` stream regardless of producer;
            // `should_suppress` mirrors the cache-write gate so audit
            // consumers can see why a request did not warm the cache.
            // The bridge module itself is non-wasm only (`verter_audit`
            // observers are a host-side concept), so the projection is
            // gated to native targets; on wasm the audit payload
            // remains in its default state.
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(resolved) = result.value.as_ref() {
                let payload = audit_builder.component_meta_payload_mut();
                payload.diagnostics = crate::host_audit_bridge::macro_expansion_to_audit_entries(
                    &resolved.synthesis_diagnostics,
                );
                payload.should_suppress = resolved.synthesis_should_suppress;
            }
            // Mine the semantic footprint when the active request is
            // capturing. Drains the per-request accumulator, builds the
            // per-file attribution vector, then feeds the rest through
            // the deterministic footprint miner before the builder
            // finalises. The file ledger is read off the state BEFORE
            // the miner consumes it.
            if let Some(ctx) = crate::request_context::current_request_context() {
                if ctx.footprint_capture {
                    if let Some(acc) = ctx.audit_accumulator.as_ref() {
                        let state = acc.drain();
                        // Direct imports of the entry: extract from
                        // the entry's shallow surface so the file-
                        // role classifier can distinguish first-level
                        // imports (`DirectImport`) from deeper-closure
                        // files (`TransitiveImport`). When the shallow
                        // surface is not yet available (rare cold
                        // path), the empty set falls back to
                        // `DirectImport` for every non-Entry file.
                        let direct_imports: rustc_hash::FxHashSet<String> = self
                            .shallow_file_state(ctx.canonical_id.as_ref())
                            .map(|sfs| {
                                sfs.import_targets
                                    .values()
                                    .map(|t| t.canonical_id.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let files = crate::component_meta_audit::build_file_audit_vec(
                            &state,
                            ctx.canonical_id.as_ref(),
                            &direct_imports,
                            self.config.audit_timing_capture && self.config.audit_enabled,
                        );
                        audit_builder.record_files(files);
                        let footprint = crate::component_meta_audit::mine_footprint(
                            self.project_type_store().semantic_graph(),
                            state,
                            &ctx,
                            self.config.max_derivation_edges,
                            &self.config.audit_caps,
                        );
                        audit_builder.record_footprint(footprint);
                    }
                }
            }
            let record = audit_builder.finish();
            crate::component_meta_audit::emit_audit_trace(&record);
            // Finalise through the `AuditRequestRegistration` planted
            // on the active `RequestContext`. The registration removes
            // the in-flight slot from the host's active-request
            // registry and inserts the record into the records store
            // so `take_audit_record(resolution.request_id)` returns it.
            // When no registration is in scope (synthetic test
            // fixture path), the helper falls back to a direct insert
            // so the host-wide store stays consistent.
            self.finalize_request_audit_record(record);
        }

        result.value
    }

    /// Test-only bare-host cold-compute entry. Production callers must
    /// thread a request-bound `&dyn ResolverContext` through one of the
    /// `*_with_view` / `*_with_session_view` / `*_with_overlay` /
    /// `*_for_fallthrough` variants — those supply the overlay-aware
    /// ctx that `compute_component_meta_state_inner` now requires
    /// unconditionally.
    ///
    /// This wrapper exists for test fixtures that drive cold-compute
    /// directly from a bare `&VerterHost`. It builds the request-bound
    /// ctx via [`crate::resolver_core::with_bare_host_ctx_for_test`]
    /// (itself `#[cfg(any(test, feature = "test-support"))]`-gated), so the
    /// release-build crate drops the helper + this wrapper entirely.
    #[cfg(any(test, feature = "test-support"))]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn compute_component_meta_state(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
    ) -> Option<ResolvedComponentMetaState> {
        crate::resolver_core::with_bare_host_ctx_for_test(self, |ctx| {
            self.compute_component_meta_state_inner(
                canonical,
                mode,
                whole_hash,
                None,
                crate::resolver_core::ComponentMetaResolutionPurpose::Full,
                RegistryMaterialization::Full,
                ctx,
            )
        })
    }

    /// Build the overlay-rooted cold-seed view for a view-bound component-
    /// meta cold compute, with currentness INTRINSIC to the read.
    ///
    /// This is the single owner of the view-bound cold-seed construction.
    /// It takes a FRESH base read and derives the cold-seed's currentness
    /// from the SAME read via [`crate::resolver_store::StoreViewRead::into_cold_seed_view`]
    /// — there is no separate currentness flag to mismatch. A
    /// [`crate::resolver_store::StoreViewRead::ReturnOnly`] read stays
    /// non-current through [`crate::resolver_store::ColdSeedHostStoreView::with_session_overlay`],
    /// so the [`crate::resolver_core::SessionResolverContext::from_cold_seed`]
    /// built from it fails every nested warm-cache probe closed. The fenced
    /// cold builder still computes from this seed; the outer `is_stable` /
    /// publish fence rejects promotion of a non-current result. Because the
    /// view and its currentness come from ONE read, there is no flag/view
    /// divergence (a stale second read marked `Current`).
    pub(crate) fn view_bound_cold_seed(
        &self,
        view: &dyn crate::session_view::SessionView,
    ) -> crate::resolver_store::ColdSeedHostStoreView {
        self.resolver_store_view_read()
            .into_cold_seed_view()
            .with_session_overlay(self, view)
    }

    /// View-aware cold compute entry. Routes resolver-tier reads (and
    /// every nested dispatcher / query-engine / prepared-decl call)
    /// through a [`crate::resolver_core::SessionResolverContext`] built over
    /// the caller-supplied overlay-rooted
    /// [`crate::resolver_store::ColdSeedHostStoreView`], so overlay candidates
    /// published by the prewarm pass are observed by the dep-source
    /// materialiser. The cold-seed carries the snapshot's currentness, so the
    /// `SessionResolverContext` fails its nested warm-cache probes closed on a
    /// non-current seed.
    ///
    /// The caller
    /// ([`ViewBoundRequestHost::compute_component_meta`](crate::host_manage::component_meta_request_impl::ViewBoundRequestHost))
    /// builds the cold-seed from the EXECUTOR's snapshot/currentness pair —
    /// the SAME read the driver's promotion fence gates on — so the compute
    /// seed and the promotion-gating seed are one read.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compute_component_meta_state_with_session_view_and_base(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
        view: &dyn crate::session_view::SessionView,
        store_view: &crate::resolver_store::ColdSeedHostStoreView,
        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
    ) -> Option<ResolvedComponentMetaState> {
        let session_ctx = crate::resolver_core::SessionResolverContext::from_cold_seed(
            self,
            view,
            store_view,
            std::sync::Arc::clone(overlay),
        );
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &session_ctx;
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            whole_hash,
            None,
            crate::resolver_core::ComponentMetaResolutionPurpose::Full,
            RegistryMaterialization::Full,
            ctx,
        )
    }

    /// Captured-inputs variant of
    /// [`Self::compute_component_meta_state_with_session_view_and_base`].
    /// Routes through a [`crate::resolver_core::SessionResolverContext`] built
    /// over the caller-supplied overlay-rooted cold-seed; the cold-seed's
    /// currentness fails nested warm-cache probes closed on a non-current
    /// seed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compute_component_meta_state_from_captured_with_session_view_and_base(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        captured: &CapturedComponentMetaInputs,
        view: &dyn crate::session_view::SessionView,
        store_view: &crate::resolver_store::ColdSeedHostStoreView,
        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
    ) -> Option<ResolvedComponentMetaState> {
        let session_ctx = crate::resolver_core::SessionResolverContext::from_cold_seed(
            self,
            view,
            store_view,
            std::sync::Arc::clone(overlay),
        );
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &session_ctx;
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            captured.whole_hash,
            Some(captured),
            crate::resolver_core::ComponentMetaResolutionPurpose::Full,
            RegistryMaterialization::Full,
            ctx,
        )
    }

    /// Compute cold-compute body with a caller-supplied executor-snapshot
    /// view. `store_view` and `base_is_current` are the executor's
    /// SINGLE-read pair; re-binding them through
    /// `StoreViewRead::from_executor_snapshot` keeps currentness intrinsic to
    /// the seed, so the `HostResolverContext` fails its nested warm-cache
    /// probes closed on a non-current snapshot. Used by the
    /// `ComponentMetaRequestHost` trait-impl boundary, which reuses the
    /// executor-snapshotted view instead of rebuilding one inside.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compute_component_meta_state_with_view_arg(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
        store_view: &crate::resolver_store::HostStoreView,
        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
        base_is_current: bool,
    ) -> Option<ResolvedComponentMetaState> {
        let cold_seed = crate::resolver_store::StoreViewRead::from_executor_snapshot(
            store_view.clone(),
            base_is_current,
        )
        .into_cold_seed_view();
        self.compute_component_meta_state_with_cold_seed_arg(
            canonical, mode, whole_hash, &cold_seed, overlay,
        )
    }

    /// Cold-compute body rooted on an already-built cold-seed whose
    /// currentness is INTRINSIC (it came from one read, via
    /// [`crate::resolver_store::StoreViewRead::into_cold_seed_view`]). The
    /// single owner of the bare-host `HostResolverContext::from_cold_seed`
    /// build, so neither caller can pair a view with a foreign flag.
    fn compute_component_meta_state_with_cold_seed_arg(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
        cold_seed: &crate::resolver_store::ColdSeedHostStoreView,
        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
    ) -> Option<ResolvedComponentMetaState> {
        let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
            self,
            cold_seed,
            std::sync::Arc::clone(overlay),
        );
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            whole_hash,
            None,
            crate::resolver_core::ComponentMetaResolutionPurpose::Full,
            RegistryMaterialization::Full,
            ctx,
        )
    }

    /// View-bearing captured-inputs cold-compute entry.
    /// Reuses an executor-snapshotted view rather than rebuilding one.
    /// `store_view` and `base_is_current` are the executor's SINGLE-read
    /// pair; re-binding them through `StoreViewRead::from_executor_snapshot`
    /// keeps currentness intrinsic to the seed, so the `HostResolverContext`
    /// fails its nested warm-cache probes closed on a non-current snapshot.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compute_component_meta_state_from_captured_with_view_arg(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        captured: &CapturedComponentMetaInputs,
        store_view: &crate::resolver_store::HostStoreView,
        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
        base_is_current: bool,
    ) -> Option<ResolvedComponentMetaState> {
        let cold_seed = crate::resolver_store::StoreViewRead::from_executor_snapshot(
            store_view.clone(),
            base_is_current,
        )
        .into_cold_seed_view();
        self.compute_component_meta_state_from_captured_with_cold_seed_arg(
            canonical, mode, captured, &cold_seed, overlay,
        )
    }

    /// Captured-inputs cold-compute body rooted on an already-built cold-seed
    /// whose currentness is INTRINSIC. The single owner of the captured-inputs
    /// bare-host `HostResolverContext::from_cold_seed` build.
    fn compute_component_meta_state_from_captured_with_cold_seed_arg(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        captured: &CapturedComponentMetaInputs,
        cold_seed: &crate::resolver_store::ColdSeedHostStoreView,
        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
    ) -> Option<ResolvedComponentMetaState> {
        let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
            self,
            cold_seed,
            std::sync::Arc::clone(overlay),
        );
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            captured.whole_hash,
            Some(captured),
            crate::resolver_core::ComponentMetaResolutionPurpose::Full,
            RegistryMaterialization::Full,
            ctx,
        )
    }

    /// Overlay-aware variant of
    /// [`Self::capture_component_meta_inputs`] used by
    /// [`ViewBoundRequestHost`](crate::host_manage::component_meta_request_impl::ViewBoundRequestHost).
    ///
    /// When `view` carries an **explicit overlay** for the owner
    /// canonical, the helper materialises the overlay's IndexedReady
    /// candidate (multi-candidate; published under the overlay-scoped
    /// key — overlay content hash plus the overlay-set discriminator)
    /// and constructs the captured inputs from the overlay's snapshot +
    /// eval_source. Otherwise — a base-only view, or an overlay view
    /// for which THIS canonical is unmasked — falls through to the
    /// base-only `capture_component_meta_inputs` path. Resolver-tier
    /// reads downstream through
    /// [`SessionResolverContext`](crate::resolver_core::SessionResolverContext)
    /// observe the overlay candidate via
    /// [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore).
    pub(crate) fn capture_component_meta_inputs_with_view(
        &self,
        canonical: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<CapturedComponentMetaInputs> {
        let audit_enabled = self.config.audit_enabled;
        let capture_started = audit_enabled.then(Instant::now);
        // Overlay-priority: when the view carries an **explicit
        // overlay** for the owner canonical, materialise its
        // IndexedReady candidate first so the base-host capture below
        // picks it up via the multi-candidate file-artifact store. The
        // materialiser derives both the source and its content hash
        // from the view itself — a single authority — so a stale
        // `FileArtifactStore`-scan hash can never be paired with the
        // fresh source. Overlay detection uses the **strict**
        // `overlay_content_hash_for` (mirroring
        // `overlay_priority::ensure_indexed_ready_serve_with_view`): a
        // base-only view, or an overlay view for which this canonical
        // is unmasked, reports `None` here and correctly delegates to
        // the base capture path — the overlay-materialiser snapshot
        // path is reserved for genuine overlays. The base host's
        // scheduler stays untouched (R17).
        let overlay_facts = if view.overlay_content_hash_for(canonical).is_some() {
            self.materialize_overlay_indexed_ready_serve_with_view(canonical, view)
                .map(|serve| serve.indexed)
        } else {
            None
        };

        if let Some(facts) = overlay_facts {
            // Overlay snapshot is the authority for the owner canonical;
            // resolver-tier deps still flow through the base host's
            // shallow state for canonicals the overlay does not cover.
            let store_read_started = audit_enabled.then(Instant::now);
            let mut snapshot = (*facts.snapshot).clone();
            self.resolve_snapshot_imports(canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                // Template inputs from the overlay artifact's OWN
                // source + SFC parse — the same content the snapshot
                // above was built from, so the template derives from
                // one coherent overlay read (never base scheduler
                // bytes converted with the overlay snapshot's
                // imports/bindings). `store_published = false` is the
                // conversion-context attestation: an overlay/session
                // conversion serves this caller only and never
                // populates the base `derived_raw_cache` slot (overlay
                // results never populate base caches; R17 — the base
                // host's scheduler and caches stay untouched).
                let template_inputs = crate::types::VueTemplateInputs {
                    source: Arc::clone(&facts.raw_source),
                    framework_parse: facts.framework_parse.clone(),
                    store_published: false,
                    // Overlay artifact read — no scheduler node
                    // generation to attest (and the persist is
                    // declined above regardless).
                    source_generation: None,
                };
                self.compute_template_analysis_if_missing(
                    canonical,
                    &mut snapshot,
                    template_inputs,
                );
            }
            let whole_hash = facts.whole_hash;
            let store_read_ms = store_read_started
                .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            let owner_eval_source = VerterHost::build_eval_script_source(
                canonical,
                &facts.raw_source,
                facts.framework_parse.as_deref(),
            );
            let direct_import_started = audit_enabled.then(Instant::now);
            let direct_dependency_candidates =
                self.cache_dependency_candidates_from_snapshot(canonical, &snapshot);
            let direct_import_proof_ms = direct_import_started
                .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            let capture_inputs_ms = capture_started
                .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            return Some(CapturedComponentMetaInputs {
                whole_hash,
                snapshot,
                owner_eval_source: Some(owner_eval_source),
                direct_dependency_candidates,
                audit_capture_inputs_ms: capture_inputs_ms,
                audit_store_read_ms: store_read_ms,
                audit_direct_import_proof_ms: direct_import_proof_ms,
            });
        }

        // No overlay for this canonical — delegate to the base capture
        // path via the request-host trait impl on `&VerterHost`. The
        // capture runs inside the request driver's fenced compute, so seed
        // from the cold-seed's inner view.
        let store_view = self
            .resolver_store_view_read()
            .into_cold_seed_view()
            .into_inner();
        <Self as crate::resolver_core::ComponentMetaRequestHost>::capture_component_meta_inputs(
            self,
            canonical,
            &store_view,
        )
    }

    pub(crate) fn compute_component_meta_state_for_fallthrough(
        &self,
        canonical: &str,
        whole_hash: Hash16,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            ProjectionMode::Expanded,
            whole_hash,
            None,
            crate::resolver_core::ComponentMetaResolutionPurpose::Fallthrough,
            RegistryMaterialization::SkipAppend,
            ctx,
        )
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn compute_component_meta_state_inner(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
        captured: Option<&CapturedComponentMetaInputs>,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
        registry_materialization: RegistryMaterialization,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<ResolvedComponentMetaState> {
        // The `ctx` parameter is required (no
        // `Option`) — the previous shape carried
        // `ctx_override: Option<&dyn ResolverContext>` with a
        // `unwrap_or(&self as &dyn ResolverContext)` bare-host
        // fallback, which the Claude review flagged as a
        // production exfiltration path that could panic any caller
        // reaching the inner via a bare-host ctx. Production callers
        // (session-bearing + view-bearing + overlay-bearing) all
        // supply a real request-bound ctx; the sole test-only wrapper
        // `compute_component_meta_state` constructs a bare-host ctx
        // via `with_bare_host_ctx_for_test` and is itself
        // `#[cfg(any(test, feature = "test-support"))]`-gated.
        // Step 6.6.A: reset the per-request dep-signature accumulator
        // so each compute call starts fresh. Inner materialize_until_stable
        // calls accumulate dispatch-side facts; we drain + merge them
        // into the published `fact_versions` below.
        reset_dispatch_dep_signature_accumulator();

        let audit_enabled = self.config.audit_enabled;
        let mut audit_timings = if audit_enabled {
            captured
                .map(|captured| crate::component_meta_audit::RequestTimingAudit {
                    capture_inputs_ms: captured.audit_capture_inputs_ms,
                    store_read_ms: captured.audit_store_read_ms,
                    direct_import_proof_ms: captured.audit_direct_import_proof_ms,
                    ..Default::default()
                })
                .unwrap_or_default()
        } else {
            crate::component_meta_audit::RequestTimingAudit::default()
        };
        component_meta_trace_custom!(
            "compute_component_meta_state",
            format!(
                "owner={} mode={mode:?} captured={} store_view={} whole_hash={whole_hash:?}",
                canonical,
                captured.is_some(),
                false,
            ),
        );
        self.provenance
            .component_meta_resolved_state_recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let snapshot = captured
            .map(|captured| captured.snapshot.clone())
            .or_else(|| self.get_raw_analysis_snapshot(canonical))?;
        component_meta_trace_custom!(
            "component_meta_snapshot",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={} script_flags={}",
                canonical,
                snapshot.imports.len(),
                snapshot.macros.len(),
                snapshot.bindings.len(),
                snapshot.template.is_some(),
                snapshot.script_flags,
            ),
        );
        // Retired `shared_owner_engine` /
        // `SessionSolverHost` pair; the resolver host is now a thin
        // wrapper around `VerterHost`.
        let resolver_host = HostComponentMetaResolver { host: self, ctx };
        let parts_started = audit_enabled.then(Instant::now);
        let parts = {
            component_meta_trace_custom!(
                "resolve_component_meta_parts",
                format!(
                    "owner={} expanded={} captured={} purpose={:?}",
                    canonical,
                    mode == ProjectionMode::Expanded,
                    captured.is_some(),
                    purpose,
                ),
            );
            crate::resolver_core::resolve_component_meta_parts(
                &resolver_host,
                canonical,
                &snapshot,
                mode == ProjectionMode::Expanded,
                captured,
                purpose,
            )
        };
        if let Some(started) = parts_started {
            audit_timings.solver_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        let mut parts = parts;
        // Graph-native slot-binding synthesis accumulators. Both
        // call sites OR-fold their `SynthesisResult` into
        // `synthesis_should_suppress` and append diagnostics into
        // `synthesis_diagnostics`; the merged result is propagated
        // through `ResolvedComponentMetaState` so the cache-write
        // gate (`!synthesis_should_suppress`) and audit-payload
        // emission both observe the same state.
        let mut synthesis_diagnostics: Vec<
            verter_semantic::analysis::component_meta::MacroExpansionDiagnostics,
        > = Vec::new();
        let mut synthesis_should_suppress = false;
        if let Some(evaluated_types) = parts.evaluated_types.as_mut() {
            let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
            let result = slot_binding_graph::resolve_slot_bindings_graph_native(
                &mut query_engine,
                canonical,
                &snapshot,
                &parts.resolved_macros,
                evaluated_types,
                &mut synthesis_diagnostics,
            );
            synthesis_should_suppress |= result.should_suppress;
        }
        let registry_before = parts.resolved_type_registry.len();
        let append_start = Instant::now();
        let should_materialize_registry = registry_materialization == RegistryMaterialization::Full;
        let should_produce_macro_object_shapes = mode == ProjectionMode::Expanded;
        let solver_audit = if should_materialize_registry || should_produce_macro_object_shapes {
            // The retired `shared_owner_engine`
            // is gone — dispatch owns all solve-like operations now.
            let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
            if should_materialize_registry {
                component_meta_trace_custom!(
                    "append_component_meta_registry_entries",
                    format!(
                        "owner={} evaluated_types={} existing_registry={}",
                        canonical,
                        parts.evaluated_types.is_some(),
                        parts.resolved_type_registry.len(),
                    ),
                );
                self.append_component_meta_registry_entries(
                    canonical,
                    &snapshot,
                    parts.evaluated_types.as_ref(),
                    &mut parts.resolved_type_registry,
                    &mut parts.resolved_type_registry_meta,
                    &mut parts.tracked_dependencies,
                    &mut query_engine,
                );
            }
            if should_produce_macro_object_shapes {
                let mut evaluated_types = parts.evaluated_types.take().unwrap_or_default();
                {
                    component_meta_trace_custom!(
                        "project_evaluated_types",
                        format!(
                            "owner={} props={} slot_bindings={} define_props={} define_slots={}",
                            canonical,
                            evaluated_types.props.len(),
                            evaluated_types.slot_bindings.len(),
                            evaluated_types.define_props.len(),
                            evaluated_types.define_slots.len(),
                        ),
                    );
                    // The flat evaluated projection is a METADATA + display
                    // surface (exactness / execution status / diagnostics /
                    // raw display types for the `evaluate_types` payload) —
                    // never a published-source authority: every published
                    // `SourcePosition` is owned by the NORMALIZED macro rows
                    // the `define_*` shapes read. Each projector dispatches
                    // `ResolveMacroPayload` + empty-path Shallow
                    // `ProjectPath`, raises surface members, and writes
                    // `Vec<ExpandedField>` into `evaluated_types.props` /
                    // `.emits`. Recursive / Error branches emit diagnostics
                    // into `synthesis_diagnostics` (silent-miss prevention).
                    crate::meta_resolve::projectors::project_evaluated_types(
                        &mut query_engine,
                        canonical,
                        &snapshot,
                        &mut evaluated_types,
                        &mut synthesis_diagnostics,
                    );
                }
                {
                    component_meta_trace_custom!(
                        "project_define_macro_shapes",
                        format!(
                            "owner={} props={} emits={}",
                            canonical,
                            evaluated_types.props.len(),
                            evaluated_types.emits.len(),
                        ),
                    );
                    // Macro-shape publication (`define_props` / `define_emits` /
                    // `define_slots`) is owned by the dispatch projector — the
                    // eager macro-object materialiser is retired. Each shape is
                    // built from the context-aware `vue_macro_dtos` normalized
                    // rows (the sole published-source authority); the flat
                    // fields contribute metadata only. NO solver / reparse /
                    // eval_source fallback.
                    crate::meta_resolve::projectors::project_define_macro_shapes(
                        &mut query_engine,
                        canonical,
                        &snapshot,
                        &mut evaluated_types,
                        &mut synthesis_diagnostics,
                        purpose,
                    );
                }
                // Slot-binding-graph synthesis + final field-type reduction are
                // only meaningful when a surface was produced — skip both passes
                // on empty surfaces to avoid no-op work. Publication is
                // unconditional: a resolved component-meta query must yield a
                // coherent `parts.evaluated_types` through both APIs
                // (`get_component_meta` + `evaluate_types`), so even an empty
                // surface set gets published — the two APIs MUST agree on whether
                // resolution produced a result.
                if !evaluated_types.is_empty() {
                    let result = slot_binding_graph::resolve_slot_bindings_graph_native(
                        &mut query_engine,
                        canonical,
                        &snapshot,
                        &parts.resolved_macros,
                        &mut evaluated_types,
                        &mut synthesis_diagnostics,
                    );
                    synthesis_should_suppress |= result.should_suppress;
                    crate::meta_resolve::projectors::reduce_published_field_types(
                        canonical,
                        &mut evaluated_types,
                        &mut query_engine,
                    );
                }
                parts.evaluated_types = Some(evaluated_types);
            }
            {
                crate::host_manage::component_meta_trace_custom!(
                    "semantic_graph_stats",
                    format!("owner={} dispatch_authority=true", canonical),
                );
            }
            if query_engine.has_fuse_tripped() {
                for trip in query_engine.fuse_trips() {
                    crate::host_manage::component_meta_trace_custom!(
                        "fuse_tripped",
                        format!(
                            "owner={} fuse={} budget={} actual={}",
                            canonical, trip.fuse_name, trip.budget, trip.actual,
                        ),
                    );
                }
            }
            crate::component_meta_audit::ComponentMetaPayload {
                total_resolve_steps: 0u64,
                solve_count: 0u32,
                ..Default::default()
            }
        } else {
            crate::host_manage::component_meta_trace_custom!(
                "semantic_graph_stats",
                format!(
                    "owner={} registry_materialization=skipped macro_shapes=skipped",
                    canonical,
                ),
            );
            crate::component_meta_audit::ComponentMetaPayload::default()
        };
        audit_timings.materialize_ms = append_start.elapsed().as_secs_f64() * 1000.0;
        let store_merge_started = audit_enabled.then(Instant::now);
        // Fact versions must reflect the post-resolution state of the host —
        // mid-request `set_import_dependencies` / `ensure_loaded` calls may
        // have updated import_routes and module_facts that the ambient
        // captured view does not see. Build a fresh snapshot here so the
        // stored facts match the live state at store time; a warm follow-up
        // query will then validate against the same post-resolution state.
        parts.fact_versions =
            self.current_dependency_fact_versions(canonical, &parts.tracked_dependencies);
        if let Some(started) = store_merge_started {
            audit_timings.store_merge_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        if audit_enabled {
            audit_timings.imported_root_proof_ms =
                crate::component_meta_audit::current_request_audit_snapshot()
                    .imported_root_proof_ms;
        }
        let append_elapsed = append_start.elapsed();
        let registry_after = parts.resolved_type_registry.len();
        if crate::host_manage::component_meta_debug_enabled() {
            let dep_cache_size = self.project_type_store.indexed().len();
            crate::host_manage::component_meta_debug(format!(
                "PROFILE owner={} registry_before={} registry_after={} registry_added={} dep_cache_entries={} append_ms={:.1}",
                canonical,
                registry_before,
                registry_after,
                registry_after - registry_before,
                dep_cache_size,
                append_elapsed.as_secs_f64() * 1000.0,
            ));
        }
        component_meta_trace_custom!(
            "component_meta_parts",
            format!(
                "owner={} resolved_macros={} resolved_type_registry={} has_evaluated_types={} fact_versions={}",
                canonical,
                parts.resolved_macros.len(),
                parts.resolved_type_registry.len(),
                parts.evaluated_types.is_some(),
                parts.fact_versions.len(),
            ),
        );
        // Step 6.6.A: drain accumulated dispatch dep_signatures and
        // merge into fact_versions before publish. Each
        // materialize_until_stable_full call inside the compute body
        // pushed the dispatch round-trip's DepSignature into the
        // thread-local accumulator; here we read + merge so warm
        // cache validation captures the dependency graph the
        // dispatch path discovered.
        let mut merged_fact_versions = parts.fact_versions;
        let dispatch_facts = drain_dispatch_dep_signature_accumulator();
        for fact in dispatch_facts {
            if !merged_fact_versions.contains(&fact) {
                merged_fact_versions.push(fact);
            }
        }

        // Step 9.1: SurfaceNodeIdentities sidecar — populated by the
        // audit-gated FieldKind closure inside
        // `compute_evaluated_types`'s
        // `expand_macro_types_impl_with_expander` call. Threaded down
        // through `ComponentMetaEvalOutputs.surface_identities` →
        // `ResolvedComponentMetaParts.surface_identities` → here.
        // `None` when audit is off (the only consumer is the scoped
        // origin export, itself audit-gated).
        let surface_identities = parts.surface_identities;
        let surface_identities_for_export = surface_identities.clone();

        // P0 #1 — Cache-suppression propagation: OR-fold the
        // request-scoped sticky flag set by reducer / materializer paths
        // (raise.rs reduce_one, field_types::materialize_*) into the
        // final synthesis-suppress signal so the `ComponentMetaResultDb`
        // admission gate at `cache_component_meta` /
        // `component_meta_entry::write_published_component_meta` refuses
        // any partial whose reducer / materializer pipeline observed a
        // budget-exceeded (or other fatal `QueryError`) read. Without
        // this OR-fold, a `cache_suppress=true` from field-type
        // materialization would warm the final-result cache and a
        // subsequent identical request would replay the poisoned partial
        // instead of re-running the cold compute against the fresh
        // budget.
        let synthesis_should_suppress = synthesis_should_suppress
            || crate::request_context::current_request_result_is_partial();
        // Typed per-result completeness: the partial signal is the union of
        // the synthesis-suppress producer signal and the request-result
        // partiality accumulator (a partial macro DTO surface / budget-tripped
        // materialize read folds in here). `synthesis_should_suppress` is the
        // bool projection of this — keep the two in lock-step.
        let completeness = if synthesis_should_suppress {
            crate::semantic_query::ResultCompleteness::partial(
                crate::semantic_query::PartialReasonSet::PROPAGATED,
            )
        } else {
            crate::semantic_query::ResultCompleteness::Complete
        };

        let state = ResolvedComponentMetaState {
            snapshot,
            mode,
            whole_hash,
            resolved_macros: parts.resolved_macros,
            resolved_type_registry: parts.resolved_type_registry,
            resolved_type_registry_meta: parts.resolved_type_registry_meta,
            evaluated_types: parts.evaluated_types,
            fact_versions: merged_fact_versions,
            surface_identities,
            synthesis_diagnostics,
            completeness,
            synthesis_should_suppress,
            compute_audit: audit_enabled.then_some(ResolvedComponentMetaComputeAudit {
                timings: audit_timings,
                solver: solver_audit,
            }),
            // F1 (D3, D34): origin_graph is audit-only. Gate matches LSP's
            // hover-provenance contract at server.rs:6918-6953 — both
            // audit_enabled and footprint_capture must be on.
            // Step 9.2 / F6: surface_identities (when populated) scopes
            // the export to the reachable subgraph rooted at the
            // request's surface nodes; falls back to workspace-total
            // export when None.
            origin_graph: (mode == ProjectionMode::Expanded
                && audit_enabled
                && self.config.footprint_capture)
                .then(|| {
                    build_origin_graph(
                        self.project_type_store.semantic_graph(),
                        surface_identities_for_export.as_ref(),
                    )
                })
                .filter(|dto| !dto.edges.is_empty()),
            request_id: 0,
        };
        Some(state)
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn append_component_meta_registry_entries(
        &self,
        owner_canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        evaluated_types: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
        resolved_type_registry: &mut Vec<
            verter_semantic::analysis::component_meta::ResolvedTypeAnalysis,
        >,
        resolved_type_registry_meta: &mut Vec<ResolvedTypeRegistryMeta>,
        tracked_dependencies: &mut BTreeSet<String>,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) {
        let _loop8_timer = crate::loop5_instrumentation::TimerGuard::new(
            &crate::loop5_instrumentation::APPEND_REGISTRY_ENTRIES_CALLS,
            &crate::loop5_instrumentation::APPEND_REGISTRY_ENTRIES_NS,
        );
        // Top-level whole-surface registry
        // projection. Subsequent commits narrow this to the actual
        // consumer demand (Pick → Include filter, Conditional → branch
        // selection); the threading through helpers + recursive call
        // sites is the load-bearing change here. Behaviour is
        // preserved at the top entry because the whole-surface cursor
        // admits every key + descend.
        let registry_projection =
            crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
                crate::meta_resolve::projection_demand::PublishedSurfaceKind::Registry,
            );
        let registry_cursor = registry_projection.cursor();
        fn track_component_meta_dependency(
            tracked_dependencies: &mut BTreeSet<String>,
            owner_canonical: &str,
            canonical_id: &str,
        ) {
            if !canonical_id.is_empty() && canonical_id != owner_canonical {
                tracked_dependencies.insert(canonical_id.to_string());
            }
        }
        // The registry-symbol "stay symbolic" root predicate, node-domain:
        // lower the resolved declaration's content-free body slot through the
        // ONE shared dispatch and classify its root via the graph-native
        // `node_root_should_stay_symbolic`. An unraisable body (evicted /
        // unloaded producing canonical) conservatively stays symbolic — the
        // consumer re-resolves the named root on demand.
        fn imported_registry_alias_should_stay_symbolic(
            ctx: &dyn crate::resolver_core::ResolverContext,
            body: &verter_type_expr::facts::PreparedTypeBodyFacts,
        ) -> bool {
            let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
            let locator =
                verter_type_expr::locators::AuthoredBodyLocator::DeclBody(body.body_slot.clone());
            match dispatch.raise_authored_locator_to_hot(
                &locator,
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
            ) {
                Some(hot) => {
                    crate::meta_resolve::exactness::node_root_should_stay_symbolic(ctx, hot.node())
                }
                None => true,
            }
        }
        /// The shallow named-reference SOURCE a registry entry publishes when
        /// its body must stay symbolic — consumers re-resolve the named root
        /// on demand through the one shared dispatch.
        fn shallow_named_ref_source(name: &str) -> verter_type_expr::facts::SemanticTypeSource {
            verter_type_expr::facts::SemanticTypeSource::Closed(
                verter_type_expr::facts::ClosedTypeFact::Leaf(
                    verter_type_expr::facts::LeafTypeFact::Ref(name.to_string()),
                ),
            )
        }
        /// The authored-body SOURCE a registry entry publishes for a resolved
        /// declaration: the content-free decl-body slot locator, lowered
        /// through the one shared dispatch on demand.
        fn authored_body_source(
            body_slot: &verter_type_expr::locators::TypeBodySlot,
        ) -> verter_type_expr::facts::SemanticTypeSource {
            verter_type_expr::facts::SemanticTypeSource::Authored(
                verter_type_expr::locators::AuthoredBodyLocator::DeclBody(body_slot.clone()),
            )
        }
        /// Node-domain imported-seed collector: walk the resolved imported
        /// alias's raised body node and enqueue transitive references,
        /// dropping deep `MemberPath` routes (len > 1) — the imported-seed
        /// contract stays path-precise and never breadth-walks unrelated
        /// imports. Top-level object member VALUES collect as roots (a bare
        /// reference member value enqueues `Whole`), matching the registry
        /// seed-scan contract.
        fn collect_imported_component_meta_registry_seed_refs_node(
            ctx: &dyn crate::resolver_core::ResolverContext,
            node: crate::semantic_query::SemanticNodeId,
            published_names: &rustc_hash::FxHashSet<String>,
            queued_names: &mut rustc_hash::FxHashSet<String>,
            output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
            source_hint: Option<&str>,
            cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
        ) {
            fn collect_one_filtered_node(
                ctx: &dyn crate::resolver_core::ResolverContext,
                node: crate::semantic_query::SemanticNodeId,
                published_names: &rustc_hash::FxHashSet<String>,
                queued_names: &mut rustc_hash::FxHashSet<String>,
                output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
                source_hint: Option<&str>,
                cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
            ) {
                let mut local_queue = std::collections::VecDeque::new();
                let mut local_names = rustc_hash::FxHashSet::default();
                collect_component_meta_registry_refs_node(
                    ctx,
                    node,
                    published_names,
                    &mut local_names,
                    &mut local_queue,
                    source_hint,
                    crate::resolver_core::component_meta_registry::RegistryMemberRefPolicy::PublicationBoundary,
                    cursor,
                );
                for pending in local_queue {
                    if matches!(
                        pending.route,
                        crate::resolver_core::RouteDemand::MemberPath(ref path) if path.len() > 1,
                    ) {
                        continue;
                    }
                    enqueue_component_meta_registry_ref(
                        published_names,
                        queued_names,
                        output,
                        pending.name.as_str(),
                        pending.source_hint.as_deref(),
                        pending.exported_name.as_deref(),
                        pending.route,
                    );
                }
            }
            match crate::project_semantic_dispatch::node_data_for(ctx, node).as_deref() {
                Some(crate::semantic_query::SemanticNodeData::Object(surface)) => {
                    for member in surface.members.iter() {
                        collect_one_filtered_node(
                            ctx,
                            member.value,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            cursor,
                        );
                    }
                    for signature in surface
                        .call_signatures
                        .iter()
                        .chain(surface.construct_signatures.iter())
                    {
                        collect_one_filtered_node(
                            ctx,
                            *signature,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            cursor,
                        );
                    }
                    for signature in surface.index_signatures.iter() {
                        collect_one_filtered_node(
                            ctx,
                            signature.key_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            cursor,
                        );
                        collect_one_filtered_node(
                            ctx,
                            signature.value_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            cursor,
                        );
                    }
                }
                _ => {
                    collect_one_filtered_node(
                        ctx,
                        node,
                        published_names,
                        queued_names,
                        output,
                        source_hint,
                        cursor,
                    );
                }
            }
        }
        let debug_enabled = crate::host_manage::component_meta_debug_enabled();
        let import_refresh_started = debug_enabled.then(Instant::now);
        // §3.4 structural role classification for the seed refresh below: a
        // registry entry whose NAME a type-role-bearing macro consumes
        // (`defineProps<ButtonSlots>()`'s direct type references) is the
        // macro root's SEPARATE symbolic publication — it stays the original
        // shallow `Ref` seed (consumers re-resolve the named root on demand
        // through the one shared dispatch; the published surface authority is
        // the typeinfo macro-surface path). Rewriting it to the authored
        // declaration body would eagerly fold the surface the registry must
        // keep shallow.
        let macro_root_names: rustc_hash::FxHashSet<&str> = snapshot
            .macros
            .iter()
            .flat_map(|mac| mac.type_references.iter().map(String::as_str))
            .collect();
        for (index, entry) in resolved_type_registry.iter_mut().enumerate() {
            let _entry_started = debug_enabled.then(Instant::now);
            let Some(meta) = resolved_type_registry_meta.get_mut(index) else {
                continue;
            };
            if macro_root_names.contains(entry.name.as_str()) {
                continue;
            }
            let declaration_source = meta.declaration.canonical_source.clone();
            if declaration_source.is_empty() || declaration_source == owner_canonical {
                continue;
            }
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration_source.as_str(),
            );
            if entry.type_source.present().is_some_and(|source| {
                should_skip_imported_registry_seed_refresh(
                    owner_canonical,
                    &meta.declaration,
                    source,
                )
            }) {
                continue;
            }
            let requested_exported_name = if meta.declaration.resolved_name.is_empty() {
                entry.name.as_str()
            } else {
                meta.declaration.resolved_name.as_str()
            };
            let Some(resolved) = query_engine.resolve_imported_registry_symbol(
                declaration_source.as_str(),
                requested_exported_name,
            ) else {
                continue;
            };
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                resolved.canonical_id.as_str(),
            );
            for dependency in &resolved.canonical_dependencies {
                track_component_meta_dependency(
                    tracked_dependencies,
                    owner_canonical,
                    dependency.as_str(),
                );
            }
            meta.declaration.canonical_source = resolved.canonical_id.clone();
            if imported_registry_alias_should_stay_symbolic(query_engine.ctx, &resolved.body) {
                entry.type_source = verter_type_expr::facts::SourcePosition::Present(
                    shallow_named_ref_source(entry.name.as_str()),
                );
                continue;
            }
            // Publish the resolved declaration's content-free authored body
            // slot — consumers lower it through the one shared dispatch on
            // demand (shallow-by-default; no materialised `TypeExpr`).
            entry.type_source = verter_type_expr::facts::SourcePosition::Present(
                authored_body_source(&resolved.body.body_slot),
            );
            if let Some(started) = _entry_started {
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                if elapsed_ms >= 5.0 {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_IMPORT_UPDATE owner={} name={} source={} resolved={} elapsed_ms={:.1}",
                        owner_canonical,
                        entry.name,
                        declaration_source,
                        meta.declaration.resolved_name,
                        elapsed_ms,
                    ));
                }
            }
        }
        let import_refresh_elapsed_ms = import_refresh_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();

        let mut referenced_names: VecDeque<PendingComponentMetaRegistryRef> = VecDeque::new();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut published_names: rustc_hash::FxHashSet<String> = resolved_type_registry
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        let public_field_collect_started = debug_enabled.then(Instant::now);
        if let Some(evaluated_types) = evaluated_types {
            for field in &evaluated_types.props {
                collect_component_meta_registry_public_field_refs(
                    query_engine.ctx,
                    owner_canonical,
                    snapshot,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
            for field in &evaluated_types.emits {
                collect_component_meta_registry_public_field_refs(
                    query_engine.ctx,
                    owner_canonical,
                    snapshot,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
            let define_props_roots =
                collect_define_props_root_names(query_engine.ctx, owner_canonical, snapshot);
            // Refuse-to-enqueue for synthetic slot-binding carriers.
            // The slot-binding graph publisher's no-parser-branch
            // mints a `TypeExpr::SyntheticSlotBinding(_)` typed-IR
            // carrier whose `binding_name` is intrinsic, NOT a real
            // type alias declared anywhere in the workspace.
            // Treating it as a public type ref would re-enter the
            // registry through `can_resolve_registry_symbol` /
            // `resolve_type_declaration` looking for a type alias
            // that does not exist, and on cache miss walk every
            // owner-local prepared decl + every imported root
            // looking for it.
            //
            // The check is scoped exactly to synthetic carriers via
            // the typed-IR variant identity — a real type alias with
            // the same identifier name (`type foo = …`) lives on a
            // different `TypeExpr` variant and cannot be suppressed
            // by this gate.
            for field in &evaluated_types.slot_bindings {
                // Issue #8 skip runs FIRST — it classifies off the binding's
                // authored SHALLOW annotation (`Props['avatar']`), which
                // survives on a graph-raised binding row whose published
                // source is the synthetic carrier. An owner-rooted binding is
                // redundant registry work regardless of which source variant
                // publishes it.
                if slot_binding_targets_define_props_root(
                    query_engine.ctx,
                    owner_canonical,
                    field,
                    &define_props_roots,
                ) {
                    #[cfg(any(test, feature = "test-support"))]
                    crate::capture_token::with_active_capture(|t| {
                        t.record_counter(
                            crate::meta_resolve::SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER,
                            1,
                        );
                    });
                    continue;
                }
                // A synthetic binding carrier (the graph-raised binding row
                // published against its first-class synthetic source) names no
                // workspace type alias — the typed SOURCE-variant identity is
                // the refuse-to-enqueue signal.
                if matches!(
                    field.r#type.present(),
                    Some(verter_type_expr::facts::SemanticTypeSource::SyntheticSlotBinding(_))
                ) {
                    continue;
                }
                collect_component_meta_registry_public_field_refs(
                    query_engine.ctx,
                    owner_canonical,
                    snapshot,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
        }
        let public_field_collect_elapsed_ms = public_field_collect_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();
        let seed_scan_started = debug_enabled.then(Instant::now);
        // One dispatch for every seed-source raise below: registry locators
        // and shallow sources lower through the ONE shared engine, and the
        // discovery walks run node-domain off the raised carriers.
        let engine_ctx = query_engine.ctx;
        let seed_dispatch =
            crate::project_semantic_dispatch::ProjectSemanticDispatch::new(engine_ctx);
        let seed_transit_ctx =
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            );
        let raise_seed_source =
            |dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
             source: &verter_type_expr::facts::SemanticTypeSource| {
                dispatch.raise_semantic_type_source_to_hot(
                    source,
                    crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                        scope_canonical_id: owner_canonical,
                        context: seed_transit_ctx,
                        interior_failures: None,
                    },
                )
            };
        for (index, entry) in resolved_type_registry.iter().enumerate() {
            let Some(meta) = resolved_type_registry_meta.get(index) else {
                continue;
            };
            let source_hint = Some(meta.declaration.canonical_source.as_str());
            let entry_import_root = owner_component_meta_registry_import_root(
                query_engine.ctx,
                owner_canonical,
                snapshot,
                entry.name.as_str(),
            );
            let entry_is_imported = entry_import_root.as_ref().is_some_and(|(canonical_id, _)| {
                !canonical_id.is_empty() && canonical_id != owner_canonical
            }) || (!meta.declaration.canonical_source.is_empty()
                && meta.declaration.canonical_source != owner_canonical);
            if entry.type_source.present().is_some_and(|source| {
                should_skip_imported_registry_seed_refresh(
                    owner_canonical,
                    &meta.declaration,
                    source,
                )
            }) {
                continue;
            }
            let source_locator = source_hint
                .filter(|source| source.is_empty() || *source == owner_canonical)
                .and_then(|_| {
                    query_engine.owner_collection_expr(owner_canonical, entry.name.as_str())
                });
            if entry_is_imported {
                // Shallow-by-default registry seeds publish imported helper
                // aliases as a bare `Ref { name }` (the registry-shallow
                // contract). A bare self-ref exposes none of the alias's
                // declared body, so transitive helper references the
                // published surface DOES materialize path-precise — e.g. a
                // slot binding `default(props: { ui: Button['ui'] })` reached
                // through an imported `ButtonSlots` — would never be
                // discovered for registry publication.
                //
                // Resolve the imported alias's declared body in ITS OWN
                // declaring scope and collect transitive refs from that body
                // (with the declaring file as the source hint, so referenced
                // names like `Button` resolve where they are declared). The
                // imported-seed collector stays path-precise: it drops deep
                // `MemberPath` routes (len > 1) and honours the registry
                // cursor, so this does not breadth-walk unrelated imports.
                let imported_collection_locator = entry
                    .type_source
                    .present()
                    .and_then(crate::resolver_core::component_meta_registry::source_bare_ref_name)
                    .filter(|ref_name| *ref_name == meta.declaration.resolved_name)
                    .filter(|_| !meta.declaration.canonical_source.is_empty())
                    .map(|_| ())
                    .and_then(|_| {
                        query_engine.named_decl_body(
                            meta.declaration.canonical_source.as_str(),
                            meta.declaration.resolved_name.as_str(),
                        )
                    });
                // Raise ONE input node through the shared dispatch: the
                // imported declaration's body locator, else the owner
                // collection locator, else the entry's own published source.
                let seed_node = match imported_collection_locator
                    .as_ref()
                    .or(source_locator.as_ref())
                {
                    Some(locator) => seed_dispatch
                        .raise_authored_locator_to_hot(locator, seed_transit_ctx)
                        .map(|hot| hot.node()),
                    None => entry
                        .type_source
                        .present()
                        .and_then(|source| raise_seed_source(&seed_dispatch, source))
                        .map(|hot| hot.node()),
                };
                if let Some(node) = seed_node {
                    collect_imported_component_meta_registry_seed_refs_node(
                        query_engine.ctx,
                        node,
                        &published_names,
                        &mut queued_names,
                        &mut referenced_names,
                        source_hint,
                        registry_cursor,
                    );
                }
            } else {
                let seed_node = match source_locator.as_ref() {
                    Some(locator) => seed_dispatch
                        .raise_authored_locator_to_hot(locator, seed_transit_ctx)
                        .map(|hot| hot.node()),
                    None => entry
                        .type_source
                        .present()
                        .and_then(|source| raise_seed_source(&seed_dispatch, source))
                        .map(|hot| hot.node()),
                };
                if let Some(node) = seed_node {
                    // Q6 (demand-scoped owner-local helper discovery): a plain
                    // named ref reached on an actively demanded member path of
                    // an OWNER-LOCAL seed is a registry dependency — the
                    // collector threads this eligibility unchanged through
                    // transparent composites and stops at the first plain ref
                    // head (discovery only; dequeue-time resolution applies
                    // the owner-local-vs-imported policy).
                    collect_component_meta_registry_refs_node(
                        query_engine.ctx,
                        node,
                        &published_names,
                        &mut queued_names,
                        &mut referenced_names,
                        source_hint,
                        crate::resolver_core::component_meta_registry::RegistryMemberRefPolicy::DemandedOwnerLocalSurface,
                        registry_cursor,
                    );
                }
            }
        }
        let seed_scan_elapsed_ms = seed_scan_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();

        // Names referenced from already-seeded registry entries.
        // Helpers that a published type transitively references should
        // still be published even when they are imported generic aliases.
        let seeded_dependency_names: rustc_hash::FxHashSet<String> = {
            let mut names = rustc_hash::FxHashSet::default();
            for entry in resolved_type_registry.iter() {
                if let Some(hot) = entry
                    .type_source
                    .present()
                    .and_then(|source| raise_seed_source(&seed_dispatch, source))
                {
                    crate::resolver_core::component_meta_registry::collect_node_ref_names(
                        query_engine.ctx,
                        hot.node(),
                        &mut names,
                    );
                }
            }
            // Also include owner-local names queued alongside a seeded
            // published entry. When the registry already has published
            // entries, any owner-local pending name was transitively
            // enqueued through seed scanning and must keep its own
            // registry entry instead of being inlined as an indexed-access
            // alias. When there are no published entries yet, pending
            // names come purely from public-field scanning and may still
            // be inlined; do not protect them here.
            if !published_names.is_empty() {
                for pending in referenced_names.iter() {
                    if pending
                        .source_hint
                        .as_deref()
                        .is_none_or(|s| s.is_empty() || s == owner_canonical)
                    {
                        names.insert(pending.name.clone());
                    }
                }
            }
            names
        };
        let mut _loop_iterations: usize = 0;
        let mut _loop_materializations: usize = 0;
        let _loop_start = Instant::now();
        while let Some(pending) = referenced_names.pop_front() {
            _loop_iterations += 1;
            if !query_engine.allow_registry_deepening() {
                break;
            }
            let _pending_started =
                crate::host_manage::component_meta_debug_enabled().then(Instant::now);
            let PendingComponentMetaRegistryRef {
                name: type_name,
                source_hint: pending_source_hint_owned,
                exported_name: pending_exported_name_owned,
                route: pending_route,
                member_use_sites: pending_member_use_sites,
            } = pending;
            // A DEEP member path (len > 1) selects NO top-level member of the
            // route root — the consumer resolves that path inline,
            // path-precisely, so publication for the root is route-scoped to
            // nothing. Skipping keeps the published entry (if any) from
            // widening into sibling members no consumer demanded.
            let pending_route_is_deep_member_path = matches!(
                &pending_route,
                crate::resolver_core::RouteDemand::MemberPath(path) if path.len() > 1
            );
            let imported_owner_route = owner_component_meta_registry_import_root(
                query_engine.ctx,
                owner_canonical,
                snapshot,
                type_name.as_str(),
            )
            .filter(|_| {
                pending_source_hint_owned
                    .as_deref()
                    .is_none_or(|source| source.is_empty() || source == owner_canonical)
            });
            let pending_source_hint = imported_owner_route
                .as_ref()
                .map(|(canonical_id, _)| canonical_id.as_str())
                .or(pending_source_hint_owned.as_deref());
            let pending_exported_name = imported_owner_route
                .as_ref()
                .map(|(_, exported_name)| exported_name.as_str())
                .or(pending_exported_name_owned.as_deref());
            if matches!(pending_route, crate::resolver_core::RouteDemand::Whole)
                && imported_owner_route
                    .as_ref()
                    .is_some_and(|(canonical_id, _)| {
                        query_engine.ctx.workspace_is_package_backed(canonical_id)
                    })
            {
                continue;
            }
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_PENDING owner={} name={} source_hint={:?} exported={:?} route={:?}",
                    owner_canonical,
                    type_name,
                    pending_source_hint,
                    pending_exported_name,
                    pending_route,
                ));
            }
            let _can_resolve = query_engine.can_resolve_registry_symbol(
                owner_canonical,
                pending_exported_name.unwrap_or(type_name.as_str()),
                pending_source_hint,
            );
            if crate::host_manage::component_meta_debug_enabled() && !_can_resolve {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_SKIP_UNRESOLVABLE owner={} name={} source_hint={:?} exported={:?}",
                    owner_canonical, type_name, pending_source_hint, pending_exported_name,
                ));
            }
            if !_can_resolve {
                continue;
            }
            let requested_exported_name = pending_exported_name.unwrap_or(type_name.as_str());
            if let Some(source_hint) = pending_source_hint
                .filter(|source| !source.is_empty() && *source != owner_canonical)
            {
                if !query_engine.allow_imported_root() {
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "REGISTRY_SKIP_BUDGET owner={} name={}",
                            owner_canonical, type_name,
                        ));
                    }
                    continue;
                }
                track_component_meta_dependency(tracked_dependencies, owner_canonical, source_hint);
                let _imported_pending_started =
                    crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                let _resolved_import = query_engine
                    .resolve_imported_registry_symbol(source_hint, requested_exported_name);
                if crate::host_manage::component_meta_debug_enabled() && _resolved_import.is_none()
                {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_IMPORT_MISS owner={} name={} source={} exported={}",
                        owner_canonical, type_name, source_hint, requested_exported_name,
                    ));
                }
                if let Some(resolved) = _resolved_import {
                    let imported_resolve_elapsed_ms = _imported_pending_started
                        .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        resolved.canonical_id.as_str(),
                    );
                    for dependency in &resolved.canonical_dependencies {
                        track_component_meta_dependency(
                            tracked_dependencies,
                            owner_canonical,
                            dependency.as_str(),
                        );
                    }
                    let declaration_started =
                        crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                    let mut declaration =
                        if matches!(pending_route, crate::resolver_core::RouteDemand::Whole) {
                            query_engine.resolve_type_declaration(
                                resolved.canonical_id.as_str(),
                                resolved.exported_name.as_str(),
                            )
                        } else {
                            query_engine
                                .resolve_direct_prepared_type_declaration_metadata(
                                    resolved.canonical_id.as_str(),
                                    resolved.exported_name.as_str(),
                                )
                                .unwrap_or_else(|| {
                                    query_engine.resolve_type_declaration(
                                        resolved.canonical_id.as_str(),
                                        resolved.exported_name.as_str(),
                                    )
                                })
                        };
                    let declaration_elapsed_ms = declaration_started
                        .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    if declaration.canonical_source.is_empty() {
                        declaration.canonical_source = resolved.canonical_id.clone();
                    }
                    let pending_route_is_whole = match &pending_route {
                        crate::resolver_core::RouteDemand::Whole => true,
                        crate::resolver_core::RouteDemand::MemberPath(path) => path.is_empty(),
                        _ => false,
                    };
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "REGISTRY_IMPORTED_GATE owner={} name={} stay_symbolic={} route_whole={} body_class={:?}",
                            owner_canonical, type_name,
                            imported_registry_alias_should_stay_symbolic(
                                query_engine.ctx,
                                &resolved.body,
                            ),
                            pending_route_is_whole,
                            resolved.body.classification,
                        ));
                    }
                    if pending_route_is_whole
                        && imported_registry_alias_should_stay_symbolic(
                            query_engine.ctx,
                            &resolved.body,
                        )
                    {
                        // Imported non-object helpers (mapped/conditional/
                        // indexed-access/typeof aliases) must not be expanded
                        // into the owner registry on a whole-type route — the
                        // consumer will resolve them through member paths.
                        //
                        // If we already published a richer entry under this
                        // name, refresh its declaration metadata (the
                        // node-domain preference in
                        // upsert_component_meta_registry_entry keeps the
                        // richer source, so the bare named-reference seed is
                        // discarded).
                        //
                        // If the name was never published, skip publication
                        // entirely — a bare named-reference seed only leaks a
                        // symbolic helper that the consumer didn't ask for.
                        if published_names.contains(&type_name) {
                            upsert_component_meta_registry_entry(
                                owner_canonical,
                                resolved_type_registry,
                                resolved_type_registry_meta,
                                &mut published_names,
                                &mut queued_names,
                                &mut referenced_names,
                                query_engine.ctx,
                                type_name.clone(),
                                shallow_named_ref_source(type_name.as_str()),
                                declaration,
                                registry_cursor,
                            );
                        }
                        continue;
                    }
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        declaration.canonical_source.as_str(),
                    );
                    if pending_route_is_deep_member_path {
                        // Route-scoped publication: a deep member path selects
                        // no top-level member — never widen the entry with the
                        // whole authored body.
                        continue;
                    }
                    let surface_started =
                        crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                    // Route-scoped registry publication: a non-whole route
                    // publishes the SELECTED one-level topology as a
                    // `Projected(Surface)` fact (member payloads stay lazy
                    // authored slots); multiple discoveries union
                    // monotonically at the upsert. A whole route (or a
                    // route-scoped build decline) publishes the resolved
                    // declaration's content-free authored body slot —
                    // consumers project it path-precisely through the one
                    // shared dispatch.
                    let route_scoped_source = (!pending_route_is_whole)
                        .then(|| {
                            // Resolve within the resolved declaration's own
                            // file scope — the pending name need not be
                            // visible from the owner (it may be referenced
                            // only inside the imported file).
                            query_engine.route_scoped_surface_fact(
                                resolved.canonical_id.as_str(),
                                resolved.exported_name.as_str(),
                                &pending_route,
                                &pending_member_use_sites,
                            )
                        })
                        .flatten()
                        .map(|fact| {
                            verter_type_expr::facts::SemanticTypeSource::Projected(
                                verter_type_expr::facts::ProjectedTypeFact::Surface(fact),
                            )
                        });
                    let type_source = match route_scoped_source {
                        Some(source) => source,
                        // A route-scoped demand publishes the SELECTED
                        // topology or nothing — an imported entry whose
                        // route-scoped surface cannot be built (an open
                        // mapped/generic surface with no closable member
                        // selection) is NEVER widened to the whole authored
                        // body (that would leak the body's unrequested
                        // references — e.g. an open mapped helper's
                        // non-identity value type — into the registry).
                        None if !pending_route_is_whole => {
                            continue;
                        }
                        None => authored_body_source(&resolved.body.body_slot),
                    };
                    let surface_elapsed_ms = surface_started
                        .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    upsert_component_meta_registry_entry(
                        owner_canonical,
                        resolved_type_registry,
                        resolved_type_registry_meta,
                        &mut published_names,
                        &mut queued_names,
                        &mut referenced_names,
                        query_engine.ctx,
                        type_name.clone(),
                        type_source,
                        declaration,
                        registry_cursor,
                    );
                    if let Some(started) = _pending_started {
                        let total_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                        if total_elapsed_ms >= 5.0 {
                            crate::host_manage::component_meta_debug(format!(
                                "REGISTRY_PENDING_IMPORTED owner={} name={} source={} resolved={} resolve_ms={:.1} declaration_ms={:.1} surface_ms={:.1} total_ms={:.1}",
                                owner_canonical,
                                type_name,
                                source_hint,
                                resolved.canonical_id,
                                imported_resolve_elapsed_ms,
                                declaration_elapsed_ms,
                                surface_elapsed_ms,
                                total_elapsed_ms,
                            ));
                        }
                    }
                    continue;
                }
            }

            let declaration_owner = pending_source_hint
                .filter(|source| !source.is_empty())
                .unwrap_or(owner_canonical);
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration_owner,
            );
            let mut declaration =
                query_engine.resolve_type_declaration(declaration_owner, type_name.as_str());
            if declaration.canonical_source.is_empty() && declaration_owner != owner_canonical {
                declaration =
                    query_engine.resolve_type_declaration(owner_canonical, type_name.as_str());
            }
            let declaration_body =
                query_engine.named_decl_body(declaration_owner, type_name.as_str());
            let owner_collection_locator =
                query_engine.owner_collection_expr(owner_canonical, type_name.as_str());
            // The owner-collection body's reference HEAD, read node-domain:
            // the OwnerCollectionDb locator lowers through the ONE shared
            // dispatch and the head classifies off the raised carrier — no
            // materialised `TypeExpr` in the BFS.
            let owner_collection_ref_head = owner_collection_locator.as_ref().and_then(|locator| {
                seed_dispatch
                    .raise_authored_locator_to_hot(locator, seed_transit_ctx)
                    .and_then(|hot| {
                        crate::resolver_core::component_meta_registry::component_meta_registry_node_ref_head(
                            engine_ctx,
                            hot.node(),
                        )
                    })
            });
            // Owner-local type aliases whose body is a generic ref to an
            // imported type should resolve inline via indexed access rather
            // than creating a separate registry entry.
            let pending_route_is_whole_local =
                matches!(pending_route, crate::resolver_core::RouteDemand::Whole)
                    || matches!(
                        pending_route,
                        crate::resolver_core::RouteDemand::MemberPath(ref p) if p.is_empty(),
                    );
            if declaration_owner == owner_canonical
                && !pending_route_is_whole_local
                && !seeded_dependency_names.contains(&type_name)
            {
                if let Some((body_ref_name, type_arguments)) = owner_collection_ref_head.as_ref() {
                    if !type_arguments.is_empty() {
                        let body_decl =
                            query_engine.resolve_type_declaration(owner_canonical, body_ref_name);
                        let body_scope = if body_decl.canonical_source.is_empty() {
                            owner_canonical
                        } else {
                            body_decl.canonical_source.as_str()
                        };
                        if body_scope != owner_canonical {
                            continue;
                        }
                    }
                }
            }
            // The published source: the declaration's content-free authored
            // body slot (the imported scope's decl body when the pending name
            // resolved off-owner, else the owner's collection locator / own
            // decl body). Consumers lower it through the one shared dispatch
            // on demand.
            let published_locator = if declaration_owner != owner_canonical {
                declaration_body.clone()
            } else {
                owner_collection_locator
                    .clone()
                    .or_else(|| query_engine.named_decl_body(owner_canonical, type_name.as_str()))
            };
            if published_locator.is_some() && declaration.canonical_source.is_empty() {
                if let Some(import) = snapshot
                    .imports
                    .iter()
                    .find(|imp| imp.bindings.iter().any(|b| b.name == type_name))
                {
                    if let Some(canonical_id) = import.resolved_canonical_id.as_deref() {
                        if let Some(binding) = import.bindings.iter().find(|b| b.name == type_name)
                        {
                            declaration.canonical_source = canonical_id.to_string();
                            declaration.resolved_name = binding
                                .imported_name
                                .as_deref()
                                .unwrap_or("default")
                                .to_string();
                        }
                    }
                }
            }
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration.canonical_source.as_str(),
            );
            let Some(published_locator) = published_locator else {
                continue;
            };
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_PENDING_LOCAL_SURFACE owner={} name={} route={:?} locator={:?}",
                    owner_canonical, type_name, pending_route, published_locator
                ));
            }
            // Substituted-whole exemption: an OWNER-LOCAL alias whose body is
            // a generic APPLICATION (`type Button = ComponentConfig<...>`)
            // publishes the WHOLE scope-bearing authored body on ANY route —
            // the observation executes the outer substitution on demand, so
            // narrowing the entry to a route selection would lose the
            // substituted sibling members consumers demand.
            let owner_local_substituted_whole = declaration_owner == owner_canonical
                && owner_collection_ref_head
                    .as_ref()
                    .is_some_and(|(_, type_arguments)| !type_arguments.is_empty());
            if pending_route_is_deep_member_path && !owner_local_substituted_whole {
                // Route-scoped publication: a deep member path selects no
                // top-level member — never widen the entry with the whole
                // authored body.
                continue;
            }
            _loop_materializations += 1;
            // Heritage composition at surface observation: a whole-route
            // owner-local entry whose raised body root is the lone-`extends`
            // heritage intersection publishes the one-level MERGED surface
            // composed through the shared empty-path Shallow surface walker
            // (member values stay shallow slots). A NON-whole route publishes
            // the route-SELECTED one-level topology as a `Projected(Surface)`
            // fact (member payloads stay lazy authored slots; multiple
            // discoveries union monotonically at the upsert) — EXCEPT the
            // substituted-whole exemption above. Every other entry keeps the
            // raw authored carrier — the raw Intersection stays the graph
            // carrier outside this structural-surface observation point.
            let published_source =
                if pending_route_is_whole_local && declaration_owner == owner_canonical {
                    query_engine
                        .heritage_merged_surface_fact(owner_canonical, type_name.as_str())
                        .map(|fact| {
                            verter_type_expr::facts::SemanticTypeSource::Projected(
                                verter_type_expr::facts::ProjectedTypeFact::Surface(fact),
                            )
                        })
                        .unwrap_or_else(|| {
                            verter_type_expr::facts::SemanticTypeSource::Authored(
                                published_locator.clone(),
                            )
                        })
                } else if let Some(fact) = (!pending_route_is_whole_local
                    && !owner_local_substituted_whole)
                    .then(|| {
                        query_engine.route_scoped_surface_fact(
                            owner_canonical,
                            type_name.as_str(),
                            &pending_route,
                            &pending_member_use_sites,
                        )
                    })
                    .flatten()
                {
                    verter_type_expr::facts::SemanticTypeSource::Projected(
                        verter_type_expr::facts::ProjectedTypeFact::Surface(fact),
                    )
                } else {
                    verter_type_expr::facts::SemanticTypeSource::Authored(published_locator.clone())
                };
            upsert_component_meta_registry_entry(
                owner_canonical,
                resolved_type_registry,
                resolved_type_registry_meta,
                &mut published_names,
                &mut queued_names,
                &mut referenced_names,
                query_engine.ctx,
                type_name.clone(),
                published_source,
                declaration,
                registry_cursor,
            );
            if let Some(started) = _pending_started {
                let total_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                if total_elapsed_ms >= 5.0 {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_PENDING_LOCAL owner={} name={} declaration_owner={} route={:?} total_ms={:.1}",
                        owner_canonical, type_name, declaration_owner, pending_route, total_elapsed_ms,
                    ));
                }
            }
        }
        if crate::host_manage::component_meta_debug_enabled()
            && (_loop_materializations > 0 || _loop_iterations > 0)
        {
            crate::host_manage::component_meta_debug(format!(
                "REGISTRY_LOOP owner={} iterations={} materializations={} published={} loop_ms={:.1}",
                owner_canonical,
                _loop_iterations,
                _loop_materializations,
                published_names.len(),
                _loop_start.elapsed().as_secs_f64() * 1000.0,
                ));
        }
        let loop_elapsed_ms = _loop_start.elapsed().as_secs_f64() * 1000.0;
        if debug_enabled {
            crate::host_manage::component_meta_debug(format!(
                "PROFILE_PHASES owner={} import_refresh_ms={:.1} public_field_collect_ms={:.1} seed_scan_ms={:.1} loop_ms={:.1}",
                owner_canonical,
                import_refresh_elapsed_ms,
                public_field_collect_elapsed_ms,
                seed_scan_elapsed_ms,
                loop_elapsed_ms,
            ));
        }
    }

    /// Get a raw analysis snapshot without any enrichment.
    ///
    /// For owner files in the scheduler, reads the scheduler's latest analysis
    /// (which reflects post-recompile state). For imported deps and non-scheduler
    /// files, reads from `FileArtifactStore` (materializing on miss). Both paths enrich
    /// the snapshot with resolved imports, destructured bindings, and template
    /// analysis.
    pub(crate) fn get_raw_analysis_snapshot(
        &self,
        canonical: &str,
    ) -> Option<FileAnalysisSnapshot> {
        component_meta_trace_custom!(
            "get_raw_analysis_snapshot",
            format!("owner={} store_view={}", canonical, false),
        );
        let normalized_canonical = self.normalized_analysis_canonical(canonical);
        let canonical = normalized_canonical.as_ref();

        {
            if self.is_canonical_evicted(canonical) {
                return None;
            }

            // Scheduler-first path for owner files: the scheduler has the
            // latest analysis after recompile, including updated import
            // routes for newly-added dependencies. FileArtifactStore may hold
            // stale import routes for owner files whose deps changed after
            // materialization.
            if let Some((snapshot, template_inputs)) =
                self.build_snapshot_from_scheduler_with_template_inputs(canonical)
            {
                let whole_hash = self
                    .current_or_read_whole_hash(canonical)
                    .unwrap_or_default();
                if !self.store_view_allows_current_whole_hash(canonical, whole_hash) {
                    return None;
                }
                let mut snapshot = snapshot;
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    // Thread the inputs joined at the snapshot's own
                    // generation — never a second independent scheduler
                    // consult. A torn join (the source moved between
                    // the analysis capture and the source read) carries
                    // `None` inputs: the template stays absent for this
                    // caller instead of deriving from bytes the
                    // snapshot was not built from and persisting them
                    // into the rail-less `derived_raw_cache` slot.
                    if let Some(inputs) = template_inputs {
                        self.compute_template_analysis_if_missing(canonical, &mut snapshot, inputs);
                    }
                }
                component_meta_trace_custom!(
                    "get_raw_analysis_snapshot_result",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={} source=scheduler",
                        canonical,
                        snapshot.imports.len(),
                        snapshot.macros.len(),
                        snapshot.bindings.len(),
                        snapshot.template.is_some(),
                    ),
                );
                return Some(snapshot);
            }
        }

        // FileArtifactStore path: covers imported deps and non-scheduler files.
        let serve = self.ensure_indexed_ready_serve(canonical)?;
        let facts = serve.indexed;
        let mut snapshot = (*facts.snapshot).clone();
        self.resolve_snapshot_imports(canonical, &mut snapshot);
        self.enrich_destructured_bindings(&mut snapshot);
        if self.config.effective_scope().needs_template_analysis() {
            // Thread the artifact's own base-authoritative source +
            // SFC parse: the snapshot above came from this artifact,
            // so the template derives from the SAME content — and an
            // artifact-only canonical (scheduler-missed) computes with
            // zero extra reads instead of losing the template. The
            // serve's publication status flows with the inputs; the
            // computed template serves THIS caller only — with no
            // scheduler node generation to attest, the
            // `derived_raw_cache` persist declines (fenced or not).
            // Build the inputs unconditionally — `compute_template_analysis_if_missing`
            // gates internally on the file's registered carrier compiler
            // (registry-dispatched, Svelte-capable), so a `.svelte` owner ingests
            // its template the same as a `.vue` one and a carrier-less file is a
            // no-op there. No hardcoded `.vue` extension gate.
            let template_inputs = crate::types::VueTemplateInputs {
                source: Arc::clone(&facts.raw_source),
                framework_parse: facts.framework_parse.clone(),
                store_published: serve.store_published,
                // Artifact-serve read — no scheduler node
                // generation to attest; the template serves
                // this caller, the persist declines (an entry
                // without a rail cannot be validated by the
                // scheduler-backed readers).
                source_generation: None,
            };
            self.compute_template_analysis_if_missing(canonical, &mut snapshot, template_inputs);
        }
        component_meta_trace_custom!(
            "get_raw_analysis_snapshot_result",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={} source=indexed_ready",
                canonical,
                snapshot.imports.len(),
                snapshot.macros.len(),
                snapshot.bindings.len(),
                snapshot.template.is_some(),
            ),
        );
        Some(snapshot)
    }

    /// Convenience wrapper that builds an owned `HostStoreView` once.
    /// Hot-path callers thread their view through
    /// [`Self::try_get_cached_resolved_meta_with_store_view`] directly.
    #[allow(dead_code)]
    pub(crate) fn try_get_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        self.try_get_cached_resolved_meta_for_view_fingerprint(canonical, mode, 0)
    }

    /// View-aware variant of [`Self::try_get_cached_resolved_meta`].
    ///
    /// The hot-path trait callers in `component_meta_request_impl.rs`
    /// route here with the request-bound `&HostStoreView` they already
    /// hold, avoiding the full-workspace snapshot rebuild on each first
    /// warm read.
    #[inline]
    pub(crate) fn try_get_cached_resolved_meta_with_store_view(
        &self,
        view: &crate::resolver_store::HostStoreView,
        canonical: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        self.try_get_cached_resolved_meta_for_view_fingerprint_with_store_view(
            view, canonical, mode, 0,
        )
    }

    /// View-fingerprint-aware variant of [`Self::try_get_cached_resolved_meta`].
    ///
    /// Overlay-bearing callers thread their view's fingerprint here so
    /// the singleflight cache lookup hits the per-view slot rather
    /// than the base host's slot. Base-only callers pass `0` and the
    /// behaviour matches the historical entry point.
    ///
    /// TOP-LEVEL convenience wrapper: reads the host store view as a typed
    /// [`crate::resolver_store::StoreViewRead`] and validates the cached
    /// resolved-meta against the view directly, returning it to the caller
    /// with NO outer publish / is_stable fence. It therefore serves a warm
    /// hit ONLY against a proven-`Current` view; a known-stale
    /// `StoreViewRead::ReturnOnly` read misses to cold (`None`). The
    /// hot-path trait callers in `component_meta_request_impl.rs` do NOT
    /// route through here — they hold a request-bound (cold-seed) view and
    /// call the `_with_store_view` variant directly, where the request
    /// driver's own currentness gate + `is_stable` fence govern.
    #[allow(dead_code)]
    pub(crate) fn try_get_cached_resolved_meta_for_view_fingerprint(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        view_fingerprint: u64,
    ) -> Option<ResolvedComponentMetaState> {
        let current_view = self.resolver_store_view_read().current()?;
        self.try_get_cached_resolved_meta_for_view_fingerprint_with_store_view(
            current_view.view(),
            canonical,
            mode,
            view_fingerprint,
        )
    }

    /// View-aware implementation behind
    /// [`Self::try_get_cached_resolved_meta_for_view_fingerprint`].
    ///
    /// NESTED-cold validator: the production callers are the
    /// `try_get_cached_component_meta` trait impls in
    /// `component_meta_request_impl.rs`, reached only as the request
    /// driver's pre-flight `try_get_cached` peek — which `run_stable_request`
    /// gates on `snapshot_view_is_current()` (a `StoreViewRead::ReturnOnly`
    /// snapshot suppresses the warm peek). The driver's `is_stable` fence
    /// re-checks the external-supersession token before promoting. The view
    /// is therefore the request-bound cold-seed `HostStoreView` (raw, not
    /// `Current`) by design; the outer fence — not this validator — owns
    /// currentness. The `Current`-only top-level wrapper above is the entry
    /// point for callers WITHOUT that fence.
    pub(crate) fn try_get_cached_resolved_meta_for_view_fingerprint_with_store_view(
        &self,
        view: &crate::resolver_store::HostStoreView,
        canonical: &str,
        mode: ProjectionMode,
        view_fingerprint: u64,
    ) -> Option<ResolvedComponentMetaState> {
        let cache_key = crate::host_manage::component_meta_request_impl::resolved_meta_cache_key_with_view_fingerprint(
            canonical,
            mode,
            view_fingerprint,
        );
        if let Some(cached) = self
            .resolver_runtime()
            .component_meta
            .get_if_valid(&cache_key, view)
        {
            if cached.completeness.is_partial() {
                return None;
            }
            self.mirror_cached_resolved_meta_arc(canonical, mode, view_fingerprint, cached.clone());
            return Some(cached.as_ref().clone());
        }

        // View-aware legacy fallback: the slot is keyed by
        // `(mode, view_fingerprint)` so an overlay-bearing reader
        // (view_fingerprint != 0) hits its own per-overlay slot, and
        // a base reader (view_fingerprint == 0) cannot observe an
        // overlay-derived entry.
        // cached_resolved_meta lives on DerivedRawState (D48 split).
        use crate::resolver_core::StoreView;
        let entry = self.derived_raw_cache().get(canonical)?;
        let cached = entry.cached_resolved_meta.get(&(mode, view_fingerprint))?;
        if cached.state.completeness.is_partial() {
            return None;
        }
        // R3/R26/R28: dispatch through
        // `StoreView::validates_fact_signature` as a per-domain
        // override hook. The default impl in `resolver_core/mod.rs`
        // walks the signature via `.iter().all(self.validates(..))`,
        // so the live behavior is the same as the legacy per-item
        // form; the dispatch point exists so future per-domain
        // implementers can short-circuit on the first mismatch
        // without changing call sites. Re-emit the structured trace
        // via `invalid_fact_details` only when validation fails so
        // the observability path still surfaces the offending facts.
        if !view.validates_fact_signature(&cached.fact_versions) {
            let invalid_details = view.invalid_fact_details(&cached.fact_versions, 6);
            component_meta_trace_custom!(
                "try_get_cached_component_meta_invalid",
                format!(
                    "owner={} mode={mode:?} cache=legacy facts={} invalid={} details=[{}]",
                    canonical,
                    cached.fact_versions.len(),
                    invalid_details.len(),
                    invalid_details.join(" | "),
                ),
            );
            return None;
        }
        // Strict admission. The cached state's fact_versions are
        // populated at cold-compute publish time; re-hydration here
        // passes them through unchanged. Empty signatures skip
        // admission rather than caching a phantom-fact entry — the
        // cached state is still returned.
        if !cached.fact_versions.is_empty() {
            self.resolver_runtime().component_meta.insert_arc_with_kind(
                cache_key,
                cached.state.clone(),
                cached.fact_versions.to_vec(),
                "component_meta.results",
            );
        }
        Some(cached.state.as_ref().clone())
    }

    pub(crate) fn store_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        state: &ResolvedComponentMetaState,
        fact_versions: &[crate::resolver_core::FactVersionRef],
    ) {
        self.store_cached_resolved_meta_for_view_fingerprint(
            canonical,
            mode,
            state,
            fact_versions,
            0,
        );
    }

    /// View-fingerprint-aware variant of [`Self::store_cached_resolved_meta`].
    pub(crate) fn store_cached_resolved_meta_for_view_fingerprint(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        state: &ResolvedComponentMetaState,
        fact_versions: &[crate::resolver_core::FactVersionRef],
        view_fingerprint: u64,
    ) {
        if state.completeness.is_partial() {
            return;
        }
        component_meta_trace_custom!(
            "store_cached_component_meta_result",
            format!(
                "owner={} mode={mode:?} facts={} macros={} resolved_types={} has_evaluated_types={}",
                canonical,
                fact_versions.len(),
                state.resolved_macros.len(),
                state.resolved_type_registry.len(),
                state.evaluated_types.is_some(),
            ),
        );
        let state = Arc::new(state.clone());
        // Drop the owner's own non-round-tripping `DerivedFactHash{Route}`
        // fact before admission, mirroring the `ComponentMetaResultDb`
        // publish path (see `component_meta_entry::strip_owner_route_fact`).
        // The owner Route hash can change as the owner's own indexed
        // surface refreshes mid-request, so it does not round-trip warm
        // validation; under concurrency a straggler false-misses on it and
        // re-leads as a second cold `Flight::Leader`. The owner
        // `FileWholeHash` fact (retained) already covers owner-content
        // edits, and cross-file route facts (retained) gate dep edits.
        let admitted = crate::host_manage::component_meta_entry::strip_owner_route_fact(
            canonical,
            fact_versions,
        );
        // Strict admission. Cold-publish path: empty signatures
        // are skipped (the publish caller passes an empty slice
        // when the cold compute didn't observe any facts — strict
        // admission would refuse and emit
        // `FactSignatureAdmissionRefused`, which would inflate the
        // refused counter on the steady-state baseline).
        if !admitted.is_empty() {
            self.resolver_runtime().component_meta.insert_arc_with_kind(
                crate::host_manage::component_meta_request_impl::resolved_meta_cache_key_with_view_fingerprint(
                    canonical,
                    mode,
                    view_fingerprint,
                ),
                state.clone(),
                admitted.to_vec(),
                "component_meta.results",
            );
        }
        // View-aware mirror: the legacy `cached_resolved_meta` slot is
        // keyed by `(ProjectionMode, view_fingerprint)` so overlay-
        // bearing publishers (`view_fingerprint != 0`) cannot
        // overwrite the base slot. A later base `resolve_component_meta`
        // (view_fingerprint == 0) does NOT fall through to an overlay-
        // derived entry.
        self.mirror_cached_resolved_meta_arc(canonical, mode, view_fingerprint, state);
    }

    pub(crate) fn mirror_cached_resolved_meta_arc(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        view_fingerprint: u64,
        state: Arc<ResolvedComponentMetaState>,
    ) {
        if state.completeness.is_partial() {
            return;
        }
        // R3/R26/R28: capture the resolved state's observed fact set
        // as an `Arc<[FactVersionRef]>` so the wrapper's warm-hit
        // validator can clone the handle without copying the slice.
        let full_fact_versions: Arc<[crate::resolver_core::FactVersionRef]> =
            Arc::from(state.fact_versions.clone().into_boxed_slice());
        // Fan-out to any active outer fact-tracer scope so transitive
        // CROSS-FILE observations bubble through the mirror site. The
        // outer cold-meta tracer (`with_fact_tracer` at
        // `component_meta_entry.rs`) captures the union and sources
        // the published `ComponentMetaResultEntry` signature from it.
        //
        // The owner's OWN facts are excluded from the fan-out. The
        // owner's content is already observed by the cold compute's
        // dispatch reads and gated by the result cache's legacy
        // whole-hash rail; bubbling them is redundant. Critically,
        // `state.fact_versions` carries the curated
        // `DerivedFactHash{owner, Route}` entry from
        // `current_dependency_fact_versions` — the owner's export
        // route is NOT a dependency of the owner's own component-meta
        // result, and its hash is dual-sourced on
        // `HostStoreView::derived_hashes` (see `resolver_store.rs`
        // `HostStoreView::build`), so it does not round-trip on warm
        // validation. Excluding owner-scoped facts from the tracer
        // fan-out keeps that non-dependency noise out of the
        // tracer-owned signature.
        let cross_file_facts: Vec<crate::resolver_core::FactVersionRef> = full_fact_versions
            .iter()
            .filter(|fact| fact.canonical_id() != Some(canonical))
            .cloned()
            .collect();
        crate::fact_signature_helpers::observe_fact_signature(&cross_file_facts);
        // Drop the owner's own non-round-tripping `DerivedFactHash{Route}`
        // fact from the STORED signature, mirroring the validated cache
        // above and the `ComponentMetaResultDb` publish path (see
        // `component_meta_entry::strip_owner_route_fact`). The owner
        // `FileWholeHash` fact is retained, so `cached_resolved_meta` warm
        // validation still rejects on owner-content edits; cross-file
        // route facts are retained, so dep edits still invalidate. Only
        // the owner self-Route fact — which a later warm-validation view
        // may source differently (or not at all) — is removed. Idempotent:
        // a warm-hit re-mirror passes an already-stripped signature.
        let fact_versions = crate::host_manage::component_meta_entry::strip_owner_route_fact(
            canonical,
            &full_fact_versions,
        );
        let cached = crate::types::ResolvedComponentMetaCacheEntry {
            fact_versions,
            state,
        };

        // cached_resolved_meta lives on DerivedRawState (D48 split).
        // View-aware key: `(mode, view_fingerprint)` prevents an
        // overlay-bearing publisher (view_fingerprint != 0) from
        // overwriting the base slot (view_fingerprint == 0) and
        // contaminating a later base read.
        {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(canonical.to_string())
                .or_default();
            derived_ref
                .value_mut()
                .cached_resolved_meta
                .insert((mode, view_fingerprint), cached);
        }
    }

    // ───────────────────────────────────────────────────────────────────────
    // Encoded payload cache (shared by NAPI/WASM)
    // ───────────────────────────────────────────────────────────────────────

    /// Try to return a cached encoded payload for the canonical
    /// component-meta query, taking a FRESH store-view read internally.
    /// Validates the cached payload's fact versions against a
    /// PROVEN-`Current` store view.
    ///
    /// Reads the host store view as a typed
    /// [`crate::resolver_store::StoreViewRead`] and serves a warm hit only
    /// when the manager proved it current. A known-stale
    /// `StoreViewRead::ReturnOnly` read misses to cold (returns `None`).
    ///
    /// This is the NO-FIXED-VIEW accessor: it owns the store-view read for
    /// a caller that does NOT already hold a captured view. The `meta.rs`
    /// payload entry points capture ONE
    /// [`crate::resolver_store::BatchFixedView`] per batch / request and
    /// probe through the `_with_store_view` variant against that fixed
    /// view's proven-current snapshot instead (the O(N)→O(1) warm-batch
    /// read collapse), so they no longer route through this fresh-read
    /// wrapper. It is
    /// retained as the canonical no-view peek and is locked by the
    /// `ReturnOnly`-suppression soundness test
    /// (`warm_meta_payload_hit_is_suppressed_when_store_view_is_not_current`),
    /// whose stale-view miss path cannot be expressed against the
    /// `&CurrentHostStoreView`-only `_with_store_view` variant.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn try_get_cached_meta_payload(&self, canonical: &str) -> Option<Vec<u8>> {
        // TOP-LEVEL warm validator: the encoded-payload peek returns the
        // cached payload directly to the FFI consumer with NO outer
        // publish / is_stable fence. It MUST validate against a
        // proven-`Current` view: a known-stale `StoreViewRead::ReturnOnly`
        // snapshot (the manager could not prove the view current under
        // sustained churn) would validate a cached payload's
        // `fact_versions` against already-mutated dependency state
        // (`old == old`) and hand the FFI consumer a stale full-meta
        // payload. On a non-current read, miss to cold (return `None`):
        // the caller falls through to `resolve_component_meta`, whose own
        // request-driver `is_stable` / publish fence gates promotion.
        let current_view = self.resolver_store_view_read().current()?;
        self.try_get_cached_meta_payload_with_store_view(&current_view, canonical)
    }

    /// View-aware implementation behind [`Self::try_get_cached_meta_payload`].
    ///
    /// Accepts ONLY a
    /// [`crate::resolver_store::CurrentHostStoreView`] — the
    /// `StoreViewManager`'s type-level proof that the view was published
    /// under a live-matching token. A known-stale
    /// `StoreViewRead::ReturnOnly` snapshot CANNOT reach this validator by
    /// construction, so it can never false-positive a superseded encoded
    /// payload against an already-mutated dependency.
    pub(crate) fn try_get_cached_meta_payload_with_store_view(
        &self,
        current_view: &crate::resolver_store::CurrentHostStoreView,
        canonical: &str,
    ) -> Option<Vec<u8>> {
        use crate::resolver_core::StoreView;
        let view = current_view.view();
        // cached_meta_payload lives on DerivedRawState (D48 split).
        let entry = self.derived_raw_cache().get(canonical)?;
        let cached = entry.cached_meta_payload.as_ref()?;
        // Value-side generation backstop (the typed result caches'
        // `validated_at_generation` discipline): an under-recorded
        // signature — the empty signature validates trivially — must not
        // keep validating across project-shape mutations, so a warm hit
        // demands the LIVE project generation.
        if cached.validated_at_generation != self.project_type_store.current_project_generation() {
            return None;
        }
        // R3/R26/R28 fast-path: dispatch through
        // `StoreView::validates_fact_signature` so per-domain validators
        // can short-circuit on the first mismatch. Empty signatures
        // trivially validate per the default-impl contract.
        if view.validates_fact_signature(&cached.fact_versions) {
            return Some(cached.payload.clone());
        }
        None
    }

    /// Store an encoded payload in the per-file cache.
    ///
    /// `validated_at_generation` is the FLIGHT-CAPTURED project
    /// generation — the value snapshotted when the producing view was
    /// taken (the `BatchFixedView`'s captured token), NOT a live
    /// re-read. Reading `current_project_generation()` here would let a
    /// project bump landing in the admission-fence→store window stamp a
    /// payload computed under the OLD graph with the NEW generation —
    /// permanently defeating the generation backstop for exactly the
    /// under-recorded/empty-signature case it exists for (the same
    /// flight-captured-stamp discipline as the `IndexedReady` publish).
    pub(crate) fn store_meta_payload(
        &self,
        canonical: &str,
        fact_versions: &[crate::resolver_core::FactVersionRef],
        payload: Vec<u8>,
        validated_at_generation: u64,
    ) {
        // R3/R26/R28: stash the observed fact signature as an
        // `Arc<[FactVersionRef]>` so warm-hit validation clones a
        // cheap handle.
        let fact_versions: Arc<[crate::resolver_core::FactVersionRef]> =
            Arc::from(fact_versions.to_vec().into_boxed_slice());
        // Fan-out to outer active tracers so the encoded-payload
        // mirror participates in transitive fact bubbling. Empty
        // signatures are a no-op per `observe_fact_signature`.
        crate::fact_signature_helpers::observe_fact_signature(&fact_versions);
        let cached = crate::types::CachedMetaPayload {
            fact_versions,
            payload,
            validated_at_generation,
        };

        // cached_meta_payload lives on DerivedRawState (D48 split).
        {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(canonical.to_string())
                .or_default();
            derived_ref.value_mut().cached_meta_payload = Some(cached);
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        self.append_dependency_fact_versions(canonical, &mut facts, &mut seen);
        for dep in tracked_deps {
            self.append_dependency_fact_versions(dep.as_str(), &mut facts, &mut seen);
        }

        facts
    }

    #[cfg(test)]
    pub(crate) fn fact_versions_match(
        &self,
        fact_versions: &[crate::resolver_core::FactVersionRef],
    ) -> bool {
        // Test-only warm-validation helper: validate against a
        // proven-`CurrentHostStoreView`. A known-stale (`ReturnOnly`) read
        // is treated as "no match" — the same miss-to-cold semantics the
        // production warm validators apply.
        let Some(current) = self.resolver_store_view_read().current() else {
            return false;
        };
        let view = current.view();
        fact_versions
            .iter()
            .all(|fact| crate::resolver_core::StoreView::validates(view, fact))
    }

    pub(crate) fn append_dependency_fact_versions(
        &self,
        canonical: &str,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
    ) {
        if let Some(hash) = self.current_or_read_whole_hash(canonical) {
            let file_fact = crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_string(),
                hash,
            };
            if seen.insert(file_fact.clone()) {
                facts.push(file_fact);
            }
        }

        for kind in [
            crate::resolver_core::DerivedFactKind::Route,
            crate::resolver_core::DerivedFactKind::ImportRoute,
        ] {
            if let Some(hash) = self.current_derived_fact_hash(canonical, kind) {
                let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                    canonical_id: canonical.to_string(),
                    kind,
                    hash,
                };
                if seen.insert(fact.clone()) {
                    facts.push(fact);
                }
            }
        }

        // Legacy barrel generation facts removed — provider route cache
        // invalidates via shallow module surface hashes.
    }

    pub(crate) fn current_derived_fact_hash(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<Hash16> {
        match kind {
            crate::resolver_core::DerivedFactKind::DirectSource => {
                self.current_or_read_whole_hash(canonical_id)
            }
            crate::resolver_core::DerivedFactKind::Route => {
                // Fact capture is OBSERVE-ONLY: it must never
                // materialise, refresh, or publish — a capture that
                // cold-builds breadth-walks every unrelated import of
                // the owner just to sign a result. So this reads through
                // `observe_content_pinned_indexed` (content-pinned, NO
                // re-index arm) and DECLINES on anything it cannot
                // observe as current:
                //
                // - NEVER-MATERIALISED canonical → `None`. The
                //   `FileWholeHash` fact (more sensitive on the owner's
                //   own content: any content change invalidates) is
                //   always captured alongside and covers invalidation
                //   until the canonical's first traversal materialises
                //   its route surface.
                // - STALE surface (edge generation / project stamp
                //   moved, or only a non-current content candidate
                //   exists) → `None`, so a dependent entry rooted on the
                //   stale `Route` fact fails warm validation and
                //   recomputes against the live state — without this
                //   read rebuilding anything itself.
                //
                // The lookup is content-pinned: a permissive `get_any`
                // would let a stale `IndexedReady` surface its old
                // `route_hash` as the "current" Route fact, confirming a
                // stale dependent cache entry as valid.
                let indexed = self.observe_content_pinned_indexed(canonical_id)?;
                if !self.indexed_surface_is_current(canonical_id, &indexed) {
                    return None;
                }
                indexed.route_hash
            }
            crate::resolver_core::DerivedFactKind::ImportRoute => {
                // Read-only: ImportRoute fact capture must not promote a
                // shallow-only tracked dependency into full IndexedReady.
                self.current_cached_import_route_hash(canonical_id)
            }
        }
    }

    pub(crate) fn current_cached_import_route_hash(&self, canonical_id: &str) -> Option<Hash16> {
        self.generation_current_import_route_hash(canonical_id)
    }
}

// ===========================================================================
// Pick member-route callable-descent predicates (module scope)
//
// These predicates are consumed by the Pick member-route materialiser in
// `append_component_meta_registry_entries` (via method-local bindings that
// forward here). They live at module scope so they are directly
// unit-testable — see the `callable_descent_predicate_tests` module.
//
// All three operate on RAW prepared-decl bodies (analyzer IR), where a
// bare constructor type (`new (...) => R`) is still present un-collapsed.
// A constructor type carries the SAME `FunctionExpr` payload as a function
// type, so both predicates must treat `TypeExpr::ConstructorType` exactly
// like `TypeExpr::Function`.
// ===========================================================================
