// Go-to-definition: span-based navigation from verter_host analysis.
//
// Supports navigation from:
// - Template bindings → script declarations
// - Import bindings → source files (with tsconfig path alias resolution)
// - Component tags → component source files
// - CSS class/ID in template ↔ style selectors (bidirectional)
// - Import source strings → resolved files
// - DOM query selector strings → matching template elements (with CSS rule fallback)

use tower_lsp_server::lsp_types::*;
use verter_analysis::types::{DomQueryCallSite, DomQueryKind};
use verter_analysis::{match_selector, MatchResult};
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Sentinel URI used when a definition is in the same file.
/// The server replaces this with the actual document URI before returning to the client.
pub const SAME_FILE_URI: &str = "verter-internal:same-file";

/// Attempt to provide go-to-definition at a given position.
///
/// Strategy:
/// 1. Find the word at the cursor position
/// 2. Look it up in analysis data:
///    - If it's an imported binding with `resolved_canonical_id`, navigate to the source file
///    - If it's an imported binding without resolution, try the path resolver, then fall back
///    - If it's a script binding (in template context), navigate to its span in script
///    - If it's a macro binding name, navigate to the macro call span
///
/// The optional `resolve_path` callback resolves import specifiers (e.g., `@/Foo.vue`)
/// to absolute canonical file paths using tsconfig.json `compilerOptions.paths`.
#[allow(clippy::type_complexity)]
pub fn definition_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    resolve_path: Option<&dyn Fn(&str) -> Option<String>>,
) -> Option<GotoDefinitionResponse> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;

    // Early exit: don't navigate from inside HTML comments in template
    let in_template = blocks.iter().any(|b| {
        b.tag_name == "template" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });
    if in_template && is_inside_html_comment(source, offset) {
        return None;
    }

    let word = word_at_offset(source, offset);

    // Word-based navigation (import bindings, component tags, script bindings)
    if let Some(ref word) = word {
        // Check if the word is an import binding — navigate to source file or import statement
        for import in &analysis.imports {
            for binding in &import.bindings {
                if binding.name == *word {
                    // If we have a resolved canonical ID, navigate to the source file
                    if let Some(ref canonical_id) = import.resolved_canonical_id {
                        return resolved_import_definition(canonical_id);
                    }
                    // Try path alias resolution (tsconfig paths)
                    if let Some(resolved) = resolve_path.as_ref().and_then(|rp| rp(&import.source))
                    {
                        return resolved_import_definition(&resolved);
                    }
                    // Otherwise, navigate to the import statement itself using span data
                    if import.span.start > 0 || import.span.end > 0 {
                        return span_definition(import.span.start, import.span.end, line_index);
                    }
                    return None;
                }
            }
        }

        if in_template {
            // Check if cursor is inside a class or id attribute — navigate to CSS selector
            if let Some(result) = css_definition_from_template(offset, source, analysis, line_index)
            {
                return Some(result);
            }

            // Check if cursor is on a component tag — navigate to the imported component file
            if let Some(ref template) = analysis.template {
                for comp in &template.components {
                    if comp.name == *word || to_pascal_case(&comp.name) == *word {
                        if let Some(ref src) = comp.import_source {
                            for import in &analysis.imports {
                                if import.source == *src {
                                    if let Some(ref cid) = import.resolved_canonical_id {
                                        return resolved_import_definition(cid);
                                    }
                                    // Try path alias resolution
                                    if let Some(resolved) =
                                        resolve_path.as_ref().and_then(|rp| rp(&import.source))
                                    {
                                        return resolved_import_definition(&resolved);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Find the binding definition using span data
            if let Some(binding) = analysis.bindings.iter().find(|b| b.name == *word) {
                if binding.span.start > 0 || binding.span.end > 0 {
                    return span_definition(binding.span.start, binding.span.end, line_index);
                }
            }
            // Check macro binding names
            for mac in &analysis.macros {
                if mac.binding_name.as_ref().is_some_and(|n| n == word)
                    && (mac.span.start > 0 || mac.span.end > 0)
                {
                    return span_definition(mac.span.start, mac.span.end, line_index);
                }
            }
        }
    }

    // Positional navigation (no word required — works inside strings, CSS selectors, etc.)

    // In script context, check import source strings and binding names
    let in_script = blocks.iter().any(|b| {
        b.tag_name == "script" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });

    if in_script {
        // Check if cursor is on an import binding name — navigate to import source
        if let Some(ref word) = word {
            for import in &analysis.imports {
                for binding in &import.bindings {
                    if binding.name == *word {
                        if let Some(ref canonical_id) = import.resolved_canonical_id {
                            return resolved_import_definition(canonical_id);
                        }
                        // Try path alias resolution
                        if let Some(resolved) =
                            resolve_path.as_ref().and_then(|rp| rp(&import.source))
                        {
                            return resolved_import_definition(&resolved);
                        }
                    }
                }
            }
        }

        // Check if cursor is inside an import source string — navigate to the file
        if let Some(result) =
            import_source_definition(offset, source, analysis, line_index, resolve_path)
        {
            return Some(result);
        }

        // Check if cursor is inside a DOM query selector string — navigate to matched element
        if let Some(result) = dom_query_definition(offset, blocks, analysis, line_index) {
            return Some(result);
        }
    }

    // Check if we're in a style block — navigate from CSS selector to template usage
    let in_style = blocks.iter().any(|b| {
        b.tag_name == "style" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });

    if in_style {
        if let Some(result) = css_definition_from_style(offset, source, analysis, line_index) {
            return Some(result);
        }
    }

    None
}

/// Create a definition response from a resolved canonical ID (cross-file navigation).
///
/// Normalizes Windows backslashes to forward slashes before constructing the file URI.
pub(crate) fn resolved_import_definition(canonical_id: &str) -> Option<GotoDefinitionResponse> {
    // Normalize backslashes for Windows paths
    let normalized = canonical_id.replace('\\', "/");
    // Convert canonical ID back to a file:// URI
    let uri_str = if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else if normalized.chars().nth(1) == Some(':') {
        // Windows drive letter (e.g., "D:/projects/...")
        format!("file:///{normalized}")
    } else {
        return None;
    };

    let uri: Uri = uri_str.parse().ok()?;
    Some(GotoDefinitionResponse::Scalar(Location {
        uri,
        range: Range::default(),
    }))
}

/// Create a same-file definition response from analysis span data.
fn span_definition(
    span_start: u32,
    span_end: u32,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    let start = line_index.offset_to_position(span_start)?;
    let end = line_index.offset_to_position(span_end)?;
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: SAME_FILE_URI.parse().unwrap(),
        range: Range { start, end },
    }))
}

// =============================================================================
// CSS Navigation (template ↔ style)
// =============================================================================

/// Enum for class/id navigation target.
enum CssTarget {
    Class(String),
    Id(String),
}

/// Detect if cursor is inside a template `class` or `id` attribute value,
/// and navigate to the matching CSS selector in style blocks.
fn css_definition_from_template(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    let template = analysis.template.as_ref()?;

    // Find which attribute (if any) contains the cursor
    let target = find_css_target_in_template(offset, source, template)?;

    // Search style blocks for matching CSS selector
    find_css_selector_definition(&target, analysis, line_index)
}

/// Find a class or id name at cursor position within template attributes.
fn find_css_target_in_template(
    offset: usize,
    source: &str,
    template: &verter_analysis::template::TemplateAnalysisSnapshot,
) -> Option<CssTarget> {
    for element in &template.elements {
        for attr in &element.attributes {
            // Only handle static class and id attributes
            if attr.is_dynamic {
                continue;
            }

            let attr_name = attr.name.as_str();
            if attr_name != "class" && attr_name != "id" {
                continue;
            }

            // Check if cursor is within this attribute's span
            if (offset as u32) < attr.span.start || (offset as u32) >= attr.span.end {
                continue;
            }

            let value = match attr.value.as_ref() {
                Some(v) => v,
                None => continue,
            };

            // Find the value portion within the attribute span.
            // Attribute span covers `class="btn primary"`.
            // Search for the value string in the source within the attribute span range.
            let attr_text = &source[attr.span.start as usize..attr.span.end as usize];
            let value_offset_in_attr = attr_text.find(value)?;
            let value_start = attr.span.start as usize + value_offset_in_attr;
            let value_end = value_start + value.len();

            // Check cursor is within the value
            if offset < value_start || offset >= value_end {
                continue;
            }

            if attr_name == "id" {
                return Some(CssTarget::Id(value.clone()));
            }

            // For class, split on whitespace and find which class name the cursor is on
            let cursor_in_value = offset - value_start;
            let mut pos = 0;
            for class_name in value.split_whitespace() {
                // Find position of this class_name in the remaining value
                let name_start = value[pos..].find(class_name)? + pos;
                let name_end = name_start + class_name.len();

                if cursor_in_value >= name_start && cursor_in_value < name_end {
                    return Some(CssTarget::Class(class_name.to_string()));
                }

                pos = name_end;
            }
        }
    }
    None
}

/// Search style blocks for a CSS class or ID selector matching the target.
fn find_css_selector_definition(
    target: &CssTarget,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    for style in &analysis.styles {
        let css = style.css.as_ref()?;
        match target {
            CssTarget::Class(name) => {
                for cls in &css.classes {
                    if cls.name == *name && cls.span.start > 0 {
                        // Convert content-relative offset to SFC-absolute
                        let abs_start = style.content_offset + cls.span.start;
                        let abs_end = style.content_offset + cls.span.end;
                        return span_definition(abs_start, abs_end, line_index);
                    }
                }
            }
            CssTarget::Id(name) => {
                for id in &css.ids {
                    if id.name == *name && id.span.start > 0 {
                        let abs_start = style.content_offset + id.span.start;
                        let abs_end = style.content_offset + id.span.end;
                        return span_definition(abs_start, abs_end, line_index);
                    }
                }
            }
        }
    }
    None
}

/// Navigate from a CSS selector in style to template usage.
/// When cursor is on `.btn` in `<style>`, navigate to `class="btn"` in template.
fn css_definition_from_style(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    let template = analysis.template.as_ref()?;

    // Find which style block contains the cursor and extract the class/id name
    let target = find_css_target_in_style(offset, source, analysis)?;

    // Search template elements for matching class/id attribute
    for element in &template.elements {
        for attr in &element.attributes {
            if attr.is_dynamic {
                continue;
            }

            let value = match attr.value.as_ref() {
                Some(v) => v,
                None => continue,
            };

            match &target {
                CssTarget::Class(name) => {
                    if attr.name == "class" && value.split_whitespace().any(|c| c == name) {
                        return span_definition(attr.span.start, attr.span.end, line_index);
                    }
                }
                CssTarget::Id(name) => {
                    if attr.name == "id" && value == name {
                        return span_definition(attr.span.start, attr.span.end, line_index);
                    }
                }
            }
        }
    }
    None
}

/// Extract class/id name at cursor position within a style block.
fn find_css_target_in_style(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<CssTarget> {
    for style in &analysis.styles {
        let css = match style.css.as_ref() {
            Some(c) => c,
            None => continue,
        };

        let co = style.content_offset as usize;

        // Check classes
        for cls in &css.classes {
            let abs_start = co + cls.span.start as usize;
            let abs_end = co + cls.span.end as usize;
            if offset >= abs_start && offset < abs_end {
                // Verify the source matches
                if abs_end <= source.len() && source[abs_start..abs_end] == cls.name {
                    return Some(CssTarget::Class(cls.name.clone()));
                }
            }
        }

        // Check IDs
        for id in &css.ids {
            let abs_start = co + id.span.start as usize;
            let abs_end = co + id.span.end as usize;
            if offset >= abs_start
                && offset < abs_end
                && abs_end <= source.len()
                && source[abs_start..abs_end] == id.name
            {
                return Some(CssTarget::Id(id.name.clone()));
            }
        }
    }
    None
}

// =============================================================================
// Import Source Navigation
// =============================================================================

/// Navigate from an import source string to the resolved file.
/// When cursor is inside `'./Foo.vue'`, navigate to Foo.vue.
#[allow(clippy::type_complexity)]
fn import_source_definition(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
    resolve_path: Option<&dyn Fn(&str) -> Option<String>>,
) -> Option<GotoDefinitionResponse> {
    for import in &analysis.imports {
        if import.span.start == 0 && import.span.end == 0 {
            continue;
        }

        let span_start = import.span.start as usize;
        let span_end = import.span.end as usize;

        // Check cursor is within the import statement span
        if offset < span_start || offset >= span_end {
            continue;
        }

        // Find the source string literal within the import span
        let import_text = &source[span_start..span_end];

        // Search for the quoted source string
        for quote in ['"', '\''] {
            let needle = format!("{}{}{}", quote, import.source, quote);
            if let Some(pos) = import_text.find(&needle) {
                let str_start = span_start + pos + 1; // after opening quote
                let str_end = str_start + import.source.len();

                // Check cursor is within the string literal
                if offset >= str_start && offset < str_end {
                    if let Some(ref canonical_id) = import.resolved_canonical_id {
                        return resolved_import_definition(canonical_id);
                    }
                    // Try path alias resolution (tsconfig paths)
                    if let Some(resolved) = resolve_path.as_ref().and_then(|rp| rp(&import.source))
                    {
                        return resolved_import_definition(&resolved);
                    }
                    // No resolution — navigate to import statement itself
                    return span_definition(import.span.start, import.span.end, line_index);
                }
            }
        }
    }
    None
}

// =============================================================================
// DOM Query Selector Navigation
// =============================================================================

/// Navigate from inside a DOM query selector string to the matching template element.
///
/// When the cursor is inside the string argument of `querySelector('.btn')`,
/// `getElementById('app')`, etc., navigates to the matching template element.
/// Falls back to the CSS rule definition if no template element matches.
fn dom_query_definition(
    offset: usize,
    _blocks: &[SfcBlock],
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    let template = analysis.template.as_ref()?;
    let elements = &template.elements;

    // DomQueryCallSite spans are SFC-absolute (adjusted by verter_host during analysis)
    let call = analysis
        .dom_query_calls
        .iter()
        .find(|c| offset >= c.arg_span.start as usize && offset < c.arg_span.end as usize)?;

    let parsed = call.parsed.as_ref()?;

    // Match against template elements
    let mut first_match: Option<usize> = None;
    for (i, _el) in elements.iter().enumerate() {
        let result = match_selector(parsed, i, elements);
        if result == MatchResult::Matches || result == MatchResult::MaybeMatches {
            first_match = Some(i);
            break;
        }
    }

    if let Some(idx) = first_match {
        let el = &elements[idx];
        if el.span.start > 0 || el.span.end > 0 {
            return span_definition(el.span.start, el.span.end, line_index);
        }
    }

    // Fallback: try to find a matching CSS rule in style blocks
    dom_query_css_fallback(call, analysis, line_index)
}

/// Fallback: navigate from DOM query selector to a matching CSS rule definition.
fn dom_query_css_fallback(
    call: &DomQueryCallSite,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    let selector = &call.selector_text;

    for style in &analysis.styles {
        let css = style.css.as_ref()?;
        let co = style.content_offset;

        if let Some(class_name) = selector.strip_prefix('.') {
            for cls in &css.classes {
                if cls.name == class_name {
                    return span_definition(co + cls.span.start, co + cls.span.end, line_index);
                }
            }
        } else if let Some(id_name) = selector.strip_prefix('#') {
            for id in &css.ids {
                if id.name == id_name {
                    return span_definition(co + id.span.start, co + id.span.end, line_index);
                }
            }
        } else if call.kind == DomQueryKind::GetElementById {
            for id in &css.ids {
                if id.name == *selector {
                    return span_definition(co + id.span.start, co + id.span.end, line_index);
                }
            }
        } else if call.kind == DomQueryKind::GetElementsByClassName {
            for cls in &css.classes {
                if cls.name == *selector {
                    return span_definition(co + cls.span.start, co + cls.span.end, line_index);
                }
            }
        }
    }

    None
}

use crate::utils::word_at_offset;

/// Check whether a byte offset falls inside an HTML comment (`<!-- ... -->`).
pub fn is_inside_html_comment(source: &str, offset: usize) -> bool {
    let before = &source[..offset];
    let comment_start = before.rfind("<!--");
    let comment_end = before.rfind("-->");
    match (comment_start, comment_end) {
        (Some(start), Some(end)) => start > end,
        (Some(_), None) => true,
        _ => false,
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

    /// @ai-generated - CTRL+click inside HTML comment should not navigate
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

    /// @ai-generated - is_inside_html_comment detects comment boundaries correctly
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

    /// @ai-generated - Component navigation via template.components
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

    /// @ai-generated - to_pascal_case converts kebab-case to PascalCase
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

    /// @ai-generated - CTRL+Click on class in template navigates to .class in style
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
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(0, 0),
                    tag_span_end: 0,
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

    /// @ai-generated - CTRL+Click on class in multi-class attr navigates to correct class
    #[test]
    fn test_css_nav_multi_class_attr() {
        let source = "<template>\n  <div class=\"btn primary\"></div>\n</template>\n\n<style>\n.btn { } .primary { }\n</style>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        use verter_analysis::style::*;
        use verter_analysis::template::*;

        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        let (style_content_start, _) = style_block.content_range();
        let style_css =
            &source[style_content_start as usize..style_block.content_range().1 as usize];

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
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(0, 0),
                    tag_span_end: 0,
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

    /// @ai-generated - CTRL+Click on id="app" navigates to #app in style
    #[test]
    fn test_css_nav_template_id_to_style() {
        let source = "<template>\n  <div id=\"app\"></div>\n</template>\n\n<style>\n#app { margin: 0; }\n</style>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        use verter_analysis::style::*;
        use verter_analysis::template::*;

        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        let (style_content_start, _) = style_block.content_range();
        let style_css =
            &source[style_content_start as usize..style_block.content_range().1 as usize];

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
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(0, 0),
                    tag_span_end: 0,
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

    /// @ai-generated - Dynamic :class does not trigger CSS navigation
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
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(0, 0),
                    tag_span_end: 0,
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

    /// @ai-generated - CTRL+Click on .btn in style navigates to class="btn" in template
    #[test]
    fn test_css_nav_style_to_template() {
        let source = "<template>\n  <div class=\"btn\"></div>\n</template>\n\n<style>\n.btn { color: red; }\n</style>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        use verter_analysis::style::*;
        use verter_analysis::template::*;

        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        let (style_content_start, _) = style_block.content_range();
        let style_css =
            &source[style_content_start as usize..style_block.content_range().1 as usize];

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
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(0, 0),
                    tag_span_end: 0,
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

    /// @ai-generated - CTRL+Click on import source string navigates to resolved file
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

    /// @ai-generated - CTRL+Click on aliased import binding navigates via path resolver
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

    /// @ai-generated - CTRL+Click on aliased import string navigates via path resolver
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

    /// @ai-generated - CTRL+Click inside querySelector('.btn') navigates to template element
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
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(btn_elem_start, btn_elem_end),
                    tag_span_end: btn_elem_end,
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

    /// @ai-generated - DOM query with no matching element returns None (no CSS either)
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
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(0, 0),
                    tag_span_end: 0,
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

    /// @ai-generated - DOM query falls back to CSS rule when no template element matches
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
        let style_css =
            &source[style_content_start as usize..style_block.content_range().1 as usize];

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
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(0, 0),
                    tag_span_end: 0,
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

    /// @ai-generated - Component tag click resolves via path alias
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
}
