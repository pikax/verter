//! The OWNED tsgo [`EngineBackend`] — the project-association authority for the
//! one-instance dual-surface tsgo provider.
//!
//! This backend realises the project-binding half of the external-TS contract for
//! the OWNED tsgo engine: `ensure_project` mints the [`BoundProject`] witness from
//! a resolved [`EnsureProject`] (itself mintable only from a resolved
//! `ProjectBinding`), preserving the `provider_op_requires_resolved_project`
//! type-state across crates. The negotiated [`EngineCapabilities`] record the
//! dual-surface handshake (§2.8): the `--api` checker + diagnostics + project
//! membership are present; wire-level cancellation is NOT (the shipped `--api`
//! exposes none — that is the EXPECTED value, not a failure).
//!
//! ## `query` / `diagnostics` ride the live provider, not this backend
//!
//! Unlike tsserver (whose publish authority writes an on-disk store a separate
//! plugin reads), the OWNED tsgo live transport IS the dual-surface
//! `TsgoOwnedProvider` (`verter_type_runtime`): `--api` diagnostics + checker over
//! the attached pipe, `--lsp` features over the shared process. That live
//! orchestration is a distinct concern from this project-association witness, and
//! the shared `EngineBackend::{query,diagnostics}` opaque-outcome surface is not
//! the channel the provider answers on. Per the Stub Prevention rule, an
//! always-empty / always-`NoResult` masquerade is forbidden, so `query` /
//! `diagnostics` `unimplemented!()` (fail LOUDLY) rather than silently degrade.

use std::sync::Arc;

use verter_session::external_ts::{
    BoundProject, Diagnostics, DiagnosticsOutcome, EngineBackend, EngineCapabilities, EngineError,
    EnsureProject, PublishSnapshot, Query, QueryOutcome,
};

/// The OWNED tsgo engine backend: the project-association witness authority for the
/// one-instance dual-surface provider.
///
/// It is otherwise stateless — the live `--api` checker + `--lsp` features are held
/// by the `TsgoOwnedProvider`, NOT here. This backend's sole production job is to
/// gate production ops behind a resolved-project [`BoundProject`] witness.
#[derive(Debug)]
pub struct TsgoEngineBackend {
    /// The negotiated capabilities reported for every bound project.
    capabilities: EngineCapabilities,
}

impl TsgoEngineBackend {
    /// Build the backend for a negotiated tsgo engine version.
    ///
    /// The dual-surface handshake (§2.8): `--api` checker + diagnostics + project
    /// membership are REQUIRED and present; the engine exposes NO static
    /// module-resolution-map endpoint and NO wire-level cancellation (the shipped
    /// `--api` has neither — `api_wire_cancel = false` is the EXPECTED recorded
    /// value, not a failure), so both capability flags are `false`.
    #[must_use]
    pub fn new(engine_version: impl Into<Arc<str>>) -> Self {
        Self {
            capabilities: EngineCapabilities {
                static_module_resolution_map: false,
                async_cancellable_queries: false,
                reported_version: Some(engine_version.into()),
            },
        }
    }
}

impl EngineBackend for TsgoEngineBackend {
    /// Mint the [`BoundProject`] witness from the [`EnsureProject`] request (itself
    /// mintable only from a resolved `ProjectBinding`). The OWNED tsgo provider
    /// holds no per-project on-disk store — the configured project is opened on the
    /// `--api` checker via `updateSnapshot({openProject})` and carriers ride `--lsp`
    /// didOpen overlays — so this materialises only the witness.
    fn ensure_project(&self, request: EnsureProject) -> Result<BoundProject, EngineError> {
        Ok(BoundProject::from_ensured(
            &request,
            self.capabilities.clone(),
        ))
    }

    /// The OWNED tsgo publish has no on-disk store: membership is conferred by the
    /// `--lsp` didOpen overlay + the configured-project `--api` `updateSnapshot`,
    /// driven by the live `TsgoOwnedProvider`, NOT this witness authority. A publish
    /// call here is a wiring error — fail LOUDLY rather than silently no-op.
    fn publish_snapshot(
        &self,
        _project: &BoundProject,
        _snapshot: PublishSnapshot,
    ) -> Result<(), EngineError> {
        unimplemented!(
            "TsgoEngineBackend confers OWNED membership through the live TsgoOwnedProvider \
             (--lsp didOpen overlays on the shared session + the configured-project --api \
             updateSnapshot), not through an on-disk publish store; there is nothing to \
             publish here."
        )
    }

    /// Answered by the live `TsgoOwnedProvider` (`--api` checker over the attached
    /// pipe), wired separately from this project-association witness.
    fn query(&self, _project: &BoundProject, _query: Query) -> Result<QueryOutcome, EngineError> {
        unimplemented!(
            "TsgoEngineBackend::query is answered by the live TsgoOwnedProvider's --api \
             checker over the attached pipe, wired separately from this witness authority."
        )
    }

    /// Answered by the live `TsgoOwnedProvider` (`--api` `getSemanticDiagnostics`),
    /// wired separately. See [`Self::query`].
    fn diagnostics(
        &self,
        _project: &BoundProject,
        _request: Diagnostics,
    ) -> Result<DiagnosticsOutcome, EngineError> {
        unimplemented!(
            "TsgoEngineBackend::diagnostics is answered by the live TsgoOwnedProvider's \
             --api getSemanticDiagnostics, wired separately from this witness authority."
        )
    }

    fn capabilities(&self) -> EngineCapabilities {
        self.capabilities.clone()
    }
}

#[cfg(test)]
#[path = "tsgo_backend_tests.rs"]
mod tests;
