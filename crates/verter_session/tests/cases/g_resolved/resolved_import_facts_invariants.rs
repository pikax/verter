//! Discriminating invariant tests for
//! [`verter_session::resolved_import_facts::ResolvedImportFactsDb`].
//!
//! Pins:
//!
//! - **R5** (content-addressed): two parses of the same source
//!   coexist; an edit re-keys.
//! - **R12** (resolve-domain isolation): a `paths` edit
//!   (`resolve_env_hash` change) invalidates `ResolvedImportFacts`.
//! - **R21** (scoping rule): `lib_env_hash` is NOT a key dimension.
//!   A TS lib change MUST NOT invalidate `ResolvedImportFacts`.
//! - **R28** (substrate version): `resolver_version` bump isolates.
//!
//! Each test in this file is discriminating: it FAILS against a tree
//! that does not implement the cache (the
//! `resolved_import_facts` module would be absent and the file
//! would fail to compile), and PASSES against a tree that exposes
//! the cache with the documented R5/R12/R21/R28 key composition.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use verter_session::resolved_import_facts::{
    ResolvedImportFacts, ResolvedImportFactsKey, RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
};
use verter_session::session_view::{EnvHashes, HostView, SessionView};
use verter_session::{
    CompileErrorPolicy, FileLanguage, Hash16, HostConfig, UpsertRequest, VerterHost,
};

/// Construct a fresh `VerterHost` for these unit tests. Hermetic —
/// no external corpus, no workspace files. The host is wrapped in
/// `Arc<>` so view constructors that need owned hosts can clone it
/// cheaply.
fn fresh_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }))
}

/// Upsert a canonical so the file-artifact store reports a real
/// content hash through the view's `content_hash_for(...)` accessor.
///
/// `evaluate_types` is the public entry point that triggers full
/// indexing (including `FileArtifactStore` population) without
/// touching the `pub(crate)` `ensure_indexed_ready` helper.
fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
    let _ = host.evaluate_types(canonical);
}

/// Build a synthetic `ResolvedImportFacts` payload. Used by tests
/// that need an admission-ready value but do not care about the
/// facts' internal shape. Returns a unique `Arc` per call so the
/// `Arc::ptr_eq` checks in the tests can distinguish admission
/// races.
fn synthetic_facts() -> Arc<ResolvedImportFacts> {
    Arc::new(ResolvedImportFacts::new())
}

/// Build a `ResolvedImportFactsKey` with explicit hash components.
///
/// Helper defaults `known_miss_generation` to `[0u8; 16]` — the same
/// value the producer composes when the owner has no recorded
/// known-misses (empty
/// `DerivedRawState::import_routes_known_miss_recorded_at_generation`
/// or no `DerivedRawState` entry yet). Tests that want to vary
/// `known_miss_generation` construct the literal directly.
fn key_with(
    canonical: &str,
    content_hash: Hash16,
    parse_env_hash: Hash16,
    resolve_env_hash: Hash16,
    resolver_version: u32,
) -> ResolvedImportFactsKey {
    ResolvedImportFactsKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash,
        resolve_env_hash,
        resolver_version,
        known_miss_generation: [0u8; 16],
    }
}

/// Build an `EnvHashes` carrier with a non-default lib_env_hash so a
/// view's lookup helper carries the supplied envs through. Other
/// dimensions remain controllable per-test.
fn env_hashes(parse_env_hash: Hash16, resolve_env_hash: Hash16, lib_env_hash: Hash16) -> EnvHashes {
    EnvHashes {
        parse_env_hash,
        resolve_env_hash,
        type_env_hash: [0u8; 16],
        lib_env_hash,
    }
}

// ---------------------------------------------------------------------------
// R21 — `lib_env_hash` is NOT part of the key
// ---------------------------------------------------------------------------

#[test]
fn key_excludes_lib_env_hash() {
    // Two `HostView` instances differ only in their `lib_env_hash`
    // dimension. The resolved-import facts cache key must not
    // include `lib_env_hash`, so both views look up the SAME slot
    // and observe the SAME `Arc<ResolvedImportFacts>`.
    let host = fresh_host();
    upsert(&host, "/x.ts", "export const a = 1;");

    let content_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/x.ts")
        .expect("base content hash for ingested canonical");

    let parse_h = [0x11u8; 16];
    let resolve_h = [0x22u8; 16];
    let lib_h_a = [0xaau8; 16];
    let lib_h_b = [0xbbu8; 16];

    // Pre-populate the resolved-import facts cache under the
    // shared (canonical, content_hash, parse_env_hash,
    // resolve_env_hash, resolver_version) quintuple.
    let key = key_with(
        "/x.ts",
        content_hash,
        parse_h,
        resolve_h,
        RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    );
    let payload = synthetic_facts();
    let admitted = host
        .project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key, Arc::clone(&payload));
    assert!(admitted, "first admission should win on the empty cache");

    let view_a =
        HostView::with_env_hashes(Arc::clone(&host), env_hashes(parse_h, resolve_h, lib_h_a));
    let view_b =
        HostView::with_env_hashes(Arc::clone(&host), env_hashes(parse_h, resolve_h, lib_h_b));

    let observed_a = view_a
        .resolved_import_facts("/x.ts")
        .expect("view A must see the cached payload under its lib env hash");
    let observed_b = view_b
        .resolved_import_facts("/x.ts")
        .expect("view B must see the cached payload under its lib env hash");

    assert!(
        Arc::ptr_eq(&observed_a, &observed_b),
        "two views differing only in `lib_env_hash` must reach the same cache slot (R21 scoping rule)"
    );
    assert!(
        Arc::ptr_eq(&observed_a, &payload),
        "the cached payload must be the same `Arc` admitted above"
    );
}

#[test]
fn lib_change_does_not_invalidate_resolved_import_facts() {
    // R21 scoping rule restated as a paired assertion:
    // changing `lib_env_hash` between two views yields the SAME
    // cache slot — there is no second slot to invalidate.
    let host = fresh_host();
    upsert(&host, "/y.ts", "export const b = 2;");

    let content_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/y.ts")
        .expect("base content hash for ingested canonical");

    let parse_h = [0x05u8; 16];
    let resolve_h = [0x06u8; 16];

    let key = key_with(
        "/y.ts",
        content_hash,
        parse_h,
        resolve_h,
        RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    );
    let payload = synthetic_facts();
    host.project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key, Arc::clone(&payload));

    let view_with_dom_lib = HostView::with_env_hashes(
        Arc::clone(&host),
        env_hashes(parse_h, resolve_h, [0x01u8; 16]),
    );
    let view_with_node_lib = HostView::with_env_hashes(
        Arc::clone(&host),
        env_hashes(parse_h, resolve_h, [0x02u8; 16]),
    );

    let observed_dom = view_with_dom_lib
        .resolved_import_facts("/y.ts")
        .expect("dom-lib view sees the cached payload");
    let observed_node = view_with_node_lib
        .resolved_import_facts("/y.ts")
        .expect("node-lib view sees the cached payload");
    assert!(
        Arc::ptr_eq(&observed_dom, &observed_node),
        "a `lib_env_hash` change MUST NOT invalidate `ResolvedImportFacts` (R21)"
    );
    assert_eq!(
        host.project_type_store()
            .resolved_import_facts()
            .entry_count(),
        1,
        "the cache must hold exactly one entry — the lib change did not produce a second slot"
    );
}

// ---------------------------------------------------------------------------
// R12 — `paths` edits invalidate
// ---------------------------------------------------------------------------

#[test]
fn paths_edit_invalidates_resolved_import_facts() {
    // A `paths` edit (resolve-env mutation) changes
    // `resolve_env_hash`. Two views with different
    // `resolve_env_hash` values look up DIFFERENT cache slots; a
    // prepopulated entry under one value is NOT visible from the
    // other.
    let host = fresh_host();
    upsert(&host, "/z.ts", "export const c = 3;");
    let content_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/z.ts")
        .expect("base content hash for ingested canonical");

    let parse_h = [0x07u8; 16];
    let resolve_h_before = [0xcc; 16];
    let resolve_h_after = [0xdd; 16];

    // Populate cache under the BEFORE resolve env.
    let key_before = key_with(
        "/z.ts",
        content_hash,
        parse_h,
        resolve_h_before,
        RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    );
    host.project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key_before, synthetic_facts());

    let view_after = HostView::with_env_hashes(
        Arc::clone(&host),
        env_hashes(parse_h, resolve_h_after, [0u8; 16]),
    );
    assert!(
        view_after.resolved_import_facts("/z.ts").is_none(),
        "a resolve_env_hash change (paths edit) MUST yield a cache miss in the AFTER env (R12)"
    );

    // And the BEFORE view still sees its entry.
    let view_before = HostView::with_env_hashes(
        Arc::clone(&host),
        env_hashes(parse_h, resolve_h_before, [0u8; 16]),
    );
    assert!(
        view_before.resolved_import_facts("/z.ts").is_some(),
        "the BEFORE view must still observe the cached payload under its own resolve env"
    );
}

#[test]
fn two_resolve_envs_coexist() {
    // R21 + R12: two distinct resolve envs admit two distinct
    // entries for the same `(canonical, content_hash,
    // parse_env_hash)` — verifying the cache stores both
    // candidates concurrently.
    let host = fresh_host();
    upsert(&host, "/w.ts", "export const d = 4;");
    let content_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/w.ts")
        .expect("base content hash");

    let parse_h = [0x33u8; 16];
    let resolve_h_a = [0x44u8; 16];
    let resolve_h_b = [0x55u8; 16];

    let key_a = key_with(
        "/w.ts",
        content_hash,
        parse_h,
        resolve_h_a,
        RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    );
    let key_b = key_with(
        "/w.ts",
        content_hash,
        parse_h,
        resolve_h_b,
        RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    );
    let payload_a = synthetic_facts();
    let payload_b = synthetic_facts();

    host.project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key_a, Arc::clone(&payload_a));
    host.project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key_b, Arc::clone(&payload_b));

    assert_eq!(
        host.project_type_store()
            .resolved_import_facts()
            .entry_count(),
        2,
        "two distinct resolve envs MUST coexist as two separate cache entries"
    );

    let view_a = HostView::with_env_hashes(
        Arc::clone(&host),
        env_hashes(parse_h, resolve_h_a, [0u8; 16]),
    );
    let view_b = HostView::with_env_hashes(
        Arc::clone(&host),
        env_hashes(parse_h, resolve_h_b, [0u8; 16]),
    );
    let observed_a = view_a
        .resolved_import_facts("/w.ts")
        .expect("view A reads its entry");
    let observed_b = view_b
        .resolved_import_facts("/w.ts")
        .expect("view B reads its entry");
    assert!(
        Arc::ptr_eq(&observed_a, &payload_a),
        "view A must see its OWN admitted payload"
    );
    assert!(
        Arc::ptr_eq(&observed_b, &payload_b),
        "view B must see its OWN admitted payload"
    );
    assert!(
        !Arc::ptr_eq(&observed_a, &observed_b),
        "the two payloads are distinct `Arc`s"
    );
}

// ---------------------------------------------------------------------------
// R5 / parser-flag / substrate isolation
// ---------------------------------------------------------------------------

#[test]
fn parse_env_hash_isolates_cache() {
    // A parser-flag change (e.g., TS-strict mode toggle, JSX mode
    // change) shifts `parse_env_hash`. Two views with different
    // `parse_env_hash` look up DIFFERENT cache slots.
    let host = fresh_host();
    upsert(&host, "/p.ts", "export const e = 5;");
    let content_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/p.ts")
        .expect("content hash");

    let parse_h_a = [0x10u8; 16];
    let parse_h_b = [0x20u8; 16];
    let resolve_h = [0x30u8; 16];

    let key_a = key_with(
        "/p.ts",
        content_hash,
        parse_h_a,
        resolve_h,
        RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    );
    host.project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key_a, synthetic_facts());

    let view_b = HostView::with_env_hashes(
        Arc::clone(&host),
        env_hashes(parse_h_b, resolve_h, [0u8; 16]),
    );
    assert!(
        view_b.resolved_import_facts("/p.ts").is_none(),
        "a parse_env_hash change must isolate the cache slot"
    );
}

#[test]
fn content_hash_isolates_cache() {
    // R5: two content hashes for the same canonical produce two
    // distinct cache slots — a source edit re-keys the entry.
    let host = fresh_host();
    upsert(&host, "/r.ts", "export const f = 6;");
    let content_hash_v1 = HostView::new(Arc::clone(&host))
        .content_hash_for("/r.ts")
        .expect("v1 content hash");

    let parse_h = [0x40u8; 16];
    let resolve_h = [0x50u8; 16];

    let key_v1 = key_with(
        "/r.ts",
        content_hash_v1,
        parse_h,
        resolve_h,
        RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    );
    host.project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key_v1, synthetic_facts());

    // Edit the source — content hash should change.
    upsert(&host, "/r.ts", "export const f = 7;");
    let content_hash_v2 = HostView::new(Arc::clone(&host))
        .content_hash_for("/r.ts")
        .expect("v2 content hash");
    assert_ne!(
        content_hash_v1, content_hash_v2,
        "edit MUST change the content hash — pre-condition"
    );

    let view =
        HostView::with_env_hashes(Arc::clone(&host), env_hashes(parse_h, resolve_h, [0u8; 16]));
    assert!(
        view.resolved_import_facts("/r.ts").is_none(),
        "a content_hash change MUST yield a cache miss (R5)"
    );
}

#[test]
fn resolver_version_isolates_cache() {
    // R28: `resolver_version` is a key dimension. A substrate bump
    // (older `resolver_version`) renders existing entries
    // unreachable through the documented accessor path.
    let host = fresh_host();
    upsert(&host, "/s.ts", "export const g = 7;");
    let content_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/s.ts")
        .expect("content hash");

    let parse_h = [0x60u8; 16];
    let resolve_h = [0x70u8; 16];

    // Plant an entry under a STALE resolver_version (production
    // never plants stale entries; this synthesises the invariant).
    let stale_version = RESOLVED_IMPORT_FACTS_RESOLVER_VERSION
        .checked_sub(1)
        .unwrap_or(0xDEAD_BEEF);
    let key_stale = key_with("/s.ts", content_hash, parse_h, resolve_h, stale_version);
    host.project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key_stale, synthetic_facts());

    let view =
        HostView::with_env_hashes(Arc::clone(&host), env_hashes(parse_h, resolve_h, [0u8; 16]));
    assert!(
        view.resolved_import_facts("/s.ts").is_none(),
        "the view reads only entries at the current resolver_version (R28); a stale entry MUST NOT be served"
    );
}

// ---------------------------------------------------------------------------
// First-writer-wins admission
// ---------------------------------------------------------------------------

#[test]
fn insert_if_absent_is_first_writer_wins() {
    // Two `insert_if_absent` calls on the same key: the first
    // returns `true`, the second returns `false`, and the original
    // `Arc` survives.
    let host = fresh_host();
    upsert(&host, "/a.ts", "export const h = 8;");
    let content_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/a.ts")
        .expect("content hash");

    let parse_h = [0x80u8; 16];
    let resolve_h = [0x90u8; 16];

    let key = key_with(
        "/a.ts",
        content_hash,
        parse_h,
        resolve_h,
        RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    );

    let payload_first = synthetic_facts();
    let payload_second = synthetic_facts();
    assert!(
        !Arc::ptr_eq(&payload_first, &payload_second),
        "the two payloads start as distinct `Arc`s — pre-condition"
    );

    let admitted_first = host
        .project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key.clone(), Arc::clone(&payload_first));
    assert!(admitted_first, "the first writer wins");

    let admitted_second = host
        .project_type_store()
        .resolved_import_facts()
        .insert_if_absent(key.clone(), Arc::clone(&payload_second));
    assert!(
        !admitted_second,
        "the second writer must lose; admission returns false"
    );

    let view =
        HostView::with_env_hashes(Arc::clone(&host), env_hashes(parse_h, resolve_h, [0u8; 16]));
    let observed = view
        .resolved_import_facts("/a.ts")
        .expect("the entry is admitted under this view's env");
    assert!(
        Arc::ptr_eq(&observed, &payload_first),
        "the cache MUST preserve the first admitted `Arc` (first-writer-wins)"
    );
    assert!(
        !Arc::ptr_eq(&observed, &payload_second),
        "the second writer's `Arc` MUST NOT have replaced the first"
    );
}

// ---------------------------------------------------------------------------
// `_unused` test to anchor unused imports as the suite grows.
// ---------------------------------------------------------------------------

#[test]
fn _construct_payload_compiles() {
    // Smoke test that the payload types remain constructible from
    // outside the crate via the documented accessor surface. Not a
    // discriminator — just a guardrail against accidental
    // pub-visibility regressions.
    let payload = ResolvedImportFacts::new();
    assert!(payload.import_clauses.is_empty());
    assert!(payload.reexport_bindings.is_empty());
    assert!(payload.specifier_resolutions.is_empty());

    // Construct the unused-import sentinel so the `FxHashMap`
    // import is not dropped by future maintenance edits.
    let _: FxHashMap<String, Arc<str>> = FxHashMap::default();
}
