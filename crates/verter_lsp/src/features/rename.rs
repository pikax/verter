// Phase 2: Rename — rename bindings across script/template blocks in a single file.
// Phase 3: Enhanced with cross-file rename from TypeProvider.

use std::collections::HashMap;

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
use crate::features::references::{
    collect_css_ref_spans, find_css_target_in_style_refs, find_css_target_in_template_refs,
    CssRefTarget,
};

/// Sentinel URI used when a rename edit is in the same file.
/// The server replaces this with the actual document URI before returning to the client.
pub const SAME_FILE_URI: &str = "verter-internal:same-file";

/// Check if the symbol at the given position can be renamed.
///
/// Returns a `Range` of the symbol if renaming is allowed, or `None` if not.
pub fn prepare_rename(
    position: &Position,
    source: &str,
    _blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Range> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;
    let word = word_at_offset(source, offset)?;

    // Only allow renaming known bindings and non-type imports
    let is_binding = analysis.bindings.iter().any(|b| b.name == word);
    let is_import = analysis
        .imports
        .iter()
        .any(|i| !i.is_type_only && i.bindings.iter().any(|b| b.name == word && !b.is_type_only));
    let is_macro = analysis
        .macros
        .iter()
        .any(|m| m.binding_name.as_ref().is_some_and(|n| n == &word));

    if !is_binding && !is_import && !is_macro {
        // Try CSS class/ID rename
        return prepare_rename_css(offset, source, analysis, line_index);
    }

    // Return the range of the word at the cursor
    let word_start = find_word_start(source.as_bytes(), offset);
    let word_end = word_start + word.len();

    let start = line_index.offset_to_position(word_start as u32)?;
    let end = line_index.offset_to_position(word_end as u32)?;
    Some(Range { start, end })
}

/// Check if a CSS class/ID name can be renamed.
fn prepare_rename_css(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<Range> {
    let target = if let Some(template) = &analysis.template {
        find_css_target_in_template_refs(offset, source, template)
    } else {
        None
    }
    .or_else(|| find_css_target_in_style_refs(offset, source, analysis))?;

    let name = match &target {
        CssRefTarget::Class(n) | CssRefTarget::Id(n) => n,
    };

    // Find the range of the CSS name at cursor — scan for its boundaries
    let spans = collect_css_ref_spans(&target, source, analysis);
    // Find the span that contains the cursor
    for (start, end) in &spans {
        if offset as u32 >= *start && (offset as u32) < *end {
            let s = line_index.offset_to_position(*start)?;
            let e = line_index.offset_to_position(*end)?;
            return Some(Range { start: s, end: e });
        }
    }

    // Fallback: use the name length
    let _ = name;
    None
}

/// Convert an analysis span offset (relative to the combined script content that the
/// host passes to OXC) to an SFC-absolute byte offset.
///
/// The host concatenates script block contents in order `[<script>, <script setup>]`
/// separated by `\n`. OXC spans are relative to this combined string. This function
/// determines which block the offset falls in and converts to an SFC-absolute offset.
fn analysis_span_to_sfc_offset(span_offset: u32, blocks: &[SfcBlock]) -> u32 {
    let script_blocks: Vec<&SfcBlock> = blocks.iter().filter(|b| b.tag_name == "script").collect();

    match script_blocks.len() {
        0 => span_offset,
        1 => script_blocks[0].content_range().0 + span_offset,
        _ => {
            // Dual block: host concatenates [normal, setup] with \n separator.
            let normal = script_blocks.iter().find(|b| !b.is_setup());
            let setup = script_blocks.iter().find(|b| b.is_setup());

            match (normal, setup) {
                (Some(n), Some(s)) => {
                    let (n_start, n_end) = n.content_range();
                    let normal_len = n_end - n_start;
                    if span_offset <= normal_len {
                        n_start + span_offset
                    } else {
                        // Skip past normal content + \n separator
                        s.content_range().0 + (span_offset - normal_len - 1)
                    }
                }
                (Some(n), None) => n.content_range().0 + span_offset,
                (None, Some(s)) => s.content_range().0 + span_offset,
                (None, None) => span_offset,
            }
        }
    }
}

/// Perform a rename of the symbol at the given position to `new_name`.
///
/// Finds all occurrences in script and template blocks and returns a
/// `WorkspaceEdit` with text edits for each occurrence.
///
/// ## Coordinate systems
///
/// Analysis spans (`AnalyzedBinding.span.start`, `AnalyzedImportBinding.span.start`)
/// are byte offsets relative to the combined script content that the host passes to
/// OXC for parsing. These must be converted to SFC-absolute offsets before calling
/// `span_to_edit()`. See `analysis_span_to_sfc_offset()` for the conversion logic.
///
/// Template binding occurrences (`TemplateBindingOccurrence.span.start`) are already
/// SFC-absolute and need no conversion.
pub fn rename_at_position(
    position: &Position,
    new_name: &str,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<WorkspaceEdit> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;
    let word = word_at_offset(source, offset)?;

    // Verify renaming is allowed
    let is_binding = analysis.bindings.iter().any(|b| b.name == word);
    let is_import = analysis
        .imports
        .iter()
        .any(|i| !i.is_type_only && i.bindings.iter().any(|b| b.name == word && !b.is_type_only));
    let is_macro = analysis
        .macros
        .iter()
        .any(|m| m.binding_name.as_ref().is_some_and(|n| n == &word));

    if !is_binding && !is_import && !is_macro {
        // Try CSS class/ID rename
        return rename_css(offset, new_name, source, analysis, line_index);
    }

    let mut edits: Vec<TextEdit> = Vec::new();

    // Collect declaration spans (convert script-relative → SFC-absolute)
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        if binding.span.start > 0 || binding.span.end > 0 {
            let abs_start = analysis_span_to_sfc_offset(binding.span.start, blocks);
            let abs_end = analysis_span_to_sfc_offset(binding.span.end, blocks);
            if let Some(edit) = span_to_edit(abs_start, abs_end, new_name, line_index) {
                edits.push(edit);
            }
        }
    }
    for import in &analysis.imports {
        for binding in &import.bindings {
            if binding.name == word && (binding.span.start > 0 || binding.span.end > 0) {
                let abs_start = analysis_span_to_sfc_offset(binding.span.start, blocks);
                let abs_end = analysis_span_to_sfc_offset(binding.span.end, blocks);
                if let Some(edit) = span_to_edit(abs_start, abs_end, new_name, line_index) {
                    edits.push(edit);
                }
            }
        }
    }

    // Use span-based template binding occurrences (precise, no false positives)
    if let Some(template) = &analysis.template {
        for occ in &template.binding_occurrences {
            if occ.name == word {
                // Skip if this overlaps a declaration span edit
                let already_covered = edits.iter().any(|e| {
                    let e_start = line_index.position_to_offset(&e.range.start);
                    e_start == Some(occ.span.start)
                });
                if !already_covered {
                    if let Some(edit) =
                        span_to_edit(occ.span.start, occ.span.end, new_name, line_index)
                    {
                        edits.push(edit);
                    }
                }
            }
        }
    }

    // For script blocks, use text search for usages (beyond declaration spans)
    for block in blocks {
        if block.tag_name != "script" {
            continue;
        }
        let (content_start, content_end) = block.content_range();
        let content = &source[content_start as usize..content_end as usize];

        for occ_offset in find_all_word_occurrences(content, &word) {
            let abs_offset = content_start as usize + occ_offset;
            let abs_end = abs_offset + word.len();

            // Skip if already covered by a declaration or template span
            let already_covered = edits.iter().any(|e| {
                let e_start = line_index.position_to_offset(&e.range.start);
                e_start == Some(abs_offset as u32)
            });
            if already_covered {
                continue;
            }

            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(abs_offset as u32),
                line_index.offset_to_position(abs_end as u32),
            ) {
                edits.push(TextEdit {
                    range: Range { start, end },
                    new_text: new_name.to_string(),
                });
            }
        }
    }

    if edits.is_empty() {
        return None;
    }

    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    #[allow(clippy::mutable_key_type)] // Uri has interior mutability but we only insert once
    let mut changes = HashMap::new();
    changes.insert(uri, edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

/// Perform a CSS class/ID rename across template and style blocks.
fn rename_css(
    offset: usize,
    new_name: &str,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<WorkspaceEdit> {
    let target = if let Some(template) = &analysis.template {
        find_css_target_in_template_refs(offset, source, template)
    } else {
        None
    }
    .or_else(|| find_css_target_in_style_refs(offset, source, analysis))?;

    let spans = collect_css_ref_spans(&target, source, analysis);
    if spans.is_empty() {
        return None;
    }

    let edits: Vec<TextEdit> = spans
        .into_iter()
        .filter_map(|(start, end)| span_to_edit(start, end, new_name, line_index))
        .collect();

    if edits.is_empty() {
        return None;
    }

    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    #[allow(clippy::mutable_key_type)]
    let mut changes = HashMap::new();
    changes.insert(uri, edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

fn span_to_edit(
    span_start: u32,
    span_end: u32,
    new_name: &str,
    line_index: &LineIndex,
) -> Option<TextEdit> {
    let start = line_index.offset_to_position(span_start)?;
    let end = line_index.offset_to_position(span_end)?;
    Some(TextEdit {
        range: Range { start, end },
        new_text: new_name.to_string(),
    })
}

use crate::utils::{find_all_word_occurrences, find_word_start, word_at_offset};

#[cfg(test)]
mod tests {
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

    /// @ai-generated - Prepare rename on class name in template returns range
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

    /// @ai-generated - Rename CSS class updates both template and style
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
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: verter_span::Span::new(0, 0),
            tag_span_end: 0,
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

    /// @ai-generated - Rename CSS ID updates both template and style
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

    /// @ai-generated - Rename CSS class doesn't affect other classes
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

    /// @ai-generated - Span-based rename avoids false positives from text search.
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
        // Use script-relative offset (OXC convention)
        let script_block = blocks.iter().find(|b| b.tag_name == "script").unwrap();
        let content_start = script_block.content_range().0;
        let count_decl_sfc = source.rfind("count").unwrap() as u32;
        let count_decl_relative = count_decl_sfc - content_start;

        let analysis = FileAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::Ref,
                type_annotation: None,
                initializer: None,
                span: verter_span::Span::new(count_decl_relative, count_decl_relative + 5),
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
            !edits.iter().any(|e| {
                line_index.position_to_offset(&e.range.start) == Some(plain_text_offset)
            }),
            "should NOT rename plain text 'count:'"
        );
    }

    /// @ai-generated - analysis_span_to_sfc_offset converts script-relative to SFC-absolute.
    #[test]
    fn test_analysis_span_offset_single_block() {
        let source = "<template><div></div></template>\n<script setup>\nconst x = 1\n</script>";
        let blocks = scan_sfc_blocks(source);
        let script = blocks.iter().find(|b| b.tag_name == "script").unwrap();
        let (content_start, content_end) = script.content_range();
        let content = &source[content_start as usize..content_end as usize];

        // Find "x" within the script content
        let x_in_content = content.find('x').unwrap() as u32;
        let abs = analysis_span_to_sfc_offset(x_in_content, &blocks);
        assert_eq!(abs, content_start + x_in_content);

        // Verify it points to "x" in the SFC source
        assert_eq!(&source[abs as usize..abs as usize + 1], "x");
    }

    /// @ai-generated - Dual script blocks: normal <script> + <script setup>
    #[test]
    fn test_analysis_span_offset_dual_blocks() {
        let source = "<script>\nexport default { name: 'App' }\n</script>\n<script setup>\nconst count = ref(0)\n</script>";
        let blocks = scan_sfc_blocks(source);

        let normal = blocks
            .iter()
            .find(|b| b.tag_name == "script" && !b.is_setup())
            .unwrap();
        let setup = blocks
            .iter()
            .find(|b| b.tag_name == "script" && b.is_setup())
            .unwrap();

        let (n_start, n_end) = normal.content_range();
        let normal_len = n_end - n_start;
        let (s_start, s_end) = setup.content_range();
        let setup_content = &source[s_start as usize..s_end as usize];

        // Offset 0 should map to the start of normal script content
        let abs_normal = analysis_span_to_sfc_offset(0, &blocks);
        assert_eq!(abs_normal, n_start);

        // Offset past normal content + \n separator should map to setup content
        let setup_base = normal_len + 1; // +1 for the \n separator
        let abs_setup = analysis_span_to_sfc_offset(setup_base, &blocks);
        assert_eq!(abs_setup, s_start);

        // "count" in setup block content, mapped through combined offset
        let count_in_setup = setup_content.find("count").unwrap() as u32;
        let count_in_combined = setup_base + count_in_setup;
        let abs_count = analysis_span_to_sfc_offset(count_in_combined, &blocks);
        assert_eq!(
            &source[abs_count as usize..abs_count as usize + 5],
            "count",
            "should point to 'count' in <script setup>"
        );
    }

    /// @ai-generated - Rename with dual script blocks correctly adjusts spans.
    #[test]
    fn test_rename_with_dual_script_blocks() {
        let source = "<template>\n  {{ count }}\n</template>\n<script>\nexport default {}\n</script>\n<script setup>\nconst count = ref(0)\n</script>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let normal = blocks
            .iter()
            .find(|b| b.tag_name == "script" && !b.is_setup())
            .unwrap();
        let setup = blocks
            .iter()
            .find(|b| b.tag_name == "script" && b.is_setup())
            .unwrap();
        let (n_start, n_end) = normal.content_range();
        let normal_len = n_end - n_start;
        let (s_start, s_end) = setup.content_range();
        let setup_content = &source[s_start as usize..s_end as usize];

        // "count" in <script setup> content, mapped to combined content offset
        let count_in_setup = setup_content.find("count").unwrap() as u32;
        let count_combined_offset = normal_len + 1 + count_in_setup;

        let template_count = source.find("count").unwrap();

        let analysis = FileAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::Ref,
                type_annotation: None,
                initializer: None,
                span: verter_span::Span::new(count_combined_offset, count_combined_offset + 5),
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
        let count_sfc_offset = source.rfind("count").unwrap() as u32;
        assert!(
            edits.iter().any(|e| {
                line_index.position_to_offset(&e.range.start) == Some(count_sfc_offset)
            }),
            "should have an edit at the declaration site in <script setup>"
        );
    }
}
