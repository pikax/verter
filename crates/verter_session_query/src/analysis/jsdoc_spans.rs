//! Byte spans of a JSDoc block and its tags, scanned from source text.

pub fn previous_identifier_token(source: &str, end: usize) -> Option<(usize, &str)> {
    if end == 0 || end > source.len() {
        return None;
    }

    let bytes = source.as_bytes();
    let mut token_end = end;
    while token_end > 0 && bytes[token_end - 1].is_ascii_whitespace() {
        token_end -= 1;
    }
    if token_end == 0 {
        return None;
    }

    let mut token_start = token_end;
    while token_start > 0 {
        let byte = bytes[token_start - 1];
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' {
            token_start -= 1;
            continue;
        }
        break;
    }

    (token_start != token_end).then_some((token_start, &source[token_start..token_end]))
}

pub fn is_jsdoc_prefix_token(token: &str) -> bool {
    matches!(
        token,
        "export"
            | "default"
            | "declare"
            | "abstract"
            | "async"
            | "public"
            | "private"
            | "protected"
            | "readonly"
            | "static"
            | "override"
            | "accessor"
            // Declaration-leading keywords: a JSDoc block precedes the WHOLE
            // declaration (`/** @type {T} */ const x = ...`), but the offset a
            // value / function extractor has is the binding NAME (`x`), so the
            // walk back from the name crosses the `const` / `let` / `var` /
            // `function` keyword before reaching the comment. These are real
            // declaration leaders, so attaching the leading JSDoc through them
            // is correct (the same as crossing `export`).
            | "const"
            | "let"
            | "var"
            | "function"
    )
}

/// Absolute `[start, end)` byte offsets of the JSDoc `/** ... */` block
/// immediately preceding `start` (after trimming trailing whitespace), or
/// `None`. The offset-returning sibling of
/// [`find_leading_jsdoc_immediately_before`].
fn jsdoc_block_offsets_immediately_before(source: &str, start: usize) -> Option<(usize, usize)> {
    if start == 0 || start > source.len() {
        return None;
    }
    let prefix = source.get(..start)?;
    let trimmed_end = prefix.trim_end().len();
    if !source.get(..trimmed_end)?.ends_with("*/") {
        return None;
    }
    let comment_start = source.get(..trimmed_end)?.rfind("/**")?;
    Some((comment_start, trimmed_end))
}

/// Absolute `[start, end)` byte offsets of the leading JSDoc block governing the
/// declaration whose name token starts at `target_start` — the offset-returning
/// sibling of [`find_leading_jsdoc_near_offset`] (same modifier / declaration-
/// keyword skip logic). `None` when no leading JSDoc block governs the offset.
pub fn find_leading_jsdoc_block_offsets(source: &str, target_start: u32) -> Option<(usize, usize)> {
    let start = target_start as usize;
    if let Some(offsets) = jsdoc_block_offsets_immediately_before(source, start) {
        return Some(offsets);
    }
    let mut cursor = start;
    for _ in 0..8 {
        let (token_start, token) = previous_identifier_token(source, cursor)?;
        if !is_jsdoc_prefix_token(token) {
            return None;
        }
        if let Some(offsets) = jsdoc_block_offsets_immediately_before(source, token_start) {
            return Some(offsets);
        }
        cursor = token_start;
    }
    None
}

/// Span (relative to the whole `source`) of a member's leading JSDoc
/// description text + each tag, computed by scanning the JSDoc block at
/// absolute offsets. The description span covers the comment body BEFORE the
/// first `@tag`; each [`JsdocTagSpanOffsets`] covers a tag's name + text.
///
/// All offsets are absolute byte offsets into `source` — the consumer slices
/// `source[span.start..span.end]`. This is the SPAN producer the typeinfo
/// surface JSDoc fields are built from; it carries no owned `String`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsdocBlockSpanOffsets {
    /// Span of the description text (`[start, end)`), or `None` for a
    /// tag-only / empty JSDoc block.
    pub description: Option<verter_span::Span>,
    /// Spans of each tag, in declaration order.
    pub tags: Vec<JsdocTagSpanOffsets>,
}

/// Offset spans of one JSDoc tag: the name (without `@`) and the optional text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsdocTagSpanOffsets {
    /// Span of the tag name (identifier after `@`).
    pub name: verter_span::Span,
    /// Span of the tag text (everything after the name), or `None` for a bare
    /// tag.
    pub text: Option<verter_span::Span>,
}

/// One stripped JSDoc line: the inner content's absolute `[start, end)` after
/// removing the leading whitespace + optional `*` decoration and trailing
/// whitespace. `None` for a line that is empty after stripping.
fn strip_jsdoc_line(source: &str, line_start: usize, line_end: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut s = line_start;
    // Leading whitespace.
    while s < line_end && bytes[s].is_ascii_whitespace() {
        s += 1;
    }
    // A single leading `*` decoration (JSDoc continuation lines).
    if s < line_end && bytes[s] == b'*' {
        s += 1;
        // Whitespace after the `*`.
        while s < line_end && bytes[s].is_ascii_whitespace() {
            s += 1;
        }
    }
    let mut e = line_end;
    while e > s && bytes[e - 1].is_ascii_whitespace() {
        e -= 1;
    }
    (s < e).then_some((s, e))
}

/// Compute the description + tag offset spans of the JSDoc block at absolute
/// `[block_start, block_end)` (the block INCLUDES its `/**` and `*/`
/// delimiters).
pub fn jsdoc_block_spans(
    source: &str,
    block_start: usize,
    block_end: usize,
) -> JsdocBlockSpanOffsets {
    // Inner content between `/**` and `*/`.
    let inner_start = (block_start + 3).min(block_end);
    let inner_end = block_end.saturating_sub(2).max(inner_start);

    let mut description_start: Option<usize> = None;
    let mut description_end: usize = inner_start;
    let mut tags: Vec<JsdocTagSpanOffsets> = Vec::new();
    let mut in_tags = false;

    let mut line_start = inner_start;
    while line_start < inner_end {
        let line_end = source[line_start..inner_end]
            .find('\n')
            .map(|rel| line_start + rel)
            .unwrap_or(inner_end);

        if let Some((s, e)) = strip_jsdoc_line(source, line_start, line_end.min(inner_end)) {
            if source.as_bytes()[s] == b'@' {
                in_tags = true;
                // Tag name: identifier after `@`.
                let name_start = s + 1;
                let mut name_end = name_start;
                let bytes = source.as_bytes();
                while name_end < e && is_identifier_continue(bytes[name_end]) {
                    name_end += 1;
                }
                // Tag text: everything after the name (trimmed).
                let mut text_start = name_end;
                while text_start < e && bytes[text_start].is_ascii_whitespace() {
                    text_start += 1;
                }
                let text =
                    (text_start < e).then(|| verter_span::Span::new(text_start as u32, e as u32));
                tags.push(JsdocTagSpanOffsets {
                    name: verter_span::Span::new(name_start as u32, name_end as u32),
                    text,
                });
            } else if in_tags {
                // A non-`@` line AFTER a tag has started is a CONTINUATION of the
                // most-recent tag's text (`@deprecated use X;\n more detail`).
                // Extend that tag's text span through this continuation line's
                // content so the full multi-line tag text is reconstructable from
                // the single span (a bare tag gains its first continuation line as
                // its text). Spans-only — no owned `String`.
                if let Some(last) = tags.last_mut() {
                    last.text = Some(match last.text {
                        Some(existing) => verter_span::Span::new(existing.start, e as u32),
                        None => verter_span::Span::new(s as u32, e as u32),
                    });
                }
            } else {
                // Description line (only while no tag has started; lines after the
                // first tag are handled by the continuation branch above).
                if description_start.is_none() {
                    description_start = Some(s);
                }
                description_end = e;
            }
        }

        line_start = line_end + 1;
    }

    JsdocBlockSpanOffsets {
        description: description_start
            .map(|s| verter_span::Span::new(s as u32, description_end as u32)),
        tags,
    }
}

/// The leading-JSDoc description + tag offset spans governing the declaration
/// whose name token starts at `target_start`, or `None` when no leading JSDoc
/// block governs the offset. The public SPAN entry the typeinfo surface JSDoc
/// fields are built from.
pub fn jsdoc_block_spans_at_offset(
    source: &str,
    target_start: u32,
) -> Option<JsdocBlockSpanOffsets> {
    let (block_start, block_end) = find_leading_jsdoc_block_offsets(source, target_start)?;
    let spans = jsdoc_block_spans(source, block_start, block_end);
    // A block with neither a description nor any tags carries no useful span.
    (spans.description.is_some() || !spans.tags.is_empty()).then_some(spans)
}

fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}
