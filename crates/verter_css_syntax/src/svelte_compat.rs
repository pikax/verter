//! Svelte compatibility validation profile — a VALIDATION-ONLY reproduction of the official
//! `svelte@5.56.10` CSS body reader's PARSE control flow (`phases/1-parse/read/style.js` + the
//! `Parser` byte primitives in `phases/1-parse/index.js`).
//!
//! This is not the crate's general CSS Syntax Module Level 3 grammar: `svelte@5.56.10` hand-rolls
//! its own CSS reader with its own GRAMMAR-ORDER control flow (which production is attempted in
//! what order, matching upstream's exact reader sequence and error priority) and its own
//! error-code taxonomy. The reader's TOKEN-LEVEL scanning runs over [`crate::lexer::Lexer`]:
//! whitespace runs, comments, and identifiers delegate to its scanning methods (parameterized for
//! the two genuine grammar deltas upstream's regexes need: JS `\s` (Unicode-aware) whitespace
//! instead of the CSS Syntax Module's ASCII-only set, and a `>= 160` identifier-char codepoint
//! threshold instead of the general `>= 128` rule — see [`crate::lexer::WhitespaceProfile`] /
//! [`crate::lexer::IdentifierProfile`]); and every structural/punctuation decision that
//! corresponds to one of the lexer's own one-byte `TokenKind`s under this reader's fixed
//! `CssDialect::Css` dialect — brace, comma, colon, semicolon, paren, bracket — is read off the
//! lexer's TOKEN STREAM (`at_token` / `eat_token` / `eat_double_colon`, peeking or consuming via
//! a cloned lexer probe), not an independent byte comparison. What stays bespoke here is the
//! CONTROL FLOW upstream's `read/style.js` implements that has no shared-lexer equivalent at all,
//! plus a small, individually-justified set of byte-level reads a real CSS token cannot represent
//! without changing observable behavior:
//!
//! - The whole-component envelope and body-loop stop condition (parsing begins at
//!   `content_start`, the body loop stops at the literal `</style` or true EOF).
//! - READ-AHEAD-THEN-REWIND grammar-ORDER decisions specific to this reader (e.g. "is this a
//!   nested rule or a declaration", the exact production sequence, the exact error-code
//!   taxonomy / first-failure priority).
//! - `read_value` / `read_attribute_value`: upstream's own raw-run-with-backslash-escapes value
//!   readers. These are NOT a CSS Syntax Module string token in disguise — unlike a spec string
//!   token, neither bails out on an embedded raw newline (upstream's `BadString`-on-newline rule
//!   has no analogue here; a quoted CSS value or attribute value can itself contain a literal
//!   newline and upstream keeps scanning to the matching quote or EOF), and their backslash
//!   handling is a plain "escape whatever follows" rule, not the CSS hex-escape grammar
//!   `read_identifier` uses. Reusing [`crate::lexer::Lexer::consume_string`] here would silently
//!   change upstream-observable behavior on an unterminated quoted value, so these stay bespoke
//!   readers; the `/* … */` comment sub-scan inside `read_value` still delegates to
//!   [`crate::lexer::Lexer::consume_comment`].
//! - The `nth-of` (`An+B`) regex-shape matcher and the percentage matcher: both are narrower,
//!   differently-shaped grammars than the general lexer's number/dimension tokenizing (no sign,
//!   no exponent, no unit), so a general `consume_number` pass would over-accept (e.g. it would
//!   tokenize `5e2%` as one `Percentage` spanning `5e2%`, where upstream's own
//!   `\d+(\.\d+)?%` regex fails to match at all past the leading `5`). These stay dedicated
//!   grammar routines, built on the shared digit-run/whitespace/codepoint primitives
//!   ([`crate::lexer::ascii_digits_len`], [`crate::lexer::is_js_whitespace_codepoint`],
//!   [`crate::lexer::codepoint_at`]) rather than a second independently-enumerated char class.
//! - `@` production dispatch (`read_body`'s `parser.match('@')`) stays a raw byte comparison: the
//!   shared lexer's `AtKeyword` token only forms when `@` is immediately followed by an
//!   identifier-start byte, so a token-kind peek would silently diverge from upstream's
//!   unconditional `matches(b"@")` on a bare/malformed `@` (e.g. `@ {`) — it would read as "no
//!   at-rule" and fall into `read_rule` instead of upstream's own `read_at_rule` failure path.
//! - The selector-grammar delimiter characters `&` (nesting), `*` (universal), `.` (class prefix),
//!   and `|` (namespace separator): the shared lexer tokenizes every one of these as the same
//!   undifferentiated `Delim` kind (there is no dedicated `TokenKind` per delimiter character), so
//!   distinguishing them still requires reading the underlying byte — a `TokenKind::Delim` peek
//!   would add a layer of indirection without replacing the byte comparison it is built on. These
//!   stay direct `eat(byte, …)` calls. `#` (id prefix) DOES have a dedicated `TokenKind::Hash`, so
//!   `read_selector`'s id-selector arm peeks it instead.
//! - The `<!-- … -->` PAIRING-AND-SWALLOWING control flow (how it composes with `/* … */` in the
//!   same `allow_comment_or_whitespace` loop, and swallowing everything between the opener and
//!   terminator): this has no shared-lexer equivalent and stays bespoke. The opener/terminator
//!   RECOGNITION itself is driven by the shared lexer's dedicated `TokenKind::Cdo`/`TokenKind::Cdc`
//!   tokens.
//! - `read_body`'s `</style` / EOF body-loop finish predicate: not a CSS token at all (a
//!   Svelte-specific whole-component terminator), so it stays a literal string match.
//!
//! It is hosted here — as the ONE crate that owns all CSS-family parsing/scanning production
//! code — specifically so no second, independently-maintained CSS reader lives outside this
//! crate.
//!
//! ## Why this exists
//!
//! Upstream's `element.js` calls `read_style` — which PARSES the `<style>` CSS body via a full
//! CSS reader and can THROW a parse error — BEFORE `if (current.css) e.style_duplicate(start)`.
//! So a malformed CSS body in the 2nd `<style>` wins the first-error race over `style_duplicate`,
//! and a consumer reproducing `svelte@5.56.10`'s diagnostics must report the EXACT upstream CSS
//! parse code for that race. Verter's Svelte official-reject gate
//! (`verter_compiler::svelte::runtime::official_reject`) reserves a probe at the `read_style`
//! position; [`style_body_reject_code`] is what fills that slot.
//!
//! Scope: this is the upstream `read_style` PARSE-ENTRY control flow ONLY (per J1-A16's ruling
//! in `docs/arch/refactor/rev11/evidence/J1/css-family-authority-inventory-gap.md` — "Preserve
//! the unusual whole-component envelope and nested-reader behavior... move its grammar corpus
//! into `verter_css_syntax`"; that ruling explicitly found no durable prior "(codex-ruled)"
//! exception on file for this module's predecessor, so this scope note is stated on its own
//! merits, not as a claimed prior authority) — enough
//! to return the FIRST exact parse code reachable from `read/style.js`'s readers
//! (`css_expected_identifier`, `css_empty_declaration`, `css_selector_invalid`, plus the generic
//! `expected_token` / `unexpected_eof` the `Parser` primitives throw). It builds NO CSS AST and
//! performs NO CSS analysis / scoping — the post-parse CSS validation family (`css_global_*`,
//! nesting placement, …) is a deferred CSS-scoping vertical (see the debt-ledger entry in
//! `docs/arch/svelte-native-compiler-plan.md`). A body that parses CLEAN here returns `None`, so
//! the caller's later `style_duplicate` (or its unsupported-`<style>` rail) wins.
//!
//! The reader operates on the WHOLE component `source` from the CSS body's `content_start` and
//! stops the body loop at the literal `</style` or EOF (upstream's `finished` predicate) — it is
//! NOT given an isolated body slice, because upstream's nested CSS readers (a `read_block` /
//! `read_value` inside an unterminated rule) run PAST `</style>` into the rest of the source, and
//! that emergent behaviour decides the exact code.

use std::sync::Arc;

use crate::dialect::CssDialect;
use crate::lexer::{
    ascii_digits_len, codepoint_at, is_js_whitespace_codepoint, IdentifierProfile, Lexer,
    WhitespaceProfile,
};
use crate::parser::CssSource;
use crate::token::{SyntaxToken, TokenFlags, TokenKind};

/// The result of one CSS reader step: `Ok(())` when the step parsed cleanly, `Err(code)` carrying
/// the EXACT upstream CSS parse code the step would `throw`.
type CssResult = Result<(), &'static str>;

/// Parse the CSS body that begins at `content_start` in `source` exactly as upstream's
/// `read_style` → `read_body(parser, p => p.match('</style') || p.index >= p.template.length)`
/// does, returning the FIRST exact CSS parse code on a body-parse FAILURE, or `None` when the
/// body parses cleanly (so the caller's later `style_duplicate` / unsupported-`<style>` rail
/// wins).
#[must_use]
pub fn style_body_reject_code(source: &str, content_start: usize) -> Option<&'static str> {
    let css_source = CssSource::new(Arc::from(source), 0).ok()?;
    let mut lexer = Lexer::new(&css_source, CssDialect::Css);
    let start = content_start.min(css_source.text().len());
    lexer.seek(start);
    let mut p = CssParser {
        src: css_source.text().as_bytes(),
        text: css_source.text(),
        lexer,
    };
    p.read_body().err()
}

/// The lexer-backed cursor + the upstream `Parser` primitives the CSS readers use. A faithful but
/// validation-only mirror of the official `Parser` (`phases/1-parse/index.js`): the `eat(str,
/// required=true)` form throws `expected_token`; `read_until` throws `unexpected_eof` at EOF; the
/// rest are non-throwing scans. Token-level scanning (whitespace runs, `/* … */` comments,
/// identifiers) delegates to `lexer`; the cursor position IS `lexer`'s own cursor — there is no
/// separate index to keep in sync.
struct CssParser<'a> {
    src: &'a [u8],
    text: &'a str,
    lexer: Lexer<'a>,
}

impl<'a> CssParser<'a> {
    /// The parser's current byte position — `lexer`'s own cursor (this `Lexer` was constructed
    /// over the whole component source with origin 0, so its absolute position IS this local
    /// index).
    fn index(&self) -> usize {
        self.lexer.position() as usize
    }

    /// Reposition the cursor — a thin wrapper over [`Lexer::seek`] so every read/rewind site in
    /// this file reads as "the parser's position", not "the lexer's".
    fn seek(&mut self, at: usize) {
        self.lexer.seek(at);
    }

    fn len(&self) -> usize {
        self.src.len()
    }

    fn at_eof(&self) -> bool {
        self.index() >= self.len()
    }

    /// `parser.match(str)` — whether `str` occurs at the current index.
    fn matches(&self, s: &[u8]) -> bool {
        self.src[self.index()..].starts_with(s)
    }

    /// `parser.eat(str, required)` — consume `str` if present (returning `true`); when `required`
    /// and absent, throw `expected_token`.
    fn eat(&mut self, s: &[u8], required: bool) -> Result<bool, &'static str> {
        if self.matches(s) {
            self.seek(self.index() + s.len());
            Ok(true)
        } else if required {
            Err("expected_token")
        } else {
            Ok(false)
        }
    }

    /// Peek the [`TokenKind`] of the token that begins exactly at the current position, via the
    /// shared lexer's real token stream — a clone so the probe never advances the parser's own
    /// cursor. `None` at EOF.
    fn peek_kind(&self) -> Option<TokenKind> {
        self.lexer.clone().next().map(SyntaxToken::kind)
    }

    /// Whether the token at the current position is exactly `kind` — non-consuming.
    fn at_token(&self, kind: TokenKind) -> bool {
        self.peek_kind() == Some(kind)
    }

    /// `parser.eat(<punctuation>, required)` reproduced over the shared lexer's TOKEN STREAM for
    /// a punctuation kind the lexer already tokenizes as its own one-byte [`TokenKind`] under
    /// this reader's fixed [`CssDialect::Css`] dialect (brace / comma / colon / semicolon / paren
    /// / bracket — every one of these is an unconditional single-byte token under `Css`, unlike
    /// e.g. `{` under the Stylus dialect): consume the token if present (returning `true`); when
    /// `required` and absent, throw `expected_token`.
    fn eat_token(&mut self, kind: TokenKind, required: bool) -> Result<bool, &'static str> {
        let mut probe = self.lexer.clone();
        match probe.next() {
            Some(token) if token.kind() == kind => {
                self.lexer = probe;
                Ok(true)
            }
            _ if required => Err("expected_token"),
            _ => Ok(false),
        }
    }

    /// `parser.eat('::', required=false)` reproduced over the token stream as two zero-gap
    /// adjacent `Colon` tokens — the CSS Syntax Module has no fused "double colon" token; `::` is
    /// simply two back-to-back `:` delimiter tokens with no byte between them.
    fn eat_double_colon(&mut self) -> bool {
        let mut probe = self.lexer.clone();
        let Some(first) = probe.next() else {
            return false;
        };
        if first.kind() != TokenKind::Colon {
            return false;
        }
        let mut probe2 = probe.clone();
        let Some(second) = probe2.next() else {
            return false;
        };
        if second.kind() == TokenKind::Colon && second.start == first.end {
            self.lexer = probe2;
            true
        } else {
            false
        }
    }

    /// `parser.allow_whitespace()` — skip a run of upstream's JS-`\s` whitespace, via the shared
    /// lexer's [`WhitespaceProfile::JsUnicode`] scan.
    fn allow_whitespace(&mut self) {
        self.lexer
            .consume_whitespace_profiled(WhitespaceProfile::JsUnicode);
    }

    /// `parser.read_until(pattern)` for a single-CODEPOINT-class pattern (the JS-regex classes are
    /// Unicode-aware — `\s` includes NBSP and the other Unicode spaces); at EOF (upstream's
    /// non-loose branch) throw `unexpected_eof`. Used for `REGEX_WHITESPACE_OR_COLON` in
    /// `read_declaration` — this is NOT an identifier-grammar read (upstream reads an arbitrary
    /// non-whitespace-non-colon run as a "property name" with no char-class validation at all),
    /// so it has no shared-lexer token equivalent; only its whitespace CLASS PREDICATE is shared.
    fn read_until_codepoint_class(
        &mut self,
        pred: impl Fn(u32) -> bool,
    ) -> Result<&'a str, &'static str> {
        if self.at_eof() {
            return Err("unexpected_eof");
        }
        let start = self.index();
        while let Some((cp, len)) = codepoint_at(self.src, self.index()) {
            if pred(cp) {
                break;
            }
            self.seek(self.index() + len);
        }
        Ok(&self.text[start..self.index()])
    }

    /// The Unicode codepoint at byte `i` (decoding the UTF-8 char). Mirrors upstream's
    /// `template.codePointAt(index)` for `read_attribute_value`'s unquoted-close whitespace test.
    fn codepoint_at(&self, i: usize) -> u32 {
        self.text[i..].chars().next().map_or(0, |c| c as u32)
    }

    /// Append the WHOLE char at the current index to `out` and advance past it (by its UTF-8 byte
    /// length) — the readers step whole scalars, never single bytes, so `codepoint_at` never lands
    /// on a continuation byte and the accumulated value keeps valid UTF-8.
    fn push_char(&mut self, out: &mut String) {
        if let Some(c) = self.text[self.index()..].chars().next() {
            out.push(c);
            self.seek(self.index() + c.len_utf8());
        } else {
            self.seek(self.index() + 1);
        }
    }

    /// Advance past the WHOLE char at the current index without accumulating it (the validation
    /// readers that do not build a value still step whole scalars).
    fn advance_char(&mut self) {
        let len = self
            .text
            .get(self.index()..)
            .and_then(|s| s.chars().next())
            .map_or(1, char::len_utf8);
        self.seek(self.index() + len);
    }

    /// `parser.match('</style' …)` body-loop finish predicate: at the literal `</style` or EOF.
    fn body_finished(&self) -> bool {
        self.matches(b"</style") || self.at_eof()
    }

    // ── read/style.js readers (validation-only) ──────────────────────────────────────────

    /// `read_body(parser, finished)` — the `read_style` entry. Loops rules / at-rules until the
    /// `</style`-or-EOF finish predicate, after `allow_comment_or_whitespace` each turn.
    fn read_body(&mut self) -> CssResult {
        loop {
            self.allow_comment_or_whitespace()?;
            if self.body_finished() {
                return Ok(());
            }
            if self.matches(b"@") {
                self.read_at_rule()?;
            } else {
                self.read_rule()?;
            }
        }
    }

    /// `read_at_rule(parser)`.
    fn read_at_rule(&mut self) -> CssResult {
        self.eat(b"@", true)?;
        self.read_identifier()?;
        self.read_value()?;
        if self.at_token(TokenKind::LeftBrace) {
            self.read_block()?;
        } else {
            self.eat_token(TokenKind::Semicolon, true)?;
        }
        Ok(())
    }

    /// `read_rule(parser)` — a selector list then a block.
    fn read_rule(&mut self) -> CssResult {
        self.read_selector_list(false)?;
        self.read_block()?;
        Ok(())
    }

    /// `read_selector_list(parser, inside_pseudo_class)`.
    fn read_selector_list(&mut self, inside_pseudo_class: bool) -> CssResult {
        self.allow_comment_or_whitespace()?;
        while self.index() < self.len() {
            self.read_selector(inside_pseudo_class)?;
            self.allow_comment_or_whitespace()?;
            let closes = if inside_pseudo_class {
                self.at_token(TokenKind::RightParen)
            } else {
                self.at_token(TokenKind::LeftBrace)
            };
            if closes {
                return Ok(());
            }
            self.eat_token(TokenKind::Comma, true)?;
            self.allow_comment_or_whitespace()?;
        }
        Err("unexpected_eof")
    }

    /// `read_selector(parser, inside_pseudo_class)` — the per-selector loop. Faithful to the
    /// upstream branch order; the only THROWING paths are `read_identifier` (type / class / id /
    /// pseudo names → `css_expected_identifier`), the `[` attribute selector's `eat(']', true)`,
    /// and the post-combinator `css_selector_invalid` when a `,`/`{`/`)` immediately follows a
    /// combinator.
    fn read_selector(&mut self, inside_pseudo_class: bool) -> CssResult {
        loop {
            if self.eat(b"&", false)? {
                // NestingSelector — no name read.
            } else if self.eat(b"*", false)? {
                if self.eat(b"|", false)? {
                    self.read_identifier()?;
                }
            } else if self.at_token(TokenKind::Hash) {
                // IdSelector (`#x`). The shared lexer's `Hash` token only forms when `#` is
                // followed by a valid name sequence, unlike upstream's unconditional
                // `eat('#', false)` — but every case that gates out (Hash doesn't form) is a case
                // where `read_identifier`'s own scan (which shares the same name-char predicate)
                // would consume zero characters starting right after `#` too, so it still throws
                // the same `css_expected_identifier` either way; a raw fallback is not needed.
                // Only the `#` prefix itself is consumed here (not the whole `Hash` token span),
                // so `read_identifier` still runs from upstream's own position and keeps applying
                // its own leading-digit/hyphen rejection.
                self.eat(b"#", true)?;
                self.read_identifier()?;
            } else if self.eat(b".", false)? {
                // ClassSelector (`.x`) — no dedicated token for `.`, stays a raw byte comparison.
                self.read_identifier()?;
            } else if self.eat_double_colon() || self.eat_token(TokenKind::Colon, false)? {
                // PseudoElementSelector (`::x`) / PseudoClassSelector (`:x`) — `::` is checked
                // FIRST (the `||` short-circuit preserves upstream's branch order so a `:` does not
                // eat the first colon of `::`); both read an identifier + an optional `(args)`.
                self.read_identifier()?;
                if self.eat_token(TokenKind::LeftParen, false)? {
                    self.read_selector_list(true)?;
                    self.eat_token(TokenKind::RightParen, true)?;
                }
            } else if self.eat_token(TokenKind::LeftBracket, false)? {
                self.allow_whitespace();
                self.read_identifier()?;
                self.allow_whitespace();
                if self.read_matcher() {
                    self.allow_whitespace();
                    self.read_attribute_value()?;
                }
                self.allow_whitespace();
                self.read_attribute_flags();
                self.allow_whitespace();
                self.eat_token(TokenKind::RightBracket, true)?;
            } else if inside_pseudo_class && self.match_nth_of() {
                self.read_nth_of();
            } else if self.match_percentage() {
                self.read_percentage();
            } else if !self.match_combinator() {
                self.read_identifier()?;
                if self.eat(b"|", false)? {
                    self.read_identifier()?;
                }
            }

            let index = self.index();
            self.allow_comment_or_whitespace()?;

            let closes = self.at_token(TokenKind::Comma)
                || if inside_pseudo_class {
                    self.at_token(TokenKind::RightParen)
                } else {
                    self.at_token(TokenKind::LeftBrace)
                };
            if closes {
                // rewind, return the complex selector.
                self.seek(index);
                return Ok(());
            }

            self.seek(index);
            let had_combinator = self.read_combinator();
            if had_combinator {
                self.allow_whitespace();
                let closes_after = self.at_token(TokenKind::Comma)
                    || if inside_pseudo_class {
                        self.at_token(TokenKind::RightParen)
                    } else {
                        self.at_token(TokenKind::LeftBrace)
                    };
                if closes_after {
                    return Err("css_selector_invalid");
                }
            }
        }
    }

    /// `read_combinator(parser)` — returns whether a combinator (an explicit `+`/`~`/`>`/`||`, or
    /// a whitespace-only descendant combinator) was read. Non-throwing.
    fn read_combinator(&mut self) -> bool {
        let start = self.index();
        self.allow_whitespace();
        if self.read_combinator_token() {
            self.allow_whitespace();
            return true;
        }
        // a whitespace-only descendant combinator: whitespace was consumed (index advanced).
        self.index() != start
    }

    /// `read_block(parser)` — `{` … block items … `}`. Throws `expected_token` on a missing brace.
    fn read_block(&mut self) -> CssResult {
        self.eat_token(TokenKind::LeftBrace, true)?;
        while self.index() < self.len() {
            self.allow_comment_or_whitespace()?;
            if self.at_token(TokenKind::RightBrace) {
                break;
            }
            self.read_block_item()?;
        }
        self.eat_token(TokenKind::RightBrace, true)?;
        Ok(())
    }

    /// `read_block_item(parser)` — disambiguate a nested rule (look-ahead sees a `{` after the
    /// value) from a declaration. Mirrors upstream's read-ahead-then-rewind.
    fn read_block_item(&mut self) -> CssResult {
        if self.matches(b"@") {
            return self.read_at_rule();
        }
        let start = self.index();
        self.read_value()?;
        let is_rule = self.at_token(TokenKind::LeftBrace);
        self.seek(start);
        if is_rule {
            self.read_rule()
        } else {
            self.read_declaration()
        }
    }

    /// `read_declaration(parser)` — a property, `:`, a value; an empty non-`--` declaration is
    /// `css_empty_declaration`; a non-`}`-terminated declaration must `eat(';', true)`.
    fn read_declaration(&mut self) -> CssResult {
        // `REGEX_WHITESPACE_OR_COLON = /[\s:]/` — JS `\s` is the Unicode set.
        let property = self.read_until_codepoint_class(|cp| {
            is_js_whitespace_codepoint(cp) || cp == u32::from(b':')
        })?;
        self.allow_whitespace();
        self.eat_token(TokenKind::Colon, false)?;
        self.allow_whitespace();
        let value = self.read_value()?;
        if value.is_empty() && !property.starts_with("--") {
            return Err("css_empty_declaration");
        }
        if !self.at_token(TokenKind::RightBrace) {
            self.eat_token(TokenKind::Semicolon, true)?;
        }
        Ok(())
    }

    /// `read_value(parser)` — read up to an unquoted/unparen `;` / `{` / `}` (returning the
    /// trimmed value text), skipping `/* … */` comments and respecting quotes + `url(...)`. At
    /// EOF it throws `unexpected_eof`.
    ///
    /// This is upstream's own raw-run-with-backslash-escapes reader, not a CSS Syntax Module
    /// string token in disguise: inside a quoted span it does NOT bail out on an embedded raw
    /// newline (there is no `BadString`-on-newline rule here — see the module doc), so it cannot
    /// be replaced by [`Lexer::consume_string`] without changing observable behavior. The `/* …
    /// */` comment sub-scan, however, has no such divergence and delegates to
    /// [`Lexer::consume_comment`].
    fn read_value(&mut self) -> Result<String, &'static str> {
        let mut value = String::new();
        let mut escaped = false;
        let mut in_url = false;
        let mut quote_mark: Option<u8> = None;
        while self.index() < self.len() {
            let ch = self.src[self.index()];
            if escaped {
                value.push('\\');
                self.push_char(&mut value);
                escaped = false;
                continue;
            } else if ch == b'\\' {
                escaped = true;
                self.seek(self.index() + 1);
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
                && self.src.get(self.index() + 1) == Some(&b'*')
            {
                // Upstream's inline comment-skip has no REQUIRED closing-token check (unlike
                // `allow_comment_or_whitespace`'s `eat('*/', true)`): an unterminated comment just
                // runs to EOF, and the OUTER loop's own EOF check below is what yields
                // `unexpected_eof` — exactly what `Lexer::consume_comment`'s `UNTERMINATED`
                // handling already does (advance to end-of-input, no throw), so its return value
                // needs no inspection here.
                self.lexer.consume_comment();
                continue;
            }
            self.push_char(&mut value);
        }
        Err("unexpected_eof")
    }

    /// `read_attribute_value(parser)` — a quoted or unquoted attribute value, closing on the
    /// matching quote or (unquoted) a whitespace / `]`. At EOF it throws `unexpected_eof`.
    ///
    /// Like `read_value`, this does not implement the CSS Syntax Module's string-token grammar: a
    /// quoted attribute value here does not bail out on an embedded raw newline either, so it
    /// stays a bespoke reader rather than a [`Lexer::consume_string`] call (see the module doc).
    fn read_attribute_value(&mut self) -> CssResult {
        let mut escaped = false;
        let quote_mark = if self.eat(b"\"", false)? {
            Some(b'"')
        } else if self.eat(b"'", false)? {
            Some(b'\'')
        } else {
            None
        };
        while self.index() < self.len() {
            let ch = self.src[self.index()];
            if escaped {
                escaped = false;
                self.advance_char();
                continue;
            } else if ch == b'\\' {
                escaped = true;
                self.seek(self.index() + 1); // `\` is ASCII (1 byte)
                continue;
            }
            // `REGEX_CLOSING_BRACKET = /[\s\]]/` — JS `\s` is the Unicode set; the reader steps
            // whole chars so `codepoint_at` is only ever read on a char boundary.
            let closes = match quote_mark {
                Some(q) => ch == q,
                None => is_js_whitespace_codepoint(self.codepoint_at(self.index())) || ch == b']',
            };
            if closes {
                if let Some(q) = quote_mark {
                    self.eat(&[q], true)?;
                }
                return Ok(());
            }
            self.advance_char();
        }
        Err("unexpected_eof")
    }

    /// `read_identifier(parser)` (the CSS one) — a CSS ident token. A leading `-?<digit>` is
    /// `css_expected_identifier`; an EMPTY identifier is `css_expected_identifier`. Delegates its
    /// char-classification loop (escape handling, ident chars including upstream's `>= 160`
    /// codepoint threshold) to the shared lexer's `SvelteCompat`/`JsUnicode`-profiled name scan.
    fn read_identifier(&mut self) -> CssResult {
        // REGEX_LEADING_HYPHEN_OR_DIGIT = /-?\d/y at the current index.
        if self.match_leading_hyphen_or_digit() {
            return Err("css_expected_identifier");
        }
        let start = self.index();
        self.lexer.consume_name_profiled(
            IdentifierProfile::SvelteCompat,
            WhitespaceProfile::JsUnicode,
        );
        if self.index() == start {
            return Err("css_expected_identifier");
        }
        Ok(())
    }

    /// `allow_comment_or_whitespace(parser)` — whitespace then any run of `/* … */` / `<!-- … -->`
    /// comments. Upstream's REQUIRED close tokens (`eat('*/', true)` / `eat('-->', true)`) raise
    /// `expected_token` on an unterminated comment: the `/* … */` arm delegates to
    /// [`Lexer::consume_comment`] and turns its `UNTERMINATED` flag into that same error; the
    /// `<!-- … -->` opener/terminator are the shared lexer's dedicated `TokenKind::Cdo`/
    /// `TokenKind::Cdc` tokens (an exact-literal match with no extra requirement, so peeking them
    /// is unconditionally equivalent to the raw byte match it replaces), driven through the same
    /// `at_token`/`eat_token` helpers as the rest of this file; only the swallow-everything-between
    /// scan (`read_until_str`) and its pairing with the `/* … */` arm in this same loop have no
    /// shared-lexer equivalent and stay bespoke.
    fn allow_comment_or_whitespace(&mut self) -> CssResult {
        self.allow_whitespace();
        while self.matches(b"/*") || self.at_token(TokenKind::Cdo) {
            if self.matches(b"/*") {
                let comment = self.lexer.consume_comment();
                if comment.flags & TokenFlags::UNTERMINATED != 0 {
                    return Err("expected_token");
                }
            }
            if self.at_token(TokenKind::Cdo) {
                self.eat_token(TokenKind::Cdo, true)?;
                self.read_until_str(b"-->");
                self.eat_token(TokenKind::Cdc, true)?;
            }
            self.allow_whitespace();
        }
        Ok(())
    }

    // ── scan helpers (the regex primitives, hand-coded) ──────────────────────────────────

    /// Advance to the first occurrence of `needle` (or EOF), like `parser.read_until(/needle/)`.
    /// Used only to swallow the run between a `<!--` opener and its `-->` terminator, which has
    /// no shared-lexer token equivalent (see above) even though the opener/terminator themselves
    /// do.
    fn read_until_str(&mut self, needle: &[u8]) {
        while self.index() < self.len() && !self.matches(needle) {
            self.seek(self.index() + 1);
        }
    }

    /// `REGEX_MATCHER = /[~^$*|]?=/y` — consume an optional `~^$*|` then a required `=`. Returns
    /// whether a matcher was read (advancing only on a full match).
    fn read_matcher(&mut self) -> bool {
        let i = self.index();
        let mut j = i;
        if let Some(&b) = self.src.get(j) {
            if matches!(b, b'~' | b'^' | b'$' | b'*' | b'|') {
                j += 1;
            }
        }
        if self.src.get(j) == Some(&b'=') {
            self.seek(j + 1);
            true
        } else {
            false
        }
    }

    /// `REGEX_ATTRIBUTE_FLAGS = /[a-zA-Z]+/y` — consume a run of ASCII letters.
    fn read_attribute_flags(&mut self) {
        while self.index() < self.len() && self.src[self.index()].is_ascii_alphabetic() {
            self.seek(self.index() + 1);
        }
    }

    /// `parser.read(REGEX_COMBINATOR)` — consume `+` / `~` / `>` / `||` at the current index.
    fn read_combinator_token(&mut self) -> bool {
        if self.matches(b"||") {
            self.seek(self.index() + 2);
            true
        } else if matches!(self.src.get(self.index()), Some(b'+' | b'~' | b'>')) {
            self.seek(self.index() + 1);
            true
        } else {
            false
        }
    }

    /// `parser.match_regex(REGEX_COMBINATOR)` — whether a combinator token is at the current index
    /// (non-consuming).
    fn match_combinator(&self) -> bool {
        self.matches(b"||") || matches!(self.src.get(self.index()), Some(b'+' | b'~' | b'>'))
    }

    /// `REGEX_LEADING_HYPHEN_OR_DIGIT = /-?\d/y` at the current index (non-consuming).
    fn match_leading_hyphen_or_digit(&self) -> bool {
        let mut j = self.index();
        if self.src.get(j) == Some(&b'-') {
            j += 1;
        }
        matches!(self.src.get(j), Some(b) if b.is_ascii_digit())
    }

    /// `REGEX_PERCENTAGE = /\d+(\.\d+)?%/y` at the current index (non-consuming). See the module
    /// doc for why this stays a dedicated grammar routine rather than a general
    /// `Lexer::consume_number` call (that grammar additionally allows a sign and an exponent,
    /// which this narrower upstream regex does not).
    fn match_percentage(&self) -> bool {
        self.percentage_len().is_some()
    }

    /// Consume a `REGEX_PERCENTAGE` match.
    fn read_percentage(&mut self) {
        if let Some(len) = self.percentage_len() {
            self.seek(self.index() + len);
        }
    }

    /// The byte length of a `\d+(\.\d+)?%` match at the current index, or `None`. The digit-run
    /// scans are the same shared primitive ([`crate::lexer::ascii_digits_len`]) the general
    /// lexer's number/dimension tokenizing uses.
    fn percentage_len(&self) -> Option<usize> {
        let start = self.index();
        let mut j = start + ascii_digits_len(self.src, start);
        if j == start {
            return None; // need ≥1 digit
        }
        if self.src.get(j) == Some(&b'.') {
            let frac_start = j + 1;
            let frac_len = ascii_digits_len(self.src, frac_start);
            if frac_len > 0 {
                j = frac_start + frac_len; // a fractional part requires ≥1 digit
            }
        }
        if self.src.get(j) == Some(&b'%') {
            Some(j + 1 - self.index())
        } else {
            None
        }
    }

    /// `REGEX_NTH_OF` match (non-consuming). The nth-of grammar is matched ONLY inside a pseudo
    /// class; this models the accepted-prefix shape of the common forms (`even` / `odd` / an
    /// optional-signed `An+B`) followed by EITHER the `\s*[,)]` end lookahead OR the `\s+of\s+`
    /// arm. The full grammar (`<An+B> of <complex-selector-list>`) recurses through
    /// `read_selector_list`, but the `of`-arm here matches `\s+of\s+` faithfully so a clean
    /// `:nth-child(<An+B> of <selector>)` PARSES (the `<selector>` after `of` reads through the
    /// normal selector loop), exactly as upstream `REGEX_NTH_OF` (`read/style.js`) does.
    fn match_nth_of(&self) -> bool {
        self.nth_of_len().is_some()
    }

    /// Consume a `REGEX_NTH_OF` match.
    fn read_nth_of(&mut self) {
        if let Some(len) = self.nth_of_len() {
            self.seek(self.index() + len);
        }
    }

    /// The byte length of a `REGEX_NTH_OF` match at the current index, or `None`. Models
    /// `(even|odd|\+?(\d+|\d*n(\s*[+-]\s*\d+)?)|-\d*n(\s*\+\s*\d+))((?=\s*[,)])|\s+of\s+)`: the
    /// leading An+B alternation, then upstream's trailing alternation — the `\s*[,)]` end
    /// LOOKAHEAD (zero-width, the selector-list / pseudo-args terminator) OR the CONSUMING
    /// `\s+of\s+` arm (so `2n+1 of .x` matches up to and including the ` of ` and the `.x`
    /// selector reads through the normal loop). The `of` arm is tried only when the lookahead
    /// fails — matching the regex alternation's left-to-right order.
    ///
    /// The An+B alternation has THREE faithfully-distinct arms (upstream's `even` / `odd` /
    /// positive `\+?(...)` / negative `-\d*n(...)`):
    /// - `even` / `odd` keywords.
    /// - POSITIVE `\+?(\d+|\d*n(\s*[+-]\s*\d+)?)` — an OPTIONAL leading `+`, then EITHER a plain
    ///   `\d+` (no `n`) OR `\d*n` (optional digits, `n`) with an OPTIONAL `±` offset.
    /// - NEGATIVE `-\d*n(\s*\+\s*\d+)` — a MANDATORY leading `-`, then `\d*n` (optional digits,
    ///   `n`), then a MANDATORY `\s*\+\s*\d+` offset (a `+` offset ONLY — `-` is not in this arm).
    ///
    /// So a bare leading-`-` form `-2` (no `n`), `-2n` (no offset), or `-2n-1` (a `-` offset) is
    /// NOT an nth match (it falls through the selector loop to `read_identifier`, which rejects a
    /// digit-leading `-?\d` as `css_expected_identifier`), while `-2n+1` / `-n+2` ARE the negative
    /// arm. A generic-optional-sign reader would over-accept `-2` / `-2n` / `-2n-1` and emit no
    /// reject — diverging from pinned `svelte@5.56.10`.
    fn nth_of_len(&self) -> Option<usize> {
        let rest = &self.src[self.index()..];
        let j = if rest.starts_with(b"even") {
            4
        } else if rest.starts_with(b"odd") {
            3
        } else if rest.first() == Some(&b'-') {
            // NEGATIVE arm `-\d*n(\s*\+\s*\d+)`: `-`, optional digits, MANDATORY `n`, MANDATORY
            // `\s*\+\s*\d+` (`+` offset only).
            self.nth_negative_arm_len(rest)?
        } else {
            // POSITIVE arm `\+?(\d+|\d*n(\s*[+-]\s*\d+)?)`.
            self.nth_positive_arm_len(rest)?
        };
        // trailing alternation `((?=\s*[,)])|\s+of\s+)`, tried left-to-right (JS `\s` — Unicode):
        // (1) the zero-width end lookahead `\s*[,)]` — return `j` WITHOUT consuming the ws.
        let k = skip_js_whitespace(rest, j);
        if matches!(rest.get(k), Some(b',' | b')')) {
            return Some(j);
        }
        // (2) the CONSUMING `\s+of\s+` arm — `\s+` (≥1), the literal `of`, `\s+` (≥1). On match,
        // return the byte length INCLUDING the trailing whitespace (so the `<selector>` after
        // `of` is read by the normal selector loop).
        let m = skip_js_whitespace(rest, j);
        if m == j {
            return None; // `\s+` needs ≥1 whitespace before `of`
        }
        if !rest[m..].starts_with(b"of") {
            return None;
        }
        let after_of = skip_js_whitespace(rest, m + 2);
        if after_of == m + 2 {
            return None; // `\s+` needs ≥1 whitespace after `of`
        }
        Some(after_of)
    }

    /// The byte length of the POSITIVE An+B arm `\+?(\d+|\d*n(\s*[+-]\s*\d+)?)` at the start of
    /// `rest`, or `None`. An OPTIONAL leading `+`; then EITHER a plain `\d+` (one-or-more digits,
    /// NO `n`) OR `\d*n` (zero-or-more digits then `n`) with an OPTIONAL `\s*[+-]\s*\d+` offset (a
    /// `+` OR `-` offset). Returns `None` when neither alternative matches (e.g. a bare `+`, or `+`
    /// followed by a non-digit non-`n`).
    fn nth_positive_arm_len(&self, rest: &[u8]) -> Option<usize> {
        let mut j = 0usize;
        if rest.first() == Some(&b'+') {
            j += 1;
        }
        let dstart = j;
        j += ascii_digits_len(rest, j);
        if rest.get(j) == Some(&b'n') {
            // `\d*n` (the leading digits are OPTIONAL here) + an OPTIONAL `\s*[+-]\s*\d+` offset.
            j += 1;
            if let Some(off) = self.nth_offset_len(rest, j, true) {
                j = off;
            }
            Some(j)
        } else if j > dstart {
            // a plain `\d+` (one-or-more digits, no `n`).
            Some(j)
        } else {
            None // neither `\d+` nor `\d*n` — not a positive nth match
        }
    }

    /// The byte length of the NEGATIVE An+B arm `-\d*n(\s*\+\s*\d+)` at the start of `rest` (which
    /// must begin with `-`), or `None`. A MANDATORY `-`; `\d*` (optional digits); a MANDATORY `n`;
    /// then a MANDATORY `\s*\+\s*\d+` offset — a `+` offset ONLY (a `-` offset, a missing `n`, or a
    /// missing offset is NOT this arm, so `-2`, `-2n`, and `-2n-1` all return `None`).
    fn nth_negative_arm_len(&self, rest: &[u8]) -> Option<usize> {
        debug_assert_eq!(rest.first(), Some(&b'-'));
        let mut j = 1usize; // the leading `-`
        j += ascii_digits_len(rest, j);
        if rest.get(j) != Some(&b'n') {
            return None; // the `n` is mandatory
        }
        j += 1;
        // a MANDATORY `+`-only offset (`plus_or_minus = false`).
        self.nth_offset_len(rest, j, false)
    }

    /// The byte length up to and including a `\s*<sign>\s*\d+` An+B offset starting at byte index
    /// `from` in `rest`, or `None` when no offset is present. `plus_or_minus` selects the offset
    /// sign set: `true` accepts `+` OR `-` (the positive arm's `[+-]`), `false` accepts `+` ONLY
    /// (the negative arm's `\+`). An offset requires the sign AND ≥1 trailing digit; a sign with no
    /// digit (e.g. `2n+`) is not a match.
    fn nth_offset_len(&self, rest: &[u8], from: usize, plus_or_minus: bool) -> Option<usize> {
        // `\s*<sign>\s*\d+` — the `\s*` runs are JS `\s` (Unicode).
        let mut k = skip_js_whitespace(rest, from);
        let sign_ok = match rest.get(k) {
            Some(b'+') => true,
            Some(b'-') => plus_or_minus,
            _ => false,
        };
        if !sign_ok {
            return None;
        }
        k = skip_js_whitespace(rest, k + 1);
        let ds = k;
        k += ascii_digits_len(rest, k);
        if k > ds {
            Some(k)
        } else {
            None // a sign with no trailing digit is not an offset
        }
    }
}

/// The JS `String.prototype.trim()` set — identical to the JS `\s` set (WhiteSpace ∪
/// LineTerminator): INCLUDES U+FEFF, EXCLUDES U+0085. Rust `str::trim` (Unicode `White_Space`)
/// diverges on exactly those two, so the official `value.trim()` routes here.
fn trim_js_whitespace(s: &str) -> &str {
    s.trim_matches(|c: char| is_js_whitespace_codepoint(c as u32))
}

/// Advance `k` past the JS-`\s` whitespace run in `rest` (a valid-UTF-8 suffix of the source; `k`
/// on a char boundary), decoding codepoints — the `\s*` / `\s+` scans of `REGEX_NTH_OF` are
/// Unicode-aware.
fn skip_js_whitespace(rest: &[u8], mut k: usize) -> usize {
    while let Some((cp, len)) = codepoint_at(rest, k) {
        if !is_js_whitespace_codepoint(cp) {
            break;
        }
        k += len;
    }
    k
}
