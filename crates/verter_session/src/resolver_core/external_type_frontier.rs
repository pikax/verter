//! Unified external type frontier engine.
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
//! `ResolvedSymbol` entries with post-local-closure symbolic bodies that later
//! materialization stages consume from cache.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::shallow_file_state::{
    BudgetDomain, BudgetExceededFailure, ExportTarget, ExternalSymbolRef, LocalClosureStatus,
    ResolutionBudgets, ResolutionCounters, ShallowFileState,
};
use verter_type_expr::facts::{NarrowFrontierBody, NarrowTypeParam};
use verter_type_expr::locators::{
    AuthoredAnchor, LocatorSymbolSpace, SymbolBodyLocator, TypeBodyPathStep, TypeBodySlot,
    TypeParamBoundPosition,
};
use verter_type_expr::TypeParam;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A pending symbol to resolve in the next frontier level.
#[derive(Debug, Clone)]
pub struct PendingExternalSymbol {
    pub canonical_id: String,
    pub exported_name: String,
    /// Route demand carried through alias and wildcard hops.
    /// `None` means `Whole`.
    pub route: Option<super::route_demand::RouteDemand>,
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
    /// The graph-free narrowed frontier body of a locally-defined symbol: the
    /// [`NarrowFrontierBody::Resolvable`] locator escape (the keyable inverse of
    /// a session handle) addressing the resolvable body, lowered on demand by a
    /// consumer. The frontier lives below the session graph and never reads the
    /// body as a `TypeExpr`; the former eager `lookup_object()` population is
    /// this locator escape. Minted from the defining file's local type
    /// declaration BEFORE local closure runs and attached regardless of the
    /// terminal [`ResolvedSymbolStatus`] — a `BudgetExceeded` /
    /// `InvalidDeclaration` symbol whose file still declares the type carries
    /// the locator, mirroring the former read (which also attached the local
    /// decl's body irrespective of closure status). `None` exactly when the
    /// symbol has no local type declaration (reexport / alias forward hops,
    /// route-not-found, missing files).
    pub frontier_body: Option<NarrowFrontierBody>,
    /// Narrowed generic parameters on the defining local symbol — each
    /// constraint / default bound is addressed by a body-slot locator, never an
    /// embedded `TypeExpr`.
    pub type_parameters: Vec<NarrowTypeParam>,
    /// External refs that need resolution in subsequent levels.
    pub unresolved_external: Vec<ExternalSymbolRef>,
    /// Route provenance for invalidation and observability.
    pub route_provenance: Option<ResolvedRouteProvenance>,
}

/// Mint the graph-free narrowed frontier body for a locally-defined symbol: a
/// [`NarrowFrontierBody::Resolvable`] wrapping a [`SymbolBodyLocator`] that
/// addresses the resolvable body (replacing the eager `lookup_object()`
/// `TypeExpr` population — the frontier lives below the session graph and never
/// consumes the body as a `TypeExpr`; a consumer lowers the locator on demand)
/// plus narrowed type-parameter facts. The caller mints this from the local
/// declaration (`state.type_decl(name)`) BEFORE running local closure and
/// attaches the result whatever the closure's terminal status turns out to be
/// (so a `BudgetExceeded` / `InvalidDeclaration` outcome still carries the
/// locator + type-param facts when the local declaration exists) — exactly as
/// the former read attached the local decl's body regardless of closure
/// status. Returns `(None, [])` when the symbol has no local declaration,
/// mirroring the former read.
fn resolve_local_frontier_body(
    state: &ShallowFileState,
    canonical_id: &str,
    symbol_name: &str,
) -> (Option<NarrowFrontierBody>, Vec<NarrowTypeParam>) {
    state
        .type_decl(symbol_name)
        .map(|lowered| {
            let anchor = AuthoredAnchor {
                canonical_id: Arc::from(canonical_id),
                symbol: Arc::from(symbol_name),
                space: LocatorSymbolSpace::Type,
            };
            let type_parameters = narrow_frontier_type_params(&lowered.type_parameters, &anchor);
            (
                Some(NarrowFrontierBody::Resolvable(SymbolBodyLocator {
                    anchor: anchor.clone(),
                })),
                type_parameters,
            )
        })
        .unwrap_or_else(|| (None, Vec::new()))
}

/// Narrow the defining symbol's authored type parameters to graph-free
/// [`NarrowTypeParam`] facts: the name + declaration ordinal are carried
/// directly, and each present constraint / default bound becomes a body-slot
/// locator addressing its authored position (never an embedded `TypeExpr`).
fn narrow_frontier_type_params(
    type_params: &[TypeParam],
    anchor: &AuthoredAnchor,
) -> Vec<NarrowTypeParam> {
    type_params
        .iter()
        .enumerate()
        .map(|(index, tp)| {
            let ordinal = index as u32;
            let bound_slot = |position: TypeParamBoundPosition| TypeBodySlot {
                anchor: anchor.clone(),
                path: Arc::from(vec![TypeBodyPathStep::TypeParamBound { ordinal, position }]),
            };
            NarrowTypeParam {
                name: tp.name.clone(),
                ordinal,
                constraint: tp
                    .constraint
                    .as_ref()
                    .map(|_| bound_slot(TypeParamBoundPosition::Constraint)),
                default: tp
                    .default
                    .as_ref()
                    .map(|_| bound_slot(TypeParamBoundPosition::Default)),
            }
        })
        .collect()
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
/// comes through this trait. Cross-file edges usually carry canonical
/// target IDs on the `ShallowFileState`; when they do not, the host may
/// lazily supply a type-route target for the missing edge.
pub trait FrontierHost {
    /// Get or build the shallow type state for a canonical file.
    fn ensure_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>>;

    /// Resolve a missing type edge from an owner file to an import/reexport source.
    fn resolve_type_edge_canonical(
        &self,
        _owner_canonical: &str,
        _source_specifier: &str,
    ) -> Option<String> {
        None
    }

    /// Route-only traversals stop once they reach the defining export target.
    /// They still follow alias and reexport edges, but they do not widen into
    /// the defining symbol's dependency graph.
    fn route_exports_only(&self) -> bool {
        false
    }
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
        while self.run_one_level(host)? {}
        Ok(())
    }

    /// Process one BFS level and report whether another level remains.
    pub fn run_one_level<H: FrontierHost>(
        &mut self,
        host: &H,
    ) -> Result<bool, BudgetExceededFailure> {
        if self.current_level.is_empty() {
            return Ok(false);
        }

        self.process_level(host)?;
        self.current_level.clear();
        std::mem::swap(&mut self.current_level, &mut self.next_level);
        Ok(!self.current_level.is_empty())
    }

    /// Drop any queued-but-unprocessed frontier work.
    pub fn clear_pending(&mut self) {
        self.current_level.clear();
        self.next_level.clear();
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
            // Prefer the pre-canonicalized edge, but ask the host for a
            // type-route target when the shallow state left the edge empty.
            for ext_ref in &resolved.unresolved_external {
                // Per-request audit attribution: one barrel-export hop
                // traversed. Each external ref the BFS expands counts
                // as one barrel step, including refs whose target is
                // later deduplicated by the `seen` set — the counter
                // measures raw walk work, not unique-target count.
                if let Some(obs) = verter_audit::current_observer() {
                    obs.record_event(verter_audit::AuditEvent::RouteDbBarrelStep);
                }
                let target_canonical = match ext_ref.canonical_id.as_deref() {
                    Some(canonical) => Some(canonical.to_string()),
                    None => host.resolve_type_edge_canonical(
                        &resolved.canonical_id,
                        &ext_ref.source_specifier,
                    ),
                };
                let Some(target_canonical) = target_canonical else {
                    continue;
                };
                let next = PendingExternalSymbol {
                    canonical_id: target_canonical.clone(),
                    exported_name: ext_ref.imported_name.clone(),
                    route: Some(ext_ref.route.clone()),
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
                frontier_body: None,
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

        // Step 2: Enqueue wildcard reexport targets for the next BFS layer.
        for (order, wildcard) in type_view.wildcard_reexports().iter().enumerate() {
            let target_canonical = if wildcard.canonical_id.is_empty() {
                host.resolve_type_edge_canonical(&pending.canonical_id, &wildcard.source_specifier)
            } else {
                Some(wildcard.canonical_id.clone())
            };
            let Some(target_canonical) = target_canonical else {
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
                        frontier_body: existing.frontier_body.clone(),
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

            let next = PendingExternalSymbol {
                canonical_id: target_canonical.clone(),
                exported_name: pending.exported_name.clone(),
                route: pending.route.clone(),
            };
            let key = (next.canonical_id.clone(), next.exported_name.clone());
            if self.seen.insert(key) {
                self.next_level.push(next);
            }
        }

        // Not found through any route
        ResolvedSymbol {
            canonical_id: pending.canonical_id.clone(),
            exported_name: pending.exported_name.clone(),
            status: ResolvedSymbolStatus::RouteNotFound,
            frontier_body: None,
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
                        let resolved_canonical = (!import_target.canonical_id.is_empty())
                            .then(|| import_target.canonical_id.clone())
                            .or_else(|| {
                                host.resolve_type_edge_canonical(
                                    &pending.canonical_id,
                                    &import_target.source_specifier,
                                )
                            });
                        if let Some(ref target_canonical) = resolved_canonical {
                            let next = PendingExternalSymbol {
                                canonical_id: target_canonical.clone(),
                                exported_name: import_target.imported_name.clone(),
                                route: pending.route.clone(),
                            };
                            let key = (next.canonical_id.clone(), next.exported_name.clone());
                            if self.seen.insert(key) {
                                self.next_level.push(next);
                            }

                            return ResolvedSymbol {
                                canonical_id: pending.canonical_id.clone(),
                                exported_name: pending.exported_name.clone(),
                                status: ResolvedSymbolStatus::ResolvedWithUnresolvedExternal,
                                frontier_body: None,
                                type_parameters: Vec::new(),
                                unresolved_external: vec![ExternalSymbolRef {
                                    local_name: symbol_name.clone(),
                                    source_specifier: import_target.source_specifier.clone(),
                                    imported_name: import_target.imported_name.clone(),
                                    canonical_id: Some(Arc::<str>::from(target_canonical.as_str())),
                                    route: pending.route.clone().unwrap_or_default(),
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
                    let (frontier_body, type_parameters) =
                        resolve_local_frontier_body(state, &pending.canonical_id, symbol_name);

                    return ResolvedSymbol {
                        canonical_id: pending.canonical_id.clone(),
                        exported_name: pending.exported_name.clone(),
                        status: ResolvedSymbolStatus::Resolved,
                        frontier_body,
                        type_parameters,
                        unresolved_external: Vec::new(),
                        route_provenance: Some(ResolvedRouteProvenance {
                            kind: RouteKind::Direct,
                            defining_canonical_id: pending.canonical_id.clone(),
                            defining_name: symbol_name.clone(),
                        }),
                    };
                }

                let (frontier_body, type_parameters) =
                    resolve_local_frontier_body(state, &pending.canonical_id, symbol_name);

                // Run route-aware closure when a route demand is present,
                // otherwise fall back to full local closure.
                let closure = if let Some(ref route) = pending.route {
                    state.route_closure(symbol_name, route, self.budgets.local_closure_steps)
                } else {
                    state
                        .type_view()
                        .local_closure(symbol_name, self.budgets.local_closure_steps)
                };
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
                    frontier_body,
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
                let effective_canonical = (!reexport_canonical.is_empty())
                    .then(|| reexport_canonical.clone())
                    .or_else(|| {
                        host.resolve_type_edge_canonical(&pending.canonical_id, source_specifier)
                    });
                if let Some(ref reexport_canonical) = effective_canonical {
                    let next = PendingExternalSymbol {
                        canonical_id: reexport_canonical.clone(),
                        exported_name: original_name.clone(),
                        route: pending.route.clone(),
                    };
                    let key = (next.canonical_id.clone(), next.exported_name.clone());
                    if self.seen.insert(key) {
                        self.next_level.push(next);
                    }

                    ResolvedSymbol {
                        canonical_id: pending.canonical_id.clone(),
                        exported_name: pending.exported_name.clone(),
                        status: ResolvedSymbolStatus::ResolvedWithUnresolvedExternal,
                        frontier_body: None,
                        type_parameters: Vec::new(),
                        unresolved_external: vec![ExternalSymbolRef {
                            local_name: pending.exported_name.clone(),
                            source_specifier: source_specifier.clone(),
                            imported_name: original_name.clone(),
                            canonical_id: Some(Arc::<str>::from(reexport_canonical.as_str())),
                            route: pending.route.clone().unwrap_or_default(),
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
                        frontier_body: None,
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

    /// Number of pending symbols queued for the current BFS layer.
    pub fn pending_count(&self) -> usize {
        self.current_level.len()
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
        let mut had_cycle = false;

        self.final_target_from(host, canonical_id, exported_name, &mut seen, &mut had_cycle)
    }

    /// Follow the resolved route chain and report whether a route cycle was
    /// encountered while proving the final target.
    pub fn final_target_for_with_cycle<H: FrontierHost>(
        &self,
        host: &H,
        canonical_id: &str,
        exported_name: &str,
    ) -> (Option<(String, String)>, bool) {
        let mut seen = FxHashSet::default();
        let mut had_cycle = false;
        let target =
            self.final_target_from(host, canonical_id, exported_name, &mut seen, &mut had_cycle);
        (target, had_cycle)
    }

    fn final_target_from<H: FrontierHost>(
        &self,
        host: &H,
        canonical_id: &str,
        exported_name: &str,
        seen: &mut FxHashSet<(String, String)>,
        had_cycle: &mut bool,
    ) -> Option<(String, String)> {
        let current = (canonical_id.to_string(), exported_name.to_string());

        if !seen.insert(current.clone()) {
            *had_cycle = true;
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
                    had_cycle,
                ),
            };
        }

        // Fall back to wildcard reexport edges using the shallow edge first,
        // then lazily proving the missing type route through the host.
        let state = host.ensure_shallow_state(&current.0)?;
        for wildcard in state.type_view().wildcard_reexports() {
            let wc_canonical = if wildcard.canonical_id.is_empty() {
                host.resolve_type_edge_canonical(&current.0, &wildcard.source_specifier)
            } else {
                Some(wildcard.canonical_id.clone())
            };
            let Some(wc_canonical) = wc_canonical else {
                continue;
            };

            if self.get_resolved(&wc_canonical, &current.1).is_none() {
                continue;
            }

            if let Some(target) =
                self.final_target_from(host, &wc_canonical, &current.1, seen, had_cycle)
            {
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
    use std::cell::RefCell;

    use super::*;
    use crate::resolver_core::ShallowImportResolver;
    use verter_semantic::analysis::Hash16;

    /// Mock host for testing the frontier engine.
    struct MockHost {
        files: FxHashMap<String, Arc<ShallowFileState>>,
        route_exports_only: bool,
        missing_type_edges: FxHashMap<(String, String), String>,
        ensured: RefCell<Vec<String>>,
    }

    impl MockHost {
        fn new() -> Self {
            Self {
                files: FxHashMap::default(),
                route_exports_only: false,
                missing_type_edges: FxHashMap::default(),
                ensured: RefCell::new(Vec::new()),
            }
        }

        fn add_file(&mut self, canonical_id: &str, state: impl Into<Arc<ShallowFileState>>) {
            self.files.insert(canonical_id.to_string(), state.into());
        }

        fn add_missing_type_edge(
            &mut self,
            owner_canonical: &str,
            source_specifier: &str,
            target_canonical: &str,
        ) {
            self.missing_type_edges.insert(
                (owner_canonical.to_string(), source_specifier.to_string()),
                target_canonical.to_string(),
            );
        }

        fn ensure_log(&self) -> Vec<String> {
            self.ensured.borrow().clone()
        }

        fn reset_ensure_log(&self) {
            self.ensured.borrow_mut().clear();
        }
    }

    impl FrontierHost for MockHost {
        fn ensure_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
            self.ensured.borrow_mut().push(canonical_id.to_string());
            self.files.get(canonical_id).cloned()
        }

        fn resolve_type_edge_canonical(
            &self,
            owner_canonical: &str,
            source_specifier: &str,
        ) -> Option<String> {
            self.missing_type_edges
                .get(&(owner_canonical.to_string(), source_specifier.to_string()))
                .cloned()
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
    ) -> Arc<verter_parser::utils::oxc::script::type_inventory::AnalyzedExternalTypeSource> {
        let alloc = oxc_allocator::Allocator::new();
        Arc::new(
            verter_parser::utils::oxc::script::type_inventory::analyze_external_type_source(
                source, &alloc,
            ),
        )
    }

    fn make_state(source: &str) -> ShallowFileState {
        ShallowFileState::header_routing_only_for_test(Hash16::default(), make_analysis(source))
    }

    fn make_state_resolved(source: &str, resolutions: &[(&str, &str)]) -> ShallowFileState {
        let resolver = MapResolver::from_pairs(resolutions);
        ShallowFileState::header_routing_only_with_resolver_for_test(
            Hash16::default(),
            make_analysis(source),
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
            route: None,
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
            route: None,
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
            route: None,
        }]);

        frontier.run(&host).unwrap();

        assert!(
            frontier.get_resolved("/src/inner.ts", "Props").is_some(),
            "Props should resolve through wildcard barrel to inner"
        );
    }

    #[test]
    fn run_one_level_defers_wildcard_child_shallowing_until_next_level() {
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
            make_state("export interface Other { source: 'second' }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
            route: None,
        }]);

        assert!(
            frontier.run_one_level(&host).unwrap(),
            "the first level should enqueue the barrel children"
        );
        assert_eq!(
            host.ensure_log(),
            vec!["/src/barrel.ts".to_string()],
            "processing the barrel level should not shallow wildcard children inline"
        );

        host.reset_ensure_log();
        assert!(
            !frontier.run_one_level(&host).unwrap(),
            "the second level should resolve the queued children"
        );
        assert_eq!(
            host.ensure_log(),
            vec!["/src/first.ts".to_string(), "/src/second.ts".to_string()],
            "the next BFS level should shallow every queued same-layer child"
        );
        assert_eq!(
            frontier.final_target_for(&host, "/src/barrel.ts", "Props"),
            Some(("/src/first.ts".to_string(), "Props".to_string())),
        );
    }

    #[test]
    fn run_one_level_keeps_same_layer_siblings_ahead_of_grandchildren() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/barrel.ts",
            make_state_resolved(
                "export * from './a'\nexport * from './b'\n",
                &[("./a", "/src/a.ts"), ("./b", "/src/b.ts")],
            ),
        );
        host.add_file(
            "/src/a.ts",
            make_state_resolved(
                "export * from './a_deep'\n",
                &[("./a_deep", "/src/a_deep.ts")],
            ),
        );
        host.add_file(
            "/src/b.ts",
            make_state("export interface Props { source: 'b' }"),
        );
        host.add_file(
            "/src/a_deep.ts",
            make_state("export interface Props { source: 'a_deep' }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
            route: None,
        }]);

        assert!(
            frontier.run_one_level(&host).unwrap(),
            "the barrel level should enqueue the same-layer children"
        );
        host.reset_ensure_log();

        assert!(
            frontier.run_one_level(&host).unwrap(),
            "processing the child layer should leave the deeper grandchild queued"
        );
        assert_eq!(
            host.ensure_log(),
            vec!["/src/a.ts".to_string(), "/src/b.ts".to_string()],
            "same-layer children should be processed before any deeper wildcard grandchild"
        );
        assert_eq!(
            frontier.final_target_for(&host, "/src/barrel.ts", "Props"),
            Some(("/src/b.ts".to_string(), "Props".to_string())),
            "a same-layer child match must beat a deeper earlier branch"
        );

        host.reset_ensure_log();
        assert!(
            !frontier.run_one_level(&host).unwrap(),
            "the queued grandchild should remain for the following level"
        );
        assert_eq!(
            host.ensure_log(),
            vec!["/src/a_deep.ts".to_string()],
            "the deeper grandchild should not run until the next BFS layer"
        );
    }

    #[test]
    fn pending_count_tracks_current_bfs_layer() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/barrel.ts",
            make_state_resolved(
                "export * from './a'\nexport * from './b'\n",
                &[("./a", "/src/a.ts"), ("./b", "/src/b.ts")],
            ),
        );
        host.add_file(
            "/src/a.ts",
            make_state_resolved(
                "export * from './a_deep'\n",
                &[("./a_deep", "/src/a_deep.ts")],
            ),
        );
        host.add_file(
            "/src/b.ts",
            make_state("export interface Props { source: 'b' }"),
        );
        host.add_file(
            "/src/a_deep.ts",
            make_state("export interface Props { source: 'a_deep' }"),
        );

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
            route: None,
        }]);

        assert_eq!(
            frontier.pending_count(),
            1,
            "the seed should occupy the first BFS layer by itself"
        );

        assert!(
            frontier.run_one_level(&host).unwrap(),
            "the root barrel should enqueue the same-layer children"
        );
        assert_eq!(
            frontier.pending_count(),
            2,
            "the next BFS layer should contain both same-layer barrel children"
        );

        assert!(
            frontier.run_one_level(&host).unwrap(),
            "the child layer should leave the grandchild queued"
        );
        assert_eq!(
            frontier.pending_count(),
            1,
            "only the deferred grandchild should remain for the following BFS layer"
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
                route: None,
            },
            PendingExternalSymbol {
                canonical_id: "/src/types.ts".to_string(),
                exported_name: "Shared".to_string(),
                route: None,
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
                route: None,
            },
            PendingExternalSymbol {
                canonical_id: "/src/types.ts".to_string(),
                exported_name: "B".to_string(),
                route: None,
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
            route: None,
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
            route: None,
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
            route: None,
        }]);

        frontier.run(&host).unwrap();

        // Props should resolve through first.ts (declared order wins)
        let resolved = frontier.get_resolved("/src/first.ts", "Props");
        assert!(
            resolved.is_some(),
            "Props should be found in first.ts (first-wins)"
        );

        assert_eq!(
            frontier.final_target_for(&host, "/src/barrel.ts", "Props"),
            Some(("/src/first.ts".to_string(), "Props".to_string())),
            "declared-order routing should still choose the first matching wildcard child"
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
            route: None,
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
            route: None,
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
            route: None,
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
            route: None,
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
            route: None,
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
            route: None,
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
            route: None,
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
        // The shallow states have canonical IDs pre-populated on their edges
        // via the MapResolver at construction time. The frontier should resolve
        // the entire chain without needing any second import-resolution step.

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
        let types_state =
            ShallowFileState::service_backed_for_test("export interface Props { label: string }");
        host.add_file("/src/types.ts", types_state);

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/barrel.ts".to_string(),
            exported_name: "Props".to_string(),
            route: None,
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
            matches!(
                types_resolved.frontier_body,
                Some(NarrowFrontierBody::Resolvable(_))
            ),
            "resolved local symbol should carry a graph-free Resolvable frontier body locator"
        );

        // Negative: canonical edges alone must suffice for the traversal.
    }

    #[test]
    fn resolved_local_generic_symbol_carries_exact_locator_and_type_param_facts() {
        // A locally-defined GENERIC symbol must narrow to the EXACT graph-free
        // locator facts — not merely "some Resolvable arm": the
        // `SymbolBodyLocator`'s `AuthoredAnchor` identity (canonical / symbol /
        // space) and, per authored type parameter, the `NarrowTypeParam` name +
        // declaration ordinal plus body-slot locators for exactly the authored
        // constraint / default bounds (absent bounds stay `None`).
        let mut host = MockHost::new();
        let source =
            "export interface Props<T extends string = number, U = boolean> { label: T; extra: U }";
        let state = ShallowFileState::service_backed_for_test(source);
        host.add_file("/src/types.ts", state);

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/types.ts".to_string(),
            exported_name: "Props".to_string(),
            route: None,
        }]);

        frontier.run(&host).unwrap();

        let resolved = frontier
            .get_resolved("/src/types.ts", "Props")
            .expect("locally-defined generic symbol should be resolved");
        assert_eq!(
            resolved.status,
            ResolvedSymbolStatus::Resolved,
            "local generic definition with an eval env should fully resolve"
        );

        let expected_anchor = AuthoredAnchor {
            canonical_id: Arc::from("/src/types.ts"),
            symbol: Arc::from("Props"),
            space: LocatorSymbolSpace::Type,
        };
        assert_eq!(
            resolved.frontier_body,
            Some(NarrowFrontierBody::Resolvable(SymbolBodyLocator {
                anchor: expected_anchor.clone(),
            })),
            "the frontier body must be the Resolvable locator anchored at the defining \
             (canonical, symbol, Type-space) declaration"
        );

        let bound_slot = |ordinal: u32, position: TypeParamBoundPosition| TypeBodySlot {
            anchor: expected_anchor.clone(),
            path: Arc::from(vec![TypeBodyPathStep::TypeParamBound { ordinal, position }]),
        };
        assert_eq!(
            resolved.type_parameters,
            vec![
                NarrowTypeParam {
                    name: "T".to_string(),
                    ordinal: 0,
                    constraint: Some(bound_slot(0, TypeParamBoundPosition::Constraint)),
                    default: Some(bound_slot(0, TypeParamBoundPosition::Default)),
                },
                NarrowTypeParam {
                    name: "U".to_string(),
                    ordinal: 1,
                    constraint: None,
                    default: Some(bound_slot(1, TypeParamBoundPosition::Default)),
                },
            ],
            "narrowed type params must carry the authored name + declaration ordinal, a \
             constraint/default body-slot locator exactly where a bound is authored, and \
             None exactly where it is not"
        );
    }

    #[test]
    fn frontier_can_follow_missing_type_edges_via_host_callback() {
        let mut host = MockHost::new();
        host.add_file(
            "/src/index.ts",
            make_state("export { Props } from './types'"),
        );
        host.add_file(
            "/src/types.ts",
            make_state("import type { Base } from './base'\nexport interface Props extends Base { label: string }"),
        );
        host.add_file(
            "/src/base.ts",
            make_state("export interface Base { id: string }"),
        );
        host.add_missing_type_edge("/src/index.ts", "./types", "/src/types.ts");
        host.add_missing_type_edge("/src/types.ts", "./base", "/src/base.ts");

        let mut frontier = ExternalTypeFrontier::new();
        frontier.seed(vec![PendingExternalSymbol {
            canonical_id: "/src/index.ts".to_string(),
            exported_name: "Props".to_string(),
            route: None,
        }]);

        frontier.run(&host).unwrap();

        assert_eq!(
            frontier.final_target_for(&host, "/src/index.ts", "Props"),
            Some(("/src/types.ts".to_string(), "Props".to_string())),
        );
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
            route: None,
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
            route: None,
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
            route: None,
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
