//! Stateless byte/tag classification + JS-aware brace scanning helpers for the
//! Svelte tokenizer.
//!
//! These are PURE functions (no parser state) the recursive-descent tokenizer leans
//! on: the tag-name byte predicate, the string/comment/regex-aware matching-brace
//! scan (shared with the runtime mixed-attribute lowering), the regex-position
//! whitelist, the element-kind / void-element classifiers, and the declaration-tag
//! keyword classifier. They live here so the main tokenizer file stays focused on the
//! forward scan + the recovery/violation recording.

use verter_span::Span;

use super::template_ast::{
    SvelteAttribute, SvelteAttributeKind, SvelteDirectiveKind, SvelteElementKind,
    SvelteSpecialKind, SvelteTagKind,
};

/// Whether a byte may appear in an element / attribute tag name.
pub(super) fn is_tag_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.'
}

/// The canonical name of a ROOT-ONLY `<svelte:*>` meta tag (`svelte:options` / `svelte:head`
/// / `svelte:window` / `svelte:document` / `svelte:body`), or `None` for any other tag.
/// Mirrors upstream's `root_only_meta_tags` map (`element.js`) — the set whose SECOND
/// occurrence is `svelte_meta_duplicate`. (The other meta tags — `svelte:element`,
/// `svelte:component`, `svelte:self`, `svelte:fragment`, `svelte:boundary` — are NOT
/// root-only and may repeat.)
pub(crate) fn root_only_meta_tag_name(name: &str) -> Option<&'static str> {
    match name {
        "svelte:options" => Some("svelte:options"),
        "svelte:head" => Some("svelte:head"),
        "svelte:window" => Some("svelte:window"),
        "svelte:document" => Some("svelte:document"),
        "svelte:body" => Some("svelte:body"),
        _ => None,
    }
}

/// The official duplicate-check TYPE-CLASS for an attribute, after the
/// `BindDirective → Attribute` normalization (`phases/1-parse/state/element.js`). Only the
/// four checkable AST types map to a class; every other form (a spread, an `on:` / `use:` /
/// `transition:` / `let:` directive) is NOT duplicate-checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DuplicateKeyClass {
    /// A plain `Attribute` (a `Plain` attribute OR a normalized `bind:` directive — so a
    /// static `value` and a `bind:value` collide).
    Attribute,
    /// A `class:` directive (a distinct namespace from a plain `class` attribute).
    Class,
    /// A `style:` directive (a distinct namespace from a plain `style` attribute).
    Style,
}

/// The normalized `(class, name)` duplicate key for an attribute, or `None` when the
/// attribute is not one of the four duplicate-checkable AST types. Mirrors the official
/// `element.js` rule EXACTLY: a `bind:X` normalizes to `Attribute`+`X`; `class:`/`style:`
/// are distinct namespaces; `on:`/`use:`/`transition:`/`in:`/`out:`/`animate:`/`let:` (and
/// an unrecognised directive) and a spread are NOT keyed.
///
/// This is the SINGLE duplicate-key normalization shared by the parser's open-tag
/// `attribute_duplicate` mint and any downstream consumer — never re-implemented as a
/// second divergent copy.
pub(crate) fn duplicate_attribute_key(attr: &SvelteAttribute) -> Option<(DuplicateKeyClass, &str)> {
    match &attr.kind {
        SvelteAttributeKind::Plain { name, .. } => Some((DuplicateKeyClass::Attribute, name)),
        SvelteAttributeKind::Directive(directive) => match directive.kind {
            SvelteDirectiveKind::Bind => Some((DuplicateKeyClass::Attribute, &directive.local)),
            SvelteDirectiveKind::Class => Some((DuplicateKeyClass::Class, &directive.local)),
            SvelteDirectiveKind::Style => Some((DuplicateKeyClass::Style, &directive.local)),
            SvelteDirectiveKind::On
            | SvelteDirectiveKind::Use
            | SvelteDirectiveKind::Transition
            | SvelteDirectiveKind::In
            | SvelteDirectiveKind::Out
            | SvelteDirectiveKind::Animate
            | SvelteDirectiveKind::Let
            | SvelteDirectiveKind::Unknown => None,
        },
        // A spread and an `{@attach}` attachment are NOT duplicate-checked (official
        // parity: attachments stack — `<div {@attach a} {@attach b}>` is valid).
        SvelteAttributeKind::Spread(_) | SvelteAttributeKind::Attach { .. } => None,
    }
}

/// Find the matching closing `}` for a brace opened just before `inner_start`
/// (i.e. `inner_start` is the first inner byte) within `src`. Returns the index of
/// the closing `}`, or `src.len()` at EOF.
///
/// STRING-, COMMENT-, and REGEX-AWARE: a `}` inside a single/double/backtick
/// string, a `//` line comment, a `/* */` block comment, or a `/regex/` literal
/// does NOT close the brace early. This is the SINGLE JS-aware brace scan shared by
/// the parser's interpolation tokenizer and the runtime mixed-attribute lowering —
/// the runtime never re-implements a byte-level `{`/`}` counter (which closes at a
/// `}` inside a string, e.g. `class="x {format('}')} y"`).
pub(crate) fn find_matching_brace_in(src: &[u8], inner_start: usize) -> usize {
    let len = src.len();
    let at = |p: usize| src.get(p).copied().unwrap_or(0);
    let starts_with_at = |p: usize, needle: &[u8]| src.get(p..p + needle.len()) == Some(needle);
    let mut depth = 1usize;
    let mut p = inner_start;
    let mut quote: Option<u8> = None;
    // The last significant byte, used to decide whether a `/` opens a regex
    // (after an operator / `(` / `,` / `=` / …) vs a division (after a value).
    let mut prev_significant: u8 = b'{';
    while p < len {
        let b = at(p);
        if let Some(q) = quote {
            if b == b'\\' {
                p += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            p += 1;
            continue;
        }
        // Comments.
        if b == b'/' && at(p + 1) == b'/' {
            p += 2;
            while p < len && at(p) != b'\n' {
                p += 1;
            }
            continue;
        }
        if b == b'/' && at(p + 1) == b'*' {
            p += 2;
            while p < len && !starts_with_at(p, b"*/") {
                p += 1;
            }
            p = (p + 2).min(len);
            continue;
        }
        // A regex literal opens only in expression position (after an
        // operator/opener, never after a value/identifier/`)`). Skip its body
        // (char-class- and escape-aware) so a `}` inside `/[}]/` does not close
        // early.
        if b == b'/' && regex_allowed_after(prev_significant) {
            let mut q = p + 1;
            let mut in_class = false;
            while q < len {
                let rb = at(q);
                if rb == b'\\' {
                    q += 2;
                    continue;
                }
                match rb {
                    b'[' => in_class = true,
                    b']' => in_class = false,
                    b'/' if !in_class => {
                        q += 1;
                        break;
                    }
                    b'\n' => break,
                    _ => {}
                }
                q += 1;
            }
            p = q;
            prev_significant = b'/';
            continue;
        }
        match b {
            b'"' | b'\'' | b'`' => quote = Some(b),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return p;
                }
            }
            _ => {}
        }
        if !b.is_ascii_whitespace() {
            prev_significant = b;
        }
        p += 1;
    }
    len
}

/// Whether a `/` in expression text opens a REGEX literal (vs a division).
///
/// CONSERVATIVE WHITELIST: a `/` is treated as a regex ONLY after a byte that
/// UNAMBIGUOUSLY precedes an expression (an opener / separator / binary
/// operator / assignment). Every other context — a value-ending byte (an
/// identifier char, `)`, `]`, `}`, a digit, `$`, `_`), AND the AMBIGUOUS postfix
/// bytes (`+`/`-` which may be `++`/`--`, `!` which may be a TS non-null
/// assertion) — is DIVISION, so the regex body is NOT skipped. A missed
/// regex-skip only matters when a `}` sits inside a regex (rare); a FALSE
/// regex-skip would swallow real expression bytes, so the whitelist fails toward
/// division. The brace scanner is correct either way for the common case; this
/// only guards the `}`-inside-regex corner.
fn regex_allowed_after(prev: u8) -> bool {
    matches!(
        prev,
        b'(' | b'['
            | b'{'
            | b','
            | b';'
            | b':'
            | b'='
            | b'<'
            | b'>'
            | b'&'
            | b'|'
            | b'?'
            | b'*'
            | b'%'
            | b'^'
            | b'~'
            | b'\n'
    )
}

/// Classify an element NAME into its structural kind (special `<svelte:*>`, nested
/// `<style>`, uppercase/dotted component, or lowercase intrinsic).
pub(super) fn classify_element(name: &str) -> SvelteElementKind {
    if let Some(local) = name.strip_prefix("svelte:") {
        return SvelteElementKind::Special(SvelteSpecialKind::from_local(local));
    }
    if name.eq_ignore_ascii_case("style") {
        return SvelteElementKind::NestedStyle;
    }
    // Component: starts uppercase or is dotted (member access).
    let first = name.chars().next().unwrap_or('a');
    if first.is_ascii_uppercase() || name.contains('.') {
        SvelteElementKind::Component
    } else {
        SvelteElementKind::Intrinsic
    }
}

/// Whether `name` is an HTML VOID element (no closing tag permitted).
pub(super) fn is_void_element(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Whether a `<p>` is AUTO-CLOSED before a direct child with the lowercased HTML tag
/// `child_tag` — the official `autoclosing_children` `<p>` descendant list (the browser
/// closes a `<p>` before any of these). Shared by the parser's explicit-`</p>` autoclose
/// reject mint (the close-handling authority) and the official-reject gate's
/// IMPLICIT-autoclose suppression scan, so both surfaces read ONE block-child predicate.
/// A §1.2-shaped component can only actually nest `div` / `h1` / `p`, but the full list is
/// mirrored faithfully.
pub(crate) fn paragraph_autocloses_on_block_child(child_tag: &str) -> bool {
    matches!(
        child_tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "div"
            | "dl"
            | "fieldset"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hgroup"
            | "hr"
            | "main"
            | "menu"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "ul"
    )
}

/// Classify a brace body as a declaration tag (`const`/`let`) — the 5.56
/// declaration-tag forms (NOT the `{@const}` legacy form, which the `@` path
/// handles).
pub(super) fn declaration_tag_kind(trimmed: &str) -> Option<SvelteTagKind> {
    if let Some(rest) = trimmed.strip_prefix("const") {
        if rest.starts_with(|c: char| c.is_whitespace()) {
            return Some(SvelteTagKind::Const);
        }
    }
    if let Some(rest) = trimmed.strip_prefix("let") {
        if rest.starts_with(|c: char| c.is_whitespace()) {
            return Some(SvelteTagKind::Let);
        }
    }
    None
}

/// A non-empty trimmed span anchored at `offset` for `text` (the raw run after
/// a keyword). Returns `None` for an all-whitespace run.
pub(super) fn nonempty_span(offset: usize, text: &str) -> Option<Span> {
    let lead = text.len() - text.trim_start().len();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let start = offset + lead;
    Some(Span::new(start as u32, (start + trimmed.len()) as u32))
}
