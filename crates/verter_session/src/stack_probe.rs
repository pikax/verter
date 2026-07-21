//! THROWAWAY DIAGNOSTIC (perf/inv-opus): native stack-depth probe.
//!
//! Measures how much native stack the shared cold-build dispatch has consumed
//! on the current thread and, once a threshold is crossed, captures ONE
//! backtrace so the repeating frame cycle behind a stack overflow is
//! observable without a debugger.
//!
//! Entirely env-gated by `VERTER_STACK_PROBE=1`; `VERTER_STACK_PROBE_MB`
//! selects the capture threshold (default 4 MiB). Remove with the rest of the
//! investigation instrumentation.

use std::cell::Cell;
use std::sync::OnceLock;

thread_local! {
    /// Address of a stack local at the OUTERMOST probed frame on this thread.
    static BASE: Cell<usize> = const { Cell::new(0) };
    /// Highest reported 512 KiB bucket, so growth is logged once per step.
    static REPORTED_BUCKET: Cell<usize> = const { Cell::new(0) };
    /// One backtrace per thread.
    static CAPTURED: Cell<bool> = const { Cell::new(false) };
    /// Re-entrancy guard: the backtrace capture itself must not re-probe.
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
}

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VERTER_STACK_PROBE").is_some())
}

fn threshold_bytes() -> usize {
    static T: OnceLock<usize> = OnceLock::new();
    *T.get_or_init(|| {
        std::env::var("VERTER_STACK_PROBE_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(4)
            * 1024
            * 1024
    })
}

/// Probe the current native stack usage from the outermost probed frame.
///
/// `label` names the dispatch key (or any caller-supplied identity) and
/// `query_depth` is the dispatcher's logical connected-query depth, so a
/// divergence between LOGICAL depth and NATIVE depth is directly visible.
pub(crate) fn probe(label: &dyn std::fmt::Debug, query_depth: u16) {
    if !enabled() {
        return;
    }
    if CAPTURING.with(|c| c.get()) {
        return;
    }
    let marker = 0u8;
    let here = std::ptr::addr_of!(marker) as usize;
    let base = BASE.with(|b| {
        if b.get() == 0 {
            b.set(here);
        }
        b.get()
    });
    // Stacks grow downward on every platform this ships on.
    let used = base.saturating_sub(here);
    let bucket = used / (512 * 1024);
    let prev = REPORTED_BUCKET.with(|r| r.get());
    if bucket > prev {
        REPORTED_BUCKET.with(|r| r.set(bucket));
        eprintln!(
            "[stack-probe] thread={:?} native_stack_used_kib={} query_depth={} key={:?}",
            std::thread::current().id(),
            used / 1024,
            query_depth,
            label,
        );
    }
    if used >= threshold_bytes() && !CAPTURED.with(|c| c.get()) {
        CAPTURED.with(|c| c.set(true));
        CAPTURING.with(|c| c.set(true));
        let bt = std::backtrace::Backtrace::force_capture();
        eprintln!(
            "[stack-probe] THRESHOLD thread={:?} used_kib={} query_depth={} key={:?}\n\
             ===BEGIN-STACK-PROBE-BACKTRACE===\n{bt}\n===END-STACK-PROBE-BACKTRACE===",
            std::thread::current().id(),
            used / 1024,
            query_depth,
            label,
        );
        CAPTURING.with(|c| c.set(false));
    }
}
