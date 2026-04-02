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
/// Keyed by `(canonical_id, whole_hash)`.  Invalidated when the file's
/// whole-hash changes.
#[derive(Debug, Clone)]
pub struct ShallowFileState {
    /// Content hash of the source that produced this state.
    pub whole_hash: Hash16,

    /// Named exports: exported name â†’ routing target.
    pub exports: FxHashMap<String, ExportTarget>,

    /// `export * from` sources, in declaration order.
    pub wildcard_reexports: Vec<String>,

    /// All locally-declared type symbols (exported or internal).
    pub symbols: FxHashMap<String, ShallowTypeSymbol>,

    /// All locally-declared value symbols that may participate in `typeof`
    /// queries or value-driven type expansion.
    pub value_symbols: FxHashMap<String, ShallowValueSymbol>,

    /// Import-local names (names that come from `import` declarations).
    /// Used to classify dependencies as local vs external during closure.
    pub import_locals: FxHashSet<String>,

    /// Import specifier targets: local import name â†’ (source_specifier, imported_name).
    pub import_targets: FxHashMap<String, (String, String)>,

    /// The underlying analyzed source (retained for methods that still need
    /// the full analysis surface during the transition).
    pub analysis: Arc<AnalyzedExternalTypeSource>,
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
    },
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

impl ShallowFileState {
    /// Build from an existing `AnalyzedExternalTypeSource` and export-routing data.
    ///
    /// This is the primary construction path: the host ensures the file is loaded
    /// and analyzed, then builds the shallow state from that analysis.
    pub fn from_analysis(
        whole_hash: Hash16,
        analysis: Arc<AnalyzedExternalTypeSource>,
        eval_env: Option<&verter_semantic::analysis::type_eval::EvalEnv>,
    ) -> Self {
        let mut exports = FxHashMap::default();
        let mut wildcard_reexports = Vec::new();
        let mut import_locals = FxHashSet::default();
        let mut import_targets = FxHashMap::default();
        let mut symbols = FxHashMap::default();
        let mut value_symbols = FxHashMap::default();

        // Populate exports from the extracted bindings
        // Direct reexports
        for (exported_name, source, original) in analysis.direct_reexport_entries() {
            exports.insert(
                exported_name.to_string(),
                ExportTarget::Reexport {
                    source_specifier: source.to_string(),
                    original_name: original.to_string(),
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

        // Wildcard reexport sources (in declaration order)
        wildcard_reexports.extend(analysis.wildcard_reexport_sources().iter().cloned());

        // Import locals and targets
        for binding in &analysis.extracted.bindings {
            import_locals.insert(binding.local_name.clone());
            import_targets.insert(
                binding.local_name.clone(),
                (binding.source.clone(), binding.imported_name.clone()),
            );
        }

        // Locally-declared symbols from eval env (if available)
        if let Some(env) = eval_env {
            for (name, decl) in &env.type_symbols {
                let local_type_sym = analysis.local_type_symbol(name);
                let (local_deps, external_deps) = if let Some(sym) = local_type_sym {
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

                symbols.insert(
                    name.clone(),
                    ShallowTypeSymbol {
                        kind: decl.kind,
                        raw_body: decl.body.clone(),
                        type_parameters: decl.type_parameters.clone(),
                        local_deps,
                        external_deps,
                    },
                );
            }

            for (name, decl) in &env.value_symbols {
                value_symbols.insert(
                    name.clone(),
                    ShallowValueSymbol {
                        kind: decl.kind,
                        type_annotation: decl.type_annotation.clone(),
                        function_signature: decl.function_signature.clone(),
                        object_shape: decl.object_shape.clone(),
                    },
                );
            }
        }
        // Note: when no eval_env is provided, symbols stay empty.
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
    pub fn import_target(&self, local_name: &str) -> Option<&(String, String)> {
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
                // This is an import â€” classify as external
                if let Some((source, imported)) = self.import_targets.get(&current) {
                    let ext_ref = ExternalSymbolRef {
                        local_name: current.clone(),
                        source_specifier: source.clone(),
                        imported_name: imported.clone(),
                    };
                    if !external_refs.iter().any(|e| {
                        e.source_specifier == ext_ref.source_specifier
                            && e.imported_name == ext_ref.imported_name
                    }) {
                        external_refs.push(ext_ref);
                    }
                } else {
                    // Import-local without a target â€” treat as missing
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
}

impl<'a> ShallowTypeView<'a> {
    /// Look up a named export. Returns `None` if the name is not directly
    /// exported (may still be available through wildcard reexports).
    pub fn export_target(self, name: &str) -> Option<&'a ExportTarget> {
        self.state.export_target(name)
    }

    /// Wildcard `export *` sources in declaration order.
    pub fn wildcard_reexports(self) -> &'a [String] {
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

/// Classify a symbol's structural dependencies into local vs external.
fn classify_deps(
    symbol_name: &str,
    analysis: &verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource,
    sym: &verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbol,
    import_locals: &FxHashSet<String>,
    import_targets: &FxHashMap<String, (String, String)>,
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
            if let Some((source, imported)) = import_targets.get(root_name) {
                let imported_name = if root_name == dep_name {
                    imported.clone()
                } else if let Some(suffix) = dep_name.strip_prefix(root_name) {
                    format!("{imported}{suffix}")
                } else {
                    imported.clone()
                };
                if !seen_external.insert((source.clone(), imported_name.clone())) {
                    continue;
                }
                external.push(ExternalSymbolRef {
                    local_name: dep_name.clone(),
                    source_specifier: source.clone(),
                    imported_name,
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

        assert_eq!(
            state.wildcard_reexports,
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
            state.wildcard_reexports.contains(&"./wildcard".to_string()),
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
        assert_eq!(
            state.import_target("Alpha"),
            Some(&("./a".to_string(), "Alpha".to_string()))
        );

        assert!(state.is_import_local("B"));
        assert_eq!(
            state.import_target("B"),
            Some(&("./b".to_string(), "Beta".to_string()))
        );
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
}
