//! `summary` subcommand — fold every record in `<dir>` into a
//! per-kind / total / cache-hit-rate summary plus the slowest 5
//! records. Reuses [`verter_audit::BatchAuditAggregator`] so the
//! per-kind partition stays consistent with the bundler-side batch
//! aggregator and there is no second implementation to drift.

use std::io::Write;
use std::path::Path;
use std::time::Instant;

use verter_audit::batch::{AuditRecordSource, BatchAuditAggregator};
use verter_audit::payloads::tags::BundlerKindTag;
use verter_audit::record::{RequestAuditRecord, RequestKind};
use verter_audit::BundlerBatchPayload;

use crate::io::{load_records_from_dir, LoadedRecord};
use crate::OutputFormat;

/// Run the `summary` subcommand. Returns a process exit code.
pub(crate) fn run(dir: &Path, format: OutputFormat) -> i32 {
    let outcome = load_records_from_dir(dir);
    if outcome.records.is_empty() && !outcome.errors.is_empty() {
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(
            stderr,
            "error: failed to load any records from {}",
            dir.display()
        );
        for err in &outcome.errors {
            let _ = writeln!(stderr, "  {}: {}", err.path.display(), err.message);
        }
        return 2;
    }
    // Soft-warn on partial parse failures — keep going so the user
    // still gets a summary of the valid records.
    if !outcome.errors.is_empty() {
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(
            stderr,
            "warning: skipped {} unreadable record(s) in {}",
            outcome.errors.len(),
            dir.display()
        );
        for err in &outcome.errors {
            let _ = writeln!(stderr, "  {}: {}", err.path.display(), err.message);
        }
    }

    let source = LoadedSource {
        records: &outcome.records,
    };
    let aggregator = BatchAuditAggregator::new(&source, BundlerKindTag::Vite);
    let payload = aggregator.summarize(None);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if format.json {
        match serde_json::to_string_pretty(&payload) {
            Ok(rendered) => {
                let _ = writeln!(out, "{rendered}");
            }
            Err(e) => {
                let _ = writeln!(std::io::stderr().lock(), "error: serialise failed: {e}");
                return 2;
            }
        }
    } else {
        render_text(&mut out, dir, &payload);
    }
    0
}

/// Adapt the loaded `Vec<LoadedRecord>` to the
/// [`AuditRecordSource`] callback contract. The aggregator stamps
/// every record with a single fixed `Instant::now()` here — the
/// bundler-side workflow uses `Instant::since` filtering, but the CLI
/// always summarises the full corpus so the timestamp is irrelevant.
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

/// Pretty-print the aggregated payload. Mirrors the field layout in
/// `BundlerBatchPayload` — total → per-kind partition → cache stats →
/// slowest 5.
fn render_text(out: &mut impl Write, dir: &Path, payload: &BundlerBatchPayload) {
    let _ = writeln!(out, "verter-audit-inspect summary  dir={}", dir.display());
    let _ = writeln!(out, "  total records:      {}", payload.total_records);
    let _ = writeln!(
        out,
        "  total duration_ms:  {:.3}",
        payload.total_duration_ms
    );
    let _ = writeln!(out, "  total bytes_parsed: {}", payload.total_bytes_parsed);
    let _ = writeln!(out, "  from_cache count:   {}", payload.from_cache_count);
    let _ = writeln!(out, "  cache hit rate:     {:.4}", payload.cache_hit_rate);
    let _ = writeln!(out, "  per-kind partition:");
    let _ = writeln!(
        out,
        "    component_meta:    {}",
        payload.component_meta_count
    );
    let _ = writeln!(
        out,
        "    type_resolution:   {}",
        payload.type_resolution_count
    );
    let _ = writeln!(
        out,
        "    semantic_analysis: {}",
        payload.semantic_analysis_count
    );
    let _ = writeln!(out, "    compile:           {}", payload.compile_count);
    let _ = writeln!(out, "    workspace:         {}", payload.workspace_count);
    let _ = writeln!(out, "    lsp:               {}", payload.lsp_count);
    let _ = writeln!(out, "    mcp:               {}", payload.mcp_count);
    let _ = writeln!(
        out,
        "    bundler_batch:     {}",
        payload.bundler_batch_count
    );
    let _ = writeln!(
        out,
        "    typeinfo_graph:    {}",
        payload.typeinfo_graph_count
    );
    let _ = writeln!(out, "    custom:            {}", payload.custom_count);
    let _ = writeln!(out, "  slowest {}:", payload.slowest_5.len());
    for (idx, slow) in payload.slowest_5.iter().enumerate() {
        let _ = writeln!(
            out,
            "    {}. request_id={} duration_ms={:.3} kind={} canonical={}",
            idx + 1,
            slow.request_id,
            slow.duration_ms,
            kind_label(&slow.kind),
            slow.canonical_id
        );
    }
}

/// Compact label for a [`RequestKind`] so the summary output stays
/// one record per line. Keeps the kind name alongside any tag/op
/// payload (compile target, workspace op, lsp method, …) so the user
/// can disambiguate variants.
pub(crate) fn kind_label(kind: &RequestKind) -> String {
    match kind {
        RequestKind::ComponentMeta => "ComponentMeta".to_string(),
        RequestKind::TypeResolution => "TypeResolution".to_string(),
        RequestKind::SemanticAnalysis => "SemanticAnalysis".to_string(),
        RequestKind::Compile { target } => format!("Compile({target:?})"),
        RequestKind::Workspace { op } => format!("Workspace({op:?})"),
        RequestKind::Lsp { method } => format!("Lsp({method:?})"),
        RequestKind::Mcp { tool } => format!("Mcp({tool})"),
        RequestKind::BundlerBatch { kind } => format!("BundlerBatch({kind:?})"),
        RequestKind::Custom { name } => format!("Custom({name})"),
        RequestKind::TypeInfoGraph => "TypeInfoGraph".to_string(),
    }
}
