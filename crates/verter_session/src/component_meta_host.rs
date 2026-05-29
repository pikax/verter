//! Host-backed component-meta session layer.
//!
//! This replaces the public `MetaProject` / `MetaSession` naming, but keeps the
//! underlying behavioral contract that component-meta depends on:
//! - project/base state is shared
//! - sessions hold isolated overlays
//! - closing a session releases its overlays
//! - native component-meta queries stay on the shared host/session path

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::resolver_core::type_expansion::TypeExpansionError;
use crate::resolver_core::type_expansion_host::{
    ScriptLang, SfcBlockSpan, SfcStructure, SourceSnapshot, TypeExpansionHost,
    TypeExpansionSnapshot,
};
#[cfg(test)]
use crate::resolver_core::{
    component_meta_resolved_macros as resolver_component_meta_resolved_macros,
    component_meta_type_registry as resolver_component_meta_type_registry,
};
#[cfg(test)]
use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
#[cfg(test)]
use verter_semantic::analysis::component_meta::{
    AcceptedSurfaceCompleteness, FallthroughSurface, RootReachability,
};
#[cfg(test)]
use verter_semantic::analysis::type_expand::ExpandedComponentTypes;
#[cfg(test)]
use verter_type_expr::{ObjectMember, TypeExpr};

use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

// Test hosts construct via `new_standalone_with_scheduler_config` with
// `SchedulerConfig { cpu_threads: 1, .. }`. The previous global
// `HEAVY_COMPONENT_META_TEST_MUTEX` (and its `acquires` / `wait_ns`
// counters) was a workaround for OS-level CPU oversubscription when each
// per-test `Scheduler` spawned `cpu_threads = num_cpus()` worker pools;
// pinning the scheduler to one CPU thread eliminates the contention
// entirely, so no shared mutex is required.

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
    /// Audit bundle requested but the host config does not enable
    /// `audit_enabled` + `footprint_capture`.
    #[error(
        "audit is not enabled on this host — set HostConfig::audit_enabled and \
         HostConfig::footprint_capture before calling get_component_meta_with_audit"
    )]
    AuditNotEnabled,
    /// The resolution returned a request_id, but no audit record was
    /// available at retrieval time. This can happen when the audit
    /// record was displaced from the bounded store by subsequent
    /// requests (store holds up to
    /// [`crate::component_meta_audit::AUDIT_RECORDS_STORE_CAPACITY`]
    /// entries).
    #[error("audit record for request_id={request_id} missing — store may have evicted it")]
    AuditRecordMissing { request_id: u64 },
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

struct ComponentMetaHostInner {
    project: Arc<crate::meta::MetaProject>,
    generation: AtomicU64,
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
        let project = crate::meta::MetaProject::new(VerterHost::new_standalone(config));
        Self {
            inner: Arc::new(ComponentMetaHostInner {
                project,
                generation: AtomicU64::new(0),
            }),
        }
    }

    /// Create a new component-meta host with a standalone memory workspace and
    /// an explicit [`SchedulerConfig`].
    ///
    /// Test harnesses construct hosts with
    /// `SchedulerConfig { cpu_threads: 1, ..SchedulerConfig::default() }`
    /// to avoid CPU oversubscription when many parallel test threads
    /// each spin up their own scheduler thread pools.
    pub fn new_standalone_with_scheduler_config(
        config: crate::types::HostConfig,
        scheduler_config: verter_scheduler::scheduler::SchedulerConfig,
    ) -> Self {
        let host = VerterHost::new_standalone_with_scheduler_config(config, scheduler_config);
        let project = crate::meta::MetaProject::new(host);
        Self {
            inner: Arc::new(ComponentMetaHostInner {
                project,
                generation: AtomicU64::new(0),
            }),
        }
    }

    /// Create a new component-meta host backed by an existing workspace.
    pub fn new(
        config: crate::types::HostConfig,
        workspace: Arc<dyn verter_workspace::WorkspaceAccess>,
    ) -> Self {
        let host = VerterHost::new(config, workspace);
        let project = crate::meta::MetaProject::new(host);
        Self {
            inner: Arc::new(ComponentMetaHostInner {
                project,
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
        })
    }

    /// Open a new isolated session in [`ExecutionMode::Batch`] mode.
    ///
    /// Test harness and MCP server callers use this path. LSP callers stay on
    /// [`Self::open_session`] (Interactive mode).
    pub fn open_session_batch(&self) -> Result<ComponentMetaSession, ComponentMetaHostError> {
        self.check_alive()?;
        Ok(ComponentMetaSession {
            inner: self
                .inner
                .project
                .open_session_batch()
                .map_err(ComponentMetaHostError::from)?,
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
}

impl ComponentMetaSession {
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
        component_meta_trace_custom!("component_meta_session_query", canonical_or_alias);
        self.inner
            .get_component_meta(canonical_or_alias)
            .map_err(ComponentMetaHostError::from)
    }

    /// Batch surface for [`Self::get_component_meta`]: compute metadata
    /// for `canonical_or_aliases` under one shared overlay view and a
    /// single scheduler dispatch.
    ///
    /// Delegates to [`crate::meta::MetaSession::get_component_meta_batch`]
    /// so all N queries share the host-owned admission caches
    /// (`MaterializeStructureDb`, `ComponentMetaResultDb`,
    /// `SemanticGraphStore`). Per-id failures surface in each result
    /// slot; the batch does not abort.
    pub fn get_component_meta_batch(
        &self,
        canonical_or_aliases: &[String],
    ) -> Result<
        Vec<
            Result<
                Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>,
                ComponentMetaHostError,
            >,
        >,
        ComponentMetaHostError,
    > {
        component_meta_trace_custom!(
            "component_meta_session_batch",
            format!("batch_size={}", canonical_or_aliases.len()),
        );
        let raw = self
            .inner
            .get_component_meta_batch(canonical_or_aliases)
            .map_err(ComponentMetaHostError::from)?;
        Ok(raw
            .into_iter()
            .map(|slot| slot.map_err(ComponentMetaHostError::from))
            .collect())
    }

    /// Get component metadata plus the resolved-state sidecar AND the
    /// per-request audit record produced by the same resolution.
    /// Synchronous — the audit record is retrievable immediately
    /// after `get_component_meta_with_resolution` returns.
    ///
    /// Requires `HostConfig::audit_enabled` + `HostConfig::footprint_capture`
    /// to be true on the underlying host; otherwise returns
    /// [`ComponentMetaHostError::AuditNotEnabled`].
    pub fn get_component_meta_with_audit(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        Option<(
            verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
            crate::meta_resolve::ResolvedComponentMetaState,
            crate::component_meta_audit::RequestAuditRecord,
        )>,
        ComponentMetaHostError,
    > {
        let host = self.inner.host();
        if !host.config.audit_enabled || !host.config.footprint_capture {
            return Err(ComponentMetaHostError::AuditNotEnabled);
        }
        // Audit capture is instrumented on the host-level path — it
        // installs the `RequestContext` TLS, stamps the resolution's
        // `request_id`, and publishes the record into the bounded
        // store. The session-runtime path (used by `get_component_meta`
        // etc.) skips that setup because it resolves through an
        // overlay-aware runtime that does not thread audit state.
        // `ComponentMetaSession::get_component_meta_with_audit` is
        // therefore equivalent to the base-project audit query — the
        // same answer the `AuditedRequest` harness returns when given
        // the same host.
        let Some((analysis, resolution)) =
            host.get_component_meta_with_resolution(canonical_or_alias)
        else {
            return Ok(None);
        };
        let request_id = resolution.request_id;
        let record = host
            .take_audit_record(request_id)
            .ok_or(ComponentMetaHostError::AuditRecordMissing { request_id })?;
        Ok(Some((analysis, resolution, record)))
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
        component_meta_trace_custom!(
            "component_meta_session_query_with_resolution",
            canonical_or_alias
        );
        self.inner
            .get_component_meta_with_resolution(canonical_or_alias)
            .map_err(ComponentMetaHostError::from)
    }

    /// Full-meta as encoded payload. Cache-first on the Verter backend.
    pub fn get_component_meta_payload(
        &self,
        canonical_or_alias: &str,
        encode_fn: impl FnOnce(
            verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
            &crate::meta_resolve::ResolvedComponentMetaState,
        ) -> Vec<u8>,
    ) -> Result<Option<Vec<u8>>, ComponentMetaHostError> {
        self.inner
            .get_component_meta_payload(canonical_or_alias, encode_fn)
            .map_err(ComponentMetaHostError::from)
    }

    /// Batch surface for [`Self::get_component_meta_payload`]: compute
    /// encoded payloads for `canonicals_or_aliases` under one shared
    /// overlay view and a single scheduler dispatch. Per-id misses /
    /// failures surface as `None` in their slot.
    pub fn get_component_meta_batch_payloads<F>(
        &self,
        canonical_or_aliases: &[String],
        encode_fn: F,
    ) -> Result<Vec<Option<Vec<u8>>>, ComponentMetaHostError>
    where
        F: Fn(
                verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
                &crate::meta_resolve::ResolvedComponentMetaState,
            ) -> Vec<u8>
            + Sync
            + Send,
    {
        self.inner
            .get_component_meta_batch_payloads(canonical_or_aliases, encode_fn)
            .map_err(ComponentMetaHostError::from)
    }

    /// Tier 1B selective surface API (D32 + D102). Returns the
    /// `ComponentMetaSurface` envelope (eager scalars + `NamedTypeHandle`
    /// for every type-bearing field), or `None` when the canonical does
    /// not resolve to a component.
    pub fn get_component_meta_surface(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<crate::component_meta_payload::ComponentMetaSurface>, ComponentMetaHostError>
    {
        self.inner
            .get_component_meta_surface(canonical_or_alias)
            .map_err(ComponentMetaHostError::from)
    }

    /// Tier 1B selective surface API (D32 + D102). Resolves a `TypeHandle`
    /// to a one-layer `TypeExpansion`. Errors are typed (D104 + D114).
    pub fn get_component_meta_type_expansion(
        &self,
        handle: crate::component_meta_payload::TypeHandle,
        depth: Option<usize>,
    ) -> Result<
        crate::component_meta_payload::TypeExpansion,
        crate::component_meta_payload::TypeHandleError,
    > {
        self.inner.get_component_meta_type_expansion(handle, depth)
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

#[cfg(test)]
fn extract_component_meta_from_resolved_with_evaluated(
    host: &VerterHost,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    canonical_id: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    evaluated_types: Option<&ExpandedComponentTypes>,
    include_fallthrough: bool,
) -> ComponentMetaAnalysis {
    let resolved_macros = resolver_component_meta_resolved_macros(
        host,
        canonical_id,
        resolved.snapshot.macros.as_ref(),
        &resolved.resolved_macros,
    );
    let resolved_type_registry =
        resolver_component_meta_type_registry(&resolved.resolved_type_registry);
    let canonical_source = host.read_analysis_source(canonical_id);
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
        canonical_source: canonical_source.as_deref(),
    };

    let mut meta = verter_semantic::analysis::component_meta::extract_component_meta(input);
    if include_fallthrough {
        let mut visiting = rustc_hash::FxHashSet::default();
        if let Some(resolution) = host.compute_fallthrough_surface_from_resolved_state(
            canonical_id,
            resolved,
            None,
            &mut visiting,
            ctx,
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

    fn make_host() -> ComponentMetaHost {
        // Tests use `cpu_threads = 1` to avoid CPU oversubscription
        // when many parallel test threads each spin up their own
        // Rayon pools.
        ComponentMetaHost::new_standalone_with_scheduler_config(
            crate::types::HostConfig::default(),
            verter_scheduler::scheduler::SchedulerConfig {
                cpu_threads: 1,
                ..verter_scheduler::scheduler::SchedulerConfig::default()
            },
        )
    }

    #[test]
    fn upsert_base_and_get_source() {
        let host = make_host();
        host.upsert_base("/src/Foo.vue", "<template><div/></template>")
            .unwrap();
        let session = host.open_session_batch().unwrap();
        let source = session.get_effective_source("/src/Foo.vue").unwrap();
        assert!(source.is_some());
        assert!(source.unwrap().contains("<template>"));
    }

    #[test]
    fn session_overlays_are_isolated() {
        let host = make_host();
        host.upsert_base("/src/Foo.vue", "<template><div/></template>")
            .unwrap();
        let session_a = host.open_session_batch().unwrap();
        let session_b = host.open_session_batch().unwrap();

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

        let session_a = host.open_session_batch().unwrap();
        session_a
            .upsert("/src/Foo.vue", "<template><span/></template>".to_string())
            .unwrap();
        session_a.close();

        let session_b = host.open_session_batch().unwrap();
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
    fn get_component_meta_returns_none_for_missing() {
        let host = make_host();
        let session = host.open_session_batch().unwrap();
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
        let session = host.open_session_batch().unwrap();
        let result = session.get_component_meta("/src/Button.vue").unwrap();
        assert!(result.is_some(), "should return meta for loaded SFC");
    }

    #[test]
    fn component_meta_with_resolution_keeps_resolved_type_registry_sidecar() {
        let host = make_host();
        host.upsert_base(
            "/src/types.ts",
            r#"type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>, A> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  appConfig?: A
}"#,
        )
        .unwrap();
        host.upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const"#,
        )
        .unwrap();
        host.upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'
import theme from './theme'

type Button = ComponentConfig<typeof theme, MissingAppConfig>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

        let session = host.open_session_batch().unwrap();
        let (_analysis, resolved) = session
            .get_component_meta_with_resolution("/src/Button.vue")
            .unwrap()
            .expect("canonical query should return meta plus resolution sidecar");

        let button_entry = resolved
            .resolved_type_registry
            .iter()
            .find(|entry| entry.name == "Button")
            .expect("canonical query should keep the resolved Button registry entry");
        let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
            panic!(
                "expected resolved Button helper to materialize as an object, got {:?}",
                button_entry.type_expr
            );
        };

        let variants_member = button_shape
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "variants" => {
                    Some(&property.ty)
                }
                _ => None,
            })
            .expect("Button registry entry should keep variants");
        let TypeExpr::Object(variants_shape) = variants_member else {
            panic!(
                "expected Button.variants to materialize as an object, got {:?}",
                variants_member
            );
        };
        assert!(
            variants_shape.properties.iter().any(
                |member| matches!(member, ObjectMember::Property(property) if property.name == "color"),
            ),
            "expected Button.variants to expose color, got {:?}",
            variants_member
        );
    }

    #[test]
    fn overlay_queries_reapply_owner_after_overlay_only_helper_upserts() {
        let host = make_host();
        let session = host.open_session_batch().unwrap();

        // Upsert the owner before its overlay-only helpers. Overlay application
        // must still leave the owner query seeing the helper files.
        session
            .upsert(
                "/src/Button.vue",
                r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#
                    .to_string(),
            )
            .unwrap();
        session
            .upsert(
                "/src/tv.ts",
                r#"type ClassValue = string | number | boolean | null | undefined | ClassValue[] | { [key: string]: any }

type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: ClassValue
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

type ComponentAppConfig<
  T,
  A extends Record<string, any>,
  K extends string,
  U extends string = 'ui' | 'ui.prose'
> = A & (
  U extends 'ui.prose'
    ? { ui?: { prose?: { [k in K]?: Partial<T> } } }
    : { [key in Exclude<U, 'ui.prose'>]?: { [k in K]?: Partial<T> } }
)

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  AppConfig: ComponentAppConfig<T, A, K, U>,
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
  slots: ComponentSlots<T>,
  ui: ComponentUI<T>
}"#
                    .to_string(),
            )
            .unwrap();
        session
            .upsert(
                "/src/schema.ts",
                r#"export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}"#
                .to_string(),
            )
            .unwrap();
        session
            .upsert(
                "/src/theme.ts",
                r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' },
    size: { sm: '', md: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const"#
                    .to_string(),
            )
            .unwrap();

        let (_analysis, resolved) = session
            .get_component_meta_with_resolution("/src/Button.vue")
            .unwrap()
            .expect("overlay-only helper query should return canonical meta plus resolution");

        let button_entry = resolved
            .resolved_type_registry
            .iter()
            .find(|entry| entry.name == "Button")
            .expect("Button helper should be published in the resolved registry");
        let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
            panic!(
                "expected Button helper to materialize as an object, got {:?}",
                button_entry.type_expr
            );
        };
        let variants_member = button_shape
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "variants" => {
                    Some(&property.ty)
                }
                _ => None,
            })
            .expect("Button helper should keep variants");
        let TypeExpr::Object(variants_shape) = variants_member else {
            panic!(
                "expected Button.variants to materialize as an object, got {:?}",
                variants_member
            );
        };
        let color_member = variants_shape
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "color" => Some(&property.ty),
                _ => None,
            })
            .expect("Button.variants should keep color");
        let TypeExpr::Union(color_variants) = color_member else {
            panic!(
                "expected Button.variants.color to stay a union surface, got {:?}",
                color_member
            );
        };
        assert!(
            color_variants.contains(&TypeExpr::string_literal("primary")),
            "expected Button.variants.color to include primary, got {:?}",
            color_member
        );
        assert!(
            color_variants.contains(&TypeExpr::string_literal("secondary")),
            "expected Button.variants.color to include secondary, got {:?}",
            color_member
        );
        assert!(
            color_variants.contains(&TypeExpr::string_literal("neutral")),
            "expected Button.variants.color to include neutral, got {:?}",
            color_member
        );
    }

    /// The 2005-interface fixture is a large finite import/heritage fan-out,
    /// not a recursive semantic case. The solver resolves it successfully
    /// within operational limits. This test verifies that large finite
    /// type graphs complete without hang or budget error.
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

        let session = host.open_session_batch().unwrap();
        let meta = session
            .get_component_meta("/src/App.vue")
            .expect("large finite heritage graph should resolve successfully")
            .expect("component meta should be present");

        // All 2005 interfaces should contribute one prop each
        assert_eq!(
            meta.props.len(),
            import_count,
            "large finite heritage graph should produce all {import_count} props"
        );
        // Spot-check the first prop and confirm the highest-numbered prop is
        // still present; projected surfaces are sorted lexicographically.
        assert_eq!(meta.props[0].name, "p0");
        assert!(
            meta.props
                .iter()
                .any(|prop| prop.name == format!("p{}", import_count - 1)),
            "large finite heritage graph should retain p{} somewhere in the deterministic lexical surface order",
            import_count - 1
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

        let _store_view = host.host().resolver_store_view();
        let resolved = host
            .host()
            .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
            .expect("button resolved state should exist for the captured store view");

        host.upsert_base("/src/Link.vue", "<script setup lang=\"ts\"></script>")
            .unwrap();

        let meta = crate::resolver_core::with_bare_host_ctx_for_test(host.host(), |ctx| {
            extract_component_meta_from_resolved_with_evaluated(
                host.host(),
                ctx,
                "/src/Button.vue",
                &resolved,
                resolved.evaluated_types.as_ref(),
                true,
            )
        });

        assert!(
            matches!(
                meta.fallthrough_surface,
                verter_semantic::analysis::component_meta::FallthroughSurface::Branches { .. }
            ),
            "captured store views should keep child fallthrough resolution pinned to the resolved snapshot",
        );
    }
}
