#![deny(missing_docs)]
//! Session-side adapter bridging workspace fan-out
//! ([`verter_workspace::audit_sink::VfsAuditSink`]) into the active
//! request's [`RequestFootprintAccumulator`].
//!
//! Each audited request:
//!
//! 1. Builds a [`SessionVfsSink`] holding its `request_id` and a
//!    [`Weak`] reference to its accumulator.
//! 2. Registers the sink against the host's workspace, receiving a
//!    [`SinkRegistration`] (RAII).
//! 3. Drops the registration at request exit; if the accumulator is
//!    dropped earlier (panic unwind), incoming events fall through
//!    silently via the `Weak::upgrade` check.
//!
//! The sink filters by `request_id` so concurrent audits on the same
//! workspace each receive only their own events — the workspace fans
//! events to every registered sink without knowing which session owns
//! which request.
//!
//! The sink never panics: the `Weak::upgrade` check handles dropped
//! accumulators, the `request_id` filter handles foreign events,
//! and the layer conversion covers every
//! [`VfsAuditLayer`](verter_workspace::audit_sink::VfsAuditLayer)
//! variant.

use std::sync::{Arc, Weak};

use verter_workspace::audit_sink::{VfsAuditSink, VfsReadEvent};

#[cfg(test)]
use super::VfsLayer;
use super::{RequestFootprintAccumulator, VfsReadRecord};

/// Session-owned VFS audit sink. Filters fan-out events by
/// `request_id` and forwards matches to the accumulator as
/// [`VfsReadRecord`]s.
pub(crate) struct SessionVfsSink {
    request_id: u64,
    accumulator: Weak<RequestFootprintAccumulator>,
}

impl SessionVfsSink {
    /// Build a sink for `request_id` that forwards events to the
    /// given accumulator. The accumulator is held by [`Weak`] — if
    /// the owning `Arc` is dropped before the workspace
    /// deregisters the sink, `on_vfs_read` no-ops.
    pub(crate) fn new(request_id: u64, accumulator: Arc<RequestFootprintAccumulator>) -> Arc<Self> {
        Arc::new(Self {
            request_id,
            accumulator: Arc::downgrade(&accumulator),
        })
    }

    /// `true` when the owning accumulator is still alive. Public
    /// in-crate for the drop-no-panic regression test.
    #[cfg(test)]
    pub(crate) fn accumulator_alive(&self) -> bool {
        self.accumulator.strong_count() > 0
    }
}

impl VfsAuditSink for SessionVfsSink {
    fn on_vfs_read(&self, event: &VfsReadEvent) {
        // Filter 1: only events routed to THIS request.
        let Some(event_request_id) = event.request_id else {
            return;
        };
        if event_request_id != self.request_id {
            return;
        }

        // Filter 2: accumulator still alive (weak-ref guard — survives
        // a panic-unwind that drops the session-side Arc before the
        // workspace deregisters us).
        let Some(acc) = self.accumulator.upgrade() else {
            return;
        };

        acc.push_vfs_read(VfsReadRecord {
            canonical_id: Arc::clone(&event.canonical_id),
            layer: super::vfs_layer_from_workspace(event.layer),
            cache_hit: event.cache_hit,
            bytes_read: event.bytes_read,
            request_id: self.request_id,
        });
        // Per-file timing ledger fan-out — the workspace
        // emits `read_ns` only when the host's `audit_timing_capture`
        // flag is on (read via TLS by `current_timing_enabled`). The
        // accumulator stores the ledger entry for `FileAudit` build
        // at request finalisation.
        acc.push_file_read_timing(super::accumulator::FileReadTiming {
            canonical_id: Arc::clone(&event.canonical_id),
            layer: super::vfs_layer_from_workspace(event.layer),
            cache_hit: event.cache_hit,
            bytes_read: event.bytes_read,
            read_ns: event.read_ns,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_workspace::audit_sink::VfsAuditLayer;

    fn make_event(
        canonical: &str,
        request_id: Option<u64>,
        layer: VfsAuditLayer,
        bytes: u64,
    ) -> VfsReadEvent {
        VfsReadEvent {
            canonical_id: Arc::from(canonical),
            layer,
            cache_hit: false,
            bytes_read: bytes,
            read_ns: None,
            request_id,
            thread_id: std::thread::current().id(),
        }
    }

    #[test]
    fn session_vfs_sink_filters_by_request_id_drops_foreign_events_under_attach_to() {
        let acc = Arc::new(RequestFootprintAccumulator::new());
        let sink = SessionVfsSink::new(42, Arc::clone(&acc));

        // Matching request: recorded.
        sink.on_vfs_read(&make_event("/own.ts", Some(42), VfsAuditLayer::Disk, 100));
        // Foreign request: dropped.
        sink.on_vfs_read(&make_event(
            "/foreign.ts",
            Some(7),
            VfsAuditLayer::Disk,
            999,
        ));
        // No request id: dropped (no active audit context at emission).
        sink.on_vfs_read(&make_event(
            "/context-less.ts",
            None,
            VfsAuditLayer::Overlay,
            1,
        ));

        let state = acc.drain();
        assert_eq!(
            state.vfs_reads.len(),
            1,
            "only events with matching request_id should be recorded"
        );
        assert_eq!(state.vfs_reads[0].canonical_id.as_ref(), "/own.ts");
        assert_eq!(state.vfs_reads[0].bytes_read, 100);
        assert_eq!(state.vfs_reads[0].request_id, 42);
    }

    #[test]
    fn session_vfs_sink_drops_events_after_accumulator_dropped_no_panic() {
        let acc = Arc::new(RequestFootprintAccumulator::new());
        let sink = SessionVfsSink::new(5, Arc::clone(&acc));
        assert!(sink.accumulator_alive());

        // Drop the strong ref — weak now returns None.
        drop(acc);
        assert!(!sink.accumulator_alive());

        // Event must no-op, never panic.
        sink.on_vfs_read(&make_event("/late.ts", Some(5), VfsAuditLayer::Disk, 0));
        // If we got here, no panic. The sink is inert now.
    }

    #[test]
    fn session_vfs_sink_maps_every_vfs_audit_layer_variant() {
        let acc = Arc::new(RequestFootprintAccumulator::new());
        let sink = SessionVfsSink::new(1, Arc::clone(&acc));

        for (layer, expected) in [
            (VfsAuditLayer::Overlay, VfsLayer::Overlay),
            (VfsAuditLayer::Snapshot, VfsLayer::Snapshot),
            (VfsAuditLayer::Disk, VfsLayer::Disk),
            (VfsAuditLayer::DirIndexNegative, VfsLayer::DirIndexNegative),
            (VfsAuditLayer::Missing, VfsLayer::Missing),
        ] {
            sink.on_vfs_read(&make_event("/x.ts", Some(1), layer, 0));
            let state = acc.drain();
            assert_eq!(state.vfs_reads.len(), 1, "one event expected for {layer:?}");
            assert_eq!(
                state.vfs_reads[0].layer, expected,
                "layer conversion mismatch for {layer:?}"
            );
        }
    }
}
