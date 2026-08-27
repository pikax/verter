//! Candidate probing over `ResolverAttemptView` and `priority_frontier`.
//!
//! The shared candidate generation and frontier composition lets other
//! algorithm pieces
//! (package `exports`/legacy resolution, tsconfig `paths`, workspace
//! aliases, project references — all of which terminate in a
//! `probe_path_for_context`-shaped candidate probe) can reuse it, instead
//! of re-deriving the same candidate list per caller.
//!
//! Faithful because `probe_path_for_context`'s own nested short-circuit
//! structure (JS-family source-sibling, then declaration-companion, then
//! bare `probe_path`'s extension/index scan) is ALREADY a priority-
//! ordered "try candidates in sequence, first hit wins" chain — exactly
//! `priority_frontier`'s own model.

// Some helpers serve only narrower resolver configurations, so the module
// permits dead-code diagnostics while retaining one candidate-probing owner.
#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

use crate::resolver_core::priority_frontier::priority_frontier_with_budgets;
use crate::resolver_core::{
    AttemptOutcome, AttemptOutput, CompletedAttempt, ConsumedResolutionObservationKey,
    KernelAttempt, ResolutionBasis, ResolverAttemptView, ResolverObservation,
};

/// Mirrors `resolve_ts_source_sibling`'s own match arms exactly
/// (`crates/verter_workspace/src/resolver.rs`).
pub(crate) fn js_family_source_exts(base: &str) -> Option<(&'static str, &'static [&'static str])> {
    if base.ends_with(".mjs") {
        Some((".mjs", &[".mts"]))
    } else if base.ends_with(".cjs") {
        Some((".cjs", &[".cts"]))
    } else if base.ends_with(".jsx") {
        Some((".jsx", &[".tsx"]))
    } else if base.ends_with(".js") {
        Some((".js", &[".ts", ".tsx"]))
    } else {
        None
    }
}

/// Mirrors `resolve_declaration_companion`'s own match arms exactly.
pub(crate) fn js_family_companion_exts(
    base: &str,
) -> Option<(&'static str, &'static [&'static str])> {
    if base.ends_with(".mjs") {
        Some((".mjs", &[".d.mts", ".d.ts"]))
    } else if base.ends_with(".cjs") {
        Some((".cjs", &[".d.cts", ".d.ts"]))
    } else if base.ends_with(".jsx") {
        Some((".jsx", &[".d.ts"]))
    } else if base.ends_with(".js") {
        Some((".js", &[".d.ts"]))
    } else {
        None
    }
}
/// Mirrors `probe_extensions()` exactly.
///
/// The script and declaration extensions are this resolver's own; the
/// CARRIER extensions are not, and are read from the language registry
/// rather than written here. A carrier extension spelled by hand is a
/// second place the set is defined, and it drifts the moment a carrier
/// is added or removed.
pub(crate) fn probe_extensions_list() -> &'static [&'static str] {
    static CELL: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    CELL.get_or_init(|| {
        const SCRIPT: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs", ".cts", ".cjs"];
        const DECLARATION: &[&str] = &[".d.ts", ".d.mts", ".d.cts"];
        let mut out: Vec<&'static str> = SCRIPT.to_vec();
        for extension in verter_language::LanguageRegistry::global().carrier_extensions() {
            out.push(Box::leak(format!(".{extension}").into_boxed_str()));
        }
        out.extend_from_slice(DECLARATION);
        out
    })
}

/// Mirrors `probe_index_files()` exactly.
pub(crate) const PROBE_INDEX_FILES: &[&str] = &[
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "index.mts",
    "index.mjs",
    "index.cts",
    "index.cjs",
    "index.vue",
    "index.d.ts",
    "index.d.mts",
    "index.d.cts",
];

/// The ONE ordered candidate list a `probe_path_for_context` call
/// produces, flattened — see module docs for why this is faithful.
///
/// `apply_source_sibling` mirrors `probe_path_for_context`'s own
/// `ctx.kind != ResolveRequestKind::SfcSrcAttr` gate: an SFC `src=`
/// attribute reads the literal file bytes named by the specifier, so
/// substituting a JS-family source sibling (`./x.js` -> `./x.ts`) would
/// change which external file is consumed — the substitution is a
/// TypeScript-IMPORT-resolution rule only, never applied for that
/// request kind.
pub(crate) fn build_probe_candidate_list(
    base: &str,
    apply_source_sibling: bool,
    prefers_declarations: bool,
) -> Vec<String> {
    let mut out = Vec::new();

    if apply_source_sibling {
        if let Some((runtime_ext, source_exts)) = js_family_source_exts(base) {
            if let Some(stem) = base.strip_suffix(runtime_ext) {
                for ext in source_exts {
                    out.push(format!("{stem}{ext}"));
                }
            }
        }
    }

    if prefers_declarations {
        if let Some((runtime_ext, companion_exts)) = js_family_companion_exts(base) {
            if let Some(stem) = base.strip_suffix(runtime_ext) {
                for ext in companion_exts {
                    out.push(format!("{stem}{ext}"));
                }
            }
        }
    }

    let has_extension = Path::new(base).extension().is_some();
    if has_extension {
        out.push(base.to_string());
    } else {
        for ext in probe_extensions_list() {
            out.push(format!("{base}{ext}"));
        }
    }

    for index_name in PROBE_INDEX_FILES {
        out.push(format!("{}/{}", base.trim_end_matches('/'), index_name));
    }

    out
}

/// Ancestor-directory chain of `path` — same algorithm as the legacy
/// witness fixture's own `ancestor_scopes`
/// (`verter_workspace::resolution_witness_contract_tests`).
pub(crate) fn ancestor_scopes(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = path;
    while let Some(index) = current.rfind('/') {
        let prefix = if index == 0 { "/" } else { &current[..index] };
        out.push(prefix.to_string());
        if prefix == "/" {
            break;
        }
        current = prefix;
    }
    out
}

pub(crate) fn record_recovery_scopes(
    output: &mut AttemptOutput,
    path: &str,
) -> Result<(), crate::resolver_core::AttemptFailure> {
    for prefix in ancestor_scopes(path) {
        output.record_consumed_resolution_observation(
            ConsumedResolutionObservationKey::RecoveryScope {
                canonical_prefix: Arc::from(prefix),
            },
        )?;
    }
    Ok(())
}

/// Mirrors `resolve_existing_path`'s own probe-then-realpath-on-hit
/// structure exactly: probe the candidate; a stable `Absent`/
/// `Inaccessible`/`Unknown` classification is a genuine miss (`Complete(None)`,
/// with the probe recorded as consumed); a `File`/`Directory` hit follows
/// up with `real_path` and records both the probe and the realpath as
/// consumed, plus recovery-scope ancestors for both the requested and
/// resolved paths.
pub(crate) fn evaluate_probe_candidate(
    view: &ResolverAttemptView,
    candidate: &str,
) -> KernelAttempt<Option<String>> {
    match view.path_probe(candidate) {
        AttemptOutcome::NeedInputs(load_set) => AttemptOutcome::NeedInputs(load_set),
        AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
        AttemptOutcome::Complete(probe) => match probe {
            crate::resolver_core::PathProbe::File | crate::resolver_core::PathProbe::Directory => {
                match view.real_path(candidate) {
                    AttemptOutcome::NeedInputs(load_set) => AttemptOutcome::NeedInputs(load_set),
                    AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
                    AttemptOutcome::Complete(resolved) => {
                        let resolved_id = resolved
                            .as_ref()
                            .map(|r| r.to_string())
                            .unwrap_or_else(|| candidate.to_string());
                        let mut output = AttemptOutput::new();
                        if let Err(failure) = output.record_consumed_resolution_observation(
                            ConsumedResolutionObservationKey::PathProbe {
                                path: Arc::from(candidate),
                            },
                        ) {
                            return AttemptOutcome::Terminal(failure);
                        }
                        if let Err(failure) = output.record_consumed_resolution_observation(
                            ConsumedResolutionObservationKey::RealPath {
                                path: Arc::from(candidate),
                            },
                        ) {
                            return AttemptOutcome::Terminal(failure);
                        }
                        if let Err(failure) = record_recovery_scopes(&mut output, candidate)
                            .and_then(|()| record_recovery_scopes(&mut output, &resolved_id))
                        {
                            return AttemptOutcome::Terminal(failure);
                        }
                        AttemptOutcome::Complete(CompletedAttempt::new(Some(resolved_id), output))
                    }
                }
            }
            _ => {
                let mut output = AttemptOutput::new();
                if let Err(failure) = output.record_consumed_resolution_observation(
                    ConsumedResolutionObservationKey::PathProbe {
                        path: Arc::from(candidate),
                    },
                ) {
                    return AttemptOutcome::Terminal(failure);
                }
                if let Err(failure) = record_recovery_scopes(&mut output, candidate) {
                    return AttemptOutcome::Terminal(failure);
                }
                AttemptOutcome::Complete(CompletedAttempt::new(None, output))
            }
        },
    }
}

/// Evaluates the full path-probe candidate sequence in one attempt (no
/// internal retry loop — that is the
/// top-level driver's job) over `base`'s full candidate list
/// (JS-family source-sibling, declaration-companion if
/// `prefers_declarations`, then the bare extension/index scan), run
/// through the real `priority_frontier`.
pub(crate) fn probe_path_for_context(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    base: &str,
    apply_source_sibling: bool,
    prefers_declarations: bool,
) -> KernelAttempt<Option<String>> {
    let candidates = build_probe_candidate_list(base, apply_source_sibling, prefers_declarations);
    priority_frontier_with_budgets(
        expected_basis,
        view.input_resolution_budgets(),
        candidates,
        |candidate| evaluate_probe_candidate(view, &candidate),
    )
}

pub(crate) fn evaluate_probe_candidates(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    candidates: &[Arc<str>],
) -> KernelAttempt<Option<String>> {
    priority_frontier_with_budgets(
        expected_basis,
        view.input_resolution_budgets(),
        candidates,
        |candidate| evaluate_probe_candidate(view, candidate),
    )
}

pub(crate) fn probe_path_with_memo(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    base: &str,
    apply_source_sibling: bool,
    prefers_declarations: bool,
    memo: &crate::resolver_core::resolve_frame::ResolutionStringMemo,
) -> KernelAttempt<Option<String>> {
    let normalized = memo.normalize(base);
    let candidates = memo.probe_candidates(&normalized, apply_source_sibling, prefers_declarations);
    evaluate_probe_candidates(view, expected_basis, &candidates)
}

#[cfg(test)]
#[path = "probe_path_resolution_tests.rs"]
mod probe_path_resolution_tests;
