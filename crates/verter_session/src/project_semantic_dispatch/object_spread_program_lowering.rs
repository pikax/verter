//! Canonical source-ordered object-spread program lowering.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectMethodKind, TypeExpr};

use super::ProjectSemanticDispatch;
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    AuthoredAccessorEffect, AuthoredIndexEffect, AuthoredMethodEffect, AuthoredPropertyEffect,
    NodeScopeId, ObjectConstructionEffect, ObjectSpreadProgram, ProjectionReductionContext,
    SemanticNodeData, SemanticNodeId, SurfaceView,
};
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

pub(super) fn direct_effects_from_surface(surface: &SurfaceView) -> Vec<ObjectConstructionEffect> {
    let mut effects = Vec::with_capacity(
        surface.positive_members().len()
            + surface.index_signatures.len()
            + surface.call_signatures.len()
            + surface.construct_signatures.len(),
    );
    for member in surface.positive_members() {
        let common = AuthoredAccessorEffect {
            key: member.key.clone(),
            signature: member.value,
            optional: member.optional,
            has_implementation_body: member.has_implementation_body,
            visibility: member.visibility,
            spans: member.spans,
            declaration_origin: member.declaration_origin.clone(),
            declared_in_macro_type_arg: member.declared_in_macro_type_arg,
            merge_role: member.merge_role,
            excess_origin: member.excess_origin,
        };
        effects.push(match member.method_kind {
            Some(ObjectMethodKind::Method) => {
                ObjectConstructionEffect::DirectMethod(AuthoredMethodEffect {
                    key: member.key.clone(),
                    signature: member.value,
                    optional: member.optional,
                    has_implementation_body: member.has_implementation_body,
                    visibility: member.visibility,
                    spans: member.spans,
                    declaration_origin: member.declaration_origin.clone(),
                    declared_in_macro_type_arg: member.declared_in_macro_type_arg,
                    merge_role: member.merge_role,
                    excess_origin: member.excess_origin,
                })
            }
            Some(ObjectMethodKind::Get) => ObjectConstructionEffect::DirectGet(common),
            Some(ObjectMethodKind::Set) => ObjectConstructionEffect::DirectSet(common),
            None => ObjectConstructionEffect::DirectProperty(AuthoredPropertyEffect {
                key: member.key.clone(),
                value: member.value,
                optional: member.optional,
                readonly: member.readonly,
                visibility: member.visibility,
                spans: member.spans,
                declaration_origin: member.declaration_origin.clone(),
                declared_in_macro_type_arg: member.declared_in_macro_type_arg,
                merge_role: member.merge_role,
                excess_origin: member.excess_origin,
            }),
        });
    }
    effects.extend(surface.index_signatures.iter().map(|index| {
        ObjectConstructionEffect::DirectIndex(AuthoredIndexEffect {
            key_type: index.key_type,
            value_type: index.value_type,
            readonly: index.readonly,
            spans: index.spans,
            declaration_origin: index.declaration_origin.clone(),
        })
    }));
    effects.extend(
        surface
            .call_signatures
            .iter()
            .copied()
            .map(ObjectConstructionEffect::DirectCall),
    );
    effects.extend(
        surface
            .construct_signatures
            .iter()
            .copied()
            .map(ObjectConstructionEffect::DirectConstruct),
    );
    effects
}

impl<'a> ProjectSemanticDispatch<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lower_spread_object_literal(
        &self,
        obj: &ObjectExpr,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<Arc<str>, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let mut effects = Vec::with_capacity(obj.properties.len());
        for member in &obj.properties {
            match member {
                ObjectMember::Property(property) => {
                    let key = self.lower_authored_property_key(
                        &property.key,
                        infer_binders,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    let value = self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        &property.ty,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    effects.push(ObjectConstructionEffect::DirectProperty(
                        AuthoredPropertyEffect {
                            key,
                            value,
                            optional: property.optional,
                            readonly: property.readonly,
                            visibility: property.visibility,
                            spans: property.spans,
                            declaration_origin: scope.canonical_file(),
                            declared_in_macro_type_arg: reduction_context.own_body_stamp(),
                            merge_role: reduction_context.role_stamp(),
                            excess_origin: property.excess_origin,
                        },
                    ));
                }
                ObjectMember::Method(method) => {
                    let key = self.lower_authored_property_key(
                        &method.key,
                        infer_binders,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    let function_expr = TypeExpr::Function(Arc::new(method.function.clone()));
                    let signature = self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        &function_expr,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    let common = AuthoredAccessorEffect {
                        key: key.clone(),
                        signature,
                        optional: method.optional,
                        has_implementation_body: method.has_implementation_body,
                        visibility: method.visibility,
                        spans: method.spans,
                        declaration_origin: scope.canonical_file(),
                        declared_in_macro_type_arg: reduction_context.own_body_stamp(),
                        merge_role: reduction_context.role_stamp(),
                        excess_origin: method.excess_origin,
                    };
                    effects.push(match method.method_kind {
                        ObjectMethodKind::Method => {
                            ObjectConstructionEffect::DirectMethod(AuthoredMethodEffect {
                                key,
                                signature,
                                optional: method.optional,
                                has_implementation_body: method.has_implementation_body,
                                visibility: method.visibility,
                                spans: method.spans,
                                declaration_origin: scope.canonical_file(),
                                declared_in_macro_type_arg: reduction_context.own_body_stamp(),
                                merge_role: reduction_context.role_stamp(),
                                excess_origin: method.excess_origin,
                            })
                        }
                        ObjectMethodKind::Get => ObjectConstructionEffect::DirectGet(common),
                        ObjectMethodKind::Set => ObjectConstructionEffect::DirectSet(common),
                    });
                }
                ObjectMember::IndexSignature(index) => {
                    let key_type = self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        &index.key_type,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    let value_type = self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        &index.value_type,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    effects.push(ObjectConstructionEffect::DirectIndex(AuthoredIndexEffect {
                        key_type,
                        value_type,
                        readonly: index.readonly,
                        spans: index.spans,
                        declaration_origin: scope.canonical_file(),
                    }));
                }
                ObjectMember::CallSignature(function) => {
                    let function_expr = TypeExpr::Function(Arc::new(function.clone()));
                    let signature = self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        &function_expr,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    effects.push(ObjectConstructionEffect::DirectCall(signature));
                }
                ObjectMember::ConstructSignature(function) => {
                    let function_expr = TypeExpr::ConstructorType(Arc::new(function.clone()));
                    let signature = self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        &function_expr,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    effects.push(ObjectConstructionEffect::DirectConstruct(signature));
                }
                ObjectMember::Spread(spread) => {
                    let operand = self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        &spread.ty,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    effects.push(ObjectConstructionEffect::Spread(operand));
                }
            }
        }
        self.graph().intern_node_with_scope(
            SemanticNodeData::ObjectSpreadProgram(ObjectSpreadProgram {
                effects: Arc::from(effects),
            }),
            scope.clone(),
        )
    }
}
