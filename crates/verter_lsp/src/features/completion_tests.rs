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
fn test_template_completions_include_bindings() {
    let source =
        "<template>\n  {{ | }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
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

    // Position inside template
    let position = Position {
        line: 1,
        character: 5,
    };
    let result = completions_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    assert!(result.is_some());
    let items = result.unwrap().items;
    assert!(items.iter().any(|i| i.label == "count"));
}

#[test]
fn test_script_completions_include_imports() {
    let source = "<script setup>\nimport { ref } from 'vue'\n\n</script>\n";
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

    // Position inside script (line 2)
    let position = Position {
        line: 2,
        character: 0,
    };
    let result = completions_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    assert!(result.is_some());
    let items = result.unwrap().items;
    assert!(items.iter().any(|i| i.label == "ref"));
}

#[test]
fn test_filters_internal_symbols() {
    // Use a source with actual content so the cursor is inside the block, not on the closing tag
    let source = "<script setup>\n\n</script>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(
        vec![AnalyzedBinding {
            name: "___VERTER___internal".to_string(),
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

    // Position on the empty line (line 1) which is block content
    let position = Position {
        line: 1,
        character: 0,
    };
    let result = completions_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    assert!(result.is_some());
    assert!(result.unwrap().items.is_empty());
}

#[test]
fn test_style_returns_css_completions() {
    let source = "<style>\n.foo {}\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis(vec![], vec![], vec![]);

    let position = Position {
        line: 1,
        character: 5,
    };
    let result = completions_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    // Style blocks now delegate to CSS completions
    if let Some(cr) = result {
        assert!(cr
            .items
            .iter()
            .any(|i| i.label == "color" || i.label == "display"));
    }
}

#[test]
fn test_template_excludes_type_only_imports() {
    let source = "<template>\n  <div/>\n</template>\n\n<script setup>\nimport type { Props } from './types'\n</script>\n";
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
        vec![],
    );

    let position = Position {
        line: 1,
        character: 3,
    };
    let result = completions_at_position(
        &position,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    assert!(result.is_some());
    // Type-only imports should not appear in template completions
    assert!(!result.unwrap().items.iter().any(|i| i.label == "Props"));
}

// =========================================================================
// CSS Class Completion Tests (A1)
// =========================================================================

#[test]
fn test_class_completions_in_static_class() {
    let source = "<template><div class=\"fo\"></div></template>\n<style scoped>\n.foo { color: red; }\n.bar { color: blue; }\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let css = build_style(source, &blocks);

    let analysis = FileAnalysisSnapshot {
        styles: vec![css],
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            elements: vec![make_element_for_completion("div", &["fo"], None, source)],
            ..Default::default()
        }),
        ..Default::default()
    };

    // Cursor inside class="fo|"
    let cursor = source.find("fo\"").unwrap() + 2; // after "fo"
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    assert!(result.is_some());
    let cr = result.unwrap();
    assert!(
        cr.is_incomplete,
        "should set is_incomplete for class completions"
    );
    assert!(
        cr.items.iter().any(|i| i.label == "foo"),
        "should offer .foo"
    );
    assert!(
        cr.items.iter().any(|i| i.label == "bar"),
        "should offer .bar"
    );
    // Verify sort_text prefix
    assert!(cr
        .items
        .iter()
        .all(|i| i.sort_text.as_ref().unwrap().starts_with('z')));
}

#[test]
fn test_no_class_completions_outside_class_attr() {
    let source = "<template><div id=\"app\"></div></template>\n<style scoped>\n.foo {}\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let css = build_style(source, &blocks);

    let analysis = FileAnalysisSnapshot {
        styles: vec![css],
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            elements: vec![make_element_for_completion("div", &[], Some("app"), source)],
            ..Default::default()
        }),
        ..Default::default()
    };

    let cursor = source.find("app").unwrap() + 1;
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    // Should NOT be class completions (id attribute), so is_incomplete should be false
    if let Some(cr) = result {
        assert!(!cr.is_incomplete || cr.items.is_empty());
    }
}

#[test]
fn test_class_completions_no_style_block() {
    let source = "<template><div class=\"foo\"></div></template>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = FileAnalysisSnapshot {
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            elements: vec![make_element_for_completion("div", &["foo"], None, source)],
            ..Default::default()
        }),
        ..Default::default()
    };

    let cursor = source.find("foo").unwrap() + 1;
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    // No styles = no CSS classes to offer. Should still return a result but with empty items.
    if let Some(cr) = result {
        assert!(
            cr.items.is_empty(),
            "no CSS classes to offer without style block"
        );
    }
}

fn make_element_for_completion(
    tag: &str,
    classes: &[&str],
    id: Option<&str>,
    source: &str,
) -> verter_analysis::TemplateElement {
    // Find the element's span in source for accurate positioning
    let tag_pattern = format!("<{}", tag);
    let span_start = source.find(&tag_pattern).unwrap_or(0) as u32;
    let span_end = source[span_start as usize..]
        .find('>')
        .map(|i| span_start + i as u32 + 1)
        .unwrap_or(span_start + 10);

    let mut attrs = Vec::new();
    if !classes.is_empty() {
        let class_val = classes.join(" ");
        let class_pattern = format!("class=\"{}\"", class_val);
        let attr_start = source.find(&class_pattern).unwrap_or(0) as u32;
        let attr_end = attr_start + class_pattern.len() as u32;
        attrs.push(verter_analysis::TemplateAttribute {
            name: "class".into(),
            value: Some(class_val),
            is_dynamic: false,
            span: verter_span::Span::new(attr_start, attr_end),
            name_end: 0,
            value_span: None,
        });
    }
    if let Some(id_val) = id {
        let id_pattern = format!("id=\"{}\"", id_val);
        let attr_start = source.find(&id_pattern).unwrap_or(0) as u32;
        let attr_end = attr_start + id_pattern.len() as u32;
        attrs.push(verter_analysis::TemplateAttribute {
            name: "id".into(),
            value: Some(id_val.into()),
            is_dynamic: false,
            span: verter_span::Span::new(attr_start, attr_end),
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
        span: verter_span::Span::new(span_start, span_end),
        tag_span_end: span_end,
        content_end: 0,
        ..Default::default()
    }
}

#[test]
fn test_class_completions_in_dynamic_class() {
    let source = "<template><div :class=\"{ 'btn': active }\"></div></template>\n<style scoped>\n.btn { color: red; }\n.active { display: block; }\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let css = build_style(source, &blocks);

    let mut el = make_element_for_completion("div", &[], None, source);
    // Add the dynamic :class attribute
    let class_pattern = ":class=\"{ 'btn': active }\"";
    let attr_start = source.find(class_pattern).unwrap_or(0) as u32;
    let attr_end = attr_start + class_pattern.len() as u32;
    el.attributes.push(verter_analysis::TemplateAttribute {
        name: "class".into(),
        value: Some("{ 'btn': active }".into()),
        is_dynamic: true,
        span: verter_span::Span::new(attr_start, attr_end),
        name_end: 0,
        value_span: None,
    });

    let analysis = FileAnalysisSnapshot {
        styles: vec![css],
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        }),
        ..Default::default()
    };

    // Position cursor inside the 'btn' string in :class="{ 'btn': active }"
    let btn_offset = source.find("'btn'").unwrap() + 2; // inside the string
    let pos = line_index.offset_to_position(btn_offset as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    assert!(
        result.is_some(),
        "should offer completions in dynamic :class"
    );
    let cr = result.unwrap();
    assert!(
        cr.items.iter().any(|i| i.label == "btn"),
        "should include 'btn' class"
    );
    assert!(
        cr.items.iter().any(|i| i.label == "active"),
        "should include 'active' class"
    );
    assert!(
        cr.is_incomplete,
        "should be is_incomplete for live filtering"
    );
}

#[test]
fn test_no_class_completions_outside_dynamic_string() {
    let source = "<template><div :class=\"{ btn: active }\"></div></template>\n<style scoped>\n.btn { color: red; }\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let css = build_style(source, &blocks);

    let mut el = make_element_for_completion("div", &[], None, source);
    let class_pattern = ":class=\"{ btn: active }\"";
    let attr_start = source.find(class_pattern).unwrap_or(0) as u32;
    let attr_end = attr_start + class_pattern.len() as u32;
    el.attributes.push(verter_analysis::TemplateAttribute {
        name: "class".into(),
        value: Some("{ btn: active }".into()),
        is_dynamic: true,
        span: verter_span::Span::new(attr_start, attr_end),
        name_end: 0,
        value_span: None,
    });

    let analysis = FileAnalysisSnapshot {
        styles: vec![css],
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        }),
        ..Default::default()
    };

    // Position cursor on the unquoted identifier 'btn' — not inside a string
    let btn_offset = source.find("btn:").unwrap() + 1;
    let pos = line_index.offset_to_position(btn_offset as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );
    // Should NOT offer CSS class completions when cursor is not inside a string
    if let Some(cr) = result {
        let has_css_class = cr
            .items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::VALUE));
        assert!(
            !has_css_class,
            "should not offer CSS class completions outside inner string"
        );
    }
}

// =========================================================================
// Event Modifier Completion Tests (#14)
// =========================================================================

#[test]
fn test_event_modifier_completions_click() {
    let source = "<template><div @click.></div></template>\n<script setup>\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let analysis = make_analysis(vec![], vec![], vec![]);

    // Position after the dot in @click.
    let dot_pos = source.find("@click.").unwrap() + 7;
    let pos = line_index.offset_to_position(dot_pos as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(result.is_some(), "should return completions after @click.");
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "stop"),
        "should include 'stop' modifier"
    );
    assert!(
        items.iter().any(|i| i.label == "prevent"),
        "should include 'prevent' modifier"
    );
    assert!(
        items.iter().any(|i| i.label == "once"),
        "should include 'once' modifier"
    );
    assert!(
        items.iter().any(|i| i.label == "capture"),
        "should include 'capture' modifier"
    );
    // Click is not a keyboard event — should NOT include key modifiers
    assert!(
        !items.iter().any(|i| i.label == "enter"),
        "click should not include key modifier 'enter'"
    );
}

#[test]
fn test_event_modifier_completions_keyup() {
    let source = "<template><input @keyup.></template>\n<script setup>\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let analysis = make_analysis(vec![], vec![], vec![]);

    let dot_pos = source.find("@keyup.").unwrap() + 7;
    let pos = line_index.offset_to_position(dot_pos as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(result.is_some(), "should return completions after @keyup.");
    let items = result.unwrap().items;
    // Should include runtime modifiers
    assert!(items.iter().any(|i| i.label == "stop"));
    assert!(items.iter().any(|i| i.label == "prevent"));
    // Should also include key modifiers for keyboard events
    assert!(
        items.iter().any(|i| i.label == "enter"),
        "keyup should include 'enter'"
    );
    assert!(
        items.iter().any(|i| i.label == "esc"),
        "keyup should include 'esc'"
    );
    assert!(
        items.iter().any(|i| i.label == "tab"),
        "keyup should include 'tab'"
    );
}

#[test]
fn test_event_modifier_completions_mouse() {
    let source = "<template><div @mousedown.></div></template>\n<script setup>\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let analysis = make_analysis(vec![], vec![], vec![]);

    let dot_pos = source.find("@mousedown.").unwrap() + 11;
    let pos = line_index.offset_to_position(dot_pos as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(result.is_some());
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "left"),
        "mouse event should include 'left'"
    );
    assert!(
        items.iter().any(|i| i.label == "right"),
        "mouse event should include 'right'"
    );
    assert!(
        items.iter().any(|i| i.label == "middle"),
        "mouse event should include 'middle'"
    );
}

#[test]
fn test_no_event_modifier_in_text() {
    let source = "<template><div>text.</div></template>\n<script setup>\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let analysis = make_analysis(vec![], vec![], vec![]);

    let dot_pos = source.find("text.").unwrap() + 5;
    let pos = line_index.offset_to_position(dot_pos as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    // Should not return event modifier completions for regular text
    if let Some(cr) = result {
        assert!(
            !cr.items.iter().any(|i| i.label == "stop"),
            "should not offer event modifiers in regular text"
        );
    }
}

#[test]
fn test_event_modifier_completions_chained() {
    let source = "<template><div @click.stop.></div></template>\n<script setup>\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let analysis = make_analysis(vec![], vec![], vec![]);

    // Position after the second dot in @click.stop.
    let second_dot = source.find(".stop.").unwrap() + 6;
    let pos = line_index.offset_to_position(second_dot as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(
        result.is_some(),
        "should return completions for chained modifiers"
    );
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "prevent"),
        "should offer 'prevent' as chained modifier"
    );
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

// ========================================================================
// SFC root completions (A3)
// ========================================================================

#[test]
fn test_root_completions_empty_file() {
    let source = "";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let pos = Position {
        line: 0,
        character: 0,
    };
    let result =
        completions_at_position(&pos, source, &blocks, None, &line_index, None, None, None);
    assert!(result.is_some(), "should provide completions at root level");
    let items = result.unwrap().items;

    // Should have scaffold snippets since file is empty
    assert!(
        items.iter().any(|i| i.label == "vue-ts"),
        "should have vue-ts scaffold: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|i| i.label == "vue"),
        "should have vue scaffold"
    );
    assert!(
        items.iter().any(|i| i.label == "template"),
        "should have template snippet"
    );
    assert!(
        items.iter().any(|i| i.label == "script setup"),
        "should have script setup snippet"
    );
}

#[test]
fn test_root_completions_with_existing_blocks() {
    let source = "<template>\n  <div/>\n</template>\n\n";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Position after </template> — at root level
    let pos = line_index
        .offset_to_position(blocks[0].close_tag_end)
        .unwrap();
    let result =
        completions_at_position(&pos, source, &blocks, None, &line_index, None, None, None);
    assert!(result.is_some());
    let items = result.unwrap().items;

    // Template already exists — should NOT offer template snippet
    assert!(
        !items.iter().any(|i| i.label == "template"),
        "should not offer template when already exists"
    );
    // Should still offer script
    assert!(
        items.iter().any(|i| i.label == "script setup"),
        "should offer script setup: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    // Should not have scaffold snippets (file is not empty)
    assert!(
        !items.iter().any(|i| i.label == "vue-ts"),
        "should not have scaffolds when file has content"
    );
}

#[test]
fn test_attribute_completions_script() {
    let source = "<script >\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Position inside opening tag (on the space after "script")
    let pos = line_index.offset_to_position(8).unwrap(); // after "<script " before ">"
    let result =
        completions_at_position(&pos, source, &blocks, None, &line_index, None, None, None);
    assert!(result.is_some());
    let items = result.unwrap().items;

    assert!(
        items.iter().any(|i| i.label == "setup"),
        "should offer setup: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(items.iter().any(|i| i.label == "lang"), "should offer lang");
    assert!(
        items.iter().any(|i| i.label == "attrs"),
        "should offer attrs"
    );
}

#[test]
fn test_attribute_completions_script_existing_attrs_filtered() {
    let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Position inside opening tag
    let pos = line_index.offset_to_position(22).unwrap(); // before ">"
    let result =
        completions_at_position(&pos, source, &blocks, None, &line_index, None, None, None);
    assert!(result.is_some());
    let items = result.unwrap().items;

    // setup and lang already exist — should NOT be offered
    assert!(
        !items.iter().any(|i| i.label == "setup"),
        "should not offer setup when already present"
    );
    assert!(
        !items.iter().any(|i| i.label == "lang"),
        "should not offer lang when already present"
    );
    // generic should be offered when setup is present
    assert!(
        items.iter().any(|i| i.label == "generic"),
        "should offer generic when setup is present: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_attribute_completions_style() {
    let source = "<style >\n.foo {}\n</style>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let pos = line_index.offset_to_position(7).unwrap(); // space before ">"
    let result =
        completions_at_position(&pos, source, &blocks, None, &line_index, None, None, None);
    assert!(result.is_some());
    let items = result.unwrap().items;

    assert!(
        items.iter().any(|i| i.label == "scoped"),
        "should offer scoped: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|i| i.label == "module"),
        "should offer module"
    );
    assert!(items.iter().any(|i| i.label == "lang"), "should offer lang");
}

#[test]
fn test_no_completions_on_closing_tag() {
    let source = "<script setup>\nconst x = 1;\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let pos = line_index
        .offset_to_position(blocks[0].close_tag_start + 2)
        .unwrap();
    let result =
        completions_at_position(&pos, source, &blocks, None, &line_index, None, None, None);
    assert!(
        result.is_none(),
        "should not offer completions on closing tag"
    );
}

// =========================================================================
// Template Cursor Context Tests (TDD — completion context filtering)
// =========================================================================

/// Helper to build analysis with a binding and template component list.
fn make_analysis_with_template(
    bindings: Vec<AnalyzedBinding>,
    components: Vec<verter_analysis::template::TemplateComponentUsage>,
) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        bindings,
        template: Some(verter_analysis::TemplateAnalysisSnapshot {
            components,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn test_tag_name_no_script_bindings() {
    // Cursor after `<` in tag name position — should NOT include script bindings like `count`
    let source = "<template>\n  <\n</template>\n<script setup>\nconst count = ref(0)\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis_with_template(
        vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(0, 0),
        }],
        vec![],
    );

    // Position right after `<` on line 1
    let cursor = source.find("  <\n").unwrap() + 3; // right after `<`
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    // Should return completions (tag names) but NOT include `count`
    if let Some(cr) = result {
        assert!(
            !cr.items.iter().any(|i| i.label == "count"),
            "tag name position should NOT include script binding 'count', got: {:?}",
            cr.items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
}

#[test]
fn test_tag_name_includes_html_elements() {
    // Cursor after `<` — should include HTML element names
    let source = "<template>\n  <\n</template>\n<script setup>\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis_with_template(vec![], vec![]);

    let cursor = source.find("  <\n").unwrap() + 3;
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(result.is_some(), "should return completions for tag names");
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "div"),
        "should include 'div': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|i| i.label == "span"),
        "should include 'span'"
    );
    assert!(
        items.iter().any(|i| i.label == "button"),
        "should include 'button'"
    );
}

#[test]
fn test_tag_name_includes_components() {
    // Cursor after `<` — should include imported components
    let source = "<template>\n  <\n</template>\n<script setup>\nimport MyComp from './MyComp.vue'\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis_with_template(
        vec![],
        vec![verter_analysis::template::TemplateComponentUsage {
            name: "MyComp".to_string(),
            import_source: Some("./MyComp.vue".to_string()),
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
    );

    let cursor = source.find("  <\n").unwrap() + 3;
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(result.is_some(), "should return completions for tag names");
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "MyComp"),
        "should include component 'MyComp': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_tag_name_includes_vue_builtins() {
    let source = "<template>\n  <\n</template>\n<script setup>\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis_with_template(vec![], vec![]);

    let cursor = source.find("  <\n").unwrap() + 3;
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(result.is_some());
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "Transition"),
        "should include Vue built-in 'Transition': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|i| i.label == "KeepAlive"),
        "should include 'KeepAlive'"
    );
    assert!(
        items.iter().any(|i| i.label == "Teleport"),
        "should include 'Teleport'"
    );
    assert!(
        items.iter().any(|i| i.label == "Suspense"),
        "should include 'Suspense'"
    );
    assert!(
        items.iter().any(|i| i.label == "slot"),
        "should include 'slot'"
    );
    assert!(
        items.iter().any(|i| i.label == "template"),
        "should include 'template'"
    );
}

#[test]
fn test_attr_name_no_script_bindings() {
    // Cursor in attribute position `<div |>` — should NOT include `count`
    let source =
        "<template>\n  <div >\n</template>\n<script setup>\nconst count = ref(0)\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis_with_template(
        vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(0, 0),
        }],
        vec![],
    );

    // Position on the space between `div` and `>`
    let cursor = source.find("<div >").unwrap() + 5; // space before >
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    if let Some(cr) = result {
        assert!(
            !cr.items.iter().any(|i| i.label == "count"),
            "attribute name position should NOT include script binding 'count', got: {:?}",
            cr.items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
}

#[test]
fn test_attr_name_includes_directives() {
    // Cursor in attribute position `<div |>` — should include Vue directives
    let source = "<template>\n  <div >\n</template>\n<script setup>\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis_with_template(vec![], vec![]);

    let cursor = source.find("<div >").unwrap() + 5;
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(
        result.is_some(),
        "should return completions for attribute names"
    );
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "v-if"),
        "should include 'v-if': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|i| i.label == "v-for"),
        "should include 'v-for'"
    );
    assert!(
        items.iter().any(|i| i.label == "v-model"),
        "should include 'v-model'"
    );
    assert!(
        items.iter().any(|i| i.label == "@click"),
        "should include '@click'"
    );
    // Negative: should NOT include tag names
    assert!(
        !items.iter().any(|i| i.label == "div"),
        "should NOT include HTML element 'div' in attribute position"
    );
}

#[test]
fn test_text_content_no_bindings() {
    // Cursor in text content `<div>text|</div>` — should NOT offer bindings as completions
    let source =
        "<template>\n  <div>some text</div>\n</template>\n<script setup>\nconst count = ref(0)\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis_with_template(
        vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(0, 0),
        }],
        vec![],
    );

    let cursor = source.find("some text").unwrap() + 4; // inside text
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    // Text content should return None (no completions)
    if let Some(cr) = result {
        assert!(
            !cr.items.iter().any(|i| i.label == "count"),
            "text content position should NOT include 'count', got: {:?}",
            cr.items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
}

#[test]
fn test_mustache_shows_bindings() {
    // Cursor inside mustache `{{ | }}` — should include `count` (already works, regression guard)
    let source =
        "<template>\n  {{ }}\n</template>\n<script setup>\nconst count = ref(0)\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis_with_template(
        vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(0, 0),
        }],
        vec![],
    );

    let cursor = source.find("{{ }}").unwrap() + 3; // inside {{ }}
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(result.is_some(), "should return completions in mustache");
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "count"),
        "mustache should include 'count'"
    );
}

#[test]
fn test_attr_value_shows_bindings() {
    // Cursor inside attribute value `:prop="|"` — should include `count`
    let source = "<template>\n  <div :foo=\"\"></div>\n</template>\n<script setup>\nconst count = ref(0)\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    let analysis = make_analysis_with_template(
        vec![AnalyzedBinding {
            name: "count".to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(0, 0),
        }],
        vec![],
    );

    let cursor = source.find(":foo=\"\"").unwrap() + 6; // between the quotes
    let pos = line_index.offset_to_position(cursor as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(result.is_some(), "should return completions in attr value");
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "count"),
        "attribute value should include 'count'"
    );
}

// =========================================================================
// v-model modifier completions
// =========================================================================

#[test]
fn test_vmodel_modifier_completions() {
    let source = "<template><input v-model.></template>\n<script setup>\n</script>";
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let analysis = make_analysis(vec![], vec![], vec![]);

    let dot_pos = source.find("v-model.").unwrap() + 8;
    let pos = line_index.offset_to_position(dot_pos as u32).unwrap();
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        Some(&analysis),
        &line_index,
        None,
        None,
        None,
    );

    assert!(result.is_some(), "should return completions after v-model.");
    let items = result.unwrap().items;
    assert!(
        items.iter().any(|i| i.label == "lazy"),
        "v-model should include 'lazy': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|i| i.label == "number"),
        "v-model should include 'number'"
    );
    assert!(
        items.iter().any(|i| i.label == "trim"),
        "v-model should include 'trim'"
    );
    // Negative: should NOT include event modifiers
    assert!(
        !items.iter().any(|i| i.label == "stop"),
        "v-model should NOT include 'stop'"
    );
}

// ── Suppress completions inside generic/attrs attribute values ───────────────

#[test]
fn test_no_completions_inside_generic_attr_value() {
    let source = r#"<script setup lang="ts" generic="T extends string">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Position inside generic="T extends |string" (line 0, character 43)
    let generic_value_pos = source.find("T extends string").unwrap();
    let col = generic_value_pos; // All on line 0
    let position = Position {
        line: 0,
        character: col as u32 + 10, // Inside the value
    };
    let result = completions_at_position(
        &position,
        source,
        &blocks,
        None,
        &line_index,
        None,
        None,
        None,
    );

    // Positive: should return None (delegate to TypeProvider)
    assert!(
        result.is_none(),
        "should return None for cursor inside generic attribute value, got: {:?}",
        result.map(|r| r.items.len())
    );
}

#[test]
fn test_no_completions_inside_attrs_attr_value() {
    let source = r#"<script setup lang="ts" attrs="{ class: string }">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Position inside attrs="{ class: |string }" (line 0)
    let attrs_value_pos = source.find("{ class: string }").unwrap();
    let col = attrs_value_pos;
    let position = Position {
        line: 0,
        character: col as u32 + 5, // Inside the value
    };
    let result = completions_at_position(
        &position,
        source,
        &blocks,
        None,
        &line_index,
        None,
        None,
        None,
    );

    // Positive: should return None (delegate to TypeProvider)
    assert!(
        result.is_none(),
        "should return None for cursor inside attrs attribute value, got: {:?}",
        result.map(|r| r.items.len())
    );
}

#[test]
fn test_normal_script_attr_completions_outside_ts_values() {
    let source = r#"<script setup lang="ts" generic="T" >
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#;
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);

    // Position on the opening tag but outside any attribute value (after generic="T" )
    // The space before > at line 0
    let pos = source.find(" >").unwrap();
    let position = Position {
        line: 0,
        character: pos as u32,
    };
    let result = completions_at_position(
        &position,
        source,
        &blocks,
        None,
        &line_index,
        None,
        None,
        None,
    );

    // Positive: should return completions (not suppressed)
    assert!(
        result.is_some(),
        "should return attribute completions outside TS attr values"
    );
}
