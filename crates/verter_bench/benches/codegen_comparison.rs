use criterion::{criterion_group, criterion_main, Criterion};
use oxc_allocator::Allocator;
use std::hint::black_box;

use verter_core::builder::codegen::{compile, compile_with_tsx, CodegenOptions};

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/benches/fixtures/{}.vue",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e))
}

fn bench_simple_sfc(c: &mut Criterion) {
    let source = load_fixture("simple");
    let mut group = c.benchmark_group("simple_sfc");

    group.bench_function("compile", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = CodegenOptions::new().with_filename("simple.vue");
            let result = compile(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("compile_with_tsx", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut options = CodegenOptions::new().with_filename("simple.vue");
            options.include_tsx = true;
            let result = compile_with_tsx(black_box(&source), black_box(&options), &allocator);
            black_box(result.tsx);
        });
    });

    group.finish();
}

fn bench_medium_sfc(c: &mut Criterion) {
    let source = load_fixture("medium");
    let mut group = c.benchmark_group("medium_sfc");

    group.bench_function("compile", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = CodegenOptions::new().with_filename("medium.vue");
            let result = compile(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("compile_with_tsx", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut options = CodegenOptions::new().with_filename("medium.vue");
            options.include_tsx = true;
            let result = compile_with_tsx(black_box(&source), black_box(&options), &allocator);
            black_box(result.tsx);
        });
    });

    group.finish();
}

fn bench_large_sfc(c: &mut Criterion) {
    let source = load_fixture("large");
    let mut group = c.benchmark_group("large_sfc");

    group.bench_function("compile", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = CodegenOptions::new().with_filename("large.vue");
            let result = compile(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("compile_with_tsx", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut options = CodegenOptions::new().with_filename("large.vue");
            options.include_tsx = true;
            let result = compile_with_tsx(black_box(&source), black_box(&options), &allocator);
            black_box(result.tsx);
        });
    });

    group.finish();
}

fn bench_kitchen_sink(c: &mut Criterion) {
    let source = load_fixture("kitchen-sink");
    let mut group = c.benchmark_group("kitchen_sink");

    group.bench_function("compile", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = CodegenOptions::new().with_filename("kitchen-sink.vue");
            let result = compile(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("compile_no_sourcemap", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut options = CodegenOptions::new().with_filename("kitchen-sink.vue");
            options.skip_source_map = true;
            let result = compile(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_simple_sfc,
    bench_medium_sfc,
    bench_large_sfc,
    bench_kitchen_sink
);
criterion_main!(benches);
