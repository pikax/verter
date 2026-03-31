//! Benchmark for Vue v-slot expression parsing.
//!
//! Tests parsing performance for various v-slot expression patterns.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc_allocator::Allocator;
use oxc_span::SourceType;
use serde::Deserialize;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use verter_compiler::utils::oxc::vue::parse_vslot;

/// Common v-slot expression patterns for benchmarking.
const COMMON_VSLOT_EXPRESSIONS: &[&str] = &[
    // Simple patterns
    "data",
    "item",
    "args",
    // Object destructuring
    "{ foo }",
    "{ bar }",
    "{ item, index }",
    "{ Component }",
    "{ data, loading }",
    // Renamed destructuring
    "{ rowData: role }",
    "{ value: item }",
    // With default values
    "{ item = defaultItem }",
    "{ data = [] }",
    "data = getData()",
    // Type annotations
    "{ data }: { data: MyType }",
    "data: Array<Item>",
    "{ rowData: role }: { rowData: ProjectRole }",
    // Multiple parameters
    "item, index",
    "item, index, extra",
    "first, ...rest",
    // Nested destructuring
    "{ user: { name, id } }",
    "{ config: { theme, locale } }",
    // Array destructuring
    "[first, second]",
    "[head, ...tail]",
    // Complex patterns
    "{ item = defaultItem, index = 0 }",
    "{ data, error, loading }: SlotProps",
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

/// Load v-slot expressions from expressions.json.
fn load_vslot_expressions() -> Vec<String> {
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
        .filter(|e| e.expr_type == "slot")
        .map(|e| e.expression.content)
        .filter(|c| !c.trim().is_empty())
        .collect()
}

/// Benchmark parsing common v-slot expressions.
fn bench_common_vslot(c: &mut Criterion) {
    let mut group = c.benchmark_group("vslot_common");

    for expr in COMMON_VSLOT_EXPRESSIONS {
        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(expr), expr, |b, expr| {
            b.iter(|| {
                let allocator = Allocator::default();
                let result = parse_vslot(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok())
            });
        });
    }

    group.finish();
}

/// Benchmark parsing v-slot expressions by complexity.
fn bench_vslot_by_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("vslot_complexity");

    // Simple: single identifier or basic destructuring
    let simple = vec!["data", "{ foo }", "{ item, index }"];

    // Medium: with defaults or renamed properties
    let medium = vec![
        "{ rowData: role }",
        "{ item = defaultItem }",
        "item, index, extra",
    ];

    // Complex: type annotations, nested destructuring
    let complex = vec![
        "{ data }: { data: MyType }",
        "{ user: { name, id } }",
        "{ rowData: role }: { rowData: ProjectRole }",
    ];

    // Simple expressions
    let simple_bytes: usize = simple.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(simple_bytes as u64));
    group.bench_function("simple", |b| {
        b.iter(|| {
            for expr in &simple {
                let allocator = Allocator::default();
                let result = parse_vslot(&allocator, expr, SourceType::tsx());
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
                let result = parse_vslot(&allocator, expr, SourceType::tsx());
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
                let result = parse_vslot(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    group.finish();
}

/// Benchmark parsing real-world v-slot expressions from expressions.json.
fn bench_real_vslot(c: &mut Criterion) {
    let expressions = load_vslot_expressions();
    if expressions.is_empty() {
        return;
    }

    let total_bytes: usize = expressions.iter().map(|e| e.len()).sum();

    let mut group = c.benchmark_group("vslot_real_world");
    group.throughput(Throughput::Bytes(total_bytes as u64));
    group.sample_size(50);

    group.bench_function(BenchmarkId::new("all", expressions.len()), |b| {
        b.iter(|| {
            for expr in &expressions {
                let allocator = Allocator::default();
                let result = parse_vslot(&allocator, expr, SourceType::tsx());
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
                let result = parse_vslot(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    group.finish();
}

/// Benchmark different parameter patterns.
fn bench_parameter_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("vslot_patterns");

    // Identifier patterns
    let identifiers = vec!["data", "item", "value", "result"];
    let id_bytes: usize = identifiers.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(id_bytes as u64));
    group.bench_function("identifiers", |b| {
        b.iter(|| {
            for expr in &identifiers {
                let allocator = Allocator::default();
                let result = parse_vslot(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    // Object patterns
    let objects = vec![
        "{ foo }",
        "{ foo, bar }",
        "{ foo, bar, baz }",
        "{ a, b, c, d }",
    ];
    let obj_bytes: usize = objects.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(obj_bytes as u64));
    group.bench_function("object_patterns", |b| {
        b.iter(|| {
            for expr in &objects {
                let allocator = Allocator::default();
                let result = parse_vslot(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    // With type annotations
    let typed = vec![
        "data: string",
        "{ data }: Props",
        "data: Array<Item>",
        "{ data }: { data: MyType }",
    ];
    let typed_bytes: usize = typed.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(typed_bytes as u64));
    group.bench_function("with_types", |b| {
        b.iter(|| {
            for expr in &typed {
                let allocator = Allocator::default();
                let result = parse_vslot(&allocator, expr, SourceType::tsx());
                black_box(result.is_ok());
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_common_vslot,
    bench_vslot_by_complexity,
    bench_real_vslot,
    bench_parameter_patterns,
);
criterion_main!(benches);
