//! VFS audit sink registry.
//!
//! Plan §2.4 / Commit 4. The workspace side publishes `VfsReadEvent`s
//! to every registered [`VfsAuditSink`]. Session-side audit
//! (`verter_session::component_meta_audit::session_vfs_sink::SessionVfsSink`)
//! registers one sink per audited request and filters events by
//! `request_id`.
//!
//! This replaces the legacy file-based component-meta trace that lived
//! in `filesystem.rs`. The clean-cut rule (plan §0.1) requires the
//! legacy trace to be deleted in the same work-unit that lands the
//! sink registry — see Commit 4's `Legacy deletions` summary.

use std::sync::Arc;

/// VFS layer a read was satisfied from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsAuditLayer {
    Overlay,
    Snapshot,
    Disk,
    DirIndexNegative,
    Missing,
}

/// Observation emitted when a VFS read completes (success or miss).
#[derive(Debug, Clone)]
pub struct VfsReadEvent {
    pub canonical_id: Arc<str>,
    pub layer: VfsAuditLayer,
    pub cache_hit: bool,
    pub bytes_read: u64,
    /// Request id of the active audited request at event time, read
    /// from `verter_scheduler::request_context::current_request_id()`.
    /// `None` when no context is installed.
    pub request_id: Option<u64>,
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
    pub fn new(handle: SinkHandle, workspace: std::sync::Weak<dyn crate::WorkspaceAccess>) -> Self {
        Self { handle, workspace }
    }

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
    fn on_vfs_read(&self, event: &VfsReadEvent);
}

/// Errors from sink registry operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditSinkError {
    NotSupported,
    HandleNotFound,
}
