//! Disk I/O helpers — load `RequestAuditRecord` JSON files from a
//! directory, recursively. Errors are reported per-file so a single
//! malformed record does not poison the rest of the corpus; the
//! caller decides whether to keep going or fail.

use std::fs;
use std::path::{Path, PathBuf};

use verter_audit::record::RequestAuditRecord;

/// One loaded record + the path it came from. The path is preserved
/// for diagnostics and so callers (e.g. `record` subcommand) can show
/// the user where a record was found.
pub(crate) struct LoadedRecord {
    pub path: PathBuf,
    pub record: RequestAuditRecord,
}

/// Outcome of loading a directory's records — successes and failures
/// are reported separately so the caller can decide how strict to be.
pub(crate) struct LoadOutcome {
    pub records: Vec<LoadedRecord>,
    pub errors: Vec<LoadError>,
}

/// A single per-file failure. Held alongside the path so error
/// messages can name the offending file rather than dumping a bare
/// `serde_json::Error`.
pub(crate) struct LoadError {
    pub path: PathBuf,
    pub message: String,
}

/// Walk `dir` recursively and parse every `*.json` file as a
/// `RequestAuditRecord`. Returns the list of successes plus a list
/// of per-file errors. The directory itself missing is reported as
/// a single error against the directory path.
pub(crate) fn load_records_from_dir(dir: &Path) -> LoadOutcome {
    let mut records: Vec<LoadedRecord> = Vec::new();
    let mut errors: Vec<LoadError> = Vec::new();
    if !dir.exists() {
        errors.push(LoadError {
            path: dir.to_path_buf(),
            message: format!("directory does not exist: {}", dir.display()),
        });
        return LoadOutcome { records, errors };
    }
    if !dir.is_dir() {
        errors.push(LoadError {
            path: dir.to_path_buf(),
            message: format!("path is not a directory: {}", dir.display()),
        });
        return LoadOutcome { records, errors };
    }
    visit_dir(dir, &mut records, &mut errors);
    // Stable ordering by path so output is deterministic across
    // platforms (read_dir is not order-stable on Linux).
    records.sort_by(|a, b| a.path.cmp(&b.path));
    errors.sort_by(|a, b| a.path.cmp(&b.path));
    LoadOutcome { records, errors }
}

fn visit_dir(dir: &Path, records: &mut Vec<LoadedRecord>, errors: &mut Vec<LoadError>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            errors.push(LoadError {
                path: dir.to_path_buf(),
                message: format!("failed to read directory: {e}"),
            });
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_dir(&path, records, errors);
            continue;
        }
        if !is_json_file(&path) {
            continue;
        }
        match load_record_from_file(&path) {
            Ok(record) => records.push(LoadedRecord { path, record }),
            Err(message) => errors.push(LoadError { path, message }),
        }
    }
}

/// Parse one JSON file. Returns the parsed record or a human-readable
/// error string (file system or JSON failure).
pub(crate) fn load_record_from_file(path: &Path) -> Result<RequestAuditRecord, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read failed: {e}"))?;
    serde_json::from_str::<RequestAuditRecord>(&raw).map_err(|e| format!("parse failed: {e}"))
}

fn is_json_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "json")
}
