//! Framework-agnostic conversion functions between FFI types and host types.
//!
//! Error-returning functions use `Result<T, FfiConversionError>`. Each consumer
//! crate converts the error to its native type (`napi::Error` or `JsValue`)
//! via the `Display` impl.

use std::sync::Arc;

use verter_session as host;

use crate::types::*;

// ─── Component-meta analysis → FFI ─────────────────────────────────────────

/// Convert an analysis-domain `ComponentMetaAnalysis` to the FFI boundary DTO.
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

fn macro_expansion_kind_to_string(
    kind: verter_semantic::analysis::component_meta::MacroExpansionKind,
) -> String {
    match kind {
        verter_semantic::analysis::component_meta::MacroExpansionKind::DefineProps => {
            "defineProps".to_string()
        }
        verter_semantic::analysis::component_meta::MacroExpansionKind::DefineEmits => {
            "defineEmits".to_string()
        }
        verter_semantic::analysis::component_meta::MacroExpansionKind::DefineSlots => {
            "defineSlots".to_string()
        }
    }
}

fn jsdoc_to_ffi(tag: verter_semantic::analysis::types::JsdocTag) -> FfiJsdocTag {
    FfiJsdocTag {
        name: tag.name,
        text: tag.text,
    }
}

fn expansion_metadata_to_ffi(
    metadata: verter_semantic::analysis::type_expand::ExpansionMetadata,
) -> FfiExpansionMetadata {
    FfiExpansionMetadata {
        exactness: expansion_exactness_to_string(metadata.exactness),
        execution_status: expansion_execution_status_to_string(metadata.execution_status),
        diagnostics: metadata
            .diagnostics
            .into_iter()
            .map(|diagnostic| FfiExpansionDiagnostic {
                reason: expansion_stop_reason_to_string(diagnostic.reason),
                context: diagnostic.context,
                property_name: diagnostic.property_name,
            })
            .collect(),
    }
}

fn expansion_exactness_to_string(
    exactness: verter_semantic::analysis::type_expand::ExpansionExactness,
) -> String {
    match exactness {
        verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete => {
            "exactConcrete".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic => {
            "exactSymbolic".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExactness::Incomplete => {
            "incomplete".to_string()
        }
    }
}

fn expansion_execution_status_to_string(
    status: verter_semantic::analysis::type_expand::ExpansionExecutionStatus,
) -> String {
    match status {
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed => {
            "completed".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Cancelled => {
            "cancelled".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Interrupted => {
            "interrupted".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::HardStop => {
            "hardStop".to_string()
        }
    }
}

fn expansion_stop_reason_to_string(
    reason: verter_semantic::analysis::type_expand::ExpansionStopReason,
) -> String {
    match reason {
        verter_semantic::analysis::type_expand::ExpansionStopReason::BudgetExceeded => {
            "budgetExceeded".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::MappedDepthExceeded => {
            "mappedDepthExceeded".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::UnresolvedReference => {
            "unresolvedReference".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::IndeterminateConditional => {
            "indeterminateConditional".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::InfiniteKeySpace => {
            "infiniteKeySpace".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::UnsupportedOperator => {
            "unsupportedOperator".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::ConditionalContextTruncated => {
            "conditionalContextTruncated".to_string()
        }
    }
}

// ─── Fallthrough surface conversions ────────────────────────────────────────

fn accepted_prop_to_ffi(
    prop: verter_semantic::analysis::component_meta::AcceptedPropAnalysis,
) -> FfiAcceptedPropMeta {
    FfiAcceptedPropMeta {
        name: prop.name,
        r#type: prop.type_expr,
        raw_type: prop.raw_type,
        required: prop.required,
        provenance: member_provenance_to_ffi(prop.provenance),
        availability: member_availability_to_ffi(prop.availability),
        kind: accepted_prop_kind_to_ffi(prop.kind),
    }
}

fn accepted_event_to_ffi(
    event: verter_semantic::analysis::component_meta::AcceptedEventAnalysis,
) -> FfiAcceptedEventMeta {
    FfiAcceptedEventMeta {
        name: event.name,
        payload: event.payload,
        raw_signature: event.raw_signature,
        provenance: member_provenance_to_ffi(event.provenance),
        availability: member_availability_to_ffi(event.availability),
        kind: accepted_event_kind_to_ffi(event.kind),
    }
}

fn member_provenance_to_ffi(
    provenance: verter_semantic::analysis::component_meta::MemberProvenance,
) -> FfiMemberProvenance {
    match provenance {
        verter_semantic::analysis::component_meta::MemberProvenance::Declared => {
            FfiMemberProvenance::Declared
        }
        verter_semantic::analysis::component_meta::MemberProvenance::Inherited { sources } => {
            FfiMemberProvenance::Inherited {
                sources: sources.into_iter().map(inherited_source_to_ffi).collect(),
            }
        }
    }
}

fn inherited_source_to_ffi(
    source: verter_semantic::analysis::component_meta::InheritedSource,
) -> FfiInheritedSource {
    match source {
        verter_semantic::analysis::component_meta::InheritedSource::NativeTag { tag } => {
            FfiInheritedSource::NativeTag { tag }
        }
        verter_semantic::analysis::component_meta::InheritedSource::Component { canonical_id } => {
            FfiInheritedSource::Component { canonical_id }
        }
    }
}

fn member_availability_to_ffi(
    availability: verter_semantic::analysis::component_meta::MemberAvailability,
) -> FfiMemberAvailability {
    match availability {
        verter_semantic::analysis::component_meta::MemberAvailability::Always => {
            FfiMemberAvailability::Always
        }
        verter_semantic::analysis::component_meta::MemberAvailability::Conditional {
            branch_keys,
        } => FfiMemberAvailability::Conditional { branch_keys },
    }
}

fn accepted_prop_kind_to_ffi(
    kind: verter_semantic::analysis::component_meta::AcceptedPropKind,
) -> FfiAcceptedPropKind {
    match kind {
        verter_semantic::analysis::component_meta::AcceptedPropKind::DeclaredProp => {
            FfiAcceptedPropKind::DeclaredProp
        }
        verter_semantic::analysis::component_meta::AcceptedPropKind::Attr => {
            FfiAcceptedPropKind::Attr
        }
    }
}

fn accepted_event_kind_to_ffi(
    kind: verter_semantic::analysis::component_meta::AcceptedEventKind,
) -> FfiAcceptedEventKind {
    match kind {
        verter_semantic::analysis::component_meta::AcceptedEventKind::DeclaredEmit => {
            FfiAcceptedEventKind::DeclaredEmit
        }
        verter_semantic::analysis::component_meta::AcceptedEventKind::Listener => {
            FfiAcceptedEventKind::Listener
        }
    }
}

fn accepted_surface_completeness_to_ffi(
    completeness: verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness,
) -> FfiAcceptedSurfaceCompleteness {
    match completeness {
        verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact => {
            FfiAcceptedSurfaceCompleteness::Exact
        }
        verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound => {
            FfiAcceptedSurfaceCompleteness::LowerBound
        }
    }
}

fn root_reachability_to_ffi(
    reachability: verter_semantic::analysis::component_meta::RootReachability,
) -> FfiRootReachability {
    match reachability {
        verter_semantic::analysis::component_meta::RootReachability::NoFallthrough { reason } => {
            FfiRootReachability::NoFallthrough {
                reason: no_fallthrough_reason_to_ffi(reason),
            }
        }
        verter_semantic::analysis::component_meta::RootReachability::Branches { branches } => {
            FfiRootReachability::Branches {
                branches: branches.into_iter().map(root_branch_to_ffi).collect(),
            }
        }
    }
}

fn root_info_to_ffi(
    reachability: &verter_semantic::analysis::component_meta::RootReachability,
) -> FfiRootInfo {
    match reachability {
        verter_semantic::analysis::component_meta::RootReachability::NoFallthrough { reason } => {
            let kind = match reason {
                verter_semantic::analysis::component_meta::NoFallthroughReason::MultiRoot
                | verter_semantic::analysis::component_meta::NoFallthroughReason::RootVFor => {
                    FfiRootInfoKind::Multiple
                }
                verter_semantic::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot => {
                    FfiRootInfoKind::Conditional
                }
                verter_semantic::analysis::component_meta::NoFallthroughReason::InheritAttrsFalse
                | verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate
                | verter_semantic::analysis::component_meta::NoFallthroughReason::EmptyTemplate
                | verter_semantic::analysis::component_meta::NoFallthroughReason::TextOrInterpolationRoot => {
                    FfiRootInfoKind::None
                }
            };
            FfiRootInfo {
                kind,
                reason: Some(no_fallthrough_reason_to_ffi(reason.clone())),
                targets: Vec::new(),
            }
        }
        verter_semantic::analysis::component_meta::RootReachability::Branches { branches } => {
            FfiRootInfo {
                kind: if branches.len() <= 1 {
                    FfiRootInfoKind::Single
                } else {
                    FfiRootInfoKind::Conditional
                },
                reason: None,
                targets: branches
                    .iter()
                    .map(|branch| root_target_ref_to_ffi(branch.target.clone()))
                    .collect(),
            }
        }
    }
}

fn no_fallthrough_reason_to_ffi(
    reason: verter_semantic::analysis::component_meta::NoFallthroughReason,
) -> FfiNoFallthroughReason {
    match reason {
        verter_semantic::analysis::component_meta::NoFallthroughReason::InheritAttrsFalse => {
            FfiNoFallthroughReason::InheritAttrsFalse
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::MultiRoot => {
            FfiNoFallthroughReason::MultiRoot
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot => {
            FfiNoFallthroughReason::BranchNotSingleRoot
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::RootVFor => {
            FfiNoFallthroughReason::RootVFor
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate => {
            FfiNoFallthroughReason::NoTemplate
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::EmptyTemplate => {
            FfiNoFallthroughReason::EmptyTemplate
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::TextOrInterpolationRoot => {
            FfiNoFallthroughReason::TextOrInterpolationRoot
        }
    }
}

fn root_branch_to_ffi(
    branch: verter_semantic::analysis::component_meta::RootBranch,
) -> FfiRootBranch {
    FfiRootBranch {
        branch_index: branch.branch_index,
        condition_text: branch.condition_text,
        target: root_target_ref_to_ffi(branch.target),
        consumed: FfiConsumedRootBindings {
            attrs: branch.consumed.attrs,
            listeners: branch.consumed.listeners,
            has_dynamic_attr_name: branch.consumed.has_dynamic_attr_name,
            has_dynamic_listener_name: branch.consumed.has_dynamic_listener_name,
        },
        has_unknown_spread: branch.has_unknown_spread,
    }
}

fn root_target_ref_to_ffi(
    target: verter_semantic::analysis::component_meta::RootTargetRef,
) -> FfiRootTargetRef {
    match target {
        verter_semantic::analysis::component_meta::RootTargetRef::NativeElement {
            element_index,
            tag,
        } => FfiRootTargetRef::NativeElement { element_index, tag },
        verter_semantic::analysis::component_meta::RootTargetRef::DynamicComponentUsage {
            element_index,
            usage_index,
        } => FfiRootTargetRef::DynamicComponentUsage {
            element_index,
            usage_index,
        },
        verter_semantic::analysis::component_meta::RootTargetRef::ComponentUsage {
            element_index,
            usage_index,
            name,
            import_source,
        } => FfiRootTargetRef::ComponentUsage {
            element_index,
            usage_index,
            name,
            import_source,
        },
        verter_semantic::analysis::component_meta::RootTargetRef::UnresolvedTarget {
            element_index,
            tag,
            reason,
        } => FfiRootTargetRef::UnresolvedTarget {
            element_index,
            tag,
            reason: unresolved_root_target_reason_to_ffi(reason),
        },
    }
}

fn unresolved_root_target_reason_to_ffi(
    reason: verter_semantic::analysis::component_meta::UnresolvedRootTargetReason,
) -> FfiUnresolvedRootTargetReason {
    match reason {
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::DynamicComponentIs => {
            FfiUnresolvedRootTargetReason::DynamicComponentIs
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::SlotOutlet => {
            FfiUnresolvedRootTargetReason::SlotOutlet
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::UnsupportedBuiltin { tag } => {
            FfiUnresolvedRootTargetReason::UnsupportedBuiltin { tag }
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::MissingUsageLink => {
            FfiUnresolvedRootTargetReason::MissingUsageLink
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::UnresolvedImport => {
            FfiUnresolvedRootTargetReason::UnresolvedImport
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::UnknownRootTarget => {
            FfiUnresolvedRootTargetReason::UnknownRootTarget
        }
    }
}

fn fallthrough_surface_to_ffi(
    surface: verter_semantic::analysis::component_meta::FallthroughSurface,
) -> FfiFallthroughSurface {
    match surface {
        verter_semantic::analysis::component_meta::FallthroughSurface::None { reason } => {
            FfiFallthroughSurface::None {
                reason: no_fallthrough_reason_to_ffi(reason),
            }
        }
        verter_semantic::analysis::component_meta::FallthroughSurface::Branches { branches } => {
            FfiFallthroughSurface::Branches {
                branches: branches
                    .into_iter()
                    .map(fallthrough_branch_to_ffi)
                    .collect(),
            }
        }
    }
}

fn fallthrough_branch_to_ffi(
    branch: verter_semantic::analysis::component_meta::FallthroughBranch,
) -> FfiFallthroughBranch {
    FfiFallthroughBranch {
        branch_key: branch.branch_key,
        condition_text: branch.condition_text,
        props: branch
            .props
            .into_iter()
            .map(|p| FfiFallthroughPropEntry {
                name: p.name,
                r#type: p.type_expr,
                raw_type: p.raw_type,
                sources: p.sources.into_iter().map(inherited_source_to_ffi).collect(),
            })
            .collect(),
        events: branch
            .events
            .into_iter()
            .map(|e| FfiFallthroughEventEntry {
                name: e.name,
                payload: e.payload,
                raw_signature: e.raw_signature,
                sources: e.sources.into_iter().map(inherited_source_to_ffi).collect(),
            })
            .collect(),
        root_chain: branch
            .root_chain
            .into_iter()
            .map(resolved_root_step_to_ffi)
            .collect(),
        status: branch_status_to_ffi(branch.status),
    }
}

fn resolved_root_step_to_ffi(
    step: verter_semantic::analysis::component_meta::ResolvedRootStep,
) -> FfiResolvedRootStep {
    match step {
        verter_semantic::analysis::component_meta::ResolvedRootStep::NativeTag { tag } => {
            FfiResolvedRootStep::NativeTag { tag }
        }
        verter_semantic::analysis::component_meta::ResolvedRootStep::Component {
            canonical_id,
            component_name,
        } => FfiResolvedRootStep::Component {
            canonical_id,
            component_name,
        },
        verter_semantic::analysis::component_meta::ResolvedRootStep::Unresolved { tag, reason } => {
            FfiResolvedRootStep::Unresolved {
                tag,
                reason: unresolved_branch_reason_to_ffi(reason),
            }
        }
    }
}

fn branch_status_to_ffi(
    status: verter_semantic::analysis::component_meta::BranchStatus,
) -> FfiBranchStatus {
    match status {
        verter_semantic::analysis::component_meta::BranchStatus::Resolved => {
            FfiBranchStatus::Resolved
        }
        verter_semantic::analysis::component_meta::BranchStatus::PartiallyUnresolved {
            reasons,
        } => FfiBranchStatus::PartiallyUnresolved {
            reasons: reasons
                .into_iter()
                .map(partial_branch_reason_to_ffi)
                .collect(),
        },
        verter_semantic::analysis::component_meta::BranchStatus::Unresolved { reason } => {
            FfiBranchStatus::Unresolved {
                reason: unresolved_branch_reason_to_ffi(reason),
            }
        }
    }
}

fn generic_resolution_failure_to_ffi(
    failure: verter_semantic::analysis::component_meta::GenericResolutionFailure,
) -> FfiGenericResolutionFailure {
    match failure {
        verter_semantic::analysis::component_meta::GenericResolutionFailure::SpreadInput => {
            FfiGenericResolutionFailure::SpreadInput
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::DynamicKey => {
            FfiGenericResolutionFailure::DynamicKey
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::MissingType => {
            FfiGenericResolutionFailure::MissingType
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::UnsupportedExpression => {
            FfiGenericResolutionFailure::UnsupportedExpression
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::MissingUsageLink => {
            FfiGenericResolutionFailure::MissingUsageLink
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::UnresolvedChildGenericSurface => {
            FfiGenericResolutionFailure::UnresolvedChildGenericSurface
        }
    }
}

fn partial_branch_reason_to_ffi(
    reason: verter_semantic::analysis::component_meta::PartialBranchReason,
) -> FfiPartialBranchReason {
    match reason {
        verter_semantic::analysis::component_meta::PartialBranchReason::DynamicAttrName => {
            FfiPartialBranchReason::DynamicAttrName
        }
        verter_semantic::analysis::component_meta::PartialBranchReason::DynamicListenerName => {
            FfiPartialBranchReason::DynamicListenerName
        }
        verter_semantic::analysis::component_meta::PartialBranchReason::UnknownSpread => {
            FfiPartialBranchReason::UnknownSpread
        }
        verter_semantic::analysis::component_meta::PartialBranchReason::GenericResolution {
            failure,
        } => FfiPartialBranchReason::GenericResolution {
            failure: generic_resolution_failure_to_ffi(failure),
        },
    }
}

fn unresolved_branch_reason_to_ffi(
    reason: verter_semantic::analysis::component_meta::UnresolvedBranchReason,
) -> FfiUnresolvedBranchReason {
    match reason {
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::Cycle { canonical_id } => {
            FfiUnresolvedBranchReason::Cycle { canonical_id }
        }
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::DynamicComponentIs => {
            FfiUnresolvedBranchReason::DynamicComponentIs
        }
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::ChildResolutionFailed => {
            FfiUnresolvedBranchReason::ChildResolutionFailed
        }
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::UnresolvedChildImport {
            import_source,
        } => FfiUnresolvedBranchReason::UnresolvedChildImport { import_source },
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::RootTarget { reason } => {
            FfiUnresolvedBranchReason::RootTarget {
                reason: unresolved_root_target_reason_to_ffi(reason),
            }
        }
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::GenericResolution { failure } => {
            FfiUnresolvedBranchReason::GenericResolution {
                failure: generic_resolution_failure_to_ffi(failure),
            }
        }
    }
}

fn resolved_component_meta_to_ffi(
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

fn resolved_macro_to_ffi(resolved: &host::meta_resolve::ResolvedMacroMeta) -> FfiResolvedMacroMeta {
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

fn resolved_jsdoc_tag_to_ffi(tag: &host::meta_resolve::ResolvedJsdocTag) -> FfiResolvedJsdocTag {
    FfiResolvedJsdocTag {
        name: tag.name.clone(),
        text: tag.text.clone(),
        raw_type: tag.raw_type.clone(),
        subject_name: tag.subject_name.clone(),
        resolved_type: tag.resolved_type.clone(),
    }
}

fn component_prop_constness_to_string(
    constness: verter_semantic::analysis::template::PropValueConstness,
) -> String {
    match constness {
        verter_semantic::analysis::template::PropValueConstness::Const => "const".to_string(),
        verter_semantic::analysis::template::PropValueConstness::Dynamic => "dynamic".to_string(),
        verter_semantic::analysis::template::PropValueConstness::Unknown => "unknown".to_string(),
    }
}

fn binding_kind_to_string(
    kind: verter_semantic::analysis::component_meta::BindingKindAnalysis,
) -> String {
    match kind {
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Const => {
            "const".to_string()
        }
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Let => "let".to_string(),
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Var => "var".to_string(),
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Function => {
            "function".to_string()
        }
        verter_semantic::analysis::component_meta::BindingKindAnalysis::AsyncFunction => {
            "asyncFunction".to_string()
        }
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Class => {
            "class".to_string()
        }
    }
}

fn reactivity_kind_to_string(kind: verter_semantic::analysis::types::ReactivityKind) -> String {
    match kind {
        verter_semantic::analysis::types::ReactivityKind::None => "none".to_string(),
        verter_semantic::analysis::types::ReactivityKind::Ref => "ref".to_string(),
        verter_semantic::analysis::types::ReactivityKind::Computed => "computed".to_string(),
        verter_semantic::analysis::types::ReactivityKind::Reactive => "reactive".to_string(),
        verter_semantic::analysis::types::ReactivityKind::MaybeRef => "maybeRef".to_string(),
        verter_semantic::analysis::types::ReactivityKind::Mutable => "mutable".to_string(),
    }
}

fn vue_api_to_string(api: verter_semantic::analysis::types::VueApiClassification) -> String {
    format!("{api:?}")
}

fn style_lang_to_string(lang: verter_semantic::analysis::style::StyleAnalysisLang) -> String {
    format!("{lang:?}")
}

fn projection_mode_to_string(mode: host::ProjectionMode) -> String {
    match mode {
        host::ProjectionMode::Identity => "identity".to_string(),
        host::ProjectionMode::Navigate => "navigate".to_string(),
        host::ProjectionMode::Shallow => "shallow".to_string(),
        host::ProjectionMode::Expanded => "expanded".to_string(),
        host::ProjectionMode::Skeleton => "skeleton".to_string(),
    }
}

fn macro_kind_to_string(kind: verter_semantic::analysis::AnalyzedMacroKind) -> String {
    match kind {
        verter_semantic::analysis::AnalyzedMacroKind::DefineProps => "defineProps".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::WithDefaults => "withDefaults".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => "defineEmits".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => "defineSlots".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineModel => "defineModel".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineExpose => "defineExpose".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => "defineOptions".to_string(),
    }
}

fn resolved_declaration_kind_to_string(
    kind: host::meta_resolve::ResolvedDeclarationKind,
) -> String {
    match kind {
        host::meta_resolve::ResolvedDeclarationKind::Interface => "interface".to_string(),
        host::meta_resolve::ResolvedDeclarationKind::TypeAlias => "typeAlias".to_string(),
        host::meta_resolve::ResolvedDeclarationKind::Class => "class".to_string(),
        host::meta_resolve::ResolvedDeclarationKind::Unknown => "unknown".to_string(),
    }
}

fn public_instance_completeness_to_string(
    completeness: verter_semantic::analysis::component_meta::PublicInstanceCompleteness,
) -> String {
    match completeness {
        verter_semantic::analysis::component_meta::PublicInstanceCompleteness::Exact => {
            "exact".to_string()
        }
        verter_semantic::analysis::component_meta::PublicInstanceCompleteness::Partial => {
            "partial".to_string()
        }
    }
}

fn public_instance_member_kind_to_string(
    kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind,
) -> String {
    match kind {
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Prop => {
            "prop".to_string()
        }
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::SlotContainer => {
            "slotContainer".to_string()
        }
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed => {
            "exposed".to_string()
        }
    }
}

fn sfc_attribute_to_ffi(
    attribute: verter_semantic::analysis::component_meta::SfcAttributeAnalysis,
) -> FfiSfcAttributeMeta {
    FfiSfcAttributeMeta {
        name: attribute.name,
        value: attribute.value,
    }
}

fn template_block_to_ffi(
    block: verter_semantic::analysis::component_meta::TemplateBlockAnalysis,
) -> FfiTemplateBlockMeta {
    FfiTemplateBlockMeta {
        lang: block.lang,
        src: block.src,
        attributes: block
            .attributes
            .into_iter()
            .map(sfc_attribute_to_ffi)
            .collect(),
    }
}

fn script_block_to_ffi(
    block: verter_semantic::analysis::component_meta::ScriptBlockAnalysis,
) -> FfiScriptBlockMeta {
    FfiScriptBlockMeta {
        lang: block.lang,
        src: block.src,
        generic: block.generic,
        attrs_type: block.attrs_type,
        attributes: block
            .attributes
            .into_iter()
            .map(sfc_attribute_to_ffi)
            .collect(),
    }
}

fn style_block_to_ffi(
    block: verter_semantic::analysis::component_meta::StyleBlockInfoAnalysis,
) -> FfiStyleBlockMeta {
    FfiStyleBlockMeta {
        index: block.index as u32,
        lang: block.lang,
        src: block.src,
        scoped: block.scoped,
        is_module: block.is_module,
        module_name: block.module_name,
        attributes: block
            .attributes
            .into_iter()
            .map(sfc_attribute_to_ffi)
            .collect(),
    }
}

fn custom_block_to_ffi(
    block: verter_semantic::analysis::component_meta::CustomBlockAnalysis,
) -> FfiCustomBlockMeta {
    FfiCustomBlockMeta {
        index: block.index as u32,
        block_type: block.block_type,
        lang: block.lang,
        src: block.src,
        attributes: block
            .attributes
            .into_iter()
            .map(sfc_attribute_to_ffi)
            .collect(),
    }
}

fn member_visibility_to_string(visibility: host::ResolvedMemberVisibility) -> String {
    match visibility {
        host::ResolvedMemberVisibility::Public => "public".to_string(),
        host::ResolvedMemberVisibility::Protected => "protected".to_string(),
        host::ResolvedMemberVisibility::Private => "private".to_string(),
    }
}

/// Typed error for FFI → host conversion failures.
#[derive(Debug, Clone)]
pub enum FfiConversionError {
    /// Invalid `compileErrorPolicy` string.
    InvalidCompileErrorPolicy(String),
    /// Invalid `analysisLevel` string.
    InvalidAnalysisLevel(String),
    /// Invalid `hmrStrategy` string.
    InvalidHmrStrategy(String),
    /// `delimiters` array must have exactly 2 elements.
    InvalidDelimiters(usize),
    /// Invalid `file_kind` string.
    InvalidFileKind(String),
    /// Invalid virtual node `kind` string.
    InvalidNodeKind(String),
    /// Invalid `target` string.
    InvalidTarget(String),
}

impl std::fmt::Display for FfiConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCompileErrorPolicy(v) => {
                write!(
                    f,
                    "invalid compileErrorPolicy '{v}' (expected 'strict' or 'dev')"
                )
            }
            Self::InvalidAnalysisLevel(v) => {
                write!(
                    f,
                    "invalid analysisLevel '{v}' (expected 'none', 'essential', or 'full')"
                )
            }
            Self::InvalidHmrStrategy(v) => {
                write!(
                    f,
                    "invalid hmrStrategy '{v}' (expected 'vite', 'webpack', or 'none')"
                )
            }
            Self::InvalidDelimiters(len) => {
                write!(f, "delimiters must have exactly 2 elements, got {len}")
            }
            Self::InvalidFileKind(v) => write!(f, "invalid file_kind '{v}'"),
            Self::InvalidNodeKind(v) => write!(f, "invalid virtual node kind '{v}'"),
            Self::InvalidTarget(v) => write!(
                f,
                "invalid target '{v}' (expected 'bundler', 'ide', 'analysis', or 'full')"
            ),
        }
    }
}

impl std::error::Error for FfiConversionError {}

impl From<FfiConversionError> for String {
    fn from(e: FfiConversionError) -> String {
        e.to_string()
    }
}

// =============================================================================
// Input: FFI → Host
// =============================================================================

/// Convert FFI host config to internal host config.
pub fn ffi_config_to_host(input: FfiHostConfig) -> Result<host::HostConfig, FfiConversionError> {
    let mut out = host::HostConfig::default();
    if let Some(dev_mode) = input.dev_mode {
        out.dev_mode = dev_mode;
    }
    if let Some(policy) = input.compile_error_policy {
        out.compile_error_policy = if policy.eq_ignore_ascii_case("strict")
            || policy.eq_ignore_ascii_case("strict_error")
            || policy.eq_ignore_ascii_case("strictError")
        {
            host::CompileErrorPolicy::StrictError
        } else if policy.eq_ignore_ascii_case("dev")
            || policy.eq_ignore_ascii_case("dev_serve_last_known_good")
            || policy.eq_ignore_ascii_case("devServeLastKnownGood")
        {
            host::CompileErrorPolicy::DevServeLastKnownGood
        } else {
            return Err(FfiConversionError::InvalidCompileErrorPolicy(policy));
        };
    }
    if let Some(lsp_scheme) = input.lsp_scheme {
        out.lsp_scheme = lsp_scheme;
    }
    if let Some(max_profiles) = input.max_profiles_per_file {
        out.max_profiles_per_file = max_profiles as usize;
    }
    if let Some(extensions) = input.resolve_extensions {
        out.resolve_extensions = extensions;
    }
    if let Some(level) = input.analysis_level {
        out.analysis_level = if level.eq_ignore_ascii_case("none") {
            host::AnalysisLevel::None
        } else if level.eq_ignore_ascii_case("essential") {
            host::AnalysisLevel::Essential
        } else if level.eq_ignore_ascii_case("full") {
            host::AnalysisLevel::Full
        } else {
            return Err(FfiConversionError::InvalidAnalysisLevel(level));
        };
    }
    if let Some(audit) = input.audit_enabled {
        out.audit_enabled = audit;
    }
    if let Some(footprint) = input.footprint_capture {
        out.footprint_capture = footprint;
    }
    Ok(out)
}

/// Convert FFI compile profile to internal compile profile.
pub fn ffi_profile_to_host(
    input: Option<FfiCompileProfile>,
) -> Result<host::CompileProfile, FfiConversionError> {
    let mut out = host::CompileProfile::default();
    if let Some(input) = input {
        out.filename = input.filename;
        if let Some(is_production) = input.is_production {
            out.is_production = is_production;
        }
        if let Some(ssr) = input.ssr {
            out.ssr = ssr;
        }
        if let Some(hmr_strategy) = input.hmr_strategy {
            out.hmr_strategy = if hmr_strategy.eq_ignore_ascii_case("vite") {
                host::HmrStrategy::Vite
            } else if hmr_strategy.eq_ignore_ascii_case("webpack") {
                host::HmrStrategy::Webpack
            } else if hmr_strategy.eq_ignore_ascii_case("none") {
                host::HmrStrategy::None
            } else {
                return Err(FfiConversionError::InvalidHmrStrategy(hmr_strategy));
            };
        }
        out.component_id = input.component_id;
        out.delimiters = if let Some(d) = input.delimiters {
            if d.len() != 2 {
                return Err(FfiConversionError::InvalidDelimiters(d.len()));
            }
            Some((d[0].clone(), d[1].clone()))
        } else {
            None
        };
        out.custom_elements = input.custom_elements;
        out.comments = input.comments;
        if let Some(runtime_module_name) = input.runtime_module_name {
            out.runtime_module_name = Some(runtime_module_name);
        }
        if let Some(types_module_name) = input.types_module_name {
            out.types_module_name = Some(types_module_name);
        }
        if let Some(force_vapor) = input.force_vapor {
            out.force_vapor = force_vapor;
        }
        if let Some(force_js) = input.force_js {
            out.force_js = force_js;
        }
        if let Some(source_map) = input.source_map {
            out.source_map = source_map;
        }
        if let Some(target) = input.target {
            out.target = ffi_target_to_compile_target(&target)?;
        }
        if let Some(strict_slots) = input.strict_slots {
            out.strict_slots = strict_slots;
        }
    }
    Ok(out)
}

/// Convert a target string to `CompileTarget` bitflags.
fn ffi_target_to_compile_target(target: &str) -> Result<host::CompileTarget, FfiConversionError> {
    use host::CompileTarget;
    match target.to_ascii_lowercase().as_str() {
        "bundler" => Ok(CompileTarget::BUNDLER),
        "ide" => Ok(CompileTarget::IDE),
        "analysis" => Ok(CompileTarget::ANALYSIS),
        "full" => Ok(CompileTarget::BUNDLER | CompileTarget::TSX | CompileTarget::TEMPLATE_DATA),
        other => Err(FfiConversionError::InvalidTarget(other.to_string())),
    }
}

/// Parse a file kind string to the host enum.
pub fn ffi_file_kind_to_host(input: Option<&str>) -> Result<host::FileKind, FfiConversionError> {
    match input.unwrap_or("vue").to_ascii_lowercase().as_str() {
        "vue" | "sfc" | "vue_sfc" => Ok(host::FileKind::VueSfc),
        "non_sfc" | "text" | "file" => Ok(host::FileKind::NonSfc),
        other => Err(FfiConversionError::InvalidFileKind(other.to_string())),
    }
}

/// Parse a virtual node kind from its FFI representation.
pub fn ffi_node_kind_to_host(
    input: FfiVirtualNodeKind,
) -> Result<host::VirtualNodeKind, FfiConversionError> {
    match input.kind.to_ascii_lowercase().as_str() {
        "main" => Ok(host::VirtualNodeKind::Main),
        "script" => Ok(host::VirtualNodeKind::Script),
        "template" => Ok(host::VirtualNodeKind::Template),
        "style" => Ok(host::VirtualNodeKind::Style {
            index: input.index.unwrap_or(0) as usize,
        }),
        "custom" => Ok(host::VirtualNodeKind::Custom {
            index: input.index.unwrap_or(0) as usize,
        }),
        other => Err(FfiConversionError::InvalidNodeKind(other.to_string())),
    }
}

/// Convert FFI upsert request to host upsert request.
pub fn ffi_upsert_to_host(
    input: FfiUpsertRequest,
) -> Result<host::UpsertRequest, FfiConversionError> {
    Ok(host::UpsertRequest {
        canonical_id: input.canonical_id,
        input_id: input.input_id,
        source: Arc::from(input.source),
        file_kind: ffi_file_kind_to_host(input.file_kind.as_deref())?,
        aliases: input.aliases.unwrap_or_default(),
    })
}

/// Parse a block type string to the host `PreprocessorBlockType` enum.
fn ffi_block_type_to_host(s: &str) -> host::PreprocessorBlockType {
    match s {
        "template" => host::PreprocessorBlockType::Template,
        "script" => host::PreprocessorBlockType::Script,
        "style" => host::PreprocessorBlockType::Style,
        _ => host::PreprocessorBlockType::Custom,
    }
}

/// Convert a host `PreprocessorBlockType` to its string representation.
fn host_block_type_to_string(bt: host::PreprocessorBlockType) -> String {
    match bt {
        host::PreprocessorBlockType::Template => "template".to_string(),
        host::PreprocessorBlockType::Script => "script".to_string(),
        host::PreprocessorBlockType::Style => "style".to_string(),
        host::PreprocessorBlockType::Custom => "custom".to_string(),
    }
}

/// Convert FFI block override request to host block override request.
pub fn ffi_block_override_to_host(
    input: FfiBlockOverrideRequest,
) -> Result<host::BlockOverrideRequest, FfiConversionError> {
    Ok(host::BlockOverrideRequest {
        canonical_id: input.canonical_id,
        compile_profile: ffi_profile_to_host(input.compile_profile)?,
        overrides: input
            .overrides
            .into_iter()
            .map(|entry| host::BlockOverrideEntry {
                block_type: ffi_block_type_to_host(&entry.block_type),
                index: entry.index as usize,
                code: Arc::from(entry.code),
                source_map: entry.source_map.map(Arc::from),
            })
            .collect(),
    })
}

/// Convert host preprocessor request to FFI representation.
pub fn host_preprocessor_request_to_ffi(req: &host::PreprocessorRequest) -> FfiPreprocessorRequest {
    FfiPreprocessorRequest {
        block_type: host_block_type_to_string(req.block_type),
        index: req.index as u32,
        lang: req.lang.clone(),
        content: req.content.clone(),
    }
}

fn host_module_reference_syntax_to_string(syntax: impl std::fmt::Debug) -> String {
    match format!("{syntax:?}").as_str() {
        "StaticImport" => "staticImport".to_string(),
        "ExportFrom" => "exportFrom".to_string(),
        "DynamicImport" => "dynamicImport".to_string(),
        "RequireCall" => "requireCall".to_string(),
        other => other.to_string(),
    }
}

fn host_module_reference_semantics_to_string(semantics: impl std::fmt::Debug) -> String {
    match format!("{semantics:?}").as_str() {
        "Import" => "import".to_string(),
        "Require" => "require".to_string(),
        other => other.to_string(),
    }
}

fn host_module_reference_analyzability_to_string(analyzability: impl std::fmt::Debug) -> String {
    match format!("{analyzability:?}").as_str() {
        "Exact" => "exact".to_string(),
        "FiniteSet" => "finiteSet".to_string(),
        "UnknownDynamic" => "unknownDynamic".to_string(),
        other => other.to_string(),
    }
}

fn host_module_reference_to_ffi(input: host::ScriptModuleReference) -> FfiModuleReference {
    FfiModuleReference {
        syntax: host_module_reference_syntax_to_string(input.syntax),
        semantics: host_module_reference_semantics_to_string(input.semantics),
        is_type_only: input.is_type_only,
        raw_text: input.raw_text,
        literal_specifier: input.literal_specifier,
        finite_specifiers: input.finite_specifiers,
        static_prefix: input.static_prefix,
        analyzability: host_module_reference_analyzability_to_string(input.analyzability),
        span_start: input.span.start,
        span_end: input.span.end,
        expr_span_start: input.expr_span.start,
        expr_span_end: input.expr_span.end,
    }
}

/// Convert FFI virtual query to host virtual query.
pub fn ffi_virtual_query_to_host(
    input: FfiVirtualQuery,
) -> Result<host::VirtualQuery, FfiConversionError> {
    let node_kind = input.node_kind.map(ffi_node_kind_to_host).transpose()?;
    Ok(host::VirtualQuery {
        raw_id: input.raw_id,
        canonical_id: input.canonical_id,
        node_kind,
        compile_profile: ffi_profile_to_host(input.compile_profile)?,
    })
}

// =============================================================================
// Output: Host → FFI
// =============================================================================

/// Convert a host virtual node kind to its FFI representation.
pub fn host_node_kind_to_ffi(input: &host::VirtualNodeKind) -> FfiVirtualNodeKind {
    match input {
        host::VirtualNodeKind::Main => FfiVirtualNodeKind {
            kind: "main".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Script => FfiVirtualNodeKind {
            kind: "script".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Template => FfiVirtualNodeKind {
            kind: "template".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Style { index } => FfiVirtualNodeKind {
            kind: "style".to_string(),
            index: Some(*index as u32),
        },
        host::VirtualNodeKind::Custom { index } => FfiVirtualNodeKind {
            kind: "custom".to_string(),
            index: Some(*index as u32),
        },
    }
}

/// Convert host diagnostics to FFI representation.
fn clamp_to_char_boundary(source: &str, byte_offset: usize) -> usize {
    let mut clamped = byte_offset.min(source.len());
    while clamped > 0 && !source.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

pub fn byte_offset_to_utf16(source: &str, byte_offset: u32) -> u32 {
    let clamped = clamp_to_char_boundary(source, byte_offset as usize);
    source[..clamped].encode_utf16().count() as u32
}

pub fn utf16_to_byte_offset(source: &str, utf16_offset: u32) -> u32 {
    let mut utf16_count = 0u32;
    for (byte_idx, ch) in source.char_indices() {
        let next = utf16_count + ch.len_utf16() as u32;
        if utf16_offset <= utf16_count || utf16_offset < next {
            return byte_idx as u32;
        }
        utf16_count = next;
    }
    source.len() as u32
}

fn maybe_utf16_offset(raw: Option<u32>, source: Option<&str>) -> Option<u32> {
    raw.map(|offset| {
        source
            .map(|s| byte_offset_to_utf16(s, offset))
            .unwrap_or(offset)
    })
}

/// Convert linter diagnostics from UTF-8 byte spans to UTF-16 spans for JS interop.
pub fn lint_diagnostics_to_utf16(
    mut diagnostics: Vec<verter_diagnostics::LintDiagnostic>,
    source: Option<&str>,
) -> Vec<verter_diagnostics::LintDiagnostic> {
    let Some(source) = source else {
        return diagnostics;
    };

    for d in &mut diagnostics {
        let start = byte_offset_to_utf16(source, d.span.start);
        let end = byte_offset_to_utf16(source, d.span.end);
        d.span = verter_span::Span::new(start, end);
    }

    diagnostics
}

pub fn host_diagnostics_to_ffi(
    input: &host::DiagnosticsSnapshot,
    source: Option<&str>,
) -> FfiDiagnosticsSnapshot {
    FfiDiagnosticsSnapshot {
        diagnostics: input
            .diagnostics
            .iter()
            .map(|d| FfiDiagnostic {
                severity: match d.severity {
                    host::HostSeverity::Error => "error".to_string(),
                    host::HostSeverity::Warning => "warning".to_string(),
                    host::HostSeverity::Info => "info".to_string(),
                },
                code: d.code.clone(),
                message: d.message.clone(),
                span_start: d
                    .span
                    .and_then(|s| maybe_utf16_offset(Some(s.start), source)),
                span_end: d.span.and_then(|s| maybe_utf16_offset(Some(s.end), source)),
            })
            .collect(),
        has_errors: input.has_errors,
    }
}

/// Convert a host update result to its FFI representation.
pub fn host_update_to_ffi(input: host::HostUpdateResult, source: Option<&str>) -> FfiUpdateResult {
    FfiUpdateResult {
        canonical_id: input.canonical_id,
        changed: input.changed,
        slice_changes: FfiSliceChanges {
            script_changed: input.slice_changes.script_changed,
            template_changed: input.slice_changes.template_changed,
            style_indices_changed: input
                .slice_changes
                .style_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            custom_indices_changed: input
                .slice_changes
                .custom_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            structure_changed: input.slice_changes.structure_changed,
            descriptor_changed: input.slice_changes.descriptor_changed,
        },
        changed_virtual_nodes: input
            .changed_virtual_nodes
            .iter()
            .map(host_node_kind_to_ffi)
            .collect(),
        removed_virtual_nodes: input
            .removed_virtual_nodes
            .iter()
            .map(host_node_kind_to_ffi)
            .collect(),
        changed_virtual_ids: input.changed_virtual_ids,
        removed_virtual_ids: input.removed_virtual_ids,
        changed_lsp_ids: input.changed_lsp_ids,
        removed_lsp_ids: input.removed_lsp_ids,
        diagnostics: host_diagnostics_to_ffi(&input.diagnostics, source),
        external_source_requests: input
            .external_source_requests
            .into_iter()
            .map(|req| FfiExternalSourceRequest {
                owner_canonical_id: req.owner_canonical_id,
                block_kind: match req.block_kind {
                    host::ExternalBlockKind::Script => "script".to_string(),
                    host::ExternalBlockKind::Template => "template".to_string(),
                    host::ExternalBlockKind::Style => "style".to_string(),
                    host::ExternalBlockKind::Custom => "custom".to_string(),
                },
                index: req.index as u32,
                specifier: req.specifier,
                resolved_canonical_id: req.resolved_canonical_id,
            })
            .collect(),
        import_specifiers: input
            .import_specifiers
            .into_iter()
            .map(|imp| FfiScriptImportInfo {
                source: imp.source,
                is_type_only: imp.is_type_only,
                bindings: imp.bindings,
            })
            .collect(),
        module_references: input
            .module_references
            .into_iter()
            .map(host_module_reference_to_ffi)
            .collect(),
        preprocessor_requests: input
            .preprocessor_requests
            .iter()
            .map(host_preprocessor_request_to_ffi)
            .collect(),
        export_signatures: input
            .export_signatures
            .into_iter()
            .map(|sig| FfiExportSignature {
                name: sig.name,
                is_type: sig.is_type,
                reexport_source: sig.reexport_source,
                reexport_local: sig.reexport_local,
            })
            .collect(),
        parse_duration_ms: input.parse_duration_ms,
    }
}

/// Convert a host virtual file response to its FFI representation.
pub fn host_virtual_file_to_ffi(
    input: host::VirtualFileResponse,
    source: Option<&str>,
) -> FfiVirtualFileResponse {
    FfiVirtualFileResponse {
        id: input.id,
        code: input.code.to_string(),
        source_map: input.source_map.as_ref().map(|s| s.to_string()),
        lang: input.lang,
        stale: input.stale,
        diagnostics: host_diagnostics_to_ffi(&input.diagnostics, source),
        meta: FfiVirtualMeta {
            scope_id: input.meta.scope_id,
            block_type: input.meta.block_type,
            style_index: input.meta.style_index.map(|i| i as u32),
            custom_index: input.meta.custom_index.map(|i| i as u32),
        },
    }
}

/// Convert a host resolved ID to its FFI representation.
pub fn host_resolved_id_to_ffi(input: host::ResolvedId) -> FfiResolvedId {
    FfiResolvedId {
        canonical_id: input.canonical_id,
        node_kind: host_node_kind_to_ffi(&input.node_kind),
        exists_in_host: input.exists_in_host,
        bundler_id: input.bundler_id,
        lsp_id: input.lsp_id,
    }
}

/// Convert a host remove result to its FFI representation.
pub fn host_remove_to_ffi(input: host::HostRemoveResult) -> FfiRemoveResult {
    FfiRemoveResult {
        canonical_id: input.canonical_id,
    }
}

/// Convert a `CrossFileResult` from the host to its FFI representation.
pub fn host_cross_file_result_to_ffi(
    input: host::cross_file::CrossFileResult,
) -> FfiCrossFileResult {
    FfiCrossFileResult {
        const_prop_overrides: input
            .const_prop_overrides
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect(),
        changed_files: input.changed_files,
        diagnostics: input
            .diagnostics
            .into_iter()
            .map(|d| FfiCrossFileDiagnostic {
                file_id: d.file_id,
                code: d.code,
                message: d.message,
            })
            .collect(),
    }
}

/// Convert a host error to a human-readable string.
///
/// Each consumer crate wraps this string in its native error type
/// (`napi::Error` or `JsValue`).
pub fn host_error_to_string(err: &host::HostError) -> String {
    match err {
        host::HostError::MissingSource { canonical_id } => {
            format!("HostError::MissingSource: {}", canonical_id)
        }
        host::HostError::InvalidQuery => "HostError::InvalidQuery".to_string(),
        host::HostError::MissingVirtualNode { canonical_id } => {
            format!("HostError::MissingVirtualNode: {}", canonical_id)
        }
        host::HostError::CompileError { diagnostics } => {
            let summary = diagnostics
                .diagnostics
                .iter()
                .map(|d| format!("[{}] {}", d.code, d.message))
                .collect::<Vec<_>>()
                .join("; ");
            format!("HostError::CompileError: {}", summary)
        }
        // Catch-all for scheduler-related errors (feature-gated in verter_session).
        #[allow(unreachable_patterns)]
        other => format!("HostError: {}", other),
    }
}

// =============================================================================
// Code action conversion
// =============================================================================

/// Convert a `verter_actions::CodeAction` to an FFI-safe `FfiCodeAction`.
///
/// Span byte offsets are converted to UTF-16 for browser consumption.
pub fn code_action_to_ffi(action: &verter_actions::CodeAction, source: &str) -> FfiCodeAction {
    FfiCodeAction {
        title: action.title.clone(),
        kind: match action.kind {
            verter_actions::ActionKind::QuickFix => "quickfix".to_string(),
            verter_actions::ActionKind::Refactor => "refactor".to_string(),
            verter_actions::ActionKind::Source => "source".to_string(),
        },
        edits: action
            .edits
            .iter()
            .map(|edit| FfiTextEdit {
                span_start: byte_offset_to_utf16(source, edit.span.start),
                span_end: byte_offset_to_utf16(source, edit.span.end),
                new_text: edit.replacement.clone(),
            })
            .collect(),
        is_preferred: action.is_preferred,
        diagnostic_rule: action.diagnostic_rule.clone(),
    }
}

// =============================================================================
// Lint rule metadata conversion
// =============================================================================

/// Convert a lint rule to its FFI metadata representation.
pub fn lint_rule_to_ffi_metadata(rule: &dyn verter_diagnostics::LintRule) -> FfiLintRuleMetadata {
    FfiLintRuleMetadata {
        name: rule.name().to_string(),
        category: rule.category().as_str().to_string(),
        default_severity: match rule.default_severity() {
            Some(verter_diagnostics::Severity::Error) => "error".to_string(),
            Some(verter_diagnostics::Severity::Warning) => "warning".to_string(),
            Some(verter_diagnostics::Severity::Info) => "info".to_string(),
            Some(verter_diagnostics::Severity::Hint) => "hint".to_string(),
            None => "off".to_string(),
        },
    }
}

// ── Offset encoding conversion ──────────────────────────────────

/// Target encoding for offset conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetEncoding {
    /// UTF-8 byte offsets (no conversion needed).
    Utf8,
    /// UTF-16 code units (JavaScript, default LSP).
    Utf16,
    /// Unicode scalar values (codepoints).
    Utf32,
}

/// Convert a UTF-8 byte offset to the target encoding's offset.
///
/// The `text` must be the string the byte offset refers to (either SFC source
/// or generated TSX). The function counts encoding units from the start of
/// `text` up to `byte_offset`.
pub fn convert_offset(text: &str, byte_offset: u32, encoding: OffsetEncoding) -> u32 {
    match encoding {
        OffsetEncoding::Utf8 => byte_offset,
        OffsetEncoding::Utf16 => byte_offset_to_utf16(text, byte_offset),
        OffsetEncoding::Utf32 => utf8_to_utf32_offset(text, byte_offset),
    }
}

/// Convert a UTF-8 byte offset to UTF-16 code unit offset.
pub fn utf8_to_utf16_offset(text: &str, byte_offset: u32) -> u32 {
    byte_offset_to_utf16(text, byte_offset)
}

/// Convert a UTF-8 byte offset to UTF-32 (codepoint) offset.
fn utf8_to_utf32_offset(text: &str, byte_offset: u32) -> u32 {
    let clamped = clamp_to_char_boundary(text, byte_offset as usize);
    text[..clamped].chars().count() as u32
}

/// Input for a single binding's source span conversion.
pub struct DestructuredBindingInput<'a> {
    pub name: &'a str,
    pub source_start: u32,
    pub source_end: u32,
}

/// Convert destructured block metadata from UTF-8 to the target encoding.
///
/// `sfc_source` is the original SFC text (for converting source spans).
/// `tsx_code` is the generated TSX text (for converting block_start/block_end).
pub fn convert_destructured_block_meta(
    bindings: &[DestructuredBindingInput<'_>],
    block_start: u32,
    block_end: u32,
    sfc_source: &str,
    tsx_code: &str,
    encoding: OffsetEncoding,
) -> FfiDestructuredBlockMeta {
    FfiDestructuredBlockMeta {
        bindings: bindings
            .iter()
            .map(|b| FfiDestructuredBinding {
                name: b.name.to_string(),
                source_start: convert_offset(sfc_source, b.source_start, encoding),
                source_end: convert_offset(sfc_source, b.source_end, encoding),
            })
            .collect(),
        block_start: convert_offset(tsx_code, block_start, encoding),
        block_end: convert_offset(tsx_code, block_end, encoding),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_analysis() -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
            props: Vec::new(),
            events: Vec::new(),
            slots: Vec::new(),
            models: Vec::new(),
            exposed: Vec::new(),
            public_instance: None,
            sfc_blocks: None,
            type_registry: Vec::new(),
            components: Vec::new(),
            template_refs: Vec::new(),
            imports: Vec::new(),
            bindings: Vec::new(),
            vue_api_calls: Vec::new(),
            styles: Vec::new(),
            flags: verter_semantic::analysis::component_meta::ComponentMetaFlags::default(),
            root_reachability:
                verter_semantic::analysis::component_meta::RootReachability::NoFallthrough {
                    reason:
                        verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
                },
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness:
                verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
            fallthrough_surface:
                verter_semantic::analysis::component_meta::FallthroughSurface::None {
                    reason:
                        verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
                },
            macro_expansion_diagnostics: Vec::new(),
            options_api: false,
            file_path: String::new(),
        }
    }

    // ── Error path tests ──────────────────────────────────────────

    #[test]
    fn invalid_compile_error_policy() {
        let config = FfiHostConfig {
            compile_error_policy: Some("banana".to_string()),
            ..Default::default()
        };
        let err = ffi_config_to_host(config).unwrap_err();
        assert!(matches!(
            err,
            FfiConversionError::InvalidCompileErrorPolicy(_)
        ));
        assert!(err.to_string().contains("banana"));
    }

    #[test]
    fn invalid_analysis_level() {
        let config = FfiHostConfig {
            analysis_level: Some("turbo".to_string()),
            ..Default::default()
        };
        let err = ffi_config_to_host(config).unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidAnalysisLevel(_)));
    }

    #[test]
    fn invalid_hmr_strategy() {
        let profile = FfiCompileProfile {
            hmr_strategy: Some("rspack".to_string()),
            ..Default::default()
        };
        let err = ffi_profile_to_host(Some(profile)).unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidHmrStrategy(_)));
    }

    #[test]
    fn invalid_delimiters_count() {
        let profile = FfiCompileProfile {
            delimiters: Some(vec!["{{".to_string()]),
            ..Default::default()
        };
        let err = ffi_profile_to_host(Some(profile)).unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidDelimiters(1)));
    }

    #[test]
    fn invalid_file_kind() {
        let err = ffi_file_kind_to_host(Some("binary")).unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidFileKind(_)));
    }

    #[test]
    fn invalid_node_kind() {
        let kind = FfiVirtualNodeKind {
            kind: "fragment".to_string(),
            index: None,
        };
        let err = ffi_node_kind_to_host(kind).unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidNodeKind(_)));
    }

    // ── Happy path smoke tests ────────────────────────────────────

    #[test]
    fn config_defaults_are_valid() {
        let config = FfiHostConfig::default();
        let result = ffi_config_to_host(config).unwrap();
        assert!(result.dev_mode);
    }

    #[test]
    fn profile_none_returns_default() {
        let result = ffi_profile_to_host(None).unwrap();
        assert!(!result.is_production);
    }

    #[test]
    fn component_meta_type_registry_keeps_expanded_and_pre_expansion_type_information() {
        let analysis = verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
            props: Vec::new(),
            events: Vec::new(),
            slots: Vec::new(),
            models: Vec::new(),
            exposed: Vec::new(),
            public_instance: None,
            sfc_blocks: None,
            type_registry: vec![
                verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
                    name: "Props".to_string(),
                    type_expr: verter_semantic::analysis::type_expr::TypeExpr::Unknown {
                        raw: "{ label: string }".to_string(),
                    },
                    type_expansion: None,
                },
            ],
            components: Vec::new(),
            template_refs: Vec::new(),
            imports: Vec::new(),
            bindings: Vec::new(),
            vue_api_calls: Vec::new(),
            styles: Vec::new(),
            flags: verter_semantic::analysis::component_meta::ComponentMetaFlags::default(),
            root_reachability:
                verter_semantic::analysis::component_meta::RootReachability::NoFallthrough {
                    reason:
                        verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
                },
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness:
                verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
            fallthrough_surface:
                verter_semantic::analysis::component_meta::FallthroughSurface::None {
                    reason:
                        verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
                },
            macro_expansion_diagnostics: Vec::new(),
            options_api: false,
            file_path: "/src/App.vue".to_string(),
        };
        let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
            snapshot: host::FileAnalysisSnapshot::default(),
            mode: host::ProjectionMode::Expanded,
            whole_hash: [0; 16],
            resolved_macros: Vec::new(),
            resolved_type_registry: Vec::new(),
            resolved_type_registry_meta: vec![host::meta_resolve::ResolvedTypeRegistryMeta {
                name: "Props".to_string(),
                declaration: host::meta_resolve::ResolvedTypeDeclaration {
                    requested_name: "Props".to_string(),
                    declaration_id: None,
                    resolved_name: "Props".to_string(),
                    canonical_source: "/src/types.ts".to_string(),
                    span: verter_span::Span::new(10, 48),
                    kind: host::meta_resolve::ResolvedDeclarationKind::Interface,
                    text: Some("export interface Props { label: string }".to_string()),
                },
            }],
            evaluated_types: None,
            fact_versions: Vec::new(),
            compute_audit: None,
            origin_graph: None,
            request_id: 0,
            surface_identities: None,
        };

        let ffi = component_meta_analysis_to_ffi_with_resolution(analysis, Some(&resolved_state));
        let entry = ffi
            .type_registry
            .first()
            .expect("type registry entry should be present");

        assert_eq!(entry.name, "Props");
        assert_eq!(
            entry.raw_type.as_deref(),
            Some("export interface Props { label: string }"),
            "native payload should expose the pre-expansion source form explicitly",
        );
        assert_eq!(
            entry
                .declaration
                .as_ref()
                .map(|declaration| declaration.canonical_source.as_str()),
            Some("/src/types.ts"),
            "native payload should also retain declaration provenance",
        );
    }

    #[test]
    fn component_meta_type_registry_prefers_resolved_registry_type_expr_when_available() {
        let analysis = verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
            props: Vec::new(),
            events: Vec::new(),
            slots: Vec::new(),
            models: Vec::new(),
            exposed: Vec::new(),
            public_instance: None,
            sfc_blocks: None,
            type_registry: vec![
                verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
                    name: "Button".to_string(),
                    type_expr: verter_semantic::analysis::type_expr::TypeExpr::named("Button"),
                    type_expansion: None,
                },
            ],
            components: Vec::new(),
            template_refs: Vec::new(),
            imports: Vec::new(),
            bindings: Vec::new(),
            vue_api_calls: Vec::new(),
            styles: Vec::new(),
            flags: verter_semantic::analysis::component_meta::ComponentMetaFlags::default(),
            root_reachability:
                verter_semantic::analysis::component_meta::RootReachability::NoFallthrough {
                    reason:
                        verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
                },
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness:
                verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
            fallthrough_surface:
                verter_semantic::analysis::component_meta::FallthroughSurface::None {
                    reason:
                        verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
                },
            macro_expansion_diagnostics: Vec::new(),
            options_api: false,
            file_path: "/src/App.vue".to_string(),
        };
        let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
            snapshot: host::FileAnalysisSnapshot::default(),
            mode: host::ProjectionMode::Expanded,
            whole_hash: [0; 16],
            resolved_macros: Vec::new(),
            resolved_type_registry: vec![
                verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
                    name: "Button".to_string(),
                    type_expr: verter_semantic::analysis::type_expr::TypeExpr::Object(Arc::new(
                        verter_semantic::analysis::type_expr::ObjectExpr {
                            properties: vec![
                                verter_semantic::analysis::type_expr::ObjectMember::Property(
                                    verter_semantic::analysis::type_expr::ObjectProperty {
                                        name: "variants".to_string(),
                                        ty: verter_semantic::analysis::type_expr::TypeExpr::Object(
                                            Arc::new(
                                                verter_semantic::analysis::type_expr::ObjectExpr {
                                                    properties: vec![],
                                                },
                                            ),
                                        ),
                                        optional: false,
                                        readonly: false,
                                    },
                                ),
                            ],
                        },
                    )),
                    type_expansion: None,
                },
            ],
            resolved_type_registry_meta: vec![host::meta_resolve::ResolvedTypeRegistryMeta {
                name: "Button".to_string(),
                declaration: host::meta_resolve::ResolvedTypeDeclaration {
                    requested_name: "Button".to_string(),
                    declaration_id: None,
                    resolved_name: "Button".to_string(),
                    canonical_source: "/src/App.vue".to_string(),
                    span: verter_span::Span::new(10, 52),
                    kind: host::meta_resolve::ResolvedDeclarationKind::TypeAlias,
                    text: Some(
                        "type Button = ComponentConfig<typeof theme, MissingAppConfig>".to_string(),
                    ),
                },
            }],
            evaluated_types: None,
            fact_versions: Vec::new(),
            compute_audit: None,
            origin_graph: None,
            request_id: 0,
            surface_identities: None,
        };

        let ffi = component_meta_analysis_to_ffi_with_resolution(analysis, Some(&resolved_state));
        let entry = ffi
            .type_registry
            .first()
            .expect("type registry entry should be present");

        assert!(
            matches!(
                entry.r#type,
                verter_semantic::analysis::type_expr::TypeExpr::Object(_)
            ),
            "resolved registry entry should override the shallow analysis alias"
        );
        assert_eq!(
            entry.raw_type.as_deref(),
            Some("type Button = ComponentConfig<typeof theme, MissingAppConfig>"),
            "resolved registry should still keep the pre-expansion source text",
        );
    }

    #[test]
    fn component_meta_ffi_exposes_root_info_summary() {
        let analysis = verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
            props: Vec::new(),
            events: Vec::new(),
            slots: Vec::new(),
            models: Vec::new(),
            exposed: Vec::new(),
            public_instance: None,
            sfc_blocks: None,
            type_registry: Vec::new(),
            components: Vec::new(),
            template_refs: Vec::new(),
            imports: Vec::new(),
            bindings: Vec::new(),
            vue_api_calls: Vec::new(),
            styles: Vec::new(),
            flags: verter_semantic::analysis::component_meta::ComponentMetaFlags::default(),
            root_reachability: verter_semantic::analysis::component_meta::RootReachability::Branches {
                branches: vec![
                    verter_semantic::analysis::component_meta::RootBranch {
                        branch_index: 0,
                        condition_text: None,
                        target: verter_semantic::analysis::component_meta::RootTargetRef::ComponentUsage {
                            element_index: 1,
                            usage_index: 0,
                            name: "PrimaryButton".to_string(),
                            import_source: Some("./PrimaryButton.vue".to_string()),
                        },
                        consumed: verter_semantic::analysis::component_meta::ConsumedRootBindings {
                            attrs: vec!["class".to_string()],
                            listeners: vec!["click".to_string()],
                            has_dynamic_attr_name: false,
                            has_dynamic_listener_name: false,
                        },
                        has_unknown_spread: false,
                    },
                    verter_semantic::analysis::component_meta::RootBranch {
                        branch_index: 1,
                        condition_text: Some("isFallback".to_string()),
                        target: verter_semantic::analysis::component_meta::RootTargetRef::NativeElement {
                            element_index: 2,
                            tag: "button".to_string(),
                        },
                        consumed: verter_semantic::analysis::component_meta::ConsumedRootBindings {
                            attrs: Vec::new(),
                            listeners: Vec::new(),
                            has_dynamic_attr_name: false,
                            has_dynamic_listener_name: false,
                        },
                        has_unknown_spread: false,
                    },
                ],
            },
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness:
                verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
            fallthrough_surface: verter_semantic::analysis::component_meta::FallthroughSurface::Branches {
                branches: Vec::new(),
            },
            macro_expansion_diagnostics: Vec::new(),
            options_api: false,
            file_path: "/src/App.vue".to_string(),
        };

        let ffi = component_meta_analysis_to_ffi(analysis);
        match ffi.root_info {
            FfiRootInfo {
                kind: FfiRootInfoKind::Conditional,
                reason: None,
                targets,
            } => {
                assert_eq!(targets.len(), 2);
                assert!(matches!(
                    targets.first(),
                    Some(FfiRootTargetRef::ComponentUsage { name, .. }) if name == "PrimaryButton"
                ));
                assert!(matches!(
                    targets.get(1),
                    Some(FfiRootTargetRef::NativeElement { tag, .. }) if tag == "button"
                ));
            }
            other => panic!("unexpected root info payload: {other:?}"),
        }
    }

    #[test]
    fn file_kind_vue_default() {
        let kind = ffi_file_kind_to_host(None).unwrap();
        assert_eq!(kind, host::FileKind::VueSfc);
    }

    #[test]
    fn file_kind_non_sfc() {
        let kind = ffi_file_kind_to_host(Some("non_sfc")).unwrap();
        assert_eq!(kind, host::FileKind::NonSfc);
    }

    #[test]
    fn node_kind_round_trip() {
        let kinds = [
            ("main", host::VirtualNodeKind::Main),
            ("script", host::VirtualNodeKind::Script),
            ("template", host::VirtualNodeKind::Template),
        ];
        for (s, expected) in &kinds {
            let ffi = FfiVirtualNodeKind {
                kind: s.to_string(),
                index: None,
            };
            assert_eq!(ffi_node_kind_to_host(ffi).unwrap(), *expected);
        }
    }

    #[test]
    fn node_kind_style_with_index() {
        let ffi = FfiVirtualNodeKind {
            kind: "style".to_string(),
            index: Some(2),
        };
        assert_eq!(
            ffi_node_kind_to_host(ffi).unwrap(),
            host::VirtualNodeKind::Style { index: 2 }
        );
    }

    #[test]
    fn config_case_insensitive_policy() {
        let config = FfiHostConfig {
            compile_error_policy: Some("STRICT".to_string()),
            ..Default::default()
        };
        let result = ffi_config_to_host(config).unwrap();
        assert_eq!(
            result.compile_error_policy,
            host::CompileErrorPolicy::StrictError
        );
    }

    #[test]
    fn ffi_conversion_error_display() {
        let err = FfiConversionError::InvalidFileKind("xyz".to_string());
        assert_eq!(err.to_string(), "invalid file_kind 'xyz'");

        let err = FfiConversionError::InvalidDelimiters(3);
        assert_eq!(
            err.to_string(),
            "delimiters must have exactly 2 elements, got 3"
        );
    }

    #[test]
    fn ffi_conversion_error_to_string_impl() {
        let err = FfiConversionError::InvalidHmrStrategy("rspack".to_string());
        let s: String = err.into();
        assert!(s.contains("rspack"));
    }

    // ── Config: all fields populated ─────────────────────────────────

    #[test]
    fn config_all_fields() {
        let config = FfiHostConfig {
            dev_mode: Some(false),
            compile_error_policy: Some("strict".to_string()),
            lsp_scheme: Some("my-scheme".to_string()),
            max_profiles_per_file: Some(4),
            resolve_extensions: Some(vec![".vue".to_string(), ".ts".to_string()]),
            analysis_level: Some("essential".to_string()),
            audit_enabled: None,
            footprint_capture: None,
        };
        let result = ffi_config_to_host(config).unwrap();
        assert!(!result.dev_mode);
        assert_eq!(
            result.compile_error_policy,
            host::CompileErrorPolicy::StrictError
        );
        assert_eq!(result.lsp_scheme, "my-scheme");
        assert_eq!(result.max_profiles_per_file, 4);
        assert_eq!(result.resolve_extensions, vec![".vue", ".ts"]);
        assert_eq!(result.analysis_level, host::AnalysisLevel::Essential);
    }

    #[test]
    fn expansion_metadata_to_ffi_preserves_exactness_and_execution_status() {
        let ffi = expansion_metadata_to_ffi(verter_semantic::analysis::type_expand::ExpansionMetadata {
            exactness: verter_semantic::analysis::type_solver::result::SolverExactness::ExactSymbolic,
            execution_status:
                verter_semantic::analysis::type_solver::result::ExecutionStatus::HardStop,
            diagnostics: vec![verter_semantic::analysis::type_expand::ExpansionDiagnostic {
                reason: verter_semantic::analysis::type_expand::ExpansionStopReason::UnsupportedOperator,
                context: "kept symbolic".to_string(),
                property_name: None,
            }],
        });

        assert_eq!(ffi.exactness, "exactSymbolic");
        assert_eq!(ffi.execution_status, "hardStop");
        assert_eq!(ffi.diagnostics.len(), 1);
    }

    // ── Config: all policy string variants ───────────────────────────

    #[test]
    fn config_policy_all_variants() {
        let strict_variants = ["strict", "strict_error", "strictError", "STRICT", "Strict"];
        for v in &strict_variants {
            let cfg = FfiHostConfig {
                compile_error_policy: Some(v.to_string()),
                ..Default::default()
            };
            assert_eq!(
                ffi_config_to_host(cfg).unwrap().compile_error_policy,
                host::CompileErrorPolicy::StrictError,
                "variant '{v}' should map to StrictError"
            );
        }

        let dev_variants = [
            "dev",
            "dev_serve_last_known_good",
            "devServeLastKnownGood",
            "DEV",
        ];
        for v in &dev_variants {
            let cfg = FfiHostConfig {
                compile_error_policy: Some(v.to_string()),
                ..Default::default()
            };
            assert_eq!(
                ffi_config_to_host(cfg).unwrap().compile_error_policy,
                host::CompileErrorPolicy::DevServeLastKnownGood,
                "variant '{v}' should map to DevServeLastKnownGood"
            );
        }
    }

    // ── Config: all analysis level variants ──────────────────────────

    #[test]
    fn config_analysis_level_all_variants() {
        let cases = [
            ("none", host::AnalysisLevel::None),
            ("NONE", host::AnalysisLevel::None),
            ("essential", host::AnalysisLevel::Essential),
            ("ESSENTIAL", host::AnalysisLevel::Essential),
            ("full", host::AnalysisLevel::Full),
            ("FULL", host::AnalysisLevel::Full),
        ];
        for (input, expected) in &cases {
            let cfg = FfiHostConfig {
                analysis_level: Some(input.to_string()),
                ..Default::default()
            };
            assert_eq!(
                ffi_config_to_host(cfg).unwrap().analysis_level,
                *expected,
                "analysis level '{input}' mismatch"
            );
        }
    }

    // ── Profile: all fields populated ────────────────────────────────

    #[test]
    fn profile_all_fields() {
        let profile = FfiCompileProfile {
            filename: Some("Comp.vue".to_string()),
            is_production: Some(true),
            ssr: Some(true),
            hmr_strategy: Some("vite".to_string()),
            component_id: Some("abc123".to_string()),
            delimiters: Some(vec!["<%".to_string(), "%>".to_string()]),
            custom_elements: Some(vec!["my-el".to_string()]),
            comments: Some(true),
            runtime_module_name: Some("vue/runtime".to_string()),
            types_module_name: Some("@custom/types".to_string()),
            force_vapor: Some(true),
            force_js: Some(true),
            source_map: Some(true),
            target: Some("ide".to_string()),
            strict_slots: Some(true),
        };
        let result = ffi_profile_to_host(Some(profile)).unwrap();
        assert_eq!(result.filename, Some("Comp.vue".to_string()));
        assert!(result.is_production);
        assert!(result.ssr);
        assert!(result.target.needs_tsx());
        assert!(result.strict_slots);
        assert_eq!(result.hmr_strategy, host::HmrStrategy::Vite);
        assert_eq!(result.component_id, Some("abc123".to_string()));
        assert_eq!(
            result.delimiters,
            Some(("<%".to_string(), "%>".to_string()))
        );
        assert_eq!(result.custom_elements, Some(vec!["my-el".to_string()]));
        assert_eq!(result.comments, Some(true));
        assert_eq!(result.runtime_module_name, Some("vue/runtime".to_string()));
        assert_eq!(result.types_module_name, Some("@custom/types".to_string()));
        assert!(result.force_vapor);
        assert!(result.force_js);
        assert!(result.source_map);
    }

    // ── Profile: all HMR strategy variants ───────────────────────────

    #[test]
    fn profile_hmr_strategy_all_variants() {
        let cases = [
            ("vite", host::HmrStrategy::Vite),
            ("VITE", host::HmrStrategy::Vite),
            ("webpack", host::HmrStrategy::Webpack),
            ("WEBPACK", host::HmrStrategy::Webpack),
            ("none", host::HmrStrategy::None),
            ("NONE", host::HmrStrategy::None),
        ];
        for (input, expected) in &cases {
            let profile = FfiCompileProfile {
                hmr_strategy: Some(input.to_string()),
                ..Default::default()
            };
            assert_eq!(
                ffi_profile_to_host(Some(profile)).unwrap().hmr_strategy,
                *expected,
                "hmr strategy '{input}' mismatch"
            );
        }
    }

    // ── Profile: delimiters edge cases ───────────────────────────────

    #[test]
    fn profile_delimiters_three_elements() {
        let profile = FfiCompileProfile {
            delimiters: Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
            ..Default::default()
        };
        let err = ffi_profile_to_host(Some(profile)).unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidDelimiters(3)));
    }

    #[test]
    fn profile_delimiters_empty_vec() {
        let profile = FfiCompileProfile {
            delimiters: Some(vec![]),
            ..Default::default()
        };
        let err = ffi_profile_to_host(Some(profile)).unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidDelimiters(0)));
    }

    // ── File kind: all accepted variants ─────────────────────────────

    #[test]
    fn file_kind_all_vue_variants() {
        for v in &["vue", "sfc", "vue_sfc", "VUE", "SFC", "Vue_Sfc"] {
            assert_eq!(
                ffi_file_kind_to_host(Some(v)).unwrap(),
                host::FileKind::VueSfc,
                "'{v}' should map to VueSfc"
            );
        }
    }

    #[test]
    fn file_kind_all_non_sfc_variants() {
        for v in &["non_sfc", "text", "file", "NON_SFC", "TEXT", "FILE"] {
            assert_eq!(
                ffi_file_kind_to_host(Some(v)).unwrap(),
                host::FileKind::NonSfc,
                "'{v}' should map to NonSfc"
            );
        }
    }

    // ── Node kind: custom with index ─────────────────────────────────

    #[test]
    fn node_kind_custom_with_index() {
        let ffi = FfiVirtualNodeKind {
            kind: "custom".to_string(),
            index: Some(5),
        };
        assert_eq!(
            ffi_node_kind_to_host(ffi).unwrap(),
            host::VirtualNodeKind::Custom { index: 5 }
        );
    }

    #[test]
    fn node_kind_style_default_index() {
        let ffi = FfiVirtualNodeKind {
            kind: "style".to_string(),
            index: None,
        };
        assert_eq!(
            ffi_node_kind_to_host(ffi).unwrap(),
            host::VirtualNodeKind::Style { index: 0 }
        );
    }

    #[test]
    fn node_kind_case_insensitive() {
        for kind in &["MAIN", "Main", "SCRIPT", "Script", "TEMPLATE", "Template"] {
            let ffi = FfiVirtualNodeKind {
                kind: kind.to_string(),
                index: None,
            };
            assert!(
                ffi_node_kind_to_host(ffi).is_ok(),
                "'{kind}' should be accepted"
            );
        }
    }

    // ── Upsert conversion ────────────────────────────────────────────

    #[test]
    fn upsert_basic() {
        let ffi = FfiUpsertRequest {
            canonical_id: Some("/src/Comp.vue".to_string()),
            input_id: "src/Comp.vue".to_string(),
            source: "<template>hi</template>".to_string(),
            file_kind: None,
            aliases: None,
        };
        let result = ffi_upsert_to_host(ffi).unwrap();
        assert_eq!(result.canonical_id, Some("/src/Comp.vue".to_string()));
        assert_eq!(result.input_id, "src/Comp.vue");
        assert_eq!(&*result.source, "<template>hi</template>");
        assert_eq!(result.file_kind, host::FileKind::VueSfc);
        assert!(result.aliases.is_empty());
    }

    #[test]
    fn upsert_with_aliases_and_non_sfc() {
        let ffi = FfiUpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: "export type Foo = string;".to_string(),
            file_kind: Some("non_sfc".to_string()),
            aliases: Some(vec!["@/types".to_string(), "~/types".to_string()]),
        };
        let result = ffi_upsert_to_host(ffi).unwrap();
        assert!(result.canonical_id.is_none());
        assert_eq!(result.file_kind, host::FileKind::NonSfc);
        assert_eq!(result.aliases, vec!["@/types", "~/types"]);
    }

    #[test]
    fn upsert_source_is_arc_str() {
        let ffi = FfiUpsertRequest {
            canonical_id: None,
            input_id: "test.vue".to_string(),
            source: "hello".to_string(),
            file_kind: None,
            aliases: None,
        };
        let result = ffi_upsert_to_host(ffi).unwrap();
        // source should be Arc<str>, verify via reference counting
        let arc: Arc<str> = result.source;
        assert_eq!(&*arc, "hello");
    }

    #[test]
    fn upsert_invalid_file_kind() {
        let ffi = FfiUpsertRequest {
            canonical_id: None,
            input_id: "test.vue".to_string(),
            source: "x".to_string(),
            file_kind: Some("binary".to_string()),
            aliases: None,
        };
        assert!(ffi_upsert_to_host(ffi).is_err());
    }

    // ── Virtual query conversion ─────────────────────────────────────

    #[test]
    fn virtual_query_with_raw_id() {
        let ffi = FfiVirtualQuery {
            raw_id: Some("Comp.vue?vue&type=style&index=0".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: None,
        };
        let result = ffi_virtual_query_to_host(ffi).unwrap();
        assert_eq!(
            result.raw_id,
            Some("Comp.vue?vue&type=style&index=0".to_string())
        );
        assert!(result.canonical_id.is_none());
        assert!(result.node_kind.is_none());
    }

    #[test]
    fn virtual_query_with_explicit_kind() {
        let ffi = FfiVirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(FfiVirtualNodeKind {
                kind: "template".to_string(),
                index: None,
            }),
            compile_profile: Some(FfiCompileProfile {
                ssr: Some(true),
                ..Default::default()
            }),
        };
        let result = ffi_virtual_query_to_host(ffi).unwrap();
        assert_eq!(result.canonical_id, Some("/src/Comp.vue".to_string()));
        assert_eq!(result.node_kind, Some(host::VirtualNodeKind::Template));
        assert!(result.compile_profile.ssr);
    }

    #[test]
    fn virtual_query_invalid_node_kind_propagates() {
        let ffi = FfiVirtualQuery {
            raw_id: None,
            canonical_id: None,
            node_kind: Some(FfiVirtualNodeKind {
                kind: "banana".to_string(),
                index: None,
            }),
            compile_profile: None,
        };
        assert!(matches!(
            ffi_virtual_query_to_host(ffi).unwrap_err(),
            FfiConversionError::InvalidNodeKind(_)
        ));
    }

    // ── Output direction: host_node_kind_to_ffi ──────────────────────

    #[test]
    fn node_kind_to_ffi_all_variants() {
        let cases: &[(host::VirtualNodeKind, &str, Option<u32>)] = &[
            (host::VirtualNodeKind::Main, "main", None),
            (host::VirtualNodeKind::Script, "script", None),
            (host::VirtualNodeKind::Template, "template", None),
            (host::VirtualNodeKind::Style { index: 3 }, "style", Some(3)),
            (
                host::VirtualNodeKind::Custom { index: 7 },
                "custom",
                Some(7),
            ),
        ];
        for (input, expected_kind, expected_index) in cases {
            let ffi = host_node_kind_to_ffi(input);
            assert_eq!(ffi.kind, *expected_kind);
            assert_eq!(ffi.index, *expected_index);
        }
    }

    // ── Output direction: host_diagnostics_to_ffi ────────────────────

    #[test]
    fn diagnostics_all_severity_levels() {
        let snapshot = host::DiagnosticsSnapshot {
            diagnostics: vec![
                host::HostDiagnostic {
                    severity: host::HostSeverity::Error,
                    code: "E001".to_string(),
                    message: "error msg".to_string(),
                    span: Some(verter_span::Span::new(0, 10)),
                },
                host::HostDiagnostic {
                    severity: host::HostSeverity::Warning,
                    code: "W001".to_string(),
                    message: "warning msg".to_string(),
                    span: None,
                },
                host::HostDiagnostic {
                    severity: host::HostSeverity::Info,
                    code: "I001".to_string(),
                    message: "info msg".to_string(),
                    span: None,
                },
            ],
            has_errors: true,
        };
        let ffi = host_diagnostics_to_ffi(&snapshot, None);
        assert!(ffi.has_errors);
        assert_eq!(ffi.diagnostics.len(), 3);
        assert_eq!(ffi.diagnostics[0].severity, "error");
        assert_eq!(ffi.diagnostics[0].code, "E001");
        assert_eq!(ffi.diagnostics[0].span_start, Some(0));
        assert_eq!(ffi.diagnostics[0].span_end, Some(10));
        assert_eq!(ffi.diagnostics[1].severity, "warning");
        assert_eq!(ffi.diagnostics[1].span_start, None);
        assert_eq!(ffi.diagnostics[2].severity, "info");
        assert_eq!(ffi.diagnostics[2].span_start, None);
        assert_eq!(ffi.diagnostics[2].span_end, None);
    }

    #[test]
    fn diagnostics_empty() {
        let snapshot = host::DiagnosticsSnapshot::default();
        let ffi = host_diagnostics_to_ffi(&snapshot, None);
        assert!(!ffi.has_errors);
        assert!(ffi.diagnostics.is_empty());
    }

    #[test]
    fn host_diagnostics_to_ffi_converts_utf8_spans_to_utf16_with_unicode_source() {
        // "😀" is 4 UTF-8 bytes and 2 UTF-16 code units.
        let source = "a😀b";
        let snapshot = host::DiagnosticsSnapshot {
            diagnostics: vec![host::HostDiagnostic {
                severity: host::HostSeverity::Error,
                code: "E_UTF".to_string(),
                message: "unicode".to_string(),
                span: Some(verter_span::Span::new(1, 5)), // byte offset at 😀 start..right after
            }],
            has_errors: true,
        };

        let ffi = host_diagnostics_to_ffi(&snapshot, Some(source));
        assert_eq!(ffi.diagnostics.len(), 1);
        assert_eq!(ffi.diagnostics[0].span_start, Some(1));
        assert_eq!(ffi.diagnostics[0].span_end, Some(3));
    }

    #[test]
    fn host_diagnostics_to_ffi_preserves_none_spans() {
        let snapshot = host::DiagnosticsSnapshot {
            diagnostics: vec![host::HostDiagnostic {
                severity: host::HostSeverity::Warning,
                code: "W_NONE".to_string(),
                message: "none".to_string(),
                span: None,
            }],
            has_errors: false,
        };
        let ffi = host_diagnostics_to_ffi(&snapshot, Some("abc"));
        assert_eq!(ffi.diagnostics.len(), 1);
        assert_eq!(ffi.diagnostics[0].span_start, None);
        assert_eq!(ffi.diagnostics[0].span_end, None);
    }

    #[test]
    fn lint_diagnostics_to_utf16_converts_spans() {
        let source = "a😀b";
        let input = vec![verter_diagnostics::LintDiagnostic {
            rule: "r".to_string(),
            category: "c".to_string(),
            severity: verter_diagnostics::Severity::Error,
            message: "m".to_string(),
            span: verter_span::Span::new(1, 5),
            tags: vec![],
            span_kind: verter_diagnostics::DiagnosticSpanKind::ElementOpenTag,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }];

        let out = lint_diagnostics_to_utf16(input, Some(source));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span.start, 1);
        assert_eq!(out[0].span.end, 3);
        assert!(out[0].tags.is_empty(), "tags should be unchanged");
    }

    // ── Output direction: host_update_to_ffi ─────────────────────────

    #[test]
    fn update_result_full_round_trip() {
        let source = "a😀b";
        let host_result = host::HostUpdateResult {
            canonical_id: "/src/App.vue".to_string(),
            changed: true,
            slice_changes: host::SliceChanges {
                script_changed: true,
                template_changed: false,
                style_indices_changed: vec![0, 2],
                custom_indices_changed: vec![1],
                structure_changed: true,
                descriptor_changed: false,
            },
            changed_virtual_nodes: vec![
                host::VirtualNodeKind::Script,
                host::VirtualNodeKind::Style { index: 0 },
            ],
            removed_virtual_nodes: vec![host::VirtualNodeKind::Style { index: 2 }],
            changed_virtual_ids: vec!["App.vue?type=script".to_string()],
            removed_virtual_ids: vec!["App.vue?type=style&index=2".to_string()],
            changed_lsp_ids: vec!["App.vue._VERTER_.script.ts".to_string()],
            removed_lsp_ids: vec!["App.vue._VERTER_.style.2.css".to_string()],
            diagnostics: host::DiagnosticsSnapshot {
                diagnostics: vec![host::HostDiagnostic {
                    severity: host::HostSeverity::Warning,
                    code: "W002".to_string(),
                    message: "unused var".to_string(),
                    span: Some(verter_span::Span::new(42, 45)),
                }],
                has_errors: false,
            },
            external_source_requests: vec![host::ExternalSourceRequest {
                owner_canonical_id: "/src/App.vue".to_string(),
                block_kind: host::ExternalBlockKind::Script,
                index: 0,
                specifier: "./script.ts".to_string(),
                resolved_canonical_id: "/src/script.ts".to_string(),
            }],
            import_specifiers: vec![host::ScriptImportInfo {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec!["ref".to_string(), "computed".to_string()],
            }],
            module_references: host::VerterHost::new_standalone(host::HostConfig::default())
                .upsert(host::UpsertRequest {
                    canonical_id: Some("/src/dynamic.ts".to_string()),
                    input_id: "/src/dynamic.ts".to_string(),
                    source: std::sync::Arc::from("const mod = import('./Foo.vue');"),
                    file_kind: host::FileKind::NonSfc,
                    aliases: Vec::new(),
                })
                .expect("upsert should extract module references")
                .module_references,
            preprocessor_requests: Vec::new(),
            export_signatures: Vec::new(),
            parse_duration_ms: 1.5,
        };

        let ffi = host_update_to_ffi(host_result, Some(source));
        assert_eq!(ffi.canonical_id, "/src/App.vue");
        assert!(ffi.changed);

        // slice changes
        assert!(ffi.slice_changes.script_changed);
        assert!(!ffi.slice_changes.template_changed);
        assert_eq!(ffi.slice_changes.style_indices_changed, vec![0, 2]);
        assert_eq!(ffi.slice_changes.custom_indices_changed, vec![1]);
        assert!(ffi.slice_changes.structure_changed);
        assert!(!ffi.slice_changes.descriptor_changed);

        // virtual nodes (usize→u32 for indexed kinds)
        assert_eq!(ffi.changed_virtual_nodes.len(), 2);
        assert_eq!(ffi.changed_virtual_nodes[0].kind, "script");
        assert_eq!(ffi.changed_virtual_nodes[1].kind, "style");
        assert_eq!(ffi.changed_virtual_nodes[1].index, Some(0));
        assert_eq!(ffi.removed_virtual_nodes.len(), 1);
        assert_eq!(ffi.removed_virtual_nodes[0].kind, "style");
        assert_eq!(ffi.removed_virtual_nodes[0].index, Some(2));

        // IDs
        assert_eq!(ffi.changed_virtual_ids, vec!["App.vue?type=script"]);
        assert_eq!(ffi.removed_virtual_ids, vec!["App.vue?type=style&index=2"]);
        assert_eq!(ffi.changed_lsp_ids, vec!["App.vue._VERTER_.script.ts"]);
        assert_eq!(ffi.removed_lsp_ids, vec!["App.vue._VERTER_.style.2.css"]);

        // diagnostics
        assert!(!ffi.diagnostics.has_errors);
        assert_eq!(ffi.diagnostics.diagnostics.len(), 1);
        assert_eq!(ffi.diagnostics.diagnostics[0].severity, "warning");

        // external source requests
        assert_eq!(ffi.external_source_requests.len(), 1);
        assert_eq!(
            ffi.external_source_requests[0].owner_canonical_id,
            "/src/App.vue"
        );
        assert_eq!(ffi.external_source_requests[0].block_kind, "script");
        assert_eq!(ffi.external_source_requests[0].index, 0);
        assert_eq!(ffi.external_source_requests[0].specifier, "./script.ts");

        // import specifiers
        assert_eq!(ffi.import_specifiers.len(), 1);
        assert_eq!(ffi.import_specifiers[0].source, "vue");
        assert!(!ffi.import_specifiers[0].is_type_only);
        assert_eq!(ffi.import_specifiers[0].bindings, vec!["ref", "computed"]);

        assert_eq!(ffi.module_references.len(), 1);
        assert_eq!(ffi.module_references[0].syntax, "dynamicImport");
        assert_eq!(ffi.module_references[0].analyzability, "exact");
        assert_eq!(
            ffi.module_references[0].literal_specifier.as_deref(),
            Some("./Foo.vue")
        );
        assert_eq!(ffi.module_references[0].expr_span_start, 19);
        assert_eq!(ffi.module_references[0].expr_span_end, 30);

        assert_eq!(ffi.parse_duration_ms, 1.5);
    }

    #[test]
    fn host_update_to_ffi_export_signatures() {
        // Use the host to produce real export signatures from a barrel file
        let h = host::VerterHost::new_standalone(host::HostConfig::default());
        let result = h
            .upsert(host::UpsertRequest {
                canonical_id: Some("/src/barrel.ts".to_string()),
                input_id: "/src/barrel.ts".to_string(),
                source: std::sync::Arc::from(
                    "export { default as Button } from './Button.vue';\nexport type { Props } from './types';",
                ),
                file_kind: host::FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        // Verify host produced export signatures
        assert!(
            !result.export_signatures.is_empty(),
            "barrel file must produce export signatures"
        );

        let ffi = host_update_to_ffi(result, None);

        // Positive: re-export signatures are mapped correctly
        let button_sig = ffi.export_signatures.iter().find(|s| s.name == "Button");
        assert!(button_sig.is_some(), "Button re-export must be present");
        let button = button_sig.unwrap();
        assert!(!button.is_type);
        assert_eq!(button.reexport_source, Some("./Button.vue".to_string()));
        assert_eq!(button.reexport_local, Some("default".to_string()));

        let props_sig = ffi.export_signatures.iter().find(|s| s.name == "Props");
        assert!(props_sig.is_some(), "Props type re-export must be present");
        let props = props_sig.unwrap();
        assert!(props.is_type);
        assert_eq!(props.reexport_source, Some("./types".to_string()));
    }

    #[test]
    fn host_update_to_ffi_export_signatures_local_exports() {
        let h = host::VerterHost::new_standalone(host::HostConfig::default());
        let result = h
            .upsert(host::UpsertRequest {
                canonical_id: Some("/src/utils.ts".to_string()),
                input_id: "/src/utils.ts".to_string(),
                source: std::sync::Arc::from(
                    "export function greet() {}\nexport type Color = string;",
                ),
                file_kind: host::FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        let ffi = host_update_to_ffi(result, None);

        let greet_sig = ffi.export_signatures.iter().find(|s| s.name == "greet");
        assert!(greet_sig.is_some(), "local export must be present");
        // Negative: local exports must not have reexport fields
        assert_eq!(greet_sig.unwrap().reexport_source, None);
        assert_eq!(greet_sig.unwrap().reexport_local, None);

        let color_sig = ffi.export_signatures.iter().find(|s| s.name == "Color");
        assert!(color_sig.is_some(), "type export must be present");
        assert!(color_sig.unwrap().is_type);
    }

    #[test]
    fn host_update_to_ffi_export_signatures_empty() {
        let result = host::HostUpdateResult::no_change("/src/Empty.vue".to_string());
        let ffi = host_update_to_ffi(result, None);
        assert!(
            ffi.export_signatures.is_empty(),
            "no-change result must have empty export_signatures"
        );
    }

    #[test]
    fn host_update_to_ffi_uses_utf16_conversion_for_embedded_diagnostics() {
        let source = "a😀b";
        let result = host::HostUpdateResult {
            diagnostics: host::DiagnosticsSnapshot {
                diagnostics: vec![host::HostDiagnostic {
                    severity: host::HostSeverity::Error,
                    code: "E_UTF".to_string(),
                    message: "unicode".to_string(),
                    span: Some(verter_span::Span::new(1, 5)),
                }],
                has_errors: true,
            },
            ..host::HostUpdateResult::no_change("x".to_string())
        };

        let ffi = host_update_to_ffi(result, Some(source));
        assert_eq!(ffi.diagnostics.diagnostics.len(), 1);
        assert_eq!(ffi.diagnostics.diagnostics[0].span_start, Some(1));
        assert_eq!(ffi.diagnostics.diagnostics[0].span_end, Some(3));
    }

    #[test]
    fn update_result_external_block_kinds() {
        let kinds = [
            (host::ExternalBlockKind::Script, "script"),
            (host::ExternalBlockKind::Template, "template"),
            (host::ExternalBlockKind::Style, "style"),
            (host::ExternalBlockKind::Custom, "custom"),
        ];
        for (host_kind, expected_str) in &kinds {
            let result = host::HostUpdateResult {
                external_source_requests: vec![host::ExternalSourceRequest {
                    owner_canonical_id: "x".to_string(),
                    block_kind: *host_kind,
                    index: 0,
                    specifier: "s".to_string(),
                    resolved_canonical_id: "r".to_string(),
                }],
                ..host::HostUpdateResult::no_change("x".to_string())
            };
            let ffi = host_update_to_ffi(result, Some("source"));
            assert_eq!(
                ffi.external_source_requests[0].block_kind, *expected_str,
                "block kind mismatch"
            );
        }
    }

    // ── Output direction: host_virtual_file_to_ffi ───────────────────

    #[test]
    fn virtual_file_arc_to_string() {
        let source = "a😀b";
        let response = host::VirtualFileResponse {
            id: "Comp.vue._VERTER_.script.ts".to_string(),
            code: Arc::from("export default {}"),
            source_map: Some(Arc::from("{\"mappings\":\"\"}")),
            lang: Some("ts".to_string()),
            stale: true,
            diagnostics: host::DiagnosticsSnapshot {
                diagnostics: vec![host::HostDiagnostic {
                    severity: host::HostSeverity::Warning,
                    code: "W_UTF".to_string(),
                    message: "unicode".to_string(),
                    span: Some(verter_span::Span::new(1, 5)),
                }],
                has_errors: false,
            },
            meta: host::VirtualMeta {
                scope_id: Some("data-v-abc123".to_string()),
                block_type: None,
                style_index: Some(2),
                custom_index: None,
            },
        };
        let ffi = host_virtual_file_to_ffi(response, Some(source));
        assert_eq!(ffi.id, "Comp.vue._VERTER_.script.ts");
        assert_eq!(ffi.code, "export default {}");
        assert_eq!(ffi.source_map, Some("{\"mappings\":\"\"}".to_string()));
        assert_eq!(ffi.lang, Some("ts".to_string()));
        assert!(ffi.stale);
        assert_eq!(ffi.meta.scope_id, Some("data-v-abc123".to_string()));
        assert!(ffi.meta.block_type.is_none());
        assert_eq!(ffi.meta.style_index, Some(2));
        assert!(ffi.meta.custom_index.is_none());
        assert_eq!(ffi.diagnostics.diagnostics.len(), 1);
        assert_eq!(ffi.diagnostics.diagnostics[0].span_start, Some(1));
        assert_eq!(ffi.diagnostics.diagnostics[0].span_end, Some(3));
    }

    #[test]
    fn host_virtual_file_to_ffi_uses_utf16_conversion_for_embedded_diagnostics() {
        let source = "a😀b";
        let response = host::VirtualFileResponse {
            id: "x".to_string(),
            code: Arc::from(""),
            source_map: None,
            lang: None,
            stale: false,
            diagnostics: host::DiagnosticsSnapshot {
                diagnostics: vec![host::HostDiagnostic {
                    severity: host::HostSeverity::Error,
                    code: "E_UTF".to_string(),
                    message: "unicode".to_string(),
                    span: Some(verter_span::Span::new(1, 5)),
                }],
                has_errors: true,
            },
            meta: host::VirtualMeta::default(),
        };

        let ffi = host_virtual_file_to_ffi(response, Some(source));
        assert_eq!(ffi.diagnostics.diagnostics.len(), 1);
        assert_eq!(ffi.diagnostics.diagnostics[0].span_start, Some(1));
        assert_eq!(ffi.diagnostics.diagnostics[0].span_end, Some(3));
    }

    #[test]
    fn virtual_file_no_source_map() {
        let response = host::VirtualFileResponse {
            id: "x".to_string(),
            code: Arc::from(""),
            source_map: None,
            lang: None,
            stale: false,
            diagnostics: host::DiagnosticsSnapshot::default(),
            meta: host::VirtualMeta::default(),
        };
        let ffi = host_virtual_file_to_ffi(response, Some("source"));
        assert!(ffi.source_map.is_none());
        assert!(ffi.lang.is_none());
        assert!(!ffi.stale);
    }

    // ── Output direction: host_resolved_id_to_ffi ────────────────────

    #[test]
    fn resolved_id_conversion() {
        let resolved = host::ResolvedId {
            canonical_id: "/src/Comp.vue".to_string(),
            node_kind: host::VirtualNodeKind::Style { index: 1 },
            exists_in_host: true,
            bundler_id: "Comp.vue?vue&type=style&index=1&lang.css".to_string(),
            lsp_id: "Comp.vue._VERTER_.style.1.css".to_string(),
        };
        let ffi = host_resolved_id_to_ffi(resolved);
        assert_eq!(ffi.canonical_id, "/src/Comp.vue");
        assert_eq!(ffi.node_kind.kind, "style");
        assert_eq!(ffi.node_kind.index, Some(1));
        assert!(ffi.exists_in_host);
        assert_eq!(ffi.bundler_id, "Comp.vue?vue&type=style&index=1&lang.css");
        assert_eq!(ffi.lsp_id, "Comp.vue._VERTER_.style.1.css");
    }

    // ── Output direction: host_remove_to_ffi ─────────────────────────

    #[test]
    fn remove_result_conversion() {
        let remove = host::HostRemoveResult {
            canonical_id: "/src/Old.vue".to_string(),
        };
        let ffi = host_remove_to_ffi(remove);
        assert_eq!(ffi.canonical_id, "/src/Old.vue");
    }

    // ── host_error_to_string: all 4 variants ─────────────────────────

    #[test]
    fn host_error_missing_source() {
        let err = host::HostError::MissingSource {
            canonical_id: "/src/X.vue".to_string(),
        };
        let s = host_error_to_string(&err);
        assert!(s.contains("MissingSource"));
        assert!(s.contains("/src/X.vue"));
    }

    #[test]
    fn host_error_invalid_query() {
        let s = host_error_to_string(&host::HostError::InvalidQuery);
        assert!(s.contains("InvalidQuery"));
    }

    #[test]
    fn host_error_missing_virtual_node() {
        let err = host::HostError::MissingVirtualNode {
            canonical_id: "/src/Y.vue".to_string(),
        };
        let s = host_error_to_string(&err);
        assert!(s.contains("MissingVirtualNode"));
        assert!(s.contains("/src/Y.vue"));
    }

    #[test]
    fn host_error_compile_error_with_diagnostics() {
        let err = host::HostError::CompileError {
            diagnostics: host::DiagnosticsSnapshot {
                diagnostics: vec![
                    host::HostDiagnostic {
                        severity: host::HostSeverity::Error,
                        code: "PARSE_ERR".to_string(),
                        message: "unexpected token".to_string(),
                        span: None,
                    },
                    host::HostDiagnostic {
                        severity: host::HostSeverity::Warning,
                        code: "WARN_01".to_string(),
                        message: "unused import".to_string(),
                        span: None,
                    },
                ],
                has_errors: true,
            },
        };
        let s = host_error_to_string(&err);
        assert!(s.contains("CompileError"));
        assert!(s.contains("[PARSE_ERR] unexpected token"));
        assert!(s.contains("[WARN_01] unused import"));
        // Both diagnostics joined by "; "
        assert!(s.contains("; "));
    }

    // ── FfiConversionError Display: all variants ─────────────────────

    #[test]
    fn ffi_conversion_error_display_all_variants() {
        let cases: Vec<(FfiConversionError, &str)> = vec![
            (
                FfiConversionError::InvalidCompileErrorPolicy("x".to_string()),
                "invalid compileErrorPolicy 'x' (expected 'strict' or 'dev')",
            ),
            (
                FfiConversionError::InvalidAnalysisLevel("y".to_string()),
                "invalid analysisLevel 'y' (expected 'none', 'essential', or 'full')",
            ),
            (
                FfiConversionError::InvalidHmrStrategy("z".to_string()),
                "invalid hmrStrategy 'z' (expected 'vite', 'webpack', or 'none')",
            ),
            (
                FfiConversionError::InvalidDelimiters(5),
                "delimiters must have exactly 2 elements, got 5",
            ),
            (
                FfiConversionError::InvalidFileKind("bin".to_string()),
                "invalid file_kind 'bin'",
            ),
            (
                FfiConversionError::InvalidNodeKind("frag".to_string()),
                "invalid virtual node kind 'frag'",
            ),
        ];
        for (err, expected) in &cases {
            assert_eq!(err.to_string(), *expected);
        }
    }

    // ── byte_offset_to_utf16: FFI boundary edge cases ─────────────────

    /// Empty source: byte_offset 0 → UTF-16 offset 0.
    #[test]
    fn utf16_empty_source() {
        assert_eq!(byte_offset_to_utf16("", 0), 0);
    }

    /// Out-of-bounds offset: an offset beyond the end of the source clamps
    /// to the end, returning the total UTF-16 length rather than panicking.
    #[test]
    fn utf16_out_of_bounds_clamps_to_end() {
        let source = "hello"; // 5 bytes, 5 UTF-16 code units
                              // offset 999 is way past the end
        assert_eq!(byte_offset_to_utf16(source, 999), 5);
        // offset exactly one past the end also clamps
        assert_eq!(byte_offset_to_utf16(source, 6), 5);
    }

    /// Mid-character clamping for a 2-byte UTF-8 sequence (U+00E9, "é").
    ///
    /// "é" encodes as `[0xC3, 0xA9]` (2 bytes).  A byte offset that lands on
    /// the continuation byte (offset 1 inside "é") must clamp backward to the
    /// start of the character (offset 0) rather than producing an invalid
    /// UTF-8 slice.  The resulting UTF-16 offset is 0 (nothing before "é").
    #[test]
    fn utf16_mid_char_2byte_clamps_to_char_start() {
        // "é" = U+00E9, UTF-8: [0xC3, 0xA9] (2 bytes), 1 UTF-16 code unit
        let source = "é"; // byte length == 2
        assert_eq!(source.len(), 2, "sanity: é is 2 bytes");

        // byte offset 0: before "é" → 0 UTF-16 code units
        assert_eq!(byte_offset_to_utf16(source, 0), 0);

        // byte offset 1 falls on the continuation byte → clamps to 0 → 0 UTF-16 CUs
        assert_eq!(
            byte_offset_to_utf16(source, 1),
            0,
            "mid-character offset must clamp to char start"
        );

        // byte offset 2: at/after end → 1 UTF-16 code unit (the full "é")
        assert_eq!(byte_offset_to_utf16(source, 2), 1);
    }

    /// Mid-character clamping for a 4-byte UTF-8 sequence (U+1F600, "😀").
    ///
    /// "😀" encodes as 4 bytes and requires 2 UTF-16 code units (a surrogate
    /// pair).  Any byte offset landing inside the 4-byte sequence must clamp
    /// backward to byte 0 of the character, yielding 0 UTF-16 code units
    /// (nothing before the emoji).
    #[test]
    fn utf16_mid_char_4byte_surrogate_pair_clamps_to_char_start() {
        // "😀" = U+1F600, UTF-8: 4 bytes, UTF-16: 2 code units (surrogate pair)
        let source = "😀";
        assert_eq!(source.len(), 4, "sanity: 😀 is 4 bytes");

        // offsets 1, 2, 3 all land inside the 4-byte sequence
        for mid in 1u32..=3 {
            assert_eq!(
                byte_offset_to_utf16(source, mid),
                0,
                "byte offset {mid} inside 😀 must clamp to 0 UTF-16 CUs"
            );
        }

        // byte offset 4 (past the char) → 2 UTF-16 code units
        assert_eq!(byte_offset_to_utf16(source, 4), 2);
    }

    /// UTF-16 offsets inside a surrogate pair clamp to the scalar start.
    #[test]
    fn utf16_to_byte_offset_clamps_inside_surrogate_pair() {
        let _source = "a\u{1F600}b";
        let source = "aðŸ˜€b";
        let _ = source;
        let source = "a\u{1F600}b";
        assert_eq!(utf16_to_byte_offset(source, 0), 0);
        assert_eq!(utf16_to_byte_offset(source, 1), 1);
        assert_eq!(
            utf16_to_byte_offset(source, 2),
            1,
            "offset inside the emoji surrogate pair should clamp to the emoji start"
        );
        assert_eq!(utf16_to_byte_offset(source, 3), 5);
        assert_eq!(utf16_to_byte_offset(source, 4), 6);
    }

    /// Verify that ASCII text is a 1:1 mapping (byte offset == UTF-16 offset).
    #[test]
    fn utf16_ascii_identity() {
        let source = "hello world";
        for i in 0..=(source.len() as u32) {
            assert_eq!(
                byte_offset_to_utf16(source, i),
                i,
                "ASCII byte offset {i} should equal its UTF-16 offset"
            );
        }
    }

    /// Mixed ASCII + multibyte: offset after a 2-byte char produces the
    /// correct UTF-16 value (prior ASCII chars + 1 CU for the 2-byte char).
    #[test]
    fn utf16_mixed_ascii_and_multibyte() {
        // "aé" = 'a' (1 byte) + 'é' (2 bytes) = 3 bytes total, 2 UTF-16 CUs
        let source = "aé";
        assert_eq!(source.len(), 3);

        assert_eq!(byte_offset_to_utf16(source, 0), 0); // before 'a'
        assert_eq!(byte_offset_to_utf16(source, 1), 1); // after 'a', before 'é'
                                                        // byte offset 2 is the continuation byte of 'é' → clamps to byte 1 → 1 UTF-16 CU
        assert_eq!(
            byte_offset_to_utf16(source, 2),
            1,
            "continuation byte of é clamps to its char start"
        );
        assert_eq!(byte_offset_to_utf16(source, 3), 2); // after 'é'
    }

    // ── Offset encoding conversion tests ────────────────────────

    #[test]
    fn utf8_to_utf16_ascii_identity() {
        assert_eq!(utf8_to_utf16_offset("hello world", 5), 5);
    }

    #[test]
    fn utf8_to_utf16_cjk() {
        // "日本" = 2 CJK chars, 3 bytes each = 6 bytes
        let text = "日本abc";
        assert_eq!(utf8_to_utf16_offset(text, 0), 0);
        assert_eq!(utf8_to_utf16_offset(text, 3), 1); // after first CJK
        assert_eq!(utf8_to_utf16_offset(text, 6), 2); // after second CJK
        assert_eq!(utf8_to_utf16_offset(text, 7), 3); // after 'a'
    }

    #[test]
    fn utf8_to_utf16_emoji_surrogate() {
        // 😀 = 4 bytes UTF-8, 2 code units UTF-16
        let text = "a😀b";
        assert_eq!(utf8_to_utf16_offset(text, 0), 0);
        assert_eq!(utf8_to_utf16_offset(text, 1), 1); // after 'a'
        assert_eq!(utf8_to_utf16_offset(text, 5), 3); // after emoji (1+2)
        assert_eq!(utf8_to_utf16_offset(text, 6), 4); // after 'b'
    }

    #[test]
    fn convert_offset_utf8_passthrough() {
        assert_eq!(convert_offset("hello", 3, OffsetEncoding::Utf8), 3);
    }

    #[test]
    fn utf8_to_utf32_basic() {
        let text = "a😀b";
        assert_eq!(utf8_to_utf32_offset(text, 0), 0);
        assert_eq!(utf8_to_utf32_offset(text, 1), 1); // after 'a'
        assert_eq!(utf8_to_utf32_offset(text, 5), 2); // after emoji (1 codepoint)
        assert_eq!(utf8_to_utf32_offset(text, 6), 3); // after 'b'
    }

    // ── E1 origin graph tests ────────────────────────────────────────

    #[test]
    fn ffi_payload_contains_origin_field_when_resolved_state_has_origin_graph() {
        use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

        let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
            snapshot: host::FileAnalysisSnapshot::default(),
            mode: host::ProjectionMode::Expanded,
            whole_hash: [0; 16],
            resolved_macros: Vec::new(),
            resolved_type_registry: Vec::new(),
            resolved_type_registry_meta: Vec::new(),
            evaluated_types: None,
            fact_versions: Vec::new(),
            compute_audit: None,
            origin_graph: Some(OriginGraphDto {
                nodes: vec![
                    OriginNodeDto {
                        id: 0,
                        kind: "Object".to_string(),
                        label: None,
                    },
                    OriginNodeDto {
                        id: 1,
                        kind: "Primitive".to_string(),
                        label: None,
                    },
                ],
                edges: vec![OriginEdgeDto {
                    source: 1,
                    target: 0,
                    kind: "instantiate".to_string(),
                    meta_index: None,
                }],
                meta_strings: Vec::new(),
            }),
            request_id: 0,
            surface_identities: None,
        };

        let ffi =
            component_meta_analysis_to_ffi_with_resolution(empty_analysis(), Some(&resolved_state));
        assert!(
            !ffi.origin.edges.is_empty(),
            "FfiComponentMeta.origin must contain edges when resolved state has origin graph"
        );
        assert_eq!(ffi.origin.edges[0].kind, "instantiate");
        assert_eq!(ffi.origin.nodes.len(), 2);
    }

    #[test]
    fn ffi_origin_subgraph_is_empty_when_resolved_state_has_no_origin_graph() {
        let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
            snapshot: host::FileAnalysisSnapshot::default(),
            mode: host::ProjectionMode::Expanded,
            whole_hash: [0; 16],
            resolved_macros: Vec::new(),
            resolved_type_registry: Vec::new(),
            resolved_type_registry_meta: Vec::new(),
            evaluated_types: None,
            fact_versions: Vec::new(),
            compute_audit: None,
            origin_graph: None,
            request_id: 0,
            surface_identities: None,
        };

        let ffi =
            component_meta_analysis_to_ffi_with_resolution(empty_analysis(), Some(&resolved_state));
        assert!(
            ffi.origin.edges.is_empty(),
            "FfiComponentMeta.origin must be empty when no origin graph"
        );
    }

    #[test]
    fn ffi_edge_meta_strings_deduplicated() {
        use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

        let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
            snapshot: host::FileAnalysisSnapshot::default(),
            mode: host::ProjectionMode::Expanded,
            whole_hash: [0; 16],
            resolved_macros: Vec::new(),
            resolved_type_registry: Vec::new(),
            resolved_type_registry_meta: Vec::new(),
            evaluated_types: None,
            fact_versions: Vec::new(),
            compute_audit: None,
            origin_graph: Some(OriginGraphDto {
                nodes: vec![
                    OriginNodeDto {
                        id: 0,
                        kind: "Object".to_string(),
                        label: None,
                    },
                    OriginNodeDto {
                        id: 1,
                        kind: "Primitive".to_string(),
                        label: None,
                    },
                ],
                edges: vec![
                    OriginEdgeDto {
                        source: 0,
                        target: 1,
                        kind: "substituteTypeParam".to_string(),
                        meta_index: Some(0),
                    },
                    OriginEdgeDto {
                        source: 1,
                        target: 0,
                        kind: "substituteTypeParam".to_string(),
                        meta_index: Some(0),
                    },
                ],
                meta_strings: vec!["SubstitutedParam(\"T\")".to_string()],
            }),
            request_id: 0,
            surface_identities: None,
        };

        let ffi =
            component_meta_analysis_to_ffi_with_resolution(empty_analysis(), Some(&resolved_state));
        assert_eq!(
            ffi.origin.meta_strings.len(),
            1,
            "meta strings must be deduplicated"
        );
        assert_eq!(ffi.origin.edges.len(), 2);
        assert_eq!(
            ffi.origin.edges[0].meta_index, ffi.origin.edges[1].meta_index,
            "both edges reference the same meta string"
        );
    }

    #[test]
    fn ffi_projection_mode_wire_format() {
        let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
            snapshot: host::FileAnalysisSnapshot::default(),
            mode: host::ProjectionMode::Expanded,
            whole_hash: [0; 16],
            resolved_macros: Vec::new(),
            resolved_type_registry: Vec::new(),
            resolved_type_registry_meta: Vec::new(),
            evaluated_types: None,
            fact_versions: Vec::new(),
            compute_audit: None,
            origin_graph: None,
            request_id: 0,
            surface_identities: None,
        };

        let ffi = resolved_component_meta_to_ffi(&resolved_state);
        assert_eq!(
            ffi.mode, "expanded",
            "ProjectionMode::Expanded wire format must be 'expanded'"
        );
    }

    #[test]
    fn ffi_payload_contains_instantiate_edge_for_generic_component() {
        use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

        let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
            snapshot: host::FileAnalysisSnapshot::default(),
            mode: host::ProjectionMode::Expanded,
            whole_hash: [0; 16],
            resolved_macros: Vec::new(),
            resolved_type_registry: Vec::new(),
            resolved_type_registry_meta: Vec::new(),
            evaluated_types: None,
            fact_versions: Vec::new(),
            compute_audit: None,
            origin_graph: Some(OriginGraphDto {
                nodes: vec![
                    OriginNodeDto {
                        id: 0,
                        kind: "Object".to_string(),
                        label: Some("{...}".to_string()),
                    },
                    OriginNodeDto {
                        id: 1,
                        kind: "Primitive".to_string(),
                        label: Some("string".to_string()),
                    },
                    OriginNodeDto {
                        id: 2,
                        kind: "TypeParam".to_string(),
                        label: Some("T".to_string()),
                    },
                ],
                edges: vec![
                    OriginEdgeDto {
                        source: 1,
                        target: 0,
                        kind: "instantiate".to_string(),
                        meta_index: None,
                    },
                    OriginEdgeDto {
                        source: 2,
                        target: 0,
                        kind: "substituteTypeParam".to_string(),
                        meta_index: Some(0),
                    },
                ],
                meta_strings: vec!["SubstitutedParam(\"T\")".to_string()],
            }),
            request_id: 0,
            surface_identities: None,
        };

        let ffi =
            component_meta_analysis_to_ffi_with_resolution(empty_analysis(), Some(&resolved_state));

        assert_eq!(ffi.origin.nodes.len(), 3, "all 3 origin nodes survive FFI");
        assert_eq!(ffi.origin.edges.len(), 2, "both origin edges survive FFI");

        let has_instantiate = ffi.origin.edges.iter().any(|e| e.kind == "instantiate");
        let has_substitute = ffi
            .origin
            .edges
            .iter()
            .any(|e| e.kind == "substituteTypeParam");
        assert!(has_instantiate, "instantiate edge must survive FFI");
        assert!(has_substitute, "substituteTypeParam edge must survive FFI");

        let type_param_node = ffi.origin.nodes.iter().find(|n| n.kind == "TypeParam");
        assert!(type_param_node.is_some(), "TypeParam node must survive FFI");
        assert_eq!(
            type_param_node.unwrap().label.as_deref(),
            Some("T"),
            "TypeParam label must survive FFI"
        );

        assert_eq!(ffi.origin.meta_strings.len(), 1, "meta strings survive FFI");
        assert_eq!(
            ffi.origin.meta_strings[0], "SubstitutedParam(\"T\")",
            "meta string content survives FFI"
        );

        let proto_bytes = verter_protocol::component_meta::encode_component_meta_payload(&ffi);
        assert!(
            !proto_bytes.is_empty(),
            "proto encoding of origin graph must produce non-empty bytes"
        );
    }
}
