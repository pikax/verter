//! Algebra 2 + 2-facts + 3 of the shared shape engine:
//! - `RaisedShapeAlg` (bottom-up facts/key, interns a structural key),
//! - `RaisedFactsAlg` (the SAME bottom-up facts, NO key interning — for the
//!   facts-only route gates),
//! - `type_expr_to_key` (folding an existing `&TypeExpr` into the same interned
//!   key space).
//!
//! The per-arm FACT + TAG formulas live ONCE in the [`summary`] constructor
//! layer; both `RaisedShapeAlg` and `RaisedFactsAlg` build their per-arm values
//! through it, so the two can never drift (parity is structural). Split from the
//! parent for file-size; the fold + the algebra trait + the interned term live
//! in the parent `shape_engine` module.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_type_expr::{LiteralValue, MappedModifier, MemberVisibility, PrimitiveName, TypeExpr};

use super::{
    FactShapeTag, FoldedFunction, FoldedTupleElement, RaisedFunction, RaisedFunctionParam,
    RaisedObjectMember, RaisedRecursiveFrame, RaisedRootKind, RaisedShapeAlgebra, RaisedShapeFacts,
    RaisedShapeKey, RaisedShapeResult, RaisedShapeSummary, RaisedTerm, RaisedTupleElement,
    RaisedTypeParam, ShapeInterner,
};
use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::resolver_core::component_meta_query_engine::{
    semantic_query_error_raw, SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE,
};
use crate::semantic_query::{QueryError, SemanticNodeData, SemanticNodeId};

// ===========================================================================
// Shared summary-constructor layer — the SINGLE source of the per-arm
// fact + tag formulas. Both `RaisedShapeAlg` (which additionally interns a
// structural key) and `RaisedFactsAlg` (which does not) build their per-arm
// [`RaisedShapeSummary`] through these pure functions, so the
// `materialized` / `expanded_surface` / `tag` rules can never drift between the
// two algebras. The functions take ONLY the child facts they fold (never a key
// or interner), exactly mirroring the historical inline `RaisedShapeAlg` arms.
// ===========================================================================
mod summary {
    use super::{
        FactShapeTag, RaisedRootKind, RaisedShapeFacts, RaisedShapeSummary, SEMANTIC_MISS,
        SEMANTIC_OBJECT_SURFACE,
    };

    /// Assemble a summary from the three facts + tag. `can_shell_raise` is
    /// ALWAYS `true` for any value the fold produces (a `Some(result)`).
    fn summary(
        materialized: bool,
        expanded_surface: bool,
        tag: FactShapeTag,
    ) -> RaisedShapeSummary {
        RaisedShapeSummary {
            facts: RaisedShapeFacts {
                can_shell_raise: true,
                materialized,
                expanded_surface,
            },
            tag,
            // Only the two sentinel-leaf constructors (`unknown` / `opaque_sentinel`)
            // set these true; every compound / non-sentinel term is `false` (its
            // ROOT is not a sentinel, even when a child is).
            root_unmaterialized_sentinel: false,
            root_semantic_miss_sentinel: false,
            // Default `Other`; the per-arm constructors that map to a root mirror
            // class (`reference_leaf` / `type_of` / `empty_object` / `key_of` /
            // `indexed_access` / `conditional` / `mapped` / `object_from_members`)
            // override it.
            root_kind: RaisedRootKind::Other,
        }
    }

    /// A materialized, expanded leaf with no special tag and no root-mirror class
    /// (Primitive, Literal, Infer, RecursiveRef, SyntheticSlotBinding, ImportType,
    /// TemplateLiteral, TypeParameter). A `Ref` carrier uses [`reference_leaf`]
    /// instead so it carries [`RaisedRootKind::Reference`].
    pub(super) fn materialized_expanded_leaf() -> RaisedShapeSummary {
        summary(true, true, FactShapeTag::Other)
    }

    /// A `Ref`-carrier leaf (`DeclRef` / `InstantiationRef` / `BareRef` /
    /// `DeclPlaceholder`): facts/tag identical to [`materialized_expanded_leaf`]
    /// (materialized + expanded, `Other` tag) — only `root_kind` differs, marking
    /// the root as a `TypeExpr::Ref` (a published-operator surface root).
    pub(super) fn reference_leaf() -> RaisedShapeSummary {
        let mut s = summary(true, true, FactShapeTag::Other);
        s.root_kind = RaisedRootKind::Reference;
        s
    }

    /// `Unknown { raw }`: materialized iff the raw is NOT an unmaterialized
    /// sentinel; an expanded leaf; tagged `ObjectSurfaceSentinel` iff the raw is
    /// exactly the object-surface sentinel (dropped from an intersection).
    pub(super) fn unknown(raw: &str) -> RaisedShapeSummary {
        let materialized =
            !crate::project_semantic_dispatch::raise_sentinel::raw_is_unmaterialized_sentinel(raw);
        let tag = if raw == SEMANTIC_OBJECT_SURFACE {
            FactShapeTag::ObjectSurfaceSentinel
        } else {
            FactShapeTag::Other
        };
        let mut s = summary(materialized, true, tag);
        // The ROOT term IS this `Unknown { raw }`, so it is a root sentinel iff the
        // raw reads unmaterialised (`!materialized`), and the NARROWER miss-root iff
        // the raw is EXACTLY the `semanticMiss` spelling.
        s.root_unmaterialized_sentinel = !materialized;
        s.root_semantic_miss_sentinel = raw == SEMANTIC_MISS;
        s
    }

    /// A TYPED resolver-control sentinel (`Opaque(QueryError)` reaching the
    /// reverse boundary, or a converted `fold_node` control arm): the
    /// node-domain counterpart of [`unknown`], but classified DIRECTLY from the
    /// typed [`QueryError`] via the shared sentinel authority instead of
    /// re-spelling a raw string. `materialized` comes from the domain-neutral
    /// `query_error_is_unmaterialized_sentinel`; the `tag` is mapped HERE — this
    /// is where [`FactShapeTag`] lives — from the domain-neutral
    /// `query_error_is_object_surface_sentinel` predicate, exactly mirroring the
    /// `raw == SEMANTIC_OBJECT_SURFACE` tag rule [`unknown`] applies (the
    /// `UnrepresentableSurface` carrier round-trips to that spelling natively, and
    /// a text-bearing `Other("semanticObjectSurface")` payload round-trips to it
    /// via the predicate's delegation — both tag `ObjectSurfaceSentinel`, exactly
    /// as the raw rule would). Both predicates are held byte-for-byte in agreement
    /// with the raw recogniser `unknown` uses (the no-drift contract), so this
    /// path and the raw-string path classify a sentinel identically.
    /// `expanded_surface` is always `true`, exactly as `unknown` passes.
    pub(super) fn opaque_sentinel(err: &crate::semantic_query::QueryError) -> RaisedShapeSummary {
        use crate::project_semantic_dispatch::raise_sentinel::{
            query_error_is_object_surface_sentinel, query_error_is_semantic_miss_sentinel,
            query_error_is_unmaterialized_sentinel,
        };
        let materialized = !query_error_is_unmaterialized_sentinel(err);
        let tag = if query_error_is_object_surface_sentinel(err) {
            FactShapeTag::ObjectSurfaceSentinel
        } else {
            FactShapeTag::Other
        };
        let mut s = summary(materialized, true, tag);
        // The ROOT term IS this typed sentinel, so it is a root sentinel iff the
        // error reads unmaterialised (`!materialized`), and the NARROWER miss-root
        // iff the error round-trips to the `semanticMiss` spelling — classified
        // DIRECTLY from the typed variant via the shared authority, never by
        // re-spelling a raw string.
        s.root_unmaterialized_sentinel = !materialized;
        s.root_semantic_miss_sentinel = query_error_is_semantic_miss_sentinel(err);
        s
    }

    /// `TypeOf`: a materialized leaf but NOT an expanded surface.
    pub(super) fn type_of() -> RaisedShapeSummary {
        let mut s = summary(true, false, FactShapeTag::Other);
        s.root_kind = RaisedRootKind::TypeOf;
        s
    }

    /// `Union`: materialized / expanded are the AND over all members.
    pub(super) fn union(
        member_facts: impl Iterator<Item = RaisedShapeFacts>,
    ) -> RaisedShapeSummary {
        let (mut materialized, mut expanded) = (true, true);
        for f in member_facts {
            materialized &= f.materialized;
            expanded &= f.expanded_surface;
        }
        summary(materialized, expanded, FactShapeTag::Other)
    }

    /// `Intersection`: materialized / expanded are the AND over all surviving
    /// arms (the fold has already dropped sentinel / empty-object arms).
    pub(super) fn intersection(
        arm_facts: impl Iterator<Item = RaisedShapeFacts>,
    ) -> RaisedShapeSummary {
        let (mut materialized, mut expanded) = (true, true);
        for f in arm_facts {
            materialized &= f.materialized;
            expanded &= f.expanded_surface;
        }
        summary(materialized, expanded, FactShapeTag::Other)
    }

    /// The representable empty object `{}` — raises to `TypeExpr::Object([])`.
    pub(super) fn empty_object() -> RaisedShapeSummary {
        let mut s = summary(true, true, FactShapeTag::EmptyObject);
        s.root_kind = RaisedRootKind::Object;
        s
    }

    /// `Array`: recurses `materialized` into its element; an expanded surface.
    pub(super) fn array(element: RaisedShapeFacts) -> RaisedShapeSummary {
        summary(element.materialized, true, FactShapeTag::Other)
    }

    /// `Tuple`: materialized is the AND over all elements; an expanded surface.
    pub(super) fn tuple(
        element_facts: impl Iterator<Item = RaisedShapeFacts>,
    ) -> RaisedShapeSummary {
        let materialized = element_facts.fold(true, |acc, f| acc & f.materialized);
        summary(materialized, true, FactShapeTag::Other)
    }

    /// `KeyOf`: recurses `materialized` into its inner; NOT an expanded surface.
    pub(super) fn key_of(base: RaisedShapeFacts) -> RaisedShapeSummary {
        let mut s = summary(base.materialized, false, FactShapeTag::Other);
        s.root_kind = RaisedRootKind::KeyOf;
        s
    }

    /// `IndexedAccess`: materialized iff BOTH object + index are; NOT an
    /// expanded surface.
    pub(super) fn indexed_access(
        object: RaisedShapeFacts,
        index: RaisedShapeFacts,
    ) -> RaisedShapeSummary {
        let mut s = summary(
            object.materialized && index.materialized,
            false,
            FactShapeTag::Other,
        );
        s.root_kind = RaisedRootKind::IndexedAccess;
        s
    }

    /// `Conditional`: materialized iff ALL of check / extends / true / false
    /// are; NOT an expanded surface.
    pub(super) fn conditional(
        check: RaisedShapeFacts,
        extends: RaisedShapeFacts,
        true_type: RaisedShapeFacts,
        false_type: RaisedShapeFacts,
    ) -> RaisedShapeSummary {
        let materialized = check.materialized
            && extends.materialized
            && true_type.materialized
            && false_type.materialized;
        let mut s = summary(materialized, false, FactShapeTag::Other);
        s.root_kind = RaisedRootKind::Conditional;
        s
    }

    /// `Mapped`: materialized iff source + value (+ name_type, when present)
    /// are; NOT an expanded surface. `value_root_semantic_miss` is the mapped
    /// VALUE's OWN raised-root `semanticMiss` fact, carried into the root class so
    /// the published-operator classifier suppresses EXACTLY the
    /// `value == Unknown { raw == "semanticMiss" }` carrier the `TypeExpr`
    /// predicate suppresses (publishing for any other value).
    pub(super) fn mapped(
        source: RaisedShapeFacts,
        value: RaisedShapeFacts,
        name_type: Option<RaisedShapeFacts>,
        value_root_semantic_miss: bool,
    ) -> RaisedShapeSummary {
        let materialized =
            source.materialized && value.materialized && name_type.is_none_or(|n| n.materialized);
        let mut s = summary(materialized, false, FactShapeTag::Other);
        s.root_kind = RaisedRootKind::Mapped {
            value_is_semantic_miss: value_root_semantic_miss,
        };
        s
    }

    /// `Function`: carries the function's folded `materialized` fact; an
    /// expanded surface; tagged `Function` (the `out_as_function` extraction
    /// subject + the constructor-rewrap signature child).
    pub(super) fn function(materialized: bool) -> RaisedShapeSummary {
        summary(materialized, true, FactShapeTag::Function)
    }

    /// `ConstructorType`: carries the signature's folded `materialized` fact; an
    /// expanded surface; tagged `Other` (the rewrap reads the SIGNATURE child,
    /// never the constructor itself, so it must NOT tag `Function`).
    pub(super) fn constructor(materialized: bool) -> RaisedShapeSummary {
        summary(materialized, true, FactShapeTag::Other)
    }

    /// `Object` from surviving members: materialized is the AND over members; an
    /// expanded surface; tagged `EmptyObject` when zero members survive
    /// (defensive — mirrors the interner-readback of `Object([])`), else `Other`.
    pub(super) fn object_from_members(
        member_materialized: impl Iterator<Item = bool>,
        is_empty: bool,
    ) -> RaisedShapeSummary {
        let materialized = member_materialized.fold(true, |acc, m| acc & m);
        let tag = if is_empty {
            FactShapeTag::EmptyObject
        } else {
            FactShapeTag::Other
        };
        let mut s = summary(materialized, true, tag);
        s.root_kind = RaisedRootKind::Object;
        s
    }
}

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
    fn unknown(&mut self, raw: Arc<str>) -> RaisedShapeResult {
        let summary = summary::unknown(&raw);
        self.result(RaisedTerm::Unknown { raw }, summary)
    }
    fn opaque_sentinel(&mut self, err: &QueryError) -> RaisedShapeResult {
        // The interned STRUCTURAL key is the same `Unknown { raw }` the
        // materializer produces (byte-identical raw, so node-vs-`TypeExpr`
        // equality is preserved); the SUMMARY is classified from the typed
        // variant via the shared authority.
        let summary = summary::opaque_sentinel(err);
        let raw: Arc<str> = Arc::from(semantic_query_error_raw(err));
        self.result(RaisedTerm::Unknown { raw }, summary)
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

    fn member_property(
        &mut self,
        name: String,
        ty: RaisedShapeResult,
        optional: bool,
        readonly: bool,
        visibility: MemberVisibility,
        spans: verter_type_expr::MemberSpans,
    ) -> RaisedMember {
        RaisedMember {
            materialized: ty.summary.facts.materialized,
            member: RaisedObjectMember::Property {
                name: Arc::from(name),
                ty: ty.key,
                optional,
                readonly,
                visibility,
                spans,
            },
        }
    }
    fn member_method(
        &mut self,
        name: String,
        function: (RaisedFunction, bool),
        optional: bool,
        visibility: MemberVisibility,
        spans: verter_type_expr::MemberSpans,
    ) -> RaisedMember {
        let (function, materialized) = function;
        RaisedMember {
            materialized,
            member: RaisedObjectMember::Method {
                name: Arc::from(name),
                function,
                optional,
                visibility,
                spans,
            },
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
    fn unknown(&mut self, raw: Arc<str>) -> RaisedShapeSummary {
        summary::unknown(&raw)
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

    fn member_property(
        &mut self,
        _name: String,
        ty: RaisedShapeSummary,
        _optional: bool,
        _readonly: bool,
        _visibility: MemberVisibility,
        _spans: verter_type_expr::MemberSpans,
    ) -> FactsMember {
        FactsMember {
            materialized: ty.facts.materialized,
        }
    }
    fn member_method(
        &mut self,
        _name: String,
        function: FactsFunction,
        _optional: bool,
        _visibility: MemberVisibility,
        _spans: verter_type_expr::MemberSpans,
    ) -> FactsMember {
        FactsMember {
            materialized: function.materialized,
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
        TypeExpr::Unknown { raw } => RaisedTerm::Unknown {
            raw: Arc::from(raw.as_str()),
        },
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
            name: Arc::from(property.name.as_str()),
            ty: type_expr_to_key(interner, &property.ty),
            optional: property.optional,
            readonly: property.readonly,
            visibility: property.visibility,
            spans: property.spans,
        },
        ObjectMember::Method(method) => RaisedObjectMember::Method {
            name: Arc::from(method.name.as_str()),
            function: function_expr_to_raised(interner, &method.function),
            optional: method.optional,
            visibility: method.visibility,
            spans: method.spans,
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
/// discarded `facts` field — the root-only projection reads `root_kind` / `tag`
/// / `root_semantic_miss_sentinel` ONLY, none of which depends on a child's fact
/// VALUES (each constructor sets `root_kind` / `tag` by WHICH constructor runs,
/// plus `mapped`'s `value_root_semantic_miss` and `object_from_members`'s
/// `is_empty`, both passed REAL). So feeding a constant here is exact — it avoids
/// the deep fold whose only product would be thrown away.
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

/// The NORMALIZED raised-ROOT [`RaisedShapeSummary`] of `node` — the root-only
/// counterpart of [`fold_node`](super::fold_node) under [`RaisedFactsAlg`],
/// computed WITHOUT folding member VALUES.
///
/// A node's `root_kind` / `tag` / `root_semantic_miss_sentinel` depend ONLY on
/// the node's own kind plus its ROOT-determining edges — the `Alias` peel, the
/// `Intersection` sentinel/empty-object arm-drop + 0/1/many collapse, the
/// `MergedDecl` reduction, the `ConstructorType` signature class, the `Object`
/// surface's empty / single-call-signature / member-presence shape, and the
/// `Mapped` VALUE's own root `semanticMiss` flag — NEVER on object property
/// values, function param/return types, array/tuple element types, union
/// members, conditional branches, indexed-access operands, keyof bases, or
/// carrier type-arguments. This projection therefore recurses ONLY along those
/// root edges and builds each result through the SHARED [`summary`] constructor
/// layer (feeding [`root_only_placeholder_facts`] it discards), so its
/// `root_kind` / `tag` / `root_semantic_miss_sentinel` are BYTE-IDENTICAL to the
/// full fold's by construction. The whole-subtree walk the full fold pays to
/// compute the (here-unused) `materialized` AND is skipped — the perf win for a
/// large object surface (an `Object` with N members raises to an `Object` root
/// after an O(1) shallow check, not an O(tree) walk).
///
/// `None` exactly when [`fold_node`](super::fold_node) returns `None` for
/// `node`. Both bottom out at `node_data_for(node)?`; the append-only interned
/// arena guarantees every present node's edges reference present nodes (the
/// invariant the whole-tree arm-walkers rely on), so the root edges this
/// projection follows are never `None` when `node` is present and the two agree
/// on `Some` / `None` for every well-formed node. Pinned equal to the full
/// fold's `root_kind` by the parity test.
pub(super) fn project_root_summary(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<RaisedShapeSummary> {
    let ctx = dispatch.ctx;
    let data = node_data_for(ctx, node)?;
    Some(match data.as_ref() {
        SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::Infer { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::ImportType(_)
        | SemanticNodeData::SyntheticBinding { .. } => summary::materialized_expanded_leaf(),

        // The `Ref` carriers + the `DeclPlaceholder` carrier raise to
        // `TypeExpr::Ref` (a published-operator surface root).
        SemanticNodeData::DeclRef { .. }
        | SemanticNodeData::InstantiationRef { .. }
        | SemanticNodeData::BareRef(_)
        | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. }) => summary::reference_leaf(),

        SemanticNodeData::TypeOf(_) => summary::type_of(),
        SemanticNodeData::KeyOf { .. } => summary::key_of(root_only_placeholder_facts()),
        SemanticNodeData::IndexedAccess { .. } => {
            summary::indexed_access(root_only_placeholder_facts(), root_only_placeholder_facts())
        }
        SemanticNodeData::Conditional { .. } => summary::conditional(
            root_only_placeholder_facts(),
            root_only_placeholder_facts(),
            root_only_placeholder_facts(),
            root_only_placeholder_facts(),
        ),
        SemanticNodeData::Function { .. } => summary::function(true),
        SemanticNodeData::Array { .. } => summary::array(root_only_placeholder_facts()),
        SemanticNodeData::Tuple { .. } => summary::tuple(std::iter::empty::<RaisedShapeFacts>()),
        SemanticNodeData::Union(_) => summary::union(std::iter::empty::<RaisedShapeFacts>()),
        SemanticNodeData::RawFallback { raw } => summary::unknown(raw),
        SemanticNodeData::VueMacroElements(_) => {
            summary::opaque_sentinel(&QueryError::VueMacroElementsPlaceholder)
        }

        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return Some(summary::opaque_sentinel(&QueryError::RaiseAliasCycle));
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
            // filter_map recurse (root-only), drop the SEMANTIC_OBJECT_SURFACE
            // sentinel + empty-object arms, then collapse: empty -> empty object,
            // len==1 -> that arm, else Intersection — identical to `fold_node`'s
            // Intersection arm, but each arm classified root-only.
            let mut arms: Vec<RaisedShapeSummary> = members
                .iter()
                .filter_map(|member| project_root_summary(dispatch, *member, active))
                .collect();
            arms.retain(|arm| {
                arm.tag != FactShapeTag::ObjectSurfaceSentinel
                    && arm.tag != FactShapeTag::EmptyObject
            });
            if arms.is_empty() {
                summary::empty_object()
            } else if arms.len() == 1 {
                arms.into_iter().next().unwrap()
            } else {
                summary::intersection(std::iter::empty::<RaisedShapeFacts>())
            }
        }
        SemanticNodeData::Mapped { mapper, .. } => {
            // Only the mapped VALUE's OWN root `semanticMiss` flag feeds the
            // root class; the source / name_type subtrees do not. Mirror
            // `fold_node`'s `?` on the value.
            let value = project_root_summary(dispatch, mapper.value_expr, active)?;
            summary::mapped(
                root_only_placeholder_facts(),
                root_only_placeholder_facts(),
                None,
                value.root_semantic_miss_sentinel,
            )
        }
        SemanticNodeData::ConstructorType { signature } => {
            // The rewrap reads the SIGNATURE child: a `Function` signature
            // rewraps to a `ConstructorType` (root Other); any other signature
            // shape passes through unchanged (its own root class). Mirror
            // `fold_node`'s `?` on the signature.
            let signature = project_root_summary(dispatch, *signature, active)?;
            if signature.tag == FactShapeTag::Function {
                summary::constructor(true)
            } else {
                signature
            }
        }
        SemanticNodeData::Object(surface) => {
            if surface.members.is_empty()
                && surface.call_signatures.is_empty()
                && surface.construct_signatures.is_empty()
                && !surface.has_index_signature
            {
                summary::empty_object()
            } else if surface.members.is_empty()
                && surface.construct_signatures.is_empty()
                && !surface.has_index_signature
                && surface.call_signatures.len() == 1
            {
                // Single-call-signature surface IS that signature's value
                // (`fold_surface_view`'s fast path), folded with a FRESH cycle set.
                let mut member_active = FxHashSet::default();
                project_root_summary(dispatch, surface.call_signatures[0], &mut member_active)
                    .unwrap_or_else(|| {
                        summary::opaque_sentinel(&QueryError::UnrepresentableSurfaceMember)
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
                let has_member = !surface.members.is_empty()
                    || !surface.index_signatures.is_empty()
                    || surface.has_index_signature
                    || surface
                        .call_signatures
                        .iter()
                        .any(|sig| signature_raises_to_function(dispatch, *sig))
                    || surface
                        .construct_signatures
                        .iter()
                        .any(|sig| signature_raises_to_function(dispatch, *sig));
                if has_member {
                    summary::object_from_members(std::iter::empty::<bool>(), false)
                } else {
                    summary::opaque_sentinel(&QueryError::UnrepresentableSurface)
                }
            }
        }
        SemanticNodeData::Opaque(err) => match err {
            QueryError::RecursiveRef { .. } => summary::materialized_expanded_leaf(),
            _ => summary::opaque_sentinel(err),
        },
    })
}

#[cfg(test)]
#[path = "node_domain_tests.rs"]
mod tests;
