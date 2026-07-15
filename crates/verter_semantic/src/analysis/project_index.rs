//! Cross-file project index for static analysis.
//!
//! Aggregates file usage information to enable cross-file queries such as:
//! - Validating inject() calls have matching provide() in the project
//! - Building component dependency graphs
//! - Tracking provide/inject key usage across the project

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::analysis::file_usage::{FileUsageFlags, FileUsageInfoOwned};
use crate::analysis::routes::{RouteAnalysisSnapshot, RouteDefinition};

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

    /// CSS ID selector → files that define it
    id_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// v-bind CSS expression → files that use it
    v_bind_css_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// CSS custom property name → files that define it
    custom_property_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// CSS variable name → files that reference it via `var()`
    var_reference_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// CSS variable name → files that set it in template `:style` bindings
    template_css_var_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// CSS variable name → files that manipulate it via script DOM APIs
    script_css_var_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// Event name → files that declare it via defineEmits
    emit_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// Event name → files that listen for it on child components
    listener_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// Template `id` attribute value → files that define it
    template_id_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// Route analysis snapshot (populated by `set_route_snapshot`)
    route_snapshot: Option<RouteAnalysisSnapshot>,

    /// Store composable/callee → consumer files
    store_usage_index: FxHashMap<String, FxHashSet<Arc<Path>>>,

    /// Store ID → defining file
    store_definition_index: FxHashMap<String, Arc<Path>>,

    /// Store ID → dependent store IDs (from store_dependencies)
    store_dep_graph: FxHashMap<String, Vec<String>>,
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

/// Cross-component CSS variable flow information.
///
/// Tracks where a CSS variable is defined, referenced, and manipulated
/// across all SFC blocks (style, template, script) in the project.
#[derive(Debug, Clone, Default)]
pub struct CssVarFlow {
    /// The CSS variable name (e.g., "--theme-color").
    pub name: String,
    /// Files that define this variable in `<style>` blocks (`--name: value`).
    pub style_definitions: Vec<Arc<Path>>,
    /// Files that reference this variable via `var(--name)` in `<style>` blocks.
    pub style_var_usages: Vec<Arc<Path>>,
    /// Files that set this variable via `:style` bindings in `<template>`.
    pub template_definitions: Vec<Arc<Path>>,
    /// Files that manipulate this variable via DOM APIs in `<script>`.
    pub script_manipulations: Vec<Arc<Path>>,
}

/// Cross-file event flow tracing result.
///
/// Traces where an event is emitted (via `defineEmits`) and where it's
/// listened (via `@eventName` on child components) across the project.
#[derive(Debug, Clone, Default)]
pub struct EventFlow {
    /// The event name being traced.
    pub event_name: String,
    /// Files that declare this event in `defineEmits`.
    pub emitters: Vec<Arc<Path>>,
    /// Files that listen for this event on child components.
    pub listeners: Vec<Arc<Path>>,
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

/// Cross-file store flow tracing result.
#[derive(Debug, Clone, Default)]
pub struct StoreFlow {
    /// The store ID being traced.
    pub store_id: String,
    /// File that defines this store (via `defineStore`/`createStore`).
    pub definition_file: Option<Arc<Path>>,
    /// Files that consume this store.
    pub consumer_files: Vec<Arc<Path>>,
    /// Other stores this store depends on.
    pub depends_on: Vec<String>,
    /// Stores that depend on this store.
    pub depended_by: Vec<String>,
}

/// Summary of store usage across the project.
#[derive(Debug, Clone, Default)]
pub struct StoreUsageSummary {
    /// All store IDs defined in the project.
    pub store_ids: Vec<String>,
    /// All store composable callee names used.
    pub store_callees: Vec<String>,
    /// Number of files using each store callee.
    pub usage_counts: FxHashMap<String, usize>,
    /// Store IDs defined but never used.
    pub unused_stores: Vec<String>,
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
    /// Total unique CSS variable references (via `var()`)
    pub unique_var_references: usize,
    /// Custom properties defined but never referenced via `var()`
    pub unreferenced_custom_properties: usize,
    /// `var()` references with no matching custom property definition
    pub unresolved_var_references: usize,
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
            id_index: FxHashMap::default(),
            v_bind_css_index: FxHashMap::default(),
            custom_property_index: FxHashMap::default(),
            var_reference_index: FxHashMap::default(),
            template_css_var_index: FxHashMap::default(),
            script_css_var_index: FxHashMap::default(),
            emit_index: FxHashMap::default(),
            listener_index: FxHashMap::default(),
            template_id_index: FxHashMap::default(),
            route_snapshot: None,
            store_usage_index: FxHashMap::default(),
            store_definition_index: FxHashMap::default(),
            store_dep_graph: FxHashMap::default(),
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
            for id_name in &style.id_names {
                self.id_index
                    .entry(id_name.clone())
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
            for var_ref in &style.var_reference_names {
                self.var_reference_index
                    .entry(var_ref.clone())
                    .or_default()
                    .insert(Arc::clone(&path));
            }
        }

        // Index template CSS variable names
        for name in &info.template_css_var_names {
            self.template_css_var_index
                .entry(name.clone())
                .or_default()
                .insert(Arc::clone(&path));
        }

        // Index script CSS variable names
        for name in &info.script_css_var_names {
            self.script_css_var_index
                .entry(name.clone())
                .or_default()
                .insert(Arc::clone(&path));
        }

        // Index emit declarations (from defineEmits)
        for emit in &info.emit_declarations {
            self.emit_index
                .entry(emit.event_name.clone())
                .or_default()
                .insert(Arc::clone(&path));
        }

        // Index listened events (from @event on child components)
        for listened in &info.listened_events {
            self.listener_index
                .entry(listened.event_name.clone())
                .or_default()
                .insert(Arc::clone(&path));
        }

        // Index template id attributes (for teleport target lookup)
        for id in &info.template_ids {
            self.template_id_index
                .entry(id.id.clone())
                .or_default()
                .insert(Arc::clone(&path));
        }

        // Index store usages (callee → consumer files)
        for usage in &info.store_usages {
            self.store_usage_index
                .entry(usage.callee.clone())
                .or_default()
                .insert(Arc::clone(&path));
        }

        // Index store definitions (store_id → defining file)
        for def in &info.store_definitions {
            if let Some(store_id) = &def.store_id {
                self.store_definition_index
                    .insert(store_id.clone(), Arc::clone(&path));
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
            for id_name in &style.id_names {
                if let Some(files) = self.id_index.get_mut(id_name) {
                    files.remove(&arc_path);
                    if files.is_empty() {
                        self.id_index.remove(id_name);
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
            for var_ref in &style.var_reference_names {
                if let Some(files) = self.var_reference_index.get_mut(var_ref) {
                    files.remove(&arc_path);
                    if files.is_empty() {
                        self.var_reference_index.remove(var_ref);
                    }
                }
            }
        }

        // Remove from template CSS variable index
        for name in &info.template_css_var_names {
            if let Some(files) = self.template_css_var_index.get_mut(name) {
                files.remove(&arc_path);
                if files.is_empty() {
                    self.template_css_var_index.remove(name);
                }
            }
        }

        // Remove from script CSS variable index
        for name in &info.script_css_var_names {
            if let Some(files) = self.script_css_var_index.get_mut(name) {
                files.remove(&arc_path);
                if files.is_empty() {
                    self.script_css_var_index.remove(name);
                }
            }
        }

        // Remove from emit index
        for emit in &info.emit_declarations {
            if let Some(files) = self.emit_index.get_mut(&emit.event_name) {
                files.remove(&arc_path);
                if files.is_empty() {
                    self.emit_index.remove(&emit.event_name);
                }
            }
        }

        // Remove from listener index
        for listened in &info.listened_events {
            if let Some(files) = self.listener_index.get_mut(&listened.event_name) {
                files.remove(&arc_path);
                if files.is_empty() {
                    self.listener_index.remove(&listened.event_name);
                }
            }
        }

        // Remove from template id index
        for id in &info.template_ids {
            if let Some(files) = self.template_id_index.get_mut(&id.id) {
                files.remove(&arc_path);
                if files.is_empty() {
                    self.template_id_index.remove(&id.id);
                }
            }
        }

        // Remove from store indexes
        for usage in &info.store_usages {
            if let Some(files) = self.store_usage_index.get_mut(&usage.callee) {
                files.remove(&arc_path);
                if files.is_empty() {
                    self.store_usage_index.remove(&usage.callee);
                }
            }
        }
        for def in &info.store_definitions {
            if let Some(store_id) = &def.store_id {
                self.store_definition_index.remove(store_id);
                self.store_dep_graph.remove(store_id);
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

    /// Get files that define a given CSS ID selector
    pub fn files_defining_id(&self, name: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.id_index.get(name).into_iter().flat_map(|s| s.iter())
    }

    /// Get all CSS ID names in the project
    pub fn all_id_names(&self) -> impl Iterator<Item = &String> {
        self.id_index.keys()
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

    /// Get files that reference a CSS variable via `var()` in style blocks
    pub fn files_referencing_custom_property(
        &self,
        name: &str,
    ) -> impl Iterator<Item = &Arc<Path>> {
        self.var_reference_index
            .get(name)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Get files that set a CSS variable in template `:style` bindings
    pub fn files_setting_css_var_in_template(
        &self,
        name: &str,
    ) -> impl Iterator<Item = &Arc<Path>> {
        self.template_css_var_index
            .get(name)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Get files that manipulate a CSS variable via script DOM APIs
    pub fn files_manipulating_css_var_in_script(
        &self,
        name: &str,
    ) -> impl Iterator<Item = &Arc<Path>> {
        self.script_css_var_index
            .get(name)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    // ==================== Event Flow Queries ====================

    /// Get files that declare a given event via defineEmits.
    pub fn files_emitting(&self, event_name: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.emit_index
            .get(event_name)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Get files that listen for a given event on child components.
    pub fn files_listening(&self, event_name: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.listener_index
            .get(event_name)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Trace the cross-file flow of an event.
    pub fn event_flow(&self, event_name: &str) -> EventFlow {
        EventFlow {
            event_name: event_name.to_string(),
            emitters: self.files_emitting(event_name).cloned().collect(),
            listeners: self.files_listening(event_name).cloned().collect(),
        }
    }

    /// Get all event names declared via defineEmits in the project.
    pub fn all_emit_names(&self) -> impl Iterator<Item = &String> {
        self.emit_index.keys()
    }

    /// Get all event names listened on child components in the project.
    pub fn all_listened_event_names(&self) -> impl Iterator<Item = &String> {
        self.listener_index.keys()
    }

    // ==================== Store Queries ====================

    /// Get files that use a given store composable (by callee name).
    pub fn files_using_store(&self, callee: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.store_usage_index
            .get(callee)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Get the file that defines a store (by store ID).
    pub fn store_defined_in(&self, store_id: &str) -> Option<&Arc<Path>> {
        self.store_definition_index.get(store_id)
    }

    /// Get stores that the given store depends on.
    pub fn store_dependencies(&self, store_id: &str) -> &[String] {
        self.store_dep_graph
            .get(store_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all store IDs defined in the project.
    pub fn all_store_ids(&self) -> impl Iterator<Item = &String> {
        self.store_definition_index.keys()
    }

    /// Get all store composable callee names used in the project.
    pub fn all_store_callees(&self) -> impl Iterator<Item = &String> {
        self.store_usage_index.keys()
    }

    /// Trace a store: definition file, all consumers, dependency chain.
    pub fn store_flow(&self, store_id: &str) -> StoreFlow {
        let definition_file = self.store_definition_index.get(store_id).cloned();
        let depends_on = self
            .store_dep_graph
            .get(store_id)
            .cloned()
            .unwrap_or_default();
        let depended_by: Vec<String> = self
            .store_dep_graph
            .iter()
            .filter(|(_, deps)| deps.contains(&store_id.to_string()))
            .map(|(id, _)| id.clone())
            .collect();
        let consumer_files: Vec<Arc<Path>> = self
            .store_usage_index
            .iter()
            .filter(|(_, files)| !files.is_empty())
            .flat_map(|(_, files)| files.iter().cloned())
            .collect();

        StoreFlow {
            store_id: store_id.to_string(),
            definition_file,
            consumer_files,
            depends_on,
            depended_by,
        }
    }

    /// Get a summary of store usage across the project.
    pub fn store_usage_summary(&self) -> StoreUsageSummary {
        let store_ids: Vec<String> = self.store_definition_index.keys().cloned().collect();
        let mut usage_counts = FxHashMap::default();
        for (callee, files) in &self.store_usage_index {
            usage_counts.insert(callee.clone(), files.len());
        }
        // A store is unused if no callee in the usage index references it
        // (simplistic: checks if the store_id appears in any callee's consumer set)
        let all_used_callees: FxHashSet<&str> =
            self.store_usage_index.keys().map(|s| s.as_str()).collect();
        let unused_stores: Vec<String> = store_ids
            .iter()
            .filter(|id| !all_used_callees.iter().any(|c| c.contains(id.as_str())))
            .cloned()
            .collect();

        StoreUsageSummary {
            store_ids,
            store_callees: self.store_usage_index.keys().cloned().collect(),
            usage_counts,
            unused_stores,
        }
    }

    // ==================== Template ID Queries ====================

    /// Get files that define a given template `id` attribute value.
    pub fn files_with_template_id(&self, id: &str) -> impl Iterator<Item = &Arc<Path>> {
        self.template_id_index
            .get(id)
            .into_iter()
            .flat_map(|s| s.iter())
    }

    /// Check if any file defines a given template `id` attribute value.
    pub fn has_template_id(&self, id: &str) -> bool {
        self.template_id_index
            .get(id)
            .is_some_and(|s| !s.is_empty())
    }

    // ==================== CSS Variable Queries ====================

    /// Get the cross-component flow for a CSS variable.
    pub fn css_var_flow(&self, name: &str) -> CssVarFlow {
        CssVarFlow {
            name: name.to_string(),
            style_definitions: self.files_defining_custom_property(name).cloned().collect(),
            style_var_usages: self
                .files_referencing_custom_property(name)
                .cloned()
                .collect(),
            template_definitions: self
                .files_setting_css_var_in_template(name)
                .cloned()
                .collect(),
            script_manipulations: self
                .files_manipulating_css_var_in_script(name)
                .cloned()
                .collect(),
        }
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
            unique_var_references: self.var_reference_index.len(),
            unreferenced_custom_properties: self
                .custom_property_index
                .keys()
                .filter(|name| !self.var_reference_index.contains_key(name.as_str()))
                .count(),
            unresolved_var_references: self
                .var_reference_index
                .keys()
                .filter(|name| !self.custom_property_index.contains_key(name.as_str()))
                .count(),
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

    // ==================== Route Analysis ====================

    /// Set the route analysis snapshot for this project.
    pub fn set_route_snapshot(&mut self, snapshot: RouteAnalysisSnapshot) {
        self.route_snapshot = Some(snapshot);
    }

    /// Get the route analysis snapshot.
    pub fn route_snapshot(&self) -> Option<&RouteAnalysisSnapshot> {
        self.route_snapshot.as_ref()
    }

    /// Find which routes render a given component file path.
    pub fn routes_for_component(&self, path: &str) -> Vec<&RouteDefinition> {
        let Some(snapshot) = &self.route_snapshot else {
            return Vec::new();
        };
        let flat = crate::analysis::routes::flatten_routes(&snapshot.routes);
        flat.into_iter()
            .filter(|r| r.component_path.as_deref() == Some(path))
            .collect()
    }

    /// Flatten all routes (including nested children) into a flat list.
    pub fn flatten_routes(&self) -> Vec<&RouteDefinition> {
        match &self.route_snapshot {
            Some(snapshot) => crate::analysis::routes::flatten_routes(&snapshot.routes),
            None => Vec::new(),
        }
    }

    /// Clear all indexed data
    pub fn clear(&mut self) {
        self.files.clear();
        self.provide_index.clear();
        self.inject_index.clear();
        self.component_graph.clear();
        self.component_reverse_index.clear();
        self.class_index.clear();
        self.id_index.clear();
        self.v_bind_css_index.clear();
        self.custom_property_index.clear();
        self.emit_index.clear();
        self.listener_index.clear();
        self.template_id_index.clear();
        self.route_snapshot = None;
        self.store_usage_index.clear();
        self.store_definition_index.clear();
        self.store_dep_graph.clear();
    }
}

#[cfg(test)]
#[path = "project_index_tests.rs"]
mod project_index_tests;
