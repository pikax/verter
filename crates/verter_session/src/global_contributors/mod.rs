//! Global contributor population — complete immutable snapshots published
//! at artifact ingestion.
//!
//! Per-file contribution facts are derived from the `IndexedReady` header
//! inventory (script/module classification, lib-file and script-file
//! interfaces/namespaces, `declare global`, module augmentations). A
//! coherent snapshot is published atomically: pin membership epoch `S`,
//! read changed membership, build the reverse index outside reader-visible
//! state, verify `S` is still current, then swap. Readers observe either
//! the previous or the new snapshot, never a mixture.
//!
//! Lookup of a global symbol reads the published population and returns
//! that symbol's contributor fingerprint, including a proved-empty set.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::SymbolSpace;
use verter_type_expr::TopLevelOwnerId;

use crate::file_artifact_store::{
    AugmentationTargetKind, FileArtifactKey, FileArtifacts, InternedName, InternedSpecifier,
    GLOBAL_AUGMENTATION_TAG,
};
use crate::project_type_store::IndexedReady;

#[cfg(test)]
mod ac1_tests;
#[cfg(test)]
mod ac2_tests;
#[cfg(test)]
mod ac3_tests;

/// TypeScript script vs external-module classification of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileModuleKind {
    /// No import/export syntax: top-level interfaces and namespaces
    /// contribute to the global object.
    Script,
    /// Has import or export syntax: only `declare global` / module
    /// augmentations contribute globally.
    Module,
}

/// How one file contributes one global (or ambient-module) symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContributorOrigin {
    DeclareGlobal,
    ModuleAugmentation,
    FileScopeInterface,
    /// Top-level `namespace N` in a script (or automatic lib). Lowered
    /// through the same retained-body path as file-scope interfaces.
    FileScopeNamespace,
}

/// One per-file contribution to a resolved global / ambient symbol.
#[derive(Debug, Clone)]
pub struct GlobalContributionFact {
    pub symbol: InternedName,
    pub space: SymbolSpace,
    pub owner: TopLevelOwnerId,
    pub origin: ContributorOrigin,
    pub specifier: Option<InternedSpecifier>,
    pub fingerprint: Hash16,
}

/// Ingestion-time record for one published live artifact version.
#[derive(Debug, Clone)]
pub struct FileContributionRecord {
    pub artifact_key: FileArtifactKey,
    pub module_kind: FileModuleKind,
    pub is_automatic_lib: bool,
    pub parse_stable_hash: Hash16,
    pub facts: Arc<[GlobalContributionFact]>,
}

/// One contributor in a published population. Precedence is applied at
/// lookup from the project's configured `files` sequence when present,
/// otherwise a stable canonical-path normalize of unordered discovery.
#[derive(Debug, Clone)]
pub struct ContributorEntry {
    pub artifact_key: FileArtifactKey,
    pub parse_stable_hash: Hash16,
    pub owner: TopLevelOwnerId,
    pub origin: ContributorOrigin,
    pub specifier: Option<InternedSpecifier>,
    pub symbol: InternedName,
    pub space: SymbolSpace,
    pub is_automatic_lib: bool,
    pub module_kind: FileModuleKind,
    pub contribution_fingerprint: Hash16,
}

/// Per-symbol contributor set plus its fingerprint (empty set included).
#[derive(Debug, Clone)]
pub struct SymbolContributors {
    pub entries: Arc<[ContributorEntry]>,
    pub fingerprint: Hash16,
}

impl SymbolContributors {
    fn empty() -> Self {
        Self {
            entries: Arc::from(Vec::new().into_boxed_slice()),
            fingerprint: fingerprint_of(&[]),
        }
    }
}

/// Immutable snapshot complete at membership epoch `S` / revision `E`.
#[derive(Debug, Clone)]
pub struct GlobalContributorPopulation {
    pub program_snapshot: u64,
    pub revision: u64,
    by_symbol: Arc<FxHashMap<SymbolKey, Arc<[ContributorEntry]>>>,
}

impl GlobalContributorPopulation {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_symbol.is_empty()
    }

    /// Contributors for `target` + `decl_name` in type and namespace
    /// space (interface + namespace merge), filtered to the overlay
    /// population and `noLib` (automatic libs only).
    #[must_use]
    pub fn lookup(
        &self,
        target: &AugmentationTargetKind,
        decl_name: &str,
        overlay_discriminator: Option<Hash16>,
        allow_automatic_libs: bool,
    ) -> SymbolContributors {
        self.lookup_spaces(
            target,
            decl_name,
            overlay_discriminator,
            allow_automatic_libs,
            &[SymbolSpace::Type, SymbolSpace::Namespace],
        )
    }

    /// Contributors in one symbol space.
    #[must_use]
    pub fn lookup_in_space(
        &self,
        target: &AugmentationTargetKind,
        decl_name: &str,
        overlay_discriminator: Option<Hash16>,
        allow_automatic_libs: bool,
        space: SymbolSpace,
    ) -> SymbolContributors {
        self.lookup_spaces(
            target,
            decl_name,
            overlay_discriminator,
            allow_automatic_libs,
            &[space],
        )
    }

    fn lookup_spaces(
        &self,
        target: &AugmentationTargetKind,
        decl_name: &str,
        overlay_discriminator: Option<Hash16>,
        allow_automatic_libs: bool,
        spaces: &[SymbolSpace],
    ) -> SymbolContributors {
        let mut matched: Vec<ContributorEntry> = Vec::new();
        for space in spaces {
            let key = SymbolKey::from_target(target, decl_name, *space);
            if let Some(all) = self.by_symbol.get(&key) {
                matched.extend(all.iter().cloned());
            }
        }
        let overlay_canonicals = overlay_replacements(&matched, overlay_discriminator);
        matched.retain(|entry| {
            if !entry.matches_overlay(overlay_discriminator) {
                return false;
            }
            if !allow_automatic_libs && entry.is_automatic_lib {
                return false;
            }
            if overlay_discriminator.is_some()
                && entry.artifact_key.is_base()
                && overlay_canonicals.contains(entry.artifact_key.canonical.as_ref())
            {
                return false;
            }
            true
        });
        if matched.is_empty() {
            return SymbolContributors::empty();
        }
        matched.sort_by(compare_contributors);
        let fingerprint = fingerprint_of(&matched);
        SymbolContributors {
            entries: Arc::from(matched.into_boxed_slice()),
            fingerprint,
        }
    }
}

fn overlay_replacements(
    entries: &[ContributorEntry],
    overlay_discriminator: Option<Hash16>,
) -> FxHashSet<Arc<str>> {
    let Some(discriminator) = overlay_discriminator else {
        return FxHashSet::default();
    };
    entries
        .iter()
        .filter(|entry| {
            !entry.artifact_key.is_base() && entry.artifact_key.parse_env_hash == discriminator
        })
        .map(|entry| Arc::clone(&entry.artifact_key.canonical))
        .collect()
}

fn compare_contributors(a: &ContributorEntry, b: &ContributorEntry) -> std::cmp::Ordering {
    a.artifact_key
        .canonical
        .as_ref()
        .cmp(b.artifact_key.canonical.as_ref())
        .then_with(|| a.parse_stable_hash.cmp(&b.parse_stable_hash))
        .then_with(|| a.owner.cmp(&b.owner))
        .then_with(|| a.space.tag().cmp(&b.space.tag()))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SymbolKey {
    target_tag: u8,
    target_text: Arc<str>,
    name: Arc<str>,
    space: SymbolSpace,
}

impl SymbolKey {
    fn from_target(target: &AugmentationTargetKind, name: &str, space: SymbolSpace) -> Self {
        let (target_tag, target_text) = match target {
            AugmentationTargetKind::ExternalSpecifier(spec) => (0, Arc::from(spec.as_ref())),
            AugmentationTargetKind::ResolvedRelativeCanonical(canon) => (1, Arc::clone(canon)),
            AugmentationTargetKind::WildcardAmbient(pat) => (2, Arc::from(pat.as_ref())),
            AugmentationTargetKind::GlobalAugmentation => (3, Arc::from(GLOBAL_AUGMENTATION_TAG)),
        };
        Self {
            target_tag,
            target_text,
            name: Arc::from(name),
            space,
        }
    }

    fn from_fact(fact: &GlobalContributionFact) -> Option<Self> {
        let (target_tag, target_text) = match fact.origin {
            ContributorOrigin::DeclareGlobal
            | ContributorOrigin::FileScopeInterface
            | ContributorOrigin::FileScopeNamespace => (3, Arc::from(GLOBAL_AUGMENTATION_TAG)),
            ContributorOrigin::ModuleAugmentation => {
                let spec = fact.specifier.as_ref()?.as_ref();
                if spec == GLOBAL_AUGMENTATION_TAG {
                    (3, Arc::from(GLOBAL_AUGMENTATION_TAG))
                } else if spec.contains('*') {
                    (2, Arc::from(spec))
                } else if verter_semantic::resolver_core::is_relative_specifier(spec) {
                    (1, Arc::from(spec))
                } else {
                    (0, Arc::from(spec))
                }
            }
        };
        Some(Self {
            target_tag,
            target_text,
            name: Arc::from(fact.symbol.as_ref()),
            space: fact.space,
        })
    }
}

impl ContributorEntry {
    fn matches_overlay(&self, overlay_discriminator: Option<Hash16>) -> bool {
        match overlay_discriminator {
            None => self.artifact_key.is_base(),
            Some(discriminator) => {
                self.artifact_key.is_base() || self.artifact_key.parse_env_hash == discriminator
            }
        }
    }
}

fn fingerprint_of(entries: &[ContributorEntry]) -> Hash16 {
    use std::hash::{BuildHasher, Hasher};
    let salt_lo = rustc_hash::FxBuildHasher;
    let salt_hi = rustc_hash::FxBuildHasher;
    let mut h_lo = salt_lo.build_hasher();
    let mut h_hi = salt_hi.build_hasher();
    h_lo.write_u64(0xC4A1_C4A1_4A1C_4A1C);
    h_hi.write_u64(0x9E37_79B9_7F4A_7C15);
    for entry in entries {
        h_lo.write(entry.artifact_key.canonical.as_bytes());
        h_lo.write(&entry.contribution_fingerprint);
        h_lo.write_u8(entry.space.tag());
        h_lo.write_u8(entry.module_kind as u8);
        h_lo.write_u8(entry.origin as u8);
        h_hi.write(entry.artifact_key.canonical.as_bytes());
        h_hi.write(&entry.contribution_fingerprint);
        h_hi.write_u8(entry.space.tag());
        h_hi.write_u8(entry.module_kind as u8);
        h_hi.write_u8(entry.origin as u8);
    }
    let lo = h_lo.finish();
    let hi = h_hi.finish();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&lo.to_le_bytes());
    out[8..].copy_from_slice(&hi.to_le_bytes());
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RecordKey {
    canonical: Arc<str>,
    overlay: Option<Hash16>,
}

impl RecordKey {
    fn from_artifact(key: &FileArtifactKey) -> Self {
        Self {
            canonical: Arc::clone(&key.canonical),
            overlay: if key.is_base() {
                None
            } else {
                Some(key.parse_env_hash)
            },
        }
    }
}

struct GroupedState {
    by_symbol: FxHashMap<SymbolKey, Vec<ContributorEntry>>,
    dirty: FxHashSet<SymbolKey>,
}

impl GroupedState {
    fn new() -> Self {
        Self {
            by_symbol: FxHashMap::default(),
            dirty: FxHashSet::default(),
        }
    }

    fn clear(&mut self) {
        self.by_symbol.clear();
        self.dirty.clear();
    }
}

/// Reverse index of per-file contribution records plus the published
/// immutable snapshot. Mutation goes through [`Self::note_live`] /
/// [`Self::note_gone`] then [`Self::publish`].
pub struct GlobalContributorIndex {
    records: DashMap<RecordKey, Arc<FileContributionRecord>>,
    grouped: parking_lot::Mutex<GroupedState>,
    snapshot: parking_lot::RwLock<Arc<GlobalContributorPopulation>>,
    publish: parking_lot::Mutex<()>,
    revision: AtomicU64,
    /// Overlay `.ts` files with file-level `declare module`/`declare global`
    /// that have not yet been IndexedReady. Drained from contribution
    /// collection, not from upsert, so fence flights stay cold.
    pending_overlay_ambient: parking_lot::Mutex<FxHashSet<Arc<str>>>,
    #[cfg(test)]
    publish_sorted_entries: AtomicU64,
}

impl std::fmt::Debug for GlobalContributorIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalContributorIndex")
            .field("records", &self.records.len())
            .field("revision", &self.revision.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Default for GlobalContributorIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalContributorIndex {
    #[must_use]
    pub fn new() -> Self {
        Self {
            records: DashMap::new(),
            grouped: parking_lot::Mutex::new(GroupedState::new()),
            snapshot: parking_lot::RwLock::new(Arc::new(GlobalContributorPopulation {
                program_snapshot: 0,
                revision: 0,
                by_symbol: Arc::new(FxHashMap::default()),
            })),
            publish: parking_lot::Mutex::new(()),
            revision: AtomicU64::new(0),
            pending_overlay_ambient: parking_lot::Mutex::new(FxHashSet::default()),
            #[cfg(test)]
            publish_sorted_entries: AtomicU64::new(0),
        }
    }

    /// Current published snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Arc<GlobalContributorPopulation> {
        Arc::clone(&self.snapshot.read())
    }

    /// Entries cloned and sorted while building a published snapshot.
    /// Zero on an unrelated file that contributes no global symbols.
    #[cfg(test)]
    #[must_use]
    pub fn publish_sorted_entry_count(&self) -> u64 {
        self.publish_sorted_entries.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub fn reset_publish_sorted_entry_count(&self) {
        self.publish_sorted_entries.store(0, Ordering::Relaxed);
    }

    /// Record (or replace) the contributions of a live artifact version.
    /// Same-canonical overlay/base slots replace in place so retained
    /// old versions are not program membership.
    pub fn note_live(&self, key: FileArtifactKey, payload: &FileArtifacts) {
        let record = Arc::new(collect_file_contributions(&key, payload));
        let rec_key = RecordKey::from_artifact(&key);
        let previous = self.records.insert(rec_key, Arc::clone(&record));
        let mut grouped = self.grouped.lock();
        if let Some(previous) = previous {
            remove_record_from_grouped(&mut grouped, &previous);
        }
        add_record_to_grouped(&mut grouped, &record);
    }

    /// Drop a retired artifact from the unpublished record set.
    pub fn note_gone(&self, key: &FileArtifactKey) {
        let rec_key = RecordKey::from_artifact(key);
        let Some((_, previous)) = self.records.remove(&rec_key) else {
            return;
        };
        if previous.artifact_key != *key {
            self.records.insert(rec_key, previous);
            return;
        }
        let mut grouped = self.grouped.lock();
        remove_record_from_grouped(&mut grouped, &previous);
    }

    pub fn note_pending_overlay_ambient(&self, canonical: &str) {
        self.pending_overlay_ambient
            .lock()
            .insert(Arc::from(canonical));
    }

    #[must_use]
    pub fn take_pending_overlay_ambient(&self) -> Vec<Arc<str>> {
        self.pending_overlay_ambient.lock().drain().collect()
    }

    pub fn clear(&self) {
        self.records.clear();
        self.pending_overlay_ambient.lock().clear();
        self.grouped.lock().clear();
        let _guard = self.publish.lock();
        *self.snapshot.write() = Arc::new(GlobalContributorPopulation {
            program_snapshot: 0,
            revision: 0,
            by_symbol: Arc::new(FxHashMap::default()),
        });
        self.revision.store(0, Ordering::Release);
        #[cfg(test)]
        self.publish_sorted_entries.store(0, Ordering::Relaxed);
    }

    /// Pin `S`, clone the reverse index off to the side, publish only if
    /// membership is still `S`. A concurrent edit that advanced the epoch
    /// abandons this attempt; that editor publishes its own snapshot.
    pub fn publish(&self, expected_epoch: u64, live_epoch: impl Fn() -> u64) {
        let _guard = self.publish.lock();
        let pinned = live_epoch();
        if pinned != expected_epoch {
            return;
        }
        self.publish_pinned(pinned, &live_epoch);
    }

    /// Publish the current record set. Pin the live epoch, clone grouped
    /// state, and retry if membership moved during the clone so a racing
    /// edit is not lost inside a claimed-current snapshot.
    pub fn publish_now(&self, live_epoch: impl Fn() -> u64) {
        let _guard = self.publish.lock();
        // bounded-loop: membership epoch retry
        for _ in 0..8 {
            let pinned = live_epoch();
            if self.publish_pinned(pinned, &live_epoch) {
                return;
            }
        }
    }

    fn publish_pinned(&self, pinned: u64, live_epoch: &impl Fn() -> u64) -> bool {
        let by_symbol = {
            let mut grouped = self.grouped.lock();
            let prev = self.snapshot.read();
            let next = if grouped.dirty.is_empty() {
                Arc::clone(&prev.by_symbol)
            } else {
                let mut next = (*prev.by_symbol).clone();
                for key in grouped.dirty.iter() {
                    match grouped.by_symbol.get(key) {
                        Some(entries) if !entries.is_empty() => {
                            let mut sorted = entries.clone();
                            sorted.sort_by(compare_contributors);
                            #[cfg(test)]
                            self.publish_sorted_entries
                                .fetch_add(sorted.len() as u64, Ordering::Relaxed);
                            next.insert(key.clone(), Arc::from(sorted.into_boxed_slice()));
                        }
                        _ => {
                            next.remove(key);
                        }
                    }
                }
                Arc::new(next)
            };
            drop(prev);
            if live_epoch() != pinned {
                return false;
            }
            grouped.dirty.clear();
            next
        };
        let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        *self.snapshot.write() = Arc::new(GlobalContributorPopulation {
            program_snapshot: pinned,
            revision,
            by_symbol,
        });
        true
    }
}

fn remove_record_from_grouped(grouped: &mut GroupedState, record: &FileContributionRecord) {
    for fact in record.facts.iter() {
        let Some(symbol_key) = SymbolKey::from_fact(fact) else {
            continue;
        };
        grouped.dirty.insert(symbol_key.clone());
        let Some(entries) = grouped.by_symbol.get_mut(&symbol_key) else {
            continue;
        };
        entries.retain(|entry| entry.artifact_key != record.artifact_key);
        if entries.is_empty() {
            grouped.by_symbol.remove(&symbol_key);
        }
    }
}

fn add_record_to_grouped(grouped: &mut GroupedState, record: &FileContributionRecord) {
    for fact in record.facts.iter() {
        let Some(symbol_key) = SymbolKey::from_fact(fact) else {
            continue;
        };
        grouped.dirty.insert(symbol_key.clone());
        grouped
            .by_symbol
            .entry(symbol_key)
            .or_default()
            .push(ContributorEntry {
                artifact_key: record.artifact_key.clone(),
                parse_stable_hash: record.parse_stable_hash,
                owner: fact.owner,
                origin: fact.origin,
                specifier: fact.specifier.clone(),
                symbol: fact.symbol.clone(),
                space: fact.space,
                is_automatic_lib: record.is_automatic_lib,
                module_kind: record.module_kind,
                contribution_fingerprint: fact.fingerprint,
            });
    }
}

/// Derive per-file global contribution facts from an ingested artifact.
#[must_use]
pub fn collect_file_contributions(
    key: &FileArtifactKey,
    payload: &FileArtifacts,
) -> FileContributionRecord {
    collect_from_indexed(
        key,
        &payload.indexed,
        &payload.augmentations,
        payload.parse_stable_hash,
    )
}

fn collect_from_indexed(
    key: &FileArtifactKey,
    indexed: &IndexedReady,
    augmentations: &[crate::file_artifact_store::ModuleAugmentationFact],
    parse_stable_hash: Hash16,
) -> FileContributionRecord {
    let module_kind = classify_module_kind(indexed);
    let is_automatic_lib = is_automatic_lib_canonical(key.canonical.as_ref());
    let mut facts: Vec<GlobalContributionFact> = Vec::new();

    for fact in augmentations {
        let origin = if fact.specifier.as_ref() == GLOBAL_AUGMENTATION_TAG {
            ContributorOrigin::DeclareGlobal
        } else {
            ContributorOrigin::ModuleAugmentation
        };
        facts.push(GlobalContributionFact {
            symbol: fact.augmented_name.clone(),
            space: fact.space,
            owner: fact.owner,
            origin,
            specifier: Some(fact.specifier.clone()),
            fingerprint: fact.augmented_member_shape_fingerprint,
        });
    }

    if module_kind == FileModuleKind::Script || is_automatic_lib {
        let headers = indexed.shallow_state.decl_bodies().header_index();
        for (binding, header) in headers.type_headers.iter() {
            if header.kind != verter_semantic::analysis::type_eval::TypeDeclKind::Interface {
                continue;
            }
            facts.push(GlobalContributionFact {
                symbol: InternedName::from(binding.name.as_ref()),
                space: SymbolSpace::Type,
                owner: binding.owner,
                origin: ContributorOrigin::FileScopeInterface,
                specifier: None,
                fingerprint: crate::fact_emission::augmentation_header_fingerprint(
                    &verter_semantic::analysis::type_eval::AugmentationScopeKind::Global,
                    binding.owner,
                    binding.name.as_ref(),
                    "Interface",
                    header.member_headers.as_slice(),
                    header.contributors.len(),
                ),
            });
        }
        for block in &headers.namespace_blocks {
            let name = block.qualified_name.as_str();
            if name.contains('.') {
                continue;
            }
            facts.push(GlobalContributionFact {
                symbol: InternedName::from(name),
                space: SymbolSpace::Namespace,
                owner: block.owner,
                origin: ContributorOrigin::FileScopeNamespace,
                specifier: None,
                fingerprint: xxhash_rust::xxh3::xxh3_128(block.qualified_name.as_bytes())
                    .to_le_bytes(),
            });
        }
    }

    FileContributionRecord {
        artifact_key: key.clone(),
        module_kind,
        is_automatic_lib,
        parse_stable_hash,
        facts: Arc::from(facts.into_boxed_slice()),
    }
}

/// A file is a module when the retained shallow inventory carries import
/// or export syntax, including the empty `export {}` form that TypeScript
/// uses to opt a `.d.ts` out of the global script scope. Dynamic `import()`
/// and nested `export` inside a namespace are not file-level module syntax.
#[must_use]
pub fn classify_module_kind(indexed: &IndexedReady) -> FileModuleKind {
    let shallow = indexed.shallow_state.as_ref();
    if !shallow.exports.is_empty()
        || !shallow.wildcard_reexports.is_empty()
        || !shallow.import_targets.is_empty()
        || shallow.export_assignment_target().is_some()
    {
        return FileModuleKind::Module;
    }
    let routes = shallow.route_inventory.as_ref();
    if !routes.imports.is_empty()
        || !routes.bindingless_imports.is_empty()
        || !routes.reexports.is_empty()
        || !routes.wildcard_reexports.is_empty()
        || !routes.local_exports.is_empty()
        || !routes.export_assignments.is_empty()
        || source_has_file_module_syntax(indexed.eval_source.as_ref())
    {
        FileModuleKind::Module
    } else {
        FileModuleKind::Script
    }
}

/// File-level `declare global` / `declare module` (quoted or ambient
/// namespace). Nested inside another block, comments, and strings are
/// not contributions. Used to ingest never-imported ambient declarers
/// without IndexedReady-ing every `.d.ts`.
#[must_use]
pub(crate) fn source_has_ambient_contribution(source: &str) -> bool {
    if !source.contains("declare") {
        return false;
    }
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut in_line = false;
    let mut in_block = false;
    let mut string: Option<u8> = None;
    let mut at_statement = true;
    let mut brace_depth: u32 = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_line {
            if b == b'\n' {
                in_line = false;
                if brace_depth == 0 {
                    at_statement = true;
                }
            }
            i += 1;
            continue;
        }
        if in_block {
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(closer) = string {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == closer {
                string = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                in_line = true;
                i += 2;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                in_block = true;
                i += 2;
            }
            b'\'' | b'"' | b'`' => {
                string = Some(b);
                at_statement = false;
                i += 1;
            }
            b'{' => {
                brace_depth = brace_depth.saturating_add(1);
                at_statement = true;
                i += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                at_statement = brace_depth == 0;
                i += 1;
            }
            b'\n' | b';' => {
                if brace_depth == 0 {
                    at_statement = true;
                }
                i += 1;
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ if at_statement && brace_depth == 0 && starts_with_ident(bytes, i, b"export") => {
                i += b"export".len();
            }
            _ if at_statement && brace_depth == 0 && starts_with_ident(bytes, i, b"declare") => {
                i += b"declare".len();
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if starts_with_ident(bytes, i, b"global") || starts_with_ident(bytes, i, b"module")
                {
                    return true;
                }
                at_statement = false;
            }
            _ => {
                at_statement = false;
                i += 1;
            }
        }
    }
    false
}

/// File-level `import`/`export` including `export {}`. Nested `export`
/// inside `namespace`/`module` blocks and dynamic `import()` are not
/// module syntax.
fn source_has_file_module_syntax(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut in_line = false;
    let mut in_block = false;
    let mut string: Option<u8> = None;
    let mut in_regex = false;
    let mut regex_class = false;
    let mut at_statement = true;
    let mut brace_depth: u32 = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_line {
            if b == b'\n' {
                in_line = false;
                if brace_depth == 0 {
                    at_statement = true;
                }
            }
            i += 1;
            continue;
        }
        if in_block {
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_regex {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == b'[' {
                regex_class = true;
            } else if b == b']' {
                regex_class = false;
            } else if b == b'/' && !regex_class {
                in_regex = false;
            }
            i += 1;
            continue;
        }
        if let Some(closer) = string {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == closer {
                string = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                in_line = true;
                i += 2;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                in_block = true;
                i += 2;
            }
            b'/' => {
                in_regex = true;
                regex_class = false;
                at_statement = false;
                i += 1;
            }
            b'\'' | b'"' | b'`' => {
                string = Some(b);
                at_statement = false;
                i += 1;
            }
            b'{' => {
                brace_depth = brace_depth.saturating_add(1);
                at_statement = true;
                i += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                at_statement = brace_depth == 0;
                i += 1;
            }
            b'\n' | b';' => {
                if brace_depth == 0 {
                    at_statement = true;
                }
                i += 1;
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ if at_statement && brace_depth == 0 && starts_with_ident(bytes, i, b"export") => {
                return true;
            }
            _ if at_statement && brace_depth == 0 && starts_with_ident(bytes, i, b"import") => {
                let mut j = i + b"import".len();
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'(' || bytes[j] == b'.') {
                    at_statement = false;
                    i += 1;
                    continue;
                }
                return true;
            }
            _ => {
                at_statement = false;
                i += 1;
            }
        }
    }
    false
}

fn starts_with_ident(bytes: &[u8], i: usize, word: &[u8]) -> bool {
    if i + word.len() > bytes.len() {
        return false;
    }
    if &bytes[i..i + word.len()] != word {
        return false;
    }
    let after = i + word.len();
    after == bytes.len() || !is_ident_continue(bytes[after])
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Automatic library files are ambient-lib virtual ids (`ambient:/…`).
/// `noLib` disables these only; a user file whose basename happens to
/// look like `lib.*.d.ts` stays a program declaration.
#[must_use]
pub fn is_automatic_lib_canonical(canonical: &str) -> bool {
    canonical.starts_with("ambient:/")
}

/// Route-surface fact key for the per-symbol population fingerprint.
/// Extra optional fields carry `decl_name` so this is distinct from the
/// target-wide augmenter-set `ModuleAugmentationIndexShape` observation.
#[must_use]
pub fn population_contributor_fact_key(
    target: &AugmentationTargetKind,
    decl_name: &str,
) -> verter_semantic::facts::FactKey {
    use verter_semantic::facts::registry::{
        AugmentationTargetKindTag, InternedGlobPattern, InternedSpecifier,
    };
    use verter_semantic::facts::FactKey;
    match target {
        AugmentationTargetKind::GlobalAugmentation => FactKey::ModuleAugmentationIndexShape {
            target_kind_tag: AugmentationTargetKindTag::GlobalAugmentation,
            external_specifier: Some(InternedSpecifier::from(decl_name)),
            resolved_relative_canonical: None,
            wildcard_pattern: None,
        },
        AugmentationTargetKind::ExternalSpecifier(spec) => FactKey::ModuleAugmentationIndexShape {
            target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
            external_specifier: Some(spec.clone()),
            resolved_relative_canonical: None,
            wildcard_pattern: Some(InternedGlobPattern::from(decl_name)),
        },
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => {
            FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ResolvedRelativeCanonical,
                external_specifier: None,
                resolved_relative_canonical: Some(Arc::clone(canon)),
                wildcard_pattern: Some(InternedGlobPattern::from(decl_name)),
            }
        }
        AugmentationTargetKind::WildcardAmbient(pat) => FactKey::ModuleAugmentationIndexShape {
            target_kind_tag: AugmentationTargetKindTag::WildcardAmbient,
            external_specifier: Some(InternedSpecifier::from(decl_name)),
            resolved_relative_canonical: None,
            wildcard_pattern: Some(pat.clone()),
        },
    }
}

/// `decl_name` encoded on a population fingerprint observation, if any.
#[must_use]
pub fn population_fact_decl_name<'a>(
    target_kind_tag: verter_semantic::facts::registry::AugmentationTargetKindTag,
    external_specifier: Option<&'a str>,
    wildcard_pattern: Option<&'a str>,
) -> Option<&'a str> {
    use verter_semantic::facts::registry::AugmentationTargetKindTag;
    match target_kind_tag {
        AugmentationTargetKindTag::GlobalAugmentation => external_specifier,
        AugmentationTargetKindTag::ExternalSpecifier => wildcard_pattern,
        AugmentationTargetKindTag::ResolvedRelativeCanonical => wildcard_pattern,
        AugmentationTargetKindTag::WildcardAmbient => external_specifier,
    }
}
