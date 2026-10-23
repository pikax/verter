//! The parse identity an artifact read keys by. A session overlay's comes
//! from the view, derived once per view (a carrier's under its registered
//! grammar, the identity its framework parse records), so the key that
//! publishes an overlay artifact is the key that reads it. A file the view
//! does not overlay is keyed from one base source snapshot: its own
//! identity, or, for a runtime script analysed through its declaration
//! companion, an identity that revision derives once.
//!
//! Derivations are counted across every thread of the process, so each
//! counting test runs in a fresh process of its own ([`in_a_fresh_process`]).

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::checker_probe_lane_tests::{
    in_a_fresh_process, probe_host, test_path, upsert_probe_files, ProbeProject,
};
use crate::session_view::OverlaidViewRef;
use verter_session_query::source::framework_parse::source_parse_identity_derivations_in_process_for_tests as derivations;

/// `A0<T> = T | undefined` and `n` exported links `A{i+1}<T> = A{i}<T>`
/// over `root`.
fn chain(n: usize, root: &str) -> String {
    let mut source = format!("export type A0<T> = {root};\n");
    for i in 0..n {
        source.push_str(&format!("export type A{}<T> = A{i}<T>;\n", i + 1));
    }
    source
}

/// One overlay `canonical` → `source`, with its precomputed content hash.
fn overlay_maps(
    canonical: &str,
    source: &str,
) -> (
    FxHashMap<String, Arc<str>>,
    FxHashMap<String, verter_session_query::analysis::types::Hash16>,
) {
    let mut sources = FxHashMap::default();
    sources.insert(canonical.to_owned(), Arc::<str>::from(source));
    let mut hashes = FxHashMap::default();
    hashes.insert(
        canonical.to_owned(),
        verter_semantic_source::source_hash::hash_16(source.as_bytes()),
    );
    (sources, hashes)
}

/// A session overlay's artifact key derives the overlay's parse identity
/// once per view, however often it is read, and that identity is exactly
/// the one the overlay bytes derive; a view over edited overlay bytes keys
/// them anew. A file the view does not overlay is keyed from the base
/// snapshot, exactly as the host keys it, with no derivation at all, and
/// follows a base edit made while the view is live; a seed for bytes the
/// base snapshot has moved past is refused.
#[test]
fn an_overlay_key_derives_once_per_view_and_a_base_key_follows_the_snapshot() {
    in_a_fresh_process(test_path!(), overlay_and_base_keys);
}

fn overlay_and_base_keys() {
    const CHAIN_FILE: &str = "/wb/chain.ts";
    let base = chain(10, "T | undefined");
    let files = [("chain.ts", base.as_str())];
    let host = probe_host(ProbeProject {
        files: &files,
        ..ProbeProject::default()
    });
    let identity = host.overlay_artifact_identity(CHAIN_FILE);
    let tombstones = std::collections::HashSet::new();
    let overlay_key = |overlay: &str| {
        let (sources, hashes) = overlay_maps(CHAIN_FILE, overlay);
        let view = OverlaidViewRef::new(&host, &sources, &hashes, &tombstones);
        let before = derivations();
        let key = identity
            .current_read_key(&host, &view)
            .expect("the overlay has a key");
        for _ in 0..50 {
            assert_eq!(identity.current_read_key(&host, &view).as_ref(), Some(&key));
        }
        assert_eq!(
            derivations() - before,
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
    let first = overlay_key(&chain(20, "T | undefined"));
    let edited = overlay_key(&chain(20, "T | null"));
    assert_ne!(
        first.parse_key, edited.parse_key,
        "a view over edited overlay bytes must not reuse the earlier identity"
    );
    assert_eq!(overlay_key(&chain(20, "T | undefined")), first);

    // A view overlaying another file reads the chain file from the base.
    let (sources, hashes) = overlay_maps("/wb/other.ts", "export {};\n");
    let view = OverlaidViewRef::new(&host, &sources, &hashes, &tombstones);
    let before = derivations();
    let base_key = identity
        .current_read_key(&host, &view)
        .expect("the base chain file has a key");
    for _ in 0..50 {
        assert_eq!(
            identity.current_read_key(&host, &view).as_ref(),
            Some(&base_key)
        );
    }
    assert_eq!(
        derivations() - before,
        0,
        "base reads take the snapshot's identity without a derivation"
    );
    assert_eq!(
        Some(&base_key),
        host.authoritative_current_artifact_key(CHAIN_FILE).as_ref(),
        "a base read keys the chain file as the host does"
    );
    let old_source = host.get_source(CHAIN_FILE).expect("the chain file's bytes");
    assert_eq!(
        identity.materialized_parse_key(&host, &view, &old_source, &base_key.file_language_id),
        Some(base_key.parse_key.clone()),
        "bytes the base snapshot holds are seeded with the read key's identity"
    );

    // A base edit while the view is live: the read key follows it, and the
    // superseded bytes are no longer seeded.
    let edited_base = chain(10, "T | null");
    upsert_probe_files(&host, &[("chain.ts", edited_base.as_str())]);
    let new_key = identity
        .current_read_key(&host, &view)
        .expect("the edited chain file has a key");
    assert_ne!(new_key.content_hash, base_key.content_hash);
    assert_ne!(new_key.parse_key, base_key.parse_key);
    assert_eq!(
        Some(&new_key),
        host.authoritative_current_artifact_key(CHAIN_FILE).as_ref(),
        "the live view's base read keys the new revision"
    );
    assert_eq!(
        identity.materialized_parse_key(&host, &view, &old_source, &new_key.file_language_id),
        None,
        "bytes the base snapshot moved past are not seeded"
    );
    let new_source = host.get_source(CHAIN_FILE).expect("the edited bytes");
    assert_eq!(
        identity.materialized_parse_key(&host, &view, &new_source, &new_key.file_language_id),
        Some(new_key.parse_key)
    );
}

/// A runtime script analysed through its declaration companion keys its
/// own bytes under the companion's language. That identity is derived once
/// per revision of the script, not per read and not per view, and an edit
/// to the script keys it anew.
#[test]
fn a_script_read_through_its_declaration_companion_derives_once_per_revision() {
    in_a_fresh_process(test_path!(), companion_keys);
}

fn companion_keys() {
    const SCRIPT: &str = "/wb/lib.js";
    const COMPANION: &str = "/wb/lib.d.ts";
    let script = "export function f(x) { return x; }\n";
    let companion = "export declare function f<T>(x: T): T;\n";
    let files = [("lib.js", script), ("lib.d.ts", companion)];
    let host = probe_host(ProbeProject {
        files: &files,
        ..ProbeProject::default()
    });
    let identity = host.overlay_artifact_identity(SCRIPT);
    assert_eq!(
        identity.analysis_canonical(),
        COMPANION,
        "the script is analysed through its declaration companion"
    );
    let companion_language = host
        .effective_file_state(COMPANION, None)
        .expect("the companion has a source state")
        .file_language;
    let tombstones = std::collections::HashSet::new();
    let read_in_a_new_view = || {
        let (sources, hashes) = overlay_maps("/wb/other.ts", "export {};\n");
        let view = OverlaidViewRef::new(&host, &sources, &hashes, &tombstones);
        let key = identity
            .current_read_key(&host, &view)
            .expect("the script has a key");
        for _ in 0..50 {
            assert_eq!(identity.current_read_key(&host, &view).as_ref(), Some(&key));
        }
        key
    };

    let before = derivations();
    let key = read_in_a_new_view();
    let second_view = read_in_a_new_view();
    assert_eq!(key, second_view);
    assert_eq!(
        derivations() - before,
        1,
        "every read of one script revision, across views, derives its companion-language identity once"
    );
    assert_eq!(key.file_language_id, companion_language);
    assert_eq!(
        Some(&key.parse_key),
        crate::parse::source_parse_key_for(script, &companion_language).as_ref(),
        "the identity is the script's bytes read under the companion's language"
    );

    upsert_probe_files(&host, &[("lib.js", "export function f(y) { return y; }\n")]);
    let edited = read_in_a_new_view();
    assert_ne!(
        edited.parse_key, key.parse_key,
        "an edit to the script keys it anew"
    );
}

/// A carrier overlay's read key carries the identity its framework parse
/// records, under the carrier's registered grammar: the materialised
/// overlay artifact is found again by that key, and the key sees the
/// grammar's delimiters (other delimiters key the same bytes differently).
#[test]
fn a_carrier_overlay_is_read_back_by_the_key_that_published_it() {
    let vue = "<script setup lang=\"ts\">\nconst n = 1\n</script>\n<template><div>{{ n }}</div></template>\n";
    let svelte = "<script lang=\"ts\">\nlet n = 1;\n</script>\n<p>{n}</p>\n";
    for (canonical, source) in [("/wb/Overlay.vue", vue), ("/wb/Overlay.svelte", svelte)] {
        let host = probe_host(ProbeProject::default());
        let identity = host.overlay_artifact_identity(canonical);
        let (sources, hashes) = overlay_maps(canonical, source);
        let tombstones = std::collections::HashSet::new();
        let view = OverlaidViewRef::new(&host, &sources, &hashes, &tombstones);
        let indexed = host
            .materialize_overlay_indexed_ready_with_view(canonical, &view)
            .unwrap_or_else(|| panic!("the overlay {canonical} materialises"));
        let recorded = indexed
            .framework_parse
            .as_deref()
            .unwrap_or_else(|| panic!("the overlay {canonical} has a carrier parse"))
            .parse_key()
            .clone();
        let key = identity
            .current_read_key(&host, &view)
            .unwrap_or_else(|| panic!("the overlay {canonical} has a read key"));
        assert_eq!(
            key.parse_key, recorded,
            "{canonical}: the read key carries the identity its carrier parse recorded"
        );
        assert_eq!(indexed.source_parse_key().as_ref(), Some(&recorded));
        let read_back = identity
            .lookup_overlay_artifacts(&host, &view)
            .unwrap_or_else(|| panic!("{canonical}: the published overlay is read back"));
        assert!(
            Arc::ptr_eq(&read_back.indexed, &indexed),
            "{canonical}: the read key finds the artifact its publish key stored"
        );
        if canonical.ends_with(".vue") {
            let language = &key.file_language_id;
            let options = |open: &str, close: &str| verter_language::ParseOptions {
                delimiters: (open.to_owned(), close.to_owned()),
                custom_elements: Vec::new(),
                svelte_loose: false,
            };
            let under = |open, close| {
                verter_language::parse_identity_for(source, language, &options(open, close))
                    .expect("a Vue identity")
                    .1
            };
            assert_eq!(key.parse_key, under("{{", "}}"));
            assert_ne!(
                key.parse_key,
                under("[[", "]]"),
                "the key sees the grammar's delimiters"
            );
        }
    }
}
