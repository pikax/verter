//! The checker's error recovery in a flow evaluation.
//!
//! The checker reports a read of a member a type proves absent (TS2339) and
//! a return that requires itself (TS7023), continues with its error type and
//! infers everything else as usual. A recovery is therefore an ANSWER, kept
//! apart from every other reason a position has no modelled value: it rides
//! `Opaque(QueryError::CheckerRecovery)` with a certified basis, reads as
//! `any`, and leaves the result complete. A position this substrate cannot
//! decide (an unresolved receiver, an open surface, an unmodelled form, a
//! budget, a cancellation) never reaches these rails: it keeps its typed gap.

use std::sync::Arc;

use verter_session_query::flow::frame_span::FrameSpan;
use verter_session_query::flow::slice::{SliceCall, SliceExpr};

use super::{CallValue, FlowDemandDriver, FlowEvaluator, InArmPresence, Positional};
use crate::semantic_query::checker_policy;
use crate::semantic_query::{
    CheckerDiagnosticCode, CheckerDiagnosticOccurrence, PrimitiveKind, QueryError,
    SemanticNodeData, SemanticNodeId,
};

impl<D: FlowDemandDriver> FlowEvaluator<'_, '_, D> {
    /// Whether a member walk's `read` found nothing: no answer, or the typed
    /// miss.
    pub(super) fn member_read_missed(&self, read: Option<SemanticNodeId>) -> bool {
        read.is_none_or(|node| {
            matches!(
                self.dispatch.arena().node_data(node).as_deref(),
                Some(SemanticNodeData::Opaque(QueryError::Miss))
            )
        })
    }

    /// Whether a read of `name` off `base`, which the shared member walk
    /// missed, is the checker's TS2339: `base` proves it absent. The walk
    /// already read the apparent members, so all that is left to prove is
    /// that no constituent the checker reads declares it, an index signature
    /// included. A constituent that does not settle proves nothing, and a
    /// composite needs every arm: an arm that declares the member may be the
    /// one a narrowing this rail does not model selects.
    pub(super) fn member_proven_absent(&mut self, base: SemanticNodeId, name: &str) -> bool {
        let base = self.dispatch.type_parameter_apparent_receiver(base);
        // A class's polymorphic `this` reads its instance.
        if let Some(SemanticNodeData::TypeParam {
            constraint: Some(instance),
            ..
        }) = self.dispatch.arena().node_data(base).as_deref()
        {
            if *instance != base {
                return self.member_proven_absent(*instance, name);
            }
        }
        // A class reference reads from its declarations: its surface is
        // not to be had while one of its member bodies is being evaluated.
        let class = match self.dispatch.arena().node_data(base).as_deref() {
            Some(SemanticNodeData::DeclRef { identity }) => Some((identity.clone(), Vec::new())),
            Some(SemanticNodeData::InstantiationRef {
                base: identity,
                args,
            }) => Some((identity.clone(), args.iter().copied().collect())),
            _ => None,
        };
        if let Some((identity, args)) = class {
            if self.dispatch.class_proves_member_absent(
                &identity.canonical_id,
                identity.owner,
                &identity.decl_name,
                &args,
                name,
            ) {
                return !self.dispatch.global_object_declares(name, self.canonical);
            }
        }
        let concrete = match self.dispatch.unwrap_identity_carrier_for_relation(base) {
            super::super::relation::IdentityCarrierUnwrap::Concrete(concrete) => concrete,
            super::super::relation::IdentityCarrierUnwrap::Unresolvable => return false,
        };
        let data = self.dispatch.arena().node_data(concrete);
        match data.as_deref() {
            Some(SemanticNodeData::Union(arms)) => {
                let arms = arms.to_vec();
                drop(data);
                arms.iter().all(|arm| self.member_proven_absent(*arm, name))
            }
            // A primitive reads its wrapper, which the walk read, once the
            // program has one to read.
            Some(SemanticNodeData::Primitive(
                PrimitiveKind::String
                | PrimitiveKind::Number
                | PrimitiveKind::Boolean
                | PrimitiveKind::BigInt
                | PrimitiveKind::Symbol,
            ))
            | Some(SemanticNodeData::Literal(_)) => {
                drop(data);
                self.dispatch
                    .apparent_wrapper_settles(concrete, self.canonical)
            }
            // A callable reads the apparent `Function`, which the walk could
            // only miss when the program declares none.
            Some(SemanticNodeData::Signature { .. }) => false,
            Some(SemanticNodeData::Object(view))
                if !view.call_signatures().is_empty()
                    || !view.construct_signatures().is_empty() =>
            {
                false
            }
            _ => {
                drop(data);
                matches!(self.arm_in_presence(concrete, name), InArmPresence::Never)
            }
        }
    }

    /// The checker's error type for a read of the absent member `name` off
    /// `receiver`, reported at the member's name `span` when the read has
    /// one. `own_method` says `receiver` is an object literal's own type and
    /// the read belongs to one of its methods' return inference: the
    /// checker prints that type in the message, which needs the return being
    /// inferred, so the method's return is circular.
    pub(super) fn absent_member_recovery(
        &mut self,
        receiver: SemanticNodeId,
        name: &Arc<str>,
        span: Option<FrameSpan>,
        own_method: bool,
    ) -> SemanticNodeId {
        if let Some(span) = span {
            self.report_absent_member(receiver, name, span.to_absolute(self.anchor));
        }
        if own_method {
            self.circular_return.set(true);
        }
        self.dispatch
            .graph()
            .intern_node(checker_policy::checker_error_type(
                checker_policy::absent_member(),
            ))
    }

    /// Record TS2339 for `name` off `receiver`. A receiver this frame binds
    /// is still being built: its literal reports the read once it has its
    /// final type.
    fn report_absent_member(
        &mut self,
        receiver: SemanticNodeId,
        name: &Arc<str>,
        span: verter_span::Span,
    ) {
        if let Some(bound) = self
            .receiver
            .as_ref()
            .filter(|bound| bound.binder == receiver)
        {
            bound
                .absent_reads
                .borrow_mut()
                .push((Arc::clone(name), span));
            return;
        }
        // A class's polymorphic `this` prints as its class.
        let printed = match self.dispatch.arena().node_data(receiver).as_deref() {
            Some(SemanticNodeData::TypeParam {
                constraint: Some(instance),
                param_index: super::super::substitute::THIS_BINDER_INDEX,
                ..
            }) => *instance,
            _ => receiver,
        };
        if let Some(printed) = self.dispatch.print_checker_type(printed) {
            self.record_member_diagnostic(name, &printed, span);
        }
    }

    /// Report each `this` member read of the body the return did not select
    /// and its receiver proves absent, as the checker's check of the whole
    /// body does. No value depends on these reads.
    pub(super) fn report_receiver_reads(
        &mut self,
        reads: Option<&verter_session_query::flow::slice::SliceReceiverReads>,
    ) {
        use verter_session_query::flow::slice::SliceThis;
        let Some(reads) = reads else {
            return;
        };
        if matches!(reads.this, SliceThis::Receiver) && self.receiver.is_none() {
            return;
        }
        let degradation = self.degradation;
        let receiver = self.eval_this(&reads.this);
        self.degradation = degradation;
        let Positional::Value(receiver) = receiver else {
            return;
        };
        let literal_value = matches!(reads.this, SliceThis::Value { .. });
        for (name, span) in reads.reads.iter() {
            let absent = if literal_value {
                !self.dispatch.global_object_declares(name, self.canonical)
            } else {
                // A receiver a contextual type supplies is read only where
                // the return reads it: its members can be bodies of their own.
                self.receiver
                    .as_ref()
                    .is_some_and(|bound| bound.binder == receiver && bound.base.is_none())
                    && self.receiver_lacks_member(receiver, name)
                    && !self.dispatch.global_object_declares(name, self.canonical)
            };
            if absent {
                self.report_absent_member(receiver, name, span.to_absolute(self.anchor));
            }
        }
        self.degradation = degradation;
    }

    /// Record TS2339 for `name` off the type the checker prints as `printed`.
    pub(super) fn record_member_diagnostic(
        &mut self,
        name: &str,
        printed: &str,
        span: verter_span::Span,
    ) {
        // A literal evaluated again reports the one read once.
        if self.checker_diagnostics.iter().any(|diagnostic| {
            diagnostic.start() == span.start
                && diagnostic.code() == CheckerDiagnosticCode::PropertyDoesNotExist
        }) {
            return;
        }
        self.record_checker_diagnostic(CheckerDiagnosticOccurrence::new(
            span.start,
            span.end,
            checker_policy::absent_member(),
            Box::new([name.into(), printed.into()]),
            Box::default(),
        ));
    }

    /// The call a `return` of `argument` makes of the function by its own
    /// declared name, awaited or not: the one re-entry the checker skips. A
    /// function a variable holds reads that variable, whose type needs the
    /// return, however the call is spelled.
    pub(super) fn return_of_bare_self_call(
        &self,
        argument: &SliceExpr,
    ) -> Option<verter_span::Span> {
        let declaration = self.self_slot.is_some_and(|slot| {
            matches!(
                slot.function.function_part,
                verter_type_expr::facts::FunctionPartIdentity::DeclarationBody
            )
        });
        let call = match argument {
            SliceExpr::Awaited { operand } => operand,
            other => other,
        };
        match call {
            SliceExpr::Call(SliceCall::DirectSelf, site, _) if declaration => Some(site.span()),
            _ => None,
        }
    }

    /// Whether a chain rooted at `root`, evaluated to `node`, reads an object
    /// literal's own `this`.
    #[inline(never)]
    pub(super) fn reads_own_literal(&self, root: &SliceExpr, node: SemanticNodeId) -> bool {
        match root {
            SliceExpr::This(verter_session_query::flow::slice::SliceThis::Value { .. }) => true,
            SliceExpr::This(_) => self
                .receiver
                .as_ref()
                .is_some_and(|bound| bound.infer_literal_members && bound.binder == node),
            _ => false,
        }
    }

    /// The checker's answer for a first link a declared literal's lowering
    /// found no member for, unless the global `Object` type supplies it.
    #[inline(never)]
    pub(super) fn undeclared_literal_member(
        &mut self,
        receiver: SemanticNodeId,
        links: &[(Arc<str>, bool)],
        name_spans: &[FrameSpan],
    ) -> Option<SemanticNodeId> {
        let (name, _) = links.first()?;
        if self.dispatch.global_object_declares(name, self.canonical) {
            return None;
        }
        let own_method = !self.frame_declares_return;
        Some(self.absent_member_recovery(receiver, name, name_spans.first().copied(), own_method))
    }

    /// A callee its object proves absent: the checker's TS2339, and a call
    /// of its error type is untyped.
    #[inline(never)]
    pub(super) fn recovered_member_callee(
        &mut self,
        callee: &verter_type_expr::TypeExpr,
        binding: Option<&super::FlowProductSubject>,
        member_spans: &[FrameSpan],
    ) -> Option<Positional<CallValue>> {
        let recovered = self.recovered_typeof_path(callee, binding, member_spans)?;
        Some(Positional::Value(CallValue::of_resolved_call(
            self.dispatch,
            recovered,
        )))
    }

    /// The value of a call that re-enters the return `target` still being
    /// inferred: the checker's error type, and every return from `target`
    /// inward closes as it.
    pub(super) fn circular_call(
        &mut self,
        target: &crate::semantic_query::FlowReturnKey,
    ) -> Positional<CallValue> {
        self.dispatch.mark_return_cycle(target);
        let recovery = self.circular_return_recovery();
        Positional::Value(CallValue::of_resolved_call(self.dispatch, recovery))
    }

    /// The checker's error type for a circular return.
    pub(super) fn circular_return_recovery(&self) -> SemanticNodeId {
        self.dispatch
            .graph()
            .intern_node(checker_policy::checker_error_type(
                checker_policy::circular_return(),
            ))
    }

    /// Whether `signature` returns the checker's error type for a circular
    /// return.
    pub(super) fn returns_circularly(&self, signature: SemanticNodeId) -> bool {
        let return_type = match self.dispatch.arena().node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature { return_type, .. }) => *return_type,
            _ => return false,
        };
        matches!(
            self.dispatch.arena().node_data(return_type).as_deref(),
            Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. }))
                if diagnostic.code == CheckerDiagnosticCode::CircularReturn
        )
    }

    /// The occurrence of the checker's TS7023 for the function it reports by
    /// `name`, under `noImplicitAny`.
    pub(super) fn circular_return_occurrence(
        &self,
        name: &(verter_span::Span, Arc<str>),
    ) -> Option<CheckerDiagnosticOccurrence> {
        self.no_implicit_any.then(|| {
            let diagnostic = checker_policy::circular_return();
            verter_debug_assert::verter_debug_assert!(
                diagnostic.code == CheckerDiagnosticCode::CircularReturn,
                "the circular return's own code"
            );
            CheckerDiagnosticOccurrence::new(
                name.0.start,
                name.0.end,
                diagnostic,
                Box::new([name.1.as_ref().into()]),
                Box::default(),
            )
        })
    }
}
