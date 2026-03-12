//! Convert raw template data from `verter_core` into `verter_analysis` types.
//!
//! This module bridges the two independent crates: `verter_core` produces
//! [`RawTemplateData`] during compilation, and this function converts it into
//! [`TemplateAnalysisSnapshot`] that `verter_host` stores alongside script/style analysis.

use verter_analysis::template::{
    BindingUsageKind, CommentDirective, CommentDirectiveKind, DefinedSlot, ElementNamespace,
    IfChain, PropValueConstness, TemplateAnalysisSnapshot, TemplateAttribute,
    TemplateBindingOccurrence, TemplateComponentUsage, TemplateComponentVModel, TemplateDirective,
    TemplateElement, TemplateEventHandler, TemplatePropUsage, TemplateRef, TemplateTextSegment,
    UnresolvedBinding, VForDirective, VModelDirective,
};
use verter_core::compile::template_data::RawTemplateData;

/// Convert raw template data from `verter_core` into `verter_analysis` types.
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
) -> TemplateAnalysisSnapshot {
    let components = raw
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
                        constness,
                        referenced_bindings: p.referenced_bindings.clone(),
                        from_spread: p.from_spread,
                        span: p.span,
                    }
                })
                .collect();

            // Extract class names from :class object syntax
            let mut dynamic_classes = c
                .dynamic_class_expr
                .as_deref()
                .map(verter_analysis::extract_dynamic_class_names)
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
                .flat_map(verter_analysis::extract_dynamic_class_names)
                .collect();

            // If no class names from object/array/ternary, try resolving
            // bare identifier bindings via string literal union types.
            if dynamic_classes.is_empty() {
                for attr in &e.attributes {
                    if attr.is_dynamic && attr.name == "class" {
                        if let Some(expr) = attr.value.as_deref() {
                            if let Some(classes) = resolve_classes_from_binding(
                                expr,
                                binding_class_unions,
                                props_binding_name,
                            ) {
                                dynamic_classes.extend_from_slice(classes);
                            }
                        }
                    }
                }
            }

            // Extract CSS variables from :style bindings (e.g., { '--color': val })
            let dynamic_style_vars: Vec<verter_analysis::template::DynamicStyleVar> = e
                .attributes
                .iter()
                .filter(|a| a.is_dynamic && a.name == "style")
                .filter_map(|a| a.value.as_deref())
                .flat_map(verter_analysis::template::extract_dynamic_style_vars)
                .collect();

            // Extract CSS variables from static style attributes (e.g., style="--color: red")
            let static_style_vars: Vec<verter_analysis::template::StaticStyleVar> = e
                .attributes
                .iter()
                .filter(|a| !a.is_dynamic && a.name == "style")
                .filter_map(|a| a.value.as_deref())
                .flat_map(verter_analysis::template::extract_static_style_vars)
                .collect();

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
                span: e.span,
                tag_span_end: e.tag_span_end,
                content_end: e.content_end,
                text_children: e
                    .text_children
                    .iter()
                    .map(|seg| match seg {
                        verter_core::compile::template_data::RawTextSegment::Text {
                            span,
                            is_entity,
                        } => TemplateTextSegment::Text {
                            span: *span,
                            is_entity: *is_entity,
                        },
                        verter_core::compile::template_data::RawTextSegment::Interpolation {
                            span,
                            expression_span,
                        } => TemplateTextSegment::Interpolation {
                            span: *span,
                            expression_span: *expression_span,
                        },
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

    TemplateAnalysisSnapshot {
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
        // prop/emit definitions and type_enhancements come from script analysis.
        ..Default::default()
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
    use verter_core::common::Span;
    use verter_core::compile::template_data::*;

    /// @ai-generated - Empty raw data converts to empty snapshot
    #[test]
    fn empty_raw_converts_to_empty_snapshot() {
        let raw = RawTemplateData::default();
        let result = convert_raw_to_analysis(&raw, &[], &[], None);

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
                }],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_class_expr: None,
                span: Span::new(10, 40),
            }],
            ..Default::default()
        };

        let imports = vec![("Child".to_string(), "./Child.vue".to_string())];
        let result = convert_raw_to_analysis(&raw, &imports, &[], None);

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
                span: Span::new(0, 20),
            }],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None);
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
                span: Span::new(0, 20),
            }],
            ..Default::default()
        };

        let imports = vec![("MyHeader".to_string(), "./MyHeader.vue".to_string())];
        let result = convert_raw_to_analysis(&raw, &imports, &[], None);

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

        let result = convert_raw_to_analysis(&raw, &[], &[], None);
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
                    },
                    RawPropData {
                        name: "const_bound".to_string(),
                        is_bound: true,
                        expression: Some("LABEL".to_string()),
                        referenced_bindings: vec!["LABEL".to_string()],
                        all_bindings_static: Some(true),
                        from_spread: false,
                        span: Span::new(0, 0),
                    },
                    RawPropData {
                        name: "dynamic_bound".to_string(),
                        is_bound: true,
                        expression: Some("count".to_string()),
                        referenced_bindings: vec!["count".to_string()],
                        all_bindings_static: Some(false),
                        from_spread: false,
                        span: Span::new(0, 0),
                    },
                    RawPropData {
                        name: "".to_string(),
                        is_bound: true,
                        expression: None,
                        referenced_bindings: vec![],
                        all_bindings_static: None,
                        from_spread: true,
                        span: Span::new(0, 0),
                    },
                ],
                has_spread: true,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_class_expr: None,
                span: Span::new(0, 50),
            }],
            ..Default::default()
        };

        let result = convert_raw_to_analysis(&raw, &[], &[], None);
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

        let result = convert_raw_to_analysis(&raw, &[], &[], None);
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

        let result = convert_raw_to_analysis(&raw, &[], &[], None);
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

        let result = convert_raw_to_analysis(&raw, &[], &[], None);
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

        let result = convert_raw_to_analysis(&raw, &[], &[], None);
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

        let result = convert_raw_to_analysis(&raw, &[], &[], None);
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

        let result = convert_raw_to_analysis(&raw, &[], &[], None);
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
        let result = convert_raw_to_analysis(&raw, &[], &unions, None);

        assert_eq!(result.elements.len(), 1);
        assert_eq!(
            result.elements[0].dynamic_classes,
            vec!["primary", "secondary"],
            "bare identifier :class should resolve to string literal union values"
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
        let result = convert_raw_to_analysis(&raw, &[], &unions, Some("props"));

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
        let result = convert_raw_to_analysis(&raw, &[], &unions, None);

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
        let result = convert_raw_to_analysis(&raw, &[], &unions, None);

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
                span: Span::new(0, 50),
            }],
            ..Default::default()
        };

        let unions = vec![(
            "variant".to_string(),
            vec!["primary".to_string(), "secondary".to_string()],
        )];
        let result = convert_raw_to_analysis(&raw, &[], &unions, None);

        assert_eq!(
            result.components[0].dynamic_classes,
            vec!["primary", "secondary"],
            "component :class bare identifier should resolve from unions"
        );
    }
}
