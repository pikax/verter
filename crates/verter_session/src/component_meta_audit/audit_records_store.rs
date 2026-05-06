#![deny(missing_docs)]
//! Bounded insert-ordered store for finished audit records.
//!
//! `VerterHost` owns a single `AuditRecordsStore` instance;
//! every audited request inserts its `RequestAuditRecord` at completion;
//! consumers (harness, NAPI, WASM, LSP) retrieve via
//! `take_audit_record(request_id)` — a strict insert-then-take flow.
//!
//! Capacity is bounded to 256 via `shift_remove_index(0)` on
//! insert-overflow (oldest-by-insertion eviction). No access-refresh
//! semantics are needed because records are drained exactly once.
//!
//! Each entry carries an `Instant` captured at insert time so the
//! batch aggregator (via [`verter_audit::batch::AuditRecordSource`])
//! can honour an `Instant`-keyed `since` window without having to
//! re-key records by wall-clock time.

use std::time::Instant;

use indexmap::IndexMap;
use parking_lot::Mutex;
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

/// Thread-safe insert-ordered store of `(request_id, (Instant, RequestAuditRecord))`.
#[derive(Debug)]
pub struct AuditRecordsStore {
    inner: Mutex<IndexMap<u64, StoredRecord>>,
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
            inner: Mutex::new(IndexMap::with_capacity(capacity.max(1))),
            capacity: capacity.max(1),
        }
    }

    /// Insert a record. If the store is at capacity, the
    /// oldest-by-insertion entry is evicted first. If the same
    /// `request_id` was already present, the prior entry is
    /// replaced in-place without affecting insertion order. The
    /// insert timestamp is captured here from `Instant::now()`.
    pub fn insert(&self, record: RequestAuditRecord) {
        let mut map = self.inner.lock();
        let key = record.request_id;
        if !map.contains_key(&key) && map.len() >= self.capacity {
            map.shift_remove_index(0);
        }
        map.insert(
            key,
            StoredRecord {
                inserted_at: Instant::now(),
                record,
            },
        );
    }

    /// Remove and return the record for `request_id`, if present.
    /// The accompanying `Instant` is dropped — only the bare record
    /// is returned to keep the established public API.
    pub fn take(&self, request_id: u64) -> Option<RequestAuditRecord> {
        let mut map = self.inner.lock();
        map.shift_remove(&request_id).map(|stored| stored.record)
    }

    /// Number of records currently stored (for diagnostics / tests).
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// `true` when the store currently holds no records.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

impl AuditRecordSource for AuditRecordsStore {
    fn for_each_record(&self, f: &mut dyn FnMut(Instant, &RequestAuditRecord)) {
        let map = self.inner.lock();
        for stored in map.values() {
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
