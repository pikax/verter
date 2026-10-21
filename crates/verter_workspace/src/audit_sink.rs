#![deny(missing_docs)]
//! VFS audit sink registry.
//!
//! The workspace side publishes `VfsReadEvent`s to every registered
//! `VfsAuditSink`. Session-side audit
//! (`verter_session::session_vfs_sink::SessionVfsSink`) registers one sink
//! per audited request and filters events by `request_id`.
//!
//! This is optional capture: the registry, the events and the per-read
//! clock exist only with `semantic-observe`. A default build keeps the
//! zero-size [`VfsAuditSinks`] / [`VfsReadProbe`] call shape and does no
//! work per read — no registry lock, no clock, no event.

#[cfg(feature = "semantic-observe")]
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
#[cfg(feature = "semantic-observe")]
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
    /// Request id bound by the capturing execution unit.
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
#[cfg(feature = "semantic-observe")]
pub struct SinkRegistration {
    handle: SinkHandle,
    workspace: std::sync::Weak<dyn crate::WorkspaceAccess>,
}

#[cfg(feature = "semantic-observe")]
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

#[cfg(feature = "semantic-observe")]
impl Drop for SinkRegistration {
    fn drop(&mut self) {
        if let Some(ws) = self.workspace.upgrade() {
            let _ = ws.deregister_audit_sink(self.handle);
        }
    }
}

/// Callback surface for workspace audit fan-out.
#[cfg(feature = "semantic-observe")]
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

/// The audit sinks one workspace fans its reads out to. Zero-size without
/// `semantic-observe`.
#[derive(Default)]
pub(crate) struct VfsAuditSinks {
    #[cfg(feature = "semantic-observe")]
    sinks: parking_lot::RwLock<Vec<(SinkHandle, Arc<dyn VfsAuditSink>)>>,
    #[cfg(feature = "semantic-observe")]
    next_id: std::sync::atomic::AtomicU64,
}

#[cfg(feature = "semantic-observe")]
impl VfsAuditSinks {
    /// Register `sink`, returning its handle.
    pub(crate) fn register(&self, sink: Arc<dyn VfsAuditSink>) -> SinkHandle {
        let handle = SinkHandle(
            self.next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1,
        );
        self.sinks.write().push((handle, sink));
        handle
    }

    /// Remove the sink registered under `handle`.
    pub(crate) fn deregister(&self, handle: SinkHandle) -> Result<(), AuditSinkError> {
        let mut sinks = self.sinks.write();
        let len_before = sinks.len();
        sinks.retain(|(h, _)| *h != handle);
        if sinks.len() < len_before {
            Ok(())
        } else {
            Err(AuditSinkError::HandleNotFound)
        }
    }
}

/// One VFS read's optional observation: the per-file read clock (when the
/// active request asked for timing) and the fan-out to registered sinks.
/// Zero-size, reading neither the clock nor the request context, without
/// `semantic-observe`.
pub(crate) struct VfsReadProbe<M: verter_audit::observe::ObserveExec> {
    mode: std::marker::PhantomData<M>,
    #[cfg(feature = "semantic-observe")]
    started: Option<verter_audit::instant::Instant>,
    #[cfg(feature = "semantic-observe")]
    request_id: Option<u64>,
}

/// Capture data bound by a request or scheduler job at its execution boundary.
/// A read borrows it directly; neither the clock decision nor the request id
/// is recovered from thread-local state at emission time.
#[derive(Clone, Copy, Debug)]
pub struct VfsReadCapture {
    /// Root request owning these observations.
    pub request_id: u64,
    /// Whether the root requested per-file timing.
    pub timing: bool,
}

impl<M: verter_audit::observe::ObserveExec> VfsReadProbe<M> {
    /// Begin observing one read.
    #[inline(always)]
    pub(crate) fn start(_capture: Option<&VfsReadCapture>) -> Self {
        #[cfg(feature = "semantic-observe")]
        let capture = if M::CAPTURED { _capture } else { None };
        Self {
            mode: std::marker::PhantomData,
            #[cfg(feature = "semantic-observe")]
            started: capture
                .filter(|capture| capture.timing)
                .map(|_| verter_audit::instant::Instant::now()),
            #[cfg(feature = "semantic-observe")]
            request_id: capture.map(|capture| capture.request_id),
        }
    }

    /// Fan the completed read out to every registered sink.
    #[inline(always)]
    pub(crate) fn emit(
        &self,
        sinks: &VfsAuditSinks,
        canonical_id: &str,
        layer: VfsAuditLayer,
        cache_hit: bool,
        bytes_read: u64,
    ) {
        #[cfg(feature = "semantic-observe")]
        {
            if !M::CAPTURED {
                return;
            }
            let registered = sinks.sinks.read();
            if registered.is_empty() {
                return;
            }
            let event = VfsReadEvent {
                canonical_id: Arc::from(canonical_id),
                layer,
                cache_hit,
                bytes_read,
                read_ns: self
                    .started
                    .map(|t| t.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64),
                request_id: self.request_id,
                thread_id: std::thread::current().id(),
            };
            for (_, sink) in registered.iter() {
                sink.on_vfs_read(&event);
            }
        }
        #[cfg(not(feature = "semantic-observe"))]
        let _ = (sinks, canonical_id, layer, cache_hit, bytes_read);
    }
}

#[cfg(all(test, feature = "semantic-observe"))]
mod tests {
    use super::*;
    use crate::WorkspaceRead;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    use verter_execution::request_context::{
        OpaqueContextGuard, OpaqueRequestContext, RequestContextLike, TlsUninstall,
    };

    struct UncapturedContext(AtomicUsize);
    impl RequestContextLike for UncapturedContext {
        fn request_id(&self) -> u64 {
            71
        }
        fn timing_enabled(&self) -> bool {
            self.0.fetch_add(1, Relaxed);
            false
        }
        fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
            unreachable!()
        }
    }

    #[test]
    fn uncaptured_vfs_reads_do_not_poll_request_timing() {
        let ctx = Arc::new(UncapturedContext(AtomicUsize::new(0)));
        let _guard = OpaqueContextGuard::install(OpaqueRequestContext(ctx.clone()));
        let memory = crate::MemoryWorkspace::new(crate::MemoryOptions::default());
        for _ in 0..3 {
            assert!(memory.read_file("/missing.ts").is_none());
        }
        let filesystem = crate::FilesystemWorkspace::new(crate::FilesystemOptions::default());
        let directory = tempfile::tempdir().unwrap();
        let missing = directory
            .path()
            .join("missing.ts")
            .to_string_lossy()
            .replace('\\', "/");
        for _ in 0..3 {
            assert!(filesystem.read_file(&missing).is_none());
        }
        assert_eq!(
            ctx.0.load(Relaxed),
            0,
            "uncaptured reads must not query optional context state"
        );
    }

    #[test]
    fn captured_vfs_reads_use_bound_identity_and_timing() {
        use crate::WorkspaceAccess;
        #[derive(Default)]
        struct Sink(parking_lot::Mutex<Vec<VfsReadEvent>>);
        impl VfsAuditSink for Sink {
            fn on_vfs_read(&self, event: &VfsReadEvent) {
                self.0.lock().push(event.clone());
            }
        }
        let ctx = Arc::new(UncapturedContext(AtomicUsize::new(0)));
        let _guard = OpaqueContextGuard::install(OpaqueRequestContext(ctx.clone()));
        let directory = tempfile::tempdir().unwrap();
        let missing = directory
            .path()
            .join("missing.ts")
            .to_string_lossy()
            .replace('\\', "/");
        let workspaces: Vec<Arc<dyn WorkspaceAccess>> = vec![
            Arc::new(crate::MemoryWorkspace::new(crate::MemoryOptions::default())),
            Arc::new(crate::FilesystemWorkspace::new(
                crate::FilesystemOptions::default(),
            )),
        ];
        for workspace in workspaces {
            let sink = Arc::new(Sink::default());
            let handle = workspace.register_audit_sink(sink.clone()).unwrap();
            let _registration = SinkRegistration::new(handle, Arc::downgrade(&workspace));
            workspace.notify_upsert("/bound.ts", Arc::from("bound content"));
            let capture = VfsReadCapture {
                request_id: 42,
                timing: false,
            };
            assert_eq!(
                workspace
                    .read_file_captured("/bound.ts", &capture)
                    .as_deref(),
                Some("bound content")
            );
            assert!(workspace
                .read_file_captured(
                    &missing,
                    &VfsReadCapture {
                        timing: true,
                        ..capture
                    }
                )
                .is_none());
            assert_eq!(
                workspace.read_file("/bound.ts").as_deref(),
                Some("bound content")
            );
            let events = sink.0.lock();
            assert_eq!(
                events.len(),
                2,
                "uncaptured reads emit nothing even with a registered sink"
            );
            assert_eq!(events[0].request_id, Some(42));
            assert_eq!(events[0].layer, VfsAuditLayer::Overlay);
            assert_eq!(events[0].bytes_read, 13);
            assert_eq!(events[0].read_ns, None);
            assert_eq!(events[1].request_id, Some(42));
            assert!(events[1].read_ns.is_some());
        }
        assert_eq!(
            ctx.0.load(Relaxed),
            0,
            "bound capture does not poll a foreign installed context"
        );
    }
}
