//! SchedulerBackedWorkspace — full-fidelity migration shim.
//!
//! Implements `WorkspaceAccess` on top of the scheduler's generation-current
//! snapshots with a disk fallback for arbitrary file reads (configs, .d.ts, etc.).

#[cfg(not(target_arch = "wasm32"))]
use std::cell::RefCell;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeSet;
#[cfg(not(target_arch = "wasm32"))]
use verter_scheduler::scheduler::Scheduler;
#[cfg(not(target_arch = "wasm32"))]
use verter_scheduler::source_loader::SourceLoader;
#[cfg(not(target_arch = "wasm32"))]
use verter_workspace::types::FileKind;
#[cfg(not(target_arch = "wasm32"))]
use verter_workspace::types::{ExactResolution, ExactResolutionResult, ParsedEdge};
#[cfg(not(target_arch = "wasm32"))]
use verter_workspace::DependencySnapshotView;
#[cfg(not(target_arch = "wasm32"))]
use verter_workspace::WorkspaceAccess;

/// Workspace shim that serves generation-current content from the scheduler,
/// falling back to disk for files not loaded into the scheduler.
///
/// This is a temporary migration bridge. Removed in Phase 8.
#[cfg(not(target_arch = "wasm32"))]
pub struct SchedulerBackedWorkspace {
    pub scheduler: Arc<Scheduler>,
    pub disk_fallback: Arc<dyn SourceLoader>,
}

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static LAST_READ_FILE_TRACE_DETAIL: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

#[cfg(not(target_arch = "wasm32"))]
fn set_last_read_file_trace_detail(canonical_id: &str, detail: impl Into<String>) {
    LAST_READ_FILE_TRACE_DETAIL.with(|last| {
        *last.borrow_mut() = Some((canonical_id.to_string(), detail.into()));
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn take_last_read_file_trace_detail(canonical_id: &str) -> Option<String> {
    LAST_READ_FILE_TRACE_DETAIL.with(|last| {
        let mut last = last.borrow_mut();
        match last.as_ref() {
            Some((seen_canonical, _)) if seen_canonical == canonical_id => {
                last.take().map(|(_, detail)| detail)
            }
            _ => None,
        }
    })
}

#[cfg(not(target_arch = "wasm32"))]
impl WorkspaceAccess for SchedulerBackedWorkspace {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        // Check scheduler's generation-current source snapshot
        if let Some(src) = self.scheduler.try_get_source(canonical_id) {
            set_last_read_file_trace_detail(canonical_id, "layer=scheduler cache=hit");
            return Some(src.source.clone());
        }
        // Fall back to disk for arbitrary file reads
        let loaded = self.disk_fallback.load(canonical_id);
        if loaded.is_some() {
            set_last_read_file_trace_detail(canonical_id, "layer=disk-fallback cache=unknown");
        } else {
            set_last_read_file_trace_detail(canonical_id, "layer=missing cache=miss");
        }
        loaded
    }

    fn take_last_read_file_trace_detail(&self, canonical_id: &str) -> Option<String> {
        take_last_read_file_trace_detail(canonical_id)
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

    // ── Reader-only stub overrides for reverse-graph methods (R6/R7) ──
    //
    // Rationale (§2.16a/§2.16b): `SchedulerBackedWorkspace` is used only in
    // scheduler test fixtures that exercise scheduler-level concerns (parsing,
    // source loading); host integration tests use `MemoryWorkspace` instead.
    // If a scheduler test begins exercising dep-flow, this shim must be
    // replaced with a real `MemoryWorkspace` for that test. The explicit no-op
    // bodies make the absence of edge tracking visible at impl-site.

    fn record_parsed_edges(&self, _canonical_id: &str, _edges: &[ParsedEdge]) {
        // No edge tracking. See §2.16b reader-only-stub rationale.
    }

    fn reverse_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    fn forward_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    fn set_exact_resolutions(
        &self,
        _canonical_id: &str,
        _resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult {
        ExactResolutionResult::default()
    }

    fn replace_semantic_transitive(&self, _canonical_id: &str, _deps: BTreeSet<String>) {}

    fn set_default_resolve_extensions(&self, _host_extensions: Vec<String>) {}

    fn dependency_snapshot(&self, _canonical_id: &str) -> Option<DependencySnapshotView> {
        None
    }

    fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use verter_scheduler::scheduler::{Request, SchedulerConfig};
    use verter_scheduler::source_loader::MemorySourceLoader;
    use verter_scheduler::stage::{Priority, TargetStage};

    #[test]
    fn scheduler_backed_workspace_reports_scheduler_hit_trace_detail() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/a.vue".to_string(), Arc::from("content"));
        let scheduler = Scheduler::new_sync(SchedulerConfig::default(), loader.clone());
        scheduler.submit_request(Request {
            file_id: "/a.vue".to_string(),
            target: TargetStage::Source,
            priority: Priority::Interactive,
            source: None,
            file_kind: None,
            request_context: None,
        });
        scheduler.drive_all();

        let ws = SchedulerBackedWorkspace {
            scheduler,
            disk_fallback: loader,
        };

        assert_eq!(ws.read_file("/a.vue").as_deref(), Some("content"));
        assert_eq!(
            ws.take_last_read_file_trace_detail("/a.vue").as_deref(),
            Some("layer=scheduler cache=hit"),
        );
    }

    #[test]
    fn scheduler_backed_workspace_reports_fallback_trace_detail() {
        let loader = Arc::new(MemorySourceLoader::new());
        loader.insert("/b.vue".to_string(), Arc::from("fallback"));
        let scheduler = Scheduler::new_sync(SchedulerConfig::default(), loader.clone());
        let ws = SchedulerBackedWorkspace {
            scheduler,
            disk_fallback: loader,
        };

        assert_eq!(ws.read_file("/b.vue").as_deref(), Some("fallback"));
        assert_eq!(
            ws.take_last_read_file_trace_detail("/b.vue").as_deref(),
            Some("layer=disk-fallback cache=unknown"),
        );
        assert!(ws.scheduler.try_get_source("/b.vue").is_none());
    }
}
