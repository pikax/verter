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
use crate::meta_resolve::component_meta_registry_prefers_structural_materialization;
use crate::meta_resolve::slot_binding_graph;
use crate::meta_resolve::STORE_VIEW_STABILITY_MAX_ATTEMPTS;
use crate::meta_resolve::{
    collect_define_props_root_names, slot_binding_targets_define_props_root,
    RegistryMaterialization, ResolvedComponentMetaState,
};
use crate::meta_resolve::{
    collect_type_expr_ref_names, lowered_preserve_package_backed_symbolic_refs,
    materialize_component_meta_type_expr_until_stable,
};
use crate::meta_resolve::{
    drain_dispatch_dep_signature_accumulator, reset_dispatch_dep_signature_accumulator,
};
use crate::meta_resolve::{
    next_component_meta_audit_request_id, request_source_performed_compute,
    should_skip_imported_registry_seed_refresh, trace_request_source, CapturedComponentMetaInputs,
    ResolvedComponentMetaComputeAudit, ResolvedTypeRegistryMeta,
};
use crate::meta_resolve::{
    pick_via_dispatch_pick_helper, project_expr_class_a_via_dispatch,
    project_expr_class_a_via_dispatch_threaded, project_type_surface_expr_via_host_threaded,
};

// Items that live in the parent shell (`crate::meta_resolve`): the
// registry-structural materialiser, the registry-route preservers, the
// graph-native registry-route + cycle-BFS predicates, and the
// origin-graph builder. The `HostComponentMetaResolver` adapter lives
// in `host_manage/jsdoc_resolve.rs` (host-impl tier).
use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
use crate::meta_resolve::build_origin_graph;
use crate::meta_resolve::{
    component_meta_registry_prefers_structural_materialization_node,
    component_meta_registry_should_keep_raw_symbolic_non_object_alias,
    materialize_component_meta_registry_structural_expr, preserve_nested_symbolic_member_routes,
    preserve_registry_callable_param_member_routes,
    type_expr_needs_nested_symbolic_route_preservation,
};

use crate::resolver_core::component_meta_registry::{
    collect_component_meta_registry_public_field_refs, collect_component_meta_registry_refs,
    component_meta_registry_expr_references_name,
    component_meta_registry_has_explicit_object_surface,
    component_meta_registry_has_non_object_top_level_surface,
    component_meta_registry_has_unmerged_heritage_intersection,
    component_meta_registry_raw_member_path_surface, component_meta_registry_ref_name,
    enqueue_component_meta_registry_ref, merge_component_meta_registry_candidates,
    owner_component_meta_registry_import_root, upsert_component_meta_registry_entry,
    PendingComponentMetaRegistryRef,
};

impl VerterHost {
    /// Single host-backed resolver API for cross-file component-meta enrichment.
    ///
    /// This is the ONLY entry point for cross-file component-meta resolution.
    /// Mode is chosen explicitly by callers â€” never inferred.
    ///
    /// - `Type`: resolves symbol identity, canonical location, and attached JSDoc
    ///   without materializing expanded shapes.
    /// - `Expanded`: resolves the same way, then materializes props/emits/slots,
    ///   populates the type registry, and computes evaluated types.
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
        let _ctx_guard = if crate::request_context::current_request_context().is_none() {
            Some(crate::request_context::RequestContextGuard::install(
                crate::request_context::RequestContext::with_kind_timing_and_projection_budget(
                    next_component_meta_audit_request_id(),
                    std::sync::Arc::<str>::from(canonical.as_str()),
                    verter_audit::RequestKind::ComponentMeta,
                    false,
                    self.config.audit_timing_capture && self.config.audit_enabled,
                    None,
                    self.config.projection_op_budget,
                ),
            ))
        } else {
            None
        };
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
    /// (itself `#[cfg(any(test, debug_assertions))]`-gated), so the
    /// release-build crate drops the helper + this wrapper entirely.
    #[cfg(any(test, debug_assertions))]
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

    /// Bare-host overlay-aware cold compute entry.
    ///
    /// Routes the bare-host (no [`SessionView`](crate::session_view::SessionView))
    /// cold-compute through a
    /// [`HostResolverContext`](crate::resolver_core::HostResolverContext)
    /// rooted on `overlay`, so the resolver-tier reads inside
    /// [`Self::compute_component_meta_state_inner`] observe canonicals
    /// promoted by mid-request `ensure_loaded` /
    /// `ensure_indexed_ready` through the shared overlay.
    ///
    /// Used by
    /// [`SessionRequestHost::compute_component_meta`](crate::host_manage::component_meta_request_impl::SessionRequestHost).
    /// The shared overlay lets cold compute calls accumulate across the
    /// request rather than each paying the per-call workspace sweep cost
    /// with no cross-call accumulation benefit.
    pub(crate) fn compute_component_meta_state_with_overlay(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
    ) -> Option<ResolvedComponentMetaState> {
        // Fenced cold compute (driver `run_stable_request` `compute`, gated
        // by `is_stable`): seed from a FRESH base read whose currentness is
        // INTRINSIC to the seed (`into_cold_seed_view` carries the read's
        // `Current` / `ReturnOnly` arm). A `ReturnOnly` seed makes the
        // derived `HostResolverContext` fail its nested warm-cache probes
        // closed; there is no separate currentness flag to mismatch with the
        // view.
        let cold_seed = self.resolver_store_view_read().into_cold_seed_view();
        self.compute_component_meta_state_with_cold_seed_arg(
            canonical, mode, whole_hash, &cold_seed, overlay,
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

    /// Captured-inputs variant of
    /// [`Self::compute_component_meta_state_with_overlay`] (same routing).
    pub(crate) fn compute_component_meta_state_from_captured_with_overlay(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        captured: &CapturedComponentMetaInputs,
        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
    ) -> Option<ResolvedComponentMetaState> {
        // Fenced cold compute (driver `run_stable_request` `compute`, gated
        // by `is_stable`): seed from a FRESH base read whose currentness is
        // INTRINSIC to the seed (see
        // [`Self::compute_component_meta_state_with_overlay`]). No separate
        // flag to mismatch with the view.
        let cold_seed = self.resolver_store_view_read().into_cold_seed_view();
        self.compute_component_meta_state_from_captured_with_cold_seed_arg(
            canonical, mode, captured, &cold_seed, overlay,
        )
    }

    /// View-bearing variant of
    /// [`Self::compute_component_meta_state_from_captured_with_overlay`].
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
        // `overlay_priority::ensure_indexed_ready_with_view`): a
        // base-only view, or an overlay view for which this canonical
        // is unmasked, reports `None` here and correctly delegates to
        // the base capture path — the overlay-materialiser snapshot
        // path is reserved for genuine overlays. The base host's
        // scheduler stays untouched (R17).
        let overlay_facts = if view.overlay_content_hash_for(canonical).is_some() {
            self.materialize_overlay_indexed_ready_with_view(canonical, view)
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
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            let whole_hash = facts.whole_hash;
            let store_read_ms = store_read_started
                .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            let owner_eval_source = VerterHost::build_eval_script_source(
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
        // `#[cfg(any(test, debug_assertions))]`-gated.
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
                    // Per-macro projectors are the sole component-meta
                    // resolution path. Each projector dispatches
                    // `ResolveMacroPayload` + empty-path Shallow
                    // `ProjectPath`, raises surface members, runs the
                    // bounded fixed-point reducer
                    // (`materialize_component_meta_type_expr_until_stable`)
                    // so nested operator chains collapse to concrete
                    // leaves before publication, and writes
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
                    // built from the context-aware `vue_macro_dtos` member
                    // authority + the flat fields `project_evaluated_types` just
                    // projected, with NO solver / reparse / eval_source fallback.
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
            || crate::request_context::current_materialization_cache_suppress();

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
        fn imported_registry_alias_should_stay_symbolic(expr: &verter_type_expr::TypeExpr) -> bool {
            use verter_type_expr::TypeExpr;

            match expr {
                TypeExpr::Parenthesized(inner) => {
                    imported_registry_alias_should_stay_symbolic(inner)
                }
                TypeExpr::Mapped { .. }
                | TypeExpr::Conditional { .. }
                | TypeExpr::IndexedAccess { .. }
                | TypeExpr::TypeOf(_) => true,
                _ => false,
            }
        }
        /// Owner-local generic-alias registry substitution via the shared
        /// dispatch `Instantiate` query (Navigate mode).
        ///
        /// When a registry candidate's raw body is an owner-local generic
        /// alias ref (`Button = ComponentConfig<typeof theme>`, where
        /// `ComponentConfig` is declared in the SAME canonical file), the
        /// registry publishes Button as the SHALLOW substituted body —
        /// helper-ref members (`variants: ComponentVariants<T>`) stay as
        /// carrier Refs with their `T` argument substituted, rather than
        /// recursively expanding them. This keeps the registry consumer's
        /// Ref-to-helper navigation path queryable.
        ///
        /// This is the SOLE owner-local generic-alias substitution path:
        /// it lowers the generic ref to the graph's `InstantiationRef`
        /// carrier, gates on owner-local identity + the prepared-decl reach
        /// constraints, then runs the shared `SemanticQueryKey::Instantiate`
        /// query and raises the instantiated body. Both the lowering and
        /// the instantiation run in `structural_transit_with_mode(Navigate)`
        /// so nested helper refs (e.g. `ComponentVariants<T>`) stay as
        /// `Ref` carriers (Navigate) without publication-demand operator
        /// reification (StructuralTransit) and without over-materializing
        /// helpers (which Expanded would do, breaking the
        /// `expanded_instantiate_calls` audit invariant).
        ///
        /// Returns `None` (caller falls through to the non-early-return
        /// path) when the raw body is not a generic Ref, the lowering does
        /// not yield an `InstantiationRef`, the base is cross-file, the
        /// prepared decl is missing / has fewer type parameters than bound
        /// arguments / has a non-Object body, or the instantiated body does
        /// not raise to an Object.
        fn owner_local_generic_alias_substituted_body_via_dispatch(
            query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            raw_body: Option<&verter_type_expr::TypeExpr>,
        ) -> Option<verter_type_expr::TypeExpr> {
            use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
            use crate::semantic_query::{
                ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticQueryApi,
                SemanticQueryKey,
            };
            use verter_type_expr::TypeExpr;

            // Step 2: require a generic Ref with non-empty type arguments.
            let raw = raw_body?;
            let TypeExpr::Ref { type_arguments, .. } = raw else {
                return None;
            };
            if type_arguments.is_empty() {
                return None;
            }

            // The shared `&dyn ResolverContext` reference is copied out so
            // the dispatch (which only borrows the context) and the
            // mutable `prepared_type_decl` read on `query_engine` are
            // independent borrows.
            let ctx = query_engine.ctx;
            let dispatch = ProjectSemanticDispatch::new(ctx);

            // Step 3: lower the generic ref in Navigate mode. A generic
            // `Ref` lowers to the graph's `InstantiationRef` carrier
            // (base + lowered/substituted args).
            let navigate_context =
                ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
            let lowered = dispatch.lower_type_expr_in_scope_with_context(
                scope_canonical_id,
                raw,
                navigate_context,
            )?;

            // Step 4: require an `InstantiationRef`. Anything else (a bare
            // DeclRef, a reduced Object, a miss shell, ...) is NOT an
            // owner-local generic instantiation — fall through.
            let lowered_data = node_data_for(ctx, lowered)?;
            let SemanticNodeData::InstantiationRef { base, args } = lowered_data.as_ref() else {
                return None;
            };

            // Step 5: owner-local guard — the instantiated declaration must
            // live in the requesting scope's file.
            if base.canonical_id.as_ref() != scope_canonical_id {
                return None;
            }

            // Step 6: preserve the old reach constraints — the prepared
            // decl must exist, declare at least as many type parameters as
            // the bound arguments, and have an Object body. (Reading the
            // prepared decl here mirrors the slow lane's gate; the
            // dispatch's `Instantiate` substitution operates on the same
            // prepared body.)
            let prepared = query_engine
                .prepared_type_decl(base.canonical_id.as_ref(), base.decl_name.as_ref())?;
            if prepared.type_parameters.len() < args.len() {
                return None;
            }
            if !matches!(prepared.body, TypeExpr::Object(_)) {
                return None;
            }

            // Step 7: execute the shared `Instantiate` query in the same
            // Navigate transit context so the substituted body materializes
            // through the one engine.
            let instantiate_prc =
                ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
            let node = match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
                base: dispatch
                    .type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name)),
                context: dispatch.instantiate_context_for(&base.canonical_id, instantiate_prc),
                args: std::sync::Arc::clone(args),
            }) {
                crate::semantic_query::QueryResult::Value(
                    crate::semantic_query::SemanticQueryOutput { value: node, .. },
                ) => node,
                crate::semantic_query::QueryResult::Recursive(_)
                | crate::semantic_query::QueryResult::Error(_) => return None,
            };

            // Step 8: raise the instantiated body; publish only when it is
            // an Object (the shallow substituted surface).
            let raised = dispatch.raise_node_to_type_expr(node)?;
            matches!(raised, TypeExpr::Object(_)).then_some(raised)
        }
        fn materialize_component_meta_registry_candidate(
            query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            symbol_name: &str,
            raw_body: Option<&verter_type_expr::TypeExpr>,
            prefer_explicit_raw_surface: bool,
        ) -> Option<verter_type_expr::TypeExpr> {
            use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

            let imported_generic_alias_scope: Option<String> = raw_body.and_then(|expr| {
                let TypeExpr::Ref {
                    name,
                    type_arguments,
                } = expr
                else {
                    return None;
                };
                if type_arguments.is_empty() {
                    return None;
                }
                let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
                if !declaration.canonical_source.is_empty()
                    && declaration.canonical_source != scope_canonical_id
                {
                    Some(declaration.canonical_source.clone())
                } else {
                    None
                }
            });
            let imported_generic_alias_root = imported_generic_alias_scope.is_some();

            // Owner-local generic Refs preserve helper-Ref structure.
            // When `Button = ComponentConfig<typeof theme>` is
            // declared in the SAME file as `ComponentConfig` (owner-
            // local), the registry publishes Button as the SHALLOW
            // substituted body — `{ variants: ComponentVariants<...>,
            // slots: ComponentSlots<...>, ui: ComponentUI<...> }` —
            // rather than fully materialising every helper. This
            // keeps the registry consumer's Ref-to-helper navigation
            // path queryable rather than collapsing helper identities
            // into
            // their concrete shapes.
            //
            // Distinct from the imported-alias path
            // (`maybe_refine_imported_generic_alias_object` above) which
            // DOES materialise cross-file aliases (because the consumer
            // can't follow Refs to a cross-file helper through the
            // registry directly).
            if !imported_generic_alias_root {
                if let Some(shallow) = owner_local_generic_alias_substituted_body_via_dispatch(
                    query_engine,
                    scope_canonical_id,
                    raw_body,
                ) {
                    return Some(shallow);
                }
            }

            let maybe_refine_imported_generic_alias_object =
                |candidate: TypeExpr,
                 query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>| {
                    if !imported_generic_alias_root {
                        return candidate;
                    }
                    // `TypeExpr` implements `Drop`, so `object` cannot be
                    // moved out of `candidate`; borrow it and return the
                    // whole `candidate` by move when it is not an object.
                    let TypeExpr::Object(object) = &candidate else {
                        return candidate;
                    };
                    let properties = object
                        .properties
                        .iter()
                        .map(|member| match member {
                            ObjectMember::Property(property) => {
                                // Owner-scope TYPED projection first:
                                // `Ref{symbol}["<property>"]` binds fully in
                                // the OWNER scope (the alias and its
                                // substituted argument references are
                                // owner-file names), and the shared dispatch
                                // resolves every nested reference in its own
                                // defining file through per-file name
                                // resolution. A raised property value cannot
                                // be re-lowered in any ONE scope — it mixes
                                // the alias body's helper names (defining
                                // file) with substituted argument names
                                // (owner file), and an argument reference
                                // that fails to bind leaves its conditional
                                // contribution silently undecided. Same
                                // route shape as the registry member-path
                                // route below.
                                let route_expr = build_registry_indexed_access_expr(
                                    symbol_name,
                                    std::slice::from_ref(&property.name),
                                );
                                let typed_projected = project_expr_class_a_via_dispatch_threaded(
                                    query_engine.ctx,
                                    Some(query_engine),
                                    scope_canonical_id,
                                    &route_expr,
                                )
                                .filter(|projected| {
                                    !crate::resolver_core::type_expr_contains_semantic_miss(
                                        projected,
                                    )
                                });
                                // One materialise + one Navigate-mode
                                // stabilisation per property, in the
                                // imported alias's DEFINING scope — the
                                // remaining carriers are the alias body's
                                // helper references, which resolve there.
                                // Publication demand is Navigate
                                // (shallow-by-default); the consumer
                                // re-resolves carriers on demand through the
                                // shared resolver, so no per-property
                                // re-solve or improvement pick runs here.
                                let materialize_scope = imported_generic_alias_scope
                                    .as_deref()
                                    .unwrap_or(scope_canonical_id);
                                let refine_input =
                                    typed_projected.as_ref().unwrap_or(&property.ty);
                                let materialized =
                                    query_engine.materialize_member_surface_expr(
                                        materialize_scope,
                                        refine_input,
                                        true,
                                    );
                                let stabilized =
                                    materialize_component_meta_type_expr_until_stable(
                                        &materialized,
                                        materialize_scope,
                                        crate::semantic_query::ProjectionMode::Navigate,
                                        query_engine,
                                    );
                                // No-poison: a property value raised from
                                // the shared graph can carry carriers whose
                                // declarations live in OTHER files; the
                                // single-scope re-materialisation misses on
                                // those by name. A reduction that
                                // INTRODUCED a semantic-miss sentinel the
                                // input did not carry is "no reduction
                                // available here" — keep the input carrier.
                                let ty = if crate::resolver_core::type_expr_contains_semantic_miss(
                                    &stabilized,
                                ) && !crate::resolver_core::type_expr_contains_semantic_miss(
                                    refine_input,
                                ) {
                                    refine_input.clone()
                                } else {
                                    stabilized
                                };
                                ObjectMember::Property(ObjectProperty::with_visibility(
                                    property.name.clone(),
                                    ty,
                                    property.optional,
                                    property.readonly,
                                    // The member is rebuilt with only its value
                                    // type changed: preserve its declared
                                    // accessibility AND its spans.
                                    property.visibility,
                                    property.spans,
                                ))
                            }
                            other => other.clone(),
                        })
                        .collect();
                    TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
                };

            // Prefer-raw applies only when the raw body already IS an
            // explicit one-level surface. An UN-MERGED heritage intersection
            // (`interface X extends Base { ... }` lowered as
            // `Intersection([Ref{Base}, Object{own}])`) is not — it must fall
            // through to the shared materialisation routes, which fold base +
            // own members into the heritage-merged shallow Object surface
            // through the one empty-path Shallow surface walker.
            if prefer_explicit_raw_surface
                && raw_body.is_some_and(|expr| {
                    component_meta_registry_has_explicit_object_surface(expr)
                        && !component_meta_registry_has_unmerged_heritage_intersection(expr)
                })
            {
                return raw_body.cloned().map(|candidate| {
                    maybe_refine_imported_generic_alias_object(candidate, query_engine)
                });
            }
            if raw_body.is_some_and(|expr| {
                component_meta_registry_has_non_object_top_level_surface(expr)
                    && component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                        expr,
                        scope_canonical_id,
                        query_engine,
                    )
            }) {
                return raw_body.cloned();
            }
            // Structural-materialisation preference is the graph-native
            // predicate: lower the raw TypeExpr to a Navigate-mode
            // SemanticNodeId and consult
            // `component_meta_registry_prefers_structural_materialization_node`.
            // Falls back to the TypeExpr predicate when lowering fails
            // (matches conservative "not structural" semantics when no
            // canonical node id exists).
            if let Some(raw) = raw_body.filter(|expr| {
                if !component_meta_registry_has_non_object_top_level_surface(expr) {
                    return false;
                }
                // An un-merged heritage intersection must NOT take the
                // structural route: the structural materialiser preserves the
                // compound's arm structure (it would publish
                // `Intersection([Object, Object])`), while the registry
                // contract for a heritage interface is the MERGED one-level
                // Object surface. Fall through to the symbol-rooted dispatch
                // bridge, whose compound-root composition runs the shared
                // empty-path Shallow surface walker (the one heritage-merge
                // engine).
                if component_meta_registry_has_unmerged_heritage_intersection(expr) {
                    return false;
                }
                let host = query_engine.ctx;
                let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
                if let Some(node) = dispatch.lower_type_expr_in_scope_with_context(
                    scope_canonical_id,
                    expr,
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        crate::semantic_query::ProjectionMode::Navigate,
                    ),
                ) {
                    let graph = host.project_type_store().semantic_graph();
                    component_meta_registry_prefers_structural_materialization_node(graph, node, 0)
                } else {
                    // Lowering failure — fall back to the TypeExpr
                    // predicate's classification. Preserves existing
                    // behaviour for shapes the dispatcher cannot lower
                    // (e.g., parser-only TypeExpr arms with no graph
                    // counterpart yet).
                    component_meta_registry_prefers_structural_materialization(expr)
                }
            }) {
                return Some(materialize_component_meta_registry_structural_expr(
                    raw,
                    scope_canonical_id,
                    query_engine,
                ));
            }
            // Bridge via per-engine helper.
            project_type_surface_expr_via_host_threaded(
                query_engine,
                scope_canonical_id,
                symbol_name,
            )
            .map(|materialized| {
                raw_body.map_or_else(
                    || materialized.clone(),
                    |raw| {
                        let preserved_package_refs = lowered_preserve_package_backed_symbolic_refs(
                            &materialized,
                            raw,
                            scope_canonical_id,
                            query_engine,
                        );
                        preserve_registry_callable_param_member_routes(&preserved_package_refs, raw)
                    },
                )
            })
            .map(|candidate| maybe_refine_imported_generic_alias_object(candidate, query_engine))
            .or_else(|| {
                raw_body.and_then(|expr| {
                    (!component_meta_registry_has_non_object_top_level_surface(expr)).then(|| {
                        maybe_refine_imported_generic_alias_object(expr.clone(), query_engine)
                    })
                })
            })
            .or_else(|| {
                raw_body.cloned().map(|candidate| {
                    maybe_refine_imported_generic_alias_object(candidate, query_engine)
                })
            })
        }
        fn build_registry_indexed_access_expr(
            symbol_name: &str,
            path: &[String],
        ) -> verter_type_expr::TypeExpr {
            path.iter().fold(
                verter_type_expr::TypeExpr::named(symbol_name),
                |object, member| verter_type_expr::TypeExpr::IndexedAccess {
                    object: std::sync::Arc::new(object),
                    index: std::sync::Arc::new(verter_type_expr::TypeExpr::string_literal(
                        member.clone(),
                    )),
                },
            )
        }
        fn wrap_registry_member_path_surface(
            path: &[String],
            leaf: verter_type_expr::TypeExpr,
        ) -> verter_type_expr::TypeExpr {
            path.iter().rfold(leaf, |child, member| {
                // Structural nested-object wrapper synthesized from a member-name
                // path with NO source object (the route fallback: `raw_body` was
                // not an Object surface, so no source member visibility exists to
                // thread). This fallback is reached only with public member names
                // — non-public class members are gated out of the keyspace and
                // out of route keys upstream — so `synthetic_public` is correct.
                // The primary `component_meta_registry_raw_member_path_surface`
                // path threads real source visibility when an Object body exists.
                verter_type_expr::TypeExpr::Object(std::sync::Arc::new(
                    verter_type_expr::ObjectExpr {
                        properties: vec![verter_type_expr::ObjectMember::Property(
                            verter_type_expr::ObjectProperty::synthetic_public(
                                member.clone(),
                                child,
                                true,
                                false,
                            ),
                        )],
                    },
                ))
            })
        }
        /// Issue #10 / predicate: does `expr` contain any
        /// callable surface (`TypeExpr::Function` or an Object with a
        /// call/method signature) anywhere reachable from its top-
        /// level structure (Array element, Intersection arm, Union
        /// arm, Object property, Tuple element)?
        ///
        /// Used by the Pick member-route materialiser to detect when
        /// descending into a member's leaf would walk through a
        /// callable parameter type — which, when the param root is
        /// package-backed, must be preserved symbolically rather than
        /// expanded as if it were prop metadata.
        fn type_expr_contains_callable_surface(expr: &verter_type_expr::TypeExpr) -> bool {
            // Real logic lives at module scope
            // (`type_expr_contains_callable_surface_impl`) so it is directly
            // unit-testable; this binding preserves the method-local call
            // sites that reference the predicate by name.
            type_expr_contains_callable_surface_impl(expr)
        }

        /// Issue #10 / extract the raw type AND declared visibility of
        /// `member` from `raw_body` when `raw_body` is an Object surface (the
        /// resolved body of the picked alias). Returns `None` when `raw_body`
        /// is not an Object or no property matches. The visibility is threaded
        /// so the Pick member-route reconstruction preserves the source
        /// member's accessibility rather than re-minting it as `Public`.
        fn raw_pick_member_leaf(
            raw_body: &verter_type_expr::TypeExpr,
            member: &str,
        ) -> Option<(
            verter_type_expr::TypeExpr,
            verter_type_expr::MemberVisibility,
        )> {
            use verter_type_expr::{ObjectMember, TypeExpr};
            match raw_body {
                TypeExpr::Parenthesized(inner) => raw_pick_member_leaf(inner, member),
                TypeExpr::Object(object) => object.properties.iter().find_map(|m| match m {
                    ObjectMember::Property(p) if p.name == member => {
                        Some((p.ty.clone(), p.visibility))
                    }
                    _ => None,
                }),
                _ => None,
            }
        }

        /// Issue #10 / predicate: does `param.ty` resolve
        /// to a package-backed declaration?
        ///
        /// Walks every `TypeExpr::Ref { name }` rooted in the
        /// parameter type, lowers each to a `SemanticNodeId` via the
        /// project's dispatch, and returns `true` iff any of those
        /// roots resolves to a package-backed declaration (per the
        /// graph-native `is_package_backed_ref` predicate, which
        /// routes the canonical-id classification through
        /// `ResolverContext::workspace_is_package_backed`).
        /// Issue #10 / predicate: does the picked member's
        /// raw leaf contain a callable surface whose param root is
        /// package-backed? When this fires, the Pick member-route
        /// materialiser MUST bypass the registry indexed-access route
        /// and project the raw leaf directly so the package-backed
        /// callable parameter type stays symbolic.
        fn pick_member_route_should_skip_callable_descent(
            raw_leaf: &verter_type_expr::TypeExpr,
            ctx: &dyn crate::resolver_core::ResolverContext,
            scope_canonical_id: &str,
        ) -> bool {
            pick_member_route_should_skip_callable_descent_impl(raw_leaf, ctx, scope_canonical_id)
        }

        fn materialize_component_meta_registry_candidate_for_route(
            query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            symbol_name: &str,
            route: &crate::resolver_core::RouteDemand,
            raw_body: Option<&verter_type_expr::TypeExpr>,
            prefer_explicit_raw_surface: bool,
        ) -> Option<verter_type_expr::TypeExpr> {
            use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

            match route {
                crate::resolver_core::RouteDemand::Whole => {
                    materialize_component_meta_registry_candidate(
                        query_engine,
                        scope_canonical_id,
                        symbol_name,
                        raw_body,
                        prefer_explicit_raw_surface,
                    )
                }
                crate::resolver_core::RouteDemand::MemberPath(path) if path.is_empty() => {
                    materialize_component_meta_registry_candidate(
                        query_engine,
                        scope_canonical_id,
                        symbol_name,
                        raw_body,
                        prefer_explicit_raw_surface,
                    )
                }
                crate::resolver_core::RouteDemand::MemberPath(path) => {
                    if let Some(projected) = raw_body.and_then(|expr| {
                        component_meta_registry_raw_member_path_surface(expr, path)
                    }) {
                        return Some(query_engine.materialize_member_surface_expr(
                            scope_canonical_id,
                            &projected,
                            true,
                        ));
                    }
                    let route_expr = build_registry_indexed_access_expr(symbol_name, path);
                    let leaf = project_expr_class_a_via_dispatch(
                        query_engine.ctx,
                        scope_canonical_id,
                        &route_expr,
                    )
                    .unwrap_or(route_expr);
                    if path.len() > 1
                        && !component_meta_registry_has_explicit_object_surface(&leaf)
                        && component_meta_registry_has_non_object_top_level_surface(&leaf)
                        && matches!(leaf, TypeExpr::IndexedAccess { .. })
                    {
                        return None;
                    }
                    if path.len() > 1
                        && !component_meta_registry_has_explicit_object_surface(&leaf)
                        && !component_meta_registry_has_non_object_top_level_surface(&leaf)
                    {
                        return None;
                    }
                    Some(query_engine.materialize_member_surface_expr(
                        scope_canonical_id,
                        &wrap_registry_member_path_surface(path, leaf),
                        false,
                    ))
                }
                crate::resolver_core::RouteDemand::Pick(members) => {
                    let mut properties = Vec::new();
                    for member in members {
                        // Source visibility of the picked member (when the
                        // resolved alias body is an Object surface). Threaded
                        // into the reconstructed member so a Pick of a class
                        // member preserves its declared accessibility instead of
                        // re-minting it as `Public`. `Public` only when the
                        // origin is genuinely source-less (no Object body / no
                        // matching member — e.g. a route-only Pick).
                        let member_visibility = raw_body
                            .and_then(|body| raw_pick_member_leaf(body, member.as_str()))
                            .map_or(verter_type_expr::MemberVisibility::Public, |(_, vis)| vis);
                        // Issue #10 / when the picked
                        // member's raw leaf contains a callable
                        // surface AND any callable parameter root
                        // resolves to a package-backed declaration,
                        // bypass the registry indexed-access route.
                        // Descending into a package-backed callable
                        // parameter (e.g. `(e, message: UIMessage) =>
                        // void` where `UIMessage` lives in `ai`)
                        // expands package internals into the
                        // consumer's prop surface and triggers
                        // unbounded recursion on cyclic external
                        // declarations. The raw leaf carries the
                        // symbolic Ref already; project it directly
                        // through `materialize_member_surface_expr`
                        // so package-backed param roots stay
                        // symbolic.
                        if let Some((raw_leaf, _)) =
                            raw_body.and_then(|body| raw_pick_member_leaf(body, member.as_str()))
                        {
                            if pick_member_route_should_skip_callable_descent(
                                &raw_leaf,
                                query_engine.ctx,
                                scope_canonical_id,
                            ) {
                                let projected_leaf = query_engine.materialize_member_surface_expr(
                                    scope_canonical_id,
                                    &raw_leaf,
                                    true,
                                );
                                // Pick-member projection — preserve the picked
                                // member's declared visibility.
                                properties.push(ObjectMember::Property(
                                    ObjectProperty::synthetic_with_visibility(
                                        member.clone(),
                                        projected_leaf,
                                        true,
                                        false,
                                        member_visibility,
                                    ),
                                ));
                                continue;
                            }
                        }
                        let member_route =
                            crate::resolver_core::RouteDemand::MemberPath(vec![member.clone()]);
                        let route_expr = build_registry_indexed_access_expr(
                            symbol_name,
                            std::slice::from_ref(member),
                        );
                        // Route shapes resolve natively through the
                        // materialiser branch; the remaining surface-expr
                        // fallback covers non-route shapes. The Class A
                        // dispatch helper handles the IndexedAccess
                        // route_expr through its registry-route fast-path
                        // internally, so the member-path route is a single
                        // dispatch call.
                        let _ = &member_route; // route demand carrier retained for parity
                        let projected = project_expr_class_a_via_dispatch_threaded(
                            query_engine.ctx,
                            Some(query_engine),
                            scope_canonical_id,
                            &route_expr,
                        )
                        .unwrap_or(route_expr);
                        // Issue #10 / record actual descent
                        // into the indexed-access route. The
                        // package-backed suppression branch above bails
                        // before this point; reaching here means we
                        // walked the route-expr through dispatch and
                        // are about to materialise the projected
                        // surface (which, for callable members, walks
                        // through callable parameters).
                        if raw_body
                            .and_then(|body| raw_pick_member_leaf(body, member.as_str()))
                            .is_some_and(|(leaf, _)| type_expr_contains_callable_surface(&leaf))
                        {
                            #[cfg(any(test, debug_assertions))]
                            crate::capture_token::with_active_capture(|t| {
                                t.record_counter(
                                    crate::meta_resolve::PICK_MEMBER_ROUTE_CALLABLE_DESCENT_COUNTER,
                                    1,
                                );
                            });
                        }
                        // One materialise + one Navigate-mode stabilisation
                        // per routed member. Publication demand is Navigate
                        // (shallow-by-default); carriers on the routed
                        // member surface are re-resolved by the consumer
                        // through the shared resolver, so no per-member
                        // re-solve or improvement pick runs here.
                        let member_surface = query_engine.materialize_member_surface_expr(
                            scope_canonical_id,
                            &projected,
                            true,
                        );
                        let stabilized = materialize_component_meta_type_expr_until_stable(
                            &member_surface,
                            scope_canonical_id,
                            crate::semantic_query::ProjectionMode::Navigate,
                            query_engine,
                        );
                        // No-poison: keep the routed member surface when the
                        // stabilisation INTRODUCED a semantic-miss sentinel
                        // it did not carry (an owner-scope re-lowering miss
                        // on a foreign-file carrier is "no reduction
                        // available here", not "unknown").
                        let ty =
                            if crate::resolver_core::type_expr_contains_semantic_miss(&stabilized)
                                && !crate::resolver_core::type_expr_contains_semantic_miss(
                                    &member_surface,
                                )
                            {
                                member_surface
                            } else {
                                stabilized
                            };
                        properties.push(ObjectMember::Property(
                            ObjectProperty::synthetic_with_visibility(
                                member.clone(),
                                ty,
                                true,
                                false,
                                member_visibility,
                            ),
                        ));
                    }
                    (!properties.is_empty())
                        .then(|| TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties })))
                        .or_else(|| {
                            // The Pick route dispatches through the builtin
                            // Pick utility path: pick_via_dispatch_pick_helper
                            // resolves the symbol to a base node via Class A
                            // lowering, then dispatches `Pick<base, key_set>`;
                            // falls back to the raw materialiser candidate for
                            // non-Object bases.
                            pick_via_dispatch_pick_helper(
                                query_engine,
                                scope_canonical_id,
                                symbol_name,
                                members.as_slice(),
                            )
                            .map(|projected| {
                                query_engine.materialize_member_surface_expr(
                                    scope_canonical_id,
                                    &projected,
                                    true,
                                )
                            })
                            .or_else(|| {
                                materialize_component_meta_registry_candidate(
                                    query_engine,
                                    scope_canonical_id,
                                    symbol_name,
                                    raw_body,
                                    prefer_explicit_raw_surface,
                                )
                            })
                        })
                }
                crate::resolver_core::RouteDemand::Omit(omitted) => {
                    let materialized = materialize_component_meta_registry_candidate(
                        query_engine,
                        scope_canonical_id,
                        symbol_name,
                        raw_body,
                        prefer_explicit_raw_surface,
                    )?;
                    // `TypeExpr` implements `Drop`; borrow `materialized` to
                    // filter the object surface and return it whole otherwise.
                    Some(match &materialized {
                        TypeExpr::Object(object) => {
                            let omitted: rustc_hash::FxHashSet<_> =
                                omitted.iter().map(String::as_str).collect();
                            TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                                properties: object
                                    .properties
                                    .iter()
                                    .filter(|member| match member {
                                        ObjectMember::Property(property) => {
                                            !omitted.contains(property.name.as_str())
                                        }
                                        _ => true,
                                    })
                                    .cloned()
                                    .collect(),
                            }))
                        }
                        _ => materialized,
                    })
                }
            }
        }
        fn collect_imported_component_meta_registry_seed_refs(
            expr: &verter_type_expr::TypeExpr,
            published_names: &rustc_hash::FxHashSet<String>,
            queued_names: &mut rustc_hash::FxHashSet<String>,
            output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
            source_hint: Option<&str>,
            cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
        ) {
            use verter_type_expr::{ObjectMember, TypeExpr};

            fn drain_filtered_pending(
                published_names: &rustc_hash::FxHashSet<String>,
                queued_names: &mut rustc_hash::FxHashSet<String>,
                output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
                pending: std::collections::VecDeque<PendingComponentMetaRegistryRef>,
            ) {
                for pending in pending {
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

            fn collect_one_filtered_expr(
                expr: &verter_type_expr::TypeExpr,
                published_names: &rustc_hash::FxHashSet<String>,
                queued_names: &mut rustc_hash::FxHashSet<String>,
                output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
                source_hint: Option<&str>,
                cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
            ) {
                let mut local_queue = std::collections::VecDeque::new();
                let mut local_names = rustc_hash::FxHashSet::default();
                collect_component_meta_registry_refs(
                    expr,
                    published_names,
                    &mut local_names,
                    &mut local_queue,
                    source_hint,
                    false,
                    cursor,
                );
                drain_filtered_pending(published_names, queued_names, output, local_queue);
            }

            match expr {
                TypeExpr::Object(obj) => {
                    for member in &obj.properties {
                        match member {
                            ObjectMember::Property(prop) => collect_one_filtered_expr(
                                &prop.ty,
                                published_names,
                                queued_names,
                                output,
                                source_hint,
                                cursor,
                            ),
                            ObjectMember::IndexSignature(sig) => {
                                collect_one_filtered_expr(
                                    &sig.key_type,
                                    published_names,
                                    queued_names,
                                    output,
                                    source_hint,
                                    cursor,
                                );
                                collect_one_filtered_expr(
                                    &sig.value_type,
                                    published_names,
                                    queued_names,
                                    output,
                                    source_hint,
                                    cursor,
                                );
                            }
                            ObjectMember::CallSignature(func)
                            | ObjectMember::ConstructSignature(func) => collect_one_filtered_expr(
                                &TypeExpr::Function(func.clone().into()),
                                published_names,
                                queued_names,
                                output,
                                source_hint,
                                cursor,
                            ),
                            ObjectMember::Method(method) => collect_one_filtered_expr(
                                &TypeExpr::Function(method.function.clone().into()),
                                published_names,
                                queued_names,
                                output,
                                source_hint,
                                cursor,
                            ),
                        }
                    }
                }
                TypeExpr::Function(func) => collect_one_filtered_expr(
                    &TypeExpr::Function(func.clone()),
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    cursor,
                ),
                _ => collect_one_filtered_expr(
                    expr,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    cursor,
                ),
            }
        }
        let debug_enabled = crate::host_manage::component_meta_debug_enabled();
        let import_refresh_started = debug_enabled.then(Instant::now);
        for (index, entry) in resolved_type_registry.iter_mut().enumerate() {
            let _entry_started = debug_enabled.then(Instant::now);
            let Some(meta) = resolved_type_registry_meta.get_mut(index) else {
                continue;
            };
            let declaration_source = meta.declaration.canonical_source.clone();
            if declaration_source.is_empty() || declaration_source == owner_canonical {
                continue;
            }
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration_source.as_str(),
            );
            if should_skip_imported_registry_seed_refresh(
                owner_canonical,
                &meta.declaration,
                &entry.type_expr,
            ) {
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
            if imported_registry_alias_should_stay_symbolic(&resolved.body) {
                entry.type_expr = verter_type_expr::TypeExpr::named(entry.name.clone());
                continue;
            }
            let materialized = materialize_component_meta_registry_candidate(
                query_engine,
                resolved.canonical_id.as_str(),
                resolved.exported_name.as_str(),
                Some(&resolved.body),
                true,
            )
            .unwrap_or_else(|| resolved.body.clone());
            entry.type_expr = merge_component_meta_registry_candidates(
                Some(entry.type_expr.clone()),
                Some(materialized),
            )
            .unwrap_or_else(|| entry.type_expr.clone());
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
            let define_props_roots = collect_define_props_root_names(snapshot);
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
                if matches!(
                    &field.r#type,
                    verter_type_expr::TypeExpr::SyntheticSlotBinding(_)
                ) {
                    continue;
                }
                if slot_binding_targets_define_props_root(field, &define_props_roots) {
                    #[cfg(any(test, debug_assertions))]
                    crate::capture_token::with_active_capture(|t| {
                        t.record_counter(
                            crate::meta_resolve::SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER,
                            1,
                        );
                    });
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
            if should_skip_imported_registry_seed_refresh(
                owner_canonical,
                &meta.declaration,
                &entry.type_expr,
            ) {
                continue;
            }
            let source_expr = source_hint
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
                let imported_collection_expr = component_meta_registry_ref_name(&entry.type_expr)
                    .filter(|ref_name| *ref_name == meta.declaration.resolved_name)
                    .filter(|_| !meta.declaration.canonical_source.is_empty())
                    .and_then(|_| {
                        query_engine.named_decl_body(
                            meta.declaration.canonical_source.as_str(),
                            meta.declaration.resolved_name.as_str(),
                        )
                    });
                collect_imported_component_meta_registry_seed_refs(
                    imported_collection_expr
                        .as_ref()
                        .or(source_expr.as_ref())
                        .unwrap_or(&entry.type_expr),
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    source_hint,
                    registry_cursor,
                );
            } else {
                collect_component_meta_registry_refs(
                    source_expr.as_ref().unwrap_or(&entry.type_expr),
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    source_hint,
                    false,
                    registry_cursor,
                );
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
                collect_type_expr_ref_names(&entry.type_expr, &mut names);
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
            } = pending;
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
                            "REGISTRY_IMPORTED_GATE owner={} name={} stay_symbolic={} route_whole={} body_variant={:?}",
                            owner_canonical, type_name,
                            imported_registry_alias_should_stay_symbolic(&resolved.body),
                            pending_route_is_whole,
                            std::mem::discriminant(&resolved.body),
                        ));
                    }
                    if pending_route_is_whole
                        && imported_registry_alias_should_stay_symbolic(&resolved.body)
                    {
                        // Imported non-object helpers (mapped/conditional/
                        // indexed-access/typeof aliases) must not be expanded
                        // into the owner registry on a whole-type route — the
                        // consumer will resolve them through member paths.
                        //
                        // If we already published a richer entry under this
                        // name, refresh its declaration metadata (the merge in
                        // upsert_component_meta_registry_entry keeps the
                        // richer body, so the bare Named placeholder is
                        // discarded by `merge_component_meta_registry_candidates`).
                        //
                        // If the name was never published, skip publication
                        // entirely — a bare Named placeholder only leaks a
                        // symbolic helper that the consumer didn't ask for.
                        if published_names.contains(&type_name) {
                            upsert_component_meta_registry_entry(
                                owner_canonical,
                                resolved_type_registry,
                                resolved_type_registry_meta,
                                &mut published_names,
                                &mut queued_names,
                                &mut referenced_names,
                                type_name.clone(),
                                verter_type_expr::TypeExpr::named(type_name.clone()),
                                declaration,
                                None,
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
                    let surface_started =
                        crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                    let type_expr = materialize_component_meta_registry_candidate_for_route(
                        query_engine,
                        resolved.canonical_id.as_str(),
                        resolved.exported_name.as_str(),
                        &pending_route,
                        Some(&resolved.body),
                        true,
                    )
                    .or_else(|| match &pending_route {
                        crate::resolver_core::RouteDemand::Whole => Some(resolved.body.clone()),
                        crate::resolver_core::RouteDemand::MemberPath(path) if path.is_empty() => {
                            Some(resolved.body.clone())
                        }
                        _ => None,
                    });
                    let Some(type_expr) = type_expr else {
                        continue;
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
                        type_name.clone(),
                        type_expr,
                        declaration,
                        None,
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
            let mut materialized = if declaration_owner != owner_canonical {
                materialize_component_meta_registry_candidate_for_route(
                    query_engine,
                    declaration_owner,
                    type_name.as_str(),
                    &pending_route,
                    declaration_body.as_ref(),
                    true,
                )
            } else {
                None
            };
            let owner_collection_expr =
                query_engine.owner_collection_expr(owner_canonical, type_name.as_str());
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
                if let Some(verter_type_expr::TypeExpr::Ref {
                    name: body_ref_name,
                    type_arguments,
                }) = owner_collection_expr.as_ref()
                {
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
            // Owner-local generic aliases publish the full shape so all
            // members (including those from deep indexed-access paths that
            // were already resolved inline) appear in the registry entry.
            let effective_local_route =
                if declaration_owner == owner_canonical && !pending_route_is_whole_local {
                    if owner_collection_expr.as_ref().is_some_and(|expr| {
                        matches!(
                            expr,
                            verter_type_expr::TypeExpr::Ref {
                                type_arguments, ..
                            } if !type_arguments.is_empty()
                        )
                    }) {
                        crate::resolver_core::RouteDemand::Whole
                    } else {
                        pending_route.clone()
                    }
                } else {
                    pending_route.clone()
                };
            materialized = materialized.or_else(|| {
                materialize_component_meta_registry_candidate_for_route(
                    query_engine,
                    owner_canonical,
                    type_name.as_str(),
                    &effective_local_route,
                    owner_collection_expr.as_ref(),
                    true,
                )
            });
            if materialized.is_some() && declaration.canonical_source.is_empty() {
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
            let Some(materialized) = materialized else {
                continue;
            };
            let collection_expr = if owner_collection_expr.as_ref().is_some_and(|expr| {
                !component_meta_registry_has_explicit_object_surface(expr)
                    && component_meta_registry_has_explicit_object_surface(&materialized)
            }) {
                Some(materialized.clone())
            } else {
                owner_collection_expr.clone()
            };
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_PENDING_LOCAL_SURFACE owner={} name={} route={:?} materialized={:?}",
                    owner_canonical, type_name, pending_route, materialized
                ));
            }
            _loop_materializations += 1;
            upsert_component_meta_registry_entry(
                owner_canonical,
                resolved_type_registry,
                resolved_type_registry_meta,
                &mut published_names,
                &mut queued_names,
                &mut referenced_names,
                type_name.clone(),
                materialized,
                declaration,
                collection_expr.as_ref(),
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
        let enrich_started = debug_enabled.then(Instant::now);

        // Registry enrichment: materialize imported type expressions through
        // the shared request-scoped engine so projection/instantiation caches
        // are reused across all registry entries in one request.
        for (index, entry) in resolved_type_registry.iter_mut().enumerate() {
            let _entry_started =
                crate::host_manage::component_meta_debug_enabled().then(Instant::now);
            let Some(meta) = resolved_type_registry_meta.get(index) else {
                continue;
            };
            let scope_canonical = if !meta.declaration.canonical_source.is_empty() {
                meta.declaration.canonical_source.as_str()
            } else {
                owner_canonical
            };
            if scope_canonical == owner_canonical {
                continue;
            }
            if !meta.declaration.resolved_name.is_empty()
                && component_meta_registry_expr_references_name(
                    &entry.type_expr,
                    meta.declaration.resolved_name.as_str(),
                )
            {
                continue;
            }
            if component_meta_registry_has_non_object_top_level_surface(&entry.type_expr)
                && component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                    &entry.type_expr,
                    scope_canonical,
                    query_engine,
                )
            {
                continue;
            }
            let raw_body = query_engine.named_decl_body(
                scope_canonical,
                if !meta.declaration.resolved_name.is_empty() {
                    meta.declaration.resolved_name.as_str()
                } else {
                    entry.name.as_str()
                },
            );
            let materialized = query_engine.materialize_member_surface_expr(
                scope_canonical,
                &entry.type_expr,
                false,
            );
            let preserved_nested_routes = raw_body
                .as_ref()
                .filter(|raw| type_expr_needs_nested_symbolic_route_preservation(raw))
                .map_or(materialized.clone(), |raw| {
                    preserve_nested_symbolic_member_routes(
                        &materialized,
                        raw,
                        scope_canonical,
                        query_engine,
                        false,
                    )
                });
            entry.type_expr = raw_body
                .as_ref()
                .map_or(preserved_nested_routes.clone(), |raw| {
                    preserve_registry_callable_param_member_routes(&preserved_nested_routes, raw)
                });
            if let Some(started) = _entry_started {
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                if elapsed_ms >= 5.0 {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_ENRICH_ENTRY owner={} name={} scope={} elapsed_ms={:.1}",
                        owner_canonical, entry.name, scope_canonical, elapsed_ms,
                    ));
                }
            }
        }
        let enrich_elapsed_ms = enrich_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();
        if debug_enabled {
            crate::host_manage::component_meta_debug(format!(
                "PROFILE_PHASES owner={} import_refresh_ms={:.1} public_field_collect_ms={:.1} seed_scan_ms={:.1} loop_ms={:.1} enrich_ms={:.1}",
                owner_canonical,
                import_refresh_elapsed_ms,
                public_field_collect_elapsed_ms,
                seed_scan_elapsed_ms,
                loop_elapsed_ms,
                enrich_elapsed_ms,
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

            // Route-owned cache fast path for imported-only files: if we
            // already built a raw snapshot via the route-owned shallow state
            // pipeline, reuse it here instead of rebuilding from the
            // scheduler. This is gated on module_facts not holding it (= fully lazy).
            if self
                .project_type_store
                .indexed()
                .get_any(canonical)
                .is_none()
            {
                if let Some(raw_snapshot) = self.cached_route_owned_snapshot(canonical) {
                    self.provenance
                        .route_owned_snapshot_cache_hits
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let mut snapshot = (*raw_snapshot).clone();
                    self.resolve_snapshot_imports(canonical, &mut snapshot);
                    self.enrich_destructured_bindings(&mut snapshot);
                    if self.config.effective_scope().needs_template_analysis() {
                        self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                    }
                    return Some(snapshot);
                }
            }

            // Scheduler-first path for owner files: the scheduler has the
            // latest analysis after recompile, including updated import
            // routes for newly-added dependencies. FileArtifactStore may hold
            // stale import routes for owner files whose deps changed after
            // materialization.
            if let Some(snapshot) = self.build_snapshot_from_scheduler(canonical) {
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
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
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

        if self
            .project_type_store
            .indexed()
            .get_any(canonical)
            .is_none()
        {
            if let Some(raw_snapshot) = self.cached_route_owned_snapshot(canonical) {
                self.provenance
                    .route_owned_snapshot_cache_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut snapshot = (*raw_snapshot).clone();
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                component_meta_trace_custom!(
                    "get_raw_analysis_snapshot_result",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={} source=route_owned_snapshot_cache",
                        canonical,
                        snapshot.imports.len(),
                        snapshot.macros.len(),
                        snapshot.bindings.len(),
                        snapshot.template.is_some(),
                    ),
                );
                return Some(snapshot);
            }

            if let Some((raw_source, framework_parse, whole_hash)) =
                self.cached_route_owned_eval_state(canonical)
            {
                if !self.store_view_allows_current_whole_hash(canonical, whole_hash) {
                    return None;
                }
                if framework_parse.is_some() {
                    self.provenance
                        .route_owned_snapshot_parse_artifact_hits
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let mut snapshot = self.build_snapshot_from_source_state(
                    canonical,
                    &raw_source,
                    framework_parse.as_deref(),
                );
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                component_meta_trace_custom!(
                    "get_raw_analysis_snapshot_result",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={} source=route_owned_cache",
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
        let facts = self.ensure_indexed_ready(canonical)?;
        let mut snapshot = (*facts.snapshot).clone();
        self.resolve_snapshot_imports(canonical, &mut snapshot);
        self.enrich_destructured_bindings(&mut snapshot);
        if self.config.effective_scope().needs_template_analysis() {
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
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
        // The owner Route hash is dual-sourced on
        // `HostStoreView::derived_hashes` (indexed surface vs
        // `route_owned_shallow`), so it does not round-trip warm
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
    pub(crate) fn store_meta_payload(
        &self,
        canonical: &str,
        fact_versions: &[crate::resolver_core::FactVersionRef],
        payload: Vec<u8>,
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
                // Read the cached `route_hash` from IndexedReady when
                // available — symmetric to `import_route_hash`. Falls
                // back to recomputing via `hash_route_surface` only
                // when the canonical isn't yet indexed (read-only:
                // this code path must NOT call ensure_indexed because
                // fact validation is side-effect-free).
                //
                // The lookup is content-pinned. This is a
                // fact-validation oracle: a permissive `get_any` here
                // would let a stale `IndexedReady` (one whose content
                // is no longer current — possible once eager
                // `evict_canonical` is retired) surface its old
                // `route_hash` as the "current" Route fact, confirming
                // a stale dependent cache entry as valid.
                // `current_content_pinned_indexed` returns `None` for
                // a stale candidate, so the recompute path below
                // produces the truly-current route surface hash.
                if let Some(indexed) = self.current_content_pinned_indexed(canonical_id) {
                    if self.route_surface_is_edge_current(
                        &indexed.shallow_state,
                        indexed.edge_generation,
                    ) {
                        if let Some(cached) = indexed.route_hash {
                            return Some(cached);
                        }
                    } else {
                        // Edge-stale indexed surface: its cached `route_hash`
                        // encodes stale wildcard `export *` edges. Do NOT
                        // reproduce it — return `None` so a dependent cache
                        // entry rooted on the stale `Route` fact fails warm
                        // validation and recomputes against the live file set.
                        return None;
                    }
                }
                let state = self.shallow_file_state(canonical_id)?;
                state
                    .has_resolvable_surface()
                    .then(|| crate::resolver_store::hash_route_surface(&state))
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

/// Does `expr` contain any callable surface (`TypeExpr::Function`, a bare
/// `TypeExpr::ConstructorType`, or an Object with a call / construct /
/// method signature) anywhere reachable from its top-level structure
/// (Array element, Intersection arm, Union arm, Object property, Tuple
/// element)?
///
/// Used by the Pick member-route materialiser to detect when descending
/// into a member's leaf would walk through a callable parameter type —
/// which, when the param root is package-backed, must be preserved
/// symbolically rather than expanded as if it were prop metadata.
pub(crate) fn type_expr_contains_callable_surface_impl(expr: &verter_type_expr::TypeExpr) -> bool {
    use verter_type_expr::{ObjectMember, TypeExpr};
    match expr {
        // A bare constructor type (`new (...) => R`) is a callable surface
        // exactly like a function type — both carry the same `FunctionExpr`
        // payload. This predicate runs on raw prepared-decl bodies (analyzer
        // IR), so a constructor type reaches it un-collapsed and MUST be
        // detected as callable.
        TypeExpr::Function(_) | TypeExpr::ConstructorType(_) => true,
        TypeExpr::Object(object) => object.properties.iter().any(|m| match m {
            ObjectMember::Property(p) => type_expr_contains_callable_surface_impl(&p.ty),
            ObjectMember::CallSignature(_)
            | ObjectMember::ConstructSignature(_)
            | ObjectMember::Method(_) => true,
            ObjectMember::IndexSignature(_) => false,
        }),
        TypeExpr::Array { element, .. } => type_expr_contains_callable_surface_impl(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|el| type_expr_contains_callable_surface_impl(&el.ty)),
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
            arms.iter().any(type_expr_contains_callable_surface_impl)
        }
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
            type_expr_contains_callable_surface_impl(inner)
        }
        _ => false,
    }
}

/// Does `param_ty` resolve to a package-backed declaration? Walks every
/// `TypeExpr::Ref { name }` rooted in the parameter type, lowers each to a
/// `SemanticNodeId` via the project's dispatch, and returns `true` iff any
/// of those roots resolves to a package-backed declaration (per the
/// graph-native `is_package_backed_ref` predicate, which routes the
/// canonical-id classification through
/// `ResolverContext::workspace_is_package_backed`).
pub(crate) fn callable_param_root_is_package_backed_impl(
    param_ty: &verter_type_expr::TypeExpr,
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope_canonical_id: &str,
) -> bool {
    use crate::component_meta_materialize::is_package_backed_ref;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::ProjectionMode;
    use verter_type_expr::TypeExpr;
    fn collect_root_refs<'a>(expr: &'a TypeExpr, out: &mut Vec<&'a TypeExpr>) {
        match expr {
            TypeExpr::Ref { .. } => out.push(expr),
            TypeExpr::Parenthesized(inner) => collect_root_refs(inner, out),
            TypeExpr::Array { element, .. } => collect_root_refs(element, out),
            TypeExpr::Tuple { elements, .. } => {
                for el in elements.iter() {
                    collect_root_refs(&el.ty, out);
                }
            }
            TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
                for a in arms.iter() {
                    collect_root_refs(a, out);
                }
            }
            _ => {}
        }
    }
    let mut roots: Vec<&TypeExpr> = Vec::new();
    collect_root_refs(param_ty, &mut roots);
    if roots.is_empty() {
        return false;
    }
    let dispatch = ProjectSemanticDispatch::new(ctx);
    roots.iter().any(|r| {
        dispatch
            .lower_type_expr_in_scope_with_mode(scope_canonical_id, r, ProjectionMode::Navigate)
            .is_some_and(|node| is_package_backed_ref(ctx, node))
    })
}

/// Does the picked member's raw leaf contain a callable surface whose
/// param root is package-backed? When this fires, the Pick member-route
/// materialiser MUST bypass the registry indexed-access route and project
/// the raw leaf directly so the package-backed callable parameter type
/// stays symbolic.
pub(crate) fn pick_member_route_should_skip_callable_descent_impl(
    raw_leaf: &verter_type_expr::TypeExpr,
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope_canonical_id: &str,
) -> bool {
    use verter_type_expr::{ObjectMember, TypeExpr};
    // Visit every callable surface reachable from `raw_leaf` and check ANY
    // parameter root for package-backed-ness.
    fn any_callable_param_is_package_backed(
        expr: &TypeExpr,
        ctx: &dyn crate::resolver_core::ResolverContext,
        scope_canonical_id: &str,
    ) -> bool {
        match expr {
            // A constructor type carries the same `FunctionExpr` payload as a
            // function type; its parameter roots must be checked for
            // package-backed-ness identically. Without this arm a raw
            // `new (m: PackageBacked) => ...` member silently fails the
            // suppression predicate and the package-backed param would be
            // descended into.
            TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                func.parameters.iter().any(|p| {
                    callable_param_root_is_package_backed_impl(&p.ty, ctx, scope_canonical_id)
                })
            }
            TypeExpr::Object(object) => object.properties.iter().any(|m| match m {
                ObjectMember::Property(p) => {
                    any_callable_param_is_package_backed(&p.ty, ctx, scope_canonical_id)
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    func.parameters.iter().any(|p| {
                        callable_param_root_is_package_backed_impl(&p.ty, ctx, scope_canonical_id)
                    })
                }
                ObjectMember::Method(method) => method.function.parameters.iter().any(|p| {
                    callable_param_root_is_package_backed_impl(&p.ty, ctx, scope_canonical_id)
                }),
                ObjectMember::IndexSignature(_) => false,
            }),
            TypeExpr::Array { element, .. } => {
                any_callable_param_is_package_backed(element, ctx, scope_canonical_id)
            }
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|el| any_callable_param_is_package_backed(&el.ty, ctx, scope_canonical_id)),
            TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => arms
                .iter()
                .any(|a| any_callable_param_is_package_backed(a, ctx, scope_canonical_id)),
            TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
                any_callable_param_is_package_backed(inner, ctx, scope_canonical_id)
            }
            _ => false,
        }
    }
    type_expr_contains_callable_surface_impl(raw_leaf)
        && any_callable_param_is_package_backed(raw_leaf, ctx, scope_canonical_id)
}

#[cfg(test)]
#[path = "component_meta_callable_descent_tests.rs"]
mod callable_descent_predicate_tests;
