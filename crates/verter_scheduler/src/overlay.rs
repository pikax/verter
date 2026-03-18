//! Concurrent overlay map for active editor buffers.
//!
//! The [`OverlayMap`] uses `DashMap` for lock-free concurrent access.
//! It is shared between [`SourceLoader`](crate::source_loader) (reads)
//! and the scheduler (writes from did_open/did_change).

use std::sync::Arc;

use dashmap::DashMap;

/// Concurrent overlay map for editor buffer content.
///
/// When a file has an overlay, source loading returns the overlay
/// content instead of disk content. Thread-safe via `DashMap`.
#[derive(Default)]
pub struct OverlayMap {
    inner: DashMap<String, Arc<str>>,
}

impl OverlayMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get overlay content for a file.
    pub fn get(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.inner.get(canonical_id).map(|v| Arc::clone(v.value()))
    }

    /// Set overlay content for a file.
    pub fn set(&self, canonical_id: String, source: Arc<str>) {
        self.inner.insert(canonical_id, source);
    }

    /// Clear overlay for a file. Returns `true` if an overlay was removed.
    pub fn clear(&self, canonical_id: &str) -> bool {
        self.inner.remove(canonical_id).is_some()
    }

    /// Check if a file has an overlay.
    pub fn has(&self, canonical_id: &str) -> bool {
        self.inner.contains_key(canonical_id)
    }

    /// Number of active overlays.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether there are no active overlays.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_set_and_get() {
        let map = OverlayMap::new();
        map.set("/a.vue".to_string(), Arc::from("hello"));
        assert_eq!(&*map.get("/a.vue").unwrap(), "hello");
    }

    #[test]
    fn overlay_get_missing_returns_none() {
        let map = OverlayMap::new();
        assert!(map.get("/missing.vue").is_none());
    }

    #[test]
    fn overlay_clear() {
        let map = OverlayMap::new();
        map.set("/a.vue".to_string(), Arc::from("hello"));
        assert!(map.clear("/a.vue"));
        assert!(map.get("/a.vue").is_none());
        // Second clear returns false
        assert!(!map.clear("/a.vue"));
    }

    #[test]
    fn overlay_has() {
        let map = OverlayMap::new();
        assert!(!map.has("/a.vue"));
        map.set("/a.vue".to_string(), Arc::from("hello"));
        assert!(map.has("/a.vue"));
    }

    #[test]
    fn overlay_len() {
        let map = OverlayMap::new();
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
        map.set("/a.vue".to_string(), Arc::from("a"));
        map.set("/b.vue".to_string(), Arc::from("b"));
        assert_eq!(map.len(), 2);
        assert!(!map.is_empty());
    }

    #[test]
    fn overlay_concurrent_access() {
        use std::sync::Arc as StdArc;
        let map = StdArc::new(OverlayMap::new());

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let m = StdArc::clone(&map);
                std::thread::spawn(move || {
                    let id = format!("/file{i}.vue");
                    let content = format!("content {i}");
                    m.set(id.clone(), Arc::from(content.as_str()));
                    assert!(m.has(&id));
                    let got = m.get(&id).unwrap();
                    assert_eq!(&*got, content.as_str());
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(map.len(), 10);
    }
}
