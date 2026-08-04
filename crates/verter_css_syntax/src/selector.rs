use smallvec::SmallVec;
use verter_span::Span;

use crate::diagnostic::{CssParseFailure, CssStructureTooLarge};
use crate::dialect::CssDialect;
use crate::event::{ParseEvent, ParseEventSink, SyntaxKind};
use crate::parser::{parse_with_sink, CssEntryPoint, CssParseMode, CssSource};
use crate::token::{
    css_identifier_eq_ignore_ascii_case, decode_css_identifier, SyntaxToken, TokenFlags, TokenKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorKind {
    Complex,
    Relative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorComponentKind {
    Namespace,
    Attribute,
    Nesting,
    PseudoClass,
    PseudoElement,
    FunctionalPseudo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombinatorKind {
    Descendant,
    Child,
    NextSibling,
    LaterSibling,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeMatcher {
    Exact,
    Includes,
    DashMatch,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PseudoFunctionKind {
    Is,
    Where,
    Not,
    Has,
    NthChild,
    NthLastChild,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NthExpression {
    pub a: i32,
    pub b: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorComponent {
    kind: SelectorComponentKind,
    span: Span,
}

impl SelectorComponent {
    #[inline]
    pub const fn kind(&self) -> SelectorComponentKind {
        self.kind
    }

    #[inline]
    pub const fn span(&self) -> Span {
        self.span
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorCombinator {
    kind: CombinatorKind,
    span: Span,
}

impl SelectorCombinator {
    #[inline]
    pub const fn kind(&self) -> CombinatorKind {
        self.kind
    }

    #[inline]
    pub const fn span(&self) -> Span {
        self.span
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorAttribute {
    span: Span,
    matcher: Option<AttributeMatcher>,
}

impl SelectorAttribute {
    #[inline]
    pub const fn span(&self) -> Span {
        self.span
    }

    #[inline]
    pub const fn matcher(&self) -> Option<AttributeMatcher> {
        self.matcher
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorPseudo {
    span: Span,
    argument_span: Span,
    kind: PseudoFunctionKind,
    selector_count: u32,
    nth: Option<NthExpression>,
}

impl SelectorPseudo {
    #[inline]
    pub const fn span(&self) -> Span {
        self.span
    }

    #[inline]
    pub const fn argument_span(&self) -> Span {
        self.argument_span
    }

    #[inline]
    pub const fn kind(&self) -> PseudoFunctionKind {
        self.kind
    }

    #[inline]
    pub const fn selector_count(&self) -> u32 {
        self.selector_count
    }

    #[inline]
    pub const fn nth(&self) -> Option<NthExpression> {
        self.nth
    }
}

pub struct SelectorStructure {
    source: CssSource,
    span: Span,
    top_level_selector_count: u32,
    components: Vec<SelectorComponent>,
    combinators: Vec<SelectorCombinator>,
    attributes: Vec<SelectorAttribute>,
    pseudos: Vec<SelectorPseudo>,
}

impl SelectorStructure {
    #[inline]
    pub const fn span(&self) -> Span {
        self.span
    }

    #[inline]
    pub const fn top_level_selector_count(&self) -> u32 {
        self.top_level_selector_count
    }

    #[inline]
    pub fn source(&self) -> &CssSource {
        &self.source
    }

    #[inline]
    pub fn components(&self) -> &[SelectorComponent] {
        &self.components
    }

    #[inline]
    pub fn combinators(&self) -> &[SelectorCombinator] {
        &self.combinators
    }

    #[inline]
    pub fn attributes(&self) -> &[SelectorAttribute] {
        &self.attributes
    }

    #[inline]
    pub fn pseudos(&self) -> &[SelectorPseudo] {
        &self.pseudos
    }
}

#[derive(Debug, Clone, Copy)]
struct OpenNode {
    kind: SyntaxKind,
    start: u32,
    token_start: usize,
    selector_count: u32,
}

struct SelectorSink {
    source: CssSource,
    open: SmallVec<[OpenNode; 16]>,
    selector_list_depth: u32,
    top_level_selector_count: u32,
    components: Vec<SelectorComponent>,
    combinators: Vec<SelectorCombinator>,
    attributes: Vec<SelectorAttribute>,
    pseudos: Vec<SelectorPseudo>,
    tokens: Vec<SyntaxToken>,
}

impl SelectorSink {
    fn new(source: CssSource) -> Self {
        Self {
            source,
            open: SmallVec::new(),
            selector_list_depth: 0,
            top_level_selector_count: 0,
            components: Vec::new(),
            combinators: Vec::new(),
            attributes: Vec::new(),
            pseudos: Vec::new(),
            tokens: Vec::new(),
        }
    }

    fn finish(self) -> SelectorStructure {
        SelectorStructure {
            span: Span::new(self.source.origin(), self.source.end()),
            source: self.source,
            top_level_selector_count: self.top_level_selector_count,
            components: self.components,
            combinators: self.combinators,
            attributes: self.attributes,
            pseudos: self.pseudos,
        }
    }

    fn record_node(&mut self, open: OpenNode, end: u32) {
        let span = Span::new(open.start, end);
        let tokens = &self.tokens[open.token_start..];
        match open.kind {
            SyntaxKind::Combinator => {
                let kind = combinator_kind(tokens, &self.source);
                self.combinators.push(SelectorCombinator { kind, span });
            }
            SyntaxKind::NamespaceSelector => {
                self.components.push(SelectorComponent {
                    kind: SelectorComponentKind::Namespace,
                    span,
                });
            }
            SyntaxKind::AttributeSelector => {
                let matcher = attribute_matcher(tokens, &self.source);
                self.attributes.push(SelectorAttribute { span, matcher });
                self.components.push(SelectorComponent {
                    kind: SelectorComponentKind::Attribute,
                    span,
                });
            }
            SyntaxKind::NestingSelector => self.components.push(SelectorComponent {
                kind: SelectorComponentKind::Nesting,
                span,
            }),
            SyntaxKind::PseudoElement => self.components.push(SelectorComponent {
                kind: SelectorComponentKind::PseudoElement,
                span,
            }),
            SyntaxKind::PseudoClass => self.components.push(SelectorComponent {
                kind: SelectorComponentKind::PseudoClass,
                span,
            }),
            SyntaxKind::PseudoSelectorList
            | SyntaxKind::NthSelector
            | SyntaxKind::UnknownPseudoFunction => {
                let kind = pseudo_kind(tokens, &self.source);
                let argument_span = pseudo_argument_span(tokens, span);
                let nth = matches!(
                    kind,
                    PseudoFunctionKind::NthChild | PseudoFunctionKind::NthLastChild
                )
                .then(|| parse_an_plus_b_tokens(tokens, &self.source))
                .flatten();
                self.pseudos.push(SelectorPseudo {
                    span,
                    argument_span,
                    kind,
                    selector_count: open.selector_count,
                    nth,
                });
                self.components.push(SelectorComponent {
                    kind: SelectorComponentKind::FunctionalPseudo,
                    span,
                });
            }
            _ => {}
        }
    }
}

impl ParseEventSink for SelectorSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        match event {
            ParseEvent::StartNode { kind, start, .. } => {
                if kind == SyntaxKind::Selector {
                    if let Some(pseudo) = self.open.iter_mut().rev().find(|node| {
                        matches!(
                            node.kind,
                            SyntaxKind::PseudoSelectorList | SyntaxKind::NthSelector
                        )
                    }) {
                        pseudo.selector_count = pseudo.selector_count.saturating_add(1);
                    }
                }
                if kind == SyntaxKind::SelectorList {
                    self.selector_list_depth += 1;
                } else if kind == SyntaxKind::Selector && self.selector_list_depth == 1 {
                    self.top_level_selector_count += 1;
                }
                self.open.push(OpenNode {
                    kind,
                    start,
                    token_start: self.tokens.len(),
                    selector_count: 0,
                });
            }
            ParseEvent::Token(token) => self.tokens.push(token),
            ParseEvent::FinishNode { kind, end } => {
                let open = self
                    .open
                    .pop()
                    .expect("parser emits balanced selector nodes");
                debug_assert_eq!(open.kind, kind);
                self.record_node(open, end);
                if kind == SyntaxKind::SelectorList {
                    self.selector_list_depth -= 1;
                }
            }
            ParseEvent::Diagnostic(_) => {}
        }
        Ok(())
    }
}

pub fn parse_selector_structure(
    source: &CssSource,
    dialect: CssDialect,
) -> Result<SelectorStructure, CssParseFailure> {
    let mut sink = SelectorSink::new(source.clone());
    parse_with_sink(
        source,
        dialect,
        CssEntryPoint::SelectorList,
        CssParseMode::Strict,
        &mut sink,
    )?;
    Ok(sink.finish())
}

fn combinator_kind(tokens: &[SyntaxToken], source: &CssSource) -> CombinatorKind {
    let mut significant = tokens
        .iter()
        .copied()
        .filter(|token| !token.kind().is_trivia());
    let Some(first) = significant.next() else {
        return CombinatorKind::Descendant;
    };
    let text = source.token_text(first);
    if text == ">" {
        CombinatorKind::Child
    } else if text == "+" {
        CombinatorKind::NextSibling
    } else if text == "~" {
        CombinatorKind::LaterSibling
    } else if text == "|"
        && significant
            .next()
            .is_some_and(|token| source.token_text(token) == "|")
    {
        CombinatorKind::Column
    } else {
        CombinatorKind::Descendant
    }
}

fn attribute_matcher(tokens: &[SyntaxToken], source: &CssSource) -> Option<AttributeMatcher> {
    let mut index = tokens
        .iter()
        .position(|token| token.kind() == TokenKind::LeftBracket)?
        + 1;
    let first = next_attribute_non_trivia(tokens, &mut index)?;
    if is_attribute_local_name(first) {
        let after_first = index;
        let mut namespace_probe = index;
        if next_attribute_comment_adjacent(tokens, &mut namespace_probe)
            .is_some_and(|token| is_pipe(token, source))
            && next_attribute_comment_adjacent(tokens, &mut namespace_probe)
                .is_some_and(is_attribute_local_name)
        {
            index = namespace_probe;
        } else {
            index = after_first;
        }
    } else if is_attribute_namespace_prefix(first, source) {
        if !(next_attribute_comment_adjacent(tokens, &mut index)
            .is_some_and(|token| is_pipe(token, source))
            && next_attribute_comment_adjacent(tokens, &mut index)
                .is_some_and(is_attribute_local_name))
        {
            return None;
        }
    } else if !(is_pipe(first, source)
        && next_attribute_comment_adjacent(tokens, &mut index).is_some_and(is_attribute_local_name))
    {
        return None;
    }

    let operator = next_attribute_non_trivia(tokens, &mut index)?;
    if operator.kind() != TokenKind::Delim {
        return None;
    }
    let text = source.token_text(operator);
    if text == "=" {
        return Some(AttributeMatcher::Exact);
    }
    let equals = next_attribute_comment_adjacent(tokens, &mut index)?;
    if equals.kind() != TokenKind::Delim || source.token_text(equals) != "=" {
        return None;
    }
    match text {
        "~" => Some(AttributeMatcher::Includes),
        "|" => Some(AttributeMatcher::DashMatch),
        "^" => Some(AttributeMatcher::Prefix),
        "$" => Some(AttributeMatcher::Suffix),
        "*" => Some(AttributeMatcher::Substring),
        _ => None,
    }
}

fn next_attribute_non_trivia(tokens: &[SyntaxToken], index: &mut usize) -> Option<SyntaxToken> {
    while tokens
        .get(*index)
        .is_some_and(|token| token.kind().is_trivia())
    {
        *index += 1;
    }
    let token = tokens.get(*index).copied()?;
    *index += 1;
    Some(token)
}

fn next_attribute_comment_adjacent(
    tokens: &[SyntaxToken],
    index: &mut usize,
) -> Option<SyntaxToken> {
    while tokens
        .get(*index)
        .is_some_and(|token| token.kind() == TokenKind::Comment)
    {
        *index += 1;
    }
    let token = tokens.get(*index).copied()?;
    *index += 1;
    Some(token)
}

fn is_attribute_namespace_prefix(token: SyntaxToken, source: &CssSource) -> bool {
    is_attribute_local_name(token)
        || (token.kind() == TokenKind::Delim && source.token_text(token) == "*")
}

fn is_attribute_local_name(token: SyntaxToken) -> bool {
    token.kind() == TokenKind::Ident
}

fn is_pipe(token: SyntaxToken, source: &CssSource) -> bool {
    token.kind() == TokenKind::Delim && source.token_text(token) == "|"
}

fn pseudo_kind(tokens: &[SyntaxToken], source: &CssSource) -> PseudoFunctionKind {
    let Some(function) = tokens
        .iter()
        .copied()
        .find(|token| token.kind() == TokenKind::Function)
    else {
        return PseudoFunctionKind::Unknown;
    };
    let text = source.token_text(function);
    let name = &text[..text.len() - 1];
    if css_identifier_eq_ignore_ascii_case(name, "is") {
        PseudoFunctionKind::Is
    } else if css_identifier_eq_ignore_ascii_case(name, "where") {
        PseudoFunctionKind::Where
    } else if css_identifier_eq_ignore_ascii_case(name, "not") {
        PseudoFunctionKind::Not
    } else if css_identifier_eq_ignore_ascii_case(name, "has") {
        PseudoFunctionKind::Has
    } else if css_identifier_eq_ignore_ascii_case(name, "nth-child") {
        PseudoFunctionKind::NthChild
    } else if css_identifier_eq_ignore_ascii_case(name, "nth-last-child") {
        PseudoFunctionKind::NthLastChild
    } else {
        PseudoFunctionKind::Unknown
    }
}

fn pseudo_argument_span(tokens: &[SyntaxToken], fallback: Span) -> Span {
    let Some(function_index) = tokens
        .iter()
        .position(|token| token.kind() == TokenKind::Function)
    else {
        return Span::new(fallback.end, fallback.end);
    };
    let start = tokens[function_index].end;
    let end = tokens[function_index + 1..]
        .iter()
        .rev()
        .find(|token| token.kind() == TokenKind::RightParen)
        .map_or(fallback.end, |token| token.start);
    Span::new(start, end)
}

fn parse_an_plus_b_tokens(tokens: &[SyntaxToken], source: &CssSource) -> Option<NthExpression> {
    let function_index = tokens
        .iter()
        .position(|token| token.kind() == TokenKind::Function)?;
    let formula_end = tokens[function_index + 1..]
        .iter()
        .position(|token| {
            token.kind() == TokenKind::RightParen
                || (token.kind() == TokenKind::Ident
                    && css_identifier_eq_ignore_ascii_case(source.token_text(*token), "of"))
        })
        .map_or(tokens.len(), |offset| function_index + 1 + offset);
    let formula = &tokens[function_index + 1..formula_end];
    let mut index = 0usize;
    let expression = parse_an_plus_b_formula(formula, source, &mut index)?;
    nth_next_token(formula, &mut index, true)
        .is_none()
        .then_some(expression)
}

fn parse_an_plus_b_formula(
    tokens: &[SyntaxToken],
    source: &CssSource,
    index: &mut usize,
) -> Option<NthExpression> {
    let first = nth_next_token(tokens, index, true)?;
    match first.kind() {
        TokenKind::Number => Some(NthExpression {
            a: 0,
            b: integer_token_value(first, source)?,
        }),
        TokenKind::Dimension => {
            let a = integer_token_value(first, source)?;
            let raw = source.token_text(first);
            let unit = decode_css_identifier(&raw[dimension_unit_start(raw)..]).ok()?;
            if unit.eq_ignore_ascii_case("n") {
                parse_nth_b(tokens, source, index, a)
            } else if unit.eq_ignore_ascii_case("n-") {
                parse_nth_signless_b(tokens, source, index, a, -1)
            } else {
                Some(NthExpression {
                    a,
                    b: parse_n_dash_digits(unit.as_ref())?,
                })
            }
        }
        TokenKind::Ident => {
            let value = decode_css_identifier(source.token_text(first)).ok()?;
            if value.eq_ignore_ascii_case("even") {
                Some(NthExpression { a: 2, b: 0 })
            } else if value.eq_ignore_ascii_case("odd") {
                Some(NthExpression { a: 2, b: 1 })
            } else if value.eq_ignore_ascii_case("n") {
                parse_nth_b(tokens, source, index, 1)
            } else if value.eq_ignore_ascii_case("-n") {
                parse_nth_b(tokens, source, index, -1)
            } else if value.eq_ignore_ascii_case("n-") {
                parse_nth_signless_b(tokens, source, index, 1, -1)
            } else if value.eq_ignore_ascii_case("-n-") {
                parse_nth_signless_b(tokens, source, index, -1, -1)
            } else {
                let (digits, a) = value
                    .strip_prefix('-')
                    .map_or((value.as_ref(), 1), |digits| (digits, -1));
                Some(NthExpression {
                    a,
                    b: parse_n_dash_digits(digits)?,
                })
            }
        }
        TokenKind::Delim if source.token_text(first) == "+" => {
            let adjacent = nth_next_token(tokens, index, false)?;
            if adjacent.kind() != TokenKind::Ident {
                return None;
            }
            let value = decode_css_identifier(source.token_text(adjacent)).ok()?;
            if value.eq_ignore_ascii_case("n") {
                parse_nth_b(tokens, source, index, 1)
            } else if value.eq_ignore_ascii_case("n-") {
                parse_nth_signless_b(tokens, source, index, 1, -1)
            } else {
                Some(NthExpression {
                    a: 1,
                    b: parse_n_dash_digits(value.as_ref())?,
                })
            }
        }
        _ => None,
    }
}

fn parse_nth_b(
    tokens: &[SyntaxToken],
    source: &CssSource,
    index: &mut usize,
    a: i32,
) -> Option<NthExpression> {
    let reset = *index;
    let Some(token) = nth_next_token(tokens, index, true) else {
        return Some(NthExpression { a, b: 0 });
    };
    if token.kind() == TokenKind::Delim && source.token_text(token) == "+" {
        return parse_nth_signless_b(tokens, source, index, a, 1);
    }
    if token.kind() == TokenKind::Delim && source.token_text(token) == "-" {
        return parse_nth_signless_b(tokens, source, index, a, -1);
    }
    if token.kind() == TokenKind::Number
        && source
            .token_text(token)
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
    {
        return Some(NthExpression {
            a,
            b: integer_token_value(token, source)?,
        });
    }
    *index = reset;
    Some(NthExpression { a, b: 0 })
}

fn parse_nth_signless_b(
    tokens: &[SyntaxToken],
    source: &CssSource,
    index: &mut usize,
    a: i32,
    sign: i32,
) -> Option<NthExpression> {
    let token = nth_next_token(tokens, index, true)?;
    let raw = source.token_text(token);
    if token.kind() != TokenKind::Number
        || raw
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
    {
        return None;
    }
    Some(NthExpression {
        a,
        b: sign.checked_mul(integer_token_value(token, source)?)?,
    })
}

fn nth_next_token(
    tokens: &[SyntaxToken],
    index: &mut usize,
    skip_whitespace: bool,
) -> Option<SyntaxToken> {
    loop {
        let token = tokens.get(*index).copied()?;
        *index += 1;
        match token.kind() {
            TokenKind::Comment => {}
            TokenKind::Whitespace | TokenKind::LineComment if skip_whitespace => {}
            _ => return Some(token),
        }
    }
}

fn integer_token_value(token: SyntaxToken, source: &CssSource) -> Option<i32> {
    if token.flags & TokenFlags::NUMBER_INTEGER == 0 {
        return None;
    }
    let raw = source.token_text(token);
    let number_end = if token.kind() == TokenKind::Dimension {
        dimension_unit_start(raw)
    } else {
        raw.len()
    };
    raw[..number_end].parse().ok()
}

fn parse_n_dash_digits(value: &str) -> Option<i32> {
    let bytes = value.as_bytes();
    if bytes.len() < 3
        || !bytes[..2].eq_ignore_ascii_case(b"n-")
        || !bytes[2..].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    value[1..].parse().ok()
}

fn dimension_unit_start(raw: &str) -> usize {
    let bytes = raw.as_bytes();
    let mut cursor = usize::from(
        bytes
            .first()
            .is_some_and(|byte| matches!(byte, b'+' | b'-')),
    );
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if bytes.get(cursor) == Some(&b'.') && bytes.get(cursor + 1).is_some_and(u8::is_ascii_digit) {
        cursor += 2;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
    }
    if bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        let exponent_end = if bytes.get(cursor + 1).is_some_and(u8::is_ascii_digit) {
            Some(cursor + 2)
        } else if bytes
            .get(cursor + 1)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
            && bytes.get(cursor + 2).is_some_and(u8::is_ascii_digit)
        {
            Some(cursor + 3)
        } else {
            None
        };
        if let Some(mut end) = exponent_end {
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            cursor = end;
        }
    }
    cursor
}
