//! Async priority-based workspace scanner for LSP initialization.
//!
//! Instead of synchronously scanning all files during `initialized()` (which
//! blocks the LSP handler for seconds), this module spawns a background task that:
//!
//! 1. Walks the filesystem for `.vue` and non-Vue source files (`.ts`, `.tsx`, `.js`, `.jsx`)
//! 2. Classifies them into priority tiers (project source vs. other)
//! 3. Processes files in two phases: Vue first, then non-Vue source files
//! 4. Follows node_modules dependencies transitively via import resolution
//! 5. Accepts priority signals from `did_open` to dynamically reorder the queue
//!
//! Vue files are processed first because they produce the provider-side Vue
//! artifacts (`.vue.tsx` for IDE analysis and `.vue.ts` for public API) that
//! cross-file resolution depends on.
//!
//! This makes `initialized()` return in <1s instead of blocking for the full scan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;
use verter_host::{CompileProfile, FileKind, UpsertRequest, VerterHost};

use crate::provider_sync::{
    commit_sync_transition, prepare_sync_transition, ProviderPathKind, ProviderSyncState,
    ResolverSnapshot,
};
use crate::tsgo::project_sync::ProjectSync;

/// Handle for communicating with the background workspace scanner.
///
/// Created by [`spawn_workspace_scanner`] and stored on the LSP server.
/// Use [`signal_priority`] from `did_open` to promote a file (and its siblings)
/// to the front of the processing queue.
pub struct WorkspaceScannerHandle {
    tx: mpsc::UnboundedSender<ScannerSignal>,
    task: tokio::task::JoinHandle<()>,
}

impl WorkspaceScannerHandle {
    /// Signal that a file was opened in the editor, promoting it and its
    /// directory siblings to the front of the scan queue.
    pub fn signal_priority(&self, canonical_id: String) {
        let _ = self.tx.send(ScannerSignal::PriorityFile(canonical_id));
    }

    /// Cancel the scanner task. Safe to call multiple times; cancels at the
    /// next `.await` point inside the scanner loop.
    pub fn stop(&self) {
        self.task.abort();
    }
}

/// Signals sent to the background scanner to influence processing order.
pub enum ScannerSignal {
    /// A file was opened in the editor — promote it and its directory siblings.
    PriorityFile(String),
}

/// Configuration for the workspace scanner background task.
pub struct WorkspaceScannerConfig {
    /// Workspace root directories (one per workspace folder).
    pub root_paths: Vec<PathBuf>,
    /// Shared host for upserting and compiling files.
    pub host: Arc<VerterHost>,
    /// Optional project sync for sending files to the type provider.
    pub project_sync: Option<ProjectSync>,
    /// Resolver snapshot used to materialize owner-aware provider paths.
    pub resolver_snapshot: Arc<parking_lot::RwLock<Option<ResolverSnapshot>>>,
    /// Tracks provider materialization per source file (shared with server).
    pub provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    /// Whether the type provider is TSGO (affects sync strategy).
    pub is_tsgo: bool,
    /// Compile profile for IDE output.
    pub tsx_profile: CompileProfile,
    /// Coverage patterns from `verter_vfs::config::discover_tsconfigs()` (e.g., `"C:/project/src/**"`).
    pub tsconfig_patterns: Vec<String>,
    /// Optional oneshot channel fired after the full scanner loop completes
    /// (both Phase 1 `.vue` files and Phase 2 non-Vue source files).
    /// Used by the server to send `$/verter/typeProviderSyncComplete`.
    pub done_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

/// Priority tier for a discovered `.vue` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Project source files covered by a tsconfig.json.
    ProjectSource = 0,
    /// Files outside tsconfig coverage (e.g., scripts/, tools/).
    Other = 1,
}

/// Directories excluded from workspace scanning.
const EXCLUDED_DIRS: &[&str] = &["node_modules", "dist", "build"];

/// Returns true if a directory name should be excluded from workspace scanning.
fn is_excluded_dir(name: &str) -> bool {
    name.starts_with('.') || EXCLUDED_DIRS.contains(&name)
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
    collect_paths_recursive(root, &mut result, |name| name.ends_with(".vue"));
    result
}

/// Recursively collect all non-Vue source file paths (`.ts`, `.tsx`, `.js`, `.jsx`)
/// under `root`.
///
/// Excludes `.d.ts`/`.d.mts`/`.d.cts` files (type declarations loaded via dependency
/// resolution, not FS walk), `node_modules`, dot-directories, `dist`, and `build`.
///
/// Returns paths with forward slashes (canonical form).
pub fn collect_source_paths(root: &Path) -> Vec<String> {
    let mut result = Vec::new();
    collect_paths_recursive(root, &mut result, is_non_vue_source_file);
    result
}

/// Returns true if the file name is a non-Vue source file we want to sync.
fn is_non_vue_source_file(name: &str) -> bool {
    // Must be .ts/.tsx/.js/.jsx but NOT .d.ts/.d.mts/.d.cts
    let is_source_ext = name.ends_with(".ts")
        || name.ends_with(".tsx")
        || name.ends_with(".js")
        || name.ends_with(".jsx")
        || name.ends_with(".mts")
        || name.ends_with(".mjs")
        || name.ends_with(".cts")
        || name.ends_with(".cjs");
    if !is_source_ext {
        return false;
    }
    // Exclude declaration files
    !is_declaration_file(name)
}

/// Returns true if the file name is a TypeScript declaration file.
fn is_declaration_file(name: &str) -> bool {
    name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
}

fn collect_paths_recursive(dir: &Path, result: &mut Vec<String>, matcher: fn(&str) -> bool) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        if path.is_dir() {
            if is_excluded_dir(&name) {
                continue;
            }
            collect_paths_recursive(&path, result, matcher);
        } else if matcher(&name) {
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
    let task = tokio::spawn(scanner_loop(config, rx));
    WorkspaceScannerHandle { tx, task }
}

async fn scanner_loop(
    config: WorkspaceScannerConfig,
    mut rx: mpsc::UnboundedReceiver<ScannerSignal>,
) {
    let roots = config.root_paths.clone();
    let tsconfig_patterns = config.tsconfig_patterns.clone();

    // Step 1: FS walk all roots (blocking) — collect both Vue and non-Vue files
    let (vue_paths, source_paths) = tokio::task::spawn_blocking(move || {
        let mut vue = Vec::new();
        let mut src = Vec::new();
        for root in &roots {
            vue.extend(collect_vue_paths(root));
            src.extend(collect_source_paths(root));
        }
        (vue, src)
    })
    .await
    .unwrap_or_default();

    if vue_paths.is_empty() && source_paths.is_empty() {
        tracing::info!("workspace_scanner: no source files found");
        if let Some(tx) = config.done_tx {
            let _ = tx.send(());
        }
        return;
    }

    // Step 2: Classify into tiers
    let mut vue_classified = classify_tiers(&vue_paths, &tsconfig_patterns);
    let vue_project_count = vue_classified
        .iter()
        .filter(|(_, t)| *t == Tier::ProjectSource)
        .count();
    let mut source_classified = classify_tiers(&source_paths, &tsconfig_patterns);
    let source_project_count = source_classified
        .iter()
        .filter(|(_, t)| *t == Tier::ProjectSource)
        .count();

    // Initial sort (no priority dirs yet)
    priority_sort(&mut vue_classified, &[]);
    priority_sort(&mut source_classified, &[]);

    tracing::info!(
        "workspace_scanner: found {} .vue files ({} project), {} source files ({} project)",
        vue_classified.len(),
        vue_project_count,
        source_classified.len(),
        source_project_count,
    );

    // Tracks all synced files (Vue + non-Vue + node_modules dependencies)
    let mut processed: HashSet<String> = HashSet::new();
    let mut priority_dirs: Vec<String> = Vec::new();
    let mut batch_count: usize = 0;

    // ── Phase 1: All .vue files (produces .vue.ts public API files for barrel re-exports) ──
    let mut idx = 0;
    while idx < vue_classified.len() {
        drain_priority_signals(&mut rx, &mut priority_dirs, &mut vue_classified[idx..]);

        let (ref path, _tier) = vue_classified[idx];
        idx += 1;

        if !processed.insert(path.clone()) {
            continue;
        }

        // Upsert + compile (blocking)
        let path_clone = path.clone();
        let host = Arc::clone(&config.host);
        let profile = config.tsx_profile.clone();

        let compile_ok = tokio::task::spawn_blocking(move || {
            if !crate::compile_blockers::ensure_source_loaded_into_host(&host, &path_clone) {
                return false;
            }
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
                    &config.resolver_snapshot,
                    config.is_tsgo,
                    &config.provider_sync_states,
                )
                .await;
            }
        }

        batch_count += 1;
        if batch_count.is_multiple_of(BATCH_SIZE) {
            tokio::task::yield_now().await;
        }
    }

    tracing::info!(
        "workspace_scanner: phase 1 complete — {} .vue files processed",
        batch_count,
    );

    // ── Phase 2: All non-Vue source files (.ts/.tsx/.js/.jsx) ──
    // Also follows node_modules dependencies transitively.
    let mut node_modules_synced: HashSet<String> = HashSet::new();
    let mut idx = 0;
    while idx < source_classified.len() {
        drain_priority_signals(&mut rx, &mut priority_dirs, &mut source_classified[idx..]);

        let (ref path, _tier) = source_classified[idx];
        idx += 1;

        if !processed.insert(path.clone()) {
            continue;
        }

        if let Some(sync) = &config.project_sync {
            let deps = sync_non_vue_file_to_provider(
                path,
                &config.host,
                sync,
                &config.resolver_snapshot,
                config.is_tsgo,
                &config.provider_sync_states,
            )
            .await;

            // Follow node_modules dependencies transitively
            follow_node_modules_deps(
                deps,
                &config.host,
                sync,
                &config.resolver_snapshot,
                config.is_tsgo,
                &config.provider_sync_states,
                &mut node_modules_synced,
            )
            .await;
        }

        batch_count += 1;
        if batch_count.is_multiple_of(BATCH_SIZE) {
            tokio::task::yield_now().await;
        }
    }

    tracing::info!(
        "workspace_scanner: complete — {} total files ({} .vue, {} source, {} node_modules deps)",
        batch_count + node_modules_synced.len(),
        vue_paths.len(),
        source_paths.len(),
        node_modules_synced.len(),
    );

    // Signal that the full scanner loop (Phase 1 + Phase 2) is complete.
    if let Some(tx) = config.done_tx {
        let _ = tx.send(());
    }
}

/// Drain priority signals from the channel and re-sort remaining unprocessed files.
fn drain_priority_signals(
    rx: &mut mpsc::UnboundedReceiver<ScannerSignal>,
    priority_dirs: &mut Vec<String>,
    remaining: &mut [(String, Tier)],
) {
    while let Ok(signal) = rx.try_recv() {
        match signal {
            ScannerSignal::PriorityFile(canonical_id) => {
                let dir = parent_dir(&canonical_id);
                if !priority_dirs.contains(&dir) {
                    priority_dirs.push(dir);
                }
                priority_sort(remaining, priority_dirs);
            }
        }
    }
}

/// Re-sync a non-Vue source file that changed on disk (outside the editor).
///
/// Called from `did_change_watched_files`. Invalidates the host cache, re-reads
/// from disk, and re-syncs to the type provider.
pub(crate) async fn resync_non_vue_file(
    canonical_id: &str,
    host: &Arc<VerterHost>,
    sync: &ProjectSync,
    resolver_snapshot: &parking_lot::RwLock<Option<ResolverSnapshot>>,
    is_tsgo: bool,
    sync_states: &DashMap<String, ProviderSyncState>,
) {
    // Invalidate host cache so ensure_source_loaded_into_host re-reads from disk
    let host_clone = Arc::clone(host);
    let id_clone = canonical_id.to_string();
    tokio::task::spawn_blocking(move || {
        host_clone.remove(&id_clone);
    })
    .await
    .ok();

    let _ = sync_non_vue_file_to_provider(
        canonical_id,
        host,
        sync,
        resolver_snapshot,
        is_tsgo,
        sync_states,
    )
    .await;
}

/// Sync a non-Vue source file to the type provider.
///
/// Reads from disk, upserts into host, rewrites imports, and loads into provider.
/// Returns the resolved dependencies for node_modules follow-through.
async fn sync_non_vue_file_to_provider(
    canonical_id: &str,
    host: &Arc<VerterHost>,
    sync: &ProjectSync,
    resolver_snapshot: &parking_lot::RwLock<Option<ResolverSnapshot>>,
    is_tsgo: bool,
    sync_states: &DashMap<String, ProviderSyncState>,
) -> Vec<crate::project_resolver::ResolveResult> {
    let snapshot = match resolver_snapshot.read().clone() {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Load source from disk into host
    let host_clone = Arc::clone(host);
    let id_clone = canonical_id.to_string();
    let load_result = tokio::task::spawn_blocking(move || {
        crate::compile_blockers::ensure_source_loaded_into_host(&host_clone, &id_clone);
        host_clone.get_source(&id_clone)
    })
    .await
    .unwrap_or(None);

    let Some(source) = load_result else {
        return Vec::new();
    };

    // Upsert + prepare non-Vue sync (CPU-bound parsing + disk I/O for import resolution)
    let host_clone = Arc::clone(host);
    let snap_clone = snapshot.clone();
    let id_clone = canonical_id.to_string();
    let source_clone = Arc::clone(&source);
    let prepared = tokio::task::spawn_blocking(move || {
        let module_references = host_clone
            .upsert(UpsertRequest {
                canonical_id: Some(id_clone.clone()),
                input_id: id_clone.clone(),
                source: source_clone.clone(),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .map(|result| result.module_references)
            .unwrap_or_default();

        let reader = crate::compile_blockers::HostFsProjectResolverReader::new(&host_clone);
        crate::server::prepare_non_vue_provider_sync(
            Some(&snap_clone),
            &reader,
            &id_clone,
            &source_clone,
            &module_references,
        )
    })
    .await
    .unwrap_or(None);

    let Some(prepared) = prepared else {
        return Vec::new();
    };

    // Sync state management
    let next_state =
        crate::provider_sync::non_vue_sync_state_for_source(&snapshot.resolver, canonical_id);
    if let Some(next) = next_state {
        if is_tsgo {
            crate::server::configure_provider_paths_for_source(sync, &snapshot, canonical_id, true)
                .await;
        }
        let transition = prepare_sync_transition(sync_states, canonical_id, next);
        close_stale_paths(sync, &transition.stale_paths).await;
        let mut committed = transition.next;

        if let Err(error) = sync
            .load_file(&prepared.provider_path, &prepared.rewritten)
            .await
        {
            tracing::warn!(
                "workspace_scanner: failed to load non-Vue file {}: {error}",
                prepared.provider_path
            );
        } else {
            committed.set_background_loaded(ProviderPathKind::Shadow, true);
        }

        commit_sync_transition(sync_states, canonical_id, committed);
    } else {
        // No project owner — still sync for import resolution
        if let Err(error) = sync
            .load_file(&prepared.provider_path, &prepared.rewritten)
            .await
        {
            tracing::warn!(
                "workspace_scanner: failed to load non-Vue file {}: {error}",
                prepared.provider_path
            );
        }
    }

    // Store import dependencies in host for dependency tracking
    if !prepared.resolved_dependencies.is_empty() {
        host.set_import_dependencies(
            canonical_id,
            prepared
                .resolved_dependencies
                .iter()
                .map(|entry| verter_host::DependencyResolution {
                    specifier: entry.provider_specifier.clone(),
                    resolved_canonical_id: Some(entry.source_id.clone()),
                    possible_canonical_ids: Vec::new(),
                })
                .collect(),
        );
    }

    prepared.resolved_dependencies
}

/// Follow node_modules dependencies transitively via BFS.
///
/// For each resolved dependency that targets a node_modules file (ProviderTarget::SourceFile
/// with a node_modules path), reads the file, rewrites its imports, syncs to the provider,
/// and follows its own dependencies recursively.
async fn follow_node_modules_deps(
    initial_deps: Vec<crate::project_resolver::ResolveResult>,
    host: &Arc<VerterHost>,
    sync: &ProjectSync,
    resolver_snapshot: &parking_lot::RwLock<Option<ResolverSnapshot>>,
    is_tsgo: bool,
    sync_states: &DashMap<String, ProviderSyncState>,
    node_modules_synced: &mut HashSet<String>,
) {
    let mut pending: Vec<crate::project_resolver::ResolveResult> = initial_deps;

    while let Some(dep) = pending.pop() {
        // Handle Vue public API dependencies (sync .vue.ts files)
        if dep.provider_target == crate::project_resolver::ProviderTarget::VuePublicApi {
            // Vue public API files are handled in phase 1 by sync_file_to_provider
            continue;
        }

        // Handle shadow source files (non-Vue workspace files — already in phase 2 queue)
        if dep.provider_target == crate::project_resolver::ProviderTarget::ShadowSourceFile {
            // These are workspace files already queued in source_classified
            continue;
        }

        // ProviderTarget::SourceFile — may be node_modules
        if !dep.source_id.contains("node_modules") {
            // Non-node_modules SourceFile — provider reads from disk
            continue;
        }

        if !node_modules_synced.insert(dep.source_id.clone()) {
            // Already synced
            continue;
        }

        // Load, rewrite, and sync the node_modules file
        let child_deps = sync_non_vue_file_to_provider(
            &dep.source_id,
            host,
            sync,
            resolver_snapshot,
            is_tsgo,
            sync_states,
        )
        .await;

        // Follow its dependencies too (transitive)
        pending.extend(child_deps);

        // Yield periodically to prevent starvation
        if node_modules_synced.len().is_multiple_of(BATCH_SIZE) {
            tokio::task::yield_now().await;
        }
    }
}

/// Sync a single compiled file's IDE and DTS output to the type provider.
async fn sync_file_to_provider(
    canonical_id: &str,
    host: &VerterHost,
    profile: &CompileProfile,
    sync: &ProjectSync,
    resolver_snapshot: &parking_lot::RwLock<Option<ResolverSnapshot>>,
    is_tsgo: bool,
    sync_states: &DashMap<String, ProviderSyncState>,
) {
    let Some(snapshot) = resolver_snapshot.read().clone() else {
        return;
    };
    let reader = crate::compile_blockers::HostFsProjectResolverReader::new(host);
    crate::compile_blockers::hydrate_vue_compile_blockers(
        host,
        &snapshot.resolver,
        &reader,
        canonical_id,
    );
    let _ = host.ensure_compiled(canonical_id, profile);
    let ide = host.get_ide(canonical_id, profile);
    let is_jsx = ide.as_ref().map(|ide| ide.is_jsx).unwrap_or(false);
    let Some(next_state) =
        crate::provider_sync::vue_sync_state_for_source(&snapshot.resolver, canonical_id, is_jsx)
    else {
        return;
    };
    if is_tsgo {
        crate::server::configure_provider_paths_for_source(sync, &snapshot, canonical_id, true)
            .await;
    }
    let transition = prepare_sync_transition(sync_states, canonical_id, next_state);
    close_stale_paths(sync, &transition.stale_paths).await;
    let mut committed_state = transition.next;

    // Sync DTS (both TSGO and tsserver)
    if let Some(api) = host.get_public_api(canonical_id) {
        let Some(dts_path) = committed_state.api_path.clone() else {
            return;
        };
        let result = if is_tsgo {
            sync.open_dts(&dts_path, &api.code).await
        } else {
            sync.load_dts(&dts_path, &api.code).await
        };
        if result.is_ok() {
            committed_state.set_background_loaded(ProviderPathKind::Api, true);
        }
    }

    // Sync IDE artifact (both TSGO and tsserver)
    if let Some(ide) = ide {
        let Some(tsx_path) = committed_state.ide_path.clone() else {
            return;
        };
        let result = if is_tsgo {
            sync.open_tsx(&tsx_path, &ide.code).await
        } else {
            sync.load_tsx(&tsx_path, &ide.code).await
        };
        if result.is_ok() {
            committed_state.set_background_loaded(ProviderPathKind::Ide, true);
        }
    }

    commit_sync_transition(sync_states, canonical_id, committed_state);
}

async fn close_stale_paths(sync: &ProjectSync, stale_paths: &[(ProviderPathKind, String)]) {
    for (kind, path) in stale_paths {
        let result = match kind {
            ProviderPathKind::Ide => sync.close_tsx(path).await,
            ProviderPathKind::Api => sync.close_dts(path).await,
            ProviderPathKind::Shadow => sync.close_file(path).await,
        };
        if let Err(error) = result {
            tracing::warn!(
                "workspace_scanner: failed to close stale provider path {path}: {error}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsgo::mock::{MockCall, MockTypeProvider};
    use crate::ProjectSyncMode;
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

    #[tokio::test]
    async fn scanner_assigns_each_vue_to_exactly_one_owner_project() {
        let host = VerterHost::new_standalone(verter_host::HostConfig::default());
        let canonical_id = "/workspace/pkg-a/src/App.vue";
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::<str>::from("<template><div>App</div></template>"),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_host::CompileTarget::BUNDLER | verter_host::CompileTarget::TSX,
            ..CompileProfile::default()
        };
        assert!(host.ensure_compiled(canonical_id, &profile).is_ok());

        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace/pkg-a".to_string(),
                "/workspace".to_string(),
                Some("/workspace/pkg-a/tsconfig.json".to_string()),
            ),
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                None,
            ),
        ]);
        let snapshot = parking_lot::RwLock::new(Some(ResolverSnapshot {
            generation: 1,
            resolver,
        }));
        let sync_states = DashMap::new();
        let sync = ProjectSync::new(
            Arc::new(MockTypeProvider::new()),
            ProjectSyncMode::FullProject,
        );

        sync_file_to_provider(
            canonical_id,
            &host,
            &profile,
            &sync,
            &snapshot,
            false,
            &sync_states,
        )
        .await;

        let state = sync_states
            .get(canonical_id)
            .expect("scanner should commit a source-keyed provider state");
        assert_eq!(
            state.owner_key, "/workspace/pkg-a/tsconfig.json",
            "matched Vue files should have owner_key set to the tsconfig path"
        );
    }

    #[tokio::test]
    async fn scanner_routes_unmatched_files_to_workspace_project_only() {
        let host = VerterHost::new_standalone(verter_host::HostConfig::default());
        let canonical_id = "/workspace/scripts/Tool.vue";
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::<str>::from("<template><div>Tool</div></template>"),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_host::CompileTarget::BUNDLER | verter_host::CompileTarget::TSX,
            ..CompileProfile::default()
        };
        assert!(host.ensure_compiled(canonical_id, &profile).is_ok());

        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace/src".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.app.json".to_string()),
            ),
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                None,
            ),
        ]);
        let snapshot = parking_lot::RwLock::new(Some(ResolverSnapshot {
            generation: 1,
            resolver,
        }));
        let sync_states = DashMap::new();
        let sync = ProjectSync::new(
            Arc::new(MockTypeProvider::new()),
            ProjectSyncMode::FullProject,
        );

        sync_file_to_provider(
            canonical_id,
            &host,
            &profile,
            &sync,
            &snapshot,
            false,
            &sync_states,
        )
        .await;

        let state = sync_states
            .get(canonical_id)
            .expect("unmatched Vue files should still sync to the workspace project");
        assert_eq!(
            state.owner_key,
            "/workspace",
            "unmatched Vue files should have owner_key set to the workspace root (fallback project)"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // collect_source_paths — non-Vue file collection
    // ═══════════════════════════════════════════════════════════

    #[tokio::test]
    async fn scanner_syncs_vue_ide_artifact_for_tsgo() {
        let host = VerterHost::new_standalone(verter_host::HostConfig::default());
        let canonical_id = "/workspace/src/App.vue";
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::<str>::from(
                r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child msg="hi" /></template>"#,
            ),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        });
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some("/workspace/src/Child.vue".to_string()),
            input_id: "/workspace/src/Child.vue".to_string(),
            source: Arc::<str>::from(
                r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
            ),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_host::CompileTarget::BUNDLER | verter_host::CompileTarget::TSX,
            ..CompileProfile::default()
        };
        assert!(host.ensure_compiled(canonical_id, &profile).is_ok());
        assert!(host
            .ensure_compiled("/workspace/src/Child.vue", &profile)
            .is_ok());

        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            ),
        ]);
        let snapshot = parking_lot::RwLock::new(Some(ResolverSnapshot {
            generation: 1,
            resolver,
        }));
        let sync_states = DashMap::new();
        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);

        sync_file_to_provider(
            canonical_id,
            &host,
            &profile,
            &sync,
            &snapshot,
            true,
            &sync_states,
        )
        .await;

        let calls = provider.calls();
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == "/workspace/src/App.vue.tsx"
            )),
            "TSGO scanner sync should open the Vue IDE artifact, calls={calls:?}"
        );
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == "/workspace/src/App.vue.ts"
            )),
            "TSGO scanner sync should keep syncing the public API artifact too, calls={calls:?}"
        );
    }

    #[tokio::test]
    async fn scanner_configures_tsgo_paths_before_opening_vue_artifacts() {
        let tmp = TempDir::new().expect("temp project should exist");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).expect("src dir should exist");
        fs::write(
            root.join("tsconfig.json"),
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
        )
        .expect("tsconfig should be written");

        let canonical_id = root.join("src").join("App.vue");
        let canonical_id = canonical_id.to_string_lossy().replace('\\', "/");
        let host = VerterHost::new_standalone(verter_host::HostConfig::default());
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: Arc::<str>::from(
                r#"<script setup lang="ts">
import Child from '@/Child.vue'
</script>
<template><div /></template>"#,
            ),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_host::CompileTarget::BUNDLER | verter_host::CompileTarget::TSX,
            ..CompileProfile::default()
        };
        assert!(host.ensure_compiled(&canonical_id, &profile).is_ok());

        let tsconfig_path = root
            .join("tsconfig.json")
            .to_string_lossy()
            .replace('\\', "/");
        let root_path = root.to_string_lossy().replace('\\', "/");
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                root_path.clone(),
                root_path.clone(),
                Some(tsconfig_path),
            ),
        ]);
        let snapshot = parking_lot::RwLock::new(Some(ResolverSnapshot {
            generation: 1,
            resolver,
        }));
        let sync_states = DashMap::new();
        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
        assert!(
            snapshot
                .read()
                .as_ref()
                .and_then(|snapshot| snapshot.resolver.owner_for_file(&canonical_id))
                .is_some(),
            "resolver should match the Vue file to the temp tsconfig owner"
        );
        let ws = verter_vfs::FilesystemWorkspace::new(verter_vfs::FilesystemOptions::default());
        let (expected_base_url, expected_paths) =
            verter_vfs::config::raw_paths_json(&ws, &root.join("tsconfig.json").to_string_lossy())
                .expect("raw_paths_json should read the temp tsconfig");

        sync_file_to_provider(
            &canonical_id,
            &host,
            &profile,
            &sync,
            &snapshot,
            true,
            &sync_states,
        )
        .await;

        let calls = provider.calls();
        let configure_index = calls
            .iter()
            .position(|call| matches!(call, MockCall::ConfigurePaths { .. }))
            .expect("TSGO scanner sync should configure owner paths from tsconfig");
        assert!(
            matches!(
                &calls[configure_index],
                MockCall::ConfigurePaths { base_url, paths }
                    if base_url == &expected_base_url && paths == &expected_paths
            ),
            "unexpected configure_paths payload, calls={calls:?}"
        );
        let first_open_index = calls
            .iter()
            .position(|call| matches!(call, MockCall::OpenFile { .. }))
            .expect("TSGO scanner sync should open provider files");
        assert!(
            configure_index < first_open_index,
            "path config must be sent before any provider file opens, calls={calls:?}"
        );
    }

    fn create_mixed_test_dir() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Source files (various extensions)
        fs::create_dir_all(root.join("src/utils")).unwrap();
        fs::write(root.join("src/App.vue"), "<template><div/></template>").unwrap();
        fs::write(root.join("src/main.ts"), "import App from './App.vue'").unwrap();
        fs::write(root.join("src/utils/helpers.ts"), "export const x = 1").unwrap();
        fs::write(root.join("src/utils/format.js"), "export function fmt() {}").unwrap();
        fs::write(root.join("src/types.tsx"), "export type Props = {}").unwrap();
        fs::write(root.join("src/render.jsx"), "export default <div/>").unwrap();

        // .d.ts files (should be excluded)
        fs::write(root.join("src/env.d.ts"), "declare module '*.vue' {}").unwrap();
        fs::write(root.join("src/global.d.mts"), "export {}").unwrap();

        // Excluded directories
        fs::create_dir_all(root.join("node_modules/vue")).unwrap();
        fs::write(root.join("node_modules/vue/index.js"), "export {}").unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/config"), "").unwrap();
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(root.join("dist/app.js"), "").unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build/output.js"), "").unwrap();

        tmp
    }

    #[test]
    fn test_collect_source_paths_finds_ts_js_tsx_jsx() {
        let tmp = create_mixed_test_dir();
        let paths = collect_source_paths(tmp.path());

        // Positive: finds .ts, .js, .tsx, .jsx files
        assert!(
            paths.iter().any(|p| p.ends_with("src/main.ts")),
            "should find .ts files"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("src/utils/helpers.ts")),
            "should find nested .ts files"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("src/utils/format.js")),
            "should find .js files"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("src/types.tsx")),
            "should find .tsx files"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("src/render.jsx")),
            "should find .jsx files"
        );

        // Negative: must NOT include .vue files
        assert!(
            !paths.iter().any(|p| p.ends_with(".vue")),
            "should not include .vue files (those use collect_vue_paths)"
        );
        // Negative: must NOT include .d.ts files
        assert!(
            !paths
                .iter()
                .any(|p| p.contains(".d.ts") || p.contains(".d.mts") || p.contains(".d.cts")),
            "should not include .d.ts/.d.mts/.d.cts files"
        );
        // Negative: must NOT include node_modules, dot-dirs, dist, build
        assert!(
            !paths.iter().any(|p| p.contains("node_modules")),
            "must not include node_modules"
        );
        assert!(
            !paths.iter().any(|p| p.contains(".git")),
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

        // All paths use forward slashes
        for p in &paths {
            assert!(!p.contains('\\'), "paths should use forward slashes: {p}");
        }
    }

    #[test]
    fn test_collect_source_paths_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let paths = collect_source_paths(tmp.path());
        assert!(paths.is_empty(), "empty dir should return no paths");
    }

    #[test]
    fn test_classify_tiers_works_for_non_vue() {
        let paths = vec![
            "C:/project/src/main.ts".to_string(),
            "C:/project/src/utils/helpers.ts".to_string(),
            "C:/project/scripts/build.js".to_string(),
        ];
        let patterns = vec!["C:/project/src/**".to_string()];

        let classified = classify_tiers(&paths, &patterns);

        // src/ files → ProjectSource
        assert_eq!(classified[0].1, Tier::ProjectSource);
        assert_eq!(classified[1].1, Tier::ProjectSource);
        // scripts/ → Other
        assert_eq!(classified[2].1, Tier::Other);
    }

    // ═══════════════════════════════════════════════════════════
    // is_non_vue_source_file / is_declaration_file
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_is_non_vue_source_file() {
        // Positive: standard source extensions
        assert!(is_non_vue_source_file("main.ts"));
        assert!(is_non_vue_source_file("App.tsx"));
        assert!(is_non_vue_source_file("utils.js"));
        assert!(is_non_vue_source_file("render.jsx"));
        assert!(is_non_vue_source_file("config.mts"));
        assert!(is_non_vue_source_file("config.mjs"));
        assert!(is_non_vue_source_file("config.cts"));
        assert!(is_non_vue_source_file("config.cjs"));

        // Negative: declaration files
        assert!(!is_non_vue_source_file("env.d.ts"), ".d.ts excluded");
        assert!(!is_non_vue_source_file("global.d.mts"), ".d.mts excluded");
        assert!(!is_non_vue_source_file("types.d.cts"), ".d.cts excluded");

        // Negative: non-source extensions
        assert!(!is_non_vue_source_file("App.vue"), ".vue not source");
        assert!(!is_non_vue_source_file("style.css"), ".css not source");
        assert!(!is_non_vue_source_file("readme.md"), ".md not source");
        assert!(!is_non_vue_source_file("data.json"), ".json not source");
    }

    #[test]
    fn test_is_declaration_file() {
        assert!(is_declaration_file("env.d.ts"));
        assert!(is_declaration_file("global.d.mts"));
        assert!(is_declaration_file("types.d.cts"));
        assert!(is_declaration_file("node_modules/@types/vue/index.d.ts"));

        assert!(
            !is_declaration_file("main.ts"),
            ".ts is not a declaration file"
        );
        assert!(
            !is_declaration_file("utils.mts"),
            ".mts is not a declaration file"
        );
    }

    #[test]
    fn test_is_excluded_dir() {
        assert!(is_excluded_dir("node_modules"));
        assert!(is_excluded_dir("dist"));
        assert!(is_excluded_dir("build"));
        assert!(is_excluded_dir(".git"));
        assert!(is_excluded_dir(".vscode"));

        assert!(!is_excluded_dir("src"), "src should not be excluded");
        assert!(!is_excluded_dir("lib"), "lib should not be excluded");
        assert!(
            !is_excluded_dir("packages"),
            "packages should not be excluded"
        );
    }
}
