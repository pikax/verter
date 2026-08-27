//! Package-manifest target resolution over `ResolverAttemptView` and
//! `priority_frontier`.
//!
//! Builds on [`super::probe_path_resolution::probe_path_for_context`] for
//! terminal candidate resolution — every path this module produces
//! bottoms out in that shared primitive. Package `exports` and
//! `main`/`module`/`types`/`typings` resolution needs no observation
//! beyond the three module-resolution primitives (`path_probe`/`real_path`/
//! `package_manifest`).
//!
//! `resolve_package_target`'s own `Array` (first item wins) and `Object`
//! (first matching condition wins) branches are THEMSELVES
//! priority-ordered fallthroughs — each recursive branch is expressed as
//! a nested [`priority_frontier`] call rather than inventing separate
//! control flow, matching `priority_frontier`'s own designed-for-nesting
//! composition (an evaluate closure may itself run a nested frontier).
//!
//! `package.json#browser` is not part of this resolver's supported
//! semantics.

#![allow(dead_code)]

use std::sync::Arc;

use crate::resolver_core::priority_frontier::priority_frontier_with_budgets;
use crate::resolver_core::probe_path_resolution::probe_path_for_context;
use crate::resolver_core::{
    AttemptOutcome, AttemptOutput, CompletedAttempt, ConsumedResolutionObservationKey,
    KernelAttempt, ResolutionBasis, ResolutionPackageManifest, ResolverAttemptView,
    ResolverObservation,
};

/// Mirrors `prefers_declaration_files(ctx)` exactly. `pub(crate)` —
/// reused by `node_modules_resolution`'s own `types`-entry-fallback
/// gate, which needs the identical predicate.
pub(crate) fn prefers_declaration_files(ctx: crate::resolver_core::ResolutionContext) -> bool {
    matches!(
        (ctx.phase, ctx.kind),
        (
            crate::resolver_core::ResolvePhase::CodegenBlocker,
            crate::resolver_core::ResolveRequestKind::TypeImport
        ) | (crate::resolver_core::ResolvePhase::ProviderGraph, _)
    )
}

/// Mirrors `probe_path_for_context`'s own `ctx.kind != SfcSrcAttr` gate.
fn applies_source_sibling(ctx: crate::resolver_core::ResolutionContext) -> bool {
    ctx.kind != crate::resolver_core::ResolveRequestKind::SfcSrcAttr
}

/// Thin `ResolutionContext`-aware wrapper over
/// [`probe_path_for_context`], deriving both its boolean gates from
/// `ctx` exactly as the legacy `probe_path_for_context(reader, base,
/// ctx)` does internally. `pub(crate)` — reused by
/// `node_modules_resolution`'s "no manifest here, probe directly"
/// branch, which is exactly the legacy `probe_path_for_context(reader,
/// &base, ctx)` call inside `resolve_node_modules_package_from_dirs`'s
/// `else` arm.
pub(crate) fn probe_for_ctx(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    base: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<String>> {
    probe_path_for_context(
        view,
        expected_basis,
        base,
        applies_source_sibling(ctx),
        prefers_declaration_files(ctx),
    )
}

pub(crate) fn probe_for_ctx_with_memo(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    base: &str,
    ctx: crate::resolver_core::ResolutionContext,
    memo: &crate::resolver_core::resolve_frame::ResolutionStringMemo,
) -> KernelAttempt<Option<String>> {
    crate::resolver_core::probe_path_resolution::probe_path_with_memo(
        view,
        expected_basis,
        base,
        applies_source_sibling(ctx),
        prefers_declaration_files(ctx),
        memo,
    )
}

fn miss() -> KernelAttempt<Option<String>> {
    AttemptOutcome::Complete(CompletedAttempt::new(None, AttemptOutput::new()))
}

/// Mirrors `package_conditions(ctx)` exactly.
pub(crate) fn package_conditions(
    ctx: crate::resolver_core::ResolutionContext,
) -> &'static [&'static str] {
    match (ctx.phase, ctx.kind) {
        (_, crate::resolver_core::ResolveRequestKind::RequireCall) => &["require", "default"],
        (
            crate::resolver_core::ResolvePhase::CodegenBlocker,
            crate::resolver_core::ResolveRequestKind::EsmImport
            | crate::resolver_core::ResolveRequestKind::SfcSrcAttr,
        ) => &["import", "default"],
        (
            crate::resolver_core::ResolvePhase::CodegenBlocker,
            crate::resolver_core::ResolveRequestKind::TypeImport,
        ) => &["types", "import", "default"],
        (crate::resolver_core::ResolvePhase::ProviderGraph, _) => &["types", "import", "default"],
    }
}

/// Mirrors `resolve_package_path` exactly — pure string logic, no I/O.
pub(crate) fn resolve_package_path(
    package_dir: &str,
    target: &str,
    captured: Option<&str>,
) -> String {
    let replaced = match captured {
        Some(captured) if target.contains('*') => {
            let star = target.find('*').unwrap_or(0);
            format!("{}{}{}", &target[..star], captured, &target[star + 1..])
        }
        _ => target.to_string(),
    };

    if crate::resolver_core::is_absolute_specifier(&replaced) {
        crate::resolver_core::normalize_canonical_id(&replaced)
    } else {
        crate::resolver_core::join_paths(package_dir, &replaced)
    }
}

/// Captures the wildcard segment of a tsconfig path pattern using pure string
/// logic and no I/O. Kept crate-private and reused by
/// `tsconfig_paths_resolution`'s
/// `paths`-pattern matching, which needs the identical algorithm.
pub(crate) fn capture_tsconfig_pattern<'a>(
    pattern: &'a str,
    specifier: &'a str,
) -> Option<&'a str> {
    if let Some(star) = pattern.find('*') {
        let prefix = &pattern[..star];
        let suffix = &pattern[star + 1..];
        if !specifier.starts_with(prefix) || !specifier.ends_with(suffix) {
            return None;
        }
        let captured_end = specifier.len().saturating_sub(suffix.len());
        if prefix.len() > captured_end {
            return None;
        }
        Some(&specifier[prefix.len()..captured_end])
    } else if pattern == specifier {
        Some("")
    } else {
        None
    }
}

/// Mirrors `match_package_mapping` exactly.
pub(crate) fn match_package_mapping<'a>(
    mappings: &'a serde_json::Map<String, serde_json::Value>,
    specifier: &str,
) -> Option<(&'a serde_json::Value, Option<String>)> {
    let mut best: Option<(&serde_json::Value, Option<String>, usize, bool)> = None;
    for (pattern, value) in mappings {
        let Some(captured) = capture_tsconfig_pattern(pattern, specifier) else {
            continue;
        };
        let exact = !pattern.contains('*');
        let score = pattern.replace('*', "").len();
        match &best {
            Some((_, _, best_score, best_exact))
                if *best_score > score || (*best_score == score && *best_exact && !exact) =>
            {
                continue;
            }
            _ => {
                best = Some((
                    value,
                    (!captured.is_empty()).then(|| captured.to_string()),
                    score,
                    exact,
                ));
            }
        }
    }

    best.map(|(value, captured, _, _)| (value, captured))
}

/// Mirrors `resolve_package_target` exactly. The `Array`/`Object`
/// branches are themselves priority-ordered fallthroughs (first array
/// item wins; first matching condition wins) — each is expressed as a
/// nested [`priority_frontier`] call rather than separate control flow.
pub(crate) fn resolve_package_target(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    package_dir: &str,
    value: &serde_json::Value,
    captured: Option<&str>,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<String>> {
    match value {
        serde_json::Value::String(target) => probe_for_ctx(
            view,
            expected_basis,
            &resolve_package_path(package_dir, target, captured),
            ctx,
        ),
        serde_json::Value::Array(items) => priority_frontier_with_budgets(
            expected_basis,
            view.input_resolution_budgets(),
            items.iter(),
            |item| resolve_package_target(view, expected_basis, package_dir, item, captured, ctx),
        ),
        serde_json::Value::Object(map) => {
            let conditions = package_conditions(ctx);
            priority_frontier_with_budgets(
                expected_basis,
                view.input_resolution_budgets(),
                conditions.iter().copied(),
                |condition| match map.get(condition) {
                    Some(entry) => resolve_package_target(
                        view,
                        expected_basis,
                        package_dir,
                        entry,
                        captured,
                        ctx,
                    ),
                    None => miss(),
                },
            )
        }
        _ => miss(),
    }
}

pub(crate) fn resolve_package_target_with_memo(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    package_dir: &str,
    value: &serde_json::Value,
    captured: Option<&str>,
    ctx: crate::resolver_core::ResolutionContext,
    memo: &crate::resolver_core::resolve_frame::ResolutionStringMemo,
) -> KernelAttempt<Option<String>> {
    match value {
        serde_json::Value::String(target) => probe_for_ctx_with_memo(
            view,
            expected_basis,
            &memo.package_path(package_dir, target, captured),
            ctx,
            memo,
        ),
        serde_json::Value::Array(items) => priority_frontier_with_budgets(
            expected_basis,
            view.input_resolution_budgets(),
            items,
            |item| {
                resolve_package_target_with_memo(
                    view,
                    expected_basis,
                    package_dir,
                    item,
                    captured,
                    ctx,
                    memo,
                )
            },
        ),
        serde_json::Value::Object(map) => priority_frontier_with_budgets(
            expected_basis,
            view.input_resolution_budgets(),
            package_conditions(ctx).iter().copied(),
            |condition| match map.get(condition) {
                Some(entry) => resolve_package_target_with_memo(
                    view,
                    expected_basis,
                    package_dir,
                    entry,
                    captured,
                    ctx,
                    memo,
                ),
                None => miss(),
            },
        ),
        _ => miss(),
    }
}

/// Mirrors `resolve_package_exports` exactly.
pub(crate) fn resolve_package_exports(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    package_dir: &str,
    exports: &serde_json::Value,
    export_key: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<String>> {
    match exports {
        serde_json::Value::String(_) | serde_json::Value::Array(_) => {
            if export_key == "." {
                resolve_package_target(view, expected_basis, package_dir, exports, None, ctx)
            } else {
                miss()
            }
        }
        serde_json::Value::Object(map) => {
            if !map.keys().any(|key| key.starts_with('.')) {
                if export_key == "." {
                    return resolve_package_target(
                        view,
                        expected_basis,
                        package_dir,
                        exports,
                        None,
                        ctx,
                    );
                }
                return miss();
            }

            match match_package_mapping(map, export_key) {
                Some((entry, captured)) => resolve_package_target(
                    view,
                    expected_basis,
                    package_dir,
                    entry,
                    captured.as_deref(),
                    ctx,
                ),
                None => miss(),
            }
        }
        _ => miss(),
    }
}

pub(crate) fn resolve_package_exports_with_memo(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    package_dir: &str,
    exports: &serde_json::Value,
    export_key: &str,
    ctx: crate::resolver_core::ResolutionContext,
    memo: &crate::resolver_core::resolve_frame::ResolutionStringMemo,
) -> KernelAttempt<Option<String>> {
    match exports {
        serde_json::Value::String(_) | serde_json::Value::Array(_) => {
            if export_key == "." {
                resolve_package_target_with_memo(
                    view,
                    expected_basis,
                    package_dir,
                    exports,
                    None,
                    ctx,
                    memo,
                )
            } else {
                miss()
            }
        }
        serde_json::Value::Object(map) => {
            if !map.keys().any(|key| key.starts_with('.')) {
                return if export_key == "." {
                    resolve_package_target_with_memo(
                        view,
                        expected_basis,
                        package_dir,
                        exports,
                        None,
                        ctx,
                        memo,
                    )
                } else {
                    miss()
                };
            }
            match match_package_mapping(map, export_key) {
                Some((entry, captured)) => resolve_package_target_with_memo(
                    view,
                    expected_basis,
                    package_dir,
                    entry,
                    captured.as_deref(),
                    ctx,
                    memo,
                ),
                None => miss(),
            }
        }
        _ => miss(),
    }
}

/// Mirrors `resolve_legacy_package` exactly. Legacy tries each
/// `main`/`module`/`types`/`typings` key (per `(phase, kind)`) in order,
/// probing its target and returning on the first hit; only once every
/// key has missed does it fall back to `package_dir/index`. Flattened
/// here into ONE ordered candidate-path list run through one outer
/// `priority_frontier` — each candidate's own evaluation is itself the
/// full `probe_path_for_context` fallthrough, exactly matching the
/// legacy per-key `probe_path_for_context` call.
pub(crate) fn resolve_legacy_package(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    package_dir: &str,
    package_json: &ResolutionPackageManifest,
    subpath: &str,
    ctx: crate::resolver_core::ResolutionContext,
) -> KernelAttempt<Option<String>> {
    if !subpath.is_empty() {
        return probe_for_ctx(
            view,
            expected_basis,
            &crate::resolver_core::join_paths(package_dir, subpath),
            ctx,
        );
    }

    let keys: &[&str] = match (ctx.phase, ctx.kind) {
        (_, crate::resolver_core::ResolveRequestKind::RequireCall) => &["main"],
        (
            crate::resolver_core::ResolvePhase::CodegenBlocker,
            crate::resolver_core::ResolveRequestKind::EsmImport
            | crate::resolver_core::ResolveRequestKind::SfcSrcAttr,
        ) => &["module", "main"],
        _ => &["types", "typings", "main"],
    };

    let mut candidates: Vec<String> = Vec::new();
    for key in keys {
        let target = match *key {
            "main" => package_json.main.as_deref(),
            "module" => package_json.module.as_deref(),
            "types" => package_json.types.as_deref(),
            "typings" => package_json.typings.as_deref(),
            _ => None,
        };
        if let Some(target) = target {
            candidates.push(resolve_package_path(package_dir, target, None));
        }
    }
    candidates.push(crate::resolver_core::join_paths(package_dir, "index"));

    priority_frontier_with_budgets(
        expected_basis,
        view.input_resolution_budgets(),
        candidates,
        |candidate| probe_for_ctx(view, expected_basis, &candidate, ctx),
    )
}

pub(crate) fn resolve_legacy_package_with_memo(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    package_dir: &str,
    package_json: &ResolutionPackageManifest,
    subpath: &str,
    ctx: crate::resolver_core::ResolutionContext,
    memo: &crate::resolver_core::resolve_frame::ResolutionStringMemo,
) -> KernelAttempt<Option<String>> {
    if !subpath.is_empty() {
        return probe_for_ctx_with_memo(
            view,
            expected_basis,
            &memo.join(package_dir, subpath),
            ctx,
            memo,
        );
    }

    let keys: &[&str] = match (ctx.phase, ctx.kind) {
        (_, crate::resolver_core::ResolveRequestKind::RequireCall) => &["main"],
        (
            crate::resolver_core::ResolvePhase::CodegenBlocker,
            crate::resolver_core::ResolveRequestKind::EsmImport
            | crate::resolver_core::ResolveRequestKind::SfcSrcAttr,
        ) => &["module", "main"],
        _ => &["types", "typings", "main"],
    };
    let mut candidates = Vec::new();
    for key in keys {
        let target = match *key {
            "main" => package_json.main.as_deref(),
            "module" => package_json.module.as_deref(),
            "types" => package_json.types.as_deref(),
            "typings" => package_json.typings.as_deref(),
            _ => None,
        };
        if let Some(target) = target {
            candidates.push(memo.package_path(package_dir, target, None));
        }
    }
    candidates.push(memo.join(package_dir, "index"));
    priority_frontier_with_budgets(
        expected_basis,
        view.input_resolution_budgets(),
        candidates,
        |candidate| probe_for_ctx_with_memo(view, expected_basis, &candidate, ctx, memo),
    )
}

/// Mirrors `resolve_manifest_types_entry` exactly. Legacy probes via the
/// bare, context-free `probe_path` — equivalent to
/// `probe_path_for_context` with both boolean gates false (no
/// source-sibling substitution, no declaration-companion preference),
/// since `types`/`typings` targets are already declaration paths.
pub(crate) fn resolve_manifest_types_entry(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    package_dir: &str,
    package_json: &ResolutionPackageManifest,
) -> KernelAttempt<Option<String>> {
    let candidates: Vec<String> = [
        package_json.types.as_deref(),
        package_json.typings.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|target| resolve_package_path(package_dir, target, None))
    .collect();

    priority_frontier_with_budgets(
        expected_basis,
        view.input_resolution_budgets(),
        candidates,
        |candidate| probe_path_for_context(view, expected_basis, &candidate, false, false),
    )
}

pub(crate) fn resolve_manifest_types_entry_with_memo(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    package_dir: &str,
    package_json: &ResolutionPackageManifest,
    memo: &crate::resolver_core::resolve_frame::ResolutionStringMemo,
) -> KernelAttempt<Option<String>> {
    let candidates = [
        package_json.types.as_deref(),
        package_json.typings.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|target| memo.package_path(package_dir, target, None))
    .collect::<Vec<_>>();
    priority_frontier_with_budgets(
        expected_basis,
        view.input_resolution_budgets(),
        candidates,
        |candidate| {
            crate::resolver_core::probe_path_resolution::probe_path_with_memo(
                view,
                expected_basis,
                &candidate,
                false,
                false,
                memo,
            )
        },
    )
}

/// Probe for `package.json`, then request its parsed manifest only for a file
/// hit. Keeping these observations separate preserves the resolver's existing
/// no-read behavior for absent ancestor manifests while still allowing the
/// workspace driver to batch snapshot inputs.
pub(crate) fn read_package_manifest_if_present(
    view: &ResolverAttemptView,
    package_dir: &str,
) -> KernelAttempt<Option<Arc<ResolutionPackageManifest>>> {
    let manifest_path = crate::resolver_core::join_paths(package_dir, "package.json");
    match view.path_probe(&manifest_path) {
        AttemptOutcome::NeedInputs(load_set) => AttemptOutcome::NeedInputs(load_set),
        AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
        AttemptOutcome::Complete(crate::resolver_core::PathProbe::File) => {
            let manifest = match view.package_manifest(package_dir) {
                AttemptOutcome::NeedInputs(load_set) => {
                    return AttemptOutcome::NeedInputs(load_set);
                }
                AttemptOutcome::Terminal(failure) => {
                    return AttemptOutcome::Terminal(failure);
                }
                AttemptOutcome::Complete(manifest) => manifest,
            };
            let mut output = AttemptOutput::new();
            let recorded = output
                .record_consumed_resolution_observation(
                    ConsumedResolutionObservationKey::PathProbe {
                        path: Arc::from(manifest_path.as_str()),
                    },
                )
                .and_then(|()| {
                    crate::resolver_core::probe_path_resolution::record_recovery_scopes(
                        &mut output,
                        &manifest_path,
                    )
                })
                .and_then(|()| {
                    output.record_consumed_resolution_observation(
                        ConsumedResolutionObservationKey::PackageManifest {
                            directory: Arc::from(package_dir),
                        },
                    )
                });
            if let Err(failure) = recorded {
                return AttemptOutcome::Terminal(failure);
            }
            AttemptOutcome::Complete(CompletedAttempt::new(manifest, output))
        }
        AttemptOutcome::Complete(_) => {
            let mut output = AttemptOutput::new();
            let recorded = output
                .record_consumed_resolution_observation(
                    ConsumedResolutionObservationKey::PathProbe {
                        path: Arc::from(manifest_path.as_str()),
                    },
                )
                .and_then(|()| {
                    crate::resolver_core::probe_path_resolution::record_recovery_scopes(
                        &mut output,
                        &manifest_path,
                    )
                });
            if let Err(failure) = recorded {
                return AttemptOutcome::Terminal(failure);
            }
            AttemptOutcome::Complete(CompletedAttempt::new(None, output))
        }
    }
}

pub(crate) fn read_package_manifest_at(
    view: &ResolverAttemptView,
    package_dir: &str,
    manifest_path: &str,
) -> KernelAttempt<Option<Arc<ResolutionPackageManifest>>> {
    match view.path_probe(manifest_path) {
        AttemptOutcome::NeedInputs(load_set) => AttemptOutcome::NeedInputs(load_set),
        AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
        AttemptOutcome::Complete(crate::resolver_core::PathProbe::File) => {
            let manifest = match view.package_manifest(package_dir) {
                AttemptOutcome::NeedInputs(load_set) => {
                    return AttemptOutcome::NeedInputs(load_set)
                }
                AttemptOutcome::Terminal(failure) => return AttemptOutcome::Terminal(failure),
                AttemptOutcome::Complete(manifest) => manifest,
            };
            let mut output = AttemptOutput::new();
            let recorded = output
                .record_consumed_resolution_observation(
                    ConsumedResolutionObservationKey::PathProbe {
                        path: Arc::from(manifest_path),
                    },
                )
                .and_then(|()| {
                    crate::resolver_core::probe_path_resolution::record_recovery_scopes(
                        &mut output,
                        manifest_path,
                    )
                })
                .and_then(|()| {
                    output.record_consumed_resolution_observation(
                        ConsumedResolutionObservationKey::PackageManifest {
                            directory: Arc::from(package_dir),
                        },
                    )
                });
            if let Err(failure) = recorded {
                return AttemptOutcome::Terminal(failure);
            }
            AttemptOutcome::Complete(CompletedAttempt::new(manifest, output))
        }
        AttemptOutcome::Complete(_) => {
            let mut output = AttemptOutput::new();
            let recorded = output
                .record_consumed_resolution_observation(
                    ConsumedResolutionObservationKey::PathProbe {
                        path: Arc::from(manifest_path),
                    },
                )
                .and_then(|()| {
                    crate::resolver_core::probe_path_resolution::record_recovery_scopes(
                        &mut output,
                        manifest_path,
                    )
                });
            if let Err(failure) = recorded {
                return AttemptOutcome::Terminal(failure);
            }
            AttemptOutcome::Complete(CompletedAttempt::new(None, output))
        }
    }
}

pub(crate) fn read_package_manifest_with_memo(
    view: &ResolverAttemptView,
    package_dir: &str,
    memo: &crate::resolver_core::resolve_frame::ResolutionStringMemo,
) -> KernelAttempt<Option<Arc<ResolutionPackageManifest>>> {
    let manifest_path = memo.join(package_dir, "package.json");
    read_package_manifest_at(view, package_dir, &manifest_path)
}

#[cfg(test)]
#[path = "package_target_resolution_tests.rs"]
mod package_target_resolution_tests;
