//! Lazy declaration-body memo — the content-addressed body store one
//! `IndexedReady` artifact owns.
//!
//! The shallow declaration-header index ([`DeclHeaderIndex`]) is the
//! eager inventory; THIS memo materialises declaration BODIES on first
//! semantic demand, through the scheduler-side
//! [`DeclLoweringService`] retained snapshot (never a re-parse per
//! touch — native retains the snapshot on a worker thread it owns;
//! `wasm32` retains it in a single-thread thread-local shard, NOT a
//! service field, since the `Rc`-backed parse is `!Send`/`!Sync`),
//! and caches the owned results per symbol.
//! The memo is a FILE-ARTIFACT
//! child: its identity is the owning artifact's
//! `(canonical, whole_hash, parse_env_hash)` [`SnapshotKey`] — content-
//! addressed by construction, so a content edit produces a fresh memo
//! and the superseded one can never answer a new-content demand.
//! Overlay artifacts own their own memo instance; an overlay body can
//! therefore never populate a base read (and vice versa).
//!
//! Concurrency: one `OnceLock` per `(space, scope, name)` entry —
//! concurrent first-touch of one symbol lowers it ONCE; waiters block
//! cooperatively on the cell. The cell is cloned OUT of the map before
//! initialisation so no map shard lock is held across the lowering
//! call. A demanded statement that also declares sibling symbols
//! backfills exactly those siblings' entries (the work was actually
//! performed — path-independent population of only what the compute
//! produced).

use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use dashmap::DashMap;
use rustc_hash::{FxHashMap, FxHashSet};

use verter_compiler::utils::oxc::script::raw_surface::{
    capture_statement_surfaces, merge_overload_groups, RawSourceSurface, SymbolSpace,
};
use verter_compiler::utils::oxc::script::type_surface::{
    collect_statement_dependency_names, AnalyzedExternalTypeSource,
};
use verter_semantic::analysis::decl_headers::DeclHeaderIndex;
use verter_semantic::analysis::type_eval::{
    AugmentationScopeKind, EnumMemberValue, EvalEnv, FunctionSignature, TypeDeclBody, TypeDeclKind,
    ValueDeclGroup, ValueDeclKind,
};
use verter_semantic::analysis::type_eval_build::{
    build_eval_env, lower_jsdoc_typedef_named, lower_top_level_statement,
};
use verter_type_expr::{ObjectExpr, TypeExpr, TypeParam};

use crate::decl_lowering::{DeclLoweringService, SnapshotKey, SnapshotLease};
use crate::resolver_core::shallow_file_state::{
    collect_type_refs, collect_typeof_roots, extract_member_deps,
};
use crate::types::MetaProvenance;

/// The lazily lowered body of one TYPE declaration group (all same-name
/// contributors folded, exactly as the whole-env walk would fold them).
#[derive(Debug, Clone)]
pub struct LoweredTypeDecl {
    pub kind: TypeDeclKind,
    /// `TypeDeclBody::Single` or the `Merged` carrier — the same
    /// merge-aware body `TypeDeclGroup::merged_body` produces.
    pub body: TypeDeclBody,
    /// Generic type parameters, unioned across contributors in source
    /// order.
    pub type_parameters: Vec<TypeParam>,
    /// Body reference names (the per-statement analyzer product), unioned
    /// across contributors.
    pub dep_names: FxHashSet<String>,
    /// Structural subset of [`dep_names`](Self::dep_names).
    pub structural_dep_names: FxHashSet<String>,
    /// Per-member dependency names over the merged lookup surface.
    pub member_deps: FxHashMap<String, Vec<String>>,
    /// `typeof` roots referenced by the merged lookup surface (sorted).
    pub typeof_root_names: Vec<String>,
}

/// The lazily lowered body of one VALUE declaration group.
#[derive(Debug, Clone)]
pub struct LoweredValueDecl {
    pub kind: ValueDeclKind,
    pub type_annotation: Option<TypeExpr>,
    /// The merged overload signature set, in source order.
    pub signatures: Vec<FunctionSignature>,
    pub object_shape: Option<ObjectExpr>,
    /// The full ordered member inventory (NAME → [`EnumMemberValue`]) for an
    /// `enum` declaration, in source declaration order, UNIONED across every
    /// same-name merged contributor (the eval-env enum arm produces each
    /// contributor's members on demand; [`ValueDeclGroup::merged_enum_unified`]
    /// folds the group). `Some` exactly when the lowered value decl is an enum.
    /// Drives `typeof Enum` (an object keyed by the member NAMES) and the
    /// `Enum.Member` projection via [`EnumMemberValue::projected_type`] — EVERY
    /// member, foldable or deferred-and-degraded. The value-body fingerprint
    /// reads the [`EnumMemberValue::folded_literal`] subset only.
    pub enum_members: Option<Vec<(String, EnumMemberValue)>>,
}

type TypeCell = Arc<OnceLock<Option<Arc<LoweredTypeDecl>>>>;
type ValueCell = Arc<OnceLock<Option<Arc<LoweredValueDecl>>>>;

/// Owned product of one statement-batch lowering job: every symbol the
/// demanded statements actually declared, ready for entry population.
struct LoweredStatementBatch {
    types: Vec<(String, LoweredTypeDecl)>,
    values: Vec<(String, LoweredValueDecl)>,
    aug_types: Vec<(AugmentationScopeKind, String, LoweredTypeDecl)>,
    aug_values: Vec<(AugmentationScopeKind, String, LoweredValueDecl)>,
    /// Declaration-body contributors lowered by this job — the
    /// `decl_bodies_lowered` increment.
    lowered_count: usize,
}

/// See module docs.
pub struct DeclBodyMemo {
    key: SnapshotKey,
    eval_source: Arc<str>,
    raw_source: Arc<str>,
    framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
    source_type: oxc_span::SourceType,
    /// `None` on a seeded memo (every entry pre-filled; nothing to
    /// compute lazily).
    service: Option<Arc<DeclLoweringService>>,
    /// LEASE pinning this memo's retained parse snapshot for the lifetime
    /// of the memo (hence the owning `IndexedReady` artifact). Acquired
    /// lazily on the first service-backed body demand; dropped with the
    /// memo, releasing the retained parse. A seeded memo (no service)
    /// never holds a lease.
    lease: OnceLock<SnapshotLease>,
    header_index: Arc<DeclHeaderIndex>,
    provenance: Arc<MetaProvenance>,
    type_entries: DashMap<String, TypeCell>,
    value_entries: DashMap<String, ValueCell>,
    aug_type_entries: DashMap<(AugmentationScopeKind, String), TypeCell>,
    aug_value_entries: DashMap<(AugmentationScopeKind, String), ValueCell>,
    whole_env: OnceLock<Arc<EvalEnv>>,
    raw_surfaces: DashMap<(String, SymbolSpace), Arc<Vec<RawSourceSurface>>>,
}

impl std::fmt::Debug for DeclBodyMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeclBodyMemo")
            .field("key", &self.key)
            .field("type_entries", &self.type_entries.len())
            .field("value_entries", &self.value_entries.len())
            .finish_non_exhaustive()
    }
}

impl DeclBodyMemo {
    /// Production constructor: an index-only memo whose bodies lower on
    /// first demand through `service`.
    ///
    /// `lease` carries the snapshot pin already acquired by the cold-index
    /// parse (the earliest service parse for this content generation) so
    /// the body demands reuse that one parse instead of re-parsing. When
    /// `None`, the memo acquires its own lease lazily on first body demand
    /// (see [`Self::ensure_lease`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        key: SnapshotKey,
        eval_source: Arc<str>,
        raw_source: Arc<str>,
        framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
        source_type: oxc_span::SourceType,
        service: Arc<DeclLoweringService>,
        header_index: Arc<DeclHeaderIndex>,
        provenance: Arc<MetaProvenance>,
        lease: Option<SnapshotLease>,
    ) -> Self {
        let lease_cell = OnceLock::new();
        if let Some(lease) = lease {
            let _ = lease_cell.set(lease);
        }
        Self {
            key,
            eval_source,
            raw_source,
            framework_parse,
            source_type,
            service: Some(service),
            lease: lease_cell,
            header_index,
            provenance,
            type_entries: DashMap::default(),
            value_entries: DashMap::default(),
            aug_type_entries: DashMap::default(),
            aug_value_entries: DashMap::default(),
            whole_env: OnceLock::new(),
            raw_surfaces: DashMap::default(),
        }
    }

    /// Seeded constructor for the env-supplied construction path (test
    /// fixtures and other already-built-env callers): every entry is
    /// pre-filled from the built env using the same per-symbol folding
    /// the lazy path performs, and the whole env is pre-set. No service;
    /// nothing lowers lazily.
    pub(crate) fn seeded_from_env(
        key: SnapshotKey,
        env: &EvalEnv,
        analysis: &AnalyzedExternalTypeSource,
        header_index: Arc<DeclHeaderIndex>,
    ) -> Self {
        let memo = Self {
            key,
            eval_source: Arc::from(""),
            raw_source: Arc::from(""),
            framework_parse: None,
            source_type: oxc_span::SourceType::ts(),
            service: None,
            lease: OnceLock::new(),
            header_index,
            provenance: Arc::new(MetaProvenance::default()),
            type_entries: DashMap::default(),
            value_entries: DashMap::default(),
            aug_type_entries: DashMap::default(),
            aug_value_entries: DashMap::default(),
            whole_env: OnceLock::new(),
            raw_surfaces: DashMap::default(),
        };

        for (name, group) in &env.type_symbols {
            let deps = analysis
                .local_type_symbol(name)
                .map(|symbol| {
                    (
                        symbol.dependency_names.clone(),
                        symbol.structural_dependency_names.clone(),
                    )
                })
                .unwrap_or_default();
            let enum_body = env
                .value_symbols
                .get(name)
                .and_then(ValueDeclGroup::enum_type_union);
            let lowered = lowered_type_decl_from_group(group, deps.0, deps.1, enum_body);
            memo.type_entries.insert(
                name.clone(),
                Arc::new(OnceLock::from(Some(Arc::new(lowered)))),
            );
        }
        for (name, group) in &env.value_symbols {
            let lowered = lowered_value_decl_from_group(group);
            memo.value_entries.insert(
                name.clone(),
                Arc::new(OnceLock::from(Some(Arc::new(lowered)))),
            );
        }
        for ((scope, name), group) in &env.augmentation_scopes {
            let lowered = lowered_type_decl_from_group(
                group,
                FxHashSet::default(),
                FxHashSet::default(),
                None,
            );
            memo.aug_type_entries.insert(
                (scope.clone(), name.clone()),
                Arc::new(OnceLock::from(Some(Arc::new(lowered)))),
            );
        }
        for ((scope, name), group) in &env.augmentation_value_scopes {
            let lowered = lowered_value_decl_from_group(group);
            memo.aug_value_entries.insert(
                (scope.clone(), name.clone()),
                Arc::new(OnceLock::from(Some(Arc::new(lowered)))),
            );
        }
        let _ = memo.whole_env.set(Arc::new(env.clone()));
        memo
    }

    pub(crate) fn header_index(&self) -> &Arc<DeclHeaderIndex> {
        &self.header_index
    }

    /// The file's statically-classified [`FileLanguage`], derived from the
    /// memo's canonical id through the global registry (no host needed) so the
    /// lazy memo path stays self-contained. This is the rune-ambient
    /// classification source for both the whole-env oracle and the centralized
    /// effective-lookup.
    fn rune_module_file_language(&self) -> verter_language::FileLanguage {
        verter_language::LanguageRegistry::global()
            .classify_static(self.key.canonical.as_ref())
            .static_resolution()
    }

    /// Whether this file is a Svelte standalone rune module — the gate the
    /// centralized effective-lookup applies before consulting the rune
    /// ambient inventory (per-file scoping). Classified from the canonical id,
    /// so a plain `.ts` / `.js` never reports `true`.
    pub(crate) fn is_rune_module(&self) -> bool {
        crate::host_resolve::is_svelte_rune_module(&self.rune_module_file_language())
    }

    /// The retained framework parse artifact for this content generation, when
    /// the file is a framework carrier. This is the SAME artifact the indexing
    /// flight resolved — exposed so the component-default synth seam can read
    /// the carrier's module-script region without re-fetching it through
    /// `current_eval_state` (which re-enters `current_content_pinned_indexed`
    /// for the owner and recurses while the owner is mid-index).
    pub(crate) fn framework_parse(&self) -> Option<&Arc<verter_language::FrameworkParseArtifact>> {
        self.framework_parse.as_ref()
    }

    /// Acquire (once) the lease pinning this memo's retained parse
    /// snapshot for the rest of the memo's life. Called before every
    /// service-backed run so the snapshot stays warm across every body /
    /// whole-env / raw-surface demand on this content generation — a live
    /// artifact never silently re-parses. The single eval-program parse
    /// is counted HERE (the lease acquisition); subsequent `service.run`
    /// calls reuse the pinned snapshot and report `parsed_now == false`.
    /// A seeded memo (no service) never acquires a lease.
    fn ensure_lease(&self) {
        let Some(service) = self.service.as_ref() else {
            return;
        };
        self.lease.get_or_init(|| {
            let outcome = service.acquire_lease(&self.key, &self.eval_source, self.source_type);
            if outcome.parsed_now {
                self.provenance
                    .eval_program_parses
                    .fetch_add(1, Ordering::Relaxed);
            }
            outcome.lease
        });
    }

    /// Demand the lowered body of one file-scope TYPE symbol.
    pub(crate) fn type_decl(&self, name: &str) -> Option<Arc<LoweredTypeDecl>> {
        let (contributors, from_jsdoc) = {
            let header = self.header_index.type_header(name)?;
            (header.contributors.clone(), header.from_jsdoc_typedef)
        };
        let cell = self
            .type_entries
            .entry(name.to_string())
            .or_default()
            .clone();
        // Backfill runs OUTSIDE `get_or_init` — see [`Self::backfill`]. The
        // closure stashes the leftover batch; only the initializing thread
        // (the one that produced `Some` here) takes it and backfills, after
        // the demanded cell's init-lock is already released.
        let leftover: std::cell::Cell<Option<LoweredStatementBatch>> = std::cell::Cell::new(None);
        let result = cell
            .get_or_init(|| {
                self.lower_demanded(name, &contributors, from_jsdoc)
                    .and_then(|batch| {
                        let result = batch
                            .types
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, decl)| Arc::new(decl.clone()));
                        leftover.set(Some(batch));
                        result
                    })
            })
            .clone();
        if let Some(batch) = leftover.take() {
            self.backfill(batch, &contributors, Some((SymbolSpace::Type, name)), None);
        }
        result
    }

    /// Demand the lowered body of one file-scope VALUE symbol.
    pub(crate) fn value_decl(&self, name: &str) -> Option<Arc<LoweredValueDecl>> {
        let contributors = self.header_index.value_header(name)?.contributors.clone();
        let cell = self
            .value_entries
            .entry(name.to_string())
            .or_default()
            .clone();
        let leftover: std::cell::Cell<Option<LoweredStatementBatch>> = std::cell::Cell::new(None);
        let result = cell
            .get_or_init(|| {
                self.lower_demanded(name, &contributors, false)
                    .and_then(|batch| {
                        let result = batch
                            .values
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, decl)| Arc::new(decl.clone()));
                        leftover.set(Some(batch));
                        result
                    })
            })
            .clone();
        if let Some(batch) = leftover.take() {
            self.backfill(batch, &contributors, Some((SymbolSpace::Value, name)), None);
        }
        result
    }

    /// The fingerprint hash INPUT for a file-scope TYPE symbol's body — the
    /// single output/compat body read on the memo side, used by the parse-time
    /// fact emitter to compute a body fingerprint (`semantic_hash` /
    /// `display_hash`) and nothing else.
    ///
    /// This is deliberately NARROW and PURPOSE-NAMED (a fingerprint hash input,
    /// not a general body accessor): it owns the one place the body fact path
    /// reads a TYPE declaration body as a `TypeExpr`, so the body STORAGE can
    /// later change shape (a handle carrier) by reworking THIS helper's
    /// internals without preserving a broad `TypeExpr` body API. It returns the
    /// folded object view (`lookup_object`) exactly as the inline read did, so
    /// the computed fingerprint is byte-identical.
    ///
    /// TEMPORARY compat surface: it exists only so the body-fact fingerprint
    /// path is fenced off from the semantic readers (which still read the typed
    /// body directly). It is anchored as a COMPAT site by the frozen
    /// body-reader inventory guard.
    pub(crate) fn compat_type_body_hash_input(&self, name: &str) -> Option<TypeExpr> {
        Some(self.type_decl(name)?.body.lookup_object().into_owned())
    }

    /// Demand the lowered body of one augmentation-scoped TYPE symbol.
    pub(crate) fn augmentation_type_decl(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<Arc<LoweredTypeDecl>> {
        let contributors = self
            .header_index
            .augmentation_type_header(scope, name)?
            .contributors
            .clone();
        let cell = self
            .aug_type_entries
            .entry((scope.clone(), name.to_string()))
            .or_default()
            .clone();
        let leftover: std::cell::Cell<Option<LoweredStatementBatch>> = std::cell::Cell::new(None);
        let result = cell
            .get_or_init(|| {
                self.lower_demanded(name, &contributors, false)
                    .and_then(|batch| {
                        let result = batch
                            .aug_types
                            .iter()
                            .find(|(s, n, _)| s == scope && n == name)
                            .map(|(_, _, decl)| Arc::new(decl.clone()));
                        leftover.set(Some(batch));
                        result
                    })
            })
            .clone();
        if let Some(batch) = leftover.take() {
            self.backfill(
                batch,
                &contributors,
                None,
                Some((scope, SymbolSpace::Type, name)),
            );
        }
        result
    }

    /// Demand the lowered body of one augmentation-scoped VALUE symbol.
    pub(crate) fn augmentation_value_decl(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<Arc<LoweredValueDecl>> {
        let contributors = self
            .header_index
            .augmentation_value_header(scope, name)?
            .contributors
            .clone();
        let cell = self
            .aug_value_entries
            .entry((scope.clone(), name.to_string()))
            .or_default()
            .clone();
        let leftover: std::cell::Cell<Option<LoweredStatementBatch>> = std::cell::Cell::new(None);
        let result = cell
            .get_or_init(|| {
                self.lower_demanded(name, &contributors, false)
                    .and_then(|batch| {
                        let result = batch
                            .aug_values
                            .iter()
                            .find(|(s, n, _)| s == scope && n == name)
                            .map(|(_, _, decl)| Arc::new(decl.clone()));
                        leftover.set(Some(batch));
                        result
                    })
            })
            .clone();
        if let Some(batch) = leftover.take() {
            self.backfill(
                batch,
                &contributors,
                None,
                Some((scope, SymbolSpace::Value, name)),
            );
        }
        result
    }

    /// The whole-file eval environment — a DEMAND product for whole-file
    /// consumers. Its most-hit consumer is `local_type_declaration_id`
    /// (type-decl identity resolution, reached on every `get_component_meta`
    /// resolution via `base_eval_env_arc`); the others are fallthrough,
    /// runtime values, and value-alias peeling. Built once through the
    /// retained snapshot and memoized; the per-symbol query path never
    /// touches it.
    pub fn whole_env(&self) -> Arc<EvalEnv> {
        self.whole_env
            .get_or_init(|| {
                let Some(service) = self.service.as_ref() else {
                    // Seeded memos pre-set the env; an un-seeded memo
                    // without a service cannot build one.
                    return Arc::new(EvalEnv::default());
                };
                // Pin the retained snapshot for this memo's lifetime
                // (parse counted at lease acquisition); the run below
                // reuses it.
                self.ensure_lease();
                let outcome = service.run(
                    &self.key,
                    &self.eval_source,
                    self.source_type,
                    move |program| {
                        program
                            .map(|p| build_eval_env(p.borrow_dependent(), p.source_str()))
                            .unwrap_or_default()
                    },
                );
                let mut env = outcome.value;
                self.provenance
                    .eval_env_builds
                    .fetch_add(1, Ordering::Relaxed);
                self.provenance
                    .decl_bodies_lowered
                    .fetch_add(env.total_decl_count() as u64, Ordering::Relaxed);
                crate::host_resolve::apply_sfc_script_setup_type_params(
                    &mut env,
                    self.raw_source.as_ref(),
                    self.framework_parse.as_deref(),
                );
                // A Svelte rune module (`.svelte.ts` / `.svelte.js`) merges the
                // module-valid runes into its whole env so its exported
                // rune-derived types infer correctly — per-file scoped, no
                // eval_source byte change. The runes are sourced from the SAME
                // centralized rune ambient inventory the graph-native
                // effective-lookup consults, so the oracle and the per-symbol
                // readers agree on rune visibility. Classify from the canonical
                // via the static registry (no host needed) so the lazy memo
                // path stays self-contained.
                let file_language = self.rune_module_file_language();
                crate::host_resolve::merge_rune_ambient_into_env(&mut env, &file_language);
                Arc::new(env)
            })
            .clone()
    }

    /// Whether the whole-file env has already been materialised (test
    /// observability — never a validity signal).
    #[cfg(test)]
    pub(crate) fn whole_env_materialized(&self) -> bool {
        self.whole_env.get().is_some()
    }

    /// Demand the parse-time `RawSourceSurface` contributor vector for
    /// one `(name, symbol_space)` — captured from exactly the demanded
    /// symbol's contributing statements through the retained snapshot,
    /// memoized per triple.
    pub fn raw_surfaces_for(&self, name: &str, space: SymbolSpace) -> Arc<Vec<RawSourceSurface>> {
        if let Some(cached) = self.raw_surfaces.get(&(name.to_string(), space)) {
            return Arc::clone(&cached);
        }

        let mut contributors: Vec<u32> = Vec::new();
        match space {
            SymbolSpace::Type => {
                if let Some(header) = self.header_index.type_header(name) {
                    contributors.extend_from_slice(&header.contributors);
                }
                // An enum is registered dual-space, so its TYPE header above
                // already carries these locators; the dedicated enum table is
                // the member-NAME authority, and folding its contributor
                // locators in defensively keeps the capture complete even if a
                // refactor ever decoupled the two (deduped below).
                if let Some(header) = self.header_index.enum_headers.get(name) {
                    contributors.extend_from_slice(&header.contributors);
                }
            }
            SymbolSpace::Value => {
                if let Some(header) = self.header_index.value_header(name) {
                    contributors.extend_from_slice(&header.contributors);
                }
                if let Some(header) = self.header_index.enum_headers.get(name) {
                    contributors.extend_from_slice(&header.contributors);
                }
            }
        }
        contributors.sort_unstable();
        contributors.dedup();

        let surfaces =
            if let (false, Some(service)) = (contributors.is_empty(), self.service.as_ref()) {
                self.ensure_lease();
                let canonical = self.key.canonical.to_string();
                let wanted = name.to_string();
                let outcome = service.run(
                    &self.key,
                    &self.eval_source,
                    self.source_type,
                    move |program| {
                        let Some(program) = program else {
                            return Vec::new();
                        };
                        let program = program.borrow_dependent();
                        let captured: Vec<_> = contributors
                            .iter()
                            .filter_map(|index| program.body.get(*index as usize))
                            .flat_map(capture_statement_surfaces)
                            .collect();
                        merge_overload_groups(captured)
                            .into_iter()
                            .filter(|c| c.name == wanted && c.symbol_space == space)
                            .map(|c| {
                                let mut surface = c.surface;
                                surface.decl_canonical = canonical.clone();
                                surface
                            })
                            .collect::<Vec<_>>()
                    },
                );
                outcome.value
            } else {
                Vec::new()
            };

        let surfaces = Arc::new(surfaces);
        self.raw_surfaces
            .insert((name.to_string(), space), Arc::clone(&surfaces));
        surfaces
    }

    /// Lower the demanded symbol's contributing statements through the
    /// retained snapshot, producing the owned per-symbol batch. `None`
    /// on a fatal parse.
    ///
    /// Unlike [`Self::whole_env`], this per-symbol path deliberately does
    /// NOT call `apply_sfc_script_setup_type_params`: a `<script setup
    /// generic="T">` parameter is never resolved through a per-symbol
    /// `type_decl` demand. SFC own-file type bodies referencing `T`
    /// resolve through the dispatch `DeclarationScopePayload`
    /// (`scope_type_bindings`, sourced from the prepared-decl bundle's
    /// script-setup type bindings), which is consulted BEFORE any fallback
    /// to the per-symbol prepared-decl lookup — so the generic is already
    /// bound and never reaches this scratch env.
    fn lower_demanded(
        &self,
        name: &str,
        contributors: &[u32],
        from_jsdoc_typedef: bool,
    ) -> Option<LoweredStatementBatch> {
        let service = self.service.as_ref()?;
        self.ensure_lease();
        let contributors = contributors.to_vec();
        let name = name.to_string();
        let outcome = service.run(
            &self.key,
            &self.eval_source,
            self.source_type,
            move |program| {
                let program = program?;
                let source = program.source_str();
                let program = program.borrow_dependent();

                let mut scratch = EvalEnv::new();
                let mut dep_records: FxHashMap<String, (FxHashSet<String>, FxHashSet<String>)> =
                    FxHashMap::default();
                for index in &contributors {
                    let Some(stmt) = program.body.get(*index as usize) else {
                        continue;
                    };
                    lower_top_level_statement(stmt, source, &mut scratch);
                    for (decl_name, deps) in collect_statement_dependency_names(stmt) {
                        let entry = dep_records.entry(decl_name).or_default();
                        entry.0.extend(deps.dependency_names);
                        entry.1.extend(deps.structural_dependency_names);
                    }
                }
                if from_jsdoc_typedef
                    && lower_jsdoc_typedef_named(&program.comments, source, &name, &mut scratch)
                {
                    // A JSDoc `@typedef` is NOT a statement, so the statement
                    // dep-collector never produces its reference edges. Derive
                    // the dependency roots from the lowered JSDoc body so the
                    // cached entry carries them (else the typedef caches with
                    // EMPTY deps → under-resolution + under-invalidation).
                    // Stored in BOTH the plain and structural sets: a typedef
                    // is an alias carrier, so its roots are structural for the
                    // required-import walk (conservative — never under-walks).
                    if let Some(group) = scratch.type_symbols.get(&name) {
                        let mut refs = Vec::new();
                        for contributor in group.contributors() {
                            collect_type_refs(&contributor.body, &mut refs);
                        }
                        let entry = dep_records.entry(name.clone()).or_default();
                        for reference in refs {
                            entry.0.insert(reference.clone());
                            entry.1.insert(reference);
                        }
                    }
                }

                let lowered_count = scratch.total_decl_count();
                let mut batch = LoweredStatementBatch {
                    types: Vec::new(),
                    values: Vec::new(),
                    aug_types: Vec::new(),
                    aug_values: Vec::new(),
                    lowered_count,
                };
                for (decl_name, group) in &scratch.type_symbols {
                    let (deps, structural) =
                        dep_records.get(decl_name).cloned().unwrap_or_default();
                    // An enum's type-space body is derived from its MERGED
                    // value members (same name → matching value group), so the
                    // type and value spaces never diverge.
                    let enum_body = scratch
                        .value_symbols
                        .get(decl_name)
                        .and_then(ValueDeclGroup::enum_type_union);
                    batch.types.push((
                        decl_name.clone(),
                        lowered_type_decl_from_group(group, deps, structural, enum_body),
                    ));
                }
                for (decl_name, group) in &scratch.value_symbols {
                    batch
                        .values
                        .push((decl_name.clone(), lowered_value_decl_from_group(group)));
                }
                for ((scope, decl_name), group) in &scratch.augmentation_scopes {
                    // Ambient augmentation blocks do not inventory enum
                    // declarations, so no value-derived enum union applies here.
                    batch.aug_types.push((
                        scope.clone(),
                        decl_name.clone(),
                        lowered_type_decl_from_group(
                            group,
                            FxHashSet::default(),
                            FxHashSet::default(),
                            None,
                        ),
                    ));
                }
                for ((scope, decl_name), group) in &scratch.augmentation_value_scopes {
                    batch.aug_values.push((
                        scope.clone(),
                        decl_name.clone(),
                        lowered_value_decl_from_group(group),
                    ));
                }
                Some(batch)
            },
        );
        let batch = outcome.value?;
        self.provenance
            .decl_bodies_lowered
            .fetch_add(batch.lowered_count as u64, Ordering::Relaxed);
        Some(batch)
    }

    /// Populate sibling entries the demanded statements ALSO declared
    /// (set-if-vacant; the demanded entry itself is excluded — it was
    /// already published by the `get_or_init` that produced this batch).
    ///
    /// Runs OUTSIDE the demanded cell's `get_or_init` closure, on the
    /// initializing thread only: publishing a sibling space with a
    /// blocking `OnceLock::set` while still holding the demanded cell's
    /// init-lock would deadlock against a concurrent demand of that
    /// sibling (a merged `class K {}` occupies BOTH the type and value
    /// space — type demand sets the value cell, value demand sets the type
    /// cell). With the demanded init-lock released first, a sibling `set`
    /// that races a concurrent initializer just returns `Err`; the
    /// concurrent initializer never waits on us, so no cycle forms.
    /// Coverage-gated: a sibling
    /// backfills ONLY when the lowered statement set covers ALL of that
    /// symbol's header contributors — a statement batch that lowered a
    /// SUBSET (the class half of an interface+class merge, demanded via
    /// its value side) must not pre-fill the full entry. Only recorded,
    /// actually lowered results enter — never broader pretend-coverage.
    fn backfill(
        &self,
        batch: LoweredStatementBatch,
        lowered_statements: &[u32],
        demanded_file_scope: Option<(SymbolSpace, &str)>,
        demanded_augmentation: Option<(&AugmentationScopeKind, SymbolSpace, &str)>,
    ) {
        let covers =
            |contributors: &[u32]| contributors.iter().all(|c| lowered_statements.contains(c));
        for (name, decl) in batch.types {
            if demanded_file_scope == Some((SymbolSpace::Type, name.as_str())) {
                continue;
            }
            if !self
                .header_index
                .type_header(&name)
                .is_some_and(|header| covers(&header.contributors))
            {
                continue;
            }
            let cell = self.type_entries.entry(name).or_default().clone();
            let _ = cell.set(Some(Arc::new(decl)));
        }
        for (name, decl) in batch.values {
            if demanded_file_scope == Some((SymbolSpace::Value, name.as_str())) {
                continue;
            }
            if !self
                .header_index
                .value_header(&name)
                .is_some_and(|header| covers(&header.contributors))
            {
                continue;
            }
            let cell = self.value_entries.entry(name).or_default().clone();
            let _ = cell.set(Some(Arc::new(decl)));
        }
        for (scope, name, decl) in batch.aug_types {
            if demanded_augmentation == Some((&scope, SymbolSpace::Type, name.as_str())) {
                continue;
            }
            if !self
                .header_index
                .augmentation_type_header(&scope, &name)
                .is_some_and(|header| covers(&header.contributors))
            {
                continue;
            }
            let cell = self
                .aug_type_entries
                .entry((scope, name))
                .or_default()
                .clone();
            let _ = cell.set(Some(Arc::new(decl)));
        }
        for (scope, name, decl) in batch.aug_values {
            if demanded_augmentation == Some((&scope, SymbolSpace::Value, name.as_str())) {
                continue;
            }
            if !self
                .header_index
                .augmentation_value_header(&scope, &name)
                .is_some_and(|header| covers(&header.contributors))
            {
                continue;
            }
            let cell = self
                .aug_value_entries
                .entry((scope, name))
                .or_default()
                .clone();
            let _ = cell.set(Some(Arc::new(decl)));
        }
    }
}

/// Fold one same-name TYPE contributor group into the lazily-served
/// per-symbol record — the same body merge / parameter union the eager
/// shallow build performed per symbol.
///
/// `enum_type_body` is the enum's value-derived type union, supplied by the
/// caller when this type name is an `enum` (see
/// [`ValueDeclGroup::enum_type_union`]). When `Some`, it REPLACES the group's
/// `merged_body()`: an enum's type-space body is the union of its member value
/// literals, derived from the MERGED value members so the type and value
/// spaces never diverge (a per-contributor `merged_body()` fold would be
/// last-wins for the enum's `Alias`-kind group and drop earlier declarations'
/// members).
fn lowered_type_decl_from_group(
    group: &verter_semantic::analysis::type_eval::TypeDeclGroup,
    dep_names: FxHashSet<String>,
    structural_dep_names: FxHashSet<String>,
    enum_type_body: Option<TypeExpr>,
) -> LoweredTypeDecl {
    let primary = group.primary();
    let body = match enum_type_body {
        Some(union) => TypeDeclBody::Single(union),
        None => group.merged_body(),
    };
    let lookup = body.lookup_object();

    let mut type_parameters: Vec<TypeParam> = Vec::new();
    for decl in group.contributors() {
        for param in &decl.type_parameters {
            if !type_parameters.iter().any(|p| p.name == param.name) {
                type_parameters.push(param.clone());
            }
        }
    }

    let member_deps = extract_member_deps(lookup.as_ref());
    let mut typeof_roots = FxHashSet::default();
    collect_typeof_roots(lookup.as_ref(), &mut typeof_roots);
    drop(lookup);
    let mut typeof_root_names: Vec<String> = typeof_roots.into_iter().collect();
    typeof_root_names.sort_unstable();

    LoweredTypeDecl {
        kind: primary.kind,
        body,
        type_parameters,
        dep_names,
        structural_dep_names,
        member_deps,
        typeof_root_names,
    }
}

/// Fold one same-name VALUE contributor group into the lazily-served
/// per-symbol record. An enum's FULL member set (every member's
/// [`EnumMemberValue`]) is unioned across every same-name contributor via
/// [`ValueDeclGroup::merged_enum_unified`] (NOT `primary()`-only, which would
/// drop earlier merged declarations' members) so the type/value projection
/// surfaces and the value-body fingerprint both read from one lossless rail.
fn lowered_value_decl_from_group(group: &ValueDeclGroup) -> LoweredValueDecl {
    let primary = group.primary();
    LoweredValueDecl {
        kind: primary.kind,
        type_annotation: primary.type_annotation.clone(),
        signatures: group.merged_signatures(),
        object_shape: primary.object_shape.clone(),
        enum_members: group.merged_enum_unified(),
    }
}

#[cfg(test)]
#[path = "decl_body_memo_tests.rs"]
mod decl_body_memo_tests;
