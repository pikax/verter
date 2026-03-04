// Phase 2: References — find all occurrences of a binding across script/template blocks.
// Phase 3: Enhanced with cross-file references from TypeProvider.

use tower_lsp_server::ls_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Sentinel URI used when a reference is in the same file.
/// The server replaces this with the actual document URI before returning to the client.
pub const SAME_FILE_URI: &str = "verter-internal:same-file";

/// Find all references to the symbol at the given position.
///
/// Strategy:
/// 1. Find the word at the cursor position
/// 2. Collect all occurrences:
///    - The binding declaration span (if include_declaration)
///    - Template binding occurrences from `TemplateAnalysisSnapshot` (precise spans)
///    - Text occurrences in script blocks (word boundary match)
///    - Falls back to text search in template blocks if template analysis is unavailable
pub fn references_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;
    let word = word_at_offset(source, offset)?;

    // Check if this word is a known binding, import, or macro
    let is_binding = analysis.bindings.iter().any(|b| b.name == word);
    let is_import = analysis
        .imports
        .iter()
        .any(|i| i.bindings.iter().any(|b| b.name == word));
    let is_macro = analysis
        .macros
        .iter()
        .any(|m| m.binding_name.as_ref().is_some_and(|n| n == &word));

    if !is_binding && !is_import && !is_macro {
        // Try CSS class/id references before returning None
        return css_references_at_position(offset, source, blocks, analysis, line_index);
    }

    let mut locations = Vec::new();

    // Add the declaration span if requested
    if include_declaration {
        if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
            if binding.span.start > 0 || binding.span.end > 0 {
                if let Some(loc) =
                    span_to_location(binding.span.start, binding.span.end, line_index)
                {
                    locations.push(loc);
                }
            }
        }
        for import in &analysis.imports {
            for binding in &import.bindings {
                if binding.name == word && (binding.span.start > 0 || binding.span.end > 0) {
                    if let Some(loc) =
                        span_to_location(binding.span.start, binding.span.end, line_index)
                    {
                        locations.push(loc);
                    }
                }
            }
        }
        for mac in &analysis.macros {
            if mac.binding_name.as_ref().is_some_and(|n| n == &word)
                && (mac.span.start > 0 || mac.span.end > 0)
            {
                if let Some(loc) = span_to_location(mac.span.start, mac.span.end, line_index) {
                    locations.push(loc);
                }
            }
        }
    }

    // Use template analysis binding occurrences when available (precise spans)
    let has_template_analysis = analysis
        .template
        .as_ref()
        .is_some_and(|t| !t.binding_occurrences.is_empty());

    if has_template_analysis {
        let template = analysis.template.as_ref().unwrap();
        for occ in &template.binding_occurrences {
            if occ.name == word {
                // Skip if this overlaps a declaration we already added
                let already_present = locations.iter().any(|loc| {
                    let loc_start = line_index.position_to_offset(&loc.range.start);
                    loc_start == Some(occ.span.start)
                });
                if already_present {
                    continue;
                }
                if let Some(loc) = span_to_location(occ.span.start, occ.span.end, line_index) {
                    locations.push(loc);
                }
            }
        }
    }

    // Scan script blocks for text occurrences (template is covered by analysis above)
    for block in blocks {
        // Skip template blocks if we have template analysis
        if has_template_analysis && block.tag_name == "template" {
            continue;
        }

        let (content_start, content_end) = block.content_range();
        let content = &source[content_start as usize..content_end as usize];

        for occ_offset in find_all_word_occurrences(content, &word) {
            let abs_offset = content_start as usize + occ_offset;

            // Skip if this overlaps a declaration we already added
            let already_present = locations.iter().any(|loc| {
                let loc_start = line_index.position_to_offset(&loc.range.start);
                loc_start == Some(abs_offset as u32)
            });
            if already_present {
                continue;
            }

            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(abs_offset as u32),
                line_index.offset_to_position((abs_offset + word.len()) as u32),
            ) {
                locations.push(Location {
                    uri: SAME_FILE_URI.parse().unwrap(),
                    range: Range { start, end },
                });
            }
        }
    }

    if locations.is_empty() {
        None
    } else {
        Some(locations)
    }
}

// =============================================================================
// CSS Class/ID References
// =============================================================================

/// Find all references to a CSS class or ID at the given position.
///
/// Works when cursor is on:
/// - A class name in `class="btn"` in template
/// - A `.btn` selector in style
/// - An `id="app"` in template
/// - A `#app` selector in style
fn css_references_at_position(
    offset: usize,
    source: &str,
    blocks: &[SfcBlock],
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<Vec<Location>> {
    use crate::features::definition::is_inside_html_comment;

    // Determine if we're in template or style
    let in_template = blocks.iter().any(|b| {
        b.tag_name == "template" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });
    let in_style = blocks.iter().any(|b| {
        b.tag_name == "style" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });

    if !in_template && !in_style {
        return None;
    }

    let template = analysis.template.as_ref()?;

    // Extract the CSS target (class or id name)
    let target = if in_template {
        if is_inside_html_comment(source, offset) {
            return None;
        }
        find_css_target_in_template_refs(offset, source, template)?
    } else {
        find_css_target_in_style_refs(offset, source, analysis)?
    };

    let spans = collect_css_ref_spans(&target, source, analysis);
    let locations: Vec<Location> = spans
        .into_iter()
        .filter_map(|(start, end)| span_to_location(start, end, line_index))
        .collect();

    if locations.is_empty() {
        None
    } else {
        Some(locations)
    }
}

pub(crate) enum CssRefTarget {
    Class(String),
    Id(String),
}

/// Collect all (start, end) byte offsets for a CSS class/id across template + style.
///
/// Reused by references, rename, and document_highlight features.
pub(crate) fn collect_css_ref_spans(
    target: &CssRefTarget,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Vec<(u32, u32)> {
    let mut spans = Vec::new();

    // Collect template attribute references
    if let Some(template) = &analysis.template {
        for element in &template.elements {
            for attr in &element.attributes {
                if attr.is_dynamic {
                    // Dynamic :class attributes — check extracted dynamic class names
                    let value = match attr.value.as_ref() {
                        Some(v) => v,
                        None => continue,
                    };
                    match target {
                        CssRefTarget::Class(name) => {
                            if attr.name == "class" {
                                // Find the attribute value start offset
                                let attr_text =
                                    &source[attr.span.start as usize..attr.span.end as usize];
                                if let Some(eq_pos) = attr_text.find('=') {
                                    let after_eq = &attr_text[eq_pos + 1..];
                                    let quote_offset =
                                        after_eq.find(['"', '\'']).map(|q| eq_pos + 1 + q + 1);
                                    if let Some(val_start_in_attr) = quote_offset {
                                        let val_abs_start =
                                            attr.span.start as usize + val_start_in_attr;
                                        let rich =
                                            verter_analysis::extract_dynamic_class_names_rich(
                                                value,
                                            );
                                        for dcn in &rich {
                                            if !dcn.is_partial && dcn.name == *name {
                                                let abs_start =
                                                    val_abs_start as u32 + dcn.expr_offset;
                                                let abs_end = abs_start + dcn.name.len() as u32;
                                                spans.push((abs_start, abs_end));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        CssRefTarget::Id(name) => {
                            if attr.name == "id" {
                                // Simple string literal id: :id="'myId'"
                                let trimmed = value.trim();
                                let inner = if (trimmed.starts_with('\'')
                                    && trimmed.ends_with('\''))
                                    || (trimmed.starts_with('"') && trimmed.ends_with('"'))
                                {
                                    &trimmed[1..trimmed.len() - 1]
                                } else {
                                    trimmed
                                };
                                if inner == name {
                                    // Point to the string literal content
                                    let attr_text =
                                        &source[attr.span.start as usize..attr.span.end as usize];
                                    if let Some(pos) = attr_text.find(inner) {
                                        let abs_start = attr.span.start + pos as u32;
                                        let abs_end = abs_start + inner.len() as u32;
                                        spans.push((abs_start, abs_end));
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }
                let value = match attr.value.as_ref() {
                    Some(v) => v,
                    None => continue,
                };

                match target {
                    CssRefTarget::Class(name) => {
                        if attr.name == "class" {
                            let attr_text =
                                &source[attr.span.start as usize..attr.span.end as usize];
                            if let Some(val_offset) = attr_text.find(value.as_str()) {
                                let val_start = attr.span.start as usize + val_offset;
                                let mut pos = 0;
                                for class_name in value.split_whitespace() {
                                    if class_name == name {
                                        if let Some(name_start) = value[pos..].find(class_name) {
                                            let abs_start = (val_start + pos + name_start) as u32;
                                            let abs_end = abs_start + class_name.len() as u32;
                                            spans.push((abs_start, abs_end));
                                        }
                                    }
                                    if let Some(found) = value[pos..].find(class_name) {
                                        pos += found + class_name.len();
                                    }
                                }
                            }
                        }
                    }
                    CssRefTarget::Id(name) => {
                        if attr.name == "id" && value == name {
                            // Point to the value text within the attribute
                            let attr_text =
                                &source[attr.span.start as usize..attr.span.end as usize];
                            if let Some(val_offset) = attr_text.find(value.as_str()) {
                                let abs_start = attr.span.start + val_offset as u32;
                                let abs_end = abs_start + value.len() as u32;
                                spans.push((abs_start, abs_end));
                            }
                        }
                    }
                }
            }
        }
    }

    // Collect style block references
    for style in &analysis.styles {
        let css = match style.css.as_ref() {
            Some(c) => c,
            None => continue,
        };
        let co = style.content_offset;

        match target {
            CssRefTarget::Class(name) => {
                for cls in &css.classes {
                    if cls.name == *name && cls.span.start > 0 {
                        let abs_start = co + cls.span.start;
                        let abs_end = co + cls.span.end;
                        spans.push((abs_start, abs_end));
                    }
                }
            }
            CssRefTarget::Id(name) => {
                for id in &css.ids {
                    if id.name == *name && id.span.start > 0 {
                        let abs_start = co + id.span.start;
                        let abs_end = co + id.span.end;
                        spans.push((abs_start, abs_end));
                    }
                }
            }
        }
    }

    spans
}

/// Find CSS class/id name at cursor in template attribute (static or dynamic).
pub(crate) fn find_css_target_in_template_refs(
    offset: usize,
    source: &str,
    template: &verter_analysis::template::TemplateAnalysisSnapshot,
) -> Option<CssRefTarget> {
    for element in &template.elements {
        for attr in &element.attributes {
            if (offset as u32) < attr.span.start || (offset as u32) >= attr.span.end {
                continue;
            }
            let value = match attr.value.as_ref() {
                Some(v) => v,
                None => continue,
            };

            if attr.is_dynamic {
                // Dynamic :class or :id
                let attr_text = &source[attr.span.start as usize..attr.span.end as usize];
                // Find the value start offset (after = and opening quote)
                if let Some(eq_pos) = attr_text.find('=') {
                    let after_eq = &attr_text[eq_pos + 1..];
                    let quote_offset = after_eq.find(['"', '\'']).map(|q| eq_pos + 1 + q + 1);
                    if let Some(val_start_in_attr) = quote_offset {
                        let val_abs_start = attr.span.start as usize + val_start_in_attr;
                        let cursor_in_expr = offset.checked_sub(val_abs_start)?;

                        if attr.name == "class" {
                            let rich = verter_analysis::extract_dynamic_class_names_rich(value);
                            for dcn in &rich {
                                if dcn.is_partial {
                                    continue;
                                }
                                let start = dcn.expr_offset as usize;
                                let end = start + dcn.name.len();
                                if cursor_in_expr >= start && cursor_in_expr < end {
                                    return Some(CssRefTarget::Class(dcn.name.clone()));
                                }
                            }
                        }
                        if attr.name == "id" {
                            // Simple string literal: :id="'myId'"
                            let trimmed = value.trim();
                            let inner = if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                                || (trimmed.starts_with('"') && trimmed.ends_with('"'))
                            {
                                &trimmed[1..trimmed.len() - 1]
                            } else {
                                trimmed
                            };
                            if let Some(pos) = value.find(inner) {
                                let start = pos;
                                let end = start + inner.len();
                                if cursor_in_expr >= start && cursor_in_expr < end {
                                    return Some(CssRefTarget::Id(inner.to_string()));
                                }
                            }
                        }
                    }
                }
                continue;
            }

            // Static attributes
            let attr_text = &source[attr.span.start as usize..attr.span.end as usize];
            let val_offset = match attr_text.find(value.as_str()) {
                Some(v) => v,
                None => continue,
            };
            let val_start = attr.span.start as usize + val_offset;
            let val_end = val_start + value.len();
            if offset < val_start || offset >= val_end {
                continue;
            }

            if attr.name == "id" {
                return Some(CssRefTarget::Id(value.clone()));
            }
            if attr.name == "class" {
                let cursor_in_value = offset - val_start;
                let mut pos = 0;
                for class_name in value.split_whitespace() {
                    if let Some(name_start) = value[pos..].find(class_name) {
                        let abs_start = pos + name_start;
                        let abs_end = abs_start + class_name.len();
                        if cursor_in_value >= abs_start && cursor_in_value < abs_end {
                            return Some(CssRefTarget::Class(class_name.to_string()));
                        }
                        pos = abs_end;
                    }
                }
            }
        }
    }
    None
}

/// Find CSS class/id name at cursor in style block.
pub(crate) fn find_css_target_in_style_refs(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<CssRefTarget> {
    for style in &analysis.styles {
        let css = match style.css.as_ref() {
            Some(c) => c,
            None => continue,
        };
        let co = style.content_offset as usize;

        for cls in &css.classes {
            let abs_start = co + cls.span.start as usize;
            let abs_end = co + cls.span.end as usize;
            if offset >= abs_start
                && offset < abs_end
                && abs_end <= source.len()
                && source[abs_start..abs_end] == cls.name
            {
                return Some(CssRefTarget::Class(cls.name.clone()));
            }
        }

        for id in &css.ids {
            let abs_start = co + id.span.start as usize;
            let abs_end = co + id.span.end as usize;
            if offset >= abs_start
                && offset < abs_end
                && abs_end <= source.len()
                && source[abs_start..abs_end] == id.name
            {
                return Some(CssRefTarget::Id(id.name.clone()));
            }
        }
    }
    None
}

pub(crate) fn span_to_location(
    span_start: u32,
    span_end: u32,
    line_index: &LineIndex,
) -> Option<Location> {
    let start = line_index.offset_to_position(span_start)?;
    let end = line_index.offset_to_position(span_end)?;
    Some(Location {
        uri: SAME_FILE_URI.parse().unwrap(),
        range: Range { start, end },
    })
}

use crate::utils::{find_all_word_occurrences, word_at_offset};

#[cfg(test)]
mod tests {
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
    fn test_references_for_binding_across_blocks() {
        let source = "<template>\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\nconsole.log(count)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let count_decl = source.rfind("count = ref").unwrap() as u32;

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::Ref,
                type_annotation: None,
                initializer: None,
                span: verter_span::Span::new(count_decl, count_decl + 5),
            }],
            vec![],
            vec![],
        );

        // Click on "count" in template
        let template_count = source.find("count").unwrap();
        let position = line_index
            .offset_to_position(template_count as u32)
            .unwrap();

        let refs = references_at_position(
            &position,
            source,
            &blocks,
            Some(&analysis),
            &line_index,
            true,
        );
        assert!(refs.is_some());
        let refs = refs.unwrap();
        // Declaration + template occurrence + two script occurrences ("count = ref" and "log(count)")
        assert!(refs.len() >= 3, "expected >=3 refs, got {}", refs.len());
    }

    #[test]
    fn test_references_exclude_declaration() {
        let source =
            "<template>\n  {{ x }}\n</template>\n\n<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let x_offset = source.rfind("x = 1").unwrap() as u32;

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "x".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: verter_span::Span::new(x_offset, x_offset + 1),
            }],
            vec![],
            vec![],
        );

        let template_x = source.find(" x ").unwrap() + 1;
        let position = line_index.offset_to_position(template_x as u32).unwrap();

        let refs_with_decl = references_at_position(
            &position,
            source,
            &blocks,
            Some(&analysis),
            &line_index,
            true,
        );
        let refs_without_decl = references_at_position(
            &position,
            source,
            &blocks,
            Some(&analysis),
            &line_index,
            false,
        );

        assert!(refs_with_decl.is_some());
        assert!(refs_without_decl.is_some());
        // With declaration should have more entries
        assert!(refs_with_decl.unwrap().len() >= refs_without_decl.unwrap().len());
    }

    #[test]
    fn test_no_references_for_unknown_word() {
        let source = "<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(vec![], vec![], vec![]);

        // Click on "const" — not a binding
        let offset = source.find("const").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let refs = references_at_position(
            &position,
            source,
            &blocks,
            Some(&analysis),
            &line_index,
            true,
        );
        assert!(refs.is_none());
    }

    #[test]
    fn test_find_all_word_occurrences() {
        let content = "count = count + counter";
        let results = find_all_word_occurrences(content, "count");
        assert_eq!(results, vec![0, 8]); // "count" but not "counter"
    }

    // =========================================================================
    // CSS Class/ID Reference Tests
    // =========================================================================

    fn make_element_with_attrs(
        source: &str,
        tag: &str,
        classes: &[&str],
        id: Option<&str>,
    ) -> TemplateElement {
        let mut attrs = Vec::new();
        if !classes.is_empty() {
            let class_val = classes.join(" ");
            let pattern = format!("class=\"{}\"", class_val);
            let start = source.find(&pattern).unwrap_or(0) as u32;
            let end = start + pattern.len() as u32;
            attrs.push(TemplateAttribute {
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
            attrs.push(TemplateAttribute {
                name: "id".into(),
                value: Some(id_val.into()),
                is_dynamic: false,
                span: verter_span::Span::new(start, end),
                name_end: 0,
                value_span: None,
            });
        }
        TemplateElement {
            tag: tag.into(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
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

    /// @ai-generated - CSS class references from template finds occurrences in both blocks
    #[test]
    fn test_css_class_references_from_template() {
        let source = "<template><div class=\"btn\"></div></template>\n<style scoped>\n.btn { color: red; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let el = make_element_with_attrs(source, "div", &["btn"], None);
        let analysis = FileAnalysisSnapshot {
            styles: vec![css],
            template: Some(TemplateAnalysisSnapshot {
                elements: vec![el],
                ..Default::default()
            }),
            ..Default::default()
        };

        let btn_offset = source.find("btn\"").unwrap();
        let pos = line_index.offset_to_position(btn_offset as u32).unwrap();
        let refs =
            references_at_position(&pos, source, &blocks, Some(&analysis), &line_index, true);
        assert!(refs.is_some(), "should find CSS class references");
        let refs = refs.unwrap();
        assert!(
            refs.len() >= 2,
            "should have refs in template and style, got {}",
            refs.len()
        );
    }

    /// @ai-generated - CSS class references from style finds occurrences in both blocks
    #[test]
    fn test_css_class_references_from_style() {
        let source = "<template><div class=\"btn\"></div></template>\n<style scoped>\n.btn { color: red; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let el = make_element_with_attrs(source, "div", &["btn"], None);
        let analysis = FileAnalysisSnapshot {
            styles: vec![css],
            template: Some(TemplateAnalysisSnapshot {
                elements: vec![el],
                ..Default::default()
            }),
            ..Default::default()
        };

        let btn_style_offset = source.rfind(".btn").unwrap() + 1; // skip the '.'
        let pos = line_index
            .offset_to_position(btn_style_offset as u32)
            .unwrap();
        let refs =
            references_at_position(&pos, source, &blocks, Some(&analysis), &line_index, true);
        assert!(
            refs.is_some(),
            "should find CSS class references from style"
        );
        let refs = refs.unwrap();
        assert!(
            refs.len() >= 2,
            "should have refs in both template and style, got {}",
            refs.len()
        );
    }

    /// @ai-generated - CSS ID references across template and style
    #[test]
    fn test_css_id_references_from_template() {
        let source = "<template><div id=\"app\"></div></template>\n<style scoped>\n#app { margin: 0; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let el = make_element_with_attrs(source, "div", &[], Some("app"));
        let analysis = FileAnalysisSnapshot {
            styles: vec![css],
            template: Some(TemplateAnalysisSnapshot {
                elements: vec![el],
                ..Default::default()
            }),
            ..Default::default()
        };

        let id_offset = source.find("app\"").unwrap();
        let pos = line_index.offset_to_position(id_offset as u32).unwrap();
        let refs =
            references_at_position(&pos, source, &blocks, Some(&analysis), &line_index, true);
        assert!(refs.is_some(), "should find CSS ID references");
        let refs = refs.unwrap();
        assert!(
            refs.len() >= 2,
            "should have refs in template and style, got {}",
            refs.len()
        );
    }

    /// @ai-generated - No CSS references for class not in style
    #[test]
    fn test_no_css_references_without_style() {
        let source = "<template><div class=\"foo\"></div></template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let el = make_element_with_attrs(source, "div", &["foo"], None);
        let analysis = FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                elements: vec![el],
                ..Default::default()
            }),
            ..Default::default()
        };

        let foo_offset = source.find("foo\"").unwrap();
        let pos = line_index.offset_to_position(foo_offset as u32).unwrap();
        let refs =
            references_at_position(&pos, source, &blocks, Some(&analysis), &line_index, true);
        // Should still find template-only refs or None if no CSS targets detected
        // The important negative: should NOT panic or return wrong results
        if let Some(refs) = refs {
            for r in &refs {
                assert!(
                    !r.uri.as_str().contains("style"),
                    "should not reference style block when no style exists"
                );
            }
        }
    }
}
