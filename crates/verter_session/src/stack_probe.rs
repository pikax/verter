//! THROWAWAY DIAGNOSTIC (perf/inv-opus): native stack-depth probe.
//!
//! Measures ABSOLUTE native stack consumption of the current thread (from a
//! base anchored at thread entry via [`set_thread_base`], falling back to the
//! first probe) and captures backtraces at rising thresholds so a repeating
//! frame cycle is observable without a debugger. Optionally appends the full
//! dispatch key sequence so a genuine `(slot, locator)` CYCLE can be told apart
//! from legitimate-but-deep instantiation.
//!
//! Env gates (all off by default, zero cost when unset):
//!   VERTER_STACK_PROBE=1                 enable
//!   VERTER_STACK_PROBE_MB=<n>            first backtrace threshold (default 4)
//!   VERTER_STACK_PROBE_EVERY_MB=<n>      another backtrace every n MiB (default 0 = once)
//!   VERTER_STACK_PROBE_MAX_CAPTURES=<n>  cap on backtraces per thread (default 3)
//!   VERTER_STACK_PROBE_KEYLOG=<path>     append "kib\tqdepth\tkey" for every probe
//!   VERTER_STACK_PROBE_KEYLOG_MIN_KIB=<n> only log keys past this depth (default 0)

use std::cell::Cell;
use std::sync::OnceLock;

thread_local! {
    /// Address of a stack local at thread entry (or the first probe).
    static BASE: Cell<usize> = const { Cell::new(0) };
    /// Highest reported 256 KiB bucket, so growth is logged once per step.
    static REPORTED_BUCKET: Cell<usize> = const { Cell::new(0) };
    /// Number of backtraces captured on this thread.
    static CAPTURES: Cell<usize> = const { Cell::new(0) };
    /// Next capture threshold in bytes for this thread.
    static NEXT_CAPTURE_AT: Cell<usize> = const { Cell::new(0) };
    /// Re-entrancy guard: the backtrace capture must not re-probe.
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
}

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VERTER_STACK_PROBE").is_some())
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

fn first_threshold_bytes() -> usize {
    static T: OnceLock<usize> = OnceLock::new();
    *T.get_or_init(|| env_usize("VERTER_STACK_PROBE_MB", 4) * 1024 * 1024)
}

fn every_bytes() -> usize {
    static T: OnceLock<usize> = OnceLock::new();
    *T.get_or_init(|| env_usize("VERTER_STACK_PROBE_EVERY_MB", 0) * 1024 * 1024)
}

fn max_captures() -> usize {
    static T: OnceLock<usize> = OnceLock::new();
    *T.get_or_init(|| env_usize("VERTER_STACK_PROBE_MAX_CAPTURES", 3))
}

fn keylog_path() -> Option<&'static str> {
    static P: OnceLock<Option<String>> = OnceLock::new();
    P.get_or_init(|| {
        std::env::var("VERTER_STACK_PROBE_KEYLOG")
            .ok()
            .filter(|s| !s.is_empty())
    })
    .as_deref()
}

fn keylog_min_kib() -> usize {
    static T: OnceLock<usize> = OnceLock::new();
    *T.get_or_init(|| env_usize("VERTER_STACK_PROBE_KEYLOG_MIN_KIB", 0))
}

/// Anchor this thread's stack base. Call as early as possible on a thread whose
/// absolute stack consumption should be measured.
pub(crate) fn set_thread_base() {
    if !enabled() {
        return;
    }
    let marker = 0u8;
    BASE.with(|b| b.set(std::ptr::addr_of!(marker) as usize));
    NEXT_CAPTURE_AT.with(|c| c.set(first_threshold_bytes()));
    eprintln!(
        "[stack-probe] BASE anchored thread={:?} name={:?}",
        std::thread::current().id(),
        std::thread::current().name().unwrap_or("-"),
    );
}

/// Unconditional stage marker: prints the current native stack usage every time
/// it is reached, so the LAST mark before a stack overflow names the stage that
/// entered the runaway recursion.
pub(crate) fn mark(label: &str) {
    if !enabled() || CAPTURING.with(|c| c.get()) {
        return;
    }
    // Global print cap so a runaway recursion produces a readable window rather
    // than gigabytes: the repeating unit is visible in the first few thousand.
    {
        static PRINTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        static CAP: OnceLock<usize> = OnceLock::new();
        let cap = *CAP.get_or_init(|| env_usize("VERTER_STACK_MARK_CAP", 4000));
        if PRINTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= cap {
            return;
        }
    }
    let marker = 0u8;
    let here = std::ptr::addr_of!(marker) as usize;
    let base = BASE.with(|b| b.get());
    let used = if base == 0 {
        0
    } else {
        base.saturating_sub(here)
    };
    CAPTURING.with(|c| c.set(true));
    eprintln!(
        "[stack-mark] {label} thread={:?} used_kib={}",
        std::thread::current().id(),
        used / 1024,
    );
    CAPTURING.with(|c| c.set(false));
}

/// Probe the current native stack usage.
///
/// `label` names the dispatch key (formatted only when something is logged) and
/// `query_depth` is the dispatcher's LOGICAL connected-query depth, so a
/// divergence between logical depth and native depth is directly visible.
pub(crate) fn probe(label: &dyn std::fmt::Debug, query_depth: u16) {
    if !enabled() || CAPTURING.with(|c| c.get()) {
        return;
    }
    let marker = 0u8;
    let here = std::ptr::addr_of!(marker) as usize;
    let base = BASE.with(|b| {
        if b.get() == 0 {
            b.set(here);
            NEXT_CAPTURE_AT.with(|c| c.set(first_threshold_bytes()));
        }
        b.get()
    });
    // Stacks grow downward on every platform this ships on.
    let used = base.saturating_sub(here);
    let used_kib = used / 1024;

    let bucket = used / (256 * 1024);
    let grew = bucket > REPORTED_BUCKET.with(|r| r.get());
    let next = NEXT_CAPTURE_AT.with(|c| c.get());
    let capture = next != 0 && used >= next && CAPTURES.with(|c| c.get()) < max_captures();
    let want_keylog = keylog_path().is_some() && used_kib >= keylog_min_kib();
    if !grew && !capture && !want_keylog {
        return;
    }

    // Everything below ALLOCATES (formatting, file IO, backtrace). Hold the
    // re-entrancy guard so the allocator-sampling probe cannot recurse into us.
    CAPTURING.with(|c| c.set(true));

    if want_keylog {
        if let Some(path) = keylog_path() {
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{used_kib}\t{query_depth}\t{label:?}");
            }
        }
    }

    if grew {
        REPORTED_BUCKET.with(|r| r.set(bucket));
        eprintln!(
            "[stack-probe] thread={:?} native_stack_used_kib={} query_depth={} at={:?}",
            std::thread::current().id(),
            used_kib,
            query_depth,
            label,
        );
    }

    if capture {
        CAPTURES.with(|c| c.set(c.get() + 1));
        NEXT_CAPTURE_AT.with(|c| {
            c.set(if every_bytes() == 0 {
                0
            } else {
                used + every_bytes()
            })
        });
        let bt = std::backtrace::Backtrace::force_capture();
        eprintln!(
            "[stack-probe] THRESHOLD #{} thread={:?} used_kib={} query_depth={} key={:?}\n\
             ===BEGIN-STACK-PROBE-BACKTRACE===\n{bt}\n===END-STACK-PROBE-BACKTRACE===",
            CAPTURES.with(|c| c.get()),
            std::thread::current().id(),
            used_kib,
            query_depth,
            label,
        );
    }

    CAPTURING.with(|c| c.set(false));
}

thread_local! {
    /// Allocation counter for the sampling allocator probe.
    static ALLOC_TICK: Cell<u32> = const { Cell::new(0) };
    /// Re-entrancy guard for the allocator hook. EVERYTHING inside the hook may
    /// allocate (env reads behind `OnceLock`, formatting, backtrace capture), and
    /// a re-entrant `OnceLock::get_or_init` on the same thread DEADLOCKS. The
    /// guard must therefore be taken before touching any of them.
    static IN_ALLOC: Cell<bool> = const { Cell::new(false) };
}

fn alloc_sample_every() -> u32 {
    static T: OnceLock<u32> = OnceLock::new();
    *T.get_or_init(|| {
        std::env::var("VERTER_STACK_PROBE_ALLOC_EVERY")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(0)
    })
}

/// Sampling probe driven from a `GlobalAlloc` wrapper. This is the UNIVERSAL
/// catcher: any deep recursion that allocates is observed regardless of which
/// module it lives in. No-op unless `VERTER_STACK_PROBE_ALLOC_EVERY` is set.
pub(crate) fn probe_alloc() {
    // Only threads whose base was explicitly anchored are sampled: that is the
    // serve thread, which is the one under investigation, and it keeps the hook
    // off every worker/allocation-heavy background thread.
    if IN_ALLOC.with(|c| c.get()) || BASE.with(|b| b.get()) == 0 {
        return;
    }
    IN_ALLOC.with(|c| c.set(true));
    let every = alloc_sample_every();
    if every != 0 && enabled() && !CAPTURING.with(|c| c.get()) {
        let tick = ALLOC_TICK.with(|t| {
            let v = t.get().wrapping_add(1);
            t.set(v);
            v
        });
        if tick % every == 0 {
            probe(&"<alloc-sample>", u16::MAX);
        }
    }
    IN_ALLOC.with(|c| c.set(false));
}
