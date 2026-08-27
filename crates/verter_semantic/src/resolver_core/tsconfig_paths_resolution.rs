//! Tsconfig path and mapping-target resolution over `ResolverAttemptView`
//! and `priority_frontier`.
//!
//! `resolve_path_mapping_target` is the SAME primitive workspace
//! aliases, tsconfig `paths`/`baseUrl`, and project references all
//! bottom out in — but its own manifest-then-exports-then-legacy shape
//! is genuinely DIFFERENT from `node_modules_resolution`'s per-directory
//! step: here, a miss on `exports` (or a missing `types` entry) still
//! falls through to `resolve_legacy_package` AT THE SAME candidate
//! directory (never `continue`s to a different directory — there IS no
//! outer directory loop), whereas the node_modules per-directory step
//! treats an `exports`-present manifest as exclusive of the legacy path
//! and moves to the next ancestor directory instead. The two shapes are
//! intentionally not interchangeable.

#![allow(dead_code)]

use std::sync::Arc;

use rustc_hash::FxHashSet;

use crate::resolver_core::package_target_resolution::{
    capture_tsconfig_pattern, prefers_declaration_files, probe_for_ctx,
    read_package_manifest_if_present, resolve_legacy_package, resolve_manifest_types_entry,
    resolve_package_exports,
};
use crate::resolver_core::priority_frontier::priority_frontier_with_budgets;
use crate::resolver_core::priority_frontier::PriorityFrontierState;
use crate::resolver_core::{
    AttemptOutcome, AttemptOutput, CompletedAttempt, KernelAttempt, ResolutionBasis,
    ResolverAttemptView,
};

#[cfg(test)]
thread_local! {
    static PATH_MAPPING_CANDIDATE_EVALUATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static WORKSPACE_ALIAS_EVALUATIONS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn take_path_mapping_candidate_evaluations() -> usize {
    PATH_MAPPING_CANDIDATE_EVALUATIONS.with(|count| count.replace(0))
}

#[cfg(test)]
pub(crate) fn take_workspace_alias_evaluations() -> Vec<String> {
    WORKSPACE_ALIAS_EVALUATIONS.with(|aliases| std::mem::take(&mut *aliases.borrow_mut()))
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

/// Mirrors `apply_tsconfig_target` exactly — pure string logic, no I/O.
pub(crate) fn apply_tsconfig_target(base_url: &str, target: &str, captured: &str) -> String {
    let replaced = if let Some(star) = target.find('*') {
        format!("{}{}{}", &target[..star], captured, &target[star + 1..])
    } else {
        target.to_string()
    };
    if crate::resolver_core::is_absolute_specifier(&replaced) {
        crate::resolver_core::normalize_canonical_id(&replaced)
    } else {
        crate::resolver_core::join_paths(base_url, &replaced)
    }
}

/// Mirrors `resolve_path_mapping_target`'s `exports`-then-`types`
/// sub-chain exactly — a miss here (whichever branch produces it) falls
/// through to `resolve_legacy_package` at the SAME directory, one level
/// up in [`resolve_via_package_manifest`].
fn resolve_via_exports_then_types(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    normalized: &str,
    package_json: &crate::resolver_core::ResolutionPackageManifest,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<String>> {
    let Some(exports) = package_json.exports.clone() else {
        return hit(None, AttemptOutput::new());
    };

    then(
        resolve_package_exports(view, expected_basis, normalized, &exports, ".", ctx),
        AttemptOutput::new(),
        move |resolved, output| match resolved {
            Some(resolved) => hit(Some(resolved), output),
            None => {
                if prefers_declaration_files(ctx) {
                    then(
                        resolve_manifest_types_entry(
                            view,
                            expected_basis,
                            normalized,
                            package_json,
                        ),
                        output,
                        hit,
                    )
                } else {
                    hit(None, output)
                }
            }
        },
    )
}

/// Mirrors `resolve_path_mapping_target`'s manifest-present branch
/// exactly (the `if let Some(package_json) = ... { ... }` body):
/// `exports`-then-`types`, and on a miss there, `resolve_legacy_package`
/// at the same directory — never advancing to a different candidate.
fn resolve_via_package_manifest(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    normalized: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<String>> {
    then(
        read_package_manifest_if_present(view, normalized),
        AttemptOutput::new(),
        move |package_json, output| {
            let Some(package_json) = package_json else {
                return hit(None, output);
            };
            then(
                resolve_via_exports_then_types(
                    view,
                    expected_basis,
                    normalized,
                    &package_json,
                    ctx,
                ),
                output,
                move |resolved, output| match resolved {
                    Some(resolved) => hit(Some(resolved), output),
                    None => then(
                        resolve_legacy_package(
                            view,
                            expected_basis,
                            normalized,
                            &package_json,
                            "",
                            ctx,
                        ),
                        output,
                        hit,
                    ),
                },
            )
        },
    )
}

/// Mirrors `resolve_path_mapping_target` exactly: try the candidate as a
/// package directory (manifest -> exports/types/legacy) first, and on a
/// miss there, fall through to the bare `probe_path_for_context` probe.
pub(crate) fn resolve_path_mapping_target(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    candidate: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<String>> {
    let normalized = crate::resolver_core::normalize_canonical_id(candidate);

    then(
        resolve_via_package_manifest(view, expected_basis, &normalized, ctx),
        AttemptOutput::new(),
        move |resolved, output| match resolved {
            Some(resolved) => hit(Some(resolved), output),
            None => then(
                probe_for_ctx(view, expected_basis, &normalized, ctx),
                output,
                hit,
            ),
        },
    )
}

/// The ONE ordered candidate list a `resolve_tsconfig_paths` call
/// produces, flattened — pure string logic (pattern matching +
/// wildcard substitution), no I/O, matching `probe_path_resolution`'s
/// own candidate-list-then-evaluate shape.
fn tsconfig_path_candidates<'a>(
    project: &'a crate::resolver_core::IdeProjectConfig,
    specifier: &'a str,
) -> impl Iterator<Item = Arc<str>> + 'a {
    let base_url = project
        .compiler_options
        .base_url
        .as_deref()
        .unwrap_or(project.root.as_str());

    let candidates = project
        .compiler_options
        .paths
        .iter()
        .flat_map(move |(pattern, targets)| {
            let captured = capture_tsconfig_pattern(pattern, specifier);
            let mut seen_targets = FxHashSet::default();
            targets
                .iter()
                .filter(move |target| seen_targets.insert(target.as_str()))
                .filter_map(move |target| {
                    captured.map(|captured| {
                        Arc::from(apply_tsconfig_target(base_url, target, captured))
                    })
                })
        });
    let mut seen_candidates = FxHashSet::default();
    candidates.filter(move |candidate| seen_candidates.insert(Arc::clone(candidate)))
}

/// Mirrors `resolve_tsconfig_paths` exactly: every `paths` pattern that
/// matches `specifier`, in declared order, each of its `targets` in
/// declared order — first candidate to resolve wins.
pub(crate) fn resolve_tsconfig_paths(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    project: &crate::resolver_core::IdeProjectConfig,
    specifier: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<String>> {
    let candidates = tsconfig_path_candidates(project, specifier);
    priority_frontier_with_budgets(
        expected_basis,
        view.input_resolution_budgets(),
        candidates,
        |candidate| resolve_path_mapping_target(view, expected_basis, &candidate, ctx),
    )
}

/// Mirrors `sorted_workspace_aliases` exactly — pure sort, no I/O.
/// Longest `find` prefix wins ties broken by lexical order, matching
/// the legacy comparator field-for-field.
pub(crate) fn sorted_workspace_aliases(
    aliases: &[crate::resolver_core::WorkspaceAlias],
) -> Vec<&crate::resolver_core::WorkspaceAlias> {
    let mut aliases = aliases.iter().collect::<Vec<_>>();
    aliases.sort_by(|a, b| {
        b.find
            .len()
            .cmp(&a.find.len())
            .then_with(|| a.find.cmp(&b.find))
    });
    aliases
}

fn resolve_workspace_aliases(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    aliases: &[crate::resolver_core::WorkspaceAlias],
    specifier: &str,
    mut evaluate: impl FnMut(
        &crate::resolver_core::WorkspaceAlias,
        String,
    ) -> KernelAttempt<Option<String>>,
) -> KernelAttempt<Option<String>> {
    let ordered = sorted_workspace_aliases(aliases);
    let mut frontier =
        PriorityFrontierState::new_with_budgets(expected_basis, view.input_resolution_budgets());
    for (ordinal, alias) in ordered.iter().enumerate() {
        let Some(remainder) = specifier.strip_prefix(alias.find.as_str()) else {
            continue;
        };
        let lease = match view.input_resolution_retention().retain_alias_geometry() {
            Ok(lease) => lease,
            Err(failure) => return AttemptOutcome::Terminal(failure),
        };
        let candidate = crate::resolver_core::join_paths(&alias.replacement, remainder);
        let duplicate = ordered[..ordinal].iter().any(|prior| {
            specifier
                .strip_prefix(prior.find.as_str())
                .is_some_and(|prior_remainder| {
                    crate::resolver_core::join_paths(&prior.replacement, prior_remainder)
                        == candidate
                })
        });
        if duplicate {
            drop(lease);
            continue;
        }
        #[cfg(test)]
        WORKSPACE_ALIAS_EVALUATIONS.with(|evaluations| {
            evaluations.borrow_mut().push(alias.find.clone());
        });
        let outcome = frontier.push(evaluate(alias, candidate));
        drop(lease);
        if let Some(outcome) = outcome {
            return outcome;
        }
    }
    frontier.finish()
}

/// Shared workspace-alias -> tsconfig `paths` -> `baseUrl` sequence used by
/// source-ID and project-reference resolution for one `IdeProjectConfig`.
pub(crate) fn resolve_via_workspace_config(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    project: &crate::resolver_core::IdeProjectConfig,
    specifier: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<(String, crate::resolver_core::ResolutionKind)>> {
    let alias_resolution = resolve_workspace_aliases(
        view,
        expected_basis,
        &project.workspace_aliases,
        specifier,
        |_alias, candidate| resolve_path_mapping_target(view, expected_basis, &candidate, ctx),
    );

    then(
        alias_resolution,
        AttemptOutput::new(),
        move |resolved, output| {
            if let Some(resolved) = resolved {
                return hit(
                    Some((
                        resolved,
                        crate::resolver_core::ResolutionKind::WorkspaceAlias,
                    )),
                    output,
                );
            }

            then(
                resolve_tsconfig_paths(view, expected_basis, project, specifier, ctx),
                output,
                move |resolved, output| {
                    if let Some(resolved) = resolved {
                        return hit(
                            Some((resolved, crate::resolver_core::ResolutionKind::TsConfigPath)),
                            output,
                        );
                    }

                    match project.compiler_options.base_url.as_deref() {
                        Some(base_url) => {
                            let base = crate::resolver_core::join_paths(base_url, specifier);
                            then(
                                resolve_path_mapping_target(view, expected_basis, &base, ctx),
                                output,
                                |resolved, output| {
                                    let value = resolved.map(|r| {
                                        (r, crate::resolver_core::ResolutionKind::TsConfigPath)
                                    });
                                    hit(value, output)
                                },
                            )
                        }
                        None => hit(None, output),
                    }
                },
            )
        },
    )
}

fn resolve_path_mapping_candidate(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    candidate: &crate::resolver_core::resolve_frame::MappingCandidate,
    ctx: crate::resolver_core::ResolutionContext,
    memo: &crate::resolver_core::resolve_frame::ResolutionStringMemo,
) -> KernelAttempt<Option<String>> {
    #[cfg(test)]
    PATH_MAPPING_CANDIDATE_EVALUATIONS.with(|count| count.set(count.get() + 1));
    then(
        crate::resolver_core::package_target_resolution::read_package_manifest_with_memo(
            view,
            &candidate.normalized,
            memo,
        ),
        AttemptOutput::new(),
        move |package_json, output| {
            let Some(package_json) = package_json else {
                return then(
                    crate::resolver_core::probe_path_resolution::evaluate_probe_candidates(
                        view,
                        expected_basis,
                        &candidate.probe_candidates,
                    ),
                    output,
                    hit,
                );
            };

            let has_exports = package_json.exports.is_some();
            let exports_attempt = package_json.exports.as_ref().map_or_else(
                || hit(None, AttemptOutput::new()),
                |exports| {
                    crate::resolver_core::package_target_resolution::resolve_package_exports_with_memo(
                        view,
                        expected_basis,
                        &candidate.normalized,
                        exports,
                        ".",
                        ctx,
                        memo,
                    )
                },
            );
            then(exports_attempt, output, move |resolved, output| {
                let types_attempt = if has_exports
                    && resolved.is_none()
                    && crate::resolver_core::package_target_resolution::prefers_declaration_files(
                        ctx,
                    ) {
                    crate::resolver_core::package_target_resolution::resolve_manifest_types_entry_with_memo(
                        view,
                        expected_basis,
                        &candidate.normalized,
                        &package_json,
                        memo,
                    )
                } else {
                    hit(resolved, AttemptOutput::new())
                };
                then(types_attempt, output, move |resolved, output| {
                    match resolved {
                    Some(resolved) => hit(Some(resolved), output),
                    None => then(
                        crate::resolver_core::package_target_resolution::resolve_legacy_package_with_memo(
                            view,
                            expected_basis,
                            &candidate.normalized,
                            &package_json,
                            "",
                            ctx,
                            memo,
                        ),
                        output,
                        hit,
                    ),
                }
                })
            })
        },
    )
}

pub(crate) fn resolve_via_workspace_config_frame(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    frame: &crate::resolver_core::ResolveFrame<'_>,
    project: &crate::resolver_core::IdeProjectConfig,
) -> KernelAttempt<Option<(String, crate::resolver_core::ResolutionKind)>> {
    let Some(geometry) = frame.project_geometry(project) else {
        return hit(None, AttemptOutput::new());
    };
    let ctx = frame.geometry.context;
    let alias_resolution = resolve_workspace_aliases(
        view,
        expected_basis,
        &project.workspace_aliases,
        &frame.geometry.specifier,
        |_alias, normalized| {
            // Workspace-alias geometry is live only for this candidate
            // evaluation. Keep every candidate-exclusive memo entry under
            // the same lease owned by `resolve_workspace_aliases`; retaining
            // it in `ResolveFrame::memo` would outlive the lease across
            // same-basis NeedInputs waves.
            let alias_memo = crate::resolver_core::resolve_frame::ResolutionStringMemo::default();
            let normalized = alias_memo.normalize(&normalized);
            let candidate = crate::resolver_core::resolve_frame::MappingCandidate {
                probe_candidates: alias_memo.probe_candidates(
                    &normalized,
                    frame.geometry.apply_source_sibling,
                    frame.geometry.prefers_declarations,
                ),
                normalized,
            };
            resolve_path_mapping_candidate(view, expected_basis, &candidate, ctx, &alias_memo)
        },
    );
    then(
        alias_resolution,
        AttemptOutput::new(),
        move |resolved, output| {
            if let Some(resolved) = resolved {
                return hit(
                    Some((
                        resolved,
                        crate::resolver_core::ResolutionKind::WorkspaceAlias,
                    )),
                    output,
                );
            }
            then(
                priority_frontier_with_budgets(
                    expected_basis,
                    view.input_resolution_budgets(),
                    frame.path_candidates(project),
                    |candidate| {
                        resolve_path_mapping_candidate(
                            view,
                            expected_basis,
                            &candidate,
                            ctx,
                            &frame.memo,
                        )
                    },
                ),
                output,
                move |resolved, output| {
                    if let Some(resolved) = resolved {
                        return hit(
                            Some((resolved, crate::resolver_core::ResolutionKind::TsConfigPath)),
                            output,
                        );
                    }
                    match &geometry.base_url_candidate {
                        Some(candidate) => then(
                            resolve_path_mapping_candidate(
                                view,
                                expected_basis,
                                candidate,
                                ctx,
                                &frame.memo,
                            ),
                            output,
                            |resolved, output| {
                                hit(
                                    resolved.map(|resolved| {
                                        (
                                            resolved,
                                            crate::resolver_core::ResolutionKind::TsConfigPath,
                                        )
                                    }),
                                    output,
                                )
                            },
                        ),
                        None => hit(None, output),
                    }
                },
            )
        },
    )
}

#[cfg(test)]
#[path = "tsconfig_paths_resolution_tests.rs"]
mod tsconfig_paths_resolution_tests;
