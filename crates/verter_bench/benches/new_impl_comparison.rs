//! Benchmark for the AST-based compilation pipeline
//! (Syntax → generate_script → generate_template → styles).
//!
//! Source maps disabled. Styles (scoped CSS, v-bind()) are processed.
//!
//! Run with:
//!   cargo bench --bench new_impl_comparison --package verter_bench
//!   cargo bench --bench new_impl_comparison --package verter_bench -- "real_world"

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc_allocator::Allocator;
use oxc_span::SourceType;
use std::hint::black_box;
use std::path::PathBuf;

use verter_core::code_transform::CodeTransform;
use verter_core::css::process_style;
use verter_core::css::types::ProcessStyleOptions;
use verter_core::diagnostics::{SyntaxPluginContext, SyntaxPluginOptions};
use verter_core::parser::Syntax as NewSyntax;
use verter_core::script::{generate_script, ScriptCodeGenOptions};
use verter_core::style::generate_style;
use verter_core::template::code_gen::{generate_template, CodeGenMode, TemplateCodeGenOptions};
use verter_core::template::oxc::parse_template_expressions;
use verter_core::tokenizer::byte::tokenize;

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/benches/fixtures/{}.vue",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e))
}

/// AST-based pipeline: tokenize → Syntax → generate_script → generate_template → styles.
/// No source maps generated.
fn compile_full(source: &str) -> String {
    let alloc = Allocator::default();

    // Step 1: tokenize + build AST
    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &opts,
        diagnostics: Vec::new(),
    };
    let mut syntax = NewSyntax::new(false);
    tokenize(source.as_bytes(), |e| syntax.handle(&e, &ctx));

    let scope_id = "a4f2eed6";
    let has_scoped = syntax.has_style_scope();

    // Step 2: style codegen — process each <style> block
    let mut style_outputs = Vec::new();
    for style_node in syntax.style_nodes() {
        let style_result = generate_style(style_node, source, &alloc, scope_id);

        if let Some(content) = &style_node.content {
            let css_source = &source[content.start as usize..content.end as usize];
            if !style_result.out.overwrites.is_empty() {
                let mut style_ct = CodeTransform::new(source, &alloc);
                style_result.out.apply_to(&mut style_ct);
                let full = style_ct.build_string();
                let css_str = &full[content.start as usize..content.end as usize];
                let processed = process_style(
                    css_str,
                    &ProcessStyleOptions {
                        scope_id,
                        scoped: style_node.scoped,
                        is_module: style_node.module,
                        module_name: None,
                        filename: None,
                        sourcemap: false,
                    },
                );
                if let Ok(result) = processed {
                    style_outputs.push(result.code);
                }
            } else {
                let processed = process_style(
                    css_source,
                    &ProcessStyleOptions {
                        scope_id,
                        scoped: style_node.scoped,
                        is_module: style_node.module,
                        module_name: None,
                        filename: None,
                        sourcemap: false,
                    },
                );
                if let Ok(result) = processed {
                    style_outputs.push(result.code);
                }
            }
        }
    }
    black_box(&style_outputs);

    // Step 3: script codegen
    let mut ct = CodeTransform::new(source, &alloc);
    let script_opts = ScriptCodeGenOptions {
        component_name: "Anonymous",
        scope_id,
        has_scoped_style: has_scoped,
        ..Default::default()
    };
    let script_result = generate_script(
        syntax.script(),
        syntax.script_setup(),
        source,
        &mut ct,
        &alloc,
        &script_opts,
    );

    // Step 4: template OXC expression parsing + codegen
    let template_ast = syntax.take_template_ast();
    if let Some(ast) = &template_ast {
        let oxc_ast = parse_template_expressions(ast, source, &alloc, SourceType::tsx());
        generate_template(
            ast,
            &oxc_ast,
            source,
            &mut ct,
            &alloc,
            script_result.bindings,
            &TemplateCodeGenOptions::default(),
        );
    }

    ct.build_string()
}

/// Benchmark only the tokenize + AST building step (no codegen).
fn parse_only(source: &str) {
    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &opts,
        diagnostics: Vec::new(),
    };
    let mut syntax = NewSyntax::new(false);
    tokenize(source.as_bytes(), |e| syntax.handle(&e, &ctx));
    black_box(syntax.script());
    black_box(syntax.script_setup());
    black_box(syntax.template_ast());
    black_box(syntax.style_nodes());
}

/// Template-only codegen — tokenize + AST build + OXC parse + template codegen.
/// Isolates template changes from script/CSS processing.
fn template_codegen(source: &str, mode: CodeGenMode) {
    let alloc = Allocator::default();

    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &opts,
        diagnostics: Vec::new(),
    };
    let mut syntax = NewSyntax::new(false);
    tokenize(source.as_bytes(), |e| syntax.handle(&e, &ctx));

    let template_ast = syntax.take_template_ast();
    if let Some(ast) = &template_ast {
        let oxc_ast = parse_template_expressions(ast, source, &alloc, SourceType::tsx());
        let mut ct = CodeTransform::new(source, &alloc);
        generate_template(
            ast,
            &oxc_ast,
            source,
            &mut ct,
            &alloc,
            Default::default(),
            &TemplateCodeGenOptions {
                mode,
                ..Default::default()
            },
        );
        black_box(ct.build_string());
    }
}

// ============================================================================
// Fixture benchmarks
// ============================================================================

fn bench_fixture(c: &mut Criterion, fixture_name: &str) {
    let source = load_fixture(fixture_name);
    let group_name = format!("new_impl_{}", fixture_name.replace('-', "_"));
    let mut group = c.benchmark_group(&group_name);

    group.bench_function("compile", |b| {
        b.iter(|| black_box(compile_full(black_box(&source))));
    });

    group.bench_function("parse_only", |b| {
        b.iter(|| parse_only(black_box(&source)));
    });

    group.bench_function("template_vdom", |b| {
        b.iter(|| template_codegen(black_box(&source), CodeGenMode::Vdom));
    });

    group.bench_function("template_vapor", |b| {
        b.iter(|| template_codegen(black_box(&source), CodeGenMode::Vapor));
    });

    group.bench_function("template_vapor2", |b| {
        b.iter(|| template_codegen(black_box(&source), CodeGenMode::Vapor2));
    });

    group.finish();
}

fn bench_simple(c: &mut Criterion) {
    bench_fixture(c, "simple");
}

fn bench_medium(c: &mut Criterion) {
    bench_fixture(c, "medium");
}

fn bench_large(c: &mut Criterion) {
    bench_fixture(c, "large");
}

fn bench_kitchen_sink(c: &mut Criterion) {
    bench_fixture(c, "kitchen-sink");
}

fn bench_template_heavy(c: &mut Criterion) {
    bench_fixture(c, "template-heavy");
}

fn bench_composition_heavy(c: &mut Criterion) {
    bench_fixture(c, "composition-heavy");
}

// ============================================================================
// Real-world project benchmarks
// ============================================================================

struct VueFile {
    filename: String,
    content: String,
}

struct ProjectFiles {
    name: String,
    files: Vec<VueFile>,
    total_bytes: u64,
}

fn find_test_repos_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("VERTER_TEST_REPOS") {
        let p = PathBuf::from(path);
        if p.is_dir() {
            return Some(p);
        }
    }
    for candidate in &[
        "D:/dev/github/verter-test-repos",
        "../../../verter-test-repos",
    ] {
        let p = PathBuf::from(candidate);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

fn load_project_files(root: &std::path::Path, project_dir: &str) -> Option<ProjectFiles> {
    let dir = root.join(project_dir);
    if !dir.is_dir() {
        return None;
    }
    let mut files = Vec::new();
    let mut total_bytes = 0u64;
    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "vue") {
            if let Ok(content) = std::fs::read_to_string(path) {
                total_bytes += content.len() as u64;
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                files.push(VueFile { filename, content });
            }
        }
    }
    if files.is_empty() {
        return None;
    }
    Some(ProjectFiles {
        name: project_dir.to_string(),
        files,
        total_bytes,
    })
}

const PROJECT_DIRS: &[&str] = &[
    "primevue",
    "shadcn-vue",
    "vuetify",
    "element-plus",
    "ant-design-vue",
    "nuxt-ui",
    "balancer-frontend-v2",
    "slidev",
    "zyronon-douyin",
    "FAIRshare",
    "requarks-wiki",
    "coreui-free-vue-admin-template",
];

fn real_world_per_project(c: &mut Criterion) {
    let Some(root) = find_test_repos_root() else {
        eprintln!(
            "Skipping real_world benchmarks: no test repos found. \
             Set VERTER_TEST_REPOS env var to the repos directory."
        );
        return;
    };

    let mut group = c.benchmark_group("real_world/per_project");

    for &project_dir in PROJECT_DIRS {
        let Some(project) = load_project_files(&root, project_dir) else {
            eprintln!("Skipping {project_dir}: not found or empty");
            continue;
        };

        group.throughput(Throughput::Bytes(project.total_bytes));

        group.bench_with_input(
            BenchmarkId::new("compile", &project.name),
            &project,
            |b, project| {
                b.iter(|| {
                    for file in &project.files {
                        black_box(compile_full(&file.content));
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("template_vdom", &project.name),
            &project,
            |b, project| {
                b.iter(|| {
                    for file in &project.files {
                        template_codegen(&file.content, CodeGenMode::Vdom);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("template_vapor", &project.name),
            &project,
            |b, project| {
                b.iter(|| {
                    for file in &project.files {
                        template_codegen(&file.content, CodeGenMode::Vapor);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("template_vapor2", &project.name),
            &project,
            |b, project| {
                b.iter(|| {
                    for file in &project.files {
                        template_codegen(&file.content, CodeGenMode::Vapor2);
                    }
                });
            },
        );
    }

    group.finish();
}

fn real_world_aggregate(c: &mut Criterion) {
    let Some(root) = find_test_repos_root() else {
        return;
    };

    let projects: Vec<ProjectFiles> = PROJECT_DIRS
        .iter()
        .filter_map(|dir| load_project_files(&root, dir))
        .collect();

    let mut all_files: Vec<&VueFile> = Vec::new();
    let mut total_bytes = 0u64;
    for project in &projects {
        total_bytes += project.total_bytes;
        all_files.extend(project.files.iter());
    }

    if all_files.is_empty() {
        return;
    }

    let mut group = c.benchmark_group("real_world/aggregate");
    group.throughput(Throughput::Bytes(total_bytes));
    group.sample_size(10);

    group.bench_function(format!("compile/{}_files", all_files.len()), |b| {
        b.iter(|| {
            for file in &all_files {
                black_box(compile_full(&file.content));
            }
        });
    });

    group.bench_function(format!("template_vdom/{}_files", all_files.len()), |b| {
        b.iter(|| {
            for file in &all_files {
                template_codegen(&file.content, CodeGenMode::Vdom);
            }
        });
    });

    group.bench_function(format!("template_vapor/{}_files", all_files.len()), |b| {
        b.iter(|| {
            for file in &all_files {
                template_codegen(&file.content, CodeGenMode::Vapor);
            }
        });
    });

    group.bench_function(format!("template_vapor2/{}_files", all_files.len()), |b| {
        b.iter(|| {
            for file in &all_files {
                template_codegen(&file.content, CodeGenMode::Vapor2);
            }
        });
    });

    group.finish();
}

criterion_group!(
    real_world_benches,
    real_world_per_project,
    real_world_aggregate,
);
criterion_main!(real_world_benches);
