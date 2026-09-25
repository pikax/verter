//! The effects signature of a statement call the content half could not
//! settle alone ([`crate::flow_slice_content::SliceStatement::CallEffect`]).

use super::{FlowCallEvidence, FlowEvaluator, Positional};
use crate::flow_slice_content::{SliceEffectCallee, SliceNarrowSubject};
use crate::semantic_query::{
    FlowGap, FlowReturnDegradation, PrimitiveKind, SemanticNodeData, SemanticNodeId,
    SignatureReturnCarrier,
};

impl FlowEvaluator<'_, '_> {
    /// Settle one statement call's effects signature, the checker's
    /// `getEffectsSignature`: an `asserts` signature narrows what follows
    /// and a `never` return ends the path, so a callee none of whose call
    /// signatures (`SignaturesOfType`) asserts or declares a `never`
    /// return leaves the path unchanged and discharges the call. Any other
    /// callee — one of those signatures, a body-derived return, a type
    /// whose signatures are not read here — takes the typed
    /// guard-narrowing gap.
    pub(super) fn settle_call_effect(
        &mut self,
        callee: &SliceEffectCallee,
        call: verter_span::Span,
    ) {
        let node = match callee {
            SliceEffectCallee::Declared(subject) => self.declared_callee_node(subject),
            SliceEffectCallee::Value(expr) => match self.eval_expr(expr) {
                Positional::Value(node) => Some(node),
                Positional::Hold | Positional::Unmodeled => None,
            },
        };
        if node.is_some_and(|node| self.signatures_neither_assert_nor_diverge(node)) {
            self.call_evidence.push(FlowCallEvidence {
                span: call,
                relations_decided: true,
            });
        } else {
            self.record_degradation(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing));
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

    /// Whether every call signature of `callee` is an authored signature
    /// that neither asserts nor returns `never`.
    fn signatures_neither_assert_nor_diverge(&mut self, callee: SemanticNodeId) -> bool {
        let Ok((calls, _)) = self.dispatch.shared_signature_buckets(callee) else {
            return false;
        };
        calls.iter().all(|signature| {
            let graph = self.dispatch.graph();
            let Some(data) = graph.node_data(*signature) else {
                return false;
            };
            let SemanticNodeData::Signature {
                return_type,
                return_carrier,
                predicate,
                ..
            } = &*data
            else {
                return false;
            };
            matches!(return_carrier, SignatureReturnCarrier::Declared(_))
                && !predicate
                    .as_ref()
                    .is_some_and(|predicate| predicate.asserts)
                && !matches!(
                    graph.node_data(*return_type).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Never))
                )
        })
    }
}
