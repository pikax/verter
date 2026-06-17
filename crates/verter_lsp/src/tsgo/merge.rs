//! Merge logic for combining verter analysis results with TypeProvider results.
//!
//! Each merge function takes verter-only results and TypeProvider results,
//! producing enhanced output. All functions handle the case where either
//! source may be absent (graceful fallback).

use std::sync::Arc;

use tower_lsp_server::ls_types::*;
use verter_span::{LspPosition, TsPosition};

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::features::hover::HoverSourceToken;
#[cfg(test)]
use crate::tsgo::protocol::Completion;
use crate::tsgo::protocol::{
    self, CompletionKind, CompletionResult, HoverInfo, InlayHint, InlayHintKind, RenameLocation,
    TypeCodeAction, TypeDiagnostic, TypeDiagnosticSeverity, TypeDocumentHighlight,
    TypeDocumentHighlightKind, TypeLocation,
};
use crate::uri::path_to_file_uri;

/// External IDE context for resolving positions in a foreign carrier IDE TSX
/// file (`Comp.vue.tsx`, `Comp.svelte.tsx`, …).
///
/// For cross-file navigation (e.g., CTRL+CLICK navigates to another carrier
/// file), the merge functions need the target file's TSX line index, position
/// mapper, and carrier-source line index. This struct carries those, and the
/// resolver closure produces it.
pub struct ExternalIdeContext {
    pub tsx_line_index: LineIndex,
    pub mapper: ProviderPositionMapper,
    pub carrier_line_index: LineIndex,
}

/// Resolver for looking up IDE context by IDE path (e.g., `/path/to/Comp.vue.tsx`).
///
/// Returns `None` if the file isn't tracked or hasn't been compiled yet.
pub type ExternalIdeResolver<'a> = &'a dyn Fn(&str) -> Option<ExternalIdeContext>;

/// Resolver for following a type-provider location through barrel re-exports.
///
/// The input is the raw provider path plus its byte-offset range in that file.
/// Returns a fully resolved LSP location when the file/range matches a known
/// re-export signature; otherwise returns `None` and merge logic keeps the
/// original provider location unchanged.
pub type BarrelResolver<'a> = &'a dyn Fn(&str, u32, u32) -> Option<Location>;

/// Reader for a definition/type-definition target's OWN source, routed through the host's
/// workspace (VFS) layer instead of direct disk I/O.
///
/// [`resolve_external_target_range`] converts the provider's byte offsets to line:col by
/// reading the same source those offsets index; the read goes through this closure so the
/// merge layer never touches `std::fs` directly — the workspace/VFS is the single source-read
/// authority (host cache → snapshot → disk, an open editor's overlay winning over stale disk
/// content). Returns `None` when the file cannot be read, and the caller then fails closed.
pub type ExternalSourceReader<'a> = &'a dyn Fn(&str) -> Option<Arc<str>>;

/// Map an LSP `Position` (in the carrier source file) to a byte offset in the
/// generated TSX.
///
/// Steps: LSP Position → byte offset via LineIndex → line/col → PositionMapper → TSX line/col → TSX byte offset via TSX LineIndex.
///
/// Returns `None` if any mapping step fails.
pub fn carrier_position_to_tsx_offset(
    position: &Position,
    _carrier_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    let tsx_pos = mapper
        .carrier_to_tsx(LspPosition::new(position.line, position.character))?
        .pos;
    tsx_line_index.position_to_offset(&Position {
        line: tsx_pos.line,
        character: tsx_pos.character,
    })
}

/// Map a carrier-source position to a TSX byte offset, with round-trip validation.
///
/// After mapping carrier→TSX, verifies the TSX offset maps back to the same
/// carrier-source line. Returns `None` if the round-trip fails (indicating the
/// TSX offset is in a synthetic region like generated JSX for HTML elements,
/// where TSGO queries would crash).
pub fn carrier_position_to_tsx_offset_validated(
    position: &Position,
    carrier_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    let tsx_offset =
        carrier_position_to_tsx_offset(position, carrier_line_index, mapper, tsx_line_index)?;
    if let Some(exact_offset) =
        find_exact_roundtrip_offset(position, tsx_offset, mapper, tsx_line_index)
    {
        return Some(exact_offset);
    }

    // Round-trip: TSX offset → TSX position → Vue position
    let tsx_pos = tsx_line_index.offset_to_position(tsx_offset)?;
    let carrier_roundtrip = mapper
        .tsx_to_carrier(TsPosition::new(tsx_pos.line, tsx_pos.character))?
        .pos;

    // The round-trip Vue position should be on the same line as the original.
    // If not, the TSX offset is in a synthetic region with no valid source correlation.
    if carrier_roundtrip.line == position.line {
        Some(tsx_offset)
    } else {
        None
    }
}

fn find_exact_roundtrip_offset(
    position: &Position,
    initial_offset: u32,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    const SEARCH_WINDOW: u32 = 256;

    let initial_pos = tsx_line_index.offset_to_position(initial_offset)?;

    let roundtrips_exact = |offset: u32| -> Option<bool> {
        let tsx_pos = tsx_line_index.offset_to_position(offset)?;
        if tsx_pos.line != initial_pos.line {
            return Some(false);
        }
        let carrier_pos = mapper
            .tsx_to_carrier(TsPosition::new(tsx_pos.line, tsx_pos.character))?
            .pos;
        Some(carrier_pos.line == position.line && carrier_pos.character == position.character)
    };

    if roundtrips_exact(initial_offset)? {
        return Some(initial_offset);
    }

    for delta in 1..=SEARCH_WINDOW {
        if initial_offset >= delta {
            let candidate = initial_offset - delta;
            if roundtrips_exact(candidate)? {
                return Some(candidate);
            }
        }

        let candidate = initial_offset + delta;
        if roundtrips_exact(candidate)? {
            return Some(candidate);
        }
    }

    None
}

/// Completion-ONLY member-boundary mapping for an incomplete member access (`obj.` / `obj?.`).
///
/// This is NOT a relaxation of the strict mappers and NOT a "fall back to the raw offset on any
/// `.` trigger". It is a precisely-guarded path used ONLY by the completion handler, AFTER
/// [`carrier_position_to_tsx_offset_validated`] has returned `None`. The strict mappers
/// ([`carrier_position_to_tsx_offset`], [`carrier_position_to_tsx_offset_validated`],
/// [`ProviderPositionMapper::carrier_to_tsx`], [`ProviderPositionMapper::tsx_to_carrier`]) keep
/// their strict in-run semantics; this helper never feeds any other feature path.
///
/// The cursor after `obj.` is a zero-width member-access boundary that sits OUTSIDE any mapped
/// run, so the strict path legitimately maps nothing. This helper anchors on a mapped run whose
/// SOURCE extent ends exactly at one of TWO same-line endpoints — the cursor itself, or the
/// position just before the operator — and accepts only when the generated TSX carries the
/// matching `.`/`?.` operator at that run's generated endpoint. Every guard is mandatory;
/// failing both anchor arms returns `None`.
///
/// Guard chain (the completion-boundary rule):
/// 1. **Validated-first / completion-only** — enforced by the caller: this runs only from
///    `handle_completion`, only when the validated strict mapper returned `None`.
/// 2. **Source PROVES incomplete member access** — the Vue source immediately before the cursor
///    must end EXACTLY with `?.` (checked first) or `.` (not merely a `.` trigger character,
///    and not a `..`/`...` suffix — a `.` preceded by another `.` rejects).
/// 3. **At-cursor anchor** — [`ProviderPositionMapper::mapped_run_ending_at_src`] at the cursor column
///    (converted to UTF-16 code units — mapped-run columns are always UTF-16, while the LSP
///    `position.character` is in the client-negotiated encoding): the run includes the trailing
///    operator as its last source content (position-preserving emission), so its source extent
///    ends AT the cursor. Accepted only when the generated text ending at the run's generated
///    endpoint ENDS WITH EXACTLY the same operator (a source `.` does not accept a generated
///    `?.`); the result is that endpoint's byte offset (immediately after the generated
///    operator).
/// 4. **Before-operator anchor** — otherwise, [`ProviderPositionMapper::mapped_run_ending_at_src`] at
///    `cursor - operator length`: the receiver run EXCLUDES the operator (relocated/planned
///    expression shapes emit the `.`/`?.` as generated content immediately after the mapped
///    endpoint). Accepted only when the generated text immediately AFTER the run's generated
///    endpoint STARTS WITH the same operator; the result is the endpoint's byte offset PLUS the
///    operator length.
/// 5. **No other lookup** — no cross-line anchor, no generated-containment lookup, no
///    nearest-preceding-run snap, no raw fallback. Both arms failing returns `None`.
///
/// Both arms demand an exact same-line source-extent-endpoint match plus source/generated
/// operator agreement. On success the returned offset is the generated byte offset immediately
/// AFTER the matched generated operator (before any trailing synthetic `}`), so a TSGO query
/// there resolves `obj`'s members.
pub(crate) fn carrier_completion_member_boundary_offset(
    position: &Position,
    carrier_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
    tsx_code: &str,
    carrier_source: &str,
) -> Option<u32> {
    // Guard 2: source PROVES incomplete member access. Inspect the Vue source bytes
    // immediately before the cursor; the suffix must be EXACTLY `?.` or `.` — a `..`/`...`
    // suffix is not a member-access boundary and rejects.
    let cursor_byte = carrier_line_index.position_to_offset(position)? as usize;
    let before = carrier_source.get(..cursor_byte)?;
    let op_str = if before.ends_with("?.") {
        "?."
    } else if let Some(stripped) = before.strip_suffix('.') {
        if stripped.ends_with('.') {
            return None;
        }
        "."
    } else {
        return None;
    };
    // The operator is ASCII, so its byte length equals its UTF-16 column width.
    let op_len = op_str.len() as u32;

    // Generated-operator agreement for the matched anchor: the generated text must carry
    // EXACTLY the source operator. For `.` that excludes a generated `?.` (a bare
    // `ends_with(".")` would also accept it); `?.` already excludes a bare `.`.
    let generated_operator_matches = |prefix: &str| -> bool {
        match op_str {
            "." => prefix.ends_with('.') && !prefix.ends_with("?."),
            _ => prefix.ends_with("?."),
        }
    };

    // Mapped-run columns are ALWAYS UTF-16 code units (the source-map column space), while
    // `position.character` is in the client-negotiated encoding (UTF-8-first). Convert the
    // cursor's source column to UTF-16 units via the byte offset guard 2 already computed:
    // the UTF-16 column is the UTF-16 length of the line's text up to the cursor byte.
    let line_start = carrier_line_index.line_start(position.line as usize)? as usize;
    let cursor_col_utf16: u32 = carrier_source
        .get(line_start..cursor_byte)?
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum();

    // The anchor returned by `mapped_run_ending_at_src` is ALSO in UTF-16 columns (the
    // generated side of the same source-map space), while `tsx_line_index` interprets
    // `Position.character` in the negotiated encoding — so converting the anchor through
    // `position_to_offset` would mis-land when non-ASCII text precedes it on the generated
    // line. Convert the UTF-16 column to a byte offset directly against the generated line
    // text (encoding-independent; a column inside a surrogate pair or past EOL rejects).
    let anchor_byte_offset = |anchor: &TsPosition| -> Option<u32> {
        let line_start = tsx_line_index.line_start(anchor.line as usize)?;
        let line_end = tsx_line_index.line_end(anchor.line as usize)?;
        let line_text = tsx_code.get(line_start as usize..line_end as usize)?;
        let mut utf16_remaining = anchor.character;
        let mut byte_col = 0u32;
        for c in line_text.chars() {
            if utf16_remaining == 0 {
                break;
            }
            let units = c.len_utf16() as u32;
            if units > utf16_remaining {
                return None;
            }
            utf16_remaining -= units;
            byte_col += c.len_utf8() as u32;
        }
        if utf16_remaining > 0 {
            return None;
        }
        Some(line_start + byte_col)
    };

    // Guard 3: at-cursor anchor. The run whose source extent ends exactly AT the cursor column
    // includes the member operator as its last source content; its generated endpoint is the
    // boundary just past the generated operator. Accept only on generated-suffix agreement.
    if let Some(anchor) = mapper.mapped_run_ending_at_src(position.line, cursor_col_utf16) {
        if let Some(anchor_offset) = anchor_byte_offset(&anchor) {
            if tsx_code
                .get(..anchor_offset as usize)
                .is_some_and(generated_operator_matches)
            {
                return Some(anchor_offset);
            }
        }
    }

    // Guard 4: before-operator anchor. The receiver run's source extent ends exactly at the
    // column just BEFORE the operator (the operator is not part of the mapped run); the
    // generated operator must sit immediately AFTER the run's generated endpoint. Accept only
    // on generated-prefix agreement, returning the endpoint plus the operator length.
    let receiver_col = cursor_col_utf16.checked_sub(op_len)?;
    let anchor = mapper.mapped_run_ending_at_src(position.line, receiver_col)?;
    let anchor_offset = anchor_byte_offset(&anchor)?;
    let generated_after = tsx_code.get(anchor_offset as usize..)?;
    if !generated_after.starts_with(op_str) {
        return None;
    }
    Some(anchor_offset + op_len)
}

/// Map a TSX byte offset range back to an LSP `Range` in the Vue source.
///
/// Routes through the mapper's strict [`PositionMapper::tsx_range_to_carrier`], which enforces
/// the half-open endpoint-compatibility rule: the range maps ONLY when both endpoints resolve
/// inside compatible mapped runs (the same run, or genuinely-contiguous runs with no
/// synthetic/unmapped content between them). A range whose endpoints fall in two runs
/// separated by synthetic content — even though each endpoint individually maps — is dropped.
///
/// Returns `None` if any mapping step fails or the endpoints are incompatible.
pub fn tsx_range_to_carrier_range(
    tsx_start: u32,
    tsx_end: u32,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Option<Range> {
    let start_pos = tsx_line_index.offset_to_position(tsx_start)?;
    let end_pos = tsx_line_index.offset_to_position(tsx_end)?;

    let (carrier_start, carrier_end) = mapper.tsx_range_to_carrier(
        TsPosition::new(start_pos.line, start_pos.character),
        TsPosition::new(end_pos.line, end_pos.character),
    )?;

    // Validate the mapped positions produce valid byte offsets
    let start_lsp = Position {
        line: carrier_start.line,
        character: carrier_start.character,
    };
    let end_lsp = Position {
        line: carrier_end.line,
        character: carrier_end.character,
    };
    carrier_line_index.position_to_offset(&start_lsp)?;
    carrier_line_index.position_to_offset(&end_lsp)?;

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
    _mapper: &ProviderPositionMapper,
    _tsx_line_index: &LineIndex,
    _carrier_line_index: &LineIndex,
    vue_kind_label: Option<&str>,
    source_token: Option<&HoverSourceToken>,
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
            // Rewrite the primary label to Vue source syntax ONLY when the verter
            // hover carries TYPED event-directive provenance — never by reparsing
            // rendered hover text and never by `on*` name-suffix sniffing.
            if let Some(HoverSourceToken::EventDirective { vue_attr }) = source_token {
                type_block = replace_primary_label_with_vue_attr(&type_block, vue_attr);
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

fn replace_primary_label_with_vue_attr(content: &str, vue_attr: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
    if lines.len() < 2 {
        return content.to_string();
    }

    let line = &lines[1];
    let prefix_end = if line.starts_with('(') {
        line.find(") ").map(|idx| idx + 2).unwrap_or(0)
    } else {
        0
    };
    let rest = &line[prefix_end..];
    let name_end = rest
        .find("?:")
        .or_else(|| rest.find(": "))
        .unwrap_or(rest.len());

    if name_end == rest.len() {
        return content.to_string();
    }

    lines[1] = format!("{}{}{}", &line[..prefix_end], vue_attr, &rest[name_end..]);
    lines.join("\n")
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
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
    carrier_line_index: &LineIndex,
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

        // Skip if already seen (from verter or a previous TSGO item),
        // but enrich the existing item's kind if the type provider has a richer one
        if !seen_labels.insert(label.clone()) {
            if let Some(tp_kind) = item.kind {
                if !matches!(tp_kind, CompletionKind::Text) {
                    if let Some(existing) = result.iter_mut().find(|i| i.label == label) {
                        existing.kind = Some(convert_completion_kind(tp_kind));
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Vec<Diagnostic> {
    let mut result = verter_diags;
    let mut dropped = 0u32;

    for diag in &type_diags {
        let range = tsx_range_to_carrier_range(
            diag.start,
            diag.end,
            tsx_line_index,
            mapper,
            carrier_line_index,
        );

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
fn resolve_carrier_tsx_range(
    path: &str,
    start: u32,
    end: u32,
    current_tsx_line_index: &LineIndex,
    current_mapper: &ProviderPositionMapper,
    current_carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
) -> Range {
    // Try external resolver first — it provides the correct mapper for the target file.
    // Without this, cross-file navigation uses the *current* file's mapper, producing
    // wrong positions (e.g., (0,0) or positions from the wrong file).
    if let Some(resolver) = external_resolver {
        if let Some(ctx) = resolver(path) {
            if let Some(range) = tsx_range_to_carrier_range(
                start,
                end,
                &ctx.tsx_line_index,
                &ctx.mapper,
                &ctx.carrier_line_index,
            ) {
                return range;
            }
        }
    }

    // Fallback: use current file context (works when target is same file being queried)
    tsx_range_to_carrier_range(
        start,
        end,
        current_tsx_line_index,
        current_mapper,
        current_carrier_line_index,
    )
    .unwrap_or_default()
}

/// Resolve a definition/type-definition target's byte-offset range to an LSP `Range` by
/// reading the target's own source through the host workspace (VFS) and converting through
/// [`LineIndex`].
///
/// The definition/type-definition providers produce `start`/`end` as REAL byte offsets into
/// the target file. `read_source` hands back that same source — routed through
/// `verter_workspace::WorkspaceRead::read_file` (host cache → snapshot → disk), so a cold
/// target is read and cached once and an open editor buffer's overlay wins over stale disk
/// content — and the offsets convert to line:col in the client-negotiated encoding. It is only
/// called when the emitted URI is the very file those offsets index (path normalization was a
/// no-op), so the offsets are valid.
///
/// The source read is the workspace layer's job, never `std::fs`: the VFS is the single
/// source-read authority for the LSP, which is exactly what `no_std_fs_in_semantic_session_paths`
/// enforces over this crate.
///
/// Returns `None` (FAIL-CLOSED) when the source cannot be read or an offset falls outside it.
/// Callers MUST then drop the location — never substitute `Range::default()`, which silently
/// sends the editor to line 0 of the wrong place (the original bug this replaces).
///
/// This resolves an external definition/type-definition range from the target's own on-disk
/// source: the provider already read this file to compute the offsets, and this re-reads it
/// (through the VFS) to convert those byte offsets back to a line:col `Range`. The resolver
/// covers definition/type-definition only, where the offsets are guaranteed to index the
/// target's own source.
pub(crate) fn resolve_external_target_range(
    path: &str,
    start: u32,
    end: u32,
    encoding: PositionEncodingKind,
    read_source: ExternalSourceReader<'_>,
) -> Option<Range> {
    let source = read_source(path)?;
    let line_index = LineIndex::new(&source, encoding);
    Some(Range {
        start: line_index.offset_to_position(start)?,
        end: line_index.offset_to_position(end)?,
    })
}

/// Resolve a definition/type-definition carrier IDE (`{carrier}.tsx`/`.jsx`) target's byte
/// offsets to a carrier-source [`Range`], FAIL-CLOSED.
///
/// The provider's offsets index a generated IDE TSX file; mapping them back to the carrier
/// source requires THAT file's own CodeTransform sourcemap, so the resolver is split by
/// whether the target is the file currently being queried:
///
/// - **Current provider file** (`path == current_tsx_path`): the in-context `mapper` / line
///   indexes passed by the handler already describe this exact TSX, so map through them.
/// - **Foreign carrier IDE file** (another component's generated file): only the external
///   resolver can supply the correct mapper. The current file's mapper describes a *different*
///   file, so reusing it would land on the wrong token — and the old `.unwrap_or_default()`
///   collapsed a failed reuse into a line-0 range pointing into the wrong file. There is
///   deliberately NO current-mapper fallback for foreign targets.
///
/// The provider mapper is the projection-agnostic [`ProviderPositionMapper`]: its `SourceMap`
/// variant preserves the strict source-map run semantics 1:1 for `{carrier}.tsx`, while its
/// `SelfFile` variant (a `.svelte.ts` rune module) needs no separate range algorithm —
/// [`tsx_range_to_carrier_range`] delegates through the enum's `tsx_range_to_carrier` and any
/// synthetic / prelude / unmapped region returns `None` (fail-closed preserved).
///
/// Returns `None` whenever the correct sourcemap is unavailable (no/unknown external resolver)
/// or the offsets do not map. The caller MUST drop the location — never substitute
/// `Range::default()`, which silently sends the editor to line 0.
///
/// Scope: definition and type-definition only (both route through
/// [`merge_definitions_with_barrel_resolver`]). References / rename / code actions handle their
/// own packed positions separately and do not use this resolver.
#[expect(
    clippy::too_many_arguments,
    reason = "current-file context (path + indexes + mapper) plus the foreign-file resolver"
)]
fn resolve_definition_carrier_tsx_range(
    path: &str,
    start: u32,
    end: u32,
    current_tsx_path: &str,
    current_tsx_line_index: &LineIndex,
    current_mapper: &ProviderPositionMapper,
    current_carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
) -> Option<Range> {
    if path == current_tsx_path {
        return tsx_range_to_carrier_range(
            start,
            end,
            current_tsx_line_index,
            current_mapper,
            current_carrier_line_index,
        );
    }

    // Foreign generated TSX: the in-context mapper describes a different file. Only its own
    // sourcemap (via the external resolver) can map the offsets — fail closed otherwise.
    let resolver = external_resolver?;
    let ctx = resolver(path)?;
    tsx_range_to_carrier_range(
        start,
        end,
        &ctx.tsx_line_index,
        &ctx.mapper,
        &ctx.carrier_line_index,
    )
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
#[expect(
    clippy::too_many_arguments,
    reason = "definition merging needs mapper, indexes, URI, and resolver inputs"
)]
pub fn merge_definitions(
    verter_def: Option<GotoDefinitionResponse>,
    type_defs: Vec<TypeLocation>,
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    document_uri: &Uri,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
) -> Option<GotoDefinitionResponse> {
    merge_definitions_with_barrel_resolver(
        verter_def,
        type_defs,
        current_tsx_path,
        tsx_line_index,
        mapper,
        carrier_line_index,
        external_resolver,
        document_uri,
        carrier_source_exists,
        None,
        negotiated_encoding,
        source_reader,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "barrel-aware definition merging adds one resolver to the shared merge context"
)]
pub fn merge_definitions_with_barrel_resolver(
    verter_def: Option<GotoDefinitionResponse>,
    type_defs: Vec<TypeLocation>,
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    document_uri: &Uri,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    barrel_resolver: Option<BarrelResolver<'_>>,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
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
            && loc.uri.as_str() != crate::features::definition::SAME_FILE_URI_STR);
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
                // A carrier IDE virtual file (`{carrier}.tsx`/`.jsx`): the provider's byte
                // offsets index the generated TSX; map them back to the carrier source through
                // that file's own CodeTransform sourcemap — the current file's in-context mapper
                // for the file being queried, the external resolver for a foreign component.
                // Fail closed (drop the location) when no sourcemap bridges the offsets; never
                // collapse to a line-0 range pointing into the wrong file. Generalized to the
                // registry carrier-extension set so `.svelte` carriers get the same fix.
                if is_carrier_ide_path(&loc.path) {
                    let uri =
                        path_to_uri(normalize_carrier_path(&loc.path, carrier_source_exists))?;
                    let range = resolve_definition_carrier_tsx_range(
                        &loc.path,
                        loc.start,
                        loc.end,
                        current_tsx_path,
                        tsx_line_index,
                        mapper,
                        carrier_line_index,
                        external_resolver,
                    )?;
                    return Some(Location { uri, range });
                }

                // Every other target emits the normalized path's URI. When normalization
                // is a no-op the emitted URI IS the file the provider's byte offsets index
                // (`.d.ts`/`.ts`/`.js`/…, or a real `{carrier}.ts` with no backing carrier
                // source), so read that source and convert the offsets to a real `Range`.
                // Barrel re-exports (terminal-decl follow) take priority for those real files;
                // fail closed when the source can't be read — never collapse to line 0.
                let normalized = normalize_carrier_path(&loc.path, carrier_source_exists);
                if normalized == loc.path {
                    if let Some(resolver) = barrel_resolver {
                        if let Some(location) = resolver(&loc.path, loc.start, loc.end) {
                            return Some(location);
                        }
                    }
                    let uri = path_to_uri(normalized)?;
                    let range = resolve_external_target_range(
                        &loc.path,
                        loc.start,
                        loc.end,
                        negotiated_encoding.clone(),
                        source_reader,
                    )?;
                    return Some(Location { uri, range });
                }

                // Normalization rewrote the path (`{carrier}.d.ts`/`{carrier}.ts` → carrier
                // source, or another file's `{carrier}.tsx` → carrier source): the offsets index
                // the generated declaration file, but the URI we emit is the carrier source and
                // no in-context sourcemap bridges them. Fail closed rather than send the editor
                // to line 0 of the wrong file.
                None
            })
            .collect();

        if locations.is_empty() {
            return verter_def;
        }

        // Deduplicate by (uri, range): distinct definitions in the same file (e.g. two
        // overloads in one `.d.ts`) must survive, while spans that resolve to the exact
        // same location collapse.
        let mut seen = std::collections::HashSet::new();
        locations.retain(|loc| {
            seen.insert((
                loc.uri.clone(),
                loc.range.start.line,
                loc.range.start.character,
                loc.range.end.line,
                loc.range.end.character,
            ))
        });

        // Prefer non-carrier definitions over carrier re-export sites.
        // When CTRL+CLICKing a library symbol (e.g., `onClickOutside` from @vueuse/core),
        // TSGO may return both the real definition (.d.mts) and carrier consumer files.
        let has_non_carrier = locations
            .iter()
            .any(|l| !verter_workspace::path_is_carrier(l.uri.as_str()));
        if has_non_carrier {
            locations.retain(|l| !verter_workspace::path_is_carrier(l.uri.as_str()));
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
///
/// The `carrier_source_exists` predicate guards against collisions with real
/// `.vue.tsx`/`.vue.ts` files on disk: if the backing `.vue` source does
/// not exist in the host, the path is left unchanged. The `.vue.d.ts`
/// case (from node_modules) has no collision risk and skips the check.
fn normalize_carrier_path<'a>(
    path: &'a str,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> &'a str {
    // The IDE virtual file is `{carrier}.tsx`/`.jsx`; stripping the trailing
    // `.tsx`/`.jsx` yields a carrier path (`Foo.vue.tsx` → `Foo.vue`,
    // `Bar.svelte.tsx` → `Bar.svelte`). The carrier-extension set is the
    // registry's (`path_is_carrier`), not a `.vue` literal.
    if (path.ends_with(".tsx") || path.ends_with(".jsx"))
        && verter_workspace::path_is_carrier(&path[..path.len() - 4])
    {
        let candidate = &path[..path.len() - 4]; // strip .tsx/.jsx
        if carrier_source_exists(candidate) {
            return candidate;
        }
    } else if path.ends_with(".d.ts") && verter_workspace::path_is_carrier(&path[..path.len() - 5])
    {
        // The `{carrier}.d.ts` accepted-spelling alias — from node_modules, no
        // collision risk.
        return &path[..path.len() - 5];
    } else if path.ends_with(".ts") && verter_workspace::path_is_carrier(&path[..path.len() - 3]) {
        let candidate = &path[..path.len() - 3]; // strip .ts
        if carrier_source_exists(candidate) {
            return candidate;
        }
    }
    path
}

/// Whether `path` is a carrier IDE virtual file (`{carrier}.tsx` / `.jsx`) —
/// the TSGO IDE output that maps back to a carrier source through the source
/// map. Generalized to the registry carrier-extension set (Vue + Svelte).
fn is_carrier_ide_path(path: &str) -> bool {
    (path.ends_with(".tsx") || path.ends_with(".jsx"))
        && verter_workspace::path_is_carrier(&path[..path.len() - 4])
}

/// Whether `path` is a carrier API / DTS virtual file (`{carrier}.ts` /
/// `{carrier}.d.ts`) — the declaration surface (default-range, no position map).
///
/// CRITICAL: a `{carrier}.ts` form is AMBIGUOUS for Svelte — appending
/// `.ts` to `Foo.svelte` is the component API virtual file, but `store.svelte.ts`
/// is also a REAL first-class rune module (classifies as a non-component
/// adapter-module Script). We disambiguate by the backing carrier source: a
/// `{carrier}.ts` is the component API virtual file ONLY when the backing
/// `{carrier}` source EXISTS. A real rune module (no backing source) is NOT a
/// carrier virtual file — it serves its own canonical path directly. The
/// `{carrier}.d.ts` accepted-spelling alias (from node_modules) has no such
/// collision and skips the check — matching `normalize_carrier_path`'s guard.
fn is_carrier_api_or_dts_path(path: &str, carrier_source_exists: &dyn Fn(&str) -> bool) -> bool {
    if path.ends_with(".d.ts") && verter_workspace::path_is_carrier(&path[..path.len() - 5]) {
        return true;
    }
    if path.ends_with(".ts") && verter_workspace::path_is_carrier(&path[..path.len() - 3]) {
        return carrier_source_exists(&path[..path.len() - 3]);
    }
    false
}

/// Like `normalize_carrier_path` but returns an owned String.
/// Used by server.rs for inline path normalization.
pub fn normalize_carrier_path_owned(
    path: &str,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> String {
    normalize_carrier_path(path, carrier_source_exists).to_string()
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> Option<Vec<Location>> {
    let mut result = verter_refs.unwrap_or_default();

    for loc in &type_refs {
        // For .vue.tsx/.vue.jsx targets, map back to .vue positions
        if is_carrier_ide_path(&loc.path) {
            let range = resolve_carrier_tsx_range(
                &loc.path,
                loc.start,
                loc.end,
                tsx_line_index,
                mapper,
                carrier_line_index,
                external_resolver,
            );
            let carrier_path = normalize_carrier_path(&loc.path, carrier_source_exists);
            if let Some(uri) = path_to_uri(carrier_path) {
                // Deduplicate: skip if we already have a ref at this position
                let dup = result
                    .iter()
                    .any(|r| r.uri == uri && r.range.start == range.start);
                if !dup {
                    result.push(Location { uri, range });
                }
            }
        } else if is_carrier_api_or_dts_path(&loc.path, carrier_source_exists) {
            // DTS declarations (.vue.d.ts or .vue.ts): strip suffix, use default range
            let carrier_path = normalize_carrier_path(&loc.path, carrier_source_exists);
            if let Some(uri) = path_to_uri(carrier_path) {
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
#[allow(clippy::mutable_key_type, clippy::too_many_arguments)]
pub fn merge_rename_locations(
    verter_edit: Option<WorkspaceEdit>,
    type_locations: Vec<RenameLocation>,
    new_name: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> Option<WorkspaceEdit> {
    let mut edit = verter_edit.unwrap_or_else(|| WorkspaceEdit {
        changes: Some(std::collections::HashMap::new()),
        ..Default::default()
    });

    let changes = edit
        .changes
        .get_or_insert_with(std::collections::HashMap::new);

    for loc in &type_locations {
        if is_carrier_ide_path(&loc.path) {
            let range = resolve_carrier_tsx_range(
                &loc.path,
                loc.start,
                loc.end,
                tsx_line_index,
                mapper,
                carrier_line_index,
                external_resolver,
            );
            let carrier_path = normalize_carrier_path(&loc.path, carrier_source_exists);
            if let Some(uri) = path_to_uri(carrier_path) {
                let edits = changes.entry(uri).or_default();
                let dup = edits.iter().any(|e| e.range.start == range.start);
                if !dup {
                    edits.push(TextEdit {
                        range,
                        new_text: new_name.to_string(),
                    });
                }
            }
        } else if is_carrier_api_or_dts_path(&loc.path, carrier_source_exists) {
            let carrier_path = normalize_carrier_path(&loc.path, carrier_source_exists);
            if let Some(uri) = path_to_uri(carrier_path) {
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Option<Vec<DocumentHighlight>> {
    let mut result = verter_highlights.unwrap_or_default();

    for th in type_highlights {
        if let Some(range) =
            tsx_range_to_carrier_range(th.start, th.end, tsx_line_index, mapper, carrier_line_index)
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> Vec<CodeActionOrCommand> {
    type_actions
        .into_iter()
        .filter_map(|action| {
            let mut changes: std::collections::HashMap<Uri, Vec<TextEdit>> =
                std::collections::HashMap::new();

            for edit in action.edits {
                if is_carrier_ide_path(&edit.path) {
                    if let Some(range) = tsx_range_to_carrier_range(
                        edit.start,
                        edit.end,
                        tsx_line_index,
                        mapper,
                        carrier_line_index,
                    ) {
                        let carrier_path =
                            normalize_carrier_path(&edit.path, carrier_source_exists);
                        if let Some(uri) = path_to_uri(carrier_path) {
                            changes.entry(uri).or_default().push(TextEdit {
                                range,
                                new_text: edit.new_text,
                            });
                        }
                    }
                } else if is_carrier_api_or_dts_path(&edit.path, carrier_source_exists) {
                    let carrier_path = normalize_carrier_path(&edit.path, carrier_source_exists);
                    if let Some(uri) = path_to_uri(carrier_path) {
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Vec<tower_lsp_server::ls_types::SemanticToken> {
    // Map each token's whole half-open range `[start, start+length)` from TSX to Vue
    // through the strict run-compatible range API. A token is emitted ONLY when both
    // endpoints resolve inside compatible mapped runs; otherwise it is dropped. There is
    // NO fallback to the original TSX `token.length` when the end does not map — such a
    // fallback could emit a Vue token whose length straddled synthetic content.
    let mut mapped: Vec<(u32, u32, u32, u32, u32)> = Vec::new(); // (line, char, length, type, mods)

    for token in type_tokens {
        let Some(carrier_range) = tsx_range_to_carrier_range(
            token.start,
            token.start + token.length,
            tsx_line_index,
            mapper,
            carrier_line_index,
        ) else {
            continue;
        };

        // The strict range API only composes compatible runs, but a multi-line token would
        // produce a cross-line range; semantic tokens are single-line, so require it.
        if carrier_range.start.line != carrier_range.end.line
            || carrier_range.end.character < carrier_range.start.character
        {
            continue;
        }
        let carrier_length = carrier_range.end.character - carrier_range.start.character;

        // Skip zero-length tokens (collapsed by mapping)
        if carrier_length == 0 {
            continue;
        }

        mapped.push((
            carrier_range.start.line,
            carrier_range.start.character,
            carrier_length,
            token.token_type,
            token.token_modifiers,
        ));
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Vec<tower_lsp_server::ls_types::InlayHint> {
    let mut result = Vec::with_capacity(type_hints.len());

    for hint in type_hints {
        // Convert TSX byte offset → TSX line/col
        let Some(tsx_pos) = tsx_line_index.offset_to_position(hint.position) else {
            continue;
        };

        // Map TSX line/col → Vue line/col via sourcemap
        let Some(carrier_mapped) = mapper
            .tsx_to_carrier(TsPosition::new(tsx_pos.line, tsx_pos.character))
            .map(|m| m.pos)
        else {
            continue;
        };

        let carrier_pos = Position {
            line: carrier_mapped.line,
            character: carrier_mapped.character,
        };

        // Validate the Vue position is within bounds
        if carrier_line_index
            .position_to_offset(&carrier_pos)
            .is_none()
        {
            continue;
        }

        let kind = hint.kind.map(|k| match k {
            InlayHintKind::Type => tower_lsp_server::ls_types::InlayHintKind::TYPE,
            InlayHintKind::Parameter => tower_lsp_server::ls_types::InlayHintKind::PARAMETER,
        });

        result.push(tower_lsp_server::ls_types::InlayHint {
            position: carrier_pos,
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
    use crate::documents::position_map::PositionMapper;

    // ── Position mapping tests ─────────────────────────────────────

    fn make_mapper_and_indexes() -> (ProviderPositionMapper, LineIndex, LineIndex) {
        // Vue source (line 0-1: template, line 3-4: script)
        let carrier_source = "<template>\n  <div>{{ msg }}</div>\n</template>\n\n<script setup>\nconst msg = \"hello\";\n</script>";
        // TSX source (script at line 0)
        let tsx_source = "const msg = \"hello\";\n";

        // Source map: TSX line 0 col 0 → Vue line 5 col 0
        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        let source_id = builder.set_source_and_content("App.vue", carrier_source);
        builder.add_token(0, 0, 5, 0, Some(source_id), None);
        builder.add_token(0, 6, 5, 6, Some(source_id), None);
        builder.add_token(0, 10, 5, 10, Some(source_id), None);
        let json = builder.into_sourcemap().to_json_string();

        let mapper = ProviderPositionMapper::source_map(PositionMapper::from_json(&json).unwrap());
        let carrier_li = LineIndex::new_utf16(carrier_source);
        let tsx_li = LineIndex::new_utf16(tsx_source);

        (mapper, carrier_li, tsx_li)
    }

    /// @ai-generated — Vue position maps to correct TSX byte offset
    #[test]
    fn vue_position_maps_to_tsx_offset() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        // Vue line 5, col 6 ("msg") → TSX line 0, col 6 → byte offset 6
        let offset = carrier_position_to_tsx_offset(
            &Position {
                line: 5,
                character: 6,
            },
            &carrier_li,
            &mapper,
            &tsx_li,
        );
        assert_eq!(offset, Some(6));
    }

    /// @ai-generated — Unmappable Vue position returns None
    #[test]
    fn unmappable_vue_position_returns_none() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        // Line 0 is in the template, not mapped in our source map
        let offset = carrier_position_to_tsx_offset(
            &Position {
                line: 0,
                character: 0,
            },
            &carrier_li,
            &mapper,
            &tsx_li,
        );
        assert!(offset.is_none());
    }

    /// Range endpoint compatibility: build a mapper with TWO mapped runs separated by
    /// synthetic/unmapped content. A TSX range whose endpoints fall in the two DIFFERENT
    /// runs must be DROPPED by `tsx_range_to_carrier_range` (the strict run-compatibility
    /// check), while a range fully inside ONE run maps correctly.
    ///
    /// Discriminating: a per-endpoint composer maps each endpoint independently and returns
    /// `Some` whenever both endpoints individually map — so the cross-run range produces a
    /// bogus Vue range straddling the synthetic content. The strict API returns `None`.
    #[test]
    fn tsx_range_rejects_cross_run_endpoints_with_synthetic_between() {
        // TSX single line "abcXXXXdef" (byte offset == UTF-16 col).
        let tsx_source = "abcXXXXdef";
        // Vue single line, long enough to hold the mapped source columns.
        let carrier_source = &" ".repeat(80);

        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        let source_id = builder.set_source_and_content("App.vue", carrier_source);
        // mapped run A: gen(0,0)->src(0,0), bounded to [0,3) by the unmapped token at 3.
        builder.add_token(0, 0, 0, 0, Some(source_id), None);
        // unmapped synthetic token at gen col 3 ("XXXX").
        builder.add_token(0, 3, 0, 0, None, None);
        // mapped run B: gen(0,7)->src(0,50).
        builder.add_token(0, 7, 0, 50, Some(source_id), None);
        let json = builder.into_sourcemap().to_json_string();

        let pm = PositionMapper::from_json(&json).unwrap();
        let tsx_li = LineIndex::new_utf16(tsx_source);
        let carrier_li = LineIndex::new_utf16(carrier_source);

        // Precondition: both endpoints individually map (start byte 1 -> run A,
        // end byte 9 -> run B), so the *old* per-endpoint composer returned Some.
        assert!(pm.tsx_to_carrier(TsPosition::new(0, 1)).is_some());
        assert!(pm.tsx_to_carrier(TsPosition::new(0, 9)).is_some());
        let mapper = ProviderPositionMapper::source_map(pm);

        // Cross-run range straddling the synthetic "XXXX" -> dropped.
        assert!(
            tsx_range_to_carrier_range(1, 9, &tsx_li, &mapper, &carrier_li).is_none(),
            "a TSX range whose endpoints land in two runs separated by synthetic content \
             must be dropped, not composed into a bogus Vue range"
        );

        // In-run range fully inside run A [0,3) -> maps.
        let r = tsx_range_to_carrier_range(1, 3, &tsx_li, &mapper, &carrier_li)
            .expect("range fully inside one mapped run must map");
        assert_eq!(
            r.start,
            Position {
                line: 0,
                character: 1
            }
        );
        assert_eq!(
            r.end,
            Position {
                line: 0,
                character: 3
            }
        );
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
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
            &carrier_li,
            None,
            None,
        );
        let text = extract_hover_text(&result.unwrap());
        assert!(text.contains("const msg: string"));
        assert!(text.contains("SetupConst"));
    }

    /// @ai-generated — Only verter hover present
    #[test]
    fn merge_hover_verter_only() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = make_verter_hover("**msg** (SetupConst)");

        let result = merge_hover(
            Some(verter),
            None,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            None,
        );
        assert!(result.is_some());
        let text = extract_hover_text(&result.unwrap());
        assert!(text.contains("SetupConst"));
    }

    /// @ai-generated — Only type hover present
    #[test]
    fn merge_hover_type_only() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let type_hover = HoverInfo {
            contents: "const msg: string".to_string(),
            range_start: None,
            range_end: None,
        };

        let result = merge_hover(
            None,
            Some(type_hover),
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            None,
        );
        assert!(result.is_some());
        let text = extract_hover_text(&result.unwrap());
        assert!(text.contains("const msg: string"));
    }

    /// @ai-generated — Neither hover present returns None
    #[test]
    fn merge_hover_neither() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_hover(None, None, &mapper, &tsx_li, &carrier_li, None, None);
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![make_type_completion("count"), make_type_completion("name")],
            is_incomplete: false,
        };

        let (result, is_incomplete) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![make_type_completion("msg")], // duplicate
            is_incomplete: false,
        };

        let (result, _) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "msg");
    }

    /// @ai-generated — ___VERTER___ prefixed completions are filtered
    #[test]
    fn merge_completions_filters_verter_internal() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("msg"),
                make_type_completion("___VERTER___hidden"),
            ],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "msg");
    }

    /// @ai-generated — is_incomplete flag is propagated from TypeProvider result
    #[test]
    fn merge_completions_propagates_is_incomplete() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![make_type_completion("count")],
            is_incomplete: true,
        };

        let (result, is_incomplete) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
        assert_eq!(result.len(), 2);
        assert!(
            is_incomplete,
            "is_incomplete should be propagated from TSGO"
        );
    }

    /// @ai-generated — $V_ prefixed type helpers are filtered
    #[test]
    fn merge_completions_filters_dollar_v_prefix() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("msg"),
                make_type_completion("$V_EmitsToProps"),
            ],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "msg");
    }

    /// @ai-generated — TSGO-internal duplicates are deduplicated
    #[test]
    fn merge_completions_deduplicates_tsgo_internal() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("onMounted"), // local binding
                make_type_completion("onMounted"), // auto-import suggestion (same label)
            ],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("onMounted")];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("onMounted"), // TSGO local
                make_type_completion("onMounted"), // TSGO auto-import
                make_type_completion("ref"),       // unique
            ],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
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

        let (result, _) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_diagnostic("parse error")];
        let types = vec![TypeDiagnostic {
            message: "Type 'number' is not assignable to type 'string'".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: 6, // TSX offset for "msg"
            end: 9,
            code: Some("2322".to_string()),
        }];

        let result = merge_diagnostics(verter, types, &tsx_li, &mapper, &carrier_li);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source.as_deref(), Some("verter"));
        assert_eq!(result[1].source.as_deref(), Some("ts"));
        assert!(result[1].message.contains("not assignable"));
    }

    /// @ai-generated — Type diagnostics in unmapped regions are filtered out
    #[test]
    fn merge_diagnostics_filters_unmapped() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        // Offset 100 is beyond the TSX source
        let types = vec![TypeDiagnostic {
            message: "error in generated code".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: 100,
            end: 110,
            code: None,
        }];

        let result = merge_diagnostics(verter, types, &tsx_li, &mapper, &carrier_li);
        assert!(result.is_empty(), "unmapped diagnostics should be filtered");
    }

    // ── Definition merge tests ─────────────────────────────────────

    fn test_doc_uri() -> Uri {
        "file:///test.vue".parse().unwrap()
    }

    /// Build an in-memory external source fixture for the definition/type-definition merge
    /// path: a synthetic forward-slash path (with `suffix`) plus an [`ExternalSourceReader`]
    /// that returns the content for that exact path, modeling the host VFS the production
    /// merge reads through (`VerterHost::workspace_read().read_file` → `WorkspaceRead::read_file`).
    /// Definition targets carry byte offsets into their own source, so the reader hands that
    /// exact source back for the offset→line:col conversion — no disk I/O.
    fn ext_source(suffix: &str, content: &str) -> (String, impl Fn(&str) -> Option<Arc<str>>) {
        let path = format!("/virtual/external{suffix}");
        let content: Arc<str> = Arc::from(content);
        let reader_path = path.clone();
        let reader = move |p: &str| (p == reader_path.as_str()).then(|| content.clone());
        (path, reader)
    }

    /// Reader for cases that never reach the external-source path (empty type defs,
    /// verter-preferred, or `.vue.tsx`/`.vue.d.ts` targets resolved before/without a source
    /// read): always `None`. Passing it documents that no external source is consulted.
    fn no_external_source(_path: &str) -> Option<Arc<str>> {
        None
    }

    /// @ai-generated — Verter definition is preferred when no type definitions
    #[test]
    fn merge_definitions_verter_only() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(GotoDefinitionResponse::Scalar(Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range::default(),
        }));

        let result = merge_definitions(
            verter,
            vec![],
            "",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &no_external_source,
        );
        assert!(result.is_some());
    }

    /// @ai-generated — Type definitions used when verter has none
    #[test]
    fn merge_definitions_type_only() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        // A real external `.ts` whose byte offsets index its own source. `sym` sits on
        // line 1, so a faithful resolve lands on line 1 (not the old line-0 default).
        let source = "export {}\nexport const sym = 1\n";
        let off = source.find("sym").unwrap() as u32;
        let (ts_path, read_source) = ext_source(".ts", source);
        let types = vec![TypeLocation {
            path: ts_path.clone(),
            start: off,
            end: off + 3,
        }];

        let result = merge_definitions(
            None,
            types,
            "",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &read_source,
        );
        match result {
            Some(GotoDefinitionResponse::Scalar(loc)) => {
                assert!(loc.uri.as_str().ends_with(".ts"));
                assert_eq!(
                    loc.range.start.line, 1,
                    "external target must resolve to the real symbol line, not line 0"
                );
            }
            other => panic!("expected a resolved external definition, got {other:?}"),
        }
    }

    /// @ai-generated — Neither source returns None
    #[test]
    fn merge_definitions_neither() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_definitions(
            None,
            vec![],
            "",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &no_external_source,
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(vec![Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range::default(),
        }]);
        let type_refs = vec![TypeLocation {
            path: "/project/utils.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_references(
            verter,
            type_refs,
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &carrier_exists,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 2);
    }

    /// @ai-generated — Empty refs from both returns None
    #[test]
    fn merge_references_neither() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_references(
            None,
            vec![],
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &carrier_exists,
        );
        assert!(result.is_none());
    }

    /// @ai-generated — Verter-only refs returned as-is
    #[test]
    fn merge_references_verter_only() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(vec![Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range::default(),
        }]);

        let result = merge_references(
            verter,
            vec![],
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &carrier_exists,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    // ── Document highlights merge tests ───────────────────────────────

    /// @ai-generated — Type highlights mapped and merged with verter highlights
    #[test]
    fn merge_highlights_both_present() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
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

        let result =
            merge_document_highlights(verter, type_highlights, &tsx_li, &mapper, &carrier_li);
        assert!(result.is_some());
        // Should be 1 (deduplicated since both point to line 5, col 6)
        assert_eq!(result.unwrap().len(), 1);
    }

    /// @ai-generated — Neither highlights returns None
    #[test]
    fn merge_highlights_neither() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_document_highlights(None, vec![], &tsx_li, &mapper, &carrier_li);
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
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

        let result = merge_code_actions(actions, &tsx_li, &mapper, &carrier_li, &carrier_exists);
        assert_eq!(result.len(), 1);
    }

    /// @ai-generated — Empty actions returns empty vec
    #[test]
    fn merge_code_actions_empty() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_code_actions(vec![], &tsx_li, &mapper, &carrier_li, &carrier_exists);
        assert!(result.is_empty());
    }

    // ── Semantic tokens merge tests ───────────────────────────────────

    /// @ai-generated — Semantic tokens mapped from TSX to Vue
    #[test]
    fn merge_semantic_tokens_basic() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        // Token at TSX offset 6 (= "msg"), length 3
        let tokens = vec![protocol::SemanticToken {
            start: 6,
            length: 3,
            token_type: 8, // VARIABLE
            token_modifiers: 0,
        }];

        let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &carrier_li);
        assert_eq!(result.len(), 1);
        // Should map to Vue line 5, col 6
        assert_eq!(result[0].length, 3);
        assert_eq!(result[0].token_type, 8);
    }

    /// @ai-generated — Empty tokens returns empty vec
    #[test]
    fn merge_semantic_tokens_empty() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_semantic_tokens(vec![], &tsx_li, &mapper, &carrier_li);
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        let tokens = vec![protocol::SemanticToken {
            start: 6,  // TSX offset of 'msg'
            length: 3, // length in TSX = 3
            token_type: 8,
            token_modifiers: 0,
        }];

        let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &carrier_li);
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        // TSX: "const msg = \"hello\";\n" (20 chars)
        // Token spanning from col 0 with excessive length that crosses line boundary
        let tokens = vec![protocol::SemanticToken {
            start: 0,
            length: 100, // way past end of line — would cross line boundaries
            token_type: 8,
            token_modifiers: 0,
        }];

        let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &carrier_li);
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
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

        let result = merge_rename_locations(
            verter,
            vec![],
            "newName",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &carrier_exists,
        );
        assert!(result.is_some());
    }

    /// @ai-generated — Empty rename from both returns None
    #[test]
    fn merge_rename_neither() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_rename_locations(
            None,
            vec![],
            "newName",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &carrier_exists,
        );
        assert!(result.is_none());
    }

    // ── Definition merge tests (Bug 2) ───────────────────────────────

    /// A `.vue.tsx` target that IS the file being queried (`loc.path == current_tsx_path`)
    /// maps its byte offsets back to Vue through the in-context mapper — no external resolver
    /// needed, and never the old `Range::default()` (0,0) collapse.
    #[test]
    fn merge_definitions_maps_current_file_carrier_tsx_to_carrier_positions() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        // The current document's generated TSX. TSX offset 6..9 = "msg" (in "const msg = ..."),
        // which the in-context mapper carries back to Vue line 5, col 6..9.
        let current_tsx_path = "/home/user/App.vue.tsx";
        let type_defs = vec![TypeLocation {
            path: current_tsx_path.to_string(),
            start: 6,
            end: 9,
        }];

        let result = merge_definitions(
            None,
            type_defs,
            current_tsx_path,
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &no_external_source,
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
                // Exact full range: "msg" at Vue line 5, cols 6..9 — not the (0,0) default,
                // and not just "some non-zero line".
                assert_ne!(
                    loc.range,
                    Range::default(),
                    "current-file .vue.tsx range must not collapse to (0,0)"
                );
                assert_eq!(
                    loc.range,
                    Range {
                        start: Position {
                            line: 5,
                            character: 6,
                        },
                        end: Position {
                            line: 5,
                            character: 9,
                        },
                    },
                    "expected exact Vue range (5,6)..(5,9) for 'msg'"
                );
            }
            _ => panic!("Unexpected definition response type"),
        }
    }

    /// A non-`.vue` target keeps its own URI (no normalization) AND resolves its byte
    /// offsets against its own source to a real `Range` — not the old line-0 default.
    #[test]
    fn merge_definitions_non_carrier_target_resolves_real_range() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        // `helper` is on line 2 of the fixture, so a faithful resolve lands on line 2.
        let source = "export {}\n\nexport function helper() {}\n";
        let off = source.find("helper").unwrap() as u32;
        let (ts_path, read_source) = ext_source(".ts", source);
        let type_defs = vec![TypeLocation {
            path: ts_path.clone(),
            start: off,
            end: off + 6,
        }];

        let result = merge_definitions(
            None,
            type_defs,
            "",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &read_source,
        );
        match result {
            Some(GotoDefinitionResponse::Scalar(loc)) => {
                // URI passes through unchanged (a non-`.vue` target is not normalized).
                assert!(
                    loc.uri.as_str().ends_with(".ts") && !loc.uri.as_str().contains(".vue"),
                    "external .ts URI should pass through unchanged, got: {}",
                    loc.uri.as_str()
                );
                // Range resolves to the real symbol line, not the old (0,0) default.
                assert_eq!(loc.range.start.line, 2, "must land on the real symbol line");
                assert_ne!(loc.range, Range::default(), "must not collapse to line 0");
            }
            other => panic!("expected a resolved external definition, got {other:?}"),
        }
    }

    #[test]
    fn merge_definitions_uses_barrel_resolver_for_non_carrier_targets() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let type_defs = vec![TypeLocation {
            path: "/home/user/index.ts".to_string(),
            start: 20,
            end: 27,
        }];
        let expected = Location {
            uri: file_path_to_uri("/home/user/Overlay.vue").unwrap(),
            range: Range {
                start: Position {
                    line: 3,
                    character: 2,
                },
                end: Position {
                    line: 3,
                    character: 9,
                },
            },
        };
        let resolver = |path: &str, start: u32, end: u32| {
            if path == "/home/user/index.ts" && start == 20 && end == 27 {
                Some(expected.clone())
            } else {
                None
            }
        };

        let result = merge_definitions_with_barrel_resolver(
            None,
            type_defs,
            "",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            Some(&resolver),
            PositionEncodingKind::UTF16,
            &no_external_source,
        );

        match result {
            Some(GotoDefinitionResponse::Scalar(loc)) => assert_eq!(loc, expected),
            other => panic!("expected scalar resolved location, got {:?}", other),
        }
    }

    /// Regression: when verter resolves to a same-file import and TSGO resolves
    /// to an external file (e.g., runtime-dom.d.ts), TSGO's cross-file result
    /// must win — verter's same-file import is just an intermediate step.
    #[test]
    fn merge_definitions_tsgo_external_overrides_verter_same_file() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        // Verter found the import statement (same file — uses SAME_FILE_URI sentinel)
        let verter_def = Some(GotoDefinitionResponse::Scalar(Location {
            uri: crate::features::definition::SAME_FILE_URI.clone(),
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

        // TSGO resolved to a real external .d.ts file (stands in for runtime-dom.d.ts).
        // `defineProps` sits on line 1, so the cross-file result resolves to line 1.
        let source = "export {}\nexport declare function defineProps(): void\n";
        let off = source.find("defineProps").unwrap() as u32;
        let (dts_path, read_source) = ext_source(".d.ts", source);
        let type_defs = vec![TypeLocation {
            path: dts_path.clone(),
            start: off,
            end: off + "defineProps".len() as u32,
        }];

        let result = merge_definitions(
            verter_def,
            type_defs,
            "",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &read_source,
        );
        assert!(result.is_some(), "should return TSGO's external definition");

        match result.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.as_str().ends_with(".d.ts"),
                    "should navigate to external .d.ts file, got: {}",
                    loc.uri.as_str()
                );
                // The external result resolves to the real declaration line (not line 0).
                assert_eq!(
                    loc.range.start.line, 1,
                    "must resolve to the real symbol line"
                );
                // Negative: must NOT be the same-file sentinel URI
                assert!(
                    !loc.uri
                        .as_str()
                        .contains(crate::features::definition::SAME_FILE_URI_STR),
                    "must not return same-file sentinel when TSGO has external target"
                );
            }
            _ => panic!("Expected scalar definition for single external target"),
        }
    }

    /// @ai-generated — merge_definitions prefers verter when type_defs is empty
    #[test]
    fn merge_definitions_verter_preferred_when_no_type_defs() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

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
            "",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &no_external_source,
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

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
            "/test.vue.tsx",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &no_external_source,
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

    // ── normalize_carrier_path tests ────────────────────────────────────

    /// Predicate that always returns true — for tests where the .vue source is known to exist.
    fn carrier_exists(_: &str) -> bool {
        true
    }

    /// Predicate that always returns false — simulates a real .vue.tsx file with no backing .vue.
    fn carrier_missing(_: &str) -> bool {
        false
    }

    #[test]
    fn normalize_carrier_path_strips_tsx() {
        assert_eq!(
            normalize_carrier_path("/src/App.vue.tsx", &carrier_exists),
            "/src/App.vue"
        );
    }

    #[test]
    fn normalize_carrier_path_strips_dts() {
        assert_eq!(
            normalize_carrier_path("/node_modules/lib/Comp.vue.d.ts", &carrier_exists),
            "/node_modules/lib/Comp.vue"
        );
    }

    #[test]
    fn normalize_carrier_path_strips_vue_ts() {
        assert_eq!(
            normalize_carrier_path("/src/App.vue.ts", &carrier_exists),
            "/src/App.vue"
        );
    }

    #[test]
    fn normalize_carrier_path_strips_vue_jsx() {
        assert_eq!(
            normalize_carrier_path("/src/App.vue.jsx", &carrier_exists),
            "/src/App.vue"
        );
    }

    #[test]
    fn normalize_carrier_path_strips_svelte_virtual_suffixes() {
        // Generalized to the carrier-extension set: a `.svelte` IDE/api/dts
        // virtual file normalizes back to the `.svelte` source.
        assert_eq!(
            normalize_carrier_path("/src/Comp.svelte.tsx", &carrier_exists),
            "/src/Comp.svelte"
        );
        assert_eq!(
            normalize_carrier_path("/src/Comp.svelte.ts", &carrier_exists),
            "/src/Comp.svelte"
        );
        assert_eq!(
            normalize_carrier_path("/node_modules/lib/C.svelte.d.ts", &carrier_exists),
            "/node_modules/lib/C.svelte"
        );
        assert!(is_carrier_ide_path("/src/Comp.svelte.tsx"));
        // `Comp.svelte.ts` is the api virtual file ONLY when `Comp.svelte`
        // EXISTS (disambiguation against a real `.svelte.ts` rune module).
        assert!(is_carrier_api_or_dts_path(
            "/src/Comp.svelte.ts",
            &carrier_exists
        ));
        // A plain `.ts`/`.tsx` is NOT a carrier virtual file (negative).
        assert!(!is_carrier_ide_path("/src/plain.tsx"));
        assert_eq!(
            normalize_carrier_path("/src/plain.ts", &carrier_exists),
            "/src/plain.ts"
        );
    }

    #[test]
    fn real_svelte_rune_module_is_not_a_carrier_virtual_file() {
        // Co-existence: `store.svelte.ts` with NO backing `store.svelte` is
        // a REAL first-class rune module — NOT the `{carrier}.ts` component API
        // virtual file. The existence guard disambiguates it from `Foo.svelte` +
        // `.ts` (the component API virtual file exists ONLY when `Foo.svelte`
        // backs it); the rune module's own provider surface is served from its
        // own canonical path, never normalized to a sibling `.svelte` component.
        assert!(!is_carrier_api_or_dts_path(
            "/src/store.svelte.ts",
            &carrier_missing
        ));
        // And it is NOT normalized to a sibling `.svelte` (the strip is guarded).
        assert_eq!(
            normalize_carrier_path("/src/store.svelte.ts", &carrier_missing),
            "/src/store.svelte.ts"
        );
    }

    #[test]
    fn normalize_carrier_path_passthrough_plain_dts() {
        // Non-.vue .d.ts files should NOT be stripped
        assert_eq!(
            normalize_carrier_path(
                "/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts",
                &carrier_exists
            ),
            "/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts"
        );
    }

    #[test]
    fn normalize_carrier_path_passthrough_plain_ts() {
        // Non-.vue .ts files should NOT be stripped
        assert_eq!(
            normalize_carrier_path("/src/utils.ts", &carrier_exists),
            "/src/utils.ts"
        );
    }

    #[test]
    fn normalize_carrier_path_skips_real_vue_tsx() {
        // A real .vue.tsx file on disk (no backing .vue source) must NOT be stripped
        assert_eq!(
            normalize_carrier_path("/src/App.vue.tsx", &carrier_missing),
            "/src/App.vue.tsx",
            "real .vue.tsx should be left unchanged when no .vue source exists"
        );
    }

    #[test]
    fn normalize_carrier_path_skips_real_vue_ts() {
        assert_eq!(
            normalize_carrier_path("/src/App.vue.ts", &carrier_missing),
            "/src/App.vue.ts",
            "real .vue.ts should be left unchanged when no .vue source exists"
        );
    }

    #[test]
    fn normalize_carrier_path_strips_virtual_vue_tsx() {
        // Virtual .vue.tsx with a backing .vue source SHOULD be stripped
        let exists_for_app = |path: &str| path == "/src/App.vue";
        assert_eq!(
            normalize_carrier_path("/src/App.vue.tsx", &exists_for_app),
            "/src/App.vue",
            "virtual .vue.tsx should strip to .vue when source exists"
        );
    }

    #[test]
    fn normalize_carrier_path_dts_always_strips_regardless_of_predicate() {
        // .vue.d.ts from node_modules has no collision risk — always strip
        assert_eq!(
            normalize_carrier_path("/node_modules/lib/Comp.vue.d.ts", &carrier_missing),
            "/node_modules/lib/Comp.vue",
            ".vue.d.ts should always strip regardless of predicate"
        );
    }

    // ── .vue.d.ts definition tests ──────────────────────────────────

    /// A `.vue.d.ts` definition target fails closed. Its byte offsets index the generated
    /// declaration file, but the URI we would emit is the `.vue` source (path normalization
    /// rewrites `.vue.d.ts` → `.vue`) and no in-context sourcemap bridges them. Rather than
    /// manufacture a line-0 `Range` into the wrong file, the merge drops the location.
    #[test]
    fn merge_definitions_carrier_dts_fails_closed_no_line_zero() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        let type_defs = vec![TypeLocation {
            path: "/node_modules/my-lib/dist/Button.vue.d.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_definitions(
            None,
            type_defs,
            "",
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &no_external_source,
        );
        // No real range is available, so no location is produced — never a (0,0) default.
        assert!(
            result.is_none(),
            "must fail closed for a `.vue.d.ts` target, got: {result:?}"
        );
    }

    /// .vue.d.ts references should map to .vue
    #[test]
    fn merge_references_vue_dts_maps_to_vue() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        let type_refs = vec![TypeLocation {
            path: "/node_modules/my-lib/dist/Button.vue.d.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_references(
            None,
            type_refs,
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &carrier_exists,
        );
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

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
            &carrier_li,
            None,
            &carrier_exists,
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
        let carrier_li = LineIndex::new_utf16("");

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

        let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
        let carrier_li = LineIndex::new_utf16("");

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

        let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
        let carrier_li = LineIndex::new_utf16("");

        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "```typescript\n(property) msg: string\n```\nThe message.".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
        let carrier_li = LineIndex::new_utf16("");

        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "(property) msg: string".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
        let carrier_li = LineIndex::new_utf16("");

        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "```typescript\n(property) select: (action: Action) => true\n```\nEmitted when selected.\n当选择时触发。".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
        let carrier_li = LineIndex::new_utf16("");

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

        let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
        let carrier_li = LineIndex::new_utf16("");

        // TSGO returns plain text with type and doc separated by blank line
        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "(property) GameItemProps.game: GameVo | ProfilePlayedVo\n\n游戏数据"
                .to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
        let carrier_li = LineIndex::new_utf16("");

        // TSGO returns plain text with type and doc separated by single newline
        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "(property) game: GameVo\nThe game data.".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
        let carrier_li = LineIndex::new_utf16("");

        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "(property) msg: string".to_string(),
        });

        let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
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
            &carrier_li,
            Some("ref"),
            None,
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

    #[test]
    fn merge_hover_rewrites_primary_label_from_typed_event_provenance() {
        // The `onCustom` → `@custom` rewrite is driven by the TYPED
        // `HoverSourceToken::EventDirective` provenance, never by reparsing the
        // rendered verter hover text.
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = make_verter_hover("`@custom`\n\nListens for the `custom` event.");
        let type_hover = HoverInfo {
            contents: "(property) onCustom: (payload: string) => void".to_string(),
            range_start: None,
            range_end: None,
        };

        let token = HoverSourceToken::EventDirective {
            vue_attr: "@custom".to_string(),
        };
        let result = merge_hover(
            Some(verter),
            Some(type_hover),
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            Some(&token),
        );
        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        let first_content_line = text.lines().nth(1).unwrap_or_default();
        assert!(
            first_content_line.contains("@custom"),
            "primary hover label should use Vue event syntax, got: {text}"
        );
        assert!(
            !first_content_line.contains("onCustom"),
            "primary hover label must not expose TSX on* naming, got: {text}"
        );
    }

    #[test]
    fn merge_hover_does_not_rewrite_label_without_typed_provenance() {
        // Discriminating: even when the verter hover TEXT contains a backticked
        // `@custom` token, the merge layer must NOT rewrite the TypeProvider label
        // unless TYPED provenance is supplied. This proves the markdown side-channel
        // (the former `extract_vue_attr_label` text reparse) is gone.
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        let verter = make_verter_hover("`@custom`\n\nSome descriptive context.");
        let type_hover = HoverInfo {
            contents: "(property) onCustom: (payload: string) => void".to_string(),
            range_start: None,
            range_end: None,
        };

        let result = merge_hover(
            Some(verter),
            Some(type_hover),
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            None,
        );
        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        let first_content_line = text.lines().nth(1).unwrap_or_default();
        assert!(
            first_content_line.contains("onCustom"),
            "without typed provenance the generated label must be preserved, got: {text}"
        );
        assert!(
            !first_content_line.contains("@custom"),
            "no text-based rewrite may occur without typed provenance, got: {text}"
        );
    }

    /// Cross-file (foreign) carrier IDE definition resolution is fail-closed and exact.
    ///
    /// When TSGO returns a carrier IDE target that is NOT the file being queried, only that
    /// target's own sourcemap (via the external resolver) can map its byte offsets back to the
    /// carrier source. Without a resolver the location is DROPPED — never collapsed to a line-0
    /// range pointing into the wrong file (the bug this guards). With the resolver it maps to
    /// the exact carrier range.
    #[test]
    fn merge_definitions_foreign_carrier_tsx_fails_closed_without_resolver_else_exact() {
        let (_mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        // The file being queried (its in-context mapper) — distinct from the target, so the
        // target is genuinely FOREIGN and the current mapper must never be used for it.
        let current_tsx_path = "/src/components/Caller.vue.tsx";

        // Build the target file's own mapper: TSX 0:0 → Vue 1:0, TSX 0:16 → Vue 1:16.
        let target_carrier = "<script setup>\ndefineComponent({})\n</script>";
        let target_tsx = "defineComponent({});\n";
        let target_carrier_li = LineIndex::new_utf16(target_carrier);
        let target_tsx_li = LineIndex::new_utf16(target_tsx);

        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        let sid = builder.set_source_and_content("Target.vue", target_carrier);
        builder.add_token(0, 0, 1, 0, Some(sid), None); // TSX 0:0 → Vue 1:0
        builder.add_token(0, 16, 1, 16, Some(sid), None); // TSX 0:16 → Vue 1:16
        let json = builder.into_sourcemap().to_json_string();
        let target_mapper =
            ProviderPositionMapper::source_map(PositionMapper::from_json(&json).unwrap());

        let type_defs = vec![TypeLocation {
            path: "/src/components/Target.vue.tsx".to_string(),
            start: 0,
            end: 16, // "defineComponent("
        }];

        // Without a resolver the foreign target has no usable sourcemap → fail closed. The
        // current file's mapper describes a DIFFERENT file and must NOT be reused, so the only
        // location is dropped and the merge returns the (empty) verter result.
        let result_no_resolver = merge_definitions(
            None,
            type_defs.clone(),
            current_tsx_path,
            &tsx_li,
            &_mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &no_external_source,
        );
        assert!(
            result_no_resolver.is_none(),
            "foreign carrier IDE target with no resolver must be DROPPED, never a line-0 range: {result_no_resolver:?}"
        );

        // With the resolver: the target's own mapper resolves the offsets to the exact range.
        let resolver = |ide_path: &str| -> Option<ExternalIdeContext> {
            if ide_path == "/src/components/Target.vue.tsx" {
                Some(ExternalIdeContext {
                    tsx_line_index: target_tsx_li.clone(),
                    mapper: target_mapper.clone(),
                    carrier_line_index: target_carrier_li.clone(),
                })
            } else {
                None
            }
        };

        let result_with_resolver = merge_definitions(
            None,
            type_defs,
            current_tsx_path,
            &tsx_li,
            &_mapper,
            &carrier_li,
            Some(&resolver),
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &no_external_source,
        );
        match result_with_resolver {
            Some(GotoDefinitionResponse::Scalar(loc)) => {
                assert!(
                    loc.uri.as_str().ends_with("Target.vue"),
                    "should navigate to .vue: {}",
                    loc.uri.as_str()
                );
                // Exact full range: "defineComponent(" at Vue (1,0)..(1,16) — both endpoints,
                // not just "line 1", and never the (0,0) default.
                assert_eq!(
                    loc.range,
                    Range {
                        start: Position {
                            line: 1,
                            character: 0,
                        },
                        end: Position {
                            line: 1,
                            character: 16,
                        },
                    },
                    "with resolver, expected exact Vue range (1,0)..(1,16), got: {:?}",
                    loc.range
                );
                assert_ne!(
                    loc.range,
                    Range::default(),
                    "with resolver, range must not be the (0,0) default"
                );
            }
            other => panic!("expected scalar definition, got: {other:?}"),
        }
    }

    // ── Definition deduplication and filtering tests ──────────────

    #[test]
    fn merge_definitions_deduplicates_identical_carrier_locations() {
        // Two identical carrier IDE spans for the file currently being queried map through the
        // in-context mapper to the same carrier range, so they are true duplicates and collapse
        // to a single location. (Distinct ranges in one file are kept; that is covered by the
        // same-file multi-definition test.)
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        let current_tsx_path = "/src/components/Dropdown.vue.tsx";
        let type_defs = vec![
            TypeLocation {
                path: current_tsx_path.to_string(),
                start: 6,
                end: 10,
            },
            TypeLocation {
                path: current_tsx_path.to_string(),
                start: 6,
                end: 10,
            },
        ];

        let result = merge_definitions(
            None,
            type_defs,
            current_tsx_path,
            &tsx_li,
            &mapper,
            &carrier_li,
            None,
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &no_external_source,
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
    fn merge_definitions_filters_vue_when_non_carrier_exists() {
        // Bug: when CTRL+CLICKing on an import from a library, both .d.mts (real def)
        // and .vue.tsx (consumer) are returned. Should filter out .vue targets.
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        // The real library definition lives in an on-disk `.d.mts` (its offsets index
        // its own source); the two `.vue.tsx` consumer spans normalize to `.vue`.
        let source = "export {}\nexport declare function onClickOutside(): void\n";
        let off = source.find("onClickOutside").unwrap() as u32;
        let (dmts_path, read_source) = ext_source(".d.mts", source);
        let type_defs = vec![
            TypeLocation {
                path: dmts_path.clone(),
                start: off,
                end: off + "onClickOutside".len() as u32,
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

        // Both foreign carrier IDE consumers map back to real carrier ranges through the
        // external resolver (their own sourcemaps), so the filter sees genuine carrier locations
        // to drop — not the old line-0 fallback that fail-closed resolution removed.
        let consumer_carrier = "<script setup>\nconst x = 1;\n</script>";
        let consumer_tsx = "const x = 1;\n";
        let consumer_carrier_li = LineIndex::new_utf16(consumer_carrier);
        let consumer_tsx_li = LineIndex::new_utf16(consumer_tsx);
        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        let sid = builder.set_source_and_content("Consumer.vue", consumer_carrier);
        builder.add_token(0, 0, 1, 0, Some(sid), None); // TSX 0:0 → Vue 1:0
        builder.add_token(0, 10, 1, 10, Some(sid), None); // TSX 0:10 → Vue 1:10
        let consumer_mapper = ProviderPositionMapper::source_map(
            PositionMapper::from_json(&builder.into_sourcemap().to_json_string()).unwrap(),
        );
        let resolver = |ide_path: &str| -> Option<ExternalIdeContext> {
            if is_carrier_ide_path(ide_path) {
                Some(ExternalIdeContext {
                    tsx_line_index: consumer_tsx_li.clone(),
                    mapper: consumer_mapper.clone(),
                    carrier_line_index: consumer_carrier_li.clone(),
                })
            } else {
                None
            }
        };

        let result = merge_definitions(
            None,
            type_defs,
            "",
            &tsx_li,
            &mapper,
            &carrier_li,
            Some(&resolver),
            &test_doc_uri(),
            &carrier_exists,
            PositionEncodingKind::UTF16,
            &read_source,
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

        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        let (items, _) = merge_completions(
            vec![],
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
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

        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

        let (items, _) = merge_completions(
            vec![],
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
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

    #[test]
    fn merge_enriches_verter_kind_from_type_provider() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        // Verter item has VARIABLE kind
        let verter = vec![CompletionItem {
            label: "inc".to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            ..Default::default()
        }];
        // Type provider has FUNCTION kind for the same label
        let type_result = CompletionResult {
            items: vec![Completion {
                label: "inc".to_string(),
                kind: Some(CompletionKind::Function),
                detail: None,
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            }],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
        assert_eq!(result.len(), 1, "duplicate should be deduped");
        assert_eq!(
            result[0].kind,
            Some(CompletionItemKind::FUNCTION),
            "verter item should be enriched with type provider's FUNCTION kind"
        );
    }

    #[test]
    fn merge_does_not_enrich_with_text_kind() {
        let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
        // Verter item has VARIABLE kind
        let verter = vec![CompletionItem {
            label: "msg".to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            ..Default::default()
        }];
        // Type provider has Text kind (fallback) for the same label
        let type_result = CompletionResult {
            items: vec![Completion {
                label: "msg".to_string(),
                kind: Some(CompletionKind::Text),
                detail: None,
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            }],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(
            verter,
            type_result,
            &mapper,
            &tsx_li,
            &carrier_li,
            None,
            false,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].kind,
            Some(CompletionItemKind::VARIABLE),
            "Text kind from type provider should NOT override verter's VARIABLE kind"
        );
    }
}
