//! Unowned source-ID resolution over `ResolverAttemptView` and
//! `priority_frontier`.
//!
//! This is the narrowest public entry: no owning project, so no workspace aliases/tsconfig
//! `paths`/project references to consult): relative/absolute specifiers
//! probe directly (gated by [`package_follow_is_confirmed`]'s
//! `node_modules` re-entry guard), `#` specifiers are `#imports`
//! (unbounded ancestor walk — no `workspace_root` boundary), everything
//! else is a `node_modules` package lookup (also unbounded). Composes
//! the shared probe and package-resolution pieces
//! ([`super::probe_path_resolution::probe_path_for_context`],
//! [`super::node_modules_resolution`]); the only local policy is
//! `package_follow_is_confirmed`'s own manifest-boundary check.

#![allow(dead_code)]

use crate::resolver_core::node_modules_resolution::{
    resolve_node_modules_package, resolve_package_imports,
};
use crate::resolver_core::package_target_resolution::{
    probe_for_ctx, read_package_manifest_if_present,
};
use crate::resolver_core::{
    AttemptOutcome, AttemptOutput, CompletedAttempt, KernelAttempt, ResolutionBasis,
    ResolverAttemptView,
};

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

type SourceIdHit = Option<(String, crate::resolver_core::ResolutionKind)>;

fn hit(value: SourceIdHit, output: AttemptOutput) -> KernelAttempt<SourceIdHit> {
    AttemptOutcome::Complete(CompletedAttempt::new(value, output))
}

fn miss<T>(output: AttemptOutput) -> KernelAttempt<Option<T>> {
    AttemptOutcome::Complete(CompletedAttempt::new(None, output))
}

/// Mirrors `normalized_starts_with` exactly — pure string logic, no I/O.
/// `pub(crate)` — reused by `project_ownership_resolution`'s own
/// nearest-root pruning, which needs the identical prefix check.
pub(crate) fn normalized_starts_with(path: &str, prefix: &str) -> bool {
    let normalized = crate::resolver_core::normalize_canonical_id(path);
    let prefix = crate::resolver_core::normalize_canonical_id(prefix);
    normalized.starts_with(&prefix)
        && (normalized.len() == prefix.len()
            || prefix.ends_with('/')
            || normalized.as_bytes().get(prefix.len()) == Some(&b'/'))
}

/// Mirrors `candidate_package_dir_for_importer` exactly — pure string
/// logic, no I/O.
fn candidate_package_dir_for_importer(importer_id: &str) -> Option<String> {
    let normalized = crate::resolver_core::normalize_canonical_id(importer_id);
    let node_modules_marker = "/node_modules/";
    let marker_index = normalized.rfind(node_modules_marker)?;
    let package_start = marker_index + node_modules_marker.len();
    let package_path = &normalized[package_start..];

    let mut parts = package_path.split('/');
    let first = parts.next()?;
    let package_rel = if first.starts_with('@') {
        let second = parts.next()?;
        format!("{first}/{second}")
    } else {
        first.to_string()
    };

    Some(format!(
        "{}{node_modules_marker}{package_rel}",
        &normalized[..marker_index]
    ))
}

/// Mirrors `package_follow_is_confirmed` exactly: an importer NOT itself
/// inside `node_modules` always confirms (nothing to re-enter); an
/// importer inside `node_modules` confirms only when the OWNING
/// package's manifest exists AND the resolved target stayed within that
/// same package directory (guards against a relative specifier walking
/// `..` out of the declared package boundary).
fn package_follow_is_confirmed(
    view: &ResolverAttemptView,
    importer_id: &str,
    resolved: &str,
) -> KernelAttempt<bool> {
    let Some(package_dir) = candidate_package_dir_for_importer(importer_id) else {
        return AttemptOutcome::Complete(CompletedAttempt::new(true, AttemptOutput::new()));
    };

    then(
        read_package_manifest_if_present(view, &package_dir),
        AttemptOutput::new(),
        move |manifest, output| {
            let confirmed = manifest.is_some() && normalized_starts_with(resolved, &package_dir);
            AttemptOutcome::Complete(CompletedAttempt::new(confirmed, output))
        },
    )
}

/// Mirrors `resolve_source_id_unowned`'s relative/absolute branch
/// exactly: probe the joined/absolute path, then confirm the
/// `node_modules` re-entry boundary.
fn evaluate_relative_or_absolute(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    importer_id: &str,
    specifier: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<SourceIdHit> {
    let importer_dir = crate::resolver_core::parent_dir(importer_id);
    let base = if crate::resolver_core::is_absolute_specifier(specifier) {
        crate::resolver_core::normalize_canonical_id(specifier)
    } else {
        crate::resolver_core::join_paths(&importer_dir, specifier)
    };

    then(
        probe_for_ctx(view, expected_basis, &base, ctx),
        AttemptOutput::new(),
        move |resolved, output| {
            let Some(resolved) = resolved else {
                return miss(output);
            };
            then(
                package_follow_is_confirmed(view, importer_id, &resolved),
                output,
                move |confirmed, output| {
                    if confirmed {
                        hit(
                            Some((resolved, crate::resolver_core::ResolutionKind::Relative)),
                            output,
                        )
                    } else {
                        miss(output)
                    }
                },
            )
        },
    )
}

/// Resolve without a configured owner: relative/absolute -> `#imports`
/// (unbounded) -> `node_modules`
/// (unbounded). Each branch is a DISTINCT specifier-shape dispatch
/// (`if`/`else if`/`else`, not a `priority_frontier` — the branches are
/// mutually exclusive by specifier shape, never competing candidates).
pub(crate) fn resolve_source_id_unowned(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    importer_id: &str,
    specifier: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<SourceIdHit> {
    if crate::resolver_core::is_relative_specifier(specifier)
        || crate::resolver_core::is_absolute_specifier(specifier)
    {
        return evaluate_relative_or_absolute(view, expected_basis, importer_id, specifier, ctx);
    }

    if specifier.starts_with('#') {
        return then(
            resolve_package_imports(view, expected_basis, importer_id, specifier, ctx, None),
            AttemptOutput::new(),
            |resolved, output| {
                let value =
                    resolved.map(|r| (r, crate::resolver_core::ResolutionKind::PackageImports));
                hit(value, output)
            },
        );
    }

    resolve_node_modules_package(view, expected_basis, importer_id, specifier, ctx, None)
}

/// Resolve within the importer's owning project: relative/absolute (no
/// `package_follow_is_confirmed` gate here — that guard is
/// `resolve_source_id_unowned`-only) -> workspace aliases/tsconfig
/// `paths`/`baseUrl` -> project references -> `#imports` (bounded by
/// `workspace_root`) -> `node_modules` (bounded by `workspace_root`).
pub(crate) fn resolve_source_id(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    projects: &[crate::resolver_core::IdeProjectConfig],
    importer_owner: &crate::resolver_core::IdeProjectConfig,
    importer_id: &str,
    specifier: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<SourceIdHit> {
    if crate::resolver_core::is_relative_specifier(specifier)
        || crate::resolver_core::is_absolute_specifier(specifier)
    {
        let importer_dir = crate::resolver_core::parent_dir(importer_id);
        let base = if crate::resolver_core::is_absolute_specifier(specifier) {
            crate::resolver_core::normalize_canonical_id(specifier)
        } else {
            crate::resolver_core::join_paths(&importer_dir, specifier)
        };
        return then(
            probe_for_ctx(view, expected_basis, &base, ctx),
            AttemptOutput::new(),
            |resolved, output| {
                let value = resolved.map(|r| (r, crate::resolver_core::ResolutionKind::Relative));
                hit(value, output)
            },
        );
    }

    then(
        crate::resolver_core::tsconfig_paths_resolution::resolve_via_workspace_config(
            view,
            expected_basis,
            importer_owner,
            specifier,
            ctx,
        ),
        AttemptOutput::new(),
        move |resolved, output| {
            if resolved.is_some() {
                return hit(resolved, output);
            }

            then(
                crate::resolver_core::project_references_resolution::resolve_project_references(
                    view,
                    expected_basis,
                    projects,
                    importer_owner,
                    specifier,
                    ctx,
                ),
                output,
                move |resolved, output| {
                    if let Some(resolved) = resolved {
                        return hit(
                            Some((
                                resolved,
                                crate::resolver_core::ResolutionKind::ProjectReference,
                            )),
                            output,
                        );
                    }

                    if specifier.starts_with('#') {
                        return then(
                            resolve_package_imports(
                                view,
                                expected_basis,
                                importer_id,
                                specifier,
                                ctx,
                                Some(&importer_owner.workspace_root),
                            ),
                            output,
                            |resolved, output| {
                                let value = resolved.map(|r| {
                                    (r, crate::resolver_core::ResolutionKind::PackageImports)
                                });
                                hit(value, output)
                            },
                        );
                    }

                    then(
                        resolve_node_modules_package(
                            view,
                            expected_basis,
                            importer_id,
                            specifier,
                            ctx,
                            Some(&importer_owner.workspace_root),
                        ),
                        output,
                        hit,
                    )
                },
            )
        },
    )
}

/// Resolve from an explicitly selected project. This has the same shape as
/// [`resolve_source_id`], except relative/absolute
/// bases off `project.root` (there is no separate `importer_id`) and
/// `#imports`/`node_modules` walk from `project.root` rather than an
/// importer file's ancestor chain.
pub(crate) fn resolve_source_id_for_project(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    projects: &[crate::resolver_core::IdeProjectConfig],
    project: &crate::resolver_core::IdeProjectConfig,
    specifier: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<SourceIdHit> {
    if crate::resolver_core::is_relative_specifier(specifier)
        || crate::resolver_core::is_absolute_specifier(specifier)
    {
        let base = if crate::resolver_core::is_absolute_specifier(specifier) {
            crate::resolver_core::normalize_canonical_id(specifier)
        } else {
            crate::resolver_core::join_paths(&project.root, specifier)
        };
        return then(
            probe_for_ctx(view, expected_basis, &base, ctx),
            AttemptOutput::new(),
            |resolved, output| {
                let value = resolved.map(|r| (r, crate::resolver_core::ResolutionKind::Relative));
                hit(value, output)
            },
        );
    }

    then(
        crate::resolver_core::tsconfig_paths_resolution::resolve_via_workspace_config(
            view,
            expected_basis,
            project,
            specifier,
            ctx,
        ),
        AttemptOutput::new(),
        move |resolved, output| {
            if resolved.is_some() {
                return hit(resolved, output);
            }

            then(
                crate::resolver_core::project_references_resolution::resolve_project_references(
                    view,
                    expected_basis,
                    projects,
                    project,
                    specifier,
                    ctx,
                ),
                output,
                move |resolved, output| {
                    if let Some(resolved) = resolved {
                        return hit(
                            Some((
                                resolved,
                                crate::resolver_core::ResolutionKind::ProjectReference,
                            )),
                            output,
                        );
                    }

                    if specifier.starts_with('#') {
                        return then(
                            crate::resolver_core::node_modules_resolution::resolve_package_imports_from_dir(
                                view,
                                expected_basis,
                                &project.root,
                                specifier,
                                ctx,
                                Some(&project.workspace_root),
                            ),
                            output,
                            |resolved, output| {
                                let value = resolved
                                    .map(|r| (r, crate::resolver_core::ResolutionKind::PackageImports));
                                hit(value, output)
                            },
                        );
                    }

                    then(
                        crate::resolver_core::node_modules_resolution::resolve_node_modules_package_from_dir(
                            view,
                            expected_basis,
                            &project.root,
                            specifier,
                            ctx,
                            Some(&project.workspace_root),
                        ),
                        output,
                        hit,
                    )
                },
            )
        },
    )
}

fn package_follow_is_confirmed_frame(
    view: &ResolverAttemptView,
    importer_id: &str,
    resolved: &str,
    frame: &crate::resolver_core::ResolveFrame<'_>,
) -> KernelAttempt<bool> {
    let normalized = frame.memo.normalize(importer_id);
    let marker = "/node_modules/";
    let Some(marker_index) = normalized.rfind(marker) else {
        return AttemptOutcome::Complete(CompletedAttempt::new(true, AttemptOutput::new()));
    };
    let package_start = marker_index + marker.len();
    let mut parts = normalized[package_start..].split('/');
    let Some(first) = parts.next() else {
        return AttemptOutcome::Complete(CompletedAttempt::new(true, AttemptOutput::new()));
    };
    let package_relative = if first.starts_with('@') {
        let Some(second) = parts.next() else {
            return AttemptOutcome::Complete(CompletedAttempt::new(true, AttemptOutput::new()));
        };
        format!("{first}/{second}")
    } else {
        first.to_string()
    };
    let package_dir = format!("{}{marker}{package_relative}", &normalized[..marker_index]);
    then(
        crate::resolver_core::package_target_resolution::read_package_manifest_with_memo(
            view,
            &package_dir,
            &frame.memo,
        ),
        AttemptOutput::new(),
        |manifest, output| {
            let normalized_resolved = frame.memo.normalize(resolved);
            let normalized_package = frame.memo.normalize(&package_dir);
            let confirmed = manifest.is_some()
                && normalized_resolved.starts_with(normalized_package.as_ref())
                && (normalized_resolved.len() == normalized_package.len()
                    || normalized_package.ends_with('/')
                    || normalized_resolved.as_bytes().get(normalized_package.len()) == Some(&b'/'));
            AttemptOutcome::Complete(CompletedAttempt::new(confirmed, output))
        },
    )
}

pub(crate) fn resolve_source_id_frame(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    frame: &crate::resolver_core::ResolveFrame<'_>,
) -> KernelAttempt<SourceIdHit> {
    let geometry = &frame.geometry;
    if geometry.class == crate::resolver_core::resolve_frame::SpecifierClass::RelativeOrAbsolute {
        return then(
            crate::resolver_core::probe_path_resolution::evaluate_probe_candidates(
                view,
                expected_basis,
                &geometry.relative_probe_candidates,
            ),
            AttemptOutput::new(),
            |resolved, output| {
                let Some(resolved) = resolved else {
                    return hit(None, output);
                };
                let unowned_importer = match (&frame.operation, geometry.owner_index) {
                    (crate::resolver_core::resolve_frame::ResolveFrameOperation::Request, None) => {
                        Some(geometry.start.as_ref())
                    }
                    _ => None,
                };
                match unowned_importer {
                    Some(importer_id) => then(
                        package_follow_is_confirmed_frame(view, importer_id, &resolved, frame),
                        output,
                        |confirmed, output| {
                            hit(
                                confirmed.then_some((
                                    resolved,
                                    crate::resolver_core::ResolutionKind::Relative,
                                )),
                                output,
                            )
                        },
                    ),
                    None => hit(
                        Some((resolved, crate::resolver_core::ResolutionKind::Relative)),
                        output,
                    ),
                }
            },
        );
    }

    let Some(owner_index) = geometry.owner_index else {
        return if geometry.class
            == crate::resolver_core::resolve_frame::SpecifierClass::PackageImports
        {
            then(
                crate::resolver_core::node_modules_resolution::resolve_package_imports_frame(
                    view,
                    expected_basis,
                    frame,
                ),
                AttemptOutput::new(),
                |resolved, output| {
                    hit(
                        resolved.map(|resolved| {
                            (
                                resolved,
                                crate::resolver_core::ResolutionKind::PackageImports,
                            )
                        }),
                        output,
                    )
                },
            )
        } else {
            crate::resolver_core::node_modules_resolution::resolve_node_modules_frame(
                view,
                expected_basis,
                frame,
            )
        };
    };
    let owner = &frame.projects[owner_index];
    then(
        crate::resolver_core::tsconfig_paths_resolution::resolve_via_workspace_config_frame(
            view,
            expected_basis,
            frame,
            owner,
        ),
        AttemptOutput::new(),
        |resolved, output| {
            if resolved.is_some() {
                return hit(resolved, output);
            }
            then(
                crate::resolver_core::project_references_resolution::resolve_project_references_frame(
                    view,
                    expected_basis,
                    frame,
                    owner,
                ),
                output,
                |resolved, output| {
                    if let Some(resolved) = resolved {
                        return hit(
                            Some((
                                resolved,
                                crate::resolver_core::ResolutionKind::ProjectReference,
                            )),
                            output,
                        );
                    }
                    if geometry.class
                        == crate::resolver_core::resolve_frame::SpecifierClass::PackageImports
                    {
                        then(
                            crate::resolver_core::node_modules_resolution::resolve_package_imports_frame(
                                view,
                                expected_basis,
                                frame,
                            ),
                            output,
                            |resolved, output| {
                                hit(
                                    resolved.map(|resolved| {
                                        (
                                            resolved,
                                            crate::resolver_core::ResolutionKind::PackageImports,
                                        )
                                    }),
                                    output,
                                )
                            },
                        )
                    } else {
                        then(
                            crate::resolver_core::node_modules_resolution::resolve_node_modules_frame(
                                view,
                                expected_basis,
                                frame,
                            ),
                            output,
                            hit,
                        )
                    }
                },
            )
        },
    )
}

#[cfg(test)]
#[path = "source_id_resolution_tests.rs"]
mod source_id_resolution_tests;
