//! Inference fixation: the candidates of the strongest priority a variable
//! received, and the type they fix it to (the checker's `getInferredType`),
//! shared by a conditional's `infer` declarations, a reverse mapped type, a
//! call's applicability and a generic signature's instantiation in the
//! context of another.

use rustc_hash::FxHashMap;

use super::super::dispatch_txn::{FixationInput, InferenceCandidate, RelationStep};
use super::super::ProjectSemanticDispatch;
use crate::semantic_query::{
    inference_candidate_precedence, FreshnessKey, InferenceCandidatePriority, PrimitiveKind,
    RelationKind, ResolveCallFailure, SemanticNodeData, SemanticNodeId, TypeParamDecl,
    VariancePhase,
};

/// The candidates of the strongest priority a variable received, by the
/// variance of the position each came from (the checker's `candidates` and
/// `contraCandidates` of one inference: a stronger priority replaces both
/// lists, an equal one adds to them).
#[derive(Debug, Clone, Default)]
pub(crate) struct WinningCandidates {
    /// The priority the candidates came at; `None` when there are none.
    pub(crate) priority: Option<InferenceCandidatePriority>,
    /// The candidates from covariant (and invariant) positions.
    pub(crate) covariant: Vec<SemanticNodeId>,
    /// The candidates from contravariant positions.
    pub(crate) contravariant: Vec<SemanticNodeId>,
}

impl WinningCandidates {
    /// Whether the variable received no candidate.
    pub(crate) fn is_empty(&self) -> bool {
        self.covariant.is_empty() && self.contravariant.is_empty()
    }

    /// The candidates a variable outside a signature fixes from, and how
    /// they combine (`getTypeFromInference`): its covariant candidates,
    /// unioned, when it has any, else its contravariant ones, intersected.
    pub(crate) fn inferred_from(&self) -> (&[SemanticNodeId], VariancePhase) {
        if self.covariant.is_empty() {
            (&self.contravariant, VariancePhase::Contravariant)
        } else {
            (&self.covariant, VariancePhase::Covariant)
        }
    }
}

/// The strongest-priority candidates of `candidates`.
pub(crate) fn winning_candidates(candidates: &[InferenceCandidate]) -> WinningCandidates {
    let Some(priority) = candidates
        .iter()
        .map(|candidate| candidate.priority)
        .max_by_key(|priority| inference_candidate_precedence(*priority))
    else {
        return WinningCandidates::default();
    };
    let mut winning = WinningCandidates {
        priority: Some(priority),
        ..WinningCandidates::default()
    };
    for candidate in candidates
        .iter()
        .filter(|candidate| candidate.priority == priority)
    {
        match candidate.variance {
            VariancePhase::Contravariant => winning.contravariant.push(candidate.node),
            VariancePhase::Covariant | VariancePhase::Invariant => {
                winning.covariant.push(candidate.node);
            }
        }
    }
    winning
}

/// A decided relation, `None` when it is not.
fn decided(step: RelationStep) -> Option<bool> {
    match step {
        RelationStep::Assignable { .. } => Some(true),
        RelationStep::NotAssignable => Some(false),
        _ => None,
    }
}

impl ProjectSemanticDispatch<'_> {
    pub(in crate::project_semantic_dispatch) fn fix_inference_inputs(
        &self,
        inputs: Vec<super::super::dispatch_txn::FixationInput>,
        type_params: &[crate::semantic_query::TypeParamDecl],
        accepts: impl FnMut(
            &super::InferenceTxn<'_, Self>,
            SemanticNodeId,
            SemanticNodeId,
        ) -> Result<Option<bool>, crate::semantic_query::ResolveCallFailure>,
    ) -> Result<
        (Vec<crate::semantic_query::InferBinding>, Vec<usize>),
        crate::semantic_query::ResolveCallFailure,
    > {
        super::InferenceTxn::new(self).fix_inference_inputs(inputs, type_params, accepts)
    }
}
impl<D: super::InferenceDemandDriver> super::InferenceTxn<'_, D> {
    /// Each type parameter's fixed binding from its session `inputs`, in
    /// declaration order, and the positions no candidate inferred. `accepts`
    /// decides whether one type is assignable to another; `None` from it
    /// is an undecided relation, which takes the constraint where a
    /// fallback is weighed against one and leaves the call undecided where
    /// an inference is chosen by it.
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
        // The covariant candidates of every parameter, read by a parameter
        // another is constrained to.
        let covariant_of: FxHashMap<SemanticNodeId, Vec<SemanticNodeId>> = inputs
            .iter()
            .map(|input| (input.param, input.candidates.covariant.clone()))
            .collect();
        // Fixation forms PROVISIONAL bindings for ALL parameters first: an
        // INFERRED parameter takes the type its candidates infer; an
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
                let dependents: Vec<&Vec<SemanticNodeId>> = type_params
                    .iter()
                    .filter(|decl| {
                        decl.param != input.param && decl.constraint == Some(input.param)
                    })
                    .filter_map(|decl| covariant_of.get(&decl.param))
                    .collect();
                self.signature_inferred_type(&input.candidates, &dependents, &mut accepts)?
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

    /// The type a signature's type parameter infers from its `winning`
    /// candidates (`getInferredType` with a signature): the covariant
    /// inference — their common supertype, widened — or the contravariant
    /// one. The covariant inference is preferred when it is neither `never`
    /// nor `any`, is assignable to some contravariant candidate, and every
    /// covariant candidate of a parameter constrained to this one
    /// (`dependents`) is assignable to it; `mix(n, (x: number) => {})` with
    /// `n: 1 | 2` infers `1 | 2`, `mix(s, (x: "a") => {})` with `s: string`
    /// infers `"a"`. An undecided relation leaves the call undecided.
    fn signature_inferred_type(
        &self,
        winning: &WinningCandidates,
        dependents: &[&Vec<SemanticNodeId>],
        accepts: &mut impl FnMut(
            &Self,
            SemanticNodeId,
            SemanticNodeId,
        ) -> Result<Option<bool>, ResolveCallFailure>,
    ) -> Result<SemanticNodeId, ResolveCallFailure> {
        let covariant = (!winning.covariant.is_empty()).then(|| {
            self.widened_covariant_inference(
                self.call_common_supertype(&winning.covariant)
                    .unwrap_or_else(|| {
                        self.relation_combine_candidates(
                            &winning.covariant,
                            VariancePhase::Covariant,
                        )
                    }),
            )
        });
        if winning.contravariant.is_empty() {
            return covariant.ok_or(ResolveCallFailure::Undecidable);
        }
        let contravariant = self.contravariant_inference(winning)?;
        let Some(covariant) = covariant else {
            return Ok(contravariant);
        };
        if matches!(
            self.graph().node_data(covariant).as_deref(),
            Some(SemanticNodeData::Primitive(
                PrimitiveKind::Never | PrimitiveKind::Any
            ))
        ) {
            return Ok(contravariant);
        }
        // `some(contraCandidates, t => covariant assignable to t)`.
        let mut some = Some(false);
        for candidate in &winning.contravariant {
            match accepts(self, covariant, *candidate)? {
                Some(true) => {
                    some = Some(true);
                    break;
                }
                Some(false) => {}
                None => some = None,
            }
        }
        match some {
            Some(false) => return Ok(contravariant),
            None => return Err(ResolveCallFailure::Undecidable),
            Some(true) => {}
        }
        for candidate in dependents.iter().flat_map(|candidates| candidates.iter()) {
            match accepts(self, *candidate, covariant)? {
                Some(true) => {}
                Some(false) => return Ok(contravariant),
                None => return Err(ResolveCallFailure::Undecidable),
            }
        }
        Ok(covariant)
    }

    /// The contravariant inference of a signature's type parameter
    /// (`getContravariantInference`): the intersection of its contravariant
    /// candidates at a priority that implies combination (the return
    /// type's), else their common subtype — the last candidate that is a
    /// subtype of the one kept before it, reading left to right
    /// (`co((x: { a: 1 }) => {}, (x: { b: 2 }) => {})` infers `{ a: 1 }`).
    fn contravariant_inference(
        &self,
        winning: &WinningCandidates,
    ) -> Result<SemanticNodeId, ResolveCallFailure> {
        if winning.priority == Some(InferenceCandidatePriority::ReturnType) {
            return Ok(self.relation_combine_candidates(
                &winning.contravariant,
                VariancePhase::Contravariant,
            ));
        }
        let Some((&first, rest)) = winning.contravariant.split_first() else {
            return Err(ResolveCallFailure::Undecidable);
        };
        let mut kept = first;
        for &candidate in rest {
            let binding_guard = self.binding.disable();
            let step = self.execute_relate_pair_kind(candidate, kept, RelationKind::Subtype);
            drop(binding_guard);
            match decided(step) {
                Some(true) => kept = candidate,
                Some(false) => {}
                None => return Err(ResolveCallFailure::Undecidable),
            }
        }
        Ok(kept)
    }

    /// A covariant inference as the checker widens it (`getWidenedType` in
    /// `getCovariantInference`): without `strictNullChecks` `null` and
    /// `undefined` widen to `any`, so `id(null)` is `any`.
    fn widened_covariant_inference(&self, bound: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        if !self.relation_strict_config().strict_null_checks
            && matches!(
                graph.node_data(bound).as_deref(),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::Null | PrimitiveKind::Undefined
                ))
            )
        {
            return graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
        }
        bound
    }

    /// A call's covariant inference from several candidates (the checker's
    /// `getCommonSupertype` in `getCovariantInference`): the leftmost
    /// candidate no later one is a supertype of, with each candidate's
    /// `null` / `undefined` set aside under `strictNullChecks` and added
    /// back to the answer — `takes(u)` over `((x: unknown) => x is A) |
    /// ((x: unknown) => x is B)` infers `A`, where a conditional type's
    /// `infer` unions its candidates. Literals of one base primitive
    /// union (`"a" | "b"`). `None` — the union the caller falls back to
    /// — when a candidate is an object literal's fresh type, an array or a
    /// tuple (the checker first unions object and array LITERAL candidates,
    /// a provenance an array node does not carry) or a subtype relation is
    /// undecided. A declared object type is an ordinary candidate: `two(x,
    /// y)` over `A` and `B` infers `A`.
    fn call_common_supertype(&self, candidates: &[SemanticNodeId]) -> Option<SemanticNodeId> {
        let graph = self.graph();
        let mut ordered: Vec<SemanticNodeId> = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            if !ordered
                .iter()
                .any(|kept| self.nodes_provably_equal(*kept, *candidate))
            {
                ordered.push(*candidate);
            }
        }
        if let [only] = ordered.as_slice() {
            return Some(*only);
        }
        let strict = self.relation_strict_config().strict_null_checks;
        let is_nullish = |node: SemanticNodeId| {
            matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::Null | PrimitiveKind::Undefined
                ))
            )
        };
        let mut nullish: Vec<SemanticNodeId> = Vec::new();
        let mut primary: Vec<SemanticNodeId> = Vec::with_capacity(ordered.len());
        for candidate in &ordered {
            let arms: Vec<SemanticNodeId> = match graph.node_data(*candidate).as_deref() {
                Some(SemanticNodeData::Union(arms)) => arms.iter().copied().collect(),
                _ => vec![*candidate],
            };
            if arms
                .iter()
                .any(|arm| match graph.node_data(*arm).as_deref() {
                    // An object LITERAL's candidate is fresh; a declared object
                    // type's is not, and takes part like any other.
                    Some(SemanticNodeData::Object(_)) => {
                        self.freshness_for_source_node(*arm) == FreshnessKey::Fresh
                    }
                    Some(
                        SemanticNodeData::Array { .. }
                        | SemanticNodeData::Tuple { .. }
                        | SemanticNodeData::ObjectSpreadProgram(_),
                    ) => true,
                    _ => false,
                })
            {
                return None;
            }
            if strict && arms.iter().any(|arm| is_nullish(*arm)) {
                let kept: Vec<SemanticNodeId> = arms
                    .iter()
                    .copied()
                    .filter(|arm| !is_nullish(*arm))
                    .collect();
                nullish.extend(arms.iter().copied().filter(|arm| is_nullish(*arm)));
                if kept.is_empty() {
                    continue;
                }
                primary.push(self.intern_normalized_union_or_intersection(&kept, true));
            } else {
                primary.push(*candidate);
            }
        }
        let literal_base = |node: SemanticNodeId| match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Literal(value)) => Some(std::mem::discriminant(value)),
            _ => None,
        };
        let supertype = match primary.as_slice() {
            [] => None,
            [first, rest @ ..]
                if literal_base(*first).is_some()
                    && rest
                        .iter()
                        .all(|other| literal_base(*other) == literal_base(*first)) =>
            {
                Some(self.intern_normalized_union_or_intersection(&primary, true))
            }
            [first, rest @ ..] => {
                let mut supertype = *first;
                for candidate in rest {
                    match self.execute_relate_pair_kind(
                        supertype,
                        *candidate,
                        crate::semantic_query::RelationKind::Subtype,
                    ) {
                        RelationStep::Assignable { .. } => supertype = *candidate,
                        RelationStep::NotAssignable => {}
                        _ => return None,
                    }
                }
                Some(supertype)
            }
        };
        let mut members: Vec<SemanticNodeId> = supertype.into_iter().collect();
        members.extend(nullish);
        match members.as_slice() {
            [] => None,
            [only] => Some(*only),
            _ => Some(self.intern_normalized_union_or_intersection(&members, true)),
        }
    }
}
