//! Inference fixation: the bindings a collecting session's candidates fix
//! (the checker's `getInferredType`), shared by a call's applicability and
//! a generic signature's instantiation in the context of another.

use rustc_hash::FxHashMap;

use super::super::dispatch_txn::FixationInput;
use super::super::ProjectSemanticDispatch;
use crate::semantic_query::{
    PrimitiveKind, ResolveCallFailure, SemanticNodeData, SemanticNodeId, TypeParamDecl,
    VariancePhase,
};

impl ProjectSemanticDispatch<'_> {
    /// Each type parameter's fixed binding from its session `inputs`, in
    /// declaration order, and the positions no candidate inferred. `accepts`
    /// decides whether a fallback satisfies its constraint; `None` from it
    /// is an undecided relation, which takes the constraint.
    pub(in crate::project_semantic_dispatch) fn fix_inference_inputs(
        &self,
        inputs: Vec<FixationInput>,
        type_params: &[TypeParamDecl],
        mut accepts: impl FnMut(
            &Self,
            SemanticNodeId,
            SemanticNodeId,
        ) -> Result<Option<bool>, ResolveCallFailure>,
    ) -> Result<(Vec<crate::semantic_query::InferBinding>, Vec<usize>), ResolveCallFailure> {
        let graph = self.graph();
        let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        let defaults: FxHashMap<SemanticNodeId, Option<SemanticNodeId>> = type_params
            .iter()
            .map(|decl| (decl.param, decl.default))
            .collect();
        let constraints: FxHashMap<SemanticNodeId, Option<SemanticNodeId>> = type_params
            .iter()
            .map(|decl| (decl.param, decl.constraint))
            .collect();
        // Fixation forms PROVISIONAL bindings for ALL parameters first: an
        // INFERRED parameter combines its winning candidate rung; an
        // UNINFERRED one starts at its default — substituted through the
        // already-fixed prefix (a default references prior siblings only,
        // TS2744) — else `unknown`. The uninferred parameters' CONSTRAINT
        // fallbacks then solve under the FULL substitution: each sweep
        // re-substitutes every uninferred parameter's default and
        // constraint through the complete current solution, so a
        // constraint referencing a FORWARD sibling — or the parameter
        // itself — resolves to that sibling's fixed bound instead of a
        // naked binder (TypeScript's `getInferredType`: `<T extends
        // string>` with nothing to infer from is `string`, not `unknown`;
        // a default that satisfies the constraint still wins). A mutually
        // dependent clause that does not converge within the
        // clause-bounded sweep budget is the typed `Undecidable`. The
        // relation runs with binding DISABLED, so the clamp deposits
        // nothing back into the session.
        let mut fixed: Vec<crate::semantic_query::InferBinding> = Vec::with_capacity(inputs.len());
        let mut uninferred_positions: Vec<usize> = Vec::new();
        let clause_len = inputs.len();
        for (position, input) in inputs.into_iter().enumerate() {
            let bound = if !input.candidates.is_empty() {
                match input.variance {
                    VariancePhase::Covariant => self.widened_covariant_inference(
                        self.call_common_supertype(&input.candidates)
                            .unwrap_or_else(|| {
                                self.relation_combine_candidates(&input.candidates, input.variance)
                            }),
                    ),
                    _ => self.relation_combine_candidates(&input.candidates, input.variance),
                }
            } else {
                uninferred_positions.push(position);
                defaults
                    .get(&input.param)
                    .and_then(|default| *default)
                    .map(|default| self.substitute_bindings(default, &fixed))
                    .unwrap_or(unknown)
            };
            fixed.push(crate::semantic_query::InferBinding {
                param: input.param,
                name: input.name,
                bound,
            });
        }
        if !uninferred_positions.is_empty() {
            let mut converged = false;
            // bounded-loop: at most clause-length + 1 constraint-solve sweeps; non-convergence is the typed `Undecidable` below.
            for _ in 0..=clause_len {
                let mut changed = false;
                for &position in &uninferred_positions {
                    let param = fixed[position].param;
                    let fallback = defaults
                        .get(&param)
                        .and_then(|default| *default)
                        .map(|default| self.substitute_bindings(default, &fixed))
                        .unwrap_or(unknown);
                    let bound = match constraints.get(&param).and_then(|bound| *bound) {
                        Some(constraint) => {
                            let constraint = self.substitute_bindings(constraint, &fixed);
                            match accepts(self, fallback, constraint)? {
                                Some(true) => fallback,
                                Some(false) | None => constraint,
                            }
                        }
                        None => fallback,
                    };
                    if fixed[position].bound != bound {
                        fixed[position].bound = bound;
                        changed = true;
                    }
                }
                if !changed {
                    converged = true;
                    break;
                }
            }
            if !converged {
                return Err(ResolveCallFailure::Undecidable);
            }
        }
        Ok((fixed, uninferred_positions))
    }
}
