//! Exact symbol-identity demand over the shared semantic dispatcher.
//!
//! This adapter owns no name or import resolver. Every declaration and
//! instantiation hop delegates to the canonical `ResolveDecl` / `Instantiate`
//! queries, preserving their dependency reads, fixed view, connected work
//! envelope, and cache-suppression rails.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_type_expr::{PropCallableRoleUnresolvedReason, ResolvedSymbolIdentity};

use super::ProjectSemanticDispatch;
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

enum IdentitySubject {
    Node(SemanticNodeId),
    Symbol(ResolvedSymbolIdentity),
}

impl ProjectSemanticDispatch<'_> {
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
                        | SemanticNodeData::TypeOf(_) => {
                            if !visited_nodes.insert(node) {
                                return self.partial_symbol_identity(
                                    PropCallableRoleUnresolvedReason::Cycle,
                                );
                            }
                            if let Err(reasons) = self.charge_connected_work() {
                                return self.partial_symbol_identity(partial_reason(reasons));
                            }
                            drop(data);
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

fn query_error_reason(error: &QueryError) -> PropCallableRoleUnresolvedReason {
    match error {
        QueryError::Miss | QueryError::DeclPlaceholder { .. } | QueryError::RaiseMiss => {
            PropCallableRoleUnresolvedReason::MissingDependency
        }
        QueryError::BudgetExceeded(_) => PropCallableRoleUnresolvedReason::BudgetExceeded,
        QueryError::AliasCycle { .. }
        | QueryError::RecursiveRef { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::TypeParamCycle => PropCallableRoleUnresolvedReason::Cycle,
        QueryError::UnsupportedIntrinsic { .. }
        | QueryError::UnrepresentableSurface
        | QueryError::UnrepresentableSurfaceMember => PropCallableRoleUnresolvedReason::Unsupported,
        QueryError::Cancelled
        | QueryError::UnstableState { .. }
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

    #[test]
    fn unsupported_and_work_limited_identity_demands_fail_closed() {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let dispatch = ProjectSemanticDispatch::new(&host);
        let raw = dispatch.graph().intern_node(SemanticNodeData::RawFallback {
            raw: Arc::from("unsupported"),
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
