// Go-to-definition: span-based navigation from verter_session analysis.
//
// Supports navigation from:
// - Template bindings → script declarations
// - Import bindings → source files (with tsconfig path alias resolution)
// - Component tags → component source files
// - CSS class/ID in template ↔ style selectors (bidirectional)
// - Import source strings → resolved files
// - DOM query selector strings → matching template elements (with CSS rule fallback)

use tower_lsp_server::ls_types::*;
use verter_semantic::analysis::types::{DomQueryCallSite, DomQueryKind};
use verter_semantic::analysis::{match_selector, MatchResult};
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

pub use super::sentinel_uris::SAME_FILE_URI;
pub use super::sentinel_uris::SAME_FILE_URI_STR;

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
    }) || analysis.template.as_deref().is_some_and(|template| {
        // The depth-ignorant SFC scanner closes the real template block at
        // the first nested `</template>`; the typed element tree is the
        // authority for template markup in those dead zones (D6 — custom
        // directive navigation must not die there). Svelte has no element
        // IR, so its behavior is unchanged.
        template
            .elements
            .iter()
            .any(|el| offset >= el.span.start as usize && offset < el.span.end as usize)
    });
    if in_template && is_inside_html_comment(source, offset) {
        return None;
    }

    // D6: custom directive NAME token (`v-my-thing` → `vMyThing`) — navigate
    // to the authored directive declaration (setup binding or import). Runs
    // BEFORE the word guard: a caret on the `-` of a kebab directive name
    // yields no identifier word, and the whole template section below is
    // word-guarded. Built-ins have no authored target (fail-closed empty);
    // unknown directives stay silent.
    if in_template {
        if let Some(ref template) = analysis.template {
            for el in &template.elements {
                for dir in &el.directives {
                    if crate::features::hover_directive_names::is_known_builtin_directive_pub(
                        &dir.name,
                    ) {
                        continue;
                    }
                    let region_end = dir
                        .arg_span
                        .as_ref()
                        .map(|span| span.start)
                        .unwrap_or(dir.name_end);
                    if (offset as u32) < dir.span.start || (offset as u32) >= region_end {
                        continue;
                    }
                    let binding_name =
                        crate::features::hover_directive_names::custom_directive_binding_name(
                            &dir.name,
                        );
                    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == binding_name)
                    {
                        if binding.span.start > 0 || binding.span.end > 0 {
                            return span_definition(
                                binding.span.start,
                                binding.span.end,
                                line_index,
                            );
                        }
                        return None;
                    }
                    for import in &analysis.imports {
                        for ib in &import.bindings {
                            if ib.name == binding_name {
                                if let Some(ref cid) = import.resolved_canonical_id {
                                    if let Some(result) = try_precise_cross_file(
                                        cid,
                                        &binding_name,
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
                                        &binding_name,
                                        resolve_export_location,
                                    ) {
                                        return Some(result);
                                    }
                                    return None;
                                }
                            }
                        }
                    }
                    // No authored declaration found → silent.
                    return None;
                }
            }
        }
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
                        // Default import of .vue file: the local name won't match script
                        // bindings, so retry with "default" which handles Vue SFC exports.
                        if crate::server::is_default_export_component_carrier(canonical_id) {
                            if let Some(result) = try_precise_cross_file(
                                canonical_id,
                                "default",
                                resolve_export_location,
                            ) {
                                return Some(result);
                            }
                            return resolved_import_definition(canonical_id);
                        }
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
                        if crate::server::is_default_export_component_carrier(&resolved) {
                            if let Some(result) = try_precise_cross_file(
                                &resolved,
                                "default",
                                resolve_export_location,
                            ) {
                                return Some(result);
                            }
                            return resolved_import_definition(&resolved);
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
                "$props" => Some(verter_semantic::analysis::AnalyzedMacroKind::DefineProps),
                "$emit" => Some(verter_semantic::analysis::AnalyzedMacroKind::DefineEmits),
                "$slots" => Some(verter_semantic::analysis::AnalyzedMacroKind::DefineSlots),
                _ => None,
            };
            if let Some(kind) = macro_kind {
                for mac in analysis.macros.iter() {
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
                                    if el.is_component {
                                        // Component event names resolve through the child
                                        // component's emits/props, not the parent handler.
                                        return None;
                                    }

                                    // Find matching event handler with a named binding
                                    if let Some(handler) =
                                        template.event_handlers.iter().find(|h| {
                                            h.event_name == *dir.argument.as_deref().unwrap_or("")
                                                && h.span.start == el.span.start
                                                && h.span.end == el.span.end
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

            // Check if cursor is inside a class or id attribute — navigate to
            // the CSS rule(s). A recognized class token FAILS CLOSED when no
            // rule matches: never fall through to a same-named script binding.
            if let Some(css_result) =
                css_definition_from_template(offset, source, analysis, line_index)
            {
                return css_result;
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
                                        if crate::server::is_default_export_component_carrier(cid) {
                                            if let Some(result) = try_precise_cross_file(
                                                cid,
                                                "default",
                                                resolve_export_location,
                                            ) {
                                                return Some(result);
                                            }
                                            return resolved_import_definition(cid);
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
                                        if crate::server::is_default_export_component_carrier(
                                            &resolved,
                                        ) {
                                            if let Some(result) = try_precise_cross_file(
                                                &resolved,
                                                "default",
                                                resolve_export_location,
                                            ) {
                                                return Some(result);
                                            }
                                            return resolved_import_definition(&resolved);
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
            for mac in analysis.macros.iter() {
                if let Some(pf) = mac.prop_fields.iter().find(|pf| pf.name == *word) {
                    if pf.span.start > 0 || pf.span.end > 0 {
                        return span_definition(pf.span.start, pf.span.end, line_index);
                    }
                }
            }
            // Check macro binding names
            for mac in analysis.macros.iter() {
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
                            if crate::server::is_default_export_component_carrier(canonical_id) {
                                if let Some(result) = try_precise_cross_file(
                                    canonical_id,
                                    "default",
                                    resolve_export_location,
                                ) {
                                    return Some(result);
                                }
                                return resolved_import_definition(canonical_id);
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
                            if crate::server::is_default_export_component_carrier(&resolved) {
                                if let Some(result) = try_precise_cross_file(
                                    &resolved,
                                    "default",
                                    resolve_export_location,
                                ) {
                                    return Some(result);
                                }
                                return resolved_import_definition(&resolved);
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

    // Positional CSS class navigation for template positions the word guard
    // misses (kebab-case class tokens have no identifier word at `-`).
    if in_template {
        if let Some(css_result) = css_definition_from_template(offset, source, analysis, line_index)
        {
            return css_result;
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
        uri: SAME_FILE_URI.clone(),
        range: Range { start, end },
    }))
}

// =============================================================================
// CSS Navigation (template ↔ style)
// =============================================================================

use crate::features::references::{
    find_css_target_in_style_refs, find_css_target_in_template_refs_with_element, CssRefTarget,
};

/// Detect if cursor is inside a template `class`/`:class`/`id` attribute
/// value and navigate to the matching CSS rule(s) in style blocks.
///
/// The OUTER `Option` is "was the cursor on a class/id token at all"; the
/// INNER `Option` is the navigation result. A recognized class token with no
/// matching rule yields `Some(None)` — the caller must fail closed (no
/// fallback to same-named script bindings — a mis-mapped affordance).
fn css_definition_from_template(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<Option<GotoDefinitionResponse>> {
    let template = analysis.template.as_deref()?;
    let (target, element_idx) =
        find_css_target_in_template_refs_with_element(offset, source, template)?;
    Some(css_rule_definition(
        &target,
        Some((element_idx, template)),
        analysis,
        line_index,
    ))
}

/// All declaration locations for a CSS class/id, hierarchy-ranked.
///
/// For classes, every rule declaring the class contributes its class-token
/// span, ordered by how the rule's selector relates to the origin element
/// (structural match first, then possible matches, then structure-less or
/// non-matching declarations in source order).
fn css_rule_definition(
    target: &CssRefTarget,
    element: Option<(
        usize,
        &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    )>,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    match target {
        CssRefTarget::Class(name) => {
            // (rank, source order, span)
            let mut hits: Vec<(u8, u32, verter_span::Span)> = Vec::new();
            for style in analysis.styles.iter() {
                let Some(css) = style.css.as_ref() else {
                    continue;
                };
                for cls in &css.classes {
                    if cls.name != *name || cls.span.start == 0 {
                        continue;
                    }
                    let rank = class_rule_match_rank(cls, css, element);
                    hits.push((rank, cls.span.start, cls.span));
                }
            }
            hits.sort_by_key(|&(rank, order, _)| (rank, order));
            let locations: Vec<Location> = hits
                .into_iter()
                .filter_map(|(_, _, span)| {
                    let start = line_index.offset_to_position(span.start)?;
                    let end = line_index.offset_to_position(span.end)?;
                    Some(Location {
                        uri: SAME_FILE_URI.clone(),
                        range: Range { start, end },
                    })
                })
                .collect();
            match locations.len() {
                0 => None,
                1 => Some(GotoDefinitionResponse::Scalar(
                    locations.into_iter().next().unwrap(),
                )),
                _ => Some(GotoDefinitionResponse::Array(locations)),
            }
        }
        CssRefTarget::Id(name) => {
            for style in analysis.styles.iter() {
                let Some(css) = style.css.as_ref() else {
                    continue;
                };
                for id in &css.ids {
                    if id.name == *name && id.span.start > 0 {
                        return span_definition(id.span.start, id.span.end, line_index);
                    }
                }
            }
            None
        }
    }
}

/// Rank a class declaration against the origin element:
/// 0 = the rule's selector structurally matches the element,
/// 1 = may match (dynamic classes),
/// 2 = no derivable structure / no element context,
/// 3 = structurally cannot match.
pub(crate) fn class_rule_match_rank(
    cls: &verter_semantic::analysis::style::AnalyzedCssClass,
    css: &verter_semantic::analysis::style::CssAnalysis,
    element: Option<(
        usize,
        &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    )>,
) -> u8 {
    let Some((element_idx, template)) = element else {
        return 2;
    };
    let Some(structure) = cls
        .selector_index
        .and_then(|si| css.selectors.get(si as usize))
        .and_then(|sel| sel.structure.as_ref())
    else {
        return 2;
    };
    match match_selector(structure, element_idx, &template.elements) {
        MatchResult::Matches => 0,
        MatchResult::MaybeMatches => 1,
        MatchResult::NoMatch => 3,
    }
}

/// Navigate from a CSS selector in style to template usage.
/// When cursor is on `.btn` in `<style>`, navigate to every `class="btn"`
/// usage in the template (all usages, source order).
fn css_definition_from_style(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    let template = analysis.template.as_deref()?;

    // Find which style block contains the cursor and extract the class/id name
    let target = find_css_target_in_style_refs(offset, source, analysis)?;

    let spans =
        crate::features::references::collect_template_css_ref_spans(&target, source, template);
    let locations: Vec<Location> = spans
        .into_iter()
        .filter_map(|(start, end)| {
            let start = line_index.offset_to_position(start)?;
            let end = line_index.offset_to_position(end)?;
            Some(Location {
                uri: SAME_FILE_URI.clone(),
                range: Range { start, end },
            })
        })
        .collect();
    match locations.len() {
        0 => None,
        1 => Some(GotoDefinitionResponse::Scalar(
            locations.into_iter().next().unwrap(),
        )),
        _ => Some(GotoDefinitionResponse::Array(locations)),
    }
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
                        if let Some(result) = resolved_import_definition(canonical_id) {
                            return Some(result);
                        }
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
    let template = analysis.template.as_deref()?;
    let elements = &template.elements;

    // DomQueryCallSite spans are SFC-absolute (adjusted by verter_session during analysis)
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

    for style in analysis.styles.iter() {
        let css = style.css.as_ref()?;

        if let Some(class_name) = selector.strip_prefix('.') {
            for cls in &css.classes {
                if cls.name == class_name {
                    return span_definition(cls.span.start, cls.span.end, line_index);
                }
            }
        } else if let Some(id_name) = selector.strip_prefix('#') {
            for id in &css.ids {
                if id.name == id_name {
                    return span_definition(id.span.start, id.span.end, line_index);
                }
            }
        } else if call.kind == DomQueryKind::GetElementById {
            for id in &css.ids {
                if id.name == *selector {
                    return span_definition(id.span.start, id.span.end, line_index);
                }
            }
        } else if call.kind == DomQueryKind::GetElementsByClassName {
            for cls in &css.classes {
                if cls.name == *selector {
                    return span_definition(cls.span.start, cls.span.end, line_index);
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
