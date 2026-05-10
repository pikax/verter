#![deny(missing_docs)]
//! VFS audit sink registry.
//!
//! The workspace side publishes `VfsReadEvent`s
//! to every registered [`VfsAuditSink`]. Session-side audit
//! (`verter_session::component_meta_audit::session_vfs_sink::SessionVfsSink`)
//! registers one sink per audited request and filters events by
//! `request_id`.

use std::sync::Arc;

/// VFS layer a read was satisfied from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsAuditLayer {
    /// Overlay (active editor buffer).
    Overlay,
    /// Snapshot cache hit.
    Snapshot,
    /// Disk read.
    Disk,
    /// Directory index returned a negative (file known not to exist).
    DirIndexNegative,
    /// Read missed every layer — the file was not found.
    Missing,
}

/// Observation emitted when a VFS read completes (success or miss).
#[derive(Debug, Clone)]
pub struct VfsReadEvent {
    /// Canonical id of the file that was read.
    pub canonical_id: Arc<str>,
    /// Which VFS layer served the read.
    pub layer: VfsAuditLayer,
    /// `true` when served by an in-memory cache (overlay / snapshot).
    pub cache_hit: bool,
    /// Number of bytes returned (0 for `DirIndexNegative` / `Missing`).
    pub bytes_read: u64,
    /// Wall-clock nanoseconds spent inside the workspace `read_file`
    /// path. `Some(value)` only when the host had timing capture
    /// enabled at event time; `None` on the zero-cost fast path.
    pub read_ns: Option<u64>,
    /// Request id of the active audited request at event time, read
    /// from `verter_scheduler::request_context::current_request_id()`.
    /// `None` when no context is installed.
    pub request_id: Option<u64>,
    /// Thread id of the worker that emitted the event.
    pub thread_id: std::thread::ThreadId,
}

/// Opaque handle returned by `register_audit_sink`. Used to
/// deregister.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SinkHandle(pub(crate) u64);

/// RAII registration — dropping the registration deregisters the sink
/// from its workspace. Session-side code holds one of these per
/// audited request.
pub struct SinkRegistration {
    handle: SinkHandle,
    workspace: std::sync::Weak<dyn crate::WorkspaceAccess>,
}

impl SinkRegistration {
    /// Construct a new registration — typically only called by the
    /// workspace's `register_audit_sink` implementation to hand a
    /// registration back to the caller.
    pub fn new(handle: SinkHandle, workspace: std::sync::Weak<dyn crate::WorkspaceAccess>) -> Self {
        Self { handle, workspace }
    }

    /// Access the registration's opaque handle — needed by callers
    /// that want to deregister explicitly without relying on drop.
    pub fn handle(&self) -> SinkHandle {
        self.handle
    }
}

impl Drop for SinkRegistration {
    fn drop(&mut self) {
        if let Some(ws) = self.workspace.upgrade() {
            let _ = ws.deregister_audit_sink(self.handle);
        }
    }
}

/// Callback surface for workspace audit fan-out.
pub trait VfsAuditSink: Send + Sync {
    /// Called when the workspace observes a VFS read. Implementations
    /// must be non-blocking — the workspace does not expect the sink
    /// to do meaningful work on the critical path.
    fn on_vfs_read(&self, event: &VfsReadEvent);
}

/// Errors from sink registry operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditSinkError {
    /// The workspace does not implement audit-sink registration.
    NotSupported,
    /// The handle passed to `deregister_audit_sink` was not found in
    /// the registry.
    HandleNotFound,
}
