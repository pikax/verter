// Go-to-definition: span-based navigation from verter_host analysis.
//
// Supports navigation from:
// - Template bindings → script declarations
// - Import bindings → source files (with tsconfig path alias resolution)
// - Component tags → component source files
// - CSS class/ID in template ↔ style selectors (bidirectional)
// - Import source strings → resolved files
// - DOM query selector strings → matching template elements (with CSS rule fallback)

use tower_lsp_server::ls_types::*;
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
///
/// The optional `resolve_export_location` callback resolves a cross-file import to
/// the exact location of the exported symbol in the target file.
/// Takes `(canonical_id, binding_name)` and returns a `Location` with precise range.
/// When this returns `None`, the function also returns `None` for cross-file imports,
/// letting the type provider handle it (it can navigate to the exact symbol).
#[allow(clippy::type_complexity)]
pub fn definition_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    resolve_path: Option<&dyn Fn(&str) -> Option<String>>,
    resolve_export_location: Option<&dyn Fn(&str, &str) -> Option<Location>>,
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
                    // Try to resolve cross-file with precise export location
                    if let Some(ref canonical_id) = import.resolved_canonical_id {
                        if let Some(result) = try_precise_cross_file(
                            canonical_id,
                            &binding.name,
                            resolve_export_location,
                        ) {
                            return Some(result);
                        }
                        // Can't resolve to exact location → let type provider handle it
                        return None;
                    }
                    // Try path alias resolution (tsconfig paths)
                    if let Some(resolved) = resolve_path.as_ref().and_then(|rp| rp(&import.source))
                    {
                        if let Some(result) = try_precise_cross_file(
                            &resolved,
                            &binding.name,
                            resolve_export_location,
                        ) {
                            return Some(result);
                        }
                        return None;
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
            // Navigate $props → defineProps, $emit → defineEmits, $slots → defineSlots
            let macro_kind = match word.as_str() {
                "$props" => Some(verter_analysis::AnalyzedMacroKind::DefineProps),
                "$emit" => Some(verter_analysis::AnalyzedMacroKind::DefineEmits),
                "$slots" => Some(verter_analysis::AnalyzedMacroKind::DefineSlots),
                _ => None,
            };
            if let Some(kind) = macro_kind {
                for mac in &analysis.macros {
                    if mac.kind == kind && (mac.span.start > 0 || mac.span.end > 0) {
                        return span_definition(mac.span.start, mac.span.end, line_index);
                    }
                }
            }

            // Check if cursor is on a v-on directive argument (@click, @input, etc.)
            // → navigate to the handler binding in script
            if let Some(ref template) = analysis.template {
                for el in &template.elements {
                    for dir in &el.directives {
                        if dir.name == "on" {
                            if let Some(ref arg_span) = dir.arg_span {
                                if (offset as u32) >= arg_span.start
                                    && (offset as u32) < arg_span.end
                                {
                                    // Find matching event handler with a named binding
                                    if let Some(handler) =
                                        template.event_handlers.iter().find(|h| {
                                            h.event_name == *dir.argument.as_deref().unwrap_or("")
                                                && h.span.start >= dir.span.start
                                                && h.span.end <= dir.span.end
                                        })
                                    {
                                        if let Some(ref binding_name) = handler.handler_binding {
                                            // Look up in bindings
                                            if let Some(b) = analysis
                                                .bindings
                                                .iter()
                                                .find(|b| b.name == *binding_name)
                                            {
                                                if b.span.start > 0 || b.span.end > 0 {
                                                    return span_definition(
                                                        b.span.start,
                                                        b.span.end,
                                                        line_index,
                                                    );
                                                }
                                            }
                                            // Look up in imports
                                            for import in &analysis.imports {
                                                for ib in &import.bindings {
                                                    if ib.name == *binding_name {
                                                        if let Some(ref cid) =
                                                            import.resolved_canonical_id
                                                        {
                                                            if let Some(result) =
                                                                try_precise_cross_file(
                                                                    cid,
                                                                    binding_name,
                                                                    resolve_export_location,
                                                                )
                                                            {
                                                                return Some(result);
                                                            }
                                                            return None;
                                                        }
                                                        if let Some(resolved) = resolve_path
                                                            .as_ref()
                                                            .and_then(|rp| rp(&import.source))
                                                        {
                                                            if let Some(result) =
                                                                try_precise_cross_file(
                                                                    &resolved,
                                                                    binding_name,
                                                                    resolve_export_location,
                                                                )
                                                            {
                                                                return Some(result);
                                                            }
                                                            return None;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // No handler binding found → return None for this directive
                                    return None;
                                }
                            }
                        }
                    }
                }
            }

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
                                    // Find the local binding name for this component import
                                    let comp_binding = import
                                        .bindings
                                        .first()
                                        .map(|b| b.name.as_str())
                                        .unwrap_or("default");
                                    if let Some(ref cid) = import.resolved_canonical_id {
                                        if let Some(result) = try_precise_cross_file(
                                            cid,
                                            comp_binding,
                                            resolve_export_location,
                                        ) {
                                            return Some(result);
                                        }
                                        return None;
                                    }
                                    if let Some(resolved) =
                                        resolve_path.as_ref().and_then(|rp| rp(&import.source))
                                    {
                                        if let Some(result) = try_precise_cross_file(
                                            &resolved,
                                            comp_binding,
                                            resolve_export_location,
                                        ) {
                                            return Some(result);
                                        }
                                        return None;
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
            // Check individual prop fields from defineProps
            for mac in &analysis.macros {
                if let Some(pf) = mac.prop_fields.iter().find(|pf| pf.name == *word) {
                    if pf.span.start > 0 || pf.span.end > 0 {
                        return span_definition(pf.span.start, pf.span.end, line_index);
                    }
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
                            if let Some(result) = try_precise_cross_file(
                                canonical_id,
                                &binding.name,
                                resolve_export_location,
                            ) {
                                return Some(result);
                            }
                            return None;
                        }
                        if let Some(resolved) =
                            resolve_path.as_ref().and_then(|rp| rp(&import.source))
                        {
                            if let Some(result) = try_precise_cross_file(
                                &resolved,
                                &binding.name,
                                resolve_export_location,
                            ) {
                                return Some(result);
                            }
                            return None;
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
/// Returns `Range::default()` (file top) — used only as a fallback when no precise
/// export location is available.
pub(crate) fn resolved_import_definition(canonical_id: &str) -> Option<GotoDefinitionResponse> {
    let uri = canonical_id_to_uri(canonical_id)?;
    Some(GotoDefinitionResponse::Scalar(Location {
        uri,
        range: Range::default(),
    }))
}

/// Convert a canonical ID to a file:// URI.
fn canonical_id_to_uri(canonical_id: &str) -> Option<Uri> {
    let normalized = canonical_id.replace('\\', "/");
    let uri_str = if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else if normalized.chars().nth(1) == Some(':') {
        format!("file:///{normalized}")
    } else {
        return None;
    };
    uri_str.parse().ok()
}

/// Try to resolve a cross-file import to a precise export location.
/// If `resolve_export_location` returns a Location, wrap it. Otherwise return `None`
/// to let the type provider handle it.
#[allow(clippy::type_complexity)]
fn try_precise_cross_file(
    canonical_id: &str,
    binding_name: &str,
    resolve_export_location: Option<&dyn Fn(&str, &str) -> Option<Location>>,
) -> Option<GotoDefinitionResponse> {
    let resolve = resolve_export_location?;
    let loc = resolve(canonical_id, binding_name)?;
    Some(GotoDefinitionResponse::Scalar(loc))
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
#[path = "definition_tests.rs"]
mod definition_tests;
