//! Baseline mode: snapshot current diagnostics to filter "new issues only".
//!
//! Each baseline entry is `(rule, file_relative_path, content_hash)` where
//! `content_hash = hash(source[span_start..span_end])`. This is content-addressable:
//! code that moves within a file still matches; code that changes naturally expires.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A single baseline entry for a diagnostic finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineEntry {
    /// Rule name (e.g., `"no-v-html"`).
    pub rule: String,
    /// Hash of the source content at the diagnostic span.
    pub hash: String,
}

/// Stored baseline of known diagnostic findings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    /// Format version for forward compatibility.
    pub version: u32,
    /// ISO 8601 timestamp when the baseline was created.
    pub created: String,
    /// Entries keyed by relative file path.
    pub entries: BTreeMap<String, Vec<BaselineEntry>>,
}

impl Default for Baseline {
    fn default() -> Self {
        Self::new()
    }
}

impl Baseline {
    /// Create a new empty baseline.
    pub fn new() -> Self {
        Self {
            version: 1,
            created: String::new(),
            entries: BTreeMap::new(),
        }
    }

    /// Add an entry to the baseline.
    pub fn add(&mut self, relative_path: &str, rule: &str, span_content: &str) {
        let hash = content_hash(span_content);
        self.entries
            .entry(relative_path.to_string())
            .or_default()
            .push(BaselineEntry {
                rule: rule.to_string(),
                hash,
            });
    }

    /// Check if a diagnostic is already in the baseline.
    pub fn contains(&self, relative_path: &str, rule: &str, span_content: &str) -> bool {
        let hash = content_hash(span_content);
        self.entries
            .get(relative_path)
            .map(|entries| entries.iter().any(|e| e.rule == rule && e.hash == hash))
            .unwrap_or(false)
    }

    /// Save the baseline to a JSON file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Load a baseline from a JSON file.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Number of total entries across all files.
    pub fn total_entries(&self) -> usize {
        self.entries.values().map(|v| v.len()).sum()
    }
}

/// Compute a hex-encoded hash of the span content.
fn content_hash(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Make a path relative to a project root.
pub fn make_relative(path: &str, root: &str) -> String {
    let path = path.replace('\\', "/");
    let root = root.replace('\\', "/");
    let root = if root.ends_with('/') {
        root
    } else {
        format!("{root}/")
    };
    if path.starts_with(&root) {
        path[root.len()..].to_string()
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_deterministic() {
        let h1 = content_hash("v-html");
        let h2 = content_hash("v-html");
        assert_eq!(h1, h2, "same content should produce same hash");
        assert_eq!(h1.len(), 16, "hash should be 16 hex chars");
    }

    #[test]
    fn content_hash_different_for_different_content() {
        let h1 = content_hash("v-html");
        let h2 = content_hash("v-text");
        assert_ne!(h1, h2, "different content should produce different hash");
    }

    #[test]
    fn baseline_add_and_contains() {
        let mut baseline = Baseline::new();
        baseline.add("src/Foo.vue", "no-v-html", "<div v-html=\"x\">");

        assert!(
            baseline.contains("src/Foo.vue", "no-v-html", "<div v-html=\"x\">"),
            "should find exact match"
        );
        assert!(
            !baseline.contains("src/Foo.vue", "no-v-html", "<div v-html=\"y\">"),
            "different content should not match"
        );
        assert!(
            !baseline.contains("src/Foo.vue", "no-v-text", "<div v-html=\"x\">"),
            "different rule should not match"
        );
        assert!(
            !baseline.contains("src/Bar.vue", "no-v-html", "<div v-html=\"x\">"),
            "different file should not match"
        );
    }

    #[test]
    fn baseline_total_entries() {
        let mut baseline = Baseline::new();
        assert_eq!(baseline.total_entries(), 0);

        baseline.add("src/A.vue", "r1", "content1");
        baseline.add("src/A.vue", "r2", "content2");
        baseline.add("src/B.vue", "r1", "content3");
        assert_eq!(baseline.total_entries(), 3);
    }

    #[test]
    fn baseline_save_load_roundtrip() {
        let mut baseline = Baseline::new();
        baseline.created = "2026-03-07T00:00:00Z".to_string();
        baseline.add("src/Foo.vue", "no-v-html", "v-html content");
        baseline.add("src/Bar.vue", "require-v-for-key", "v-for item");

        let tmp = std::env::temp_dir().join("verter-baseline-test.json");
        baseline.save(&tmp).expect("save");

        let loaded = Baseline::load(&tmp).expect("load");
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.created, "2026-03-07T00:00:00Z");
        assert_eq!(loaded.total_entries(), 2);
        assert!(loaded.contains("src/Foo.vue", "no-v-html", "v-html content"));
        assert!(loaded.contains("src/Bar.vue", "require-v-for-key", "v-for item"));
        assert!(!loaded.contains("src/Foo.vue", "no-v-html", "different content"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn baseline_serde_format() {
        let mut baseline = Baseline::new();
        baseline.created = "2026-03-07T00:00:00Z".to_string();
        baseline.add("src/Foo.vue", "no-v-html", "content");

        let json = serde_json::to_string_pretty(&baseline).expect("serialize");
        assert!(json.contains("\"version\": 1"));
        assert!(json.contains("\"src/Foo.vue\""));
        assert!(json.contains("\"no-v-html\""));
        assert!(
            !json.contains("\"src/Bar.vue\""),
            "should not have entries for Bar.vue"
        );
    }

    #[test]
    fn make_relative_strips_root() {
        assert_eq!(
            make_relative("/project/src/Foo.vue", "/project"),
            "src/Foo.vue"
        );
        assert_eq!(
            make_relative("/project/src/Foo.vue", "/project/"),
            "src/Foo.vue"
        );
    }

    #[test]
    fn make_relative_handles_backslashes() {
        assert_eq!(
            make_relative("C:\\project\\src\\Foo.vue", "C:\\project"),
            "src/Foo.vue"
        );
    }

    #[test]
    fn make_relative_returns_original_if_no_match() {
        assert_eq!(
            make_relative("/other/src/Foo.vue", "/project"),
            "/other/src/Foo.vue"
        );
    }
}
