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

use std::collections::BTreeSet;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::route_demand::RouteDemand;
use crate::decl_body_memo::{DemandOutcome, LoweredTypeDecl, LoweredValueDecl};
use verter_parser::utils::oxc::script::route_inventory::{
    RouteCapability, RouteImportForm, RouteImportedName, ScriptRouteInventory,
};
use verter_semantic::analysis::decl_headers::{TypeDeclHeader, ValueDeclHeader};
use verter_semantic::analysis::type_eval::{TypeDeclKind, ValueDeclKind};
use verter_semantic::analysis::Hash16;
use verter_span::Span;
use verter_type_expr::facts::TypeDependencyPathFact;
use verter_type_expr::{DeclKey, TopLevelOwnerId, TypeExpr};

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

    /// Authoritative owner-qualified import table. The public string-keyed
    /// table above is the ordinary-file compatibility projection only.
    pub(crate) owner_import_targets: FxHashMap<DeclKey, ImportTarget>,

    /// Parser-authored import/export route inventory from the retained
    /// program. Declaration headers and bodies remain owned by
    /// [`Self::decl_bodies`]; this carrier contains no semantic analysis.
    pub route_inventory: Arc<ScriptRouteInventory>,

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
    type_deps_cache: dashmap::DashMap<DeclKey, Option<Arc<ClassifiedTypeDeps>>>,

    /// EAGER synthesised value-symbol HEADERS (the `.vue` implicit
    /// `default` public-instance shape) — header-only records carrying
    /// the `is_synthesised_component_default` provenance flag. The matching
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

    /// Lazy per-state memo of the route-surface digest
    /// (`crate::resolver_store::hash_route_surface`). The digest is a pure
    /// function of this state's routing surface (`exports`,
    /// `wildcard_reexports`, `import_targets`, `whole_hash`), which is
    /// mutated only during construction — strictly before the state is
    /// `Arc`-published and first hashed — so one computation serves every
    /// later read. See [`RouteSurfaceHashMemo`] for the clone semantics.
    route_surface_hash: RouteSurfaceHashMemo,
}

/// One-shot memo cell for a [`ShallowFileState`]'s route-surface digest.
///
/// Thin wrapper over [`std::sync::OnceLock`] (the state is shared via
/// `Arc` across threads) whose `Clone` RESETS to an empty cell instead of
/// carrying the source's cached digest: a cloned state is exactly the
/// shape that may still be mutated (the routing fields are `pub`, and the
/// synthesised-`default` injection takes `&mut self`), so the clone must
/// re-digest its OWN surface on first demand rather than serve a stale
/// copy. Deliberately excluded from any equality/serialization semantics
/// — the enclosing struct derives only `Debug` + `Clone`.
#[derive(Debug, Default)]
pub(crate) struct RouteSurfaceHashMemo(std::sync::OnceLock<Hash16>);

impl RouteSurfaceHashMemo {
    /// The memoized digest, computing (and caching) it on first demand.
    /// Concurrent first demands may both run `compute`; the winning value
    /// is served to both — `compute` is deterministic over the immutable
    /// surface, so either result is the same digest.
    pub(crate) fn get_or_init(&self, compute: impl FnOnce() -> Hash16) -> Hash16 {
        *self.0.get_or_init(compute)
    }

    /// Test observability: the currently cached digest, `None` while
    /// unpopulated.
    #[cfg(test)]
    pub(crate) fn get(&self) -> Option<Hash16> {
        self.0.get().copied()
    }
}

impl Clone for RouteSurfaceHashMemo {
    fn clone(&self) -> Self {
        Self(std::sync::OnceLock::new())
    }
}

/// A wildcard `export * from ‘...’` reexport with its resolved canonical target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WildcardReexport {
    pub owner: TopLevelOwnerId,
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
    /// Whether the local binding is a namespace import (`import * as NS`).
    pub is_namespace: bool,
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
    Local {
        owner: TopLevelOwnerId,
        symbol_name: String,
    },
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
    /// Full declaration span of the last source-order contributor.
    pub span: Span,
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
            span: header.span,
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
    /// Same-file runtime values reached through a type query (`typeof seed`).
    /// These are not type-closure hops: consumers that omit the owning body
    /// must provide a declaration-safe value carrier or reject the projection.
    pub owner_value_deps: Vec<String>,
    /// Same-file dual-space roots reached in a runtime-value role. Their
    /// exact declaration contributors can satisfy body-omitting output.
    pub retained_value_carrier_deps: Vec<String>,
    /// Names of import-local symbols this type directly depends on.
    /// These become `ExternalSymbolRef` during frontier traversal.
    pub external_deps: Vec<ExternalSymbolRef>,
    /// TSC declaration-carrier closure. Kept separate so component-meta keeps
    /// its historical FULL/STRUCTURAL breadth.
    pub declaration_local_deps: Vec<String>,
    pub declaration_external_deps: Vec<ExternalSymbolRef>,
    /// Bare namespace roots cannot identify an exported declaration carrier.
    pub unroutable_declaration_dependencies: Vec<String>,
    pub has_unroutable_value_position: bool,
    /// Import-local roots reached through `typeof` queries.
    pub external_value_queries: Vec<String>,
    /// Import-local roots required in a runtime value position by declaration
    /// syntax, currently class `extends` heritage.
    pub external_value_positions: Vec<String>,
}

/// Classification of an arbitrary parser-authored dependency-path set against
/// this file's import table and local header inventory.
#[derive(Debug, Clone, Default)]
pub(crate) struct ClassifiedDependencyPaths {
    pub(crate) local_deps: Vec<String>,
    pub(crate) external_deps: Vec<ExternalSymbolRef>,
    pub(crate) unroutable_imports: Vec<String>,
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
    pub is_synthesised_component_default: bool,
}

impl ShallowValueSymbol {
    /// Build the slim header view from a shallow value-declaration header.
    /// `is_synthesised_component_default` is `false` for every header-index
    /// (userland) value symbol.
    fn from_header(header: &ValueDeclHeader) -> Self {
        Self {
            kind: header.kind,
            object_member_headers: header
                .object_member_headers
                .iter()
                .map(|m| m.name.clone())
                .collect(),
            is_synthesised_component_default: false,
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
            is_synthesised_component_default: true,
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
#[cfg(any(test, feature = "test-support"))]
struct NullResolver;

#[cfg(any(test, feature = "test-support"))]
impl ShallowImportResolver for NullResolver {
    fn resolve_canonical(&self, _specifier: &str) -> Option<String> {
        None
    }
}

impl ShallowFileState {
    /// Test-only HEADER/ROUTING-ONLY constructor: build the routing tables
    /// (exports / imports / wildcard reexports) from an existing route
    /// inventory with a null resolver — canonical IDs on
    /// cross-file edges stay empty. The declaration-body memo is EMPTY and
    /// serviceless: NO symbol inventory, NO bodies, and any body demand is a
    /// genuine miss — callers must provably never demand a declaration body
    /// (body-demanding fixtures use [`Self::service_backed_for_test`], the
    /// production lazy shape). Gated `#[cfg(any(test, feature = "test-support"))]` —
    /// integration tests in `tests/` compile without `cfg(test)`; release
    /// production builds compile this edge-less constructor out.
    #[cfg(any(test, feature = "test-support"))]
    pub fn header_routing_only_for_test(
        whole_hash: Hash16,
        route_inventory: Arc<ScriptRouteInventory>,
    ) -> Self {
        let memo = Self::empty_header_only_memo(whole_hash);
        Self::from_route_inventory_with_memo(whole_hash, route_inventory, memo, &NullResolver)
    }

    /// [`Self::header_routing_only_for_test`] resolving cross-file edges
    /// through the supplied resolver (canonical IDs populated on the
    /// reexport / import / wildcard edges). Same header-only contract:
    /// EMPTY serviceless memo, no bodies, callers provably never demand
    /// a declaration body.
    #[cfg(any(test, feature = "test-support"))]
    pub fn header_routing_only_with_resolver_for_test(
        whole_hash: Hash16,
        route_inventory: Arc<ScriptRouteInventory>,
        resolver: &dyn ShallowImportResolver,
    ) -> Self {
        let memo = Self::empty_header_only_memo(whole_hash);
        Self::from_route_inventory_with_memo(whole_hash, route_inventory, memo, resolver)
    }

    /// The EMPTY serviceless memo backing the header/routing-only test
    /// constructors: no symbol inventory, no bodies, every body demand a
    /// genuine miss. Same gate as [`Self::header_routing_only_for_test`].
    #[cfg(any(test, feature = "test-support"))]
    fn empty_header_only_memo(whole_hash: Hash16) -> Arc<crate::decl_body_memo::DeclBodyMemo> {
        let env = verter_semantic::analysis::type_eval::EvalEnv::default();
        let header_index =
            Arc::new(verter_semantic::analysis::decl_headers::DeclHeaderIndex::from_eval_env(&env));
        Arc::new(crate::decl_body_memo::DeclBodyMemo::seeded_from_env(
            crate::decl_lowering::SnapshotKey {
                canonical: Arc::from(""),
                whole_hash,
                parse_env_hash: [0u8; 16],
            },
            &env,
            header_index,
        ))
    }

    /// Test-only builder for a SERVICE-backed [`ShallowFileState`] — the
    /// production lazy-memo shape: a live
    /// [`crate::decl_lowering::DeclLoweringService`] retains the parse
    /// snapshot, construction lowers ZERO declaration bodies, and every
    /// declaration body materialises on first demand through the memo
    /// exactly as production (the shared lens pair installs from the
    /// finished state). Broken-lease no-warm regressions break the lease
    /// out-of-band via
    /// [`crate::decl_body_memo::DeclBodyMemo::release_retained_snapshot_for_test`].
    /// Gated `#[cfg(any(test, feature = "test-support"))]` — integration tests in
    /// `tests/` compile without `cfg(test)`; release production builds
    /// compile this out.
    #[cfg(any(test, feature = "test-support"))]
    pub fn service_backed_for_test(source: &str) -> Arc<Self> {
        Self::service_backed_for_test_at("/ws/fixture.ts", source)
    }

    /// [`Self::service_backed_for_test`] with a caller-chosen canonical id —
    /// for fixtures whose locators/routes anchor on a specific canonical
    /// (augmentation scopes, multi-file stitches).
    #[cfg(any(test, feature = "test-support"))]
    pub fn service_backed_for_test_at(canonical: &str, source: &str) -> Arc<Self> {
        Self::service_backed_with_provenance_for_test(canonical, source).0
    }

    /// [`Self::service_backed_for_test_at`] additionally handing back the
    /// memo's [`crate::types::MetaProvenance`] counters so lazy-demand
    /// tests can assert HOW MANY bodies lowered / programs parsed.
    #[cfg(any(test, feature = "test-support"))]
    pub fn service_backed_with_provenance_for_test(
        canonical: &str,
        source: &str,
    ) -> (Arc<Self>, Arc<crate::types::MetaProvenance>) {
        Self::service_backed_with_provenance_and_resolver_for_test(canonical, source, &NullResolver)
    }

    /// [`Self::service_backed_for_test_at`] with a caller-supplied
    /// `whole_hash` — for artifact-identity fixtures that vary the content
    /// hash as a controlled invalidation axis. The hash keys BOTH the memo
    /// snapshot and the state, exactly like the content-derived default.
    #[cfg(any(test, feature = "test-support"))]
    pub fn service_backed_for_test_with_hash(
        canonical: &str,
        source: &str,
        whole_hash: Hash16,
    ) -> Arc<Self> {
        Self::service_backed_core_for_test(canonical, source, Some(whole_hash), &NullResolver, None)
            .0
    }

    /// [`Self::service_backed_for_test`] with an exact lexical owner for
    /// every parsed top-level statement.
    #[cfg(any(test, feature = "test-support"))]
    pub fn service_backed_for_test_with_statement_owners(
        canonical: &str,
        source: &str,
        statement_owners: &[TopLevelOwnerId],
    ) -> Arc<Self> {
        Self::service_backed_core_for_test(
            canonical,
            source,
            None,
            &NullResolver,
            Some(statement_owners),
        )
        .0
    }

    /// Full-parameter service-backed test builder: caller-chosen canonical,
    /// import resolver for cross-file canonical edges, and the provenance
    /// handle. `whole_hash` derives from the source content (production
    /// shape: same content ⇒ same hash, different content ⇒ different
    /// hash) and keys BOTH the memo snapshot and the state.
    #[cfg(any(test, feature = "test-support"))]
    pub fn service_backed_with_provenance_and_resolver_for_test(
        canonical: &str,
        source: &str,
        resolver: &dyn ShallowImportResolver,
    ) -> (Arc<Self>, Arc<crate::types::MetaProvenance>) {
        Self::service_backed_core_for_test(canonical, source, None, resolver, None)
    }

    /// The ONE service-backed construction core every `service_backed_*`
    /// test front delegates to.
    #[cfg(any(test, feature = "test-support"))]
    fn service_backed_core_for_test(
        canonical: &str,
        source: &str,
        whole_hash: Option<Hash16>,
        resolver: &dyn ShallowImportResolver,
        statement_owners: Option<&[TopLevelOwnerId]>,
    ) -> (Arc<Self>, Arc<crate::types::MetaProvenance>) {
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
        assert!(!parsed.panicked, "service-backed test fixture must parse");
        let owner_table = Arc::new(match statement_owners {
            Some(owners) => {
                verter_semantic::analysis::TopLevelOwnerTable::try_from_statement_owners(
                    parsed.program.body.len(),
                    owners.iter().copied(),
                )
                .expect("service-backed test owner table must cover every statement")
            }
            None => verter_semantic::analysis::TopLevelOwnerTable::ordinary_file(
                parsed.program.body.len(),
            ),
        });
        let shallow_index =
            verter_semantic::analysis::script_shallow_index::build_script_shallow_index_with_owners(
                &parsed.program,
                source,
                owner_table.as_ref(),
            )
            .expect("service-backed test owner table must cover every statement");
        let header_index = Arc::new(shallow_index.declaration_headers);
        let route_inventory = Arc::new(shallow_index.routes);
        let eval_source: Arc<str> = Arc::from(source);
        let whole_hash = whole_hash.unwrap_or_else(|| crate::hash::hash_16(source.as_bytes()));
        let provenance = Arc::new(crate::types::MetaProvenance::default());
        let memo = Arc::new(crate::decl_body_memo::DeclBodyMemo::new(
            crate::decl_lowering::SnapshotKey {
                canonical: Arc::from(canonical),
                whole_hash,
                parse_env_hash: [0u8; 16],
            },
            Arc::clone(&eval_source),
            None,
            oxc_span::SourceType::ts(),
            owner_table,
            false,
            Arc::new(crate::decl_lowering::DeclLoweringService::new()),
            header_index,
            Arc::clone(&provenance),
            None,
        ));
        (
            Arc::new(Self::from_route_inventory_with_resolver(
                whole_hash,
                route_inventory,
                memo,
                resolver,
            )),
            provenance,
        )
    }

    /// Test-only HEADER/ROUTING-ONLY constructor with caller-supplied
    /// ROUTING tables (exports, wildcard reexports, import tables) and an
    /// EMPTY symbol inventory — for fixtures that exercise route surfaces
    /// without any declared symbols and provably never demand a
    /// declaration body. Same gate as
    /// [`Self::header_routing_only_for_test`].
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::too_many_arguments)]
    pub fn routing_tables_only_for_test(
        whole_hash: Hash16,
        exports: FxHashMap<String, ExportTarget>,
        wildcard_reexports: Vec<WildcardReexport>,
        import_locals: FxHashSet<String>,
        import_targets: FxHashMap<String, ImportTarget>,
        route_inventory: Arc<ScriptRouteInventory>,
    ) -> Self {
        let memo = Self::empty_header_only_memo(whole_hash);
        let mut state = Self::assemble_from_route_inventory_with_memo(
            whole_hash,
            route_inventory,
            memo,
            &NullResolver,
        );
        state.exports = exports;
        state.wildcard_reexports = wildcard_reexports;
        state.import_locals = import_locals;
        state.import_targets = import_targets;
        state.owner_import_targets = state
            .import_targets
            .iter()
            .map(|(name, target)| {
                (
                    DeclKey::new(TopLevelOwnerId::ordinary_file(), name.as_str()),
                    target.clone(),
                )
            })
            .collect();
        // The lens installs AFTER the routing mutation above: the memo's
        // `install_shallow_lens` is OnceLock-first-wins, so installing
        // through `header_routing_only_for_test` first would pin a lens
        // over the
        // PRE-routing state forever (stale exports/imports feeding the
        // lowering-time fingerprint and the parse-fact emitter). Assemble
        // un-lensed, apply the caller's routing tables, then derive the ONE
        // shared lens from the FINAL routed state.
        state.install_shallow_lens_from_final_state();
        state
    }

    /// Build from the syntax-only route inventory + the lazy declaration-body
    /// memo, with a resolver that canonicalizes all cross-file edges.
    ///
    /// This is the production construction path: HEADER work only —
    /// export/import routing tables, no body lowering. Symbol bodies
    /// materialise on first demand through the memo.
    pub fn from_route_inventory_with_resolver(
        whole_hash: Hash16,
        route_inventory: Arc<ScriptRouteInventory>,
        decl_bodies: Arc<crate::decl_body_memo::DeclBodyMemo>,
        resolver: &dyn ShallowImportResolver,
    ) -> Self {
        Self::from_route_inventory_with_memo(whole_hash, route_inventory, decl_bodies, resolver)
    }

    /// Shared routing-table builder for every `ShallowFileState`
    /// constructor: it reads the ALREADY-extracted route inventory and
    /// the SUPPLIED lazy declaration-body `decl_bodies` memo, canonicalizes
    /// cross-file edges through `resolver`, and assembles the
    /// export/import/wildcard routing tables. It performs NO reparse, NO
    /// `parse_and_build_env`, and NO eval-env build — every body materialises
    /// later on demand through the supplied memo.
    fn from_route_inventory_with_memo(
        whole_hash: Hash16,
        route_inventory: Arc<ScriptRouteInventory>,
        decl_bodies: Arc<crate::decl_body_memo::DeclBodyMemo>,
        resolver: &dyn ShallowImportResolver,
    ) -> Self {
        let state = Self::assemble_from_route_inventory_with_memo(
            whole_hash,
            route_inventory,
            decl_bodies,
            resolver,
        );
        state.install_shallow_lens_from_final_state();
        state
    }

    /// Assemble the routing tables WITHOUT installing the memo's shared
    /// shallow lens. Every constructor finishes by calling
    /// [`Self::install_shallow_lens_from_final_state`] exactly once on its
    /// FINISHED state — [`Self::from_route_inventory_with_memo`] immediately after
    /// assembly, [`Self::routing_tables_only_for_test`] after its routing-table
    /// mutation — so the one shared lens always derives from the final
    /// routed state.
    fn assemble_from_route_inventory_with_memo(
        whole_hash: Hash16,
        route_inventory: Arc<ScriptRouteInventory>,
        decl_bodies: Arc<crate::decl_body_memo::DeclBodyMemo>,
        resolver: &dyn ShallowImportResolver,
    ) -> Self {
        // Capacity bounds are exact per-source counts (cheap header-inventory
        // walks, no allocation); `entry` collisions across the export sources
        // only ever shrink the final size.
        let ordinary_owner = TopLevelOwnerId::ordinary_file();
        let binding_count = route_inventory
            .imports
            .iter()
            .filter(|binding| binding.owner == ordinary_owner)
            .count();
        let export_bound = route_inventory.reexports.len()
            + route_inventory.local_exports.len()
            + route_inventory
                .wildcard_reexports
                .iter()
                .filter(|route| route.exported_namespace.is_some())
                .count();
        let mut exports = FxHashMap::with_capacity_and_hasher(export_bound, Default::default());
        let mut ambiguous_exports = FxHashSet::default();
        let mut wildcard_reexports = Vec::with_capacity(route_inventory.wildcard_reexports.len());
        let mut import_locals =
            FxHashSet::with_capacity_and_hasher(binding_count, Default::default());
        let mut import_targets: FxHashMap<String, ImportTarget> =
            FxHashMap::with_capacity_and_hasher(binding_count, Default::default());
        let mut owner_import_targets: FxHashMap<DeclKey, ImportTarget> =
            FxHashMap::with_capacity_and_hasher(route_inventory.imports.len(), Default::default());
        let mut ambiguous_imports = FxHashSet::default();

        let insert_export =
            |exported_name: String,
             route: ExportTarget,
             exports: &mut FxHashMap<String, ExportTarget>,
             ambiguous_exports: &mut FxHashSet<String>| {
                if ambiguous_exports.contains(&exported_name) {
                    return;
                }
                if exports
                    .get(&exported_name)
                    .is_some_and(|existing| existing == &route)
                {
                    return;
                }
                if exports.insert(exported_name.clone(), route).is_some() {
                    exports.remove(&exported_name);
                    ambiguous_exports.insert(exported_name);
                }
            };

        for target in &route_inventory.reexports {
            let canonical_id = resolver
                .resolve_canonical(&target.source)
                .unwrap_or_default();
            let is_type = matches!(target.capability, RouteCapability::TypeOnly)
                || resolver.is_type_reexport(&target.exported, &target.source);
            let route = ExportTarget::Reexport {
                source_specifier: target.source.clone(),
                original_name: target.imported.clone(),
                canonical_id,
                is_type,
            };
            insert_export(
                target.exported.clone(),
                route,
                &mut exports,
                &mut ambiguous_exports,
            );
        }

        for target in &route_inventory.local_exports {
            insert_export(
                target.exported.clone(),
                ExportTarget::Local {
                    owner: target.owner,
                    symbol_name: target.local.clone(),
                },
                &mut exports,
                &mut ambiguous_exports,
            );
        }

        // Wildcard reexport sources (in declaration order) with canonical targets
        for wildcard in &route_inventory.wildcard_reexports {
            let canonical_id = resolver
                .resolve_canonical(&wildcard.source)
                .unwrap_or_default();
            if let Some(exported_namespace) = &wildcard.exported_namespace {
                insert_export(
                    exported_namespace.clone(),
                    ExportTarget::Reexport {
                        source_specifier: wildcard.source.clone(),
                        original_name: exported_namespace.clone(),
                        canonical_id: canonical_id.clone(),
                        is_type: matches!(wildcard.capability, RouteCapability::TypeOnly),
                    },
                    &mut exports,
                    &mut ambiguous_exports,
                );
            }
            wildcard_reexports.push(WildcardReexport {
                owner: wildcard.owner,
                source_specifier: wildcard.source.clone(),
                canonical_id,
            });
        }

        // Import locals and targets
        for binding in &route_inventory.imports {
            let canonical_id = resolver
                .resolve_canonical(&binding.source)
                .unwrap_or_default();
            let target = ImportTarget {
                source_specifier: binding.source.clone(),
                imported_name: match &binding.imported {
                    RouteImportedName::Namespace => binding.local.clone(),
                    RouteImportedName::Name(name) => name.clone(),
                },
                is_namespace: matches!(binding.form, RouteImportForm::Namespace),
                canonical_id,
            };
            let key = DeclKey::new(binding.owner, binding.local.as_str());
            if ambiguous_imports.contains(&key) {
                continue;
            }
            if owner_import_targets
                .insert(key.clone(), target.clone())
                .is_some()
            {
                owner_import_targets.remove(&key);
                ambiguous_imports.insert(key);
                if binding.owner == ordinary_owner {
                    import_locals.remove(&binding.local);
                    import_targets.remove(&binding.local);
                }
                continue;
            }
            if binding.owner == ordinary_owner {
                let local_name = binding.local.clone();
                import_locals.insert(local_name.clone());
                import_targets.insert(local_name, target);
            }
        }

        let mut ordinary_export_assignments = route_inventory
            .export_assignments
            .iter()
            .filter(|assignment| assignment.owner == ordinary_owner);
        let export_assignment = ordinary_export_assignments
            .next()
            .filter(|_| ordinary_export_assignments.next().is_none())
            .map(|assignment| assignment.local.clone());

        Self {
            whole_hash,
            exports,
            wildcard_reexports,
            import_locals,
            export_assignment,
            import_targets,
            owner_import_targets,
            route_inventory,
            decl_bodies,
            type_deps_cache: dashmap::DashMap::default(),
            synthesised_value_symbols: FxHashMap::default(),
            synthesised_value_bodies: FxHashMap::default(),
            route_surface_hash: RouteSurfaceHashMemo::default(),
        }
    }

    /// Install the ONE shared shallow cross-decl lens on the memo. The lens
    /// derives from the FINISHED state (exports / import targets / symbol
    /// names), so every constructor calls this exactly once as its LAST
    /// construction step — strictly before any body demand can reach the
    /// memo's lowering-time fingerprint site or the parse-fact emitter.
    /// (`install_shallow_lens` is OnceLock-first-wins: the FIRST install is
    /// the one the memo serves forever, which is why assembly never
    /// installs early.)
    fn install_shallow_lens_from_final_state(&self) {
        self.decl_bodies.install_shallow_lens(Arc::new(
            crate::fact_emission::ShallowLens::from_shallow(self),
        ));
        self.decl_bodies.install_route_fact_lens(Arc::new(
            crate::fact_emission::RouteLens::from_shallow(self),
        ));
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

    /// The lazy route-surface digest memo
    /// ([`crate::resolver_store::hash_route_surface`] populates and reads
    /// it). Routing mutations are construction-time only, strictly before
    /// the first hash, so the populated digest never goes stale.
    pub(crate) fn route_surface_hash_memo(&self) -> &RouteSurfaceHashMemo {
        &self.route_surface_hash
    }

    /// Every file-scope TYPE symbol name in the shallow inventory
    /// (header-level — no body lowering).
    pub fn type_symbol_names(&self) -> impl Iterator<Item = &str> {
        self.decl_bodies
            .header_index()
            .type_headers
            .keys()
            .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
            .map(|key| key.name.as_ref())
    }

    /// Every file-scope VALUE symbol name in the shallow inventory,
    /// including eager synthesised symbols (header-level).
    pub fn value_symbol_names(&self) -> impl Iterator<Item = &str> {
        let headers = &self.decl_bodies.header_index().value_headers;
        headers
            .keys()
            .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
            .map(|key| key.name.as_ref())
            .chain(
                self.synthesised_value_symbols
                    .keys()
                    .map(String::as_str)
                    .filter(move |name| {
                        !headers
                            .contains_key(&DeclKey::new(TopLevelOwnerId::ordinary_file(), *name))
                    }),
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
        self.type_symbol_kind_in(TopLevelOwnerId::ordinary_file(), name)
    }

    /// Header-level kind of an exact owner-qualified TYPE symbol.
    pub(crate) fn type_symbol_kind_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::TypeDeclKind> {
        self.decl_bodies
            .header_index()
            .type_header_in(owner, name)
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
            .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
            .map(|key| key.name.as_ref())
    }

    /// Header-level ordered member (variant) names of an `enum`
    /// declaration, in source order. `None` when `name` is not an enum.
    pub fn enum_member_names(&self, name: &str) -> Option<&[String]> {
        self.decl_bodies
            .header_index()
            .enum_headers
            .get(&DeclKey::new(TopLevelOwnerId::ordinary_file(), name))
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
        self.has_type_symbol_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn has_type_symbol_in(&self, owner: TopLevelOwnerId, name: &str) -> bool {
        self.decl_bodies
            .header_index()
            .type_header_in(owner, name)
            .is_some()
    }

    /// Whether `name` is a file-scope VALUE symbol (header-level,
    /// including synthesised symbols).
    pub fn has_value_symbol(&self, name: &str) -> bool {
        self.has_value_symbol_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn has_value_symbol_in(&self, owner: TopLevelOwnerId, name: &str) -> bool {
        self.decl_bodies
            .header_index()
            .value_header_in(owner, name)
            .is_some()
            || (owner == TopLevelOwnerId::ordinary_file()
                && self.synthesised_value_symbols.contains_key(name))
    }

    /// Canonical one-way lexical parent for a carrier instance owner.
    ///
    /// The relation is derived exclusively from the validated owner table. An
    /// instance sees a sole module owner; module/frontmatter owners have no
    /// parent, and multiple module owners are ambiguous and fail closed.
    pub(crate) fn validated_lexical_parent_owner(
        &self,
        owner: TopLevelOwnerId,
    ) -> Option<TopLevelOwnerId> {
        (owner.kind() == verter_type_expr::TopLevelOwnerKind::Instance)
            .then(|| {
                self.decl_bodies
                    .owner_table()
                    .unique_owner_of_kind(verter_type_expr::TopLevelOwnerKind::Module)
            })
            .flatten()
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
            .flat_map(|(scope, names)| {
                names
                    .keys()
                    .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
                    .map(move |key| (scope, key.name.as_ref()))
            })
    }

    pub(crate) fn augmentation_type_decl_keys(
        &self,
    ) -> impl Iterator<
        Item = (
            &verter_semantic::analysis::type_eval::AugmentationScopeKind,
            &DeclKey,
        ),
    > {
        self.decl_bodies
            .header_index()
            .augmentation_type_headers
            .iter()
            .flat_map(|(scope, declarations)| declarations.keys().map(move |key| (scope, key)))
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
            .flat_map(|(scope, names)| {
                names
                    .keys()
                    .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
                    .map(move |key| (scope, key.name.as_ref()))
            })
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
    /// `IndexedReady.import_routes`; the retained route inventory is
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
            || !self.route_inventory.bindingless_imports.is_empty()
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

    /// Read the kind and span of an exact owner-qualified TYPE symbol without
    /// allocating a full [`ShallowTypeSymbol`] view.
    pub(crate) fn type_symbol_metadata_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<(TypeDeclKind, Span)> {
        self.decl_bodies
            .header_index()
            .type_header_in(owner, name)
            .map(|header| (header.kind, header.span))
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
        self.type_decl_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn type_decl_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<LoweredTypeDecl>> {
        self.decl_bodies.type_decl_in(owner, name)
    }

    pub(crate) fn type_decl_outcome_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> DemandOutcome<LoweredTypeDecl> {
        self.decl_bodies.type_decl_outcome_in(owner, name)
    }

    /// Demand the lowered BODY of a local VALUE symbol — eager
    /// synthesised `.vue`-default bodies first, then the lazy memo. A
    /// miss returns `None`.
    pub fn value_decl(&self, name: &str) -> Option<Arc<LoweredValueDecl>> {
        self.value_decl_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn value_decl_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<LoweredValueDecl>> {
        if owner == TopLevelOwnerId::ordinary_file() {
            if let Some(body) = self.synthesised_value_bodies.get(name) {
                return Some(Arc::clone(body));
            }
        }
        self.decl_bodies.value_decl_in(owner, name)
    }

    pub(crate) fn value_decl_outcome_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> DemandOutcome<LoweredValueDecl> {
        if owner == TopLevelOwnerId::ordinary_file() {
            if let Some(body) = self.synthesised_value_bodies.get(name) {
                return DemandOutcome::Ready(Some(Arc::clone(body)));
            }
        }
        self.decl_bodies.value_decl_outcome_in(owner, name)
    }

    /// The per-contributor body surface for a file-scope TYPE symbol, as the
    /// typeinfo/hover oracle's source walk consumes it — the single
    /// output/compat body read on the shallow-state side.
    ///
    /// This is deliberately NARROW and PURPOSE-NAMED (a typeinfo-oracle
    /// contributor read, not a general body accessor): it owns the one place the
    /// source walk reads a TYPE declaration body's per-contributor `TypeExpr`
    /// vector, so the body STORAGE can later change shape (a handle carrier) by
    /// reworking THIS helper's internals without preserving a broad `TypeExpr`
    /// body API. It returns `Some(contributors)` exactly as the inline
    /// `type_decl(name)?.body.contributors().to_vec()` read did (a header miss
    /// is `None`), so the oracle's admission verdict is byte-identical.
    ///
    /// TEMPORARY compat surface: it fences the typeinfo-oracle body read off
    /// from the semantic readers (which still read the typed body directly). It
    /// is anchored as a COMPAT site by the frozen body-reader inventory guard.
    ///
    /// `#[allow(dead_code)]`: genuinely dead in a default build. The sole reach
    /// is the typeinfo/hover oracle's source walk
    /// (`oracle_core::source_walk::walk`), and the WHOLE `oracle_core` module is
    /// gated `#[cfg(any(test, feature = "oracle-gen"))]` (`typeinfo/mod.rs`) with
    /// `oracle-gen` NOT a default feature (`Cargo.toml: default = []`), so the
    /// default resolver build compiles this helper out entirely. Its outermost
    /// callers are the `#[oracle_row]`-lifted `#[test]` rows (the proc-macro
    /// expands to `oracle::run_row(file!(), "<fn>")` — `run_row` itself is the
    /// one-hop-up `#[cfg(test)] pub(crate)` dispatcher) and the `oracle-gen`-only
    /// generator binary (`src/bin/oracle_gen`, `required-features =
    /// ["oracle-gen"]`) — the same guard/generator-only reach the sibling oracle
    /// helpers in `oracle_core::admission` carry the allow for. The compat
    /// routing did NOT change reachability: the previous inline read was equally
    /// cfg-gated but went through the `pub` `type_decl`, which never surfaced
    /// this latent visibility fact.
    #[allow(dead_code)]
    pub(crate) fn compat_type_contributors_for_typeinfo(
        &self,
        name: &str,
    ) -> Option<Vec<TypeExpr>> {
        // The record stores content-free contributor LOCATORS; the oracle's
        // per-contributor `TypeExpr` view is re-borrowed lease-only from the
        // retained snapshot (the internals rework this compat helper's
        // contract anticipated). A header miss stays `None`; a body-less
        // re-borrow (broken lease / seeded state) is a conservative `None`.
        self.type_decl(name)?;
        match self.decl_bodies.transient_type_bodies(name) {
            crate::decl_body_memo::DemandOutcome::Ready(Some(bodies)) => {
                Some(bodies.as_ref().clone())
            }
            _ => None,
        }
    }

    /// The CENTRALIZED effective VALUE-symbol lookup — the single authority
    /// applying ambient-overlay precedence so per-symbol graph-native readers
    /// never special-case any ambient surface:
    ///
    /// 1. A user/synthesized declaration WINS ([`Self::value_decl`]).
    /// 2. If absent AND the file is a Svelte rune module, the symbol resolves
    ///    to the centralized rune ambient inventory (`$state`/`$derived`/
    ///    `$effect`/`$inspect`).
    /// 3. Otherwise it is a miss.
    ///
    /// Rune visibility roots on the rune module's own content-addressed
    /// identity: the prelude version is folded into the file's `parse_env_hash`
    /// (via the workspace parser flag), so a prelude-surface change invalidates
    /// the file's whole cache lineage — the synthetic ambient root is observed
    /// transitively through the rune module's own facts, never a fail-closed
    /// synthetic canonical.
    pub fn effective_value_decl(&self, name: &str) -> Option<Arc<LoweredValueDecl>> {
        self.effective_value_decl_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn effective_value_decl_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<LoweredValueDecl>> {
        if let Some(decl) = self.value_decl_in(owner, name) {
            return Some(decl);
        }
        if owner == TopLevelOwnerId::ordinary_file() && self.decl_bodies.rune_ambient_visible() {
            return crate::host_resolve::rune_ambient_value_decl(name);
        }
        None
    }

    /// VALUE-symbol PRESENCE under the centralized effective lookup — header
    /// presence first (no body materialisation), then the rune ambient
    /// inventory for a rune module. Mirrors [`Self::effective_value_decl`]'s
    /// precedence without lowering a body.
    pub fn effective_value_header_present(&self, name: &str) -> bool {
        self.effective_value_header_present_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn effective_value_header_present_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> bool {
        if self.has_value_symbol_in(owner, name) {
            return true;
        }
        owner == TopLevelOwnerId::ordinary_file()
            && self.decl_bodies.rune_ambient_visible()
            && crate::host_resolve::rune_ambient_has_value(name)
    }

    /// TYPE-space counterpart of [`Self::effective_value_decl`]: a user
    /// declaration wins, else the rune ambient inventory's TYPE symbols (the
    /// rune namespace types) for a rune module, else a miss.
    pub fn effective_type_decl(&self, name: &str) -> Option<Arc<LoweredTypeDecl>> {
        self.effective_type_decl_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn effective_type_decl_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<LoweredTypeDecl>> {
        if let Some(decl) = self.type_decl_in(owner, name) {
            return Some(decl);
        }
        if owner == TopLevelOwnerId::ordinary_file() && self.decl_bodies.rune_ambient_visible() {
            return crate::host_resolve::rune_ambient_type_decl(name);
        }
        None
    }

    /// TYPE-symbol PRESENCE under the centralized effective lookup.
    pub fn effective_type_header_present(&self, name: &str) -> bool {
        self.effective_type_header_present_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn effective_type_header_present_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> bool {
        if self.has_type_symbol_in(owner, name) {
            return true;
        }
        owner == TopLevelOwnerId::ordinary_file()
            && self.decl_bodies.rune_ambient_visible()
            && crate::host_resolve::rune_ambient_has_type(name)
    }

    /// Demand the dependency-edge classification of a local TYPE symbol —
    /// the local/external split over its reference graph, baked against
    /// THIS state's import targets and cached per name. Returns `Some`
    /// for any header type symbol (possibly with empty edge lists); a
    /// header miss returns `None` without lowering or caching anything.
    pub fn type_deps(&self, name: &str) -> Option<Arc<ClassifiedTypeDeps>> {
        self.type_deps_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn type_deps_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<ClassifiedTypeDeps>> {
        self.decl_bodies
            .header_index()
            .type_header_in(owner, name)?;
        let key = DeclKey::new(owner, name);
        if let Some(hit) = self.type_deps_cache.get(&key) {
            return hit.clone();
        }
        // A header-present symbol classifies to a genuine `Ready` under a live
        // lease; a `LeaseMiss` here is a BROKEN decl-body lease pin (the
        // demanded body lowering ReturnOnly'd). Fail CLOSED: do NOT cache the
        // transient result as genuine absence — a cached wrong-empty would
        // under-classify the symbol's dependency edges for the artifact's life
        // (under-invalidation). A later demand under a live lease recovers.
        match self.classify_type_deps_in(owner, name) {
            DemandOutcome::LeaseMiss => {
                // Broken decl-body lease pin: mark the generalized
                // non-cacheability rail so an enclosing traced compute refuses
                // shared-cache admission (this accessor collapses the
                // `DemandOutcome` directly, bypassing `into_option`), and fail
                // closed — never cache the transient empty classification.
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
                );
                None
            }
            DemandOutcome::Ready(computed) => {
                self.type_deps_cache.insert(key, computed.clone());
                computed
            }
        }
    }

    /// Test observability: whether the `type_deps` classification cache holds a
    /// COMMITTED `None` entry for `name`. A `None` cached for a header-present
    /// symbol is a wrong-empty warm admission (a broken decl-body lease pin that
    /// should have failed closed) — never a validity signal.
    #[cfg(test)]
    pub(crate) fn type_deps_cache_has_none_entry(&self, name: &str) -> bool {
        self.type_deps_cache
            .get(&DeclKey::new(TopLevelOwnerId::ordinary_file(), name))
            .is_some_and(|entry| entry.is_none())
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
        self.augmentation_type_decl_in(scope, TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn augmentation_type_decl_in(
        &self,
        scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<LoweredTypeDecl>> {
        self.decl_bodies
            .header_index()
            .augmentation_type_header_in(scope, owner, name)?;
        self.decl_bodies
            .augmentation_type_decl_in(scope, owner, name)
    }

    pub(crate) fn augmentation_type_decl_outcome_in(
        &self,
        scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> DemandOutcome<LoweredTypeDecl> {
        if self
            .decl_bodies
            .header_index()
            .augmentation_type_header_in(scope, owner, name)
            .is_none()
        {
            return DemandOutcome::Ready(None);
        }
        self.decl_bodies
            .augmentation_type_decl_outcome_in(scope, owner, name)
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

    pub(crate) fn classify_dependency_paths(
        &self,
        declaration_owner: TopLevelOwnerId,
        declaration_name: &str,
        paths: &FxHashSet<TypeDependencyPathFact>,
    ) -> ClassifiedDependencyPaths {
        let mut local = FxHashSet::default();
        let mut external = FxHashSet::default();
        let mut unroutable = FxHashSet::default();

        for path in paths {
            let root = path.root();
            if let Some(target) = self
                .owner_import_targets
                .get(&DeclKey::new(declaration_owner, root))
            {
                let (imported_name, member_path) = if target.is_namespace {
                    let Some((exported_name, member_path)) = path.member_path().split_first()
                    else {
                        unroutable.insert(root.to_string());
                        continue;
                    };
                    (exported_name.clone(), member_path)
                } else {
                    (target.imported_name.clone(), path.member_path())
                };
                let route = if member_path.is_empty() {
                    RouteDemand::Whole
                } else {
                    RouteDemand::MemberPath(Arc::from(member_path.to_vec().into_boxed_slice()))
                };
                external.insert(ExternalSymbolRef {
                    local_name: root.to_string(),
                    source_specifier: target.source_specifier.clone(),
                    imported_name,
                    canonical_id: external_canonical(target),
                    route,
                });
                continue;
            }

            if root != declaration_name && self.has_type_symbol_in(declaration_owner, root) {
                local.insert(root.to_string());
            }
        }

        let mut local = local.into_iter().collect::<Vec<_>>();
        local.sort();
        let mut external = external.into_iter().collect::<Vec<_>>();
        external.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.source_specifier.cmp(&right.source_specifier))
                .then_with(|| left.imported_name.cmp(&right.imported_name))
                .then_with(|| {
                    let left_path: &[String] = match &left.route {
                        RouteDemand::MemberPath(path) => path,
                        _ => &[],
                    };
                    let right_path: &[String] = match &right.route {
                        RouteDemand::MemberPath(path) => path,
                        _ => &[],
                    };
                    left_path.cmp(right_path)
                })
        });
        let mut unroutable = unroutable.into_iter().collect::<Vec<_>>();
        unroutable.sort();
        ClassifiedDependencyPaths {
            local_deps: local,
            external_deps: external,
            unroutable_imports: unroutable,
        }
    }

    /// Classify one file-scope TYPE symbol's dependency edges: the local
    /// vs external split over the per-declaration dependency names,
    /// `typeof`-root import edges appended, deterministic ordering by
    /// final sort. Lowers the body through the memo to read its
    /// dependency-name roots; stores ONLY dependency edges.
    fn classify_type_deps_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> DemandOutcome<ClassifiedTypeDeps> {
        let lowered = match self.decl_bodies.type_decl_outcome_in(owner, name) {
            // A broken decl-body lease pin: surface the DISTINCT no-warm signal
            // so the caller declines to cache a transient empty classification.
            DemandOutcome::LeaseMiss => return DemandOutcome::LeaseMiss,
            // A genuine, cacheable miss (the header lowered to no type decl).
            DemandOutcome::Ready(None) => return DemandOutcome::Ready(None),
            DemandOutcome::Ready(Some(lowered)) => lowered,
        };

        let legacy = self.classify_dependency_paths(owner, name, &lowered.dependency_paths);
        let declaration =
            self.classify_dependency_paths(owner, name, &lowered.declaration_carrier_paths);
        let local_deps = legacy.local_deps;
        let mut external_deps = legacy.external_deps;
        let declaration_local_deps = declaration.local_deps;
        let mut declaration_external_deps = declaration.external_deps;
        let mut unroutable_declaration_dependencies = declaration.unroutable_imports;

        let mut owner_value_deps = lowered
            .value_query_paths
            .iter()
            .chain(lowered.value_position_paths.iter())
            .map(TypeDependencyPathFact::root)
            .filter(|root| {
                !self
                    .owner_import_targets
                    .contains_key(&DeclKey::new(owner, *root))
            })
            .filter(|root| *root != name)
            .filter(|root| {
                self.has_value_symbol_in(owner, root) && !self.has_type_symbol_in(owner, root)
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        owner_value_deps.sort();
        owner_value_deps.dedup();

        let mut retained_value_carrier_deps = lowered
            .value_query_paths
            .iter()
            .chain(lowered.value_position_paths.iter())
            .map(TypeDependencyPathFact::root)
            .filter(|root| {
                !self
                    .owner_import_targets
                    .contains_key(&DeclKey::new(owner, *root))
            })
            .filter(|root| {
                self.has_value_symbol_in(owner, root) && self.has_type_symbol_in(owner, root)
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        retained_value_carrier_deps.sort();
        retained_value_carrier_deps.dedup();

        // Value-role roots retain the import declaration even when they do not
        // identify a type-space exported symbol (notably bare namespace
        // queries). They are appended to both legacy and declaration rails;
        // the role vectors below tell TSC whether the import must be usable as
        // a runtime value.
        for path in lowered
            .value_query_paths
            .iter()
            .chain(lowered.value_position_paths.iter())
        {
            let root = path.root();
            let Some(target) = self.owner_import_targets.get(&DeclKey::new(owner, root)) else {
                continue;
            };
            let external = ExternalSymbolRef {
                local_name: root.to_string(),
                source_specifier: target.source_specifier.clone(),
                imported_name: target.imported_name.clone(),
                canonical_id: external_canonical(target),
                route: RouteDemand::Whole,
            };
            if !external_deps
                .iter()
                .any(|dependency| dependency.local_name == root)
            {
                external_deps.push(external.clone());
            }
            if !declaration_external_deps
                .iter()
                .any(|dependency| dependency.local_name == root)
            {
                declaration_external_deps.push(external);
            }
        }

        unroutable_declaration_dependencies.retain(|root| {
            !lowered
                .value_query_paths
                .iter()
                .chain(lowered.value_position_paths.iter())
                .any(|path| path.root() == root)
        });

        external_deps.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.source_specifier.cmp(&right.source_specifier))
                .then_with(|| left.imported_name.cmp(&right.imported_name))
        });
        declaration_external_deps.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.source_specifier.cmp(&right.source_specifier))
                .then_with(|| left.imported_name.cmp(&right.imported_name))
        });
        let mut external_value_queries = lowered
            .value_query_paths
            .iter()
            .map(TypeDependencyPathFact::root)
            .filter(|root| {
                self.owner_import_targets
                    .contains_key(&DeclKey::new(owner, *root))
            })
            .map(str::to_string)
            .collect::<FxHashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        external_value_queries.sort();
        let mut external_value_positions = lowered
            .value_position_paths
            .iter()
            .map(TypeDependencyPathFact::root)
            .filter(|root| {
                self.owner_import_targets
                    .contains_key(&DeclKey::new(owner, *root))
            })
            .map(str::to_string)
            .collect::<FxHashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        external_value_positions.sort();

        DemandOutcome::Ready(Some(Arc::new(ClassifiedTypeDeps {
            local_deps,
            owner_value_deps,
            retained_value_carrier_deps,
            external_deps,
            declaration_local_deps,
            declaration_external_deps,
            unroutable_declaration_dependencies,
            has_unroutable_value_position: lowered.has_unroutable_value_position,
            external_value_queries,
            external_value_positions,
        })))
    }

    /// The transitive required-import closure of one local type — the
    /// import-local names the type's structural dependency graph reaches.
    /// Demand-scoped: only the walked symbols' bodies lower.
    pub(crate) fn required_import_names(&self, type_name: &str) -> FxHashSet<String> {
        self.required_import_names_in(TopLevelOwnerId::ordinary_file(), type_name)
    }

    /// Exact-owner transitive required-import closure of one local type.
    pub(crate) fn required_import_names_in(
        &self,
        owner: TopLevelOwnerId,
        type_name: &str,
    ) -> FxHashSet<String> {
        let mut required_imports = FxHashSet::default();
        let mut visited = FxHashSet::default();
        let mut pending = vec![type_name.to_string()];

        // Import membership is exact-owner and includes named, default, and
        // namespace bindings. Namespace roots are required carriers when a
        // declaration references a member path such as `NS.Payload`.
        let is_import_local = |name: &str| -> bool {
            self.owner_import_targets
                .get(&DeclKey::new(owner, name))
                .is_some()
        };

        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }

            if is_import_local(&current) {
                required_imports.insert(current);
                continue;
            }

            if self.has_type_symbol_in(owner, &current) {
                let Some(lowered) = self.decl_bodies.type_decl_in(owner, &current) else {
                    continue;
                };
                for reference in &lowered.structural_dependency_paths {
                    let root = reference.root().to_string();
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
                owner: if self.decl_bodies.framework_parse().is_some() {
                    TopLevelOwnerId::instance(0)
                } else {
                    TopLevelOwnerId::ordinary_file()
                },
                symbol_name: name.to_string(),
            });
    }

    /// Check if a name is an import-local binding.
    pub fn is_import_local(&self, name: &str) -> bool {
        self.is_import_local_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub(crate) fn is_import_local_in(&self, owner: TopLevelOwnerId, name: &str) -> bool {
        self.owner_import_targets
            .contains_key(&DeclKey::new(owner, name))
    }

    /// Get the import target for a local import name.
    pub fn import_target(&self, local_name: &str) -> Option<&ImportTarget> {
        self.import_target_in(TopLevelOwnerId::ordinary_file(), local_name)
    }

    pub(crate) fn import_target_in(
        &self,
        owner: TopLevelOwnerId,
        local_name: &str,
    ) -> Option<&ImportTarget> {
        self.owner_import_targets
            .get(&DeclKey::new(owner, local_name))
    }

    // -----------------------------------------------------------------------
    // Local closure
    // -----------------------------------------------------------------------

    /// Compute same-file closure for one symbol, collecting external refs.
    ///
    /// Budget limits the total number of local symbols visited to prevent
    /// pathological same-file dependency chains. Thin driver over the shared
    /// fact-closure core (`verter_semantic::facts::route_closure`) reading
    /// this state's stored per-decl route facts + dependency edges.
    pub fn local_closure(&self, symbol_name: &str, budget: usize) -> LocalClosureResult {
        self.local_closure_in(TopLevelOwnerId::ordinary_file(), symbol_name, budget)
    }

    pub(crate) fn local_closure_in(
        &self,
        owner: TopLevelOwnerId,
        symbol_name: &str,
        budget: usize,
    ) -> LocalClosureResult {
        from_fact_closure(verter_semantic::facts::local_closure_over_facts(
            &SfsRouteFactProvider { state: self, owner },
            symbol_name,
            budget,
        ))
    }

    /// Compute a narrower closure for a specific route on an exported symbol.
    ///
    /// For `Route::Whole`, the transitive whole-route edge walk; for
    /// `Route::MemberPath(p)`, the path-precise seed walk; for
    /// `Route::Pick`/`Route::Omit`, the member-seeded dependency closure.
    /// Falls back to the plain local closure when member-level data is
    /// unavailable. Thin driver over the shared fact-closure core: the
    /// transitive semantics live in `verter_semantic::facts::route_closure`,
    /// reading each declaration's stored `ShallowRouteFacts` through
    /// [`SfsRouteFactProvider`] — declaration bodies are never re-walked at
    /// query time.
    pub fn route_closure(
        &self,
        symbol_name: &str,
        route: &RouteDemand,
        budget: usize,
    ) -> LocalClosureResult {
        self.route_closure_in(TopLevelOwnerId::ordinary_file(), symbol_name, route, budget)
    }

    pub(crate) fn route_closure_in(
        &self,
        owner: TopLevelOwnerId,
        symbol_name: &str,
        route: &RouteDemand,
        budget: usize,
    ) -> LocalClosureResult {
        from_fact_closure(verter_semantic::facts::route_closure_over_facts(
            &SfsRouteFactProvider { state: self, owner },
            symbol_name,
            route,
            budget,
        ))
    }
}

/// The session-side provider for the shared route-closure core: stored route
/// facts (lazily lowered on first demand through the memo), header
/// membership, and the baked dependency-edge classification — never a body
/// re-walk.
struct SfsRouteFactProvider<'s> {
    state: &'s ShallowFileState,
    owner: TopLevelOwnerId,
}

impl verter_semantic::facts::RouteClosureProvider for SfsRouteFactProvider<'_> {
    fn has_type_symbol(&self, name: &str) -> bool {
        self.state.has_type_symbol_in(self.owner, name)
    }

    fn route_facts(&self, name: &str) -> Option<verter_type_expr::facts::ShallowRouteFacts> {
        self.state
            .type_decl_in(self.owner, name)
            .map(|lowered| lowered.route_facts.clone())
    }

    fn classified_deps(&self, name: &str) -> Option<verter_semantic::facts::ClassifiedRouteDeps> {
        let deps = self.state.type_deps_in(self.owner, name)?;
        Some(verter_semantic::facts::ClassifiedRouteDeps {
            local_deps: deps.local_deps.clone(),
            external_deps: deps
                .external_deps
                .iter()
                .map(external_ref_to_fact)
                .collect(),
        })
    }

    fn is_import_local(&self, name: &str) -> bool {
        self.state.is_import_local_in(self.owner, name)
    }

    fn import_route_target(
        &self,
        name: &str,
    ) -> Option<verter_type_expr::facts::ExternalRouteRefFact> {
        let target = self.state.import_target_in(self.owner, name)?;
        Some(verter_type_expr::facts::ExternalRouteRefFact {
            local_name: name.to_string(),
            source_specifier: target.source_specifier.clone(),
            imported_name: target.imported_name.clone(),
            canonical_id: external_canonical(target),
            route: RouteDemand::Whole,
        })
    }

    fn key_source_lookup(&self, name: &str) -> verter_semantic::facts::KeySourceLookup {
        use verter_semantic::facts::KeySourceLookup;
        use verter_type_expr::facts::KeySourceFact;

        // Header-decidable without any body demand: a non-type-symbol alias
        // enumerates to zero keys (the legacy non-symbol arm — the empty-keys
        // fall-through applies downstream).
        if !self.state.has_type_symbol_in(self.owner, name) {
            return KeySourceLookup::MissingTypeSymbol;
        }

        // The ENGINE half of the key-source producer/dispatch split: a
        // least-fixed-point fold over the same-file alias graph, one
        // content-free `KeySourceFact` mint per visited declaration
        // (`mint_key_source_fact` — the local, non-transitive producer over
        // the lease-borrowed lowered body). Any unavailable hop POISONS the
        // whole enumeration to `Unavailable` (fail closed — a partial key
        // set is never handed out, so no torn result can reach a route or a
        // cache); only a COMPLETED enumeration is sorted/deduped and
        // returned. Every hop demands through the same lazy decl-body memo
        // path `route_facts` rides (`type_decl` + lease-only transient
        // re-borrow), so the fact rail observes exactly the declarations the
        // enumeration consumed. The route-closure core never sees a
        // declaration body — only this tri-state outcome.
        let route_fact_lens = self.state.decl_bodies().route_fact_lens();
        let route_lens = route_fact_lens.for_owner(self.owner);
        let own_canonical = verter_semantic::facts::RouteFactLens::own_canonical_id(&route_lens);
        let mut visited = FxHashSet::default();
        let mut keys: Vec<String> = Vec::new();
        let mut pending = vec![name.to_string()];
        visited.insert(name.to_string());
        while let Some(current) = pending.pop() {
            let Some(fact) = self.mint_key_source_fact(&current, &route_lens) else {
                return KeySourceLookup::Unavailable;
            };
            match fact {
                // A non-finite surface contributes zero keys (distinct from
                // an unavailable hop — the enumeration stays decided).
                KeySourceFact::NoFiniteKeys => {}
                KeySourceFact::LiteralAliasUnion { literals, aliases } => {
                    keys.extend(literals.iter().cloned());
                    for alias in aliases.iter() {
                        // The producer anchors same-scope refs on the owning
                        // file's canonical; any other hop is unresolvable
                        // here — fail closed, never a fabricated key set.
                        if alias.anchor.canonical_id != own_canonical
                            || alias.anchor.owner != self.owner
                        {
                            return KeySourceLookup::Unavailable;
                        }
                        let symbol = alias.anchor.symbol.as_ref();
                        // A ref that names no file-scope TYPE symbol
                        // enumerates to zero keys (the legacy guard arm).
                        if !self.state.has_type_symbol_in(self.owner, symbol) {
                            continue;
                        }
                        if visited.insert(symbol.to_string()) {
                            pending.push(symbol.to_string());
                        }
                    }
                }
            }
        }
        keys.sort();
        keys.dedup();
        KeySourceLookup::Ready(keys)
    }
}

impl SfsRouteFactProvider<'_> {
    /// Mint the content-free key-source fact for ONE same-file alias hop:
    /// demand the alias's lowered record through the memo demand cell first
    /// (the same lazy path `route_facts` rides), then normalize the
    /// lease-borrowed per-contributor authored bodies LOCALLY and
    /// NON-TRANSITIVELY through the shared producer
    /// (`produce_key_source_fact`). `None` = UNAVAILABLE: a header miss,
    /// fatal parse, broken lease pin (`LeaseMiss`), or body-less re-borrow
    /// (seeded memo — no retained authored source) leaves the enumeration
    /// UNDECIDED, not empty — the caller fails closed so the empty-keys
    /// fallback does NOT fire (under-production, never a wrong route).
    fn mint_key_source_fact(
        &self,
        name: &str,
        lens: &dyn verter_semantic::facts::RouteFactLens,
    ) -> Option<verter_type_expr::facts::KeySourceFact> {
        self.state.type_decl_in(self.owner, name)?;
        match self
            .state
            .decl_bodies()
            .transient_type_bodies_in(self.owner, name)
        {
            crate::decl_body_memo::DemandOutcome::Ready(Some(bodies)) if !bodies.is_empty() => {
                Some(verter_semantic::facts::produce_key_source_fact(
                    bodies.as_ref(),
                    lens,
                ))
            }
            // The DISTINCT transient-body broken-lease pin: this wildcard
            // collapse bypasses `into_option`, so mark the generalized
            // non-cacheability rail on `LeaseMiss` (a genuine `Ready(None)` /
            // body-less re-borrow stays an unmarked, cacheable undecided miss).
            crate::decl_body_memo::DemandOutcome::LeaseMiss => {
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
                );
                None
            }
            _ => None,
        }
    }
}

/// 1:1 field conversion: session `ExternalSymbolRef` → lower-crate
/// `ExternalRouteRefFact` (the fact-closure boundary).
fn external_ref_to_fact(ext: &ExternalSymbolRef) -> verter_type_expr::facts::ExternalRouteRefFact {
    verter_type_expr::facts::ExternalRouteRefFact {
        local_name: ext.local_name.clone(),
        source_specifier: ext.source_specifier.clone(),
        imported_name: ext.imported_name.clone(),
        canonical_id: ext.canonical_id.clone(),
        route: ext.route.clone(),
    }
}

/// 1:1 field conversion: lower-crate `ExternalRouteRefFact` → session
/// `ExternalSymbolRef`.
fn external_fact_to_ref(fact: verter_type_expr::facts::ExternalRouteRefFact) -> ExternalSymbolRef {
    ExternalSymbolRef {
        local_name: fact.local_name,
        source_specifier: fact.source_specifier,
        imported_name: fact.imported_name,
        canonical_id: fact.canonical_id,
        route: fact.route,
    }
}

/// Convert a shared fact-closure result to the session closure result
/// (status arms map 1:1; external refs convert field-by-field).
fn from_fact_closure(result: verter_semantic::facts::FactClosureResult) -> LocalClosureResult {
    use verter_semantic::facts::FactClosureStatus;
    LocalClosureResult {
        status: match result.status {
            FactClosureStatus::Resolved => LocalClosureStatus::Resolved,
            FactClosureStatus::ResolvedWithExternalDeps => {
                LocalClosureStatus::ResolvedWithExternalDeps
            }
            FactClosureStatus::MissingLocalSymbol { name } => {
                LocalClosureStatus::MissingLocalSymbol { name }
            }
            FactClosureStatus::BudgetExceeded => LocalClosureStatus::BudgetExceeded,
        },
        local_symbols_used: result.local_symbols_used,
        unresolved_external: result
            .unresolved_external
            .into_iter()
            .map(external_fact_to_ref)
            .collect(),
        steps: result.steps,
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

pub(crate) trait TypeofDependencyCollector {
    fn record(&mut self, value_ref: &verter_type_expr::ValueRef);
}

/// Runaway-safety fuse for session-owned semantic-inference tree walks.
/// Parser syntax depth is substantially lower; this bound protects mutated or
/// synthesized owned IR while leaving ordinary authored programs untouched.
pub(crate) const SEMANTIC_INFERENCE_TRAVERSAL_BUDGET: usize = 4_096;

impl TypeofDependencyCollector for FxHashSet<String> {
    fn record(&mut self, value_ref: &verter_type_expr::ValueRef) {
        if let Some(root) = value_ref.path.first() {
            self.insert(root.clone());
        }
    }
}

impl TypeofDependencyCollector for BTreeSet<TypeDependencyPathFact> {
    fn record(&mut self, value_ref: &verter_type_expr::ValueRef) {
        if let Some(path) = TypeDependencyPathFact::from_segments(value_ref.path.iter().cloned()) {
            self.insert(path);
        }
    }
}

/// Collect every `typeof <value>` dependency reachable in `expr`. Callers
/// choose either root-name or full typed-path retention through the collector.
pub(crate) fn collect_typeof_roots<C: TypeofDependencyCollector>(
    expr: &TypeExpr,
    out: &mut C,
) -> Result<(), verter_type_expr::facts::InferenceUnavailableReason> {
    let mut pending = vec![expr];
    let mut visited = 0usize;
    while let Some(current) = pending.pop() {
        visited = visited.saturating_add(1);
        if visited > SEMANTIC_INFERENCE_TRAVERSAL_BUDGET {
            return Err(verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded);
        }
        if let TypeExpr::TypeOf(value_ref) = current {
            out.record(value_ref);
        }
        push_type_expr_children(current, &mut pending);
    }
    Ok(())
}

/// Whether every rendered leaf is declaration-safe. Inferred declaration
/// splices must never hide an implicit `any`/unknown lowering inside a nested
/// function, collection, object, or generic argument.
pub(crate) fn type_expr_is_declaration_safe(
    expr: &TypeExpr,
) -> Result<bool, verter_type_expr::facts::InferenceUnavailableReason> {
    let mut pending = vec![expr];
    let mut visited = 0usize;
    while let Some(current) = pending.pop() {
        visited = visited.saturating_add(1);
        if visited > SEMANTIC_INFERENCE_TRAVERSAL_BUDGET {
            return Err(verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded);
        }
        match current {
            TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::Any | verter_type_expr::PrimitiveName::Unknown,
            )
            | TypeExpr::Unknown { .. }
            | TypeExpr::SyntheticSlotBinding(_) => return Ok(false),
            TypeExpr::Function(function) | TypeExpr::ConstructorType(function)
                if function.return_type.is_none() =>
            {
                return Ok(false);
            }
            _ => push_type_expr_children(current, &mut pending),
        }
    }
    Ok(true)
}

fn push_type_expr_children<'a>(expr: &'a TypeExpr, pending: &mut Vec<&'a TypeExpr>) {
    let push_type_param = |parameter: &'a verter_type_expr::TypeParam,
                           pending: &mut Vec<&'a TypeExpr>| {
        if let Some(constraint) = parameter.constraint.as_deref() {
            pending.push(constraint);
        }
        if let Some(default) = parameter.default.as_deref() {
            pending.push(default);
        }
    };
    let push_function = |function: &'a verter_type_expr::FunctionExpr,
                         pending: &mut Vec<&'a TypeExpr>| {
        for parameter in &function.parameters {
            pending.push(&parameter.ty);
        }
        if let Some(return_type) = function.return_type.as_deref() {
            pending.push(return_type);
        }
        for parameter in &function.type_parameters {
            push_type_param(parameter, pending);
        }
    };

    match expr {
        TypeExpr::TypeOf(value_ref) => pending.extend(value_ref.type_args.iter()),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => pending.extend(types.iter()),
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => pending.push(element),
        TypeExpr::Tuple { elements, .. } => {
            pending.extend(elements.iter().map(|element| &element.ty));
        }
        TypeExpr::Object(object) => {
            for member in &object.properties {
                match member {
                    verter_type_expr::ObjectMember::Property(property) => {
                        pending.push(&property.ty);
                    }
                    verter_type_expr::ObjectMember::IndexSignature(signature) => {
                        pending.push(&signature.key_type);
                        pending.push(&signature.value_type);
                    }
                    verter_type_expr::ObjectMember::CallSignature(function)
                    | verter_type_expr::ObjectMember::ConstructSignature(function) => {
                        push_function(function, pending);
                    }
                    verter_type_expr::ObjectMember::Method(method) => {
                        push_function(&method.function, pending);
                    }
                }
            }
        }
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            push_function(function, pending);
        }
        TypeExpr::IndexedAccess { object, index } => {
            pending.push(object);
            pending.push(index);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            pending.push(check);
            pending.push(extends);
            pending.push(true_type);
            pending.push(false_type);
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            pending.push(source);
            pending.push(value);
            if let Some(name_type) = name_type.as_deref() {
                pending.push(name_type);
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => pending.extend(expressions.iter()),
        TypeExpr::Ref { type_arguments, .. } | TypeExpr::ImportType { type_arguments, .. } => {
            pending.extend(type_arguments.iter())
        }
        TypeExpr::TypeParameter(parameter) => push_type_param(parameter, pending),
        TypeExpr::RecursiveRef {
            type_arguments,
            conditional_context,
            ..
        } => {
            pending.extend(type_arguments.iter());
            for frame in conditional_context.iter() {
                pending.push(&frame.check);
                pending.push(&frame.extends);
            }
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::type_eval::ValueDeclKind;

    #[test]
    fn route_inventory_preserves_same_name_bindings_across_exact_owners() {
        let source = concat!(
            "import type { A as Payload } from './module';\n",
            "import type { B as Payload } from './instance';\n",
            "export type { Payload as ModulePayload };\n",
            "export type { Payload as InstancePayload };\n",
        );
        let owners = [
            TopLevelOwnerId::module(0),
            TopLevelOwnerId::instance(0),
            TopLevelOwnerId::module(0),
            TopLevelOwnerId::instance(0),
        ];
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
        assert!(!parsed.panicked, "owner-qualified route fixture must parse");
        let owner_table = verter_semantic::analysis::TopLevelOwnerTable::try_from_statement_owners(
            parsed.program.body.len(),
            owners,
        )
        .expect("owner table exactly covers the route fixture");
        let index = verter_semantic::analysis::script_shallow_index::build_script_shallow_index_with_owners(
            &parsed.program,
            source,
            &owner_table,
        )
        .expect("route inventory accepts the validated owner table");
        let memo_state = ShallowFileState::service_backed_for_test_with_statement_owners(
            "/ws/owners.ts",
            source,
            &owners,
        );
        let state = ShallowFileState::from_route_inventory_with_resolver(
            memo_state.whole_hash,
            Arc::new(index.routes),
            Arc::clone(memo_state.decl_bodies()),
            &NullResolver,
        );

        assert_eq!(
            state
                .import_target_in(TopLevelOwnerId::module(0), "Payload")
                .map(|target| target.source_specifier.as_str()),
            Some("./module"),
        );
        assert_eq!(
            state
                .import_target_in(TopLevelOwnerId::instance(0), "Payload")
                .map(|target| target.source_specifier.as_str()),
            Some("./instance"),
        );
        assert_eq!(
            state.export_target("ModulePayload"),
            Some(&ExportTarget::Local {
                owner: TopLevelOwnerId::module(0),
                symbol_name: "Payload".to_string(),
            }),
        );
        assert_eq!(
            state.export_target("InstancePayload"),
            Some(&ExportTarget::Local {
                owner: TopLevelOwnerId::instance(0),
                symbol_name: "Payload".to_string(),
            }),
        );
    }

    #[test]
    fn lexical_parent_owner_is_one_way_unique_and_ambiguity_safe() {
        let module = TopLevelOwnerId::module(0);
        let instance = TopLevelOwnerId::instance(0);
        let unique = ShallowFileState::service_backed_for_test_with_statement_owners(
            "/ws/unique-owner.ts",
            "type Module = {}; type Setup = {};",
            &[module, instance],
        );
        assert_eq!(
            unique.validated_lexical_parent_owner(instance),
            Some(module)
        );
        assert_eq!(unique.validated_lexical_parent_owner(module), None);

        let ambiguous = ShallowFileState::service_backed_for_test_with_statement_owners(
            "/ws/ambiguous-owner.ts",
            "type First = {}; type Second = {}; type Setup = {};",
            &[module, TopLevelOwnerId::module(1), instance],
        );
        assert_eq!(ambiguous.validated_lexical_parent_owner(instance), None);
    }

    #[test]
    fn deep_inference_type_tree_fails_with_typed_budget_without_recursion() {
        let mut expr = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
        for _ in 0..=SEMANTIC_INFERENCE_TRAVERSAL_BUDGET {
            expr = TypeExpr::Array {
                element: Arc::new(expr),
                readonly: false,
            };
        }

        assert_eq!(
            type_expr_is_declaration_safe(&expr),
            Err(verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded),
            "deep inferred initializer/return types fail typed instead of overflowing"
        );
        let mut roots = FxHashSet::default();
        assert_eq!(
            collect_typeof_roots(&expr, &mut roots),
            Err(verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded),
        );
        assert!(roots.is_empty());

        // Avoid making the test's destructor itself recursively drop the
        // adversarial Arc chain; the production walkers never own or drop it.
        std::mem::forget(expr);
    }

    fn make_routes(source: &str) -> Arc<ScriptRouteInventory> {
        let alloc = oxc_allocator::Allocator::new();
        let parsed = oxc_parser::Parser::new(&alloc, source, oxc_span::SourceType::ts()).parse();
        assert!(!parsed.panicked, "route fixture must parse");
        Arc::new(
            verter_parser::utils::oxc::script::route_inventory::build_script_route_inventory(
                &parsed.program,
            ),
        )
    }

    #[test]
    fn simple_interface_produces_local_export() {
        let routes = make_routes("export interface Props { label: string }");
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

        assert!(
            state.export_target("Props").is_some(),
            "Props should be exported"
        );
        match state.export_target("Props").unwrap() {
            ExportTarget::Local { symbol_name, .. } => {
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
    fn merged_exported_interfaces_preserve_one_idempotent_local_route() {
        let routes = make_routes(
            "export interface Props { label: string }\n\
             export interface Props { count: number }\n",
        );
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

        assert_eq!(
            state.export_target("Props"),
            Some(&ExportTarget::Local {
                owner: TopLevelOwnerId::ordinary_file(),
                symbol_name: "Props".to_string(),
            }),
            "identical routes from legal declaration merging are idempotent, not ambiguous",
        );
    }

    #[test]
    fn reexport_produces_reexport_target() {
        let routes = make_routes(r#"export { Foo } from "./inner""#);
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

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
        let routes = make_routes("export * from './a'\nexport * from './b'\nexport * from './c'\n");
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

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

    // NOTE: local_closure tests require declaration headers to populate
    // symbols. Routing-only construction produces empty symbols, so closure
    // tests use the service-backed path. We test export and import routing
    // with routing-only state, and closure with memo headers below.

    #[test]
    fn routing_only_has_no_symbols() {
        let routes = make_routes("export interface Props { label: string }\n");
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

        // Symbols require eval_env
        assert!(
            state.type_symbol_names().next().is_none(),
            "routing-only construction should produce no symbols"
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
        let state = ShallowFileState::service_backed_for_test(source);

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
        assert!(!matches!(
            defaults_body.type_annotation.classification,
            verter_type_expr::facts::ValueAnnotationClass::Absent
        ));
    }

    /// A type that references a JSDoc-`@typedef` name must classify that
    /// reference as a LOCAL dep — the typedef lives in the shallow header
    /// index, while the syntax route inventory intentionally contains no
    /// declarations, so local classification must consult the header index.
    #[test]
    fn jsdoc_typedef_reference_classifies_as_local_dep() {
        let source = r#"
import { Imported } from './dep'
/** @typedef {Imported} Alias */
export type X = Alias
"#;
        let state = ShallowFileState::service_backed_for_test(source);

        // `Alias` is a shallow type symbol owned by the declaration headers.
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
        let state = ShallowFileState::service_backed_for_test(source);
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
    fn qualified_import_dependencies_preserve_export_head_and_member_route() {
        let source = r#"
import type * as NS from './types'
import type { Foo as F } from './named'
export interface Props { value: NS.Value.Inner; named: F.Bar }
"#;
        let state = ShallowFileState::service_backed_for_test(source);
        let props = state.type_deps("Props").expect("Props should exist");

        let namespace = props
            .external_deps
            .iter()
            .find(|dependency| dependency.local_name == "NS")
            .unwrap();
        assert_eq!(namespace.imported_name, "Value");
        assert_eq!(
            namespace.route,
            RouteDemand::MemberPath(Arc::from(["Inner".to_string()])),
        );
        assert_ne!(namespace.imported_name, "*.Value.Inner");

        let named = props
            .external_deps
            .iter()
            .find(|dependency| dependency.local_name == "F")
            .unwrap();
        assert_eq!(named.imported_name, "Foo");
        assert_eq!(
            named.route,
            RouteDemand::MemberPath(Arc::from(["Bar".to_string()])),
        );
    }

    #[test]
    fn bare_namespace_type_dependency_is_explicitly_unroutable() {
        let state = ShallowFileState::service_backed_for_test(
            "import type * as NS from './types'; export type Props = NS;",
        );
        let dependencies = state.type_deps("Props").unwrap();

        assert_eq!(dependencies.unroutable_declaration_dependencies, ["NS"],);
        assert!(dependencies.declaration_external_deps.is_empty());
    }

    #[test]
    fn bare_namespace_type_query_is_a_value_import_not_an_unroutable_type_route() {
        let state = ShallowFileState::service_backed_for_test(
            "import * as NS from './types'; export type Props = typeof NS;",
        );
        let dependencies = state.type_deps("Props").unwrap();

        assert!(dependencies.unroutable_declaration_dependencies.is_empty());
        assert_eq!(dependencies.external_value_queries, ["NS"]);
        assert!(dependencies
            .declaration_external_deps
            .iter()
            .any(|dependency| dependency.local_name == "NS"));
    }

    #[test]
    fn qualified_class_heritage_is_a_routable_value_position() {
        let state = ShallowFileState::service_backed_for_test(
            "import * as NS from './types'; export class Props extends NS.Base {}",
        );
        let dependencies = state.type_deps("Props").unwrap();

        assert_eq!(dependencies.external_value_positions, ["NS"]);
        assert!(!dependencies.has_unroutable_value_position);
        let base = dependencies
            .declaration_external_deps
            .iter()
            .find(|dependency| dependency.local_name == "NS")
            .unwrap();
        assert_eq!(base.imported_name, "Base");
        assert_eq!(base.route, RouteDemand::Whole);
    }

    #[test]
    fn call_expression_class_heritage_is_explicitly_unroutable() {
        let state = ShallowFileState::service_backed_for_test(
            "declare function mixin<T>(base: T): T; \
             declare class Base {}; export class Props extends mixin(Base) {}",
        );
        let dependencies = state.type_deps("Props").unwrap();

        assert!(dependencies.has_unroutable_value_position);
    }

    #[test]
    fn direct_export_takes_precedence_over_wildcard_route() {
        let routes = make_routes(
            r#"
export { Foo } from './direct'
export * from './wildcard'
"#,
        );
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

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
        let routes = make_routes(
            r#"
import type { Alpha } from './a'
import type { Beta as B } from './b'
export interface Props extends Alpha { beta: B }
"#,
        );
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

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
        let routes = make_routes(
            r#"
import Foo from './dep'
export { Foo as Bar }
"#,
        );
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

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
            Some(ExportTarget::Local { symbol_name, .. }) => {
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
        let routes = make_routes(
            r#"
export default class Props {
  label!: string
}
"#,
        );
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

        assert!(
            state.export_target("Props").is_none(),
            "named class identifier should not be published as a separate export for default-only classes"
        );

        match state.export_target("default") {
            Some(ExportTarget::Local { symbol_name, .. }) => {
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
        let state = ShallowFileState::service_backed_for_test(source);

        let defaults = state
            .value_symbol("defaults")
            .expect("defaults value symbol should be present");
        assert_eq!(defaults.kind, ValueDeclKind::Const);
        let defaults_body = state
            .value_decl("defaults")
            .expect("defaults value body should lower on demand");
        assert!(!matches!(
            defaults_body.type_annotation.classification,
            verter_type_expr::facts::ValueAnnotationClass::Absent
        ));
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
        let state = ShallowFileState::service_backed_for_test(source);
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
        let routes = make_routes(source);
        let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), routes);

        match state.export_target("defaults") {
            Some(ExportTarget::Local { symbol_name, .. }) => assert_eq!(symbol_name, "defaults"),
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
        let state = ShallowFileState::service_backed_for_test(source);

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
        let members = &symbol.member_names;
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
        let state = ShallowFileState::service_backed_for_test(source);

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
            &crate::identity_interner::IdentityInterner::with_default_budget(),
        )
        .expect("Props preparation should not fail")
        .expect("Props should prepare");

        // Local dep should resolve to same file
        assert_eq!(
            prepared
                .name_resolution
                .get("Local")
                .map(|r| r.canonical_id.as_ref()),
            Some("/src/types.ts"),
            "local dep should resolve to same file"
        );
        // External dep should resolve through dep_edges
        assert_eq!(
            prepared
                .name_resolution
                .get("Inner")
                .map(|r| r.canonical_id.as_ref()),
            Some("/resolved/inner.ts"),
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
        let state = ShallowFileState::service_backed_for_test(source);

        let sym = state.type_decl("CheckboxProps").expect("CheckboxProps");

        // Member dependency edges should exist for 'ui', 'indicator', but not
        // 'color' (primitive).
        let member_edge = |member: &str| {
            sym.route_facts
                .member_dependency_edges
                .iter()
                .find(|edge| edge.member == member)
        };
        assert!(
            member_edge("ui").is_some(),
            "ui should have member deps, edges: {:?}",
            sym.route_facts.member_dependency_edges
        );
        assert!(
            member_edge("indicator").is_some(),
            "indicator should have member deps"
        );
        // 'color' is just 'string' — no refs
        assert!(
            member_edge("color").is_none(),
            "color (primitive string) should have no deps"
        );

        // Verify 'ui' deps reference AppConfig
        let ui_edge = member_edge("ui").expect("ui edge");
        assert!(
            ui_edge.depends_on.iter().any(|dep| matches!(
                dep,
                verter_type_expr::facts::RouteDependencyRefFact::Local { name, .. } if name == "AppConfig"
            )),
            "ui deps should reference AppConfig, got {:?}",
            ui_edge.depends_on
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
        let state = ShallowFileState::service_backed_for_test(source);

        // Route::Member("a") should only include Alpha deps, not Beta
        let closure_a = state.route_closure(
            "Props",
            &RouteDemand::member_path(vec!["a".to_string()]),
            500,
        );
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
        let state = ShallowFileState::service_backed_for_test(source);

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
            RouteDemand::member_path(vec!["name".to_string()]),
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
        let state = ShallowFileState::service_backed_for_test(source);

        // Pick(['x', 'z']) should include A and C but not B
        let closure = state.route_closure(
            "Props",
            &RouteDemand::pick(vec!["x".to_string(), "z".to_string()]),
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
        let state = ShallowFileState::service_backed_for_test(source);

        let closure = state.route_closure("Props", &RouteDemand::omit(vec!["y".to_string()]), 500);
        let ext_names: Vec<&str> = closure
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert!(ext_names.contains(&"A"));
        assert!(ext_names.contains(&"C"));
        assert!(!ext_names.contains(&"B"));
    }

    /// Discriminator for the deferred key-source hand-off
    /// ([`SfsRouteFactProvider::key_source_lookup`]): a deferred Pick key
    /// alias (`type K = 'a' | 'b'; type D = Pick<Imported, K>`) enumerates
    /// through the content-free key-source fact minted off the lazy
    /// decl-body machinery, so the whole-route closure over `D` emits the
    /// EXTERNAL route for `Imported` carrying the CONCRETE keys
    /// `Pick(["a", "b"])`.
    ///
    /// With a broken hand-off (`key_source_lookup → Unavailable`) the
    /// deferred edge contributes nothing — the closure emits NO external
    /// route at all, so this test fails RED there.
    #[test]
    fn route_closure_deferred_pick_key_alias_derefs_to_literal_union_keys() {
        let source = r#"
import type { Imported } from './dep'

type K = 'a' | 'b'
export type D = Pick<Imported, K>
"#;
        let state = ShallowFileState::service_backed_for_test(source);

        let closure = state.route_closure("D", &RouteDemand::Whole, 500);
        assert_eq!(
            closure
                .unresolved_external
                .iter()
                .map(|e| (e.imported_name.as_str(), e.route.clone()))
                .collect::<Vec<_>>(),
            vec![("Imported", RouteDemand::pick(["a", "b"]))],
            "the deferred key alias `K` must deref to its literal union keys \
             through the lazy decl-body machinery and route the imported Pick \
             base with the concrete keys, got {:?}",
            closure.unresolved_external
        );
        assert!(
            matches!(closure.status, LocalClosureStatus::ResolvedWithExternalDeps),
            "the deferred edge resolves with the external base outstanding, \
             got {:?}",
            closure.status
        );
    }

    /// Discriminator for the ENGINE-side alias follow (the dispatch half of
    /// the key-source producer/dispatch split): a CHAINED key alias
    /// (`type K = K2; type K2 = 'a' | 'b'`) resolves through the
    /// least-fixed-point fold over per-decl content-free key-source facts —
    /// the producer mints `K`'s fact with an UNRESOLVED alias ref to `K2`,
    /// and the session dispatch follows it. With the alias follow neutered
    /// (the ref arm not pushed), `K` enumerates to zero literal keys and the
    /// concrete `Pick(["a", "b"])` route never materializes — this test
    /// fails RED there.
    #[test]
    fn route_closure_deferred_pick_key_alias_chain_follows_through_engine_dispatch() {
        let source = r#"
import type { Imported } from './dep'

type K = K2
type K2 = 'a' | 'b'
export type D = Pick<Imported, K>
"#;
        let state = ShallowFileState::service_backed_for_test(source);

        let closure = state.route_closure("D", &RouteDemand::Whole, 500);
        assert_eq!(
            closure
                .unresolved_external
                .iter()
                .map(|e| (e.imported_name.as_str(), e.route.clone()))
                .collect::<Vec<_>>(),
            vec![("Imported", RouteDemand::pick(["a", "b"]))],
            "the chained key alias must resolve through the engine-side \
             alias-graph fold to the terminal literal union, got {:?}",
            closure.unresolved_external
        );
    }

    /// Fail-closed CONTROL for the deferred key-source hand-off: a key
    /// alias whose body genuinely cannot be resolved (a broken decl-body
    /// lease pin) is UNAVAILABLE — the deferred edge contributes NOTHING
    /// and in particular must NOT fire the userland `Pick` empty-keys
    /// fallback (which would emit `Imported2` whole — a route the authoring
    /// walk never produced).
    #[test]
    fn route_closure_deferred_key_source_lease_miss_fails_closed_without_fallback() {
        let source = r#"
import type { Imported } from './dep'
import type { Imported2 } from './b'

type K = 'a' | 'b'
type Pick = Imported2
export type D = Pick<Imported, K>
"#;
        let state = ShallowFileState::service_backed_for_test(source);

        // Under a LIVE lease the keys resolve and the imported base routes
        // path-precisely (positive control: the hand-off genuinely works on
        // this fixture before the lease breaks).
        let live = state.route_closure("D", &RouteDemand::Whole, 500);
        assert_eq!(
            live.unresolved_external
                .iter()
                .map(|e| (e.imported_name.as_str(), e.route.clone()))
                .collect::<Vec<_>>(),
            vec![("Imported", RouteDemand::pick(["a", "b"]))],
            "live-lease control: the deferred keys resolve, got {:?}",
            live.unresolved_external
        );

        // Break the retained snapshot out-of-band: the next transient body
        // borrow lease-misses, so the key source is genuinely UNAVAILABLE.
        state.decl_bodies().release_retained_snapshot_for_test();

        let broken = state.route_closure("D", &RouteDemand::Whole, 500);
        assert!(
            broken
                .unresolved_external
                .iter()
                .all(|e| e.imported_name != "Imported2"),
            "an unavailable key source must NOT fire the userland Pick \
             empty-keys fallback (wrong route), got {:?}",
            broken.unresolved_external
        );
        assert_eq!(
            broken.unresolved_external,
            Vec::new(),
            "an unavailable key source fails closed: the deferred edge \
             contributes nothing (under-production, never a fabricated route)"
        );
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
        let state = ShallowFileState::service_backed_for_test(source);

        let closure = state.route_closure(
            "Props",
            &RouteDemand::member_path(vec!["color".to_string()]),
            500,
        );
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
        let state = ShallowFileState::service_backed_for_test(source);

        let closure = state.route_closure(
            "Props",
            &RouteDemand::member_path(vec!["variants".to_string(), "color".to_string()]),
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
        let state = ShallowFileState::service_backed_for_test(source);

        let closure = state.route_closure(
            "Props",
            &RouteDemand::member_path(vec!["primary".to_string(), "label".to_string()]),
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
            RouteDemand::member_path(vec!["label".to_string()]),
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
        let state = ShallowFileState::service_backed_for_test(source);

        let closure = state.route_closure(
            "Props",
            &RouteDemand::member_path(vec!["variants".to_string(), "missing".to_string()]),
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
        let (state, _) = ShallowFileState::service_backed_with_provenance_and_resolver_for_test(
            "/ws/fixture.ts",
            source,
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
        let state = ShallowFileState::service_backed_for_test(source);

        let closure = state.route_closure(
            "Props",
            &RouteDemand::member_path(vec!["inherited".to_string()]),
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
