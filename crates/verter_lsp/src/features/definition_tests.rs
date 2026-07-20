use super::*;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use verter_semantic::analysis::types::ImportBindingKind;
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
            used_in_script: false,
            used_in_style: false,
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
fn test_go_to_import_with_resolved_canonical_id_no_export_resolver_falls_back_to_import_span() {
    // When resolved_canonical_id is set but no precise export resolver is available,
    // fall back to the import statement span instead of returning None.
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
        None,
    );
    let response = result.expect("should fall back to import span");
    let location = match response {
        GotoDefinitionResponse::Scalar(location) => location,
        other => panic!("expected scalar fallback location, got {other:?}"),
    };
    assert_eq!(
        location.uri.as_str(),
        crate::features::definition::SAME_FILE_URI_STR
    );
    assert_eq!(location.range.start.line, 1);
    assert_eq!(location.range.end.line, 1);
}

#[test]
fn test_go_to_import_with_resolved_canonical_id_and_export_resolver() {
    // When resolved_canonical_id is set AND resolve_export_location returns a location,
    // returns the precise cross-file location
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
            span: verter_span::Span::new(15, 40),
            resolved_canonical_id: Some("/usr/lib/node_modules/vue/dist/vue.d.ts".to_string()),
        }],
        vec![],
    );

    let ref_offset = source.find("ref").unwrap();
    let position = line_index.offset_to_position(ref_offset as u32).unwrap();

    let export_resolver = |canonical_id: &str, binding_name: &str| -> Option<Location> {
        if canonical_id.contains("vue.d.ts") && binding_name == "ref" {
            Some(Location {
                uri: "file:///usr/lib/node_modules/vue/dist/vue.d.ts"
                    .parse()
                    .unwrap(),
                range: Range {
                    start: Position {
                        line: 100,
                        character: 16,
                    },
                    end: Position {
                        line: 100,
                        character: 19,
                    },
                },
            })
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
        None,
        Some(&export_resolver),
    );
    assert!(result.is_some(), "should navigate with export resolver");

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(loc.uri.as_str().contains("vue.d.ts"));
        assert_eq!(loc.range.start.line, 100);
        assert_eq!(loc.range.start.character, 16);
    } else {
        panic!("expected scalar location");
    }
}

#[test]
fn test_go_to_import_falls_back_to_path_resolution_when_resolved_canonical_id_fails() {
    let source = "<script setup>\nimport { Overlay } from './components'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "./components".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Overlay".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(24, 31),
            }],
            span: verter_span::Span::new(15, 51),
            resolved_canonical_id: Some("/project/components".to_string()),
        }],
        vec![],
    );

    let overlay_offset = source.find("Overlay").unwrap();
    let position = line_index
        .offset_to_position(overlay_offset as u32)
        .unwrap();

    let resolve_path = |specifier: &str| -> Option<String> {
        (specifier == "./components").then(|| "/project/components/index.ts".to_string())
    };
    let export_resolver = |canonical_id: &str, binding_name: &str| -> Option<Location> {
        if canonical_id == "/project/components/index.ts" && binding_name == "Overlay" {
            Some(Location {
                uri: "file:///project/components/Overlay.vue".parse().unwrap(),
                range: Range {
                    start: Position {
                        line: 1,
                        character: 6,
                    },
                    end: Position {
                        line: 1,
                        character: 13,
                    },
                },
            })
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
        Some(&resolve_path),
        Some(&export_resolver),
    );
    let Some(GotoDefinitionResponse::Scalar(loc)) = result else {
        panic!("expected scalar location");
    };
    assert_eq!(loc.uri.as_str(), "file:///project/components/Overlay.vue");
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
                kind: ImportBindingKind::Named,
                imported_name: None,
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
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
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
            used_in_script: false,
            used_in_style: false,
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
            used_in_script: false,
            used_in_style: false,
        }],
        vec![AnalyzedImport {
            source: "./MyComponent.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "MyComponent".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
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

    use verter_semantic::analysis::template::*;

    let analysis = FileAnalysisSnapshot {
        imports: vec![AnalyzedImport {
            source: "./ChildComp.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "ChildComp".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: Some("/project/ChildComp.vue".to_string()),
        }],
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    bindings: vec![],
                    events: vec![],
                    span: verter_span::Span::new(0, 0),
                }],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // Click on "ChildComp" in template — without export resolver, falls back to file navigation
    let offset = source.find("ChildComp").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );
    // With .vue fallback, navigates to file top even without export resolver
    assert!(result.is_some(), "should navigate to .vue file as fallback");
    if let Some(GotoDefinitionResponse::Scalar(loc)) = &result {
        assert!(loc.uri.as_str().contains("ChildComp.vue"));
        assert_eq!(loc.range, Range::default());
    } else {
        panic!("expected scalar location");
    }

    // With export resolver, navigates to precise location
    let export_resolver = |canonical_id: &str, _binding_name: &str| -> Option<Location> {
        if canonical_id.contains("ChildComp.vue") {
            Some(Location {
                uri: "file:///project/ChildComp.vue".parse().unwrap(),
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 9,
                    },
                },
            })
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
        None,
        Some(&export_resolver),
    );
    assert!(result.is_some(), "should navigate with export resolver");

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

    use verter_semantic::analysis::style::*;
    use verter_semantic::analysis::template::*;

    // Find the offsets for the style block content
    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css =
        &source[style_block.content_range().0 as usize..style_block.content_range().1 as usize];

    // Build analysis with template element + style analysis
    let class_attr_start = source.find("class=\"btn\"").unwrap() as u32;
    let class_attr_end = class_attr_start + "class=\"btn\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )])
        .into(),
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

    use verter_semantic::analysis::style::*;
    use verter_semantic::analysis::template::*;

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css = &source[style_content_start as usize..style_block.content_range().1 as usize];

    let class_attr_start = source.find("class=\"btn primary\"").unwrap() as u32;
    let class_attr_end = class_attr_start + "class=\"btn primary\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )])
        .into(),
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

    use verter_semantic::analysis::style::*;
    use verter_semantic::analysis::template::*;

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css = &source[style_content_start as usize..style_block.content_range().1 as usize];

    let id_attr_start = source.find("id=\"app\"").unwrap() as u32;
    let id_attr_end = id_attr_start + "id=\"app\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )])
        .into(),
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
        None,
    );
    assert!(
        result.is_some(),
        "should navigate from template id to style"
    );
}

#[test]
fn test_css_nav_dynamic_class_object_key_navigates() {
    let source = "<template>\n  <div :class=\"{ active: true }\"></div>\n</template>\n\n<style>\n.active { }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_semantic::analysis::style::*;
    use verter_semantic::analysis::template::*;

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (scs, _) = style_block.content_range();
    let style_css = &source[scs as usize..style_block.content_range().1 as usize];

    let attr_start = source.find(":class").unwrap() as u32;
    let attr_end = attr_start + ":class=\"{ active: true }\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            scs,
        )])
        .into(),
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
        None,
    );
    // A resolvable `:class` object KEY navigates to its declaring rule —
    // the key names a class, not a script binding.
    let result = result.expect(":class object key with a matching rule must navigate");
    if let GotoDefinitionResponse::Scalar(loc) = result {
        // Should land exactly on "active" inside `.active { }` in style.
        let style_active_offset = source.rfind("active").unwrap() as u32;
        let expected_start = line_index.offset_to_position(style_active_offset).unwrap();
        let expected_end = line_index
            .offset_to_position(style_active_offset + "active".len() as u32)
            .unwrap();
        assert_eq!(loc.range.start, expected_start);
        assert_eq!(loc.range.end, expected_end);
    } else {
        panic!("expected scalar location for the single declaring rule");
    }
}

#[test]
fn test_css_nav_style_to_template() {
    let source = "<template>\n  <div class=\"btn\"></div>\n</template>\n\n<style>\n.btn { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_semantic::analysis::style::*;
    use verter_semantic::analysis::template::*;

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css = &source[style_content_start as usize..style_block.content_range().1 as usize];

    let class_attr_start = source.find("class=\"btn\"").unwrap() as u32;
    let class_attr_end = class_attr_start + "class=\"btn\"".len() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )])
        .into(),
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
        None,
    );
    assert!(
        result.is_some(),
        "should navigate from style .btn to template class=\"btn\""
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        // Exact value-token range: the "btn" inside class="btn", not the
        // whole attribute.
        let token_start = source.find("class=\"btn\"").unwrap() as u32 + "class=\"".len() as u32;
        let expected_start = line_index.offset_to_position(token_start).unwrap();
        let expected_end = line_index
            .offset_to_position(token_start + "btn".len() as u32)
            .unwrap();
        assert_eq!(loc.range.start, expected_start);
        assert_eq!(loc.range.end, expected_end);
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
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(import_start, import_end),
            resolved_canonical_id: Some("/project/Foo.vue".to_string()),
        }],
        vec![],
    );

    // Click on "./" inside the import source string (not on the binding name)
    let dot_slash_offset = source.find("./Foo.vue").unwrap();
    let position = line_index
        .offset_to_position(dot_slash_offset as u32)
        .unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
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
                kind: ImportBindingKind::Named,
                imported_name: None,
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

    // With path resolver but no export resolver: falls back to .vue file navigation
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
        None,
    );
    assert!(result.is_some(), "should navigate to .vue file as fallback");
    if let Some(GotoDefinitionResponse::Scalar(loc)) = &result {
        assert!(loc.uri.as_str().contains("Foo.vue"));
        assert_eq!(loc.range, Range::default());
    } else {
        panic!("expected scalar location");
    }

    // With both resolvers: navigates to precise location
    let export_resolver = |canonical_id: &str, binding_name: &str| -> Option<Location> {
        if canonical_id.contains("Foo.vue") && binding_name == "Foo" {
            Some(Location {
                uri: "file:///project/src/components/Foo.vue".parse().unwrap(),
                range: Range {
                    start: Position {
                        line: 5,
                        character: 0,
                    },
                    end: Position {
                        line: 5,
                        character: 3,
                    },
                },
            })
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
        Some(&export_resolver),
    );
    assert!(result.is_some(), "should navigate with export resolver");

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(
            loc.uri.as_str().contains("Foo.vue"),
            "should resolve to Foo.vue, got: {}",
            loc.uri.as_str()
        );
        assert_eq!(loc.range.start.line, 5);
    } else {
        panic!("expected scalar location");
    }

    // Without any resolver: should fall back to import span
    let result_no_resolver = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
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
            SAME_FILE_URI_STR,
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
                kind: ImportBindingKind::Named,
                imported_name: None,
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
        None,
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
    use verter_semantic::analysis::style::*;
    use verter_semantic::analysis::template::*;
    use verter_semantic::analysis::types::*;

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
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        dom_query_calls: (vec![DomQueryCallSite {
            kind: DomQueryKind::QuerySelector,
            selector_text: ".btn".to_string(),
            parsed: Some(parsed),
            span: verter_span::Span::new(doc_start, call_end),
            arg_span: verter_span::Span::new(arg_start as u32, arg_end as u32),
        }])
        .into(),
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
    use verter_semantic::analysis::style::*;
    use verter_semantic::analysis::template::*;
    use verter_semantic::analysis::types::*;

    let source = "<template>\n  <div>hello</div>\n</template>\n\n<script setup>\ndocument.querySelector('.missing')\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let parsed = parse_selector(".missing").unwrap();

    // Use SFC-absolute offsets (spans are adjusted by verter_session during analysis)
    let qs_str_start = source.find("'.missing'").unwrap();
    let arg_start = qs_str_start + 1;
    let arg_end = arg_start + ".missing".len();

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        dom_query_calls: (vec![DomQueryCallSite {
            kind: DomQueryKind::QuerySelector,
            selector_text: ".missing".to_string(),
            parsed: Some(parsed),
            span: verter_span::Span::new(0, 40),
            arg_span: verter_span::Span::new(arg_start as u32, arg_end as u32),
        }])
        .into(),
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
        None,
    );
    assert!(
        result.is_none(),
        "no template element or CSS rule matches .missing"
    );
}

#[test]
fn test_dom_query_selector_falls_back_to_css() {
    use verter_semantic::analysis::style::*;
    use verter_semantic::analysis::template::*;
    use verter_semantic::analysis::types::*;

    // Template has no .btn element, but style has .btn rule
    let source = "<template>\n  <div>hello</div>\n</template>\n\n<script setup>\ndocument.querySelector('.btn')\n</script>\n\n<style>\n.btn { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (style_content_start, _) = style_block.content_range();
    let style_css = &source[style_content_start as usize..style_block.content_range().1 as usize];

    let parsed = parse_selector(".btn").unwrap();

    // Use SFC-absolute offsets (spans are adjusted by verter_session during analysis)
    let qs_str_start = source.find("'.btn'").unwrap();
    let arg_start = qs_str_start + 1;
    let arg_end = arg_start + ".btn".len();

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into(),
        ),
        dom_query_calls: (vec![DomQueryCallSite {
            kind: DomQueryKind::QuerySelector,
            selector_text: ".btn".to_string(),
            parsed: Some(parsed),
            span: verter_span::Span::new(0, 40),
            arg_span: verter_span::Span::new(arg_start as u32, arg_end as u32),
        }])
        .into(),
        styles: (vec![build_css_style_analysis(
            style_css,
            VueStyleInput::default(),
            false,
            false,
            None,
            style_content_start,
        )])
        .into(),
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

    use verter_semantic::analysis::template::*;

    let analysis = FileAnalysisSnapshot {
        imports: vec![AnalyzedImport {
            source: "@/components/FooComp.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "FooComp".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        }],
        template: Some(
            (TemplateAnalysisSnapshot {
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
                    bindings: vec![],
                    events: vec![],
                    span: verter_span::Span::new(0, 0),
                }],
                ..Default::default()
            })
            .into(),
        ),
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
    // Without export resolver: falls back to .vue file navigation
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        Some(&resolver),
        None,
    );
    assert!(result.is_some(), "should navigate to .vue file as fallback");
    if let Some(GotoDefinitionResponse::Scalar(loc)) = &result {
        assert!(loc.uri.as_str().contains("FooComp.vue"));
        assert_eq!(loc.range, Range::default());
    } else {
        panic!("expected scalar location");
    }

    // With export resolver: navigates to precise location
    let export_resolver = |canonical_id: &str, _binding_name: &str| -> Option<Location> {
        if canonical_id.contains("FooComp.vue") {
            Some(Location {
                uri: "file:///project/src/components/FooComp.vue"
                    .parse()
                    .unwrap(),
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 7,
                    },
                },
            })
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
        Some(&export_resolver),
    );
    assert!(
        result.is_some(),
        "should navigate to component via path + export resolver"
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

// ========================================================================
// Fix 3: Event handler navigation (Bug 6)
// ========================================================================

#[test]
fn test_go_to_definition_event_handler_click() {
    // CTRL+CLICK on `click` in `@click="handleClick"` → navigate to handleClick binding
    let source = "<template>\n  <button @click=\"handleClick\">go</button>\n</template>\n\n<script setup>\nfunction handleClick() {}\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let click_offset = source.find("@click").unwrap();
    let handler_offset = source.rfind("handleClick").unwrap();

    let analysis = FileAnalysisSnapshot {
        bindings: vec![AnalyzedBinding {
            name: "handleClick".to_string(),
            kind: AnalyzedBindingKind::Function,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(handler_offset as u32, (handler_offset + 11) as u32),
            used_in_script: false,
            used_in_style: false,
        }],
        template: Some(
            (TemplateAnalysisSnapshot {
                elements: vec![TemplateElement {
                    tag: "button".into(),
                    is_component: false,
                    is_self_closing: false,
                    namespace: ElementNamespace::Html,
                    attributes: vec![],
                    directives: vec![TemplateDirective {
                        name: "on".into(),
                        raw_name: "@click".into(),
                        argument: Some("click".into()),
                        modifiers: vec![],
                        expression: Some("handleClick".into()),
                        span: verter_span::Span::new(
                            click_offset as u32,
                            (click_offset + 22) as u32,
                        ),
                        name_end: (click_offset + 6) as u32,
                        arg_span: Some(verter_span::Span::new(
                            (click_offset + 1) as u32,
                            (click_offset + 6) as u32,
                        )),
                        expression_span: None,
                        modifier_spans: vec![],
                    }],
                    v_for: None,
                    v_model: None,
                    has_v_if: false,
                    has_v_else: false,
                    has_v_else_if: false,
                    has_v_show: false,
                    has_v_html: false,
                    has_v_text: false,
                    has_text_content: true,
                    has_bare_text: true,
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(11, 60),
                    tag_span_end: 45,
                    content_end: 0,
                    ..Default::default()
                }],
                event_handlers: vec![verter_semantic::analysis::template::TemplateEventHandler {
                    event_name: "click".into(),
                    handler_binding: Some("handleClick".into()),
                    is_inline: false,
                    target_tag: "button".into(),
                    // TemplateEventHandler.span is the ELEMENT span (set by extract_event_handlers)
                    span: verter_span::Span::new(11, 60),
                }],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // CTRL+CLICK on "click" part of @click
    let pos = line_index
        .offset_to_position((click_offset + 1) as u32)
        .unwrap();
    let result = definition_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );

    assert!(
        result.is_some(),
        "should navigate from @click to handleClick binding"
    );
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(
            loc.uri.as_str(),
            SAME_FILE_URI_STR,
            "should navigate within same file"
        );
        let start_offset = line_index.position_to_offset(&loc.range.start).unwrap();
        assert_eq!(
            start_offset, handler_offset as u32,
            "should navigate to handleClick function"
        );
    } else {
        panic!("expected scalar definition");
    }
}

#[test]
fn test_go_to_definition_inline_event_no_binding() {
    // CTRL+CLICK on `click` in `@click="count++"` → returns None (inline, no handler binding)
    let source =
        "<template>\n  <button @click=\"count++\">go</button>\n</template>\n\n<script setup>\nlet count = 0\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let click_offset = source.find("@click").unwrap();

    let analysis = FileAnalysisSnapshot {
        bindings: vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Let,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(0, 0),
            used_in_script: false,
            used_in_style: false,
        }],
        template: Some(
            (TemplateAnalysisSnapshot {
                elements: vec![TemplateElement {
                    tag: "button".into(),
                    is_component: false,
                    is_self_closing: false,
                    namespace: ElementNamespace::Html,
                    attributes: vec![],
                    directives: vec![TemplateDirective {
                        name: "on".into(),
                        raw_name: "@click".into(),
                        argument: Some("click".into()),
                        modifiers: vec![],
                        expression: Some("count++".into()),
                        span: verter_span::Span::new(
                            click_offset as u32,
                            (click_offset + 17) as u32,
                        ),
                        name_end: (click_offset + 6) as u32,
                        arg_span: Some(verter_span::Span::new(
                            (click_offset + 1) as u32,
                            (click_offset + 6) as u32,
                        )),
                        expression_span: None,
                        modifier_spans: vec![],
                    }],
                    v_for: None,
                    v_model: None,
                    has_v_if: false,
                    has_v_else: false,
                    has_v_else_if: false,
                    has_v_show: false,
                    has_v_html: false,
                    has_v_text: false,
                    has_text_content: true,
                    has_bare_text: true,
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(11, 55),
                    tag_span_end: 40,
                    content_end: 0,
                    ..Default::default()
                }],
                event_handlers: vec![verter_semantic::analysis::template::TemplateEventHandler {
                    event_name: "click".into(),
                    handler_binding: None, // inline expression, no binding
                    is_inline: true,
                    target_tag: "button".into(),
                    // TemplateEventHandler.span is the ELEMENT span
                    span: verter_span::Span::new(11, 55),
                }],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // CTRL+CLICK on "click" in @click — inline expr, should return None
    let pos = line_index
        .offset_to_position((click_offset + 1) as u32)
        .unwrap();
    let result = definition_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );

    assert!(
        result.is_none(),
        "inline @click expression should not navigate"
    );
}

#[test]
fn test_go_to_definition_component_event_name_defers_to_server() {
    use verter_semantic::analysis::template::*;

    let source = "<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n\n<script setup>\nfunction handleCustom() {}\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let event_offset = source.find("@custom").unwrap();
    let handler_offset = source.find("handleCustom").unwrap();

    let analysis = FileAnalysisSnapshot {
        bindings: vec![AnalyzedBinding {
            name: "handleCustom".to_string(),
            kind: AnalyzedBindingKind::Function,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(
                handler_offset as u32,
                (handler_offset + "handleCustom".len()) as u32,
            ),
            used_in_script: true,
            used_in_style: false,
        }],
        template: Some(
            (TemplateAnalysisSnapshot {
                elements: vec![TemplateElement {
                    tag: "MyComp".into(),
                    is_component: true,
                    is_self_closing: true,
                    namespace: ElementNamespace::Html,
                    attributes: vec![],
                    directives: vec![TemplateDirective {
                        name: "on".into(),
                        raw_name: "@custom".into(),
                        argument: Some("custom".into()),
                        modifiers: vec![],
                        expression: Some("handleCustom".into()),
                        span: verter_span::Span::new(
                            event_offset as u32,
                            (event_offset + "@custom=\"handleCustom\"".len()) as u32,
                        ),
                        name_end: (event_offset + "@custom".len()) as u32,
                        arg_span: Some(verter_span::Span::new(
                            (event_offset + 1) as u32,
                            (event_offset + "@custom".len()) as u32,
                        )),
                        expression_span: None,
                        modifier_spans: vec![],
                    }],
                    span: verter_span::Span::new(11, 44),
                    tag_span_end: 44,
                    content_end: 0,
                    ..Default::default()
                }],
                event_handlers: vec![TemplateEventHandler {
                    event_name: "custom".into(),
                    handler_binding: Some("handleCustom".into()),
                    is_inline: false,
                    target_tag: "MyComp".into(),
                    span: verter_span::Span::new(11, 44),
                }],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    let pos = line_index
        .offset_to_position((event_offset + 1) as u32)
        .unwrap();
    let result = definition_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );

    assert!(
        result.is_none(),
        "component event names should defer to server-side child resolution"
    );
}

// ========================================================================
// Fix 4: $props navigation (Bug 10)
// ========================================================================

#[test]
fn test_go_to_definition_dollar_props() {
    // CTRL+CLICK on `$props` → navigate to defineProps macro call
    let source = "<template>\n  {{ $props.msg }}\n</template>\n\n<script setup>\nconst props = defineProps<{msg: string}>()\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let props_offset = source.find("$props").unwrap();
    let define_offset = source.find("defineProps").unwrap();

    let analysis = FileAnalysisSnapshot {
        macros: (vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec![],
            binding_name: Some("props".into()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: verter_span::Span::new(define_offset as u32, (define_offset + 30) as u32),
        }])
        .into(),
        template: Some(
            (TemplateAnalysisSnapshot {
                elements: vec![],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    let pos = line_index.offset_to_position(props_offset as u32).unwrap();
    let result = definition_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );

    assert!(
        result.is_some(),
        "should navigate from $props to defineProps"
    );
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let start_offset = line_index.position_to_offset(&loc.range.start).unwrap();
        assert_eq!(
            start_offset, define_offset as u32,
            "should navigate to defineProps call"
        );
    } else {
        panic!("expected scalar definition");
    }
}

#[test]
fn test_go_to_definition_dollar_emit() {
    // CTRL+CLICK on `$emit` → navigate to defineEmits macro call
    let source = "<template>\n  <button @click=\"$emit('done')\">go</button>\n</template>\n\n<script setup>\nconst emit = defineEmits(['done'])\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let emit_offset = source.find("$emit").unwrap();
    let define_offset = source.find("defineEmits").unwrap();

    let analysis = FileAnalysisSnapshot {
        macros: (vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            is_type_based: false,
            type_references: vec![],
            binding_name: Some("emit".into()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: verter_span::Span::new(define_offset as u32, (define_offset + 22) as u32),
        }])
        .into(),
        template: Some(
            (TemplateAnalysisSnapshot {
                elements: vec![],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    let pos = line_index.offset_to_position(emit_offset as u32).unwrap();
    let result = definition_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );

    assert!(
        result.is_some(),
        "should navigate from $emit to defineEmits"
    );
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let start_offset = line_index.position_to_offset(&loc.range.start).unwrap();
        assert_eq!(
            start_offset, define_offset as u32,
            "should navigate to defineEmits call"
        );
    } else {
        panic!("expected scalar definition");
    }
}

#[test]
fn test_go_to_definition_dollar_props_without_macro() {
    // CTRL+CLICK on `$props` without any defineProps → returns None
    let source = "<template>\n  {{ $props.msg }}\n</template>\n\n<script setup>\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let props_offset = source.find("$props").unwrap();

    let analysis = FileAnalysisSnapshot {
        macros: (vec![]).into(),
        template: Some(
            (TemplateAnalysisSnapshot {
                elements: vec![],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    let pos = line_index.offset_to_position(props_offset as u32).unwrap();
    let result = definition_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );

    assert!(
        result.is_none(),
        "should return None when no defineProps exists"
    );
}

// =========================================================================
// Prop field go-to-definition
// =========================================================================

/// Ctrl+Click on a prop binding in template navigates to the prop declaration.
#[test]
fn definition_prop_field_type_based() {
    let source = "<template>\n  {{ count }}\n</template>\n\n<script setup lang=\"ts\">\nconst props = defineProps<{ count: number }>()\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Find the span of "count" in the defineProps type parameter
    // "defineProps<{ count: number }>" — find "count" after the opening brace
    let define_props_offset = source.find("defineProps").unwrap();
    let type_count_offset =
        source[define_props_offset..].find("count").unwrap() + define_props_offset;
    let type_count_end = type_count_offset + 5;

    let analysis = make_analysis(
        vec![], // No regular bindings — count is a prop, not a top-level binding
        vec![],
        vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec![],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![AnalyzedPropField {
                name: "count".to_string(),
                span: verter_span::Span::new(type_count_offset as u32, type_count_end as u32),
                type_annotation: None,
                is_optional: false,
                description: None,
                tags: vec![],
                resolution_source: TypeResolutionSource::Rust,
                resolution_error: None,
                payload: None,
                type_expr_scope: None,
                declared_in_macro_type_arg: false,
            }],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: verter_span::Span::new(
                define_props_offset as u32,
                (define_props_offset + 45) as u32,
            ),
        }],
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
        None,
    );

    assert!(result.is_some(), "should navigate to prop declaration");

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let expected_start = line_index
            .offset_to_position(type_count_offset as u32)
            .unwrap();
        let expected_end = line_index
            .offset_to_position(type_count_end as u32)
            .unwrap();
        assert_eq!(
            loc.range.start, expected_start,
            "should point to count in defineProps type param"
        );
        assert_eq!(loc.range.end, expected_end);
    } else {
        panic!("expected scalar location");
    }
}

/// Ctrl+Click on a prop in template navigates to the runtime prop declaration.
#[test]
fn definition_prop_field_runtime() {
    let source = "<template>\n  {{ name }}\n</template>\n\n<script setup>\ndefineProps({ name: String })\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Find the span of "name" in the defineProps runtime argument
    let define_props_offset = source.find("defineProps").unwrap();
    let runtime_name_offset =
        source[define_props_offset..].find("name").unwrap() + define_props_offset;
    let runtime_name_end = runtime_name_offset + 4;

    let analysis = make_analysis(
        vec![],
        vec![],
        vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: false,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![AnalyzedPropField {
                name: "name".to_string(),
                span: verter_span::Span::new(runtime_name_offset as u32, runtime_name_end as u32),
                type_annotation: None,
                is_optional: false,
                description: None,
                tags: vec![],
                resolution_source: TypeResolutionSource::Rust,
                resolution_error: None,
                payload: None,
                type_expr_scope: None,
                declared_in_macro_type_arg: false,
            }],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: verter_span::Span::new(
                define_props_offset as u32,
                (define_props_offset + 28) as u32,
            ),
        }],
    );

    // Click on "name" in template
    let template_name_offset = source.find("name").unwrap();
    let position = line_index
        .offset_to_position(template_name_offset as u32)
        .unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );

    assert!(
        result.is_some(),
        "should navigate to runtime prop declaration"
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let expected_start = line_index
            .offset_to_position(runtime_name_offset as u32)
            .unwrap();
        assert_eq!(
            loc.range.start, expected_start,
            "should point to name in defineProps object"
        );
    } else {
        panic!("expected scalar location");
    }
}

/// When both a regular binding and a prop field share a name, the binding wins.
#[test]
fn definition_binding_takes_precedence_over_prop_field() {
    let source = "<template>\n  {{ count }}\n</template>\n\n<script setup lang=\"ts\">\nconst count = ref(0)\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

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
            used_in_script: false,
            used_in_style: false,
        }],
        vec![],
        vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec![],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![AnalyzedPropField {
                name: "count".to_string(),
                span: verter_span::Span::new(100, 105),
                type_annotation: None,
                is_optional: false,
                description: None,
                tags: vec![],
                resolution_source: TypeResolutionSource::Rust,
                resolution_error: None,
                payload: None,
                type_expr_scope: None,
                declared_in_macro_type_arg: false,
            }],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: verter_span::Span::new(90, 140),
        }],
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
        None,
    );

    assert!(result.is_some(), "should find definition");

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let binding_start = line_index.offset_to_position(script_count_offset).unwrap();
        assert_eq!(
            loc.range.start, binding_start,
            "binding should take precedence over prop field"
        );
    } else {
        panic!("expected scalar location");
    }
}

// =============================================================================
// "default" fallback for .vue imports
// =============================================================================

/// @ai-generated - Tests that default import of .vue file retries with "default" export name
#[test]
fn test_vue_default_import_retries_with_default_binding() {
    // When the resolver can't match the local name but CAN match "default", returns the result
    let source = "<script setup>\nimport MyComp from './Child.vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "./Child.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "MyComp".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: Some("/project/Child.vue".to_string()),
        }],
        vec![],
    );

    // Resolver only matches "default", not "MyComp"
    let export_resolver = |canonical_id: &str, binding_name: &str| -> Option<Location> {
        if canonical_id == "/project/Child.vue" && binding_name == "default" {
            Some(Location {
                uri: "file:///project/Child.vue".parse().unwrap(),
                range: Range {
                    start: Position {
                        line: 2,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 10,
                    },
                },
            })
        } else {
            None
        }
    };

    let offset = source.find("MyComp").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        Some(&export_resolver),
    );

    assert!(result.is_some(), "should resolve via 'default' fallback");
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(loc.uri.as_str().contains("Child.vue"));
        assert_eq!(
            loc.range.start.line, 2,
            "should use precise location from resolver"
        );
    } else {
        panic!("expected scalar location");
    }
}

/// @ai-generated - Named import from non-.vue file still returns None without resolver
#[test]
fn test_named_import_non_carrier_no_default_fallback() {
    let source = "<script setup>\nimport { helper } from './utils'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "./utils".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "helper".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: Some("/project/utils.ts".to_string()),
        }],
        vec![],
    );

    let offset = source.find("helper").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    // Without export resolver, non-.vue targets still return None
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );
    assert!(
        result.is_none(),
        "non-.vue imports should not get file fallback"
    );
}

/// @ai-generated - Component tag in template uses "default" fallback for .vue imports
#[test]
fn test_component_tag_default_fallback() {
    let source = "<template>\n  <WrappedBtn />\n</template>\n\n<script setup>\nimport WrappedBtn from './WrappedBtn.vue'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    use verter_semantic::analysis::template::*;

    let analysis = FileAnalysisSnapshot {
        imports: vec![AnalyzedImport {
            source: "./WrappedBtn.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "WrappedBtn".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: Some("/project/WrappedBtn.vue".to_string()),
        }],
        template: Some(
            (TemplateAnalysisSnapshot {
                components: vec![TemplateComponentUsage {
                    name: "WrappedBtn".to_string(),
                    import_source: Some("./WrappedBtn.vue".to_string()),
                    is_dynamic: false,
                    props: vec![],
                    has_spread: false,
                    slots_used: vec![],
                    static_classes: vec![],
                    has_dynamic_class: false,
                    dynamic_classes: vec![],
                    v_models: vec![],
                    bindings: vec![],
                    events: vec![],
                    span: verter_span::Span::new(0, 0),
                }],
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };

    // Resolver only matches "default"
    let export_resolver = |canonical_id: &str, binding_name: &str| -> Option<Location> {
        if canonical_id == "/project/WrappedBtn.vue" && binding_name == "default" {
            Some(Location {
                uri: "file:///project/WrappedBtn.vue".parse().unwrap(),
                range: Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 5,
                    },
                },
            })
        } else {
            None
        }
    };

    // Click on "WrappedBtn" in template tag
    let offset = source.find("WrappedBtn").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        Some(&export_resolver),
    );

    assert!(
        result.is_some(),
        "should resolve component via 'default' fallback"
    );
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(loc.uri.as_str().contains("WrappedBtn.vue"));
        assert_eq!(loc.range.start.line, 1);
    } else {
        panic!("expected scalar location");
    }
}

/// @ai-generated - Script-context import binding uses "default" fallback for .vue imports
#[test]
fn test_script_context_vue_import_default_fallback() {
    let source = "<script setup>\nimport Comp from './Comp.vue'\nconst x = 1\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "./Comp.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Comp".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: Some("/project/Comp.vue".to_string()),
        }],
        vec![],
    );

    // Resolver only matches "default"
    let export_resolver = |canonical_id: &str, binding_name: &str| -> Option<Location> {
        if canonical_id == "/project/Comp.vue" && binding_name == "default" {
            Some(Location {
                uri: "file:///project/Comp.vue".parse().unwrap(),
                range: Range {
                    start: Position {
                        line: 3,
                        character: 0,
                    },
                    end: Position {
                        line: 3,
                        character: 5,
                    },
                },
            })
        } else {
            None
        }
    };

    // Click on "Comp" in the import statement (script context)
    let offset = source.find("Comp").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        Some(&export_resolver),
    );

    assert!(
        result.is_some(),
        "should resolve via 'default' fallback in script"
    );
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(loc.uri.as_str().contains("Comp.vue"));
        assert_eq!(loc.range.start.line, 3);
    } else {
        panic!("expected scalar location");
    }
}

// =====================================================================
// B4: hierarchy-aware multi-target class navigation + fail-closed
// =====================================================================

/// Minimal element builder for CSS-navigation tests.
fn css_nav_element(
    tag: &str,
    class_value: &str,
    attr_span: verter_span::Span,
    el_span: verter_span::Span,
    parent_index: Option<u32>,
    nesting_depth: u16,
) -> verter_semantic::analysis::template::TemplateElement {
    use verter_semantic::analysis::template::*;
    TemplateElement {
        tag: tag.to_string(),
        namespace: ElementNamespace::Html,
        attributes: vec![TemplateAttribute {
            name: "class".to_string(),
            value: Some(class_value.to_string()),
            is_dynamic: false,
            span: attr_span,
            name_end: attr_span.start + 5,
            value_span: None,
        }],
        nesting_depth,
        parent_index,
        span: el_span,
        ..Default::default()
    }
}

/// Two rules declare `.title`; the one whose selector matches the elements
/// ancestry ranks first; ALL declaration locations are returned.
#[test]
fn css_class_definition_returns_all_declaring_rules_hierarchy_first() {
    let source = "<template>\n  <div class=\"card\"><span class=\"title\"></span></div>\n</template>\n<style scoped>\n.other .title { color: blue; }\n.card .title { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (scs, sce) = style_block.content_range();
    let style_css = &source[scs as usize..sce as usize];

    let card_attr = source.find("class=\"card\"").unwrap() as u32;
    let title_attr = source.find("class=\"title\"").unwrap() as u32;
    let div_start = source.find("<div").unwrap() as u32;
    let span_start = source.find("<span").unwrap() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (verter_semantic::analysis::template::TemplateAnalysisSnapshot {
                elements: vec![
                    css_nav_element(
                        "div",
                        "card",
                        verter_span::Span::new(card_attr, card_attr + 12),
                        verter_span::Span::new(div_start, div_start + 60),
                        None,
                        0,
                    ),
                    css_nav_element(
                        "span",
                        "title",
                        verter_span::Span::new(title_attr, title_attr + 13),
                        verter_span::Span::new(span_start, span_start + 25),
                        Some(0),
                        1,
                    ),
                ],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![verter_semantic::analysis::build_css_style_analysis(
            style_css,
            verter_semantic::analysis::VueStyleInput::default(),
            true,
            false,
            None,
            scs,
        )])
        .into(),
        ..Default::default()
    };

    // Cursor on "title" inside class="title"
    let cursor = source.find("class=\"title\"").unwrap() + 7;
    let position = line_index.offset_to_position(cursor as u32).unwrap();
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    )
    .expect("class with two declaring rules must navigate");

    let locations = match result {
        GotoDefinitionResponse::Array(locs) => locs,
        other => panic!("expected Array of declaration locations, got {other:?}"),
    };
    assert_eq!(locations.len(), 2, "both declaring rules are targets");

    // First target = the hierarchy-matching `.card .title` rule token.
    let card_title_token = source.find(".card .title").unwrap() + ".card .".len();
    let expected_first = line_index
        .offset_to_position(card_title_token as u32)
        .unwrap();
    assert_eq!(
        locations[0].range.start, expected_first,
        "hierarchy-matching rule ranks first"
    );

    // Second target = the non-matching `.other .title` declaration.
    let other_title_token = source.find(".other .title").unwrap() + ".other .".len();
    let expected_second = line_index
        .offset_to_position(other_title_token as u32)
        .unwrap();
    assert_eq!(locations[1].range.start, expected_second);
}

/// A class token with NO declaring rule fails closed even when a script
/// binding shares the name — never a mis-mapped jump into the script.
#[test]
fn css_class_definition_fails_closed_on_no_rule_despite_binding_collision() {
    let source = "<template>\n  <div class=\"primary\"></div>\n</template>\n<script setup>\nconst primary = 1\n</script>\n<style scoped>\n.unrelated { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (scs, sce) = style_block.content_range();
    let style_css = &source[scs as usize..sce as usize];
    let attr = source.find("class=\"primary\"").unwrap() as u32;
    let binding_start = source.find("const primary").unwrap() as u32 + 6;

    let analysis = FileAnalysisSnapshot {
        bindings: vec![AnalyzedBinding {
            name: "primary".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(binding_start, binding_start + 7),
            used_in_script: false,
            used_in_style: false,
        }],
        template: Some(
            (verter_semantic::analysis::template::TemplateAnalysisSnapshot {
                elements: vec![css_nav_element(
                    "div",
                    "primary",
                    verter_span::Span::new(attr, attr + 15),
                    verter_span::Span::new(attr - 7, attr + 17),
                    None,
                    0,
                )],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![verter_semantic::analysis::build_css_style_analysis(
            style_css,
            verter_semantic::analysis::VueStyleInput::default(),
            true,
            false,
            None,
            scs,
        )])
        .into(),
        ..Default::default()
    };

    let cursor = source.find("class=\"primary\"").unwrap() + 8;
    let position = line_index.offset_to_position(cursor as u32).unwrap();
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    );
    assert!(
        result.is_none(),
        "a class token without a rule must produce NO definition (not the binding)"
    );
}

/// `:deep(.inner)` inner classes are reachable targets from the template.
#[test]
fn css_class_definition_reaches_deep_inner_class() {
    let source = "<template>\n  <div class=\"inner\"></div>\n</template>\n<style scoped>\n.wrap :deep(.inner) { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (scs, sce) = style_block.content_range();
    let style_css = &source[scs as usize..sce as usize];
    let attr = source.find("class=\"inner\"").unwrap() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (verter_semantic::analysis::template::TemplateAnalysisSnapshot {
                elements: vec![css_nav_element(
                    "div",
                    "inner",
                    verter_span::Span::new(attr, attr + 13),
                    verter_span::Span::new(attr - 7, attr + 15),
                    None,
                    0,
                )],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![verter_semantic::analysis::build_css_style_analysis(
            style_css,
            verter_semantic::analysis::VueStyleInput::default(),
            true,
            false,
            None,
            scs,
        )])
        .into(),
        ..Default::default()
    };

    let cursor = source.find("class=\"inner\"").unwrap() + 8;
    let position = line_index.offset_to_position(cursor as u32).unwrap();
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    )
    .expect(":deep inner class must be a reachable declaration target");

    let expected = source.find(":deep(.inner)").unwrap() + ":deep(.".len();
    let expected_pos = line_index.offset_to_position(expected as u32).unwrap();
    match result {
        GotoDefinitionResponse::Scalar(loc) => assert_eq!(loc.range.start, expected_pos),
        other => panic!("expected scalar, got {other:?}"),
    }
}

/// Nested SCSS classes are definition targets with exact spans.
#[test]
fn css_class_definition_reaches_nested_scss_class() {
    let source = "<template>\n  <div class=\"title\"></div>\n</template>\n<style lang=\"scss\" scoped>\n.card {\n  .title { color: red; }\n}\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (scs, sce) = style_block.content_range();
    let style_css = &source[scs as usize..sce as usize];
    let attr = source.find("class=\"title\"").unwrap() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (verter_semantic::analysis::template::TemplateAnalysisSnapshot {
                elements: vec![css_nav_element(
                    "div",
                    "title",
                    verter_span::Span::new(attr, attr + 13),
                    verter_span::Span::new(attr - 7, attr + 15),
                    None,
                    0,
                )],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![verter_semantic::analysis::build_scanned_style_analysis(
            verter_semantic::analysis::StyleAnalysisLang::Scss,
            style_css,
            verter_semantic::analysis::VueStyleInput::default(),
            true,
            false,
            None,
            scs,
        )])
        .into(),
        ..Default::default()
    };

    let cursor = source.find("class=\"title\"").unwrap() + 8;
    let position = line_index.offset_to_position(cursor as u32).unwrap();
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    )
    .expect("nested scss class must be a definition target");

    let expected = source.find(".title { color: red;").unwrap() + 1;
    let expected_pos = line_index.offset_to_position(expected as u32).unwrap();
    match result {
        GotoDefinitionResponse::Scalar(loc) => assert_eq!(loc.range.start, expected_pos),
        other => panic!("expected scalar, got {other:?}"),
    }
}

/// A kebab-case class token navigates even with the caret ON the hyphen
/// (no identifier word at that position — the positional path must serve it).
#[test]
fn css_class_definition_kebab_token_at_hyphen_position() {
    let source = "<template>\n  <div class=\"my-card\"></div>\n</template>\n<style scoped>\n.my-card { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (scs, sce) = style_block.content_range();
    let style_css = &source[scs as usize..sce as usize];
    let attr = source.find("class=\"my-card\"").unwrap() as u32;

    let analysis = FileAnalysisSnapshot {
        template: Some(
            (verter_semantic::analysis::template::TemplateAnalysisSnapshot {
                elements: vec![css_nav_element(
                    "div",
                    "my-card",
                    verter_span::Span::new(attr, attr + 15),
                    verter_span::Span::new(attr - 7, attr + 17),
                    None,
                    0,
                )],
                ..Default::default()
            })
            .into(),
        ),
        styles: (vec![verter_semantic::analysis::build_css_style_analysis(
            style_css,
            verter_semantic::analysis::VueStyleInput::default(),
            true,
            false,
            None,
            scs,
        )])
        .into(),
        ..Default::default()
    };

    // Caret exactly on the hyphen of my-card.
    let cursor = source.find("class=\"my-card\"").unwrap() + 7 + "my".len();
    let position = line_index.offset_to_position(cursor as u32).unwrap();
    let result = definition_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
    )
    .expect("kebab class token must navigate from the hyphen position");

    let expected = source.find(".my-card {").unwrap() + 1;
    let expected_pos = line_index.offset_to_position(expected as u32).unwrap();
    match result {
        GotoDefinitionResponse::Scalar(loc) => assert_eq!(loc.range.start, expected_pos),
        other => panic!("expected scalar, got {other:?}"),
    }
}
