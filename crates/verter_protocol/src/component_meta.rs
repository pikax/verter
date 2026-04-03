use std::sync::Arc;

use crate::graph::{
    GraphBuilder, GraphFunctionParam, GraphNode, GraphObjectMember, GraphTupleElement,
};
use crate::types::*;
use prost::Message;
use verter_semantic::analysis::type_expr::{
    empty_type_args, ObjectExpr, ObjectMember as TypeObjectMember, ObjectProperty, PrimitiveName,
    TypeExpr,
};

use crate::verter::v1::{
    self as proto, type_node, AcceptedEventMeta, AcceptedPropMeta, ArrayNode, BindingMeta,
    BranchStatus, ComponentFlags, ComponentMetaBody, ComponentMetaPayload, ComponentMetaResolution,
    ComponentPropUsage, ComponentUsage, ConditionalNode, ConsumedRootBindings,
    CustomBlockMeta as ProtoCustomBlockMeta, EventMeta, ExpansionDiagnostic, ExpansionMetadata,
    ExposedMeta, FallthroughBranch, FallthroughEventEntry, FallthroughPropEntry,
    FallthroughSurface, FunctionNode, FunctionParameter, ImportBindingMeta, ImportMeta,
    IndexedAccessNode, InferNode, InheritedSource, JsdocTag, KeyOfNode, LiteralNode, MappedNode,
    MemberAvailability, MemberProvenance, ModelMeta, ObjectMember as ProtoObjectMember, ObjectNode,
    ParenthesizedNode, PartialBranchReason, PropMeta, PublicInstanceMemberMeta, PublicInstanceMeta,
    RefNode, ResolvedEmitField, ResolvedJsdocBlock, ResolvedJsdocTag, ResolvedMacroMeta,
    ResolvedNativeProp, ResolvedPropField, ResolvedRootStep, ResolvedSlotBinding,
    ResolvedSlotField, ResolvedTypeDeclaration, RestNode, RootBranch, RootInfo, RootReachability,
    RootTargetRef, ScriptBlockMeta, SelectorMeta, SfcAttributeMeta, SfcBlocksMeta, SlotBindingMeta,
    SlotMeta, StyleBlockMeta as ProtoStyleBlockMeta, StyleMeta, TemplateBlockMeta,
    TemplateLiteralNode, TemplateRefMeta, TupleElement, TupleNode, TypeGraph, TypeNode, TypeOfNode,
    TypeParameterNode, TypeRegistryEntry, UnionNode, UnknownNode, UnresolvedBranchReason,
    UnresolvedRootTargetReason, VueApiCallMeta,
};

pub const COMPONENT_META_SCHEMA_VERSION: u32 = 1;

pub fn component_meta_payload(meta: &FfiComponentMeta) -> ComponentMetaPayload {
    let mut builder = GraphBuilder::new();
    let type_registry = meta
        .type_registry
        .iter()
        .map(|entry| type_registry_entry_to_proto(&mut builder, entry))
        .collect();
    let body = Some(component_meta_body_to_proto(&mut builder, meta));
    let type_graph = Some(TypeGraph {
        strings: builder.strings().to_vec(),
        nodes: builder.nodes().iter().map(graph_node_to_proto).collect(),
    });

    ComponentMetaPayload {
        schema_version: COMPONENT_META_SCHEMA_VERSION,
        type_graph,
        type_registry,
        body,
    }
}

pub fn encode_component_meta_payload(meta: &FfiComponentMeta) -> Vec<u8> {
    component_meta_payload(meta).encode_to_vec()
}

pub fn build_test_payload() -> ComponentMetaPayload {
    component_meta_payload(&build_test_meta())
}

fn component_meta_body_to_proto(
    builder: &mut GraphBuilder,
    meta: &FfiComponentMeta,
) -> ComponentMetaBody {
    ComponentMetaBody {
        file_path_id: builder.string_id(&meta.file_path),
        options_api: meta.options_api,
        props: meta
            .props
            .iter()
            .map(|prop| prop_meta_to_proto(builder, prop))
            .collect(),
        events: meta
            .events
            .iter()
            .map(|event| event_meta_to_proto(builder, event))
            .collect(),
        slots: meta
            .slots
            .iter()
            .map(|slot| slot_meta_to_proto(builder, slot))
            .collect(),
        models: meta
            .models
            .iter()
            .map(|model| ModelMeta {
                name_id: builder.string_id(&model.name),
                type_node_id: builder.node_id(&model.r#type),
            })
            .collect(),
        exposed: meta
            .exposed
            .iter()
            .map(|exposed| ExposedMeta {
                name_id: builder.string_id(&exposed.name),
                type_node_id: builder.node_id(&exposed.r#type),
                type_expansion: exposed
                    .type_expansion
                    .as_ref()
                    .map(|metadata| expansion_metadata_to_proto(builder, metadata)),
                description_id: builder.string_id_opt(exposed.description.as_deref()),
            })
            .collect(),
        components: meta
            .components
            .iter()
            .map(|component| component_usage_to_proto(builder, component))
            .collect(),
        template_refs: meta
            .template_refs
            .iter()
            .map(|template_ref| TemplateRefMeta {
                name_id: builder.string_id(&template_ref.name),
                is_dynamic: template_ref.is_dynamic,
                target_tag_id: builder.string_id(&template_ref.target_tag),
            })
            .collect(),
        imports: meta
            .imports
            .iter()
            .map(|import| import_meta_to_proto(builder, import))
            .collect(),
        bindings: meta
            .bindings
            .iter()
            .map(|binding| BindingMeta {
                name_id: builder.string_id(&binding.name),
                kind_id: builder.string_id(&binding.kind),
                reactivity_kind_id: builder.string_id(&binding.reactivity_kind),
                type_annotation_id: builder.string_id_opt(binding.type_annotation.as_deref()),
                used_in_template: binding.used_in_template,
                used_in_style: binding.used_in_style,
            })
            .collect(),
        vue_api_calls: meta
            .vue_api_calls
            .iter()
            .map(|call| VueApiCallMeta {
                api_id: builder.string_id(&call.api),
                arg_value_id: builder.string_id_opt(call.arg_value.as_deref()),
            })
            .collect(),
        styles: meta
            .styles
            .iter()
            .map(|style| style_meta_to_proto(builder, style))
            .collect(),
        flags: Some(ComponentFlags {
            async_setup: meta.flags.async_setup,
            has_reactive_state: meta.flags.has_reactive_state,
            has_computed: meta.flags.has_computed,
            has_watchers: meta.flags.has_watchers,
            has_lifecycle_hooks: meta.flags.has_lifecycle_hooks,
            has_provide: meta.flags.has_provide,
            has_inject: meta.flags.has_inject,
            has_inherit_attrs_false: meta.flags.has_inherit_attrs_false,
            has_store_usage: meta.flags.has_store_usage,
        }),
        accepted_props: meta
            .accepted_props
            .iter()
            .map(|prop| accepted_prop_meta_to_proto(builder, prop))
            .collect(),
        accepted_events: meta
            .accepted_events
            .iter()
            .map(|event| accepted_event_meta_to_proto(builder, event))
            .collect(),
        accepted_surface_completeness: accepted_surface_completeness_to_proto(
            &meta.accepted_surface_completeness,
        ) as i32,
        root_reachability: Some(root_reachability_to_proto(builder, &meta.root_reachability)),
        fallthrough_surface: Some(fallthrough_surface_to_proto(
            builder,
            &meta.fallthrough_surface,
        )),
        root_info: Some(root_info_to_proto(builder, &meta.root_info)),
        public_instance: meta
            .public_instance
            .as_ref()
            .map(|public_instance| public_instance_to_proto(builder, public_instance)),
        sfc_blocks: meta
            .sfc_blocks
            .as_ref()
            .map(|blocks| sfc_blocks_to_proto(builder, blocks)),
        resolution: meta
            .resolution
            .as_ref()
            .map(|resolution| component_meta_resolution_to_proto(builder, resolution)),
    }
}

fn prop_meta_to_proto(builder: &mut GraphBuilder, prop: &FfiPropMeta) -> PropMeta {
    PropMeta {
        name_id: builder.string_id(&prop.name),
        type_node_id: builder.node_id(&prop.r#type),
        type_expansion: prop
            .type_expansion
            .as_ref()
            .map(|metadata| expansion_metadata_to_proto(builder, metadata)),
        raw_type_id: builder.string_id_opt(prop.raw_type.as_deref()),
        required: prop.required,
        has_default: prop.has_default,
        default_value_id: builder.string_id_opt(prop.default_value.as_deref()),
        description_id: builder.string_id_opt(prop.description.as_deref()),
        tags: prop
            .tags
            .iter()
            .map(|tag| jsdoc_tag_to_proto(builder, tag))
            .collect(),
    }
}

fn event_meta_to_proto(builder: &mut GraphBuilder, event: &FfiEventMeta) -> EventMeta {
    EventMeta {
        name_id: builder.string_id(&event.name),
        payload_node_id: builder.node_id(&event.payload),
        payload_expansion: event
            .payload_expansion
            .as_ref()
            .map(|metadata| expansion_metadata_to_proto(builder, metadata)),
        raw_signature_id: builder.string_id_opt(event.raw_signature.as_deref()),
        description_id: builder.string_id_opt(event.description.as_deref()),
        tags: event
            .tags
            .iter()
            .map(|tag| jsdoc_tag_to_proto(builder, tag))
            .collect(),
    }
}

fn slot_meta_to_proto(builder: &mut GraphBuilder, slot: &FfiSlotMeta) -> SlotMeta {
    SlotMeta {
        name_id: builder.string_id(&slot.name),
        is_scoped: slot.is_scoped,
        bindings: slot
            .bindings
            .iter()
            .map(|binding| SlotBindingMeta {
                name_id: builder.string_id(&binding.name),
                type_node_id: builder.node_id(&binding.r#type),
                type_expansion: binding
                    .type_expansion
                    .as_ref()
                    .map(|metadata| expansion_metadata_to_proto(builder, metadata)),
                raw_type_id: builder.string_id_opt(binding.raw_type.as_deref()),
            })
            .collect(),
        is_required: slot.is_required,
        return_type_id: builder.string_id_opt(slot.return_type.as_deref()),
        description_id: builder.string_id_opt(slot.description.as_deref()),
        tags: slot
            .tags
            .iter()
            .map(|tag| jsdoc_tag_to_proto(builder, tag))
            .collect(),
    }
}

fn type_registry_entry_to_proto(
    builder: &mut GraphBuilder,
    entry: &FfiResolvedTypeMeta,
) -> TypeRegistryEntry {
    TypeRegistryEntry {
        name_id: builder.string_id(&entry.name),
        type_node_id: builder.node_id(&entry.r#type),
        type_expansion: entry
            .type_expansion
            .as_ref()
            .map(|metadata| expansion_metadata_to_proto(builder, metadata)),
        raw_type_id: builder.string_id_opt(entry.raw_type.as_deref()),
        declaration: entry
            .declaration
            .as_ref()
            .map(|declaration| resolved_type_declaration_to_proto(builder, declaration)),
    }
}

fn component_usage_to_proto(
    builder: &mut GraphBuilder,
    component: &FfiComponentUsage,
) -> ComponentUsage {
    ComponentUsage {
        name_id: builder.string_id(&component.name),
        import_source_id: builder.string_id_opt(component.import_source.as_deref()),
        is_dynamic: component.is_dynamic,
        props: component
            .props
            .iter()
            .map(|prop| ComponentPropUsage {
                name_id: builder.string_id(&prop.name),
                is_bound: prop.is_bound,
                constness_id: builder.string_id(&prop.constness),
            })
            .collect(),
        slots_used_ids: string_ids(builder, &component.slots_used),
        static_class_ids: string_ids(builder, &component.static_classes),
        has_dynamic_class: component.has_dynamic_class,
        v_model_ids: string_ids(builder, &component.v_models),
    }
}

fn import_meta_to_proto(builder: &mut GraphBuilder, import: &FfiImportMeta) -> ImportMeta {
    ImportMeta {
        source_id: builder.string_id(&import.source),
        is_type_only: import.is_type_only,
        bindings: import
            .bindings
            .iter()
            .map(|binding| ImportBindingMeta {
                name_id: builder.string_id(&binding.name),
                kind_id: builder.string_id(&binding.kind),
                imported_name_id: builder.string_id_opt(binding.imported_name.as_deref()),
                is_type_only: binding.is_type_only,
            })
            .collect(),
    }
}

fn style_meta_to_proto(builder: &mut GraphBuilder, style: &FfiStyleMeta) -> StyleMeta {
    StyleMeta {
        lang_id: builder.string_id(&style.lang),
        scoped: style.scoped,
        is_module: style.is_module,
        module_name_id: builder.string_id_opt(style.module_name.as_deref()),
        class_ids: string_ids(builder, &style.classes),
        id_ids: string_ids(builder, &style.ids),
        custom_property_ids: string_ids(builder, &style.custom_properties),
        v_bind_ids: string_ids(builder, &style.v_binds),
        selectors: style
            .selectors
            .iter()
            .map(|selector| SelectorMeta {
                text_id: builder.string_id(&selector.text),
                specificity_a: selector.specificity.0,
                specificity_b: selector.specificity.1,
                specificity_c: selector.specificity.2,
            })
            .collect(),
    }
}

fn accepted_prop_meta_to_proto(
    builder: &mut GraphBuilder,
    prop: &FfiAcceptedPropMeta,
) -> AcceptedPropMeta {
    AcceptedPropMeta {
        name_id: builder.string_id(&prop.name),
        type_node_id: builder.node_id(&prop.r#type),
        raw_type_id: builder.string_id_opt(prop.raw_type.as_deref()),
        required: prop.required,
        provenance: Some(member_provenance_to_proto(builder, &prop.provenance)),
        availability: Some(member_availability_to_proto(builder, &prop.availability)),
        kind: accepted_prop_kind_to_proto(&prop.kind) as i32,
    }
}

fn accepted_event_meta_to_proto(
    builder: &mut GraphBuilder,
    event: &FfiAcceptedEventMeta,
) -> AcceptedEventMeta {
    AcceptedEventMeta {
        name_id: builder.string_id(&event.name),
        payload_node_id: builder.node_id(&event.payload),
        raw_signature_id: builder.string_id_opt(event.raw_signature.as_deref()),
        provenance: Some(member_provenance_to_proto(builder, &event.provenance)),
        availability: Some(member_availability_to_proto(builder, &event.availability)),
        kind: accepted_event_kind_to_proto(&event.kind) as i32,
    }
}

fn member_provenance_to_proto(
    builder: &mut GraphBuilder,
    provenance: &FfiMemberProvenance,
) -> MemberProvenance {
    match provenance {
        FfiMemberProvenance::Declared => MemberProvenance {
            kind: proto::MemberProvenanceKind::Declared as i32,
            sources: Vec::new(),
        },
        FfiMemberProvenance::Inherited { sources } => MemberProvenance {
            kind: proto::MemberProvenanceKind::Inherited as i32,
            sources: sources
                .iter()
                .map(|source| inherited_source_to_proto(builder, source))
                .collect(),
        },
    }
}

fn inherited_source_to_proto(
    builder: &mut GraphBuilder,
    source: &FfiInheritedSource,
) -> InheritedSource {
    match source {
        FfiInheritedSource::NativeTag { tag } => InheritedSource {
            kind: proto::InheritedSourceKind::NativeTag as i32,
            tag_id: builder.string_id(tag),
            canonical_id_id: 0,
        },
        FfiInheritedSource::Component { canonical_id } => InheritedSource {
            kind: proto::InheritedSourceKind::Component as i32,
            tag_id: 0,
            canonical_id_id: builder.string_id(canonical_id),
        },
    }
}

fn member_availability_to_proto(
    builder: &mut GraphBuilder,
    availability: &FfiMemberAvailability,
) -> MemberAvailability {
    match availability {
        FfiMemberAvailability::Always => MemberAvailability {
            kind: proto::MemberAvailabilityKind::Always as i32,
            branch_key_ids: Vec::new(),
        },
        FfiMemberAvailability::Conditional { branch_keys } => MemberAvailability {
            kind: proto::MemberAvailabilityKind::Conditional as i32,
            branch_key_ids: string_ids(builder, branch_keys),
        },
    }
}

fn root_reachability_to_proto(
    builder: &mut GraphBuilder,
    reachability: &FfiRootReachability,
) -> RootReachability {
    match reachability {
        FfiRootReachability::NoFallthrough { reason } => RootReachability {
            kind: proto::RootReachabilityKind::NoFallthrough as i32,
            reason: no_fallthrough_reason_to_proto(reason) as i32,
            branches: Vec::new(),
        },
        FfiRootReachability::Branches { branches } => RootReachability {
            kind: proto::RootReachabilityKind::Branches as i32,
            reason: proto::NoFallthroughReason::Unspecified as i32,
            branches: branches
                .iter()
                .map(|branch| root_branch_to_proto(builder, branch))
                .collect(),
        },
    }
}

fn root_info_to_proto(builder: &mut GraphBuilder, info: &FfiRootInfo) -> RootInfo {
    RootInfo {
        kind: match info.kind {
            FfiRootInfoKind::None => proto::RootInfoKind::None,
            FfiRootInfoKind::Single => proto::RootInfoKind::Single,
            FfiRootInfoKind::Conditional => proto::RootInfoKind::Conditional,
            FfiRootInfoKind::Multiple => proto::RootInfoKind::Multiple,
        } as i32,
        reason: info
            .reason
            .as_ref()
            .map(no_fallthrough_reason_to_proto)
            .unwrap_or(proto::NoFallthroughReason::Unspecified) as i32,
        targets: info
            .targets
            .iter()
            .map(|target| root_target_ref_to_proto(builder, target))
            .collect(),
    }
}

fn public_instance_to_proto(
    builder: &mut GraphBuilder,
    public_instance: &FfiPublicInstanceMeta,
) -> PublicInstanceMeta {
    PublicInstanceMeta {
        completeness: match public_instance.completeness.as_str() {
            "exact" => proto::PublicInstanceCompleteness::Exact,
            _ => proto::PublicInstanceCompleteness::Partial,
        } as i32,
        members: public_instance
            .members
            .iter()
            .map(|member| PublicInstanceMemberMeta {
                name_id: builder.string_id(&member.name),
                kind: match member.kind.as_str() {
                    "prop" => proto::PublicInstanceMemberKind::Prop,
                    "slotContainer" => proto::PublicInstanceMemberKind::SlotContainer,
                    "exposed" => proto::PublicInstanceMemberKind::Exposed,
                    _ => proto::PublicInstanceMemberKind::Unspecified,
                } as i32,
                type_node_id: builder.node_id(&member.r#type),
                type_expansion: member
                    .type_expansion
                    .as_ref()
                    .map(|metadata| expansion_metadata_to_proto(builder, metadata)),
                raw_type_id: builder.string_id_opt(member.raw_type.as_deref()),
                description_id: builder.string_id_opt(member.description.as_deref()),
            })
            .collect(),
    }
}

fn sfc_blocks_to_proto(builder: &mut GraphBuilder, blocks: &FfiSfcBlocksMeta) -> SfcBlocksMeta {
    SfcBlocksMeta {
        template: blocks
            .template
            .as_ref()
            .map(|template| template_block_to_proto(builder, template)),
        script: blocks
            .script
            .as_ref()
            .map(|script| script_block_to_proto(builder, script)),
        script_setup: blocks
            .script_setup
            .as_ref()
            .map(|script| script_block_to_proto(builder, script)),
        styles: blocks
            .styles
            .iter()
            .map(|style| style_block_to_proto(builder, style))
            .collect(),
        custom: blocks
            .custom
            .iter()
            .map(|custom| custom_block_to_proto(builder, custom))
            .collect(),
    }
}

fn sfc_attribute_to_proto(
    builder: &mut GraphBuilder,
    attribute: &FfiSfcAttributeMeta,
) -> SfcAttributeMeta {
    SfcAttributeMeta {
        name_id: builder.string_id(&attribute.name),
        value_id: builder.string_id_opt(attribute.value.as_deref()),
    }
}

fn template_block_to_proto(
    builder: &mut GraphBuilder,
    block: &FfiTemplateBlockMeta,
) -> TemplateBlockMeta {
    TemplateBlockMeta {
        lang_id: builder.string_id_opt(block.lang.as_deref()),
        src_id: builder.string_id_opt(block.src.as_deref()),
        attributes: block
            .attributes
            .iter()
            .map(|attribute| sfc_attribute_to_proto(builder, attribute))
            .collect(),
    }
}

fn script_block_to_proto(
    builder: &mut GraphBuilder,
    block: &FfiScriptBlockMeta,
) -> ScriptBlockMeta {
    ScriptBlockMeta {
        lang_id: builder.string_id_opt(block.lang.as_deref()),
        src_id: builder.string_id_opt(block.src.as_deref()),
        generic_id: builder.string_id_opt(block.generic.as_deref()),
        attrs_type_id: builder.string_id_opt(block.attrs_type.as_deref()),
        attributes: block
            .attributes
            .iter()
            .map(|attribute| sfc_attribute_to_proto(builder, attribute))
            .collect(),
    }
}

fn style_block_to_proto(
    builder: &mut GraphBuilder,
    block: &FfiStyleBlockMeta,
) -> ProtoStyleBlockMeta {
    ProtoStyleBlockMeta {
        index: block.index,
        lang_id: builder.string_id_opt(block.lang.as_deref()),
        src_id: builder.string_id_opt(block.src.as_deref()),
        scoped: block.scoped,
        is_module: block.is_module,
        module_name_id: builder.string_id_opt(block.module_name.as_deref()),
        attributes: block
            .attributes
            .iter()
            .map(|attribute| sfc_attribute_to_proto(builder, attribute))
            .collect(),
    }
}

fn custom_block_to_proto(
    builder: &mut GraphBuilder,
    block: &FfiCustomBlockMeta,
) -> ProtoCustomBlockMeta {
    ProtoCustomBlockMeta {
        index: block.index,
        block_type_id: builder.string_id(&block.block_type),
        lang_id: builder.string_id_opt(block.lang.as_deref()),
        src_id: builder.string_id_opt(block.src.as_deref()),
        attributes: block
            .attributes
            .iter()
            .map(|attribute| sfc_attribute_to_proto(builder, attribute))
            .collect(),
    }
}

fn root_branch_to_proto(builder: &mut GraphBuilder, branch: &FfiRootBranch) -> RootBranch {
    RootBranch {
        branch_index: u32::from(branch.branch_index),
        condition_text_id: builder.string_id_opt(branch.condition_text.as_deref()),
        target: Some(root_target_ref_to_proto(builder, &branch.target)),
        consumed: Some(consumed_root_bindings_to_proto(builder, &branch.consumed)),
        has_unknown_spread: branch.has_unknown_spread,
    }
}

fn root_target_ref_to_proto(
    builder: &mut GraphBuilder,
    target: &FfiRootTargetRef,
) -> RootTargetRef {
    match target {
        FfiRootTargetRef::NativeElement { element_index, tag } => RootTargetRef {
            kind: proto::RootTargetKind::NativeElement as i32,
            element_index: *element_index,
            usage_index: 0,
            tag_id: builder.string_id(tag),
            name_id: 0,
            import_source_id: 0,
            unresolved_reason: None,
        },
        FfiRootTargetRef::DynamicComponentUsage {
            element_index,
            usage_index,
        } => RootTargetRef {
            kind: proto::RootTargetKind::DynamicComponentUsage as i32,
            element_index: *element_index,
            usage_index: *usage_index,
            tag_id: 0,
            name_id: 0,
            import_source_id: 0,
            unresolved_reason: None,
        },
        FfiRootTargetRef::ComponentUsage {
            element_index,
            usage_index,
            name,
            import_source,
        } => RootTargetRef {
            kind: proto::RootTargetKind::ComponentUsage as i32,
            element_index: *element_index,
            usage_index: *usage_index,
            tag_id: 0,
            name_id: builder.string_id(name),
            import_source_id: builder.string_id_opt(import_source.as_deref()),
            unresolved_reason: None,
        },
        FfiRootTargetRef::UnresolvedTarget {
            element_index,
            tag,
            reason,
        } => RootTargetRef {
            kind: proto::RootTargetKind::Unresolved as i32,
            element_index: *element_index,
            usage_index: 0,
            tag_id: builder.string_id(tag),
            name_id: 0,
            import_source_id: 0,
            unresolved_reason: Some(unresolved_root_target_reason_to_proto(builder, reason)),
        },
    }
}

fn unresolved_root_target_reason_to_proto(
    builder: &mut GraphBuilder,
    reason: &FfiUnresolvedRootTargetReason,
) -> UnresolvedRootTargetReason {
    match reason {
        FfiUnresolvedRootTargetReason::DynamicComponentIs => UnresolvedRootTargetReason {
            kind: proto::UnresolvedRootTargetReasonKind::DynamicComponentIs as i32,
            tag_id: 0,
        },
        FfiUnresolvedRootTargetReason::SlotOutlet => UnresolvedRootTargetReason {
            kind: proto::UnresolvedRootTargetReasonKind::SlotOutlet as i32,
            tag_id: 0,
        },
        FfiUnresolvedRootTargetReason::UnsupportedBuiltin { tag } => UnresolvedRootTargetReason {
            kind: proto::UnresolvedRootTargetReasonKind::UnsupportedBuiltin as i32,
            tag_id: builder.string_id(tag),
        },
        FfiUnresolvedRootTargetReason::MissingUsageLink => UnresolvedRootTargetReason {
            kind: proto::UnresolvedRootTargetReasonKind::MissingUsageLink as i32,
            tag_id: 0,
        },
        FfiUnresolvedRootTargetReason::UnresolvedImport => UnresolvedRootTargetReason {
            kind: proto::UnresolvedRootTargetReasonKind::UnresolvedImport as i32,
            tag_id: 0,
        },
        FfiUnresolvedRootTargetReason::UnknownRootTarget => UnresolvedRootTargetReason {
            kind: proto::UnresolvedRootTargetReasonKind::UnknownRootTarget as i32,
            tag_id: 0,
        },
    }
}

fn consumed_root_bindings_to_proto(
    builder: &mut GraphBuilder,
    bindings: &FfiConsumedRootBindings,
) -> ConsumedRootBindings {
    ConsumedRootBindings {
        attr_ids: string_ids(builder, &bindings.attrs),
        listener_ids: string_ids(builder, &bindings.listeners),
        has_dynamic_attr_name: bindings.has_dynamic_attr_name,
        has_dynamic_listener_name: bindings.has_dynamic_listener_name,
    }
}

fn fallthrough_surface_to_proto(
    builder: &mut GraphBuilder,
    surface: &FfiFallthroughSurface,
) -> FallthroughSurface {
    match surface {
        FfiFallthroughSurface::None { reason } => FallthroughSurface {
            kind: proto::FallthroughSurfaceKind::None as i32,
            reason: no_fallthrough_reason_to_proto(reason) as i32,
            branches: Vec::new(),
        },
        FfiFallthroughSurface::Branches { branches } => FallthroughSurface {
            kind: proto::FallthroughSurfaceKind::Branches as i32,
            reason: proto::NoFallthroughReason::Unspecified as i32,
            branches: branches
                .iter()
                .map(|branch| fallthrough_branch_to_proto(builder, branch))
                .collect(),
        },
    }
}

fn fallthrough_branch_to_proto(
    builder: &mut GraphBuilder,
    branch: &FfiFallthroughBranch,
) -> FallthroughBranch {
    FallthroughBranch {
        branch_key_id: builder.string_id(&branch.branch_key),
        condition_text_id: builder.string_id_opt(branch.condition_text.as_deref()),
        props: branch
            .props
            .iter()
            .map(|prop| FallthroughPropEntry {
                name_id: builder.string_id(&prop.name),
                type_node_id: builder.node_id(&prop.r#type),
                raw_type_id: builder.string_id_opt(prop.raw_type.as_deref()),
                sources: prop
                    .sources
                    .iter()
                    .map(|source| inherited_source_to_proto(builder, source))
                    .collect(),
            })
            .collect(),
        events: branch
            .events
            .iter()
            .map(|event| FallthroughEventEntry {
                name_id: builder.string_id(&event.name),
                payload_node_id: builder.node_id(&event.payload),
                raw_signature_id: builder.string_id_opt(event.raw_signature.as_deref()),
                sources: event
                    .sources
                    .iter()
                    .map(|source| inherited_source_to_proto(builder, source))
                    .collect(),
            })
            .collect(),
        root_chain: branch
            .root_chain
            .iter()
            .map(|step| resolved_root_step_to_proto(builder, step))
            .collect(),
        status: Some(branch_status_to_proto(builder, &branch.status)),
    }
}

fn resolved_root_step_to_proto(
    builder: &mut GraphBuilder,
    step: &FfiResolvedRootStep,
) -> ResolvedRootStep {
    match step {
        FfiResolvedRootStep::NativeTag { tag } => ResolvedRootStep {
            kind: proto::ResolvedRootStepKind::NativeTag as i32,
            tag_id: builder.string_id(tag),
            canonical_id_id: 0,
            component_name_id: 0,
            reason: None,
        },
        FfiResolvedRootStep::Component {
            canonical_id,
            component_name,
        } => ResolvedRootStep {
            kind: proto::ResolvedRootStepKind::Component as i32,
            tag_id: 0,
            canonical_id_id: builder.string_id(canonical_id),
            component_name_id: builder.string_id(component_name),
            reason: None,
        },
        FfiResolvedRootStep::Unresolved { tag, reason } => ResolvedRootStep {
            kind: proto::ResolvedRootStepKind::Unresolved as i32,
            tag_id: builder.string_id(tag),
            canonical_id_id: 0,
            component_name_id: 0,
            reason: Some(unresolved_branch_reason_to_proto(builder, reason)),
        },
    }
}

fn branch_status_to_proto(builder: &mut GraphBuilder, status: &FfiBranchStatus) -> BranchStatus {
    match status {
        FfiBranchStatus::Resolved => BranchStatus {
            kind: proto::BranchStatusKind::Resolved as i32,
            reasons: Vec::new(),
            reason: None,
        },
        FfiBranchStatus::PartiallyUnresolved { reasons } => BranchStatus {
            kind: proto::BranchStatusKind::PartiallyUnresolved as i32,
            reasons: reasons.iter().map(partial_branch_reason_to_proto).collect(),
            reason: None,
        },
        FfiBranchStatus::Unresolved { reason } => BranchStatus {
            kind: proto::BranchStatusKind::Unresolved as i32,
            reasons: Vec::new(),
            reason: Some(unresolved_branch_reason_to_proto(builder, reason)),
        },
    }
}

fn partial_branch_reason_to_proto(reason: &FfiPartialBranchReason) -> PartialBranchReason {
    match reason {
        FfiPartialBranchReason::DynamicAttrName => PartialBranchReason {
            kind: proto::PartialBranchReasonKind::DynamicAttrName as i32,
            failure: proto::GenericResolutionFailure::Unspecified as i32,
        },
        FfiPartialBranchReason::DynamicListenerName => PartialBranchReason {
            kind: proto::PartialBranchReasonKind::DynamicListenerName as i32,
            failure: proto::GenericResolutionFailure::Unspecified as i32,
        },
        FfiPartialBranchReason::UnknownSpread => PartialBranchReason {
            kind: proto::PartialBranchReasonKind::UnknownSpread as i32,
            failure: proto::GenericResolutionFailure::Unspecified as i32,
        },
        FfiPartialBranchReason::GenericResolution { failure } => PartialBranchReason {
            kind: proto::PartialBranchReasonKind::GenericResolution as i32,
            failure: generic_resolution_failure_to_proto(failure) as i32,
        },
    }
}

fn unresolved_branch_reason_to_proto(
    builder: &mut GraphBuilder,
    reason: &FfiUnresolvedBranchReason,
) -> UnresolvedBranchReason {
    match reason {
        FfiUnresolvedBranchReason::Cycle { canonical_id } => UnresolvedBranchReason {
            kind: proto::UnresolvedBranchReasonKind::Cycle as i32,
            canonical_id_id: builder.string_id(canonical_id),
            import_source_id: 0,
            root_target_reason: None,
            failure: proto::GenericResolutionFailure::Unspecified as i32,
        },
        FfiUnresolvedBranchReason::DynamicComponentIs => UnresolvedBranchReason {
            kind: proto::UnresolvedBranchReasonKind::DynamicComponentIs as i32,
            canonical_id_id: 0,
            import_source_id: 0,
            root_target_reason: None,
            failure: proto::GenericResolutionFailure::Unspecified as i32,
        },
        FfiUnresolvedBranchReason::ChildResolutionFailed => UnresolvedBranchReason {
            kind: proto::UnresolvedBranchReasonKind::ChildResolutionFailed as i32,
            canonical_id_id: 0,
            import_source_id: 0,
            root_target_reason: None,
            failure: proto::GenericResolutionFailure::Unspecified as i32,
        },
        FfiUnresolvedBranchReason::UnresolvedChildImport { import_source } => {
            UnresolvedBranchReason {
                kind: proto::UnresolvedBranchReasonKind::UnresolvedChildImport as i32,
                canonical_id_id: 0,
                import_source_id: builder.string_id_opt(import_source.as_deref()),
                root_target_reason: None,
                failure: proto::GenericResolutionFailure::Unspecified as i32,
            }
        }
        FfiUnresolvedBranchReason::RootTarget { reason } => UnresolvedBranchReason {
            kind: proto::UnresolvedBranchReasonKind::RootTarget as i32,
            canonical_id_id: 0,
            import_source_id: 0,
            root_target_reason: Some(unresolved_root_target_reason_to_proto(builder, reason)),
            failure: proto::GenericResolutionFailure::Unspecified as i32,
        },
        FfiUnresolvedBranchReason::GenericResolution { failure } => UnresolvedBranchReason {
            kind: proto::UnresolvedBranchReasonKind::GenericResolution as i32,
            canonical_id_id: 0,
            import_source_id: 0,
            root_target_reason: None,
            failure: generic_resolution_failure_to_proto(failure) as i32,
        },
    }
}

fn component_meta_resolution_to_proto(
    builder: &mut GraphBuilder,
    resolution: &FfiComponentMetaResolution,
) -> ComponentMetaResolution {
    ComponentMetaResolution {
        mode_id: builder.string_id(&resolution.mode),
        macros: resolution
            .macros
            .iter()
            .map(|mac| resolved_macro_meta_to_proto(builder, mac))
            .collect(),
    }
}

fn resolved_macro_meta_to_proto(
    builder: &mut GraphBuilder,
    mac: &FfiResolvedMacroMeta,
) -> ResolvedMacroMeta {
    ResolvedMacroMeta {
        macro_index: mac.macro_index,
        macro_kind_id: builder.string_id(&mac.macro_kind),
        type_name_id: builder.string_id(&mac.type_name),
        import_source_id: builder.string_id(&mac.import_source),
        declaration: Some(resolved_type_declaration_to_proto(
            builder,
            &mac.declaration,
        )),
        native_props: mac
            .native_props
            .iter()
            .map(|prop| ResolvedNativeProp {
                name_id: builder.string_id(&prop.name),
                is_optional: prop.is_optional,
                type_annotation_id: builder.string_id_opt(prop.type_annotation.as_deref()),
                visibility_id: builder.string_id(&prop.visibility),
                span_start: prop.span_start,
                span_end: prop.span_end,
            })
            .collect(),
        props: mac
            .props
            .iter()
            .map(|prop| ResolvedPropField {
                name_id: builder.string_id(&prop.name),
                is_optional: prop.is_optional,
                type_annotation_id: builder.string_id_opt(prop.type_annotation.as_deref()),
                description_id: builder.string_id_opt(prop.description.as_deref()),
                tags: prop
                    .tags
                    .iter()
                    .map(|tag| jsdoc_tag_to_proto(builder, tag))
                    .collect(),
            })
            .collect(),
        emits: mac
            .emits
            .iter()
            .map(|emit| ResolvedEmitField {
                name_id: builder.string_id(&emit.name),
                payload_type_id: builder.string_id_opt(emit.payload_type.as_deref()),
                description_id: builder.string_id_opt(emit.description.as_deref()),
                tags: emit
                    .tags
                    .iter()
                    .map(|tag| jsdoc_tag_to_proto(builder, tag))
                    .collect(),
            })
            .collect(),
        slots: mac
            .slots
            .iter()
            .map(|slot| ResolvedSlotField {
                name_id: builder.string_id(&slot.name),
                is_required: slot.is_required,
                bindings: slot
                    .bindings
                    .iter()
                    .map(|binding| ResolvedSlotBinding {
                        name_id: builder.string_id(&binding.name),
                        type_annotation_id: builder
                            .string_id_opt(binding.type_annotation.as_deref()),
                    })
                    .collect(),
                return_type_id: builder.string_id_opt(slot.return_type.as_deref()),
                description_id: builder.string_id_opt(slot.description.as_deref()),
                tags: slot
                    .tags
                    .iter()
                    .map(|tag| jsdoc_tag_to_proto(builder, tag))
                    .collect(),
            })
            .collect(),
        jsdoc: mac
            .jsdoc
            .as_ref()
            .map(|block| resolved_jsdoc_block_to_proto(builder, block)),
    }
}

fn resolved_jsdoc_block_to_proto(
    builder: &mut GraphBuilder,
    block: &FfiResolvedJsdocBlock,
) -> ResolvedJsdocBlock {
    ResolvedJsdocBlock {
        description_id: builder.string_id_opt(block.description.as_deref()),
        tags: block
            .tags
            .iter()
            .map(|tag| ResolvedJsdocTag {
                name_id: builder.string_id(&tag.name),
                text_id: builder.string_id_opt(tag.text.as_deref()),
                raw_type_id: builder.string_id_opt(tag.raw_type.as_deref()),
                subject_name_id: builder.string_id_opt(tag.subject_name.as_deref()),
                resolved_type_node_id: tag
                    .resolved_type
                    .as_ref()
                    .map(|ty| builder.node_id(ty))
                    .unwrap_or(0),
            })
            .collect(),
    }
}

fn jsdoc_tag_to_proto(builder: &mut GraphBuilder, tag: &FfiJsdocTag) -> JsdocTag {
    JsdocTag {
        name_id: builder.string_id(&tag.name),
        text_id: builder.string_id_opt(tag.text.as_deref()),
    }
}

fn expansion_metadata_to_proto(
    builder: &mut GraphBuilder,
    metadata: &FfiExpansionMetadata,
) -> ExpansionMetadata {
    ExpansionMetadata {
        exactness: expansion_exactness_to_proto(&metadata.exactness) as i32,
        execution_status: expansion_execution_status_to_proto(&metadata.execution_status) as i32,
        diagnostics: metadata
            .diagnostics
            .iter()
            .map(|diagnostic| ExpansionDiagnostic {
                reason: expansion_reason_to_proto(&diagnostic.reason) as i32,
                context_id: builder.string_id(&diagnostic.context),
                property_name_id: builder.string_id_opt(diagnostic.property_name.as_deref()),
            })
            .collect(),
    }
}

fn resolved_type_declaration_to_proto(
    builder: &mut GraphBuilder,
    declaration: &FfiResolvedTypeDeclaration,
) -> ResolvedTypeDeclaration {
    ResolvedTypeDeclaration {
        requested_name_id: builder.string_id(&declaration.requested_name),
        resolved_name_id: builder.string_id(&declaration.resolved_name),
        canonical_source_id: builder.string_id(&declaration.canonical_source),
        span_start: declaration.span_start,
        span_end: declaration.span_end,
        kind_id: builder.string_id(&declaration.kind),
        text_id: builder.string_id_opt(declaration.text.as_deref()),
    }
}

fn graph_node_to_proto(node: &GraphNode) -> TypeNode {
    let kind = match node {
        GraphNode::Primitive { primitive } => type_node::Kind::Primitive(proto::PrimitiveNode {
            primitive: *primitive as i32,
        }),
        GraphNode::LiteralString { value } => type_node::Kind::Literal(LiteralNode {
            literal_kind: proto::LiteralKind::String as i32,
            string_id: *value,
            number_value: 0.0,
            boolean_value: false,
        }),
        GraphNode::LiteralNumber { bits } => type_node::Kind::Literal(LiteralNode {
            literal_kind: proto::LiteralKind::Number as i32,
            string_id: 0,
            number_value: f64::from_bits(*bits),
            boolean_value: false,
        }),
        GraphNode::LiteralBoolean { value } => type_node::Kind::Literal(LiteralNode {
            literal_kind: proto::LiteralKind::Boolean as i32,
            string_id: 0,
            number_value: 0.0,
            boolean_value: *value,
        }),
        GraphNode::LiteralBigInt { value } => type_node::Kind::Literal(LiteralNode {
            literal_kind: proto::LiteralKind::BigInt as i32,
            string_id: *value,
            number_value: 0.0,
            boolean_value: false,
        }),
        GraphNode::Union { types } => type_node::Kind::Union(UnionNode {
            type_node_ids: types.clone(),
        }),
        GraphNode::Intersection { types } => {
            type_node::Kind::Intersection(proto::IntersectionNode {
                type_node_ids: types.clone(),
            })
        }
        GraphNode::Array { element, readonly } => type_node::Kind::Array(ArrayNode {
            element_node_id: *element,
            readonly: *readonly,
        }),
        GraphNode::Tuple { readonly, elements } => type_node::Kind::Tuple(TupleNode {
            readonly: *readonly,
            elements: elements.iter().map(tuple_element_to_proto).collect(),
        }),
        GraphNode::Object { members } => type_node::Kind::Object(ObjectNode {
            members: members.iter().map(object_member_to_proto).collect(),
        }),
        GraphNode::Function {
            parameters,
            return_type,
            type_parameters,
        } => type_node::Kind::Function(FunctionNode {
            parameters: parameters.iter().map(function_parameter_to_proto).collect(),
            return_type_node_id: *return_type,
            type_parameter_node_ids: type_parameters.clone(),
        }),
        GraphNode::Ref {
            name,
            type_arguments,
        } => type_node::Kind::Ref(RefNode {
            name_id: *name,
            type_argument_node_ids: type_arguments.clone(),
        }),
        GraphNode::TypeParameter {
            name,
            constraint,
            default,
        } => type_node::Kind::TypeParameter(TypeParameterNode {
            name_id: *name,
            constraint_node_id: *constraint,
            default_node_id: *default,
        }),
        GraphNode::KeyOf { operand } => type_node::Kind::KeyOf(KeyOfNode {
            operand_node_id: *operand,
        }),
        GraphNode::TypeOf { path } => type_node::Kind::TypeOf(TypeOfNode {
            path_ids: path.clone(),
        }),
        GraphNode::IndexedAccess { object, index } => {
            type_node::Kind::IndexedAccess(IndexedAccessNode {
                object_node_id: *object,
                index_node_id: *index,
            })
        }
        GraphNode::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => type_node::Kind::Conditional(ConditionalNode {
            check_node_id: *check,
            extends_node_id: *extends,
            true_type_node_id: *true_type,
            false_type_node_id: *false_type,
        }),
        GraphNode::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => type_node::Kind::Mapped(MappedNode {
            parameter_id: *parameter,
            source_node_id: *source,
            value_node_id: *value,
            optional_modifier: *optional as i32,
            readonly_modifier: *readonly as i32,
            name_type_node_id: *name_type,
        }),
        GraphNode::TemplateLiteral {
            quasis,
            expressions,
        } => type_node::Kind::TemplateLiteral(TemplateLiteralNode {
            quasi_ids: quasis.clone(),
            expression_node_ids: expressions.clone(),
        }),
        GraphNode::Parenthesized { inner } => type_node::Kind::Parenthesized(ParenthesizedNode {
            inner_node_id: *inner,
        }),
        GraphNode::Unknown { raw } => type_node::Kind::Unknown(UnknownNode { raw_id: *raw }),
        GraphNode::Infer { name } => type_node::Kind::Infer(InferNode { name_id: *name }),
        GraphNode::Rest { inner } => type_node::Kind::Rest(RestNode {
            inner_node_id: *inner,
        }),
        GraphNode::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => type_node::Kind::RecursiveRef(proto::RecursiveRefNode {
            name_id: *name,
            type_argument_node_ids: type_arguments.clone(),
            conditional_context: conditional_context
                .iter()
                .map(|f| proto::ConditionalFrameNode {
                    branch: f.branch,
                    decided: f.decided,
                    check_node_id: f.check,
                    extends_node_id: f.extends,
                })
                .collect(),
        }),
    };
    TypeNode { kind: Some(kind) }
}

fn tuple_element_to_proto(element: &GraphTupleElement) -> TupleElement {
    TupleElement {
        label_id: element.label,
        type_node_id: element.ty,
        optional: element.optional,
        rest: element.rest,
    }
}

fn object_member_to_proto(member: &GraphObjectMember) -> ProtoObjectMember {
    ProtoObjectMember {
        kind: member.kind as i32,
        name_id: member.name,
        type_node_id: member.ty,
        optional: member.optional,
        readonly: member.readonly,
        key_name_id: member.key_name,
        key_type_node_id: member.key_type,
        value_type_node_id: member.value_type,
        function_node_id: member.function,
    }
}

fn function_parameter_to_proto(parameter: &GraphFunctionParam) -> FunctionParameter {
    FunctionParameter {
        name_id: parameter.name,
        type_node_id: parameter.ty,
        optional: parameter.optional,
        rest: parameter.rest,
    }
}

fn string_ids(builder: &mut GraphBuilder, values: &[String]) -> Vec<u32> {
    values
        .iter()
        .map(|value| builder.string_id(value))
        .collect()
}

fn expansion_exactness_to_proto(value: &str) -> proto::ExpansionExactness {
    match value {
        "exactConcrete" => proto::ExpansionExactness::ExactConcrete,
        "exactSymbolic" => proto::ExpansionExactness::ExactSymbolic,
        "incomplete" => proto::ExpansionExactness::Incomplete,
        other => panic!("unknown expansion exactness {other}"),
    }
}

fn expansion_execution_status_to_proto(value: &str) -> proto::ExpansionExecutionStatus {
    match value {
        "completed" => proto::ExpansionExecutionStatus::Completed,
        "cancelled" => proto::ExpansionExecutionStatus::Cancelled,
        "interrupted" => proto::ExpansionExecutionStatus::Interrupted,
        "hardStop" => proto::ExpansionExecutionStatus::HardStop,
        other => panic!("unknown expansion execution status {other}"),
    }
}

fn expansion_reason_to_proto(value: &str) -> proto::ExpansionStopReason {
    match value {
        "budgetExceeded" => proto::ExpansionStopReason::BudgetExceeded,
        "mappedDepthExceeded" => proto::ExpansionStopReason::MappedDepthExceeded,
        "unresolvedReference" => proto::ExpansionStopReason::UnresolvedReference,
        "indeterminateConditional" => proto::ExpansionStopReason::IndeterminateConditional,
        "infiniteKeySpace" => proto::ExpansionStopReason::InfiniteKeySpace,
        "unsupportedOperator" => proto::ExpansionStopReason::UnsupportedOperator,
        other => panic!("unknown expansion reason {other}"),
    }
}

fn accepted_surface_completeness_to_proto(
    value: &FfiAcceptedSurfaceCompleteness,
) -> proto::AcceptedSurfaceCompleteness {
    match value {
        FfiAcceptedSurfaceCompleteness::Exact => proto::AcceptedSurfaceCompleteness::Exact,
        FfiAcceptedSurfaceCompleteness::LowerBound => {
            proto::AcceptedSurfaceCompleteness::LowerBound
        }
    }
}

fn accepted_prop_kind_to_proto(value: &FfiAcceptedPropKind) -> proto::AcceptedPropKind {
    match value {
        FfiAcceptedPropKind::DeclaredProp => proto::AcceptedPropKind::DeclaredProp,
        FfiAcceptedPropKind::Attr => proto::AcceptedPropKind::Attr,
    }
}

fn accepted_event_kind_to_proto(value: &FfiAcceptedEventKind) -> proto::AcceptedEventKind {
    match value {
        FfiAcceptedEventKind::DeclaredEmit => proto::AcceptedEventKind::DeclaredEmit,
        FfiAcceptedEventKind::Listener => proto::AcceptedEventKind::Listener,
    }
}

fn no_fallthrough_reason_to_proto(value: &FfiNoFallthroughReason) -> proto::NoFallthroughReason {
    match value {
        FfiNoFallthroughReason::InheritAttrsFalse => proto::NoFallthroughReason::InheritAttrsFalse,
        FfiNoFallthroughReason::MultiRoot => proto::NoFallthroughReason::MultiRoot,
        FfiNoFallthroughReason::BranchNotSingleRoot => {
            proto::NoFallthroughReason::BranchNotSingleRoot
        }
        FfiNoFallthroughReason::RootVFor => proto::NoFallthroughReason::RootVFor,
        FfiNoFallthroughReason::NoTemplate => proto::NoFallthroughReason::NoTemplate,
        FfiNoFallthroughReason::EmptyTemplate => proto::NoFallthroughReason::EmptyTemplate,
        FfiNoFallthroughReason::TextOrInterpolationRoot => {
            proto::NoFallthroughReason::TextOrInterpolationRoot
        }
    }
}

fn generic_resolution_failure_to_proto(
    value: &FfiGenericResolutionFailure,
) -> proto::GenericResolutionFailure {
    match value {
        FfiGenericResolutionFailure::SpreadInput => proto::GenericResolutionFailure::SpreadInput,
        FfiGenericResolutionFailure::DynamicKey => proto::GenericResolutionFailure::DynamicKey,
        FfiGenericResolutionFailure::MissingType => proto::GenericResolutionFailure::MissingType,
        FfiGenericResolutionFailure::UnsupportedExpression => {
            proto::GenericResolutionFailure::UnsupportedExpression
        }
        FfiGenericResolutionFailure::MissingUsageLink => {
            proto::GenericResolutionFailure::MissingUsageLink
        }
        FfiGenericResolutionFailure::UnresolvedChildGenericSurface => {
            proto::GenericResolutionFailure::UnresolvedChildGenericSurface
        }
    }
}

fn build_test_meta() -> FfiComponentMeta {
    let tree_ref = TypeExpr::Ref {
        name: Arc::from("TreeNode"),
        type_arguments: empty_type_args(),
    };
    let tree_node = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            TypeObjectMember::Property(ObjectProperty {
                name: "label".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            }),
            TypeObjectMember::Property(ObjectProperty {
                name: "next".to_string(),
                ty: TypeExpr::union(vec![
                    tree_ref.clone(),
                    TypeExpr::Primitive(PrimitiveName::Undefined),
                ]),
                optional: true,
                readonly: false,
            }),
        ],
    }));

    FfiComponentMeta {
        props: vec![FfiPropMeta {
            name: "root".to_string(),
            r#type: tree_ref.clone(),
            type_expansion: None,
            raw_type: Some("TreeNode".to_string()),
            required: true,
            has_default: false,
            default_value: None,
            description: None,
            tags: Vec::new(),
        }],
        events: Vec::new(),
        slots: vec![FfiSlotMeta {
            name: "default".to_string(),
            is_scoped: true,
            bindings: vec![FfiSlotBindingMeta {
                name: "root".to_string(),
                r#type: tree_ref.clone(),
                type_expansion: None,
                raw_type: Some("TreeNode".to_string()),
            }],
            is_required: false,
            return_type: Some("VNode[]".to_string()),
            description: None,
            tags: Vec::new(),
        }],
        models: Vec::new(),
        exposed: Vec::new(),
        public_instance: None,
        sfc_blocks: None,
        type_registry: vec![FfiResolvedTypeMeta {
            name: "TreeNode".to_string(),
            r#type: tree_node,
            type_expansion: None,
            raw_type: Some("TreeNode".to_string()),
            declaration: None,
        }],
        components: Vec::new(),
        template_refs: Vec::new(),
        imports: Vec::new(),
        bindings: Vec::new(),
        vue_api_calls: Vec::new(),
        styles: Vec::new(),
        flags: FfiComponentMetaFlags {
            async_setup: false,
            has_reactive_state: false,
            has_computed: false,
            has_watchers: false,
            has_lifecycle_hooks: false,
            has_provide: false,
            has_inject: false,
            has_inherit_attrs_false: false,
            has_store_usage: false,
        },
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness: FfiAcceptedSurfaceCompleteness::Exact,
        root_info: FfiRootInfo {
            kind: FfiRootInfoKind::None,
            reason: Some(FfiNoFallthroughReason::NoTemplate),
            targets: Vec::new(),
        },
        root_reachability: FfiRootReachability::NoFallthrough {
            reason: FfiNoFallthroughReason::NoTemplate,
        },
        fallthrough_surface: FfiFallthroughSurface::None {
            reason: FfiNoFallthroughReason::NoTemplate,
        },
        options_api: false,
        file_path: "/src/Tree.vue".to_string(),
        resolution: None,
    }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::{build_test_payload, ComponentMetaPayload};
    use crate::graph::GraphBuilder;
    use crate::types::FfiComponentMetaFlags;
    use verter_semantic::analysis::type_expr::TypeExpr;

    #[test]
    fn component_meta_payload_roundtrips_recursive_graph() {
        let payload = build_test_payload();

        let bytes = payload.encode_to_vec();
        let decoded =
            ComponentMetaPayload::decode(bytes.as_slice()).expect("proto payload should decode");

        let file_path_id = decoded
            .body
            .as_ref()
            .map(|body| body.file_path_id)
            .expect("body should exist");
        let graph = decoded.type_graph.as_ref().expect("graph should exist");

        assert_eq!(
            graph.strings[(file_path_id - 1) as usize].as_str(),
            "/src/Tree.vue",
            "file path string should survive round-trip"
        );
        assert_eq!(
            decoded.type_registry.len(),
            1,
            "registry entry should survive round-trip"
        );
        assert_eq!(
            graph.strings[(decoded
                .body
                .as_ref()
                .and_then(|body| body.props.first())
                .map(|prop| prop.name_id)
                .expect("prop should exist")
                - 1) as usize]
                .as_str(),
            "root",
            "prop metadata should survive round-trip"
        );
        assert_ne!(
            decoded
                .type_graph
                .as_ref()
                .map(|graph| graph.nodes.len())
                .unwrap_or_default(),
            0,
            "graph nodes should be present"
        );
    }

    #[test]
    fn protocol_owns_component_meta_schema_modules() {
        let mut builder = GraphBuilder::new();
        let node_id = builder.node_id(&TypeExpr::named("TreeNode"));
        let flags = FfiComponentMetaFlags {
            async_setup: false,
            has_reactive_state: false,
            has_computed: false,
            has_watchers: false,
            has_lifecycle_hooks: false,
            has_provide: false,
            has_inject: false,
            has_inherit_attrs_false: false,
            has_store_usage: false,
        };

        assert_eq!(
            node_id, 1,
            "protocol graph builder should remain locally usable"
        );
        assert!(
            !flags.async_setup,
            "protocol should compile the component-meta flag DTOs directly"
        );
    }
}
