use super::*;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use verter_analysis::template;
use verter_analysis::*;

fn make_analysis(
    bindings: Vec<AnalyzedBinding>,
    imports: Vec<AnalyzedImport>,
) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        bindings,
        imports,
        ..Default::default()
    }
}

#[test]
fn test_rename_binding_across_blocks() {
    let source = "<template>\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // AnalyzedBinding spans are script-content-relative (OXC offset 0 = script content start)
    let script_block = blocks.iter().find(|b| b.tag_name == "script").unwrap();
    let content_start = script_block.content_range().0;
    let count_decl_sfc = source.rfind("count").unwrap() as u32;
    let count_decl_relative = count_decl_sfc - content_start;
    let template_count = source.find("count").unwrap();

    let analysis = FileAnalysisSnapshot {
        bindings: vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(count_decl_relative, count_decl_relative + 5),
            used_in_script: false,
            used_in_style: false,
        }],
        template: Some(template::TemplateAnalysisSnapshot {
            binding_occurrences: vec![template::TemplateBindingOccurrence {
                name: "count".to_string(),
                span: verter_span::Span::new(template_count as u32, template_count as u32 + 5),
                usage_kind: template::BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let position = line_index
        .offset_to_position(template_count as u32)
        .unwrap();

    let edit = rename_at_position(
        &position,
        "counter",
        source,
        &blocks,
        Some(&analysis),
        &line_index,
    );
    assert!(edit.is_some());

    let edit = edit.unwrap();
    let changes = edit.changes.unwrap();
    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    let edits = changes.get(&uri).unwrap();

    // Declaration + template usage = at least 2 edits
    assert!(edits.len() >= 2, "expected >=2 edits, got {}", edits.len());
    assert!(edits.iter().all(|e| e.new_text == "counter"));
}

#[test]
fn test_prepare_rename_returns_range() {
    let source = "<script setup>\nconst count = ref(0)\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let count_offset = source.find("count").unwrap() as u32;
    // prepare_rename uses word_at_offset (SFC-level), so span values don't affect it.
    // But keep consistent: use script-relative spans (OXC convention).
    let script_block = blocks.iter().find(|b| b.tag_name == "script").unwrap();
    let content_start = script_block.content_range().0;
    let count_relative = count_offset - content_start;

    let analysis = make_analysis(
        vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(count_relative, count_relative + 5),
            used_in_script: false,
            used_in_style: false,
        }],
        vec![],
    );

    let position = line_index.offset_to_position(count_offset).unwrap();

    let range = prepare_rename(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(range.is_some());
    let range = range.unwrap();
    assert_eq!(range.start, position);
}

#[test]
fn test_cannot_rename_unknown_word() {
    let source = "<script setup>\nconst x = 1\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(vec![], vec![]);

    let offset = source.find("const").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let range = prepare_rename(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(range.is_none());
}

// =========================================================================
// CSS Class/ID Rename Tests (A3)
// =========================================================================

#[test]
fn test_prepare_rename_css_class_in_template() {
    let source = "<template><div class=\"btn\"></div></template>\n<style scoped>\n.btn { color: red; }\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let css = build_style(source, &blocks);

    let btn_offset = source.find("btn\"").unwrap(); // "btn" in class="btn"
    let el = make_element_with_attrs(source, "div", &["btn"], None);

    let analysis = FileAnalysisSnapshot {
        styles: vec![css],
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        }),
        ..Default::default()
    };

    let pos = line_index.offset_to_position(btn_offset as u32).unwrap();
    let range = prepare_rename(&pos, source, &blocks, Some(&analysis), &line_index);
    assert!(range.is_some(), "should allow renaming CSS class");
}

#[test]
fn test_rename_css_class_across_template_and_style() {
    let source = "<template><div class=\"btn\"></div></template>\n<style scoped>\n.btn { color: red; }\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let css = build_style(source, &blocks);

    let btn_offset = source.find("btn\"").unwrap();
    let el = make_element_with_attrs(source, "div", &["btn"], None);

    let analysis = FileAnalysisSnapshot {
        styles: vec![css],
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        }),
        ..Default::default()
    };

    let pos = line_index.offset_to_position(btn_offset as u32).unwrap();
    let edit = rename_at_position(
        &pos,
        "button",
        source,
        &blocks,
        Some(&analysis),
        &line_index,
    );
    assert!(edit.is_some());
    let edit = edit.unwrap();
    let changes = edit.changes.unwrap();
    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    let edits = changes.get(&uri).unwrap();
    // Should have at least 2 edits: template class + style selector
    assert!(edits.len() >= 2, "expected >=2 edits, got {}", edits.len());
    assert!(edits.iter().all(|e| e.new_text == "button"));
}

fn make_element_with_attrs(
    source: &str,
    tag: &str,
    classes: &[&str],
    id: Option<&str>,
) -> verter_analysis::TemplateElement {
    let mut attrs = Vec::new();
    if !classes.is_empty() {
        let class_val = classes.join(" ");
        let pattern = format!("class=\"{}\"", class_val);
        let start = source.find(&pattern).unwrap_or(0) as u32;
        let end = start + pattern.len() as u32;
        attrs.push(verter_analysis::TemplateAttribute {
            name: "class".into(),
            value: Some(class_val),
            is_dynamic: false,
            span: verter_span::Span::new(start, end),
            name_end: 0,
            value_span: None,
        });
    }
    if let Some(id_val) = id {
        let pattern = format!("id=\"{}\"", id_val);
        let start = source.find(&pattern).unwrap_or(0) as u32;
        let end = start + pattern.len() as u32;
        attrs.push(verter_analysis::TemplateAttribute {
            name: "id".into(),
            value: Some(id_val.into()),
            is_dynamic: false,
            span: verter_span::Span::new(start, end),
            name_end: 0,
            value_span: None,
        });
    }
    verter_analysis::TemplateElement {
        tag: tag.into(),
        is_component: false,
        is_self_closing: false,
        namespace: verter_analysis::ElementNamespace::Html,
        attributes: attrs,
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
    }
}

fn build_style(source: &str, blocks: &[SfcBlock]) -> verter_analysis::StyleBlockAnalysis {
    let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
    let (content_start, content_end) = style_block.content_range();
    let css_content = &source[content_start as usize..content_end as usize];
    let scoped = style_block.attrs_raw.contains("scoped");
    verter_analysis::style::build_css_style_analysis(
        css_content,
        verter_analysis::style::VueStyleInput {
            v_binds: vec![],
            special_pseudos: vec![],
        },
        scoped,
        false,
        None,
        content_start,
    )
}

#[test]
fn test_rename_css_id_across_template_and_style() {
    let source = "<template><div id=\"app\"></div></template>\n<style scoped>\n#app { margin: 0; }\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let css = build_style(source, &blocks);

    let el = make_element_with_attrs(source, "div", &[], Some("app"));

    let analysis = FileAnalysisSnapshot {
        styles: vec![css],
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        }),
        ..Default::default()
    };

    let id_offset = source.find("app\"").unwrap();
    let pos = line_index.offset_to_position(id_offset as u32).unwrap();
    let edit = rename_at_position(&pos, "root", source, &blocks, Some(&analysis), &line_index);
    assert!(edit.is_some(), "should allow renaming CSS ID");
    let edit = edit.unwrap();
    let changes = edit.changes.unwrap();
    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    let edits = changes.get(&uri).unwrap();
    assert!(
        edits.len() >= 2,
        "should have edits in template and style, got {}",
        edits.len()
    );
    assert!(
        edits.iter().all(|e| e.new_text == "root"),
        "all edits should be new name"
    );
    // Negative: no edit should contain the old name
    assert!(
        !edits.iter().any(|e| e.new_text.contains("app")),
        "should not contain old name"
    );
}

#[test]
fn test_rename_css_class_doesnt_affect_other_names() {
    let source = "<template><div class=\"btn active\"></div></template>\n<style scoped>\n.btn { color: red; }\n.active { display: block; }\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let css = build_style(source, &blocks);

    let el = make_element_with_attrs(source, "div", &["btn", "active"], None);

    let analysis = FileAnalysisSnapshot {
        styles: vec![css],
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        }),
        ..Default::default()
    };

    let btn_offset = source.find("btn ").unwrap();
    let pos = line_index.offset_to_position(btn_offset as u32).unwrap();
    let edit = rename_at_position(
        &pos,
        "button",
        source,
        &blocks,
        Some(&analysis),
        &line_index,
    );
    assert!(edit.is_some());
    let edit = edit.unwrap();
    let changes = edit.changes.unwrap();
    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    let edits = changes.get(&uri).unwrap();
    // All rename edits should be "button", never "active"
    assert!(edits.iter().all(|e| e.new_text == "button"));
}

#[test]
fn test_cannot_rename_type_only_import() {
    let source = "<script setup>\nimport type { Props } from './types'\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![],
        vec![AnalyzedImport {
            source: "./types".to_string(),
            is_type_only: true,
            bindings: vec![AnalyzedImportBinding {
                name: "Props".to_string(),
                is_type_only: true,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        }],
    );

    let offset = source.find("Props").unwrap();
    let position = line_index.offset_to_position(offset as u32).unwrap();

    let range = prepare_rename(&position, source, &blocks, Some(&analysis), &line_index);
    assert!(range.is_none());
}

/// Template text containing the binding name as plain text (not an expression)
/// should NOT produce rename edits.
#[test]
fn test_span_based_rename_no_false_positives() {
    // "count" appears in plain text "count: " but only the interpolation {{ count }}
    // should be found via binding_occurrences, not plain text.
    let source = "<template>\n  <div>count: {{ count }}</div>\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // The interpolation {{ count }} — find the second "count" in template
    let first_count = source.find("count").unwrap(); // "count:" plain text
    let interp_count = source[first_count + 5..].find("count").unwrap() + first_count + 5;
    // Host analysis spans are SFC-absolute (not script-relative)
    let count_decl_sfc = source.rfind("count").unwrap() as u32;

    let analysis = FileAnalysisSnapshot {
        bindings: vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(count_decl_sfc, count_decl_sfc + 5),
            used_in_script: false,
            used_in_style: false,
        }],
        template: Some(template::TemplateAnalysisSnapshot {
            binding_occurrences: vec![template::TemplateBindingOccurrence {
                name: "count".to_string(),
                span: verter_span::Span::new(interp_count as u32, interp_count as u32 + 5),
                usage_kind: template::BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    let position = line_index.offset_to_position(interp_count as u32).unwrap();
    let edit = rename_at_position(
        &position,
        "counter",
        source,
        &blocks,
        Some(&analysis),
        &line_index,
    );
    assert!(edit.is_some());
    let edit = edit.unwrap();
    let changes = edit.changes.unwrap();
    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    let edits = changes.get(&uri).unwrap();

    // Should have exactly 2 edits: declaration + interpolation binding
    // (NOT the plain text "count:" which text search would have caught)
    assert_eq!(
        edits.len(),
        2,
        "should have 2 edits (declaration + interpolation), not the plain text. Got: {:?}",
        edits
    );
    // Verify the plain text "count:" offset is NOT in the edits
    let plain_text_offset = first_count as u32;
    assert!(
        !edits
            .iter()
            .any(|e| { line_index.position_to_offset(&e.range.start) == Some(plain_text_offset) }),
        "should NOT rename plain text 'count:'"
    );
}

#[test]
fn test_rename_with_dual_script_blocks() {
    let source = "<template>\n  {{ count }}\n</template>\n<script>\nexport default {}\n</script>\n<script setup>\nconst count = ref(0)\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let setup = blocks
        .iter()
        .find(|b| b.tag_name == "script" && b.is_setup())
        .unwrap();
    let (s_start, s_end) = setup.content_range();
    let setup_content = &source[s_start as usize..s_end as usize];

    // "count" in <script setup> content, stored as an SFC-absolute host span
    let count_in_setup = setup_content.find("count").unwrap() as u32;
    let count_sfc_offset = s_start + count_in_setup;

    let template_count = source.find("count").unwrap();

    let analysis = FileAnalysisSnapshot {
        bindings: vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(count_sfc_offset, count_sfc_offset + 5),
            used_in_script: false,
            used_in_style: false,
        }],
        template: Some(template::TemplateAnalysisSnapshot {
            binding_occurrences: vec![template::TemplateBindingOccurrence {
                name: "count".to_string(),
                span: verter_span::Span::new(template_count as u32, template_count as u32 + 5),
                usage_kind: template::BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    let position = line_index
        .offset_to_position(template_count as u32)
        .unwrap();

    let edit = rename_at_position(
        &position,
        "counter",
        source,
        &blocks,
        Some(&analysis),
        &line_index,
    );
    assert!(edit.is_some());

    let edit = edit.unwrap();
    let changes = edit.changes.unwrap();
    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    let edits = changes.get(&uri).unwrap();

    // Should include declaration + template + script usage edits
    assert!(edits.len() >= 2, "expected >=2 edits, got {}", edits.len());
    assert!(
        edits.iter().all(|e| e.new_text == "counter"),
        "all edits should be the new name"
    );

    // Verify the declaration edit points to "count" in <script setup>, not somewhere random
    assert!(
        edits
            .iter()
            .any(|e| { line_index.position_to_offset(&e.range.start) == Some(count_sfc_offset) }),
        "should have an edit at the declaration site in <script setup>"
    );
}
