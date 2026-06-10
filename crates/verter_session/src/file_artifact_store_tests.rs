//! `FileArtifactStore` unit tests.

use std::sync::Arc;

use verter_semantic::analysis::Hash16;

use super::{
    AugmentationTargetKey, AugmentationTargetKind, FileArtifactKey, FileArtifactStore,
    FileArtifacts, ProjectIdentity,
};
use crate::project_type_store::IndexedReady;

fn synth_indexed(hash: u8) -> Arc<IndexedReady> {
    Arc::new(IndexedReady::new_for_test([hash; 16]))
}

fn synth_artifacts(hash: u8) -> Arc<FileArtifacts> {
    Arc::new(FileArtifacts::with_indexed(synth_indexed(hash)))
}

fn synth_key(canonical: &str, content_hash: Hash16, parse_env_hash: Hash16) -> FileArtifactKey {
    FileArtifactKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash,
        parser_version: 1,
        file_language_id: FileArtifactKey::derived_file_language_id(canonical),
    }
}

/// D-r per-file invalidation column: keys that differ ONLY in
/// `file_language_id` occupy DISTINCT artifact slots. Inert for static
/// rows (the column is extension-derived, so one file always builds the
/// same key today); load-bearing the moment a host-gated classification
/// row can flip a file's resolved language — the flip misses exactly
/// that file's slots, with no global env-hash invalidation.
#[test]
fn distinct_file_language_id_values_occupy_distinct_slots() {
    use verter_language::{FileLanguage, ScriptSourceType};

    let store = FileArtifactStore::new();
    let base = synth_key("/widget.html", [3u8; 16], [4u8; 16]);
    assert_eq!(
        base.file_language_id,
        FileLanguage::script(ScriptSourceType::Ts),
        "an unregistered extension derives the plain-script fallthrough"
    );
    let gated_flip = FileArtifactKey {
        file_language_id: FileLanguage::FrameworkTemplate {
            adapter_id: verter_language::FrameworkAdapterId::new("fixture-framework"),
            owner_hint: None,
        },
        ..base.clone()
    };

    let script_payload = synth_artifacts(0x11);
    store.insert_artifacts(base.clone(), Arc::clone(&script_payload));

    // The language-flipped key misses the script slot...
    assert!(
        store.get_artifacts(&gated_flip).is_none(),
        "a file_language_id flip MUST miss the other language's slot"
    );

    // ...and owns its own slot alongside it.
    let template_payload = synth_artifacts(0x22);
    store.insert_artifacts(gated_flip.clone(), Arc::clone(&template_payload));
    assert!(
        Arc::ptr_eq(
            &store.get_artifacts(&base).expect("script slot intact"),
            &script_payload
        ),
        "the original slot must survive the flipped insert"
    );
    assert!(
        Arc::ptr_eq(
            &store.get_artifacts(&gated_flip).expect("template slot"),
            &template_payload
        ),
        "the flipped key must read back its own payload"
    );
}

#[test]
fn empty_store_returns_none() {
    let store = FileArtifactStore::new();
    let key = synth_key("/a.ts", [0u8; 16], [1u8; 16]);
    assert!(store.get_artifacts(&key).is_none());
    assert_eq!(store.len(), 0);
    assert!(store.is_empty());
}

#[test]
fn insert_then_get_returns_payload() {
    let store = FileArtifactStore::new();
    let key = synth_key("/a.ts", [1u8; 16], [2u8; 16]);
    let payload = synth_artifacts(0xaa);
    store.insert_artifacts(key.clone(), Arc::clone(&payload));
    let got = store.get_artifacts(&key).expect("entry MUST exist");
    assert!(Arc::ptr_eq(&got, &payload), "MUST return the inserted Arc");
    assert_eq!(store.len(), 1);
}

#[test]
fn two_content_hashes_for_same_canonical_coexist() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/a.ts", [2u8; 16], [10u8; 16]);
    store.insert_artifacts(key_a.clone(), synth_artifacts(0xaa));
    store.insert_artifacts(key_b.clone(), synth_artifacts(0xbb));
    assert!(store.get_artifacts(&key_a).is_some());
    assert!(store.get_artifacts(&key_b).is_some());
    assert_eq!(
        store.len(),
        2,
        "two content hashes MUST coexist under same canonical"
    );
}

#[test]
fn two_parse_envs_for_same_canonical_coexist() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [9u8; 16], [10u8; 16]);
    let key_b = synth_key("/a.ts", [9u8; 16], [11u8; 16]);
    store.insert_artifacts(key_a.clone(), synth_artifacts(0xaa));
    store.insert_artifacts(key_b.clone(), synth_artifacts(0xbb));
    assert_eq!(
        store.len(),
        2,
        "two parse envs MUST coexist under same (canonical, content_hash)"
    );
    assert!(store.get_artifacts(&key_a).is_some());
    assert!(store.get_artifacts(&key_b).is_some());
}

#[test]
fn remove_artifacts_returns_previous_entry() {
    let store = FileArtifactStore::new();
    let key = synth_key("/a.ts", [0u8; 16], [1u8; 16]);
    store.insert_artifacts(key.clone(), synth_artifacts(0xcc));
    let removed = store.remove_artifacts(&key);
    assert!(removed.is_some(), "remove MUST return prior entry");
    assert!(
        store.get_artifacts(&key).is_none(),
        "post-remove get MUST be None"
    );
}

#[test]
fn remove_canonical_drops_every_version() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/a.ts", [2u8; 16], [10u8; 16]);
    let key_other = synth_key("/b.ts", [1u8; 16], [10u8; 16]);
    store.insert_artifacts(key_a, synth_artifacts(0xaa));
    store.insert_artifacts(key_b, synth_artifacts(0xbb));
    store.insert_artifacts(key_other.clone(), synth_artifacts(0xcc));
    let removed = store.remove_canonical("/a.ts");
    assert_eq!(removed, 2, "MUST drop both versions of /a.ts");
    assert!(
        store.get_artifacts(&key_other).is_some(),
        "MUST NOT touch /b.ts"
    );
}

#[test]
fn get_artifacts_any_returns_some_entry_for_canonical() {
    let store = FileArtifactStore::new();
    // `get_artifacts_any` is a base canonical-wide scan — it surfaces
    // only `legacy`-key (base) artifacts, never overlay-scoped ones.
    let key = FileArtifactKey::legacy(Arc::from("/a.ts"), [9u8; 16]);
    store.insert_artifacts(key, synth_artifacts(0xaa));
    assert!(store.get_artifacts_any("/a.ts").is_some());
    assert!(store.get_artifacts_any("/nonexistent.ts").is_none());
}

#[test]
fn augmentation_index_starts_empty() {
    let store = FileArtifactStore::new();
    assert_eq!(store.augmentation_index_len(), 0);
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([0u8; 16]),
        resolve_env_hash: [0u8; 16],
        lib_env_hash: [0u8; 16],
        population: crate::file_artifact_store::AugmentationPopulation::Base,
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    assert!(store.get_augmenter_set(&key).is_none());
}

#[test]
fn augmentation_index_round_trip() {
    use smallvec::smallvec;

    use super::{AugmenterEntry, AugmenterSet, FileArtifactKey};

    let store = FileArtifactStore::new();
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([42u8; 16]),
        resolve_env_hash: [1u8; 16],
        lib_env_hash: [2u8; 16],
        population: crate::file_artifact_store::AugmentationPopulation::Base,
        target: AugmentationTargetKind::ExternalSpecifier(super::InternedSpecifier::from("vue")),
    };
    let set = Arc::new(AugmenterSet {
        entries: smallvec![AugmenterEntry {
            artifact_key: FileArtifactKey::legacy(Arc::from("/aug.ts"), [9u8; 16]),
            parse_stable_hash: [3u8; 16],
        }],
        fingerprint: [4u8; 16],
    });
    store.populate_augmenter_set(key.clone(), Arc::clone(&set));
    let got = store.get_augmenter_set(&key).expect("MUST round-trip");
    assert!(Arc::ptr_eq(&got, &set));
    assert_eq!(store.augmentation_index_len(), 1);
}

#[test]
fn snapshot_artifacts_observes_every_entry() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/b.ts", [1u8; 16], [10u8; 16]);
    store.insert_artifacts(key_a, synth_artifacts(0xaa));
    store.insert_artifacts(key_b, synth_artifacts(0xbb));
    let snap = store.snapshot_artifacts();
    assert_eq!(snap.len(), 2);
}

#[test]
fn artifact_keys_returns_every_key() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/b.ts", [1u8; 16], [10u8; 16]);
    store.insert_artifacts(key_a.clone(), synth_artifacts(0xaa));
    store.insert_artifacts(key_b.clone(), synth_artifacts(0xbb));
    let keys = store.artifact_keys();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&key_a));
    assert!(keys.contains(&key_b));
}

// ── Legacy API smoke ──

#[test]
fn legacy_insert_get_round_trip() {
    let store = FileArtifactStore::new();
    let canonical: Arc<str> = Arc::from("/legacy.ts");
    let indexed = Arc::new(IndexedReady::new_for_test([7u8; 16]));
    store.insert(Arc::clone(&canonical), Arc::clone(&indexed));
    let got = store.get("/legacy.ts", [7u8; 16]).expect("MUST hit");
    assert!(Arc::ptr_eq(&got, &indexed));
    assert_eq!(store.len(), 1);
    // get_any without hash lookup also succeeds.
    let any = store.get_any("/legacy.ts").expect("MUST hit");
    assert!(Arc::ptr_eq(&any, &indexed));
}

#[test]
fn legacy_remove_drops_entry() {
    let store = FileArtifactStore::new();
    let canonical: Arc<str> = Arc::from("/legacy.ts");
    let indexed = Arc::new(IndexedReady::new_for_test([7u8; 16]));
    store.insert(Arc::clone(&canonical), indexed);
    store.remove("/legacy.ts");
    assert!(store.get("/legacy.ts", [7u8; 16]).is_none());
    assert!(store.is_empty());
}

// ── Overlay-scoped isolation from base canonical-wide scans ──
//
// [`FileArtifactKey::overlay_scoped`] keys a session-view overlay
// artifact distinctly from the base artifact (the discriminator lives
// in the `parse_env_hash` dimension). Exact-key lookups (`get` /
// `get_overlay_scoped` / `get_artifacts`) are isolated by the key.
// The canonical-wide *scans* (`get_any`, `get_artifacts_any`,
// `snapshot_all`) match by `canonical` only — and must NOT surface an
// overlay-scoped artifact to a base reader, which would then derive
// base cache keys / route facts from session-specific import routes.
// These tests pin that isolation invariant: a base canonical-wide
// scan must NEVER surface an overlay-scoped artifact.

/// Stable non-zero overlay discriminator for the isolation tests.
/// Mirrors the `parse_env_hash` shape `FileArtifactKey::overlay_scoped`
/// builds from a session view's overlay-set fingerprint — non-zero so
/// it can never alias [`super::LEGACY_PARSE_ENV_HASH`].
fn overlay_discriminator_for_test() -> Hash16 {
    [
        b'v', b'o', b'v', b'l', b'-', b'a', b'r', b't', 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
        0x88,
    ]
}

#[test]
fn base_canonical_wide_scans_do_not_surface_overlay_only_artifact() {
    // Discrimination property: a session installs an overlay for
    // canonical X and the overlay artifact is published under its
    // `overlay_scoped` key — and NO base (`legacy`-key) artifact
    // exists for X. A *base* scan for X must therefore return `None`
    // (a base reader sees no base artifact), NEVER the overlay-scoped
    // artifact.
    //
    // Discrimination: a scan that matched `canonical` only would surface
    // the overlay-scoped entry — the sole entry for X — to the base reader.
    // The base canonical-wide scans filter to `legacy` keys, so they
    // return `None` / omit X and never leak the overlay artifact to a base
    // reader.
    let store = FileArtifactStore::new();
    let content_hash = [0x5au8; 16];
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/overlay-only.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(overlay_key.clone(), synth_artifacts(0x5a));

    // A base canonical-wide scan must NOT surface the overlay artifact.
    assert!(
        store.get_any("/overlay-only.ts").is_none(),
        "get_any (base scan) MUST NOT surface an overlay-scoped artifact"
    );
    assert!(
        store.get_artifacts_any("/overlay-only.ts").is_none(),
        "get_artifacts_any (base scan) MUST NOT surface an overlay-scoped artifact"
    );
    assert!(
        store
            .snapshot_all()
            .iter()
            .all(|(canonical, _)| canonical.as_ref() != "/overlay-only.ts"),
        "snapshot_all (base scan) MUST NOT include an overlay-scoped artifact"
    );

    // Inverse: the view-aware exact-key accessor still reaches it.
    assert!(
        store
            .get_overlay_scoped(
                "/overlay-only.ts",
                content_hash,
                overlay_discriminator_for_test()
            )
            .is_some(),
        "get_overlay_scoped (view-aware accessor) MUST still reach the overlay artifact"
    );
}

#[test]
fn base_canonical_wide_scans_return_base_artifact_when_base_and_overlay_coexist() {
    // Discrimination property: a base artifact and an overlay-scoped
    // artifact for the SAME canonical + content hash coexist (the
    // byte-identical-overlay case — the common LSP case). A base scan
    // MUST return exactly the base artifact, never the overlay one.
    //
    // Discrimination: a scan that matched `canonical` only would let
    // DashMap iteration order decide which of the two entries is surfaced
    // — the overlay artifact could win. The scan filters to the `legacy`
    // key and deterministically returns the base artifact.
    let store = FileArtifactStore::new();
    let content_hash = [0x77u8; 16];
    let base_indexed = synth_indexed(0xb0);
    let base_key = FileArtifactKey::legacy(Arc::from("/shared.ts"), content_hash);
    store.insert_artifacts(
        base_key,
        Arc::new(FileArtifacts::with_indexed(Arc::clone(&base_indexed))),
    );
    let overlay_indexed = synth_indexed(0x0e);
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/shared.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(
        overlay_key,
        Arc::new(FileArtifacts::with_indexed(Arc::clone(&overlay_indexed))),
    );

    let any = store
        .get_any("/shared.ts")
        .expect("get_any MUST hit the base artifact");
    assert!(
        Arc::ptr_eq(&any, &base_indexed),
        "get_any MUST return the base artifact, never the overlay-scoped sibling"
    );
    let any_artifacts = store
        .get_artifacts_any("/shared.ts")
        .expect("get_artifacts_any MUST hit the base artifact");
    assert!(
        Arc::ptr_eq(&any_artifacts.indexed, &base_indexed),
        "get_artifacts_any MUST return the base artifact, never the overlay-scoped sibling"
    );
    let snap = store.snapshot_all();
    let shared_entries: Vec<&Arc<IndexedReady>> = snap
        .iter()
        .filter(|(canonical, _)| canonical.as_ref() == "/shared.ts")
        .map(|(_, indexed)| indexed)
        .collect();
    assert_eq!(
        shared_entries.len(),
        1,
        "snapshot_all MUST surface exactly one entry for the canonical (the base)"
    );
    assert!(
        Arc::ptr_eq(shared_entries[0], &base_indexed),
        "snapshot_all MUST surface the base artifact, never the overlay-scoped sibling"
    );

    // The overlay-scoped exact-key read still reaches the overlay.
    let overlay_hit = store
        .get_overlay_scoped("/shared.ts", content_hash, overlay_discriminator_for_test())
        .expect("get_overlay_scoped MUST still reach the overlay artifact");
    assert!(
        Arc::ptr_eq(&overlay_hit, &overlay_indexed),
        "get_overlay_scoped MUST reach the overlay artifact"
    );
}

#[test]
fn get_artifacts_for_content_stays_view_independent_across_base_and_overlay() {
    // `get_artifacts_for_content` is content-addressed and
    // view-independent BY DESIGN: its sole consumer
    // (`parse_fact_ref_for_observed_current_content`) reads the
    // parse-domain `FileFacts` registry, which is derived purely from
    // the source bytes — identical across a base artifact and a
    // byte-identical overlay artifact at the same content hash. It
    // MUST therefore still resolve a `FileArtifacts` for a content
    // version that exists ONLY as an overlay-scoped artifact, so a
    // parse fact can still be recovered.
    let store = FileArtifactStore::new();
    let content_hash = [0x3cu8; 16];
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/overlay-only.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(overlay_key, synth_artifacts(0x3c));
    assert!(
        store
            .get_artifacts_for_content("/overlay-only.ts", content_hash)
            .is_some(),
        "get_artifacts_for_content MUST stay content-addressed / view-independent \
         so parse-fact recovery works for an overlay-only content version"
    );
    // A mismatched content hash still misses (content-pinned).
    assert!(
        store
            .get_artifacts_for_content("/overlay-only.ts", [0x00u8; 16])
            .is_none(),
        "get_artifacts_for_content MUST stay content-pinned"
    );
}

#[test]
fn remove_canonical_drains_overlay_scoped_keys() {
    // A removal / eviction scan MUST keep draining ALL of a
    // canonical's keys — base AND overlay-scoped — so an eviction
    // never leaves a stale overlay artifact behind. `remove_canonical`
    // is a lifecycle scan, NOT a base-read scan: it stays unfiltered.
    let store = FileArtifactStore::new();
    let content_hash = [0x9bu8; 16];
    let base_key = FileArtifactKey::legacy(Arc::from("/evict-me.ts"), content_hash);
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/evict-me.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(base_key.clone(), synth_artifacts(0xb0));
    store.insert_artifacts(overlay_key.clone(), synth_artifacts(0x0e));
    assert_eq!(
        store.len(),
        2,
        "both base + overlay entries MUST be present"
    );

    let removed = store.remove_canonical("/evict-me.ts");
    assert_eq!(
        removed, 2,
        "remove_canonical MUST drain BOTH the base and overlay-scoped keys"
    );
    assert_eq!(store.len(), 0, "no entry MUST survive the eviction");
    assert!(
        store.get_artifacts(&overlay_key).is_none(),
        "the overlay-scoped artifact MUST NOT survive remove_canonical"
    );
    assert!(
        store.get_artifacts(&base_key).is_none(),
        "the base artifact MUST NOT survive remove_canonical"
    );
}

#[test]
fn legacy_remove_drains_overlay_scoped_keys() {
    // `remove` (the legacy per-canonical removal) is likewise a
    // lifecycle scan and MUST drain overlay-scoped keys too.
    let store = FileArtifactStore::new();
    let content_hash = [0xa5u8; 16];
    let base_key = FileArtifactKey::legacy(Arc::from("/drop-me.ts"), content_hash);
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/drop-me.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(base_key.clone(), synth_artifacts(0xb0));
    store.insert_artifacts(overlay_key.clone(), synth_artifacts(0x0e));

    store.remove("/drop-me.ts");
    assert!(
        store.is_empty(),
        "remove MUST drain base + overlay-scoped keys"
    );
    assert!(store.get_artifacts(&overlay_key).is_none());
    assert!(store.get_artifacts(&base_key).is_none());
}

// ── F1: bump-iff-actually-changed for the base-folded `artifact_generation` ──
//
// `artifact_generation` is folded into every base `StoreViewValidationToken`.
// It MUST advance on every artifact insert/replace/populate that changes a
// base-visible snapshot value (NO under-bump — under-bumping reintroduces
// stale-base-view reads), but it MUST NOT churn on a true no-op (a re-insert
// of byte-identical content) or on an overlay-scoped re-insert that does not
// alter any base snapshot (that spuriously invalidates the manager-cached
// base view and splits singleflight lanes). Each test below discriminates
// against an `insert_artifacts` / `insert` / `populate_augmenter_set` that
// bumped the generation unconditionally.

#[test]
fn artifact_generation_does_not_bump_on_noop_replace_of_legacy_key() {
    // SAFETY ARM B (no over-bump): re-inserting byte-identical content under
    // the SAME content-addressed key is a no-op for every base snapshot
    // dimension, so the base-folded generation MUST stay put. This
    // discriminates against an implementation that bumped unconditionally
    // on the replace.
    let store = FileArtifactStore::new();
    let key = FileArtifactKey::legacy(Arc::from("/noop.ts"), [0x42u8; 16]);
    store.insert_artifacts(key.clone(), synth_artifacts(0x42));
    let after_first = store.artifact_generation();
    // Distinct `Arc`, identical content (same whole_hash / surface / facts).
    store.insert_artifacts(key, synth_artifacts(0x42));
    assert_eq!(
        after_first,
        store.artifact_generation(),
        "a byte-identical re-insert MUST NOT advance artifact_generation \
         (no spurious base-view invalidation)"
    );
}

#[test]
fn artifact_generation_bumps_on_base_visible_change_of_legacy_key() {
    // SOUNDNESS ARM A (no under-bump — the mandatory arm): replacing a
    // legacy key's value with one whose whole_hash (a base-visible snapshot
    // dimension) differs MUST advance the generation, or a manager-cached
    // base view would go stale and warm-hit validation would false-MISS.
    let store = FileArtifactStore::new();
    let key = FileArtifactKey::legacy(Arc::from("/changed.ts"), [0x42u8; 16]);
    store.insert_artifacts(key.clone(), synth_artifacts(0x42));
    let after_first = store.artifact_generation();
    // Same key, DIFFERENT content → different whole_hash → base-visible.
    store.insert_artifacts(key, synth_artifacts(0x99));
    assert_ne!(
        after_first,
        store.artifact_generation(),
        "a base-visible artifact change MUST advance artifact_generation \
         (no under-bump — else a manager-cached base view goes stale)"
    );
}

#[test]
fn artifact_generation_bumps_on_fresh_insert() {
    // A fresh insert moves a canonical's base snapshot from absent → present,
    // which is always a base-visible change and MUST bump.
    let store = FileArtifactStore::new();
    let before = store.artifact_generation();
    let key = FileArtifactKey::legacy(Arc::from("/fresh.ts"), [0x11u8; 16]);
    store.insert_artifacts(key, synth_artifacts(0x11));
    assert_ne!(
        before,
        store.artifact_generation(),
        "a fresh artifact insert MUST advance artifact_generation"
    );
}

#[test]
fn artifact_generation_does_not_bump_on_noop_overlay_reinsert() {
    // Overlay-only no-op: a base (`legacy`-key) artifact is present; an
    // overlay-scoped artifact is re-inserted byte-identical. `snapshot_all()`
    // filters to legacy keys, and the re-insert changes nothing base-visible,
    // so the base-folded generation MUST stay put. This discriminates against
    // an implementation where the overlay re-insert bumped the SINGLE global
    // generation folded into every BASE token, churning unrelated base store
    // views.
    let store = FileArtifactStore::new();
    let content_hash = [0x7eu8; 16];
    // Base artifact under a DIFFERENT content so the overlay never aliases
    // the base `file_facts` slot (whole_hashes[canonical] != overlay hash).
    store.insert_artifacts(
        FileArtifactKey::legacy(Arc::from("/ov.ts"), [0x01u8; 16]),
        synth_artifacts(0x01),
    );
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/ov.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    // Seed the overlay artifact once (fresh insert — allowed to bump).
    store.insert_artifacts(overlay_key.clone(), synth_artifacts(0x7e));
    let after_seed = store.artifact_generation();
    // Re-insert the SAME overlay value byte-identically → no-op.
    store.insert_artifacts(overlay_key, synth_artifacts(0x7e));
    assert_eq!(
        after_seed,
        store.artifact_generation(),
        "a byte-identical overlay-scoped re-insert MUST NOT advance the \
         base-folded artifact_generation"
    );
}

#[test]
fn legacy_insert_does_not_bump_on_noop_replace() {
    // The legacy `insert` surface (drain-then-insert) must also be
    // bump-iff-changed: re-inserting the same `IndexedReady` content is a
    // no-op for the base snapshot. This discriminates against an
    // implementation that bumped unconditionally.
    let store = FileArtifactStore::new();
    let canonical: Arc<str> = Arc::from("/legacy-noop.ts");
    store.insert(Arc::clone(&canonical), synth_indexed(0x33));
    let after_first = store.artifact_generation();
    store.insert(canonical, synth_indexed(0x33));
    assert_eq!(
        after_first,
        store.artifact_generation(),
        "a no-op legacy re-insert MUST NOT advance artifact_generation"
    );
}

#[test]
fn legacy_insert_bumps_on_content_change() {
    // No-under-bump arm for the legacy surface: a content change MUST bump.
    let store = FileArtifactStore::new();
    let canonical: Arc<str> = Arc::from("/legacy-change.ts");
    store.insert(Arc::clone(&canonical), synth_indexed(0x33));
    let after_first = store.artifact_generation();
    store.insert(canonical, synth_indexed(0x44));
    assert_ne!(
        after_first,
        store.artifact_generation(),
        "a legacy content change MUST advance artifact_generation (no under-bump)"
    );
}

#[test]
fn populate_augmenter_set_is_bump_iff_fingerprint_changed() {
    use smallvec::smallvec;

    use super::{AugmenterEntry, AugmenterSet};

    let store = FileArtifactStore::new();
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([7u8; 16]),
        resolve_env_hash: [1u8; 16],
        lib_env_hash: [2u8; 16],
        population: crate::file_artifact_store::AugmentationPopulation::Base,
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    let make_set = |fingerprint: Hash16| {
        Arc::new(AugmenterSet {
            entries: smallvec![AugmenterEntry {
                artifact_key: FileArtifactKey::legacy(Arc::from("/aug.ts"), [9u8; 16]),
                parse_stable_hash: [3u8; 16],
            }],
            fingerprint,
        })
    };
    // Fresh populate (absent → present) bumps.
    let before = store.artifact_generation();
    store.populate_augmenter_set(key.clone(), make_set([4u8; 16]));
    assert_ne!(
        before,
        store.artifact_generation(),
        "a fresh augmenter populate MUST bump"
    );
    // Re-populate identical fingerprint → no-op, MUST NOT bump (discriminates
    // against an implementation that bumped on the re-populate).
    let after_seed = store.artifact_generation();
    store.populate_augmenter_set(key.clone(), make_set([4u8; 16]));
    assert_eq!(
        after_seed,
        store.artifact_generation(),
        "a same-fingerprint augmenter re-populate MUST NOT advance artifact_generation"
    );
    // Re-populate a DIFFERENT fingerprint → base-visible change, MUST bump.
    store.populate_augmenter_set(key, make_set([5u8; 16]));
    assert_ne!(
        after_seed,
        store.artifact_generation(),
        "a changed-fingerprint augmenter populate MUST advance artifact_generation (no under-bump)"
    );
}

// ── Gap-free no-op: a base-equivalent no-op legacy replace must expose NO
//    absent window for the current key. The legacy `insert` drains every
//    prior version
//    before re-inserting; for a byte-identical replace the drained-then-
//    reinserted key is the CURRENT one a base `HostStoreView` snapshots
//    (`snapshot_all` / `snapshot_file_facts_into` gate on `content_hash ==
//    live whole_hash`). A snapshot interleaving that remove-then-reinsert
//    would observe the canonical's live-content artifact as momentarily
//    ABSENT (a missing `file_facts` / `Route` fact) while
//    `artifact_generation` is unchanged (no-op → no bump), caching an
//    incomplete snapshot under the unchanged token. The fix leaves the
//    base-equivalent current-key entry in place: a true no-op is a literal
//    no-op. ──

#[test]
fn noop_legacy_replace_leaves_current_key_entry_in_place() {
    // GAP-FREE NO-OP SOUNDNESS (deterministic discrimination): a
    // byte-identical legacy re-insert must touch NOTHING for the current
    // key. Assert the entry's `Arc` pointer is preserved across the no-op
    // replace. The base-equivalent current-key entry is left untouched (no
    // remove, no re-insert), so its `Arc` identity is preserved and it is
    // never absent. This discriminates against a legacy `insert` that
    // drained the current key and re-inserted a FRESH `Arc<FileArtifacts>`
    // (a new `FileArtifacts::with_indexed(...)`) — under which the `Arc`
    // pointer changes and the key is momentarily absent between the drain
    // and the re-insert, so the `Arc::ptr_eq` assertion fails.
    let store = FileArtifactStore::new();
    let canonical: Arc<str> = Arc::from("/g2-noop.ts");
    store.insert(Arc::clone(&canonical), synth_indexed(0x55));

    let current_key = FileArtifactKey::legacy(Arc::clone(&canonical), [0x55u8; 16]);
    let before = store
        .get_artifacts(&current_key)
        .expect("current-key entry must exist after the first insert");

    // Byte-identical re-insert (same whole_hash → same current key, same
    // base snapshot value in every dimension).
    store.insert(Arc::clone(&canonical), synth_indexed(0x55));

    let after = store.get_artifacts(&current_key).expect(
        "REGRESSION (gap-free no-op): the current-key entry is ABSENT after a no-op \
                 replace — the legacy insert drained it before re-inserting, exposing an \
                 absent window a racing base snapshot could observe",
    );
    assert!(
        Arc::ptr_eq(&before, &after),
        "REGRESSION (gap-free no-op): a base-equivalent no-op legacy replace removed and \
         re-inserted the current key (the `Arc` pointer changed) instead of leaving it in \
         place — a base snapshot interleaving the remove-then-reinsert would see the \
         canonical's \
         live-content artifact momentarily absent and cache an incomplete snapshot under the \
         unchanged token"
    );
}

#[test]
fn noop_legacy_replace_still_drains_stale_and_overlay_keys() {
    // PARITY ARM: the gap-free no-op path must NOT regress legacy "exactly
    // one base entry per canonical" semantics. A no-op replace at the
    // current key still drains a STALE-content legacy key and any
    // overlay-scoped key for the same canonical — only the base-equivalent
    // current key is preserved.
    let store = FileArtifactStore::new();
    let canonical: Arc<str> = Arc::from("/g2-drain.ts");

    // Seed a STALE-content legacy entry and an overlay-scoped entry directly.
    let stale_key = FileArtifactKey::legacy(Arc::clone(&canonical), [0xAAu8; 16]);
    store.insert_artifacts(stale_key.clone(), synth_artifacts(0xAA));
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::clone(&canonical),
        [0xBBu8; 16],
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(overlay_key.clone(), synth_artifacts(0xBB));

    // Insert the CURRENT content, then re-insert it byte-identically (no-op).
    store.insert(Arc::clone(&canonical), synth_indexed(0x55));
    let current_key = FileArtifactKey::legacy(Arc::clone(&canonical), [0x55u8; 16]);
    assert!(store.get_artifacts(&current_key).is_some());

    store.insert(Arc::clone(&canonical), synth_indexed(0x55));

    // The current key survives the no-op; the stale + overlay keys are gone.
    assert!(
        store.get_artifacts(&current_key).is_some(),
        "the current-content legacy entry must remain present after a no-op replace"
    );
    assert!(
        store.get_artifacts(&stale_key).is_none(),
        "a no-op replace must still drain a stale-content legacy key for the same canonical"
    );
    assert!(
        store.get_artifacts(&overlay_key).is_none(),
        "a no-op replace must still drain an overlay-scoped key for the same canonical"
    );
}

#[test]
fn noop_legacy_replace_never_exposes_absent_current_key_under_race() {
    // GAP-FREE NO-OP SOUNDNESS (concurrency, watchdog-guarded): a base
    // reader hammering the current key while another thread performs
    // repeated base-equivalent no-op replaces must NEVER observe the current
    // key as absent. The no-op leaves the current key in place, so a
    // concurrent `get_artifacts(current_key)` always returns `Some`. This
    // discriminates against an implementation where each no-op replace
    // removed the current key before re-inserting it: a reader interleaving
    // that window would observe `None`, and with enough iterations the race
    // is hit and the assertion fails.
    //
    // The reader thread is bounded by an iteration cap and the whole test by
    // the writer's finite loop, so a regression FAILS (asserts) rather than
    // hangs.
    use std::sync::atomic::{AtomicBool, Ordering};

    let store = Arc::new(FileArtifactStore::new());
    let canonical: Arc<str> = Arc::from("/g2-race.ts");
    store.insert(Arc::clone(&canonical), synth_indexed(0x55));
    let current_key = FileArtifactKey::legacy(Arc::clone(&canonical), [0x55u8; 16]);

    let stop = Arc::new(AtomicBool::new(false));
    let saw_absent = Arc::new(AtomicBool::new(false));

    let reader = {
        let store = Arc::clone(&store);
        let current_key = current_key.clone();
        let stop = Arc::clone(&stop);
        let saw_absent = Arc::clone(&saw_absent);
        std::thread::spawn(move || {
            // Bounded spin: stop when the writer signals done OR after a
            // generous iteration cap (watchdog — never an unbounded loop).
            for _ in 0..5_000_000u64 {
                if store.get_artifacts(&current_key).is_none() {
                    saw_absent.store(true, Ordering::Relaxed);
                    return;
                }
                if stop.load(Ordering::Relaxed) {
                    return;
                }
            }
        })
    };

    // Many base-equivalent no-op replaces, racing the reader.
    for _ in 0..20_000u64 {
        store.insert(Arc::clone(&canonical), synth_indexed(0x55));
    }
    stop.store(true, Ordering::Relaxed);
    reader.join().expect("reader thread must not panic");

    assert!(
        !saw_absent.load(Ordering::Relaxed),
        "REGRESSION (gap-free no-op): a concurrent base reader observed the current key ABSENT during a \
         base-equivalent no-op replace — the legacy insert drained the current key before \
         re-inserting it, exposing an absent window. A base `HostStoreView` build snapshotting \
         in that window would cache an incomplete snapshot under the unchanged token."
    );
}

// ── Augmentation-index cold-populate is bump-iff-fingerprint-changed.
//    `route_surface_index_fingerprints` is snapshotted BY VALUE on a
//    `HostStoreView`, and `artifact_generation` is folded into the
//    store-view reuse oracle. `ensure_augmentation_index_populated` must
//    therefore mirror `populate_augmenter_set`'s gate: only the real
//    absent → present transition advances the generation; a concurrent
//    duplicate cold populate that re-inserts an IDENTICAL fingerprint is a
//    no-op for the base snapshot and must NOT churn the token (which would
//    spuriously invalidate the manager-cached base view and split
//    singleflight lanes under batch load). The changed-over-existing arm of
//    the identical predicate is characterised by
//    `populate_augmenter_set_is_bump_iff_fingerprint_changed`; through
//    `ensure_augmentation_index_populated` itself a changed-over-existing
//    replace is unreachable single-threaded (the warm-hit `get` short-
//    circuits before the cold scan), so the duplicate-replace path it gates
//    only arises under the concurrent race exercised below. ──

/// Build a `FileArtifacts` carrying exactly one relative-specifier
/// (`declare module "./dep"`) augmentation fact, with a caller-chosen
/// `parse_stable_hash`. `parse_stable_hash` (with the augmenter canonical)
/// is the only input folded into `compute_augmenter_set_fingerprint`, so
/// two augmenters with the same canonical + `parse_stable_hash` produce the
/// same augmenter-set fingerprint.
fn synth_relative_augmenter_artifacts(parse_stable_hash: Hash16) -> Arc<FileArtifacts> {
    use super::{FileFacts, ModuleAugmentationFact, ParsedEdges};
    use verter_semantic::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};

    Arc::new(FileArtifacts {
        indexed: synth_indexed(0xA9),
        facts: Arc::new(FileFacts::empty()),
        parsed_edges: Arc::new(ParsedEdges::empty()),
        parse_stable_hash,
        augmentations: Arc::new(vec![ModuleAugmentationFact {
            specifier: InternedSpecifier::from("./dep"),
            augmented_name: InternedName::from("Augmented"),
            space: SymbolSpace::Type,
            augmented_member_shape_fingerprint: [0u8; 16],
        }]),
    })
}

fn relative_dep_target_key() -> AugmentationTargetKey {
    AugmentationTargetKey {
        project_identity: ProjectIdentity([7u8; 16]),
        resolve_env_hash: [1u8; 16],
        lib_env_hash: [2u8; 16],
        population: crate::file_artifact_store::AugmentationPopulation::Base,
        target: AugmentationTargetKind::ResolvedRelativeCanonical(Arc::from("/dep")),
    }
}

#[test]
fn cold_populate_concurrent_duplicate_does_not_overbump_artifact_generation() {
    // DISCRIMINATING (deterministic, both directions): N threads cold-
    // populate the SAME augmentation target against the SAME augmenter set.
    // A scan barrier — entered from the resolver hook, which
    // `ensure_augmentation_index_populated` invokes DURING the cold scan,
    // strictly AFTER the warm-hit `get` and strictly BEFORE the `insert` —
    // provably holds every thread past `get` (absent) before any thread
    // inserts. So all N threads reach `insert`: the first lands the
    // absent → present transition (one real bump), the other N-1 replace it
    // with the IDENTICAL fingerprint (no-op replaces).
    //
    // POST-FIX: only the one real transition bumps; the N-1 duplicate no-op
    // replaces are gated → `artifact_generation` advances by EXACTLY 1.
    // PRE-FIX (unconditional bump): every one of the N inserts bumps →
    // `artifact_generation` advances by N (>= 2). The `== 1` assertion
    // rejects the pre-fix tree with full force, on every run (the scan
    // barrier makes the contention deterministic, not probabilistic).
    use std::cell::Cell;
    use std::sync::Barrier;
    use std::thread;

    const N: usize = 16;

    let store = Arc::new(FileArtifactStore::new());
    // One base augmenter declaring `module "./dep"`.
    store.insert_artifacts(
        FileArtifactKey::legacy(Arc::from("/aug.ts"), [9u8; 16]),
        synth_relative_augmenter_artifacts([0x11u8; 16]),
    );
    let key = relative_dep_target_key();

    let start = Arc::new(Barrier::new(N));
    // The scan barrier is hit exactly once per thread (one candidate × one
    // matching fact → one resolver invocation), so `Barrier::new(N)`
    // rendezvouses all N threads inside the cold scan.
    let scan = Arc::new(Barrier::new(N));

    let before = store.artifact_generation();

    let handles: Vec<_> = (0..N)
        .map(|_| {
            let store = Arc::clone(&store);
            let key = key.clone();
            let start = Arc::clone(&start);
            let scan = Arc::clone(&scan);
            thread::spawn(move || {
                // Per-thread guard so the resolver waits on the shared scan
                // barrier at most once even if matching ever re-invokes it
                // (defensive — exactly one call is expected).
                let synced = Cell::new(false);
                let resolver = |_augmenter: &str, _specifier: &str| -> Option<Arc<str>> {
                    if !synced.replace(true) {
                        scan.wait();
                    }
                    Some(Arc::from("/dep"))
                };
                start.wait();
                store.ensure_augmentation_index_populated(&key, resolver, None);
            })
        })
        .collect();
    for h in handles {
        h.join().expect("populate thread must not panic");
    }

    let after = store.artifact_generation();
    assert_eq!(
        after - before,
        1,
        "REGRESSION (augmentation-index over-bump): {N} concurrent cold populates of the same \
         target with an IDENTICAL augmenter-set fingerprint advanced artifact_generation by {} — \
         only the single absent → present transition may bump; every duplicate no-op replace MUST \
         be gated (mirrors populate_augmenter_set). An unconditional bump churns the base store-view \
         reuse token under concurrent duplicate population.",
        after - before
    );
}

#[test]
fn cold_populate_fresh_transition_bumps_artifact_generation() {
    // NO-UNDER-BUMP arm: a fresh cold populate of a NON-EMPTY augmenter set
    // is an absent → present transition for the base snapshot and MUST
    // advance `artifact_generation`. This guards against a gate that
    // over-suppresses (a `prev_fingerprint != Some(fingerprint)` that mis-
    // handled the `None` / fresh case would fail to bump here).
    let store = FileArtifactStore::new();
    store.insert_artifacts(
        FileArtifactKey::legacy(Arc::from("/aug.ts"), [9u8; 16]),
        synth_relative_augmenter_artifacts([0x22u8; 16]),
    );
    let key = relative_dep_target_key();

    let before = store.artifact_generation();
    let set = store.ensure_augmentation_index_populated(&key, |_, _| Some(Arc::from("/dep")), None);
    assert_eq!(
        set.entries.len(),
        1,
        "the cold scan MUST match the one declared augmenter (non-empty set)"
    );
    assert_eq!(
        store.artifact_generation() - before,
        1,
        "a fresh cold populate of a non-empty augmenter set MUST advance artifact_generation \
         (absent → present transition; no under-bump)"
    );
}

// ── No-op augmentation reinsert must not invalidate the index or bump the
//    base-folded `artifact_generation`.
//
//    After the augmentation index has been populated, re-inserting a
//    byte-identical module-augmentation file is a true no-op: the augmenter's
//    contribution (its `ModuleAugmentationFact` set) is unchanged, so no index
//    entry's fold can change. The insert path runs
//    `invalidate_augmentation_index_for_augmenter` (which removes contributing
//    index rows and bumps the generation on removal) — that invalidation must
//    be gated on the augmentation-contribution equivalence, NOT run
//    unconditionally before the equivalence check. A no-op reinsert that
//    advances the store-view validation token forces warm-cache misses and
//    base-view rebuilds, contrary to the no-op guarantee.
//
//    A GENUINE augmentation change (different `parse_stable_hash` /
//    augmented-member fingerprint) MUST still invalidate the stale index rows
//    and bump (no over-suppression). ──

#[test]
fn noop_augmenter_reinsert_via_insert_artifacts_does_not_bump_artifact_generation() {
    // DISCRIMINATING: populate the augmentation index for a module-augmentation
    // file, then re-insert the BYTE-IDENTICAL augmenter through the
    // content-addressed `insert_artifacts` path. The augmenter's facts are
    // unchanged, so the contribution is a no-op and the validation token MUST
    // NOT advance.
    //
    // PRE-FIX: `insert_artifacts` calls
    // `invalidate_augmentation_index_for_augmenter` BEFORE its
    // `base_snapshot_equivalent` gate; the invalidation removes the populated
    // index row and bumps `artifact_generation` — so `after == before` FAILS.
    // POST-FIX: the invalidation is gated on contribution equivalence and is
    // skipped, so the token is unchanged.
    let store = FileArtifactStore::new();
    let aug_key = FileArtifactKey::legacy(Arc::from("/aug.ts"), [0xA9u8; 16]);
    store.insert_artifacts(
        aug_key.clone(),
        synth_relative_augmenter_artifacts([0x11u8; 16]),
    );

    // Populate the index so there is a row the augmenter contributes to.
    let target = relative_dep_target_key();
    let set =
        store.ensure_augmentation_index_populated(&target, |_, _| Some(Arc::from("/dep")), None);
    assert_eq!(
        set.entries.len(),
        1,
        "precondition: the cold scan must match the one declared augmenter"
    );
    assert_eq!(
        store.augmentation_index_len(),
        1,
        "precondition: the augmentation index must hold the populated row"
    );

    let before = store.artifact_generation();
    // Byte-identical reinsert: SAME content hash, SAME parse_stable_hash, SAME
    // augmentation facts → a true no-op.
    store.insert_artifacts(aug_key, synth_relative_augmenter_artifacts([0x11u8; 16]));
    assert_eq!(
        store.artifact_generation(),
        before,
        "REGRESSION (no-op augmenter reinsert): a byte-identical module-augmentation reinsert \
         advanced artifact_generation — the augmentation-index invalidation ran before the \
         base-equivalence gate and bumped the store-view token on a no-op, forcing warm-cache \
         misses and base-view rebuilds. The invalidation MUST be gated on augmentation-\
         contribution equivalence."
    );
}

#[test]
fn genuine_augmenter_change_via_insert_artifacts_still_invalidates_and_bumps() {
    // NO-OVER-SUPPRESSION arm: a GENUINE augmentation change (different
    // `parse_stable_hash`) MUST still invalidate the stale index row and bump
    // `artifact_generation`. Guards against a fix that suppresses the
    // invalidation unconditionally rather than only on a true no-op.
    let store = FileArtifactStore::new();
    let aug_key = FileArtifactKey::legacy(Arc::from("/aug.ts"), [0xA9u8; 16]);
    store.insert_artifacts(
        aug_key.clone(),
        synth_relative_augmenter_artifacts([0x11u8; 16]),
    );

    let target = relative_dep_target_key();
    let _ =
        store.ensure_augmentation_index_populated(&target, |_, _| Some(Arc::from("/dep")), None);
    assert_eq!(
        store.augmentation_index_len(),
        1,
        "precondition: index populated"
    );

    let before = store.artifact_generation();
    // Genuine change: a DIFFERENT augmented-member fingerprint changes the
    // augmenter's contribution to the effective surface even though the
    // declared specifier is the same.
    let changed = {
        use super::{FileFacts, ModuleAugmentationFact, ParsedEdges};
        use verter_semantic::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};
        Arc::new(FileArtifacts {
            indexed: synth_indexed(0xA9),
            facts: Arc::new(FileFacts::empty()),
            parsed_edges: Arc::new(ParsedEdges::empty()),
            parse_stable_hash: [0x11u8; 16],
            augmentations: Arc::new(vec![ModuleAugmentationFact {
                specifier: InternedSpecifier::from("./dep"),
                augmented_name: InternedName::from("Augmented"),
                space: SymbolSpace::Type,
                // CHANGED member-shape fingerprint → genuine contribution change.
                augmented_member_shape_fingerprint: [0x77u8; 16],
            }]),
        })
    };
    store.insert_artifacts(aug_key, changed);
    assert!(
        store.artifact_generation() > before,
        "a GENUINE augmentation-contribution change MUST invalidate the stale index row and \
         advance artifact_generation (no over-suppression of the no-op gate)"
    );
    assert_eq!(
        store.augmentation_index_len(),
        0,
        "a genuine augmenter change MUST invalidate (remove) the stale index row so the next \
         cold rescan folds the change in"
    );
}

#[test]
fn parse_stable_hash_change_with_same_facts_invalidates_augmentation_index_and_bumps() {
    // DISCRIMINATING (the [P2] under-invalidation arm): replace an augmenter
    // with the IDENTICAL `ModuleAugmentationFact` set but a DIFFERENT
    // `parse_stable_hash`. `parse_stable_hash` (with the augmenter canonical)
    // is folded into `compute_augmenter_set_fingerprint`, so a populated index
    // row this augmenter contributes to now carries a STALE fingerprint and
    // MUST be invalidated (so the next cold rescan re-folds the new hash) and
    // the base-folded `artifact_generation` MUST advance.
    //
    // PRE-FIX (`augmentation_contribution_equivalent` compared only the fact
    // multiset): SAME facts → predicate returns `true` → invalidation skipped →
    // the stale-fingerprint row survives and `artifact_generation` does NOT
    // bump, so BOTH assertions below fail.
    // POST-FIX (equivalence derives from the fingerprint inputs, i.e. also
    // `parse_stable_hash`): a `parse_stable_hash` change is NOT equivalent → the
    // row is invalidated and the generation bumps.
    let store = FileArtifactStore::new();
    let aug_key = FileArtifactKey::legacy(Arc::from("/aug.ts"), [0xA9u8; 16]);
    store.insert_artifacts(
        aug_key.clone(),
        synth_relative_augmenter_artifacts([0x11u8; 16]),
    );

    let target = relative_dep_target_key();
    let _ =
        store.ensure_augmentation_index_populated(&target, |_, _| Some(Arc::from("/dep")), None);
    assert_eq!(
        store.augmentation_index_len(),
        1,
        "precondition: index populated"
    );

    let before = store.artifact_generation();
    // SAME facts (identical specifier / name / space / member-shape fingerprint),
    // DIFFERENT `parse_stable_hash` — a decl-skeleton edit that moves the
    // fingerprint without changing the augmenter's declared target membership.
    store.insert_artifacts(aug_key, synth_relative_augmenter_artifacts([0x22u8; 16]));
    assert!(
        store.artifact_generation() > before,
        "REGRESSION (augmentation under-invalidation): an augmenter replaced with the SAME facts \
         but a DIFFERENT parse_stable_hash did NOT advance artifact_generation — the no-op gate \
         compared only the fact multiset, not the parse_stable_hash that feeds the \
         AugmenterSet fingerprint, so the stale-fingerprint index row keeps validating until an \
         unrelated invalidation lands"
    );
    assert_eq!(
        store.augmentation_index_len(),
        0,
        "REGRESSION (augmentation under-invalidation): the stale-fingerprint index row survived a \
         parse_stable_hash change — equivalence MUST be derived from the fingerprint inputs \
         (canonical + parse_stable_hash), not the fact multiset alone, so the next cold rescan \
         re-folds the new parse_stable_hash"
    );
}

// ── Lockstep invariant: the augmentation no-op equivalence predicate is
//    derived from the SAME inputs that determine the `AugmenterSet`
//    fingerprint, so it can never drift (too loose → under-invalidate, or
//    too tight → over-invalidate). Two artifacts differing ONLY in
//    `parse_stable_hash` (a fingerprint input) must BOTH be non-equivalent
//    under the predicate AND produce different fingerprints; identical
//    fingerprint inputs must be equivalent. The predicate and
//    `compute_augmenter_set_fingerprint` are checked together so the two
//    definitions stay in lockstep. ──
#[test]
fn augmentation_contribution_equivalence_tracks_fingerprint_inputs() {
    use smallvec::smallvec;

    use super::{
        augmentation_contribution_equivalent, compute_augmenter_set_fingerprint, AugmenterEntry,
        FileArtifactKey,
    };

    // Helper: the single fingerprint input that varies per augmenter at a
    // fixed canonical is `parse_stable_hash`. Build the one-entry augmenter
    // set the index would fold for a `/aug.ts` augmenter at a given hash.
    let canonical: Arc<str> = Arc::from("/aug.ts");
    let fingerprint_for = |parse_stable_hash: Hash16| -> Hash16 {
        let entries: smallvec::SmallVec<[AugmenterEntry; 2]> = smallvec![AugmenterEntry {
            artifact_key: FileArtifactKey::legacy(Arc::clone(&canonical), [9u8; 16]),
            parse_stable_hash,
        }];
        compute_augmenter_set_fingerprint(&entries)
    };

    let base = synth_relative_augmenter_artifacts([0x11u8; 16]);
    let same = synth_relative_augmenter_artifacts([0x11u8; 16]);
    let diff_hash = synth_relative_augmenter_artifacts([0x22u8; 16]);

    // Identical fingerprint inputs (same facts, same parse_stable_hash) →
    // equivalent AND identical fingerprint.
    assert!(
        augmentation_contribution_equivalent(&base, &same),
        "identical facts + identical parse_stable_hash MUST be equivalent (no over-invalidation; \
         preserves the no-op perf win)"
    );
    assert_eq!(
        fingerprint_for([0x11u8; 16]),
        fingerprint_for([0x11u8; 16]),
        "fingerprint MUST be stable for identical inputs"
    );

    // Differ ONLY in parse_stable_hash (a fingerprint input) → NOT equivalent
    // AND different fingerprint. This is the lockstep guard: if the predicate
    // stopped tracking parse_stable_hash it would return `true` here while the
    // fingerprint still differs — the exact [P2] under-invalidation drift.
    assert!(
        !augmentation_contribution_equivalent(&base, &diff_hash),
        "a parse_stable_hash change MUST NOT be equivalent — it changes the AugmenterSet \
         fingerprint, so the predicate must track it (lockstep with the fingerprint definition)"
    );
    assert_ne!(
        fingerprint_for([0x11u8; 16]),
        fingerprint_for([0x22u8; 16]),
        "a parse_stable_hash change MUST change the fingerprint — confirms parse_stable_hash is a \
         genuine fingerprint input, so the predicate's non-equivalence above is required, not \
         spurious"
    );

    // Differ in facts only (member-shape fingerprint) → NOT equivalent
    // (target-membership / contribution change). The fingerprint inputs here
    // are unchanged, but the fact multiset governs which index rows the
    // augmenter contributes to, so a fact change must still invalidate.
    let diff_facts = {
        use super::{FileFacts, ModuleAugmentationFact, ParsedEdges};
        use verter_semantic::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};
        Arc::new(FileArtifacts {
            indexed: synth_indexed(0xA9),
            facts: Arc::new(FileFacts::empty()),
            parsed_edges: Arc::new(ParsedEdges::empty()),
            parse_stable_hash: [0x11u8; 16],
            augmentations: Arc::new(vec![ModuleAugmentationFact {
                specifier: InternedSpecifier::from("./dep"),
                augmented_name: InternedName::from("Augmented"),
                space: SymbolSpace::Type,
                augmented_member_shape_fingerprint: [0x77u8; 16],
            }]),
        })
    };
    assert!(
        !augmentation_contribution_equivalent(&base, &diff_facts),
        "a fact-set change MUST NOT be equivalent — it can change which index rows the augmenter \
         contributes to"
    );
}
