//! A linear pre-scan bounding how deeply a script's syntax tree nests.
//!
//! oxc's parser is a recursive descent that spends native stack per level
//! of nesting. [`scan`] bounds, from above and in one pass over the source
//! text, how deeply the syntax tree nests, so the parse can be given a
//! stack its source cannot exhaust ([`super::parse_stack_bytes`]). It
//! refuses nothing.
//!
//! The measure is token-level and over-counts. Every open bracket (`(`,
//! `[`, `{`, a template's `${`, a JSX tag or element) is one level, and
//! within the innermost one every operator, member access, prefix keyword,
//! arrow and chained call or index since the last `,`, `;` or
//! statement-ending line break adds one more: a chain `a + b + c`,
//! `x.y.z`, `!!!x`, `f()()()`, `a ? b : c ? d : e` or `Box<Box<1>>` nests
//! its syntax tree once per link although no bracket does. A statement or
//! a comma-separated element starts over, so siblings never add up; a
//! comma inside an expression's open `<` does not, as it may separate the
//! nested type arguments of one list. A statement that is the body of a
//! braceless `if`, `while`, `for`, `with`, `do`, `else` or label nests one
//! level under its head, and an `else` continues its `if` past the `;`
//! that ends the `if`'s body.
//!
//! A type union or intersection (`'a' | 'b' | …`) is one node however many
//! members it has, where the same tokens in an expression nest once per
//! operator; `|` and `&` are not counted where the source can only be a
//! type: a type alias's right-hand side, an interface body, an annotation
//! after `:` (a parameter's, a variable's, a class member's or a return
//! type) up to an initializer or, for a return type, the function's body
//! or an arrow's `=>`, and a declaration file outside its enum bodies and
//! initializers.
//!
//! Strings, comments (HTML-like ones where oxc reads them), regular
//! expressions, template text and JSX text are skipped, each to where the
//! lexical grammar ends it. Where the lexical grammar needs the parser to
//! tell a regular expression from a division, the scan decides from the
//! previous token: a keyword read as a property name, a type operator's
//! word and an object literal's `}` precede a division, a keyword that
//! expects an expression and a statement's `}` a regular expression. Where
//! the previous token cannot decide (`await`, `yield` and `of`, each also
//! an identifier, and the `}` of a body the scan reads as a function or
//! class expression's, which a declaration's may be), the rest of the
//! source is bounded by its length, a level per byte. A regular
//! expression's own brackets are still counted, and a closing bracket
//! that matches no open one is ignored.

use oxc_span::SourceType;
use oxc_syntax::identifier::{
    is_identifier_part_unicode, is_identifier_start_unicode, is_irregular_whitespace,
};
use oxc_syntax::line_terminator::is_irregular_line_terminator;

/// Where a source first nests deeper than a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestingExceeded {
    /// Byte offset of the token that went past the limit.
    pub offset: u32,
}

/// The deepest nesting [`scan`] measured over a whole source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nesting {
    pub depth: u32,
}

/// Measure `source`, stopping at the first token deeper than `limit`.
pub fn scan(source: &str, source_type: SourceType, limit: u32) -> Result<Nesting, NestingExceeded> {
    Scanner::new(source, source_type, limit).run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    /// The file itself: statements.
    Root,
    Paren,
    Bracket,
    /// A block, object literal, class or interface body.
    Brace,
    /// A template literal's text.
    Template,
    /// A template literal's `${ … }`.
    TemplateExpr,
    /// A JSX tag between `<` and `>` / `/>`.
    JsxTag,
    /// A JSX closing tag between `</` and `>`.
    JsxClosingTag,
    /// A JSX element's children, between its tags.
    JsxChildren,
    /// A type argument or parameter list, between `<` and `>`, where the
    /// source is a type.
    Angle,
    /// A JSX `{ … }` in a tag or among children.
    JsxExpr,
}

/// A `function` or `class` whose body has not opened yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingBody {
    /// A declaration: a statement follows its body.
    Declaration { class: bool },
    /// An expression: its body's `}` ends an operand.
    Expression { class: bool },
}

#[derive(Debug, Clone, Copy)]
struct Group {
    kind: GroupKind,
    /// The depth this group's own tokens start from.
    base: u32,
    /// Links counted in the current segment (since the last `,`, `;` or
    /// statement-ending line break).
    chain: u32,
    /// The level of the body of the current statement's innermost braceless
    /// head (`if (…) body`, a label's `l: body`).
    head_chain: u32,
    /// [`Self::head_chain`] of the statement that ended last: an `else`
    /// after it continues its `if` from there.
    ended_chain: u32,
    /// Whether the group's segments start as type-only syntax.
    types: bool,
    /// Whether the rest of the current segment is type-only syntax.
    segment_types: bool,
    /// The segment's type-only syntax is a return type: an arrow's `=>` or
    /// the function's body ends it.
    return_annotation: bool,
    /// How the current segment began, for the declarations that switch
    /// type-only syntax on or off.
    head: Head,
    /// Angle-bracket depth inside a type alias's head (`type A<T = B> =`).
    head_angles: u32,
    /// `<`s of the segment, read where the source is not a type, still
    /// waiting for their `>`: a comma between them may separate type
    /// arguments, which does not start the chain over.
    angles: u32,
    /// Conditional `?`s in the segment still waiting for their `:`.
    questions: u32,
    /// The segment's links up to its last one that is not a member access
    /// or a chained call or index: where a type union's members start.
    member_base: u32,
    /// A `(` of a statement head (`if (…)`, `while (…)`, …), whose `)` is
    /// followed by a statement, or a `{` opening a block or body rather
    /// than an object literal.
    statement_head: bool,
    /// A `{` opening the body of a function or class expression, or of a
    /// function whose kind the scan does not know: its `}` ends an operand,
    /// and a `/` after it may divide or start a regular expression.
    expression_body: bool,
    /// A `(` opened right after a keyword (`if`, `case`, `return`, …):
    /// never a parameter list, so a `:` after its `)` is not a return type.
    after_keyword: bool,
    /// A `{` opening a class body, whose members' `:` start annotations.
    class_body: bool,
    /// The segment's `function` or `class` whose body opens next.
    pending_body: Option<PendingBody>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Head {
    /// Nothing but modifiers (`export`, `declare`, …) yet.
    Start,
    /// `type`, then its name: the next `=` outside angle brackets starts
    /// the alias's right-hand side.
    TypeAlias {
        named: bool,
    },
    /// The right-hand side of a type alias.
    TypeAliasBody,
    /// `interface …`: its body is type-only.
    Interface,
    /// `enum …`: its body holds initializers.
    Enum,
    /// `class …`: its body's members carry annotations.
    Class,
    /// `let` / `const` / `var` / `using`: a `:` starts an annotation.
    Declaration,
    Other,
}

/// The class of the previous significant token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Last {
    /// Start of input, `(`, `[`, `{`, `,`, `;`, an operator or a keyword
    /// that expects an expression: a `/` here starts a regular expression
    /// and a `<` a JSX element.
    ExpressionStart,
    /// An identifier, literal, `)`, `]` or a closed object literal: a `/`
    /// here divides and a line break before a statement start ends the
    /// statement.
    Operand,
    /// A block's or a declaration's `}`: a statement follows.
    StatementEnd,
    /// The `)` of a statement head or a label's `:`: the statement that
    /// follows is its body, one level under it.
    StatementHead,
}

struct Scanner<'s> {
    source: &'s str,
    bytes: &'s [u8],
    pos: usize,
    jsx: bool,
    /// A module: `<!--` starts a comment only at the start of a line, and
    /// `-->` never does.
    module: bool,
    limit: u32,
    max: u32,
    stack: Vec<Group>,
    last: Last,
    /// The previous significant token was an identifier or keyword; its
    /// text, for the keyword checks that follow it.
    last_word: &'s [u8],
    /// The previous significant token was `=>`.
    last_arrow: bool,
    /// The previous significant token closed a bracket or a template: a
    /// `(`, `[` or template after it chains a call, index or tag.
    last_closer: bool,
    /// The previous significant token closed a parameter list: a `:`
    /// after it starts a return type.
    last_parameters: bool,
    /// The previous significant token was `.`, `?.` or `#`: a word after
    /// it is a property or private name, never a keyword.
    last_member: bool,
    /// The previous significant token was a word a type follows as a
    /// keyword (`as`, `keyof`, …) and an operand follows as an identifier:
    /// a `/` after it divides.
    last_divides: bool,
    /// Whether a `/` after the previous significant token divides or
    /// starts a regular expression depends on more than the token.
    last_ambiguous: bool,
    /// A line break (or a comment holding one) since the previous token,
    /// or no token yet.
    newline: bool,
}

/// An expression starts after the keyword.
const EXPRESSION: u8 = 1;
/// The keyword nests the syntax tree one level: a prefix or binary operator
/// spelled as a word, an `else` chaining an `if`, or a `do`. A conditional
/// type nests once, at its `?`; `extends` and a predicate's `is` /
/// `asserts` do not nest.
const LINK: u8 = 2;
/// The keyword continues an expression across a line break.
const CONTINUING: u8 = 4;
/// A modifier a declaration or class member may start with.
const MODIFIER: u8 = 8;
/// A type follows the word as a keyword, and an operand follows it as an
/// identifier: a `/` after it divides.
const DIVIDES: u8 = 16;
/// The word is a keyword a regular expression may follow, and also an
/// identifier a division may follow.
const AMBIGUOUS: u8 = 32;

/// What the scan reads from the word `word`, as the flags above.
fn keyword(word: &[u8]) -> u8 {
    match word.len() {
        2 => match word {
            b"in" => EXPRESSION | LINK | CONTINUING,
            b"as" => EXPRESSION | LINK | CONTINUING | DIVIDES,
            b"of" => EXPRESSION | CONTINUING | AMBIGUOUS,
            b"is" => EXPRESSION | CONTINUING | DIVIDES,
            b"do" => EXPRESSION | LINK,
            b"if" => EXPRESSION,
            _ => 0,
        },
        3 => match word {
            b"new" => EXPRESSION | LINK,
            b"for" => EXPRESSION,
            _ => 0,
        },
        4 => match word {
            b"void" => EXPRESSION | LINK,
            b"else" => EXPRESSION | LINK | CONTINUING,
            b"case" | b"with" => EXPRESSION,
            _ => 0,
        },
        5 => match word {
            b"throw" | b"while" => EXPRESSION,
            b"catch" => EXPRESSION | CONTINUING,
            b"yield" | b"await" => EXPRESSION | LINK | AMBIGUOUS,
            b"keyof" | b"infer" => EXPRESSION | LINK | DIVIDES,
            b"async" => MODIFIER,
            _ => 0,
        },
        6 => match word {
            b"return" => EXPRESSION,
            b"typeof" | b"delete" => EXPRESSION | LINK,
            b"unique" => EXPRESSION | LINK | DIVIDES,
            b"switch" => EXPRESSION,
            b"export" | b"public" | b"static" => MODIFIER,
            _ => 0,
        },
        7 => match word {
            b"extends" => EXPRESSION | CONTINUING,
            b"asserts" => EXPRESSION | DIVIDES,
            b"default" => MODIFIER | EXPRESSION,
            b"declare" | b"private" => MODIFIER,
            b"finally" => CONTINUING,
            _ => 0,
        },
        8 => match word {
            b"readonly" => EXPRESSION | LINK | DIVIDES,
            b"abstract" | b"override" | b"accessor" => MODIFIER,
            _ => 0,
        },
        9 => match word {
            b"satisfies" => EXPRESSION | LINK | CONTINUING | DIVIDES,
            b"protected" => MODIFIER,
            _ => 0,
        },
        10 => match word {
            b"instanceof" => EXPRESSION | LINK | CONTINUING,
            b"implements" => CONTINUING,
            _ => 0,
        },
        _ => 0,
    }
}

/// Identifier bytes: ASCII letters, digits, `_`, `$`, an escape's `\` and
/// every byte of a multi-byte character (which [`Scanner::word_at`] and
/// [`Scanner::token`] then read as a character).
const IDENT_PART: [bool; 256] = {
    let mut table = [false; 256];
    let mut byte = 0;
    while byte < 256 {
        let b = byte as u8;
        table[byte] =
            b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'\\' || b >= 0x80;
        byte += 1;
    }
    table
};

fn is_ident_start(byte: u8) -> bool {
    IDENT_PART[byte as usize] && !byte.is_ascii_digit()
}

fn is_ident_part(byte: u8) -> bool {
    IDENT_PART[byte as usize]
}

/// Whether `bytes` at `at` holds U+2028 or U+2029, the line terminators
/// beyond ASCII.
fn is_unicode_line_break(bytes: &[u8], at: usize) -> bool {
    bytes.get(at) == Some(&0xE2)
        && bytes.get(at + 1) == Some(&0x80)
        && matches!(bytes.get(at + 2), Some(0xA8 | 0xA9))
}

/// Whether `text` holds a line terminator.
fn holds_line_break(text: &[u8]) -> bool {
    memchr::memchr2(b'\n', b'\r', text).is_some()
        || memchr::memmem::find(text, "\u{2028}".as_bytes()).is_some()
        || memchr::memmem::find(text, "\u{2029}".as_bytes()).is_some()
}

/// The end of the numeric literal starting at `start`, as the lexical
/// grammar reads it: a `0x` / `0o` / `0b` literal's digits, or decimal
/// digits with one fraction and one exponent, each with `_` separators,
/// then a BigInt's `n`. A `.` after that is a member access (`1..a`,
/// `1.5.a`, `0x1f.a`, `017.a`), which the scan counts.
fn numeric_literal_end(bytes: &[u8], start: usize) -> usize {
    let at = |index: usize| bytes.get(index).copied().unwrap_or(0);
    let digits = |mut index: usize, digit: fn(&u8) -> bool| {
        while digit(&at(index)) || at(index) == b'_' {
            index += 1;
        }
        index
    };
    let mut end = start;
    if at(end) == b'0' && matches!(at(end + 1), b'x' | b'X' | b'o' | b'O' | b'b' | b'B') {
        end = digits(end + 2, u8::is_ascii_hexdigit);
    } else if at(end) == b'0' && at(end + 1).is_ascii_digit() {
        // A legacy octal literal (`017`) takes no fraction; one read as a
        // member access after it only over-counts.
        return digits(end, u8::is_ascii_digit);
    } else {
        end = digits(end, u8::is_ascii_digit);
        if at(end) == b'.' {
            end = digits(end + 1, u8::is_ascii_digit);
        }
        if matches!(at(end), b'e' | b'E') {
            let sign = usize::from(matches!(at(end + 1), b'+' | b'-'));
            if at(end + 1 + sign).is_ascii_digit() {
                end = digits(end + 1 + sign, u8::is_ascii_digit);
            }
        }
    }
    if at(end) == b'n' {
        end += 1;
    }
    end
}

/// The length of the operator starting `rest`: the longest the scan tells
/// apart (`>>>=` down to one byte).
fn operator_len(rest: &[u8]) -> usize {
    let at = |index: usize| rest.get(index).copied().unwrap_or(0);
    match at(0) {
        b'>' => match (at(1), at(2), at(3)) {
            (b'>', b'>', b'=') => 4,
            (b'>', b'>' | b'=', _) => 3,
            (b'>' | b'=', _, _) => 2,
            _ => 1,
        },
        b'<' => match (at(1), at(2)) {
            (b'<', b'=') => 3,
            (b'<' | b'=', _) => 2,
            _ => 1,
        },
        b'=' => match (at(1), at(2)) {
            (b'=', b'=') => 3,
            (b'=' | b'>', _) => 2,
            _ => 1,
        },
        b'!' => match (at(1), at(2)) {
            (b'=', b'=') => 3,
            (b'=', _) => 2,
            _ => 1,
        },
        b'*' | b'&' | b'|' => match (at(1), at(2)) {
            (second, b'=') if second == at(0) => 3,
            (second, _) if second == at(0) => 2,
            (b'=', _) => 2,
            _ => 1,
        },
        b'?' => match (at(1), at(2)) {
            (b'?', b'=') => 3,
            (b'?', _) => 2,
            (b'.', digit) if !digit.is_ascii_digit() => 2,
            _ => 1,
        },
        b'.' if at(1) == b'.' && at(2) == b'.' => 3,
        b'+' | b'-' if at(1) == at(0) || at(1) == b'=' => 2,
        b'/' | b'%' | b'^' if at(1) == b'=' => 2,
        _ => 1,
    }
}

impl Group {
    fn new(kind: GroupKind, base: u32, types: bool) -> Self {
        Self {
            kind,
            base,
            chain: 0,
            head_chain: 0,
            ended_chain: 0,
            types,
            segment_types: types,
            return_annotation: false,
            head: Head::Start,
            head_angles: 0,
            angles: 0,
            questions: 0,
            member_base: 0,
            statement_head: false,
            expression_body: false,
            after_keyword: false,
            class_body: false,
            pending_body: None,
        }
    }

    /// Whether the group holds statements: the file, or a block or body.
    fn statements(&self) -> bool {
        self.kind == GroupKind::Root || (self.kind == GroupKind::Brace && self.statement_head)
    }
}

impl<'s> Scanner<'s> {
    fn new(source: &'s str, source_type: SourceType, limit: u32) -> Self {
        let declaration = source_type.is_typescript_definition();
        let mut root = Group::new(GroupKind::Root, 0, declaration);
        root.statement_head = true;
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            jsx: source_type.is_jsx(),
            module: source_type.is_module(),
            limit,
            max: 0,
            stack: vec![root],
            last: Last::ExpressionStart,
            last_word: b"",
            last_arrow: false,
            last_closer: false,
            last_parameters: false,
            last_member: false,
            last_divides: false,
            last_ambiguous: false,
            newline: true,
        }
    }

    fn top(&mut self) -> &mut Group {
        self.stack
            .last_mut()
            .expect("the root group is never popped")
    }

    fn peek(&self, ahead: usize) -> u8 {
        self.bytes.get(self.pos + ahead).copied().unwrap_or(0)
    }

    fn rest_starts_with(&self, text: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(text)
    }

    fn check(&mut self, depth: u32, offset: usize) -> Result<(), NestingExceeded> {
        self.max = self.max.max(depth);
        if depth > self.limit {
            Err(NestingExceeded {
                offset: offset as u32,
            })
        } else {
            Ok(())
        }
    }

    /// One more link in the innermost group's segment.
    ///
    /// In a type every such link (a conditional's `?`, a function type's
    /// `=>`) is the parent of the member accesses and type operators before
    /// it in the segment, never their child, so it builds on the segment's
    /// last such link rather than on them.
    fn link(&mut self, offset: usize) -> Result<(), NestingExceeded> {
        let top = self.top();
        if top.segment_types {
            top.chain = top.member_base;
        }
        self.link_member(offset)?;
        let top = self.top();
        top.member_base = top.chain;
        Ok(())
    }

    /// One more link that a type union's next member does not sit under:
    /// a member access, or a call or index chained on a result.
    fn link_member(&mut self, offset: usize) -> Result<(), NestingExceeded> {
        let top = self.top();
        top.chain += 1;
        let depth = top.base + top.chain;
        self.check(depth, offset)
    }

    fn push(&mut self, kind: GroupKind, types: bool) -> Result<(), NestingExceeded> {
        let offset = self.pos;
        let top = *self.top();
        let base = top.base + top.chain + 1;
        self.stack.push(Group::new(kind, base, types));
        self.check(base, offset)
    }

    /// Bound everything from `offset` on by its length: however the source
    /// after it reads, each level of its nesting takes a byte of it.
    fn bound_rest(&mut self, offset: usize) -> Result<(), NestingExceeded> {
        let top = *self.top();
        let rest = u32::try_from(self.bytes.len() - offset).unwrap_or(u32::MAX);
        self.check(
            (top.base + top.chain)
                .saturating_add(1)
                .saturating_add(rest),
            offset,
        )
    }

    /// Start a new comma-separated segment in the innermost group.
    fn reset_segment(&mut self) {
        let top = self.top();
        top.chain = 0;
        top.segment_types = top.types;
        top.return_annotation = false;
        top.head = Head::Start;
        top.head_angles = 0;
        top.angles = 0;
        top.questions = 0;
        top.member_base = 0;
        top.pending_body = None;
    }

    /// Start a new statement in the innermost group.
    fn reset(&mut self) {
        self.reset_segment();
        let top = self.top();
        top.ended_chain = top.head_chain;
        top.head_chain = 0;
    }

    /// Pop the innermost group if it is one of `kinds`; a closer that
    /// matches nothing is ignored.
    fn pop(&mut self, kinds: &[GroupKind]) -> Option<Group> {
        if self.stack.len() > 1 && kinds.contains(&self.top().kind) {
            self.stack.pop()
        } else {
            None
        }
    }

    fn run(mut self) -> Result<Nesting, NestingExceeded> {
        if self.bytes.starts_with(b"#!") {
            self.skip_line();
        }
        while self.pos < self.bytes.len() {
            match self.top().kind {
                GroupKind::Template => self.template_text()?,
                GroupKind::JsxChildren => self.jsx_children()?,
                GroupKind::JsxTag | GroupKind::JsxClosingTag => self.jsx_tag()?,
                _ => self.token()?,
            }
        }
        Ok(Nesting { depth: self.max })
    }

    /// Skip to the line terminator that ends a line comment.
    fn skip_line(&mut self) {
        let mut at = self.pos;
        while let Some(found) = memchr::memchr3(b'\n', b'\r', 0xE2, &self.bytes[at..]) {
            at += found;
            if self.bytes[at] != 0xE2 || is_unicode_line_break(self.bytes, at) {
                self.pos = at;
                return;
            }
            at += 1;
        }
        self.pos = self.bytes.len();
    }

    /// The character at the scan's position, when it starts one.
    fn char_here(&self) -> Option<char> {
        self.source.get(self.pos..)?.chars().next()
    }

    /// Whether a line break before a token starting with `byte` (and, for
    /// a word, spelled `word`) ends the statement or type before it.
    fn ends_segment(&self, byte: u8, word: &[u8]) -> bool {
        if !self.newline {
            return false;
        }
        let top = self.stack.last().expect("root");
        if !matches!(top.kind, GroupKind::Root | GroupKind::Brace) {
            return false;
        }
        match self.last {
            // `} else`, `} catch` and `} finally` continue the statement.
            Last::StatementEnd => !matches!(word, b"else" | b"catch" | b"finally"),
            // A statement head's body follows it.
            Last::ExpressionStart | Last::StatementHead => false,
            Last::Operand if top.segment_types => {
                // A type continues across a line break only through these.
                !matches!(
                    byte,
                    b'|' | b'&' | b'?' | b':' | b'.' | b',' | b')' | b']' | b'}' | b'>' | b'='
                ) && !matches!(word, b"extends" | b"is")
            }
            Last::Operand => {
                if is_ident_start(byte) {
                    keyword(word) & CONTINUING == 0
                } else {
                    matches!(byte, b'{' | b'!' | b'~' | b'@' | b'#' | b'"' | b'\'')
                        || byte.is_ascii_digit()
                        || (byte == b'+' && self.peek(1) == b'+')
                        || (byte == b'-' && self.peek(1) == b'-')
                }
            }
        }
    }

    fn token(&mut self) -> Result<(), NestingExceeded> {
        let byte = self.bytes[self.pos];
        match byte {
            b'\n' | b'\r' => {
                self.newline = true;
                self.pos += 1;
                return Ok(());
            }
            b' ' | b'\t' | 0x0b | 0x0c => {
                self.pos += 1;
                while matches!(self.peek(0), b' ' | b'\t') {
                    self.pos += 1;
                }
                return Ok(());
            }
            b'/' if self.peek(1) == b'/' => {
                self.skip_line();
                return Ok(());
            }
            b'/' if self.peek(1) == b'*' => {
                let body = &self.bytes[self.pos + 2..];
                let end = memchr::memmem::find(body, b"*/").unwrap_or(body.len());
                if holds_line_break(&body[..end]) {
                    self.newline = true;
                }
                self.pos = (self.pos + 2 + end + 2).min(self.bytes.len());
                return Ok(());
            }
            // HTML-like comments (Annex B.1.1), where oxc reads them:
            // `<!--` outside a module or at a module's line start, `-->` at
            // a line start outside a module.
            b'<' if self.rest_starts_with(b"<!--") && (!self.module || self.newline) => {
                self.skip_line();
                return Ok(());
            }
            b'-' if self.rest_starts_with(b"-->") && !self.module && self.newline => {
                self.skip_line();
                return Ok(());
            }
            0x80.. => match self.char_here() {
                Some(space) if is_irregular_whitespace(space) => {
                    self.pos += space.len_utf8();
                    return Ok(());
                }
                Some(line_break) if is_irregular_line_terminator(line_break) => {
                    self.newline = true;
                    self.pos += line_break.len_utf8();
                    return Ok(());
                }
                Some(identifier) if is_identifier_start_unicode(identifier) => {}
                // A character oxc rejects, or a byte inside one.
                other => {
                    self.pos += other.map_or(1, char::len_utf8);
                    self.operand();
                    return Ok(());
                }
            },
            _ => {}
        }
        let word = if is_ident_start(byte) {
            self.word_at(self.pos)
        } else {
            b""
        };
        if self.ends_segment(byte, word) {
            self.reset();
        }
        self.newline = false;
        let start = self.pos;
        if self.last == Last::StatementHead && byte != b'{' {
            // A braceless statement head's body, one level under it.
            self.link(start)?;
            let top = self.top();
            top.head_chain = top.chain;
        }
        if is_ident_start(byte) {
            self.pos += word.len().max(1);
            return self.word(word, start);
        }
        if self.top().head == Head::Start {
            self.top().head = Head::Other;
        }
        if byte.is_ascii_digit() || (byte == b'.' && self.peek(1).is_ascii_digit()) {
            self.pos = numeric_literal_end(self.bytes, self.pos);
            self.operand();
            return Ok(());
        }
        match byte {
            b'"' | b'\'' => {
                self.string(byte);
                self.operand();
            }
            b'`' => {
                self.pos += 1;
                if self.last_closer {
                    // A template tagging a call's or template's result.
                    self.link_member(start)?;
                }
                let types = self.top().segment_types;
                self.push(GroupKind::Template, types)?;
            }
            b'(' | b'[' => {
                self.pos += 1;
                if self.last_closer {
                    // A call or index on a call's or index's result.
                    self.link_member(start)?;
                }
                let after_keyword =
                    self.last == Last::ExpressionStart && !self.last_word.is_empty();
                let statement_head = byte == b'('
                    && matches!(
                        self.last_word,
                        b"if" | b"while" | b"for" | b"with" | b"switch" | b"catch"
                    );
                let types = self.top().segment_types;
                let kind = if byte == b'(' {
                    GroupKind::Paren
                } else {
                    GroupKind::Bracket
                };
                self.push(kind, types)?;
                let top = self.top();
                top.statement_head = statement_head;
                top.after_keyword = after_keyword;
                self.expression_start();
            }
            b'{' => self.open_brace()?,
            b')' | b']' | b'}' => {
                self.pos += 1;
                let kinds: &[GroupKind] = match byte {
                    b')' => &[GroupKind::Paren],
                    b']' => &[GroupKind::Bracket],
                    _ => &[
                        GroupKind::Brace,
                        GroupKind::TemplateExpr,
                        GroupKind::JsxExpr,
                    ],
                };
                // A type argument list left open (a `<` read as one where it
                // compared) closes with the bracket around it.
                while self.top().kind == GroupKind::Angle {
                    self.stack.pop();
                }
                let closed = self.pop(kinds);
                self.operand();
                self.last_closer = true;
                self.last_parameters = matches!(
                    closed,
                    Some(group) if group.kind == GroupKind::Paren && !group.after_keyword
                );
                match closed {
                    Some(group) if group.kind == GroupKind::Paren && group.statement_head => {
                        self.last = Last::StatementHead;
                    }
                    Some(group) if group.kind == GroupKind::Brace && group.expression_body => {
                        self.last_ambiguous = true;
                    }
                    Some(group) if group.kind == GroupKind::Brace && group.statement_head => {
                        self.last = Last::StatementEnd;
                    }
                    _ => {}
                }
            }
            b',' if self.top().angles > 0 => {
                // Possibly between the type arguments of one list.
                self.pos += 1;
                self.expression_start();
            }
            b',' | b';' => {
                self.pos += 1;
                let head = self.top().head;
                if byte == b',' {
                    self.reset_segment();
                } else {
                    self.reset();
                }
                if byte == b',' && matches!(head, Head::Interface | Head::Class | Head::Declaration)
                {
                    // The next heritage type or declarator of the same
                    // declaration.
                    self.top().head = head;
                }
                self.expression_start();
            }
            b':' => {
                self.pos += 1;
                let label = self.colon();
                self.expression_start();
                if label {
                    self.last = Last::StatementHead;
                }
            }
            b'/' => {
                if self.last_ambiguous {
                    self.bound_rest(start)?;
                }
                if self.last == Last::Operand || self.last_divides {
                    self.punctuator(start)?;
                } else {
                    self.regex(start)?;
                    self.operand();
                }
            }
            b'<' if self.jsx
                && self.last != Last::Operand
                && !self.last_divides
                && !self.top().segment_types
                && self.jsx_tag_start() =>
            {
                self.pos += 1;
                self.push(GroupKind::JsxTag, false)?;
            }
            b'\\' => {
                // An escape outside a string or regular expression (an
                // identifier's unicode escape): skip it whole.
                self.pos = (self.pos + 2).min(self.bytes.len());
                self.operand();
            }
            _ if byte.is_ascii_punctuation() => self.punctuator(start)?,
            _ => {
                self.pos += 1;
            }
        }
        Ok(())
    }

    /// A `{`: a block or body, or an object literal or type.
    fn open_brace(&mut self) -> Result<(), NestingExceeded> {
        self.pos += 1;
        let top = *self.top();
        // A block or body follows an operand (a function's or a class's
        // head), a statement head, an arrow, `else`, `do`, `try` or
        // `finally`, or starts a statement; anything else opens an object
        // literal.
        let block = match self.last {
            Last::Operand | Last::StatementEnd | Last::StatementHead => true,
            Last::ExpressionStart => {
                self.last_arrow
                    || matches!(self.last_word, b"else" | b"do" | b"try" | b"finally")
                    || (self.last_word.is_empty() && top.chain == 0 && top.statements())
            }
        };
        let types = match top.head {
            Head::Interface => true,
            Head::Enum => false,
            // A function's body after its return type.
            _ if block && top.return_annotation => false,
            _ => top.segment_types,
        };
        // After an operand the block is a body: a function or class
        // expression's ends an operand, as does one whose kind the scan
        // does not know; a declaration's, or a class member's, ends a
        // statement.
        let expression_body = block
            && self.last == Last::Operand
            && !top.class_body
            && !matches!(top.pending_body, Some(PendingBody::Declaration { .. }));
        let class_body = top.head == Head::Class
            || matches!(
                top.pending_body,
                Some(
                    PendingBody::Declaration { class: true }
                        | PendingBody::Expression { class: true }
                )
            );
        if block {
            let outer = self.top();
            outer.pending_body = None;
            if outer.return_annotation {
                // The expression goes on after the function's body.
                outer.return_annotation = false;
                outer.segment_types = false;
            }
        }
        self.push(GroupKind::Brace, types)?;
        let group = self.top();
        group.statement_head = block;
        group.expression_body = expression_body;
        group.class_body = class_body;
        self.expression_start();
        Ok(())
    }

    /// A `:` closes a pending conditional, or starts an annotation where
    /// one can stand: in a parameter list, after a parameter list (a
    /// return type), after a variable's name, or after a class member's.
    /// Otherwise, after an identifier starting a statement, it ends a
    /// label, and the result is `true`.
    fn colon(&mut self) -> bool {
        let last_parameters = self.last_parameters;
        let identifier = self.last == Last::Operand
            && !self.last_word.is_empty()
            && keyword(self.last_word) == 0;
        let top = self.top();
        if top.questions > 0 {
            top.questions -= 1;
            return false;
        }
        let annotation = top.kind == GroupKind::Paren
            || last_parameters
            || (matches!(top.kind, GroupKind::Root | GroupKind::Brace)
                && (top.head == Head::Declaration || top.class_body));
        if annotation {
            top.segment_types = true;
            top.return_annotation = last_parameters;
            top.member_base = top.chain;
            return false;
        }
        identifier && top.statements() && !top.segment_types
    }

    /// The identifier or keyword starting at `start`: ASCII identifier
    /// characters, an escape's backslash and the byte after it, and the
    /// characters beyond ASCII an identifier continues with.
    fn word_at(&self, start: usize) -> &'s [u8] {
        let mut end = start;
        while end < self.bytes.len() && is_ident_part(self.bytes[end]) {
            match self.bytes[end] {
                b'\\' => end += 2,
                0x80.. => match self.source.get(end..).and_then(|rest| rest.chars().next()) {
                    Some(part) if is_identifier_part_unicode(part) => end += part.len_utf8(),
                    _ => break,
                },
                _ => end += 1,
            }
        }
        &self.bytes[start..end.min(self.bytes.len())]
    }

    /// Whether the group's current statement starts here: after a
    /// statement's end or head, or after nothing but modifiers.
    fn statement_start(&self) -> bool {
        let top = self.stack.last().expect("root");
        top.statements()
            && (matches!(self.last, Last::StatementEnd | Last::StatementHead)
                || (top.chain == 0 && top.head == Head::Start))
    }

    fn word(&mut self, word: &'s [u8], start: usize) -> Result<(), NestingExceeded> {
        if self.last_member {
            // A property or private name, never a keyword.
            self.operand();
            return Ok(());
        }
        let flags = keyword(word);
        if matches!(word, b"function" | b"class") {
            let class = word == b"class";
            let body = if self.statement_start() {
                PendingBody::Declaration { class }
            } else {
                PendingBody::Expression { class }
            };
            self.top().pending_body = Some(body);
        }
        // Declarations that turn type-only syntax on or off.
        let group = self.top();
        group.head = match (group.head, word) {
            (Head::Start, _) if flags & MODIFIER != 0 => Head::Start,
            (Head::Start, b"type") => Head::TypeAlias { named: false },
            (Head::Start, b"interface") => Head::Interface,
            (Head::Start | Head::Declaration, b"enum") => Head::Enum,
            (Head::Start, b"class") => Head::Class,
            (Head::Start, b"let" | b"var" | b"const" | b"using") => Head::Declaration,
            (Head::TypeAlias { named: false }, _) => Head::TypeAlias { named: true },
            (Head::Start, _) => Head::Other,
            (head, _) => head,
        };
        if word == b"else" {
            // The `if` it continues nested as deeply as its statement.
            group.chain = group.chain.max(group.ended_chain);
        }
        if flags & LINK != 0 && self.top().segment_types {
            // A type operator (`keyof`, `typeof`, …) binds within one
            // union member.
            self.link_member(start)?;
        } else if flags & LINK != 0 {
            self.link(start)?;
        }
        self.expression_start();
        self.last_word = word;
        if flags & EXPRESSION == 0 {
            self.last = Last::Operand;
        }
        self.last_divides = flags & DIVIDES != 0;
        self.last_ambiguous = flags & AMBIGUOUS != 0;
        Ok(())
    }

    fn clear_last(&mut self) {
        self.last_word = b"";
        self.last_arrow = false;
        self.last_closer = false;
        self.last_parameters = false;
        self.last_member = false;
        self.last_divides = false;
        self.last_ambiguous = false;
    }

    fn operand(&mut self) {
        self.last = Last::Operand;
        self.clear_last();
    }

    fn expression_start(&mut self) {
        self.last = Last::ExpressionStart;
        self.clear_last();
    }

    fn string(&mut self, quote: u8) {
        self.pos += 1;
        while self.pos < self.bytes.len() {
            match memchr::memchr3(quote, b'\\', b'\n', &self.bytes[self.pos..]) {
                Some(at) => {
                    // A CR ends an unterminated string as a LF does.
                    let line = &self.bytes[self.pos..self.pos + at];
                    if let Some(cr) = memchr::memchr(b'\r', line) {
                        self.pos += cr;
                        return;
                    }
                    self.pos += at;
                }
                None => {
                    self.pos = memchr::memchr(b'\r', &self.bytes[self.pos..])
                        .map_or(self.bytes.len(), |cr| self.pos + cr);
                    return;
                }
            }
            match self.bytes[self.pos] {
                // A line continuation takes its CRLF whole.
                b'\\' if self.peek(1) == b'\r' && self.peek(2) == b'\n' => self.pos += 3,
                b'\\' => self.pos += 2,
                b'\n' => return,
                b if b == quote => {
                    self.pos += 1;
                    return;
                }
                _ => self.pos += 1,
            }
        }
        self.pos = self.pos.min(self.bytes.len());
    }

    /// A regular expression from its opening `/`: its brackets nest on
    /// top of the current depth, and a `/` inside a character class does
    /// not end it.
    fn regex(&mut self, start: usize) -> Result<(), NestingExceeded> {
        let top = *self.top();
        let base = top.base + top.chain + 1;
        let mut local = 0u32;
        let mut class = false;
        self.pos += 1;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'\\' => self.pos += 1,
                b'\n' | b'\r' => break,
                0xE2 if is_unicode_line_break(self.bytes, self.pos) => break,
                b'/' if !class => {
                    self.pos += 1;
                    while self.pos < self.bytes.len() && is_ident_part(self.bytes[self.pos]) {
                        self.pos += 1;
                    }
                    return Ok(());
                }
                b'[' if !class => {
                    class = true;
                    local += 1;
                    self.check(base + local, start)?;
                }
                b']' if class => {
                    class = false;
                    local = local.saturating_sub(1);
                }
                b'(' | b'{' if !class => {
                    local += 1;
                    self.check(base + local, start)?;
                }
                b')' | b'}' if !class => local = local.saturating_sub(1),
                _ => {}
            }
            self.pos += 1;
        }
        self.pos = self.pos.min(self.bytes.len());
        Ok(())
    }

    fn punctuator(&mut self, start: usize) -> Result<(), NestingExceeded> {
        if self.top().kind == GroupKind::Angle && self.bytes[self.pos] == b'>' {
            // Each `>` closes one type argument list.
            self.pos += 1;
            self.stack.pop();
            self.operand();
            self.last_closer = true;
            return Ok(());
        }
        let rest = &self.bytes[self.pos..];
        let len = operator_len(rest);
        let op = &rest[..len];
        self.pos += len;
        let top = *self.top();
        if top.segment_types && op == b"<" {
            // A type argument or parameter list opens.
            self.push(GroupKind::Angle, true)?;
            self.expression_start();
            return Ok(());
        }
        let optional_marker = op == b"?" && matches!(self.peek(0), b':' | b')' | b',');
        let flat_in_types = matches!(op, b"|" | b"&" | b">" | b">>" | b">>>");
        let counted =
            !(top.segment_types && flat_in_types) && !optional_marker && op != b"@" && op != b"#";
        if counted && matches!(op, b"." | b"?.") {
            self.link_member(start)?;
        } else if counted {
            self.link(start)?;
        } else if top.segment_types && matches!(op, b"|" | b"&") {
            // A union's or intersection's next member: a sibling of the
            // one before it, one level under the union.
            let group = self.top();
            group.chain = group.member_base + 1;
        }
        if op == b"?" && !optional_marker {
            self.top().questions += 1;
        }
        if !top.segment_types {
            // An expression's `<` may open type arguments.
            let group = self.top();
            match op {
                b"<" => group.angles += 1,
                b">" | b">>" | b">>>" => group.angles = group.angles.saturating_sub(len as u32),
                _ => {}
            }
        }
        // A type alias's head: `=` outside its type parameters starts the
        // right-hand side; any other `=` starts an initializer.
        match (top.head, op) {
            (Head::TypeAlias { named: true }, b"<") => self.top().head_angles += 1,
            (Head::TypeAlias { named: true }, b">" | b">>" | b">>>") => {
                let group = self.top();
                group.head_angles = group.head_angles.saturating_sub(len as u32);
            }
            (Head::TypeAlias { named: true }, b"=") if top.head_angles == 0 => {
                let group = self.top();
                group.head = Head::TypeAliasBody;
                group.segment_types = true;
                group.member_base = group.chain;
            }
            (Head::TypeAliasBody, _) => {}
            (_, b"=") => {
                let group = self.top();
                group.segment_types = false;
                group.return_annotation = false;
            }
            (_, b"=>") if top.return_annotation => {
                // An arrow's body follows its return type.
                let group = self.top();
                group.segment_types = false;
                group.return_annotation = false;
            }
            _ => {}
        }
        if matches!(op, b"++" | b"--" | b"!") && self.last == Last::Operand {
            // A postfix update or non-null assertion ends an operand.
            let closer = self.last_closer;
            self.operand();
            self.last_closer = closer;
        } else {
            self.expression_start();
            self.last_arrow = op == b"=>";
            self.last_member = matches!(op, b"." | b"?." | b"#");
        }
        Ok(())
    }

    /// The position after the whitespace and comments at `at`.
    fn skip_trivia(&self, mut at: usize) -> usize {
        loop {
            while at < self.bytes.len() && self.bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            let rest = &self.bytes[at.min(self.bytes.len())..];
            if rest.starts_with(b"/*") {
                at += memchr::memmem::find(&rest[2..], b"*/").map_or(rest.len(), |end| end + 4);
            } else if rest.starts_with(b"//") {
                at += memchr::memchr(b'\n', rest).unwrap_or(rest.len());
            } else {
                return at;
            }
        }
    }

    /// Whether the `<` at `pos` opens a JSX element rather than a TSX
    /// arrow function's type parameters (`<T,>` / `<T extends U>`).
    fn jsx_tag_start(&self) -> bool {
        let at = self.skip_trivia(self.pos + 1);
        let next = self.bytes.get(at).copied().unwrap_or(0);
        if next == b'>' {
            return true;
        }
        if !is_ident_start(next) {
            return false;
        }
        let name = self.word_at(at);
        let after = self.skip_trivia(at + name.len());
        let rest = &self.bytes[after.min(self.bytes.len())..];
        let extends = rest.starts_with(b"extends")
            && rest
                .get(b"extends".len())
                .is_some_and(|byte| byte.is_ascii_whitespace());
        !(rest.starts_with(b",") || extends)
    }

    /// Inside a JSX tag: attributes up to `>` (children follow) or `/>`.
    fn jsx_tag(&mut self) -> Result<(), NestingExceeded> {
        let byte = self.bytes[self.pos];
        match byte {
            b'/' if self.peek(1) == b'>' => {
                self.pos += 2;
                self.pop(&[GroupKind::JsxTag, GroupKind::JsxClosingTag]);
                self.jsx_element_done();
            }
            b'/' if matches!(self.peek(1), b'/' | b'*') => self.pos = self.skip_trivia(self.pos),
            b'>' => {
                self.pos += 1;
                let closing = self.top().kind == GroupKind::JsxClosingTag;
                self.pop(&[GroupKind::JsxTag, GroupKind::JsxClosingTag]);
                if closing {
                    // `</name>`: the element it closes ends too.
                    self.pop(&[GroupKind::JsxChildren]);
                    self.jsx_element_done();
                } else {
                    self.push(GroupKind::JsxChildren, false)?;
                }
            }
            b'{' => {
                self.pos += 1;
                self.push(GroupKind::JsxExpr, false)?;
                self.expression_start();
            }
            b'.' => {
                // A member of the element's name.
                self.link_member(self.pos)?;
                self.pos += 1;
            }
            b'<' => {
                let value = self.bytes[..self.pos]
                    .iter()
                    .rev()
                    .find(|byte| !byte.is_ascii_whitespace())
                    == Some(&b'=');
                self.pos += 1;
                if value {
                    // An element as an attribute's value.
                    self.push(GroupKind::JsxTag, false)?;
                } else {
                    // The element's type arguments.
                    self.push(GroupKind::Angle, true)?;
                    self.expression_start();
                }
            }
            b'"' | b'\'' => {
                let quote = byte;
                self.pos += 1;
                while self.pos < self.bytes.len() && self.bytes[self.pos] != quote {
                    self.pos += 1;
                }
                self.pos = (self.pos + 1).min(self.bytes.len());
            }
            _ => self.pos += 1,
        }
        Ok(())
    }

    /// After a JSX element closes: back among its parent's children or in
    /// its parent's tag, or an operand in the script.
    fn jsx_element_done(&mut self) {
        if !matches!(
            self.top().kind,
            GroupKind::JsxChildren | GroupKind::JsxTag | GroupKind::JsxClosingTag
        ) {
            self.operand();
        }
    }

    /// Among a JSX element's children: text up to a tag or a `{`.
    fn jsx_children(&mut self) -> Result<(), NestingExceeded> {
        match self.bytes[self.pos] {
            b'<' if self.bytes[self.skip_trivia(self.pos + 1)..].starts_with(b"/") => {
                self.pos += 1;
                self.pos = self.skip_trivia(self.pos) + 1;
                self.push(GroupKind::JsxClosingTag, false)?;
            }
            b'<' => {
                self.pos += 1;
                self.push(GroupKind::JsxTag, false)?;
            }
            b'{' => {
                self.pos += 1;
                self.push(GroupKind::JsxExpr, false)?;
                self.expression_start();
            }
            _ => self.pos += 1,
        }
        Ok(())
    }

    /// A template literal's text, up to its closing backtick or a `${`.
    fn template_text(&mut self) -> Result<(), NestingExceeded> {
        match self.bytes[self.pos] {
            b'\\' => self.pos = (self.pos + 2).min(self.bytes.len()),
            b'`' => {
                self.pos += 1;
                self.pop(&[GroupKind::Template]);
                self.operand();
                self.last_closer = true;
            }
            b'$' if self.peek(1) == b'{' => {
                self.pos += 2;
                let types = self.top().types;
                self.push(GroupKind::TemplateExpr, types)?;
                self.expression_start();
            }
            _ => self.pos += 1,
        }
        Ok(())
    }
}
