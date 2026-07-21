//! Hover merge: combine verter hover with TypeProvider hover, fence handling,
//! and Vue-specific label rewrites.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::features::hover::HoverSourceToken;
use crate::type_provider::protocol::HoverInfo;

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
            let mut type_block = strip_synthetic_prefix(&wrap_type_block(&type_info.contents));
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
            let mut type_block = strip_synthetic_prefix(&wrap_type_block(&type_info.contents));
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

/// Display-domain normalization: Verter's reserved synthetic-identifier prefix
/// never reaches user-facing hover text — a provider hover over a
/// GlobalComponents fallback const renders `GlobalComponentType<"Name">`, not
/// `___VERTER___GlobalComponentType<"Name">`. Pure display rewrite on the
/// provider's rendered type block (the same display-only class as
/// [`replace_kind_prefix`]); it never feeds a semantic decision. The prefix is
/// Verter's reserved namespace (`super::completion::VERTER_INTERNAL_PREFIX`),
/// so no user identifier can legitimately contain it.
fn strip_synthetic_prefix(content: &str) -> String {
    content.replace(super::completion::VERTER_INTERNAL_PREFIX, "")
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

pub(crate) fn extract_hover_text(hover: &Hover) -> String {
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
pub(crate) fn strip_leading_code_block(text: &str) -> &str {
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
pub(crate) fn replace_kind_prefix(content: &str, vue_label: &str) -> String {
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
