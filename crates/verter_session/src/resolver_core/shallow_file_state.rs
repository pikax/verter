//! Canonical shallow file state for imported dependencies.
//!
//! `ShallowFileState` is the authoritative representation of an imported
//! file's shallow symbol/export surface.  It is populated exactly once per
//! `(canonical_id, whole_hash)` through the shared host ensure-path and reused
//! by component-meta, LSP, MCP, and other host-backed consumers.
//!
//! **Design invariants**
//! - No full type evaluation or expansion during construction.
//! - Export routing and symbol lookup are O(1) after construction.
//! - Same-file local closure is computed lazily per symbol on first access.
//! - Cross-file references are returned as `ExternalSymbolRef` for the
//!   frontier engine to handle â€” this module never crosses import boundaries.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::route_demand::RouteDemand;
use crate::decl_body_memo::{LoweredTypeDecl, LoweredValueDecl};
use verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource;
use verter_semantic::analysis::decl_headers::{TypeDeclHeader, ValueDeclHeader};
use verter_semantic::analysis::type_eval::{TypeDeclKind, ValueDeclKind};
use verter_semantic::analysis::Hash16;
use verter_type_expr::TypeExpr;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Authoritative shallow state for one imported file.
///
/// Keyed by `(canonical_id, whole_hash)`.  Invalidated when the file’s
/// whole-hash changes.
///
/// All cross-file edges carry canonical target IDs resolved at construction
/// time.  The frontier and other consumers never need to re-resolve raw
/// specifiers during the hot path.
#[derive(Debug, Clone)]
pub struct ShallowFileState {
    /// Content hash of the source that produced this state.
    pub whole_hash: Hash16,

    /// Named exports: exported name → routing target.
    pub exports: FxHashMap<String, ExportTarget>,

    /// `export * from` sources with canonical targets, in declaration order.
    pub wildcard_reexports: Vec<WildcardReexport>,

    /// Import-local names (names that come from `import` declarations).
    /// Used to classify dependencies as local vs external during closure.
    pub import_locals: FxHashSet<String>,

    /// Import specifier targets: local import name → canonical import target.
    pub import_targets: FxHashMap<String, ImportTarget>,

    /// The HEADER-ONLY analyzed source: import/export/reexport tables plus
    /// the local type-symbol NAME inventory (kind + span, no dependency
    /// names). Body-derived data lives behind the lazy declaration-body
    /// memo.
    pub analysis: Arc<AnalyzedExternalTypeSource>,

    /// The lazy declaration-body memo this state materialises symbols
    /// from — the SOLE body authority. Shared (`Arc`) across route-only
    /// edge refreshes of the same content generation.
    decl_bodies: Arc<crate::decl_body_memo::DeclBodyMemo>,

    /// Per-name DEPENDENCY-EDGE cache: the local/external dependency
    /// classification for one file-scope TYPE symbol (canonicals baked
    /// from THIS state's import targets — rebuilt with the state at edge
    /// refresh so cross-file edges re-resolve). Stores ONLY dependency
    /// edges, never a lowered body product — body data is read through
    /// the lazy memo accessors ([`Self::type_decl`]). Populated only for
    /// names the header inventory knows; a header miss never inserts.
    type_deps_cache: dashmap::DashMap<String, Option<Arc<ClassifiedTypeDeps>>>,

    /// EAGER synthesised value-symbol HEADERS (the `.vue` implicit
    /// `default` public-instance shape) — header-only records carrying
    /// the `is_synthesised_vue_default` provenance flag. The matching
    /// eager body lives in [`Self::synthesised_value_bodies`].
    synthesised_value_symbols: FxHashMap<String, Arc<ShallowValueSymbol>>,

    /// EAGER synthesised value BODIES (the macro-producer-boundary
    /// `LoweredValueDecl` for the `.vue` implicit `default`). Kept in a
    /// dedicated body map rather than hidden inside the header symbol —
    /// `value_decl(name)` routes through it before the lazy memo.
    synthesised_value_bodies: FxHashMap<String, Arc<LoweredValueDecl>>,

    /// The local value name a CommonJS `export = X` assigns the whole module
    /// to, when present (part of the shallow EXPORT inventory). `typeof
    /// import("./m")` against such a module resolves to `typeof X`, not an
    /// object of named exports. `None` for an ordinary ESM module.
    export_assignment: Option<String>,
}

/// A wildcard `export * from ‘...’` reexport with its resolved canonical target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WildcardReexport {
    /// The raw source specifier (e.g., `./types`).
    pub source_specifier: String,
    /// The resolved canonical file ID of the target.
    pub canonical_id: String,
}

/// An import target with both the raw specifier and resolved canonical ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportTarget {
    /// The raw source specifier (e.g., `./types`).
    pub source_specifier: String,
    /// The original exported name in the source module.
    pub imported_name: String,
    /// The resolved canonical file ID of the target.
    pub canonical_id: String,
}

/// Narrow type-resolution view over [`ShallowFileState`].
///
/// This keeps the frontier and other type-only consumers focused on the
/// export/type-symbol surface even though the canonical file cache also owns
/// value-side declarations.
#[derive(Clone, Copy)]
pub struct ShallowTypeView<'a> {
    state: &'a ShallowFileState,
}

/// Where an exported name resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportTarget {
    /// Locally declared and exported.
    Local { symbol_name: String },
    /// Explicitly re-exported from another module.
    /// `export { Foo } from './bar'` or `export { Foo as Bar } from './bar'`
    Reexport {
        source_specifier: String,
        original_name: String,
        /// The resolved canonical file ID of the target.
        canonical_id: String,
        /// Whether this is a type-only reexport (`export type { ... }`).
        /// Used by the export graph to choose type vs. value resolution.
        is_type: bool,
    },
}

/// Slim HEADER metadata for one locally-declared type symbol.
///
/// This is a header-only view over the shallow declaration index — it
/// OWNS no body product. Declaration BODIES live exclusively in the
/// memo-owned [`LoweredTypeDecl`] (read through [`ShallowFileState::type_decl`]);
/// dependency edges live in [`ClassifiedTypeDeps`] (read through
/// [`ShallowFileState::type_deps`]).
#[derive(Debug, Clone)]
pub struct ShallowTypeSymbol {
    /// Declaration kind (header fact).
    pub kind: TypeDeclKind,
    /// Generic type-parameter NAMES, unioned across contributors in
    /// first-seen order. (The full `TypeParam` carriers — constraints /
    /// defaults — are body data, read through `type_decl`.)
    pub type_param_names: Vec<String>,
    /// Direct syntactic member NAMES (own members only, heritage
    /// excluded) — a shallow shape fact.
    pub member_names: Vec<String>,
    /// Number of same-name contributing declarations that merged into
    /// this symbol.
    pub contributor_count: usize,
}

impl ShallowTypeSymbol {
    /// Build the slim header view from a shallow type-declaration header.
    fn from_header(header: &TypeDeclHeader) -> Self {
        Self {
            kind: header.kind,
            type_param_names: header.type_params.iter().map(|p| p.name.clone()).collect(),
            member_names: header
                .member_headers
                .iter()
                .map(|m| m.name.clone())
                .collect(),
            contributor_count: header.contributors.len(),
        }
    }
}

/// Per-symbol dependency-edge classification — the local vs external
/// split over one type declaration's reference graph, baked against the
/// owning state's import targets. Dependency EDGES only; no body product.
#[derive(Debug, Clone, Default)]
pub struct ClassifiedTypeDeps {
    /// Names of same-file symbols this type directly depends on.
    /// Used for iterative local closure.
    pub local_deps: Vec<String>,
    /// Names of import-local symbols this type directly depends on.
    /// These become `ExternalSymbolRef` during frontier traversal.
    pub external_deps: Vec<ExternalSymbolRef>,
}

/// Slim HEADER metadata for one locally-declared value symbol.
///
/// A header-only view over the shallow declaration index (kind +
/// object-literal member names) plus the `.vue`-default provenance flag.
/// It OWNS no body product — declaration bodies live exclusively in the
/// memo-owned (or eager synthesised) [`LoweredValueDecl`], read through
/// [`ShallowFileState::value_decl`].
#[derive(Debug, Clone)]
pub struct ShallowValueSymbol {
    /// Declaration kind (header fact).
    pub kind: ValueDeclKind,
    /// Direct member NAMES of an object-literal initializer
    /// (`const x = { a, b }`) — a shallow shape fact; empty for
    /// non-object-literal values.
    pub object_member_headers: Vec<String>,
    /// Structural PROVENANCE fact: `true` only for the synthesized `default`
    /// VALUE symbol that [`super::vue_default_synth::synthesise_vue_default_value_symbol`]
    /// fabricates for a `.vue` SFC's implicit public instance (the construct
    /// signature returning `{ $props, $emit, $slots }`). `false` for EVERY
    /// userland-declared value symbol — including a userland `export default`
    /// in a `.vue`'s `<script>` block.
    ///
    /// This is the direct consumer proof that a resolved `default` IS the
    /// synthesized public instance. Synthesized-default consumers
    /// (`build_vue_default_instance`, the `.vue default` branch in
    /// `build_instantiate`, `resolve_vue_public_type`, the synthesized-default
    /// convergence in `build_typeof`) gate on this flag rather than on the
    /// file-classifier `is_synthesis_candidate`, so a `.vue` with a USERLAND
    /// `export default` (synthesis skipped, userland default present) is never
    /// mistreated as the synthesized public instance.
    pub is_synthesised_vue_default: bool,
}

impl ShallowValueSymbol {
    /// Build the slim header view from a shallow value-declaration header.
    /// `is_synthesised_vue_default` is `false` for every header-index
    /// (userland) value symbol.
    fn from_header(header: &ValueDeclHeader) -> Self {
        Self {
            kind: header.kind,
            object_member_headers: header
                .object_member_headers
                .iter()
                .map(|m| m.name.clone())
                .collect(),
            is_synthesised_vue_default: false,
        }
    }

    /// Build the slim header view for the EAGER synthesised `.vue`-default
    /// from its macro-producer lowered body. The body itself is stored
    /// separately ([`ShallowFileState::synthesised_value_bodies`]); this
    /// is the header probe carrying the provenance flag.
    pub(crate) fn synthesised_from_lowered(lowered: &LoweredValueDecl) -> Self {
        Self {
            kind: lowered.kind,
            object_member_headers: Vec::new(),
            is_synthesised_vue_default: true,
        }
    }
}

/// A reference to an imported symbol that needs cross-file resolution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExternalSymbolRef {
    /// The local import name in this file.
    pub local_name: String,
    /// The import specifier (e.g., `./types`, `reka-ui`).
    pub source_specifier: String,
    /// The original exported name in the source module.
    pub imported_name: String,
    /// The resolved canonical file ID of the target; `None` when the
    /// specifier did not resolve (construction without a host resolver,
    /// or a genuinely unresolvable specifier).
    pub canonical_id: Option<Arc<str>>,
    /// The remaining route demand on the imported symbol.
    pub route: RouteDemand,
}

/// Lift an [`ImportTarget`]'s resolved canonical onto the typed
/// resolved/unresolved carrier (the import table keeps the empty-string
/// miss sentinel internally; external refs carry the explicit `Option`).
pub(crate) fn external_canonical(target: &ImportTarget) -> Option<Arc<str>> {
    (!target.canonical_id.is_empty()).then(|| Arc::<str>::from(target.canonical_id.as_str()))
}

/// Keyed-map external-ref accumulator: merges same-`(specifier,
/// imported_name)` refs by route-union in O(1) per add (insertion order
/// preserved for deterministic output).
#[derive(Debug, Default)]
pub(crate) struct ExternalRefAccumulator {
    index: FxHashMap<(String, String), usize>,
    refs: Vec<ExternalSymbolRef>,
}

impl ExternalRefAccumulator {
    pub(crate) fn add(&mut self, ext_ref: ExternalSymbolRef) {
        match self.index.entry((
            ext_ref.source_specifier.clone(),
            ext_ref.imported_name.clone(),
        )) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                let existing = &mut self.refs[*entry.get()];
                existing.route =
                    crate::resolver_core::merge_route_demands(&existing.route, &ext_ref.route);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(self.refs.len());
                self.refs.push(ext_ref);
            }
        }
    }

    pub(crate) fn into_vec(self) -> Vec<ExternalSymbolRef> {
        self.refs
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WholeRouteContext {
    Root,
    CallableParam,
    LeafProperty,
}

// ---------------------------------------------------------------------------
// Budget and failure contract
// ---------------------------------------------------------------------------

/// Deterministic budgets for the three resolution domains.
///
/// These are safety rails against runaway recursion, not the normal control
/// flow.  High ceilings ensure valid deep library graphs still resolve.
/// When a budget trips, the system surfaces a structured failure state
/// rather than silently normalizing it.
#[derive(Debug, Clone)]
pub struct ResolutionBudgets {
    /// Max local symbols visited during same-file closure.
    pub local_closure_steps: usize,
    /// Max `(canonical_id, exported_name)` pairs visited by the frontier.
    pub frontier_symbol_visits: usize,
    /// Max symbolic expansion steps in the builder.
    pub builder_expansion_steps: usize,
}

impl Default for ResolutionBudgets {
    fn default() -> Self {
        Self {
            // Intentionally high: most files have <50 local types.
            // 500 covers even very large single-file type libraries.
            local_closure_steps: 500,
            // Intentionally high: real dependency graphs rarely exceed
            // 200 unique external symbols per query.  The shared external
            // type step ceiling keeps frontier and external resolution in
            // sync.
            frontier_symbol_visits: crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS,
            // Intentionally high: expansion is the costliest phase but
            // the frontier already limits how much work reaches here.
            builder_expansion_steps: 5000,
        }
    }
}

/// Counters tracking work done across the three resolution domains.
///
/// These are per-request counters, not global accumulators.
#[derive(Debug, Clone, Default)]
pub struct ResolutionCounters {
    pub local_closure_steps: u64,
    pub frontier_symbol_visits: u64,
    pub builder_expansion_steps: u64,
}

/// Structured failure when a budget is exceeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetExceededFailure {
    /// Which budget domain was exceeded.
    pub domain: BudgetDomain,
    /// The budget limit that was hit.
    pub limit: usize,
    /// Actual count at the time of failure.
    pub actual: u64,
    /// Context about what was being resolved when the budget tripped.
    pub context: String,
}

/// Which resolution domain hit its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDomain {
    LocalClosure,
    Frontier,
    BuilderExpansion,
    ProjectionOperation,
    SolverResolveSteps,
    SolverArenaNodes,
    SolverInstantiationDepth,
}

impl std::fmt::Display for BudgetExceededFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BUDGET_EXCEEDED({:?}): limit={}, actual={}, context={}",
            self.domain, self.limit, self.actual, self.context
        )
    }
}

// ---------------------------------------------------------------------------
// Local symbol closure
// ---------------------------------------------------------------------------

/// Result of resolving same-file dependencies for one symbol.
#[derive(Debug, Clone)]
pub enum LocalClosureStatus {
    /// All same-file deps resolved; no external deps.
    Resolved,
    /// Same-file closure succeeded but external deps remain.
    ResolvedWithExternalDeps,
    /// A referenced local symbol does not exist in the file.
    MissingLocalSymbol { name: String },
    /// Budget for local closure steps was exceeded.
    BudgetExceeded,
}
// NOTE: No `LocalCycle` variant. Same-file cycles are handled by the visited
// set â€” a revisited node is silently skipped, which is correct graph traversal.
// The closure result reflects the *reachable* set, not a cycle error.

/// Outcome of local closure for one requested symbol.
#[derive(Debug, Clone)]
pub struct LocalClosureResult {
    pub status: LocalClosureStatus,
    /// All local symbol names that participate in this closure
    /// (the transitive same-file dependency set).
    pub local_symbols_used: Vec<String>,
    /// External refs discovered during closure that need frontier resolution.
    pub unresolved_external: Vec<ExternalSymbolRef>,
    /// Number of local symbols visited during closure.
    pub steps: u64,
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

/// Trait for resolving import specifiers to canonical file IDs during
/// shallow state construction.
pub trait ShallowImportResolver {
    /// Resolve an import specifier from the file being analyzed to its
    /// canonical file ID.  Returns `None` if the specifier cannot be resolved.
    fn resolve_canonical(&self, specifier: &str) -> Option<String>;

    /// Classify a direct reexport as type-only.  Returns `true` if the reexport
    /// was declared with `export type { ... } from '...'`.
    fn is_type_reexport(&self, _exported_name: &str, _specifier: &str) -> bool {
        false
    }
}

/// A no-op resolver that cannot resolve any specifiers.
/// Used for test construction where canonical IDs are not needed.
struct NullResolver;

impl ShallowImportResolver for NullResolver {
    fn resolve_canonical(&self, _specifier: &str) -> Option<String> {
        None
    }
}

impl ShallowFileState {
    /// Test-only convenience: build from an existing
    /// `AnalyzedExternalTypeSource` with a null resolver — canonical IDs
    /// on cross-file edges stay empty. The supplied `eval_env` seeds the
    /// declaration-body memo (every entry pre-filled through the same
    /// per-symbol fold the lazy path performs). The production
    /// construction path is [`Self::from_analysis_with_resolver`] (the
    /// materialise closure supplies the resolver and the lazy memo).
    /// Gated `#[cfg(any(test, debug_assertions))]` — integration tests
    /// in `tests/` compile without `cfg(test)`; release production
    /// builds compile this edge-less constructor out.
    #[cfg(any(test, debug_assertions))]
    pub fn from_analysis(
        whole_hash: Hash16,
        analysis: Arc<AnalyzedExternalTypeSource>,
        eval_env: Option<&verter_semantic::analysis::type_eval::EvalEnv>,
    ) -> Self {
        let empty_env = verter_semantic::analysis::type_eval::EvalEnv::default();
        let env = eval_env.unwrap_or(&empty_env);
        let header_index =
            Arc::new(verter_semantic::analysis::decl_headers::DeclHeaderIndex::from_eval_env(env));
        let memo = crate::decl_body_memo::DeclBodyMemo::seeded_from_env(
            crate::decl_lowering::SnapshotKey {
                canonical: Arc::from(""),
                whole_hash,
                parse_env_hash: [0u8; 16],
            },
            env,
            analysis.as_ref(),
            header_index,
        );
        Self::from_analysis_inner(whole_hash, analysis, Arc::new(memo), &NullResolver)
    }

    /// Test-only counterpart of [`Self::from_analysis_with_resolver`]
    /// taking an already-built `EvalEnv` to SEED the declaration-body
    /// memo (every entry pre-filled through the same per-symbol fold the
    /// lazy path performs) while resolving cross-file edges through the
    /// supplied resolver. Same gate as [`Self::from_analysis`].
    #[cfg(any(test, debug_assertions))]
    pub fn from_analysis_with_resolver_seeded(
        whole_hash: Hash16,
        analysis: Arc<AnalyzedExternalTypeSource>,
        eval_env: Option<&verter_semantic::analysis::type_eval::EvalEnv>,
        resolver: &dyn ShallowImportResolver,
    ) -> Self {
        let empty_env = verter_semantic::analysis::type_eval::EvalEnv::default();
        let env = eval_env.unwrap_or(&empty_env);
        let header_index =
            Arc::new(verter_semantic::analysis::decl_headers::DeclHeaderIndex::from_eval_env(env));
        let memo = crate::decl_body_memo::DeclBodyMemo::seeded_from_env(
            crate::decl_lowering::SnapshotKey {
                canonical: Arc::from(""),
                whole_hash,
                parse_env_hash: [0u8; 16],
            },
            env,
            analysis.as_ref(),
            header_index,
        );
        Self::from_analysis_inner(whole_hash, analysis, Arc::new(memo), resolver)
    }

    /// Test-only constructor with caller-supplied ROUTING tables (exports,
    /// wildcard reexports, import tables) and an EMPTY seeded symbol
    /// inventory — for fixtures that exercise route surfaces without any
    /// declared symbols. Same gate as [`Self::from_analysis`].
    #[cfg(any(test, debug_assertions))]
    #[allow(clippy::too_many_arguments)]
    pub fn new_for_test_with_routing(
        whole_hash: Hash16,
        exports: FxHashMap<String, ExportTarget>,
        wildcard_reexports: Vec<WildcardReexport>,
        import_locals: FxHashSet<String>,
        import_targets: FxHashMap<String, ImportTarget>,
        analysis: Arc<AnalyzedExternalTypeSource>,
    ) -> Self {
        let mut state = Self::from_analysis(whole_hash, analysis, None);
        state.exports = exports;
        state.wildcard_reexports = wildcard_reexports;
        state.import_locals = import_locals;
        state.import_targets = import_targets;
        state
    }

    /// Build from the header-only analysis + the lazy declaration-body
    /// memo, with a resolver that canonicalizes all cross-file edges.
    ///
    /// This is the production construction path: HEADER work only —
    /// export/import routing tables, no body lowering. Symbol bodies
    /// materialise on first demand through the memo.
    pub fn from_analysis_with_resolver(
        whole_hash: Hash16,
        analysis: Arc<AnalyzedExternalTypeSource>,
        decl_bodies: Arc<crate::decl_body_memo::DeclBodyMemo>,
        resolver: &dyn ShallowImportResolver,
    ) -> Self {
        Self::from_analysis_inner(whole_hash, analysis, decl_bodies, resolver)
    }

    fn from_analysis_inner(
        whole_hash: Hash16,
        analysis: Arc<AnalyzedExternalTypeSource>,
        decl_bodies: Arc<crate::decl_body_memo::DeclBodyMemo>,
        resolver: &dyn ShallowImportResolver,
    ) -> Self {
        let mut exports = FxHashMap::default();
        let mut wildcard_reexports = Vec::new();
        let mut import_locals = FxHashSet::default();
        let mut import_targets: FxHashMap<String, ImportTarget> = FxHashMap::default();

        // Populate exports from the extracted bindings
        // Direct reexports
        for (exported_name, source, original) in analysis.direct_reexport_entries() {
            let canonical_id = resolver.resolve_canonical(source).unwrap_or_default();
            let is_type = resolver.is_type_reexport(exported_name, source);
            exports.insert(
                exported_name.to_string(),
                ExportTarget::Reexport {
                    source_specifier: source.to_string(),
                    original_name: original.to_string(),
                    canonical_id,
                    is_type,
                },
            );
        }

        // Locally exported type names
        for name in analysis.exported_local_type_names() {
            exports
                .entry(name.to_string())
                .or_insert_with(|| ExportTarget::Local {
                    symbol_name: name.to_string(),
                });
        }

        for name in analysis.exported_local_symbol_names() {
            let symbol_name = analysis.local_export_symbol_target(name).unwrap_or(name);
            exports
                .entry(name.to_string())
                .or_insert_with(|| ExportTarget::Local {
                    symbol_name: symbol_name.to_string(),
                });
        }

        if analysis.local_symbol_span("default").is_some() {
            exports
                .entry("default".to_string())
                .or_insert_with(|| ExportTarget::Local {
                    symbol_name: "default".to_string(),
                });
        }

        // Wildcard reexport sources (in declaration order) with canonical targets
        for source in analysis.wildcard_reexport_sources() {
            let canonical_id = resolver.resolve_canonical(source).unwrap_or_default();
            wildcard_reexports.push(WildcardReexport {
                source_specifier: source.clone(),
                canonical_id,
            });
        }

        // Import locals and targets
        for binding in &analysis.extracted.bindings {
            import_locals.insert(binding.local_name.clone());
            let canonical_id = resolver
                .resolve_canonical(&binding.source)
                .unwrap_or_default();
            import_targets.insert(
                binding.local_name.clone(),
                ImportTarget {
                    source_specifier: binding.source.clone(),
                    imported_name: binding.imported_name.clone(),
                    canonical_id,
                },
            );
        }

        Self {
            whole_hash,
            exports,
            wildcard_reexports,
            import_locals,
            export_assignment: analysis.export_assignment_target().map(str::to_string),
            import_targets,
            analysis,
            decl_bodies,
            type_deps_cache: dashmap::DashMap::default(),
            synthesised_value_symbols: FxHashMap::default(),
            synthesised_value_bodies: FxHashMap::default(),
        }
    }

    // -----------------------------------------------------------------------
    // Emptiness check
    // -----------------------------------------------------------------------

    /// Returns `true` when the shallow state carries no meaningful content
    /// (no type symbols, no value symbols, no exports, no wildcard reexports,
    /// and no import targets). A non-empty state is worth caching and
    /// returning to callers even when the symbol inventory alone is empty
    /// (e.g. a barrel file with only reexports, or an SFC with only value
    /// bindings). Header-level check — no body lowering.
    pub fn is_empty(&self) -> bool {
        !self.has_any_type_symbol_names()
            && !self.has_any_value_symbol_names()
            && self.exports.is_empty()
            && self.wildcard_reexports.is_empty()
            && self.import_targets.is_empty()
    }

    /// Returns `true` when this state has content that the frontier can
    /// actually resolve against: local type/value symbols, direct exports,
    /// or wildcard reexport entries. Files that only contain imports but no
    /// exports or symbols should not be handed to the frontier — they have
    /// nothing to contribute to export resolution. Header-level check.
    pub fn has_resolvable_surface(&self) -> bool {
        self.has_any_type_symbol_names()
            || self.has_any_value_symbol_names()
            || !self.exports.is_empty()
            || !self.wildcard_reexports.is_empty()
    }

    fn has_any_type_symbol_names(&self) -> bool {
        !self.decl_bodies.header_index().type_headers.is_empty()
    }

    fn has_any_value_symbol_names(&self) -> bool {
        !self.decl_bodies.header_index().value_headers.is_empty()
            || !self.synthesised_value_symbols.is_empty()
    }

    /// The lazy declaration-body memo this state reads from (the body
    /// authority for this content generation).
    pub fn decl_bodies(&self) -> &Arc<crate::decl_body_memo::DeclBodyMemo> {
        &self.decl_bodies
    }

    /// Every file-scope TYPE symbol name in the shallow inventory
    /// (header-level — no body lowering).
    pub fn type_symbol_names(&self) -> impl Iterator<Item = &str> {
        self.decl_bodies
            .header_index()
            .type_headers
            .keys()
            .map(String::as_str)
    }

    /// Every file-scope VALUE symbol name in the shallow inventory,
    /// including eager synthesised symbols (header-level).
    pub fn value_symbol_names(&self) -> impl Iterator<Item = &str> {
        let headers = &self.decl_bodies.header_index().value_headers;
        headers.keys().map(String::as_str).chain(
            self.synthesised_value_symbols
                .keys()
                .map(String::as_str)
                .filter(move |name| !headers.contains_key(*name)),
        )
    }

    /// Every eager synthesised VALUE body (the `.vue` implicit
    /// `default`'s macro-producer [`LoweredValueDecl`]) as
    /// `(name, body)` pairs. Eager-only: it iterates the synthesised
    /// inventory directly and never materializes an ordinary value
    /// declaration body through the lazy memo.
    pub fn synthesised_value_bodies(&self) -> impl Iterator<Item = (&str, &Arc<LoweredValueDecl>)> {
        self.synthesised_value_bodies
            .iter()
            .map(|(name, body)| (name.as_str(), body))
    }

    /// Header-level kind of a file-scope TYPE symbol (no body lowering).
    pub fn type_symbol_kind(
        &self,
        name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::TypeDeclKind> {
        self.decl_bodies
            .header_index()
            .type_header(name)
            .map(|header| header.kind)
    }

    /// Header-level kind of a file-scope VALUE symbol (no body lowering;
    /// synthesised symbols answer from their eager record).
    pub fn value_symbol_kind(
        &self,
        name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::ValueDeclKind> {
        if let Some(synthesised) = self.synthesised_value_symbols.get(name) {
            return Some(synthesised.kind);
        }
        self.decl_bodies
            .header_index()
            .value_header(name)
            .map(|header| header.kind)
    }

    /// Header-level direct syntactic member headers of a file-scope TYPE
    /// symbol (no body lowering).
    pub fn type_member_headers(
        &self,
        name: &str,
    ) -> Option<&[verter_semantic::analysis::decl_headers::MemberHeader]> {
        self.decl_bodies
            .header_index()
            .type_header(name)
            .map(|header| header.member_headers.as_slice())
    }

    /// Every `enum` declaration name in the shallow inventory
    /// (header-level). An enum symbol is registered DUAL-SPACE — it carries
    /// both a type header (its projected-type union) and a value header (its
    /// `typeof` object), so it IS yielded by both
    /// [`Self::type_symbol_names`] and [`Self::value_symbol_names`]. This
    /// dedicated enum table is the separate authority for the member
    /// (variant) NAMES — the member-presence facts rail — which the
    /// type/value headers do not carry.
    pub fn enum_symbol_names(&self) -> impl Iterator<Item = &str> {
        self.decl_bodies
            .header_index()
            .enum_headers
            .keys()
            .map(String::as_str)
    }

    /// Header-level ordered member (variant) names of an `enum`
    /// declaration, in source order. `None` when `name` is not an enum.
    pub fn enum_member_names(&self, name: &str) -> Option<&[String]> {
        self.decl_bodies
            .header_index()
            .enum_headers
            .get(name)
            .map(|header| header.member_names.as_slice())
    }

    /// Header-level type-parameter names of a file-scope TYPE symbol.
    pub fn type_param_names(&self, name: &str) -> Option<Vec<&str>> {
        self.decl_bodies
            .header_index()
            .type_header(name)
            .map(|header| header.type_params.iter().map(|p| p.name.as_str()).collect())
    }

    /// Header-level type-parameter headers of a file-scope TYPE symbol
    /// (each carries the param name plus the source locators of its
    /// constraint / default clauses). No body lowering.
    pub fn type_param_headers(
        &self,
        name: &str,
    ) -> Option<&[verter_semantic::analysis::decl_headers::TypeParamHeader]> {
        self.decl_bodies
            .header_index()
            .type_header(name)
            .map(|header| header.type_params.as_slice())
    }

    /// Number of source-order contributing top-level statements for a
    /// file-scope TYPE symbol (a same-name decl split / merge changes
    /// this). No body lowering.
    pub fn type_contributor_count(&self, name: &str) -> Option<usize> {
        self.decl_bodies
            .header_index()
            .type_header(name)
            .map(|header| header.contributors.len())
    }

    /// Header-level direct syntactic member headers of an object-literal
    /// initializer (or class-static surface) bound to a file-scope VALUE
    /// symbol. No body lowering.
    pub fn value_object_member_headers(
        &self,
        name: &str,
    ) -> Option<&[verter_semantic::analysis::decl_headers::MemberHeader]> {
        self.decl_bodies
            .header_index()
            .value_header(name)
            .map(|header| header.object_member_headers.as_slice())
    }

    /// Number of source-order contributing top-level statements for a
    /// file-scope VALUE symbol. No body lowering.
    pub fn value_contributor_count(&self, name: &str) -> Option<usize> {
        self.decl_bodies
            .header_index()
            .value_header(name)
            .map(|header| header.contributors.len())
    }

    /// Whether `name` is a file-scope TYPE symbol (header-level).
    pub fn has_type_symbol(&self, name: &str) -> bool {
        self.decl_bodies.header_index().type_header(name).is_some()
    }

    /// Whether `name` is a file-scope VALUE symbol (header-level,
    /// including synthesised symbols).
    pub fn has_value_symbol(&self, name: &str) -> bool {
        self.decl_bodies.header_index().value_header(name).is_some()
            || self.synthesised_value_symbols.contains_key(name)
    }

    /// Every `(scope, name)` key in the augmentation-scope TYPE inventory
    /// (header-level).
    pub fn augmentation_type_keys(
        &self,
    ) -> impl Iterator<
        Item = (
            &verter_semantic::analysis::type_eval::AugmentationScopeKind,
            &str,
        ),
    > {
        self.decl_bodies
            .header_index()
            .augmentation_type_headers
            .iter()
            .flat_map(|(scope, names)| names.keys().map(move |name| (scope, name.as_str())))
    }

    /// Every `(scope, name)` key in the augmentation-scope VALUE inventory
    /// (header-level).
    pub fn augmentation_value_keys(
        &self,
    ) -> impl Iterator<
        Item = (
            &verter_semantic::analysis::type_eval::AugmentationScopeKind,
            &str,
        ),
    > {
        self.decl_bodies
            .header_index()
            .augmentation_value_headers
            .iter()
            .flat_map(|(scope, names)| names.keys().map(move |name| (scope, name.as_str())))
    }

    // -----------------------------------------------------------------------
    // Export routing
    // -----------------------------------------------------------------------

    /// Look up a named export. Returns `None` if the name is not directly
    /// exported (may still be available through wildcard reexports).
    pub fn export_target(&self, name: &str) -> Option<&ExportTarget> {
        self.exports.get(name)
    }

    /// The local value name a CommonJS `export = X` assigns the whole module
    /// to (`Some("X")`), or `None` for an ordinary ESM module. Part of the
    /// shallow EXPORT inventory; consumed by `typeof import("./m")` resolution.
    pub fn export_assignment_target(&self) -> Option<&str> {
        self.export_assignment.as_deref()
    }

    /// Get the narrow type-resolution view over this file state.
    pub fn type_view(&self) -> ShallowTypeView<'_> {
        ShallowTypeView { state: self }
    }

    /// Whether this file has any wildcard re-exports.
    pub fn has_wildcard_reexports(&self) -> bool {
        !self.wildcard_reexports.is_empty()
    }

    /// Whether the SHALLOW INVENTORY carries any cross-file edge — a
    /// resolved import target, a wildcard reexport, a named reexport, or a
    /// bindingless (side-effect / empty-list) import. Every such edge
    /// bakes or implies a target `canonical_id` that depends on the
    /// DEPENDENCY file set (not this file's own content). Bindingless
    /// imports carry no local binding (so they never enter
    /// `import_targets`) yet still resolve into
    /// `IndexedReady.import_routes`; the retained analysis inventory is
    /// the single authoritative source for them.
    ///
    /// This is a COMPONENT predicate, not an edge-currency authority: an
    /// artifact can carry cross-file edges exclusively in its
    /// `import_routes` table (the SFC external `src=` class, caller-pushed
    /// route snapshots) that are invisible here. The complete authority is
    /// `IndexedReady::has_cross_file_edges` (`!import_routes.is_empty() ||`
    /// this component); the shared edge-currency oracle
    /// (`route_surface_is_edge_current`) and the reuse gates consult ONLY
    /// the complete authority. The legitimate component uses are the
    /// authority's own composition and gates over data derived purely from
    /// the shallow inventory (e.g. whether a bare shallow route-surface
    /// hash is currency-independent).
    pub fn has_shallow_cross_file_edges(&self) -> bool {
        !self.import_targets.is_empty()
            || self.has_wildcard_reexports()
            || !self
                .analysis
                .extracted
                .bindingless_import_sources
                .is_empty()
            || self
                .exports
                .values()
                .any(|target| matches!(target, ExportTarget::Reexport { .. }))
    }

    /// Look up the slim HEADER view of a local TYPE symbol by name —
    /// header-only (no body lowering). A header miss returns `None`.
    pub fn symbol(&self, name: &str) -> Option<Arc<ShallowTypeSymbol>> {
        self.decl_bodies
            .header_index()
            .type_header(name)
            .map(|header| Arc::new(ShallowTypeSymbol::from_header(header)))
    }

    /// Look up the slim HEADER view of a local VALUE symbol by name —
    /// synthesised symbols first (eager macro-producer header records),
    /// then the header index. Header-only (no body lowering).
    pub fn value_symbol(&self, name: &str) -> Option<Arc<ShallowValueSymbol>> {
        if let Some(synthesised) = self.synthesised_value_symbols.get(name) {
            return Some(Arc::clone(synthesised));
        }
        self.decl_bodies
            .header_index()
            .value_header(name)
            .map(|header| Arc::new(ShallowValueSymbol::from_header(header)))
    }

    /// Demand the lowered BODY of a local TYPE symbol — lazily lowered
    /// through the declaration-body memo on first touch. A header miss
    /// returns `None`. This is the sole body authority for file-scope
    /// type symbols; the slim [`ShallowTypeSymbol`] carries no body.
    pub fn type_decl(&self, name: &str) -> Option<Arc<LoweredTypeDecl>> {
        self.decl_bodies.type_decl(name)
    }

    /// Demand the lowered BODY of a local VALUE symbol — eager
    /// synthesised `.vue`-default bodies first, then the lazy memo. A
    /// miss returns `None`.
    pub fn value_decl(&self, name: &str) -> Option<Arc<LoweredValueDecl>> {
        if let Some(body) = self.synthesised_value_bodies.get(name) {
            return Some(Arc::clone(body));
        }
        self.decl_bodies.value_decl(name)
    }

    /// Demand the dependency-edge classification of a local TYPE symbol —
    /// the local/external split over its reference graph, baked against
    /// THIS state's import targets and cached per name. Returns `Some`
    /// for any header type symbol (possibly with empty edge lists); a
    /// header miss returns `None` without lowering or caching anything.
    pub fn type_deps(&self, name: &str) -> Option<Arc<ClassifiedTypeDeps>> {
        self.decl_bodies.header_index().type_header(name)?;
        if let Some(hit) = self.type_deps_cache.get(name) {
            return hit.clone();
        }
        let computed = self.classify_type_deps(name);
        self.type_deps_cache
            .insert(name.to_string(), computed.clone());
        computed
    }

    /// Demand the lowered BODY of an ambient-augmentation-scoped TYPE
    /// symbol (a declaration nested in a `declare module "X"` /
    /// `declare global` block) by scope + name — lazily lowered through
    /// the memo; borrowed-key lookup (no tuple allocation). A header
    /// miss returns `None`.
    pub fn augmentation_type_decl(
        &self,
        scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
        name: &str,
    ) -> Option<Arc<LoweredTypeDecl>> {
        self.decl_bodies
            .header_index()
            .augmentation_type_header(scope, name)?;
        self.decl_bodies.augmentation_type_decl(scope, name)
    }

    /// Value-space counterpart of [`Self::augmentation_type_decl`].
    pub fn augmentation_value_decl(
        &self,
        scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
        name: &str,
    ) -> Option<Arc<LoweredValueDecl>> {
        self.decl_bodies
            .header_index()
            .augmentation_value_header(scope, name)?;
        self.decl_bodies.augmentation_value_decl(scope, name)
    }

    /// Classify one file-scope TYPE symbol's dependency edges: the local
    /// vs external split over the per-declaration dependency names,
    /// `typeof`-root import edges appended, deterministic ordering by
    /// final sort. Lowers the body through the memo to read its
    /// dependency-name roots; stores ONLY dependency edges.
    fn classify_type_deps(&self, name: &str) -> Option<Arc<ClassifiedTypeDeps>> {
        let lowered = self.decl_bodies.type_decl(name)?;

        // LOCAL deps: dependency-name roots that are local type symbols
        // (analyzer name inventory — header data), never imports, never
        // self.
        let mut local_set = FxHashSet::default();
        for reference in &lowered.dep_names {
            let root = reference.split('.').next().unwrap_or(reference.as_str());
            if self.import_locals.contains(root) {
                continue;
            }
            // Local membership consults the analyzer inventory AND the
            // shallow header index: namespaced (`N.T`), default-aliased,
            // and JSDoc-`@typedef` symbols live in the header index but
            // not the analyzer's `local_type_symbol` table, so a dep on
            // one must still resolve as a LOCAL hop — never a phantom
            // import / dropped edge.
            if root != name
                && (self.analysis.local_type_symbol(root).is_some() || self.has_type_symbol(root))
            {
                local_set.insert(root.to_string());
            }
        }
        let mut local_deps: Vec<String> = local_set.into_iter().collect();
        local_deps.sort();

        // EXTERNAL deps: dependency-name roots bound by imports, deduped
        // by `(specifier, imported_name)` via keyed-map accumulation.
        let mut external_deps = Vec::new();
        let mut seen_external: FxHashSet<(String, String)> = FxHashSet::default();
        for dep_name in lowered
            .dep_names
            .iter()
            .chain(lowered.structural_dep_names.iter())
        {
            let root = dep_name.split('.').next().unwrap_or(dep_name.as_str());
            if !self.import_locals.contains(root) {
                continue;
            }
            let Some(target) = self.import_targets.get(root) else {
                continue;
            };
            let imported_name = if root == dep_name {
                target.imported_name.clone()
            } else if let Some(suffix) = dep_name.strip_prefix(root) {
                format!("{}{suffix}", target.imported_name)
            } else {
                target.imported_name.clone()
            };
            if !seen_external.insert((target.source_specifier.clone(), imported_name.clone())) {
                continue;
            }
            external_deps.push(ExternalSymbolRef {
                local_name: dep_name.clone(),
                source_specifier: target.source_specifier.clone(),
                imported_name,
                canonical_id: external_canonical(target),
                route: RouteDemand::Whole,
            });
        }

        // `typeof` roots referencing imports contribute external edges
        // (the precomputed roots ride on the lowered entry — no body
        // re-walk here).
        for root in &lowered.typeof_root_names {
            let Some(target) = self.import_targets.get(root.as_str()) else {
                continue;
            };
            if !seen_external.insert((
                target.source_specifier.clone(),
                target.imported_name.clone(),
            )) {
                continue;
            }
            external_deps.push(ExternalSymbolRef {
                local_name: root.clone(),
                source_specifier: target.source_specifier.clone(),
                imported_name: target.imported_name.clone(),
                canonical_id: external_canonical(target),
                route: RouteDemand::Whole,
            });
        }

        external_deps.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.source_specifier.cmp(&right.source_specifier))
                .then_with(|| left.imported_name.cmp(&right.imported_name))
        });

        Some(Arc::new(ClassifiedTypeDeps {
            local_deps,
            external_deps,
        }))
    }

    /// The transitive required-import closure of one local type — the
    /// import-local names the type's structural dependency graph reaches.
    /// Demand-scoped: only the walked symbols' bodies lower.
    pub(crate) fn required_import_names(&self, type_name: &str) -> FxHashSet<String> {
        let mut required_imports = FxHashSet::default();
        let mut visited = FxHashSet::default();
        let mut pending = vec![type_name.to_string()];

        // Import membership mirrors the analyzer's `import_locals` set
        // (named + default bindings; namespace bindings are NOT members),
        // so the walk agrees with the historical whole-analysis product.
        let is_import_local =
            |name: &str| -> bool { self.analysis.local_import_symbol_target(name).is_some() };

        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }

            if is_import_local(&current) {
                required_imports.insert(current);
                continue;
            }

            // Header-index membership too: namespaced / default-aliased /
            // JSDoc-`@typedef` symbols are not in the analyzer table but
            // ARE shallow type symbols whose structural dep graph must be
            // walked for the required-import closure.
            if self.analysis.local_type_symbol(&current).is_some() || self.has_type_symbol(&current)
            {
                let Some(lowered) = self.decl_bodies.type_decl(&current) else {
                    continue;
                };
                for reference in &lowered.structural_dep_names {
                    let root = reference
                        .split('.')
                        .next()
                        .map(str::to_string)
                        .unwrap_or_else(|| reference.clone());
                    if is_import_local(&root) {
                        required_imports.insert(root);
                    } else if !visited.contains(&root) {
                        pending.push(root);
                    }
                }
            }
        }

        required_imports
    }

    /// Whether this file declares any global (`declare global`) augmentation
    /// contributors for `name` (header-level check).
    pub fn has_global_augmentation(&self, name: &str) -> bool {
        self.decl_bodies
            .header_index()
            .augmentation_type_header(
                &verter_semantic::analysis::type_eval::AugmentationScopeKind::Global,
                name,
            )
            .is_some()
    }

    /// Inject the eager synthesised `.vue`-default value body that the
    /// file's eval-env path did not produce, plus the matching slim
    /// header record and the `ExportTarget::Local` entry so
    /// `default`-style routes resolve identically to userland-declared
    /// symbols.
    ///
    /// Used by [`super::vue_default_synth`] to publish the implicit
    /// SFC default-export instance shape without modifying the
    /// `verter_semantic` analysis pipeline (which sees only the raw
    /// `<script setup>` content and therefore cannot observe the
    /// compiler-driven default export). The body lands in the dedicated
    /// synthesised-body map (read through [`Self::value_decl`]); the
    /// header-only [`ShallowValueSymbol`] carries the provenance flag.
    ///
    /// No-op when a value symbol with the given name already exists
    /// — userland declarations always win over synthesised ones.
    pub fn insert_synthesised_value_default(&mut self, name: &str, lowered: LoweredValueDecl) {
        if self.has_value_symbol(name) {
            return;
        }
        let header = ShallowValueSymbol::synthesised_from_lowered(&lowered);
        self.synthesised_value_symbols
            .insert(name.to_string(), Arc::new(header));
        self.synthesised_value_bodies
            .insert(name.to_string(), Arc::new(lowered));
        self.exports
            .entry(name.to_string())
            .or_insert_with(|| ExportTarget::Local {
                symbol_name: name.to_string(),
            });
    }

    /// Check if a name is an import-local binding.
    pub fn is_import_local(&self, name: &str) -> bool {
        self.import_locals.contains(name)
    }

    /// Get the import target for a local import name.
    pub fn import_target(&self, local_name: &str) -> Option<&ImportTarget> {
        self.import_targets.get(local_name)
    }

    // -----------------------------------------------------------------------
    // Local closure
    // -----------------------------------------------------------------------

    /// Compute same-file closure for one symbol, collecting external refs.
    ///
    /// Budget limits the total number of local symbols visited to prevent
    /// pathological same-file dependency chains.
    pub fn local_closure(&self, symbol_name: &str, budget: usize) -> LocalClosureResult {
        let mut visited = FxHashSet::default();
        let mut pending = vec![symbol_name.to_string()];
        let mut external_refs = ExternalRefAccumulator::default();
        let mut local_used = Vec::new();
        let mut steps = 0;

        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            steps += 1;
            if steps >= budget {
                return LocalClosureResult {
                    status: LocalClosureStatus::BudgetExceeded,
                    local_symbols_used: local_used,
                    unresolved_external: external_refs.into_vec(),
                    steps: steps as u64,
                };
            }

            if let Some(deps) = self.type_deps(&current) {
                local_used.push(current.clone());

                // Queue same-file dependencies
                for dep in &deps.local_deps {
                    if !visited.contains(dep.as_str()) {
                        pending.push(dep.clone());
                    }
                }

                // Collect external refs
                for ext in &deps.external_deps {
                    external_refs.add(ext.clone());
                }
            } else if self.import_locals.contains(&current) {
                // This is an import — classify as external
                if let Some(target) = self.import_targets.get(&current) {
                    let ext_ref = ExternalSymbolRef {
                        local_name: current.clone(),
                        source_specifier: target.source_specifier.clone(),
                        imported_name: target.imported_name.clone(),
                        canonical_id: external_canonical(target),
                        route: RouteDemand::Whole,
                    };
                    external_refs.add(ext_ref);
                } else {
                    // Import-local without a target — treat as missing
                    return LocalClosureResult {
                        status: LocalClosureStatus::MissingLocalSymbol { name: current },
                        local_symbols_used: local_used,
                        unresolved_external: external_refs.into_vec(),
                        steps: steps as u64,
                    };
                }
            } else {
                return LocalClosureResult {
                    status: LocalClosureStatus::MissingLocalSymbol { name: current },
                    local_symbols_used: local_used,
                    unresolved_external: external_refs.into_vec(),
                    steps: steps as u64,
                };
            }
        }

        let external_refs = external_refs.into_vec();
        let status = if external_refs.is_empty() {
            LocalClosureStatus::Resolved
        } else {
            LocalClosureStatus::ResolvedWithExternalDeps
        };

        LocalClosureResult {
            status,
            local_symbols_used: local_used,
            unresolved_external: external_refs,
            steps: steps as u64,
        }
    }

    /// Compute a narrower closure for a specific route on an exported symbol.
    ///
    /// For `Route::Whole`, delegates to `local_closure`.
    /// For `Route::Member(m)`, starts only from the member's type deps
    /// (if per-member tracking is available for the symbol).
    /// For `Route::Pick(members)`, unions the member deps for all listed members.
    ///
    /// Falls back to whole closure when member-level data is unavailable.
    pub fn route_closure(
        &self,
        symbol_name: &str,
        route: &RouteDemand,
        budget: usize,
    ) -> LocalClosureResult {
        match route {
            RouteDemand::Whole => self.whole_route_closure(symbol_name, budget),
            RouteDemand::MemberPath(path) if !path.is_empty() => {
                self.member_path_route_closure(symbol_name, path, budget)
            }
            RouteDemand::MemberPath(_) => self.local_closure(symbol_name, budget),
            RouteDemand::Pick(members) => {
                let refs: Vec<&str> = members.iter().map(|s| s.as_str()).collect();
                self.member_route_closure(symbol_name, &refs, budget)
            }
            RouteDemand::Omit(omitted) => {
                let Some(lowered) = self.type_decl(symbol_name) else {
                    return self.local_closure(symbol_name, budget);
                };
                let lookup = lowered.body.lookup_object();
                let Some(members) = direct_object_member_names(lookup.as_ref()) else {
                    return self.local_closure(symbol_name, budget);
                };
                let omitted: FxHashSet<&str> = omitted.iter().map(|name| name.as_str()).collect();
                let remaining = members
                    .into_iter()
                    .filter(|name| !omitted.contains(name.as_str()))
                    .collect::<Vec<_>>();
                if remaining.is_empty() {
                    return LocalClosureResult {
                        status: LocalClosureStatus::Resolved,
                        local_symbols_used: vec![symbol_name.to_string()],
                        unresolved_external: Vec::new(),
                        steps: 1,
                    };
                }
                let refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();
                self.member_route_closure(symbol_name, &refs, budget)
            }
        }
    }

    fn member_path_route_closure(
        &self,
        symbol_name: &str,
        path: &[String],
        budget: usize,
    ) -> LocalClosureResult {
        if path.len() == 1 {
            return self.member_route_closure(symbol_name, &[path[0].as_str()], budget);
        }

        let Some(lowered) = self.type_decl(symbol_name) else {
            return self.local_closure(symbol_name, budget);
        };

        let mut seed_names = Vec::new();
        let mut seed_external = ExternalRefAccumulator::default();
        let mut seen_symbols = FxHashSet::default();
        let lookup = lowered.body.lookup_object();
        let found_path = collect_member_path_seed_names(
            self,
            lookup.as_ref(),
            path,
            &mut seed_names,
            &mut seed_external,
            &mut seen_symbols,
        );

        if !found_path {
            return LocalClosureResult {
                status: LocalClosureStatus::Resolved,
                local_symbols_used: vec![symbol_name.to_string()],
                unresolved_external: Vec::new(),
                steps: 1,
            };
        }

        if seed_names.is_empty() && seed_external.is_empty() {
            return LocalClosureResult {
                status: LocalClosureStatus::Resolved,
                local_symbols_used: vec![symbol_name.to_string()],
                unresolved_external: Vec::new(),
                steps: 1,
            };
        }

        let mut visited = FxHashSet::default();
        visited.insert(symbol_name.to_string());
        let mut pending = seed_names;
        let mut external_refs = seed_external;
        let mut local_used = vec![symbol_name.to_string()];
        let mut steps = 1u64;

        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            steps += 1;
            if steps as usize >= budget {
                return LocalClosureResult {
                    status: LocalClosureStatus::BudgetExceeded,
                    local_symbols_used: local_used,
                    unresolved_external: external_refs.into_vec(),
                    steps,
                };
            }

            if let Some(dep_edges) = self.type_deps(&current) {
                local_used.push(current.clone());
                for dep in &dep_edges.local_deps {
                    if !visited.contains(dep.as_str()) {
                        pending.push(dep.clone());
                    }
                }
                for ext in &dep_edges.external_deps {
                    external_refs.add(ext.clone());
                }
            } else if self.import_locals.contains(&current) {
                if let Some(target) = self.import_targets.get(&current) {
                    let ext_ref = ExternalSymbolRef {
                        local_name: current.clone(),
                        source_specifier: target.source_specifier.clone(),
                        imported_name: target.imported_name.clone(),
                        canonical_id: external_canonical(target),
                        route: RouteDemand::Whole,
                    };
                    external_refs.add(ext_ref);
                }
            }
        }

        let external_refs = external_refs.into_vec();
        let status = if external_refs.is_empty() {
            LocalClosureStatus::Resolved
        } else {
            LocalClosureStatus::ResolvedWithExternalDeps
        };

        LocalClosureResult {
            status,
            local_symbols_used: local_used,
            unresolved_external: external_refs,
            steps,
        }
    }

    /// Internal: compute closure starting from specific member deps only.
    fn member_route_closure(
        &self,
        symbol_name: &str,
        members: &[&str],
        budget: usize,
    ) -> LocalClosureResult {
        let lowered = match self.type_decl(symbol_name) {
            Some(s) => s,
            None => return self.local_closure(symbol_name, budget),
        };

        // If no member_deps tracking, fall back to whole closure
        if lowered.member_deps.is_empty() {
            return self.local_closure(symbol_name, budget);
        }

        // Collect the initial seed names from the requested members' deps
        let lookup = lowered.body.lookup_object();
        let mut seed_names: Vec<String> = Vec::new();
        let mut saw_known_member = false;
        for member in members {
            if let Some(deps) = lowered.member_deps.get(*member) {
                saw_known_member = true;
                for dep in deps {
                    if !seed_names.contains(dep) {
                        seed_names.push(dep.clone());
                    }
                }
                continue;
            }
            if let Some(prop) = direct_object_property(lookup.as_ref(), member) {
                saw_known_member = true;
                let mut refs = Vec::new();
                collect_type_refs(&prop.ty, &mut refs);
                for dep in refs {
                    if !seed_names.contains(&dep) {
                        seed_names.push(dep);
                    }
                }
                continue;
            }
        }

        if !saw_known_member {
            return self.local_closure(symbol_name, budget);
        }

        if seed_names.is_empty() {
            // No deps for the requested members — minimal closure
            return LocalClosureResult {
                status: LocalClosureStatus::Resolved,
                local_symbols_used: vec![symbol_name.to_string()],
                unresolved_external: Vec::new(),
                steps: 1,
            };
        }

        // Now run the local closure starting from only the seed names
        let mut visited = FxHashSet::default();
        visited.insert(symbol_name.to_string()); // mark the root as visited
        let mut pending = seed_names;
        let mut external_refs = ExternalRefAccumulator::default();
        let mut local_used = vec![symbol_name.to_string()];
        let mut steps = 1u64;

        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            steps += 1;
            if steps as usize >= budget {
                return LocalClosureResult {
                    status: LocalClosureStatus::BudgetExceeded,
                    local_symbols_used: local_used,
                    unresolved_external: external_refs.into_vec(),
                    steps,
                };
            }

            if let Some(dep_edges) = self.type_deps(&current) {
                local_used.push(current.clone());
                for dep in &dep_edges.local_deps {
                    if !visited.contains(dep.as_str()) {
                        pending.push(dep.clone());
                    }
                }
                for ext in &dep_edges.external_deps {
                    external_refs.add(ext.clone());
                }
            } else if self.import_locals.contains(&current) {
                if let Some(target) = self.import_targets.get(&current) {
                    let ext_ref = ExternalSymbolRef {
                        local_name: current.clone(),
                        source_specifier: target.source_specifier.clone(),
                        imported_name: target.imported_name.clone(),
                        canonical_id: external_canonical(target),
                        route: RouteDemand::Whole,
                    };
                    external_refs.add(ext_ref);
                }
            }
            // Skip unknown names silently — they may be type parameters
        }

        let external_refs = external_refs.into_vec();
        let status = if external_refs.is_empty() {
            LocalClosureStatus::Resolved
        } else {
            LocalClosureStatus::ResolvedWithExternalDeps
        };

        LocalClosureResult {
            status,
            local_symbols_used: local_used,
            unresolved_external: external_refs,
            steps,
        }
    }

    fn whole_route_closure(&self, symbol_name: &str, budget: usize) -> LocalClosureResult {
        let Some(lowered) = self.type_decl(symbol_name) else {
            return self.local_closure(symbol_name, budget);
        };

        let mut visited = FxHashSet::default();
        visited.insert(symbol_name.to_string());
        let mut local_used = vec![symbol_name.to_string()];
        let mut external_refs = ExternalRefAccumulator::default();
        let mut steps = 1u64;

        let lookup = lowered.body.lookup_object();
        if !self.collect_whole_route_refs(
            lookup.as_ref(),
            WholeRouteContext::Root,
            &mut visited,
            &mut local_used,
            &mut external_refs,
            &mut steps,
            budget,
        ) {
            return LocalClosureResult {
                status: LocalClosureStatus::BudgetExceeded,
                local_symbols_used: local_used,
                unresolved_external: external_refs.into_vec(),
                steps,
            };
        }

        let external_refs = external_refs.into_vec();
        let status = if external_refs.is_empty() {
            LocalClosureStatus::Resolved
        } else {
            LocalClosureStatus::ResolvedWithExternalDeps
        };

        LocalClosureResult {
            status,
            local_symbols_used: local_used,
            unresolved_external: external_refs,
            steps,
        }
    }

    fn collect_whole_route_refs(
        &self,
        expr: &TypeExpr,
        context: WholeRouteContext,
        visited: &mut FxHashSet<String>,
        local_used: &mut Vec<String>,
        external_refs: &mut ExternalRefAccumulator,
        steps: &mut u64,
        budget: usize,
    ) -> bool {
        match expr {
            TypeExpr::Parenthesized(inner) | TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => self
                .collect_whole_route_refs(
                    inner,
                    context,
                    visited,
                    local_used,
                    external_refs,
                    steps,
                    budget,
                ),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for inner in types.iter() {
                    if !self.collect_whole_route_refs(
                        inner,
                        context,
                        visited,
                        local_used,
                        external_refs,
                        steps,
                        budget,
                    ) {
                        return false;
                    }
                }
                true
            }
            TypeExpr::Array { element, .. } => self.collect_whole_route_refs(
                element,
                context,
                visited,
                local_used,
                external_refs,
                steps,
                budget,
            ),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter() {
                    if !self.collect_whole_route_refs(
                        &element.ty,
                        context,
                        visited,
                        local_used,
                        external_refs,
                        steps,
                        budget,
                    ) {
                        return false;
                    }
                }
                true
            }
            TypeExpr::Object(obj) => {
                if matches!(context, WholeRouteContext::LeafProperty) {
                    return true;
                }

                for member in &obj.properties {
                    let status = match member {
                        verter_type_expr::ObjectMember::Property(prop) => self
                            .collect_whole_route_refs(
                                &prop.ty,
                                WholeRouteContext::LeafProperty,
                                visited,
                                local_used,
                                external_refs,
                                steps,
                                budget,
                            ),
                        verter_type_expr::ObjectMember::IndexSignature(sig) => self
                            .collect_whole_route_refs(
                                &sig.value_type,
                                WholeRouteContext::LeafProperty,
                                visited,
                                local_used,
                                external_refs,
                                steps,
                                budget,
                            ),
                        verter_type_expr::ObjectMember::CallSignature(func)
                        | verter_type_expr::ObjectMember::ConstructSignature(func) => self
                            .collect_whole_route_function_refs(
                                func,
                                visited,
                                local_used,
                                external_refs,
                                steps,
                                budget,
                            ),
                        verter_type_expr::ObjectMember::Method(method) => self
                            .collect_whole_route_function_refs(
                                &method.function,
                                visited,
                                local_used,
                                external_refs,
                                steps,
                                budget,
                            ),
                    };
                    if !status {
                        return false;
                    }
                }
                true
            }
            // A constructor type carries the same `FunctionExpr` payload as a
            // function type; its whole-route signature refs are collected
            // identically.
            TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                if matches!(context, WholeRouteContext::LeafProperty) {
                    return true;
                }
                self.collect_whole_route_function_refs(
                    func,
                    visited,
                    local_used,
                    external_refs,
                    steps,
                    budget,
                )
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                let symbol_name = name.as_ref();
                if let Some(target) = self.import_targets.get(symbol_name) {
                    if matches!(
                        context,
                        WholeRouteContext::Root | WholeRouteContext::CallableParam
                    ) {
                        external_refs.add(ExternalSymbolRef {
                            local_name: symbol_name.to_string(),
                            source_specifier: target.source_specifier.clone(),
                            imported_name: target.imported_name.clone(),
                            canonical_id: external_canonical(target),
                            route: RouteDemand::Whole,
                        });
                    }
                    return true;
                }

                if let Some(route) = self.utility_route_for_ref(symbol_name, type_arguments) {
                    return self.follow_routed_expr(
                        &type_arguments[0],
                        route,
                        visited,
                        local_used,
                        external_refs,
                        steps,
                        budget,
                    );
                }

                if matches!(
                    symbol_name,
                    "Partial" | "Required" | "Readonly" | "NonNullable"
                ) && !type_arguments.is_empty()
                    && !matches!(context, WholeRouteContext::LeafProperty)
                {
                    return self.collect_whole_route_refs(
                        &type_arguments[0],
                        context,
                        visited,
                        local_used,
                        external_refs,
                        steps,
                        budget,
                    );
                }

                if self.has_type_symbol(symbol_name) {
                    return self.follow_local_symbol_precise(
                        symbol_name,
                        context,
                        visited,
                        local_used,
                        external_refs,
                        steps,
                        budget,
                    );
                }

                true
            }
            TypeExpr::IndexedAccess { .. } => {
                let Some((base_expr, route)) = self.extract_indexed_access_route(expr) else {
                    return true;
                };
                self.follow_routed_expr(
                    base_expr,
                    route,
                    visited,
                    local_used,
                    external_refs,
                    steps,
                    budget,
                )
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                for inner in [check, extends, true_type, false_type] {
                    if !self.collect_whole_route_refs(
                        inner,
                        context,
                        visited,
                        local_used,
                        external_refs,
                        steps,
                        budget,
                    ) {
                        return false;
                    }
                }
                true
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                if !self.collect_whole_route_refs(
                    source,
                    context,
                    visited,
                    local_used,
                    external_refs,
                    steps,
                    budget,
                ) {
                    return false;
                }
                if !self.collect_whole_route_refs(
                    value,
                    context,
                    visited,
                    local_used,
                    external_refs,
                    steps,
                    budget,
                ) {
                    return false;
                }
                if let Some(name_type) = name_type.as_deref() {
                    return self.collect_whole_route_refs(
                        name_type,
                        context,
                        visited,
                        local_used,
                        external_refs,
                        steps,
                        budget,
                    );
                }
                true
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for inner in expressions.iter() {
                    if !self.collect_whole_route_refs(
                        inner,
                        context,
                        visited,
                        local_used,
                        external_refs,
                        steps,
                        budget,
                    ) {
                        return false;
                    }
                }
                true
            }
            TypeExpr::TypeOf(value_ref) => {
                if let Some(root) = value_ref.path.first() {
                    if let Some(target) = self.import_targets.get(root.as_str()) {
                        if matches!(
                            context,
                            WholeRouteContext::Root | WholeRouteContext::CallableParam
                        ) {
                            external_refs.add(ExternalSymbolRef {
                                local_name: root.clone(),
                                source_specifier: target.source_specifier.clone(),
                                imported_name: target.imported_name.clone(),
                                canonical_id: external_canonical(target),
                                route: RouteDemand::Whole,
                            });
                        }
                    }
                }
                true
            }
            // Mirrors the `Ref` arm's recursion into `type_arguments`. The
            // `specifier`/`qualifier` are leaf strings naming the cross-file
            // module path (not a local route symbol to follow), so only the
            // nested type-argument exprs are walked for further refs.
            //
            // TODO(follow-up): the `specifier` names a cross-file module that
            // `typeof import("X")` / `import("X").T` depends on, but it is NOT
            // yet recorded as a cross-file dependency EDGE here — so a content
            // edit to module "X" does not invalidate this file's read-set
            // through the import-type path. The architect DEFERRED wiring this
            // into the cross-file invalidation/read-set bucket; do NOT stuff the
            // specifier into the local-ref accumulator as a fake local name — it
            // needs its own dependency-edge channel.
            TypeExpr::ImportType { type_arguments, .. } => {
                for argument in type_arguments.iter() {
                    if !self.collect_whole_route_refs(
                        argument,
                        context,
                        visited,
                        local_used,
                        external_refs,
                        steps,
                        budget,
                    ) {
                        return false;
                    }
                }
                true
            }
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::RecursiveRef { .. }
            // Synthetic carriers contribute no whole-route refs — they
            // are intrinsic terminal leaves.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Unknown { .. } => true,
        }
    }

    fn collect_whole_route_function_refs(
        &self,
        func: &verter_type_expr::FunctionExpr,
        visited: &mut FxHashSet<String>,
        local_used: &mut Vec<String>,
        external_refs: &mut ExternalRefAccumulator,
        steps: &mut u64,
        budget: usize,
    ) -> bool {
        for param in &func.parameters {
            if !self.collect_whole_route_refs(
                &param.ty,
                WholeRouteContext::CallableParam,
                visited,
                local_used,
                external_refs,
                steps,
                budget,
            ) {
                return false;
            }
        }
        for type_param in &func.type_parameters {
            if let Some(constraint) = type_param.constraint.as_deref() {
                if !self.collect_whole_route_refs(
                    constraint,
                    WholeRouteContext::CallableParam,
                    visited,
                    local_used,
                    external_refs,
                    steps,
                    budget,
                ) {
                    return false;
                }
            }
            if let Some(default) = type_param.default.as_deref() {
                if !self.collect_whole_route_refs(
                    default,
                    WholeRouteContext::CallableParam,
                    visited,
                    local_used,
                    external_refs,
                    steps,
                    budget,
                ) {
                    return false;
                }
            }
        }
        true
    }

    fn follow_local_symbol_precise(
        &self,
        symbol_name: &str,
        context: WholeRouteContext,
        visited: &mut FxHashSet<String>,
        local_used: &mut Vec<String>,
        external_refs: &mut ExternalRefAccumulator,
        steps: &mut u64,
        budget: usize,
    ) -> bool {
        if !visited.insert(symbol_name.to_string()) {
            return true;
        }
        *steps += 1;
        if *steps as usize >= budget {
            return false;
        }
        let Some(lowered) = self.type_decl(symbol_name) else {
            return true;
        };
        local_used.push(symbol_name.to_string());
        let lookup = lowered.body.lookup_object();
        self.collect_whole_route_refs(
            lookup.as_ref(),
            context,
            visited,
            local_used,
            external_refs,
            steps,
            budget,
        )
    }

    fn follow_routed_expr(
        &self,
        expr: &TypeExpr,
        route: RouteDemand,
        visited: &mut FxHashSet<String>,
        local_used: &mut Vec<String>,
        external_refs: &mut ExternalRefAccumulator,
        steps: &mut u64,
        budget: usize,
    ) -> bool {
        match expr {
            TypeExpr::Parenthesized(inner) => self.follow_routed_expr(
                inner,
                route,
                visited,
                local_used,
                external_refs,
                steps,
                budget,
            ),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                let symbol_name = name.as_ref();
                if let Some(target) = self.import_targets.get(symbol_name) {
                    external_refs.add(ExternalSymbolRef {
                        local_name: symbol_name.to_string(),
                        source_specifier: target.source_specifier.clone(),
                        imported_name: target.imported_name.clone(),
                        canonical_id: external_canonical(target),
                        route,
                    });
                    return true;
                }
                if self.has_type_symbol(symbol_name) {
                    match &route {
                        RouteDemand::Whole => self.follow_local_symbol_precise(
                            symbol_name,
                            WholeRouteContext::Root,
                            visited,
                            local_used,
                            external_refs,
                            steps,
                            budget,
                        ),
                        RouteDemand::MemberPath(path) => {
                            let Some(lowered) = self.type_decl(symbol_name) else {
                                return true;
                            };
                            let mut seed_names = Vec::new();
                            let mut seed_external = ExternalRefAccumulator::default();
                            let mut seen_symbols = FxHashSet::default();
                            let lookup = lowered.body.lookup_object();
                            let found_path = collect_member_path_seed_names(
                                self,
                                lookup.as_ref(),
                                path,
                                &mut seed_names,
                                &mut seed_external,
                                &mut seen_symbols,
                            );
                            if !found_path {
                                return true;
                            }
                            for ext in seed_external.into_vec() {
                                external_refs.add(ext);
                            }
                            for seed_name in seed_names {
                                if let Some(target) = self.import_targets.get(seed_name.as_str()) {
                                    external_refs.add(ExternalSymbolRef {
                                        local_name: seed_name.clone(),
                                        source_specifier: target.source_specifier.clone(),
                                        imported_name: target.imported_name.clone(),
                                        canonical_id: external_canonical(target),
                                        route: RouteDemand::Whole,
                                    });
                                } else if self.has_type_symbol(seed_name.as_str())
                                    && !self.follow_local_symbol_precise(
                                        seed_name.as_str(),
                                        WholeRouteContext::Root,
                                        visited,
                                        local_used,
                                        external_refs,
                                        steps,
                                        budget,
                                    )
                                {
                                    return false;
                                }
                            }
                            true
                        }
                        RouteDemand::Pick(_) | RouteDemand::Omit(_) => {
                            let closure = self.route_closure(symbol_name, &route, budget);
                            if matches!(closure.status, LocalClosureStatus::BudgetExceeded) {
                                return false;
                            }
                            for local_name in closure.local_symbols_used {
                                if visited.insert(local_name.clone()) {
                                    local_used.push(local_name);
                                }
                            }
                            for ext in closure.unresolved_external {
                                external_refs.add(ext);
                            }
                            true
                        }
                    }
                } else {
                    true
                }
            }
            _ => true,
        }
    }

    fn utility_route_for_ref(
        &self,
        name: &str,
        type_arguments: &[TypeExpr],
    ) -> Option<RouteDemand> {
        if type_arguments.len() != 2 {
            return None;
        }
        let mut seen_locals = FxHashSet::default();
        let keys =
            self.extract_string_literal_keys_from_type_expr(&type_arguments[1], &mut seen_locals);
        if keys.is_empty() {
            return None;
        }
        match name {
            "Pick" => Some(RouteDemand::Pick(keys)),
            "Omit" => Some(RouteDemand::Omit(keys)),
            _ => None,
        }
    }

    fn extract_string_literal_keys_from_type_expr(
        &self,
        expr: &TypeExpr,
        seen_locals: &mut FxHashSet<String>,
    ) -> Vec<String> {
        match expr {
            TypeExpr::Literal(verter_type_expr::LiteralValue::String(value)) => {
                vec![value.clone()]
            }
            TypeExpr::Union(types) => {
                let mut keys = Vec::new();
                for inner in types.iter() {
                    keys.extend(
                        self.extract_string_literal_keys_from_type_expr(inner, seen_locals),
                    );
                }
                keys.sort();
                keys.dedup();
                keys
            }
            TypeExpr::Parenthesized(inner) => {
                self.extract_string_literal_keys_from_type_expr(inner, seen_locals)
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() && self.has_type_symbol(name.as_ref()) => {
                if !seen_locals.insert(name.to_string()) {
                    return Vec::new();
                }
                let keys = self
                    .type_decl(name.as_ref())
                    .map(|lowered| {
                        let lookup = lowered.body.lookup_object();
                        self.extract_string_literal_keys_from_type_expr(
                            lookup.as_ref(),
                            seen_locals,
                        )
                    })
                    .unwrap_or_default();
                seen_locals.remove(name.as_ref());
                keys
            }
            _ => Vec::new(),
        }
    }

    fn extract_indexed_access_route<'a>(
        &self,
        expr: &'a TypeExpr,
    ) -> Option<(&'a TypeExpr, RouteDemand)> {
        let TypeExpr::IndexedAccess { object, index } = expr else {
            return None;
        };
        let mut seen_locals = FxHashSet::default();
        let keys = self.extract_string_literal_keys_from_type_expr(index, &mut seen_locals);
        if keys.is_empty() {
            return None;
        }
        let (base_expr, mut path) = self.extract_indexed_access_base(object.as_ref())?;
        if keys.len() == 1 {
            path.push(keys[0].clone());
            Some((base_expr, RouteDemand::MemberPath(path)))
        } else if path.is_empty() {
            Some((base_expr, RouteDemand::Pick(keys)))
        } else {
            None
        }
    }

    fn extract_indexed_access_base<'a>(
        &self,
        expr: &'a TypeExpr,
    ) -> Option<(&'a TypeExpr, Vec<String>)> {
        match expr {
            TypeExpr::Parenthesized(inner) => self.extract_indexed_access_base(inner),
            TypeExpr::IndexedAccess { .. } => {
                let (base_expr, route) = self.extract_indexed_access_route(expr)?;
                match route {
                    RouteDemand::MemberPath(path) => Some((base_expr, path)),
                    _ => None,
                }
            }
            _ => Some((expr, Vec::new())),
        }
    }
}

fn collect_member_path_seed_names(
    state: &ShallowFileState,
    expr: &TypeExpr,
    path: &[String],
    seed_names: &mut Vec<String>,
    seed_external: &mut ExternalRefAccumulator,
    seen_symbols: &mut FxHashSet<String>,
) -> bool {
    if path.is_empty() {
        collect_type_refs(expr, seed_names);
        seed_names.sort();
        seed_names.dedup();
        return true;
    }

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => {
            let symbol_name = name.to_string();
            if let Some(target) = state.import_targets.get(symbol_name.as_str()) {
                seed_external.add(ExternalSymbolRef {
                    local_name: symbol_name,
                    source_specifier: target.source_specifier.clone(),
                    imported_name: target.imported_name.clone(),
                    canonical_id: external_canonical(target),
                    route: RouteDemand::MemberPath(path.to_vec()),
                });
                return true;
            }
            if !seen_symbols.insert(symbol_name.clone()) {
                return false;
            }
            let result = state
                .type_decl(symbol_name.as_str())
                .is_some_and(|lowered| {
                    let lookup = lowered.body.lookup_object();
                    collect_member_path_seed_names(
                        state,
                        lookup.as_ref(),
                        path,
                        seed_names,
                        seed_external,
                        seen_symbols,
                    )
                });
            seen_symbols.remove(symbol_name.as_str());
            result
        }
        TypeExpr::Parenthesized(inner) => collect_member_path_seed_names(
            state,
            inner,
            path,
            seed_names,
            seed_external,
            seen_symbols,
        ),
        _ => {
            let Some(prop) = direct_object_property(expr, path[0].as_str()) else {
                return false;
            };
            if path.len() == 1 {
                collect_type_refs(&prop.ty, seed_names);
                seed_names.sort();
                seed_names.dedup();
                true
            } else {
                collect_member_path_seed_names(
                    state,
                    &prop.ty,
                    &path[1..],
                    seed_names,
                    seed_external,
                    seen_symbols,
                )
            }
        }
    }
}

impl<'a> ShallowTypeView<'a> {
    /// Look up a named export. Returns `None` if the name is not directly
    /// exported (may still be available through wildcard reexports).
    pub fn export_target(self, name: &str) -> Option<&'a ExportTarget> {
        self.state.export_target(name)
    }

    /// Wildcard `export *` sources in declaration order.
    pub fn wildcard_reexports(self) -> &'a [WildcardReexport] {
        &self.state.wildcard_reexports
    }

    /// Look up a local type symbol by name (demand-materialised).
    pub fn symbol(self, name: &str) -> Option<Arc<ShallowTypeSymbol>> {
        self.state.symbol(name)
    }

    /// Compute same-file closure for one symbol.
    pub fn local_closure(self, symbol_name: &str, budget: usize) -> LocalClosureResult {
        self.state.local_closure(symbol_name, budget)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract per-member dependency names from direct object slices in the body.
/// For each direct property, collects all type names referenced in that
/// property's type annotation. Transparent intersections are flattened
/// right-to-left so declaration-merged interfaces keep earlier members while
/// later object slices win on duplicate names.
pub(crate) fn extract_member_deps(body: &TypeExpr) -> FxHashMap<String, Vec<String>> {
    let mut result = FxHashMap::default();
    for prop in direct_object_properties(body) {
        let mut refs = Vec::new();
        collect_type_refs(&prop.ty, &mut refs);
        if !refs.is_empty() {
            result.insert(prop.name.clone(), refs);
        }
    }
    result
}

fn direct_object_member_names(body: &TypeExpr) -> Option<Vec<String>> {
    let names = direct_object_properties(body)
        .into_iter()
        .map(|prop| prop.name.clone())
        .collect::<Vec<_>>();
    (!names.is_empty()).then_some(names)
}

fn direct_object_property<'a>(
    body: &'a TypeExpr,
    name: &str,
) -> Option<&'a verter_type_expr::ObjectProperty> {
    direct_object_properties(body)
        .into_iter()
        .find(|prop| prop.name == name)
}

fn direct_object_properties(body: &TypeExpr) -> Vec<&verter_type_expr::ObjectProperty> {
    let mut result = Vec::new();
    let mut seen = FxHashSet::default();
    collect_direct_object_properties(body, &mut result, &mut seen);
    result
}

fn collect_direct_object_properties<'a>(
    body: &'a TypeExpr,
    out: &mut Vec<&'a verter_type_expr::ObjectProperty>,
    seen: &mut FxHashSet<String>,
) {
    match body {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let verter_type_expr::ObjectMember::Property(prop) = member {
                    if seen.insert(prop.name.clone()) {
                        out.push(prop);
                    }
                }
            }
        }
        TypeExpr::Intersection(parts) => {
            for part in parts.iter().rev() {
                collect_direct_object_properties(part, out, seen);
            }
        }
        TypeExpr::Parenthesized(inner) => {
            collect_direct_object_properties(inner, out, seen);
        }
        _ => {}
    }
}

/// Collect all named type references from a TypeExpr, non-recursively
/// (only direct references, not transitive).
pub(crate) fn collect_type_refs(expr: &TypeExpr, out: &mut Vec<String>) {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            out.push(name.to_string());
            for arg in type_arguments.iter() {
                collect_type_refs(arg, out);
            }
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            for m in members.iter() {
                collect_type_refs(m, out);
            }
        }
        TypeExpr::Array { element, .. } => collect_type_refs(element, out),
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let verter_type_expr::ObjectMember::Property(prop) = member {
                    collect_type_refs(&prop.ty, out);
                }
            }
        }
        TypeExpr::Tuple { elements, .. } => {
            for el in elements.iter() {
                collect_type_refs(&el.ty, out);
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_type_refs(object, out);
            collect_type_refs(index, out);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_type_refs(check, out);
            collect_type_refs(extends, out);
            collect_type_refs(true_type, out);
            collect_type_refs(false_type, out);
        }
        // A constructor type's signature refs are collected identically to a
        // function type's (same `FunctionExpr` payload).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            for param in &func.parameters {
                collect_type_refs(&param.ty, out);
            }
            if let Some(ref ret) = func.return_type {
                collect_type_refs(ret, out);
            }
        }
        TypeExpr::Mapped { source, value, .. } => {
            collect_type_refs(source, out);
            collect_type_refs(value, out);
        }
        TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) | TypeExpr::Parenthesized(inner) => {
            collect_type_refs(inner, out);
        }
        // Mirrors the `Ref` arm's recursion into `type_arguments`. The
        // `specifier`/`qualifier` are a module path, not a collectable local
        // type-ref name, so only the nested type-argument exprs are walked.
        TypeExpr::ImportType { type_arguments, .. } => {
            for arg in type_arguments.iter() {
                collect_type_refs(arg, out);
            }
        }
        TypeExpr::TypeOf { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        // Synthetic carriers reference no type aliases — terminal leaves.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => {}
    }
}

/// Fold one lazily lowered VALUE declaration into the shallow value
/// symbol shape (synthesised-default provenance is always `false` for a
/// lowered userland declaration).
pub(crate) fn collect_typeof_roots(expr: &TypeExpr, out: &mut FxHashSet<String>) {
    match expr {
        TypeExpr::TypeOf(value_ref) => {
            if let Some(root) = value_ref.path.first() {
                out.insert(root.clone());
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for inner in types.iter() {
                collect_typeof_roots(inner, out);
            }
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => collect_typeof_roots(element, out),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_typeof_roots(&element.ty, out);
            }
        }
        TypeExpr::Object(object) => {
            for member in &object.properties {
                match member {
                    verter_type_expr::ObjectMember::Property(prop) => {
                        collect_typeof_roots(&prop.ty, out);
                    }
                    verter_type_expr::ObjectMember::IndexSignature(sig) => {
                        collect_typeof_roots(&sig.key_type, out);
                        collect_typeof_roots(&sig.value_type, out);
                    }
                    verter_type_expr::ObjectMember::CallSignature(func)
                    | verter_type_expr::ObjectMember::ConstructSignature(func) => {
                        collect_typeof_roots_in_function(func, out);
                    }
                    verter_type_expr::ObjectMember::Method(method) => {
                        collect_typeof_roots_in_function(&method.function, out);
                    }
                }
            }
        }
        // A constructor type's typeof roots are collected identically to a
        // function type's (same `FunctionExpr` payload).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            collect_typeof_roots_in_function(func, out)
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_typeof_roots(object, out);
            collect_typeof_roots(index, out);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_typeof_roots(check, out);
            collect_typeof_roots(extends, out);
            collect_typeof_roots(true_type, out);
            collect_typeof_roots(false_type, out);
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_typeof_roots(source, out);
            collect_typeof_roots(value, out);
            if let Some(name_type) = name_type.as_deref() {
                collect_typeof_roots(name_type, out);
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            for expr in expressions.iter() {
                collect_typeof_roots(expr, out);
            }
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::RecursiveRef { .. }
        // Synthetic carriers carry no `typeof` operand — terminal leaves.
        | TypeExpr::SyntheticSlotBinding(_)
        // An import-type carries no `typeof X` operand of its own — like the
        // terminal `Ref` arm here, it is a no-op leaf (its module-path
        // qualifier is not a `typeof` root).
        | TypeExpr::ImportType { .. }
        | TypeExpr::Unknown { .. } => {}
    }
}

fn collect_typeof_roots_in_function(
    func: &verter_type_expr::FunctionExpr,
    out: &mut FxHashSet<String>,
) {
    for param in &func.parameters {
        collect_typeof_roots(&param.ty, out);
    }
    if let Some(return_type) = func.return_type.as_deref() {
        collect_typeof_roots(return_type, out);
    }
    for param in &func.type_parameters {
        if let Some(constraint) = param.constraint.as_deref() {
            collect_typeof_roots(constraint, out);
        }
        if let Some(default) = param.default.as_deref() {
            collect_typeof_roots(default, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::type_eval::ValueDeclKind;
    use verter_semantic::analysis::type_eval_build::parse_and_build_env;

    fn make_analysis(source: &str) -> Arc<AnalyzedExternalTypeSource> {
        let alloc = oxc_allocator::Allocator::new();
        Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source(
                source, &alloc,
            ),
        )
    }

    #[test]
    fn simple_interface_produces_local_export() {
        let analysis = make_analysis("export interface Props { label: string }");
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, None);

        assert!(
            state.export_target("Props").is_some(),
            "Props should be exported"
        );
        match state.export_target("Props").unwrap() {
            ExportTarget::Local { symbol_name } => {
                assert_eq!(symbol_name, "Props");
            }
            other => panic!("expected Local export, got {other:?}"),
        }
        assert!(
            state.wildcard_reexports.is_empty(),
            "no wildcard reexports expected"
        );
    }

    #[test]
    fn reexport_produces_reexport_target() {
        let analysis = make_analysis(r#"export { Foo } from "./inner""#);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, None);

        match state.export_target("Foo") {
            Some(ExportTarget::Reexport {
                source_specifier,
                original_name,
                ..
            }) => {
                assert_eq!(source_specifier, "./inner");
                assert_eq!(original_name, "Foo");
            }
            other => panic!("expected Reexport, got {other:?}"),
        }
    }

    #[test]
    fn wildcard_reexport_captured_in_order() {
        let analysis =
            make_analysis("export * from './a'\nexport * from './b'\nexport * from './c'\n");
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, None);

        let specifiers: Vec<&str> = state
            .wildcard_reexports
            .iter()
            .map(|w| w.source_specifier.as_str())
            .collect();
        assert_eq!(
            specifiers,
            vec!["./a", "./b", "./c"],
            "wildcard sources must be in declaration order"
        );
    }

    // NOTE: local_closure tests require eval_env to populate symbols.
    // Analysis-only construction produces empty symbols, so closure tests
    // use the eval-env path.  We test export routing and import targets
    // with analysis-only, and closure with eval-env below.

    #[test]
    fn analysis_only_has_no_symbols() {
        let analysis = make_analysis("export interface Props { label: string }\n");
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, None);

        // Symbols require eval_env
        assert!(
            state.type_symbol_names().next().is_none(),
            "analysis-only construction should produce no symbols"
        );
        // But exports should still be populated
        assert!(state.export_target("Props").is_some());
    }

    #[test]
    fn env_backed_construction_populates_symbols() {
        let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        assert!(
            state.symbol("Props").is_some(),
            "env-backed construction should populate type symbols"
        );
        let defaults = state
            .value_symbol("defaults")
            .expect("env-backed construction should populate value symbols");
        assert_eq!(defaults.kind, ValueDeclKind::Const);
        let defaults_body = state
            .value_decl("defaults")
            .expect("value body should lower on demand");
        assert!(defaults_body.type_annotation.is_some());
    }

    /// A type that references a JSDoc-`@typedef` name must classify that
    /// reference as a LOCAL dep — the typedef lives in the shallow header
    /// index but NOT the analyzer's `local_type_symbol` table, so local
    /// classification must consult the header index. Pre-fix the `Alias`
    /// hop is dropped (neither local nor external → a phantom/lost edge).
    #[test]
    fn jsdoc_typedef_reference_classifies_as_local_dep() {
        let source = r#"
import { Imported } from './dep'
/** @typedef {Imported} Alias */
export type X = Alias
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        // `Alias` is a shallow type symbol (header index) even though the
        // analyzer never tracked it.
        assert!(
            state.has_type_symbol("Alias"),
            "the JSDoc typedef must be a shallow type symbol"
        );
        let x_deps = state.type_deps("X").expect("X should exist");
        assert!(
            x_deps.local_deps.iter().any(|d| d == "Alias"),
            "a reference to a JSDoc typedef must classify as a LOCAL dep \
             (not be dropped); got local_deps = {:?}",
            x_deps.local_deps
        );
    }

    #[test]
    fn typeof_imports_are_recorded_as_external_deps() {
        let source = r#"
import { theme } from './theme'

export type Button = {
  slots: keyof typeof theme
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));
        let button = state.type_deps("Button").expect("Button should exist");

        assert!(
            button.external_deps.iter().any(|dep| {
                dep.local_name == "theme"
                    && dep.source_specifier == "./theme"
                    && dep.imported_name == "theme"
            }),
            "typeof import roots should be tracked as external deps: {:?}",
            button.external_deps
        );
    }

    #[test]
    fn direct_export_takes_precedence_over_wildcard_route() {
        let analysis = make_analysis(
            r#"
export { Foo } from './direct'
export * from './wildcard'
"#,
        );
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, None);

        // Foo should resolve through the direct reexport, not the wildcard
        match state.export_target("Foo") {
            Some(ExportTarget::Reexport {
                source_specifier, ..
            }) => {
                assert_eq!(
                    source_specifier, "./direct",
                    "direct reexport should take precedence"
                );
            }
            other => panic!("expected Reexport from ./direct, got {other:?}"),
        }

        // Wildcard sources should still be recorded
        assert!(
            state
                .wildcard_reexports
                .iter()
                .any(|w| w.source_specifier == "./wildcard"),
            "wildcard sources should be captured"
        );
    }

    #[test]
    fn import_target_lookup() {
        let analysis = make_analysis(
            r#"
import type { Alpha } from './a'
import type { Beta as B } from './b'
export interface Props extends Alpha { beta: B }
"#,
        );
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, None);

        assert!(state.is_import_local("Alpha"));
        let alpha_target = state.import_target("Alpha").unwrap();
        assert_eq!(alpha_target.source_specifier, "./a");
        assert_eq!(alpha_target.imported_name, "Alpha");

        assert!(state.is_import_local("B"));
        let b_target = state.import_target("B").unwrap();
        assert_eq!(b_target.source_specifier, "./b");
        assert_eq!(b_target.imported_name, "Beta");
    }

    #[test]
    fn local_export_alias_preserves_underlying_local_target() {
        let analysis = make_analysis(
            r#"
import Foo from './dep'
export { Foo as Bar }
"#,
        );
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, None);

        assert!(state.is_import_local("Foo"));
        assert!(
            !state.is_import_local("Bar"),
            "only the local import should be marked as import-local"
        );
        assert!(
            state.export_target("Foo").is_none(),
            "the local name should not be exported once it is aliased"
        );

        match state.export_target("Bar") {
            Some(ExportTarget::Local { symbol_name }) => {
                assert_eq!(
                    symbol_name, "Foo",
                    "aliased local exports should keep the underlying local target"
                );
            }
            other => panic!("expected Local export for Bar, got {other:?}"),
        }
    }

    #[test]
    fn default_exported_class_is_published_as_default() {
        let analysis = make_analysis(
            r#"
export default class Props {
  label!: string
}
"#,
        );
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, None);

        assert!(
            state.export_target("Props").is_none(),
            "named class identifier should not be published as a separate export for default-only classes"
        );

        match state.export_target("default") {
            Some(ExportTarget::Local { symbol_name }) => {
                assert_eq!(
                    symbol_name, "default",
                    "default-exported classes should be published under the default export name"
                );
            }
            other => panic!("expected Local export for default, got {other:?}"),
        }
    }

    #[test]
    fn eval_env_construction_populates_value_symbols() {
        let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
export function makeProps(): Props { return defaults }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let defaults = state
            .value_symbol("defaults")
            .expect("defaults value symbol should be present");
        assert_eq!(defaults.kind, ValueDeclKind::Const);
        let defaults_body = state
            .value_decl("defaults")
            .expect("defaults value body should lower on demand");
        assert!(defaults_body.type_annotation.is_some());
        assert!(defaults_body.object_shape.is_some());

        let make_props = state
            .value_symbol("makeProps")
            .expect("makeProps value symbol should be present");
        assert_eq!(make_props.kind, ValueDeclKind::Function);
        let make_props_body = state
            .value_decl("makeProps")
            .expect("makeProps value body should lower on demand");
        assert!(!make_props_body.signatures.is_empty());
    }

    #[test]
    fn type_view_exposes_only_type_resolution_surface() {
        let source = r#"
import type { Shared } from './shared'
export interface Props extends Shared { label: string }
export const defaults: Props = { label: 'ok' }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));
        let view = state.type_view();

        assert!(view.export_target("Props").is_some());
        assert!(view.symbol("Props").is_some());
        assert!(view.wildcard_reexports().is_empty());
        assert!(
            state.value_symbol("defaults").is_some(),
            "value symbols remain on the broad file state"
        );
    }

    #[test]
    fn exported_value_declarations_are_published_in_export_targets() {
        let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
        let analysis = make_analysis(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, None);

        match state.export_target("defaults") {
            Some(ExportTarget::Local { symbol_name }) => assert_eq!(symbol_name, "defaults"),
            other => panic!("expected Local export for defaults, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_interface_declarations_merge_members() {
        // Two same-name `interface Props` declarations in one file merge their
        // members (TS same-file declaration merging). The shallow symbol must
        // carry BOTH contributors' members — last-wins (dropping `x`) is the
        // bug this discriminates against.
        let source = r#"
export interface Props { x: string }
export interface Props { y: number }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let symbol = state.symbol("Props").expect("Props symbol should exist");
        assert_eq!(
            symbol.kind,
            TypeDeclKind::Interface,
            "symbol should keep Interface kind"
        );

        let body = state.type_decl("Props").expect("Props body should lower");
        assert!(
            body.body.is_merged(),
            "two `interface Props` declarations must produce a Merged body"
        );
        let members = body.body.merged_member_names();
        assert!(
            members.contains(&"x".to_string()),
            "merged Props must expose `x`; got {members:?}"
        );
        assert!(
            members.contains(&"y".to_string()),
            "merged Props must expose `y`; got {members:?}"
        );
    }

    #[test]
    fn name_resolution_populated_for_type_decls_with_deps() {
        let source = r#"
import type { Inner } from "./inner"
type Local = { x: number }
export interface Props { child: Inner; data: Local }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let dep_edges = {
            let mut edges = FxHashMap::default();
            edges.insert("./inner".to_string(), "/resolved/inner.ts".to_string());
            edges
        };
        let prepared = super::super::prepared_decl::prepare_exported_type_decl(
            "/src/types.ts",
            &state,
            "Props",
            Some(&dep_edges),
        )
        .expect("Props should prepare");

        // Local dep should resolve to same file
        assert_eq!(
            prepared
                .name_resolution
                .get("Local")
                .map(|r| &r.canonical_id),
            Some(&"/src/types.ts".to_string()),
            "local dep should resolve to same file"
        );
        // External dep should resolve through dep_edges
        assert_eq!(
            prepared
                .name_resolution
                .get("Inner")
                .map(|r| &r.canonical_id),
            Some(&"/resolved/inner.ts".to_string()),
            "external dep should resolve through dep_edges"
        );
    }

    // -----------------------------------------------------------------------
    // Workstream C: route-aware closure tests
    // -----------------------------------------------------------------------

    #[test]
    fn member_deps_populated_for_interface_with_typed_members() {
        let source = r#"
import type { AvatarProps } from './avatar'
import type { ButtonHTMLAttributes } from 'vue'

export interface CheckboxProps extends ButtonHTMLAttributes {
  ui?: AppConfig
  color?: string
  indicator?: AvatarProps
}

type AppConfig = { theme: string }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let sym = state.type_decl("CheckboxProps").expect("CheckboxProps");

        // member_deps should exist for 'ui', 'indicator', but not 'color' (primitive)
        assert!(
            sym.member_deps.contains_key("ui"),
            "ui should have member deps, member_deps: {:?}",
            sym.member_deps
        );
        assert!(
            sym.member_deps.contains_key("indicator"),
            "indicator should have member deps"
        );
        // 'color' is just 'string' — no refs
        assert!(
            !sym.member_deps.contains_key("color"),
            "color (primitive string) should have no deps"
        );

        // Verify 'ui' deps reference AppConfig
        let ui_deps = &sym.member_deps["ui"];
        assert!(
            ui_deps.contains(&"AppConfig".to_string()),
            "ui deps should reference AppConfig, got {:?}",
            ui_deps
        );
    }

    #[test]
    fn route_closure_member_narrows_to_member_deps_only() {
        let source = r#"
import type { Alpha } from './alpha'
import type { Beta } from './beta'

export interface Props {
  a: Alpha
  b: Beta
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        // Route::Member("a") should only include Alpha deps, not Beta
        let closure_a =
            state.route_closure("Props", &RouteDemand::MemberPath(vec!["a".into()]), 500);
        let ext_names: Vec<&str> = closure_a
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert!(
            ext_names.contains(&"Alpha"),
            "Member('a') should include Alpha, got {:?}",
            ext_names
        );
        assert!(
            !ext_names.contains(&"Beta"),
            "Member('a') should NOT include Beta"
        );

        // Route::Whole should keep direct object prop refs symbolic
        let closure_whole = state.route_closure("Props", &RouteDemand::Whole, 500);
        let ext_names_whole: Vec<&str> = closure_whole
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert!(
            ext_names_whole.is_empty(),
            "Whole route should keep direct imported object prop refs symbolic, got {:?}",
            ext_names_whole
        );
    }

    #[test]
    fn route_closure_whole_keeps_leaf_object_prop_imports_symbolic() {
        let source = r#"
import type { AvatarProps } from './avatar'
import type { IconProps } from './icon'

export interface Props {
  icon?: IconProps['name']
  avatar?: AvatarProps
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let closure = state.route_closure("Props", &RouteDemand::Whole, 500);
        assert_eq!(
            closure
                .unresolved_external
                .iter()
                .map(|e| e.imported_name.as_str())
                .collect::<Vec<_>>(),
            vec!["IconProps"],
            "Whole route should keep direct imported object props symbolic while still following actionable member routes, got {:?}",
            closure.unresolved_external
        );
        assert_eq!(
            closure.unresolved_external[0].route,
            RouteDemand::MemberPath(vec!["name".into()]),
            "whole-route imported closure should preserve the member tail on the external route"
        );
    }

    #[test]
    fn route_closure_pick_narrows_to_subset() {
        let source = r#"
import type { A } from './a'
import type { B } from './b'
import type { C } from './c'

export interface Props {
  x: A
  y: B
  z: C
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        // Pick(['x', 'z']) should include A and C but not B
        let closure = state.route_closure(
            "Props",
            &RouteDemand::Pick(vec!["x".into(), "z".into()]),
            500,
        );
        let ext_names: Vec<&str> = closure
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert!(ext_names.contains(&"A"), "Pick(['x','z']) should include A");
        assert!(ext_names.contains(&"C"), "Pick(['x','z']) should include C");
        assert!(
            !ext_names.contains(&"B"),
            "Pick(['x','z']) should NOT include B"
        );
    }

    #[test]
    fn route_closure_omit_narrows_to_remaining_members() {
        let source = r#"
import type { A } from './a'
import type { B } from './b'
import type { C } from './c'

export interface Props {
  x: A
  y: B
  z: C
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let closure = state.route_closure("Props", &RouteDemand::Omit(vec!["y".into()]), 500);
        let ext_names: Vec<&str> = closure
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert!(ext_names.contains(&"A"));
        assert!(ext_names.contains(&"C"));
        assert!(!ext_names.contains(&"B"));
    }

    #[test]
    fn route_closure_member_with_primitive_property_stays_minimal() {
        let source = r#"
import type { Alpha } from './alpha'

export interface Props {
  a: Alpha
  color: string
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let closure =
            state.route_closure("Props", &RouteDemand::MemberPath(vec!["color".into()]), 500);
        assert!(closure.unresolved_external.is_empty());
        assert_eq!(closure.local_symbols_used, vec!["Props".to_string()]);
    }

    #[test]
    fn route_closure_nested_member_path_follows_full_depth() {
        let source = r#"
import type { Alpha } from './alpha'
import type { Beta } from './beta'

type Variants = {
  color: Alpha
  size: Beta
}

export interface Props {
  variants: Variants
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let closure = state.route_closure(
            "Props",
            &RouteDemand::MemberPath(vec!["variants".into(), "color".into()]),
            500,
        );
        let ext_names: Vec<&str> = closure
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert!(
            ext_names.contains(&"Alpha"),
            "nested member path should include Alpha, got {:?}",
            ext_names
        );
        assert!(
            !ext_names.contains(&"Beta"),
            "nested member path should not widen to sibling nested members, got {:?}",
            ext_names
        );
    }

    #[test]
    fn route_closure_nested_member_path_carries_tail_into_imported_type() {
        let source = r#"
import type { Alpha } from './alpha'
import type { Beta } from './beta'

export interface Props {
  primary: Alpha
  secondary: Beta
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let closure = state.route_closure(
            "Props",
            &RouteDemand::MemberPath(vec!["primary".into(), "label".into()]),
            500,
        );
        let ext_names: Vec<&str> = closure
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert_eq!(
            ext_names,
            vec!["Alpha"],
            "nested member path should cross into the imported type without widening, got {:?}",
            ext_names
        );
        assert_eq!(
            closure.unresolved_external[0].route,
            RouteDemand::MemberPath(vec!["label".into()]),
            "imported companion should keep only the remaining member path"
        );
    }

    #[test]
    fn route_closure_nested_member_path_miss_stays_bounded() {
        let source = r#"
import type { Alpha } from './alpha'
import type { Beta } from './beta'

type Variants = {
  color: Alpha
  size: Beta
}

export interface Props {
  variants: Variants
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let closure = state.route_closure(
            "Props",
            &RouteDemand::MemberPath(vec!["variants".into(), "missing".into()]),
            500,
        );
        let ext_names: Vec<&str> = closure
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert!(
            ext_names.is_empty(),
            "nested member miss should stay bounded instead of widening, got {:?}",
            ext_names
        );
        assert_eq!(
            closure.local_symbols_used,
            vec!["Props".to_string()],
            "nested member miss should not widen into the nested object route"
        );
    }

    #[test]
    fn canonical_edges_populated_by_resolver() {
        use crate::resolver_core::ShallowImportResolver;

        struct TestResolver;

        impl ShallowImportResolver for TestResolver {
            fn resolve_canonical(&self, specifier: &str) -> Option<String> {
                match specifier {
                    "./bar" => Some("/resolved/bar.ts".to_string()),
                    "./types" => Some("/resolved/types.ts".to_string()),
                    _ => None,
                }
            }
        }

        let source = r#"
import type { Foo } from './bar'
export { Foo } from './bar'
export * from './types'
export interface Props { child: Foo }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis_with_resolver_seeded(
            Hash16::default(),
            analysis,
            Some(&env),
            &TestResolver,
        );

        // Wildcard reexport should have the resolved canonical ID
        assert_eq!(
            state.wildcard_reexports.len(),
            1,
            "should have exactly one wildcard reexport"
        );
        assert_eq!(
            state.wildcard_reexports[0].canonical_id, "/resolved/types.ts",
            "wildcard reexport canonical ID should be resolved"
        );
        assert_eq!(
            state.wildcard_reexports[0].source_specifier, "./types",
            "wildcard reexport source specifier should be preserved"
        );

        // Reexport target should carry the resolved canonical ID
        match state.export_target("Foo") {
            Some(ExportTarget::Reexport {
                canonical_id,
                original_name,
                source_specifier,
                ..
            }) => {
                assert_eq!(
                    canonical_id, "/resolved/bar.ts",
                    "reexport canonical ID should be resolved"
                );
                assert_eq!(original_name, "Foo");
                assert_eq!(source_specifier, "./bar");
            }
            other => panic!("expected Reexport for Foo, got {other:?}"),
        }

        // Import target should carry the resolved canonical ID
        let foo_target = state
            .import_target("Foo")
            .expect("Foo import target should exist");
        assert_eq!(
            foo_target.canonical_id, "/resolved/bar.ts",
            "import target canonical ID should be resolved"
        );
        assert_eq!(foo_target.source_specifier, "./bar");
        assert_eq!(foo_target.imported_name, "Foo");

        // External symbol refs on Props should carry the resolved canonical ID
        let props_deps = state.type_deps("Props").expect("Props symbol should exist");
        let foo_ext = props_deps
            .external_deps
            .iter()
            .find(|dep| dep.local_name == "Foo")
            .expect("Props should have Foo as an external dep");
        assert_eq!(
            foo_ext.canonical_id.as_deref(),
            Some("/resolved/bar.ts"),
            "external symbol ref canonical ID should be resolved"
        );
        assert_eq!(foo_ext.imported_name, "Foo");
        assert_eq!(foo_ext.source_specifier, "./bar");

        // Negative: no unresolved canonical IDs (empty strings) on known specifiers
        for wc in &state.wildcard_reexports {
            assert!(
                !wc.canonical_id.is_empty(),
                "wildcard reexport canonical ID should not be empty"
            );
        }
        for (name, target) in &state.import_targets {
            if target.source_specifier == "./bar" || target.source_specifier == "./types" {
                assert!(
                    !target.canonical_id.is_empty(),
                    "import target {name} canonical ID should not be empty"
                );
            }
        }
    }

    #[test]
    fn route_closure_missing_or_inherited_member_falls_back_to_whole() {
        let source = r#"
import type { Alpha } from './alpha'

interface Base {
  inherited: Alpha
}

export interface Props extends Base {
  own: string
}
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let closure = state.route_closure(
            "Props",
            &RouteDemand::MemberPath(vec!["inherited".into()]),
            500,
        );
        let ext_names: Vec<&str> = closure
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert!(
            ext_names.contains(&"Alpha"),
            "missing direct metadata should conservatively fall back to whole closure"
        );
    }
}
