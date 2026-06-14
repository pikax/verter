//! The Svelte byte tokenizer + recursive-descent template parser.
//!
//! A single forward byte scan over the component source produces a
//! [`ParsedSvelte`]. The scan is INFALLIBLE: malformed or out-of-scope
//! constructs collect an inline [`SvelteParseDiagnostic`] and the scan
//! continues — the matrix's parse-without-crash contract. Expression interiors
//! are NOT parsed (no type lowering in this crate, per the thin-adapters
//! guard); the parser records their spans and leaves the bytes to the
//! projector.
//!
//! Brace-depth tracking inside expressions is string-aware (single/double/
//! template quotes) so a `}` inside a string literal does not close an
//! interpolation early. Element nesting is tracked so the parser pairs open and
//! close tags and recovers on a mismatch.

use verter_span::Span;

use super::template_ast::{
    ParsedSvelte, SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue, SvelteBlock,
    SvelteBlockClause, SvelteBlockKind, SvelteClauseKind, SvelteDirective, SvelteDirectiveKind,
    SvelteElement, SvelteElementKind, SvelteNode, SvelteParseDiagnostic, SvelteScript,
    SvelteSpecialKind, SvelteStyle, SvelteTag, SvelteTagKind,
};

/// Parse Svelte component `source` into a [`ParsedSvelte`].
#[must_use]
pub fn parse_svelte(source: &str) -> ParsedSvelte {
    let mut parser = SvelteParser::new(source);
    parser.parse_root();
    parser.finish()
}

/// The forward byte parser state.
struct SvelteParser<'a> {
    src: &'a [u8],
    text: &'a str,
    pos: usize,
    instance_script: Option<SvelteScript>,
    module_script: Option<SvelteScript>,
    styles: Vec<SvelteStyle>,
    template: Vec<SvelteNode>,
    diagnostics: Vec<SvelteParseDiagnostic>,
}

impl<'a> SvelteParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            src: source.as_bytes(),
            text: source,
            pos: 0,
            instance_script: None,
            module_script: None,
            styles: Vec::new(),
            template: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn finish(self) -> ParsedSvelte {
        ParsedSvelte {
            instance_script: self.instance_script,
            module_script: self.module_script,
            styles: self.styles,
            template: self.template,
            diagnostics: self.diagnostics,
        }
    }

    fn len(&self) -> usize {
        self.src.len()
    }

    fn at(&self, pos: usize) -> u8 {
        self.src.get(pos).copied().unwrap_or(0)
    }

    fn cur(&self) -> u8 {
        self.at(self.pos)
    }

    fn eof(&self) -> bool {
        self.pos >= self.len()
    }

    fn slice(&self, span: Span) -> &'a str {
        let (s, e) = (span.start as usize, span.end as usize);
        self.text.get(s..e).unwrap_or("")
    }

    fn diag(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        self.diagnostics.push(SvelteParseDiagnostic {
            code,
            message: message.into(),
            span,
        });
    }

    /// Whether the source from `pos` begins with `needle` (ASCII, case-sensitive).
    fn starts_with_at(&self, pos: usize, needle: &[u8]) -> bool {
        self.src
            .get(pos..pos + needle.len())
            .is_some_and(|s| s == needle)
    }

    /// Whether the source from `pos` begins with `needle`, ASCII-case-insensitive.
    fn starts_with_ci_at(&self, pos: usize, needle: &[u8]) -> bool {
        self.src
            .get(pos..pos + needle.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(needle))
    }

    // ── Root scan ──────────────────────────────────────────────────────

    /// Scan the top-level component body: text, comments, `<script>` /
    /// `<style>` blocks, elements, and template tags/blocks.
    fn parse_root(&mut self) {
        let mut text_start = self.pos;
        while !self.eof() {
            let b = self.cur();
            if b == b'<' {
                // A comment, a script/style block, or an element.
                self.flush_text(&mut text_start);
                if self.starts_with_at(self.pos, b"<!--") {
                    let node = self.parse_comment();
                    self.template.push(node);
                } else if self.try_parse_special_block_root() {
                    // consumed a <script>/<style> block at root scope
                } else {
                    let node = self.parse_element_or_recover();
                    self.template.extend(node);
                }
                text_start = self.pos;
            } else if b == b'{' {
                self.flush_text(&mut text_start);
                let node = self.parse_brace_construct();
                self.template.extend(node);
                text_start = self.pos;
            } else {
                self.pos += 1;
            }
        }
        // trailing text
        if text_start < self.pos {
            self.template.push(SvelteNode::Text(Span::new(
                text_start as u32,
                self.pos as u32,
            )));
        }
    }

    fn flush_text(&mut self, text_start: &mut usize) {
        if *text_start < self.pos {
            self.template.push(SvelteNode::Text(Span::new(
                *text_start as u32,
                self.pos as u32,
            )));
        }
        *text_start = self.pos;
    }

    /// At a `<`, try to consume a top-level `<script>` or `<style>` block,
    /// recording it on the parser. Returns `true` when one was consumed.
    fn try_parse_special_block_root(&mut self) -> bool {
        if self.starts_with_ci_at(self.pos, b"<script") && self.is_tag_boundary(self.pos + 7) {
            if let Some(script) = self.parse_script_block() {
                if script.is_module {
                    if self.module_script.is_none() {
                        self.module_script = Some(script);
                    }
                } else if self.instance_script.is_none() {
                    self.instance_script = Some(script);
                }
            }
            return true;
        }
        if self.starts_with_ci_at(self.pos, b"<style") && self.is_tag_boundary(self.pos + 6) {
            if let Some(style) = self.parse_style_block() {
                self.styles.push(style);
            }
            return true;
        }
        false
    }

    /// Whether `pos` is a tag-name boundary (whitespace, `>`, `/`, or EOF) —
    /// distinguishes `<script>` from `<scripted>`.
    fn is_tag_boundary(&self, pos: usize) -> bool {
        match self.src.get(pos) {
            None => true,
            Some(&b) => b.is_ascii_whitespace() || b == b'>' || b == b'/',
        }
    }

    // ── Script / style blocks ──────────────────────────────────────────

    fn parse_script_block(&mut self) -> Option<SvelteScript> {
        let open_start = self.pos;
        let (attributes, open_end, self_closing) = self.parse_open_tag_attributes(self.pos + 1)?;
        let tag_open = Span::new(open_start as u32, open_end as u32);
        let lang = attr_text_value(&attributes, self, "lang");
        let is_module = attribute_present(&attributes, self, "module")
            || attr_text_value(&attributes, self, "context").as_deref() == Some("module");
        if self_closing {
            return Some(SvelteScript {
                is_module,
                tag_open,
                content: None,
                attributes,
                lang,
            });
        }
        self.pos = open_end;
        let content_start = self.pos;
        let close = self.find_close_tag(b"script");
        match close {
            Some((content_end, after)) => {
                self.pos = after;
                Some(SvelteScript {
                    is_module,
                    tag_open,
                    content: Some(Span::new(content_start as u32, content_end as u32)),
                    attributes,
                    lang,
                })
            }
            None => {
                self.diag(
                    "unterminated-script",
                    "unterminated <script> block",
                    tag_open,
                );
                self.pos = self.len();
                Some(SvelteScript {
                    is_module,
                    tag_open,
                    content: Some(Span::new(content_start as u32, self.len() as u32)),
                    attributes,
                    lang,
                })
            }
        }
    }

    fn parse_style_block(&mut self) -> Option<SvelteStyle> {
        let open_start = self.pos;
        let (attributes, open_end, self_closing) = self.parse_open_tag_attributes(self.pos + 1)?;
        let tag_open = Span::new(open_start as u32, open_end as u32);
        if self_closing {
            return Some(SvelteStyle {
                tag_open,
                content: None,
                attributes,
            });
        }
        self.pos = open_end;
        let content_start = self.pos;
        match self.find_close_tag(b"style") {
            Some((content_end, after)) => {
                self.pos = after;
                Some(SvelteStyle {
                    tag_open,
                    content: Some(Span::new(content_start as u32, content_end as u32)),
                    attributes,
                })
            }
            None => {
                self.diag("unterminated-style", "unterminated <style> block", tag_open);
                self.pos = self.len();
                Some(SvelteStyle {
                    tag_open,
                    content: Some(Span::new(content_start as u32, self.len() as u32)),
                    attributes,
                })
            }
        }
    }

    /// Find the matching `</tag>` close from `self.pos`. Returns
    /// `(content_end, after_close)`. Scans raw (script/style contents are
    /// opaque) — it does not descend into nested markup.
    fn find_close_tag(&self, tag: &[u8]) -> Option<(usize, usize)> {
        let mut p = self.pos;
        while p < self.len() {
            if self.at(p) == b'<' && self.at(p + 1) == b'/' && self.starts_with_ci_at(p + 2, tag) {
                let after_name = p + 2 + tag.len();
                if self.is_tag_boundary(after_name) || self.at(after_name) == b'>' {
                    // advance to the closing '>'
                    let mut q = after_name;
                    while q < self.len() && self.at(q) != b'>' {
                        q += 1;
                    }
                    let after = (q + 1).min(self.len());
                    return Some((p, after));
                }
            }
            p += 1;
        }
        None
    }

    // ── Comments ───────────────────────────────────────────────────────

    fn parse_comment(&mut self) -> SvelteNode {
        let start = self.pos;
        // skip "<!--"
        self.pos += 4;
        while self.pos < self.len() {
            if self.starts_with_at(self.pos, b"-->") {
                self.pos += 3;
                return SvelteNode::Comment(Span::new(start as u32, self.pos as u32));
            }
            self.pos += 1;
        }
        self.diag(
            "unterminated-comment",
            "unterminated comment",
            Span::new(start as u32, self.len() as u32),
        );
        self.pos = self.len();
        SvelteNode::Comment(Span::new(start as u32, self.len() as u32))
    }

    // ── Elements ───────────────────────────────────────────────────────

    /// Parse an element at `<`, recovering by emitting the `<` as text on a
    /// malformed tag.
    fn parse_element_or_recover(&mut self) -> Vec<SvelteNode> {
        let start = self.pos;
        if self.at(self.pos + 1) == b'/' {
            // A stray close tag at this scope — skip it (handled by the caller
            // recursion when nested). Emit nothing; consume to '>'.
            let mut p = self.pos + 2;
            while p < self.len() && self.at(p) != b'>' {
                p += 1;
            }
            self.pos = (p + 1).min(self.len());
            return Vec::new();
        }
        // Parse the tag name.
        let name_start = self.pos + 1;
        let mut p = name_start;
        while p < self.len() && is_tag_name_byte(self.at(p)) {
            p += 1;
        }
        if p == name_start {
            // Not a real tag (`<` followed by non-name) — emit `<` as text.
            self.pos += 1;
            return vec![SvelteNode::Text(Span::new(start as u32, self.pos as u32))];
        }
        let name_span = Span::new(name_start as u32, p as u32);
        let name = self.slice(name_span).to_string();
        let kind = classify_element(&name);

        let Some((attributes, open_end, self_closing)) = self.parse_open_tag_attributes(p) else {
            // Unterminated open tag — emit text and bail.
            self.diag(
                "unterminated-tag",
                "unterminated element open tag",
                Span::new(start as u32, self.len() as u32),
            );
            self.pos = self.len();
            return Vec::new();
        };
        let open_span = Span::new(start as u32, open_end as u32);
        self.pos = open_end;

        let void = self_closing || is_void_element(&name);
        let mut children = Vec::new();
        if !void {
            if matches!(kind, SvelteElementKind::NestedStyle) {
                // Nested <style> inside template markup — opaque content.
                let content_start = self.pos;
                if let Some((_content_end, after)) = self.find_close_tag(b"style") {
                    self.pos = after;
                    children.push(SvelteNode::Text(Span::new(
                        content_start as u32,
                        self.pos as u32,
                    )));
                } else {
                    self.pos = self.len();
                }
            } else {
                children = self.parse_children_until_close(&name);
            }
        }

        vec![SvelteNode::Element(SvelteElement {
            name,
            name_span,
            kind,
            attributes,
            children,
            self_closing: void,
            open_span,
        })]
    }

    /// Parse child nodes until the matching `</name>` close (or EOF). The close
    /// tag is consumed.
    fn parse_children_until_close(&mut self, name: &str) -> Vec<SvelteNode> {
        let mut children = Vec::new();
        let mut text_start = self.pos;
        while !self.eof() {
            let b = self.cur();
            if b == b'<' {
                // Is this the matching close tag?
                if self.at(self.pos + 1) == b'/' {
                    if self.matches_close_name(self.pos + 2, name) {
                        // flush text and consume the close tag
                        if text_start < self.pos {
                            children.push(SvelteNode::Text(Span::new(
                                text_start as u32,
                                self.pos as u32,
                            )));
                        }
                        self.consume_close_tag();
                        return children;
                    }
                    // A different close tag — recovery: flush and consume it,
                    // treating the current element as implicitly closed.
                    if text_start < self.pos {
                        children.push(SvelteNode::Text(Span::new(
                            text_start as u32,
                            self.pos as u32,
                        )));
                    }
                    // Leave the foreign close for the parent to handle: stop.
                    return children;
                }
                if text_start < self.pos {
                    children.push(SvelteNode::Text(Span::new(
                        text_start as u32,
                        self.pos as u32,
                    )));
                }
                if self.starts_with_at(self.pos, b"<!--") {
                    let c = self.parse_comment();
                    children.push(c);
                } else {
                    let nodes = self.parse_element_or_recover();
                    children.extend(nodes);
                }
                text_start = self.pos;
            } else if b == b'{' {
                if text_start < self.pos {
                    children.push(SvelteNode::Text(Span::new(
                        text_start as u32,
                        self.pos as u32,
                    )));
                }
                // A block-closing/clause token belongs to an enclosing block —
                // stop child scan so the block parser sees it.
                if self.is_block_close_or_clause() {
                    return children;
                }
                let nodes = self.parse_brace_construct();
                children.extend(nodes);
                text_start = self.pos;
            } else {
                self.pos += 1;
            }
        }
        if text_start < self.pos {
            children.push(SvelteNode::Text(Span::new(
                text_start as u32,
                self.pos as u32,
            )));
        }
        children
    }

    fn matches_close_name(&self, pos: usize, name: &str) -> bool {
        let nb = name.as_bytes();
        self.src.get(pos..pos + nb.len()).is_some_and(|s| s == nb) && {
            let after = pos + nb.len();
            self.at(after) == b'>' || self.at(after).is_ascii_whitespace()
        }
    }

    fn consume_close_tag(&mut self) {
        // at `</`
        let mut p = self.pos + 2;
        while p < self.len() && self.at(p) != b'>' {
            p += 1;
        }
        self.pos = (p + 1).min(self.len());
    }

    /// Parse an open tag's attributes starting at `from` (just after the tag
    /// name). Returns `(attributes, position_after_'>', self_closing)`, or
    /// `None` if the tag is unterminated.
    fn parse_open_tag_attributes(
        &mut self,
        from: usize,
    ) -> Option<(Vec<SvelteAttribute>, usize, bool)> {
        let mut p = from;
        let mut attributes = Vec::new();
        loop {
            // skip whitespace
            while p < self.len() && self.at(p).is_ascii_whitespace() {
                p += 1;
            }
            // Skip an HTML comment inside the open tag (5.53 tolerance): a
            // `<!-- ... -->` between attributes is consumed (not recorded) so the
            // following real attributes are not lost.
            if self.starts_with_at(p, b"<!--") {
                let mut q = p + 4;
                while q < self.len() && !self.starts_with_at(q, b"-->") {
                    q += 1;
                }
                p = (q + 3).min(self.len());
                continue;
            }
            if p >= self.len() {
                return None;
            }
            let b = self.at(p);
            if b == b'>' {
                return Some((attributes, p + 1, false));
            }
            if b == b'/' && self.at(p + 1) == b'>' {
                return Some((attributes, p + 2, true));
            }
            if b == b'{' {
                // A spread `{...x}`, shorthand `{value}`, or an inline tag
                // (`{@attach}`/comment) used as an attribute.
                let (attr, next) = self.parse_brace_attribute(p);
                attributes.push(attr);
                p = next;
                continue;
            }
            // A named attribute or directive.
            let (attr, next) = self.parse_named_attribute(p);
            attributes.push(attr);
            p = next;
        }
    }

    /// Parse a `{ ... }` attribute (spread / shorthand / inline). Returns the
    /// attribute and the position after the closing brace.
    fn parse_brace_attribute(&mut self, from: usize) -> (SvelteAttribute, usize) {
        let inner_start = from + 1;
        let end = self.find_matching_brace(inner_start);
        let inner = Span::new(inner_start as u32, end as u32);
        let span = Span::new(from as u32, (end + 1).min(self.len()) as u32);
        let body = self.slice(inner).trim_start();
        let attr = if body.starts_with("...") {
            SvelteAttribute {
                kind: SvelteAttributeKind::Spread(inner),
                span,
            }
        } else if body.starts_with('@') || body.starts_with('#') || body.starts_with('/') {
            // `{@attach expr}` / other tag used in attribute position — record
            // as a plain attribute carrying the inner span so the projector
            // can dispatch on the leading sigil.
            SvelteAttribute {
                kind: SvelteAttributeKind::Plain {
                    name: String::new(),
                    name_span: Span::new(inner_start as u32, inner_start as u32),
                    value: Some(SvelteAttributeValue::Expression(inner)),
                },
                span,
            }
        } else {
            // Attribute-value shorthand `{name}` → name == value expression.
            SvelteAttribute {
                kind: SvelteAttributeKind::Plain {
                    name: body.to_string(),
                    name_span: inner,
                    value: Some(SvelteAttributeValue::Expression(inner)),
                },
                span,
            }
        };
        (attr, (end + 1).min(self.len()))
    }

    /// Parse a named attribute or directive at `from`. Returns the attribute
    /// and the position after it.
    fn parse_named_attribute(&mut self, from: usize) -> (SvelteAttribute, usize) {
        let mut p = from;
        // The attribute name runs until whitespace, `=`, `/`, `>`, or EOF.
        while p < self.len() {
            let b = self.at(p);
            if b.is_ascii_whitespace()
                || b == b'='
                || b == b'>'
                || (b == b'/' && self.at(p + 1) == b'>')
            {
                break;
            }
            p += 1;
        }
        let name_span = Span::new(from as u32, p as u32);
        let raw_name = self.slice(name_span).to_string();

        // Optional value.
        let mut value: Option<SvelteAttributeValue> = None;
        let mut after = p;
        // skip ws before '='
        let mut q = p;
        while q < self.len() && self.at(q).is_ascii_whitespace() {
            q += 1;
        }
        if self.at(q) == b'=' {
            q += 1;
            while q < self.len() && self.at(q).is_ascii_whitespace() {
                q += 1;
            }
            let (val, next) = self.parse_attribute_value(q);
            value = val;
            after = next;
        }

        let span = Span::new(from as u32, after as u32);
        // Directive? `prefix:local|mods`
        if let Some(colon) = raw_name.find(':') {
            let prefix = &raw_name[..colon];
            let rest = &raw_name[colon + 1..];
            let mut parts = rest.split('|');
            let local = parts.next().unwrap_or("").to_string();
            let modifiers: Vec<String> = parts.map(|s| s.to_string()).collect();
            let dkind = SvelteDirectiveKind::from_prefix(prefix);
            // `svelte:` is an element namespace, never a directive — but element
            // names are handled before this point, so a `svelte:` here is an odd
            // attribute; classify as Unknown directive (parse-without-crash).
            let kind = SvelteAttributeKind::Directive(SvelteDirective {
                kind: dkind,
                local,
                modifiers,
                value,
            });
            return (SvelteAttribute { kind, span }, after);
        }

        (
            SvelteAttribute {
                kind: SvelteAttributeKind::Plain {
                    name: raw_name,
                    name_span,
                    value,
                },
                span,
            },
            after,
        )
    }

    /// Parse an attribute value at `from` (a quoted string, a `{expr}`, or a
    /// mixed run). Returns the value and the position after it.
    fn parse_attribute_value(&mut self, from: usize) -> (Option<SvelteAttributeValue>, usize) {
        let b = self.at(from);
        if b == b'"' || b == b'\'' {
            let quote = b;
            let body_start = from + 1;
            let mut p = body_start;
            let mut saw_brace = false;
            while p < self.len() && self.at(p) != quote {
                if self.at(p) == b'{' {
                    saw_brace = true;
                }
                p += 1;
            }
            let body = Span::new(body_start as u32, p as u32);
            let after = (p + 1).min(self.len());
            let value = if saw_brace {
                SvelteAttributeValue::Mixed(body)
            } else {
                SvelteAttributeValue::Text(body)
            };
            (Some(value), after)
        } else if b == b'{' {
            let inner_start = from + 1;
            let end = self.find_matching_brace(inner_start);
            let inner = Span::new(inner_start as u32, end as u32);
            (
                Some(SvelteAttributeValue::Expression(inner)),
                (end + 1).min(self.len()),
            )
        } else {
            // Unquoted value: runs until whitespace / `>` / `/`.
            let mut p = from;
            while p < self.len() {
                let c = self.at(p);
                if c.is_ascii_whitespace() || c == b'>' || (c == b'/' && self.at(p + 1) == b'>') {
                    break;
                }
                p += 1;
            }
            (
                Some(SvelteAttributeValue::Text(Span::new(from as u32, p as u32))),
                p,
            )
        }
    }

    // ── Brace constructs ───────────────────────────────────────────────

    /// At a `{`, dispatch on the construct (`{#...}` block, `{@...}`/`{const}`/
    /// `{let}` tag, or plain `{expr}` interpolation).
    fn parse_brace_construct(&mut self) -> Vec<SvelteNode> {
        let start = self.pos;
        let next = self.at(self.pos + 1);
        match next {
            b'#' => self.parse_block(),
            b'@' => vec![self.parse_at_tag()],
            b'/' => {
                // Stray block-close at this scope — consume it and warn.
                let end = self.find_matching_brace(self.pos + 1);
                self.diag(
                    "unexpected-block-close",
                    "unexpected block-closing tag",
                    Span::new(start as u32, (end + 1) as u32),
                );
                self.pos = (end + 1).min(self.len());
                Vec::new()
            }
            b':' => {
                // Stray clause at this scope — consume it and warn.
                let end = self.find_matching_brace(self.pos + 1);
                self.diag(
                    "unexpected-clause",
                    "unexpected block clause",
                    Span::new(start as u32, (end + 1) as u32),
                );
                self.pos = (end + 1).min(self.len());
                Vec::new()
            }
            _ => {
                // Either a declaration tag (`{const x = …}` / `{let x = …}`) or
                // a plain interpolation `{expr}`.
                let inner_start = self.pos + 1;
                let end = self.find_matching_brace(inner_start);
                let inner = Span::new(inner_start as u32, end as u32);
                let body = self.slice(inner);
                let trimmed = body.trim_start();
                self.pos = (end + 1).min(self.len());
                if let Some(kind) = declaration_tag_kind(trimmed) {
                    let keyword_len = if matches!(kind, SvelteTagKind::Const) {
                        5
                    } else {
                        3
                    };
                    let lead_ws = body.len() - body.trim_start().len();
                    let decl_inner_start = inner.start as usize + lead_ws + keyword_len;
                    vec![SvelteNode::Tag(SvelteTag {
                        kind,
                        span: Span::new(start as u32, self.pos as u32),
                        inner: Span::new(
                            decl_inner_start.min(inner.end as usize) as u32,
                            inner.end,
                        ),
                    })]
                } else {
                    vec![SvelteNode::Interpolation(inner)]
                }
            }
        }
    }

    /// Parse a `{#...}` block.
    fn parse_block(&mut self) -> Vec<SvelteNode> {
        let start = self.pos;
        let head_inner_start = self.pos + 2; // skip `{#`
        let head_end = self.find_matching_brace(head_inner_start);
        let head = self.slice(Span::new(head_inner_start as u32, head_end as u32));
        self.pos = (head_end + 1).min(self.len());

        let mut keyword_end = 0;
        while keyword_end < head.len() && head.as_bytes()[keyword_end].is_ascii_alphabetic() {
            keyword_end += 1;
        }
        let keyword = &head[..keyword_end];
        let head_rest_start = head_inner_start + keyword_end;
        let head_rest = &head[keyword_end..];

        match keyword {
            "if" => self.parse_if_block(start, head_rest_start, head_rest),
            "each" => self.parse_each_block(start, head_rest_start, head_rest),
            "await" => self.parse_await_block(start, head_rest_start, head_rest),
            "key" => self.parse_key_block(start, head_rest_start, head_rest),
            "snippet" => self.parse_snippet_block(start, head_rest_start, head_rest),
            _ => {
                self.diag(
                    "unknown-block",
                    format!("unknown block `{{#{keyword}}}`"),
                    Span::new(start as u32, self.pos as u32),
                );
                // Best-effort: scan children until a matching `{/keyword}`.
                let kw = keyword.to_string();
                let children = self.parse_block_children(&[]);
                self.consume_block_close(&kw);
                vec![SvelteNode::Block(SvelteBlock {
                    kind: SvelteBlockKind::Key,
                    span: Span::new(start as u32, self.pos as u32),
                    head_expr: None,
                    children,
                    clauses: Vec::new(),
                })]
            }
        }
    }

    fn parse_if_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        let head_expr = nonempty_span(head_rest_start, head_rest);
        let children = self.parse_block_children(&["else", "/if"]);
        let mut clauses = Vec::new();
        loop {
            match self.peek_clause_keyword() {
                Some(kw) if kw == "else" => {
                    let (clause_kind, expr, tag_span, body) = self.parse_else_clause();
                    clauses.push(SvelteBlockClause {
                        kind: clause_kind,
                        expr,
                        tag_span,
                        children: body,
                    });
                    if matches!(clause_kind, SvelteClauseKind::Else) {
                        break;
                    }
                }
                _ => break,
            }
        }
        self.consume_block_close("if");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::If,
            span: Span::new(start as u32, self.pos as u32),
            head_expr,
            children,
            clauses,
        })]
    }

    fn parse_each_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        // `expr as item, index (key)` — `as`/item optional (the no-item form).
        let (list_expr, item, index, key) =
            super::block_head::parse_each_head(head_rest_start, head_rest);
        let children = self.parse_block_children(&["else", "/each"]);
        let mut clauses = Vec::new();
        if matches!(self.peek_clause_keyword().as_deref(), Some("else")) {
            let (_kind, _expr, tag_span, body) = self.parse_else_clause();
            clauses.push(SvelteBlockClause {
                kind: SvelteClauseKind::Else,
                expr: None,
                tag_span,
                children: body,
            });
        }
        self.consume_block_close("each");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::Each { item, index, key },
            span: Span::new(start as u32, self.pos as u32),
            head_expr: list_expr,
            children,
            clauses,
        })]
    }

    fn parse_await_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        // `{#await expr}` or `{#await expr then v}` or `{#await expr catch e}`.
        let trimmed = head_rest.trim();
        let (promise_expr, then_inline, catch_inline) =
            super::block_head::parse_await_head(head_rest_start, head_rest);
        let _ = trimmed;
        let mut then_binding = then_inline;
        let mut catch_binding = catch_inline;
        let children = self.parse_block_children(&["then", "catch", "/await"]);
        let mut clauses = Vec::new();
        loop {
            match self.peek_clause_keyword().as_deref() {
                Some("then") => {
                    let (binding, tag_span, body) = self.parse_then_or_catch("then");
                    then_binding = binding.or(then_binding);
                    clauses.push(SvelteBlockClause {
                        kind: SvelteClauseKind::Then,
                        expr: binding,
                        tag_span,
                        children: body,
                    });
                }
                Some("catch") => {
                    let (binding, tag_span, body) = self.parse_then_or_catch("catch");
                    catch_binding = binding.or(catch_binding);
                    clauses.push(SvelteBlockClause {
                        kind: SvelteClauseKind::Catch,
                        expr: binding,
                        tag_span,
                        children: body,
                    });
                }
                _ => break,
            }
        }
        self.consume_block_close("await");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::Await {
                then_binding,
                catch_binding,
            },
            span: Span::new(start as u32, self.pos as u32),
            head_expr: promise_expr,
            children,
            clauses,
        })]
    }

    fn parse_key_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        let head_expr = nonempty_span(head_rest_start, head_rest);
        let children = self.parse_block_children(&["/key"]);
        self.consume_block_close("key");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::Key,
            span: Span::new(start as u32, self.pos as u32),
            head_expr,
            children,
            clauses: Vec::new(),
        })]
    }

    fn parse_snippet_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        // `name(params)`
        let (name_span, name_text, params) =
            super::block_head::parse_snippet_head(head_rest_start, head_rest);
        let children = self.parse_block_children(&["/snippet"]);
        self.consume_block_close("snippet");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::Snippet {
                name: name_span,
                name_text,
                params,
            },
            span: Span::new(start as u32, self.pos as u32),
            head_expr: None,
            children,
            clauses: Vec::new(),
        })]
    }

    /// Parse a block body run, stopping at any of the `stoppers` clause/close
    /// keywords (without the `{:`/`{/` prefix; `/if` etc. denote the close).
    fn parse_block_children(&mut self, _stoppers: &[&str]) -> Vec<SvelteNode> {
        let mut children = Vec::new();
        let mut text_start = self.pos;
        while !self.eof() {
            let b = self.cur();
            if b == b'{' {
                if self.is_block_close_or_clause() {
                    if text_start < self.pos {
                        children.push(SvelteNode::Text(Span::new(
                            text_start as u32,
                            self.pos as u32,
                        )));
                    }
                    return children;
                }
                if text_start < self.pos {
                    children.push(SvelteNode::Text(Span::new(
                        text_start as u32,
                        self.pos as u32,
                    )));
                }
                let nodes = self.parse_brace_construct();
                children.extend(nodes);
                text_start = self.pos;
            } else if b == b'<' {
                if text_start < self.pos {
                    children.push(SvelteNode::Text(Span::new(
                        text_start as u32,
                        self.pos as u32,
                    )));
                }
                if self.starts_with_at(self.pos, b"<!--") {
                    let c = self.parse_comment();
                    children.push(c);
                } else {
                    let nodes = self.parse_element_or_recover();
                    children.extend(nodes);
                }
                text_start = self.pos;
            } else {
                self.pos += 1;
            }
        }
        if text_start < self.pos {
            children.push(SvelteNode::Text(Span::new(
                text_start as u32,
                self.pos as u32,
            )));
        }
        children
    }

    /// Whether the brace at `self.pos` opens a block clause (`{:`) or close
    /// (`{/`).
    fn is_block_close_or_clause(&self) -> bool {
        self.cur() == b'{' && (self.at(self.pos + 1) == b':' || self.at(self.pos + 1) == b'/')
    }

    /// Peek the keyword of a `{:keyword ...}` clause at `self.pos`, without
    /// consuming.
    fn peek_clause_keyword(&self) -> Option<String> {
        if self.cur() != b'{' || self.at(self.pos + 1) != b':' {
            return None;
        }
        let mut p = self.pos + 2;
        let kw_start = p;
        while p < self.len() && self.at(p).is_ascii_alphabetic() {
            p += 1;
        }
        self.text.get(kw_start..p).map(|s| s.to_string())
    }

    /// Parse an `{:else}` / `{:else if expr}` clause and its body.
    ///
    /// Returns the clause kind, the optional condition span, the clause-tag head
    /// span (`{:else…}` INCLUDING braces — overwritten verbatim by the
    /// projector), and the body.
    fn parse_else_clause(&mut self) -> (SvelteClauseKind, Option<Span>, Span, Vec<SvelteNode>) {
        // at `{:else...}`
        let tag_start = self.pos;
        let inner_start = self.pos + 2;
        let head_end = self.find_matching_brace(inner_start);
        let head = self.slice(Span::new(inner_start as u32, head_end as u32));
        self.pos = (head_end + 1).min(self.len());
        let tag_span = Span::new(tag_start as u32, self.pos as u32);
        let rest = head.trim_start_matches("else");
        let trimmed = rest.trim_start();
        let (kind, expr) = if let Some(after_if) = trimmed.strip_prefix("if") {
            let expr_text = after_if.trim();
            let expr_offset = inner_start
                + (head.len() - rest.len())
                + (rest.len() - trimmed.len())
                + 2
                + (after_if.len() - after_if.trim_start().len());
            (
                SvelteClauseKind::ElseIf,
                nonempty_span(expr_offset, expr_text),
            )
        } else {
            (SvelteClauseKind::Else, None)
        };
        let body = self.parse_block_children(&["else", "/if", "/each"]);
        (kind, expr, tag_span, body)
    }

    /// Parse a `{:then v}` / `{:catch e}` clause and its body.
    ///
    /// Returns the optional binding span, the clause-tag head span (`{:then…}` /
    /// `{:catch…}` INCLUDING braces — overwritten verbatim by the projector),
    /// and the body.
    fn parse_then_or_catch(&mut self, keyword: &str) -> (Option<Span>, Span, Vec<SvelteNode>) {
        let tag_start = self.pos;
        let inner_start = self.pos + 2;
        let head_end = self.find_matching_brace(inner_start);
        let head = self.slice(Span::new(inner_start as u32, head_end as u32));
        self.pos = (head_end + 1).min(self.len());
        let tag_span = Span::new(tag_start as u32, self.pos as u32);
        let rest = head.trim_start().trim_start_matches(keyword);
        let binding_text = rest.trim();
        let offset =
            inner_start + (head.len() - rest.len()) + (rest.len() - rest.trim_start().len());
        let binding = nonempty_span(offset, binding_text);
        let body = self.parse_block_children(&["then", "catch", "/await"]);
        (binding, tag_span, body)
    }

    /// Consume the matching `{/keyword}` close, warning if it is missing.
    fn consume_block_close(&mut self, keyword: &str) {
        if self.cur() == b'{' && self.at(self.pos + 1) == b'/' {
            let inner_start = self.pos + 2;
            let head_end = self.find_matching_brace(inner_start);
            let name = self.slice(Span::new(inner_start as u32, head_end as u32));
            if name.trim() == keyword {
                self.pos = (head_end + 1).min(self.len());
                return;
            }
        }
        self.diag(
            "unterminated-block",
            format!("missing `{{/{keyword}}}` close"),
            Span::new(
                self.pos.min(self.len()) as u32,
                self.pos.min(self.len()) as u32,
            ),
        );
    }

    /// Parse an `{@...}` tag.
    fn parse_at_tag(&mut self) -> SvelteNode {
        let start = self.pos;
        let inner_start = self.pos + 1; // skip `{`, keep `@`
        let end = self.find_matching_brace(inner_start);
        let inner = self.slice(Span::new(inner_start as u32, end as u32));
        self.pos = (end + 1).min(self.len());
        // inner starts with `@keyword`
        let after_at = &inner[1..];
        let mut kw_end = 0;
        while kw_end < after_at.len() && after_at.as_bytes()[kw_end].is_ascii_alphabetic() {
            kw_end += 1;
        }
        let keyword = &after_at[..kw_end];
        let kind = match keyword {
            "render" => SvelteTagKind::Render,
            "html" => SvelteTagKind::Html,
            "const" => SvelteTagKind::LegacyConst,
            "debug" => SvelteTagKind::Debug,
            "attach" => SvelteTagKind::Attach,
            _ => SvelteTagKind::Unknown,
        };
        if matches!(kind, SvelteTagKind::Unknown) {
            self.diag(
                "unknown-tag",
                format!("unknown tag `{{@{keyword}}}`"),
                Span::new(start as u32, self.pos as u32),
            );
        }
        // The inner expression span begins after `@keyword` plus separating ws.
        let body_after_kw = &after_at[kw_end..];
        let lead = body_after_kw.len() - body_after_kw.trim_start().len();
        let expr_start = inner_start + 1 + kw_end + lead;
        let expr_end = end;
        SvelteNode::Tag(SvelteTag {
            kind,
            span: Span::new(start as u32, self.pos as u32),
            inner: Span::new(expr_start.min(expr_end) as u32, expr_end as u32),
        })
    }

    /// Find the matching closing `}` for a brace opened just before
    /// `inner_start` (i.e. `inner_start` is the first inner byte). Returns the
    /// index of the closing `}` (or EOF). STRING- AND COMMENT-AWARE so a `}`
    /// inside a quote, a `//` line comment, a `/* */` block comment, or a regex
    /// literal does not close the interpolation early.
    fn find_matching_brace(&self, inner_start: usize) -> usize {
        let mut depth = 1usize;
        let mut p = inner_start;
        let mut quote: Option<u8> = None;
        // The last significant byte, used to decide whether a `/` opens a regex
        // (after an operator / `(` / `,` / `=` / …) vs a division (after a value).
        let mut prev_significant: u8 = b'{';
        while p < self.len() {
            let b = self.at(p);
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
            if b == b'/' && self.at(p + 1) == b'/' {
                p += 2;
                while p < self.len() && self.at(p) != b'\n' {
                    p += 1;
                }
                continue;
            }
            if b == b'/' && self.at(p + 1) == b'*' {
                p += 2;
                while p < self.len() && !self.starts_with_at(p, b"*/") {
                    p += 1;
                }
                p = (p + 2).min(self.len());
                continue;
            }
            // A regex literal opens only in expression position (after an
            // operator/opener, never after a value/identifier/`)`). Skip its
            // body (char-class- and escape-aware) so a `}` inside `/[}]/` does
            // not close early.
            if b == b'/' && regex_allowed_after(prev_significant) {
                let mut q = p + 1;
                let mut in_class = false;
                while q < self.len() {
                    let rb = self.at(q);
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
        self.len()
    }
}

// ── Free helpers ───────────────────────────────────────────────────────

fn is_tag_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.'
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

fn classify_element(name: &str) -> SvelteElementKind {
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

fn is_void_element(name: &str) -> bool {
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

/// Classify a brace body as a declaration tag (`const`/`let`) — the 5.56
/// declaration-tag forms (NOT the `{@const}` legacy form, which the `@` path
/// handles).
fn declaration_tag_kind(trimmed: &str) -> Option<SvelteTagKind> {
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

/// Read a quoted/text attribute value by name (case-insensitive) from a parsed
/// attribute list, returning the value text.
fn attr_text_value(attrs: &[SvelteAttribute], parser: &SvelteParser, name: &str) -> Option<String> {
    attrs.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain { name: n, value, .. } if n.eq_ignore_ascii_case(name) => {
            match value {
                Some(SvelteAttributeValue::Text(span)) => Some(parser.slice(*span).to_string()),
                Some(SvelteAttributeValue::Expression(span)) => {
                    Some(parser.slice(*span).to_string())
                }
                Some(SvelteAttributeValue::Mixed(span)) => Some(parser.slice(*span).to_string()),
                None => Some("true".to_string()),
            }
        }
        _ => None,
    })
}

/// Whether a bare (valueless) attribute of `name` is present.
fn attribute_present(attrs: &[SvelteAttribute], parser: &SvelteParser, name: &str) -> bool {
    attrs.iter().any(|a| match &a.kind {
        SvelteAttributeKind::Plain { name: n, value, .. } => {
            let _ = parser;
            n.eq_ignore_ascii_case(name) && value.is_none()
        }
        _ => false,
    })
}
