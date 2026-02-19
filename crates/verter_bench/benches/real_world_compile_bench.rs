use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc_allocator::Allocator;
use std::hint::black_box;
use std::path::PathBuf;

use verter_core::{
    builder::codegen::{compile, CodegenOptions},
    new_impl,
};

/// A loaded Vue file ready for benchmarking.
struct VueFile {
    filename: String,
    content: String,
}

/// All Vue files for a single project.
struct ProjectFiles {
    name: String,
    files: Vec<VueFile>,
    total_bytes: u64,
}

/// Discover the test repos root directory.
/// Checks VERTER_TEST_REPOS env var, then falls back to known paths.
fn find_test_repos_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("VERTER_TEST_REPOS") {
        let p = PathBuf::from(path);
        if p.is_dir() {
            return Some(p);
        }
    }

    let project_root_repos = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".integration-tests")
        .join("repos");
    if project_root_repos.is_dir() {
        return Some(project_root_repos);
    }

    // Known fallback paths
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

/// Load all .vue files from a project directory.
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

/// Compile all Vue files in a project (with source maps), returning error count for diagnostics.
fn compile_all_files(files: &[VueFile]) -> usize {
    let mut error_count = 0;
    for file in files {
        let allocator = Allocator::new();
        let mut options = CodegenOptions::new().with_filename(&file.filename);
        options.skip_source_map = true; // Skip source map generation for faster benchmarking
        let result = compile(&file.content, &options, &allocator);
        error_count += result.errors.len();
        black_box(&result.code);
        black_box(&result.source_map);
    }
    error_count
}
fn compile_all_files_new(files: &[VueFile]) -> usize {
    let mut error_count = 0;
    for file in files {
        let allocator = Allocator::new();

        let options = CodegenOptions::new().with_filename(&file.filename);
        let compiler_options = new_impl::compile::VerterCompileOptions {
            source_map: false,

            ..Default::default()
        };
        let result =
            new_impl::compile::compile(&file.content, &options, &compiler_options, &allocator);

        error_count += result.errors.len();
        black_box(&result.template);
    }
    error_count
}

// ============================================================================
// Benchmark: Compile all .vue files per project (throughput)
// ============================================================================

fn real_world_per_project(c: &mut Criterion) {
    let Some(root) = find_test_repos_root() else {
        eprintln!(
            "Skipping real_world_compile_bench: no test repos found. \
             Set VERTER_TEST_REPOS env var to the repos directory."
        );
        return;
    };

    // Projects ordered roughly by file count (descending) — matches integration-test/projects.mjs
    let project_dirs = [
        "primevue",
        "shadcn-vue",
        "vuetify",
        "element-plus",
        "ant-design-vue",
        "nuxt-ui",
        "balancer-frontend-v2",
        "slidev",
        "zyronon-douyin",
        "MQTTX",
        "FAIRshare",
        "requarks-wiki",
        "coreui-free-vue-admin-template",
    ];

    let mut group = c.benchmark_group("real_world/per_project");

    for &project_dir in &project_dirs {
        let Some(project) = load_project_files(&root, project_dir) else {
            eprintln!("Skipping {project_dir}: not found or empty");
            continue;
        };

        group.throughput(Throughput::Bytes(project.total_bytes));
        group.bench_with_input(
            BenchmarkId::new("compile", &project.name),
            &project,
            |b, project| {
                b.iter(|| compile_all_files(&project.files));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("compile_new", &project.name),
            &project,
            |b, project| {
                b.iter(|| compile_all_files_new(&project.files));
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark: Aggregate — all projects combined
// ============================================================================

fn real_world_aggregate(c: &mut Criterion) {
    let Some(root) = find_test_repos_root() else {
        return;
    };

    // Load all projects that exist
    let mut all_files: Vec<&VueFile> = Vec::new();
    let mut total_bytes = 0u64;

    // We need to own the data, so collect all project files first
    let project_dirs = [
        "primevue",
        "shadcn-vue",
        "vuetify",
        "element-plus",
        "ant-design-vue",
        "nuxt-ui",
        "balancer-frontend-v2",
        "slidev",
        "zyronon-douyin",
        "MQTTX",
        "FAIRshare",
        "requarks-wiki",
        "coreui-free-vue-admin-template",
    ];

    let projects: Vec<ProjectFiles> = project_dirs
        .iter()
        .filter_map(|dir| load_project_files(&root, dir))
        .collect();

    for project in &projects {
        total_bytes += project.total_bytes;
        all_files.extend(project.files.iter());
    }

    if all_files.is_empty() {
        return;
    }

    let mut group = c.benchmark_group("real_world/aggregate");
    group.throughput(Throughput::Bytes(total_bytes));
    group.sample_size(10); // Large workload — fewer samples needed

    group.bench_function(format!("compile_all/{}_files", all_files.len()), |b| {
        b.iter(|| {
            let mut error_count = 0;
            for file in &all_files {
                let allocator = Allocator::new();
                let options = CodegenOptions::new().with_filename(&file.filename);
                let result = compile(&file.content, &options, &allocator);
                error_count += result.errors.len();
                black_box(&result.code);
                black_box(&result.source_map);
            }
            black_box(error_count)
        });
    });

    group.bench_function(format!("compile_all_new/{}_files", all_files.len()), |b| {
        b.iter(|| {
            let mut error_count = 0;
            for file in &all_files {
                let allocator = Allocator::new();
                let options = CodegenOptions::new().with_filename(&file.filename);
                let compiler_options = new_impl::compile::VerterCompileOptions {
                    source_map: false,
                    ..Default::default()
                };
                let result = new_impl::compile::compile(
                    &file.content,
                    &options,
                    &compiler_options,
                    &allocator,
                );
                error_count += result.errors.len();
                black_box(&result.script);
            }
            black_box(error_count)
        });
    });

    group.finish();
}

criterion_group!(benches, real_world_per_project, real_world_aggregate);
criterion_main!(benches);
