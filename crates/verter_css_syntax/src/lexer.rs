use memchr::memchr;

use crate::dialect::{css, less, sass, scss, stylus, CssDialect};
use crate::parser::CssSource;
use crate::token::{css_identifier_eq_ignore_ascii_case, SyntaxToken, TokenFlags, TokenKind};

/// Whitespace classification profile a scan uses. `Css` is the CSS Syntax Module Level 3 ASCII
/// set ([`is_css_whitespace`]); `JsUnicode` is JS `\s` (that ASCII core, plus vertical tab, plus a
/// run of Unicode space codepoints) — the profile the Svelte compat validation reader
/// ([`crate::svelte_compat`]) needs so its scans match upstream `svelte@5.56.10`'s own
/// Unicode-aware `\s` regexes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WhitespaceProfile {
    Css,
    JsUnicode,
}

/// Identifier name-start/name-continue codepoint profile. `Css` treats any codepoint `>= 128` as
/// an identifier char (the general CSS Syntax Module rule this lexer otherwise applies);
/// `SvelteCompat` narrows that threshold to `>= 160`, matching upstream `svelte@5.56.10`'s own
/// identifier-char test (which excludes the U+0080..U+009F block the general rule admits).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentifierProfile {
    Css,
    SvelteCompat,
}

/// Decode the UTF-8 codepoint starting at byte `at`, returning `(codepoint, byte length)` — or
/// `None` at or past the end of `bytes`. Shared by the general lexer's codepoint-aware scans (the
/// `SvelteCompat` `>= 160` identifier threshold, `JsUnicode` whitespace) and
/// [`crate::svelte_compat`]'s own codepoint-class scans (`nth-of`'s whitespace runs, the
/// whitespace-or-colon property-name reader).
#[inline]
pub(crate) fn codepoint_at(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let &lead = bytes.get(at)?;
    if lead < 0x80 {
        return Some((u32::from(lead), 1));
    }
    let width = char_width(bytes, at);
    let slice = bytes.get(at..at + width)?;
    let c = std::str::from_utf8(slice).ok()?.chars().next()?;
    Some((c as u32, c.len_utf8()))
}

/// Whether `cp` is whitespace under `profile`.
#[inline]
pub(crate) fn is_whitespace_codepoint(cp: u32, profile: WhitespaceProfile) -> bool {
    match profile {
        WhitespaceProfile::Css => cp < 128 && is_css_whitespace(cp as u8),
        WhitespaceProfile::JsUnicode => is_js_whitespace_codepoint(cp),
    }
}

/// JS `\s` (`WhiteSpace` ∪ `LineTerminator`): the [`is_css_whitespace`] ASCII core, plus vertical
/// tab `\x0B` (which CSS does not treat as whitespace), plus the Unicode space run. Used by the
/// Svelte compat profile ([`crate::svelte_compat`]) to match upstream `svelte@5.56.10`'s
/// Unicode-aware `\s` regexes — genuinely wider than the general CSS Syntax Module ASCII set.
#[inline]
pub(crate) fn is_js_whitespace_codepoint(cp: u32) -> bool {
    if cp < 128 {
        return cp == 0x0B || is_css_whitespace(cp as u8);
    }
    if cp < 160 {
        return false;
    }
    matches!(cp, 160 | 5760 | 8232 | 8233 | 8239 | 8287 | 12288 | 65279)
        || (8192..=8202).contains(&cp)
}

/// Whether `byte` (an ASCII byte `< 0x80`) is an identifier name char under `profile`. `Css`
/// delegates to the general [`is_name`] rule (alphanumeric, `_`, `-`, plus the CSS Syntax Module's
/// NUL-substitution allowance); `SvelteCompat` is upstream `svelte@5.56.10`'s own narrower
/// `REGEX_VALID_IDENTIFIER_CHAR = /[a-zA-Z0-9_-]/` (no NUL allowance).
#[inline]
fn is_ascii_name_char(byte: u8, profile: IdentifierProfile) -> bool {
    match profile {
        IdentifierProfile::Css => is_name(byte),
        IdentifierProfile::SvelteCompat => {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
        }
    }
}

#[derive(Clone)]
pub struct Lexer<'a> {
    source: &'a CssSource,
    bytes: &'a [u8],
    cursor: usize,
    dialect: CssDialect,
    at_statement_boundary: bool,
    unicode_ranges_allowed: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a CssSource, dialect: CssDialect) -> Self {
        Self {
            source,
            bytes: source.text().as_bytes(),
            cursor: 0,
            dialect,
            at_statement_boundary: true,
            unicode_ranges_allowed: false,
        }
    }

    pub(crate) fn set_unicode_ranges_allowed(&mut self, allowed: bool) -> bool {
        std::mem::replace(&mut self.unicode_ranges_allowed, allowed)
    }

    #[inline]
    pub fn position(&self) -> u32 {
        self.absolute(self.cursor)
    }

    #[inline]
    fn absolute(&self, local: usize) -> u32 {
        // CssSource construction already proved origin+len fits u32.
        self.source.origin() + local as u32
    }

    #[inline]
    fn make(&self, kind: TokenKind, flags: u16, start: usize, end: usize) -> SyntaxToken {
        SyntaxToken::new(kind, flags, self.absolute(start), self.absolute(end))
    }

    fn consume_whitespace(&mut self) -> SyntaxToken {
        self.consume_whitespace_profiled(WhitespaceProfile::Css)
    }

    /// Consume a run of whitespace under `profile`. [`WhitespaceProfile::Css`]
    /// uses a byte loop over the Syntax Module ASCII set; [`WhitespaceProfile::JsUnicode`]
    /// additionally recognizes vertical tab and the Unicode space run.
    pub(crate) fn consume_whitespace_profiled(
        &mut self,
        profile: WhitespaceProfile,
    ) -> SyntaxToken {
        let start = self.cursor;
        let mut flags = TokenFlags::TRIVIA;
        if profile == WhitespaceProfile::Css {
            while self.cursor < self.bytes.len() {
                let byte = self.bytes[self.cursor];
                if !is_css_whitespace(byte) {
                    break;
                }
                if matches!(byte, b'\n' | b'\r' | b'\x0c') {
                    flags |= TokenFlags::CONTAINS_NEWLINE;
                }
                self.cursor += 1;
            }
            return self.make(TokenKind::Whitespace, flags, start, self.cursor);
        }
        while let Some((cp, len)) = codepoint_at(self.bytes, self.cursor) {
            if !is_whitespace_codepoint(cp, profile) {
                break;
            }
            if matches!(self.bytes[self.cursor], b'\n' | b'\r' | b'\x0c') {
                flags |= TokenFlags::CONTAINS_NEWLINE;
            }
            self.cursor += len;
        }
        self.make(TokenKind::Whitespace, flags, start, self.cursor)
    }

    pub(crate) fn consume_comment(&mut self) -> SyntaxToken {
        let start = self.cursor;
        self.cursor += 2;
        let mut flags = TokenFlags::TRIVIA;
        loop {
            let Some(relative) = memchr(b'*', &self.bytes[self.cursor..]) else {
                if self.bytes[self.cursor..]
                    .iter()
                    .any(|byte| matches!(byte, b'\n' | b'\r' | b'\x0c'))
                {
                    flags |= TokenFlags::CONTAINS_NEWLINE;
                }
                self.cursor = self.bytes.len();
                flags |= TokenFlags::UNTERMINATED;
                break;
            };
            if self.bytes[self.cursor..self.cursor + relative]
                .iter()
                .any(|byte| matches!(byte, b'\n' | b'\r' | b'\x0c'))
            {
                flags |= TokenFlags::CONTAINS_NEWLINE;
            }
            self.cursor += relative;
            if self.bytes.get(self.cursor + 1) == Some(&b'/') {
                self.cursor += 2;
                break;
            }
            self.cursor += 1;
        }
        self.make(TokenKind::Comment, flags, start, self.cursor)
    }

    fn consume_line_comment(&mut self) -> SyntaxToken {
        let start = self.cursor;
        self.cursor += 2;
        while self.cursor < self.bytes.len()
            && !matches!(self.bytes[self.cursor], b'\n' | b'\r' | b'\x0c')
        {
            self.cursor += char_width(self.bytes, self.cursor);
        }
        self.make(
            TokenKind::LineComment,
            TokenFlags::TRIVIA | TokenFlags::DIALECT_EXTENSION,
            start,
            self.cursor,
        )
    }

    fn consume_string(
        &mut self,
        start: usize,
        quote: u8,
        kind: TokenKind,
        extra_flags: u16,
    ) -> SyntaxToken {
        self.cursor += 1;
        let mut flags = extra_flags;
        while self.cursor < self.bytes.len() {
            match self.bytes[self.cursor] {
                byte if byte == quote => {
                    self.cursor += 1;
                    return self.make(kind, flags, start, self.cursor);
                }
                b'\n' | b'\r' | b'\x0c' => {
                    flags |= TokenFlags::UNTERMINATED;
                    return self.make(TokenKind::BadString, flags, start, self.cursor);
                }
                b'\\' => {
                    flags |= TokenFlags::CONTAINS_ESCAPE;
                    match self.bytes.get(self.cursor + 1) {
                        Some(b'\r') => {
                            flags |= TokenFlags::CONTAINS_NEWLINE;
                            self.cursor += 2;
                            if self.bytes.get(self.cursor) == Some(&b'\n') {
                                self.cursor += 1;
                            }
                        }
                        Some(b'\n' | b'\x0c') => {
                            flags |= TokenFlags::CONTAINS_NEWLINE;
                            self.cursor += 2;
                        }
                        _ => {
                            let escape_start = self.cursor;
                            self.consume_escape();
                            if self.bytes[escape_start..self.cursor]
                                .iter()
                                .any(|byte| matches!(byte, b'\n' | b'\r' | b'\x0c'))
                            {
                                flags |= TokenFlags::CONTAINS_NEWLINE;
                            }
                        }
                    }
                }
                _ => self.cursor += char_width(self.bytes, self.cursor),
            }
        }
        flags |= TokenFlags::UNTERMINATED;
        self.make(kind, flags, start, self.cursor)
    }

    fn consume_name(&mut self) -> u16 {
        self.consume_name_profiled(IdentifierProfile::Css, WhitespaceProfile::Css)
    }

    /// Consume a run of name (identifier-continuation) chars under `identifier_profile`,
    /// decoding escapes via [`Self::consume_escape_profiled`] under `whitespace_profile` for the
    /// escape's optional trailing separator. Shared by the general lexer's own identifier
    /// tokenizing (`Css`/`Css`) and the Svelte compat profile's `read_identifier`
    /// (`SvelteCompat`/`JsUnicode`) — see [`crate::svelte_compat`].
    pub(crate) fn consume_name_profiled(
        &mut self,
        identifier_profile: IdentifierProfile,
        whitespace_profile: WhitespaceProfile,
    ) -> u16 {
        let mut flags = 0u16;
        while self.cursor < self.bytes.len() {
            let byte = self.bytes[self.cursor];
            if byte < 0x80 {
                if is_ascii_name_char(byte, identifier_profile) {
                    self.cursor += 1;
                } else if valid_escape(self.bytes, self.cursor) {
                    flags |= TokenFlags::CONTAINS_ESCAPE;
                    self.consume_escape_profiled(whitespace_profile);
                } else {
                    break;
                }
                continue;
            }
            match identifier_profile {
                IdentifierProfile::Css => {
                    self.cursor += char_width(self.bytes, self.cursor);
                }
                IdentifierProfile::SvelteCompat => {
                    // A lead byte alone can't distinguish U+0080..U+009F (excluded by Svelte's
                    // own `>= 160` rule) from U+00A0 upward (allowed) — decode the codepoint.
                    let Some((cp, len)) = codepoint_at(self.bytes, self.cursor) else {
                        break;
                    };
                    if cp >= 160 {
                        self.cursor += len;
                    } else {
                        break;
                    }
                }
            }
        }
        flags
    }

    fn consume_number(&mut self) -> SyntaxToken {
        let start = self.cursor;
        if self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            self.cursor += 1;
        }
        self.cursor += ascii_digits_len(self.bytes, self.cursor);
        let mut integer = true;
        if self.bytes.get(self.cursor) == Some(&b'.')
            && self
                .bytes
                .get(self.cursor + 1)
                .is_some_and(u8::is_ascii_digit)
        {
            integer = false;
            self.cursor += 2;
            self.cursor += ascii_digits_len(self.bytes, self.cursor);
        }
        if self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| matches!(byte, b'e' | b'E'))
        {
            let exponent_digits = if self
                .bytes
                .get(self.cursor + 1)
                .is_some_and(u8::is_ascii_digit)
            {
                Some(self.cursor + 1)
            } else if self
                .bytes
                .get(self.cursor + 1)
                .is_some_and(|byte| matches!(byte, b'+' | b'-'))
                && self
                    .bytes
                    .get(self.cursor + 2)
                    .is_some_and(u8::is_ascii_digit)
            {
                Some(self.cursor + 2)
            } else {
                None
            };
            if let Some(digit_start) = exponent_digits {
                integer = false;
                self.cursor = digit_start + 1;
                self.cursor += ascii_digits_len(self.bytes, self.cursor);
            }
        }
        let flags = if integer {
            TokenFlags::NUMBER_INTEGER
        } else {
            0
        };
        if starts_identifier(self.bytes, self.cursor) {
            let name_flags = self.consume_name();
            self.make(TokenKind::Dimension, flags | name_flags, start, self.cursor)
        } else if self.bytes.get(self.cursor) == Some(&b'%') {
            self.cursor += 1;
            self.make(TokenKind::Percentage, flags, start, self.cursor)
        } else {
            self.make(TokenKind::Number, flags, start, self.cursor)
        }
    }

    fn consume_ident_like(&mut self) -> SyntaxToken {
        let start = self.cursor;
        let flags = self.consume_name();
        if self.bytes.get(self.cursor) != Some(&b'(') {
            return self.make(TokenKind::Ident, flags, start, self.cursor);
        }
        self.cursor += 1;
        let name_end = self.cursor - 1;
        if css_identifier_eq_ignore_ascii_case(
            std::str::from_utf8(&self.bytes[start..name_end])
                .expect("CssSource contains valid UTF-8"),
            "url",
        ) {
            let mut lookahead = self.cursor;
            while self
                .bytes
                .get(lookahead)
                .is_some_and(|byte| is_css_whitespace(*byte))
            {
                lookahead += 1;
            }
            if self
                .bytes
                .get(lookahead)
                .is_some_and(|byte| matches!(byte, b'"' | b'\''))
            {
                return self.make(TokenKind::Function, flags, start, self.cursor);
            }
            return self.consume_url(start, flags);
        }
        self.make(TokenKind::Function, flags, start, self.cursor)
    }

    fn consume_url(&mut self, start: usize, mut flags: u16) -> SyntaxToken {
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| is_css_whitespace(*byte))
        {
            self.cursor += 1;
        }
        while self.cursor < self.bytes.len() {
            match self.bytes[self.cursor] {
                b')' => {
                    self.cursor += 1;
                    return self.make(TokenKind::Url, flags, start, self.cursor);
                }
                byte if is_css_whitespace(byte) => {
                    while self
                        .bytes
                        .get(self.cursor)
                        .is_some_and(|next| is_css_whitespace(*next))
                    {
                        self.cursor += 1;
                    }
                    if self.bytes.get(self.cursor) == Some(&b')') {
                        self.cursor += 1;
                        return self.make(TokenKind::Url, flags, start, self.cursor);
                    }
                    if self.cursor == self.bytes.len() {
                        flags |= TokenFlags::UNTERMINATED;
                        return self.make(TokenKind::Url, flags, start, self.cursor);
                    }
                    return self.consume_bad_url(start, flags);
                }
                b'"' | b'\'' | b'(' | 0x01..=0x08 | 0x0b | 0x0e..=0x1f | 0x7f => {
                    return self.consume_bad_url(start, flags);
                }
                b'\\' if !valid_escape(self.bytes, self.cursor) => {
                    return self.consume_bad_url(start, flags);
                }
                b'\\' => {
                    flags |= TokenFlags::CONTAINS_ESCAPE;
                    self.consume_escape();
                }
                _ => self.cursor += char_width(self.bytes, self.cursor),
            }
        }
        flags |= TokenFlags::UNTERMINATED;
        self.make(TokenKind::Url, flags, start, self.cursor)
    }

    fn consume_bad_url(&mut self, start: usize, mut flags: u16) -> SyntaxToken {
        while self.cursor < self.bytes.len() {
            match self.bytes[self.cursor] {
                b')' => {
                    self.cursor += 1;
                    break;
                }
                b'\\' if valid_escape(self.bytes, self.cursor) => {
                    flags |= TokenFlags::CONTAINS_ESCAPE;
                    self.consume_escape();
                }
                _ => self.cursor += char_width(self.bytes, self.cursor),
            }
        }
        self.make(TokenKind::BadUrl, flags, start, self.cursor)
    }

    fn consume_prefixed_name(&mut self, kind: TokenKind) -> SyntaxToken {
        let start = self.cursor;
        self.cursor += 1;
        let flags = self.consume_name() | TokenFlags::DIALECT_EXTENSION;
        self.make(kind, flags, start, self.cursor)
    }

    fn consume_escape(&mut self) {
        self.consume_escape_profiled(WhitespaceProfile::Css);
    }

    /// Consume one escape sequence (the `\` this method is called on, plus its hex numeral or
    /// single escaped char) under `whitespace_profile` for the hex form's optional trailing
    /// separator — [`WhitespaceProfile::JsUnicode`] matches upstream `svelte@5.56.10`'s
    /// `REGEX_UNICODE_SEQUENCE` (`\r\n`, or any single JS-`\s` codepoint including multi-byte
    /// Unicode spaces), narrower [`WhitespaceProfile::Css`] only ever closes on a single ASCII
    /// whitespace byte.
    pub(crate) fn consume_escape_profiled(&mut self, whitespace_profile: WhitespaceProfile) {
        verter_debug_assert_eq!(self.bytes.get(self.cursor), Some(&b'\\'));
        if let Some(len) = hex_escape_digits_len(self.bytes, self.cursor) {
            self.cursor += len;
            if let Some((cp, cp_len)) = codepoint_at(self.bytes, self.cursor) {
                if is_whitespace_codepoint(cp, whitespace_profile) {
                    if self.bytes[self.cursor] == b'\r'
                        && self.bytes.get(self.cursor + 1) == Some(&b'\n')
                    {
                        self.cursor += 2;
                    } else {
                        self.cursor += cp_len;
                    }
                }
            }
        } else {
            self.cursor += 1;
            if self.cursor < self.bytes.len() {
                self.cursor += char_width(self.bytes, self.cursor);
            }
        }
    }

    /// Reposition the cursor to a LOCAL byte offset within this lexer's source (a local position,
    /// not an absolute one — see [`Self::position`]) — used by the Svelte compat profile's
    /// grammar-order lookahead/rewind ([`crate::svelte_compat`]).
    pub(crate) fn seek(&mut self, local: usize) {
        self.cursor = local;
    }

    fn less_variable_declaration_follows(&self, start: usize) -> bool {
        let mut probe = self.clone();
        probe.cursor = start + 1;
        probe.consume_name();
        loop {
            let Some(token) = probe.next_token() else {
                return false;
            };
            match token.kind() {
                TokenKind::Whitespace | TokenKind::Comment => {}
                TokenKind::Colon => return true,
                _ => return false,
            }
        }
    }

    fn consume_unicode_range(&mut self) -> SyntaxToken {
        let start = self.cursor;
        self.cursor += 2;
        let mut hex_digits = 0usize;
        while self.cursor < self.bytes.len()
            && hex_digits < 6
            && self.bytes[self.cursor].is_ascii_hexdigit()
        {
            self.cursor += 1;
            hex_digits += 1;
        }
        let mut wildcards = 0usize;
        while self.cursor < self.bytes.len()
            && hex_digits + wildcards < 6
            && self.bytes[self.cursor] == b'?'
        {
            self.cursor += 1;
            wildcards += 1;
        }
        if wildcards == 0
            && self.bytes.get(self.cursor) == Some(&b'-')
            && self
                .bytes
                .get(self.cursor + 1)
                .is_some_and(u8::is_ascii_hexdigit)
        {
            self.cursor += 1;
            let mut end_digits = 0usize;
            while self
                .bytes
                .get(self.cursor)
                .is_some_and(u8::is_ascii_hexdigit)
                && end_digits < 6
            {
                self.cursor += 1;
                end_digits += 1;
            }
        }
        self.make(TokenKind::UnicodeRange, 0, start, self.cursor)
    }
}

impl Lexer<'_> {
    fn next_token(&mut self) -> Option<SyntaxToken> {
        if self.cursor >= self.bytes.len() {
            return None;
        }
        let start = self.cursor;
        let byte = self.bytes[start];
        if is_css_whitespace(byte) {
            return Some(self.consume_whitespace());
        }
        if self.bytes[start..].starts_with(b"/*") {
            return Some(self.consume_comment());
        }
        if self.dialect.allows_line_comments() && self.bytes[start..].starts_with(b"//") {
            return Some(self.consume_line_comment());
        }
        if self.bytes[start..].starts_with(css::CDO) {
            self.cursor += css::CDO.len();
            return Some(self.make(TokenKind::Cdo, 0, start, self.cursor));
        }
        if self.bytes[start..].starts_with(css::CDC) {
            self.cursor += css::CDC.len();
            return Some(self.make(TokenKind::Cdc, 0, start, self.cursor));
        }
        if self.unicode_ranges_allowed
            && matches!(byte, b'u' | b'U')
            && self.bytes.get(start + 1) == Some(&b'+')
            && self
                .bytes
                .get(start + 2)
                .is_some_and(|next| next.is_ascii_hexdigit() || *next == b'?')
        {
            return Some(self.consume_unicode_range());
        }
        if matches!(self.dialect, CssDialect::Scss | CssDialect::Sass)
            && byte
                == if self.dialect == CssDialect::Sass {
                    sass::VARIABLE_PREFIX
                } else {
                    scss::VARIABLE_PREFIX
                }
            && starts_identifier(self.bytes, start + 1)
        {
            return Some(self.consume_prefixed_name(TokenKind::ScssVariable));
        }
        if matches!(self.dialect, CssDialect::Scss | CssDialect::Sass)
            && self.bytes[start..].starts_with(if self.dialect == CssDialect::Sass {
                sass::INTERPOLATION_PREFIX
            } else {
                scss::INTERPOLATION_PREFIX
            })
        {
            self.cursor += 2;
            return Some(self.make(
                TokenKind::ScssInterpolationStart,
                TokenFlags::DIALECT_EXTENSION,
                start,
                self.cursor,
            ));
        }
        if self.dialect == CssDialect::Stylus
            && self.bytes[start..].starts_with(stylus::DOLLAR_INTERPOLATION_PREFIX)
        {
            self.cursor += stylus::DOLLAR_INTERPOLATION_PREFIX.len();
            return Some(self.make(
                TokenKind::StylusInterpolationStart,
                TokenFlags::DIALECT_EXTENSION,
                start,
                self.cursor,
            ));
        }
        if self.dialect == CssDialect::Less
            && self.bytes[start..].starts_with(less::INTERPOLATION_PREFIX)
        {
            self.cursor += 2;
            return Some(self.make(
                TokenKind::LessInterpolationStart,
                TokenFlags::DIALECT_EXTENSION,
                start,
                self.cursor,
            ));
        }
        if self.dialect == CssDialect::Less
            && byte == b'~'
            && self
                .bytes
                .get(start + 1)
                .is_some_and(|next| matches!(next, b'"' | b'\''))
        {
            self.cursor += 1;
            let quote = self.bytes[self.cursor];
            return Some(self.consume_string(
                start,
                quote,
                TokenKind::LessEscapedString,
                TokenFlags::DIALECT_EXTENSION,
            ));
        }
        if starts_number(self.bytes, start) {
            return Some(self.consume_number());
        }
        if starts_identifier(self.bytes, start) {
            return Some(self.consume_ident_like());
        }

        match byte {
            b'"' | b'\'' => Some(self.consume_string(start, byte, TokenKind::String, 0)),
            b'#' if starts_name_sequence(self.bytes, start + 1) => {
                self.cursor += 1;
                let mut flags = self.consume_name();
                if starts_identifier(self.bytes, start + 1) {
                    flags |= TokenFlags::ID_HASH;
                }
                Some(self.make(TokenKind::Hash, flags, start, self.cursor))
            }
            byte if byte == less::VARIABLE_PREFIX
                && self.dialect == CssDialect::Less
                && starts_identifier(self.bytes, start + 1)
                && (self.less_variable_declaration_follows(start)
                    || !self.at_statement_boundary) =>
            {
                Some(self.consume_prefixed_name(TokenKind::LessVariable))
            }
            b'@' if starts_identifier(self.bytes, start + 1) => {
                self.cursor += 1;
                let flags = self.consume_name();
                Some(self.make(TokenKind::AtKeyword, flags, start, self.cursor))
            }
            b':' => {
                self.cursor += 1;
                Some(self.make(TokenKind::Colon, 0, start, self.cursor))
            }
            b';' => {
                self.cursor += 1;
                Some(self.make(TokenKind::Semicolon, 0, start, self.cursor))
            }
            b',' => {
                self.cursor += 1;
                Some(self.make(TokenKind::Comma, 0, start, self.cursor))
            }
            b'(' => {
                self.cursor += 1;
                Some(self.make(TokenKind::LeftParen, 0, start, self.cursor))
            }
            b')' => {
                self.cursor += 1;
                Some(self.make(TokenKind::RightParen, 0, start, self.cursor))
            }
            b'[' => {
                self.cursor += 1;
                Some(self.make(TokenKind::LeftBracket, 0, start, self.cursor))
            }
            b']' => {
                self.cursor += 1;
                Some(self.make(TokenKind::RightBracket, 0, start, self.cursor))
            }
            b'{' => {
                self.cursor += 1;
                let adjacent_fragment = start > 0
                    && !is_css_whitespace(self.bytes[start - 1])
                    && matches!(
                        self.bytes[start - 1],
                        b'-' | b'_' | b')' | b']' | b'#' | b'.' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z'
                    );
                if self.dialect == CssDialect::Stylus
                    && adjacent_fragment
                    && self.stylus_brace_is_interpolation(start)
                {
                    Some(self.make(
                        TokenKind::StylusInterpolationStart,
                        TokenFlags::DIALECT_EXTENSION,
                        start,
                        self.cursor,
                    ))
                } else {
                    Some(self.make(TokenKind::LeftBrace, 0, start, self.cursor))
                }
            }
            b'}' => {
                self.cursor += 1;
                Some(self.make(TokenKind::RightBrace, 0, start, self.cursor))
            }
            _ => {
                self.cursor += char_width(self.bytes, self.cursor);
                Some(self.make(TokenKind::Delim, 0, start, self.cursor))
            }
        }
    }

    fn stylus_brace_is_interpolation(&self, start: usize) -> bool {
        let mut cursor = start + 1;
        let mut nested = 0usize;
        let mut has_content = false;
        let mut has_top_level_whitespace = false;
        let mut has_expression_operator = false;
        let mut has_question = false;
        while let Some(byte) = self.bytes.get(cursor).copied() {
            match byte {
                b'\n' | b'\r' | b'\x0c' | b';' if nested == 0 => return false,
                b':' if nested == 0 && !has_question => return false,
                b'\t' | b' ' if nested == 0 => has_top_level_whitespace = true,
                b'?' if nested == 0 => {
                    has_content = true;
                    has_question = true;
                    has_expression_operator = true;
                }
                b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'=' | b'!' | b'&' | b'|'
                    if nested == 0 =>
                {
                    has_content = true;
                    has_expression_operator = true;
                }
                b'{' => {
                    has_content = true;
                    nested += 1;
                }
                b'}' if nested == 0 => {
                    return has_content && (!has_top_level_whitespace || has_expression_operator);
                }
                b'}' => nested -= 1,
                b'"' | b'\'' => {
                    has_content = true;
                    let quote = byte;
                    cursor += 1;
                    while let Some(inner) = self.bytes.get(cursor).copied() {
                        if inner == b'\\' {
                            cursor = cursor.saturating_add(2);
                            continue;
                        }
                        if inner == quote {
                            break;
                        }
                        if matches!(inner, b'\n' | b'\r' | b'\x0c') {
                            return false;
                        }
                        cursor += 1;
                    }
                }
                _ if !is_css_whitespace(byte) => has_content = true,
                _ => {}
            }
            cursor += 1;
        }
        false
    }
}

impl Iterator for Lexer<'_> {
    type Item = SyntaxToken;

    fn next(&mut self) -> Option<Self::Item> {
        let token = self.next_token()?;
        if !token.kind().is_trivia() {
            self.at_statement_boundary = matches!(
                token.kind(),
                TokenKind::Cdo
                    | TokenKind::Cdc
                    | TokenKind::LeftBrace
                    | TokenKind::RightBrace
                    | TokenKind::Semicolon
            );
        }
        Some(token)
    }
}

/// The CSS Syntax Module Level 3 ASCII whitespace set (tab, LF, FF, CR, space) — deliberately
/// narrower than JS `\s` (which additionally includes vertical tab `\x0B` plus a run of Unicode
/// space codepoints). The Svelte compat profile ([`crate::svelte_compat`]) builds its own
/// Unicode-aware whitespace predicate on top of this shared ASCII core rather than
/// re-enumerating the ASCII set a second time.
#[inline]
pub(crate) fn is_css_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

/// The byte length of a run of ASCII digits (`[0-9]*`) starting at `bytes[at]` — `0` when `at` is
/// not itself a digit. Shared by the general lexer's number/dimension/percentage scanning and the
/// Svelte compat profile's percentage and `nth-of` (`An+B`) digit runs.
#[inline]
pub(crate) fn ascii_digits_len(bytes: &[u8], at: usize) -> usize {
    let mut j = at;
    while bytes.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    j - at
}

/// The byte length of a CSS hex escape's numeral part (`\` + 1–6 ASCII hex digits) starting at
/// `bytes[at]` (which must be `\\`), or `None` when the following byte is not a hex digit. Shared
/// by [`Lexer::consume_escape`] (which then applies the ASCII CSS-Syntax trailing-whitespace rule)
/// and the Svelte compat profile's `REGEX_UNICODE_SEQUENCE` matcher (which applies the Unicode JS
/// `\s` trailing rule) — the two callers diverge only in which whitespace predicate closes the
/// escape, not in how the hex digits themselves are scanned.
#[inline]
pub(crate) fn hex_escape_digits_len(bytes: &[u8], at: usize) -> Option<usize> {
    verter_debug_assert_eq!(bytes.get(at), Some(&b'\\'));
    let hex_start = at + 1;
    let digits = ascii_hex_digits_len(bytes, hex_start);
    if digits == 0 {
        return None;
    }
    Some(1 + digits)
}

/// The byte length of a run of ASCII hex digits, capped at 6 (the CSS/JS unicode-escape numeral
/// limit) starting at `bytes[at]`.
#[inline]
fn ascii_hex_digits_len(bytes: &[u8], at: usize) -> usize {
    let mut j = at;
    while j - at < 6 && bytes.get(j).is_some_and(u8::is_ascii_hexdigit) {
        j += 1;
    }
    j - at
}

#[inline]
fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'\0' || byte >= 0x80
}

#[inline]
fn is_name(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit() || byte == b'-'
}

#[inline]
fn valid_escape(bytes: &[u8], offset: usize) -> bool {
    bytes.get(offset) == Some(&b'\\')
        && !bytes
            .get(offset + 1)
            .is_some_and(|byte| matches!(byte, b'\n' | b'\r' | b'\x0c'))
}

fn starts_identifier(bytes: &[u8], offset: usize) -> bool {
    let Some(first) = bytes.get(offset).copied() else {
        return false;
    };
    if is_name_start(first) {
        return true;
    }
    if first == b'\\' {
        return valid_escape(bytes, offset);
    }
    first == b'-'
        && bytes.get(offset + 1).is_some_and(|second| {
            is_name_start(*second)
                || *second == b'-'
                || (*second == b'\\' && valid_escape(bytes, offset + 1))
        })
}

fn starts_name_sequence(bytes: &[u8], offset: usize) -> bool {
    bytes.get(offset).is_some_and(|byte| is_name(*byte)) || valid_escape(bytes, offset)
}

fn starts_number(bytes: &[u8], offset: usize) -> bool {
    let first = bytes.get(offset).copied();
    let second = bytes.get(offset + 1).copied();
    let third = bytes.get(offset + 2).copied();
    match first {
        Some(b'+' | b'-') => {
            second.is_some_and(|byte| byte.is_ascii_digit())
                || (second == Some(b'.') && third.is_some_and(|byte| byte.is_ascii_digit()))
        }
        Some(b'.') => second.is_some_and(|byte| byte.is_ascii_digit()),
        Some(byte) => byte.is_ascii_digit(),
        None => false,
    }
}

/// The UTF-8 byte width of the character whose lead byte is `bytes[offset]`. Shared by the general
/// lexer's char-stepping and the Svelte compat profile's char-stepping — a `&str` guarantees every
/// char-boundary lead byte is well-formed, so the invalid-lead-byte fallback below is never
/// observed by either caller.
#[inline]
pub(crate) fn char_width(bytes: &[u8], offset: usize) -> usize {
    match bytes[offset] {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}
