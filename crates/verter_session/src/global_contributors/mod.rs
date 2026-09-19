//! Global contributor population — complete immutable snapshots published
//! at artifact ingestion.
//!
//! Per-file contribution facts are derived from the `IndexedReady` header
//! inventory (script/module classification, lib-file and script-file
//! interfaces/namespaces, `declare global`, module augmentations). A
//! coherent snapshot is published atomically: pin membership epoch `S`,
//! build the reverse index outside reader-visible state, verify `S` is
//! still current, then swap. Readers observe either the previous or the
//! new snapshot, never a mixture.
//!
//! Lookup of a global symbol reads the published population and returns
//! that symbol's contributor fingerprint, including a proved-empty set.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHashMap;
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::SymbolSpace;
use verter_type_expr::TopLevelOwnerId;

use crate::file_artifact_store::{
    compute_augmenter_set_fingerprint, AugmentationTargetKind, AugmenterEntry, FileArtifactKey,
    FileArtifacts, InternedName, InternedSpecifier, GLOBAL_AUGMENTATION_TAG,
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

/// Ingestion-time record for one published artifact.
#[derive(Debug, Clone)]
pub struct FileContributionRecord {
    pub module_kind: FileModuleKind,
    pub is_automatic_lib: bool,
    pub parse_stable_hash: Hash16,
    pub facts: Arc<[GlobalContributionFact]>,
}

/// One contributor in a published population, ordered by the declaration
/// authority `(canonical, parse_stable_hash)` — never arrival order.
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
            fingerprint: compute_augmenter_set_fingerprint(&[]),
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

    /// Contributors for `target` + `decl_name`, filtered to the overlay
    /// population and `noLib` (automatic libs only).
    #[must_use]
    pub fn lookup(
        &self,
        target: &AugmentationTargetKind,
        decl_name: &str,
        overlay_discriminator: Option<Hash16>,
        allow_automatic_libs: bool,
    ) -> SymbolContributors {
        let key = SymbolKey::from_target(target, decl_name);
        let Some(all) = self.by_symbol.get(&key) else {
            return SymbolContributors::empty();
        };
        let mut matched: Vec<ContributorEntry> = all
            .iter()
            .filter(|entry| entry.matches_overlay(overlay_discriminator))
            .filter(|entry| allow_automatic_libs || !entry.is_automatic_lib)
            .cloned()
            .collect();
        matched.sort_by(|a, b| {
            a.artifact_key
                .canonical
                .as_ref()
                .cmp(b.artifact_key.canonical.as_ref())
                .then_with(|| a.parse_stable_hash.cmp(&b.parse_stable_hash))
        });
        let fingerprint = fingerprint_of(&matched);
        SymbolContributors {
            entries: Arc::from(matched.into_boxed_slice()),
            fingerprint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SymbolKey {
    target_tag: u8,
    target_text: Arc<str>,
    name: Arc<str>,
}

impl SymbolKey {
    fn from_target(target: &AugmentationTargetKind, name: &str) -> Self {
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
                    // Relative targets resolve at lookup; stored under the
                    // authored specifier so the relative stitch can still
                    // use the augmenter index as the merge authority.
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
    let augmenter: Vec<AugmenterEntry> = entries
        .iter()
        .map(|entry| AugmenterEntry {
            artifact_key: entry.artifact_key.clone(),
            parse_stable_hash: entry.parse_stable_hash,
        })
        .collect();
    compute_augmenter_set_fingerprint(&augmenter)
}

/// Reverse index of per-file contribution records plus the published
/// immutable snapshot. Mutation goes through [`Self::note_live`] /
/// [`Self::note_gone`] then [`Self::publish`].
pub struct GlobalContributorIndex {
    records: DashMap<FileArtifactKey, Arc<FileContributionRecord>>,
    snapshot: parking_lot::RwLock<Arc<GlobalContributorPopulation>>,
    publish: parking_lot::Mutex<()>,
    revision: AtomicU64,
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
            snapshot: parking_lot::RwLock::new(Arc::new(GlobalContributorPopulation {
                program_snapshot: 0,
                revision: 0,
                by_symbol: Arc::new(FxHashMap::default()),
            })),
            publish: parking_lot::Mutex::new(()),
            revision: AtomicU64::new(0),
        }
    }

    /// Current published snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Arc<GlobalContributorPopulation> {
        Arc::clone(&self.snapshot.read())
    }

    /// Record (or replace) the contributions of a live artifact.
    pub fn note_live(&self, key: FileArtifactKey, payload: &FileArtifacts) {
        let record = collect_file_contributions(&key, payload);
        self.records.insert(key, Arc::new(record));
    }

    /// Drop a retired artifact from the unpublished record set.
    pub fn note_gone(&self, key: &FileArtifactKey) {
        self.records.remove(key);
    }

    pub fn clear(&self) {
        self.records.clear();
        let _guard = self.publish.lock();
        *self.snapshot.write() = Arc::new(GlobalContributorPopulation {
            program_snapshot: 0,
            revision: 0,
            by_symbol: Arc::new(FxHashMap::default()),
        });
        self.revision.store(0, Ordering::Release);
    }

    /// Pin `S`, build the reverse index off to the side, publish only if
    /// membership is still `S`. A concurrent edit that advanced the epoch
    /// abandons this attempt; that editor publishes its own snapshot.
    pub fn publish(&self, expected_epoch: u64, live_epoch: impl Fn() -> u64) {
        if live_epoch() != expected_epoch {
            return;
        }
        self.publish_now(live_epoch);
    }

    /// Rebuild the snapshot from the current record set. Used after a
    /// membership mutation so concurrent inserts cannot all abandon.
    pub fn publish_now(&self, live_epoch: impl Fn() -> u64) {
        let _guard = self.publish.lock();
        let mut grouped: FxHashMap<SymbolKey, Vec<ContributorEntry>> = FxHashMap::default();
        for entry in self.records.iter() {
            let key = entry.key();
            let record = entry.value();
            for fact in record.facts.iter() {
                let Some(symbol_key) = SymbolKey::from_fact(fact) else {
                    continue;
                };
                grouped
                    .entry(symbol_key)
                    .or_default()
                    .push(ContributorEntry {
                        artifact_key: key.clone(),
                        parse_stable_hash: record.parse_stable_hash,
                        owner: fact.owner,
                        origin: fact.origin,
                        specifier: fact.specifier.clone(),
                        symbol: fact.symbol.clone(),
                        space: fact.space,
                        is_automatic_lib: record.is_automatic_lib,
                    });
            }
        }
        let program_snapshot = live_epoch();
        for entries in grouped.values_mut() {
            entries.sort_by(|a, b| {
                a.artifact_key
                    .canonical
                    .as_ref()
                    .cmp(b.artifact_key.canonical.as_ref())
                    .then_with(|| a.parse_stable_hash.cmp(&b.parse_stable_hash))
                    .then_with(|| a.owner.cmp(&b.owner))
            });
        }
        let by_symbol = grouped
            .into_iter()
            .map(|(k, v)| (k, Arc::from(v.into_boxed_slice())))
            .collect();
        let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        *self.snapshot.write() = Arc::new(GlobalContributorPopulation {
            program_snapshot,
            revision,
            by_symbol: Arc::new(by_symbol),
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
            facts.push(GlobalContributionFact {
                symbol: InternedName::from(block.qualified_name.as_str()),
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
        module_kind,
        is_automatic_lib,
        parse_stable_hash,
        facts: Arc::from(facts.into_boxed_slice()),
    }
}

/// A file is a module when it carries import or export syntax, including
/// the empty `export {}` form that TypeScript uses to opt a `.d.ts` out
/// of the global script scope.
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
        || source_has_module_syntax(indexed.eval_source.as_ref())
    {
        FileModuleKind::Module
    } else {
        FileModuleKind::Script
    }
}

fn source_has_module_syntax(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut in_line = false;
    let mut in_block = false;
    let mut string: Option<u8> = None;
    let mut at_statement = true;
    while i < bytes.len() {
        let b = bytes[i];
        if in_line {
            if b == b'\n' {
                in_line = false;
                at_statement = true;
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
            b'\n' | b';' | b'{' => {
                at_statement = true;
                i += 1;
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ if at_statement && starts_with_ident(bytes, i, b"import")
                || at_statement && starts_with_ident(bytes, i, b"export") =>
            {
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

/// Automatic library files: ambient virtual ids and default `lib.*.d.ts`
/// names. `noLib` disables these only; user globals stay.
#[must_use]
pub fn is_automatic_lib_canonical(canonical: &str) -> bool {
    if canonical.starts_with("ambient:/") {
        return true;
    }
    let name = canonical.rsplit(['/', '\\']).next().unwrap_or(canonical);
    name.starts_with("lib.") && name.ends_with(".d.ts")
}
