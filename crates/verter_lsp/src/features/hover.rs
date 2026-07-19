// Hover — binding name, kind, source location from verter_session analysis.
// Enhanced with full resolved type signature, JSDoc from TypeProvider.

use std::collections::{HashMap, HashSet};

use tower_lsp_server::ls_types::*;
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::{
    classify_cursor, parse_opening_tag, SfcBlock, SfcCursorContext,
};
use crate::features::hover_event_tokens::{
    camelize_event_name, capitalize_first, event_directive_hover, hyphenate_event_name,
    v_model_hover, vue_event_attr_label,
};

/// Hover result from verter's own analysis, optionally carrying a Vue-specific
/// kind label (e.g., "ref", "computed") to replace the type provider's generic kind prefix.
pub struct VerterHoverResult {
    pub hover: Hover,
    /// If set, replaces the type provider's `({kind})` prefix in the merged hover.
    pub vue_kind_label: Option<String>,
    /// Typed source-token provenance for this hover. When the hover describes a
    /// Vue template syntax token whose generated TSX counterpart differs (e.g. an
    /// `@event` directive lowered to `onEvent`), this carries the structured source
    /// identity so the merge layer can rewrite a paired TypeProvider hover back to
    /// the source token — WITHOUT reparsing rendered hover markdown.
    pub source_token: Option<HoverSourceToken>,
}

/// Structured provenance for a Vue template hover whose generated TSX token differs
/// from the source token the user wrote.
///
/// This is the typed channel the merge layer reads to decide whether (and how) to
/// rewrite a TypeProvider hover label back to Vue source syntax. The label travels
/// through this typed channel, never through the rendered hover text: display text
/// is display-only and is never reparsed for a backticked `@event` label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverSourceToken {
    /// A `v-on` / `@event` directive token. `vue_attr` is the canonical Vue
    /// attribute label (`@click`, `@update:model-value`) that should replace the
    /// generated `onClick` prop label in a merged TypeProvider hover.
    EventDirective { vue_attr: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildHoverTarget {
    ComponentTag(ComponentTagHoverTarget),
    ImportBinding(ImportBindingHoverTarget),
    EventAttribute(ComponentEventHoverTarget),
    SlotAttribute(SlotAttributeHoverTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentTagHoverTarget {
    pub component_name: String,
    pub import_source: String,
    pub usage_props: Vec<ComponentUsagePropInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBindingHoverTarget {
    pub binding_name: String,
    pub import_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentEventHoverTarget {
    pub component_name: String,
    pub import_source: String,
    pub event_name: String,
    pub vue_attr: String,
}

/// A slot-name token on a component slot usage (`#header`, `v-slot:header`,
/// `#default`, kebab `#my-slot`, or arg-less `v-slot` = default) whose typed
/// hover comes from the CHILD's declared slots surface (defineSlots fields /
/// template defined slots) — never from the generated TSX, where the authored
/// name token lowers to a semantically dead string literal (D3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotAttributeHoverTarget {
    pub component_name: String,
    pub import_source: String,
    /// Authored slot argument (`my-slot` as written; `default` when arg-less).
    pub slot_name: String,
    /// Display label in the authored spelling (`#my-slot` / `v-slot:my-slot`).
    pub vue_attr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentUsagePropInfo {
    pub name: String,
    pub is_bound: bool,
    pub constness: verter_semantic::analysis::template::PropValueConstness,
}

impl From<Hover> for VerterHoverResult {
    fn from(hover: Hover) -> Self {
        Self {
            hover,
            vue_kind_label: None,
            source_token: None,
        }
    }
}

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
    ssr_context: bool,
) -> Option<VerterHoverResult> {
    let offset = line_index.position_to_offset(position)?;

    // Check SFC structural context BEFORE requiring analysis data.
    // This allows hover on tags even when analysis hasn't completed.
    match classify_cursor(offset, blocks) {
        SfcCursorContext::OpeningTag { block_index } => {
            return sfc_tag_hover(source, &blocks[block_index], offset).map(|h| h.into());
        }
        SfcCursorContext::ClosingTag { block_index } => {
            return sfc_tag_name_hover(&blocks[block_index].tag_name).map(|h| h.into());
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
        "script" => hover_in_script(offset, source, analysis, ssr_context),
        "template" => hover_in_template(offset, source, analysis, line_index),
        "style" => crate::css::css_hover(position, source, blocks, Some(analysis), line_index)
            .map(|h| h.into()),
        _ => None,
    }
}

// ── SFC Tag Hover ────────────────────────────────────────────────────────────

/// Hover on an SFC opening tag — dispatches to tag name or attribute hover.
/// Returns `None` for cursor inside `generic`/`attrs`/`attributes` attribute values
/// on script tags, letting the TypeProvider handle those via sourcemapped TSX positions.
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
                // For generic/attrs/attributes values on script tags, return None
                // to delegate to TypeProvider for TS intellisense.
                if block.tag_name == "script"
                    && matches!(attr.name.as_str(), "generic" | "attrs" | "attributes")
                {
                    return None;
                }
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

pub fn hover_text(hover: &Hover) -> String {
    match &hover.contents {
        HoverContents::Markup(markup) => markup.value.clone(),
        HoverContents::Scalar(MarkedString::String(text)) => text.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(text)) => text.value.clone(),
        HoverContents::Array(items) => items
            .iter()
            .map(|item| match item {
                MarkedString::String(text) => text.clone(),
                MarkedString::LanguageString(text) => text.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn child_hover_target_at_offset(
    offset: u32,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<ChildHoverTarget> {
    if let Some(template) = analysis.template.as_deref() {
        if let Some(target) = component_event_hover_target(offset, template, analysis) {
            return Some(ChildHoverTarget::EventAttribute(target));
        }

        if let Some(target) = slot_attribute_hover_target(offset, template, analysis) {
            return Some(ChildHoverTarget::SlotAttribute(target));
        }

        if let Some(target) = component_tag_hover_target(offset, source, template, analysis) {
            return Some(ChildHoverTarget::ComponentTag(target));
        }
    }

    direct_import_binding_hover_target(offset, analysis).map(ChildHoverTarget::ImportBinding)
}

pub fn build_child_component_hover(
    component_name: &str,
    import_source: &str,
    child_analysis: &FileAnalysisSnapshot,
    public_contract: Option<&verter_session::framework::api_projector::ComponentPublicContract>,
    public_api_code: Option<&str>,
    usage_props: &[ComponentUsagePropInfo],
) -> Hover {
    let template = child_analysis.template.as_deref();
    let handler_props = public_api_code
        .map(parse_public_api_handler_props)
        .unwrap_or_default();
    let prop_types = public_api_code
        .map(parse_public_api_props)
        .unwrap_or_default();

    let mut lines = vec![format!("**`<{component_name}>`** (from `{import_source}`)")];

    let mut prop_lines = Vec::new();
    if let Some(contract) = public_contract {
        for prop in &contract.props {
            let optional_marker = if prop.optional || prop.has_default {
                "?"
            } else {
                ""
            };
            let prop_type = prop.type_annotation.as_deref().unwrap_or("unknown");
            prop_lines.push(format!(
                "- `{}`{}: {}",
                prop.name, optional_marker, prop_type
            ));
        }
    } else if let Some(template) = template {
        if !template.prop_definitions.is_empty() {
            for prop in &template.prop_definitions {
                let prop_type = prop
                    .type_annotation
                    .clone()
                    .or_else(|| prop_types.get(&prop.name).cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let optional = !prop.is_required || prop.has_default;
                let optional_marker = if optional { "?" } else { "" };
                prop_lines.push(format!(
                    "- `{}`{}: {}",
                    prop.name, optional_marker, prop_type
                ));
            }
        }
    }
    if prop_lines.is_empty() && !prop_types.is_empty() {
        let mut props: Vec<_> = prop_types.into_iter().collect();
        props.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, prop_type) in props {
            prop_lines.push(format!("- `{name}`: {prop_type}"));
        }
    }
    if !prop_lines.is_empty() {
        lines.push(String::new());
        lines.push("**Props:**".to_string());
        lines.extend(prop_lines);
    }

    let mut emit_lines = Vec::new();
    if let Some(template) = template {
        let emit_entries: Vec<String> = template
            .emit_definitions
            .iter()
            .filter(|emit| emit.is_declared)
            .map(|emit| {
                format!(
                    "- `{}`{}",
                    emit_name_for_summary(&emit.event_name),
                    emit_summary_signature(&emit.event_name, &handler_props)
                )
            })
            .collect();
        emit_lines.extend(emit_entries);
    }
    if emit_lines.is_empty() && !handler_props.is_empty() {
        let mut seen = HashSet::new();
        let mut fallback_emits = handler_props
            .iter()
            .filter_map(|(name, signature)| {
                let vue_attr = crate::type_provider::merge::jsx_prop_to_vue_attr(name)?;
                if !vue_attr.starts_with('@') {
                    return None;
                }
                let emit_name = vue_attr.trim_start_matches('@').to_string();
                if !seen.insert(emit_name.clone()) {
                    return None;
                }
                Some(format!(
                    "- `{emit_name}`{}",
                    summarize_event_handler_signature(signature)
                ))
            })
            .collect::<Vec<_>>();
        fallback_emits.sort();
        emit_lines.extend(fallback_emits);
    }
    if !emit_lines.is_empty() {
        lines.push(String::new());
        lines.push("**Emits:**".to_string());
        lines.extend(emit_lines);
    }

    if !usage_props.is_empty() {
        lines.push(String::new());
        lines.push("**Usage:**".to_string());
        for prop in usage_props {
            let constness_label = match prop.constness {
                verter_semantic::analysis::template::PropValueConstness::Const => "const",
                verter_semantic::analysis::template::PropValueConstness::Dynamic => "dynamic",
                verter_semantic::analysis::template::PropValueConstness::Unknown => "unknown",
            };
            let icon = match prop.constness {
                verter_semantic::analysis::template::PropValueConstness::Const => "\u{2713}",
                verter_semantic::analysis::template::PropValueConstness::Dynamic => "\u{2197}",
                verter_semantic::analysis::template::PropValueConstness::Unknown => "?",
            };
            let bound = if prop.is_bound { ":" } else { "" };
            lines.push(format!(
                "- {icon} `{bound}{}` — *{constness_label}*",
                prop.name
            ));
        }
    }

    make_hover(lines.join("\n"))
}

pub fn build_child_event_hover(
    vue_attr: &str,
    child_analysis: &FileAnalysisSnapshot,
    public_api_code: Option<&str>,
) -> Option<Hover> {
    let template = child_analysis.template.as_deref()?;
    let handler_props = public_api_code
        .map(parse_public_api_handler_props)
        .unwrap_or_default();

    if let Some(prop) = template.prop_definitions.iter().find(|prop| {
        crate::type_provider::merge::jsx_prop_to_vue_attr(&prop.name).as_deref() == Some(vue_attr)
    }) {
        let signature = prop
            .type_annotation
            .clone()
            .or_else(|| {
                handler_props
                    .iter()
                    .find(|(name, _)| {
                        crate::type_provider::merge::jsx_prop_to_vue_attr(name).as_deref()
                            == Some(vue_attr)
                    })
                    .map(|(_, signature)| signature.clone())
            })
            .unwrap_or_else(|| "() => void".to_string());
        return Some(make_hover(format!(
            "```typescript\n{}{}\n```",
            vue_attr,
            normalize_event_handler_signature(&signature)
        )));
    }

    if let Some(signature) = template
        .emit_definitions
        .iter()
        .filter(|emit| emit.is_declared)
        .find_map(|emit| {
            let emit_vue_attr = vue_event_attr_label(&emit.event_name);
            if emit_vue_attr == vue_attr {
                Some(
                    handler_signature_for_event(&emit.event_name, &handler_props)
                        .unwrap_or_else(|| "() => void".to_string()),
                )
            } else {
                None
            }
        })
    {
        return Some(make_hover(format!(
            "```typescript\n{}{}\n```",
            vue_attr,
            normalize_event_handler_signature(&signature)
        )));
    }

    handler_props
        .iter()
        .find(|(name, _)| {
            crate::type_provider::merge::jsx_prop_to_vue_attr(name).as_deref() == Some(vue_attr)
        })
        .map(|(_, signature)| {
            make_hover(format!(
                "```typescript\n{}{}\n```",
                vue_attr,
                normalize_event_handler_signature(signature)
            ))
        })
}

fn hover_in_script(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    ssr_context: bool,
) -> Option<VerterHoverResult> {
    // Check if the cursor is on a Vue API call site — add context if so
    if let Some(api_hover) = vue_api_hover_at_offset(offset as u32, analysis, ssr_context) {
        return Some(api_hover.into());
    }

    let word = word_at_offset(source, offset)?;
    hover_for_word(&word, analysis)
}

fn direct_import_binding_hover_target(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
) -> Option<ImportBindingHoverTarget> {
    for import in &analysis.imports {
        if import.is_type_only {
            continue;
        }
        if let Some(binding) = import
            .bindings
            .iter()
            .find(|binding| offset >= binding.span.start && offset < binding.span.end)
        {
            return Some(ImportBindingHoverTarget {
                binding_name: binding.name.clone(),
                import_source: import.source.clone(),
            });
        }
    }
    None
}

fn hover_in_template(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<VerterHoverResult> {
    // Don't provide hover inside HTML comments
    if crate::features::definition::is_inside_html_comment(source, offset) {
        return None;
    }

    // Check if cursor is on a slot outlet element — show slot name and props
    if let Some(hover) = slot_outlet_hover(offset as u32, analysis) {
        return Some(hover.into());
    }

    // Check if cursor is on a v-slot / #name directive NAME+ARG (slot consumer).
    // Pattern positions inside the destructure are deliberately NOT answered
    // here: they map into the generated TSX and the provider supplies the
    // typed binding quickinfo (D4).
    if let Some(hover) = v_slot_hover(offset as u32, analysis) {
        return Some(hover.into());
    }

    // Slot destructure-pattern positions yield no Verter-native hover — the
    // provider answers them with the typed binding quickinfo through the
    // mapped pattern bytes (D4). This also prevents a pattern key that
    // collides with a same-named script binding from showing the WRONG
    // script-binding hover.
    if is_inside_slot_pattern(offset as u32, analysis) {
        return None;
    }

    // Check if cursor is on a template element tag name — show matching CSS rules
    if let Some(hover) = element_css_hover(offset as u32, analysis) {
        return Some(hover.into());
    }

    // Check if cursor is on a component element tag name — show prop constness info
    if let Some(hover) = component_prop_constness_hover(offset as u32, source, analysis) {
        return Some(hover.into());
    }

    // Source-owned Vue template syntax hovers. These run BEFORE the attribute-name
    // suppression below because event directives, modifiers, and the static `ref`
    // attribute are Vue syntax tokens — not script bindings. The generated TSX either
    // renames them (`@click` → `onClick`) or deletes them outright (modifiers,
    // no-value directives, static `ref`), so the TypeProvider can never describe the
    // source token. We reconstruct the hover label/range from the existing
    // `TemplateDirective` / `TemplateAttribute` spans instead.
    if let Some(hover) = event_directive_hover(offset as u32, source, analysis, line_index) {
        return Some(hover);
    }
    // Source-owned `v-model` directive-name + arg hover. Runs BEFORE the
    // attribute-name suppression: the `v-model` name and its `:show` arg are Vue
    // syntax tokens the generated TSX renames/overwrites, so the TypeProvider can
    // never describe the source token. TSGO supplies the bound prop TYPE (via the
    // mapped prop-name codegen); this hover supplies the Vue source context.
    if let Some(hover) = v_model_hover(offset as u32, source, analysis, line_index) {
        return Some(hover);
    }
    if let Some(hover) = template_ref_hover(offset as u32, source, analysis, line_index) {
        return Some(hover);
    }

    // D6: directive-NAME tokens. Built-ins get Volar-style doc hovers; custom
    // directives (`v-my-thing` → `vMyThing`) get the resolved binding's typed
    // hover. Both run BEFORE the attribute-name suppression below — the
    // generated TSX erases/lowers directive names, so the provider can never
    // describe the authored token.
    if let Some(hover) = crate::features::hover_directive_names::builtin_directive_name_hover(
        offset as u32,
        analysis,
    ) {
        return Some(hover);
    }
    if let Some(hover) =
        crate::features::hover_directive_names::custom_directive_name_hover(offset as u32, analysis)
    {
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

/// Build an LSP range from a source byte span via the document line index.
pub(super) fn span_to_range(line_index: &LineIndex, start: u32, end: u32) -> Option<Range> {
    Some(Range {
        start: line_index.offset_to_position(start)?,
        end: line_index.offset_to_position(end)?,
    })
}

/// Source-owned hover for a static template `ref` attribute (`<span ref="el">`).
///
/// The IDE codegen deletes the static `ref="..."` and reinserts an unmapped
/// synthetic `ref={"..."}`, and the generic attribute-name suppression would
/// otherwise route this through the imported Vue `ref()` symbol. We return a hover
/// describing the template-ref declaration from the existing `TemplateAttribute`
/// facts instead, without surfacing the import.
fn template_ref_hover(
    offset: u32,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<VerterHoverResult> {
    let template = analysis.template.as_deref()?;
    for el in &template.elements {
        for attr in &el.attributes {
            if attr.name != "ref" || attr.is_dynamic {
                continue;
            }
            if offset < attr.span.start || offset >= attr.span.end {
                continue;
            }
            let token = source
                .get(attr.span.start as usize..attr.span.end as usize)
                .unwrap_or("ref");
            let mut value = format!("`{token}`\n\nTemplate ref");
            match attr.value.as_deref() {
                Some(name) if !name.is_empty() => {
                    value.push_str(&format!(" — registers a reference named `{name}`."));
                }
                _ => value.push('.'),
            }
            return Some(VerterHoverResult {
                hover: Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value,
                    }),
                    range: span_to_range(line_index, attr.span.start, attr.span.end),
                },
                vue_kind_label: None,
                source_token: None,
            });
        }
    }
    None
}

/// Check if the given offset falls on an attribute name or directive name in the template.
fn is_on_attribute_name(offset: u32, analysis: &FileAnalysisSnapshot) -> bool {
    if let Some(template) = analysis.template.as_deref() {
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
    let template = analysis.template.as_deref()?;

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

/// Check if the given SFC offset is on slot-related syntax (outlet or v-slot directive).
/// The type provider returns unhelpful `() any`/`string` for these positions.
///
/// Scoped to the slot NAME+ARG region: destructure-pattern positions are
/// answered by the provider through the mapped pattern bytes (D4), so they are
/// NOT slot syntax for merge-suppression purposes.
pub fn is_on_slot_syntax(offset: u32, analysis: &FileAnalysisSnapshot) -> bool {
    let Some(template) = analysis.template.as_deref() else {
        return false;
    };

    // Slot outlets: <slot name="x" />
    if template
        .elements
        .iter()
        .any(|el| el.tag == "slot" && offset >= el.span.start && offset < el.tag_span_end)
    {
        return true;
    }

    // v-slot / #name directive NAME+ARG region (never the pattern expression)
    for el in &template.elements {
        for dir in &el.directives {
            if dir.name != "slot" {
                continue;
            }
            let (region_start, region_end) = slot_directive_name_arg_region(dir);
            if offset >= region_start && offset < region_end {
                return true;
            }
        }
    }
    false
}

/// Whether the offset sits inside a `v-slot` / `#name` destructure-pattern
/// expression span (D4 — provider-answered positions).
fn is_inside_slot_pattern(offset: u32, analysis: &FileAnalysisSnapshot) -> bool {
    let Some(template) = analysis.template.as_deref() else {
        return false;
    };
    template.elements.iter().any(|el| {
        el.directives.iter().any(|dir| {
            dir.name == "slot"
                && dir
                    .expression_span
                    .as_ref()
                    .is_some_and(|span| offset >= span.start && offset < span.end)
        })
    })
}

/// When hovering on a `<slot>` element (tag name, `name` attribute, or its value),
/// show slot outlet information: the slot name and any scoped props being passed.
///
/// Uses `DefinedSlot` from template analysis for richer binding info.
fn slot_outlet_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_deref()?;

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

    let mut lines = Vec::new();
    lines.push(format!("**`<slot>`** outlet — **\"{slot_name}\"**"));
    lines.push("Renders content provided by the parent component.".to_string());

    // Use DefinedSlot for pre-extracted binding info
    if let Some(def) = template.defined_slots.iter().find(|s| s.name == slot_name) {
        if def.has_bindings && !def.binding_names.is_empty() {
            let props: Vec<String> = def
                .binding_names
                .iter()
                .zip(def.binding_expressions.iter())
                .map(|(name, expr)| format!(":{name}=\"{expr}\""))
                .collect();
            lines.push(format!("\n**Scoped props:** {}", props.join(", ")));
        }
    }

    Some(make_hover(lines.join("\n\n")))
}

/// When hovering on a `v-slot` / `#name` directive NAME+ARG, show slot content
/// information. This is the fallback surface for slots whose child component
/// cannot be resolved; a resolvable child is answered by the typed
/// [`build_child_slot_hover`] from the child's declared slots surface (D3).
/// Destructure-pattern positions are excluded — the provider answers them
/// with typed binding quickinfo through the mapped pattern bytes (D4).
fn v_slot_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_deref()?;

    for el in &template.elements {
        for dir in &el.directives {
            if dir.name != "slot" {
                continue;
            }
            let (region_start, region_end) = slot_directive_name_arg_region(dir);
            if offset < region_start || offset >= region_end {
                continue;
            }

            let slot_name = dir.argument.as_deref().unwrap_or("default");
            let mut lines = Vec::new();

            let syntax = if dir.raw_name.starts_with('#') {
                format!("`#{slot_name}`")
            } else if dir.argument.is_some() {
                format!("`v-slot:{slot_name}`")
            } else {
                "`v-slot`".to_string()
            };
            lines.push(format!("**Slot content** — {syntax}"));
            lines.push(format!(
                "Provides content for the **\"{slot_name}\"** slot."
            ));

            if let Some(ref expr) = dir.expression {
                lines.push(format!("\n**Scoped params:** `{expr}`"));
            }

            return Some(make_hover(lines.join("\n\n")));
        }
    }
    None
}

/// When hovering on a template element tag name, show matching CSS rules with specificity.
fn element_css_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_deref()?;

    // Find element at cursor position — match only the tag name, not the full element span.
    // Element span starts at '<', so tag name starts at span.start + 1.
    let (el_idx, element) = template.elements.iter().enumerate().find(|(_, el)| {
        let tag_start = el.span.start + 1; // skip '<'
        let tag_end = tag_start + el.tag.len() as u32;
        offset >= tag_start && offset < tag_end
    })?;

    // Collect matching selectors from all style blocks
    let mut matches: Vec<(
        &str,
        (u32, u32, u32),
        verter_semantic::analysis::MatchResult,
    )> = Vec::new();

    for style in analysis.styles.iter() {
        if let Some(css) = &style.css {
            for sel in &css.selectors {
                if let Some(ref structure) = sel.structure {
                    let result = verter_semantic::analysis::match_selector(
                        structure,
                        el_idx,
                        &template.elements,
                    );
                    if !matches!(result, verter_semantic::analysis::MatchResult::NoMatch) {
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
    matches.sort_by_key(|m| std::cmp::Reverse(m.1));

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
            verter_semantic::analysis::MatchResult::Matches => "",
            verter_semantic::analysis::MatchResult::MaybeMatches => " *(maybe)*",
            verter_semantic::analysis::MatchResult::NoMatch => unreachable!(),
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
    let template = analysis.template.as_deref()?;

    // Find component usage at cursor position — match only the tag name.
    // c.name is PascalCase-normalized but the source tag may be kebab-case,
    // so scan the source to find the actual tag name end.
    let comp = find_component_usage_at_tag_offset(offset, source, template)?;

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
            verter_semantic::analysis::template::PropValueConstness::Const => "const",
            verter_semantic::analysis::template::PropValueConstness::Dynamic => "dynamic",
            verter_semantic::analysis::template::PropValueConstness::Unknown => "unknown",
        };
        let icon = match prop.constness {
            verter_semantic::analysis::template::PropValueConstness::Const => "\u{2713}", // ✓
            verter_semantic::analysis::template::PropValueConstness::Dynamic => "\u{2197}", // ↗
            verter_semantic::analysis::template::PropValueConstness::Unknown => "?",
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

fn component_tag_hover_target(
    offset: u32,
    source: &str,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    analysis: &FileAnalysisSnapshot,
) -> Option<ComponentTagHoverTarget> {
    let comp = find_component_usage_at_tag_offset(offset, source, template)?;
    let import_source = component_import_source(comp, analysis)?;
    let usage_props = comp
        .props
        .iter()
        .map(|prop| ComponentUsagePropInfo {
            name: prop.name.clone(),
            is_bound: prop.is_bound,
            constness: prop.constness,
        })
        .collect();

    Some(ComponentTagHoverTarget {
        component_name: comp.name.clone(),
        import_source,
        usage_props,
    })
}

fn component_event_hover_target(
    offset: u32,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    analysis: &FileAnalysisSnapshot,
) -> Option<ComponentEventHoverTarget> {
    for el in &template.elements {
        if !el.is_component {
            continue;
        }
        for dir in &el.directives {
            if dir.name != "on" {
                continue;
            }
            let Some(arg_span) = dir.arg_span.as_ref() else {
                continue;
            };
            let hover_start = dir.span.start.saturating_sub(1);
            let hover_end = arg_span.end;
            if offset < hover_start || offset >= hover_end {
                continue;
            }
            let component = template.components.iter().find(|component| {
                component.span.start == el.span.start && component.span.end == el.span.end
            })?;
            let import_source = component_import_source(component, analysis)?;
            let event_name = dir.argument.clone()?;
            return Some(ComponentEventHoverTarget {
                component_name: component.name.clone(),
                import_source,
                vue_attr: vue_event_attr_label(&event_name),
                event_name,
            });
        }
    }
    None
}

/// The slot-NAME region of a `v-slot` / `#name` directive: the directive name
/// plus its argument, EXCLUDING the destructure-pattern expression. Pattern
/// positions map into the generated TSX (the pattern is emitted as verbatim
/// mapped bytes inside the slot IIFE) and are answered by the provider's typed
/// binding quickinfo (D4); only the name/arg region is Verter-owned (D3).
fn slot_directive_name_arg_region(
    dir: &verter_semantic::analysis::template::TemplateDirective,
) -> (u32, u32) {
    let end = dir
        .arg_span
        .as_ref()
        .map(|span| span.end)
        .unwrap_or(dir.name_end);
    (dir.span.start, end)
}

/// Identify a slot-name token (`#header`, `v-slot:header`, `#default`, kebab
/// `#my-slot`, arg-less `v-slot`) on a component slot usage, resolving the
/// owning child component — the element itself for `<MyComp #header>`, or the
/// parent component element for `<template #header>`.
fn slot_attribute_hover_target(
    offset: u32,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    analysis: &FileAnalysisSnapshot,
) -> Option<SlotAttributeHoverTarget> {
    for el in &template.elements {
        for dir in &el.directives {
            if dir.name != "slot" {
                continue;
            }
            let (region_start, region_end) = slot_directive_name_arg_region(dir);
            if offset < region_start || offset >= region_end {
                continue;
            }
            let component = if el.is_component {
                template.components.iter().find(|component| {
                    component.span.start == el.span.start && component.span.end == el.span.end
                })?
            } else if el.tag == "template" {
                let parent = el
                    .parent_index
                    .and_then(|idx| template.elements.get(idx as usize))?;
                if !parent.is_component {
                    continue;
                }
                template.components.iter().find(|component| {
                    component.name == parent.tag
                        || component.name == crate::server::to_pascal_case(&parent.tag)
                })?
            } else {
                continue;
            };
            let import_source = component_import_source(component, analysis)?;
            let slot_name = dir
                .argument
                .clone()
                .unwrap_or_else(|| "default".to_string());
            let vue_attr = if dir.raw_name.starts_with('#') {
                format!("#{slot_name}")
            } else if dir.argument.is_some() {
                format!("v-slot:{slot_name}")
            } else {
                "v-slot".to_string()
            };
            return Some(SlotAttributeHoverTarget {
                component_name: component.name.clone(),
                import_source,
                slot_name,
                vue_attr,
            });
        }
    }
    None
}

/// Build the typed slot-name hover from the CHILD's declared slots surface
/// (D3): the slot's defineSlots signature — name, slot-props payload, return
/// type — resolved with Vue's kebab↔camel equivalence (`#my-slot` → `mySlot`).
/// Falls back to the child's template-defined slot names (untyped). A slot the
/// child never declared yields `None` (fail-closed — no fabrication).
pub fn build_child_slot_hover(
    vue_attr: &str,
    slot_name: &str,
    child_analysis: &FileAnalysisSnapshot,
) -> Option<Hover> {
    let best = crate::server::select_best_ranked_candidate(
        child_analysis
            .macros
            .iter()
            .filter(|mac| mac.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots)
            .flat_map(|mac| mac.slot_fields.iter())
            .filter_map(|slot_field| {
                crate::server::attr_name_match_rank(slot_name, &slot_field.name)
                    .map(|rank| (rank, slot_field.span, slot_field))
            }),
    );
    if let Some((_, _, slot_field)) = best {
        let props = if slot_field.bindings.is_empty() {
            String::new()
        } else {
            let bindings = slot_field
                .bindings
                .iter()
                .map(|binding| {
                    format!(
                        "{}: {}",
                        binding.name,
                        binding.type_annotation.as_deref().unwrap_or("unknown")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!("(props: {{ {bindings} }})")
        };
        let return_type = slot_field.return_type.as_deref().unwrap_or("any");
        let mut value = format!(
            "```typescript\n(slot) {}{props}: {return_type}\n```",
            slot_field.name
        );
        if let Some(description) = &slot_field.description {
            value.push_str("\n\n");
            value.push_str(description);
        }
        return Some(make_hover(value));
    }

    if let Some(child_template) = child_analysis.template.as_deref() {
        let best = crate::server::select_best_ranked_candidate(
            child_template
                .defined_slots
                .iter()
                .filter_map(|defined_slot| {
                    crate::server::attr_name_match_rank(slot_name, &defined_slot.name)
                        .map(|rank| (rank, defined_slot.span, defined_slot))
                }),
        );
        if let Some((_, _, defined_slot)) = best {
            let mut lines = vec![format!("**Slot** `{vue_attr}`")];
            if !defined_slot.binding_names.is_empty() {
                lines.push(format!(
                    "**Scoped props:** {}",
                    defined_slot
                        .binding_names
                        .iter()
                        .map(|name| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            return Some(make_hover(lines.join("\n\n")));
        }
    }

    None
}

/// Resolve the authored module source for a template component.
///
/// The template analyzer normally stamps `import_source` directly. During an
/// editor/provider resync, however, a newly rebuilt template snapshot can
/// temporarily retain the component usage while that optional convenience
/// field is absent. The script import inventory remains authoritative and is
/// enough to recover the same source by the component's local binding. Global
/// components and unresolved tags still fail closed because they have no
/// matching value import.
fn component_import_source(
    component: &verter_semantic::analysis::template::TemplateComponentUsage,
    analysis: &FileAnalysisSnapshot,
) -> Option<String> {
    component.import_source.clone().or_else(|| {
        analysis
            .imports
            .iter()
            .filter(|import| !import.is_type_only)
            .find(|import| {
                import
                    .bindings
                    .iter()
                    .any(|binding| !binding.is_type_only && binding.name == component.name)
            })
            .map(|import| import.source.clone())
    })
}

fn find_component_usage_at_tag_offset<'a>(
    offset: u32,
    source: &str,
    template: &'a verter_semantic::analysis::template::TemplateAnalysisSnapshot,
) -> Option<&'a verter_semantic::analysis::template::TemplateComponentUsage> {
    template.components.iter().find(|component| {
        let tag_start = component.span.start + 1;
        let tag_end = source
            .get(tag_start as usize..)
            .and_then(|slice| {
                slice
                    .find(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                    .map(|idx| tag_start + idx as u32)
            })
            .unwrap_or(component.span.end);
        offset >= tag_start && offset < tag_end
    })
}

/// Check if the offset is on a Vue API call site name, and if so return a hover
/// with Vue API context (category, sync requirement, description).
/// Client-only lifecycle hooks that never fire during SSR.
const CLIENT_ONLY_HOOKS: &[verter_semantic::analysis::VueApiClassification] = &[
    verter_semantic::analysis::VueApiClassification::OnMounted,
    verter_semantic::analysis::VueApiClassification::OnUpdated,
    verter_semantic::analysis::VueApiClassification::OnActivated,
    verter_semantic::analysis::VueApiClassification::OnDeactivated,
    verter_semantic::analysis::VueApiClassification::OnBeforeUpdate,
    verter_semantic::analysis::VueApiClassification::OnBeforeMount,
];

fn vue_api_hover_at_offset(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
    ssr_context: bool,
) -> Option<Hover> {
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
        verter_semantic::analysis::VueApiClassification::Provide
            | verter_semantic::analysis::VueApiClassification::Inject
    ) {
        "Dependency Injection"
    } else if matches!(
        api,
        verter_semantic::analysis::VueApiClassification::Ref
            | verter_semantic::analysis::VueApiClassification::ShallowRef
            | verter_semantic::analysis::VueApiClassification::Reactive
            | verter_semantic::analysis::VueApiClassification::ShallowReactive
            | verter_semantic::analysis::VueApiClassification::Computed
            | verter_semantic::analysis::VueApiClassification::ToRef
            | verter_semantic::analysis::VueApiClassification::ToRefs
            | verter_semantic::analysis::VueApiClassification::Readonly
            | verter_semantic::analysis::VueApiClassification::ShallowReadonly
            | verter_semantic::analysis::VueApiClassification::CustomRef
            | verter_semantic::analysis::VueApiClassification::TriggerRef
    ) {
        "Reactivity Primitive"
    } else {
        "Vue API"
    };

    lines.push(format!("*{category}*"));

    if api.requires_sync_context() {
        lines.push("Must be called during synchronous `setup()` execution.".to_string());
    }

    // SSR warning for client-only hooks
    if ssr_context && CLIENT_ONLY_HOOKS.contains(api) {
        lines.push(
            "**⚠ SSR Warning:** This hook does not fire during server-side rendering. \
             Move DOM-dependent logic here, or use `onServerPrefetch()` for data fetching."
                .to_string(),
        );
    }

    // SSR note for useTemplateRef
    if ssr_context
        && matches!(
            api,
            verter_semantic::analysis::VueApiClassification::UseTemplateRef
        )
    {
        lines.push(
            "**⚠ SSR Warning:** Template refs are `null` during SSR. \
             Access `.value` inside `onMounted()` or guard with `import.meta.client`."
                .to_string(),
        );
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    })
}

pub(super) fn hover_for_word(
    word: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<VerterHoverResult> {
    // Check bindings
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        let vue_kind_label = reactivity_kind_label(binding);
        return Some(VerterHoverResult {
            hover: format_binding_hover(binding),
            vue_kind_label,
            source_token: None,
        });
    }

    // Check imports
    for import in &analysis.imports {
        if let Some(binding) = import.bindings.iter().find(|b| b.name == word) {
            return Some(format_import_hover(binding, &import.source).into());
        }
    }

    // Check macros
    for mac in analysis.macros.iter() {
        if mac.binding_name.as_ref().is_some_and(|name| name == word) {
            return Some(format_macro_hover(mac).into());
        }
    }

    None
}

/// Map a binding's reactivity kind to a label for the hover kind prefix.
fn reactivity_kind_label(binding: &verter_semantic::analysis::AnalyzedBinding) -> Option<String> {
    match binding.reactivity_kind {
        verter_semantic::analysis::ReactivityKind::Ref => Some("ref".to_string()),
        verter_semantic::analysis::ReactivityKind::Computed => Some("computed".to_string()),
        verter_semantic::analysis::ReactivityKind::Reactive => Some("reactive".to_string()),
        verter_semantic::analysis::ReactivityKind::MaybeRef => Some("maybe ref".to_string()),
        verter_semantic::analysis::ReactivityKind::Mutable => Some("mutable".to_string()),
        verter_semantic::analysis::ReactivityKind::None => {
            if binding.is_reactive {
                Some("reactive".to_string())
            } else {
                None
            }
        }
    }
}

fn format_binding_hover(binding: &verter_semantic::analysis::AnalyzedBinding) -> Hover {
    let mut lines = Vec::new();

    let kind_str = match binding.kind {
        verter_semantic::analysis::AnalyzedBindingKind::Const => "const",
        verter_semantic::analysis::AnalyzedBindingKind::Let => "let",
        verter_semantic::analysis::AnalyzedBindingKind::Var => "var",
        verter_semantic::analysis::AnalyzedBindingKind::Function => "function",
        verter_semantic::analysis::AnalyzedBindingKind::AsyncFunction => "async function",
        verter_semantic::analysis::AnalyzedBindingKind::Class => "class",
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
        verter_semantic::analysis::ReactivityKind::None => {
            if binding.is_reactive {
                lines.push("*(reactive)*".to_string());
            }
        }
        verter_semantic::analysis::ReactivityKind::Ref => {
            lines.push("*(ref — needs `.value`)*".to_string())
        }
        verter_semantic::analysis::ReactivityKind::Computed => {
            lines.push("*(computed — needs `.value`, read-only)*".to_string());
        }
        verter_semantic::analysis::ReactivityKind::Reactive => {
            lines.push("*(reactive — direct property access)*".to_string());
        }
        verter_semantic::analysis::ReactivityKind::MaybeRef => {
            lines.push("*(maybe ref — may need `.value`)*".to_string());
        }
        verter_semantic::analysis::ReactivityKind::Mutable => {
            lines.push("*(mutable — reassignable)*".to_string());
        }
    }

    if let Some(ref init) = binding.initializer {
        match init {
            verter_semantic::analysis::BindingInitializer::FunctionCall {
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
            verter_semantic::analysis::BindingInitializer::Literal { kind } => {
                lines.push(format!("Literal: {kind:?}"));
            }
            verter_semantic::analysis::BindingInitializer::Reference { name } => {
                lines.push(format!("References `{name}`"));
            }
            verter_semantic::analysis::BindingInitializer::Other => {}
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

fn format_import_hover(
    binding: &verter_semantic::analysis::AnalyzedImportBinding,
    source: &str,
) -> Hover {
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

fn format_macro_hover(mac: &verter_semantic::analysis::AnalyzedMacro) -> Hover {
    let macro_name = match mac.kind {
        verter_semantic::analysis::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_semantic::analysis::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_semantic::analysis::AnalyzedMacroKind::DefineExpose => "defineExpose",
        verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => "defineOptions",
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_semantic::analysis::AnalyzedMacroKind::WithDefaults => "withDefaults",
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

fn parse_public_api_handler_props(code: &str) -> HashMap<String, String> {
    parse_public_api_fields(code)
        .into_iter()
        .filter_map(|(name, value)| {
            if name.starts_with("on") && value.contains("=>") {
                Some((name, value))
            } else {
                None
            }
        })
        .collect()
}

fn parse_public_api_props(code: &str) -> HashMap<String, String> {
    parse_public_api_fields(code)
        .into_iter()
        .filter_map(|(name, value)| {
            if name.starts_with("on") || name.starts_with('$') {
                return None;
            }
            Some((name, value))
        })
        .collect()
}

fn parse_public_api_fields(code: &str) -> Vec<(String, String)> {
    let Some(props_start) = code.find("$props:") else {
        return Vec::new();
    };
    let props_slice = &code[props_start + "$props:".len()..];
    let mut fields = Vec::new();
    let mut brace_cursor = 0usize;
    while let Some(rel) = props_slice[brace_cursor..].find('{') {
        let open = brace_cursor + rel;
        let Some(close) = find_matching_delimiter(props_slice, open, '{', '}') else {
            break;
        };
        let block = &props_slice[open + 1..close];
        fields.extend(parse_type_literal_fields(block));
        brace_cursor = close + 1;
    }
    fields
}

fn parse_type_literal_fields(block: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let bytes = block.as_bytes();
    let mut start = 0usize;
    let mut depth_paren = 0u32;
    let mut depth_bracket = 0u32;
    let mut depth_brace = 0u32;
    let mut idx = 0usize;

    while idx < bytes.len() {
        match bytes[idx] {
            b'(' => depth_paren += 1,
            b')' => depth_paren = depth_paren.saturating_sub(1),
            b'[' => depth_bracket += 1,
            b']' => depth_bracket = depth_bracket.saturating_sub(1),
            b'{' => depth_brace += 1,
            b'}' => depth_brace = depth_brace.saturating_sub(1),
            b';' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                if let Some(field) = parse_field_entry(block[start..idx].trim()) {
                    fields.push(field);
                }
                start = idx + 1;
            }
            _ => {}
        }
        idx += 1;
    }

    if let Some(field) = parse_field_entry(block[start..].trim()) {
        fields.push(field);
    }

    fields
}

fn parse_field_entry(field: &str) -> Option<(String, String)> {
    let trimmed = field.trim();
    if trimmed.is_empty() || trimmed.starts_with("/**") {
        return None;
    }

    let separator = find_field_separator(trimmed)?;
    let raw_name = trimmed[..separator].trim();
    let raw_value = trimmed[separator + 1..].trim();
    let raw_name = raw_name.trim_end_matches('?').trim();
    let name = raw_name
        .strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
        .unwrap_or(raw_name)
        .trim()
        .to_string();
    if name.is_empty() || raw_value.is_empty() {
        return None;
    }

    Some((name, raw_value.to_string()))
}

fn find_field_separator(field: &str) -> Option<usize> {
    let mut in_string = false;
    for (idx, ch) in field.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            ':' if !in_string => return Some(idx),
            _ => {}
        }
    }
    None
}

fn find_matching_delimiter(text: &str, open_idx: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0u32;
    for (idx, ch) in text.char_indices().skip(open_idx) {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

fn handler_signature_for_event(
    event_name: &str,
    handler_props: &HashMap<String, String>,
) -> Option<String> {
    let vue_attr = vue_event_attr_label(event_name);
    emit_handler_keys(event_name)
        .into_iter()
        .find_map(|key| handler_props.get(&key).cloned())
        .or_else(|| {
            handler_props
                .iter()
                .find(|(name, _)| {
                    crate::type_provider::merge::jsx_prop_to_vue_attr(name).as_deref()
                        == Some(vue_attr.as_str())
                })
                .map(|(_, signature)| signature.clone())
        })
}

fn emit_summary_signature(event_name: &str, handler_props: &HashMap<String, String>) -> String {
    handler_signature_for_event(event_name, handler_props)
        .map(|signature| summarize_event_handler_signature(&signature))
        .unwrap_or_else(|| "()".to_string())
}

fn emit_name_for_summary(event_name: &str) -> String {
    vue_event_attr_label(event_name)
        .trim_start_matches('@')
        .to_string()
}

fn normalize_event_handler_signature(signature: &str) -> String {
    if let Some(tuple_params) = tuple_payload_params(signature) {
        return format!("({tuple_params}) => void");
    }
    signature.trim().to_string()
}

fn summarize_event_handler_signature(signature: &str) -> String {
    if let Some(tuple_params) = tuple_payload_params(signature) {
        return format!("({tuple_params})");
    }
    if let Some(params) = parameter_list(signature) {
        return format!("({params})");
    }
    "()".to_string()
}

fn tuple_payload_params(signature: &str) -> Option<String> {
    let trimmed = signature.trim();
    let start = trimmed.strip_prefix("(...args: [")?;
    let end = start.find("]) =>")?;
    Some(start[..end].trim().to_string())
}

fn parameter_list(signature: &str) -> Option<String> {
    let trimmed = signature.trim();
    let params = trimmed.strip_prefix('(')?;
    let end = params.find(')')?;
    Some(params[..end].trim().to_string())
}

fn emit_handler_keys(event_name: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let canonical = format!("on{}", capitalize_first(event_name));
    keys.push(canonical.clone());

    if !event_name.contains(':') {
        let camel = format!("on{}", capitalize_first(&camelize_event_name(event_name)));
        if camel != canonical {
            keys.push(camel);
        }

        let kebab = format!("on{}", capitalize_first(&hyphenate_event_name(event_name)));
        if kebab != canonical && !keys.iter().any(|key| key == &kebab) {
            keys.push(kebab);
        }
    }

    keys
}

use crate::utils::word_at_offset;

#[cfg(test)]
#[path = "hover_tests.rs"]
mod hover_tests;
