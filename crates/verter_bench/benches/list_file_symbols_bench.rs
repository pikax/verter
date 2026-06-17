//! Wall-clock budget for `VerterHost::list_file_symbols` on a ~5 KLOC /
//! 250-symbol TypeScript file.
//!
//! This bench is the SOLE home of the wall-clock measurement for the
//! symbol-inventory read path. The corresponding unit test
//! (`verter_session::typeinfo::tests::list_file_symbols_5kloc_is_complete_and_warm`)
//! asserts only DETERMINISTIC invariants (exactly 250 symbols + a warm
//! pass that performs zero IndexedReady rebuild); fixed wall-clock
//! ceilings flake under nextest process-per-test CPU oversubscription,
//! so the timing budget lives here instead.
//!
//! Two cache modes are measured separately:
//!
//! - `cold`: a fresh host + upsert per sample (the `IndexedReady` is not
//!   yet materialised), measuring the first `list_file_symbols` — the
//!   parse → shallow-analysis → IndexedReady build.
//! - `warm`: a pre-warmed host (IndexedReady already cached), measuring a
//!   repeated `list_file_symbols` that serves the cached artifact.
//!
//! Each sample asserts completeness (exactly `SYMBOL_COUNT` entries) so a
//! regression that silently truncated the inventory cannot masquerade as
//! a faster run.

#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use verter_session::{HostConfig, UpsertRequest, VerterHost};

/// Top-level `export type T{i}` declarations in the fixture. Every other
/// line is a `// filler` comment, so this is the exact listed-symbol
/// count.
const SYMBOL_COUNT: usize = 250;

const BIG_TS_CANONICAL: &str = "/big.ts";

/// Build the ~5 KLOC fixture: `SYMBOL_COUNT` type aliases, each padded
/// with 19 comment lines (≈ 5_000 lines total). Mirrors the unit-test
/// fixture in `verter_session::typeinfo::tests`.
fn big_ts_fixture() -> String {
    let mut source = String::with_capacity(5_000);
    for i in 0..SYMBOL_COUNT {
        source.push_str("export type T");
        source.push_str(&i.to_string());
        source.push_str(" = { value: number };\n");
        for _ in 0..19 {
            source.push_str("// filler line\n");
        }
    }
    source
}

fn upsert_big_ts(host: &VerterHost, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(BIG_TS_CANONICAL.to_string()),
        input_id: BIG_TS_CANONICAL.to_string(),
        source: Arc::from(source),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(BIG_TS_CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// Build a host with the fixture upserted but the `IndexedReady` NOT yet
/// materialised — the next `list_file_symbols` is a cold build.
fn cold_host(source: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_big_ts(&host, source);
    host
}

fn list_file_symbols_bench(c: &mut Criterion) {
    let source = big_ts_fixture();

    let mut group = c.benchmark_group("list_file_symbols_5kloc");
    // Report symbol count as the throughput element count, so criterion
    // prints per-symbol cost alongside per-call cost.
    group.throughput(Throughput::Elements(SYMBOL_COUNT as u64));

    // cache mode = cold: fresh IndexedReady build per sample.
    group.bench_with_input(
        BenchmarkId::new("cache_mode", "cold"),
        &source,
        |b, source| {
            b.iter_batched(
                || cold_host(source),
                |host| {
                    let symbols = host.list_file_symbols(BIG_TS_CANONICAL);
                    assert_eq!(
                        symbols.len(),
                        SYMBOL_COUNT,
                        "cold list must surface exactly {SYMBOL_COUNT} symbols"
                    );
                    symbols
                },
                criterion::BatchSize::SmallInput,
            );
        },
    );

    // cache mode = warm: IndexedReady already cached; serves the artifact.
    let warm_host = cold_host(&source);
    // Warm the cache once outside the measured loop.
    let warmup = warm_host.list_file_symbols(BIG_TS_CANONICAL);
    assert_eq!(
        warmup.len(),
        SYMBOL_COUNT,
        "warm-up list must surface exactly {SYMBOL_COUNT} symbols"
    );
    group.bench_with_input(
        BenchmarkId::new("cache_mode", "warm"),
        &warm_host,
        |b, host| {
            b.iter(|| {
                let symbols = host.list_file_symbols(BIG_TS_CANONICAL);
                assert_eq!(
                    symbols.len(),
                    SYMBOL_COUNT,
                    "warm list must surface exactly {SYMBOL_COUNT} symbols"
                );
                symbols
            });
        },
    );

    group.finish();
}

criterion_group!(list_file_symbols_benches, list_file_symbols_bench);
criterion_main!(list_file_symbols_benches);
