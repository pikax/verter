//! Exported alias chains read each exported name once, and each read keys
//! the declaring file's artifact. That key's parse identity belongs to the
//! source revision: it is derived once with the source snapshot (or, for a
//! session overlay, once per view) and every read reuses it. Deriving it
//! per read hashed the whole file once per exported name, so a chain of N
//! exported links in one file hashed about N times the file's size, and
//! the file grows with N: the square of the chain.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::checker_probe_lane_tests::{
    evaluated_render_in as evaluated, probe_host, upsert_probe_files, ProbeProject,
};
use crate::file_artifact_store::FileArtifactKeySource;
use crate::session_view::OverlaidViewRef;
use verter_session_query::source::artifact_key::{FileArtifactKey, BASE_PARSE_ENV_HASH};
use verter_session_query::source::framework_parse::source_parse_identity_derivations_for_tests;

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

/// The whole-source parse-identity derivations one cold answer of the
/// chain's last link makes, and that answer: the chain in the probe module
/// itself, or (`cross_file`) in a sibling module the probe imports from.
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
    let host = probe_host(project);
    let module = if cross_file {
        format!("import type {{ A{n} }} from \"./chain\";\n")
    } else {
        source.clone()
    };
    let before = source_parse_identity_derivations_for_tests();
    let answer = evaluated(&host, project, &module, &format!("A{n}<\"ok\">"));
    (
        source_parse_identity_derivations_for_tests() - before,
        answer,
    )
}

/// The derivations do not grow with the chain: 100, 200 and 400 exported
/// links, in the probe module and imported from a sibling, derive the same
/// number of times (deriving per exported-name read made it N + 3 and
/// N + 5, each pass over a file of size proportional to N).
#[test]
fn an_exported_alias_chain_derives_its_source_parse_identity_a_fixed_number_of_times() {
    for (exported, cross_file) in [(true, false), (true, true), (false, false)] {
        let counts: Vec<usize> = [100, 200, 400]
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
            counts.windows(2).all(|pair| pair[0] == pair[1]),
            "doubling the chain (exported: {exported}, cross-file: {cross_file}) must not \
             add derivations: {counts:?} at 100, 200 and 400 links"
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

/// A session overlay's artifact key derives the overlay's parse identity
/// once per view, however often it is read, and that identity is exactly
/// the one the overlay bytes derive; a view over edited overlay bytes keys
/// them anew.
#[test]
fn an_overlay_key_derives_its_parse_identity_once_per_view() {
    let base = chain(10, true);
    let files = [("chain.ts", base.as_str())];
    let host = probe_host(ProbeProject {
        files: &files,
        ..ProbeProject::default()
    });
    let identity = host.overlay_artifact_identity(CHAIN_FILE);
    let tombstones = std::collections::HashSet::new();
    let overlay_key = |overlay: &str| {
        let mut sources = FxHashMap::default();
        sources.insert(CHAIN_FILE.to_owned(), Arc::<str>::from(overlay));
        let mut hashes = FxHashMap::default();
        hashes.insert(
            CHAIN_FILE.to_owned(),
            verter_semantic_source::source_hash::hash_16(overlay.as_bytes()),
        );
        let view = OverlaidViewRef::new(&host, &sources, &hashes, &tombstones);
        let before = source_parse_identity_derivations_for_tests();
        let key = identity
            .current_read_key(&host, &view)
            .expect("the overlay has a key");
        for _ in 0..50 {
            assert_eq!(identity.current_read_key(&host, &view).as_ref(), Some(&key));
        }
        assert_eq!(
            source_parse_identity_derivations_for_tests() - before,
            1,
            "fifty-one reads of one view's overlay key derive its identity once"
        );
        let from_bytes = verter_session_query::source::framework_parse::exact_source_parse_key(
            overlay,
            &key.file_language_id,
            None,
        );
        assert_eq!(Some(&key.parse_key), from_bytes.as_ref());
        key
    };
    let first = overlay_key(&chain(20, true));
    let edited = overlay_key(&chain_from(20, true, "T | null"));
    assert_ne!(
        first.parse_key, edited.parse_key,
        "a view over edited overlay bytes must not reuse the earlier identity"
    );
    assert_eq!(overlay_key(&chain(20, true)), first);
}
