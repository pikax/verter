//! The effects signature of a statement call the content half could not
//! settle alone ([`crate::flow_slice_content::SliceStatement::CallEffect`]).

use super::{FlowCallEvidence, FlowEvaluator, Positional};
use crate::flow_slice_content::{
    SliceCallArguments, SliceCallSite, SliceEffectCallee, SliceNarrowSubject,
};
use crate::semantic_query::{
    FlowGap, FlowReturnDegradation, PrimitiveKind, SemanticNodeData, SemanticNodeId,
    SignatureReturnCarrier,
};

/// What one call signature of an effects callee says about the path.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SignatureEffect {
    /// A declared return that is not `never`, and no assertion.
    None,
    /// A declared `never` return.
    Diverges,
    /// An assertion, or a return not declared (read from a body).
    Unread,
}

impl FlowEvaluator<'_, '_> {
    /// Settle one statement call's effects signature, the checker's
    /// `getEffectsSignature`: a callee's lone non-generic call signature,
    /// or — when some signature asserts or returns `never` — the signature
    /// the call resolves (`getResolvedSignature`). A `never` return ends
    /// the path, and a signature that neither asserts nor returns `never`
    /// leaves it unchanged; either discharges the call. An asserting
    /// signature, a body-derived return, a type whose signatures are not
    /// read here and a call that does not resolve take the typed
    /// guard-narrowing gap. Returns whether the path goes on.
    pub(super) fn settle_call_effect(
        &mut self,
        callee: &SliceEffectCallee,
        site: SliceCallSite,
    ) -> bool {
        let node = match callee {
            SliceEffectCallee::Declared(subject) => self.declared_callee_node(subject),
            SliceEffectCallee::Value(expr) => match self.eval_expr(expr) {
                Positional::Value(node) => Some(node),
                Positional::Hold | Positional::Unmodeled => None,
            },
        };
        match node.and_then(|node| self.effects_signature_diverges(node, site)) {
            Some(diverges) => {
                self.call_evidence.push(FlowCallEvidence {
                    span: site.span(),
                    relations_decided: true,
                });
                !diverges
            }
            None => {
                self.record_degradation(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing));
                true
            }
        }
    }

    /// The declared type of a static member path under an annotated
    /// parameter or local — what `getTypeOfDottedName` reads, never a
    /// narrowed value.
    fn declared_callee_node(&mut self, subject: &SliceNarrowSubject) -> Option<SemanticNodeId> {
        let root = SliceNarrowSubject {
            root: subject.root.clone(),
            path: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        };
        let declared = self.target_declared_node(&root)?;
        if subject.path.is_empty() {
            return Some(declared);
        }
        self.project_member_path(declared, &subject.path)
    }

    /// Whether the effects signature of a call of `callee` at `site`
    /// returns `never`; `None` when that signature is not one this
    /// evaluator reads (an assertion, a body-derived return, an unsettled
    /// signature list, a call that does not resolve).
    fn effects_signature_diverges(
        &mut self,
        callee: SemanticNodeId,
        site: SliceCallSite,
    ) -> Option<bool> {
        let Ok((calls, _)) = self.dispatch.shared_signature_buckets(callee) else {
            return None;
        };
        let effects: Vec<(SignatureEffect, bool)> = calls
            .iter()
            .map(|signature| self.signature_effect(*signature))
            .collect::<Option<_>>()?;
        if effects
            .iter()
            .all(|(effect, _)| *effect == SignatureEffect::None)
        {
            return Some(false);
        }
        if effects
            .iter()
            .any(|(effect, _)| *effect == SignatureEffect::Unread)
        {
            return None;
        }
        // The lone non-generic signature is the effects signature; any
        // other set with a `never` signature is the one the call resolves.
        if let [(effect, false)] = effects.as_slice() {
            return Some(*effect == SignatureEffect::Diverges);
        }
        match self.resolve_call_step(callee, site, &SliceCallArguments::none())? {
            Positional::Value(
                crate::project_semantic_dispatch::call_resolve::ResolveCallStep::Complete(
                    crate::semantic_query::ResolvedCallResult::Selected { return_type, .. },
                ),
            ) => Some(matches!(
                self.dispatch.graph().node_data(return_type).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Never))
            )),
            _ => None,
        }
    }

    /// One call signature's effect, and whether it is generic.
    fn signature_effect(&self, signature: SemanticNodeId) -> Option<(SignatureEffect, bool)> {
        let graph = self.dispatch.graph();
        let data = graph.node_data(signature)?;
        let SemanticNodeData::Signature {
            return_type,
            return_carrier,
            predicate,
            type_parameters,
            ..
        } = &*data
        else {
            return None;
        };
        let effect = if !matches!(return_carrier, SignatureReturnCarrier::Declared(_))
            || predicate
                .as_ref()
                .is_some_and(|predicate| predicate.asserts)
        {
            SignatureEffect::Unread
        } else if matches!(
            graph.node_data(*return_type).as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Never))
        ) {
            SignatureEffect::Diverges
        } else {
            SignatureEffect::None
        };
        Some((effect, !type_parameters.is_empty()))
    }
}
