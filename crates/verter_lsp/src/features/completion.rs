// Phase 2: Completion — template bindings, component names, props from verter_host analysis.
// Phase 3: Enhanced with typed member access, generic inference from TypeProvider.

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::{
    classify_cursor, parse_opening_tag, SfcBlock, SfcCursorContext,
};

/// Result from completion, including an `is_incomplete` flag for re-query behavior.
pub struct CompletionResult {
    pub items: Vec<CompletionItem>,
    pub is_incomplete: bool,
}

/// Provide completions at a given position.
///
/// Strategy:
/// 1. Find which SFC block the position is in
/// 2. For script blocks: offer bindings, imports, Vue APIs
/// 3. For template blocks:
///    a. If cursor is inside a class attribute → offer CSS class completions
///    b. If cursor is inside a component's opening tag → offer component props/events
///    c. Otherwise → offer all available bindings from script setup
///
/// The optional `resolve_component` callback takes an import source (e.g., `./Button.vue`)
/// and returns that component's analysis snapshot, enabling cross-file prop completions.
/// A workspace component available for auto-import.
pub struct WorkspaceComponent {
    /// PascalCase component name (derived from filename).
    pub name: String,
    /// Relative or absolute import path (e.g., `./Button.vue`).
    pub import_path: String,
}

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

    // Check SFC structural context BEFORE requiring analysis data.
    match classify_cursor(offset, blocks) {
        SfcCursorContext::RootLevel => {
            return Some(CompletionResult {
                items: sfc_root_completions(source, blocks),
                is_incomplete: false,
            });
        }
        SfcCursorContext::OpeningTag { block_index } => {
            return Some(CompletionResult {
                items: sfc_attribute_completions(source, &blocks[block_index]),
                is_incomplete: false,
            });
        }
        SfcCursorContext::ClosingTag { .. } => return None,
        SfcCursorContext::BlockContent { .. } => {} // fall through
    }

    let analysis = analysis?;
    let offset = offset as usize;

    // Determine which block the cursor is in
    let block = blocks.iter().find(|b| {
        let (content_start, content_end) = b.content_range();
        offset >= content_start as usize && offset <= content_end as usize
    })?;

    match block.tag_name.as_str() {
        "script" => Some(CompletionResult {
            items: script_completions(analysis),
            is_incomplete: false,
        }),
        "template" => {
            // Check if cursor is after an event modifier dot — offer event modifier completions
            if let Some(result) = event_modifier_completions(offset, source) {
                return Some(result);
            }
            // Check if cursor is inside a class attribute — offer CSS class completions
            if let Some(result) = class_attribute_completions(offset, source, analysis) {
                return Some(result);
            }
            // Check if cursor is inside a component's opening tag — offer prop completions
            if let Some(items) =
                component_prop_completions(offset, source, analysis, resolve_component)
            {
                return Some(CompletionResult {
                    items,
                    is_incomplete: false,
                });
            }
            Some(CompletionResult {
                items: template_completions(analysis, workspace_components, doc_uri),
                is_incomplete: false,
            })
        }
        "style" => {
            crate::css::css_completions(position, source, blocks, Some(analysis), line_index).map(
                |items| CompletionResult {
                    items,
                    is_incomplete: false,
                },
            )
        }
        _ => None,
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

    // Offer existing bindings
    for binding in &analysis.bindings {
        items.push(CompletionItem {
            label: binding.name.clone(),
            kind: Some(binding_completion_kind(&binding.kind)),
            detail: Some(binding_detail(binding)),
            ..Default::default()
        });
    }

    // Offer imports
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
                ..Default::default()
            });
        }
    }

    // Filter out ___VERTER___ internal symbols
    items.retain(|item| !item.label.starts_with("___VERTER___"));

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
    items.retain(|item| !item.label.starts_with("___VERTER___"));

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

/// Detect if cursor is in an event modifier context (`@event.` or `@event.stop.`)
/// and return the event name if so. Scans backward from cursor.
fn detect_event_modifier_context(offset: usize, source: &str) -> Option<&str> {
    let bytes = source.as_bytes();
    if offset == 0 || offset > bytes.len() {
        return None;
    }

    // Scan backward from cursor to find a dot, tracking what we pass over
    let mut i = offset;
    loop {
        if i == 0 {
            return None;
        }
        i -= 1;
        match bytes[i] {
            b'.' => break, // found a dot
            // Hit whitespace, quotes, angle brackets, or equals — not a modifier context
            b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b'<' | b'>' | b'=' => return None,
            _ => {}
        }
    }

    // Found a dot at position `i`. Now scan backward for more dots or the event name.
    // Walk back through any modifier.modifier chain to find the @event
    let mut event_end = i; // end of event name or modifier
    loop {
        // Extract the word before this dot (the modifier or event name)
        let mut j = event_end;
        while j > 0
            && (bytes[j - 1].is_ascii_alphanumeric()
                || bytes[j - 1] == b'-'
                || bytes[j - 1] == b'_')
        {
            j -= 1;
        }

        if j == 0 {
            return None;
        }

        // Check what's before this word
        match bytes[j - 1] {
            b'@' => {
                // Found `@eventname` — return the event name
                return Some(&source[j..event_end]);
            }
            b'.' => {
                // Another dot — continue scanning backward (chained modifiers)
                event_end = j - 1;
            }
            _ => return None,
        }
    }
}

/// Provide event modifier completions when cursor is after `@event.` in a template.
fn event_modifier_completions(offset: usize, source: &str) -> Option<CompletionResult> {
    let event_name = detect_event_modifier_context(offset, source)?;

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

    Some(CompletionResult {
        items,
        is_incomplete: false,
    })
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
    fn test_template_completions_include_bindings() {
        let source = "<template>\n  {{ | }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
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

    /// @ai-generated - Class completions offered inside static class=""
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

    /// @ai-generated - No class completions outside class attribute
    #[test]
    fn test_no_class_completions_outside_class_attr() {
        let source =
            "<template><div id=\"app\"></div></template>\n<style scoped>\n.foo {}\n</style>";
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

    /// @ai-generated - No class completions when no style block
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
            span: verter_span::Span::new(span_start, span_end),
            tag_span_end: span_end,
        }
    }

    /// @ai-generated - Dynamic :class completions offer CSS classes inside inner string
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

    /// @ai-generated - No class completions outside dynamic :class string context
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

    /// @ai-generated — Event modifier completions after @click.
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

    /// @ai-generated — Keyboard event modifier completions include key modifiers
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

    /// @ai-generated — Mouse event modifiers include left/right/middle
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

    /// @ai-generated — No event modifier completions outside event attribute
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

    /// @ai-generated — Chained modifiers (after existing modifier)
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
}
