use std::sync::Arc;

use smallvec::SmallVec;
use verter_span::Span;

use crate::diagnostic::{
    CssDiagnostic, CssDiagnosticKind, CssParseFailure, CssSeverity, CssSourceTooLarge,
    CssStructureTooLarge, RecoveryKind,
};
use crate::dialect::CssDialect;
use crate::event::{NodeFlags, ParseEvent, ParseEventSink, ParseSummary, SyntaxKind};
use crate::lexer::Lexer;
use crate::token::{
    css_identifier_eq_ignore_ascii_case, css_identifier_starts_with, SyntaxToken, TokenFlags,
    TokenKind,
};

const MAX_NESTING_DEPTH: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtRuleContext {
    RuleList,
    StyleBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclarationContext {
    Style,
    FontFaceDescriptor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSize {
    origin: u32,
    len: u32,
    end: u32,
}

impl SourceSize {
    pub fn new(origin: u32, source_len: usize) -> Result<Self, CssSourceTooLarge> {
        let source_len_u64 = u64::try_from(source_len).unwrap_or(u64::MAX);
        let len = u32::try_from(source_len).map_err(|_| CssSourceTooLarge {
            origin,
            source_len: source_len_u64,
        })?;
        let end = origin.checked_add(len).ok_or(CssSourceTooLarge {
            origin,
            source_len: source_len_u64,
        })?;
        Ok(Self { origin, len, end })
    }

    #[inline]
    pub const fn origin(self) -> u32 {
        self.origin
    }

    #[inline]
    pub const fn len(self) -> u32 {
        self.len
    }

    #[inline]
    pub const fn end(self) -> u32 {
        self.end
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }
}

#[derive(Debug, Clone)]
pub struct CssSource {
    text: Arc<str>,
    size: SourceSize,
}

impl CssSource {
    pub fn new(text: Arc<str>, origin: u32) -> Result<Self, CssSourceTooLarge> {
        let size = SourceSize::new(origin, text.len())?;
        Ok(Self { text, size })
    }

    #[inline]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[inline]
    pub const fn origin(&self) -> u32 {
        self.size.origin()
    }

    #[inline]
    pub const fn end(&self) -> u32 {
        self.size.end()
    }

    pub fn slice(&self, span: Span) -> &str {
        let start = span
            .start
            .checked_sub(self.origin())
            .expect("span starts before CssSource origin");
        let end = span
            .end
            .checked_sub(self.origin())
            .expect("span ends before CssSource origin");
        let start = usize::try_from(start).expect("u32 fits usize on supported targets");
        let end = usize::try_from(end).expect("u32 fits usize on supported targets");
        &self.text[start..end]
    }

    pub fn token_text(&self, token: SyntaxToken) -> &str {
        self.slice(Span::new(token.start, token.end))
    }

    /// Materialises a fresh `String` from a token stream — the ONE allocating
    /// source-text reconstruction primitive in this crate (`LosslessCst::reconstruct`
    /// is its only in-crate caller). Every call is counted for
    /// [`css_source_token_reconstructions`].
    pub fn slice_tokens(&self, tokens: impl IntoIterator<Item = SyntaxToken>) -> String {
        #[cfg(any(test, feature = "test-support"))]
        SOURCE_TOKEN_RECONSTRUCTIONS.with(|count| count.set(count.get() + 1));
        let mut output = String::with_capacity(self.text.len());
        for token in tokens {
            output.push_str(self.token_text(token));
        }
        output
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Per-thread count of [`CssSource::slice_tokens`] executions — the
    /// RECONSTRUCTION half of the "one shared-grammar parse, and no
    /// reconstruct-then-rescan" proof. `slice_tokens` is the sole allocating
    /// token-stream-to-`String` materialisation this crate offers, so a
    /// consumer that rebuilds CSS text (in order to re-scan or re-parse it)
    /// moves this counter even though the parse counter stays put — which is
    /// exactly the shape a parse-count-only probe cannot see. Compiled only
    /// under `test-support`, so production builds carry neither the TLS nor
    /// the increment.
    static SOURCE_TOKEN_RECONSTRUCTIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The number of [`CssSource::slice_tokens`] token-stream reconstructions
/// performed on the CALLING thread. Test/guard observability only — see the
/// thread-local's doc.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn css_source_token_reconstructions() -> u64 {
    SOURCE_TOKEN_RECONSTRUCTIONS.with(std::cell::Cell::get)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssEntryPoint {
    Stylesheet,
    SelectorList,
    ComponentValueList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssParseMode {
    Strict,
    Recover,
}

pub fn parse_with_sink(
    source: &CssSource,
    dialect: CssDialect,
    entry: CssEntryPoint,
    mode: CssParseMode,
    sink: &mut impl ParseEventSink,
) -> Result<ParseSummary, CssParseFailure> {
    Parser::new(source, dialect, entry, mode).parse(sink)
}

pub(crate) struct Parser<'a> {
    source: &'a CssSource,
    dialect: CssDialect,
    mode: CssParseMode,
    entry: CssEntryPoint,
    lexer: Lexer<'a>,
    lookahead: Option<SyntaxToken>,
    nesting: SmallVec<[TokenKind; 16]>,
    summary: ParseSummary,
    emit_whitespace_trivia: bool,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(
        source: &'a CssSource,
        dialect: CssDialect,
        entry: CssEntryPoint,
        mode: CssParseMode,
    ) -> Self {
        Self {
            source,
            dialect,
            mode,
            entry,
            lexer: Lexer::new(source, dialect),
            lookahead: None,
            nesting: SmallVec::new(),
            summary: ParseSummary::default(),
            emit_whitespace_trivia: true,
        }
    }

    pub(crate) fn parse(
        mut self,
        sink: &mut impl ParseEventSink,
    ) -> Result<ParseSummary, CssParseFailure> {
        self.emit_whitespace_trivia = sink.retain_whitespace_trivia();
        if self.entry == CssEntryPoint::Stylesheet
            && crate::layout::should_use_layout(self.source, self.dialect)
        {
            return crate::layout::parse_layout(self.source, self.dialect, self.mode, sink);
        }
        match self.entry {
            CssEntryPoint::Stylesheet => {
                self.start(sink, SyntaxKind::Stylesheet, self.source.origin())?;
                self.parse_rule_list(sink, false)?;
                self.finish(sink, SyntaxKind::Stylesheet, self.source.end())?;
            }
            CssEntryPoint::SelectorList => {
                self.parse_selector_list(sink, None, false)?;
            }
            CssEntryPoint::ComponentValueList => {
                self.start(sink, SyntaxKind::ComponentValueList, self.source.origin())?;
                self.parse_component_values(sink, None)?;
                self.finish(sink, SyntaxKind::ComponentValueList, self.source.end())?;
            }
        }
        while self.peek().is_some() {
            self.recover_current(sink, CssDiagnosticKind::UnexpectedClosingDelimiter)?;
        }
        Ok(self.summary)
    }

    fn emit(
        &mut self,
        sink: &mut impl ParseEventSink,
        event: ParseEvent,
    ) -> Result<(), CssParseFailure> {
        self.summary.events = self
            .summary
            .events
            .checked_add(1)
            .ok_or(CssStructureTooLarge {
                kind: crate::diagnostic::StructureOverflowKind::ElementIndex,
                attempted: u64::from(self.summary.events) + 1,
            })?;
        self.summary.fingerprint = event.fold_fingerprint(self.summary.fingerprint);
        match event {
            ParseEvent::StartNode { kind, .. } => {
                self.summary.nodes =
                    self.summary
                        .nodes
                        .checked_add(1)
                        .ok_or(CssStructureTooLarge {
                            kind: crate::diagnostic::StructureOverflowKind::NodeIndex,
                            attempted: u64::from(self.summary.nodes) + 1,
                        })?;
                if kind == SyntaxKind::Recovery {
                    self.summary.recoveries =
                        self.summary
                            .recoveries
                            .checked_add(1)
                            .ok_or(CssStructureTooLarge {
                                kind: crate::diagnostic::StructureOverflowKind::NodeIndex,
                                attempted: u64::from(self.summary.recoveries) + 1,
                            })?;
                }
            }
            ParseEvent::Token(_) => {
                self.summary.tokens =
                    self.summary
                        .tokens
                        .checked_add(1)
                        .ok_or(CssStructureTooLarge {
                            kind: crate::diagnostic::StructureOverflowKind::TokenIndex,
                            attempted: u64::from(self.summary.tokens) + 1,
                        })?;
            }
            ParseEvent::Diagnostic(_) => {
                self.summary.diagnostics =
                    self.summary
                        .diagnostics
                        .checked_add(1)
                        .ok_or(CssStructureTooLarge {
                            kind: crate::diagnostic::StructureOverflowKind::ElementIndex,
                            attempted: u64::from(self.summary.diagnostics) + 1,
                        })?;
            }
            ParseEvent::FinishNode { .. } => {}
        }
        sink.event(event)?;
        Ok(())
    }

    #[inline]
    fn start(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: SyntaxKind,
        start: u32,
    ) -> Result<(), CssParseFailure> {
        self.emit(
            sink,
            ParseEvent::StartNode {
                kind,
                flags: NodeFlags::default(),
                start,
            },
        )
    }

    #[inline]
    fn start_with_flags(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: SyntaxKind,
        flags: NodeFlags,
        start: u32,
    ) -> Result<(), CssParseFailure> {
        self.emit(sink, ParseEvent::StartNode { kind, flags, start })
    }

    #[inline]
    fn start_recovered(
        &mut self,
        sink: &mut impl ParseEventSink,
        start: u32,
    ) -> Result<(), CssParseFailure> {
        self.emit(
            sink,
            ParseEvent::StartNode {
                kind: SyntaxKind::Recovery,
                flags: NodeFlags::RECOVERED,
                start,
            },
        )
    }

    #[inline]
    fn finish(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: SyntaxKind,
        end: u32,
    ) -> Result<(), CssParseFailure> {
        self.emit(sink, ParseEvent::FinishNode { kind, end })
    }

    fn peek(&mut self) -> Option<SyntaxToken> {
        if self.lookahead.is_none() {
            self.lookahead = self.lexer.next();
        }
        self.lookahead
    }

    fn bump(
        &mut self,
        sink: &mut impl ParseEventSink,
    ) -> Result<Option<SyntaxToken>, CssParseFailure> {
        let token = if let Some(token) = self.lookahead.take() {
            Some(token)
        } else {
            self.lexer.next()
        };
        if let Some(token) = token {
            if !self.emit_whitespace_trivia && token.kind == TokenKind::Whitespace as u16 {
                return Ok(Some(token));
            }
            if let Some(kind) = lexical_diagnostic(token) {
                let diagnostic = CssDiagnostic {
                    kind,
                    severity: CssSeverity::Error,
                    span: Span::new(token.start, token.end),
                    recovery: if self.mode == CssParseMode::Strict {
                        RecoveryKind::None
                    } else {
                        RecoveryKind::AdvanceOneToken
                    },
                };
                if self.mode == CssParseMode::Strict {
                    return Err(CssParseFailure::Diagnostic(diagnostic));
                }
                self.emit(sink, ParseEvent::Diagnostic(diagnostic))?;
                self.start_recovered(sink, token.start)?;
                self.emit(sink, ParseEvent::Token(token))?;
                self.finish(sink, SyntaxKind::Recovery, token.end)?;
            } else {
                self.emit(sink, ParseEvent::Token(token))?;
            }
        }
        Ok(token)
    }

    fn current_position(&mut self) -> u32 {
        self.peek().map_or(self.source.end(), |token| token.start)
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
            recovery: if self.mode == CssParseMode::Strict {
                RecoveryKind::None
            } else {
                recovery
            },
        };
        if self.mode == CssParseMode::Strict {
            return Err(CssParseFailure::Diagnostic(diagnostic));
        }
        self.emit(sink, ParseEvent::Diagnostic(diagnostic))
    }

    fn recover_current(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: CssDiagnosticKind,
    ) -> Result<(), CssParseFailure> {
        let token = self.peek();
        let span = token.map_or_else(
            || Span::new(self.source.end(), self.source.end()),
            |token| Span::new(token.start, token.end),
        );
        self.diagnostic(sink, kind, span, RecoveryKind::AdvanceOneToken)?;
        self.start_recovered(sink, span.start)?;
        if token.is_some() {
            self.bump(sink)?;
        }
        self.finish(sink, SyntaxKind::Recovery, span.end)
    }

    fn recover_to_boundary(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: CssDiagnosticKind,
    ) -> Result<(), CssParseFailure> {
        let position = self.current_position();
        self.diagnostic(
            sink,
            kind,
            Span::new(position, position),
            RecoveryKind::AdvanceToBoundary,
        )?;
        self.start_recovered(sink, position)?;
        self.finish(sink, SyntaxKind::Recovery, position)
    }

    fn parse_rule_list(
        &mut self,
        sink: &mut impl ParseEventSink,
        stop_at_right_brace: bool,
    ) -> Result<(), CssParseFailure> {
        loop {
            let Some(token) = self.peek() else {
                return Ok(());
            };
            match token.kind() {
                kind if kind.is_trivia() || matches!(kind, TokenKind::Cdo | TokenKind::Cdc) => {
                    self.bump(sink)?;
                }
                TokenKind::RightBrace if stop_at_right_brace => return Ok(()),
                TokenKind::RightBrace => {
                    self.recover_current(sink, CssDiagnosticKind::UnexpectedClosingDelimiter)?;
                }
                TokenKind::AtKeyword if self.is_mixin_at_keyword(token) => {
                    self.parse_mixin_or_function(sink)?;
                }
                TokenKind::AtKeyword => self.parse_at_rule(sink, AtRuleContext::RuleList)?,
                TokenKind::ScssVariable if self.looks_like_declaration() => {
                    self.parse_declaration(sink, DeclarationContext::Style)?;
                }
                TokenKind::LessVariable if self.looks_like_declaration() => {
                    self.parse_declaration(sink, DeclarationContext::Style)?;
                }
                _ if self.looks_like_less_mixin() => self.parse_mixin_or_function(sink)?,
                _ => self.parse_qualified_rule(sink, false)?,
            }
        }
    }

    fn parse_qualified_rule(
        &mut self,
        sink: &mut impl ParseEventSink,
        declaration_recovery: bool,
    ) -> Result<(), CssParseFailure> {
        let start = self.current_position();
        self.start(sink, SyntaxKind::QualifiedRule, start)?;
        self.parse_selector_list(sink, Some(TokenKind::LeftBrace), declaration_recovery)?;
        if self.peek().map(SyntaxToken::kind) != Some(TokenKind::LeftBrace) {
            let position = self.current_position();
            self.recover_to_boundary(sink, CssDiagnosticKind::ExpectedRuleBlock)?;
            if self.peek().map(SyntaxToken::kind) == Some(TokenKind::RightBrace) {
                return self.finish(sink, SyntaxKind::QualifiedRule, position);
            }
            while let Some(token) = self.peek() {
                self.bump(sink)?;
                if token.kind() == TokenKind::Semicolon {
                    break;
                }
            }
            let end = self.current_position();
            return self.finish(sink, SyntaxKind::QualifiedRule, end);
        }
        let opener = self.peek().expect("qualified rule block has an opener");
        self.start(sink, SyntaxKind::RuleBlock, opener.start)?;
        self.bump(sink)?;
        self.enter_nesting(TokenKind::LeftBrace)?;
        let block_result = self.parse_block_items(sink, DeclarationContext::Style);
        self.leave_nesting();
        block_result?;
        let end = if self.peek().map(SyntaxToken::kind) == Some(TokenKind::RightBrace) {
            self.bump(sink)?
                .map_or(self.source.end(), |token| token.end)
        } else {
            let position = self.source.end();
            self.diagnostic(
                sink,
                CssDiagnosticKind::UnterminatedBlock,
                Span::new(position, position),
                RecoveryKind::CloseAtEndOfInput,
            )?;
            position
        };
        self.finish(sink, SyntaxKind::RuleBlock, end)?;
        self.finish(sink, SyntaxKind::QualifiedRule, end)
    }

    fn parse_block_items(
        &mut self,
        sink: &mut impl ParseEventSink,
        declaration_context: DeclarationContext,
    ) -> Result<(), CssParseFailure> {
        loop {
            let Some(token) = self.peek() else {
                return Ok(());
            };
            match token.kind() {
                TokenKind::RightBrace => return Ok(()),
                kind if kind.is_trivia() || kind == TokenKind::Semicolon => {
                    self.bump(sink)?;
                }
                TokenKind::AtKeyword if self.is_mixin_at_keyword(token) => {
                    self.parse_mixin_or_function(sink)?;
                }
                TokenKind::AtKeyword => self.parse_at_rule(sink, AtRuleContext::StyleBlock)?,
                _ if self.looks_like_declaration() => {
                    self.parse_declaration(sink, declaration_context)?;
                }
                _ if self.looks_like_less_mixin() => self.parse_mixin_or_function(sink)?,
                _ => self.parse_qualified_rule(sink, true)?,
            }
        }
    }

    fn is_mixin_at_keyword(&self, token: SyntaxToken) -> bool {
        if !matches!(self.dialect, CssDialect::Scss | CssDialect::Sass) {
            return false;
        }
        let name = self.source.token_text(token).trim_start_matches('@');
        identifier_is_any(name, &["mixin", "function", "include", "extend"])
    }

    fn looks_like_less_mixin(&mut self) -> bool {
        if self.dialect != CssDialect::Less {
            return false;
        }
        let Some(first) = self.peek() else {
            return false;
        };
        if first.kind() != TokenKind::Delim || self.source.token_text(first) != "." {
            return false;
        }
        self.lexer
            .clone()
            .find(|token| !token.kind().is_trivia())
            .is_some_and(|token| token.kind() == TokenKind::Function && token.start == first.end)
    }

    fn parse_mixin_or_function(
        &mut self,
        sink: &mut impl ParseEventSink,
    ) -> Result<(), CssParseFailure> {
        let start = self.current_position();
        self.start(sink, SyntaxKind::MixinOrFunctionHeader, start)?;
        self.start(sink, SyntaxKind::ComponentValueList, start)?;
        while let Some(token) = self.peek() {
            match token.kind() {
                TokenKind::Semicolon | TokenKind::LeftBrace | TokenKind::RightBrace => break,
                kind if kind.is_opening_delimiter() => self.parse_component_block(sink)?,
                kind if kind.is_closing_delimiter() => {
                    self.recover_current(sink, CssDiagnosticKind::UnexpectedClosingDelimiter)?;
                }
                _ => {
                    self.bump(sink)?;
                }
            }
        }
        let header_end = self.current_position();
        self.finish(sink, SyntaxKind::ComponentValueList, header_end)?;

        if self.peek().map(SyntaxToken::kind) == Some(TokenKind::Semicolon) {
            let end = self.bump(sink)?.map_or(header_end, |token| token.end);
            return self.finish(sink, SyntaxKind::MixinOrFunctionHeader, end);
        }
        if self.peek().map(SyntaxToken::kind) != Some(TokenKind::LeftBrace) {
            return self.finish(sink, SyntaxKind::MixinOrFunctionHeader, header_end);
        }

        let opener = self.peek().expect("mixin block has an opener");
        self.start(sink, SyntaxKind::RuleBlock, opener.start)?;
        self.bump(sink)?;
        self.enter_nesting(TokenKind::LeftBrace)?;
        let block_result = self.parse_block_items(sink, DeclarationContext::Style);
        self.leave_nesting();
        block_result?;
        let end = if self.peek().map(SyntaxToken::kind) == Some(TokenKind::RightBrace) {
            self.bump(sink)?
                .map_or(self.source.end(), |token| token.end)
        } else {
            let end = self.source.end();
            self.diagnostic(
                sink,
                CssDiagnosticKind::UnterminatedBlock,
                Span::new(end, end),
                RecoveryKind::CloseAtEndOfInput,
            )?;
            end
        };
        self.finish(sink, SyntaxKind::RuleBlock, end)?;
        self.finish(sink, SyntaxKind::MixinOrFunctionHeader, end)
    }

    fn looks_like_declaration(&mut self) -> bool {
        let Some(first) = self.peek() else {
            return false;
        };
        if !matches!(
            first.kind(),
            TokenKind::Ident | TokenKind::ScssVariable | TokenKind::LessVariable
        ) {
            return false;
        }
        let custom_property_name = first.kind() == TokenKind::Ident
            && css_identifier_starts_with(self.source.token_text(first), "--");
        let mut clone = self.lexer.clone();
        let Some(colon) = clone.find(|token| !token.kind().is_trivia()) else {
            return false;
        };
        if colon.kind() != TokenKind::Colon {
            return false;
        }
        if custom_property_name || first.kind() != TokenKind::Ident {
            return true;
        }

        declaration_value_shape_admits(self.source, clone)
    }
}

/// The value-shape half of `looks_like_declaration`: given every token AFTER a declaration
/// colon, does the value read as a declaration's value rather than a rule's body?
///
/// A value may contain at most one top-level `{ ... }` block, and only as its SOLE value (an
/// optional trailing `!important` excepted) — `color: { … }` is a declaration, `foo: bar { … }`
/// is a qualified rule whose prelude happens to contain a colon. Shared so the indentation-aware
/// layout parser reaches the identical verdict instead of approximating it: `foo: bar { … }`
/// classified as a declaration there and as a rule here, for exactly that reason.
pub(crate) fn declaration_value_shape_admits(
    source: &CssSource,
    tokens: impl Iterator<Item = SyntaxToken>,
) -> bool {
    let mut depth = 0u32;
    let mut top_level_brace_blocks = 0u32;
    let mut other_top_level_values = 0u32;
    let mut penultimate_top_level = None;
    let mut last_top_level = None;
    {
        for token in tokens {
            match token.kind() {
                TokenKind::Function => {
                    if depth == 0 {
                        other_top_level_values = other_top_level_values.saturating_add(1);
                        penultimate_top_level = last_top_level;
                        last_top_level = Some(token);
                    }
                    depth += 1;
                }
                TokenKind::LeftParen | TokenKind::LeftBracket => {
                    if depth == 0 {
                        other_top_level_values = other_top_level_values.saturating_add(1);
                        penultimate_top_level = last_top_level;
                        last_top_level = Some(token);
                    }
                    depth += 1;
                }
                TokenKind::LeftBrace => {
                    if depth == 0 {
                        top_level_brace_blocks = top_level_brace_blocks.saturating_add(1);
                        penultimate_top_level = last_top_level;
                        last_top_level = Some(token);
                    }
                    depth += 1;
                }
                TokenKind::RightParen | TokenKind::RightBracket | TokenKind::RightBrace
                    if depth > 0 =>
                {
                    depth -= 1;
                }
                TokenKind::Semicolon | TokenKind::RightBrace if depth == 0 => break,
                kind if depth == 0 && !kind.is_trivia() => {
                    other_top_level_values = other_top_level_values.saturating_add(1);
                    penultimate_top_level = last_top_level;
                    last_top_level = Some(token);
                }
                _ => {}
            }
        }
    }
    if top_level_brace_blocks == 0 {
        return true;
    }
    let terminal_important = last_top_level.is_some_and(|token| {
        token.kind() == TokenKind::Ident
            && css_identifier_eq_ignore_ascii_case(source.token_text(token), "important")
    }) && penultimate_top_level
        .is_some_and(|token| token.kind() == TokenKind::Delim && source.token_text(token) == "!");
    let remaining_other_values =
        other_top_level_values.saturating_sub(if terminal_important { 2 } else { 0 });
    top_level_brace_blocks == 1 && remaining_other_values == 0
}

impl<'a> Parser<'a> {
    fn parse_declaration(
        &mut self,
        sink: &mut impl ParseEventSink,
        context: DeclarationContext,
    ) -> Result<(), CssParseFailure> {
        let property = self.peek().expect("declaration has a first token");
        let custom = css_identifier_starts_with(self.source.token_text(property), "--");
        let unicode_range_value = context == DeclarationContext::FontFaceDescriptor
            && property.kind() == TokenKind::Ident
            && css_identifier_eq_ignore_ascii_case(
                self.source.token_text(property),
                "unicode-range",
            );
        let kind = if custom {
            SyntaxKind::CustomPropertyDeclaration
        } else {
            SyntaxKind::Declaration
        };
        self.start(sink, kind, property.start)?;
        self.bump(sink)?;
        while self.peek().is_some_and(|token| token.kind().is_trivia()) {
            self.bump(sink)?;
        }
        let Some(colon) = self.peek() else {
            let position = self.source.end();
            self.diagnostic(
                sink,
                CssDiagnosticKind::ExpectedDeclarationColon,
                Span::new(position, position),
                RecoveryKind::AdvanceToBoundary,
            )?;
            return self.finish(sink, kind, position);
        };
        if colon.kind() != TokenKind::Colon {
            self.diagnostic(
                sink,
                CssDiagnosticKind::ExpectedDeclarationColon,
                Span::new(colon.start, colon.start),
                RecoveryKind::AdvanceToBoundary,
            )?;
            let end = self.current_position();
            return self.finish(sink, kind, end);
        }
        self.bump(sink)?;
        verter_debug_assert!(
            self.lookahead.is_none(),
            "unicode-range mode must change before the value is tokenized"
        );

        let previous_unicode_range_mode =
            self.lexer.set_unicode_ranges_allowed(unicode_range_value);
        let value_result = (|| {
            let value_start = self.current_position();
            self.start(sink, SyntaxKind::ComponentValueList, value_start)?;
            self.parse_component_values_until_declaration_boundary(sink)?;
            let value_end = self.current_position();
            self.finish(sink, SyntaxKind::ComponentValueList, value_end)?;
            Ok::<u32, CssParseFailure>(value_end)
        })();
        self.lexer
            .set_unicode_ranges_allowed(previous_unicode_range_mode);
        let value_end = value_result?;
        let end = if self.peek().map(SyntaxToken::kind) == Some(TokenKind::Semicolon) {
            self.bump(sink)?.map_or(value_end, |token| token.end)
        } else {
            value_end
        };
        self.finish(sink, kind, end)
    }

    fn parse_component_values_until_declaration_boundary(
        &mut self,
        sink: &mut impl ParseEventSink,
    ) -> Result<(), CssParseFailure> {
        loop {
            let Some(token) = self.peek() else {
                return Ok(());
            };
            match token.kind() {
                TokenKind::Semicolon | TokenKind::RightBrace => return Ok(()),
                kind if kind.is_opening_delimiter() => self.parse_component_block(sink)?,
                kind if kind.is_closing_delimiter() => {
                    self.recover_to_boundary(sink, CssDiagnosticKind::MismatchedDelimiter)?;
                    return Ok(());
                }
                _ => {
                    self.bump(sink)?;
                }
            }
        }
    }

    fn parse_at_rule(
        &mut self,
        sink: &mut impl ParseEventSink,
        context: AtRuleContext,
    ) -> Result<(), CssParseFailure> {
        let at_keyword = self.peek().expect("at-rule begins with an at-keyword");
        let raw_name = &self.source.token_text(at_keyword)[1..];
        let kind = if matches!(self.dialect, CssDialect::Scss | CssDialect::Sass)
            && identifier_is_any(raw_name, &["if", "else", "for", "each", "while", "return"])
        {
            SyntaxKind::ControlDirective
        } else {
            classify_at_rule(raw_name)
        };
        let descriptor_context = if identifier_is_any(raw_name, &["font-face"]) {
            DeclarationContext::FontFaceDescriptor
        } else {
            DeclarationContext::Style
        };
        self.start(sink, kind, at_keyword.start)?;
        self.bump(sink)?;
        let prelude_start = self.current_position();
        self.start(sink, SyntaxKind::AtRulePrelude, prelude_start)?;
        while let Some(token) = self.peek() {
            match token.kind() {
                TokenKind::Semicolon | TokenKind::LeftBrace => break,
                opening if opening.is_opening_delimiter() => self.parse_component_block(sink)?,
                TokenKind::RightBrace => {
                    self.recover_to_boundary(sink, CssDiagnosticKind::ExpectedAtRuleTerminator)?;
                    break;
                }
                closing if closing.is_closing_delimiter() => {
                    self.recover_current(sink, CssDiagnosticKind::UnexpectedClosingDelimiter)?;
                }
                _ => {
                    self.bump(sink)?;
                }
            }
        }
        let prelude_end = self.current_position();
        self.finish(sink, SyntaxKind::AtRulePrelude, prelude_end)?;
        if self.peek().map(SyntaxToken::kind) == Some(TokenKind::Semicolon) {
            let end = self
                .bump(sink)?
                .map_or(self.source.end(), |token| token.end);
            return self.finish(sink, kind, end);
        }
        if self.peek().map(SyntaxToken::kind) != Some(TokenKind::LeftBrace) {
            let end = self.current_position();
            return self.finish(sink, kind, end);
        }
        let opener = self.peek().expect("at-rule block has an opener");
        self.start(sink, SyntaxKind::AtRuleBlock, opener.start)?;
        self.bump(sink)?;
        self.enter_nesting(TokenKind::LeftBrace)?;
        let block_result = match (kind, context) {
            (SyntaxKind::GroupAtRule, AtRuleContext::StyleBlock) => {
                self.parse_block_items(sink, DeclarationContext::Style)
            }
            (SyntaxKind::GroupAtRule | SyntaxKind::KeyframesAtRule, _) => {
                self.parse_rule_list(sink, true)
            }
            (SyntaxKind::UnknownAtRule, _) => {
                self.parse_component_values(sink, Some(TokenKind::RightBrace))
            }
            _ => self.parse_block_items(sink, descriptor_context),
        };
        self.leave_nesting();
        block_result?;
        let end = if self.peek().map(SyntaxToken::kind) == Some(TokenKind::RightBrace) {
            self.bump(sink)?
                .map_or(self.source.end(), |token| token.end)
        } else {
            let end = self.source.end();
            self.diagnostic(
                sink,
                CssDiagnosticKind::UnterminatedBlock,
                Span::new(end, end),
                RecoveryKind::CloseAtEndOfInput,
            )?;
            end
        };
        self.finish(sink, SyntaxKind::AtRuleBlock, end)?;
        self.finish(sink, kind, end)
    }

    fn parse_component_values(
        &mut self,
        sink: &mut impl ParseEventSink,
        stop: Option<TokenKind>,
    ) -> Result<(), CssParseFailure> {
        loop {
            let Some(token) = self.peek() else {
                return Ok(());
            };
            if stop == Some(token.kind()) {
                return Ok(());
            }
            if token.kind().is_opening_delimiter() {
                self.parse_component_block(sink)?;
            } else if token.kind().is_closing_delimiter() {
                if stop.is_some() && token.kind() == TokenKind::RightBrace {
                    self.recover_to_boundary(sink, CssDiagnosticKind::MismatchedDelimiter)?;
                    return Ok(());
                }
                let kind = if stop.is_some() {
                    CssDiagnosticKind::MismatchedDelimiter
                } else {
                    CssDiagnosticKind::UnexpectedClosingDelimiter
                };
                self.recover_current(sink, kind)?;
            } else {
                self.bump(sink)?;
            }
        }
    }

    fn parse_component_block(
        &mut self,
        sink: &mut impl ParseEventSink,
    ) -> Result<(), CssParseFailure> {
        let opener = self.peek().expect("component block has opener");
        let open_kind = opener.kind();
        let (node_kind, expected) = match open_kind {
            TokenKind::Function => (SyntaxKind::Function, TokenKind::RightParen),
            TokenKind::LeftParen => (SyntaxKind::ComponentValueBlock, TokenKind::RightParen),
            TokenKind::LeftBracket => (SyntaxKind::ComponentValueBlock, TokenKind::RightBracket),
            TokenKind::LeftBrace
            | TokenKind::ScssInterpolationStart
            | TokenKind::LessInterpolationStart
            | TokenKind::StylusInterpolationStart => {
                let node = if matches!(
                    open_kind,
                    TokenKind::ScssInterpolationStart
                        | TokenKind::LessInterpolationStart
                        | TokenKind::StylusInterpolationStart
                ) {
                    SyntaxKind::Interpolation
                } else {
                    SyntaxKind::ComponentValueBlock
                };
                (node, TokenKind::RightBrace)
            }
            _ => return Ok(()),
        };
        let flags = if matches!(
            open_kind,
            TokenKind::ScssInterpolationStart
                | TokenKind::LessInterpolationStart
                | TokenKind::StylusInterpolationStart
        ) {
            NodeFlags::DIALECT_EXTENSION
        } else {
            NodeFlags::default()
        };
        self.start_with_flags(sink, node_kind, flags, opener.start)?;
        self.bump(sink)?;
        self.enter_nesting(open_kind)?;
        let values_result = self.parse_component_values(sink, Some(expected));
        self.leave_nesting();
        values_result?;
        let end = if self.peek().map(SyntaxToken::kind) == Some(expected) {
            self.bump(sink)?
                .map_or(self.source.end(), |token| token.end)
        } else {
            let position = self.current_position();
            self.diagnostic(
                sink,
                CssDiagnosticKind::UnterminatedBlock,
                Span::new(position, position),
                RecoveryKind::CloseAtEndOfInput,
            )?;
            position
        };
        self.finish(sink, node_kind, end)
    }

    fn parse_selector_list(
        &mut self,
        sink: &mut impl ParseEventSink,
        stop: Option<TokenKind>,
        stop_at_semicolon: bool,
    ) -> Result<(), CssParseFailure> {
        let start = self.current_position();
        self.start(sink, SyntaxKind::SelectorList, start)?;
        let mut selector_open = false;
        let mut compound_open = false;
        let mut at_type_position = true;
        while let Some(token) = self.peek() {
            if stop == Some(token.kind()) {
                break;
            }
            if stop_at_semicolon && token.kind() == TokenKind::Semicolon {
                break;
            }
            if token.kind() == TokenKind::RightBrace
                && (stop == Some(TokenKind::LeftBrace)
                    || (stop == Some(TokenKind::RightParen) && self.has_rule_block_owner()))
            {
                break;
            }

            if token.kind().is_trivia() {
                if !selector_open {
                    self.consume_selector_trivia(sink, false)?;
                    continue;
                }
                let separates = self.selector_trivia_contains_whitespace();
                if !separates {
                    self.consume_selector_trivia(sink, false)?;
                    continue;
                }
                let had_compound = compound_open;
                if compound_open {
                    self.finish(sink, SyntaxKind::CompoundSelector, token.start)?;
                    compound_open = false;
                }
                let next = self.next_significant_after_current();
                let descendant = had_compound
                    && next.is_some_and(|next| {
                        stop != Some(next.kind())
                            && next.kind() != TokenKind::Comma
                            && !next.kind().is_closing_delimiter()
                            && !self.next_significant_is_explicit_combinator()
                    });
                self.consume_selector_trivia(sink, descendant)?;
                at_type_position = true;
                continue;
            }

            if !selector_open {
                self.start(sink, SyntaxKind::Selector, token.start)?;
                selector_open = true;
                at_type_position = true;
            }
            let namespace_tokens = self.namespace_token_count();
            if !compound_open
                && token.kind() != TokenKind::Comma
                && !token.kind().is_closing_delimiter()
                && (namespace_tokens != 0 || !self.current_is_explicit_combinator())
            {
                self.start(sink, SyntaxKind::CompoundSelector, token.start)?;
                compound_open = true;
                at_type_position = true;
            }

            if at_type_position && namespace_tokens != 0 {
                self.parse_namespace_selector(sink)?;
                at_type_position = false;
                continue;
            }

            match token.kind() {
                TokenKind::Ident if at_type_position => {
                    self.parse_selector_name_component(sink, SyntaxKind::TypeSelector, false)?;
                    at_type_position = false;
                }
                TokenKind::Delim if at_type_position && self.source.token_text(token) == "*" => {
                    self.parse_selector_name_component(sink, SyntaxKind::TypeSelector, false)?;
                    at_type_position = false;
                }
                TokenKind::Delim if self.source.token_text(token) == "." => {
                    let mut probe = self.lexer.clone();
                    if next_adjacent_after_comments(&mut probe).is_some_and(|(next, _)| {
                        matches!(
                            next.kind(),
                            TokenKind::Ident
                                | TokenKind::ScssInterpolationStart
                                | TokenKind::LessInterpolationStart
                                | TokenKind::StylusInterpolationStart
                        )
                    }) {
                        self.parse_selector_name_component(sink, SyntaxKind::ClassSelector, true)?;
                    } else {
                        self.bump(sink)?;
                    }
                    at_type_position = false;
                }
                TokenKind::Hash if token.flags & TokenFlags::ID_HASH != 0 => {
                    self.parse_selector_name_component(sink, SyntaxKind::IdSelector, false)?;
                    at_type_position = false;
                }
                TokenKind::Comma => {
                    if compound_open {
                        self.finish(sink, SyntaxKind::CompoundSelector, token.start)?;
                        compound_open = false;
                    }
                    if selector_open {
                        self.finish(sink, SyntaxKind::Selector, token.start)?;
                        selector_open = false;
                    }
                    self.bump(sink)?;
                    at_type_position = true;
                }
                TokenKind::Delim if matches!(self.source.token_text(token), ">" | "+" | "~") => {
                    if compound_open {
                        self.finish(sink, SyntaxKind::CompoundSelector, token.start)?;
                        compound_open = false;
                    }
                    self.start(sink, SyntaxKind::Combinator, token.start)?;
                    let end = self.bump(sink)?.map_or(token.end, |value| value.end);
                    self.finish(sink, SyntaxKind::Combinator, end)?;
                    at_type_position = true;
                }
                TokenKind::Delim if self.source.token_text(token) == "|" => {
                    let column_tokens = self.current_column_combinator_token_count();
                    if column_tokens != 0 {
                        if compound_open {
                            self.finish(sink, SyntaxKind::CompoundSelector, token.start)?;
                            compound_open = false;
                        }
                        self.start(sink, SyntaxKind::Combinator, token.start)?;
                        let mut end = token.end;
                        for _ in 0..column_tokens {
                            end = self.bump(sink)?.map_or(end, |value| value.end);
                        }
                        self.finish(sink, SyntaxKind::Combinator, end)?;
                        at_type_position = true;
                    } else {
                        self.bump(sink)?;
                        at_type_position = false;
                    }
                }
                TokenKind::Delim if self.source.token_text(token) == "&" => {
                    self.start(sink, SyntaxKind::NestingSelector, token.start)?;
                    let end = self.bump(sink)?.map_or(token.end, |value| value.end);
                    self.finish(sink, SyntaxKind::NestingSelector, end)?;
                    at_type_position = false;
                }
                TokenKind::LeftBracket => {
                    self.parse_selector_attribute(sink)?;
                    at_type_position = false;
                }
                TokenKind::Colon => {
                    self.parse_selector_pseudo(sink)?;
                    at_type_position = false;
                }
                kind if kind.is_opening_delimiter() => {
                    self.parse_component_block(sink)?;
                    at_type_position = false;
                }
                kind if kind.is_closing_delimiter() => {
                    self.recover_current(sink, CssDiagnosticKind::MismatchedDelimiter)?;
                }
                _ => {
                    self.bump(sink)?;
                    at_type_position = false;
                }
            }
        }
        let end = self.current_position();
        if compound_open {
            self.finish(sink, SyntaxKind::CompoundSelector, end)?;
        }
        if selector_open {
            self.finish(sink, SyntaxKind::Selector, end)?;
        }
        self.finish(sink, SyntaxKind::SelectorList, end)
    }

    fn parse_selector_name_component(
        &mut self,
        sink: &mut impl ParseEventSink,
        kind: SyntaxKind,
        has_prefix: bool,
    ) -> Result<(), CssParseFailure> {
        let start = self.current_position();
        self.start(sink, kind, start)?;
        self.bump(sink)?;
        if has_prefix {
            while self
                .peek()
                .is_some_and(|token| token.kind() == TokenKind::Comment)
            {
                self.bump(sink)?;
            }
        }
        while let Some(token) = self.peek() {
            match token.kind() {
                TokenKind::Ident if token.start == self.current_position() => {
                    self.bump(sink)?;
                }
                TokenKind::ScssInterpolationStart
                | TokenKind::LessInterpolationStart
                | TokenKind::StylusInterpolationStart
                    if token.start == self.current_position() =>
                {
                    self.parse_component_block(sink)?;
                }
                _ => break,
            }
        }
        let end = self.current_position();
        self.finish(sink, kind, end)
    }

    fn parse_selector_attribute(
        &mut self,
        sink: &mut impl ParseEventSink,
    ) -> Result<(), CssParseFailure> {
        let start = self.current_position();
        self.start(sink, SyntaxKind::AttributeSelector, start)?;
        self.bump(sink)?;
        self.enter_nesting(TokenKind::LeftBracket)?;
        while self.peek().is_some_and(|token| token.kind().is_trivia()) {
            self.bump(sink)?;
        }
        if self.namespace_token_count() != 0 {
            self.parse_namespace_selector(sink)?;
        }
        let values_result = self.parse_component_values(sink, Some(TokenKind::RightBracket));
        self.leave_nesting();
        values_result?;
        let end = if self.peek().map(SyntaxToken::kind) == Some(TokenKind::RightBracket) {
            self.bump(sink)?
                .map_or(self.source.end(), |token| token.end)
        } else {
            let position = self.current_position();
            self.diagnostic(
                sink,
                CssDiagnosticKind::UnterminatedBlock,
                Span::new(position, position),
                RecoveryKind::CloseAtEndOfInput,
            )?;
            position
        };
        self.finish(sink, SyntaxKind::AttributeSelector, end)
    }

    fn selector_trivia_contains_whitespace(&self) -> bool {
        self.lookahead
            .is_some_and(|token| token.kind() == TokenKind::Whitespace)
            || self
                .lexer
                .clone()
                .take_while(|token| token.kind().is_trivia())
                .any(|token| token.kind() == TokenKind::Whitespace)
    }

    fn next_significant_after_current(&self) -> Option<SyntaxToken> {
        self.lexer.clone().find(|token| !token.kind().is_trivia())
    }

    fn consume_selector_trivia(
        &mut self,
        sink: &mut impl ParseEventSink,
        descendant: bool,
    ) -> Result<(), CssParseFailure> {
        let start = self.peek().expect("selector trivia is present").start;
        if descendant {
            self.start(sink, SyntaxKind::Combinator, start)?;
        }
        while self.peek().is_some_and(|token| token.kind().is_trivia()) {
            self.bump(sink)?;
        }
        if descendant {
            let end = self.current_position();
            self.finish(sink, SyntaxKind::Combinator, end)?;
        }
        Ok(())
    }

    fn next_significant_is_explicit_combinator(&self) -> bool {
        let mut probe = self.lexer.clone();
        let Some(token) = probe.find(|token| !token.kind().is_trivia()) else {
            return false;
        };
        is_non_column_combinator(token, self.source)
            || (is_pipe(token, self.source)
                && next_adjacent_after_comments(&mut probe)
                    .is_some_and(|(next, _)| is_pipe(next, self.source)))
    }

    fn current_is_explicit_combinator(&self) -> bool {
        let Some(token) = self.lookahead else {
            return false;
        };
        is_non_column_combinator(token, self.source)
            || self.current_column_combinator_token_count() != 0
    }

    fn current_column_combinator_token_count(&self) -> usize {
        let Some(current) = self.lookahead else {
            return 0;
        };
        if !is_pipe(current, self.source) {
            return 0;
        }
        let mut probe = self.lexer.clone();
        next_adjacent_after_comments(&mut probe).map_or(0, |(next, count)| {
            if is_pipe(next, self.source) {
                1 + count
            } else {
                0
            }
        })
    }

    fn namespace_token_count(&self) -> usize {
        let Some(current) = self.lookahead else {
            return 0;
        };
        let mut probe = self.lexer.clone();
        if is_namespace_name_side(current, self.source) {
            let Some((pipe, pipe_count)) = next_adjacent_after_comments(&mut probe) else {
                return 0;
            };
            let Some((name, name_count)) = next_adjacent_after_comments(&mut probe) else {
                return 0;
            };
            if is_pipe(pipe, self.source) && is_namespace_name_side(name, self.source) {
                return 1 + pipe_count + name_count;
            }
        } else if is_pipe(current, self.source) {
            if let Some((name, name_count)) = next_adjacent_after_comments(&mut probe) {
                if is_namespace_name_side(name, self.source) {
                    return 1 + name_count;
                }
            }
        }
        0
    }

    fn parse_namespace_selector(
        &mut self,
        sink: &mut impl ParseEventSink,
    ) -> Result<(), CssParseFailure> {
        let count = self.namespace_token_count();
        verter_debug_assert!(count >= 2);
        let start = self.current_position();
        self.start(sink, SyntaxKind::NamespaceSelector, start)?;
        let mut end = start;
        for _ in 0..count {
            end = self.bump(sink)?.expect("namespace token exists").end;
        }
        self.finish(sink, SyntaxKind::NamespaceSelector, end)
    }

    fn parse_selector_pseudo(
        &mut self,
        sink: &mut impl ParseEventSink,
    ) -> Result<(), CssParseFailure> {
        let start = self.current_position();
        let mut probe = self.lexer.clone();
        let next = next_adjacent_after_comments(&mut probe);
        let pseudo_element = next.map(|(token, _)| token.kind()) == Some(TokenKind::Colon);
        let (name_token, token_count) = if pseudo_element {
            let colon_count = next.map_or(0, |(_, count)| count);
            if let Some((name, name_count)) = next_adjacent_after_comments(&mut probe)
                .filter(|(token, _)| matches!(token.kind(), TokenKind::Ident | TokenKind::Function))
            {
                (Some(name), colon_count + name_count)
            } else {
                (None, colon_count)
            }
        } else {
            next.filter(|(token, _)| matches!(token.kind(), TokenKind::Ident | TokenKind::Function))
                .map_or((None, 0), |(token, count)| (Some(token), count))
        };
        let name = name_token.map_or("", |token| token_name(self.source, token));
        let functional = name_token.map(SyntaxToken::kind) == Some(TokenKind::Function);
        let kind = if functional && is_selector_list_pseudo(name, pseudo_element) {
            SyntaxKind::PseudoSelectorList
        } else if pseudo_element {
            SyntaxKind::PseudoElement
        } else if functional && is_nth_pseudo(name) {
            SyntaxKind::NthSelector
        } else if functional {
            SyntaxKind::UnknownPseudoFunction
        } else {
            SyntaxKind::PseudoClass
        };
        self.start(sink, kind, start)?;
        self.bump(sink)?;
        for _ in 0..token_count {
            self.bump(sink)?;
        }
        let Some(name_token) = name_token else {
            let end = self.current_position();
            return self.finish(sink, kind, end);
        };
        if name_token.kind() != TokenKind::Function {
            return self.finish(sink, kind, name_token.end);
        }
        self.enter_nesting(TokenKind::Function)?;
        let arguments_result = if kind == SyntaxKind::PseudoSelectorList {
            self.parse_selector_list(sink, Some(TokenKind::RightParen), false)
        } else if kind == SyntaxKind::NthSelector {
            self.parse_nth_arguments(sink)
        } else {
            self.parse_component_values(sink, Some(TokenKind::RightParen))
        };
        self.leave_nesting();
        arguments_result?;
        let end = if self.peek().map(SyntaxToken::kind) == Some(TokenKind::RightParen) {
            self.bump(sink)?
                .map_or(self.source.end(), |token| token.end)
        } else if self.peek().map(SyntaxToken::kind) == Some(TokenKind::RightBrace)
            && self.has_rule_block_owner()
        {
            let position = self.current_position();
            self.recover_to_boundary(sink, CssDiagnosticKind::UnterminatedBlock)?;
            position
        } else {
            let position = self.current_position();
            self.diagnostic(
                sink,
                CssDiagnosticKind::UnterminatedBlock,
                Span::new(position, position),
                RecoveryKind::CloseAtEndOfInput,
            )?;
            position
        };
        self.finish(sink, kind, end)
    }

    fn has_rule_block_owner(&self) -> bool {
        self.nesting.contains(&TokenKind::LeftBrace)
    }

    fn enter_nesting(&mut self, opener: TokenKind) -> Result<(), CssParseFailure> {
        if self.nesting.len() >= MAX_NESTING_DEPTH {
            return Err(CssStructureTooLarge {
                kind: crate::diagnostic::StructureOverflowKind::NestingDepth,
                attempted: u64::try_from(self.nesting.len() + 1).unwrap_or(u64::MAX),
            }
            .into());
        }
        self.nesting.push(opener);
        Ok(())
    }

    fn leave_nesting(&mut self) {
        self.nesting.pop();
    }

    fn parse_nth_arguments(
        &mut self,
        sink: &mut impl ParseEventSink,
    ) -> Result<(), CssParseFailure> {
        loop {
            let Some(token) = self.peek() else {
                return Ok(());
            };
            if token.kind() == TokenKind::RightParen {
                return Ok(());
            }
            if token.kind() == TokenKind::Ident
                && css_identifier_eq_ignore_ascii_case(self.source.token_text(token), "of")
            {
                self.bump(sink)?;
                let selector_start = self.current_position();
                self.start(sink, SyntaxKind::NthOfSelectorList, selector_start)?;
                self.parse_selector_list(sink, Some(TokenKind::RightParen), false)?;
                let selector_end = self.current_position();
                self.finish(sink, SyntaxKind::NthOfSelectorList, selector_end)?;
                return Ok(());
            }
            if token.kind().is_opening_delimiter() {
                self.parse_component_block(sink)?;
            } else if token.kind().is_closing_delimiter() {
                return Ok(());
            } else {
                self.bump(sink)?;
            }
        }
    }
}

fn next_adjacent_after_comments(probe: &mut Lexer<'_>) -> Option<(SyntaxToken, usize)> {
    let mut count = 0usize;
    loop {
        let token = probe.next()?;
        count += 1;
        if token.kind() == TokenKind::Comment {
            continue;
        }
        return (!token.kind().is_trivia()).then_some((token, count));
    }
}

pub(crate) fn classify_at_rule(name: &str) -> SyntaxKind {
    if identifier_is_any(name, &["keyframes", "-webkit-keyframes"]) {
        SyntaxKind::KeyframesAtRule
    } else if identifier_is_any(
        name,
        &[
            "media",
            "supports",
            "container",
            "layer",
            "scope",
            "document",
            "starting-style",
        ],
    ) {
        SyntaxKind::GroupAtRule
    } else if identifier_is_any(
        name,
        &[
            "font-face",
            "font-feature-values",
            "page",
            "property",
            "counter-style",
        ],
    ) {
        SyntaxKind::DescriptorAtRule
    } else {
        SyntaxKind::UnknownAtRule
    }
}

/// Carrier-neutral special selector-list pseudos (`:deep` / `:global` / `:slotted`).
///
/// One typed authority both the parser's selector-list classification and the
/// semantic projector's special-pseudo recognition consume. Adding a variant
/// forces every exhaustive match (including the semantic projector) to
/// classify the new name before it compiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpecialSelectorListPseudo {
    Deep,
    Global,
    Slotted,
}

impl SpecialSelectorListPseudo {
    pub const ALL: [Self; 3] = [Self::Deep, Self::Global, Self::Slotted];

    pub const fn ident(self) -> &'static str {
        match self {
            Self::Deep => "deep",
            Self::Global => "global",
            Self::Slotted => "slotted",
        }
    }

    pub const fn vue_prefixed_ident(self) -> &'static str {
        match self {
            Self::Deep => "v-deep",
            Self::Global => "v-global",
            Self::Slotted => "v-slotted",
        }
    }

    pub fn from_ident(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| css_identifier_eq_ignore_ascii_case(name, kind.ident()))
    }
}

fn is_selector_list_pseudo(name: &str, pseudo_element: bool) -> bool {
    if identifier_is_any(name, &["is", "where", "not", "has"]) {
        return true;
    }
    if pseudo_element {
        SpecialSelectorListPseudo::ALL
            .iter()
            .any(|kind| css_identifier_eq_ignore_ascii_case(name, kind.vue_prefixed_ident()))
    } else {
        SpecialSelectorListPseudo::from_ident(name).is_some()
    }
}

fn is_nth_pseudo(name: &str) -> bool {
    identifier_is_any(name, &["nth-child", "nth-last-child"])
}

fn identifier_is_any(raw: &str, expected: &[&str]) -> bool {
    expected
        .iter()
        .any(|known| css_identifier_eq_ignore_ascii_case(raw, known))
}

fn token_name(source: &CssSource, token: SyntaxToken) -> &str {
    let text = source.token_text(token);
    if token.kind() == TokenKind::Function {
        &text[..text.len() - 1]
    } else {
        text
    }
}

fn is_pipe(token: SyntaxToken, source: &CssSource) -> bool {
    token.kind() == TokenKind::Delim && source.token_text(token) == "|"
}

fn is_non_column_combinator(token: SyntaxToken, source: &CssSource) -> bool {
    token.kind() == TokenKind::Delim && matches!(source.token_text(token), ">" | "+" | "~")
}

fn is_namespace_name_side(token: SyntaxToken, source: &CssSource) -> bool {
    token.kind() == TokenKind::Ident
        || (token.kind() == TokenKind::Delim && source.token_text(token) == "*")
}

fn lexical_diagnostic(token: SyntaxToken) -> Option<CssDiagnosticKind> {
    match token.kind() {
        TokenKind::Comment if token.flags & TokenFlags::UNTERMINATED != 0 => {
            Some(CssDiagnosticKind::UnterminatedComment)
        }
        TokenKind::String | TokenKind::LessEscapedString
            if token.flags & TokenFlags::UNTERMINATED != 0 =>
        {
            Some(CssDiagnosticKind::UnterminatedString)
        }
        TokenKind::BadString => Some(CssDiagnosticKind::BadString),
        TokenKind::Url if token.flags & TokenFlags::UNTERMINATED != 0 => {
            Some(CssDiagnosticKind::UnterminatedUrl)
        }
        TokenKind::BadUrl => Some(CssDiagnosticKind::BadUrl),
        _ => None,
    }
}
