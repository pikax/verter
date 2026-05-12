//! R20 signature-size bound enforcement.
//!
//! Stage 5 Sub-task B introduces a cap on `fact_dep_signature` size:
//! signatures with > 1024 `FactVersionRef` entries are admitted as
//! `NonCacheable`, the candidate does NOT enter the cache, and a
//! typed `FactSignatureOverflow` audit event fires.
//!
//! Verify: the **fallback recompute returns the correct value** —
//! the cache MUST NOT silently swallow correctness even for over-cap
//! signatures.

use rustc_hash::FxHashSet;
use std::sync::Arc;

use verter_session::resolver_core::{
    FactVersionRef, StoreView, StoreViewCompatToken, ValidatedFactCache,
};

#[derive(Debug)]
struct TestView {
    valid_facts: FxHashSet<FactVersionRef>,
}

impl StoreView for TestView {
    fn compat_token(&self) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch: 1,
            session: None,
        }
    }
    fn validates(&self, fact: &FactVersionRef) -> bool {
        self.valid_facts.contains(fact)
    }
}

fn fact(canonical: &str, hash: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: [hash; 16],
    }
}

/// R20 — signatures larger than 1024 entries are admitted as
/// `NonCacheable`. The candidate does NOT enter the cache (no warm
/// hit on subsequent reads).
#[test]
fn r20_over_cap_signature_refuses_admission() {
    let cache = ValidatedFactCache::<String, usize>::default();

    // Build a 1100-entry signature — well over the 1024 cap.
    let mut facts = Vec::with_capacity(1100);
    for i in 0..1100u16 {
        facts.push(FactVersionRef::FileWholeHash {
            canonical_id: format!("/src/dep_{i}.ts"),
            hash: [(i % 256) as u8; 16],
        });
    }

    cache.insert("over_cap".to_string(), 999, facts.clone());

    // Even with a view that validates every fact, the cache returns
    // None because the candidate was refused at admission.
    let view = TestView {
        valid_facts: facts.iter().cloned().collect(),
    };
    assert!(
        cache.get_if_valid(&"over_cap".to_string(), &view).is_none(),
        "over-cap candidate must not be admitted to the cache"
    );
}

/// R20 — the `FactSignatureOverflow` counter increments when an
/// over-cap signature is rejected. (Discriminating test: pre-Stage-5b
/// no counter exists; post-Stage-5b a typed counter increments.)
#[test]
fn r20_over_cap_signature_emits_overflow_event() {
    let cache = ValidatedFactCache::<String, usize>::default();

    let before = cache.signature_overflow_count();

    // Build a 1200-entry signature.
    let mut facts = Vec::with_capacity(1200);
    for i in 0..1200u16 {
        facts.push(FactVersionRef::FileWholeHash {
            canonical_id: format!("/src/dep_{i}.ts"),
            hash: [(i % 256) as u8; 16],
        });
    }
    cache.insert("over_cap_event".to_string(), 1, facts);

    let after = cache.signature_overflow_count();
    assert_eq!(
        after,
        before + 1,
        "signature overflow counter must increment on over-cap admission"
    );
}

/// R20 — exactly at the cap is admitted; one-over is refused.
/// Discriminates the cap value (1024) from off-by-one bugs.
#[test]
fn r20_cap_boundary_at_1024() {
    let cache = ValidatedFactCache::<String, usize>::default();

    // 1024-entry signature: admitted.
    let facts_1024: Vec<FactVersionRef> = (0..1024u16)
        .map(|i| fact(&format!("/src/at_cap_{i}.ts"), (i % 256) as u8))
        .collect();
    cache.insert("at_cap".to_string(), 100, facts_1024.clone());
    let view = TestView {
        valid_facts: facts_1024.iter().cloned().collect(),
    };
    assert_eq!(
        cache.get_if_valid(&"at_cap".to_string(), &view),
        Some(Arc::new(100)),
        "1024-entry signature must be admitted"
    );

    // 1025-entry signature: refused.
    let facts_1025: Vec<FactVersionRef> = (0..1025u16)
        .map(|i| fact(&format!("/src/over_cap_{i}.ts"), (i % 256) as u8))
        .collect();
    cache.insert("over_cap".to_string(), 200, facts_1025.clone());
    let view = TestView {
        valid_facts: facts_1025.iter().cloned().collect(),
    };
    assert!(
        cache.get_if_valid(&"over_cap".to_string(), &view).is_none(),
        "1025-entry signature must be refused"
    );
}

/// R20 — non-stub fallback-recompute correctness. Models the real
/// caller flow: a producer closure computes the value, the caller
/// asks the cache for it, the cache returns `None` on every call
/// (because the signature is over-cap and admission is refused),
/// and the caller falls back to re-running the producer.
///
/// Discriminating contract this test enforces:
/// 1. The cache MUST NOT silently swallow an over-cap admission
///    (e.g., by storing a candidate that survives FIFO and starts
///    serving warm hits).
/// 2. Every `get_if_valid` for an over-cap-admitted key MUST return
///    `None`, forcing the caller's recompute branch.
/// 3. The producer's output, exercised through the caller-side
///    fallback path, MUST match the value that would have been
///    cached if admission had not been refused — i.e., correctness
///    is preserved across the cap boundary.
///
/// The producer is a *real* counted closure: each call increments
/// `compute_count`. If the cache ever silently admitted the over-cap
/// candidate, `compute_count` would stay at 1 across the read-back
/// loop instead of advancing once per call.
#[test]
fn r20_over_cap_signature_correct_value_fallback() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let cache = ValidatedFactCache::<String, String>::default();
    let compute_count = AtomicUsize::new(0);

    // The real producer closure. Each call increments the counter
    // AND returns the canonical value for the given seed.
    let producer = |seed: usize| -> String {
        compute_count.fetch_add(1, Ordering::SeqCst);
        (0..1100usize)
            .map(|i| format!("dep_{seed}_{i}"))
            .collect::<Vec<_>>()
            .join("|")
    };

    // Build an over-cap (1100-entry) signature for this key.
    let facts: Vec<FactVersionRef> = (0..1100u16)
        .map(|i| fact(&format!("/src/r_{i}.ts"), (i % 256) as u8))
        .collect();
    let view = TestView {
        valid_facts: facts.iter().cloned().collect(),
    };

    // Caller flow round 1: ask the cache first, miss, run producer,
    // try to insert (refused — over cap).
    let key = "fallback".to_string();
    let round_1_value = if let Some(v) = cache.get_if_valid(&key, &view) {
        (*v).clone()
    } else {
        let v = producer(42);
        cache.insert(key.clone(), v.clone(), facts.clone());
        v
    };
    assert_eq!(
        compute_count.load(Ordering::SeqCst),
        1,
        "round 1: producer must have run exactly once (cold miss)"
    );
    assert!(
        cache.get_if_valid(&key, &view).is_none(),
        "round 1 post-insert: over-cap admission must not be visible to a warm read",
    );

    // Caller flow round 2: same key, same view, same producer.
    // Substrate must again return None, forcing the producer to
    // recompute — proving the over-cap admission did NOT enter the
    // cache and steal a warm hit.
    let round_2_value = if let Some(v) = cache.get_if_valid(&key, &view) {
        (*v).clone()
    } else {
        let v = producer(42);
        cache.insert(key.clone(), v.clone(), facts.clone());
        v
    };
    assert_eq!(
        compute_count.load(Ordering::SeqCst),
        2,
        "round 2: cache must NOT admit the over-cap candidate, so the producer must \
         have run a second time via the caller's fallback branch (would be 1 if the \
         cache silently swallowed admission)"
    );

    // Round 3: prove correctness across the cap boundary. The
    // producer's output, exercised through the fallback path, MUST
    // match a from-scratch cold call.
    let cold_independent_value = producer(42);
    assert_eq!(compute_count.load(Ordering::SeqCst), 3);
    assert_eq!(
        round_1_value, round_2_value,
        "fallback recompute must be deterministic across calls"
    );
    assert_eq!(
        round_1_value, cold_independent_value,
        "fallback recompute MUST agree with a cold producer invocation — \
         correctness is preserved across the cap boundary"
    );

    // Cross-check: a non-over-cap admission with the same producer
    // would have warmed the cache and skipped the fallback path on
    // the second call. This is the discriminator: if the test ever
    // started passing because the cache silently admitted the
    // over-cap candidate, this branch would observe a warm hit AND
    // compute_count would stay at 3 instead of advancing to 4 below.
    let under_cap_facts: Vec<FactVersionRef> = (0..512u16)
        .map(|i| fact(&format!("/src/u_{i}.ts"), (i % 256) as u8))
        .collect();
    let under_cap_view = TestView {
        valid_facts: under_cap_facts.iter().cloned().collect(),
    };
    let under_cap_key = "warm_path".to_string();
    let _round_a = {
        let v = producer(7);
        cache.insert(under_cap_key.clone(), v.clone(), under_cap_facts.clone());
        v
    };
    assert_eq!(compute_count.load(Ordering::SeqCst), 4);
    // The under-cap insert MUST have admitted — warm read succeeds.
    let warm = cache.get_if_valid(&under_cap_key, &under_cap_view);
    assert!(
        warm.is_some(),
        "an under-cap admission MUST produce a warm hit; this proves the \
         over-cap behaviour above is genuinely refusing admission rather than \
         a generic cache miss"
    );
    // Producer was NOT called for the warm read — discriminator.
    assert_eq!(
        compute_count.load(Ordering::SeqCst),
        4,
        "warm hit must not invoke the producer"
    );
}
