//! Component-meta analysis → FFI projection. Includes the public entry points
//! `component_meta_analysis_to_ffi[_with_resolution]` and
//! `component_meta_resolution_to_ffi`, plus the internal resolved-meta /
//! resolved-macro projections.

use verter_session as host;

use crate::types::*;

use super::fallthrough::{fallthrough_surface_to_ffi, root_info_to_ffi, root_reachability_to_ffi};
use super::sfc_blocks::{
    custom_block_to_ffi, script_block_to_ffi, style_block_to_ffi, template_block_to_ffi,
};
use super::string_helpers::{
    accepted_event_to_ffi, accepted_prop_to_ffi, accepted_surface_completeness_to_ffi,
    binding_kind_to_string, component_prop_constness_to_string, expansion_exactness_to_string,
    expansion_execution_status_to_string, expansion_metadata_to_ffi,
    expansion_stop_reason_to_string, jsdoc_to_ffi, macro_expansion_kind_to_string,
    macro_kind_to_string, member_visibility_to_string, projection_mode_to_string,
    public_instance_completeness_to_string, public_instance_member_kind_to_string,
    reactivity_kind_to_string, resolved_declaration_kind_to_string, resolved_jsdoc_tag_to_ffi,
    style_lang_to_string, vue_api_to_string,
};

pub fn component_meta_analysis_to_ffi(
    analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> FfiComponentMeta {
    component_meta_analysis_to_ffi_with_resolution(analysis, None)
}

/// Convert component-meta analysis plus optional native resolved-state sidecar
/// to the FFI boundary DTO.
pub fn component_meta_analysis_to_ffi_with_resolution(
    analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    resolved_state: Option<&host::meta_resolve::ResolvedComponentMetaState>,
) -> FfiComponentMeta {
    let root_info = root_info_to_ffi(&analysis.root_reachability);
    let mut merged_type_registry = analysis.type_registry;
    if let Some(state) = resolved_state {
        for resolved_entry in &state.resolved_type_registry {
            if let Some(existing) = merged_type_registry
                .iter_mut()
                .find(|entry| entry.name == resolved_entry.name)
            {
                *existing = resolved_entry.clone();
            } else {
                merged_type_registry.push(resolved_entry.clone());
            }
        }
    }
    FfiComponentMeta {
        props: analysis
            .props
            .into_iter()
            .map(|p| FfiPropMeta {
                name: p.name,
                r#type: p.type_expr,
                type_expansion: p.type_expansion.map(expansion_metadata_to_ffi),
                raw_type: p.raw_type,
                required: p.required,
                has_default: p.has_default,
                default_value: p.default_value,
                description: p.description,
                tags: p.tags.into_iter().map(jsdoc_to_ffi).collect(),
                declared_in_macro_type_arg: p.declared_in_macro_type_arg,
            })
            .collect(),
        events: analysis
            .events
            .into_iter()
            .map(|e| FfiEventMeta {
                name: e.name,
                payload: e.payload,
                payload_expansion: e.payload_expansion.map(expansion_metadata_to_ffi),
                raw_signature: e.raw_signature,
                description: e.description,
                tags: e.tags.into_iter().map(jsdoc_to_ffi).collect(),
            })
            .collect(),
        slots: analysis
            .slots
            .into_iter()
            .map(|s| FfiSlotMeta {
                name: s.name,
                is_scoped: s.is_scoped,
                bindings: s
                    .bindings
                    .into_iter()
                    .map(|b| FfiSlotBindingMeta {
                        name: b.name,
                        r#type: b.type_expr,
                        type_expansion: b.type_expansion.map(expansion_metadata_to_ffi),
                        raw_type: b.raw_type,
                    })
                    .collect(),
                is_required: s.is_required,
                return_type: s.return_type,
                description: s.description,
                tags: s.tags.into_iter().map(jsdoc_to_ffi).collect(),
            })
            .collect(),
        models: analysis
            .models
            .into_iter()
            .map(|m| FfiModelMeta {
                name: m.name,
                r#type: m.type_expr,
            })
            .collect(),
        exposed: analysis
            .exposed
            .into_iter()
            .map(|e| FfiExposedMeta {
                name: e.name,
                r#type: e.type_expr,
                type_expansion: e.type_expansion.map(expansion_metadata_to_ffi),
                description: e.description,
            })
            .collect(),
        public_instance: analysis
            .public_instance
            .map(|public_instance| FfiPublicInstanceMeta {
                completeness: public_instance_completeness_to_string(public_instance.completeness),
                members: public_instance
                    .members
                    .into_iter()
                    .map(|member| FfiPublicInstanceMemberMeta {
                        name: member.name,
                        kind: public_instance_member_kind_to_string(member.kind),
                        r#type: member.type_expr,
                        type_expansion: member.type_expansion.map(expansion_metadata_to_ffi),
                        raw_type: member.raw_type,
                        description: member.description,
                    })
                    .collect(),
            }),
        sfc_blocks: analysis.sfc_blocks.map(|blocks| FfiSfcBlocksMeta {
            template: blocks.template.map(template_block_to_ffi),
            script: blocks.script.map(script_block_to_ffi),
            script_setup: blocks.script_setup.map(script_block_to_ffi),
            styles: blocks.styles.into_iter().map(style_block_to_ffi).collect(),
            custom: blocks.custom.into_iter().map(custom_block_to_ffi).collect(),
        }),
        type_registry: merged_type_registry
            .into_iter()
            .map(|entry| {
                let declaration = resolved_state
                    .and_then(|state| {
                        state
                            .resolved_type_registry_meta
                            .iter()
                            .find(|meta| meta.name == entry.name)
                    })
                    .map(|meta| FfiResolvedTypeDeclaration {
                        requested_name: meta.declaration.requested_name.clone(),
                        resolved_name: meta.declaration.resolved_name.clone(),
                        canonical_source: meta.declaration.canonical_source.clone(),
                        span_start: meta.declaration.span.start,
                        span_end: meta.declaration.span.end,
                        kind: resolved_declaration_kind_to_string(meta.declaration.kind),
                        text: meta.declaration.text.clone(),
                    });

                FfiResolvedTypeMeta {
                    name: entry.name,
                    r#type: entry.type_expr,
                    type_expansion: entry.type_expansion.map(expansion_metadata_to_ffi),
                    raw_type: declaration
                        .as_ref()
                        .and_then(|declaration| declaration.text.clone()),
                    declaration,
                }
            })
            .collect(),
        components: analysis
            .components
            .into_iter()
            .map(|component| FfiComponentUsage {
                name: component.name,
                import_source: component.import_source,
                is_dynamic: component.is_dynamic,
                props: component
                    .props
                    .into_iter()
                    .map(|prop| FfiComponentPropUsage {
                        name: prop.name,
                        is_bound: prop.is_bound,
                        constness: component_prop_constness_to_string(prop.constness),
                        expression: prop.expression,
                        referenced_bindings: prop.referenced_bindings,
                        from_spread: prop.from_spread,
                        is_shorthand: prop.is_shorthand,
                    })
                    .collect(),
                has_spread: component.has_spread,
                slots_used: component.slots_used,
                static_classes: component.static_classes,
                has_dynamic_class: component.has_dynamic_class,
                v_models: component.v_models,
                v_model_entries: component
                    .v_model_entries
                    .into_iter()
                    .map(|entry| FfiComponentVModelEntry {
                        binding_name: entry.binding_name,
                    })
                    .collect(),
            })
            .collect(),
        template_refs: analysis
            .template_refs
            .into_iter()
            .map(|template_ref| FfiTemplateRefMeta {
                name: template_ref.name,
                is_dynamic: template_ref.is_dynamic,
                target_tag: template_ref.target_tag,
            })
            .collect(),
        imports: analysis
            .imports
            .into_iter()
            .map(|import| FfiImportMeta {
                source: import.source,
                is_type_only: import.is_type_only,
                bindings: import
                    .bindings
                    .into_iter()
                    .map(|binding| FfiImportBindingMeta {
                        name: binding.name,
                        kind: match binding.kind {
                            verter_semantic::analysis::types::ImportBindingKind::Named => {
                                "named".to_string()
                            }
                            verter_semantic::analysis::types::ImportBindingKind::Default => {
                                "default".to_string()
                            }
                            verter_semantic::analysis::types::ImportBindingKind::Namespace => {
                                "namespace".to_string()
                            }
                        },
                        imported_name: binding.imported_name,
                        is_type_only: binding.is_type_only,
                    })
                    .collect(),
            })
            .collect(),
        bindings: analysis
            .bindings
            .into_iter()
            .map(|binding| FfiBindingMeta {
                name: binding.name,
                kind: binding_kind_to_string(binding.kind),
                reactivity_kind: reactivity_kind_to_string(binding.reactivity_kind),
                type_annotation: binding.type_annotation,
                used_in_template: binding.used_in_template,
                used_in_style: binding.used_in_style,
            })
            .collect(),
        vue_api_calls: analysis
            .vue_api_calls
            .into_iter()
            .map(|call| FfiVueApiCallMeta {
                api: vue_api_to_string(call.api),
                arg_value: call.arg_value,
            })
            .collect(),
        styles: analysis
            .styles
            .into_iter()
            .map(|style| FfiStyleMeta {
                lang: style_lang_to_string(style.lang),
                scoped: style.scoped,
                is_module: style.is_module,
                module_name: style.module_name,
                classes: style.classes,
                ids: style.ids,
                custom_properties: style.custom_properties,
                v_binds: style.v_binds,
                selectors: style
                    .selectors
                    .into_iter()
                    .map(|selector| FfiSelectorMeta {
                        text: selector.text,
                        specificity: selector.specificity,
                    })
                    .collect(),
            })
            .collect(),
        flags: FfiComponentMetaFlags {
            async_setup: analysis.flags.async_setup,
            has_reactive_state: analysis.flags.has_reactive_state,
            has_computed: analysis.flags.has_computed,
            has_watchers: analysis.flags.has_watchers,
            has_lifecycle_hooks: analysis.flags.has_lifecycle_hooks,
            has_provide: analysis.flags.has_provide,
            has_inject: analysis.flags.has_inject,
            has_inherit_attrs_false: analysis.flags.has_inherit_attrs_false,
            has_store_usage: analysis.flags.has_store_usage,
            has_macro_failure: analysis.flags.has_macro_failure,
        },
        accepted_props: analysis
            .accepted_props
            .into_iter()
            .map(accepted_prop_to_ffi)
            .collect(),
        accepted_events: analysis
            .accepted_events
            .into_iter()
            .map(accepted_event_to_ffi)
            .collect(),
        accepted_surface_completeness: accepted_surface_completeness_to_ffi(
            analysis.accepted_surface_completeness,
        ),
        root_info,
        root_reachability: root_reachability_to_ffi(analysis.root_reachability),
        fallthrough_surface: fallthrough_surface_to_ffi(analysis.fallthrough_surface),
        macro_expansion_diagnostics: analysis
            .macro_expansion_diagnostics
            .into_iter()
            .map(|entry| FfiMacroExpansionDiagnostics {
                macro_kind: macro_expansion_kind_to_string(entry.macro_kind),
                macro_index: entry.macro_index as u32,
                exactness: expansion_exactness_to_string(entry.exactness),
                execution_status: expansion_execution_status_to_string(entry.execution_status),
                diagnostics: entry
                    .diagnostics
                    .into_iter()
                    .map(|d| FfiExpansionDiagnostic {
                        reason: expansion_stop_reason_to_string(d.reason),
                        context: d.context,
                        property_name: d.property_name,
                    })
                    .collect(),
            })
            .collect(),
        options_api: analysis.options_api,
        file_path: analysis.file_path,
        resolution: resolved_state.map(resolved_component_meta_to_ffi),
        origin: resolved_state
            .and_then(|s| s.origin_graph.as_ref())
            .cloned()
            .unwrap_or_default(),
    }
}
pub(super) fn resolved_component_meta_to_ffi(
    state: &host::meta_resolve::ResolvedComponentMetaState,
) -> FfiComponentMetaResolution {
    FfiComponentMetaResolution {
        mode: projection_mode_to_string(state.mode),
        macros: state
            .resolved_macros
            .iter()
            .map(resolved_macro_to_ffi)
            .collect(),
    }
}

/// Public wrapper exposing the resolved-state → FFI projection. Used
/// by the NAPI/WASM audit bindings to package the resolution alongside
/// the audit record as JSON.
pub fn component_meta_resolution_to_ffi(
    state: &host::meta_resolve::ResolvedComponentMetaState,
) -> FfiComponentMetaResolution {
    resolved_component_meta_to_ffi(state)
}

pub(super) fn resolved_macro_to_ffi(
    resolved: &host::meta_resolve::ResolvedMacroMeta,
) -> FfiResolvedMacroMeta {
    FfiResolvedMacroMeta {
        macro_index: resolved.macro_index as u32,
        macro_kind: macro_kind_to_string(resolved.macro_kind),
        type_name: resolved.type_name.clone(),
        import_source: resolved.import_source.clone(),
        declaration: FfiResolvedTypeDeclaration {
            requested_name: resolved.declaration.requested_name.clone(),
            resolved_name: resolved.declaration.resolved_name.clone(),
            canonical_source: resolved.declaration.canonical_source.clone(),
            span_start: resolved.declaration.span.start,
            span_end: resolved.declaration.span.end,
            kind: resolved_declaration_kind_to_string(resolved.declaration.kind),
            text: resolved.declaration.text.clone(),
        },
        native_props: resolved
            .native_props
            .iter()
            .map(|prop| FfiResolvedNativeProp {
                name: prop.name.clone(),
                is_optional: prop.is_optional,
                type_annotation: prop.type_annotation.clone(),
                visibility: member_visibility_to_string(prop.visibility),
                span_start: prop.span.start,
                span_end: prop.span.end,
            })
            .collect(),
        props: resolved
            .props
            .iter()
            .map(|prop| FfiResolvedPropField {
                name: prop.name.clone(),
                is_optional: prop.is_optional,
                type_annotation: prop.type_annotation.clone(),
                description: prop.description.clone(),
                tags: prop.tags.iter().cloned().map(jsdoc_to_ffi).collect(),
            })
            .collect(),
        emits: resolved
            .emits
            .iter()
            .map(|emit| FfiResolvedEmitField {
                name: emit.name.clone(),
                payload_type: emit.payload_type.clone(),
                description: emit.description.clone(),
                tags: emit.tags.iter().cloned().map(jsdoc_to_ffi).collect(),
            })
            .collect(),
        slots: resolved
            .slots
            .iter()
            .map(|slot| FfiResolvedSlotField {
                name: slot.name.clone(),
                is_required: slot.is_required,
                bindings: slot
                    .bindings
                    .iter()
                    .map(|binding| FfiResolvedSlotBinding {
                        name: binding.name.clone(),
                        type_annotation: binding.type_annotation.clone(),
                    })
                    .collect(),
                return_type: slot.return_type.clone(),
                description: slot.description.clone(),
                tags: slot.tags.iter().cloned().map(jsdoc_to_ffi).collect(),
            })
            .collect(),
        jsdoc: resolved.jsdoc.as_ref().map(|jsdoc| FfiResolvedJsdocBlock {
            description: jsdoc.description.clone(),
            tags: jsdoc.tags.iter().map(resolved_jsdoc_tag_to_ffi).collect(),
        }),
    }
}
