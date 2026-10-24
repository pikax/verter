//! The parse identity an artifact read keys by. A session overlay's comes
//! from the view, derived once per view (a carrier's under its registered
//! grammar, the identity its framework parse records), so the key that
//! publishes an overlay artifact is the key that reads it. A file the view
//! does not overlay, while the view holds its base content, is keyed by the
//! host's published base key from one source snapshot — for a runtime
//! script analysed through its declaration companion, the companion's; a
//! view that declines the file keys nothing.
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
        identity.materialization_identity(&host, &view, &old_source),
        (
            base_key.file_language_id.clone(),
            Some(base_key.parse_key.clone())
        ),
        "bytes the base snapshot holds materialise under the read key's language and identity"
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
        identity
            .materialization_identity(&host, &view, &old_source)
            .1,
        None,
        "bytes the base snapshot moved past are not seeded"
    );
    let new_source = host.get_source(CHAIN_FILE).expect("the edited bytes");
    assert_eq!(
        identity.materialization_identity(&host, &view, &new_source),
        (new_key.file_language_id.clone(), Some(new_key.parse_key))
    );
}

/// A view over the host that declines one canonical (no source, no
/// content), or serves its own bytes for it without declaring an overlay.
struct OneCanonicalView {
    base: crate::session_view::HostView,
    canonical: &'static str,
    served: Option<Arc<str>>,
}

impl crate::session_view::SessionView for OneCanonicalView {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        if canonical == self.canonical {
            return self.served.clone();
        }
        crate::session_view::SessionView::source(&self.base, canonical)
    }

    fn content_hash_for(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::analysis::types::Hash16> {
        if canonical == self.canonical {
            return self
                .served
                .as_ref()
                .map(|served| verter_semantic_source::source_hash::hash_16(served.as_bytes()));
        }
        crate::session_view::SessionView::content_hash_for(&self.base, canonical)
    }

    fn project_identity(&self) -> verter_session_query::resolution::ProjectIdentity {
        crate::session_view::SessionView::project_identity(&self.base)
    }

    fn env_hashes(&self) -> &verter_session_query::resolution::EnvHashes {
        crate::session_view::SessionView::env_hashes(&self.base)
    }

    fn resolved_import_facts(
        &self,
        canonical: &str,
    ) -> Option<crate::resolver_core::CachedValue<crate::resolved_import_facts::ResolvedImportFacts>>
    {
        crate::session_view::SessionView::resolved_import_facts(&self.base, canonical)
    }
}

/// A view that declines a file's content keys no artifact for it, though
/// the host holds a live base revision; a view serving its own bytes for it
/// without declaring an overlay keys those bytes, never the host's.
#[test]
fn a_view_that_declines_a_file_keys_no_base_artifact_for_it() {
    const FILE: &str = "/wb/chain.ts";
    let base = chain(5, "T | undefined");
    let files = [("chain.ts", base.as_str())];
    let host = probe_host(ProbeProject {
        files: &files,
        ..ProbeProject::default()
    });
    let identity = host.overlay_artifact_identity(FILE);
    assert!(
        host.authoritative_current_artifact_key(FILE).is_some(),
        "the host holds a live base revision"
    );
    let declining = OneCanonicalView {
        base: crate::session_view::HostView::new(Arc::clone(&host)),
        canonical: FILE,
        served: None,
    };
    assert_eq!(
        identity.current_read_key(&host, &declining),
        None,
        "a declined file keys nothing"
    );
    assert!(identity
        .lookup_overlay_artifacts(&host, &declining)
        .is_none());

    let other = chain(5, "T | null");
    let serving = OneCanonicalView {
        base: crate::session_view::HostView::new(Arc::clone(&host)),
        canonical: FILE,
        served: Some(Arc::from(other.as_str())),
    };
    let key = identity
        .current_read_key(&host, &serving)
        .expect("served bytes have a key");
    assert_eq!(
        key.content_hash,
        verter_semantic_source::source_hash::hash_16(other.as_bytes())
    );
    assert_eq!(
        Some(&key.parse_key),
        crate::parse::source_parse_key_for(&other, &key.file_language_id).as_ref(),
        "the view's own bytes are keyed, not the host's"
    );
    assert_ne!(Some(key), host.authoritative_current_artifact_key(FILE));
}

/// A runtime script analysed through its declaration companion is keyed,
/// on a base read, by the key the host publishes the companion's artifact
/// under — the companion's own snapshot identity, with no derivation — and
/// follows an edit to the companion. The script's own bytes never seed an
/// artifact under that key.
#[test]
fn a_script_read_through_its_declaration_companion_keys_the_companion_artifact() {
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
    let tombstones = std::collections::HashSet::new();
    let (sources, hashes) = overlay_maps("/wb/other.ts", "export {};\n");
    let view = OverlaidViewRef::new(&host, &sources, &hashes, &tombstones);

    let before = derivations();
    let key = identity
        .current_read_key(&host, &view)
        .expect("the script has a key");
    for _ in 0..50 {
        assert_eq!(identity.current_read_key(&host, &view).as_ref(), Some(&key));
    }
    assert_eq!(
        derivations() - before,
        0,
        "the companion's snapshot identity is read, not derived"
    );
    assert_eq!(
        Some(&key),
        host.authoritative_current_artifact_key(COMPANION).as_ref(),
        "the read key is the key the host publishes the companion under"
    );
    let script_bytes = host.get_source(SCRIPT).expect("the script's bytes");
    assert_eq!(
        identity
            .materialization_identity(&host, &view, &script_bytes)
            .1,
        None,
        "the script's own bytes are never seeded with the companion's key"
    );

    upsert_probe_files(
        &host,
        &[("lib.d.ts", "export declare function f<T>(x: T): [T];\n")],
    );
    let edited = identity
        .current_read_key(&host, &view)
        .expect("the script has a key after the companion's edit");
    assert_ne!(edited.parse_key, key.parse_key);
    assert_eq!(
        Some(&edited),
        host.authoritative_current_artifact_key(COMPANION).as_ref(),
        "the read key follows the companion's new revision"
    );
}

/// A carrier overlay's read key carries the identity its framework parse
/// records, under the carrier's registered grammar: the materialised
/// overlay artifact is found again by that key, a fresh view over the same
/// overlay derives it once however often it reads it, and the key sees the
/// grammar's delimiters (other delimiters key the same bytes differently).
#[test]
fn a_carrier_overlay_is_read_back_by_the_key_that_published_it() {
    in_a_fresh_process(test_path!(), carrier_overlay_keys);
}

fn carrier_overlay_keys() {
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
        let fresh = OverlaidViewRef::new(&host, &sources, &hashes, &tombstones);
        let before = derivations();
        for _ in 0..51 {
            assert_eq!(
                identity.current_read_key(&host, &fresh).as_ref(),
                Some(&key)
            );
        }
        assert_eq!(
            derivations() - before,
            1,
            "{canonical}: fifty-one reads of one view's carrier overlay key derive it once"
        );
        assert!(
            identity
                .lookup_overlay_artifacts(&host, &fresh)
                .is_some_and(|facts| Arc::ptr_eq(&facts.indexed, &indexed)),
            "{canonical}: a fresh view over the same overlay reads the artifact back"
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
