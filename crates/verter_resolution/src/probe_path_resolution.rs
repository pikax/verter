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

use crate::input_resolution_budgets::CompletedWitnessRetentionKey;
use crate::priority_frontier::{CandidateOutput, PriorityFrontierState, RecordedCandidate};
use crate::resolver_attempt_view::SnapshotRead;
use crate::{AttemptOutput, InputResolutionRetention, KernelAttempt, ResolverAttemptView};
use verter_session_query::resolution::{
    ancestor_scope_prefixes, ConsumedResolutionObservationKey, InputKey, ResolutionBasis,
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

/// A JS-family suffix and the extensions that substitute for it.
type ExtensionSubstitution = fn(&str) -> Option<(&'static str, &'static [&'static str])>;

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
///
/// Each candidate is spelled once, in a reused buffer, straight into the
/// shared string its probe key holds, and the list is collected from an
/// iterator of known length into its final shared slice in one allocation.
pub(crate) fn build_probe_candidate_list(
    base: &str,
    apply_source_sibling: bool,
    prefers_declarations: bool,
) -> Arc<[Arc<str>]> {
    let spelling = std::cell::RefCell::new(String::with_capacity(base.len() + 16));
    let spell = |parts: [&str; 3]| -> Arc<str> {
        let mut spelling = spelling.borrow_mut();
        spelling.clear();
        for part in parts {
            spelling.push_str(part);
        }
        Arc::from(spelling.as_str())
    };
    let substitution = |enabled: bool, exts: ExtensionSubstitution| {
        enabled
            .then(|| exts(base))
            .flatten()
            .and_then(|(runtime_ext, exts)| base.strip_suffix(runtime_ext).map(|stem| (stem, exts)))
            .unwrap_or(("", &[]))
    };
    let (source_stem, source_exts) = substitution(apply_source_sibling, js_family_source_exts);
    let (companion_stem, companion_exts) =
        substitution(prefers_declarations, js_family_companion_exts);

    let has_extension = Path::new(base).extension().is_some();
    // A dotted import stem is not necessarily a complete source filename:
    // `./types.d` and `./Button.types` still need `.ts` probing. Keep explicit
    // script/carrier suffixes and literal SFC src attributes unchanged.
    let extensions = probe_extensions_list();
    let dotted_import_stem =
        apply_source_sibling && !extensions.iter().any(|extension| base.ends_with(extension));
    let scanned_extensions = if !has_extension || dotted_import_stem {
        extensions
    } else {
        &[]
    };
    let directory = base.trim_end_matches('/');

    source_exts
        .iter()
        .map(|ext| spell([source_stem, ext, ""]))
        .chain(
            companion_exts
                .iter()
                .map(|ext| spell([companion_stem, ext, ""])),
        )
        .chain(
            has_extension
                .then_some(base)
                .into_iter()
                .map(|base| spell([base, "", ""])),
        )
        .chain(scanned_extensions.iter().map(|ext| spell([base, ext, ""])))
        .chain(
            PROBE_INDEX_FILES
                .iter()
                .map(|index| spell([directory, "/", index])),
        )
        .collect()
}

/// Ancestor-directory chain of `path` — the shared
/// [`verter_session_query::resolution::ancestor_scope_prefixes`] walk, the
/// same one the legacy witness fixture's `ancestor_scopes`
/// (`verter_workspace::resolution_witness_contract_tests`) spells.
pub(crate) fn ancestor_scopes(path: &str) -> Vec<String> {
    ancestor_scope_prefixes(path).map(str::to_owned).collect()
}

/// The recovery-scope witnesses of `path`, nearest ancestor first: exactly
/// [`ancestor_scopes`], as the keys an output records.
///
/// Every path in one directory has the same chain, and the candidates of an
/// import — and the imports of a barrel — share their directory, so the
/// chain is built once and shared instead of being spelled again for every
/// candidate. The chain is a pure function of the spelling; each thread keeps
/// the chains of the directories it used most recently.
pub(crate) fn recovery_scope_chain(path: &str) -> Arc<[ConsumedResolutionObservationKey]> {
    let Some(directory) = ancestor_scope_prefixes(path).next() else {
        return Arc::from([]);
    };
    SCOPE_CHAINS.with(|chains| {
        let mut chains = chains.borrow_mut();
        if let Some(chain) = chains.get(directory) {
            return chain;
        }
        #[cfg(test)]
        SCOPE_CHAIN_BUILDS.set(SCOPE_CHAIN_BUILDS.get() + 1);
        let chain: Arc<[ConsumedResolutionObservationKey]> = ancestor_scope_prefixes(path)
            .map(|prefix| ConsumedResolutionObservationKey::RecoveryScope {
                canonical_prefix: Arc::from(prefix),
            })
            .collect();
        chains.insert(&chain);
        chain
    })
}

/// The chains of the [`SCOPE_CHAIN_SLOTS`] directories used most recently,
/// most recent first; a lookup moves its chain to the front and a new chain
/// replaces the least recent. Each slot is keyed by its chain's first scope,
/// which is the directory itself.
#[derive(Default)]
struct ScopeChainMemo {
    slots: [Option<Arc<[ConsumedResolutionObservationKey]>>; SCOPE_CHAIN_SLOTS],
}

/// Enough for a frontier's own directories (the base's, its index
/// directory's, and the real paths it follows) across the imports of
/// several directories at once.
const SCOPE_CHAIN_SLOTS: usize = 16;

impl ScopeChainMemo {
    fn get(&mut self, directory: &str) -> Option<Arc<[ConsumedResolutionObservationKey]>> {
        let index = self.slots.iter().position(|slot| {
            matches!(
                slot.as_deref().and_then(<[_]>::first),
                Some(ConsumedResolutionObservationKey::RecoveryScope { canonical_prefix })
                    if canonical_prefix.as_ref() == directory
            )
        })?;
        self.slots[..=index].rotate_right(1);
        self.slots[0].clone()
    }

    fn insert(&mut self, chain: &Arc<[ConsumedResolutionObservationKey]>) {
        self.slots.rotate_right(1);
        self.slots[0] = Some(Arc::clone(chain));
    }
}

thread_local! {
    static SCOPE_CHAINS: std::cell::RefCell<ScopeChainMemo> =
        std::cell::RefCell::new(ScopeChainMemo::default());
}

#[cfg(test)]
thread_local! {
    /// Recovery-scope chains built (not reused) on this thread.
    pub(crate) static SCOPE_CHAIN_BUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(crate) fn record_recovery_scopes(
    output: &mut AttemptOutput,
    path: &str,
) -> Result<(), verter_session_query::resolution::AttemptFailure> {
    record_scope_chain(output, &recovery_scope_chain(path))
}

fn record_scope_chain(
    output: &mut AttemptOutput,
    chain: &[ConsumedResolutionObservationKey],
) -> Result<(), verter_session_query::resolution::AttemptFailure> {
    for key in chain {
        output.record_consumed_resolution_observation(key.clone())?;
    }
    Ok(())
}

/// The recovery-scope chains one frontier has already recorded whole into
/// its output. Recording one again changes nothing, so it is skipped. Only
/// the first [`RECORDED_CHAIN_SLOTS`] are remembered; a chain past them is
/// recorded again, which is still exact, only slower.
#[derive(Default)]
struct RecordedChains {
    slots: [Option<Arc<[ConsumedResolutionObservationKey]>>; RECORDED_CHAIN_SLOTS],
}

const RECORDED_CHAIN_SLOTS: usize = 8;

impl RecordedChains {
    fn contains(&self, chain: &Arc<[ConsumedResolutionObservationKey]>) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|recorded| Arc::ptr_eq(recorded, chain))
    }

    fn remember(&mut self, chain: Arc<[ConsumedResolutionObservationKey]>) {
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(chain);
        }
    }
}

/// Mirrors `resolve_existing_path`'s own probe-then-realpath-on-hit
/// structure exactly: probe the candidate; a stable `Absent`/
/// `Inaccessible`/`Unknown` classification is a genuine miss (`Complete(None)`,
/// with the probe recorded as consumed); a `File`/`Directory` hit follows
/// up with `real_path` and records both the probe and the realpath as
/// consumed, plus recovery-scope ancestors for both the requested and
/// resolved paths.
///
/// The output goes where the frontier says ([`CandidateOutput`]): straight
/// into the frontier's own output while no block is held, and nowhere once
/// one is — then a miss only checks that its witnesses could have been
/// retained, and a hit, which cannot win, is not even spelled. Every
/// observation is read before anything is recorded, so a candidate that
/// needs an input has written nothing.
fn evaluate_probe_candidate(
    view: &ResolverAttemptView,
    candidate: &Arc<str>,
    output: CandidateOutput<'_>,
    recorded: &mut RecordedChains,
) -> RecordedCandidate<String> {
    let probe = match view.read_path_probe(candidate) {
        SnapshotRead::Known(probe) => probe,
        SnapshotRead::Missing(basis) => {
            return missing(|path| InputKey::PathProbe { path }, candidate, *basis)
        }
        SnapshotRead::Unavailable(observation) => return unavailable(observation),
    };
    match probe {
        verter_session_query::resolution::PathProbe::File
        | verter_session_query::resolution::PathProbe::Directory => {
            let resolved = match view.read_real_path(candidate) {
                SnapshotRead::Known(resolved) => resolved,
                SnapshotRead::Missing(basis) => {
                    return missing(|path| InputKey::RealPath { path }, candidate, *basis)
                }
                SnapshotRead::Unavailable(observation) => return unavailable(observation),
            };
            let CandidateOutput::Accumulate(mut accumulator) = output else {
                // Rule 5: a hit after a block cannot win, so its answer is
                // never spelled.
                return RecordedCandidate::DiscardedHit;
            };
            let resolved_id = resolved
                .as_ref()
                .map(|r| r.to_string())
                .unwrap_or_else(|| candidate.to_string());
            let output = accumulator.output();
            let recorded = output
                .record_consumed_resolution_observation(
                    ConsumedResolutionObservationKey::PathProbe {
                        path: Arc::clone(candidate),
                    },
                )
                .and_then(|()| {
                    output.record_consumed_resolution_observation(
                        ConsumedResolutionObservationKey::RealPath {
                            path: Arc::clone(candidate),
                        },
                    )
                })
                .and_then(|()| record_candidate_chain(output, candidate, recorded))
                .and_then(|()| record_candidate_chain(output, &resolved_id, recorded));
            match recorded {
                Ok(()) => RecordedCandidate::Hit(resolved_id),
                Err(failure) => RecordedCandidate::Terminal(failure),
            }
        }
        _ => {
            let recorded = match output {
                CandidateOutput::Accumulate(mut accumulator) => {
                    let output = accumulator.output();
                    output
                        .record_consumed_resolution_observation(
                            ConsumedResolutionObservationKey::PathProbe {
                                path: Arc::clone(candidate),
                            },
                        )
                        .and_then(|()| record_candidate_chain(output, candidate, recorded))
                }
                // The probe witness and the candidate's ancestors, which are
                // distinct keys.
                CandidateOutput::Discard => {
                    InputResolutionRetention::current_admits_fresh_completed_witnesses(
                        1 + ancestor_scope_prefixes(candidate).count(),
                        || {
                            let chain = recovery_scope_chain(candidate);
                            std::iter::once(ConsumedResolutionObservationKey::PathProbe {
                                path: Arc::clone(candidate),
                            })
                            .chain(chain.iter().cloned())
                            .map(CompletedWitnessRetentionKey::ConsumedResolutionObservation)
                            .collect()
                        },
                    )
                }
            };
            match recorded {
                Ok(()) => RecordedCandidate::Miss,
                Err(failure) => RecordedCandidate::Terminal(failure),
            }
        }
    }
}

fn missing<T>(
    key: fn(Arc<str>) -> InputKey,
    candidate: &Arc<str>,
    basis: ResolutionBasis,
) -> RecordedCandidate<T> {
    RecordedCandidate::NeedInput(key(Arc::clone(candidate)), basis)
}

fn unavailable<T>(
    observation: verter_session_query::resolution::ResolverObservationKind,
) -> RecordedCandidate<T> {
    RecordedCandidate::Terminal(
        verter_session_query::resolution::AttemptFailure::ObservationUnavailable { observation },
    )
}

/// Records `path`'s recovery scopes into the frontier's output, unless this
/// frontier already recorded that chain whole.
fn record_candidate_chain(
    output: &mut AttemptOutput,
    path: &str,
    recorded: &mut RecordedChains,
) -> Result<(), verter_session_query::resolution::AttemptFailure> {
    let chain = recovery_scope_chain(path);
    if recorded.contains(&chain) {
        return Ok(());
    }
    record_scope_chain(output, &chain)?;
    recorded.remember(chain);
    Ok(())
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
    evaluate_probe_candidates(view, expected_basis, &candidates)
}

/// Runs `candidates` through one priority frontier (see
/// [`crate::priority_frontier`] for the rules), each candidate recording
/// its witnesses where the frontier says.
pub(crate) fn evaluate_probe_candidates(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    candidates: &[Arc<str>],
) -> KernelAttempt<Option<String>> {
    let mut frontier =
        PriorityFrontierState::new_with_budgets(expected_basis, view.input_resolution_budgets());
    let mut recorded = RecordedChains::default();
    for candidate in candidates {
        let step =
            evaluate_probe_candidate(view, candidate, frontier.candidate_output(), &mut recorded);
        if let Some(outcome) = frontier.push_recorded(step) {
            return outcome;
        }
    }
    frontier.finish()
}

pub(crate) fn probe_path_with_memo(
    view: &ResolverAttemptView,
    expected_basis: ResolutionBasis,
    base: &str,
    apply_source_sibling: bool,
    prefers_declarations: bool,
    memo: &crate::resolve_frame::ResolutionStringMemo,
) -> KernelAttempt<Option<String>> {
    let normalized = memo.normalize(base);
    let candidates = memo.probe_candidates(&normalized, apply_source_sibling, prefers_declarations);
    evaluate_probe_candidates(view, expected_basis, &candidates)
}

#[cfg(test)]
#[path = "probe_path_resolution_tests.rs"]
mod probe_path_resolution_tests;
