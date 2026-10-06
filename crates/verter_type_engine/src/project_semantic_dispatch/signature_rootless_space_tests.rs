//! A rootless signature's binder space is named by its complete stable key.

use super::RootlessSpace;
use crate::semantic_query::stable_key::StableKey;
use crate::signature_kernel::{SignatureStore, StoreError};

/// Two rootless signatures whose stable keys share a forced fingerprint but
/// differ in bytes name two binder spaces: they never claim one identity,
/// so the store either keeps them under two keys or reports the typed
/// collision, and never merges them. The same bytes under any fingerprint
/// name one space.
#[test]
fn a_forced_fingerprint_collision_never_merges_two_binder_spaces() {
    const FINGERPRINT: u64 = 0x5eed;
    let first = RootlessSpace::of(&StableKey::with_forced_fingerprint(
        b"rootless-a".to_vec(),
        FINGERPRINT,
    ));
    let second = RootlessSpace::of(&StableKey::with_forced_fingerprint(
        b"rootless-b".to_vec(),
        FINGERPRINT,
    ));
    assert_ne!(
        first.identity, second.identity,
        "keys that share only a fingerprint name two binder spaces"
    );
    let store = SignatureStore::new();
    assert_eq!(store.claim_space_key(first.key, first.identity), Ok(()));
    match store.claim_space_key(second.key, second.identity) {
        Ok(()) => assert_ne!(first.key, second.key, "two spaces under one key"),
        Err(error) => assert_eq!(error, StoreError::BinderKeyCollision),
    }
    assert_eq!(
        RootlessSpace::of(&StableKey::with_forced_fingerprint(
            b"rootless-a".to_vec(),
            FINGERPRINT ^ 1,
        )),
        first,
        "the space is a function of the key bytes, not of the fingerprint"
    );
}
