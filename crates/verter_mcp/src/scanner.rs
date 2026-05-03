//! Project directory scanner — discovers and loads Vue files.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use verter_session::{FileKind, UpsertRequest, VerterHost};
use walkdir::WalkDir;

/// Result of scanning a project directory.
#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub files_loaded: usize,
    pub parse_errors: usize,
    pub errors: Vec<String>,
    pub scan_duration_ms: f64,
}

/// Scan a directory for Vue files and upsert them into the host.
///
/// Excludes `node_modules`, dot-directories, and common build output directories.
pub fn scan_directory(root: &Path, host: &VerterHost, include_script_deps: bool) -> ScanResult {
    let start = Instant::now();
    let mut files_loaded = 0usize;
    let mut parse_errors = 0usize;
    let mut errors = Vec::new();

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            // Skip hidden directories, node_modules, and build output
            if entry.file_type().is_dir() {
                return !name.starts_with('.')
                    && name != "node_modules"
                    && name != "dist"
                    && name != "target"
                    && name != ".output";
            }
            true
        });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let file_kind = match ext {
            "vue" => FileKind::VueSfc,
            "ts" | "tsx" | "js" | "jsx" if include_script_deps => FileKind::NonSfc,
            _ => continue,
        };

        let canonical = path.to_string_lossy().replace('\\', "/");

        let workspace = host.workspace_read();
        match workspace.read_file(&canonical) {
            Some(source) => {
                let result = host.upsert(UpsertRequest {
                    canonical_id: Some(canonical.clone()),
                    input_id: canonical,
                    source: Arc::from(source.as_ref()),
                    file_kind,
                    aliases: vec![],
                });
                match result {
                    Ok(update) => {
                        files_loaded += 1;
                        if update.diagnostics.has_errors {
                            parse_errors += 1;
                        }
                    }
                    Err(e) => {
                        errors.push(format!("{}: {}", path.display(), e));
                    }
                }
            }
            None => {
                errors.push(format!("{}: file not found via workspace", path.display()));
            }
        }
    }

    ScanResult {
        files_loaded,
        parse_errors,
        errors,
        scan_duration_ms: start.elapsed().as_secs_f64() * 1000.0,
    }
}
