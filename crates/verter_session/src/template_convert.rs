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
#[path = "template_convert_tests.rs"]
mod tests;
