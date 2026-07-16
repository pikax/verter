//! Async priority-based workspace scanner for LSP initialization.
//!
//! Instead of synchronously scanning all files during `initialized()` (which
//! blocks the LSP handler for seconds), this module spawns a background task that:
//!
//! 1. Walks the filesystem for `.vue` and non-carrier source files (`.ts`, `.tsx`, `.js`, `.jsx`)
//! 2. Classifies them into priority tiers (project source vs. other)
//! 3. Processes files in two phases: Vue first, then non-carrier source files
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

use dashmap::{DashMap, DashSet};
use tokio::sync::mpsc;
use verter_session::{CompileProfile, UpsertRequest, VerterHost};

#[cfg(test)]
use verter_session::FileLanguage;

use crate::provider_sync::{
    commit_sync_transition, genuinely_stale_after_sync, non_decl_close_targets,
    prepare_sync_transition, revert_unsynced_kinds, NonDeclProviderPathKind, ProviderPathKind,
    ProviderSyncState,
};
use crate::type_provider::project_sync::ProjectSync;

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
    /// VFS workspace for published resolver snapshot.
    pub vfs_workspace: Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>>,
    /// Tracks provider materialization per source file (shared with server).
    pub provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    /// The generation-stamped provider-surface store (shared with the server's
    /// `DocumentRegistry`). The background scan records/forgets a generation here
    /// for each carrier API surface it syncs/closes, so a cross-file rename can
    /// pin and map against the exact generation — scanner-synced (closed) carrier
    /// surfaces included.
    pub provider_surfaces: crate::provider_surface_store::ProviderSurfaceStore,
    /// Whether the type provider is TSGO (affects sync strategy).
    pub is_tsgo: bool,
    /// The tsserver carrier-publish coordinator (the store-publish membership
    /// authority). `Some` for the tsserver engine so the background scan PUBLISHES /
    /// RETRACTS carrier membership through the single carrier-sync gateway; `None`
    /// for tsgo (whose carriers reach the engine through the project-bound `--api`
    /// direct open — `open_project` + `root_files`).
    pub carrier_publish_coordinator: Option<crate::external_ts::CarrierPublishCoordinator>,
    /// The per-source carrier transaction coordinator (admission gate, owner-loss barrier,
    /// non-owned retry disposition), shared with the server so the background scan's carrier
    /// commits and non-owned settlements serialize on the ONE barrier map.
    pub carrier_transaction_coordinator: Arc<crate::external_ts::CarrierTransactionCoordinator>,
    /// The server's pending-provider-sync requeue set (shared). A scan-lane carrier commit
    /// refused by the admission gate (Superseded), or a transiently not-ready / not-advertised
    /// carrier, is re-queued here so the drain retries it through a fresh transaction — the
    /// same interactive requeue path, never a requeue-less drop.
    pub pending_snapshot_provider_sync: Arc<DashSet<String>>,
    /// Compile profile for IDE output.
    pub tsx_profile: CompileProfile,
    /// Coverage patterns from `verter_workspace::config::discover_tsconfigs()` (e.g., `"C:/project/src/**"`).
    /// Legacy — used when `workspace_snapshot` is `None`.
    pub tsconfig_patterns: Vec<String>,
    /// Published workspace snapshot for ownership-based tier classification.
    /// When `Some`, `classify_from_snapshot()` is used instead of
    /// `classify_tiers()`. Generation-pinned at spawn time.
    pub workspace_snapshot: Option<std::sync::Arc<verter_workspace::WorkspaceSnapshot>>,
    /// Optional oneshot channel fired after the full scanner loop completes
    /// (both `.vue` files and non-carrier source files).
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
const EXCLUDED_DIRS: &[&str] = &["node_modules"];

/// Returns true if a directory name should be excluded from workspace scanning.
fn is_excluded_dir(name: &str) -> bool {
    EXCLUDED_DIRS.contains(&name)
}

/// Recursively collect all `.vue` file paths under `root`.
///
/// Skips only fallback-excluded directories (`node_modules`).
///
/// Returns paths with forward slashes (canonical form).
pub fn collect_carrier_paths(
    workspace: &dyn verter_workspace::WorkspaceRead,
    root: &Path,
) -> Vec<String> {
    let root_str = root.to_string_lossy().replace('\\', "/");
    workspace
        .walk(
            &root_str,
            &|dir: &str| {
                let name = dir.rsplit('/').next().unwrap_or(dir);
                !is_excluded_dir(name)
            },
            &|file: &str| {
                // Any framework CARRIER file (`.vue` / `.svelte`), from the
                // registry carrier-extension set — not a `.vue`-literal.
                verter_workspace::path_is_carrier(file)
            },
        )
        .unwrap_or_default()
}

/// Recursively collect all non-carrier source file paths (`.ts`, `.tsx`, `.js`, `.jsx`)
/// under `root`.
///
/// Excludes `.d.ts`/`.d.mts`/`.d.cts` files (type declarations loaded via dependency
/// resolution, not FS walk) and fallback-excluded directories (`node_modules`).
///
/// Returns paths with forward slashes (canonical form).
pub fn collect_source_paths(
    workspace: &dyn verter_workspace::WorkspaceRead,
    root: &Path,
) -> Vec<String> {
    let root_str = root.to_string_lossy().replace('\\', "/");
    workspace
        .walk(
            &root_str,
            &|dir: &str| {
                let name = dir.rsplit('/').next().unwrap_or(dir);
                !is_excluded_dir(name)
            },
            &|file: &str| {
                let name = file.rsplit('/').next().unwrap_or(file);
                is_plain_source_file(name)
            },
        )
        .unwrap_or_default()
}

/// Returns true if the file name is a non-carrier source file we want to sync.
fn is_plain_source_file(name: &str) -> bool {
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

/// Classify paths into priority scan tiers using the published workspace snapshot.
///
/// A path is `Tier::ProjectSource` if any configured project in the snapshot
/// claims it. Otherwise it's `Tier::Other`.
///
/// This is a scan-PRIORITIZATION hint, NOT the authoritative ownership decision.
/// It shares the SAME F2 authority the serve path uses — the snapshot's
/// [`configured_owner_resolution_for_file`](verter_workspace::WorkspaceSnapshot::configured_owner_resolution_for_file)
/// (solution-graph-pruned) — so a `ProjectSource`/`Other` tier can never disagree
/// with the serve path on whether a file is configured at all
/// (`scanner_tier_and_resolver_ownership_are_byte_equivalent` guards this axis). But
/// it deliberately COLLAPSES `Unique` and `Ambiguous` into one `ProjectSource` tier:
/// for scan ORDER, both a uniquely-owned and an ambiguously-owned carrier are worth
/// scanning early, so the fine-grained bind-vs-ambiguous distinction is not needed
/// here. That authoritative distinction — `Bound` (serve) vs `NoProject` / `Ambiguous`
/// (terminal, no serve) vs `NotReady` (retry) — is re-resolved per sync pass through
/// the carrier-sync gateway's single captured
/// [`CarrierOwnershipResolution`](crate::external_ts::CarrierOwnershipResolution),
/// never read back from a scan tier. A stale-snapshot tier only mis-PRIORITIZES a
/// scan; it can never cause a wrong ownership decision.
pub fn classify_from_snapshot(
    paths: &[String],
    snapshot: &verter_workspace::WorkspaceSnapshot,
) -> Vec<(String, Tier)> {
    paths
        .iter()
        .map(|path| {
            let tier = match snapshot.configured_owner_resolution_for_file(path) {
                verter_workspace::ConfiguredOwnerResolution::None => Tier::Other,
                // Unique or Ambiguous → both are configured (a scan-priority tier, not
                // the authoritative bind decision; the gateway re-resolves at serve).
                _ => Tier::ProjectSource,
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
    let workspace_snapshot = config.workspace_snapshot.clone();
    let vfs_workspace = config.vfs_workspace.clone();

    // Step 1: FS walk all roots (blocking) — collect both Vue and non-carrier files
    let (carrier_paths, source_paths) = tokio::task::spawn_blocking(move || {
        let mut carrier = Vec::new();
        let mut src = Vec::new();
        let ws_handle = vfs_workspace.read().clone();
        let ws = ws_handle.unwrap_or_else(|| {
            Arc::new(verter_workspace::FilesystemWorkspace::new(
                verter_workspace::FilesystemOptions::default(),
            ))
        });
        for root in &roots {
            carrier.extend(collect_carrier_paths(&*ws, root));
            src.extend(collect_source_paths(&*ws, root));
        }
        (carrier, src)
    })
    .await
    .unwrap_or_default();

    if carrier_paths.is_empty() && source_paths.is_empty() {
        tracing::info!("workspace_scanner: no source files found");
        if let Some(tx) = config.done_tx {
            let _ = tx.send(());
        }
        return;
    }

    // Step 2: Classify into tiers (prefer snapshot, fallback to patterns)
    let mut carrier_classified = if let Some(ref snap) = workspace_snapshot {
        classify_from_snapshot(&carrier_paths, snap)
    } else {
        classify_tiers(&carrier_paths, &tsconfig_patterns)
    };
    let carrier_project_count = carrier_classified
        .iter()
        .filter(|(_, t)| *t == Tier::ProjectSource)
        .count();
    let mut source_classified = if let Some(ref snap) = workspace_snapshot {
        classify_from_snapshot(&source_paths, snap)
    } else {
        classify_tiers(&source_paths, &tsconfig_patterns)
    };
    let source_project_count = source_classified
        .iter()
        .filter(|(_, t)| *t == Tier::ProjectSource)
        .count();

    // Initial sort (no priority dirs yet)
    priority_sort(&mut carrier_classified, &[]);
    priority_sort(&mut source_classified, &[]);

    tracing::info!(
        "workspace_scanner: found {} carrier files ({} project), {} source files ({} project)",
        carrier_classified.len(),
        carrier_project_count,
        source_classified.len(),
        source_project_count,
    );

    // Tracks all synced files (carrier + non-carrier + node_modules dependencies)
    let mut processed: HashSet<String> = HashSet::new();
    let mut priority_dirs: Vec<String> = Vec::new();
    let mut batch_count: usize = 0;

    // ── All carrier files (produce carrier public API artifacts for barrel re-exports) ──
    let mut idx = 0;
    while idx < carrier_classified.len() {
        drain_priority_signals(&mut rx, &mut priority_dirs, &mut carrier_classified[idx..]);

        let (ref path, _tier) = carrier_classified[idx];
        idx += 1;

        if !processed.insert(path.clone()) {
            continue;
        }

        // Upsert + compile (blocking)
        let path_clone = path.clone();
        let host = Arc::clone(&config.host);
        let profile = config.tsx_profile.clone();

        let compile_ok = tokio::task::spawn_blocking(move || {
            if !&host.ensure_loaded(&path_clone) {
                return false;
            }
            // IDE-sync: gate on the IDE/TSX surface, NOT the runtime `Main`
            // node. A Main-less carrier (Svelte) projects only `CachedTsx`, so
            // `ensure_compiled` (which demands `Main`) would report failure and
            // the file would never reach the type provider. `ensure_ide_compiled`
            // gates on the IDE surface: `Ok(true)` ⇒ sync to the provider.
            host.ensure_ide_compiled(&path_clone, &profile)
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false);

        // Sync to type provider
        if compile_ok
            && (config.project_sync.is_some() || config.carrier_publish_coordinator.is_some())
        {
            sync_file_to_provider(
                path,
                &config.host,
                &config.tsx_profile,
                config.project_sync.as_ref(),
                &config.provider_surfaces,
                &config.vfs_workspace,
                config.is_tsgo,
                &config.provider_sync_states,
                config.carrier_publish_coordinator.as_ref(),
                &config.carrier_transaction_coordinator,
                Some(&config.pending_snapshot_provider_sync),
            )
            .await;
        }

        batch_count += 1;
        if batch_count.is_multiple_of(BATCH_SIZE) {
            tokio::task::yield_now().await;
        }
    }

    tracing::info!(
        "workspace_scanner: carrier file pass complete — {} carrier files processed",
        batch_count,
    );

    // ── All non-carrier source files (.ts/.tsx/.js/.jsx) ──
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
            let deps = sync_non_carrier_file_to_provider(
                path,
                &config.host,
                sync,
                &config.provider_surfaces,
                &config.vfs_workspace,
                config.is_tsgo,
                &config.provider_sync_states,
            )
            .await;

            // Follow node_modules dependencies transitively
            follow_node_modules_deps(
                deps,
                &config.host,
                sync,
                &config.provider_surfaces,
                &config.vfs_workspace,
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
        carrier_paths.len(),
        source_paths.len(),
        node_modules_synced.len(),
    );

    // Signal that the full scanner loop is complete.
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

/// Re-sync a non-carrier source file that changed on disk (outside the editor).
///
/// Called from `did_change_watched_files`. Invalidates the host cache, re-reads
/// from disk, and re-syncs to the type provider.
pub(crate) async fn resync_non_carrier_file(
    canonical_id: &str,
    host: &Arc<VerterHost>,
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
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

    let _ = sync_non_carrier_file_to_provider(
        canonical_id,
        host,
        sync,
        provider_surfaces,
        vfs_workspace,
        is_tsgo,
        sync_states,
    )
    .await;
}

/// Sync a non-carrier source file to the type provider.
///
/// Reads from disk, upserts into host, rewrites imports, and loads into provider.
/// Returns the resolved dependencies for node_modules follow-through.
async fn sync_non_carrier_file_to_provider(
    canonical_id: &str,
    host: &Arc<VerterHost>,
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    is_tsgo: bool,
    sync_states: &DashMap<String, ProviderSyncState>,
) -> Vec<crate::project_resolver::ResolveResult> {
    let snapshot = {
        let ws = vfs_workspace.read();
        ws.as_ref().and_then(|ws| {
            let published = ws.load_published()?;
            Some(crate::server::PublishedResolverSnapshot {
                resolver: published.snapshot.resolver.clone(),
                ownership_ready: published.ownership_ready,
            })
        })
    };
    let snapshot = match snapshot {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Load source from disk into host
    let host_clone = Arc::clone(host);
    let id_clone = canonical_id.to_string();
    let load_result = tokio::task::spawn_blocking(move || {
        host_clone.ensure_loaded(&id_clone);
        host_clone.get_source(&id_clone)
    })
    .await
    .unwrap_or(None);

    let Some(source) = load_result else {
        return Vec::new();
    };

    // Framework carriers never sync to the provider as raw scripts.
    let Some(file_language) = crate::provider_sync::provider_script_language(host, canonical_id)
    else {
        return Vec::new();
    };

    // Upsert + prepare non-carrier sync (CPU-bound parsing + disk I/O for import resolution)
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
                file_language,
                aliases: Vec::new(),
            })
            .map(|result| result.module_references)
            .unwrap_or_default();

        // `prepare_non_carrier_provider_sync` is
        // a read-only consumer; route through `host.workspace_read()`.
        let ws = host_clone.workspace_read();
        crate::server::prepare_non_carrier_provider_sync(
            Some(&snap_clone),
            ws.as_ref(),
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
        crate::provider_sync::non_carrier_sync_state_for_source(&snapshot.resolver, canonical_id);
    if let Some(next) = next_state {
        if is_tsgo {
            crate::server::configure_provider_paths_for_source(sync, &snapshot, canonical_id, true)
                .await;
        }
        let transition = prepare_sync_transition(sync_states, canonical_id, next);
        close_stale_paths(
            sync,
            provider_surfaces,
            &non_decl_close_targets(&transition.stale_paths),
        )
        .await;
        let mut committed = transition.next;

        if let Err(error) = sync
            .load_file(&prepared.provider_path, &prepared.rewritten)
            .await
        {
            tracing::warn!(
                "workspace_scanner: failed to load non-carrier file {}: {error}",
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
                "workspace_scanner: failed to load non-carrier file {}: {error}",
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
                .map(|entry| verter_session::DependencyResolution {
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
#[allow(
    clippy::too_many_arguments,
    reason = "node_modules follow-through threads the provider-surface store alongside its sync inputs"
)]
async fn follow_node_modules_deps(
    initial_deps: Vec<crate::project_resolver::ResolveResult>,
    host: &Arc<VerterHost>,
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    is_tsgo: bool,
    sync_states: &DashMap<String, ProviderSyncState>,
    node_modules_synced: &mut HashSet<String>,
) {
    let mut pending: Vec<crate::project_resolver::ResolveResult> = initial_deps;

    while let Some(dep) = pending.pop() {
        // Handle Vue public API dependencies (sync .vue.verter.ts files)
        if dep.provider_target == crate::project_resolver::ProviderTarget::CarrierPublicApi {
            // Vue public API files are handled in by sync_file_to_provider
            continue;
        }

        // Handle shadow source files (non-carrier workspace files — already in queue)
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
        let child_deps = sync_non_carrier_file_to_provider(
            &dep.source_id,
            host,
            sync,
            provider_surfaces,
            vfs_workspace,
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
#[allow(
    clippy::too_many_arguments,
    reason = "carrier file sync threads the provider-surface store alongside its compile/sync inputs"
)]
async fn sync_file_to_provider(
    canonical_id: &str,
    host: &VerterHost,
    profile: &CompileProfile,
    sync: Option<&ProjectSync>,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    is_tsgo: bool,
    sync_states: &DashMap<String, ProviderSyncState>,
    carrier_publish_coordinator: Option<&crate::external_ts::CarrierPublishCoordinator>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
    requeue: Option<&DashSet<String>>,
) {
    // Capture the published resolver snapshot AND the filesystem-workspace handle in
    // one read (the gateway's membership reconcile resolves ownership against the
    // SAME published snapshot). The guard is dropped before the awaits below.
    let Some((snapshot, vfs_handle)) = ({
        let ws = vfs_workspace.read();
        ws.as_ref().and_then(|ws| {
            let published = ws.load_published()?;
            Some((
                crate::server::PublishedResolverSnapshot {
                    resolver: published.snapshot.resolver.clone(),
                    ownership_ready: published.ownership_ready,
                },
                Arc::clone(ws),
            ))
        })
    }) else {
        return;
    };
    host.ensure_loaded(canonical_id);
    // IDE-sync: drive the IDE/TSX surface (not the runtime `Main`) so a
    // Main-less carrier (Svelte) populates its `CachedTsx` before `get_ide`.
    let _ = host.ensure_ide_compiled(canonical_id, profile);
    let ide = host.get_ide(canonical_id, profile);
    let is_jsx = ide.as_ref().map(|ide| ide.is_jsx).unwrap_or(false);

    // Route through the SINGLE carrier-sync gateway: the membership decision
    // (publish on owned / retract on owner-loss for tsserver) is FUSED with the
    // provider-state commit. The background full-project scan previously committed a
    // tsserver carrier's state WITHOUT publishing its membership (and never retracted
    // on owner-loss) — gap E. The gateway closes that: the receipt is required to
    // commit, and only a reconcile mints it. `None` coordinator ⇒ tsgo direct-open.
    let membership =
        carrier_publish_coordinator.map(|coordinator| crate::external_ts::CarrierMembershipCtx {
            coordinator,
            provider_delivery: if is_tsgo {
                crate::external_ts::CarrierProviderDelivery::DirectOpen
            } else {
                crate::external_ts::CarrierProviderDelivery::StoreBacked
            },
        });
    let decision =
        crate::external_ts::reconcile_carrier_source(crate::external_ts::CarrierSyncRequest {
            host,
            vfs: Some(&vfs_handle),
            ownership_ready: snapshot.ownership_ready,
            resolver: &snapshot.resolver,
            provider_sync_states: sync_states,
            provider_surfaces,
            // The background scan has no `DocumentRegistry`; the carrier source resolves
            // host/VFS-only for surface recording.
            documents: None,
            canonical_id,
            is_jsx,
            ide: ide.as_ref(),
            membership,
            admission: carrier_coordinator,
            reason: crate::external_ts::ReconcileReason::SourceSynced,
        })
        .await;

    match decision {
        crate::external_ts::CarrierSyncDecision::Published {
            committed_state,
            receipt,
        } => {
            // tsserver: the plugin serves the companions as configured-project
            // members; commit the store-resident state through the receipt gate. A
            // `Superseded` commit (a newer transaction reclaimed the source, or an owner-loss
            // advanced the barrier) is re-queued for a fresh transaction — never a
            // requeue-less drop.
            if carrier_coordinator.admit_owned(sync_states, canonical_id, committed_state, &receipt)
                == crate::external_ts::AdmitOutcome::Superseded
            {
                if let Some(requeue) = requeue {
                    requeue.insert(canonical_id.to_string());
                }
            }
        }
        crate::external_ts::CarrierSyncDecision::DirectOpen {
            transition,
            pending,
        } => {
            let Some(sync) = sync else {
                tracing::error!(
                    "workspace_scanner: direct-open carrier decision has no managed provider sync"
                );
                return;
            };
            if is_tsgo {
                crate::server::configure_provider_paths_for_source(
                    sync,
                    &snapshot,
                    canonical_id,
                    true,
                )
                .await;
            }
            // Close-AFTER-successful-sync (per-kind, skip-active): capture prior state
            // + stale paths, open each kind directly, then commit (receipt-gated) and
            // close only genuinely-stale paths. A failed replacement sync must never
            // close the prior live path nor commit an unsynced path.
            let previous_state = sync_states.get(canonical_id).map(|entry| entry.clone());
            let stale_paths = transition.stale_paths;
            let mut committed_state = transition.next;
            let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();

            // Sync DTS (tsgo opens the companion buffer directly).
            if let Some(api) = host.get_public_api(canonical_id) {
                if let Some(dts_path) = committed_state.api_path.clone() {
                    let result = if is_tsgo {
                        sync.open_dts(&dts_path, &api.code).await
                    } else {
                        sync.load_dts(&dts_path, &api.code).await
                    };
                    if result.is_ok() {
                        committed_state.set_background_loaded(ProviderPathKind::Api, true);
                        synced_kinds.push(ProviderPathKind::Api);
                        // Record a fresh generation pinning the synced content + its
                        // same-content source map. The background scan has no
                        // `DocumentRegistry`; the carrier source resolves host/VFS-only.
                        crate::provider_surface_store::record_carrier_api_surface(
                            provider_surfaces,
                            None,
                            host,
                            canonical_id,
                            &dts_path,
                            &api.code,
                            api.source_map.as_deref(),
                        );
                    }
                }
            }

            // Sync IDE artifact.
            if let Some(ide) = ide {
                if let Some(tsx_path) = committed_state.ide_path.clone() {
                    let result = if is_tsgo {
                        sync.open_tsx(&tsx_path, &ide.code).await
                    } else {
                        sync.load_tsx(&tsx_path, &ide.code).await
                    };
                    if result.is_ok() {
                        committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                        synced_kinds.push(ProviderPathKind::Ide);
                        // Record a fresh generation pinning the EXACT IDE bytes just
                        // synced (interactive queries capture this surface). The
                        // background scan has no `DocumentRegistry`; the carrier
                        // source resolves host/VFS-only.
                        let provider_code = sync
                            .synced_tsx_content(&tsx_path)
                            .unwrap_or_else(|| std::sync::Arc::clone(&ide.code));
                        crate::provider_surface_store::record_carrier_ide_surface(
                            provider_surfaces,
                            None,
                            host,
                            canonical_id,
                            &tsx_path,
                            provider_code.as_ref(),
                            ide.source_map.as_deref(),
                        );
                    }
                }
            }

            if !synced_kinds.is_empty() {
                revert_unsynced_kinds(&mut committed_state, previous_state.as_ref(), &synced_kinds);
                let genuinely_stale =
                    genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
                // A kind opened: NOW mint the receipt (post-open), attesting EXACTLY the
                // kinds that actually opened this pass, and commit through the coordinator.
                let ide_surface = committed_state
                    .ide_path
                    .as_deref()
                    .and_then(|path| sync.synced_tsx_surface(path));
                let receipt = pending.confirm_opened_with_ide_surface(&synced_kinds, ide_surface);
                // Gate the stale-path close on ADMISSION and never drop the outcome: a
                // `Superseded` commit (a newer transaction reclaimed the source, or an
                // owner-loss advanced the barrier) re-queues the source and closes NOTHING —
                // the computed stale paths may be the newer transaction's LIVE buffers. Only
                // an admitted commit closes them.
                if carrier_coordinator.admit_owned(
                    sync_states,
                    canonical_id,
                    committed_state,
                    &receipt,
                ) == crate::external_ts::AdmitOutcome::Superseded
                {
                    if let Some(requeue) = requeue {
                        requeue.insert(canonical_id.to_string());
                    }
                } else {
                    close_stale_paths(
                        sync,
                        provider_surfaces,
                        &non_decl_close_targets(&genuinely_stale),
                    )
                    .await;
                }
            }
            // On total failure nothing is committed and nothing is closed: the
            // previous state + provider paths are retained intact, and the pending
            // drops unconfirmed so no receipt is minted.
        }
        crate::external_ts::CarrierSyncDecision::NotOwned(not_owned) => {
            // Settle the non-owned disposition through the coordinator: the terminal
            // `Unresolved` advances the owner-loss barrier; a transient `NotReady`/`Pending`
            // is re-queued through the shared pending set (the same interactive requeue path)
            // so the drain retries it — never a requeue-less drop. This is a BACKGROUND scan
            // (not an open editor document), so for a settled no-owner class drop any stale
            // local provider state + close its buffers. The gateway already retracted the
            // STORE/ledger membership (tsserver).
            let class = carrier_coordinator.settle(not_owned, canonical_id, requeue);
            if class.runs_buffer_cleanup() {
                // Advance-before-mutate: the coordinator advances the owner-loss barrier
                // BEFORE it vacates the slot when the removed state was a previously-committed
                // carrier, so a late owned token cannot resurrect the source.
                if let Some(state) =
                    carrier_coordinator.advance_barrier_and_remove(sync_states, canonical_id)
                {
                    // The declaration overlay (`Decl`), if any, is released by
                    // `DeclOverlayOwner` via the `did_close` lifecycle, never here.
                    if let Some(sync) = sync {
                        close_stale_paths(sync, provider_surfaces, &state.active_non_decl_paths())
                            .await;
                    }
                }
            }
        }
    }
}

async fn close_stale_paths(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    stale_paths: &[(NonDeclProviderPathKind, String)],
) {
    for (kind, path) in stale_paths {
        // EVERY closing store-backed surface (IDE / API / Shadow) is no longer
        // the active synced virtual surface — retire its active generation under
        // a fresh close EPOCH (in-flight captures stay valid; the `Closing`
        // state keeps the path failing closed until the provider close is
        // CONFIRMED). Retiring only the API role would leave a closed IDE /
        // Shadow surface `Current` — capturable by an interactive query against
        // a CLOSED provider buffer. Capture the epoch-stamped token so the
        // finalize is scoped to THIS close.
        let close_token = provider_surfaces.forget(path);
        // A declaration overlay (`Decl`) is unrepresentable here — its lifecycle is
        // owned by `DeclOverlayOwner`, never this generic close.
        let result = match kind {
            NonDeclProviderPathKind::Ide => sync.close_tsx(path).await,
            NonDeclProviderPathKind::Api => sync.close_dts(path).await,
            NonDeclProviderPathKind::Shadow => sync.close_file(path).await,
        };
        match result {
            // Only a CONFIRMED close finalizes, and only via THIS close's token —
            // a reopen (or newer close) during the await makes the epoch mismatch
            // and the finalize a no-op (the fresh snapshot survives). An error
            // drops the token, leaving the `Closing` state (fail closed).
            Ok(()) => {
                provider_surfaces.finalize_close(close_token);
            }
            Err(error) => {
                tracing::warn!(
                    "workspace_scanner: failed to close stale provider path {path}: {error}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_provider::mock::{MockCall, MockTypeProvider};
    use crate::ProjectSyncMode;
    use std::fs;
    use tempfile::TempDir;

    fn fs_workspace() -> verter_workspace::FilesystemWorkspace {
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default())
    }

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
    fn test_collect_carrier_paths() {
        let tmp = create_test_dir();
        let root = tmp.path();
        let paths = collect_carrier_paths(&fs_workspace(), root);

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
        assert!(
            paths.iter().any(|p| p.contains(".hidden/Secret.vue")),
            "should not prune dot-directories before ownership classification"
        );
        assert!(
            paths.iter().any(|p| p.contains("/dist/Built.vue")),
            "should not prune dist/ before ownership classification"
        );
        assert!(
            paths.iter().any(|p| p.contains("/build/Output.vue")),
            "should not prune build/ before ownership classification"
        );
        assert_eq!(paths.len(), 7, "should find exactly 7 .vue files");

        // Negative: does NOT include fallback-excluded directories
        assert!(
            !paths.iter().any(|p| p.contains("node_modules")),
            "must not include node_modules"
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
    fn test_collect_carrier_paths_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let paths = collect_carrier_paths(&fs_workspace(), tmp.path());
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
        let host = VerterHost::new_standalone(verter_session::HostConfig::default());
        let canonical_id = "/workspace/pkg-a/src/App.vue";
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::<str>::from("<template><div>App</div></template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_session::CompileTarget::BUNDLER | verter_session::CompileTarget::TSX,
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
        let snapshot = crate::test_utils::make_test_vfs_workspace_with_resolver_and_projects(
            resolver,
            &[
                (
                    "/workspace/pkg-a",
                    "/workspace",
                    Some("/workspace/pkg-a/tsconfig.json"),
                ),
                ("/workspace", "/workspace", None),
            ],
        );
        let sync_states = DashMap::new();
        let sync = ProjectSync::new(
            Arc::new(MockTypeProvider::new()),
            ProjectSyncMode::FullProject,
        );

        sync_file_to_provider(
            canonical_id,
            &host,
            &profile,
            Some(&sync),
            &crate::provider_surface_store::ProviderSurfaceStore::new(),
            &snapshot,
            false,
            &sync_states,
            None,
            &crate::external_ts::CarrierTransactionCoordinator::new(),
            None,
        )
        .await;

        let state = sync_states
            .get(canonical_id)
            .expect("scanner should commit a source-keyed provider state");
        assert_eq!(
            state.owner_binding,
            crate::provider_sync::ProviderOwnerBinding::Owned(
                "/workspace/pkg-a/tsconfig.json".to_string()
            ),
            "matched Vue files should have owner_binding set to the tsconfig path"
        );
    }

    #[tokio::test]
    async fn scanner_does_not_sync_carrier_without_a_configured_owner() {
        let host = VerterHost::new_standalone(verter_session::HostConfig::default());
        let canonical_id = "/workspace/scripts/Tool.vue";
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::<str>::from("<template><div>Tool</div></template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_session::CompileTarget::BUNDLER | verter_session::CompileTarget::TSX,
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
        let snapshot = crate::test_utils::make_test_vfs_workspace_with_resolver_and_projects(
            resolver,
            &[
                (
                    "/workspace/src",
                    "/workspace",
                    Some("/workspace/tsconfig.app.json"),
                ),
                ("/workspace", "/workspace", None),
            ],
        );
        let sync_states = DashMap::new();
        let sync = ProjectSync::new(
            Arc::new(MockTypeProvider::new()),
            ProjectSyncMode::FullProject,
        );

        sync_file_to_provider(
            canonical_id,
            &host,
            &profile,
            Some(&sync),
            &crate::provider_surface_store::ProviderSurfaceStore::new(),
            &snapshot,
            false,
            &sync_states,
            None,
            &crate::external_ts::CarrierTransactionCoordinator::new(),
            None,
        )
        .await;

        // A carrier under the inferred FALLBACK project (`/workspace`, no tsconfig) but
        // OUTSIDE the configured `/workspace/src` project has NO configured owner ⇒
        // `NoProject`. Per the Project-Bound External-TS Contract, an inferred/fallback
        // project never owns a carrier for external-TS, so the scan syncs NOTHING (no
        // provider state committed) — never an inferred-project overlay.
        assert!(
            sync_states.get(canonical_id).is_none(),
            "a carrier with no CONFIGURED owner must NOT be synced (NoProject), not \
             routed to the inferred fallback project"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // collect_source_paths — non-carrier file collection
    // ═══════════════════════════════════════════════════════════

    #[tokio::test]
    async fn scanner_syncs_vue_ide_artifact_for_tsgo() {
        let host = VerterHost::new_standalone(verter_session::HostConfig::default());
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
            file_language: FileLanguage::vue(),
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
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_session::CompileTarget::BUNDLER | verter_session::CompileTarget::TSX,
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
        let snapshot = crate::test_utils::make_test_vfs_workspace_with_resolver_and_projects(
            resolver,
            &[("/workspace", "/workspace", Some("/workspace/tsconfig.json"))],
        );
        let sync_states = DashMap::new();
        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);

        sync_file_to_provider(
            canonical_id,
            &host,
            &profile,
            Some(&sync),
            &crate::provider_surface_store::ProviderSurfaceStore::new(),
            &snapshot,
            true,
            &sync_states,
            None,
            &crate::external_ts::CarrierTransactionCoordinator::new(),
            None,
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
                MockCall::OpenFile { path, .. } if path == "/workspace/src/App.vue.verter.ts"
            )),
            "TSGO scanner sync should keep syncing the public API artifact (.vue.verter.ts) too, calls={calls:?}"
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
        let host = VerterHost::new_standalone(verter_session::HostConfig::default());
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: Arc::<str>::from(
                r#"<script setup lang="ts">
import Child from '@/Child.vue'
</script>
<template><div /></template>"#,
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_session::CompileTarget::BUNDLER | verter_session::CompileTarget::TSX,
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
                Some(tsconfig_path.clone()),
            ),
        ]);
        let snapshot = crate::test_utils::make_test_vfs_workspace_with_resolver_and_projects(
            resolver,
            &[(&root_path, &root_path, Some(&tsconfig_path))],
        );
        let sync_states = DashMap::new();
        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
        assert!(
            snapshot
                .read()
                .as_ref()
                .and_then(|ws| ws.load_published())
                .and_then(|published| {
                    published
                        .snapshot
                        .resolver
                        .nearest_config_for_path(&canonical_id)
                        .cloned()
                })
                .is_some(),
            "resolver should match the Vue file to the temp tsconfig owner"
        );
        let ws = verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        );
        let (expected_base_url, expected_paths) = verter_workspace::config::raw_paths_json(
            &ws,
            &root.join("tsconfig.json").to_string_lossy(),
        )
        .expect("raw_paths_json should read the temp tsconfig");

        sync_file_to_provider(
            &canonical_id,
            &host,
            &profile,
            Some(&sync),
            &crate::provider_surface_store::ProviderSurfaceStore::new(),
            &snapshot,
            true,
            &sync_states,
            None,
            &crate::external_ts::CarrierTransactionCoordinator::new(),
            None,
        )
        .await;

        let calls = provider.calls();
        let configure_index = calls
            .iter()
            .position(|call| matches!(call, MockCall::ConfigurePaths { .. }))
            .expect("TSGO scanner sync should configure owner paths from tsconfig");
        // The configured payload carries the tsconfig's own `paths` rows
        // (base_url + every expected row PRESENT), PLUS the always-injected
        // Svelte IDE-projection shim rows (`@verter/svelte-jsx/*`) — assert the
        // expected tsconfig rows survive injection (subset), not byte-equality.
        match &calls[configure_index] {
            MockCall::ConfigurePaths { base_url, paths } => {
                assert_eq!(base_url, &expected_base_url, "base_url, calls={calls:?}");
                let expected_obj = expected_paths
                    .as_object()
                    .expect("expected paths is an object");
                let actual_obj = paths.as_object().expect("actual paths is an object");
                for (k, v) in expected_obj {
                    assert_eq!(
                        actual_obj.get(k),
                        Some(v),
                        "tsconfig path row `{k}` must survive svelte injection, calls={calls:?}"
                    );
                }
                // The svelte-jsx shim rows are always injected.
                assert!(
                    actual_obj.contains_key("@verter/svelte-jsx/jsx-runtime"),
                    "svelte-jsx shim row injected, calls={calls:?}"
                );
            }
            other => panic!("expected ConfigurePaths, got {other:?}"),
        }
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
        let paths = collect_source_paths(&fs_workspace(), tmp.path());

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
            "should not include .vue files (those use collect_carrier_paths)"
        );
        // Negative: must NOT include .d.ts files
        assert!(
            !paths
                .iter()
                .any(|p| p.contains(".d.ts") || p.contains(".d.mts") || p.contains(".d.cts")),
            "should not include .d.ts/.d.mts/.d.cts files"
        );
        // Negative: must NOT include node_modules
        assert!(
            !paths.iter().any(|p| p.contains("node_modules")),
            "must not include node_modules"
        );
        assert!(
            paths.iter().any(|p| p.contains("/dist/app.js")),
            "scanner should not prune dist/ before ownership classification"
        );
        assert!(
            paths.iter().any(|p| p.contains("/build/output.js")),
            "scanner should not prune build/ before ownership classification"
        );
        assert!(
            !paths.iter().any(|p| p.contains(".git")),
            "dot-directories without matching source extensions should not appear"
        );

        // All paths use forward slashes
        for p in &paths {
            assert!(!p.contains('\\'), "paths should use forward slashes: {p}");
        }
    }

    #[test]
    fn test_collect_source_paths_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let paths = collect_source_paths(&fs_workspace(), tmp.path());
        assert!(paths.is_empty(), "empty dir should return no paths");
    }

    #[test]
    fn test_classify_tiers_works_for_non_carrier() {
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
    // is_plain_source_file / is_declaration_file
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_is_plain_source_file() {
        // Positive: standard source extensions
        assert!(is_plain_source_file("main.ts"));
        assert!(is_plain_source_file("App.tsx"));
        assert!(is_plain_source_file("utils.js"));
        assert!(is_plain_source_file("render.jsx"));
        assert!(is_plain_source_file("config.mts"));
        assert!(is_plain_source_file("config.mjs"));
        assert!(is_plain_source_file("config.cts"));
        assert!(is_plain_source_file("config.cjs"));

        // Negative: declaration files
        assert!(!is_plain_source_file("env.d.ts"), ".d.ts excluded");
        assert!(!is_plain_source_file("global.d.mts"), ".d.mts excluded");
        assert!(!is_plain_source_file("types.d.cts"), ".d.cts excluded");

        // Negative: non-source extensions
        assert!(!is_plain_source_file("App.vue"), ".vue not source");
        assert!(!is_plain_source_file("style.css"), ".css not source");
        assert!(!is_plain_source_file("readme.md"), ".md not source");
        assert!(!is_plain_source_file("data.json"), ".json not source");
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
        assert!(!is_excluded_dir("dist"));
        assert!(!is_excluded_dir("build"));
        assert!(!is_excluded_dir(".git"));
        assert!(!is_excluded_dir(".vscode"));

        assert!(!is_excluded_dir("src"), "src should not be excluded");
        assert!(!is_excluded_dir("lib"), "lib should not be excluded");
        assert!(
            !is_excluded_dir("packages"),
            "packages should not be excluded"
        );
    }

    // ── classify_from_snapshot tests ──

    #[test]
    fn classify_from_snapshot_configured_is_project_source() {
        use verter_workspace::workspace_snapshot::*;
        use verter_workspace::{
            CanonicalPath, CompiledGlob, ConfiguredMembership, FallbackMembership, NormalizedGlob,
            ProjectResolver, StaticMembershipSpec,
        };

        let root = CanonicalPath::new("d:/project");
        let spec = StaticMembershipSpec::with_typescript_defaults(&root);

        let snap = WorkspaceSnapshot {
            owners_memo: Default::default(),
            projects: vec![
                OwnershipProject {
                    id: ProjectId(0),
                    root: root.clone(),
                    workspace_root: root.clone(),
                    payload: ProjectPayload::Configured {
                        tsconfig_path: CanonicalPath::new("d:/project/tsconfig.json"),
                        membership: ConfiguredMembership {
                            spec,
                            materialized_files: Default::default(), // empty → falls back to spec
                        },
                        compiler_options: Default::default(),
                        references: vec![],
                        workspace_aliases: vec![],
                    },
                },
                OwnershipProject {
                    id: ProjectId(1),
                    root: root.clone(),
                    workspace_root: root.clone(),
                    payload: ProjectPayload::Fallback {
                        membership: FallbackMembership {
                            root: root.clone(),
                            exclude: vec![CompiledGlob::new(NormalizedGlob::new(
                                "d:/project/node_modules/**",
                            ))]
                            .into(),
                        },
                    },
                },
            ],
            resolver: ProjectResolver::default(),
            generation: SnapshotGeneration(1),
        };

        let paths = vec![
            "d:/project/src/App.vue".to_string(),
            "d:/project/scripts/build.ts".to_string(),
        ];

        let classified = classify_from_snapshot(&paths, &snap);

        // src/App.vue is under the configured project root with MatchAll → ProjectSource
        assert_eq!(classified[0].1, Tier::ProjectSource);
        // scripts/build.ts is also under root with MatchAll → ProjectSource
        assert_eq!(classified[1].1, Tier::ProjectSource);
    }

    #[test]
    fn classify_from_snapshot_outside_all_projects_is_other() {
        use verter_workspace::workspace_snapshot::*;
        use verter_workspace::ProjectResolver;

        let snap = WorkspaceSnapshot {
            owners_memo: Default::default(),
            projects: vec![],
            resolver: ProjectResolver::default(),
            generation: SnapshotGeneration(1),
        };

        let paths = vec!["d:/other/foo.vue".to_string()];
        let classified = classify_from_snapshot(&paths, &snap);
        assert_eq!(classified[0].1, Tier::Other);
    }

    #[test]
    fn classify_from_snapshot_node_modules_is_other() {
        use verter_workspace::workspace_snapshot::*;
        use verter_workspace::{
            CanonicalPath, ConfiguredMembership, ProjectResolver, StaticMembershipSpec,
        };

        let root = CanonicalPath::new("d:/project");

        let snap = WorkspaceSnapshot {
            owners_memo: Default::default(),
            projects: vec![OwnershipProject {
                id: ProjectId(0),
                root: root.clone(),
                workspace_root: root.clone(),
                payload: ProjectPayload::Configured {
                    tsconfig_path: CanonicalPath::new("d:/project/tsconfig.json"),
                    membership: ConfiguredMembership {
                        spec: StaticMembershipSpec::with_typescript_defaults(&root),
                        materialized_files: Default::default(),
                    },
                    compiler_options: Default::default(),
                    references: vec![],
                    workspace_aliases: vec![],
                },
            }],
            resolver: ProjectResolver::default(),
            generation: SnapshotGeneration(1),
        };

        let paths = vec!["d:/project/node_modules/vue/index.ts".to_string()];
        let classified = classify_from_snapshot(&paths, &snap);

        // node_modules is excluded by default TS excludes
        assert_eq!(
            classified[0].1,
            Tier::Other,
            "node_modules should be excluded by configured project defaults"
        );
    }

    #[tokio::test]
    async fn scanner_sync_retains_stale_paths_when_owner_change_sync_fails() {
        // AUDIT (workspace_scanner, invariant b): the initial-scan background
        // sync must sync the NEW paths first and close stale paths only AFTER a
        // successful sync. Pre-fix it closed `transition.stale_paths` BEFORE
        // syncing, so a failed sync left the prior live paths closed.
        let canonical_id = "/workspace/src/App.vue";
        let host = VerterHost::new_standalone(verter_session::HostConfig::default());
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::<str>::from(
                r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_session::CompileTarget::BUNDLER | verter_session::CompileTarget::TSX,
            ..CompileProfile::default()
        };
        assert!(host.ensure_compiled(canonical_id, &profile).is_ok());

        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            ),
        ]);
        let snapshot = crate::test_utils::make_test_vfs_workspace_with_resolver_and_projects(
            resolver,
            &[("/workspace", "/workspace", Some("/workspace/tsconfig.json"))],
        );

        let ide_path = format!("{canonical_id}.tsx");
        let api_path = format!("{canonical_id}.ts");
        let sync_states = DashMap::new();
        // Prior Owned state from a DIFFERENT owner → owner-change force-rebind
        // marks the same IDE/API paths stale.
        sync_states.insert(
            canonical_id.to_string(),
            ProviderSyncState {
                owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                    "/old/tsconfig.json".to_string(),
                ),
                ide_path: Some(ide_path.clone()),
                api_path: Some(api_path.clone()),
                decl_path: None,
                ide_background_loaded: true,
                api_background_loaded: true,
                decl_background_loaded: false,
                shadow_path: None,
                shadow_background_loaded: false,
                committed_ide_surface: None,
                commit_stamp: None,
            },
        );

        let provider = Arc::new(MockTypeProvider::new());
        provider.set_fail_file_ops(true);
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);

        sync_file_to_provider(
            canonical_id,
            &host,
            &profile,
            Some(&sync),
            &crate::provider_surface_store::ProviderSurfaceStore::new(),
            &snapshot,
            false,
            &sync_states,
            None,
            &crate::external_ts::CarrierTransactionCoordinator::new(),
            None,
        )
        .await;

        let calls = provider.file_sync_calls();
        // Reach (R3-2): the scanner sync must have ATTEMPTED to sync the new
        // owner's IDE `.tsx` (the failing mock records the open/update before
        // erroring) before the no-close assertion. A no-op impl that returned
        // before syncing would pass the absence-of-close assertion vacuously.
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. }
                    | MockCall::UpdateFile { path, .. }
                    | MockCall::LoadFile { path, .. }
                if path == &ide_path
            )),
            "failed scanner sync must REACH the sync and attempt the new `.tsx`, calls={calls:?}"
        );
        // Discriminator: with the new sync failing, NOTHING must be closed.
        // Pre-fix the stale-paths loop closed both paths before the failing sync.
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, MockCall::CloseFile { .. })),
            "a failed owner-change scanner sync must not close any provider path, calls={calls:?}"
        );
    }

    /// Build a published VFS workspace whose `Configured` project (tsconfig-backed)
    /// OWNS every file under `root` — the only payload the shared resolver mints a
    /// `ProjectBinding` for (a `Fallback`-only snapshot fails closed). Mirrors the
    /// production snapshot builder so the carrier-membership path exercises the same
    /// mechanism production uses.
    fn make_configured_carrier_vfs(
        root: &str,
        tsconfig: &str,
    ) -> parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>> {
        let vfs_ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        ));
        let root_cp = verter_workspace::CanonicalPath::new(root);
        let spec = verter_workspace::StaticMembershipSpec {
            files: Vec::new(),
            include: vec![verter_workspace::CompiledGlob::new(
                verter_workspace::NormalizedGlob::from_root_and_pattern(&root_cp, "**/*"),
            )],
            exclude: vec![verter_workspace::CompiledGlob::new(
                verter_workspace::NormalizedGlob::from_root_and_pattern(
                    &root_cp,
                    "node_modules/**",
                ),
            )]
            .into(),
        };
        let projects = vec![verter_workspace::workspace_snapshot::OwnershipProject {
            id: verter_workspace::workspace_snapshot::ProjectId(0),
            root: root_cp.clone(),
            workspace_root: root_cp.clone(),
            payload: verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                tsconfig_path: verter_workspace::CanonicalPath::new(tsconfig),
                membership: verter_workspace::ConfiguredMembership {
                    spec,
                    materialized_files: Default::default(),
                },
                compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                workspace_aliases: Vec::new(),
            },
        }];
        let resolver = verter_workspace::ProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                root.to_string(),
                root.to_string(),
                Some(tsconfig.to_string()),
            ),
        ]);
        let snapshot = Arc::new(verter_workspace::WorkspaceSnapshot {
            owners_memo: Default::default(),
            projects,
            resolver,
            generation: verter_workspace::workspace_snapshot::SnapshotGeneration(1),
        });
        let views = crate::workspace_state::build_lsp_views(&*vfs_ws, &snapshot, vec![]);
        vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
            snapshot,
            Box::new(views),
        ));
        parking_lot::RwLock::new(Some(vfs_ws))
    }

    /// Gap E (production path): a carrier reached ONLY by the background full-project
    /// workspace scan MUST be PUBLISHED to the on-disk store + advertised set through
    /// the membership reconciler, and on a later scan that observes OWNER LOSS it MUST
    /// be RETRACTED.
    ///
    /// DISCRIMINATION: pre-fix the scanner's `sync_file_to_provider` committed a
    /// tsserver carrier's provider state via the no-op buffer verbs WITHOUT ever
    /// publishing its store membership (and its no-owner branch `return`ed without
    /// retracting). Both assertions below catch that: with the membership step
    /// absent the carrier never enters `external_files`, and an owner-loss scan never
    /// removes it. Driving the REAL `sync_file_to_provider` with a real
    /// `CarrierPublishCoordinator` over a real `TsserverEngineBackend` exercises the
    /// production scan path.
    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_scan_publishes_then_retracts_carrier_membership_for_tsserver() {
        use crate::type_provider::traits::TypeProvider;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let ws_root = format!("/verter_scan_e_{nanos}/ws");
        let tsconfig = format!("{ws_root}/tsconfig.json");
        let source = format!("{ws_root}/src/App.vue");

        let host = VerterHost::new_standalone(verter_session::HostConfig::default());
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(source.clone()),
            input_id: source.clone(),
            source: Arc::<str>::from(
                "<script setup lang=\"ts\">\nconst msg: string = 'hi'\n</script>\n\
                 <template><div>{{ msg }}</div></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_session::CompileTarget::BUNDLER | verter_session::CompileTarget::TSX,
            ..CompileProfile::default()
        };
        assert!(host.ensure_compiled(&source, &profile).is_ok());

        // A real coordinator over a real on-disk backend (the same store the plugin
        // reads); its membership ledger + advertised set are the authority under test.
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let backend =
            Arc::new(crate::external_ts::TsserverEngineBackend::with_default_host_version());
        let coordinator = crate::external_ts::CarrierPublishCoordinator::new(
            Arc::clone(&backend),
            Arc::clone(&type_provider),
            "5.9.0",
        );

        // tsserver carrier: the scanner suppresses direct buffer opens — the publish
        // store + plugin membership IS the mechanism.
        let sync = ProjectSync::new_with_kind(
            type_provider,
            ProjectSyncMode::FullProject,
            crate::TypeProviderKind::Tsserver,
        );
        let sync_states = DashMap::new();
        let surfaces = crate::provider_surface_store::ProviderSurfaceStore::new();

        // 1. Owner-ready snapshot owning the workspace → the scan PUBLISHES.
        let owning_vfs = make_configured_carrier_vfs(&ws_root, &tsconfig);
        sync_file_to_provider(
            &source,
            &host,
            &profile,
            Some(&sync),
            &surfaces,
            &owning_vfs,
            false, // tsserver (not tsgo)
            &sync_states,
            Some(&coordinator),
            &crate::external_ts::CarrierTransactionCoordinator::new(),
            None,
        )
        .await;

        let canonical = crate::external_ts::CanonicalSource::from(source.as_str());
        assert!(
            backend.membership_ledger().is_advertised(&canonical),
            "the background scan MUST advertise the owner-resolved carrier in the \
             ledger-backed getExternalFiles (gap E: the scan previously committed \
             provider state without publishing membership)"
        );
        let advertised = backend.external_files_for_project(&tsconfig);
        assert!(
            !advertised.is_empty(),
            "the scan-published carrier MUST appear in the project's advertised set; got {advertised:?}"
        );

        // 2. Owner LOSS: a later scan over a snapshot rooted ELSEWHERE that does not
        //    own the file MUST retract the carrier.
        let other_root = format!("/verter_scan_e_other_{nanos}/ws");
        let other_vfs =
            make_configured_carrier_vfs(&other_root, &format!("{other_root}/tsconfig.json"));
        sync_file_to_provider(
            &source,
            &host,
            &profile,
            Some(&sync),
            &surfaces,
            &other_vfs,
            false,
            &sync_states,
            Some(&coordinator),
            &crate::external_ts::CarrierTransactionCoordinator::new(),
            None,
        )
        .await;

        assert!(
            !backend.membership_ledger().is_advertised(&canonical),
            "an owner-loss background scan MUST retract the carrier from the \
             ledger-backed getExternalFiles (gap E: the no-owner branch previously \
             returned without retracting)"
        );
        let advertised_after = backend.external_files_for_project(&tsconfig);
        assert!(
            advertised_after.is_empty(),
            "owner loss MUST remove the carrier from the project's advertised set; got {advertised_after:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_scan_publishes_for_editor_tsserver_without_project_sync() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let ws_root = format!("/verter_scan_editor_{nanos}/ws");
        let tsconfig = format!("{ws_root}/tsconfig.json");
        let source = format!("{ws_root}/src/App.vue");
        let host = VerterHost::new_standalone(verter_session::HostConfig::default());
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(source.clone()),
            input_id: source.clone(),
            source: Arc::<str>::from(
                "<script setup lang=\"ts\">\nconst msg: string = 'hi'\n</script>\n\
                 <template><div>{{ msg }}</div></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_session::CompileTarget::BUNDLER | verter_session::CompileTarget::TSX,
            ..CompileProfile::default()
        };
        assert!(host.ensure_compiled(&source, &profile).is_ok());

        let backend =
            Arc::new(crate::external_ts::TsserverEngineBackend::with_default_host_version());
        let coordinator = crate::external_ts::CarrierPublishCoordinator::new_editor_owned(
            Arc::clone(&backend),
            "5.9.0",
        );
        let sync_states = DashMap::new();
        let surfaces = crate::provider_surface_store::ProviderSurfaceStore::new();
        let owning_vfs = make_configured_carrier_vfs(&ws_root, &tsconfig);

        sync_file_to_provider(
            &source,
            &host,
            &profile,
            None,
            &surfaces,
            &owning_vfs,
            false,
            &sync_states,
            Some(&coordinator),
            &crate::external_ts::CarrierTransactionCoordinator::new(),
            None,
        )
        .await;

        let canonical = crate::external_ts::CanonicalSource::from(source.as_str());
        assert!(backend.membership_ledger().is_advertised(&canonical));
        assert!(!backend.external_files_for_project(&tsconfig).is_empty());
        let state = sync_states
            .get(&source)
            .expect("store-only publish must admit receipt-backed carrier state");
        assert!(state.ide_background_loaded && state.api_background_loaded);
    }

    /// The background scan's DIRECT-OPEN (tsgo) IDE sync must record the
    /// `CarrierIde` surface it delivered: a scan-synced carrier is queryable,
    /// so the interactive request-surface capture needs the recorded surface —
    /// without it every provider-backed feature drops its provider
    /// contribution for scan-synced files.
    #[tokio::test(flavor = "multi_thread")]
    async fn workspace_scan_direct_ide_sync_records_carrier_ide_surface() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let ws_root = format!("/verter_scan_ide_rec_{nanos}/ws");
        let tsconfig = format!("{ws_root}/tsconfig.json");
        let source = format!("{ws_root}/src/App.vue");

        let host = VerterHost::new_standalone(verter_session::HostConfig::default());
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(source.clone()),
            input_id: source.clone(),
            source: Arc::<str>::from(
                "<script setup lang=\"ts\">\nconst msg: string = 'hi'\n</script>\n\
                 <template><div>{{ msg }}</div></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
        let profile = CompileProfile {
            target: verter_session::CompileTarget::BUNDLER | verter_session::CompileTarget::TSX,
            ..CompileProfile::default()
        };
        assert!(host.ensure_compiled(&source, &profile).is_ok());

        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
        let sync_states = DashMap::new();
        let surfaces = crate::provider_surface_store::ProviderSurfaceStore::new();

        let owning_vfs = make_configured_carrier_vfs(&ws_root, &tsconfig);
        sync_file_to_provider(
            &source,
            &host,
            &profile,
            Some(&sync),
            &surfaces,
            &owning_vfs,
            true, // tsgo direct open
            &sync_states,
            None,
            &crate::external_ts::CarrierTransactionCoordinator::new(),
            None,
        )
        .await;

        let state = sync_states
            .get(&source)
            .map(|entry| entry.clone())
            .expect("the scan must commit provider state for the owned carrier");
        assert!(
            state.ide_background_loaded,
            "the scan's direct IDE open must mark the IDE kind live"
        );
        let ide_path = state
            .ide_path
            .clone()
            .expect("the owned carrier must commit an IDE path");
        let snapshot = surfaces
            .current_snapshot(&ide_path)
            .expect("the scan's successful direct IDE open must record a CarrierIde surface");
        assert_eq!(
            snapshot.kind,
            crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
            "the recorded surface must carry the CarrierIde role"
        );
        let ide = host
            .get_ide(&source, &profile)
            .expect("IDE output should exist");
        assert_eq!(
            snapshot.provider_content.as_ref(),
            ide.code.as_ref(),
            "the recorded surface must pin the EXACT bytes delivered to the provider"
        );
    }
}
