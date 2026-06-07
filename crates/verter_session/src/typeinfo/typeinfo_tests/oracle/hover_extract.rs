//! Hover-extraction grammar — extracts the `<RHS>` of `type <probe_name> =
//! <RHS>` from tsgo's MARKDOWN hover (`docs/arch/u0-oracle-harness-design.md`
//! §Q2 "hover-extraction grammar", §4 `hover_extraction_grammar_is_versioned`).
//!
//! PURE + offline (tsgo-free): so BOTH the generator AND the offline
//! `raw_capture_matches_oracle_value` audit re-run it without tsgo. The output
//! RHS is handed to `admission::admit_hover_text` (which parses it via the SAME
//! OXC type parser and runs the backstop + positive allowlist + drop-counter).
//! Versioned by `PROBE_SYNTHESIS_VERSION`.
//!
//! The grammar (FIXED): the FIRST fenced ```` ```typescript ```` / ```` ```ts ````
//! block that contains the probe header `type <probe_name>` (prose / inline /
//! other-language blocks ignored); leading JSDoc/comment lines inside the block
//! are skipped; the RHS runs from after the alias `=` to the DEPTH-0 `;` — a `;`
//! nested inside `{}` / `[]` / `()` / `<>` or a string/template literal does NOT
//! terminate — or to end-of-block when tsgo omits the trailing `;` (as it does
//! for a type-alias hover). An UNCLOSED fence FAILS.
//!
//! Driver-shape note (`hover_driver_config_pinned`): the declared LSP client
//! `textDocument.hover.contentFormat` determines whether tsgo wraps the type in a
//! markdown ```` ```typescript ```` fence (markdown caps) or returns the BARE
//! `type <probe_name> = <RHS>` text (the empty/plaintext caps the adopted LSP
//! driver sends, Q3). Both are the SAME probe header; the
//! driver config is pinned into `probe_synthesis_version`, so a shape change is a
//! version change, never a silent mismatch. The grammar therefore accepts the
//! bare form via a WHOLE-TEXT fallback that fires ONLY when the hover carries NO
//! fenced code block of any language (so a markdown hover's prose/inline/other
//! blocks are still ignored — the fallback never overrides a fenced hover).

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HoverExtractError {
    /// No fenced ```typescript / ```ts block names the probe (the wrong-hover /
    /// no-probe-binding case — `probe_header_names_target`).
    NoProbeBlock,
    /// A fenced block opened but never closed (truncated / malformed hover).
    UnclosedFence,
}

/// Extract the RHS type-text of `type <probe_name> = <rhs>` from `hover_markdown`.
pub(crate) fn extract_probe_rhs(
    hover_markdown: &str,
    probe_name: &str,
) -> Result<String, HoverExtractError> {
    let mut saw_unclosed = false;
    let mut any_fence = false;
    let blocks = fenced_typescript_blocks(hover_markdown, &mut saw_unclosed, &mut any_fence);
    for block in blocks {
        if let Some(rhs) = block_probe_rhs(block, probe_name) {
            return Ok(rhs);
        }
    }
    if saw_unclosed {
        return Err(HoverExtractError::UnclosedFence);
    }
    // WHOLE-TEXT fallback for the BARE (unfenced) plaintext-caps hover shape: only
    // when there is NO fenced code block of any language, so a markdown hover's
    // prose / inline / other-language blocks never trigger it.
    if !any_fence {
        if let Some(rhs) = block_probe_rhs(hover_markdown, probe_name) {
            return Ok(rhs);
        }
    }
    Err(HoverExtractError::NoProbeBlock)
}

/// Yield the inner text of each fenced ```typescript / ```ts block, in order.
/// Sets `saw_unclosed` if a ts fence opened with no closing fence; sets
/// `any_fence` if ANY fenced code block (any language) was seen.
fn fenced_typescript_blocks<'a>(
    md: &'a str,
    saw_unclosed: &mut bool,
    any_fence: &mut bool,
) -> Vec<&'a str> {
    let mut blocks = Vec::new();
    let bytes = md.as_bytes();
    let mut i = 0;
    while i < md.len() {
        // A fence opener is ``` at the start of the string or right after a `\n`.
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if at_line_start && md[i..].starts_with("```") {
            *any_fence = true;
            let after_ticks = i + 3;
            // The info string runs to the end of the line.
            let line_end = md[after_ticks..]
                .find('\n')
                .map(|r| after_ticks + r)
                .unwrap_or(md.len());
            let info = md[after_ticks..line_end].trim();
            let is_ts = info.eq_ignore_ascii_case("typescript") || info.eq_ignore_ascii_case("ts");
            // The body starts after the info line's newline.
            let body_start = if line_end < md.len() {
                line_end + 1
            } else {
                md.len()
            };
            // Find the closing fence ``` at a line start.
            let mut j = body_start;
            let mut closing: Option<usize> = None;
            while j < md.len() {
                let line_at_start = j == body_start || bytes[j - 1] == b'\n';
                if line_at_start && md[j..].starts_with("```") {
                    closing = Some(j);
                    break;
                }
                j += 1;
            }
            match closing {
                Some(close) => {
                    if is_ts {
                        blocks.push(&md[body_start..close]);
                    }
                    // Advance past the closing fence line.
                    let close_line_end = md[close..]
                        .find('\n')
                        .map(|r| close + r + 1)
                        .unwrap_or(md.len());
                    i = close_line_end;
                    continue;
                }
                None => {
                    if is_ts {
                        *saw_unclosed = true;
                    }
                    break;
                }
            }
        }
        i += 1;
    }
    blocks
}

/// If `block` contains the probe header `type <probe_name>`, return its RHS.
fn block_probe_rhs(block: &str, probe_name: &str) -> Option<String> {
    // Find `type <probe_name>` as a token boundary (so `type Foo` does not match
    // a longer `FooBar`). The probe name is unique per ordinal by construction.
    let needle = format!("type {probe_name}");
    let header_at = find_decl(block, &needle)?;
    // After the name, skip to `=`.
    let after_name = header_at + needle.len();
    let eq_rel = block[after_name..].find('=')?;
    let rhs_start = after_name + eq_rel + 1;
    let rhs = capture_rhs(&block[rhs_start..]);
    Some(rhs.trim().to_string())
}

/// Find `needle` (`type <probe_name>`) where the char AFTER the name is a
/// non-identifier char (boundary), skipping matches inside a longer identifier.
fn find_decl(block: &str, needle: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = block[from..].find(needle) {
        let at = from + rel;
        let after = at + needle.len();
        let next = block[after..].chars().next();
        let boundary = matches!(next, None | Some(' ') | Some('=') | Some('\t') | Some('<'));
        // Also require the `type` keyword to be at a token boundary on its left.
        let prev = block[..at].chars().next_back();
        let left_ok = matches!(prev, None | Some('\n') | Some(' ') | Some('\t') | Some(';'));
        if boundary && left_ok {
            return Some(at);
        }
        from = after;
    }
    None
}

/// Capture from the start of `rest` (just after `=`) to the DEPTH-0 `;` or
/// end-of-input, honoring `{}` / `[]` / `()` / `<>` nesting and string / template
/// literals (a `;` inside any of these does not terminate).
fn capture_rhs(rest: &str) -> &str {
    let bytes = rest.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0;
    let mut string: Option<u8> = None; // active quote char, or None
    while i < rest.len() {
        let c = bytes[i];
        if let Some(q) = string {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                string = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => string = Some(c),
            b'{' | b'[' | b'(' | b'<' => depth += 1,
            b'}' | b']' | b')' | b'>' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            b';' if depth == 0 => return &rest[..i],
            _ => {}
        }
        i += 1;
    }
    rest
}

#[cfg(test)]
mod tests;
