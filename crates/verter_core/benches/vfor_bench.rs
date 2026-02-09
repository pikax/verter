//! Benchmark for Vue v-for expression parsing.
//!
//! Tests parsing performance for various v-for expression patterns.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc_allocator::Allocator;
use oxc_span::SourceType;
use serde::Deserialize;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use verter_core::utils::oxc::vue::parse_vfor;

/// Common v-for expression patterns for benchmarking.
const COMMON_VFOR_EXPRESSIONS: &[&str] = &[
    // Simple patterns
    "item of items",
    "item in items",
    "todo in todos",
    // With index
    "(item, index) of items",
    "(value, key) in obj",
    "(value, key, index) in obj",
    // Destructuring
    "{ id, name } of items",
    "{ user: { name } } of items",
    "[first, second] of pairs",
    // Member expressions
    "item of data.items",
    "item of state.user.posts",
    // Function calls
    "item of getItems()",
    "n of Array(10).keys()",
    // TypeScript assertions
    "item of (items as Item[])",
    "item of (data as Array<Item>)",
    // Complex patterns
    "({ id, name }, index) of items",
    "field of availableFields",
    "(step, i) of allSteps",
    "color in colors",
    "line in data.description",
];

// Input JSON structures (matching expressions.json format)
#[derive(Debug, Deserialize)]
struct InputFile {
    #[allow(dead_code)]
    path: String,
    expressions: Vec<InputExpression>,
}

#[derive(Debug, Deserialize)]
struct InputExpression {
    #[serde(rename = "type")]
    expr_type: String,
    expression: ExpressionContent,
}

#[derive(Debug, Deserialize)]
struct ExpressionContent {
    content: String,
}

/// Load v-for expressions from expressions.json.
fn load_vfor_expressions() -> Vec<String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let input_path = Path::new(manifest_dir)
        .join("examples")
        .join("expressions")
        .join("source")
        .join("expressions.json");

    let content = fs::read_to_string(&input_path).expect("Failed to read expressions.json");
    let files: Vec<InputFile> = serde_json::from_str(&content).expect("Failed to parse JSON");

    files
        .into_iter()
        .flat_map(|f| f.expressions)
        .filter(|e| e.expr_type == "for")
        .map(|e| e.expression.content)
        .filter(|c| !c.trim().is_empty())
        .collect()
}

/// Benchmark parsing common v-for expressions.
fn bench_common_vfor(c: &mut Criterion) {
    let mut group = c.benchmark_group("vfor_common");

    for expr in COMMON_VFOR_EXPRESSIONS {
        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(expr), expr, |b, expr| {
            b.iter(|| {
                let allocator = Allocator::default();
                let result = parse_vfor(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok())
            });
        });
    }

    group.finish();
}

/// Benchmark parsing v-for expressions by complexity.
fn bench_vfor_by_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("vfor_complexity");

    // Simple: single identifier
    let simple = vec!["item of items", "todo in todos", "n in numbers"];

    // Medium: with index or destructuring
    let medium = vec![
        "(item, index) of items",
        "{ id, name } of users",
        "[a, b] of pairs",
    ];

    // Complex: member expressions, function calls, TypeScript
    let complex = vec![
        "item of state.user.posts.filter(p => p.visible)",
        "({ id, name }, index) of items",
        "item of (data as Array<Item>)",
    ];

    // Simple expressions
    let simple_bytes: usize = simple.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(simple_bytes as u64));
    group.bench_function("simple", |b| {
        b.iter(|| {
            for expr in &simple {
                let allocator = Allocator::default();
                let result = parse_vfor(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    // Medium expressions
    let medium_bytes: usize = medium.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(medium_bytes as u64));
    group.bench_function("medium", |b| {
        b.iter(|| {
            for expr in &medium {
                let allocator = Allocator::default();
                let result = parse_vfor(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    // Complex expressions
    let complex_bytes: usize = complex.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(complex_bytes as u64));
    group.bench_function("complex", |b| {
        b.iter(|| {
            for expr in &complex {
                let allocator = Allocator::default();
                let result = parse_vfor(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    group.finish();
}

/// Benchmark parsing real-world v-for expressions from expressions.json.
fn bench_real_vfor(c: &mut Criterion) {
    let expressions = load_vfor_expressions();
    if expressions.is_empty() {
        return;
    }

    let total_bytes: usize = expressions.iter().map(|e| e.len()).sum();

    let mut group = c.benchmark_group("vfor_real_world");
    group.throughput(Throughput::Bytes(total_bytes as u64));
    group.sample_size(50);

    group.bench_function(BenchmarkId::new("all", expressions.len()), |b| {
        b.iter(|| {
            for expr in &expressions {
                let allocator = Allocator::default();
                let result = parse_vfor(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    // Per-expression throughput
    group.throughput(Throughput::Elements(expressions.len() as u64));
    group.bench_function("per_expression", |b| {
        b.iter(|| {
            for expr in &expressions {
                let allocator = Allocator::default();
                let result = parse_vfor(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    group.finish();
}

/// Benchmark separator detection (in vs of).
fn bench_separator_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("vfor_separator");

    let of_expressions = vec!["item of items", "(item, index) of items", "{ id } of items"];

    let in_expressions = vec!["item in items", "(item, index) in items", "{ id } in items"];

    // "of" separator
    let of_bytes: usize = of_expressions.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(of_bytes as u64));
    group.bench_function("of_separator", |b| {
        b.iter(|| {
            for expr in &of_expressions {
                let allocator = Allocator::default();
                let result = parse_vfor(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    // "in" separator
    let in_bytes: usize = in_expressions.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(in_bytes as u64));
    group.bench_function("in_separator", |b| {
        b.iter(|| {
            for expr in &in_expressions {
                let allocator = Allocator::default();
                let result = parse_vfor(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_common_vfor,
    bench_vfor_by_complexity,
    bench_real_vfor,
    bench_separator_detection,
);
criterion_main!(benches);
