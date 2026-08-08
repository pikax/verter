//! Algebra 2 + 2-facts + 3 of the shared shape engine:
//! - `RaisedShapeAlg` (bottom-up facts/key, interns a structural key),
//! - `RaisedFactsAlg` (the SAME bottom-up facts, NO key interning — for the
//!   facts-only route gates),
//! - `type_expr_to_key` (folding an existing `&TypeExpr` into the same interned
//!   key space).
//!
//! The per-arm FACT + TAG formulas live ONCE in the [`summary`] constructor
//! layer; both `RaisedShapeAlg` and `RaisedFactsAlg` build their per-arm values
//! through it, so the two can never drift (parity is structural). The node-domain
//! algebras stay separate while the fold, algebra trait, and interned term remain
//! in the parent `shape_engine` module.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_type_expr::{
    LiteralValue, MappedModifier, MemberVisibility, PrimitiveName, TypeExpr, UnknownValue,
};

use super::fold::{FoldedFunction, FoldedTupleElement};
use super::{
    FactShapeTag, RaisedFunction, RaisedFunctionParam, RaisedObjectMember, RaisedRecursiveFrame,
    RaisedRootKind, RaisedShapeAlgebra, RaisedShapeFacts, RaisedShapeKey, RaisedShapeResult,
    RaisedShapeSummary, RaisedTerm, RaisedTupleElement, RaisedTypeParam, RootOnlySummary,
    ShapeInterner,
};
use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::resolver_core::component_meta_query_engine::semantic_query_error_raw;
use crate::semantic_query::{IndexKey, QueryError, SemanticNodeData, SemanticNodeId};

// ===========================================================================
// Shared summary-constructor layer — the SINGLE source of the per-arm
// fact + tag formulas. Both `RaisedShapeAlg` (which additionally interns a
// structural key) and `RaisedFactsAlg` (which does not) build their per-arm
// [`RaisedShapeSummary`] through these pure functions, so the
// `materialized` / `expanded_surface` / `tag` rules can never drift between the
// two algebras. The functions take ONLY the child facts they fold (never a key
// or interner), exactly mirroring the historical inline `RaisedShapeAlg` arms.
// ===========================================================================
mod summary;

// ===========================================================================
// Algebra 2 — `RaisedShapeAlg` (Out = RaisedShapeResult).
//
// The TRUE bottom-up facts/key, computed from the POST-NORMALIZED raised shape
// WITHOUT allocating a `TypeExpr`. Each arm interns a `RaisedTerm` (producing a
// `RaisedShapeKey`) AND folds the two facts from children's facts exactly as
// `dispatch_route_expr_is_materialized` / `type_expr_is_expanded_surface` would
// on the constructed shape.
// ===========================================================================

/// Node-domain facts/key algebra, backed by a per-evaluation interner. Its `Out`
/// carries the interned key PLUS the shared [`RaisedShapeSummary`] (facts + tag),
/// so the per-arm fact/tag logic comes from the [`summary`] layer and only the
/// term construction + interning are added here. The surviving members of an
/// object are accumulated as `(RaisedObjectMember, materialized_fact)` pairs so
/// `object_from_members` can fold the object's `materialized` fact over them.
pub(super) struct RaisedShapeAlg<'i> {
    pub(super) interner: &'i mut ShapeInterner,
}

/// A folded object member plus the `materialized` fact it contributes (the
/// `expanded_surface` fact never recurses object members, so only the
/// `materialized` AND needs per-member tracking).
pub(super) struct RaisedMember {
    member: RaisedObjectMember,
    materialized: bool,
}

impl RaisedShapeAlg<'_> {
    /// Intern `term` and pair it with the shared [`summary`]-layer
    /// [`RaisedShapeSummary`] into a [`RaisedShapeResult`].
    fn result(&mut self, term: RaisedTerm, summary: RaisedShapeSummary) -> RaisedShapeResult {
        let key = self.interner.intern(term);
        RaisedShapeResult { key, summary }
    }

    /// Fold a [`FoldedFunction`] of [`RaisedShapeResult`] children into the
    /// interned [`RaisedFunction`] plus its `materialized` fact (params +
    /// return all materialized).
    fn build_raised_function(
        &mut self,
        function: FoldedFunction<RaisedShapeResult>,
    ) -> (RaisedFunction, bool) {
        let mut materialized = true;
        let parameters: Vec<RaisedFunctionParam> = function
            .parameters
            .into_iter()
            .map(|p| {
                materialized &= p.ty.facts().materialized;
                RaisedFunctionParam {
                    name: p.name,
                    ty: p.ty.key,
                    optional: p.optional,
                    rest: p.rest,
                    span: p.span,
                }
            })
            .collect();
        let return_type = function.return_type.map(|r| {
            materialized &= r.facts().materialized;
            r.key
        });
        let type_parameters: Vec<RaisedTypeParam> = function
            .type_parameters
            .into_iter()
            .map(|tp| RaisedTypeParam {
                name: tp.name,
                // Type-parameter constraint/default are NOT checked by
                // `dispatch_route_expr_is_materialized` for Function (it only
                // recurses params + return), so they do not gate `materialized`.
                constraint: tp.constraint.map(|c| c.key),
                default: tp.default.map(|d| d.key),
                is_const: tp.is_const,
            })
            .collect();
        (
            RaisedFunction {
                parameters,
                return_type,
                type_parameters,
                signature_span: function.signature_span,
                return_type_span: function.return_type_span,
            },
            materialized,
        )
    }
}

impl RaisedShapeAlgebra for RaisedShapeAlg<'_> {
    type Out = RaisedShapeResult;
    type Fn = (RaisedFunction, bool);
    type Member = RaisedMember;

    fn primitive(&mut self, kind: PrimitiveName) -> RaisedShapeResult {
        self.result(
            RaisedTerm::Primitive(kind),
            summary::materialized_expanded_leaf(),
        )
    }
    fn literal(&mut self, value: LiteralValue) -> RaisedShapeResult {
        self.result(
            RaisedTerm::Literal(value),
            summary::materialized_expanded_leaf(),
        )
    }
    fn infer(&mut self, name: Arc<str>) -> RaisedShapeResult {
        self.result(
            RaisedTerm::Infer { name },
            summary::materialized_expanded_leaf(),
        )
    }
    fn unknown(&mut self, value: UnknownValue) -> RaisedShapeResult {
        let summary = summary::unknown(&value);
        self.result(RaisedTerm::Unknown(value), summary)
    }
    fn opaque_sentinel(&mut self, err: &QueryError) -> RaisedShapeResult {
        // The interned STRUCTURAL key is the terminal compatibility
        // projection — raw-only identity, so node-vs-`TypeExpr` equality is
        // preserved byte-for-byte; the SUMMARY is classified from the typed
        // variant via the shared authority. Provenance and the `QueryError`
        // never enter structural key identity.
        let summary = summary::opaque_sentinel(err);
        self.result(
            RaisedTerm::Unknown(UnknownValue::compatibility_projection(
                semantic_query_error_raw(err),
            )),
            summary,
        )
    }
    fn recursive_ref(&mut self, name: Arc<str>) -> RaisedShapeResult {
        self.result(
            RaisedTerm::RecursiveRef {
                name,
                type_arguments: Vec::new(),
                conditional_context: Vec::new(),
            },
            summary::materialized_expanded_leaf(),
        )
    }
    fn reference(
        &mut self,
        name: Arc<str>,
        type_arguments: Vec<RaisedShapeResult>,
    ) -> RaisedShapeResult {
        // A `Ref` is materialized regardless of its type-argument shapes
        // (`dispatch_route_expr_is_materialized` treats `Ref { .. }` as a
        // materialized leaf), and is an expanded surface.
        let type_arguments = type_arguments.into_iter().map(|a| a.key).collect();
        self.result(
            RaisedTerm::Ref {
                name,
                type_arguments,
            },
            summary::reference_leaf(),
        )
    }
    fn synthetic_slot_binding(
        &mut self,
        carrier: Arc<verter_type_expr::SyntheticCarrierKey>,
    ) -> RaisedShapeResult {
        self.result(
            RaisedTerm::SyntheticSlotBinding(carrier),
            summary::materialized_expanded_leaf(),
        )
    }
    fn import_type(
        &mut self,
        specifier: Arc<str>,
        qualifier: Arc<[Arc<str>]>,
        typeof_query: bool,
        type_arguments: Vec<RaisedShapeResult>,
    ) -> RaisedShapeResult {
        let type_arguments = type_arguments.into_iter().map(|a| a.key).collect();
        self.result(
            RaisedTerm::ImportType {
                specifier,
                qualifier: qualifier.iter().map(Arc::clone).collect(),
                typeof_query,
                type_arguments,
            },
            summary::materialized_expanded_leaf(),
        )
    }
    fn type_of(
        &mut self,
        path: Vec<String>,
        type_args: Vec<RaisedShapeResult>,
    ) -> RaisedShapeResult {
        let path = path.into_iter().map(Arc::from).collect();
        let type_args = type_args.into_iter().map(|a| a.key).collect();
        self.result(RaisedTerm::TypeOf { path, type_args }, summary::type_of())
    }

    fn union(&mut self, members: Vec<RaisedShapeResult>) -> RaisedShapeResult {
        let summary = summary::union(members.iter().map(|m| m.summary.facts));
        let members = members.into_iter().map(|m| m.key).collect();
        self.result(RaisedTerm::Union(members), summary)
    }
    fn intersection(&mut self, arms: Vec<RaisedShapeResult>) -> RaisedShapeResult {
        let summary = summary::intersection(arms.iter().map(|a| a.summary.facts));
        let arms = arms.into_iter().map(|a| a.key).collect();
        self.result(RaisedTerm::Intersection(arms), summary)
    }
    fn empty_object(&mut self) -> RaisedShapeResult {
        self.result(RaisedTerm::Object(Vec::new()), summary::empty_object())
    }
    fn array(&mut self, element: RaisedShapeResult, readonly: bool) -> RaisedShapeResult {
        let summary = summary::array(element.summary.facts);
        self.result(
            RaisedTerm::Array {
                element: element.key,
                readonly,
            },
            summary,
        )
    }
    fn tuple(
        &mut self,
        elements: Vec<FoldedTupleElement<RaisedShapeResult>>,
        readonly: bool,
    ) -> RaisedShapeResult {
        let summary = summary::tuple(elements.iter().map(|e| e.ty.summary.facts));
        let elements = elements
            .into_iter()
            .map(|e| RaisedTupleElement {
                label: e.label.map(Arc::from),
                ty: e.ty.key,
                optional: e.optional,
                rest: e.rest,
            })
            .collect();
        self.result(RaisedTerm::Tuple { elements, readonly }, summary)
    }
    fn key_of(&mut self, base: RaisedShapeResult) -> RaisedShapeResult {
        let summary = summary::key_of(base.summary.facts);
        self.result(RaisedTerm::KeyOf(base.key), summary)
    }
    fn indexed_access(
        &mut self,
        object: RaisedShapeResult,
        index: RaisedShapeResult,
    ) -> RaisedShapeResult {
        let summary = summary::indexed_access(object.summary.facts, index.summary.facts);
        self.result(
            RaisedTerm::IndexedAccess {
                object: object.key,
                index: index.key,
            },
            summary,
        )
    }
    fn conditional(
        &mut self,
        check: RaisedShapeResult,
        extends: RaisedShapeResult,
        true_type: RaisedShapeResult,
        false_type: RaisedShapeResult,
    ) -> RaisedShapeResult {
        let summary = summary::conditional(
            check.summary.facts,
            extends.summary.facts,
            true_type.summary.facts,
            false_type.summary.facts,
        );
        self.result(
            RaisedTerm::Conditional {
                check: check.key,
                extends: extends.key,
                true_type: true_type.key,
                false_type: false_type.key,
            },
            summary,
        )
    }
    fn mapped(
        &mut self,
        parameter: String,
        source: RaisedShapeResult,
        value: RaisedShapeResult,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<RaisedShapeResult>,
    ) -> RaisedShapeResult {
        let summary = summary::mapped(
            source.summary.facts,
            value.summary.facts,
            name_type.as_ref().map(|n| n.summary.facts),
            value.summary.root_semantic_miss_sentinel,
        );
        self.result(
            RaisedTerm::Mapped {
                parameter: Arc::from(parameter),
                source: source.key,
                value: value.key,
                optional,
                readonly,
                name_type: name_type.map(|n| n.key),
            },
            summary,
        )
    }
    fn template_literal(
        &mut self,
        quasis: Vec<String>,
        expressions: Vec<RaisedShapeResult>,
    ) -> RaisedShapeResult {
        // `TemplateLiteral` is a materialized, expanded leaf
        // (`dispatch_route_expr_is_materialized` does NOT recurse its
        // expressions).
        let quasis = quasis.into_iter().map(Arc::from).collect();
        let expressions = expressions.into_iter().map(|e| e.key).collect();
        self.result(
            RaisedTerm::TemplateLiteral {
                quasis,
                expressions,
            },
            summary::materialized_expanded_leaf(),
        )
    }
    fn type_parameter(
        &mut self,
        name: Arc<str>,
        constraint: Option<RaisedShapeResult>,
        default: Option<RaisedShapeResult>,
    ) -> RaisedShapeResult {
        // `TypeParameter` is a materialized, expanded leaf
        // (`dispatch_route_expr_is_materialized` treats it as `true`; its
        // constraint/default are not recursed).
        self.result(
            RaisedTerm::TypeParameter {
                name,
                constraint: constraint.map(|c| c.key),
                default: default.map(|d| d.key),
            },
            summary::materialized_expanded_leaf(),
        )
    }

    fn build_function(
        &mut self,
        function: FoldedFunction<RaisedShapeResult>,
    ) -> (RaisedFunction, bool) {
        self.build_raised_function(function)
    }
    fn function_to_out(&mut self, function: (RaisedFunction, bool)) -> RaisedShapeResult {
        let (function, materialized) = function;
        self.result(
            RaisedTerm::Function(function),
            summary::function(materialized),
        )
    }
    fn constructor_to_out(&mut self, function: (RaisedFunction, bool)) -> RaisedShapeResult {
        let (function, materialized) = function;
        self.result(
            RaisedTerm::ConstructorType(function),
            summary::constructor(materialized),
        )
    }
    fn out_as_function(&self, out: &RaisedShapeResult) -> Option<(RaisedFunction, bool)> {
        // Gate on the shared tag (the SAME class the facts-only algebra checks),
        // then extract the interned `RaisedFunction` structure the rewrap /
        // object-member assembly needs. Tag and interner-readback agree by
        // construction (`function_to_out` is the only `Function`-tagging arm).
        if out.summary.tag != FactShapeTag::Function {
            return None;
        }
        match self
            .interner
            .terms
            .get(out.key.0 as usize)
            .map(|t| t.as_ref())
        {
            Some(RaisedTerm::Function(function)) => {
                Some((function.clone(), out.summary.facts.materialized))
            }
            _ => None,
        }
    }

    fn out_as_constructor(&self, out: &RaisedShapeResult) -> Option<(RaisedFunction, bool)> {
        match self
            .interner
            .terms
            .get(out.key.0 as usize)
            .map(|t| t.as_ref())
        {
            Some(RaisedTerm::ConstructorType(function)) => {
                Some((function.clone(), out.summary.facts.materialized))
            }
            _ => None,
        }
    }

    fn member_property(
        &mut self,
        key: verter_type_expr::AuthoredPropertyKey<
            RaisedShapeResult,
            verter_type_expr::facts::ValueDeclIdentityPart,
        >,
        ty: RaisedShapeResult,
        optional: bool,
        readonly: bool,
        visibility: MemberVisibility,
        excess_origin: verter_type_expr::ExcessPropertyOrigin,
        spans: verter_type_expr::MemberSpans,
    ) -> RaisedMember {
        let mut materialized = ty.summary.facts.materialized;
        let key = key.map(
            |computed| {
                materialized &= computed.summary.facts.materialized;
                computed.key
            },
            |identity| identity,
        );
        RaisedMember {
            materialized,
            member: RaisedObjectMember::Property {
                key,
                ty: ty.key,
                optional,
                readonly,
                visibility,
                excess_origin,
                spans,
            },
        }
    }
    fn member_method(
        &mut self,
        key: verter_type_expr::AuthoredPropertyKey<
            RaisedShapeResult,
            verter_type_expr::facts::ValueDeclIdentityPart,
        >,
        function: (RaisedFunction, bool),
        optional: bool,
        method_kind: verter_type_expr::ObjectMethodKind,
        has_implementation_body: bool,
        visibility: MemberVisibility,
        excess_origin: verter_type_expr::ExcessPropertyOrigin,
        spans: verter_type_expr::MemberSpans,
    ) -> RaisedMember {
        let (function, mut materialized) = function;
        let key = key.map(
            |computed| {
                materialized &= computed.summary.facts.materialized;
                computed.key
            },
            |identity| identity,
        );
        RaisedMember {
            materialized,
            member: RaisedObjectMember::Method {
                key,
                function,
                optional,
                method_kind,
                has_implementation_body,
                visibility,
                excess_origin,
                spans,
            },
        }
    }
    fn member_spread(&mut self, ty: RaisedShapeResult) -> RaisedMember {
        RaisedMember {
            materialized: ty.summary.facts.materialized,
            member: RaisedObjectMember::Spread { ty: ty.key },
        }
    }
    fn member_call_signature(&mut self, function: (RaisedFunction, bool)) -> RaisedMember {
        let (function, materialized) = function;
        RaisedMember {
            materialized,
            member: RaisedObjectMember::CallSignature(function),
        }
    }
    fn member_construct_signature(&mut self, function: (RaisedFunction, bool)) -> RaisedMember {
        let (function, materialized) = function;
        RaisedMember {
            materialized,
            member: RaisedObjectMember::ConstructSignature(function),
        }
    }
    fn member_index_signature(
        &mut self,
        key_name: String,
        key_type: RaisedShapeResult,
        value_type: RaisedShapeResult,
        readonly: bool,
        spans: verter_type_expr::IndexSignatureSpans,
    ) -> RaisedMember {
        RaisedMember {
            materialized: key_type.summary.facts.materialized
                && value_type.summary.facts.materialized,
            member: RaisedObjectMember::IndexSignature {
                key_name: Arc::from(key_name),
                key_type: key_type.key,
                value_type: value_type.key,
                readonly,
                spans,
            },
        }
    }
    fn object_from_members(&mut self, members: Vec<RaisedMember>) -> RaisedShapeResult {
        let summary = summary::object_from_members(
            members.iter().map(|m| m.materialized),
            members.is_empty(),
        );
        let members = members.into_iter().map(|m| m.member).collect();
        self.result(RaisedTerm::Object(members), summary)
    }

    fn is_object_surface_sentinel(&self, out: &RaisedShapeResult) -> bool {
        out.summary.tag == FactShapeTag::ObjectSurfaceSentinel
    }
    fn is_empty_object(&self, out: &RaisedShapeResult) -> bool {
        out.summary.tag == FactShapeTag::EmptyObject
    }
}

// ===========================================================================
// Algebra 2-facts — `RaisedFactsAlg` (Out = RaisedShapeSummary).
//
// The SAME bottom-up facts as `RaisedShapeAlg`, computed through the SAME
// [`summary`] constructor layer, but with NO key interning: the facts-only
// route gates (`materialized` / `expanded_surface` / `can_shell_raise`) never
// compare a structural key, so building the `RaisedTerm` DAG is pure waste for
// them. The three structural inspections the fold needs
// (`is_object_surface_sentinel` / `is_empty_object` / `out_as_function`) read
// the shared [`FactShapeTag`] carried in the `Out`, identically to how the full
// algebra's interner-readback classified them.
// ===========================================================================

/// Stateless facts-only algebra — no interner.
pub(super) struct RaisedFactsAlg;

/// The facts-only function representation: only the folded `materialized` fact
/// (the facts-only path never needs the `RaisedFunction` structure).
#[derive(Clone, Copy)]
pub(super) struct FactsFunction {
    materialized: bool,
}

/// A facts-only object member: only the `materialized` fact it contributes.
pub(super) struct FactsMember {
    materialized: bool,
}

impl RaisedFactsAlg {
    /// Fold a [`FoldedFunction`] of [`RaisedShapeSummary`] children into the
    /// function's `materialized` fact (params + return all materialized),
    /// IGNORING type-parameter constraints/defaults — exactly as
    /// `RaisedShapeAlg::build_raised_function` does.
    fn function_materialized(function: &FoldedFunction<RaisedShapeSummary>) -> bool {
        let mut materialized = true;
        for p in &function.parameters {
            materialized &= p.ty.facts.materialized;
        }
        if let Some(r) = function.return_type.as_ref() {
            materialized &= r.facts.materialized;
        }
        materialized
    }
}

impl RaisedShapeAlgebra for RaisedFactsAlg {
    type Out = RaisedShapeSummary;
    type Fn = FactsFunction;
    type Member = FactsMember;

    fn primitive(&mut self, _kind: PrimitiveName) -> RaisedShapeSummary {
        summary::materialized_expanded_leaf()
    }
    fn literal(&mut self, _value: LiteralValue) -> RaisedShapeSummary {
        summary::materialized_expanded_leaf()
    }
    fn infer(&mut self, _name: Arc<str>) -> RaisedShapeSummary {
        summary::materialized_expanded_leaf()
    }
    fn unknown(&mut self, value: UnknownValue) -> RaisedShapeSummary {
        summary::unknown(&value)
    }
    fn opaque_sentinel(&mut self, err: &QueryError) -> RaisedShapeSummary {
        summary::opaque_sentinel(err)
    }
    fn recursive_ref(&mut self, _name: Arc<str>) -> RaisedShapeSummary {
        summary::materialized_expanded_leaf()
    }
    fn reference(
        &mut self,
        _name: Arc<str>,
        _type_arguments: Vec<RaisedShapeSummary>,
    ) -> RaisedShapeSummary {
        summary::reference_leaf()
    }
    fn synthetic_slot_binding(
        &mut self,
        _carrier: Arc<verter_type_expr::SyntheticCarrierKey>,
    ) -> RaisedShapeSummary {
        summary::materialized_expanded_leaf()
    }
    fn import_type(
        &mut self,
        _specifier: Arc<str>,
        _qualifier: Arc<[Arc<str>]>,
        _typeof_query: bool,
        _type_arguments: Vec<RaisedShapeSummary>,
    ) -> RaisedShapeSummary {
        summary::materialized_expanded_leaf()
    }
    fn type_of(
        &mut self,
        _path: Vec<String>,
        _type_args: Vec<RaisedShapeSummary>,
    ) -> RaisedShapeSummary {
        summary::type_of()
    }

    fn union(&mut self, members: Vec<RaisedShapeSummary>) -> RaisedShapeSummary {
        summary::union(members.into_iter().map(|m| m.facts))
    }
    fn intersection(&mut self, arms: Vec<RaisedShapeSummary>) -> RaisedShapeSummary {
        summary::intersection(arms.into_iter().map(|a| a.facts))
    }
    fn empty_object(&mut self) -> RaisedShapeSummary {
        summary::empty_object()
    }
    fn array(&mut self, element: RaisedShapeSummary, _readonly: bool) -> RaisedShapeSummary {
        summary::array(element.facts)
    }
    fn tuple(
        &mut self,
        elements: Vec<FoldedTupleElement<RaisedShapeSummary>>,
        _readonly: bool,
    ) -> RaisedShapeSummary {
        summary::tuple(elements.into_iter().map(|e| e.ty.facts))
    }
    fn key_of(&mut self, base: RaisedShapeSummary) -> RaisedShapeSummary {
        summary::key_of(base.facts)
    }
    fn indexed_access(
        &mut self,
        object: RaisedShapeSummary,
        index: RaisedShapeSummary,
    ) -> RaisedShapeSummary {
        summary::indexed_access(object.facts, index.facts)
    }
    fn conditional(
        &mut self,
        check: RaisedShapeSummary,
        extends: RaisedShapeSummary,
        true_type: RaisedShapeSummary,
        false_type: RaisedShapeSummary,
    ) -> RaisedShapeSummary {
        summary::conditional(
            check.facts,
            extends.facts,
            true_type.facts,
            false_type.facts,
        )
    }
    fn mapped(
        &mut self,
        _parameter: String,
        source: RaisedShapeSummary,
        value: RaisedShapeSummary,
        _optional: MappedModifier,
        _readonly: MappedModifier,
        name_type: Option<RaisedShapeSummary>,
    ) -> RaisedShapeSummary {
        summary::mapped(
            source.facts,
            value.facts,
            name_type.map(|n| n.facts),
            value.root_semantic_miss_sentinel,
        )
    }
    fn template_literal(
        &mut self,
        _quasis: Vec<String>,
        _expressions: Vec<RaisedShapeSummary>,
    ) -> RaisedShapeSummary {
        summary::materialized_expanded_leaf()
    }
    fn type_parameter(
        &mut self,
        _name: Arc<str>,
        _constraint: Option<RaisedShapeSummary>,
        _default: Option<RaisedShapeSummary>,
    ) -> RaisedShapeSummary {
        summary::materialized_expanded_leaf()
    }

    fn build_function(&mut self, function: FoldedFunction<RaisedShapeSummary>) -> FactsFunction {
        FactsFunction {
            materialized: Self::function_materialized(&function),
        }
    }
    fn function_to_out(&mut self, function: FactsFunction) -> RaisedShapeSummary {
        summary::function(function.materialized)
    }
    fn constructor_to_out(&mut self, function: FactsFunction) -> RaisedShapeSummary {
        summary::constructor(function.materialized)
    }
    fn out_as_function(&self, out: &RaisedShapeSummary) -> Option<FactsFunction> {
        // Read the SHARED tag — the same class the full algebra's interner
        // readback checks (only `function_to_out` tags `Function`).
        (out.tag == FactShapeTag::Function).then_some(FactsFunction {
            materialized: out.facts.materialized,
        })
    }
    fn out_as_constructor(&self, out: &RaisedShapeSummary) -> Option<FactsFunction> {
        // Read the shared tag (only `constructor_to_out` tags `Constructor`).
        (out.tag == FactShapeTag::Constructor).then_some(FactsFunction {
            materialized: out.facts.materialized,
        })
    }

    fn member_property(
        &mut self,
        _key: verter_type_expr::AuthoredPropertyKey<
            RaisedShapeSummary,
            verter_type_expr::facts::ValueDeclIdentityPart,
        >,
        ty: RaisedShapeSummary,
        _optional: bool,
        _readonly: bool,
        _visibility: MemberVisibility,
        _excess_origin: verter_type_expr::ExcessPropertyOrigin,
        _spans: verter_type_expr::MemberSpans,
    ) -> FactsMember {
        FactsMember {
            materialized: ty.facts.materialized,
        }
    }
    fn member_method(
        &mut self,
        _key: verter_type_expr::AuthoredPropertyKey<
            RaisedShapeSummary,
            verter_type_expr::facts::ValueDeclIdentityPart,
        >,
        function: FactsFunction,
        _optional: bool,
        _method_kind: verter_type_expr::ObjectMethodKind,
        _has_implementation_body: bool,
        _visibility: MemberVisibility,
        _excess_origin: verter_type_expr::ExcessPropertyOrigin,
        _spans: verter_type_expr::MemberSpans,
    ) -> FactsMember {
        FactsMember {
            materialized: function.materialized,
        }
    }
    fn member_spread(&mut self, ty: RaisedShapeSummary) -> FactsMember {
        FactsMember {
            materialized: ty.facts.materialized,
        }
    }
    fn member_call_signature(&mut self, function: FactsFunction) -> FactsMember {
        FactsMember {
            materialized: function.materialized,
        }
    }
    fn member_construct_signature(&mut self, function: FactsFunction) -> FactsMember {
        FactsMember {
            materialized: function.materialized,
        }
    }
    fn member_index_signature(
        &mut self,
        _key_name: String,
        key_type: RaisedShapeSummary,
        value_type: RaisedShapeSummary,
        _readonly: bool,
        _spans: verter_type_expr::IndexSignatureSpans,
    ) -> FactsMember {
        FactsMember {
            materialized: key_type.facts.materialized && value_type.facts.materialized,
        }
    }
    fn object_from_members(&mut self, members: Vec<FactsMember>) -> RaisedShapeSummary {
        summary::object_from_members(members.iter().map(|m| m.materialized), members.is_empty())
    }

    fn is_object_surface_sentinel(&self, out: &RaisedShapeSummary) -> bool {
        out.tag == FactShapeTag::ObjectSurfaceSentinel
    }
    fn is_empty_object(&self, out: &RaisedShapeSummary) -> bool {
        out.tag == FactShapeTag::EmptyObject
    }
}

// ===========================================================================
// Algebra 4 — `DeclarationFactsAlg`: the node-domain declaration-safety and
// `typeof`-dependency facts of one node, in ONE facts-only fold (no key
// interning). Mirrors the terminal splice pipeline's rules exactly:
//
// - UNSAFE leaves: `any` / `unknown` primitives, raw `Unknown` carriers,
//   typed opaque sentinels (they materialize to `Unknown`), and synthetic
//   slot bindings. A function whose folded return is ABSENT is unsafe.
// - `typeof <value>` arms record their root path; every other arm combines
//   child facts (safe = AND, paths = union).
// ===========================================================================

/// The shape tag a folded declaration-facts value carries (drives the fold's
/// intersection arm-drop and the function/constructor rewrap inspection).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DeclarationTag {
    Other,
    EmptyObject,
    UnrepresentableSurface,
    Function,
    Constructor,
}

/// The folded declaration facts of one node.
pub(super) struct DeclarationOut {
    tag: DeclarationTag,
    pub(super) safe: bool,
    pub(super) typeof_paths:
        std::collections::BTreeSet<verter_type_expr::facts::TypeDependencyPathFact>,
}

impl DeclarationOut {
    fn leaf(tag: DeclarationTag) -> Self {
        Self {
            tag,
            safe: true,
            typeof_paths: std::collections::BTreeSet::new(),
        }
    }
    fn unsafe_leaf() -> Self {
        Self {
            tag: DeclarationTag::Other,
            safe: false,
            typeof_paths: std::collections::BTreeSet::new(),
        }
    }
    fn combine(tag: DeclarationTag, children: Vec<DeclarationOut>) -> Self {
        let mut safe = true;
        let mut typeof_paths = std::collections::BTreeSet::new();
        for child in children {
            safe &= child.safe;
            typeof_paths.extend(child.typeof_paths);
        }
        Self {
            tag,
            safe,
            typeof_paths,
        }
    }
}

/// The declaration-facts function representation: the safety fact plus the
/// `typeof` dependency paths (the node-domain path never needs the
/// parameter structure).
pub(super) struct DeclarationFunction {
    safe: bool,
    typeof_paths: std::collections::BTreeSet<verter_type_expr::facts::TypeDependencyPathFact>,
}

/// A declaration-facts object member: only the facts it contributes.
pub(super) struct DeclarationMember {
    safe: bool,
    typeof_paths: std::collections::BTreeSet<verter_type_expr::facts::TypeDependencyPathFact>,
}

/// Stateless declaration-facts algebra — no interner.
pub(super) struct DeclarationFactsAlg;

impl DeclarationFactsAlg {
    fn function_safe(function: &FoldedFunction<DeclarationOut>) -> bool {
        // A function with NO folded return is declaration-unsafe (the
        // terminal pipeline never splices a return-less function type).
        let mut safe = function.return_type.is_some();
        for param in &function.parameters {
            safe &= param.ty.safe;
        }
        if let Some(return_type) = function.return_type.as_ref() {
            safe &= return_type.safe;
        }
        safe
    }
    fn function_paths(
        function: &FoldedFunction<DeclarationOut>,
    ) -> std::collections::BTreeSet<verter_type_expr::facts::TypeDependencyPathFact> {
        let mut paths = std::collections::BTreeSet::new();
        for param in &function.parameters {
            paths.extend(param.ty.typeof_paths.iter().cloned());
        }
        if let Some(return_type) = function.return_type.as_ref() {
            paths.extend(return_type.typeof_paths.iter().cloned());
        }
        paths
    }
}

impl RaisedShapeAlgebra for DeclarationFactsAlg {
    type Out = DeclarationOut;
    type Fn = DeclarationFunction;
    type Member = DeclarationMember;

    fn primitive(&mut self, kind: PrimitiveName) -> DeclarationOut {
        match kind {
            PrimitiveName::Any | PrimitiveName::Unknown => DeclarationOut::unsafe_leaf(),
            _ => DeclarationOut::leaf(DeclarationTag::Other),
        }
    }
    fn literal(&mut self, _value: LiteralValue) -> DeclarationOut {
        DeclarationOut::leaf(DeclarationTag::Other)
    }
    fn infer(&mut self, _name: Arc<str>) -> DeclarationOut {
        DeclarationOut::leaf(DeclarationTag::Other)
    }
    fn unknown(&mut self, _value: UnknownValue) -> DeclarationOut {
        DeclarationOut::unsafe_leaf()
    }
    fn opaque_sentinel(&mut self, err: &QueryError) -> DeclarationOut {
        let tag = if matches!(err, QueryError::UnrepresentableSurface) {
            DeclarationTag::UnrepresentableSurface
        } else {
            DeclarationTag::Other
        };
        DeclarationOut {
            tag,
            ..DeclarationOut::unsafe_leaf()
        }
    }
    fn recursive_ref(&mut self, _name: Arc<str>) -> DeclarationOut {
        DeclarationOut::leaf(DeclarationTag::Other)
    }
    fn reference(
        &mut self,
        _name: Arc<str>,
        type_arguments: Vec<DeclarationOut>,
    ) -> DeclarationOut {
        DeclarationOut::combine(DeclarationTag::Other, type_arguments)
    }
    fn synthetic_slot_binding(
        &mut self,
        _carrier: Arc<verter_type_expr::SyntheticCarrierKey>,
    ) -> DeclarationOut {
        DeclarationOut::unsafe_leaf()
    }
    fn import_type(
        &mut self,
        _specifier: Arc<str>,
        _qualifier: Arc<[Arc<str>]>,
        _typeof_query: bool,
        type_arguments: Vec<DeclarationOut>,
    ) -> DeclarationOut {
        DeclarationOut::combine(DeclarationTag::Other, type_arguments)
    }
    fn type_of(&mut self, path: Vec<String>, type_args: Vec<DeclarationOut>) -> DeclarationOut {
        let mut out = DeclarationOut::combine(DeclarationTag::Other, type_args);
        if let Some(fact) = verter_type_expr::facts::TypeDependencyPathFact::from_segments(path) {
            out.typeof_paths.insert(fact);
        }
        out
    }

    fn union(&mut self, members: Vec<DeclarationOut>) -> DeclarationOut {
        DeclarationOut::combine(DeclarationTag::Other, members)
    }
    fn intersection(&mut self, arms: Vec<DeclarationOut>) -> DeclarationOut {
        DeclarationOut::combine(DeclarationTag::Other, arms)
    }
    fn empty_object(&mut self) -> DeclarationOut {
        DeclarationOut::leaf(DeclarationTag::EmptyObject)
    }
    fn array(&mut self, element: DeclarationOut, _readonly: bool) -> DeclarationOut {
        DeclarationOut::combine(DeclarationTag::Other, vec![element])
    }
    fn tuple(
        &mut self,
        elements: Vec<FoldedTupleElement<DeclarationOut>>,
        _readonly: bool,
    ) -> DeclarationOut {
        DeclarationOut::combine(
            DeclarationTag::Other,
            elements.into_iter().map(|element| element.ty).collect(),
        )
    }
    fn key_of(&mut self, base: DeclarationOut) -> DeclarationOut {
        DeclarationOut::combine(DeclarationTag::Other, vec![base])
    }
    fn indexed_access(&mut self, object: DeclarationOut, index: DeclarationOut) -> DeclarationOut {
        DeclarationOut::combine(DeclarationTag::Other, vec![object, index])
    }
    fn conditional(
        &mut self,
        check: DeclarationOut,
        extends: DeclarationOut,
        true_type: DeclarationOut,
        false_type: DeclarationOut,
    ) -> DeclarationOut {
        DeclarationOut::combine(
            DeclarationTag::Other,
            vec![check, extends, true_type, false_type],
        )
    }
    fn mapped(
        &mut self,
        _parameter: String,
        source: DeclarationOut,
        value: DeclarationOut,
        _optional: MappedModifier,
        _readonly: MappedModifier,
        name_type: Option<DeclarationOut>,
    ) -> DeclarationOut {
        let mut children = vec![source, value];
        children.extend(name_type);
        DeclarationOut::combine(DeclarationTag::Other, children)
    }
    fn template_literal(
        &mut self,
        _quasis: Vec<String>,
        expressions: Vec<DeclarationOut>,
    ) -> DeclarationOut {
        DeclarationOut::combine(DeclarationTag::Other, expressions)
    }
    fn type_parameter(
        &mut self,
        _name: Arc<str>,
        constraint: Option<DeclarationOut>,
        default: Option<DeclarationOut>,
    ) -> DeclarationOut {
        DeclarationOut::combine(
            DeclarationTag::Other,
            constraint.into_iter().chain(default).collect(),
        )
    }

    fn build_function(&mut self, function: FoldedFunction<DeclarationOut>) -> DeclarationFunction {
        DeclarationFunction {
            safe: Self::function_safe(&function),
            typeof_paths: Self::function_paths(&function),
        }
    }
    fn function_to_out(&mut self, function: DeclarationFunction) -> DeclarationOut {
        DeclarationOut {
            tag: DeclarationTag::Function,
            safe: function.safe,
            typeof_paths: function.typeof_paths,
        }
    }
    fn constructor_to_out(&mut self, function: DeclarationFunction) -> DeclarationOut {
        DeclarationOut {
            tag: DeclarationTag::Constructor,
            safe: function.safe,
            typeof_paths: function.typeof_paths,
        }
    }
    fn out_as_function(&self, out: &DeclarationOut) -> Option<DeclarationFunction> {
        (out.tag == DeclarationTag::Function).then_some(DeclarationFunction {
            safe: out.safe,
            typeof_paths: out.typeof_paths.clone(),
        })
    }
    fn out_as_constructor(&self, out: &DeclarationOut) -> Option<DeclarationFunction> {
        (out.tag == DeclarationTag::Constructor).then_some(DeclarationFunction {
            safe: out.safe,
            typeof_paths: out.typeof_paths.clone(),
        })
    }

    fn member_property(
        &mut self,
        _key: verter_type_expr::AuthoredPropertyKey<
            DeclarationOut,
            verter_type_expr::facts::ValueDeclIdentityPart,
        >,
        ty: DeclarationOut,
        _optional: bool,
        _readonly: bool,
        _visibility: MemberVisibility,
        _excess_origin: verter_type_expr::ExcessPropertyOrigin,
        _spans: verter_type_expr::MemberSpans,
    ) -> DeclarationMember {
        DeclarationMember {
            safe: ty.safe,
            typeof_paths: ty.typeof_paths,
        }
    }
    fn member_method(
        &mut self,
        _key: verter_type_expr::AuthoredPropertyKey<
            DeclarationOut,
            verter_type_expr::facts::ValueDeclIdentityPart,
        >,
        function: DeclarationFunction,
        _optional: bool,
        _method_kind: verter_type_expr::ObjectMethodKind,
        _has_implementation_body: bool,
        _visibility: MemberVisibility,
        _excess_origin: verter_type_expr::ExcessPropertyOrigin,
        _spans: verter_type_expr::MemberSpans,
    ) -> DeclarationMember {
        DeclarationMember {
            safe: function.safe,
            typeof_paths: function.typeof_paths,
        }
    }
    fn member_spread(&mut self, ty: DeclarationOut) -> DeclarationMember {
        DeclarationMember {
            safe: ty.safe,
            typeof_paths: ty.typeof_paths,
        }
    }
    fn member_call_signature(&mut self, function: DeclarationFunction) -> DeclarationMember {
        DeclarationMember {
            safe: function.safe,
            typeof_paths: function.typeof_paths,
        }
    }
    fn member_construct_signature(&mut self, function: DeclarationFunction) -> DeclarationMember {
        DeclarationMember {
            safe: function.safe,
            typeof_paths: function.typeof_paths,
        }
    }
    fn member_index_signature(
        &mut self,
        _key_name: String,
        key_type: DeclarationOut,
        value_type: DeclarationOut,
        _readonly: bool,
        _spans: verter_type_expr::IndexSignatureSpans,
    ) -> DeclarationMember {
        let combined = DeclarationOut::combine(DeclarationTag::Other, vec![key_type, value_type]);
        DeclarationMember {
            safe: combined.safe,
            typeof_paths: combined.typeof_paths,
        }
    }
    fn object_from_members(&mut self, members: Vec<DeclarationMember>) -> DeclarationOut {
        let mut safe = true;
        let mut typeof_paths = std::collections::BTreeSet::new();
        for member in members {
            safe &= member.safe;
            typeof_paths.extend(member.typeof_paths);
        }
        DeclarationOut {
            tag: DeclarationTag::Other,
            safe,
            typeof_paths,
        }
    }

    fn is_object_surface_sentinel(&self, out: &DeclarationOut) -> bool {
        out.tag == DeclarationTag::UnrepresentableSurface
    }
    fn is_empty_object(&self, out: &DeclarationOut) -> bool {
        out.tag == DeclarationTag::EmptyObject
    }
}

// ===========================================================================
// Algebra 3 — `TypeExprShapeAlg`: fold an existing `&TypeExpr` into the SAME
// key space, so a node's raised shape can be compared against a caller's input
// `TypeExpr` without materializing the node.
//
// A faithful, INJECTIVE mirror of EVERY `TypeExpr` variant into the same
// `RaisedTerm` interner: structurally-equal `TypeExpr`s (and a node whose
// raised shape equals one) intern to the SAME key, and distinct `TypeExpr`s
// intern to distinct keys. This is the input side of
// `raised_shape_eq_node_type_expr`.
// ===========================================================================

/// Intern an existing `&TypeExpr` into the shared key space.
pub(super) fn type_expr_to_key(interner: &mut ShapeInterner, expr: &TypeExpr) -> RaisedShapeKey {
    let term = match expr {
        TypeExpr::Primitive(kind) => RaisedTerm::Primitive(*kind),
        TypeExpr::Literal(value) => RaisedTerm::Literal(value.clone()),
        TypeExpr::Union(members) => RaisedTerm::Union(
            members
                .iter()
                .map(|m| type_expr_to_key(interner, m))
                .collect(),
        ),
        TypeExpr::Intersection(members) => RaisedTerm::Intersection(
            members
                .iter()
                .map(|m| type_expr_to_key(interner, m))
                .collect(),
        ),
        TypeExpr::Array { element, readonly } => RaisedTerm::Array {
            element: type_expr_to_key(interner, element),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => RaisedTerm::Tuple {
            elements: elements
                .iter()
                .map(|e| RaisedTupleElement {
                    label: e.label.as_ref().map(|l| Arc::from(l.as_str())),
                    ty: type_expr_to_key(interner, &e.ty),
                    optional: e.optional,
                    rest: e.rest,
                })
                .collect(),
            readonly: *readonly,
        },
        TypeExpr::Object(object) => RaisedTerm::Object(
            object
                .properties
                .iter()
                .map(|m| object_member_to_raised(interner, m))
                .collect(),
        ),
        TypeExpr::Function(function) => {
            RaisedTerm::Function(function_expr_to_raised(interner, function))
        }
        TypeExpr::ConstructorType(function) => {
            RaisedTerm::ConstructorType(function_expr_to_raised(interner, function))
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => RaisedTerm::Ref {
            name: Arc::clone(name),
            type_arguments: type_arguments
                .iter()
                .map(|a| type_expr_to_key(interner, a))
                .collect(),
        },
        TypeExpr::TypeParameter(tp) => RaisedTerm::TypeParameter {
            name: Arc::from(tp.name.as_str()),
            constraint: tp
                .constraint
                .as_ref()
                .map(|c| type_expr_to_key(interner, c)),
            default: tp.default.as_ref().map(|d| type_expr_to_key(interner, d)),
        },
        TypeExpr::KeyOf(inner) => RaisedTerm::KeyOf(type_expr_to_key(interner, inner)),
        TypeExpr::TypeOf(value_ref) => RaisedTerm::TypeOf {
            path: value_ref
                .path
                .iter()
                .map(|s| Arc::from(s.as_str()))
                .collect(),
            type_args: value_ref
                .type_args
                .iter()
                .map(|a| type_expr_to_key(interner, a))
                .collect(),
        },
        TypeExpr::IndexedAccess { object, index } => RaisedTerm::IndexedAccess {
            object: type_expr_to_key(interner, object),
            index: type_expr_to_key(interner, index),
        },
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => RaisedTerm::Conditional {
            check: type_expr_to_key(interner, check),
            extends: type_expr_to_key(interner, extends),
            true_type: type_expr_to_key(interner, true_type),
            false_type: type_expr_to_key(interner, false_type),
        },
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => RaisedTerm::Mapped {
            parameter: Arc::from(parameter.as_str()),
            source: type_expr_to_key(interner, source),
            value: type_expr_to_key(interner, value),
            optional: *optional,
            readonly: *readonly,
            name_type: name_type.as_ref().map(|n| type_expr_to_key(interner, n)),
        },
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => RaisedTerm::TemplateLiteral {
            quasis: quasis.iter().map(|q| Arc::from(q.as_str())).collect(),
            expressions: expressions
                .iter()
                .map(|e| type_expr_to_key(interner, e))
                .collect(),
        },
        TypeExpr::Infer { name } => RaisedTerm::Infer {
            name: Arc::from(name.as_str()),
        },
        TypeExpr::Rest(inner) => RaisedTerm::Rest(type_expr_to_key(interner, inner)),
        TypeExpr::Parenthesized(inner) => {
            RaisedTerm::Parenthesized(type_expr_to_key(interner, inner))
        }
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => RaisedTerm::RecursiveRef {
            name: Arc::clone(name),
            type_arguments: type_arguments
                .iter()
                .map(|a| type_expr_to_key(interner, a))
                .collect(),
            conditional_context: conditional_context
                .iter()
                .map(|frame| RaisedRecursiveFrame {
                    branch: frame.branch,
                    decided: frame.decided,
                    check: type_expr_to_key(interner, &frame.check),
                    extends: type_expr_to_key(interner, &frame.extends),
                })
                .collect(),
        },
        TypeExpr::SyntheticSlotBinding(carrier) => {
            RaisedTerm::SyntheticSlotBinding(Arc::clone(carrier))
        }
        TypeExpr::ImportType {
            specifier,
            qualifier,
            typeof_query,
            type_arguments,
        } => RaisedTerm::ImportType {
            specifier: Arc::clone(specifier),
            qualifier: qualifier.iter().map(Arc::clone).collect(),
            typeof_query: *typeof_query,
            type_arguments: type_arguments
                .iter()
                .map(|a| type_expr_to_key(interner, a))
                .collect(),
        },
        TypeExpr::Unknown(value) => RaisedTerm::Unknown(value.clone()),
    };
    interner.intern(term)
}

fn object_member_to_raised(
    interner: &mut ShapeInterner,
    member: &verter_type_expr::ObjectMember,
) -> RaisedObjectMember {
    use verter_type_expr::ObjectMember;
    match member {
        ObjectMember::Property(property) => RaisedObjectMember::Property {
            key: property.key.clone().map(
                |computed| type_expr_to_key(interner, &computed),
                |identity| identity,
            ),
            ty: type_expr_to_key(interner, &property.ty),
            optional: property.optional,
            readonly: property.readonly,
            visibility: property.visibility,
            excess_origin: property.excess_origin,
            spans: property.spans,
        },
        ObjectMember::Method(method) => RaisedObjectMember::Method {
            key: method.key.clone().map(
                |computed| type_expr_to_key(interner, &computed),
                |identity| identity,
            ),
            function: function_expr_to_raised(interner, &method.function),
            optional: method.optional,
            method_kind: method.method_kind,
            has_implementation_body: method.has_implementation_body,
            visibility: method.visibility,
            excess_origin: method.excess_origin,
            spans: method.spans,
        },
        ObjectMember::Spread(spread) => RaisedObjectMember::Spread {
            ty: type_expr_to_key(interner, &spread.ty),
        },
        ObjectMember::CallSignature(function) => {
            RaisedObjectMember::CallSignature(function_expr_to_raised(interner, function))
        }
        ObjectMember::ConstructSignature(function) => {
            RaisedObjectMember::ConstructSignature(function_expr_to_raised(interner, function))
        }
        ObjectMember::IndexSignature(signature) => RaisedObjectMember::IndexSignature {
            key_name: Arc::from(signature.key_name.as_str()),
            key_type: type_expr_to_key(interner, &signature.key_type),
            value_type: type_expr_to_key(interner, &signature.value_type),
            readonly: signature.readonly,
            spans: signature.spans,
        },
    }
}

fn function_expr_to_raised(
    interner: &mut ShapeInterner,
    function: &verter_type_expr::FunctionExpr,
) -> RaisedFunction {
    let parameters = function
        .parameters
        .iter()
        .map(|p| RaisedFunctionParam {
            name: p.name.as_ref().map(|n| Arc::from(n.as_str())),
            ty: type_expr_to_key(interner, &p.ty),
            optional: p.optional,
            rest: p.rest,
            span: p.span,
        })
        .collect();
    let return_type = function
        .return_type
        .as_ref()
        .map(|r| type_expr_to_key(interner, r));
    let type_parameters = function
        .type_parameters
        .iter()
        .map(|tp| RaisedTypeParam {
            name: Arc::from(tp.name.as_str()),
            constraint: tp
                .constraint
                .as_ref()
                .map(|c| type_expr_to_key(interner, c)),
            default: tp.default.as_ref().map(|d| type_expr_to_key(interner, d)),
            is_const: tp.is_const,
        })
        .collect();
    RaisedFunction {
        parameters,
        return_type,
        type_parameters,
        signature_span: function.spans.signature,
        return_type_span: function.spans.return_type,
    }
}

// ===========================================================================
// Root-only projection — the NORMALIZED raised-ROOT class WITHOUT folding
// member values. The root-shape mirrors (`project_node_root_kind` and the
// per-fact classifiers it backs) need ONLY the post-normalized root class, not
// the whole-tree `materialized` AND the full fold also computes; this projection
// skips the deep walk for the perf win on large surfaces.
// ===========================================================================

/// Placeholder child facts for [`project_root_summary`]. The shared [`summary`]
/// constructors fold child `materialized` / `expanded_surface` ONLY into the
/// `facts` field that [`RootOnlySummary::from_summary`] STRIPS — the root-only
/// projection keeps `root_kind` / `tag` / `root_semantic_miss_sentinel` ONLY, none
/// of which depends on a child's fact VALUES (each constructor sets `root_kind` /
/// `tag` by WHICH constructor runs, plus `mapped`'s `value_root_semantic_miss` and
/// `object_from_members`'s `is_empty`, both passed REAL). So feeding a constant
/// here is exact — it avoids the deep fold whose only product would be stripped.
fn root_only_placeholder_facts() -> RaisedShapeFacts {
    RaisedShapeFacts {
        can_shell_raise: true,
        materialized: true,
        expanded_surface: true,
    }
}

/// `true` when surface signature `sig` raises to a `Function` shape — the
/// root-only equivalent of the `out_as_function(fold_member(sig)).is_some()`
/// gate `fold_surface_view` applies to a call / construct signature (tag
/// `Function`). Folds the signature root-only with a FRESH cycle set (matching
/// `fold_member`'s fresh-per-member `active`).
fn signature_raises_to_function(
    dispatch: &ProjectSemanticDispatch<'_>,
    sig: SemanticNodeId,
) -> bool {
    let mut active = FxHashSet::default();
    project_root_summary(dispatch, sig, &mut active)
        .is_some_and(|s| s.tag == FactShapeTag::Function)
}

/// The NARROW [`RootOnlySummary`] of `node` — the root-only counterpart of
/// [`fold_node`](super::fold_node) under [`RaisedFactsAlg`], computed WITHOUT
/// folding member VALUES.
///
/// A node's `root_kind` / `tag` / `root_semantic_miss_sentinel` depend ONLY on
/// the node's own kind plus its ROOT-determining edges — the `Alias` peel, the
/// `Intersection` sentinel/empty-object arm-drop + 0/1/many collapse, the
/// `MergedDecl` reduction, the `ConstructorType` signature class, the `Object`
/// surface's empty / single-call-signature / member-presence shape, and the
/// `Mapped` VALUE's own root `semanticMiss` flag — NEVER on object property
/// values, function param/return types, union members, or carrier type-arguments.
/// This projection therefore builds each result through the SHARED [`summary`]
/// constructor layer (feeding [`root_only_placeholder_facts`] that
/// [`RootOnlySummary::from_summary`] then strips), so its returned ROOT FIELDS
/// MATCH THE FULL FOLD's by construction. The whole-subtree walk the full fold
/// pays to compute the (here-stripped) `materialized` AND is skipped — the perf
/// win for a large object surface (an `Object` with N members raises to an
/// `Object` root after an O(1) shallow check, not an O(tree) walk).
///
/// Returns `None` when [`fold_node`](super::fold_node) returns `None` for `node`,
/// on every WELL-FORMED node and on the dangling-required-child cases the
/// malformed-child parity test covers. Both bottom out at `node_data_for(node)?`,
/// and this projection propagates the SAME required-edge `?` aborts `fold_node`
/// does — `Array.element`, `KeyOf.base`, `IndexedAccess.object` + a `TypeNode`
/// `index`, every `Conditional` operand, `Mapped`'s source / value / name-remap,
/// the `ConstructorType` signature, and the `Alias` / `MergedDecl` peel — while
/// LEAVING object member values short-circuited (the full fold wraps a missing
/// member as a sentinel, never `None`, so a root-only deep member walk would
/// FALSELY diverge). The two therefore agree on `Some` / `None` for every
/// well-formed node and for the dangling-required-child cases the parity test
/// pins. ONE honest asymmetry, NOT an exact `None`-iff-`None` equivalence: a
/// malformed / headless `TypeOf` carrier makes `fold_node` `.expect()`-panic on
/// its missing head, whereas this root-only projection reads no head and returns
/// `Some` — the projection is the strictly more lenient / safe side, so it never
/// panics where the full fold would. Root fields pinned equal to the full fold's
/// by the parity test.
pub(super) fn project_root_summary(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<RootOnlySummary> {
    let ctx = dispatch.ctx;
    let data = node_data_for(ctx, node)?;
    Some(match data.as_ref() {
        SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::Infer { .. }
        | SemanticNodeData::InferRef { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::ImportType(_)
        | SemanticNodeData::SyntheticBinding { .. }
        | SemanticNodeData::DeferredCallable(_) => {
            RootOnlySummary::from_summary(summary::materialized_expanded_leaf())
        }

        // The `Ref` carriers + the `DeclPlaceholder` carrier raise to
        // `TypeExpr::Ref` (a published-operator surface root).
        SemanticNodeData::DeclRef { .. }
        | SemanticNodeData::InstantiationRef { .. }
        | SemanticNodeData::BareRef(_)
        | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. }) => {
            RootOnlySummary::from_summary(summary::reference_leaf())
        }

        SemanticNodeData::TypeOf(_) => RootOnlySummary::from_summary(summary::type_of()),

        // Required-edge `?` parity with `fold_node`, SCOPED to the operator
        // operands (KeyOf base, IndexedAccess object/index, the four Conditional
        // operands, Mapped source/value/name_remap, Array element, ConstructorType
        // signature): a dangling OPERAND aborts BOTH (the root class itself does
        // not read the child — only its `Some`/`None` does).
        //
        // DOCUMENTED ASYMMETRY: `fold_node` ALSO fails a value-composite on a
        // PRESENT-but-unraisable child (union/intersection members, tuple
        // elements, template expressions, function params/return/type-param
        // slots, standalone type-param constraint/default) — this root-only
        // projection deliberately does NOT mirror those edges (it classifies the
        // ROOT shape from placeholder facts, it is NOT a raisability oracle), so
        // it returns `Some` for exactly those malformed composites the full fold
        // fails. The asymmetry is pinned in
        // `root_only_projection_returns_none_on_malformed_required_child_like_full_fold`.
        // Object member values stay short-circuited below, with no member
        // deep-walk.
        SemanticNodeData::KeyOf { base } => {
            project_root_summary(dispatch, *base, active)?;
            RootOnlySummary::from_summary(summary::key_of(root_only_placeholder_facts()))
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            project_root_summary(dispatch, *object, active)?;
            if let IndexKey::Computed(index_node) = index {
                project_root_summary(dispatch, *index_node, active)?;
            }
            RootOnlySummary::from_summary(summary::indexed_access(
                root_only_placeholder_facts(),
                root_only_placeholder_facts(),
            ))
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            ..
        } => {
            project_root_summary(dispatch, *check, active)?;
            project_root_summary(dispatch, *extends, active)?;
            project_root_summary(dispatch, *true_branch_ref, active)?;
            project_root_summary(dispatch, *false_branch_ref, active)?;
            RootOnlySummary::from_summary(summary::conditional(
                root_only_placeholder_facts(),
                root_only_placeholder_facts(),
                root_only_placeholder_facts(),
                root_only_placeholder_facts(),
            ))
        }
        // Root-classification only (see the DOCUMENTED ASYMMETRY note above):
        // function params/return/type-param slots and tuple/union members are
        // NOT probed — the full fold fails those malformed composites while
        // this projection still answers the root class.
        SemanticNodeData::Signature { kind, .. } => match kind {
            crate::semantic_query::SignatureKind::Call => {
                RootOnlySummary::from_summary(summary::function(true))
            }
            crate::semantic_query::SignatureKind::Construct => {
                RootOnlySummary::from_summary(summary::constructor(true))
            }
        },
        SemanticNodeData::Array { element, .. } => {
            project_root_summary(dispatch, *element, active)?;
            RootOnlySummary::from_summary(summary::array(root_only_placeholder_facts()))
        }
        SemanticNodeData::Tuple { .. } => {
            RootOnlySummary::from_summary(summary::tuple(std::iter::empty::<RaisedShapeFacts>()))
        }
        SemanticNodeData::Union(_) => {
            RootOnlySummary::from_summary(summary::union(std::iter::empty::<RaisedShapeFacts>()))
        }
        SemanticNodeData::RawFallback { value } => {
            RootOnlySummary::from_summary(summary::unknown(value))
        }

        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return Some(RootOnlySummary::from_summary(summary::opaque_sentinel(
                    &QueryError::RaiseAliasCycle,
                )));
            }
            let result = project_root_summary(dispatch, *target, active);
            active.remove(&node);
            return result;
        }
        SemanticNodeData::MergedDecl { contributors } => {
            let merged = crate::project_semantic_dispatch::walk::reduce_merged_decl_with_graph(
                dispatch.graph(),
                contributors,
            );
            return project_root_summary(dispatch, merged, active);
        }
        SemanticNodeData::Intersection(members) => {
            // filter_map recurse (root-only), drop the ObjectSurfaceSentinel +
            // empty-object arms, then collapse: empty -> empty object,
            // len==1 -> that arm, else Intersection — the same COLLAPSE shape as
            // `fold_node`'s Intersection arm, but each arm classified root-only,
            // and a DANGLING arm is dropped here while the full fold fails the
            // whole composite (the DOCUMENTED ASYMMETRY).
            let mut arms: Vec<RootOnlySummary> = members
                .iter()
                .filter_map(|member| project_root_summary(dispatch, *member, active))
                .collect();
            arms.retain(|arm| {
                arm.tag != FactShapeTag::ObjectSurfaceSentinel
                    && arm.tag != FactShapeTag::EmptyObject
            });
            if arms.is_empty() {
                RootOnlySummary::from_summary(summary::empty_object())
            } else if arms.len() == 1 {
                arms.into_iter().next().unwrap()
            } else {
                RootOnlySummary::from_summary(summary::intersection(std::iter::empty::<
                    RaisedShapeFacts,
                >()))
            }
        }
        SemanticNodeData::Mapped { mapper, .. } => {
            // Required-edge `?` parity with `fold_node`'s Mapped arm: the source
            // (`mapper.key_space`; a `KeyOf` node is `None` iff its base is, so
            // recursing `key_space` root-only gives the IDENTICAL `None` the
            // KeyOf-aware `base` fold would), the VALUE, and the optional
            // name-remap all propagate. Only the VALUE's OWN root `semanticMiss`
            // flag feeds the root class.
            project_root_summary(dispatch, mapper.key_space, active)?;
            let value = project_root_summary(dispatch, mapper.value_expr, active)?;
            if let Some(remap) = mapper.name_remap {
                project_root_summary(dispatch, remap, active)?;
            }
            RootOnlySummary::from_summary(summary::mapped(
                root_only_placeholder_facts(),
                root_only_placeholder_facts(),
                None,
                value.root_semantic_miss_sentinel,
            ))
        }
        SemanticNodeData::Object(surface) => {
            if surface.closed().is_empty() {
                RootOnlySummary::from_summary(summary::empty_object())
            } else if surface.positive_members().is_empty()
                && surface.construct_signatures.is_empty()
                && !surface.has_known_index_signature()
                && surface.call_signatures.len() == 1
            {
                // Single-call-signature surface IS that signature's value
                // (`fold_surface_view`'s fast path), folded with a FRESH cycle set.
                // A miss becomes the surface-member sentinel (NOT `None`), exactly
                // as `fold_member`'s `unwrap_or_else` does — so the Object arm
                // never propagates `None`.
                let mut member_active = FxHashSet::default();
                project_root_summary(dispatch, surface.call_signatures[0], &mut member_active)
                    .unwrap_or_else(|| {
                        RootOnlySummary::from_summary(summary::opaque_sentinel(
                            &QueryError::UnrepresentableSurfaceMember,
                        ))
                    })
            } else {
                // A representable member survives iff there is a property, a
                // concrete-or-open index signature, or a call / construct
                // signature that raises to a `Function` — exactly
                // `fold_surface_view`'s member-set non-emptiness. A surviving
                // surface raises to an `Object` root; an empty one becomes the
                // `UnrepresentableSurface` sentinel (tag `ObjectSurfaceSentinel`,
                // root `Other`). The signature scan runs ONLY when a property /
                // index has not already settled it (short-circuit `||`).
                let has_member = !surface.positive_members().is_empty()
                    || !surface.index_signatures.is_empty()
                    || surface.has_known_index_signature()
                    || surface
                        .call_signatures
                        .iter()
                        .any(|sig| signature_raises_to_function(dispatch, *sig))
                    || surface
                        .construct_signatures
                        .iter()
                        .any(|sig| signature_raises_to_function(dispatch, *sig));
                if has_member {
                    RootOnlySummary::from_summary(summary::object_from_members(
                        std::iter::empty::<bool>(),
                        false,
                    ))
                } else {
                    RootOnlySummary::from_summary(summary::opaque_sentinel(
                        &QueryError::UnrepresentableSurface,
                    ))
                }
            }
        }
        SemanticNodeData::ObjectSpreadProgram(_) => RootOnlySummary::from_summary(
            summary::opaque_sentinel(&QueryError::UnrepresentableSurface),
        ),
        SemanticNodeData::Opaque(err) => match err {
            QueryError::RecursiveRef { .. } => {
                RootOnlySummary::from_summary(summary::materialized_expanded_leaf())
            }
            _ => RootOnlySummary::from_summary(summary::opaque_sentinel(err)),
        },
    })
}

/// `true` when `node` is an interned resolver-control FAILURE carrier — a
/// [`SemanticNodeData::Opaque`] whose shell materialization renders
/// `Unknown { raw }` (`Miss`, `BudgetExceeded`, `Other(..)`, …). The two
/// legitimately-publishable opaque carriers are NOT failures and stay
/// `false`: `RecursiveRef` raises to `TypeExpr::RecursiveRef` and
/// `DeclPlaceholder` raises to the named `Ref` shell — mirroring the
/// `fold_node` `Opaque` conduit and the root-summary `Opaque` arm above,
/// held in agreement by proximity. Raise boundaries use this to FAIL
/// CLOSED: a projection that "succeeds" onto such a node is a projection
/// MISS and must answer `None` instead of handing out a node whose
/// publication would silently read `Unknown`.
pub(in crate::project_semantic_dispatch) fn node_is_unknown_materializing_failure(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> bool {
    // Derived from the SINGLE `QueryError` disposition authority — never a
    // local re-listing of which arms are publishable.
    matches!(
        node_data_for(dispatch.ctx, node).as_deref(),
        Some(SemanticNodeData::Opaque(err))
            if crate::project_semantic_dispatch::query_error_disposition::query_error_disposition(
                err,
            )
            .is_unknown_materializing()
    )
}

#[cfg(test)]
#[path = "node_domain_tests.rs"]
mod tests;
