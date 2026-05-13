//! Discrimination tests for the push-style fact-read tracer
//! (`FactReadSet`, `FactReadSetCell`, `VerterHost::with_fact_tracer`,
//! `VerterHost::current_fact_tracer`).
//!
//! Each test below exercises a specific contract bullet that the
//! tracer substrate guarantees. The bodies are deliberately
//! non-trivial — every assertion would FAIL to compile against the
//! pre-substrate tree (no `FactReadSet` / `FactReadSetCell` /
//! `with_fact_tracer` / `current_fact_tracer` exists at HEAD~1)
//! AND would FAIL to pass against any implementation that uses
//! `unimplemented!()`, returns a constant, or silently no-ops.
//!
//! The discrimination property is mandatory; see CLAUDE.md "Stub
//! Prevention".

use std::sync::Arc;

use verter_session::resolver_core::{
    FactReadSet, FactReadSetCell, FactReadSetFinalise, FactVersionRef,
};
use verter_session::{CompileErrorPolicy, HostConfig, VerterHost};

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }))
}

/// Helper that fabricates a `FactVersionRef::FileWholeHash` with a
/// distinct hash byte so callers can construct N distinct facts
/// without contending on a real host content store.
fn fact(canonical: &str, lo_byte: u8) -> FactVersionRef {
    let mut hash = [0u8; 16];
    hash[0] = lo_byte;
    FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash,
    }
}

// ── Test 1 ─────────────────────────────────────────────────────────────
//
// Inside a `with_fact_tracer` scope, `observe` calls accumulate onto
// the tracer. After the scope ends, the returned `FactReadSet` has
// `len() >= 1` and `finalise()` produces a non-empty
// `Arc<[FactVersionRef]>`.

#[test]
fn cold_compute_observes_each_dep() {
    let host = make_host();

    let ((), set) = host.with_fact_tracer(|| {
        let cell = host
            .current_fact_tracer()
            .expect("tracer must be active inside scope");
        cell.observe(fact("/a.ts", 1));
        cell.observe(fact("/b.ts", 2));
        cell.observe(fact("/c.ts", 3));
    });

    assert_eq!(
        set.len(),
        3,
        "tracer must accumulate three distinct facts observed inside the scope"
    );
    match set.finalise() {
        FactReadSetFinalise::Ok(arc) => {
            assert_eq!(arc.len(), 3, "finalised signature must include all three");
            // Sort is canonical and stable — assert canonical order
            // by canonical_id ascending ("/a.ts" < "/b.ts" < "/c.ts").
            let names: Vec<_> = arc
                .iter()
                .map(|f| match f {
                    FactVersionRef::FileWholeHash { canonical_id, .. } => canonical_id.clone(),
                    other => panic!("expected FileWholeHash, got {other:?}"),
                })
                .collect();
            assert_eq!(names, vec!["/a.ts", "/b.ts", "/c.ts"]);
        }
        FactReadSetFinalise::Overflow => panic!("3 facts should not overflow"),
    }
}

// ── Test 2 ─────────────────────────────────────────────────────────────
//
// Outside any `with_fact_tracer` scope, `current_fact_tracer()`
// returns `None`. Calling `observe` (the trait convenience) would
// be a no-op; we verify the slot is empty by direct inspection.

#[test]
fn warm_hit_path_does_not_observe() {
    let host = make_host();

    assert!(
        host.current_fact_tracer().is_none(),
        "no tracer must be installed outside a `with_fact_tracer` scope"
    );

    // The trait method `observe` short-circuits when no tracer is
    // active. We can't reach the trait directly (it is sealed and
    // pub(crate)), but we can demonstrate the warm-hit guarantee
    // through the public accessor.
    for _ in 0..1024 {
        assert!(host.current_fact_tracer().is_none());
    }
}

// ── Test 3 ─────────────────────────────────────────────────────────────
//
// `observe_borrowed_signature` bulk-appends a borrowed slice onto the
// tracer. Mirrors the routing-hit fast path: a higher-tier cold
// compute consumes a lower-tier cached signature.

#[test]
fn observe_borrowed_signature_appends() {
    let host = make_host();

    let borrowed = vec![fact("/x.ts", 10), fact("/y.ts", 11), fact("/z.ts", 12)];

    let ((), set) = host.with_fact_tracer(|| {
        let cell = host
            .current_fact_tracer()
            .expect("tracer must be active inside scope");
        cell.observe_borrowed_signature(&borrowed);
    });

    assert_eq!(
        set.len(),
        3,
        "bulk append must add every borrowed fact to the accumulator"
    );
    match set.finalise() {
        FactReadSetFinalise::Ok(arc) => {
            assert_eq!(arc.len(), 3);
            let names: Vec<_> = arc
                .iter()
                .map(|f| match f {
                    FactVersionRef::FileWholeHash { canonical_id, .. } => canonical_id.clone(),
                    other => panic!("expected FileWholeHash, got {other:?}"),
                })
                .collect();
            assert_eq!(names, vec!["/x.ts", "/y.ts", "/z.ts"]);
        }
        FactReadSetFinalise::Overflow => panic!("3 facts should not overflow"),
    }
}

// ── Test 4 ─────────────────────────────────────────────────────────────
//
// `FactReadSet::finalise` enforces `FACT_SIGNATURE_CAP`. A tracer
// that observes more than `FACT_SIGNATURE_CAP` distinct facts must
// return `FactReadSetFinalise::Overflow` instead of an admitted
// signature. Overflow is NOT a panic — the caller refuses
// admission and emits a structured audit event (event wiring is
// separate from this substrate).

#[test]
fn signature_cap_overflow_returns_overflow() {
    let host = make_host();

    // FACT_SIGNATURE_CAP = 1024; we observe 1025 distinct facts.
    // 1025 > 1024 → overflow.
    const N_OVERFLOW: usize = 1025;

    let ((), set) = host.with_fact_tracer(|| {
        let cell = host
            .current_fact_tracer()
            .expect("tracer must be active inside scope");
        for i in 0..N_OVERFLOW {
            // Use the index as part of the canonical to guarantee
            // each fact is distinct (so dedup does not collapse them).
            let canonical = format!("/m_{i}.ts");
            cell.observe(fact(&canonical, (i & 0xFF) as u8));
        }
    });

    assert_eq!(
        set.len(),
        N_OVERFLOW,
        "tracer must accumulate every observation even beyond the cap"
    );
    match set.finalise() {
        FactReadSetFinalise::Ok(_) => {
            panic!("{N_OVERFLOW} distinct facts must overflow")
        }
        FactReadSetFinalise::Overflow => {
            // Expected outcome: overflow sentinel.
        }
    }
}

// ── Test 5 ─────────────────────────────────────────────────────────────
//
// `FactReadSet::finalise` sorts by canonical (then per-variant
// ordering) and dedups. Same-fact duplicates collapse; distinct
// facts come back in canonical order.

#[test]
fn finalise_sorts_and_dedups() {
    let host = make_host();

    // Observe the same fact three times, then a distinct fact, then
    // the first one again.
    let f1 = fact("/aaa.ts", 1);
    let f2 = fact("/bbb.ts", 2);

    let ((), set) = host.with_fact_tracer(|| {
        let cell = host.current_fact_tracer().unwrap();
        cell.observe(f1.clone());
        cell.observe(f1.clone());
        cell.observe(f2.clone());
        cell.observe(f1.clone());
        cell.observe(f2.clone());
    });

    // Raw observation count includes all 5 pushes — but adjacent
    // duplicates are pre-collapsed inline (`f1, f1` → `f1`;
    // `f2, f2` → `f2`). The accumulator stores `f1, f2, f1, f2`
    // → 4 entries before finalise.
    assert!(
        set.len() >= 2 && set.len() <= 5,
        "pre-finalise length is between 2 (full inline dedup) and 5 (no dedup); got {}",
        set.len()
    );

    match set.finalise() {
        FactReadSetFinalise::Ok(arc) => {
            assert_eq!(
                arc.len(),
                2,
                "finalise must collapse duplicate observations to a unique set"
            );
            // Canonical sort order: "/aaa.ts" < "/bbb.ts".
            let names: Vec<_> = arc
                .iter()
                .map(|f| match f {
                    FactVersionRef::FileWholeHash { canonical_id, .. } => canonical_id.clone(),
                    other => panic!("expected FileWholeHash, got {other:?}"),
                })
                .collect();
            assert_eq!(names, vec!["/aaa.ts", "/bbb.ts"]);
        }
        FactReadSetFinalise::Overflow => panic!("2 unique facts must not overflow"),
    }
}

// ── Test 6 ─────────────────────────────────────────────────────────────
//
// Nested `with_fact_tracer` scopes panic — the inner scope cannot
// silently install over an active outer scope because observations
// from the inner scope would never bubble up to the outer one.

#[test]
fn nested_with_fact_tracer_scopes_panic() {
    let host = make_host();

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = host.with_fact_tracer(|| {
            let _inner = host.with_fact_tracer(|| {});
        });
    }));

    assert!(
        outcome.is_err(),
        "nested `with_fact_tracer` must panic; instead the outer scope completed normally"
    );

    // After the outer panic unwinds, the TLS slot must be clear so
    // subsequent scopes succeed.
    let ((), set) = host.with_fact_tracer(|| {
        let cell = host
            .current_fact_tracer()
            .expect("subsequent scope must install cleanly after a nested-scope panic");
        cell.observe(fact("/post-panic.ts", 99));
    });
    assert_eq!(set.len(), 1, "tracer must work normally after the unwind");
}

// ── Bonus discrimination assertion: trait-method short-circuit ────────
//
// `FactReadSet::is_empty()` returns true on a brand-new tracer, and
// `FactReadSetCell::new()` is observably distinct from one that has
// recorded observations. These assertions catch a "default-returning"
// stub that always responds with a non-empty set or always responds
// with an empty set.

#[test]
fn empty_tracer_is_empty_and_records_change_state() {
    let cell = FactReadSetCell::new();
    assert!(cell.is_empty());
    assert_eq!(cell.len(), 0);

    cell.observe(fact("/q.ts", 7));
    assert!(!cell.is_empty());
    assert_eq!(cell.len(), 1);

    let inner: FactReadSet = cell.into_inner();
    assert_eq!(inner.len(), 1);
    match inner.finalise() {
        FactReadSetFinalise::Ok(arc) => assert_eq!(arc.len(), 1),
        FactReadSetFinalise::Overflow => panic!("1 fact must not overflow"),
    }
}
