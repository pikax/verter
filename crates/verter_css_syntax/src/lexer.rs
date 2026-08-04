use memchr::memchr;

use crate::dialect::{css, less, scss, CssDialect};
use crate::parser::CssSource;
use crate::token::{css_identifier_eq_ignore_ascii_case, SyntaxToken, TokenFlags, TokenKind};

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

    fn absolute(&self, local: usize) -> u32 {
        let local = u32::try_from(local).expect("CssSource validates its local span domain");
        self.source
            .origin()
            .checked_add(local)
            .expect("CssSource validates origin plus source length")
    }

    #[inline]
    fn make(&self, kind: TokenKind, flags: u16, start: usize, end: usize) -> SyntaxToken {
        SyntaxToken::new(kind, flags, self.absolute(start), self.absolute(end))
    }

    fn consume_whitespace(&mut self) -> SyntaxToken {
        let start = self.cursor;
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| is_css_whitespace(*byte))
        {
            self.cursor += 1;
        }
        self.make(
            TokenKind::Whitespace,
            TokenFlags::TRIVIA,
            start,
            self.cursor,
        )
    }

    fn consume_comment(&mut self) -> SyntaxToken {
        let start = self.cursor;
        self.cursor += 2;
        let mut flags = TokenFlags::TRIVIA;
        loop {
            let Some(relative) = memchr(b'*', &self.bytes[self.cursor..]) else {
                self.cursor = self.bytes.len();
                flags |= TokenFlags::UNTERMINATED;
                break;
            };
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
                            self.cursor += 2;
                            if self.bytes.get(self.cursor) == Some(&b'\n') {
                                self.cursor += 1;
                            }
                        }
                        Some(b'\n' | b'\x0c') => {
                            self.cursor += 2;
                        }
                        _ => {
                            self.consume_escape();
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
        let mut flags = 0u16;
        while self.cursor < self.bytes.len() {
            if is_name(self.bytes[self.cursor]) {
                self.cursor += char_width(self.bytes, self.cursor);
            } else if valid_escape(self.bytes, self.cursor) {
                flags |= TokenFlags::CONTAINS_ESCAPE;
                self.consume_escape();
            } else {
                break;
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
        while self.bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
            self.cursor += 1;
        }
        let mut integer = true;
        if self.bytes.get(self.cursor) == Some(&b'.')
            && self
                .bytes
                .get(self.cursor + 1)
                .is_some_and(u8::is_ascii_digit)
        {
            integer = false;
            self.cursor += 2;
            while self.bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                self.cursor += 1;
            }
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
                while self.bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                    self.cursor += 1;
                }
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
        debug_assert_eq!(self.bytes.get(self.cursor), Some(&b'\\'));
        self.cursor += 1;
        if self.cursor >= self.bytes.len() {
            return;
        }
        if self.bytes[self.cursor].is_ascii_hexdigit() {
            let mut digits = 0usize;
            while self.cursor < self.bytes.len()
                && self.bytes[self.cursor].is_ascii_hexdigit()
                && digits < 6
            {
                self.cursor += 1;
                digits += 1;
            }
            if self
                .bytes
                .get(self.cursor)
                .is_some_and(|byte| is_css_whitespace(*byte))
            {
                self.cursor += 1;
                if self.bytes[self.cursor - 1] == b'\r'
                    && self.bytes.get(self.cursor) == Some(&b'\n')
                {
                    self.cursor += 1;
                }
            }
        } else {
            self.cursor += char_width(self.bytes, self.cursor);
        }
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
        if self.dialect == CssDialect::Scss
            && byte == scss::VARIABLE_PREFIX
            && starts_identifier(self.bytes, start + 1)
        {
            return Some(self.consume_prefixed_name(TokenKind::ScssVariable));
        }
        if self.dialect == CssDialect::Scss
            && self.bytes[start..].starts_with(scss::INTERPOLATION_PREFIX)
        {
            self.cursor += 2;
            return Some(self.make(
                TokenKind::ScssInterpolationStart,
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
                Some(self.make(TokenKind::LeftBrace, 0, start, self.cursor))
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

#[inline]
fn is_css_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
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

#[inline]
fn char_width(bytes: &[u8], offset: usize) -> usize {
    match bytes[offset] {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}
