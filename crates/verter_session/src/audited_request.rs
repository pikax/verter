//! `AuditedRequest` harness — plan §2.6.
//!
//! The harness wraps one `get_component_meta_with_resolution` call in
//! a request-scoped audit: it enables `footprint_capture`, installs a
//! `NESTED_AUDIT_GUARD`, resets the `REQUESTS_CREATED_IN_CURRENT_AUDITED_RUN`
//! counter, runs the resolution (or a caller-supplied closure),
//! validates that exactly one request was created, and returns the
//! triple `(ComponentMetaAnalysis, ResolvedComponentMetaState,
//! RustAuditRecord)`.
//!
//! The harness is the sole entry point for tests that need a mined
//! footprint. Direct callers of `get_component_meta_with_resolution`
//! get a `RustAuditRecord` via `take_audit_record(request_id)` but
//! must manage context/accumulator themselves.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
use verter_workspace::WorkspaceAccess;

use crate::component_meta_audit::{
    AuditRecordsStore, RequestFootprintAccumulator, RustAuditRecord,
};
use crate::meta_resolve::ResolvedComponentMetaState;
use crate::request_context::{
    current_request_context, nested_audit_in_progress, requests_created_snapshot,
    reset_requests_created, NestedAuditGuard,
};
use crate::types::AnalysisLevel;
use crate::{HostConfig, VerterHost};

/// Errors surfaced by [`AuditedRequestBuilder::resolve`] and
/// [`AuditedRequestBuilder::run_custom`].
#[derive(Debug)]
pub enum AuditedRequestError {
    /// Same-thread re-entry — an audited run was already active on this
    /// thread. Cross-thread concurrent audits on the same host are
    /// allowed.
    NestedAuditNotSupported,
    /// More than one `get_component_meta_with_resolution` call ran
    /// inside a `run_custom` closure. Each audited run must produce
    /// exactly one request.
    MultipleRequestsInSingleRun,
    /// The request completed but no `RustAuditRecord` was published
    /// (capture disabled, misconfigured host, or a store eviction).
    AuditRecordMissing,
    /// `HostConfig::validate` rejected the configuration the builder
    /// would produce (e.g. footprint_capture without audit_enabled).
    PrerequisitesNotMet(crate::types::HostConfigError),
    /// The underlying `get_component_meta_with_resolution` returned
    /// `None` (canonical not found, analysis failed, etc.).
    ResolutionFailed,
}

impl std::fmt::Display for AuditedRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NestedAuditNotSupported => {
                write!(f, "nested audit on the same thread is not supported")
            }
            Self::MultipleRequestsInSingleRun => write!(
                f,
                "`run_custom` closure produced more than one request; each audited run must \
                 produce exactly one `get_component_meta_with_resolution` call"
            ),
            Self::AuditRecordMissing => write!(
                f,
                "no RustAuditRecord was published for the request — check HostConfig::audit_enabled"
            ),
            Self::PrerequisitesNotMet(err) => write!(f, "prerequisites not met: {err}"),
            Self::ResolutionFailed => write!(f, "get_component_meta_with_resolution returned None"),
        }
    }
}

impl std::error::Error for AuditedRequestError {}

/// Builder entry point. Use `AuditedRequest::builder()` to obtain one.
pub struct AuditedRequest;

impl AuditedRequest {
    pub fn builder() -> AuditedRequestBuilder {
        AuditedRequestBuilder::default()
    }
}

#[derive(Default)]
pub struct AuditedRequestBuilder {
    workspace: Option<Arc<dyn WorkspaceAccess>>,
    files: Vec<(String, String)>,
    analysis_level: Option<AnalysisLevel>,
    host_config: Option<HostConfig>,
    attach_to: Option<Arc<VerterHost>>,
    // `true` means "build a fresh host internally" (default);
    // `false` means "must have been called with attach_to".
    hermetic: bool,
}

impl AuditedRequestBuilder {
    pub fn workspace(mut self, ws: Arc<dyn WorkspaceAccess>) -> Self {
        self.workspace = Some(ws);
        self
    }

    pub fn files<I, S1, S2>(mut self, files: I) -> Self
    where
        I: IntoIterator<Item = (S1, S2)>,
        S1: Into<String>,
        S2: Into<String>,
    {
        self.files = files
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }

    pub fn analysis_level(mut self, level: AnalysisLevel) -> Self {
        self.analysis_level = Some(level);
        self
    }

    pub fn host_config(mut self, config: HostConfig) -> Self {
        self.host_config = Some(config);
        self
    }

    /// Default mode — create a fresh host, inject `files`, run the
    /// resolution, drain the audit record, and drop the host.
    pub fn hermetic(mut self) -> Self {
        self.hermetic = true;
        self.attach_to = None;
        self
    }

    /// Attach to an existing host. Concurrent audits on distinct
    /// threads on the same host are supported; same-thread nested
    /// audits are rejected via `NESTED_AUDIT_GUARD`.
    pub fn attach_to(mut self, host: Arc<VerterHost>) -> Self {
        self.attach_to = Some(host);
        self.hermetic = false;
        self
    }

    /// Build (or reuse) a host and resolve `canonical_id`. Returns the
    /// triple `(analysis, resolution, record)`.
    pub fn resolve(
        self,
        canonical_id: &str,
    ) -> Result<
        (
            ComponentMetaAnalysis,
            ResolvedComponentMetaState,
            RustAuditRecord,
        ),
        AuditedRequestError,
    > {
        let host = self.build_host()?;
        run_audited(&host, |host_ref| {
            host_ref
                .get_component_meta_with_resolution(canonical_id)
                .ok_or(AuditedRequestError::ResolutionFailed)
        })
    }

    /// Run an arbitrary closure against a fresh (or attached) host.
    /// The closure must perform exactly one
    /// `get_component_meta_with_resolution` call; any other count
    /// surfaces as [`AuditedRequestError::MultipleRequestsInSingleRun`].
    pub fn run_custom<F>(
        self,
        f: F,
    ) -> Result<
        (
            ComponentMetaAnalysis,
            ResolvedComponentMetaState,
            RustAuditRecord,
        ),
        AuditedRequestError,
    >
    where
        F: FnOnce(&VerterHost) -> Option<(ComponentMetaAnalysis, ResolvedComponentMetaState)>,
    {
        let host = self.build_host()?;
        run_audited(&host, |host_ref| {
            f(host_ref).ok_or(AuditedRequestError::ResolutionFailed)
        })
    }

    fn build_host(&self) -> Result<Arc<VerterHost>, AuditedRequestError> {
        if let Some(host) = self.attach_to.clone() {
            host.config
                .validate()
                .map_err(AuditedRequestError::PrerequisitesNotMet)?;
            return Ok(host);
        }
        // Hermetic path: construct a fresh host with audit_enabled +
        // footprint_capture pre-enabled.
        let mut config = self.host_config.clone().unwrap_or_default();
        config.audit_enabled = true;
        config.footprint_capture = true;
        if let Some(level) = self.analysis_level {
            config.analysis_level = level;
        }
        config
            .validate()
            .map_err(AuditedRequestError::PrerequisitesNotMet)?;

        let workspace: Arc<dyn WorkspaceAccess> = match self.workspace.clone() {
            Some(ws) => ws,
            None => Arc::new(verter_workspace::MemoryWorkspace::new(
                verter_workspace::MemoryOptions::default(),
            )),
        };
        let host = Arc::new(VerterHost::new(config, workspace));
        // Inject any `files` the builder collected.
        for (canonical, content) in &self.files {
            let _ = host.upsert(crate::UpsertRequest {
                canonical_id: Some(canonical.clone()),
                input_id: canonical.clone(),
                source: Arc::from(content.as_str()),
                file_kind: crate::types::FileKind::from_path(canonical),
                aliases: Vec::new(),
            });
        }
        Ok(host)
    }
}

fn run_audited<F>(
    host: &Arc<VerterHost>,
    f: F,
) -> Result<
    (
        ComponentMetaAnalysis,
        ResolvedComponentMetaState,
        RustAuditRecord,
    ),
    AuditedRequestError,
>
where
    F: FnOnce(
        &VerterHost,
    )
        -> Result<(ComponentMetaAnalysis, ResolvedComponentMetaState), AuditedRequestError>,
{
    // Same-thread nested-audit check.
    if nested_audit_in_progress() {
        return Err(AuditedRequestError::NestedAuditNotSupported);
    }
    let _guard = NestedAuditGuard::enter().ok_or(AuditedRequestError::NestedAuditNotSupported)?;

    reset_requests_created();
    let result = f(host);
    let created = requests_created_snapshot();
    reset_requests_created();

    let (analysis, resolution) = result?;
    if created > 1 {
        return Err(AuditedRequestError::MultipleRequestsInSingleRun);
    }

    let record = host
        .take_audit_record(resolution.request_id)
        .ok_or(AuditedRequestError::AuditRecordMissing)?;

    Ok((analysis, resolution, record))
}

/// Fire-and-forget reference to silence unused warnings for helper
/// types the public API re-exports but doesn't consume internally.
#[doc(hidden)]
pub fn _touch_refs() {
    let _ = (
        std::any::type_name::<AuditRecordsStore>(),
        std::any::type_name::<RequestFootprintAccumulator>(),
        current_request_context().is_some(),
    );
}
