//! `SchedulerCacheId` is an opaque newtype that lives in `cache_id.rs`.
//!
//! The opaque `SchedulerCacheId(pub u64)` newtype is preferred over an
//! `enum SchedulerCacheId` — the scheduler must NOT interpret
//! cache-family semantics; the session owns cache meaning and issues
//! these ids. The newtype lives in a dedicated `cache_id.rs` module
//! (single canonical definition, no duplicate, no shim).
//!
//! These tests pin both the opacity contract and the module location:
//!
//! 1. The type lives at `verter_scheduler::cache_id::SchedulerCacheId`.
//! 2. It constructs from a raw `u64` and exposes the inner value (opaque
//!    transparent newtype — the scheduler does not branch on the value).
//! 3. Equality / ordering / hashing behave as an opaque id key.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// MUST resolve at the canonical location: the newtype is defined in
// `cache_id.rs` and re-exported / made reachable there.
use verter_scheduler::cache_id::SchedulerCacheId;

/// The newtype constructs from a raw id and round-trips its inner value.
#[test]
fn opaque_newtype_constructs_and_exposes_inner() {
    let id = SchedulerCacheId(42);
    assert_eq!(id.0, 42, "opaque newtype exposes its raw inner id");
}

/// Two ids with the same inner value compare equal; different values are
/// distinct. The scheduler treats the value as an opaque discriminator —
/// no semantic interpretation.
#[test]
fn equality_is_by_inner_value() {
    let a = SchedulerCacheId(7);
    let b = SchedulerCacheId(7);
    let c = SchedulerCacheId(8);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

/// Hash is consistent with equality so the id works as a `HashMap` /
/// `DashMap` key — the scheduler keys work-node identities on it.
#[test]
fn hash_consistent_with_equality() {
    let a = SchedulerCacheId(99);
    let b = SchedulerCacheId(99);
    let mut ha = DefaultHasher::new();
    a.hash(&mut ha);
    let mut hb = DefaultHasher::new();
    b.hash(&mut hb);
    assert_eq!(ha.finish(), hb.finish());
}

/// `Copy` + `Ord`: the id composes cheaply into ordered keys (it derives
/// `Copy, Ord` so it can sit inside `BTreeMap` keys and copy by value).
#[test]
fn copy_and_ord_semantics() {
    let a = SchedulerCacheId(1);
    let b = a; // Copy — `a` is still usable below.
    assert_eq!(a, b);
    assert!(SchedulerCacheId(1) < SchedulerCacheId(2));
}
