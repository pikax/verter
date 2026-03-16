use rustc_hash::FxHashMap;
use std::sync::Arc;

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
        self.entries.get(canonical_id).cloned()
    }

    /// Set overlay content for a file.
    pub fn set(&mut self, canonical_id: String, source: Arc<str>) {
        self.entries.insert(canonical_id, source);
    }

    /// Clear overlay for a file. Returns `true` if an overlay was removed.
    pub fn clear(&mut self, canonical_id: &str) -> bool {
        self.entries.remove(canonical_id).is_some()
    }

    /// Check if a file has an overlay set.
    pub fn has_overlay(&self, canonical_id: &str) -> bool {
        self.entries.contains_key(canonical_id)
    }

    /// Number of active overlays.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no active overlays.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
#[path = "overlay_tests.rs"]
mod tests;
