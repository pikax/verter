//! Audit + cold/warm comparison runner for real component-meta requests.
//!
//! Builds a [`FilesystemWorkspace`] rooted at `<project-root>` and runs
//! `get_component_meta_with_resolution` for each Vue component on a
//! shared `VerterHost` with `audit_enabled + footprint_capture` enabled.
//!
//! Passes:
//!   * **fresh-cold** — fresh host (and fresh workspace clone) per
//!     component. Measures true cold cost in isolation.
//!   * **cold-seq** — fresh host, run all components in order; cache
//!     accumulates within the host across calls.
//!   * **warm** — re-run cold-seq's host. Should hit the
//!     `ComponentMetaResultDb` final-result cache.
//!   * **warm2** — third run; should match warm exactly.
//!
//! Per-(pass, component) JSON audit records are written to an output
//! directory and a per-pass summary CSV is also dumped.
//!
//! Usage:
//!   cargo run -p verter_bench --release --example audit_real_component_meta
//!
//! Environment variables:
//!   VERTER_AUDIT_PROJECT_ROOT  project root (default: repo-root-anchored `.integration-tests/repos/nuxt-ui`)
//!   VERTER_AUDIT_OUT_DIR       output directory (default: <OS temp dir>/verter-audit-run)
//!   VERTER_AUDIT_TARGETS       comma-separated component names. Default:
//!                              auto-discover all `*.vue` under
//!                              `<root>/src/runtime/components/`.
//!   VERTER_AUDIT_PASSES        comma list of passes to run, in order.
//!                              Choices: fresh-cold, cold-seq, warm, warm2.
//!                              Default: fresh-cold,cold-seq,warm,warm2.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use verter_session::audited_request::{AuditedRequest, AuditedRequestError};
use verter_session::component_meta_audit::{RequestAuditRecord, VfsLayer};
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{
    FilesystemOptions, FilesystemWorkspace, ProjectGraph, ViteConfigOptions, WorkspaceAccess,
};

fn normalize_lsp_style_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut normalized = if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_string()
    } else {
        normalized
    };
    if normalized.len() >= 2
        && normalized.as_bytes()[0].is_ascii_uppercase()
        && normalized.as_bytes()[1] == b':'
    {
        normalized.replace_range(0..1, &normalized[0..1].to_ascii_lowercase());
    }
    normalized
}

fn path_to_host_id(path: &Path) -> String {
    normalize_lsp_style_path(&path.to_string_lossy())
}

/// Resolve a target spec (env-var or default-list entry) to an absolute
/// canonical id. Accepts:
///   * a bare component name → `src/runtime/components/<name>.vue`,
///     falling back to a unique nested match (e.g. `LocaleSelect` →
///     `src/runtime/components/locale/LocaleSelect.vue`) when no
///     top-level file exists
///   * a relative path under the components dir → e.g. `prose/PCallout`
///     becomes `src/runtime/components/prose/PCallout.vue`
///   * a path that already begins with `src/` or `runtime/` → used as
///     project-root-relative (so `src/runtime/vue/components/Icon.vue`
///     stays put)
fn target_id_for_name(project_root: &Path, spec: &str) -> String {
    let with_ext = if spec.ends_with(".vue") {
        spec.to_string()
    } else {
        format!("{spec}.vue")
    };
    let path = if with_ext.starts_with("src/") || with_ext.starts_with("runtime/") {
        project_root.join(&with_ext)
    } else {
        // Bare name OR slug like `prose/PCallout` — both resolve under
        // the canonical components dir.
        project_root
            .join("src")
            .join("runtime")
            .join("components")
            .join(&with_ext)
    };
    // Bare-name fallback: if the nominal top-level file doesn't exist
    // but a unique file with that basename lives deeper under
    // `components/`, use it. Without this, a target like `LocaleSelect`
    // (actually at `locale/LocaleSelect.vue`) silently produces
    // `ResolutionFailed` because the host never sees the canonical.
    if !with_ext.contains('/') && !path.exists() {
        if let Some(found) = find_unique_component(project_root, &with_ext) {
            return path_to_host_id(&found);
        }
    }
    path_to_host_id(&path)
}

fn find_unique_component(project_root: &Path, basename: &str) -> Option<PathBuf> {
    let components_root = project_root.join("src").join("runtime").join("components");
    let mut found: Vec<PathBuf> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![components_root];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let ftype = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ftype.is_dir() {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if !name.starts_with('.') && name != "node_modules" {
                    stack.push(p);
                }
            } else if p.file_name().and_then(|s| s.to_str()) == Some(basename) {
                found.push(p);
                if found.len() > 1 {
                    // Ambiguous — bail to caller's nominal path.
                    return None;
                }
            }
        }
    }
    found.into_iter().next()
}

/// Walk `<project_root>/src/runtime` and return every `*.vue` file as a
/// path relative to `src/runtime/components/` when possible (so `Badge`
/// for top-level, `prose/PCallout` for nested), or relative to project
/// root otherwise (for files outside `components/`).
fn discover_all_components(project_root: &Path) -> io::Result<Vec<String>> {
    let runtime = project_root.join("src").join("runtime");
    let components_root = runtime.join("components");
    let mut names = Vec::new();
    let mut stack: Vec<PathBuf> = vec![runtime.clone()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let ftype = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ftype.is_dir() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name.starts_with('.') || name == "node_modules" {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !ftype.is_file() {
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("vue") {
                continue;
            }
            // Prefer component-relative spec (Badge, prose/PCallout);
            // otherwise fall back to project-root-relative.
            let spec = match path.strip_prefix(&components_root) {
                Ok(rel) => rel.with_extension("").to_string_lossy().replace('\\', "/"),
                Err(_) => match path.strip_prefix(project_root) {
                    Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
                    Err(_) => path.to_string_lossy().replace('\\', "/"),
                },
            };
            names.push(spec);
        }
    }
    names.sort();
    Ok(names)
}

fn build_host(project_root: &Path) -> io::Result<Arc<VerterHost>> {
    let project_root_id = path_to_host_id(project_root);
    let ws = FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![project_root_id.clone()],
        ..Default::default()
    });
    let graph_result = ProjectGraph::from_workspace_roots(
        &ws,
        std::slice::from_ref(&project_root_id),
        &ViteConfigOptions::default(),
    );
    ws.set_project_graph(graph_result.graph);
    let ws_access: Arc<dyn WorkspaceAccess> = Arc::new(ws);
    Ok(Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws_access,
    )))
}

#[allow(dead_code)] // some fields are dumped to JSON only, not the table
#[derive(Debug, Clone)]
struct PassRow {
    pass: String,
    target: String,
    elapsed_ms: f64,
    total_ms: f64,
    capture_inputs_ms: f64,
    store_read_ms: f64,
    direct_import_proof_ms: f64,
    imported_root_proof_ms: f64,
    solver_ms: f64,
    materialize_ms: f64,
    serialize_ms: f64,
    vfs_reads: usize,
    vfs_disk: usize,
    vfs_snapshot: usize,
    vfs_overlay: usize,
    vfs_missing: usize,
    indexed_ready_builds: usize,
    instantiations: usize,
    projections: usize,
    materializations: usize,
    substitutions: usize,
    alias_resolutions: usize,
    conditional_decisions: usize,
    cold_builds: u32,
    warm_hits: u32,
    joined_waits: u32,
    inflight_aborted_retries: u32,
    store_view_hits: u32,
    store_view_misses: u32,
    imported_dependency_entries: u32,
    imported_dependency_kb: u64,
    prepared_type_decls: u32,
    prepared_value_decls: u32,
    rss_delta_kb: i64,
    derivation_nodes: usize,
    derivation_edges: usize,
    edges_truncated: u32,
    has_orphan_edges: bool,
    /// Max BFS frontier depth observed by the component-meta bridge while
    /// assembling this fixture's full payload (D115). Reserved column —
    /// the BFS bridge ships in Tier 1B; pre-bridge runs record `0` and
    /// the column slot stays in `summary-179.csv`. A pre-Tier-0 dry run
    /// measured a corpus-wide max of 11 (ChatMessages family); the
    /// `MAX_BRIDGE_DEPTH = 32` constant is justified at ~3x that floor.
    bridge_max_depth_observed: u32,
    /// Count of structured events emitted by the request and surfaced
    /// on `RequestFootprintAudit::structured_events`. The
    /// audit dump always includes the full event log in the per-fixture
    /// JSON; the summary CSV reports only the count so operators can
    /// spot fixtures whose log is much larger than peers (signal of
    /// an unusually-deep materialiser walk or a regression in the
    /// trace surface).
    structured_events_count: usize,
    /// Block 7.5 diagnostic counter: number of
    /// `HostStoreView::from_host` invocations on the request. Per-request
    /// hoist (Block 6.c) expects `~1`; counts >1 reveal resolver-tier
    /// carriers that still build their own owned view.
    host_store_view_builds: u64,
    /// Block 7.5 diagnostic counter: number of bare-host
    /// `ComponentMetaQueryEngine::new(ctx)` constructions on the
    /// request where `ctx.is_request_bound() == false`. The 17 Class
    /// B bypass sites are the surviving sources; post-Block-7.5
    /// invariant: `0`.
    bare_engine_constructions: u64,
    /// Block 7.5 diagnostic counter: number of
    /// `ResolverContext::resolver_store_view()` calls on the
    /// request. Each call rebuilds an owned `HostStoreView`;
    /// warm-hit validator paths in `fact_signature_helpers` rebuild
    /// on EVERY cache lookup pre-Bug-2.
    resolver_store_view_calls: u64,
    error: Option<String>,
}

fn summarize(pass: &str, target: &str, elapsed_ms: f64, record: &RequestAuditRecord) -> PassRow {
    let fp = record.footprint.as_ref();
    let (vfs_reads, vfs_disk, vfs_snapshot, vfs_overlay, vfs_missing) = match fp {
        Some(f) => {
            let mut disk = 0;
            let mut snap = 0;
            let mut overlay = 0;
            let mut missing = 0;
            for r in &f.vfs_reads {
                match r.layer {
                    VfsLayer::Disk => disk += 1,
                    VfsLayer::Snapshot => snap += 1,
                    VfsLayer::Overlay => overlay += 1,
                    VfsLayer::Missing | VfsLayer::DirIndexNegative => missing += 1,
                }
            }
            (f.vfs_reads.len(), disk, snap, overlay, missing)
        }
        None => (0, 0, 0, 0, 0),
    };
    PassRow {
        pass: pass.to_string(),
        target: target.to_string(),
        elapsed_ms,
        total_ms: record.timings.total_ms,
        capture_inputs_ms: record.timings.capture_inputs_ms,
        store_read_ms: record.timings.store_read_ms,
        direct_import_proof_ms: record.timings.direct_import_proof_ms,
        imported_root_proof_ms: record.timings.imported_root_proof_ms,
        solver_ms: record.timings.solver_ms,
        materialize_ms: record.timings.materialize_ms,
        serialize_ms: record.timings.serialize_ms,
        vfs_reads,
        vfs_disk,
        vfs_snapshot,
        vfs_overlay,
        vfs_missing,
        indexed_ready_builds: fp.map(|f| f.indexed_ready_builds.len()).unwrap_or(0),
        instantiations: fp.map(|f| f.instantiations.len()).unwrap_or(0),
        projections: fp.map(|f| f.projections.len()).unwrap_or(0),
        materializations: fp.map(|f| f.materializations.len()).unwrap_or(0),
        substitutions: fp.map(|f| f.substitutions.len()).unwrap_or(0),
        alias_resolutions: fp.map(|f| f.alias_resolutions.len()).unwrap_or(0),
        conditional_decisions: fp.map(|f| f.conditional_decisions.len()).unwrap_or(0),
        cold_builds: fp.map(|f| f.cache_outcomes.cold_builds).unwrap_or(0),
        warm_hits: fp.map(|f| f.cache_outcomes.warm_hits).unwrap_or(0),
        joined_waits: fp.map(|f| f.cache_outcomes.joined_waits).unwrap_or(0),
        inflight_aborted_retries: fp
            .map(|f| f.cache_outcomes.inflight_aborted_retries)
            .unwrap_or(0),
        store_view_hits: record.store.store_view_hits,
        store_view_misses: record.store.store_view_misses,
        imported_dependency_entries: record.store.imported_dependency_entries,
        imported_dependency_kb: record.store.imported_dependency_bytes / 1024,
        prepared_type_decls: record.store.prepared_type_decls,
        prepared_value_decls: record.store.prepared_value_decls,
        rss_delta_kb: record.memory.process_rss_delta_bytes / 1024,
        derivation_nodes: fp.map(|f| f.derivation_subgraph.nodes.len()).unwrap_or(0),
        derivation_edges: fp.map(|f| f.derivation_subgraph.edges.len()).unwrap_or(0),
        edges_truncated: fp
            .map(|f| f.graph_completeness.edges_truncated)
            .unwrap_or(0),
        has_orphan_edges: fp
            .map(|f| f.graph_completeness.has_orphan_edges)
            .unwrap_or(false),
        // D115: column slot reserved. The BFS bridge ships in Tier 1B and
        // will write the actual frontier depth here; today's audit record
        // exposes no frontier depth so we record 0. Pre-Tier-0 dry-run
        // (manual instrumentation, not committed) measured corpus max = 11.
        bridge_max_depth_observed: 0,
        structured_events_count: fp.map(|f| f.structured_events.len()).unwrap_or(0),
        host_store_view_builds: record
            .store
            .bypass_diagnostics
            .host_store_view_from_host_builds,
        bare_engine_constructions: record.store.bypass_diagnostics.bare_engine_constructions,
        resolver_store_view_calls: record.store.bypass_diagnostics.resolver_store_view_calls,
        error: None,
    }
}

fn error_row(pass: &str, target: &str, elapsed_ms: f64, error: String) -> PassRow {
    PassRow {
        pass: pass.to_string(),
        target: target.to_string(),
        elapsed_ms,
        total_ms: 0.0,
        capture_inputs_ms: 0.0,
        store_read_ms: 0.0,
        direct_import_proof_ms: 0.0,
        imported_root_proof_ms: 0.0,
        solver_ms: 0.0,
        materialize_ms: 0.0,
        serialize_ms: 0.0,
        vfs_reads: 0,
        vfs_disk: 0,
        vfs_snapshot: 0,
        vfs_overlay: 0,
        vfs_missing: 0,
        indexed_ready_builds: 0,
        instantiations: 0,
        projections: 0,
        materializations: 0,
        substitutions: 0,
        alias_resolutions: 0,
        conditional_decisions: 0,
        cold_builds: 0,
        warm_hits: 0,
        joined_waits: 0,
        inflight_aborted_retries: 0,
        store_view_hits: 0,
        store_view_misses: 0,
        imported_dependency_entries: 0,
        imported_dependency_kb: 0,
        prepared_type_decls: 0,
        prepared_value_decls: 0,
        rss_delta_kb: 0,
        derivation_nodes: 0,
        derivation_edges: 0,
        edges_truncated: 0,
        has_orphan_edges: false,
        bridge_max_depth_observed: 0,
        structured_events_count: 0,
        host_store_view_builds: 0,
        bare_engine_constructions: 0,
        resolver_store_view_calls: 0,
        error: Some(error),
    }
}

fn dump_record(
    out_dir: &Path,
    pass: &str,
    slug: &str,
    record: &RequestAuditRecord,
) -> io::Result<()> {
    // Replace path separators in slug with `--` so nested components
    // (e.g. `prose/PCallout`) produce flat files like
    // `prose--PCallout.json` and we don't need to mkdir parents.
    let flat_slug = slug.replace(['/', '\\'], "--");
    let dir = out_dir.join(pass);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{flat_slug}.json"));
    let json = serde_json::to_string_pretty(record).map_err(io::Error::other)?;
    fs::write(&path, json)?;
    Ok(())
}

fn run_one(
    host: &Arc<VerterHost>,
    project_root: &Path,
    target: &str,
    pass: &str,
    out_dir: &Path,
) -> PassRow {
    let canonical = target_id_for_name(project_root, target);
    let started = Instant::now();
    let outcome = AuditedRequest::builder()
        .attach_to(Arc::clone(host))
        .resolve_component_meta(&canonical);
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    // Loop-5 instrumentation — capture counter values RIGHT AFTER the
    // resolve_component_meta call returns, so the snapshot reflects the
    // exact request just observed. Counters are cumulative across
    // requests; consumers can subtract to get per-request deltas.
    let loop5_counters_json =
        verter_session::loop5_instrumentation::dump_loop5_instrumentation_counters();
    let flat_slug = target.replace(['/', '\\'], "--");
    let counters_dir = out_dir.join(pass);
    if let Err(err) = fs::create_dir_all(&counters_dir) {
        eprintln!("warn: counters dir create failed for {target}: {err}");
    }
    let counters_path = counters_dir.join(format!("{flat_slug}.loop5.json"));
    if let Err(err) = fs::write(&counters_path, &loop5_counters_json) {
        eprintln!("warn: loop5 counter dump failed for {target}: {err}");
    }
    match outcome {
        Ok((_, _, record)) => {
            let row = summarize(pass, target, elapsed_ms, &record);
            if let Err(err) = dump_record(out_dir, pass, target, &record) {
                eprintln!("warn: dump_record failed for {target}: {err}");
            }
            row
        }
        Err(AuditedRequestError::ResolutionFailed) => {
            error_row(pass, target, elapsed_ms, "ResolutionFailed".into())
        }
        Err(other) => error_row(pass, target, elapsed_ms, format!("audit error: {other}")),
    }
}

fn run_pass_seq(
    host: &Arc<VerterHost>,
    project_root: &Path,
    targets: &[String],
    pass: &str,
    out_dir: &Path,
) -> Vec<PassRow> {
    let mut rows = Vec::with_capacity(targets.len());
    for (idx, target) in targets.iter().enumerate() {
        eprint!("({:>3}/{}) [{pass}] {target} ... ", idx + 1, targets.len());
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let row = run_one(host, project_root, target, pass, out_dir);
        if let Some(err) = &row.error {
            eprintln!("ERR {} ({:.0}ms)", err, row.elapsed_ms);
        } else {
            eprintln!(
                "OK {:.0}ms (vfs={} ir={} inst={} mat={} cold={}/warm={})",
                row.elapsed_ms,
                row.vfs_reads,
                row.indexed_ready_builds,
                row.instantiations,
                row.materializations,
                row.cold_builds,
                row.warm_hits,
            );
        }
        rows.push(row);
    }
    rows
}

fn run_pass_fresh_cold(
    project_root: &Path,
    targets: &[String],
    pass: &str,
    out_dir: &Path,
) -> io::Result<Vec<PassRow>> {
    let mut rows = Vec::with_capacity(targets.len());
    for (idx, target) in targets.iter().enumerate() {
        eprint!(
            "({:>3}/{}) [{pass}] {target} (fresh host) ... ",
            idx + 1,
            targets.len()
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let host = build_host(project_root)?;
        let row = run_one(&host, project_root, target, pass, out_dir);
        if let Some(err) = &row.error {
            eprintln!("ERR {} ({:.0}ms)", err, row.elapsed_ms);
        } else {
            eprintln!(
                "OK {:.0}ms (vfs={} ir={} inst={} mat={} cold={}/warm={})",
                row.elapsed_ms,
                row.vfs_reads,
                row.indexed_ready_builds,
                row.instantiations,
                row.materializations,
                row.cold_builds,
                row.warm_hits,
            );
        }
        rows.push(row);
        drop(host);
    }
    Ok(rows)
}

/// Block 6.e per-call-site attribution dump.
///
/// Snapshots [`verter_session::dump_from_host_call_sites`] (the
/// `HostStoreView::from_host` per-`#[track_caller]`-propagated site
/// counter), writes a sorted-descending table to
/// `<out_dir>/<pass>--from_host_attribution.tsv`, and prints the
/// top 30 lines to stderr so the run output surfaces the dominant
/// validator sites without scraping the file. Called at the end of
/// every pass (`fresh-cold`, `cold-seq`, `warm`, `warm2`) so each
/// pass gets its own attribution snapshot — useful for distinguishing
/// cold-build cost from warm-hit validator cost.
fn write_from_host_attribution(out_dir: &Path, pass: &str) -> io::Result<()> {
    let rows = verter_session::dump_from_host_call_sites();
    let total: u64 = rows.iter().map(|(_, c)| *c).sum();
    let path = out_dir.join(format!("{pass}--from_host_attribution.tsv"));
    let mut s = String::new();
    s.push_str("# Block 6.e per-call-site `HostStoreView::from_host` attribution\n");
    s.push_str(&format!("# pass: {pass}\n"));
    s.push_str(&format!("# total_builds: {total}\n"));
    s.push_str("# Sites listed below are `#[track_caller]`-propagated\n");
    s.push_str("# locations of the ORIGINAL caller (typically a warm-hit\n");
    s.push_str("# validator inside `fact_signature_helpers` or a cache\n");
    s.push_str("# layer reaching `resolver_store_view()`).\n");
    s.push_str("count\tpercent\tlocation\n");
    for (loc, count) in &rows {
        let pct = if total == 0 {
            0.0
        } else {
            (*count as f64) * 100.0 / (total as f64)
        };
        s.push_str(&format!("{count}\t{pct:.2}\t{loc}\n"));
    }
    fs::write(&path, &s)?;
    eprintln!(
        "from_host attribution: {} (total={})",
        path.display(),
        total
    );
    let head = rows.iter().take(30);
    for (loc, count) in head {
        let pct = if total == 0 {
            0.0
        } else {
            (*count as f64) * 100.0 / (total as f64)
        };
        eprintln!("  {count:>10}  {pct:>5.1}%  {loc}");
    }
    if rows.len() > 30 {
        eprintln!("  (... {} more sites in TSV)", rows.len() - 30);
    }
    eprintln!();
    Ok(())
}

fn write_summary_csv(out_dir: &Path, rows: &[PassRow]) -> io::Result<()> {
    let path = out_dir.join("summary.csv");
    let mut s = String::new();
    s.push_str("pass,target,error,elapsed_ms,total_ms,capture_inputs_ms,store_read_ms,direct_import_proof_ms,imported_root_proof_ms,solver_ms,materialize_ms,serialize_ms,vfs_reads,vfs_disk,vfs_snapshot,vfs_overlay,vfs_missing,indexed_ready_builds,instantiations,projections,materializations,substitutions,alias_resolutions,conditional_decisions,cold_builds,warm_hits,joined_waits,inflight_aborted_retries,store_view_hits,store_view_misses,imported_dependency_entries,imported_dependency_kb,prepared_type_decls,prepared_value_decls,rss_delta_kb,derivation_nodes,derivation_edges,edges_truncated,has_orphan_edges,bridge_max_depth_observed,structured_events_count,host_store_view_builds,bare_engine_constructions,resolver_store_view_calls\n");
    for r in rows {
        s.push_str(&format!(
            "{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            r.pass,
            r.target,
            r.error.as_deref().unwrap_or(""),
            r.elapsed_ms, r.total_ms,
            r.capture_inputs_ms, r.store_read_ms, r.direct_import_proof_ms, r.imported_root_proof_ms,
            r.solver_ms, r.materialize_ms, r.serialize_ms,
            r.vfs_reads, r.vfs_disk, r.vfs_snapshot, r.vfs_overlay, r.vfs_missing,
            r.indexed_ready_builds, r.instantiations, r.projections, r.materializations,
            r.substitutions, r.alias_resolutions, r.conditional_decisions,
            r.cold_builds, r.warm_hits, r.joined_waits, r.inflight_aborted_retries,
            r.store_view_hits, r.store_view_misses, r.imported_dependency_entries,
            r.imported_dependency_kb, r.prepared_type_decls, r.prepared_value_decls,
            r.rss_delta_kb, r.derivation_nodes, r.derivation_edges, r.edges_truncated,
            r.has_orphan_edges, r.bridge_max_depth_observed, r.structured_events_count,
            r.host_store_view_builds, r.bare_engine_constructions, r.resolver_store_view_calls,
        ));
    }
    fs::write(&path, s)?;
    eprintln!("summary csv: {}", path.display());
    Ok(())
}

fn parse_targets(project_root: &Path) -> io::Result<Vec<String>> {
    if let Ok(raw) = std::env::var("VERTER_AUDIT_TARGETS") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect());
        }
    }
    discover_all_components(project_root)
}

fn parse_passes() -> Vec<String> {
    if let Ok(raw) = std::env::var("VERTER_AUDIT_PASSES") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    vec![
        "fresh-cold".into(),
        "cold-seq".into(),
        "warm".into(),
        "warm2".into(),
    ]
}

fn default_project_root() -> PathBuf {
    // Absolute repo-root-anchored default; override with
    // VERTER_AUDIT_PROJECT_ROOT. The corpus itself is gitignored.
    // Parent-traversal (NOT textual `../..`): the downstream host-id /
    // canonicalize-path machinery does not collapse `..` segments, so a
    // literal `crates/verter_bench/../..` would split canonical identity
    // from realpath. `CARGO_MANIFEST_DIR` is always `<repo>/crates/verter_bench`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap() // -> <repo>/crates
        .parent()
        .unwrap() // -> <repo>
        .join(".integration-tests/repos/nuxt-ui")
}

fn default_out_dir() -> PathBuf {
    // Under the OS temp dir; override with VERTER_AUDIT_OUT_DIR.
    std::env::temp_dir().join("verter-audit-run")
}

fn main() -> io::Result<()> {
    // Watchdog spawn — opt-in via `VERTER_WATCHDOG_STALL_MS` (default
    // disabled). When set, spawn the verter_session watchdog thread
    // before driving the workload. The thread polls
    // `WATCHDOG_PROGRESS_BEAT` every `VERTER_WATCHDOG_INTERVAL_MS`
    // (default 1000ms) and emits a `[WATCHDOG_STALL]` line + flips
    // `WATCHDOG_DUMP_BACKTRACE_NOW` whenever the beat counter has not
    // advanced for `stall_ms` milliseconds. The next call into
    // `shallow_lower_type_expr` then dumps a self-backtrace tagged
    // `[WATCHDOG_DUMP]`. This is the in-process replacement for an
    // external sampling debugger on platforms where samply / cdb /
    // procdump are unavailable.
    // Watchdog modes:
    //   stall  — dump after `VERTER_WATCHDOG_STALL_MS` of no
    //            `watchdog_beat` advance. Use for true hangs.
    //   sample — dump every `VERTER_WATCHDOG_INTERVAL_MS`. Use for
    //            slow recursive work that advances beat rapidly but
    //            is stuck in deep recursion.
    let watchdog_mode = std::env::var("VERTER_WATCHDOG_MODE")
        .ok()
        .map(|s| s.to_lowercase());
    let watchdog_interval_ms = std::env::var("VERTER_WATCHDOG_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(1000);
    match watchdog_mode.as_deref() {
        Some("sample") => {
            eprintln!(
                "[WATCHDOG] spawn mode=sample interval_ms={}",
                watchdog_interval_ms
            );
            verter_session::loop5_instrumentation::spawn_watchdog_with_mode(
                verter_session::loop5_instrumentation::WatchdogMode::Sample,
                0,
                watchdog_interval_ms,
            );
        }
        Some("stall") | None => {
            if let Ok(stall_raw) = std::env::var("VERTER_WATCHDOG_STALL_MS") {
                if let Ok(stall_ms) = stall_raw.parse::<u64>() {
                    eprintln!(
                        "[WATCHDOG] spawn mode=stall stall_ms={} interval_ms={}",
                        stall_ms, watchdog_interval_ms
                    );
                    verter_session::loop5_instrumentation::spawn_watchdog_with_mode(
                        verter_session::loop5_instrumentation::WatchdogMode::Stall,
                        stall_ms,
                        watchdog_interval_ms,
                    );
                }
            }
        }
        Some(other) => {
            eprintln!(
                "[WATCHDOG] unknown mode '{}', expected 'stall' or 'sample'",
                other
            );
        }
    }

    let project_root = std::env::var("VERTER_AUDIT_PROJECT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_project_root());
    let out_dir = std::env::var("VERTER_AUDIT_OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_out_dir());
    let targets = parse_targets(&project_root)?;
    let passes = parse_passes();

    eprintln!("audit_real_component_meta");
    eprintln!("  project_root:   {}", project_root.display());
    eprintln!("  out_dir:        {}", out_dir.display());
    eprintln!(
        "  targets ({:>3}):  first 5: {}{}",
        targets.len(),
        targets
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
        if targets.len() > 5 { ", ..." } else { "" }
    );
    eprintln!("  passes:         {}", passes.join(", "));
    eprintln!();

    fs::create_dir_all(&out_dir)?;

    let mut all_rows: Vec<PassRow> = Vec::new();
    let mut shared_host: Option<Arc<VerterHost>> = None;

    for pass in &passes {
        // Block 6.e: reset the per-call-site counter so each pass's
        // attribution dump reflects ONLY that pass's `from_host`
        // builds, not the cumulative count across earlier passes.
        verter_session::reset_from_host_call_sites();
        match pass.as_str() {
            "fresh-cold" => {
                eprintln!(
                    "=== PASS: fresh-cold (fresh host AND fresh workspace per component) ==="
                );
                let started = Instant::now();
                let rows = run_pass_fresh_cold(&project_root, &targets, "fresh-cold", &out_dir)?;
                eprintln!("fresh-cold pass took {:?}\n", started.elapsed());
                all_rows.extend(rows);
            }
            "cold-seq" => {
                eprintln!("=== PASS: cold-seq (fresh host, components run in order, cache accumulates) ===");
                let started = Instant::now();
                let host = build_host(&project_root)?;
                let rows = run_pass_seq(&host, &project_root, &targets, "cold-seq", &out_dir);
                eprintln!("cold-seq pass took {:?}\n", started.elapsed());
                shared_host = Some(host);
                all_rows.extend(rows);
            }
            "warm" => {
                eprintln!("=== PASS: warm (re-run cold-seq host; expect cache hits) ===");
                let host = match &shared_host {
                    Some(h) => Arc::clone(h),
                    None => {
                        eprintln!("(no prior cold-seq host; building one and priming silently)");
                        let h = build_host(&project_root)?;
                        for t in &targets {
                            let _ = run_one(&h, &project_root, t, "_prime", &out_dir);
                        }
                        shared_host = Some(Arc::clone(&h));
                        h
                    }
                };
                // Reset AGAIN after prime so the per-pass dump excludes
                // the silent prime work for the no-prior-host case.
                verter_session::reset_from_host_call_sites();
                let started = Instant::now();
                let rows = run_pass_seq(&host, &project_root, &targets, "warm", &out_dir);
                eprintln!("warm pass took {:?}\n", started.elapsed());
                all_rows.extend(rows);
            }
            "warm2" => {
                eprintln!("=== PASS: warm2 (third run, must match warm) ===");
                let host = match &shared_host {
                    Some(h) => Arc::clone(h),
                    None => {
                        eprintln!("(no prior cold-seq/warm host; building and priming)");
                        let h = build_host(&project_root)?;
                        for t in &targets {
                            let _ = run_one(&h, &project_root, t, "_prime", &out_dir);
                        }
                        shared_host = Some(Arc::clone(&h));
                        h
                    }
                };
                verter_session::reset_from_host_call_sites();
                let started = Instant::now();
                let rows = run_pass_seq(&host, &project_root, &targets, "warm2", &out_dir);
                eprintln!("warm2 pass took {:?}\n", started.elapsed());
                all_rows.extend(rows);
            }
            other => {
                eprintln!("warn: unknown pass `{other}` — skipping");
                continue;
            }
        }
        // Block 6.e: dump the per-call-site attribution snapshot for
        // this pass before moving on to the next.
        if let Err(e) = write_from_host_attribution(&out_dir, pass) {
            eprintln!("warn: from_host attribution dump failed: {e}");
        }
    }

    write_summary_csv(&out_dir, &all_rows)?;
    Ok(())
}
