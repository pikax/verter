use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::canonical_path::canonicalize_path;

/// In-memory overlay store for active editor content.
///
/// When a file has an overlay, all reads return the overlay content
/// instead of the snapshot/disk content. The overlay owner (LSP, bundler)
/// is responsible for lifecycle management.
#[derive(Debug, Default)]
pub struct OverlayStore {
    entries: FxHashMap<String, Arc<str>>,
}

impl OverlayStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get overlay content for a file. Returns `None` if no overlay is set.
    pub fn get(&self, canonical_id: &str) -> Option<Arc<str>> {
        let key = canonicalize_path(canonical_id);
        self.entries.get(&key).cloned()
    }

    /// Set overlay content for a file. Returns `true` iff the stored
    /// content actually changed. R22 idempotency contract: callers
    /// gate on this return so byte-identical re-upserts do not bump
    /// downstream generation counters or fire reverse-graph
    /// invalidation.
    pub fn set(&mut self, canonical_id: String, source: Arc<str>) -> bool {
        let key = canonicalize_path(&canonical_id);
        match self.entries.get(&key) {
            Some(existing) if Arc::ptr_eq(existing, &source) || **existing == *source => false,
            _ => {
                self.entries.insert(key, source);
                true
            }
        }
    }

    /// Clear overlay for a file. Returns `true` if an overlay was removed.
    pub fn clear(&mut self, canonical_id: &str) -> bool {
        let key = canonicalize_path(canonical_id);
        self.entries.remove(&key).is_some()
    }

    /// Check if a file has an overlay set.
    pub fn has_overlay(&self, canonical_id: &str) -> bool {
        let key = canonicalize_path(canonical_id);
        self.entries.contains_key(&key)
    }

    /// Number of active overlays.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Approximate bytes retained by overlay content and canonical IDs.
    pub fn approx_bytes(&self) -> u64 {
        self.entries
            .iter()
            .map(|(canonical_id, source)| canonical_id.len() as u64 + source.len() as u64)
            .sum()
    }

    /// Whether there are no active overlays.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over the canonical IDs that currently have an overlay set.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(|s| s.as_str())
    }
}

#[cfg(test)]
#[path = "overlay_tests.rs"]
mod tests;
