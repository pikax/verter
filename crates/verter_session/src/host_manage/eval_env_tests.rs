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
