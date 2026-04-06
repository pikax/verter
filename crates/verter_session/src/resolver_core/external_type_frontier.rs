//! Unified external type frontier engine (Phase 3).
//!
//! Shared BFS kernel for cross-file type deepening.
//! Host-backed resolver entrypoints and merge-root collection can use this
//! frontier to discover the import graph level-by-level while keeping
//! same-file closure local to the owning file.
//!
//! The frontier traversal:
//!
//! - Deduplicates `(canonical_id, exported_name)` across the entire request
//! - Resolves export routing: direct named > aliased > wildcard (declared order)
//! - Runs local closure per file, never crossing import boundaries in-place
//! - Collects external refs into the next frontier level
//! - Tracks counters against the frontier budget
//!
//! The engine does NOT perform type evaluation or expansion.  It produces
//! `ResolvedSymbol` entries with post-local-closure symbolic bodies that the
//! builder stage (Phase 6) consumes from cache.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::shallow_file_state::{
    BudgetDomain, BudgetExceededFailure, ExportTarget, ExternalSymbolRef, LocalClosureStatus,
    ResolutionBudgets, ResolutionCounters, ShallowFileState,
};
use verter_semantic::analysis::type_expr::{TypeExpr, TypeParam};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A pending symbol to resolve in the next frontier level.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PendingExternalSymbol {
    pub canonical_id: String,
    pub exported_name: String,
}

/// How a symbol's export route was discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteKind {
    /// Directly exported from the file.
    Direct,
    /// Exported as an alias (`export { X as Y }`).
    Alias,
    /// Found through a wildcard `export *` chain.
    Wildcard {
        /// The barrel file that declared the `export *`.
        barrel_canonical_id: String,
        /// Position in the barrel's wildcard source list (declared order).
        source_order: usize,
    },
}

/// Provenance of the route that resolved a symbol.
#[derive(Debug, Clone)]
pub struct ResolvedRouteProvenance {
    /// How the export route was discovered.
    pub kind: RouteKind,
    /// The file that ultimately defines the symbol.
    pub defining_canonical_id: String,
    /// The defining file's name for this symbol (may differ from exported name
    /// due to aliases).
    pub defining_name: String,
}

/// Status of a resolved symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedSymbolStatus {
    /// Fully resolved (post-local-closure, no remaining external deps
    /// within this symbol's local scope).
    Resolved,
    /// Resolved but some external deps within the symbol's local scope
    /// could not be followed (e.g., missing files).
    ResolvedWithUnresolvedExternal,
    /// Export routing did not find a matching symbol.
    RouteNotFound,
    /// Routing found a target but local closure failed.
    InvalidDeclaration,
    /// Budget was exceeded during resolution.
    BudgetExceeded,
}

/// A fully resolved symbol from the frontier.
#[derive(Debug, Clone)]
pub struct ResolvedSymbol {
    pub canonical_id: String,
    pub exported_name: String,
    pub status: ResolvedSymbolStatus,
    /// Post-local-closure symbolic body for locally-defined symbols.
    pub body: Option<TypeExpr>,
    /// Generic parameters on the defining local symbol.
    pub type_parameters: Vec<TypeParam>,
    /// External refs that need resolution in subsequent levels.
    pub unresolved_external: Vec<ExternalSymbolRef>,
    /// Route provenance for invalidation and observability.
    pub route_provenance: Option<ResolvedRouteProvenance>,
}

// ---------------------------------------------------------------------------
// Frontier state
// ---------------------------------------------------------------------------

/// The BFS frontier engine.
pub struct ExternalTypeFrontier {
    /// Current level of pending symbols to process.
    current_level: Vec<PendingExternalSymbol>,
    /// Next level accumulated from unresolved external refs.
    next_level: Vec<PendingExternalSymbol>,
    /// All resolved symbols, keyed by `(canonical_id, exported_name)`.
    pub resolved: FxHashMap<(String, String), ResolvedSymbol>,
    /// Set of `(canonical_id, exported_name)` pairs that have been seen
    /// (either resolved or currently pending).
    seen: FxHashSet<(String, String)>,
    /// Per-request counters.
    pub counters: ResolutionCounters,
    /// Budget limits.
    budgets: ResolutionBudgets,
    /// If a budget was exceeded, the failure details.
    pub budget_failure: Option<BudgetExceededFailure>,
}

/// Trait for the host to provide file state to the frontier engine.
///
/// The frontier engine never performs file I/O itself — all file state
/// comes through this trait.  Cross-file edges are pre-canonicalized on
/// the `ShallowFileState` when possible, so most traversal runs without
/// calling `resolve_import_canonical`.  The fallback is only used for
/// edges whose canonical ID could not be resolved at construction time.
pub trait FrontierHost {
    /// Get or build the shallow type state for a canonical file.
    fn ensure_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>>;

    /// Fallback: resolve an import specifier to its canonical ID.
    ///
    /// Only called for edges whose `canonical_id` was empty at construction
    /// time (e.g., files loaded before their dependency graph was fully
    /// materialized).  Production hosts should ensure shallow states are
    /// built with a resolver so this is rarely hit.
    fn resolve_import_canonical(&self, _from_canonical: &str, _specifier: &str) -> Option<String> {
        None
    }

    /// Route-only traversals stop once they reach the defining export target.
    /// They still follow alias and reexport edges, but they do not widen into
    /// the defining symbol's dependency graph.
    fn route_exports_only(&self) -> bool {
        false
    }
}

fn wildcard_source_stem(source: &str) -> &str {
    let leaf = source.rsplit('/').next().unwrap_or(source);
    leaf.strip_suffix(".d.ts")
        .or_else(|| leaf.strip_suffix(".d.mts"))
        .or_else(|| leaf.strip_suffix(".d.cts"))
        .or_else(|| leaf.strip_suffix(".vue"))
        .or_else(|| leaf.strip_suffix(".mts"))
        .or_else(|| leaf.strip_suffix(".cts"))
        .or_else(|| leaf.strip_suffix(".ts"))
        .or_else(|| leaf.strip_suffix(".js"))
        .unwrap_or(leaf)
}

fn pascalize_wildcard_source_stem(stem: &str) -> String {
    let mut result = String::with_capacity(stem.len());
    let mut uppercase_next = true;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                result.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                result.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }
    result
}

fn wildcard_source_hint_prefix_len(source: &str, exported_name: &str) -> usize {
    let stem = wildcard_source_stem(source);
    let mut best = if exported_name.starts_with(stem) {
        stem.len()
    } else {
        0
    };
    let pascal = pascalize_wildcard_source_stem(stem);
    if !pascal.is_empty() && exported_name.starts_with(&pascal) {
        best = best.max(pascal.len());
    }
    best
}

fn ordered_wildcard_sources<'a>(
    exported_name: &str,
    wildcard_sources: &'a [super::shallow_file_state::WildcardReexport],
) -> Vec<(usize, &'a super::shallow_file_state::WildcardReexport)> {
    let mut ordered: Vec<_> = wildcard_sources.iter().enumerate().collect();
    ordered.sort_by(|(left_index, left_wc), (right_index, right_wc)| {
        wildcard_source_hint_prefix_len(&right_wc.source_specifier, exported_name)
            .cmp(&wildcard_source_hint_prefix_len(
                &left_wc.source_specifier,
                exported_name,
            ))
            .then_with(|| left_index.cmp(right_index))
    });
    ordered
}

impl ExternalTypeFrontier {
    /// Create a new frontier with default budgets.
    pub fn new() -> Self {
        Self::with_budgets(ResolutionBudgets::default())
    }

    /// Create a new frontier with custom budgets.
    pub fn with_budgets(budgets: ResolutionBudgets) -> Self {
        Self {
            current_level: Vec::new(),
            next_level: Vec::new(),
            resolved: FxHashMap::default(),
            seen: FxHashSet::default(),
            counters: ResolutionCounters::default(),
            budgets,
            budget_failure: None,
        }
    }

    /// Seed the frontier with initial symbols to resolve.
    pub fn seed(&mut self, symbols: impl IntoIterator<Item = PendingExternalSymbol>) {
        for sym in symbols {
            let key = (sym.canonical_id.clone(), sym.exported_name.clone());
            if self.seen.insert(key) {
                self.current_level.push(sym);
            }
        }
    }

    /// Run the frontier to completion.
    ///
    /// Returns `Ok(())` if all reachable symbols were processed (or budget
    /// was exceeded), `Err` for host-level failures.
    pub fn run<H: FrontierHost>(&mut self, host: &H) -> Result<(), BudgetExceededFailure> {
        while !self.current_level.is_empty() {
            self.process_level(host)?;

            // Swap levels
            self.current_level.clear();
            std::mem::swap(&mut self.current_level, &mut self.next_level);
        }
        Ok(())
    }

    /// Process one level of the frontier.
    fn process_level<H: FrontierHost>(&mut self, host: &H) -> Result<(), BudgetExceededFailure> {
        // Take ownership of current level to avoid borrow issues
        let level: Vec<_> = self.current_level.drain(..).collect();

        for pending in level {
            // Budget check
            self.counters.frontier_symbol_visits += 1;
            if self.counters.frontier_symbol_visits >= self.budgets.frontier_symbol_visits as u64 {
                let failure = BudgetExceededFailure {
                    domain: BudgetDomain::Frontier,
                    limit: self.budgets.frontier_symbol_visits,
                    actual: self.counters.frontier_symbol_visits,
                    context: format!("{}::{}", pending.canonical_id, pending.exported_name),
                };
                self.budget_failure = Some(failure.clone());
                return Err(failure);
            }

            let resolved = self.resolve_one(host, &pending);

            // Enqueue external refs from this symbol into next level.
            // External refs carry pre-canonicalized target IDs when available.
            // For edges that couldn't be canonicalized at construction time,
            // fall back to the host's resolve_import_canonical.
            for ext_ref in &resolved.unresolved_external {
                let target_canonical = if !ext_ref.canonical_id.is_empty() {
                    ext_ref.canonical_id.clone()
                } else if let Some(resolved_id) =
                    host.resolve_import_canonical(&pending.canonical_id, &ext_ref.source_specifier)
                {
                    resolved_id
                } else {
                    continue;
                };
                let next = PendingExternalSymbol {
                    canonical_id: target_canonical.clone(),
                    exported_name: ext_ref.imported_name.clone(),
                };
                let key = (next.canonical_id.clone(), next.exported_name.clone());
                if self.seen.insert(key) {
                    self.next_level.push(next);
                }
            }

            // Store resolved symbol
            self.resolved.insert(
                (
                    resolved.canonical_id.clone(),
                    resolved.exported_name.clone(),
                ),
                resolved,
            );
        }

        Ok(())
    }

    /// Resolve one `(canonical_id, exported_name)` pair.
    fn resolve_one<H: FrontierHost>(
        &mut self,
        host: &H,
        pending: &PendingExternalSymbol,
    ) -> ResolvedSymbol {
        let Some(state) = host.ensure_shallow_state(&pending.canonical_id) else {
            return ResolvedSymbol {
                canonical_id: pending.canonical_id.clone(),
                exported_name: pending.exported_name.clone(),
                status: ResolvedSymbolStatus::RouteNotFound,
                body: None,
                type_parameters: Vec::new(),
                unresolved_external: Vec::new(),
                route_provenance: None,
            };
        };

        let type_view = state.type_view();

        // Step 1: Try direct/alias export routing
        if let Some(target) = type_view.export_target(&pending.exported_name) {
            return self.resolve_through_export(host, pending, &state, target);
        }

        // Step 2: Try wildcard reexport routing using pre-canonicalized edges.
        // Fall back to host resolution for edges with empty canonical IDs.
        for (order, wildcard) in
            ordered_wildcard_sources(&pending.exported_name, type_view.wildcard_reexports())
        {
            let target_canonical_owned;
            let target_canonical = if !wildcard.canonical_id.is_empty() {
                &wildcard.canonical_id
            } else if let Some(resolved) =
                host.resolve_import_canonical(&pending.canonical_id, &wildcard.source_specifier)
            {
                target_canonical_owned = resolved;
                &target_canonical_owned
            } else {
                continue;
            };

            // Check if already resolved from this chain
            let key = (target_canonical.clone(), pending.exported_name.clone());
            if let Some(existing) = self.resolved.get(&key) {
                if existing.status == ResolvedSymbolStatus::Resolved
                    || existing.status == ResolvedSymbolStatus::ResolvedWithUnresolvedExternal
                {
                    return ResolvedSymbol {
                        canonical_id: pending.canonical_id.clone(),
                        exported_name: pending.exported_name.clone(),
                        status: existing.status.clone(),
                        body: existing.body.clone(),
                        type_parameters: existing.type_parameters.clone(),
                        unresolved_external: existing.unresolved_external.clone(),
                        route_provenance: Some(ResolvedRouteProvenance {
                            kind: RouteKind::Wildcard {
                                barrel_canonical_id: pending.canonical_id.clone(),
                                source_order: order,
                            },
                            defining_canonical_id: target_canonical.clone(),
                            defining_name: pending.exported_name.clone(),
                        }),
                    };
                }
            }

            // Try resolving in the wildcard target
            if let Some(wc_state) = host.ensure_shallow_state(target_canonical) {
                if wc_state
                    .type_view()
                    .export_target(&pending.exported_name)
                    .is_some()
                {
                    // Found it — enqueue as next-level work from the target file
                    let next = PendingExternalSymbol {
                        canonical_id: target_canonical.clone(),
                        exported_name: pending.exported_name.clone(),
                    };
                    let key = (next.canonical_id.clone(), next.exported_name.clone());
                    if self.seen.insert(key) {
                        self.next_level.push(next);
                    }

                    return ResolvedSymbol {
                        canonical_id: pending.canonical_id.clone(),
                        exported_name: pending.exported_name.clone(),
                        status: ResolvedSymbolStatus::ResolvedWithUnresolvedExternal,
                        body: None,
                        type_parameters: Vec::new(),
                        unresolved_external: vec![ExternalSymbolRef {
                            local_name: pending.exported_name.clone(),
                            source_specifier: wildcard.source_specifier.clone(),
                            imported_name: pending.exported_name.clone(),
                            canonical_id: target_canonical.clone(),
                        }],
                        route_provenance: Some(ResolvedRouteProvenance {
                            kind: RouteKind::Wildcard {
                                barrel_canonical_id: pending.canonical_id.clone(),
                                source_order: order,
                            },
                            defining_canonical_id: target_canonical.clone(),
                            defining_name: pending.exported_name.clone(),
                        }),
                    };
                }

                // Check if the wildcard target itself has wildcards (recursive barrel)
                if wc_state.has_wildcard_reexports() {
                    let next = PendingExternalSymbol {
                        canonical_id: target_canonical.clone(),
                        exported_name: pending.exported_name.clone(),
                    };
                    let key = (next.canonical_id.clone(), next.exported_name.clone());
                    if self.seen.insert(key) {
                        self.next_level.push(next);
                    }
                }
            }
        }

        // Not found through any route
        ResolvedSymbol {
            canonical_id: pending.canonical_id.clone(),
            exported_name: pending.exported_name.clone(),
            status: ResolvedSymbolStatus::RouteNotFound,
            body: None,
            type_parameters: Vec::new(),
            unresolved_external: Vec::new(),
            route_provenance: None,
        }
    }

    /// Resolve a symbol through a known export target.
    fn resolve_through_export<H: FrontierHost>(
        &mut self,
        host: &H,
        pending: &PendingExternalSymbol,
        state: &Arc<ShallowFileState>,
        target: &ExportTarget,
    ) -> ResolvedSymbol {
        match target {
            ExportTarget::Local { symbol_name } => {
                if state.is_import_local(symbol_name) {
                    if let Some(import_target) = state.import_target(symbol_name) {
                        let resolved_canonical = if !import_target.canonical_id.is_empty() {
                            Some(import_target.canonical_id.clone())
                        } else {
                            host.resolve_import_canonical(
                                &pending.canonical_id,
                                &import_target.source_specifier,
                            )
                        };
                        if let Some(ref target_canonical) = resolved_canonical {
                            let next = PendingExternalSymbol {
                                canonical_id: target_canonical.clone(),
                                exported_name: import_target.imported_name.clone(),
                            };
                            let key = (next.canonical_id.clone(), next.exported_name.clone());
                            if self.seen.insert(key) {
                                self.next_level.push(next);
                            }

                            return ResolvedSymbol {
                                canonical_id: pending.canonical_id.clone(),
                                exported_name: pending.exported_name.clone(),
                                status: ResolvedSymbolStatus::ResolvedWithUnresolvedExternal,
                                body: None,
                                type_parameters: Vec::new(),
                                unresolved_external: vec![ExternalSymbolRef {
                                    local_name: symbol_name.clone(),
                                    source_specifier: import_target.source_specifier.clone(),
                                    imported_name: import_target.imported_name.clone(),
                                    canonical_id: target_canonical.clone(),
                                }],
                                route_provenance: Some(ResolvedRouteProvenance {
                                    kind: RouteKind::Alias,
                                    defining_canonical_id: target_canonical.clone(),
                                    defining_name: import_target.imported_name.clone(),
                                }),
                            };
                        }
                    }
                }

                if host.route_exports_only() {
                    let (body, type_parameters) = state
                        .type_view()
                        .symbol(symbol_name)
                        .map(|symbol| {
                            (
                                Some(symbol.raw_body.clone()),
                                symbol.type_parameters.clone(),
                            )
                        })
                        .unwrap_or_else(|| (None, Vec::new()));

                    return ResolvedSymbol {
                        canonical_id: pending.canonical_id.clone(),
                        exported_name: pending.exported_name.clone(),
                        status: ResolvedSymbolStatus::Resolved,
                        body,
                        type_parameters,
                        unresolved_external: Vec::new(),
                        route_provenance: Some(ResolvedRouteProvenance {
                            kind: RouteKind::Direct,
                            defining_canonical_id: pending.canonical_id.clone(),
                            defining_name: symbol_name.clone(),
                        }),
                    };
                }

                let (body, type_parameters) = state
                    .type_view()
                    .symbol(symbol_name)
                    .map(|symbol| {
                        (
                            Some(symbol.raw_body.clone()),
                            symbol.type_parameters.clone(),
                        )
                    })
                    .unwrap_or_else(|| (None, Vec::new()));

                // Run local closure
                let closure = state
                    .type_view()
                    .local_closure(symbol_name, self.budgets.local_closure_steps);
                self.counters.local_closure_steps += closure.steps;

                let status = match &closure.status {
                    LocalClosureStatus::Resolved => ResolvedSymbolStatus::Resolved,
                    LocalClosureStatus::ResolvedWithExternalDeps => {
                        ResolvedSymbolStatus::ResolvedWithUnresolvedExternal
                    }
                    LocalClosureStatus::BudgetExceeded => ResolvedSymbolStatus::BudgetExceeded,
                    LocalClosureStatus::MissingLocalSymbol { .. } => {
                        ResolvedSymbolStatus::InvalidDeclaration
                    }
                };

                ResolvedSymbol {
                    canonical_id: pending.canonical_id.clone(),
                    exported_name: pending.exported_name.clone(),
                    status,
                    body,
                    type_parameters,
                    unresolved_external: closure.unresolved_external,
                    route_provenance: Some(ResolvedRouteProvenance {
                        kind: RouteKind::Direct,
                        defining_canonical_id: pending.canonical_id.clone(),
                        defining_name: symbol_name.clone(),
                    }),
                }
            }
            ExportTarget::Reexport {
                source_specifier,
                original_name,
                canonical_id: reexport_canonical,
                ..
            } => {
                // Follow the reexport using pre-canonicalized target, with fallback
                let effective_canonical = if !reexport_canonical.is_empty() {
                    Some(reexport_canonical.clone())
                } else {
                    host.resolve_import_canonical(&pending.canonical_id, source_specifier)
                };
                if let Some(ref reexport_canonical) = effective_canonical {
                    let next = PendingExternalSymbol {
                        canonical_id: reexport_canonical.clone(),
                        exported_name: original_name.clone(),
                    };
                    let key = (next.canonical_id.clone(), next.exported_name.clone());
                    if self.seen.insert(key) {
                        self.next_level.push(next);
                    }

                    ResolvedSymbol {
                        canonical_id: pending.canonical_id.clone(),
                        exported_name: pending.exported_name.clone(),
                        status: ResolvedSymbolStatus::ResolvedWithUnresolvedExternal,
                        body: None,
                        type_parameters: Vec::new(),
                        unresolved_external: vec![ExternalSymbolRef {
                            local_name: pending.exported_name.clone(),
                            source_specifier: source_specifier.clone(),
                            imported_name: original_name.clone(),
                            canonical_id: reexport_canonical.clone(),
                        }],
                        route_provenance: Some(ResolvedRouteProvenance {
                            kind: RouteKind::Alias,
                            defining_canonical_id: reexport_canonical.clone(),
                            defining_name: original_name.clone(),
                        }),
                    }
                } else {
                    ResolvedSymbol {
                        canonical_id: pending.canonical_id.clone(),
                        exported_name: pending.exported_name.clone(),
                        status: ResolvedSymbolStatus::RouteNotFound,
                        body: None,
                        type_parameters: Vec::new(),
                        unresolved_external: Vec::new(),
                        route_provenance: None,
                    }
                }
            }
        }
    }

    /// Number of resolved symbols.
    pub fn resolved_count(&self) -> usize {
        self.resolved.len()
    }

    /// Get a resolved symbol by key.
    pub fn get_resolved(&self, canonical_id: &str, exported_name: &str) -> Option<&ResolvedSymbol> {
        self.resolved
            .get(&(canonical_id.to_string(), exported_name.to_string()))
    }

    /// Follow the resolved route chain for one exported symbol until it reaches
    /// the final defining symbol.
    pub fn final_target_for<H: FrontierHost>(
        &self,
        host: &H,
        canonical_id: &str,
        exported_name: &str,
    ) -> Option<(String, String)> {
        let mut seen = FxHashSet::default();

        self.final_target_from(host, canonical_id, exported_name, &mut seen)
    }

    fn final_target_from<H: FrontierHost>(
        &self,
        host: &H,
        canonical_id: &str,
        exported_name: &str,
        seen: &mut FxHashSet<(String, String)>,
    ) -> Option<(String, String)> {
        let current = (canonical_id.to_string(), exported_name.to_string());

        if !seen.insert(current.clone()) {
            return None;
        }

        let resolved = self.get_resolved(&current.0, &current.1)?;
        if let Some(provenance) = resolved.route_provenance.as_ref() {
            return match provenance.kind {
                RouteKind::Direct => Some((
                    provenance.defining_canonical_id.clone(),
                    provenance.defining_name.clone(),
                )),
                RouteKind::Alias | RouteKind::Wildcard { .. } => self.final_target_from(
                    host,
                    &provenance.defining_canonical_id,
                    &provenance.defining_name,
                    seen,
                ),
            };
        }

        // Fall back to wildcard reexport edges using canonical IDs
        let state = host.ensure_shallow_state(&current.0)?;
        for wildcard in state.type_view().wildcard_reexports() {
            let wc_canonical = if !wildcard.canonical_id.is_empty() {
                wildcard.canonical_id.clone()
            } else if let Some(resolved) =
                host.resolve_import_canonical(&current.0, &wildcard.source_specifier)
            {
                resolved
            } else {
                continue;
            };

            if self.get_resolved(&wc_canonical, &current.1).is_none() {
                continue;
            }

            if let Some(target) = self.final_target_from(host, &wc_canonical, &current.1, seen) {
                return Some(target);
            }
        }

        None
    }

    /// Get all resolved symbols that were successfully resolved (Resolved or
    /// ResolvedWithUnresolvedExternal).
    pub fn successfully_resolved(
        &self,
    ) -> impl Iterator<Item = (&(String, String), &ResolvedSymbol)> {
        self.resolved.iter().filter(|(_, sym)| {
            matches!(
                sym.status,
                ResolvedSymbolStatus::Resolved
                    | ResolvedSymbolStatus::ResolvedWithUnresolvedExternal
            )
        })
    }

    /// Get all canonical file IDs that were touched during frontier traversal.
    pub fn touched_canonical_ids(&self) -> FxHashSet<String> {
        let mut ids = FxHashSet::default();
        for (canonical_id, _) in self.resolved.keys() {
            ids.insert(canonical_id.clone());
        }
        ids
    }

    /// Check if a specific `(canonical_id, exported_name)` pair was resolved
    /// successfully.
    pub fn is_resolved(&self, canonical_id: &str, exported_name: &str) -> bool {
        self.resolved
            .get(&(canonical_id.to_string(), exported_name.to_string()))
            .map(|s| {
                matches!(
                    s.status,
                    ResolvedSymbolStatus::Resolved
                        | ResolvedSymbolStatus::ResolvedWithUnresolvedExternal
                )
            })
            .unwrap_or(false)
    }
}

impl Default for ExternalTypeFrontier {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::ShallowImportResolver;
    use verter_semantic::analysis::Hash16;

    /// Mock host for testing the frontier engine.
    struct MockHost {
        files: FxHashMap<String, Arc<ShallowFileState>>,
        route_exports_only: bool,
    }

    impl MockHost {
        fn new() -> Self {
            Self {
                files: FxHashMap::default(),
                route_exports_only: false,
            }
        }

        fn add_file(&mut self, canonical_id: &str, state: ShallowFileState) {
            self.files.insert(canonical_id.to_string(), Arc::new(state));
        }
    }

    impl FrontierHost for MockHost {
        fn ensure_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
            self.files.get(canonical_id).cloned()
        }

        fn route_exports_only(&self) -> bool {
            self.route_exports_only
        }
    }

    /// Mock resolver that maps specifiers to canonical IDs during state construction.
    struct MapResolver {
        map: FxHashMap<String, String>,
    }

    impl MapResolver {
        fn from_pairs(pairs: &[(&str, &str)]) -> Self {
            let mut map = FxHashMap::default();
            for &(spec, canonical) in pairs {
                map.insert(spec.to_string(), canonical.to_string());
            }
            Self { map }
        }
    }

    impl ShallowImportResolver for MapResolver {
        fn resolve_canonical(&self, specifier: &str) -> Option<String> {
            self.map.get(specifier).cloned()
        }
    }

    fn make_analysis(
        source: &str,
    ) -> Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource> {
        let alloc = oxc_allocator::Allocator::new();
        Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source(
                source, &alloc,
            ),
        )
    }

    fn make_state(source: &str) -> ShallowFileState {
        ShallowFileState::from_analysis(Hash16::default(), make_analysis(source), None)
    }

    fn make_state_resolved(source: &str, resolutions: &[(&str, &str)]) -> ShallowFileState {
        let resolver = MapResolver::from_pairs(resolutions);
        ShallowFileState::from_analysis_with_resolver(
            Hash16::default(),
            make_analysis(source),
            None,
            None,
            &resolver,
        )
    }

    #[test]
    fn direct_export_resolves_in_one_level() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/types.ts",
            make_state("export interface Props { label: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/types.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        assert_eq!(frontier.resolved_count(), 1);
        let resolved = frontier.get_resolved("/src/types.ts", "Props").unwrap();
        assert!(
            matches!(
                resolved.status,
                ResolvedSymbolStatus::Resolved | ResolvedSymbolStatus::InvalidDeclaration
            ),
            "Props should resolve or be invalid (no eval env): {:?}",
            resolved.status
        );
    }

    #[test]
    fn reexport_follows_chain_across_levels() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/barrel.ts",
            make_state_resolved(
                r#"export { Props } from "./inner""#,
                &[("./inner", "/src/inner.ts")],
            ),
        );
        host.add_file(
            "/src/inner.ts",
            make_state("export interface Props { label: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        // Should have resolved both the barrel re-export and the inner definition
        assert!(
            frontier.resolved_count() >= 2,
            "should resolve barrel + inner"
        );
        assert!(
            frontier.get_resolved("/src/barrel.ts", "Props").is_some(),
            "barrel entry should be resolved"
        );
        assert!(
            frontier.get_resolved("/src/inner.ts", "Props").is_some(),
            "inner entry should be resolved"
        );
    }

    #[test]
    fn wildcard_barrel_resolves_through_export_star() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/barrel.ts",
            make_state_resolved("export * from './inner'", &[("./inner", "/src/inner.ts")]),
        );
        host.add_file(
            "/src/inner.ts",
            make_state("export interface Props { label: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        assert!(
            frontier.get_resolved("/src/inner.ts", "Props").is_some(),
            "Props should resolve through wildcard barrel to inner"
        );
    }

    #[test]
    fn dedup_prevents_double_resolution() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/types.ts",
            make_state("export interface Shared { id: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        // Seed the same symbol twice
        frontier.seed(vec![
            PendingExternalSymbol {
                canonical_id: "/src/types.ts".to_string(),
                exported_name: "Shared".to_string(),
            },
            PendingExternalSymbol {
                canonical_id: "/src/types.ts".to_string(),
                exported_name: "Shared".to_string(),
            },
        ]);

        frontier.run(&host).unwrap();

        // Should only visit once
        assert_eq!(
            frontier.counters.frontier_symbol_visits, 1,
            "dedup should prevent double visits"
        );
    }

    #[test]
    fn budget_exceeded_returns_structured_failure() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/types.ts",
            make_state("export interface A { a: string }\nexport interface B { b: number }"),
        );

        let mut frontier = ExternalTypeFrontier::with_budgets(ResolutionBudgets {
            frontier_symbol_visits: 1,
            ..ResolutionBudgets::default()
        });
        frontier.seed(vec![
            PendingExternalSymbol {
                canonical_id: "/src/types.ts".to_string(),
                exported_name: "A".to_string(),
            },
            PendingExternalSymbol {
                canonical_id: "/src/types.ts".to_string(),
                exported_name: "B".to_string(),
            },
        ]);

        let result = frontier.run(&host);
        assert!(result.is_err(), "should fail with budget exceeded");
        let failure = result.unwrap_err();
        assert_eq!(failure.domain, BudgetDomain::Frontier);
        assert!(frontier.budget_failure.is_some());
    }

    #[test]
    fn missing_file_produces_route_not_found() {
        let host = MockHost::new(); // empty â€” no files

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/missing.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        let resolved = frontier.get_resolved("/src/missing.ts", "Props").unwrap();
        assert_eq!(resolved.status, ResolvedSymbolStatus::RouteNotFound);
    }

    #[test]
    fn cycle_does_not_reenter_seen_symbols() {
        let mut host = MockHost::new();
        // a.ts reexports from b.ts, b.ts reexports from a.ts
        host.add_file(
            "/src/a.ts",
            make_state_resolved(r#"export { B } from "./b""#, &[("./b", "/src/b.ts")]),
        );
        host.add_file(
            "/src/b.ts",
            make_state_resolved(r#"export { A } from "./a""#, &[("./a", "/src/a.ts")]),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/a.ts".to_string(),
            exported_name: "B".to_string(),
        }]);

        // Must not hang
        frontier.run(&host).unwrap();

        // Should have visited a bounded number of symbols
        assert!(
            frontier.counters.frontier_symbol_visits <= 10,
            "cycle should not cause unbounded visits: {}",
            frontier.counters.frontier_symbol_visits
        );
    }

    #[test]
    fn wildcard_first_wins_when_multiple_sources_export_same_name() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/barrel.ts",
            make_state_resolved(
                "export * from './first'\nexport * from './second'\n",
                &[("./first", "/src/first.ts"), ("./second", "/src/second.ts")],
            ),
        );
        host.add_file(
            "/src/first.ts",
            make_state("export interface Props { source: 'first' }"),
        );
        host.add_file(
            "/src/second.ts",
            make_state("export interface Props { source: 'second' }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        // Props should resolve through first.ts (declared order wins)
        let resolved = frontier.get_resolved("/src/first.ts", "Props");
        assert!(
            resolved.is_some(),
            "Props should be found in first.ts (first-wins)"
        );

        // second.ts should NOT have been visited for Props
        // (first source already claimed it)
        let second = frontier.get_resolved("/src/second.ts", "Props");
        assert!(
            second.is_none(),
            "Props should NOT be resolved from second.ts â€” first-wins"
        );
    }

    #[test]
    fn recursive_barrel_chain_resolves() {
        let mut host = MockHost::new();
        // a -> export * from b -> export * from c -> defines Props
        host.add_file(
            "/src/a.ts",
            make_state_resolved("export * from './b'", &[("./b", "/src/b.ts")]),
        );
        host.add_file(
            "/src/b.ts",
            make_state_resolved("export * from './c'", &[("./c", "/src/c.ts")]),
        );
        host.add_file(
            "/src/c.ts",
            make_state("export interface Props { deep: boolean }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/a.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        // Props should ultimately be found in c.ts
        assert!(
            frontier.get_resolved("/src/c.ts", "Props").is_some(),
            "Props should resolve through recursive barrel chain to c.ts"
        );
    }

    #[test]
    fn final_target_for_follows_alias_and_wildcard_chain() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/root.ts",
            make_state_resolved(
                "export { Props as RootProps } from './barrel'",
                &[("./barrel", "/src/barrel.ts")],
            ),
        );
        host.add_file(
            "/src/barrel.ts",
            make_state_resolved("export * from './inner'", &[("./inner", "/src/inner.ts")]),
        );
        host.add_file(
            "/src/inner.ts",
            make_state("export interface Props { label: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/root.ts".to_string(),
            exported_name: "RootProps".to_string(),
        }]);

        frontier.run(&host).unwrap();

        let final_target = frontier
            .final_target_for(&host, "/src/root.ts", "RootProps")
            .expect("frontier should report the final defining target");

        assert_eq!(
            final_target,
            ("/src/inner.ts".to_string(), "Props".to_string()),
            "frontier final target should follow alias and wildcard hops to the defining symbol"
        );
    }

    #[test]
    fn final_target_for_stops_at_direct_symbol_with_companions() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/types.ts",
            make_state_resolved(
                "import type { Dep } from './dep'\nexport interface Props { label: Dep }",
                &[("./dep", "/src/dep.ts")],
            ),
        );
        host.add_file(
            "/src/dep.ts",
            make_state("export interface Dep { value: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/types.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        let final_target = frontier
            .final_target_for(&host, "/src/types.ts", "Props")
            .expect("direct symbols with companion deps should still report their own target");

        assert_eq!(
            final_target,
            ("/src/types.ts".to_string(), "Props".to_string()),
            "frontier must not treat companion imports as route hops for direct symbols"
        );
    }

    #[test]
    fn final_target_for_follows_nested_wildcard_chain() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/a.ts",
            make_state_resolved("export * from './b'", &[("./b", "/src/b.ts")]),
        );
        host.add_file(
            "/src/b.ts",
            make_state_resolved("export * from './c'", &[("./c", "/src/c.ts")]),
        );
        host.add_file(
            "/src/c.ts",
            make_state("export interface Props { deep: boolean }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/a.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        let final_target = frontier
            .final_target_for(&host, "/src/a.ts", "Props")
            .expect("nested wildcard chain should report the final defining target");

        assert_eq!(
            final_target,
            ("/src/c.ts".to_string(), "Props".to_string()),
            "frontier should keep wildcard route breadcrumbs across nested barrels"
        );
    }

    #[test]
    fn final_target_for_follows_exported_import_local_alias() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/index.ts",
            make_state_resolved(
                "import { Foo as Bar } from './types'; export { Bar };",
                &[("./types", "/src/types.ts")],
            ),
        );
        host.add_file(
            "/src/types.ts",
            make_state("export interface Foo { value: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/index.ts".to_string(),
            exported_name: "Bar".to_string(),
        }]);

        frontier.run(&host).unwrap();

        let final_target = frontier
            .final_target_for(&host, "/src/index.ts", "Bar")
            .expect("exported import-local aliases should route to the defining import target");

        assert_eq!(
            final_target,
            ("/src/types.ts".to_string(), "Foo".to_string()),
            "frontier should route exported import-local aliases through their import target"
        );
    }

    #[test]
    fn final_target_for_follows_default_import_alias_export() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/index.ts",
            make_state_resolved(
                "import PropsDefault from './dep'; export { PropsDefault as Props };",
                &[("./dep", "/src/dep.ts")],
            ),
        );
        host.add_file(
            "/src/dep.ts",
            make_state("export default class Props { label!: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/index.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        let final_target = frontier
            .final_target_for(&host, "/src/index.ts", "Props")
            .expect("default-import alias exports should route to the default defining symbol");

        assert_eq!(
            final_target,
            ("/src/dep.ts".to_string(), "default".to_string()),
            "frontier should preserve default-export identity across local export aliases"
        );
    }

    #[test]
    fn route_only_frontier_stops_at_defining_export_without_following_symbol_deps() {
        let mut host = MockHost::new();
        host.route_exports_only = true;
        host.add_file(
            "/src/index.ts",
            make_state_resolved(
                "export { Props } from './types'",
                &[("./types", "/src/types.ts")],
            ),
        );
        host.add_file(
            "/src/types.ts",
            make_state_resolved(
                "import type { Base } from './base'\nexport interface Props extends Base { label: string }",
                &[("./base", "/src/base.ts")],
            ),
        );
        host.add_file(
            "/src/base.ts",
            make_state("export interface Base { id: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/index.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        assert_eq!(
            frontier.final_target_for(&host, "/src/index.ts", "Props"),
            Some(("/src/types.ts".to_string(), "Props".to_string())),
        );
        assert!(
            frontier.get_resolved("/src/base.ts", "Base").is_none(),
            "route-only traversal should not widen into the defining symbol's dependency graph",
        );
    }

    #[test]
    fn frontier_resolves_through_canonical_edges_without_host_callback() {
        // The MockHost does NOT implement resolve_import_canonical (returns None).
        // But the shallow states have canonical IDs pre-populated on their edges
        // via the MapResolver at construction time. The frontier should resolve
        // the entire chain without needing the fallback host callback.

        let mut host = MockHost::new();

        // barrel.ts: reexports Props from types.ts via a named reexport
        // The canonical ID on the reexport edge is pre-resolved.
        host.add_file(
            "/src/barrel.ts",
            make_state_resolved(
                r#"export { Props } from './types'"#,
                &[("./types", "/src/types.ts")],
            ),
        );

        // types.ts: defines Props locally
        let types_source = "export interface Props { label: string }";
        let types_analysis = make_analysis(types_source);
        let types_env =
            verter_semantic::analysis::type_eval_build::parse_and_build_env(types_source);
        let types_state =
            ShallowFileState::from_analysis(Hash16::default(), types_analysis, Some(&types_env));
        host.add_file("/src/types.ts", types_state);

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        // The barrel entry should be resolved (as a reexport hop)
        let barrel_resolved = frontier
            .get_resolved("/src/barrel.ts", "Props")
            .expect("barrel entry should be resolved");
        assert_ne!(
            barrel_resolved.status,
            ResolvedSymbolStatus::RouteNotFound,
            "barrel reexport should not be RouteNotFound when canonical edges are pre-populated"
        );

        // The types.ts entry should be resolved with the actual body
        let types_resolved = frontier
            .get_resolved("/src/types.ts", "Props")
            .expect("types.ts entry should be resolved");
        assert_eq!(
            types_resolved.status,
            ResolvedSymbolStatus::Resolved,
            "types.ts Props should be fully resolved (local definition with eval env)"
        );
        assert!(
            types_resolved.body.is_some(),
            "resolved local symbol should have a body"
        );

        // Negative: the host's resolve_import_canonical was never called.
        // Since MockHost does not override the default (returns None), if the
        // frontier had needed it, the reexport would have been RouteNotFound.
        // The fact that it resolved proves the canonical edges sufficed.
    }

    #[test]
    fn reexport_with_unresolvable_import_returns_route_not_found() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/barrel.ts",
            make_state(r#"export { Props } from "./missing""#),
        );
        // No resolution for ./missing â€” host returns None

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        let resolved = frontier.get_resolved("/src/barrel.ts", "Props").unwrap();
        assert_eq!(
            resolved.status,
            ResolvedSymbolStatus::RouteNotFound,
            "reexport with unresolvable import should be RouteNotFound"
        );
    }

    #[test]
    fn direct_export_takes_precedence_over_wildcard_in_same_file() {
        let mut host = MockHost::new();
        // barrel has both a direct reexport AND a wildcard that could provide Props
        host.add_file(
            "/src/barrel.ts",
            make_state_resolved(
                "export { Props } from './direct'\nexport * from './wildcard'\n",
                &[
                    ("./direct", "/src/direct.ts"),
                    ("./wildcard", "/src/wildcard.ts"),
                ],
            ),
        );
        host.add_file(
            "/src/direct.ts",
            make_state("export interface Props { from: 'direct' }"),
        );
        host.add_file(
            "/src/wildcard.ts",
            make_state("export interface Props { from: 'wildcard' }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        // barrel should resolve as a reexport (alias route), not wildcard
        let barrel_resolved = frontier.get_resolved("/src/barrel.ts", "Props").unwrap();
        assert!(
            barrel_resolved.route_provenance.is_some(),
            "should have route provenance"
        );
        match &barrel_resolved.route_provenance.as_ref().unwrap().kind {
            RouteKind::Alias => {} // correct
            other => panic!("expected Alias route (direct reexport), got {other:?}"),
        }

        // direct.ts should be resolved, not wildcard.ts
        assert!(
            frontier.get_resolved("/src/direct.ts", "Props").is_some(),
            "direct.ts should be resolved"
        );
    }

    #[test]
    fn local_export_without_eval_env_produces_invalid_declaration() {
        let mut host = MockHost::new();
        // Without eval_env, symbols map is empty â€” local closure fails
        host.add_file(
            "/src/types.ts",
            make_state("export interface Props { label: string }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/types.ts".to_string(),
            exported_name: "Props".to_string(),
        }]);

        frontier.run(&host).unwrap();

        let resolved = frontier.get_resolved("/src/types.ts", "Props").unwrap();
        // Without eval_env, symbols are empty so local closure returns
        // MissingLocalSymbol â†’ InvalidDeclaration
        assert_eq!(
            resolved.status,
            ResolvedSymbolStatus::InvalidDeclaration,
            "local export without eval env should produce InvalidDeclaration"
        );
    }
}
