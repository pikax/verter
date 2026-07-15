//! Project directory scanner — discovers and loads framework-carrier files.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use verter_session::{UpsertRequest, VerterHost};
use walkdir::WalkDir;

/// Result of scanning a project directory.
#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub files_loaded: usize,
    pub parse_errors: usize,
    pub errors: Vec<String>,
    pub scan_duration_ms: f64,
}

/// Scan a directory for framework-carrier files (any registered carrier:
/// `.vue`, `.svelte`, …) and upsert them into the host. When
/// `include_script_deps` is set, plain script files (`.ts`/`.tsx`/`.js`/`.jsx`,
/// which includes the `.svelte.ts`/`.svelte.js` rune-module rows that classify
/// as scripts) are ingested too.
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

        let canonical = path.to_string_lossy().replace('\\', "/");

        // Ingestion gate is carrier-GENERIC: every registered framework carrier
        // (`.vue`, `.svelte`, …) is loaded, sourced from the language registry's
        // carrier-extension set via `path_is_carrier` — never a hardcoded
        // single-framework arm. A `.svelte.ts`/`.svelte.js` rune module is NOT a
        // carrier row (it classifies as a script), so it rides the script-deps
        // arm below exactly like any other `.ts`/`.js`, preserving prior
        // behavior. A new carrier vertical participates the moment its row is
        // registered, with no edit here.
        let is_carrier = verter_workspace::path_is_carrier(&canonical);
        let is_script_dep = include_script_deps && matches!(ext, "ts" | "tsx" | "js" | "jsx");
        if !is_carrier && !is_script_dep {
            continue;
        }

        let file_language = host.language_classifier().classify(&canonical);

        let workspace = host.workspace_read();
        match workspace.read_file(&canonical) {
            Some(source) => {
                let result = host.upsert(UpsertRequest {
                    canonical_id: Some(canonical.clone()),
                    input_id: canonical,
                    source: Arc::from(source.as_ref()),
                    file_language,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use verter_session::HostConfig;

    /// Allocate a fresh, process-unique scratch directory under the OS temp dir
    /// (no `tempfile` dependency in this crate). Hermetic: depends only on
    /// locally-created fixtures.
    fn fresh_scratch_dir(tag: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "verter_mcp_scanner_{tag}_{}_{nanos}_{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn host_rooted_at(dir: &Path) -> Arc<VerterHost> {
        let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions {
                roots: vec![dir.to_string_lossy().replace('\\', "/")],
                eager_preload: false,
            },
        ));
        Arc::new(VerterHost::new(HostConfig::default(), workspace))
    }

    /// DISCRIMINATING ingestion guard for the carrier-generic scan gate.
    ///
    /// Under the pre-fix hardcoded `match ext { "vue" => {} ... }` arm a
    /// `.svelte` COMPONENT carrier hit `_ => continue` and was NEVER upserted,
    /// so it never appeared in `list_files()` — the entire A1
    /// `is_framework_carrier()` MCP generalization was inert for Svelte. With
    /// the registry-driven `path_is_carrier` gate the `.svelte` row IS ingested.
    /// The `.vue` control proves the path that already worked still works; the
    /// non-carrier `.css` proves the gate still excludes non-carriers.
    #[test]
    fn scan_directory_ingests_svelte_carrier_not_just_vue() {
        let dir = fresh_scratch_dir("svelte_ingest");
        fs::write(
            dir.join("Comp.svelte"),
            "<script lang=\"ts\">let x: number = 1;</script>\n<div>{x}</div>\n",
        )
        .unwrap();
        fs::write(
            dir.join("Comp.vue"),
            "<script setup lang=\"ts\">const y = 2;</script>\n<template><div /></template>\n",
        )
        .unwrap();
        // A non-carrier file must NOT be ingested when script deps are off.
        fs::write(dir.join("styles.css"), ".a { color: red; }\n").unwrap();

        let host = host_rooted_at(&dir);
        // `include_script_deps = false` so ONLY carriers are eligible — the
        // `.svelte` row reaching `list_files()` is attributable solely to the
        // carrier-generic gate, not the script-deps arm.
        let result = scan_directory(&dir, &host, false);
        assert!(
            result.errors.is_empty(),
            "scan reported errors: {:?}",
            result.errors
        );

        let files = host.list_files();
        let svelte_ingested = files
            .iter()
            .any(|(id, lang)| id.ends_with("/Comp.svelte") && lang.is_framework_carrier());
        let vue_ingested = files
            .iter()
            .any(|(id, lang)| id.ends_with("/Comp.vue") && lang.is_framework_carrier());
        let css_ingested = files.iter().any(|(id, _)| id.ends_with("/styles.css"));

        assert!(
            svelte_ingested,
            "the `.svelte` carrier must be ingested by scan_directory (carrier-generic gate); \
             list_files() = {files:?}"
        );
        assert!(
            vue_ingested,
            "the `.vue` carrier control must still be ingested; list_files() = {files:?}"
        );
        assert!(
            !css_ingested,
            "a non-carrier `.css` file must NOT be ingested with include_script_deps=false; \
             list_files() = {files:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// A `.svelte.ts` rune MODULE is a script row (NOT a carrier), so it must
    /// ride the script-deps arm: ingested only when `include_script_deps` is on,
    /// never via the carrier gate. Pins the prior script-deps behavior so the
    /// carrier generalization did not silently change rune-module handling.
    #[test]
    fn scan_directory_treats_rune_module_as_script_dep_not_carrier() {
        let dir = fresh_scratch_dir("rune_script_dep");
        fs::write(
            dir.join("store.svelte.ts"),
            "export const count = $state(0);\n",
        )
        .unwrap();

        // Carrier-only pass: the rune module is NOT a carrier ⇒ not ingested.
        let host_carrier_only = host_rooted_at(&dir);
        let _ = scan_directory(&dir, &host_carrier_only, false);
        assert!(
            !host_carrier_only
                .list_files()
                .iter()
                .any(|(id, _)| id.ends_with("/store.svelte.ts")),
            "a `.svelte.ts` rune module must NOT be ingested via the carrier gate"
        );

        // Script-deps pass: now the rune module rides the script arm.
        let host_with_scripts = host_rooted_at(&dir);
        let _ = scan_directory(&dir, &host_with_scripts, true);
        assert!(
            host_with_scripts
                .list_files()
                .iter()
                .any(|(id, _)| id.ends_with("/store.svelte.ts")),
            "a `.svelte.ts` rune module must be ingested via the script-deps arm"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
