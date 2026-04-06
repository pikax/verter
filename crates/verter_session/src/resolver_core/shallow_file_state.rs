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
use verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource;
use verter_semantic::analysis::type_eval::{FunctionSignature, TypeDeclKind, ValueDeclKind};
use verter_semantic::analysis::type_expr::{ObjectExpr, TypeExpr, TypeParam};
use verter_semantic::analysis::Hash16;

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

    /// All locally-declared type symbols (exported or internal).
    pub symbols: FxHashMap<String, ShallowTypeSymbol>,

    /// All locally-declared value symbols that may participate in `typeof`
    /// queries or value-driven type expansion.
    pub value_symbols: FxHashMap<String, ShallowValueSymbol>,

    /// Import-local names (names that come from `import` declarations).
    /// Used to classify dependencies as local vs external during closure.
    pub import_locals: FxHashSet<String>,

    /// Import specifier targets: local import name → canonical import target.
    pub import_targets: FxHashMap<String, ImportTarget>,

    /// The underlying analyzed source (retained for methods that still need
    /// the full analysis surface during the transition).
    pub analysis: Arc<AnalyzedExternalTypeSource>,
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

/// Narrow access route for an exported type. Used to compute a narrower
/// import closure that only includes dependencies reachable from the
/// requested route, not the full export.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExportedRoute {
    /// Full export — all dependencies.
    Whole,
    /// Single member access: `Type['member']`.
    Member(String),
    /// Pick subset: `Pick<Type, 'a' | 'b'>`.
    Pick(Vec<String>),
    /// Omit subset: `Omit<Type, 'a' | 'b'>`.
    Omit(Vec<String>),
}

/// Shallow metadata for one locally-declared type symbol.
#[derive(Debug, Clone)]
pub struct ShallowTypeSymbol {
    /// Declaration kind.
    pub kind: TypeDeclKind,
    /// The raw symbolic body (pre-evaluation TypeExpr).
    pub raw_body: TypeExpr,
    /// Generic type parameters.
    pub type_parameters: Vec<TypeParam>,
    /// Names of same-file symbols this type directly depends on.
    /// Used for iterative local closure.
    pub local_deps: Vec<String>,
    /// Names of import-local symbols this type directly depends on.
    /// These become `ExternalSymbolRef` during frontier traversal.
    pub external_deps: Vec<ExternalSymbolRef>,
    /// Per-member dependency tracking for route-aware closure narrowing.
    /// Maps member name → names referenced in that member's type.
    /// Populated only for Object-bodied symbols (interfaces, object types).
    /// Empty for non-object bodies or if member-level tracking is unavailable.
    pub member_deps: rustc_hash::FxHashMap<String, Vec<String>>,
}

/// Shallow metadata for one locally-declared value symbol.
#[derive(Debug, Clone)]
pub struct ShallowValueSymbol {
    /// Declaration kind.
    pub kind: ValueDeclKind,
    /// Explicit type annotation, if present.
    pub type_annotation: Option<TypeExpr>,
    /// Function signature, if this value is callable.
    pub function_signature: Option<FunctionSignature>,
    /// Object literal shape, if the declaration is a literal object.
    pub object_shape: Option<ObjectExpr>,
    /// Enum member values — populated for `ValueDeclKind::Enum`.
    pub enum_members: Option<rustc_hash::FxHashMap<String, TypeExpr>>,
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
    /// The resolved canonical file ID of the target.
    /// Empty string if unresolved (construction without host resolver).
    pub canonical_id: String,
}

// ---------------------------------------------------------------------------
// Budget and failure contract (Phase 1.5)
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
#[derive(Debug, Clone)]
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
    /// Build from an existing `AnalyzedExternalTypeSource` and export-routing data.
    ///
    /// This is the primary construction path: the host ensures the file is loaded
    /// and analyzed, then builds the shallow state from that analysis.
    ///
    /// Uses a null resolver — canonical IDs on edges will be empty strings.
    /// Production code should prefer `from_analysis_with_resolver`.
    pub fn from_analysis(
        whole_hash: Hash16,
        analysis: Arc<AnalyzedExternalTypeSource>,
        eval_env: Option<&verter_semantic::analysis::type_eval::EvalEnv>,
    ) -> Self {
        Self::from_analysis_with_source(whole_hash, analysis, None, eval_env)
    }

    /// Build from analysis with a resolver that canonicalizes all cross-file edges.
    ///
    /// This is the preferred production construction path.
    pub fn from_analysis_with_resolver(
        whole_hash: Hash16,
        analysis: Arc<AnalyzedExternalTypeSource>,
        eval_source: Option<&str>,
        eval_env: Option<&verter_semantic::analysis::type_eval::EvalEnv>,
        resolver: &dyn ShallowImportResolver,
    ) -> Self {
        Self::from_analysis_inner(whole_hash, analysis, eval_source, eval_env, resolver)
    }

    /// Build from analysis with an optional source fallback that can populate
    /// symbol inventories when the caller does not already have an `EvalEnv`.
    ///
    /// Uses a null resolver — canonical IDs on edges will be empty strings.
    pub fn from_analysis_with_source(
        whole_hash: Hash16,
        analysis: Arc<AnalyzedExternalTypeSource>,
        eval_source: Option<&str>,
        eval_env: Option<&verter_semantic::analysis::type_eval::EvalEnv>,
    ) -> Self {
        Self::from_analysis_inner(whole_hash, analysis, eval_source, eval_env, &NullResolver)
    }

    fn from_analysis_inner(
        whole_hash: Hash16,
        analysis: Arc<AnalyzedExternalTypeSource>,
        eval_source: Option<&str>,
        eval_env: Option<&verter_semantic::analysis::type_eval::EvalEnv>,
        resolver: &dyn ShallowImportResolver,
    ) -> Self {
        let fallback_env =
            eval_source.map(verter_semantic::analysis::type_eval_build::parse_and_build_env);
        let eval_env = eval_env.or(fallback_env.as_ref());
        let mut exports = FxHashMap::default();
        let mut wildcard_reexports = Vec::new();
        let mut import_locals = FxHashSet::default();
        let mut import_targets: FxHashMap<String, ImportTarget> = FxHashMap::default();
        let mut symbols: FxHashMap<String, ShallowTypeSymbol> = FxHashMap::default();
        let mut value_symbols: FxHashMap<String, ShallowValueSymbol> = FxHashMap::default();

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

        // Locally-declared symbols from eval env (if available)
        if let Some(env) = eval_env {
            for (name, decl) in &env.type_symbols {
                let local_type_sym = analysis.local_type_symbol(name);
                let (local_deps, mut external_deps) = if let Some(sym) = local_type_sym {
                    classify_deps(
                        name,
                        analysis.as_ref(),
                        sym,
                        &import_locals,
                        &import_targets,
                    )
                } else {
                    (Vec::new(), Vec::new())
                };
                augment_with_typeof_import_deps(&decl.body, &import_targets, &mut external_deps);

                // Phase 1b: Declaration merging — multiple interfaces with the
                // same name merge their members (TS declaration merging).
                if let Some(existing) = symbols.get_mut(name) {
                    if existing.kind == TypeDeclKind::Interface
                        && decl.kind == TypeDeclKind::Interface
                    {
                        let merged_member_deps = extract_member_deps(&decl.body);
                        // Merge bodies: combine members via Intersection
                        existing.raw_body = TypeExpr::intersection(vec![
                            existing.raw_body.clone(),
                            decl.body.clone(),
                        ]);
                        // Merge type parameters (keep first decl's params,
                        // add any new params from subsequent declarations)
                        for param in &decl.type_parameters {
                            if !existing
                                .type_parameters
                                .iter()
                                .any(|p| p.name == param.name)
                            {
                                existing.type_parameters.push(param.clone());
                            }
                        }
                        // Merge local deps
                        for dep in &local_deps {
                            if !existing.local_deps.contains(dep) {
                                existing.local_deps.push(dep.clone());
                            }
                        }
                        // Merge external deps
                        for dep in &external_deps {
                            if !existing.external_deps.contains(dep) {
                                existing.external_deps.push(dep.clone());
                            }
                        }
                        for (member, deps) in merged_member_deps {
                            let entry = existing.member_deps.entry(member).or_default();
                            for dep in deps {
                                if !entry.contains(&dep) {
                                    entry.push(dep);
                                }
                            }
                        }
                        continue;
                    }
                }
                let member_deps = extract_member_deps(&decl.body);
                symbols.insert(
                    name.clone(),
                    ShallowTypeSymbol {
                        kind: decl.kind,
                        raw_body: decl.body.clone(),
                        type_parameters: decl.type_parameters.clone(),
                        local_deps,
                        external_deps,
                        member_deps,
                    },
                );
            }

            for (name, decl) in &env.value_symbols {
                // For enum values, extract member names from the corresponding
                // type symbol's union body (TypeScript enums are dual-space:
                // type = union, value = object with member lookup).
                let enum_members = if decl.kind == ValueDeclKind::Enum {
                    env.type_symbols
                        .get(name)
                        .and_then(|type_decl| extract_enum_members_from_type_body(&type_decl.body))
                } else {
                    None
                };
                value_symbols.insert(
                    name.clone(),
                    ShallowValueSymbol {
                        kind: decl.kind,
                        type_annotation: decl.type_annotation.clone(),
                        function_signature: decl.function_signature.clone(),
                        object_shape: decl.object_shape.clone(),
                        enum_members,
                    },
                );
            }
        }
        // Without an eval env or source fallback, the shallow state can still
        // expose export/import routing but cannot populate declaration bodies.
        // The analysis-level symbol data (AnalyzedExternalTypeSymbol) does not
        // carry TypeExpr bodies â€” only dependency metadata. Callers that need
        // symbol bodies must build the shallow state through the eval-env path.

        Self {
            whole_hash,
            exports,
            wildcard_reexports,
            symbols,
            value_symbols,
            import_locals,
            import_targets,
            analysis,
        }
    }

    // -----------------------------------------------------------------------
    // Export routing
    // -----------------------------------------------------------------------

    /// Look up a named export. Returns `None` if the name is not directly
    /// exported (may still be available through wildcard reexports).
    pub fn export_target(&self, name: &str) -> Option<&ExportTarget> {
        self.exports.get(name)
    }

    /// Get the narrow type-resolution view over this file state.
    pub fn type_view(&self) -> ShallowTypeView<'_> {
        ShallowTypeView { state: self }
    }

    /// Whether this file has any wildcard re-exports.
    pub fn has_wildcard_reexports(&self) -> bool {
        !self.wildcard_reexports.is_empty()
    }

    /// Look up a local symbol by name.
    pub fn symbol(&self, name: &str) -> Option<&ShallowTypeSymbol> {
        self.symbols.get(name)
    }

    /// Look up a local value symbol by name.
    pub fn value_symbol(&self, name: &str) -> Option<&ShallowValueSymbol> {
        self.value_symbols.get(name)
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
        let mut external_refs = Vec::new();
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
                    unresolved_external: external_refs,
                    steps: steps as u64,
                };
            }

            if let Some(sym) = self.symbols.get(&current) {
                local_used.push(current.clone());

                // Queue same-file dependencies
                for dep in &sym.local_deps {
                    if !visited.contains(dep.as_str()) {
                        pending.push(dep.clone());
                    }
                }

                // Collect external refs
                for ext in &sym.external_deps {
                    if !external_refs.iter().any(|e: &ExternalSymbolRef| {
                        e.source_specifier == ext.source_specifier
                            && e.imported_name == ext.imported_name
                    }) {
                        external_refs.push(ext.clone());
                    }
                }
            } else if self.import_locals.contains(&current) {
                // This is an import — classify as external
                if let Some(target) = self.import_targets.get(&current) {
                    let ext_ref = ExternalSymbolRef {
                        local_name: current.clone(),
                        source_specifier: target.source_specifier.clone(),
                        imported_name: target.imported_name.clone(),
                        canonical_id: target.canonical_id.clone(),
                    };
                    if !external_refs.iter().any(|e| {
                        e.source_specifier == ext_ref.source_specifier
                            && e.imported_name == ext_ref.imported_name
                    }) {
                        external_refs.push(ext_ref);
                    }
                } else {
                    // Import-local without a target — treat as missing
                    return LocalClosureResult {
                        status: LocalClosureStatus::MissingLocalSymbol { name: current },
                        local_symbols_used: local_used,
                        unresolved_external: external_refs,
                        steps: steps as u64,
                    };
                }
            } else {
                return LocalClosureResult {
                    status: LocalClosureStatus::MissingLocalSymbol { name: current },
                    local_symbols_used: local_used,
                    unresolved_external: external_refs,
                    steps: steps as u64,
                };
            }
        }

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
        route: &ExportedRoute,
        budget: usize,
    ) -> LocalClosureResult {
        match route {
            ExportedRoute::Whole => self.local_closure(symbol_name, budget),
            ExportedRoute::Member(member) => {
                self.member_route_closure(symbol_name, &[member.as_str()], budget)
            }
            ExportedRoute::Pick(members) => {
                let refs: Vec<&str> = members.iter().map(|s| s.as_str()).collect();
                self.member_route_closure(symbol_name, &refs, budget)
            }
            ExportedRoute::Omit(omitted) => {
                let Some(sym) = self.symbols.get(symbol_name) else {
                    return self.local_closure(symbol_name, budget);
                };
                let Some(members) = direct_object_member_names(&sym.raw_body) else {
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

    /// Internal: compute closure starting from specific member deps only.
    fn member_route_closure(
        &self,
        symbol_name: &str,
        members: &[&str],
        budget: usize,
    ) -> LocalClosureResult {
        let sym = match self.symbols.get(symbol_name) {
            Some(s) => s,
            None => return self.local_closure(symbol_name, budget),
        };

        // If no member_deps tracking, fall back to whole closure
        if sym.member_deps.is_empty() {
            return self.local_closure(symbol_name, budget);
        }

        // Collect the initial seed names from the requested members' deps
        let mut seed_names: Vec<String> = Vec::new();
        let mut saw_known_member = false;
        for member in members {
            if let Some(deps) = sym.member_deps.get(*member) {
                saw_known_member = true;
                for dep in deps {
                    if !seed_names.contains(dep) {
                        seed_names.push(dep.clone());
                    }
                }
                continue;
            }
            if let Some(prop) = direct_object_property(&sym.raw_body, member) {
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
        let mut external_refs = Vec::new();
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
                    unresolved_external: external_refs,
                    steps,
                };
            }

            if let Some(dep_sym) = self.symbols.get(&current) {
                local_used.push(current.clone());
                for dep in &dep_sym.local_deps {
                    if !visited.contains(dep.as_str()) {
                        pending.push(dep.clone());
                    }
                }
                for ext in &dep_sym.external_deps {
                    if !external_refs.iter().any(|e: &ExternalSymbolRef| {
                        e.source_specifier == ext.source_specifier
                            && e.imported_name == ext.imported_name
                    }) {
                        external_refs.push(ext.clone());
                    }
                }
            } else if self.import_locals.contains(&current) {
                if let Some(target) = self.import_targets.get(&current) {
                    let ext_ref = ExternalSymbolRef {
                        local_name: current.clone(),
                        source_specifier: target.source_specifier.clone(),
                        imported_name: target.imported_name.clone(),
                        canonical_id: target.canonical_id.clone(),
                    };
                    if !external_refs.iter().any(|e| {
                        e.source_specifier == ext_ref.source_specifier
                            && e.imported_name == ext_ref.imported_name
                    }) {
                        external_refs.push(ext_ref);
                    }
                }
            }
            // Skip unknown names silently — they may be type parameters
        }

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

    /// Look up a local type symbol by name.
    pub fn symbol(self, name: &str) -> Option<&'a ShallowTypeSymbol> {
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

/// Extract enum member names and values from a union type body.
///
/// TypeScript enums produce a union of literal types as their type-space body.
/// This function extracts the member name → literal value mapping for the
/// value-space `enum_members` field.
fn extract_enum_members_from_type_body(body: &TypeExpr) -> Option<FxHashMap<String, TypeExpr>> {
    match body {
        TypeExpr::Union(members) => {
            let mut result = FxHashMap::default();
            for (i, member) in members.iter().enumerate() {
                match member {
                    TypeExpr::Literal(lit) => {
                        let name = match lit {
                            verter_semantic::analysis::type_expr::LiteralValue::String(s) => {
                                s.clone()
                            }
                            verter_semantic::analysis::type_expr::LiteralValue::Number(n) => {
                                format!("{n}")
                            }
                            verter_semantic::analysis::type_expr::LiteralValue::Boolean(b) => {
                                format!("{b}")
                            }
                            verter_semantic::analysis::type_expr::LiteralValue::BigInt(s) => {
                                s.clone()
                            }
                        };
                        result.insert(name, member.clone());
                    }
                    _ => {
                        // Non-literal member — use index as name
                        result.insert(format!("_{i}"), member.clone());
                    }
                }
            }
            if result.is_empty() {
                None
            } else {
                Some(result)
            }
        }
        _ => None,
    }
}

/// Extract per-member dependency names from direct object slices in the body.
/// For each direct property, collects all type names referenced in that
/// property's type annotation. Transparent intersections are flattened
/// right-to-left so declaration-merged interfaces keep earlier members while
/// later object slices win on duplicate names.
fn extract_member_deps(body: &TypeExpr) -> FxHashMap<String, Vec<String>> {
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
) -> Option<&'a verter_semantic::analysis::type_expr::ObjectProperty> {
    direct_object_properties(body)
        .into_iter()
        .find(|prop| prop.name == name)
}

fn direct_object_properties(
    body: &TypeExpr,
) -> Vec<&verter_semantic::analysis::type_expr::ObjectProperty> {
    let mut result = Vec::new();
    let mut seen = FxHashSet::default();
    collect_direct_object_properties(body, &mut result, &mut seen);
    result
}

fn collect_direct_object_properties<'a>(
    body: &'a TypeExpr,
    out: &mut Vec<&'a verter_semantic::analysis::type_expr::ObjectProperty>,
    seen: &mut FxHashSet<String>,
) {
    match body {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let verter_semantic::analysis::type_expr::ObjectMember::Property(prop) = member {
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
fn collect_type_refs(expr: &TypeExpr, out: &mut Vec<String>) {
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
                if let verter_semantic::analysis::type_expr::ObjectMember::Property(prop) = member {
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
        TypeExpr::Function(func) => {
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
        TypeExpr::TypeOf { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Infer { .. } => {}
    }
}

/// Classify a symbol's structural dependencies into local vs external.
fn classify_deps(
    symbol_name: &str,
    analysis: &verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource,
    sym: &verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbol,
    import_locals: &FxHashSet<String>,
    import_targets: &FxHashMap<String, ImportTarget>,
) -> (Vec<String>, Vec<ExternalSymbolRef>) {
    let mut local = analysis
        .local_symbol_dependency_names(symbol_name)
        .into_iter()
        .collect::<Vec<_>>();
    local.sort();

    let mut external = Vec::new();
    let mut seen_external = FxHashSet::default();

    for dep_name in sym
        .dependency_names
        .iter()
        .chain(sym.structural_dependency_names.iter())
    {
        let root_name = dep_name.split('.').next().unwrap_or(dep_name.as_str());
        if import_locals.contains(root_name) {
            if let Some(target) = import_targets.get(root_name) {
                let imported_name = if root_name == dep_name {
                    target.imported_name.clone()
                } else if let Some(suffix) = dep_name.strip_prefix(root_name) {
                    format!("{}{suffix}", target.imported_name)
                } else {
                    target.imported_name.clone()
                };
                if !seen_external.insert((target.source_specifier.clone(), imported_name.clone())) {
                    continue;
                }
                external.push(ExternalSymbolRef {
                    local_name: dep_name.clone(),
                    source_specifier: target.source_specifier.clone(),
                    imported_name,
                    canonical_id: target.canonical_id.clone(),
                });
            }
        }
    }

    external.sort_by(|left, right| {
        left.local_name
            .cmp(&right.local_name)
            .then_with(|| left.source_specifier.cmp(&right.source_specifier))
            .then_with(|| left.imported_name.cmp(&right.imported_name))
    });

    (local, external)
}

fn augment_with_typeof_import_deps(
    expr: &TypeExpr,
    import_targets: &FxHashMap<String, ImportTarget>,
    external: &mut Vec<ExternalSymbolRef>,
) {
    let mut roots = FxHashSet::default();
    collect_typeof_roots(expr, &mut roots);
    for root in roots {
        let Some(target) = import_targets.get(root.as_str()) else {
            continue;
        };
        let dep = ExternalSymbolRef {
            local_name: root.clone(),
            source_specifier: target.source_specifier.clone(),
            imported_name: target.imported_name.clone(),
            canonical_id: target.canonical_id.clone(),
        };
        if !external.contains(&dep) {
            external.push(dep);
        }
    }
    external.sort_by(|left, right| {
        left.local_name
            .cmp(&right.local_name)
            .then_with(|| left.source_specifier.cmp(&right.source_specifier))
            .then_with(|| left.imported_name.cmp(&right.imported_name))
    });
}

fn collect_typeof_roots(expr: &TypeExpr, out: &mut FxHashSet<String>) {
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
                    verter_semantic::analysis::type_expr::ObjectMember::Property(prop) => {
                        collect_typeof_roots(&prop.ty, out);
                    }
                    verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(sig) => {
                        collect_typeof_roots(&sig.key_type, out);
                        collect_typeof_roots(&sig.value_type, out);
                    }
                    verter_semantic::analysis::type_expr::ObjectMember::CallSignature(func)
                    | verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(
                        func,
                    ) => {
                        collect_typeof_roots_in_function(func, out);
                    }
                    verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                        collect_typeof_roots_in_function(&method.function, out);
                    }
                }
            }
        }
        TypeExpr::Function(func) => collect_typeof_roots_in_function(func, out),
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
        | TypeExpr::Unknown { .. } => {}
    }
}

fn collect_typeof_roots_in_function(
    func: &verter_semantic::analysis::type_expr::FunctionExpr,
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
            state.symbols.is_empty(),
            "analysis-only construction should produce no symbols"
        );
        // But exports should still be populated
        assert!(state.export_target("Props").is_some());
    }

    #[test]
    fn source_backed_construction_populates_symbols_without_caller_env() {
        let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
        let analysis = make_analysis(source);
        let state = ShallowFileState::from_analysis_with_source(
            Hash16::default(),
            analysis,
            Some(source),
            None,
        );

        assert!(
            state.symbol("Props").is_some(),
            "source-backed construction should populate type symbols without a caller-provided env"
        );
        let defaults = state
            .value_symbol("defaults")
            .expect("source-backed construction should populate value symbols");
        assert_eq!(defaults.kind, ValueDeclKind::Const);
        assert!(defaults.type_annotation.is_some());
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
        let button = state.symbol("Button").expect("Button should exist");

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
        assert!(defaults.type_annotation.is_some());
        assert!(defaults.object_shape.is_some());

        let make_props = state
            .value_symbol("makeProps")
            .expect("makeProps value symbol should be present");
        assert_eq!(make_props.kind, ValueDeclKind::Function);
        assert!(make_props.function_signature.is_some());
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
    fn duplicate_interface_names_handled_without_panic() {
        // The eval env deduplicates interface declarations (HashMap last-wins).
        // The merging code in from_analysis is a safety net for future env
        // changes that might preserve duplicates. This test verifies that
        // duplicate names don't cause panics or data loss.
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
        // EvalEnv does last-wins for same-name symbols, so only the second
        // interface's body is present. The merging code in from_analysis
        // would merge if the env produced duplicates.
        assert!(
            !matches!(symbol.raw_body, TypeExpr::Unknown { .. }),
            "body should not be Unknown"
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

        let sym = state.symbols.get("CheckboxProps").expect("CheckboxProps");

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
        let closure_a = state.route_closure("Props", &ExportedRoute::Member("a".into()), 500);
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

        // Route::Whole should include both
        let closure_whole = state.route_closure("Props", &ExportedRoute::Whole, 500);
        let ext_names_whole: Vec<&str> = closure_whole
            .unresolved_external
            .iter()
            .map(|e| e.imported_name.as_str())
            .collect();
        assert!(
            ext_names_whole.contains(&"Alpha") && ext_names_whole.contains(&"Beta"),
            "Whole should include both Alpha and Beta, got {:?}",
            ext_names_whole
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
            &ExportedRoute::Pick(vec!["x".into(), "z".into()]),
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

        let closure = state.route_closure("Props", &ExportedRoute::Omit(vec!["y".into()]), 500);
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

        let closure = state.route_closure("Props", &ExportedRoute::Member("color".into()), 500);
        assert!(closure.unresolved_external.is_empty());
        assert_eq!(closure.local_symbols_used, vec!["Props".to_string()]);
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
        let state = ShallowFileState::from_analysis_with_resolver(
            Hash16::default(),
            analysis,
            None,
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
        let props_sym = state.symbol("Props").expect("Props symbol should exist");
        let foo_ext = props_sym
            .external_deps
            .iter()
            .find(|dep| dep.local_name == "Foo")
            .expect("Props should have Foo as an external dep");
        assert_eq!(
            foo_ext.canonical_id, "/resolved/bar.ts",
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

        let closure = state.route_closure("Props", &ExportedRoute::Member("inherited".into()), 500);
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
