//! Pure block-head parsers for Svelte template blocks.
//!
//! These free helpers split a `{#each …}` / `{#await …}` / `{#snippet …}` head
//! into its component span pieces (list/item/index/key, promise/then/catch,
//! name/params) and provide the top-level (string/bracket/brace/paren-aware)
//! scanning primitives the splits rely on. They take spans + raw head text and
//! return spans into the ORIGINAL source — no parser state, no `self`.

use verter_span::Span;

use super::tokenizer::nonempty_span;

/// Split an `{#each ...}` head into `(list_expr, item, index, key)`. The
/// `as`/item is optional (the `{#each {length:n}}` no-item form).
pub(super) fn parse_each_head(
    head_rest_start: usize,
    head_rest: &str,
) -> (Option<Span>, Option<Span>, Option<Span>, Option<Span>) {
    // Find the top-level ` as ` separator (string/brace/bracket/paren-aware).
    let as_idx = find_top_level_keyword(head_rest, "as");
    let (list_part, binding_part) = match as_idx {
        Some(idx) => (&head_rest[..idx], Some(&head_rest[idx + 2..])),
        None => (head_rest, None),
    };
    let list_expr = nonempty_span(head_rest_start, list_part);
    let Some(binding_part) = binding_part else {
        return (list_expr, None, None, None);
    };
    let binding_offset = head_rest_start + as_idx.unwrap() + 2;
    // `pattern, index (key)` — the `(key)` is a trailing parenthesised group.
    let (before_key, key) = split_trailing_paren(binding_offset, binding_part);
    // split `pattern , index` at the LAST top-level comma
    let (item_text, index_text, index_offset) = split_last_top_level_comma(before_key);
    let item = nonempty_span(binding_offset, item_text);
    let index = index_text.and_then(|t| nonempty_span(binding_offset + index_offset, t));
    (list_expr, item, index, key)
}

/// Split an `{#await ...}` head into `(promise, inline_then, inline_catch)`.
pub(super) fn parse_await_head(
    head_rest_start: usize,
    head_rest: &str,
) -> (Option<Span>, Option<Span>, Option<Span>) {
    if let Some(idx) = find_top_level_keyword(head_rest, "then") {
        let promise = nonempty_span(head_rest_start, &head_rest[..idx]);
        let binding = nonempty_span(head_rest_start + idx + 4, &head_rest[idx + 4..]);
        return (promise, binding, None);
    }
    if let Some(idx) = find_top_level_keyword(head_rest, "catch") {
        let promise = nonempty_span(head_rest_start, &head_rest[..idx]);
        let binding = nonempty_span(head_rest_start + idx + 5, &head_rest[idx + 5..]);
        return (promise, None, binding);
    }
    (nonempty_span(head_rest_start, head_rest), None, None)
}

/// Split a `{#snippet name(params)}` head into `(name_span, name_text, params)`.
pub(super) fn parse_snippet_head(
    head_rest_start: usize,
    head_rest: &str,
) -> (Span, String, Option<Span>) {
    let lead = head_rest.len() - head_rest.trim_start().len();
    let body = head_rest.trim_start();
    let name_end = body.find('(').unwrap_or(body.len());
    let name_text = body[..name_end].trim_end().to_string();
    let name_start = head_rest_start + lead;
    let name_span = Span::new(name_start as u32, (name_start + name_text.len()) as u32);
    let params = if name_end < body.len() {
        // params between the matching parens
        let paren_open = head_rest_start + lead + name_end;
        // find matching ')' within the head (heads are brace-bounded already)
        let inner_start = paren_open + 1;
        let close = find_matching_paren(head_rest, lead + name_end + 1);
        let params_end = head_rest_start + close;
        Some(Span::new(inner_start as u32, params_end as u32))
    } else {
        None
    };
    (name_span, name_text, params)
}

/// Find the offset of a standalone keyword (surrounded by whitespace) at the
/// top level of `s` (not inside strings/brackets/braces/parens).
pub(super) fn find_top_level_keyword(s: &str, keyword: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let kw = keyword.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' | b'`' => quote = Some(b),
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ => {
                    if depth == 0
                        && i + kw.len() <= bytes.len()
                        && &bytes[i..i + kw.len()] == kw
                        && (i == 0 || bytes[i - 1].is_ascii_whitespace())
                        && bytes
                            .get(i + kw.len())
                            .is_none_or(|c| c.is_ascii_whitespace())
                    {
                        return Some(i);
                    }
                }
            },
        }
        i += 1;
    }
    None
}

/// Split off a trailing top-level `(key)` group. Returns `(before, key_span)`.
fn split_trailing_paren(offset: usize, s: &str) -> (&str, Option<Span>) {
    let trimmed = s.trim_end();
    if !trimmed.ends_with(')') {
        return (s, None);
    }
    // find the matching '(' for the trailing ')'
    let bytes = trimmed.as_bytes();
    let mut depth = 0i32;
    let mut open = None;
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    open = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    match open {
        Some(open_idx) => {
            let key_inner_start = offset + open_idx + 1;
            let key_inner_end = offset + trimmed.len() - 1;
            let key = if key_inner_end > key_inner_start {
                Some(Span::new(key_inner_start as u32, key_inner_end as u32))
            } else {
                None
            };
            (&s[..open_idx], key)
        }
        None => (s, None),
    }
}

/// Split at the LAST top-level comma. Returns `(before, after, after_offset)`.
fn split_last_top_level_comma(s: &str) -> (&str, Option<&str>, usize) {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut last = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' | b'`' => quote = Some(b),
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => last = Some(i),
                _ => {}
            },
        }
        i += 1;
    }
    match last {
        Some(idx) => (&s[..idx], Some(&s[idx + 1..]), idx + 1),
        None => (s, None, 0),
    }
}

/// Find the matching `)` for a `(` whose inner content begins at `inner_start`
/// within `s`. Returns the index of the matching `)` (or `s.len()`).
fn find_matching_paren(s: &str, inner_start: usize) -> usize {
    let bytes = s.as_bytes();
    let mut depth = 1i32;
    let mut i = inner_start;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' | b'`' => quote = Some(b),
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return i;
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    s.len()
}
