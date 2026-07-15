//! Warm-hit fact-tracer microbench.
//!
//! Measures the computational cost of [`VerterHost::current_fact_tracer`]
//! when no [`VerterHost::with_fact_tracer`] scope is on the stack
//! (the warm-hit path).
//!
//! The R24 contract states that warm cache validation is
//! counter-only — zero allocation, zero structured payload emission
//! per hit. The microbench here proves the COMPUTATIONAL component
//! of that contract: in the absence of an installed tracer, the
//! accessor is a pointer-load + null-check. p99 latency should sit
//! well under 50ns.
//!
//! The full allocation-counter assertion (R24 "zero allocation per
//! hit") requires a test-allocator harness that is not yet wired
//! into `verter_bench`. The bench here is informational: it
//! demonstrates the warm-hit path is constant-time / zero-branch
//! and provides a wall-clock baseline. The allocator-counter
//! assertion is a follow-up wiring tracked in the bench harness
//! upgrade — this bench provides the baseline + the call shape
//! that the future allocator harness will measure against.
//!
//! Hermeticity: no third-party corpus or external fixture is used;
//! the bench constructs a bare-host singleton in-process.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};

use verter_session::{CompileErrorPolicy, HostConfig, VerterHost};

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }))
}

/// Warm-hit microbench: no tracer installed; the accessor returns
/// `None` after a pointer-load + null-check.
fn bench_warm_hit_no_tracer(c: &mut Criterion) {
    let host = make_host();
    c.bench_function("fact_tracer/warm_hit_no_tracer", |b| {
        b.iter(|| {
            // The host returns `None` here. We `black_box` the
            // result so the optimiser cannot elide the read.
            let result = black_box(host.current_fact_tracer());
            black_box(result.is_none());
        });
    });
}

/// Cold-compute microbench (informational): one tracer install +
/// uninstall per iteration. Measures the per-scope overhead of
/// `with_fact_tracer` so future refactors can spot a regression
/// without firing the warm-hit baseline.
fn bench_cold_compute_install_uninstall(c: &mut Criterion) {
    let host = make_host();
    c.bench_function("fact_tracer/cold_compute_install_uninstall", |b| {
        b.iter(|| {
            let ((), set) = host.with_fact_tracer(|| {
                // Empty body — measures the install+drop overhead.
            });
            black_box(set);
        });
    });
}

/// One-observation microbench (informational): install a tracer,
/// record one observation, uninstall, finalise. Measures the
/// minimal-work cold path.
fn bench_cold_compute_one_observation(c: &mut Criterion) {
    let host = make_host();
    // Construct a representative fact once outside the loop; cloning
    // a `FactVersionRef::FileWholeHash` is the cheapest variant.
    let fact = verter_session::resolver_core::FactVersionRef::FileWholeHash {
        canonical_id: "/m.ts".to_string(),
        hash: [0; 16],
    };
    c.bench_function("fact_tracer/cold_compute_one_observation", |b| {
        b.iter(|| {
            let ((), set) = host.with_fact_tracer(|| {
                if let Some(cell) = host.current_fact_tracer() {
                    cell.observe(fact.clone());
                }
            });
            black_box(set);
        });
    });
}

criterion_group!(
    benches,
    bench_warm_hit_no_tracer,
    bench_cold_compute_install_uninstall,
    bench_cold_compute_one_observation,
);
criterion_main!(benches);
