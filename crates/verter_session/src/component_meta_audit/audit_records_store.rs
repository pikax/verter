#![deny(missing_docs)]
//! Bounded insert-ordered store for finished audit records.
//!
//! `VerterHost` owns a single `AuditRecordsStore` instance;
//! every audited request inserts its `RequestAuditRecord` at completion;
//! consumers (harness, NAPI, WASM, LSP) retrieve via
//! `take_audit_record(request_id)` — a strict insert-then-take flow.
//!
//! Capacity is bounded to 256 by oldest-by-insertion eviction on
//! insert-overflow. No access-refresh semantics are needed because
//! records are drained exactly once.
//!
//! Every audited request of the host passes through this one lock, so
//! the work under it is bounded independently of the fill: insertion
//! order is a sequence number indexed by an ordered map, so evicting the
//! oldest entry or taking any entry relocates no other entry, each record
//! is boxed before the lock so an insert moves a pointer, and a
//! record leaving the store (evicted or replaced) is dropped
//! after the lock is released.
//!
//! Each entry carries an `Instant` captured at insert time so the
//! batch aggregator (via [`verter_audit::batch::AuditRecordSource`])
//! can honour an `Instant`-keyed `since` window without having to
//! re-key records by wall-clock time.

use verter_type_engine::instant::Instant;

use std::collections::BTreeMap;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_audit::batch::AuditRecordSource;

use super::RequestAuditRecord;

/// Default capacity per.
pub const AUDIT_RECORDS_STORE_CAPACITY: usize = 256;

/// One stored entry — the record and the wall-clock `Instant`
/// captured at insert time.
#[derive(Debug)]
struct StoredRecord {
    inserted_at: Instant,
    record: RequestAuditRecord,
}

/// The store's state under its lock: each record by request id with its
/// insertion sequence number, and the request ids by sequence number, in
/// insertion order.
#[derive(Debug, Default)]
struct Entries {
    by_request: FxHashMap<u64, (u64, Box<StoredRecord>)>,
    by_sequence: BTreeMap<u64, u64>,
    next_sequence: u64,
}

impl Entries {
    fn remove(&mut self, request_id: u64) -> Option<Box<StoredRecord>> {
        let (sequence, stored) = self.by_request.remove(&request_id)?;
        self.by_sequence.remove(&sequence);
        Some(stored)
    }
}

/// Thread-safe insert-ordered store of `(request_id, (Instant, RequestAuditRecord))`.
#[derive(Debug)]
pub struct AuditRecordsStore {
    inner: Mutex<Entries>,
    capacity: usize,
}

impl Default for AuditRecordsStore {
    fn default() -> Self {
        Self::with_capacity(AUDIT_RECORDS_STORE_CAPACITY)
    }
}

impl AuditRecordsStore {
    /// Construct a store bounded to `capacity` entries (oldest-by-
    /// insertion is evicted on overflow). A capacity below 1 is
    /// clamped to 1.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Entries::default()),
            capacity: capacity.max(1),
        }
    }

    /// Insert a record. If the store is at capacity, the
    /// oldest-by-insertion entry is evicted first. If the same
    /// `request_id` was already present, the prior entry is
    /// replaced in-place without affecting insertion order. The
    /// insert timestamp is captured here from `Instant::now()`.
    pub fn insert(&self, record: RequestAuditRecord) {
        let key = record.request_id;
        let stored = Box::new(StoredRecord {
            inserted_at: Instant::now(),
            record,
        });
        let mut entries = self.inner.lock();
        let released = if let Some((_, slot)) = entries.by_request.get_mut(&key) {
            Some(std::mem::replace(slot, stored))
        } else {
            let evicted = if entries.by_request.len() >= self.capacity {
                let oldest = entries.by_sequence.first_key_value().map(|(_, id)| *id);
                oldest.and_then(|id| entries.remove(id))
            } else {
                None
            };
            let sequence = entries.next_sequence;
            entries.next_sequence += 1;
            entries.by_sequence.insert(sequence, key);
            entries.by_request.insert(key, (sequence, stored));
            evicted
        };
        drop(entries);
        drop(released);
    }

    /// Remove and return the record for `request_id`, if present.
    /// The accompanying `Instant` is dropped — only the bare record
    /// is returned to keep the established public API.
    pub fn take(&self, request_id: u64) -> Option<RequestAuditRecord> {
        let taken = self.inner.lock().remove(request_id);
        taken.map(|stored| stored.record)
    }

    /// Number of records currently stored (for diagnostics / tests).
    pub fn len(&self) -> usize {
        self.inner.lock().by_request.len()
    }

    /// `true` when the store currently holds no records.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().by_request.is_empty()
    }
}

#[cfg(test)]
impl AuditRecordsStore {
    /// Where the record of `request_id` sits in the store's insertion
    /// order: a position that only its own removal ends.
    fn position_of(&self, request_id: u64) -> Option<u64> {
        self.inner
            .lock()
            .by_request
            .get(&request_id)
            .map(|(sequence, _)| *sequence)
    }
}

impl AuditRecordSource for AuditRecordsStore {
    fn for_each_record(&self, f: &mut dyn FnMut(Instant, &RequestAuditRecord)) {
        let entries = self.inner.lock();
        for request_id in entries.by_sequence.values() {
            let (_, stored) = &entries.by_request[request_id];
            f(stored.inserted_at, &stored.record);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_meta_audit::{
        ComponentMetaPayload, RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit,
    };

    fn dummy_record(request_id: u64) -> RequestAuditRecord {
        RequestAuditRecord {
            request_id,
            canonical_id: format!("/req{request_id}.vue"),
            target_identity: Some(verter_audit::RequestTargetIdentity::RegisteredCanonical(
                format!("/req{request_id}.vue"),
            )),
            kind: super::super::RequestKind::ComponentMeta,
            parent_request_id: None,
            timings: RequestTimingAudit::default(),
            store: RequestStoreAudit::default(),
            memory: RequestMemoryAudit::default(),
            footprint: None,
            scheduler: None,
            from_cache: false,
            files: Vec::new(),
            waits: None,
            kind_payload: super::super::RequestKindPayload::ComponentMeta(
                ComponentMetaPayload::default(),
            ),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: String::new(),
        }
    }

    #[test]
    fn take_audit_record_returns_some_after_insert() {
        let store = AuditRecordsStore::default();
        store.insert(dummy_record(1));
        let taken = store.take(1);
        assert!(taken.is_some());
        assert_eq!(taken.unwrap().request_id, 1);
    }

    #[test]
    fn take_audit_record_returns_none_after_drain() {
        let store = AuditRecordsStore::default();
        store.insert(dummy_record(2));
        let _ = store.take(2).expect("first take succeeds");
        assert!(store.take(2).is_none(), "second take drains to None");
    }

    #[test]
    fn audit_records_store_evicts_oldest_by_insertion_at_capacity_256() {
        let store = AuditRecordsStore::with_capacity(256);
        for id in 1..=256 {
            store.insert(dummy_record(id));
        }
        assert_eq!(store.len(), 256);
        assert!(store.take(1).is_some(), "id=1 still present at the limit");
        // Re-fill, then one more insert must evict the oldest.
        store.insert(dummy_record(1));
        store.insert(dummy_record(257));
        assert_eq!(store.len(), 256);
        assert!(
            store.take(2).is_none(),
            "id=2 must have been evicted as the oldest on overflow",
        );
        assert!(store.take(257).is_some(), "newest entry is retained");
    }

    /// Every audited request of a host inserts here under one lock, so an
    /// insert into a full store must not relocate the entries it keeps:
    /// relocating them made each request's time under the lock grow with
    /// the store's fill, and concurrent requests on unrelated keys queued
    /// behind it.
    #[test]
    fn an_eviction_or_a_take_relocates_no_retained_record() {
        let store = AuditRecordsStore::with_capacity(8);
        for id in 1..=8 {
            store.insert(dummy_record(id));
        }
        let positions = |store: &AuditRecordsStore| -> Vec<(u64, Option<u64>)> {
            (1..=12).map(|id| (id, store.position_of(id))).collect()
        };
        let before = positions(&store);
        // Two evictions (ids 1 and 2) and a take from the middle.
        store.insert(dummy_record(9));
        store.insert(dummy_record(10));
        assert!(store.take(5).is_some());
        let after = positions(&store);
        for (id, position) in before {
            if matches!(id, 1 | 2 | 5) || position.is_none() {
                continue;
            }
            let kept = after
                .iter()
                .find(|(other, _)| *other == id)
                .map(|(_, p)| *p);
            assert_eq!(kept, Some(position), "record {id} kept its position");
        }
        assert_eq!(store.position_of(1), None);
        assert_eq!(store.position_of(2), None);
        assert_eq!(store.position_of(5), None);
    }

    #[test]
    fn eviction_follows_insertion_order_across_takes_and_replacements() {
        let store = AuditRecordsStore::with_capacity(3);
        store.insert(dummy_record(1));
        store.insert(dummy_record(2));
        store.insert(dummy_record(3));
        // Replacing 1 keeps its place; taking 2 frees a slot.
        store.insert(dummy_record(1));
        assert!(store.take(2).is_some());
        store.insert(dummy_record(4));
        // Full again: the next insert evicts the oldest, which is 1.
        store.insert(dummy_record(5));
        let mut order = Vec::new();
        store.for_each_record(&mut |_, record| order.push(record.request_id));
        assert_eq!(order, vec![3, 4, 5]);
        assert!(store.take(1).is_none(), "the oldest record was evicted");
    }

    #[test]
    fn for_each_record_yields_every_stored_record_with_an_instant() {
        let store = AuditRecordsStore::default();
        for id in 1..=3 {
            store.insert(dummy_record(id));
        }
        let mut seen: Vec<(u64, bool)> = Vec::new();
        let now = Instant::now();
        store.for_each_record(&mut |inserted_at, record| {
            // Every inserted_at must precede a freshly-captured
            // `Instant::now()` — proves the store actually captured
            // a real time stamp rather than a constant default.
            seen.push((record.request_id, inserted_at <= now));
        });
        seen.sort_by_key(|(id, _)| *id);
        assert_eq!(seen, vec![(1, true), (2, true), (3, true)]);
    }
}
