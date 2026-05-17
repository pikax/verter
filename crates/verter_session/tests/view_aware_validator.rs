//! Stage 4c discriminating test — `HostFenceValidator` is view-aware.
//!
//! Binds **R17** (sessions are views) and **R19** (fact validation is
//! the cache-correctness oracle, orthogonal to the concurrency oracle).
//!
//! The test mounts two concurrent overlaid views over the SAME base
//! host. Each overlay carries a different source under the same
//! canonical id. A `HostFenceValidator` bound to one overlay's view
//! validates against THAT overlay's content hash — not the other
//! overlay's, and not the base host's. Pre-Stage-4c
//! `HostFenceValidator` read directly from the host's
//! `shallow_file_state` and would have returned the base content
//! hash for both validators, giving identical answers regardless of
//! the view supplied (the bug Stage 4c fixes).
//!
//! Discriminating property: this test FAILS on the pre-Stage-4c
//! tree (validator ignores the view; both validators report the
//! same content hash) and PASSES on the post-Stage-4c tree
//! (validator routes the WholeHash arm through the view's
//! content-hash accessor first).

use std::collections::HashSet;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use verter_session::host_manage::HostFenceValidator;
use verter_session::semantic_query::DepVersion;
use verter_session::session_view::{HostView, OverlaidView, OverlaidViewRef, SessionView};
use verter_session::{CompileErrorPolicy, FileKind, Hash16, HostConfig, UpsertRequest, VerterHost};

fn host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }))
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
    // Force `FileArtifactStore` to materialise the canonical so the
    // view's content-hash accessor returns `Some(...)` from the
    // start of the test (the validator does not trigger
    // materialisation itself).
    let _ = host.evaluate_types(canonical);
}

fn overlay_with(host: Arc<VerterHost>, canonical: &str, source: &str) -> OverlaidView {
    let mut map: FxHashMap<String, Arc<str>> = FxHashMap::default();
    map.insert(canonical.to_string(), Arc::from(source));
    OverlaidView::new(host, map)
}

#[test]
fn two_concurrent_overlays_validate_independently_under_view_aware_validator() {
    // Single base host with one canonical at content A.
    let host = host();
    upsert(&host, "/x.ts", "export const a = 1;");

    // Two overlays diverging from the base AND from each other.
    let overlay_a = overlay_with(Arc::clone(&host), "/x.ts", "export const a = 100;");
    let overlay_b = overlay_with(Arc::clone(&host), "/x.ts", "export const a = 200;");

    let hash_a = overlay_a.content_hash_for("/x.ts").expect("overlay A hash");
    let hash_b = overlay_b.content_hash_for("/x.ts").expect("overlay B hash");
    assert_ne!(
        hash_a, hash_b,
        "overlays carry distinct sources → distinct content hashes"
    );

    // Validator bound to overlay A.
    let validator_a = HostFenceValidator {
        host: &host,
        view: &overlay_a,
    };

    // Validator A MUST accept overlay A's content hash and REJECT
    // overlay B's. Pre-Stage-4c the validator would have consulted
    // the host's `shallow_file_state` for canonical `/x.ts` (which
    // is the BASE content hash, not overlay A's), so it would have
    // accepted neither hash and the second assertion below would
    // have spuriously held — but the FIRST assertion would have
    // failed: overlay A's hash differs from the base hash, so a
    // pre-Stage-4c validator would reject overlay A's hash for
    // overlay A's validator. That is the failing condition this
    // test detects.
    assert!(
        validator_a.validate_dep_fact("/x.ts", &DepVersion::WholeHash(hash_a)),
        "validator bound to overlay A MUST accept overlay A's content hash \
         (R17 — sessions are views; the validator routes WholeHash through the view)"
    );
    assert!(
        !validator_a.validate_dep_fact("/x.ts", &DepVersion::WholeHash(hash_b)),
        "validator bound to overlay A MUST reject overlay B's content hash"
    );

    // Now flip and confirm symmetry.
    let validator_b = HostFenceValidator {
        host: &host,
        view: &overlay_b,
    };

    assert!(
        validator_b.validate_dep_fact("/x.ts", &DepVersion::WholeHash(hash_b)),
        "validator bound to overlay B MUST accept overlay B's content hash"
    );
    assert!(
        !validator_b.validate_dep_fact("/x.ts", &DepVersion::WholeHash(hash_a)),
        "validator bound to overlay B MUST reject overlay A's content hash"
    );
}

#[test]
fn host_view_validator_matches_base_host_content_hash() {
    // Sanity companion test — an overlay-free `HostView` validator
    // observes the same content hash as the base host. This locks
    // in the property that overlay-free queries continue to validate
    // correctly under the view-aware path.
    let host = host();
    upsert(&host, "/x.ts", "export const a = 1;");

    let host_view = HostView::new(Arc::clone(&host));
    let base_hash = host_view
        .content_hash_for("/x.ts")
        .expect("base hash present");

    let validator = HostFenceValidator {
        host: &host,
        view: &host_view,
    };

    assert!(
        validator.validate_dep_fact("/x.ts", &DepVersion::WholeHash(base_hash)),
        "HostView validator MUST accept the host's content hash"
    );

    // Reject a synthetic non-matching hash.
    let bogus: Hash16 = [0xffu8; 16];
    assert_ne!(base_hash, bogus);
    assert!(
        !validator.validate_dep_fact("/x.ts", &DepVersion::WholeHash(bogus)),
        "HostView validator MUST reject a non-matching hash"
    );
}

#[test]
fn validator_falls_through_to_host_when_view_misses() {
    // The view-aware path falls through to the host's
    // `shallow_file_state` when the view returns `None`. This
    // covers canonicals not yet ingested under the view (e.g.,
    // freshly-upserted files that haven't been indexed yet).
    let host = host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/y.ts".to_string(),
            source: Arc::from("export const a = 1;"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert");
    // Deliberately DO NOT call `evaluate_types(...)` — the view's
    // `content_hash_for` will return `None` because
    // `FileArtifactStore` is populated lazily. The validator must
    // fall through to `host.shallow_file_state`, which has its own
    // route-owned-shallow fallback for unmaterialised canonicals.
    //
    // The exact validation result here depends on the host's
    // synchronous-upsert behaviour (canonical may or may not have
    // a shallow state on first call) — the assertion is that the
    // call does NOT panic and produces a deterministic boolean,
    // and that supplying a bogus hash always fails.
    let view = HostView::new(Arc::clone(&host));
    let validator = HostFenceValidator {
        host: &host,
        view: &view,
    };

    let bogus: Hash16 = [0xffu8; 16];
    assert!(
        !validator.validate_dep_fact("/y.ts", &DepVersion::WholeHash(bogus)),
        "validator MUST reject a bogus hash even on the host-fallback path"
    );
}

#[test]
fn validator_rejects_session_tombstoned_canonical_against_base_host() {
    // Codex P2 #2 — the legacy dep-signature rail must NOT validate a
    // `WholeHash` dependency on a file the session has deleted.
    //
    // A session delete is an overlay: `MetaSession::delete` records a
    // `SessionOverlay::Delete` and never mutates the base host. So the
    // base host's `shallow_file_state` keeps reporting the pre-delete
    // content hash. For a tombstoned canonical the view's
    // `content_hash_for` returns `None`; pre-fix the `HostFenceValidator`
    // `WholeHash` arm then fell THROUGH to `host.shallow_file_state`,
    // validating a `WholeHash` dependency against a file the session
    // deleted — any legacy dep-signature cache entry under that session
    // could reuse base results past the delete.
    //
    // Post-fix the `WholeHash` arm rejects a tombstoned canonical before
    // the base-host fallback.
    //
    // Discriminating property: this test FAILS on the pre-fix tree (the
    // validator falls through and ACCEPTS the base content hash for the
    // deleted file) and PASSES post-fix (the validator rejects the
    // tombstoned canonical regardless of what the base host reports).
    let host = host();
    upsert(&host, "/x.ts", "export const a = 1;");

    // The base host's content hash for `/x.ts` — still live on the
    // base host even after the session deletes the file.
    let base_view = HostView::new(Arc::clone(&host));
    let base_hash = base_view
        .content_hash_for("/x.ts")
        .expect("base content hash for the live file");

    // `OverlaidViewRef` with an EMPTY overlay-source map and a single
    // tombstone for `/x.ts` — exactly the shape
    // `MetaSession::with_overlay_view` builds for a `SessionOverlay::Delete`.
    let overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    let overlay_hashes: FxHashMap<String, Hash16> = FxHashMap::default();
    let mut tombstones: HashSet<String> = HashSet::new();
    tombstones.insert("/x.ts".to_string());
    let tombstone_view = OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &tombstones);
    assert!(
        tombstone_view.is_tombstoned("/x.ts"),
        "fixture invariant: the view tombstones /x.ts"
    );
    assert_eq!(
        tombstone_view.content_hash_for("/x.ts"),
        None,
        "fixture invariant: a tombstoned canonical reports no content hash — \
         this is what makes the pre-fix validator fall through to the base host"
    );

    let validator = HostFenceValidator {
        host: &host,
        view: &tombstone_view,
    };

    // The validator MUST reject the base content hash for the deleted
    // file. Pre-fix it would ACCEPT it: `content_hash_for` returns
    // `None`, the arm falls through to `host.shallow_file_state("/x.ts")`
    // (which still reports `base_hash` — the base host was never
    // mutated), and `state.whole_hash == base_hash` holds.
    assert!(
        !validator.validate_dep_fact("/x.ts", &DepVersion::WholeHash(base_hash)),
        "the legacy dep-signature rail MUST reject a `WholeHash` dependency on \
         a session-tombstoned canonical — the file is deleted in this session. \
         Validating it against the base host's `shallow_file_state` would let a \
         legacy cache entry reuse base results past the delete (codex P2 #2)."
    );

    // A non-tombstoned canonical under the same view still validates
    // against the base host — proves the rejection is scoped to the
    // tombstoned canonical, not a blanket reject under a delete-bearing
    // session.
    upsert(&host, "/kept.ts", "export const b = 2;");
    let kept_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/kept.ts")
        .expect("base content hash for the kept file");
    assert!(
        validator.validate_dep_fact("/kept.ts", &DepVersion::WholeHash(kept_hash)),
        "a canonical the session did NOT delete MUST still validate against the \
         base host — the tombstone rejection is per-canonical, not session-wide"
    );
}
