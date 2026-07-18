//! Text-level v-model expression validation.
//!
//! Mirrors the official Vue compiler's `isMemberExpression` acceptance
//! (`@vue/compiler-core`): the official compiler parses the expression,
//! unwraps TS-only wrappers (`unwrapTSNode`: `as` / `satisfies` casts and
//! `!` non-null assertions — parentheses produce no node of their own), and
//! accepts when the result is a member expression, an optional-chaining
//! member expression, or an identifier other than `undefined`.
//!
//! Verter does not run a JS parser at this stage — the template parser is
//! byte-driven — so the same acceptance is implemented as a small
//! recursive-descent scan over the expression text:
//!
//! ```text
//! Expr    := Operand AsSuffix*
//! Operand := Atom Postfix*
//! Atom    := Identifier | '(' Expr ')' | '(' <opaque balanced> ')'
//! Postfix := '.' Ident | '?.' Ident | '?.' '[' … ']' | '[' … ']'
//!          | '(' … ')'                 // call — `fn(x).y` is a valid member
//!                                      // expression; a call as the FINAL
//!                                      // node is rejected
//!          | '!'                       // TS non-null — transparent
//! AsSuffix := ('as' | 'satisfies') TypeText
//! ```
//!
//! The opaque parenthesized atom mirrors the official compiler exactly: any
//! parenthesized group followed by a member postfix is a member expression
//! no matter what the group contains (`(a + b).c` is valid), while a bare
//! group is only valid when its content is itself a valid reference
//! (`(a + b)` is a binary expression — rejected).
//!
//! Known (harmless) divergences from the official compiler, all on inputs no
//! real template writes:
//! - `import.meta.url` / `this.#field` member chains are rejected here but
//!   pass its member check.
//! - JS comments are not lexed: inside computed members / call groups they
//!   are opaque content, and at the top level (`a /* x */ .b`) they reject —
//!   no real template writes comments inside a v-model expression.
//! - Some syntactically INVALID inputs are accepted leniently because group
//!   interiors are opaque (`(a +).x`, `a[)]`) and identifiers admit any
//!   non-ASCII byte (emoji). The official compiler rejects these at parse
//!   time; here the downstream type-check surfaces them instead — the lenient
//!   direction never false-rejects valid code.

/// What the (TS-unwrapped) expression parsed so far would be in the official
/// compiler's terms.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// A plain identifier — a valid v-model reference on its own.
    Ident,
    /// A node that is NOT a valid bare reference (keyword literal such as
    /// `this` / `true` / `undefined`, or an opaque parenthesized group), but
    /// becomes one when a member postfix is applied (`this.x`, `(a + b).c`).
    BareInvalid,
    /// Ends in `.x` / `?.x` / `[…]` — a member expression.
    Member,
    /// Ends in a call — never a valid v-model target.
    Call,
}

/// Reserved words that cannot start an expression at all (a parse error in
/// the official compiler). Contextual keywords (`let`, `await`, `yield`,
/// `async`, `as`, …) stay valid identifiers there too.
const HARD_RESERVED: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "return",
    "switch",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
];

/// Keyword literals: parse fine, but are not `Identifier`/`Member` nodes on
/// their own (`this` → `ThisExpression`, `true` → `BooleanLiteral`, …).
/// `undefined` parses as an `Identifier` but the official compiler rejects
/// it by name. All of them become valid once a member postfix applies.
const KEYWORD_LITERALS: &[&str] = &["this", "super", "true", "false", "null", "undefined"];

/// Check whether `expr` is a valid v-model expression per the official Vue
/// compiler's member-expression acceptance (see module docs).
pub(crate) fn is_member_expression(expr: &str) -> bool {
    let mut s = Scanner {
        bytes: expr.as_bytes(),
        pos: 0,
    };
    s.skip_ws();
    let Some(shape) = s.parse_expr() else {
        return false;
    };
    s.skip_ws();
    if s.pos != s.bytes.len() {
        // Leftover input: binary/ternary/sequence/assignment operators, etc.
        return false;
    }
    matches!(shape, Shape::Ident | Shape::Member)
}

struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

fn ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$' || b >= 0x80
}

fn ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    /// Maximal-munch identifier word starting at `pos`; `None` when the next
    /// byte is not an identifier start.
    fn parse_word(&mut self) -> Option<&'a str> {
        if !self.peek().is_some_and(ident_start) {
            return None;
        }
        let start = self.pos;
        while self.peek().is_some_and(ident_continue) {
            self.pos += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.pos]).ok()
    }

    /// The identifier word at `pos` without consuming it (empty when the
    /// next byte is not an identifier start).
    fn peek_word(&self) -> &'a str {
        let mut end = self.pos;
        if self.bytes.get(end).copied().is_some_and(ident_start) {
            while self.bytes.get(end).copied().is_some_and(ident_continue) {
                end += 1;
            }
        }
        std::str::from_utf8(&self.bytes[self.pos..end]).unwrap_or("")
    }

    /// Expr := Operand AsSuffix*
    fn parse_expr(&mut self) -> Option<Shape> {
        let shape = self.parse_operand()?;
        loop {
            let save = self.pos;
            self.skip_ws();
            let word = self.peek_word();
            if word == "as" || word == "satisfies" {
                self.pos += word.len();
                if !self.scan_type_text() {
                    return None;
                }
                // TS casts are transparent (`unwrapTSNode`): shape unchanged.
            } else {
                self.pos = save;
                return Some(shape);
            }
        }
    }

    /// Operand := Atom Postfix*
    fn parse_operand(&mut self) -> Option<Shape> {
        let mut shape = self.parse_atom()?;
        loop {
            let save = self.pos;
            self.skip_ws();
            match self.peek() {
                Some(b'.') => {
                    self.pos += 1;
                    self.skip_ws();
                    // Property names may be any identifier word, including
                    // reserved words (`a.new` is valid JS).
                    self.parse_word()?;
                    shape = Shape::Member;
                }
                Some(b'?') if self.peek_at(1) == Some(b'.') => {
                    self.pos += 2;
                    self.skip_ws();
                    match self.peek() {
                        Some(b'[') => {
                            self.scan_bracket_group()?;
                            shape = Shape::Member;
                        }
                        Some(b'(') => {
                            // `a?.(…)` — an optional call: rejected as the
                            // FINAL node (like any call), but `fn?.().x` is a
                            // member expression the official compiler accepts.
                            self.scan_balanced_group(b'(', b')', true)?;
                            shape = Shape::Call;
                        }
                        Some(b) if ident_start(b) => {
                            self.parse_word()?;
                            shape = Shape::Member;
                        }
                        _ => return None, // syntax error
                    }
                }
                Some(b'[') => {
                    self.scan_bracket_group()?;
                    shape = Shape::Member;
                }
                Some(b'(') => {
                    self.scan_balanced_group(b'(', b')', true)?;
                    shape = Shape::Call;
                }
                Some(b'!') => {
                    // TS non-null assertion — transparent (`unwrapTSNode`).
                    self.pos += 1;
                }
                _ => {
                    self.pos = save;
                    return Some(shape);
                }
            }
        }
    }

    /// Atom := Identifier | StringLiteral | '(' Expr ')' | '(' <opaque> ')'
    fn parse_atom(&mut self) -> Option<Shape> {
        match self.peek()? {
            b'(' => {
                let save = self.pos;
                self.pos += 1;
                self.skip_ws();
                if let Some(shape) = self.parse_expr() {
                    self.skip_ws();
                    if self.peek() == Some(b')') {
                        // Parentheses are transparent — they produce no node of their own.
                        self.pos += 1;
                        return Some(shape);
                    }
                }
                // The group content is not a reference expression — treat it
                // as an opaque balanced group (still a parseable expression): only
                // valid once a member postfix applies (`(a + b).c`). An EMPTY
                // group is a parse error (`().x`). The speculative parse then
                // rescan is O(depth × len) on pathological nesting — fine for
                // attribute-sized input.
                self.pos = save;
                let close_at = self.scan_balanced_group(b'(', b')', true)?;
                if self.bytes[save + 1..close_at]
                    .iter()
                    .all(|b| b.is_ascii_whitespace())
                {
                    return None;
                }
                Some(Shape::BareInvalid)
            }
            // A string/template literal is a valid member-expression ROOT
            // (`'str'.length` passes the official compiler's member check) —
            // invalid bare, like the keyword literals.
            b'\'' | b'"' | b'`' => {
                self.skip_string()?;
                Some(Shape::BareInvalid)
            }
            b if ident_start(b) => {
                let word = self.parse_word()?;
                if HARD_RESERVED.contains(&word) {
                    None
                } else if KEYWORD_LITERALS.contains(&word) {
                    Some(Shape::BareInvalid)
                } else {
                    Some(Shape::Ident)
                }
            }
            _ => None,
        }
    }

    /// Computed member access `[…]` — the content is a full expression in
    /// the official compiler and the overall node is a member expression regardless of what
    /// it contains, so the content is scanned as an opaque balanced group.
    /// Empty content (`a[]`) is a parse error.
    fn scan_bracket_group(&mut self) -> Option<()> {
        let content_start = self.pos + 1;
        let content_end_excl = self.scan_balanced_group(b'[', b']', false)?;
        let content = &self.bytes[content_start..content_end_excl];
        if content.iter().all(|b| b.is_ascii_whitespace()) {
            return None;
        }
        Some(())
    }

    /// Consume a balanced `open…close` group starting at `pos` (which must
    /// be at `open`). Tracks nesting of the same delimiter pair and skips
    /// string/template literals. Returns the offset of the closing
    /// delimiter. `allow_empty` distinguishes call args (`fn()` fine) from
    /// computed members (`a[]` invalid — checked by the caller).
    fn scan_balanced_group(&mut self, open: u8, close: u8, _allow_empty: bool) -> Option<usize> {
        debug_assert_eq!(self.peek(), Some(open));
        self.pos += 1;
        let mut depth = 1usize;
        while let Some(b) = self.peek() {
            match b {
                b'\'' | b'"' | b'`' => {
                    self.skip_string()?;
                    continue;
                }
                _ if b == open => depth += 1,
                _ if b == close => {
                    depth -= 1;
                    if depth == 0 {
                        let close_at = self.pos;
                        self.pos += 1;
                        return Some(close_at);
                    }
                }
                _ => {}
            }
            self.pos += 1;
        }
        None // unterminated
    }

    /// Skip a string or template literal starting at the opening quote.
    /// Template literals are treated as opaque up to the closing backtick.
    fn skip_string(&mut self) -> Option<()> {
        let quote = self.peek()?;
        self.pos += 1;
        while let Some(b) = self.peek() {
            if b == b'\\' {
                self.pos += 2;
                continue;
            }
            self.pos += 1;
            if b == quote {
                return Some(());
            }
        }
        None // unterminated
    }

    /// Scan the type text of an `as` / `satisfies` suffix.
    ///
    /// Babel parses the TYPE grammar here, so an expression operator after
    /// the type re-enters expression context: `a as string + b` is
    /// `(a as string) + b` — a `BinaryExpression` the official compiler
    /// rejects. The scan therefore tracks whether the next depth-0 token must
    /// be a type ATOM (start, after `|`/`&`/`.`/`extends`/`?`/`:`/`=>`) or a
    /// CONTINUATION (after an atom), and STOPS — leaving leftover input the
    /// caller rejects — at anything that cannot continue a TS type:
    /// expression operators (`+ - * / % ! ~ ^ ; =`), `?` outside an
    /// `extends` conditional, a second consecutive `|`/`&`, an unexpected
    /// word, a top-level `,` or non-`=>` `>`, or a `)` closing an enclosing
    /// group. A top-level `as` / `satisfies` word in continuation position
    /// starts the next chained cast (`a as unknown as string`). Content
    /// inside `(` `[` `{` `<` groups stays opaque. Returns `false` for
    /// unbalanced groups, stray closers, an empty type, or a type ending
    /// where an atom is still expected (`a as A |`).
    fn scan_type_text(&mut self) -> bool {
        /// Type-operator words that keep expecting a type atom after them.
        const TYPE_PREFIX_WORDS: &[&str] = &[
            "keyof", "typeof", "readonly", "infer", "unique", "abstract", "new", "asserts",
        ];
        self.skip_ws();
        let start = self.pos;
        let mut depth = 0usize;
        let mut prev_nonspace = 0u8;
        let mut expect_atom = true;
        let mut prev_was_pipe = false;
        // Open `extends` conditionals: each accepts one `?` and one `:`.
        let mut extends_pending = 0usize;
        let mut branch_pending = 0usize;
        while let Some(b) = self.peek() {
            if depth > 0 {
                // Inside a balanced group the content is opaque.
                match b {
                    b'\'' | b'"' | b'`' => {
                        if self.skip_string().is_none() {
                            return false;
                        }
                        prev_nonspace = b'"';
                        continue;
                    }
                    b'(' | b'[' | b'{' | b'<' => depth += 1,
                    b']' | b'}' => depth -= 1,
                    b')' => depth -= 1,
                    // `=>` in function types never closes a `<`.
                    b'>' if prev_nonspace != b'=' => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    // The atom is the whole group (`(x) => …` handles the
                    // arrow via `=` below).
                    expect_atom = false;
                    prev_was_pipe = false;
                }
                if !b.is_ascii_whitespace() {
                    prev_nonspace = b;
                }
                self.pos += 1;
                continue;
            }
            match b {
                _ if b.is_ascii_whitespace() => {}
                b'\'' | b'"' | b'`' => {
                    if !expect_atom {
                        break; // a literal cannot continue a type
                    }
                    if self.skip_string().is_none() {
                        return false;
                    }
                    prev_nonspace = b'"';
                    expect_atom = false;
                    prev_was_pipe = false;
                    continue;
                }
                b'(' | b'{' if !expect_atom => break, // `T (…)` / `T {…}`
                b'(' | b'[' | b'{' | b'<' => depth += 1,
                b')' => break,               // closes an enclosing group — not ours
                b']' | b'}' => return false, // stray closer
                b'>' => {
                    if prev_nonspace == b'=' {
                        // `=>` — function-type arrow; the return type follows.
                        expect_atom = true;
                    } else {
                        break; // comparison operator — leftover input
                    }
                }
                b',' => break, // sequence expression — leftover
                b'=' if self.peek_at(1) == Some(b'>') => {} // arrow, `>` above
                b'|' | b'&' => {
                    if expect_atom && prev_was_pipe {
                        break; // `||` / `&&` — logical operator
                    }
                    expect_atom = true;
                    prev_was_pipe = true;
                }
                b'.' => {
                    if expect_atom {
                        break; // a type cannot start with `.`
                    }
                    expect_atom = true; // qualified name continues (`Ns.Inner`)
                }
                b'?' => {
                    if extends_pending == 0 {
                        break; // expression ternary — leftover input
                    }
                    extends_pending -= 1;
                    branch_pending += 1;
                    expect_atom = true;
                }
                b':' => {
                    if branch_pending == 0 {
                        break; // expression ternary branch — leftover input
                    }
                    branch_pending -= 1;
                    expect_atom = true;
                }
                b'-' if expect_atom => {} // negative literal type (`-1`)
                b'0'..=b'9' => {
                    if !expect_atom && prev_nonspace != b'-' && !ident_continue(prev_nonspace) {
                        break;
                    }
                    expect_atom = false;
                    prev_was_pipe = false;
                }
                b if ident_start(b) => {
                    let word = self.peek_word();
                    if !expect_atom {
                        match word {
                            // Chained cast — the caller parses the next suffix.
                            "as" | "satisfies" => break,
                            // Conditional type / type predicate continuations.
                            "extends" => {
                                extends_pending += 1;
                                expect_atom = true;
                            }
                            "is" => expect_atom = true,
                            // Any other word cannot continue a type
                            // (`a as T U` is not a type).
                            _ => break,
                        }
                    } else if TYPE_PREFIX_WORDS.contains(&word) {
                        // Still expecting the operand type.
                    } else {
                        expect_atom = false;
                        prev_was_pipe = false;
                    }
                    self.pos += word.len();
                    prev_nonspace = *word.as_bytes().last().unwrap_or(&0);
                    continue;
                }
                // Expression operators and anything else that cannot appear
                // at the top level of a TS type: stop, leaving leftover
                // input (`a as string + b` → BinaryExpression → invalid).
                _ => break,
            }
            if !b.is_ascii_whitespace() {
                prev_nonspace = b;
            }
            self.pos += 1;
        }
        depth == 0
            && !expect_atom
            && self.bytes[start..self.pos]
                .iter()
                .any(|b| !b.is_ascii_whitespace())
    }
}
