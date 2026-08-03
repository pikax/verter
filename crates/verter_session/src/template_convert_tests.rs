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
        AnalyzedEmitField, AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
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
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
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
        let tpl = convert_raw_to_analysis(&RawTemplateData::default(), &[], &[], None, Some(&c));
        assert!(tpl.prop_definitions.is_empty(), "style use must suppress");

        // `$props` referenced in the template.
        let raw = RawTemplateData {
            binding_occurrences: vec![occurrence("$props")],
            ..Default::default()
        };
        let usage = MacroUsageFacts::default();
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
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
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
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
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
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
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
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
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
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
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
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
        let tpl = convert_raw_to_analysis(&RawTemplateData::default(), &[], &[], None, Some(&c));
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
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
        assert_eq!(tpl.slot_declarations.len(), 2);
        let by_name = |n: &str| tpl.slot_declarations.iter().find(|s| s.name == n).unwrap();
        assert!(by_name("default").used);
        assert!(!by_name("header").used);

        // Dynamic outlet suppresses everything.
        let raw = RawTemplateData {
            has_dynamic_slot_outlet: true,
            ..Default::default()
        };
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
        assert!(
            tpl.slot_declarations.is_empty(),
            "dynamic outlet must suppress"
        );
        assert!(tpl.has_dynamic_slot_outlet);

        // useSlots() suppresses everything.
        let mut c = ctx(&macros, Some(&usage));
        c.use_slots_called = true;
        let tpl = convert_raw_to_analysis(&RawTemplateData::default(), &[], &[], None, Some(&c));
        assert!(tpl.slot_declarations.is_empty(), "useSlots must suppress");

        // `$slots` in the template suppresses everything.
        let raw = RawTemplateData {
            binding_occurrences: vec![occurrence("$slots")],
            ..Default::default()
        };
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
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
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
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

    /// `const props = withDefaults(defineProps<{ title: string; dead: number }>(), …)`
    /// as the analyzer actually records it: the INNER `DefineProps` (binding
    /// `None`, carrying the prop fields) is pushed BEFORE the OUTER
    /// `WithDefaults` (binding `Some("props")`).
    fn bound_with_defaults_macros() -> Vec<AnalyzedMacro> {
        let mut inner = macro_of(AnalyzedMacroKind::DefineProps);
        inner.prop_fields = vec![prop_field("title", 10), prop_field("dead", 20)];
        let mut outer = macro_of(AnalyzedMacroKind::WithDefaults);
        outer.binding_name = Some("props".to_string());
        vec![inner, outer]
    }

    #[test]
    fn with_defaults_bound_root_template_member_read_marks_prop_used() {
        // The props root lives on the OUTER `WithDefaults` macro; selecting it
        // must skip the inner `DefineProps`' `None` binding.
        let macros = bound_with_defaults_macros();
        let usage = MacroUsageFacts::default();
        let raw = RawTemplateData {
            binding_occurrences: vec![RawBindingOccurrence {
                name: "props".to_string(),
                span: Span::new(100, 105),
                is_in_bindings_map: true,
                usage_kind: 0,
            }],
            member_reads: vec![RawMemberRead {
                root: "props".to_string(),
                member: "title".to_string(),
                root_span: Span::new(100, 105),
            }],
            ..Default::default()
        };
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
        let by_name = |n: &str| tpl.prop_definitions.iter().find(|p| p.name == n).unwrap();
        assert!(
            by_name("title").used_in_template,
            "`{{ props.title }}` must attribute to the bound withDefaults root"
        );
        assert!(
            !by_name("dead").used_in_template && !by_name("dead").used_in_script,
            "the unread member still surfaces — the fix must not fail open"
        );
    }

    #[test]
    fn with_defaults_bound_root_bare_template_use_escapes() {
        // `v-bind="props"` — an UNCONSUMED root occurrence is a whole-object
        // escape: suppress every prop diagnostic.
        let macros = bound_with_defaults_macros();
        let usage = MacroUsageFacts::default();
        let raw = RawTemplateData {
            binding_occurrences: vec![RawBindingOccurrence {
                name: "props".to_string(),
                span: Span::new(200, 205),
                is_in_bindings_map: true,
                usage_kind: 1,
            }],
            ..Default::default()
        };
        let tpl = convert_raw_to_analysis(&raw, &[], &[], None, Some(&ctx(&macros, Some(&usage))));
        assert!(
            tpl.prop_definitions.is_empty(),
            "a bare template use of the bound withDefaults root must suppress, got: {:?}",
            tpl.prop_definitions
        );
    }

    #[test]
    fn with_defaults_bound_root_used_in_style_is_detected() {
        // `<style> .x { color: v-bind(props.color) } </style>` — the root
        // binding referenced from style must arm the whole-kind suppression.
        let macros = bound_with_defaults_macros();
        let bindings = vec![verter_semantic::analysis::types::AnalyzedBinding {
            name: "props".to_string(),
            kind: verter_semantic::analysis::types::AnalyzedBindingKind::Const,
            is_reactive: false,
            reactivity_kind: verter_semantic::analysis::types::ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(6, 11),
            used_in_script: true,
            used_in_style: true,
        }];
        let context = UnusedDeclarationContext::from_analysis(&macros, None, &[], &bindings, &[]);
        assert!(
            context.props_root_used_in_style,
            "the bound withDefaults root referenced from style must be detected"
        );
    }
}
