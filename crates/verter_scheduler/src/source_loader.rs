//! Source loader implementations.
//!
//! [`SourceLoader`] is the trait used by the scheduler's Source stage
//! to load file content. Two implementations are provided:
//!
//! - [`MemorySourceLoader`] — in-memory map (tests, WASM, playground)
//! - [`DiskSourceLoader`] — overlay + disk via NativeFs (native builds)
//!
//! **Note**: This trait mirrors [`verter_workspace::SourceLoader`] but is defined
//! separately to keep `verter_scheduler` free of VFS dependencies. The VFS
//! trait is the canonical definition; this is a scheduler-local copy.
//!
//! Classification authority: the built-in loaders classify through the
//! PURE static extension registry (`verter_language`). Host-gated
//! classification (project-capability-resolved rows) reaches the
//! scheduler exclusively through the session-implemented [`SourceLoader`]
//! seam.

use std::sync::Arc;

use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use verter_language::FileLanguage;

use crate::overlay::OverlayMap;

/// Read-only file loading interface for the scheduler's I/O pool.
///
/// Implementations check overlay first, then fall back to their
/// backing store (memory map or disk). All methods are sync.
pub trait SourceLoader: Send + Sync {
    /// Load file content by canonical ID.
    fn load(&self, canonical_id: &str) -> Option<Arc<str>>;

    /// Check whether a file exists.
    fn exists(&self, canonical_id: &str) -> bool;

    /// Classify a file. Built-in loaders use the static extension
    /// registry; the session's loader composes host-gated rows.
    fn classify(&self, canonical_id: &str) -> FileLanguage;

    /// Resolve symlinks to real path.
    fn realpath(&self, canonical_id: &str) -> Option<String>;
}

/// In-memory source loader. Checks overlay first, then the injected files map.
///
/// Used by tests, WASM, and playground where there is no filesystem.
pub struct MemorySourceLoader {
    files: RwLock<FxHashMap<String, Arc<str>>>,
    overlay: Arc<OverlayMap>,
}

impl MemorySourceLoader {
    pub fn new() -> Self {
        Self {
            files: RwLock::new(FxHashMap::default()),
            overlay: Arc::new(OverlayMap::new()),
        }
    }

    pub fn with_overlay(overlay: Arc<OverlayMap>) -> Self {
        Self {
            files: RwLock::new(FxHashMap::default()),
            overlay,
        }
    }

    /// Inject a file into the memory store.
    pub fn insert(&self, canonical_id: String, source: Arc<str>) {
        self.files.write().insert(canonical_id, source);
    }

    /// Remove a file from the memory store.
    pub fn remove(&self, canonical_id: &str) {
        self.files.write().remove(canonical_id);
    }

    /// Get a reference to the shared overlay.
    pub fn overlay(&self) -> &Arc<OverlayMap> {
        &self.overlay
    }
}

impl Default for MemorySourceLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceLoader for MemorySourceLoader {
    fn load(&self, canonical_id: &str) -> Option<Arc<str>> {
        // Overlay first
        if let Some(content) = self.overlay.get(canonical_id) {
            return Some(content);
        }
        self.files.read().get(canonical_id).cloned()
    }

    fn exists(&self, canonical_id: &str) -> bool {
        self.overlay.has(canonical_id) || self.files.read().contains_key(canonical_id)
    }

    fn classify(&self, canonical_id: &str) -> FileLanguage {
        verter_language::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution()
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        if self.exists(canonical_id) {
            Some(canonical_id.to_string())
        } else {
            None
        }
    }
}

/// Disk-backed source loader. Checks overlay first, then reads from disk.
///
/// Used by the LSP, MCP, and bundler plugins in native builds.
/// Only available on non-WASM targets.
#[cfg(not(target_arch = "wasm32"))]
pub struct DiskSourceLoader {
    overlay: Arc<OverlayMap>,
}

#[cfg(not(target_arch = "wasm32"))]
impl DiskSourceLoader {
    pub fn new(overlay: Arc<OverlayMap>) -> Self {
        Self { overlay }
    }

    /// Get a reference to the shared overlay.
    pub fn overlay(&self) -> &Arc<OverlayMap> {
        &self.overlay
    }

    /// Read from disk using OS path conversion.
    fn read_disk(&self, canonical_id: &str) -> Option<Arc<str>> {
        let os_path = canonical_to_os_path(canonical_id);
        std::fs::read_to_string(&os_path).ok().map(Arc::from)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SourceLoader for DiskSourceLoader {
    fn load(&self, canonical_id: &str) -> Option<Arc<str>> {
        if let Some(content) = self.overlay.get(canonical_id) {
            return Some(content);
        }
        self.read_disk(canonical_id)
    }

    fn exists(&self, canonical_id: &str) -> bool {
        if self.overlay.has(canonical_id) {
            return true;
        }
        let os_path = canonical_to_os_path(canonical_id);
        std::path::Path::new(&os_path).exists()
    }

    fn classify(&self, canonical_id: &str) -> FileLanguage {
        verter_language::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution()
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        let os_path = canonical_to_os_path(canonical_id);
        std::fs::canonicalize(&os_path)
            .ok()
            .map(|p| normalize_path_str(&p.to_string_lossy()))
    }
}

/// Convert a canonical ID (forward-slash, no drive prefix on Windows)
/// to an OS path.
#[cfg(not(target_arch = "wasm32"))]
fn canonical_to_os_path(canonical_id: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        // /c/foo/bar → C:\foo\bar
        if canonical_id.len() >= 3
            && canonical_id.as_bytes()[0] == b'/'
            && canonical_id.as_bytes()[2] == b'/'
        {
            let drive = canonical_id.as_bytes()[1].to_ascii_uppercase() as char;
            let rest = &canonical_id[2..];
            return format!("{drive}:{}", rest.replace('/', "\\"));
        }
        canonical_id.replace('/', "\\")
    }
    #[cfg(not(target_os = "windows"))]
    {
        canonical_id.to_string()
    }
}

/// Normalize an OS path string to canonical form.
#[cfg(not(target_arch = "wasm32"))]
fn normalize_path_str(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    // Strip \\?\ UNC prefix (Windows canonicalize artifact)
    let normalized = normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_string();
    // Lowercase drive letter: C:/foo → /c/foo
    #[cfg(target_os = "windows")]
    {
        if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
            let drive = normalized.as_bytes()[0].to_ascii_lowercase() as char;
            return format!("/{drive}{}", &normalized[2..]);
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MemorySourceLoader ──

    #[test]
    fn memory_loader_insert_and_load() {
        let loader = MemorySourceLoader::new();
        loader.insert("/a.vue".to_string(), Arc::from("<template>hi</template>"));

        let content = loader.load("/a.vue").unwrap();
        assert_eq!(&*content, "<template>hi</template>");
    }

    #[test]
    fn memory_loader_overlay_priority() {
        let overlay = Arc::new(OverlayMap::new());
        let loader = MemorySourceLoader::with_overlay(Arc::clone(&overlay));

        loader.insert("/a.vue".to_string(), Arc::from("disk content"));
        overlay.set("/a.vue".to_string(), Arc::from("overlay content"));

        assert_eq!(&*loader.load("/a.vue").unwrap(), "overlay content");
    }

    #[test]
    fn memory_loader_exists() {
        let loader = MemorySourceLoader::new();
        assert!(!loader.exists("/a.vue"));
        loader.insert("/a.vue".to_string(), Arc::from("x"));
        assert!(loader.exists("/a.vue"));
    }

    #[test]
    fn memory_loader_classify() {
        use verter_language::ScriptSourceType;
        let loader = MemorySourceLoader::new();
        assert_eq!(loader.classify("/a.vue"), FileLanguage::vue());
        assert_eq!(
            loader.classify("/a.ts"),
            FileLanguage::script(ScriptSourceType::Ts)
        );
        assert_eq!(
            loader.classify("/a.tsx"),
            FileLanguage::script(ScriptSourceType::Tsx)
        );
    }

    #[test]
    fn memory_loader_realpath() {
        let loader = MemorySourceLoader::new();
        assert!(loader.realpath("/a.vue").is_none());
        loader.insert("/a.vue".to_string(), Arc::from("x"));
        assert_eq!(loader.realpath("/a.vue").unwrap(), "/a.vue");
    }

    #[test]
    fn memory_loader_remove() {
        let loader = MemorySourceLoader::new();
        loader.insert("/a.vue".to_string(), Arc::from("x"));
        assert!(loader.exists("/a.vue"));
        loader.remove("/a.vue");
        assert!(!loader.exists("/a.vue"));
    }

    // ── DiskSourceLoader (native only) ──

    #[cfg(not(target_arch = "wasm32"))]
    mod disk_tests {
        use super::*;

        #[test]
        fn disk_loader_overlay_priority() {
            let overlay = Arc::new(OverlayMap::new());
            let loader = DiskSourceLoader::new(Arc::clone(&overlay));

            overlay.set("/nonexistent.vue".to_string(), Arc::from("overlay"));
            assert_eq!(&*loader.load("/nonexistent.vue").unwrap(), "overlay");
        }

        #[test]
        fn disk_loader_missing_file_returns_none() {
            let loader = DiskSourceLoader::new(Arc::new(OverlayMap::new()));
            assert!(loader.load("/definitely/not/a/real/file.vue").is_none());
        }

        #[test]
        fn disk_loader_exists_with_overlay() {
            let overlay = Arc::new(OverlayMap::new());
            let loader = DiskSourceLoader::new(Arc::clone(&overlay));

            assert!(!loader.exists("/nonexistent.vue"));
            overlay.set("/nonexistent.vue".to_string(), Arc::from("x"));
            assert!(loader.exists("/nonexistent.vue"));
        }

        #[test]
        fn canonical_to_os_path_unix_passthrough() {
            #[cfg(not(target_os = "windows"))]
            assert_eq!(
                canonical_to_os_path("/home/user/file.vue"),
                "/home/user/file.vue"
            );
        }

        #[test]
        #[cfg(target_os = "windows")]
        fn canonical_to_os_path_windows_drive() {
            assert_eq!(canonical_to_os_path("/c/foo/bar.vue"), "C:\\foo\\bar.vue");
            assert_eq!(canonical_to_os_path("/d/src/app.ts"), "D:\\src\\app.ts");
        }
    }
}
