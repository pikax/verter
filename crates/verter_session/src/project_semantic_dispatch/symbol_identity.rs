//! Exact symbol-identity demand over the shared semantic dispatcher.
//!
//! This adapter owns no name or import resolver. Every declaration and
//! instantiation hop delegates to the canonical `ResolveDecl` / `Instantiate`
//! queries, preserving their dependency reads, fixed view, connected work
//! envelope, and cache-suppression rails.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_type_expr::facts::AuthoredReferenceHeadFact;
use verter_type_expr::{
    PropCallableRoleUnresolvedReason, ResolutionExactness, ResolvedSymbolIdentity,
};

use super::ProjectSemanticDispatch;
use crate::resolver_core::shallow_file_state::ExportTarget;
use crate::semantic_query::{
    PartialReasonSet, ProjectionMode, ProjectionReductionContext, QueryError, QueryResult,
    ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

/// Typed identity-demand outcome. A partial result deliberately carries no
/// graph node or speculative identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SymbolIdentityDemandOutcome {
    /// Complete resolution. `None` means the subject has no single declaration
    /// identity and therefore cannot match a nominal feature role.
    Complete(Option<ResolvedSymbolIdentity>),
    /// Resolution was incomplete; callers must fail closed.
    Partial(PropCallableRoleUnresolvedReason),
}

/// Exact identity proof paired with the terminal carrier that proved it.
///
/// The carrier is the fully substituted `InstantiationRef` reached after
/// traversing every alias hop through the shared dispatcher. Feature readers
/// must use its arguments rather than the authored arguments of an outer alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalSymbolInstantiation {
    pub(crate) symbol: ResolvedSymbolIdentity,
    pub(crate) args: Arc<[SemanticNodeId]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedReferenceRoute {
    pub(crate) authored_head: AuthoredReferenceHeadFact,
    pub(crate) owner_canonical: Arc<str>,
    pub(crate) local_binding: Arc<str>,
    pub(crate) import_source: Arc<str>,
    pub(crate) imported_name: Arc<str>,
    pub(crate) terminal_import_source: Arc<str>,
    pub(crate) local_alias_hops: Arc<[Arc<str>]>,
    pub(crate) terminal: ResolvedSymbolIdentity,
    pub(crate) exactness: ResolutionExactness,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalSymbolInstantiationDemandOutcome {
    Complete(Option<TerminalSymbolInstantiation>),
    Partial(PropCallableRoleUnresolvedReason),
}

enum IdentitySubject {
    Node(SemanticNodeId),
    Symbol(ResolvedSymbolIdentity),
}

/// The authored first import hop of a reference route:
/// `(owner_canonical, local_binding, source_specifier, imported_name)`.
type AuthoredImportEdge = (Arc<str>, Arc<str>, Arc<str>, Arc<str>);

impl ProjectSemanticDispatch<'_> {
    /// Compose exact authored-route evidence through requested local aliases
    /// and the shared direct-import route authority.
    pub(crate) fn resolve_authored_reference_route(
        &self,
        owner_canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        origin: &AuthoredReferenceHeadFact,
    ) -> Result<Option<ResolvedReferenceRoute>, PropCallableRoleUnresolvedReason> {
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            return Err(partial_reason(reasons));
        }
        let (mut head, mut pending_members, import_type_specifier) = match origin {
            AuthoredReferenceHeadFact::Bare { local_name, .. } => {
                (Arc::clone(local_name), Vec::new(), None)
            }
            AuthoredReferenceHeadFact::Qualified {
                local_root,
                member_path,
                ..
            } => (
                Arc::clone(local_root),
                member_path.iter().cloned().collect(),
                None,
            ),
            AuthoredReferenceHeadFact::NotReference => return Ok(None),
            AuthoredReferenceHeadFact::ImportType {
                specifier,
                member_path,
                ..
            } => {
                let Some((first, rest)) = member_path.split_first() else {
                    return Err(PropCallableRoleUnresolvedReason::Unsupported);
                };
                (
                    Arc::clone(first),
                    rest.to_vec(),
                    Some(Arc::clone(specifier)),
                )
            }
            AuthoredReferenceHeadFact::Unavailable => {
                return Err(PropCallableRoleUnresolvedReason::Unsupported)
            }
        };
        let mut aliases = Vec::new();
        let mut visited = FxHashSet::default();
        let mut current_canonical = Arc::<str>::from(owner_canonical);
        let mut current_owner = owner;
        let mut resolve_export = false;
        let mut import_edge: Option<AuthoredImportEdge> = None;
        let mut terminal_import_source: Option<Arc<str>> = None;

        if let Some(specifier) = import_type_specifier {
            let target = self
                .ctx
                .resolve_type_dependency_canonical(owner_canonical, specifier.as_ref())
                .ok_or(PropCallableRoleUnresolvedReason::MissingDependency)?;
            import_edge = Some((
                Arc::from(owner_canonical),
                Arc::from(""),
                Arc::clone(&specifier),
                Arc::clone(&head),
            ));
            terminal_import_source = Some(Arc::clone(&specifier));
            let (resolved, route_facts) = self
                .ctx
                .resolve_imported_type_root_with_facts(&target, head.as_ref());
            self.ctx.observe_borrowed_signature(&route_facts);
            let _terminal = resolved.ok_or(PropCallableRoleUnresolvedReason::MissingDependency)?;
            current_canonical = Arc::from(target);
            current_owner = verter_type_expr::TopLevelOwnerId::instance(0);
            resolve_export = true;
        }
        loop {
            if !visited.insert((
                Arc::clone(&current_canonical),
                current_owner,
                Arc::clone(&head),
            )) {
                return Err(PropCallableRoleUnresolvedReason::Cycle);
            }
            if let Err(reasons) = self.charge_connected_work() {
                return Err(partial_reason(reasons));
            }
            let shallow = self
                .ctx
                .shallow_file_state(current_canonical.as_ref())
                .ok_or(PropCallableRoleUnresolvedReason::MissingDependency)?;
            if resolve_export {
                match shallow.export_target(head.as_ref()) {
                    Some(ExportTarget::Local { owner, symbol_name }) => {
                        current_owner = *owner;
                        head = Arc::from(symbol_name.as_str());
                        resolve_export = false;
                    }
                    Some(ExportTarget::Reexport {
                        source_specifier,
                        original_name,
                        ..
                    }) => {
                        terminal_import_source = Some(Arc::from(source_specifier.as_str()));
                        let target = self
                            .ctx
                            .resolve_type_dependency_canonical(
                                current_canonical.as_ref(),
                                source_specifier,
                            )
                            .ok_or(PropCallableRoleUnresolvedReason::MissingDependency)?;
                        current_canonical = Arc::from(target);
                        current_owner = verter_type_expr::TopLevelOwnerId::instance(0);
                        head = Arc::from(original_name.as_str());
                        continue;
                    }
                    None => return Err(PropCallableRoleUnresolvedReason::MissingDependency),
                }
            }
            if let Some(crate::resolver_core::shallow_file_state::LexicalValueBinding::Import(
                target,
            )) = shallow.visible_value_binding(current_owner, head.as_ref())
            {
                let routed_name = if target.is_namespace {
                    if pending_members.is_empty() {
                        return Err(PropCallableRoleUnresolvedReason::Unsupported);
                    }
                    pending_members.remove(0)
                } else {
                    if !pending_members.is_empty() {
                        return Err(PropCallableRoleUnresolvedReason::Unsupported);
                    }
                    Arc::from(target.imported_name.as_str())
                };
                terminal_import_source = Some(Arc::from(target.source_specifier.as_str()));
                if import_edge.is_none() {
                    import_edge = Some((
                        Arc::clone(&current_canonical),
                        Arc::clone(&head),
                        Arc::from(target.source_specifier.as_str()),
                        Arc::clone(&routed_name),
                    ));
                }
                let target_canonical = self
                    .ctx
                    .resolve_type_dependency_canonical(
                        current_canonical.as_ref(),
                        &target.source_specifier,
                    )
                    .ok_or(PropCallableRoleUnresolvedReason::MissingDependency)?;
                let (resolved, route_facts) = self
                    .ctx
                    .resolve_imported_type_root_with_facts(&target_canonical, routed_name.as_ref());
                self.ctx.observe_borrowed_signature(&route_facts);
                let _terminal =
                    resolved.ok_or(PropCallableRoleUnresolvedReason::MissingDependency)?;
                current_canonical = Arc::from(target_canonical);
                current_owner = verter_type_expr::TopLevelOwnerId::instance(0);
                head = routed_name;
                resolve_export = true;
                continue;
            }

            if !pending_members.is_empty() {
                return Err(PropCallableRoleUnresolvedReason::Unsupported);
            }

            let prepared = self
                .ctx
                .prepared_type_decl(current_canonical.as_ref(), current_owner, head.as_ref())
                .map_err(|_| PropCallableRoleUnresolvedReason::Fault)?
                .ok_or(PropCallableRoleUnresolvedReason::Unsupported)?;
            let verter_type_expr::facts::PreparedProjectionClassFact::ForwardSubject(forward) =
                &prepared.projection_class
            else {
                let Some((edge_owner, local_binding, import_source, imported_name)) = import_edge
                else {
                    return Ok(None);
                };
                return Ok(Some(ResolvedReferenceRoute {
                    authored_head: origin.clone(),
                    owner_canonical: edge_owner,
                    local_binding,
                    import_source,
                    imported_name,
                    terminal_import_source: terminal_import_source
                        .expect("a resolved reference route has an import edge"),
                    local_alias_hops: Arc::from(aliases),
                    terminal: ResolvedSymbolIdentity {
                        canonical_id: current_canonical,
                        owner: current_owner,
                        symbol: head,
                    },
                    exactness: ResolutionExactness::ExactSymbolic,
                }));
            };
            aliases.push(Arc::clone(&head));
            head = Arc::from(forward.target_name.as_str());
        }
    }

    /// Resolve `node`, stopping as soon as an exact expected declaration
    /// identity is reached. This preserves direct package-export proof without
    /// expanding the export's unrelated body.
    pub(crate) fn demand_symbol_identity(
        &self,
        node: SemanticNodeId,
        expected: &[ResolvedSymbolIdentity],
    ) -> SymbolIdentityDemandOutcome {
        self.demand_symbol_identity_subject(IdentitySubject::Node(node), expected)
    }

    /// Resolve an exact expected symbol and retain the terminal
    /// `InstantiationRef` arguments at the match point.
    pub(crate) fn demand_terminal_symbol_instantiation(
        &self,
        node: SemanticNodeId,
        expected: &[ResolvedSymbolIdentity],
    ) -> TerminalSymbolInstantiationDemandOutcome {
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            let reason = partial_reason(reasons);
            self.fold_local_partial_completeness(reason_partial_set(reason));
            return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
        }

        let context =
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
        let mut subject = IdentitySubject::Node(node);
        let mut visited_nodes = FxHashSet::default();
        let mut visited_symbols = FxHashSet::default();

        loop {
            match subject {
                IdentitySubject::Symbol(symbol) => {
                    if expected.contains(&symbol) {
                        return TerminalSymbolInstantiationDemandOutcome::Complete(None);
                    }
                    if !visited_symbols.insert(symbol.clone()) {
                        let reason = PropCallableRoleUnresolvedReason::Cycle;
                        self.fold_local_partial_completeness(reason_partial_set(reason));
                        return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                    }
                    if let Err(reasons) = self.charge_connected_work() {
                        let reason = partial_reason(reasons);
                        self.fold_local_partial_completeness(reason_partial_set(reason));
                        return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                    }
                    let read = self.execute_read(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: ScopeId::file(Arc::clone(&symbol.canonical_id), symbol.owner),
                        name: Arc::clone(&symbol.symbol),
                    }));
                    subject = match self.identity_read_subject(read) {
                        Ok(next) => IdentitySubject::Node(next),
                        Err(reason) => {
                            self.fold_local_partial_completeness(reason_partial_set(reason));
                            return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                        }
                    };
                }
                IdentitySubject::Node(current) => {
                    let Some(data) = self.graph().node_data(current) else {
                        let reason = PropCallableRoleUnresolvedReason::Fault;
                        self.fold_local_partial_completeness(reason_partial_set(reason));
                        return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                    };
                    match data.as_ref() {
                        SemanticNodeData::Alias(next) => {
                            if !visited_nodes.insert(current) {
                                let reason = PropCallableRoleUnresolvedReason::Cycle;
                                self.fold_local_partial_completeness(reason_partial_set(reason));
                                return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                            }
                            if let Err(reasons) = self.charge_connected_work() {
                                let reason = partial_reason(reasons);
                                self.fold_local_partial_completeness(reason_partial_set(reason));
                                return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                            }
                            subject = IdentitySubject::Node(*next);
                        }
                        SemanticNodeData::DeclRef { identity } => {
                            subject = IdentitySubject::Symbol(ResolvedSymbolIdentity {
                                canonical_id: Arc::clone(&identity.canonical_id),
                                owner: identity.owner,
                                symbol: Arc::clone(&identity.decl_name),
                            });
                        }
                        SemanticNodeData::InstantiationRef { base, args } => {
                            let symbol = ResolvedSymbolIdentity {
                                canonical_id: Arc::clone(&base.canonical_id),
                                owner: base.owner,
                                symbol: Arc::clone(&base.decl_name),
                            };
                            if expected.contains(&symbol) {
                                return TerminalSymbolInstantiationDemandOutcome::Complete(Some(
                                    TerminalSymbolInstantiation {
                                        symbol,
                                        args: Arc::clone(args),
                                    },
                                ));
                            }
                            if !visited_symbols.insert(symbol.clone()) {
                                let reason = PropCallableRoleUnresolvedReason::Cycle;
                                self.fold_local_partial_completeness(reason_partial_set(reason));
                                return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                            }
                            if let Err(reasons) = self.charge_connected_work() {
                                let reason = partial_reason(reasons);
                                self.fold_local_partial_completeness(reason_partial_set(reason));
                                return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                            }
                            let slot = self.type_slot_for(
                                Arc::clone(&base.canonical_id),
                                base.owner,
                                Arc::clone(&base.decl_name),
                            );
                            let read = self.execute_read(SemanticQueryKey::Instantiate(
                                crate::semantic_query::InstantiateKey::new(
                                    slot,
                                    Arc::clone(args),
                                    self.instantiate_context_for(&base.canonical_id, context),
                                ),
                            ));
                            subject = match self.identity_read_subject(read) {
                                Ok(next) if next != current => IdentitySubject::Node(next),
                                Ok(_) => {
                                    let reason = PropCallableRoleUnresolvedReason::Cycle;
                                    self.fold_local_partial_completeness(reason_partial_set(
                                        reason,
                                    ));
                                    return TerminalSymbolInstantiationDemandOutcome::Partial(
                                        reason,
                                    );
                                }
                                Err(reason) => {
                                    self.fold_local_partial_completeness(reason_partial_set(
                                        reason,
                                    ));
                                    return TerminalSymbolInstantiationDemandOutcome::Partial(
                                        reason,
                                    );
                                }
                            };
                        }
                        SemanticNodeData::Opaque(error) => {
                            let reason = query_error_reason(error);
                            self.fold_local_partial_completeness(reason_partial_set(reason));
                            return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                        }
                        SemanticNodeData::RawFallback { .. } => {
                            let reason = PropCallableRoleUnresolvedReason::Unsupported;
                            self.fold_local_partial_completeness(reason_partial_set(reason));
                            return TerminalSymbolInstantiationDemandOutcome::Partial(reason);
                        }
                        _ => return TerminalSymbolInstantiationDemandOutcome::Complete(None),
                    }
                }
            }
        }
    }

    fn demand_symbol_identity_subject(
        &self,
        mut subject: IdentitySubject,
        expected: &[ResolvedSymbolIdentity],
    ) -> SymbolIdentityDemandOutcome {
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            return self.partial_symbol_identity(partial_reason(reasons));
        }

        let context =
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
        let mut visited_nodes = FxHashSet::default();
        let mut visited_symbols = FxHashSet::default();
        let mut last_symbol = None;

        loop {
            match subject {
                IdentitySubject::Symbol(symbol) => {
                    if expected.contains(&symbol) {
                        return SymbolIdentityDemandOutcome::Complete(Some(symbol));
                    }
                    if !visited_symbols.insert(symbol.clone()) {
                        return self
                            .partial_symbol_identity(PropCallableRoleUnresolvedReason::Cycle);
                    }
                    if let Err(reasons) = self.charge_connected_work() {
                        return self.partial_symbol_identity(partial_reason(reasons));
                    }
                    last_symbol = Some(symbol.clone());
                    let read = self.execute_read(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: ScopeId::file(Arc::clone(&symbol.canonical_id), symbol.owner),
                        name: Arc::clone(&symbol.symbol),
                    }));
                    subject = match self.identity_read_subject(read) {
                        Ok(node) => IdentitySubject::Node(node),
                        Err(reason) => return self.partial_symbol_identity(reason),
                    };
                }
                IdentitySubject::Node(node) => {
                    let Some(data) = self.graph().node_data(node) else {
                        return self
                            .partial_symbol_identity(PropCallableRoleUnresolvedReason::Fault);
                    };
                    match data.as_ref() {
                        SemanticNodeData::Alias(next) => {
                            if !visited_nodes.insert(node) {
                                return self.partial_symbol_identity(
                                    PropCallableRoleUnresolvedReason::Cycle,
                                );
                            }
                            if let Err(reasons) = self.charge_connected_work() {
                                return self.partial_symbol_identity(partial_reason(reasons));
                            }
                            subject = IdentitySubject::Node(*next);
                        }
                        SemanticNodeData::DeclRef { identity } => {
                            subject = IdentitySubject::Symbol(ResolvedSymbolIdentity {
                                canonical_id: Arc::clone(&identity.canonical_id),
                                owner: identity.owner,
                                symbol: Arc::clone(&identity.decl_name),
                            });
                        }
                        SemanticNodeData::InstantiationRef { base, args } => {
                            let symbol = ResolvedSymbolIdentity {
                                canonical_id: Arc::clone(&base.canonical_id),
                                owner: base.owner,
                                symbol: Arc::clone(&base.decl_name),
                            };
                            if expected.contains(&symbol) {
                                return SymbolIdentityDemandOutcome::Complete(Some(symbol));
                            }
                            if !visited_symbols.insert(symbol.clone()) {
                                return self.partial_symbol_identity(
                                    PropCallableRoleUnresolvedReason::Cycle,
                                );
                            }
                            if let Err(reasons) = self.charge_connected_work() {
                                return self.partial_symbol_identity(partial_reason(reasons));
                            }
                            last_symbol = Some(symbol);
                            let slot = self.type_slot_for(
                                Arc::clone(&base.canonical_id),
                                base.owner,
                                Arc::clone(&base.decl_name),
                            );
                            let read = self.execute_read(SemanticQueryKey::Instantiate(
                                crate::semantic_query::InstantiateKey::new(
                                    slot,
                                    Arc::clone(args),
                                    self.instantiate_context_for(&base.canonical_id, context),
                                ),
                            ));
                            subject = match self.identity_read_subject(read) {
                                Ok(next) if next != node => IdentitySubject::Node(next),
                                Ok(_) => {
                                    return self.partial_symbol_identity(
                                        PropCallableRoleUnresolvedReason::Cycle,
                                    )
                                }
                                Err(reason) => return self.partial_symbol_identity(reason),
                            };
                        }
                        SemanticNodeData::BareRef(_)
                        | SemanticNodeData::ImportType(_)
                        | SemanticNodeData::TypeOf(_)
                        | SemanticNodeData::TypeOfNominal(_) => {
                            if !visited_nodes.insert(node) {
                                return self.partial_symbol_identity(
                                    PropCallableRoleUnresolvedReason::Cycle,
                                );
                            }
                            if let Err(reasons) = self.charge_connected_work() {
                                return self.partial_symbol_identity(partial_reason(reasons));
                            }
                            drop(data);
                            // The callable role is a STRUCTURAL question and
                            // a `unique symbol` is definitively not one of
                            // the symbol identities this walk looks for. A
                            // nominal carrier is terminal under carrier
                            // resolution, so the `resolved == node` rail
                            // below would publish a resolution-FAILURE
                            // marker for a program that resolves fine.
                            // Widen and let the walk terminate on the
                            // concrete primitive.
                            if let Some(widened) = self.widened_nominal_typeof(node) {
                                subject = IdentitySubject::Node(widened);
                                continue;
                            }
                            let (resolved, observed, completeness) =
                                self.resolve_carrier_subject_node_capturing_suppress(node, context);
                            if observed.result_is_partial {
                                let reason = partial_reason(completeness.reasons());
                                return self.partial_symbol_identity(reason);
                            }
                            if resolved == node {
                                return self.partial_symbol_identity(
                                    PropCallableRoleUnresolvedReason::MissingDependency,
                                );
                            }
                            subject = IdentitySubject::Node(resolved);
                        }
                        SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                            canonical_id,
                            owner,
                            name,
                            ..
                        }) => {
                            let symbol = ResolvedSymbolIdentity {
                                canonical_id: Arc::clone(canonical_id),
                                owner: *owner,
                                symbol: Arc::clone(name),
                            };
                            if expected.contains(&symbol) {
                                return SymbolIdentityDemandOutcome::Complete(Some(symbol));
                            }
                            last_symbol = Some(symbol);
                            if !visited_nodes.insert(node) {
                                return self.partial_symbol_identity(
                                    PropCallableRoleUnresolvedReason::Cycle,
                                );
                            }
                            if let Err(reasons) = self.charge_connected_work() {
                                return self.partial_symbol_identity(partial_reason(reasons));
                            }
                            let slot = self.type_slot_for(
                                Arc::clone(canonical_id),
                                *owner,
                                Arc::clone(name),
                            );
                            let read = self.execute_read(SemanticQueryKey::Instantiate(
                                crate::semantic_query::InstantiateKey::new(
                                    slot,
                                    Arc::from([]),
                                    self.instantiate_context_for(canonical_id, context),
                                ),
                            ));
                            subject = match self.identity_read_subject(read) {
                                Ok(next) if next != node => IdentitySubject::Node(next),
                                Ok(_) => {
                                    return self.partial_symbol_identity(
                                        PropCallableRoleUnresolvedReason::Cycle,
                                    )
                                }
                                Err(reason) => return self.partial_symbol_identity(reason),
                            };
                        }
                        SemanticNodeData::Opaque(error) => {
                            return self.partial_symbol_identity(query_error_reason(error));
                        }
                        SemanticNodeData::RawFallback { .. } => {
                            return self.partial_symbol_identity(
                                PropCallableRoleUnresolvedReason::Unsupported,
                            );
                        }
                        _ => return SymbolIdentityDemandOutcome::Complete(last_symbol),
                    }
                }
            }
        }
    }

    fn identity_read_subject(
        &self,
        read: crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
    ) -> Result<SemanticNodeId, PropCallableRoleUnresolvedReason> {
        crate::request_context::observe_component_meta_read_suppress(&read);
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &read.dep_signature);
        match read.value {
            QueryResult::Recursive(_) => Err(PropCallableRoleUnresolvedReason::Cycle),
            QueryResult::Error(error) => Err(query_error_reason(&error)),
            QueryResult::Value(_) if read.result_is_partial => {
                Err(PropCallableRoleUnresolvedReason::Fault)
            }
            QueryResult::Value(node) => Ok(node),
        }
    }

    fn partial_symbol_identity(
        &self,
        reason: PropCallableRoleUnresolvedReason,
    ) -> SymbolIdentityDemandOutcome {
        self.fold_local_partial_completeness(reason_partial_set(reason));
        SymbolIdentityDemandOutcome::Partial(reason)
    }
}

fn partial_reason(reasons: PartialReasonSet) -> PropCallableRoleUnresolvedReason {
    if reasons.contains(PartialReasonSet::BUDGET_EXCEEDED) {
        PropCallableRoleUnresolvedReason::BudgetExceeded
    } else if reasons.contains(PartialReasonSet::SAME_PATH_RECURSION) {
        PropCallableRoleUnresolvedReason::Cycle
    } else if reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT)
        || reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT)
    {
        PropCallableRoleUnresolvedReason::WorkLimitExceeded
    } else if reasons.contains(PartialReasonSet::MISSING_DEPENDENCY) {
        PropCallableRoleUnresolvedReason::MissingDependency
    } else {
        PropCallableRoleUnresolvedReason::Fault
    }
}

/// The typed [`PartialReasonSet`] one [`QueryError`] carrier contributes when
/// a consumer refuses to publish a surface built on it.
///
/// The ONE composition of the two classifiers below, so a second consumer
/// cannot spell a divergent class for the same carrier: an unresolved
/// declaration is a missing dependency, a budget trip is a budget trip, and a
/// cycle carrier is a same-path recursion. `Cancelled` / `UnstableState`
/// keep their DEDICATED bits (matching the runtime-side classifier in
/// `broad_runtime`): the coarse role enum below has no arm for them (a
/// cancelled read IS a `Fault` role), but the partial CLASS is
/// consumer-observable — `partial_failure`'s Cancelled / UnstableState
/// arms and the connected-limit cancel fast path branch on the exact bit,
/// and collapsing it to `SEMANTIC_QUERY_FAULT` would misclassify an
/// operational interruption as a semantic fault.
#[must_use]
pub(crate) fn query_error_partial_reasons(error: &QueryError) -> PartialReasonSet {
    match error {
        QueryError::Cancelled => PartialReasonSet::CANCELLED,
        QueryError::UnstableState { .. } | QueryError::StaleSemanticOperand => {
            PartialReasonSet::UNSTABLE_STATE
        }
        QueryError::IncompleteSemanticOperand { reasons } => *reasons,
        other => reason_partial_set(query_error_reason(other)),
    }
}

fn query_error_reason(error: &QueryError) -> PropCallableRoleUnresolvedReason {
    match error {
        QueryError::Miss | QueryError::DeclPlaceholder { .. } | QueryError::RaiseMiss => {
            PropCallableRoleUnresolvedReason::MissingDependency
        }
        // Non-fault surface signals: like `Miss`, an open surface or an
        // unmodeled flow position is a well-formed "no resolved role", not
        // a request fault.
        QueryError::OpenSurface | QueryError::UnmodeledPosition => {
            PropCallableRoleUnresolvedReason::MissingDependency
        }
        QueryError::BudgetExceeded(_) | QueryError::SignatureOverflow => {
            PropCallableRoleUnresolvedReason::BudgetExceeded
        }
        QueryError::AliasCycle { .. }
        | QueryError::RecursiveRef { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::TypeParamCycle => PropCallableRoleUnresolvedReason::Cycle,
        QueryError::UnsupportedIntrinsic { .. }
        | QueryError::UnrepresentableSurface
        | QueryError::UnrepresentableSurfaceMember => PropCallableRoleUnresolvedReason::Unsupported,
        QueryError::Cancelled
        | QueryError::UnstableState { .. }
        | QueryError::ForeignSemanticOperand
        | QueryError::StaleSemanticOperand
        | QueryError::IncompleteSemanticOperand { .. }
        | QueryError::Other(_)
        | QueryError::ValueDomainMismatch { .. } => PropCallableRoleUnresolvedReason::Fault,
    }
}

fn reason_partial_set(reason: PropCallableRoleUnresolvedReason) -> PartialReasonSet {
    match reason {
        PropCallableRoleUnresolvedReason::MissingDependency => PartialReasonSet::MISSING_DEPENDENCY,
        PropCallableRoleUnresolvedReason::Cycle => PartialReasonSet::SAME_PATH_RECURSION,
        PropCallableRoleUnresolvedReason::BudgetExceeded => PartialReasonSet::BUDGET_EXCEEDED,
        PropCallableRoleUnresolvedReason::WorkLimitExceeded => {
            PartialReasonSet::PROJECTION_WORK_LIMIT
        }
        PropCallableRoleUnresolvedReason::AnalysisUnavailable
        | PropCallableRoleUnresolvedReason::Unsupported
        | PropCallableRoleUnresolvedReason::Fault => PartialReasonSet::SEMANTIC_QUERY_FAULT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_semantic_dispatch::carrier_head_resolution_tests::{
        bare_ref_carrier, file_scope, host, import_type_carrier, upsert_ts,
    };
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::resolver_core::{BudgetDomain, BudgetExceededFailure};

    fn expected() -> ResolvedSymbolIdentity {
        ResolvedSymbolIdentity {
            canonical_id: Arc::from("/node_modules/svelte/index.d.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("Snippet"),
        }
    }

    fn demand_error(error: QueryError) -> SymbolIdentityDemandOutcome {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let dispatch = ProjectSemanticDispatch::new(&host);
        let node = dispatch
            .graph()
            .intern_node(SemanticNodeData::Opaque(error));
        dispatch.demand_symbol_identity(node, &[expected()])
    }

    #[test]
    fn partial_identity_reasons_are_typed_and_carry_no_node() {
        assert_eq!(
            demand_error(QueryError::RecursiveRef {
                name: Arc::from("Cycle")
            }),
            SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::Cycle)
        );
        assert_eq!(
            demand_error(QueryError::BudgetExceeded(BudgetExceededFailure {
                domain: BudgetDomain::ProjectionOperation,
                limit: 1,
                actual: 2,
                context: "identity-demand".to_string(),
            })),
            SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::BudgetExceeded)
        );
        assert_eq!(
            demand_error(QueryError::Miss),
            SymbolIdentityDemandOutcome::Partial(
                PropCallableRoleUnresolvedReason::MissingDependency
            )
        );
    }

    /// `Cancelled` / `UnstableState` carriers keep their DEDICATED partial
    /// classes through the shared carrier classification — the same bits the
    /// runtime-side classifier returns for the same carriers — so consumers
    /// that branch on the exact class (`partial_failure`'s Cancelled /
    /// UnstableState arms, the connected-limit cancel fast path) observe the
    /// operational cause rather than the generic semantic-fault class. The
    /// coarse role outcome stays `Fault` (the role enum has no cancel arm).
    #[test]
    fn cancelled_and_unstable_carriers_keep_dedicated_partial_classes() {
        assert_eq!(
            query_error_partial_reasons(&QueryError::Cancelled),
            PartialReasonSet::CANCELLED,
            "a cancelled carrier must classify as CANCELLED, not SEMANTIC_QUERY_FAULT"
        );
        assert_eq!(
            query_error_partial_reasons(&QueryError::UnstableState { attempts: 3 }),
            PartialReasonSet::UNSTABLE_STATE,
            "a torn-state carrier must classify as UNSTABLE_STATE, not SEMANTIC_QUERY_FAULT"
        );
        assert_eq!(
            demand_error(QueryError::Cancelled),
            SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::Fault),
            "the coarse role classification keeps its Fault arm"
        );
    }

    #[test]
    fn unsupported_and_work_limited_identity_demands_fail_closed() {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let dispatch = ProjectSemanticDispatch::new(&host);
        let raw = dispatch.graph().intern_node(SemanticNodeData::RawFallback {
            value: verter_type_expr::UnknownValue::wire_opaque("unsupported"),
        });
        assert_eq!(
            dispatch.demand_symbol_identity(raw, &[expected()]),
            SymbolIdentityDemandOutcome::Partial(PropCallableRoleUnresolvedReason::Unsupported)
        );

        let terminal = dispatch.graph().intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
        let alias = dispatch
            .graph()
            .intern_node(SemanticNodeData::Alias(terminal));
        dispatch.connected_work_limit_for_tests.set(0);
        let _scope = crate::request_context::ColdComputeCompletenessScope::enter();
        assert_eq!(
            dispatch.demand_symbol_identity(alias, &[expected()]),
            SymbolIdentityDemandOutcome::Partial(
                PropCallableRoleUnresolvedReason::WorkLimitExceeded
            )
        );
        assert!(
            crate::request_context::current_cold_compute_completeness().is_partial(),
            "a partial identity demand must refuse warm admission"
        );
    }

    /// Mutation recipe: map the partial observation returned by
    /// `resolve_carrier_subject_node_capturing_suppress` to `Fault`; the
    /// work-limited assertion must fail while the missing control stays green.
    #[test]
    fn real_carrier_path_preserves_partial_reason_classes() {
        let host = host();
        upsert_ts(
            &host,
            "/identity.ts",
            "export type Target = { value: string };\n",
        );
        let dispatch = ProjectSemanticDispatch::new(&host);
        let scope = file_scope(&dispatch, "/identity.ts");

        let missing = bare_ref_carrier(&dispatch, "Missing", scope.clone(), &[]);
        assert_eq!(
            dispatch.demand_symbol_identity(missing, &[expected()]),
            SymbolIdentityDemandOutcome::Partial(
                PropCallableRoleUnresolvedReason::MissingDependency
            ),
            "an unresolved real carrier must preserve MissingDependency"
        );

        let target = bare_ref_carrier(&dispatch, "Target", scope, &[]);
        dispatch.set_connected_limits_for_tests(1, u16::MAX);
        assert_eq!(
            dispatch.demand_symbol_identity(target, &[expected()]),
            SymbolIdentityDemandOutcome::Partial(
                PropCallableRoleUnresolvedReason::WorkLimitExceeded
            ),
            "a nested real-carrier query must preserve its work-limit reason"
        );

        let limited_host = crate::VerterHost::new_standalone(crate::types::HostConfig {
            projection_op_budget: 1,
            ..crate::types::HostConfig::default()
        });
        upsert_ts(
            &limited_host,
            "/dep.ts",
            "export namespace Surface { export type Target = { value: string } }\n",
        );
        upsert_ts(&limited_host, "/limited.ts", "export type Seed = string;\n");
        let limited_dispatch = ProjectSemanticDispatch::new(&limited_host);
        let limited_scope = file_scope(&limited_dispatch, "/limited.ts");
        let import_carrier = import_type_carrier(
            &limited_dispatch,
            "./dep",
            &["Surface", "Target"],
            &[],
            false,
            limited_scope,
        );
        let request = RequestContext::with_kind_timing_and_projection_budget(
            1,
            Arc::from("/limited.ts"),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            1,
        );
        let _request_guard = RequestContextGuard::install(request);
        assert_eq!(
            limited_dispatch.demand_symbol_identity(import_carrier, &[expected()]),
            SymbolIdentityDemandOutcome::Partial(
                PropCallableRoleUnresolvedReason::WorkLimitExceeded
            ),
            "a projection-work-truncated import carrier must preserve WorkLimitExceeded"
        );
    }
}
