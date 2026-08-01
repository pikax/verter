//! Parse-time fact emission produces an emitted-fact CARDINALITY that is
//! affine in the declaration count.
//!
//! # What this guard claims, exactly
//!
//! One thing, and it is a statement about OUTPUT SIZE: the number of
//! facts `emit_parse_facts` puts in the registry grows by a constant
//! amount per declaration. Nothing here is a statement about how much
//! WORK the emitter does to produce them.
//!
//! That distinction is load-bearing and this file previously blurred it.
//! An emitter can be affine in output cardinality while doing arbitrarily
//! more work per declaration — the emitter's own `sort_unstable` calls
//! are already `O(N log N)`, and this guard passes them, as it should.
//! Read the claim as "output cardinality", never as an asymptotic work
//! or `O(file_size)` guarantee.
//!
//! # The instrument: a ZERO SECOND DIFFERENCE
//!
//! Emitted fact count is an exact, load-immune integer. Measure it at
//! three EQUALLY SPACED sizes `N`, `2N`, `3N` and take finite
//! differences:
//!
//! - affine `f(k) = a·k + c` ⇒ first differences `f(2N) − f(N)` and
//!   `f(3N) − f(2N)` are both `a·N` ⇒ SECOND difference exactly ZERO,
//!   for every `a` and every `c`;
//! - quadratic `f(k) = a·k²` ⇒ first differences `3a·N²` then `5a·N²` ⇒
//!   a second difference of `2a·N²`.
//!
//! So there is no threshold to tune and no magic constant to keep in
//! sync with the per-decl fact yield: any legitimate change to the
//! constant factor `a` or the per-file constant `c` cancels. The
//! quantity is a function of the input alone, so machine load cannot
//! move it.
//!
//! An exact-equality test can pass vacuously if the measured quantity
//! stops growing at all, so the guard also asserts the per-decl yield
//! is at least one fact per added declaration.
//!
//! # The other rails, and what each of them claims
//!
//! No single deterministic instrument covers the emitter's cost, so the
//! claims are split and each one is stated narrowly:
//!
//! | rail | claims |
//! |---|---|
//! | this file | emitted fact CARDINALITY is affine in the declaration count |
//! | `fact_emission_work_class.rs` | no repeated inventory TRAVERSAL (a fixed number of passes over the shallow inventory, independent of file size) |
//! | `tests/allocator_canaries.rs` | ALLOCATION volume (calls and bytes) stays in the linear class |
//! | `crates/verter_bench/benches/fact_emission_scaling.rs` | wall-clock scaling — reported, not asserted |
//!
//! The first three are exact and load-immune, which is why they live in
//! the correctness suite. None of them bounds arbitrary computation over
//! already-collected data; that residue is what the bench measurement is
//! for, and it is reported rather than gated because a wall-clock number
//! is a property of the machine as much as of the code.
//!
//! # Why the timing assert this file used to carry was removed
//!
//! It timed `emit_parse_facts` at `N` and `2N` and asserted
//! `T(2N)/T(N) < 3.0`. Under machine contention the larger input is
//! descheduled disproportionately, so the ratio inflates while the
//! algorithmic class is untouched: 3.061x on one loaded run, passing on
//! the six unloaded runs around it. Min-of-N iterations shrinks that
//! window without closing it, and raising the threshold only lowers the
//! flake rate while keeping a load-sensitive assert in a correctness
//! suite.
//!
//! The emitter does the HEADER work (per-member presence facts, per-
//! import facts, `MemberShape`, the per-file export set); body-derived
//! fingerprints are NOT computed here — they lower lazily on later
//! semantic demand. So this guard characterises the parse-time
//! header-walk output surface.

use std::sync::Arc;

use verter_session::fact_emission::emit_parse_facts;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;

fn build_large_indexed(decl_count: usize) -> Arc<IndexedReady> {
    // Author `decl_count` IDENTICAL-shape interface decls and build
    // through the production-shaped service-backed path: the real header
    // walk pre-pays the shallow inventory at construction, so the
    // measured section below observes ONLY `emit_parse_facts`.
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
    ))
}

/// Number of parse-domain facts `emit_parse_facts` emits for a file of
/// `decl_count` identical-shape declarations. Exact and deterministic:
/// the same input yields the same count on every machine and under any
/// load.
fn emitted_fact_count(decl_count: usize) -> usize {
    let indexed = build_large_indexed(decl_count);
    emit_parse_facts(&indexed).facts.registry().len()
}

#[test]
fn emitted_fact_cardinality_is_affine_in_decl_count() {
    // Three EQUALLY SPACED sizes. Equal spacing is what makes the
    // second difference meaningful: for an affine `f`, equally spaced
    // samples have equal first differences by construction, so any
    // inequality is a departure from linearity.
    //
    // `N` is deliberately modest. The instrument is EXACT, so scale buys
    // no signal here — it only buys realism, and the three fixtures
    // together parse the same 30k declarations the two fixtures of the
    // wall-clock predecessor did, while running three emits instead of
    // sixteen.
    const N: usize = 5_000;

    let f1 = emitted_fact_count(N);
    let f2 = emitted_fact_count(2 * N);
    let f3 = emitted_fact_count(3 * N);

    // Monotonicity, checked before the subtractions below so a shrinking
    // count reports the shape it actually has instead of panicking on
    // `usize` underflow. A larger file emitting FEWER header facts is a
    // regression in its own right (a key collision collapsing distinct
    // declarations onto one entry).
    assert!(
        f2 >= f1 && f3 >= f2,
        "emitted fact volume must not shrink as the declaration count grows: \
         facts({N})={f1}, facts({})={f2}, facts({})={f3}. A larger file emitting fewer header \
         facts means distinct declarations are collapsing onto one registry key.",
        2 * N,
        3 * N,
    );

    // First differences over a step of exactly `N` declarations.
    let d1 = f2 - f1;
    let d2 = f3 - f2;

    eprintln!(
        "fact-emission output cardinality — facts({N})={f1}, facts({})={f2}, facts({})={f3}; \
         first differences {d1} and {d2} (equal ⇒ affine in decl count); \
         per-added-decl fact yield {}.{:02}",
        2 * N,
        3 * N,
        d1 / N,
        (d1 * 100 / N) % 100,
    );

    // ANTI-VACUITY: an exact-equality assertion is satisfied trivially
    // by a quantity that does not grow, so pin the per-decl yield first.
    // Each added declaration must contribute at least one fact —
    // otherwise `d1 == d2 == 0` would "prove" linearity for an emitter
    // that had stopped emitting.
    assert!(
        d1 >= N,
        "anti-vacuity: adding {N} declarations added only {d1} facts (expected at least one per \
         declaration). The linearity assertion below compares first differences and would pass \
         vacuously on a non-growing count, so this guard must fail loudly instead. \
         facts({N})={f1}, facts({})={f2}.",
        2 * N,
    );

    // THE CARDINALITY GUARD. Equal first differences over equal input
    // steps ⇒ the emitted fact COUNT is exactly affine in the declaration
    // count ⇒ each declaration contributes a constant number of facts. A
    // regression emitting a fact per (declaration, declaration) pair has
    // first differences 3a·N² then 5a·N², and fails this exact integer
    // comparison — which machine load cannot mask.
    //
    // This says nothing about how much WORK produced those facts; see the
    // rail table in the module docs above.
    assert_eq!(
        d2,
        d1,
        "emit_parse_facts must emit a CONSTANT number of facts per declaration, so the emitted \
         cardinality is affine in the declaration count and equal input steps must produce EQUAL \
         first differences: facts({}) − facts({N}) = {d1}, but facts({}) − facts({}) = {d2}. \
         A second difference of {} means the per-declaration fact yield grows with file size — \
         the signature of emitting facts per declaration PAIR. Counts: facts({N})={f1}, \
         facts({})={f2}, facts({})={f3}. (This is an output-cardinality guard only: it does not \
         bound the emitter's work. Inventory traversal is guarded by \
         fact_emission_work_class.rs, allocation by tests/allocator_canaries.rs.)",
        2 * N,
        3 * N,
        2 * N,
        d2.abs_diff(d1),
        2 * N,
        3 * N,
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
    // scale. The cardinality test above would NOT notice eager body-fact
    // emission reintroduced at scale: a per-declaration body fact keeps
    // the count affine in the declaration count, so the second difference
    // stays zero. This count makes that regression fail.
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
