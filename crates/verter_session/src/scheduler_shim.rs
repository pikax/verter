//! SchedulerBackedWorkspace — full-fidelity migration shim.
//!
//! Implements `WorkspaceAccess` on top of the scheduler's generation-current
//! snapshots with a disk fallback for arbitrary file reads (configs, .d.ts, etc.).

#[cfg(feature = "scheduler")]
use std::sync::Arc;

#[cfg(feature = "scheduler")]
use verter_scheduler::scheduler::Scheduler;
#[cfg(feature = "scheduler")]
use verter_scheduler::source_loader::SourceLoader;
#[cfg(feature = "scheduler")]
use verter_workspace::types::FileKind;
#[cfg(feature = "scheduler")]
use verter_workspace::WorkspaceAccess;

/// Workspace shim that serves generation-current content from the scheduler,
/// falling back to disk for files not loaded into the scheduler.
///
/// This is a temporary migration bridge. Removed in Phase 8.
#[cfg(feature = "scheduler")]
pub struct SchedulerBackedWorkspace {
    pub scheduler: Arc<Scheduler>,
    pub disk_fallback: Arc<dyn SourceLoader>,
}

#[cfg(feature = "scheduler")]
impl WorkspaceAccess for SchedulerBackedWorkspace {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        // Check scheduler's generation-current source snapshot
        if let Some(src) = self.scheduler.try_get_source(canonical_id) {
            return Some(src.source.clone());
        }
        // Fall back to disk for arbitrary file reads
        self.disk_fallback.load(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.scheduler.has_node(canonical_id) || self.disk_fallback.exists(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.disk_fallback.realpath(canonical_id)
    }

    fn classify_file(&self, canonical_id: &str) -> FileKind {
        if canonical_id.ends_with(".vue") {
            FileKind::VueSfc
        } else {
            FileKind::NonSfc
        }
    }
}
