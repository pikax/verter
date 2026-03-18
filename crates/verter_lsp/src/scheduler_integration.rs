//! Scheduler integration for the LSP.
//!
//! Provides helper functions for using the scheduler's lock-free read paths
//! in LSP request handlers (hover, completion, diagnostics, etc.).
//!
//! The scheduler runs alongside the host's existing `files: Shared<FxHashMap>`.
//! LSP handlers can use these helpers for fast, contention-free reads:
//!
//! - `scheduler_source()` — read parsed source without the global files lock
//! - `scheduler_analysis()` — read analysis data without the global files lock
//!
//! The host's `upsert()` populates both the legacy `files` map and the scheduler
//! in parallel, so both read paths return consistent data.

use std::sync::Arc;

use verter_host::VerterHost;
use verter_scheduler::node::SourceSnapshot;
use verter_scheduler::scheduler::Request;
use verter_scheduler::stage::{Priority, TargetStage};

/// Priority mapping for LSP operations.
///
/// Maps LSP request types to scheduler priority tiers:
/// - `Critical` — blocking the user (hover, completion, go-to-definition)
/// - `Interactive` — user-triggered (did_open, did_change)
/// - `Background` — workspace scanner, background compilation
pub fn lsp_priority_for_hover() -> Priority {
    Priority::Critical
}

pub fn lsp_priority_for_completion() -> Priority {
    Priority::Critical
}

pub fn lsp_priority_for_did_open() -> Priority {
    Priority::Interactive
}

pub fn lsp_priority_for_did_change() -> Priority {
    Priority::Interactive
}

pub fn lsp_priority_for_workspace_scan() -> Priority {
    Priority::Background
}

/// Submit a file to the scheduler at the appropriate LSP priority.
///
/// This is used alongside `host.upsert()` — the scheduler tracks generation
/// and priority while the host handles the full upsert logic.
pub fn submit_to_scheduler(host: &VerterHost, file_id: &str, source: Arc<str>, priority: Priority) {
    let scheduler = host.scheduler();
    let _handle = scheduler.submit_request(Request {
        file_id: file_id.to_string(),
        target: TargetStage::Analysis,
        priority,
        source: Some(source),
    });
    scheduler.drive_all();
}

/// Read source content from the scheduler (lock-free).
///
/// Falls back to `None` if the file hasn't been upserted or the snapshot is stale.
pub fn read_source(host: &VerterHost, file_id: &str) -> Option<Arc<SourceSnapshot>> {
    host.scheduler_source(file_id)
}
