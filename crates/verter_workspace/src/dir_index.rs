use crate::path_matches_prefix;
use rustc_hash::FxHashMap;
// `FxHashSet` only backs the native-only `DirListing::basenames` field; the
// WASM build excludes that field, so the import would otherwise be unused.
#[cfg(not(target_arch = "wasm32"))]
use rustc_hash::FxHashSet;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirIndexLookup {
    Hit(bool),
    Dirty,
    Unindexed,
}

#[derive(Debug, Default)]
pub struct DirIndex {
    entries: FxHashMap<String, DirListing>,
}

#[derive(Debug, Default)]
struct DirListing {
    #[cfg(not(target_arch = "wasm32"))]
    basenames: FxHashSet<String>,
    dirty: bool,
}

impl DirIndex {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn file_exists(&self, canonical_id: &str) -> Option<bool> {
        match self.lookup(canonical_id) {
            DirIndexLookup::Hit(exists) => Some(exists),
            DirIndexLookup::Dirty | DirIndexLookup::Unindexed => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn lookup(&self, canonical_id: &str) -> DirIndexLookup {
        let Some((dir, basename)) = split_parent_basename(canonical_id) else {
            return DirIndexLookup::Unindexed;
        };
        let Some(listing) = self.entries.get(dir) else {
            return DirIndexLookup::Unindexed;
        };
        if listing.dirty {
            return DirIndexLookup::Dirty;
        }
        DirIndexLookup::Hit(listing.basenames.contains(basename))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn refresh(&mut self, dir: &str, basenames: Vec<String>) {
        self.entries.insert(
            dir.to_string(),
            DirListing {
                basenames: basenames.into_iter().collect(),
                dirty: false,
            },
        );
    }

    pub fn mark_dirty(&mut self, dir: &str) {
        if let Some(listing) = self.entries.get_mut(dir) {
            listing.dirty = true;
        }
    }

    pub fn mark_dirty_under(&mut self, prefix: &str) {
        for (dir, listing) in &mut self.entries {
            if path_matches_prefix(dir, prefix) {
                listing.dirty = true;
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn split_parent_basename(canonical_id: &str) -> Option<(&str, &str)> {
    let (parent, basename) = canonical_id.rsplit_once('/')?;
    if parent.is_empty() || basename.is_empty() {
        return None;
    }
    Some((parent, basename))
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "dir_index_tests.rs"]
mod tests;
