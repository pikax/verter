use super::*;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use verter_analysis::*;

fn make_analysis(
    bindings: Vec<AnalyzedBinding>,
    imports: Vec<AnalyzedImport>,
    macros: Vec<AnalyzedMacro>,
) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        bindings,
        imports,
        macros,
        ..Default::default()
    }
}

#[test]
fn test_go_to_definition_from_template_to_script_via_span() {
    let source =
        "<template>\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // "const count" in script — find the byte offset of "count" in the declaration
    let script_count_offset = source.rfind("count").unwrap() as u32;
    let script_count_end = script_count_offset + 5;

    let analysis = make_analysis(
        vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(script_count_offset, script_count_end),
        }],
        vec![],
        vec![],
    );

    // Click on "count" in template
    let template_count_offset = source.find("count").unwrap();
    let position = line_index
        .offset_to_position(template_count_offset as u32)
        .unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(result.is_some());

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        // Should point to the "count" declaration span in script
        assert_eq!(loc.range.start.line, 5);
        assert_eq!(loc.range.start.character, 6); // after "const "
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_go_to_import_with_resolved_canonical_id() {
    let source = "<script setup>\nimport { ref } from 'vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "ref".to_string(),
                is_type_only: false,
                vue_api: Some(VueApiClassification::Ref),
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(15, 40),
            resolved_canonical_id: Some("/usr/lib/node_modules/vue/dist/vue.d.ts".to_string()),
        }],
        vec![],
    );

    let ref_offset = source.find("ref").unwrap();
    let position = line_index.offset_to_position(ref_offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(result.is_some());

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        // Should navigate to the resolved file
        assert!(loc.uri.as_str().contains("vue.d.ts"));
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_go_to_import_without_resolution_falls_back_to_import_span() {
    let source = "<script setup>\nimport { helper } from './utils'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let import_start = source.find("import").unwrap() as u32;
    let import_end = source.find("'./utils'").unwrap() as u32 + 9;

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "./utils".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "helper".to_string(),
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(import_start, import_end),
            resolved_canonical_id: None,
        }],
        vec![],
    );

    let helper_offset = source.find("helper").unwrap();
    let position = line_index.offset_to_position(helper_offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(result.is_some());

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        // Should point to the import statement span
        let start_pos = line_index.offset_to_position(import_start).unwrap();
        assert_eq!(loc.range.start, start_pos);
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_go_to_macro_binding_from_template() {
    let source = "<template>\n  {{ props.msg }}\n</template>\n\n<script setup>\nconst props = defineProps<{ msg: string }>()\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let macro_start = source.find("defineProps").unwrap() as u32;
    let macro_end = source.rfind("()").unwrap() as u32 + 2;

    let analysis = make_analysis(
        vec![],
        vec![],
        vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec![],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            span: verter_span::Span::new(macro_start, macro_end),
        }],
    );

    // Click on "props" in template
    let props_offset = source.find("props").unwrap();
    let position = line_index.offset_to_position(props_offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(result.is_some());

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let expected_start = line_index.offset_to_position(macro_start).unwrap();
        assert_eq!(loc.range.start, expected_start);
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_no_definition_for_unknown_binding() {
    let source =
        "<template>\n  {{ unknown }}\n</template>\n\n<script setup>\nconst x = 1\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![AnalyzedBinding {
            name: "x".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(0, 0),
        }],
        vec![],
        vec![],
    );

    let offset = source.find("unknown").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(result.is_none());
}

#[test]
fn test_no_definition_inside_html_comment() {
    let source = "<template>\n  <!-- MyComponent -->\n  {{ count }}\n</template>\n\n<script setup>\nimport MyComponent from './MyComponent.vue'\nconst count = ref(0)\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(0, 0),
        }],
        vec![AnalyzedImport {
            source: "./MyComponent.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "MyComponent".to_string(),
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: Some("/project/MyComponent.vue".to_string()),
        }],
        vec![],
    );

    // Click on "MyComponent" inside the comment
    let offset = source.find("MyComponent").unwrap();
    assert!(
        source[..offset].contains("<!--"),
        "should be inside comment"
    );
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(
        result.is_none(),
        "should not navigate from inside HTML comment"
    );
}

#[test]
fn test_is_inside_html_comment() {
    let source = "<div><!-- hello --> world <!-- bye --></div>";
    // Inside first comment
    let offset = source.find("hello").unwrap();
    assert!(is_inside_html_comment(source, offset));

    // Between comments (after first -->)
    let offset = source.find("world").unwrap();
    assert!(!is_inside_html_comment(source, offset));

    // Inside second comment
    let offset = source.find("bye").unwrap();
    assert!(is_inside_html_comment(source, offset));

    // Before any comment
    assert!(!is_inside_html_comment(source, 1));
}

#[test]
fn test_go_to_component_definition_from_template() {
    let source = "<template>\n  <ChildComp />\n</template>\n\n<script setup>\nimport ChildComp from './ChildComp.vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_analysis::template::*;

    let analysis = FileAnalysisSnapshot {
        imports: vec![AnalyzedImport {
            source: "./ChildComp.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "ChildComp".to_string(),
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: Some("/project/ChildComp.vue".to_string()),
        }],
        template: Some(TemplateAnalysisSnapshot {
            components: vec![TemplateComponentUsage {
                name: "ChildComp".to_string(),
                import_source: Some("./ChildComp.vue".to_string()),
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                span: verter_span::Span::new(0, 0),
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    // Click on "ChildComp" in template
    let offset = source.find("ChildComp").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(result.is_some(), "should navigate to component file");

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(loc.uri.as_str().contains("ChildComp.vue"));
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_to_pascal_case() {
    assert_eq!(to_pascal_case("my-header"), "MyHeader");
    assert_eq!(to_pascal_case("my_comp"), "MyComp");
    assert_eq!(to_pascal_case("already"), "Already");
    assert_eq!(to_pascal_case("a-b-c"), "ABC");
}

// =====================================================================
// CSS Navigation Tests (template ↔ style)
// =====================================================================

#[test]
fn test_css_nav_template_class_to_style() {
    let source = "<template>\n  <div class=\"btn\"></div>\n</template>\n\n<style>\n.btn { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_analysis::style::*;
    use verter_analysis::template::*;

    // Find the offsets for the style block content
    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css =
        &source[style_block.content_range().0 as usize..style_block.content_range().1 as usize];

    // Build analysis with template element + style analysis
    let class_attr_start = source.find("class=\"btn\"").unwrap() as u32;
    let class_attr_end = class_attr_start + "class=\"btn\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".to_string(),
                    value: Some("btn".to_string()),
                    is_dynamic: false,
                    span: verter_span::Span::new(class_attr_start, class_attr_end),
                    name_end: 0,
                    value_span: None,
                }],
                directives: vec![],
                v_for: None,
                v_model: None,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: verter_span::Span::new(0, 0),
                tag_span_end: 0,
                content_end: 0,
                text_children: Vec::new(),
            }],
            ..Default::default()
        }),
        styles: vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )],
        ..Default::default()
    };

    // Click on "btn" in class="btn"
    let btn_offset = source.find("btn").unwrap();
    let position = line_index.offset_to_position(btn_offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(
        result.is_some(),
        "should navigate from template class to style"
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        // Should point to "btn" inside .btn { } in style
        let style_btn_offset = source.rfind("btn").unwrap();
        let expected_pos = line_index
            .offset_to_position(style_btn_offset as u32)
            .unwrap();
        assert_eq!(loc.range.start, expected_pos);
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_css_nav_multi_class_attr() {
    let source = "<template>\n  <div class=\"btn primary\"></div>\n</template>\n\n<style>\n.btn { } .primary { }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_analysis::style::*;
    use verter_analysis::template::*;

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css = &source[style_content_start as usize..style_block.content_range().1 as usize];

    let class_attr_start = source.find("class=\"btn primary\"").unwrap() as u32;
    let class_attr_end = class_attr_start + "class=\"btn primary\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".to_string(),
                    value: Some("btn primary".to_string()),
                    is_dynamic: false,
                    span: verter_span::Span::new(class_attr_start, class_attr_end),
                    name_end: 0,
                    value_span: None,
                }],
                directives: vec![],
                v_for: None,
                v_model: None,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: verter_span::Span::new(0, 0),
                tag_span_end: 0,
                content_end: 0,
                text_children: Vec::new(),
            }],
            ..Default::default()
        }),
        styles: vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )],
        ..Default::default()
    };

    // Click on "primary" in class="btn primary"
    let primary_offset = source.find("primary").unwrap();
    let position = line_index
        .offset_to_position(primary_offset as u32)
        .unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(result.is_some(), "should navigate to .primary in style");

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        // Should point to "primary" inside .primary { } in style
        let style_primary_offset = source.rfind("primary").unwrap();
        let expected_pos = line_index
            .offset_to_position(style_primary_offset as u32)
            .unwrap();
        assert_eq!(loc.range.start, expected_pos);
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_css_nav_template_id_to_style() {
    let source = "<template>\n  <div id=\"app\"></div>\n</template>\n\n<style>\n#app { margin: 0; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_analysis::style::*;
    use verter_analysis::template::*;

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css = &source[style_content_start as usize..style_block.content_range().1 as usize];

    let id_attr_start = source.find("id=\"app\"").unwrap() as u32;
    let id_attr_end = id_attr_start + "id=\"app\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "id".to_string(),
                    value: Some("app".to_string()),
                    is_dynamic: false,
                    span: verter_span::Span::new(id_attr_start, id_attr_end),
                    name_end: 0,
                    value_span: None,
                }],
                directives: vec![],
                v_for: None,
                v_model: None,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: verter_span::Span::new(0, 0),
                tag_span_end: 0,
                content_end: 0,
                text_children: Vec::new(),
            }],
            ..Default::default()
        }),
        styles: vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )],
        ..Default::default()
    };

    // Click on "app" in id="app"
    let app_offset = source.find("app").unwrap();
    let position = line_index.offset_to_position(app_offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(
        result.is_some(),
        "should navigate from template id to style"
    );
}

#[test]
fn test_css_nav_dynamic_class_skipped() {
    let source = "<template>\n  <div :class=\"{ active: true }\"></div>\n</template>\n\n<style>\n.active { }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_analysis::style::*;
    use verter_analysis::template::*;

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (scs, _) = style_block.content_range();
    let style_css = &source[scs as usize..style_block.content_range().1 as usize];

    let attr_start = source.find(":class").unwrap() as u32;
    let attr_end = attr_start + ":class=\"{ active: true }\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".to_string(),
                    value: Some("{ active: true }".to_string()),
                    is_dynamic: true,
                    span: verter_span::Span::new(attr_start, attr_end),
                    name_end: 0,
                    value_span: None,
                }],
                directives: vec![],
                v_for: None,
                v_model: None,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: verter_span::Span::new(0, 0),
                tag_span_end: 0,
                content_end: 0,
                text_children: Vec::new(),
            }],
            ..Default::default()
        }),
        styles: vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            scs,
        )],
        ..Default::default()
    };

    // Click on "active" inside :class
    let active_offset = source.find("active").unwrap();
    let position = line_index.offset_to_position(active_offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    // Should NOT navigate — it's a dynamic class binding
    assert!(
        result.is_none(),
        "dynamic :class should not trigger CSS navigation"
    );
}

#[test]
fn test_css_nav_style_to_template() {
    let source = "<template>\n  <div class=\"btn\"></div>\n</template>\n\n<style>\n.btn { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_analysis::style::*;
    use verter_analysis::template::*;

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css = &source[style_content_start as usize..style_block.content_range().1 as usize];

    let class_attr_start = source.find("class=\"btn\"").unwrap() as u32;
    let class_attr_end = class_attr_start + "class=\"btn\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".to_string(),
                    value: Some("btn".to_string()),
                    is_dynamic: false,
                    span: verter_span::Span::new(class_attr_start, class_attr_end),
                    name_end: 0,
                    value_span: None,
                }],
                directives: vec![],
                v_for: None,
                v_model: None,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: verter_span::Span::new(0, 0),
                tag_span_end: 0,
                content_end: 0,
                text_children: Vec::new(),
            }],
            ..Default::default()
        }),
        styles: vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )],
        ..Default::default()
    };

    // Click on "btn" in .btn { } in style
    let style_btn_offset = source.rfind("btn").unwrap();
    let position = line_index
        .offset_to_position(style_btn_offset as u32)
        .unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(
        result.is_some(),
        "should navigate from style .btn to template class=\"btn\""
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let expected_start = line_index.offset_to_position(class_attr_start).unwrap();
        assert_eq!(loc.range.start, expected_start);
    } else {
        panic!("expected scalar location");
    }
}

// =====================================================================
// Import Source Navigation Tests
// =====================================================================

#[test]
fn test_import_source_string_navigation() {
    let source = "<script setup>\nimport Foo from './Foo.vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let import_start = source.find("import").unwrap() as u32;
    let import_end = source.find("'./Foo.vue'").unwrap() as u32 + "'./Foo.vue'".len() as u32;

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "./Foo.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Foo".to_string(),
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(import_start, import_end),
            resolved_canonical_id: Some("/project/Foo.vue".to_string()),
        }],
        vec![],
    );

    // Click on "Foo.vue" inside the import string
    let foo_vue_offset = source.find("Foo.vue").unwrap();
    let position = line_index
        .offset_to_position(foo_vue_offset as u32)
        .unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(
        result.is_some(),
        "should navigate to resolved file from import string"
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(
            loc.uri.as_str().contains("Foo.vue"),
            "should resolve to Foo.vue"
        );
    } else {
        panic!("expected scalar location");
    }
}

// =====================================================================
// Path Alias Resolution Tests
// =====================================================================

#[test]
fn test_path_alias_resolution_on_binding() {
    let source = "<script setup>\nimport Foo from '@/components/Foo.vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let import_start = source.find("import").unwrap() as u32;
    let import_end = source.find("'@/components/Foo.vue'").unwrap() as u32
        + "'@/components/Foo.vue'".len() as u32;

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "@/components/Foo.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Foo".to_string(),
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(import_start, import_end),
            resolved_canonical_id: None, // not resolved by host
        }],
        vec![],
    );

    // Click on "Foo" binding name
    let foo_offset = source.find("Foo").unwrap();
    let position = line_index.offset_to_position(foo_offset as u32).unwrap();

    // With resolver: should navigate to resolved file
    let resolver = |specifier: &str| -> Option<String> {
        if specifier == "@/components/Foo.vue" {
            Some("/project/src/components/Foo.vue".to_string())
        } else {
            None
        }
    };
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        Some(&resolver),
    );
    assert!(result.is_some(), "should navigate via path resolver");

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(
            loc.uri.as_str().contains("Foo.vue"),
            "should resolve to Foo.vue, got: {}",
            loc.uri.as_str()
        );
    } else {
        panic!("expected scalar location");
    }

    // Without resolver: should fall back to import span
    let result_no_resolver = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(
        result_no_resolver.is_some(),
        "should fall back to import span"
    );
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result_no_resolver {
        // Should point to import statement, not to a file
        assert_eq!(
            loc.uri.as_str(),
            SAME_FILE_URI,
            "without resolver should stay in same file"
        );
    }
}

#[test]
fn test_path_alias_resolution_on_import_string() {
    let source = "<script setup>\nimport Foo from '@/components/Foo.vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let import_start = source.find("import").unwrap() as u32;
    let import_end = source.find("'@/components/Foo.vue'").unwrap() as u32
        + "'@/components/Foo.vue'".len() as u32;

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "@/components/Foo.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Foo".to_string(),
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(import_start, import_end),
            resolved_canonical_id: None,
        }],
        vec![],
    );

    // Click on "@/components" inside the import string
    let at_offset = source.find("@/components").unwrap();
    let position = line_index.offset_to_position(at_offset as u32).unwrap();

    let resolver = |specifier: &str| -> Option<String> {
        if specifier == "@/components/Foo.vue" {
            Some("/project/src/components/Foo.vue".to_string())
        } else {
            None
        }
    };
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        Some(&resolver),
    );
    assert!(
        result.is_some(),
        "should navigate from import string via resolver"
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(
            loc.uri.as_str().contains("Foo.vue"),
            "should resolve to Foo.vue from string click"
        );
    } else {
        panic!("expected scalar location");
    }
}

// =====================================================================
// DOM Query Selector Navigation Tests
// =====================================================================

#[test]
fn test_dom_query_selector_navigates_to_element() {
    use verter_analysis::style::*;
    use verter_analysis::template::*;
    use verter_analysis::types::*;

    let source = "<template>\n  <button class=\"btn\">Click</button>\n</template>\n\n<script setup>\ndocument.querySelector('.btn')\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Build a selector for .btn
    let parsed = parse_selector(".btn").unwrap();

    // Find string argument span as SFC-absolute offsets
    let qs_str_start = source.find("'.btn'").unwrap();
    // arg spans point at the content inside quotes
    let arg_start = qs_str_start + 1; // after '
    let arg_end = arg_start + ".btn".len();

    let btn_elem_start = source.find("<button").unwrap() as u32;
    let btn_elem_end = source.find("</button>").unwrap() as u32 + "</button>".len() as u32;

    let class_attr_start = source.find("class=\"btn\"").unwrap() as u32;
    let class_attr_end = class_attr_start + "class=\"btn\"".len() as u32;

    // DomQueryCallSite spans are SFC-absolute
    let doc_start = source.find("document").unwrap() as u32;
    let call_end = (source.find("'.btn')").unwrap() + "'.btn')".len()) as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "button".to_string(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".to_string(),
                    value: Some("btn".to_string()),
                    is_dynamic: false,
                    span: verter_span::Span::new(class_attr_start, class_attr_end),
                    name_end: 0,
                    value_span: None,
                }],
                directives: vec![],
                v_for: None,
                v_model: None,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: verter_span::Span::new(btn_elem_start, btn_elem_end),
                tag_span_end: btn_elem_end,
                content_end: 0,
                text_children: Vec::new(),
            }],
            ..Default::default()
        }),
        dom_query_calls: vec![DomQueryCallSite {
            kind: DomQueryKind::QuerySelector,
            selector_text: ".btn".to_string(),
            parsed: Some(parsed),
            span: verter_span::Span::new(doc_start, call_end),
            arg_span: verter_span::Span::new(arg_start as u32, arg_end as u32),
        }],
        ..Default::default()
    };

    // Click on ".btn" inside the selector string
    let abs_cursor = arg_start + 1; // on 'b' in '.btn'
    let position = line_index.offset_to_position(abs_cursor as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(
        result.is_some(),
        "should navigate from querySelector arg to template element"
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        // Should point to the <button> element span
        let expected = line_index.offset_to_position(btn_elem_start).unwrap();
        assert_eq!(loc.range.start, expected);
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_dom_query_selector_no_match() {
    use verter_analysis::style::*;
    use verter_analysis::template::*;
    use verter_analysis::types::*;

    let source = "<template>\n  <div>hello</div>\n</template>\n\n<script setup>\ndocument.querySelector('.missing')\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let parsed = parse_selector(".missing").unwrap();

    // Use SFC-absolute offsets (spans are adjusted by verter_host during analysis)
    let qs_str_start = source.find("'.missing'").unwrap();
    let arg_start = qs_str_start + 1;
    let arg_end = arg_start + ".missing".len();

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![],
                directives: vec![],
                v_for: None,
                v_model: None,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: verter_span::Span::new(0, 0),
                tag_span_end: 0,
                content_end: 0,
                text_children: Vec::new(),
            }],
            ..Default::default()
        }),
        dom_query_calls: vec![DomQueryCallSite {
            kind: DomQueryKind::QuerySelector,
            selector_text: ".missing".to_string(),
            parsed: Some(parsed),
            span: verter_span::Span::new(0, 40),
            arg_span: verter_span::Span::new(arg_start as u32, arg_end as u32),
        }],
        ..Default::default()
    };

    let abs_cursor = arg_start + 1; // already SFC-absolute
    let position = line_index.offset_to_position(abs_cursor as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(
        result.is_none(),
        "no template element or CSS rule matches .missing"
    );
}

#[test]
fn test_dom_query_selector_falls_back_to_css() {
    use verter_analysis::style::*;
    use verter_analysis::template::*;
    use verter_analysis::types::*;

    // Template has no .btn element, but style has .btn rule
    let source = "<template>\n  <div>hello</div>\n</template>\n\n<script setup>\ndocument.querySelector('.btn')\n</script>\n\n<style>\n.btn { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css = &source[style_content_start as usize..style_block.content_range().1 as usize];

    let parsed = parse_selector(".btn").unwrap();

    // Use SFC-absolute offsets (spans are adjusted by verter_host during analysis)
    let qs_str_start = source.find("'.btn'").unwrap();
    let arg_start = qs_str_start + 1;
    let arg_end = arg_start + ".btn".len();

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![],
                directives: vec![],
                v_for: None,
                v_model: None,
                has_v_if: false,
                has_v_else: false,
                has_v_else_if: false,
                has_v_show: false,
                has_v_html: false,
                has_v_text: false,
                has_text_content: false,
                has_bare_text: false,
                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: verter_span::Span::new(0, 0),
                tag_span_end: 0,
                content_end: 0,
                text_children: Vec::new(),
            }],
            ..Default::default()
        }),
        dom_query_calls: vec![DomQueryCallSite {
            kind: DomQueryKind::QuerySelector,
            selector_text: ".btn".to_string(),
            parsed: Some(parsed),
            span: verter_span::Span::new(0, 40),
            arg_span: verter_span::Span::new(arg_start as u32, arg_end as u32),
        }],
        styles: vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )],
        ..Default::default()
    };

    let abs_cursor = arg_start + 1; // already SFC-absolute
    let position = line_index.offset_to_position(abs_cursor as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
    );
    assert!(
        result.is_some(),
        "should fall back to CSS rule definition for .btn"
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        // Should point to the .btn in style (the class span)
        let style_btn_offset = source.rfind("btn").unwrap();
        let expected = line_index
            .offset_to_position(style_btn_offset as u32)
            .unwrap();
        assert_eq!(
            loc.range.start, expected,
            "should navigate to .btn CSS rule"
        );
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_path_alias_resolution_on_component_tag() {
    let source = "<template>\n  <FooComp />\n</template>\n\n<script setup>\nimport FooComp from '@/components/FooComp.vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_analysis::template::*;

    let analysis = FileAnalysisSnapshot {
        imports: vec![AnalyzedImport {
            source: "@/components/FooComp.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "FooComp".to_string(),
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        }],
        template: Some(TemplateAnalysisSnapshot {
            components: vec![TemplateComponentUsage {
                name: "FooComp".to_string(),
                import_source: Some("@/components/FooComp.vue".to_string()),
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                span: verter_span::Span::new(0, 0),
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    // Click on "FooComp" in template
    let offset = source.find("FooComp").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let resolver = |specifier: &str| -> Option<String> {
        if specifier == "@/components/FooComp.vue" {
            Some("/project/src/components/FooComp.vue".to_string())
        } else {
            None
        }
    };
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        Some(&resolver),
    );
    assert!(
        result.is_some(),
        "should navigate to component via path resolver"
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(
            loc.uri.as_str().contains("FooComp.vue"),
            "should resolve to FooComp.vue, got: {}",
            loc.uri.as_str()
        );
    } else {
        panic!("expected scalar location");
    }
}
