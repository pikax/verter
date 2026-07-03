//! Canonical-JSON published-surface envelopes for the dual-leg parity
//! oracle.
//!
//! An [`OracleEnvelope`] carries an outcome tag plus the canonical JSON
//! of the FULL published DTO. Canonicalisation: object keys are written
//! in sorted order at every level; array order is preserved (array order
//! is public semantics on these surfaces). The serialisers destructure
//! every non-serde analysis DTO EXHAUSTIVELY (no `..`), so a new field
//! fails compilation here until it is added to the envelope. Serde-backed
//! leaves (`TypeExpr`, `JsdocTag`, `ExpansionMetadata`, the classifier
//! enums) serialise through their existing `Serialize` impls.
//!
//! One documented normalisation: `FallthroughResolution.fact_versions`
//! is cache-validity metadata (the observed fact rail), not a published
//! surface — the two legs legitimately record different validity facts
//! for identical published answers, so the envelope replaces it with its
//! length only.

use serde_json::{json, Value};
use verter_semantic::analysis::component_meta::{
    AcceptedEventAnalysis, AcceptedEventKind, AcceptedPropAnalysis, AcceptedPropKind,
    AcceptedSurfaceCompleteness, BindingAnalysis, BindingKindAnalysis, BranchStatus,
    ComponentBindingUsageAnalysis, ComponentEventUsageAnalysis, ComponentMetaAnalysis,
    ComponentMetaFlags, ComponentPropUsageAnalysis, ComponentUsageAnalysis,
    ComponentVModelUsageAnalysis, ConsumedRootBindings, CustomBlockAnalysis, EventAnalysis,
    ExposedAnalysis, FallthroughBranch, FallthroughEventEntry, FallthroughPropEntry,
    FallthroughSurface, GenericResolutionFailure, ImportAnalysis, ImportBindingAnalysis,
    InheritedSource, MacroExpansionDiagnostics, MacroExpansionKind, MemberAvailability,
    MemberProvenance, ModelAnalysis, NoFallthroughReason, PartialBranchReason, PropAnalysis,
    PublicInstanceAnalysis, PublicInstanceCompleteness, PublicInstanceMemberAnalysis,
    PublicInstanceMemberKind, ResolvedTypeAnalysis, RootBranch, RootReachability, RootTargetRef,
    ScriptBlockAnalysis, SelectorAnalysis, SfcAttributeAnalysis, SfcBlocksAnalysis, SlotAnalysis,
    SlotBindingAnalysis, StyleAnalysis, StyleBlockInfoAnalysis, TemplateBlockAnalysis,
    TemplateRefAnalysis, UnresolvedBranchReason, UnresolvedRootTargetReason, VueApiCallAnalysis,
};

use crate::types::FallthroughResolution;

/// Outcome status plus canonical JSON bytes of the full published DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OracleEnvelope {
    /// Outcome tag: `"some"` / `"none"` for optional surfaces.
    pub(crate) outcome: String,
    /// Canonical JSON (sorted object keys, preserved array order).
    pub(crate) canonical_json: String,
}

impl OracleEnvelope {
    fn from_value(outcome: &str, value: Value) -> Self {
        let mut out = String::new();
        write_canonical(&value, &mut out);
        Self {
            outcome: outcome.to_string(),
            canonical_json: out,
        }
    }
}

/// Write `value` as canonical JSON: object keys sorted, arrays in order.
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&serde_json::to_string(s).expect("string serialises")),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("key serialises"));
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
    }
}

/// Canonicalisation entry for the runner's determinism test.
pub(crate) fn write_canonical_for_test(value: &Value, out: &mut String) {
    write_canonical(value, out);
}

/// Serde-backed leaf → `Value`.
fn ser<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serde leaf serialises")
}

/// Envelope over an optional native component-meta payload.
pub(crate) fn component_meta_envelope(meta: Option<&ComponentMetaAnalysis>) -> OracleEnvelope {
    match meta {
        None => OracleEnvelope::from_value("none", Value::Null),
        Some(meta) => OracleEnvelope::from_value("some", component_meta_value(meta)),
    }
}

/// Envelope over an optional fallthrough resolution.
pub(crate) fn fallthrough_envelope(res: Option<&FallthroughResolution>) -> OracleEnvelope {
    match res {
        None => OracleEnvelope::from_value("none", Value::Null),
        Some(res) => {
            let FallthroughResolution {
                accepted_props,
                accepted_events,
                accepted_surface_completeness,
                fallthrough_surface,
                fact_versions,
            } = res;
            let value = json!({
                "accepted_props": accepted_props.iter().map(accepted_prop_value).collect::<Vec<_>>(),
                "accepted_events": accepted_events.iter().map(accepted_event_value).collect::<Vec<_>>(),
                "accepted_surface_completeness": accepted_surface_completeness_value(accepted_surface_completeness),
                "fallthrough_surface": fallthrough_surface_value(fallthrough_surface),
                // Documented normalisation: the observed validity-fact rail is
                // not a published surface; only its presence shape is pinned.
                "fact_versions_len": fact_versions.len(),
            });
            OracleEnvelope::from_value("some", value)
        }
    }
}

fn component_meta_value(meta: &ComponentMetaAnalysis) -> Value {
    let ComponentMetaAnalysis {
        props,
        events,
        slots,
        models,
        exposed,
        public_instance,
        sfc_blocks,
        type_registry,
        components,
        template_refs,
        imports,
        bindings,
        vue_api_calls,
        styles,
        flags,
        root_reachability,
        accepted_props,
        accepted_events,
        accepted_surface_completeness,
        fallthrough_surface,
        macro_expansion_diagnostics,
        options_api,
        file_path,
    } = meta;
    json!({
        "props": props.iter().map(prop_value).collect::<Vec<_>>(),
        "events": events.iter().map(event_value).collect::<Vec<_>>(),
        "slots": slots.iter().map(slot_value).collect::<Vec<_>>(),
        "models": models.iter().map(model_value).collect::<Vec<_>>(),
        "exposed": exposed.iter().map(exposed_value).collect::<Vec<_>>(),
        "public_instance": public_instance.as_ref().map(public_instance_value),
        "sfc_blocks": sfc_blocks.as_ref().map(sfc_blocks_value),
        "type_registry": type_registry.iter().map(resolved_type_value).collect::<Vec<_>>(),
        "components": components.iter().map(component_usage_value).collect::<Vec<_>>(),
        "template_refs": template_refs.iter().map(template_ref_value).collect::<Vec<_>>(),
        "imports": imports.iter().map(import_value).collect::<Vec<_>>(),
        "bindings": bindings.iter().map(binding_value).collect::<Vec<_>>(),
        "vue_api_calls": vue_api_calls.iter().map(vue_api_call_value).collect::<Vec<_>>(),
        "styles": styles.iter().map(style_value).collect::<Vec<_>>(),
        "flags": flags_value(flags),
        "root_reachability": root_reachability_value(root_reachability),
        "accepted_props": accepted_props.iter().map(accepted_prop_value).collect::<Vec<_>>(),
        "accepted_events": accepted_events.iter().map(accepted_event_value).collect::<Vec<_>>(),
        "accepted_surface_completeness": accepted_surface_completeness_value(accepted_surface_completeness),
        "fallthrough_surface": fallthrough_surface_value(fallthrough_surface),
        "macro_expansion_diagnostics": macro_expansion_diagnostics
            .iter()
            .map(macro_expansion_diagnostics_value)
            .collect::<Vec<_>>(),
        "options_api": options_api,
        "file_path": file_path,
    })
}

fn prop_value(prop: &PropAnalysis) -> Value {
    let PropAnalysis {
        name,
        type_expr,
        type_expansion,
        raw_type,
        raw_type_expr,
        required,
        has_default,
        default_value,
        description,
        tags,
        declared_in_macro_type_arg,
    } = prop;
    json!({
        "name": name,
        "type_expr": ser(type_expr),
        "type_expansion": type_expansion.as_ref().map(ser),
        "raw_type": raw_type,
        "raw_type_expr": raw_type_expr.as_ref().map(ser),
        "required": required,
        "has_default": has_default,
        "default_value": default_value,
        "description": description,
        "tags": tags.iter().map(ser).collect::<Vec<_>>(),
        "declared_in_macro_type_arg": declared_in_macro_type_arg,
    })
}

fn event_value(event: &EventAnalysis) -> Value {
    let EventAnalysis {
        name,
        payload,
        payload_expansion,
        raw_signature,
        description,
        tags,
    } = event;
    json!({
        "name": name,
        "payload": ser(payload),
        "payload_expansion": payload_expansion.as_ref().map(ser),
        "raw_signature": raw_signature,
        "description": description,
        "tags": tags.iter().map(ser).collect::<Vec<_>>(),
    })
}

fn slot_value(slot: &SlotAnalysis) -> Value {
    let SlotAnalysis {
        name,
        is_scoped,
        bindings,
        is_required,
        return_type,
        return_expr,
        return_expr_scope,
        description,
        tags,
    } = slot;
    json!({
        "name": name,
        "is_scoped": is_scoped,
        "bindings": bindings.iter().map(slot_binding_value).collect::<Vec<_>>(),
        "is_required": is_required,
        "return_type": return_type,
        "return_expr": return_expr.as_ref().map(ser),
        "return_expr_scope": return_expr_scope.as_ref().map(ser),
        "description": description,
        "tags": tags.iter().map(ser).collect::<Vec<_>>(),
    })
}

fn slot_binding_value(binding: &SlotBindingAnalysis) -> Value {
    let SlotBindingAnalysis {
        name,
        type_expr,
        type_expansion,
        raw_type,
        raw_type_expr,
    } = binding;
    json!({
        "name": name,
        "type_expr": ser(type_expr),
        "type_expansion": type_expansion.as_ref().map(ser),
        "raw_type": raw_type,
        "raw_type_expr": raw_type_expr.as_ref().map(ser),
    })
}

fn model_value(model: &ModelAnalysis) -> Value {
    let ModelAnalysis { name, type_expr } = model;
    json!({ "name": name, "type_expr": ser(type_expr) })
}

fn exposed_value(exposed: &ExposedAnalysis) -> Value {
    let ExposedAnalysis {
        name,
        type_expr,
        type_expansion,
        description,
        tags,
    } = exposed;
    json!({
        "name": name,
        "type_expr": ser(type_expr),
        "type_expansion": type_expansion.as_ref().map(ser),
        "description": description,
        "tags": tags.iter().map(ser).collect::<Vec<_>>(),
    })
}

fn public_instance_value(pi: &PublicInstanceAnalysis) -> Value {
    let PublicInstanceAnalysis {
        members,
        completeness,
    } = pi;
    json!({
        "members": members.iter().map(public_instance_member_value).collect::<Vec<_>>(),
        "completeness": match completeness {
            PublicInstanceCompleteness::Exact => "exact",
            PublicInstanceCompleteness::Partial => "partial",
        },
    })
}

fn public_instance_member_value(member: &PublicInstanceMemberAnalysis) -> Value {
    let PublicInstanceMemberAnalysis {
        name,
        kind,
        type_expr,
        type_expansion,
        raw_type,
        description,
        tags,
    } = member;
    json!({
        "name": name,
        "kind": match kind {
            PublicInstanceMemberKind::Prop => "prop",
            PublicInstanceMemberKind::SlotContainer => "slot_container",
            PublicInstanceMemberKind::Exposed => "exposed",
        },
        "type_expr": ser(type_expr),
        "type_expansion": type_expansion.as_ref().map(ser),
        "raw_type": raw_type,
        "description": description,
        "tags": tags.iter().map(ser).collect::<Vec<_>>(),
    })
}

fn sfc_attribute_value(attr: &SfcAttributeAnalysis) -> Value {
    let SfcAttributeAnalysis { name, value } = attr;
    json!({ "name": name, "value": value })
}

fn sfc_blocks_value(blocks: &SfcBlocksAnalysis) -> Value {
    let SfcBlocksAnalysis {
        template,
        script,
        script_setup,
        styles,
        custom,
    } = blocks;
    json!({
        "template": template.as_ref().map(|t| {
            let TemplateBlockAnalysis { lang, src, attributes } = t;
            json!({
                "lang": lang,
                "src": src,
                "attributes": attributes.iter().map(sfc_attribute_value).collect::<Vec<_>>(),
            })
        }),
        "script": script.as_ref().map(script_block_value),
        "script_setup": script_setup.as_ref().map(script_block_value),
        "styles": styles.iter().map(style_block_info_value).collect::<Vec<_>>(),
        "custom": custom.iter().map(custom_block_value).collect::<Vec<_>>(),
    })
}

fn script_block_value(block: &ScriptBlockAnalysis) -> Value {
    let ScriptBlockAnalysis {
        lang,
        src,
        generic,
        attrs_type,
        attributes,
    } = block;
    json!({
        "lang": lang,
        "src": src,
        "generic": generic,
        "attrs_type": attrs_type,
        "attributes": attributes.iter().map(sfc_attribute_value).collect::<Vec<_>>(),
    })
}

fn style_block_info_value(block: &StyleBlockInfoAnalysis) -> Value {
    let StyleBlockInfoAnalysis {
        index,
        lang,
        src,
        scoped,
        is_module,
        module_name,
        attributes,
    } = block;
    json!({
        "index": index,
        "lang": lang,
        "src": src,
        "scoped": scoped,
        "is_module": is_module,
        "module_name": module_name,
        "attributes": attributes.iter().map(sfc_attribute_value).collect::<Vec<_>>(),
    })
}

fn custom_block_value(block: &CustomBlockAnalysis) -> Value {
    let CustomBlockAnalysis {
        index,
        block_type,
        lang,
        src,
        attributes,
    } = block;
    json!({
        "index": index,
        "block_type": block_type,
        "lang": lang,
        "src": src,
        "attributes": attributes.iter().map(sfc_attribute_value).collect::<Vec<_>>(),
    })
}

fn resolved_type_value(rt: &ResolvedTypeAnalysis) -> Value {
    let ResolvedTypeAnalysis {
        name,
        type_expr,
        type_expansion,
    } = rt;
    json!({
        "name": name,
        "type_expr": ser(type_expr),
        "type_expansion": type_expansion.as_ref().map(ser),
    })
}

fn component_usage_value(usage: &ComponentUsageAnalysis) -> Value {
    let ComponentUsageAnalysis {
        name,
        import_source,
        is_dynamic,
        props,
        has_spread,
        slots_used,
        static_classes,
        has_dynamic_class,
        v_models,
        v_model_entries,
        bindings,
        events,
    } = usage;
    json!({
        "name": name,
        "import_source": import_source,
        "is_dynamic": is_dynamic,
        "props": props.iter().map(component_prop_usage_value).collect::<Vec<_>>(),
        "has_spread": has_spread,
        "slots_used": slots_used,
        "static_classes": static_classes,
        "has_dynamic_class": has_dynamic_class,
        "v_models": v_models,
        "v_model_entries": v_model_entries.iter().map(|e| {
            let ComponentVModelUsageAnalysis { binding_name } = e;
            json!({ "binding_name": binding_name })
        }).collect::<Vec<_>>(),
        "bindings": bindings.iter().map(|b| {
            let ComponentBindingUsageAnalysis { name, modifiers } = b;
            json!({ "name": name, "modifiers": modifiers })
        }).collect::<Vec<_>>(),
        "events": events.iter().map(|e| {
            let ComponentEventUsageAnalysis { name, handler_expression, is_inline, modifiers } = e;
            json!({
                "name": name,
                "handler_expression": handler_expression,
                "is_inline": is_inline,
                "modifiers": modifiers,
            })
        }).collect::<Vec<_>>(),
    })
}

fn component_prop_usage_value(prop: &ComponentPropUsageAnalysis) -> Value {
    let ComponentPropUsageAnalysis {
        name,
        is_bound,
        constness,
        expression,
        referenced_bindings,
        from_spread,
        is_shorthand,
    } = prop;
    json!({
        "name": name,
        "is_bound": is_bound,
        "constness": ser(constness),
        "expression": expression,
        "referenced_bindings": referenced_bindings,
        "from_spread": from_spread,
        "is_shorthand": is_shorthand,
    })
}

fn template_ref_value(tr: &TemplateRefAnalysis) -> Value {
    let TemplateRefAnalysis {
        name,
        is_dynamic,
        target_tag,
    } = tr;
    json!({ "name": name, "is_dynamic": is_dynamic, "target_tag": target_tag })
}

fn import_value(import: &ImportAnalysis) -> Value {
    let ImportAnalysis {
        source,
        is_type_only,
        bindings,
    } = import;
    json!({
        "source": source,
        "is_type_only": is_type_only,
        "bindings": bindings.iter().map(|b| {
            let ImportBindingAnalysis { name, kind, imported_name, is_type_only } = b;
            json!({
                "name": name,
                "kind": ser(kind),
                "imported_name": imported_name,
                "is_type_only": is_type_only,
            })
        }).collect::<Vec<_>>(),
    })
}

fn binding_value(binding: &BindingAnalysis) -> Value {
    let BindingAnalysis {
        name,
        kind,
        reactivity_kind,
        type_annotation,
        used_in_template,
        used_in_style,
    } = binding;
    json!({
        "name": name,
        "kind": match kind {
            BindingKindAnalysis::Const => "const",
            BindingKindAnalysis::Let => "let",
            BindingKindAnalysis::Var => "var",
            BindingKindAnalysis::Function => "function",
            BindingKindAnalysis::AsyncFunction => "async_function",
            BindingKindAnalysis::Class => "class",
        },
        "reactivity_kind": ser(reactivity_kind),
        "type_annotation": type_annotation,
        "used_in_template": used_in_template,
        "used_in_style": used_in_style,
    })
}

fn vue_api_call_value(call: &VueApiCallAnalysis) -> Value {
    let VueApiCallAnalysis { api, arg_value } = call;
    json!({ "api": ser(api), "arg_value": arg_value })
}

fn style_value(style: &StyleAnalysis) -> Value {
    let StyleAnalysis {
        lang,
        scoped,
        is_module,
        module_name,
        classes,
        ids,
        custom_properties,
        v_binds,
        selectors,
    } = style;
    json!({
        "lang": ser(lang),
        "scoped": scoped,
        "is_module": is_module,
        "module_name": module_name,
        "classes": classes,
        "ids": ids,
        "custom_properties": custom_properties,
        "v_binds": v_binds,
        "selectors": selectors.iter().map(|s| {
            let SelectorAnalysis { text, specificity } = s;
            json!({ "text": text, "specificity": [specificity.0, specificity.1, specificity.2] })
        }).collect::<Vec<_>>(),
    })
}

fn flags_value(flags: &ComponentMetaFlags) -> Value {
    let ComponentMetaFlags {
        async_setup,
        has_reactive_state,
        has_computed,
        has_watchers,
        has_lifecycle_hooks,
        has_provide,
        has_inject,
        has_inherit_attrs_false,
        has_store_usage,
        has_macro_failure,
    } = flags;
    json!({
        "async_setup": async_setup,
        "has_reactive_state": has_reactive_state,
        "has_computed": has_computed,
        "has_watchers": has_watchers,
        "has_lifecycle_hooks": has_lifecycle_hooks,
        "has_provide": has_provide,
        "has_inject": has_inject,
        "has_inherit_attrs_false": has_inherit_attrs_false,
        "has_store_usage": has_store_usage,
        "has_macro_failure": has_macro_failure,
    })
}

fn no_fallthrough_reason_value(reason: &NoFallthroughReason) -> Value {
    Value::String(
        match reason {
            NoFallthroughReason::InheritAttrsFalse => "inherit_attrs_false",
            NoFallthroughReason::MultiRoot => "multi_root",
            NoFallthroughReason::BranchNotSingleRoot => "branch_not_single_root",
            NoFallthroughReason::RootVFor => "root_v_for",
            NoFallthroughReason::NoTemplate => "no_template",
            NoFallthroughReason::EmptyTemplate => "empty_template",
            NoFallthroughReason::TextOrInterpolationRoot => "text_or_interpolation_root",
        }
        .to_string(),
    )
}

fn root_reachability_value(rr: &RootReachability) -> Value {
    match rr {
        RootReachability::NoFallthrough { reason } => json!({
            "no_fallthrough": { "reason": no_fallthrough_reason_value(reason) },
        }),
        RootReachability::Branches { branches } => json!({
            "branches": branches.iter().map(root_branch_value).collect::<Vec<_>>(),
        }),
    }
}

fn root_branch_value(branch: &RootBranch) -> Value {
    let RootBranch {
        branch_index,
        condition_text,
        target,
        consumed,
        has_unknown_spread,
    } = branch;
    json!({
        "branch_index": branch_index,
        "condition_text": condition_text,
        "target": root_target_value(target),
        "consumed": consumed_root_bindings_value(consumed),
        "has_unknown_spread": has_unknown_spread,
    })
}

fn unresolved_root_target_reason_value(reason: &UnresolvedRootTargetReason) -> Value {
    match reason {
        UnresolvedRootTargetReason::DynamicComponentIs => json!("dynamic_component_is"),
        UnresolvedRootTargetReason::SlotOutlet => json!("slot_outlet"),
        UnresolvedRootTargetReason::UnsupportedBuiltin { tag } => {
            json!({ "unsupported_builtin": { "tag": tag } })
        }
        UnresolvedRootTargetReason::MissingUsageLink => json!("missing_usage_link"),
        UnresolvedRootTargetReason::UnresolvedImport => json!("unresolved_import"),
        UnresolvedRootTargetReason::UnknownRootTarget => json!("unknown_root_target"),
    }
}

fn root_target_value(target: &RootTargetRef) -> Value {
    match target {
        RootTargetRef::NativeElement { element_index, tag } => json!({
            "native_element": { "element_index": element_index, "tag": tag },
        }),
        RootTargetRef::DynamicComponentUsage {
            element_index,
            usage_index,
        } => json!({
            "dynamic_component_usage": {
                "element_index": element_index,
                "usage_index": usage_index,
            },
        }),
        RootTargetRef::ComponentUsage {
            element_index,
            usage_index,
            name,
            import_source,
        } => json!({
            "component_usage": {
                "element_index": element_index,
                "usage_index": usage_index,
                "name": name,
                "import_source": import_source,
            },
        }),
        RootTargetRef::UnresolvedTarget {
            element_index,
            tag,
            reason,
        } => json!({
            "unresolved_target": {
                "element_index": element_index,
                "tag": tag,
                "reason": unresolved_root_target_reason_value(reason),
            },
        }),
    }
}

fn consumed_root_bindings_value(consumed: &ConsumedRootBindings) -> Value {
    let ConsumedRootBindings {
        attrs,
        listeners,
        has_dynamic_attr_name,
        has_dynamic_listener_name,
    } = consumed;
    json!({
        "attrs": attrs,
        "listeners": listeners,
        "has_dynamic_attr_name": has_dynamic_attr_name,
        "has_dynamic_listener_name": has_dynamic_listener_name,
    })
}

fn inherited_source_value(source: &InheritedSource) -> Value {
    match source {
        InheritedSource::NativeTag { tag } => json!({ "native_tag": { "tag": tag } }),
        InheritedSource::Component { canonical_id } => {
            json!({ "component": { "canonical_id": canonical_id } })
        }
    }
}

fn member_provenance_value(provenance: &MemberProvenance) -> Value {
    match provenance {
        MemberProvenance::Declared => json!("declared"),
        MemberProvenance::Inherited { sources } => json!({
            "inherited": {
                "sources": sources.iter().map(inherited_source_value).collect::<Vec<_>>(),
            },
        }),
    }
}

fn member_availability_value(availability: &MemberAvailability) -> Value {
    match availability {
        MemberAvailability::Always => json!("always"),
        MemberAvailability::Conditional { branch_keys } => json!({
            "conditional": { "branch_keys": branch_keys },
        }),
    }
}

fn accepted_prop_value(prop: &AcceptedPropAnalysis) -> Value {
    let AcceptedPropAnalysis {
        name,
        type_expr,
        raw_type,
        raw_type_expr,
        required,
        provenance,
        availability,
        kind,
    } = prop;
    json!({
        "name": name,
        "type_expr": ser(type_expr),
        "raw_type": raw_type,
        "raw_type_expr": raw_type_expr.as_ref().map(ser),
        "required": required,
        "provenance": member_provenance_value(provenance),
        "availability": member_availability_value(availability),
        "kind": match kind {
            AcceptedPropKind::DeclaredProp => "declared_prop",
            AcceptedPropKind::Attr => "attr",
        },
    })
}

fn accepted_event_value(event: &AcceptedEventAnalysis) -> Value {
    let AcceptedEventAnalysis {
        name,
        payload,
        raw_signature,
        provenance,
        availability,
        kind,
    } = event;
    json!({
        "name": name,
        "payload": ser(payload),
        "raw_signature": raw_signature,
        "provenance": member_provenance_value(provenance),
        "availability": member_availability_value(availability),
        "kind": match kind {
            AcceptedEventKind::DeclaredEmit => "declared_emit",
            AcceptedEventKind::Listener => "listener",
        },
    })
}

fn accepted_surface_completeness_value(completeness: &AcceptedSurfaceCompleteness) -> Value {
    match completeness {
        AcceptedSurfaceCompleteness::Exact => json!("exact"),
        AcceptedSurfaceCompleteness::LowerBound => json!("lower_bound"),
    }
}

fn generic_resolution_failure_value(failure: &GenericResolutionFailure) -> Value {
    json!(match failure {
        GenericResolutionFailure::SpreadInput => "spread_input",
        GenericResolutionFailure::DynamicKey => "dynamic_key",
        GenericResolutionFailure::MissingType => "missing_type",
        GenericResolutionFailure::UnsupportedExpression => "unsupported_expression",
        GenericResolutionFailure::MissingUsageLink => "missing_usage_link",
        GenericResolutionFailure::UnresolvedChildGenericSurface =>
            "unresolved_child_generic_surface",
    })
}

fn partial_branch_reason_value(reason: &PartialBranchReason) -> Value {
    match reason {
        PartialBranchReason::DynamicAttrName => json!("dynamic_attr_name"),
        PartialBranchReason::DynamicListenerName => json!("dynamic_listener_name"),
        PartialBranchReason::UnknownSpread => json!("unknown_spread"),
        PartialBranchReason::GenericResolution { failure } => json!({
            "generic_resolution": { "failure": generic_resolution_failure_value(failure) },
        }),
    }
}

fn unresolved_branch_reason_value(reason: &UnresolvedBranchReason) -> Value {
    match reason {
        UnresolvedBranchReason::Cycle { canonical_id } => {
            json!({ "cycle": { "canonical_id": canonical_id } })
        }
        UnresolvedBranchReason::DynamicComponentIs => json!("dynamic_component_is"),
        UnresolvedBranchReason::ChildResolutionFailed => json!("child_resolution_failed"),
        UnresolvedBranchReason::UnresolvedChildImport { import_source } => json!({
            "unresolved_child_import": { "import_source": import_source },
        }),
        UnresolvedBranchReason::RootTarget { reason } => json!({
            "root_target": { "reason": unresolved_root_target_reason_value(reason) },
        }),
        UnresolvedBranchReason::GenericResolution { failure } => json!({
            "generic_resolution": { "failure": generic_resolution_failure_value(failure) },
        }),
    }
}

fn resolved_root_step_value(
    step: &verter_semantic::analysis::component_meta::ResolvedRootStep,
) -> Value {
    use verter_semantic::analysis::component_meta::ResolvedRootStep;
    match step {
        ResolvedRootStep::NativeTag { tag } => json!({ "native_tag": { "tag": tag } }),
        ResolvedRootStep::Component {
            canonical_id,
            component_name,
        } => json!({
            "component": {
                "canonical_id": canonical_id,
                "component_name": component_name,
            },
        }),
        ResolvedRootStep::Unresolved { tag, reason } => json!({
            "unresolved": {
                "tag": tag,
                "reason": unresolved_branch_reason_value(reason),
            },
        }),
    }
}

fn branch_status_value(status: &BranchStatus) -> Value {
    match status {
        BranchStatus::Resolved => json!("resolved"),
        BranchStatus::PartiallyUnresolved { reasons } => json!({
            "partially_unresolved": {
                "reasons": reasons.iter().map(partial_branch_reason_value).collect::<Vec<_>>(),
            },
        }),
        BranchStatus::Unresolved { reason } => json!({
            "unresolved": { "reason": unresolved_branch_reason_value(reason) },
        }),
    }
}

fn fallthrough_surface_value(surface: &FallthroughSurface) -> Value {
    match surface {
        FallthroughSurface::None { reason } => json!({
            "none": { "reason": no_fallthrough_reason_value(reason) },
        }),
        FallthroughSurface::Branches { branches } => json!({
            "branches": branches.iter().map(|branch| {
                let FallthroughBranch {
                    branch_key,
                    condition_text,
                    props,
                    events,
                    root_chain,
                    status,
                } = branch;
                json!({
                    "branch_key": branch_key,
                    "condition_text": condition_text,
                    "props": props.iter().map(|p| {
                        let FallthroughPropEntry { name, type_expr, raw_type, sources } = p;
                        json!({
                            "name": name,
                            "type_expr": ser(type_expr),
                            "raw_type": raw_type,
                            "sources": sources.iter().map(inherited_source_value).collect::<Vec<_>>(),
                        })
                    }).collect::<Vec<_>>(),
                    "events": events.iter().map(|e| {
                        let FallthroughEventEntry { name, payload, raw_signature, sources } = e;
                        json!({
                            "name": name,
                            "payload": ser(payload),
                            "raw_signature": raw_signature,
                            "sources": sources.iter().map(inherited_source_value).collect::<Vec<_>>(),
                        })
                    }).collect::<Vec<_>>(),
                    "root_chain": root_chain.iter().map(resolved_root_step_value).collect::<Vec<_>>(),
                    "status": branch_status_value(status),
                })
            }).collect::<Vec<_>>(),
        }),
    }
}

fn macro_expansion_diagnostics_value(diag: &MacroExpansionDiagnostics) -> Value {
    let MacroExpansionDiagnostics {
        macro_kind,
        macro_index,
        diagnostics,
        exactness,
        execution_status,
    } = diag;
    json!({
        "macro_kind": match macro_kind {
            MacroExpansionKind::DefineProps => "define_props",
            MacroExpansionKind::DefineEmits => "define_emits",
            MacroExpansionKind::DefineSlots => "define_slots",
        },
        "macro_index": macro_index,
        "diagnostics": diagnostics.iter().map(ser).collect::<Vec<_>>(),
        "exactness": ser(exactness),
        "execution_status": ser(execution_status),
    })
}
