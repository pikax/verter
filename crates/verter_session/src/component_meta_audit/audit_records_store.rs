//! Bounded insert-ordered store for finished audit records.
//!
//! Plan §2.5. `VerterHost` owns a single `AuditRecordsStore` instance;
//! every audited request inserts its `RustAuditRecord` at completion;
//! consumers (harness, NAPI, WASM, LSP) retrieve via
//! `take_audit_record(request_id)` — a strict insert-then-take flow.
//!
//! Capacity is bounded to 256 via `shift_remove_index(0)` on
//! insert-overflow (oldest-by-insertion eviction). No access-refresh
//! semantics are needed because records are drained exactly once.

use indexmap::IndexMap;
use parking_lot::Mutex;

use super::RustAuditRecord;

/// Default capacity per plan §1.4 / §2.5.
pub const AUDIT_RECORDS_STORE_CAPACITY: usize = 256;

/// Thread-safe insert-ordered store of `(request_id, RustAuditRecord)`.
#[derive(Debug)]
pub struct AuditRecordsStore {
    inner: Mutex<IndexMap<u64, RustAuditRecord>>,
    capacity: usize,
}

impl Default for AuditRecordsStore {
    fn default() -> Self {
        Self::with_capacity(AUDIT_RECORDS_STORE_CAPACITY)
    }
}

impl AuditRecordsStore {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(IndexMap::with_capacity(capacity.max(1))),
            capacity: capacity.max(1),
        }
    }

    /// Insert a record. If the store is at capacity, the
    /// oldest-by-insertion entry is evicted first. If the same
    /// `request_id` was already present, the prior entry is
    /// replaced in-place without affecting insertion order.
    pub fn insert(&self, record: RustAuditRecord) {
        let mut map = self.inner.lock();
        let key = record.request_id;
        if !map.contains_key(&key) && map.len() >= self.capacity {
            map.shift_remove_index(0);
        }
        map.insert(key, record);
    }

    /// Remove and return the record for `request_id`, if present.
    pub fn take(&self, request_id: u64) -> Option<RustAuditRecord> {
        let mut map = self.inner.lock();
        map.shift_remove(&request_id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_meta_audit::{
        RustMemoryAudit, RustSolverAudit, RustStoreAudit, RustTimingAudit,
    };

    fn dummy_record(request_id: u64) -> RustAuditRecord {
        RustAuditRecord {
            request_id,
            canonical_id: format!("/req{request_id}.vue"),
            timings: RustTimingAudit::default(),
            solver: RustSolverAudit::default(),
            store: RustStoreAudit::default(),
            memory: RustMemoryAudit::default(),
            footprint: None,
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
}
