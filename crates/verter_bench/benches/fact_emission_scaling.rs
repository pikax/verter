//! Wall-clock scaling bench for `emit_parse_facts` — the residual
//! coverage the deterministic rails cannot provide.
//!
//! Parse-time fact emission is guarded in `verter_session`'s correctness
//! suite by three DETERMINISTIC rails, none of which uses a clock. Each
//! claims one thing:
//!
//! - `tests/cases/g_fact/fact_emission_output_cardinality.rs` — emitted
//!   fact CARDINALITY is affine in the declaration count (a zero second
//!   difference over three equally spaced sizes);
//! - `tests/cases/g_fact/fact_emission_work_class.rs` — NO REPEATED
//!   INVENTORY TRAVERSAL: the emitter takes a fixed number of passes over
//!   its shallow inventory regardless of file size;
//! - `tests/allocator_canaries.rs` — ALLOCATION volume (calls and bytes)
//!   stays in the linear class.
//!
//! Those instruments are exact and load-immune, which is why they belong
//! in a correctness suite.
//!
//! # What is left for a clock, stated precisely
//!
//! The traversal rail covers the specific shape of re-scanning the
//! inventory once per declaration — an allocation-free linear scan
//! replacing a constant-time lookup IS caught, deterministically, and
//! turns that rail RED. What no counter sees is arbitrary quadratic
//! computation over data the emitter has ALREADY collected: a nested loop
//! over a materialised `Vec`, or a sort moved inside the per-declaration
//! loop, emits no extra facts, allocates nothing, and re-walks no
//! inventory. Only elapsed time sees that.
//!
//! So a wall-clock measurement is still worth having, and this is where it
//! belongs. Its predecessor lived in the correctness suite as
//! `fact_emission_scales_linearly_on_10k_decl_input`, asserting
//! `T(2N)/T(N) < 3.0`; under machine contention the larger input is
//! descheduled disproportionately, so the ratio inflated while the
//! algorithmic class was untouched (3.061x on one loaded run, passing on
//! the six unloaded runs around it). A wall-clock number is a property of
//! the machine as much as of the code, so it is reported here rather than
//! asserted in a gate.
//!
//! The reported class number uses the same reasoning the retired
//! assertion did: with identical per-declaration shape at both sizes the
//! constant factor cancels, so
//!
//! - linear    O(N)  ⇒ doubling the input ⇒ T(2N)/T(N) ≈ 2.0x
//! - quadratic O(N²) ⇒ doubling the input ⇒ T(2N)/T(N) ≈ 4.0x
//!
//! and a value drifting toward 4.0x on an otherwise idle machine is the
//! signal to investigate. Criterion's own per-size statistics on top of
//! that give the usual regression tracking.
//!
//! The fixture is built through the PUBLIC host path — upsert a synthetic
//! TypeScript file, materialise its artifact the way any read does, and
//! take the published `IndexedReady` back out of the project store — so
//! the bench needs no test-only constructor and no `test-support` feature
//! edge, and it measures the same artifact shape production publishes.
//!
//! Hermeticity: no third-party corpus; the declarations are synthesised
//! in-process on a standalone host.
//!
//! Run with:
//! `cargo bench -p verter_bench --bench fact_emission_scaling`

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use verter_session::fact_emission::emit_parse_facts;
use verter_session::project_type_store::IndexedReady;
use verter_session::{HostConfig, UpsertRequest, VerterHost};

const CANONICAL: &str = "/large.ts";

/// Publish a synthetic file of `decl_count` IDENTICAL-shape interface
/// declarations through the real host and hand back the resulting
/// artifact. Identical per-declaration shape at every size is what makes
/// the constant factor cancel out of the ratio below.
fn indexed_for(decl_count: usize) -> Arc<IndexedReady> {
    let mut source = String::with_capacity(decl_count * 48);
    for i in 0..decl_count {
        source.push_str(&format!("export interface Decl{i} {{ a: string }}\n"));
    }

    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: Arc::from(source.as_str()),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });

    // `upsert` records the source; it does NOT materialise the canonical
    // post-parse artifact. Any read does — `list_file_symbols` runs the
    // shared `ensure_indexed_ready_serve` pipeline — and it doubles as a
    // completeness check that the fixture really carries `decl_count`
    // declarations, so a truncated fixture cannot silently shrink the
    // measured work.
    let symbols = host.list_file_symbols(CANONICAL);
    assert_eq!(
        symbols.len(),
        decl_count,
        "fixture must publish exactly {decl_count} symbols; a truncated inventory would \
         understate the measured emission work"
    );

    host.project_type_store()
        .indexed()
        .get_any(CANONICAL)
        .expect("materialising the fixture must publish an IndexedReady artifact")
}

/// Minimum wall-clock cost of one `emit_parse_facts` call over `iters`
/// runs. Timing noise is additive, so the minimum is the cleanest
/// estimate of the true cost — the same statistic the retired assertion
/// used, kept because it is the right statistic even when the number is
/// only reported.
fn min_emit_cost(indexed: &Arc<IndexedReady>, iters: u32) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..iters {
        let start = Instant::now();
        let emission = emit_parse_facts(indexed);
        let elapsed = start.elapsed();
        black_box(&emission);
        best = best.min(elapsed);
    }
    best
}

fn fact_emission_scaling(c: &mut Criterion) {
    const N: usize = 10_000;
    const ITERS: u32 = 7;

    let indexed_n = indexed_for(N);
    let indexed_2n = indexed_for(2 * N);

    // Warm both inputs (allocator, page faults, instruction cache) so the
    // first timed run is not an outlier.
    black_box(emit_parse_facts(&indexed_n));
    black_box(emit_parse_facts(&indexed_2n));

    let n_dur = min_emit_cost(&indexed_n, ITERS);
    let twon_dur = min_emit_cost(&indexed_2n, ITERS);

    // Scaled by 1000 for integer reporting. Linear ⇒ ~2000;
    // quadratic ⇒ ~4000.
    let ratio_milli = (twon_dur.as_nanos() * 1000) / n_dur.as_nanos().max(1);
    println!(
        "fact-emission wall-clock scaling — emit({N}): {n_dur:?}; emit({}): {twon_dur:?}; \
         T(2N)/T(N) = {}.{:03}x (linear ≈ 2.0x, quadratic ≈ 4.0x). Reported, not asserted: \
         a wall-clock ratio is a property of the machine as well as the code. Drift toward \
         4.0x on an idle machine indicates a super-linear regression in the header walk.",
        2 * N,
        ratio_milli / 1000,
        ratio_milli % 1000,
    );

    let mut group = c.benchmark_group("fact_emission_scaling");
    for (label, indexed) in [(N, &indexed_n), (2 * N, &indexed_2n)] {
        group.bench_with_input(
            BenchmarkId::new("emit_parse_facts", label),
            indexed,
            |b, input| b.iter(|| black_box(emit_parse_facts(input))),
        );
    }
    group.finish();
}

criterion_group!(benches, fact_emission_scaling);
criterion_main!(benches);
