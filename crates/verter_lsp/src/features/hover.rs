// Phase 2: Hover — binding name, kind, source location from verter_host analysis.
// Phase 3: Enhanced with full resolved type signature, JSDoc from TypeProvider.

use tower_lsp_server::ls_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::{
    classify_cursor, parse_opening_tag, SfcBlock, SfcCursorContext,
};

/// Attempt to provide hover information at a given position.
///
/// Strategy:
/// 1. Classify cursor context (opening tag, closing tag, content, root level)
/// 2. For SFC tags: show documentation for tag names and attributes
/// 3. For block content: look up bindings, imports, macros from analysis
pub fn hover_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Hover> {
    let offset = line_index.position_to_offset(position)?;

    // Check SFC structural context BEFORE requiring analysis data.
    // This allows hover on tags even when analysis hasn't completed.
    match classify_cursor(offset, blocks) {
        SfcCursorContext::OpeningTag { block_index } => {
            return sfc_tag_hover(source, &blocks[block_index], offset);
        }
        SfcCursorContext::ClosingTag { block_index } => {
            return sfc_tag_name_hover(&blocks[block_index].tag_name);
        }
        SfcCursorContext::RootLevel => return None,
        SfcCursorContext::BlockContent { .. } => {} // fall through to analysis-based hover
    }

    let analysis = analysis?;
    let offset = offset as usize;

    // Determine which block the cursor is in
    let block = blocks.iter().find(|b| {
        let (content_start, content_end) = b.content_range();
        offset >= content_start as usize && offset < content_end as usize
    })?;

    match block.tag_name.as_str() {
        "script" => hover_in_script(offset, source, analysis),
        "template" => hover_in_template(offset, source, analysis),
        "style" => crate::css::css_hover(position, source, blocks, Some(analysis), line_index),
        _ => None,
    }
}

// ── SFC Tag Hover ────────────────────────────────────────────────────────────

/// Hover on an SFC opening tag — dispatches to tag name or attribute hover.
fn sfc_tag_hover(source: &str, block: &SfcBlock, offset: u32) -> Option<Hover> {
    let ctx = parse_opening_tag(source, block);

    // Check if cursor is on the tag name
    if offset >= ctx.tag_name_start && offset < ctx.tag_name_end {
        return sfc_tag_name_hover(&ctx.tag_name);
    }

    // Check if cursor is on an attribute name or value
    for attr in &ctx.attrs {
        if offset >= attr.name_start && offset < attr.name_end {
            return sfc_attr_hover(&ctx.tag_name, &attr.name);
        }
        if let (Some(vs), Some(ve)) = (attr.value_start, attr.value_end) {
            if offset >= vs && offset < ve {
                return sfc_attr_hover(&ctx.tag_name, &attr.name);
            }
        }
    }

    None
}

/// Hover documentation for SFC tag names.
fn sfc_tag_name_hover(tag_name: &str) -> Option<Hover> {
    let doc = match tag_name {
        "script" => "**`<script>`** — JavaScript/TypeScript logic block.\n\nContains component logic, imports, and exports. Use `setup` attribute for Composition API.",
        "template" => "**`<template>`** — HTML template block.\n\nContains the component's template markup with Vue directives and expressions.",
        "style" => "**`<style>`** — CSS style block.\n\nContains component styles. Use `scoped` for component-scoped CSS, `module` for CSS modules.",
        _ => return Some(make_hover(format!("**`<{tag_name}>`** — Custom block."))),
    };
    Some(make_hover(doc.to_string()))
}

/// Hover documentation for SFC tag attributes.
fn sfc_attr_hover(tag_name: &str, attr_name: &str) -> Option<Hover> {
    let doc = match (tag_name, attr_name) {
        // script attributes
        ("script", "setup") => "**`setup`** — Enables `<script setup>` syntax.\n\nAll top-level bindings are automatically exposed to the template. Compiler macros like `defineProps()` and `defineEmits()` are available.",
        ("script", "lang") => "**`lang`** — Script language.\n\nValues: `ts` (TypeScript), `tsx`, `jsx`. Defaults to JavaScript.",
        ("script", "generic") => "**`generic`** — Generic type parameters for `<script setup>`.\n\nDefines component-level generics: `<script setup generic=\"T extends object\">`.",
        ("script", "attrs" | "attributes") => "**`attrs`** — Typed `$attrs` declaration.\n\nDefines the type of `$attrs` / `useAttrs()` return value:\n```vue\n<script setup attrs=\"{ class?: string }\">\n```",
        ("script", "src") => "**`src`** — External script source.\n\nLoad script content from an external file: `<script src=\"./script.ts\">`.",
        // template attributes
        ("template", "lang") => "**`lang`** — Template language.\n\nValues: `pug`. Defaults to HTML.",
        // style attributes
        ("style", "scoped") => "**`scoped`** — Component-scoped CSS.\n\nStyles only apply to the current component via automatically added data attributes.",
        ("style", "module") => "**`module`** — CSS Modules.\n\nCSS classes are exposed as `$style` object. Named modules: `<style module=\"classes\">`.",
        ("style", "lang") => "**`lang`** — Style language.\n\nValues: `scss`, `sass`, `less`, `stylus`. Defaults to CSS.",
        // common
        (_, "lang") => "**`lang`** — Block language preprocessor.",
        _ => return None,
    };
    Some(make_hover(doc.to_string()))
}

fn make_hover(value: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    }
}

fn hover_in_script(offset: usize, source: &str, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    // Check if the cursor is on a Vue API call site — add context if so
    if let Some(api_hover) = vue_api_hover_at_offset(offset as u32, analysis) {
        return Some(api_hover);
    }

    let word = word_at_offset(source, offset)?;
    hover_for_word(&word, analysis)
}

fn hover_in_template(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<Hover> {
    // Don't provide hover inside HTML comments
    if crate::features::definition::is_inside_html_comment(source, offset) {
        return None;
    }

    // Check if cursor is on a slot outlet element — show slot name and props
    if let Some(hover) = slot_outlet_hover(offset as u32, analysis) {
        return Some(hover);
    }

    // Check if cursor is on a template element tag name — show matching CSS rules
    if let Some(hover) = element_css_hover(offset as u32, analysis) {
        return Some(hover);
    }

    // Check if cursor is on a component element tag name — show prop constness info
    if let Some(hover) = component_prop_constness_hover(offset as u32, source, analysis) {
        return Some(hover);
    }

    // In template, look for bindings used in expressions like {{ myVar }}
    let word = word_at_offset(source, offset)?;

    // Guard: don't show binding/import hover for attribute names (e.g., `ref` attr
    // should not show Vue `ref()` import info). Let TSGO handle attribute names.
    if is_on_attribute_name(offset as u32, analysis) {
        return None;
    }

    hover_for_word(&word, analysis)
}

/// Check if the given offset falls on an attribute name or directive name in the template.
fn is_on_attribute_name(offset: u32, analysis: &FileAnalysisSnapshot) -> bool {
    if let Some(ref template) = analysis.template {
        for el in &template.elements {
            for attr in &el.attributes {
                if offset >= attr.span.start && offset < attr.name_end {
                    return true;
                }
            }
            for dir in &el.directives {
                if offset >= dir.span.start && offset < dir.name_end {
                    return true;
                }
            }
        }
    }
    false
}

/// When the cursor is on a static `class` or `style` attribute that has been merged
/// with a dynamic `:class`/`:style` binding, the static attribute is removed from
/// the generated TSX and TSGO can't provide hover at that position.
///
/// This function detects that case and returns the corresponding directive's argument
/// start position (the `class` in `:class`) — which IS mapped in the generated TSX
/// and can be used to redirect the TSGO hover query.
pub fn merged_attribute_redirect_offset(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
) -> Option<u32> {
    let template = analysis.template.as_ref()?;

    for el in &template.elements {
        // Check if cursor is on a static `class` or `style` attribute
        for attr in &el.attributes {
            if offset >= attr.span.start && offset < attr.name_end {
                let attr_name = &attr.name;
                if attr_name == "class" || attr_name == "style" {
                    // Check if this element also has a dynamic `:class` or `:style`
                    for dir in &el.directives {
                        if dir.name == "bind" && dir.argument.as_deref() == Some(attr_name.as_str())
                        {
                            // Return the directive argument start (the `class` in `:class`)
                            // which is mapped in TSX to the merged `class={normalizeClass(...)}`
                            if let Some(ref arg_span) = dir.arg_span {
                                return Some(arg_span.start);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// When hovering on a `<slot>` element (tag name, `name` attribute, or its value),
/// show slot outlet information: the slot name and any scoped props being passed.
///
/// This is the primary hover for slot outlets — the type provider typically returns
/// the unhelpful generic `() any` from Vue's `Slots` index signature.
fn slot_outlet_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_ref()?;

    // Find a <slot> element where the cursor is within the opening tag range.
    // span.start = `<`, tag_span_end = end of opening tag (after `>` or `/>`)
    let element = template
        .elements
        .iter()
        .find(|el| el.tag == "slot" && offset >= el.span.start && offset < el.tag_span_end)?;

    // Extract slot name from `name="..."` attribute
    let slot_name = element
        .attributes
        .iter()
        .find(|a| a.name == "name" && !a.is_dynamic)
        .and_then(|a| a.value.as_deref())
        .unwrap_or("default");

    // Collect scoped slot props (non-name attributes and directives with bind)
    let mut props: Vec<String> = Vec::new();
    for attr in &element.attributes {
        if attr.name == "name" && !attr.is_dynamic {
            continue;
        }
        if attr.is_dynamic {
            if let Some(ref val) = attr.value {
                props.push(format!(":{} = {}", attr.name, val));
            } else {
                props.push(format!(":{}", attr.name));
            }
        } else if let Some(ref val) = attr.value {
            props.push(format!("{} = \"{}\"", attr.name, val));
        }
    }
    for dir in &element.directives {
        if dir.name == "bind" {
            if let Some(ref arg) = dir.argument {
                props.push(format!(":{arg}"));
            }
        }
    }

    let mut lines = Vec::new();
    lines.push(format!("**`<slot>`** outlet — **\"{slot_name}\"**"));
    lines.push("Renders content provided by the parent component for this slot.".to_string());

    if !props.is_empty() {
        lines.push(format!("\n**Scoped props:** {}", props.join(", ")));
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    })
}

/// When hovering on a template element tag name, show matching CSS rules with specificity.
fn element_css_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_ref()?;

    // Find element at cursor position — match only the tag name, not the full element span.
    // Element span starts at '<', so tag name starts at span.start + 1.
    let (el_idx, element) = template.elements.iter().enumerate().find(|(_, el)| {
        let tag_start = el.span.start + 1; // skip '<'
        let tag_end = tag_start + el.tag.len() as u32;
        offset >= tag_start && offset < tag_end
    })?;

    // Collect matching selectors from all style blocks
    let mut matches: Vec<(&str, (u32, u32, u32), verter_analysis::MatchResult)> = Vec::new();

    for style in &analysis.styles {
        if let Some(css) = &style.css {
            for sel in &css.selectors {
                if let Some(ref structure) = sel.structure {
                    let result =
                        verter_analysis::match_selector(structure, el_idx, &template.elements);
                    if !matches!(result, verter_analysis::MatchResult::NoMatch) {
                        matches.push((&sel.text, sel.specificity, result));
                    }
                }
            }
        }
    }

    if matches.is_empty() {
        return None;
    }

    // Sort by specificity (highest first)
    matches.sort_by(|a, b| b.1.cmp(&a.1));

    let classes: Vec<&str> = element.static_classes().collect();
    let class_info = if classes.is_empty() {
        String::new()
    } else {
        format!(" class=\"{}\"", classes.join(" "))
    };
    let id_info = element
        .static_id()
        .map(|id| format!(" id=\"{id}\""))
        .unwrap_or_default();

    let mut lines = Vec::new();
    lines.push(format!(
        "**`<{}{id_info}{class_info}>`**\n\n**CSS rules ({}):**",
        element.tag,
        matches.len()
    ));

    for (text, spec, result) in &matches {
        let certainty = match result {
            verter_analysis::MatchResult::Matches => "",
            verter_analysis::MatchResult::MaybeMatches => " *(maybe)*",
            verter_analysis::MatchResult::NoMatch => unreachable!(),
        };
        lines.push(format!(
            "- `{}` — specificity `({}, {}, {})`{certainty}",
            text, spec.0, spec.1, spec.2,
        ));
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: None,
    })
}

/// When hovering on a component element tag name in the template, show prop constness info.
///
/// This helps visualize cross-file optimization: which props are always const
/// (optimizable) vs dynamic (require reactive tracking).
fn component_prop_constness_hover(
    offset: u32,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<Hover> {
    let template = analysis.template.as_ref()?;

    // Find component usage at cursor position — match only the tag name.
    // c.name is PascalCase-normalized but the source tag may be kebab-case,
    // so scan the source to find the actual tag name end.
    let comp = template.components.iter().find(|c| {
        let tag_start = c.span.start + 1; // skip '<'
                                          // Scan source to find tag name end (handles kebab-case)
        let tag_end = source
            .get(tag_start as usize..)
            .and_then(|s| {
                s.find(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                    .map(|i| tag_start + i as u32)
            })
            .unwrap_or(c.span.end);
        offset >= tag_start && offset < tag_end
    })?;

    let mut lines = Vec::new();
    let source_info = comp
        .import_source
        .as_deref()
        .map(|s| format!(" (from `{s}`)"))
        .unwrap_or_default();
    lines.push(format!("**`<{}>`**{source_info}\n", comp.name));

    if comp.props.is_empty() {
        // Return component info even without props — TSGO may also return None,
        // so this ensures the user always sees at least the component name.
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: lines.join("\n"),
            }),
            range: None,
        });
    }

    lines.push("**Props:**".to_string());

    for prop in &comp.props {
        let constness_label = match prop.constness {
            verter_analysis::template::PropValueConstness::Const => "const",
            verter_analysis::template::PropValueConstness::Dynamic => "dynamic",
            verter_analysis::template::PropValueConstness::Unknown => "unknown",
        };
        let icon = match prop.constness {
            verter_analysis::template::PropValueConstness::Const => "\u{2713}", // ✓
            verter_analysis::template::PropValueConstness::Dynamic => "\u{2197}", // ↗
            verter_analysis::template::PropValueConstness::Unknown => "?",
        };
        let bound = if prop.is_bound { ":" } else { "" };
        lines.push(format!(
            "- {icon} `{bound}{}` — *{constness_label}*",
            prop.name
        ));
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: None,
    })
}

/// Check if the offset is on a Vue API call site name, and if so return a hover
/// with Vue API context (category, sync requirement, description).
fn vue_api_hover_at_offset(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let call = analysis
        .vue_api_calls
        .iter()
        .find(|c| offset >= c.span.start && offset < c.span.end)?;

    let api = &call.api;
    let name = api.display_name();

    let mut lines = Vec::new();

    lines.push(format!("```typescript\n{name}()\n```"));

    // Category label
    let category = if api.is_lifecycle() {
        "Lifecycle Hook"
    } else if api.is_watcher() {
        "Watcher"
    } else if matches!(
        api,
        verter_analysis::VueApiClassification::Provide
            | verter_analysis::VueApiClassification::Inject
    ) {
        "Dependency Injection"
    } else if matches!(
        api,
        verter_analysis::VueApiClassification::Ref
            | verter_analysis::VueApiClassification::ShallowRef
            | verter_analysis::VueApiClassification::Reactive
            | verter_analysis::VueApiClassification::ShallowReactive
            | verter_analysis::VueApiClassification::Computed
            | verter_analysis::VueApiClassification::ToRef
            | verter_analysis::VueApiClassification::ToRefs
            | verter_analysis::VueApiClassification::Readonly
            | verter_analysis::VueApiClassification::ShallowReadonly
            | verter_analysis::VueApiClassification::CustomRef
            | verter_analysis::VueApiClassification::TriggerRef
    ) {
        "Reactivity Primitive"
    } else {
        "Vue API"
    };

    lines.push(format!("*{category}*"));

    if api.requires_sync_context() {
        lines.push("Must be called during synchronous `setup()` execution.".to_string());
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    })
}

fn hover_for_word(word: &str, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    // Check bindings
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        return Some(format_binding_hover(binding));
    }

    // Check imports
    for import in &analysis.imports {
        if let Some(binding) = import.bindings.iter().find(|b| b.name == word) {
            return Some(format_import_hover(binding, &import.source));
        }
    }

    // Check macros
    for mac in &analysis.macros {
        if mac.binding_name.as_ref().is_some_and(|name| name == word) {
            return Some(format_macro_hover(mac));
        }
    }

    None
}

fn format_binding_hover(binding: &verter_analysis::AnalyzedBinding) -> Hover {
    let mut lines = Vec::new();

    let kind_str = match binding.kind {
        verter_analysis::AnalyzedBindingKind::Const => "const",
        verter_analysis::AnalyzedBindingKind::Let => "let",
        verter_analysis::AnalyzedBindingKind::Var => "var",
        verter_analysis::AnalyzedBindingKind::Function => "function",
        verter_analysis::AnalyzedBindingKind::AsyncFunction => "async function",
        verter_analysis::AnalyzedBindingKind::Class => "class",
    };

    // Show type annotation if available
    let type_str = binding
        .type_annotation
        .as_deref()
        .map(|t| format!(": {t}"))
        .unwrap_or_default();

    lines.push(format!(
        "```typescript\n{kind_str} {}{type_str}\n```",
        binding.name
    ));

    // Show granular reactivity kind
    match binding.reactivity_kind {
        verter_analysis::ReactivityKind::None => {
            if binding.is_reactive {
                lines.push("*(reactive)*".to_string());
            }
        }
        verter_analysis::ReactivityKind::Ref => lines.push("*(ref — needs `.value`)*".to_string()),
        verter_analysis::ReactivityKind::Computed => {
            lines.push("*(computed — needs `.value`, read-only)*".to_string());
        }
        verter_analysis::ReactivityKind::Reactive => {
            lines.push("*(reactive — direct property access)*".to_string());
        }
        verter_analysis::ReactivityKind::MaybeRef => {
            lines.push("*(maybe ref — may need `.value`)*".to_string());
        }
        verter_analysis::ReactivityKind::Mutable => {
            lines.push("*(mutable — reassignable)*".to_string());
        }
    }

    if let Some(ref init) = binding.initializer {
        match init {
            verter_analysis::BindingInitializer::FunctionCall {
                callee,
                callee_import_source,
                ..
            } => {
                let source_info = callee_import_source
                    .as_ref()
                    .map(|s| format!(" (from `{s}`)"))
                    .unwrap_or_default();
                lines.push(format!("Initialized via `{callee}()`{source_info}"));
            }
            verter_analysis::BindingInitializer::Literal { kind } => {
                lines.push(format!("Literal: {kind:?}"));
            }
            verter_analysis::BindingInitializer::Reference { name } => {
                lines.push(format!("References `{name}`"));
            }
            verter_analysis::BindingInitializer::Other => {}
        }
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

fn format_import_hover(binding: &verter_analysis::AnalyzedImportBinding, source: &str) -> Hover {
    let type_prefix = if binding.is_type_only { "type " } else { "" };
    let mut lines = vec![format!(
        "```typescript\nimport {type_prefix}{{ {} }} from '{}'\n```",
        binding.name, source
    )];

    if let Some(ref api) = binding.vue_api {
        lines.push(format!("Vue API: `{api:?}`"));
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

fn format_macro_hover(mac: &verter_analysis::AnalyzedMacro) -> Hover {
    let macro_name = match mac.kind {
        verter_analysis::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_analysis::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_analysis::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_analysis::AnalyzedMacroKind::DefineExpose => "defineExpose",
        verter_analysis::AnalyzedMacroKind::DefineOptions => "defineOptions",
        verter_analysis::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_analysis::AnalyzedMacroKind::WithDefaults => "withDefaults",
    };

    let mut lines = Vec::new();

    if let Some(ref binding) = mac.binding_name {
        lines.push(format!(
            "```typescript\nconst {binding} = {macro_name}()\n```"
        ));
    } else {
        lines.push(format!("```typescript\n{macro_name}()\n```"));
    }

    if mac.is_type_based {
        let types = if mac.type_references.is_empty() {
            "inline type".to_string()
        } else {
            mac.type_references.join(", ")
        };
        lines.push(format!("Type-based: `<{types}>`"));
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

use crate::utils::word_at_offset;

#[cfg(test)]
#[path = "hover_tests.rs"]
mod hover_tests;
