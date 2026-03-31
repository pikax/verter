use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::types::PackageManifest;

/// Lazy cache for parsed `package.json` manifests.
///
/// Manifests are loaded through normal `read_file()` on first access and cached.
/// `node_modules` add/remove/update events invalidate affected entries.
pub struct PackageIndex {
    /// Cached manifests keyed by canonical path to package.json.
    cache: FxHashMap<String, PackageManifest>,
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
    pub fn get_or_parse(&mut self, package_json_path: &str, source: &str) -> &PackageManifest {
        if !self.cache.contains_key(package_json_path) {
            let manifest = parse_package_json(source);
            self.cache.insert(package_json_path.to_string(), manifest);
        }
        self.cache.get(package_json_path).unwrap()
    }

    /// Get a cached manifest without triggering a parse.
    pub fn get_cached(&self, package_json_path: &str) -> Option<&PackageManifest> {
        self.cache.get(package_json_path)
    }

    /// Invalidate a cached manifest (e.g., after a watcher event).
    pub fn invalidate(&mut self, package_json_path: &str) -> bool {
        self.cache.remove(package_json_path).is_some()
    }

    /// Invalidate all manifests under a given directory prefix.
    pub fn invalidate_under(&mut self, prefix: &str) {
        self.cache.retain(|k, _| !k.starts_with(prefix));
    }

    /// Number of cached manifests.
    pub fn len(&self) -> usize {
        self.cache.len()
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
