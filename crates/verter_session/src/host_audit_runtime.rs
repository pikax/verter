#![deny(missing_docs)]
//! `HostAuditRuntime` — host-owned concrete audit runtime.
//!
//! Wraps the [`AuditRecordsStore`] instance, the `AuditConfig`
//! snapshot, and the active-request registry that
//! [`AuditRequestRegistration`] populates. The
//! `active_requests` field is **private** so callers cannot
//! mutate the map outside the three crate-private surface methods
//! `register_active_request`, `finalize_active_request`, and
//! `drop_active_request`. Tests observe the runtime via the public
//! read-only [`HostAuditRuntime::snapshot`] accessor.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use verter_audit::{AuditConfig, RequestAuditRecord};

use crate::component_meta_audit::AuditRecordsStore;
use crate::request_context::RequestContext;

/// Host-owned audit-runtime concrete type. Wraps the records store,
/// the audit-config snapshot, and the active-request registry.
///
/// The records store is consumer-visible via
/// [`Self::take_record`], [`Self::insert_record`], and
/// [`Self::audit_records_store`]. The active-request registry is
/// strictly behind crate-private surface methods so the
/// `AuditRequestRegistration` lifecycle remains the single
/// authority for inserts and removes.
pub struct HostAuditRuntime {
    config: Arc<AuditConfig>,
    records: Arc<AuditRecordsStore>,
    /// PRIVATE — direct access from outside this module is impossible.
    /// The three crate-private methods below mediate every access.
    active_requests: RwLock<FxHashMap<u64, Weak<RequestContext>>>,
}

impl HostAuditRuntime {
    /// Construct a new runtime. Each `VerterHost` owns one independent
    /// runtime; multiple hosts in one process do NOT share audit state.
    #[must_use]
    pub fn new(config: AuditConfig, records: Arc<AuditRecordsStore>) -> Self {
        Self {
            config: Arc::new(config),
            records,
            active_requests: RwLock::new(FxHashMap::default()),
        }
    }

    /// Borrow the audit-config snapshot. Read-only — the host
    /// updates the runtime as a whole when configuration changes.
    #[must_use]
    pub fn audit_config(&self) -> Arc<AuditConfig> {
        Arc::clone(&self.config)
    }

    /// Borrow the underlying records store. Consumers (NAPI / WASM /
    /// LSP) read records through this accessor; producers insert via
    /// [`Self::finalize_active_request`].
    #[must_use]
    pub fn audit_records_store(&self) -> &Arc<AuditRecordsStore> {
        &self.records
    }

    /// Public read-only snapshot of in-flight audit state. Tests
    /// probe lifecycle invariants by calling
    /// `host.host_audit_runtime().snapshot().contains_active_request(id)`.
    /// Mutation is impossible — the snapshot is owned data with no
    /// back-reference.
    #[must_use]
    pub fn snapshot(&self) -> AuditRuntimeSnapshot {
        let map = self.active_requests.read();
        let mut active_request_ids: Vec<u64> = map.keys().copied().collect();
        active_request_ids.sort_unstable();
        active_request_ids.dedup();
        let active_request_count = active_request_ids.len();
        let records_size = self.records.len();
        AuditRuntimeSnapshot {
            active_request_count,
            active_request_ids,
            records_store_size: records_size,
            records_store_capacity: crate::component_meta_audit::AUDIT_RECORDS_STORE_CAPACITY,
        }
    }

    /// Take the audit record published for `request_id`, removing it
    /// from the records store. Mirrors the existing
    /// [`AuditRecordsStore::take`] surface.
    #[must_use]
    pub fn take_record(&self, request_id: u64) -> Option<RequestAuditRecord> {
        self.records.take(request_id)
    }

    /// Crate-private. Called ONLY by `AuditRequestRegistration::new`
    /// to insert a `Weak<RequestContext>` into the active-request
    /// registry. The architecture guard
    /// `audit_request_registration_lifecycle` enforces the single
    /// in-tree call site.
    pub(crate) fn register_active_request(&self, request_id: u64, ctx: &Arc<RequestContext>) {
        let mut map = self.active_requests.write();
        map.insert(request_id, Arc::downgrade(ctx));
    }

    /// Crate-private. Called ONLY by
    /// `AuditRequestRegistration::finalize` to atomically remove the
    /// entry from the active-request registry AND publish the
    /// finalised record into the records store.
    pub(crate) fn finalize_active_request(&self, request_id: u64, record: RequestAuditRecord) {
        let mut map = self.active_requests.write();
        map.remove(&request_id);
        drop(map); // release before insertion to avoid lock-order coupling
        self.records.insert(record);
    }

    /// Crate-private. Called ONLY by `AuditRequestRegistration::drop`
    /// (defensive cleanup on panic / cancellation paths). Removes the
    /// entry from the active-request registry; does NOT publish a
    /// record — the absence of a record is itself observable.
    pub(crate) fn drop_active_request(&self, request_id: u64) {
        let mut map = self.active_requests.write();
        map.remove(&request_id);
    }
}

/// Read-only snapshot of in-flight audit state. Returned by
/// [`HostAuditRuntime::snapshot`]; safe for tests to assert against
/// without holding any lock.
#[derive(Debug, Clone)]
pub struct AuditRuntimeSnapshot {
    /// Number of in-flight registrations at sample time.
    pub active_request_count: usize,
    /// Sorted, deduped list of active request ids at sample time.
    pub active_request_ids: Vec<u64>,
    /// Number of records currently held in the records store.
    pub records_store_size: usize,
    /// Bound on the records store size (FIFO eviction at capacity).
    pub records_store_capacity: usize,
}

impl AuditRuntimeSnapshot {
    /// `true` if `request_id` was present in the active-request
    /// registry at the moment the snapshot was taken.
    #[must_use]
    pub fn contains_active_request(&self, request_id: u64) -> bool {
        self.active_request_ids.binary_search(&request_id).is_ok()
    }
}

/// Logical-request-scoped registration object.
///
/// `Active(...)` captures a slot in the host's active-request
/// registry; `Noop` means the audit-config filter rejected the kind
/// at registration time and downstream emits no record.
///
/// Constructed via [`Self::new`]. Finalised by [`Self::finalize`]
/// (idempotent). Defensive `Drop` cleans up the active-request
/// registry entry on panic / cancellation paths.
pub enum AuditRequestRegistration {
    /// Active registration — the request will produce a record on
    /// finalize.
    Active(ActiveRegistration),
    /// No-op registration — the audit-config filter rejected the
    /// request kind. No record will be produced.
    Noop,
}

impl AuditRequestRegistration {
    /// Construct a new registration. Reads the audit-config filter
    /// ONCE; if the filter rejects the request's kind, returns the
    /// `Noop` variant without entering the active-request registry.
    /// Otherwise inserts into the registry and returns `Active(...)`.
    pub fn new(host: &crate::VerterHost, ctx: Arc<RequestContext>) -> Self {
        let runtime = host.host_audit_runtime();
        let cfg = runtime.audit_config();
        if !cfg.consumer_filter.allows(&ctx.kind()) {
            return Self::Noop;
        }
        runtime.register_active_request(ctx.request_id, &ctx);
        Self::Active(ActiveRegistration {
            request_id: ctx.request_id,
            runtime: host.host_audit_runtime_arc(),
            finalized: AtomicBool::new(false),
        })
    }

    /// Idempotent finalisation. Returns `true` on the first call
    /// against an `Active` registration (the record is stored and
    /// the active-request entry is removed); `false` on subsequent
    /// calls or on `Noop`.
    pub fn finalize(&self, record: RequestAuditRecord) -> bool {
        match self {
            Self::Noop => false,
            Self::Active(active) => active.finalize(record),
        }
    }

    /// Test-only: borrow the underlying request id when the
    /// registration is `Active`. Used by the discriminating tests
    /// to probe lifecycle membership in the active-request
    /// registry.
    #[must_use]
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::Noop => None,
            Self::Active(active) => Some(active.request_id),
        }
    }
}

/// `Active` arm of the registration enum. Owns the request id, an
/// `Arc<HostAuditRuntime>` for the finalize / drop path, and a
/// `finalized` flag so finalize is idempotent.
pub struct ActiveRegistration {
    request_id: u64,
    runtime: Arc<HostAuditRuntime>,
    finalized: AtomicBool,
}

impl ActiveRegistration {
    /// Idempotent finalize. Returns `true` only on the first call.
    pub fn finalize(&self, record: RequestAuditRecord) -> bool {
        if self.finalized.swap(true, Ordering::Relaxed) {
            return false;
        }
        self.runtime
            .finalize_active_request(self.request_id, record);
        true
    }
}

impl Drop for ActiveRegistration {
    fn drop(&mut self) {
        // Defensive cleanup on panic / cancellation paths. If
        // `finalize` already ran the flag is set and we leave the
        // (already-removed) registry alone; otherwise we strip the
        // entry without publishing a record.
        if !self.finalized.load(Ordering::Relaxed) {
            self.runtime.drop_active_request(self.request_id);
        }
    }
}

impl crate::VerterHost {
    /// Borrow the host's audit runtime. Consumers (tests, NAPI,
    /// WASM, LSP) call this to reach the records store, the
    /// audit-config snapshot, and the public snapshot accessor.
    #[must_use]
    pub fn host_audit_runtime(&self) -> &HostAuditRuntime {
        self.host_audit_runtime.as_ref()
    }

    /// Reference-counted handle to the audit runtime — needed by
    /// `AuditRequestRegistration::new` so the registration owns a
    /// runtime handle for its `finalize` / `drop` paths.
    #[must_use]
    pub fn host_audit_runtime_arc(&self) -> Arc<HostAuditRuntime> {
        Arc::clone(&self.host_audit_runtime)
    }
}
