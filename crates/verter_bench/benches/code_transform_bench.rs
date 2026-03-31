use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc_allocator::Allocator;
use std::hint::black_box;

use verter_compiler::code_transform::{CodeTransform, SourceMapOptions};

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/benches/fixtures/{}.vue",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e))
}

// ============================================================================
// Group 1: Basic operations on medium.vue (~2.6KB)
// ============================================================================

fn basic_operations(c: &mut Criterion) {
    let source = load_fixture("medium");
    let len = source.len() as u32;
    let mid = len / 2;
    let mut group = c.benchmark_group("code_transform/basic");

    group.bench_function("overwrite", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut ct = CodeTransform::new(black_box(&source), &allocator);
            ct.overwrite(mid, mid + 100, "replacement_content");
            black_box(ct.build_string());
        });
    });

    group.bench_function("remove", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut ct = CodeTransform::new(black_box(&source), &allocator);
            ct.remove(mid, mid + 100);
            black_box(ct.build_string());
        });
    });

    group.bench_function("prepend_left", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut ct = CodeTransform::new(black_box(&source), &allocator);
            ct.prepend_left(mid, "inserted_content");
            black_box(ct.build_string());
        });
    });

    group.bench_function("append_right", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut ct = CodeTransform::new(black_box(&source), &allocator);
            ct.append_right(mid, "inserted_content");
            black_box(ct.build_string());
        });
    });

    group.bench_function("move_wrapped", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut ct = CodeTransform::new(black_box(&source), &allocator);
            ct.move_wrapped(mid, mid + 100, 0, "prefix:", ",\n");
            black_box(ct.build_string());
        });
    });

    group.bench_function("prepend_append", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut ct = CodeTransform::new(black_box(&source), &allocator);
            ct.prepend("// header\n");
            ct.append("\n// footer");
            black_box(ct.build_string());
        });
    });

    group.bench_function("build_string_only", |b| {
        // Pre-edit outside the iteration loop to isolate build_string cost
        let allocator = Allocator::new();
        let mut ct = CodeTransform::new(&source, &allocator);
        ct.overwrite(100, 200, "replaced");
        ct.prepend_left(300, "inserted");
        ct.prepend("// header\n");
        ct.append("\n// footer");

        b.iter(|| {
            black_box(ct.build_string());
        });
    });

    group.finish();
}

// ============================================================================
// Group 2: Batch vs sequential operations
// ============================================================================

fn batch_vs_sequential(c: &mut Criterion) {
    let source = load_fixture("kitchen-sink");
    let len = source.len() as u32;
    let mut group = c.benchmark_group("code_transform/batch_vs_sequential");

    for &n in &[10u32, 50, 200] {
        let step = len / (n + 1);

        // --- Overwrite: sequential vs batch ---

        group.bench_with_input(BenchmarkId::new("sequential_overwrite", n), &n, |b, &n| {
            b.iter(|| {
                let allocator = Allocator::new();
                let mut ct = CodeTransform::new(black_box(&source), &allocator);
                for i in 0..n {
                    let start = step * (i + 1);
                    let end = (start + 5).min(len);
                    ct.overwrite(start, end, "X");
                }
                black_box(ct.build_string());
            });
        });

        // Pre-compute overwrite items for batch
        let overwrite_items: Vec<(u32, u32, &str)> = (0..n)
            .map(|i| {
                let start = step * (i + 1);
                let end = (start + 5).min(len);
                (start, end, "X")
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("batch_overwrite", n), &n, |b, _| {
            b.iter(|| {
                let allocator = Allocator::new();
                let mut ct = CodeTransform::new(black_box(&source), &allocator);
                ct.batch_overwrite(black_box(&overwrite_items));
                black_box(ct.build_string());
            });
        });

        // --- Prepend left: sequential vs batch ---

        group.bench_with_input(
            BenchmarkId::new("sequential_prepend_left", n),
            &n,
            |b, &n| {
                b.iter(|| {
                    let allocator = Allocator::new();
                    let mut ct = CodeTransform::new(black_box(&source), &allocator);
                    for i in 0..n {
                        let pos = step * (i + 1);
                        ct.prepend_left(pos, "_ctx.");
                    }
                    black_box(ct.build_string());
                });
            },
        );

        let prepend_items: Vec<(u32, &str)> = (0..n)
            .map(|i| {
                let pos = step * (i + 1);
                (pos, "_ctx.")
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("batch_prepend_left_static", n),
            &n,
            |b, _| {
                b.iter(|| {
                    let allocator = Allocator::new();
                    let mut ct = CodeTransform::new(black_box(&source), &allocator);
                    ct.batch_prepend_left_static(black_box(&prepend_items));
                    black_box(ct.build_string());
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Group 3: Move operations scaling
// ============================================================================

fn move_operations(c: &mut Criterion) {
    let source = load_fixture("kitchen-sink");
    let len = source.len() as u32;
    let mut group = c.benchmark_group("code_transform/moves");

    for &n in &[1u32, 5, 20] {
        let step = len / (n + 2); // Leave room at boundaries

        group.bench_with_input(BenchmarkId::new("move_wrapped", n), &n, |b, &n| {
            b.iter(|| {
                let allocator = Allocator::new();
                let mut ct = CodeTransform::new(black_box(&source), &allocator);
                for i in 0..n {
                    let start = step * (i + 1);
                    let end = (start + 20).min(len - 1);
                    ct.move_wrapped(start, end, 0, "/*moved*/", ",");
                }
                black_box(ct.build_string());
            });
        });
    }

    group.finish();
}

// ============================================================================
// Group 4: Source map generation cost
// ============================================================================

fn source_map_generation(c: &mut Criterion) {
    let source = load_fixture("kitchen-sink");
    let len = source.len() as u32;
    let mut group = c.benchmark_group("code_transform/source_map");

    group.bench_function("generate_map/unmodified", |b| {
        let allocator = Allocator::new();
        let ct = CodeTransform::new(&source, &allocator);
        b.iter(|| {
            let options = SourceMapOptions::new().with_source("test.vue");
            black_box(ct.generate_map(options));
        });
    });

    for &n in &[10u32, 100] {
        let step = len / (n + 1);

        group.bench_function(format!("generate_map/{n}_edits"), |b| {
            let allocator = Allocator::new();
            let mut ct = CodeTransform::new(&source, &allocator);
            for i in 0..n {
                let start = step * (i + 1);
                let end = (start + 5).min(len);
                ct.overwrite(start, end, "X");
            }
            b.iter(|| {
                let options = SourceMapOptions::new().with_source("test.vue");
                black_box(ct.generate_map(options));
            });
        });
    }

    group.finish();
}

// ============================================================================
// Group 5: Chunk iteration throughput (isolates cache effects of enum size)
// ============================================================================

fn chunk_iteration(c: &mut Criterion) {
    let source = "x".repeat(10_000);
    let len = source.len() as u32;
    let mut group = c.benchmark_group("code_transform/chunk_iteration");

    for &num_edits in &[100u32, 500, 2000] {
        let step = len / (num_edits + 1);

        // Pre-build a CodeTransform with many chunks, then measure build_string only
        group.bench_with_input(
            BenchmarkId::new("build_string", num_edits),
            &num_edits,
            |b, &n| {
                let allocator = Allocator::new();
                let mut ct = CodeTransform::new(&source, &allocator);
                for i in 1..=n {
                    let pos = step * i;
                    let end = (pos + 1).min(len);
                    ct.overwrite(pos, end, "Y");
                }
                b.iter(|| black_box(ct.build_string()));
            },
        );

        // Same but with source map generation (iterates chunks twice)
        group.bench_with_input(
            BenchmarkId::new("build_string_and_map", num_edits),
            &num_edits,
            |b, &n| {
                let allocator = Allocator::new();
                let mut ct = CodeTransform::new(&source, &allocator);
                for i in 1..=n {
                    let pos = step * i;
                    let end = (pos + 1).min(len);
                    ct.overwrite(pos, end, "Y");
                }
                b.iter(|| {
                    black_box(ct.build_string());
                    let options = SourceMapOptions::new().with_source("test.vue");
                    black_box(ct.generate_map(options));
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Group 6: Throughput scaling by input size
// ============================================================================

fn scaling(c: &mut Criterion) {
    let fixtures: Vec<(&str, String)> = ["simple", "medium", "large", "kitchen-sink"]
        .iter()
        .map(|name| (*name, load_fixture(name)))
        .collect();

    let mut group = c.benchmark_group("code_transform/scaling");

    for (name, source) in &fixtures {
        group.throughput(Throughput::Bytes(source.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("realistic_edits", name),
            source,
            |b, source| {
                let len = source.len() as u32;
                b.iter(|| {
                    let allocator = Allocator::new();
                    let mut ct = CodeTransform::new(black_box(source), &allocator);

                    // 20 overwrites spread across the source
                    let ow_step = len / 21;
                    for i in 1..=20u32 {
                        let start = ow_step * i;
                        let end = (start + 5).min(len);
                        ct.overwrite(start, end, "X");
                    }

                    // 10 inserts spread across the source
                    let ins_step = len / 11;
                    for i in 1..=10u32 {
                        let pos = ins_step * i;
                        ct.prepend_left(pos, "_ctx.");
                    }

                    // 2 moves
                    if len > 200 {
                        ct.move_wrapped(50, 80, 0, "/*m1*/", ",");
                        ct.move_wrapped(120, 150, 0, "/*m2*/", ",");
                    }

                    // Build output + source map
                    black_box(ct.build_string());
                    let options = SourceMapOptions::new().with_source("test.vue");
                    black_box(ct.generate_map(options));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    basic_operations,
    batch_vs_sequential,
    move_operations,
    source_map_generation,
    chunk_iteration,
    scaling,
);
criterion_main!(benches);
