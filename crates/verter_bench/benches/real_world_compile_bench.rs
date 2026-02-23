use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc_allocator::Allocator;
use std::hint::black_box;
use std::path::PathBuf;

use verter_core::compile::{compile, CodegenOptions, VerterCompileOptions};

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

fn compile_bench(files: &[VueFile], source_map: bool) -> usize {
    let mut error_count = 0;
    for file in files {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename(&file.filename);
        let compiler_options = VerterCompileOptions {
            source_map,
            ..Default::default()
        };
        let result = compile(&file.content, &options, &compiler_options, &allocator);
        error_count += result.errors.len();
        black_box(&result.script);
        black_box(&result.template);
    }
    error_count
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

// ============================================================================
// Without source maps
// ============================================================================

fn per_project_no_sourcemap(c: &mut Criterion) {
    let Some(root) = find_test_repos_root() else {
        eprintln!(
            "Skipping real_world_compile_bench: no test repos found. \
             Set VERTER_TEST_REPOS env var to the repos directory."
        );
        return;
    };

    let mut group = c.benchmark_group("no_sourcemap/per_project");

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
                b.iter(|| compile_bench(&project.files, false));
            },
        );
    }

    group.finish();
}

fn aggregate_no_sourcemap(c: &mut Criterion) {
    let Some(root) = find_test_repos_root() else {
        return;
    };

    let projects: Vec<ProjectFiles> = PROJECT_DIRS
        .iter()
        .filter_map(|dir| load_project_files(&root, dir))
        .collect();

    let all_files: Vec<&VueFile> = projects.iter().flat_map(|p| p.files.iter()).collect();
    let total_bytes: u64 = projects.iter().map(|p| p.total_bytes).sum();

    if all_files.is_empty() {
        return;
    }

    let mut group = c.benchmark_group("no_sourcemap/aggregate");
    group.throughput(Throughput::Bytes(total_bytes));
    group.sample_size(10);

    let owned_files: Vec<&VueFile> = all_files;
    let file_count = owned_files.len();

    group.bench_function(format!("compile/{file_count}_files"), |b| {
        b.iter(|| {
            let mut errors = 0;
            for file in &owned_files {
                let allocator = Allocator::new();
                let options = CodegenOptions::new().with_filename(&file.filename);
                let compiler_options = VerterCompileOptions {
                    source_map: false,
                    ..Default::default()
                };
                let result = compile(&file.content, &options, &compiler_options, &allocator);
                errors += result.errors.len();
                black_box(&result.script);
            }
            black_box(errors)
        });
    });

    group.finish();
}

// ============================================================================
// With source maps
// ============================================================================

fn per_project_with_sourcemap(c: &mut Criterion) {
    let Some(root) = find_test_repos_root() else {
        return;
    };

    let mut group = c.benchmark_group("with_sourcemap/per_project");

    for &project_dir in PROJECT_DIRS {
        let Some(project) = load_project_files(&root, project_dir) else {
            continue;
        };

        group.throughput(Throughput::Bytes(project.total_bytes));
        group.bench_with_input(
            BenchmarkId::new("compile", &project.name),
            &project,
            |b, project| {
                b.iter(|| compile_bench(&project.files, true));
            },
        );
    }

    group.finish();
}

fn aggregate_with_sourcemap(c: &mut Criterion) {
    let Some(root) = find_test_repos_root() else {
        return;
    };

    let projects: Vec<ProjectFiles> = PROJECT_DIRS
        .iter()
        .filter_map(|dir| load_project_files(&root, dir))
        .collect();

    let all_files: Vec<&VueFile> = projects.iter().flat_map(|p| p.files.iter()).collect();
    let total_bytes: u64 = projects.iter().map(|p| p.total_bytes).sum();

    if all_files.is_empty() {
        return;
    }

    let mut group = c.benchmark_group("with_sourcemap/aggregate");
    group.throughput(Throughput::Bytes(total_bytes));
    group.sample_size(10);

    let file_count = all_files.len();

    group.bench_function(format!("compile/{file_count}_files"), |b| {
        b.iter(|| {
            let mut errors = 0;
            for file in &all_files {
                let allocator = Allocator::new();
                let options = CodegenOptions::new().with_filename(&file.filename);
                let compiler_options = VerterCompileOptions {
                    source_map: true,
                    ..Default::default()
                };
                let result = compile(&file.content, &options, &compiler_options, &allocator);
                errors += result.errors.len();
                black_box(&result.script);
                black_box(&result.template);
            }
            black_box(errors)
        });
    });

    group.finish();
}

criterion_group!(
    no_sourcemap_benches,
    per_project_no_sourcemap,
    aggregate_no_sourcemap,
);
criterion_group!(
    with_sourcemap_benches,
    per_project_with_sourcemap,
    aggregate_with_sourcemap,
);
criterion_main!(no_sourcemap_benches, with_sourcemap_benches);
