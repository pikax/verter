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

/// Map a resolved [`verter_session::FileLanguage`] to its markup [`CarrierKind`], fail-closed.
///
/// This is the SHARED, framework-NEUTRAL carrier-kind classifier the markup
/// routing layers consult (the on-type auto-close gate and the add-import
/// carrier-preamble re-anchor). It keys off DESCRIPTOR IDENTITY, not a Vue-only
/// predicate or a `!is_svelte()` fallback: it confirms the language is a
/// registered framework CARRIER row (its adapter id AND carrier language match a
/// `built_in_descriptors()` row), then maps the row's wire
/// [`FrameworkTag`](verter_protocol::typeinfo::graph::FrameworkTag) to a
/// `CarrierKind`.
///
/// FAIL-CLOSED: a non-carrier `FileLanguage` (plain script / template row) and
/// any carrier whose tag has no explicit `CarrierKind` arm both return `None`. A
/// future third markup carrier therefore drops cleanly here until it gets its
/// own `CarrierKind` and an arm below — it is NEVER silently treated as Vue (the
/// hazard a `!is_svelte()` "not-Svelte ⇒ Vue" fallback would carry).
pub(crate) fn carrier_kind_for_language(
    language: &verter_session::FileLanguage,
) -> Option<CarrierKind> {
    use verter_protocol::typeinfo::graph::FrameworkTag;
    let adapter_id = language.adapter_id()?;
    let carrier_language = language.carrier_language_id()?;
    let tag = verter_session::framework::descriptor::built_in_descriptors()
        .into_iter()
        .find(|d| &d.id == adapter_id && d.carrier_language.as_ref() == Some(carrier_language))
        .map(|d| d.tag)?;
    match tag {
        FrameworkTag::Vue => Some(CarrierKind::Vue),
        FrameworkTag::Svelte => Some(CarrierKind::Svelte),
        _ => None,
    }
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
#[path = "auto_close_tag_tests.rs"]
mod auto_close_tag_tests;
