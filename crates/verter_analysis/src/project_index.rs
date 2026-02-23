//! Cross-file project index for static analysis.
//!
//! Aggregates file usage information to enable cross-file queries such as:
//! - Validating inject() calls have matching provide() in the project
//! - Building component dependency graphs
//! - Tracking provide/inject key usage across the project

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::file_usage::{FileUsageFlags, FileUsageInfoOwned};

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
    /// All indexed files with their usage information.
    /// Uses `Arc<Path>` to avoid full path cloning when inserting into multiple indexes.
    files: FxHashMap<Arc<Path>, FileUsageInfoOwned>,

    /// Index of provide keys to files that provide them
    provide_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// Index of inject keys to files that inject them
    inject_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// Component usage graph: file → components it uses
    component_graph: FxHashMap<Arc<Path>, Vec<ComponentEdge>>,

    /// Reverse component graph: component name → files that use it
    component_reverse_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// CSS class name → files that define it
    class_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// v-bind CSS expression → files that use it
    v_bind_css_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// CSS custom property name → files that define it
    custom_property_index: FxHashMap<String, FxHashSet<Arc<Path>>>,
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
        providers: Vec<Arc<Path>>,
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
    pub providers: Vec<Arc<Path>>,
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
    /// Number of files with scoped styles
    pub files_with_scoped_styles: usize,
    /// Number of files with CSS modules
    pub files_with_css_modules: usize,
    /// Number of files with v-bind() in CSS
    pub files_with_v_bind_css: usize,
    /// Total unique CSS class names
    pub unique_css_classes: usize,
    /// Total unique CSS custom properties
    pub unique_custom_properties: usize,
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
        ProjectIndex {
            files: FxHashMap::with_capacity_and_hasher(file_count, Default::default()),
            provide_index: FxHashMap::default(),
            inject_index: FxHashMap::default(),
            component_graph: FxHashMap::with_capacity_and_hasher(file_count, Default::default()),
            component_reverse_index: FxHashMap::default(),
            class_index: FxHashMap::default(),
            v_bind_css_index: FxHashMap::default(),
            custom_property_index: FxHashMap::default(),
        }
    }

    // ==================== File Management ====================

    /// Add or update a file in the index
    pub fn add_file(&mut self, path: PathBuf, info: FileUsageInfoOwned) {
        // Remove old entries if file was previously indexed
        if self.files.contains_key(path.as_path()) {
            self.remove_file(&path);
        }

        // Convert PathBuf to Arc<Path> once, then Arc::clone for each index insertion
        let path: Arc<Path> = Arc::from(path);

        // Index provide keys
        for provide in &info.provides {
            if let Some(key) = &provide.key {
                self.provide_index
                    .entry(key.clone())
                    .or_default()
                    .insert(Arc::clone(&path));
            }
        }

        // Index inject keys
        for inject in &info.injects {
            if let Some(key) = &inject.key {
                self.inject_index
                    .entry(key.clone())
                    .or_default()
                    .insert(Arc::clone(&path));
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
                .insert(Arc::clone(&path));
        }

        self.component_graph.insert(Arc::clone(&path), edges);

        // Index style data
        for style in &info.styles {
            for class_name in &style.class_names {
                self.class_index
                    .entry(class_name.clone())
                    .or_default()
                    .insert(Arc::clone(&path));
            }
            for expr in &style.v_bind_expressions {
                self.v_bind_css_index
                    .entry(expr.clone())
                    .or_default()
                    .insert(Arc::clone(&path));
            }
            for prop in &style.custom_property_names {
                self.custom_property_index
                    .entry(prop.clone())
                    .or_default()
                    .insert(Arc::clone(&path));
            }
        }

        self.files.insert(path, info);
    }

    /// Remove a file from the index
    pub fn remove_file(&mut self, path: &Path) -> Option<FileUsageInfoOwned> {
        let info = self.files.remove(path)?;

        // We need an Arc<Path> to remove from HashSets.
        // Construct one from the path to match entries in the sets.
        let arc_path: Arc<Path> = Arc::from(path);

        // Remove from provide index
        for provide in &info.provides {
            if let Some(key) = &provide.key {
                if let Some(providers) = self.provide_index.get_mut(key) {
                    providers.remove(&arc_path);
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
                    injectors.remove(&arc_path);
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
                    users.remove(&arc_path);
                    if users.is_empty() {
                        self.component_reverse_index.remove(&edge.component_name);
                    }
                }
            }
        }

        // Remove from style indexes
        for style in &info.styles {
            for class_name in &style.class_names {
                if let Some(files) = self.class_index.get_mut(class_name) {
                    files.remove(&arc_path);
                    if files.is_empty() {
                        self.class_index.remove(class_name);
                    }
                }
            }
            for expr in &style.v_bind_expressions {
                if let Some(files) = self.v_bind_css_index.get_mut(expr) {
                    files.remove(&arc_path);
                    if files.is_empty() {
                        self.v_bind_css_index.remove(expr);
                    }
                }
            }
            for prop in &style.custom_property_names {
                if let Some(files) = self.custom_property_index.get_mut(prop) {
                    files.remove(&arc_path);
                    if files.is_empty() {
                        self.custom_property_index.remove(prop);
                    }
                }
            }
        }

        Some(info)
    }

    /// Get file usage information by path
    pub fn get_file(&self, path: &Path) -> Option<&FileUsageInfoOwned> {
        self.files.get(path)
    }

    /// Check if a file is indexed
    pub fn contains_file(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    /// Get all indexed file paths
    pub fn file_paths(&self) -> impl Iterator<Item = &Arc<Path>> {
        self.files.keys()
    }

    /// Get the number of indexed files
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    // ==================== Provide/Inject Queries ====================

    /// Get files that provide a given key
    pub fn files_providing(&self, key: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.provide_index
            .get(key)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Check if any file provides a given key
    pub fn has_providers(&self, key: &str) -> bool {
        self.provide_index.get(key).is_some_and(|s| !s.is_empty())
    }

    /// Get files that inject a given key
    pub fn files_injecting(&self, key: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.inject_index
            .get(key)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Check if any file injects a given key
    pub fn has_injectors(&self, key: &str) -> bool {
        self.inject_index.get(key).is_some_and(|s| !s.is_empty())
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
    pub fn components_used_by(&self, path: &Path) -> &[ComponentEdge] {
        self.component_graph
            .get(path)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get files that use a component
    pub fn files_using_component(&self, component_name: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.component_reverse_index
            .get(component_name)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Get all component names used in the project
    pub fn all_component_names(&self) -> impl Iterator<Item = &String> {
        self.component_reverse_index.keys()
    }

    // ==================== Style Queries ====================

    /// Get files that define a given CSS class name
    pub fn files_defining_class(&self, name: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.class_index
            .get(name)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Get files that use a given v-bind CSS expression
    pub fn files_using_v_bind_css(&self, expr: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.v_bind_css_index
            .get(expr)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Get files that define a given CSS custom property
    pub fn files_defining_custom_property(&self, name: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.custom_property_index
            .get(name)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Get all CSS class names in the project
    pub fn all_class_names(&self) -> impl Iterator<Item = &String> {
        self.class_index.keys()
    }

    /// Get all CSS custom property names in the project
    pub fn all_custom_properties(&self) -> impl Iterator<Item = &String> {
        self.custom_property_index.keys()
    }

    // ==================== Validation ====================

    /// Validate a single inject key for a file
    pub fn validate_inject(&self, file: &Path, key: &str) -> InjectValidation {
        let Some(info) = self.files.get(file) else {
            return InjectValidation::KeyNotFound;
        };

        let inject = info.injects.iter().find(|i| i.key.as_deref() == Some(key));

        match inject {
            Some(i) if i.is_dynamic_key => InjectValidation::DynamicKey,
            Some(_) => {
                if self.has_providers(key) {
                    let providers: Vec<Arc<Path>> = self.files_providing(key).cloned().collect();
                    InjectValidation::Valid { providers }
                } else {
                    InjectValidation::NoProvider
                }
            }
            None => InjectValidation::KeyNotFound,
        }
    }

    /// Validate all injects in a file
    pub fn validate_file_injects(&self, file: &Path) -> FileInjectValidation {
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
                // Check emptiness first to avoid unnecessary cloning
                if self.has_providers(key) {
                    let providers: Vec<Arc<Path>> = self.files_providing(key).cloned().collect();
                    result.valid.push(InjectValidationEntry {
                        key: key.clone(),
                        providers,
                        start: inject.start,
                        end: inject.end,
                    });
                } else {
                    result.missing_providers.push(InjectValidationEntry {
                        key: key.clone(),
                        providers: Vec::new(),
                        start: inject.start,
                        end: inject.end,
                    });
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

        ComponentUsageSummary {
            component_names,
            usage_counts,
        }
    }

    /// Get project-wide statistics
    pub fn stats(&self) -> ProjectStats {
        let mut stats = ProjectStats {
            file_count: self.files.len(),
            unique_provide_keys: self.provide_index.len(),
            unique_inject_keys: self.inject_index.len(),
            unique_css_classes: self.class_index.len(),
            unique_custom_properties: self.custom_property_index.len(),
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
            if info.has_flag(FileUsageFlags::HAS_SCOPED_STYLE) {
                stats.files_with_scoped_styles += 1;
            }
            if info.has_flag(FileUsageFlags::HAS_CSS_MODULES) {
                stats.files_with_css_modules += 1;
            }
            if info.has_flag(FileUsageFlags::HAS_V_BIND_CSS) {
                stats.files_with_v_bind_css += 1;
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
        self.class_index.clear();
        self.v_bind_css_index.clear();
        self.custom_property_index.clear();
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_usage::{
        ComponentUsageOwned, FileUsageFlags, InjectUsageOwned, ProvideUsageOwned,
        StyleUsageInfoOwned,
    };

    fn make_file_info() -> FileUsageInfoOwned {
        FileUsageInfoOwned::default()
    }

    fn make_file_with_provide(key: &str) -> FileUsageInfoOwned {
        let mut info = make_file_info();
        info.provides.push(ProvideUsageOwned {
            key: Some(key.to_string()),
            is_dynamic_key: false,
            start: 0,
            end: 10,
        });
        info.flags |= FileUsageFlags::HAS_PROVIDE.bits();
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
        info.flags |= FileUsageFlags::HAS_INJECT.bits();
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
        info.flags |= FileUsageFlags::HAS_INJECT.bits();
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
        info.flags |= FileUsageFlags::HAS_COMPONENT_USAGE.bits();
        info
    }

    #[test]
    fn new_index_is_empty() {
        let index = ProjectIndex::new();
        assert_eq!(index.file_count(), 0);
        assert_eq!(index.all_provide_keys().count(), 0);
        assert_eq!(index.all_inject_keys().count(), 0);
    }

    #[test]
    fn add_and_get_file() {
        let mut index = ProjectIndex::new();
        let path = PathBuf::from("src/App.vue");
        let info = make_file_info();

        index.add_file(path.clone(), info);

        assert!(index.contains_file(&path));
        assert_eq!(index.file_count(), 1);
        assert!(index.get_file(&path).is_some());
    }

    #[test]
    fn remove_file() {
        let mut index = ProjectIndex::new();
        let path = PathBuf::from("src/App.vue");
        let info = make_file_with_provide("theme");

        index.add_file(path.clone(), info);
        assert_eq!(index.files_providing("theme").count(), 1);

        let removed = index.remove_file(&path);
        assert!(removed.is_some());
        assert!(!index.contains_file(&path));
        assert_eq!(index.files_providing("theme").count(), 0);
    }

    #[test]
    fn provide_index() {
        let mut index = ProjectIndex::new();

        let app_path = PathBuf::from("src/App.vue");
        let child_path = PathBuf::from("src/Child.vue");

        index.add_file(app_path.clone(), make_file_with_provide("theme"));
        index.add_file(child_path.clone(), make_file_with_provide("theme"));

        let providers: Vec<_> = index.files_providing("theme").collect();
        assert_eq!(providers.len(), 2);
        assert!(providers.iter().any(|p| p.as_ref() == app_path.as_path()));
        assert!(providers.iter().any(|p| p.as_ref() == child_path.as_path()));
    }

    #[test]
    fn inject_index() {
        let mut index = ProjectIndex::new();

        let child1_path = PathBuf::from("src/Child1.vue");
        let child2_path = PathBuf::from("src/Child2.vue");

        index.add_file(child1_path.clone(), make_file_with_inject("theme"));
        index.add_file(child2_path.clone(), make_file_with_inject("theme"));

        assert_eq!(index.files_injecting("theme").count(), 2);
    }

    #[test]
    fn validate_inject_valid() {
        let mut index = ProjectIndex::new();

        let provider_path = PathBuf::from("src/Provider.vue");
        let consumer_path = PathBuf::from("src/Consumer.vue");

        index.add_file(provider_path.clone(), make_file_with_provide("config"));
        index.add_file(consumer_path.clone(), make_file_with_inject("config"));

        let validation = index.validate_inject(&consumer_path, "config");
        match validation {
            InjectValidation::Valid { providers } => {
                assert_eq!(providers.len(), 1);
                assert!(providers
                    .iter()
                    .any(|p| p.as_ref() == provider_path.as_path()));
            }
            _ => panic!("Expected Valid validation"),
        }
    }

    #[test]
    fn validate_inject_no_provider() {
        let mut index = ProjectIndex::new();

        let consumer_path = PathBuf::from("src/Consumer.vue");
        index.add_file(consumer_path.clone(), make_file_with_inject("missing"));

        let validation = index.validate_inject(&consumer_path, "missing");
        assert_eq!(validation, InjectValidation::NoProvider);
    }

    #[test]
    fn validate_inject_dynamic_key() {
        let mut index = ProjectIndex::new();

        let consumer_path = PathBuf::from("src/Consumer.vue");
        let info = make_file_with_dynamic_inject();
        index.add_file(consumer_path.clone(), info);

        let file_validation = index.validate_file_injects(&consumer_path);
        assert_eq!(file_validation.dynamic_keys.len(), 1);
    }

    #[test]
    fn validate_inject_key_not_found() {
        let mut index = ProjectIndex::new();

        let consumer_path = PathBuf::from("src/Consumer.vue");
        index.add_file(consumer_path.clone(), make_file_with_inject("exists"));

        let validation = index.validate_inject(&consumer_path, "nonexistent");
        assert_eq!(validation, InjectValidation::KeyNotFound);
    }

    #[test]
    fn validate_file_injects_mixed() {
        let mut index = ProjectIndex::new();

        let provider_path = PathBuf::from("src/Provider.vue");
        let consumer_path = PathBuf::from("src/Consumer.vue");

        index.add_file(provider_path, make_file_with_provide("provided"));

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
        consumer_info.flags |= FileUsageFlags::HAS_INJECT.bits();
        index.add_file(consumer_path.clone(), consumer_info);

        let validation = index.validate_file_injects(&consumer_path);
        assert_eq!(validation.valid.len(), 1);
        assert_eq!(validation.valid[0].key, "provided");
        assert_eq!(validation.missing_providers.len(), 1);
        assert_eq!(validation.missing_providers[0].key, "missing");
        assert_eq!(validation.dynamic_keys.len(), 1);
    }

    #[test]
    fn component_graph() {
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

        let files_using_header: Vec<_> = index.files_using_component("Header").collect();
        assert_eq!(files_using_header.len(), 1);
        assert!(files_using_header
            .iter()
            .any(|p| p.as_ref() == app_path.as_path()));
    }

    #[test]
    fn provide_inject_summary() {
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
        assert_eq!(summary.provide_keys.len(), 2);
        assert_eq!(summary.inject_keys.len(), 2);
        assert_eq!(summary.unused_provides.len(), 1);
        assert_eq!(summary.missing_provides.len(), 1);
    }

    #[test]
    fn project_stats() {
        let mut index = ProjectIndex::new();

        let mut info1 = make_file_with_provide("theme");
        info1.flags |= FileUsageFlags::HAS_DEFINE_PROPS.bits();
        index.add_file(PathBuf::from("src/App.vue"), info1);

        let mut info2 = make_file_with_inject("theme");
        info2.flags |= (FileUsageFlags::HAS_DEFINE_EMITS | FileUsageFlags::IS_ASYNC_SETUP).bits();
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
    fn update_file_reindexes() {
        let mut index = ProjectIndex::new();
        let path = PathBuf::from("src/App.vue");

        index.add_file(path.clone(), make_file_with_provide("old"));
        assert_eq!(index.files_providing("old").count(), 1);
        assert_eq!(index.files_providing("new").count(), 0);

        index.add_file(path.clone(), make_file_with_provide("new"));
        assert_eq!(index.files_providing("old").count(), 0);
        assert_eq!(index.files_providing("new").count(), 1);
    }

    #[test]
    fn clear_index() {
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
    fn component_usage_summary() {
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
        assert_eq!(summary.component_names.len(), 2);
        assert_eq!(summary.usage_counts.get("Header"), Some(&2));
        assert_eq!(summary.usage_counts.get("Footer"), Some(&1));
    }

    #[test]
    fn file_paths_iterator() {
        let mut index = ProjectIndex::new();
        let path1 = PathBuf::from("src/App.vue");
        let path2 = PathBuf::from("src/Child.vue");

        index.add_file(path1.clone(), make_file_info());
        index.add_file(path2.clone(), make_file_info());

        let paths: Vec<_> = index.file_paths().collect();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|p| p.as_ref() == path1.as_path()));
        assert!(paths.iter().any(|p| p.as_ref() == path2.as_path()));
    }

    // ==================== Style Index Tests ====================

    fn make_file_with_styles(
        class_names: &[&str],
        v_binds: &[&str],
        custom_props: &[&str],
        scoped: bool,
    ) -> FileUsageInfoOwned {
        let mut info = make_file_info();
        info.styles.push(StyleUsageInfoOwned {
            lang: Some("css".to_string()),
            scoped,
            class_names: class_names.iter().map(|s| s.to_string()).collect(),
            v_bind_expressions: v_binds.iter().map(|s| s.to_string()).collect(),
            custom_property_names: custom_props.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        });
        let mut flags = FileUsageFlags::empty();
        if scoped {
            flags |= FileUsageFlags::HAS_SCOPED_STYLE;
        }
        if !v_binds.is_empty() {
            flags |= FileUsageFlags::HAS_V_BIND_CSS;
        }
        info.flags |= flags.bits();
        info
    }

    /// @ai-generated - Class index add and remove
    #[test]
    fn test_class_index_add_remove() {
        let mut index = ProjectIndex::new();
        let path = PathBuf::from("src/App.vue");

        index.add_file(
            path.clone(),
            make_file_with_styles(&["btn", "active"], &[], &[], false),
        );

        assert_eq!(index.files_defining_class("btn").count(), 1);
        assert_eq!(index.files_defining_class("active").count(), 1);
        assert_eq!(index.files_defining_class("missing").count(), 0);

        // Remove file - indexes should be cleaned up
        index.remove_file(&path);
        assert_eq!(index.files_defining_class("btn").count(), 0);
        assert_eq!(index.files_defining_class("active").count(), 0);
    }

    /// @ai-generated - v-bind CSS index
    #[test]
    fn test_v_bind_css_index() {
        let mut index = ProjectIndex::new();

        index.add_file(
            PathBuf::from("src/A.vue"),
            make_file_with_styles(&[], &["color"], &[], false),
        );
        index.add_file(
            PathBuf::from("src/B.vue"),
            make_file_with_styles(&[], &["color", "size"], &[], false),
        );

        assert_eq!(index.files_using_v_bind_css("color").count(), 2);
        assert_eq!(index.files_using_v_bind_css("size").count(), 1);
        assert_eq!(index.files_using_v_bind_css("missing").count(), 0);
    }

    /// @ai-generated - Custom property index
    #[test]
    fn test_custom_property_index() {
        let mut index = ProjectIndex::new();

        index.add_file(
            PathBuf::from("src/A.vue"),
            make_file_with_styles(&[], &[], &["--primary", "--spacing"], false),
        );

        assert_eq!(index.files_defining_custom_property("--primary").count(), 1);
        assert_eq!(index.files_defining_custom_property("--spacing").count(), 1);
        assert_eq!(index.files_defining_custom_property("--missing").count(), 0);

        assert_eq!(index.all_custom_properties().count(), 2);
    }

    /// @ai-generated - Stats include style data
    #[test]
    fn test_stats_with_styles() {
        let mut index = ProjectIndex::new();

        index.add_file(
            PathBuf::from("src/A.vue"),
            make_file_with_styles(&["btn"], &["color"], &["--primary"], true),
        );

        let mut module_info = make_file_info();
        module_info.styles.push(StyleUsageInfoOwned {
            is_module: true,
            class_names: vec!["card".to_string()],
            ..Default::default()
        });
        module_info.flags |= FileUsageFlags::HAS_CSS_MODULES.bits();
        index.add_file(PathBuf::from("src/B.vue"), module_info);

        let stats = index.stats();
        assert_eq!(stats.files_with_scoped_styles, 1);
        assert_eq!(stats.files_with_css_modules, 1);
        assert_eq!(stats.files_with_v_bind_css, 1);
        assert_eq!(stats.unique_css_classes, 2); // "btn" and "card"
        assert_eq!(stats.unique_custom_properties, 1);
    }

    /// @ai-generated - Class index handles multiple files with same class
    #[test]
    fn test_class_index_multiple_files() {
        let mut index = ProjectIndex::new();

        index.add_file(
            PathBuf::from("src/A.vue"),
            make_file_with_styles(&["btn"], &[], &[], false),
        );
        index.add_file(
            PathBuf::from("src/B.vue"),
            make_file_with_styles(&["btn"], &[], &[], false),
        );

        assert_eq!(index.files_defining_class("btn").count(), 2);
        assert_eq!(index.all_class_names().count(), 1);
    }

    /// @ai-generated - Update file re-indexes style data
    #[test]
    fn test_style_reindex_on_update() {
        let mut index = ProjectIndex::new();
        let path = PathBuf::from("src/App.vue");

        index.add_file(
            path.clone(),
            make_file_with_styles(&["old-class"], &[], &[], false),
        );
        assert_eq!(index.files_defining_class("old-class").count(), 1);

        index.add_file(
            path.clone(),
            make_file_with_styles(&["new-class"], &[], &[], false),
        );
        assert_eq!(index.files_defining_class("old-class").count(), 0);
        assert_eq!(index.files_defining_class("new-class").count(), 1);
    }

    /// @ai-generated - Stress test: 200 files with varied data, then update + remove subset
    #[test]
    fn stress_test_200_files() {
        let mut index = ProjectIndex::with_capacity(200);

        // Add 200 files with varied provide/inject/component usage
        for i in 0..200 {
            let path = PathBuf::from(format!("src/components/Comp{i}.vue"));
            let mut info = make_file_info();

            // Every 3rd file provides a key
            if i % 3 == 0 {
                info.provides.push(ProvideUsageOwned {
                    key: Some(format!("key-{}", i % 15)),
                    is_dynamic_key: false,
                    start: 0,
                    end: 10,
                });
                info.flags |= FileUsageFlags::HAS_PROVIDE.bits();
            }

            // Every 4th file injects a key
            if i % 4 == 0 {
                info.injects.push(InjectUsageOwned {
                    key: Some(format!("key-{}", (i + 3) % 15)),
                    is_dynamic_key: false,
                    has_default: false,
                    binding_name: None,
                    start: 0,
                    end: 10,
                });
                info.flags |= FileUsageFlags::HAS_INJECT.bits();
            }

            // Every 2nd file uses a component
            if i % 2 == 0 {
                info.components.push(ComponentUsageOwned {
                    name: Some(format!("Widget{}", i % 10)),
                    is_dynamic: false,
                    start: 0,
                    end: 10,
                });
                info.flags |= FileUsageFlags::HAS_COMPONENT_USAGE.bits();
            }

            // Some files have styles
            if i % 5 == 0 {
                info.styles.push(StyleUsageInfoOwned {
                    class_names: vec![format!("cls-{}", i % 20)],
                    custom_property_names: vec![format!("--var-{}", i % 10)],
                    scoped: i % 10 == 0,
                    ..Default::default()
                });
                if i % 10 == 0 {
                    info.flags |= FileUsageFlags::HAS_SCOPED_STYLE.bits();
                }
            }

            index.add_file(path, info);
        }

        assert_eq!(index.file_count(), 200);

        let stats_before = index.stats();
        assert_eq!(stats_before.file_count, 200);
        assert!(stats_before.files_with_provide > 0);
        assert!(stats_before.files_with_inject > 0);
        assert!(stats_before.unique_provide_keys > 0);

        let summary = index.provide_inject_summary();
        assert!(!summary.provide_keys.is_empty());
        assert!(!summary.inject_keys.is_empty());

        // Update 20 files (change their provide keys)
        for i in (0..200).step_by(10) {
            let path = PathBuf::from(format!("src/components/Comp{i}.vue"));
            let mut info = make_file_info();
            info.provides.push(ProvideUsageOwned {
                key: Some(format!("updated-key-{}", i % 5)),
                is_dynamic_key: false,
                start: 0,
                end: 10,
            });
            info.flags |= FileUsageFlags::HAS_PROVIDE.bits();
            index.add_file(path, info);
        }

        assert_eq!(
            index.file_count(),
            200,
            "file count should stay the same after updates"
        );

        // Remove 50 files
        for i in 0..50 {
            let path = PathBuf::from(format!("src/components/Comp{i}.vue"));
            index.remove_file(&path);
        }

        assert_eq!(index.file_count(), 150);

        let stats_after = index.stats();
        assert_eq!(stats_after.file_count, 150);

        // Summary should still be consistent
        let summary_after = index.provide_inject_summary();
        // Provide keys in index should match what files actually provide
        for key in &summary_after.provide_keys {
            assert!(
                index.has_providers(key),
                "provide key '{key}' should have at least one provider"
            );
        }
        for key in &summary_after.inject_keys {
            assert!(
                index.has_injectors(key),
                "inject key '{key}' should have at least one injector"
            );
        }
    }

    /// @ai-generated - validate_inject on unindexed file returns KeyNotFound
    #[test]
    fn validate_inject_unindexed_file() {
        let index = ProjectIndex::new();
        let path = PathBuf::from("src/Unknown.vue");
        let result = index.validate_inject(&path, "anything");
        assert_eq!(
            result,
            InjectValidation::KeyNotFound,
            "validate_inject on unindexed file should return KeyNotFound"
        );
    }

    /// @ai-generated - validate_file_injects on unindexed file returns default (empty)
    #[test]
    fn validate_file_injects_unindexed_file() {
        let index = ProjectIndex::new();
        let path = PathBuf::from("src/Unknown.vue");
        let result = index.validate_file_injects(&path);
        assert!(result.valid.is_empty());
        assert!(result.missing_providers.is_empty());
        assert!(result.dynamic_keys.is_empty());
    }
}
