use std::borrow::Cow;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Whitespace = 0,
    Comment,
    LineComment,
    Cdo,
    Cdc,
    Ident,
    Function,
    AtKeyword,
    Hash,
    String,
    BadString,
    Url,
    BadUrl,
    Delim,
    Number,
    Percentage,
    Dimension,
    Colon,
    Semicolon,
    Comma,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    ScssVariable,
    ScssInterpolationStart,
    LessVariable,
    LessInterpolationStart,
    LessEscapedString,
    UnicodeRange,
    StylusInterpolationStart,
}

impl TokenKind {
    #[inline]
    pub const fn raw(self) -> u16 {
        self as u16
    }

    pub const fn from_raw(raw: u16) -> Self {
        match raw {
            0 => Self::Whitespace,
            1 => Self::Comment,
            2 => Self::LineComment,
            3 => Self::Cdo,
            4 => Self::Cdc,
            5 => Self::Ident,
            6 => Self::Function,
            7 => Self::AtKeyword,
            8 => Self::Hash,
            9 => Self::String,
            10 => Self::BadString,
            11 => Self::Url,
            12 => Self::BadUrl,
            13 => Self::Delim,
            14 => Self::Number,
            15 => Self::Percentage,
            16 => Self::Dimension,
            17 => Self::Colon,
            18 => Self::Semicolon,
            19 => Self::Comma,
            20 => Self::LeftParen,
            21 => Self::RightParen,
            22 => Self::LeftBracket,
            23 => Self::RightBracket,
            24 => Self::LeftBrace,
            25 => Self::RightBrace,
            26 => Self::ScssVariable,
            27 => Self::ScssInterpolationStart,
            28 => Self::LessVariable,
            29 => Self::LessInterpolationStart,
            30 => Self::LessEscapedString,
            31 => Self::UnicodeRange,
            32 => Self::StylusInterpolationStart,
            _ => Self::Delim,
        }
    }

    #[inline]
    pub const fn is_trivia(self) -> bool {
        matches!(self, Self::Whitespace | Self::Comment | Self::LineComment)
    }

    #[inline]
    pub const fn is_opening_delimiter(self) -> bool {
        matches!(
            self,
            Self::Function
                | Self::LeftParen
                | Self::LeftBracket
                | Self::LeftBrace
                | Self::ScssInterpolationStart
                | Self::LessInterpolationStart
                | Self::StylusInterpolationStart
        )
    }

    #[inline]
    pub const fn is_closing_delimiter(self) -> bool {
        matches!(
            self,
            Self::RightParen | Self::RightBracket | Self::RightBrace
        )
    }
}

pub struct TokenFlags;

impl TokenFlags {
    pub const TRIVIA: u16 = 1 << 0;
    pub const CONTAINS_ESCAPE: u16 = 1 << 1;
    pub const ID_HASH: u16 = 1 << 2;
    pub const NUMBER_INTEGER: u16 = 1 << 3;
    pub const UNTERMINATED: u16 = 1 << 4;
    pub const DIALECT_EXTENSION: u16 = 1 << 5;
    pub const CONTAINS_NEWLINE: u16 = 1 << 6;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyntaxToken {
    pub kind: u16,
    pub flags: u16,
    pub start: u32,
    pub end: u32,
}

const _: [(); 12] = [(); std::mem::size_of::<SyntaxToken>()];

impl SyntaxToken {
    #[inline]
    pub const fn new(kind: TokenKind, flags: u16, start: u32, end: u32) -> Self {
        Self {
            kind: kind.raw(),
            flags,
            start,
            end,
        }
    }

    #[inline]
    pub const fn kind(self) -> TokenKind {
        TokenKind::from_raw(self.kind)
    }

    #[inline]
    pub const fn contains_escape(self) -> bool {
        self.flags & TokenFlags::CONTAINS_ESCAPE != 0
    }
}

pub type DecodedName<'a> = Cow<'a, str>;

pub fn decode_css_identifier(raw: &str) -> Result<DecodedName<'_>, std::str::Utf8Error> {
    if !raw.as_bytes().contains(&b'\\') && !raw.as_bytes().contains(&b'\0') {
        return Ok(Cow::Borrowed(raw));
    }

    let mut output = String::with_capacity(raw.len());
    let mut cursor = 0usize;
    while cursor < raw.len() {
        if let Some((decoded, next)) = next_preprocessed_code_point(raw, cursor) {
            output.push(decoded);
            cursor = next;
        } else {
            cursor = consume_newline(raw.as_bytes(), cursor + 1);
        }
    }
    Ok(Cow::Owned(output))
}

pub fn css_identifier_eq_ignore_ascii_case(raw: &str, expected: &str) -> bool {
    let mut raw_cursor = 0usize;
    let mut expected_chars = expected.chars();
    while raw_cursor < raw.len() {
        let Some((decoded, next)) = next_preprocessed_code_point(raw, raw_cursor) else {
            raw_cursor = consume_newline(raw.as_bytes(), raw_cursor + 1);
            continue;
        };
        let Some(expected) = expected_chars.next() else {
            return false;
        };
        if !decoded.eq_ignore_ascii_case(&expected) {
            return false;
        }
        raw_cursor = next;
    }
    expected_chars.next().is_none()
}

pub(crate) fn css_identifier_starts_with(raw: &str, expected_prefix: &str) -> bool {
    let mut raw_cursor = 0usize;
    for expected in expected_prefix.chars() {
        let decoded = loop {
            if raw_cursor >= raw.len() {
                return false;
            }
            if let Some((decoded, next)) = next_preprocessed_code_point(raw, raw_cursor) {
                raw_cursor = next;
                break decoded;
            }
            raw_cursor = consume_newline(raw.as_bytes(), raw_cursor + 1);
        };
        if decoded != expected {
            return false;
        }
    }
    true
}

fn next_preprocessed_code_point(raw: &str, cursor: usize) -> Option<(char, usize)> {
    let bytes = raw.as_bytes();
    if bytes[cursor] == b'\0' {
        return Some(('\u{fffd}', cursor + 1));
    }
    if bytes[cursor] != b'\\' {
        let decoded = raw[cursor..].chars().next().unwrap_or('\u{fffd}');
        return Some((decoded, cursor + decoded.len_utf8()));
    }

    let escaped = cursor + 1;
    if escaped >= bytes.len() {
        return Some(('\u{fffd}', escaped));
    }
    if is_newline_start(bytes, escaped) {
        return None;
    }
    if !bytes[escaped].is_ascii_hexdigit() {
        let decoded = raw[escaped..].chars().next().unwrap_or('\u{fffd}');
        return Some((decoded, escaped + decoded.len_utf8()));
    }

    let mut end = escaped;
    let mut digits = 0usize;
    while end < bytes.len() && bytes[end].is_ascii_hexdigit() && digits < 6 {
        end += 1;
        digits += 1;
    }
    let value = u32::from_str_radix(&raw[escaped..end], 16).unwrap_or(0xfffd);
    let decoded = char::from_u32(value)
        .filter(|decoded| *decoded != '\0' && !(0xd800..=0xdfff).contains(&value))
        .unwrap_or('\u{fffd}');
    if end < bytes.len() && is_css_whitespace(bytes[end]) {
        end = consume_one_whitespace(bytes, end);
    }
    Some((decoded, end))
}

#[inline]
fn is_css_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

#[inline]
fn is_newline_start(bytes: &[u8], offset: usize) -> bool {
    matches!(bytes[offset], b'\n' | b'\r' | b'\x0c')
}

#[inline]
fn consume_newline(bytes: &[u8], offset: usize) -> usize {
    if bytes[offset] == b'\r' && bytes.get(offset + 1) == Some(&b'\n') {
        offset + 2
    } else {
        offset + 1
    }
}

#[inline]
fn consume_one_whitespace(bytes: &[u8], offset: usize) -> usize {
    consume_newline(bytes, offset)
}
