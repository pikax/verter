//! Algebra 1 of the shared shape engine: `MaterializeTypeExprAlg` — the EXACT
//! historical `SemanticNodeId -> TypeExpr` materialization, reached only through
//! the sealed `OutputProjector` output seam and the `#[cfg(test)]` oracle. Split
//! from the parent for file-size; the fold + the algebra trait live in the
//! parent `shape_engine` module.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_type_expr::{LiteralValue, MappedModifier, MemberVisibility, PrimitiveName, TypeExpr};

use super::super::ProjectSemanticDispatch;
use super::{fold_node, FoldedFunction, FoldedTupleElement, RaisedShapeAlgebra};
use crate::resolver_core::component_meta_query_engine::{
    semantic_query_error_raw, SEMANTIC_OBJECT_SURFACE,
};
use crate::semantic_query::{QueryError, SemanticNodeId};

// ===========================================================================
// Algebra 1 — `MaterializeTypeExprAlg` (Out = TypeExpr).
//
// The EXACT historical materialization, reached ONLY through the sealed
// `OutputProjector` output seam and the `#[cfg(test)]` oracle. Each arm
// reproduces the former `raise_node_to_type_expr_core_impl` construction
// byte-for-byte (the byte-identity contract pinned by the raise /
// materialization suite + the 20 raised-shape parity tests).
// ===========================================================================

/// Stateless materialization algebra.
pub(in crate::project_semantic_dispatch) struct MaterializeTypeExprAlg;

impl RaisedShapeAlgebra for MaterializeTypeExprAlg {
    type Out = TypeExpr;
    type Fn = Arc<verter_type_expr::FunctionExpr>;
    type Member = verter_type_expr::ObjectMember;

    fn primitive(&mut self, kind: PrimitiveName) -> TypeExpr {
        TypeExpr::Primitive(kind)
    }
    fn literal(&mut self, value: LiteralValue) -> TypeExpr {
        TypeExpr::Literal(value)
    }
    fn infer(&mut self, name: Arc<str>) -> TypeExpr {
        TypeExpr::Infer {
            name: name.as_ref().to_string(),
        }
    }
    fn unknown(&mut self, raw: Arc<str>) -> TypeExpr {
        TypeExpr::Unknown {
            raw: raw.as_ref().to_string(),
        }
    }
    fn opaque_sentinel(&mut self, err: &QueryError) -> TypeExpr {
        // Byte-for-byte the legacy `Unknown { raw }` string the old hardcoded
        // literal emitted: the materializer is the `TypeExpr`-output domain, so
        // a typed control sentinel re-materialises through the single
        // `semantic_query_error_raw` mapping (the round-trip authority).
        TypeExpr::Unknown {
            raw: semantic_query_error_raw(err),
        }
    }
    fn recursive_ref(&mut self, name: Arc<str>) -> TypeExpr {
        TypeExpr::recursive_ref(name.as_ref(), Vec::new())
    }
    fn reference(&mut self, name: Arc<str>, type_arguments: Vec<TypeExpr>) -> TypeExpr {
        let type_arguments = if type_arguments.is_empty() {
            verter_type_expr::empty_type_args()
        } else {
            Arc::from(type_arguments.into_boxed_slice())
        };
        TypeExpr::Ref {
            name,
            type_arguments,
        }
    }
    fn synthetic_slot_binding(
        &mut self,
        carrier: Arc<verter_type_expr::SyntheticCarrierKey>,
    ) -> TypeExpr {
        TypeExpr::SyntheticSlotBinding(carrier)
    }
    fn import_type(
        &mut self,
        specifier: Arc<str>,
        qualifier: Arc<[Arc<str>]>,
        typeof_query: bool,
        type_arguments: Vec<TypeExpr>,
    ) -> TypeExpr {
        TypeExpr::ImportType {
            specifier,
            qualifier,
            typeof_query,
            type_arguments: Arc::from(type_arguments.into_boxed_slice()),
        }
    }
    fn type_of(&mut self, path: Vec<String>, type_args: Vec<TypeExpr>) -> TypeExpr {
        TypeExpr::TypeOf(verter_type_expr::ValueRef { path, type_args })
    }

    fn union(&mut self, members: Vec<TypeExpr>) -> TypeExpr {
        TypeExpr::Union(Arc::from(members.into_boxed_slice()))
    }
    fn intersection(&mut self, arms: Vec<TypeExpr>) -> TypeExpr {
        TypeExpr::Intersection(Arc::from(arms.into_boxed_slice()))
    }
    fn empty_object(&mut self) -> TypeExpr {
        TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
            properties: Vec::new(),
        }))
    }
    fn array(&mut self, element: TypeExpr, readonly: bool) -> TypeExpr {
        TypeExpr::Array {
            element: Arc::new(element),
            readonly,
        }
    }
    fn tuple(&mut self, elements: Vec<FoldedTupleElement<TypeExpr>>, readonly: bool) -> TypeExpr {
        let elements: Vec<verter_type_expr::TupleElement> = elements
            .into_iter()
            .map(|e| verter_type_expr::TupleElement {
                label: e.label,
                ty: e.ty,
                optional: e.optional,
                rest: e.rest,
            })
            .collect();
        TypeExpr::Tuple {
            elements: Arc::from(elements.into_boxed_slice()),
            readonly,
        }
    }
    fn key_of(&mut self, base: TypeExpr) -> TypeExpr {
        TypeExpr::KeyOf(Arc::new(base))
    }
    fn indexed_access(&mut self, object: TypeExpr, index: TypeExpr) -> TypeExpr {
        TypeExpr::IndexedAccess {
            object: Arc::new(object),
            index: Arc::new(index),
        }
    }
    fn conditional(
        &mut self,
        check: TypeExpr,
        extends: TypeExpr,
        true_type: TypeExpr,
        false_type: TypeExpr,
    ) -> TypeExpr {
        TypeExpr::Conditional {
            check: Arc::new(check),
            extends: Arc::new(extends),
            true_type: Arc::new(true_type),
            false_type: Arc::new(false_type),
        }
    }
    fn mapped(
        &mut self,
        parameter: String,
        source: TypeExpr,
        value: TypeExpr,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<TypeExpr>,
    ) -> TypeExpr {
        TypeExpr::Mapped {
            parameter,
            source: Arc::new(source),
            value: Arc::new(value),
            optional,
            readonly,
            name_type: name_type.map(Arc::new),
        }
    }
    fn template_literal(&mut self, quasis: Vec<String>, expressions: Vec<TypeExpr>) -> TypeExpr {
        TypeExpr::TemplateLiteral {
            quasis,
            expressions: Arc::from(expressions.into_boxed_slice()),
        }
    }
    fn type_parameter(
        &mut self,
        name: Arc<str>,
        constraint: Option<TypeExpr>,
        default: Option<TypeExpr>,
    ) -> TypeExpr {
        TypeExpr::TypeParameter(verter_type_expr::TypeParam {
            name: name.as_ref().to_string(),
            constraint: constraint.map(Arc::new),
            default: default.map(Arc::new),
        })
    }

    fn build_function(
        &mut self,
        function: FoldedFunction<TypeExpr>,
    ) -> Arc<verter_type_expr::FunctionExpr> {
        use verter_type_expr::{FunctionExpr, FunctionParam, FunctionSpans, TypeParam};
        let parameters: Vec<FunctionParam> = function
            .parameters
            .into_iter()
            .map(|p| {
                FunctionParam::with_span(
                    p.name.as_ref().map(|n| n.as_ref().to_string()),
                    p.ty,
                    p.optional,
                    p.rest,
                    p.span,
                    false,
                )
            })
            .collect();
        let type_params: Vec<TypeParam> = function
            .type_parameters
            .into_iter()
            .map(|tp| TypeParam {
                name: tp.name.as_ref().to_string(),
                constraint: tp.constraint.map(Arc::new),
                default: tp.default.map(Arc::new),
            })
            .collect();
        Arc::new(FunctionExpr::with_spans(
            parameters,
            function.return_type.map(Arc::new),
            type_params,
            FunctionSpans {
                signature: function.signature_span,
                return_type: function.return_type_span,
            },
        ))
    }
    fn function_to_out(&mut self, function: Arc<verter_type_expr::FunctionExpr>) -> TypeExpr {
        TypeExpr::Function(function)
    }
    fn constructor_to_out(&mut self, function: Arc<verter_type_expr::FunctionExpr>) -> TypeExpr {
        TypeExpr::ConstructorType(function)
    }
    fn out_as_function(&self, out: &TypeExpr) -> Option<Arc<verter_type_expr::FunctionExpr>> {
        match out {
            TypeExpr::Function(function) => Some(Arc::clone(function)),
            _ => None,
        }
    }

    fn member_property(
        &mut self,
        name: String,
        ty: TypeExpr,
        optional: bool,
        readonly: bool,
        visibility: MemberVisibility,
        spans: verter_type_expr::MemberSpans,
    ) -> verter_type_expr::ObjectMember {
        verter_type_expr::ObjectMember::Property(verter_type_expr::ObjectProperty::with_visibility(
            name, ty, optional, readonly, visibility, spans,
        ))
    }
    fn member_method(
        &mut self,
        name: String,
        function: Arc<verter_type_expr::FunctionExpr>,
        optional: bool,
        visibility: MemberVisibility,
        spans: verter_type_expr::MemberSpans,
    ) -> verter_type_expr::ObjectMember {
        verter_type_expr::ObjectMember::Method(verter_type_expr::MethodSignature::with_visibility(
            name,
            (*function).clone(),
            optional,
            visibility,
            spans,
        ))
    }
    fn member_call_signature(
        &mut self,
        function: Arc<verter_type_expr::FunctionExpr>,
    ) -> verter_type_expr::ObjectMember {
        verter_type_expr::ObjectMember::CallSignature(verter_type_expr::FunctionExpr::with_spans(
            function.parameters.clone(),
            function.return_type.clone(),
            function.type_parameters.clone(),
            function.spans,
        ))
    }
    fn member_construct_signature(
        &mut self,
        function: Arc<verter_type_expr::FunctionExpr>,
    ) -> verter_type_expr::ObjectMember {
        verter_type_expr::ObjectMember::ConstructSignature(
            verter_type_expr::FunctionExpr::with_spans(
                function.parameters.clone(),
                function.return_type.clone(),
                function.type_parameters.clone(),
                function.spans,
            ),
        )
    }
    fn member_index_signature(
        &mut self,
        key_name: String,
        key_type: TypeExpr,
        value_type: TypeExpr,
        readonly: bool,
        spans: verter_type_expr::IndexSignatureSpans,
    ) -> verter_type_expr::ObjectMember {
        verter_type_expr::ObjectMember::IndexSignature(
            verter_type_expr::IndexSignature::with_spans(
                key_name, key_type, value_type, readonly, spans,
            ),
        )
    }
    fn object_from_members(&mut self, members: Vec<verter_type_expr::ObjectMember>) -> TypeExpr {
        TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
            properties: members,
        }))
    }

    fn is_object_surface_sentinel(&self, out: &TypeExpr) -> bool {
        matches!(out, TypeExpr::Unknown { raw } if raw == SEMANTIC_OBJECT_SURFACE)
    }
    fn is_empty_object(&self, out: &TypeExpr) -> bool {
        matches!(out, TypeExpr::Object(object) if object.properties.is_empty())
    }
}

/// Fold `node` to a `TypeExpr` through the shared `MaterializeTypeExprAlg` — the
/// entry the raise-side shell primitive
/// ([`super::ProjectSemanticDispatch::raise_node_to_type_expr`]) delegates to.
/// `None` when the node — or a `?`-propagating required child — is unavailable /
/// unraisable. (Named `fold_to_type_expr`, NOT `materialize_type_expr`, so it
/// does not collide with the `#[cfg(test)]` `materialize_type_expr(HotTypeRef)`
/// boundary the G-A guard pins to exactly one definition.)
pub(in crate::project_semantic_dispatch) fn fold_to_type_expr(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<TypeExpr> {
    let mut alg = MaterializeTypeExprAlg;
    fold_node(&mut alg, dispatch, node, active)
}

#[cfg(test)]
mod tests {
    use verter_type_expr::TypeExpr;

    use super::{MaterializeTypeExprAlg, RaisedShapeAlgebra};
    use crate::resolver_core::component_meta_query_engine::{
        semantic_query_error_raw, SEMANTIC_OBJECT_SURFACE, SEMANTIC_SURFACE_MEMBER,
    };
    use crate::semantic_query::QueryError;

    /// The typed `opaque_sentinel` algebra entry point on the materializer must
    /// produce the BYTE-IDENTICAL `TypeExpr::Unknown { raw }` the legacy
    /// hardcoded `alg.unknown(Arc::from("literal"))` produced — for every typed
    /// control-sentinel variant. This pins that routing a sentinel through the
    /// typed entry point is byte-equivalent to the old raw-literal construction
    /// (the materialization byte-identity contract for the swap).
    #[test]
    fn opaque_sentinel_materializes_byte_identical_legacy_raw() {
        // (variant, the exact legacy raw string the old literal emitted)
        let cases: &[(QueryError, &str)] = &[
            (QueryError::RaiseAliasCycle, "semanticAliasCycle"),
            (QueryError::TypeParamCycle, "semanticTypeParamCycle"),
            (QueryError::RaiseMiss, "<raise miss>"),
            (QueryError::UnrepresentableSurface, SEMANTIC_OBJECT_SURFACE),
            (
                QueryError::UnrepresentableSurfaceMember,
                SEMANTIC_SURFACE_MEMBER,
            ),
            (QueryError::VueMacroElementsPlaceholder, "VueMacroElements"),
        ];

        for (variant, expected_raw) in cases {
            let mut alg = MaterializeTypeExprAlg;
            let produced = alg.opaque_sentinel(variant);
            // The typed entry point is byte-equal to the old literal …
            assert_eq!(
                produced,
                TypeExpr::Unknown {
                    raw: (*expected_raw).to_string(),
                },
                "opaque_sentinel({variant:?}) must materialize Unknown {{ raw: {expected_raw:?} }}"
            );
            // … and equal to what `semantic_query_error_raw` maps the variant to
            // (the round-trip authority), proving the entry point routes through
            // that single mapping rather than a private literal.
            assert_eq!(
                produced,
                TypeExpr::Unknown {
                    raw: semantic_query_error_raw(variant),
                },
                "opaque_sentinel({variant:?}) must agree with semantic_query_error_raw"
            );
        }
    }
}
