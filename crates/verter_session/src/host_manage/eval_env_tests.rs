//! Tier 1A discriminating tests for `host_manage::eval_env`.
//!
//! Includes `single_parse_authority_repeated_queries_reuse_lowered_artifact`.

use crate::owned_artifacts::eval_program::OwnedEvalProgram;
use crate::project_type_store::{EvalEnvCacheDb, OwnedArtifactKey};
use std::sync::Arc;
use verter_semantic::analysis::Hash16;

#[test]
fn single_parse_authority_repeated_queries_reuse_lowered_artifact() {
    // Tier 1A invariant: the typed `EvalEnvCacheDb` is the single
    // owned-artifact authority. Repeated `get` calls for the same
    // `(canonical_id, whole_hash)` MUST return Arc clones of the same
    // payload — there is exactly ONE lowered `OwnedEvalProgram` per
    // (canonical_id, whole_hash) tuple.
    //
    // Discriminator (FAIL pre-1A): the cache type itself does not
    // exist pre-1A. Compilation against the pre-1A tree fails. Post-1A
    // the cache is empty by default; the test populates one entry and
    // asserts pointer-equality across two reads.
    let db = EvalEnvCacheDb::new();
    let key = OwnedArtifactKey::new("file.vue", [0u8; 16]);
    let original: Arc<OwnedEvalProgram> = Arc::new(OwnedEvalProgram::empty());
    db.insert(key.clone(), Arc::clone(&original));

    let read_a = db.get(&key).expect("entry present after insert");
    let read_b = db.get(&key).expect("entry present on repeat read");
    // Pointer-equality: same Arc payload across reads. A regression
    // that clones the inner OwnedEvalProgram on every read (instead of
    // sharing the Arc) would fail this assertion.
    assert!(
        Arc::ptr_eq(&read_a, &read_b),
        "repeated reads must reuse the same Arc payload (single parse authority)"
    );
    // And the payload pointer-equals the inserted one — the cache is
    // not synthesizing a new lowered artifact on read.
    assert!(
        Arc::ptr_eq(&original, &read_a),
        "cache MUST NOT re-lower on read"
    );

    // Distinct content versions get distinct entries — the
    // (canonical_id, whole_hash) identity tuple is the discriminator,
    // NOT canonical_id alone.
    let mut other_hash: Hash16 = [0u8; 16];
    other_hash[0] = 1;
    let key2 = OwnedArtifactKey::new("file.vue", other_hash);
    let other_program = Arc::new(OwnedEvalProgram::empty());
    db.insert(key2.clone(), Arc::clone(&other_program));
    let read_c = db.get(&key2).expect("second-version entry present");
    assert!(
        !Arc::ptr_eq(&read_a, &read_c),
        "different whole_hash MUST NOT alias to the same payload"
    );
}

#[test]
fn eval_env_cache_db_clear_drains_all_entries() {
    let db = EvalEnvCacheDb::new();
    let key = OwnedArtifactKey::new("file.vue", [0u8; 16]);
    db.insert(key.clone(), Arc::new(OwnedEvalProgram::empty()));
    assert_eq!(db.len(), 1);
    db.clear();
    assert_eq!(db.len(), 0);
    assert!(db.get(&key).is_none(), "post-clear lookup MUST miss");
}

/// Block 1.J.3 production-path discriminator — the legacy
/// `Arc<EvalEnv>` cache reflects a content edit through KEY identity,
/// not through `eval_env_cache().clear()`.
///
/// `upsert_without_dependent_eviction` runs the full own-canonical
/// upsert pipeline but SKIPS the reverse-dep cascade — and the
/// upsert's `eval_env_cache().clear()` lives inside that cascade
/// block. So after this upsert the eval-env cache is NOT cleared.
/// With the legacy storage keyed by the full R21 `FileArtifactKey`
/// (content hash folded into the key), the edited file's new content
/// hash yields a fresh key → cache miss → recompute. The freshly
/// built env therefore reflects the new declaration.
///
/// Discriminating: the assertion checks the env exposes the
/// post-edit type declaration. The eager `clear()` is intentionally
/// not exercised here — this proves cache CORRECTNESS comes from the
/// key, which is the precondition for Block 2 removing the clear.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn base_eval_env_reflects_content_edit_without_eager_clear() {
    use crate::types::{FileKind, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());

    // Initial content — one interface member.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/types.ts".to_string()),
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface Foo { a: number }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("initial upsert");

    // Build + cache the eval-env for the initial content.
    let env_before = host
        .base_eval_env("/src/types.ts")
        .expect("eval-env builds for initial content");
    assert!(
        env_before.type_declaration_id("Foo").is_some(),
        "precondition: initial eval-env knows interface Foo"
    );
    assert!(
        env_before.type_declaration_id("Bar").is_none(),
        "precondition: initial eval-env does NOT know Bar"
    );

    // Edit the file — add a second interface. Crucially via
    // `upsert_without_dependent_eviction`, which SKIPS the reverse-dep
    // cascade where the upsert's `eval_env_cache().clear()` lives. The
    // eval-env cache is therefore NOT cleared by this upsert.
    let _ = host
        .upsert_without_dependent_eviction(UpsertRequest {
            canonical_id: Some("/src/types.ts".to_string()),
            input_id: "/src/types.ts".to_string(),
            source: Arc::from(
                "export interface Foo { a: number }\nexport interface Bar { b: string }",
            ),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("content edit upsert");

    // The eval-env for the edited file MUST reflect the new content.
    // The new content hash produces a fresh `FileArtifactKey` → the
    // stale entry under the old key cannot be hit → recompute.
    let env_after = host
        .base_eval_env("/src/types.ts")
        .expect("eval-env builds for edited content");
    assert!(
        env_after.type_declaration_id("Bar").is_some(),
        "B1.j.3: the eval-env cache MUST reflect a content edit via \
         key identity (the new content hash is part of the \
         FileArtifactKey), NOT via eager `eval_env_cache().clear()`. \
         A missing `Bar` here means a stale env was served."
    );
}
