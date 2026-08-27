//! Top-level module-resolution orchestration.
//!
//! The public entry points are `resolve_with_reader`,
//! `resolve_for_project_with_reader`, `preferred_specifier`, and
//! `project_exact_result`. This module assembles
//! owner selection ([`super::project_ownership_resolution`]) ->
//! per-shape dispatch ([`super::source_id_resolution`]) -> provider-graph
//! projection ([`super::provider_projection_resolution`]), composing
//! the kernel's `ResolveResult` DTO.

#![allow(dead_code)]

use crate::resolver_core::project_ownership_resolution::{
    nearest_config_for_path, project_for_ownership,
};
use crate::resolver_core::provider_projection_resolution::{
    build_project_resolve_result, build_resolve_result,
};
use crate::resolver_core::source_id_resolution::{
    resolve_source_id, resolve_source_id_for_project, resolve_source_id_unowned,
};
use crate::resolver_core::{
    AttemptOutcome, AttemptOutput, CompletedAttempt, KernelAttempt, ResolutionBasis,
    ResolverAttemptView,
};

pub(crate) fn resolve_frame_with_reader(
    frame: &crate::resolver_core::resolve_frame::ResolveFrame<'_>,
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
) -> KernelAttempt<Option<crate::resolver_core::ResolveResult>> {
    if matches!(
        frame.operation,
        crate::resolver_core::resolve_frame::ResolveFrameOperation::Project
    ) && frame.geometry.owner_index.is_none()
    {
        return hit(None, AttemptOutput::new());
    }
    then(
        crate::resolver_core::source_id_resolution::resolve_source_id_frame(
            view,
            expected_basis,
            frame,
        ),
        AttemptOutput::new(),
        |resolved, output| {
            let Some((source_id, resolution_kind)) = resolved else {
                return hit(None, output);
            };
            let result = match &frame.operation {
                crate::resolver_core::resolve_frame::ResolveFrameOperation::Request => {
                    crate::resolver_core::provider_projection_resolution::build_resolve_result_with_memo(
                        frame.projects,
                        &frame.geometry.start,
                        &frame.geometry.specifier,
                        source_id,
                        resolution_kind,
                        &frame.memo,
                    )
                }
                crate::resolver_core::resolve_frame::ResolveFrameOperation::Project => crate::resolver_core::provider_projection_resolution::build_project_resolve_result_with_memo(
                    frame.projects,
                    &frame.geometry.specifier,
                    source_id,
                    resolution_kind,
                    &frame.memo,
                ),
            };
            hit(Some(result), output)
        },
    )
}

/// Local sequencing helper — see `node_modules_resolution.rs`'s own
/// `then` for the full rationale (kept local per-file, not a shared
/// abstraction).
fn then<T, U>(
    step: KernelAttempt<T>,
    prior_output: AttemptOutput,
    continue_with: impl FnOnce(T, AttemptOutput) -> KernelAttempt<U>,
) -> KernelAttempt<U> {
    match step {
        AttemptOutcome::NeedInputs(load_set) => AttemptOutcome::NeedInputs(load_set),
        AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
        AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
            let mut merged = prior_output;
            if let Err(failure) = merged.merge(output) {
                return AttemptOutcome::Terminal(failure);
            }
            continue_with(value, merged)
        }
    }
}

fn hit<T>(value: T, output: AttemptOutput) -> KernelAttempt<T> {
    AttemptOutcome::Complete(CompletedAttempt::new(value, output))
}

/// Select the importer's owning project (owner selection is pure — no observation,
/// no `KernelAttempt` involvement of its own), dispatch to
/// `resolve_source_id` (owned) or `resolve_source_id_unowned`
/// (unowned), then assemble the final `ResolveResult` via
/// `build_resolve_result`.
pub(crate) fn resolve_with_reader(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    projects: &[crate::resolver_core::IdeProjectConfig],
    request: &crate::resolver_core::ResolveRequest,
) -> KernelAttempt<Option<crate::resolver_core::ResolveResult>> {
    let importer_owner = nearest_config_for_path(projects, &request.importer_id);
    let ctx = crate::resolver_core::ResolutionContext {
        phase: request.phase,
        kind: request.kind,
    };

    let source_id_attempt = match importer_owner {
        Some(owner) => resolve_source_id(
            view,
            expected_basis,
            projects,
            owner,
            &request.importer_id,
            &request.specifier,
            ctx,
        ),
        None => resolve_source_id_unowned(
            view,
            expected_basis,
            &request.importer_id,
            &request.specifier,
            ctx,
        ),
    };

    then(
        source_id_attempt,
        AttemptOutput::new(),
        move |resolved, output| match resolved {
            Some((source_id, resolution_kind)) => {
                let result = build_resolve_result(projects, request, source_id, resolution_kind);
                hit(Some(result), output)
            }
            None => hit(None, output),
        },
    )
}

/// Resolve `owner` to the exact `IdeProjectConfig` it names (a genuine
/// duplicate or a miss both refuse with `None`, matching
/// `project_for_ownership`), dispatch to `resolve_source_id_for_project`,
/// then assemble via `build_project_resolve_result`.
pub(crate) fn resolve_for_project_with_reader(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    projects: &[crate::resolver_core::IdeProjectConfig],
    owner: &crate::resolver_core::ProjectOwnership,
    specifier: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<crate::resolver_core::ResolveResult>> {
    let Some(project) = project_for_ownership(projects, owner) else {
        return hit(None, AttemptOutput::new());
    };

    then(
        resolve_source_id_for_project(view, expected_basis, projects, project, specifier, ctx),
        AttemptOutput::new(),
        move |resolved, output| match resolved {
            Some((source_id, resolution_kind)) => {
                let result =
                    build_project_resolve_result(projects, specifier, source_id, resolution_kind);
                hit(Some(result), output)
            }
            None => hit(None, output),
        },
    )
}

#[cfg(test)]
#[path = "top_level_resolution_tests.rs"]
mod top_level_resolution_tests;
