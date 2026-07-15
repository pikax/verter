//! Synthetic host pipeline profiler.
//!
//! Generates a deterministic set of Vue SFCs in a tempdir and runs the full
//! Verter host pipeline (upsert → compile → lint) against them.  Unlike
//! `profile_host`, this harness requires no real project checkouts — it is
//! fully self-contained and reproducible.
//!
//! Usage:
//!   cargo run -p verter_bench --example profile_host_synthetic --release --features=hotpath
//!
//! Environment variables:
//!   VERTER_SYNTHETIC_COUNT — number of SFCs to generate (default: 128)
//!   VERTER_MEMORY_MODE=1   — use MemoryWorkspace instead of FilesystemWorkspace

use std::path::Path;
use std::sync::Arc;

use verter_diagnostics::{LintConfig, Linter};
use verter_semantic::analysis::types::{AnalysisFlags, ScriptAnalysisSnapshot};
use verter_session::{
    CompileProfile, CompileTarget, FileAnalysisSnapshot, HostConfig, UpsertRequest, VerterHost,
};
use verter_workspace::{
    FilesystemOptions, FilesystemWorkspace, MemoryOptions, MemoryWorkspace, ProjectGraph,
    ViteConfigOptions,
};

fn path_to_host_id(path: &Path) -> std::io::Result<String> {
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

/// Generate a synthetic Vue project with `count` SFCs in `dir`.
fn generate_synthetic_project(count: usize, dir: &Path) {
    // node_modules/vue
    let vue_dir = dir.join("node_modules/vue/dist");
    std::fs::create_dir_all(&vue_dir).unwrap();
    std::fs::write(
        dir.join("node_modules/vue/package.json"),
        r#"{"name":"vue","module":"dist/vue.esm-bundler.js","exports":{".":{"import":"./dist/vue.esm-bundler.js"}}}"#,
    )
    .unwrap();
    std::fs::write(
        vue_dir.join("vue.esm-bundler.js"),
        "export const ref = () => {};\n",
    )
    .unwrap();

    // node_modules/pinia
    let pinia_dir = dir.join("node_modules/pinia/dist");
    std::fs::create_dir_all(&pinia_dir).unwrap();
    std::fs::write(
        dir.join("node_modules/pinia/package.json"),
        r#"{"name":"pinia","module":"dist/pinia.mjs","exports":{".":{"import":"./dist/pinia.mjs"}}}"#,
    )
    .unwrap();
    std::fs::write(
        pinia_dir.join("pinia.mjs"),
        "export const useStore = () => {};\n",
    )
    .unwrap();

    // src/utils
    let utils_dir = dir.join("src/utils");
    std::fs::create_dir_all(&utils_dir).unwrap();
    std::fs::write(
        utils_dir.join("helpers.ts"),
        "export const helper = () => {};\n",
    )
    .unwrap();
    std::fs::write(
        utils_dir.join("index.ts"),
        "export { helper } from './helpers';\n",
    )
    .unwrap();

    // src/components/Comp{0..count}.vue
    let comp_dir = dir.join("src/components");
    std::fs::create_dir_all(&comp_dir).unwrap();
    for i in 0..count {
        let sfc = format!(
            "<template><div>{{{{ msg }}}}</div></template>\n\
             <script setup lang=\"ts\">\n\
             import {{ ref }} from 'vue'\n\
             import {{ useStore }} from 'pinia'\n\
             import {{ helper }} from '../utils/helpers'\n\
             import {{ barrel }} from '../utils'\n\
             const msg = ref('hello {i}')\n\
             </script>\n"
        );
        std::fs::write(comp_dir.join(format!("Comp{i}.vue")), sfc).unwrap();
    }
}

fn run_with_filesystem_workspace(count: usize, project_root: &str, dir: &Path) {
    eprintln!("Mode: FilesystemWorkspace");
    let ws = FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![project_root.to_string()],
        ..Default::default()
    });

    let vite_opts = ViteConfigOptions::default();
    let graph_result =
        ProjectGraph::from_workspace_roots(&ws, &[project_root.to_string()], &vite_opts);
    ws.set_project_graph(graph_result.graph);

    let host = VerterHost::new(HostConfig::default(), Arc::new(ws));
    run_pipeline(&host, count, dir);
}

fn run_with_memory_workspace(count: usize, project_root: &str, dir: &Path) {
    eprintln!("Mode: MemoryWorkspace");
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    // Inject all generated files into memory
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(id) = path_to_host_id(entry.path()) {
                    ws.inject_file(id, Arc::from(content.as_str()));
                }
            }
        }
    }

    // Configure same project graph as filesystem mode
    let vite_opts = ViteConfigOptions::default();
    let graph_result =
        ProjectGraph::from_workspace_roots(&ws, &[project_root.to_string()], &vite_opts);
    ws.set_project_graph(graph_result.graph);

    let host = VerterHost::new(HostConfig::default(), Arc::new(ws));
    run_pipeline(&host, count, dir);
}

fn run_pipeline(host: &VerterHost, count: usize, dir: &Path) {
    let linter = Linter::new(LintConfig::default());

    // Collect .vue file paths and content
    let mut vue_files: Vec<(String, String)> = Vec::with_capacity(count);
    for i in 0..count {
        let path = dir.join(format!("src/components/Comp{i}.vue"));
        let id = path_to_host_id(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        vue_files.push((id, content));
    }

    // Pass 1: Upsert
    let start = std::time::Instant::now();
    for (id, content) in &vue_files {
        let req = UpsertRequest {
            canonical_id: Some(id.clone()),
            input_id: id.clone(),
            source: Arc::from(content.as_str()),
            file_language: verter_session::FileLanguage::vue(),
            aliases: Vec::new(),
        };
        let _ = host.upsert(req);
    }
    eprintln!("Upsert {} files: {:?}", count, start.elapsed());

    // Pass 2: Bundler compile
    let bundler_profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        ..CompileProfile::default()
    };
    let start = std::time::Instant::now();
    for (id, _) in &vue_files {
        let _ = host.ensure_compiled(id, &bundler_profile);
    }
    eprintln!("Bundler compile: {:?}", start.elapsed());

    // Pass 3: IDE compile
    let ide_profile = CompileProfile {
        target: CompileTarget::IDE,
        ..CompileProfile::default()
    };
    let start = std::time::Instant::now();
    for (id, _) in &vue_files {
        let _ = host.ensure_compiled(id, &ide_profile);
    }
    eprintln!("IDE compile: {:?}", start.elapsed());

    // Pass 4: Lint
    let start = std::time::Instant::now();
    let mut linted = 0;
    for (id, content) in &vue_files {
        if let Some(analysis) = host.get_analysis(id) {
            let script = script_from_host(&analysis);
            let _set = linter.lint_with_source(
                Some(&script),
                analysis.template.as_deref(),
                &analysis.styles,
                Some(content),
            );
            linted += 1;
        }
    }
    eprintln!("Lint {linted} files: {:?}", start.elapsed());

    // Summary
    eprintln!("\n── Summary ──");
    eprintln!("  Files: {count}");
    eprintln!("  Upserted: {count}");
    eprintln!("  Linted: {linted}");
}

#[cfg_attr(feature = "hotpath", hotpath::main(limit = 50))]
fn main() {
    let count: usize = std::env::var("VERTER_SYNTHETIC_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(128);
    let memory_mode = std::env::var("VERTER_MEMORY_MODE").is_ok();

    let tempdir = tempfile::tempdir().unwrap();
    generate_synthetic_project(count, tempdir.path());

    let project_root = path_to_host_id(tempdir.path()).unwrap();

    eprintln!("Synthetic host pipeline profiler");
    eprintln!("  Count: {count}");
    eprintln!("  Project root: {project_root}\n");

    if memory_mode {
        run_with_memory_workspace(count, &project_root, tempdir.path());
    } else {
        run_with_filesystem_workspace(count, &project_root, tempdir.path());
    }
}
