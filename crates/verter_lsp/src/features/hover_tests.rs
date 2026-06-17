use super::*;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use verter_semantic::analysis::types::ImportBindingKind;
use verter_semantic::analysis::types::VueApiCallSite;
use verter_semantic::analysis::*;

fn make_analysis(
    bindings: Vec<AnalyzedBinding>,
    imports: Vec<AnalyzedImport>,
    macros: Vec<AnalyzedMacro>,
) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        bindings,
        imports,
        macros: macros.into(),
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
            used_in_script: false,
            used_in_style: false,
        }],
        vec![],
        vec![],
    );

    // Hover on "count" — find its offset
    let offset = source.find("count").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let hover = hover_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    );
    assert!(hover.is_some());
    let contents = match hover.unwrap().hover.contents {
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
                kind: ImportBindingKind::Named,
                imported_name: None,
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

    let hover = hover_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    );
    assert!(hover.is_some());
    let contents = match hover.unwrap().hover.contents {
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
    let hover = hover_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    );
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
            used_in_script: false,
            used_in_style: false,
        }],
        vec![],
        vec![],
    );

    // Find "count" in the template
    let offset = source.find("count").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let hover = hover_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    );
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
        vue_api_calls: (vec![VueApiCallSite {
            api: VueApiClassification::OnMounted,
            span: verter_span::Span::new(
                call_offset as u32,
                (call_offset + "onMounted".len()) as u32,
            ),
            arg_value: None,
            has_type_params: false,
            is_async_callback: false,
            callback_params: vec![],
        }])
        .into(),
        ..Default::default()
    };

    let position = line_index.offset_to_position(call_offset as u32).unwrap();

    let hover = hover_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    );
    assert!(hover.is_some());
    let contents = match hover.unwrap().hover.contents {
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

    let hover = hover_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    );
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
            used_in_script: false,
            used_in_style: false,
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

    let hover = hover_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    );
    assert!(hover.is_none(), "should not hover inside HTML comment");

    // Hover on "count" in the interpolation — should work
    let second_offset = source[offset + 5..].find("count").unwrap() + offset + 5;
    let position2 = line_index.offset_to_position(second_offset as u32).unwrap();

    let hover2 = hover_at_position(
        &position2,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    );
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
        template: Some(
            (TemplateAnalysisSnapshot {
                components: vec![verter_semantic::analysis::template::TemplateComponentUsage {
                    name: "MyButton".into(),
                    import_source: Some("./MyButton.vue".into()),
                    is_dynamic: false,
                    props: vec![
                        verter_semantic::analysis::template::TemplatePropUsage {
                            name: "title".into(),
                            is_bound: true,
                            expression: None,
                            constness: verter_semantic::analysis::template::PropValueConstness::Dynamic,
                            referenced_bindings: vec!["msg".into()],
                            from_spread: false,
                            span: verter_span::Span::new(
                                (comp_offset + 10) as u32,
                                (comp_offset + 22) as u32,
                            ),
                            name_span: verter_span::Span::new(0, 0),
                            is_shorthand: false,
                        },
                        verter_semantic::analysis::template::TemplatePropUsage {
                            name: "disabled".into(),
                            is_bound: false,
                            expression: None,
                            constness: verter_semantic::analysis::template::PropValueConstness::Const,
                            referenced_bindings: vec![],
                            from_spread: false,
                            span: verter_span::Span::new(
                                (comp_offset + 23) as u32,
                                (comp_offset + 31) as u32,
                            ),
                            name_span: verter_span::Span::new(0, 0),
                            is_shorthand: false,
                        },
                    ],
                    has_spread: false,
                    slots_used: vec![],
                    static_classes: vec![],
                    has_dynamic_class: false,
                    dynamic_classes: vec![],
                    v_models: vec![],
                    bindings: vec![],
                    events: vec![],
                    span: verter_span::Span::new(comp_offset as u32, (comp_offset + 40) as u32),
                }],
                elements: vec![],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // Hover on "MyButton" tag name
    let pos = line_index
        .offset_to_position((comp_offset + 1) as u32)
        .unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false);

    assert!(hover.is_some(), "should provide hover on component element");
    let contents = match hover.unwrap().hover.contents {
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
        template: Some(
            (TemplateAnalysisSnapshot {
                components: vec![
                    verter_semantic::analysis::template::TemplateComponentUsage {
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
                        bindings: vec![],
                        events: vec![],
                        span: verter_span::Span::new(comp_offset as u32, (comp_offset + 10) as u32),
                    },
                ],
                elements: vec![],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // Hover on "Popup" tag name — should return info even with no props
    let pos = line_index
        .offset_to_position((comp_offset + 1) as u32)
        .unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false);

    assert!(
        hover.is_some(),
        "should provide hover on component even without props"
    );
    let contents = match hover.unwrap().hover.contents {
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

    let style = verter_semantic::analysis::style::build_css_style_analysis(
        css_content,
        verter_semantic::analysis::style::VueStyleInput {
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
        styles: (vec![style]).into(),
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // Hover on the "div" tag name
    let pos = line_index
        .offset_to_position((div_offset + 1) as u32)
        .unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false);

    assert!(hover.is_some(), "should provide hover on template element");
    let contents = match hover.unwrap().hover.contents {
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
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(hover.is_some(), "should hover on script tag name");
    let contents = match hover.unwrap().hover.contents {
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
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(hover.is_some(), "should hover on template tag name");
    let contents = match hover.unwrap().hover.contents {
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
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(hover.is_some(), "should hover on setup attribute");
    let contents = match hover.unwrap().hover.contents {
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
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(hover.is_some(), "should hover on lang attribute");
    let contents = match hover.unwrap().hover.contents {
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
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(hover.is_some(), "should hover on scoped attribute");
    let contents = match hover.unwrap().hover.contents {
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
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(hover.is_some(), "should hover on closing tag");
    let contents = match hover.unwrap().hover.contents {
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
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(hover.is_none(), "should not hover at root level");
}

#[test]
fn test_hover_on_attrs_attribute() {
    let source = "<script setup attrs=\"{ class?: string }\" lang=\"ts\">\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let attrs_offset = source.find("attrs").unwrap();
    let pos = line_index.offset_to_position(attrs_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(hover.is_some(), "should hover on attrs attribute");
    let contents = match hover.unwrap().hover.contents {
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
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(hover.is_some(), "should hover on custom block tag");
    let contents = match hover.unwrap().hover.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        contents.contains("Custom block"),
        "should describe as custom block"
    );
    assert!(!contents.contains("<script>"), "should not describe script");
}

// ========================================================================
// Fix 1: Narrow hover span matching (Bugs 5, 2)
// ========================================================================

#[test]
fn test_hover_on_component_attr_does_not_show_constness() {
    // Hovering on `:icon` attribute inside <Popup :icon="x"> should NOT show
    // the component prop constness hover — only the tag name should trigger it.
    let source =
        "<template>\n  <Popup :icon=\"x\">\n  </Popup>\n</template>\n\n<script setup>\nimport Popup from './Popup.vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let comp_offset = source.find("<Popup").unwrap();
    let icon_offset = source.find(":icon").unwrap();

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
                components: vec![
                    verter_semantic::analysis::template::TemplateComponentUsage {
                        name: "Popup".into(),
                        import_source: Some("./Popup.vue".into()),
                        is_dynamic: false,
                        props: vec![verter_semantic::analysis::template::TemplatePropUsage {
                            name: "icon".into(),
                            is_bound: true,
                            expression: None,
                            constness:
                                verter_semantic::analysis::template::PropValueConstness::Dynamic,
                            referenced_bindings: vec!["x".into()],
                            from_spread: false,
                            span: verter_span::Span::new(
                                icon_offset as u32,
                                (icon_offset + 10) as u32,
                            ),
                            name_span: verter_span::Span::new(0, 0),
                            is_shorthand: false,
                        }],
                        has_spread: false,
                        slots_used: vec![],
                        static_classes: vec![],
                        has_dynamic_class: false,
                        dynamic_classes: vec![],
                        v_models: vec![],
                        bindings: vec![],
                        events: vec![],
                        span: verter_span::Span::new(comp_offset as u32, (comp_offset + 30) as u32),
                    },
                ],
                elements: vec![],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // Hover on `:icon` — should NOT return constness hover
    let pos = line_index.offset_to_position(icon_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false);

    // Should be None (or at least not contain constness info)
    if let Some(h) = hover {
        let contents = match h.hover.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(
            !contents.contains("Props:"),
            "hovering on :icon should NOT show prop constness, got: {}",
            contents
        );
    }
}

#[test]
fn test_hover_on_div_class_attr_does_not_show_css() {
    // Hovering on `class` attribute name in <div class="foo"> should NOT show
    // the CSS rules hover — only the tag name should trigger it.
    let source = "<template>\n  <div class=\"foo\">hello</div>\n</template>\n\n<style scoped>\n.foo { color: red; }\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (content_start, content_end) = style_block.content_range();
    let css_content = &source[content_start as usize..content_end as usize];

    let style = verter_semantic::analysis::style::build_css_style_analysis(
        css_content,
        verter_semantic::analysis::style::VueStyleInput {
            v_binds: vec![],
            special_pseudos: vec![],
        },
        true,
        false,
        None,
        content_start,
    );

    let div_offset = source.find("<div class").unwrap();
    let class_offset = source.find("class=").unwrap();

    let analysis = FileAnalysisSnapshot {
        styles: (vec![style]).into(),
        template: Some(
            (TemplateAnalysisSnapshot {
                elements: vec![TemplateElement {
                    tag: "div".into(),
                    is_component: false,
                    is_self_closing: false,
                    namespace: ElementNamespace::Html,
                    attributes: vec![TemplateAttribute {
                        name: "class".into(),
                        value: Some("foo".into()),
                        is_dynamic: false,
                        span: verter_span::Span::new(
                            class_offset as u32,
                            (class_offset + 11) as u32,
                        ),
                        name_end: (class_offset + 5) as u32,
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
                    span: verter_span::Span::new(div_offset as u32, (div_offset + 30) as u32),
                    tag_span_end: (div_offset + 20) as u32,
                    content_end: 0,
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // Hover on "class" attribute name — should NOT show CSS rules
    let pos = line_index.offset_to_position(class_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false);

    if let Some(h) = hover {
        let contents = match h.hover.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(
            !contents.contains("CSS rules"),
            "hovering on class attr name should NOT show CSS rules, got: {}",
            contents
        );
    }
}

#[test]
fn test_hover_on_ref_attr_does_not_show_import() {
    // Hovering on a static `ref` attribute name in <span ref="el"> must return a
    // SOURCE-OWNED template-ref hover: it names the `ref` attribute and its `el`
    // target, and must NOT surface the imported Vue `ref()` symbol.
    let source = "<template>\n  <span ref=\"el\">text</span>\n</template>\n\n<script setup>\nimport { ref } from 'vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let template_ref_offset = source.find("ref=\"el\"").unwrap();

    let analysis = FileAnalysisSnapshot {
        imports: vec![AnalyzedImport {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "ref".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: Some(VueApiClassification::Ref),
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        }],
        template: Some(
            (TemplateAnalysisSnapshot {
                elements: vec![TemplateElement {
                    tag: "span".into(),
                    is_component: false,
                    is_self_closing: false,
                    namespace: ElementNamespace::Html,
                    attributes: vec![TemplateAttribute {
                        name: "ref".into(),
                        value: Some("el".into()),
                        is_dynamic: false,
                        span: verter_span::Span::new(
                            template_ref_offset as u32,
                            (template_ref_offset + 8) as u32,
                        ),
                        name_end: (template_ref_offset + 3) as u32,
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
                    span: verter_span::Span::new(0, 50),
                    tag_span_end: 30,
                    content_end: 0,
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // Hover on "ref" attribute name — must return a source-owned template-ref hover
    let pos = line_index
        .offset_to_position(template_ref_offset as u32)
        .unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false)
        .expect("template ref attribute hover should exist");
    let contents = match hover.hover.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        contents.contains("ref"),
        "template ref hover should mention `ref`, got: {}",
        contents
    );
    assert!(
        contents.contains("el"),
        "template ref hover should name the `el` target, got: {}",
        contents
    );
    assert!(
        !contents.contains("import"),
        "hovering on ref attr should NOT show import hover, got: {}",
        contents
    );
}

/// Build a native (non-component) `<{tag}>` element carrying a single `v-on`
/// directive, for the source-owned event-hover tests below.
fn native_event_element(tag: &str, dir: TemplateDirective) -> TemplateElement {
    TemplateElement {
        tag: tag.into(),
        is_component: false,
        directives: vec![dir],
        ..Default::default()
    }
}

fn template_only_analysis(el: TemplateElement) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        template: Some(
            TemplateAnalysisSnapshot {
                elements: vec![el],
                ..Default::default()
            }
            .into(),
        ),
        ..Default::default()
    }
}

fn markup(hover: &VerterHoverResult) -> String {
    match &hover.hover.contents {
        HoverContents::Markup(m) => m.value.clone(),
        _ => panic!("expected markup hover"),
    }
}

#[test]
fn native_event_directive_hover_shows_vue_source_token() {
    // Hovering on a native `@click` event directive must return a SOURCE-OWNED
    // hover that names the Vue token (`@click`), never the generated JSX prop
    // (`onClick`). The hover range stays on the source `@click` token so the merge
    // layer can rewrite a paired `onClick` TypeProvider hover back to `@click`.
    let source = r#"<template><button @click="increment">x</button></template>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let at_pos = source.find("@click").unwrap() as u32;
    let click_pos = at_pos + 1; // 'click' arg starts right after '@'
    let arg_span = verter_span::Span::new(click_pos, click_pos + 5);
    let dir = TemplateDirective {
        name: "on".into(),
        raw_name: "@click".into(),
        argument: Some("click".into()),
        modifiers: vec![],
        expression: Some("increment".into()),
        span: verter_span::Span::new(at_pos, click_pos + 5),
        name_end: click_pos,
        arg_span: Some(arg_span),
        expression_span: None,
        modifier_spans: vec![],
    };
    let analysis = template_only_analysis(native_event_element("button", dir));

    // Hover inside `click`.
    let pos = line_index.offset_to_position(click_pos + 2).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false)
        .expect("native event directive hover should exist");
    let contents = markup(&hover);
    assert!(
        contents.contains("@click"),
        "event hover must name the Vue source token `@click`, got: {contents}"
    );
    assert!(
        !contents.contains("onClick"),
        "event hover must NOT surface the generated JSX prop `onClick`, got: {contents}"
    );
    // Range must stay on the source `@click` token, not the generated prop.
    let expected_start = line_index.offset_to_position(at_pos).unwrap();
    assert_eq!(
        hover.hover.range.expect("hover range").start,
        expected_start,
        "event hover range must start on the source `@` token"
    );
    // The onClick→@click rewrite must ride TYPED provenance, not the rendered text:
    // the hover carries a structured `EventDirective` token with the canonical label.
    assert_eq!(
        hover.source_token,
        Some(HoverSourceToken::EventDirective {
            vue_attr: "@click".to_string()
        }),
        "native event hover must carry typed event-directive provenance"
    );
}

#[test]
fn v_on_long_form_hover_canonicalizes_provenance_to_at_form() {
    // `v-on:click` is the long form of `@click`. The DISPLAY token reflects what the
    // user wrote, but the TYPED provenance canonicalizes to `@click` so the merge
    // layer rewrites a paired `onClick` TypeProvider label to `@click`.
    let source = r#"<template><button v-on:click="increment">x</button></template>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let dir_start = source.find("v-on:click").unwrap() as u32;
    let click_pos = source.find("click").unwrap() as u32;
    let arg_span = verter_span::Span::new(click_pos, click_pos + 5);
    let dir = TemplateDirective {
        name: "on".into(),
        raw_name: "v-on:click".into(),
        argument: Some("click".into()),
        modifiers: vec![],
        expression: Some("increment".into()),
        span: verter_span::Span::new(dir_start, click_pos + 5),
        name_end: dir_start + 4, // `v-on`
        arg_span: Some(arg_span),
        expression_span: None,
        modifier_spans: vec![],
    };
    let analysis = template_only_analysis(native_event_element("button", dir));

    let pos = line_index.offset_to_position(click_pos + 2).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false)
        .expect("v-on:click hover should exist");
    assert_eq!(
        hover.source_token,
        Some(HoverSourceToken::EventDirective {
            vue_attr: "@click".to_string()
        }),
        "v-on:click provenance must canonicalize to the `@click` form"
    );
}

#[test]
fn event_modifier_stop_hover() {
    // Hovering on a `.stop` modifier token must return a source-owned modifier
    // hover describing `stopPropagation`, sourced from the shared modifier table.
    let source = r#"<template><button @click.stop="increment">x</button></template>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let at_pos = source.find("@click").unwrap() as u32;
    let click_pos = at_pos + 1;
    let dot_pos = source.find(".stop").unwrap() as u32;
    let stop_pos = dot_pos + 1;
    let mod_span = verter_span::Span::new(stop_pos, stop_pos + 4);
    let dir = TemplateDirective {
        name: "on".into(),
        raw_name: "@click".into(),
        argument: Some("click".into()),
        modifiers: vec!["stop".into()],
        expression: Some("increment".into()),
        span: verter_span::Span::new(at_pos, stop_pos + 4),
        name_end: click_pos,
        arg_span: Some(verter_span::Span::new(click_pos, click_pos + 5)),
        expression_span: None,
        modifier_spans: vec![mod_span],
    };
    let analysis = template_only_analysis(native_event_element("button", dir));

    // Hover inside `stop`.
    let pos = line_index.offset_to_position(stop_pos + 1).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false)
        .expect("event modifier hover should exist");
    let contents = markup(&hover);
    assert!(
        contents.contains(".stop"),
        "modifier hover must name `.stop`, got: {contents}"
    );
    assert!(
        contents.contains("stopPropagation"),
        "modifier hover must describe stopPropagation, got: {contents}"
    );
    // F2a: the hover range must include the leading `.`, so the highlighted token
    // is `.stop` (not just `stop`). The compiler span is name-only; hover expands it.
    let expected_dot_start = line_index.offset_to_position(dot_pos).unwrap();
    let expected_end = line_index.offset_to_position(stop_pos + 4).unwrap();
    let range = hover.hover.range.expect("modifier hover range");
    assert_eq!(
        range.start, expected_dot_start,
        "modifier hover range must start at the leading `.`"
    );
    assert_eq!(
        range.end, expected_end,
        "modifier hover range must end after the modifier name"
    );
    // Hovering directly on the `.` must also resolve the modifier hover.
    let dot_hover_pos = line_index.offset_to_position(dot_pos).unwrap();
    assert!(
        hover_at_position(
            &dot_hover_pos,
            source,
            &blocks,
            Some(&analysis),
            &line_index,
            false
        )
        .is_some(),
        "hovering on the leading `.` of a modifier should resolve a hover"
    );
}

#[test]
fn mouse_event_modifier_left_is_mouse_button_not_arrow_key() {
    // F2b: `@click.left` must describe the LEFT MOUSE BUTTON. The context-free
    // modifier table would return "Arrow Left" (key family) — passing the event name
    // (`click`) disambiguates to the mouse-button family.
    let source = r#"<template><button @click.left="pick">x</button></template>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let at_pos = source.find("@click").unwrap() as u32;
    let click_pos = at_pos + 1;
    let dot_pos = source.find(".left").unwrap() as u32;
    let left_pos = dot_pos + 1;
    let dir = TemplateDirective {
        name: "on".into(),
        raw_name: "@click".into(),
        argument: Some("click".into()),
        modifiers: vec!["left".into()],
        expression: Some("pick".into()),
        span: verter_span::Span::new(at_pos, left_pos + 4),
        name_end: click_pos,
        arg_span: Some(verter_span::Span::new(click_pos, click_pos + 5)),
        expression_span: None,
        modifier_spans: vec![verter_span::Span::new(left_pos, left_pos + 4)],
    };
    let analysis = template_only_analysis(native_event_element("button", dir));

    let pos = line_index.offset_to_position(left_pos + 1).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false)
        .expect("mouse-button modifier hover should exist");
    let contents = markup(&hover);
    assert!(
        contents.contains("mouse button"),
        "@click.left must describe the mouse button, got: {contents}"
    );
    assert!(
        !contents.contains("Arrow"),
        "@click.left must NOT use the arrow-key reading, got: {contents}"
    );
}

#[test]
fn no_value_event_modifier_hover() {
    // The exact user example `<div @touchmove.stop />` — a no-value event
    // directive deleted entirely from the generated TSX — must still resolve a
    // source-owned hover for BOTH the event token and the modifier, with no
    // dependency on any generated TSX anchor.
    let source = r#"<template><div @touchmove.stop></div></template>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let at_pos = source.find("@touchmove").unwrap() as u32;
    let event_pos = at_pos + 1; // 'touchmove' = 9 chars
    let dot_pos = source.find(".stop").unwrap() as u32;
    let stop_pos = dot_pos + 1;
    let dir = TemplateDirective {
        name: "on".into(),
        raw_name: "@touchmove".into(),
        argument: Some("touchmove".into()),
        modifiers: vec!["stop".into()],
        expression: None,
        span: verter_span::Span::new(at_pos, stop_pos + 4),
        name_end: event_pos,
        arg_span: Some(verter_span::Span::new(event_pos, event_pos + 9)),
        expression_span: None,
        modifier_spans: vec![verter_span::Span::new(stop_pos, stop_pos + 4)],
    };
    let analysis = template_only_analysis(native_event_element("div", dir));

    // Hover on the event token `touchmove`.
    let event_hover_pos = line_index.offset_to_position(event_pos + 2).unwrap();
    let event_hover = hover_at_position(
        &event_hover_pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    )
    .expect("no-value event token hover should exist");
    assert!(
        markup(&event_hover).contains("@touchmove"),
        "no-value event hover must name `@touchmove`, got: {}",
        markup(&event_hover)
    );

    // Hover on the modifier `stop`.
    let mod_hover_pos = line_index.offset_to_position(stop_pos + 1).unwrap();
    let mod_hover = hover_at_position(
        &mod_hover_pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        false,
    )
    .expect("no-value modifier hover should exist");
    let mod_contents = markup(&mod_hover);
    assert!(
        mod_contents.contains(".stop"),
        "no-value modifier hover must name `.stop`, got: {mod_contents}"
    );
    assert!(
        mod_contents.contains("stopPropagation"),
        "no-value modifier hover must describe stopPropagation, got: {mod_contents}"
    );
}

#[test]
fn test_hover_on_ref_in_interpolation_still_shows_import() {
    // Hovering on `ref` in {{ ref(0) }} should still show the import hover.
    let source = "<template>\n  {{ ref(0) }}\n</template>\n\n<script setup>\nimport { ref } from 'vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = FileAnalysisSnapshot {
        imports: vec![AnalyzedImport {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "ref".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: Some(VueApiClassification::Ref),
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        }],
        template: Some(
            (TemplateAnalysisSnapshot {
                elements: vec![],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // Find "ref" in the template interpolation
    let ref_offset = source.find("ref(0)").unwrap();
    let pos = line_index.offset_to_position(ref_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index, false);

    assert!(
        hover.is_some(),
        "should show hover for ref in interpolation"
    );
    let contents = match hover.unwrap().hover.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        contents.contains("import"),
        "should show import hover for ref in interpolation, got: {}",
        contents
    );
}

// ── Delegate to TypeProvider for generic/attrs attribute values ──────────────

#[test]
fn test_hover_on_generic_attr_name_shows_docs() {
    let source = r#"<script setup lang="ts" generic="T extends string">
const x = 1;
</script>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Hover on the "generic" attribute NAME → should show SFC docs
    let name_offset = source.find("generic").unwrap();
    let pos = line_index.offset_to_position(name_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(
        hover.is_some(),
        "should show SFC docs when hovering on 'generic' attribute name"
    );
}

#[test]
fn test_hover_on_generic_attr_value_returns_none() {
    let source = r#"<script setup lang="ts" generic="T extends string">
const x = 1;
</script>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Hover INSIDE the generic value "T extends string" → should return None
    // to delegate to TypeProvider
    let value_offset = source.find("T extends").unwrap();
    let pos = line_index.offset_to_position(value_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(
        hover.is_none(),
        "should return None for cursor inside generic value (delegates to TypeProvider)"
    );
}

#[test]
fn test_hover_on_attrs_attr_value_returns_none() {
    let source = r#"<script setup lang="ts" attrs="{ class: string }">
const x = 1;
</script>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Hover INSIDE the attrs value "{ class: string }" → should return None
    let value_offset = source.find("class: string").unwrap();
    let pos = line_index.offset_to_position(value_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(
        hover.is_none(),
        "should return None for cursor inside attrs value (delegates to TypeProvider)"
    );
}

#[test]
fn test_hover_on_lang_attr_value_still_works() {
    let source = r#"<script setup lang="ts" generic="T">
const x = 1;
</script>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Hover inside `lang="ts"` value → should still show SFC attr docs (not generic/attrs)
    let value_offset = source.find("ts").unwrap();
    let pos = line_index.offset_to_position(value_offset as u32).unwrap();
    let hover = hover_at_position(&pos, source, &blocks, None, &line_index, false);
    assert!(
        hover.is_some(),
        "should show SFC docs for lang attribute value (not delegated)"
    );
}
