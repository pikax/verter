//! `record` subcommand — find a single record by `request_id` in
//! `<dir>` and print it. Human-readable output prints the envelope
//! field-by-field; JSON output dumps the full record verbatim.

use std::io::Write;
use std::path::Path;

use verter_audit::record::RequestAuditRecord;

use crate::io::load_records_from_dir;
use crate::summary::kind_label;
use crate::OutputFormat;

/// Run the `record` subcommand. Returns a process exit code: `0` on
/// success, `1` if no record matched, `2` on I/O / parse failure.
pub(crate) fn run(request_id: &str, dir: &Path, format: OutputFormat) -> i32 {
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
    let target_id: u64 = match request_id.parse() {
        Ok(n) => n,
        Err(e) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "error: --request_id `{request_id}` is not a valid u64: {e}"
            );
            return 2;
        }
    };
    let hit = outcome
        .records
        .into_iter()
        .find(|loaded| loaded.record.request_id == target_id);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match hit {
        None => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "error: no record with request_id={request_id} under {}",
                dir.display()
            );
            1
        }
        Some(loaded) => {
            if format.json {
                match serde_json::to_string_pretty(&loaded.record) {
                    Ok(rendered) => {
                        let _ = writeln!(out, "{rendered}");
                        0
                    }
                    Err(e) => {
                        let _ = writeln!(std::io::stderr().lock(), "error: serialise failed: {e}");
                        2
                    }
                }
            } else {
                render_text(&mut out, &loaded.path.display().to_string(), &loaded.record);
                0
            }
        }
    }
}

fn render_text(out: &mut impl Write, source_path: &str, record: &RequestAuditRecord) {
    let _ = writeln!(out, "verter-audit-inspect record  source={source_path}");
    let _ = writeln!(out, "  request_id:        {}", record.request_id);
    let _ = writeln!(out, "  canonical_id:      {}", record.canonical_id);
    let _ = writeln!(out, "  kind:              {}", kind_label(&record.kind));
    let _ = writeln!(
        out,
        "  parent_request_id: {}",
        record.parent_request_id.as_deref().unwrap_or("(none)")
    );
    let _ = writeln!(out, "  from_cache:        {}", record.from_cache);
    let _ = writeln!(out, "  timings:");
    let _ = writeln!(
        out,
        "    total_ms:                   {:.3}",
        record.timings.total_ms
    );
    let _ = writeln!(
        out,
        "    capture_inputs_ms:          {:.3}",
        record.timings.capture_inputs_ms
    );
    let _ = writeln!(
        out,
        "    store_read_ms:              {:.3}",
        record.timings.store_read_ms
    );
    let _ = writeln!(
        out,
        "    store_merge_ms:             {:.3}",
        record.timings.store_merge_ms
    );
    let _ = writeln!(
        out,
        "    direct_import_proof_ms:     {:.3}",
        record.timings.direct_import_proof_ms
    );
    let _ = writeln!(
        out,
        "    imported_root_proof_ms:     {:.3}",
        record.timings.imported_root_proof_ms
    );
    let _ = writeln!(
        out,
        "    solver_ms:                  {:.3}",
        record.timings.solver_ms
    );
    let _ = writeln!(
        out,
        "    materialize_ms:             {:.3}",
        record.timings.materialize_ms
    );
    let _ = writeln!(
        out,
        "    serialize_ms:               {:.3}",
        record.timings.serialize_ms
    );
    let _ = writeln!(
        out,
        "    request_critical_path_ms:   {:.3}",
        record.timings.request_critical_path_ms
    );
    let _ = writeln!(out, "  memory:");
    let _ = writeln!(
        out,
        "    bytes_parsed:               {}",
        record.memory.bytes_parsed
    );
    let _ = writeln!(out, "  store:");
    let _ = writeln!(
        out,
        "    store_view_hits:            {}",
        record.store.store_view_hits
    );
    let _ = writeln!(
        out,
        "    store_view_misses:          {}",
        record.store.store_view_misses
    );
    let _ = writeln!(
        out,
        "    structural_merges:          {}",
        record.store.structural_merges
    );
    let _ = writeln!(
        out,
        "    imported_dependency_entries: {}",
        record.store.imported_dependency_entries
    );
    let _ = writeln!(
        out,
        "    imported_dependency_bytes:   {}",
        record.store.imported_dependency_bytes
    );
    let _ = writeln!(
        out,
        "  files: {} entr{}",
        record.files.len(),
        if record.files.len() == 1 { "y" } else { "ies" }
    );
    if let Some(waits) = record.waits.as_ref() {
        let _ = writeln!(
            out,
            "  waits: lock_wait_ns={} queue_wait_ns={} lock_acquisitions={}",
            waits.lock_wait_ns, waits.queue_wait_ns, waits.lock_acquisitions
        );
    } else {
        let _ = writeln!(out, "  waits: (none)");
    }
    let _ = writeln!(
        out,
        "  scheduler: {}",
        if record.scheduler.is_some() {
            "(captured)"
        } else {
            "(none)"
        }
    );
    let _ = writeln!(
        out,
        "  footprint: {}",
        if record.footprint.is_some() {
            "(captured)"
        } else {
            "(none)"
        }
    );
}
