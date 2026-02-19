//! Benchmark: Vec<NodeId> vs SmallVec<[NodeId; 4]> for element children.
//!
//! Uses real .vue files from integration-test repos to determine if SmallVec
//! is worthwhile. Three parts:
//!
//! 1. **Distribution analysis** — Parse all .vue files, print children count histogram.
//! 2. **Isolated allocation benchmark** — Replay realistic push/iterate/drop for Vec vs SmallVec.
//! 3. **Full pipeline benchmark** — End-to-end new_syntax throughput on real files.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use smallvec::SmallVec;
use std::hint::black_box;
use walkdir::WalkDir;

use verter_core::new_impl::ast::types::{AstNodeKind, TemplateAst};
use verter_core::new_impl::syntax::Syntax as NewSyntax;
use verter_core::new_impl::types::NodeId;
use verter_core::syntax::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
use verter_core::tokenizer::byte::tokenize;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const REPOS_DIR: &str = "D:/dev/github/verter-test-repos";

struct VueFile {
    /// Short label: "project/relative_path.vue"
    label: String,
    source: String,
}

/// Collect all .vue files from the integration-test repos directory.
fn collect_vue_files() -> Vec<VueFile> {
    let mut files = Vec::new();
    for entry in WalkDir::new(REPOS_DIR)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip node_modules, .git, dist, and other non-source dirs
            !matches!(
                name.as_ref(),
                "node_modules" | ".git" | "dist" | ".nuxt" | ".output" | "coverage"
            )
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "vue") {
            if let Ok(source) = std::fs::read_to_string(path) {
                // Only include files with <template> (skip script-only SFCs)
                if source.contains("<template") {
                    let rel = path
                        .strip_prefix(REPOS_DIR)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    files.push(VueFile { label: rel, source });
                }
            }
        }
    }
    files
}

/// Parse a Vue SFC through the new syntax pipeline and return the TemplateAst.
fn parse_to_ast(source: &str) -> Option<TemplateAst> {
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
    syntax.take_template_ast()
}

/// Walk a TemplateAst and collect children counts for all elements.
fn collect_children_counts(ast: &TemplateAst) -> Vec<usize> {
    let mut counts = Vec::new();
    for node in &ast.nodes {
        if let AstNodeKind::Element(el) = &node.kind {
            if let Some(content) = &el.content {
                counts.push(content.children.len());
            } else {
                counts.push(0); // self-closing
            }
        }
    }
    counts
}

// ---------------------------------------------------------------------------
// Part A: Distribution analysis (runs once as a "benchmark" to print stats)
// ---------------------------------------------------------------------------

fn bench_distribution_analysis(c: &mut Criterion) {
    let files = collect_vue_files();

    eprintln!("\n=== Children Count Distribution ===");
    eprintln!("Scanning {} .vue files from {}", files.len(), REPOS_DIR);

    let mut all_counts: Vec<usize> = Vec::new();
    let mut total_elements: usize = 0;
    let mut files_parsed: usize = 0;

    for file in &files {
        if let Some(ast) = parse_to_ast(&file.source) {
            let counts = collect_children_counts(&ast);
            total_elements += counts.len();
            all_counts.extend(counts);
            files_parsed += 1;
        }
    }

    // Build histogram
    let mut histogram = [0usize; 16]; // 0..14, 15+ bucket
    for &count in &all_counts {
        let bucket = count.min(15);
        histogram[bucket] += 1;
    }

    eprintln!("Files parsed: {}", files_parsed);
    eprintln!("Total elements: {}", total_elements);
    eprintln!();
    eprintln!("Children count | Count    | Pct    | Cumulative");
    eprintln!("---------------|----------|--------|----------");

    let mut cumulative = 0usize;
    for (i, &count) in histogram.iter().enumerate() {
        cumulative += count;
        let pct = if total_elements > 0 {
            (count as f64 / total_elements as f64) * 100.0
        } else {
            0.0
        };
        let cum_pct = if total_elements > 0 {
            (cumulative as f64 / total_elements as f64) * 100.0
        } else {
            0.0
        };
        let label = if i == 15 {
            "15+".to_string()
        } else {
            format!("{:>3}", i)
        };
        eprintln!(
            "  {}            | {:>8} | {:>5.1}% | {:>5.1}%",
            label, count, pct, cum_pct
        );
    }

    let le4 = histogram[0..=4].iter().sum::<usize>();
    let le4_pct = if total_elements > 0 {
        (le4 as f64 / total_elements as f64) * 100.0
    } else {
        0.0
    };
    eprintln!();
    eprintln!(
        "Elements with <= 4 children: {} ({:.1}%) — these would avoid heap allocation with SmallVec<[NodeId; 4]>",
        le4, le4_pct
    );
    eprintln!("=================================\n");

    // Trivial benchmark to satisfy criterion (this is really just for the stats printout)
    let mut group = c.benchmark_group("children_distribution");
    group.bench_function("parse_all_vue_files", |b| {
        b.iter(|| {
            let mut total = 0usize;
            // Sample: parse first 20 files each iteration
            for file in files.iter().take(20) {
                if let Some(ast) = parse_to_ast(&file.source) {
                    total += ast.nodes.len();
                }
            }
            black_box(total);
        });
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// Part B: Isolated Vec vs SmallVec allocation benchmark
// ---------------------------------------------------------------------------

/// Simulate the element children push/iterate/drop pattern with Vec.
#[inline(never)]
fn simulate_vec(distributions: &[usize]) -> usize {
    let mut total = 0usize;
    for &n in distributions {
        let mut children: Vec<NodeId> = Vec::new();
        for i in 0..n {
            children.push(NodeId(i));
        }
        for child in &children {
            total += child.0;
        }
        // children dropped here
    }
    total
}

/// Simulate the element children push/iterate/drop pattern with SmallVec<[NodeId; 4]>.
#[inline(never)]
fn simulate_smallvec(distributions: &[usize]) -> usize {
    let mut total = 0usize;
    for &n in distributions {
        let mut children: SmallVec<[NodeId; 4]> = SmallVec::new();
        for i in 0..n {
            children.push(NodeId(i));
        }
        for child in &children {
            total += child.0;
        }
        // children dropped here
    }
    total
}

fn bench_vec_vs_smallvec(c: &mut Criterion) {
    let files = collect_vue_files();

    // Collect real distributions from all files
    let mut all_counts: Vec<usize> = Vec::new();
    for file in &files {
        if let Some(ast) = parse_to_ast(&file.source) {
            all_counts.extend(collect_children_counts(&ast));
        }
    }

    if all_counts.is_empty() {
        eprintln!("WARNING: No elements found — skipping Vec vs SmallVec benchmark");
        return;
    }

    eprintln!(
        "\n=== Vec vs SmallVec Benchmark ({} element samples) ===\n",
        all_counts.len()
    );

    // Use chunks of different sizes to test scaling
    let mut group = c.benchmark_group("children_alloc");

    for &chunk_size in &[100, 500, 1000, 5000] {
        let chunk: Vec<usize> = all_counts
            .iter()
            .cycle()
            .take(chunk_size)
            .copied()
            .collect();

        group.throughput(Throughput::Elements(chunk_size as u64));

        group.bench_with_input(
            BenchmarkId::new("vec", chunk_size),
            &chunk,
            |b, distributions| {
                b.iter(|| black_box(simulate_vec(distributions)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("smallvec4", chunk_size),
            &chunk,
            |b, distributions| {
                b.iter(|| black_box(simulate_smallvec(distributions)));
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Part C: Full pipeline throughput on real files
// ---------------------------------------------------------------------------

fn bench_full_pipeline(c: &mut Criterion) {
    let files = collect_vue_files();

    if files.is_empty() {
        eprintln!("WARNING: No .vue files found — skipping pipeline benchmark");
        return;
    }

    // Select representative files: pick diverse sizes across projects
    let mut selected: Vec<&VueFile> = Vec::new();

    // Group by project (first path component)
    let mut by_project: std::collections::HashMap<&str, Vec<&VueFile>> =
        std::collections::HashMap::new();
    for file in &files {
        let project = file.label.split('/').next().unwrap_or("unknown");
        by_project.entry(project).or_default().push(file);
    }

    // From each project, pick a small, medium, and large file
    for (_project, mut project_files) in by_project {
        project_files.sort_by_key(|f| f.source.len());
        if let Some(small) = project_files.first() {
            selected.push(small);
        }
        if project_files.len() > 2 {
            selected.push(project_files[project_files.len() / 2]);
        }
        if let Some(large) = project_files.last() {
            if project_files.len() > 1 {
                selected.push(large);
            }
        }
    }

    let total_bytes: usize = selected.iter().map(|f| f.source.len()).sum();

    eprintln!(
        "\n=== Full Pipeline Benchmark: {} files, {} total bytes ===\n",
        selected.len(),
        total_bytes
    );

    let mut group = c.benchmark_group("children_pipeline");
    group.throughput(Throughput::Bytes(total_bytes as u64));

    group.bench_function("new_syntax_all_selected", |b| {
        b.iter(|| {
            let mut total_nodes = 0usize;
            for file in &selected {
                if let Some(ast) = parse_to_ast(&file.source) {
                    total_nodes += ast.nodes.len();
                }
            }
            black_box(total_nodes);
        });
    });

    group.finish();

    // Print type sizes for reference
    eprintln!("\n=== Type Sizes ===");
    eprintln!(
        "  NodeId:                    {} bytes",
        std::mem::size_of::<NodeId>()
    );
    eprintln!(
        "  Vec<NodeId>:               {} bytes (stack)",
        std::mem::size_of::<Vec<NodeId>>()
    );
    eprintln!(
        "  SmallVec<[NodeId; 4]>:     {} bytes (stack)",
        std::mem::size_of::<SmallVec<[NodeId; 4]>>()
    );
    eprintln!(
        "  SmallVec<[NodeId; 8]>:     {} bytes (stack)",
        std::mem::size_of::<SmallVec<[NodeId; 8]>>()
    );
    eprintln!("==================\n");
}

criterion_group!(
    benches,
    bench_distribution_analysis,
    bench_vec_vs_smallvec,
    bench_full_pipeline,
);
criterion_main!(benches);
