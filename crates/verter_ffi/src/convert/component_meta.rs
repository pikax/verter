//! Session output-envelope → FFI projection. The single public entry point
//! is [`component_meta_output_to_ffi`]: it consumes the session-owned,
//! fully-materialized [`ComponentMetaOutput`] envelope by value and
//! mechanically maps it onto the wire DTO ([`FfiComponentMeta`]).
//!
//! CONTEXT-FREE MAPPER: this module performs no dispatch, no lowering, no
//! source lookup, no reparse, and no second resolution. Every wire type
//! position reads the envelope's materialized POSITIONAL lane (order-aligned
//! 1:1 with its analysis vector — duplicate names, nested slot bindings,
//! registry rows, and every fallthrough branch are positional). The resolved
//! type-registry name-overlay finalize is SESSION-OWNED and already applied
//! to `analysis.type_registry` before the envelope was sealed.

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

/// Convert the session-owned output envelope to the FFI boundary DTO.
///
/// The envelope is consumed BY VALUE through its single destructive
/// transfer accessor; the materialized lanes are zipped positionally with
/// their analysis vectors. This is the sole production wire conversion for
/// component-meta — there is no raw-analysis converter.
pub fn component_meta_output_to_ffi(
    output: verter_session::meta_resolve::ComponentMetaOutput,
) -> FfiComponentMeta {
    let (analysis, resolution, types) = output.into_parts();
    component_meta_parts_to_ffi(analysis, resolution, types.into_lanes())
}

/// HARD wire-boundary alignment guard: refuse the conversion loudly when a
/// materialized lane's length does not match its analysis vector. The lanes
/// are positional 1:1 by construction; a mismatch means the envelope is torn,
/// and the positional `zip`s below would SILENTLY TRUNCATE the wire payload
/// (dropping trailing members or pairing values onto the wrong rows). Active
/// in EVERY build profile — a debug-only assert would let a release build
/// ship the truncated payload. Shared with the fallthrough lane conversion
/// (`super::fallthrough`), which zips the same class of positional lanes.
#[track_caller]
pub(super) fn require_lane_aligned(lane: &str, analysis_len: usize, lane_len: usize) {
    assert_eq!(
        analysis_len, lane_len,
        "component-meta FFI conversion refused: the `{lane}` lane carries {lane_len} \
         materialized value(s) for {analysis_len} analysis row(s) — wire lanes are \
         positional 1:1 and a zip would silently truncate",
    );
}

/// Parts-level mechanical mapping — the body of
/// [`component_meta_output_to_ffi`] after the envelope's destructive
/// transfer. Module-private: production code converts ONLY the sealed
/// envelope; the parts seam exists so converter unit tests can exercise the
/// mapping with hand-built parts without a live host.
pub(super) fn component_meta_parts_to_ffi(
    analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    resolution: Option<verter_session::meta_resolve::ComponentMetaResolutionOutput>,
    lanes: verter_session::meta_resolve::MaterializedComponentMetaTypeLanes,
) -> FfiComponentMeta {
    let root_info = root_info_to_ffi(&analysis.root_reachability);

    require_lane_aligned("props", analysis.props.len(), lanes.props.len());
    require_lane_aligned(
        "event-payloads",
        analysis.events.len(),
        lanes.event_payloads.len(),
    );
    require_lane_aligned(
        "slot-bindings",
        analysis.slots.len(),
        lanes.slot_bindings.len(),
    );
    for (index, (slot, lane)) in analysis
        .slots
        .iter()
        .zip(lanes.slot_bindings.iter())
        .enumerate()
    {
        assert_eq!(
            slot.bindings.len(),
            lane.len(),
            "component-meta FFI conversion refused: slot #{index} (`{name}`) carries \
             {lane_len} materialized binding value(s) for {analysis_len} analysis \
             binding(s) — inner slot-binding lanes are positional 1:1 and a zip \
             would silently truncate",
            name = slot.name,
            lane_len = lane.len(),
            analysis_len = slot.bindings.len(),
        );
    }
    require_lane_aligned("models", analysis.models.len(), lanes.models.len());
    require_lane_aligned("exposed", analysis.exposed.len(), lanes.exposed.len());
    require_lane_aligned(
        "public-instance-members",
        analysis
            .public_instance
            .as_ref()
            .map(|p| p.members.len())
            .unwrap_or(0),
        lanes.public_instance_members.len(),
    );
    require_lane_aligned(
        "type-registry-entries",
        analysis.type_registry.len(),
        lanes.type_registry_entries.len(),
    );
    require_lane_aligned(
        "accepted-props",
        analysis.accepted_props.len(),
        lanes.accepted_props.len(),
    );
    require_lane_aligned(
        "accepted-event-payloads",
        analysis.accepted_events.len(),
        lanes.accepted_event_payloads.len(),
    );
    FfiComponentMeta {
        // Typed resolution status: honest on every lane — a payload
        // without the resolution sidecar self-describes as
        // `Unavailable(ResolutionProviderAbsent)`.
        resolution_status: if resolution.is_some() {
            FfiComponentMetaResolutionStatus::Resolved
        } else {
            FfiComponentMetaResolutionStatus::Unavailable(
                FfiResolutionUnavailableReason::ResolutionProviderAbsent,
            )
        },
        props: analysis
            .props
            .into_iter()
            .zip(lanes.props)
            .map(|(p, r#type)| FfiPropMeta {
                name: p.name,
                r#type,
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
            .zip(lanes.event_payloads)
            .map(|(e, payload)| FfiEventMeta {
                name: e.name,
                payload,
                payload_expansion: e.payload_expansion.map(expansion_metadata_to_ffi),
                raw_signature: e.raw_signature,
                description: e.description,
                tags: e.tags.into_iter().map(jsdoc_to_ffi).collect(),
            })
            .collect(),
        slots: analysis
            .slots
            .into_iter()
            .zip(lanes.slot_bindings)
            .map(|(s, binding_types)| FfiSlotMeta {
                name: s.name,
                is_scoped: s.is_scoped,
                bindings: s
                    .bindings
                    .into_iter()
                    .zip(binding_types)
                    .map(|(b, r#type)| FfiSlotBindingMeta {
                        name: b.name,
                        r#type,
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
            .zip(lanes.models)
            .map(|(m, r#type)| FfiModelMeta {
                name: m.name,
                r#type,
            })
            .collect(),
        exposed: analysis
            .exposed
            .into_iter()
            .zip(lanes.exposed)
            .map(|(e, r#type)| FfiExposedMeta {
                name: e.name,
                r#type,
                type_expansion: e.type_expansion.map(expansion_metadata_to_ffi),
                description: e.description,
                tags: e.tags.into_iter().map(jsdoc_to_ffi).collect(),
            })
            .collect(),
        public_instance: analysis
            .public_instance
            .map(|public_instance| FfiPublicInstanceMeta {
                completeness: public_instance_completeness_to_string(public_instance.completeness),
                members: public_instance
                    .members
                    .into_iter()
                    .zip(lanes.public_instance_members)
                    .map(|(member, r#type)| FfiPublicInstanceMemberMeta {
                        name: member.name,
                        kind: public_instance_member_kind_to_string(member.kind),
                        r#type,
                        type_expansion: member.type_expansion.map(expansion_metadata_to_ffi),
                        raw_type: member.raw_type,
                        description: member.description,
                        tags: member.tags.into_iter().map(jsdoc_to_ffi).collect(),
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
        // `analysis.type_registry` already carries the SESSION-owned
        // resolved-registry name-overlay finalize (applied before
        // materialization when the envelope carries a resolution); the lane
        // aligns with the merged registry. The per-name declaration sidecar
        // reads the narrowed resolution output.
        type_registry: analysis
            .type_registry
            .into_iter()
            .zip(lanes.type_registry_entries)
            .map(|(entry, r#type)| {
                let declaration = resolution
                    .as_ref()
                    .and_then(|resolution| {
                        resolution
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
                    r#type,
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
                bindings: component
                    .bindings
                    .into_iter()
                    .map(|binding| FfiComponentBindingUsage {
                        name: binding.name,
                        modifiers: binding.modifiers,
                    })
                    .collect(),
                events: component
                    .events
                    .into_iter()
                    .map(|event| FfiComponentEventUsage {
                        name: event.name,
                        handler_expression: event.handler_expression,
                        is_inline: event.is_inline,
                        modifiers: event.modifiers,
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
            .zip(lanes.accepted_props)
            .map(|(prop, r#type)| accepted_prop_to_ffi(prop, r#type))
            .collect(),
        accepted_events: analysis
            .accepted_events
            .into_iter()
            .zip(lanes.accepted_event_payloads)
            .map(|(event, payload)| accepted_event_to_ffi(event, payload))
            .collect(),
        accepted_surface_completeness: accepted_surface_completeness_to_ffi(
            analysis.accepted_surface_completeness,
        ),
        root_info,
        root_reachability: root_reachability_to_ffi(analysis.root_reachability),
        fallthrough_surface: fallthrough_surface_to_ffi(
            analysis.fallthrough_surface,
            lanes.fallthrough_props,
            lanes.fallthrough_event_payloads,
        ),
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
        origin: resolution
            .as_ref()
            .and_then(|resolution| resolution.origin_graph.clone())
            .unwrap_or_default(),
        resolution: resolution.map(|resolution| resolved_component_meta_to_ffi(&resolution)),
    }
}

pub(super) fn resolved_component_meta_to_ffi(
    resolution: &verter_session::meta_resolve::ComponentMetaResolutionOutput,
) -> FfiComponentMetaResolution {
    FfiComponentMetaResolution {
        mode: projection_mode_to_string(resolution.mode),
        macros: resolution
            .resolved_macros
            .iter()
            .map(resolved_macro_to_ffi)
            .collect(),
    }
}

pub(super) fn resolved_macro_to_ffi(
    resolved: &verter_session::meta_resolve::ResolvedMacroMeta,
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
        jsdoc: resolved.jsdoc.as_ref().map(|jsdoc| FfiResolvedJsdocBlock {
            description: jsdoc.description.clone(),
            tags: jsdoc.tags.iter().map(resolved_jsdoc_tag_to_ffi).collect(),
        }),
    }
}
