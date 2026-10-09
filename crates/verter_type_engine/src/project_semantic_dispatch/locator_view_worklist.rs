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

#[cfg(any(test, feature = "test-support"))]
std::thread_local! {
    static MAPPED_AFTER_SOURCE_VISITS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(any(test, feature = "test-support"))]
pub fn mapped_after_source_visits_for_tests() -> usize {
    MAPPED_AFTER_SOURCE_VISITS.get()
}

#[cfg(any(test, feature = "test-support"))]
fn record_mapped_after_source_visit_for_tests() {
    MAPPED_AFTER_SOURCE_VISITS.set(MAPPED_AFTER_SOURCE_VISITS.get() + 1);
}

use super::carrier::{
    CarrierArgsContinuation, CarrierFinish, CarrierResolutionPlan, CarrierResolverContext,
};
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
pub struct ProjectedViewOutcome {
    pub node: SemanticNodeId,
    pub completeness: ResultCompleteness,
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
    ConditionalSelectedFinish {
        node: SemanticNodeId,
        selected: SemanticNodeId,
        context: ProjectionReductionContext,
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
    /// A node whose projection is the instantiation the run is waiting on:
    /// the delivered node is its projection.
    AwaitInstantiation {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    },
}

const _: () = assert!(std::mem::size_of::<ProjectionFrame>() <= 32);

/// How a projection run treats the instantiations it reaches.
///
/// Run inline, a projection evaluates each instantiation where it reaches
/// it. Run as a continuation, it parks the node instead, records the
/// instantiation as the run's need, and stops: the caller delivers the
/// instantiated node to [`ProjectSemanticDispatch::drain_projection`],
/// which resumes the run exactly where it stopped.
#[derive(Debug, Default)]
pub(super) struct ProjectionSeam {
    suspend: bool,
    need: Option<SemanticQueryKey>,
}

impl ProjectionSeam {
    /// Evaluate every instantiation in place.
    pub(super) fn inline() -> Self {
        Self::default()
    }

    /// Stop at every instantiation and hand it to the caller.
    pub(super) fn suspending() -> Self {
        Self {
            suspend: true,
            need: None,
        }
    }
}

/// A projection run the caller can resume: the explicit stack of what
/// remains of each node's projection, owned so it can wait across the
/// evaluation of an instantiation it needs.
pub(super) struct ProjectionRun {
    root: SemanticNodeId,
    root_context: ProjectionReductionContext,
    frames: SmallVec<[ProjectionFrame; 16]>,
}

/// Where a projection run stands after it ran as far as it could.
pub(super) enum ProjectionPoll {
    /// The run finished.
    Done(ProjectedViewOutcome),
    /// The run waits on this instantiation.
    Need(SemanticQueryKey),
}

/// How a node's projection finishes: with a node, or with the
/// instantiation whose node it is.
pub(super) enum ProjectionFinish {
    Node(SemanticNodeId),
    Instantiate(SemanticQueryKey),
}

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
    /// The reference projects to this instantiation's node.
    Instantiate(SemanticQueryKey),
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
        /// The predicate target, projected LAST.
        predicate_target: Option<SemanticNodeId>,
        context: ProjectionReductionContext,
    },
    General {
        data: &'a SemanticNodeData,
        context: ProjectionReductionContext,
    },
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Whether an application of a declaration an enclosing build is
    /// materialising stays its recursive back-edge where the run finishes
    /// it now, read off its enclosing nodes' frames on the run's stack:
    ///
    /// - where the checker defers instantiating it — inside an object type
    ///   (its members and signatures), an array or tuple element, a
    ///   signature, a type argument of an interface or class, or either
    ///   branch of a conditional type that stays undecided (the checker
    ///   defers the whole conditional; only a decided conditional's selected
    ///   branch is instantiated);
    /// - or where it is the body's own value through selected conditional
    ///   branches alone: the checker's tail loop, which the build runs in
    ///   place (`getConditionalType`).
    ///
    /// Anywhere else — an alias's type argument, a conditional's check or
    /// extends type, a union's member — the checker instantiates it
    /// eagerly. An instantiated body starts at its own root.
    fn in_deferred_position(&self, ancestors: &[ProjectionFrame]) -> bool {
        let tail = ancestors.iter().all(|frame| match frame {
            ProjectionFrame::ConditionalSelectedFinish { .. }
            | ProjectionFrame::ConditionalAfterTrue { .. }
            | ProjectionFrame::ConditionalFinish { .. }
            | ProjectionFrame::Enter { .. } => true,
            ProjectionFrame::CompositeResume { data, .. } => {
                matches!(data.as_ref(), SemanticNodeData::Alias(_))
            }
            _ => false,
        });
        tail || ancestors.iter().any(|frame| match frame {
            // Both branches of an undecided conditional are projected here;
            // the checker instantiates neither.
            ProjectionFrame::ConditionalAfterTrue { .. }
            | ProjectionFrame::ConditionalFinish { .. } => true,
            ProjectionFrame::CompositeResume { data, .. } => match data.as_ref() {
                SemanticNodeData::Object(_)
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::Signature { .. } => true,
                SemanticNodeData::InstantiationRef { base, .. } => self.defers_type_arguments(base),
                _ => false,
            },
            ProjectionFrame::ReferenceArgsResume { state, .. } => matches!(
                &state.continuation,
                CarrierArgsContinuation::Instantiate { identity, .. }
                    if self.defers_type_arguments(identity)
            ),
            _ => false,
        })
    }

    /// Whether the checker defers instantiating the type arguments of an
    /// application of `declaration`: an interface's or a class's (a
    /// library one included), never an alias's.
    fn defers_type_arguments(&self, declaration: &crate::semantic_query::DeclIdentity) -> bool {
        use verter_session_query::declarations::TypeDeclKind;
        if declaration.canonical_id.as_ref() == "__builtin__" {
            return true;
        }
        !matches!(
            self.prepared_decl_kind(declaration),
            Some(TypeDeclKind::Alias)
        )
    }

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
        .then(|| self.recursive_ref_sentinel(identity, Arc::from([])))
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
                // A value's own type named inside its declared body — a
                // static method returning its class, a literal's method
                // returning the literal — is the type being declared, read
                // by reference as the checker reads it: its members resolve
                // one at a time where a consumer demands them. Resolving it
                // here would build the very surface this body is part of.
                if path.is_empty()
                    && type_args.is_empty()
                    && inputs.self_value.is_some_and(|anchor| {
                        value_root.scope.local_scope.is_none()
                            && value_root.scope.canonical_id == anchor.canonical_id
                            && value_root.scope.owner == anchor.owner
                            && value_root.name == anchor.symbol
                    })
                {
                    return ReferenceProjectionPlan::Ready(node);
                }
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
                    return ReferenceProjectionPlan::Ready(
                        self.recursive_ref_sentinel(identity, Arc::from([])),
                    );
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
                ReferenceProjectionPlan::Instantiate(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        self.type_slot_for(
                            Arc::clone(&identity.canonical_id),
                            identity.owner,
                            Arc::clone(&identity.decl_name),
                        ),
                        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        self.instantiate_context_for(&identity.canonical_id, context),
                    ),
                ))
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn project_view_node_worklist(
        &self,
        root: SemanticNodeId,
        root_context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &mut ViewMemo,
    ) -> ProjectedViewOutcome {
        // A completed projection is free reusable work: a warm root neither
        // installs a connected-demand state nor consumes its work budget.
        if let Some(&done) = memo.get(&(root, root_context)) {
            return ProjectedViewOutcome::complete(done);
        }
        // One connected demand spans the whole run, so its work budget
        // counts every step of it.
        let (_connected_guard, _) = self.enter_connected_demand(false);
        let mut seam = ProjectionSeam::inline();
        let mut run =
            match self.begin_projection(root, root_context, inputs, substitutions, memo, &mut seam)
            {
                Ok(run) => run,
                Err(outcome) => return outcome,
            };
        match self.drain_projection(&mut run, inputs, substitutions, memo, &mut seam, None) {
            ProjectionPoll::Done(outcome) => outcome,
            ProjectionPoll::Need(_) => {
                unreachable!("an inline projection evaluates its instantiations in place")
            }
        }
    }

    /// Begin projecting `root`: answer it at once (a memoized, terminal or
    /// refused root), or schedule it and return the run
    /// [`Self::drain_projection`] carries on.
    pub(super) fn begin_projection(
        &self,
        root: SemanticNodeId,
        root_context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &mut ViewMemo,
        seam: &mut ProjectionSeam,
    ) -> Result<ProjectionRun, ProjectedViewOutcome> {
        // A completed projection is free reusable work. Preserve the original
        // recursive primitive's memo-first contract: warm hits neither install
        // a connected-demand state nor consume its runaway-work budget.
        if let Some(&done) = memo.get(&(root, root_context)) {
            return Err(ProjectedViewOutcome::complete(done));
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
            return Err(ProjectedViewOutcome::complete(recursive));
        }
        if let Some(reasons) = preexisting_trip {
            return Err(ProjectedViewOutcome::partial(root, reasons));
        }
        crate::loop5_instrumentation::watchdog_beat();
        crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
        let Some(root_data) = root_data else {
            if let Err(reasons) = self.charge_connected_work() {
                return Err(ProjectedViewOutcome::partial(root, reasons));
            }
            memo.insert((root, root_context), root);
            return Err(ProjectedViewOutcome::complete(root));
        };
        if let Err(reasons) = self.charge_connected_work() {
            return Err(ProjectedViewOutcome::partial(root, reasons));
        }
        match root_data.as_ref() {
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::SyntheticBinding { .. } => {
                memo.insert((root, root_context), root);
                return Err(ProjectedViewOutcome::complete(root));
            }
            SemanticNodeData::RawFallback { .. } => {
                let miss = self.opaque(QueryError::Miss);
                memo.insert((root, root_context), miss);
                return Err(ProjectedViewOutcome::complete(miss));
            }
            _ => {}
        }

        let mut frames: SmallVec<[ProjectionFrame; 16]> = SmallVec::new();
        let mut work_credit = match ConnectedWorkCredit::new(self.connected_demand()) {
            Ok(credit) => credit,
            Err(reasons) => return Err(ProjectedViewOutcome::partial(root, reasons)),
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
            seam,
        ) {
            return Err(ProjectedViewOutcome::partial(root, reasons));
        }

        Ok(ProjectionRun {
            root,
            root_context,
            frames,
        })
    }

    /// Carry a projection run on as far as it can go. `delivery` is the
    /// node of the instantiation the run last stopped at, when it stopped at
    /// one. An inline run always finishes; a suspending run stops at each
    /// instantiation it reaches.
    pub(super) fn drain_projection(
        &self,
        run: &mut ProjectionRun,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &mut ViewMemo,
        seam: &mut ProjectionSeam,
        delivery: Option<SemanticNodeId>,
    ) -> ProjectionPoll {
        if let Some(key) = seam.need.take() {
            return ProjectionPoll::Need(key);
        }
        let root = run.root;
        let root_context = run.root_context;
        let (_connected_guard, _) = self.enter_connected_demand(false);
        let mut work_credit = match ConnectedWorkCredit::new(self.connected_demand()) {
            Ok(credit) => credit,
            Err(reasons) => {
                return ProjectionPoll::Done(ProjectedViewOutcome::partial(root, reasons))
            }
        };
        let frames = &mut run.frames;
        if let Some(instantiated) = delivery {
            let Some(ProjectionFrame::AwaitInstantiation { node, context }) = frames.pop() else {
                unreachable!("a delivery resumes the node that awaited it")
            };
            self.memoize_projected(memo, node, context, instantiated);
        }
        let mut trip = None;
        // A step that stops at an instantiation leaves the run at once: the
        // node awaiting it stays on top of the stack for its delivery.
        while seam.need.is_none() {
            let Some(frame) = frames.pop() else {
                break;
            };
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
                        frames,
                        &mut work_credit,
                        seam,
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
                            frames,
                            &mut work_credit,
                            seam,
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
                            let result = match self.finish_projection_node(
                                node,
                                context,
                                data.as_ref(),
                                inputs,
                                substitutions,
                                memo,
                                frames,
                            ) {
                                ProjectionFinish::Node(result) => result,
                                ProjectionFinish::Instantiate(key) if seam.suspend => {
                                    frames.push(ProjectionFrame::AwaitInstantiation {
                                        node,
                                        context,
                                    });
                                    seam.need = Some(key);
                                    break;
                                }
                                ProjectionFinish::Instantiate(key) => self.instantiated_node(key),
                            };
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
                            frames,
                            &mut work_credit,
                            seam,
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
                        check,
                        extends,
                        true_branch_ref,
                        false_branch_ref,
                        distributive,
                        pending,
                    } = data.as_ref()
                    else {
                        unreachable!("conditional staging frame must carry a conditional")
                    };
                    let decision_shell = SemanticNodeData::Conditional {
                        check: projected(memo, *check, context),
                        extends: projected(memo, *extends, extends_context),
                        true_branch_ref: *true_branch_ref,
                        false_branch_ref: *false_branch_ref,
                        distributive: *distributive,
                        pending: pending.clone(),
                    };
                    work_credit.settle();
                    let selected = match self.execute_type_node(SemanticQueryKey::Conditional {
                        check: projected(memo, *check, context),
                        extends: projected(memo, *extends, extends_context),
                        true_branch: *true_branch_ref,
                        false_branch: *false_branch_ref,
                        distributive: *distributive,
                        pending: pending.clone(),
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                        _ => self.opaque(QueryError::Miss),
                    };
                    if let Err(reasons) = work_credit.refresh() {
                        trip = Some(reasons);
                        break;
                    }
                    if self.graph().node_data(selected).as_deref() != Some(&decision_shell) {
                        frames.push(ProjectionFrame::ConditionalSelectedFinish {
                            node,
                            selected,
                            context,
                        });
                        frames.push(ProjectionFrame::Enter {
                            node: selected,
                            context,
                        });
                        continue;
                    }
                    // Both branches of a suspended conditional are demanded by
                    // this view. Discharge before projecting either subtree.
                    let true_branch = self.apply_conditional_branch_pending(
                        *true_branch_ref,
                        pending.as_deref(),
                        true,
                    );
                    let false_branch = self.apply_conditional_branch_pending(
                        *false_branch_ref,
                        pending.as_deref(),
                        false,
                    );
                    let data = Arc::new(SemanticNodeData::Conditional {
                        check: *check,
                        extends: *extends,
                        true_branch_ref: true_branch,
                        false_branch_ref: false_branch,
                        distributive: *distributive,
                        pending: None,
                    });
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
                ProjectionFrame::ConditionalSelectedFinish {
                    node,
                    selected,
                    context,
                } => {
                    self.memoize_projected(memo, node, context, projected(memo, selected, context));
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
                        pending,
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
                        pending: pending.clone(),
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
                    #[cfg(any(test, feature = "test-support"))]
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
                ProjectionFrame::AwaitInstantiation { .. } => {
                    unreachable!("an awaiting node resumes only with its delivery")
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
                        let deferred = || self.in_deferred_position(frames);
                        let result = match self.plan_carrier_finish(
                            state.continuation,
                            projected_args,
                            &deferred,
                        ) {
                            CarrierFinish::Node(result) => result,
                            CarrierFinish::Instantiate(key) if seam.suspend => {
                                frames.push(ProjectionFrame::AwaitInstantiation { node, context });
                                seam.need = Some(key);
                                break;
                            }
                            CarrierFinish::Instantiate(key) => self.instantiated_node(key),
                        };
                        if let Err(reasons) = work_credit.refresh() {
                            trip = Some(reasons);
                            break;
                        }
                        self.memoize_projected(memo, node, context, result);
                    }
                }
            }
            if seam.need.is_some() {
                break;
            }
            if let Some(reasons) = self.connected_demand_trip() {
                trip = Some(reasons);
                break;
            }
        }

        work_credit.settle();
        if let Some(key) = seam.need.take() {
            return ProjectionPoll::Need(key);
        }
        ProjectionPoll::Done(
            if let Some(reasons) = trip.or_else(|| self.connected_demand_trip()) {
                ProjectedViewOutcome::partial(root, reasons)
            } else {
                ProjectedViewOutcome::complete(projected(memo, root, root_context))
            },
        )
    }

    /// The node an instantiation a projection reached evaluates to.
    pub(super) fn instantiated_node(&self, key: SemanticQueryKey) -> SemanticNodeId {
        match self.execute_type_node(key) {
            QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
            _ => self.opaque(QueryError::Miss),
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
            | SemanticNodeData::EnumLiteral(_)
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
            // An inert structure is its own projection under every context.
            SemanticNodeData::Tuple { .. } | SemanticNodeData::Array { .. }
                if self.graph().node_is_inert_structure(node) =>
            {
                work_credit.consume()?;
                crate::loop5_instrumentation::watchdog_beat();
                crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node_worklist");
                memo.insert((node, context), node);
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
        seam: &mut ProjectionSeam,
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
                | SemanticNodeData::EnumLiteral(_)
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
                // An inert structure is its own projection under every context.
                SemanticNodeData::Tuple { .. } | SemanticNodeData::Array { .. }
                    if self.graph().node_is_inert_structure(child) =>
                {
                    work_credit.consume()?;
                    crate::loop5_instrumentation::watchdog_beat();
                    crate::loop5_instrumentation::watchdog_check_and_dump(
                        "project_view_node_worklist",
                    );
                    vacant.insert(child);
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
                seam,
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
        let result = match self.finish_projection_node(
            node,
            context,
            data.as_ref(),
            inputs,
            substitutions,
            memo,
            frames,
        ) {
            ProjectionFinish::Node(result) => result,
            ProjectionFinish::Instantiate(key) if seam.suspend => {
                frames.push(ProjectionFrame::AwaitInstantiation { node, context });
                seam.need = Some(key);
                return Ok(());
            }
            ProjectionFinish::Instantiate(key) => self.instantiated_node(key),
        };
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
            over_type_variable: mapper.over_type_variable,
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
        seam: &mut ProjectionSeam,
    ) -> Result<bool, crate::semantic_query::PartialReasonSet> {
        match data.as_ref() {
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_)
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
                match plan {
                    ReferenceProjectionPlan::Ready(result) => {
                        work_credit.refresh()?;
                        self.memoize_projected(memo, node, context, result);
                        Ok(false)
                    }
                    ReferenceProjectionPlan::Instantiate(key) if seam.suspend => {
                        frames.push(ProjectionFrame::AwaitInstantiation { node, context });
                        seam.need = Some(key);
                        Ok(true)
                    }
                    ReferenceProjectionPlan::Instantiate(key) => {
                        let result = self.instantiated_node(key);
                        work_credit.refresh()?;
                        self.memoize_projected(memo, node, context, result);
                        Ok(false)
                    }
                    ReferenceProjectionPlan::NeedsArgs {
                        continuation,
                        args,
                        argument_context,
                    } => {
                        work_credit.refresh()?;
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
            SemanticNodeData::IntrinsicApplication { args, .. } => args
                .get(index)
                .map(|argument| (*argument, context.into_structural_provenance())),
            SemanticNodeData::Alias(target) => (index == 0).then_some((*target, context)),
            // The reference's type arguments, then the instance surface.
            SemanticNodeData::ClassExpressionInstance {
                type_arguments,
                surface,
                ..
            } => match type_arguments.get(index) {
                Some(argument) => Some((*argument, context)),
                None => (index == type_arguments.len()).then_some((*surface, context)),
            },
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
                predicate,
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
                predicate
                    .and_then(|predicate| predicate.ty)
                    .filter(|_| remaining == 0)
                    .map(|target| (target, context))
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
            | SemanticNodeData::EnumLiteral(_)
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
                predicate,
                ..
            } => ProjectionChildPlan::Function {
                params,
                return_type: *return_type,
                type_parameters,
                predicate_target: predicate.and_then(|predicate| predicate.ty),
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
                predicate_target,
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
                predicate_target
                    .filter(|_| remaining == 0)
                    .map(|target| (target, *context))
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
