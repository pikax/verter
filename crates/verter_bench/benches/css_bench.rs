//! CSS style-pipeline benchmarks.
//!
//! The benchmark universe — groups, identities, input generators, and the
//! measured pipeline call behind each identity — is defined ONCE in
//! `verter_bench::css_identities` and registered here; the latency gate binary
//! (`src/bin/css_latency_gate.rs`) derives its identity universe from that
//! same module, so the set a gate compares against is structurally the set
//! this bench registers.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use verter_bench::css_identities::universe;

fn bench_group(c: &mut Criterion, group_name: &str) {
    let mut group = c.benchmark_group(group_name);
    for case in universe()
        .into_iter()
        .filter(|case| case.group == group_name)
    {
        group.throughput(Throughput::Bytes(case.css.len() as u64));
        match case.param {
            Some(param) => {
                group.bench_with_input(
                    BenchmarkId::new(case.function_id, param),
                    &case,
                    |b, case| {
                        b.iter(|| case.op.run(&case.css));
                    },
                );
            }
            None => {
                group.bench_function(case.function_id, |b| {
                    b.iter(|| case.op.run(&case.css));
                });
            }
        }
    }
    group.finish();
}

fn bench_process_style(c: &mut Criterion) {
    bench_group(c, "process_style");
}

fn bench_prepass(c: &mut Criterion) {
    bench_group(c, "prepass");
}

fn bench_scoped(c: &mut Criterion) {
    bench_group(c, "scoped");
}

fn bench_modules(c: &mut Criterion) {
    bench_group(c, "modules");
}

criterion_group!(
    benches,
    bench_process_style,
    bench_prepass,
    bench_scoped,
    bench_modules,
);
criterion_main!(benches);
