//! Host-backed component-meta session layer.
//!
//! This replaces the public `MetaProject` / `MetaSession` naming, but keeps the
//! underlying behavioral contract that component-meta depends on:
//! - project/base state is shared
//! - sessions hold isolated overlays
//! - closing a session releases its overlays
//! - the selected type-expansion backend must never silently fall back

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::resolver_core::type_expansion::{
    ExpansionCompleteness, TypeExpansionBackend, TypeExpansionError, TypeExpansionRequest,
    TypeExpansionResult,
};
use crate::resolver_core::type_expansion_host::{
    ScriptLang, SfcBlockSpan, SfcStructure, SourceSnapshot, TypeExpansionHost,
    TypeExpansionSnapshot,
};
use crate::resolver_core::{
    component_meta_resolved_macros as resolver_component_meta_resolved_macros,
    component_meta_type_registry as resolver_component_meta_type_registry,
};
use verter_semantic::analysis::component_meta::{
    AcceptedSurfaceCompleteness, ComponentMetaAnalysis, FallthroughSurface, RootReachability,
};
use verter_semantic::analysis::type_expand::{
    ExpandedCallSignature, ExpandedComponentTypes, ExpandedField, ExpandedMacroObjectShape,
    ExpandedMacroProps, ExpandedObjectShape, ExpandedParameter, ExpandedProperty,
    ExpansionCompleteness as AnalysisExpansionCompleteness, ExpansionDiagnostic,
    ExpansionResult as AnalysisExpansionResult, ExpansionStopReason,
};
use verter_semantic::analysis::type_expr::{FunctionExpr, ObjectMember, PrimitiveName, TypeExpr};

use crate::host_manage::{component_meta_trace_event, component_meta_trace_scope};
use crate::VerterHost;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from ComponentMetaHost operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ComponentMetaHostError {
    #[error("host has been shut down")]
    Shutdown,
    #[error("host error: {0}")]
    Host(String),
}
impl From<crate::meta::MetaError> for ComponentMetaHostError {
    fn from(value: crate::meta::MetaError) -> Self {
        match value {
            crate::meta::MetaError::Shutdown => Self::Shutdown,
            crate::meta::MetaError::SessionClosed => Self::Host("session is closed".to_string()),
            crate::meta::MetaError::Host(message) => Self::Host(message),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentMetaTraceCursor {
    pub request_id: u64,
    pub span_id: u64,
    pub caller_id: Option<u64>,
    pub depth: usize,
}

pub trait ComponentMetaTypeExpander: Send + Sync {
    fn expand_type(
        &self,
        request: &TypeExpansionRequest,
        snapshot: TypeExpansionSnapshot,
        trace_cursor: Option<ComponentMetaTraceCursor>,
    ) -> Result<TypeExpansionResult, TypeExpansionError>;

    fn expand_slot_bindings(
        &self,
        _request: &TypeExpansionRequest,
        _snapshot: TypeExpansionSnapshot,
        _slot_name: &str,
        _trace_cursor: Option<ComponentMetaTraceCursor>,
    ) -> Result<Option<TypeExpansionResult>, TypeExpansionError> {
        Ok(None)
    }

    fn shutdown(&self) {}
}

// ---------------------------------------------------------------------------
// Shared inner state
// ---------------------------------------------------------------------------

struct ComponentMetaHostInner {
    project: Arc<crate::meta::MetaProject>,
    backend: TypeExpansionBackend,
    external_expander: parking_lot::RwLock<Option<Arc<dyn ComponentMetaTypeExpander>>>,
    generation: AtomicU64,
}

impl ComponentMetaHostInner {
    fn backend_error(&self) -> Option<ComponentMetaHostError> {
        match self.backend {
            TypeExpansionBackend::Verter => None,
            TypeExpansionBackend::Tsserver
            | TypeExpansionBackend::Tsgo
            | TypeExpansionBackend::Auto => {
                if self.external_expander.read().is_some() {
                    None
                } else {
                    let backend_name = match self.backend {
                        TypeExpansionBackend::Tsserver => "tsserver",
                        TypeExpansionBackend::Tsgo => "tsgo",
                        TypeExpansionBackend::Auto => "auto",
                        TypeExpansionBackend::Verter => unreachable!(),
                    };
                    Some(ComponentMetaHostError::Host(format!(
                        "type expansion backend '{backend_name}' is not connected"
                    )))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ComponentMetaHost
// ---------------------------------------------------------------------------

/// Shared component-meta host with isolated overlay sessions.
pub struct ComponentMetaHost {
    inner: Arc<ComponentMetaHostInner>,
}

impl ComponentMetaHost {
    /// Create a new component-meta host with a standalone memory workspace.
    pub fn new_standalone(config: crate::types::HostConfig) -> Self {
        let backend = config.type_expansion_backend;
        let project = crate::meta::MetaProject::new(VerterHost::new_standalone(config));
        Self {
            inner: Arc::new(ComponentMetaHostInner {
                project,
                backend,
                external_expander: parking_lot::RwLock::new(None),
                generation: AtomicU64::new(0),
            }),
        }
    }

    /// Create a new component-meta host backed by an existing workspace.
    pub fn new(
        config: crate::types::HostConfig,
        workspace: Arc<dyn verter_workspace::WorkspaceAccess>,
    ) -> Self {
        let backend = config.type_expansion_backend;
        #[cfg(not(target_arch = "wasm32"))]
        let host = VerterHost::new(config, workspace);
        #[cfg(target_arch = "wasm32")]
        let host = {
            let host = VerterHost::new(config);
            host.set_workspace(workspace);
            host
        };
        let project = crate::meta::MetaProject::new(host);
        Self {
            inner: Arc::new(ComponentMetaHostInner {
                project,
                backend,
                external_expander: parking_lot::RwLock::new(None),
                generation: AtomicU64::new(0),
            }),
        }
    }

    fn check_alive(&self) -> Result<(), ComponentMetaHostError> {
        if self.inner.project.is_shutdown() {
            return Err(ComponentMetaHostError::Shutdown);
        }
        Ok(())
    }

    /// Access the underlying host.
    pub fn host(&self) -> &VerterHost {
        self.inner.project.host()
    }

    /// Which backend is configured for type expansion.
    pub fn backend(&self) -> TypeExpansionBackend {
        self.inner.backend
    }

    /// Set an external type expander for non-Verter session queries.
    pub fn set_type_expander(&self, expander: Arc<dyn ComponentMetaTypeExpander>) {
        *self.inner.external_expander.write() = Some(expander);
    }

    /// Whether an external type expander is connected.
    pub fn has_external_expander(&self) -> bool {
        self.inner.external_expander.read().is_some()
    }

    /// Current base-state revision.
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::Acquire)
    }

    /// Load a file into the shared base project.
    pub fn upsert_base(
        &self,
        canonical_id: &str,
        source: &str,
    ) -> Result<(), ComponentMetaHostError> {
        self.check_alive()?;
        self.inner
            .project
            .upsert_base(canonical_id, source)
            .map_err(ComponentMetaHostError::from)?;
        self.inner.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }

    /// Ensure a workspace-backed base file is loaded.
    pub fn ensure_loaded(&self, canonical_id: &str) -> Result<bool, ComponentMetaHostError> {
        self.check_alive()?;
        let loaded = self
            .inner
            .project
            .ensure_loaded(canonical_id)
            .map_err(ComponentMetaHostError::from)?;
        if loaded {
            self.inner.generation.fetch_add(1, Ordering::Release);
        }
        Ok(loaded)
    }

    /// Refresh a workspace-backed base file from the current workspace.
    pub fn refresh_base(&self, canonical_id: &str) -> Result<bool, ComponentMetaHostError> {
        self.check_alive()?;
        let loaded = self
            .inner
            .project
            .refresh_base(canonical_id)
            .map_err(ComponentMetaHostError::from)?;
        self.inner.generation.fetch_add(1, Ordering::Release);
        Ok(loaded)
    }

    /// Open a new isolated session against this host.
    pub fn open_session(&self) -> Result<ComponentMetaSession, ComponentMetaHostError> {
        self.check_alive()?;
        Ok(ComponentMetaSession {
            inner: self
                .inner
                .project
                .open_session()
                .map_err(ComponentMetaHostError::from)?,
            owner: Arc::clone(&self.inner),
        })
    }

    /// Configure project-scoped path aliases.
    pub fn configure_projects(
        &self,
        configs: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig>,
    ) -> Result<(), ComponentMetaHostError> {
        self.check_alive()?;
        self.inner
            .project
            .configure_projects(configs)
            .map_err(ComponentMetaHostError::from)
    }

    /// Clear shared compile caches.
    pub fn clear_caches(&self) -> Result<(), ComponentMetaHostError> {
        self.check_alive()?;
        self.inner
            .project
            .clear_caches()
            .map_err(ComponentMetaHostError::from)
    }

    /// Terminal shutdown.
    pub fn shutdown(&self) {
        if let Some(expander) = self.inner.external_expander.read().as_ref() {
            expander.shutdown();
        }
        self.inner.project.shutdown();
    }

    /// Whether this host has been shut down.
    pub fn is_shutdown(&self) -> bool {
        self.inner.project.is_shutdown()
    }

    /// Canonical IDs of files in the shared base index.
    pub fn base_file_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.inner.project.base_file_ids().into_iter().collect();
        ids.sort();
        ids
    }

    /// Number of active sessions.
    pub fn session_count(&self) -> usize {
        self.inner.project.session_count()
    }
}

// ---------------------------------------------------------------------------
// ComponentMetaSession
// ---------------------------------------------------------------------------

/// Isolated overlay session backed by the shared host/project.
pub struct ComponentMetaSession {
    inner: crate::meta::MetaSession,
    owner: Arc<ComponentMetaHostInner>,
}

impl ComponentMetaSession {
    fn backend_gate(&self) -> Result<(), ComponentMetaHostError> {
        if let Some(err) = self.owner.backend_error() {
            return Err(err);
        }
        Ok(())
    }

    /// Store a file overlay in this session.
    pub fn upsert(&self, canonical_id: &str, source: String) -> Result<(), ComponentMetaHostError> {
        self.inner
            .upsert(canonical_id, source)
            .map_err(ComponentMetaHostError::from)
    }

    /// Tombstone a file in this session.
    pub fn delete(&self, canonical_id: &str) -> Result<(), ComponentMetaHostError> {
        self.inner
            .delete(canonical_id)
            .map_err(ComponentMetaHostError::from)
    }

    /// Clear a session-local overlay for a file, revealing the shared base.
    pub fn reset(&self, canonical_id: &str) -> Result<(), ComponentMetaHostError> {
        self.inner
            .reset(canonical_id)
            .map_err(ComponentMetaHostError::from)
    }

    /// Get the effective source for a file in this session.
    pub fn get_effective_source(
        &self,
        canonical_id: &str,
    ) -> Result<Option<String>, ComponentMetaHostError> {
        self.inner
            .get_effective_source(canonical_id)
            .map_err(ComponentMetaHostError::from)
    }

    /// Check whether a file is visible in this session.
    pub fn has_file(&self, canonical_id: &str) -> Result<bool, ComponentMetaHostError> {
        self.inner
            .has_file(canonical_id)
            .map_err(ComponentMetaHostError::from)
    }

    /// Get component metadata in this session's overlay context.
    pub fn get_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>,
        ComponentMetaHostError,
    > {
        self.get_component_meta_with_fallthrough(canonical_or_alias, true)
    }

    /// Get component metadata plus the resolved-state sidecar in this session's
    /// overlay context.
    pub fn get_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        Option<(
            verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
            crate::meta_resolve::ResolvedComponentMetaState,
        )>,
        ComponentMetaHostError,
    > {
        let _trace = component_meta_trace_scope!(
            "component_meta_session_query_with_resolution",
            format!(
                "backend={:?} owner={}",
                self.owner.backend, canonical_or_alias
            ),
        );
        match self.owner.backend {
            TypeExpansionBackend::Verter => self
                .inner
                .get_component_meta_with_resolution(canonical_or_alias)
                .map_err(ComponentMetaHostError::from),
            TypeExpansionBackend::Tsserver | TypeExpansionBackend::Tsgo => {
                let Some((canonical, resolved, store_view)) = self
                    .inner
                    .resolve_component_meta_state_with_view(
                        canonical_or_alias,
                        crate::types::ResolverMode::Expanded,
                    )
                    .map_err(ComponentMetaHostError::from)?
                else {
                    return Ok(None);
                };
                let Some(analysis) = self.get_component_meta_via_external_backend_from_resolved(
                    &canonical,
                    &resolved,
                    true,
                    Some(&store_view),
                )?
                else {
                    return Ok(None);
                };
                Ok(Some((analysis, resolved)))
            }
            TypeExpansionBackend::Auto => {
                let canonical = self
                    .inner
                    .resolve_alias_or_canonical(canonical_or_alias)
                    .map_err(ComponentMetaHostError::from)?;
                let Some((canonical, resolved, store_view)) = self
                    .inner
                    .resolve_component_meta_state_with_view(
                        &canonical,
                        crate::types::ResolverMode::Expanded,
                    )
                    .map_err(ComponentMetaHostError::from)?
                else {
                    return Ok(None);
                };

                let exceeds_threshold =
                    resolved_state_exceeds_verter_complexity_threshold(&resolved);
                let analysis = if exceeds_threshold {
                    let Some(analysis) = self
                        .get_component_meta_via_external_backend_from_resolved(
                            &canonical,
                            &resolved,
                            true,
                            Some(&store_view),
                        )?
                    else {
                        return Ok(None);
                    };
                    analysis
                } else {
                    extract_component_meta_from_resolved_with_evaluated(
                        self.owner.project.host(),
                        &canonical,
                        &resolved,
                        resolved.evaluated_types.as_ref(),
                        should_include_fallthrough_surface(&resolved),
                        Some(&store_view),
                    )
                };
                Ok(Some((analysis, resolved)))
            }
        }
    }

    /// Get declared-only component metadata in this session's overlay context.
    ///
    /// This skips accepted-surface and fallthrough resolution so compat callers
    /// can match Volar-style metadata without paying the inheritance cost.
    pub fn get_declared_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>,
        ComponentMetaHostError,
    > {
        self.get_component_meta_with_fallthrough(canonical_or_alias, false)
    }

    fn get_component_meta_with_fallthrough(
        &self,
        canonical_or_alias: &str,
        include_fallthrough: bool,
    ) -> Result<
        Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>,
        ComponentMetaHostError,
    > {
        let _trace = component_meta_trace_scope!(
            "component_meta_session_query",
            format!(
                "backend={:?} owner={} include_fallthrough={}",
                self.owner.backend, canonical_or_alias, include_fallthrough,
            ),
        );
        match self.owner.backend {
            TypeExpansionBackend::Verter if !include_fallthrough => self
                .inner
                .get_declared_component_meta(canonical_or_alias)
                .map_err(ComponentMetaHostError::from),
            TypeExpansionBackend::Verter => self
                .inner
                .get_component_meta(canonical_or_alias)
                .map_err(ComponentMetaHostError::from),
            TypeExpansionBackend::Tsserver | TypeExpansionBackend::Tsgo => self
                .get_component_meta_via_external_backend(canonical_or_alias, include_fallthrough),
            TypeExpansionBackend::Auto => {
                self.get_component_meta_via_auto_policy(canonical_or_alias, include_fallthrough)
            }
        }
    }

    /// Get the analysis snapshot in this session's overlay context.
    pub fn get_analysis(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<crate::types::FileAnalysisSnapshot>, ComponentMetaHostError> {
        self.inner
            .get_analysis(canonical_or_alias)
            .map_err(ComponentMetaHostError::from)
    }

    /// Get provenance counters for observability.
    pub fn get_provenance(
        &self,
    ) -> Result<crate::types::MetaProvenanceSnapshot, ComponentMetaHostError> {
        self.inner
            .get_provenance()
            .map_err(ComponentMetaHostError::from)
    }

    /// Canonical IDs visible to this session.
    pub fn tracked_file_ids(&self) -> Result<Vec<String>, ComponentMetaHostError> {
        self.inner
            .visible_file_ids()
            .map_err(ComponentMetaHostError::from)
    }

    /// Close the session, releasing its overlays.
    pub fn close(&self) {
        self.inner.close();
    }

    /// Whether this session has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Per-session overlay generation counter.
    pub fn overlay_generation(&self) -> u64 {
        self.inner.overlay_generation()
    }

    // `Auto` commits each request to one backend path based on the
    // resolved-state complexity signals, rather than querying external by default.
    fn get_component_meta_via_auto_policy(
        &self,
        canonical_or_alias: &str,
        include_fallthrough: bool,
    ) -> Result<Option<ComponentMetaAnalysis>, ComponentMetaHostError> {
        let _trace = component_meta_trace_scope!(
            "component_meta_auto_policy",
            format!(
                "owner={} include_fallthrough={}",
                canonical_or_alias, include_fallthrough,
            ),
        );
        let Some((canonical, resolved, store_view)) = self
            .inner
            .resolve_component_meta_state_with_view(
                canonical_or_alias,
                crate::types::ResolverMode::Expanded,
            )
            .map_err(ComponentMetaHostError::from)?
        else {
            return Ok(None);
        };

        let exceeds_threshold = resolved_state_exceeds_verter_complexity_threshold(&resolved);
        component_meta_trace_event!(
            "component_meta_auto_policy_decision",
            format!(
                "owner={} exceeds_threshold={} resolved_macros={} resolved_types={} has_evaluated_types={}",
                canonical,
                exceeds_threshold,
                resolved.resolved_macros.len(),
                resolved.resolved_type_registry.len(),
                resolved.evaluated_types.is_some(),
            ),
        );
        if exceeds_threshold {
            return self.get_component_meta_via_external_backend_from_resolved(
                &canonical,
                &resolved,
                include_fallthrough,
                Some(&store_view),
            );
        }

        Ok(Some(extract_component_meta_from_resolved_with_evaluated(
            self.owner.project.host(),
            &canonical,
            &resolved,
            resolved.evaluated_types.as_ref(),
            include_fallthrough && should_include_fallthrough_surface(&resolved),
            Some(&store_view),
        )))
    }

    fn get_component_meta_via_external_backend(
        &self,
        canonical_or_alias: &str,
        include_fallthrough: bool,
    ) -> Result<Option<ComponentMetaAnalysis>, ComponentMetaHostError> {
        let Some((canonical, resolved, store_view)) = self
            .inner
            .resolve_component_meta_state_with_view(
                canonical_or_alias,
                crate::types::ResolverMode::Expanded,
            )
            .map_err(ComponentMetaHostError::from)?
        else {
            return Ok(None);
        };
        self.get_component_meta_via_external_backend_from_resolved(
            &canonical,
            &resolved,
            include_fallthrough,
            Some(&store_view),
        )
    }

    fn get_component_meta_via_external_backend_from_resolved(
        &self,
        canonical: &str,
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
        include_fallthrough: bool,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Result<Option<ComponentMetaAnalysis>, ComponentMetaHostError> {
        let _trace = component_meta_trace_scope!(
            "component_meta_external_backend",
            format!(
                "backend={:?} owner={} include_fallthrough={} resolved_macros={} resolved_types={}",
                self.owner.backend,
                canonical,
                include_fallthrough,
                resolved.resolved_macros.len(),
                resolved.resolved_type_registry.len(),
            ),
        );
        self.backend_gate()?;
        let Some(expander) = self.owner.external_expander.read().clone() else {
            return Err(self.owner.backend_error().unwrap_or_else(|| {
                ComponentMetaHostError::Host("backend is not connected".to_string())
            }));
        };
        let Some(source) = self
            .inner
            .get_effective_source(canonical)
            .map_err(ComponentMetaHostError::from)?
        else {
            return Ok(None);
        };

        let snapshot = build_type_expansion_snapshot(
            canonical,
            &source,
            compose_snapshot_revision(
                self.owner.generation.load(Ordering::Acquire),
                self.inner.overlay_generation(),
            ),
        );
        let evaluated_types = build_external_component_types(
            expander.as_ref(),
            canonical,
            self.inner.id(),
            &source,
            &snapshot,
            resolved,
        )?;
        let include_fallthrough =
            include_fallthrough && should_include_fallthrough_surface(resolved);
        component_meta_trace_event!(
            "component_meta_external_backend_result",
            format!(
                "backend={:?} owner={} expanded_props={} expanded_events={} expanded_slots={} include_fallthrough={}",
                self.owner.backend,
                canonical,
                evaluated_types.props.len(),
                evaluated_types.emits.len(),
                evaluated_types.slot_bindings.len(),
                include_fallthrough,
            ),
        );

        Ok(Some(extract_component_meta_from_resolved_with_evaluated(
            self.owner.project.host(),
            canonical,
            resolved,
            (!evaluated_types.is_empty()).then_some(&evaluated_types),
            include_fallthrough,
            store_view,
        )))
    }
}

// ---------------------------------------------------------------------------
// VerterComponentMetaProvider implementation
// ---------------------------------------------------------------------------

impl crate::resolver_core::type_expansion_verter::VerterComponentMetaProvider
    for ComponentMetaHost
{
    fn get_component_meta(
        &self,
        canonical_id: &str,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        self.host().get_component_meta(canonical_id)
    }
}

// ---------------------------------------------------------------------------
// TypeExpansionHost implementation
// ---------------------------------------------------------------------------

impl TypeExpansionHost for ComponentMetaHost {
    fn snapshot_view(
        &self,
        canonical_id: &str,
    ) -> Result<TypeExpansionSnapshot, TypeExpansionError> {
        let source = self
            .host()
            .get_source(canonical_id)
            .ok_or(TypeExpansionError::SourceUnavailable)?;

        let lang = if canonical_id.ends_with(".tsx") {
            ScriptLang::Tsx
        } else if canonical_id.ends_with(".jsx") {
            ScriptLang::Jsx
        } else if canonical_id.ends_with(".js")
            || canonical_id.ends_with(".mjs")
            || canonical_id.ends_with(".cjs")
        {
            ScriptLang::Js
        } else {
            ScriptLang::Ts
        };

        let sfc_structure = extract_sfc_structure(&source);

        Ok(TypeExpansionSnapshot {
            source: SourceSnapshot {
                text: source.to_string(),
                lang,
            },
            sfc_structure,
            revision: self.generation(),
        })
    }
}

/// Extract SFC block spans from raw source text.
fn extract_sfc_structure(source: &str) -> SfcStructure {
    let mut script = None;
    let mut script_setup = None;
    let mut template = None;

    let mut pos = 0;
    let bytes = source.as_bytes();

    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            pos += 1;
            continue;
        }

        let rest = &source[pos..];
        if let Some(block) = try_extract_block(rest, "script setup", pos) {
            script_setup = Some(block);
            pos = block.content.end as usize + "</script>".len();
        } else if let Some(block) = try_extract_block(rest, "script", pos) {
            if script_setup.is_none() || block.content.start != script_setup.unwrap().content.start
            {
                script = Some(block);
            }
            pos = block.content.end as usize + "</script>".len();
        } else if let Some(block) = try_extract_block(rest, "template", pos) {
            template = Some(block);
            pos = block.content.end as usize + "</template>".len();
        } else {
            pos += 1;
        }
    }

    SfcStructure {
        script,
        script_setup,
        template,
    }
}

fn build_type_expansion_snapshot(
    canonical_id: &str,
    source: &str,
    revision: u64,
) -> TypeExpansionSnapshot {
    TypeExpansionSnapshot {
        source: SourceSnapshot {
            text: source.to_string(),
            lang: detect_script_lang(canonical_id),
        },
        sfc_structure: extract_sfc_structure(source),
        revision,
    }
}

fn detect_script_lang(canonical_id: &str) -> ScriptLang {
    if canonical_id.ends_with(".tsx") {
        ScriptLang::Tsx
    } else if canonical_id.ends_with(".jsx") {
        ScriptLang::Jsx
    } else if canonical_id.ends_with(".js")
        || canonical_id.ends_with(".mjs")
        || canonical_id.ends_with(".cjs")
    {
        ScriptLang::Js
    } else {
        ScriptLang::Ts
    }
}

fn compose_snapshot_revision(base_generation: u64, overlay_generation: u64) -> u64 {
    (base_generation << 32) ^ overlay_generation
}

fn should_include_fallthrough_surface(
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> bool {
    resolved
        .cached_eval_inputs
        .as_ref()
        .is_none_or(|inputs| inputs.overflow.is_none())
}

fn resolved_state_exceeds_verter_complexity_threshold(
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> bool {
    resolved
        .cached_eval_inputs
        .as_ref()
        .is_some_and(|inputs| inputs.overflow.is_some())
        || resolved
            .evaluated_types
            .as_ref()
            .is_some_and(component_meta_symbolic_budget_exceeded)
}

fn component_meta_symbolic_budget_exceeded(types: &ExpandedComponentTypes) -> bool {
    let field_has_budget = |field: &ExpandedField| {
        field
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.reason == ExpansionStopReason::BudgetExceeded)
    };
    let macro_has_budget = |shape: &ExpandedMacroObjectShape| {
        shape
            .result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.reason == ExpansionStopReason::BudgetExceeded)
    };
    let props_has_budget = |shape: &ExpandedMacroProps| {
        shape
            .result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.reason == ExpansionStopReason::BudgetExceeded)
    };

    types.props.iter().any(field_has_budget)
        || types.emits.iter().any(field_has_budget)
        || types.slot_bindings.iter().any(field_has_budget)
        || types.bindings.iter().any(field_has_budget)
        || types.define_props.iter().any(props_has_budget)
        || types.define_emits.iter().any(macro_has_budget)
        || types.define_slots.iter().any(macro_has_budget)
}

fn build_external_component_types(
    expander: &dyn ComponentMetaTypeExpander,
    canonical_id: &str,
    session_id: u64,
    source: &str,
    snapshot: &TypeExpansionSnapshot,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Result<ExpandedComponentTypes, ComponentMetaHostError> {
    let mut output = ExpandedComponentTypes::default();
    let synthetic_canonical = synthetic_expansion_canonical_id(canonical_id, session_id);

    let resolved_macro_map: HashMap<usize, &crate::meta_resolve::ResolvedMacroMeta> = resolved
        .resolved_macros
        .iter()
        .map(|meta| (meta.macro_index, meta))
        .collect();

    for (macro_index, mac) in resolved.snapshot.macros.iter().enumerate() {
        if !mac.is_type_based {
            continue;
        }

        let Some(type_span) = macro_type_argument_span(source, mac) else {
            continue;
        };

        let request = TypeExpansionRequest {
            canonical_id: synthetic_canonical.clone(),
            span: type_span,
            profile: crate::resolver_core::type_expansion::ExpansionProfile::ComponentMeta,
        };
        let _trace = component_meta_trace_scope!(
            "component_meta_external_macro",
            format!(
                "owner={} macro_index={} macro_kind={:?} span={}..{}",
                canonical_id, macro_index, mac.kind, type_span.start, type_span.end,
            ),
        );
        let expansion = expander
            .expand_type(
                &request,
                snapshot.clone(),
                crate::host_manage::current_component_meta_trace_cursor(),
            )
            .map_err(external_expansion_error)?;

        if matches!(
            mac.kind,
            verter_semantic::analysis::types::AnalyzedMacroKind::DefineSlots
        ) {
            collect_external_slot_binding_fields(
                &mut output,
                expander,
                &request,
                snapshot,
                &expansion,
            )?;
        }

        apply_type_expansion_result(
            &mut output,
            macro_index,
            mac,
            resolved_macro_map.get(&macro_index).copied(),
            expansion,
        );
    }

    Ok(output)
}

fn collect_external_slot_binding_fields(
    output: &mut ExpandedComponentTypes,
    expander: &dyn ComponentMetaTypeExpander,
    request: &TypeExpansionRequest,
    snapshot: &TypeExpansionSnapshot,
    expansion: &TypeExpansionResult,
) -> Result<(), ComponentMetaHostError> {
    let slot_names = type_expansion_members(expansion)
        .into_iter()
        .map(|member| member.name)
        .collect::<Vec<_>>();
    component_meta_trace_event!(
        "component_meta_external_slot_binding_slots",
        format!(
            "owner={} slot_count={} slots={}",
            request.canonical_id,
            slot_names.len(),
            slot_names.join(","),
        ),
    );
    for slot_name in slot_names {
        component_meta_trace_event!(
            "component_meta_external_slot_binding_query",
            format!("owner={} slot={slot_name}", request.canonical_id),
        );
        let Some(slot_bindings) = expander
            .expand_slot_bindings(
                request,
                snapshot.clone(),
                &slot_name,
                crate::host_manage::current_component_meta_trace_cursor(),
            )
            .map_err(external_expansion_error)?
        else {
            continue;
        };
        output
            .slot_bindings
            .extend(
                type_expansion_members(&slot_bindings)
                    .into_iter()
                    .map(|member| ExpandedField {
                        name: format!("{}.{}", slot_name, member.name),
                        r#type: member.type_expr,
                        raw_type: member.raw_type,
                        optional: member.optional,
                        completeness: analysis_completeness(slot_bindings.completeness),
                        diagnostics: expansion_diagnostics(
                            slot_bindings.completeness,
                            format!("type expansion for slot binding {slot_name}"),
                            None,
                        ),
                    }),
            );
    }
    Ok(())
}

fn type_expansion_members(
    expansion: &TypeExpansionResult,
) -> Vec<crate::resolver_core::type_expansion::ExpandedMember> {
    if !expansion.members.is_empty() {
        return expansion.members.clone();
    }

    match &expansion.type_expr {
        TypeExpr::Object(object) => object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => {
                    Some(crate::resolver_core::type_expansion::ExpandedMember {
                        name: property.name.clone(),
                        type_expr: property.ty.clone(),
                        raw_type: None,
                        optional: property.optional,
                        description: None,
                    })
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn external_expansion_error(error: TypeExpansionError) -> ComponentMetaHostError {
    ComponentMetaHostError::Host(format!("external type expansion failed: {error}"))
}

fn apply_type_expansion_result(
    output: &mut ExpandedComponentTypes,
    macro_index: usize,
    mac: &verter_semantic::analysis::types::AnalyzedMacro,
    resolved_macro: Option<&crate::meta_resolve::ResolvedMacroMeta>,
    expansion: TypeExpansionResult,
) {
    match mac.kind {
        verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps
        | verter_semantic::analysis::types::AnalyzedMacroKind::WithDefaults => {
            for member in &expansion.members {
                output.props.push(ExpandedField {
                    name: member.name.clone(),
                    r#type: member.type_expr.clone(),
                    raw_type: member.raw_type.clone(),
                    optional: member.optional,
                    completeness: analysis_completeness(expansion.completeness),
                    diagnostics: expansion_diagnostics(
                        expansion.completeness,
                        format!("type expansion for prop {}", member.name),
                        Some(member.name.clone()),
                    ),
                });
            }
            output.define_props.push(ExpandedMacroProps {
                macro_index,
                result: expansion_result_to_object_shape(expansion),
            });
        }
        verter_semantic::analysis::types::AnalyzedMacroKind::DefineEmits => {
            let emit_fields = merged_emit_fields(mac, resolved_macro);
            if emit_fields.is_empty() {
                output.define_emits.push(ExpandedMacroObjectShape {
                    macro_index,
                    result: expansion_result_to_object_shape(expansion),
                });
                return;
            }

            let completeness = analysis_completeness(expansion.completeness);
            let diagnostics = expansion_diagnostics(
                expansion.completeness,
                "external type expansion".to_string(),
                None,
            );
            let members_by_name: HashMap<_, _> = expansion
                .members
                .iter()
                .map(|member| (member.name.as_str(), member))
                .collect();
            let expanded_events: Vec<_> = emit_fields
                .iter()
                .map(|field| {
                    expanded_emit_field_from_source(
                        field,
                        members_by_name.get(field.name.as_str()).copied(),
                        completeness,
                        &diagnostics,
                    )
                })
                .collect();

            output.emits.extend(expanded_events.iter().cloned());
            output.define_emits.push(ExpandedMacroObjectShape {
                macro_index,
                result: AnalysisExpansionResult {
                    value: ExpandedObjectShape {
                        properties: expanded_events
                            .iter()
                            .map(|field| ExpandedProperty {
                                name: field.name.clone(),
                                ty: field.r#type.clone(),
                                optional: field.optional,
                                readonly: false,
                            })
                            .collect(),
                        index_signatures: Vec::new(),
                        call_signatures: Vec::new(),
                    },
                    completeness,
                    diagnostics,
                },
            });
        }
        verter_semantic::analysis::types::AnalyzedMacroKind::DefineSlots => {
            output.define_slots.push(ExpandedMacroObjectShape {
                macro_index,
                result: expansion_result_to_object_shape(expansion),
            });
        }
        verter_semantic::analysis::types::AnalyzedMacroKind::DefineModel => {
            let field_name = mac
                .model_name
                .clone()
                .or_else(|| {
                    resolved_macro
                        .and_then(|meta| meta.props.first().map(|field| field.name.clone()))
                })
                .unwrap_or_else(|| "modelValue".to_string());
            let field = ExpandedField {
                name: field_name.clone(),
                r#type: expansion.type_expr.clone(),
                raw_type: None,
                optional: false,
                completeness: analysis_completeness(expansion.completeness),
                diagnostics: expansion_diagnostics(
                    expansion.completeness,
                    format!("type expansion for defineModel<{field_name}>"),
                    None,
                ),
            };
            output.props.push(field.clone());
            output.emits.push(ExpandedField {
                name: format!("update:{field_name}"),
                r#type: TypeExpr::Tuple {
                    elements: Arc::from(vec![verter_semantic::analysis::type_expr::TupleElement {
                        label: Some("value".to_string()),
                        ty: field.r#type,
                        optional: false,
                        rest: false,
                    }]),
                    readonly: false,
                },
                raw_type: None,
                optional: false,
                completeness: field.completeness,
                diagnostics: field.diagnostics,
            });
        }
        _ => {}
    }
}

fn merged_emit_fields(
    mac: &verter_semantic::analysis::types::AnalyzedMacro,
    resolved_macro: Option<&crate::meta_resolve::ResolvedMacroMeta>,
) -> Vec<verter_semantic::analysis::types::AnalyzedEmitField> {
    let mut fields = mac.emit_fields.clone();
    let mut seen: std::collections::HashSet<String> =
        fields.iter().map(|field| field.name.clone()).collect();
    if let Some(resolved_macro) = resolved_macro {
        for emit in &resolved_macro.emits {
            if seen.insert(emit.name.clone()) {
                fields.push(emit.clone());
            }
        }
    }
    fields
}

fn expanded_emit_field_from_source(
    field: &verter_semantic::analysis::types::AnalyzedEmitField,
    member: Option<&crate::resolver_core::type_expansion::ExpandedMember>,
    completeness: AnalysisExpansionCompleteness,
    diagnostics: &[ExpansionDiagnostic],
) -> ExpandedField {
    let source_payload = field
        .payload_type
        .as_deref()
        .map(str::trim)
        .filter(|payload| !payload.is_empty());
    if let Some(member) = member {
        if !source_payload
            .is_some_and(|payload| source_emit_payload_beats_backend_member(member, payload))
        {
            return ExpandedField {
                name: field.name.clone(),
                r#type: member.type_expr.clone(),
                raw_type: member.raw_type.clone().or_else(|| {
                    source_payload
                        .and_then(strip_event_tuple_wrapper)
                        .map(str::to_string)
                }),
                optional: member.optional,
                completeness,
                diagnostics: diagnostics.to_vec(),
            };
        }
    }

    let source_type = source_payload
        .map(crate::resolver_core::type_text_parser::parse_type_text)
        .unwrap_or_else(|| TypeExpr::Unknown {
            raw: "unknown".to_string(),
        });
    ExpandedField {
        name: field.name.clone(),
        r#type: source_type,
        raw_type: source_payload
            .and_then(strip_event_tuple_wrapper)
            .map(str::to_string)
            .or_else(|| source_payload.map(str::to_string)),
        optional: false,
        completeness,
        diagnostics: diagnostics.to_vec(),
    }
}

fn source_emit_payload_beats_backend_member(
    member: &crate::resolver_core::type_expansion::ExpandedMember,
    source_payload: &str,
) -> bool {
    let source_inner = strip_event_tuple_wrapper(source_payload)
        .unwrap_or(source_payload)
        .trim();
    if source_inner.is_empty() || matches!(source_inner, "any" | "unknown") {
        return false;
    }
    if !matches!(member.type_expr, TypeExpr::Tuple { .. }) {
        return true;
    }

    let Some(raw_type) = member.raw_type.as_deref().map(str::trim) else {
        return false;
    };
    raw_type.is_empty()
        || matches!(raw_type, "any" | "unknown")
        || (source_payload.contains(" extends ") && !raw_type.contains(" extends "))
        || (source_payload.contains('[')
            && source_payload.contains(']')
            && (raw_type.starts_with('{') || raw_type.contains('\n')))
}

fn strip_event_tuple_wrapper(payload: &str) -> Option<&str> {
    let payload = payload.trim();
    let payload = payload.strip_prefix("[value:")?;
    let payload = payload.strip_suffix(']')?;
    Some(payload.trim())
}

fn expansion_result_to_object_shape(
    expansion: TypeExpansionResult,
) -> AnalysisExpansionResult<ExpandedObjectShape> {
    AnalysisExpansionResult {
        value: expanded_object_shape_from_type_expansion(&expansion),
        completeness: analysis_completeness(expansion.completeness),
        diagnostics: expansion_diagnostics(
            expansion.completeness,
            "external type expansion".to_string(),
            None,
        ),
    }
}

fn expanded_object_shape_from_type_expansion(
    expansion: &TypeExpansionResult,
) -> ExpandedObjectShape {
    let mut shape = ExpandedObjectShape::empty();

    if !expansion.members.is_empty() {
        shape.properties = expansion
            .members
            .iter()
            .map(|member| ExpandedProperty {
                name: member.name.clone(),
                ty: member.type_expr.clone(),
                optional: member.optional,
                readonly: false,
            })
            .collect();
        return shape;
    }

    match &expansion.type_expr {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => shape.properties.push(ExpandedProperty {
                        name: prop.name.clone(),
                        ty: prop.ty.clone(),
                        optional: prop.optional,
                        readonly: prop.readonly,
                    }),
                    ObjectMember::IndexSignature(sig) => {
                        shape.index_signatures.push(
                            verter_semantic::analysis::type_expand::ExpandedIndexSignature {
                                key_type: sig.key_type.clone(),
                                value_type: sig.value_type.clone(),
                                readonly: sig.readonly,
                            },
                        );
                    }
                    ObjectMember::CallSignature(function)
                    | ObjectMember::ConstructSignature(function) => {
                        shape
                            .call_signatures
                            .push(expanded_call_signature(function));
                    }
                    ObjectMember::Method(method) => {
                        shape.properties.push(ExpandedProperty {
                            name: method.name.clone(),
                            ty: TypeExpr::Function(Arc::new(method.function.clone())),
                            optional: method.optional,
                            readonly: false,
                        });
                    }
                }
            }
        }
        TypeExpr::Function(function) => {
            shape
                .call_signatures
                .push(expanded_call_signature(function));
        }
        _ => {}
    }

    shape
}

fn expanded_call_signature(function: &FunctionExpr) -> ExpandedCallSignature {
    ExpandedCallSignature {
        parameters: function
            .parameters
            .iter()
            .map(|param| ExpandedParameter {
                name: param.name.clone().unwrap_or_default(),
                ty: param.ty.clone(),
                optional: param.optional,
                rest: param.rest,
            })
            .collect(),
        return_type: function
            .return_type
            .as_ref()
            .map(|ty| ty.as_ref().clone())
            .unwrap_or_else(|| TypeExpr::primitive(PrimitiveName::Void)),
        type_parameters: function.type_parameters.clone(),
    }
}

fn analysis_completeness(completeness: ExpansionCompleteness) -> AnalysisExpansionCompleteness {
    match completeness {
        ExpansionCompleteness::Exact => AnalysisExpansionCompleteness::Exact,
        ExpansionCompleteness::LowerBound | ExpansionCompleteness::OpaqueFallback => {
            AnalysisExpansionCompleteness::Partial
        }
    }
}

fn expansion_diagnostics(
    completeness: ExpansionCompleteness,
    context: String,
    property_name: Option<String>,
) -> Vec<ExpansionDiagnostic> {
    match completeness {
        ExpansionCompleteness::Exact => Vec::new(),
        ExpansionCompleteness::LowerBound => vec![ExpansionDiagnostic {
            reason: ExpansionStopReason::UnresolvedReference,
            context,
            property_name,
        }],
        ExpansionCompleteness::OpaqueFallback => vec![ExpansionDiagnostic {
            reason: ExpansionStopReason::UnsupportedOperator,
            context,
            property_name,
        }],
    }
}

fn synthetic_expansion_canonical_id(canonical_id: &str, session_id: u64) -> String {
    format!("{canonical_id}.__verter_session_{session_id}")
}

fn macro_type_argument_span(
    source: &str,
    mac: &verter_semantic::analysis::types::AnalyzedMacro,
) -> Option<verter_span::Span> {
    let start = mac.span.start as usize;
    let end = mac.span.end as usize;
    let span_text = source.get(start..end)?;

    let macro_name = match mac.kind {
        verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_semantic::analysis::types::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_semantic::analysis::types::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_semantic::analysis::types::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_semantic::analysis::types::AnalyzedMacroKind::WithDefaults => "defineProps",
        _ => return None,
    };

    let name_offset = span_text.find(macro_name)?;
    let mut search_offset = name_offset + macro_name.len();
    while let Some(ch) = span_text[search_offset..].chars().next() {
        if ch.is_whitespace() {
            search_offset += ch.len_utf8();
            continue;
        }
        break;
    }
    let lt_rel = search_offset + span_text[search_offset..].find('<')?;
    let gt_rel = matching_angle_bracket(span_text, lt_rel)?;

    let mut inner_start = lt_rel + 1;
    let mut inner_end = gt_rel;
    while span_text[inner_start..inner_end]
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
    {
        inner_start += span_text[inner_start..inner_end]
            .chars()
            .next()
            .unwrap()
            .len_utf8();
    }
    while span_text[..inner_end]
        .chars()
        .next_back()
        .is_some_and(char::is_whitespace)
    {
        inner_end -= span_text[..inner_end]
            .chars()
            .next_back()
            .unwrap()
            .len_utf8();
    }

    (inner_start < inner_end)
        .then(|| verter_span::Span::new((start + inner_start) as u32, (start + inner_end) as u32))
}

fn matching_angle_bracket(text: &str, lt_rel: usize) -> Option<usize> {
    let mut angle_depth = 0u32;
    let mut paren_depth = 0u32;
    let mut brace_depth = 0u32;
    let mut bracket_depth = 0u32;
    let mut in_string = false;
    let mut string_delim = '\0';
    let mut escape = false;

    for (offset, ch) in text[lt_rel..].char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == string_delim {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                string_delim = ch;
            }
            '<' => angle_depth += 1,
            '>' => {
                angle_depth = angle_depth.saturating_sub(1);
                if angle_depth == 0 && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                    return Some(lt_rel + offset);
                }
            }
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ => {}
        }
    }

    None
}

fn extract_component_meta_from_resolved_with_evaluated(
    host: &VerterHost,
    canonical_id: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    evaluated_types: Option<&ExpandedComponentTypes>,
    include_fallthrough: bool,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> ComponentMetaAnalysis {
    let resolved_macros = resolver_component_meta_resolved_macros(
        resolved.snapshot.macros.as_ref(),
        &resolved.resolved_macros,
    );
    let resolved_type_registry =
        resolver_component_meta_type_registry(&resolved.resolved_type_registry);
    let input = verter_semantic::analysis::component_meta::ComponentMetaInput {
        macros: &resolved.snapshot.macros,
        bindings: &resolved.snapshot.bindings,
        imports: &resolved.snapshot.imports,
        template: resolved.snapshot.template.as_deref(),
        options_api: resolved.snapshot.options_api.as_ref(),
        analysis_flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
            resolved.snapshot.script_flags,
        ),
        styles: &resolved.snapshot.styles,
        vue_api_calls: &resolved.snapshot.vue_api_calls,
        store_usages: &resolved.snapshot.store_usages,
        resolved_macros: &resolved_macros,
        resolved_type_registry: &resolved_type_registry,
        evaluated_types,
        file_path: canonical_id,
    };

    let mut meta = verter_semantic::analysis::component_meta::extract_component_meta(input);
    if include_fallthrough {
        let mut visiting = rustc_hash::FxHashSet::default();
        if let Some(resolution) = host.compute_fallthrough_surface_from_resolved_state(
            canonical_id,
            resolved,
            None,
            &mut visiting,
            store_view,
        ) {
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
        } else {
            meta.accepted_surface_completeness = AcceptedSurfaceCompleteness::LowerBound;
            meta.root_reachability = RootReachability::NoFallthrough {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
            };
            meta.fallthrough_surface = FallthroughSurface::None {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
            };
        }
    }

    crate::host_manage::populate_public_instance_sidecar(&mut meta);
    crate::host_manage::populate_sfc_blocks_sidecar(host, canonical_id, &mut meta);
    meta
}

/// Try to extract a block's content span from `<tag ...>content</tag>`.
fn try_extract_block(rest: &str, tag: &str, base_offset: usize) -> Option<SfcBlockSpan> {
    let open_prefix = format!("<{}", tag);
    if !rest.starts_with(&open_prefix) {
        return None;
    }

    let after_tag = &rest[open_prefix.len()..];
    if !after_tag.is_empty() {
        let next = after_tag.as_bytes()[0];
        if next != b' ' && next != b'>' && next != b'\n' && next != b'\r' && next != b'\t' {
            return None;
        }
    }

    let gt_pos = rest.find('>')?;
    let content_start = base_offset + gt_pos + 1;

    let close_tag = format!("</{}>", tag.split_whitespace().next().unwrap_or(tag));
    let close_pos = rest.find(&close_tag)?;
    let content_end = base_offset + close_pos;

    Some(SfcBlockSpan {
        content: verter_span::Span::new(content_start as u32, content_end as u32),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};
    use std::sync::Arc;

    struct FakeTypeExpander {
        requests: parking_lot::Mutex<Vec<TypeExpansionRequest>>,
        slot_binding_requests: parking_lot::Mutex<Vec<(TypeExpansionRequest, String)>>,
        trace_cursors: parking_lot::Mutex<Vec<Option<ComponentMetaTraceCursor>>>,
        result: TypeExpansionResult,
        slot_binding_results: HashMap<String, TypeExpansionResult>,
    }

    impl FakeTypeExpander {
        fn object_with_members(members: Vec<(&str, TypeExpr, bool)>) -> Self {
            Self {
                requests: parking_lot::Mutex::new(Vec::new()),
                slot_binding_requests: parking_lot::Mutex::new(Vec::new()),
                trace_cursors: parking_lot::Mutex::new(Vec::new()),
                result: TypeExpansionResult {
                    type_expr: TypeExpr::Object(Arc::new(
                        verter_semantic::analysis::type_expr::ObjectExpr {
                            properties: members
                                .iter()
                                .map(|(name, ty, optional)| {
                                    ObjectMember::Property(
                                        verter_semantic::analysis::type_expr::ObjectProperty {
                                            name: (*name).to_string(),
                                            ty: ty.clone(),
                                            optional: *optional,
                                            readonly: false,
                                        },
                                    )
                                })
                                .collect(),
                        },
                    )),
                    members: members
                        .into_iter()
                        .map(|(name, type_expr, optional)| {
                            crate::resolver_core::type_expansion::ExpandedMember {
                                name: name.to_string(),
                                type_expr,
                                raw_type: None,
                                optional,
                                description: None,
                            }
                        })
                        .collect(),
                    completeness: ExpansionCompleteness::Exact,
                },
                slot_binding_results: HashMap::new(),
            }
        }

        fn with_slot_bindings(
            mut self,
            slot_name: &str,
            members: Vec<(&str, TypeExpr, bool)>,
        ) -> Self {
            let result = TypeExpansionResult {
                type_expr: TypeExpr::Object(Arc::new(
                    verter_semantic::analysis::type_expr::ObjectExpr {
                        properties: members
                            .iter()
                            .map(|(name, ty, optional)| {
                                ObjectMember::Property(
                                    verter_semantic::analysis::type_expr::ObjectProperty {
                                        name: (*name).to_string(),
                                        ty: ty.clone(),
                                        optional: *optional,
                                        readonly: false,
                                    },
                                )
                            })
                            .collect(),
                    },
                )),
                members: members
                    .into_iter()
                    .map(|(name, type_expr, optional)| {
                        crate::resolver_core::type_expansion::ExpandedMember {
                            name: name.to_string(),
                            type_expr,
                            raw_type: None,
                            optional,
                            description: None,
                        }
                    })
                    .collect(),
                completeness: ExpansionCompleteness::Exact,
            };
            self.slot_binding_results
                .insert(slot_name.to_string(), result);
            self
        }
    }

    impl ComponentMetaTypeExpander for FakeTypeExpander {
        fn expand_type(
            &self,
            request: &TypeExpansionRequest,
            _snapshot: TypeExpansionSnapshot,
            trace_cursor: Option<ComponentMetaTraceCursor>,
        ) -> Result<TypeExpansionResult, TypeExpansionError> {
            self.requests.lock().push(request.clone());
            self.trace_cursors.lock().push(trace_cursor);
            Ok(self.result.clone())
        }

        fn expand_slot_bindings(
            &self,
            request: &TypeExpansionRequest,
            _snapshot: TypeExpansionSnapshot,
            slot_name: &str,
            _trace_cursor: Option<ComponentMetaTraceCursor>,
        ) -> Result<Option<TypeExpansionResult>, TypeExpansionError> {
            self.slot_binding_requests
                .lock()
                .push((request.clone(), slot_name.to_string()));
            Ok(self.slot_binding_results.get(slot_name).cloned())
        }
    }

    fn make_host() -> ComponentMetaHost {
        ComponentMetaHost::new_standalone(crate::types::HostConfig::default())
    }

    #[test]
    fn upsert_base_and_get_source() {
        let host = make_host();
        host.upsert_base("/src/Foo.vue", "<template><div/></template>")
            .unwrap();
        let session = host.open_session().unwrap();
        let source = session.get_effective_source("/src/Foo.vue").unwrap();
        assert!(source.is_some());
        assert!(source.unwrap().contains("<template>"));
    }

    #[test]
    fn session_overlays_are_isolated() {
        let host = make_host();
        host.upsert_base("/src/Foo.vue", "<template><div/></template>")
            .unwrap();
        let session_a = host.open_session().unwrap();
        let session_b = host.open_session().unwrap();

        session_a
            .upsert("/src/Foo.vue", "<template><span/></template>".to_string())
            .unwrap();

        assert_eq!(
            session_a.get_effective_source("/src/Foo.vue").unwrap(),
            Some("<template><span/></template>".to_string())
        );
        assert_eq!(
            session_b.get_effective_source("/src/Foo.vue").unwrap(),
            Some("<template><div/></template>".to_string())
        );
    }

    #[test]
    fn closing_session_reverts_its_overlays() {
        let host = make_host();
        host.upsert_base("/src/Foo.vue", "<template><div/></template>")
            .unwrap();

        let session_a = host.open_session().unwrap();
        session_a
            .upsert("/src/Foo.vue", "<template><span/></template>".to_string())
            .unwrap();
        session_a.close();

        let session_b = host.open_session().unwrap();
        assert_eq!(
            session_b.get_effective_source("/src/Foo.vue").unwrap(),
            Some("<template><div/></template>".to_string())
        );
    }

    #[test]
    fn shutdown_prevents_further_operations() {
        let host = make_host();
        host.shutdown();
        assert!(host.is_shutdown());
        assert!(host.upsert_base("/src/X.vue", "").is_err());
    }

    #[test]
    fn backend_defaults_to_verter() {
        let host = make_host();
        assert_eq!(host.backend(), TypeExpansionBackend::Verter);
    }

    #[test]
    fn backend_respects_config() {
        let mut config = crate::types::HostConfig::default();
        config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let host = ComponentMetaHost::new_standalone(config);
        assert_eq!(host.backend(), TypeExpansionBackend::Tsgo);
    }

    #[test]
    fn get_component_meta_returns_none_for_missing() {
        let host = make_host();
        let session = host.open_session().unwrap();
        let result = session.get_component_meta("/nonexistent.vue").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_component_meta_returns_some_for_loaded_sfc() {
        let host = make_host();
        host.upsert_base(
            "/src/Button.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n</script>\n<template><div>{{ msg }}</div></template>",
        )
        .unwrap();
        let session = host.open_session().unwrap();
        let result = session.get_component_meta("/src/Button.vue").unwrap();
        assert!(result.is_some(), "should return meta for loaded SFC");
    }

    #[test]
    fn declared_component_meta_skips_fallthrough_surface() {
        let host = make_host();
        host.upsert_base(
            "/src/App.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n</script>\n<template><div>{{ msg }}</div></template>",
        )
        .unwrap();

        let session = host.open_session().unwrap();
        let full = session
            .get_component_meta("/src/App.vue")
            .unwrap()
            .expect("full query should return component meta");
        let declared = session
            .get_declared_component_meta("/src/App.vue")
            .unwrap()
            .expect("declared query should return component meta");

        assert!(
            full.accepted_props.iter().any(|prop| prop.name == "id"),
            "full metadata should include inherited attrs from the root element"
        );
        assert!(
            full.accepted_events
                .iter()
                .any(|event| event.name == "click"),
            "full metadata should include inherited listeners from the root element"
        );
        assert!(
            declared.accepted_props.is_empty(),
            "declared-only metadata should skip accepted props, got {:?}",
            declared
                .accepted_props
                .iter()
                .map(|prop| prop.name.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            declared.accepted_events.is_empty(),
            "declared-only metadata should skip accepted events, got {:?}",
            declared
                .accepted_events
                .iter()
                .map(|event| event.name.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            !matches!(
                declared.fallthrough_surface,
                verter_semantic::analysis::component_meta::FallthroughSurface::Branches { .. }
            ),
            "declared-only metadata should skip fallthrough branches, got {:?}",
            declared.fallthrough_surface
        );
    }

    #[test]
    fn non_verter_backend_without_expander_errors_instead_of_falling_back() {
        let mut config = crate::types::HostConfig::default();
        config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let host = ComponentMetaHost::new_standalone(config);
        host.upsert_base(
            "/src/Button.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n</script>\n<template><div>{{ msg }}</div></template>",
        )
        .unwrap();

        let session = host.open_session().unwrap();
        let err = session
            .get_component_meta("/src/Button.vue")
            .expect_err("unwired backend must not silently fall back to Verter");

        match err {
            ComponentMetaHostError::Host(message) => {
                assert!(
                    message.contains("backend")
                        && (message.contains("not connected")
                            || message.contains("not yet integrated")),
                    "error should explain backend wiring failure, got: {message}"
                );
            }
            other => panic!("expected host error, got {other:?}"),
        }
    }

    #[test]
    fn non_verter_backend_with_external_expander_returns_component_meta() {
        let mut config = crate::types::HostConfig::default();
        config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let host = ComponentMetaHost::new_standalone(config);
        let fake = Arc::new(FakeTypeExpander::object_with_members(vec![
            ("msg", TypeExpr::primitive(PrimitiveName::String), false),
            ("count", TypeExpr::primitive(PrimitiveName::Number), true),
        ]));
        host.set_type_expander(fake.clone());
        host.upsert_base(
            "/src/types.ts",
            "export interface Props { msg: string; count?: number }",
        )
        .unwrap();
        host.upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();
        host.host().set_import_dependencies(
            "/src/Button.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let session = host.open_session().unwrap();
        let result = session
            .get_component_meta("/src/Button.vue")
            .unwrap()
            .expect("external backend should return component meta");

        let props: BTreeSet<_> = result.props.iter().map(|prop| prop.name.as_str()).collect();
        assert_eq!(props, BTreeSet::from(["count", "msg"]));

        let request = fake
            .requests
            .lock()
            .first()
            .cloned()
            .expect("external expander should be invoked");
        assert!(
            request.canonical_id.contains(".__verter_session_"),
            "request should use a session-scoped generated identity, got: {}",
            request.canonical_id
        );
    }

    #[test]
    fn non_verter_backend_can_supply_slot_bindings_via_external_expander() {
        let mut config = crate::types::HostConfig::default();
        config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let host = ComponentMetaHost::new_standalone(config);
        let fake = Arc::new(
            FakeTypeExpander::object_with_members(vec![(
                "leading",
                TypeExpr::named("SlotProps"),
                true,
            )])
            .with_slot_bindings(
                "leading",
                vec![
                    ("item", TypeExpr::primitive(PrimitiveName::String), false),
                    ("index", TypeExpr::primitive(PrimitiveName::Number), false),
                ],
            ),
        );
        host.set_type_expander(fake.clone());
        host.upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
defineSlots<{
  leading?: (props: { item: string; index: number }) => any
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

        let session = host.open_session().unwrap();
        let result = session
            .get_component_meta("/src/Button.vue")
            .unwrap()
            .expect("external backend should return component meta");

        let leading = result
            .slots
            .iter()
            .find(|slot| slot.name == "leading")
            .expect("leading slot should be present");
        let bindings: BTreeSet<_> = leading
            .bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect();
        assert_eq!(
            bindings,
            BTreeSet::from(["index", "item"]),
            "slot bindings should come from the external slot-binding query, got {:?}",
            leading
                .bindings
                .iter()
                .map(|binding| binding.name.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            fake.slot_binding_requests.lock().len(),
            1,
            "defineSlots external path should request slot bindings for alias-based slot props"
        );
    }

    #[test]
    fn non_verter_backend_prefers_raw_emit_payload_over_macro_return_shape() {
        let mut config = crate::types::HostConfig::default();
        config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let host = ComponentMetaHost::new_standalone(config);
        let fake = Arc::new(FakeTypeExpander::object_with_members(vec![(
            "update:modelValue",
            TypeExpr::Object(Arc::new(verter_semantic::analysis::type_expr::ObjectExpr {
                properties: vec![ObjectMember::Property(
                    verter_semantic::analysis::type_expr::ObjectProperty {
                        name: "$props".to_string(),
                        ty: TypeExpr::Object(Arc::new(
                            verter_semantic::analysis::type_expr::ObjectExpr {
                                properties: Vec::new(),
                            },
                        )),
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
            false,
        )]));
        host.set_type_expander(fake);
        host.upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
defineEmits<{
  'update:modelValue': [value: string | number]
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

        let session = host.open_session().unwrap();
        let result = session
            .get_component_meta("/src/Button.vue")
            .unwrap()
            .expect("external backend should return component meta");

        let event = result
            .events
            .iter()
            .find(|event| event.name == "update:modelValue")
            .expect("update:modelValue should be present");
        assert!(
            matches!(event.payload, TypeExpr::Tuple { .. }),
            "event payload should come from the raw emit tuple, got {:?}",
            event.payload
        );
        assert_eq!(
            event.raw_signature.as_deref(),
            Some("[value: string | number]")
        );
    }

    #[test]
    fn non_verter_backend_queries_slot_bindings_for_alias_slots() {
        let mut config = crate::types::HostConfig::default();
        config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let host = ComponentMetaHost::new_standalone(config);
        let fake = Arc::new(
            FakeTypeExpander::object_with_members(vec![(
                "leading",
                TypeExpr::named("SlotProps"),
                true,
            )])
            .with_slot_bindings(
                "leading",
                vec![
                    ("item", TypeExpr::primitive(PrimitiveName::String), false),
                    ("index", TypeExpr::primitive(PrimitiveName::Number), false),
                ],
            ),
        );
        host.set_type_expander(fake.clone());
        host.upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
type SlotProps = (props: { item: string; index: number }) => any
interface Slots {
  leading?: SlotProps
}
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

        let session = host.open_session().unwrap();
        let result = session
            .get_component_meta("/src/Button.vue")
            .unwrap()
            .expect("external backend should return component meta");

        let leading = result
            .slots
            .iter()
            .find(|slot| slot.name == "leading")
            .expect("leading slot should be present");
        let bindings: BTreeSet<_> = leading
            .bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect();
        assert_eq!(
            bindings,
            BTreeSet::from(["index", "item"]),
            "slot bindings should be resolved through the alias-based external query, got {:?}",
            leading
                .bindings
                .iter()
                .map(|binding| binding.name.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            fake.slot_binding_requests.lock().len(),
            1,
            "slot binding expansion should run for alias-based slot declarations"
        );
    }

    #[test]
    fn non_verter_backend_passes_trace_cursor_to_external_expander() {
        crate::host_manage::with_component_meta_trace_enabled_for_test(true, || {
            let mut config = crate::types::HostConfig::default();
            config.type_expansion_backend = TypeExpansionBackend::Tsgo;
            let host = ComponentMetaHost::new_standalone(config);
            let fake = Arc::new(FakeTypeExpander::object_with_members(vec![(
                "msg",
                TypeExpr::primitive(PrimitiveName::String),
                false,
            )]));
            host.set_type_expander(fake.clone());
            host.upsert_base("/src/types.ts", "export interface Props { msg: string }")
                .unwrap();
            host.upsert_base(
                "/src/Button.vue",
                r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
            )
            .unwrap();
            host.host().set_import_dependencies(
                "/src/Button.vue",
                vec![crate::types::DependencyResolution {
                    specifier: "./types".to_string(),
                    resolved_canonical_id: Some("/src/types.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                }],
            );

            let session = host.open_session().unwrap();
            let result = session.get_component_meta("/src/Button.vue").unwrap();
            assert!(result.is_some());

            let cursor = fake
                .trace_cursors
                .lock()
                .first()
                .and_then(|cursor| *cursor)
                .expect("external expander should receive a trace cursor");
            assert!(cursor.request_id > 0);
            assert!(cursor.span_id > 0);
            assert!(cursor.depth > 0);
        });
    }

    #[test]
    fn auto_backend_keeps_simple_requests_on_verter_path() {
        let mut config = crate::types::HostConfig::default();
        config.type_expansion_backend = TypeExpansionBackend::Auto;
        let host = ComponentMetaHost::new_standalone(config);
        let fake = Arc::new(FakeTypeExpander::object_with_members(vec![(
            "from_external",
            TypeExpr::primitive(PrimitiveName::String),
            false,
        )]));
        host.set_type_expander(fake.clone());
        host.upsert_base(
            "/src/Button.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n</script>\n<template><div>{{ msg }}</div></template>",
        )
        .unwrap();

        let session = host.open_session().unwrap();
        let result = session
            .get_component_meta("/src/Button.vue")
            .unwrap()
            .expect("auto backend should still return component meta");

        let props: BTreeSet<_> = result.props.iter().map(|prop| prop.name.as_str()).collect();
        assert_eq!(props, BTreeSet::from(["msg"]));
        assert!(
            fake.requests.lock().is_empty(),
            "simple auto request should not touch the external expander"
        );
    }

    #[test]
    fn component_meta_budget_errors_surface_on_new_session_api() {
        let host = make_host();

        let import_count = 2_005usize;
        let mut defs_source = String::new();
        for index in 0..import_count {
            defs_source.push_str(&format!(
                "export interface T{index} {{ p{index}: string }}\n"
            ));
        }

        let mut types_source = String::new();
        types_source.push_str("import type { ");
        for index in 0..import_count {
            if index > 0 {
                types_source.push_str(", ");
            }
            types_source.push_str(&format!("T{index}"));
        }
        types_source.push_str(" } from './defs'\n");
        types_source.push_str("export interface Props extends ");
        for index in 0..import_count {
            if index > 0 {
                types_source.push_str(", ");
            }
            types_source.push_str(&format!("T{index}"));
        }
        types_source.push_str(" {}\n");

        host.upsert_base("/src/defs.ts", &defs_source).unwrap();
        host.upsert_base("/src/types.ts", &types_source).unwrap();
        host.upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
        host.host().set_import_dependencies(
            "/src/App.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.host().set_import_dependencies(
            "/src/types.ts",
            vec![crate::types::DependencyResolution {
                specifier: "./defs".to_string(),
                resolved_canonical_id: Some("/src/defs.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let session = host.open_session().unwrap();
        let err = session
            .get_component_meta("/src/App.vue")
            .expect_err("runaway external type resolution should fail explicitly");

        match err {
            ComponentMetaHostError::Host(message) => {
                assert!(
                    message.contains("external type resolution step budget exceeded"),
                    "error should explain the traversal cap, got: {message}"
                );
                assert!(
                    message.contains("2000"),
                    "error should include the configured step cap, got: {message}"
                );
            }
            other => panic!("expected host budget error, got {other:?}"),
        }
    }

    #[test]
    fn external_backend_bypasses_native_budget_error_path() {
        let mut config = crate::types::HostConfig::default();
        config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let host = ComponentMetaHost::new_standalone(config);
        host.set_type_expander(Arc::new(FakeTypeExpander::object_with_members(vec![(
            "label",
            TypeExpr::primitive(PrimitiveName::String),
            false,
        )])));

        let import_count = 2_005usize;
        let mut defs_source = String::new();
        for index in 0..import_count {
            defs_source.push_str(&format!(
                "export interface T{index} {{ p{index}: string }}\n"
            ));
        }

        let mut types_source = String::new();
        types_source.push_str("import type { ");
        for index in 0..import_count {
            if index > 0 {
                types_source.push_str(", ");
            }
            types_source.push_str(&format!("T{index}"));
        }
        types_source.push_str(" } from './defs'\n");
        types_source.push_str("export interface Props extends ");
        for index in 0..import_count {
            if index > 0 {
                types_source.push_str(", ");
            }
            types_source.push_str(&format!("T{index}"));
        }
        types_source.push_str(" {}\n");

        host.upsert_base("/src/defs.ts", &defs_source).unwrap();
        host.upsert_base("/src/types.ts", &types_source).unwrap();
        host.upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
        host.host().set_import_dependencies(
            "/src/App.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.host().set_import_dependencies(
            "/src/types.ts",
            vec![crate::types::DependencyResolution {
                specifier: "./defs".to_string(),
                resolved_canonical_id: Some("/src/defs.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let session = host.open_session().unwrap();
        let result = session
            .get_component_meta("/src/App.vue")
            .unwrap()
            .expect("external backend should avoid the native symbolic budget failure");

        assert_eq!(result.props.len(), 1);
        assert_eq!(result.props[0].name, "label");
    }

    #[test]
    fn auto_backend_escalates_to_external_expander_when_native_budget_is_exceeded() {
        let mut config = crate::types::HostConfig::default();
        config.type_expansion_backend = TypeExpansionBackend::Auto;
        let host = ComponentMetaHost::new_standalone(config);
        let fake = Arc::new(FakeTypeExpander::object_with_members(vec![(
            "label",
            TypeExpr::primitive(PrimitiveName::String),
            false,
        )]));
        host.set_type_expander(fake.clone());

        let import_count = 2_005usize;
        let mut defs_source = String::new();
        for index in 0..import_count {
            defs_source.push_str(&format!(
                "export interface T{index} {{ p{index}: string }}\n"
            ));
        }

        let mut types_source = String::new();
        types_source.push_str("import type { ");
        for index in 0..import_count {
            if index > 0 {
                types_source.push_str(", ");
            }
            types_source.push_str(&format!("T{index}"));
        }
        types_source.push_str(" } from './defs'\n");
        types_source.push_str("export interface Props extends ");
        for index in 0..import_count {
            if index > 0 {
                types_source.push_str(", ");
            }
            types_source.push_str(&format!("T{index}"));
        }
        types_source.push_str(" {}\n");

        host.upsert_base("/src/defs.ts", &defs_source).unwrap();
        host.upsert_base("/src/types.ts", &types_source).unwrap();
        host.upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
        host.host().set_import_dependencies(
            "/src/App.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.host().set_import_dependencies(
            "/src/types.ts",
            vec![crate::types::DependencyResolution {
                specifier: "./defs".to_string(),
                resolved_canonical_id: Some("/src/defs.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let session = host.open_session().unwrap();
        let result = session
            .get_component_meta("/src/App.vue")
            .unwrap()
            .expect("auto backend should escalate to external expansion");

        assert_eq!(result.props.len(), 1);
        assert_eq!(result.props[0].name, "label");
        assert_eq!(
            fake.requests.lock().len(),
            1,
            "threshold-exceeded auto request should use the external expander"
        );
    }

    #[test]
    fn snapshot_view_returns_source_and_structure() {
        let host = make_host();
        host.upsert_base(
            "/src/Foo.vue",
            "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div/></template>",
        )
        .unwrap();

        let snapshot = host.snapshot_view("/src/Foo.vue").unwrap();
        assert!(snapshot.source.text.contains("const x = 1"));
        assert!(snapshot.sfc_structure.script_setup.is_some());
        assert!(snapshot.sfc_structure.template.is_some());
        assert!(snapshot.sfc_structure.script.is_none());
    }

    #[test]
    fn snapshot_view_dual_script() {
        let host = make_host();
        host.upsert_base(
            "/src/Bar.vue",
            "<script lang=\"ts\">\nexport default {}\n</script>\n<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n</script>\n<template><div/></template>",
        )
        .unwrap();

        let snapshot = host.snapshot_view("/src/Bar.vue").unwrap();
        assert!(
            snapshot.sfc_structure.script.is_some(),
            "should have companion script"
        );
        assert!(
            snapshot.sfc_structure.script_setup.is_some(),
            "should have script setup"
        );
        assert!(
            snapshot.sfc_structure.template.is_some(),
            "should have template"
        );

        let setup = snapshot.sfc_structure.script_setup.unwrap();
        let setup_text =
            &snapshot.source.text[setup.content.start as usize..setup.content.end as usize];
        assert!(
            setup_text.contains("defineProps"),
            "setup span should contain defineProps, got: {setup_text}"
        );
    }

    #[test]
    fn snapshot_view_missing_file_returns_error() {
        let host = make_host();
        let result = host.snapshot_view("/nonexistent.vue");
        assert!(result.is_err());
    }

    #[test]
    fn snapshot_view_revision_matches_generation() {
        let host = make_host();
        host.upsert_base("/src/A.vue", "<template/>").unwrap();
        let snap = host.snapshot_view("/src/A.vue").unwrap();
        assert_eq!(snap.revision, host.generation());
    }

    #[test]
    fn extract_sfc_structure_handles_empty() {
        let structure = super::extract_sfc_structure("");
        assert!(structure.script.is_none());
        assert!(structure.script_setup.is_none());
        assert!(structure.template.is_none());
    }

    #[test]
    fn macro_type_argument_span_handles_nested_with_defaults() {
        let source = r#"<script setup lang="ts">
const props = withDefaults(defineProps<Record<string, Array<Foo<Bar>>>>(), {
  value: () => [],
})
</script>"#;
        let start = source.find("withDefaults").unwrap() as u32;
        let end = source.find("})").unwrap() as u32 + 2;
        let span = macro_type_argument_span(
            source,
            &verter_semantic::analysis::types::AnalyzedMacro {
                kind: verter_semantic::analysis::types::AnalyzedMacroKind::WithDefaults,
                is_type_based: true,
                type_references: vec!["Record".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: vec!["value".to_string()],
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                span: verter_span::Span::new(start, end),
            },
        )
        .expect("nested defineProps type span should be found");

        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "Record<string, Array<Foo<Bar>>>"
        );
    }

    #[test]
    fn extracted_external_meta_keeps_fallthrough_on_captured_store_view() {
        let host = make_host();
        host.upsert_base("/src/Link.vue", "<template><a /></template>")
            .unwrap();
        host.upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import Link from './Link.vue'
</script>
<template><Link /></template>"#,
        )
        .unwrap();

        let store_view = host.host().resolver_store_view();
        let resolved = host
            .host()
            .resolve_component_meta_in_view(
                "/src/Button.vue",
                crate::types::ResolverMode::Expanded,
                &store_view,
            )
            .expect("button resolved state should exist for the captured store view");

        host.upsert_base("/src/Link.vue", "<script setup lang=\"ts\"></script>")
            .unwrap();

        let meta = extract_component_meta_from_resolved_with_evaluated(
            host.host(),
            "/src/Button.vue",
            &resolved,
            resolved.evaluated_types.as_ref(),
            true,
            Some(&store_view),
        );

        assert!(
            matches!(
                meta.fallthrough_surface,
                verter_semantic::analysis::component_meta::FallthroughSurface::Branches { .. }
            ),
            "captured store views should keep child fallthrough resolution pinned to the resolved snapshot",
        );
    }
}
