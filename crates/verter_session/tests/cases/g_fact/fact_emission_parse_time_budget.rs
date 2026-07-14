//! Parse-time fact emission scales LINEARLY with file size — the
//! emitter walks the pre-extracted `ShallowFileState` once, doing
//! O(file_size) work, never O(N²) or worse. That asymptotic class is
//! the hard guarantee this test defends.
//!
//! A single absolute wall-clock measurement at ONE input size cannot
//! prove the class: a constant-factor-slow-but-linear emitter and a
//! quadratic one are indistinguishable from one data point, and the
//! absolute number is hostage to machine speed (this is exactly why an
//! earlier single-size "≤ Nx the baseline" ceiling was too noisy to be
//! meaningful). So we measure the *scaling* across two sizes instead:
//!
//! 1. Build two synthetic inputs of `N` and `2N` decls with IDENTICAL
//!    per-decl shape (`build_large_indexed`), so the per-decl constant
//!    factor is the same on both and cancels out of the ratio. No
//!    parser is invoked — the shallow walk's cost is pre-paid; we time
//!    only `emit_parse_facts`.
//! 2. Time `emit_parse_facts` on each size, taking the MINIMUM over
//!    several iterations: timing noise is additive (scheduling, page
//!    faults, thermal), so the minimum is the cleanest estimate of the
//!    true per-size cost and is far less flaky than a mean.
//! 3. Assert the time ratio `T(2N) / T(N)` stays below the class
//!    boundary:
//!      - linear    O(N)  ⇒ doubling the input ⇒ ratio ≈ 2.0x
//!      - quadratic O(N²) ⇒ doubling the input ⇒ ratio ≈ 4.0x
//!
//!    A `3.0x` threshold sits squarely between the two classes: it
//!    passes a linear emitter with comfortable margin and trips on a
//!    quadratic (or worse) one. It is a SCALING guard, not a
//!    constant-factor one — a 10x-slower-but-still-linear emitter has
//!    the same ~2.0x ratio and passes; only a change in the algorithmic
//!    class moves the ratio.
//!
//! The emitter does the HEADER work (per-member presence facts, per-
//! import facts, `MemberShape`, the per-file export set); body-derived
//! fingerprints are NOT computed here — they lower lazily on later
//! semantic demand. So this guard characterises the parse-time
//! header-walk contribution, and proves it stays O(file_size).

use std::sync::Arc;
use std::time::Instant;

use verter_session::fact_emission::emit_parse_facts;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;

fn empty_external(
) -> Arc<verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSource> {
    Arc::new(verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default())
}

fn build_large_indexed(decl_count: usize) -> Arc<IndexedReady> {
    // Author `decl_count` IDENTICAL-shape interface decls and build
    // through the production-shaped service-backed path: the real header
    // walk pre-pays the shallow inventory at construction (untimed), so
    // the timed section below measures ONLY `emit_parse_facts`.
    let mut source = String::with_capacity(decl_count * 48);
    for i in 0..decl_count {
        source.push_str(&format!("export interface Decl{i} {{ a: string }}\n"));
    }
    let shallow =
        ShallowFileState::service_backed_for_test_with_hash("/large.ts", &source, [0u8; 16]);
    Arc::new(IndexedReady::new_for_test_with_state(
        [0u8; 16],
        shallow,
        Arc::from(source.as_str()),
        Arc::from(source.as_str()),
        empty_external(),
    ))
}

/// Minimum wall-clock cost of one `emit_parse_facts` call on `indexed`,
/// taken over `iters` runs. Timing noise is strictly additive, so the
/// minimum is the cleanest estimate of the true cost and the least
/// flaky statistic for a ratio comparison.
fn min_emit_cost(indexed: &Arc<IndexedReady>, iters: u32) -> std::time::Duration {
    let mut best = std::time::Duration::MAX;
    for _ in 0..iters {
        let start = Instant::now();
        let _ = emit_parse_facts(indexed);
        best = best.min(start.elapsed());
    }
    best
}

#[test]
fn fact_emission_scales_linearly_on_10k_decl_input() {
    // Two-size algorithmic-scaling guard. We time `emit_parse_facts` at
    // size `N` and `2N` with IDENTICAL per-decl shape, so the per-decl
    // constant factor cancels and only the asymptotic class shows in the
    // ratio:
    //   - linear    O(N)  ⇒ doubling the input ⇒ T(2N)/T(N) ≈ 2.0x
    //   - quadratic O(N²) ⇒ doubling the input ⇒ T(2N)/T(N) ≈ 4.0x
    // The 3.0x threshold sits between the two classes: it passes a linear
    // emitter with margin and trips on a quadratic (or worse) one. This
    // is a SCALING guard — a constant-factor-slow-but-linear emitter has
    // the same ~2.0x ratio and still passes; only a change of algorithmic
    // class moves the ratio past 3.0x.
    const N: usize = 10_000;
    const ITERS: u32 = 7;
    const THRESHOLD_MILLI: u128 = 3_000; // 3.0x, scaled by 1000 for integer math.

    let indexed_n = build_large_indexed(N);
    let indexed_2n = build_large_indexed(2 * N);

    // Warm up both inputs (allocator, page faults, instruction cache)
    // before any timed run so the first iteration is not an outlier.
    let _ = emit_parse_facts(&indexed_n);
    let _ = emit_parse_facts(&indexed_2n);

    let n_dur = min_emit_cost(&indexed_n, ITERS);
    let twon_dur = min_emit_cost(&indexed_2n, ITERS);

    // Ratio T(2N)/T(N), scaled by 1000 so we can compare with integer math
    // (avoids float flakiness). Linear ⇒ ~2000; quadratic ⇒ ~4000.
    let ratio_milli = (twon_dur.as_nanos() * 1000) / n_dur.as_nanos().max(1);
    eprintln!(
        "fact-emission scaling — emit({N}): {n_dur:?}; emit({}): {twon_dur:?}; \
         T(2N)/T(N) ratio: {}.{:03}x (threshold {}.{:03}x)",
        2 * N,
        ratio_milli / 1000,
        ratio_milli % 1000,
        THRESHOLD_MILLI / 1000,
        THRESHOLD_MILLI % 1000,
    );

    assert!(
        ratio_milli < THRESHOLD_MILLI,
        "emit_parse_facts must scale LINEARLY (O(file_size)), not O(N²). Doubling the \
         decl count from {N} to {} should roughly double the time (ratio ≈ 2.0x); a \
         quadratic emitter would show ≈ 4.0x. Observed ratio {}.{:03}x crosses the 3.0x \
         class boundary, indicating a super-linear regression. emit({N})={n_dur:?}, \
         emit({})={twon_dur:?}.",
        2 * N,
        ratio_milli / 1000,
        ratio_milli % 1000,
        2 * N,
    );
}

#[test]
fn fact_emission_produces_expected_fact_count_on_10k_decls() {
    use verter_semantic::facts::registry::FactKey;

    // 10k single-member interface decls produce per-decl `MemberShape`
    // + per-member `MemberPresence` + the per-file `SyntacticExportSet`
    // — all HEADER-derived and eager. The body-sensitive `Export` /
    // `LocalDecl` facts are NOT emitted at parse time (publishing lowers
    // zero declaration bodies; they lower lazily on first demand), so
    // they are ABSENT from this registry. The exact count is
    // implementation-detail-bound; we check the lower bound plus the
    // body-sensitivity invariant.
    let indexed = build_large_indexed(10_000);
    let emission = emit_parse_facts(&indexed);
    let registry = emission.facts.registry();
    assert!(
        registry.len() >= 10_000,
        "fact emission MUST emit at least one header fact per decl ({} got, expected ≥ 10_000)",
        registry.len()
    );

    // Body-sensitivity guard (mirrors the keys checked by
    // `emit_parse_facts_never_hashes_decl_bodies`): publishing lowers
    // ZERO declaration bodies, so NOT ONE body-derived `Export` /
    // `LocalDecl` fact may leak into the parse-time registry — even at
    // scale. The two-size scaling test above guards the asymptotic
    // class but would NOT notice eager body-fact emission reintroduced
    // at scale (eager body hashing is still linear); this count makes
    // that regression fail.
    let body_derived = registry
        .iter()
        .filter(|(key, _)| matches!(key, FactKey::Export { .. } | FactKey::LocalDecl { .. }))
        .count();
    assert_eq!(
        body_derived,
        0,
        "parse-time fact emission must be HEADER-ONLY: {body_derived} body-derived \
         Export/LocalDecl facts leaked into the publish-time registry (expected 0 — \
         they lower lazily on first demand). The {} total facts are all header-derived.",
        registry.len(),
    );
}
