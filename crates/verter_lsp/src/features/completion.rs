// Phase 2: Completion — template bindings, component names, props from verter_host analysis.
// Phase 3: Enhanced with typed member access, generic inference from TypeProvider.
// Phase 4: AST-based cursor context detection via cursor_context module.

use tower_lsp_server::ls_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::{parse_opening_tag, SfcBlock};
use crate::features::cursor_context::{
    classify_cursor_context, CursorContext, ExpressionKind, StyleCursorContext,
    TemplateCursorContext,
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
/// and returns that component's analysis snapshot, enabling cross-file prop completions.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn completions_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    resolve_component: Option<&dyn Fn(&str) -> Option<FileAnalysisSnapshot>>,
    workspace_components: Option<&[WorkspaceComponent]>,
    doc_uri: Option<&str>,
) -> Option<CompletionResult> {
    let offset = line_index.position_to_offset(position)?;

    // Classify cursor using AST-based context detection
    let context = classify_cursor_context(offset, source, blocks, analysis);

    match context {
        CursorContext::RootLevel => Some(CompletionResult {
            items: sfc_root_completions(source, blocks),
            is_incomplete: false,
        }),
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
            Some(CompletionResult {
                items: script_completions(analysis),
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
                TemplateCursorContext::TagName { .. } => Some(CompletionResult {
                    items: tag_name_completions(analysis, workspace_components, doc_uri),
                    is_incomplete: false,
                }),
                TemplateCursorContext::ClosingTagName { .. } => None,
                TemplateCursorContext::AttributeName {
                    tag_name: _,
                    is_component,
                    ..
                } => {
                    // For components, try to offer prop/event completions
                    if is_component {
                        let comp_offset = offset as usize;
                        if let Some(items) = component_prop_completions(
                            comp_offset,
                            source,
                            analysis,
                            resolve_component,
                        ) {
                            return Some(CompletionResult {
                                items,
                                is_incomplete: false,
                            });
                        }
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
                        items: template_completions(analysis, workspace_components, doc_uri),
                        is_incomplete: false,
                    })
                }
                TemplateCursorContext::Interpolation => Some(CompletionResult {
                    items: template_completions(analysis, workspace_components, doc_uri),
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
fn sfc_root_completions(source: &str, blocks: &[SfcBlock]) -> Vec<CompletionItem> {
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
        ));
    }
    if !has_script {
        items.push(snippet_item(
            "script setup",
            "<script setup lang=\"ts\">\n$0\n</script>",
            "Add <script setup> block",
            2,
        ));
        items.push(snippet_item(
            "script",
            "<script lang=\"ts\">\n$0\n</script>",
            "Add <script> block",
            3,
        ));
    }
    if !has_style {
        items.push(snippet_item(
            "style scoped",
            "<style scoped>\n$0\n</style>",
            "Add <style scoped> block",
            4,
        ));
        items.push(snippet_item(
            "style",
            "<style>\n$0\n</style>",
            "Add <style> block",
            5,
        ));
    }

    // Scaffold snippets (only when file is mostly empty)
    if is_empty {
        items.push(snippet_item(
            "vue-ts",
            "<script setup lang=\"ts\">\n$0\n</script>\n\n<template>\n\t\n</template>",
            "Vue SFC scaffold (TypeScript)",
            0,
        ));
        items.push(snippet_item(
            "vue",
            "<script setup>\n$0\n</script>\n\n<template>\n\t\n</template>",
            "Vue SFC scaffold (JavaScript)",
            0,
        ));
        items.push(snippet_item(
            "vue-options",
            "<script lang=\"ts\">\nimport { defineComponent } from 'vue'\n\nexport default defineComponent({\n\t$0\n})\n</script>\n\n<template>\n\t\n</template>",
            "Vue SFC scaffold (Options API)",
            0,
        ));
    }

    // Custom block snippets
    items.push(snippet_item(
        "i18n",
        "<i18n lang=\"${1:json}\">\n$0\n</i18n>",
        "Add <i18n> block",
        6,
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

fn snippet_item(
    label: &str,
    insert_text: &str,
    detail: &str,
    sort_priority: u32,
) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(detail.to_string()),
        insert_text: Some(insert_text.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        sort_text: Some(format!("{:04}", sort_priority)),
        ..Default::default()
    }
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
fn tag_name_completions(
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
                label: comp.name.clone(),
                kind: Some(CompletionItemKind::MODULE),
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
        let existing: std::collections::HashSet<String> =
            items.iter().map(|i| i.label.clone()).collect();
        for comp in ws_components {
            if existing.contains(&comp.name) {
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
                kind: Some(CompletionItemKind::MODULE),
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
    let template = analysis.template.as_ref()?;

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

    for style in &analysis.styles {
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

/// Completions available in `<template>` context.
///
/// Offers all script-setup bindings that are available in the template scope.
fn template_completions(
    analysis: &FileAnalysisSnapshot,
    workspace_components: Option<&[WorkspaceComponent]>,
    doc_uri: Option<&str>,
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
            verter_analysis::ReactivityKind::Ref => Some("ref"),
            verter_analysis::ReactivityKind::Computed => Some("computed"),
            verter_analysis::ReactivityKind::Reactive => Some("reactive"),
            verter_analysis::ReactivityKind::MaybeRef => Some("maybe-ref"),
            verter_analysis::ReactivityKind::Mutable => Some("mutable"),
            verter_analysis::ReactivityKind::None => {
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
    for mac in &analysis.macros {
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
        let existing_labels: std::collections::HashSet<String> =
            items.iter().map(|i| i.label.clone()).collect();
        for comp in ws_components {
            if existing_labels.contains(&comp.name) {
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
                kind: Some(CompletionItemKind::MODULE),
                detail: Some(format!("Auto import from '{}'", comp.import_path)),
                sort_text: Some(format!("z{}", comp.name)), // Sort after local items
                data: Some(data),
                ..Default::default()
            });
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

fn binding_completion_kind(kind: &verter_analysis::AnalyzedBindingKind) -> CompletionItemKind {
    match kind {
        verter_analysis::AnalyzedBindingKind::Const => CompletionItemKind::CONSTANT,
        verter_analysis::AnalyzedBindingKind::Let | verter_analysis::AnalyzedBindingKind::Var => {
            CompletionItemKind::VARIABLE
        }
        verter_analysis::AnalyzedBindingKind::Function
        | verter_analysis::AnalyzedBindingKind::AsyncFunction => CompletionItemKind::FUNCTION,
        verter_analysis::AnalyzedBindingKind::Class => CompletionItemKind::CLASS,
    }
}

fn binding_detail(binding: &verter_analysis::AnalyzedBinding) -> String {
    let kind = match binding.kind {
        verter_analysis::AnalyzedBindingKind::Const => "const",
        verter_analysis::AnalyzedBindingKind::Let => "let",
        verter_analysis::AnalyzedBindingKind::Var => "var",
        verter_analysis::AnalyzedBindingKind::Function => "function",
        verter_analysis::AnalyzedBindingKind::AsyncFunction => "async function",
        verter_analysis::AnalyzedBindingKind::Class => "class",
    };
    kind.to_string()
}

// =============================================================================
// Event Modifier Completions
// =============================================================================

/// Runtime modifiers available for all events.
const RUNTIME_MODIFIERS: &[(&str, &str)] = &[
    ("stop", "Call event.stopPropagation()"),
    ("prevent", "Call event.preventDefault()"),
    ("self", "Only trigger if event.target is the element itself"),
    ("once", "Trigger at most once"),
    ("capture", "Use capture mode for addEventListener"),
    ("passive", "Mark addEventListener as passive"),
];

/// System modifier keys (available for all events).
const SYSTEM_MODIFIERS: &[(&str, &str)] = &[
    ("ctrl", "Require Ctrl key"),
    ("shift", "Require Shift key"),
    ("alt", "Require Alt key"),
    ("meta", "Require Meta/Command key"),
    ("exact", "Require exact modifier combination"),
];

/// Key modifiers for keyboard events (keydown, keyup, keypress).
const KEY_MODIFIERS: &[(&str, &str)] = &[
    ("enter", "Enter key"),
    ("tab", "Tab key"),
    ("delete", "Delete or Backspace key"),
    ("esc", "Escape key"),
    ("space", "Space key"),
    ("up", "Arrow Up"),
    ("down", "Arrow Down"),
    ("left", "Arrow Left (key)"),
    ("right", "Arrow Right (key)"),
    ("page-down", "Page Down"),
    ("page-up", "Page Up"),
    ("home", "Home key"),
    ("end", "End key"),
];

/// Mouse button modifiers (for click, mousedown, mouseup).
const MOUSE_BUTTON_MODIFIERS: &[(&str, &str)] = &[
    ("left", "Left mouse button"),
    ("right", "Right mouse button"),
    ("middle", "Middle mouse button"),
];

/// Provide event modifier completions for a given event name.
/// The event name is determined by the cursor context module.
fn event_modifier_completions_for(event_name: &str) -> CompletionResult {
    let is_keyboard = event_name.starts_with("key");
    let is_mouse_button = matches!(
        event_name,
        "click" | "dblclick" | "mousedown" | "mouseup" | "contextmenu"
    );

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
    resolve_component: Option<&dyn Fn(&str) -> Option<FileAnalysisSnapshot>>,
) -> Option<Vec<CompletionItem>> {
    let template = analysis.template.as_ref()?;

    // Find which component's opening tag contains the cursor
    let component_name = find_component_at_cursor(offset, source, template)?;

    // Find the component usage to get its import source
    let comp_usage = template
        .components
        .iter()
        .find(|c| c.name == component_name || to_kebab_case(&c.name) == component_name)?;

    let import_source = comp_usage.import_source.as_ref()?;

    // Resolve the component's analysis
    let resolve_fn = resolve_component?;
    let child_analysis = resolve_fn(import_source)?;
    let child_template = child_analysis.template.as_ref()?;

    let mut items = Vec::new();

    // Collect already-used prop names to avoid offering duplicates
    let used_props: std::collections::HashSet<String> =
        comp_usage.props.iter().map(|p| p.name.clone()).collect();

    // Props from child component's defineProps
    for prop_def in &child_template.prop_definitions {
        if used_props.contains(&prop_def.name) {
            continue;
        }
        let label = to_kebab_case(&prop_def.name);
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
            // Boolean props can be used without value: <Comp disabled />
            Some(label.clone())
        } else {
            // Suggest binding syntax for non-boolean: :prop="$1"
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
            sort_text: Some(format!("0{}", label)), // Props sort before events
            ..Default::default()
        });
    }

    // Events from child component's defineEmits
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
            sort_text: Some(format!("1{}", event_name)), // Events sort after props
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
    template: &verter_analysis::template::TemplateAnalysisSnapshot,
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

    None
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

/// Convert kebab-case to PascalCase.
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

fn macro_kind_label(kind: &verter_analysis::AnalyzedMacroKind) -> &'static str {
    match kind {
        verter_analysis::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_analysis::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_analysis::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_analysis::AnalyzedMacroKind::DefineExpose => "defineExpose",
        verter_analysis::AnalyzedMacroKind::DefineOptions => "defineOptions",
        verter_analysis::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_analysis::AnalyzedMacroKind::WithDefaults => "withDefaults",
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
#[path = "completion_tests.rs"]
mod completion_tests;
