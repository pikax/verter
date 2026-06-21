//! A faithful VALIDATION-ONLY port of the official `svelte@5.56.3` CSS body reader's PARSE
//! control flow (`phases/1-parse/read/style.js` + the `Parser` byte primitives in
//! `phases/1-parse/index.js`).
//!
//! Upstream's `element.js` calls `read_style` — which PARSES the `<style>` CSS body via a full
//! CSS reader and can THROW a parse error — BEFORE `if (current.css) e.style_duplicate(start)`.
//! So a malformed CSS body in the 2nd `<style>` wins the first-error race over `style_duplicate`,
//! and Verter must report the EXACT upstream CSS parse code. The parser reserves a
//! [`StyleBodyProbe`] at the `read_style` position; this module is what the official-reject gate
//! runs to FILL that slot.
//!
//! Scope (codex-ruled): this is the upstream `read_style` PARSE-ENTRY control flow ONLY — enough
//! to return the FIRST exact parse code reachable from `read/style.js`'s readers
//! (`css_expected_identifier`, `css_empty_declaration`, `css_selector_invalid`, plus the generic
//! `expected_token` / `unexpected_eof` the `Parser` primitives throw). It builds NO CSS AST and
//! performs NO CSS analysis / scoping — the post-parse CSS validation family (`css_global_*`,
//! nesting placement, …) is a deferred CSS-scoping vertical (see the debt-ledger entry in
//! `docs/arch/svelte-native-compiler-plan.md`). A body that parses CLEAN here returns `None`, so
//! the later `style_duplicate` (or the unsupported-`<style>` rail) wins.
//!
//! The reader operates on the WHOLE component `source` from the CSS body's `content_start` and
//! stops the body loop at the literal `</style` or EOF (upstream's `finished` predicate) — it is
//! NOT given an isolated body slice, because upstream's nested CSS readers (a `read_block` /
//! `read_value` inside an unterminated rule) run PAST `</style>` into the rest of the source, and
//! that emergent behaviour decides the exact code.

/// The result of one CSS reader step: `Ok(())` when the step parsed cleanly, `Err(code)` carrying
/// the EXACT upstream CSS parse code the step would `throw`.
type CssResult = Result<(), &'static str>;

/// Parse the CSS body that begins at `content_start` in `source` exactly as upstream's
/// `read_style` → `read_body(parser, p => p.match('</style') || p.index >= p.template.length)`
/// does, returning the FIRST exact CSS parse code on a body-parse FAILURE, or `None` when the
/// body parses cleanly (so the later `style_duplicate` / unsupported-`<style>` rail wins).
#[must_use]
pub(super) fn css_body_parse_error(source: &str, content_start: usize) -> Option<&'static str> {
    let mut p = CssParser {
        src: source.as_bytes(),
        text: source,
        index: content_start.min(source.len()),
    };
    p.read_body().err()
}

/// The byte cursor + the upstream `Parser` primitives the CSS readers use. A faithful but
/// validation-only mirror of the official `Parser` (`phases/1-parse/index.js`): the `eat(str,
/// required=true)` form throws `expected_token`; `read_until` throws `unexpected_eof` at EOF; the
/// rest are non-throwing scans.
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

    /// `parser.eat(str, required)` — consume `str` if present (returning `true`); when `required`
    /// and absent, throw `expected_token`.
    fn eat(&mut self, s: &[u8], required: bool) -> Result<bool, &'static str> {
        if self.matches(s) {
            self.index += s.len();
            Ok(true)
        } else if required {
            Err("expected_token")
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

    /// `parser.read_until(pattern)` for a SIMPLE single-char-class pattern: advance to the first
    /// byte satisfying `pred`, returning the consumed slice; at EOF (upstream's non-loose branch)
    /// throw `unexpected_eof`. Used for `REGEX_WHITESPACE_OR_COLON` in `read_declaration`.
    fn read_until_char_class(
        &mut self,
        pred: impl Fn(u8) -> bool,
    ) -> Result<&'a str, &'static str> {
        if self.at_eof() {
            return Err("unexpected_eof");
        }
        let start = self.index;
        while self.index < self.len() && !pred(self.src[self.index]) {
            self.index += 1;
        }
        Ok(&self.text[start..self.index])
    }

    /// The Unicode codepoint at byte `i` (decoding the UTF-8 char). Mirrors upstream's
    /// `template.codePointAt(index)` for the `>= 160` identifier-char test and whitespace.
    fn codepoint_at(&self, i: usize) -> u32 {
        self.text[i..].chars().next().map_or(0, |c| c as u32)
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
        if self.matches(b"{") {
            self.read_block()?;
        } else {
            self.eat(b";", true)?;
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
        while self.index < self.len() {
            self.read_selector(inside_pseudo_class)?;
            self.allow_comment_or_whitespace()?;
            let closes = if inside_pseudo_class {
                self.matches(b")")
            } else {
                self.matches(b"{")
            };
            if closes {
                return Ok(());
            }
            self.eat(b",", true)?;
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
            } else if self.eat(b"#", false)? || self.eat(b".", false)? {
                // IdSelector (`#x`) / ClassSelector (`.x`) — distinct AST nodes upstream, the same
                // parse control flow: a required identifier.
                self.read_identifier()?;
            } else if self.eat(b"::", false)? || self.eat(b":", false)? {
                // PseudoElementSelector (`::x`) / PseudoClassSelector (`:x`) — `::` is checked
                // FIRST (the `||` short-circuit preserves upstream's branch order so a `:` does not
                // eat the first colon of `::`); both read an identifier + an optional `(args)`.
                self.read_identifier()?;
                if self.eat(b"(", false)? {
                    self.read_selector_list(true)?;
                    self.eat(b")", true)?;
                }
            } else if self.eat(b"[", false)? {
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
                self.eat(b"]", true)?;
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

            let index = self.index;
            self.allow_comment_or_whitespace()?;

            let closes = self.matches(b",")
                || if inside_pseudo_class {
                    self.matches(b")")
                } else {
                    self.matches(b"{")
                };
            if closes {
                // rewind, return the complex selector.
                self.index = index;
                return Ok(());
            }

            self.index = index;
            let had_combinator = self.read_combinator();
            if had_combinator {
                self.allow_whitespace();
                let closes_after = self.matches(b",")
                    || if inside_pseudo_class {
                        self.matches(b")")
                    } else {
                        self.matches(b"{")
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
        let start = self.index;
        self.allow_whitespace();
        if self.read_combinator_token() {
            self.allow_whitespace();
            return true;
        }
        // a whitespace-only descendant combinator: whitespace was consumed (index advanced).
        self.index != start
    }

    /// `read_block(parser)` — `{` … block items … `}`. Throws `expected_token` on a missing brace.
    fn read_block(&mut self) -> CssResult {
        self.eat(b"{", true)?;
        while self.index < self.len() {
            self.allow_comment_or_whitespace()?;
            if self.matches(b"}") {
                break;
            }
            self.read_block_item()?;
        }
        self.eat(b"}", true)?;
        Ok(())
    }

    /// `read_block_item(parser)` — disambiguate a nested rule (look-ahead sees a `{` after the
    /// value) from a declaration. Mirrors upstream's read-ahead-then-rewind.
    fn read_block_item(&mut self) -> CssResult {
        if self.matches(b"@") {
            return self.read_at_rule();
        }
        let start = self.index;
        self.read_value()?;
        let next = self.src.get(self.index).copied();
        self.index = start;
        if next == Some(b'{') {
            self.read_rule()
        } else {
            self.read_declaration()
        }
    }

    /// `read_declaration(parser)` — a property, `:`, a value; an empty non-`--` declaration is
    /// `css_empty_declaration`; a non-`}`-terminated declaration must `eat(';', true)`.
    fn read_declaration(&mut self) -> CssResult {
        let property = self.read_until_char_class(|b| b.is_ascii_whitespace() || b == b':')?;
        self.allow_whitespace();
        self.eat(b":", false)?;
        self.allow_whitespace();
        let value = self.read_value()?;
        if value.is_empty() && !property.starts_with("--") {
            return Err("css_empty_declaration");
        }
        if !self.matches(b"}") {
            self.eat(b";", true)?;
        }
        Ok(())
    }

    /// `read_value(parser)` — read up to an unquoted/unparen `;` / `{` / `}` (returning the
    /// trimmed value text), skipping `/* … */` comments and respecting quotes + `url(...)`. At
    /// EOF it throws `unexpected_eof`.
    fn read_value(&mut self) -> Result<String, &'static str> {
        let mut value = String::new();
        let mut escaped = false;
        let mut in_url = false;
        let mut quote_mark: Option<u8> = None;
        while self.index < self.len() {
            let ch = self.src[self.index];
            if escaped {
                value.push('\\');
                value.push(ch as char);
                escaped = false;
                self.index += 1;
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
            } else if ch == b'(' && value.len() >= 3 && &value[value.len() - 3..] == "url" {
                in_url = true;
            } else if (ch == b';' || ch == b'{' || ch == b'}') && !in_url && quote_mark.is_none() {
                return Ok(value.trim().to_string());
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
            value.push(ch as char);
            self.index += 1;
        }
        Err("unexpected_eof")
    }

    /// `read_attribute_value(parser)` — a quoted or unquoted attribute value, closing on the
    /// matching quote or (unquoted) a whitespace / `]`. At EOF it throws `unexpected_eof`.
    fn read_attribute_value(&mut self) -> CssResult {
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
                escaped = false;
            } else if ch == b'\\' {
                escaped = true;
            } else {
                let closes = match quote_mark {
                    Some(q) => ch == q,
                    None => ch.is_ascii_whitespace() || ch == b']',
                };
                if closes {
                    if let Some(q) = quote_mark {
                        self.eat(&[q], true)?;
                    }
                    return Ok(());
                }
            }
            self.index += 1;
        }
        Err("unexpected_eof")
    }

    /// `read_identifier(parser)` (the CSS one) — a CSS ident token. A leading `-?<digit>` is
    /// `css_expected_identifier`; an EMPTY identifier is `css_expected_identifier`. Handles `\`
    /// unicode escapes and ident chars (`[a-zA-Z0-9_-]` plus any codepoint ≥ 160).
    fn read_identifier(&mut self) -> CssResult {
        // REGEX_LEADING_HYPHEN_OR_DIGIT = /-?\d/y at the current index.
        if self.match_leading_hyphen_or_digit() {
            return Err("css_expected_identifier");
        }
        let mut len = 0usize;
        while self.index < self.len() {
            let ch = self.src[self.index];
            if ch == b'\\' {
                if let Some(seq_len) = self.match_unicode_sequence() {
                    self.index += seq_len;
                    len += 1;
                } else {
                    // `\` + next char (2 bytes in the ASCII case upstream slices).
                    let next_len = self.src.get(self.index + 1).map_or(1, |&b| char_len(b));
                    self.index += 1 + next_len;
                    len += 1;
                }
            } else {
                let cp = self.codepoint_at(self.index);
                if cp >= 160 || is_valid_identifier_char(ch) {
                    self.index += char_len(ch);
                    len += 1;
                } else {
                    break;
                }
            }
        }
        if len == 0 {
            return Err("css_expected_identifier");
        }
        Ok(())
    }

    /// `allow_comment_or_whitespace(parser)` — whitespace then any run of `/* … */` / `<!-- … -->`
    /// comments. Upstream's REQUIRED close tokens (`eat('*/', true)` / `eat('-->', true)`) raise
    /// `expected_token` on an unterminated comment, so this returns [`CssResult`] and propagates
    /// that error: `read_until` advances to the close token OR end-of-input, after which the
    /// REQUIRED `eat` fails (`expected_token`) when the close is absent.
    fn allow_comment_or_whitespace(&mut self) -> CssResult {
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

    // ── scan helpers (the regex primitives, hand-coded) ──────────────────────────────────

    /// Advance to the first occurrence of `needle` (or EOF), like `parser.read_until(/needle/)`.
    fn read_until_str(&mut self, needle: &[u8]) {
        while self.index < self.len() && !self.matches(needle) {
            self.index += 1;
        }
    }

    /// `REGEX_MATCHER = /[~^$*|]?=/y` — consume an optional `~^$*|` then a required `=`. Returns
    /// whether a matcher was read (advancing only on a full match).
    fn read_matcher(&mut self) -> bool {
        let i = self.index;
        let mut j = i;
        if let Some(&b) = self.src.get(j) {
            if matches!(b, b'~' | b'^' | b'$' | b'*' | b'|') {
                j += 1;
            }
        }
        if self.src.get(j) == Some(&b'=') {
            self.index = j + 1;
            true
        } else {
            false
        }
    }

    /// `REGEX_ATTRIBUTE_FLAGS = /[a-zA-Z]+/y` — consume a run of ASCII letters.
    fn read_attribute_flags(&mut self) {
        while self.index < self.len() && self.src[self.index].is_ascii_alphabetic() {
            self.index += 1;
        }
    }

    /// `parser.read(REGEX_COMBINATOR)` — consume `+` / `~` / `>` / `||` at the current index.
    fn read_combinator_token(&mut self) -> bool {
        if self.matches(b"||") {
            self.index += 2;
            true
        } else if matches!(self.src.get(self.index), Some(b'+' | b'~' | b'>')) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    /// `parser.match_regex(REGEX_COMBINATOR)` — whether a combinator token is at the current index
    /// (non-consuming).
    fn match_combinator(&self) -> bool {
        self.matches(b"||") || matches!(self.src.get(self.index), Some(b'+' | b'~' | b'>'))
    }

    /// `REGEX_LEADING_HYPHEN_OR_DIGIT = /-?\d/y` at the current index (non-consuming).
    fn match_leading_hyphen_or_digit(&self) -> bool {
        let mut j = self.index;
        if self.src.get(j) == Some(&b'-') {
            j += 1;
        }
        matches!(self.src.get(j), Some(b) if b.is_ascii_digit())
    }

    /// `REGEX_PERCENTAGE = /\d+(\.\d+)?%/y` at the current index (non-consuming).
    fn match_percentage(&self) -> bool {
        self.percentage_len().is_some()
    }

    /// Consume a `REGEX_PERCENTAGE` match.
    fn read_percentage(&mut self) {
        if let Some(len) = self.percentage_len() {
            self.index += len;
        }
    }

    /// The byte length of a `\d+(\.\d+)?%` match at the current index, or `None`.
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
                j = k; // a fractional part requires ≥1 digit; otherwise the `.` is not consumed
            }
        }
        if self.src.get(j) == Some(&b'%') {
            Some(j + 1 - self.index)
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
            self.index += len;
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
    /// reject — diverging from pinned `svelte@5.56.3`.
    fn nth_of_len(&self) -> Option<usize> {
        let rest = &self.src[self.index..];
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
        // trailing alternation `((?=\s*[,)])|\s+of\s+)`, tried left-to-right:
        // (1) the zero-width end lookahead `\s*[,)]` — return `j` WITHOUT consuming the ws.
        let mut k = j;
        while matches!(rest.get(k), Some(b) if b.is_ascii_whitespace()) {
            k += 1;
        }
        if matches!(rest.get(k), Some(b',' | b')')) {
            return Some(j);
        }
        // (2) the CONSUMING `\s+of\s+` arm — `\s+` (≥1), the literal `of`, `\s+` (≥1). On match,
        // return the byte length INCLUDING the trailing whitespace (so the `<selector>` after
        // `of` is read by the normal selector loop).
        let mut m = j;
        let ws_before = m;
        while matches!(rest.get(m), Some(b) if b.is_ascii_whitespace()) {
            m += 1;
        }
        if m == ws_before {
            return None; // `\s+` needs ≥1 whitespace before `of`
        }
        if !rest[m..].starts_with(b"of") {
            return None;
        }
        m += 2;
        let ws_after = m;
        while matches!(rest.get(m), Some(b) if b.is_ascii_whitespace()) {
            m += 1;
        }
        if m == ws_after {
            return None; // `\s+` needs ≥1 whitespace after `of`
        }
        Some(m)
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
        while matches!(rest.get(j), Some(b) if b.is_ascii_digit()) {
            j += 1;
        }
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
        while matches!(rest.get(j), Some(b) if b.is_ascii_digit()) {
            j += 1;
        }
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
        let mut k = from;
        while matches!(rest.get(k), Some(b) if b.is_ascii_whitespace()) {
            k += 1;
        }
        let sign_ok = match rest.get(k) {
            Some(b'+') => true,
            Some(b'-') => plus_or_minus,
            _ => false,
        };
        if !sign_ok {
            return None;
        }
        k += 1;
        while matches!(rest.get(k), Some(b) if b.is_ascii_whitespace()) {
            k += 1;
        }
        let ds = k;
        while matches!(rest.get(k), Some(b) if b.is_ascii_digit()) {
            k += 1;
        }
        if k > ds {
            Some(k)
        } else {
            None // a sign with no trailing digit is not an offset
        }
    }

    /// `REGEX_UNICODE_SEQUENCE = /\\[0-9a-fA-F]{1,6}(\r\n|\s)?/y` at the current index — the byte
    /// length of the match (including the leading `\`), or `None` when it is not a hex escape.
    fn match_unicode_sequence(&self) -> Option<usize> {
        if self.src.get(self.index) != Some(&b'\\') {
            return None;
        }
        let mut j = self.index + 1;
        let hex_start = j;
        while j < self.len() && j - hex_start < 6 && self.src[j].is_ascii_hexdigit() {
            j += 1;
        }
        if j == hex_start {
            return None; // need ≥1 hex digit
        }
        // optional trailing `\r\n` or a single whitespace.
        if self.src.get(j) == Some(&b'\r') && self.src.get(j + 1) == Some(&b'\n') {
            j += 2;
        } else if matches!(self.src.get(j), Some(b) if is_css_whitespace(*b as u32)) {
            j += 1;
        }
        Some(j - self.index)
    }
}

/// Whether `cp` is in the official parser's whitespace set (`is_whitespace` in `index.js`): the
/// common ASCII whitespace plus the rare Unicode whitespace codepoints.
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
#[path = "css_reject_tests.rs"]
mod css_reject_tests;
