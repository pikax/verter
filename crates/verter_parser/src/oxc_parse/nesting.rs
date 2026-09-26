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
//! a comma-separated element starts over, so siblings never add up.
//!
//! A type union or intersection (`'a' | 'b' | …`) is one node however many
//! members it has, where the same tokens in an expression nest once per
//! operator; `|` and `&` are not counted where the source can only be a
//! type: a type alias's right-hand side, an interface body, an annotation
//! after `:` (a parameter's, a variable's, a class member's or a return
//! type) up to an initializer, and a declaration file outside its enum
//! bodies and initializers.
//!
//! Strings, comments, regular expressions, template text and JSX text are
//! skipped. Where the lexical grammar needs the parser to tell a regular
//! expression from a division, the scan decides from the previous token;
//! a regular expression's own brackets are still counted, and a closing
//! bracket that matches no open one is ignored, so a misread in either
//! direction over-counts rather than hides a level.

use oxc_span::SourceType;

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

#[derive(Debug, Clone, Copy)]
struct Group {
    kind: GroupKind,
    /// The depth this group's own tokens start from.
    base: u32,
    /// Links counted in the current segment (since the last `,`, `;` or
    /// statement-ending line break).
    chain: u32,
    /// Whether the group's segments start as type-only syntax.
    types: bool,
    /// Whether the rest of the current segment is type-only syntax.
    segment_types: bool,
    /// How the current segment began, for the declarations that switch
    /// type-only syntax on or off.
    head: Head,
    /// Angle-bracket depth inside a type alias's head (`type A<T = B> =`).
    head_angles: u32,
    /// Conditional `?`s in the segment still waiting for their `:`.
    questions: u32,
    /// The segment's links up to its last one that is not a member access
    /// or a chained call or index: where a type union's members start.
    member_base: u32,
    /// A `(` of a statement head (`if (…)`, `while (…)`, …), whose `)` is
    /// followed by a statement, or a `{` opening a block or body rather
    /// than an object literal.
    statement_head: bool,
    /// A `(` opened right after a keyword (`if`, `case`, `return`, …):
    /// never a parameter list, so a `:` after its `)` is not a return type.
    after_keyword: bool,
    /// A `{` opening a class body, whose members' `:` start annotations.
    class_body: bool,
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
    /// A block's `}` or the `)` of a statement head: a statement follows.
    StatementEnd,
}

struct Scanner<'s> {
    bytes: &'s [u8],
    pos: usize,
    jsx: bool,
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
    /// A line break (or a comment holding one) since the previous token.
    newline: bool,
}

/// An expression starts after the keyword.
const EXPRESSION: u8 = 1;
/// The keyword nests the syntax tree one level: a prefix or binary operator
/// spelled as a word, or an `else` chaining an `if`. A conditional type
/// nests once, at its `?`; `extends` and a predicate's `is` / `asserts` do
/// not nest.
const LINK: u8 = 2;
/// The keyword continues an expression across a line break.
const CONTINUING: u8 = 4;
/// A modifier a declaration or class member may start with.
const MODIFIER: u8 = 8;

/// What the scan reads from the word `word`, as the flags above.
fn keyword(word: &[u8]) -> u8 {
    match word.len() {
        2 => match word {
            b"in" => EXPRESSION | LINK | CONTINUING,
            b"as" => EXPRESSION | LINK | CONTINUING,
            b"of" | b"is" => EXPRESSION | CONTINUING,
            b"do" | b"if" => EXPRESSION,
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
            b"yield" | b"await" | b"keyof" | b"infer" => EXPRESSION | LINK,
            b"async" => MODIFIER,
            _ => 0,
        },
        6 => match word {
            b"return" => EXPRESSION,
            b"typeof" | b"delete" | b"unique" => EXPRESSION | LINK,
            b"switch" => EXPRESSION,
            b"export" | b"public" | b"static" => MODIFIER,
            _ => 0,
        },
        7 => match word {
            b"extends" => EXPRESSION | CONTINUING,
            b"asserts" => EXPRESSION,
            b"declare" | b"default" | b"private" => MODIFIER,
            b"finally" => CONTINUING,
            _ => 0,
        },
        8 => match word {
            b"readonly" => EXPRESSION | LINK,
            b"abstract" | b"override" | b"accessor" => MODIFIER,
            _ => 0,
        },
        9 => match word {
            b"satisfies" => EXPRESSION | LINK | CONTINUING,
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
/// every byte of a multi-byte character.
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

impl<'s> Scanner<'s> {
    fn new(source: &'s str, source_type: SourceType, limit: u32) -> Self {
        let declaration = source_type.is_typescript_definition();
        Self {
            bytes: source.as_bytes(),
            pos: 0,
            jsx: source_type.is_jsx(),
            limit,
            max: 0,
            stack: vec![Group {
                kind: GroupKind::Root,
                base: 0,
                chain: 0,
                types: declaration,
                segment_types: declaration,
                head: Head::Start,
                head_angles: 0,
                questions: 0,
                member_base: 0,
                statement_head: true,
                after_keyword: false,
                class_body: false,
            }],
            last: Last::ExpressionStart,
            last_word: b"",
            last_arrow: false,
            last_closer: false,
            last_parameters: false,
            newline: false,
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
        self.stack.push(Group {
            kind,
            base,
            chain: 0,
            types,
            segment_types: types,
            head: Head::Start,
            head_angles: 0,
            questions: 0,
            member_base: 0,
            statement_head: false,
            after_keyword: false,
            class_body: false,
        });
        self.check(base, offset)
    }

    /// Start a new segment in the innermost group.
    fn reset(&mut self) {
        let top = self.top();
        top.chain = 0;
        top.segment_types = top.types;
        top.head = Head::Start;
        top.head_angles = 0;
        top.questions = 0;
        top.member_base = 0;
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

    fn skip_line(&mut self) {
        self.pos = memchr::memchr(b'\n', &self.bytes[self.pos..])
            .map_or(self.bytes.len(), |at| self.pos + at);
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
            Last::ExpressionStart => false,
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
            b'\n' => {
                self.newline = true;
                self.pos += 1;
                return Ok(());
            }
            b' ' | b'\t' | b'\r' | 0x0b | 0x0c => {
                self.pos += 1;
                while matches!(self.peek(0), b' ' | b'\t' | b'\r') {
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
                if memchr::memchr(b'\n', &body[..end]).is_some() {
                    self.newline = true;
                }
                self.pos = (self.pos + 2 + end + 2).min(self.bytes.len());
                return Ok(());
            }
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
        if is_ident_start(byte) {
            self.pos += word.len().max(1);
            return self.word(word, start);
        }
        if self.top().head == Head::Start {
            self.top().head = Head::Other;
        }
        if byte.is_ascii_digit() || (byte == b'.' && self.peek(1).is_ascii_digit()) {
            while self.pos < self.bytes.len()
                && (is_ident_part(self.bytes[self.pos]) || self.bytes[self.pos] == b'.')
            {
                self.pos += 1;
            }
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
            b'{' => {
                self.pos += 1;
                let top = *self.top();
                let types = match top.head {
                    Head::Interface => true,
                    Head::Enum => false,
                    _ => top.segment_types,
                };
                // A block or body follows an operand (a function's or a
                // class's head), a statement head, an arrow, `else`, `do`,
                // `try` or `finally`, or starts a statement; anything else
                // opens an object literal.
                let block = match self.last {
                    Last::Operand | Last::StatementEnd => true,
                    Last::ExpressionStart => {
                        self.last_arrow
                            || matches!(self.last_word, b"else" | b"do" | b"try" | b"finally")
                            || (self.last_word.is_empty()
                                && top.chain == 0
                                && matches!(top.kind, GroupKind::Root | GroupKind::Brace))
                    }
                };
                self.push(GroupKind::Brace, types)?;
                let group = self.top();
                group.statement_head = block;
                group.class_body = top.head == Head::Class;
                self.expression_start();
            }
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
                self.last_word = b"";
                self.last_arrow = false;
                self.last_closer = true;
                self.last_parameters = matches!(
                    closed,
                    Some(group) if group.kind == GroupKind::Paren && !group.after_keyword
                );
                self.last = match closed {
                    Some(group) if group.statement_head => Last::StatementEnd,
                    _ => Last::Operand,
                };
            }
            b',' | b';' => {
                self.pos += 1;
                let head = self.top().head;
                self.reset();
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
                self.colon();
                self.expression_start();
            }
            b'/' if self.last != Last::Operand => {
                self.regex(start)?;
                self.operand();
            }
            b'<' if self.jsx
                && self.last != Last::Operand
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

    /// A `:` closes a pending conditional, or starts an annotation where
    /// one can stand: in a parameter list, after a parameter list (a
    /// return type), after a variable's name, or after a class member's.
    fn colon(&mut self) {
        let last_parameters = self.last_parameters;
        let top = self.top();
        if top.questions > 0 {
            top.questions -= 1;
            return;
        }
        let annotation = top.kind == GroupKind::Paren
            || last_parameters
            || (matches!(top.kind, GroupKind::Root | GroupKind::Brace)
                && (top.head == Head::Declaration || top.class_body));
        if annotation {
            top.segment_types = true;
            top.member_base = top.chain;
        }
    }

    /// The identifier or keyword starting at `start`: ASCII identifier
    /// characters, an escape's backslash and the byte after it, and every
    /// byte of a multi-byte character.
    fn word_at(&self, start: usize) -> &'s [u8] {
        let mut end = start;
        while end < self.bytes.len() && is_ident_part(self.bytes[end]) {
            if self.bytes[end] == b'\\' {
                end += 1;
            }
            end += 1;
        }
        &self.bytes[start..end.min(self.bytes.len())]
    }

    fn word(&mut self, word: &'s [u8], start: usize) -> Result<(), NestingExceeded> {
        let flags = keyword(word);
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
        if flags & LINK != 0 && self.top().segment_types {
            // A type operator (`keyof`, `typeof`, …) binds within one
            // union member.
            self.link_member(start)?;
        } else if flags & LINK != 0 {
            self.link(start)?;
        }
        self.last_word = word;
        self.last_arrow = false;
        self.last_closer = false;
        self.last_parameters = false;
        self.last = if flags & EXPRESSION != 0 {
            Last::ExpressionStart
        } else {
            Last::Operand
        };
        Ok(())
    }

    fn operand(&mut self) {
        self.last = Last::Operand;
        self.last_word = b"";
        self.last_arrow = false;
        self.last_closer = false;
        self.last_parameters = false;
    }

    fn expression_start(&mut self) {
        self.last = Last::ExpressionStart;
        self.last_word = b"";
        self.last_arrow = false;
        self.last_closer = false;
        self.last_parameters = false;
    }

    fn string(&mut self, quote: u8) {
        self.pos += 1;
        while self.pos < self.bytes.len() {
            match memchr::memchr3(quote, b'\\', b'\n', &self.bytes[self.pos..]) {
                Some(at) => self.pos += at,
                None => {
                    self.pos = self.bytes.len();
                    return;
                }
            }
            match self.bytes[self.pos] {
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
    /// top of the current depth, so reading a division as one never
    /// hides a level.
    fn regex(&mut self, start: usize) -> Result<(), NestingExceeded> {
        let top = *self.top();
        let base = top.base + top.chain + 1;
        let mut local = 0u32;
        self.pos += 1;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'\\' => self.pos += 1,
                b'\n' => break,
                b'/' => {
                    self.pos += 1;
                    while self.pos < self.bytes.len() && is_ident_part(self.bytes[self.pos]) {
                        self.pos += 1;
                    }
                    return Ok(());
                }
                b'(' | b'[' | b'{' => {
                    local += 1;
                    self.check(base + local, start)?;
                }
                b')' | b']' | b'}' => local = local.saturating_sub(1),
                _ => {}
            }
            self.pos += 1;
        }
        self.pos = self.pos.min(self.bytes.len());
        Ok(())
    }

    fn punctuator(&mut self, start: usize) -> Result<(), NestingExceeded> {
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
        if matches!(op, b">" | b">>" | b">>>") && top.kind == GroupKind::Angle {
            // Each `>` closes one list.
            for _ in 0..op.len() {
                if self.pop(&[GroupKind::Angle]).is_none() {
                    break;
                }
            }
            self.operand();
            self.last_closer = true;
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
        // A type alias's head: `=` outside its type parameters starts the
        // right-hand side; any other `=` starts an initializer.
        match (top.head, op) {
            (Head::TypeAlias { named: true }, b"<") => self.top().head_angles += 1,
            (Head::TypeAlias { named: true }, b">") => {
                let group = self.top();
                group.head_angles = group.head_angles.saturating_sub(1);
            }
            (Head::TypeAlias { named: true }, b"=") if top.head_angles == 0 => {
                let group = self.top();
                group.head = Head::TypeAliasBody;
                group.segment_types = true;
                group.member_base = group.chain;
            }
            (Head::TypeAliasBody, _) => {}
            (_, b"=") => self.top().segment_types = false,
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
        }
        Ok(())
    }

    /// Whether the `<` at `pos` opens a JSX element rather than a TSX
    /// arrow function's type parameters (`<T,>` / `<T extends U>`).
    fn jsx_tag_start(&self) -> bool {
        let next = self.peek(1);
        if next == b'>' {
            return true;
        }
        if !is_ident_start(next) {
            return false;
        }
        let name = self.word_at(self.pos + 1);
        let mut after = self.pos + 1 + name.len();
        while after < self.bytes.len() && self.bytes[after].is_ascii_whitespace() {
            after += 1;
        }
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

    /// After a JSX element closes: back among its parent's children, or an
    /// operand in the script.
    fn jsx_element_done(&mut self) {
        if self.top().kind != GroupKind::JsxChildren {
            self.operand();
        }
    }

    /// Among a JSX element's children: text up to a tag or a `{`.
    fn jsx_children(&mut self) -> Result<(), NestingExceeded> {
        match self.bytes[self.pos] {
            b'<' if self.peek(1) == b'/' => {
                self.pos += 2;
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
