//! [`FlowReturnResult`] — the SUCCESS carrier of a `FlowReturn` query, and
//! the derivation of its degradation verdict.
//!
//! STRUCTURAL CONFINEMENT. All three fields are PRIVATE to THIS module, so
//! [`FlowReturnResult::new`] is the only way to obtain one anywhere in the
//! crate — the parent `semantic_query` module included. That matters
//! because the constructor does not merely store the caller's degradation:
//! it walks the RESULT NODE and folds
//! [`FlowReturnDegradation::UnresolvedValue`] in when the value reaches a
//! semantic-miss carrier. Three independent admission gates (the family
//! memo's cold publish, the SCC batch publish, and the root build's
//! `cache_suppress`) decide warm-vs-`ReturnOnly` on that one field alone,
//! so a struct literal that set it to `None` over an unknown value would
//! be a warm-admitted lie at all three. A struct literal is
//! unrepresentable outside this file (`E0451`), and a post-construction
//! `result.return_type = …` is unrepresentable too (`E0616`) — the one
//! rebuild, [`FlowReturnResult::with_return_type`], re-derives.

use super::{authored_property_key_child, FlowReturnDegradation, SemanticNodeData, SemanticNodeId};

/// The SUCCESS carrier of a `FlowReturn` query — including DEGRADED
/// successes. A no-value failure and a usable degraded value are
/// different public outcomes: a degraded success (a usable result whose
/// evaluation substituted a modeled-`any` for something it could not
/// model) returns through THIS carrier with its typed
/// [`FlowReturnDegradation`] reason and defaults to `ReturnOnly` — it
/// never warms the family memo (a later explicit fact-rooted
/// admission-table row is the only thing that could change that; none
/// exists). Only a COMPLETE, non-degraded evaluation is admitted warm;
/// unsupported, missing, budgeted, cyclic-empty, torn, or otherwise
/// NO-VALUE results are typed `FlowReturnFailure`s through
/// `ReturnOnly`. The whole-return node is canonical and
/// carrier-preserving; consumers project or publish it afterward under
/// their own mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowReturnResult {
    /// The whole-function return type (widened join of every contributor).
    ///
    /// PRIVATE for the same reason as [`Self::degradation`]: the
    /// verdict is derived FROM this node, so a post-construction
    /// reassignment would leave a stale verdict attached to a value it
    /// was never taken over. [`Self::with_return_type`] is the one
    /// rebuild, and it re-derives.
    return_type: SemanticNodeId,
    /// Whether execution can reach the end of the body without a return.
    pub can_fall_through: bool,
    /// The typed degradation reason, when the evaluation produced a
    /// USABLE result through a modeled-`any` substitution, OR when the
    /// value carries an unresolved semantic carrier. `Some` gates the
    /// result to `ReturnOnly`; `None` is the warm-admissible arm.
    ///
    /// PRIVATE by construction. Every one of the three admission gates
    /// that reads this channel — the family memo's cold publish, the SCC
    /// batch publish, and the root build's `cache_suppress` — decides
    /// warm-vs-`ReturnOnly` on it alone, so a `None` over a value that is
    /// not actually known would be a warm-admitted lie at all three. The
    /// only way to obtain a `FlowReturnResult` is [`Self::new`], which
    /// derives this field from the RESULT NODE rather than accepting a
    /// caller's word for it. See [`FlowReturnDegradation::UnresolvedValue`].
    degradation: Option<FlowReturnDegradation>,
}

impl FlowReturnResult {
    /// THE construction point of a flow-return value.
    ///
    /// `degradation` is the evaluation's OWN observed reason (a
    /// modeled-`any` substitution it made knowingly). It is not the whole
    /// verdict: this constructor additionally inspects `return_type` and
    /// folds [`FlowReturnDegradation::UnresolvedValue`] in whenever the
    /// value REACHES a semantic-miss carrier
    /// ([`flow_return_value_is_unresolved`]). The evaluation's own reason
    /// wins when both apply (first-observed reason, deterministic).
    ///
    /// This is why the field is private: an evaluation cannot know, from
    /// the arms it took, whether some leaf lowering silently answered
    /// `Opaque(Miss)` three composition levels down. The node does know,
    /// and it is the node that admission publishes.
    #[must_use]
    pub(crate) fn new(
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        return_type: SemanticNodeId,
        can_fall_through: bool,
        degradation: Option<FlowReturnDegradation>,
    ) -> Self {
        let degradation = degradation.or_else(|| {
            flow_return_value_is_unresolved(graph, return_type)
                .then_some(FlowReturnDegradation::UnresolvedValue)
        });
        Self {
            return_type,
            can_fall_through,
            degradation,
        }
    }

    /// The whole-function return type.
    #[must_use]
    pub fn return_type(&self) -> SemanticNodeId {
        self.return_type
    }

    /// The typed degradation reason, or `None` for the warm-admissible arm.
    #[must_use]
    pub fn degradation(&self) -> Option<FlowReturnDegradation> {
        self.degradation
    }

    /// The same value with its RETURN TYPE replaced — the only
    /// field-level rebuild (the post-convergence literal widening in the
    /// SCC discharge), and it re-derives the verdict through
    /// [`Self::new`] rather than carrying the old one over.
    #[must_use]
    pub(crate) fn with_return_type(
        &self,
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        return_type: SemanticNodeId,
    ) -> Self {
        Self::new(graph, return_type, self.can_fall_through, self.degradation)
    }
}

/// Whether a flow-return VALUE reaches a semantic-miss carrier — a node
/// whose payload says "the represented type is not known"
/// (`QueryError::means_type_is_not_yet_known`).
///
/// A miss carrier is an honest LOCAL answer (the leaf really did resolve
/// to nothing), but it is not a complete RESULT: publishing it warm hands
/// an enclosing composition an opaque interior with no partial marker,
/// which is exactly what the degradation channel exists to prevent. So
/// the verdict is taken here, once, over the value the admission gates
/// are about to publish — never per-arm, where rounds of fixes close one
/// ingress and leave its siblings.
///
/// The walk descends the structure a flow evaluation COMPOSES or lowers
/// inline, and stops at every SHALLOW CARRIER (`DeclRef`,
/// `InstantiationRef`'s base, `BareRef`, `ImportType`, `TypeOf`,
/// `MergedDecl`). Descending a carrier would be materialisation, which
/// the shallow-by-default rule forbids — and a miss INSIDE a referenced
/// declaration is that declaration's own admission problem, gated by its
/// own query. Carrier TYPE ARGUMENTS are locally-supplied structure and
/// do descend, through the one sanctioned accessor.
///
/// The match is exhaustive with no wildcard: a new [`SemanticNodeData`]
/// variant does not compile until it is dispositioned as descend-or-stop
/// here.
fn flow_return_value_is_unresolved(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    root: SemanticNodeId,
) -> bool {
    /// Bound on the structure inspected. A flow-return value is composed
    /// from shallow carriers, so real answers are orders of magnitude
    /// under this; exceeding it means cleanliness could NOT be proved,
    /// and an unproven answer must not warm.
    const NODE_BUDGET: usize = 4096;

    let mut seen: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let mut stack: Vec<SemanticNodeId> = vec![root];
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        if seen.len() > NODE_BUDGET {
            return true;
        }
        let Some(data) = graph.node_data(node) else {
            // A node id the graph cannot resolve is not proof of a known
            // value.
            return true;
        };
        // The three structural carriers' arguments are locally-supplied
        // structure: they descend through the ONE sanctioned accessor
        // (the carriers' own heads do not).
        stack.extend_from_slice(data.carrier_type_args());
        match data.as_ref() {
            SemanticNodeData::Opaque(error) => {
                if error.means_type_is_not_yet_known() {
                    return true;
                }
            }
            // ── Composed / inline structure: descend ──────────────────
            SemanticNodeData::Alias(inner) => stack.push(*inner),
            SemanticNodeData::Object(surface) => {
                for member in surface.positive_members() {
                    stack.extend(authored_property_key_child(&member.key));
                    stack.push(member.value);
                }
                stack.extend_from_slice(&surface.call_signatures);
                stack.extend_from_slice(&surface.construct_signatures);
                for index in surface.index_signatures.iter() {
                    stack.push(index.key_type);
                    stack.push(index.value_type);
                }
                stack.extend(surface.keyspace);
            }
            SemanticNodeData::ObjectSpreadProgram(program) => stack.extend(program.child_nodes()),
            SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                stack.extend_from_slice(members);
            }
            SemanticNodeData::Array { element, .. } => stack.push(*element),
            SemanticNodeData::Tuple { elements, .. } => {
                stack.extend(elements.iter().map(|element| element.value));
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                stack.extend_from_slice(expressions);
            }
            SemanticNodeData::KeyOf { base } => stack.push(*base),
            SemanticNodeData::IndexedAccess { object, index } => {
                stack.push(*object);
                stack.extend(authored_property_key_child(index));
            }
            SemanticNodeData::Mapped { source, mapper } => {
                stack.push(*source);
                stack.push(mapper.key_space);
                stack.push(mapper.value_expr);
                stack.extend(mapper.name_remap);
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                stack.push(*check);
                stack.push(*extends);
                stack.push(*true_branch_ref);
                stack.push(*false_branch_ref);
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                ..
            } => {
                stack.extend(params.iter().map(|param| param.ty));
                stack.push(*return_type);
                for parameter in type_parameters.iter() {
                    stack.extend(parameter.constraint);
                    stack.extend(parameter.default);
                }
            }
            SemanticNodeData::InstantiationRef { args, .. } => stack.extend_from_slice(args),
            // ── Settled leaves and SHALLOW CARRIERS: stop ─────────────
            //
            // `Primitive` / `Literal` / `Infer` / `InferRef` /
            // `RawFallback` / `SyntheticBinding` are settled values.
            // `TypeParam` is a binder, and its constraint / default are
            // the DECLARATION's meaning, not this value's. `DeclRef`,
            // `InstantiationRef`'s base, `MergedDecl`, `BareRef`,
            // `ImportType` and `TypeOf` are shallow carriers: descending
            // one would materialise a referenced declaration, which the
            // shallow-by-default rule forbids, and a miss inside it is
            // that declaration's own admission problem.
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::TypeParam { .. }
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::MergedDecl { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::RawFallback { .. }
            | SemanticNodeData::SyntheticBinding { .. } => {}
        }
    }
    false
}
