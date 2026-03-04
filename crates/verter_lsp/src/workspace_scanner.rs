//! Async priority-based workspace scanner for LSP initialization.
//!
//! Instead of synchronously scanning all `.vue` files during `initialized()` (which
//! blocks the LSP handler for seconds), this module spawns a background task that:
//!
//! 1. Walks the filesystem for `.vue` files
//! 2. Classifies them into priority tiers (project source vs. other)
//! 3. Processes them in priority order, yielding between batches
//! 4. Accepts priority signals from `did_open` to dynamically reorder the queue
//!
//! This makes `initialized()` return in <1s instead of blocking for the full scan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;
use verter_host::{CompileProfile, FileKind, UpsertRequest, VerterHost};

use crate::tsgo::project_sync::ProjectSync;

/// Handle for communicating with the background workspace scanner.
///
/// Created by [`spawn_workspace_scanner`] and stored on the LSP server.
/// Use [`signal_priority`] from `did_open` to promote a file (and its siblings)
/// to the front of the processing queue.
pub struct WorkspaceScannerHandle {
    tx: mpsc::UnboundedSender<ScannerSignal>,
}

impl WorkspaceScannerHandle {
    /// Signal that a file was opened in the editor, promoting it and its
    /// directory siblings to the front of the scan queue.
    pub fn signal_priority(&self, canonical_id: String) {
        let _ = self.tx.send(ScannerSignal::PriorityFile(canonical_id));
    }
}

/// Signals sent to the background scanner to influence processing order.
pub enum ScannerSignal {
    /// A file was opened in the editor — promote it and its directory siblings.
    PriorityFile(String),
}

/// Configuration for the workspace scanner background task.
pub struct WorkspaceScannerConfig {
    /// Workspace root directory.
    pub root_path: PathBuf,
    /// Shared host for upserting and compiling files.
    pub host: Arc<VerterHost>,
    /// Optional project sync for sending files to the type provider.
    pub project_sync: Option<ProjectSync>,
    /// Tracks which files were synced as background files (shared with server).
    pub background_synced_files: Arc<DashMap<String, ()>>,
    /// Whether the type provider is TSGO (affects sync strategy).
    pub is_tsgo: bool,
    /// Compile profile for IDE output.
    pub tsx_profile: CompileProfile,
    /// Coverage patterns from `TsConfigDiscovery` (e.g., `"C:/project/src/**"`).
    pub tsconfig_patterns: Vec<String>,
}

/// Priority tier for a discovered `.vue` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Project source files covered by a tsconfig.json.
    ProjectSource = 0,
    /// Files outside tsconfig coverage (e.g., scripts/, tools/).
    Other = 1,
}

/// Recursively collect all `.vue` file paths under `root`.
///
/// Skips `node_modules`, dot-directories (`.git`, `.vscode`, etc.),
/// `dist`, and `build` directories — same exclusions as the old
/// `scan_vue_files_recursive`.
///
/// Returns paths with forward slashes (canonical form).
pub fn collect_vue_paths(root: &Path) -> Vec<String> {
    let mut result = Vec::new();
    collect_vue_paths_recursive(root, &mut result);
    result
}

fn collect_vue_paths_recursive(dir: &Path, result: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        if path.is_dir() {
            if name == "node_modules" || name.starts_with('.') || name == "dist" || name == "build"
            {
                continue;
            }
            collect_vue_paths_recursive(&path, result);
        } else if name.ends_with(".vue") {
            result.push(path.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Classify paths into priority tiers based on tsconfig coverage patterns.
///
/// A path is Tier::ProjectSource if it matches any of the `tsconfig_patterns`
/// (glob patterns like `"C:/project/src/**"`). Otherwise it's Tier::Other.
pub fn classify_tiers(paths: &[String], tsconfig_patterns: &[String]) -> Vec<(String, Tier)> {
    let compiled: Vec<glob::Pattern> = tsconfig_patterns
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect();

    paths
        .iter()
        .map(|path| {
            let tier = if compiled.iter().any(|pat| pat.matches(path)) {
                Tier::ProjectSource
            } else {
                Tier::Other
            };
            (path.clone(), tier)
        })
        .collect()
}

/// Compute BFS directory distance between two directory paths.
///
/// - Same directory → 0
/// - Sibling directory → 2 (up one + down one)
/// - Parent's sibling → 3
///
/// Returns `u32::MAX` if paths share no common ancestor.
pub fn directory_distance(dir_a: &str, dir_b: &str) -> u32 {
    let a_parts: Vec<&str> = dir_a.split('/').filter(|s| !s.is_empty()).collect();
    let b_parts: Vec<&str> = dir_b.split('/').filter(|s| !s.is_empty()).collect();

    // Find common prefix length
    let common = a_parts
        .iter()
        .zip(b_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    if common == 0 && (!a_parts.is_empty() || !b_parts.is_empty()) {
        // Check if they share a drive letter on Windows (e.g., "C:")
        if a_parts.first() != b_parts.first() {
            return u32::MAX;
        }
    }

    let up = a_parts.len() - common;
    let down = b_parts.len() - common;
    (up + down) as u32
}

/// Sort paths by priority: files in `priority_dirs` first (within same tier),
/// then remaining Tier::ProjectSource, then Tier::Other.
///
/// Within the same tier, files closer (by [`directory_distance`]) to any
/// priority directory come first.
pub fn priority_sort(classified: &mut [(String, Tier)], priority_dirs: &[String]) {
    classified.sort_by(|a, b| {
        // Primary: tier ordering
        let tier_cmp = a.1.cmp(&b.1);
        if tier_cmp != std::cmp::Ordering::Equal {
            return tier_cmp;
        }

        // Secondary: distance to nearest priority dir (lower = earlier)
        let dist_a = min_distance_to_priority(&a.0, priority_dirs);
        let dist_b = min_distance_to_priority(&b.0, priority_dirs);
        dist_a.cmp(&dist_b)
    });
}

fn min_distance_to_priority(path: &str, priority_dirs: &[String]) -> u32 {
    if priority_dirs.is_empty() {
        return 0;
    }
    let dir = parent_dir(path);
    priority_dirs
        .iter()
        .map(|pd| directory_distance(&dir, pd))
        .min()
        .unwrap_or(u32::MAX)
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(idx) => path[..idx].to_string(),
        None => String::new(),
    }
}

/// How many files to process before yielding to the tokio runtime.
const BATCH_SIZE: usize = 10;

/// Spawn the background workspace scanner task.
///
/// Returns a [`WorkspaceScannerHandle`] for sending priority signals from `did_open`.
///
/// The scanner:
/// 1. Walks the filesystem (in `spawn_blocking`) to find all `.vue` files
/// 2. Classifies them by tsconfig coverage
/// 3. Processes files in priority order: upsert → compile → sync to type provider
/// 4. Yields every [`BATCH_SIZE`] files to let LSP request handlers run
/// 5. Accepts [`ScannerSignal::PriorityFile`] to dynamically reorder the queue
pub fn spawn_workspace_scanner(config: WorkspaceScannerConfig) -> WorkspaceScannerHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(scanner_loop(config, rx));
    WorkspaceScannerHandle { tx }
}

async fn scanner_loop(
    config: WorkspaceScannerConfig,
    mut rx: mpsc::UnboundedReceiver<ScannerSignal>,
) {
    let root = config.root_path.clone();
    let tsconfig_patterns = config.tsconfig_patterns.clone();

    // Step 1: FS walk (blocking)
    let paths = tokio::task::spawn_blocking(move || collect_vue_paths(&root))
        .await
        .unwrap_or_default();

    if paths.is_empty() {
        tracing::info!("workspace_scanner: no .vue files found");
        return;
    }

    // Step 2: Classify into tiers
    let mut classified = classify_tiers(&paths, &tsconfig_patterns);
    let tier1_count = classified
        .iter()
        .filter(|(_, t)| *t == Tier::ProjectSource)
        .count();

    // Initial sort (no priority dirs yet)
    priority_sort(&mut classified, &[]);

    tracing::info!(
        "workspace_scanner: found {} .vue files ({} project source, {} other)",
        classified.len(),
        tier1_count,
        classified.len() - tier1_count,
    );

    // Step 3: Process loop
    let mut processed: HashSet<String> = HashSet::new();
    let mut priority_dirs: Vec<String> = Vec::new();
    let mut idx = 0;

    while idx < classified.len() {
        // Drain priority signals
        while let Ok(signal) = rx.try_recv() {
            match signal {
                ScannerSignal::PriorityFile(canonical_id) => {
                    let dir = parent_dir(&canonical_id);
                    if !priority_dirs.contains(&dir) {
                        priority_dirs.push(dir);
                    }
                    // Re-sort remaining unprocessed files
                    priority_sort(&mut classified[idx..], &priority_dirs);
                }
            }
        }

        let (ref path, _tier) = classified[idx];
        idx += 1;

        if processed.contains(path) {
            continue;
        }
        processed.insert(path.clone());

        // Upsert + compile (blocking)
        let path_clone = path.clone();
        let host = Arc::clone(&config.host);
        let profile = config.tsx_profile.clone();

        let compile_ok = tokio::task::spawn_blocking(move || {
            let source = match std::fs::read_to_string(&path_clone) {
                Ok(s) => s,
                Err(_) => return false,
            };
            let _ = host.upsert(UpsertRequest {
                canonical_id: None,
                input_id: path_clone.clone(),
                source: source.into(),
                file_kind: FileKind::VueSfc,
                aliases: Vec::new(),
            });
            host.ensure_compiled(&path_clone, &profile).is_ok()
        })
        .await
        .unwrap_or(false);

        // Sync to type provider
        if compile_ok {
            if let Some(sync) = &config.project_sync {
                sync_file_to_provider(
                    path,
                    &config.host,
                    &config.tsx_profile,
                    sync,
                    config.is_tsgo,
                    &config.background_synced_files,
                )
                .await;
            }
        }

        // Yield every BATCH_SIZE files to let request handlers run
        if processed.len().is_multiple_of(BATCH_SIZE) {
            tokio::task::yield_now().await;
        }
    }

    tracing::info!(
        "workspace_scanner: completed {} files ({} project source, {} other)",
        processed.len(),
        tier1_count,
        processed.len().saturating_sub(tier1_count),
    );
}

/// Sync a single compiled file's IDE and DTS output to the type provider.
async fn sync_file_to_provider(
    canonical_id: &str,
    host: &VerterHost,
    profile: &CompileProfile,
    sync: &ProjectSync,
    is_tsgo: bool,
    bg_files: &DashMap<String, ()>,
) {
    // Sync DTS (both TSGO and tsserver)
    if let Some(api) = host.get_public_api(canonical_id) {
        let base = canonical_id.strip_suffix(".vue").unwrap_or(canonical_id);
        let dts_path = format!("{base}.vue.ts");
        let result = if is_tsgo {
            sync.open_dts(&dts_path, &api.code).await
        } else {
            sync.load_dts(&dts_path, &api.code).await
        };
        if result.is_ok() {
            bg_files.insert(dts_path, ());
        }
    }

    // Sync IDE (tsserver only — TSGO resolves via DTS)
    if !is_tsgo {
        if let Some(ide) = host.get_ide(canonical_id, profile) {
            let ext = if ide.is_jsx { ".jsx" } else { ".tsx" };
            let tsx_path = format!("{canonical_id}{ext}");
            if sync.load_tsx(&tsx_path, &ide.code).await.is_ok() {
                bg_files.insert(tsx_path, ());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_dir() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Project source files
        fs::create_dir_all(root.join("src/components")).unwrap();
        fs::create_dir_all(root.join("src/views")).unwrap();
        fs::write(
            root.join("src/components/Foo.vue"),
            "<template><div>Foo</div></template>",
        )
        .unwrap();
        fs::write(
            root.join("src/components/Bar.vue"),
            "<template><div>Bar</div></template>",
        )
        .unwrap();
        fs::write(
            root.join("src/views/Home.vue"),
            "<template><div>Home</div></template>",
        )
        .unwrap();

        // Files outside tsconfig
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("scripts/Tool.vue"),
            "<template><div>Tool</div></template>",
        )
        .unwrap();

        // Excluded directories
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::write(
            root.join("node_modules/pkg/Dep.vue"),
            "<template></template>",
        )
        .unwrap();

        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join(".hidden/Secret.vue"), "<template></template>").unwrap();

        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(root.join("dist/Built.vue"), "<template></template>").unwrap();

        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build/Output.vue"), "<template></template>").unwrap();

        // Non-vue files (should be ignored)
        fs::write(root.join("src/main.ts"), "export {}").unwrap();

        tmp
    }

    #[test]
    fn test_collect_vue_paths() {
        let tmp = create_test_dir();
        let root = tmp.path();
        let paths = collect_vue_paths(root);

        // Positive: finds all .vue files in src/ and scripts/
        assert!(
            paths.iter().any(|p| p.ends_with("src/components/Foo.vue")),
            "should find src/components/Foo.vue"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("src/components/Bar.vue")),
            "should find src/components/Bar.vue"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("src/views/Home.vue")),
            "should find src/views/Home.vue"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("scripts/Tool.vue")),
            "should find scripts/Tool.vue"
        );
        assert_eq!(paths.len(), 4, "should find exactly 4 .vue files");

        // Negative: does NOT include excluded directories
        assert!(
            !paths.iter().any(|p| p.contains("node_modules")),
            "must not include node_modules"
        );
        assert!(
            !paths.iter().any(|p| p.contains(".hidden")),
            "must not include dot-directories"
        );
        assert!(
            !paths.iter().any(|p| p.contains("/dist/")),
            "must not include dist/"
        );
        assert!(
            !paths.iter().any(|p| p.contains("/build/")),
            "must not include build/"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with(".ts")),
            "must not include non-.vue files"
        );

        // All paths use forward slashes
        for p in &paths {
            assert!(!p.contains('\\'), "paths should use forward slashes: {p}");
        }
    }

    #[test]
    fn test_classify_tiers() {
        let paths = vec![
            "C:/project/src/App.vue".to_string(),
            "C:/project/src/components/Foo.vue".to_string(),
            "C:/project/scripts/Tool.vue".to_string(),
            "C:/project/tests/E2e.vue".to_string(),
        ];
        let patterns = vec!["C:/project/src/**".to_string()];

        let classified = classify_tiers(&paths, &patterns);

        // Positive: src/ files → ProjectSource
        assert_eq!(
            classified[0].1,
            Tier::ProjectSource,
            "src/App.vue should be ProjectSource"
        );
        assert_eq!(
            classified[1].1,
            Tier::ProjectSource,
            "src/components/Foo.vue should be ProjectSource"
        );

        // Negative: files outside src/ → Other
        assert_eq!(
            classified[2].1,
            Tier::Other,
            "scripts/Tool.vue should be Other"
        );
        assert_eq!(
            classified[3].1,
            Tier::Other,
            "tests/E2e.vue should be Other"
        );
    }

    #[test]
    fn test_classify_tiers_multiple_patterns() {
        let paths = vec![
            "C:/project/src/App.vue".to_string(),
            "C:/project/packages/ui/Btn.vue".to_string(),
            "C:/project/scripts/Tool.vue".to_string(),
        ];
        let patterns = vec![
            "C:/project/src/**".to_string(),
            "C:/project/packages/**".to_string(),
        ];

        let classified = classify_tiers(&paths, &patterns);

        assert_eq!(classified[0].1, Tier::ProjectSource);
        assert_eq!(classified[1].1, Tier::ProjectSource);
        assert_eq!(classified[2].1, Tier::Other);
    }

    #[test]
    fn test_priority_sort_with_open_dir() {
        let mut classified = vec![
            (
                "C:/project/src/views/B.vue".to_string(),
                Tier::ProjectSource,
            ),
            (
                "C:/project/src/components/A.vue".to_string(),
                Tier::ProjectSource,
            ),
            ("C:/project/scripts/Tool.vue".to_string(), Tier::Other),
        ];
        let priority_dirs = vec!["C:/project/src/components".to_string()];

        priority_sort(&mut classified, &priority_dirs);

        // Positive: component file (in priority dir) comes first among ProjectSource
        assert!(
            classified[0].0.contains("components/A.vue"),
            "file in priority dir should come first, got: {}",
            classified[0].0
        );

        // Negative: Tier::Other never appears before any Tier::ProjectSource
        let first_other = classified.iter().position(|(_, t)| *t == Tier::Other);
        let last_project = classified
            .iter()
            .rposition(|(_, t)| *t == Tier::ProjectSource);
        if let (Some(fo), Some(lp)) = (first_other, last_project) {
            assert!(
                fo > lp,
                "Other-tier file at index {fo} must come after last ProjectSource at index {lp}"
            );
        }
    }

    #[test]
    fn test_priority_sort_no_signals() {
        let mut classified = vec![
            ("C:/project/scripts/Tool.vue".to_string(), Tier::Other),
            ("C:/project/src/App.vue".to_string(), Tier::ProjectSource),
            ("C:/project/scripts/Dev.vue".to_string(), Tier::Other),
            (
                "C:/project/src/components/Foo.vue".to_string(),
                Tier::ProjectSource,
            ),
        ];

        priority_sort(&mut classified, &[]);

        // Positive: all ProjectSource files come first
        assert_eq!(
            classified[0].1,
            Tier::ProjectSource,
            "index 0 should be ProjectSource"
        );
        assert_eq!(
            classified[1].1,
            Tier::ProjectSource,
            "index 1 should be ProjectSource"
        );

        // Negative: Other files not interleaved with ProjectSource
        assert_eq!(classified[2].1, Tier::Other, "index 2 should be Other");
        assert_eq!(classified[3].1, Tier::Other, "index 3 should be Other");
    }

    #[test]
    fn test_directory_distance_same_dir() {
        assert_eq!(directory_distance("C:/project/src", "C:/project/src"), 0);
    }

    #[test]
    fn test_directory_distance_sibling() {
        // src/components → src/views = up 1 + down 1 = 2
        assert_eq!(
            directory_distance("C:/project/src/components", "C:/project/src/views"),
            2
        );
    }

    #[test]
    fn test_directory_distance_parent_sibling() {
        // src/components → scripts = up 2 + down 1 = 3
        assert_eq!(
            directory_distance("C:/project/src/components", "C:/project/scripts"),
            3
        );
    }

    #[test]
    fn test_directory_distance_child() {
        // src → src/components = down 1 = 1
        assert_eq!(
            directory_distance("C:/project/src", "C:/project/src/components"),
            1
        );
    }

    #[test]
    fn test_directory_distance_no_common() {
        assert_eq!(
            directory_distance("C:/project/src", "D:/other/dir"),
            u32::MAX,
        );
    }

    #[test]
    fn test_collect_vue_paths_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let paths = collect_vue_paths(tmp.path());
        assert!(paths.is_empty(), "empty dir should return no paths");
    }

    #[test]
    fn test_classify_tiers_empty_patterns() {
        let paths = vec!["C:/project/src/App.vue".to_string()];
        let classified = classify_tiers(&paths, &[]);

        // With no tsconfig patterns, everything is Other
        assert_eq!(classified[0].1, Tier::Other);
    }
}
