//! Convert raw template data from `verter_compiler` into `verter_semantic::analysis` types.
//!
//! This module bridges the two independent crates: `verter_compiler` produces
//! [`RawTemplateData`] during compilation, and this function converts it into
//! [`TemplateAnalysisSnapshot`] that `verter_session` stores alongside script/style analysis.

use rustc_hash::FxHashSet;
use verter_compiler::compile::template_data::RawTemplateData;
use verter_semantic::analysis::macro_usage::MacroUsageFacts;
use verter_semantic::analysis::template::{
    AnalyzedEmitDefinition, AnalyzedPropDefinition, AnalyzedSlotDeclaration, BindingUsageKind,
    CommentDirective, CommentDirectiveKind, DefinedSlot, ElementNamespace, IfChain,
    PropValueConstness, SnippetDefinition, SvelteDirectiveInfo, TemplateAnalysisSnapshot,
    TemplateAttribute, TemplateBindingOccurrence, TemplateComponentBinding, TemplateComponentEvent,
    TemplateComponentUsage, TemplateComponentVModel, TemplateDirective, TemplateElement,
    TemplateEventHandler, TemplateMemberRead, TemplatePropUsage, TemplateRef, TemplateTextSegment,
    UnresolvedBinding, VForDirective, VModelDirective,
};
use verter_semantic::analysis::types::{
    AnalyzedBinding, AnalyzedMacro, AnalyzedMacroKind, VueApiCallSite, VueApiClassification,
};

/// Declaration + script-usage context for the unused-declaration diagnostics
/// (`no-unused-props` / `no-unused-emit-declarations` / `no-unused-slots`).
/// Built from the SAME script analysis snapshot the template conversion pairs
/// with — one shared population path for all three kinds.
pub(crate) struct UnusedDeclarationContext<'a> {
    pub macros: &'a [AnalyzedMacro],
    pub macro_usage: Option<&'a MacroUsageFacts>,
    /// A `useSlots()` call exists — slot usage cannot be statically bounded.
    pub use_slots_called: bool,
    /// The `defineProps` root binding is referenced from `<style>` `v-bind()` —
    /// member-level liveness cannot be bounded (suppresses unused-prop).
    pub props_root_used_in_style: bool,
    /// Root identifiers referenced by `<style>` `v-bind()` expressions. CSS
    /// `v-bind()` resolves through the component render context, which
    /// includes PROPS by bare name — a prop whose name appears here is live
    /// (per-member fact, not a whole-kind suppression).
    pub style_vbind_roots: &'a [String],
}

impl<'a> UnusedDeclarationContext<'a> {
    pub(crate) fn from_analysis(
        macros: &'a [AnalyzedMacro],
        macro_usage: Option<&'a MacroUsageFacts>,
        vue_api_calls: &[VueApiCallSite],
        bindings: &[AnalyzedBinding],
        style_vbind_roots: &'a [String],
    ) -> Self {
        let use_slots_called = vue_api_calls
            .iter()
            .any(|call| matches!(call.api, VueApiClassification::UseSlots));
        let props_binding = macros
            .iter()
            .find(|m| {
                matches!(
                    m.kind,
                    AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults
                )
            })
            .and_then(|m| m.binding_name.as_deref());
        let props_root_used_in_style = props_binding.is_some_and(|name| {
            bindings
                .iter()
                .any(|binding| binding.name == name && binding.used_in_style)
        });
        Self {
            macros,
            macro_usage,
            use_slots_called,
            props_root_used_in_style,
            style_vbind_roots,
        }
    }
}

/// Convert raw template data from `verter_compiler` into `verter_semantic::analysis` types.
///
/// The `script_imports` map resolves component tag names to their import source
/// paths for cross-file analysis. This is populated from the script analysis
/// (e.g., `"Child"` → `"./Child.vue"`).
///
/// `binding_class_unions` maps binding names to their string literal union values
/// (e.g., `[("variant", ["primary", "secondary"])]`). Used to resolve bare
/// `:class="variant"` bindings to CSS class names.
///
/// `props_binding_name` is the variable name used for `defineProps` return value
/// (e.g., `"props"` from `const props = defineProps<...>()`). Used to resolve
/// `:class="props.variant"` patterns.
pub fn convert_raw_to_analysis(
    raw: &RawTemplateData,
    script_imports: &[(String, String)], // (local_name, source_path)
    binding_class_unions: &[(String, Vec<String>)], // (binding_name, class_names)
    props_binding_name: Option<&str>,
    unused_declarations: Option<&UnusedDeclarationContext<'_>>,
) -> TemplateAnalysisSnapshot {
    let components: Vec<TemplateComponentUsage> = raw
        .components
        .iter()
        .map(|c| {
            let pascal_tag = to_pascal_case(&c.tag_name);
            let import_source = script_imports
                .iter()
                .find(|(name, _)| name == &c.tag_name || *name == pascal_tag)
                .map(|(_, source)| source.clone());

            let props = c
                .props
                .iter()
                .map(|p| {
                    let constness = if p.from_spread {
                        PropValueConstness::Unknown
                    } else if !p.is_bound {
                        PropValueConstness::Const
                    } else {
                        match p.all_bindings_static {
                            Some(true) => PropValueConstness::Const,
                            Some(false) => PropValueConstness::Dynamic,
                            None => PropValueConstness::Unknown,
                        }
                    };

                    TemplatePropUsage {
                        name: p.name.clone(),
                        is_bound: p.is_bound,
                        expression: p.expression.clone(),
                        constness,
                        referenced_bindings: p.referenced_bindings.clone(),
                        from_spread: p.from_spread,
                        span: p.span,
                        name_span: p.name_span,
                        is_shorthand: p.is_same_name_shorthand,
                    }
                })
                .collect();

            // Extract class names from :class object syntax
            let mut dynamic_classes = c
                .dynamic_class_expr
                .as_deref()
                .map(verter_semantic::analysis::extract_dynamic_class_names)
                .unwrap_or_default();

            // If extract_dynamic_class_names found nothing, try resolving
            // bare identifier bindings via string literal union types.
            if dynamic_classes.is_empty() {
                if let Some(expr) = c.dynamic_class_expr.as_deref() {
                    if let Some(classes) =
                        resolve_classes_from_binding(expr, binding_class_unions, props_binding_name)
                    {
                        dynamic_classes = classes.to_vec();
                    }
                }
            }

            // Collect v-model directives that target this component
            let v_models: Vec<TemplateComponentVModel> = raw
                .v_model_directives
                .iter()
                .filter(|vm| vm.target_is_component && vm.target_tag == c.tag_name)
                .filter(|vm| {
                    // Match by span overlap: the v-model's span should be within this component's span
                    vm.span.start >= c.span.start && vm.span.end <= c.span.end
                })
                .map(|vm| {
                    // v-model without an argument name defaults to "modelValue"
                    let binding_name = if vm.binding_name.is_empty() {
                        "modelValue".to_string()
                    } else {
                        vm.binding_name.clone()
                    };
                    TemplateComponentVModel {
                        binding_name,
                        span: vm.span,
                    }
                })
                .collect();

            // Framework-neutral bindings/events (the Svelte `bind:` family and
            // the legacy `on:` directive events). Empty for Vue.
            let bindings = c
                .bindings
                .iter()
                .map(|b| TemplateComponentBinding {
                    name: b.name.clone(),
                    modifiers: b.modifiers.clone(),
                    span: b.span,
                })
                .collect();
            let events = c
                .events
                .iter()
                .map(|e| TemplateComponentEvent {
                    name: e.name.clone(),
                    handler_expression: e.handler_expression.clone(),
                    is_inline: e.is_inline,
                    modifiers: e.modifiers.clone(),
                    span: e.span,
                })
                .collect();

            TemplateComponentUsage {
                name: c.tag_name.clone(),
                import_source,
                is_dynamic: c.is_dynamic,
                props,
                has_spread: c.has_spread,
                slots_used: c.slots_used.clone(),
                static_classes: c.static_classes.clone(),
                has_dynamic_class: c.has_dynamic_class,
                dynamic_classes,
                v_models,
                bindings,
                events,
                span: c.span,
            }
        })
        .collect();

    let mut binding_occurrences = Vec::new();
    let mut unresolved_bindings = Vec::new();

    for b in &raw.binding_occurrences {
        let usage_kind = match b.usage_kind {
            0 => BindingUsageKind::Interpolation,
            1 => BindingUsageKind::DirectiveValue,
            2 => BindingUsageKind::EventHandler,
            3 => BindingUsageKind::ComponentTag,
            4 => BindingUsageKind::TemplateRef,
            5 => BindingUsageKind::IteratorSource,
            _ => BindingUsageKind::DirectiveValue,
        };

        if b.is_in_bindings_map {
            binding_occurrences.push(TemplateBindingOccurrence {
                name: b.name.clone(),
                span: b.span,
                usage_kind,
            });
        } else {
            unresolved_bindings.push(UnresolvedBinding {
                name: b.name.clone(),
                span: b.span,
            });
        }
    }

    let defined_slots = raw
        .slot_definitions
        .iter()
        .map(|s| DefinedSlot {
            name: s.name.clone(),
            has_bindings: s.has_bindings,
            binding_names: s.binding_names.clone(),
            binding_expressions: s.binding_expressions.clone(),
            binding_value_spans: s.binding_value_spans.clone(),
            has_fallback_content: s.has_fallback_content,
            span: s.span,
        })
        .collect();

    let template_refs = raw
        .template_refs
        .iter()
        .map(|r| TemplateRef {
            name: r.name.clone(),
            is_dynamic: r.is_dynamic,
            target_tag: r.target_tag.clone(),
        })
        .collect();

    let event_handlers = raw
        .event_handlers
        .iter()
        .map(|h| {
            let handler_binding = if !h.is_inline {
                h.handler_expression.clone()
            } else {
                None
            };
            TemplateEventHandler {
                event_name: h.event_name.clone(),
                handler_binding,
                is_inline: h.is_inline,
                target_tag: h.target_tag.clone(),
                span: h.span,
            }
        })
        .collect();

    let if_chains = raw
        .if_chains
        .iter()
        .map(|chain| IfChain {
            conditions: chain.conditions.clone(),
        })
        .collect();

    let comment_directives = raw
        .comment_directives
        .iter()
        .map(|d| {
            let kind = match d.kind {
                0 => CommentDirectiveKind::Disable,
                1 => CommentDirectiveKind::DisableNextLine,
                2 => CommentDirectiveKind::Enable,
                3 => CommentDirectiveKind::Todo,
                4 => CommentDirectiveKind::Fixme,
                5 => CommentDirectiveKind::Deprecated,
                6 => CommentDirectiveKind::IgnoreStart,
                7 => CommentDirectiveKind::IgnoreEnd,
                _ => CommentDirectiveKind::Disable,
            };
            CommentDirective {
                kind,
                message: d.rule_or_message.clone(),
                span: d.span,
                affects_next_line: d.affects_next_line,
            }
        })
        .collect();

    let mut v_if_v_for_conflicts = Vec::new();

    let elements: Vec<TemplateElement> = raw
        .elements
        .iter()
        .map(|e| {
            // Detect v-if + v-for on same element — use the v-if directive span
            if e.has_v_if && e.v_for_idx.is_some() {
                let (start, end) = e
                    .directives
                    .iter()
                    .find(|d| d.name == "if")
                    .map(|d| (d.span.start, d.span.end))
                    .unwrap_or((e.span.start, e.span.end));
                v_if_v_for_conflicts.push((start, end));
            }

            let attributes = e
                .attributes
                .iter()
                .map(|a| TemplateAttribute {
                    name: a.name.clone(),
                    value: a.value.clone(),
                    is_dynamic: a.is_dynamic,
                    span: a.span,
                    name_end: a.name_end,
                    value_span: a.value_span,
                })
                .collect();

            let directives = e
                .directives
                .iter()
                .map(|d| TemplateDirective {
                    name: d.name.clone(),
                    raw_name: d.raw_name.clone(),
                    argument: d.argument.clone(),
                    modifiers: d.modifiers.clone(),
                    expression: d.expression.clone(),
                    span: d.span,
                    name_end: d.name_end,
                    arg_span: d.arg_span,
                    expression_span: d.expression_span,
                    modifier_spans: d.modifier_spans.clone(),
                })
                .collect();
            let bind_class_expressions: Vec<&str> = e
                .directives
                .iter()
                .filter(|d| d.name == "bind" && d.argument.as_deref() == Some("class"))
                .filter_map(|d| d.expression.as_deref())
                .collect();
            let bind_style_expressions: Vec<&str> = e
                .directives
                .iter()
                .filter(|d| d.name == "bind" && d.argument.as_deref() == Some("style"))
                .filter_map(|d| d.expression.as_deref())
                .collect();

            let v_for = e.v_for_idx.map(|idx| {
                let vf = &raw.v_for_directives[idx];
                VForDirective {
                    variable: vf.variable.clone(),
                    index: vf.index.clone(),
                    iterable: vf.iterable.clone(),
                    has_key: vf.has_key,
                    key_expression: vf.key_expression.clone(),
                    key_uses_index: vf.key_uses_index,
                    span: vf.span,
                }
            });

            let v_model = e.v_model_idx.map(|idx| {
                let vm = &raw.v_model_directives[idx];
                VModelDirective {
                    binding_name: vm.binding_name.clone(),
                    modifiers: vm.modifiers.clone(),
                    target_is_component: vm.target_is_component,
                    target_tag: vm.target_tag.clone(),
                    span: vm.span,
                }
            });

            // Extract class names from :class object syntax (e.g., { 'foo': cond })
            let mut dynamic_classes: Vec<String> = e
                .attributes
                .iter()
                .filter(|a| a.is_dynamic && a.name == "class")
                .filter_map(|a| a.value.as_deref())
                .chain(bind_class_expressions.iter().copied())
                .flat_map(verter_semantic::analysis::extract_dynamic_class_names)
                .collect();

            // If no class names from object/array/ternary, try resolving
            // bare identifier bindings via string literal union types.
            if dynamic_classes.is_empty() {
                for expr in e
                    .attributes
                    .iter()
                    .filter(|a| a.is_dynamic && a.name == "class")
                    .filter_map(|a| a.value.as_deref())
                    .chain(bind_class_expressions.iter().copied())
                {
                    if let Some(classes) =
                        resolve_classes_from_binding(expr, binding_class_unions, props_binding_name)
                    {
                        dynamic_classes.extend_from_slice(classes);
                    }
                }
            }

            // Extract CSS variables from :style bindings (e.g., { '--color': val })
            let dynamic_style_vars: Vec<verter_semantic::analysis::template::DynamicStyleVar> = e
                .attributes
                .iter()
                .filter(|a| a.is_dynamic && a.name == "style")
                .filter_map(|a| a.value.as_deref())
                .chain(bind_style_expressions.iter().copied())
                .flat_map(verter_semantic::analysis::template::extract_dynamic_style_vars)
                .collect();

            // Extract CSS variables from static style attributes (e.g., style="--color: red")
            let static_style_vars: Vec<verter_semantic::analysis::template::StaticStyleVar> = e
                .attributes
                .iter()
                .filter(|a| !a.is_dynamic && a.name == "style")
                .filter_map(|a| a.value.as_deref())
                .flat_map(verter_semantic::analysis::template::extract_static_style_vars)
                .collect();

            let component_usage_index = if e.is_component {
                let pascal_tag = to_pascal_case(&e.tag);
                components
                    .iter()
                    .enumerate()
                    .find(|(_, c)| {
                        c.span == e.span
                            && (c.name == e.tag
                                || c.name.eq_ignore_ascii_case(&e.tag)
                                || c.name == pascal_tag)
                    })
                    .map(|(idx, _)| idx as u32)
            } else {
                None
            };

            TemplateElement {
                tag: e.tag.clone(),
                is_component: e.is_component,
                is_self_closing: e.is_self_closing,
                namespace: ElementNamespace::Html,
                attributes,
                directives,
                v_for,
                v_model,
                has_v_if: e.has_v_if,
                has_v_else: e.has_v_else,
                has_v_else_if: e.has_v_else_if,
                v_if_condition: e.v_if_condition.clone(),
                has_v_show: e.has_v_show,
                has_v_html: e.has_v_html,
                has_v_text: e.has_v_text,
                has_text_content: e.has_text_content,
                has_bare_text: e.has_bare_text,
                has_element_children: e.has_element_children,
                nesting_depth: e.nesting_depth,
                parent_tag: e.parent_tag.clone(),
                parent_index: e.parent_index,
                dynamic_classes,
                dynamic_style_vars,
                static_style_vars,
                component_usage_index,
                span: e.span,
                tag_span_end: e.tag_span_end,
                content_end: e.content_end,
                text_children: e
                    .text_children
                    .iter()
                    .map(|seg| {
                        match seg {
                        verter_compiler::compile::template_data::RawTextSegment::Text {
                            span,
                            is_entity,
                        } => TemplateTextSegment::Text {
                            span: *span,
                            is_entity: *is_entity,
                        },
                        verter_compiler::compile::template_data::RawTextSegment::Interpolation {
                            span,
                            expression_span,
                        } => TemplateTextSegment::Interpolation {
                            span: *span,
                            expression_span: *expression_span,
                        },
                    }
                    })
                    .collect(),
            }
        })
        .collect();

    // Collect all CSS variable names from template inline styles (deduped)
    let css_var_names: Vec<String> = {
        let mut names: Vec<String> = elements
            .iter()
            .flat_map(|el| {
                el.dynamic_style_vars
                    .iter()
                    .map(|v| v.name.clone())
                    .chain(el.static_style_vars.iter().map(|v| v.name.clone()))
            })
            .collect();
        names.sort();
        names.dedup();
        names
    };

    let mut snapshot = TemplateAnalysisSnapshot {
        components,
        binding_occurrences,
        unresolved_bindings,
        defined_slots,
        template_refs,
        event_handlers,
        elements,
        if_chains,
        max_nesting_depth: raw.max_nesting_depth,
        v_if_v_for_conflicts,
        comment_directives,
        css_var_names,
        has_expression_errors: raw.has_expression_errors,
        has_dynamic_slot_outlet: raw.has_dynamic_slot_outlet,
        member_reads: raw
            .member_reads
            .iter()
            .map(|read| TemplateMemberRead {
                root: read.root.clone(),
                member: read.member.clone(),
                root_span: read.root_span,
            })
            .collect(),
        snippet_definitions: raw
            .snippet_definitions
            .iter()
            .map(|snippet| SnippetDefinition {
                name: snippet.name.clone(),
                span: snippet.name_span,
                params_text: snippet.params_text.clone(),
            })
            .collect(),
        svelte_directives: raw
            .svelte_directives
            .iter()
            .map(|directive| SvelteDirectiveInfo {
                keyword: directive.keyword.clone(),
                local: directive.local.clone(),
                span: directive.span,
                keyword_end: directive.keyword_end,
                local_span: directive.local_span,
                value_span: directive.value_span,
            })
            .collect(),
        // prop/emit definitions and type_enhancements come from script analysis.
        ..Default::default()
    };

    if let Some(ctx) = unused_declarations {
        populate_unused_declaration_facts(&mut snapshot, ctx);
    }

    snapshot
}

/// Populate the unused-declaration inventories (`prop_definitions`,
/// `emit_definitions`, `slot_declarations`) from the macro member inventories
/// crossed with script + template usage facts.
///
/// FAIL-OPEN contract: a kind's inventory is populated ONLY when its usage can
/// be statically bounded — any escape, incompleteness, or dynamic construct
/// leaves that inventory EMPTY and the corresponding lint rule silent:
/// - all kinds: any template expression parse error;
/// - props: script escape (spread/call-arg/alias/computed), destructured
///   `defineProps` (provider-owned TS6133 — never double-report), style
///   `v-bind()` on the props root, `$props` referenced in the template;
/// - emits: script emit escape (aliased/passed/dynamic name), `$emit` OR the
///   `defineEmits` return binding referenced in the template;
/// - slots: dynamic outlet `<slot :name="expr">`, `useSlots()` anywhere,
///   `$slots` referenced in the template.
///
/// `defineModel` members are self-consuming: its implicit prop never enters
/// `prop_fields`, and an explicitly declared `update:<model>` event is skipped.
///
/// Name-identity caveat (accepted false-NEGATIVE class, mirror of the
/// `macro_usage` scope-blindness note): `template_mentions` matches template
/// occurrences by NAME, not by scope — an unrelated same-named template
/// binding (a `v-for` item variable or slot-prop shadowing a prop name) marks
/// that member "used" and the diagnostic is missed. Errs toward silence;
/// never produces a false positive.
fn populate_unused_declaration_facts(
    tpl: &mut TemplateAnalysisSnapshot,
    ctx: &UnusedDeclarationContext<'_>,
) {
    if tpl.has_expression_errors {
        return;
    }
    let Some(usage) = ctx.macro_usage else {
        return;
    };
    let template_mentions = |tpl: &TemplateAnalysisSnapshot, name: &str| -> bool {
        tpl.binding_occurrences.iter().any(|o| o.name == name)
            || tpl.unresolved_bindings.iter().any(|u| u.name == name)
    };
    // For a tracked ROOT (the props binding, `$props`, `$slots`): the set of
    // literal members read off it in the template — provided EVERY occurrence
    // of the root is consumed by a member read. A bare/spread/computed use of
    // the root leaves an unconsumed occurrence => `None` (whole-object escape).
    let bounded_member_reads =
        |tpl: &TemplateAnalysisSnapshot, root: &str| -> Option<FxHashSet<String>> {
            let consumed: FxHashSet<u32> = tpl
                .member_reads
                .iter()
                .filter(|read| read.root == root)
                .map(|read| read.root_span.start)
                .collect();
            let escaped = tpl
                .binding_occurrences
                .iter()
                .filter(|o| o.name == root)
                .map(|o| o.span.start)
                .chain(
                    tpl.unresolved_bindings
                        .iter()
                        .filter(|u| u.name == root)
                        .map(|u| u.span.start),
                )
                .any(|start| !consumed.contains(&start));
            if escaped {
                return None;
            }
            Some(
                tpl.member_reads
                    .iter()
                    .filter(|read| read.root == root)
                    .map(|read| read.member.clone())
                    .collect(),
            )
        };

    // ── Props ──
    let props_root = ctx
        .macros
        .iter()
        .find(|m| {
            matches!(
                m.kind,
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults
            )
        })
        .and_then(|m| m.binding_name.as_deref());
    let props_root_template = match props_root {
        Some(root) => bounded_member_reads(tpl, root),
        None => Some(FxHashSet::default()),
    };
    let dollar_props_template = bounded_member_reads(tpl, "$props");
    let props_suppressed = usage.props_escapes
        || usage.props_destructured
        || ctx.props_root_used_in_style
        || props_root_template.is_none()
        || dollar_props_template.is_none();
    if !props_suppressed {
        let props_root_template = props_root_template.unwrap_or_default();
        let dollar_props_template = dollar_props_template.unwrap_or_default();
        let reads: FxHashSet<&str> = usage
            .props_member_reads
            .iter()
            .map(String::as_str)
            .collect();
        let default_keys: FxHashSet<&str> = ctx
            .macros
            .iter()
            .flat_map(|m| m.default_keys.iter().map(String::as_str))
            .collect();
        let mut definitions = Vec::new();
        for mac in ctx.macros.iter().filter(|m| {
            matches!(
                m.kind,
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults
            )
        }) {
            for field in &mac.prop_fields {
                // Author-local members only: foreign/synthetic spans (heritage,
                // Pick/Omit, external intersection arms) are not diagnosable
                // on an authored range — fail open per member.
                if !field.declared_in_macro_type_arg || field.span.end <= field.span.start {
                    continue;
                }
                let used_in_template = template_mentions(tpl, &field.name)
                    || props_root_template.contains(field.name.as_str())
                    || dollar_props_template.contains(field.name.as_str());
                // `<style>` `v-bind(color)` resolves the prop by bare name
                // through the render context — a per-member liveness fact
                // (the whole-kind style suppression above covers only the
                // props ROOT binding escaping into style).
                let used_in_style = ctx.style_vbind_roots.iter().any(|root| root == &field.name);
                definitions.push(AnalyzedPropDefinition {
                    name: field.name.clone(),
                    type_annotation: field.type_annotation.clone(),
                    has_default: default_keys.contains(field.name.as_str()),
                    is_required: !field.is_optional,
                    is_boolean: false,
                    used_in_template,
                    used_in_script: reads.contains(field.name.as_str()) || used_in_style,
                    span: field.span,
                });
            }
        }
        tpl.prop_definitions = definitions;
    }

    // ── Emits ──
    // A template occurrence of the `defineEmits` RETURN BINDING
    // (`@click="emit('close')"`, `:handler="emit"`) is the standard
    // template-emit pattern: suppress the whole kind on any occurrence
    // (per-name template call extraction stays deferred — fail-open).
    let emit_binding = ctx
        .macros
        .iter()
        .find(|m| matches!(m.kind, AnalyzedMacroKind::DefineEmits))
        .and_then(|m| m.binding_name.as_deref());
    let emits_suppressed = usage.emit_escapes
        || template_mentions(tpl, "$emit")
        || emit_binding.is_some_and(|name| template_mentions(tpl, name));
    if !emits_suppressed {
        let model_events: FxHashSet<String> = ctx
            .macros
            .iter()
            .filter(|m| matches!(m.kind, AnalyzedMacroKind::DefineModel))
            .map(|m| format!("update:{}", m.model_name.as_deref().unwrap_or("modelValue")))
            .collect();
        let mut definitions = Vec::new();
        for mac in ctx
            .macros
            .iter()
            .filter(|m| matches!(m.kind, AnalyzedMacroKind::DefineEmits))
        {
            for field in &mac.emit_fields {
                if field.span.end <= field.span.start {
                    continue;
                }
                // `defineModel('x')` emits `update:x` itself — self-consuming.
                if model_events.contains(&field.name) {
                    continue;
                }
                let emit_locations: Vec<(u32, u32)> = usage
                    .emit_literal_calls
                    .iter()
                    .filter(|call| call.name == field.name)
                    .map(|call| (call.span.start, call.span.end))
                    .collect();
                definitions.push(AnalyzedEmitDefinition {
                    event_name: field.name.clone(),
                    has_validator: false,
                    is_declared: true,
                    emit_locations,
                    span: field.span,
                });
            }
        }
        tpl.emit_definitions = definitions;
    }

    // ── Slots ──
    let dollar_slots_template = bounded_member_reads(tpl, "$slots");
    let slots_suppressed =
        tpl.has_dynamic_slot_outlet || ctx.use_slots_called || dollar_slots_template.is_none();
    if !slots_suppressed {
        let dollar_slots_template = dollar_slots_template.unwrap_or_default();
        let outlets: FxHashSet<&str> = tpl.defined_slots.iter().map(|s| s.name.as_str()).collect();
        let mut declarations = Vec::new();
        for mac in ctx
            .macros
            .iter()
            .filter(|m| matches!(m.kind, AnalyzedMacroKind::DefineSlots))
        {
            for field in &mac.slot_fields {
                if field.span.end <= field.span.start {
                    continue;
                }
                let used = outlets.contains(field.name.as_str())
                    || dollar_slots_template.contains(field.name.as_str());
                declarations.push(AnalyzedSlotDeclaration {
                    name: field.name.clone(),
                    span: field.span,
                    used,
                });
            }
        }
        tpl.slot_declarations = declarations;
    }
}

/// Convert a kebab-case or snake_case string to PascalCase.
fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Resolve a `:class` expression to class names via string literal union bindings.
///
/// Returns `Some(&[String])` if the expression is:
/// - A bare identifier matching a name in `binding_class_unions`
///   (e.g., `:class="variant"` where `variant: 'primary' | 'secondary'`)
/// - A props member access matching `{props_binding_name}.{prop_name}`
///   (e.g., `:class="props.variant"`)
///
/// Returns `None` otherwise.
fn resolve_classes_from_binding<'a>(
    expr: &str,
    binding_class_unions: &'a [(String, Vec<String>)],
    props_binding_name: Option<&str>,
) -> Option<&'a [String]> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Check bare identifier: `:class="variant"`
    if is_simple_identifier(trimmed) {
        if let Some((_, classes)) = binding_class_unions.iter().find(|(n, _)| n == trimmed) {
            return Some(classes);
        }
    }

    // Check props member access: `:class="props.variant"`
    if let Some(props_name) = props_binding_name {
        if let Some(rest) = trimmed.strip_prefix(props_name) {
            if let Some(member) = rest.strip_prefix('.') {
                let member = member.trim();
                if is_simple_identifier(member) {
                    if let Some((_, classes)) =
                        binding_class_unions.iter().find(|(n, _)| n == member)
                    {
                        return Some(classes);
                    }
                }
            }
        }
    }

    None
}

/// Check if a string is a valid JS identifier (no dots, brackets, parens, etc.).
fn is_simple_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !s.chars().next().unwrap().is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_compiler::common::Span;
    use verter_compiler::compile::template_data::*;

    /// @ai-generated - Empty raw data converts to empty snapshot
    #[test]
    fn empty_raw_converts_to_empty_snapshot() {
        let raw = RawTemplateData::default();
        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);

        assert!(result.components.is_empty());
        assert!(result.binding_occurrences.is_empty());
        assert!(result.unresolved_bindings.is_empty());
        assert!(result.defined_slots.is_empty());
        assert!(result.template_refs.is_empty());
        assert!(result.event_handlers.is_empty());
        assert_eq!(result.max_nesting_depth, 0);
    }

    /// @ai-generated - Component usage converts with import resolution
    #[test]
    fn component_with_import_resolved() {
        let raw = RawTemplateData {
            components: vec![RawComponentUsage {
                tag_name: "Child".to_string(),
                is_dynamic: false,
                props: vec![RawPropData {
                    name: "msg".to_string(),
                    is_bound: false,
                    expression: Some("hello".to_string()),
                    referenced_bindings: vec![],
                    all_bindings_static: None,
                    from_spread: false,
                    span: Span::new(0, 0),
                    name_span: Span::new(0, 0),
                    is_same_name_shorthand: false,
                }],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_class_expr: None,
                bindings: vec![],
                events: vec![],
                span: Span::new(10, 40),
            }],
            ..Default::default()
        };

        let imports = vec![("Child".to_string(), "./Child.vue".to_string())];
        let result = convert_raw_to_analysis(&raw, &imports, &[], None, None);

        assert_eq!(result.components.len(), 1);
        assert_eq!(result.components[0].name, "Child");
        assert_eq!(
            result.components[0].import_source.as_deref(),
            Some("./Child.vue")
        );
        assert_eq!(result.components[0].props.len(), 1);
        assert_eq!(
            result.components[0].props[0].constness,
            PropValueConstness::Const
        );
    }

    /// @ai-generated - Unresolved component has no import source
    #[test]
    fn component_without_import_unresolved() {
        let raw = RawTemplateData {
            components: vec![RawComponentUsage {
                tag_name: "Unknown".to_string(),
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_class_expr: None,
                bindings: vec![],
                events: vec![],
                span: Span::new(0, 20),
            }],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);
        assert!(result.components[0].import_source.is_none());
    }

    /// @ai-generated - Kebab-case tag name matches PascalCase import
    #[test]
    fn component_kebab_case_matches_pascal_import() {
        let raw = RawTemplateData {
            components: vec![RawComponentUsage {
                tag_name: "my-header".to_string(),
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_class_expr: None,
                bindings: vec![],
                events: vec![],
                span: Span::new(0, 20),
            }],
            ..Default::default()
        };

        let imports = vec![("MyHeader".to_string(), "./MyHeader.vue".to_string())];
        let result = convert_raw_to_analysis(&raw, &imports, &[], None, None);

        assert_eq!(
            result.components[0].import_source.as_deref(),
            Some("./MyHeader.vue"),
            "kebab-case tag should match PascalCase import"
        );
    }

    /// @ai-generated - Binding occurrences split into resolved and unresolved
    #[test]
    fn bindings_split_resolved_unresolved() {
        let raw = RawTemplateData {
            binding_occurrences: vec![
                RawBindingOccurrence {
                    name: "msg".to_string(),
                    span: Span::new(10, 13),
                    is_in_bindings_map: true,
                    usage_kind: 0,
                },
                RawBindingOccurrence {
                    name: "unknown".to_string(),
                    span: Span::new(20, 27),
                    is_in_bindings_map: false,
                    usage_kind: 0,
                },
            ],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);
        assert_eq!(result.binding_occurrences.len(), 1);
        assert_eq!(result.binding_occurrences[0].name, "msg");
        assert_eq!(
            result.binding_occurrences[0].usage_kind,
            BindingUsageKind::Interpolation
        );
        assert_eq!(result.unresolved_bindings.len(), 1);
        assert_eq!(result.unresolved_bindings[0].name, "unknown");
    }

    /// @ai-generated - Prop constness classification
    #[test]
    fn prop_constness_classified() {
        let raw = RawTemplateData {
            components: vec![RawComponentUsage {
                tag_name: "Child".to_string(),
                is_dynamic: false,
                props: vec![
                    RawPropData {
                        name: "static_prop".to_string(),
                        is_bound: false,
                        expression: Some("hello".to_string()),
                        referenced_bindings: vec![],
                        all_bindings_static: None,
                        from_spread: false,
                        span: Span::new(0, 0),
                        name_span: Span::new(0, 0),
                        is_same_name_shorthand: false,
                    },
                    RawPropData {
                        name: "const_bound".to_string(),
                        is_bound: true,
                        expression: Some("LABEL".to_string()),
                        referenced_bindings: vec!["LABEL".to_string()],
                        all_bindings_static: Some(true),
                        from_spread: false,
                        span: Span::new(0, 0),
                        name_span: Span::new(0, 0),
                        is_same_name_shorthand: false,
                    },
                    RawPropData {
                        name: "dynamic_bound".to_string(),
                        is_bound: true,
                        expression: Some("count".to_string()),
                        referenced_bindings: vec!["count".to_string()],
                        all_bindings_static: Some(false),
                        from_spread: false,
                        span: Span::new(0, 0),
                        name_span: Span::new(0, 0),
                        is_same_name_shorthand: false,
                    },
                    RawPropData {
                        name: "".to_string(),
                        is_bound: true,
                        expression: None,
                        referenced_bindings: vec![],
                        all_bindings_static: None,
                        from_spread: true,
                        span: Span::new(0, 0),
                        name_span: Span::new(0, 0),
                        is_same_name_shorthand: false,
                    },
                ],
                has_spread: true,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_class_expr: None,
                bindings: vec![],
                events: vec![],
                span: Span::new(0, 50),
            }],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);
        let props = &result.components[0].props;
        assert_eq!(props[0].constness, PropValueConstness::Const); // static
        assert_eq!(props[1].constness, PropValueConstness::Const); // bound const
        assert_eq!(props[2].constness, PropValueConstness::Dynamic); // bound ref
        assert_eq!(props[3].constness, PropValueConstness::Unknown); // spread
    }

    /// @ai-generated - Template refs convert correctly
    #[test]
    fn template_refs_converted() {
        let raw = RawTemplateData {
            template_refs: vec![
                RawTemplateRef {
                    name: "el".to_string(),
                    is_dynamic: false,
                    target_tag: "div".to_string(),
                    span: Span::new(0, 20),
                },
                RawTemplateRef {
                    name: "elRef".to_string(),
                    is_dynamic: true,
                    target_tag: "input".to_string(),
                    span: Span::new(25, 50),
                },
            ],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);
        assert_eq!(result.template_refs.len(), 2);
        assert!(!result.template_refs[0].is_dynamic);
        assert!(result.template_refs[1].is_dynamic);
    }

    /// @ai-generated - Event handlers convert with handler binding extraction
    #[test]
    fn event_handlers_converted() {
        let raw = RawTemplateData {
            event_handlers: vec![
                RawEventHandler {
                    event_name: "click".to_string(),
                    handler_expression: Some("handleClick".to_string()),
                    is_inline: false,
                    target_tag: "div".to_string(),
                    span: Span::new(0, 20),
                },
                RawEventHandler {
                    event_name: "click".to_string(),
                    handler_expression: Some("count++".to_string()),
                    is_inline: true,
                    target_tag: "div".to_string(),
                    span: Span::new(25, 50),
                },
            ],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);
        assert_eq!(result.event_handlers.len(), 2);
        assert_eq!(
            result.event_handlers[0].handler_binding.as_deref(),
            Some("handleClick")
        );
        assert!(!result.event_handlers[0].is_inline);
        assert!(result.event_handlers[1].handler_binding.is_none());
        assert!(result.event_handlers[1].is_inline);
    }

    /// @ai-generated - Comment directives convert with correct kinds
    #[test]
    fn comment_directives_converted() {
        let raw = RawTemplateData {
            comment_directives: vec![
                RawCommentDirective {
                    kind: 0,
                    rule_or_message: Some("no-v-html".to_string()),
                    span: Span::new(0, 40),
                    affects_next_line: false,
                },
                RawCommentDirective {
                    kind: 1,
                    rule_or_message: Some("require-v-for-key".to_string()),
                    span: Span::new(45, 80),
                    affects_next_line: true,
                },
            ],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);
        assert_eq!(result.comment_directives.len(), 2);
        assert_eq!(
            result.comment_directives[0].kind,
            CommentDirectiveKind::Disable
        );
        assert_eq!(
            result.comment_directives[1].kind,
            CommentDirectiveKind::DisableNextLine
        );
        assert!(result.comment_directives[1].affects_next_line);
    }

    /// @ai-generated - If chains convert correctly
    #[test]
    fn if_chains_converted() {
        let raw = RawTemplateData {
            if_chains: vec![RawIfChain {
                conditions: vec![
                    ("a".to_string(), 0, 10),
                    ("b".to_string(), 15, 25),
                    ("".to_string(), 30, 40),
                ],
            }],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);
        assert_eq!(result.if_chains.len(), 1);
        assert_eq!(result.if_chains[0].conditions.len(), 3);
    }

    /// @ai-generated - Max nesting depth preserved
    #[test]
    fn max_nesting_depth_preserved() {
        let raw = RawTemplateData {
            max_nesting_depth: 7,
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);
        assert_eq!(result.max_nesting_depth, 7);
    }

    /// @ai-generated - Usage kind mapping covers all values
    #[test]
    fn usage_kind_mapping() {
        let raw = RawTemplateData {
            binding_occurrences: vec![
                RawBindingOccurrence {
                    name: "a".to_string(),
                    span: Span::new(0, 1),
                    is_in_bindings_map: true,
                    usage_kind: 0,
                },
                RawBindingOccurrence {
                    name: "b".to_string(),
                    span: Span::new(5, 6),
                    is_in_bindings_map: true,
                    usage_kind: 1,
                },
                RawBindingOccurrence {
                    name: "c".to_string(),
                    span: Span::new(10, 11),
                    is_in_bindings_map: true,
                    usage_kind: 2,
                },
                RawBindingOccurrence {
                    name: "d".to_string(),
                    span: Span::new(15, 16),
                    is_in_bindings_map: true,
                    usage_kind: 3,
                },
                RawBindingOccurrence {
                    name: "e".to_string(),
                    span: Span::new(20, 21),
                    is_in_bindings_map: true,
                    usage_kind: 4,
                },
                RawBindingOccurrence {
                    name: "f".to_string(),
                    span: Span::new(25, 26),
                    is_in_bindings_map: true,
                    usage_kind: 5,
                },
            ],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);
        assert_eq!(
            result.binding_occurrences[0].usage_kind,
            BindingUsageKind::Interpolation
        );
        assert_eq!(
            result.binding_occurrences[1].usage_kind,
            BindingUsageKind::DirectiveValue
        );
        assert_eq!(
            result.binding_occurrences[2].usage_kind,
            BindingUsageKind::EventHandler
        );
        assert_eq!(
            result.binding_occurrences[3].usage_kind,
            BindingUsageKind::ComponentTag
        );
        assert_eq!(
            result.binding_occurrences[4].usage_kind,
            BindingUsageKind::TemplateRef
        );
        assert_eq!(
            result.binding_occurrences[5].usage_kind,
            BindingUsageKind::IteratorSource
        );
    }

    // =========================================================================
    // Binding class union resolution tests
    // =========================================================================

    fn make_element_with_dynamic_class(expr: &str) -> RawElementData {
        RawElementData {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            v_if_condition: None,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            span: Span::new(0, 50),
            tag_span_end: 30,
            content_end: 45,
            attributes: vec![RawAttributeData {
                name: "class".to_string(),
                value: Some(expr.to_string()),
                is_dynamic: true,
                span: Span::new(5, 20),
                name_end: 10,
                value_span: Some(Span::new(11, 19)),
            }],
            directives: vec![],
            v_for_idx: None,
            v_model_idx: None,
            text_children: vec![],
        }
    }

    #[test]
    fn element_class_bare_identifier_resolved_from_union() {
        let raw = RawTemplateData {
            elements: vec![make_element_with_dynamic_class("variant")],
            ..Default::default()
        };

        let unions = vec![(
            "variant".to_string(),
            vec!["primary".to_string(), "secondary".to_string()],
        )];
        let result = convert_raw_to_analysis(&raw, &[], &unions, None, None);

        assert_eq!(result.elements.len(), 1);
        assert_eq!(
            result.elements[0].dynamic_classes,
            vec!["primary", "secondary"],
            "bare identifier :class should resolve to string literal union values"
        );
    }

    #[test]
    fn element_class_bind_directive_resolved_from_union() {
        let raw = RawTemplateData {
            elements: vec![RawElementData {
                tag: "div".to_string(),
                is_component: false,
                is_self_closing: false,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                v_if_condition: None,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                span: Span::new(0, 50),
                tag_span_end: 30,
                content_end: 45,
                attributes: vec![],
                directives: vec![RawDirectiveData {
                    name: "bind".to_string(),
                    raw_name: ":".to_string(),
                    argument: Some("class".to_string()),
                    modifiers: vec![],
                    expression: Some("variant".to_string()),
                    span: Span::new(5, 20),
                    name_end: 6,
                    arg_span: Some(Span::new(6, 11)),
                    expression_span: Some(Span::new(12, 19)),
                    modifier_spans: vec![],
                }],
                v_for_idx: None,
                v_model_idx: None,
                text_children: vec![],
            }],
            ..Default::default()
        };

        let unions = vec![(
            "variant".to_string(),
            vec!["primary".to_string(), "secondary".to_string()],
        )];
        let result = convert_raw_to_analysis(&raw, &[], &unions, None, None);

        assert_eq!(
            result.elements[0].dynamic_classes,
            vec!["primary", "secondary"],
            "bind directive :class should resolve to string literal union values"
        );
    }

    #[test]
    fn element_style_bind_directive_extracts_css_vars() {
        let raw = RawTemplateData {
            elements: vec![RawElementData {
                tag: "div".to_string(),
                is_component: false,
                is_self_closing: false,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                v_if_condition: None,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                span: Span::new(0, 60),
                tag_span_end: 30,
                content_end: 55,
                attributes: vec![],
                directives: vec![RawDirectiveData {
                    name: "bind".to_string(),
                    raw_name: ":".to_string(),
                    argument: Some("style".to_string()),
                    modifiers: vec![],
                    expression: Some("{ '--theme-color': color }".to_string()),
                    span: Span::new(5, 35),
                    name_end: 6,
                    arg_span: Some(Span::new(6, 11)),
                    expression_span: Some(Span::new(12, 34)),
                    modifier_spans: vec![],
                }],
                v_for_idx: None,
                v_model_idx: None,
                text_children: vec![],
            }],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);

        assert_eq!(result.css_var_names, vec!["--theme-color"]);
        assert_eq!(result.elements[0].dynamic_style_vars.len(), 1);
        assert_eq!(
            result.elements[0].dynamic_style_vars[0].name,
            "--theme-color"
        );
    }

    #[test]
    fn element_class_props_member_access_resolved() {
        let raw = RawTemplateData {
            elements: vec![make_element_with_dynamic_class("props.variant")],
            ..Default::default()
        };

        let unions = vec![(
            "variant".to_string(),
            vec!["primary".to_string(), "secondary".to_string()],
        )];
        let result = convert_raw_to_analysis(&raw, &[], &unions, Some("props"), None);

        assert_eq!(
            result.elements[0].dynamic_classes,
            vec!["primary", "secondary"],
            "props.variant :class should resolve via props_binding_name"
        );
    }

    #[test]
    fn element_class_no_match_returns_empty() {
        let raw = RawTemplateData {
            elements: vec![make_element_with_dynamic_class("someVar")],
            ..Default::default()
        };

        let unions = vec![("variant".to_string(), vec!["primary".to_string()])];
        let result = convert_raw_to_analysis(&raw, &[], &unions, None, None);

        assert!(
            result.elements[0].dynamic_classes.is_empty(),
            "unmatched binding should not produce dynamic classes"
        );
    }

    #[test]
    fn element_class_object_syntax_not_overridden_by_union() {
        // When extract_dynamic_class_names succeeds, don't use union resolution
        let raw = RawTemplateData {
            elements: vec![make_element_with_dynamic_class("{ active: isActive }")],
            ..Default::default()
        };

        // Even though "active" matches, object syntax should take precedence
        let unions = vec![("active".to_string(), vec!["x".to_string()])];
        let result = convert_raw_to_analysis(&raw, &[], &unions, None, None);

        assert_eq!(
            result.elements[0].dynamic_classes,
            vec!["active"],
            "object syntax should take precedence over union resolution"
        );
    }

    #[test]
    fn component_class_bare_identifier_resolved_from_union() {
        let raw = RawTemplateData {
            components: vec![RawComponentUsage {
                tag_name: "MyComp".to_string(),
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: true,
                dynamic_class_expr: Some("variant".to_string()),
                bindings: vec![],
                events: vec![],
                span: Span::new(0, 50),
            }],
            ..Default::default()
        };

        let unions = vec![(
            "variant".to_string(),
            vec!["primary".to_string(), "secondary".to_string()],
        )];
        let result = convert_raw_to_analysis(&raw, &[], &unions, None, None);

        assert_eq!(
            result.components[0].dynamic_classes,
            vec!["primary", "secondary"],
            "component :class bare identifier should resolve from unions"
        );
    }

    #[test]
    fn component_element_gets_component_usage_index_from_span_match() {
        let raw = RawTemplateData {
            components: vec![RawComponentUsage {
                tag_name: "Child".to_string(),
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_class_expr: None,
                bindings: vec![],
                events: vec![],
                span: Span::new(10, 40),
            }],
            elements: vec![RawElementData {
                tag: "Child".to_string(),
                is_component: true,
                is_self_closing: true,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                v_if_condition: None,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                span: Span::new(10, 40),
                tag_span_end: 17,
                content_end: 40,
                attributes: vec![],
                directives: vec![],
                v_for_idx: None,
                v_model_idx: None,
                text_children: vec![],
            }],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None, None);

        assert_eq!(result.elements.len(), 1);
        assert_eq!(
            result.elements[0].component_usage_index,
            Some(0),
            "component elements should link to the matching TemplateComponentUsage by stable index"
        );
    }

    // =========================================================================
    // Unused-declaration population (props / emits / slots)
    // =========================================================================

    mod unused_declaration_population {
        use super::*;
        use verter_semantic::analysis::macro_usage::{MacroUsageCall, MacroUsageFacts};
        use verter_semantic::analysis::types::{
            AnalyzedEmitField, AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField,
            AnalyzedSlotField,
        };

        fn macro_of(kind: AnalyzedMacroKind) -> AnalyzedMacro {
            AnalyzedMacro {
                kind,
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_based: true,
                type_references: vec![],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                default_values: vec![],
                expose_fields: vec![],
                resolved_local_types: vec![],
                parsed_type_argument: None,
                parsed_type_argument_scope: None,
                span: verter_span::Span::new(0, 10),
            }
        }

        fn prop_field(name: &str, start: u32) -> AnalyzedPropField {
            AnalyzedPropField {
                name: name.to_string(),
                span: verter_span::Span::new(start, start + name.len() as u32),
                type_annotation: None,
                is_optional: false,
                description: None,
                tags: vec![],
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
                payload: None,
                type_expr_scope: None,
                declared_in_macro_type_arg: true,
            }
        }

        fn emit_field(name: &str, start: u32) -> AnalyzedEmitField {
            AnalyzedEmitField {
                name: name.to_string(),
                span: verter_span::Span::new(start, start + name.len() as u32),
                payload_type: None,
                payload: None,
                payload_expr_scope: None,
                description: None,
                tags: vec![],
            }
        }

        fn slot_field(name: &str, start: u32) -> AnalyzedSlotField {
            AnalyzedSlotField {
                name: name.to_string(),
                is_required: false,
                span: verter_span::Span::new(start, start + name.len() as u32),
                bindings: vec![],
                return_type: None,
                payload: None,
                return_expr_scope: None,
                description: None,
                tags: vec![],
            }
        }

        fn ctx<'a>(
            macros: &'a [AnalyzedMacro],
            usage: Option<&'a MacroUsageFacts>,
        ) -> UnusedDeclarationContext<'a> {
            UnusedDeclarationContext {
                macros,
                macro_usage: usage,
                use_slots_called: false,
                props_root_used_in_style: false,
                style_vbind_roots: &[],
            }
        }

        fn occurrence(name: &str) -> RawBindingOccurrence {
            RawBindingOccurrence {
                name: name.to_string(),
                span: Span::new(100, 100 + name.len() as u32),
                is_in_bindings_map: false,
                usage_kind: 0,
            }
        }

        #[test]
        fn props_populate_with_script_and_template_usage_split() {
            let mut mac = macro_of(AnalyzedMacroKind::DefineProps);
            mac.prop_fields = vec![
                prop_field("a", 10),
                prop_field("b", 20),
                prop_field("c", 30),
            ];
            let macros = vec![mac];
            let usage = MacroUsageFacts {
                props_member_reads: vec!["a".into()],
                ..Default::default()
            };
            let raw = RawTemplateData {
                binding_occurrences: vec![occurrence("b")],
                ..Default::default()
            };
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert_eq!(tpl.prop_definitions.len(), 3);
            let by_name = |n: &str| tpl.prop_definitions.iter().find(|p| p.name == n).unwrap();
            assert!(by_name("a").used_in_script && !by_name("a").used_in_template);
            assert!(!by_name("b").used_in_script && by_name("b").used_in_template);
            assert!(!by_name("c").used_in_script && !by_name("c").used_in_template);
            assert_eq!(by_name("c").span, verter_span::Span::new(30, 31));
        }

        #[test]
        fn props_fail_open_on_escape_destructure_style_or_dollar_props() {
            let mut mac = macro_of(AnalyzedMacroKind::DefineProps);
            mac.prop_fields = vec![prop_field("a", 10)];
            let macros = vec![mac];

            // Escape.
            let usage = MacroUsageFacts {
                props_escapes: true,
                ..Default::default()
            };
            let tpl = convert_raw_to_analysis(
                &RawTemplateData::default(),
                &[],
                &[],
                None,
                Some(&ctx(&macros, Some(&usage))),
            );
            assert!(tpl.prop_definitions.is_empty(), "escape must suppress");

            // Destructured defineProps — provider-owned TS6133.
            let usage = MacroUsageFacts {
                props_destructured: true,
                ..Default::default()
            };
            let tpl = convert_raw_to_analysis(
                &RawTemplateData::default(),
                &[],
                &[],
                None,
                Some(&ctx(&macros, Some(&usage))),
            );
            assert!(tpl.prop_definitions.is_empty(), "destructure must suppress");

            // Style v-bind on the props root.
            let usage = MacroUsageFacts::default();
            let mut c = ctx(&macros, Some(&usage));
            c.props_root_used_in_style = true;
            let tpl =
                convert_raw_to_analysis(&RawTemplateData::default(), &[], &[], None, Some(&c));
            assert!(tpl.prop_definitions.is_empty(), "style use must suppress");

            // `$props` referenced in the template.
            let raw = RawTemplateData {
                binding_occurrences: vec![occurrence("$props")],
                ..Default::default()
            };
            let usage = MacroUsageFacts::default();
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert!(tpl.prop_definitions.is_empty(), "$props must suppress");
        }

        #[test]
        fn template_props_member_reads_count_per_member_and_bare_root_escapes() {
            let mut mac = macro_of(AnalyzedMacroKind::DefineProps);
            mac.binding_name = Some("props".to_string());
            mac.prop_fields = vec![prop_field("live", 10), prop_field("dead", 20)];
            let macros = vec![mac];
            let usage = MacroUsageFacts::default();

            // `{{ props.live }}`: the root occurrence is consumed by a member
            // read — `live` is template-used, `dead` stays unused.
            let raw = RawTemplateData {
                binding_occurrences: vec![RawBindingOccurrence {
                    name: "props".to_string(),
                    span: Span::new(100, 105),
                    is_in_bindings_map: true,
                    usage_kind: 0,
                }],
                member_reads: vec![RawMemberRead {
                    root: "props".to_string(),
                    member: "live".to_string(),
                    root_span: Span::new(100, 105),
                }],
                ..Default::default()
            };
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert_eq!(tpl.prop_definitions.len(), 2);
            let by_name = |n: &str| tpl.prop_definitions.iter().find(|p| p.name == n).unwrap();
            assert!(
                by_name("live").used_in_template,
                "props.live is a template read"
            );
            assert!(!by_name("dead").used_in_template);

            // Bare `v-bind="props"`: an UNCONSUMED root occurrence is a
            // whole-object escape — suppress every prop diagnostic.
            let raw = RawTemplateData {
                binding_occurrences: vec![RawBindingOccurrence {
                    name: "props".to_string(),
                    span: Span::new(200, 205),
                    is_in_bindings_map: true,
                    usage_kind: 1,
                }],
                ..Default::default()
            };
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert!(
                tpl.prop_definitions.is_empty(),
                "bare template use of the props root must suppress"
            );
        }

        #[test]
        fn template_dollar_slots_member_read_counts_that_slot_used() {
            let mut mac = macro_of(AnalyzedMacroKind::DefineSlots);
            mac.slot_fields = vec![slot_field("header", 10), slot_field("footer", 30)];
            let macros = vec![mac];
            let usage = MacroUsageFacts::default();

            // `v-if="$slots.header"`: a literal member read marks `header`
            // used without an outlet; `footer` stays unused.
            let raw = RawTemplateData {
                binding_occurrences: vec![RawBindingOccurrence {
                    name: "$slots".to_string(),
                    span: Span::new(100, 106),
                    is_in_bindings_map: false,
                    usage_kind: 1,
                }],
                member_reads: vec![RawMemberRead {
                    root: "$slots".to_string(),
                    member: "header".to_string(),
                    root_span: Span::new(100, 106),
                }],
                ..Default::default()
            };
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert_eq!(tpl.slot_declarations.len(), 2);
            let by_name = |n: &str| tpl.slot_declarations.iter().find(|s| s.name == n).unwrap();
            assert!(by_name("header").used);
            assert!(!by_name("footer").used);
        }

        #[test]
        fn non_author_local_prop_members_are_skipped() {
            let mut mac = macro_of(AnalyzedMacroKind::DefineProps);
            let mut foreign = prop_field("ext", 40);
            foreign.declared_in_macro_type_arg = false;
            mac.prop_fields = vec![prop_field("a", 10), foreign];
            let macros = vec![mac];
            let usage = MacroUsageFacts::default();
            let tpl = convert_raw_to_analysis(
                &RawTemplateData::default(),
                &[],
                &[],
                None,
                Some(&ctx(&macros, Some(&usage))),
            );
            assert_eq!(tpl.prop_definitions.len(), 1);
            assert_eq!(tpl.prop_definitions[0].name, "a");
        }

        #[test]
        fn emits_populate_with_literal_call_locations() {
            let mut mac = macro_of(AnalyzedMacroKind::DefineEmits);
            mac.emit_fields = vec![emit_field("save", 10), emit_field("close", 20)];
            let macros = vec![mac];
            let usage = MacroUsageFacts {
                emit_literal_calls: vec![MacroUsageCall {
                    name: "save".into(),
                    span: verter_span::Span::new(60, 72),
                }],
                ..Default::default()
            };
            let tpl = convert_raw_to_analysis(
                &RawTemplateData::default(),
                &[],
                &[],
                None,
                Some(&ctx(&macros, Some(&usage))),
            );
            assert_eq!(tpl.emit_definitions.len(), 2);
            let by_name = |n: &str| {
                tpl.emit_definitions
                    .iter()
                    .find(|e| e.event_name == n)
                    .unwrap()
            };
            assert_eq!(by_name("save").emit_locations, vec![(60, 72)]);
            assert!(
                by_name("close").emit_locations.is_empty(),
                "unused event stays empty"
            );
            assert!(by_name("close").is_declared);
        }

        #[test]
        fn emits_fail_open_on_escape_or_template_dollar_emit() {
            let mut mac = macro_of(AnalyzedMacroKind::DefineEmits);
            mac.emit_fields = vec![emit_field("save", 10)];
            let macros = vec![mac];

            let usage = MacroUsageFacts {
                emit_escapes: true,
                ..Default::default()
            };
            let tpl = convert_raw_to_analysis(
                &RawTemplateData::default(),
                &[],
                &[],
                None,
                Some(&ctx(&macros, Some(&usage))),
            );
            assert!(tpl.emit_definitions.is_empty(), "emit escape must suppress");

            let raw = RawTemplateData {
                binding_occurrences: vec![occurrence("$emit")],
                ..Default::default()
            };
            let usage = MacroUsageFacts::default();
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert!(
                tpl.emit_definitions.is_empty(),
                "template $emit must suppress"
            );
        }

        #[test]
        fn emits_fail_open_on_template_use_of_the_emit_binding() {
            // `@click="emit('close')"` (or `:handler="emit"`) — the standard
            // template-emit pattern calls the `defineEmits` RETURN BINDING.
            // Any template occurrence of that binding name must suppress the
            // whole kind (per-name template extraction stays deferred).
            let mut mac = macro_of(AnalyzedMacroKind::DefineEmits);
            mac.binding_name = Some("emit".to_string());
            mac.emit_fields = vec![emit_field("close", 10)];
            let macros = vec![mac];
            let raw = RawTemplateData {
                binding_occurrences: vec![occurrence("emit")],
                ..Default::default()
            };
            let usage = MacroUsageFacts::default();
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert!(
                tpl.emit_definitions.is_empty(),
                "a template occurrence of the emit binding must suppress unused-emit \
                 diagnostics (fail-open), got: {:?}",
                tpl.emit_definitions
            );
        }

        #[test]
        fn style_vbind_root_matching_a_prop_name_marks_that_member_live() {
            // `<style> .x { color: v-bind(color) } </style>` with
            // non-destructured `defineProps<{ color; dead }>()`: `color` is
            // live through the render context; `dead` still surfaces —
            // a per-member fact, not a whole-kind suppression.
            let mut mac = macro_of(AnalyzedMacroKind::DefineProps);
            mac.prop_fields = vec![prop_field("color", 10), prop_field("dead", 20)];
            let macros = vec![mac];
            let usage = MacroUsageFacts::default();
            let mut c = ctx(&macros, Some(&usage));
            let roots = vec!["color".to_string()];
            c.style_vbind_roots = &roots;
            let tpl =
                convert_raw_to_analysis(&RawTemplateData::default(), &[], &[], None, Some(&c));
            let by_name = |n: &str| tpl.prop_definitions.iter().find(|p| p.name == n).unwrap();
            assert!(
                by_name("color").used_in_script,
                "style v-bind(color) keeps the prop live"
            );
            assert!(
                !by_name("dead").used_in_script && !by_name("dead").used_in_template,
                "the style fact is per-member — dead stays flagged"
            );
        }

        #[test]
        fn define_model_update_event_is_self_consuming() {
            let mut emits = macro_of(AnalyzedMacroKind::DefineEmits);
            emits.emit_fields = vec![emit_field("update:title", 10), emit_field("save", 30)];
            let mut model = macro_of(AnalyzedMacroKind::DefineModel);
            model.model_name = Some("title".into());
            let macros = vec![emits, model];
            let usage = MacroUsageFacts::default();
            let tpl = convert_raw_to_analysis(
                &RawTemplateData::default(),
                &[],
                &[],
                None,
                Some(&ctx(&macros, Some(&usage))),
            );
            let names: Vec<&str> = tpl
                .emit_definitions
                .iter()
                .map(|e| e.event_name.as_str())
                .collect();
            assert_eq!(
                names,
                ["save"],
                "update:title is defineModel-consumed, never flagged"
            );
        }

        #[test]
        fn slots_populate_from_outlets_and_fail_open_on_dynamic_use_slots_or_dollar_slots() {
            let mut mac = macro_of(AnalyzedMacroKind::DefineSlots);
            mac.slot_fields = vec![slot_field("default", 10), slot_field("header", 30)];
            let macros = vec![mac];
            let usage = MacroUsageFacts::default();

            // Outlet for `default` exists; `header` has none → unused.
            let raw = RawTemplateData {
                slot_definitions: vec![RawSlotDef {
                    name: "default".to_string(),
                    has_bindings: false,
                    binding_names: vec![],
                    binding_expressions: vec![],
                    binding_value_spans: vec![],
                    has_fallback_content: false,
                    span: Span::new(200, 210),
                }],
                ..Default::default()
            };
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert_eq!(tpl.slot_declarations.len(), 2);
            let by_name = |n: &str| tpl.slot_declarations.iter().find(|s| s.name == n).unwrap();
            assert!(by_name("default").used);
            assert!(!by_name("header").used);

            // Dynamic outlet suppresses everything.
            let raw = RawTemplateData {
                has_dynamic_slot_outlet: true,
                ..Default::default()
            };
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert!(
                tpl.slot_declarations.is_empty(),
                "dynamic outlet must suppress"
            );
            assert!(tpl.has_dynamic_slot_outlet);

            // useSlots() suppresses everything.
            let mut c = ctx(&macros, Some(&usage));
            c.use_slots_called = true;
            let tpl =
                convert_raw_to_analysis(&RawTemplateData::default(), &[], &[], None, Some(&c));
            assert!(tpl.slot_declarations.is_empty(), "useSlots must suppress");

            // `$slots` in the template suppresses everything.
            let raw = RawTemplateData {
                binding_occurrences: vec![occurrence("$slots")],
                ..Default::default()
            };
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert!(tpl.slot_declarations.is_empty(), "$slots must suppress");
        }

        #[test]
        fn expression_errors_fail_open_for_every_kind() {
            let mut props = macro_of(AnalyzedMacroKind::DefineProps);
            props.prop_fields = vec![prop_field("a", 10)];
            let mut emits = macro_of(AnalyzedMacroKind::DefineEmits);
            emits.emit_fields = vec![emit_field("save", 20)];
            let mut slots = macro_of(AnalyzedMacroKind::DefineSlots);
            slots.slot_fields = vec![slot_field("default", 30)];
            let macros = vec![props, emits, slots];
            let usage = MacroUsageFacts::default();
            let raw = RawTemplateData {
                has_expression_errors: true,
                ..Default::default()
            };
            let tpl =
                convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
            assert!(tpl.has_expression_errors);
            assert!(tpl.prop_definitions.is_empty());
            assert!(tpl.emit_definitions.is_empty());
            assert!(tpl.slot_declarations.is_empty());
        }

        #[test]
        fn no_usage_facts_populates_nothing() {
            let mut mac = macro_of(AnalyzedMacroKind::DefineProps);
            mac.prop_fields = vec![prop_field("a", 10)];
            let macros = vec![mac];
            let tpl = convert_raw_to_analysis(
                &RawTemplateData::default(),
                &[],
                &[],
                None,
                Some(&ctx(&macros, None)),
            );
            assert!(tpl.prop_definitions.is_empty(), "no facts => fail open");
        }
    }
}
