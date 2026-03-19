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
use verter_vfs::{FilesystemOptions, FilesystemWorkspace, ProjectGraph, ViteConfigOptions};

struct VueFile {
    canonical_id: String,
    content: String,
}

fn path_to_host_id(path: &std::path::Path) -> std::io::Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(normalize_lsp_style_path(&absolute.to_string_lossy()))
}

fn normalize_lsp_style_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_string()
    } else {
        normalized
    }
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

/// Load `.vue` files with absolute LSP-style paths (not repo-relative).
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
                if let Ok(canonical_id) = path_to_host_id(path) {
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
        module_references: analysis.module_references.to_vec(),
        bindings: analysis.bindings.clone(),
        macros: analysis.macros.to_vec(),
        macro_type_deps: analysis.macro_type_deps.to_vec(),
        flags: AnalysisFlags::from_bits_truncate(analysis.script_flags),
        vue_api_calls: analysis.vue_api_calls.to_vec(),
        dom_query_calls: analysis.dom_query_calls.to_vec(),
        css_var_manipulations: analysis.css_var_manipulations.to_vec(),
        script_binding_occurrences: analysis.script_binding_occurrences.to_vec(),
        options_api: analysis.options_api.clone(),
        store_usages: analysis.store_usages.to_vec(),
        store_definitions: analysis.store_definitions.to_vec(),
        is_typescript: analysis.is_typescript,
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
        let project_root = match path_to_host_id(&project_dir) {
            Ok(root) => root,
            Err(e) => {
                eprintln!("    Could not absolutize {}: {e}", project_dir.display());
                continue;
            }
        };

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
        let mut upserted_files = Vec::with_capacity(files.len());
        for file in &files {
            let req = UpsertRequest {
                canonical_id: Some(file.canonical_id.clone()),
                input_id: file.canonical_id.clone(),
                source: Arc::from(file.content.as_str()),
                file_kind: verter_host::FileKind::VueSfc,
                aliases: Vec::new(),
            };
            if host.upsert(req).is_ok() {
                upserted_files.push(file);
                total_upserted += 1;
            }
        }

        // ── Pass 2: Bundler compile ──
        let bundler_profile = CompileProfile {
            target: CompileTarget::BUNDLER,
            ..CompileProfile::default()
        };
        for file in &upserted_files {
            if host
                .ensure_compiled(&file.canonical_id, &bundler_profile)
                .is_ok()
            {
                total_compiled_bundler += 1;
            }
        }

        // ── Pass 3: IDE compile ──
        let ide_profile = CompileProfile {
            target: CompileTarget::IDE,
            ..CompileProfile::default()
        };
        for file in &upserted_files {
            if host
                .ensure_compiled(&file.canonical_id, &ide_profile)
                .is_ok()
            {
                total_compiled_ide += 1;
            }
        }

        // ── Pass 4: Lint ──
        for file in &upserted_files {
            if let Some(analysis) = host.get_analysis(&file.canonical_id) {
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

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::types::{
        AnalyzedModuleReference, AnalyzedOptionsApi, CssVarManipulation, CssVarManipulationKind,
        DomQueryCallSite, DomQueryKind, ModuleReferenceAnalyzability, ModuleReferenceSemantics,
        ModuleReferenceSyntax, ScriptBindingOccurrence, ScriptUsageKind, StoreApiClassification,
        StoreDefinition, StoreUsage,
    };
    use verter_span::Span;

    #[test]
    fn script_from_host_preserves_lint_relevant_fields() {
        let analysis = FileAnalysisSnapshot {
            module_references: Arc::new(vec![AnalyzedModuleReference {
                syntax: ModuleReferenceSyntax::DynamicImport,
                semantics: ModuleReferenceSemantics::Import,
                is_type_only: false,
                span: Span::new(1, 2),
                expr_span: Span::new(2, 3),
                raw_text: "import('./dep')".to_string(),
                literal_specifier: Some("./dep".to_string()),
                finite_specifiers: Vec::new(),
                static_prefix: None,
                analyzability: ModuleReferenceAnalyzability::Exact,
            }]),
            dom_query_calls: Arc::new(vec![DomQueryCallSite {
                kind: DomQueryKind::QuerySelector,
                selector_text: ".button".to_string(),
                parsed: None,
                span: Span::new(3, 4),
                arg_span: Span::new(4, 5),
            }]),
            css_var_manipulations: Arc::new(vec![CssVarManipulation {
                kind: CssVarManipulationKind::SetProperty,
                var_name: "--accent".to_string(),
                value_expr: Some("color".to_string()),
                span: Span::new(5, 6),
            }]),
            script_binding_occurrences: Arc::new(vec![ScriptBindingOccurrence {
                name: "count".to_string(),
                span: Span::new(6, 7),
                usage_kind: ScriptUsageKind::Read,
            }]),
            options_api: Some(AnalyzedOptionsApi {
                is_define_component: true,
                object_span: Span::new(7, 8),
                ..AnalyzedOptionsApi::default()
            }),
            store_usages: Arc::new(vec![StoreUsage {
                binding_name: "userStore".to_string(),
                callee: "useUserStore".to_string(),
                import_source: "@/stores/user".to_string(),
                store_api: StoreApiClassification::StoreComposable,
                span: Span::new(8, 9),
                has_store_to_refs: false,
                destructured_props: vec!["name".to_string()],
                destructured_without_store_to_refs: true,
            }]),
            store_definitions: Arc::new(vec![StoreDefinition {
                store_id: Some("user".to_string()),
                export_name: "useUserStore".to_string(),
                store_api: StoreApiClassification::PiniaDefineStore,
                state_properties: vec!["name".to_string()],
                getters: vec!["displayName".to_string()],
                actions: vec!["rename".to_string()],
                store_dependencies: vec!["useSessionStore".to_string()],
                span: Span::new(9, 10),
                file_id: Some("/repo/stores/user.ts".to_string()),
            }]),
            is_typescript: true,
            ..FileAnalysisSnapshot::default()
        };

        let script = script_from_host(&analysis);

        assert_eq!(
            &script.module_references,
            analysis.module_references.as_ref()
        );
        assert_eq!(&script.dom_query_calls, analysis.dom_query_calls.as_ref());
        assert_eq!(
            &script.css_var_manipulations,
            analysis.css_var_manipulations.as_ref()
        );
        assert_eq!(
            &script.script_binding_occurrences,
            analysis.script_binding_occurrences.as_ref()
        );
        assert_eq!(script.options_api, analysis.options_api);
        assert_eq!(&script.store_usages, analysis.store_usages.as_ref());
        assert_eq!(
            &script.store_definitions,
            analysis.store_definitions.as_ref()
        );
        assert!(script.is_typescript);
    }

    #[cfg(windows)]
    #[test]
    fn path_to_host_id_preserves_lsp_style_drive_case_without_realpath() {
        let path = std::path::Path::new(r"C:\Users\dev\project\App.vue");
        let id =
            path_to_host_id(path).expect("path conversion should not require the file to exist");
        assert_eq!(id, "C:/Users/dev/project/App.vue");
    }
}
