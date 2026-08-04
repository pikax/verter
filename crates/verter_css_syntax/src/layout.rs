use std::sync::Arc;

use verter_span::Span;

use crate::diagnostic::{
    CssDiagnostic, CssDiagnosticKind, CssParseFailure, CssSeverity, CssStructureTooLarge,
    RecoveryKind, StructureOverflowKind,
};
use crate::dialect::CssDialect;
use crate::event::{NodeFlags, ParseEvent, ParseEventSink, ParseSummary, SyntaxKind};
use crate::lexer::Lexer;
use crate::parser::{parse_with_sink, CssEntryPoint, CssParseMode, CssSource};
use crate::token::{css_identifier_eq_ignore_ascii_case, SyntaxToken, TokenFlags, TokenKind};

pub(crate) fn should_use_layout(source: &CssSource, dialect: CssDialect) -> bool {
    match dialect {
        CssDialect::Css => false,
        CssDialect::Sass | CssDialect::Stylus => true,
        CssDialect::Scss | CssDialect::Less => {
            let tokens: Vec<_> = Lexer::new(source, dialect).collect();
            !tokens
                .iter()
                .any(|token| token.kind() == TokenKind::LeftBrace)
                && has_indentation_structure(source, &tokens)
        }
    }
}

pub(crate) fn parse_layout(
    source: &CssSource,
    dialect: CssDialect,
    mode: CssParseMode,
    sink: &mut impl ParseEventSink,
) -> Result<ParseSummary, CssParseFailure> {
    LayoutParser::new(source, dialect, mode).parse(sink)
}

struct LayoutParser<'a> {
    source: &'a CssSource,
    dialect: CssDialect,
    mode: CssParseMode,
    tokens: Vec<SyntaxToken>,
    cursor: usize,
    summary: ParseSummary,
    indent_levels: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryKind {
    Newline,
    Semicolon,
    Block,
    RightBrace,
    End,
}

#[derive(Debug, Clone, Copy)]
struct Boundary {
    kind: BoundaryKind,
    index: usize,
    unterminated_interpolation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatementClass {
    Rule,
    Declaration,
    Variable,
    Directive(SyntaxKind),
    MixinOrFunction,
    Ambiguous,
}

impl<'a> LayoutParser<'a> {
    fn new(source: &'a CssSource, dialect: CssDialect, mode: CssParseMode) -> Self {
        Self {
            source,
            dialect,
            mode,
            tokens: Lexer::new(source, dialect).collect(),
            cursor: 0,
            summary: ParseSummary::default(),
            indent_levels: vec![Vec::new()],
        }
    }

    fn parse(mut self, sink: &mut impl ParseEventSink) -> Result<ParseSummary, CssParseFailure> {
        self.emit(
            sink,
            ParseEvent::StartNode {
                kind: SyntaxKind::Stylesheet,
                flags: NodeFlags::default(),
                start: self.source.origin(),
            },
        )?;
        self.parse_list(sink, None, false)?;
        while self.cursor < self.tokens.len() {
            self.emit_token(sink, self.tokens[self.cursor])?;
            self.cursor += 1;
        }
        self.emit(
            sink,
            ParseEvent::FinishNode {
                kind: SyntaxKind::Stylesheet,
                end: self.source.end(),
            },
        )?;
        Ok(self.summary)
    }

    fn parse_list(
        &mut self,
        sink: &mut impl ParseEventSink,
        expected_indent: Option<&[u8]>,
        stop_at_right_brace: bool,
    ) -> Result<(), CssParseFailure> {
        loop {
            self.emit_trivia(sink)?;
            let Some(token) = self.tokens.get(self.cursor).copied() else {
                return Ok(());
            };
            if stop_at_right_brace && token.kind() == TokenKind::RightBrace {
                return Ok(());
            }

            let indent = indent_prefix(self.source, &self.tokens, self.cursor);
            if expected_indent.is_none() && !stop_at_right_brace && !indent.is_empty() {
                self.diagnostic(
                    sink,
                    CssDiagnosticKind::UnexpectedIndentation,
                    Span::new(token.start, token.start),
                    RecoveryKind::AdvanceToBoundary,
                )?;
                self.parse_statement(sink, &indent, true)?;
                continue;
            }
            if let Some(expected) = expected_indent {
                if indent == expected {
                    // sibling
                } else if prior_level(&self.indent_levels, &indent).is_some() {
                    return Ok(());
                } else if indent.starts_with(expected) && indent.len() > expected.len() {
                    self.diagnostic(
                        sink,
                        CssDiagnosticKind::UnexpectedIndentation,
                        Span::new(token.start, token.start),
                        RecoveryKind::AdvanceToBoundary,
                    )?;
                    self.parse_statement(sink, &indent, true)?;
                    continue;
                } else {
                    self.diagnostic(
                        sink,
                        CssDiagnosticKind::InconsistentIndentation,
                        Span::new(token.start, token.start),
                        RecoveryKind::AdvanceToBoundary,
                    )?;
                    return Ok(());
                }
            }
            self.parse_statement(sink, &indent, false)?;
        }
    }

    fn parse_statement(
        &mut self,
        sink: &mut impl ParseEventSink,
        indent: &[u8],
        forced_ambiguous: bool,
    ) -> Result<(), CssParseFailure> {
        let start_index = self.cursor;
        let boundary = self.find_boundary(start_index);
        let header_end = trim_trailing_trivia(&self.tokens, start_index, boundary.index);
        if header_end == start_index {
            if boundary.index < self.tokens.len() {
                self.emit_token(sink, self.tokens[boundary.index])?;
                self.cursor = boundary.index + 1;
            } else {
                self.cursor = boundary.index;
            }
            return Ok(());
        }

        if boundary.unterminated_interpolation {
            let end = self.tokens[header_end - 1].end;
            self.diagnostic(
                sink,
                CssDiagnosticKind::UnterminatedInterpolation,
                Span::new(end, end),
                RecoveryKind::AdvanceToBoundary,
            )?;
        }

        let next_index = match boundary.kind {
            BoundaryKind::Newline => next_significant(&self.tokens, boundary.index + 1),
            BoundaryKind::Semicolon => next_significant(&self.tokens, boundary.index + 1),
            BoundaryKind::Block | BoundaryKind::RightBrace | BoundaryKind::End => None,
        };
        let next_indent = next_index.map(|index| indent_prefix(self.source, &self.tokens, index));
        let has_indented_body = next_indent
            .as_deref()
            .is_some_and(|next| next.starts_with(indent) && next.len() > indent.len());
        let mut class = self.classify(
            start_index,
            header_end,
            has_indented_body || boundary.kind == BoundaryKind::Block,
        );
        if forced_ambiguous {
            class = StatementClass::Ambiguous;
        }

        let outer_kind = match class {
            StatementClass::Rule => SyntaxKind::QualifiedRule,
            StatementClass::Declaration => SyntaxKind::Declaration,
            StatementClass::Variable => SyntaxKind::VariableDeclaration,
            StatementClass::Directive(kind) => kind,
            StatementClass::MixinOrFunction => SyntaxKind::MixinOrFunctionHeader,
            StatementClass::Ambiguous => SyntaxKind::AmbiguousStatement,
        };
        let start = self.tokens[start_index].start;
        self.start(sink, outer_kind, start, class == StatementClass::Ambiguous)?;
        if class == StatementClass::Ambiguous {
            self.diagnostic(
                sink,
                CssDiagnosticKind::AmbiguousStatement,
                Span::new(start, self.tokens[header_end - 1].end),
                RecoveryKind::None,
            )?;
        }

        match class {
            StatementClass::Rule => self.emit_selector_range(sink, start_index, header_end)?,
            StatementClass::Declaration | StatementClass::Variable => {
                self.emit_declaration_header(sink, start_index, header_end)?
            }
            StatementClass::Directive(_) => {
                self.emit_token(sink, self.tokens[start_index])?;
                if start_index + 1 < header_end {
                    self.emit_value_range(
                        sink,
                        SyntaxKind::AtRulePrelude,
                        start_index + 1,
                        header_end,
                    )?;
                }
            }
            StatementClass::MixinOrFunction | StatementClass::Ambiguous => {
                self.emit_value_range(
                    sink,
                    SyntaxKind::ComponentValueList,
                    start_index,
                    header_end,
                )?;
            }
        }
        self.cursor = header_end;
        while self.cursor < boundary.index {
            self.emit_token(sink, self.tokens[self.cursor])?;
            self.cursor += 1;
        }

        let mut end = self.tokens[header_end - 1].end;
        if boundary.kind == BoundaryKind::Block {
            let block_kind = if matches!(class, StatementClass::Directive(_)) {
                SyntaxKind::AtRuleBlock
            } else {
                SyntaxKind::RuleBlock
            };
            let opener = self.tokens[boundary.index];
            self.start(sink, block_kind, opener.start, false)?;
            self.emit_token(sink, opener)?;
            self.cursor = boundary.index + 1;
            self.parse_list(sink, None, true)?;
            if self
                .tokens
                .get(self.cursor)
                .is_some_and(|token| token.kind() == TokenKind::RightBrace)
            {
                let closer = self.tokens[self.cursor];
                self.emit_token(sink, closer)?;
                self.cursor += 1;
                end = closer.end;
            } else {
                end = self.current_position();
                self.diagnostic(
                    sink,
                    CssDiagnosticKind::UnterminatedBlock,
                    Span::new(end, end),
                    RecoveryKind::CloseAtEndOfInput,
                )?;
            }
            self.finish(sink, block_kind, end)?;
        } else {
            if boundary.kind == BoundaryKind::Semicolon {
                let terminator = self.tokens[boundary.index];
                self.emit_token(sink, terminator)?;
                self.cursor = boundary.index + 1;
                end = terminator.end;
            } else {
                self.cursor = boundary.index;
            }

            if has_indented_body {
                let child_indent = next_indent.expect("indented lookahead has a prefix");
                let block_start = self
                    .tokens
                    .get(self.cursor)
                    .map_or(end, |token| token.start);
                self.start(sink, SyntaxKind::IndentedBlock, block_start, false)?;
                self.indent_levels.push(child_indent.clone());
                self.parse_list(sink, Some(&child_indent), false)?;
                self.indent_levels.pop();
                end = self.current_position();
                self.finish(sink, SyntaxKind::IndentedBlock, end)?;
            }
        }

        self.finish(sink, outer_kind, end)
    }

    fn classify(&self, start: usize, end: usize, has_body: bool) -> StatementClass {
        let first = self.tokens[start];
        if first.kind() == TokenKind::AtKeyword {
            let name = self.source.token_text(first).trim_start_matches('@');
            if ["mixin", "function", "include", "extend"]
                .iter()
                .any(|expected| css_identifier_eq_ignore_ascii_case(name, expected))
            {
                return StatementClass::MixinOrFunction;
            }
            let kind = if ["if", "else", "for", "each", "while", "return"]
                .iter()
                .any(|expected| css_identifier_eq_ignore_ascii_case(name, expected))
            {
                SyntaxKind::ControlDirective
            } else {
                SyntaxKind::UnknownAtRule
            };
            return StatementClass::Directive(kind);
        }
        if matches!(
            first.kind(),
            TokenKind::ScssVariable | TokenKind::LessVariable
        ) {
            return StatementClass::Variable;
        }
        if self.dialect == CssDialect::Less
            && is_less_mixin_header(&self.tokens[start..end], self.source)
        {
            return StatementClass::MixinOrFunction;
        }
        if self.dialect == CssDialect::Stylus && first.kind() == TokenKind::Function && has_body {
            return StatementClass::MixinOrFunction;
        }
        if self.dialect == CssDialect::Sass
            && first.kind() == TokenKind::Delim
            && matches!(self.source.token_text(first), "+" | "=")
        {
            let adjacent = self
                .tokens
                .get(start + 1)
                .is_some_and(|next| next.start == first.end && next.kind() == TokenKind::Ident);
            return if adjacent {
                StatementClass::MixinOrFunction
            } else {
                StatementClass::Ambiguous
            };
        }
        if self.dialect == CssDialect::Stylus
            && has_top_level_assignment(&self.tokens[start..end], self.source)
        {
            return StatementClass::Variable;
        }

        let explicit_colon = self.tokens[start..end]
            .iter()
            .position(|token| token.kind() == TokenKind::Colon)
            .map(|offset| start + offset);
        let selector_lead = is_selector_lead(first, self.source);
        if self.dialect == CssDialect::Sass
            && first.kind() == TokenKind::Colon
            && (!has_body || has_whitespace_between_significant(&self.tokens[start..end]))
        {
            return StatementClass::Ambiguous;
        }
        if has_body {
            if explicit_colon.is_some_and(|colon| {
                colon_starts_declaration(&self.tokens[start..end], colon - start)
            }) {
                return StatementClass::Declaration;
            }
            if selector_lead || explicit_colon.is_none() {
                return StatementClass::Rule;
            }
        }
        if explicit_colon.is_some() {
            return StatementClass::Declaration;
        }
        if matches!(self.dialect, CssDialect::Sass | CssDialect::Stylus)
            && has_whitespace_between_significant(&self.tokens[start..end])
        {
            return StatementClass::Ambiguous;
        }
        if selector_lead {
            StatementClass::Ambiguous
        } else {
            StatementClass::Declaration
        }
    }

    fn emit_declaration_header(
        &mut self,
        sink: &mut impl ParseEventSink,
        start: usize,
        end: usize,
    ) -> Result<(), CssParseFailure> {
        let separator = self.tokens[start..end]
            .iter()
            .position(|token| token.kind() == TokenKind::Colon)
            .map(|offset| start + offset);
        let name_end =
            separator.unwrap_or_else(|| next_significant(&self.tokens, start + 1).unwrap_or(end));
        for index in start..name_end.min(end) {
            self.emit_token(sink, self.tokens[index])?;
        }
        let value_start = if let Some(separator) = separator {
            self.emit_token(sink, self.tokens[separator])?;
            separator + 1
        } else {
            name_end
        };
        self.emit_value_range(sink, SyntaxKind::ComponentValueList, value_start, end)
    }

    fn emit_selector_range(
        &mut self,
        sink: &mut impl ParseEventSink,
        start: usize,
        end: usize,
    ) -> Result<(), CssParseFailure> {
        self.replay_subparse(sink, CssEntryPoint::SelectorList, start, end)
    }

    fn emit_value_range(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: SyntaxKind,
        start: usize,
        end: usize,
    ) -> Result<(), CssParseFailure> {
        if start >= end {
            let at = self
                .tokens
                .get(start)
                .map_or_else(|| self.current_position(), |token| token.start);
            self.start(sink, kind, at, false)?;
            return self.finish(sink, kind, at);
        }
        if kind == SyntaxKind::ComponentValueList {
            self.replay_subparse(sink, CssEntryPoint::ComponentValueList, start, end)
        } else {
            let start_pos = self.tokens[start].start;
            self.start(sink, kind, start_pos, false)?;
            self.replay_subparse(sink, CssEntryPoint::ComponentValueList, start, end)?;
            self.finish(sink, kind, self.tokens[end - 1].end)
        }
    }

    fn replay_subparse(
        &mut self,
        sink: &mut impl ParseEventSink,
        entry: CssEntryPoint,
        start: usize,
        end: usize,
    ) -> Result<(), CssParseFailure> {
        let start_pos = self.tokens[start].start;
        let end_pos = self.tokens[end - 1].end;
        let text: Arc<str> = self.source.slice(Span::new(start_pos, end_pos)).into();
        let source = CssSource::new(text, start_pos).expect("subspan remains in the source domain");
        let mut events = VecSink::default();
        parse_with_sink(&source, self.dialect, entry, self.mode, &mut events)?;
        for event in events.events {
            self.emit(sink, event)?;
        }
        Ok(())
    }

    fn find_boundary(&self, start: usize) -> Boundary {
        let mut delimiters: Vec<(TokenKind, bool)> = Vec::new();
        let mut index = start;
        while index < self.tokens.len() {
            let token = self.tokens[index];
            match token.kind() {
                TokenKind::Function | TokenKind::LeftParen => {
                    delimiters.push((TokenKind::RightParen, false));
                }
                TokenKind::LeftBracket => {
                    delimiters.push((TokenKind::RightBracket, false));
                }
                TokenKind::ScssInterpolationStart
                | TokenKind::LessInterpolationStart
                | TokenKind::StylusInterpolationStart => {
                    delimiters.push((TokenKind::RightBrace, true));
                }
                TokenKind::LeftBrace if delimiters.is_empty() => {
                    return Boundary {
                        kind: BoundaryKind::Block,
                        index,
                        unterminated_interpolation: false,
                    };
                }
                TokenKind::LeftBrace => delimiters.push((TokenKind::RightBrace, false)),
                closing
                    if closing.is_closing_delimiter()
                        && delimiters
                            .last()
                            .is_some_and(|(expected, _)| *expected == closing) =>
                {
                    delimiters.pop();
                }
                TokenKind::RightBrace if delimiters.is_empty() => {
                    return Boundary {
                        kind: BoundaryKind::RightBrace,
                        index,
                        unterminated_interpolation: false,
                    };
                }
                TokenKind::Semicolon if delimiters.is_empty() => {
                    return Boundary {
                        kind: BoundaryKind::Semicolon,
                        index,
                        unterminated_interpolation: false,
                    };
                }
                _ if token.flags & TokenFlags::CONTAINS_NEWLINE != 0
                    && (delimiters.is_empty()
                        || delimiters.iter().any(|(_, interpolation)| *interpolation))
                    && !continues_after(&self.tokens, start, index, self.source, self.dialect) =>
                {
                    return Boundary {
                        kind: BoundaryKind::Newline,
                        index,
                        unterminated_interpolation: delimiters
                            .iter()
                            .any(|(_, interpolation)| *interpolation),
                    };
                }
                _ => {}
            }
            index += 1;
        }
        Boundary {
            kind: BoundaryKind::End,
            index,
            unterminated_interpolation: delimiters.iter().any(|(_, interpolation)| *interpolation),
        }
    }

    fn emit_trivia(&mut self, sink: &mut impl ParseEventSink) -> Result<(), CssParseFailure> {
        while self
            .tokens
            .get(self.cursor)
            .is_some_and(|token| token.kind().is_trivia())
        {
            self.emit_token(sink, self.tokens[self.cursor])?;
            self.cursor += 1;
        }
        Ok(())
    }

    fn current_position(&self) -> u32 {
        self.tokens
            .get(self.cursor)
            .map_or(self.source.end(), |token| token.start)
    }

    fn start(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: SyntaxKind,
        start: u32,
        recovered: bool,
    ) -> Result<(), CssParseFailure> {
        self.emit(
            sink,
            ParseEvent::StartNode {
                kind,
                flags: if recovered {
                    NodeFlags::RECOVERED
                } else {
                    NodeFlags::default()
                },
                start,
            },
        )
    }

    fn finish(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: SyntaxKind,
        end: u32,
    ) -> Result<(), CssParseFailure> {
        self.emit(sink, ParseEvent::FinishNode { kind, end })
    }

    fn emit_token(
        &mut self,
        sink: &mut impl ParseEventSink,
        token: SyntaxToken,
    ) -> Result<(), CssParseFailure> {
        self.emit(sink, ParseEvent::Token(token))
    }

    fn diagnostic(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: CssDiagnosticKind,
        span: Span,
        recovery: RecoveryKind,
    ) -> Result<(), CssParseFailure> {
        let diagnostic = CssDiagnostic {
            kind,
            severity: CssSeverity::Error,
            span,
            recovery,
        };
        if self.mode == CssParseMode::Strict {
            return Err(CssParseFailure::Diagnostic(diagnostic));
        }
        self.emit(sink, ParseEvent::Diagnostic(diagnostic))
    }

    fn emit(
        &mut self,
        sink: &mut impl ParseEventSink,
        event: ParseEvent,
    ) -> Result<(), CssParseFailure> {
        self.summary.events =
            checked_increment(self.summary.events, StructureOverflowKind::ElementIndex)?;
        self.summary.fingerprint = event.fold_fingerprint(self.summary.fingerprint);
        match event {
            ParseEvent::StartNode { kind, .. } => {
                self.summary.nodes =
                    checked_increment(self.summary.nodes, StructureOverflowKind::NodeIndex)?;
                if matches!(kind, SyntaxKind::Recovery | SyntaxKind::AmbiguousStatement) {
                    self.summary.recoveries = checked_increment(
                        self.summary.recoveries,
                        StructureOverflowKind::NodeIndex,
                    )?;
                }
            }
            ParseEvent::Token(_) => {
                self.summary.tokens =
                    checked_increment(self.summary.tokens, StructureOverflowKind::TokenIndex)?;
            }
            ParseEvent::Diagnostic(_) => {
                self.summary.diagnostics = checked_increment(
                    self.summary.diagnostics,
                    StructureOverflowKind::ElementIndex,
                )?;
            }
            ParseEvent::FinishNode { .. } => {}
        }
        sink.event(event).map_err(CssParseFailure::Structure)
    }
}

#[derive(Default)]
struct VecSink {
    events: Vec<ParseEvent>,
}

impl ParseEventSink for VecSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        self.events.push(event);
        Ok(())
    }
}

fn checked_increment(value: u32, kind: StructureOverflowKind) -> Result<u32, CssParseFailure> {
    value.checked_add(1).ok_or_else(|| {
        CssParseFailure::Structure(CssStructureTooLarge {
            kind,
            attempted: u64::from(value) + 1,
        })
    })
}

fn trim_trailing_trivia(tokens: &[SyntaxToken], start: usize, mut end: usize) -> usize {
    while end > start && tokens[end - 1].kind().is_trivia() {
        end -= 1;
    }
    end
}

fn next_significant(tokens: &[SyntaxToken], start: usize) -> Option<usize> {
    (start..tokens.len()).find(|index| !tokens[*index].kind().is_trivia())
}

fn has_indentation_structure(source: &CssSource, tokens: &[SyntaxToken]) -> bool {
    let mut previous_line_indent: Option<Vec<u8>> = None;
    let mut previous_significant = None;
    for (index, token) in tokens.iter().enumerate() {
        if token.kind().is_trivia() {
            continue;
        }
        let starts_line = previous_significant.is_none_or(|previous| {
            tokens[previous + 1..index]
                .iter()
                .any(|trivia| trivia.flags & TokenFlags::CONTAINS_NEWLINE != 0)
        });
        if starts_line {
            let indent = indent_prefix(source, tokens, index);
            if previous_line_indent.as_ref().is_some_and(|previous| {
                indent.starts_with(previous) && indent.len() > previous.len()
            }) {
                return true;
            }
            previous_line_indent = Some(indent);
        }
        previous_significant = Some(index);
    }
    false
}

fn indent_prefix(source: &CssSource, tokens: &[SyntaxToken], significant: usize) -> Vec<u8> {
    let mut prefix = Vec::new();
    let mut index = significant;
    while index > 0 && tokens[index - 1].kind().is_trivia() {
        index -= 1;
    }
    for token in &tokens[index..significant] {
        let text = source.token_text(*token).as_bytes();
        if token.flags & TokenFlags::CONTAINS_NEWLINE != 0 {
            prefix.clear();
            let after = text
                .iter()
                .rposition(|byte| matches!(byte, b'\n' | b'\r' | b'\x0c'))
                .map_or(0, |position| position + 1);
            prefix.extend_from_slice(&text[after..]);
        } else if token.kind() == TokenKind::Whitespace {
            prefix.extend_from_slice(text);
        }
    }
    prefix
}

fn prior_level(levels: &[Vec<u8>], indent: &[u8]) -> Option<usize> {
    levels.iter().rposition(|level| level.as_slice() == indent)
}

fn is_selector_lead(token: SyntaxToken, source: &CssSource) -> bool {
    match token.kind() {
        TokenKind::Ident | TokenKind::Hash | TokenKind::LeftBracket | TokenKind::Colon => true,
        TokenKind::Delim => matches!(
            source.token_text(token),
            "." | "#" | "&" | "*" | ">" | "+" | "~"
        ),
        _ => false,
    }
}

fn has_whitespace_between_significant(tokens: &[SyntaxToken]) -> bool {
    let first = tokens.iter().position(|token| !token.kind().is_trivia());
    let last = tokens.iter().rposition(|token| !token.kind().is_trivia());
    match (first, last) {
        (Some(first), Some(last)) if first < last => tokens[first + 1..last]
            .iter()
            .any(|token| token.kind() == TokenKind::Whitespace),
        _ => false,
    }
}

fn is_less_mixin_header(tokens: &[SyntaxToken], source: &CssSource) -> bool {
    let mut significant = tokens.iter().filter(|token| !token.kind().is_trivia());
    let Some(first) = significant.next() else {
        return false;
    };
    let Some(second) = significant.next() else {
        return false;
    };
    first.kind() == TokenKind::Delim
        && source.token_text(*first) == "."
        && second.kind() == TokenKind::Function
        && second.start == first.end
}

fn has_top_level_assignment(tokens: &[SyntaxToken], source: &CssSource) -> bool {
    let mut closing = Vec::new();
    for token in tokens {
        match token.kind() {
            TokenKind::Function | TokenKind::LeftParen => closing.push(TokenKind::RightParen),
            TokenKind::LeftBracket => closing.push(TokenKind::RightBracket),
            TokenKind::LeftBrace
            | TokenKind::ScssInterpolationStart
            | TokenKind::LessInterpolationStart
            | TokenKind::StylusInterpolationStart => closing.push(TokenKind::RightBrace),
            kind if closing.last().is_some_and(|expected| *expected == kind) => {
                closing.pop();
            }
            TokenKind::Delim if closing.is_empty() && source.token_text(*token) == "=" => {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn colon_starts_declaration(tokens: &[SyntaxToken], colon: usize) -> bool {
    let significant_before: Vec<_> = tokens[..colon]
        .iter()
        .filter(|token| !token.kind().is_trivia())
        .collect();
    if significant_before.len() != 1
        || !matches!(
            significant_before[0].kind(),
            TokenKind::Ident | TokenKind::ScssVariable | TokenKind::LessVariable
        )
    {
        return false;
    }
    let separator = tokens[colon];
    let Some(next) = tokens[colon + 1..]
        .iter()
        .find(|token| !token.kind().is_trivia())
    else {
        return true;
    };
    next.start != separator.end
}

fn continues_after(
    tokens: &[SyntaxToken],
    start: usize,
    newline: usize,
    source: &CssSource,
    dialect: CssDialect,
) -> bool {
    let Some(previous) = tokens[start..newline]
        .iter()
        .rev()
        .find(|token| !token.kind().is_trivia())
    else {
        return false;
    };
    (previous.kind() == TokenKind::Colon && dialect != CssDialect::Sass)
        || previous.kind() == TokenKind::Comma
        || (previous.kind() == TokenKind::Delim
            && matches!(
                source.token_text(*previous),
                "\\" | "+" | "-" | "*" | "/" | "%" | "=" | "<" | ">" | "&" | "|" | "?"
            ))
}
