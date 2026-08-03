//! Hover merge: combine verter hover with TypeProvider hover, rendering the
//! provider's STRUCTURED quick-info fields (signature / kind / documentation)
//! into markdown at this boundary — the only place markdown is produced from
//! them — plus Vue-specific display rewrites.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::features::hover::HoverSourceToken;
use crate::type_provider::protocol::HoverInfo;

const SVELTE_PUBLIC_COMPONENT_DISPLAY_NAME: &str = "__VerterPublicComponent";

/// Replace the implementation-only name used by the Svelte public facade with
/// the authored identifier under the cursor.
///
/// TypeScript normally presents the local import alias for a default import.
/// When a provider follows Verter's declaration carrier to its default-export
/// target, however, QuickInfo can expose the carrier's private facade const
/// instead. This is a display-only normalization at the LSP boundary, applied
/// to the BRANDED display signature (the value this boundary renders from):
/// provider identity and navigation continue to use the unmodified typed
/// surface.
pub(crate) fn rewrite_svelte_public_component_label(
    type_hover: &mut HoverInfo,
    authored_identifier: Option<&str>,
) {
    let Some(authored_identifier) = authored_identifier else {
        return;
    };
    type_hover.display_signature = type_hover.display_signature.as_ref().map(|signature| {
        signature.with_display_rewrite(|display| {
            display.replace(SVELTE_PUBLIC_COMPONENT_DISPLAY_NAME, authored_identifier)
        })
    });
}

/// Merge verter hover with TypeProvider hover.
///
/// Strategy:
/// - If TypeProvider supplies structured quick-info, render its type block
///   here (from `display_signature` + `kind` + `documentation` — never by
///   re-parsing the rendered `contents` blob) and prepend it to verter's hover
///   content.
/// - If only verter provides hover, use it as-is.
/// - If only TypeProvider provides hover, use its rendered block, with the
///   provider range mapped back to carrier coordinates (fail closed to no
///   range when the mapping cannot be made exact).
/// - A provider hover WITHOUT a structured signature contributes nothing
///   (fail closed) — there is no scrape-the-blob fallback.
pub fn merge_hover(
    verter_hover: Option<Hover>,
    type_hover: Option<HoverInfo>,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
    carrier_line_index: &LineIndex,
    vue_kind_label: Option<&str>,
    source_token: Option<&HoverSourceToken>,
) -> Option<Hover> {
    let rendered_type_block = type_hover
        .as_ref()
        .and_then(|info| render_type_block(info, vue_kind_label, source_token));
    match (verter_hover, rendered_type_block) {
        (Some(verter), Some(type_block)) => {
            // The provider supplies the richer type signature — strip verter's
            // leading code block to avoid duplicate fenced blocks in the
            // merged hover.
            let verter_text = extract_hover_text(&verter);
            let context = strip_leading_code_block(&verter_text);
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
        (None, Some(type_block)) => Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: type_block,
            }),
            range: type_hover.as_ref().and_then(|info| {
                provider_range_to_carrier_range(info, tsx_line_index, mapper, carrier_line_index)
            }),
        }),
        (None, None) => None,
    }
}

/// Render the provider's structured quick-info fields into the fenced type
/// block — the boundary renderer.
///
/// Composition is `(vue_kind_label ?? kind) + signature + documentation`,
/// idempotently: a display string already carrying its exact `({kind}) `
/// prefix is never double-prefixed (the same invariant the producer's
/// `format_quickinfo_hover` encodes), and a Vue label REPLACES an existing
/// `(word) ` display prefix rather than stacking onto it. Returns `None` when
/// the hover carries no structured signature — the rendered `contents` blob is
/// NEVER re-parsed as a fallback.
fn render_type_block(
    info: &HoverInfo,
    vue_kind_label: Option<&str>,
    source_token: Option<&HoverSourceToken>,
) -> Option<String> {
    // `(kind) display` through the SAME shared boundary formatter the
    // producers render `contents` with.
    let mut line = info.kind_labeled_signature()?;
    if let Some(label) = vue_kind_label {
        line = apply_vue_kind_label_override(&line, label);
    }
    // Rewrite the primary label to Vue source syntax ONLY when the verter
    // hover carries TYPED event-directive provenance — never by reparsing
    // rendered hover text and never by `on*` name-suffix sniffing.
    if let Some(HoverSourceToken::EventDirective { vue_attr }) = source_token {
        line = replace_primary_label_with_vue_attr(&line, vue_attr);
    }
    let block = match info
        .documentation
        .as_deref()
        .map(str::trim)
        .filter(|docs| !docs.is_empty())
    {
        Some(docs) => format!("```typescript\n{line}\n```\n\n{docs}"),
        None => format!("```typescript\n{line}\n```"),
    };
    Some(strip_synthetic_prefix(&block))
}

/// Map the provider hover's generated-file byte range back to a carrier
/// `Range`, failing closed to `None` when the snapshot carries no range or the
/// mapping cannot be made exact.
fn provider_range_to_carrier_range(
    info: &HoverInfo,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Option<Range> {
    let (start, end) = (info.range_start?, info.range_end?);
    super::position::tsx_range_to_carrier_range(
        start,
        end,
        tsx_line_index,
        mapper,
        carrier_line_index,
    )
}

/// Display-domain normalization: Verter's reserved synthetic-identifier prefix
/// never reaches user-facing hover text — a provider hover over a
/// GlobalComponents fallback const renders `GlobalComponentType<"Name">`, not
/// `___VERTER___GlobalComponentType<"Name">`. Pure display rewrite on the
/// provider's rendered type block (the same display-only class as
/// [`apply_vue_kind_label_override`]); it never feeds a semantic decision. The prefix is
/// Verter's reserved namespace (`super::completion::VERTER_INTERNAL_PREFIX`),
/// so no user identifier can legitimately contain it.
fn strip_synthetic_prefix(content: &str) -> String {
    content
        .replace(super::completion::VERTER_INTERNAL_PREFIX, "")
        // Svelte snippet declarations are branded internally so TypeScript
        // preserves their callable tuple contract. The authored display type
        // is `Snippet`; the helper identifier is projection-only.
        .replace("__VerterSnippet", "Snippet")
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
/// the remainder. Operates on VERTER'S OWN rendered hover (never the
/// provider's): it prevents duplicate code fences when merging provider +
/// verter hover.
pub(crate) fn strip_leading_code_block(text: &str) -> &str {
    if let Some(rest) = text.strip_prefix("```") {
        if let Some(end) = rest.find("\n```") {
            let after = &rest[end + 4..];
            return after.trim_start_matches('\n');
        }
    }
    text
}

/// Replace (or apply) the `({kind})` display prefix on a SIGNATURE LINE with a
/// Vue-specific label.
///
/// E.g. `(const) const count: Ref<number>` with vue_label `"ref"` becomes
/// `(ref) const count: Ref<number>`; a line with no `(word) ` prefix gains
/// `(ref) `. Display-domain rewrite over the single-line signature — a label
/// override REPLACES an existing display prefix rather than stacking onto it,
/// which also keeps the composition idempotent when the display string already
/// carries TypeScript's own `(property)`-style prefix.
pub(crate) fn apply_vue_kind_label_override(line: &str, vue_label: &str) -> String {
    let target_prefix = format!("({vue_label}) ");
    if line.starts_with(&target_prefix) {
        return line.to_string();
    }
    if line.starts_with('(') {
        if let Some(paren_end) = line.find(") ") {
            return format!("{target_prefix}{}", &line[paren_end + 2..]);
        }
    }
    format!("{target_prefix}{line}")
}

/// Rewrite the primary label of a single-line SIGNATURE to the authored Vue
/// attribute (e.g. `onCustom` → `@custom`), preserving an optional `(kind) `
/// display prefix. A name-boundary find on a signature is inherent to a
/// display rewrite and stays display-domain; the trigger is the TYPED
/// `HoverSourceToken::EventDirective` provenance, never text sniffing.
fn replace_primary_label_with_vue_attr(line: &str, vue_attr: &str) -> String {
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
        return line.to_string();
    }

    format!("{}{}{}", &line[..prefix_end], vue_attr, &rest[name_end..])
}
