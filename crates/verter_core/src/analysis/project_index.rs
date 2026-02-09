//! Cross-file project index for static analysis.
//!
//! This module provides a project-wide index that aggregates file usage information
//! to enable cross-file analysis such as:
//! - Validating inject() calls have matching provide() in ancestor components
//! - Building component dependency graphs
//! - Tracking provide/inject key usage across the project
//!
//! # Usage
//!
//! ```ignore
//! use verter_core::analysis::{ProjectIndex, FileUsageInfoOwned};
//! use std::path::PathBuf;
//!
//! // Create a new project index
//! let mut index = ProjectIndex::new();
//!
//! // Add files as they are parsed
//! let file_path = PathBuf::from("src/components/App.vue");
//! let usage_info = FileUsageInfoOwned { /* ... */ };
//! index.add_file(file_path, usage_info);
//!
//! // Query the index
//! let providers = index.files_providing("theme");
//! let injectors = index.files_injecting("theme");
//!
//! // Validate inject calls
//! let validation = index.validate_inject(&file_path, "theme");
//! match validation {
//!     InjectValidation::Valid { providers } => { /* inject is valid */ }
//!     InjectValidation::NoProvider => { /* warning: no provider found */ }
//!     InjectValidation::DynamicKey => { /* can't validate dynamic keys */ }
//! }
//! ```

#![allow(dead_code)]

use rustc_hash::FxHashMap;
use std::path::PathBuf;

use super::file_usage::FileUsageInfoOwned;

// =============================================================================
// Project Index
// =============================================================================

/// Project-wide index for cross-file static analysis.
///
/// Maintains indexes for:
/// - File usage information (imports, macros, provide/inject, etc.)
/// - Provide key → file mapping (which files provide which keys)
/// - Inject key → file mapping (which files inject which keys)
/// - Component usage graph (which files use which components)
#[derive(Debug, Default)]
pub struct ProjectIndex {
    /// All indexed files with their usage information
    files: FxHashMap<PathBuf, FileUsageInfoOwned>,

    /// Index of provide keys to files that provide them
    /// Key: injection key string, Value: list of file paths
    provide_index: FxHashMap<String, Vec<PathBuf>>,

    /// Index of inject keys to files that inject them
    /// Key: injection key string, Value: list of file paths
    inject_index: FxHashMap<String, Vec<PathBuf>>,

    /// Component usage graph: file → components it uses
    component_graph: FxHashMap<PathBuf, Vec<ComponentEdge>>,

    /// Reverse component graph: component name → files that use it
    component_reverse_index: FxHashMap<String, Vec<PathBuf>>,
}

/// An edge in the component usage graph
#[derive(Debug, Clone)]
pub struct ComponentEdge {
    /// The component name being used
    pub component_name: String,
    /// Whether this is a dynamic component usage
    pub is_dynamic: bool,
    /// Start offset in source (for diagnostics)
    pub start: u32,
    /// End offset in source
    pub end: u32,
}

// =============================================================================
// Validation Types
// =============================================================================

/// Result of validating an inject() call
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectValidation {
    /// The inject has at least one potential provider
    Valid {
        /// Files that provide this key
        providers: Vec<PathBuf>,
    },
    /// No provider found for this key in the project
    NoProvider,
    /// The inject uses a dynamic key that can't be statically validated
    DynamicKey,
    /// The key was not found in the specified file
    KeyNotFound,
}

/// Result of validating all injects in a file
#[derive(Debug, Clone, Default)]
pub struct FileInjectValidation {
    /// Valid injects with their providers
    pub valid: Vec<InjectValidationEntry>,
    /// Injects with no provider
    pub missing_providers: Vec<InjectValidationEntry>,
    /// Injects with dynamic keys (can't validate)
    pub dynamic_keys: Vec<DynamicInjectEntry>,
}

/// An entry for a validated inject
#[derive(Debug, Clone)]
pub struct InjectValidationEntry {
    /// The injection key
    pub key: String,
    /// Files that provide this key (empty if no provider)
    pub providers: Vec<PathBuf>,
    /// Start offset in source
    pub start: u32,
    /// End offset in source
    pub end: u32,
}

/// An entry for a dynamic inject that can't be validated
#[derive(Debug, Clone)]
pub struct DynamicInjectEntry {
    /// Start offset in source
    pub start: u32,
    /// End offset in source
    pub end: u32,
}

// =============================================================================
// Query Result Types
// =============================================================================

/// Summary of provide/inject usage in the project
#[derive(Debug, Clone, Default)]
pub struct ProvideInjectSummary {
    /// All unique provide keys in the project
    pub provide_keys: Vec<String>,
    /// All unique inject keys in the project
    pub inject_keys: Vec<String>,
    /// Keys that are provided but never injected
    pub unused_provides: Vec<String>,
    /// Keys that are injected but never provided
    pub missing_provides: Vec<String>,
}

/// Summary of component usage in the project
#[derive(Debug, Clone, Default)]
pub struct ComponentUsageSummary {
    /// All unique component names used
    pub component_names: Vec<String>,
    /// Components that are used but not found in the project
    pub external_components: Vec<String>,
    /// Number of files using each component
    pub usage_counts: FxHashMap<String, usize>,
}

/// Project-wide statistics
#[derive(Debug, Clone, Default)]
pub struct ProjectStats {
    /// Total number of indexed files
    pub file_count: usize,
    /// Number of files with provide() calls
    pub files_with_provide: usize,
    /// Number of files with inject() calls
    pub files_with_inject: usize,
    /// Number of files with defineProps
    pub files_with_props: usize,
    /// Number of files with defineEmits
    pub files_with_emits: usize,
    /// Number of files using async setup
    pub files_with_async_setup: usize,
    /// Total unique provide keys
    pub unique_provide_keys: usize,
    /// Total unique inject keys
    pub unique_inject_keys: usize,
}

// =============================================================================
// Implementation
// =============================================================================

impl ProjectIndex {
    /// Create a new empty project index
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with capacity hints based on expected project size
    pub fn with_capacity(file_count: usize) -> Self {
        Self {
            files: FxHashMap::with_capacity_and_hasher(file_count, Default::default()),
            provide_index: FxHashMap::default(),
            inject_index: FxHashMap::default(),
            component_graph: FxHashMap::with_capacity_and_hasher(file_count, Default::default()),
            component_reverse_index: FxHashMap::default(),
        }
    }

    // ==================== File Management ====================

    /// Add or update a file in the index
    pub fn add_file(&mut self, path: PathBuf, info: FileUsageInfoOwned) {
        // Remove old entries if file was previously indexed
        if self.files.contains_key(&path) {
            self.remove_file(&path);
        }

        // Index provide keys
        for provide in &info.provides {
            if let Some(key) = &provide.key {
                self.provide_index
                    .entry(key.clone())
                    .or_default()
                    .push(path.clone());
            }
        }

        // Index inject keys
        for inject in &info.injects {
            if let Some(key) = &inject.key {
                self.inject_index
                    .entry(key.clone())
                    .or_default()
                    .push(path.clone());
            }
        }

        // Build component graph
        let edges: Vec<ComponentEdge> = info
            .components
            .iter()
            .filter_map(|c| {
                c.name.as_ref().map(|name| ComponentEdge {
                    component_name: name.clone(),
                    is_dynamic: c.is_dynamic,
                    start: c.start,
                    end: c.end,
                })
            })
            .collect();

        // Update reverse index
        for edge in &edges {
            self.component_reverse_index
                .entry(edge.component_name.clone())
                .or_default()
                .push(path.clone());
        }

        self.component_graph.insert(path.clone(), edges);
        self.files.insert(path, info);
    }

    /// Remove a file from the index
    pub fn remove_file(&mut self, path: &PathBuf) -> Option<FileUsageInfoOwned> {
        let info = self.files.remove(path)?;

        // Remove from provide index
        for provide in &info.provides {
            if let Some(key) = &provide.key {
                if let Some(providers) = self.provide_index.get_mut(key) {
                    providers.retain(|p| p != path);
                    if providers.is_empty() {
                        self.provide_index.remove(key);
                    }
                }
            }
        }

        // Remove from inject index
        for inject in &info.injects {
            if let Some(key) = &inject.key {
                if let Some(injectors) = self.inject_index.get_mut(key) {
                    injectors.retain(|p| p != path);
                    if injectors.is_empty() {
                        self.inject_index.remove(key);
                    }
                }
            }
        }

        // Remove from component graph
        if let Some(edges) = self.component_graph.remove(path) {
            for edge in edges {
                if let Some(users) = self.component_reverse_index.get_mut(&edge.component_name) {
                    users.retain(|p| p != path);
                    if users.is_empty() {
                        self.component_reverse_index.remove(&edge.component_name);
                    }
                }
            }
        }

        Some(info)
    }

    /// Get file usage information by path
    pub fn get_file(&self, path: &PathBuf) -> Option<&FileUsageInfoOwned> {
        self.files.get(path)
    }

    /// Check if a file is indexed
    pub fn contains_file(&self, path: &PathBuf) -> bool {
        self.files.contains_key(path)
    }

    /// Get all indexed file paths
    pub fn file_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.keys()
    }

    /// Get the number of indexed files
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    // ==================== Provide/Inject Queries ====================

    /// Get files that provide a given key
    pub fn files_providing(&self, key: &str) -> &[PathBuf] {
        self.provide_index
            .get(key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get files that inject a given key
    pub fn files_injecting(&self, key: &str) -> &[PathBuf] {
        self.inject_index
            .get(key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all provide keys in the project
    pub fn all_provide_keys(&self) -> impl Iterator<Item = &String> {
        self.provide_index.keys()
    }

    /// Get all inject keys in the project
    pub fn all_inject_keys(&self) -> impl Iterator<Item = &String> {
        self.inject_index.keys()
    }

    // ==================== Component Graph Queries ====================

    /// Get components used by a file
    pub fn components_used_by(&self, path: &PathBuf) -> &[ComponentEdge] {
        self.component_graph
            .get(path)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get files that use a component
    pub fn files_using_component(&self, component_name: &str) -> &[PathBuf] {
        self.component_reverse_index
            .get(component_name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all component names used in the project
    pub fn all_component_names(&self) -> impl Iterator<Item = &String> {
        self.component_reverse_index.keys()
    }

    // ==================== Validation ====================

    /// Validate a single inject key for a file
    pub fn validate_inject(&self, file: &PathBuf, key: &str) -> InjectValidation {
        // First check if the file has this inject
        let Some(info) = self.files.get(file) else {
            return InjectValidation::KeyNotFound;
        };

        // Find the inject entry
        let inject = info.injects.iter().find(|i| i.key.as_deref() == Some(key));

        match inject {
            Some(i) if i.is_dynamic_key => InjectValidation::DynamicKey,
            Some(_) => {
                let providers: Vec<PathBuf> = self.files_providing(key).to_vec();
                if providers.is_empty() {
                    InjectValidation::NoProvider
                } else {
                    InjectValidation::Valid { providers }
                }
            }
            None => InjectValidation::KeyNotFound,
        }
    }

    /// Validate all injects in a file
    pub fn validate_file_injects(&self, file: &PathBuf) -> FileInjectValidation {
        let Some(info) = self.files.get(file) else {
            return FileInjectValidation::default();
        };

        let mut result = FileInjectValidation::default();

        for inject in &info.injects {
            if inject.is_dynamic_key {
                result.dynamic_keys.push(DynamicInjectEntry {
                    start: inject.start,
                    end: inject.end,
                });
            } else if let Some(key) = &inject.key {
                let providers: Vec<PathBuf> = self.files_providing(key).to_vec();
                let entry = InjectValidationEntry {
                    key: key.clone(),
                    providers: providers.clone(),
                    start: inject.start,
                    end: inject.end,
                };

                if providers.is_empty() {
                    result.missing_providers.push(entry);
                } else {
                    result.valid.push(entry);
                }
            }
        }

        result
    }

    // ==================== Summaries ====================

    /// Get a summary of provide/inject usage
    pub fn provide_inject_summary(&self) -> ProvideInjectSummary {
        let provide_keys: Vec<String> = self.provide_index.keys().cloned().collect();
        let inject_keys: Vec<String> = self.inject_index.keys().cloned().collect();

        let unused_provides = provide_keys
            .iter()
            .filter(|k| !self.inject_index.contains_key(*k))
            .cloned()
            .collect();

        let missing_provides = inject_keys
            .iter()
            .filter(|k| !self.provide_index.contains_key(*k))
            .cloned()
            .collect();

        ProvideInjectSummary {
            provide_keys,
            inject_keys,
            unused_provides,
            missing_provides,
        }
    }

    /// Get a summary of component usage
    pub fn component_usage_summary(&self) -> ComponentUsageSummary {
        let component_names: Vec<String> = self.component_reverse_index.keys().cloned().collect();

        let usage_counts: FxHashMap<String, usize> = self
            .component_reverse_index
            .iter()
            .map(|(name, files)| (name.clone(), files.len()))
            .collect();

        // External components are those used but not defined in the project
        // For now, we can't determine this without knowing which files define components
        // This would require component definition tracking
        let external_components = Vec::new();

        ComponentUsageSummary {
            component_names,
            external_components,
            usage_counts,
        }
    }

    /// Get project-wide statistics
    pub fn stats(&self) -> ProjectStats {
        use super::file_usage::FileUsageFlags;

        let mut stats = ProjectStats {
            file_count: self.files.len(),
            unique_provide_keys: self.provide_index.len(),
            unique_inject_keys: self.inject_index.len(),
            ..Default::default()
        };

        for info in self.files.values() {
            if info.has_flag(FileUsageFlags::HAS_PROVIDE) {
                stats.files_with_provide += 1;
            }
            if info.has_flag(FileUsageFlags::HAS_INJECT) {
                stats.files_with_inject += 1;
            }
            if info.has_flag(FileUsageFlags::HAS_DEFINE_PROPS) {
                stats.files_with_props += 1;
            }
            if info.has_flag(FileUsageFlags::HAS_DEFINE_EMITS) {
                stats.files_with_emits += 1;
            }
            if info.has_flag(FileUsageFlags::IS_ASYNC_SETUP) {
                stats.files_with_async_setup += 1;
            }
        }

        stats
    }

    /// Clear all indexed data
    pub fn clear(&mut self) {
        self.files.clear();
        self.provide_index.clear();
        self.inject_index.clear();
        self.component_graph.clear();
        self.component_reverse_index.clear();
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::file_usage::{
        ComponentUsageOwned, FileUsageFlags, InjectUsageOwned, ProvideUsageOwned,
    };

    fn make_file_info() -> FileUsageInfoOwned {
        FileUsageInfoOwned {
            imports: Vec::new(),
            macros: Vec::new(),
            provides: Vec::new(),
            injects: Vec::new(),
            components: Vec::new(),
            flags: 0,
        }
    }

    fn make_file_with_provide(key: &str) -> FileUsageInfoOwned {
        let mut info = make_file_info();
        info.provides.push(ProvideUsageOwned {
            key: Some(key.to_string()),
            is_dynamic_key: false,
            start: 0,
            end: 10,
        });
        info.flags |= FileUsageFlags::HAS_PROVIDE;
        info
    }

    fn make_file_with_inject(key: &str) -> FileUsageInfoOwned {
        let mut info = make_file_info();
        info.injects.push(InjectUsageOwned {
            key: Some(key.to_string()),
            is_dynamic_key: false,
            has_default: false,
            binding_name: None,
            start: 0,
            end: 10,
        });
        info.flags |= FileUsageFlags::HAS_INJECT;
        info
    }

    fn make_file_with_dynamic_inject() -> FileUsageInfoOwned {
        let mut info = make_file_info();
        info.injects.push(InjectUsageOwned {
            key: None,
            is_dynamic_key: true,
            has_default: false,
            binding_name: None,
            start: 0,
            end: 10,
        });
        info.flags |= FileUsageFlags::HAS_INJECT;
        info
    }

    fn make_file_with_component(name: &str) -> FileUsageInfoOwned {
        let mut info = make_file_info();
        info.components.push(ComponentUsageOwned {
            name: Some(name.to_string()),
            is_dynamic: false,
            start: 0,
            end: 10,
        });
        info.flags |= FileUsageFlags::HAS_COMPONENT_USAGE;
        info
    }

    #[test]
    fn test_new_index_is_empty() {
        let index = ProjectIndex::new();
        assert_eq!(index.file_count(), 0);
        assert_eq!(index.all_provide_keys().count(), 0);
        assert_eq!(index.all_inject_keys().count(), 0);
    }

    #[test]
    fn test_add_and_get_file() {
        let mut index = ProjectIndex::new();
        let path = PathBuf::from("src/App.vue");
        let info = make_file_info();

        index.add_file(path.clone(), info);

        assert!(index.contains_file(&path));
        assert_eq!(index.file_count(), 1);
        assert!(index.get_file(&path).is_some());
    }

    #[test]
    fn test_remove_file() {
        let mut index = ProjectIndex::new();
        let path = PathBuf::from("src/App.vue");
        let info = make_file_with_provide("theme");

        index.add_file(path.clone(), info);
        assert_eq!(index.files_providing("theme").len(), 1);

        let removed = index.remove_file(&path);
        assert!(removed.is_some());
        assert!(!index.contains_file(&path));
        assert_eq!(index.files_providing("theme").len(), 0);
    }

    #[test]
    fn test_provide_index() {
        let mut index = ProjectIndex::new();

        let app_path = PathBuf::from("src/App.vue");
        let child_path = PathBuf::from("src/Child.vue");

        index.add_file(app_path.clone(), make_file_with_provide("theme"));
        index.add_file(child_path.clone(), make_file_with_provide("theme"));

        let providers = index.files_providing("theme");
        assert_eq!(providers.len(), 2);
        assert!(providers.contains(&app_path));
        assert!(providers.contains(&child_path));
    }

    #[test]
    fn test_inject_index() {
        let mut index = ProjectIndex::new();

        let child1_path = PathBuf::from("src/Child1.vue");
        let child2_path = PathBuf::from("src/Child2.vue");

        index.add_file(child1_path.clone(), make_file_with_inject("theme"));
        index.add_file(child2_path.clone(), make_file_with_inject("theme"));

        let injectors = index.files_injecting("theme");
        assert_eq!(injectors.len(), 2);
        assert!(injectors.contains(&child1_path));
        assert!(injectors.contains(&child2_path));
    }

    #[test]
    fn test_validate_inject_valid() {
        let mut index = ProjectIndex::new();

        let provider_path = PathBuf::from("src/Provider.vue");
        let consumer_path = PathBuf::from("src/Consumer.vue");

        index.add_file(provider_path.clone(), make_file_with_provide("config"));
        index.add_file(consumer_path.clone(), make_file_with_inject("config"));

        let validation = index.validate_inject(&consumer_path, "config");
        match validation {
            InjectValidation::Valid { providers } => {
                assert_eq!(providers.len(), 1);
                assert!(providers.contains(&provider_path));
            }
            _ => panic!("Expected Valid validation"),
        }
    }

    #[test]
    fn test_validate_inject_no_provider() {
        let mut index = ProjectIndex::new();

        let consumer_path = PathBuf::from("src/Consumer.vue");
        index.add_file(consumer_path.clone(), make_file_with_inject("missing"));

        let validation = index.validate_inject(&consumer_path, "missing");
        assert_eq!(validation, InjectValidation::NoProvider);
    }

    #[test]
    fn test_validate_inject_dynamic_key() {
        let mut index = ProjectIndex::new();

        let consumer_path = PathBuf::from("src/Consumer.vue");
        let mut info = make_file_with_dynamic_inject();
        // Add a static inject too so we have something to look for
        info.injects.push(InjectUsageOwned {
            key: Some("static".to_string()),
            is_dynamic_key: false,
            has_default: false,
            binding_name: None,
            start: 20,
            end: 30,
        });
        index.add_file(consumer_path.clone(), info);

        // The file has a dynamic inject but validate_inject looks for specific key
        // So we need to test validate_file_injects for dynamic detection
        let file_validation = index.validate_file_injects(&consumer_path);
        assert_eq!(file_validation.dynamic_keys.len(), 1);
    }

    #[test]
    fn test_validate_inject_key_not_found() {
        let mut index = ProjectIndex::new();

        let consumer_path = PathBuf::from("src/Consumer.vue");
        index.add_file(consumer_path.clone(), make_file_with_inject("exists"));

        let validation = index.validate_inject(&consumer_path, "nonexistent");
        assert_eq!(validation, InjectValidation::KeyNotFound);
    }

    #[test]
    fn test_validate_file_injects() {
        let mut index = ProjectIndex::new();

        let provider_path = PathBuf::from("src/Provider.vue");
        let consumer_path = PathBuf::from("src/Consumer.vue");

        index.add_file(provider_path.clone(), make_file_with_provide("provided"));

        let mut consumer_info = make_file_info();
        consumer_info.injects.push(InjectUsageOwned {
            key: Some("provided".to_string()),
            is_dynamic_key: false,
            has_default: false,
            binding_name: None,
            start: 0,
            end: 10,
        });
        consumer_info.injects.push(InjectUsageOwned {
            key: Some("missing".to_string()),
            is_dynamic_key: false,
            has_default: false,
            binding_name: None,
            start: 20,
            end: 30,
        });
        consumer_info.injects.push(InjectUsageOwned {
            key: None,
            is_dynamic_key: true,
            has_default: false,
            binding_name: None,
            start: 40,
            end: 50,
        });
        consumer_info.flags |= FileUsageFlags::HAS_INJECT;
        index.add_file(consumer_path.clone(), consumer_info);

        let validation = index.validate_file_injects(&consumer_path);
        assert_eq!(validation.valid.len(), 1);
        assert_eq!(validation.valid[0].key, "provided");
        assert_eq!(validation.missing_providers.len(), 1);
        assert_eq!(validation.missing_providers[0].key, "missing");
        assert_eq!(validation.dynamic_keys.len(), 1);
    }

    #[test]
    fn test_component_graph() {
        let mut index = ProjectIndex::new();

        let app_path = PathBuf::from("src/App.vue");
        let mut app_info = make_file_info();
        app_info.components.push(ComponentUsageOwned {
            name: Some("Header".to_string()),
            is_dynamic: false,
            start: 0,
            end: 10,
        });
        app_info.components.push(ComponentUsageOwned {
            name: Some("Footer".to_string()),
            is_dynamic: false,
            start: 20,
            end: 30,
        });
        index.add_file(app_path.clone(), app_info);

        let components = index.components_used_by(&app_path);
        assert_eq!(components.len(), 2);

        let files_using_header = index.files_using_component("Header");
        assert_eq!(files_using_header.len(), 1);
        assert!(files_using_header.contains(&app_path));
    }

    #[test]
    fn test_provide_inject_summary() {
        let mut index = ProjectIndex::new();

        index.add_file(
            PathBuf::from("src/Provider.vue"),
            make_file_with_provide("theme"),
        );
        index.add_file(
            PathBuf::from("src/Provider2.vue"),
            make_file_with_provide("config"),
        );
        index.add_file(
            PathBuf::from("src/Consumer.vue"),
            make_file_with_inject("theme"),
        );
        index.add_file(
            PathBuf::from("src/Consumer2.vue"),
            make_file_with_inject("missing"),
        );

        let summary = index.provide_inject_summary();
        assert_eq!(summary.provide_keys.len(), 2); // theme, config
        assert_eq!(summary.inject_keys.len(), 2); // theme, missing
        assert_eq!(summary.unused_provides.len(), 1); // config
        assert_eq!(summary.missing_provides.len(), 1); // missing
    }

    #[test]
    fn test_project_stats() {
        let mut index = ProjectIndex::new();

        let mut info1 = make_file_with_provide("theme");
        info1.flags |= FileUsageFlags::HAS_DEFINE_PROPS;
        index.add_file(PathBuf::from("src/App.vue"), info1);

        let mut info2 = make_file_with_inject("theme");
        info2.flags |= FileUsageFlags::HAS_DEFINE_EMITS | FileUsageFlags::IS_ASYNC_SETUP;
        index.add_file(PathBuf::from("src/Child.vue"), info2);

        let stats = index.stats();
        assert_eq!(stats.file_count, 2);
        assert_eq!(stats.files_with_provide, 1);
        assert_eq!(stats.files_with_inject, 1);
        assert_eq!(stats.files_with_props, 1);
        assert_eq!(stats.files_with_emits, 1);
        assert_eq!(stats.files_with_async_setup, 1);
        assert_eq!(stats.unique_provide_keys, 1);
        assert_eq!(stats.unique_inject_keys, 1);
    }

    #[test]
    fn test_update_file_reindexes() {
        let mut index = ProjectIndex::new();
        let path = PathBuf::from("src/App.vue");

        // Add with "old" key
        index.add_file(path.clone(), make_file_with_provide("old"));
        assert_eq!(index.files_providing("old").len(), 1);
        assert_eq!(index.files_providing("new").len(), 0);

        // Update with "new" key
        index.add_file(path.clone(), make_file_with_provide("new"));
        assert_eq!(index.files_providing("old").len(), 0);
        assert_eq!(index.files_providing("new").len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut index = ProjectIndex::new();
        index.add_file(
            PathBuf::from("src/App.vue"),
            make_file_with_provide("theme"),
        );

        assert_eq!(index.file_count(), 1);
        index.clear();
        assert_eq!(index.file_count(), 0);
        assert_eq!(index.all_provide_keys().count(), 0);
    }

    #[test]
    fn test_component_usage_summary() {
        let mut index = ProjectIndex::new();

        index.add_file(
            PathBuf::from("src/App.vue"),
            make_file_with_component("Header"),
        );
        index.add_file(
            PathBuf::from("src/Page.vue"),
            make_file_with_component("Header"),
        );
        index.add_file(
            PathBuf::from("src/Other.vue"),
            make_file_with_component("Footer"),
        );

        let summary = index.component_usage_summary();
        assert_eq!(summary.component_names.len(), 2); // Header, Footer
        assert_eq!(summary.usage_counts.get("Header"), Some(&2));
        assert_eq!(summary.usage_counts.get("Footer"), Some(&1));
    }

    #[test]
    fn test_file_paths_iterator() {
        let mut index = ProjectIndex::new();
        let path1 = PathBuf::from("src/App.vue");
        let path2 = PathBuf::from("src/Child.vue");

        index.add_file(path1.clone(), make_file_info());
        index.add_file(path2.clone(), make_file_info());

        let paths: Vec<_> = index.file_paths().collect();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&&path1));
        assert!(paths.contains(&&path2));
    }
}
