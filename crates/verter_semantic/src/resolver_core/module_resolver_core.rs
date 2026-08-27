//! `ModuleResolverCore` is the public semantic-owned module-resolution
//! kernel.
//!
//! Everything the algorithm needs (`probe_path_resolution`,
//! `package_target_resolution`, `node_modules_resolution`,
//! `tsconfig_paths_resolution`, `project_references_resolution`,
//! `project_ownership_resolution`, `provider_projection_resolution`,
//! `top_level_resolution`, `preferred_specifier_resolution`) lives as
//! free functions across separate modules; this type is the actual
//! public core wrapping them with the real four public surfaces
//! (`resolve_attempt`/`resolve_for_project_attempt`/
//! `preferred_specifier_candidates`/`project_exact_result`, plus owner
//! selection).
//!
//! Matches the settled storage shape: holds ONLY
//! immutable graph/config state (`configs: Arc<[IdeProjectConfig]>`,
//! pre-sorted by precedence via [`sorted_by_precedence`]) — no
//! filesystem reader, manifest cache, transaction, or workspace
//! handle. A proposed `by_tsconfig`/`reference_edges` compiled-index
//! pair over the same data is a performance optimization, not a
//! correctness requirement. `project_references_resolution`'s linear
//! `.find()` over `configs` already satisfies every "must preserve
//! exactly" invariant (reference order, duplicates, unresolved
//! references, first-matching-config, the depth-256 + active-path
//! cycle guard) without a compiled index.

use std::sync::Arc;

use crate::resolver_core::preferred_specifier_resolution::preferred_specifier_candidates;
use crate::resolver_core::project_ownership_resolution::{
    effective_configs_for_path, nearest_config_for_path, project_for_ownership,
    sorted_by_precedence,
};
use crate::resolver_core::provider_projection_resolution::project_exact_result;
use crate::resolver_core::provider_projection_resolution::{
    provider_id_for_source, provider_ide_id_for_source, source_id_from_provider_id,
};
use crate::resolver_core::{KernelAttempt, ResolutionBasis, ResolverAttemptView};

/// The real core type: an immutable, sorted-by-precedence project
/// config graph plus the four real public entry points assembled over
/// it. Construction is the ONLY place `configs` is sorted — every
/// query method assumes the one-time precedence-sort invariant already
/// holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleResolverCore {
    configs: Arc<[crate::resolver_core::IdeProjectConfig]>,
}

impl ModuleResolverCore {
    /// Sorts `projects` by precedence once, at construction.
    pub fn new(projects: Vec<crate::resolver_core::IdeProjectConfig>) -> Self {
        Self {
            configs: sorted_by_precedence(projects).into(),
        }
    }

    /// The full sorted config slice — exposed for callers that need
    /// direct owner-selection access without going through
    /// `nearest_config_for_path`/`effective_configs_for_path`.
    pub fn configs(&self) -> &[crate::resolver_core::IdeProjectConfig] {
        &self.configs
    }

    /// Returns every effective project configuration for `file_id`.
    pub fn effective_configs_for_path(
        &self,
        file_id: &str,
    ) -> Vec<&crate::resolver_core::IdeProjectConfig> {
        effective_configs_for_path(&self.configs, file_id)
    }

    /// Returns the highest-precedence effective configuration for `file_id`.
    pub fn nearest_config_for_path(
        &self,
        file_id: &str,
    ) -> Option<&crate::resolver_core::IdeProjectConfig> {
        nearest_config_for_path(&self.configs, file_id)
    }

    /// Resolves an immutable ownership identity to its unique configuration.
    pub fn project_for_ownership(
        &self,
        owner: &crate::resolver_core::ProjectOwnership,
    ) -> Option<&crate::resolver_core::IdeProjectConfig> {
        project_for_ownership(&self.configs, owner)
    }

    #[must_use]
    pub fn provider_id_for_source(&self, source_id: &str) -> Option<String> {
        provider_id_for_source(source_id)
    }

    #[must_use]
    pub fn provider_ide_id_for_source(&self, source_id: &str, is_jsx: bool) -> Option<String> {
        provider_ide_id_for_source(source_id, is_jsx)
    }

    #[must_use]
    pub fn source_id_from_provider_id(&self, provider_id: &str) -> Option<String> {
        source_id_from_provider_id(&self.configs, provider_id)
    }

    /// Runs one kernel attempt. The workspace-owned retry driver supplies the
    /// immutable `ResolverAttemptView` and restarts on `NeedInputs` or a basis
    /// mismatch.
    pub fn resolve_attempt(
        &self,
        view: &ResolverAttemptView,
        expected_basis: ResolutionBasis,
        request: &crate::resolver_core::ResolveRequest,
    ) -> KernelAttempt<Option<crate::resolver_core::ResolveResult>> {
        self.resolve_frame(request).attempt(view, expected_basis)
    }

    pub fn resolve_frame(
        &self,
        request: &crate::resolver_core::ResolveRequest,
    ) -> crate::resolver_core::ResolveFrame<'_> {
        crate::resolver_core::resolve_frame::ResolveFrame::for_request(&self.configs, request)
    }

    /// Runs one kernel attempt against the project identified by `owner`.
    pub fn resolve_for_project_attempt(
        &self,
        view: &ResolverAttemptView,
        expected_basis: ResolutionBasis,
        owner: &crate::resolver_core::ProjectOwnership,
        specifier: &str,
        ctx: crate::resolver_core::ResolutionContext,
    ) -> KernelAttempt<Option<crate::resolver_core::ResolveResult>> {
        self.resolve_for_project_frame(owner, specifier, ctx)
            .attempt(view, expected_basis)
    }

    pub fn resolve_for_project_frame(
        &self,
        owner: &crate::resolver_core::ProjectOwnership,
        specifier: &str,
        ctx: crate::resolver_core::ResolutionContext,
    ) -> crate::resolver_core::ResolveFrame<'_> {
        crate::resolver_core::resolve_frame::ResolveFrame::for_project(
            &self.configs,
            owner,
            specifier,
            ctx,
        )
    }

    /// Returns pure reverse-mapped specifier candidates; no
    /// `KernelAttempt` is involved.
    pub fn preferred_specifier_candidates(
        &self,
        importer_id: &str,
        target_id: &str,
    ) -> Option<Vec<String>> {
        preferred_specifier_candidates(&self.configs, importer_id, target_id)
    }

    /// Projects an already exact source identity into a pure resolve result.
    pub fn project_exact_result(
        &self,
        importer_id: &str,
        specifier: &str,
        source_id: String,
        context: crate::resolver_core::ResolutionContext,
    ) -> crate::resolver_core::ResolveResult {
        project_exact_result(&self.configs, importer_id, specifier, source_id, context)
    }
}

#[cfg(test)]
#[path = "module_resolver_core_tests.rs"]
mod module_resolver_core_tests;
