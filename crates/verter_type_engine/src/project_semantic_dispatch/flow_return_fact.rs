//! The fact a flow-return read establishes for its consumer, and the
//! scoped observation of a flow read whose consumer may still decline it.
//!
//! # Producer guarantees
//!
//! A flow-return demand answers through [`FlowReturnStep`]; its consumer
//! reads that step together with the completeness its read observed as ONE
//! [`FactResult`] ([`ObservedRead::fact`]):
//!
//! - **Complete** — an undegraded evaluated return whose read observed no
//!   partiality: the exact return, warm-capable.
//! - **Approximate** — a USABLE UNFINISHED value. The documented
//!   interpretation is the evaluated return itself: a degraded success
//!   keeps every modelled position exact and carries the typed unresolved
//!   marker at a position it could not type ([`PartialReason::FlowReturnUninferred`](crate::semantic_query::PartialReason::FlowReturnUninferred)),
//!   or its member set is complete while a member's type may be unverified
//!   ([`PartialReason::FlowReturnUnverified`](crate::semantic_query::PartialReason::FlowReturnUnverified)), or it is the checker's
//!   recovery after an operation exhausted its allowance
//!   ([`PartialReason::OperationBudget`](crate::semantic_query::PartialReason::OperationBudget)). An undegraded value whose read
//!   observed partiality (a withheld proof, an unverified dependency) is
//!   approximate by the read's own classes. An approximation is never an
//!   exact answer and never warms.
//! - **Unavailable** — NO VALUE: a typed [`FlowReturnFailure`] or an
//!   in-flight recursive hold surfacing at its consumer. Always carries
//!   [`PartialReason::FlowReturnNoSurface`](crate::semantic_query::PartialReason::FlowReturnNoSurface), plus whatever the read
//!   observed.
//! - **Aborted** — [`FlowReturnStep::Aborted`] carries invalid execution
//!   evidence ([`ExecutionAbort`]); the consumer publishes nothing. A
//!   preparation cause alone never invalidates an independently evaluated value.
//!
//! The arm decides; the causes explain. A consumer never re-derives
//! availability from the reason classes.
//!
//! # Discarded probes
//!
//! A read the consumer may DECLINE — a probe with a fall-through route
//! that re-derives the authoritative answer and its own rails — runs under
//! [`ProjectSemanticDispatch::observe_read`]. Its observations (the
//! per-cold-compute completeness, the build-local taint frame and the
//! deferred request sticky) are held aside, and the consumer states its
//! choice by type: [`ObservedRead::adopt`] joins them into the
//! enclosing build exactly as an unscoped read would have, and
//! [`ObservedRead::discard`] drops them because the fall-through
//! route owns the answer. A failed probe therefore never marks the
//! enclosing composition partial, and an adopted one never hides its
//! partiality. Execution validity has a separate, non-discardable scope:
//! declining a proof probe cannot clear an abort it observed.

use super::flow_return::NO_VALUE_REASON;
use super::{BuildLocalTaint, BuildLocalTaintGuard, ProjectSemanticDispatch};
use crate::semantic_query::surface_resolution::NonEmptyReasons;
use crate::semantic_query::{
    ExecutionAbort, FactResult, FlowReturnResult, FlowReturnStep, ResultCompleteness,
};

/// One flow read run under private observation rails, awaiting its
/// consumer's adopt-or-discard decision.
#[must_use = "a scoped flow read must be adopted or discarded"]
pub(super) struct ObservedRead<T> {
    value: T,
    abort: Option<ExecutionAbort>,
    completeness: ResultCompleteness,
    frame: BuildLocalTaint,
    degraded_member_reads: u32,
}

/// The observation owner for a stepped read, held until its child delivers.
pub(super) struct ReadObservationScope<'a> {
    validity: crate::request_context::ExecutionValidityScope,
    sticky: crate::request_context::DeferredPartialStickyScope,
    completeness: crate::request_context::ColdComputeCompletenessScope,
    frame: BuildLocalTaintGuard<'a>,
    degraded: DegradedCounterScope<'a>,
}

struct DegradedCounterScope<'a> {
    counter: &'a verter_execution::sync_cell::SyncU32,
    before: u32,
}

impl Drop for DegradedCounterScope<'_> {
    fn drop(&mut self) {
        self.counter.set(self.before);
    }
}

impl ReadObservationScope<'_> {
    pub(super) fn finish<T>(self, value: T) -> ObservedRead<T> {
        let degraded_member_reads = self
            .degraded
            .counter
            .get()
            .saturating_sub(self.degraded.before);
        let abort = crate::request_context::current_execution_abort();
        let frame = self.frame.finish();
        let completeness = crate::request_context::current_cold_compute_completeness();
        self.completeness.discard();
        drop(self.sticky);
        drop(self.validity);
        drop(self.degraded);
        ObservedRead {
            value,
            abort,
            completeness,
            frame,
            degraded_member_reads,
        }
    }
}

/// An owned return source awaiting a value demand.
/// Its constructor is confined to the shared signature producer below.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeferredFlowReturnSource {
    source: verter_type_expr::facts::FunctionReturnSource,
    bindings: std::sync::Arc<[(std::sync::Arc<str>, crate::semantic_query::SemanticNodeId)]>,
    receiver: Option<crate::semantic_query::SemanticNodeId>,
}

impl DeferredFlowReturnSource {
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn bindings_for_tests(
        &self,
    ) -> &[(std::sync::Arc<str>, crate::semantic_query::SemanticNodeId)] {
        &self.bindings
    }

    pub(crate) fn bindings(
        &self,
    ) -> &[(std::sync::Arc<str>, crate::semantic_query::SemanticNodeId)] {
        &self.bindings
    }

    /// Capture the actual lexical header images; body inference still runs
    /// in its declaration environment before this existing rebinding applies.
    pub(super) fn with_bindings(
        mut self,
        bindings: impl IntoIterator<Item = (std::sync::Arc<str>, crate::semantic_query::SemanticNodeId)>,
    ) -> Self {
        let mut bindings: Vec<_> = bindings.into_iter().collect();
        bindings.sort_by(|left, right| left.0.cmp(&right.0));
        self.bindings = std::sync::Arc::from(bindings);
        self
    }

    /// A consumed own clause specializes its deferred result at this boundary;
    /// uninstantiated headers never capture their own binders as lexical images.
    pub(crate) fn with_instantiated_clause(
        self,
        bindings: impl IntoIterator<Item = (std::sync::Arc<str>, crate::semantic_query::SemanticNodeId)>,
    ) -> Self {
        let mut images = self.bindings.to_vec();
        for (name, image) in bindings {
            if let Some(binding) = images.iter_mut().find(|(bound, _)| *bound == name) {
                binding.1 = image;
            } else {
                images.push((name, image));
            }
        }
        self.with_bindings(images)
    }

    pub(crate) fn receiver(&self) -> Option<crate::semantic_query::SemanticNodeId> {
        self.receiver
    }

    pub(crate) fn with_receiver(mut self, receiver: crate::semantic_query::SemanticNodeId) -> Self {
        self.receiver = Some(receiver);
        self
    }

    pub(crate) fn map_images(
        &self,
        mut map: impl FnMut(
            crate::semantic_query::SemanticNodeId,
        ) -> crate::semantic_query::SemanticNodeId,
    ) -> Self {
        Self {
            source: self.source.clone(),
            bindings: std::sync::Arc::from(
                self.bindings
                    .iter()
                    .map(|(name, image)| (name.clone(), map(*image)))
                    .collect::<Vec<_>>(),
            ),
            receiver: self.receiver.map(map),
        }
    }

    pub(super) fn apply_result<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
        value: crate::semantic_query::SemanticNodeId,
    ) -> crate::semantic_query::SemanticNodeId {
        let value = dispatch.rebind_flow_return_binders(
            value,
            self.identity().anchor.canonical_id.as_ref(),
            |name| {
                self.bindings
                    .iter()
                    .find_map(|(bound, image)| (bound.as_ref() == name).then_some(*image))
            },
        );
        self.receiver.map_or(value, |receiver| {
            dispatch.bind_callee_receiver(value, receiver)
        })
    }
    pub(crate) fn source(&self) -> &verter_type_expr::facts::FunctionReturnSource {
        &self.source
    }
    pub(crate) fn identity(&self) -> &verter_type_expr::facts::FlowFunctionReturnIdentity {
        let verter_type_expr::facts::FunctionReturnSource::Flow(identity) = &self.source else {
            unreachable!("only a body source constructs this carrier")
        };
        identity
    }
}

pub(super) enum SignatureReturnProbe {
    Evaluated(FlowReturnResult),
    Deferred(DeferredFlowReturnSource),
}

pub(super) type SignatureSourceHeaders = verter_execution::sync_cell::SyncRefCell<
    Vec<(
        std::sync::Arc<str>,
        verter_type_expr::TopLevelOwnerId,
        std::sync::Arc<str>,
    )>,
>;

pub(super) struct SignatureSourceHeaderGuard<'a>(&'a SignatureSourceHeaders);
impl<'a> SignatureSourceHeaderGuard<'a> {
    pub(super) fn enter(
        headers: &'a SignatureSourceHeaders,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol: &str,
    ) -> Self {
        headers.borrow_mut().push((
            std::sync::Arc::<str>::from(verter_type_expr::OwnedStr::from(canonical)),
            owner,
            std::sync::Arc::<str>::from(verter_type_expr::OwnedStr::from(symbol)),
        ));
        Self(headers)
    }
}
impl Drop for SignatureSourceHeaderGuard<'_> {
    fn drop(&mut self) {
        self.0.borrow_mut().pop();
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Returning a callable value demands its inferred return slot. Merely
    /// carrying a signature source as a type operand does not demand that slot.
    pub(super) fn materialize_returned_callable(
        &self,
        node: crate::semantic_query::SemanticNodeId,
    ) -> crate::semantic_query::SemanticNodeId {
        use crate::semantic_query::{
            FactStatus, QueryError, SemanticNodeData, SignatureKind, SignatureReturnCarrier,
        };
        let graph = self.graph();
        let Some(data) = graph.node_data(node) else {
            return node;
        };
        if let SemanticNodeData::DeferredCallable(callable) = data.as_ref() {
            let callable = callable.clone();
            let parts =
                callable.parts(&crate::semantic_query::ResolveOverloadSetConsumer::witness());
            let carrier = parts.return_carrier.clone();
            let mut inferred_predicate = None;
            let returned = match carrier.result_source() {
                crate::semantic_query::SignatureResultSource::Declared(node) => *node,
                crate::semantic_query::SignatureResultSource::Function(source) => {
                    let Some(identity) = carrier.flow_identity() else {
                        return node;
                    };
                    let key = self.flow_return_key_for(identity);
                    if self
                        .dispatch_txn
                        .borrow()
                        .reentry()
                        .find(&super::dispatch_txn::ObligationIdentity::FlowReturn(key))
                        .is_some()
                    {
                        return node;
                    }
                    match self.execute_function_return_source(
                        source,
                        identity.anchor.canonical_id.as_ref(),
                    ) {
                        super::flow_return::FunctionReturnNode::Flow(result) => match result
                            .fact()
                            .status()
                        {
                            FactStatus::Complete | FactStatus::Approximate(_, _) => {
                                inferred_predicate = result.inferred_predicate();
                                self.apply_signature_return_source(&carrier, result.return_type())
                            }
                            FactStatus::Unavailable(causes) => {
                                self.opaque(QueryError::IncompleteSemanticOperand {
                                    reasons: causes.get(),
                                })
                            }
                        },
                        super::flow_return::FunctionReturnNode::Aborted(abort) => {
                            self.opaque(QueryError::ExecutionAborted(abort))
                        }
                        _ => self.opaque(QueryError::IncompleteSemanticOperand {
                            reasons:
                                crate::semantic_query::PartialReasonSet::FLOW_RETURN_NO_SURFACE,
                        }),
                    }
                }
            };
            let mut signature = callable.into_general_signature(returned);
            if let SemanticNodeData::Signature { predicate, .. } = &mut signature {
                if predicate.is_none() {
                    *predicate = inferred_predicate;
                }
            }
            return graph.intern_preserving_scope(node, signature);
        }
        if let SemanticNodeData::Object(surface) = data.as_ref() {
            let entries = surface
                .entries
                .iter()
                .map(|entry| match entry {
                    crate::semantic_query::SurfaceEntry::CallSignature(signature) => {
                        crate::semantic_query::SurfaceEntry::CallSignature(
                            self.materialize_returned_callable(*signature),
                        )
                    }
                    _ => entry.clone(),
                })
                .collect::<Vec<_>>();
            if entries.as_slice() == surface.entries.as_ref() {
                return node;
            }
            return graph.intern_node_with_scope(
                SemanticNodeData::Object(crate::semantic_query::SurfaceView::from_entries(
                    entries,
                    surface.keyspace,
                    surface.closed().has_index_signature(),
                )),
                graph
                    .node_scope(node)
                    .unwrap_or(crate::semantic_query::NodeScopeId::Global),
            );
        }
        let SemanticNodeData::Signature {
            kind: SignatureKind::Call,
            return_carrier: SignatureReturnCarrier::DeferredFlow(source),
            ..
        } = data.as_ref()
        else {
            return node;
        };
        let source = source.clone();
        let key = self.flow_return_key_for(source.identity());
        if self
            .dispatch_txn
            .borrow()
            .reentry()
            .find(&super::dispatch_txn::ObligationIdentity::FlowReturn(key))
            .is_some()
        {
            return node;
        }
        let mut inferred_predicate = None;
        let returned = match self.execute_function_return_source(
            source.source(),
            source.identity().anchor.canonical_id.as_ref(),
        ) {
            super::flow_return::FunctionReturnNode::Flow(result) => match result.fact().status() {
                FactStatus::Complete | FactStatus::Approximate(_, _) => {
                    inferred_predicate = result.inferred_predicate();
                    source.apply_result(self, result.return_type())
                }
                FactStatus::Unavailable(causes) => {
                    self.opaque(QueryError::IncompleteSemanticOperand {
                        reasons: causes.get(),
                    })
                }
            },
            super::flow_return::FunctionReturnNode::Aborted(abort) => {
                self.opaque(QueryError::ExecutionAborted(abort))
            }
            _ => self.opaque(QueryError::IncompleteSemanticOperand {
                reasons: crate::semantic_query::PartialReasonSet::FLOW_RETURN_NO_SURFACE,
            }),
        };
        let mut rebuilt = data.as_ref().clone();
        let SemanticNodeData::Signature {
            return_type,
            predicate,
            return_carrier,
            ..
        } = &mut rebuilt
        else {
            unreachable!()
        };
        *return_type = returned;
        *return_carrier = crate::semantic_query::SignatureReturnCarrier::EvaluatedFlow(source);
        if predicate.is_none() {
            *predicate = inferred_predicate;
        }
        graph.intern_node_with_scope(
            rebuilt,
            graph
                .node_scope(node)
                .unwrap_or(crate::semantic_query::NodeScopeId::Global),
        )
    }

    pub(super) fn apply_signature_return_source(
        &self,
        carrier: &crate::semantic_query::SignatureReturnCarrier,
        value: crate::semantic_query::SemanticNodeId,
    ) -> crate::semantic_query::SemanticNodeId {
        match carrier {
            crate::semantic_query::SignatureReturnCarrier::DeferredFlow(source)
            | crate::semantic_query::SignatureReturnCarrier::EvaluatedFlow(source) => {
                source.apply_result(self, value)
            }
            _ => value,
        }
    }

    /// Observe the selected value fact separately from the whole-return
    /// completion proof. Only the producer's sealed source certificate permits
    /// this projection; an uncertified result adopts the whole read unchanged.
    pub(super) fn observe_flow_return_value(
        &self,
        key: crate::semantic_query::FlowReturnKey,
    ) -> ObservedRead<FlowReturnStep> {
        self.observe_read(|| {
            let whole = self.observe_read(|| self.execute_flow_return(key.clone()));
            let proof = match whole.value() {
                FlowReturnStep::Complete(result) => result.independent_member(&key).cloned(),
                _ => None,
            };
            if let Some(proof) = proof {
                let mut roots = whole.frame.observed_self_roots.clone();
                for root in proof.roots() {
                    if !roots.contains(root) {
                        roots.push(root.clone());
                    }
                }
                self.fold_observed_frame_into_top(&BuildLocalTaint {
                    cache_suppress: whole.frame.cache_suppress,
                    observed_self_roots: roots,
                    ..BuildLocalTaint::default()
                });
                whole.discard().unwrap_or_else(FlowReturnStep::Aborted)
            } else {
                whole.adopt(self).unwrap_or_else(FlowReturnStep::Aborted)
            }
        })
    }

    /// Demand the selected return VALUE fact through the source producer.
    /// A finite source certificate can prove this value independently of the
    /// whole frame's runtime capture contract; inherited read failures and
    /// execution aborts still prevent an exact answer.
    #[cfg(any(test, feature = "test-support"))]
    pub fn execute_flow_return_value_fact(
        &self,
        key: crate::semantic_query::FlowReturnKey,
    ) -> Result<FactResult<crate::semantic_query::SemanticNodeId>, ExecutionAbort> {
        let read = self.observe_flow_return_value(key.clone());
        let independent = matches!(read.value(), FlowReturnStep::Complete(result)
            if result.independent_member(&key).is_some());
        let fact = if independent && read.observations_are_complete() {
            let FlowReturnStep::Complete(result) = read.value() else {
                unreachable!()
            };
            FactResult::Complete(result.return_type())
        } else {
            read.fact()?.map(|result| result.return_type())
        };
        read.adopt(self)?;
        Ok(fact)
    }

    /// A key-domain demand consumes only the source's certified static keys.
    /// Whole-return value limitations remain on the separate value fact.
    pub(super) fn observe_flow_return_key_domain(
        &self,
        key: crate::semantic_query::FlowReturnKey,
    ) -> ObservedRead<FlowReturnStep> {
        self.observe_read(|| {
            let whole = self.observe_read(|| self.execute_flow_return(key.clone()));
            let domain = match whole.value() {
                FlowReturnStep::Complete(result) => result.exact_return_keys().cloned(),
                _ => None,
            };
            if let Some(domain) = domain {
                self.fold_observed_frame_into_top(&BuildLocalTaint {
                    cache_suppress: whole.frame.cache_suppress,
                    observed_self_roots: domain.roots().to_vec(),
                    ..BuildLocalTaint::default()
                });
                whole.discard().unwrap_or_else(FlowReturnStep::Aborted)
            } else {
                whole.adopt(self).unwrap_or_else(FlowReturnStep::Aborted)
            }
        })
    }

    /// Signature construction owns its source; consumers demand its value.
    pub(super) fn probe_signature_flow_return(
        &self,
        identity: &verter_type_expr::facts::FlowFunctionReturnIdentity,
        canonical: &str,
    ) -> SignatureReturnProbe {
        if self
            .signature_source_headers
            .borrow()
            .iter()
            .any(|(canonical, owner, symbol)| {
                canonical == &identity.anchor.canonical_id
                    && *owner == identity.anchor.owner
                    && symbol == &identity.anchor.symbol
            })
        {
            return SignatureReturnProbe::Deferred(DeferredFlowReturnSource {
                source: verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
                bindings: std::sync::Arc::from([]),
                receiver: None,
            });
        }
        let read = self.observe_read(|| {
            self.execute_function_return_source(
                &verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
                canonical,
            )
        });
        let key = self.flow_return_key_for(identity);
        if read.observations_are_complete()
            && matches!(read.value(), super::flow_return::FunctionReturnNode::Flow(result)
                if result.fact().status().is_complete() || result.independent_member(&key).is_some())
        {
            if let Ok(super::flow_return::FunctionReturnNode::Flow(result)) = read.adopt(self) {
                return SignatureReturnProbe::Evaluated(result);
            }
        } else {
            let _ = read.discard();
        }
        // A structural source may be complete when an ordinary eager probe
        // is declined. A pending continuation operand must instead prevent
        // that tentative source shape from warming before its retry.
        if self.decision_waits() {
            self.fold_local_partial_completeness(
                crate::semantic_query::PartialReasonSet::PROPAGATED,
            );
        }
        SignatureReturnProbe::Deferred(DeferredFlowReturnSource {
            source: verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
            bindings: std::sync::Arc::from([]),
            receiver: None,
        })
    }

    /// A failed value projection can produce a contained diagnostic instead.
    /// That diagnostic retains source roots and reuse refusal, but consumes no
    /// provisional value. Successful projections adopt all their observations.
    pub fn project_semantic_attempt<T, E>(
        &self,
        projection: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<T, E>, ExecutionAbort> {
        let read = self.observe_read(projection);
        if read.value().is_ok() {
            read.adopt(self)
        } else {
            self.fold_observed_frame_into_top(&BuildLocalTaint {
                cache_suppress: read.frame.cache_suppress,
                observed_self_roots: read.frame.observed_self_roots.clone(),
                ..BuildLocalTaint::default()
            });
            read.discard()
        }
    }

    /// Run `read` with its observations held aside: the request sticky is
    /// deferred, and the per-cold-compute completeness and build-local
    /// taint the read produces are captured instead of joining the
    /// enclosing build. The returned read states, by
    /// [`ObservedRead::adopt`] or [`ObservedRead::discard`],
    /// whether they join it.
    pub(super) fn observe_read<T>(&self, read: impl FnOnce() -> T) -> ObservedRead<T> {
        let scope = self.begin_read_observation();
        scope.finish(read())
    }

    pub(super) fn begin_read_observation(&self) -> ReadObservationScope<'_> {
        ReadObservationScope {
            validity: crate::request_context::ExecutionValidityScope::enter(),
            sticky: crate::request_context::DeferredPartialStickyScope::enter(),
            completeness: crate::request_context::ColdComputeCompletenessScope::enter(),
            frame: BuildLocalTaintGuard::push(&self.build_local_taint),
            degraded: DegradedCounterScope {
                counter: &self.degraded_member_reads,
                before: self.degraded_member_reads.get(),
            },
        }
    }
}

impl<T> ObservedRead<T> {
    /// An exact optional probe needs its observed rails to be complete.
    pub(super) fn observations_are_complete(&self) -> bool {
        self.abort.is_none()
            && !self.completeness.is_partial()
            && !self.frame.result_is_partial
            && self.frame.partial_reasons.is_empty()
            && self.degraded_member_reads == 0
    }

    /// Completion rails of a started computation, forwarded before adoption.
    pub(super) fn result_completeness(&self) -> ResultCompleteness {
        self.completeness
            .or_partial_if(self.frame.result_is_partial, self.frame.partial_reasons)
            .or_partial_if(
                self.degraded_member_reads > 0,
                crate::semantic_query::PartialReasonSet::PROPAGATED,
            )
            .or_partial_if(
                self.abort.is_some(),
                self.abort
                    .map_or(crate::semantic_query::PartialReasonSet::empty(), |abort| {
                        abort.reason_class()
                    }),
            )
    }

    /// Reuse restrictions and roots belong to the observed source even when
    /// its optional diagnostic quality is declined.
    pub(super) fn preserve_reuse_evidence<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) {
        dispatch.fold_observed_frame_into_top(&BuildLocalTaint {
            cache_suppress: self.frame.cache_suppress,
            observed_self_roots: self.frame.observed_self_roots.clone(),
            ..BuildLocalTaint::default()
        });
    }

    pub(super) fn value(&self) -> &T {
        &self.value
    }

    pub(super) fn operation_refusals(&self) -> &[super::walk::ShallowDiagnostic] {
        &self.frame.operation_refusals
    }

    /// Keep a provisional value and its observations separate until the
    /// consumer either retains that exact position or replaces it.
    pub(super) fn split(self) -> Result<(T, ObservedRead<()>), ExecutionAbort> {
        if let Some(abort) = self.abort {
            crate::request_context::observe_execution_abort(abort);
            return Err(abort);
        }
        Ok((
            self.value,
            ObservedRead {
                value: (),
                abort: None,
                completeness: self.completeness,
                frame: self.frame,
                degraded_member_reads: self.degraded_member_reads,
            },
        ))
    }

    /// The consumer USES the value: the read's observations join the
    /// enclosing build and the request exactly as an unscoped read's would.
    pub(super) fn adopt<C: crate::resolver_core::ResolverCapabilities>(
        self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Result<T, ExecutionAbort> {
        if let Some(abort) = self.abort {
            crate::request_context::observe_execution_abort(abort);
            return Err(abort);
        }
        crate::request_context::fold_result_completeness(self.completeness);
        dispatch.fold_observed_frame_into_top(&self.frame);
        dispatch.degraded_member_reads.set(
            dispatch
                .degraded_member_reads
                .get()
                .saturating_add(self.degraded_member_reads),
        );
        Ok(self.value)
    }

    /// The consumer DECLINES the value and a fall-through route owns the
    /// answer: the read's observations are dropped.
    pub(super) fn discard(self) -> Result<T, ExecutionAbort> {
        if let Some(abort) = self.abort {
            crate::request_context::observe_execution_abort(abort);
            Err(abort)
        } else {
            Ok(self.value)
        }
    }
}

impl ObservedRead<()> {
    /// Re-open a stepped read's observation for its next step: a fresh
    /// scope that already holds what the read observed in its earlier
    /// steps, so a read whose computation waits across the steps of a
    /// continuation frame observes exactly what it would in one native call.
    pub(super) fn resume<'d, C: crate::resolver_core::ResolverCapabilities>(
        self,
        dispatch: &'d ProjectSemanticDispatch<'_, C>,
    ) -> ReadObservationScope<'d> {
        let scope = dispatch.begin_read_observation();
        if let Some(abort) = self.abort {
            crate::request_context::observe_execution_abort(abort);
        }
        crate::request_context::fold_result_completeness(self.completeness);
        dispatch.fold_observed_frame_into_top(&self.frame);
        dispatch.degraded_member_reads.set(
            dispatch
                .degraded_member_reads
                .get()
                .saturating_add(self.degraded_member_reads),
        );
        scope
    }
}

impl ObservedRead<FlowReturnStep> {
    /// Consume the separately certified selected value, retaining source roots
    /// and reuse refusal. The whole-return preparation is not its value proof.
    pub(super) fn adopt_independent_member<C: crate::resolver_core::ResolverCapabilities>(
        self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
        key: &crate::semantic_query::FlowReturnKey,
    ) -> Result<crate::semantic_query::SemanticNodeId, ExecutionAbort> {
        self.into_independent_member(key)?.adopt(dispatch)
    }

    fn into_independent_member(
        self,
        key: &crate::semantic_query::FlowReturnKey,
    ) -> Result<ObservedRead<crate::semantic_query::SemanticNodeId>, ExecutionAbort> {
        if let Some(abort) = self.abort {
            crate::request_context::observe_execution_abort(abort);
            return Err(abort);
        }
        let FlowReturnStep::Complete(result) = self.value else {
            unreachable!("only a separately certified value can be adopted")
        };
        let proof = result
            .independent_member(key)
            .expect("only a separately certified value can be adopted");
        let mut roots = self.frame.observed_self_roots.clone();
        for root in proof.roots() {
            if !roots.contains(root) {
                roots.push(root.clone());
            }
        }
        let mut frame = self.frame;
        frame.observed_self_roots = roots;
        Ok(ObservedRead {
            value: result.return_type(),
            abort: None,
            completeness: self.completeness,
            frame,
            degraded_member_reads: self.degraded_member_reads,
        })
    }

    /// The fact this flow-return read established. See the module
    /// documentation for each arm's guarantee.
    pub(super) fn fact(&self) -> Result<FactResult<&FlowReturnResult>, ExecutionAbort> {
        if let Some(abort) = self.abort {
            return Err(abort);
        }
        flow_return_fact(&self.value, self.completeness)
    }
}

/// The fact a flow-return step establishes under the completeness its read
/// observed. The ONE mapping from the flow producer's typed outcome to its
/// consumer-facing quality.
pub(super) fn flow_return_fact(
    step: &FlowReturnStep,
    observed: ResultCompleteness,
) -> Result<FactResult<&FlowReturnResult>, ExecutionAbort> {
    if let FlowReturnStep::Aborted(abort) = step {
        return Err(*abort);
    }
    let observed_reasons = match observed {
        ResultCompleteness::Complete => None,
        ResultCompleteness::Partial(reasons) => {
            Some(NonEmptyReasons::new(reasons).unwrap_or_else(|| {
                NonEmptyReasons::of(crate::semantic_query::PartialReason::SemanticQueryFault)
            }))
        }
    };
    Ok(match step {
        FlowReturnStep::Aborted(_) => unreachable!("aborts are not facts"),
        FlowReturnStep::Complete(result) => {
            let own = match result.fact() {
                FactResult::Complete(_) => None,
                FactResult::Approximate { causes, .. } => Some(causes),
                FactResult::Unavailable { .. } => {
                    unreachable!("a flow value has evaluated evidence")
                }
            };
            match join(own, observed_reasons) {
                None => FactResult::complete(result),
                Some(causes) => FactResult::approximate(result, causes),
            }
        }
        FlowReturnStep::NoValue(_) | FlowReturnStep::Hold(_) => {
            let no_value = NonEmptyReasons::of(NO_VALUE_REASON);
            FactResult::unavailable(match observed_reasons {
                Some(observed) => no_value.union(observed),
                None => no_value,
            })
        }
    })
}

fn join(left: Option<NonEmptyReasons>, right: Option<NonEmptyReasons>) -> Option<NonEmptyReasons> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.union(right)),
        (one, None) | (None, one) => one,
    }
}

#[cfg(test)]
#[path = "flow_return_fact_tests.rs"]
mod flow_return_fact_tests;
