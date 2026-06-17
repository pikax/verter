//! Component-meta request-host trait impls + audit-capture types +
//! cache-key helper.
//!
//! domain 4 + cache-key helper (domain 14) of the
//! meta_resolve.rs split.
//!
//! Owns the four pieces of the request-orchestration boundary:
//!
//! - `ComponentMetaRequestHost for VerterHost` (process-wide adapter)
//! - `SessionRequestHost<'a>` + `ComponentMetaRequestHost` impl
//!   (session-scoped adapter)
//! - `pub struct CapturedComponentMetaInputs` — captured-snapshot type
//!   used by the request executor at `component_meta_request.rs`
//! - The `Resolved*` type aliases re-exported from `resolver_core`
//!   under the `meta_resolve` namespace
//! - `pub struct ResolvedComponentMetaComputeAudit` — non-semantic
//!   compute-audit sidecar
//! - `resolved_meta_cache_key(canonical, mode)` cache-key builder

// file moved from `meta_resolve/request_host.rs` to
// `host_manage/component_meta_request_impl.rs`. Original `super::X`
// imports resolved through `meta_resolve` private siblings; after the
// move, `super` is `host_manage`, so the rewrite goes via the parent
// module's `pub(crate)`-re-exported surface.
use crate::host_manage::component_meta_trace_custom;
use crate::meta_resolve::ResolvedComponentMetaState;
use crate::resolver_core::{ComponentMetaRequestHost, RequestSource, SingleflightRole};
use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};
use crate::VerterHost;

use crate::instant::Instant;

pub(crate) fn next_component_meta_audit_request_id() -> u64 {
    static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn trace_request_source(source: RequestSource) -> &'static str {
    match source {
        RequestSource::Cache => "cache",
        RequestSource::Flight {
            role: SingleflightRole::Leader,
            ..
        } => "flight:leader",
        RequestSource::Flight {
            role: SingleflightRole::Follower,
            ..
        } => "flight:follower",
        RequestSource::Fallback => "fallback",
    }
}

pub(crate) fn request_source_performed_compute(source: RequestSource) -> bool {
    matches!(
        source,
        RequestSource::Flight {
            role: SingleflightRole::Leader,
            ..
        } | RequestSource::Fallback,
    )
}

pub(crate) fn should_skip_imported_registry_seed_refresh(
    owner_canonical: &str,
    declaration: &ResolvedTypeDeclaration,
    existing_expr: &verter_type_expr::TypeExpr,
) -> bool {
    crate::resolver_core::component_meta::imported_registry_seed_can_skip_refresh(
        owner_canonical,
        declaration,
        existing_expr,
    )
}

#[derive(Debug, Clone)]
pub struct CapturedComponentMetaInputs {
    pub(crate) whole_hash: Hash16,
    pub(crate) snapshot: FileAnalysisSnapshot,
    pub(crate) owner_eval_source: Option<String>,
    pub(crate) direct_dependency_candidates: std::collections::BTreeSet<String>,
    pub(crate) audit_capture_inputs_ms: f64,
    pub(crate) audit_store_read_ms: f64,
    pub(crate) audit_direct_import_proof_ms: f64,
}

/// View-bound [`ComponentMetaRequestHost`] adapter that threads a
/// [`SessionView`](crate::session_view::SessionView) into the request
/// orchestration boundary.
///
/// `cache_key` folds the view's fingerprint into the resolution key so
/// two concurrent sessions with different overlays do not coalesce on
/// the same singleflight slot (R20 multi-candidate isolation). The
/// view's overlay source is consulted in `capture_component_meta_inputs`
/// so cold-compute reads observe overlay content for the owner
/// canonical. All other methods delegate to the inner host.
///
/// ## Attempt-scoped overlay carrier
///
/// `overlay` is the request-scoped
/// [`CanonicalCompletionOverlay`](crate::resolver_core::CanonicalCompletionOverlay)
/// — built ONCE at adapter construction time and shared by every
/// resolver call inside the request. Mid-request `ensure_loaded` /
/// `ensure_indexed_ready_serve` successes promote canonicals into THIS
/// overlay; subsequent reads through the same request observe the
/// promotions through the `RequestStoreView` shadowing rail.
///
/// Before the attempt-scoped overlay carrier, the overlay was constructed inside each
/// `compute_component_meta_state_*_with_view` helper call — that
/// allocated a fresh empty overlay per cold compute and paid the
/// shadowing write overhead with zero cross-call accumulation
/// benefit, regressing the bench by +49% (per an earlier profiling
/// diagnosis). Hoisting the overlay onto the adapter struct closes
/// the gap by letting `compute_component_meta_state_*_with_view`
/// borrow the shared `Arc` instead of `Arc::new()`-ing one per call.
pub(crate) struct ViewBoundRequestHost<'a> {
    pub(crate) host: &'a VerterHost,
    pub(crate) view: &'a dyn crate::session_view::SessionView,
    pub(crate) overlay: std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
}

impl ComponentMetaRequestHost for VerterHost {
    type View = crate::resolver_store::HostStoreView;
    type Mode = ProjectionMode;
    type Resolution = ResolvedComponentMetaState;
    type CapturedInputs = CapturedComponentMetaInputs;

    fn cache_key(
        &self,
        canonical: &str,
        mode: Self::Mode,
    ) -> crate::resolver_core::ResolutionNodeKey {
        resolved_meta_cache_key(canonical, mode)
    }

    fn snapshot_store_view(&self) -> Self::View {
        // The request driver gates currentness through
        // `snapshot_store_view_read` + `snapshot_view_is_current`; this
        // owned-view accessor hands back the cold-seed's inner view.
        self.resolver_store_view_read()
            .into_cold_seed_view()
            .into_inner()
    }

    fn snapshot_store_view_read(&self) -> (Self::View, bool) {
        self.resolver_store_view_with_currentness()
    }

    fn current_view_supersession_fingerprint(&self) -> u64 {
        self.current_external_supersession_fingerprint()
    }

    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        _view: &Self::View,
    ) -> Option<Self::CapturedInputs> {
        let audit_enabled = self.config.audit_enabled;
        let capture_started = audit_enabled.then(Instant::now);
        let store_read_started = audit_enabled.then(Instant::now);
        component_meta_trace_custom!(
            "capture_component_meta_inputs",
            format!("owner={} store_view=true", canonical),
        );
        let snapshot = self.get_raw_analysis_snapshot(canonical)?;
        component_meta_trace_custom!(
            "capture_component_meta_snapshot",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={}",
                canonical,
                snapshot.imports.len(),
                snapshot.macros.len(),
                snapshot.bindings.len(),
                snapshot.template.is_some(),
            ),
        );
        let facts = self.ensure_indexed_ready_serve(canonical)?.indexed;
        let whole_hash = facts.whole_hash;
        let store_read_ms = store_read_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        component_meta_trace_custom!(
            "capture_component_meta_eval_state",
            format!(
                "owner={} source_len={} has_parse_artifact={} whole_hash={whole_hash:?}",
                canonical,
                facts.raw_source.len(),
                facts.framework_parse.is_some(),
            ),
        );
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
        component_meta_trace_custom!(
            "capture_component_meta_inputs_result",
            format!(
                "owner={} owner_eval_source_len={} dependency_candidates={}",
                canonical,
                owner_eval_source.len(),
                direct_dependency_candidates.len(),
            ),
        );
        Some(CapturedComponentMetaInputs {
            whole_hash,
            snapshot,
            owner_eval_source: Some(owner_eval_source),
            direct_dependency_candidates,
            audit_capture_inputs_ms: capture_inputs_ms,
            audit_store_read_ms: store_read_ms,
            audit_direct_import_proof_ms: direct_import_proof_ms,
        })
    }

    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        component_meta_trace_custom!(
            "try_get_cached_component_meta",
            format!("owner={} mode={mode:?}", canonical),
        );
        // Thread the request-bound view through the warm-hit accessor
        // so the per-warm-hit `HostStoreView` rebuild is eliminated on
        // the bare-host hot path (bypass audit
        // top-leverage fix).
        let result = self.try_get_cached_resolved_meta_with_store_view(store_view, canonical, mode);
        component_meta_trace_custom!(
            "try_get_cached_component_meta_result",
            format!("owner={} mode={mode:?} hit={}", canonical, result.is_some()),
        );
        result
    }

    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        store_view: Option<&Self::View>,
        base_is_current: bool,
    ) -> Option<Self::Resolution> {
        // Consume the request-bound `store_view` to construct a
        // `HostResolverContext` so the cold-compute pipeline binds
        // overlay-aware reads to the same view the executor already
        // snapshotted. The singleflight executor always supplies a
        // view in production (`snapshot_view` builds one above);
        // the `None` branch falls back to building a view here for
        // robustness. `base_is_current` carries the executor snapshot's
        // currentness so the `HostResolverContext` fails its nested
        // warm-cache probes closed on a non-current seed.
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        match store_view {
            Some(view) => {
                if let Some(captured) = captured {
                    return self.compute_component_meta_state_from_captured_with_view_arg(
                        canonical,
                        mode,
                        captured,
                        view,
                        &overlay,
                        base_is_current,
                    );
                }
                let whole_hash = self
                    .current_or_read_whole_hash(canonical)
                    .unwrap_or_default();
                self.compute_component_meta_state_with_view_arg(
                    canonical,
                    mode,
                    whole_hash,
                    view,
                    &overlay,
                    base_is_current,
                )
            }
            None => {
                // Cold compute with no driver-supplied view: this runs
                // inside `run_stable_request`'s `compute`, whose `is_stable`
                // fence gates promotion. Seed from the cold-seed view AND
                // carry its currentness so the derived context fails its
                // nested warm-cache probes closed on a non-current seed.
                let cold_seed = self.resolver_store_view_read().into_cold_seed_view();
                let seed_is_current = cold_seed.is_current();
                let view = cold_seed.into_inner();
                if let Some(captured) = captured {
                    return self.compute_component_meta_state_from_captured_with_view_arg(
                        canonical,
                        mode,
                        captured,
                        &view,
                        &overlay,
                        seed_is_current,
                    );
                }
                let whole_hash = self
                    .current_or_read_whole_hash(canonical)
                    .unwrap_or_default();
                self.compute_component_meta_state_with_view_arg(
                    canonical,
                    mode,
                    whole_hash,
                    &view,
                    &overlay,
                    seed_is_current,
                )
            }
        }
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.store_cached_resolved_meta(canonical, mode, result, &result.fact_versions);
    }

    fn resolution_is_partial(&self, result: &Self::Resolution) -> bool {
        result.synthesis_should_suppress
    }
}

impl<'a> ComponentMetaRequestHost for ViewBoundRequestHost<'a> {
    type View = crate::resolver_store::HostStoreView;
    type Mode = ProjectionMode;
    type Resolution = ResolvedComponentMetaState;
    type CapturedInputs = CapturedComponentMetaInputs;

    fn cache_key(
        &self,
        canonical: &str,
        mode: Self::Mode,
    ) -> crate::resolver_core::ResolutionNodeKey {
        resolved_meta_cache_key_with_view_fingerprint(canonical, mode, self.view.fingerprint())
    }

    fn snapshot_store_view(&self) -> Self::View {
        // The request driver gates currentness through
        // `snapshot_store_view_read` + `snapshot_view_is_current`; this
        // owned-view accessor hands back the cold-seed's inner view,
        // overlay-rooted for the request view (mirroring
        // `snapshot_store_view_read`) so every view this host hands the
        // executor is already overlay-aware.
        self.host
            .resolver_store_view_read()
            .into_cold_seed_view()
            .with_session_overlay(self.host, self.view)
            .into_inner()
    }

    fn snapshot_store_view_read(&self) -> (Self::View, bool) {
        // Overlay-root the per-attempt snapshot ONCE here so the view the
        // executor hands to `try_get_cached` / `compute` is already
        // overlay-aware. This is the NON-fixed (interactive, N=1) path: the
        // executor's fixed-view branch supplies a view that
        // `capture_batch_fixed_view` ALREADY overlaid once per batch and
        // never calls this accessor. Overlaying here (rather than re-rooting
        // per compute call downstream) keeps the overlay application a single
        // copy-on-write per snapshot, and makes the `compute_component_meta`
        // `Some(view)` arm uniform: the view is overlay-aware whether it came
        // from the per-attempt read or the shared fixed view.
        let (base, is_current) = self.host.resolver_store_view_with_currentness();
        (base.with_session_overlay(self.host, self.view), is_current)
    }

    fn current_view_supersession_fingerprint(&self) -> u64 {
        self.host.current_external_supersession_fingerprint()
    }

    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        _view: &Self::View,
    ) -> Option<Self::CapturedInputs> {
        // Overlay-priority capture: when the session view carries an
        // overlay source for the owner canonical, the cold-compute
        // inputs MUST reflect overlay content. The shared
        // `capture_component_meta_inputs_with_view` helper publishes
        // the overlay's IndexedReady into FileArtifactStore on first
        // demand (multi-candidate; an overlay-covered canonical is
        // keyed under the overlay-scoped key) so resolver-tier reads
        // through SessionResolverContext find the overlay; the helper
        // then constructs CapturedComponentMetaInputs from the overlay
        // snapshot.
        self.host
            .capture_component_meta_inputs_with_view(canonical, self.view)
    }

    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        // View-aware variant: thread the request-bound view through so
        // the per-warm-hit rebuild is eliminated on the view-bound hot
        // path (bypass-audit top-leverage fix).
        self.host
            .try_get_cached_resolved_meta_for_view_fingerprint_with_store_view(
                store_view,
                canonical,
                mode,
                self.view.fingerprint(),
            )
    }

    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        store_view: Option<&Self::View>,
        base_is_current: bool,
    ) -> Option<Self::Resolution> {
        // View-aware cold compute: thread the session view through a
        // `SessionResolverContext` so the resolver-tier reads (prepared
        // declarations, dep-source materialisation, registry+macro shapes)
        // observe overlay candidates published by the prewarm pass.
        //
        // The cold-seed is built from the EXECUTOR's `(store_view,
        // base_is_current)` pair — the SAME single read the driver's
        // promotion fence (`is_stable`) gates on — re-bound through
        // `StoreViewRead::from_executor_snapshot` so currentness stays
        // intrinsic to the seed. This makes the compute seed and the
        // promotion-gating seed ONE read, matching the bare-host and
        // session-host paths (which already rebind through
        // `from_executor_snapshot`). Taking a SECOND fresh base read here
        // would diverge from the fence: under additive store-view churn —
        // which advances the artifact / load generations the
        // external-supersession fingerprint EXCLUDES — that second read can be
        // `ReturnOnly` while the executor snapshot is `Current`, so the fence
        // would promote a result computed from a non-current seed.
        // `self.overlay` is the request-scoped completion overlay shared
        // across capture / try-get-cached / compute boundaries.
        //
        // The supplied `store_view` is ALREADY overlay-rooted — the fixed-view
        // (batch) path's view was overlaid ONCE by `capture_batch_fixed_view`
        // and shared across jobs; the per-attempt path's view was overlaid by
        // `snapshot_store_view_read`. So the overlay is NOT re-applied per job
        // here (that per-job copy-on-write was the O(N²) regression) — the
        // seed only rebinds currentness onto the already-overlaid view.
        //
        // The `None` arm (no executor-supplied view) is a robustness fallback —
        // production always arrives through `run_component_meta_request` →
        // executor → `compute(view)` with a request-bound view. It derives the
        // seed (and its currentness) from its OWN fresh read, overlay-rooted,
        // so currentness stays intrinsic.
        let cold_seed = match store_view {
            Some(view) => crate::resolver_store::StoreViewRead::from_executor_snapshot(
                view.clone(),
                base_is_current,
            )
            .into_cold_seed_view(),
            None => self.host.view_bound_cold_seed(self.view),
        };
        if let Some(captured) = captured {
            return self
                .host
                .compute_component_meta_state_from_captured_with_session_view_and_base(
                    canonical,
                    mode,
                    captured,
                    self.view,
                    &cold_seed,
                    &self.overlay,
                );
        }
        let whole_hash = self
            .host
            .current_or_read_whole_hash(canonical)
            .unwrap_or_default();
        self.host
            .compute_component_meta_state_with_session_view_and_base(
                canonical,
                mode,
                whole_hash,
                self.view,
                &cold_seed,
                &self.overlay,
            )
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.host.store_cached_resolved_meta_for_view_fingerprint(
            canonical,
            mode,
            result,
            &result.fact_versions,
            self.view.fingerprint(),
        );
    }

    fn resolution_is_partial(&self, result: &Self::Resolution) -> bool {
        result.synthesis_should_suppress
    }
}

// ---------------------------------------------------------------------------
// SessionRequestHost — session-scoped ComponentMetaRequestHost
// ---------------------------------------------------------------------------

/// Session-scoped request host that routes reads through the session
/// runtime and writes to the session-scoped resolved-meta cache.
///
/// Replaces `impl ComponentMetaRequestHost for VerterHost` for all
/// session-scoped callers. The generic executor at
/// `component_meta_request.rs` calls these methods on the trait object,
/// so every axis is session-aware end to end.
///
/// ## Attempt-scoped overlay carrier
///
/// `overlay` is the request-scoped
/// [`CanonicalCompletionOverlay`](crate::resolver_core::CanonicalCompletionOverlay)
/// — built ONCE at adapter construction time and shared by every
/// resolver call inside the request. See [`ViewBoundRequestHost`]'s
/// doc for the full rationale.
pub struct SessionRequestHost<'a> {
    pub(crate) runtime: &'a crate::session_runtime::SessionRuntime,
    pub(crate) overlay: std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
}

impl<'a> ComponentMetaRequestHost for SessionRequestHost<'a> {
    type View = crate::resolver_store::HostStoreView;
    type Mode = ProjectionMode;
    type Resolution = ResolvedComponentMetaState;
    type CapturedInputs = CapturedComponentMetaInputs;

    fn cache_key(
        &self,
        canonical: &str,
        mode: Self::Mode,
    ) -> crate::resolver_core::ResolutionNodeKey {
        resolved_meta_cache_key(canonical, mode)
    }

    fn snapshot_store_view(&self) -> Self::View {
        // The session-scoped overlay-mutation machinery is retired
        // (R17); singleflight lane identity reads the raw session id
        // directly.
        crate::resolver_store::HostStoreView::from_session_id(
            self.runtime.session_id(),
            self.runtime.host(),
        )
    }

    fn snapshot_store_view_read(&self) -> (Self::View, bool) {
        crate::resolver_store::HostStoreView::from_session_id_read(
            self.runtime.session_id(),
            self.runtime.host(),
        )
    }

    fn current_view_supersession_fingerprint(&self) -> u64 {
        // The session overlay identity is frozen for the request, so the
        // BASE external-supersession fold (overlay = None at both capture
        // points) is the precise "external mutation superseded my
        // snapshot" oracle — env / epoch / project / identity shifts.
        self.runtime
            .host()
            .current_external_supersession_fingerprint()
    }

    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        _view: &Self::View,
    ) -> Option<Self::CapturedInputs> {
        let host = self.runtime.host();
        let audit_enabled = host.config.audit_enabled;
        let capture_started = audit_enabled.then(Instant::now);
        let store_read_started = audit_enabled.then(Instant::now);
        component_meta_trace_custom!(
            "session_capture_component_meta_inputs",
            format!("owner={} session={}", canonical, self.runtime.session_id()),
        );
        let snapshot = host.get_raw_analysis_snapshot(canonical)?;
        let facts = host.ensure_indexed_ready_serve(canonical)?.indexed;
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
            host.cache_dependency_candidates_from_snapshot(canonical, &snapshot);
        let direct_import_proof_ms = direct_import_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let capture_inputs_ms = capture_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Some(CapturedComponentMetaInputs {
            whole_hash,
            snapshot,
            owner_eval_source: Some(owner_eval_source),
            direct_dependency_candidates,
            audit_capture_inputs_ms: capture_inputs_ms,
            audit_store_read_ms: store_read_ms,
            audit_direct_import_proof_ms: direct_import_proof_ms,
        })
    }

    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        // Session-bearing hot path: thread the request-bound view
        // through so the per-warm-hit rebuild is eliminated (per the
        // iter3 — bypass audit top-leverage fix).
        self.runtime
            .try_get_cached_resolved_meta_with_store_view(store_view, canonical, mode)
    }

    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        store_view: Option<&Self::View>,
        base_is_current: bool,
    ) -> Option<Self::Resolution> {
        // Consume the executor-snapshotted `store_view` to build the
        // request-bound `HostResolverContext` so the cold-compute pipeline
        // reuses it rather than rebuilding a fresh workspace snapshot.
        // The shared overlay (`self.overlay`) lives across capture /
        // try-get-cached / compute boundaries so canonicals promoted
        // mid-request by `ensure_loaded` / `ensure_indexed_ready_serve` stay
        // visible. `base_is_current` carries the snapshot's currentness so
        // the `HostResolverContext` fails its nested warm-cache probes
        // closed on a non-current seed.
        let host = self.runtime.host();
        match store_view {
            Some(view) => {
                if let Some(captured) = captured {
                    return host.compute_component_meta_state_from_captured_with_view_arg(
                        canonical,
                        mode,
                        captured,
                        view,
                        &self.overlay,
                        base_is_current,
                    );
                }
                let whole_hash = host
                    .current_or_read_whole_hash(canonical)
                    .unwrap_or_default();
                host.compute_component_meta_state_with_view_arg(
                    canonical,
                    mode,
                    whole_hash,
                    view,
                    &self.overlay,
                    base_is_current,
                )
            }
            None => {
                // No executor-supplied view: the overlay entry does its OWN
                // fresh base read whose currentness is INTRINSIC to the seed
                // (`compute_component_meta_state_with_overlay`), so the
                // executor's `base_is_current` is NOT threaded into this arm
                // — pairing it with a fresh read is the divergence this path
                // closed.
                if let Some(captured) = captured {
                    return host.compute_component_meta_state_from_captured_with_overlay(
                        canonical,
                        mode,
                        captured,
                        &self.overlay,
                    );
                }
                let whole_hash = host
                    .current_or_read_whole_hash(canonical)
                    .unwrap_or_default();
                host.compute_component_meta_state_with_overlay(
                    canonical,
                    mode,
                    whole_hash,
                    &self.overlay,
                )
            }
        }
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.runtime.store_resolved_meta(canonical, mode, result);
    }

    fn resolution_is_partial(&self, result: &Self::Resolution) -> bool {
        result.synthesis_should_suppress
    }
}

/// Native declaration kind for the resolved pre-expansion type.
pub type ResolvedDeclarationKind = crate::resolver_core::ResolvedDeclarationKind;

/// Native pre-expansion declaration metadata retained by the shared resolver.
pub type ResolvedTypeDeclaration = crate::resolver_core::ResolvedTypeDeclaration;
pub type ResolvedTypeRegistryMeta = crate::resolver_core::ResolvedTypeRegistryMeta;
pub type ResolvedMacroMeta = crate::resolver_core::ResolvedMacroMeta;
pub type ResolvedNativeProp = crate::resolver_core::ResolvedNativeProp;
pub type ResolvedJsdocBlock = crate::resolver_core::ResolvedJsdocBlock;
pub type ResolvedJsdocTag = crate::resolver_core::ResolvedJsdocTag;

/// Host-owned sidecar result for component-meta / analysis enrichment.
///
/// Raw snapshot remains raw — resolved imported metadata lives in this sidecar.
/// `Expanded` mode carries materialized surfaces; `Type` mode carries
/// identity/location only.
#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaComputeAudit {
    pub timings: crate::component_meta_audit::RequestTimingAudit,
    pub solver: crate::component_meta_audit::ComponentMetaPayload,
}

pub(crate) fn resolved_meta_cache_key(
    canonical: &str,
    mode: ProjectionMode,
) -> crate::resolver_core::ResolutionNodeKey {
    resolved_meta_cache_key_with_view_fingerprint(canonical, mode, 0)
}

/// View-fingerprint-aware variant of [`resolved_meta_cache_key`].
///
/// `view_fingerprint == 0` is the overlay-free base host shape and
/// matches what `resolved_meta_cache_key` returns. Non-zero values
/// (produced by overlay-bearing
/// [`SessionView::fingerprint`](crate::session_view::SessionView::fingerprint)
/// implementations) admit distinct singleflight slots so two
/// concurrent sessions with different overlays cannot coalesce on the
/// same in-flight build (R20 multi-candidate isolation).
pub(crate) fn resolved_meta_cache_key_with_view_fingerprint(
    canonical: &str,
    mode: ProjectionMode,
    view_fingerprint: u64,
) -> crate::resolver_core::ResolutionNodeKey {
    crate::resolver_core::ResolutionNodeKey {
        symbol_id: canonical.to_string(),
        node_kind: crate::resolver_core::ResolutionNodeKind::Assemble,
        traversal_lens: crate::resolver_core::TraversalLens::StructuralObject,
        member_path_hash: 0,
        type_args_hash: 0,
        behavior_flags: match mode {
            ProjectionMode::Identity => 1,
            ProjectionMode::Navigate => 2,
            ProjectionMode::Shallow => 3,
            ProjectionMode::Expanded => 4,
            ProjectionMode::Skeleton => 5,
        },
        view_fingerprint,
    }
}
