use crate::{
    code_transform::CodeTransform,
    syntax::{
        plugin::SyntaxPluginContext,
        plugins::code_gen::{
            template::shared::helper::escape_js_string_in_place, types::TemplateImportDependencies,
        },
        types::Text,
    },
};

use super::{ChildInfo, ChildKind, StateStack};

/// Process a text node within a template element.
///
/// Applies Vue's default whitespace condensation (condense mode) before emitting:
/// 1. If the text is ALL whitespace AND contains a newline → **removed entirely**
/// 2. If the text is ALL whitespace without a newline → condensed to a single space
/// 3. Otherwise, consecutive whitespace (including newlines) → single space
///
/// Records a `ChildInfo` in the parent state and wraps the text content in quotes
/// to produce a JS string literal. Does NOT add separators — the close phase
/// retroactively inserts separators based on the full children list.
///
/// For source like `hello` between elements, this produces `"hello"`.
/// Text containing special characters (quotes, backslashes, etc.) is escaped
/// to produce valid JS string literals.
///
/// # Ordering Invariant
///
/// This function must NOT call `prepend_left(ev.start, ...)`. The opening `"`
/// quote is deferred to the parent's close phase via `ChildKind::Text.content_prefix()`.
/// Only `append_left(ev.end, "\"")` is safe here because it operates at `ev.end`,
/// a different position from the separator insertion point (`ev.start`).
pub(crate) fn handle_text<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    ev: &Text,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack<'alloc>,
    _imports: &mut TemplateImportDependencies,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
    pending_append_lefts: &mut Vec<(u32, &'alloc str)>,
) {
    let raw = &ctx.input[ev.start as usize..ev.end as usize];
    let raw_bytes = raw.as_bytes();

    // --- Whitespace condensation (Vue condense mode) ---
    let is_all_whitespace = raw_bytes
        .iter()
        .all(|&b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C));
    let has_newline = raw_bytes.iter().any(|&b| matches!(b, b'\n' | b'\r'));

    if is_all_whitespace {
        // All whitespace-only text is deferred to resolve_whitespace_candidates().
        // Vue's condense mode has context-dependent rules:
        // - First/last child → always remove
        // - Between comments → always remove
        // - Between element+comment → always remove
        // - Between elements with newline → remove
        // - Otherwise → condense to single space
        //
        // Since we don't know adjacent siblings yet, defer the decision.
        let kind = if has_newline {
            ChildKind::WhitespaceNewline
        } else {
            ChildKind::WhitespaceSpace
        };
        state.children.push(ChildInfo {
            start: ev.start,
            end: ev.end,
            kind,
            scope_prefix: "",
            is_named_slot: false,
        });
        return;
    }

    // Rule 3: Mixed content — condense consecutive whitespace runs to single space.
    // Check if any condensation is needed at all.
    let needs_condense = raw_bytes
        .windows(2)
        .any(|w| is_ws_byte(w[0]) && is_ws_byte(w[1]))
        || raw_bytes.iter().any(|&b| matches!(b, b'\n' | b'\r' | 0x0C));

    state.children.push(ChildInfo {
        start: ev.start,
        end: ev.end,
        kind: ChildKind::Text,
        scope_prefix: "",
        is_named_slot: false,
    });

    if needs_condense {
        // Build condensed text: replace consecutive whitespace with single space.
        // Also escape for JS string literal in the same pass.
        let condensed = condense_and_escape(raw);
        let s = code_transform.alloc_str(&condensed);
        pending_overwrites.push((ev.start, ev.end, s));
    } else {
        // No condensation needed — just escape in-place as before.
        escape_js_string_in_place(
            code_transform,
            ev.start,
            ev.end,
            ctx.input,
            pending_overwrites,
        );
    }

    // The opening quote is added by the close phase via ChildKind::content_prefix().
    pending_append_lefts.push((ev.end, "\""));
}

/// Check if a byte is a whitespace character for condensation purposes.
#[inline]
fn is_ws_byte(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

/// Condense consecutive whitespace to single space AND escape for JS string literal.
///
/// This combines both operations in a single pass to avoid an intermediate allocation.
/// Characters that need JS escaping (`"`, `\`, control chars, LS/PS) are escaped.
fn condense_and_escape(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_whitespace = false;

    for ch in text.chars() {
        if is_ws_byte(ch as u8) && ch.is_ascii() {
            if !in_whitespace {
                result.push(' ');
                in_whitespace = true;
            }
            // Skip additional whitespace characters
        } else {
            in_whitespace = false;
            match ch {
                '\\' => result.push_str("\\\\"),
                '"' => result.push_str("\\\""),
                '\0' => result.push_str("\\0"),
                '\u{2028}' => result.push_str("\\u2028"),
                '\u{2029}' => result.push_str("\\u2029"),
                c if c.is_ascii_control() => {
                    // Escape control characters as \xHH
                    result.push_str("\\x");
                    let byte = c as u8;
                    result.push(char::from(HEX_DIGITS[(byte >> 4) as usize]));
                    result.push(char::from(HEX_DIGITS[(byte & 0x0F) as usize]));
                }
                c => result.push(c),
            }
        }
    }

    result
}

const HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";

/// Resolve deferred whitespace-only children based on their neighbors.
///
/// Vue's condense mode rules for whitespace-only text (from `@vue/compiler-core`):
///
/// **Always remove** when:
/// - It's the first or last child (`!prev || !next`)
/// - Both adjacent are comments
/// - One adjacent is a comment and the other is an element
///
/// **Remove if has newline** (`WhitespaceNewline`) when:
/// - Both adjacent are elements
///
/// **Keep as single space** otherwise (e.g., between element and interpolation,
/// between two interpolations, or between elements without newline).
///
/// Must be called in the close phase (when all children are known) before
/// any child-processing logic that reads `state.children`.
pub(crate) fn resolve_whitespace_candidates(
    children: &mut Vec<ChildInfo>,
    pending_overwrites: &mut Vec<(u32, u32, &str)>,
    pending_append_lefts: &mut Vec<(u32, &str)>,
) {
    if children.is_empty() {
        return;
    }

    // Fast path: no whitespace candidates.
    if !children.iter().any(|c| {
        matches!(
            c.kind,
            ChildKind::WhitespaceNewline | ChildKind::WhitespaceSpace
        )
    }) {
        return;
    }

    let len = children.len();
    let mut to_remove: Vec<usize> = Vec::new();

    for i in 0..len {
        let ws_kind = children[i].kind;
        if !matches!(
            ws_kind,
            ChildKind::WhitespaceNewline | ChildKind::WhitespaceSpace
        ) {
            continue;
        }

        let has_newline = ws_kind == ChildKind::WhitespaceNewline;

        // Find the effective previous sibling (skip over other whitespace candidates).
        let prev_kind = (0..i)
            .rev()
            .find(|&j| {
                !matches!(
                    children[j].kind,
                    ChildKind::WhitespaceNewline | ChildKind::WhitespaceSpace
                )
            })
            .map(|j| children[j].kind);

        // Find the effective next sibling (skip over other whitespace candidates).
        let next_kind = ((i + 1)..len)
            .find(|&j| {
                !matches!(
                    children[j].kind,
                    ChildKind::WhitespaceNewline | ChildKind::WhitespaceSpace
                )
            })
            .map(|j| children[j].kind);

        // Vue's removal rules (from @vue/compiler-core condenseWhitespace):
        let should_remove = match (prev_kind, next_kind) {
            // First or last effective child → always remove.
            (None, _) | (_, None) => true,
            (Some(p), Some(n)) => {
                let p_is_el_or_comment = matches!(p, ChildKind::Element | ChildKind::Comment);
                let n_is_el_or_comment = matches!(n, ChildKind::Element | ChildKind::Comment);
                // Both comments → remove
                // Comment + element (either order) → remove
                // Both elements + has newline → remove
                (p == ChildKind::Comment && n == ChildKind::Comment)
                    || (p == ChildKind::Comment && n_is_el_or_comment)
                    || (p_is_el_or_comment && n == ChildKind::Comment)
                    || (p == ChildKind::Element && n == ChildKind::Element && has_newline)
            }
        };

        if should_remove {
            pending_overwrites.push((children[i].start, children[i].end, ""));
            to_remove.push(i);
        } else {
            // Keep as single space: overwrite source, add closing quote.
            pending_overwrites.push((children[i].start, children[i].end, " "));
            pending_append_lefts.push((children[i].end, "\""));
            children[i].kind = ChildKind::Text;
        }
    }

    // Remove marked children in reverse order to preserve indices.
    for &i in to_remove.iter().rev() {
        children.remove(i);
    }
}
