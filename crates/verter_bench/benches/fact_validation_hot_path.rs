//! Sub-task K — `fact_validation_hot_path` warm-hit microbench.
//!
//! Stage 6d / R24 contract: warm cache validation runs counter-only
//! — zero allocation, zero structured payload emission per hit. The
//! microbench here measures the WALL-CLOCK warm-hit p50/p99 against
//! the Stage 0 `target/cache-baseline.json` target. The CI job's
//! >10% p99 regression gate consumes the Criterion output.
//!
//! **Allocator instrumentation.** The R24 "zero allocation per
//! warm hit" assertion requires a `#[global_allocator]` counter
//! wrapper. `verter_bench` does not currently install one — the
//! per-bench allocation count would require swapping the global
//! allocator and bumping a counter on every `alloc`/`dealloc`.
//! That is a substantial wiring change; the Stage 6d landing
//! documents the gap (commit body + the Stage 7 canary
//! follow-up). The wall-clock baseline this bench produces is
//! consumable by Stage 7's allocator-aware canary as the
//! regression target.
//!
//! Hermeticity: no third-party corpus or external fixture is used;
//! the bench constructs a bare-host singleton in-process and
//! exercises the warm-hit path through `ValidatedFactCache`.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};

use verter_session::resolver_core::{
    FactVersionRef, ParseFactRef, ValidatedFactCache,
};
use verter_session::semantic_query::HashValue;
use verter_semantic::facts::{FactKey, FactLane, SymbolSpace};

fn make_view() -> impl verter_session::resolver_core::StoreView {
    verter_session::resolver_core::PermissiveStoreView::default()
}

fn dummy_fact(canonical: &str, name: &str, expected_hash: HashValue) -> FactVersionRef {
    FactVersionRef::Parse(ParseFactRef {
        canonical_id: canonical.to_string(),
        key: FactKey::Export {
            name: name.into(),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash,
    })
}

/// Single-fact warm hit on a populated cache. The view's
/// `validates(...)` returns `true` (the `PermissiveStoreView`
/// accepts every fact), so this measures the steady-state warm-hit
/// path: shard-read on the outer `DashMap`, `ArcSwap.load()`,
/// iterate the (single) candidate, fact-validate, return
/// `Some(Arc<V>)`.
fn bench_warm_hit_single_fact(c: &mut Criterion) {
    let cache: ValidatedFactCache<&'static str, u32> = ValidatedFactCache::default();
    let view = make_view();
    cache.insert("k", 42u32, vec![dummy_fact("/w/a.ts", "Foo", [0; 16])]);

    c.bench_function("fact_validation_hot_path/warm_hit_single_fact", |b| {
        b.iter(|| {
            let result = cache.get_if_valid(&"k", &view);
            black_box(result);
        });
    });
}

/// 8-fact warm hit on a populated cache. Measures the steady-state
/// warm-hit path with a realistic fact-set size (a typical
/// component-meta query observes between 4 and 16 facts).
fn bench_warm_hit_eight_facts(c: &mut Criterion) {
    let cache: ValidatedFactCache<&'static str, u32> = ValidatedFactCache::default();
    let view = make_view();
    let mut facts = Vec::with_capacity(8);
    for i in 0..8u8 {
        let mut h = [0u8; 16];
        h[0] = i;
        facts.push(dummy_fact("/w/a.ts", &format!("F{i}"), h));
    }
    cache.insert("k", 42u32, facts);

    c.bench_function("fact_validation_hot_path/warm_hit_8_facts", |b| {
        b.iter(|| {
            let result = cache.get_if_valid(&"k", &view);
            black_box(result);
        });
    });
}

/// 32-fact warm hit. Stresses the fact-iteration loop on a larger
/// signature — the inner `.iter().all(|fact| view.validates(fact))`
/// must scale linearly + cheaply.
fn bench_warm_hit_thirtytwo_facts(c: &mut Criterion) {
    let cache: ValidatedFactCache<&'static str, u32> = ValidatedFactCache::default();
    let view = make_view();
    let mut facts = Vec::with_capacity(32);
    for i in 0..32u8 {
        let mut h = [0u8; 16];
        h[0] = i;
        facts.push(dummy_fact("/w/a.ts", &format!("F{i}"), h));
    }
    cache.insert("k", 42u32, facts);

    c.bench_function("fact_validation_hot_path/warm_hit_32_facts", |b| {
        b.iter(|| {
            let result = cache.get_if_valid(&"k", &view);
            black_box(result);
        });
    });
}

/// Warm-miss path (no entry under the key). Discriminates the
/// outer-DashMap shard-read latency from the inner candidate
/// iteration — a missing entry returns `None` after one shard
/// read.
fn bench_warm_miss(c: &mut Criterion) {
    let cache: ValidatedFactCache<&'static str, u32> = ValidatedFactCache::default();
    let view = make_view();
    cache.insert("k", 42u32, vec![dummy_fact("/w/a.ts", "Foo", [0; 16])]);

    c.bench_function("fact_validation_hot_path/warm_miss_no_entry", |b| {
        b.iter(|| {
            let result = cache.get_if_valid(&"missing", &view);
            black_box(result);
        });
    });
}

criterion_group!(
    benches,
    bench_warm_hit_single_fact,
    bench_warm_hit_eight_facts,
    bench_warm_hit_thirtytwo_facts,
    bench_warm_miss,
);
criterion_main!(benches);
