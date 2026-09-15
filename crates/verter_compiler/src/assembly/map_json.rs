//! JSON reader whose admissible domain is this crate's, not a parser's.
//!
//! "Parses as JSON" differs across languages: out-of-range numbers,
//! unpaired surrogates, repeated object members (RFC 8259 undefined).
//! This reader records what it saw; the caller applies domain rules in
//! specified order (numbers before strings, both before any member).
//! Delegating any of those to a parser is how two conforming
//! implementations disagree.

/// A parsed JSON string, plus whether it survived unescaping as well-formed
/// Unicode. An unpaired surrogate escape is replaced by `U+FFFD` and flagged
/// rather than failing the parse, so the caller can report it at its own place
/// in the validation order.
#[derive(Debug, Clone)]
pub(crate) struct JsonStr {
    pub(crate) value: String,
    pub(crate) well_formed: bool,
}

/// A parsed JSON value. Object members are kept in document order and
/// duplicates are RETAINED, because collapsing them is the decision this
/// reader refuses to make on the caller's behalf.
#[derive(Debug, Clone)]
pub(crate) enum Json {
    Null,
    // The payload is unread — no source-map member is a boolean — but a model
    // that cannot tell `true` from `false` is not a model of the document.
    #[allow(dead_code)]
    Bool(bool),
    /// The lexeme converted to IEEE-754 binary64 under round-ties-to-even. May
    /// be infinite: that is a domain question, not a parse question.
    Number(f64),
    Str(JsonStr),
    Array(Vec<Json>),
    Object(Vec<(JsonStr, Json)>),
}

impl Json {
    /// The value of `name`'s FIRST declaration, or `None`. Only meaningful
    /// once duplicate members have been ruled out.
    pub(crate) fn member(&self, name: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members
                .iter()
                .find(|(key, _)| key.value == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(string) => Some(&string.value),
            _ => None,
        }
    }

    pub(crate) fn as_number(&self) -> Option<f64> {
        match self {
            Json::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// The first number in document order that does not denote a FINITE
    /// binary64 value, if any. Object member names carry no number, so a
    /// member contributes its value's numbers only.
    pub(crate) fn first_non_finite_number(&self) -> Option<f64> {
        match self {
            Json::Number(value) => (!value.is_finite()).then_some(*value),
            Json::Array(items) => items.iter().find_map(Json::first_non_finite_number),
            Json::Object(members) => members
                .iter()
                .find_map(|(_, value)| value.first_non_finite_number()),
            _ => None,
        }
    }

    /// Whether some string in the document — INCLUDING an object member name —
    /// failed to unescape as well-formed Unicode, scanning in document order.
    pub(crate) fn has_ill_formed_string(&self) -> bool {
        match self {
            Json::Str(string) => !string.well_formed,
            Json::Array(items) => items.iter().any(Json::has_ill_formed_string),
            Json::Object(members) => members
                .iter()
                .any(|(key, value)| !key.well_formed || value.has_ill_formed_string()),
            _ => false,
        }
    }

    /// Whether some object in the document declares one member name twice.
    pub(crate) fn has_duplicate_member(&self) -> bool {
        match self {
            Json::Array(items) => items.iter().any(Json::has_duplicate_member),
            Json::Object(members) => {
                for (index, (key, _)) in members.iter().enumerate() {
                    if members[..index]
                        .iter()
                        .any(|(earlier, _)| earlier.value == key.value)
                    {
                        return true;
                    }
                }
                members
                    .iter()
                    .any(|(_, value)| value.has_duplicate_member())
            }
            _ => false,
        }
    }
}

/// RFC 8259 §9 permits an implementation to bound nesting depth. A source map
/// is two levels deep; this ceiling exists only so a pathological document
/// fails as unparseable rather than by exhausting the stack.
const MAX_DEPTH: u32 = 128;

/// Parse `input` as RFC 8259 JSON. `Err(())` means the bytes are not a JSON
/// document at all — never that they are a JSON document this crate dislikes.
pub(crate) fn parse(input: &str) -> Result<Json, ()> {
    let mut reader = Reader {
        bytes: input.as_bytes(),
        at: 0,
        depth: 0,
    };
    reader.skip_whitespace();
    let value = reader.value()?;
    reader.skip_whitespace();
    if reader.at != reader.bytes.len() {
        return Err(());
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: u32,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), ()> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), ()> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn value(&mut self) -> Result<Json, ()> {
        match self.peek().ok_or(())? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(Json::Str(self.string()?)),
            b't' => self.literal("true").map(|()| Json::Bool(true)),
            b'f' => self.literal("false").map(|()| Json::Bool(false)),
            b'n' => self.literal("null").map(|()| Json::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(()),
        }
    }

    fn enter(&mut self) -> Result<(), ()> {
        self.depth += 1;
        (self.depth <= MAX_DEPTH).then_some(()).ok_or(())
    }

    fn object(&mut self) -> Result<Json, ()> {
        self.enter()?;
        self.expect(b'{')?;
        let mut members = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.value()?;
            members.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(()),
            }
        }
    }

    fn array(&mut self) -> Result<Json, ()> {
        self.enter()?;
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(()),
            }
        }
    }

    fn string(&mut self) -> Result<JsonStr, ()> {
        self.expect(b'"')?;
        let mut value = String::new();
        let mut well_formed = true;
        loop {
            match self.peek().ok_or(())? {
                b'"' => {
                    self.at += 1;
                    return Ok(JsonStr { value, well_formed });
                }
                b'\\' => {
                    self.at += 1;
                    match self.peek().ok_or(())? {
                        b'"' => (value.push('"'), self.at += 1).0,
                        b'\\' => (value.push('\\'), self.at += 1).0,
                        b'/' => (value.push('/'), self.at += 1).0,
                        b'b' => (value.push('\u{8}'), self.at += 1).0,
                        b'f' => (value.push('\u{c}'), self.at += 1).0,
                        b'n' => (value.push('\n'), self.at += 1).0,
                        b'r' => (value.push('\r'), self.at += 1).0,
                        b't' => (value.push('\t'), self.at += 1).0,
                        b'u' => {
                            self.at += 1;
                            let first = self.hex4()?;
                            let scalar = if (0xD800..0xDC00).contains(&first) {
                                // A high surrogate must be followed by its low
                                // half. Anything else is an unpaired surrogate.
                                self.paired_low_surrogate()?.map(|low| {
                                    0x1_0000
                                        + ((u32::from(first) - 0xD800) << 10)
                                        + (u32::from(low) - 0xDC00)
                                })
                            } else if (0xDC00..0xE000).contains(&first) {
                                // A lone low surrogate.
                                None
                            } else {
                                Some(u32::from(first))
                            };
                            match scalar.and_then(char::from_u32) {
                                Some(character) => value.push(character),
                                None => {
                                    well_formed = false;
                                    value.push('\u{FFFD}');
                                }
                            }
                        }
                        _ => return Err(()),
                    }
                }
                // An unescaped control character is a syntax error.
                byte if byte < 0x20 => return Err(()),
                _ => {
                    // Copy one whole UTF-8 character; the input is already
                    // valid UTF-8, so a literal unpaired surrogate cannot occur.
                    let rest = std::str::from_utf8(&self.bytes[self.at..]).map_err(|_| ())?;
                    let character = rest.chars().next().ok_or(())?;
                    value.push(character);
                    self.at += character.len_utf8();
                }
            }
        }
    }

    /// A `\uXXXX` low surrogate immediately following, consumed only when it is
    /// really there — an unpaired high surrogate must not swallow whatever
    /// follows it.
    fn paired_low_surrogate(&mut self) -> Result<Option<u16>, ()> {
        if self.bytes[self.at..].starts_with(b"\\u") {
            let resume = self.at;
            self.at += 2;
            let low = self.hex4()?;
            if (0xDC00..0xE000).contains(&low) {
                return Ok(Some(low));
            }
            self.at = resume;
        }
        Ok(None)
    }

    fn hex4(&mut self) -> Result<u16, ()> {
        let digits = self.bytes.get(self.at..self.at + 4).ok_or(())?;
        let text = std::str::from_utf8(digits).map_err(|_| ())?;
        if !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(());
        }
        self.at += 4;
        u16::from_str_radix(text, 16).map_err(|_| ())
    }

    fn number(&mut self) -> Result<Json, ()> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek().ok_or(())? {
            b'0' => self.at += 1,
            b'1'..=b'9' => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.at += 1;
                }
            }
            _ => return Err(()),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        let lexeme = std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| ())?;
        // `f64::from_str` is correctly rounded (round-ties-to-even) and yields
        // an infinity on overflow rather than an error, which is exactly the
        // conversion the domain rule is stated over.
        lexeme.parse::<f64>().map(Json::Number).map_err(|_| ())
    }
}
