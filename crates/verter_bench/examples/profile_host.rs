//! Full host pipeline profiler.
//!
//! Runs the full Verter host pipeline (upsert → compile → lint) on real-world
//! Vue projects from verter-test-repos, with hotpath instrumentation to identify
//! bottlenecks across all layers: VFS, scheduler, analysis, compilation, diagnostics.
//!
//! Usage:
//!   cargo run -p verter_bench --example profile_host --release --features=hotpath
//!
//! Environment variables:
//!   VERTER_TEST_REPOS   — path to the test repos directory
//!   VERTER_TRUST_VITE=1 — auto-trust all complex vite configs for full alias resolution
//!   VERTER_NODE_PATH    — explicit path to node binary (otherwise scans PATH)
//!
//! Unlike `profile_ast`, this harness requires real test repos — there is no
//! fixture fallback, since the point is exercising VFS + resolver + scheduler.

use std::path::PathBuf;
use std::sync::Arc;

use verter_analysis::types::{AnalysisFlags, ScriptAnalysisSnapshot};
use verter_diagnostics::{LintConfig, Linter};
use verter_host::{
    CompileProfile, CompileTarget, FileAnalysisSnapshot, HostConfig, UpsertRequest, VerterHost,
};
use verter_vfs::resolver::normalize_canonical_id;
use verter_vfs::{FilesystemOptions, FilesystemWorkspace, ProjectGraph, ViteConfigOptions};

struct VueFile {
    canonical_id: String,
    content: String,
}

fn find_test_repos_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("VERTER_TEST_REPOS") {
        let p = PathBuf::from(path);
        if p.is_dir() {
            return Some(p);
        }
    }
    let workspace_repos = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".integration-tests")
        .join("repos");
    if workspace_repos.is_dir() {
        return Some(workspace_repos);
    }
    None
}

/// Load `.vue` files with absolute canonical paths (not repo-relative).
fn load_project_vue_files_absolute(project_dir: &std::path::Path) -> Vec<VueFile> {
    if !project_dir.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(project_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "vue") {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(abs) = std::fs::canonicalize(path) {
                    let canonical_id = normalize_canonical_id(&abs.to_string_lossy());
                    files.push(VueFile {
                        canonical_id,
                        content,
                    });
                }
            }
        }
    }
    files
}

/// Probe for `node` on PATH.
fn find_node_on_path() -> Option<String> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let name = format!("node{ext}");
    std::env::var("PATH").ok().and_then(|path_var| {
        let sep = if cfg!(windows) { ';' } else { ':' };
        path_var.split(sep).find_map(|dir| {
            let full = std::path::Path::new(dir).join(&name);
            full.exists().then(|| full.to_string_lossy().to_string())
        })
    })
}

/// Construct a `ScriptAnalysisSnapshot` by borrowing from a host
/// `FileAnalysisSnapshot` (mirrors LSP's `script_from_host`).
fn script_from_host(analysis: &FileAnalysisSnapshot) -> ScriptAnalysisSnapshot {
    ScriptAnalysisSnapshot {
        imports: analysis.imports.clone(),
        bindings: analysis.bindings.clone(),
        macros: analysis.macros.to_vec(),
        macro_type_deps: analysis.macro_type_deps.to_vec(),
        flags: AnalysisFlags::from_bits_truncate(analysis.script_flags),
        vue_api_calls: analysis.vue_api_calls.to_vec(),
        ..Default::default()
    }
}

#[cfg_attr(feature = "hotpath", hotpath::main(limit = 50))]
fn main() {
    let Some(root) = find_test_repos_root() else {
        eprintln!("ERROR: No test repos found.");
        eprintln!("Set VERTER_TEST_REPOS env var to point to a directory containing Vue project checkouts,");
        eprintln!("or run integration tests first to populate .integration-tests/repos/.");
        eprintln!("\nFor fixture-only profiling, use `profile_ast` instead.");
        std::process::exit(1);
    };

    let projects = [
        "vuetify",
        "element-plus",
        "primevue",
        "ant-design-vue",
        "shadcn-vue",
        "slidev",
        "nuxt-ui",
        "zyronon-douyin",
    ];

    eprintln!("Host pipeline profiler");
    eprintln!("  Repos root: {}\n", root.display());

    let mut total_files = 0usize;
    let mut total_bytes = 0u64;
    let mut total_upserted = 0usize;
    let mut total_compiled_bundler = 0usize;
    let mut total_compiled_ide = 0usize;
    let mut total_linted = 0usize;

    for project in &projects {
        let project_dir = root.join(project);
        let files = load_project_vue_files_absolute(&project_dir);
        if files.is_empty() {
            eprintln!("  Skipping {project} (not found or no .vue files)");
            continue;
        }

        let project_bytes: u64 = files.iter().map(|f| f.content.len() as u64).sum();
        eprintln!(
            "  {project}: {} files, {:.1} KB",
            files.len(),
            project_bytes as f64 / 1024.0
        );
        total_files += files.len();
        total_bytes += project_bytes;

        // ── Workspace setup ──
        let abs_root = match std::fs::canonicalize(&project_dir) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("    Could not canonicalize {}: {e}", project_dir.display());
                continue;
            }
        };
        let project_root = normalize_canonical_id(&abs_root.to_string_lossy());

        let ws = FilesystemWorkspace::new(FilesystemOptions {
            roots: vec![project_root.clone()],
            ..Default::default()
        });

        // Build project graph (tsconfig + vite aliases)
        let node_path = std::env::var("VERTER_NODE_PATH")
            .ok()
            .or_else(find_node_on_path);
        let trust_vite = std::env::var("VERTER_TRUST_VITE").is_ok();

        let mut vite_opts = ViteConfigOptions {
            node_path: node_path.clone(),
            ..Default::default()
        };
        let graph_result =
            ProjectGraph::from_workspace_roots(&ws, &[project_root.clone()], &vite_opts);

        if trust_vite && !graph_result.trust_required.is_empty() && node_path.is_some() {
            vite_opts.trusted_files = graph_result
                .trust_required
                .iter()
                .map(|info| info.config_path.clone())
                .collect();
            let graph_result =
                ProjectGraph::from_workspace_roots(&ws, &[project_root.clone()], &vite_opts);
            ws.set_project_graph(graph_result.graph);
            eprintln!(
                "    Trusted {} vite configs for full alias resolution",
                vite_opts.trusted_files.len()
            );
        } else {
            if !graph_result.trust_required.is_empty() {
                eprintln!(
                    "    Note: {} vite configs need trusted execution (set VERTER_TRUST_VITE=1)",
                    graph_result.trust_required.len()
                );
            }
            ws.set_project_graph(graph_result.graph);
        }

        let host = VerterHost::new(HostConfig::default(), Arc::new(ws));
        let linter = Linter::new(LintConfig::default());

        // ── Pass 1: Upsert ──
        let mut upserted_ids = Vec::with_capacity(files.len());
        for file in &files {
            let req = UpsertRequest {
                canonical_id: Some(file.canonical_id.clone()),
                input_id: file.canonical_id.clone(),
                source: Arc::from(file.content.as_str()),
                file_kind: verter_host::FileKind::VueSfc,
                aliases: Vec::new(),
            };
            if host.upsert(req).is_ok() {
                upserted_ids.push(&file.canonical_id);
                total_upserted += 1;
            }
        }

        // ── Pass 2: Bundler compile ──
        let bundler_profile = CompileProfile {
            target: CompileTarget::BUNDLER,
            ..CompileProfile::default()
        };
        for id in &upserted_ids {
            if host.ensure_compiled(id, &bundler_profile).is_ok() {
                total_compiled_bundler += 1;
            }
        }

        // ── Pass 3: IDE compile ──
        let ide_profile = CompileProfile {
            target: CompileTarget::IDE,
            ..CompileProfile::default()
        };
        for id in &upserted_ids {
            if host.ensure_compiled(id, &ide_profile).is_ok() {
                total_compiled_ide += 1;
            }
        }

        // ── Pass 4: Lint ──
        for (file, id) in files.iter().zip(upserted_ids.iter()) {
            if let Some(analysis) = host.get_analysis(id) {
                let script = script_from_host(&analysis);
                let _set = linter.lint_with_source(
                    Some(&script),
                    analysis.template.as_deref(),
                    &analysis.styles,
                    Some(&file.content),
                );
                total_linted += 1;
            }
        }
    }

    eprintln!("\n── Summary ──");
    eprintln!(
        "  Files: {total_files} ({:.1} KB)",
        total_bytes as f64 / 1024.0
    );
    eprintln!("  Upserted:        {total_upserted}");
    eprintln!("  Compiled (BUNDLER): {total_compiled_bundler}");
    eprintln!("  Compiled (IDE):     {total_compiled_ide}");
    eprintln!("  Linted:          {total_linted}");
}
