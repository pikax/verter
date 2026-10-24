//! The parse identity an artifact read keys by. A session overlay's comes
//! from the view, derived once per view (a carrier's under its registered
//! grammar, the identity its framework parse records), so the key that
//! publishes an overlay artifact is the key that reads it. A file the view
//! does not overlay, while the view holds its base content, is keyed from
//! one base source snapshot, the bytes a materialisation publishes from
//! included; a view that declines the file keys nothing.
//!
//! Derivations are counted across every thread of the process, so each
//! counting test runs in a fresh process of its own ([`in_a_fresh_process`]).

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::checker_probe_lane_tests::{
    in_a_fresh_process, probe_host, test_path, upsert_probe_files, ProbeProject,
};
use crate::file_artifact_store::FileArtifactKeySource;
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
    let content = identity
        .view_content(&host, &view)
        .expect("the view holds the chain file's base content");
    assert!(
        Arc::ptr_eq(&content.source, &old_source),
        "a materialisation reads the snapshot's own bytes"
    );
    assert_eq!(content.content_hash, base_key.content_hash);
    assert_eq!(content.file_language, base_key.file_language_id);
    assert_eq!(content.parse_key.as_ref(), Some(&base_key.parse_key));

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
    let new_source = host.get_source(CHAIN_FILE).expect("the edited bytes");
    let content = identity
        .view_content(&host, &view)
        .expect("the view holds the edited base content");
    assert!(
        Arc::ptr_eq(&content.source, &new_source) && !Arc::ptr_eq(&content.source, &old_source),
        "a materialisation after the edit reads the new revision's bytes"
    );
    assert_eq!(content.content_hash, new_key.content_hash);
    assert_eq!(content.parse_key, Some(new_key.parse_key));
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
    let language = host
        .effective_file_state(FILE, None)
        .expect("a source state")
        .file_language;
    assert_eq!(
        crate::session_view::SessionView::overlay_source_parse_key(&serving, FILE, &language),
        None,
        "a view reporting no overlay derives no overlay identity, though it serves bytes"
    );
    assert!(identity
        .lookup_overlay_artifacts(&host, &declining)
        .is_none());
}

/// A runtime script analysed through its declaration companion is read
/// and published by one key: the script's own bytes and content hash, read
/// under the companion's language. Its materialisation is found again by
/// that key on the next request, the identity is derived once per revision
/// of the script across views, and an edit to the script keys it anew.
#[test]
fn a_script_read_through_its_declaration_companion_is_read_back_by_its_publish_key() {
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
    // An overlay-free view: the materialiser publishes its base-passthrough
    // artifact under the base key.
    let view = crate::session_view::HostView::new(Arc::clone(&host));

    let before = derivations();
    let key = identity
        .current_read_key(&host, &view)
        .expect("the script has a key");
    for _ in 0..50 {
        assert_eq!(identity.current_read_key(&host, &view).as_ref(), Some(&key));
    }
    let second_view = crate::session_view::HostView::new(Arc::clone(&host));
    assert_eq!(
        identity.current_read_key(&host, &second_view).as_ref(),
        Some(&key)
    );
    let derived = derivations() - before;
    assert_eq!(
        key.content_hash,
        verter_semantic_source::source_hash::hash_16(script.as_bytes()),
        "the key names the script's own bytes"
    );
    assert_eq!(key.file_language_id, companion_language);
    assert_eq!(
        Some(&key.parse_key),
        crate::parse::source_parse_key_for(script, &companion_language).as_ref()
    );

    let indexed = host
        .materialize_overlay_indexed_ready_with_view(SCRIPT, &view)
        .expect("the script materialises");
    assert_eq!(
        verter_session_query::source::artifact_key::FileArtifactKey::for_indexed(
            Arc::from(COMPANION),
            &indexed,
            key.parse_env_hash,
        ),
        key,
        "the publish key is the read key"
    );
    let read_back = identity
        .lookup_overlay_artifacts(&host, &second_view)
        .expect("the next request reads the published artifact back");
    assert!(Arc::ptr_eq(&read_back.indexed, &indexed));
    let again = host
        .materialize_overlay_indexed_ready_with_view(SCRIPT, &second_view)
        .expect("the script materialises again");
    assert!(
        Arc::ptr_eq(&again, &indexed),
        "the next request hits the published artifact"
    );

    assert_eq!(
        derived, 1,
        "every read of one script revision, across views, derives its identity once"
    );

    upsert_probe_files(&host, &[("lib.js", "export function f(y) { return y; }\n")]);
    let edited = identity
        .current_read_key(&host, &view)
        .expect("the edited script has a key");
    assert_ne!(edited.content_hash, key.content_hash);
    assert_ne!(
        edited.parse_key, key.parse_key,
        "an edit keys the script anew"
    );
}

/// A raw id the host resolves through an alias: the view serves the
/// canonical's bytes for it but hashes no content under the alias, so the
/// read keys nothing and nothing materialises.
#[test]
fn an_aliased_id_keys_nothing_the_view_does_not_hash() {
    const CANONICAL: &str = "/wb/real.ts";
    const ALIAS: &str = "/wb/alias.ts";
    let host = probe_host(ProbeProject::default());
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(CANONICAL.to_owned()),
            input_id: CANONICAL.to_owned(),
            source: Arc::from("export type T = 1;\n"),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![ALIAS.to_owned()],
        })
        .expect("the aliased file upserts");
    let view = crate::session_view::HostView::new(Arc::clone(&host));
    assert!(
        crate::session_view::SessionView::source(&view, ALIAS).is_some(),
        "the view serves the canonical's bytes under the alias"
    );
    assert_eq!(
        crate::session_view::SessionView::content_hash_for(&view, ALIAS),
        None,
        "the view hashes no content under the alias"
    );
    let identity = host.overlay_artifact_identity(ALIAS);
    assert!(identity.view_content(&host, &view).is_none());
    assert_eq!(identity.current_read_key(&host, &view), None);
    assert!(host
        .materialize_overlay_indexed_ready_with_view(ALIAS, &view)
        .is_none());
    let canonical = host.overlay_artifact_identity(CANONICAL);
    assert_eq!(
        canonical.current_read_key(&host, &view),
        host.authoritative_current_artifact_key(CANONICAL),
        "the canonical id itself is keyed as the host keys it"
    );
}

/// A view over the host that reports, for one canonical, the content hash
/// and source it is given rather than its own.
struct ScriptedView {
    base: crate::session_view::HostView,
    canonical: &'static str,
    content_hash: Option<verter_session_query::analysis::types::Hash16>,
    source: Option<Arc<str>>,
}

impl crate::session_view::SessionView for ScriptedView {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        if canonical == self.canonical {
            return self.source.clone();
        }
        crate::session_view::SessionView::source(&self.base, canonical)
    }

    fn content_hash_for(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::analysis::types::Hash16> {
        if canonical == self.canonical {
            return self.content_hash;
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

/// The bytes a view serves decide the read, never the host's snapshot
/// behind it: a view that hashes the host's revision but declines its
/// source keys nothing, and one that serves bytes of its own under the
/// host's hash keys those bytes.
#[test]
fn a_view_passing_the_host_hash_through_keys_its_own_bytes() {
    const FILE: &str = "/wb/chain.ts";
    let base = chain(5, "T | undefined");
    let files = [("chain.ts", base.as_str())];
    let host = probe_host(ProbeProject {
        files: &files,
        ..ProbeProject::default()
    });
    let identity = host.overlay_artifact_identity(FILE);
    let host_key = host
        .authoritative_current_artifact_key(FILE)
        .expect("the host holds a live base revision");
    let view = |source: Option<Arc<str>>| ScriptedView {
        base: crate::session_view::HostView::new(Arc::clone(&host)),
        canonical: FILE,
        content_hash: Some(host_key.content_hash),
        source,
    };
    assert_eq!(
        identity.current_read_key(&host, &view(None)),
        None,
        "a view declining the source keys nothing, though it hashes the host's revision"
    );
    let own = chain(5, "T | null");
    let serving = view(Some(Arc::from(own.as_str())));
    let key = identity
        .current_read_key(&host, &serving)
        .expect("the view's own bytes are keyed");
    assert_eq!(key.content_hash, host_key.content_hash);
    assert_eq!(
        Some(&key.parse_key),
        crate::parse::source_parse_key_for(&own, &key.file_language_id).as_ref(),
        "the view's bytes are keyed, not the host snapshot's"
    );
    assert_ne!(key.parse_key, host_key.parse_key);
}

/// A base edit between the view's hash read and its source read (the view
/// hashes the earlier revision, serves the current one): the read still
/// keys the bytes the view served, under the hash it reported, and a
/// materialisation still serves — never no answer.
#[test]
fn a_base_edit_between_the_two_view_reads_still_keys_the_served_bytes() {
    const FILE: &str = "/wb/chain.ts";
    let base = chain(5, "T | undefined");
    let files = [("chain.ts", base.as_str())];
    let host = probe_host(ProbeProject {
        files: &files,
        ..ProbeProject::default()
    });
    let identity = host.overlay_artifact_identity(FILE);
    let earlier = host
        .authoritative_current_artifact_key(FILE)
        .expect("the earlier revision's key");
    let edited = chain(5, "T | null");
    upsert_probe_files(&host, &[("chain.ts", edited.as_str())]);
    let current = host.get_source(FILE).expect("the current revision's bytes");
    let raced = ScriptedView {
        base: crate::session_view::HostView::new(Arc::clone(&host)),
        canonical: FILE,
        content_hash: Some(earlier.content_hash),
        source: Some(Arc::clone(&current)),
    };
    let content = identity
        .view_content(&host, &raced)
        .expect("the raced read still has content");
    assert!(Arc::ptr_eq(&content.source, &current));
    assert_eq!(content.content_hash, earlier.content_hash);
    assert_eq!(
        content.parse_key,
        crate::parse::source_parse_key_for(&edited, &content.file_language),
        "the served bytes are keyed"
    );
    assert!(identity.current_read_key(&host, &raced).is_some());
    assert!(
        host.materialize_overlay_indexed_ready_serve_with_view(FILE, &raced)
            .is_some(),
        "the raced materialisation still serves"
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
