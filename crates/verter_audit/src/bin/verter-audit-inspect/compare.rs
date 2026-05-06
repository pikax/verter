//! `compare` subcommand — load two record directories and print a
//! delta report. Compares record counts, per-kind partition, total
//! duration, and cache hit rate. The diff is "b - a" so a positive
//! number is "b is larger".

use std::io::Write;
use std::path::Path;
use std::time::Instant;

use serde::Serialize;
use verter_audit::batch::{AuditRecordSource, BatchAuditAggregator};
use verter_audit::payloads::tags::BundlerKindTag;
use verter_audit::record::RequestAuditRecord;
use verter_audit::BundlerBatchPayload;

use crate::io::{load_records_from_dir, LoadedRecord};
use crate::OutputFormat;

/// Run the `compare` subcommand. Returns a process exit code.
pub(crate) fn run(dir_a: &Path, dir_b: &Path, format: OutputFormat) -> i32 {
    let a = load_records_from_dir(dir_a);
    let b = load_records_from_dir(dir_b);
    let mut had_fatal = false;
    if a.records.is_empty() && !a.errors.is_empty() {
        eprintln_load_errors(dir_a, &a.errors);
        had_fatal = true;
    }
    if b.records.is_empty() && !b.errors.is_empty() {
        eprintln_load_errors(dir_b, &b.errors);
        had_fatal = true;
    }
    if had_fatal {
        return 2;
    }

    let payload_a = aggregate(&a.records);
    let payload_b = aggregate(&b.records);
    let diff = ComparePayload::diff(dir_a, dir_b, &payload_a, &payload_b);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if format.json {
        match serde_json::to_string_pretty(&diff) {
            Ok(rendered) => {
                let _ = writeln!(out, "{rendered}");
            }
            Err(e) => {
                let _ = writeln!(std::io::stderr().lock(), "error: serialise failed: {e}");
                return 2;
            }
        }
    } else {
        render_text(&mut out, &diff);
    }
    0
}

fn eprintln_load_errors(dir: &Path, errors: &[crate::io::LoadError]) {
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(
        stderr,
        "error: failed to load any records from {}",
        dir.display()
    );
    for err in errors {
        let _ = writeln!(stderr, "  {}: {}", err.path.display(), err.message);
    }
}

fn aggregate(records: &[LoadedRecord]) -> BundlerBatchPayload {
    let source = LoadedSource { records };
    BatchAuditAggregator::new(&source, BundlerKindTag::Vite).summarize(None)
}

struct LoadedSource<'a> {
    records: &'a [LoadedRecord],
}

impl AuditRecordSource for LoadedSource<'_> {
    fn for_each_record(&self, f: &mut dyn FnMut(Instant, &RequestAuditRecord)) {
        let now = Instant::now();
        for entry in self.records {
            f(now, &entry.record);
        }
    }
}

/// Per-kind delta + scalar deltas + raw "a"/"b" totals so callers
/// can show the underlying numbers, not just the diff.
#[derive(Debug, Serialize)]
struct ComparePayload {
    dir_a: String,
    dir_b: String,
    a_total_records: u32,
    b_total_records: u32,
    delta_total_records: i64,
    a_total_duration_ms: f64,
    b_total_duration_ms: f64,
    delta_total_duration_ms: f64,
    a_cache_hit_rate: f32,
    b_cache_hit_rate: f32,
    delta_cache_hit_rate: f32,
    per_kind: Vec<KindDelta>,
}

#[derive(Debug, Serialize)]
struct KindDelta {
    kind: &'static str,
    a: u32,
    b: u32,
    delta: i64,
}

impl ComparePayload {
    fn diff(dir_a: &Path, dir_b: &Path, a: &BundlerBatchPayload, b: &BundlerBatchPayload) -> Self {
        let per_kind = vec![
            KindDelta {
                kind: "component_meta",
                a: a.component_meta_count,
                b: b.component_meta_count,
                delta: i64::from(b.component_meta_count) - i64::from(a.component_meta_count),
            },
            KindDelta {
                kind: "type_resolution",
                a: a.type_resolution_count,
                b: b.type_resolution_count,
                delta: i64::from(b.type_resolution_count) - i64::from(a.type_resolution_count),
            },
            KindDelta {
                kind: "semantic_analysis",
                a: a.semantic_analysis_count,
                b: b.semantic_analysis_count,
                delta: i64::from(b.semantic_analysis_count) - i64::from(a.semantic_analysis_count),
            },
            KindDelta {
                kind: "compile",
                a: a.compile_count,
                b: b.compile_count,
                delta: i64::from(b.compile_count) - i64::from(a.compile_count),
            },
            KindDelta {
                kind: "workspace",
                a: a.workspace_count,
                b: b.workspace_count,
                delta: i64::from(b.workspace_count) - i64::from(a.workspace_count),
            },
            KindDelta {
                kind: "lsp",
                a: a.lsp_count,
                b: b.lsp_count,
                delta: i64::from(b.lsp_count) - i64::from(a.lsp_count),
            },
            KindDelta {
                kind: "mcp",
                a: a.mcp_count,
                b: b.mcp_count,
                delta: i64::from(b.mcp_count) - i64::from(a.mcp_count),
            },
            KindDelta {
                kind: "bundler_batch",
                a: a.bundler_batch_count,
                b: b.bundler_batch_count,
                delta: i64::from(b.bundler_batch_count) - i64::from(a.bundler_batch_count),
            },
            KindDelta {
                kind: "custom",
                a: a.custom_count,
                b: b.custom_count,
                delta: i64::from(b.custom_count) - i64::from(a.custom_count),
            },
        ];
        Self {
            dir_a: dir_a.display().to_string(),
            dir_b: dir_b.display().to_string(),
            a_total_records: a.total_records,
            b_total_records: b.total_records,
            delta_total_records: i64::from(b.total_records) - i64::from(a.total_records),
            a_total_duration_ms: a.total_duration_ms,
            b_total_duration_ms: b.total_duration_ms,
            delta_total_duration_ms: b.total_duration_ms - a.total_duration_ms,
            a_cache_hit_rate: a.cache_hit_rate,
            b_cache_hit_rate: b.cache_hit_rate,
            delta_cache_hit_rate: b.cache_hit_rate - a.cache_hit_rate,
            per_kind,
        }
    }
}

fn render_text(out: &mut impl Write, diff: &ComparePayload) {
    let _ = writeln!(out, "verter-audit-inspect compare");
    let _ = writeln!(out, "  a={}", diff.dir_a);
    let _ = writeln!(out, "  b={}", diff.dir_b);
    let _ = writeln!(
        out,
        "  total records:     a={} b={} delta={:+}",
        diff.a_total_records, diff.b_total_records, diff.delta_total_records
    );
    let _ = writeln!(
        out,
        "  total duration_ms: a={:.3} b={:.3} delta={:+.3}",
        diff.a_total_duration_ms, diff.b_total_duration_ms, diff.delta_total_duration_ms
    );
    let _ = writeln!(
        out,
        "  cache hit rate:    a={:.4} b={:.4} delta={:+.4}",
        diff.a_cache_hit_rate, diff.b_cache_hit_rate, diff.delta_cache_hit_rate
    );
    let _ = writeln!(out, "  per-kind:");
    let _ = writeln!(
        out,
        "    {:<18} {:>8} {:>8} {:>8}",
        "kind", "a", "b", "delta"
    );
    for row in &diff.per_kind {
        let _ = writeln!(
            out,
            "    {:<18} {:>8} {:>8} {:>+8}",
            row.kind, row.a, row.b, row.delta
        );
    }
}
