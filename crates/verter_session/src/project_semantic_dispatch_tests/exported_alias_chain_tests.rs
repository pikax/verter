//! Exported alias chains read each exported name once, and each read keys
//! the declaring file's artifact. That key's parse identity belongs to the
//! source revision: it is derived once with the source snapshot and every
//! read reuses it, so the whole-source derivations an answer makes do not
//! grow with the chain.
//!
//! The derivations are counted across every thread of the process (the
//! source stage derives on the scheduler's workers), so each counting test
//! runs in a fresh process of its own ([`in_a_fresh_process`]).

use std::sync::Arc;

use super::checker_probe_lane_tests::{
    evaluated_render_in as evaluated, in_a_fresh_process, probe_host, test_path,
    upsert_probe_files, ProbeProject,
};
use crate::file_artifact_store::FileArtifactKeySource;
use verter_session_query::source::artifact_key::{FileArtifactKey, BASE_PARSE_ENV_HASH};
use verter_session_query::source::framework_parse::source_parse_identity_derivations_in_process_for_tests as derivations;

/// The chain file's canonical in a probe project.
const CHAIN_FILE: &str = "/wb/chain.ts";

/// `A0<T> = T | undefined` and `n` links `A{i+1}<T> = A{i}<T>`, each
/// exported when `exported`.
fn chain(n: usize, exported: bool) -> String {
    chain_from(n, exported, "T | undefined")
}

fn chain_from(n: usize, exported: bool, root: &str) -> String {
    let export = if exported { "export " } else { "" };
    let mut source = format!("{export}type A0<T> = {root};\n");
    for i in 0..n {
        source.push_str(&format!("{export}type A{}<T> = A{i}<T>;\n", i + 1));
    }
    source
}

/// The whole-source parse-identity derivations, on any thread, that
/// loading the project and one cold answer of the chain's last link make,
/// and that answer: the chain in the probe module itself, or (`cross_file`)
/// in a sibling module the probe imports from.
fn derivations_for(n: usize, exported: bool, cross_file: bool) -> (usize, String) {
    let source = chain(n, exported);
    let files = [("chain.ts", source.as_str())];
    let project = if cross_file {
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        }
    } else {
        ProbeProject::default()
    };
    let module = if cross_file {
        format!("import type {{ A{n} }} from \"./chain\";\n")
    } else {
        source.clone()
    };
    let before = derivations();
    let host = probe_host(project);
    let answer = evaluated(&host, project, &module, &format!("A{n}<\"ok\">"));
    (derivations() - before, answer)
}

fn exported_alias_chain_derivations_stay_fixed() {
    assert_alias_chain_derivations_stay_fixed([100, 200, 400]);
}

/// The chains at 25, 50 and 101 links: the gate version of the 100-, 200-
/// and 400-link test in the nightly scale tier. A derivation per link shows
/// at every length, so equal counts at these lengths are the full-size
/// test's guarantee; the 101-link chain is still past the checker's
/// instantiation depth of 100, so the longest gate chain answers where a
/// depth-bound reading would refuse it.
#[test]
fn an_alias_chain_derives_its_source_parse_identity_a_fixed_number_of_times() {
    in_a_fresh_process(test_path!(), || {
        assert_alias_chain_derivations_stay_fixed([25, 50, 101])
    });
}

/// Each layout's chain answers at each of `lengths` with the same, non-zero
/// number of whole-source parse-identity derivations.
fn assert_alias_chain_derivations_stay_fixed(lengths: [usize; 3]) {
    for (exported, cross_file) in [(true, false), (true, true), (false, false)] {
        let counts: Vec<usize> = lengths
            .into_iter()
            .map(|n| {
                let (derivations, answer) = derivations_for(n, exported, cross_file);
                assert_eq!(
                    answer, "Union(\"ok\" | undefined)",
                    "the {n}-link chain (exported: {exported}, cross-file: {cross_file}) answers"
                );
                derivations
            })
            .collect();
        assert!(
            counts[0] > 0,
            "the count must see each source revision's one derivation \
             (exported: {exported}, cross-file: {cross_file}): {counts:?}"
        );
        assert!(
            counts.windows(2).all(|pair| pair[0] == pair[1]),
            "lengthening the chain (exported: {exported}, cross-file: {cross_file}) must not \
             add derivations: {counts:?} at {lengths:?} links"
        );
    }
}

/// An edit to the chain file changes its key and the answer: the reused
/// identity is the edited revision's, never the one before it; restoring
/// the original bytes restores the original key.
#[test]
fn an_edit_to_an_exported_chain_rekeys_it_and_changes_the_answer() {
    const N: usize = 50;
    let original = chain(N, true);
    let edited = chain_from(N, true, "T | null");
    let files = [("chain.ts", original.as_str())];
    let project = ProbeProject {
        files: &files,
        ..ProbeProject::default()
    };
    let host = probe_host(project);
    let module = format!("import type {{ A{N} }} from \"./chain\";\n");
    let probe = format!("A{N}<\"ok\">");
    let key_from_source = |source: &str| {
        let state = host
            .effective_file_state(CHAIN_FILE, None)
            .expect("the chain file has a source state");
        assert_eq!(
            state.source.as_ref(),
            source,
            "the chain file holds the bytes"
        );
        FileArtifactKey::for_source_identity(
            Arc::from(CHAIN_FILE),
            state.whole_hash,
            source,
            state.file_language.clone(),
            None,
            BASE_PARSE_ENV_HASH,
        )
        .expect("a script has a parse identity")
    };

    assert_eq!(
        evaluated(&host, project, &module, &probe),
        "Union(\"ok\" | undefined)"
    );
    let original_key = host
        .authoritative_current_artifact_key(CHAIN_FILE)
        .expect("the chain file has a key");
    assert_eq!(original_key, key_from_source(&original));

    upsert_probe_files(&host, &[("chain.ts", edited.as_str())]);
    let edited_key = host
        .authoritative_current_artifact_key(CHAIN_FILE)
        .expect("the edited chain file has a key");
    assert_ne!(
        edited_key.parse_key, original_key.parse_key,
        "an edit must not reuse the earlier revision's parse identity"
    );
    assert_eq!(edited_key, key_from_source(&edited));
    assert_eq!(
        evaluated(&host, project, &module, &probe),
        "Union(\"ok\" | null)",
        "the edited chain answers from its own revision"
    );

    upsert_probe_files(&host, &[("chain.ts", original.as_str())]);
    assert_eq!(
        host.authoritative_current_artifact_key(CHAIN_FILE),
        Some(original_key),
        "the same bytes key the same identity again"
    );
    assert_eq!(
        evaluated(&host, project, &module, &probe),
        "Union(\"ok\" | undefined)"
    );
}

/// Full-size workloads, run by the nightly scale tier (`.config/nextest.toml`).
/// Each has a gate version outside this module.
mod nightly_scale {
    use super::*;

    /// The derivations do not grow with the chain: 100, 200 and 400 exported
    /// links, in the probe module and imported from a sibling, derive the same
    /// number of times — each source revision once, in its source stage, which
    /// the count must see (it is never zero).
    #[test]
    fn an_exported_alias_chain_derives_its_source_parse_identity_a_fixed_number_of_times() {
        in_a_fresh_process(test_path!(), exported_alias_chain_derivations_stay_fixed);
    }
}
