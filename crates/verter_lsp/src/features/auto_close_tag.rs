use crate::documents::sfc_scanner::{classify_cursor, scan_sfc_blocks, SfcBlock, SfcCursorContext};

/// HTML void elements that must not be auto-closed.
const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// The framework carrier whose markup region the on-type auto-close applies to.
///
/// The markup region differs by carrier: a Vue SFC wraps its markup in a
/// `<template>` block, whereas a Svelte component places its markup at the SFC
/// root (outside `<script>` / `<style>`). The on-type handler resolves this from
/// the document's authoritative editor `language_id`; a non-carrier document
/// (plain `.ts` / `.js` / `.tsx`) has NO markup region and never reaches here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierKind {
    /// A Vue SFC — markup lives inside the `<template>` block.
    Vue,
    /// A Svelte component — markup lives at the SFC root.
    Svelte,
}

/// Gated auto-close entry point used by the on-type formatting handler.
///
/// Returns the closing-tag snippet ONLY when the typed `>` at `offset` sits in
/// the carrier's TEMPLATE/MARKUP region AND closes a real markup open tag.
/// It rejects (returns `None`) when the `>`:
///   * is in a `<script>` block (TS/JS — e.g. a `Box<Foo>` generic), or
///   * is in a `<style>` block (CSS — e.g. an `a > b` child combinator), or
///   * is at the Vue SFC root (Vue markup is template-only), or
///   * is inside a quoted attribute value (`title="a>b"`), or
///   * is inside a `{{ }}` interpolation expression (a TS-expression `>`).
///
/// Region classification is driven by the typed SFC block boundaries
/// ([`scan_sfc_blocks`] / [`classify_cursor`]) — the same structural authority
/// the rest of the LSP's SFC-aware features use — not by ad-hoc string sniffing
/// of the surrounding text. Once the region is confirmed markup, the actual
/// open-tag validity check is delegated to the carrier-aware
/// [`auto_close_tag_carrier`] (NOT the raw carrier-blind [`auto_close_tag`]).
pub fn auto_close_tag_in_carrier(
    source: &str,
    offset: usize,
    carrier: CarrierKind,
) -> Option<String> {
    if offset == 0 || offset > source.len() {
        return None;
    }
    let gt_pos = offset - 1;
    if source.as_bytes()[gt_pos] != b'>' {
        return None;
    }

    let blocks = scan_sfc_blocks(source);

    // The carrier's markup window `[win_start, win_end)`: the OUTER `<template>`
    // content span for Vue (nesting-balanced so a nested slot `<template>` does
    // not truncate it — F7), or the whole document for Svelte. `None` ⇒ the `>`
    // is not in a markup region (Vue `<script>`/`<style>`/root, or Svelte
    // `<script>`/`<style>`), so it must not auto-close.
    let (win_start, win_end) = markup_window(source, gt_pos as u32, &blocks, carrier)?;
    if gt_pos < win_start || gt_pos >= win_end {
        return None;
    }
    if gt_in_attribute_value(source, gt_pos, win_start, carrier) {
        return None;
    }
    if gt_in_mustache(source, gt_pos, win_start, win_end, carrier) {
        return None;
    }
    // Svelte uses single-brace `{ expr }` for bindings and logic blocks; a `>`
    // inside such an expression is a comparison / TS-generic, not a tag close.
    // Vue uses `{ … }` as literal text, so this check is Svelte-only.
    if carrier == CarrierKind::Svelte && gt_in_svelte_expression(source, gt_pos, win_start, win_end)
    {
        return None;
    }

    auto_close_tag_carrier(source, offset, win_start, carrier)
}

/// The markup window `[start, end)` for the carrier at `gt_pos`, or `None` when
/// `gt_pos` is NOT in a markup region.
///
/// This is the SINGLE authority for "is this a markup position, and what window
/// bounds the expression-region / close-tag scans". One window keeps the
/// quote-state anchor genuinely unquoted (the multi-line attribute fix) and the
/// region check consistent with the scans.
///
/// * Vue — the OUTER SFC `<template>` content span, computed by nesting-balanced
///   matching ([`outer_template_content_span`]) so a nested slot `<template>` does
///   NOT truncate the region (F7). `None` outside the template (script / style /
///   root gaps).
/// * Svelte — markup is the SFC root, so the window is the whole document EXCEPT
///   when `gt_pos` is inside a `<script>` / `<style>` block (where a `>` is a TS
///   generic / CSS combinator). The flat block scan is authoritative here, now
///   that case-variant and unclosed script/style blocks classify correctly.
fn markup_window(
    source: &str,
    gt_pos: u32,
    blocks: &[SfcBlock],
    carrier: CarrierKind,
) -> Option<(usize, usize)> {
    match carrier {
        CarrierKind::Vue => {
            let (s, e) = outer_template_content_span(source, blocks)?;
            if (gt_pos as usize) >= s && (gt_pos as usize) < e {
                Some((s, e))
            } else {
                None
            }
        }
        CarrierKind::Svelte => {
            let in_script_or_style = match classify_cursor(gt_pos, blocks) {
                SfcCursorContext::RootLevel => false,
                SfcCursorContext::BlockContent { block_index }
                | SfcCursorContext::OpeningTag { block_index }
                | SfcCursorContext::ClosingTag { block_index } => {
                    matches!(blocks[block_index].tag_name.as_str(), "script" | "style")
                }
            };
            if in_script_or_style {
                None
            } else {
                Some((0usize, source.len()))
            }
        }
    }
}

/// The OUTER SFC `<template>` content span `[content_start, content_end)`,
/// computed by NESTING-BALANCED tag matching so a nested slot
/// `<template #foo>…</template>` does not truncate it (the flat
/// [`scan_sfc_blocks`] stops the outer block at the first nested `</template>`,
/// which strands markup after the slot at root level — F7).
///
/// The OUTER `<template>` open tag is located via the STRUCTURAL SFC block scan
/// ([`scan_sfc_blocks`]), NOT a raw `<template` byte-search: the block scan
/// consumes `<script>` / `<style>` block content (and the comments / strings
/// within), so a literal `<template>` inside a script comment placed BEFORE the
/// real template can never be mistaken for the SFC template open (which would
/// leak the markup window into the script and auto-close a `Box<Foo>` generic).
/// From the outer template's content start the close is found by balancing
/// nested `<template …>` / `</template>` (skipping `<!-- -->` comments and
/// quoted attribute values) so a nested slot template does not truncate the
/// region (F7). The span runs from just past the outer open tag's `>` to the
/// `<` of its matching `</template>`. `None` when there is no top-level
/// `<template>` block (a self-closing `<template/>` has no content, and is not
/// returned as a block by the scanner).
///
// TODO(follow-up): the flat `scan_sfc_blocks` still mis-nests same-name tags;
// the auto-close gate balances the outer-template close locally here rather than
// changing the shared scanner (which many features consume). If another feature
// needs nesting-balanced SFC blocks, lift this into the scanner with full
// regression coverage instead of duplicating the balance walk.
fn outer_template_content_span(source: &str, blocks: &[SfcBlock]) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let len = bytes.len();

    // Locate the OUTER (first top-level) `<template>` block structurally. The
    // block scanner already skips `<script>` / `<style>` content and quoted
    // attribute values, so a literal `<template>` in a script comment is not it.
    // The block's `open_tag_end` is just past the outer open tag's `>` — the
    // content start. (A self-closing `<template/>` is never returned as a block,
    // so it correctly yields `None`.)
    let template = blocks.iter().find(|b| b.tag_name == "template")?;
    let content_start = template.open_tag_end as usize;
    let mut depth = 1i32;

    // Balance nested `<template …>` / `</template>` from content_start.
    let mut k = content_start;
    let mut in_quote: Option<u8> = None;
    while k < len {
        let b = bytes[k];
        if let Some(q) = in_quote {
            if b == q {
                in_quote = None;
            }
            k += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => {
                in_quote = Some(b);
                k += 1;
            }
            b'<' => {
                // Comment?
                if k + 4 <= len && &bytes[k..k + 4] == b"<!--" {
                    match source[k + 4..].find("-->") {
                        Some(off) => k = k + 4 + off + 3,
                        None => return None,
                    }
                    continue;
                }
                // Closing `</template>`?
                if is_template_close_at(bytes, k) {
                    depth -= 1;
                    if depth == 0 {
                        return Some((content_start, k));
                    }
                    k += "</template".len();
                } else if is_template_open_at(bytes, k) {
                    depth += 1;
                    k += "<template".len();
                } else {
                    k += 1;
                }
            }
            _ => k += 1,
        }
    }
    // Unterminated outer template: treat the rest of the document as its
    // content so markup typed before the (missing) close still classifies.
    Some((content_start, len))
}

/// Whether `bytes[at..]` begins a `<template` open tag (case-insensitive) with a
/// proper tag-name boundary (the next byte is whitespace, `>`, `/`, or EOF), so
/// `<templatefoo` does not match.
fn is_template_open_at(bytes: &[u8], at: usize) -> bool {
    const TAG: &[u8] = b"<template";
    if at + TAG.len() > bytes.len() {
        return false;
    }
    if !bytes[at..at + TAG.len()].eq_ignore_ascii_case(TAG) {
        return false;
    }
    match bytes.get(at + TAG.len()) {
        None => true,
        Some(&b) => b.is_ascii_whitespace() || b == b'>' || b == b'/',
    }
}

/// Whether `bytes[at..]` begins a `</template` closing tag (case-insensitive)
/// with a proper boundary (next byte whitespace or `>`).
fn is_template_close_at(bytes: &[u8], at: usize) -> bool {
    const TAG: &[u8] = b"</template";
    if at + TAG.len() > bytes.len() {
        return false;
    }
    if !bytes[at..at + TAG.len()].eq_ignore_ascii_case(TAG) {
        return false;
    }
    match bytes.get(at + TAG.len()) {
        None => true,
        Some(&b) => b.is_ascii_whitespace() || b == b'>',
    }
}

/// Whether the `>` at `gt_pos` sits inside a quoted attribute value (single or
/// double quotes) rather than being a tag-closing `>`.
///
/// Quote state is anchored to the CANDIDATE TAG's opening `<` ([`nearest_tag_lt`])
/// — the genuinely unquoted, brace-aware start of the tag whose `>` is being
/// typed — NOT to the whole markup window. This keeps the multi-line-attribute
/// fix intact (a value opened earlier in the SAME tag, e.g. `<div\n title="a>b">`
/// or a value spanning a newline `title="a\nb>c">`, is still seen as in-quote
/// because `nearest_tag_lt` walks back across newlines to the tag's `<`), while
/// eliminating the prose-apostrophe false positive: an unmatched apostrophe in
/// template TEXT (`Bob's`) BEFORE the candidate tag's `<` is no longer scanned,
/// so it cannot desync the quote state and suppress a later tag's auto-close.
///
/// Returns `false` when there is no candidate `<` before `gt_pos` in the window
/// (the `>` is not closing a tag — [`auto_close_tag_carrier`] rejects it anyway).
///
/// The candidate-tag lookup is carrier-aware: it skips the carrier's expression
/// spans (Vue `{{ }}` mustache, Svelte `{ }`) so a `<` or `"` inside an
/// interpolation is never recorded as the candidate tag — a lone Vue `{` stays
/// literal template text and must not hide a following tag's `<`.
fn gt_in_attribute_value(
    source: &str,
    gt_pos: usize,
    win_start: usize,
    carrier: CarrierKind,
) -> bool {
    let Some(lt) = nearest_tag_lt(source, gt_pos, win_start, carrier) else {
        return false;
    };
    let bytes = source.as_bytes();

    // Track quote state from the candidate tag's `<` up to gt_pos. If we are
    // inside an open quote when we reach gt_pos, the `>` is an attribute-value
    // char (`title="a>b"`), not the tag close.
    let mut in_quote: Option<u8> = None;
    let mut i = lt;
    while i < gt_pos {
        let b = bytes[i];
        match in_quote {
            Some(q) => {
                if b == q {
                    in_quote = None;
                }
            }
            None => {
                if b == b'"' || b == b'\'' {
                    in_quote = Some(b);
                }
            }
        }
        i += 1;
    }
    in_quote.is_some()
}

/// Whether the `>` at `gt_pos` sits inside a `{{ ... }}` interpolation within
/// the carrier's markup window (a TS-expression `>`, e.g. `{{ mk<Foo>() }}`).
///
/// Mustache delimiters are template SYNTAX structure (the same category as the
/// comment skip in [`scan_sfc_blocks`]), not a semantic heuristic. The scan is
/// bounded to the precomputed markup window `[win_start, win_end)`. Svelte uses
/// single-brace `{ … }` for expressions rather than `{{ }}`; this guard is the
/// Vue-mustache defense (the Svelte single-brace case is handled by
/// [`gt_in_svelte_expression`]) and is a no-op for Svelte windows with no `{{`.
fn gt_in_mustache(
    source: &str,
    gt_pos: usize,
    win_start: usize,
    win_end: usize,
    _carrier: CarrierKind,
) -> bool {
    if gt_pos < win_start || gt_pos >= win_end {
        return false;
    }

    // Determine whether gt_pos lies inside an OPEN `{{ ... }}` by scanning the
    // window for `{{` / `}}` pairs. A `>` is in a mustache iff the most recent
    // delimiter before it within the window is an opening `{{`.
    let bytes = source.as_bytes();
    let mut i = win_start;
    let mut open: Option<usize> = None;
    while i + 1 < win_end && i < gt_pos {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            open = Some(i);
            i += 2;
            continue;
        }
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            open = None;
            i += 2;
            continue;
        }
        i += 1;
    }
    open.is_some()
}

/// The position of the candidate tag's opening `<` for `gt_pos`: the nearest
/// preceding `<` within `[win_start, gt_pos)` that genuinely STARTS a tag —
/// outside a quoted attribute value AND outside the carrier's EXPRESSION spans
/// (Vue `{{ … }}` mustache, Svelte `{ … }`).
///
/// The scan is a structural tag/text state machine. A quote (`'` / `"`) is an
/// attribute-value delimiter ONLY while inside a tag (between `<` and its `>`);
/// in ordinary markup TEXT a `'` is a literal apostrophe (`Bob's`, `don't`) and
/// must NOT open a quote — otherwise an unmatched text apostrophe would swallow
/// every following `<` and strand the candidate-tag lookup on a too-early `<`.
///
/// The carrier's EXPRESSION spans are skipped WHOLESALE — neither a `<` nor a
/// `"`/`'` inside an expression span is ever treated as markup, so a literal
/// `<`/quote inside an interpolation (`{{ "<" }}`) can never be recorded as the
/// candidate tag's `<` (which would mis-anchor the attribute-value guard). The
/// span definition MIRRORS the gate's existing detectors and is carrier-specific:
/// * Vue — `{{ … }}` DOUBLE-brace mustache (mirrors [`gt_in_mustache`]): a `{{`
///   opens a span that is advanced to the matching `}}`, ignoring everything in
///   between. A LONE single `{` is LITERAL template text and stays inert — it
///   must not hide a following tag's `<` (which would desync the attr guard).
/// * Svelte — `{ … }` single-brace expression (mirrors the brace-depth tracking
///   in [`gt_in_svelte_expression`]): `{` raises the depth, `}` lowers it, and a
///   `<` / `"` / `'` is only markup at depth 0, so a comparison / TS generic
///   inside `{a < b}` (and a quoted brace inside it) is correctly ignored.
///
/// A `>` inside a quoted attribute value does not prematurely end the tag (so a
/// value opened on a previous line is still in-tag at a later `<`). Returns
/// `None` when no tag `<` exists before `gt_pos`.
fn nearest_tag_lt(
    source: &str,
    gt_pos: usize,
    win_start: usize,
    carrier: CarrierKind,
) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut in_quote: Option<u8> = None;
    let mut brace_depth: i32 = 0;
    let mut in_tag = false;
    let mut lt: Option<usize> = None;
    let mut i = win_start.min(gt_pos);
    while i < gt_pos {
        let b = bytes[i];
        if let Some(q) = in_quote {
            if b == q {
                in_quote = None;
            }
            i += 1;
            continue;
        }
        // Vue `{{ … }}` mustache is an expression span: a lone `{` is literal
        // text and inert; only `{{` (outside a tag) opens a span, which is
        // advanced to its matching `}}` so neither a `<` nor a `"`/`'` inside it
        // is ever recorded as markup. Svelte uses single-brace `{ … }` instead,
        // tracked by depth in the unified state machine below.
        if carrier == CarrierKind::Vue
            && !in_tag
            && b == b'{'
            && i + 1 < gt_pos
            && bytes[i + 1] == b'{'
        {
            let mut j = i + 2;
            while j + 1 < gt_pos && !(bytes[j] == b'}' && bytes[j + 1] == b'}') {
                j += 1;
            }
            i = if j + 1 < gt_pos && bytes[j] == b'}' && bytes[j + 1] == b'}' {
                j + 2
            } else {
                // Unterminated `{{` before gt_pos: the span runs through gt_pos,
                // so nothing after it can be the candidate tag.
                gt_pos
            };
            continue;
        }
        match b {
            // Quotes delimit attribute values only inside a tag or (Svelte) a
            // `{ … }` expression; a `'`/`"` in markup TEXT is literal. Inside a
            // Svelte expression a quote is a string delimiter (so a `}` in the
            // string does not close the expression — quoted-brace handling).
            b'"' | b'\'' if in_tag || brace_depth > 0 => in_quote = Some(b),
            // Svelte single-brace expression nesting; Vue's `{{ }}` is handled
            // above and a lone Vue `{`/`}` is literal text, so braces are inert
            // for Vue (brace_depth never leaves 0).
            b'{' if carrier == CarrierKind::Svelte => brace_depth += 1,
            b'}' if carrier == CarrierKind::Svelte => brace_depth = (brace_depth - 1).max(0),
            b'<' if brace_depth == 0 => {
                lt = Some(i);
                in_tag = true;
            }
            b'>' if brace_depth == 0 && in_tag => in_tag = false,
            _ => {}
        }
        i += 1;
    }
    lt
}

/// Whether the `>` at `gt_pos` sits inside a Svelte single-brace `{ … }`
/// expression (an attribute binding `disabled={a > b}` or a logic block
/// `{#if a > b}`), where the `>` is a comparison / TS generic, NOT a tag close.
///
/// Svelte (unlike Vue) uses a SINGLE brace for expressions, so any `>` at
/// brace-nesting depth > 0 is an expression char. Scanning starts at the
/// candidate tag's `<` ([`nearest_tag_lt`]) so within-tag quotes are correctly
/// treated as attribute delimiters and text apostrophes outside the tag never
/// desync quote state; from there it tracks `{`/`}` nesting (quote-aware, so a
/// `}` inside an expression string literal does not close the expression). A
/// `>` reached at depth > 0 is in an expression.
fn gt_in_svelte_expression(source: &str, gt_pos: usize, win_start: usize, win_end: usize) -> bool {
    if gt_pos < win_start || gt_pos >= win_end {
        return false;
    }
    // Svelte-only by construction: the carrier-aware scan skips `{ … }`
    // expression spans so the anchor is the real tag `<` (a `<` inside an
    // earlier `{ … }` expression is correctly ignored).
    let Some(anchor) = nearest_tag_lt(source, gt_pos, win_start, CarrierKind::Svelte) else {
        return false;
    };
    let bytes = source.as_bytes();
    let mut in_quote: Option<u8> = None;
    let mut brace_depth: i32 = 0;
    let mut i = anchor;
    while i < gt_pos {
        let b = bytes[i];
        match in_quote {
            Some(q) => {
                if b == q {
                    in_quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' => in_quote = Some(b),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = (brace_depth - 1).max(0),
                _ => {}
            },
        }
        i += 1;
    }
    brace_depth > 0
}

/// The single carrier-aware close emitter for the LSP on-type auto-close path.
///
/// It returns `Some("$0</name>")` only when the typed `>` at `offset` is the
/// actual closing delimiter of a real markup start tag, reasoning over the
/// carrier's markup window with the SAME state machine the gate guards use — it
/// does NOT fall back to the carrier-blind backward `<`-scan ([`auto_close_tag`]).
///
/// Steps:
///   1. Guard the offset / typed `>` / self-closing (`/>`, `?>`).
///   2. Locate the candidate start tag's `<` via [`nearest_tag_lt`] — the nearest
///      preceding REAL-markup `<` (not inside a quote, not inside the carrier's
///      expression span: Vue `{{ … }}` / Svelte `{ … }`, not after an unmatched
///      `{{`). A `<` that lives only inside an interpolation is therefore never
///      the candidate — closing the carrier-blind interpolation false-fire.
///   3. Verify `gt_pos` is the FIRST real (unquoted, expression-skipped) `>` after
///      that `<` ([`gt_closes_candidate_tag`]) — i.e. genuinely this tag's close,
///      not a later tag's and not a `>` inside one of the tag's quoted attribute
///      values. This forward scan is carrier-aware in the same way as step 2.
///   4. Reject closing `</` / comment `<!` / processing `<?`, extract the tag
///      name, reject empty / void / already-closed-immediately-after.
fn auto_close_tag_carrier(
    source: &str,
    offset: usize,
    win_start: usize,
    carrier: CarrierKind,
) -> Option<String> {
    if offset == 0 || offset > source.len() {
        return None;
    }
    let bytes = source.as_bytes();
    let gt_pos = offset - 1;
    if bytes[gt_pos] != b'>' {
        return None;
    }
    // Self-closing `/>` or `?>`.
    if gt_pos > 0 && (bytes[gt_pos - 1] == b'/' || bytes[gt_pos - 1] == b'?') {
        return None;
    }

    // The candidate tag's `<`: the nearest preceding real-markup `<` (carrier-
    // aware — skips quotes and the carrier's expression spans). A `<` inside a
    // Vue `{{ … }}` / Svelte `{ … }` span is never returned, so an interpolation-
    // interior `<` cannot anchor the close.
    let lt = nearest_tag_lt(source, gt_pos, win_start, carrier)?;

    // `gt_pos` must be the FIRST real `>` after `lt` (carrier-aware): an earlier
    // real `>` means gt_pos closes a different tag, and a `>` reached inside a
    // quote / expression span is not the tag close.
    if !gt_closes_candidate_tag(bytes, lt, gt_pos, carrier) {
        return None;
    }

    let after_lt = lt + 1;
    if after_lt >= gt_pos {
        return None;
    }
    let first_char = bytes[after_lt];
    // Closing `</`, comment `<!`, processing `<?`.
    if first_char == b'/' || first_char == b'!' || first_char == b'?' {
        return None;
    }

    // Tag name: from after `<` up to first whitespace, `/`, `>`, or `{`.
    let tag_content = &source[after_lt..gt_pos];
    let tag_name = tag_content
        .split(|c: char| c.is_whitespace() || c == '/' || c == '>' || c == '{')
        .next()
        .unwrap_or("")
        .trim();
    if tag_name.is_empty() {
        return None;
    }

    let tag_lower = tag_name.to_ascii_lowercase();
    if VOID_TAGS.contains(&tag_lower.as_str()) {
        return None;
    }

    // Already closed immediately after? HTML tag names are case-insensitive, so `<DIV>` already
    // followed by `</div>` is closed — compare the leading `</tag` ASCII-case-insensitively over the
    // byte slice (safe across char boundaries; `tag_name` may carry a non-ASCII component char).
    let remaining = source[offset..].trim_start();
    let expected_close = format!("</{tag_name}");
    let remaining_bytes = remaining.as_bytes();
    if remaining_bytes
        .get(..expected_close.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(expected_close.as_bytes()))
    {
        // The `</tag` prefix matches; require the NEXT byte to terminate the tag name — `>`, `/`,
        // ASCII whitespace, or end of input. An identifier byte means a DIFFERENT, longer tag
        // (`</diverse>` is not a close for `<div>`), which does NOT already-close this element.
        let terminated = match remaining_bytes.get(expected_close.len()) {
            None => true,
            Some(&b) => b == b'>' || b == b'/' || b.is_ascii_whitespace(),
        };
        if terminated {
            return None;
        }
    }

    Some(format!("$0</{tag_name}>"))
}

/// Whether `gt_pos` is the FIRST real (unquoted, expression-skipped) `>` that
/// follows the candidate tag's opening `<` at `lt` — i.e. `gt_pos` genuinely
/// closes THIS start tag rather than a later one, and is not a `>` sitting inside
/// one of the tag's quoted attribute values or inside an embedded expression span.
///
/// The forward scan mirrors [`nearest_tag_lt`]'s carrier semantics so the close
/// emitter and the gate guards agree on what is markup:
/// * Svelte — single-brace `{ … }` raises/lowers a depth; quotes are string
///   delimiters (a `}` inside a string does not close the expression); a `>` only
///   ends the tag at depth 0. So `<button disabled={a > b}>` is not aborted by the
///   inner `>` (F6).
/// * Vue — a lone `{` is literal text and inert; a `{{ … }}` mustache span is
///   skipped wholesale (advanced to its matching `}}`), so a `>` inside an
///   interpolation between `lt` and `gt_pos` is not mistaken for the tag close;
///   quotes delimit attribute values.
///
/// Returns `true` iff no real `>` precedes `gt_pos` after `lt`.
fn gt_closes_candidate_tag(bytes: &[u8], lt: usize, gt_pos: usize, carrier: CarrierKind) -> bool {
    let mut in_quote: Option<u8> = None;
    let mut brace_depth: i32 = 0;
    let mut i = lt + 1;
    while i < gt_pos {
        let b = bytes[i];
        if let Some(q) = in_quote {
            if b == q {
                in_quote = None;
            }
            i += 1;
            continue;
        }
        // Vue `{{ … }}` mustache is an expression span skipped wholesale; a lone
        // `{` is literal text. Only `{{` opens a span, advanced to its matching
        // `}}`, so neither a `>` nor a `"`/`'` inside it ends the tag.
        if carrier == CarrierKind::Vue && b == b'{' && i + 1 < gt_pos && bytes[i + 1] == b'{' {
            let mut j = i + 2;
            while j + 1 < gt_pos && !(bytes[j] == b'}' && bytes[j + 1] == b'}') {
                j += 1;
            }
            i = if j + 1 < gt_pos && bytes[j] == b'}' && bytes[j + 1] == b'}' {
                j + 2
            } else {
                // Unterminated `{{` before gt_pos: the span runs through gt_pos,
                // so no real `>` precedes it.
                gt_pos
            };
            continue;
        }
        match b {
            // A quote opens an attribute value (Vue/Svelte) or, inside a Svelte
            // `{ … }` expression, a string literal; either way a `>` inside it is
            // not the tag close.
            b'"' | b'\'' => in_quote = Some(b),
            // Svelte single-brace expression nesting; Vue's `{{ }}` is handled
            // above and a lone Vue `{` is literal, so braces are inert for Vue.
            b'{' if carrier == CarrierKind::Svelte => brace_depth += 1,
            b'}' if carrier == CarrierKind::Svelte => brace_depth = (brace_depth - 1).max(0),
            // A real (non-expression, unquoted) `>` before gt_pos means gt_pos is
            // not this tag's close.
            b'>' if brace_depth == 0 => return false,
            _ => {}
        }
        i += 1;
    }
    true
}

/// Raw HTML-only auto-close helper: a naive backward `<`-scan with no awareness
/// of carrier expression spans (Vue `{{ … }}`, Svelte `{ … }`).
///
/// Returns `$0</tagname>` right after the typed `>` at `offset`, or `None` if no
/// closing tag should be inserted. The carrier LSP path does NOT use this — it
/// routes through the carrier-aware [`auto_close_tag_carrier`], which skips
/// interpolation/expression spans so an interpolation-interior `<` cannot anchor a
/// close. This function is retained only as a raw HTML helper characterized by its
/// own unit tests.
pub fn auto_close_tag(source: &str, offset: usize) -> Option<String> {
    // `offset` points right after the typed `>`.
    // Walk backward to find the opening tag.
    if offset == 0 || offset > source.len() {
        return None;
    }

    let bytes = source.as_bytes();

    // The `>` itself is at offset - 1.
    let gt_pos = offset - 1;
    if bytes[gt_pos] != b'>' {
        return None;
    }

    // Skip if self-closing: `/>` or `?>`
    if gt_pos > 0 && (bytes[gt_pos - 1] == b'/' || bytes[gt_pos - 1] == b'?') {
        return None;
    }

    // Walk backward from gt_pos to find the matching `<`
    let mut pos = gt_pos;
    let mut found_lt = false;
    while pos > 0 {
        pos -= 1;
        if bytes[pos] == b'<' {
            found_lt = true;
            break;
        }
        // If we hit another `>`, we've gone past a different tag
        if bytes[pos] == b'>' {
            return None;
        }
    }

    if !found_lt {
        return None;
    }

    // pos points to `<`. Check it's not a closing tag, comment, or special tag.
    let after_lt = pos + 1;
    if after_lt >= gt_pos {
        return None;
    }
    let first_char = bytes[after_lt];

    // Skip closing tags `</`, comments `<!`, processing `<?`
    if first_char == b'/' || first_char == b'!' || first_char == b'?' {
        return None;
    }

    // Extract tag name: from after `<` up to first whitespace, `/`, or `>`
    let tag_content = &source[after_lt..gt_pos];
    let tag_name = tag_content
        .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or("")
        .trim();

    if tag_name.is_empty() {
        return None;
    }

    // Skip void elements
    let tag_lower = tag_name.to_ascii_lowercase();
    if VOID_TAGS.contains(&tag_lower.as_str()) {
        return None;
    }

    // Already closed immediately after? HTML tag names are case-insensitive, so `<DIV>` already
    // followed by `</div>` is closed — compare the leading `</tag` ASCII-case-insensitively over the
    // byte slice (safe across char boundaries; `tag_name` may carry a non-ASCII component char).
    let remaining = source[offset..].trim_start();
    let expected_close = format!("</{tag_name}");
    let remaining_bytes = remaining.as_bytes();
    if remaining_bytes
        .get(..expected_close.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(expected_close.as_bytes()))
    {
        // The `</tag` prefix matches; require the NEXT byte to terminate the tag name — `>`, `/`,
        // ASCII whitespace, or end of input. An identifier byte means a DIFFERENT, longer tag
        // (`</diverse>` is not a close for `<div>`), which does NOT already-close this element.
        let terminated = match remaining_bytes.get(expected_close.len()) {
            None => true,
            Some(&b) => b == b'>' || b == b'/' || b.is_ascii_whitespace(),
        };
        if terminated {
            return None;
        }
    }

    Some(format!("$0</{}>", tag_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_close_div() {
        let source = "<template><div></div></template>";
        // Cursor right after `<div>` at offset 15
        let result = auto_close_tag(source, 15);
        // Already has </div> immediately after
        assert!(
            result.is_none(),
            "should not close when </div> already exists"
        );
    }

    #[test]
    fn auto_close_div_no_existing() {
        let source = "<template><div>\n</template>";
        let result = auto_close_tag(source, 15);
        assert_eq!(result, Some("$0</div>".to_string()));
    }

    /// HTML tag names are case-insensitive: an opening `<DIV>` already followed by a
    /// differently-cased `</div>` is CLOSED, so the raw helper must NOT insert a second close. Fails
    /// if the already-closed guard compares the `</tag` prefix case-sensitively.
    #[test]
    fn auto_close_raw_uppercase_open_already_closed_lowercase_is_none() {
        // `<DIV>` ends at offset 5; `</div>` follows immediately (different case).
        let source = "<DIV></div>";
        let result = auto_close_tag(source, 5);
        assert!(
            result.is_none(),
            "an already-closed (case-insensitively) tag must NOT be re-closed, got {result:?}"
        );
    }

    /// A following `</diverse>` shares the `</div` prefix but is a DIFFERENT, longer tag — it does
    /// NOT already-close `<div>`, so the raw helper must still emit the close. Fails if the
    /// already-closed guard treats `</div` as a prefix without requiring a tag-name terminator.
    #[test]
    fn auto_close_raw_longer_tag_sharing_prefix_still_closes() {
        let source = "<div></diverse>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag(source, off),
            Some("$0</div>".to_string()),
            "`</diverse>` does not close `<div>`; the close must still be emitted",
        );
    }

    /// Positive control: `<DIV>` with NO following close still auto-closes, preserving the typed
    /// case in the inserted tag — proving the case-insensitive already-closed guard did not disable
    /// the normal insertion.
    #[test]
    fn auto_close_raw_uppercase_open_unclosed_inserts_same_case() {
        let source = "<DIV>";
        let result = auto_close_tag(source, 5);
        assert_eq!(result, Some("$0</DIV>".to_string()));
    }

    #[test]
    fn no_close_for_void_element() {
        let source = "<template><br></template>";
        let result = auto_close_tag(source, 14);
        assert!(result.is_none(), "void elements should not be closed");
    }

    #[test]
    fn no_close_for_self_closing() {
        let source = "<template><MyComp /></template>";
        // Offset after `/>` — but `>` is at pos 19, so offset is 20
        let result = auto_close_tag(source, 20);
        assert!(result.is_none(), "self-closing tags should not be closed");
    }

    #[test]
    fn auto_close_component() {
        let source = "<template><MyComponent>\n</template>";
        let result = auto_close_tag(source, 23);
        assert_eq!(result, Some("$0</MyComponent>".to_string()));
    }

    #[test]
    fn auto_close_with_attributes() {
        let source = r#"<template><div class="foo" id="bar">"#;
        let result = auto_close_tag(source, 36);
        assert_eq!(result, Some("$0</div>".to_string()));
    }

    #[test]
    fn no_close_for_closing_tag() {
        let source = "<template></div></template>";
        // Cursor after `</div>` at offset 16
        let result = auto_close_tag(source, 16);
        assert!(
            result.is_none(),
            "closing tags should not trigger auto-close"
        );
    }

    #[test]
    fn no_close_for_comment() {
        let source = "<template><!-- comment --></template>";
        // This is `-->` so `>` at offset 25
        let result = auto_close_tag(source, 26);
        assert!(result.is_none(), "comments should not trigger auto-close");
    }

    #[test]
    fn auto_close_template_tag() {
        let source = "<template>\n</template>";
        // Offset after first `<template>`
        let result = auto_close_tag(source, 10);
        // Already has </template> right after (with newline)
        assert!(
            result.is_none(),
            "should not close when </template> already exists after whitespace"
        );
    }

    #[test]
    fn auto_close_preserves_case() {
        let source = "<template><MyButton>\n</template>";
        let result = auto_close_tag(source, 20);
        assert_eq!(
            result,
            Some("$0</MyButton>".to_string()),
            "should preserve original tag case"
        );
    }

    #[test]
    fn no_close_for_void_input() {
        let source = r#"<template><input type="text"></template>"#;
        let result = auto_close_tag(source, 29);
        assert!(result.is_none(), "input is a void element");
    }

    // ========================================================================
    // Carrier markup-context gate (BLOCKER 1 + 2)
    //
    // `auto_close_tag_in_carrier` is the gated entry point the on-type handler
    // calls. It must fire ONLY in the TEMPLATE/MARKUP region of the carrier:
    //   * Vue  — inside `<template>` content (NOT `<script>` / `<style>`).
    //   * Svelte — at the root markup (NOT inside `<script>` / `<style>`).
    // and within that region only for a real markup open tag — never for a
    // TS-generic `>` (mustache expression) or a `>` inside a quoted attribute.
    // ========================================================================

    /// Byte offset of the position immediately AFTER `needle` in `source`.
    fn after(source: &str, needle: &str) -> usize {
        source.find(needle).expect("needle present") + needle.len()
    }

    // ── Vue carrier ────────────────────────────────────────────────────────

    #[test]
    fn vue_template_closes_div() {
        let source = "<template><div>\n</template>\n<script>const x = 1;</script>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "a `>` in the Vue template region must auto-close",
        );
    }

    /// HTML tag names are case-insensitive, so a `<DIV>` already followed by `</div>` (different
    /// case) is ALREADY CLOSED — auto-close must not insert a duplicate `</DIV>`. Fails if the
    /// already-closed guard compares the `</tag` prefix case-sensitively.
    #[test]
    fn already_closed_check_is_case_insensitive() {
        let source = "<template><DIV></div></template>";
        let off = after(source, "<DIV>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a `<DIV>` already closed by `</div>` (different case) must NOT insert a duplicate close",
        );

        // Positive control: a `<DIV>` with NO following close still auto-closes (case preserved),
        // proving the case-insensitive guard did not over-fire.
        let open_only = "<template><DIV>\n</template>";
        let off = after(open_only, "<DIV>");
        assert_eq!(
            auto_close_tag_in_carrier(open_only, off, CarrierKind::Vue),
            Some("$0</DIV>".to_string()),
            "an unclosed `<DIV>` must still auto-close, preserving the original case",
        );
    }

    /// A following `</diverse>` shares the `</div` prefix but is a DIFFERENT, longer tag — it does
    /// NOT already-close `<div>`, so the carrier helper must still emit the close. Fails if the
    /// already-closed guard treats `</div` as a prefix without requiring a tag-name terminator.
    #[test]
    fn already_closed_check_requires_tag_name_boundary() {
        let source = "<template><div></diverse></template>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "`</diverse>` does not close `<div>`; the close must still be emitted",
        );
    }

    #[test]
    fn vue_script_generic_does_not_close() {
        // The `>` here closes a TS generic `Box<Foo>` inside `<script lang="ts">`.
        // It is NOT markup and must NOT insert `</Foo>` — BLOCKER 1/2.
        let source = "<template><div></div></template>\n<script lang=\"ts\">\nconst x: Box<Foo> = mk();\n</script>";
        let off = after(source, "Box<Foo>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a TS generic `>` inside <script lang=ts> must never auto-close",
        );
    }

    #[test]
    fn vue_style_gt_does_not_close() {
        // A `>` child combinator inside `<style>` is CSS, not markup.
        let source = "<template><div></div></template>\n<style>\n.a > .b { color: red }\n</style>";
        let off = after(source, ".a >");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a `>` inside <style> (CSS combinator) must never auto-close",
        );
    }

    #[test]
    fn vue_attribute_value_gt_does_not_close() {
        // The `>` is INSIDE a quoted attribute value, not the tag-closing `>`.
        let source = "<template><div title=\"a>b\">\n</template>";
        let off = after(source, "title=\"a>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a `>` inside a quoted attribute value must never auto-close",
        );
    }

    #[test]
    fn vue_mustache_generic_does_not_close() {
        // The `>` closes a TS generic inside a `{{ }}` interpolation expression.
        let source = "<template><div>{{ mk<Foo>() }}</div>\n</template>";
        let off = after(source, "mk<Foo>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a TS generic `>` inside a `{{ }}` interpolation must never auto-close",
        );
    }

    #[test]
    fn vue_template_real_tag_after_mustache_still_closes() {
        // Discriminator: a real markup `>` AFTER a closed mustache still closes —
        // proves the mustache guard does not over-reject the whole template.
        let source = "<template>{{ a }}<section>\n</template>";
        let off = after(source, "<section>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</section>".to_string()),
            "a real open tag after a closed mustache must still auto-close",
        );
    }

    #[test]
    fn vue_root_level_gt_does_not_close() {
        // Between blocks (root level of the SFC) is not markup for Vue.
        let source = "<template><div></div></template>\n<Foo>\n<script></script>";
        let off = after(source, "<Foo>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "Vue markup is only inside <template>; root-level tags must not close",
        );
    }

    #[test]
    fn vue_template_void_still_not_closed() {
        // Void-element behavior is preserved through the gate.
        let source = "<template><br>\n</template>";
        let off = after(source, "<br>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "void elements stay un-closed inside the template region",
        );
    }

    #[test]
    fn vue_template_quoted_attr_tag_still_closes() {
        // Discriminator: the attribute-value guard must NOT over-reject a normal
        // tag whose closing `>` follows quoted attribute values.
        let source = "<template><div class=\"foo\" id=\"bar\">\n</template>";
        let off = after(source, "id=\"bar\">");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "a tag with quoted attributes must still auto-close on its real `>`",
        );
    }

    #[test]
    fn vue_template_control_closes_but_void_sibling_does_not() {
        // Mirrors the e2e readiness pattern: a positive control `<article>` in
        // the same template closes, while the void `<br>` sibling does not — so
        // the e2e's "ready + correctly no edit" distinction is grounded here too.
        let source = "<template><article><br>\n</template>";
        assert_eq!(
            auto_close_tag_in_carrier(source, after(source, "<article>"), CarrierKind::Vue),
            Some("$0</article>".to_string()),
            "the control tag must close (proves readiness in the e2e)",
        );
        assert_eq!(
            auto_close_tag_in_carrier(source, after(source, "<br>"), CarrierKind::Vue),
            None,
            "the void sibling must not close even when a control nearby does",
        );
    }

    #[test]
    fn vue_template_self_closing_still_not_closed() {
        let source = "<template><MyComp />\n</template>";
        let off = after(source, "<MyComp />");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "self-closing tags stay un-closed inside the template region",
        );
    }

    #[test]
    fn vue_template_existing_close_not_doubled() {
        let source = "<template><div></div></template>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "an already-closed tag must not be doubled",
        );
    }

    // ── Svelte carrier ─────────────────────────────────────────────────────

    #[test]
    fn svelte_root_markup_closes_section() {
        // Svelte markup lives at the root (NO <template> wrapper).
        let source = "<script lang=\"ts\">let n = 1;</script>\n<section>\n<p>hi</p>";
        let off = after(source, "<section>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            Some("$0</section>".to_string()),
            "a `>` in Svelte root markup must auto-close",
        );
    }

    #[test]
    fn svelte_script_generic_does_not_close() {
        let source = "<script lang=\"ts\">\nconst x: Box<Foo> = mk();\n</script>\n<div></div>";
        let off = after(source, "Box<Foo>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a TS generic `>` inside a Svelte <script> must never auto-close",
        );
    }

    #[test]
    fn svelte_style_gt_does_not_close() {
        let source = "<div></div>\n<style>\n.a > .b { color: red }\n</style>";
        let off = after(source, ".a >");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a `>` inside a Svelte <style> (CSS combinator) must never auto-close",
        );
    }

    #[test]
    fn svelte_attribute_value_gt_does_not_close() {
        let source = "<div title=\"a>b\">\n<p>hi</p>";
        let off = after(source, "title=\"a>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a `>` inside a quoted attribute value must never auto-close (svelte)",
        );
    }

    #[test]
    fn svelte_void_still_not_closed() {
        let source = "<br>\n<p>hi</p>";
        let off = after(source, "<br>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "void elements stay un-closed in Svelte root markup",
        );
    }

    // ── Svelte single-brace expression region (F2 / F6) ──────────────────────
    //
    // Svelte uses SINGLE-brace `{ expr }` for attribute bindings and logic
    // blocks, NOT Vue's `{{ }}` mustache. A `>` (comparison or TS generic)
    // inside a single-brace expression is NOT a tag close and must not fire.
    // The opposite direction (F6): the REAL tag-closing `>` of a tag that has
    // a `>`-containing single-brace attribute must still close.

    #[test]
    fn svelte_single_brace_comparison_does_not_close() {
        // `a > b` inside `disabled={...}` is a comparison expression, not markup.
        let source = "<button disabled={a > b}>\n<p>x</p>";
        let off = after(source, "{a >");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a `>` inside a Svelte single-brace expression must never auto-close",
        );
    }

    #[test]
    fn svelte_single_brace_class_comparison_does_not_close() {
        let source = "<div class={x > y}>";
        let off = after(source, "{x >");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a `>` inside a Svelte `class={{x > y}}` expression must never auto-close",
        );
    }

    #[test]
    fn svelte_single_brace_generic_does_not_close() {
        // A TS generic `mk<Foo>()` inside a single-brace expression.
        let source = "<Comp value={mk<Foo>()}>";
        let off = after(source, "mk<Foo>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a TS generic `>` inside a Svelte single-brace expression must never auto-close",
        );
    }

    #[test]
    fn svelte_real_close_after_single_brace_attr_still_closes() {
        // F6: the REAL closing `>` of `<button disabled={a > b}>` must still
        // insert `</button>` even though a `>` appears inside the brace expr.
        let source = "<button disabled={a > b}>\n<p>x</p>";
        // Offset after the FINAL `>` (the tag close), i.e. just past `}>`.
        let off = after(source, "{a > b}>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            Some("$0</button>".to_string()),
            "the real tag-closing `>` must still close even with a `>`-containing single-brace attr",
        );
    }

    #[test]
    fn svelte_single_brace_does_not_disturb_vue_literal_brace() {
        // Discriminator: the Svelte single-brace handling must NOT bleed into
        // Vue. In a Vue template, `{` is literal text; a `>` after a literal
        // `{` that is a real tag close must still close. (Vue uses `{{ }}`.)
        let source = "<template><div>text { not an expr <span>\n</template>";
        let off = after(source, "<span>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</span>".to_string()),
            "Vue treats `{{` as literal text; a real tag close after it must still close",
        );
    }

    // ── Multi-line opening tag / multi-line quoted attribute value (F3) ──────
    //
    // When the opening `<` is on a PREVIOUS line, or a quoted value spans a
    // newline, the `>` inside the quoted value must still be recognized as an
    // attribute char (NOT a tag close). The old current-line anchor missed
    // these because no `<` exists on the cursor's line.

    #[test]
    fn vue_multiline_open_tag_attr_gt_does_not_close() {
        // The `<div` is on a previous line; the `>` is inside `title="a>b"`.
        let source = "<template>\n<div\n title=\"a>b\">\n</template>";
        let off = after(source, "title=\"a>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a `>` inside a quoted attr on a multi-line open tag must never auto-close (vue)",
        );
    }

    #[test]
    fn vue_attr_value_spanning_newline_gt_does_not_close() {
        // The quoted value itself spans a newline; the `>` after the newline is
        // still inside the open quote.
        let source = "<template>\n<div title=\"a\nb>c\">\n</template>";
        let off = after(source, "b>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a `>` inside a quoted value spanning a newline must never auto-close (vue)",
        );
    }

    #[test]
    fn svelte_multiline_open_tag_attr_gt_does_not_close() {
        let source = "<div\n title=\"a>b\">\n<p>x</p>";
        let off = after(source, "title=\"a>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a `>` inside a quoted attr on a multi-line open tag must never auto-close (svelte)",
        );
    }

    // ── Case-variant / unclosed script in a Svelte carrier (F4 / F5) ─────────

    #[test]
    fn svelte_case_variant_script_generic_does_not_close() {
        // F4: `<SCRIPT>` (uppercase) must still be a script region, so a TS
        // generic `>` inside it does not auto-close.
        let source = "<SCRIPT lang=\"ts\">\nconst x: Box<Foo> = mk();\n</SCRIPT>\n<div></div>";
        let off = after(source, "Box<Foo>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a TS generic `>` inside a case-variant <SCRIPT> must never auto-close (svelte)",
        );
    }

    #[test]
    fn svelte_unclosed_script_generic_does_not_close() {
        // F5: an unclosed `<script>` (mid-typing) must still establish a
        // non-markup region to EOF, so a generic `>` inside it does not fire.
        let source = "<script lang=\"ts\">\nconst x: Box<Foo> = mk();";
        let off = after(source, "Box<Foo>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a TS generic `>` inside an unclosed <script> must never auto-close (svelte)",
        );
    }

    // ── Nested <template> inside a Vue SFC (F7) ──────────────────────────────

    #[test]
    fn vue_tag_after_nested_template_still_closes() {
        // A nested slot `<template #foo>...</template>` inside the SFC
        // `<template>` must NOT terminate the outer markup region. A real tag
        // typed AFTER the nested template still auto-closes.
        let source = "<template>\n<template #foo><span></span></template>\n<div>\n</template>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "a tag after a nested <template> must still auto-close (outer template region)",
        );
    }

    // ── Apostrophe in template/markup prose does not suppress a later close ───
    //
    // An apostrophe in ordinary template TEXT (`Bob's`, `don't`) is NOT an
    // attribute-value quote: it must not desync the attribute-quote scan and
    // suppress the auto-close of a LATER, genuine open tag. The attribute-quote
    // scan must therefore anchor at the CANDIDATE tag's `<` (which sits AFTER
    // the prose apostrophe), not at the whole markup window's start.

    #[test]
    fn vue_apostrophe_in_text_does_not_suppress_later_tag_close() {
        // `Bob's` apostrophe is template prose. The later `<div>` must close.
        let source = "<template><p>Bob's</p>\n<div>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "an apostrophe in template text must not suppress a later tag's auto-close (vue)",
        );
    }

    #[test]
    fn svelte_apostrophe_in_earlier_markup_does_not_suppress_later_close() {
        // `don't` apostrophe sits in earlier (script) content. The root `<div>`
        // must still close — the apostrophe must not desync the attr-quote scan.
        let source = "<script>// don't</script>\n<div>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            Some("$0</div>".to_string()),
            "an apostrophe in earlier markup must not suppress a later root tag's close (svelte)",
        );
    }

    // ── Literal `<template>` in a script comment does not leak the markup window ─
    //
    // A `<script>` block placed BEFORE the real `<template>` that contains a
    // literal `<template>` in a comment/string must NOT be mistaken for the SFC
    // template open: the markup window must be located via the structural SFC
    // block scan (which skips script/style content), so a `Box<Foo>` generic in
    // that script is never treated as Vue template markup.

    #[test]
    fn vue_template_literal_in_script_comment_does_not_leak_markup_window() {
        let source = "<script>\n// <template>\nconst x: Box<Foo> = mk();\n</script>\n<template><div></div></template>";
        let off = after(source, "Box<Foo>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a literal <template> in a script comment must not leak the markup window into the script",
        );
    }

    #[test]
    fn vue_script_before_template_real_template_tag_still_closes() {
        // Discriminator for the structural-window fix: with a `<script>` block
        // BEFORE the real `<template>` (containing a decoy `<template>` in a
        // comment), a genuine open tag INSIDE the real template still closes —
        // the window must lock onto the real template, not over-reject it.
        let source =
            "<script>\n// <template>\nconst x = 1;\n</script>\n<template>\n<div>\n</template>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "a real tag inside the true template must still close despite a script-comment decoy",
        );
    }

    // ── Vue literal `{` must NOT make the tag-lookup brace-aware ──────────────
    //
    // Vue interpolation is `{{ }}`; a LONE `{` in a Vue template is literal text.
    // The candidate-tag lookup ([`nearest_tag_lt`]) tracks `{`/`}` nesting only
    // for Svelte (where `{ expr }` is an expression span whose inner `<` must be
    // ignored). For Vue that brace tracking is WRONG: a literal `{` before a tag
    // would hide that tag's `<` (recorded only at brace-depth 0) and also let a
    // following `"` open a spurious attribute quote at brace-depth > 0. Both
    // desync the attribute-value guard:
    //   * the bug-case below would wrong-FIRE `</div>` inside an attribute value
    //     (a literal `{` hides the `<`, so the guard declines and the brace-blind
    //     fallback inserts a close mid-attribute — buffer corruption);
    //   * the positive complement would wrong-SUPPRESS a real tag close (a
    //     literal `{` then a literal `"` open a spurious quote that swallows the
    //     real tag's `<`, so the guard reports "in attribute value" and the close
    //     is refused).
    // With Vue brace tracking OFF both are corrected.

    #[test]
    fn vue_literal_brace_before_tag_does_not_break_attr_guard() {
        // A literal `{` precedes `<div title="a>b">`. Typing the `>` INSIDE the
        // quoted value must NOT insert a close mid-attribute. Pre-fix the literal
        // `{` made the brace-aware lookup hide the `<`, so the attribute guard
        // declined and the fallback wrong-fired `</div>` inside the value.
        let source = "<template>{ <div title=\"a>b\">";
        let off = after(source, "title=\"a>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a literal `{{` before a tag must not make the attribute guard miss; \
             a `>` inside a quoted attr value must never auto-close (vue)",
        );
    }

    #[test]
    fn vue_literal_brace_then_quote_does_not_suppress_real_close() {
        // Positive complement: a literal `{` AND a literal `"` in Vue text before
        // `<div>`. Typing the REAL `>` of `<div>` must still close. Pre-fix the
        // brace-aware lookup opened a spurious quote at brace-depth > 0 on the
        // literal `"`, swallowed the real tag's `<`, and the attribute guard
        // wrongly suppressed the close (returned None). With braces inert for Vue
        // the literal `"` stays literal text and the real close fires.
        let source = "<template><a>{ \"<div>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "a literal `{{` then a literal `\"` in Vue text must not suppress a later real tag close",
        );
    }

    // ── Carrier EXPRESSION spans never anchor the candidate tag (P2) ──────────
    //
    // The candidate-tag scan ([`nearest_tag_lt`]) must SKIP the carrier's
    // expression spans (Vue `{{ }}` mustache, Svelte `{ }`) entirely. A `<` or a
    // `"`/`'` INSIDE an expression span is not markup: it must never be recorded
    // as the candidate tag's `<` (which would mis-anchor the attribute-value
    // guard and let a `>` inside a later quoted attribute wrongly fire a close).
    // A LONE single `{` in a Vue template is literal text — only `{{` opens a
    // span — so it stays inert.

    #[test]
    fn vue_lt_inside_mustache_string_does_not_misanchor_attr_guard() {
        // A `<` lives literally inside a `{{ "<" }}` interpolation string. Typing
        // the `>` inside the LATER `title="a>b"` attribute must NOT close. Pre-fix
        // the brace-blind Vue scan recorded the mustache-interior `<` as the
        // candidate tag, mis-anchored the quote walk, and the brace-blind fallback
        // wrong-fired `</div>` mid-attribute. Post-fix the `{{ }}` span is skipped,
        // the real `<div` anchors the guard, the `>` is seen in-quote → None.
        let source = "<template>{{ \"<\" }}<div title=\"a>b\">";
        let off = after(source, "title=\"a>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a `<` inside a `{{ }}` mustache string must not anchor the attr guard; \
             a `>` inside a later quoted attr value must never auto-close (vue)",
        );
    }

    #[test]
    fn vue_real_tag_after_mustache_with_lt_string_still_closes() {
        // Positive complement: the same `{{ "<" }}` span precedes `<div>`; typing
        // the REAL `>` of `<div>` must close. Pre-fix the unmatched `"` from the
        // mustache string leaked across the brace-blind scan and opened a spurious
        // quote that swallowed the real tag's `<`, so the attr guard wrongly
        // reported in-quote and suppressed the close (None). Post-fix the span is
        // skipped, the real `<div` anchors cleanly, and the close fires.
        let source = "<template>{{ \"<\" }}<div>\n</template>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "a real open tag after a `{{ }}` mustache containing a `<`/`\"` must still close (vue)",
        );
    }

    #[test]
    fn svelte_lt_inside_single_brace_span_does_not_misanchor_attr_guard() {
        // A `<` lives inside a Svelte `{ a < b }` single-brace expression. Typing
        // the `>` inside the LATER `title="a>b"` attribute must NOT close — the
        // candidate-tag scan must skip the `{ }` span so the real `<div` anchors
        // the quote walk. (Confirms the carrier-aware skip keeps the established
        // Svelte brace behavior for the expression-span class.)
        let source = "{ a < b }<div title=\"a>b\">";
        let off = after(source, "title=\"a>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
            None,
            "a `<` inside a Svelte single-brace expression must not anchor the attr guard; \
             a `>` inside a later quoted attr value must never auto-close (svelte)",
        );
    }

    // ── Vue `{{ }}` interpolation close-emitter parity (carrier-blind regression) ─
    //
    // The Vue close emitter must reason over markup the SAME way the gate guards
    // do: the candidate `<` is the nearest preceding REAL-markup `<` (skipping a
    // Vue `{{ … }}` mustache span). A `<` that lives only inside a closed
    // interpolation must NEVER anchor the close emitter — typing the literal `>`
    // right after the `}}` must produce NO close, not a garbage `</foo">`.

    #[test]
    fn vue_lt_inside_closed_mustache_string_then_gt_does_not_close() {
        // `<foo` lives only inside the closed `{{ "<foo" }}` interpolation; the
        // typed `>` sits immediately after `}}` in template text. There is no real
        // start tag here, so nothing closes.
        let source = "<template>{{ \"<foo\" }}>";
        let off = after(source, "}}>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            None,
            "a `<foo` that lives only inside a closed `{{ }}` interpolation must not \
             anchor the close emitter; a `>` typed after `}}` must never auto-close (vue)",
        );
    }

    #[test]
    fn vue_real_tag_after_closed_mustache_with_lt_string_still_closes() {
        // Positive complement: the SAME `{{ "<foo" }}` span precedes a REAL
        // `<div>`; typing the real `>` of `<div>` must still close. The mustache
        // span is skipped, the real `<div` anchors the emitter, and the close
        // fires — proving the collapse onto the carrier-aware scanner did not break
        // Vue close behavior.
        let source = "<template>{{ \"<foo\" }}<div>\n</template>";
        let off = after(source, "<div>");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "a real open tag after a closed `{{ }}` interpolation containing a `<foo` \
             string must still auto-close (vue)",
        );
    }

    #[test]
    fn vue_carrier_emitter_closes_div_with_attributes() {
        // The unified carrier emitter (driven through the gated entry) must close a
        // plain Vue template tag whose closing `>` follows quoted attribute values —
        // the behavior the legacy carrier-blind `auto_close_tag` provided, now
        // preserved through the single carrier-aware scanner.
        let source = "<template><div class=\"foo\" id=\"bar\">";
        let off = after(source, "id=\"bar\">");
        assert_eq!(
            auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
            Some("$0</div>".to_string()),
            "the unified carrier emitter must close a Vue tag with quoted attributes \
             on its real `>`",
        );
    }
}
