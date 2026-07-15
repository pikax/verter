//! Completion merge: JSX→Vue attribute transformation, the provider-neutral
//! `verter_resolve` envelope, and the verter↔TypeProvider completion merge.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::type_provider::protocol::{
    self, CompletionInsertTextFormat, CompletionKind, CompletionResult,
};

use super::position::tsx_range_to_carrier_range;

// ── JSX → Vue Attribute Transformation ──────────────────────────────

/// Convert a JSX prop name to a Vue template attribute name.
///
/// Returns `Some(vue_attr)` if a transformation is needed, `None` if the label
/// should be kept as-is (already valid in Vue template).
///
/// Transformations:
/// - `onClick` → `@click` (strip "on", decapitalize, prepend "@")
/// - `onCustomEvent` → `@custom-event` (PascalCase → kebab-case)
/// - `onUpdate:modelValue` → `@update:model-value`
/// - `modelValue` → `model-value` (camelCase → kebab-case)
/// - `tabIndex` → `tab-index`
/// - `class`, `id`, `key`, `ref`, `style` → no change
/// - `data-*`, `aria-*` → no change
pub fn jsx_prop_to_vue_attr(label: &str) -> Option<String> {
    // Already kebab-case or simple lowercase — no transformation
    if label.contains('-') || label.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }

    // Event handler: on* → @*
    if let Some(rest) = label.strip_prefix("on") {
        if rest.is_empty() {
            return None;
        }
        // First char must be uppercase (onClick) or it's "on" itself (not an event)
        let first = rest.chars().next()?;
        if !first.is_ascii_uppercase() && first != 'U' {
            // Not an event handler pattern
            return None;
        }

        // Handle onUpdate:modelValue → @update:model-value
        if let Some(after_update) = rest.strip_prefix("Update:") {
            let kebab = camel_to_kebab(after_update);
            return Some(format!("@update:{}", kebab));
        }

        let kebab = camel_to_kebab(rest);
        return Some(format!("@{}", kebab));
    }

    // camelCase prop → kebab-case (e.g., modelValue → model-value)
    if label.chars().any(|c| c.is_ascii_uppercase()) {
        return Some(camel_to_kebab(label));
    }

    None
}

/// Convert a camelCase or PascalCase string to kebab-case.
fn camel_to_kebab(s: &str) -> String {
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

// ── Completion merge ───────────────────────────────────────────────

/// Internal verter helper prefix that should be filtered from completions.
const VERTER_INTERNAL_PREFIX: &str = "___VERTER___";

/// Internal compiler identifiers that should never appear in completions.
fn is_internal_dunder(label: &str) -> bool {
    matches!(
        label,
        "__props" | "__emit" | "__slots" | "__expose" | "__returned"
    )
}

/// Mint the provider-NEUTRAL `verter_resolve` envelope for a completion item's
/// resolve handle, returning the LSP `data` JSON to stamp onto the item.
///
/// The envelope is namespaced SEPARATELY from the top-level workspace-component
/// `auto_import` data shape (the two never overload one key). It carries the
/// active provider's id, the carrier (generated-TSX) path the resolve must be
/// re-issued against, and the serialized provider-pure resolve key.
/// `completionItem/resolve` validates `provider_id` before routing.
///
/// Returns `None` (no envelope stamped) when:
/// * the handle is NOT actionable — a local member (a `TsserverEntry` with no
///   `source`/`data`) resolves to nothing but lazy detail, so stamping it would
///   be per-keystroke payload bloat and a no-op resolve round-trip (review
///   finding F3). Only an auto-import-capable handle is worth an envelope, OR
/// * there is no carrier `tsx_path` to route the resolve back to.
///
/// This is the single place the envelope is built, shared by [`merge_completions`]
/// and the virtual-file completion path (so a provider auto-import returned on
/// EITHER path resolves identically — review finding F1).
pub(crate) fn mint_resolve_envelope(
    resolve_data: &protocol::CompletionResolveData,
    provider_id: &str,
    tsx_path: Option<&str>,
) -> Option<serde_json::Value> {
    // Only an actionable handle (auto-import) earns an envelope.
    if !resolve_data.is_actionable() {
        return None;
    }
    // No carrier path ⇒ the resolve cannot be routed back to a generated file;
    // drop the handle rather than emit an unroutable envelope.
    let tsx_path = tsx_path?;
    let provider_data = serde_json::to_value(resolve_data).ok()?;
    Some(serde_json::json!({
        "verter_resolve": {
            "kind": "type_provider",
            "provider_id": provider_id,
            "provider_path": tsx_path,
            "provider_data": provider_data,
        }
    }))
}

/// Convert ONE provider [`protocol::Completion`] into an LSP [`CompletionItem`],
/// stamping the provider-neutral `verter_resolve` envelope on an actionable
/// resolve handle.
///
/// `text_edit` is the already-mapped carrier-source edit (the virtual-file path
/// passes `None` because its byte offsets are already in the file the editor
/// shows). `label` is the (possibly JSX→Vue-transformed) display label.
///
/// Shared by [`merge_completions`] and the virtual-file completion path so a
/// provider auto-import completion resolves identically regardless of which path
/// produced the item (review finding F1 — the virtual-file path previously
/// stripped `Completion.data`, so its auto-imports could never resolve).
pub(crate) fn provider_completion_to_lsp_item(
    item: protocol::Completion,
    label: String,
    text_edit: Option<CompletionTextEdit>,
    provider_id: &str,
    tsx_path: Option<&str>,
) -> CompletionItem {
    let data = item
        .data
        .as_ref()
        .and_then(|d| mint_resolve_envelope(d, provider_id, tsx_path));
    CompletionItem {
        label,
        kind: item.kind.map(convert_completion_kind),
        detail: item.detail,
        documentation: item.documentation.map(|d| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: d,
            })
        }),
        sort_text: item.sort_text,
        // When `text_edit` is present the client applies it and ignores this
        // field. When the replace-range was dropped fail-closed (or there was no
        // textEdit), the plain-insert text prefers the dropped edit's
        // `textEdit.newText` over an explicit `insertText`, so the item inserts
        // the intended text rather than the (possibly decorated) display label.
        insert_text: item.text_edit_new_text.or(item.insert_text),
        // Carrier-IDE fidelity: propagate the provider's snippet/commit/filter/
        // preselect/label-detail signals so a mapped completion behaves like the
        // equivalent `.ts` one. Each is `None` when the provider gave no signal
        // (fail-closed — never fabricated here).
        insert_text_format: item.insert_text_format.map(convert_insert_text_format),
        commit_characters: item.commit_characters,
        filter_text: item.filter_text,
        preselect: item.preselect,
        label_details: item.label_details.map(|ld| CompletionItemLabelDetails {
            detail: ld.detail,
            description: ld.description,
        }),
        text_edit,
        data,
        ..Default::default()
    }
}

/// Map the neutral [`CompletionInsertTextFormat`] carrier onto the LSP
/// [`InsertTextFormat`].
fn convert_insert_text_format(format: CompletionInsertTextFormat) -> InsertTextFormat {
    match format {
        CompletionInsertTextFormat::PlainText => InsertTextFormat::PLAIN_TEXT,
        CompletionInsertTextFormat::Snippet => InsertTextFormat::SNIPPET,
    }
}

/// Merge verter completions with TypeProvider completions.
///
/// Strategy:
/// - Combine both lists
/// - Filter out internal `___VERTER___` identifiers from TypeProvider results
/// - Deduplicate by label (verter items take priority for sort ordering)
/// - When `template_attr_context` is true, transform JSX prop names to Vue syntax
///   (e.g., `onClick` → `@click`, `modelValue` → `model-value`)
///
/// `provider_id` is the active provider's [`TypeProvider::provider_id`]. Items
/// that carry an ACTIONABLE resolve handle are stamped with the provider-NEUTRAL
/// `verter_resolve` envelope (kind + provider id + carrier path + the serialized
/// provider resolve key) so `completionItem/resolve` can route back to the
/// originating provider regardless of which backend produced the list. On a
/// label collision the import-capable (actionable-envelope) item is preserved so
/// the auto-import handle is never silently dropped (review finding F2).
// The merge takes the verter items, the provider result, three position
// indices, the carrier path, the provider id, and the template-context flag —
// each is an independent input to one positional merge, not a bundle worth a
// params struct.
#[allow(clippy::too_many_arguments)]
pub fn merge_completions(
    verter_items: Vec<CompletionItem>,
    type_result: CompletionResult,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
    carrier_line_index: &LineIndex,
    tsx_path: Option<&str>,
    provider_id: &str,
    template_attr_context: bool,
) -> (Vec<CompletionItem>, bool) {
    let is_incomplete = type_result.is_incomplete;
    let mut result = verter_items;
    let mut seen_labels: std::collections::HashSet<String> =
        result.iter().map(|i| i.label.clone()).collect();

    for item in type_result.items {
        // Filter internal verter identifiers
        if item.label.starts_with(VERTER_INTERNAL_PREFIX) {
            continue;
        }
        // Filter $V_ prefixed types (string-exported type helpers)
        if item.label.starts_with("$V_") {
            continue;
        }
        // Filter compiler-internal dunder identifiers (__props, __emit, __slots, etc.)
        if is_internal_dunder(&item.label) {
            continue;
        }
        // Apply JSX→Vue transformation when in template attribute context
        let label = if template_attr_context {
            if let Some(vue_label) = jsx_prop_to_vue_attr(&item.label) {
                vue_label
            } else {
                item.label.clone()
            }
        } else {
            item.label.clone()
        };

        // Already seen (from verter or a previous provider item). Do NOT silently
        // discard the incoming item: it may be the one carrying the auto-import
        // resolve handle while the retained item is a plain local with the same
        // label (e.g. a local `computed` shadowing the `vue` auto-import, or two
        // same-label external entries from different `source` modules). Enrich the
        // retained item — upgrade its kind, and ADOPT an actionable resolve
        // envelope when it has none yet — so the import-capable handle survives
        // the dedupe (review finding F2).
        if !seen_labels.insert(label.clone()) {
            if let Some(existing) = result.iter_mut().find(|i| i.label == label) {
                if let Some(tp_kind) = item.kind {
                    if !matches!(tp_kind, CompletionKind::Text) {
                        existing.kind = Some(convert_completion_kind(tp_kind));
                    }
                }
                // Preserve the import-capable handle: if the retained item has no
                // actionable resolve envelope but the incoming one does, move the
                // incoming envelope onto the retained item.
                if !has_actionable_resolve_envelope(existing) {
                    if let Some(envelope) = item
                        .data
                        .as_ref()
                        .and_then(|d| mint_resolve_envelope(d, provider_id, tsx_path))
                    {
                        existing.data = Some(envelope);
                    }
                }
            }
            continue;
        }

        let edit_range =
            if let (Some(start), Some(end)) = (item.edit_range_start, item.edit_range_end) {
                tsx_range_to_carrier_range(start, end, tsx_line_index, mapper, carrier_line_index)
            } else {
                None
            };

        // A SURVIVING replace-range commits the provider's `textEdit.newText`,
        // never an explicit `insertText` (per LSP the editor applies the edit's
        // newText and ignores insertText when a textEdit is present). The range
        // only survives when a textEdit existed, so `text_edit_new_text` is set;
        // the `insert_text` / `label` chain is a defensive last resort.
        let text_edit = edit_range.map(|range| {
            CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: item
                    .text_edit_new_text
                    .clone()
                    .or_else(|| item.insert_text.clone())
                    .unwrap_or(item.label.clone()),
            })
        });

        result.push(provider_completion_to_lsp_item(
            item,
            label,
            text_edit,
            provider_id,
            tsx_path,
        ));
    }

    (result, is_incomplete)
}

/// Whether an LSP completion item already carries an actionable provider-neutral
/// `verter_resolve` envelope (a `type_provider` kind with `provider_data`).
///
/// Used by the dedupe path to decide whether a colliding provider item's handle
/// should be adopted onto the retained item (review finding F2): a retained item
/// that already has the envelope keeps it; one that does not adopts the incoming
/// actionable handle.
fn has_actionable_resolve_envelope(item: &CompletionItem) -> bool {
    item.data
        .as_ref()
        .and_then(|d| d.get("verter_resolve"))
        .map(|e| e.get("kind").and_then(|k| k.as_str()) == Some("type_provider"))
        .unwrap_or(false)
}

fn convert_completion_kind(kind: CompletionKind) -> CompletionItemKind {
    match kind {
        CompletionKind::Function => CompletionItemKind::FUNCTION,
        CompletionKind::Variable => CompletionItemKind::VARIABLE,
        CompletionKind::Property => CompletionItemKind::PROPERTY,
        CompletionKind::Class => CompletionItemKind::CLASS,
        CompletionKind::Interface => CompletionItemKind::INTERFACE,
        CompletionKind::Module => CompletionItemKind::MODULE,
        CompletionKind::Keyword => CompletionItemKind::KEYWORD,
        CompletionKind::Snippet => CompletionItemKind::SNIPPET,
        CompletionKind::Text => CompletionItemKind::TEXT,
        CompletionKind::Method => CompletionItemKind::METHOD,
        CompletionKind::Field => CompletionItemKind::FIELD,
        CompletionKind::Enum => CompletionItemKind::ENUM,
        CompletionKind::EnumMember => CompletionItemKind::ENUM_MEMBER,
        CompletionKind::Constant => CompletionItemKind::CONSTANT,
        CompletionKind::TypeParameter => CompletionItemKind::TYPE_PARAMETER,
        CompletionKind::File => CompletionItemKind::FILE,
        CompletionKind::Folder => CompletionItemKind::FOLDER,
    }
}
