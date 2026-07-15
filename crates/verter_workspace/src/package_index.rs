use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::types::PackageManifest;

/// Tri-state cache entry for package manifest lookups.
#[derive(Debug, Clone)]
pub enum ManifestEntry {
    /// Successfully parsed manifest.
    Found(Box<PackageManifest>),
    /// File does not exist (negative cache).
    NotFound,
}

/// Host-owned cache for parsed `package.json` manifests with negative caching.
///
/// Both positive (Found) and negative (NotFound) results are cached so
/// repeated ancestor-chain probes for missing manifests are answered from
/// cache instead of repeated I/O.
///
/// Invalidation via `invalidate` / `invalidate_under` clears both positive
/// and negative entries, allowing re-probe after `npm install` or file changes.
pub struct PackageIndex {
    /// Cached manifest entries keyed by canonical path to package.json.
    cache: FxHashMap<String, ManifestEntry>,
}

impl PackageIndex {
    pub fn new() -> Self {
        Self {
            cache: FxHashMap::default(),
        }
    }

    /// Get a cached manifest, or parse and cache it from the provided source.
    ///
    /// The caller is responsible for reading the file content via the VFS
    /// and passing it here. This keeps PackageIndex free of I/O concerns.
    /// If a prior `NotFound` entry exists, it is upgraded to `Found`.
    pub fn get_or_parse(&mut self, package_json_path: &str, source: &str) -> &PackageManifest {
        // Always insert/overwrite with Found when source is provided.
        let needs_insert = match self.cache.get(package_json_path) {
            Some(ManifestEntry::Found(_)) => false,
            _ => true, // None or NotFound → insert
        };
        if needs_insert {
            let manifest = parse_package_json(source);
            self.cache.insert(
                package_json_path.to_string(),
                ManifestEntry::Found(Box::new(manifest)),
            );
        }
        match self.cache.get(package_json_path).unwrap() {
            ManifestEntry::Found(m) => m,
            ManifestEntry::NotFound => unreachable!("just inserted Found"),
        }
    }

    /// Get a cached entry without triggering a parse.
    /// Returns `Some(ManifestEntry::Found(_))` for positive hits,
    /// `Some(ManifestEntry::NotFound)` for negative hits,
    /// `None` for never-probed paths.
    pub fn get_cached(&self, package_json_path: &str) -> Option<&ManifestEntry> {
        self.cache.get(package_json_path)
    }

    /// Record that a package.json file does not exist at the given path.
    /// Subsequent `get_cached` calls will return `Some(ManifestEntry::NotFound)`.
    pub fn insert_not_found(&mut self, package_json_path: &str) {
        self.cache
            .insert(package_json_path.to_string(), ManifestEntry::NotFound);
    }

    /// Invalidate a cached entry (e.g., after a watcher event).
    /// Clears both positive and negative entries.
    pub fn invalidate(&mut self, package_json_path: &str) -> bool {
        self.cache.remove(package_json_path).is_some()
    }

    /// Invalidate all entries under a given directory prefix.
    /// Clears both positive and negative entries.
    pub fn invalidate_under(&mut self, prefix: &str) {
        self.cache.retain(|k, _| !k.starts_with(prefix));
    }

    /// Number of cached entries (both positive and negative).
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Number of positive (Found) entries only.
    pub fn found_count(&self) -> usize {
        self.cache
            .values()
            .filter(|e| matches!(e, ManifestEntry::Found(_)))
            .count()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl Default for PackageIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PackageIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageIndex")
            .field("cached_count", &self.cache.len())
            .finish()
    }
}

/// Parse a package.json string into a `PackageManifest`.
pub(crate) fn parse_package_json(source: &str) -> PackageManifest {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(source) else {
        return PackageManifest {
            raw: Some(Arc::from(source)),
            ..Default::default()
        };
    };

    let obj = match &value {
        serde_json::Value::Object(map) => map,
        _ => {
            return PackageManifest {
                raw: Some(Arc::from(source)),
                ..Default::default()
            };
        }
    };

    PackageManifest {
        name: obj
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        version: obj
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        main: obj
            .get("main")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        module: obj
            .get("module")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        types: obj
            .get("types")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        typings: obj
            .get("typings")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        exports: obj.get("exports").cloned(),
        imports: obj.get("imports").cloned(),
        raw: Some(Arc::from(source)),
    }
}

#[cfg(test)]
#[path = "package_index_tests.rs"]
mod tests;
