//! Same-file TRANSITIVE route closure over stored per-decl route facts — the
//! downstream half of the route-fact producer/closure split.
//!
//! [`route_closure_over_facts`] / [`local_closure_over_facts`] reproduce the
//! legacy shallow-state closures (whole-route / member / member-path / Omit /
//! plain local) by reading each declaration's stored
//! [`ShallowRouteFacts`] + dependency-edge classification through a
//! [`RouteClosureProvider`] — never by re-walking declaration bodies. This is
//! NOT a second resolution engine: no dispatch, no query keys, no cross-file
//! deref — only sibling fact reads plus the same visited/budget bookkeeping
//! the legacy walk carried. The session's `ShallowFileState` implements the
//! provider and converts the resulting [`ExternalRouteRefFact`]s to its
//! `ExternalSymbolRef` 1:1 at the boundary; the byte-parity target is
//! `unresolved_external` (5 fields + route, deduped/merged/ordered) plus
//! `status`.
//!
//! The ONE downstream evaluation that reaches beyond stored facts is the
//! deferred key-source (a `Pick<Imported, LocalKeys>` recipe): the key alias
//! enumerates through [`RouteClosureProvider::key_source_lookup`] — a
//! CONTENT-FREE hand-off (the provider owns the alias-graph follow over
//! per-decl `KeySourceFact`s; this closure never receives a declaration
//! body). The outcome is TRI-STATE ([`KeySourceLookup`]): an UNAVAILABLE
//! hand-off fails CLOSED — the deferred edge contributes nothing, and in
//! particular the empty-keys fallback does NOT fire; a hand-off that RESOLVES
//! to zero literal keys applies the legacy `utility → None` fall-through (the
//! userland local-decl follow); a non-empty resolution applies the route.

use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use verter_type_expr::facts::{
    DeferredKeyUtilityEdge, DeferredKeyUtilityKind, ExternalRouteRefFact, KeyDomainFact,
    MemberNamesRoute, MemberPathSeedTarget, RouteDependencyRefFact, ShallowRouteFacts,
    WholeRouteContextFact, WholeRouteEdgeFact,
};
use verter_type_expr::{merge_route_demands, RouteDemand};

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

/// The per-symbol dependency-edge classification (the fact view of the
/// session's `ClassifiedTypeDeps`): same-file local dep names plus external
/// route refs, both in the session's deterministic sort order.
#[derive(Debug, Clone, Default)]
pub struct ClassifiedRouteDeps {
    /// Same-file local dependency names (sorted).
    pub local_deps: Vec<String>,
    /// External dependency refs (sorted by the session's key).
    pub external_deps: Vec<ExternalRouteRefFact>,
}

/// The fact source a route closure runs against. Every method is a stored-fact
/// or header read — a provider must never hand a declaration body to this
/// closure (the [`key_source_lookup`](Self::key_source_lookup) hand-off is
/// content-free: the provider enumerates the deferred key-source alias graph
/// itself and returns only literal keys, failing closed when unavailable).
pub trait RouteClosureProvider {
    /// Header-level TYPE-symbol membership.
    fn has_type_symbol(&self, name: &str) -> bool;

    /// The stored per-decl route facts (`None` ⟺ the decl-body demand
    /// missed — header miss or lease miss — matching the legacy
    /// `type_decl(name)` miss).
    fn route_facts(&self, name: &str) -> Option<ShallowRouteFacts>;

    /// The per-symbol local/external dependency-edge classification (`None`
    /// ⟺ the legacy `type_deps(name)` miss).
    fn classified_deps(&self, name: &str) -> Option<ClassifiedRouteDeps>;

    /// Whether `name` is an import-local binding.
    fn is_import_local(&self, name: &str) -> bool;

    /// The whole-route external shell for an import-local binding (`None` ⟺
    /// an import-local with no target — the legacy missing-target arm).
    fn import_route_target(&self, name: &str) -> Option<ExternalRouteRefFact>;

    /// The deferred key-source hand-off: enumerate a local key alias to its
    /// FINITE literal key set. CONTENT-FREE — the provider owns the
    /// alias-graph follow over per-decl normalized `KeySourceFact`s (the
    /// engine-side dispatch: resolve each alias ref, recurse, poison on any
    /// unavailable hop, sort/dedup only a COMPLETED set); this closure only
    /// consumes the tri-state [`KeySourceLookup`] outcome.
    fn key_source_lookup(&self, name: &str) -> KeySourceLookup;
}

/// Tri-state deferred key-source enumeration outcome: source AVAILABILITY is a
/// separate axis from what the alias graph enumerates to. Conflating the two
/// turns a sanctioned under-production (an unavailable hand-off) into the
/// empty-keys fallback — a route the authoring walk never produced.
#[derive(Debug, Clone, PartialEq, Eq, verter_no_typeexpr::NoTypeExpr)]
pub enum KeySourceLookup {
    /// The alias graph enumerated to COMPLETION: the sorted, deduped literal
    /// key set — possibly empty (the legacy `utility → None` fall-through).
    Ready(Vec<String>),
    /// Header-decidable without any body demand: the alias names no
    /// file-scope TYPE symbol — it enumerates to zero keys (the legacy
    /// non-symbol arm), so the empty-keys fall-through applies.
    MissingTypeSymbol,
    /// A demanded key-source hop could not be enumerated (decl-body demand
    /// miss, broken lease, unresolved alias hop): the enumeration is
    /// UNDECIDED, not empty — the deferred edge fails closed and the
    /// empty-keys fallback must NOT fire.
    Unavailable,
}

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------

/// Closure status — 1:1 with the session `LocalClosureStatus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactClosureStatus {
    /// All same-file deps resolved; no external deps.
    Resolved,
    /// Same-file closure succeeded but external deps remain.
    ResolvedWithExternalDeps,
    /// A referenced local symbol does not exist in the file.
    MissingLocalSymbol {
        /// The missing name.
        name: String,
    },
    /// The local-closure step budget was exceeded.
    BudgetExceeded,
}

/// Closure result — 1:1 with the session `LocalClosureResult` (the session
/// converts `unresolved_external` to `ExternalSymbolRef` field-by-field).
#[derive(Debug, Clone)]
pub struct FactClosureResult {
    /// Bounded status.
    pub status: FactClosureStatus,
    /// All local symbol names that participate in this closure.
    pub local_symbols_used: Vec<String>,
    /// External refs discovered during closure (deduped by
    /// `(source_specifier, imported_name)`, route-merged, insertion-ordered).
    pub unresolved_external: Vec<ExternalRouteRefFact>,
    /// Number of local symbols visited.
    pub steps: u64,
}

/// Keyed-map external-ref accumulator over the fact carrier — merges
/// same-`(specifier, imported_name)` refs by route-union in O(1) per add,
/// insertion order preserved (the session accumulator's exact semantics).
#[derive(Debug, Default)]
struct FactExternalAccumulator {
    index: FxHashMap<(String, String), usize>,
    refs: Vec<ExternalRouteRefFact>,
}

impl FactExternalAccumulator {
    fn add(&mut self, ext_ref: ExternalRouteRefFact) {
        match self.index.entry((
            ext_ref.source_specifier.clone(),
            ext_ref.imported_name.clone(),
        )) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                let existing = &mut self.refs[*entry.get()];
                existing.route = merge_route_demands(&existing.route, &ext_ref.route);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(self.refs.len());
                self.refs.push(ext_ref);
            }
        }
    }

    fn into_vec(self) -> Vec<ExternalRouteRefFact> {
        self.refs
    }

    fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }
}

fn dependency_name(entry: &RouteDependencyRefFact) -> &str {
    match entry {
        RouteDependencyRefFact::Local { name, .. } => name,
        RouteDependencyRefFact::External(ext) => ext.local_name.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// The route-aware closure dispatch — 1:1 with the legacy
/// `ShallowFileState::route_closure` arms.
pub fn route_closure_over_facts(
    provider: &dyn RouteClosureProvider,
    symbol_name: &str,
    route: &RouteDemand,
    budget: usize,
) -> FactClosureResult {
    match route {
        RouteDemand::Whole => whole_route_closure(provider, symbol_name, budget),
        RouteDemand::MemberPath(path) if !path.is_empty() => {
            member_path_route_closure(provider, symbol_name, path, budget)
        }
        RouteDemand::MemberPath(_) => local_closure_over_facts(provider, symbol_name, budget),
        RouteDemand::Pick(members) => {
            let refs: Vec<&str> = members.iter().map(|s| s.as_str()).collect();
            member_route_closure(provider, symbol_name, &refs, budget)
        }
        RouteDemand::Omit(omitted) => {
            let Some(facts) = provider.route_facts(symbol_name) else {
                return local_closure_over_facts(provider, symbol_name, budget);
            };
            let MemberNamesRoute::Closed(names) = &facts.member_names else {
                // The open/undecidable carrier-stop never narrows an Omit —
                // conservative full local closure.
                return local_closure_over_facts(provider, symbol_name, budget);
            };
            if names.is_empty() {
                // Not a direct object with properties — the legacy
                // `direct_object_member_names → None` fallback.
                return local_closure_over_facts(provider, symbol_name, budget);
            }
            let remaining = names
                .iter()
                .filter(|name| !omitted.contains(name.as_str()))
                .collect::<Vec<_>>();
            if remaining.is_empty() {
                return FactClosureResult {
                    status: FactClosureStatus::Resolved,
                    local_symbols_used: vec![symbol_name.to_string()],
                    unresolved_external: Vec::new(),
                    steps: 1,
                };
            }
            let refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();
            member_route_closure(provider, symbol_name, &refs, budget)
        }
    }
}

/// The plain same-file closure — 1:1 with the legacy
/// `ShallowFileState::local_closure` (BFS over classified dependency edges).
pub fn local_closure_over_facts(
    provider: &dyn RouteClosureProvider,
    symbol_name: &str,
    budget: usize,
) -> FactClosureResult {
    let mut visited = FxHashSet::default();
    let mut pending = vec![symbol_name.to_string()];
    let mut external_refs = FactExternalAccumulator::default();
    let mut local_used = Vec::new();
    let mut steps: u64 = 0;

    while let Some(current) = pending.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        steps += 1;
        if steps as usize >= budget {
            return FactClosureResult {
                status: FactClosureStatus::BudgetExceeded,
                local_symbols_used: local_used,
                unresolved_external: external_refs.into_vec(),
                steps,
            };
        }

        if let Some(deps) = provider.classified_deps(&current) {
            local_used.push(current.clone());
            for dep in &deps.local_deps {
                if !visited.contains(dep.as_str()) {
                    pending.push(dep.clone());
                }
            }
            for ext in &deps.external_deps {
                external_refs.add(ext.clone());
            }
        } else if provider.is_import_local(&current) {
            if let Some(shell) = provider.import_route_target(&current) {
                external_refs.add(shell);
            } else {
                return FactClosureResult {
                    status: FactClosureStatus::MissingLocalSymbol { name: current },
                    local_symbols_used: local_used,
                    unresolved_external: external_refs.into_vec(),
                    steps,
                };
            }
        } else {
            return FactClosureResult {
                status: FactClosureStatus::MissingLocalSymbol { name: current },
                local_symbols_used: local_used,
                unresolved_external: external_refs.into_vec(),
                steps,
            };
        }
    }

    finish(local_used, external_refs, steps)
}

fn finish(
    local_used: Vec<String>,
    external_refs: FactExternalAccumulator,
    steps: u64,
) -> FactClosureResult {
    let unresolved_external = external_refs.into_vec();
    let status = if unresolved_external.is_empty() {
        FactClosureStatus::Resolved
    } else {
        FactClosureStatus::ResolvedWithExternalDeps
    };
    FactClosureResult {
        status,
        local_symbols_used: local_used,
        unresolved_external,
        steps,
    }
}

// ---------------------------------------------------------------------------
// Whole-route closure (transitive edge walk)
// ---------------------------------------------------------------------------

fn whole_route_closure(
    provider: &dyn RouteClosureProvider,
    symbol_name: &str,
    budget: usize,
) -> FactClosureResult {
    let Some(facts) = provider.route_facts(symbol_name) else {
        return local_closure_over_facts(provider, symbol_name, budget);
    };

    let mut walk = WholeRouteWalk {
        provider,
        budget,
        visited: FxHashSet::default(),
        local_used: Vec::new(),
        external_refs: FactExternalAccumulator::default(),
        steps: 1,
    };
    walk.visited.insert(symbol_name.to_string());
    walk.local_used.push(symbol_name.to_string());

    if !walk.walk_edges(&facts, WholeRouteContextFact::Root) {
        return FactClosureResult {
            status: FactClosureStatus::BudgetExceeded,
            local_symbols_used: walk.local_used,
            unresolved_external: walk.external_refs.into_vec(),
            steps: walk.steps,
        };
    }

    finish(walk.local_used, walk.external_refs, walk.steps)
}

struct WholeRouteWalk<'p> {
    provider: &'p dyn RouteClosureProvider,
    budget: usize,
    visited: FxHashSet<String>,
    local_used: Vec<String>,
    external_refs: FactExternalAccumulator,
    steps: u64,
}

impl WholeRouteWalk<'_> {
    /// Process one decl's stored direct edges under a follow context —
    /// reproducing the legacy body walk's context semantics over facts:
    /// under an emitting follow (`Root`/`CallableParam`) every stored edge
    /// applies with its own stored context; under a `LeafProperty` follow
    /// only fully-transparent sites (stored context `Root`) remain reachable,
    /// the import-emit gate re-applies there (`route == Whole` externals
    /// drop), and reachable local follows compose to a `LeafProperty` walk of
    /// the target.
    fn walk_edges(&mut self, facts: &ShallowRouteFacts, follow: WholeRouteContextFact) -> bool {
        let leaf_follow = matches!(follow, WholeRouteContextFact::LeafProperty);
        for edge in facts.whole_route_edges.iter() {
            match edge {
                WholeRouteEdgeFact::External {
                    external_ref,
                    context,
                } => {
                    if leaf_follow
                        && !(matches!(context, WholeRouteContextFact::Root)
                            && external_ref.route != RouteDemand::Whole)
                    {
                        continue;
                    }
                    self.external_refs.add(external_ref.clone());
                }
                WholeRouteEdgeFact::Local {
                    name,
                    route,
                    context,
                } => {
                    let Some(effective) = compose_follow(follow, *context) else {
                        continue;
                    };
                    if !self.follow_local_route(name, route, effective) {
                        return false;
                    }
                }
                WholeRouteEdgeFact::DeferredKeyUtility(deferred) => {
                    let Some(effective) = compose_follow(follow, deferred.context) else {
                        continue;
                    };
                    if !self.follow_deferred(deferred, effective) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Dispatch one local edge by its route demand — the legacy
    /// `follow_local_symbol_precise` (Whole) / `follow_routed_expr` (routed)
    /// reproduction.
    fn follow_local_route(
        &mut self,
        name: &str,
        route: &RouteDemand,
        effective: WholeRouteContextFact,
    ) -> bool {
        match route {
            RouteDemand::Whole => self.follow_local(name, effective),
            RouteDemand::MemberPath(path) => self.follow_local_member_path(name, path),
            RouteDemand::Pick(_) | RouteDemand::Omit(_) => {
                self.follow_local_sub_closure(name, route)
            }
        }
    }

    /// The legacy `follow_local_symbol_precise`: pre-gated on header
    /// membership (a spurious edge for an unknown name is a no-op charging
    /// NO budget), visited-once at the FIRST-visit context, charged BEFORE
    /// the decl read.
    fn follow_local(&mut self, name: &str, context: WholeRouteContextFact) -> bool {
        if !self.provider.has_type_symbol(name) {
            return true;
        }
        if !self.visited.insert(name.to_string()) {
            return true;
        }
        self.steps += 1;
        if self.steps as usize >= self.budget {
            return false;
        }
        let Some(facts) = self.provider.route_facts(name) else {
            return true;
        };
        self.local_used.push(name.to_string());
        self.walk_edges(&facts, context)
    }

    /// The legacy `follow_routed_expr` MemberPath-over-local branch: the
    /// target decl is read WITHOUT a visited mark or budget charge; the seed
    /// walk runs with its own fresh cycle guard; terminal seeds then follow
    /// at `Root` (context reset), seed imports emit whole-route externals.
    fn follow_local_member_path(&mut self, name: &str, path: &[String]) -> bool {
        if !self.provider.has_type_symbol(name) {
            return true;
        }
        let Some(facts) = self.provider.route_facts(name) else {
            return true;
        };
        let mut seed_entries = Vec::new();
        let mut seed_external = FactExternalAccumulator::default();
        let mut seen_symbols = FxHashSet::default();
        let found_path = member_path_seed_walk(
            self.provider,
            &facts,
            path,
            &mut seed_entries,
            &mut seed_external,
            &mut seen_symbols,
        );
        if !found_path {
            return true;
        }
        for ext in seed_external.into_vec() {
            self.external_refs.add(ext);
        }
        for entry in seed_entries {
            match entry {
                RouteDependencyRefFact::External(shell) => {
                    self.external_refs.add(ExternalRouteRefFact {
                        route: RouteDemand::Whole,
                        ..shell
                    });
                }
                RouteDependencyRefFact::Local { name: seed, .. } => {
                    if self.provider.has_type_symbol(&seed)
                        && !self.follow_local(&seed, WholeRouteContextFact::Root)
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// The legacy `follow_routed_expr` Pick/Omit-over-local branch: a FRESH
    /// sub-closure (own visited set, own step counter, full budget) whose
    /// locals merge into the outer visited/used sets and whose externals
    /// accumulate; only a budget trip propagates as failure.
    fn follow_local_sub_closure(&mut self, name: &str, route: &RouteDemand) -> bool {
        if !self.provider.has_type_symbol(name) {
            return true;
        }
        let closure = route_closure_over_facts(self.provider, name, route, self.budget);
        if matches!(closure.status, FactClosureStatus::BudgetExceeded) {
            return false;
        }
        for local_name in closure.local_symbols_used {
            if self.visited.insert(local_name.clone()) {
                self.local_used.push(local_name);
            }
        }
        for ext in closure.unresolved_external {
            self.external_refs.add(ext);
        }
        true
    }

    /// A deferred-key edge: enumerate the key source through the provider
    /// hand-off — tri-state: UNAVAILABLE fails closed (no fallback), a
    /// resolved-EMPTY enumeration applies the legacy empty-keys fall-through,
    /// a non-empty resolution applies the route to the stored base exactly
    /// like a literal-key site.
    fn follow_deferred(
        &mut self,
        deferred: &DeferredKeyUtilityEdge,
        effective: WholeRouteContextFact,
    ) -> bool {
        let KeyDomainFact::FollowSlot(slot) = &deferred.key_source else {
            // The producer only mints FollowSlot recipes; any other recipe
            // shape fails closed.
            return true;
        };
        let keys = match self.provider.key_source_lookup(slot.anchor.symbol.as_ref()) {
            KeySourceLookup::Unavailable => {
                // The key source could not be enumerated: the outcome is
                // UNDECIDED, not empty. Fail closed — the deferred edge
                // contributes nothing; firing the empty-keys fallback here
                // would emit a route the authoring walk never produced.
                return true;
            }
            // Header-decidable non-symbol alias: zero keys — the legacy
            // fall-through applies below.
            KeySourceLookup::MissingTypeSymbol => Vec::new(),
            KeySourceLookup::Ready(keys) => keys,
        };

        if keys.is_empty() {
            // The legacy `utility → None` fall-through: only the userland
            // local decl named `Pick`/`Omit` (captured at produce time)
            // remains, followed whole at the SITE context.
            if let Some(fallback) = &deferred.empty_keys_fallback {
                return self.follow_local(fallback, effective);
            }
            return true;
        }

        let route = match deferred.kind {
            DeferredKeyUtilityKind::Pick => RouteDemand::pick(keys),
            DeferredKeyUtilityKind::Omit => RouteDemand::omit(keys),
            DeferredKeyUtilityKind::IndexedAccess => {
                if keys.len() == 1 {
                    let mut full: Vec<String> = deferred.base_path.to_vec();
                    full.extend(keys);
                    RouteDemand::member_path(full)
                } else if deferred.base_path.is_empty() {
                    RouteDemand::pick(keys)
                } else {
                    return true;
                }
            }
        };

        match &deferred.base {
            None => true,
            Some(RouteDependencyRefFact::External(shell)) => {
                self.external_refs.add(ExternalRouteRefFact {
                    route,
                    ..shell.clone()
                });
                true
            }
            Some(RouteDependencyRefFact::Local { name, .. }) => match &route {
                RouteDemand::Whole => self.follow_local(name, WholeRouteContextFact::Root),
                RouteDemand::MemberPath(path) => self.follow_local_member_path(name, path),
                RouteDemand::Pick(_) | RouteDemand::Omit(_) => {
                    self.follow_local_sub_closure(name, &route)
                }
            },
        }
    }
}

/// Compose a stored edge context with the follow context: an emitting follow
/// keeps the stored context; a `LeafProperty` follow reaches ONLY
/// fully-transparent sites (stored `Root`), which then walk as `LeafProperty`.
fn compose_follow(
    follow: WholeRouteContextFact,
    stored: WholeRouteContextFact,
) -> Option<WholeRouteContextFact> {
    if matches!(follow, WholeRouteContextFact::LeafProperty) {
        if matches!(stored, WholeRouteContextFact::Root) {
            Some(WholeRouteContextFact::LeafProperty)
        } else {
            None
        }
    } else {
        Some(stored)
    }
}

// ---------------------------------------------------------------------------
// Member-path seed walk (over stored seed edges)
// ---------------------------------------------------------------------------

/// Reproduce the legacy `collect_member_path_seed_names` over stored seed
/// edges: an EXACT `TerminalDeps` match yields the sorted/deduped seed
/// entries; a strict-prefix `ForwardBoundary` forwards the remaining tail
/// (import ⇒ external `MemberPath(tail)`, local ⇒ cycle-guarded recursion
/// into the target's edges); no matching edge is the fail-closed MISS.
fn member_path_seed_walk(
    provider: &dyn RouteClosureProvider,
    facts: &ShallowRouteFacts,
    path: &[String],
    seed_entries: &mut Vec<RouteDependencyRefFact>,
    seed_external: &mut FactExternalAccumulator,
    seen_symbols: &mut FxHashSet<String>,
) -> bool {
    // Exact terminal match.
    if let Some(deps) = facts.member_path_seed_edges.iter().find_map(|edge| {
        match (&edge.depends_on, edge.path.as_ref() == path) {
            (MemberPathSeedTarget::TerminalDeps(deps), true) => Some(deps),
            _ => None,
        }
    }) {
        let mut entries: Vec<RouteDependencyRefFact> = deps.to_vec();
        entries.sort_by(|a, b| dependency_name(a).cmp(dependency_name(b)));
        entries.dedup_by(|a, b| dependency_name(a) == dependency_name(b));
        for entry in entries {
            if !seed_entries
                .iter()
                .any(|existing| dependency_name(existing) == dependency_name(&entry))
            {
                seed_entries.push(entry);
            }
        }
        return true;
    }

    // Strict-prefix forward boundary (unique per query: enumeration stops
    // below a carrier, so at most one carrier prefixes any path).
    if let Some((edge_path, target)) = facts.member_path_seed_edges.iter().find_map(|edge| {
        match (
            &edge.depends_on,
            edge.path.len() < path.len() && path.starts_with(&edge.path),
        ) {
            (MemberPathSeedTarget::ForwardBoundary(target), true) => {
                Some((edge.path.clone(), target.clone()))
            }
            _ => None,
        }
    }) {
        let tail = &path[edge_path.len()..];
        match target {
            RouteDependencyRefFact::External(shell) => {
                seed_external.add(ExternalRouteRefFact {
                    route: RouteDemand::member_path(tail.iter().cloned()),
                    ..shell
                });
                return true;
            }
            RouteDependencyRefFact::Local { name, .. } => {
                if !seen_symbols.insert(name.clone()) {
                    return false;
                }
                let result = provider.route_facts(&name).is_some_and(|target_facts| {
                    member_path_seed_walk(
                        provider,
                        &target_facts,
                        tail,
                        seed_entries,
                        seed_external,
                        seen_symbols,
                    )
                });
                seen_symbols.remove(name.as_str());
                return result;
            }
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Member (Pick) closure
// ---------------------------------------------------------------------------

fn member_route_closure(
    provider: &dyn RouteClosureProvider,
    symbol_name: &str,
    members: &[&str],
    budget: usize,
) -> FactClosureResult {
    let Some(facts) = provider.route_facts(symbol_name) else {
        return local_closure_over_facts(provider, symbol_name, budget);
    };

    // No member-dependency tracking ⇒ the legacy whole-closure fallback.
    if facts.member_dependency_edges.is_empty() {
        return local_closure_over_facts(provider, symbol_name, budget);
    }

    let direct_member_names: &[String] = match &facts.member_names {
        MemberNamesRoute::Closed(names) => names,
        MemberNamesRoute::OpenKeyDomain => &[],
    };

    let mut seed_entries: Vec<RouteDependencyRefFact> = Vec::new();
    let mut saw_known_member = false;
    for member in members {
        if let Some(edge) = facts
            .member_dependency_edges
            .iter()
            .find(|edge| edge.member == *member)
        {
            saw_known_member = true;
            for dep in edge.depends_on.iter() {
                if !seed_entries
                    .iter()
                    .any(|existing| dependency_name(existing) == dependency_name(dep))
                {
                    seed_entries.push(dep.clone());
                }
            }
            continue;
        }
        // Ref-less direct property: known member, no seeds (a ref-carrying
        // property always has a member-dependency edge, so the direct-object
        // fallback can only mark knownness).
        if direct_member_names.iter().any(|name| name == member) {
            saw_known_member = true;
        }
    }

    if !saw_known_member {
        return local_closure_over_facts(provider, symbol_name, budget);
    }

    if seed_entries.is_empty() {
        return FactClosureResult {
            status: FactClosureStatus::Resolved,
            local_symbols_used: vec![symbol_name.to_string()],
            unresolved_external: Vec::new(),
            steps: 1,
        };
    }

    seeded_dependency_bfs(
        provider,
        symbol_name,
        seed_entries,
        FactExternalAccumulator::default(),
        budget,
    )
}

// ---------------------------------------------------------------------------
// Member-path closure
// ---------------------------------------------------------------------------

fn member_path_route_closure(
    provider: &dyn RouteClosureProvider,
    symbol_name: &str,
    path: &[String],
    budget: usize,
) -> FactClosureResult {
    if path.len() == 1 {
        return member_route_closure(provider, symbol_name, &[path[0].as_str()], budget);
    }

    let Some(facts) = provider.route_facts(symbol_name) else {
        return local_closure_over_facts(provider, symbol_name, budget);
    };

    let mut seed_entries = Vec::new();
    let mut seed_external = FactExternalAccumulator::default();
    let mut seen_symbols = FxHashSet::default();
    let found_path = member_path_seed_walk(
        provider,
        &facts,
        path,
        &mut seed_entries,
        &mut seed_external,
        &mut seen_symbols,
    );

    if !found_path || (seed_entries.is_empty() && seed_external.is_empty()) {
        return FactClosureResult {
            status: FactClosureStatus::Resolved,
            local_symbols_used: vec![symbol_name.to_string()],
            unresolved_external: Vec::new(),
            steps: 1,
        };
    }

    seeded_dependency_bfs(provider, symbol_name, seed_entries, seed_external, budget)
}

/// The shared seeded dependency BFS (the legacy member / member-path closure
/// tail): LIFO over seed entries, `type_deps`-style expansion for locals
/// (import entries emit at pop), silent skip for unknown names.
fn seeded_dependency_bfs(
    provider: &dyn RouteClosureProvider,
    symbol_name: &str,
    seed_entries: Vec<RouteDependencyRefFact>,
    seed_external: FactExternalAccumulator,
    budget: usize,
) -> FactClosureResult {
    let mut visited = FxHashSet::default();
    visited.insert(symbol_name.to_string());
    let mut pending = seed_entries;
    let mut external_refs = seed_external;
    let mut local_used = vec![symbol_name.to_string()];
    let mut steps = 1u64;

    while let Some(entry) = pending.pop() {
        let current = dependency_name(&entry).to_string();
        if !visited.insert(current.clone()) {
            continue;
        }
        steps += 1;
        if steps as usize >= budget {
            return FactClosureResult {
                status: FactClosureStatus::BudgetExceeded,
                local_symbols_used: local_used,
                unresolved_external: external_refs.into_vec(),
                steps,
            };
        }

        if let Some(dep_edges) = provider.classified_deps(&current) {
            local_used.push(current.clone());
            for dep in &dep_edges.local_deps {
                if !visited.contains(dep.as_str()) {
                    pending.push(RouteDependencyRefFact::Local {
                        name: dep.clone(),
                        route: RouteDemand::Whole,
                    });
                }
            }
            for ext in &dep_edges.external_deps {
                external_refs.add(ext.clone());
            }
        } else if let RouteDependencyRefFact::External(shell) = &entry {
            external_refs.add(shell.clone());
        } else if provider.is_import_local(&current) {
            if let Some(shell) = provider.import_route_target(&current) {
                external_refs.add(shell);
            }
        }
        // Unknown names skip silently — they may be type parameters.
    }

    finish(local_used, external_refs, steps)
}
