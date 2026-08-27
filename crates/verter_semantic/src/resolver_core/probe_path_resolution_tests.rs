use std::sync::Arc;

use super::probe_path_for_context;
use crate::resolver_core::{
    AttemptOutcome, CompletedAttempt, ConsumedResolutionObservationKey, ResolutionBasis,
    ResolutionObservationSnapshot, ResolutionWorldBasis, ResolverAttemptView,
};

fn basis() -> ResolutionBasis {
    ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(1),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(1),
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
        snapshot.insert_path_probe((*path).to_string(), crate::resolver_core::PathProbe::File);
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
    let candidates = super::build_probe_candidate_list("/proj/src/thing", false, false);

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
