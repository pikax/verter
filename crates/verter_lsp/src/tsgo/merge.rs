//! Merge logic for combining verter analysis results with TypeProvider results.
//!
//! Each merge function takes verter-only results and TypeProvider results,
//! producing enhanced output. All functions handle the case where either
//! source may be absent (graceful fallback).

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
#[cfg(test)]
use crate::tsgo::protocol::Completion;
use crate::tsgo::protocol::{
    self, CompletionKind, CompletionResult, HoverInfo, InlayHint, InlayHintKind, RenameLocation,
    TypeCodeAction, TypeDiagnostic, TypeDiagnosticSeverity, TypeDocumentHighlight,
    TypeDocumentHighlightKind, TypeLocation,
};
use crate::uri::path_to_file_uri;

/// External IDE context for resolving positions in a foreign `.vue.tsx` file.
///
/// For cross-file navigation (e.g., CTRL+CLICK navigates to another `.vue` file),
/// the merge functions need the target file's TSX line index, position mapper, and
/// Vue line index. This struct carries those, and the resolver closure produces it.
pub struct ExternalIdeContext {
    pub tsx_line_index: LineIndex,
    pub mapper: PositionMapper,
    pub vue_line_index: LineIndex,
}

/// Resolver for looking up IDE context by IDE path (e.g., `/path/to/Comp.vue.tsx`).
///
/// Returns `None` if the file isn't tracked or hasn't been compiled yet.
pub type ExternalIdeResolver<'a> = &'a dyn Fn(&str) -> Option<ExternalIdeContext>;

/// Map an LSP `Position` (in the Vue file) to a byte offset in the generated TSX.
///
/// Steps: LSP Position → byte offset via LineIndex → line/col → PositionMapper → TSX line/col → TSX byte offset via TSX LineIndex.
///
/// Returns `None` if any mapping step fails.
pub fn vue_position_to_tsx_offset(
    position: &Position,
    _vue_line_index: &LineIndex,
    mapper: &PositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    let vue_line = position.line;
    let vue_col = position.character;
    let tsx_pos = mapper.vue_to_tsx(vue_line, vue_col)?;
    tsx_line_index.position_to_offset(&Position {
        line: tsx_pos.line,
        character: tsx_pos.column,
    })
}

/// Map a Vue position to a TSX byte offset, with round-trip validation.
///
/// After mapping Vue→TSX, verifies the TSX offset maps back to the same Vue line.
/// Returns `None` if the round-trip fails (indicating the TSX offset is in a synthetic
/// region like generated JSX for HTML elements, where TSGO queries would crash).
pub fn vue_position_to_tsx_offset_validated(
    position: &Position,
    vue_line_index: &LineIndex,
    mapper: &PositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    let tsx_offset = vue_position_to_tsx_offset(position, vue_line_index, mapper, tsx_line_index)?;

    // Round-trip: TSX offset → TSX position → Vue position
    let tsx_pos = tsx_line_index.offset_to_position(tsx_offset)?;
    let vue_roundtrip = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.character)?;

    // The round-trip Vue position should be on the same line as the original.
    // If not, the TSX offset is in a synthetic region with no valid source correlation.
    if vue_roundtrip.line == position.line {
        Some(tsx_offset)
    } else {
        None
    }
}

/// Map a TSX byte offset range back to an LSP `Range` in the Vue source.
///
/// Returns `None` if any mapping step fails.
pub fn tsx_range_to_vue_range(
    tsx_start: u32,
    tsx_end: u32,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Option<Range> {
    let start_pos = tsx_line_index.offset_to_position(tsx_start)?;
    let end_pos = tsx_line_index.offset_to_position(tsx_end)?;

    let vue_start = mapper.tsx_to_vue(start_pos.line, start_pos.character)?;
    let vue_end = mapper.tsx_to_vue(end_pos.line, end_pos.character)?;

    // Validate the mapped positions produce valid byte offsets
    let start_lsp = Position {
        line: vue_start.line,
        character: vue_start.column,
    };
    let end_lsp = Position {
        line: vue_end.line,
        character: vue_end.column,
    };
    vue_line_index.position_to_offset(&start_lsp)?;
    vue_line_index.position_to_offset(&end_lsp)?;

    Some(Range {
        start: start_lsp,
        end: end_lsp,
    })
}

// ── Hover merge ────────────────────────────────────────────────────

/// Merge verter hover with TypeProvider hover.
///
/// Strategy:
/// - If TypeProvider provides hover info, prepend the type signature to verter's hover content
/// - If only verter provides hover, use it as-is
/// - If only TypeProvider provides hover, use it (mapped back to Vue positions)
pub fn merge_hover(
    verter_hover: Option<Hover>,
    type_hover: Option<HoverInfo>,
    _mapper: &PositionMapper,
    _tsx_line_index: &LineIndex,
    _vue_line_index: &LineIndex,
    vue_kind_label: Option<&str>,
) -> Option<Hover> {
    match (verter_hover, type_hover) {
        (Some(verter), Some(type_info)) => {
            // TSGO provides the richer type signature — strip verter's leading code block
            // to avoid duplicate fenced blocks in the merged hover.
            let verter_text = extract_hover_text(&verter);
            let context = strip_leading_code_block(&verter_text);
            let mut type_block = wrap_type_block(&type_info.contents);
            if let Some(label) = vue_kind_label {
                type_block = replace_kind_prefix(&type_block, label);
            }
            let merged = if context.trim().is_empty() {
                type_block
            } else {
                format!("{}\n---\n{}", type_block, context)
            };
            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: merged,
                }),
                range: verter.range,
            })
        }
        (Some(verter), None) => Some(verter),
        (None, Some(type_info)) => {
            let mut type_block = wrap_type_block(&type_info.contents);
            if let Some(label) = vue_kind_label {
                type_block = replace_kind_prefix(&type_block, label);
            }
            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: type_block,
                }),
                range: None,
            })
        }
        (None, None) => None,
    }
}

/// Wrap type content in a code fence if not already Markdown-formatted.
///
/// TSGO may return content in several forms:
/// - Already wrapped in a code fence with documentation after the closing fence
///   → use as-is to avoid double-fencing
/// - Plain text with type signature and documentation separated by a blank line
///   → wrap only the type part in a fence, keep doc outside
/// - Plain text with type and doc on consecutive lines (single `\n`)
///   → wrap the first line in a fence, keep rest as doc
/// - Plain text with no newlines
///   → wrap everything in a fence (can't reliably split type from doc)
fn wrap_type_block(contents: &str) -> String {
    if contents.starts_with("```") {
        return contents.to_string();
    }

    // Split at first blank line (\n\n) — type signature before, documentation after
    if let Some(idx) = contents.find("\n\n") {
        let type_part = &contents[..idx];
        let doc_part = contents[idx + 2..].trim();
        return if doc_part.is_empty() {
            format!("```typescript\n{type_part}\n```")
        } else {
            format!("```typescript\n{type_part}\n```\n\n{doc_part}")
        };
    }

    // Split at first \n — first line is the type signature, rest is documentation.
    // Type signatures in hover are typically single-line (e.g., "(property) name: Type").
    if let Some(idx) = contents.find('\n') {
        let type_part = &contents[..idx];
        let doc_part = contents[idx + 1..].trim();
        if !doc_part.is_empty() {
            return format!("```typescript\n{type_part}\n```\n\n{doc_part}");
        }
    }

    format!("```typescript\n{contents}\n```")
}

fn extract_hover_text(hover: &Hover) -> String {
    match &hover.contents {
        HoverContents::Markup(m) => m.value.clone(),
        HoverContents::Scalar(MarkedString::String(s)) => s.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value.clone(),
        HoverContents::Array(items) => items
            .iter()
            .map(|item| match item {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => ls.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Strip the leading fenced code block from hover text.
///
/// If `text` starts with ` ```...lang\n...\n``` `, removes that block and returns
/// the remainder. This prevents duplicate code fences when merging TSGO + verter hover.
fn strip_leading_code_block(text: &str) -> &str {
    if let Some(rest) = text.strip_prefix("```") {
        if let Some(end) = rest.find("\n```") {
            let after = &rest[end + 4..];
            return after.trim_start_matches('\n');
        }
    }
    text
}

/// Replace the `({kind})` prefix in a fenced code block with a Vue-specific label.
///
/// E.g., `(const) const count` with vue_label `"ref"` becomes `(ref) const count`.
fn replace_kind_prefix(content: &str, vue_label: &str) -> String {
    // Look for the pattern `({word}) ` or `({words}) ` at the start of a line inside a code fence
    if let Some(fence_end) = content.find("\n```") {
        // Find the first content line after the opening fence
        if let Some(first_nl) = content.find('\n') {
            let after_fence = &content[first_nl + 1..];
            // Check if line starts with `(word) ` pattern
            if after_fence.starts_with('(') {
                if let Some(paren_end) = after_fence.find(") ") {
                    let new_prefix = format!("({vue_label}) ");
                    // Reconstruct: opening fence + new prefix + rest of content
                    let rest_start = first_nl + 1 + paren_end + 2;
                    if rest_start <= fence_end + 1 {
                        return format!(
                            "{}\n{}{}",
                            &content[..first_nl],
                            new_prefix,
                            &content[rest_start..]
                        );
                    }
                }
            }
        }
    }
    content.to_string()
}

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

/// Merge verter completions with TypeProvider completions.
///
/// Strategy:
/// - Combine both lists
/// - Filter out internal `___VERTER___` identifiers from TypeProvider results
/// - Deduplicate by label (verter items take priority for sort ordering)
/// - When `template_attr_context` is true, transform JSX prop names to Vue syntax
///   (e.g., `onClick` → `@click`, `modelValue` → `model-value`)
pub fn merge_completions(
    verter_items: Vec<CompletionItem>,
    type_result: CompletionResult,
    mapper: &PositionMapper,
    tsx_line_index: &LineIndex,
    vue_line_index: &LineIndex,
    tsx_path: Option<&str>,
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

        // Skip if already seen (from verter or a previous TSGO item)
        if !seen_labels.insert(label.clone()) {
            continue;
        }

        let edit_range =
            if let (Some(start), Some(end)) = (item.edit_range_start, item.edit_range_end) {
                tsx_range_to_vue_range(start, end, tsx_line_index, mapper, vue_line_index)
            } else {
                None
            };

        let text_edit = edit_range.map(|range| {
            CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: item.insert_text.clone().unwrap_or(item.label.clone()),
            })
        });

        // Tag TSGO items with marker data for completion resolve
        let data = if item.data.is_some() {
            let mut tagged = serde_json::json!({
                "tsgo": true,
                "original_data": item.data,
            });
            if let Some(p) = tsx_path {
                tagged["tsx_path"] = serde_json::Value::String(p.to_string());
            }
            Some(tagged)
        } else {
            None
        };

        result.push(CompletionItem {
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
            text_edit,
            data,
            ..Default::default()
        });
    }

    (result, is_incomplete)
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

// ── Diagnostics merge ──────────────────────────────────────────────

/// Merge verter diagnostics with TypeProvider diagnostics.
///
/// Strategy:
/// - Verter diagnostics are already in Vue positions
/// - TypeProvider diagnostics are in TSX positions; map back to Vue
/// - Filter out diagnostics that map to unmapped regions (generated code)
pub fn merge_diagnostics(
    verter_diags: Vec<Diagnostic>,
    type_diags: Vec<TypeDiagnostic>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Vec<Diagnostic> {
    let mut result = verter_diags;
    let mut dropped = 0u32;

    for diag in &type_diags {
        let range =
            tsx_range_to_vue_range(diag.start, diag.end, tsx_line_index, mapper, vue_line_index);

        if let Some(range) = range {
            result.push(Diagnostic {
                range,
                severity: Some(convert_severity(diag.severity)),
                code: diag.code.clone().map(NumberOrString::String),
                source: Some("ts".to_string()),
                message: diag.message.clone(),
                ..Default::default()
            });
        } else {
            dropped += 1;
            tracing::debug!(
                "merge_diagnostics: dropped type provider diagnostic (unmapped range) — {:?} at offsets {}..{}",
                diag.message,
                diag.start,
                diag.end,
            );
        }
    }

    if dropped > 0 {
        tracing::debug!(
            "merge_diagnostics: {dropped}/{} type provider diagnostics dropped (unmapped ranges)",
            type_diags.len()
        );
    }

    result
}

fn convert_severity(sev: TypeDiagnosticSeverity) -> DiagnosticSeverity {
    match sev {
        TypeDiagnosticSeverity::Error => DiagnosticSeverity::ERROR,
        TypeDiagnosticSeverity::Warning => DiagnosticSeverity::WARNING,
        TypeDiagnosticSeverity::Info => DiagnosticSeverity::INFORMATION,
        TypeDiagnosticSeverity::Hint => DiagnosticSeverity::HINT,
    }
}

// ── Definition merge ───────────────────────────────────────────────

/// Resolve a `.vue.tsx` target's byte offsets to a Vue source Range.
///
/// Prioritizes the external resolver (which looks up the target file's actual IDE context)
/// over the current file's mapper. Only falls back to the current file's context if
/// no external resolver is provided or the resolver doesn't know about the target.
fn resolve_vue_tsx_range(
    path: &str,
    start: u32,
    end: u32,
    current_tsx_line_index: &LineIndex,
    current_mapper: &PositionMapper,
    current_vue_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
) -> Range {
    // Try external resolver first — it provides the correct mapper for the target file.
    // Without this, cross-file navigation uses the *current* file's mapper, producing
    // wrong positions (e.g., (0,0) or positions from the wrong file).
    if let Some(resolver) = external_resolver {
        if let Some(ctx) = resolver(path) {
            if let Some(range) = tsx_range_to_vue_range(
                start,
                end,
                &ctx.tsx_line_index,
                &ctx.mapper,
                &ctx.vue_line_index,
            ) {
                return range;
            }
        }
    }

    // Fallback: use current file context (works when target is same file being queried)
    tsx_range_to_vue_range(
        start,
        end,
        current_tsx_line_index,
        current_mapper,
        current_vue_line_index,
    )
    .unwrap_or_default()
}

/// Merge verter definition with TypeProvider definitions.
///
/// Strategy:
/// - If verter provides a definition, use it (it's already precise for in-file navigation)
/// - TypeProvider definitions are used for cross-file navigation (import targets, etc.)
/// - Map TypeProvider locations back to Vue positions where applicable
///
/// `external_resolver` is used to resolve positions in `.vue.tsx` files that differ
/// from the current file (cross-file navigation, e.g., CTRL+CLICK on component tag
/// navigates to the target component's file).
pub fn merge_definitions(
    verter_def: Option<GotoDefinitionResponse>,
    type_defs: Vec<TypeLocation>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    document_uri: &Uri,
) -> Option<GotoDefinitionResponse> {
    // If verter provides a definition, prefer it when:
    // - TSGO returned nothing, or
    // - verter resolved cross-file (TSGO often returns *.vue shim declarations)
    //
    // Compare against the actual document URI — the sentinel has already been
    // replaced by the server handler before this function is called.
    if let Some(ref vd) = verter_def {
        let is_cross_file = matches!(vd, GotoDefinitionResponse::Scalar(loc)
            if loc.uri != *document_uri
            && loc.uri.as_str() != crate::features::definition::SAME_FILE_URI);
        let is_same_file =
            matches!(vd, GotoDefinitionResponse::Scalar(loc) if loc.uri == *document_uri);
        if type_defs.is_empty() || is_cross_file || is_same_file {
            return verter_def;
        }
    }

    // If TypeProvider provides definitions, convert them
    if !type_defs.is_empty() {
        let mut locations: Vec<Location> = type_defs
            .into_iter()
            .filter_map(|loc| {
                // TypeProvider returns paths; convert to URIs
                // For .vue files, strip virtual suffixes (.tsx or .d.ts)
                // Also strip .verter/ide/<hash>/ prefix if present
                let normalized = normalize_vue_path(&loc.path);
                let file_path_owned;
                let file_path = if let Some(stripped) = strip_verter_ide_prefix_owned(normalized) {
                    file_path_owned = stripped;
                    &file_path_owned
                } else {
                    normalized
                };
                let uri = path_to_uri(file_path)?;
                // Map TSX byte offsets back to Vue positions for .vue.tsx targets
                // (.vue.d.ts targets use Range::default — no position mapping available)
                let range = if loc.path.ends_with(".vue.tsx") || loc.path.ends_with(".vue.jsx") {
                    resolve_vue_tsx_range(
                        &loc.path,
                        loc.start,
                        loc.end,
                        tsx_line_index,
                        mapper,
                        vue_line_index,
                        external_resolver,
                    )
                } else {
                    Range::default()
                };
                Some(Location { uri, range })
            })
            .collect();

        if locations.is_empty() {
            return verter_def;
        }

        // Deduplicate by URI (multiple .vue.tsx spans normalize to the same .vue)
        let mut seen = std::collections::HashSet::new();
        locations.retain(|loc| seen.insert(loc.uri.clone()));

        // Prefer non-.vue definitions over .vue re-export sites.
        // When CTRL+CLICKing a library symbol (e.g., `onClickOutside` from @vueuse/core),
        // TSGO may return both the real definition (.d.mts) and .vue consumer files.
        let has_non_vue = locations.iter().any(|l| !l.uri.as_str().ends_with(".vue"));
        if has_non_vue {
            locations.retain(|l| !l.uri.as_str().ends_with(".vue"));
        }

        return Some(if locations.len() == 1 {
            GotoDefinitionResponse::Scalar(locations.into_iter().next().unwrap())
        } else {
            GotoDefinitionResponse::Array(locations)
        });
    }

    verter_def
}

/// Normalize a TypeProvider path back to the original Vue file path.
///
/// Strips virtual file suffixes from Verter-generated paths:
/// - `.vue.tsx` / `.vue.jsx` → `.vue` (IDE output)
/// - `.vue.ts` → `.vue` (public API / DTS output)
/// - `.vue.d.ts` → `.vue` (published type declarations)
fn normalize_vue_path(path: &str) -> &str {
    if path.ends_with(".vue.tsx") || path.ends_with(".vue.jsx") {
        &path[..path.len() - 4] // strip .tsx/.jsx
    } else if path.ends_with(".vue.ts") {
        &path[..path.len() - 3] // strip .ts
    } else if path.ends_with(".vue.d.ts") {
        path.trim_end_matches(".d.ts")
    } else {
        path
    }
}

/// Like `normalize_vue_path` but returns an owned String.
/// Used by server.rs for inline path normalization.
pub fn normalize_vue_path_owned(path: &str) -> String {
    normalize_vue_path(path).to_string()
}

/// Strip `.verter/ide/<hash>/` prefix from a provider path.
///
/// Resolves paths like `/project/.verter/ide/a1b2c3d4e5f6g7h8/src/App.vue.tsx`
/// back to `/project/src/App.vue.tsx`.
fn strip_verter_ide_prefix_owned(path: &str) -> Option<String> {
    let marker_fwd = "/.verter/ide/";
    let marker_win = "\\.verter\\ide\\";
    let (pos, marker_len, sep) = if let Some(p) = path.find(marker_fwd) {
        (p, marker_fwd.len(), "/")
    } else if let Some(p) = path.find(marker_win) {
        (p, marker_win.len(), "\\")
    } else {
        return None;
    };

    let after = &path[pos + marker_len..];
    // Skip the 16-char hex hash + separator
    if after.len() > 17 && (after.as_bytes()[16] == b'/' || after.as_bytes()[16] == b'\\') {
        let relative = &after[17..];
        let root = &path[..pos];
        return Some(format!("{root}{sep}{relative}"));
    }
    None
}

/// Convert a file path to a `file://` URI.
///
/// Handles both Windows (`C:/Users/...`) and Unix (`/home/user/...`) paths.
/// Also available as `file_path_to_uri` for use outside this module.
pub fn file_path_to_uri(path: &str) -> Option<Uri> {
    path_to_uri(path)
}

/// Convert a file path to a `file://` URI (internal).
fn path_to_uri(path: &str) -> Option<Uri> {
    path_to_file_uri(path)
}

// ── References merge ────────────────────────────────────────────────

/// Merge verter references with TypeProvider references.
///
/// Strategy:
/// - Combine verter in-file refs with TypeProvider cross-file refs
/// - Map TSX locations back to Vue for same-file targets
/// - Deduplicate by (uri, range.start)
pub fn merge_references(
    verter_refs: Option<Vec<Location>>,
    type_refs: Vec<TypeLocation>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
) -> Option<Vec<Location>> {
    let mut result = verter_refs.unwrap_or_default();

    for loc in &type_refs {
        // For .vue.tsx/.vue.jsx targets, map back to .vue positions
        if loc.path.ends_with(".vue.tsx") || loc.path.ends_with(".vue.jsx") {
            let range = resolve_vue_tsx_range(
                &loc.path,
                loc.start,
                loc.end,
                tsx_line_index,
                mapper,
                vue_line_index,
                external_resolver,
            );
            let vue_path = normalize_vue_path(&loc.path);
            if let Some(uri) = path_to_uri(vue_path) {
                // Deduplicate: skip if we already have a ref at this position
                let dup = result
                    .iter()
                    .any(|r| r.uri == uri && r.range.start == range.start);
                if !dup {
                    result.push(Location { uri, range });
                }
            }
        } else if loc.path.ends_with(".vue.d.ts") || loc.path.ends_with(".vue.ts") {
            // DTS declarations (.vue.d.ts or .vue.ts): strip suffix, use default range
            let vue_path = normalize_vue_path(&loc.path);
            if let Some(uri) = path_to_uri(vue_path) {
                result.push(Location {
                    uri,
                    range: Range::default(),
                });
            }
        } else {
            // Cross-file .ts/.js targets: pass through
            if let Some(uri) = path_to_uri(&loc.path) {
                result.push(Location {
                    uri,
                    range: Range::default(), // offset mapping for external files requires their content
                });
            }
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

// ── Rename merge ────────────────────────────────────────────────────

/// Merge verter rename edits with TypeProvider rename locations.
///
/// Strategy:
/// - Start with verter's same-file WorkspaceEdit
/// - Add TypeProvider's cross-file rename locations as additional TextEdits
/// - Map TSX ranges back to Vue for .vue targets
#[allow(clippy::mutable_key_type)] // Uri has interior mutability but is used as key by tower-lsp API
pub fn merge_rename_locations(
    verter_edit: Option<WorkspaceEdit>,
    type_locations: Vec<RenameLocation>,
    new_name: &str,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
) -> Option<WorkspaceEdit> {
    let mut edit = verter_edit.unwrap_or_else(|| WorkspaceEdit {
        changes: Some(std::collections::HashMap::new()),
        ..Default::default()
    });

    let changes = edit
        .changes
        .get_or_insert_with(std::collections::HashMap::new);

    for loc in &type_locations {
        if loc.path.ends_with(".vue.tsx") || loc.path.ends_with(".vue.jsx") {
            let range = resolve_vue_tsx_range(
                &loc.path,
                loc.start,
                loc.end,
                tsx_line_index,
                mapper,
                vue_line_index,
                external_resolver,
            );
            let vue_path = normalize_vue_path(&loc.path);
            if let Some(uri) = path_to_uri(vue_path) {
                let edits = changes.entry(uri).or_default();
                let dup = edits.iter().any(|e| e.range.start == range.start);
                if !dup {
                    edits.push(TextEdit {
                        range,
                        new_text: new_name.to_string(),
                    });
                }
            }
        } else if loc.path.ends_with(".vue.d.ts") || loc.path.ends_with(".vue.ts") {
            let vue_path = normalize_vue_path(&loc.path);
            if let Some(uri) = path_to_uri(vue_path) {
                let edits = changes.entry(uri).or_default();
                edits.push(TextEdit {
                    range: Range::default(),
                    new_text: new_name.to_string(),
                });
            }
        } else if let Some(uri) = path_to_uri(&loc.path) {
            let edits = changes.entry(uri).or_default();
            edits.push(TextEdit {
                range: Range::default(),
                new_text: new_name.to_string(),
            });
        }
    }

    // Return None if no edits
    if changes.is_empty() {
        None
    } else {
        Some(edit)
    }
}

// ── Document highlights merge ───────────────────────────────────────

/// Merge verter document highlights with TypeProvider highlights.
///
/// Strategy:
/// - Prefer verter's Read/Write distinction
/// - Supplement with TypeProvider highlights that map back to Vue
/// - Deduplicate by range start
pub fn merge_document_highlights(
    verter_highlights: Option<Vec<DocumentHighlight>>,
    type_highlights: Vec<TypeDocumentHighlight>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Option<Vec<DocumentHighlight>> {
    let mut result = verter_highlights.unwrap_or_default();

    for th in type_highlights {
        if let Some(range) =
            tsx_range_to_vue_range(th.start, th.end, tsx_line_index, mapper, vue_line_index)
        {
            let dup = result.iter().any(|h| h.range.start == range.start);
            if !dup {
                result.push(DocumentHighlight {
                    range,
                    kind: Some(match th.kind {
                        TypeDocumentHighlightKind::Read => DocumentHighlightKind::READ,
                        TypeDocumentHighlightKind::Write => DocumentHighlightKind::WRITE,
                        TypeDocumentHighlightKind::Text => DocumentHighlightKind::TEXT,
                    }),
                });
            }
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

// ── Signature help merge ────────────────────────────────────────────

/// Convert TypeProvider signature help to LSP SignatureHelp.
///
/// No verter equivalent exists; this is a direct conversion from protocol types.
pub fn merge_signature_help(
    type_sig: Option<protocol::SignatureHelp>,
) -> Option<tower_lsp_server::ls_types::SignatureHelp> {
    let sig = type_sig?;
    Some(tower_lsp_server::ls_types::SignatureHelp {
        signatures: sig
            .signatures
            .into_iter()
            .map(|s| SignatureInformation {
                label: s.label,
                documentation: s.documentation.map(|d| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: d,
                    })
                }),
                parameters: Some(
                    s.parameters
                        .into_iter()
                        .map(|p| ParameterInformation {
                            label: ParameterLabel::Simple(p.label),
                            documentation: p.documentation.map(|d| {
                                Documentation::MarkupContent(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: d,
                                })
                            }),
                        })
                        .collect(),
                ),
                active_parameter: None,
            })
            .collect(),
        active_signature: sig.active_signature,
        active_parameter: sig.active_parameter,
    })
}

// ── Code actions merge ──────────────────────────────────────────────

/// Convert TypeProvider code actions to LSP CodeActions.
///
/// Maps edits from TSX positions back to Vue positions.
/// Filters out actions whose edits don't map back to Vue.
#[allow(clippy::mutable_key_type)] // Uri has interior mutability but is used as key by tower-lsp API
pub fn merge_code_actions(
    type_actions: Vec<TypeCodeAction>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Vec<CodeActionOrCommand> {
    type_actions
        .into_iter()
        .filter_map(|action| {
            let mut changes: std::collections::HashMap<Uri, Vec<TextEdit>> =
                std::collections::HashMap::new();

            for edit in action.edits {
                if edit.path.ends_with(".vue.tsx") || edit.path.ends_with(".vue.jsx") {
                    if let Some(range) = tsx_range_to_vue_range(
                        edit.start,
                        edit.end,
                        tsx_line_index,
                        mapper,
                        vue_line_index,
                    ) {
                        let vue_path = normalize_vue_path(&edit.path);
                        if let Some(uri) = path_to_uri(vue_path) {
                            changes.entry(uri).or_default().push(TextEdit {
                                range,
                                new_text: edit.new_text,
                            });
                        }
                    }
                } else if edit.path.ends_with(".vue.d.ts") || edit.path.ends_with(".vue.ts") {
                    let vue_path = normalize_vue_path(&edit.path);
                    if let Some(uri) = path_to_uri(vue_path) {
                        changes.entry(uri).or_default().push(TextEdit {
                            range: Range::default(),
                            new_text: edit.new_text,
                        });
                    }
                } else if let Some(uri) = path_to_uri(&edit.path) {
                    changes.entry(uri).or_default().push(TextEdit {
                        range: Range::default(),
                        new_text: edit.new_text,
                    });
                }
            }

            if changes.is_empty() {
                return None;
            }

            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: action.title,
                kind: action.kind.map(CodeActionKind::from),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                ..Default::default()
            }))
        })
        .collect()
}

// ── Semantic tokens merge ───────────────────────────────────────────

/// Convert TypeProvider semantic tokens to LSP semantic tokens.
///
/// Maps each token's TSX start offset to Vue position.
/// Re-encodes as delta-encoded sequence. Filters tokens in unmapped regions.
pub fn merge_semantic_tokens(
    type_tokens: Vec<protocol::SemanticToken>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Vec<tower_lsp_server::ls_types::SemanticToken> {
    // Map each token from TSX to Vue positions, filtering unmapped ones
    let mut mapped: Vec<(u32, u32, u32, u32, u32)> = Vec::new(); // (line, char, length, type, mods)

    for token in type_tokens {
        let start_pos = tsx_line_index.offset_to_position(token.start);

        if let Some(start_lsp) = start_pos {
            if let Some(vs) = mapper.tsx_to_vue(start_lsp.line, start_lsp.character) {
                // Validate start offset is within the Vue source
                let start_lsp_pos = Position {
                    line: vs.line,
                    character: vs.column,
                };
                if vue_line_index.position_to_offset(&start_lsp_pos).is_none() {
                    continue;
                }

                // Map end position too to compute correct Vue-space length.
                // If the end maps to a different line, filter the token out.
                let end_offset = token.start + token.length;
                let vue_length =
                    if let Some(end_lsp) = tsx_line_index.offset_to_position(end_offset) {
                        if let Some(ve) = mapper.tsx_to_vue(end_lsp.line, end_lsp.character) {
                            if ve.line == vs.line && ve.column >= vs.column {
                                ve.column - vs.column
                            } else {
                                // Cross-line or backward mapping — skip token
                                continue;
                            }
                        } else {
                            // End doesn't map — fall back to original length
                            token.length
                        }
                    } else {
                        // End offset out of TSX bounds — skip token
                        continue;
                    };

                // Skip zero-length tokens (collapsed by mapping)
                if vue_length == 0 {
                    continue;
                }

                mapped.push((
                    vs.line,
                    vs.column,
                    vue_length,
                    token.token_type,
                    token.token_modifiers,
                ));
            }
        }
    }

    // Sort by (line, character) for correct delta encoding
    mapped.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    // Delta-encode
    let mut result = Vec::with_capacity(mapped.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for (line, character, length, token_type, token_modifiers) in mapped {
        let delta_line = line - prev_line;
        let delta_start = if delta_line > 0 {
            character
        } else {
            character - prev_start
        };

        result.push(tower_lsp_server::ls_types::SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: token_modifiers,
        });

        prev_line = line;
        prev_start = character;
    }

    result
}

// ── Inlay hints merge ─────────────────────────────────────────────

/// Map TypeProvider inlay hints from TSX positions back to Vue positions.
///
/// Each hint position (byte offset in TSX) is mapped through the sourcemap
/// back to the Vue source. Hints that fall in generated code (no mapping)
/// are filtered out.
pub fn merge_inlay_hints(
    type_hints: Vec<InlayHint>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Vec<tower_lsp_server::ls_types::InlayHint> {
    let mut result = Vec::with_capacity(type_hints.len());

    for hint in type_hints {
        // Convert TSX byte offset → TSX line/col
        let Some(tsx_pos) = tsx_line_index.offset_to_position(hint.position) else {
            continue;
        };

        // Map TSX line/col → Vue line/col via sourcemap
        let Some(vue_mapped) = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.character) else {
            continue;
        };

        let vue_pos = Position {
            line: vue_mapped.line,
            character: vue_mapped.column,
        };

        // Validate the Vue position is within bounds
        if vue_line_index.position_to_offset(&vue_pos).is_none() {
            continue;
        }

        let kind = hint.kind.map(|k| match k {
            InlayHintKind::Type => tower_lsp_server::ls_types::InlayHintKind::TYPE,
            InlayHintKind::Parameter => tower_lsp_server::ls_types::InlayHintKind::PARAMETER,
        });

        result.push(tower_lsp_server::ls_types::InlayHint {
            position: vue_pos,
            label: tower_lsp_server::ls_types::InlayHintLabel::String(hint.label),
            kind,
            text_edits: None,
            tooltip: None,
            padding_left: hint.padding_left,
            padding_right: hint.padding_right,
            data: None,
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Position mapping tests ─────────────────────────────────────

    fn make_mapper_and_indexes() -> (PositionMapper, LineIndex, LineIndex) {
        // Vue source (line 0-1: template, line 3-4: script)
        let vue_source = "<template>\n  <div>{{ msg }}</div>\n</template>\n\n<script setup>\nconst msg = \"hello\";\n</script>";
        // TSX source (script at line 0)
        let tsx_source = "const msg = \"hello\";\n";

        // Source map: TSX line 0 col 0 → Vue line 5 col 0
        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        let source_id = builder.set_source_and_content("App.vue", vue_source);
        builder.add_token(0, 0, 5, 0, Some(source_id), None);
        builder.add_token(0, 6, 5, 6, Some(source_id), None);
        builder.add_token(0, 10, 5, 10, Some(source_id), None);
        let json = builder.into_sourcemap().to_json_string();

        let mapper = PositionMapper::from_json(&json).unwrap();
        let vue_li = LineIndex::new_utf16(vue_source);
        let tsx_li = LineIndex::new_utf16(tsx_source);

        (mapper, vue_li, tsx_li)
    }

    /// @ai-generated — Vue position maps to correct TSX byte offset
    #[test]
    fn vue_position_maps_to_tsx_offset() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // Vue line 5, col 6 ("msg") → TSX line 0, col 6 → byte offset 6
        let offset = vue_position_to_tsx_offset(
            &Position {
                line: 5,
                character: 6,
            },
            &vue_li,
            &mapper,
            &tsx_li,
        );
        assert_eq!(offset, Some(6));
    }

    /// @ai-generated — Unmappable Vue position returns None
    #[test]
    fn unmappable_vue_position_returns_none() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // Line 0 is in the template, not mapped in our source map
        let offset = vue_position_to_tsx_offset(
            &Position {
                line: 0,
                character: 0,
            },
            &vue_li,
            &mapper,
            &tsx_li,
        );
        assert!(offset.is_none());
    }

    // ── Hover merge tests ──────────────────────────────────────────

    fn make_verter_hover(text: &str) -> Hover {
        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: text.to_string(),
            }),
            range: None,
        }
    }

    /// @ai-generated — Both verter and type hover are merged
    #[test]
    fn merge_hover_both_present() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = make_verter_hover("**msg** (SetupConst)");
        let type_hover = HoverInfo {
            contents: "const msg: string".to_string(),
            range_start: None,
            range_end: None,
        };

        let result = merge_hover(
            Some(verter),
            Some(type_hover),
            &mapper,
            &tsx_li,
            &vue_li,
            None,
        );
        let text = extract_hover_text(&result.unwrap());
        assert!(text.contains("const msg: string"));
        assert!(text.contains("SetupConst"));
    }

    /// @ai-generated — Only verter hover present
    #[test]
    fn merge_hover_verter_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = make_verter_hover("**msg** (SetupConst)");

        let result = merge_hover(Some(verter), None, &mapper, &tsx_li, &vue_li, None);
        assert!(result.is_some());
        let text = extract_hover_text(&result.unwrap());
        assert!(text.contains("SetupConst"));
    }

    /// @ai-generated — Only type hover present
    #[test]
    fn merge_hover_type_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let type_hover = HoverInfo {
            contents: "const msg: string".to_string(),
            range_start: None,
            range_end: None,
        };

        let result = merge_hover(None, Some(type_hover), &mapper, &tsx_li, &vue_li, None);
        assert!(result.is_some());
        let text = extract_hover_text(&result.unwrap());
        assert!(text.contains("const msg: string"));
    }

    /// @ai-generated — Neither hover present returns None
    #[test]
    fn merge_hover_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_hover(None, None, &mapper, &tsx_li, &vue_li, None);
        assert!(result.is_none());
    }

    // ── Completion merge tests ─────────────────────────────────────

    fn make_verter_completion(label: &str) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            ..Default::default()
        }
    }

    fn make_type_completion(label: &str) -> Completion {
        Completion {
            label: label.to_string(),
            kind: Some(CompletionKind::Variable),
            detail: None,
            documentation: None,
            edit_range_start: None,
            edit_range_end: None,
            insert_text: None,
            sort_text: None,
            data: None,
        }
    }

    /// @ai-generated — TypeProvider completions are added alongside verter completions
    #[test]
    fn merge_completions_combines_both() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![make_type_completion("count"), make_type_completion("name")],
            is_incomplete: false,
        };

        let (result, is_incomplete) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li, None, false);
        assert_eq!(result.len(), 3);
        assert!(!is_incomplete);
        let labels: Vec<&str> = result.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"msg"));
        assert!(labels.contains(&"count"));
        assert!(labels.contains(&"name"));
    }

    /// @ai-generated — Duplicate labels are deduplicated (verter wins)
    #[test]
    fn merge_completions_deduplicates() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![make_type_completion("msg")], // duplicate
            is_incomplete: false,
        };

        let (result, _) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li, None, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "msg");
    }

    /// @ai-generated — ___VERTER___ prefixed completions are filtered
    #[test]
    fn merge_completions_filters_verter_internal() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("msg"),
                make_type_completion("___VERTER___hidden"),
            ],
            is_incomplete: false,
        };

        let (result, _) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li, None, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "msg");
    }

    /// @ai-generated — is_incomplete flag is propagated from TypeProvider result
    #[test]
    fn merge_completions_propagates_is_incomplete() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![make_type_completion("count")],
            is_incomplete: true,
        };

        let (result, is_incomplete) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li, None, false);
        assert_eq!(result.len(), 2);
        assert!(
            is_incomplete,
            "is_incomplete should be propagated from TSGO"
        );
    }

    /// @ai-generated — $V_ prefixed type helpers are filtered
    #[test]
    fn merge_completions_filters_dollar_v_prefix() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("msg"),
                make_type_completion("$V_EmitsToProps"),
            ],
            is_incomplete: false,
        };

        let (result, _) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li, None, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "msg");
    }

    /// @ai-generated — TSGO-internal duplicates are deduplicated
    #[test]
    fn merge_completions_deduplicates_tsgo_internal() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("onMounted"), // local binding
                make_type_completion("onMounted"), // auto-import suggestion (same label)
            ],
            is_incomplete: false,
        };

        let (result, _) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li, None, false);
        let on_mounted_count = result.iter().filter(|i| i.label == "onMounted").count();
        assert_eq!(
            on_mounted_count, 1,
            "TSGO-internal duplicates should be deduplicated"
        );
        assert_eq!(result.len(), 2); // msg + onMounted
    }

    /// @ai-generated — Labels present in both verter and TSGO are deduplicated (verter wins)
    #[test]
    fn merge_completions_deduplicates_across_all_sources() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("onMounted")];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("onMounted"), // TSGO local
                make_type_completion("onMounted"), // TSGO auto-import
                make_type_completion("ref"),       // unique
            ],
            is_incomplete: false,
        };

        let (result, _) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li, None, false);
        let on_mounted_count = result.iter().filter(|i| i.label == "onMounted").count();
        assert_eq!(
            on_mounted_count, 1,
            "onMounted should appear exactly once (from verter)"
        );
        assert_eq!(result.len(), 2); // onMounted + ref
    }

    /// Internal compiler identifiers like __props, __emit should be filtered
    #[test]
    fn merge_completions_filters_dunder_internal() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("msg"),
                make_type_completion("__props"),
                make_type_completion("__emit"),
                make_type_completion("__slots"),
                make_type_completion("__expose"),
            ],
            is_incomplete: false,
        };

        let (result, _) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li, None, false);
        let labels: Vec<&str> = result.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["msg"],
            "should filter __props, __emit, __slots, __expose"
        );
    }

    // ── Diagnostics merge tests ────────────────────────────────────

    fn make_verter_diagnostic(msg: &str) -> Diagnostic {
        Diagnostic {
            range: Range::default(),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("verter".to_string()),
            message: msg.to_string(),
            ..Default::default()
        }
    }

    /// @ai-generated — Type diagnostics are mapped and added to verter diagnostics
    #[test]
    fn merge_diagnostics_combines_both() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_diagnostic("parse error")];
        let types = vec![TypeDiagnostic {
            message: "Type 'number' is not assignable to type 'string'".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: 6, // TSX offset for "msg"
            end: 9,
            code: Some("2322".to_string()),
        }];

        let result = merge_diagnostics(verter, types, &tsx_li, &mapper, &vue_li);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source.as_deref(), Some("verter"));
        assert_eq!(result[1].source.as_deref(), Some("ts"));
        assert!(result[1].message.contains("not assignable"));
    }

    /// @ai-generated — Type diagnostics in unmapped regions are filtered out
    #[test]
    fn merge_diagnostics_filters_unmapped() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        // Offset 100 is beyond the TSX source
        let types = vec![TypeDiagnostic {
            message: "error in generated code".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: 100,
            end: 110,
            code: None,
        }];

        let result = merge_diagnostics(verter, types, &tsx_li, &mapper, &vue_li);
        assert!(result.is_empty(), "unmapped diagnostics should be filtered");
    }

    // ── Definition merge tests ─────────────────────────────────────

    fn test_doc_uri() -> Uri {
        "file:///test.vue".parse().unwrap()
    }

    /// @ai-generated — Verter definition is preferred when no type definitions
    #[test]
    fn merge_definitions_verter_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(GotoDefinitionResponse::Scalar(Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range::default(),
        }));

        let result = merge_definitions(
            verter,
            vec![],
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(result.is_some());
    }

    /// @ai-generated — Type definitions used when verter has none
    #[test]
    fn merge_definitions_type_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let types = vec![TypeLocation {
            path: "/project/utils.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_definitions(
            None,
            types,
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(result.is_some());
    }

    /// @ai-generated — Neither source returns None
    #[test]
    fn merge_definitions_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_definitions(
            None,
            vec![],
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(result.is_none());
    }

    // ── path_to_uri tests ──────────────────────────────────────────

    /// @ai-generated — Unix path converted correctly
    #[test]
    fn path_to_uri_unix() {
        let uri = path_to_uri("/home/user/project/App.vue").unwrap();
        assert_eq!(uri.as_str(), "file:///home/user/project/App.vue");
    }

    /// @ai-generated — Windows path converted correctly
    #[test]
    fn path_to_uri_windows() {
        let uri = path_to_uri("C:/Users/dev/project/App.vue").unwrap();
        assert_eq!(uri.as_str(), "file:///C:/Users/dev/project/App.vue");
    }

    // ── References merge tests ────────────────────────────────────────

    /// @ai-generated — TypeProvider references are merged with verter refs
    #[test]
    fn merge_references_both_present() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(vec![Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range::default(),
        }]);
        let type_refs = vec![TypeLocation {
            path: "/project/utils.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_references(verter, type_refs, &tsx_li, &mapper, &vue_li, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 2);
    }

    /// @ai-generated — Empty refs from both returns None
    #[test]
    fn merge_references_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_references(None, vec![], &tsx_li, &mapper, &vue_li, None);
        assert!(result.is_none());
    }

    /// @ai-generated — Verter-only refs returned as-is
    #[test]
    fn merge_references_verter_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(vec![Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range::default(),
        }]);

        let result = merge_references(verter, vec![], &tsx_li, &mapper, &vue_li, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    // ── Document highlights merge tests ───────────────────────────────

    /// @ai-generated — Type highlights mapped and merged with verter highlights
    #[test]
    fn merge_highlights_both_present() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(vec![DocumentHighlight {
            range: Range {
                start: Position {
                    line: 5,
                    character: 6,
                },
                end: Position {
                    line: 5,
                    character: 9,
                },
            },
            kind: Some(DocumentHighlightKind::READ),
        }]);
        // TSX offset 6-9 maps to Vue line 5, col 6-9
        let type_highlights = vec![TypeDocumentHighlight {
            start: 6,
            end: 9,
            kind: TypeDocumentHighlightKind::Write,
        }];

        let result = merge_document_highlights(verter, type_highlights, &tsx_li, &mapper, &vue_li);
        assert!(result.is_some());
        // Should be 1 (deduplicated since both point to line 5, col 6)
        assert_eq!(result.unwrap().len(), 1);
    }

    /// @ai-generated — Neither highlights returns None
    #[test]
    fn merge_highlights_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_document_highlights(None, vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_none());
    }

    // ── Signature help merge tests ────────────────────────────────────

    /// @ai-generated — TypeProvider signature help is converted to LSP type
    #[test]
    fn merge_signature_help_present() {
        let sig = protocol::SignatureHelp {
            signatures: vec![protocol::SignatureInfo {
                label: "fn(x: number): void".to_string(),
                documentation: Some("A test function".to_string()),
                parameters: vec![protocol::ParameterInfo {
                    label: "x".to_string(),
                    documentation: Some("The number param".to_string()),
                }],
            }],
            active_signature: Some(0),
            active_parameter: Some(0),
        };

        let result = merge_signature_help(Some(sig));
        assert!(result.is_some());
        let help = result.unwrap();
        assert_eq!(help.signatures.len(), 1);
        assert_eq!(help.signatures[0].label, "fn(x: number): void");
        assert_eq!(help.active_signature, Some(0));
    }

    /// @ai-generated — None input returns None
    #[test]
    fn merge_signature_help_none() {
        assert!(merge_signature_help(None).is_none());
    }

    // ── Code actions merge tests ──────────────────────────────────────

    /// @ai-generated — Code actions with mappable edits are returned
    #[test]
    fn merge_code_actions_with_edits() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let actions = vec![TypeCodeAction {
            title: "Add missing import".to_string(),
            kind: Some("quickfix".to_string()),
            edits: vec![protocol::TypeCodeEdit {
                path: "/test.vue.tsx".to_string(),
                start: 0,
                end: 0,
                new_text: "import { ref } from 'vue';\n".to_string(),
            }],
        }];

        let result = merge_code_actions(actions, &tsx_li, &mapper, &vue_li);
        assert_eq!(result.len(), 1);
    }

    /// @ai-generated — Empty actions returns empty vec
    #[test]
    fn merge_code_actions_empty() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_code_actions(vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_empty());
    }

    // ── Semantic tokens merge tests ───────────────────────────────────

    /// @ai-generated — Semantic tokens mapped from TSX to Vue
    #[test]
    fn merge_semantic_tokens_basic() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        // Token at TSX offset 6 (= "msg"), length 3
        let tokens = vec![protocol::SemanticToken {
            start: 6,
            length: 3,
            token_type: 8, // VARIABLE
            token_modifiers: 0,
        }];

        let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &vue_li);
        assert_eq!(result.len(), 1);
        // Should map to Vue line 5, col 6
        assert_eq!(result[0].length, 3);
        assert_eq!(result[0].token_type, 8);
    }

    /// @ai-generated — Empty tokens returns empty vec
    #[test]
    fn merge_semantic_tokens_empty() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_semantic_tokens(vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_empty());
    }

    /// Semantic token length should be computed in Vue coordinates by mapping both
    /// start and end positions, not by preserving the TSX length verbatim.
    /// When the TSX text differs in length from Vue text (e.g., `__props` vs `$props`),
    /// the raw TSX length would be wrong in Vue space.
    #[test]
    fn merge_semantic_tokens_length_via_end_mapping() {
        // Vue: line 5 has `const msg = "hello";` (col 6 = 'msg', length 3)
        // TSX: line 0 has `const msg = "hello";` (col 6 = 'msg', length 3)
        // In this case TSX and Vue lengths match, but the mechanism should map end too.
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let tokens = vec![protocol::SemanticToken {
            start: 6,  // TSX offset of 'msg'
            length: 3, // length in TSX = 3
            token_type: 8,
            token_modifiers: 0,
        }];

        let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &vue_li);
        assert_eq!(result.len(), 1);
        // Both start AND end should be mapped — length should be 3 in Vue coordinates
        assert_eq!(
            result[0].length, 3,
            "length should be correct in Vue coordinates"
        );
    }

    /// Token whose end position maps to a different line should be filtered out.
    #[test]
    fn merge_semantic_tokens_cross_line_filtered() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // TSX: "const msg = \"hello\";\n" (20 chars)
        // Token spanning from col 0 with excessive length that crosses line boundary
        let tokens = vec![protocol::SemanticToken {
            start: 0,
            length: 100, // way past end of line — would cross line boundaries
            token_type: 8,
            token_modifiers: 0,
        }];

        let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &vue_li);
        // Should be filtered out because end position mapping crosses line or is out of bounds
        // (or length should be clamped to line end)
        if !result.is_empty() {
            assert!(
                result[0].length < 100,
                "excessive length should be clamped or token filtered, got length {}",
                result[0].length
            );
        }
    }

    // ── Rename merge tests ────────────────────────────────────────────

    /// @ai-generated — Verter-only rename returns as-is
    #[test]
    fn merge_rename_verter_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(WorkspaceEdit {
            changes: Some({
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "file:///test.vue".parse().unwrap(),
                    vec![TextEdit {
                        range: Range::default(),
                        new_text: "newName".to_string(),
                    }],
                );
                m
            }),
            ..Default::default()
        });

        let result =
            merge_rename_locations(verter, vec![], "newName", &tsx_li, &mapper, &vue_li, None);
        assert!(result.is_some());
    }

    /// @ai-generated — Empty rename from both returns None
    #[test]
    fn merge_rename_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result =
            merge_rename_locations(None, vec![], "newName", &tsx_li, &mapper, &vue_li, None);
        assert!(result.is_none());
    }

    // ── Definition merge tests (Bug 2) ───────────────────────────────

    /// @ai-generated — merge_definitions maps .vue.tsx offsets to correct Vue positions
    ///
    /// This tests Bug 2: merge_definitions was returning Range::default() (0,0)-(0,0)
    /// for all .vue.tsx targets instead of mapping TSX byte offsets back to Vue positions.
    #[test]
    fn merge_definitions_maps_vue_tsx_to_vue_positions() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // Simulate TSGO returning a definition in App.vue.tsx
        // TSX offset 6..9 = "msg" (in "const msg = ...")
        // This should map back to Vue line 5, col 6..9
        let type_defs = vec![TypeLocation {
            path: "/home/user/App.vue.tsx".to_string(),
            start: 6,
            end: 9,
        }];

        let result = merge_definitions(
            None,
            type_defs,
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(result.is_some(), "Expected definition response");

        match result.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                // URI should point to .vue (not .vue.tsx)
                assert!(
                    loc.uri.as_str().ends_with("App.vue"),
                    "URI should be .vue, got: {}",
                    loc.uri.as_str()
                );
                // Range should NOT be (0,0)-(0,0) — that's the bug
                assert_ne!(
                    loc.range,
                    Range::default(),
                    "Definition range should not be (0,0)-(0,0) — \
                     TSX offsets must be mapped to Vue positions"
                );
                // The start should be on Vue line 5 (where "const msg = ..." is)
                assert_eq!(
                    loc.range.start.line, 5,
                    "Expected Vue line 5 for 'msg', got line {}",
                    loc.range.start.line
                );
            }
            GotoDefinitionResponse::Array(locs) => {
                assert_eq!(locs.len(), 1);
                assert_ne!(
                    locs[0].range,
                    Range::default(),
                    "Definition range should not be (0,0)-(0,0)"
                );
            }
            _ => panic!("Unexpected definition response type"),
        }
    }

    /// @ai-generated — merge_definitions passes through non-.vue targets unchanged
    #[test]
    fn merge_definitions_non_vue_targets_unchanged() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let type_defs = vec![TypeLocation {
            path: "/home/user/utils.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_definitions(
            None,
            type_defs,
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(result.is_some());
    }

    /// Regression: when verter resolves to a same-file import and TSGO resolves
    /// to an external file (e.g., runtime-dom.d.ts), TSGO's cross-file result
    /// must win — verter's same-file import is just an intermediate step.
    #[test]
    fn merge_definitions_tsgo_external_overrides_verter_same_file() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // Verter found the import statement (same file — uses SAME_FILE_URI sentinel)
        let verter_def = Some(GotoDefinitionResponse::Scalar(Location {
            uri: crate::features::definition::SAME_FILE_URI.parse().unwrap(),
            range: Range {
                start: Position {
                    line: 1,
                    character: 0,
                },
                end: Position {
                    line: 1,
                    character: 20,
                },
            },
        }));

        // TSGO resolved to an external .d.ts file
        let type_defs = vec![TypeLocation {
            path: "/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts".to_string(),
            start: 100,
            end: 120,
        }];

        let result = merge_definitions(
            verter_def,
            type_defs,
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(result.is_some(), "should return TSGO's external definition");

        match result.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.as_str().contains("runtime-dom.d.ts"),
                    "should navigate to external .d.ts file, got: {}",
                    loc.uri.as_str()
                );
                // Negative: must NOT be the same-file sentinel URI
                assert!(
                    !loc.uri
                        .as_str()
                        .contains(crate::features::definition::SAME_FILE_URI),
                    "must not return same-file sentinel when TSGO has external target"
                );
            }
            _ => panic!("Expected scalar definition for single external target"),
        }
    }

    /// @ai-generated — merge_definitions prefers verter when type_defs is empty
    #[test]
    fn merge_definitions_verter_preferred_when_no_type_defs() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let verter_def = Some(GotoDefinitionResponse::Scalar(Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range {
                start: Position {
                    line: 5,
                    character: 6,
                },
                end: Position {
                    line: 5,
                    character: 9,
                },
            },
        }));

        let result = merge_definitions(
            verter_def,
            vec![],
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(result.is_some());
        match result.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.range.start.line, 5);
                assert_eq!(loc.range.start.character, 6);
            }
            _ => panic!("Expected scalar definition"),
        }
    }

    /// When verter provides a same-file definition (URI == document_uri) and
    /// the type provider also returns results for the same .vue.tsx file,
    /// verter should be preferred — its analysis spans are precise, while
    /// the type provider's .vue.tsx byte offsets may fail position mapping.
    #[test]
    fn merge_definitions_prefers_verter_same_file_over_type_provider() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // Verter resolved to line 5 in the same file (sentinel already replaced)
        let verter_def = Some(GotoDefinitionResponse::Scalar(Location {
            uri: test_doc_uri(),
            range: Range {
                start: Position {
                    line: 5,
                    character: 6,
                },
                end: Position {
                    line: 5,
                    character: 12,
                },
            },
        }));

        // Type provider also returns a result for the same file's .vue.tsx
        // (position mapping will fail → would produce (0,0))
        let type_defs = vec![TypeLocation {
            path: "/test.vue.tsx".to_string(),
            start: 999,
            end: 1010,
        }];

        let result = merge_definitions(
            verter_def,
            type_defs,
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(
            result.is_some(),
            "should return verter's same-file definition"
        );

        match result.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                // Positive: verter's precise position is preserved
                assert_eq!(loc.uri, test_doc_uri());
                assert_eq!(loc.range.start.line, 5);
                assert_eq!(loc.range.start.character, 6);
                // Negative: must NOT be (0,0) from failed type provider mapping
                assert_ne!(
                    loc.range.start.line, 0,
                    "must not be (0,0) from failed type provider mapping"
                );
            }
            _ => panic!("Expected scalar definition"),
        }
    }

    // ── Hover merge tests ──────────────────────────────────────────

    // ── normalize_vue_path tests ────────────────────────────────────

    #[test]
    fn normalize_vue_path_strips_tsx() {
        assert_eq!(normalize_vue_path("/src/App.vue.tsx"), "/src/App.vue");
    }

    #[test]
    fn normalize_vue_path_strips_dts() {
        assert_eq!(
            normalize_vue_path("/node_modules/lib/Comp.vue.d.ts"),
            "/node_modules/lib/Comp.vue"
        );
    }

    #[test]
    fn normalize_vue_path_strips_vue_ts() {
        assert_eq!(normalize_vue_path("/src/App.vue.ts"), "/src/App.vue");
    }

    #[test]
    fn normalize_vue_path_strips_vue_jsx() {
        assert_eq!(normalize_vue_path("/src/App.vue.jsx"), "/src/App.vue");
    }

    #[test]
    fn normalize_vue_path_passthrough_plain_dts() {
        // Non-.vue .d.ts files should NOT be stripped
        assert_eq!(
            normalize_vue_path("/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts"),
            "/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts"
        );
    }

    #[test]
    fn normalize_vue_path_passthrough_plain_ts() {
        // Non-.vue .ts files should NOT be stripped
        assert_eq!(normalize_vue_path("/src/utils.ts"), "/src/utils.ts");
    }

    // ── strip_verter_ide_prefix tests ────────────────────────────────

    /// @ai-generated - Strips .verter/ide/<hash>/ prefix from provider paths
    #[test]
    fn strip_verter_ide_prefix_valid_unix() {
        let path = "/project/.verter/ide/a1b2c3d4e5f6g7h8/src/App.vue";
        let result = strip_verter_ide_prefix_owned(path);
        assert_eq!(result, Some("/project/src/App.vue".to_string()));
    }

    #[test]
    fn strip_verter_ide_prefix_valid_windows() {
        let path = "D:\\project\\.verter\\ide\\a1b2c3d4e5f6g7h8\\src\\App.vue";
        let result = strip_verter_ide_prefix_owned(path);
        assert_eq!(result, Some("D:\\project\\src\\App.vue".to_string()));
    }

    #[test]
    fn strip_verter_ide_prefix_no_marker() {
        let path = "/project/src/App.vue";
        let result = strip_verter_ide_prefix_owned(path);
        assert_eq!(result, None);
    }

    #[test]
    fn strip_verter_ide_prefix_short_hash() {
        // Hash too short (< 16 chars) — should return None
        let path = "/project/.verter/ide/abc123/src/App.vue";
        let result = strip_verter_ide_prefix_owned(path);
        assert_eq!(result, None);
    }

    #[test]
    fn strip_verter_ide_prefix_with_vue_tsx_suffix() {
        let path = "/project/.verter/ide/a1b2c3d4e5f6g7h8/src/App.vue.tsx";
        let result = strip_verter_ide_prefix_owned(path);
        assert_eq!(result, Some("/project/src/App.vue.tsx".to_string()));
    }

    // ── .vue.d.ts definition tests ──────────────────────────────────

    /// TypeProvider returning .vue.d.ts should navigate to .vue
    #[test]
    fn merge_definitions_vue_dts_maps_to_vue() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let type_defs = vec![TypeLocation {
            path: "/node_modules/my-lib/dist/Button.vue.d.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_definitions(
            None,
            type_defs,
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(result.is_some());
        match result.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.as_str().ends_with("Button.vue"),
                    "should navigate to .vue, got: {}",
                    loc.uri.as_str()
                );
                // Negative: must NOT contain .d.ts
                assert!(
                    !loc.uri.as_str().contains(".d.ts"),
                    "URI must not contain .d.ts suffix"
                );
            }
            _ => panic!("Expected scalar definition"),
        }
    }

    /// .vue.d.ts references should map to .vue
    #[test]
    fn merge_references_vue_dts_maps_to_vue() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let type_refs = vec![TypeLocation {
            path: "/node_modules/my-lib/dist/Button.vue.d.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_references(None, type_refs, &tsx_li, &mapper, &vue_li, None);
        assert!(result.is_some());
        let locs = result.unwrap();
        assert_eq!(locs.len(), 1);
        assert!(
            locs[0].uri.as_str().ends_with("Button.vue"),
            "should reference .vue, got: {}",
            locs[0].uri.as_str()
        );
        assert!(
            !locs[0].uri.as_str().contains(".d.ts"),
            "URI must not contain .d.ts suffix"
        );
    }

    /// .vue.d.ts rename locations should map to .vue
    #[test]
    fn merge_rename_vue_dts_maps_to_vue() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let type_locations = vec![RenameLocation {
            path: "/node_modules/my-lib/dist/Button.vue.d.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_rename_locations(
            None,
            type_locations,
            "NewName",
            &tsx_li,
            &mapper,
            &vue_li,
            None,
        );
        assert!(result.is_some());
        let edit = result.unwrap();
        let changes = edit.changes.unwrap();
        let uris: Vec<String> = changes.keys().map(|u| u.as_str().to_string()).collect();
        assert!(
            uris.iter().any(|u| u.ends_with("Button.vue")),
            "should rename in .vue file, got: {:?}",
            uris
        );
        assert!(
            !uris.iter().any(|u| u.contains(".d.ts")),
            "URI must not contain .d.ts suffix"
        );
    }

    // ── Hover merge tests ──────────────────────────────────────────
    #[test]
    fn strip_leading_code_block_removes_fence() {
        let text = "```typescript\nconst count: number\n```\n*(reactive)*";
        assert_eq!(strip_leading_code_block(text), "*(reactive)*");
    }

    /// @ai-generated — strip_leading_code_block returns full text when no fence
    #[test]
    fn strip_leading_code_block_no_fence() {
        let text = "*(reactive)*\nInitialized via `ref()`";
        assert_eq!(strip_leading_code_block(text), text);
    }

    /// @ai-generated — merge_hover deduplicates code fences
    #[test]
    fn merge_hover_no_duplicate_fences() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        let verter = Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```typescript\nconst count\n```\n\n*(reactive)*".to_string(),
            }),
            range: None,
        });
        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "const count: Ref<number>".to_string(),
        });

        let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &vue_li, None);
        assert!(result.is_some());

        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        // Should have exactly one code fence from TSGO, plus verter context
        assert!(text.contains("const count: Ref<number>"));
        assert!(text.contains("*(reactive)*"));
        // Count code fence openings — should be exactly 1
        assert_eq!(text.matches("```typescript").count(), 1, "text: {text}");
    }

    /// @ai-generated — merge_hover with verter-only code block and TSGO replaces it cleanly
    #[test]
    fn merge_hover_verter_only_code_block() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        let verter = Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```typescript\nconst x\n```".to_string(),
            }),
            range: None,
        });
        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "const x: string".to_string(),
        });

        let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &vue_li, None);
        assert!(result.is_some());

        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        // Only TSGO type block, no "---" separator since verter had nothing extra
        assert_eq!(text, "```typescript\nconst x: string\n```");
    }

    // ── Bug 3: TSGO already-markdown hover tests ─────────────────

    #[test]
    fn merge_hover_tsgo_already_markdown_no_double_fence() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "```typescript\n(property) msg: string\n```\nThe message.".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &vue_li, None);
        assert!(result.is_some());

        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        // Should start with the type signature in a code fence
        assert!(
            text.starts_with("```typescript\n(property) msg: string\n```"),
            "should start with original code fence: {text}"
        );
        // Documentation should appear OUTSIDE the code fence
        assert!(
            text.contains("The message."),
            "documentation should be present: {text}"
        );
        // Count code fence openings — should be exactly 1
        assert_eq!(
            text.matches("```typescript").count(),
            1,
            "should not double-fence: {text}"
        );
    }

    #[test]
    fn merge_hover_tsgo_plain_text_gets_wrapped() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "(property) msg: string".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &vue_li, None);
        assert!(result.is_some());

        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        assert_eq!(text, "```typescript\n(property) msg: string\n```");
    }

    #[test]
    fn merge_hover_tsgo_with_jsdoc_newlines_preserved() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "```typescript\n(property) select: (action: Action) => true\n```\nEmitted when selected.\n当选择时触发。".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &vue_li, None);
        assert!(result.is_some());

        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        assert!(
            text.contains("Emitted when selected."),
            "documentation should be preserved: {text}"
        );
        assert!(
            text.contains("当选择时触发。"),
            "CJK documentation should be preserved: {text}"
        );
        // Doc should be outside code fence
        assert_eq!(
            text.matches("```typescript").count(),
            1,
            "should not double-fence: {text}"
        );
    }

    #[test]
    fn merge_hover_verter_and_tsgo_combined_markdown() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        let verter = Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```typescript\nconst count\n```\n*(reactive)*".to_string(),
            }),
            range: None,
        });
        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "```typescript\nconst count: Ref<number>\n```\nA counter.".to_string(),
        });

        let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &vue_li, None);
        assert!(result.is_some());

        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        // TSGO signature should be present (not double-fenced)
        assert!(
            text.contains("const count: Ref<number>"),
            "should have TSGO signature: {text}"
        );
        assert!(
            text.contains("*(reactive)*"),
            "should have verter context: {text}"
        );
        // Only 1 typescript code fence
        assert_eq!(
            text.matches("```typescript").count(),
            1,
            "should not double-fence: {text}"
        );
    }

    #[test]
    fn wrap_type_block_plain_text_with_blank_line_separator() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        // TSGO returns plain text with type and doc separated by blank line
        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "(property) GameItemProps.game: GameVo | ProfilePlayedVo\n\n游戏数据"
                .to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &vue_li, None);
        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        // Type should be inside fence
        assert!(
            text.starts_with(
                "```typescript\n(property) GameItemProps.game: GameVo | ProfilePlayedVo\n```"
            ),
            "type should be in code fence: {text}"
        );
        // Doc should be outside fence
        assert!(
            text.contains("游戏数据"),
            "documentation should be preserved: {text}"
        );
        // Doc must not be inside the code fence
        let fence_end = text.find("\n```").unwrap();
        let doc_pos = text.find("游戏数据").unwrap();
        assert!(
            doc_pos > fence_end,
            "documentation should be outside the code fence: {text}"
        );
    }

    #[test]
    fn wrap_type_block_plain_text_with_single_newline_separator() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        // TSGO returns plain text with type and doc separated by single newline
        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "(property) game: GameVo\nThe game data.".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &vue_li, None);
        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        // Type should be inside fence
        assert!(
            text.starts_with("```typescript\n(property) game: GameVo\n```"),
            "type should be in code fence: {text}"
        );
        // Doc should be outside fence
        let fence_end = text.find("\n```").unwrap();
        let doc_pos = text.find("The game data.").unwrap();
        assert!(
            doc_pos > fence_end,
            "documentation should be outside the code fence: {text}"
        );
    }

    #[test]
    fn wrap_type_block_plain_text_no_newline() {
        // When there's no newline separator, everything goes in the fence
        // (can't reliably split type from doc without a separator)
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "(property) msg: string".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &vue_li, None);
        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        assert_eq!(text, "```typescript\n(property) msg: string\n```");
    }

    #[test]
    fn replace_kind_prefix_replaces_const_with_ref() {
        let input = "```typescript\n(const) const count: Ref<number>\n```";
        let result = replace_kind_prefix(input, "ref");
        assert_eq!(result, "```typescript\n(ref) const count: Ref<number>\n```");
        assert!(!result.contains("(const)"), "old prefix must be replaced");
    }

    #[test]
    fn replace_kind_prefix_no_prefix_passthrough() {
        let input = "```typescript\nconst count: number\n```";
        let result = replace_kind_prefix(input, "ref");
        // No `(...)` prefix to replace, so content passes through unchanged
        assert_eq!(result, input);
    }

    #[test]
    fn merge_hover_with_vue_kind_label() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter =
            make_verter_hover("```typescript\nconst count\n```\n\n*(ref — needs `.value`)*");
        let type_hover = HoverInfo {
            contents: "```typescript\n(const) const count: Ref<number>\n```".to_string(),
            range_start: None,
            range_end: None,
        };

        let result = merge_hover(
            Some(verter),
            Some(type_hover),
            &mapper,
            &tsx_li,
            &vue_li,
            Some("ref"),
        );
        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(
            text.contains("(ref) const count"),
            "kind prefix should be replaced with vue label: {text}"
        );
        assert!(
            !text.contains("(const)"),
            "generic kind prefix must be replaced: {text}"
        );
    }

    /// External resolver provides correct position mapping for cross-file definitions.
    /// When TSGO returns a .vue.tsx target pointing to a *different* file,
    /// the merge function should use the external resolver's mapper instead of the
    /// current file's mapper.
    #[test]
    fn merge_definitions_uses_external_resolver_for_cross_file() {
        let (_mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // Build target file's mapper: TSX line 0 col 0 → Vue line 1 col 0
        let target_vue = "<script setup>\ndefineComponent({})\n</script>";
        let target_tsx = "defineComponent({});\n";
        let target_vue_li = LineIndex::new_utf16(target_vue);
        let target_tsx_li = LineIndex::new_utf16(target_tsx);

        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        let sid = builder.set_source_and_content("Target.vue", target_vue);
        builder.add_token(0, 0, 1, 0, Some(sid), None); // TSX 0:0 → Vue 1:0
        builder.add_token(0, 16, 1, 16, Some(sid), None); // TSX 0:16 → Vue 1:16
        let json = builder.into_sourcemap().to_json_string();
        let target_mapper = PositionMapper::from_json(&json).unwrap();

        let type_defs = vec![TypeLocation {
            path: "/src/components/Target.vue.tsx".to_string(),
            start: 0,
            end: 16, // "defineComponent("
        }];

        // Without resolver: current file's mapper can't resolve target's TSX offsets → default range
        let result_no_resolver = merge_definitions(
            None,
            type_defs.clone(),
            &tsx_li,
            &_mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        assert!(result_no_resolver.is_some());
        match result_no_resolver.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.as_str().ends_with("Target.vue"),
                    "should navigate to .vue: {}",
                    loc.uri.as_str()
                );
            }
            _ => panic!("expected scalar"),
        }

        // With resolver: external mapper resolves correctly
        let resolver = |ide_path: &str| -> Option<ExternalIdeContext> {
            if ide_path == "/src/components/Target.vue.tsx" {
                Some(ExternalIdeContext {
                    tsx_line_index: target_tsx_li.clone(),
                    mapper: target_mapper.clone(),
                    vue_line_index: target_vue_li.clone(),
                })
            } else {
                None
            }
        };

        let result_with_resolver = merge_definitions(
            None,
            type_defs,
            &tsx_li,
            &_mapper,
            &vue_li,
            Some(&resolver),
            &test_doc_uri(),
        );
        assert!(result_with_resolver.is_some());
        match result_with_resolver.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.as_str().ends_with("Target.vue"),
                    "should navigate to .vue: {}",
                    loc.uri.as_str()
                );
                // Position should map to Vue line 1 (inside <script setup>)
                assert_eq!(
                    loc.range.start.line, 1,
                    "with resolver, definition should map to Vue line 1, got: {:?}",
                    loc.range
                );
                assert_ne!(
                    loc.range,
                    Range::default(),
                    "with resolver, range should not be default (0,0)"
                );
            }
            _ => panic!("expected scalar"),
        }
    }

    // ── Definition deduplication and filtering tests ──────────────

    #[test]
    fn merge_definitions_deduplicates_vue_locations() {
        // Bug: multiple .vue.tsx targets for the same .vue file should deduplicate
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let type_defs = vec![
            TypeLocation {
                path: "/src/components/Dropdown.vue.tsx".to_string(),
                start: 0,
                end: 10,
            },
            TypeLocation {
                path: "/src/components/Dropdown.vue.tsx".to_string(),
                start: 20,
                end: 30,
            },
        ];

        let result = merge_definitions(
            None,
            type_defs,
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        match result {
            Some(GotoDefinitionResponse::Scalar(_)) => {
                // Deduplicated to a single location — correct
            }
            Some(GotoDefinitionResponse::Array(locs)) => {
                panic!(
                    "should deduplicate to Scalar, got Array with {} locations",
                    locs.len()
                );
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn merge_definitions_filters_vue_when_non_vue_exists() {
        // Bug: when CTRL+CLICKing on an import from a library, both .d.mts (real def)
        // and .vue.tsx (consumer) are returned. Should filter out .vue targets.
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let type_defs = vec![
            TypeLocation {
                path: "/node_modules/@vueuse/core/index.d.mts".to_string(),
                start: 100,
                end: 120,
            },
            TypeLocation {
                path: "/src/components/Dropdown.vue.tsx".to_string(),
                start: 0,
                end: 10,
            },
            TypeLocation {
                path: "/src/components/Drawer.vue.tsx".to_string(),
                start: 0,
                end: 10,
            },
        ];

        let result = merge_definitions(
            None,
            type_defs,
            &tsx_li,
            &mapper,
            &vue_li,
            None,
            &test_doc_uri(),
        );
        match result {
            Some(GotoDefinitionResponse::Scalar(loc)) => {
                // Should keep only the .d.mts definition
                assert!(
                    !loc.uri.as_str().contains(".vue"),
                    ".vue targets should be filtered out, got: {:?}",
                    loc.uri
                );
            }
            Some(GotoDefinitionResponse::Array(locs)) => {
                for loc in &locs {
                    assert!(
                        !loc.uri.as_str().contains(".vue"),
                        ".vue targets should be filtered out when .d.mts exists, got: {:?}",
                        loc.uri
                    );
                }
            }
            None => panic!("expected some definitions"),
            _ => panic!("unexpected response type"),
        }
    }

    // ── JSX→Vue reverse transformation tests ──────────────────────

    #[test]
    fn test_jsx_event_to_vue_click() {
        assert_eq!(jsx_prop_to_vue_attr("onClick"), Some("@click".to_string()));
    }

    #[test]
    fn test_jsx_event_to_vue_custom() {
        assert_eq!(
            jsx_prop_to_vue_attr("onCustomEvent"),
            Some("@custom-event".to_string())
        );
    }

    #[test]
    fn test_jsx_event_to_vue_update_model() {
        assert_eq!(
            jsx_prop_to_vue_attr("onUpdate:modelValue"),
            Some("@update:model-value".to_string())
        );
    }

    #[test]
    fn test_jsx_prop_camel_to_kebab() {
        assert_eq!(
            jsx_prop_to_vue_attr("modelValue"),
            Some("model-value".to_string())
        );
    }

    #[test]
    fn test_jsx_data_attr_unchanged() {
        assert_eq!(
            jsx_prop_to_vue_attr("data-id"),
            None // Already kebab, no transformation needed
        );
    }

    #[test]
    fn test_jsx_simple_attr_unchanged() {
        // Simple lowercase attrs like "class", "id", "key" — no transformation
        assert_eq!(jsx_prop_to_vue_attr("class"), None);
        assert_eq!(jsx_prop_to_vue_attr("id"), None);
        assert_eq!(jsx_prop_to_vue_attr("key"), None);
        assert_eq!(jsx_prop_to_vue_attr("ref"), None);
    }

    #[test]
    fn test_jsx_tab_index_lowercase() {
        assert_eq!(
            jsx_prop_to_vue_attr("tabIndex"),
            Some("tab-index".to_string())
        );
    }

    #[test]
    fn test_merge_completions_transforms_jsx_events() {
        // Create a TSGO completion result with an onClick item
        let type_result = CompletionResult {
            items: vec![
                Completion {
                    label: "onClick".to_string(),
                    kind: Some(CompletionKind::Property),
                    detail: None,
                    documentation: None,
                    sort_text: None,
                    insert_text: None,
                    edit_range_start: None,
                    edit_range_end: None,
                    data: None,
                },
                Completion {
                    label: "modelValue".to_string(),
                    kind: Some(CompletionKind::Property),
                    detail: None,
                    documentation: None,
                    sort_text: None,
                    insert_text: None,
                    edit_range_start: None,
                    edit_range_end: None,
                    data: None,
                },
            ],
            is_incomplete: false,
        };

        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let (items, _) = merge_completions(
            vec![],
            type_result,
            &mapper,
            &tsx_li,
            &vue_li,
            None,
            true, // template_attr_context
        );

        // onClick should be transformed to @click
        assert!(
            items.iter().any(|i| i.label == "@click"),
            "onClick should be transformed to @click, got: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
        assert!(
            !items.iter().any(|i| i.label == "onClick"),
            "onClick should NOT remain"
        );

        // modelValue should be transformed to model-value
        assert!(
            items.iter().any(|i| i.label == "model-value"),
            "modelValue should be transformed to model-value"
        );
    }

    #[test]
    fn merge_expression_context_does_not_transform_jsx() {
        // When template_attr_context=false (expression context like {{ props. }}),
        // JSX prop names should NOT be transformed to Vue syntax.
        let type_result = CompletionResult {
            items: vec![
                Completion {
                    label: "onClick".to_string(),
                    kind: Some(CompletionKind::Property),
                    detail: None,
                    documentation: None,
                    sort_text: None,
                    insert_text: None,
                    edit_range_start: None,
                    edit_range_end: None,
                    data: None,
                },
                Completion {
                    label: "modelValue".to_string(),
                    kind: Some(CompletionKind::Property),
                    detail: None,
                    documentation: None,
                    sort_text: None,
                    insert_text: None,
                    edit_range_start: None,
                    edit_range_end: None,
                    data: None,
                },
                Completion {
                    label: "title".to_string(),
                    kind: Some(CompletionKind::Property),
                    detail: None,
                    documentation: None,
                    sort_text: None,
                    insert_text: None,
                    edit_range_start: None,
                    edit_range_end: None,
                    data: None,
                },
            ],
            is_incomplete: false,
        };

        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let (items, _) = merge_completions(
            vec![],
            type_result,
            &mapper,
            &tsx_li,
            &vue_li,
            None,
            false, // NOT in template attr context — expression context
        );

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

        // POSITIVE: labels should remain as-is
        assert!(
            labels.contains(&"onClick"),
            "onClick should remain as-is, got: {labels:?}"
        );
        assert!(
            labels.contains(&"modelValue"),
            "modelValue should remain as-is, got: {labels:?}"
        );
        assert!(
            labels.contains(&"title"),
            "title should remain as-is, got: {labels:?}"
        );

        // NEGATIVE: no Vue-transformed labels
        assert!(
            !labels.iter().any(|l| l.starts_with('@')),
            "no @-prefixed items in expression context, got: {labels:?}"
        );
        assert!(
            !labels.contains(&"model-value"),
            "no kebab-case transformation in expression context, got: {labels:?}"
        );
    }
}
