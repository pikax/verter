//! Compilation pipeline profiler.
//!
//! Runs the compilation pipeline on real-world Vue projects from verter-test-repos,
//! with hotpath instrumentation to identify bottlenecks.
//!
//! Two modes:
//!   - **AST-only** (default): tokenize → AST → OXC expression parsing
//!   - **Full compile** (`VERTER_PROFILE_FULL=1`): full SFC → JS/CSS pipeline
//!
//! Usage:
//!   # AST-only timing report:
//!   cargo run -p verter_bench --example profile_ast --release --features=hotpath
//!
//!   # Full compile timing report:
//!   VERTER_PROFILE_FULL=1 cargo run -p verter_bench --example profile_ast --release --features=hotpath
//!
//!   # With memory allocation tracking:
//!   cargo run -p verter_bench --example profile_ast --release --features=hotpath-alloc
//!
//! Set VERTER_TEST_REPOS env var to point to the repos directory, or it will
//! check known fallback paths. Falls back to bench fixtures if no repos found.

use std::path::PathBuf;

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use verter_core::compile::{compile, CodegenOptions, VerterCompileOptions};
use verter_core::diagnostics::{SyntaxPluginContext, SyntaxPluginOptions};
use verter_core::parser::Syntax as NewSyntax;
use verter_core::template::oxc::parse_template_expressions;
use verter_core::tokenizer::byte::tokenize;

struct VueFile {
    path: String,
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

fn load_project_vue_files(root: &std::path::Path, project: &str) -> Vec<VueFile> {
    let dir = root.join(project);
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "vue") {
            if let Ok(content) = std::fs::read_to_string(path) {
                files.push(VueFile {
                    path: path
                        .strip_prefix(root)
                        .unwrap_or(path)
                        .display()
                        .to_string(),
                    content,
                });
            }
        }
    }
    files
}

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/benches/fixtures/{}.vue",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e))
}

/// AST-only pipeline: tokenize → AST → OXC expression parsing.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn run_pipeline(source: &str) {
    let bytes = source.as_bytes();
    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes,
        options: &opts,
        diagnostics: Vec::new(),
    };

    let mut syntax = NewSyntax::new(false);
    tokenize(bytes, |e| syntax.handle(&e, &ctx));
    let ast = syntax.take_template_ast();

    if let Some(ast) = &ast {
        let alloc = Allocator::default();
        let result = parse_template_expressions(ast, source, &alloc, SourceType::tsx());
        std::hint::black_box(result);
    }

    std::hint::black_box(ast);
}

/// Full compile pipeline: tokenize → parse → style → script → template codegen.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn run_compile(source: &str) {
    let alloc = Allocator::default();
    let options = CodegenOptions::default();
    let verter_options = VerterCompileOptions::default();
    let result = compile(source, &options, &verter_options, &alloc);
    std::hint::black_box(result);
}

#[cfg_attr(feature = "hotpath", hotpath::main(limit = 40))]
fn main() {
    let full_mode = std::env::var("VERTER_PROFILE_FULL").is_ok();
    let mode_label = if full_mode {
        "full compile"
    } else {
        "AST-only"
    };
    eprintln!("Profiling mode: {mode_label}");
    eprintln!("  (set VERTER_PROFILE_FULL=1 for full compile pipeline)\n");

    if let Some(root) = find_test_repos_root() {
        // Use real-world projects
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

        let mut total_files = 0;
        let mut total_bytes = 0u64;

        for project in &projects {
            let files = load_project_vue_files(&root, project);
            if files.is_empty() {
                eprintln!("  Skipping {project} (no .vue files found)");
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

            for file in &files {
                if full_mode {
                    run_compile(&file.content);
                } else {
                    run_pipeline(&file.content);
                }
            }
        }

        eprintln!(
            "\nTotal: {} files, {:.1} KB across {} projects",
            total_files,
            total_bytes as f64 / 1024.0,
            projects.len()
        );
    } else {
        // Fallback to bench fixtures
        eprintln!("No verter-test-repos found, using bench fixtures...");
        eprintln!("Set VERTER_TEST_REPOS env var for real-world profiling.");

        let fixtures = ["template-heavy", "kitchen-sink", "simple", "large"];
        for name in &fixtures {
            let source = load_fixture(name);
            eprintln!(
                "Profiling {name} ({} bytes, {} iterations)...",
                source.len(),
                1000
            );
            for _ in 0..1000 {
                if full_mode {
                    run_compile(&source);
                } else {
                    run_pipeline(&source);
                }
            }
        }
    }
}
