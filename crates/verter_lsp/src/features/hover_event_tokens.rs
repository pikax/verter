//! Source-owned hovers for Vue `v-on` / `@event` syntax tokens, plus the `@event`
//! name canonicalization shared with the merge layer and completion.
//!
//! The IDE codegen lowers `@click` to a generated `onClick` JSX prop, deletes modifier
//! syntax, and deletes no-value directives entirely, so a TypeProvider hover can only
//! ever describe the generated/absent token. These hovers reconstruct the label and
//! range from the source `TemplateDirective` spans instead, keeping the highlight on what
//! the user actually wrote. The event-name hover carries TYPED provenance
//! ([`HoverSourceToken::EventDirective`]) so the merge layer rewrites a paired `onClick`
//! TypeProvider hover back to `@click` — never a blind `on*` name match, never a reparse
//! of the rendered hover markdown.
//!
//! This module lives apart from `hover.rs` so each stays within the production
//! line-count budget; the canonicalization helpers ([`vue_event_attr_label`],
//! [`capitalize_first`], [`camelize_event_name`], [`hyphenate_event_name`]) live
//! here because they are the event-name primitives the directive hovers are built
//! from, and are re-exported for the handler-signature summarizers in `hover.rs`.

use tower_lsp_server::ls_types::*;
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::features::event_modifiers::{modifier_description, modifier_description_for_event};

use super::hover::{span_to_range, HoverSourceToken, VerterHoverResult};

/// Source-owned hover for `v-on` / `@event` syntax tokens — both the event-name token
/// (`@click`, `v-on:click`) and individual modifier tokens (`.stop`).
///
/// The IDE codegen lowers `@click` to a generated `onClick` JSX prop and deletes modifier
/// syntax (and deletes no-value directives entirely), so a TypeProvider hover can only
/// ever describe the generated token. We rebuild the hover from the source
/// `TemplateDirective` spans, keeping the label and range on what the user actually wrote.
/// The event-name hover carries TYPED provenance ([`HoverSourceToken::EventDirective`]) so
/// the merge layer rewrites a paired `onClick` TypeProvider hover to `@click`
/// (`merge::replace_primary_label_with_vue_attr`) — this is the *only* trigger for that
/// rewrite, never a blind `on*` name match and never a reparse of the rendered hover text.
pub(super) fn event_directive_hover(
    offset: u32,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<VerterHoverResult> {
    let template = analysis.template.as_deref()?;
    for el in &template.elements {
        for dir in &el.directives {
            if dir.name != "on" {
                continue;
            }

            // Modifier token (`.stop`, `.prevent`, …) — first-class even when the
            // directive has no value (`<div @touchmove.stop />`). The compiler's
            // `modifier_spans` slice the NAME only (`stop`); the hover range expands
            // to include the leading `.` (so the highlighted token is `.stop`), while
            // the description LOOKUP still uses the name-only slice.
            for mod_span in &dir.modifier_spans {
                let range_start = if mod_span.start > 0
                    && source.as_bytes().get(mod_span.start as usize - 1) == Some(&b'.')
                {
                    mod_span.start - 1
                } else {
                    mod_span.start
                };
                if offset >= range_start && offset < mod_span.end {
                    let modifier = source.get(mod_span.start as usize..mod_span.end as usize)?;
                    return Some(event_modifier_hover(
                        modifier,
                        dir.argument.as_deref(),
                        range_start,
                        mod_span.end,
                        line_index,
                    ));
                }
            }

            // Event-name token (`@click` / `v-on:click`) — bounded by the directive
            // start through the end of the argument so it never covers the handler
            // value (which the TypeProvider types) or trailing modifiers.
            if let Some(arg_span) = dir.arg_span.as_ref() {
                let token_start = dir.span.start;
                let token_end = arg_span.end;
                if offset >= token_start && offset < token_end {
                    let token = source.get(token_start as usize..token_end as usize)?;
                    let event = source.get(arg_span.start as usize..arg_span.end as usize)?;
                    return Some(native_event_hover(
                        token,
                        event,
                        token_start,
                        token_end,
                        line_index,
                    ));
                }
            }
        }
    }
    None
}

/// Source-owned hover for the `v-model` directive NAME and its static/dynamic ARG.
///
/// The IDE codegen lowers `v-model:show="x"` on a component to a generated `show=`
/// prop (+ an `onUpdate:show` handler) and overwrites the whole `v-model:show="x"`
/// span, so the source `v-model` / `:show` tokens have no TypeProvider description
/// of their own. (The mapped prop-name codegen lets TSGO describe the bound prop
/// TYPE; this hover supplies the Vue SOURCE context for the directive name + arg.)
/// We rebuild the label and range from the source `TemplateDirective` spans.
///
/// No merge-layer typed provenance is attached (`source_token: None`): TSGO shows
/// the real prop type label, and this Verter hover gives the `v-model:show` source
/// context. The hover bounds run from the directive start through the end of the
/// arg (or the directive name when there is no arg), so it never covers the bound
/// VALUE expression (which the TypeProvider types).
pub(super) fn v_model_hover(
    offset: u32,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<VerterHoverResult> {
    let template = analysis.template.as_deref()?;
    for el in &template.elements {
        for dir in &el.directives {
            if dir.name != "model" {
                continue;
            }
            // Token bounds: from the directive start through the end of the arg (so
            // both the `v-model` name AND the `:show` arg are covered), or just the
            // directive name when `v-model` has no arg.
            let token_start = dir.span.start;
            let token_end = dir.arg_span.as_ref().map_or(dir.name_end, |a| a.end);
            if offset < token_start || offset >= token_end {
                continue;
            }
            let token = source.get(token_start as usize..token_end as usize)?;
            let value = match dir.argument.as_deref() {
                Some(arg) if !arg.is_empty() => {
                    format!("`{token}`\n\nTwo-way binding to the `{arg}` model prop.")
                }
                _ => format!("`{token}`\n\nTwo-way binding (`v-model`)."),
            };
            return Some(VerterHoverResult {
                hover: Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value,
                    }),
                    range: span_to_range(line_index, token_start, token_end),
                },
                vue_kind_label: None,
                // No merge-layer rewrite for v-model — TSGO shows the prop type
                // label; this hover supplies the Vue source context only.
                source_token: None,
            });
        }
    }
    None
}

fn native_event_hover(
    token: &str,
    event: &str,
    start: u32,
    end: u32,
    line_index: &LineIndex,
) -> VerterHoverResult {
    // The display text shows the source token for context; it is NOT parsed by the
    // merge layer. The label-rewrite decision rides the typed `source_token` below.
    let value = format!("`{token}`\n\nListens for the `{event}` event.");
    VerterHoverResult {
        hover: Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: span_to_range(line_index, start, end),
        },
        vue_kind_label: None,
        // Canonical `@event` label drives the merge-layer rewrite of a paired
        // `onClick` TypeProvider hover. `v-on:click` and `@click` both canonicalize
        // to `@click` here.
        source_token: Some(HoverSourceToken::EventDirective {
            vue_attr: vue_event_attr_label(event),
        }),
    }
}

fn event_modifier_hover(
    modifier: &str,
    event_name: Option<&str>,
    start: u32,
    end: u32,
    line_index: &LineIndex,
) -> VerterHoverResult {
    let mut value = format!("`.{modifier}`");
    // Event-aware description: `@click.left` is the LEFT MOUSE BUTTON, while
    // `@keydown.left` is Arrow Left. With the directive argument (the event name)
    // we disambiguate; without it we fall back to the context-free lookup.
    let desc = match event_name {
        Some(event) => modifier_description_for_event(event, modifier),
        None => modifier_description(modifier),
    };
    if let Some(desc) = desc {
        value.push_str(&format!("\n\nEvent modifier — {desc}"));
    } else {
        value.push_str("\n\nEvent modifier");
    }
    VerterHoverResult {
        hover: Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: span_to_range(line_index, start, end),
        },
        vue_kind_label: None,
        source_token: None,
    }
}

/// Capitalize the first character of `text` (`"click"` → `"Click"`).
pub(super) fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Camelize a kebab-case event name (`"my-event"` → `"myEvent"`).
pub(super) fn camelize_event_name(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut capitalize_next = false;
    for ch in text.chars() {
        if ch == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Hyphenate a camelCase event name (`"myEvent"` → `"my-event"`).
pub(super) fn hyphenate_event_name(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 4);
    for (idx, ch) in text.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

/// Canonical Vue `@event` attribute label for an event name (`"updateModelValue"` /
/// `"update:modelValue"` → `"@update:model-value"`). Drives the merge-layer rewrite of a
/// paired `onClick` TypeProvider hover back to its source `@click` form.
pub(super) fn vue_event_attr_label(event_name: &str) -> String {
    let mut parts = event_name.splitn(2, ':');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    match second {
        Some(second) => format!(
            "@{}:{}",
            hyphenate_event_name(first),
            hyphenate_event_name(second)
        ),
        None => format!("@{}", hyphenate_event_name(first)),
    }
}
