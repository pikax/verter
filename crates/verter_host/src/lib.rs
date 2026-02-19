use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use oxc_allocator::Allocator;
use sha2::{Digest, Sha256};
use thiserror::Error;
use verter_core::builder::codegen::CodegenOptions;
use verter_core::new_impl::compile::{
    compile as compile_sfc, VerterCompileOptions, VerterCompileResult,
};
use verter_core::new_impl::syntax::Syntax;
use verter_core::new_impl::types::NodeProp;
use verter_core::syntax::plugin::{DiagnosticSeverity, SyntaxPluginContext, SyntaxPluginOptions};
use verter_core::tokenizer::byte::tokenize;

#[cfg(feature = "single_threaded")]
type Shared<T> = std::cell::RefCell<T>;
#[cfg(not(feature = "single_threaded"))]
type Shared<T> = std::sync::RwLock<T>;

#[cfg(feature = "single_threaded")]
fn read_lock<T>(lock: &Shared<T>) -> std::cell::Ref<'_, T> {
    lock.borrow()
}
#[cfg(not(feature = "single_threaded"))]
fn read_lock<T>(lock: &Shared<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().expect("rwlock poisoned")
}

#[cfg(feature = "single_threaded")]
fn write_lock<T>(lock: &Shared<T>) -> std::cell::RefMut<'_, T> {
    lock.borrow_mut()
}
#[cfg(not(feature = "single_threaded"))]
fn write_lock<T>(lock: &Shared<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().expect("rwlock poisoned")
}

type Hash16 = [u8; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    VueSfc,
    NonSfc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HmrStrategy {
    None,
    Vite,
    Webpack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileErrorPolicy {
    StrictError,
    DevServeLastKnownGood,
}

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub dev_mode: bool,
    pub compile_error_policy: CompileErrorPolicy,
    pub lsp_scheme: String,
    pub max_profiles_per_file: usize,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            dev_mode: true,
            compile_error_policy: CompileErrorPolicy::DevServeLastKnownGood,
            lsp_scheme: "verter-virtual".to_string(),
            max_profiles_per_file: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompileProfile {
    pub filename: Option<String>,
    pub is_production: bool,
    pub ssr: bool,
    pub hmr_strategy: HmrStrategy,
    pub component_id: Option<String>,
    pub delimiters: Option<(String, String)>,
    pub custom_elements: Option<Vec<String>>,
    pub comments: Option<bool>,
    pub runtime_module_name: Option<String>,
    pub force_vapor: bool,
    pub strip_ts: bool,
    pub source_map: bool,
}

impl Default for CompileProfile {
    fn default() -> Self {
        Self {
            filename: None,
            is_production: false,
            ssr: false,
            hmr_strategy: HmrStrategy::None,
            component_id: None,
            delimiters: None,
            custom_elements: None,
            comments: None,
            runtime_module_name: Some("vue".to_string()),
            force_vapor: false,
            strip_ts: false,
            source_map: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VirtualNodeKind {
    Main,
    Script,
    Template,
    Style { index: usize },
    Custom { index: usize },
}

#[derive(Debug, Clone)]
pub struct ExternalSourceRequest {
    pub owner_canonical_id: String,
    pub block_kind: ExternalBlockKind,
    pub index: usize,
    pub specifier: String,
    pub resolved_canonical_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalBlockKind {
    Script,
    Template,
    Style,
    Custom,
}

#[derive(Debug, Clone, Default)]
pub struct SliceChanges {
    pub script_changed: bool,
    pub template_changed: bool,
    pub style_indices_changed: Vec<usize>,
    pub custom_indices_changed: Vec<usize>,
    pub structure_changed: bool,
    pub descriptor_changed: bool,
}

#[derive(Debug, Clone)]
pub struct HostUpdateResult {
    pub canonical_id: String,
    pub changed: bool,
    pub slice_changes: SliceChanges,
    pub changed_virtual_nodes: Vec<VirtualNodeKind>,
    pub removed_virtual_nodes: Vec<VirtualNodeKind>,
    pub changed_virtual_ids: Vec<String>,
    pub removed_virtual_ids: Vec<String>,
    pub changed_lsp_ids: Vec<String>,
    pub removed_lsp_ids: Vec<String>,
    pub diagnostics: DiagnosticsSnapshot,
    pub external_source_requests: Vec<ExternalSourceRequest>,
}

#[derive(Debug, Clone)]
pub struct ResolvedId {
    pub canonical_id: String,
    pub node_kind: VirtualNodeKind,
    pub exists_in_host: bool,
    pub bundler_id: String,
    pub lsp_id: String,
}

#[derive(Debug, Clone)]
pub struct VirtualQuery {
    pub raw_id: Option<String>,
    pub canonical_id: Option<String>,
    pub node_kind: Option<VirtualNodeKind>,
    pub compile_profile: CompileProfile,
}

#[derive(Debug, Clone, Default)]
pub struct VirtualMeta {
    pub scope_id: Option<String>,
    pub block_type: Option<String>,
    pub style_index: Option<usize>,
    pub custom_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct VirtualFileResponse {
    pub id: String,
    pub code: Arc<str>,
    pub source_map: Option<Arc<str>>,
    pub lang: Option<String>,
    pub stale: bool,
    pub diagnostics: DiagnosticsSnapshot,
    pub meta: VirtualMeta,
}

#[derive(Debug, Clone)]
pub struct UpsertRequest {
    pub canonical_id: Option<String>,
    pub input_id: String,
    pub source: Arc<str>,
    pub file_kind: FileKind,
    pub aliases: Vec<String>,
    pub compile_profile: CompileProfile,
}

#[derive(Debug, Clone)]
pub struct StyleOverrideEntry {
    pub index: usize,
    pub code: Arc<str>,
    pub source_map: Option<Arc<str>>,
}

#[derive(Debug, Clone)]
pub struct StyleOverrideRequest {
    pub canonical_id: String,
    pub compile_profile: CompileProfile,
    pub overrides: Vec<StyleOverrideEntry>,
}

#[derive(Debug, Clone)]
pub struct HostRemoveResult {
    pub canonical_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct HostDiagnostic {
    pub severity: HostSeverity,
    pub code: String,
    pub message: String,
    pub span_start: Option<u32>,
    pub span_end: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticsSnapshot {
    pub diagnostics: Vec<HostDiagnostic>,
    pub has_errors: bool,
}

impl DiagnosticsSnapshot {
    fn from_vec(diagnostics: Vec<HostDiagnostic>) -> Self {
        let has_errors = diagnostics
            .iter()
            .any(|d| d.severity == HostSeverity::Error);
        Self {
            diagnostics,
            has_errors,
        }
    }

    fn merge(mut self, mut other: DiagnosticsSnapshot) -> Self {
        self.diagnostics.append(&mut other.diagnostics);
        self.has_errors = self.has_errors || other.has_errors;
        self
    }
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error("missing source for canonical id '{canonical_id}'")]
    MissingSource { canonical_id: String },
    #[error("invalid virtual query")]
    InvalidQuery,
    #[error("missing virtual node for id '{canonical_id}'")]
    MissingVirtualNode { canonical_id: String },
    #[error("compile error")]
    CompileError { diagnostics: DiagnosticsSnapshot },
}

#[derive(Debug, Clone)]
struct ParsedRawId {
    canonical_id: String,
    node_kind: VirtualNodeKind,
    was_lsp_like: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DescriptorMin {
    script_count: usize,
    template_count: usize,
    style_count: usize,
    custom_count: usize,
    script_attr_fingerprints: Vec<String>,
    template_attr_fingerprints: Vec<String>,
    style_attr_fingerprints: Vec<String>,
    custom_attr_fingerprints: Vec<String>,
    vapor: bool,
}

#[derive(Debug, Clone, Default)]
struct SliceHashes {
    script: Option<Hash16>,
    template: Option<Hash16>,
    styles: Vec<Hash16>,
    custom: Vec<Hash16>,
}

#[derive(Debug, Clone, Default)]
struct FileMeta {
    has_script: bool,
    has_template: bool,
    style_langs: Vec<Option<String>>,
    custom_types: Vec<String>,
    custom_langs: Vec<Option<String>>,
}

#[derive(Debug, Clone)]
struct SrcBlockInfo {
    tag_name: String,
    resolved_canonical_id: String,
    tag_open_start: u32,
    tag_open_end: u32,
    tag_close_start: Option<u32>,
}

#[derive(Debug, Clone)]
struct ParseSnapshot {
    whole_hash: Hash16,
    semantic_hash: Hash16,
    slices: SliceHashes,
    descriptor: DescriptorMin,
    meta: FileMeta,
    external_requests: Vec<ExternalSourceRequest>,
    src_blocks: Vec<SrcBlockInfo>,
    parse_diagnostics: DiagnosticsSnapshot,
}

#[derive(Debug, Clone)]
struct StyleOverrideLayer {
    hash: u64,
    by_index: HashMap<usize, StyleOverrideEntry>,
}

#[derive(Debug, Clone)]
struct CachedVirtualFile {
    code: Arc<str>,
    source_map: Option<Arc<str>>,
    lang: Option<String>,
    meta: VirtualMeta,
}

#[derive(Debug, Clone)]
struct CompileSlot {
    semantic_hash: Hash16,
    style_override_hash: u64,
    outputs: HashMap<VirtualNodeKind, CachedVirtualFile>,
    diagnostics: DiagnosticsSnapshot,
    last_good_outputs: Option<HashMap<VirtualNodeKind, CachedVirtualFile>>,
    last_access_tick: u64,
}

#[derive(Debug, Clone)]
struct FileEntry {
    canonical_id: String,
    file_kind: FileKind,
    source: Arc<str>,
    whole_hash: Hash16,
    semantic_hash: Hash16,
    slices: SliceHashes,
    descriptor: DescriptorMin,
    meta: FileMeta,
    aliases: BTreeSet<String>,
    dependencies: BTreeSet<String>,
    external_requests: Vec<ExternalSourceRequest>,
    src_blocks: Vec<SrcBlockInfo>,
    parse_diagnostics: DiagnosticsSnapshot,
    style_overrides: HashMap<u64, StyleOverrideLayer>,
    compile_slots: HashMap<u64, CompileSlot>,
    latest_diagnostics: HashMap<u64, DiagnosticsSnapshot>,
    generation: u64,
}

impl FileEntry {
    fn all_virtual_nodes(&self) -> Vec<VirtualNodeKind> {
        let mut nodes = vec![VirtualNodeKind::Main];
        if self.meta.has_script {
            nodes.push(VirtualNodeKind::Script);
        }
        if self.meta.has_template {
            nodes.push(VirtualNodeKind::Template);
        }
        for i in 0..self.meta.style_langs.len() {
            nodes.push(VirtualNodeKind::Style { index: i });
        }
        for i in 0..self.meta.custom_types.len() {
            nodes.push(VirtualNodeKind::Custom { index: i });
        }
        nodes
    }
}

#[derive(Debug, Default)]
#[cfg(feature = "host_metrics")]
pub struct HostMetricsSnapshot {
    pub upserts: u64,
    pub compile_requests: u64,
    pub compile_cache_hits: u64,
    pub compile_cache_hit_rate: f64,
    pub virtual_loads: u64,
    pub resolves: u64,
    pub style_override_calls: u64,
    pub slice_hash_time_us_total: u64,
    pub avg_slice_hash_time_us: f64,
    pub compile_time_us_total: u64,
    pub compile_time_us_total_by_profile: BTreeMap<u64, u64>,
    pub compile_count_by_profile: BTreeMap<u64, u64>,
}

#[derive(Debug, Default)]
#[cfg(feature = "host_metrics")]
struct HostMetrics {
    upserts: std::sync::atomic::AtomicU64,
    compile_requests: std::sync::atomic::AtomicU64,
    compile_cache_hits: std::sync::atomic::AtomicU64,
    virtual_loads: std::sync::atomic::AtomicU64,
    resolves: std::sync::atomic::AtomicU64,
    style_override_calls: std::sync::atomic::AtomicU64,
    slice_hash_time_us_total: std::sync::atomic::AtomicU64,
    compile_time_us_total: std::sync::atomic::AtomicU64,
    compile_time_us_total_by_profile: std::sync::Mutex<HashMap<u64, u64>>,
    compile_count_by_profile: std::sync::Mutex<HashMap<u64, u64>>,
}

#[derive(Debug)]
pub struct VerterHost {
    config: HostConfig,
    files: Shared<HashMap<String, FileEntry>>,
    alias_to_canonical: Shared<HashMap<String, String>>,
    reverse_dependencies: Shared<HashMap<String, BTreeSet<String>>>,
    tick: std::sync::atomic::AtomicU64,
    #[cfg(feature = "host_metrics")]
    metrics: HostMetrics,
}

impl VerterHost {
    pub fn new(config: HostConfig) -> Self {
        Self {
            config,
            files: default_shared(HashMap::new()),
            alias_to_canonical: default_shared(HashMap::new()),
            reverse_dependencies: default_shared(HashMap::new()),
            tick: std::sync::atomic::AtomicU64::new(1),
            #[cfg(feature = "host_metrics")]
            metrics: HostMetrics::default(),
        }
    }

    #[cfg(feature = "host_metrics")]
    pub fn metrics_snapshot(&self) -> HostMetricsSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        let upserts = self.metrics.upserts.load(Relaxed);
        let compile_requests = self.metrics.compile_requests.load(Relaxed);
        let compile_cache_hits = self.metrics.compile_cache_hits.load(Relaxed);
        let slice_hash_time_us_total = self.metrics.slice_hash_time_us_total.load(Relaxed);
        let compile_time_us_total = self.metrics.compile_time_us_total.load(Relaxed);

        let compile_time_us_total_by_profile: BTreeMap<u64, u64> = self
            .metrics
            .compile_time_us_total_by_profile
            .lock()
            .expect("metrics lock poisoned")
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        let compile_count_by_profile: BTreeMap<u64, u64> = self
            .metrics
            .compile_count_by_profile
            .lock()
            .expect("metrics lock poisoned")
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();

        HostMetricsSnapshot {
            upserts,
            compile_requests,
            compile_cache_hits,
            compile_cache_hit_rate: if compile_requests == 0 {
                0.0
            } else {
                compile_cache_hits as f64 / compile_requests as f64
            },
            virtual_loads: self.metrics.virtual_loads.load(Relaxed),
            resolves: self.metrics.resolves.load(Relaxed),
            style_override_calls: self.metrics.style_override_calls.load(Relaxed),
            slice_hash_time_us_total,
            avg_slice_hash_time_us: if upserts == 0 {
                0.0
            } else {
                slice_hash_time_us_total as f64 / upserts as f64
            },
            compile_time_us_total,
            compile_time_us_total_by_profile,
            compile_count_by_profile,
        }
    }

    pub fn resolve(&self, raw_id: &str) -> Option<ResolvedId> {
        #[cfg(feature = "host_metrics")]
        self.metrics
            .resolves
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let parsed = parse_raw_id(raw_id)?;
        let canonical = self.resolve_alias_or_canonical(&parsed.canonical_id);
        let exists = read_lock(&self.files).contains_key(&canonical);
        let file_meta = read_lock(&self.files)
            .get(&canonical)
            .map(|f| f.meta.clone())
            .unwrap_or_default();
        let (bundler_id, lsp_id) = render_ids(&canonical, &parsed.node_kind, &file_meta);
        Some(ResolvedId {
            canonical_id: canonical,
            node_kind: parsed.node_kind,
            exists_in_host: exists,
            bundler_id,
            lsp_id,
        })
    }

    pub fn upsert(&self, req: UpsertRequest) -> Result<HostUpdateResult, HostError> {
        #[cfg(feature = "host_metrics")]
        self.metrics
            .upserts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let canonical_id = req
            .canonical_id
            .clone()
            .unwrap_or_else(|| canonicalize_id(&req.input_id));

        #[cfg(feature = "host_metrics")]
        let start_hash = std::time::Instant::now();
        let snapshot = match req.file_kind {
            FileKind::VueSfc => parse_vue_snapshot(&canonical_id, &req.source),
            FileKind::NonSfc => parse_non_sfc_snapshot(&canonical_id, &req.source),
        };
        #[cfg(feature = "host_metrics")]
        self.metrics.slice_hash_time_us_total.fetch_add(
            start_hash.elapsed().as_micros() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );

        let mut old_entry: Option<FileEntry> = None;
        {
            let files = read_lock(&self.files);
            if let Some(existing) = files.get(&canonical_id) {
                old_entry = Some(existing.clone());
            }
        }

        let mut slice_changes = SliceChanges::default();
        let changed = if let Some(old) = &old_entry {
            if old.whole_hash == snapshot.whole_hash {
                false
            } else {
                slice_changes.script_changed = old.slices.script != snapshot.slices.script;
                slice_changes.template_changed = old.slices.template != snapshot.slices.template;
                slice_changes.style_indices_changed =
                    diff_indices(&old.slices.styles, &snapshot.slices.styles);
                slice_changes.custom_indices_changed =
                    diff_indices(&old.slices.custom, &snapshot.slices.custom);
                slice_changes.structure_changed = old.descriptor.script_count
                    != snapshot.descriptor.script_count
                    || old.descriptor.template_count != snapshot.descriptor.template_count
                    || old.descriptor.style_count != snapshot.descriptor.style_count
                    || old.descriptor.custom_count != snapshot.descriptor.custom_count;
                slice_changes.descriptor_changed = old.descriptor != snapshot.descriptor;

                !(old.semantic_hash != snapshot.semantic_hash)
                    .then_some(())
                    .is_none()
            }
        } else {
            true
        };

        // change detection rule: if whole hash changed but semantic hash stayed, no invalidation.
        if let Some(old) = &old_entry {
            if old.whole_hash != snapshot.whole_hash
                && old.semantic_hash == snapshot.semantic_hash
                && old.descriptor == snapshot.descriptor
            {
                slice_changes = SliceChanges::default();
            }
        }

        let prev_nodes = old_entry
            .as_ref()
            .map(|f| f.all_virtual_nodes())
            .unwrap_or_default();

        let mut new_entry = old_entry.clone().unwrap_or(FileEntry {
            canonical_id: canonical_id.clone(),
            file_kind: req.file_kind,
            source: Arc::<str>::from(""),
            whole_hash: snapshot.whole_hash,
            semantic_hash: snapshot.semantic_hash,
            slices: snapshot.slices.clone(),
            descriptor: snapshot.descriptor.clone(),
            meta: snapshot.meta.clone(),
            aliases: BTreeSet::new(),
            dependencies: BTreeSet::new(),
            external_requests: Vec::new(),
            src_blocks: Vec::new(),
            parse_diagnostics: DiagnosticsSnapshot::default(),
            style_overrides: HashMap::new(),
            compile_slots: HashMap::new(),
            latest_diagnostics: HashMap::new(),
            generation: 0,
        });

        new_entry.file_kind = req.file_kind;
        new_entry.source = req.source.clone();
        new_entry.whole_hash = snapshot.whole_hash;
        new_entry.semantic_hash = snapshot.semantic_hash;
        new_entry.slices = snapshot.slices.clone();
        new_entry.descriptor = snapshot.descriptor.clone();
        new_entry.meta = snapshot.meta.clone();
        new_entry.external_requests = snapshot.external_requests.clone();
        new_entry.src_blocks = snapshot.src_blocks.clone();
        new_entry.parse_diagnostics = snapshot.parse_diagnostics.clone();
        new_entry.generation = new_entry.generation.saturating_add(1);

        if changed {
            // only invalidate when semantic change occurred.
            let semantic_changed = old_entry
                .as_ref()
                .map(|o| {
                    o.semantic_hash != snapshot.semantic_hash || o.descriptor != snapshot.descriptor
                })
                .unwrap_or(true);
            if semantic_changed {
                new_entry.latest_diagnostics.clear();
                if slice_changes.script_changed
                    || slice_changes.structure_changed
                    || slice_changes.descriptor_changed
                {
                    new_entry.compile_slots.clear();
                } else if slice_changes.template_changed {
                    invalidate_nodes(
                        &mut new_entry.compile_slots,
                        &[VirtualNodeKind::Main, VirtualNodeKind::Template],
                    );
                } else {
                    let mut nodes = Vec::new();
                    for idx in &slice_changes.style_indices_changed {
                        nodes.push(VirtualNodeKind::Style { index: *idx });
                    }
                    for idx in &slice_changes.custom_indices_changed {
                        nodes.push(VirtualNodeKind::Custom { index: *idx });
                    }
                    invalidate_nodes(&mut new_entry.compile_slots, &nodes);
                }
            }
        }

        let mut alias_set: BTreeSet<String> =
            req.aliases.iter().map(|a| canonicalize_id(a)).collect();
        alias_set.insert(canonicalize_id(&req.input_id));
        alias_set.insert(canonical_id.clone());

        // update alias map and reverse deps atomically-ish under write lock
        {
            let mut alias_map = write_lock(&self.alias_to_canonical);
            for old_alias in new_entry.aliases.iter() {
                if !alias_set.contains(old_alias) {
                    alias_map.remove(old_alias);
                }
            }
            for alias in &alias_set {
                alias_map.insert(alias.clone(), canonical_id.clone());
            }
        }
        new_entry.aliases = alias_set;

        // dependency graph for src blocks
        let new_deps: BTreeSet<String> = new_entry
            .external_requests
            .iter()
            .map(|r| r.resolved_canonical_id.clone())
            .collect();

        {
            let mut rev = write_lock(&self.reverse_dependencies);
            for dep in new_entry.dependencies.iter() {
                if !new_deps.contains(dep) {
                    if let Some(owners) = rev.get_mut(dep) {
                        owners.remove(&canonical_id);
                        if owners.is_empty() {
                            rev.remove(dep);
                        }
                    }
                }
            }
            for dep in new_deps.iter() {
                rev.entry(dep.clone())
                    .or_insert_with(BTreeSet::new)
                    .insert(canonical_id.clone());
            }
        }
        new_entry.dependencies = new_deps;

        {
            let mut files = write_lock(&self.files);
            files.insert(canonical_id.clone(), new_entry.clone());
        }

        // if this upserted file is a dependency for owners, invalidate their compile slots.
        self.invalidate_dependents(&canonical_id);

        let new_nodes = new_entry.all_virtual_nodes();
        let (changed_nodes, removed_nodes) =
            compute_changed_removed_nodes(&slice_changes, changed, &prev_nodes, &new_nodes);

        let changed_nodes_sorted = sorted_nodes(changed_nodes);
        let removed_nodes_sorted = sorted_nodes(removed_nodes);

        let old_meta = old_entry
            .as_ref()
            .map(|o| o.meta.clone())
            .unwrap_or_default();

        let mut changed_virtual_ids = Vec::new();
        let mut changed_lsp_ids = Vec::new();
        for node in &changed_nodes_sorted {
            let (b, l) = render_ids(&canonical_id, node, &new_entry.meta);
            changed_virtual_ids.push(b);
            changed_lsp_ids.push(l);
        }

        let mut removed_virtual_ids = Vec::new();
        let mut removed_lsp_ids = Vec::new();
        for node in &removed_nodes_sorted {
            let (b, l) = render_ids(&canonical_id, node, &old_meta);
            removed_virtual_ids.push(b);
            removed_lsp_ids.push(l);
        }

        let mut diagnostics = new_entry.parse_diagnostics.clone();
        if diagnostics.diagnostics.is_empty() {
            diagnostics = DiagnosticsSnapshot::default();
        }

        Ok(HostUpdateResult {
            canonical_id,
            changed: !changed_nodes_sorted.is_empty() || !removed_nodes_sorted.is_empty(),
            slice_changes,
            changed_virtual_nodes: changed_nodes_sorted,
            removed_virtual_nodes: removed_nodes_sorted,
            changed_virtual_ids,
            removed_virtual_ids,
            changed_lsp_ids,
            removed_lsp_ids,
            diagnostics,
            external_source_requests: snapshot.external_requests,
        })
    }

    pub fn apply_style_overrides(
        &self,
        req: StyleOverrideRequest,
    ) -> Result<HostUpdateResult, HostError> {
        #[cfg(feature = "host_metrics")]
        self.metrics
            .style_override_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let canonical = self.resolve_alias_or_canonical(&req.canonical_id);
        let profile_hash = compile_profile_hash(&req.compile_profile);
        let mut files = write_lock(&self.files);
        let entry = files
            .get_mut(&canonical)
            .ok_or_else(|| HostError::MissingSource {
                canonical_id: canonical.clone(),
            })?;

        let mut by_index = HashMap::new();
        for ov in req.overrides {
            by_index.insert(ov.index, ov);
        }

        let override_hash = style_override_hash(&by_index);
        let previous_hash = entry
            .style_overrides
            .get(&profile_hash)
            .map(|o| o.hash)
            .unwrap_or(0);

        entry.style_overrides.insert(
            profile_hash,
            StyleOverrideLayer {
                hash: override_hash,
                by_index: by_index.clone(),
            },
        );

        entry.compile_slots.remove(&profile_hash);

        let mut changed_nodes = Vec::new();
        for idx in by_index.keys() {
            changed_nodes.push(VirtualNodeKind::Style { index: *idx });
        }
        changed_nodes = sorted_nodes(changed_nodes);

        let mut changed_virtual_ids = Vec::new();
        let mut changed_lsp_ids = Vec::new();
        for node in &changed_nodes {
            let (b, l) = render_ids(&canonical, node, &entry.meta);
            changed_virtual_ids.push(b);
            changed_lsp_ids.push(l);
        }

        Ok(HostUpdateResult {
            canonical_id: canonical,
            changed: previous_hash != override_hash,
            slice_changes: SliceChanges::default(),
            changed_virtual_nodes: changed_nodes,
            removed_virtual_nodes: Vec::new(),
            changed_virtual_ids,
            removed_virtual_ids: Vec::new(),
            changed_lsp_ids,
            removed_lsp_ids: Vec::new(),
            diagnostics: DiagnosticsSnapshot::default(),
            external_source_requests: entry.external_requests.clone(),
        })
    }

    pub fn get_virtual_file(&self, query: VirtualQuery) -> Result<VirtualFileResponse, HostError> {
        #[cfg(feature = "host_metrics")]
        self.metrics
            .virtual_loads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let (canonical_id, node_kind, raw_was_lsp) = if let Some(raw) = query.raw_id.clone() {
            let parsed = parse_raw_id(&raw).ok_or(HostError::InvalidQuery)?;
            (
                self.resolve_alias_or_canonical(&parsed.canonical_id),
                parsed.node_kind,
                parsed.was_lsp_like,
            )
        } else if let (Some(canonical), Some(node_kind)) =
            (query.canonical_id.clone(), query.node_kind.clone())
        {
            (
                self.resolve_alias_or_canonical(&canonical),
                node_kind,
                false,
            )
        } else {
            return Err(HostError::InvalidQuery);
        };

        let profile_hash = compile_profile_hash(&query.compile_profile);

        let snapshot = {
            let files = read_lock(&self.files);
            files
                .get(&canonical_id)
                .cloned()
                .ok_or_else(|| HostError::MissingSource {
                    canonical_id: canonical_id.clone(),
                })?
        };

        let style_override_hash = snapshot
            .style_overrides
            .get(&profile_hash)
            .map(|o| o.hash)
            .unwrap_or(0);

        if let Some(slot) = snapshot.compile_slots.get(&profile_hash) {
            if slot.semantic_hash == snapshot.semantic_hash
                && slot.style_override_hash == style_override_hash
            {
                #[cfg(feature = "host_metrics")]
                self.metrics
                    .compile_cache_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                if let Some(found) = slot.outputs.get(&node_kind) {
                    return Ok(VirtualFileResponse {
                        id: if raw_was_lsp {
                            render_ids(&canonical_id, &node_kind, &snapshot.meta).1
                        } else {
                            render_ids(&canonical_id, &node_kind, &snapshot.meta).0
                        },
                        code: found.code.clone(),
                        source_map: found.source_map.clone(),
                        lang: found.lang.clone(),
                        stale: false,
                        diagnostics: slot.diagnostics.clone(),
                        meta: found.meta.clone(),
                    });
                }
            }
        }

        #[cfg(feature = "host_metrics")]
        self.metrics
            .compile_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        #[cfg(feature = "host_metrics")]
        let compile_start = std::time::Instant::now();

        let existing_slot = snapshot.compile_slots.get(&profile_hash).cloned();
        let fallback_last_good = existing_slot
            .as_ref()
            .and_then(|slot| slot.last_good_outputs.clone());
        let (compiled_outputs, diagnostics, stale) =
            match self.compile_entry(&snapshot, &query.compile_profile) {
                Ok((outputs, diagnostics)) => (outputs, diagnostics, false),
                Err(diagnostics) => {
                    self.store_latest_diagnostics(&canonical_id, profile_hash, diagnostics.clone());
                    let policy = self.config.compile_error_policy;
                    if self.config.dev_mode && policy == CompileErrorPolicy::DevServeLastKnownGood {
                        if let Some(last_good) = fallback_last_good.clone() {
                            (last_good, diagnostics, true)
                        } else {
                            return Err(HostError::CompileError { diagnostics });
                        }
                    } else {
                        return Err(HostError::CompileError { diagnostics });
                    }
                }
            };

        #[cfg(feature = "host_metrics")]
        {
            let compile_elapsed_us = compile_start.elapsed().as_micros() as u64;
            self.metrics
                .compile_time_us_total
                .fetch_add(compile_elapsed_us, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut per_profile) = self.metrics.compile_time_us_total_by_profile.lock() {
                let entry = per_profile.entry(profile_hash).or_insert(0);
                *entry = entry.saturating_add(compile_elapsed_us);
            }
            if let Ok(mut per_profile_count) = self.metrics.compile_count_by_profile.lock() {
                let entry = per_profile_count.entry(profile_hash).or_insert(0);
                *entry = entry.saturating_add(1);
            }
        }

        let last_tick = self.tick.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical_id) {
                let last_good_outputs = if stale {
                    fallback_last_good.clone()
                } else {
                    Some(compiled_outputs.clone())
                };
                entry.compile_slots.insert(
                    profile_hash,
                    CompileSlot {
                        semantic_hash: entry.semantic_hash,
                        style_override_hash,
                        outputs: compiled_outputs.clone(),
                        diagnostics: diagnostics.clone(),
                        last_good_outputs,
                        last_access_tick: last_tick,
                    },
                );
                entry
                    .latest_diagnostics
                    .insert(profile_hash, diagnostics.clone());
                enforce_profile_cap(entry, self.config.max_profiles_per_file.max(1));
            }
        }

        let found =
            compiled_outputs
                .get(&node_kind)
                .ok_or_else(|| HostError::MissingVirtualNode {
                    canonical_id: canonical_id.clone(),
                })?;

        Ok(VirtualFileResponse {
            id: if raw_was_lsp {
                render_ids(&canonical_id, &node_kind, &snapshot.meta).1
            } else {
                render_ids(&canonical_id, &node_kind, &snapshot.meta).0
            },
            code: found.code.clone(),
            source_map: found.source_map.clone(),
            lang: found.lang.clone(),
            stale,
            diagnostics,
            meta: found.meta.clone(),
        })
    }

    pub fn list_virtual_files(&self, canonical_id: &str) -> Vec<VirtualNodeKind> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let files = read_lock(&self.files);
        files
            .get(&canonical)
            .map(|f| f.all_virtual_nodes())
            .unwrap_or_default()
    }

    pub fn remove(&self, canonical_or_alias: &str) -> Option<HostRemoveResult> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        let removed = {
            let mut files = write_lock(&self.files);
            files.remove(&canonical)
        }?;

        {
            let mut alias_map = write_lock(&self.alias_to_canonical);
            for alias in &removed.aliases {
                alias_map.remove(alias);
            }
        }

        {
            let mut rev = write_lock(&self.reverse_dependencies);
            for dep in &removed.dependencies {
                if let Some(owners) = rev.get_mut(dep) {
                    owners.remove(&canonical);
                    if owners.is_empty() {
                        rev.remove(dep);
                    }
                }
            }
            rev.remove(&canonical);
        }

        Some(HostRemoveResult {
            canonical_id: canonical,
        })
    }

    fn resolve_alias_or_canonical(&self, id: &str) -> String {
        let normalized = canonicalize_id(id);
        let alias_map = read_lock(&self.alias_to_canonical);
        alias_map.get(&normalized).cloned().unwrap_or(normalized)
    }

    fn store_latest_diagnostics(
        &self,
        canonical_id: &str,
        profile_hash: u64,
        diagnostics: DiagnosticsSnapshot,
    ) {
        let mut files = write_lock(&self.files);
        if let Some(entry) = files.get_mut(canonical_id) {
            entry.latest_diagnostics.insert(profile_hash, diagnostics);
        }
    }

    fn invalidate_dependents(&self, dependency_id: &str) {
        let owners = {
            let rev = read_lock(&self.reverse_dependencies);
            rev.get(dependency_id).cloned().unwrap_or_default()
        };

        if owners.is_empty() {
            return;
        }

        let mut files = write_lock(&self.files);
        for owner in owners {
            if let Some(file) = files.get_mut(&owner) {
                file.compile_slots.clear();
            }
        }
    }

    fn compile_entry(
        &self,
        snapshot: &FileEntry,
        profile: &CompileProfile,
    ) -> Result<
        (
            HashMap<VirtualNodeKind, CachedVirtualFile>,
            DiagnosticsSnapshot,
        ),
        DiagnosticsSnapshot,
    > {
        let mut diagnostics = snapshot.parse_diagnostics.clone();

        let mut merged_source = snapshot.source.to_string();
        if !snapshot.src_blocks.is_empty() {
            let ext_sources = {
                let files = read_lock(&self.files);
                let mut map = HashMap::new();
                for req in &snapshot.external_requests {
                    if let Some(dep_entry) = files.get(&req.resolved_canonical_id) {
                        map.insert(req.resolved_canonical_id.clone(), dep_entry.source.clone());
                    }
                }
                map
            };

            for req in &snapshot.external_requests {
                if !ext_sources.contains_key(&req.resolved_canonical_id) {
                    diagnostics =
                        diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                            severity: HostSeverity::Error,
                            code: "HOST_MISSING_EXTERNAL_SOURCE".to_string(),
                            message: format!(
                                "missing external source '{}' for '{}'",
                                req.specifier, snapshot.canonical_id
                            ),
                            span_start: None,
                            span_end: None,
                        }]));
                }
            }

            if diagnostics.has_errors {
                return Err(diagnostics);
            }

            merged_source =
                merge_external_sources(&merged_source, &snapshot.src_blocks, &ext_sources);
        }

        let alloc = Allocator::new();
        let mut core_opts = CodegenOptions::default();
        core_opts.filename = profile
            .filename
            .clone()
            .or_else(|| Some(snapshot.canonical_id.clone()));
        core_opts.is_production = profile.is_production;
        core_opts.component_id = profile.component_id.clone();
        core_opts.delimiters = profile.delimiters.clone();
        core_opts.custom_elements = profile.custom_elements.clone();
        core_opts.comments = profile.comments;
        core_opts.runtime_module_name = profile.runtime_module_name.clone();

        let verter_opts = VerterCompileOptions {
            force_vapor: profile.force_vapor,
            strip_ts: profile.strip_ts,
            source_map: profile.source_map,
        };

        let compiled: VerterCompileResult =
            compile_sfc(&merged_source, &core_opts, &verter_opts, &alloc);

        let mut compile_diags = diagnostics.clone();
        if !compiled.errors.is_empty() {
            compile_diags = compile_diags.merge(DiagnosticsSnapshot::from_vec(
                compiled
                    .errors
                    .iter()
                    .map(|d| HostDiagnostic {
                        severity: match d.severity {
                            verter_core::builder::codegen::CompileDiagnosticSeverity::Error => {
                                HostSeverity::Error
                            }
                            verter_core::builder::codegen::CompileDiagnosticSeverity::Warning => {
                                HostSeverity::Warning
                            }
                            verter_core::builder::codegen::CompileDiagnosticSeverity::Info => {
                                HostSeverity::Info
                            }
                        },
                        code: d.code.clone(),
                        message: d.message.clone(),
                        span_start: d.span.map(|s| s.start),
                        span_end: d.span.map(|s| s.end),
                    })
                    .collect(),
            ));
        }

        if compile_diags.has_errors {
            return Err(compile_diags);
        }

        let mut outputs = HashMap::new();

        let main_code =
            assemble_main_module(&snapshot.canonical_id, &compiled, &snapshot.meta, profile);
        outputs.insert(
            VirtualNodeKind::Main,
            CachedVirtualFile {
                code: Arc::from(main_code),
                source_map: None,
                lang: Some("js".to_string()),
                meta: VirtualMeta {
                    scope_id: if compiled.scope_id.is_empty() {
                        None
                    } else {
                        Some(compiled.scope_id.clone())
                    },
                    ..VirtualMeta::default()
                },
            },
        );

        if let Some(script) = compiled.script {
            outputs.insert(
                VirtualNodeKind::Script,
                CachedVirtualFile {
                    code: Arc::from(script.code),
                    source_map: if script.source_map.is_empty() {
                        None
                    } else {
                        Some(Arc::from(script.source_map))
                    },
                    lang: Some("ts".to_string()),
                    meta: VirtualMeta::default(),
                },
            );
        }

        if let Some(template) = compiled.template {
            outputs.insert(
                VirtualNodeKind::Template,
                CachedVirtualFile {
                    code: Arc::from(template.code),
                    source_map: if template.source_map.is_empty() {
                        None
                    } else {
                        Some(Arc::from(template.source_map))
                    },
                    lang: Some("tsx".to_string()),
                    meta: VirtualMeta::default(),
                },
            );
        }

        let profile_hash = compile_profile_hash(profile);
        let style_layer = snapshot.style_overrides.get(&profile_hash);

        for (i, style) in compiled.styles.into_iter().enumerate() {
            let override_entry = style_layer.and_then(|layer| layer.by_index.get(&i));
            outputs.insert(
                VirtualNodeKind::Style { index: i },
                CachedVirtualFile {
                    code: override_entry
                        .map(|e| e.code.clone())
                        .unwrap_or_else(|| Arc::from(style.code)),
                    source_map: override_entry.and_then(|e| e.source_map.clone()),
                    lang: Some(style.lang.unwrap_or_else(|| "css".to_string())),
                    meta: VirtualMeta {
                        style_index: Some(i),
                        ..VirtualMeta::default()
                    },
                },
            );
        }

        for (i, block) in compiled.custom_blocks.into_iter().enumerate() {
            outputs.insert(
                VirtualNodeKind::Custom { index: i },
                CachedVirtualFile {
                    code: Arc::from(block.content),
                    source_map: None,
                    lang: snapshot.meta.custom_langs.get(i).cloned().flatten(),
                    meta: VirtualMeta {
                        custom_index: Some(i),
                        block_type: Some(block.block_type),
                        ..VirtualMeta::default()
                    },
                },
            );
        }

        Ok((outputs, compile_diags))
    }
}

fn default_shared<T>(value: T) -> Shared<T> {
    #[cfg(feature = "single_threaded")]
    {
        std::cell::RefCell::new(value)
    }
    #[cfg(not(feature = "single_threaded"))]
    {
        std::sync::RwLock::new(value)
    }
}

fn node_sort_key(node: &VirtualNodeKind) -> (u8, usize) {
    match node {
        VirtualNodeKind::Main => (0, 0),
        VirtualNodeKind::Script => (1, 0),
        VirtualNodeKind::Template => (2, 0),
        VirtualNodeKind::Style { index } => (3, *index),
        VirtualNodeKind::Custom { index } => (4, *index),
    }
}

fn sorted_nodes(mut nodes: Vec<VirtualNodeKind>) -> Vec<VirtualNodeKind> {
    nodes.sort_by_key(node_sort_key);
    nodes.dedup();
    nodes
}

fn compute_changed_removed_nodes(
    slice_changes: &SliceChanges,
    changed: bool,
    prev_nodes: &[VirtualNodeKind],
    new_nodes: &[VirtualNodeKind],
) -> (Vec<VirtualNodeKind>, Vec<VirtualNodeKind>) {
    if !changed {
        return (Vec::new(), Vec::new());
    }
    if prev_nodes.is_empty() {
        return (new_nodes.to_vec(), Vec::new());
    }

    let prev_set: BTreeSet<_> = prev_nodes.iter().cloned().collect();
    let new_set: BTreeSet<_> = new_nodes.iter().cloned().collect();

    let removed: Vec<VirtualNodeKind> = prev_set.difference(&new_set).cloned().collect();

    let mut changed_nodes = Vec::new();
    if slice_changes.structure_changed
        || slice_changes.descriptor_changed
        || slice_changes.script_changed
    {
        changed_nodes.extend(new_nodes.iter().cloned());
    } else if slice_changes.template_changed {
        changed_nodes.push(VirtualNodeKind::Main);
        if new_set.contains(&VirtualNodeKind::Template) {
            changed_nodes.push(VirtualNodeKind::Template);
        }
    } else {
        for idx in &slice_changes.style_indices_changed {
            if new_set.contains(&VirtualNodeKind::Style { index: *idx }) {
                changed_nodes.push(VirtualNodeKind::Style { index: *idx });
            }
        }
        for idx in &slice_changes.custom_indices_changed {
            if new_set.contains(&VirtualNodeKind::Custom { index: *idx }) {
                changed_nodes.push(VirtualNodeKind::Custom { index: *idx });
            }
        }
    }

    (changed_nodes, removed)
}

fn compile_profile_hash(profile: &CompileProfile) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    profile.hash(&mut hasher);
    hasher.finish()
}

fn style_override_hash(overrides: &HashMap<usize, StyleOverrideEntry>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut entries: Vec<_> = overrides.iter().collect();
    entries.sort_by_key(|(idx, _)| **idx);
    for (idx, entry) in entries {
        idx.hash(&mut hasher);
        entry.code.as_ref().hash(&mut hasher);
        if let Some(sm) = &entry.source_map {
            sm.as_ref().hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn diff_indices<T: PartialEq>(old: &[T], new: &[T]) -> Vec<usize> {
    let max = old.len().max(new.len());
    let mut out = Vec::new();
    for i in 0..max {
        if old.get(i) != new.get(i) {
            out.push(i);
        }
    }
    out
}

fn hash_16(input: &[u8]) -> Hash16 {
    let mut hasher = Sha256::new();
    hasher.update(input);
    let bytes = hasher.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes[..16]);
    out
}

fn semantic_hash(slices: &SliceHashes, descriptor: &DescriptorMin) -> Hash16 {
    let mut hasher = Sha256::new();
    if let Some(script) = slices.script {
        hasher.update(script);
    }
    if let Some(template) = slices.template {
        hasher.update(template);
    }
    for h in &slices.styles {
        hasher.update(h);
    }
    for h in &slices.custom {
        hasher.update(h);
    }
    hasher.update(descriptor.script_count.to_le_bytes());
    hasher.update(descriptor.template_count.to_le_bytes());
    hasher.update(descriptor.style_count.to_le_bytes());
    hasher.update(descriptor.custom_count.to_le_bytes());
    hasher.update([descriptor.vapor as u8]);
    for fp in &descriptor.script_attr_fingerprints {
        hasher.update(fp.as_bytes());
        hasher.update([0]);
    }
    for fp in &descriptor.template_attr_fingerprints {
        hasher.update(fp.as_bytes());
        hasher.update([0]);
    }
    for fp in &descriptor.style_attr_fingerprints {
        hasher.update(fp.as_bytes());
        hasher.update([0]);
    }
    for fp in &descriptor.custom_attr_fingerprints {
        hasher.update(fp.as_bytes());
        hasher.update([0]);
    }
    let bytes = hasher.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes[..16]);
    out
}

fn canonicalize_id(input: &str) -> String {
    let mut s = input.trim().replace('\\', "/");
    if let Some((base, _)) = s.split_once('?') {
        s = base.to_string();
    }
    if let Some((base, _)) = s.split_once("._VERTER_.") {
        s = base.to_string();
    }
    s
}

fn parse_raw_id(raw: &str) -> Option<ParsedRawId> {
    let normalized_raw = raw.trim().replace('\\', "/");

    if let Some((canonical, suffix)) = normalized_raw.split_once("._VERTER_.") {
        let canonical = canonicalize_id(canonical);
        let node_kind = if suffix.starts_with("bundle.ts") {
            VirtualNodeKind::Main
        } else if suffix.starts_with("options.ts") {
            VirtualNodeKind::Script
        } else if suffix.starts_with("render.tsx") {
            VirtualNodeKind::Template
        } else if let Some(rest) = suffix.strip_prefix("style.") {
            let parts: Vec<&str> = rest.split('.').collect();
            let index = parts
                .first()
                .and_then(|p| p.parse::<usize>().ok())
                .unwrap_or(0);
            VirtualNodeKind::Style { index }
        } else if let Some(rest) = suffix.strip_prefix("custom.") {
            let parts: Vec<&str> = rest.split('.').collect();
            let index = parts
                .first()
                .and_then(|p| p.parse::<usize>().ok())
                .unwrap_or(0);
            VirtualNodeKind::Custom { index }
        } else {
            return None;
        };

        return Some(ParsedRawId {
            canonical_id: canonical,
            node_kind,
            was_lsp_like: true,
        });
    }

    let (base, query) = if let Some((b, q)) = normalized_raw.split_once('?') {
        (b, Some(q))
    } else {
        (normalized_raw.as_str(), None)
    };

    let canonical = canonicalize_id(base);
    let mut ty: Option<String> = None;
    let mut index: Option<usize> = None;

    if let Some(q) = query {
        for chunk in q.split('&') {
            if chunk.is_empty() {
                continue;
            }
            if chunk.eq_ignore_ascii_case("vue") || chunk.eq_ignore_ascii_case("verter") {
                continue;
            }
            if let Some(lang_tail) = chunk.strip_prefix("lang.") {
                let _ = lang_tail;
                continue;
            }
            let (k, v_opt) = if let Some((k, v)) = chunk.split_once('=') {
                (k.to_ascii_lowercase(), Some(v))
            } else {
                (chunk.to_ascii_lowercase(), None)
            };
            match k.as_str() {
                "type" => {
                    ty = v_opt.map(|v| v.to_ascii_lowercase());
                }
                "index" => {
                    if let Some(v) = v_opt {
                        index = v.parse::<usize>().ok();
                    }
                }
                _ => {}
            }
        }
    }

    let node_kind = match ty.as_deref() {
        Some("script") => VirtualNodeKind::Script,
        Some("template") => VirtualNodeKind::Template,
        Some("style") => VirtualNodeKind::Style {
            index: index.unwrap_or(0),
        },
        Some("custom") => VirtualNodeKind::Custom {
            index: index.unwrap_or(0),
        },
        Some(other) => {
            if index.is_some() {
                let _ = other;
                VirtualNodeKind::Custom {
                    index: index.unwrap_or(0),
                }
            } else {
                VirtualNodeKind::Main
            }
        }
        None => VirtualNodeKind::Main,
    };

    Some(ParsedRawId {
        canonical_id: canonical,
        node_kind,
        was_lsp_like: false,
    })
}

fn normalize_attr_map(attrs: &[(String, String)], include: &[&str]) -> String {
    let include: BTreeSet<String> = include.iter().map(|s| s.to_string()).collect();
    let mut map = BTreeMap::<String, String>::new();
    for (k, v) in attrs {
        let key = k.to_ascii_lowercase();
        if include.contains(&key) {
            let value = if v.is_empty() {
                "true".to_string()
            } else {
                v.to_string()
            };
            map.insert(key, value);
        }
    }
    let mut out = String::new();
    for (k, v) in map {
        let _ = write!(&mut out, "{}={}\\n", k, v);
    }
    out
}

fn extract_attrs(props: &[NodeProp], source: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    for p in props {
        let name = source[p.start as usize..p.name_end as usize].to_string();
        let value = match (p.value_start, p.value_end) {
            (Some(s), Some(e)) => source[s as usize..e as usize].to_string(),
            _ => String::new(),
        };
        attrs.push((name.to_ascii_lowercase(), value));
    }
    attrs
}

fn find_attr(attrs: &[(String, String)], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| {
            if v.is_empty() {
                "true".to_string()
            } else {
                v.clone()
            }
        })
}

fn resolve_external(owner: &str, specifier: &str) -> String {
    if specifier.starts_with('/') {
        return canonicalize_id(specifier);
    }
    if specifier.starts_with(".") {
        let mut parts: Vec<&str> = owner.split('/').collect();
        parts.pop();
        for segment in specifier.split('/') {
            match segment {
                "." | "" => {}
                ".." => {
                    let _ = parts.pop();
                }
                other => parts.push(other),
            }
        }
        return parts.join("/");
    }
    canonicalize_id(specifier)
}

fn parse_vue_snapshot(canonical_id: &str, source: &str) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());

    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &opts,
        diagnostics: Vec::new(),
    };

    let mut syntax = Syntax::new(false);
    tokenize(source.as_bytes(), |e| syntax.handle(&e, &ctx));

    let mut script_hashes = Vec::new();
    let mut script_attrs_fp = Vec::new();
    let mut script_count = 0;
    let mut has_script = false;
    let mut src_blocks = Vec::new();
    let mut external_requests = Vec::new();

    for (idx, script) in [syntax.script(), syntax.script_setup()]
        .into_iter()
        .flatten()
        .enumerate()
    {
        script_count += 1;
        has_script = true;
        let content = if let Some(span) = script.content {
            &source.as_bytes()[span.start as usize..span.end as usize]
        } else {
            b""
        };
        script_hashes.push(hash_16(content));

        let attrs = extract_attrs(&script.attributes, source);
        let mut attrs2 = attrs.clone();
        if script.is_setup {
            attrs2.push(("setup".to_string(), "true".to_string()));
        }
        if let Some(src_span) = script.src {
            let specifier = source[src_span.start as usize..src_span.end as usize].to_string();
            let resolved = resolve_external(canonical_id, &specifier);
            external_requests.push(ExternalSourceRequest {
                owner_canonical_id: canonical_id.to_string(),
                block_kind: ExternalBlockKind::Script,
                index: idx,
                specifier: specifier.clone(),
                resolved_canonical_id: resolved.clone(),
            });
            src_blocks.push(SrcBlockInfo {
                tag_name: "script".to_string(),
                resolved_canonical_id: resolved,
                tag_open_start: script.tag_open.start,
                tag_open_end: script.tag_open.end,
                tag_close_start: script.tag_close.as_ref().map(|c| c.start),
            });
        }
        script_attrs_fp.push(normalize_attr_map(&attrs2, &["setup", "lang", "src"]));
    }

    let script_hash = if script_hashes.is_empty() {
        None
    } else {
        let mut hasher = Sha256::new();
        for h in &script_hashes {
            hasher.update(h);
        }
        let out = hasher.finalize();
        let mut r = [0u8; 16];
        r.copy_from_slice(&out[..16]);
        Some(r)
    };

    let mut template_count = 0;
    let mut has_template = false;
    let mut template_hash = None;
    let mut template_attrs_fp = Vec::new();

    if let Some(ast) = syntax.template_ast() {
        template_count = 1;
        has_template = true;
        if let Some(content) = ast.root.content.as_ref() {
            template_hash = Some(hash_16(
                &source.as_bytes()[content.start as usize..content.end as usize],
            ));
        } else {
            template_hash = Some(hash_16(&[]));
        }

        let attrs = extract_attrs(&ast.root.attributes, source);
        if let Some(src) = find_attr(&attrs, "src") {
            let resolved = resolve_external(canonical_id, &src);
            external_requests.push(ExternalSourceRequest {
                owner_canonical_id: canonical_id.to_string(),
                block_kind: ExternalBlockKind::Template,
                index: 0,
                specifier: src.clone(),
                resolved_canonical_id: resolved.clone(),
            });
            src_blocks.push(SrcBlockInfo {
                tag_name: "template".to_string(),
                resolved_canonical_id: resolved,
                tag_open_start: ast.root.tag_open.start,
                tag_open_end: ast.root.tag_open.end,
                tag_close_start: ast.root.tag_close.as_ref().map(|c| c.start),
            });
        }
        template_attrs_fp.push(normalize_attr_map(&attrs, &["lang", "src"]));
    }

    let mut style_hashes = Vec::new();
    let mut style_attrs_fp = Vec::new();
    let mut style_langs = Vec::new();

    for (idx, style) in syntax.style_nodes().iter().enumerate() {
        let content = if let Some(span) = style.content {
            &source.as_bytes()[span.start as usize..span.end as usize]
        } else {
            b""
        };
        style_hashes.push(hash_16(content));

        let mut attrs = extract_attrs(&style.attributes, source);
        if style.scoped {
            attrs.push(("scoped".to_string(), "true".to_string()));
        }
        if style.module {
            attrs.push(("module".to_string(), "true".to_string()));
        }

        if let Some(src) = find_attr(&attrs, "src") {
            let resolved = resolve_external(canonical_id, &src);
            external_requests.push(ExternalSourceRequest {
                owner_canonical_id: canonical_id.to_string(),
                block_kind: ExternalBlockKind::Style,
                index: idx,
                specifier: src.clone(),
                resolved_canonical_id: resolved.clone(),
            });
            src_blocks.push(SrcBlockInfo {
                tag_name: "style".to_string(),
                resolved_canonical_id: resolved,
                tag_open_start: style.tag_open.start,
                tag_open_end: style.tag_open.end,
                tag_close_start: style.tag_close.as_ref().map(|c| c.start),
            });
        }

        style_attrs_fp.push(normalize_attr_map(
            &attrs,
            &["scoped", "module", "lang", "src"],
        ));

        style_langs.push(find_attr(&attrs, "lang"));
    }

    let mut custom_hashes = Vec::new();
    let mut custom_attrs_fp = Vec::new();
    let mut custom_types = Vec::new();
    let mut custom_langs = Vec::new();

    for (idx, custom) in syntax.unknown_nodes().iter().enumerate() {
        let content = if let Some(span) = custom.content {
            &source.as_bytes()[span.start as usize..span.end as usize]
        } else {
            b""
        };
        custom_hashes.push(hash_16(content));

        let block_type = source
            [custom.tag_open.start as usize + 1..custom.tag_open.name_end as usize]
            .to_string();
        custom_types.push(block_type.clone());

        let mut attrs = extract_attrs(&custom.attributes, source);
        attrs.push(("type".to_string(), block_type.clone()));

        if let Some(src) = find_attr(&attrs, "src") {
            let resolved = resolve_external(canonical_id, &src);
            external_requests.push(ExternalSourceRequest {
                owner_canonical_id: canonical_id.to_string(),
                block_kind: ExternalBlockKind::Custom,
                index: idx,
                specifier: src.clone(),
                resolved_canonical_id: resolved.clone(),
            });
            src_blocks.push(SrcBlockInfo {
                tag_name: block_type,
                resolved_canonical_id: resolved,
                tag_open_start: custom.tag_open.start,
                tag_open_end: custom.tag_open.end,
                tag_close_start: custom.tag_close.as_ref().map(|c| c.start),
            });
        }

        custom_langs.push(find_attr(&attrs, "lang"));

        custom_attrs_fp.push(normalize_attr_map(&attrs, &["type", "lang", "src"]));
    }

    let descriptor = DescriptorMin {
        script_count,
        template_count,
        style_count: style_hashes.len(),
        custom_count: custom_hashes.len(),
        script_attr_fingerprints: script_attrs_fp,
        template_attr_fingerprints: template_attrs_fp,
        style_attr_fingerprints: style_attrs_fp,
        custom_attr_fingerprints: custom_attrs_fp,
        vapor: syntax.is_vapor(),
    };

    let slices = SliceHashes {
        script: script_hash,
        template: template_hash,
        styles: style_hashes,
        custom: custom_hashes,
    };

    let semantic_hash = semantic_hash(&slices, &descriptor);

    let parse_diagnostics = DiagnosticsSnapshot::from_vec(
        syntax
            .take_diagnostics()
            .into_iter()
            .map(|d| HostDiagnostic {
                severity: match d.severity {
                    DiagnosticSeverity::Error => HostSeverity::Error,
                    DiagnosticSeverity::Warning => HostSeverity::Warning,
                    DiagnosticSeverity::Info => HostSeverity::Info,
                },
                code: format!("{:?}", d.code),
                message: d.message,
                span_start: d.span.map(|s| s.start),
                span_end: d.span.map(|s| s.end),
            })
            .collect(),
    );

    ParseSnapshot {
        whole_hash,
        semantic_hash,
        slices,
        descriptor,
        meta: FileMeta {
            has_script,
            has_template,
            style_langs,
            custom_types,
            custom_langs,
        },
        external_requests,
        src_blocks,
        parse_diagnostics,
    }
}

fn parse_non_sfc_snapshot(_canonical_id: &str, source: &str) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());
    let slices = SliceHashes::default();
    let descriptor = DescriptorMin::default();
    let semantic_hash = semantic_hash(&slices, &descriptor);
    ParseSnapshot {
        whole_hash,
        semantic_hash,
        slices,
        descriptor,
        meta: FileMeta::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics: DiagnosticsSnapshot::default(),
    }
}

fn merge_external_sources(
    source: &str,
    src_blocks: &[SrcBlockInfo],
    external_sources: &HashMap<String, Arc<str>>,
) -> String {
    let mut merged = source.to_string();
    let mut blocks = src_blocks.to_vec();
    blocks.sort_by(|a, b| b.tag_open_start.cmp(&a.tag_open_start));

    for block in blocks {
        let ext = external_sources
            .get(&block.resolved_canonical_id)
            .map(|s| s.as_ref())
            .unwrap_or("");

        if let Some(close_start) = block.tag_close_start {
            merged.replace_range(block.tag_open_end as usize..close_start as usize, ext);
        } else {
            let open_raw = &merged[block.tag_open_start as usize..block.tag_open_end as usize];
            let open_fixed = if let Some(stripped) = open_raw.strip_suffix("/>") {
                format!("{}>", stripped)
            } else {
                open_raw.to_string()
            };
            let replacement = format!("{}{} </{}>", open_fixed, ext, block.tag_name);
            merged.replace_range(
                block.tag_open_start as usize..block.tag_open_end as usize,
                &replacement,
            );
        }
    }

    merged
}

fn render_ids(canonical_id: &str, node: &VirtualNodeKind, meta: &FileMeta) -> (String, String) {
    match node {
        VirtualNodeKind::Main => (
            canonical_id.to_string(),
            format!("{}._VERTER_.bundle.ts", canonical_id),
        ),
        VirtualNodeKind::Script => (
            format!("{}?vue&type=script", canonical_id),
            format!("{}._VERTER_.options.ts", canonical_id),
        ),
        VirtualNodeKind::Template => (
            format!("{}?vue&type=template", canonical_id),
            format!("{}._VERTER_.render.tsx", canonical_id),
        ),
        VirtualNodeKind::Style { index } => {
            let lang = meta
                .style_langs
                .get(*index)
                .cloned()
                .flatten()
                .unwrap_or_else(|| "css".to_string());
            (
                format!(
                    "{}?vue&type=style&index={}&lang.{}",
                    canonical_id, index, lang
                ),
                format!("{}._VERTER_.style.{}.{}", canonical_id, index, lang),
            )
        }
        VirtualNodeKind::Custom { index } => {
            let block_type = meta
                .custom_types
                .get(*index)
                .cloned()
                .unwrap_or_else(|| "custom".to_string());
            (
                format!(
                    "{}?vue&type=custom&index={}&blockType={}",
                    canonical_id, index, block_type
                ),
                format!("{}._VERTER_.custom.{}.{}", canonical_id, index, block_type),
            )
        }
    }
}

fn assemble_main_module(
    canonical_id: &str,
    compiled: &VerterCompileResult,
    meta: &FileMeta,
    profile: &CompileProfile,
) -> String {
    let mut lines = Vec::<String>::new();

    for idx in 0..compiled.styles.len() {
        let (id, _) = render_ids(canonical_id, &VirtualNodeKind::Style { index: idx }, meta);
        lines.push(format!("import \"{}\"", id));
    }

    for idx in 0..compiled.custom_blocks.len() {
        let (id, _) = render_ids(canonical_id, &VirtualNodeKind::Custom { index: idx }, meta);
        lines.push(format!("import block{} from \"{}\"", idx, id));
    }

    if !compiled.styles.is_empty() || !compiled.custom_blocks.is_empty() {
        lines.push(String::new());
    }

    if let Some(script) = &compiled.script {
        let mut script_code = script.code.clone();
        script_code = script_code.replace("const __sfc__ =", "const _sfc_main =");
        script_code = script_code.replace("export default __sfc__;", "");
        lines.push(script_code);
    } else {
        lines.push("const _sfc_main = {}".to_string());
    }

    if let Some(template) = &compiled.template {
        lines.push(String::new());
        lines.push(template.code.clone());
        if template.code.contains("function render(") {
            lines.push("_sfc_main.render = render".to_string());
        }
    }

    for idx in 0..compiled.custom_blocks.len() {
        lines.push(format!(
            "if (typeof block{} === 'function') block{}(_sfc_main)",
            idx, idx
        ));
    }

    if !profile.is_production {
        lines.push(format!("_sfc_main.__file = {:?}", canonical_id));
    }

    if !profile.is_production && !profile.ssr {
        match profile.hmr_strategy {
            HmrStrategy::Vite => {
                lines.push("/* HMR(vite) */".to_string());
                lines.push("if (import.meta.hot) { import.meta.hot.accept(() => {}) }".to_string());
            }
            HmrStrategy::Webpack => {
                lines.push("/* HMR(webpack) */".to_string());
                lines.push("if (module.hot) { module.hot.accept(() => {}) }".to_string());
            }
            HmrStrategy::None => {}
        }
    }

    lines.push("export default _sfc_main".to_string());

    lines.join("\n")
}

fn invalidate_nodes(slots: &mut HashMap<u64, CompileSlot>, nodes: &[VirtualNodeKind]) {
    for slot in slots.values_mut() {
        for node in nodes {
            slot.outputs.remove(node);
            if let Some(last_good) = slot.last_good_outputs.as_mut() {
                last_good.remove(node);
            }
        }
    }
}

fn enforce_profile_cap(entry: &mut FileEntry, max_profiles: usize) {
    if entry.compile_slots.len() <= max_profiles {
        return;
    }
    let mut items: Vec<(u64, u64)> = entry
        .compile_slots
        .iter()
        .map(|(k, v)| (*k, v.last_access_tick))
        .collect();
    items.sort_by_key(|(_, tick)| *tick);
    let excess = entry.compile_slots.len() - max_profiles;
    for (k, _) in items.into_iter().take(excess) {
        entry.compile_slots.remove(&k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_dev() -> CompileProfile {
        CompileProfile {
            is_production: false,
            hmr_strategy: HmrStrategy::Vite,
            ..CompileProfile::default()
        }
    }

    fn profile_prod() -> CompileProfile {
        CompileProfile {
            is_production: true,
            hmr_strategy: HmrStrategy::None,
            ..CompileProfile::default()
        }
    }

    fn upsert_vue(host: &VerterHost, id: &str, src: &str) -> HostUpdateResult {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src.to_string()),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
            compile_profile: profile_dev(),
        })
        .unwrap()
    }

    #[test]
    fn resolve_query_param_tolerance_and_order() {
        let host = VerterHost::new(HostConfig::default());

        let a = host
            .resolve("Comp.vue?vue&type=style&index=0&id=abc&scoped=true&lang.css")
            .unwrap();
        let b = host
            .resolve("Comp.vue?vue&index=0&type=style&lang.css&id=abc")
            .unwrap();

        assert_eq!(a.canonical_id, b.canonical_id);
        assert_eq!(a.node_kind, b.node_kind);
        assert_eq!(a.node_kind, VirtualNodeKind::Style { index: 0 });
    }

    #[test]
    fn resolve_explicit_script_template_custom() {
        let host = VerterHost::new(HostConfig::default());

        assert_eq!(
            host.resolve("Comp.vue?vue&type=script").unwrap().node_kind,
            VirtualNodeKind::Script
        );
        assert_eq!(
            host.resolve("Comp.vue?vue&type=template")
                .unwrap()
                .node_kind,
            VirtualNodeKind::Template
        );
        assert_eq!(
            host.resolve("Comp.vue?vue&type=custom&index=2")
                .unwrap()
                .node_kind,
            VirtualNodeKind::Custom { index: 2 }
        );
    }

    #[test]
    fn resolve_succeeds_without_source_get_virtual_file_missing_source() {
        let host = VerterHost::new(HostConfig::default());

        let resolved = host.resolve("/x/Comp.vue?vue&type=template").unwrap();
        assert!(!resolved.exists_in_host);

        let err = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("/x/Comp.vue?vue&type=template".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap_err();

        match err {
            HostError::MissingSource { canonical_id } => {
                assert_eq!(canonical_id, "/x/Comp.vue");
            }
            _ => panic!("expected MissingSource"),
        }
    }

    #[test]
    fn non_slice_edit_no_invalidation() {
        let host = VerterHost::new(HostConfig::default());

        let src1 = "<script setup>const n = 1</script>\n<template><div>{{n}}</div></template>\n<style>.a{color:red}</style>";
        let src2 = "<script setup>const n = 1</script>\n\n\n<template><div>{{n}}</div></template>\n\n<style>.a{color:red}</style>";

        let first = upsert_vue(&host, "Comp.vue", src1);
        assert!(first.changed);

        let second = upsert_vue(&host, "Comp.vue", src2);
        assert!(!second.changed);
        assert!(second.changed_virtual_ids.is_empty());
        assert!(second.changed_lsp_ids.is_empty());
    }

    #[test]
    fn style_only_edit_returns_only_style_virtual_id() {
        let host = VerterHost::new(HostConfig::default());

        let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";
        let src2 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:blue}</style>";

        upsert_vue(&host, "Comp.vue", src1);
        let result = upsert_vue(&host, "Comp.vue", src2);

        assert_eq!(
            result.changed_virtual_nodes,
            vec![VirtualNodeKind::Style { index: 0 }]
        );
        assert_eq!(result.changed_virtual_ids.len(), 1);
        assert!(result.changed_virtual_ids[0].contains("type=style"));
    }

    #[test]
    fn template_edit_returns_main_and_template_ids() {
        let host = VerterHost::new(HostConfig::default());

        let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";
        let src2 = "<script setup>const n = 1</script><template><section>{{n}}</section></template><style>.a{color:red}</style>";

        upsert_vue(&host, "Comp.vue", src1);
        let result = upsert_vue(&host, "Comp.vue", src2);

        assert_eq!(
            result.changed_virtual_nodes,
            vec![VirtualNodeKind::Main, VirtualNodeKind::Template]
        );
    }

    #[test]
    fn script_edit_returns_all_virtual_ids() {
        let host = VerterHost::new(HostConfig::default());

        let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";
        let src2 = "<script setup>const n = 2</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";

        upsert_vue(&host, "Comp.vue", src1);
        let result = upsert_vue(&host, "Comp.vue", src2);

        assert!(result
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Main));
        assert!(result
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Script));
        assert!(result
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Template));
        assert!(result
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Style { index: 0 }));
    }

    #[test]
    fn compile_profile_changes_produce_different_cached_outputs() {
        let host = VerterHost::new(HostConfig::default());
        let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
        upsert_vue(&host, "Comp.vue", src);

        let dev = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();
        let prod = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_prod(),
            })
            .unwrap();

        assert_ne!(dev.code.as_ref(), prod.code.as_ref());
    }

    #[test]
    fn style_override_updates_style_without_reupsert() {
        let host = VerterHost::new(HostConfig::default());
        let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";
        upsert_vue(&host, "Comp.vue", src);

        let before = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue?vue&type=style&index=0".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        host.apply_style_overrides(StyleOverrideRequest {
            canonical_id: "Comp.vue".to_string(),
            compile_profile: profile_dev(),
            overrides: vec![StyleOverrideEntry {
                index: 0,
                code: Arc::from(".a{color:green}"),
                source_map: None,
            }],
        })
        .unwrap();

        let after = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue?vue&type=style&index=0".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        assert_ne!(before.code.as_ref(), after.code.as_ref());
        assert_eq!(after.code.as_ref(), ".a{color:green}");
    }

    #[test]
    fn update_result_contains_both_bundler_and_lsp_ids() {
        let host = VerterHost::new(HostConfig::default());

        let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";
        let src2 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:blue}</style>";

        upsert_vue(&host, "Comp.vue", src1);
        let result = upsert_vue(&host, "Comp.vue", src2);

        assert_eq!(
            result.changed_virtual_ids.len(),
            result.changed_lsp_ids.len()
        );
        assert!(result.changed_virtual_ids[0].contains("?vue&type=style"));
        assert!(result.changed_lsp_ids[0].contains("._VERTER_.style."));
    }

    #[test]
    fn src_policy_missing_external_source_produces_deterministic_error() {
        let host = VerterHost::new(HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        });

        let src = "<template src=\"./t.html\"></template><script setup>const n=1</script>";
        let update = upsert_vue(&host, "Comp.vue", src);
        assert_eq!(update.external_source_requests.len(), 1);

        let err = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap_err();

        match err {
            HostError::CompileError { diagnostics } => {
                assert!(diagnostics
                    .diagnostics
                    .iter()
                    .any(|d| d.code == "HOST_MISSING_EXTERNAL_SOURCE"));
            }
            _ => panic!("expected compile error"),
        }
    }

    #[test]
    fn external_upsert_invalidates_dependent_owner() {
        let host = VerterHost::new(HostConfig::default());

        upsert_vue(
            &host,
            "Comp.vue",
            "<template src=\"./tpl.html\"></template><script setup>const n = 1</script>",
        );

        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "tpl.html".to_string(),
            source: Arc::from("<div>A</div>"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
            compile_profile: profile_dev(),
        })
        .unwrap();

        let first = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue?vue&type=template".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "tpl.html".to_string(),
            source: Arc::from("<section>B</section>"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
            compile_profile: profile_dev(),
        })
        .unwrap();

        let second = host
            .get_virtual_file(VirtualQuery {
                raw_id: Some("Comp.vue?vue&type=template".to_string()),
                canonical_id: None,
                node_kind: None,
                compile_profile: profile_dev(),
            })
            .unwrap();

        assert_ne!(first.code.as_ref(), second.code.as_ref());
    }
}
