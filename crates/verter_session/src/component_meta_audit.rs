//! Rust-first native audit surface for component-meta requests.
//!
//! Gated by `HostConfig::audit_enabled`. When off, the runtime stays on the
//! zero-overhead default path. When on, timing/memory/store snapshots are
//! captured per request and emitted as a structured `RustAuditRecord`.
//!
//! This module owns the canonical audit record types. JS benchmark/harness
//! audit is a separate concern — it does not inline or redefine these types.

use std::cell::RefCell;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Audit record types
// ---------------------------------------------------------------------------

/// Top-level audit record for one component-meta request.
#[derive(Debug, Clone)]
pub struct RustAuditRecord {
    pub request_id: u64,
    pub canonical_id: String,
    pub timings: RustTimingAudit,
    pub solver: RustSolverAudit,
    pub store: RustStoreAudit,
    pub memory: RustMemoryAudit,
}

/// Phase timings in milliseconds.
#[derive(Debug, Clone, Default)]
pub struct RustTimingAudit {
    pub total_ms: f64,
    pub capture_inputs_ms: f64,
    pub store_read_ms: f64,
    pub store_merge_ms: f64,
    pub direct_import_proof_ms: f64,
    pub imported_root_proof_ms: f64,
    pub solver_ms: f64,
    pub materialize_ms: f64,
    pub serialize_ms: f64,
}

/// Solver-level counters from `SolverResult.steps`.
#[derive(Debug, Clone, Default)]
pub struct RustSolverAudit {
    /// Total resolve steps across all solves in this request.
    pub total_resolve_steps: u64,
    /// Number of distinct solves performed.
    pub solve_count: u32,
}

/// Store/view counters.
#[derive(Debug, Clone, Default)]
pub struct RustStoreAudit {
    pub store_view_hits: u32,
    pub store_view_misses: u32,
    pub structural_merges: u32,
    pub imported_dependency_entries: u32,
    pub imported_dependency_bytes: u64,
    pub prepared_type_decls: u32,
    pub prepared_value_decls: u32,
}

/// Memory snapshots.
#[derive(Debug, Clone, Default)]
pub struct RustMemoryAudit {
    pub process_rss_before_bytes: u64,
    pub process_rss_after_bytes: u64,
    pub process_rss_delta_bytes: i64,
    pub host_cache_before_bytes: u64,
    pub host_cache_after_bytes: u64,
    pub workspace_before_bytes: u64,
    pub workspace_after_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RequestPhaseAudit {
    pub imported_root_proof_ms: f64,
}

// ---------------------------------------------------------------------------
// Audit builder — accumulates data during a request
// ---------------------------------------------------------------------------

/// Builder for accumulating audit data during a component-meta request.
/// Created only when `audit_enabled` is true.
pub struct AuditBuilder {
    request_id: u64,
    canonical_id: String,
    request_start: Instant,
    phase_start: Instant,
    timings: RustTimingAudit,
    solver: RustSolverAudit,
    store: RustStoreAudit,
    memory: RustMemoryAudit,
}

impl AuditBuilder {
    pub fn new(request_id: u64, canonical_id: String) -> Self {
        let now = Instant::now();
        let rss = current_process_rss();
        Self {
            request_id,
            canonical_id,
            request_start: now,
            phase_start: now,
            timings: RustTimingAudit::default(),
            solver: RustSolverAudit::default(),
            store: RustStoreAudit::default(),
            memory: RustMemoryAudit {
                process_rss_before_bytes: rss,
                ..Default::default()
            },
        }
    }

    /// Mark the end of the current phase and start the next one.
    pub fn end_phase(&mut self, phase: AuditPhase) {
        let elapsed = self.phase_start.elapsed().as_secs_f64() * 1000.0;
        match phase {
            AuditPhase::CaptureInputs => self.timings.capture_inputs_ms = elapsed,
            AuditPhase::StoreRead => self.timings.store_read_ms = elapsed,
            AuditPhase::StoreMerge => self.timings.store_merge_ms = elapsed,
            AuditPhase::DirectImportProof => self.timings.direct_import_proof_ms = elapsed,
            AuditPhase::ImportedRootProof => self.timings.imported_root_proof_ms = elapsed,
            AuditPhase::Solver => self.timings.solver_ms = elapsed,
            AuditPhase::Materialize => self.timings.materialize_ms = elapsed,
            AuditPhase::Serialize => self.timings.serialize_ms = elapsed,
        }
        self.phase_start = Instant::now();
    }

    /// Record solver steps from a completed solve.
    pub fn record_solver_steps(&mut self, steps: u64) {
        self.solver.total_resolve_steps += steps;
        self.solver.solve_count += 1;
    }

    /// Record store counters.
    pub fn record_store(&mut self, store: RustStoreAudit) {
        self.store = store;
    }

    /// Record host/workspace memory snapshots taken outside the builder.
    pub fn record_memory_snapshots(
        &mut self,
        host_cache_before_bytes: u64,
        host_cache_after_bytes: u64,
        workspace_before_bytes: u64,
        workspace_after_bytes: u64,
    ) {
        self.memory.host_cache_before_bytes = host_cache_before_bytes;
        self.memory.host_cache_after_bytes = host_cache_after_bytes;
        self.memory.workspace_before_bytes = workspace_before_bytes;
        self.memory.workspace_after_bytes = workspace_after_bytes;
    }

    /// Replace timing fields captured by deeper request stages.
    pub fn record_timings(&mut self, timings: RustTimingAudit) {
        self.timings = timings;
    }

    /// Replace solver counters captured by deeper request stages.
    pub fn record_solver(&mut self, solver: RustSolverAudit) {
        self.solver = solver;
    }

    /// Finalize and return the audit record.
    pub fn finish(mut self) -> RustAuditRecord {
        self.timings.total_ms = self.request_start.elapsed().as_secs_f64() * 1000.0;
        self.memory.process_rss_after_bytes = current_process_rss();
        self.memory.process_rss_delta_bytes = self.memory.process_rss_after_bytes as i64
            - self.memory.process_rss_before_bytes as i64;

        RustAuditRecord {
            request_id: self.request_id,
            canonical_id: self.canonical_id,
            timings: self.timings,
            solver: self.solver,
            store: self.store,
            memory: self.memory,
        }
    }
}

/// Named phases for timing capture.
#[derive(Debug, Clone, Copy)]
pub enum AuditPhase {
    CaptureInputs,
    StoreRead,
    StoreMerge,
    DirectImportProof,
    ImportedRootProof,
    Solver,
    Materialize,
    Serialize,
}

thread_local! {
    static ACTIVE_REQUEST_AUDIT: RefCell<Vec<(u64, RequestPhaseAudit)>> = const { RefCell::new(Vec::new()) };
}

pub struct RequestAuditGuard {
    request_id: u64,
}

impl RequestAuditGuard {
    pub fn snapshot(&self) -> RequestPhaseAudit {
        ACTIVE_REQUEST_AUDIT.with(|stack| {
            stack
                .borrow()
                .iter()
                .rev()
                .find(|(request_id, _)| *request_id == self.request_id)
                .map(|(_, audit)| audit.clone())
                .unwrap_or_default()
        })
    }
}

impl Drop for RequestAuditGuard {
    fn drop(&mut self) {
        ACTIVE_REQUEST_AUDIT.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(position) = stack
                .iter()
                .rposition(|(request_id, _)| *request_id == self.request_id)
            {
                stack.remove(position);
            }
        });
    }
}

pub fn begin_request_audit(request_id: u64) -> RequestAuditGuard {
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        stack
            .borrow_mut()
            .push((request_id, RequestPhaseAudit::default()));
    });
    RequestAuditGuard { request_id }
}

pub fn record_imported_root_proof_ms(elapsed_ms: f64) {
    if elapsed_ms <= 0.0 {
        return;
    }
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        if let Some((_, audit)) = stack.borrow_mut().last_mut() {
            audit.imported_root_proof_ms += elapsed_ms;
        }
    });
}

pub fn current_request_audit_snapshot() -> RequestPhaseAudit {
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        stack
            .borrow()
            .last()
            .map(|(_, audit)| audit.clone())
            .unwrap_or_default()
    })
}

// ---------------------------------------------------------------------------
// Process memory snapshot
// ---------------------------------------------------------------------------

/// Get current process RSS in bytes. Returns 0 if unavailable.
fn current_process_rss() -> u64 {
    // Linux: read from /proc/self/statm
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(rss_pages) = statm.split_whitespace().nth(1) {
                if let Ok(pages) = rss_pages.parse::<u64>() {
                    return pages * 4096;
                }
            }
        }
    }
    // macOS: use rusage
    #[cfg(target_os = "macos")]
    {
        let mut usage = std::mem::MaybeUninit::<libc_rusage>::uninit();
        // SAFETY: getrusage is a POSIX function that fills the provided struct.
        let ret = unsafe {
            getrusage(0 /* RUSAGE_SELF */, usage.as_mut_ptr())
        };
        if ret == 0 {
            let usage = unsafe { usage.assume_init() };
            // On macOS, ru_maxrss is in bytes (not KB).
            return usage.ru_maxrss as u64;
        }
    }
    0
}

// Minimal rusage FFI for macOS (avoids libc crate dependency).
#[cfg(target_os = "macos")]
#[repr(C)]
#[allow(non_camel_case_types)]
struct libc_rusage {
    ru_utime: [i64; 2],
    ru_stime: [i64; 2],
    ru_maxrss: i64,
    _pad: [i64; 13],
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn getrusage(who: i32, usage: *mut libc_rusage) -> i32;
}

// ---------------------------------------------------------------------------
// Trace emission
// ---------------------------------------------------------------------------

/// Emit an audit record via the component-meta trace system.
pub fn emit_audit_trace(record: &RustAuditRecord) {
    // Use the existing component_meta_trace_event! macro if tracing is active
    let detail = format!(
        "request_id={} canonical={} total_ms={:.2} solver_ms={:.2} solver_steps={} solve_count={} \
         capture_inputs_ms={:.2} store_read_ms={:.2} store_merge_ms={:.2} \
         direct_import_proof_ms={:.2} imported_root_proof_ms={:.2} \
         materialize_ms={:.2} serialize_ms={:.2} \
         rss_before={}B rss_after={}B rss_delta={}B \
         host_cache_before={}B host_cache_after={}B \
         workspace_before={}B workspace_after={}B \
         store_view_hits={} store_view_misses={} structural_merges={} \
         imported_dep_entries={} imported_dep_bytes={} prepared_type_decls={} prepared_value_decls={}",
        record.request_id,
        record.canonical_id,
        record.timings.total_ms,
        record.timings.solver_ms,
        record.solver.total_resolve_steps,
        record.solver.solve_count,
        record.timings.capture_inputs_ms,
        record.timings.store_read_ms,
        record.timings.store_merge_ms,
        record.timings.direct_import_proof_ms,
        record.timings.imported_root_proof_ms,
        record.timings.materialize_ms,
        record.timings.serialize_ms,
        record.memory.process_rss_before_bytes,
        record.memory.process_rss_after_bytes,
        record.memory.process_rss_delta_bytes,
        record.memory.host_cache_before_bytes,
        record.memory.host_cache_after_bytes,
        record.memory.workspace_before_bytes,
        record.memory.workspace_after_bytes,
        record.store.store_view_hits,
        record.store.store_view_misses,
        record.store.structural_merges,
        record.store.imported_dependency_entries,
        record.store.imported_dependency_bytes,
        record.store.prepared_type_decls,
        record.store.prepared_value_decls,
    );
    // Use stderr to avoid polluting stdout; the existing component_meta_trace
    // system also writes to stderr or a file path.
    eprintln!("[verter-rust-audit] {detail}");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_builder_captures_total_timing() {
        let builder = AuditBuilder::new(1, "test.vue".into());
        std::thread::sleep(std::time::Duration::from_millis(5));
        let record = builder.finish();

        assert!(
            record.timings.total_ms >= 4.0,
            "total_ms should be >= 4ms, got {}",
            record.timings.total_ms
        );
        assert_eq!(record.request_id, 1);
        assert_eq!(record.canonical_id, "test.vue");
    }

    #[test]
    fn audit_builder_records_solver_steps() {
        let mut builder = AuditBuilder::new(2, "component.vue".into());
        builder.record_solver_steps(42);
        builder.record_solver_steps(100);
        let record = builder.finish();

        assert_eq!(record.solver.total_resolve_steps, 142);
        assert_eq!(record.solver.solve_count, 2);
    }

    #[test]
    fn audit_builder_captures_phase_timings() {
        let mut builder = AuditBuilder::new(3, "phased.vue".into());
        std::thread::sleep(std::time::Duration::from_millis(2));
        builder.end_phase(AuditPhase::CaptureInputs);
        std::thread::sleep(std::time::Duration::from_millis(2));
        builder.end_phase(AuditPhase::Solver);
        let record = builder.finish();

        assert!(record.timings.capture_inputs_ms >= 1.0);
        assert!(record.timings.solver_ms >= 1.0);
        // Phases we didn't touch should be 0
        assert_eq!(record.timings.store_read_ms, 0.0);
        assert_eq!(record.timings.serialize_ms, 0.0);
    }

    #[test]
    fn audit_builder_captures_process_rss() {
        let builder = AuditBuilder::new(4, "rss.vue".into());
        let record = builder.finish();

        // On supported platforms (macOS, Linux), RSS should be > 0.
        // On unsupported platforms, it's 0 which is acceptable.
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(
            record.memory.process_rss_before_bytes > 0,
            "RSS before should be > 0"
        );
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(
            record.memory.process_rss_after_bytes > 0,
            "RSS after should be > 0"
        );
    }

    #[test]
    fn audit_default_host_config_is_off() {
        let config = crate::HostConfig::default();
        assert!(
            !config.audit_enabled,
            "audit_enabled should default to false"
        );
    }

    #[test]
    fn solver_result_carries_steps() {
        use verter_semantic::analysis::type_solver::result::SolverResult;

        let result = SolverResult::exact_concrete(42);
        assert_eq!(result.steps, 0, "constructor should initialize steps to 0");

        let mapped = result.map(|v| v + 1);
        assert_eq!(mapped.steps, 0, "map should preserve steps");
    }
}
