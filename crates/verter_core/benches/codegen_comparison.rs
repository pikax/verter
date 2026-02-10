use criterion::{criterion_group, criterion_main, Criterion};
use oxc_allocator::Allocator;
use std::hint::black_box;

use verter_core::builder::codegen::{generate, CodegenOptions};
use verter_core::builder::codegen_kai::{generate_kai, generate_with_tsx_kai, KaiCodegenOptions};

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

    group.bench_function("old/generate", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = CodegenOptions::new().with_filename("simple.vue");
            let result = generate(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("new/generate_kai", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = KaiCodegenOptions::new().with_filename("simple.vue");
            let result = generate_kai(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("new/generate_tsx_kai", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut options = KaiCodegenOptions::new().with_filename("simple.vue");
            options.include_tsx = true;
            let result = generate_with_tsx_kai(black_box(&source), black_box(&options), &allocator);
            black_box(result.tsx);
        });
    });

    group.finish();
}

fn bench_medium_sfc(c: &mut Criterion) {
    let source = load_fixture("medium");
    let mut group = c.benchmark_group("medium_sfc");

    group.bench_function("old/generate", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = CodegenOptions::new().with_filename("medium.vue");
            let result = generate(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("new/generate_kai", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = KaiCodegenOptions::new().with_filename("medium.vue");
            let result = generate_kai(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("new/generate_tsx_kai", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut options = KaiCodegenOptions::new().with_filename("medium.vue");
            options.include_tsx = true;
            let result = generate_with_tsx_kai(black_box(&source), black_box(&options), &allocator);
            black_box(result.tsx);
        });
    });

    group.finish();
}

fn bench_large_sfc(c: &mut Criterion) {
    let source = load_fixture("large");
    let mut group = c.benchmark_group("large_sfc");

    group.bench_function("old/generate", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = CodegenOptions::new().with_filename("large.vue");
            let result = generate(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("new/generate_kai", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let options = KaiCodegenOptions::new().with_filename("large.vue");
            let result = generate_kai(black_box(&source), black_box(&options), &allocator);
            black_box(result.code);
        });
    });

    group.bench_function("new/generate_tsx_kai", |b| {
        b.iter(|| {
            let allocator = Allocator::new();
            let mut options = KaiCodegenOptions::new().with_filename("large.vue");
            options.include_tsx = true;
            let result = generate_with_tsx_kai(black_box(&source), black_box(&options), &allocator);
            black_box(result.tsx);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_simple_sfc, bench_medium_sfc, bench_large_sfc);
criterion_main!(benches);
