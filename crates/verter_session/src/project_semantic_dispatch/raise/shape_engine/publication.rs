//! Algebra 4 of the shared shape engine: the PUBLICATION-SCORING facts —
//! `symbolic_carriers` / `generic_detail` / `structural_top_level` /
//! `exact_unknown_root` — folded over the SAME single [`super::fold_node`]
//! traversal the materializer (`MaterializeTypeExprAlg`) and the raised-facts
//! algebra (`RaisedFactsAlg`) ride.
//!
//! The publication finaliser ([`crate::meta_resolve::compare_node_improvement`])
//! picks the better of two candidate published-field shapes; it scores them in
//! NODE DOMAIN through [`project_node_publication_score`] (one facts-only fold
//! per node, NO `TypeExpr` allocation, NO structural-key interner). The
//! `&TypeExpr` front [`type_expr_publication_score`] feeds the SAME per-arm rules
//! ([`combine`]) so `compare_node_improvement` and
//! [`crate::meta_resolve::compare_type_expr_improvement`] share ONE formula:
//! there is no second, hand-maintained TypeExpr scorer to drift from.
//!
//! Because the node front rides [`super::fold_node`], a node's score is computed
//! over EXACTLY the shape the materializer produces (the same intersection
//! arm-drop + 0/1/many collapse, the same single-call-signature surface fast
//! path, the same `Mapped` source/value/name-type children — NEVER both `source`
//! AND `key_space`, the same `TypeParam` / `RawFallback` / `SyntheticBinding` /
//! `Opaque(RecursiveRef)` arms). So `project_node_publication_score(node)` equals
//! `type_expr_publication_score(raise(node))` per-fact, locked by the differential
//! oracle in [`super::super::super::raised_shape_tests`].

use std::sync::Arc;

use rustc_hash::FxHashSet;
#[cfg(test)]
use verter_type_expr::TypeExpr;
use verter_type_expr::{LiteralValue, MappedModifier, MemberVisibility, PrimitiveName};

use super::super::ProjectSemanticDispatch;
use super::{
    FactShapeTag, FoldedFunction, FoldedTupleElement, PublicationScore, RaisedShapeAlgebra,
};
use crate::resolver_core::component_meta_query_engine::SEMANTIC_OBJECT_SURFACE;
use crate::semantic_query::{QueryError, SemanticNodeId};

// The [`PublicationScore`] facts type is defined in the parent `shape_engine`
// module alongside `RaisedShapeFacts` / `NodeShapeEq` (so the one-hop crate
// re-export through `raise` resolves it); this child owns the algebra that folds
// it.

/// The publication score of a folded FUNCTION signature (the `RaisedShapeAlgebra::Fn`
/// for [`PublicationScoreAlg`]). A function carries no root-fact of its own here —
/// `function_out` stamps `structural_top_level = true` / `exact_unknown_root =
/// false` when the function becomes an `Out`.
#[derive(Debug, Clone, Copy)]
struct FnScore {
    symbolic_carriers: usize,
    generic_detail: usize,
}

/// The publication score contribution of one object member (the
/// `RaisedShapeAlgebra::Member` for [`PublicationScoreAlg`]).
#[derive(Debug, Clone, Copy)]
struct MemberScore {
    symbolic_carriers: usize,
    generic_detail: usize,
}

/// The node-domain `Out` of [`PublicationScoreAlg`]: the [`PublicationScore`]
/// plus the shared [`FactShapeTag`] so the fold's three structural inspections
/// (`is_object_surface_sentinel` / `is_empty_object` / `out_as_function`) decide
/// IDENTICALLY to the materializer — the intersection arm-drop + collapse and the
/// `ConstructorType` rewrap must match, or the score would be folded over a
/// different surviving-arm set than the materialized shape has.
#[derive(Debug, Clone, Copy)]
struct ScoredOut {
    score: PublicationScore,
    tag: FactShapeTag,
}

// ===========================================================================
// Shared per-arm scoring rules — the SINGLE source of the publication formula.
// BOTH the node-front algebra arms and the `&TypeExpr` front feed already-scored
// children into these, so the formula can never drift between the two fronts.
// Each function reproduces the CURRENT `count_symbolic_carriers_in_expr` /
// `count_generic_detail_in_expr` / `type_expr_has_structural_top_level` /
// `matches!(_, Unknown)` semantics EXACTLY.
// ===========================================================================
mod combine {
    use super::PublicationScore;

    /// Sum the `symbolic_carriers` + `generic_detail` of a child iterator.
    fn sum_children(children: impl Iterator<Item = PublicationScore>) -> (usize, usize) {
        children.fold((0, 0), |(sym, gen), child| {
            (sym + child.symbolic_carriers, gen + child.generic_detail)
        })
    }

    /// A concrete structural leaf (`Primitive` / `Literal`): no symbolic carrier,
    /// structural root, not unknown.
    pub(super) fn structural_leaf() -> PublicationScore {
        PublicationScore {
            symbolic_carriers: 0,
            generic_detail: 0,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }

    /// A symbolic leaf with NO scored children (`Infer` / `TypeOf` /
    /// `SyntheticSlotBinding`): one symbolic carrier, no generic detail,
    /// non-structural root, not unknown. (`TypeOf` is a leaf for scoring — its
    /// path/args are never counted, mirroring `count_symbolic_carriers_in_expr`.)
    pub(super) fn symbolic_leaf() -> PublicationScore {
        PublicationScore {
            symbolic_carriers: 1,
            generic_detail: 0,
            structural_top_level: false,
            exact_unknown_root: false,
        }
    }

    /// An `Unknown`-materialising leaf (`RawFallback`, every `Opaque(QueryError)`
    /// sentinel reaching the reverse boundary, and `TypeExpr::Unknown`): one
    /// symbolic carrier, non-structural root, AND the exact-unknown root clause.
    pub(super) fn unknown_leaf() -> PublicationScore {
        PublicationScore {
            symbolic_carriers: 1,
            generic_detail: 0,
            structural_top_level: false,
            exact_unknown_root: true,
        }
    }

    /// A `RecursiveRef` carrier: a symbolic carrier (`+1`) plus its type-argument
    /// carriers, a STRUCTURAL root, and NOT unknown (it raises to
    /// `TypeExpr::RecursiveRef`, never `Unknown`).
    pub(super) fn recursive_ref(args: impl Iterator<Item = PublicationScore>) -> PublicationScore {
        let (sym, gen) = sum_children(args);
        PublicationScore {
            symbolic_carriers: 1 + sym,
            generic_detail: gen,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }

    /// A reference carrier (`Ref` / `ImportType` / `BareRef` / `DeclPlaceholder`):
    /// a symbolic carrier (`+1`) plus its type-argument carriers; non-structural
    /// root; not unknown.
    pub(super) fn reference(args: impl Iterator<Item = PublicationScore>) -> PublicationScore {
        let (sym, gen) = sum_children(args);
        PublicationScore {
            symbolic_carriers: 1 + sym,
            generic_detail: gen,
            structural_top_level: false,
            exact_unknown_root: false,
        }
    }

    /// A `TypeParameter`: a symbolic carrier (`+1`; its constraint/default are NOT
    /// summed into `symbolic_carriers`, mirroring the leaf-group
    /// `count_symbolic_carriers_in_expr` arm), generic detail `+1` PLUS the
    /// constraint/default generic detail; non-structural root; not unknown.
    pub(super) fn type_parameter(
        constraint: Option<PublicationScore>,
        default: Option<PublicationScore>,
    ) -> PublicationScore {
        let generic_detail = 1
            + constraint.map_or(0, |c| c.generic_detail)
            + default.map_or(0, |d| d.generic_detail);
        PublicationScore {
            symbolic_carriers: 1,
            generic_detail,
            structural_top_level: false,
            exact_unknown_root: false,
        }
    }

    /// An `IndexedAccess`: a symbolic carrier (`+1`) plus object + index carriers;
    /// non-structural root.
    pub(super) fn indexed_access(
        object: PublicationScore,
        index: PublicationScore,
    ) -> PublicationScore {
        PublicationScore {
            symbolic_carriers: 1 + object.symbolic_carriers + index.symbolic_carriers,
            generic_detail: object.generic_detail + index.generic_detail,
            structural_top_level: false,
            exact_unknown_root: false,
        }
    }

    /// A `Conditional`: a symbolic carrier (`+1`) plus all four branch carriers;
    /// non-structural root.
    pub(super) fn conditional(
        check: PublicationScore,
        extends: PublicationScore,
        true_type: PublicationScore,
        false_type: PublicationScore,
    ) -> PublicationScore {
        let (sym, gen) = sum_children([check, extends, true_type, false_type].into_iter());
        PublicationScore {
            symbolic_carriers: 1 + sym,
            generic_detail: gen,
            structural_top_level: false,
            exact_unknown_root: false,
        }
    }

    /// A `Mapped`: a symbolic carrier (`+1`) plus source + value + name-type
    /// carriers — NEVER both `source` AND `key_space` (the fold passes ONE source
    /// child); non-structural root.
    pub(super) fn mapped(
        source: PublicationScore,
        value: PublicationScore,
        name_type: Option<PublicationScore>,
    ) -> PublicationScore {
        let mut children = vec![source, value];
        if let Some(name_type) = name_type {
            children.push(name_type);
        }
        let (sym, gen) = sum_children(children.into_iter());
        PublicationScore {
            symbolic_carriers: 1 + sym,
            generic_detail: gen,
            structural_top_level: false,
            exact_unknown_root: false,
        }
    }

    /// A `TemplateLiteral`: a symbolic carrier (`+1`) plus its expression
    /// carriers; STRUCTURAL root.
    pub(super) fn template_literal(
        expressions: impl Iterator<Item = PublicationScore>,
    ) -> PublicationScore {
        let (sym, gen) = sum_children(expressions);
        PublicationScore {
            symbolic_carriers: 1 + sym,
            generic_detail: gen,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }

    /// A `Union` / `Intersection`: the sum over members, no self-penalty;
    /// STRUCTURAL root.
    pub(super) fn union_or_intersection(
        members: impl Iterator<Item = PublicationScore>,
    ) -> PublicationScore {
        let (sym, gen) = sum_children(members);
        PublicationScore {
            symbolic_carriers: sym,
            generic_detail: gen,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }

    /// An `Array`: the element's carriers, no self-penalty; STRUCTURAL root.
    pub(super) fn array(element: PublicationScore) -> PublicationScore {
        PublicationScore {
            symbolic_carriers: element.symbolic_carriers,
            generic_detail: element.generic_detail,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }

    /// A `Tuple`: the sum over element types, no self-penalty; STRUCTURAL root.
    pub(super) fn tuple(elements: impl Iterator<Item = PublicationScore>) -> PublicationScore {
        let (sym, gen) = sum_children(elements);
        PublicationScore {
            symbolic_carriers: sym,
            generic_detail: gen,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }

    /// A `KeyOf`: the base's carriers, no self-penalty; STRUCTURAL root.
    pub(super) fn key_of(base: PublicationScore) -> PublicationScore {
        PublicationScore {
            symbolic_carriers: base.symbolic_carriers,
            generic_detail: base.generic_detail,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }

    /// A `Parenthesized` fidelity wrapper (`&TypeExpr` front only — the node fold
    /// peels `Alias` instead): the inner's carriers AND the inner's
    /// `structural_top_level` (it recurses, mirroring
    /// `type_expr_has_structural_top_level`), but exact-unknown root is FALSE
    /// (`matches!(Parenthesized(_), Unknown)` is false, so a `Parenthesized`
    /// wrapper is never an exact-unknown root even around an `Unknown`).
    #[cfg(test)]
    pub(super) fn parenthesized(inner: PublicationScore) -> PublicationScore {
        PublicationScore {
            symbolic_carriers: inner.symbolic_carriers,
            generic_detail: inner.generic_detail,
            structural_top_level: inner.structural_top_level,
            exact_unknown_root: false,
        }
    }

    /// A standalone `Rest` (`...T`; `&TypeExpr` front only): the inner's carriers,
    /// no self-penalty; STRUCTURAL root.
    #[cfg(test)]
    pub(super) fn rest(inner: PublicationScore) -> PublicationScore {
        PublicationScore {
            symbolic_carriers: inner.symbolic_carriers,
            generic_detail: inner.generic_detail,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }

    /// An `Object` surface from its member `(symbolic_carriers, generic_detail)`
    /// contributions; STRUCTURAL root. The empty object `{}` is the
    /// `members`-empty case (carriers `0`).
    pub(super) fn object(members: impl Iterator<Item = (usize, usize)>) -> PublicationScore {
        let (sym, gen) = members.fold((0, 0), |(sym, gen), (m_sym, m_gen)| {
            (sym + m_sym, gen + m_gen)
        });
        PublicationScore {
            symbolic_carriers: sym,
            generic_detail: gen,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }

    /// The representable empty object `{}`.
    pub(super) fn empty_object() -> PublicationScore {
        PublicationScore {
            symbolic_carriers: 0,
            generic_detail: 0,
            structural_top_level: true,
            exact_unknown_root: false,
        }
    }
}

/// Fold a FUNCTION signature's already-scored children into its [`FnScore`]:
/// `symbolic_carriers` is the sum over parameters + return; `generic_detail` adds
/// each declared type-parameter (`+1`) plus its constraint/default detail, then
/// the parameter + return detail. Mirrors the `Function` / `ConstructorType` arms
/// of `count_symbolic_carriers_in_expr` + `count_generic_detail_in_expr`.
fn function_score(
    parameters: impl Iterator<Item = PublicationScore>,
    return_type: Option<PublicationScore>,
    type_parameters: impl Iterator<Item = (Option<PublicationScore>, Option<PublicationScore>)>,
) -> FnScore {
    let mut symbolic_carriers = 0;
    let mut generic_detail = 0;
    for param in parameters {
        symbolic_carriers += param.symbolic_carriers;
        generic_detail += param.generic_detail;
    }
    if let Some(return_type) = return_type {
        symbolic_carriers += return_type.symbolic_carriers;
        generic_detail += return_type.generic_detail;
    }
    for (constraint, default) in type_parameters {
        generic_detail += 1
            + constraint.map_or(0, |c| c.generic_detail)
            + default.map_or(0, |d| d.generic_detail);
    }
    FnScore {
        symbolic_carriers,
        generic_detail,
    }
}

/// A FUNCTION / CONSTRUCTOR-TYPE `Out`: a structural root carrying the function's
/// `symbolic_carriers` + `generic_detail`, never unknown.
fn function_out(function: FnScore) -> PublicationScore {
    PublicationScore {
        symbolic_carriers: function.symbolic_carriers,
        generic_detail: function.generic_detail,
        structural_top_level: true,
        exact_unknown_root: false,
    }
}

// ===========================================================================
// The node-front algebra — folds `SemanticNodeData` through `fold_node`.
// ===========================================================================

/// The publication-scoring algebra (`Out = ScoredOut`). Each arm constructs its
/// score from already-folded children via the shared [`combine`] rules; the fold
/// owns ALL control flow + child recursion, so this algebra reproduces
/// `count_*_in_expr(raise(node))` by construction.
struct PublicationScoreAlg;

impl RaisedShapeAlgebra for PublicationScoreAlg {
    type Out = ScoredOut;
    type Fn = FnScore;
    type Member = MemberScore;

    fn primitive(&mut self, _kind: PrimitiveName) -> ScoredOut {
        ScoredOut {
            score: combine::structural_leaf(),
            tag: FactShapeTag::Other,
        }
    }
    fn literal(&mut self, _value: LiteralValue) -> ScoredOut {
        ScoredOut {
            score: combine::structural_leaf(),
            tag: FactShapeTag::Other,
        }
    }
    fn infer(&mut self, _name: Arc<str>) -> ScoredOut {
        ScoredOut {
            score: combine::symbolic_leaf(),
            tag: FactShapeTag::Other,
        }
    }
    fn unknown(&mut self, raw: Arc<str>) -> ScoredOut {
        // Tag the object-surface sentinel exactly as the materializer's
        // `is_object_surface_sentinel` does so the intersection arm-drop matches.
        let tag = if raw.as_ref() == SEMANTIC_OBJECT_SURFACE {
            FactShapeTag::ObjectSurfaceSentinel
        } else {
            FactShapeTag::Other
        };
        ScoredOut {
            score: combine::unknown_leaf(),
            tag,
        }
    }
    fn opaque_sentinel(&mut self, err: &QueryError) -> ScoredOut {
        // Every `Opaque` reaching this arm materialises to `Unknown { raw }`
        // (`RecursiveRef` / `DeclPlaceholder` are routed by separate fold arms).
        // Tag the object-surface sentinel from the typed variant, the same class
        // the materializer recognises via the raw spelling.
        let tag = if crate::project_semantic_dispatch::raise_sentinel::query_error_is_object_surface_sentinel(err) {
            FactShapeTag::ObjectSurfaceSentinel
        } else {
            FactShapeTag::Other
        };
        ScoredOut {
            score: combine::unknown_leaf(),
            tag,
        }
    }
    fn recursive_ref(&mut self, _name: Arc<str>) -> ScoredOut {
        ScoredOut {
            score: combine::recursive_ref(std::iter::empty()),
            tag: FactShapeTag::Other,
        }
    }
    fn reference(&mut self, _name: Arc<str>, type_arguments: Vec<ScoredOut>) -> ScoredOut {
        ScoredOut {
            score: combine::reference(type_arguments.iter().map(|a| a.score)),
            tag: FactShapeTag::Other,
        }
    }
    fn synthetic_slot_binding(
        &mut self,
        _carrier: Arc<verter_type_expr::SyntheticCarrierKey>,
    ) -> ScoredOut {
        ScoredOut {
            score: combine::symbolic_leaf(),
            tag: FactShapeTag::Other,
        }
    }
    fn import_type(
        &mut self,
        _specifier: Arc<str>,
        _qualifier: Arc<[Arc<str>]>,
        _typeof_query: bool,
        type_arguments: Vec<ScoredOut>,
    ) -> ScoredOut {
        ScoredOut {
            score: combine::reference(type_arguments.iter().map(|a| a.score)),
            tag: FactShapeTag::Other,
        }
    }
    fn type_of(&mut self, _path: Vec<String>, _type_args: Vec<ScoredOut>) -> ScoredOut {
        ScoredOut {
            score: combine::symbolic_leaf(),
            tag: FactShapeTag::Other,
        }
    }

    fn union(&mut self, members: Vec<ScoredOut>) -> ScoredOut {
        ScoredOut {
            score: combine::union_or_intersection(members.iter().map(|m| m.score)),
            tag: FactShapeTag::Other,
        }
    }
    fn intersection(&mut self, arms: Vec<ScoredOut>) -> ScoredOut {
        ScoredOut {
            score: combine::union_or_intersection(arms.iter().map(|a| a.score)),
            tag: FactShapeTag::Other,
        }
    }
    fn empty_object(&mut self) -> ScoredOut {
        ScoredOut {
            score: combine::empty_object(),
            tag: FactShapeTag::EmptyObject,
        }
    }
    fn array(&mut self, element: ScoredOut, _readonly: bool) -> ScoredOut {
        ScoredOut {
            score: combine::array(element.score),
            tag: FactShapeTag::Other,
        }
    }
    fn tuple(
        &mut self,
        elements: Vec<FoldedTupleElement<ScoredOut>>,
        _readonly: bool,
    ) -> ScoredOut {
        ScoredOut {
            score: combine::tuple(elements.iter().map(|e| e.ty.score)),
            tag: FactShapeTag::Other,
        }
    }
    fn key_of(&mut self, base: ScoredOut) -> ScoredOut {
        ScoredOut {
            score: combine::key_of(base.score),
            tag: FactShapeTag::Other,
        }
    }
    fn indexed_access(&mut self, object: ScoredOut, index: ScoredOut) -> ScoredOut {
        ScoredOut {
            score: combine::indexed_access(object.score, index.score),
            tag: FactShapeTag::Other,
        }
    }
    fn conditional(
        &mut self,
        check: ScoredOut,
        extends: ScoredOut,
        true_type: ScoredOut,
        false_type: ScoredOut,
    ) -> ScoredOut {
        ScoredOut {
            score: combine::conditional(
                check.score,
                extends.score,
                true_type.score,
                false_type.score,
            ),
            tag: FactShapeTag::Other,
        }
    }
    fn mapped(
        &mut self,
        _parameter: String,
        source: ScoredOut,
        value: ScoredOut,
        _optional: MappedModifier,
        _readonly: MappedModifier,
        name_type: Option<ScoredOut>,
    ) -> ScoredOut {
        ScoredOut {
            score: combine::mapped(source.score, value.score, name_type.map(|n| n.score)),
            tag: FactShapeTag::Other,
        }
    }
    fn template_literal(&mut self, _quasis: Vec<String>, expressions: Vec<ScoredOut>) -> ScoredOut {
        ScoredOut {
            score: combine::template_literal(expressions.iter().map(|e| e.score)),
            tag: FactShapeTag::Other,
        }
    }
    fn type_parameter(
        &mut self,
        _name: Arc<str>,
        constraint: Option<ScoredOut>,
        default: Option<ScoredOut>,
    ) -> ScoredOut {
        ScoredOut {
            score: combine::type_parameter(constraint.map(|c| c.score), default.map(|d| d.score)),
            tag: FactShapeTag::Other,
        }
    }

    fn build_function(&mut self, function: FoldedFunction<ScoredOut>) -> FnScore {
        function_score(
            function.parameters.iter().map(|p| p.ty.score),
            function.return_type.map(|r| r.score),
            function
                .type_parameters
                .iter()
                .map(|tp| (tp.constraint.map(|c| c.score), tp.default.map(|d| d.score))),
        )
    }
    fn function_to_out(&mut self, function: FnScore) -> ScoredOut {
        ScoredOut {
            score: function_out(function),
            tag: FactShapeTag::Function,
        }
    }
    fn constructor_to_out(&mut self, function: FnScore) -> ScoredOut {
        // A constructor type scores identically to a function but tags `Other`:
        // the constructor rewrap reads the SIGNATURE child, never the constructor
        // itself, so it must not be `out_as_function`-extractable.
        ScoredOut {
            score: function_out(function),
            tag: FactShapeTag::Other,
        }
    }
    fn out_as_function(&self, out: &ScoredOut) -> Option<FnScore> {
        (out.tag == FactShapeTag::Function).then_some(FnScore {
            symbolic_carriers: out.score.symbolic_carriers,
            generic_detail: out.score.generic_detail,
        })
    }

    fn member_property(
        &mut self,
        _name: String,
        ty: ScoredOut,
        _optional: bool,
        _readonly: bool,
        _visibility: MemberVisibility,
        _spans: verter_type_expr::MemberSpans,
    ) -> MemberScore {
        MemberScore {
            symbolic_carriers: ty.score.symbolic_carriers,
            generic_detail: ty.score.generic_detail,
        }
    }
    fn member_method(
        &mut self,
        _name: String,
        function: FnScore,
        _optional: bool,
        _visibility: MemberVisibility,
        _spans: verter_type_expr::MemberSpans,
    ) -> MemberScore {
        MemberScore {
            symbolic_carriers: function.symbolic_carriers,
            generic_detail: function.generic_detail,
        }
    }
    fn member_call_signature(&mut self, function: FnScore) -> MemberScore {
        MemberScore {
            symbolic_carriers: function.symbolic_carriers,
            generic_detail: function.generic_detail,
        }
    }
    fn member_construct_signature(&mut self, function: FnScore) -> MemberScore {
        MemberScore {
            symbolic_carriers: function.symbolic_carriers,
            generic_detail: function.generic_detail,
        }
    }
    fn member_index_signature(
        &mut self,
        _key_name: String,
        key_type: ScoredOut,
        value_type: ScoredOut,
        _readonly: bool,
        _spans: verter_type_expr::IndexSignatureSpans,
    ) -> MemberScore {
        MemberScore {
            symbolic_carriers: key_type.score.symbolic_carriers
                + value_type.score.symbolic_carriers,
            generic_detail: key_type.score.generic_detail + value_type.score.generic_detail,
        }
    }
    fn object_from_members(&mut self, members: Vec<MemberScore>) -> ScoredOut {
        let tag = if members.is_empty() {
            FactShapeTag::EmptyObject
        } else {
            FactShapeTag::Other
        };
        ScoredOut {
            score: combine::object(
                members
                    .iter()
                    .map(|m| (m.symbolic_carriers, m.generic_detail)),
            ),
            tag,
        }
    }

    fn is_object_surface_sentinel(&self, out: &ScoredOut) -> bool {
        out.tag == FactShapeTag::ObjectSurfaceSentinel
    }
    fn is_empty_object(&self, out: &ScoredOut) -> bool {
        out.tag == FactShapeTag::EmptyObject
    }
}

// ===========================================================================
// Public entry points.
// ===========================================================================

/// The [`PublicationScore`] of `node`, folded through the publication algebra over
/// the shared [`super::fold_node`] traversal. `None` when the whole raise is
/// `None` (the node — or a `?`-propagating required child — is unraisable).
pub(in crate::project_semantic_dispatch) fn project_node_publication_score(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<PublicationScore> {
    let mut alg = PublicationScoreAlg;
    let mut active = FxHashSet::default();
    Some(fold_node_publication(&mut alg, dispatch, node, &mut active)?.score)
}

/// Thin alias so the entry point and the recursion read clearly.
fn fold_node_publication(
    alg: &mut PublicationScoreAlg,
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<ScoredOut> {
    super::fold_node(alg, dispatch, node, active)
}

/// The [`PublicationScore`] of an existing `&TypeExpr` — the TypeExpr front of
/// the SHARED publication formula ([`combine`]). Reproduces the current
/// `count_symbolic_carriers_in_expr` / `count_generic_detail_in_expr` /
/// `type_expr_has_structural_top_level` / `matches!(_, Unknown)` semantics
/// EXACTLY, so `compare_type_expr_improvement`'s existing callers see
/// byte-identical verdicts.
#[cfg(test)]
pub(in crate::project_semantic_dispatch) fn type_expr_publication_score(
    expr: &TypeExpr,
) -> PublicationScore {
    match expr {
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => combine::structural_leaf(),
        TypeExpr::Infer { .. } | TypeExpr::TypeOf(_) | TypeExpr::SyntheticSlotBinding(_) => {
            combine::symbolic_leaf()
        }
        TypeExpr::Unknown { .. } => combine::unknown_leaf(),
        TypeExpr::RecursiveRef { type_arguments, .. } => {
            combine::recursive_ref(type_arguments.iter().map(type_expr_publication_score))
        }
        TypeExpr::Ref { type_arguments, .. } | TypeExpr::ImportType { type_arguments, .. } => {
            combine::reference(type_arguments.iter().map(type_expr_publication_score))
        }
        TypeExpr::TypeParameter(parameter) => combine::type_parameter(
            parameter
                .constraint
                .as_deref()
                .map(type_expr_publication_score),
            parameter
                .default
                .as_deref()
                .map(type_expr_publication_score),
        ),
        TypeExpr::IndexedAccess { object, index } => combine::indexed_access(
            type_expr_publication_score(object),
            type_expr_publication_score(index),
        ),
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => combine::conditional(
            type_expr_publication_score(check),
            type_expr_publication_score(extends),
            type_expr_publication_score(true_type),
            type_expr_publication_score(false_type),
        ),
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => combine::mapped(
            type_expr_publication_score(source),
            type_expr_publication_score(value),
            name_type.as_deref().map(type_expr_publication_score),
        ),
        TypeExpr::TemplateLiteral { expressions, .. } => {
            combine::template_literal(expressions.iter().map(type_expr_publication_score))
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            combine::union_or_intersection(members.iter().map(type_expr_publication_score))
        }
        TypeExpr::Array { element, .. } => combine::array(type_expr_publication_score(element)),
        TypeExpr::Tuple { elements, .. } => {
            combine::tuple(elements.iter().map(|e| type_expr_publication_score(&e.ty)))
        }
        TypeExpr::KeyOf(inner) => combine::key_of(type_expr_publication_score(inner)),
        TypeExpr::Parenthesized(inner) => {
            combine::parenthesized(type_expr_publication_score(inner))
        }
        TypeExpr::Rest(inner) => combine::rest(type_expr_publication_score(inner)),
        TypeExpr::Object(object) => {
            combine::object(object.properties.iter().map(object_member_score))
        }
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            function_out(function_expr_score(function))
        }
    }
}

/// The `(symbolic_carriers, generic_detail)` contribution of one `&TypeExpr`
/// object member — mirrors the per-member arms of `count_symbolic_carriers_in_expr`
/// + `count_generic_detail_in_expr`.
#[cfg(test)]
fn object_member_score(member: &verter_type_expr::ObjectMember) -> (usize, usize) {
    use verter_type_expr::ObjectMember;
    match member {
        ObjectMember::Property(property) => {
            let score = type_expr_publication_score(&property.ty);
            (score.symbolic_carriers, score.generic_detail)
        }
        ObjectMember::Method(method) => {
            let function = function_expr_score(&method.function);
            (function.symbolic_carriers, function.generic_detail)
        }
        ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
            let function = function_expr_score(function);
            (function.symbolic_carriers, function.generic_detail)
        }
        ObjectMember::IndexSignature(signature) => {
            let key = type_expr_publication_score(&signature.key_type);
            let value = type_expr_publication_score(&signature.value_type);
            (
                key.symbolic_carriers + value.symbolic_carriers,
                key.generic_detail + value.generic_detail,
            )
        }
    }
}

/// The [`FnScore`] of a `&FunctionExpr` — feeds the shared [`function_score`].
#[cfg(test)]
fn function_expr_score(function: &verter_type_expr::FunctionExpr) -> FnScore {
    function_score(
        function
            .parameters
            .iter()
            .map(|p| type_expr_publication_score(&p.ty)),
        function
            .return_type
            .as_deref()
            .map(type_expr_publication_score),
        function.type_parameters.iter().map(|tp| {
            (
                tp.constraint.as_deref().map(type_expr_publication_score),
                tp.default.as_deref().map(type_expr_publication_score),
            )
        }),
    )
}
