//! Project-reference resolution over `ResolverAttemptView` and
//! `priority_frontier`.
//!
//! `projects: &[IdeProjectConfig]` is passed directly as plain
//! in-memory configuration data, not an observation — workspace project
//! configs are resident, resolved workspace state (like the single
//! `project: &IdeProjectConfig` already threaded through
//! `resolve_tsconfig_paths`/`resolve_via_workspace_config`), never a
//! lazily-loaded filesystem fact behind the 3-primitive observation
//! seam. This recursive descent is graph traversal over that resident
//! config plus the three module-resolution I/O
//! pieces — no new observation primitive needed.
//!
//! Each per-reference loop is a priority frontier (first reference to
//! produce a hit wins). Transitive descent uses an explicit heap-backed
//! frame stack so the project graph depth does not also become native call
//! stack depth; every frame carries the same resumable frontier state and
//! push-before-descend/pop-after-return cycle state as the recursive form.

#![allow(dead_code)]

use std::collections::HashSet;

use crate::resolver_core::priority_frontier::PriorityFrontierState;
use crate::resolver_core::tsconfig_paths_resolution::resolve_via_workspace_config;
use crate::resolver_core::{
    AttemptFailure, AttemptOutcome, AttemptOutput, CompletedAttempt, KernelAttempt,
    ResolutionBasis, ResolverAttemptView,
};

fn hit<T>(value: T, output: AttemptOutput) -> KernelAttempt<T> {
    AttemptOutcome::Complete(CompletedAttempt::new(value, output))
}

/// Mirrors `PROJECT_REFERENCE_DEPTH_LIMIT` exactly.
const PROJECT_REFERENCE_DEPTH_LIMIT: u32 = 256;

/// Mirrors `ProjectReferenceTraversalState` exactly.
pub(crate) struct ProjectReferenceTraversalState {
    active: HashSet<String>,
    remaining_depth: u32,
}

impl ProjectReferenceTraversalState {
    /// Mirrors `ProjectReferenceTraversalState::seeded_with` exactly.
    pub(crate) fn seeded_with(importer_tsconfig: Option<&str>) -> Self {
        let mut active = HashSet::new();
        if let Some(tsconfig) = importer_tsconfig {
            active.insert(tsconfig.to_string());
        }
        Self {
            active,
            remaining_depth: PROJECT_REFERENCE_DEPTH_LIMIT,
        }
    }
}

struct TraversalFrame<'a> {
    owner: &'a crate::resolver_core::IdeProjectConfig,
    next_reference: usize,
    frontier: PriorityFrontierState,
    deferred_depth_limit: Option<AttemptFailure>,
    entered_reference: Option<String>,
    prior_output: AttemptOutput,
}

impl<'a> TraversalFrame<'a> {
    fn root(
        owner: &'a crate::resolver_core::IdeProjectConfig,
        expected_basis: ResolutionBasis,
        budgets: crate::resolver_core::InputResolutionBudgets,
    ) -> Self {
        Self {
            owner,
            next_reference: 0,
            frontier: PriorityFrontierState::new_with_budgets(expected_basis, budgets),
            deferred_depth_limit: None,
            entered_reference: None,
            prior_output: AttemptOutput::new(),
        }
    }

    fn child(
        owner: &'a crate::resolver_core::IdeProjectConfig,
        expected_basis: ResolutionBasis,
        budgets: crate::resolver_core::InputResolutionBudgets,
        entered_reference: String,
        prior_output: AttemptOutput,
    ) -> Self {
        Self {
            owner,
            next_reference: 0,
            frontier: PriorityFrontierState::new_with_budgets(expected_basis, budgets),
            deferred_depth_limit: None,
            entered_reference: Some(entered_reference),
            prior_output,
        }
    }

    /// A path-local depth failure is retained while lower-priority siblings
    /// are tried. A later hit proves the request without needing the truncated
    /// arm; if the frontier exhausts, the retained typed failure is returned.
    fn push_candidate(
        &mut self,
        candidate: KernelAttempt<Option<String>>,
    ) -> Option<KernelAttempt<Option<String>>> {
        match candidate {
            AttemptOutcome::Terminal(
                failure @ AttemptFailure::InputResolutionDepthLimit { .. },
            ) => {
                if self.deferred_depth_limit.is_none() {
                    self.deferred_depth_limit = Some(failure);
                }
                None
            }
            AttemptOutcome::Terminal(_) if self.deferred_depth_limit.is_some() => {
                Some(AttemptOutcome::Terminal(
                    self.deferred_depth_limit
                        .take()
                        .expect("the guarded deferred depth failure must exist"),
                ))
            }
            candidate => self.frontier.push(candidate),
        }
    }

    fn finish(&mut self) -> KernelAttempt<Option<String>> {
        let outcome = self.frontier.finish();
        match outcome {
            AttemptOutcome::Complete(CompletedAttempt { value: None, .. }) => self
                .deferred_depth_limit
                .take()
                .map_or(outcome, AttemptOutcome::Terminal),
            outcome => outcome,
        }
    }
}

/// Walk each declared reference in order, skipping a back-edge already on the
/// active descent path, first hit (aliases/paths/`baseUrl` at that
/// project, THEN transitive descent bounded by the depth fuse) wins.
pub(crate) fn resolve_project_references_inner<'a>(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    projects: &'a [crate::resolver_core::IdeProjectConfig],
    importer_owner: &'a crate::resolver_core::IdeProjectConfig,
    specifier: &str,
    ctx: crate::resolver_core::ResolutionContext,
    state: &mut ProjectReferenceTraversalState,
) -> KernelAttempt<Option<String>> {
    let mut frames = vec![TraversalFrame::root(
        importer_owner,
        expected_basis,
        view.input_resolution_budgets(),
    )];
    let mut completed_frame = None;

    loop {
        if let Some(mut outcome) = completed_frame.take() {
            let finished = frames
                .pop()
                .expect("a completed traversal frame must exist");
            let Some(entered_reference) = finished.entered_reference else {
                return outcome;
            };

            state.active.remove(entered_reference.as_str());
            state.remaining_depth += 1;

            if let AttemptOutcome::Complete(CompletedAttempt { value, output }) = outcome {
                let mut merged = finished.prior_output;
                outcome = match merged.merge(output) {
                    Ok(()) => hit(value, merged),
                    Err(failure) => AttemptOutcome::Terminal(failure),
                };
            }

            let parent = frames
                .last_mut()
                .expect("a non-root traversal frame must have a parent");
            completed_frame = parent.push_candidate(outcome);
            continue;
        }

        let frame = frames
            .last_mut()
            .expect("project-reference traversal always retains its root frame");
        let Some(reference) = frame.owner.references.get(frame.next_reference).cloned() else {
            completed_frame = Some(frame.finish());
            continue;
        };
        frame.next_reference += 1;

        // Back-edge to a project already on the active descent path.
        if state.active.contains(reference.as_str()) {
            completed_frame = frame.push_candidate(hit(None, AttemptOutput::new()));
            continue;
        }

        let Some(project) = projects
            .iter()
            .find(|candidate| candidate.tsconfig_path.as_deref() == Some(reference.as_str()))
        else {
            completed_frame = frame.push_candidate(hit(None, AttemptOutput::new()));
            continue;
        };

        let candidate =
            match resolve_via_workspace_config(view, expected_basis, project, specifier, ctx) {
                AttemptOutcome::NeedInputs(load_set) => AttemptOutcome::NeedInputs(load_set),
                AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
                AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
                    if let Some((resolved, _kind)) = value {
                        hit(Some(resolved), output)
                    } else {
                        let has_unwalked_reference = project.references.iter().any(|candidate| {
                            candidate != &reference
                                && !state.active.contains(candidate.as_str())
                                && projects.iter().any(|project| {
                                    project.tsconfig_path.as_deref() == Some(candidate.as_str())
                                })
                        });
                        if !has_unwalked_reference {
                            hit(None, output)
                        } else if state.remaining_depth == 0 {
                            AttemptOutcome::Terminal(AttemptFailure::InputResolutionDepthLimit {
                                unresolved: Vec::new(),
                                depth: PROJECT_REFERENCE_DEPTH_LIMIT,
                            })
                        } else {
                            state.remaining_depth -= 1;
                            state.active.insert(reference.clone());
                            frames.push(TraversalFrame::child(
                                project,
                                expected_basis,
                                view.input_resolution_budgets(),
                                reference,
                                output,
                            ));
                            continue;
                        }
                    }
                }
            };

        let frame = frames
            .last_mut()
            .expect("candidate evaluation retains its traversal frame");
        completed_frame = frame.push_candidate(candidate);
    }
}

/// Seeds the traversal state with the importer's own tsconfig (so a reference
/// back-edge to the importer terminates instead of reprocessing it),
/// then delegates to the bounded recursive walk.
pub(crate) fn resolve_project_references(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    projects: &[crate::resolver_core::IdeProjectConfig],
    importer_owner: &crate::resolver_core::IdeProjectConfig,
    specifier: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<String>> {
    let mut state =
        ProjectReferenceTraversalState::seeded_with(importer_owner.tsconfig_path.as_deref());
    resolve_project_references_inner(
        view,
        expected_basis,
        projects,
        importer_owner,
        specifier,
        ctx,
        &mut state,
    )
}

pub(crate) fn resolve_project_references_frame(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    frame: &crate::resolver_core::ResolveFrame<'_>,
    importer_owner: &crate::resolver_core::IdeProjectConfig,
) -> KernelAttempt<Option<String>> {
    let mut state =
        ProjectReferenceTraversalState::seeded_with(importer_owner.tsconfig_path.as_deref());
    let mut frames = vec![TraversalFrame::root(
        importer_owner,
        expected_basis,
        view.input_resolution_budgets(),
    )];
    let mut completed_frame = None;

    loop {
        if let Some(mut outcome) = completed_frame.take() {
            let finished = frames
                .pop()
                .expect("a completed traversal frame must exist");
            let Some(entered_reference) = finished.entered_reference else {
                return outcome;
            };
            state.active.remove(entered_reference.as_str());
            state.remaining_depth += 1;
            if let AttemptOutcome::Complete(CompletedAttempt { value, output }) = outcome {
                let mut merged = finished.prior_output;
                outcome = match merged.merge(output) {
                    Ok(()) => hit(value, merged),
                    Err(failure) => AttemptOutcome::Terminal(failure),
                };
            }
            let parent = frames
                .last_mut()
                .expect("a non-root traversal frame must have a parent");
            completed_frame = parent.push_candidate(outcome);
            continue;
        }

        let traversal = frames
            .last_mut()
            .expect("project-reference traversal always retains its root frame");
        let Some(reference) = traversal
            .owner
            .references
            .get(traversal.next_reference)
            .cloned()
        else {
            completed_frame = Some(traversal.finish());
            continue;
        };
        traversal.next_reference += 1;
        if state.active.contains(reference.as_str()) {
            completed_frame = traversal.push_candidate(hit(None, AttemptOutput::new()));
            continue;
        }
        let Some(project) = frame
            .projects
            .iter()
            .find(|candidate| candidate.tsconfig_path.as_deref() == Some(reference.as_str()))
        else {
            completed_frame = traversal.push_candidate(hit(None, AttemptOutput::new()));
            continue;
        };

        let candidate = match crate::resolver_core::tsconfig_paths_resolution::resolve_via_workspace_config_frame(
            view,
            expected_basis,
            frame,
            project,
        ) {
            AttemptOutcome::NeedInputs(load_set) => AttemptOutcome::NeedInputs(load_set),
            AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
            AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
                if let Some((resolved, _)) = value {
                    hit(Some(resolved), output)
                } else {
                    let has_unwalked_reference = project.references.iter().any(|candidate| {
                        candidate != &reference
                            && !state.active.contains(candidate.as_str())
                            && frame.projects.iter().any(|project| {
                                project.tsconfig_path.as_deref() == Some(candidate.as_str())
                            })
                    });
                    if !has_unwalked_reference {
                        hit(None, output)
                    } else if state.remaining_depth == 0 {
                        AttemptOutcome::Terminal(AttemptFailure::InputResolutionDepthLimit {
                            unresolved: Vec::new(),
                            depth: PROJECT_REFERENCE_DEPTH_LIMIT,
                        })
                    } else {
                        state.remaining_depth -= 1;
                        state.active.insert(reference.clone());
                        frames.push(TraversalFrame::child(
                            project,
                            expected_basis,
                            view.input_resolution_budgets(),
                            reference,
                            output,
                        ));
                        continue;
                    }
                }
            }
        };
        let traversal = frames
            .last_mut()
            .expect("candidate evaluation retains its traversal frame");
        completed_frame = traversal.push_candidate(candidate);
    }
}

#[cfg(test)]
#[path = "project_references_resolution_tests.rs"]
mod project_references_resolution_tests;
