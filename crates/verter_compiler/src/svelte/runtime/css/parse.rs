//! The span-bearing Svelte CSS parser — a faithful AST-building port of the
//! official `svelte@5.56.3` CSS body reader (`phases/1-parse/read/style.js` +
//! the `Parser` byte primitives in `phases/1-parse/index.js`).
//!
//! The byte cursor mirrors the validation-only reader in
//! [`css_reject`](crate::svelte::runtime::css_reject) (the official-reject
//! gate's first-error CSS-body probe): the reader operates on the WHOLE
//! component `source` from the CSS body's start and stops the body loop at
//! the literal `</style` or EOF, because upstream's nested readers run PAST
//! `</style>` inside an unterminated construct. Every produced node carries a
//! byte [`Span`] of ABSOLUTE offsets into the ORIGINAL source. A malformed
//! body returns a typed [`CssParseError`] carrying the exact official parse
//! code — never a panic.

use verter_span::Span;

use super::types::{
    Atrule, Block, BlockChild, Combinator, ComplexSelector, ComplexSelectorMetadata, Declaration,
    RelativeSelector, RelativeSelectorMetadata, Rule, RuleMetadata, SelectorList, SimpleSelector,
    StyleChild, StyleSheet,
};

/// A typed CSS body-parse failure: the official parse code + the byte span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssParseError {
    /// The official parse code (`css_expected_identifier` /
    /// `css_empty_declaration` / `css_selector_invalid` / `expected_token` /
    /// `unexpected_eof`), or the reader-internal
    /// `css_unicode_escape_out_of_range` for an escape the decoded name
    /// cannot represent.
    pub code: &'static str,
    /// The byte span of the failure (absolute in the component source).
    pub span: Span,
}

impl CssParseError {
    fn at(code: &'static str, start: usize, end: usize) -> Self {
        Self {
            code,
            span: Span::new(start as u32, end as u32),
        }
    }
}

type ParseResult<T> = Result<T, CssParseError>;

/// Parse the CSS body at `content` (absolute byte offsets into `source`) into
/// the span-bearing [`StyleSheet`] AST — the official `read_style` body read
/// (`read_body(parser, p => p.match('</style') || p.index >= p.template.length)`).
pub fn parse_style_body(source: &str, content: Span) -> ParseResult<StyleSheet> {
    let mut parser = CssParser {
        src: source.as_bytes(),
        text: source,
        index: (content.start as usize).min(source.len()),
    };
    let children = parser.read_body()?;
    Ok(StyleSheet {
        span: content,
        children,
    })
}

/// The byte cursor + the upstream `Parser` primitives the CSS readers use —
/// the AST-building counterpart of the validation-only cursor in
/// [`css_reject`](crate::svelte::runtime::css_reject).
struct CssParser<'a> {
    src: &'a [u8],
    text: &'a str,
    index: usize,
}

impl<'a> CssParser<'a> {
    fn len(&self) -> usize {
        self.src.len()
    }

    fn at_eof(&self) -> bool {
        self.index >= self.len()
    }

    /// `parser.match(str)` — whether `str` occurs at the current index.
    fn matches(&self, s: &[u8]) -> bool {
        self.src[self.index..].starts_with(s)
    }

    /// `parser.eat(str, required)` — consume `str` if present (returning
    /// `true`); when `required` and absent, throw `expected_token`.
    fn eat(&mut self, s: &[u8], required: bool) -> ParseResult<bool> {
        if self.matches(s) {
            self.index += s.len();
            Ok(true)
        } else if required {
            Err(CssParseError::at("expected_token", self.index, self.index))
        } else {
            Ok(false)
        }
    }

    /// `parser.allow_whitespace()` — skip the parser's whitespace set.
    fn allow_whitespace(&mut self) {
        while self.index < self.len() && is_css_whitespace(self.codepoint_at(self.index)) {
            self.index += char_len(self.src[self.index]);
        }
    }

    /// The Unicode codepoint at byte `i` (decoding the UTF-8 char).
    fn codepoint_at(&self, i: usize) -> u32 {
        self.text[i..].chars().next().map_or(0, |c| c as u32)
    }

    /// `parser.match('</style' …)` body-loop finish predicate: at the literal
    /// `</style` or EOF.
    fn body_finished(&self) -> bool {
        self.matches(b"</style") || self.at_eof()
    }

    // ── read/style.js readers ─────────────────────────────────────────────

    /// `read_body(parser, finished)` — the `read_style` entry loop.
    fn read_body(&mut self) -> ParseResult<Vec<StyleChild>> {
        let mut children = Vec::new();
        loop {
            self.allow_comment_or_whitespace()?;
            if self.body_finished() {
                return Ok(children);
            }
            if self.matches(b"@") {
                children.push(StyleChild::Atrule(self.read_at_rule()?));
            } else {
                children.push(StyleChild::Rule(self.read_rule()?));
            }
        }
    }

    /// `read_at_rule(parser)`.
    fn read_at_rule(&mut self) -> ParseResult<Atrule> {
        let start = self.index;
        self.eat(b"@", true)?;
        let name_start = self.index;
        let name = self.read_identifier()?;
        let name_span = Span::new(name_start as u32, self.index as u32);
        let prelude_start = self.index;
        let prelude = self.read_value()?;
        let prelude_span = Span::new(prelude_start as u32, self.index as u32);
        let block = if self.matches(b"{") {
            Some(self.read_block()?)
        } else {
            self.eat(b";", true)?;
            None
        };
        Ok(Atrule {
            span: Span::new(start as u32, self.index as u32),
            name,
            name_span,
            prelude,
            prelude_span,
            block,
        })
    }

    /// `read_rule(parser)` — a selector list then a block.
    fn read_rule(&mut self) -> ParseResult<Rule> {
        let start = self.index;
        let prelude = self.read_selector_list(false)?;
        let block = self.read_block()?;
        Ok(Rule {
            span: Span::new(start as u32, self.index as u32),
            prelude,
            block,
            metadata: RuleMetadata::default(),
        })
    }

    /// `read_selector_list(parser, inside_pseudo_class)`.
    fn read_selector_list(&mut self, inside_pseudo_class: bool) -> ParseResult<SelectorList> {
        let mut children = Vec::new();
        self.allow_comment_or_whitespace()?;
        let start = self.index;
        while self.index < self.len() {
            children.push(self.read_selector(inside_pseudo_class)?);
            let end = self.index;
            self.allow_comment_or_whitespace()?;
            let closes = if inside_pseudo_class {
                self.matches(b")")
            } else {
                self.matches(b"{")
            };
            if closes {
                return Ok(SelectorList {
                    span: Span::new(start as u32, end as u32),
                    children,
                });
            }
            self.eat(b",", true)?;
            self.allow_comment_or_whitespace()?;
        }
        Err(CssParseError::at("unexpected_eof", self.len(), self.len()))
    }

    /// `read_selector(parser, inside_pseudo_class)` — the per-selector loop.
    /// Branch order is upstream-faithful (`::` before `:`, nth-of before the
    /// combinator lookahead, the combinator lookahead before the bare type
    /// identifier).
    fn read_selector(&mut self, inside_pseudo_class: bool) -> ParseResult<ComplexSelector> {
        let list_start = self.index;
        let mut children: Vec<RelativeSelector> = Vec::new();

        // `create_selector(combinator, start)`.
        let create_selector = |combinator: Option<Combinator>, start: usize| RelativeSelector {
            span: Span::new(start as u32, start as u32),
            combinator,
            selectors: Vec::new(),
            metadata: RelativeSelectorMetadata::default(),
        };

        let mut relative_selector = create_selector(None, self.index);

        while self.index < self.len() {
            let start = self.index;

            if self.eat(b"&", false)? {
                relative_selector.selectors.push(SimpleSelector::Nesting {
                    span: Span::new(start as u32, self.index as u32),
                });
            } else if self.eat(b"*", false)? {
                let mut name = "*".to_string();
                if self.eat(b"|", false)? {
                    // `*` is the namespace (which we ignore).
                    name = self.read_identifier()?;
                }
                relative_selector.selectors.push(SimpleSelector::Type {
                    span: Span::new(start as u32, self.index as u32),
                    name,
                });
            } else if self.eat(b"#", false)? {
                let name = self.read_identifier()?;
                relative_selector.selectors.push(SimpleSelector::Id {
                    span: Span::new(start as u32, self.index as u32),
                    name,
                });
            } else if self.eat(b".", false)? {
                let name = self.read_identifier()?;
                relative_selector.selectors.push(SimpleSelector::Class {
                    span: Span::new(start as u32, self.index as u32),
                    name,
                });
            } else if self.eat(b"::", false)? {
                let name = self.read_identifier()?;
                // The official node is pushed BEFORE its args are read and
                // DISCARDED, so the span excludes any `(...)`.
                relative_selector
                    .selectors
                    .push(SimpleSelector::PseudoElement {
                        span: Span::new(start as u32, self.index as u32),
                        name,
                    });
                if self.eat(b"(", false)? {
                    self.read_selector_list(true)?;
                    self.eat(b")", true)?;
                }
            } else if self.eat(b":", false)? {
                let name = self.read_identifier()?;
                let args = if self.eat(b"(", false)? {
                    let args = self.read_selector_list(true)?;
                    self.eat(b")", true)?;
                    Some(args)
                } else {
                    None
                };
                relative_selector
                    .selectors
                    .push(SimpleSelector::PseudoClass {
                        span: Span::new(start as u32, self.index as u32),
                        name,
                        args,
                    });
            } else if self.eat(b"[", false)? {
                self.allow_whitespace();
                let name = self.read_identifier()?;
                self.allow_whitespace();
                let matcher = self.read_matcher();
                let value = if matcher.is_some() {
                    self.allow_whitespace();
                    Some(self.read_attribute_value()?)
                } else {
                    None
                };
                self.allow_whitespace();
                let flags = self.read_attribute_flags();
                self.allow_whitespace();
                self.eat(b"]", true)?;
                relative_selector.selectors.push(SimpleSelector::Attribute {
                    span: Span::new(start as u32, self.index as u32),
                    name,
                    matcher,
                    value,
                    flags,
                });
            } else if inside_pseudo_class && self.match_nth_of() {
                // The nth-of matcher must come before the combinator matcher
                // to prevent collision (the `+` in `+2n-1`).
                let value = self.read_nth_of();
                relative_selector.selectors.push(SimpleSelector::Nth {
                    span: Span::new(start as u32, self.index as u32),
                    value,
                });
            } else if self.match_percentage() {
                let value = self.read_percentage();
                relative_selector
                    .selectors
                    .push(SimpleSelector::Percentage {
                        span: Span::new(start as u32, self.index as u32),
                        value,
                    });
            } else if !self.match_combinator() {
                let mut name = self.read_identifier()?;
                if self.eat(b"|", false)? {
                    // The namespace is ignored when matching element classes.
                    name = self.read_identifier()?;
                }
                relative_selector.selectors.push(SimpleSelector::Type {
                    span: Span::new(start as u32, self.index as u32),
                    name,
                });
            }

            let index = self.index;
            self.allow_comment_or_whitespace()?;

            let closes = self.matches(b",")
                || if inside_pseudo_class {
                    self.matches(b")")
                } else {
                    self.matches(b"{")
                };
            if closes {
                // Rewind, so the caller decides whether the list continues.
                self.index = index;
                relative_selector.span = Span::new(relative_selector.span.start, index as u32);
                children.push(relative_selector);
                return Ok(ComplexSelector {
                    span: Span::new(list_start as u32, index as u32),
                    children,
                    metadata: ComplexSelectorMetadata::default(),
                });
            }

            self.index = index;
            let combinator = self.read_combinator();

            if let Some(combinator) = combinator {
                if !relative_selector.selectors.is_empty() {
                    relative_selector.span = Span::new(relative_selector.span.start, index as u32);
                    children.push(relative_selector);
                }

                // …and start a new one.
                let combinator_start = combinator.span.start as usize;
                relative_selector = create_selector(Some(combinator), combinator_start);

                self.allow_whitespace();

                let closes_after = self.matches(b",")
                    || if inside_pseudo_class {
                        self.matches(b")")
                    } else {
                        self.matches(b"{")
                    };
                if closes_after {
                    return Err(CssParseError::at(
                        "css_selector_invalid",
                        self.index,
                        self.index,
                    ));
                }
            }
        }

        Err(CssParseError::at("unexpected_eof", self.len(), self.len()))
    }

    /// `read_combinator(parser)` — an explicit `+`/`~`/`>`/`||` combinator, a
    /// whitespace-only descendant combinator, or `None`.
    fn read_combinator(&mut self) -> Option<Combinator> {
        let start = self.index;
        self.allow_whitespace();

        let index = self.index;
        if let Some(name) = self.read_combinator_token() {
            let end = self.index;
            self.allow_whitespace();
            return Some(Combinator {
                span: Span::new(index as u32, end as u32),
                name: name.to_string(),
            });
        }

        if self.index != start {
            return Some(Combinator {
                span: Span::new(start as u32, self.index as u32),
                name: " ".to_string(),
            });
        }

        None
    }

    /// `read_block(parser)` — `{` … block items … `}`.
    fn read_block(&mut self) -> ParseResult<Block> {
        let start = self.index;
        self.eat(b"{", true)?;
        let mut children = Vec::new();
        while self.index < self.len() {
            self.allow_comment_or_whitespace()?;
            if self.matches(b"}") {
                break;
            }
            children.push(self.read_block_item()?);
        }
        self.eat(b"}", true)?;
        Ok(Block {
            span: Span::new(start as u32, self.index as u32),
            children,
        })
    }

    /// `read_block_item(parser)` — disambiguate a nested rule (the look-ahead
    /// sees a `{` after the value) from a declaration.
    fn read_block_item(&mut self) -> ParseResult<BlockChild> {
        if self.matches(b"@") {
            return Ok(BlockChild::Atrule(self.read_at_rule()?));
        }
        let start = self.index;
        self.read_value()?;
        let next = self.src.get(self.index).copied();
        self.index = start;
        if next == Some(b'{') {
            Ok(BlockChild::Rule(self.read_rule()?))
        } else {
            Ok(BlockChild::Declaration(self.read_declaration()?))
        }
    }

    /// `read_declaration(parser)`.
    fn read_declaration(&mut self) -> ParseResult<Declaration> {
        let start = self.index;
        // `REGEX_WHITESPACE_OR_COLON = /[\s:]/` — JS `\s` is the Unicode set.
        let property =
            self.read_until_codepoint_class(|cp| is_css_whitespace(cp) || cp == u32::from(b':'))?;
        let property = property.to_string();
        self.allow_whitespace();
        self.eat(b":", false)?;
        let index = self.index;
        self.allow_whitespace();
        let value = self.read_value()?;
        if value.is_empty() && !property.starts_with("--") {
            return Err(CssParseError::at("css_empty_declaration", start, index));
        }
        let end = self.index;
        if !self.matches(b"}") {
            self.eat(b";", true)?;
        }
        Ok(Declaration {
            span: Span::new(start as u32, end as u32),
            property,
            value,
        })
    }

    /// `read_value(parser)` — read up to an unquoted/unparen `;` / `{` / `}`
    /// (returning the trimmed value text), skipping `/* … */` comments and
    /// respecting quotes + `url(...)`. At EOF it throws `unexpected_eof`.
    fn read_value(&mut self) -> ParseResult<String> {
        let mut value = String::new();
        let mut escaped = false;
        let mut in_url = false;
        let mut quote_mark: Option<u8> = None;
        while self.index < self.len() {
            let ch = self.src[self.index];
            if escaped {
                value.push('\\');
                self.push_char(&mut value);
                escaped = false;
                continue;
            } else if ch == b'\\' {
                escaped = true;
                self.index += 1;
                continue;
            } else if Some(ch) == quote_mark {
                quote_mark = None;
            } else if ch == b')' {
                in_url = false;
            } else if quote_mark.is_none() && (ch == b'"' || ch == b'\'') {
                quote_mark = Some(ch);
            } else if ch == b'(' && value.ends_with("url") {
                in_url = true;
            } else if (ch == b';' || ch == b'{' || ch == b'}') && !in_url && quote_mark.is_none() {
                return Ok(trim_js_whitespace(&value).to_string());
            } else if ch == b'/'
                && !in_url
                && quote_mark.is_none()
                && self.src.get(self.index + 1) == Some(&b'*')
            {
                self.index += 2;
                while self.index < self.len() {
                    if self.src[self.index] == b'*' && self.src.get(self.index + 1) == Some(&b'/') {
                        self.index += 2;
                        break;
                    }
                    self.index += 1;
                }
                continue;
            }
            self.push_char(&mut value);
        }
        Err(CssParseError::at("unexpected_eof", self.len(), self.len()))
    }

    /// `read_attribute_value(parser)` — a quoted or unquoted attribute value
    /// (quote marks stripped, escapes kept, the result trimmed).
    fn read_attribute_value(&mut self) -> ParseResult<String> {
        let mut value = String::new();
        let mut escaped = false;
        let quote_mark = if self.eat(b"\"", false)? {
            Some(b'"')
        } else if self.eat(b"'", false)? {
            Some(b'\'')
        } else {
            None
        };
        while self.index < self.len() {
            let ch = self.src[self.index];
            if escaped {
                value.push('\\');
                self.push_char(&mut value);
                escaped = false;
                continue;
            } else if ch == b'\\' {
                escaped = true;
                self.index += 1;
                continue;
            }
            // `REGEX_CLOSING_BRACKET = /[\s\]]/` — JS `\s` is the Unicode
            // set, so the unquoted close decodes the codepoint.
            let closes = match quote_mark {
                Some(q) => ch == q,
                None => is_css_whitespace(self.codepoint_at(self.index)) || ch == b']',
            };
            if closes {
                if let Some(q) = quote_mark {
                    self.eat(&[q], true)?;
                }
                return Ok(trim_js_whitespace(&value).to_string());
            }
            self.push_char(&mut value);
        }
        Err(CssParseError::at("unexpected_eof", self.len(), self.len()))
    }

    /// `read_identifier(parser)` — a CSS ident token, DECODED: `\<hex>`
    /// unicode sequences resolve to their codepoint, other `\` escapes keep
    /// the backslash + char (the official `read_identifier`).
    fn read_identifier(&mut self) -> ParseResult<String> {
        let start = self.index;
        if self.match_leading_hyphen_or_digit() {
            return Err(CssParseError::at("css_expected_identifier", start, start));
        }
        let mut identifier = String::new();
        while self.index < self.len() {
            let ch = self.src[self.index];
            if ch == b'\\' {
                if let Some((seq_len, cp)) = self.match_unicode_sequence() {
                    let Some(decoded) = char::from_u32(cp) else {
                        // The official reader would `String.fromCodePoint` an
                        // out-of-range / surrogate codepoint JS can carry but
                        // a Rust string cannot — fail closed.
                        return Err(CssParseError::at(
                            "css_unicode_escape_out_of_range",
                            self.index,
                            self.index + seq_len,
                        ));
                    };
                    identifier.push(decoded);
                    self.index += seq_len;
                } else {
                    identifier.push('\\');
                    self.index += 1;
                    if self.index < self.len() {
                        self.push_char(&mut identifier);
                    }
                }
            } else {
                let cp = self.codepoint_at(self.index);
                if cp >= 160 || is_valid_identifier_char(ch) {
                    self.push_char(&mut identifier);
                } else {
                    break;
                }
            }
        }
        if identifier.is_empty() {
            return Err(CssParseError::at("css_expected_identifier", start, start));
        }
        Ok(identifier)
    }

    /// `allow_comment_or_whitespace(parser)` — whitespace then any run of
    /// `/* … */` / `<!-- … -->` comments; an unterminated comment is
    /// `expected_token` (the required close).
    fn allow_comment_or_whitespace(&mut self) -> ParseResult<()> {
        self.allow_whitespace();
        while self.matches(b"/*") || self.matches(b"<!--") {
            if self.matches(b"/*") {
                self.index += 2;
                self.read_until_str(b"*/");
                self.eat(b"*/", true)?;
            }
            if self.matches(b"<!--") {
                self.index += 4;
                self.read_until_str(b"-->");
                self.eat(b"-->", true)?;
            }
            self.allow_whitespace();
        }
        Ok(())
    }

    // ── scan helpers (the regex primitives, hand-coded) ───────────────────

    /// Push the (whole) char at the current index onto `out` and advance past
    /// it.
    fn push_char(&mut self, out: &mut String) {
        if let Some(c) = self.text[self.index..].chars().next() {
            out.push(c);
            self.index += c.len_utf8();
        } else {
            self.index += 1;
        }
    }

    /// `parser.read_until(pattern)` for a single-CODEPOINT-class pattern (the
    /// JS-regex classes are Unicode-aware — `\s` includes NBSP and the other
    /// Unicode spaces); at EOF (upstream's non-loose branch) throw
    /// `unexpected_eof`.
    fn read_until_codepoint_class(&mut self, pred: impl Fn(u32) -> bool) -> ParseResult<&'a str> {
        if self.at_eof() {
            return Err(CssParseError::at("unexpected_eof", self.len(), self.len()));
        }
        let start = self.index;
        while self.index < self.len() && !pred(self.codepoint_at(self.index)) {
            self.index += char_len(self.src[self.index]);
        }
        Ok(&self.text[start..self.index])
    }

    /// Advance to the first occurrence of `needle` (or EOF).
    fn read_until_str(&mut self, needle: &[u8]) {
        while self.index < self.len() && !self.matches(needle) {
            self.index += 1;
        }
    }

    /// `REGEX_MATCHER = /[~^$*|]?=/y` — the attribute matcher operator text,
    /// when present.
    fn read_matcher(&mut self) -> Option<String> {
        let i = self.index;
        let mut j = i;
        if let Some(&b) = self.src.get(j) {
            if matches!(b, b'~' | b'^' | b'$' | b'*' | b'|') {
                j += 1;
            }
        }
        if self.src.get(j) == Some(&b'=') {
            let matcher = self.text[i..=j].to_string();
            self.index = j + 1;
            Some(matcher)
        } else {
            None
        }
    }

    /// `REGEX_ATTRIBUTE_FLAGS = /[a-zA-Z]+/y` — the flags run, when present.
    fn read_attribute_flags(&mut self) -> Option<String> {
        let start = self.index;
        while self.index < self.len() && self.src[self.index].is_ascii_alphabetic() {
            self.index += 1;
        }
        (self.index > start).then(|| self.text[start..self.index].to_string())
    }

    /// `parser.read(REGEX_COMBINATOR)` — consume `+` / `~` / `>` / `||`.
    fn read_combinator_token(&mut self) -> Option<&'static str> {
        if self.matches(b"||") {
            self.index += 2;
            Some("||")
        } else {
            match self.src.get(self.index) {
                Some(b'+') => {
                    self.index += 1;
                    Some("+")
                }
                Some(b'~') => {
                    self.index += 1;
                    Some("~")
                }
                Some(b'>') => {
                    self.index += 1;
                    Some(">")
                }
                _ => None,
            }
        }
    }

    /// `parser.match_regex(REGEX_COMBINATOR)` (non-consuming).
    fn match_combinator(&self) -> bool {
        self.matches(b"||") || matches!(self.src.get(self.index), Some(b'+' | b'~' | b'>'))
    }

    /// `REGEX_LEADING_HYPHEN_OR_DIGIT = /-?\d/y` at the current index
    /// (non-consuming).
    fn match_leading_hyphen_or_digit(&self) -> bool {
        let mut j = self.index;
        if self.src.get(j) == Some(&b'-') {
            j += 1;
        }
        matches!(self.src.get(j), Some(b) if b.is_ascii_digit())
    }

    /// `REGEX_PERCENTAGE = /\d+(\.\d+)?%/y` (non-consuming).
    fn match_percentage(&self) -> bool {
        self.percentage_len().is_some()
    }

    /// Consume a `REGEX_PERCENTAGE` match, returning its text.
    fn read_percentage(&mut self) -> String {
        let len = self.percentage_len().unwrap_or(0);
        let value = self.text[self.index..self.index + len].to_string();
        self.index += len;
        value
    }

    /// The byte length of a `\d+(\.\d+)?%` match at the current index.
    fn percentage_len(&self) -> Option<usize> {
        let mut j = self.index;
        let start = j;
        while matches!(self.src.get(j), Some(b) if b.is_ascii_digit()) {
            j += 1;
        }
        if j == start {
            return None; // need ≥1 digit
        }
        if self.src.get(j) == Some(&b'.') {
            let mut k = j + 1;
            let frac_start = k;
            while matches!(self.src.get(k), Some(b) if b.is_ascii_digit()) {
                k += 1;
            }
            if k > frac_start {
                j = k; // a fractional part requires ≥1 digit
            }
        }
        if self.src.get(j) == Some(&b'%') {
            Some(j + 1 - self.index)
        } else {
            None
        }
    }

    /// `REGEX_NTH_OF` match (non-consuming) — see
    /// [`css_reject`](crate::svelte::runtime::css_reject) for the arm-by-arm
    /// derivation of the pinned upstream regex
    /// `(even|odd|\+?(\d+|\d*n(\s*[+-]\s*\d+)?)|-\d*n(\s*\+\s*\d+))((?=\s*[,)])|\s+of\s+)`.
    fn match_nth_of(&self) -> bool {
        self.nth_of_len().is_some()
    }

    /// Consume a `REGEX_NTH_OF` match, returning its text (INCLUDING a
    /// consumed ` of ` arm, exactly as the official read does).
    fn read_nth_of(&mut self) -> String {
        let len = self.nth_of_len().unwrap_or(0);
        let value = self.text[self.index..self.index + len].to_string();
        self.index += len;
        value
    }

    /// The byte length of a `REGEX_NTH_OF` match at the current index, or
    /// `None`.
    fn nth_of_len(&self) -> Option<usize> {
        let rest = &self.src[self.index..];
        let j = if rest.starts_with(b"even") {
            4
        } else if rest.starts_with(b"odd") {
            3
        } else if rest.first() == Some(&b'-') {
            // NEGATIVE arm `-\d*n(\s*\+\s*\d+)`.
            self.nth_negative_arm_len(rest)?
        } else {
            // POSITIVE arm `\+?(\d+|\d*n(\s*[+-]\s*\d+)?)`.
            self.nth_positive_arm_len(rest)?
        };
        // Trailing alternation `((?=\s*[,)])|\s+of\s+)`, left-to-right:
        // (1) the zero-width end lookahead `\s*[,)]` (JS `\s` — Unicode).
        let k = skip_css_whitespace(rest, j);
        if matches!(rest.get(k), Some(b',' | b')')) {
            return Some(j);
        }
        // (2) the CONSUMING `\s+of\s+` arm.
        let mut m = skip_css_whitespace(rest, j);
        if m == j {
            return None; // `\s+` needs ≥1 whitespace before `of`
        }
        if !rest[m..].starts_with(b"of") {
            return None;
        }
        m += 2;
        let after_of = skip_css_whitespace(rest, m);
        if after_of == m {
            return None; // `\s+` needs ≥1 whitespace after `of`
        }
        Some(after_of)
    }

    /// The POSITIVE An+B arm `\+?(\d+|\d*n(\s*[+-]\s*\d+)?)`.
    fn nth_positive_arm_len(&self, rest: &[u8]) -> Option<usize> {
        let mut j = 0usize;
        if rest.first() == Some(&b'+') {
            j += 1;
        }
        let dstart = j;
        while matches!(rest.get(j), Some(b) if b.is_ascii_digit()) {
            j += 1;
        }
        if rest.get(j) == Some(&b'n') {
            j += 1;
            if let Some(off) = self.nth_offset_len(rest, j, true) {
                j = off;
            }
            Some(j)
        } else if j > dstart {
            Some(j)
        } else {
            None
        }
    }

    /// The NEGATIVE An+B arm `-\d*n(\s*\+\s*\d+)` (a `+` offset ONLY).
    fn nth_negative_arm_len(&self, rest: &[u8]) -> Option<usize> {
        debug_assert_eq!(rest.first(), Some(&b'-'));
        let mut j = 1usize;
        while matches!(rest.get(j), Some(b) if b.is_ascii_digit()) {
            j += 1;
        }
        if rest.get(j) != Some(&b'n') {
            return None;
        }
        j += 1;
        self.nth_offset_len(rest, j, false)
    }

    /// The byte length up to and including a `\s*<sign>\s*\d+` An+B offset
    /// starting at `from` (JS `\s` — Unicode), or `None`.
    fn nth_offset_len(&self, rest: &[u8], from: usize, plus_or_minus: bool) -> Option<usize> {
        let mut k = skip_css_whitespace(rest, from);
        let sign_ok = match rest.get(k) {
            Some(b'+') => true,
            Some(b'-') => plus_or_minus,
            _ => false,
        };
        if !sign_ok {
            return None;
        }
        k += 1;
        k = skip_css_whitespace(rest, k);
        let ds = k;
        while matches!(rest.get(k), Some(b) if b.is_ascii_digit()) {
            k += 1;
        }
        if k > ds {
            Some(k)
        } else {
            None
        }
    }

    /// `REGEX_UNICODE_SEQUENCE = /\\[0-9a-fA-F]{1,6}(\r\n|\s)?/y` at the
    /// current index — the byte length of the match (including the leading
    /// `\`) plus the decoded codepoint, or `None`.
    fn match_unicode_sequence(&self) -> Option<(usize, u32)> {
        if self.src.get(self.index) != Some(&b'\\') {
            return None;
        }
        let mut j = self.index + 1;
        let hex_start = j;
        let mut cp: u32 = 0;
        while j < self.len() && j - hex_start < 6 && self.src[j].is_ascii_hexdigit() {
            cp = cp * 16 + (self.src[j] as char).to_digit(16).unwrap_or(0);
            j += 1;
        }
        if j == hex_start {
            return None; // need ≥1 hex digit
        }
        // Optional trailing `\r\n` or a single whitespace.
        if self.src.get(j) == Some(&b'\r') && self.src.get(j + 1) == Some(&b'\n') {
            j += 2;
        } else if j < self.len() && is_css_whitespace(self.codepoint_at(j)) {
            j += char_len(self.src[j]);
        }
        Some((j - self.index, cp))
    }
}

/// The JS `String.prototype.trim()` set — identical to the JS `\s` set
/// (WhiteSpace ∪ LineTerminator): INCLUDES U+FEFF, EXCLUDES U+0085. Rust
/// `str::trim` (Unicode `White_Space`) diverges on exactly those two, so the
/// official `value.trim()` calls route here.
fn trim_js_whitespace(s: &str) -> &str {
    s.trim_matches(|c: char| is_css_whitespace(c as u32))
}

/// Advance `k` past the JS-`\s` whitespace run in `rest` (a valid-UTF-8
/// suffix of the source; `k` on a char boundary), decoding codepoints — the
/// `\s*` / `\s+` scans of `REGEX_NTH_OF` are Unicode-aware.
fn skip_css_whitespace(rest: &[u8], mut k: usize) -> usize {
    while let Some((cp, len)) = codepoint_with_len(rest, k) {
        if !is_css_whitespace(cp) {
            break;
        }
        k += len;
    }
    k
}

/// Decode the UTF-8 codepoint starting at byte `i` of `bytes`, returning
/// `(codepoint, byte length)` — `None` at EOF or on undecodable bytes (a
/// mid-character `i`; never produced by the boundary-preserving scans).
fn codepoint_with_len(bytes: &[u8], i: usize) -> Option<(u32, usize)> {
    let &lead = bytes.get(i)?;
    if lead < 0x80 {
        return Some((u32::from(lead), 1));
    }
    let slice = bytes.get(i..i + char_len(lead))?;
    let c = std::str::from_utf8(slice).ok()?.chars().next()?;
    Some((c as u32, c.len_utf8()))
}

/// Whether `cp` is in the official parser's whitespace set (`is_whitespace`
/// in `phases/1-parse/index.js`).
fn is_css_whitespace(cp: u32) -> bool {
    if cp == 32 || (9..=13).contains(&cp) {
        return true;
    }
    if cp < 160 {
        return false;
    }
    matches!(cp, 160 | 5760 | 8232 | 8233 | 8239 | 8287 | 12288 | 65279)
        || (8192..=8202).contains(&cp)
}

/// `REGEX_VALID_IDENTIFIER_CHAR = /[a-zA-Z0-9_-]/` for an ASCII byte.
fn is_valid_identifier_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// The UTF-8 byte length of the char whose leading byte is `b` (1 for ASCII).
fn char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >= 0xF0 {
        4
    } else if b >= 0xE0 {
        3
    } else if b >= 0xC0 {
        2
    } else {
        1 // a stray continuation byte — advance one to make progress
    }
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod parse_tests;
