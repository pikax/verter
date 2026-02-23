//! Benchmarks for ProjectIndex operations in verter_analysis.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::path::PathBuf;
use verter_analysis::{
    ComponentUsageOwned, FileUsageFlags, FileUsageInfoOwned, InjectUsageOwned, ProjectIndex,
    ProvideUsageOwned, StyleUsageInfoOwned,
};

/// Build a realistic file usage info with provide, inject, component and style data.
fn make_file_info(i: usize) -> FileUsageInfoOwned {
    let mut info = FileUsageInfoOwned::default();

    // Half the files provide
    if i % 2 == 0 {
        info.provides.push(ProvideUsageOwned {
            key: Some(format!("key-{}", i % 20)),
            is_dynamic_key: false,
            start: 0,
            end: 10,
        });
        info.flags |= FileUsageFlags::HAS_PROVIDE.bits();
    }

    // A third inject
    if i % 3 == 0 {
        info.injects.push(InjectUsageOwned {
            key: Some(format!("key-{}", (i + 5) % 20)),
            is_dynamic_key: false,
            has_default: false,
            binding_name: None,
            start: 0,
            end: 10,
        });
        info.flags |= FileUsageFlags::HAS_INJECT.bits();
    }

    // Most files use components
    if i % 4 != 0 {
        info.components.push(ComponentUsageOwned {
            name: Some(format!("Component{}", i % 10)),
            is_dynamic: false,
            start: 0,
            end: 10,
        });
        info.flags |= FileUsageFlags::HAS_COMPONENT_USAGE.bits();
    }

    // Some files have styles
    if i % 5 == 0 {
        info.styles.push(StyleUsageInfoOwned {
            lang: Some("css".to_string()),
            scoped: true,
            class_names: vec![format!("cls-{i}"), format!("active-{i}")],
            custom_property_names: vec![format!("--var-{i}")],
            ..Default::default()
        });
        info.flags |= FileUsageFlags::HAS_SCOPED_STYLE.bits();
    }

    info
}

fn make_path(i: usize) -> PathBuf {
    PathBuf::from(format!("src/components/Component{i}.vue"))
}

fn build_index(n: usize) -> ProjectIndex {
    let mut index = ProjectIndex::with_capacity(n);
    for i in 0..n {
        index.add_file(make_path(i), make_file_info(i));
    }
    index
}

fn bench_add_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("add_file");
    for n in [10, 50, 100, 500] {
        let infos: Vec<_> = (0..n).map(|i| (make_path(i), make_file_info(i))).collect();
        group.bench_with_input(BenchmarkId::from_parameter(n), &infos, |b, infos| {
            b.iter(|| {
                let mut index = ProjectIndex::with_capacity(infos.len());
                for (path, info) in infos {
                    index.add_file(path.clone(), info.clone());
                }
                black_box(&index);
            });
        });
    }
    group.finish();
}

fn bench_remove_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("remove_file");
    for n in [10, 50, 100] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || build_index(n),
                |mut index| {
                    for i in 0..n {
                        index.remove_file(&make_path(i));
                    }
                    black_box(&index);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_files_providing(c: &mut Criterion) {
    let index = build_index(200);
    c.bench_function("files_providing", |b| {
        b.iter(|| {
            for k in 0..20 {
                black_box(index.files_providing(&format!("key-{k}")).count());
            }
        });
    });
}

fn bench_validate_file_injects(c: &mut Criterion) {
    let index = build_index(200);
    c.bench_function("validate_file_injects", |b| {
        b.iter(|| {
            for i in (0..200).step_by(3) {
                black_box(index.validate_file_injects(&make_path(i)));
            }
        });
    });
}

fn bench_stats(c: &mut Criterion) {
    let index = build_index(200);
    c.bench_function("stats", |b| {
        b.iter(|| black_box(index.stats()));
    });
}

fn bench_provide_inject_summary(c: &mut Criterion) {
    let index = build_index(200);
    c.bench_function("provide_inject_summary", |b| {
        b.iter(|| black_box(index.provide_inject_summary()));
    });
}

criterion_group!(
    benches,
    bench_add_file,
    bench_remove_file,
    bench_files_providing,
    bench_validate_file_injects,
    bench_stats,
    bench_provide_inject_summary,
);
criterion_main!(benches);
