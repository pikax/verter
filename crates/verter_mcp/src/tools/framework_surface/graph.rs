//! Structural projection of the framework surface's wire graph. References stay
//! as ids; no graph edges are followed and no type meaning is reconstructed.

use super::resolve_string;
use serde_json::{json, Value};
use verter_protocol::verter::v1 as wire;

macro_rules! enum_name {
    ($ty:ty, $value:expr) => {
        <$ty>::try_from($value)
            .map(|v| v.as_str_name().to_string())
            .unwrap_or_else(|_| format!("UNKNOWN({})", $value))
    };
}

fn tagged(kind: &str, mut payload: Value) -> Value {
    payload
        .as_object_mut()
        .expect("wire payload is an object")
        .insert("kind".to_string(), kind.into());
    payload
}

pub(super) fn project_graph(v: &wire::SemanticTypeGraph, strings: &[String]) -> Value {
    json!({
        "schemaVersion": v.schema_version,
        "nodes": v.nodes.iter().map(|v| project_node(v, strings)).collect::<Vec<_>>(),
        "symbols": v.symbols.iter().map(|v| project_symbol_node(v, strings)).collect::<Vec<_>>(),
        "signatures": v.signatures.iter().map(|v| project_signature(v, strings)).collect::<Vec<_>>(),
        "edges": v.edges.iter().map(|v| project_origin_edge(v, strings)).collect::<Vec<_>>(),
        "rootIds": v.root_ids,
        "exactness": v.exactness.iter().map(|v| project_node_status(v, strings)).collect::<Vec<_>>(),
        "diagnostics": v.diagnostics.iter().map(|v| project_diagnostic(v, strings)).collect::<Vec<_>>(),
        "nodeIdMap": v.node_id_map.iter().map(|v| project_node_id_map_entry(v, strings)).collect::<Vec<_>>(),
        "symbolIdMap": v.symbol_id_map.iter().map(|v| project_symbol_id_map_entry(v, strings)).collect::<Vec<_>>(),
        "strings": v.strings.as_ref().map(|v| project_string_table(v, strings)),
        "relationProofs": v.relation_proofs.iter().map(|v| project_relation_proof_entry(v, strings)).collect::<Vec<_>>(),
        "factIdentity": v.fact_identity.as_ref().map(|v| project_fact_identity(v, strings)),
    })
}

fn project_node(v: &wire::GraphTypeNode, strings: &[String]) -> Value {
    use wire::graph_type_node::Kind;
    match &v.kind {
        Some(Kind::Primitive(v)) => tagged("primitive", project_primitive(v, strings)),
        Some(Kind::Literal(v)) => tagged("literal", project_literal(v, strings)),
        Some(Kind::UniqueSymbol(v)) => tagged("uniqueSymbol", project_unique_symbol(v, strings)),
        Some(Kind::Union(v)) => tagged("union", project_union(v, strings)),
        Some(Kind::Intersection(v)) => tagged("intersection", project_intersection(v, strings)),
        Some(Kind::Object(v)) => tagged("object", project_object(v, strings)),
        Some(Kind::Array(v)) => tagged("array", project_array(v, strings)),
        Some(Kind::Tuple(v)) => tagged("tuple", project_tuple(v, strings)),
        Some(Kind::Reference(v)) => tagged("reference", project_reference(v, strings)),
        Some(Kind::AliasInstantiation(v)) => tagged(
            "aliasInstantiation",
            project_alias_instantiation(v, strings),
        ),
        Some(Kind::TypeParameter(v)) => tagged("typeParameter", project_type_parameter(v, strings)),
        Some(Kind::KeyOf(v)) => tagged("keyOf", project_key_of(v, strings)),
        Some(Kind::IndexedAccess(v)) => tagged("indexedAccess", project_indexed_access(v, strings)),
        Some(Kind::Conditional(v)) => tagged("conditional", project_conditional(v, strings)),
        Some(Kind::Mapped(v)) => tagged("mapped", project_mapped(v, strings)),
        Some(Kind::TemplateLiteral(v)) => {
            tagged("templateLiteral", project_template_literal(v, strings))
        }
        Some(Kind::TypeofNode(v)) => tagged("typeofNode", project_type_of(v, strings)),
        Some(Kind::SatisfiesNode(v)) => tagged("satisfiesNode", project_satisfies(v, strings)),
        Some(Kind::ClassNode(v)) => tagged("classNode", project_class(v, strings)),
        Some(Kind::ThisType(v)) => tagged("thisType", project_this_type(v, strings)),
        Some(Kind::MergedDeclaration(v)) => {
            tagged("mergedDeclaration", project_merged_declaration(v, strings))
        }
        Some(Kind::AmbientModule(v)) => tagged("ambientModule", project_ambient_module(v, strings)),
        Some(Kind::ModuleAugmentation(v)) => tagged(
            "moduleAugmentation",
            project_module_augmentation(v, strings),
        ),
        Some(Kind::AmbientNamespace(v)) => {
            tagged("ambientNamespace", project_ambient_namespace(v, strings))
        }
        Some(Kind::GlobalAugmentation(v)) => tagged(
            "globalAugmentation",
            project_global_augmentation(v, strings),
        ),
        Some(Kind::FlowNarrowing(v)) => tagged("flowNarrowing", project_flow_narrowing(v, strings)),
        Some(Kind::ContextualType(v)) => {
            tagged("contextualType", project_contextual_type(v, strings))
        }
        Some(Kind::InferNode(v)) => tagged("inferNode", project_infer(v, strings)),
        Some(Kind::EnumNode(v)) => tagged("enumNode", project_enum(v, strings)),
        Some(Kind::Opaque(v)) => tagged("opaque", project_opaque(v, strings)),
        Some(Kind::Cycle(v)) => tagged("cycle", project_cycle(v, strings)),
        Some(Kind::ObjectSpreadProgram(v)) => tagged(
            "objectSpreadProgram",
            project_object_spread_program(v, strings),
        ),
        Some(Kind::IntrinsicApplication(v)) => tagged(
            "intrinsicApplication",
            project_intrinsic_application(v, strings),
        ),
        None => Value::Null,
    }
}

fn project_primitive(v: &wire::GraphPrimitive, _strings: &[String]) -> Value {
    json!({
        "primitiveKind": enum_name!(wire::GraphPrimitiveKind, v.kind),
    })
}

fn project_literal(v: &wire::GraphLiteral, strings: &[String]) -> Value {
    json!({
        "value": v.value.as_ref().map(|v| project_literal_value(v, strings)),
    })
}

fn project_literal_value(v: &wire::GraphLiteralValue, strings: &[String]) -> Value {
    use wire::graph_literal_value::Kind;
    match &v.kind {
        Some(Kind::StringNameId(v)) => {
            json!({ "kind": "stringNameId", "value": v, "string": resolve_string(strings, *v) })
        }
        Some(Kind::NumberBits(v)) => json!({ "kind": "numberBits", "value": v }),
        Some(Kind::BooleanValue(v)) => json!({ "kind": "booleanValue", "value": v }),
        Some(Kind::BigintNameId(v)) => {
            json!({ "kind": "bigintNameId", "value": v, "string": resolve_string(strings, *v) })
        }
        None => Value::Null,
    }
}

fn project_unique_symbol(v: &wire::GraphUniqueSymbol, _strings: &[String]) -> Value {
    json!({
        "declSymbolId": v.decl_symbol_id,
    })
}

fn project_union(v: &wire::GraphUnion, _strings: &[String]) -> Value {
    json!({
        "memberNodeIds": v.member_node_ids,
    })
}

fn project_intersection(v: &wire::GraphIntersection, _strings: &[String]) -> Value {
    json!({
        "memberNodeIds": v.member_node_ids,
    })
}

fn project_object(v: &wire::GraphObject, strings: &[String]) -> Value {
    json!({
        "members": v.members.iter().map(|v| project_object_member(v, strings)).collect::<Vec<_>>(),
        "indexSignatures": v.index_signatures.iter().map(|v| project_index_signature(v, strings)).collect::<Vec<_>>(),
        "callSignatureRefs": v.call_signature_refs,
        "constructSignatureRefs": v.construct_signature_refs,
        "flags": v.flags,
    })
}

fn project_object_member(v: &wire::GraphObjectMember, strings: &[String]) -> Value {
    json!({
        "valueNodeId": v.value_node_id,
        "optional": v.optional,
        "readonly": v.readonly,
        "accessibility": enum_name!(wire::GraphAccessibility, v.accessibility),
        "staticSide": v.static_side,
        "declarationSymbolId": v.declaration_symbol_id,
        "propertyKey": v.property_key.as_ref().map(|v| project_property_key(v, strings)),
        "memberKind": enum_name!(wire::GraphObjectMemberKind, v.member_kind),
        "hasImplementationBody": v.has_implementation_body,
    })
}

fn project_property_key(v: &wire::GraphPropertyKey, strings: &[String]) -> Value {
    use wire::graph_property_key::Key;
    match &v.key {
        Some(Key::StringId(v)) => {
            json!({ "kind": "stringId", "value": v, "string": resolve_string(strings, *v) })
        }
        Some(Key::CanonicalNumber(v)) => json!({ "kind": "canonicalNumber", "value": v }),
        Some(Key::UniqueSymbolDeclId(v)) => json!({ "kind": "uniqueSymbolDeclId", "value": v }),
        Some(Key::ComputedNodeId(v)) => json!({ "kind": "computedNodeId", "value": v }),
        None => Value::Null,
    }
}

fn project_index_signature(v: &wire::GraphIndexSignature, _strings: &[String]) -> Value {
    json!({
        "keyKind": enum_name!(wire::GraphIndexKeyKind, v.key_kind),
        "valueNodeId": v.value_node_id,
        "readonly": v.readonly,
    })
}

fn project_array(v: &wire::GraphArray, _strings: &[String]) -> Value {
    json!({
        "elementNodeId": v.element_node_id,
        "readonly": v.readonly,
    })
}

fn project_tuple(v: &wire::GraphTuple, strings: &[String]) -> Value {
    json!({
        "elements": v.elements.iter().map(|v| project_tuple_element(v, strings)).collect::<Vec<_>>(),
        "readonly": v.readonly,
    })
}

fn project_tuple_element(v: &wire::GraphTupleElement, strings: &[String]) -> Value {
    json!({
        "labelNameId": v.label_name_id,
        "label": if v.label_name_id == 0 { None } else { resolve_string(strings, v.label_name_id) },
        "valueNodeId": v.value_node_id,
        "optional": v.optional,
        "rest": v.rest,
    })
}

fn project_reference(v: &wire::GraphReference, _strings: &[String]) -> Value {
    json!({
        "symbolId": v.symbol_id,
    })
}

fn project_alias_instantiation(v: &wire::GraphAliasInstantiation, _strings: &[String]) -> Value {
    json!({
        "aliasSymbolId": v.alias_symbol_id,
        "typeArgumentNodeIds": v.type_argument_node_ids,
        "targetNodeId": v.target_node_id,
        "displayRefNodeId": v.display_ref_node_id,
    })
}

fn project_type_parameter(v: &wire::GraphTypeParameter, strings: &[String]) -> Value {
    json!({
        "symbolId": v.symbol_id,
        "declSlotRef": v.decl_slot_ref,
        "paramIndex": v.param_index,
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
        "constraintNodeId": v.constraint_node_id,
        "defaultNodeId": v.default_node_id,
        "variance": enum_name!(wire::GraphVariance, v.variance),
        "isConst": v.is_const,
        "noInfer": v.no_infer,
        "binding": v.binding.as_ref().map(|v| project_type_parameter_binding(v, strings)),
    })
}

fn project_type_parameter_binding(
    v: &wire::GraphTypeParameterBinding,
    strings: &[String],
) -> Value {
    use wire::graph_type_parameter_binding::Kind;
    match &v.kind {
        Some(Kind::Unbound(v)) => tagged(
            "unbound",
            project_type_parameter_binding_unbound(v, strings),
        ),
        Some(Kind::Concrete(v)) => tagged(
            "concrete",
            project_type_parameter_binding_concrete(v, strings),
        ),
        Some(Kind::Inferred(v)) => tagged(
            "inferred",
            project_type_parameter_binding_inferred(v, strings),
        ),
        Some(Kind::Substituted(v)) => tagged(
            "substituted",
            project_type_parameter_binding_substituted(v, strings),
        ),
        None => Value::Null,
    }
}

fn project_type_parameter_binding_unbound(
    _v: &wire::GraphTypeParameterBindingUnbound,
    _strings: &[String],
) -> Value {
    json!({})
}

fn project_type_parameter_binding_concrete(
    v: &wire::GraphTypeParameterBindingConcrete,
    _strings: &[String],
) -> Value {
    json!({
        "typeRefNodeId": v.type_ref_node_id,
    })
}

fn project_type_parameter_binding_inferred(
    v: &wire::GraphTypeParameterBindingInferred,
    _strings: &[String],
) -> Value {
    json!({
        "typeRefNodeId": v.type_ref_node_id,
        "sourceOriginRef": v.source_origin_ref,
    })
}

fn project_type_parameter_binding_substituted(
    v: &wire::GraphTypeParameterBindingSubstituted,
    _strings: &[String],
) -> Value {
    json!({
        "typeRefNodeId": v.type_ref_node_id,
        "fromNodeId": v.from_node_id,
    })
}

fn project_key_of(v: &wire::GraphKeyOf, _strings: &[String]) -> Value {
    json!({
        "baseNodeId": v.base_node_id,
    })
}

fn project_indexed_access(v: &wire::GraphIndexedAccess, _strings: &[String]) -> Value {
    json!({
        "objectNodeId": v.object_node_id,
        "indexNodeId": v.index_node_id,
    })
}

fn project_conditional(v: &wire::GraphConditional, strings: &[String]) -> Value {
    json!({
        "checkNodeId": v.check_node_id,
        "extendsNodeId": v.extends_node_id,
        "trueBranchNodeId": v.true_branch_node_id,
        "falseBranchNodeId": v.false_branch_node_id,
        "distributive": v.distributive,
        "resolution": v.resolution.as_ref().map(|v| project_conditional_resolution(v, strings)),
    })
}

fn project_conditional_resolution(
    v: &wire::GraphConditionalResolution,
    strings: &[String],
) -> Value {
    use wire::graph_conditional_resolution::Kind;
    match &v.kind {
        Some(Kind::SelectedTrue(v)) => tagged(
            "selectedTrue",
            project_conditional_resolution_selected(v, strings),
        ),
        Some(Kind::SelectedFalse(v)) => tagged(
            "selectedFalse",
            project_conditional_resolution_selected(v, strings),
        ),
        Some(Kind::UnresolvedGeneric(v)) => tagged(
            "unresolvedGeneric",
            project_conditional_resolution_unresolved_generic(v, strings),
        ),
        Some(Kind::Distributed(v)) => tagged(
            "distributed",
            project_conditional_resolution_distributed(v, strings),
        ),
        Some(Kind::Unsupported(v)) => tagged(
            "unsupported",
            project_conditional_resolution_unsupported(v, strings),
        ),
        None => Value::Null,
    }
}

fn project_conditional_resolution_selected(
    v: &wire::GraphConditionalResolutionSelected,
    _strings: &[String],
) -> Value {
    json!({
        "proofRef": v.proof_ref,
    })
}

fn project_conditional_resolution_unresolved_generic(
    v: &wire::GraphConditionalResolutionUnresolvedGeneric,
    _strings: &[String],
) -> Value {
    json!({
        "blockerNodeIds": v.blocker_node_ids,
        "reason": enum_name!(wire::GraphRelationUnknownReason, v.reason),
    })
}

fn project_conditional_resolution_distributed(
    v: &wire::GraphConditionalResolutionDistributed,
    strings: &[String],
) -> Value {
    json!({
        "cases": v.cases.iter().map(|v| project_distributed_conditional_case(v, strings)).collect::<Vec<_>>(),
    })
}

fn project_distributed_conditional_case(
    v: &wire::GraphDistributedConditionalCase,
    _strings: &[String],
) -> Value {
    json!({
        "caseNodeId": v.case_node_id,
        "resultNodeId": v.result_node_id,
    })
}

fn project_conditional_resolution_unsupported(
    v: &wire::GraphConditionalResolutionUnsupported,
    _strings: &[String],
) -> Value {
    json!({
        "reason": enum_name!(wire::GraphUnsupportedConstruct, v.reason),
    })
}

fn project_mapped(v: &wire::GraphMapped, _strings: &[String]) -> Value {
    json!({
        "keyTypeNodeId": v.key_type_node_id,
        "sourceNodeId": v.source_node_id,
        "nameRemapNodeId": v.name_remap_node_id,
        "valueTypeNodeId": v.value_type_node_id,
        "readonlyModifier": enum_name!(wire::GraphMappedModifier, v.readonly_modifier),
        "optionalModifier": enum_name!(wire::GraphMappedModifier, v.optional_modifier),
    })
}

fn project_template_literal(v: &wire::GraphTemplateLiteral, strings: &[String]) -> Value {
    json!({
        "quasiNameIds": v.quasi_name_ids,
        "quasiNames": v.quasi_name_ids.iter().map(|id| resolve_string(strings, *id)).collect::<Vec<_>>(),
        "expressionNodeIds": v.expression_node_ids,
    })
}

fn project_type_of(v: &wire::GraphTypeOf, strings: &[String]) -> Value {
    json!({
        "valueRootRef": v.value_root_ref,
        "pathNameIds": v.path_name_ids,
        "pathNames": v.path_name_ids.iter().map(|id| resolve_string(strings, *id)).collect::<Vec<_>>(),
    })
}

fn project_satisfies(v: &wire::GraphSatisfies, _strings: &[String]) -> Value {
    json!({
        "valueNodeId": v.value_node_id,
        "constraintNodeId": v.constraint_node_id,
    })
}

fn project_class(v: &wire::GraphClass, strings: &[String]) -> Value {
    json!({
        "symbolId": v.symbol_id,
        "typeParameterNodeIds": v.type_parameter_node_ids,
        "heritage": v.heritage.iter().map(|v| project_heritage_clause(v, strings)).collect::<Vec<_>>(),
        "members": v.members.iter().map(|v| project_object_member(v, strings)).collect::<Vec<_>>(),
        "staticMembers": v.static_members.iter().map(|v| project_object_member(v, strings)).collect::<Vec<_>>(),
        "constructSignatureRefs": v.construct_signature_refs,
        "flags": v.flags,
    })
}

fn project_heritage_clause(v: &wire::GraphHeritageClause, _strings: &[String]) -> Value {
    json!({
        "kind": enum_name!(wire::GraphHeritageKind, v.kind),
        "baseNodeId": v.base_node_id,
        "typeArgumentNodeIds": v.type_argument_node_ids,
    })
}

fn project_this_type(v: &wire::GraphThisType, _strings: &[String]) -> Value {
    json!({
        "declSymbolId": v.decl_symbol_id,
    })
}

fn project_merged_declaration(v: &wire::GraphMergedDeclaration, strings: &[String]) -> Value {
    json!({
        "mergedSymbolId": v.merged_symbol_id,
        "parts": v.parts.iter().map(|v| project_declaration_part(v, strings)).collect::<Vec<_>>(),
    })
}

fn project_declaration_part(v: &wire::GraphDeclarationPart, strings: &[String]) -> Value {
    json!({
        "sourceCanonicalNameId": v.source_canonical_name_id,
        "sourceCanonicalName": resolve_string(strings, v.source_canonical_name_id),
        "declarationNodeId": v.declaration_node_id,
        "kind": enum_name!(wire::GraphDeclarationPartKind, v.kind),
    })
}

fn project_ambient_module(v: &wire::GraphAmbientModule, strings: &[String]) -> Value {
    json!({
        "specifierNameId": v.specifier_name_id,
        "specifierName": resolve_string(strings, v.specifier_name_id),
        "moduleNamespaceNodeId": v.module_namespace_node_id,
    })
}

fn project_module_augmentation(v: &wire::GraphModuleAugmentation, strings: &[String]) -> Value {
    json!({
        "specifierNameId": v.specifier_name_id,
        "specifierName": resolve_string(strings, v.specifier_name_id),
        "parts": v.parts.iter().map(|v| project_declaration_part(v, strings)).collect::<Vec<_>>(),
    })
}

fn project_ambient_namespace(v: &wire::GraphAmbientNamespace, strings: &[String]) -> Value {
    json!({
        "namespaceNameId": v.namespace_name_id,
        "namespaceName": resolve_string(strings, v.namespace_name_id),
        "namespaceNodeId": v.namespace_node_id,
    })
}

fn project_global_augmentation(v: &wire::GraphGlobalAugmentation, strings: &[String]) -> Value {
    json!({
        "parts": v.parts.iter().map(|v| project_declaration_part(v, strings)).collect::<Vec<_>>(),
    })
}

fn project_flow_narrowing(v: &wire::GraphFlowNarrowing, _strings: &[String]) -> Value {
    json!({
        "siteSpanRef": v.site_span_ref,
        "narrowedNodeId": v.narrowed_node_id,
        "baseNodeId": v.base_node_id,
    })
}

fn project_contextual_type(v: &wire::GraphContextualType, _strings: &[String]) -> Value {
    json!({
        "siteSpanRef": v.site_span_ref,
        "contextualNodeId": v.contextual_node_id,
    })
}

fn project_infer(v: &wire::GraphInfer, strings: &[String]) -> Value {
    json!({
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
        "constraintNodeId": v.constraint_node_id,
    })
}

fn project_enum(v: &wire::GraphEnum, strings: &[String]) -> Value {
    json!({
        "symbolId": v.symbol_id,
        "members": v.members.iter().map(|v| project_enum_member(v, strings)).collect::<Vec<_>>(),
        "isConst": v.is_const,
    })
}

fn project_enum_member(v: &wire::GraphEnumMember, strings: &[String]) -> Value {
    json!({
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
        "value": v.value.as_ref().map(|v| project_enum_member_value(v, strings)),
    })
}

fn project_enum_member_value(v: &wire::GraphEnumMemberValue, strings: &[String]) -> Value {
    use wire::graph_enum_member_value::Kind;
    match &v.kind {
        Some(Kind::Numeric(v)) => json!({ "kind": "numeric", "value": v }),
        Some(Kind::StringNameId(v)) => {
            json!({ "kind": "stringNameId", "value": v, "string": resolve_string(strings, *v) })
        }
        Some(Kind::Computed(v)) => json!({ "kind": "computed", "value": v }),
        None => Value::Null,
    }
}

fn project_opaque(v: &wire::GraphOpaque, strings: &[String]) -> Value {
    json!({
        "error": v.error.as_ref().map(|v| project_query_error(v, strings)),
    })
}

fn project_query_error(v: &wire::GraphQueryError, strings: &[String]) -> Value {
    use wire::graph_query_error::Kind;
    match &v.kind {
        Some(Kind::Miss(v)) => tagged("miss", project_query_error_miss(v, strings)),
        Some(Kind::UnsupportedIntrinsic(v)) => tagged(
            "unsupportedIntrinsic",
            project_query_error_unsupported_intrinsic(v, strings),
        ),
        Some(Kind::BudgetExceeded(v)) => tagged(
            "budgetExceeded",
            project_query_error_budget_exceeded(v, strings),
        ),
        Some(Kind::UnstableState(v)) => tagged(
            "unstableState",
            project_query_error_unstable_state(v, strings),
        ),
        Some(Kind::AliasCycle(v)) => {
            tagged("aliasCycle", project_query_error_alias_cycle(v, strings))
        }
        Some(Kind::RecursiveRef(v)) => tagged(
            "recursiveRef",
            project_query_error_recursive_ref(v, strings),
        ),
        Some(Kind::Other(v)) => tagged("other", project_query_error_other(v, strings)),
        Some(Kind::DeclPlaceholder(v)) => tagged(
            "declPlaceholder",
            project_query_error_decl_placeholder(v, strings),
        ),
        None => Value::Null,
    }
}

fn project_query_error_miss(_v: &wire::GraphQueryErrorMiss, _strings: &[String]) -> Value {
    json!({})
}

fn project_query_error_unsupported_intrinsic(
    v: &wire::GraphQueryErrorUnsupportedIntrinsic,
    strings: &[String],
) -> Value {
    json!({
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
    })
}

fn project_query_error_budget_exceeded(
    v: &wire::GraphQueryErrorBudgetExceeded,
    strings: &[String],
) -> Value {
    json!({
        "domain": enum_name!(wire::GraphBudgetDomain, v.domain),
        "limit": v.limit,
        "actual": v.actual,
        "contextNameId": v.context_name_id,
        "contextName": resolve_string(strings, v.context_name_id),
    })
}

fn project_query_error_unstable_state(
    v: &wire::GraphQueryErrorUnstableState,
    _strings: &[String],
) -> Value {
    json!({
        "attempts": v.attempts,
    })
}

fn project_query_error_alias_cycle(
    _v: &wire::GraphQueryErrorAliasCycle,
    _strings: &[String],
) -> Value {
    json!({})
}

fn project_query_error_recursive_ref(
    v: &wire::GraphQueryErrorRecursiveRef,
    strings: &[String],
) -> Value {
    json!({
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
    })
}

fn project_query_error_other(v: &wire::GraphQueryErrorOther, strings: &[String]) -> Value {
    json!({
        "messageNameId": v.message_name_id,
        "messageName": resolve_string(strings, v.message_name_id),
    })
}

fn project_query_error_decl_placeholder(
    v: &wire::GraphQueryErrorDeclPlaceholder,
    strings: &[String],
) -> Value {
    json!({
        "canonicalNameId": v.canonical_name_id,
        "canonicalName": resolve_string(strings, v.canonical_name_id),
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
        "wholeHash": v.whole_hash,
    })
}

fn project_cycle(v: &wire::GraphCycle, _strings: &[String]) -> Value {
    json!({
        "cycleRootNodeId": v.cycle_root_node_id,
        "participants": v.participants,
    })
}

fn project_object_spread_program(v: &wire::GraphObjectSpreadProgram, strings: &[String]) -> Value {
    json!({
        "effects": v.effects.iter().map(|v| project_object_construction_effect(v, strings)).collect::<Vec<_>>(),
    })
}

fn project_object_construction_effect(
    v: &wire::GraphObjectConstructionEffect,
    strings: &[String],
) -> Value {
    use wire::graph_object_construction_effect::Kind;
    match &v.kind {
        Some(Kind::DirectProperty(v)) => {
            tagged("directProperty", project_object_named_effect(v, strings))
        }
        Some(Kind::DirectMethod(v)) => {
            tagged("directMethod", project_object_named_effect(v, strings))
        }
        Some(Kind::DirectGet(v)) => tagged("directGet", project_object_named_effect(v, strings)),
        Some(Kind::DirectSet(v)) => tagged("directSet", project_object_named_effect(v, strings)),
        Some(Kind::DirectIndex(v)) => {
            tagged("directIndex", project_object_index_effect(v, strings))
        }
        Some(Kind::DirectCall(v)) => {
            tagged("directCall", project_object_signature_effect(v, strings))
        }
        Some(Kind::DirectConstruct(v)) => tagged(
            "directConstruct",
            project_object_signature_effect(v, strings),
        ),
        Some(Kind::Spread(v)) => tagged("spread", project_object_spread_effect(v, strings)),
        None => Value::Null,
    }
}

fn project_object_named_effect(v: &wire::GraphObjectNamedEffect, strings: &[String]) -> Value {
    json!({
        "propertyKey": v.property_key.as_ref().map(|v| project_property_key(v, strings)),
        "valueNodeId": v.value_node_id,
        "optional": v.optional,
        "readonly": v.readonly,
        "hasImplementationBody": v.has_implementation_body,
        "accessibility": enum_name!(wire::GraphAccessibility, v.accessibility),
        "spans": v.spans.as_ref().map(|v| project_object_member_spans(v, strings)),
        "declarationOriginNameId": v.declaration_origin_name_id,
        "declarationOriginName": v.has_declaration_origin.then(|| resolve_string(strings, v.declaration_origin_name_id)).flatten(),
        "hasDeclarationOrigin": v.has_declaration_origin,
        "declaredInMacroTypeArg": v.declared_in_macro_type_arg,
        "mergeRole": enum_name!(wire::GraphObjectMergeRole, v.merge_role),
        "excessOrigin": enum_name!(wire::GraphObjectExcessOrigin, v.excess_origin),
    })
}

fn project_object_member_spans(v: &wire::GraphObjectMemberSpans, strings: &[String]) -> Value {
    json!({
        "declaration": v.declaration.as_ref().map(|v| project_span_ref(v, strings)),
        "name": v.name.as_ref().map(|v| project_span_ref(v, strings)),
        "typeAnnotation": v.type_annotation.as_ref().map(|v| project_span_ref(v, strings)),
    })
}

fn project_span_ref(v: &wire::GraphSpanRef, _strings: &[String]) -> Value {
    json!({
        "canonicalId": v.canonical_id,
        "start": v.start,
        "end": v.end,
    })
}

fn project_object_index_effect(v: &wire::GraphObjectIndexEffect, strings: &[String]) -> Value {
    json!({
        "keyTypeNodeId": v.key_type_node_id,
        "valueTypeNodeId": v.value_type_node_id,
        "readonly": v.readonly,
        "spans": v.spans.as_ref().map(|v| project_object_index_spans(v, strings)),
        "declarationOriginNameId": v.declaration_origin_name_id,
        "declarationOriginName": v.has_declaration_origin.then(|| resolve_string(strings, v.declaration_origin_name_id)).flatten(),
        "hasDeclarationOrigin": v.has_declaration_origin,
    })
}

fn project_object_index_spans(v: &wire::GraphObjectIndexSpans, strings: &[String]) -> Value {
    json!({
        "declaration": v.declaration.as_ref().map(|v| project_span_ref(v, strings)),
        "key": v.key.as_ref().map(|v| project_span_ref(v, strings)),
        "value": v.value.as_ref().map(|v| project_span_ref(v, strings)),
    })
}

fn project_object_signature_effect(
    v: &wire::GraphObjectSignatureEffect,
    _strings: &[String],
) -> Value {
    json!({
        "signatureNodeId": v.signature_node_id,
    })
}

fn project_object_spread_effect(v: &wire::GraphObjectSpreadEffect, _strings: &[String]) -> Value {
    json!({
        "operandNodeId": v.operand_node_id,
    })
}

fn project_intrinsic_application(
    v: &wire::GraphIntrinsicApplication,
    _strings: &[String],
) -> Value {
    json!({
        "op": enum_name!(wire::CompilerIntrinsicTypeOp, v.op),
        "argumentNodeIds": v.argument_node_ids,
    })
}

fn project_symbol_node(v: &wire::GraphSymbolNode, strings: &[String]) -> Value {
    json!({
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
        "canonicalNameId": v.canonical_name_id,
        "canonicalName": resolve_string(strings, v.canonical_name_id),
        "namespace": enum_name!(wire::GraphSymbolNamespace, v.namespace),
        "declSlotRef": v.decl_slot_ref,
    })
}

fn project_signature(v: &wire::GraphSignature, strings: &[String]) -> Value {
    json!({
        "typeParameterNodeIds": v.type_parameter_node_ids,
        "thisParam": v.this_param.as_ref().map(|v| project_this_parameter(v, strings)),
        "parameters": v.parameters.iter().map(|v| project_signature_parameter(v, strings)).collect::<Vec<_>>(),
        "returnTypeNodeId": v.return_type_node_id,
        "returnPredicate": v.return_predicate.as_ref().map(|v| project_type_predicate(v, strings)),
        "asserts": v.asserts.as_ref().map(|v| project_assertion_effect(v, strings)),
        "overloadIndex": v.overload_index,
        "isConstruct": v.is_construct,
        "isImplementation": v.is_implementation,
        "isAbstract": v.is_abstract,
        "flags": v.flags,
        "signatureKind": enum_name!(wire::GraphSignatureKind, v.signature_kind),
        "signatureOrigin": enum_name!(wire::GraphSignatureOrigin, v.signature_origin),
    })
}

fn project_this_parameter(v: &wire::GraphThisParameter, _strings: &[String]) -> Value {
    json!({
        "present": v.present,
        "typeNodeId": v.type_node_id,
    })
}

fn project_signature_parameter(v: &wire::GraphSignatureParameter, strings: &[String]) -> Value {
    json!({
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
        "typeNodeId": v.type_node_id,
        "optional": v.optional,
        "rest": v.rest,
        "inferencePolicy": enum_name!(wire::GraphInferencePolicy, v.inference_policy),
    })
}

fn project_type_predicate(v: &wire::GraphTypePredicate, strings: &[String]) -> Value {
    json!({
        "present": v.present,
        "subject": v.subject.as_ref().map(|v| project_predicate_subject(v, strings)),
        "predicateTypeNodeId": v.predicate_type_node_id,
        "asserts": v.asserts,
    })
}

fn project_predicate_subject(v: &wire::GraphPredicateSubject, strings: &[String]) -> Value {
    use wire::graph_predicate_subject::Kind;
    match &v.kind {
        Some(Kind::Identifier(v)) => tagged(
            "identifier",
            project_predicate_subject_identifier(v, strings),
        ),
        Some(Kind::ThisSubject(v)) => {
            tagged("thisSubject", project_predicate_subject_this(v, strings))
        }
        None => Value::Null,
    }
}

fn project_predicate_subject_identifier(
    v: &wire::GraphPredicateSubjectIdentifier,
    strings: &[String],
) -> Value {
    json!({
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
    })
}

fn project_predicate_subject_this(
    _v: &wire::GraphPredicateSubjectThis,
    _strings: &[String],
) -> Value {
    json!({})
}

fn project_assertion_effect(v: &wire::GraphAssertionEffect, strings: &[String]) -> Value {
    use wire::graph_assertion_effect::Kind;
    let kind = match &v.kind {
        Some(Kind::Identifier(v)) => tagged(
            "identifier",
            project_assertion_effect_identifier(v, strings),
        ),
        Some(Kind::ThisAssert(v)) => {
            tagged("thisAssert", project_assertion_effect_this(v, strings))
        }
        Some(Kind::Condition(v)) => {
            tagged("condition", project_assertion_effect_condition(v, strings))
        }
        None => Value::Null,
    };
    json!({
        "present": v.present,
        "kind": kind,
    })
}

fn project_assertion_effect_identifier(
    v: &wire::GraphAssertionEffectIdentifier,
    strings: &[String],
) -> Value {
    json!({
        "nameId": v.name_id,
        "name": resolve_string(strings, v.name_id),
        "predicateNodeId": v.predicate_node_id,
    })
}

fn project_assertion_effect_this(v: &wire::GraphAssertionEffectThis, _strings: &[String]) -> Value {
    json!({
        "predicateNodeId": v.predicate_node_id,
    })
}

fn project_assertion_effect_condition(
    _v: &wire::GraphAssertionEffectCondition,
    _strings: &[String],
) -> Value {
    json!({})
}

fn project_origin_edge(v: &wire::GraphOriginEdge, strings: &[String]) -> Value {
    json!({
        "sourceNodeId": v.source_node_id,
        "targetNodeId": v.target_node_id,
        "kind": enum_name!(wire::GraphOriginEdgeKind, v.kind),
        "metaNameId": v.meta_name_id,
        "metaName": v.has_meta.then(|| resolve_string(strings, v.meta_name_id)).flatten(),
        "hasMeta": v.has_meta,
    })
}

fn project_node_status(v: &wire::GraphNodeStatus, _strings: &[String]) -> Value {
    json!({
        "nodeId": v.node_id,
        "exactness": enum_name!(wire::GraphExactness, v.exactness),
    })
}

fn project_diagnostic(v: &wire::GraphDiagnostic, strings: &[String]) -> Value {
    json!({
        "severity": enum_name!(wire::GraphDiagnosticSeverity, v.severity),
        "messageNameId": v.message_name_id,
        "messageName": resolve_string(strings, v.message_name_id),
        "spanCanonicalNameId": v.span_canonical_name_id,
        "spanCanonicalName": v.has_span.then(|| resolve_string(strings, v.span_canonical_name_id)).flatten(),
        "spanStart": v.span_start,
        "spanEnd": v.span_end,
        "hasSpan": v.has_span,
    })
}

fn project_node_id_map_entry(v: &wire::GraphNodeIdMapEntry, strings: &[String]) -> Value {
    json!({
        "nodeId": v.node_id,
        "identity": v.identity.as_ref().map(|v| project_resolved_decl_slot_identity(v, strings)),
        "nodeRef": v.node_ref.as_ref().map(|v| project_node_ref(v, strings)),
    })
}

fn project_resolved_decl_slot_identity(
    v: &wire::GraphResolvedDeclSlotIdentity,
    strings: &[String],
) -> Value {
    json!({
        "canonicalNameId": v.canonical_name_id,
        "canonicalName": resolve_string(strings, v.canonical_name_id),
        "declNameId": v.decl_name_id,
        "declName": resolve_string(strings, v.decl_name_id),
        "wholeHash": v.whole_hash,
        "namespace": enum_name!(wire::GraphSymbolNamespace, v.namespace),
    })
}

fn project_node_ref(v: &wire::NodeRef, _strings: &[String]) -> Value {
    json!({
        "queryIdentityDigest": v.query_identity_digest,
        "inputBasisDigest": v.input_basis_digest,
        "declSlotDigest": v.decl_slot_digest,
        "roleDigest": v.role_digest,
    })
}

fn project_symbol_id_map_entry(v: &wire::GraphSymbolIdMapEntry, strings: &[String]) -> Value {
    json!({
        "symbolId": v.symbol_id,
        "identity": v.identity.as_ref().map(|v| project_resolved_decl_slot_identity(v, strings)),
    })
}

fn project_string_table(v: &wire::GraphStringTable, _strings: &[String]) -> Value {
    json!({
        "entries": v.entries,
    })
}

fn project_relation_proof_entry(v: &wire::GraphRelationProofEntry, strings: &[String]) -> Value {
    use wire::graph_relation_proof_entry::Kind;
    match &v.kind {
        Some(Kind::Assignable(v)) => tagged("assignable", project_relation_derivation(v, strings)),
        Some(Kind::NotAssignable(v)) => {
            tagged("notAssignable", project_relation_failure(v, strings))
        }
        Some(Kind::BudgetExceeded(v)) => {
            tagged("budgetExceeded", project_relation_budget_cap(v, strings))
        }
        Some(Kind::CoinductiveCycle(v)) => {
            tagged("coinductiveCycle", project_relation_cycle_keys(v, strings))
        }
        None => Value::Null,
    }
}

fn project_relation_derivation(v: &wire::GraphRelationDerivation, strings: &[String]) -> Value {
    json!({
        "subDerivations": v.sub_derivations.iter().map(|v| project_sub_relation_ref(v, strings)).collect::<Vec<_>>(),
    })
}

fn project_sub_relation_ref(v: &wire::GraphSubRelationRef, strings: &[String]) -> Value {
    json!({
        "sourceNodeId": v.source_node_id,
        "targetNodeId": v.target_node_id,
        "positionKind": enum_name!(wire::GraphSubRelationPositionKind, v.position_kind),
        "positionNameId": v.position_name_id,
        "positionName": if v.position_kind == wire::GraphSubRelationPositionKind::Member as i32 {
            resolve_string(strings, v.position_name_id)
        } else {
            None
        },
        "positionIndex": v.position_index,
    })
}

fn project_relation_failure(v: &wire::GraphRelationFailure, strings: &[String]) -> Value {
    json!({
        "reason": enum_name!(wire::GraphRelationFailureCode, v.reason),
        "failingSub": v.failing_sub.as_ref().map(|v| project_sub_relation_ref(v, strings)),
    })
}

fn project_relation_budget_cap(v: &wire::GraphRelationBudgetCap, _strings: &[String]) -> Value {
    json!({
        "budgetKind": enum_name!(wire::GraphBudgetExceededKind, v.kind),
        "limit": v.limit,
    })
}

fn project_relation_cycle_keys(v: &wire::GraphRelationCycleKeys, _strings: &[String]) -> Value {
    json!({
        "keys": v.keys,
    })
}

fn project_fact_identity(v: &wire::GraphFactIdentity, _strings: &[String]) -> Value {
    json!({
        "queryIdentityDigest": v.query_identity_digest,
        "inputBasisDigest": v.input_basis_digest,
    })
}
