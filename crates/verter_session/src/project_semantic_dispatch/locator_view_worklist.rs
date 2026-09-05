//! Stack-safe post-substitution view projection.
//!
//! Structural descent is an explicit post-order worklist. Operator and
//! reference dispatches remain synchronous leaves: a nested query may build its
//! own worklist, but authored structural depth never consumes one host frame per
//! node.

use std::sync::Arc;

use smallvec::SmallVec;

mod work_credit;

use work_credit::ConnectedWorkCredit;

#[cfg(test)]
std::thread_local! {
    static MAPPED_AFTER_SOURCE_VISITS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn mapped_after_source_visits_for_tests() -> usize {
    MAPPED_AFTER_SOURCE_VISITS.get()
}

#[cfg(test)]
fn record_mapped_after_source_visit_for_tests() {
    MAPPED_AFTER_SOURCE_VISITS.set(MAPPED_AFTER_SOURCE_VISITS.get() + 1);
}

use super::carrier::{CarrierArgsContinuation, CarrierResolutionPlan, CarrierResolverContext};
use super::locator_view::{LocatorViewInputs, ViewMemo};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    may_reduce_operator, FunctionParam, IndexKey, IndexSignature, MapperKey, MemberMergeRole,
    NodeScopeId, PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryError,
    QueryResult, ReductionDemand, ResolveDeclKey, ResultCompleteness, ScopeId, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SurfaceMember,
    SurfaceView, TupleElement, TypeParamDecl, ValueRootKey,
};

#[must_use]
pub(super) struct ProjectedViewOutcome {
    pub(super) node: SemanticNodeId,
    pub(super) completeness: ResultCompleteness,
}

impl ProjectedViewOutcome {
    fn complete(node: SemanticNodeId) -> Self {
        Self {
            node,
            completeness: ResultCompleteness::Complete,
        }
    }

    fn partial(node: SemanticNodeId, reasons: crate::semantic_query::PartialReasonSet) -> Self {
        Self {
            node,
            completeness: ResultCompleteness::partial(reasons),
        }
    }
}

enum ProjectionFrame {
    Enter {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    },
    CompositeResume {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: Arc<SemanticNodeData>,
        next_child: usize,
    },
    ConditionalAfterCheck {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: Arc<SemanticNodeData>,
        check: SemanticNodeId,
    },
    ConditionalAfterExtends {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: Arc<SemanticNodeData>,
        extends_context: ProjectionReductionContext,
    },
    ConditionalAfterTrue {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: Arc<SemanticNodeData>,
        extends_context: ProjectionReductionContext,
    },
    ConditionalFinish {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: Arc<SemanticNodeData>,
        extends_context: ProjectionReductionContext,
    },
    MappedAfterSource {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: Arc<SemanticNodeData>,
        source: SemanticNodeId,
    },
    MappedAfterValue {
        state: Box<MappedContinuationState>,
    },
    MappedFinish {
        state: Box<MappedContinuationState>,
    },
    ReferenceArgsResume {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        state: Box<ReferenceArgsState>,
    },
}

const _: () = assert!(std::mem::size_of::<ProjectionFrame>() <= 32);

struct MappedContinuationState {
    node: SemanticNodeId,
    context: ProjectionReductionContext,
    data: Arc<SemanticNodeData>,
    source_id: SemanticNodeId,
    key_space: SemanticNodeId,
}

struct ReferenceArgsState {
    continuation: CarrierArgsContinuation,
    args: Arc<[SemanticNodeId]>,
    argument_context: ProjectionReductionContext,
    next_arg: usize,
}

enum ReferenceProjectionPlan {
    Ready(SemanticNodeId),
    NeedsArgs {
        continuation: CarrierArgsContinuation,
        args: Arc<[SemanticNodeId]>,
        argument_context: ProjectionReductionContext,
    },
}

/// Borrowed child topology selected once per composite parent. The hot
/// breadth cases avoid re-running the exhaustive `SemanticNodeData` match for
/// every child; uncommon shapes retain the complete generic scheduler.
enum ProjectionChildPlan<'a> {
    Uniform {
        children: &'a [SemanticNodeId],
        context: ProjectionReductionContext,
    },
    Object {
        view: &'a SurfaceView,
        context: ProjectionReductionContext,
    },
    Function {
        params: &'a [FunctionParam],
        return_type: SemanticNodeId,
        type_parameters: &'a [TypeParamDecl],
        context: ProjectionReductionContext,
    },
    General {
        data: &'a SemanticNodeData,
        context: ProjectionReductionContext,
    },
}

impl<'a> ProjectSemanticDispatch<'a> {
    #[inline(always)]
    fn active_decl_recursion_sentinel(&self, data: &SemanticNodeData) -> Option<SemanticNodeId> {
        let SemanticNodeData::DeclRef { identity } = data else {
            return None;
        };
        self.is_instantiate_active(
            identity.canonical_id.as_ref(),
            identity.owner,
            identity.decl_name.as_ref(),
        )
        .then(|| self.recursive_ref_sentinel(identity))
    }

    fn plan_reference_projection(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: &SemanticNodeData,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
    ) -> ReferenceProjectionPlan {
        match data {
            // The terminal nominal carrier IS the type it denotes: resolving
            // its head would project the annotation down to the shared
            // `symbol` primitive and erase the declaring identity. The plan
            // is already complete.
            SemanticNodeData::TypeOfNominal(_) => ReferenceProjectionPlan::Ready(node),
            SemanticNodeData::TypeOf(_) => {
                let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                let value_root = value_root.clone();
                let path = Arc::clone(path);
                let type_args: Arc<[SemanticNodeId]> =
                    Arc::from(data.carrier_type_args().to_vec().into_boxed_slice());
                let result = match self.execute_type_node(self.typeof_key_with_path(
                    value_root.clone(),
                    Arc::clone(&path),
                    context,
                )) {
                    QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                    _ if !path.is_empty() => {
                        let joined: Arc<str> =
                            Arc::from(format!("{}.{}", value_root.name, path[0]));
                        let rest: Arc<[Arc<str>]> =
                            Arc::from(path[1..].to_vec().into_boxed_slice());
                        match self.execute_type_node(self.typeof_key_with_path(
                            ValueRootKey {
                                scope: value_root.scope.clone(),
                                name: joined,
                            },
                            rest,
                            context,
                        )) {
                            QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                            _ => {
                                return ReferenceProjectionPlan::Ready(
                                    self.opaque(QueryError::Miss),
                                );
                            }
                        }
                    }
                    _ => {
                        return ReferenceProjectionPlan::Ready(self.opaque(QueryError::Miss));
                    }
                };
                if type_args.is_empty() {
                    ReferenceProjectionPlan::Ready(result)
                } else {
                    ReferenceProjectionPlan::NeedsArgs {
                        continuation: CarrierArgsContinuation::ApplyTypeof { base: result },
                        args: type_args,
                        argument_context: context,
                    }
                }
            }
            SemanticNodeData::DeclRef { identity } => {
                if self.is_instantiate_active(
                    identity.canonical_id.as_ref(),
                    identity.owner,
                    identity.decl_name.as_ref(),
                ) {
                    return ReferenceProjectionPlan::Ready(self.recursive_ref_sentinel(identity));
                }
                if matches!(
                    context.mode,
                    ProjectionMode::Navigate | ProjectionMode::Skeleton | ProjectionMode::Shallow
                ) {
                    return ReferenceProjectionPlan::Ready(node);
                }
                let anchor =
                    match self.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: ScopeId {
                            canonical_id: Arc::clone(&identity.canonical_id),
                            owner: identity.owner,
                            local_scope: None,
                            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                                identity.owner,
                            ),
                        },
                        name: Arc::clone(&identity.decl_name),
                    })) {
                        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                        _ => {
                            return ReferenceProjectionPlan::Ready(self.opaque(QueryError::Miss));
                        }
                    };
                let routes_through_instantiate = self
                    .ctx
                    .prepared_type_decl_return_only(
                        identity.canonical_id.as_ref(),
                        identity.owner,
                        identity.decl_name.as_ref(),
                    )
                    .is_some_and(|prepared| !prepared.type_parameters.is_empty());
                if !routes_through_instantiate {
                    return ReferenceProjectionPlan::Ready(anchor);
                }
                let result = match self.execute_type_node(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        self.type_slot_for(
                            Arc::clone(&identity.canonical_id),
                            identity.owner,
                            Arc::clone(&identity.decl_name),
                        ),
                        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        self.instantiate_context_for(&identity.canonical_id, context),
                    ),
                )) {
                    QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                    _ => self.opaque(QueryError::Miss),
                };
                ReferenceProjectionPlan::Ready(result)
            }
            SemanticNodeData::BareRef(_) => {
                self.plan_bare_reference_projection(data, context, inputs, substitutions)
            }
            SemanticNodeData::ImportType(_) => {
                self.plan_import_reference_projection(data, context, inputs)
            }
            _ => unreachable!("non-reference node reached reference projection planner"),
        }
    }

    pub(super) fn project_view_node_worklist(
        &self,
        root: SemanticNodeId,
        root_context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &mut ViewMemo,
    ) -> ProjectedViewOutcome {
        // A completed projection is free reusable work. Preserve the original
        // recursive primitive's memo-first contract: warm hits neither install
        // a connected-demand state nor consume its runaway-work budget.
        if let Some(&done) = memo.get(&(root, root_context)) {
            return ProjectedViewOutcome::complete(done);
        }
        let (_connected_guard, preexisting_trip) = self.enter_connected_demand(false);
        let root_data = self.graph().node_data(root);
        // The established active-identity recursion sentinel is semantic cycle
        // handling, not resource exhaustion. Preserve its precedence even
        // after the connected work envelope has already tripped.
        if let Some(recursive) = root_data
            .as_deref()
            .and_then(|data| self.active_decl_recursion_sentinel(data))
        {
            memo.insert((root, root_context), recursive);
            return ProjectedViewOutcome::complete(recursive);
        }
        if let Some(reasons) = preexisting_trip {
            return ProjectedViewOutcome::partial(root, reasons);
        }
        crate::loop5_instrumentation::watchdog_beat();
        crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
        let Some(root_data) = root_data else {
            if let Err(reasons) = self.charge_connected_work() {
                return ProjectedViewOutcome::partial(root, reasons);
            }
            memo.insert((root, root_context), root);
            return ProjectedViewOutcome::complete(root);
        };
        if let Err(reasons) = self.charge_connected_work() {
            return ProjectedViewOutcome::partial(root, reasons);
        }
        match root_data.as_ref() {
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::SyntheticBinding { .. } => {
                memo.insert((root, root_context), root);
                return ProjectedViewOutcome::complete(root);
            }
            SemanticNodeData::RawFallback { .. } => {
                let miss = self.opaque(QueryError::Miss);
                memo.insert((root, root_context), miss);
                return ProjectedViewOutcome::complete(miss);
            }
            _ => {}
        }

        let mut frames: SmallVec<[ProjectionFrame; 16]> = SmallVec::new();
        let mut work_credit = match ConnectedWorkCredit::new(self) {
            Ok(credit) => credit,
            Err(reasons) => return ProjectedViewOutcome::partial(root, reasons),
        };
        if let Err(reasons) = self.schedule_projection_node(
            root,
            root_context,
            root_data,
            inputs,
            substitutions,
            memo,
            &mut frames,
            &mut work_credit,
        ) {
            return ProjectedViewOutcome::partial(root, reasons);
        }

        let mut trip = None;
        while let Some(frame) = frames.pop() {
            match frame {
                ProjectionFrame::Enter { node, context } => {
                    let data =
                        match self.prepare_projection_node(node, context, memo, &mut work_credit) {
                            Ok(Some(data)) => data,
                            Ok(None) => continue,
                            Err(reasons) => {
                                trip = Some(reasons);
                                break;
                            }
                        };
                    if let Err(reasons) = self.schedule_projection_node(
                        node,
                        context,
                        data,
                        inputs,
                        substitutions,
                        memo,
                        &mut frames,
                        &mut work_credit,
                    ) {
                        trip = Some(reasons);
                        break;
                    }
                }
                ProjectionFrame::CompositeResume {
                    node,
                    context,
                    data,
                    next_child,
                } => {
                    let mut cursor = next_child;
                    let child_plan = self.projection_child_plan(data.as_ref(), context);
                    if let ProjectionChildPlan::Uniform {
                        children,
                        context: child_context,
                    } = &child_plan
                    {
                        if let Err(reasons) = self.resume_uniform_projection(
                            node,
                            context,
                            &data,
                            children,
                            *child_context,
                            cursor,
                            inputs,
                            substitutions,
                            memo,
                            &mut frames,
                            &mut work_credit,
                        ) {
                            trip = Some(reasons);
                            break;
                        }
                        continue;
                    }
                    loop {
                        let Some((child, child_context)) =
                            self.projection_child_from_plan(&child_plan, cursor)
                        else {
                            let synchronize =
                                projection_finish_may_dispatch(data.as_ref(), context);
                            if synchronize {
                                work_credit.settle();
                            }
                            let result = self.finish_projection_node(
                                node,
                                context,
                                data.as_ref(),
                                inputs,
                                substitutions,
                                memo,
                            );
                            if synchronize {
                                if let Err(reasons) = work_credit.refresh() {
                                    trip = Some(reasons);
                                    break;
                                }
                            }
                            self.memoize_projected(memo, node, context, result);
                            break;
                        };
                        let child_data = match self.prepare_projection_node(
                            child,
                            child_context,
                            memo,
                            &mut work_credit,
                        ) {
                            Ok(Some(data)) => data,
                            Ok(None) => {
                                cursor += 1;
                                continue;
                            }
                            Err(reasons) => {
                                trip = Some(reasons);
                                break;
                            }
                        };

                        // Install the parent continuation below any frames the
                        // child schedules. A head-resolved reference can still
                        // complete synchronously; remove the unused parent and
                        // continue the same cursor without a push/pop cycle per
                        // terminal child.
                        frames.push(ProjectionFrame::CompositeResume {
                            node,
                            context,
                            data: Arc::clone(&data),
                            next_child: cursor + 1,
                        });
                        match self.schedule_projection_node(
                            child,
                            child_context,
                            child_data,
                            inputs,
                            substitutions,
                            memo,
                            &mut frames,
                            &mut work_credit,
                        ) {
                            Ok(true) => break,
                            Ok(false) => {}
                            Err(reasons) => {
                                trip = Some(reasons);
                                break;
                            }
                        }
                        let resumed = frames.pop();
                        verter_debug_assert!(matches!(
                            resumed,
                            Some(ProjectionFrame::CompositeResume { .. })
                        ));
                        cursor += 1;
                    }
                    if trip.is_some() {
                        break;
                    }
                }
                ProjectionFrame::ConditionalAfterCheck {
                    node,
                    context,
                    data,
                    check,
                } => {
                    let check_id = projected(memo, check, context);
                    let check_is_object_relation_subject = matches!(
                        self.graph().node_data(check_id).as_deref(),
                        Some(
                            SemanticNodeData::Object(_)
                                | SemanticNodeData::Intersection(_)
                                | SemanticNodeData::Alias(_)
                                | SemanticNodeData::DeclRef { .. }
                                | SemanticNodeData::InstantiationRef { .. }
                                | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. })
                        )
                    );
                    let extends_context = if check_is_object_relation_subject {
                        context
                    } else {
                        ProjectionReductionContext::structural_transit_with_mode(context.mode)
                            .with_orthogonal_axes_from(context)
                    };
                    let SemanticNodeData::Conditional { extends, .. } = data.as_ref() else {
                        unreachable!("conditional staging frame must carry a conditional")
                    };
                    let extends = *extends;
                    frames.push(ProjectionFrame::ConditionalAfterExtends {
                        node,
                        context,
                        data,
                        extends_context,
                    });
                    frames.push(ProjectionFrame::Enter {
                        node: extends,
                        context: extends_context,
                    });
                }
                ProjectionFrame::ConditionalAfterExtends {
                    node,
                    context,
                    data,
                    extends_context,
                } => {
                    let SemanticNodeData::Conditional {
                        true_branch_ref, ..
                    } = data.as_ref()
                    else {
                        unreachable!("conditional staging frame must carry a conditional")
                    };
                    let true_branch = *true_branch_ref;
                    frames.push(ProjectionFrame::ConditionalAfterTrue {
                        node,
                        context,
                        data,
                        extends_context,
                    });
                    frames.push(ProjectionFrame::Enter {
                        node: true_branch,
                        context,
                    });
                }
                ProjectionFrame::ConditionalAfterTrue {
                    node,
                    context,
                    data,
                    extends_context,
                } => {
                    let SemanticNodeData::Conditional {
                        false_branch_ref, ..
                    } = data.as_ref()
                    else {
                        unreachable!("conditional staging frame must carry a conditional")
                    };
                    let false_branch = *false_branch_ref;
                    frames.push(ProjectionFrame::ConditionalFinish {
                        node,
                        context,
                        data,
                        extends_context,
                    });
                    frames.push(ProjectionFrame::Enter {
                        node: false_branch,
                        context,
                    });
                }
                ProjectionFrame::ConditionalFinish {
                    node,
                    context,
                    data,
                    extends_context,
                } => {
                    let SemanticNodeData::Conditional {
                        check,
                        extends,
                        true_branch_ref,
                        false_branch_ref,
                        distributive,
                    } = data.as_ref()
                    else {
                        unreachable!("conditional finish frame must carry a conditional")
                    };
                    work_credit.settle();
                    let result = match self.execute_type_node(SemanticQueryKey::Conditional {
                        check: projected(memo, *check, context),
                        extends: projected(memo, *extends, extends_context),
                        true_branch: projected(memo, *true_branch_ref, context),
                        false_branch: projected(memo, *false_branch_ref, context),
                        distributive: *distributive,
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                        _ => self.opaque(QueryError::Miss),
                    };
                    if let Err(reasons) = work_credit.refresh() {
                        trip = Some(reasons);
                        break;
                    }
                    self.memoize_projected(memo, node, context, result);
                }
                ProjectionFrame::MappedAfterSource {
                    node,
                    context,
                    data,
                    source,
                } => {
                    #[cfg(test)]
                    record_mapped_after_source_visit_for_tests();
                    let SemanticNodeData::Mapped { mapper, .. } = data.as_ref() else {
                        unreachable!("mapped staging frame must carry a mapped node")
                    };
                    let source_id = projected(memo, source, context);
                    let keyof_sourced = matches!(
                        self.graph().node_data(mapper.key_space).as_deref(),
                        Some(SemanticNodeData::KeyOf { base }) if *base == source
                    );
                    let key_space = if keyof_sourced {
                        // The exact `keyof infer T` descriptor is open only
                        // until the enclosing conditional relation fixes T.
                        // Reducing it during locator-view projection can only
                        // turn the authored `KeyOf` carrier into `Miss`,
                        // making the reverse-homomorphic pattern
                        // unrecognizable. Preserve that exact selected Infer
                        // operand in every projection mode; concrete sources
                        // keep the established eager `KeyOf` path.
                        let selected_base_is_infer = matches!(
                            self.graph().node_data(source_id).as_deref(),
                            Some(SemanticNodeData::Infer { .. })
                        );
                        if may_reduce_operator(context) && !selected_base_is_infer {
                            work_credit.settle();
                            let result = match self.execute_type_node(SemanticQueryKey::KeyOf {
                                base: source_id,
                                context,
                            }) {
                                QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                                _ => self.opaque(QueryError::Miss),
                            };
                            if let Err(reasons) = work_credit.refresh() {
                                trip = Some(reasons);
                                break;
                            }
                            result
                        } else {
                            match self.graph().node_data(source_id).as_deref() {
                                Some(SemanticNodeData::Opaque(_)) | None => {
                                    self.opaque(QueryError::Miss)
                                }
                                _ => self.graph().intern_preserving_scope(
                                    mapper.key_space,
                                    SemanticNodeData::KeyOf { base: source_id },
                                ),
                            }
                        }
                    } else {
                        source_id
                    };
                    let value_expr = mapper.value_expr;
                    frames.push(ProjectionFrame::MappedAfterValue {
                        state: Box::new(MappedContinuationState {
                            node,
                            context,
                            data,
                            source_id,
                            key_space,
                        }),
                    });
                    frames.push(ProjectionFrame::Enter {
                        node: value_expr,
                        context,
                    });
                }
                ProjectionFrame::MappedAfterValue { state } => {
                    let SemanticNodeData::Mapped { mapper, .. } = state.data.as_ref() else {
                        unreachable!("mapped staging frame must carry a mapped node")
                    };
                    if let Some(name_remap) = mapper.name_remap {
                        let context = state.context;
                        frames.push(ProjectionFrame::MappedFinish { state });
                        frames.push(ProjectionFrame::Enter {
                            node: name_remap,
                            context,
                        });
                    } else {
                        work_credit.settle();
                        let result = self.finish_mapped_projection(
                            state.node,
                            state.context,
                            state.data.as_ref(),
                            state.source_id,
                            state.key_space,
                            memo,
                        );
                        if let Err(reasons) = work_credit.refresh() {
                            trip = Some(reasons);
                            break;
                        }
                        self.memoize_projected(memo, state.node, state.context, result);
                    }
                }
                ProjectionFrame::MappedFinish { state } => {
                    work_credit.settle();
                    let result = self.finish_mapped_projection(
                        state.node,
                        state.context,
                        state.data.as_ref(),
                        state.source_id,
                        state.key_space,
                        memo,
                    );
                    if let Err(reasons) = work_credit.refresh() {
                        trip = Some(reasons);
                        break;
                    }
                    self.memoize_projected(memo, state.node, state.context, result);
                }
                ProjectionFrame::ReferenceArgsResume {
                    node,
                    context,
                    mut state,
                } => {
                    if state.next_arg < state.args.len() {
                        let argument = state.args[state.next_arg];
                        let argument_context = state.argument_context;
                        state.next_arg += 1;
                        frames.push(ProjectionFrame::ReferenceArgsResume {
                            node,
                            context,
                            state,
                        });
                        frames.push(ProjectionFrame::Enter {
                            node: argument,
                            context: argument_context,
                        });
                    } else {
                        let projected_args: Arc<[SemanticNodeId]> = Arc::from(
                            state
                                .args
                                .iter()
                                .map(|argument| projected(memo, *argument, state.argument_context))
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        );
                        work_credit.settle();
                        let result =
                            self.finish_carrier_resolution(state.continuation, projected_args);
                        if let Err(reasons) = work_credit.refresh() {
                            trip = Some(reasons);
                            break;
                        }
                        self.memoize_projected(memo, node, context, result);
                    }
                }
            }
            if let Some(reasons) = self.connected_demand_trip() {
                trip = Some(reasons);
                break;
            }
        }

        work_credit.settle();
        if let Some(reasons) = trip.or_else(|| self.connected_demand_trip()) {
            ProjectedViewOutcome::partial(root, reasons)
        } else {
            ProjectedViewOutcome::complete(projected(memo, root, root_context))
        }
    }

    fn memoize_projected(
        &self,
        memo: &mut ViewMemo,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        result: SemanticNodeId,
    ) {
        if self.connected_demand_trip().is_none() {
            memo.insert((node, context), result);
        }
    }

    /// Perform the common memo/cycle/budget/terminal prelude for one node.
    /// `Ok(None)` means the node completed synchronously; `Ok(Some(data))`
    /// hands a non-terminal to the explicit staging worklist.
    #[inline(always)]
    fn prepare_projection_node(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        memo: &mut ViewMemo,
        work_credit: &mut ConnectedWorkCredit<'_, '_>,
    ) -> Result<Option<Arc<SemanticNodeData>>, crate::semantic_query::PartialReasonSet> {
        if memo.contains_key(&(node, context)) {
            return Ok(None);
        }
        let data = self.graph().node_data(node);
        let Some(data) = data else {
            work_credit.consume()?;
            crate::loop5_instrumentation::watchdog_beat();
            crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
            memo.insert((node, context), node);
            return Ok(None);
        };
        match data.as_ref() {
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::SyntheticBinding { .. } => {
                work_credit.consume()?;
                crate::loop5_instrumentation::watchdog_beat();
                crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
                memo.insert((node, context), node);
                Ok(None)
            }
            SemanticNodeData::RawFallback { .. } => {
                work_credit.consume()?;
                crate::loop5_instrumentation::watchdog_beat();
                crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
                memo.insert((node, context), self.opaque(QueryError::Miss));
                Ok(None)
            }
            SemanticNodeData::DeclRef { .. } => {
                if let Some(recursive) = self.active_decl_recursion_sentinel(data.as_ref()) {
                    memo.insert((node, context), recursive);
                    return Ok(None);
                }
                work_credit.consume()?;
                crate::loop5_instrumentation::watchdog_beat();
                crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
                Ok(Some(data))
            }
            _ => {
                work_credit.consume()?;
                crate::loop5_instrumentation::watchdog_beat();
                crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
                Ok(Some(data))
            }
        }
    }

    /// Resume a composite whose children are a direct semantic-node slice.
    /// This is the breadth hot path: it preserves the generic scheduler's
    /// exact order and mixed-child behavior without constructing a
    /// `Result<Option<Arc<_>>>` state for every terminal leaf.
    #[allow(clippy::too_many_arguments)]
    fn resume_uniform_projection(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: &Arc<SemanticNodeData>,
        children: &[SemanticNodeId],
        child_context: ProjectionReductionContext,
        next_child: usize,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &mut ViewMemo,
        frames: &mut SmallVec<[ProjectionFrame; 16]>,
        work_credit: &mut ConnectedWorkCredit<'_, '_>,
    ) -> Result<(), crate::semantic_query::PartialReasonSet> {
        let remaining_children = children.get(next_child..).unwrap_or_default();
        for (offset, &child) in remaining_children.iter().enumerate() {
            let cursor = next_child + offset;
            let std::collections::hash_map::Entry::Vacant(vacant) =
                memo.entry((child, child_context))
            else {
                continue;
            };
            let child_data = self.graph().node_data(child);
            let Some(child_data) = child_data else {
                work_credit.consume()?;
                crate::loop5_instrumentation::watchdog_beat();
                crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
                vacant.insert(child);
                continue;
            };
            match child_data.as_ref() {
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Literal(_)
                | SemanticNodeData::Opaque(_)
                | SemanticNodeData::Infer { .. }
                | SemanticNodeData::InferRef { .. }
                | SemanticNodeData::SyntheticBinding { .. } => {
                    work_credit.consume()?;
                    crate::loop5_instrumentation::watchdog_beat();
                    crate::loop5_instrumentation::watchdog_check_and_dump(
                        "project_view_node_worklist",
                    );
                    vacant.insert(child);
                    continue;
                }
                SemanticNodeData::RawFallback { .. } => {
                    work_credit.consume()?;
                    crate::loop5_instrumentation::watchdog_beat();
                    crate::loop5_instrumentation::watchdog_check_and_dump(
                        "project_view_node_worklist",
                    );
                    vacant.insert(self.opaque(QueryError::Miss));
                    continue;
                }
                SemanticNodeData::DeclRef { .. } => {
                    if let Some(recursive) =
                        self.active_decl_recursion_sentinel(child_data.as_ref())
                    {
                        vacant.insert(recursive);
                        continue;
                    }
                }
                _ => {}
            }

            // The vacant-entry handle is no longer used beyond this point, so
            // its mutable memo borrow ends before non-terminal staging.
            work_credit.consume()?;
            crate::loop5_instrumentation::watchdog_beat();
            crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
            frames.push(ProjectionFrame::CompositeResume {
                node,
                context,
                data: Arc::clone(data),
                next_child: cursor + 1,
            });
            if self.schedule_projection_node(
                child,
                child_context,
                child_data,
                inputs,
                substitutions,
                memo,
                frames,
                work_credit,
            )? {
                return Ok(());
            }
            let resumed = frames.pop();
            verter_debug_assert!(matches!(
                resumed,
                Some(ProjectionFrame::CompositeResume { .. })
            ));
        }

        let synchronize = projection_finish_may_dispatch(data.as_ref(), context);
        if synchronize {
            work_credit.settle();
        }
        let result =
            self.finish_projection_node(node, context, data.as_ref(), inputs, substitutions, memo);
        if synchronize {
            work_credit.refresh()?;
        }
        self.memoize_projected(memo, node, context, result);
        Ok(())
    }

    fn finish_mapped_projection(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: &SemanticNodeData,
        source_id: SemanticNodeId,
        key_space: SemanticNodeId,
        memo: &ViewMemo,
    ) -> SemanticNodeId {
        let SemanticNodeData::Mapped { mapper, .. } = data else {
            unreachable!("mapped finish frame must carry a mapped node")
        };
        let value_expr = projected(memo, mapper.value_expr, context);
        let name_remap = mapper.name_remap.map(|name| projected(memo, name, context));
        let projected_mapper = MapperKey {
            parameter_node: mapper.parameter_node,
            key_space,
            value_expr,
            optionality: mapper.optionality,
            readonly: mapper.readonly,
            name_remap,
            kind: crate::semantic_query::MapperKind::classify_value_expr(
                self.graph(),
                value_expr,
                source_id,
                mapper.parameter_node,
            ),
        };
        if super::raise::mapped_type_is_open_or_unknown(self, source_id, &projected_mapper) {
            self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::Mapped {
                    source: source_id,
                    mapper: projected_mapper,
                },
            )
        } else {
            match self.execute_type_node(SemanticQueryKey::MappedType {
                source: source_id,
                mapper: projected_mapper,
                context,
            }) {
                QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                _ => self.opaque(QueryError::Miss),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn schedule_projection_node(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: Arc<SemanticNodeData>,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &mut ViewMemo,
        frames: &mut SmallVec<[ProjectionFrame; 16]>,
        work_credit: &mut ConnectedWorkCredit<'_, '_>,
    ) -> Result<bool, crate::semantic_query::PartialReasonSet> {
        match data.as_ref() {
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::SyntheticBinding { .. } => {
                memo.insert((node, context), node);
                Ok(false)
            }
            SemanticNodeData::RawFallback { .. } => {
                memo.insert((node, context), self.opaque(QueryError::Miss));
                Ok(false)
            }
            SemanticNodeData::Conditional { check, .. } => {
                let check = *check;
                frames.push(ProjectionFrame::ConditionalAfterCheck {
                    node,
                    context,
                    data,
                    check,
                });
                frames.push(ProjectionFrame::Enter {
                    node: check,
                    context,
                });
                Ok(true)
            }
            SemanticNodeData::Mapped { source, .. } => {
                let source = *source;
                frames.push(ProjectionFrame::MappedAfterSource {
                    node,
                    context,
                    data,
                    source,
                });
                frames.push(ProjectionFrame::Enter {
                    node: source,
                    context,
                });
                Ok(true)
            }
            SemanticNodeData::TypeOf(_)
            | SemanticNodeData::TypeOfNominal(_)
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_) => {
                work_credit.settle();
                let plan = self.plan_reference_projection(
                    node,
                    context,
                    data.as_ref(),
                    inputs,
                    substitutions,
                );
                work_credit.refresh()?;
                match plan {
                    ReferenceProjectionPlan::Ready(result) => {
                        self.memoize_projected(memo, node, context, result);
                        Ok(false)
                    }
                    ReferenceProjectionPlan::NeedsArgs {
                        continuation,
                        args,
                        argument_context,
                    } => {
                        frames.push(ProjectionFrame::ReferenceArgsResume {
                            node,
                            context,
                            state: Box::new(ReferenceArgsState {
                                continuation,
                                args,
                                argument_context,
                                next_arg: 0,
                            }),
                        });
                        Ok(true)
                    }
                }
            }
            _ => {
                frames.push(ProjectionFrame::CompositeResume {
                    node,
                    context,
                    data,
                    next_child: 0,
                });
                Ok(true)
            }
        }
    }

    fn projection_child_at(
        &self,
        data: &SemanticNodeData,
        context: ProjectionReductionContext,
        index: usize,
    ) -> Option<(SemanticNodeId, ProjectionReductionContext)> {
        match data {
            SemanticNodeData::Alias(target) => (index == 0).then_some((*target, context)),
            SemanticNodeData::TypeParam {
                constraint,
                default,
                ..
            } => {
                let mut remaining = index;
                if let Some(constraint) = constraint {
                    if remaining == 0 {
                        return Some((*constraint, context));
                    }
                    remaining -= 1;
                }
                default
                    .filter(|_| remaining == 0)
                    .map(|default| (default, context))
            }
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let arms = composite.composite_members().expect("composite arm");
                arms.get(index).map(|child| (*child, context))
            }
            SemanticNodeData::MergedDecl { contributors: arms } => {
                arms.get(index).map(|child| (*child, context))
            }
            SemanticNodeData::Array { element, .. } => (index == 0).then_some((*element, context)),
            SemanticNodeData::Tuple { elements, .. } => {
                elements.get(index).map(|element| (element.value, context))
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => expressions
                .get(index)
                .map(|expression| (*expression, context)),
            SemanticNodeData::Object(view) => object_projection_child(view, context, index),
            SemanticNodeData::ObjectSpreadProgram(program) => program
                .child_nodes()
                .nth(index)
                .map(|child| (child, context)),
            SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                ..
            } => {
                let mut remaining = index;
                if let Some(parameter) = params.get(remaining) {
                    return Some((parameter.ty, context));
                }
                remaining = remaining.saturating_sub(params.len());
                if remaining == 0 {
                    return Some((*return_type, context));
                }
                remaining -= 1;
                for parameter in type_parameters.iter() {
                    if remaining == 0 {
                        return Some((parameter.param, context));
                    }
                    remaining -= 1;
                    if let Some(constraint) = parameter.constraint {
                        if remaining == 0 {
                            return Some((constraint, context));
                        }
                        remaining -= 1;
                    }
                    if let Some(default) = parameter.default {
                        if remaining == 0 {
                            return Some((default, context));
                        }
                        remaining -= 1;
                    }
                }
                None
            }
            SemanticNodeData::KeyOf { base } => (index == 0).then_some((*base, context)),
            SemanticNodeData::IndexedAccess {
                object,
                index: index_key,
            } => {
                let object_context = if matches!(
                    self.graph().node_data(*object).as_deref(),
                    Some(SemanticNodeData::IndexedAccess { .. })
                ) {
                    context.with_mode(ProjectionMode::Navigate)
                } else {
                    context
                };
                match (index, index_key) {
                    (0, _) => Some((*object, object_context)),
                    (1, IndexKey::Computed(index)) => Some((*index, context)),
                    _ => None,
                }
            }
            SemanticNodeData::InstantiationRef { args, .. } => {
                let argument_context = context.into_structural_provenance();
                args.get(index)
                    .map(|argument| (*argument, argument_context))
            }
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::RawFallback { .. }
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::SyntheticBinding { .. }
            | SemanticNodeData::DeferredCallable(_)
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::Mapped { .. }
            | SemanticNodeData::TypeOf(_)
            // The nominal terminal is a resolved scalar leaf.
            | SemanticNodeData::TypeOfNominal(_)
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_) => {
                unreachable!("leaf or staged node reached composite child scheduler")
            }
        }
    }

    fn projection_child_plan<'data>(
        &self,
        data: &'data SemanticNodeData,
        context: ProjectionReductionContext,
    ) -> ProjectionChildPlan<'data> {
        match data {
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let children = composite.composite_members().expect("composite arm");
                ProjectionChildPlan::Uniform { children, context }
            }
            SemanticNodeData::MergedDecl {
                contributors: children,
            }
            | SemanticNodeData::TemplateLiteral {
                expressions: children,
                ..
            } => ProjectionChildPlan::Uniform { children, context },
            SemanticNodeData::InstantiationRef { args, .. } => ProjectionChildPlan::Uniform {
                children: args,
                context: context.into_structural_provenance(),
            },
            SemanticNodeData::Object(view) => ProjectionChildPlan::Object { view, context },
            SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                ..
            } => ProjectionChildPlan::Function {
                params,
                return_type: *return_type,
                type_parameters,
                context,
            },
            _ => ProjectionChildPlan::General { data, context },
        }
    }

    #[inline(always)]
    fn projection_child_from_plan(
        &self,
        plan: &ProjectionChildPlan<'_>,
        index: usize,
    ) -> Option<(SemanticNodeId, ProjectionReductionContext)> {
        match plan {
            ProjectionChildPlan::Uniform { children, context } => {
                children.get(index).map(|child| (*child, *context))
            }
            ProjectionChildPlan::Object { view, context } => {
                object_projection_child(view, *context, index)
            }
            ProjectionChildPlan::Function {
                params,
                return_type,
                type_parameters,
                context,
            } => {
                let mut remaining = index;
                if let Some(parameter) = params.get(remaining) {
                    return Some((parameter.ty, *context));
                }
                remaining = remaining.saturating_sub(params.len());
                if remaining == 0 {
                    return Some((*return_type, *context));
                }
                remaining -= 1;
                for parameter in type_parameters.iter() {
                    if remaining == 0 {
                        return Some((parameter.param, *context));
                    }
                    remaining -= 1;
                    if let Some(constraint) = parameter.constraint {
                        if remaining == 0 {
                            return Some((constraint, *context));
                        }
                        remaining -= 1;
                    }
                    if let Some(default) = parameter.default {
                        if remaining == 0 {
                            return Some((default, *context));
                        }
                        remaining -= 1;
                    }
                }
                None
            }
            ProjectionChildPlan::General { data, context } => {
                self.projection_child_at(data, *context, index)
            }
        }
    }
}

mod finish;
fn projection_finish_may_dispatch(
    data: &SemanticNodeData,
    context: ProjectionReductionContext,
) -> bool {
    let _ = context;
    matches!(
        data,
        SemanticNodeData::KeyOf { .. }
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::InstantiationRef { .. }
    )
}

fn projected(
    memo: &ViewMemo,
    node: SemanticNodeId,
    context: ProjectionReductionContext,
) -> SemanticNodeId {
    *memo
        .get(&(node, context))
        .unwrap_or_else(|| panic!("projection child {node:?} was not completed before its parent"))
}

#[inline(always)]
fn object_projection_child(
    view: &SurfaceView,
    context: ProjectionReductionContext,
    index: usize,
) -> Option<(SemanticNodeId, ProjectionReductionContext)> {
    let member_context = context.into_structural_provenance();
    let mut remaining = index;
    for member in view.positive_members() {
        if let verter_type_expr::AuthoredPropertyKey::Computed(key) = &member.key {
            if remaining == 0 {
                return Some((*key, member_context));
            }
            remaining -= 1;
        }
        if remaining == 0 {
            return Some((member.value, member_context));
        }
        remaining -= 1;
    }
    if let Some(signature) = view.call_signatures.get(remaining) {
        return Some((*signature, context));
    }
    remaining = remaining.saturating_sub(view.call_signatures.len());
    if let Some(signature) = view.construct_signatures.get(remaining) {
        return Some((*signature, context));
    }
    remaining = remaining.saturating_sub(view.construct_signatures.len());
    let index_signature = remaining / 2;
    if let Some(signature) = view.index_signatures.get(index_signature) {
        return Some(if remaining.is_multiple_of(2) {
            (signature.key_type, context)
        } else {
            (signature.value_type, context)
        });
    }
    remaining = remaining.saturating_sub(view.index_signatures.len() * 2);
    if let Some(keyspace) = view.keyspace {
        if remaining == 0 {
            return Some((keyspace, context));
        }
    }
    let _ = remaining;
    None
}

#[cfg(test)]
mod witness_tests {
    use super::{mapped_after_source_visits_for_tests, record_mapped_after_source_visit_for_tests};

    #[test]
    fn mapped_after_source_witness_is_test_thread_local() {
        let main_before = mapped_after_source_visits_for_tests();
        std::thread::spawn(|| {
            let child_before = mapped_after_source_visits_for_tests();
            record_mapped_after_source_visit_for_tests();
            assert_eq!(
                mapped_after_source_visits_for_tests(),
                child_before + 1,
                "the executing test thread observes its own production-path witness"
            );
        })
        .join()
        .expect("witness worker must finish");
        assert_eq!(
            mapped_after_source_visits_for_tests(),
            main_before,
            "a parallel test thread must not mutate this test's witness"
        );
    }
}
