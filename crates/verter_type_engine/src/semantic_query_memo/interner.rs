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
//!
//! **Storage charge.** A `Weak` keeps its allocation's block allocated
//! after the last strong holder drops (only the elements are dropped), so
//! an interned rail's block lives exactly as long as its entry here, never
//! shorter. The interner therefore charges, pinned against its store's
//! account, every entry's block, every bucket's backing and its tables'
//! buckets, settled as entries are added and pruned; a holder of an
//! interned rail pays only for the names its elements hold
//! ([`interned_rail_heap_bytes`]).

use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use crate::semantic_query::{DepSignature, DepVersion};

/// One interned rail's element.
type RailEntry = (Arc<str>, DepVersion);

/// `Weak` view of an interned `DepSignature` payload — see
/// [`DepSignatureInterner`].
type DepSignatureWeak = Weak<[RailEntry]>;

/// Shards the interner's buckets are spread over.
const SHARDS: usize = 16;

/// Granule the interner's storage is charged in.
const STORAGE_GRANULE: usize = 4 * 1024;

/// Bytes one bucket of a shard's table holds: its hash, its entry list
/// header and its control byte.
const TABLE_BUCKET_BYTES: usize = std::mem::size_of::<(u64, Vec<DepSignatureWeak>)>() + 1;

pub const SWEEP_INTERVAL: u64 = 1024;

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
/// dead `Weak` on next lookup and prunes it. `sweep()` reclaims empty
/// buckets and dead entries everywhere.
///
/// **Bucketing key:** `u64` content hash via `FxHash` over the
/// canonicalised payload. Collisions are tolerated — within a bucket
/// the `intern` path performs a content equality check before
/// returning the existing Arc.
pub struct DepSignatureInterner {
    shards: [Mutex<FxHashMap<u64, Vec<DepSignatureWeak>>>; SHARDS],
    storage: Mutex<Storage>,
    /// Counter-based auto-sweep trigger. Incremented on
    /// every successful intern; sweep runs when the counter hits
    /// `SWEEP_INTERVAL`. Cheap O(buckets) walk; off the hot path.
    inserts_since_sweep: std::sync::atomic::AtomicU64,
}

/// The interner's pinned storage charge.
struct Storage {
    account: Arc<verter_session_query::retention::SemanticRetentionAccount>,
    pin: Option<verter_session_query::retention::granular::GranularPin>,
}

impl Storage {
    fn settle(&mut self, added: usize, freed: usize) {
        if added == freed {
            return;
        }
        let account = &self.account;
        let pin = self.pin.get_or_insert_with(|| {
            verter_session_query::retention::granular::GranularPin::new(account, STORAGE_GRANULE)
        });
        if added > freed {
            pin.add(added - freed);
        } else {
            pin.sub(freed - added);
        }
    }
}

/// The block an entry's allocation holds, read from its handle's slice
/// length — alive or not.
fn entry_block_bytes(entry: &DepSignatureWeak) -> usize {
    verter_session_query::retention::allocation::arc_block_bytes(
        entry.as_ptr().len() * std::mem::size_of::<RailEntry>(),
    )
}

/// The backing one bucket's entry list holds.
fn bucket_list_bytes(bucket: &Vec<DepSignatureWeak>) -> usize {
    verter_session_query::retention::allocation::vec_bytes(bucket)
}

/// Bytes one shard's storage holds: its table buckets, every bucket's
/// entry list and every entry's block.
fn shard_bytes(shard: &FxHashMap<u64, Vec<DepSignatureWeak>>) -> usize {
    shard.capacity() * TABLE_BUCKET_BYTES
        + shard
            .values()
            .map(|bucket| {
                bucket_list_bytes(bucket) + bucket.iter().map(entry_block_bytes).sum::<usize>()
            })
            .sum::<usize>()
}

/// The heap a holder of an interned rail pays for: the names its elements
/// hold. The rail's block is the interner's (see the module docs).
pub(crate) fn interned_rail_heap_bytes(
    rail: &DepSignature,
    aliases: &mut verter_session_query::retention::allocation::AliasSet,
) -> usize {
    rail.iter()
        .map(|(canonical, _)| {
            verter_session_query::retention::allocation::arc_str_bytes(canonical, aliases)
        })
        .sum()
}

/// Whether `rail` is an interned rail, whose block the interner charges.
/// Only the interner downgrades a rail, and it keeps that `Weak` for as
/// long as any strong holder lives.
pub(crate) fn is_interned_rail(rail: &DepSignature) -> bool {
    Arc::weak_count(rail) > 0
}

impl Default for DepSignatureInterner {
    fn default() -> Self {
        Self::charged(&verter_session_query::retention::SemanticRetentionAccount::process_local())
    }
}

impl std::fmt::Debug for DepSignatureInterner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DepSignatureInterner")
            .field("buckets", &self.bucket_count())
            .finish_non_exhaustive()
    }
}

impl DepSignatureInterner {
    /// Construct a fresh interner with no buckets, charging the
    /// process-local account.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh interner whose storage is charged to `account`.
    #[must_use]
    pub fn charged(
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self {
            shards: std::array::from_fn(|_| Mutex::new(FxHashMap::default())),
            storage: Mutex::new(Storage {
                account: Arc::clone(account),
                pin: None,
            }),
            inserts_since_sweep: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Intern `payload`, returning a shared `Arc` whose pointer
    /// equality (`Arc::ptr_eq`) matches every other equivalent intern.
    ///
    /// Equivalent dep_signatures are normalised before lookup: pairs
    /// are sorted by `(canonical, version)` and adjacent duplicates
    /// removed. This ensures `intern([(a, v1), (b, v2)])` returns the
    /// same `Arc` as `intern([(b, v2), (a, v1), (a, v1)])`. An empty
    /// payload interns to the process-static empty rail.
    pub fn intern(&self, payload: &[RailEntry]) -> DepSignature {
        // Normalise: sort + dedup so equivalent content collapses.
        let mut normalised: Vec<RailEntry> = payload.to_vec();
        normalised.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()).then_with(|| a.1.cmp(&b.1)));
        normalised.dedup();
        // Every empty rail is the one process-static empty rail.
        if normalised.is_empty() {
            return super::empty_signature();
        }
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

        let mut storage = self.storage.lock();
        let mut shard = self.shards[(hash as usize) % SHARDS].lock();
        let table_before = shard.capacity() * TABLE_BUCKET_BYTES;
        let bucket = shard.entry(hash).or_default();
        let list_before = bucket_list_bytes(bucket);
        // Prune dead Weaks while scanning: their blocks are freed with them.
        let mut freed = 0;
        bucket.retain(|w| {
            let live = w.strong_count() > 0;
            if !live {
                freed += entry_block_bytes(w);
            }
            live
        });
        let hit = bucket
            .iter()
            .find_map(|w| w.upgrade().filter(|arc| arc.iter().eq(normalised.iter())));
        let (rail, added_block) = match hit {
            Some(arc) => {
                crate::request_observers::record_dep_signature_intern_hit();
                (arc, 0)
            }
            None => {
                // Miss: insert a fresh Arc and downgrade for the bucket.
                let fresh: DepSignature = Arc::from(normalised.into_boxed_slice());
                let entry = Arc::downgrade(&fresh);
                let block = entry_block_bytes(&entry);
                bucket.push(entry);
                (fresh, block)
            }
        };
        let list_after = bucket_list_bytes(bucket);
        let table_after = shard.capacity() * TABLE_BUCKET_BYTES;
        drop(shard);
        storage.settle(
            added_block + list_after + table_after,
            freed + list_before + table_before,
        );
        drop(storage);

        // Auto-sweep trigger. cheap O(buckets) walk every
        // SWEEP_INTERVAL inserts.
        let n = self
            .inserts_since_sweep
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        if n.is_multiple_of(SWEEP_INTERVAL) {
            self.sweep();
        }

        rail
    }

    /// Intern a single `(canonical, version)` pair. Convenience for
    /// call sites that build dep_signatures incrementally.
    pub fn intern_canonical(&self, canonical: Arc<str>, version: DepVersion) -> DepSignature {
        verter_debug_assert!(
            !canonical.as_ref().is_empty(),
            "intern_canonical: canonical id must be non-empty"
        );
        self.intern(&[(canonical, version)])
    }

    /// Periodic sweep — removes empty buckets and dead `Weak`s, and
    /// gives back the backing a drained table no longer needs.
    /// Called by the host's idle-time cleanup pipeline AND
    /// auto-triggered every `SWEEP_INTERVAL` inserts.
    ///
    /// O(buckets) where buckets = distinct content hashes seen so
    /// far. Cheap relative to a full warm-cache sweep because
    /// dep_signature content is highly redundant in practice.
    pub fn sweep(&self) {
        let mut storage = self.storage.lock();
        for shard in &self.shards {
            let mut shard = shard.lock();
            let before = shard_bytes(&shard);
            shard.retain(|_, bucket| {
                bucket.retain(|w| w.strong_count() > 0);
                !bucket.is_empty()
            });
            let len = shard.len();
            if len == 0 {
                *shard = FxHashMap::default();
            } else if len * 4 <= shard.capacity() {
                shard.shrink_to(len * 2);
            }
            let after = shard_bytes(&shard);
            storage.settle(after, before);
        }
    }

    /// Test/diagnostic: number of distinct hash buckets currently
    /// stored. May include empty buckets that have not yet been
    /// reaped by `sweep`.
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.shards.iter().map(|shard| shard.lock().len()).sum()
    }

    /// Test/diagnostic: number of distinct interned dep_signatures
    /// (i.e., total live `Weak`s across every bucket).
    #[must_use]
    pub fn live_signature_count(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| {
                shard
                    .lock()
                    .values()
                    .map(|bucket| bucket.iter().filter(|w| w.strong_count() > 0).count())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Bytes the interner's own storage is charged.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn storage_charge_bytes(&self) -> usize {
        self.storage.lock().pin.as_ref().map_or(
            0,
            verter_session_query::retention::granular::GranularPin::charged,
        )
    }
}
