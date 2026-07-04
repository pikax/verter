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
use verter_type_expr::locators::{
    AuthoredAugmentationScope, AuthoredBodyLocator, LocatorSymbolSpace, MacroPayloadPosition,
    TypeBodyPathStep,
};
use verter_type_expr::{ObjectExpr, ObjectMember, TypeExpr, TypeParam};

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

/// Outcome of a demanded per-symbol lowering ([`DeclBodyMemo::lower_demanded`]).
///
/// The two `None`-shaped miss classes are DISTINCT and must be handled
/// differently by the caller's memo commit:
///
/// - [`Ready`](Self::Ready) — the lease-only run completed. `Some(batch)` is
///   the lowered product; `None` is a GENUINE miss (no service on a seeded
///   memo, or a fatal parse) whose body-less result is CORRECT and cacheable.
/// - [`LeaseMiss`](Self::LeaseMiss) — the lease pin was broken (unreachable in
///   practice): the lowering did not run and produced NOTHING. Fail CLOSED via
///   ReturnOnly — the caller must NOT memoize this as a body-less warm entry
///   (a silent wrong-empty result), in DEBUG *or* RELEASE. A later demand
///   under a live lease recovers.
enum DemandLower {
    Ready(Option<LoweredStatementBatch>),
    LeaseMiss,
}

/// Outcome of a per-symbol body DEMAND ([`DeclBodyMemo::demand_and_commit`])
/// as seen by a caller that needs to DISTINGUISH the two `None`-shaped miss
/// classes (the locator-deref path, which must not collapse a transient
/// ReturnOnly into a cacheable resolution result):
///
/// - [`Ready`](Self::Ready) — the lease-only run completed. `Some` is the
///   demanded decl; `None` is a GENUINE, cacheable miss (the symbol is not
///   inventoried, or the run produced a fatal-parse empty).
/// - [`LeaseMiss`](Self::LeaseMiss) — the lease pin was broken: the demand
///   ran NOTHING and committed NOTHING (`ReturnOnly`). A caller must route
///   this to a no-warm signal, never treat it as a genuine miss.
enum DemandOutcome<D> {
    Ready(Option<Arc<D>>),
    LeaseMiss,
}

impl<D> DemandOutcome<D> {
    /// Collapse to the plain `Option` API: a lease-miss reads as `None`. Used
    /// by the broad `Option`-returning demand accessors whose consumers do
    /// NOT distinguish the transient ReturnOnly from a genuine miss (the
    /// per-symbol demand cell already fails closed by evicting the poisoned
    /// cell, so a later demand under a live lease recovers).
    fn into_option(self) -> Option<Arc<D>> {
        match self {
            DemandOutcome::Ready(value) => value,
            DemandOutcome::LeaseMiss => None,
        }
    }
}

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
    /// is counted HERE (the lease acquisition); every subsequent demand
    /// runs LEASE-ONLY (`run_leased`) against the pinned snapshot, so a
    /// broken pin is a lowering MISS, never a transient re-parse.
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
        self.type_decl_outcome(name).into_option()
    }

    /// Demand the lowered body of one file-scope TYPE symbol, PRESERVING the
    /// lease-miss ReturnOnly outcome distinctly. The locator-deref path uses
    /// this so a broken-lease demand becomes a typed no-warm signal rather
    /// than collapsing into a cacheable genuine miss.
    fn type_decl_outcome(&self, name: &str) -> DemandOutcome<LoweredTypeDecl> {
        let Some((contributors, from_jsdoc)) = self
            .header_index
            .type_header(name)
            .map(|header| (header.contributors.clone(), header.from_jsdoc_typedef))
        else {
            // Not inventoried: a genuine, cacheable absence — never a
            // lease-miss.
            return DemandOutcome::Ready(None);
        };
        let cell = self
            .type_entries
            .entry(name.to_string())
            .or_default()
            .clone();
        // Backfill runs OUTSIDE the cell commit — see [`Self::backfill`]. The
        // initializing caller receives the batch and backfills siblings after
        // its own cell is committed; a lease-miss evicts the cell and commits
        // nothing.
        let (outcome, batch) = self.demand_and_commit(
            &cell,
            name,
            &contributors,
            from_jsdoc,
            |batch| {
                batch
                    .types
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, decl)| Arc::new(decl.clone()))
            },
            || {
                self.type_entries.remove(name);
            },
        );
        if let Some(batch) = batch {
            self.backfill(batch, &contributors, Some((SymbolSpace::Type, name)), None);
        }
        outcome
    }

    /// Demand the lowered body of one file-scope VALUE symbol.
    pub(crate) fn value_decl(&self, name: &str) -> Option<Arc<LoweredValueDecl>> {
        self.value_decl_outcome(name).into_option()
    }

    /// Demand the lowered body of one file-scope VALUE symbol, PRESERVING the
    /// lease-miss ReturnOnly outcome distinctly (locator-deref no-warm rail).
    fn value_decl_outcome(&self, name: &str) -> DemandOutcome<LoweredValueDecl> {
        let Some(contributors) = self
            .header_index
            .value_header(name)
            .map(|header| header.contributors.clone())
        else {
            return DemandOutcome::Ready(None);
        };
        let cell = self
            .value_entries
            .entry(name.to_string())
            .or_default()
            .clone();
        let (outcome, batch) = self.demand_and_commit(
            &cell,
            name,
            &contributors,
            false,
            |batch| {
                batch
                    .values
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, decl)| Arc::new(decl.clone()))
            },
            || {
                self.value_entries.remove(name);
            },
        );
        if let Some(batch) = batch {
            self.backfill(batch, &contributors, Some((SymbolSpace::Value, name)), None);
        }
        outcome
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
        self.augmentation_type_decl_outcome(scope, name)
            .into_option()
    }

    /// Demand the lowered body of one augmentation-scoped TYPE symbol,
    /// PRESERVING the lease-miss ReturnOnly outcome distinctly (locator-deref
    /// no-warm rail).
    fn augmentation_type_decl_outcome(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> DemandOutcome<LoweredTypeDecl> {
        let Some(contributors) = self
            .header_index
            .augmentation_type_header(scope, name)
            .map(|header| header.contributors.clone())
        else {
            return DemandOutcome::Ready(None);
        };
        let cell = self
            .aug_type_entries
            .entry((scope.clone(), name.to_string()))
            .or_default()
            .clone();
        let (outcome, batch) = self.demand_and_commit(
            &cell,
            name,
            &contributors,
            false,
            |batch| {
                batch
                    .aug_types
                    .iter()
                    .find(|(s, n, _)| s == scope && n == name)
                    .map(|(_, _, decl)| Arc::new(decl.clone()))
            },
            || {
                self.aug_type_entries
                    .remove(&(scope.clone(), name.to_string()));
            },
        );
        if let Some(batch) = batch {
            self.backfill(
                batch,
                &contributors,
                None,
                Some((scope, SymbolSpace::Type, name)),
            );
        }
        outcome
    }

    /// Demand the lowered body of one augmentation-scoped VALUE symbol.
    pub(crate) fn augmentation_value_decl(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<Arc<LoweredValueDecl>> {
        self.augmentation_value_decl_outcome(scope, name)
            .into_option()
    }

    /// Demand the lowered body of one augmentation-scoped VALUE symbol,
    /// PRESERVING the lease-miss ReturnOnly outcome distinctly (locator-deref
    /// no-warm rail).
    fn augmentation_value_decl_outcome(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> DemandOutcome<LoweredValueDecl> {
        let Some(contributors) = self
            .header_index
            .augmentation_value_header(scope, name)
            .map(|header| header.contributors.clone())
        else {
            return DemandOutcome::Ready(None);
        };
        let cell = self
            .aug_value_entries
            .entry((scope.clone(), name.to_string()))
            .or_default()
            .clone();
        let (outcome, batch) = self.demand_and_commit(
            &cell,
            name,
            &contributors,
            false,
            |batch| {
                batch
                    .aug_values
                    .iter()
                    .find(|(s, n, _)| s == scope && n == name)
                    .map(|(_, _, decl)| Arc::new(decl.clone()))
            },
            || {
                self.aug_value_entries
                    .remove(&(scope.clone(), name.to_string()));
            },
        );
        if let Some(batch) = batch {
            self.backfill(
                batch,
                &contributors,
                None,
                Some((scope, SymbolSpace::Value, name)),
            );
        }
        outcome
    }

    /// The whole-file eval environment — a DEMAND product for whole-file
    /// consumers. Its most-hit consumer is `local_type_declaration_id`
    /// (type-decl identity resolution, reached on every `get_component_meta`
    /// resolution via `base_eval_env_arc`); the others are fallthrough,
    /// runtime values, and value-alias peeling. Built once through the
    /// retained snapshot and memoized; the per-symbol query path never
    /// touches it.
    pub fn whole_env(&self) -> Arc<EvalEnv> {
        // Warm path.
        if let Some(cached) = self.whole_env.get() {
            return cached.clone();
        }
        let Some(service) = self.service.as_ref() else {
            // Seeded memos pre-set the env; an un-seeded memo without a service
            // has no body to lower — the empty env is the CORRECT value, cache
            // it (this is a genuine miss, not a lease-pin break).
            return self
                .whole_env
                .get_or_init(|| Arc::new(EvalEnv::default()))
                .clone();
        };
        // Pin the retained snapshot for this memo's lifetime (parse counted at
        // lease acquisition); the LEASE-ONLY run below reuses it.
        self.ensure_lease();
        let Some(mut env) = service.run_leased(&self.key, move |program| {
            program
                .map(|p| build_eval_env(p.borrow_dependent(), p.source_str()))
                .unwrap_or_default()
        }) else {
            // Broken lease pin (unreachable in practice): fail CLOSED via
            // ReturnOnly. NEVER memoize the empty env — that is the silent
            // wrong-empty warm entry release builds used to admit; a retry
            // under a live lease recovers. Loud, not silent.
            tracing::error!(
                canonical = %self.key.canonical,
                "decl-body lease pin broken: whole_env's lease-only run missed the \
                 retained snapshot; failing closed to an uncached empty env (ReturnOnly)"
            );
            return Arc::new(EvalEnv::default());
        };
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
        // Commit only the REAL env (idempotent — a cold race loses harmlessly).
        self.whole_env.get_or_init(|| Arc::new(env)).clone()
    }

    /// Whether the whole-file env has already been materialised (test
    /// observability — never a validity signal).
    #[cfg(test)]
    pub(crate) fn whole_env_materialized(&self) -> bool {
        self.whole_env.get().is_some()
    }

    /// Whether a per-symbol TYPE cell has a COMMITTED entry (test
    /// observability — never a validity signal). A lease-miss ReturnOnly
    /// leaves the (lazily-created) cell uninitialised, so this returns `false`.
    #[cfg(test)]
    pub(crate) fn type_entry_materialized(&self, name: &str) -> bool {
        self.type_entries
            .get(name)
            .is_some_and(|cell| cell.get().is_some())
    }

    /// Whether a `(name, space)` raw-surface capture has a COMMITTED entry
    /// (test observability — never a validity signal). A lease-miss ReturnOnly
    /// never inserts, so this returns `false`.
    #[cfg(test)]
    pub(crate) fn raw_surfaces_materialized(&self, name: &str, space: SymbolSpace) -> bool {
        self.raw_surfaces.contains_key(&(name.to_string(), space))
    }

    /// Break the memo's worker-retained parse snapshot so the NEXT body
    /// demand lease-misses (test observability for the fail-closed ReturnOnly
    /// rail). The memo still HOLDS its `SnapshotLease` (so `ensure_lease`
    /// will not re-acquire), but the worker-side retained snapshot is
    /// released — mirroring the invariant-violation scenario. No-op on a
    /// seeded memo (no service).
    #[cfg(test)]
    pub(crate) fn release_retained_snapshot_for_test(&self) {
        if let Some(service) = self.service.as_ref() {
            service.release_retained_snapshot_for_test(&self.key);
        }
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
                // LEASE-ONLY run: never a transient re-parse. A broken lease
                // pin (the run misses the retained snapshot) fails CLOSED via
                // ReturnOnly BELOW — the empty capture is returned UNCACHED so
                // a lease-pin break can never silently memoize a wrong-empty
                // capture (in DEBUG *or* RELEASE).
                let leased = service.run_leased(&self.key, move |program| {
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
                });
                let Some(surfaces) = leased else {
                    // Broken lease pin (unreachable in practice): ReturnOnly —
                    // return the empty capture WITHOUT memoizing it; a retry
                    // under a live lease recovers. Loud, not silent.
                    tracing::error!(
                        canonical = %self.key.canonical,
                        "decl-body lease pin broken: raw_surfaces_for's lease-only run \
                         missed the retained snapshot; failing closed to an uncached \
                         empty capture (ReturnOnly)"
                    );
                    return Arc::new(Vec::new());
                };
                surfaces
            } else {
                // No contributors / no service: a GENUINE empty capture — cache
                // it (the demanded symbol has no parse-time surfaces).
                Vec::new()
            };

        let surfaces = Arc::new(surfaces);
        self.raw_surfaces
            .insert((name.to_string(), space), Arc::clone(&surfaces));
        surfaces
    }

    /// Lower the demanded symbol's contributing statements through the
    /// retained snapshot, producing the owned per-symbol batch. `None`
    /// on a fatal parse — or on a broken lease pin (the LEASE-ONLY run
    /// fails CLOSED to the lowering miss, loudly in debug/test builds;
    /// it can never transiently re-parse).
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
    ) -> DemandLower {
        // A seeded memo has no service: nothing to lower, a genuine (cacheable)
        // body-less miss — NOT a lease-pin break.
        let Some(service) = self.service.as_ref() else {
            return DemandLower::Ready(None);
        };
        self.ensure_lease();
        let contributors = contributors.to_vec();
        let name = name.to_string();
        let outcome = service.run_leased(&self.key, move |program| {
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
                let (deps, structural) = dep_records.get(decl_name).cloned().unwrap_or_default();
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
        });
        // Outer `None` = a broken lease pin (the lease-only run missed the
        // retained snapshot; the job did NOT run). Fail CLOSED via ReturnOnly:
        // this must NEVER be memoized as a body-less warm entry, in DEBUG *or*
        // RELEASE (silent wrong-empty is the defect the prior debug-only
        // `debug_assert!` left latent in release). Loud, not silent
        // (fail-lowering, not silent-skip); a later demand under a live lease
        // recovers. Inner `Some/None` = the run completed (batch / fatal-parse
        // genuine miss) — the caller may cache it.
        let Some(inner) = outcome else {
            tracing::error!(
                canonical = %self.key.canonical,
                "decl-body lease pin broken: the demanded lowering's lease-only run \
                 missed the retained snapshot; failing closed to ReturnOnly (uncached)"
            );
            return DemandLower::LeaseMiss;
        };
        if let Some(batch) = inner.as_ref() {
            self.provenance
                .decl_bodies_lowered
                .fetch_add(batch.lowered_count as u64, Ordering::Relaxed);
        }
        DemandLower::Ready(inner)
    }

    /// Get-or-compute a per-symbol cell under `get_or_init` single-flight, with
    /// a lease-miss ReturnOnly rail.
    ///
    /// The demanded lowering runs INSIDE `get_or_init` so a symbol demanded
    /// concurrently lowers exactly ONCE (the hot-path single-flight contract).
    /// A [`DemandLower::Ready`] commits the extracted decl and returns the batch
    /// so the initializing caller can backfill siblings. A
    /// [`DemandLower::LeaseMiss`] transiently commits `None` under the init lock
    /// (unavoidable with a write-once `OnceLock`), then the caller's
    /// `on_lease_miss_evict` DROPS the poisoned cell from its owning map — so no
    /// future demand serves the wrong-empty warm entry and the next demand
    /// retries under a live lease. Fail CLOSED via ReturnOnly, in DEBUG *and*
    /// RELEASE.
    fn demand_and_commit<D>(
        &self,
        cell: &OnceLock<Option<Arc<D>>>,
        name: &str,
        contributors: &[u32],
        from_jsdoc: bool,
        extract: impl FnOnce(&LoweredStatementBatch) -> Option<Arc<D>>,
        on_lease_miss_evict: impl FnOnce(),
    ) -> (DemandOutcome<D>, Option<LoweredStatementBatch>) {
        if let Some(cached) = cell.get() {
            return (DemandOutcome::Ready(cached.clone()), None);
        }
        let leftover: std::cell::Cell<Option<LoweredStatementBatch>> = std::cell::Cell::new(None);
        let lease_missed = std::cell::Cell::new(false);
        let result = cell
            .get_or_init(
                || match self.lower_demanded(name, contributors, from_jsdoc) {
                    DemandLower::Ready(maybe_batch) => {
                        let decl = maybe_batch.as_ref().and_then(extract);
                        leftover.set(maybe_batch);
                        decl
                    }
                    DemandLower::LeaseMiss => {
                        lease_missed.set(true);
                        None
                    }
                },
            )
            .clone();
        if lease_missed.get() {
            // The cell transiently committed `None` under the init lock; drop
            // it from the owning map so it can never serve a wrong-empty warm
            // entry and the next demand retries. Surface the DISTINCT
            // `LeaseMiss` outcome so a caller that must not collapse this
            // transient ReturnOnly into a cacheable genuine miss (the
            // locator-deref path) can route it to a no-warm signal.
            on_lease_miss_evict();
            return (DemandOutcome::LeaseMiss, None);
        }
        (DemandOutcome::Ready(result), leftover.take())
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

/// Why a locator deref could not produce the authored typed IR. Every
/// variant is a typed, fail-closed non-result — a deref NEVER fabricates a
/// body and NEVER falls back to a transient re-parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocatorBodyDerefError {
    /// The locator anchor names no inventoried declaration. This is a
    /// GENUINE, cacheable resolution result (the symbol truly does not
    /// exist) — DISTINCT from [`Self::LeaseMiss`].
    UnknownSymbol,
    /// The demanded body lowering hit a BROKEN lease pin (`ReturnOnly`): the
    /// lowering ran NOTHING and produced NOTHING. This is a transient no-warm
    /// signal, NOT a cacheable resolution result — the enclosing
    /// `LowerLocator` / `Instantiate` build must refuse warm admission
    /// (`cache_suppress`) so a later demand under a live lease recovers.
    /// Never collapsed into [`Self::UnknownSymbol`].
    LeaseMiss,
    /// The producer-emitted path does not resolve against the authored
    /// body (a stale / out-of-range ordinal, or a shape mismatch).
    PathUnresolved,
    /// A VALUE anchor whose declaration carries no authored type
    /// annotation — there is no authored TYPE body at that position.
    ValueAnnotationAbsent,
    /// Namespace bodies are not inventoried by the decl-body memo; a
    /// namespace anchor has no memo-backed authored body to deref.
    NamespaceBodyUnrouted,
    /// No consumer demands an augmentation-scoped VALUE / namespace body
    /// through a locator; the deref fails closed rather than fabricating one.
    AugmentationBodySpaceUnrouted,
    /// The macro generic type argument has exactly ONE sanctioned producer
    /// (`macro_type_arg_hot_ref`, the sole query-free structural
    /// macro-argument producer); a locator deref for it is rejected so a
    /// second producer path for the same payload can never exist.
    MacroTypeArgumentHasSoleHotMirrorProducer,
    /// No producer emits object-argument / analyzed-field payload
    /// locators; a deref for such a position fails closed with this typed
    /// error rather than fabricating a body.
    MacroPayloadPositionUnrouted,
}

/// The derefed authored SHAPE of a locator position: the whole decl body
/// (preserving the distinct merged-contributor carrier) or one
/// path-addressed sub-position.
#[derive(Debug, Clone)]
pub(crate) enum DerefedBodyShape {
    /// A single authored body / sub-position expression.
    Single(TypeExpr),
    /// The ordered same-name merged contributors of a whole merged decl
    /// body. Preserved as a DISTINCT carrier — never collapsed to an
    /// intersection (the merged-decl peer-merge reducer needs the
    /// contributor structure).
    Merged(Vec<TypeExpr>),
}

/// Owned typed-IR product of one locator deref: the derefed shape plus the
/// owning declaration's generic parameters (so the session phase can bind
/// them as `TypeParam` shells in the authored position's own lexical
/// scope). NEVER a `SemanticNodeId` — graph lowering is the session
/// phase's job.
#[derive(Debug, Clone)]
pub(crate) struct DerefedAuthoredBody {
    pub(crate) shape: DerefedBodyShape,
    pub(crate) type_parameters: Vec<TypeParam>,
}

impl DeclBodyMemo {
    /// Locator deref — the WORKER phase of locator-shape lowering: re-borrow
    /// the retained snapshot sub-position named by the locator's
    /// producer-emitted origin path and return transient OWNED typed IR.
    ///
    /// Lease-only purity: the deref serves through the memo's own lazy
    /// demand cells (`type_decl` / `value_decl` / `augmentation_type_decl`),
    /// whose demanded lowering (`lower_demanded`) runs through
    /// [`DeclLoweringService::run_leased`] against the memo's retained
    /// snapshot (the lease is [`Self::ensure_lease`]-pinned for the memo's
    /// lifetime) — NO transient parse (a broken lease pin is a lowering
    /// MISS), no host / dispatch / service re-entry inside the job.
    /// Authored macro payloads reuse THIS memo (the producing canonical's
    /// snapshot) — never a separate payload memo. Every failure is a typed
    /// [`LocatorBodyDerefError`], never a fabricated body.
    pub(crate) fn deref_locator_body(
        &self,
        locator: &AuthoredBodyLocator,
    ) -> Result<DerefedAuthoredBody, LocatorBodyDerefError> {
        match locator {
            AuthoredBodyLocator::MacroPayload(payload) => match payload.payload {
                // The macro generic type argument keeps its sole sanctioned
                // producer (`macro_type_arg_hot_ref`); rejecting the deref
                // here means a second producer path cannot come into
                // existence.
                MacroPayloadPosition::TypeArgument => {
                    Err(LocatorBodyDerefError::MacroTypeArgumentHasSoleHotMirrorProducer)
                }
                // No producer emits these payload locators; fail closed
                // with the typed non-result.
                MacroPayloadPosition::ObjectArgument | MacroPayloadPosition::Field { .. } => {
                    Err(LocatorBodyDerefError::MacroPayloadPositionUnrouted)
                }
            },
            AuthoredBodyLocator::DeclBody(slot) => {
                debug_assert_eq!(
                    slot.anchor.canonical_id.as_ref(),
                    self.key.canonical.as_ref(),
                    "a locator must deref through the memo of its OWN producing canonical"
                );
                match slot.anchor.space {
                    LocatorSymbolSpace::Type => {
                        // Serve through the memo's OWN lazy demand cell so a
                        // body lowers exactly once per (canonical, content,
                        // symbol) regardless of which route demands it first.
                        // A file-scope miss falls through to the GLOBAL
                        // ambient inventory — the same file-scope-then-global
                        // resolution order the prepared-decl route applies. A
                        // BROKEN-lease demand surfaces the DISTINCT `LeaseMiss`
                        // (a transient no-warm ReturnOnly), never collapsed into
                        // the cacheable `UnknownSymbol`.
                        let lowered = match self.type_decl_outcome(slot.anchor.symbol.as_ref()) {
                            DemandOutcome::LeaseMiss => {
                                return Err(LocatorBodyDerefError::LeaseMiss)
                            }
                            DemandOutcome::Ready(Some(lowered)) => lowered,
                            DemandOutcome::Ready(None) => {
                                match self.augmentation_type_decl_outcome(
                                    &AugmentationScopeKind::Global,
                                    slot.anchor.symbol.as_ref(),
                                ) {
                                    DemandOutcome::LeaseMiss => {
                                        return Err(LocatorBodyDerefError::LeaseMiss)
                                    }
                                    DemandOutcome::Ready(Some(lowered)) => lowered,
                                    DemandOutcome::Ready(None) => {
                                        return Err(LocatorBodyDerefError::UnknownSymbol)
                                    }
                                }
                            }
                        };
                        let shape = navigate_type_body(lowered.body.clone(), &slot.path)?;
                        Ok(DerefedAuthoredBody {
                            shape,
                            type_parameters: lowered.type_parameters.clone(),
                        })
                    }
                    LocatorSymbolSpace::Value => {
                        let lowered = match self.value_decl_outcome(slot.anchor.symbol.as_ref()) {
                            DemandOutcome::LeaseMiss => {
                                return Err(LocatorBodyDerefError::LeaseMiss)
                            }
                            DemandOutcome::Ready(Some(lowered)) => lowered,
                            DemandOutcome::Ready(None) => {
                                return Err(LocatorBodyDerefError::UnknownSymbol)
                            }
                        };
                        let annotation = lowered
                            .type_annotation
                            .clone()
                            .ok_or(LocatorBodyDerefError::ValueAnnotationAbsent)?;
                        let shape =
                            navigate_type_body(TypeDeclBody::Single(annotation), &slot.path)?;
                        Ok(DerefedAuthoredBody {
                            shape,
                            // A value annotation position binds no declared
                            // type parameters of its own.
                            type_parameters: Vec::new(),
                        })
                    }
                    LocatorSymbolSpace::Namespace => {
                        Err(LocatorBodyDerefError::NamespaceBodyUnrouted)
                    }
                }
            }
            AuthoredBodyLocator::AugmentationBody(aug) => {
                debug_assert_eq!(
                    aug.anchor.canonical_id.as_ref(),
                    self.key.canonical.as_ref(),
                    "a locator must deref through the memo of its OWN producing canonical"
                );
                let scope_kind = match &aug.scope {
                    AuthoredAugmentationScope::Global => AugmentationScopeKind::Global,
                    AuthoredAugmentationScope::Module { specifier } => {
                        AugmentationScopeKind::Module(specifier.as_ref().to_string())
                    }
                };
                match aug.anchor.space {
                    LocatorSymbolSpace::Type => {
                        // Serve through the memo's scoped lazy demand cell
                        // (one lowering per (scope, symbol) per content). A
                        // broken-lease demand surfaces the DISTINCT `LeaseMiss`
                        // no-warm signal, never a cacheable `UnknownSymbol`.
                        let lowered = match self
                            .augmentation_type_decl_outcome(&scope_kind, aug.anchor.symbol.as_ref())
                        {
                            DemandOutcome::LeaseMiss => {
                                return Err(LocatorBodyDerefError::LeaseMiss)
                            }
                            DemandOutcome::Ready(Some(lowered)) => lowered,
                            DemandOutcome::Ready(None) => {
                                return Err(LocatorBodyDerefError::UnknownSymbol)
                            }
                        };
                        let shape = match lowered.body.clone() {
                            TypeDeclBody::Single(expr) => DerefedBodyShape::Single(expr),
                            TypeDeclBody::Merged(merged) => {
                                DerefedBodyShape::Merged(merged.contributors)
                            }
                        };
                        Ok(DerefedAuthoredBody {
                            shape,
                            type_parameters: lowered.type_parameters.clone(),
                        })
                    }
                    LocatorSymbolSpace::Value | LocatorSymbolSpace::Namespace => {
                        Err(LocatorBodyDerefError::AugmentationBodySpaceUnrouted)
                    }
                }
            }
        }
    }
}

/// Navigate a producer-emitted [`TypeBodyPathStep`] path over the OWNED
/// typed body. Empty path = the whole body (preserving the merged-contributor
/// carrier); a non-empty path selects exactly the named sub-position.
/// Fail-closed: any shape/ordinal mismatch is
/// [`LocatorBodyDerefError::PathUnresolved`].
fn navigate_type_body(
    body: TypeDeclBody,
    path: &[TypeBodyPathStep],
) -> Result<DerefedBodyShape, LocatorBodyDerefError> {
    let Some((first, rest)) = path.split_first() else {
        return Ok(match body {
            TypeDeclBody::Single(expr) => DerefedBodyShape::Single(expr),
            TypeDeclBody::Merged(merged) => DerefedBodyShape::Merged(merged.contributors),
        });
    };
    let (start, remaining) = match (body, first) {
        (TypeDeclBody::Merged(merged), TypeBodyPathStep::MergedContributor { ordinal }) => {
            let expr = merged
                .contributors
                .into_iter()
                .nth(*ordinal as usize)
                .ok_or(LocatorBodyDerefError::PathUnresolved)?;
            (expr, rest)
        }
        // A merged body's sub-positions are addressed through a contributor
        // step first; any other first step is unresolvable by shape.
        (TypeDeclBody::Merged(_), _) => return Err(LocatorBodyDerefError::PathUnresolved),
        // A single body has no contributor axis; the whole path navigates
        // the body expression directly.
        (TypeDeclBody::Single(expr), _) => (expr, path),
    };
    navigate_expr(start, remaining).map(DerefedBodyShape::Single)
}

/// The current navigation position: an expression, or a selected object /
/// interface member (from which `MemberValue` — or path termination —
/// descends to the member's value type).
enum NavigatePosition {
    Expr(TypeExpr),
    Member(ObjectMember),
}

/// Navigate `path` over an owned expression. Parenthesized wrappers are
/// structurally transparent at every expression step.
fn navigate_expr(
    expr: TypeExpr,
    path: &[TypeBodyPathStep],
) -> Result<TypeExpr, LocatorBodyDerefError> {
    let mut position = NavigatePosition::Expr(expr);
    for step in path {
        position = match (position, step) {
            (NavigatePosition::Expr(expr), TypeBodyPathStep::IntersectionArm { ordinal }) => {
                match unwrap_parenthesized(expr) {
                    TypeExpr::Intersection(ref arms) => NavigatePosition::Expr(
                        arms.get(*ordinal as usize)
                            .cloned()
                            .ok_or(LocatorBodyDerefError::PathUnresolved)?,
                    ),
                    _ => return Err(LocatorBodyDerefError::PathUnresolved),
                }
            }
            (NavigatePosition::Expr(expr), TypeBodyPathStep::Member { ordinal }) => {
                match unwrap_parenthesized(expr) {
                    TypeExpr::Object(ref obj) => NavigatePosition::Member(
                        obj.properties
                            .get(*ordinal as usize)
                            .cloned()
                            .ok_or(LocatorBodyDerefError::PathUnresolved)?,
                    ),
                    _ => return Err(LocatorBodyDerefError::PathUnresolved),
                }
            }
            (NavigatePosition::Member(member), TypeBodyPathStep::MemberValue) => {
                NavigatePosition::Expr(member_value_expr(member)?)
            }
            _ => return Err(LocatorBodyDerefError::PathUnresolved),
        };
    }
    match position {
        NavigatePosition::Expr(expr) => Ok(expr),
        // A path terminating on a selected member derefs to that member's
        // value type (the one typed-IR expression at a member position).
        NavigatePosition::Member(member) => member_value_expr(member),
    }
}

/// Unwrap structurally-transparent `Parenthesized` layers.
fn unwrap_parenthesized(mut expr: TypeExpr) -> TypeExpr {
    while let TypeExpr::Parenthesized(ref inner) = expr {
        let unwrapped = inner.as_ref().clone();
        expr = unwrapped;
    }
    expr
}

/// The value-type expression of a selected object member. An index
/// signature has no single member-value expression — fail closed.
fn member_value_expr(member: ObjectMember) -> Result<TypeExpr, LocatorBodyDerefError> {
    match member {
        ObjectMember::Property(prop) => Ok(prop.ty),
        ObjectMember::Method(method) => {
            Ok(TypeExpr::Function(std::sync::Arc::new(method.function)))
        }
        ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
            Ok(TypeExpr::Function(std::sync::Arc::new(func)))
        }
        ObjectMember::IndexSignature(_) => Err(LocatorBodyDerefError::PathUnresolved),
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
