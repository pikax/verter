#![deny(missing_docs)]
//! Host-owned LRU cache for the typeinfo scratch URIs synthesised
//! by [`crate::VerterHost::evaluate_type_expression_with_audit`].
//!
//! Lives at the host level so two requests evaluating the same
//! `(scope_canonical, expression, extra_imports)` triple share a
//! single scratch file. Cache is bypassed when the request sets
//! `cacheable = false`.
//!
//! Capacity defaults to 64; evictions are LRU by access time.
//! Backing store is a `Mutex<…>` so concurrent calls serialise on
//! the metadata only — actual evaluation work sits outside the
//! lock.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::semantic_query::SemanticNodeId;

/// Default cache capacity. Entries evict LRU above this bound.
pub const DEFAULT_CAPACITY: usize = 64;

/// One cache entry — pairs a scratch URI with its resolved semantic
/// node id and the access-time tick used by the LRU eviction sweep.
#[derive(Debug, Clone, Copy)]
struct Entry {
    node_id: SemanticNodeId,
    last_access: u64,
}

/// LRU cache. Internal state is a `HashMap` of URI → entry plus a
/// monotonic counter for last-access bookkeeping. The cache holds
/// `node_id` only — the synthesised scratch source is stored
/// indirectly via the host's normal upsert pipeline (the URI is the
/// canonical id of an upserted file, and re-resolution from the URI
/// hits the host's IndexedReady cache).
#[derive(Debug)]
pub(crate) struct ScratchCache {
    entries: HashMap<String, Entry>,
    capacity: usize,
    tick: AtomicU64,
}

impl ScratchCache {
    /// Construct a cache with the default capacity.
    pub fn with_default_capacity() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Construct a cache with an explicit capacity. `0` is treated as
    /// "do not cache" — every request hits cold synthesis.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity.max(1)),
            capacity,
            tick: AtomicU64::new(1),
        }
    }

    /// Lookup a URI. Bumps the entry's `last_access` tick on hit so
    /// the next eviction sweep keeps freshly-touched entries.
    pub fn get(&mut self, uri: &str) -> Option<SemanticNodeId> {
        if self.capacity == 0 {
            return None;
        }
        let now = self.tick.fetch_add(1, Ordering::Relaxed);
        let entry = self.entries.get_mut(uri)?;
        entry.last_access = now;
        Some(entry.node_id)
    }

    /// Insert a URI → node-id mapping. If insertion would exceed
    /// `capacity`, the entry with the smallest `last_access` tick is
    /// dropped FIRST so the new entry never lands on a full cache.
    /// Returns the URI that was evicted (if any) so callers can clean
    /// up the host-side scratch file.
    pub fn insert(&mut self, uri: String, node_id: SemanticNodeId) -> Option<String> {
        if self.capacity == 0 {
            return None;
        }
        let now = self.tick.fetch_add(1, Ordering::Relaxed);
        let evicted = if self.entries.len() >= self.capacity && !self.entries.contains_key(&uri) {
            // Find the entry with the oldest last_access tick.
            let mut oldest_uri: Option<String> = None;
            let mut oldest_tick = u64::MAX;
            for (k, e) in &self.entries {
                if e.last_access < oldest_tick {
                    oldest_tick = e.last_access;
                    oldest_uri = Some(k.clone());
                }
            }
            if let Some(ref evicted_uri) = oldest_uri {
                self.entries.remove(evicted_uri);
            }
            oldest_uri
        } else {
            None
        };
        self.entries.insert(
            uri,
            Entry {
                node_id,
                last_access: now,
            },
        );
        evicted
    }

    /// Drop the cached entry for `uri`. No-op if absent. Used by
    /// integration tests + the `evict_unreachable_artifacts` hook.
    #[allow(dead_code)]
    pub fn remove(&mut self, uri: &str) -> Option<SemanticNodeId> {
        self.entries.remove(uri).map(|e| e.node_id)
    }

    /// Number of cached entries.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
