use std::sync::Arc;

use super::probe_path_for_context;
use crate::{CompletedAttempt, ResolverAttemptView};
use verter_session_query::resolution::{
    AttemptOutcome, ConsumedResolutionObservationKey, ResolutionBasis,
    ResolutionObservationSnapshot, ResolutionWorldBasis,
};

#[test]
fn dotted_import_stems_resolve_script_and_declaration_files() {
    for (stem, target) in [
        ("/p/types.d", "/p/types.d.ts"),
        ("/p/Button.types", "/p/Button.types.ts"),
        ("/p/view.component", "/p/view.component.tsx"),
    ] {
        let view = known_world_view(&[(target, "")], &[(target, target)]);
        assert!(
            matches!(
                probe_path_for_context(&view, basis(), stem, true, true),
                AttemptOutcome::Complete(CompletedAttempt { value: Some(ref v), .. }) if v == target
            ),
            "import stem {stem} must resolve {target}"
        );
    }
}

#[test]
fn literal_sfc_src_does_not_append_extensions_to_a_dotted_filename() {
    let view = known_world_view(&[("/p/script.custom.ts", "")], &[]);
    assert!(matches!(
        probe_path_for_context(&view, basis(), "/p/script.custom", false, false),
        AttemptOutcome::Complete(CompletedAttempt { value: None, .. })
    ));
}

fn basis() -> ResolutionBasis {
    ResolutionBasis::new(
        ResolutionWorldBasis::new(
            verter_session_query::resolution::WorkspaceAuthorityId::test_only(1),
            verter_session_query::resolution::ResolutionPopulation::Base,
            verter_session_query::resolution::ResolutionWorldId::test_only(1),
            None,
        ),
        None,
    )
}

/// A FULLY-KNOWN-WORLD test view: every path not explicitly listed as
/// present answers the stable `Absent` fact directly (never `NeedInputs`)
/// — this file's tests exercise `probe_path_for_context`'s candidate
/// generation and fallthrough logic in isolation, not the retry-loop
/// driver (that is `resolution_dual_runner_tests.rs`'s job).
fn known_world_view(files: &[(&str, &str)], realpaths: &[(&str, &str)]) -> ResolverAttemptView {
    let mut snapshot = ResolutionObservationSnapshot::with_stable_absent_defaults_for_test();
    for (path, _) in files {
        snapshot.insert_path_probe(
            (*path).to_string(),
            verter_session_query::resolution::PathProbe::File,
        );
    }
    for (path, realpath) in realpaths {
        snapshot.insert_real_path((*path).to_string(), Some(Arc::from(*realpath)));
    }
    ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis())
}

#[test]
fn resolves_via_the_ts_source_sibling_before_the_bare_extension_scan() {
    let view = known_world_view(
        &[("/p/mod.tsx", "")],
        &[("/p/mod.tsx", "/store/pkg/mod.tsx")],
    );

    let outcome = probe_path_for_context(&view, basis(), "/p/mod.js", true, true);
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
            assert_eq!(value.as_deref(), Some("/store/pkg/mod.tsx"));
            // Discriminates: matches the dual-runner harness's own
            // proven witness for this exact scenario — the rejected
            // higher-priority .ts candidate must be retained.
            assert!(output.consumed_resolution_observations().contains(
                &ConsumedResolutionObservationKey::PathProbe {
                    path: Arc::from("/p/mod.ts")
                }
            ));
        }
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

#[test]
fn falls_through_to_the_bare_extension_scan_when_no_source_sibling_exists() {
    let view = known_world_view(&[("/p/mod.js", "")], &[("/p/mod.js", "/p/mod.js")]);

    // "/p/mod.js" itself is the only present file — neither .ts nor .tsx
    // exist, so `resolve_ts_source_sibling` misses and the bare
    // as-is-extension probe (probe_path's has_extension branch) wins.
    let outcome = probe_path_for_context(&view, basis(), "/p/mod.js", true, true);
    assert!(matches!(
        outcome,
        AttemptOutcome::Complete(CompletedAttempt { value: Some(ref v), .. }) if v == "/p/mod.js"
    ));
}

#[test]
fn exhausts_every_candidate_on_a_genuine_miss() {
    let view = known_world_view(&[], &[]);

    let outcome = probe_path_for_context(&view, basis(), "/p/missing", true, true);
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: None,
            output,
        }) => {
            let probe_count = output
                .consumed_resolution_observations()
                .iter()
                .filter(|key| matches!(key, ConsumedResolutionObservationKey::PathProbe { .. }))
                .count();
            // Discriminates: a port that stopped early (e.g. dropped the
            // index-file scan) retains fewer PathProbe facts than the walk
            // produces.
            //
            // The carrier half of the probe set follows the language
            // registry, so the expected count follows it too. Freezing a
            // total here is how the resolver's own list went stale: a
            // number that must track a registry, pinned where the registry
            // cannot reach it. The non-carrier extensions ARE fixed in
            // source, so that half stays a constant.
            const NON_CARRIER_PROBES: usize = 23;
            let carriers = verter_language::LanguageRegistry::global()
                .carrier_extensions()
                .len();
            assert!(
                carriers > 0,
                "the registry must declare a carrier or this asserts nothing"
            );
            assert_eq!(probe_count, NON_CARRIER_PROBES + carriers);
        }
        other => panic!("expected Complete(None), got {other:?}"),
    }
}

#[test]
fn no_extension_base_skips_the_source_sibling_and_declaration_companion_steps() {
    let view = known_world_view(
        &[("/p/dir/index.ts", "")],
        &[("/p/dir/index.ts", "/p/dir/index.ts")],
    );

    let outcome = probe_path_for_context(&view, basis(), "/p/dir", true, true);
    assert!(matches!(
        outcome,
        AttemptOutcome::Complete(CompletedAttempt { value: Some(ref v), .. }) if v == "/p/dir/index.ts"
    ));
}

#[test]
fn sfc_src_attr_style_calls_skip_the_source_sibling_substitution() {
    // A world where BOTH the literal ".js" file and a ".tsx" source
    // sibling exist — mirrors `<script src="./mod.js">`, which must read
    // the literal file bytes named by the specifier, never substitute a
    // TypeScript source sibling (that substitution is an IMPORT-
    // resolution rule only, per `probe_path_for_context`'s own
    // `ctx.kind != ResolveRequestKind::SfcSrcAttr` gate).
    let view = known_world_view(
        &[("/p/mod.js", ""), ("/p/mod.tsx", "")],
        &[
            ("/p/mod.js", "/store/pkg/mod.js"),
            ("/p/mod.tsx", "/store/pkg/mod.tsx"),
        ],
    );

    let outcome = probe_path_for_context(&view, basis(), "/p/mod.js", false, true);
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
            // Discriminates: with the substitution wrongly applied this
            // would resolve to the .tsx sibling instead.
            assert_eq!(value.as_deref(), Some("/store/pkg/mod.js"));
            assert!(!output.consumed_resolution_observations().contains(
                &ConsumedResolutionObservationKey::PathProbe {
                    path: Arc::from("/p/mod.tsx")
                }
            ));
        }
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

/// A registered framework carrier's modules must be probed, not just Vue's.
///
/// The probe set was hardcoded and listed `.vue` alone, so a `.svelte`
/// module could not be resolved at all. Deriving the carrier half from
/// the language registry is what fixes it, and this asserts the outcome
/// rather than the mechanism: both registered carriers are probed, in
/// registry order, and neither is spelled here by hand.
#[test]
fn every_registered_carrier_extension_is_probed_not_only_vue() {
    let candidates: Vec<String> =
        super::build_probe_candidate_list("/proj/src/thing", false, false)
            .iter()
            .map(ToString::to_string)
            .collect();

    let carrier_candidates: Vec<String> = verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .into_iter()
        .map(|extension| format!("/proj/src/thing.{extension}"))
        .collect();

    assert!(
        !carrier_candidates.is_empty(),
        "the registry must declare at least one carrier, or this asserts nothing"
    );
    for expected in &carrier_candidates {
        assert!(
            candidates.contains(expected),
            "registered carrier candidate {expected} is missing from {candidates:?}"
        );
    }
    assert!(
        candidates.contains(&"/proj/src/thing.svelte".to_string()),
        "svelte is a registered carrier and must be probed: {candidates:?}"
    );
}

/// A view over exactly these observations: a path not listed is unknown,
/// so probing it asks for the input (`NeedInputs`).
fn partial_world_view(
    probes: &[(&str, verter_session_query::resolution::PathProbe)],
    realpaths: &[(&str, &str)],
) -> ResolverAttemptView {
    let mut snapshot = ResolutionObservationSnapshot::default();
    for (path, probe) in probes {
        snapshot.insert_path_probe(*path, *probe);
    }
    for (path, realpath) in realpaths {
        snapshot.insert_real_path(*path, Some(Arc::from(*realpath)));
    }
    ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis())
}

/// An empty snapshot captured under a different world than the attempt
/// expects.
fn stale_world_view() -> ResolverAttemptView {
    let stale = ResolutionBasis::new(
        ResolutionWorldBasis::new(
            verter_session_query::resolution::WorkspaceAuthorityId::test_only(1),
            verter_session_query::resolution::ResolutionPopulation::Base,
            verter_session_query::resolution::ResolutionWorldId::test_only(2),
            None,
        ),
        None,
    );
    ResolverAttemptView::from_resolution_snapshot(
        Arc::new(ResolutionObservationSnapshot::default()),
        stale,
    )
}

/// A barrel directory of `count` modules as an import's attempts see it
/// once its probes are loaded: every candidate's probe is known, first
/// without the hit's real path, then with it.
fn barrel_views(directory: &str, count: usize) -> (ResolverAttemptView, ResolverAttemptView) {
    let view = |with_realpaths: bool| {
        let mut snapshot = ResolutionObservationSnapshot::default();
        for index in 0..count {
            let base = format!("{directory}/leaf-{index}");
            let leaf = format!("{base}.ts");
            for candidate in super::build_probe_candidate_list(&base, true, true).iter() {
                let probe = if **candidate == *leaf {
                    verter_session_query::resolution::PathProbe::File
                } else {
                    verter_session_query::resolution::PathProbe::Absent
                };
                snapshot.insert_path_probe(Arc::clone(candidate), probe);
            }
            if with_realpaths {
                snapshot.insert_real_path(leaf.as_str(), Some(Arc::from(leaf.as_str())));
            }
        }
        ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis())
    };
    (view(false), view(true))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ProbeWork {
    outputs_built: u64,
    witness_retains: u64,
    scope_chains_built: u64,
}

fn probe_work<T>(run: impl FnOnce() -> T) -> (T, ProbeWork) {
    let read = || ProbeWork {
        outputs_built: crate::attempt_output::OUTPUTS_BUILT.get(),
        witness_retains: crate::input_resolution_budgets::WITNESS_RETAINS.get(),
        scope_chains_built: super::SCOPE_CHAIN_BUILDS.get(),
    };
    let before = read();
    let value = run();
    let after = read();
    (
        value,
        ProbeWork {
            outputs_built: after.outputs_built - before.outputs_built,
            witness_retains: after.witness_retains - before.witness_retains,
            scope_chains_built: after.scope_chains_built - before.scope_chains_built,
        },
    )
}

/// Resolves every import of a barrel of `count` modules through the two
/// attempts the driver makes once the probes are loaded, and returns each
/// import's work in them.
fn barrel_import_work(count: usize) -> Vec<(ProbeWork, ProbeWork)> {
    let directory = format!("/repo/barrel-{count}/src");
    let (blocked, known) = barrel_views(&directory, count);
    (0..count)
        .map(|index| {
            let base = format!("{directory}/leaf-{index}");
            let leaf = format!("{base}.ts");
            let candidates = super::build_probe_candidate_list(&base, true, true);
            let operation = crate::InputResolutionRetention::default();
            operation.scope(|| {
                let (outcome, blocked_work) =
                    probe_work(|| super::evaluate_probe_candidates(&blocked, basis(), &candidates));
                match outcome {
                    AttemptOutcome::NeedInputs(load_set) => assert_eq!(
                        load_set.keys(),
                        [verter_session_query::resolution::InputKey::RealPath {
                            path: Arc::from(leaf.as_str())
                        }]
                    ),
                    other => panic!("expected the real path to be needed, got {other:?}"),
                }
                let (outcome, known_work) =
                    probe_work(|| super::evaluate_probe_candidates(&known, basis(), &candidates));
                match outcome {
                    AttemptOutcome::Complete(CompletedAttempt {
                        value: Some(value), ..
                    }) => assert_eq!(value, leaf),
                    other => panic!("expected {leaf} to resolve, got {other:?}"),
                }
                (blocked_work, known_work)
            })
        })
        .collect()
}

/// Every import of a barrel shares its directory, scopes and candidate
/// shape, so its resolution work must not grow with the barrel. Counted,
/// not timed: per import, the blocked attempt builds no output at all (the
/// candidates after the block are discarded, so none is built for them),
/// the resolving attempt builds only the frontier's own, every import
/// retains the same number of witnesses, and the directory's recovery
/// scopes are spelled once for the whole barrel rather than once per
/// candidate.
#[test]
fn barrel_import_resolution_work_stays_flat_as_the_barrel_grows() {
    let small = barrel_import_work(8);
    let large = barrel_import_work(64);

    let first = small[0];
    for (index, work) in small.iter().chain(&large).enumerate() {
        assert_eq!(
            (work.0.outputs_built, work.0.witness_retains),
            (0, 0),
            "import {index}: a blocked attempt keeps no output, so it must build none"
        );
        assert_eq!(
            work.1.outputs_built, 1,
            "import {index}: a resolving attempt builds only the frontier's output"
        );
        assert_eq!(
            work.1.witness_retains, first.1.witness_retains,
            "import {index}: witness work must not depend on the barrel's size"
        );
    }
    let chains = |imports: &[(ProbeWork, ProbeWork)]| {
        imports
            .iter()
            .map(|(blocked, known)| blocked.scope_chains_built + known.scope_chains_built)
            .sum::<u64>()
    };
    assert_eq!(chains(&small), 1, "one directory, one recovery-scope chain");
    assert_eq!(chains(&large), 1, "one directory, one recovery-scope chain");

    // Revert controls: building an output per candidate puts one output per
    // discarded candidate on every blocked attempt; a chain memo that does
    // not keep the directory's chain across imports builds one per import.
}

/// The probe frontier as it was built before candidates recorded into the
/// frontier's own output: each completed candidate builds and fills its own
/// output, which an eager accumulator merges before any block and drops
/// after one. Kept as the reference the frontier must match exactly.
fn reference_probe_frontier(
    view: &ResolverAttemptView,
    candidates: &[Arc<str>],
) -> crate::KernelAttempt<Option<String>> {
    use crate::AttemptOutput;
    use verter_session_query::resolution::{LoadSet, PathProbe};

    fn record(
        output: &mut AttemptOutput,
        key: ConsumedResolutionObservationKey,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
        output.record_consumed_resolution_observation(key)
    }
    fn scopes(
        output: &mut AttemptOutput,
        path: &str,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
        for prefix in super::ancestor_scopes(path) {
            record(
                output,
                ConsumedResolutionObservationKey::RecoveryScope {
                    canonical_prefix: Arc::from(prefix),
                },
            )?;
        }
        Ok(())
    }
    let evaluate = |candidate: &str| -> crate::KernelAttempt<Option<String>> {
        let probe = match crate::ResolverObservation::path_probe(view, candidate) {
            AttemptOutcome::Complete(probe) => probe,
            AttemptOutcome::NeedInputs(load_set) => return AttemptOutcome::NeedInputs(load_set),
            AttemptOutcome::Terminal(failure) => return AttemptOutcome::Terminal(failure),
        };
        if !matches!(probe, PathProbe::File | PathProbe::Directory) {
            let mut output = AttemptOutput::new();
            let recorded = record(
                &mut output,
                ConsumedResolutionObservationKey::PathProbe {
                    path: Arc::from(candidate),
                },
            )
            .and_then(|()| scopes(&mut output, candidate));
            return match recorded {
                Ok(()) => AttemptOutcome::Complete(CompletedAttempt::new(None, output)),
                Err(failure) => AttemptOutcome::Terminal(failure),
            };
        }
        let resolved = match crate::ResolverObservation::real_path(view, candidate) {
            AttemptOutcome::Complete(resolved) => resolved,
            AttemptOutcome::NeedInputs(load_set) => return AttemptOutcome::NeedInputs(load_set),
            AttemptOutcome::Terminal(failure) => return AttemptOutcome::Terminal(failure),
        };
        let resolved = resolved.map_or_else(|| candidate.to_string(), |r| r.to_string());
        let mut output = AttemptOutput::new();
        let recorded = record(
            &mut output,
            ConsumedResolutionObservationKey::PathProbe {
                path: Arc::from(candidate),
            },
        )
        .and_then(|()| {
            record(
                &mut output,
                ConsumedResolutionObservationKey::RealPath {
                    path: Arc::from(candidate),
                },
            )
        })
        .and_then(|()| scopes(&mut output, candidate))
        .and_then(|()| scopes(&mut output, &resolved));
        match recorded {
            Ok(()) => AttemptOutcome::Complete(CompletedAttempt::new(Some(resolved), output)),
            Err(failure) => AttemptOutcome::Terminal(failure),
        }
    };

    let blocked_set = |blocked: std::collections::BTreeSet<_>| {
        AttemptOutcome::NeedInputs(LoadSet::new(blocked.into_iter().collect(), basis()))
    };
    let mut accumulated = AttemptOutput::new();
    let mut blocked: Option<std::collections::BTreeSet<_>> = None;
    for candidate in candidates {
        match evaluate(candidate) {
            AttemptOutcome::Complete(CompletedAttempt {
                value: Some(hit),
                output,
            }) => {
                return match blocked.take() {
                    Some(blocked) => blocked_set(blocked),
                    None => match accumulated.merge(output) {
                        Err(failure) => AttemptOutcome::Terminal(failure),
                        Ok(()) => AttemptOutcome::Complete(CompletedAttempt::new(
                            Some(hit),
                            std::mem::take(&mut accumulated),
                        )),
                    },
                };
            }
            AttemptOutcome::Complete(CompletedAttempt {
                value: None,
                output,
            }) => {
                if blocked.is_none() {
                    if let Err(failure) = accumulated.merge(output) {
                        return AttemptOutcome::Terminal(failure);
                    }
                }
            }
            AttemptOutcome::NeedInputs(load_set) => {
                if load_set.basis() != basis() {
                    return AttemptOutcome::NeedInputs(load_set);
                }
                blocked
                    .get_or_insert_with(Default::default)
                    .extend(load_set.keys().iter().cloned());
            }
            AttemptOutcome::Terminal(failure) => {
                return match blocked.take() {
                    None => AttemptOutcome::Terminal(failure),
                    Some(blocked) => blocked_set(blocked),
                };
            }
        }
    }
    match blocked {
        Some(blocked) => blocked_set(blocked),
        None => AttemptOutcome::Complete(CompletedAttempt::new(None, accumulated)),
    }
}

/// What a frontier answered, and how many witnesses its operation retained
/// while the answer was alive and after it was dropped.
fn frontier_summary(
    held_paths: &[Arc<str>],
    force_retained: Option<u32>,
    run: impl FnOnce() -> crate::KernelAttempt<Option<String>>,
) -> (String, u32, u32) {
    let operation = crate::InputResolutionRetention::default();
    operation.scope(|| {
        // Witnesses another live output of the operation already holds.
        let mut held = crate::AttemptOutput::new();
        for path in held_paths {
            held.record_consumed_resolution_observation(
                ConsumedResolutionObservationKey::PathProbe {
                    path: Arc::clone(path),
                },
            )
            .expect("the held witnesses fit");
            super::record_recovery_scopes(&mut held, path).expect("the held witnesses fit");
        }
        operation.force_completed_witness_retained_for_test(force_retained);
        let outcome = run();
        operation.force_completed_witness_retained_for_test(None);
        let alive = operation.retained_for_test().2;
        let summary = format!("{outcome:?}");
        drop(outcome);
        drop(held);
        (summary, alive, operation.retained_for_test().2)
    })
}

/// The frontier writes a candidate's witnesses straight into its own
/// output before a block and only admits them after one. The answer, the
/// witnesses in order, the needed inputs and the retention left behind must
/// all match building a separate output per candidate, including at the
/// edge of the witness budget, where a discarded candidate whose witnesses
/// no longer fit still ends the frontier on its block.
#[test]
fn the_probe_frontier_matches_building_an_output_per_candidate() {
    use verter_session_query::resolution::PathProbe::{Absent, File};

    let candidates = |base: &str| super::build_probe_candidate_list(base, true, true).to_vec();
    let listed = |paths: &[&str]| {
        paths
            .iter()
            .map(|path| Arc::from(*path))
            .collect::<Vec<_>>()
    };
    let blocked_then_misses =
        || partial_world_view(&[("/r/src/a.ts", File), ("/r/src/a.tsx", Absent)], &[]);
    let maximum = verter_session_query::resolution::InputResolutionBudgets::default()
        .completed_witness_retention();
    type Case = (
        &'static str,
        ResolverAttemptView,
        Vec<Arc<str>>,
        Vec<Arc<str>>,
        Option<u32>,
    );
    let cases: Vec<Case> = vec![
        (
            "hit on the first candidate",
            known_world_view(&[("/r/src/a.ts", "")], &[("/r/src/a.ts", "/r/src/a.ts")]),
            candidates("/r/src/a"),
            vec![],
            None,
        ),
        (
            "hit through a link after misses",
            known_world_view(
                &[("/r/src/a/index.ts", "")],
                &[("/r/src/a/index.ts", "/r/real/index.ts")],
            ),
            candidates("/r/src/a"),
            vec![],
            None,
        ),
        (
            "every candidate misses",
            known_world_view(&[], &[]),
            candidates("/r/src/missing"),
            vec![],
            None,
        ),
        (
            "unknown probes are all needed",
            partial_world_view(&[], &[]),
            candidates("/r/src/a"),
            vec![],
            None,
        ),
        (
            "a block on the real path, then a known miss and an unknown probe",
            blocked_then_misses(),
            listed(&["/r/src/a.ts", "/r/src/a.tsx", "/r/src/a.js"]),
            vec![],
            None,
        ),
        (
            "a discarded miss at the budget edge ends the frontier on its block",
            blocked_then_misses(),
            listed(&["/r/src/a.ts", "/r/src/a.tsx", "/r/src/a.js"]),
            vec![],
            Some(maximum),
        ),
        (
            "a discarded miss whose witnesses are already held fits at the edge",
            blocked_then_misses(),
            listed(&["/r/src/a.ts", "/r/src/a.tsx", "/r/src/a.js"]),
            listed(&["/r/src/a.tsx"]),
            Some(maximum),
        ),
        (
            "a kept miss over the budget is terminal",
            known_world_view(&[], &[]),
            candidates("/r/src/missing"),
            vec![],
            Some(maximum),
        ),
        (
            "a missing input of another world is returned, never unioned",
            stale_world_view(),
            candidates("/r/src/a"),
            vec![],
            None,
        ),
        (
            "a kept hit over the budget is terminal",
            known_world_view(&[("/r/src/a.ts", "")], &[("/r/src/a.ts", "/r/src/a.ts")]),
            candidates("/r/src/a"),
            vec![],
            Some(maximum),
        ),
        (
            "a hit after a block cannot win",
            partial_world_view(
                &[("/r/src/a.ts", File), ("/r/src/a.tsx", File)],
                &[("/r/src/a.tsx", "/r/src/a.tsx")],
            ),
            listed(&["/r/src/a.ts", "/r/src/a.tsx"]),
            vec![],
            None,
        ),
    ];
    for (name, view, candidates, held, force) in cases {
        let expected = frontier_summary(&held, force, || {
            reference_probe_frontier(&view, &candidates)
        });
        let actual = frontier_summary(&held, force, || {
            super::evaluate_probe_candidates(&view, basis(), &candidates)
        });
        assert_eq!(actual, expected, "{name}");
    }

    // Revert control: admitting nothing after a block (treating every
    // discarded miss as fitting) unions the unknown probe in the budget-edge
    // case, so its needed inputs differ from the reference.
}

/// Imports spread over several directories, some resolving through their
/// directory index and every one through a link into a store, touch more
/// directories than a handful: each import's own directory, its index
/// directory, and the real directory it follows. Their recovery-scope
/// chains are still spelled once each, however often the imports repeat.
#[test]
fn recovery_scope_chains_are_spelled_once_across_many_directories() {
    let packages = 6;
    let mut files = Vec::new();
    let mut realpaths = Vec::new();
    for package in 0..packages {
        let (file, real) = if package % 2 == 0 {
            (
                format!("/r/pkg-{package}/src/mod.ts"),
                format!("/r/store/pkg-{package}/mod.ts"),
            )
        } else {
            (
                format!("/r/pkg-{package}/src/mod/index.ts"),
                format!("/r/store/pkg-{package}/index.ts"),
            )
        };
        files.push(file);
        realpaths.push(real);
    }
    let file_refs: Vec<(&str, &str)> = files.iter().map(|file| (file.as_str(), "")).collect();
    let link_refs: Vec<(&str, &str)> = files
        .iter()
        .zip(&realpaths)
        .map(|(file, real)| (file.as_str(), real.as_str()))
        .collect();
    let view = known_world_view(&file_refs, &link_refs);
    let imports: Vec<_> = (0..packages)
        .map(|package| {
            super::build_probe_candidate_list(&format!("/r/pkg-{package}/src/mod"), true, true)
        })
        .collect();
    // Each package's source directory and store directory, and the index
    // directory of every package resolved through its index.
    let directories = packages * 2 + packages / 2;

    let mut builds_per_round = Vec::new();
    for _round in 0..3 {
        let ((), work) = probe_work(|| {
            for (package, candidates) in imports.iter().enumerate() {
                match super::evaluate_probe_candidates(&view, basis(), candidates) {
                    AttemptOutcome::Complete(CompletedAttempt {
                        value: Some(value), ..
                    }) => assert_eq!(value, realpaths[package]),
                    other => panic!("expected package {package} to resolve, got {other:?}"),
                }
            }
        });
        builds_per_round.push(work.scope_chains_built);
    }
    assert_eq!(
        builds_per_round,
        [directories as u64, 0, 0],
        "every directory's chain is spelled once, then reused"
    );

    // Revert control: a cache of four directories replaced in turn cycles
    // through these directories and spells their chains again every round.
}
