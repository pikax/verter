use super::*;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use verter_analysis::types::VueApiCallSite;
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
fn test_word_at_offset() {
    assert_eq!(word_at_offset("const foo = 1", 6), Some("foo".to_string()));
    assert_eq!(word_at_offset("const foo = 1", 5), None); // space
    assert_eq!(word_at_offset("hello", 0), Some("hello".to_string()));
    assert_eq!(word_at_offset("hello", 4), Some("hello".to_string()));
    assert_eq!(word_at_offset("", 0), None);
}

#[test]
fn test_hover_on_binding() {
    let source = "<script setup>\nconst count = ref(0)\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: Some(BindingInitializer::FunctionCall {
                callee: "ref".to_string(),
                callee_import_source: Some("vue".to_string()),
                vue_api: Some(VueApiClassification::Ref),
            }),
            span: verter_span::Span::new(0, 0),
        }],
        vec![],
        vec![],
    );

    // Hover on "count" — find its offset
    let offset = source.find("count").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(hover.is_some());
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(contents.contains("const count"));
    assert!(contents.contains("reactive"));
    assert!(contents.contains("ref()"));
}

#[test]
fn test_hover_on_import() {
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
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        }],
        vec![],
    );

    let ref_offset = source.find("ref").unwrap();
    let position = line_index.offset_to_position(ref_offset as u32).unwrap();

    let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(hover.is_some());
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(contents.contains("import"));
    assert!(contents.contains("'vue'"));
}

#[test]
fn test_hover_outside_blocks() {
    let source = "<!-- comment -->\n<script setup>\nconst x = 1;\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(vec![], vec![], vec![]);

    // Position in the comment (outside blocks)
    let position = Position {
        line: 0,
        character: 5,
    };
    let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(hover.is_none());
}

#[test]
fn test_hover_on_template_binding() {
    let source = "<template>\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
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
        vec![],
        vec![],
    );

    // Find "count" in the template
    let offset = source.find("count").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(hover.is_some());
}

#[test]
fn test_hover_on_vue_api_call_site() {
    let source =
        "<script setup>\nimport { onMounted } from 'vue'\nonMounted(() => {})\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Offset of "onMounted(() => {})" call
    let call_offset = source.find("onMounted(() =>").unwrap();

    let analysis = FileAnalysisSnapshot {
        vue_api_calls: vec![VueApiCallSite {
            api: VueApiClassification::OnMounted,
            span: verter_span::Span::new(
                call_offset as u32,
                (call_offset + "onMounted".len()) as u32,
            ),
            arg_value: None,
            is_async_callback: false,
        }],
        ..Default::default()
    };

    let position = line_index.offset_to_position(call_offset as u32).unwrap();

    let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(hover.is_some());
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(contents.contains("onMounted()"));
    assert!(contents.contains("Lifecycle Hook"));
    assert!(contents.contains("synchronous"));
}

#[test]
fn test_no_hover_on_unknown_word() {
    let source = "<script setup>\nconst unknownVar = 1;\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Empty analysis — no bindings registered
    let analysis = make_analysis(vec![], vec![], vec![]);

    let offset = source.find("unknownVar").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(hover.is_none());
}

#[test]
fn test_no_hover_inside_html_comment() {
    let source = "<template>\n  <!-- count -->\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
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
        vec![],
        vec![],
    );

    // Hover on "count" inside the comment — should return None
    let offset = source.find("count").unwrap();
    assert!(
        source[..offset].contains("<!--"),
        "should be inside comment"
    );
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(hover.is_none(), "should not hover inside HTML comment");

    // Hover on "count" in the interpolation — should work
    let second_offset = source[offset + 5..].find("count").unwrap() + offset + 5;
    let position2 = line_index.offset_to_position(second_offset as u32).unwrap();

    let hover2 = hover_at_position(&position2, source, &blocks, Some(&analysis), &line_index);
    assert!(hover2.is_some(), "should hover on binding outside comment");
}

#[test]
fn test_hover_on_component_shows_prop_constness() {
    let source =
        "<template>\n  <MyButton :title=\"msg\" disabled>\n  </MyButton>\n</template>\n\n<script setup>\nimport MyButton from './MyButton.vue'\nconst msg = ref('hello')\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let comp_offset = source.find("<MyButton").unwrap();

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            components: vec![verter_analysis::template::TemplateComponentUsage {
                name: "MyButton".into(),
                import_source: Some("./MyButton.vue".into()),
                is_dynamic: false,
                props: vec![
                    verter_analysis::template::TemplatePropUsage {
                        name: "title".into(),
                        is_bound: true,
                        constness: verter_analysis::template::PropValueConstness::Dynamic,
                        referenced_bindings: vec!["msg".into()],
                        from_spread: false,
                        span: verter_span::Span::new(
                            (comp_offset + 10) as u32,
                            (comp_offset + 22) as u32,
                        ),
                    },
                    verter_analysis::template::TemplatePropUsage {
                        name: "disabled".into(),
                        is_bound: false,
                        constness: verter_analysis::template::PropValueConstness::Const,
                        referenced_bindings: vec![],
                        from_spread: false,
                        span: verter_span::Span::new(
                            (comp_offset + 23) as u32,
                            (comp_offset + 31) as u32,
                        ),
                    },
                ],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                span: verter_span::Span::new(comp_offset as u32, (comp_offset + 40) as u32),
            }],
            elements: vec![],
            ..Default::default()
        }),
        ..Default::default()
    };

    // Hover on "MyButton" tag name
    let pos = line_index
        .offset_to_position((comp_offset + 1) as u32)
        .unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index);

    assert!(hover.is_some(), "should provide hover on component element");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(contents.contains("MyButton"), "should show component name");
    assert!(
        contents.contains("title") && contents.contains("dynamic"),
        "should show title prop as dynamic: {}",
        contents
    );
    assert!(
        contents.contains("disabled") && contents.contains("const"),
        "should show disabled prop as const: {}",
        contents
    );
}

#[test]
fn test_hover_on_component_with_no_props() {
    let source =
        "<template>\n  <Popup />\n</template>\n\n<script setup>\nimport Popup from './Popup.vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let comp_offset = source.find("<Popup").unwrap();

    let analysis = FileAnalysisSnapshot {
        template: Some(TemplateAnalysisSnapshot {
            components: vec![verter_analysis::template::TemplateComponentUsage {
                name: "Popup".into(),
                import_source: Some("./Popup.vue".into()),
                is_dynamic: false,
                props: vec![], // No props
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                span: verter_span::Span::new(comp_offset as u32, (comp_offset + 10) as u32),
            }],
            elements: vec![],
            ..Default::default()
        }),
        ..Default::default()
    };

    // Hover on "Popup" tag name — should return info even with no props
    let pos = line_index
        .offset_to_position((comp_offset + 1) as u32)
        .unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index);

    assert!(
        hover.is_some(),
        "should provide hover on component even without props"
    );
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(contents.contains("Popup"), "should show component name");
    assert!(
        contents.contains("./Popup.vue"),
        "should show import source"
    );
    assert!(
        !contents.contains("Props:"),
        "should not show Props section when empty"
    );
}

#[test]
fn test_hover_on_element_shows_css_rules() {
    let source = "<template>\n  <div class=\"foo\">hello</div>\n</template>\n\n<style scoped>\n.foo { color: red; }\ndiv { font-size: 14px; }\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Build style analysis from the actual CSS content
    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (content_start, content_end) = style_block.content_range();
    let css_content = &source[content_start as usize..content_end as usize];

    let style = verter_analysis::style::build_css_style_analysis(
        css_content,
        verter_analysis::style::VueStyleInput {
            v_binds: vec![],
            special_pseudos: vec![],
        },
        true,
        false,
        None,
        content_start,
    );

    // Find the div element's offset in template
    let div_offset = source.find("<div class").unwrap();

    let analysis = FileAnalysisSnapshot {
        styles: vec![style],
        template: Some(TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".into(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".into(),
                    value: Some("foo".into()),
                    is_dynamic: false,
                    span: verter_span::Span::new(0, 0),
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
                span: verter_span::Span::new(div_offset as u32, (div_offset + 20) as u32),
                tag_span_end: (div_offset + 20) as u32,
                content_end: 0,
                text_children: Vec::new(),
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    // Hover on the "div" tag name
    let pos = line_index
        .offset_to_position((div_offset + 1) as u32)
        .unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index);

    assert!(hover.is_some(), "should provide hover on template element");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        contents.contains("CSS rules"),
        "should show CSS rules section"
    );
    assert!(contents.contains(".foo"), "should list .foo selector");
    assert!(contents.contains("div"), "should list div selector");
}

// ========================================================================
// SFC tag hover (A2)
// ========================================================================

#[test]
fn test_hover_on_script_tag_name() {
    let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Hover on "script" in opening tag (offset 1 = 's')
    let pos = line_index.offset_to_position(1).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index);
    assert!(hover.is_some(), "should hover on script tag name");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(contents.contains("<script>"), "should describe script tag");
    assert!(
        !contents.contains("<template>"),
        "should not describe template"
    );
}

#[test]
fn test_hover_on_template_tag_name() {
    let source = "<template>\n  <div/>\n</template>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let pos = line_index.offset_to_position(1).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index);
    assert!(hover.is_some(), "should hover on template tag name");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        contents.contains("<template>"),
        "should describe template tag"
    );
}

#[test]
fn test_hover_on_setup_attr() {
    let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // "setup" starts at offset 8 in "<script setup lang=\"ts\">"
    let setup_offset = source.find("setup").unwrap();
    let pos = line_index.offset_to_position(setup_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index);
    assert!(hover.is_some(), "should hover on setup attribute");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        contents.contains("setup"),
        "should describe setup attribute"
    );
    assert!(
        contents.contains("defineProps"),
        "should mention defineProps"
    );
}

#[test]
fn test_hover_on_lang_attr() {
    let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let lang_offset = source.find("lang").unwrap();
    let pos = line_index.offset_to_position(lang_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index);
    assert!(hover.is_some(), "should hover on lang attribute");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(contents.contains("lang"), "should describe lang attribute");
}

#[test]
fn test_hover_on_scoped_attr() {
    let source = "<style scoped>\n.foo {}\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let scoped_offset = source.find("scoped").unwrap();
    let pos = line_index.offset_to_position(scoped_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index);
    assert!(hover.is_some(), "should hover on scoped attribute");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        contents.contains("scoped"),
        "should describe scoped attribute"
    );
}

#[test]
fn test_hover_on_closing_tag() {
    let source = "<script setup>\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Closing tag: "</script>" — hover on it
    let close_offset = blocks[0].close_tag_start + 2; // skip "</"
    let pos = line_index.offset_to_position(close_offset).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index);
    assert!(hover.is_some(), "should hover on closing tag");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(contents.contains("<script>"), "should describe script tag");
}

#[test]
fn test_no_hover_at_root_level() {
    let source = "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Between blocks — root level
    let between = blocks[0].close_tag_end;
    let pos = line_index.offset_to_position(between).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index);
    assert!(hover.is_none(), "should not hover at root level");
}

#[test]
fn test_hover_on_attrs_attribute() {
    let source = "<script setup attrs=\"{ class?: string }\" lang=\"ts\">\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let attrs_offset = source.find("attrs").unwrap();
    let pos = line_index.offset_to_position(attrs_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index);
    assert!(hover.is_some(), "should hover on attrs attribute");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(contents.contains("attrs"), "should describe attrs");
    assert!(contents.contains("$attrs"), "should mention $attrs");
}

#[test]
fn test_hover_on_custom_block_tag() {
    let source = "<i18n lang=\"json\">\n{\"en\": {}}\n</i18n>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let pos = line_index.offset_to_position(1).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index);
    assert!(hover.is_some(), "should hover on custom block tag");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        contents.contains("Custom block"),
        "should describe as custom block"
    );
    assert!(!contents.contains("<script>"), "should not describe script");
}
