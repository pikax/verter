//! Content-hash bucketed `Weak<...>` interner for `DepSignature`.
//!
//! Equivalent dep_signatures (same `(canonical, version)` set after
//! sort+dedup) share a single `Arc<[(...)]>` so the reverse-index
//! `Arc::ptr_eq` discrimination matches "our entry" vs "fresh
//! post-publish write" correctly, and memory pressure stays bounded.
//!
//! Liveness is via `Weak<...>`: the interner holds `Weak` references
//! only. When the last strong `Arc` is dropped, `intern` notices the
//! dead `Weak` on next lookup and prunes it. `sweep()` can be called
//! periodically to reclaim empty buckets.

use std::sync::Arc;

use dashmap::DashMap;

use crate::semantic_query::DepSignature;

/// Content-hash bucketed `Weak<...>` interner for `DepSignature`.
/// Equivalent dep_signatures (same `(canonical, version)` set after
/// sort+dedup) share a single `Arc<[(...)]>` so:
///
/// 1. The reverse-index `Arc::ptr_eq` discrimination matches
///    "our entry" vs "fresh post-publish write" correctly.
/// 2. Memory pressure stays bounded — N publishes of the same dep
///    closure store one allocation, not N.
///
/// **Liveness via `Weak<...>`:** the interner holds `Weak` references
/// only. When the last strong `Arc` is dropped, `intern` notices the
/// dead `Weak` on next lookup and prunes it. `sweep()` can be called
/// periodically to reclaim empty buckets.
///
/// **Bucketing key:** `u64` content hash via `FxHash` over the
/// canonicalised payload. Collisions are tolerated — within a bucket
/// the `intern` path performs a content equality check before
/// returning the existing Arc.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct DepSignatureInterner {
    table: DashMap<u64, Vec<DepSignatureWeak>>,
    /// Counter-based auto-sweep trigger. Incremented on
    /// every successful intern; sweep runs when the counter hits
    /// `SWEEP_INTERVAL`. Cheap O(buckets) walk; off the hot path.
    inserts_since_sweep: std::sync::atomic::AtomicU64,
}

/// `Weak` view of an interned `DepSignature` payload — see
/// [`DepSignatureInterner`].
#[allow(dead_code)]
type DepSignatureWeak = std::sync::Weak<[(Arc<str>, crate::semantic_query::DepVersion)]>;

#[allow(dead_code)]
pub(super) const SWEEP_INTERVAL: u64 = 1024;

#[allow(dead_code)]
impl DepSignatureInterner {
    /// Construct a fresh interner with no buckets.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `payload`, returning a shared `Arc` whose pointer
    /// equality (`Arc::ptr_eq`) matches every other equivalent intern.
    ///
    /// Equivalent dep_signatures are normalised before lookup: pairs
    /// are sorted by `(canonical, version)` and adjacent duplicates
    /// removed. This ensures `intern([(a, v1), (b, v2)])` returns the
    /// same `Arc` as `intern([(b, v2), (a, v1), (a, v1)])`.
    pub fn intern(
        &self,
        payload: &[(Arc<str>, crate::semantic_query::DepVersion)],
    ) -> DepSignature {
        // Normalise: sort + dedup so equivalent content collapses.
        let mut normalised: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = payload.to_vec();
        normalised.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()).then_with(|| a.1.cmp(&b.1)));
        normalised.dedup();
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = rustc_hash::FxHasher::default();
            normalised.len().hash(&mut hasher);
            for (canonical, version) in &normalised {
                canonical.hash(&mut hasher);
                version.hash(&mut hasher);
            }
            hasher.finish()
        };

        let mut bucket = self.table.entry(hash).or_default();
        // Prune dead Weaks while scanning.
        bucket.retain(|w| w.strong_count() > 0);
        for w in bucket.iter() {
            if let Some(arc) = w.upgrade() {
                if arc.iter().eq(normalised.iter()) {
                    crate::host_manage::record_dep_signature_intern_hit();
                    return Arc::clone(&arc) as DepSignature;
                }
            }
        }
        // Miss: insert a fresh Arc and downgrade for the bucket.
        let fresh: Arc<[(Arc<str>, crate::semantic_query::DepVersion)]> =
            Arc::from(normalised.into_boxed_slice());
        bucket.push(Arc::downgrade(&fresh));
        drop(bucket);

        // Auto-sweep trigger. cheap O(buckets) walk every
        // SWEEP_INTERVAL inserts.
        let n = self
            .inserts_since_sweep
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        if n.is_multiple_of(SWEEP_INTERVAL) {
            self.sweep();
        }

        fresh
    }

    /// Intern a single `(canonical, version)` pair. Convenience for
    /// call sites that build dep_signatures incrementally.
    pub fn intern_canonical(
        &self,
        canonical: Arc<str>,
        version: crate::semantic_query::DepVersion,
    ) -> DepSignature {
        debug_assert!(
            !canonical.as_ref().is_empty(),
            "intern_canonical: canonical id must be non-empty"
        );
        self.intern(&[(canonical, version)])
    }

    /// Periodic sweep — removes empty buckets and dead `Weak`s.
    /// Called by the host's idle-time cleanup pipeline AND
    /// auto-triggered every `SWEEP_INTERVAL` inserts.
    ///
    /// O(buckets) where buckets = distinct content hashes seen so
    /// far. Cheap relative to a full warm-cache sweep because
    /// dep_signature content is highly redundant in practice.
    pub fn sweep(&self) {
        self.table.retain(|_, bucket| {
            bucket.retain(|w| w.strong_count() > 0);
            !bucket.is_empty()
        });
    }

    /// Test/diagnostic: number of distinct hash buckets currently
    /// stored. May include empty buckets that have not yet been
    /// reaped by `sweep`.
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.table.len()
    }

    /// Test/diagnostic: number of distinct interned dep_signatures
    /// (i.e., total live `Weak`s across every bucket).
    #[must_use]
    pub fn live_signature_count(&self) -> usize {
        self.table
            .iter()
            .map(|entry| {
                entry
                    .value()
                    .iter()
                    .filter(|w| w.strong_count() > 0)
                    .count()
            })
            .sum()
    }
}
