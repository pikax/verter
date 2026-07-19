// Completion — template bindings, component names, props from verter_session analysis.
// Enhanced with typed member access, generic inference from TypeProvider.
// AST-based cursor context detection via cursor_context module.

use tower_lsp_server::ls_types::*;
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::{parse_opening_tag, SfcBlock};
use crate::features::cursor_context::{
    classify_cursor_context_for_language, CarrierTemplateLanguage, CursorContext, ExpressionKind,
    StyleCursorContext, TemplateCursorContext,
};

/// Result from completion, including an `is_incomplete` flag for re-query behavior.
pub struct CompletionResult {
    pub items: Vec<CompletionItem>,
    pub is_incomplete: bool,
}

/// A workspace component available for auto-import.
pub struct WorkspaceComponent {
    /// PascalCase component name (derived from filename).
    pub name: String,
    /// Relative or absolute import path (e.g., `./Button.vue`).
    pub import_path: String,
}

/// Provide completions at a given position using AST-based cursor context detection.
///
/// Strategy:
/// 1. Classify cursor context using TemplateAnalysisSnapshot AST data
/// 2. Route to appropriate completion provider based on context
/// 3. For expression contexts, TypeProvider supplements/replaces verter completions
///
/// The optional `resolve_component` callback takes an import source (e.g., `./Button.vue`)
/// and an optional component name (for barrel re-export following), returning that
/// component's analysis snapshot for cross-file prop completions.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn completions_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    resolve_component: Option<&dyn Fn(&str, Option<&str>) -> Option<FileAnalysisSnapshot>>,
    workspace_components: Option<&[WorkspaceComponent]>,
    doc_uri: Option<&str>,
    ssr_context: bool,
) -> Option<CompletionResult> {
    let offset = line_index.position_to_offset(position)?;
    let carrier_language = doc_uri.and_then(CarrierTemplateLanguage::from_uri);

    // Classify cursor using AST-based context detection
    let context =
        classify_cursor_context_for_language(offset, source, blocks, analysis, carrier_language);

    match context {
        CursorContext::RootLevel => {
            // Vue SFC scaffold snippets are invalid at Svelte's markup root.
            // Until there is a Svelte-native root producer, fail closed and let
            // the provider/editor supply ordinary markup completions.
            if carrier_language == Some(CarrierTemplateLanguage::Svelte) {
                None
            } else {
                Some(CompletionResult {
                    items: sfc_root_completions(source, blocks, offset, line_index),
                    is_incomplete: false,
                })
            }
        }
        CursorContext::BlockOpeningTag { ref tag_name } => {
            // When cursor is inside a `generic`, `attrs`, or `attributes` attribute
            // value on a <script> tag, return None to let the TypeProvider handle
            // completions via sourcemapped TSX positions.
            if tag_name == "script" {
                if let Some(block) = blocks.iter().find(|b| b.tag_name == "script") {
                    let parsed = parse_opening_tag(source, block);
                    let is_in_ts_attr_value = parsed.attrs.iter().any(|a| {
                        matches!(a.name.as_str(), "generic" | "attrs" | "attributes")
                            && a.value_start.is_some_and(|vs| offset >= vs)
                            && a.value_end.is_some_and(|ve| offset <= ve)
                    });
                    if is_in_ts_attr_value {
                        return None;
                    }
                }
            }
            let block = blocks
                .iter()
                .find(|b| b.tag_name.as_str() == tag_name.as_str())?;
            Some(CompletionResult {
                items: sfc_attribute_completions(source, block),
                is_incomplete: false,
            })
        }
        CursorContext::BlockClosingTag => None,
        CursorContext::Script => {
            let analysis = analysis?;
            let mut items = script_completions(analysis);
            if ssr_context {
                items.extend(ssr_script_completions());
            }
            Some(CompletionResult {
                items,
                is_incomplete: false,
            })
        }
        CursorContext::Style(StyleCursorContext::VBind) => {
            // Style v-bind: offer reactive bindings via css completions
            crate::css::css_completions(position, source, blocks, analysis, line_index).map(
                |items| CompletionResult {
                    items,
                    is_incomplete: false,
                },
            )
        }
        CursorContext::Style(StyleCursorContext::General) => crate::css::css_completions(
            position, source, blocks, analysis, line_index,
        )
        .map(|items| CompletionResult {
            items,
            is_incomplete: false,
        }),
        CursorContext::CustomBlock { .. } => None,
        CursorContext::Template(tc) => {
            let analysis = analysis?;
            match tc {
                TemplateCursorContext::TagName { .. } => {
                    let mut items = tag_name_completions(analysis, workspace_components, doc_uri);
                    if ssr_context {
                        items.extend(ssr_tag_name_completions());
                    }
                    Some(CompletionResult {
                        items,
                        is_incomplete: false,
                    })
                }
                TemplateCursorContext::ClosingTagName { .. } => None,
                TemplateCursorContext::AttributeName {
                    tag_name: _,
                    is_component,
                    ref existing_attrs,
                } => {
                    // For components, try to offer prop/event completions
                    if is_component {
                        let comp_offset = offset as usize;
                        if let Some(items) = component_prop_completions(
                            comp_offset,
                            source,
                            analysis,
                            resolve_component,
                            existing_attrs,
                            carrier_language,
                        ) {
                            return Some(CompletionResult {
                                items,
                                is_incomplete: false,
                            });
                        }
                    }
                    // The generic attribute table below is Vue syntax. Until a
                    // Svelte-native attribute producer exists, unresolved and
                    // zero-public-prop Svelte elements fail closed instead of
                    // leaking `v-*`, `v-model`, and `@click` suggestions.
                    if carrier_language == Some(CarrierTemplateLanguage::Svelte) {
                        return None;
                    }
                    Some(CompletionResult {
                        items: attribute_name_completions(),
                        is_incomplete: false,
                    })
                }
                TemplateCursorContext::EventModifier { ref event_name, .. } => {
                    Some(event_modifier_completions_for(event_name))
                }
                TemplateCursorContext::VModelModifier { .. } => Some(vmodel_modifier_completions()),
                TemplateCursorContext::DirectiveArgument { .. } => None,
                TemplateCursorContext::SlotName {
                    ref tag_name,
                    is_component,
                } => slot_name_completions(
                    offset as usize,
                    source,
                    analysis,
                    resolve_component,
                    tag_name,
                    is_component,
                )
                .map(|items| CompletionResult {
                    items,
                    is_incomplete: false,
                }),
                TemplateCursorContext::SvelteSnippetName { ref tag_name } => {
                    svelte_snippet_slot_completions(
                        offset as usize,
                        source,
                        analysis,
                        resolve_component,
                        tag_name,
                    )
                    .map(|items| CompletionResult {
                        items,
                        is_incomplete: false,
                    })
                }
                TemplateCursorContext::SvelteRenderCallee => {
                    svelte_render_callee_completions(analysis).map(|items| CompletionResult {
                        items,
                        is_incomplete: false,
                    })
                }
                TemplateCursorContext::Expression { ref kind } => {
                    // Check if class attribute expression — offer CSS class completions
                    if matches!(
                        kind,
                        ExpressionKind::Prop { ref prop_name } if prop_name == "class"
                    ) {
                        if let Some(result) =
                            class_attribute_completions(offset as usize, source, analysis)
                        {
                            return Some(result);
                        }
                    }
                    Some(CompletionResult {
                        items: template_completions(
                            analysis,
                            workspace_components,
                            doc_uri,
                            Some(offset),
                        ),
                        is_incomplete: false,
                    })
                }
                TemplateCursorContext::Interpolation => Some(CompletionResult {
                    items: template_completions(
                        analysis,
                        workspace_components,
                        doc_uri,
                        Some(offset),
                    ),
                    is_incomplete: false,
                }),
                TemplateCursorContext::StaticValue { ref attr_name } => {
                    if attr_name == "class" {
                        class_attribute_completions(offset as usize, source, analysis)
                    } else {
                        None
                    }
                }
                TemplateCursorContext::TextContent => None,
            }
        }
    }
}

// =============================================================================
// SFC Root & Attribute Completions
// =============================================================================

/// Completions at root level (outside all blocks): tag snippets + scaffold snippets.
///
/// Every snippet emitted here begins with `<`. To avoid a doubled `<` when the
/// user has already typed a leading `<` (e.g. `<script|`), each snippet carries an
/// explicit `<`-anchored `text_edit` computed from the cursor `offset`. See
/// [`snippet_item`] for the replace-range walk-back rule.
fn sfc_root_completions(
    source: &str,
    blocks: &[SfcBlock],
    offset: u32,
    line_index: &LineIndex,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let has_template = blocks.iter().any(|b| b.tag_name == "template");
    let has_script = blocks.iter().any(|b| b.tag_name == "script");
    let has_style = blocks.iter().any(|b| b.tag_name == "style");
    let is_empty = source.trim().is_empty();

    // Tag snippets (filtered by existing blocks)
    if !has_template {
        items.push(snippet_item(
            "template",
            "<template>\n\t$0\n</template>",
            "Add <template> block",
            1,
            source,
            offset,
            line_index,
        ));
    }
    if !has_script {
        items.push(snippet_item(
            "script setup",
            "<script setup lang=\"ts\">\n$0\n</script>",
            "Add <script setup> block",
            2,
            source,
            offset,
            line_index,
        ));
        items.push(snippet_item(
            "script",
            "<script lang=\"ts\">\n$0\n</script>",
            "Add <script> block",
            3,
            source,
            offset,
            line_index,
        ));
    }
    if !has_style {
        items.push(snippet_item(
            "style scoped",
            "<style scoped>\n$0\n</style>",
            "Add <style scoped> block",
            4,
            source,
            offset,
            line_index,
        ));
        items.push(snippet_item(
            "style",
            "<style>\n$0\n</style>",
            "Add <style> block",
            5,
            source,
            offset,
            line_index,
        ));
    }

    // Scaffold snippets (only when file is mostly empty)
    if is_empty {
        items.push(snippet_item(
            "vue-ts",
            "<script setup lang=\"ts\">\n$0\n</script>\n\n<template>\n\t\n</template>",
            "Vue SFC scaffold (TypeScript)",
            0,
            source,
            offset,
            line_index,
        ));
        items.push(snippet_item(
            "vue",
            "<script setup>\n$0\n</script>\n\n<template>\n\t\n</template>",
            "Vue SFC scaffold (JavaScript)",
            0,
            source,
            offset,
            line_index,
        ));
        items.push(snippet_item(
            "vue-options",
            "<script lang=\"ts\">\nimport { defineComponent } from 'vue'\n\nexport default defineComponent({\n\t$0\n})\n</script>\n\n<template>\n\t\n</template>",
            "Vue SFC scaffold (Options API)",
            0,
            source,
            offset,
            line_index,
        ));
    }

    // Custom block snippets
    items.push(snippet_item(
        "i18n",
        "<i18n lang=\"${1:json}\">\n$0\n</i18n>",
        "Add <i18n> block",
        6,
        source,
        offset,
        line_index,
    ));

    items
}

/// Completions inside an SFC opening tag: context-sensitive attributes.
fn sfc_attribute_completions(source: &str, block: &SfcBlock) -> Vec<CompletionItem> {
    let ctx = parse_opening_tag(source, block);
    let existing: Vec<&str> = ctx.attrs.iter().map(|a| a.name.as_str()).collect();
    let mut items = Vec::new();

    match ctx.tag_name.as_str() {
        "script" => {
            if !existing.contains(&"setup") {
                items.push(attr_item("setup", None, "Enable <script setup> syntax"));
            }
            if !existing.contains(&"lang") {
                items.push(attr_item(
                    "lang",
                    Some("\"${1:ts}\""),
                    "Script language (ts, tsx, jsx)",
                ));
            }
            if !existing.contains(&"generic") && existing.contains(&"setup") {
                items.push(attr_item(
                    "generic",
                    Some("\"$1\""),
                    "Generic type parameters",
                ));
            }
            if !existing.contains(&"attrs") && !existing.contains(&"attributes") {
                items.push(attr_item(
                    "attrs",
                    Some("\"$1\""),
                    "Typed $attrs declaration",
                ));
            }
            if !existing.contains(&"src") {
                items.push(attr_item("src", Some("\"$1\""), "External script source"));
            }
        }
        "template" => {
            if !existing.contains(&"lang") {
                items.push(attr_item("lang", Some("\"${1:pug}\""), "Template language"));
            }
        }
        "style" => {
            if !existing.contains(&"scoped") {
                items.push(attr_item("scoped", None, "Component-scoped CSS"));
            }
            if !existing.contains(&"module") {
                items.push(attr_item("module", None, "CSS Modules"));
            }
            if !existing.contains(&"lang") {
                items.push(attr_item(
                    "lang",
                    Some("\"${1:scss}\""),
                    "Style language (scss, less, stylus)",
                ));
            }
        }
        _ => {
            if !existing.contains(&"lang") {
                items.push(attr_item("lang", Some("\"$1\""), "Block language"));
            }
        }
    }

    items
}

/// Build a root-level SFC snippet completion item.
///
/// Root SFC snippets all begin with `<` (e.g. `<script setup lang="ts">…`), so each
/// carries a `<`-anchored `text_edit` whose replace range absorbs an already-typed
/// `<` (see [`sfc_snippet_text_edit`] for the walk-back contract) — the
/// leading-`<` accounting is explicit and provider-neutral rather than left to a
/// client word heuristic. `insert_text` stays set as a fallback for clients that
/// ignore `text_edit`, and the item stays a SNIPPET with tab-stops preserved.
#[allow(clippy::too_many_arguments)]
fn snippet_item(
    label: &str,
    insert_text: &str,
    detail: &str,
    sort_priority: u32,
    source: &str,
    offset: u32,
    line_index: &LineIndex,
) -> CompletionItem {
    let text_edit = sfc_snippet_text_edit(insert_text, source, offset, line_index);
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(detail.to_string()),
        insert_text: Some(insert_text.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        sort_text: Some(format!("{:04}", sort_priority)),
        text_edit,
        ..Default::default()
    }
}

/// Compute the `<`-anchored replace edit for a root SFC tag snippet.
///
/// Root tag snippets (`insert_text` beginning with `<`) must supply an explicit
/// replace range that includes an immediately-preceding typed `<`, so accepting
/// the item yields a single `<`. The range walks back from the cursor over the
/// partial tag-name word (a pure cursor-range computation over ASCII tag bytes,
/// not source-text semantic inspection) and absorbs a leading `<` when present;
/// with no typed `<` the range starts at the word boundary so the snippet's own
/// `<` is the only one. Returns `None` for non-`<` snippets and `None`
/// (fail-closed) when a byte offset cannot be mapped to a `Position`.
fn sfc_snippet_text_edit(
    insert_text: &str,
    source: &str,
    offset: u32,
    line_index: &LineIndex,
) -> Option<CompletionTextEdit> {
    if !insert_text.starts_with('<') {
        return None;
    }

    let bytes = source.as_bytes();
    let cursor = offset as usize;
    if cursor > bytes.len() {
        return None;
    }

    // Walk back over the partial tag-name word (ASCII alphanumeric / `-` / `_`).
    let mut i = cursor;
    while i > 0 {
        let b = bytes[i - 1];
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            i -= 1;
        } else {
            break;
        }
    }

    // If a typed `<` immediately precedes the partial word, absorb it; otherwise
    // start at the word start so the snippet's own `<` is the only one.
    let start_byte = if i > 0 && bytes[i - 1] == b'<' {
        i - 1
    } else {
        i
    };

    let start = line_index.offset_to_position(start_byte as u32)?;
    let end = line_index.offset_to_position(cursor as u32)?;
    Some(CompletionTextEdit::Edit(TextEdit {
        range: Range { start, end },
        new_text: insert_text.to_string(),
    }))
}

fn attr_item(name: &str, value_snippet: Option<&str>, detail: &str) -> CompletionItem {
    let (insert_text, format) = if let Some(val) = value_snippet {
        (format!("{}={}", name, val), InsertTextFormat::SNIPPET)
    } else {
        (name.to_string(), InsertTextFormat::PLAIN_TEXT)
    };
    CompletionItem {
        label: name.to_string(),
        kind: Some(CompletionItemKind::PROPERTY),
        detail: Some(detail.to_string()),
        insert_text: Some(insert_text),
        insert_text_format: Some(format),
        ..Default::default()
    }
}

// =============================================================================
// Tag Name Completions
// =============================================================================

/// Common HTML element names for tag completion.
const HTML_ELEMENTS: &[&str] = &[
    "div",
    "span",
    "p",
    "a",
    "button",
    "input",
    "form",
    "label",
    "select",
    "option",
    "textarea",
    "ul",
    "ol",
    "li",
    "table",
    "tr",
    "td",
    "th",
    "thead",
    "tbody",
    "tfoot",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "img",
    "video",
    "audio",
    "canvas",
    "svg",
    "section",
    "nav",
    "header",
    "footer",
    "main",
    "article",
    "aside",
    "details",
    "summary",
    "dialog",
    "pre",
    "code",
    "blockquote",
    "br",
    "hr",
    "strong",
    "em",
    "b",
    "i",
    "small",
    "sub",
    "sup",
];

/// Vue built-in component names for tag completion.
const VUE_BUILTINS: &[&str] = &[
    "Transition",
    "TransitionGroup",
    "KeepAlive",
    "Teleport",
    "Suspense",
    "component",
    "slot",
    "template",
];

/// Build tag name completions: HTML elements + Vue built-ins + imported components + workspace components.
pub(crate) fn tag_name_completions(
    analysis: &FileAnalysisSnapshot,
    workspace_components: Option<&[WorkspaceComponent]>,
    doc_uri: Option<&str>,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // HTML elements
    for &tag in HTML_ELEMENTS {
        items.push(CompletionItem {
            label: tag.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some("HTML element".to_string()),
            sort_text: Some(format!("2{}", tag)),
            ..Default::default()
        });
    }

    // Vue built-ins
    for &tag in VUE_BUILTINS {
        items.push(CompletionItem {
            label: tag.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Vue built-in".to_string()),
            sort_text: Some(format!("1{}", tag)),
            ..Default::default()
        });
    }

    // Imported components from template analysis
    if let Some(template) = &analysis.template {
        for comp in &template.components {
            items.push(CompletionItem {
                // Confirmed component used in the template, offered in tag position:
                // CLASS (component icon).
                label: comp.name.clone(),
                kind: Some(CompletionItemKind::CLASS),
                detail: comp.import_source.as_ref().map(|s| format!("from '{}'", s)),
                sort_text: Some(format!("0{}", comp.name)),
                ..Default::default()
            });
        }
    }

    // Uppercase bindings (component-like: PascalCase names from script)
    for binding in &analysis.bindings {
        if binding
            .name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
        {
            // Avoid duplicates with components already added
            if !items.iter().any(|i| i.label == binding.name) {
                items.push(CompletionItem {
                    label: binding.name.clone(),
                    // Heuristic uppercase-name binding (may NOT be a component): keep
                    // MODULE, not CLASS — only confirmed component tags earn the
                    // component (CLASS) icon, so an arbitrary `const Foo = ...` does
                    // not get a misleading component glyph.
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some("component binding".to_string()),
                    sort_text: Some(format!("0{}", binding.name)),
                    ..Default::default()
                });
            }
        }
    }

    // Non-type uppercase imports (component-like)
    for import in &analysis.imports {
        if import.is_type_only {
            continue;
        }
        for binding in &import.bindings {
            if binding.is_type_only {
                continue;
            }
            if binding
                .name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
                && !items.iter().any(|i| i.label == binding.name)
            {
                items.push(CompletionItem {
                    label: binding.name.clone(),
                    // Heuristic uppercase-name import (may NOT be a component): keep
                    // MODULE, not CLASS — the uppercase-first-char guess is not a
                    // confirmed component, so it must not claim the component icon.
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some(format!("from '{}'", import.source)),
                    sort_text: Some(format!("0{}", binding.name)),
                    ..Default::default()
                });
            }
        }
    }

    // Workspace components (auto-import)
    if let Some(ws_components) = workspace_components {
        let mut existing: std::collections::HashSet<String> =
            items.iter().map(|i| i.label.clone()).collect();
        for comp in ws_components {
            // Skip names already declared/imported in this file AND dedup
            // workspace-vs-workspace label collisions: two carriers can sanitize
            // to the same identifier (e.g. `Model.Named.vue` and `ModelNamed.vue`
            // both → `ModelNamed`). `build_workspace_components` sorts candidates
            // by canonical path, so the first-inserted label wins (the
            // lexicographically-first carrier); later collisions are dropped so the
            // user never sees two indistinguishable items.
            if !existing.insert(comp.name.clone()) {
                continue;
            }
            let mut data = serde_json::json!({
                "auto_import": true,
                "import_path": comp.import_path,
                "component_name": comp.name,
            });
            if let Some(uri) = doc_uri {
                data["uri"] = serde_json::Value::String(uri.to_string());
            }
            items.push(CompletionItem {
                label: comp.name.clone(),
                // Confirmed workspace component file, offered in tag position
                // (auto-import on accept): CLASS (component icon).
                kind: Some(CompletionItemKind::CLASS),
                detail: Some(format!("Auto import from '{}'", comp.import_path)),
                sort_text: Some(format!("3{}", comp.name)),
                data: Some(data),
                ..Default::default()
            });
        }
    }

    items
}

// =============================================================================
// Attribute Name Completions
// =============================================================================

/// Build attribute name completions: Vue directives + common HTML attributes + event shorthands.
fn attribute_name_completions() -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // Vue directives
    let directives = [
        ("v-if", "Conditional rendering"),
        ("v-else", "Else branch for v-if"),
        ("v-else-if", "Else-if branch for v-if"),
        ("v-for", "List rendering"),
        ("v-show", "Toggle element visibility"),
        ("v-model", "Two-way data binding"),
        ("v-on", "Event listener"),
        ("v-bind", "Dynamic attribute binding"),
        ("v-slot", "Named slot"),
        ("v-html", "Set innerHTML"),
        ("v-text", "Set textContent"),
        ("v-pre", "Skip compilation for this element"),
        ("v-once", "Render only once"),
        ("v-memo", "Memoize sub-tree"),
        ("v-cloak", "Hide until compiled"),
    ];
    for (name, desc) in directives {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(desc.to_string()),
            sort_text: Some(format!("0{}", name)),
            ..Default::default()
        });
    }

    // Common HTML attributes
    let html_attrs = [
        ("class", "CSS class"),
        ("id", "Element ID"),
        ("style", "Inline styles"),
        ("ref", "Template ref"),
        ("key", "v-for key"),
    ];
    for (name, desc) in html_attrs {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(desc.to_string()),
            sort_text: Some(format!("1{}", name)),
            ..Default::default()
        });
    }

    // Event shorthands
    let events = [
        ("@click", "Click event"),
        ("@input", "Input event"),
        ("@change", "Change event"),
        ("@submit", "Submit event"),
        ("@keydown", "Keydown event"),
        ("@keyup", "Keyup event"),
        ("@mousedown", "Mousedown event"),
        ("@focus", "Focus event"),
        ("@blur", "Blur event"),
    ];
    for (name, desc) in events {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::EVENT),
            detail: Some(desc.to_string()),
            insert_text: Some(format!("{}=\"$1\"", name)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some(format!("2{}", name)),
            ..Default::default()
        });
    }

    items
}

// =============================================================================
// v-model / v-bind Modifier Completions
// =============================================================================

/// v-model modifiers: lazy, number, trim
const VMODEL_MODIFIERS: &[(&str, &str)] = &[
    ("lazy", "Sync after change instead of input"),
    ("number", "Typecast input value to number"),
    ("trim", "Trim whitespace from input"),
];

/// Provide v-model modifier completions.
fn vmodel_modifier_completions() -> CompletionResult {
    let items = VMODEL_MODIFIERS
        .iter()
        .map(|(name, desc)| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(desc.to_string()),
            sort_text: Some(format!("0{}", name)),
            ..Default::default()
        })
        .collect();
    CompletionResult {
        items,
        is_incomplete: false,
    }
}

// =============================================================================
// CSS Class Attribute Completions
// =============================================================================

/// Offer CSS class completions when cursor is inside a `class="..."` or `:class` attribute.
///
/// Returns `is_incomplete: true` so VS Code re-queries on every keystroke for live filtering.
fn class_attribute_completions(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<CompletionResult> {
    let template = analysis.template.as_deref()?;

    // Check if cursor is inside a class attribute value
    for element in &template.elements {
        for attr in &element.attributes {
            if (offset as u32) < attr.span.start || (offset as u32) >= attr.span.end {
                continue;
            }
            if attr.name != "class" {
                continue;
            }

            let attr_text = &source[attr.span.start as usize..attr.span.end as usize];

            if attr.is_dynamic {
                // Dynamic :class — check if cursor is inside a quoted string within the expression
                if let Some(eq_pos) = attr_text.find('=') {
                    let after_eq = &attr_text[eq_pos + 1..];
                    // Find outer quote
                    if let Some(q_pos) = after_eq.find(['"', '\'']) {
                        let outer_quote = after_eq.as_bytes()[q_pos];
                        let val_start = attr.span.start as usize + eq_pos + 1 + q_pos + 1;
                        let cursor_in_expr = offset.saturating_sub(val_start);
                        let value = attr.value.as_deref().unwrap_or("");

                        // Walk expression to find if cursor is inside an inner quoted string
                        if is_cursor_in_inner_string(value, cursor_in_expr, outer_quote) {
                            return Some(build_class_completions(analysis, source, offset));
                        }
                    }
                }
            } else {
                // Static class="..." — check if cursor is in the value portion
                let value = match attr.value.as_ref() {
                    Some(v) => v,
                    None => continue,
                };
                if let Some(val_offset) = attr_text.find(value.as_str()) {
                    let val_start = attr.span.start as usize + val_offset;
                    let val_end = val_start + value.len();
                    if offset >= val_start && offset <= val_end {
                        return Some(build_class_completions(analysis, source, offset));
                    }
                }
            }
        }
    }

    None
}

/// Check if the cursor is inside a quoted string within a `:class` expression.
fn is_cursor_in_inner_string(expr: &str, cursor_pos: usize, outer_quote: u8) -> bool {
    let bytes = expr.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if b == outer_quote {
            // End of expression, not inside a string
            return false;
        }
        if b == b'\'' || b == b'"' {
            let quote = b;
            let start = i + 1;
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            // Cursor is between start and i (the closing quote)?
            if cursor_pos >= start && cursor_pos <= i {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Build CSS class completion items from all style blocks.
fn build_class_completions(
    analysis: &FileAnalysisSnapshot,
    _source: &str,
    _offset: usize,
) -> CompletionResult {
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();

    for style in analysis.styles.iter() {
        if let Some(css) = &style.css {
            for cls in &css.classes {
                if seen.insert(cls.name.clone()) {
                    items.push(CompletionItem {
                        label: cls.name.clone(),
                        kind: Some(CompletionItemKind::VALUE),
                        detail: Some("scoped CSS".into()),
                        sort_text: Some(format!("z{}", cls.name)),
                        ..Default::default()
                    });
                }
            }
        }
    }

    CompletionResult {
        items,
        is_incomplete: true,
    }
}

/// Completions available in `<script setup>` context.
fn script_completions(analysis: &FileAnalysisSnapshot) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // Offer existing bindings (sort prefix "0" = highest priority)
    for binding in &analysis.bindings {
        items.push(CompletionItem {
            label: binding.name.clone(),
            kind: Some(binding_completion_kind(&binding.kind)),
            detail: Some(binding_detail(binding)),
            sort_text: Some(format!("0{}", binding.name)),
            ..Default::default()
        });
    }

    // Offer imports (sort prefix "1" = below locals, above globals)
    for import in &analysis.imports {
        for binding in &import.bindings {
            items.push(CompletionItem {
                label: binding.name.clone(),
                kind: Some(if binding.is_type_only || import.is_type_only {
                    CompletionItemKind::TYPE_PARAMETER
                } else {
                    CompletionItemKind::MODULE
                }),
                detail: Some(format!("from '{}'", import.source)),
                sort_text: Some(format!("1{}", binding.name)),
                ..Default::default()
            });
        }
    }

    // Filter out internal symbols
    items
        .retain(|item| !item.label.starts_with("___VERTER___") && !is_internal_dunder(&item.label));

    items
}

/// Extra completions for script context in SSR projects.
///
/// Boosts `onServerPrefetch` and `import.meta.server/client` patterns.
fn ssr_script_completions() -> Vec<CompletionItem> {
    vec![
        CompletionItem {
            label: "onServerPrefetch".to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some("SSR data-fetching hook (from 'vue')".to_string()),
            sort_text: Some("0onServerPrefetch".to_string()),
            insert_text: Some("onServerPrefetch(async () => {\n\t$0\n})".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
        CompletionItem {
            label: "import.meta.server".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("true during SSR, false on client".to_string()),
            sort_text: Some("0import.meta.server".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "import.meta.client".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("true on client, false during SSR".to_string()),
            sort_text: Some("0import.meta.client".to_string()),
            ..Default::default()
        },
    ]
}

/// Extra tag name completions for template context in SSR projects.
fn ssr_tag_name_completions() -> Vec<CompletionItem> {
    vec![CompletionItem {
        label: "ClientOnly".to_string(),
        kind: Some(CompletionItemKind::CLASS),
        detail: Some("Render children only on client side".to_string()),
        sort_text: Some("0ClientOnly".to_string()),
        insert_text: Some("ClientOnly>\n\t$0\n</ClientOnly>".to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    }]
}

/// Completions available in `<template>` context.
///
/// Offers all script-setup bindings that are available in the template scope.
fn template_completions(
    analysis: &FileAnalysisSnapshot,
    workspace_components: Option<&[WorkspaceComponent]>,
    doc_uri: Option<&str>,
    offset: Option<u32>,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // All bindings from script setup are available in template
    for binding in &analysis.bindings {
        let mut item = CompletionItem {
            label: binding.name.clone(),
            kind: Some(binding_completion_kind(&binding.kind)),
            detail: Some(binding_detail(binding)),
            ..Default::default()
        };

        // Add reactivity indicator
        let reactivity_tag = match binding.reactivity_kind {
            verter_semantic::analysis::ReactivityKind::Ref => Some("ref"),
            verter_semantic::analysis::ReactivityKind::Computed => Some("computed"),
            verter_semantic::analysis::ReactivityKind::Reactive => Some("reactive"),
            verter_semantic::analysis::ReactivityKind::MaybeRef => Some("maybe-ref"),
            verter_semantic::analysis::ReactivityKind::Mutable => Some("mutable"),
            verter_semantic::analysis::ReactivityKind::None => {
                if binding.is_reactive {
                    Some("reactive")
                } else {
                    None
                }
            }
        };
        if let Some(tag) = reactivity_tag {
            item.detail = Some(format!("{} ({tag})", item.detail.unwrap_or_default()));
        }

        items.push(item);
    }

    // Non-type imports are also available in template
    for import in &analysis.imports {
        if import.is_type_only {
            continue;
        }
        for binding in &import.bindings {
            if binding.is_type_only {
                continue;
            }
            items.push(CompletionItem {
                label: binding.name.clone(),
                kind: Some(CompletionItemKind::MODULE),
                detail: Some(format!("from '{}'", import.source)),
                ..Default::default()
            });
        }
    }

    // Macro result bindings are available too
    for mac in analysis.macros.iter() {
        if let Some(ref name) = mac.binding_name {
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(format!("{}()", macro_kind_label(&mac.kind))),
                ..Default::default()
            });
        }
    }

    // Workspace components available for auto-import.
    // Only add components not already imported/declared in this file.
    if let Some(ws_components) = workspace_components {
        let mut existing_labels: std::collections::HashSet<String> =
            items.iter().map(|i| i.label.clone()).collect();
        for comp in ws_components {
            // Skip names already in scope AND dedup workspace-vs-workspace label
            // collisions first-wins (lex-first carrier, since
            // `build_workspace_components` sorts by canonical path) — same contract
            // as `tag_name_completions`.
            if !existing_labels.insert(comp.name.clone()) {
                continue;
            }
            let mut data = serde_json::json!({
                "auto_import": true,
                "import_path": comp.import_path,
                "component_name": comp.name,
            });
            if let Some(uri) = doc_uri {
                data["uri"] = serde_json::Value::String(uri.to_string());
            }
            items.push(CompletionItem {
                label: comp.name.clone(),
                // This auto-import item is offered in EXPRESSION / INTERPOLATION
                // scope ({{ }} / bound attr value), NOT a component-tag position, so
                // it is referenced as a value binding, not inserted as a `<Tag>`.
                // Keep MODULE — CLASS is reserved for genuine tag-position component
                // completions (see `tag_name_completions`).
                kind: Some(CompletionItemKind::MODULE),
                detail: Some(format!("Auto import from '{}'", comp.import_path)),
                sort_text: Some(format!("z{}", comp.name)), // Sort after local items
                data: Some(data),
                ..Default::default()
            });
        }
    }

    // V-for scoped variables available at cursor position
    if let (Some(offset), Some(template)) = (offset, &analysis.template) {
        for el in &template.elements {
            if let Some(ref vf) = el.v_for {
                if offset >= el.span.start && offset < el.span.end {
                    for name in extract_vfor_variable_names(&vf.variable) {
                        items.push(CompletionItem {
                            label: name.to_string(),
                            kind: Some(CompletionItemKind::VARIABLE),
                            detail: Some("v-for variable".to_string()),
                            ..Default::default()
                        });
                    }
                    if let Some(ref idx) = vf.index {
                        items.push(CompletionItem {
                            label: idx.clone(),
                            kind: Some(CompletionItemKind::VARIABLE),
                            detail: Some("v-for index".to_string()),
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }

    // Filter out internal symbols
    items
        .retain(|item| !item.label.starts_with("___VERTER___") && !is_internal_dunder(&item.label));

    // Deduplicate by label
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label);

    items
}

/// Extract identifier names from a v-for variable pattern.
/// Handles simple names (`item`), destructured objects (`{ name, email }`),
/// and destructured arrays (`[first, second]`).
fn extract_vfor_variable_names(pattern: &str) -> Vec<&str> {
    let trimmed = pattern.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        trimmed
            .trim_matches(|c| c == '{' || c == '}' || c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim())
            .filter(|s| {
                !s.is_empty()
                    && s.chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
            })
            .collect()
    } else {
        vec![trimmed]
    }
}

fn binding_completion_kind(
    kind: &verter_semantic::analysis::AnalyzedBindingKind,
) -> CompletionItemKind {
    match kind {
        verter_semantic::analysis::AnalyzedBindingKind::Const => CompletionItemKind::VARIABLE,
        verter_semantic::analysis::AnalyzedBindingKind::Let
        | verter_semantic::analysis::AnalyzedBindingKind::Var => CompletionItemKind::VARIABLE,
        verter_semantic::analysis::AnalyzedBindingKind::Function
        | verter_semantic::analysis::AnalyzedBindingKind::AsyncFunction => {
            CompletionItemKind::FUNCTION
        }
        verter_semantic::analysis::AnalyzedBindingKind::Class => CompletionItemKind::CLASS,
    }
}

fn binding_detail(binding: &verter_semantic::analysis::AnalyzedBinding) -> String {
    let kind = match binding.kind {
        verter_semantic::analysis::AnalyzedBindingKind::Const => "const",
        verter_semantic::analysis::AnalyzedBindingKind::Let => "let",
        verter_semantic::analysis::AnalyzedBindingKind::Var => "var",
        verter_semantic::analysis::AnalyzedBindingKind::Function => "function",
        verter_semantic::analysis::AnalyzedBindingKind::AsyncFunction => "async function",
        verter_semantic::analysis::AnalyzedBindingKind::Class => "class",
    };
    kind.to_string()
}

// =============================================================================
// Event Modifier Completions
// =============================================================================

use crate::features::event_modifiers::{
    is_keyboard_event, is_mouse_button_event, KEY_MODIFIERS, MOUSE_BUTTON_MODIFIERS,
    RUNTIME_MODIFIERS, SYSTEM_MODIFIERS,
};

/// Provide event modifier completions for a given event name.
/// The event name is determined by the cursor context module.
fn event_modifier_completions_for(event_name: &str) -> CompletionResult {
    // Event-family classification is shared with hover (see `event_modifiers`) so the
    // two surfaces never disagree on which modifiers apply to which events.
    let is_keyboard = is_keyboard_event(event_name);
    let is_mouse_button = is_mouse_button_event(event_name);

    let mut items = Vec::new();

    // Runtime modifiers (all events)
    for (name, desc) in RUNTIME_MODIFIERS {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(desc.to_string()),
            sort_text: Some(format!("0{}", name)),
            ..Default::default()
        });
    }

    // System modifiers (all events)
    for (name, desc) in SYSTEM_MODIFIERS {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(desc.to_string()),
            sort_text: Some(format!("1{}", name)),
            ..Default::default()
        });
    }

    // Key modifiers (keyboard events only)
    if is_keyboard {
        for (name, desc) in KEY_MODIFIERS {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(desc.to_string()),
                sort_text: Some(format!("2{}", name)),
                ..Default::default()
            });
        }
    }

    // Mouse button modifiers (click/mouse button events)
    if is_mouse_button {
        for (name, desc) in MOUSE_BUTTON_MODIFIERS {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(desc.to_string()),
                sort_text: Some(format!("2{}", name)),
                ..Default::default()
            });
        }
    }

    CompletionResult {
        items,
        is_incomplete: false,
    }
}

// =============================================================================
// Component Prop Completions
// =============================================================================

/// Detect if cursor is inside a component's opening tag and offer prop/event completions.
///
/// Scans backward from cursor to find `<ComponentName`, verifies we're still in the
/// opening tag (not past `>`), then looks up the component's prop/emit definitions.
#[allow(clippy::type_complexity)]
fn component_prop_completions(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    resolve_component: Option<&dyn Fn(&str, Option<&str>) -> Option<FileAnalysisSnapshot>>,
    existing_attrs: &[String],
    carrier_language: Option<CarrierTemplateLanguage>,
) -> Option<Vec<CompletionItem>> {
    let template = analysis.template.as_deref()?;

    // Find which component's opening tag contains the cursor
    let component_name = find_component_at_cursor(offset, source, template)?;

    // Prefer the analyzed component usage. During an incomplete opening tag the
    // template parser may not retain that usage yet, so fall back to the script
    // import whose local binding matches the structurally scanned tag name.
    let comp_usage = template
        .components
        .iter()
        .find(|c| c.name == component_name || to_kebab_case(&c.name) == component_name);
    let imported_binding = analysis.imports.iter().find_map(|import| {
        import
            .bindings
            .iter()
            .find(|binding| {
                binding.name == component_name || to_kebab_case(&binding.name) == component_name
            })
            .map(|binding| (import.source.as_str(), binding.name.as_str()))
    });
    let (import_source, resolved_component_name) = if let Some(usage) = comp_usage {
        (usage.import_source.as_deref()?, usage.name.as_str())
    } else {
        imported_binding?
    };

    // Resolve the component's analysis (pass component name for barrel re-export following)
    let resolve_fn = resolve_component?;
    let child_analysis = resolve_fn(import_source, Some(resolved_component_name))?;
    // Attribute syntax is owned by the PARENT template language. Import
    // spelling is not a framework discriminator: extensionless resolution and
    // barrel re-exports can both terminate at a Svelte child.
    let uses_svelte_syntax = carrier_language == Some(CarrierTemplateLanguage::Svelte);

    let mut items = Vec::new();

    // Collect already-used prop/event names on THIS element (not all usages of the component)
    // to avoid offering duplicates. existing_attrs comes from cursor_context and contains
    // only the attributes on the specific element at the cursor position.
    let used_props: std::collections::HashSet<&str> =
        existing_attrs.iter().map(|s| s.as_str()).collect();

    // --- Props ---
    // Try template.prop_definitions first (future type-provider enrichment),
    // fall back to macro prop_fields from defineProps.
    let child_template = child_analysis.template.as_deref();
    let has_prop_defs = child_template.is_some_and(|t| !t.prop_definitions.is_empty());

    if let (true, Some(child_template)) = (has_prop_defs, child_template) {
        for prop_def in &child_template.prop_definitions {
            let label = if uses_svelte_syntax {
                prop_def.name.clone()
            } else {
                to_kebab_case(&prop_def.name)
            };
            if used_props.contains(prop_def.name.as_str()) || used_props.contains(label.as_str()) {
                continue;
            }
            let mut detail_parts = Vec::new();
            if let Some(ref ty) = prop_def.type_annotation {
                detail_parts.push(ty.clone());
            }
            if prop_def.is_required {
                detail_parts.push("required".to_string());
            }
            if prop_def.has_default {
                detail_parts.push("has default".to_string());
            }

            let insert_text = if prop_def.is_boolean {
                Some(label.clone())
            } else if uses_svelte_syntax {
                Some(format!("{}={{$1}}", label))
            } else {
                Some(format!(":{}=\"$1\"", label))
            };

            items.push(CompletionItem {
                label: label.clone(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: if detail_parts.is_empty() {
                    Some("prop".to_string())
                } else {
                    Some(detail_parts.join(", "))
                },
                insert_text,
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                sort_text: Some(format!("0{}", label)),
                ..Default::default()
            });
        }
    } else {
        // Fall back to macro prop_fields
        for m in child_analysis.macros.iter() {
            if m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps {
                for field in &m.prop_fields {
                    let label = if uses_svelte_syntax {
                        field.name.clone()
                    } else {
                        to_kebab_case(&field.name)
                    };
                    if used_props.contains(field.name.as_str())
                        || used_props.contains(label.as_str())
                    {
                        continue;
                    }
                    items.push(CompletionItem {
                        label: label.clone(),
                        kind: Some(CompletionItemKind::PROPERTY),
                        detail: Some("prop".to_string()),
                        insert_text: Some(if uses_svelte_syntax {
                            format!("{}={{$1}}", label)
                        } else {
                            format!(":{}=\"$1\"", label)
                        }),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        sort_text: Some(format!("0{}", label)),
                        ..Default::default()
                    });
                }
            }
        }
    }

    // --- Events ---
    // Try template.emit_definitions first, fall back to macro emit_fields.
    let has_emit_defs = child_template.is_some_and(|t| !t.emit_definitions.is_empty());

    if let (true, Some(child_template)) = (has_emit_defs, child_template) {
        for emit_def in &child_template.emit_definitions {
            let event_name = &emit_def.event_name;
            let label = format!("@{}", to_kebab_case(event_name));
            let insert_text = Some(format!("@{}=\"$1\"", to_kebab_case(event_name)));

            items.push(CompletionItem {
                label,
                kind: Some(CompletionItemKind::EVENT),
                detail: Some("event".to_string()),
                insert_text,
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                sort_text: Some(format!("1{}", event_name)),
                ..Default::default()
            });
        }
    } else {
        // Fall back to macro emit_fields
        for m in child_analysis.macros.iter() {
            if m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineEmits {
                for field in &m.emit_fields {
                    let label = format!("@{}", to_kebab_case(&field.name));
                    let insert_text = Some(format!("@{}=\"$1\"", to_kebab_case(&field.name)));

                    items.push(CompletionItem {
                        label,
                        kind: Some(CompletionItemKind::PROPERTY),
                        detail: Some("event".to_string()),
                        insert_text,
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        sort_text: Some(format!("1{}", field.name)),
                        ..Default::default()
                    });
                }
            }
        }
    }

    if items.is_empty() {
        return None;
    }

    Some(items)
}

// =============================================================================
// D5 — slot-name completion (Vue `v-slot`/`#`, Svelte `{#snippet}`/`{@render}`)
// =============================================================================

/// Resolve the component usage that owns the slot attribute at `offset`:
/// the component element itself for `<MyComp #|>`, or the parent component
/// element for `<template #|>`. Returns the usage plus the used slot names
/// recorded on it.
fn slot_owner_component<'a>(
    offset: usize,
    source: &str,
    template: &'a verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    tag_name: &str,
    is_component: bool,
) -> Option<&'a verter_semantic::analysis::template::TemplateComponentUsage> {
    if is_component && tag_name != "template" {
        return template.components.iter().find(|component| {
            component.name == tag_name
                || to_kebab_case(&component.name) == tag_name
                || component.name == to_pascal_case(tag_name)
        });
    }
    // `<template #…>`: typed parent walk — the template element whose open-tag
    // region contains the offset, then its parent component element.
    let offset32 = offset as u32;
    let owner_element = template.elements.iter().find(|el| {
        el.tag == "template"
            && offset32 >= el.span.start
            && offset32 < el.tag_span_end.max(el.span.start + 1)
    });
    if let Some(el) = owner_element {
        let parent = el
            .parent_index
            .and_then(|idx| template.elements.get(idx as usize));
        if let Some(parent) = parent {
            if parent.is_component {
                if let Some(component) = template.components.iter().find(|component| {
                    component.name == parent.tag || component.name == to_pascal_case(&parent.tag)
                }) {
                    return Some(component);
                }
            }
        }
    }
    // Incomplete-parse fallback: scan backwards from the slot tag for the
    // nearest enclosing component open tag (skipping closing tags and the
    // slot's own `<template` tag).
    let bytes = source.as_bytes();
    let mut i = offset.min(source.len());
    let mut saw_slot_tag = false;
    while i > 0 {
        i -= 1;
        if bytes[i] != b'<' {
            continue;
        }
        let tag_start = i + 1;
        if tag_start < source.len() && bytes[tag_start] == b'/' {
            continue; // closing tag
        }
        let mut tag_end = tag_start;
        while tag_end < source.len()
            && (bytes[tag_end].is_ascii_alphanumeric()
                || bytes[tag_end] == b'-'
                || bytes[tag_end] == b'_')
        {
            tag_end += 1;
        }
        let name = &source[tag_start..tag_end];
        if !saw_slot_tag {
            // The first open tag before the cursor is the slot's own
            // `<template`/`<MyComp` tag — step over it to find the owner.
            saw_slot_tag = true;
            if name != "template" {
                return template.components.iter().find(|component| {
                    component.name == name
                        || to_kebab_case(&component.name) == name
                        || component.name == to_pascal_case(name)
                });
            }
            continue;
        }
        return template.components.iter().find(|component| {
            component.name == name
                || to_kebab_case(&component.name) == name
                || component.name == to_pascal_case(name)
        });
    }
    None
}

/// The tag OWNING the slot attribute being typed, recovered from source while
/// the parse has not retained the usage: the component tag itself for
/// `<MyComp #|>`, or the nearest enclosing component open tag for
/// `<template #|>`. Non-component tags (lowercase elements like `<span>`) are
/// never slot owners and are skipped; closing tags are stepped over.
fn slot_owner_tag_name(
    offset: usize,
    source: &str,
    is_component: bool,
    analysis: &FileAnalysisSnapshot,
) -> Option<String> {
    let bytes = source.as_bytes();
    let mut i = offset.min(source.len());
    let mut skip_slot_tag = !is_component;
    while i > 0 {
        i -= 1;
        if bytes[i] != b'<' {
            continue;
        }
        let tag_start = i + 1;
        if tag_start < source.len() && bytes[tag_start] == b'/' {
            continue; // closing tag
        }
        let mut tag_end = tag_start;
        while tag_end < source.len()
            && (bytes[tag_end].is_ascii_alphanumeric()
                || bytes[tag_end] == b'-'
                || bytes[tag_end] == b'_')
        {
            tag_end += 1;
        }
        let name = source.get(tag_start..tag_end)?;
        if name.is_empty() {
            return None;
        }
        if skip_slot_tag && name == "template" {
            // The first open tag before the cursor is the slot's own
            // `<template` tag — step over it to find the owner.
            skip_slot_tag = false;
            continue;
        }
        let is_component_tag = name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_uppercase())
            || analysis.imports.iter().any(|import| {
                import
                    .bindings
                    .iter()
                    .any(|binding| binding.name == name || to_kebab_case(&binding.name) == name)
            });
        if is_component_tag {
            return Some(name.to_string());
        }
    }
    None
}

/// D5: slot-name completion for `<template #|>` / `<template v-slot:|>` /
/// `<MyComp #|>` — server-owned items from the CHILD's declared slots surface
/// (defineSlots fields, template defined-slots fallback), never a document
/// word fallback. Already-used slot names are filtered with Vue's kebab↔camel
/// equivalence; `default` is offered while unused and undeclared.
#[allow(clippy::type_complexity)]
fn slot_name_completions(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    resolve_component: Option<&dyn Fn(&str, Option<&str>) -> Option<FileAnalysisSnapshot>>,
    tag_name: &str,
    is_component: bool,
) -> Option<Vec<CompletionItem>> {
    let template = analysis.template.as_deref();
    let component = template.and_then(|template| {
        slot_owner_component(offset, source, template, tag_name, is_component)
    });
    // While the slot tag is still being typed the error-tolerant parse may
    // drop the component usage entirely; recover the owner through the script
    // import whose local binding matches the scanned tag (the same recovery
    // component prop completion uses for incomplete tags).
    let owner_tag = component
        .map(|usage| usage.name.clone())
        .or_else(|| slot_owner_tag_name(offset, source, is_component, analysis))?;
    let (import_source, resolved_name) = if let Some(usage) = component {
        (usage.import_source.as_deref()?, usage.name.as_str())
    } else {
        analysis.imports.iter().find_map(|import| {
            import
                .bindings
                .iter()
                .find(|binding| {
                    binding.name == owner_tag || to_kebab_case(&binding.name) == owner_tag
                })
                .map(|binding| (import.source.as_str(), binding.name.as_str()))
        })?
    };
    let resolve_fn = resolve_component?;
    let child_analysis = resolve_fn(import_source, Some(resolved_name))?;

    // Already-used slot names: the usage's recorded slots plus sibling
    // `<template #x>` slot directives under the same parent element. Both are
    // typed facts that exist only while the parse retains the usage; in a
    // mid-typing broken-parse window the filter degrades to "all declared
    // slots offered" rather than guessing from source text.
    let mut used: Vec<String> = component
        .map(|usage| usage.slots_used.clone())
        .unwrap_or_default();
    if let Some(template) = template {
        let offset32 = offset as u32;
        let owner_parent_idx = template
            .elements
            .iter()
            .position(|el| {
                el.tag == "template"
                    && offset32 >= el.span.start
                    && offset32 < el.tag_span_end.max(el.span.start + 1)
            })
            .and_then(|idx| template.elements[idx].parent_index)
            .map(|idx| idx as usize);
        if let Some(parent_idx) = owner_parent_idx {
            for el in &template.elements {
                if el.parent_index.map(|idx| idx as usize) != Some(parent_idx) {
                    continue;
                }
                for dir in &el.directives {
                    if dir.name == "slot" {
                        used.push(
                            dir.argument
                                .clone()
                                .unwrap_or_else(|| "default".to_string()),
                        );
                    }
                }
            }
        }
    }
    let is_used = |name: &str| {
        used.iter()
            .any(|used_name| crate::server::attr_name_match_rank(name, used_name).is_some())
    };

    let mut items = Vec::new();
    let mut declared_default = false;
    for mac in child_analysis.macros.iter() {
        if mac.kind != verter_semantic::analysis::AnalyzedMacroKind::DefineSlots {
            continue;
        }
        for field in &mac.slot_fields {
            if field.name == "default" {
                declared_default = true;
            }
            if is_used(&field.name) {
                continue;
            }
            let props = if field.bindings.is_empty() {
                String::new()
            } else {
                format!(
                    "(props: {{ {} }})",
                    field
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
                        .join("; ")
                )
            };
            let return_type = field.return_type.as_deref().unwrap_or("any");
            let name = field.name.clone();
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!("(slot) {name}{props}: {return_type}")),
                documentation: field
                    .description
                    .clone()
                    .map(tower_lsp_server::ls_types::Documentation::String),
                insert_text: Some(name.clone()),
                sort_text: Some(format!("0{name}")),
                ..Default::default()
            });
        }
    }
    if items.is_empty() {
        // Fallback: the child's template-defined slots (untyped names).
        if let Some(child_template) = child_analysis.template.as_deref() {
            for slot in &child_template.defined_slots {
                if slot.name == "default" {
                    declared_default = true;
                }
                if is_used(&slot.name) {
                    continue;
                }
                items.push(CompletionItem {
                    label: slot.name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some("slot".to_string()),
                    insert_text: Some(slot.name.clone()),
                    sort_text: Some(format!("0{}", slot.name)),
                    ..Default::default()
                });
            }
        }
    }
    if !declared_default && !is_used("default") {
        items.push(CompletionItem {
            label: "default".to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some("default slot".to_string()),
            insert_text: Some("default".to_string()),
            sort_text: Some("0default".to_string()),
            ..Default::default()
        });
    }
    if items.is_empty() {
        return None;
    }
    Some(items)
}

/// Classify a captured type annotation as Svelte's `Snippet` prop type by its
/// ROOT type-reference name — the typed-IR classification of the annotation,
/// not a substring sniff. `Snippet`, `Snippet<…>`, `svelte.Snippet`, and
/// `import("svelte").Snippet<…>` classify; `NotSnippet`, `SnippetExtra`, or a
/// payload that merely mentions `Snippet` inside generic arguments do not.
fn is_snippet_type_annotation(type_annotation: &str) -> bool {
    let mut root = type_annotation.trim();
    // An `import("…")` qualifier prefixes the type-reference path.
    if let Some(rest) = root.strip_prefix("import(") {
        if let Some(close) = rest.find(')') {
            root = rest[close + 1..].trim_start_matches('.').trim();
        }
    }
    // The root type-reference name ends where generic arguments, unions,
    // intersections, arrays, or function parameters begin.
    let cut = root.find(['<', '|', '&', '[', '(']).unwrap_or(root.len());
    let root = root[..cut].trim();
    // Qualified roots (`svelte.Snippet`) classify on their trailing segment.
    root.rsplit('.').next().unwrap_or(root).trim() == "Snippet"
}

/// D5 Svelte: `{#snippet |` inside a component completes the snippet-slot
/// names the CHILD accepts — its snippet-typed props (e.g.
/// `header?: import("svelte").Snippet<…>`) — with used slots filtered.
#[allow(clippy::type_complexity)]
fn svelte_snippet_slot_completions(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    resolve_component: Option<&dyn Fn(&str, Option<&str>) -> Option<FileAnalysisSnapshot>>,
    tag_name: &str,
) -> Option<Vec<CompletionItem>> {
    let template = analysis.template.as_deref()?;
    let component = template.components.iter().find(|component| {
        component.name == tag_name
            || to_kebab_case(&component.name) == tag_name
            || component.name == to_pascal_case(tag_name)
    });
    let component = match component {
        Some(component) => component,
        None => slot_owner_component(offset, source, template, tag_name, true)?,
    };
    let import_source = component.import_source.as_deref()?;
    let resolve_fn = resolve_component?;
    let child_analysis = resolve_fn(import_source, Some(&component.name))?;

    let is_used = |name: &str| component.slots_used.iter().any(|used| used == name);
    let mut items = Vec::new();
    if let Some(child_template) = child_analysis.template.as_deref() {
        for prop_def in &child_template.prop_definitions {
            let is_snippet = prop_def
                .type_annotation
                .as_deref()
                .is_some_and(is_snippet_type_annotation);
            if !is_snippet || is_used(&prop_def.name) {
                continue;
            }
            items.push(CompletionItem {
                label: prop_def.name.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: prop_def
                    .type_annotation
                    .clone()
                    .or_else(|| Some("snippet".to_string())),
                insert_text: Some(prop_def.name.clone()),
                sort_text: Some(format!("0{}", prop_def.name)),
                ..Default::default()
            });
        }
    }
    if items.is_empty() {
        return None;
    }
    Some(items)
}

/// D5 Svelte: `{@render |}` completes the in-scope snippet names — local
/// `{#snippet}` declarations (typed template IR) plus the component's own
/// snippet-typed props.
fn svelte_render_callee_completions(
    analysis: &FileAnalysisSnapshot,
) -> Option<Vec<CompletionItem>> {
    let template = analysis.template.as_deref()?;
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for snippet in &template.snippet_definitions {
        if !seen.insert(snippet.name.clone()) {
            continue;
        }
        let detail = snippet
            .params_text
            .as_deref()
            .map(|params| format!("({params})"))
            .unwrap_or_else(|| "()".to_string());
        items.push(CompletionItem {
            label: snippet.name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(format!("snippet {}{detail}", snippet.name)),
            insert_text: Some(snippet.name.clone()),
            sort_text: Some(format!("0{}", snippet.name)),
            ..Default::default()
        });
    }
    for prop_def in &template.prop_definitions {
        let is_snippet = prop_def
            .type_annotation
            .as_deref()
            .is_some_and(is_snippet_type_annotation);
        if !is_snippet || !seen.insert(prop_def.name.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label: prop_def.name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: prop_def
                .type_annotation
                .clone()
                .or_else(|| Some("snippet prop".to_string())),
            insert_text: Some(prop_def.name.clone()),
            sort_text: Some(format!("1{}", prop_def.name)),
            ..Default::default()
        });
    }
    if items.is_empty() {
        return None;
    }
    Some(items)
}

/// Find the component name at the cursor position by scanning the template source.
///
/// Scans backward from the cursor to find the most recent `<` that starts a component tag,
/// then verifies the cursor is still inside the opening tag (before `>` or `/>`).
fn find_component_at_cursor(
    offset: usize,
    source: &str,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
) -> Option<String> {
    let bytes = source.as_bytes();

    // Scan backward from cursor to find the nearest `<` that opens a tag
    let mut i = offset;
    while i > 0 {
        i -= 1;
        if bytes[i] == b'<' {
            break;
        }
        // If we hit `>` before `<`, cursor is outside any opening tag
        if bytes[i] == b'>' {
            return None;
        }
    }

    if i >= offset || bytes[i] != b'<' {
        return None;
    }

    // Skip past `<` and optional `/` (closing tags don't have props)
    let tag_start = i + 1;
    if tag_start < source.len() && bytes[tag_start] == b'/' {
        return None; // Closing tag
    }

    // Extract the tag name
    let mut tag_end = tag_start;
    while tag_end < source.len()
        && (bytes[tag_end].is_ascii_alphanumeric()
            || bytes[tag_end] == b'-'
            || bytes[tag_end] == b'_')
    {
        tag_end += 1;
    }

    if tag_end == tag_start {
        return None;
    }

    let tag_name = &source[tag_start..tag_end];

    // Verify cursor is after the tag name (in attribute position)
    if offset <= tag_end {
        return None; // Cursor is on the tag name itself, not in attribute position
    }

    // Check if this tag name is a component (starts with uppercase or matches a known component)
    let is_component = tag_name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
        || template
            .components
            .iter()
            .any(|c| c.name == tag_name || to_kebab_case(&c.name) == tag_name);

    if !is_component {
        return None;
    }

    // Convert kebab-case to PascalCase for matching
    if tag_name.contains('-') {
        let pascal = to_pascal_case(tag_name);
        if template.components.iter().any(|c| c.name == pascal) {
            return Some(pascal);
        }
    }

    // Try direct match
    if template.components.iter().any(|c| c.name == tag_name) {
        return Some(tag_name.to_string());
    }

    // An incomplete opening tag can be absent from the semantic component
    // inventory. Preserve the structurally recognized name so the caller can
    // resolve it through the matching script import.
    Some(tag_name.to_string())
}

/// Convert PascalCase to kebab-case.
fn to_kebab_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert kebab-case to PascalCase for tag-name MATCHING (`my-comp` ↔ `MyComp`).
///
/// Intentionally narrower than `server_utils::to_pascal_case`: it splits ONLY on
/// `-`/`_` and applies no identifier sanitization (no `.`/non-ident separator,
/// no leading-digit guard). This is a kebab↔name casing normalizer for matching
/// an existing tag against a binding name, NOT a filename→import-binding
/// synthesizer, so it must not rewrite `.` or guard digits (that would change
/// match keys).
//
// TODO(follow-up): consolidating the three `to_pascal_case` copies
// (server_utils.rs — the sanitizing import-binding synthesizer; here and
// definition.rs — the narrower kebab↔name matchers) onto one shared helper is a
// nice cleanup but NOT low-risk: the matchers deliberately have different
// separator/digit semantics, so a naive merge would alter match keys. Defer
// until a shared helper can express both modes (e.g. a `sanitize: bool` axis)
// without entangling matching with import synthesis.
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

/// Internal compiler identifiers that should never appear in completions.
fn is_internal_dunder(label: &str) -> bool {
    matches!(
        label,
        "__props" | "__emit" | "__slots" | "__expose" | "__returned"
    )
}

fn macro_kind_label(kind: &verter_semantic::analysis::AnalyzedMacroKind) -> &'static str {
    match kind {
        verter_semantic::analysis::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_semantic::analysis::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_semantic::analysis::AnalyzedMacroKind::DefineExpose => "defineExpose",
        verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => "defineOptions",
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_semantic::analysis::AnalyzedMacroKind::WithDefaults => "withDefaults",
    }
}

/// Returns `true` when the cursor in compiled TSX is in a member access context —
/// either right after a `.`/`?.` or partway through typing an identifier after one
/// (e.g. `foo.`, `foo.te`, `foo?.va`). When true, only the TypeProvider should
/// supply completions (property/method members), not Verter's global bindings.
///
/// `tsx_offset` is the byte offset in `tsx_content` where the cursor maps to.
#[cfg(test)]
pub(crate) fn is_member_access_in_tsx(tsx_content: &str, tsx_offset: u32) -> bool {
    if tsx_offset == 0 {
        return false;
    }
    let bytes = tsx_content.as_bytes();
    let len = bytes.len();
    let mut i = tsx_offset as usize;
    if i > len {
        return false;
    }
    // Skip backward past any partial identifier the user is typing (e.g. `foo.te|`)
    while i > 0
        && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_' || bytes[i - 1] == b'$')
    {
        i -= 1;
    }
    // Skip whitespace between dot and identifier (rare but possible)
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] != b'.' {
        return false;
    }
    // We found a '.'. Check for `..` (spread) — not member access.
    if i >= 2 && bytes[i - 2] == b'.' {
        return false;
    }
    i -= 1; // now pointing at the '.'
            // Check for optional chaining `?.`
    if i > 0 && bytes[i - 1] == b'?' {
        return true;
    }
    // The char before `.` must be an identifier char, `)`, or `]`
    if i > 0 {
        let c = bytes[i - 1];
        c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c == b')' || c == b']'
    } else {
        false
    }
}

#[cfg(test)]
#[allow(clippy::type_complexity)]
#[path = "completion_tests.rs"]
mod completion_tests;
