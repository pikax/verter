//! CSS style-pipeline latency/allocation gate.
//!
//! `capture` measures every benchmark identity in the shared universe
//! (`verter_bench::css_identities`) against the style pipeline this binary was
//! compiled with, plus the per-generator-category allocation counts, and
//! writes one provenance-stamped, integrity-sealed JSON record.
//!
//! `compare` reads a committed baseline record and a fresh candidate record,
//! proves the compiled-in identity universe, the baseline, and the candidate
//! are the same set, refuses on any measurement-protocol or environment
//! identity mismatch, and gates every identity's wall-clock median at 1.2x.
//!
//! The counting `#[global_allocator]` lives here because a global allocator is
//! process-global: the library supplies the measurement code and receives the
//! counter hooks.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::process::ExitCode;

use verter_bench::css_gate::{
    build_is_optimized, compare_records, run_capture, AllocHooks, ComparePolicy, CssBaselineRecord,
};
use verter_bench::css_identities::identity_universe;

// =============================================================================
// Counting allocator (thread-local counters; every measured path is
// synchronous on the main thread)
// =============================================================================

struct CountingAllocator;

thread_local! {
    static ALLOC_COUNTER: Cell<u64> = const { Cell::new(0) };
    static ALLOC_BYTES: Cell<u64> = const { Cell::new(0) };
}

fn increment_alloc_counter(bytes: usize) {
    // Allocation can occur while a thread is tearing down TLS. Do not turn
    // an otherwise valid allocation into a panic if this key is no longer
    // accessible.
    let _ = ALLOC_COUNTER.try_with(|counter| counter.set(counter.get().wrapping_add(1)));
    let _ = ALLOC_BYTES.try_with(|total| total.set(total.get().wrapping_add(bytes as u64)));
}

fn reset_alloc_counter() {
    ALLOC_COUNTER.with(|counter| counter.set(0));
    ALLOC_BYTES.with(|total| total.set(0));
}

fn read_alloc_counter() -> (u64, u64) {
    (ALLOC_COUNTER.with(Cell::get), ALLOC_BYTES.with(Cell::get))
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        increment_alloc_counter(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

// =============================================================================
// CLI
// =============================================================================

const USAGE: &str = "usage:
  css_latency_gate capture --out <path> [--pipeline <discriminant>] [--allow-unoptimized]
  css_latency_gate compare --baseline <path> --candidate <path> [--expect-transition <from>:<to>]

capture measures the shared benchmark-identity universe against the style
pipeline compiled into this binary and writes a provenance-stamped record;
compare gates a fresh candidate record against a committed baseline record.
A produce-then-gate run is `capture --out cand.json` followed by
`compare --baseline <committed> --candidate cand.json`.";

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("capture") => capture(&args[1..]),
        Some("compare") => compare(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn capture(args: &[String]) -> ExitCode {
    let Some(out) = arg_value(args, "--out") else {
        eprintln!("capture requires --out <path>\n{USAGE}");
        return ExitCode::from(2);
    };
    let pipeline =
        arg_value(args, "--pipeline").unwrap_or_else(|| "legacy-lightningcss".to_string());

    if !build_is_optimized() && !args.iter().any(|a| a == "--allow-unoptimized") {
        eprintln!(
            "REFUSED: this is an unoptimized (debug_assertions) build; a baseline must be an \
             optimized measurement. Build with --release, or pass --allow-unoptimized for a \
             throwaway local run."
        );
        return ExitCode::from(3);
    }

    let hooks = AllocHooks {
        reset: reset_alloc_counter,
        read: read_alloc_counter,
    };
    let record = run_capture(&pipeline, &hooks);
    let json = serde_json::to_string_pretty(&record).expect("record serializes");
    if let Err(err) = verter_workspace::native_fs::NativeFs::new().write_file(&out, &(json + "\n"))
    {
        eprintln!("failed to write {out}: {err:?}");
        return ExitCode::from(1);
    }
    eprintln!(
        "captured {} identities and {} allocation categories to {out}",
        record.identities.len(),
        record.allocation_by_category.len()
    );
    ExitCode::SUCCESS
}

fn read_record(path: &str) -> Result<CssBaselineRecord, String> {
    let text = verter_workspace::native_fs::NativeFs::new()
        .read_file(path)
        .ok_or_else(|| format!("failed to read {path}"))?;
    serde_json::from_str(&text).map_err(|err| format!("failed to parse {path}: {err}"))
}

fn compare(args: &[String]) -> ExitCode {
    let (Some(baseline_path), Some(candidate_path)) = (
        arg_value(args, "--baseline"),
        arg_value(args, "--candidate"),
    ) else {
        eprintln!("compare requires --baseline <path> and --candidate <path>\n{USAGE}");
        return ExitCode::from(2);
    };

    let policy = match arg_value(args, "--expect-transition") {
        None => ComparePolicy::default(),
        Some(spec) => match spec.split_once(':') {
            Some((from, to)) if !from.is_empty() && !to.is_empty() => ComparePolicy {
                allowed_pipeline_transition: Some((from.to_string(), to.to_string())),
            },
            _ => {
                eprintln!("--expect-transition takes <from>:<to>\n{USAGE}");
                return ExitCode::from(2);
            }
        },
    };

    let (baseline, candidate) = match (read_record(&baseline_path), read_record(&candidate_path)) {
        (Ok(b), Ok(c)) => (b, c),
        (b, c) => {
            for err in [b.err(), c.err()].into_iter().flatten() {
                eprintln!("{err}");
            }
            return ExitCode::from(1);
        }
    };

    let universe: BTreeSet<String> = identity_universe();
    match compare_records(&universe, &baseline, &candidate, &policy) {
        Ok(report) => {
            println!(
                "PASS: {} identities, every candidate wall-clock median within 1.2x of \
                 baseline",
                report.per_identity.len()
            );
            for row in &report.per_identity {
                println!(
                    "  {:<40} {:>10}ns -> {:>10}ns  ({:.3}x)",
                    row.identity,
                    row.baseline_wall_ns_median,
                    row.candidate_wall_ns_median,
                    row.ratio
                );
            }
            ExitCode::SUCCESS
        }
        Err(failures) => {
            eprintln!("FAIL: comparison refused / gate exceeded:");
            for failure in &failures {
                eprintln!("  - {failure}");
            }
            ExitCode::from(1)
        }
    }
}
