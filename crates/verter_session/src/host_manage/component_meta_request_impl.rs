//! Component-meta request-host trait impls + audit-capture types +
//! cache-key helper.
//!
//! Phase 11a domain 4 + cache-key helper (domain 14) of the
//! meta_resolve.rs split.
//!
//! Owns the four pieces of the request-orchestration boundary:
//!
//! - `ComponentMetaRequestHost for VerterHost` (process-wide adapter)
//! - `SessionRequestHost<'a>` + `ComponentMetaRequestHost` impl
//!   (session-scoped adapter — Path C C14)
//! - `pub struct CapturedComponentMetaInputs` — captured-snapshot type
//!   used by the request executor at `component_meta_request.rs`
//! - The `Resolved*` type aliases re-exported from `resolver_core`
//!   under the `meta_resolve` namespace
//! - `pub struct ResolvedComponentMetaComputeAudit` — non-semantic
//!   compute-audit sidecar
//! - `resolved_meta_cache_key(canonical, mode)` cache-key builder

// Phase 10a: file moved from `meta_resolve/request_host.rs` to
// `host_manage/component_meta_request_impl.rs`. Original `super::X`
// imports resolved through `meta_resolve` private siblings; after the
// move, `super` is `host_manage`, so the rewrite goes via the parent
// module's `pub(crate)`-re-exported surface.
use crate::host_manage::component_meta_trace_custom;
use crate::meta_resolve::ResolvedComponentMetaState;
use crate::resolver_core::{ComponentMetaRequestHost, RequestSource, SingleflightRole};
use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};
use crate::VerterHost;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

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
    existing_expr: &verter_semantic::analysis::type_expr::TypeExpr,
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
        self.resolver_store_view()
    }

    fn view_mutation_epoch(&self, store_view: &Self::View) -> u64 {
        store_view.mutation_epoch()
    }

    fn current_store_view_epoch(&self) -> u64 {
        VerterHost::current_store_view_epoch(self)
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
        let facts = self.ensure_indexed_ready(canonical)?;
        let whole_hash = facts.whole_hash;
        let store_read_ms = store_read_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        component_meta_trace_custom!(
            "capture_component_meta_eval_state",
            format!(
                "owner={} source_len={} has_cached_parse={} whole_hash={whole_hash:?}",
                canonical,
                facts.raw_source.len(),
                facts.cached_parse.is_some(),
            ),
        );
        let owner_eval_source =
            VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
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
        _store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        component_meta_trace_custom!(
            "try_get_cached_component_meta",
            format!("owner={} mode={mode:?}", canonical),
        );
        let result = self.try_get_cached_resolved_meta(canonical, mode);
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
        _store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution> {
        if let Some(captured) = captured {
            return self.compute_component_meta_state_from_captured(canonical, mode, captured);
        }

        let whole_hash = self
            .current_or_read_whole_hash(canonical)
            .unwrap_or_default();
        self.compute_component_meta_state(canonical, mode, whole_hash)
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.store_cached_resolved_meta(canonical, mode, result, &result.fact_versions);
    }
}

// ---------------------------------------------------------------------------
// SessionRequestHost — session-scoped ComponentMetaRequestHost (Path C C14)
// ---------------------------------------------------------------------------

/// Session-scoped request host that routes reads through the session
/// runtime and writes to the session-scoped resolved-meta cache.
///
/// Replaces `impl ComponentMetaRequestHost for VerterHost` for all
/// session-scoped callers. The generic executor at
/// `component_meta_request.rs` calls these methods on the trait object,
/// so every axis is session-aware end to end.
pub struct SessionRequestHost<'a> {
    pub(crate) runtime: &'a crate::session_runtime::SessionRuntime,
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
        let view = self.runtime.current_view();
        crate::resolver_store::HostStoreView::from_session(&view, self.runtime.host())
    }

    fn view_mutation_epoch(&self, store_view: &Self::View) -> u64 {
        store_view.mutation_epoch()
    }

    fn current_store_view_epoch(&self) -> u64 {
        self.runtime.current_store_view_epoch()
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
        let facts = host.ensure_indexed_ready(canonical)?;
        let whole_hash = facts.whole_hash;
        let store_read_ms = store_read_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let owner_eval_source =
            VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
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
        _store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        self.runtime.try_get_cached_resolved_meta(canonical, mode)
    }

    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        _store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution> {
        let host = self.runtime.host();
        if let Some(captured) = captured {
            return host.compute_component_meta_state_from_captured(canonical, mode, captured);
        }
        let whole_hash = host
            .current_or_read_whole_hash(canonical)
            .unwrap_or_default();
        host.compute_component_meta_state(canonical, mode, whole_hash)
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.runtime.store_resolved_meta(canonical, mode, result);
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
    pub timings: crate::component_meta_audit::RustTimingAudit,
    pub solver: crate::component_meta_audit::RustSolverAudit,
}

pub(crate) fn resolved_meta_cache_key(
    canonical: &str,
    mode: ProjectionMode,
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
    }
}
