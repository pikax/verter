//! Bump-backed stylesheet IR.
//!
//! Public node types (`StyleBlock`, `StyleRule`, `StyleStatement`,
//! `ComponentValueTree`, …) borrow storage from [`StyleSyntaxIr`] and are
//! not `Clone` or `Copy`. Clone [`StyleSyntaxIr`] (an `Arc`) when an owned
//! handle is needed; that clone keeps the bump alive.

use std::sync::Arc;

use bumpalo::Bump;
use smallvec::SmallVec;
use verter_span::Span;

use crate::arena::{alloc_str, bump_for_source, freeze_vec, BumpSlice, BumpStr};
use crate::diagnostic::{CssDiagnostic, CssParseFailure, CssStructureTooLarge};
use crate::dialect::CssDialect;
use crate::event::{NodeFlags, ParseEvent, ParseEventSink, SyntaxKind};
use crate::parser::{parse_with_sink, CssEntryPoint, CssParseMode, CssSource};
use crate::selector::{SelectorComponent, SelectorComponentKind, SelectorList, SelectorSink};
use crate::svelte_compat::svelte_read_value_text;
use crate::token::{css_identifier_eq_ignore_ascii_case, SyntaxToken, TokenKind};
use crate::version::CssSyntaxGrammarVersion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleCompleteness {
    Complete,
    Recovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleBlockKind {
    Braced,
    Indented,
}

#[derive(Debug)]
pub struct StyleBlock {
    kind: StyleBlockKind,
    span: Span,
    statements: BumpSlice<StyleStatement>,
    completeness: StyleCompleteness,
}

impl StyleBlock {
    pub const fn kind(&self) -> StyleBlockKind {
        self.kind
    }

    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn statements(&self) -> &[StyleStatement] {
        self.statements.as_slice()
    }

    pub const fn completeness(&self) -> StyleCompleteness {
        self.completeness
    }
}

#[derive(Debug)]
pub struct StyleRule {
    span: Span,
    selector_list: SelectorList,
    body: StyleBlock,
    completeness: StyleCompleteness,
}

impl StyleRule {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn selector_list(&self) -> &SelectorList {
        &self.selector_list
    }

    pub fn body(&self) -> &StyleBlock {
        &self.body
    }

    pub const fn completeness(&self) -> StyleCompleteness {
        self.completeness
    }
}

#[derive(Debug)]
pub struct StyleDeclaration {
    span: Span,
    name_span: Span,
    value: ComponentValueTree,
    body: Option<StyleBlock>,
    completeness: StyleCompleteness,
}

impl StyleDeclaration {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub const fn name_span(&self) -> Span {
        self.name_span
    }

    pub fn value(&self) -> &ComponentValueTree {
        &self.value
    }

    pub fn body(&self) -> Option<&StyleBlock> {
        self.body.as_ref()
    }

    pub const fn completeness(&self) -> StyleCompleteness {
        self.completeness
    }
}

#[derive(Debug)]
pub struct StyleDirective {
    span: Span,
    head_span: Span,
    opaque_args: ComponentValueTree,
    /// The at-rule's TRIMMED, comment-stripped prelude text (Svelte's
    /// `Atrule.prelude` — its `read_value`'s result), decoded once here, at
    /// parse time, over the already-delimited [`Self::opaque_args`] span —
    /// see [`crate::svelte_read_value_text`]'s doc for why a plain trim
    /// under-approximates this. A reader never re-decodes it.
    prelude_text: BumpStr,
    body: Option<StyleBlock>,
    completeness: StyleCompleteness,
}

impl StyleDirective {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub const fn head_span(&self) -> Span {
        self.head_span
    }

    pub fn opaque_args(&self) -> &ComponentValueTree {
        &self.opaque_args
    }

    /// The at-rule's decoded prelude text — see [`Self::prelude_text`]'s doc.
    pub fn prelude_text(&self) -> &str {
        self.prelude_text.as_str()
    }

    pub fn body(&self) -> Option<&StyleBlock> {
        self.body.as_ref()
    }

    pub const fn completeness(&self) -> StyleCompleteness {
        self.completeness
    }
}

#[derive(Debug)]
pub struct StyleMixinOrFunction {
    span: Span,
    head_span: Span,
    opaque_args: ComponentValueTree,
    body: Option<StyleBlock>,
    completeness: StyleCompleteness,
}

impl StyleMixinOrFunction {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub const fn head_span(&self) -> Span {
        self.head_span
    }

    pub fn opaque_args(&self) -> &ComponentValueTree {
        &self.opaque_args
    }

    pub fn body(&self) -> Option<&StyleBlock> {
        self.body.as_ref()
    }

    pub const fn completeness(&self) -> StyleCompleteness {
        self.completeness
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownStatementKind {
    Unknown,
    Ambiguous,
    Recovery,
}

#[derive(Debug)]
pub struct UnknownStatement {
    kind: UnknownStatementKind,
    span: Span,
    body: Option<StyleBlock>,
    opaque_values: Option<ComponentValueTree>,
}

impl UnknownStatement {
    pub const fn kind(&self) -> UnknownStatementKind {
        self.kind
    }

    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn body(&self) -> Option<&StyleBlock> {
        self.body.as_ref()
    }

    pub fn opaque_values(&self) -> Option<&ComponentValueTree> {
        self.opaque_values.as_ref()
    }
}

#[derive(Debug)]
pub enum StyleStatement {
    Rule(StyleRule),
    Declaration(StyleDeclaration),
    AtRule(StyleDirective),
    MixinOrFunction(StyleMixinOrFunction),
    Unknown(UnknownStatement),
}

#[derive(Debug)]
pub struct ComponentValueTree {
    span: Span,
    values: BumpSlice<ComponentValue>,
    completeness: StyleCompleteness,
}

impl ComponentValueTree {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn values(&self) -> &[ComponentValue] {
        self.values.as_slice()
    }

    pub const fn completeness(&self) -> StyleCompleteness {
        self.completeness
    }

    fn empty(at: u32) -> Self {
        Self {
            span: Span::new(at, at),
            values: BumpSlice::empty(),
            completeness: StyleCompleteness::Complete,
        }
    }
}

#[derive(Debug)]
pub enum ComponentValue {
    Token(ComponentToken),
    String(ComponentToken),
    Comment(ComponentToken),
    Function(ComponentFunction),
    Block(ComponentBlock),
    Interpolation(ValueInterpolation),
}

impl ComponentValue {
    pub const fn span(&self) -> Span {
        match self {
            Self::Token(value) | Self::String(value) | Self::Comment(value) => value.span,
            Self::Function(value) => value.full_span,
            Self::Block(value) => value.full_span,
            Self::Interpolation(value) => value.full_span,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ComponentToken {
    kind: TokenKind,
    span: Span,
}

impl ComponentToken {
    pub const fn kind(&self) -> TokenKind {
        self.kind
    }

    pub const fn span(&self) -> Span {
        self.span
    }
}

#[derive(Debug)]
pub struct ComponentFunction {
    name_span: Span,
    full_span: Span,
    values: BumpSlice<ComponentValue>,
    complete: bool,
}

impl ComponentFunction {
    pub const fn name_span(&self) -> Span {
        self.name_span
    }

    pub const fn full_span(&self) -> Span {
        self.full_span
    }

    pub fn values(&self) -> &[ComponentValue] {
        self.values.as_slice()
    }

    pub const fn is_complete(&self) -> bool {
        self.complete
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentDelimiter {
    Parentheses,
    Brackets,
    Braces,
}

#[derive(Debug)]
pub struct ComponentBlock {
    delimiter: ComponentDelimiter,
    full_span: Span,
    values: BumpSlice<ComponentValue>,
    complete: bool,
}

impl ComponentBlock {
    pub const fn delimiter(&self) -> ComponentDelimiter {
        self.delimiter
    }

    pub const fn full_span(&self) -> Span {
        self.full_span
    }

    pub fn values(&self) -> &[ComponentValue] {
        self.values.as_slice()
    }

    pub const fn is_complete(&self) -> bool {
        self.complete
    }
}

#[derive(Debug)]
pub struct ValueInterpolation {
    full_span: Span,
    payload_span: Span,
    values: BumpSlice<ComponentValue>,
    complete: bool,
}

impl ValueInterpolation {
    pub const fn full_span(&self) -> Span {
        self.full_span
    }

    pub const fn payload_span(&self) -> Span {
        self.payload_span
    }

    pub fn values(&self) -> &[ComponentValue] {
        self.values.as_slice()
    }

    pub const fn is_complete(&self) -> bool {
        self.complete
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StaticClassFact {
    name_span: Span,
}

impl StaticClassFact {
    pub const fn name_span(self) -> Span {
        self.name_span
    }
}

struct StyleSyntaxIrData {
    /// Owns the bump that `statements` and nested child slices point into.
    /// Frozen after parse: never allocated into again, so the IR is `Sync`
    /// even though [`Bump`] itself is not.
    _bump: Bump,
    source: CssSource,
    dialect: CssDialect,
    statements: BumpSlice<StyleStatement>,
    diagnostics: Vec<CssDiagnostic>,
    imports_unresolved: bool,
    /// Every `/* … */` / line-comment span the tokenizer visited while
    /// parsing this stylesheet, in ascending source order (comments cannot
    /// overlap or nest, and tokens are produced in strictly increasing
    /// source-position order, so this list is already sorted by
    /// construction). Retained so a consumer can find comment boundaries
    /// within a byte range through [`StyleSyntaxIr::comment_spans_in`] instead of
    /// re-lexing the source for comment/string state itself.
    comment_spans: Vec<Span>,
    /// The span of a `<!--` CDO token that was never paired with a later
    /// `-->` CDC token in this stylesheet. CSS Syntax Module ignores CDO/CDC
    /// as trivia; Svelte's `read/style.js` treats `<!-- … -->` as a required
    /// paired HTML comment. Minted from the parse's own token stream so the
    /// Svelte reject projection does not re-scan source for the pairing.
    unpaired_cdo_span: Option<Span>,
}

// The bump is write-once during parse and immutable afterwards. Child
// `BumpSlice`s are never written, so sharing the finished IR across threads
// is sound even though `bumpalo::Bump` is `!Sync`.
unsafe impl Sync for StyleSyntaxIrData {}

/// Parsed stylesheet. `Clone` is an `Arc` clone and keeps bump-backed nodes valid.
#[derive(Clone)]
pub struct StyleSyntaxIr {
    data: Arc<StyleSyntaxIrData>,
}

impl StyleSyntaxIr {
    pub fn source(&self) -> &CssSource {
        &self.data.source
    }

    pub fn dialect(&self) -> CssDialect {
        self.data.dialect
    }

    /// Grammar identity that cache keys must include for this projection.
    pub const fn grammar_version(&self) -> CssSyntaxGrammarVersion {
        CssSyntaxGrammarVersion::CURRENT
    }

    pub fn statements(&self) -> &[StyleStatement] {
        self.data.statements.as_slice()
    }

    pub fn diagnostics(&self) -> &[CssDiagnostic] {
        &self.data.diagnostics
    }

    pub fn imports_unresolved(&self) -> bool {
        self.data.imports_unresolved
    }

    pub fn selector_components(&self) -> std::vec::IntoIter<&SelectorComponent> {
        let mut components = Vec::new();
        collect_statement_components(self.statements(), &mut components);
        components.into_iter()
    }

    pub fn complete_static_classes(&self) -> std::vec::IntoIter<StaticClassFact> {
        let mut classes = Vec::new();
        collect_complete_static_classes(self.statements(), &mut classes);
        classes.into_iter()
    }

    pub fn has_dynamic_selectors(&self) -> bool {
        self.selector_components().any(|component| {
            matches!(
                component.kind(),
                SelectorComponentKind::DynamicClass | SelectorComponentKind::Interpolation
            )
        })
    }

    /// Every retained comment span FULLY CONTAINED in `range`, in source
    /// order — a binary-search slice of the parse-time comment inventory, never a
    /// re-lex of `range`'s bytes.
    /// The span of an unpaired `<!--` CDO token, if the parse saw one.
    /// See [`StyleSyntaxIrData::unpaired_cdo_span`] field doc.
    #[must_use]
    pub fn unpaired_cdo_span(&self) -> Option<Span> {
        self.data.unpaired_cdo_span
    }

    pub fn comment_spans_in(&self, range: Span) -> impl Iterator<Item = Span> + '_ {
        let start_idx = self
            .data
            .comment_spans
            .partition_point(|comment| comment.start < range.start);
        self.data.comment_spans[start_idx..]
            .iter()
            .copied()
            .take_while(move |comment| comment.end <= range.end)
    }
}

fn collect_complete_static_classes(
    statements: &[StyleStatement],
    output: &mut Vec<StaticClassFact>,
) {
    for statement in statements {
        match statement {
            StyleStatement::Rule(rule) => {
                for selector in rule.selector_list.selectors() {
                    if selector.facts().completeness()
                        == crate::selector::SelectorCompleteness::Complete
                    {
                        for compound in selector.compounds() {
                            for component in compound.components() {
                                collect_static_class_component(component, output);
                            }
                        }
                    }
                }
                collect_complete_static_classes(rule.body.statements(), output);
            }
            StyleStatement::AtRule(value) => {
                if let Some(body) = value.body() {
                    collect_complete_static_classes(body.statements(), output);
                }
            }
            StyleStatement::MixinOrFunction(value) => {
                if let Some(body) = value.body() {
                    collect_complete_static_classes(body.statements(), output);
                }
            }
            StyleStatement::Unknown(value) => {
                if let Some(body) = value.body() {
                    collect_complete_static_classes(body.statements(), output);
                }
            }
            StyleStatement::Declaration(value) => {
                if let Some(body) = value.body() {
                    collect_complete_static_classes(body.statements(), output);
                }
            }
        }
    }
}

fn collect_static_class_component(
    component: &SelectorComponent,
    output: &mut Vec<StaticClassFact>,
) {
    if component.kind() == SelectorComponentKind::Class && component.facts().is_complete_static() {
        if let Some(name_span) = component.name_span() {
            output.push(StaticClassFact { name_span });
        }
    }
    for nested in component.nested_components() {
        collect_static_class_component(nested, output);
    }
    if let Some(list) = component.pseudo().and_then(|pseudo| pseudo.selector_list()) {
        for selector in list.selectors() {
            for compound in selector.compounds() {
                for nested in compound.components() {
                    collect_static_class_component(nested, output);
                }
            }
        }
    }
}

fn collect_statement_components<'a>(
    statements: &'a [StyleStatement],
    output: &mut Vec<&'a SelectorComponent>,
) {
    for statement in statements {
        match statement {
            StyleStatement::Rule(rule) => {
                for selector in rule.selector_list.selectors() {
                    for compound in selector.compounds() {
                        for component in compound.components() {
                            collect_component_refs(component, output);
                        }
                    }
                }
                collect_statement_components(rule.body.statements(), output);
            }
            StyleStatement::AtRule(value) => {
                if let Some(body) = &value.body {
                    collect_statement_components(body.statements(), output);
                }
            }
            StyleStatement::MixinOrFunction(value) => {
                if let Some(body) = &value.body {
                    collect_statement_components(body.statements(), output);
                }
            }
            StyleStatement::Unknown(value) => {
                if let Some(body) = &value.body {
                    collect_statement_components(body.statements(), output);
                }
            }
            StyleStatement::Declaration(value) => {
                if let Some(body) = &value.body {
                    collect_statement_components(body.statements(), output);
                }
            }
        }
    }
}

fn collect_component_refs<'a>(
    component: &'a SelectorComponent,
    output: &mut Vec<&'a SelectorComponent>,
) {
    output.push(component);
    for nested in component.nested_components() {
        collect_component_refs(nested, output);
    }
    if let Some(list) = component.pseudo().and_then(|pseudo| pseudo.selector_list()) {
        for selector in list.selectors() {
            for compound in selector.compounds() {
                for nested in compound.components() {
                    collect_component_refs(nested, output);
                }
            }
        }
    }
}

struct OpenFrame<'b> {
    kind: SyntaxKind,
    flags: NodeFlags,
    start: u32,
    token_start: usize,
    statements: bumpalo::collections::Vec<'b, StyleStatement>,
    values: bumpalo::collections::Vec<'b, ComponentValue>,
    value_tree: Option<ComponentValueTree>,
    selector_list: Option<SelectorList>,
    block: Option<StyleBlock>,
    recovered: bool,
}

pub(crate) struct StyleSyntaxIrSink<'b> {
    bump: &'b Bump,
    source: CssSource,
    dialect: CssDialect,
    open: SmallVec<[OpenFrame<'b>; 16]>,
    tokens: bumpalo::collections::Vec<'b, SyntaxToken>,
    statements: bumpalo::collections::Vec<'b, StyleStatement>,
    diagnostics: Vec<CssDiagnostic>,
    selector_sink: Option<(usize, SelectorSink<'b>)>,
    imports_unresolved: bool,
    root_value_tree: Option<ComponentValueTree>,
    /// Whole-source comment inventory, accumulated at event time. Comment
    /// spans are a fact about the SOURCE, not about the IR tree: a comment in
    /// a selector prelude belongs here just as much as one in a declaration
    /// block, even though the selector's events are owned by `SelectorSink`
    /// and open no IR frame. Appending as events arrive keeps it in ascending
    /// source order, which [`StyleSyntaxIr::comment_spans_in`] binary-searches.
    comment_spans: Vec<Span>,
    /// Running CDO/CDC pairing state, folded at event time for the same
    /// reason: `<!--` and `-->` are trivia that can appear anywhere in the
    /// source, selector preludes included.
    unpaired_cdo_span: Option<Span>,
}

struct FrozenStyleIr {
    source: CssSource,
    dialect: CssDialect,
    statements: BumpSlice<StyleStatement>,
    diagnostics: Vec<CssDiagnostic>,
    imports_unresolved: bool,
    comment_spans: Vec<Span>,
    unpaired_cdo_span: Option<Span>,
    root_value_tree: Option<ComponentValueTree>,
}

impl<'b> StyleSyntaxIrSink<'b> {
    pub fn new(bump: &'b Bump, source: CssSource, dialect: CssDialect) -> Self {
        Self::with_entry_point(bump, source, dialect, CssEntryPoint::Stylesheet)
    }

    /// A sink sized for `entry`. Only a stylesheet parse produces top-level statements, so a
    /// component-value parse reserves none: `parse_component_value_tree` returns just the value
    /// tree, and `verter_semantic` drives that path for variable extraction — a reservation it
    /// would never fill.
    pub fn with_entry_point(
        bump: &'b Bump,
        source: CssSource,
        dialect: CssDialect,
        entry: CssEntryPoint,
    ) -> Self {
        let token_cap = (source.text().len() / 3).max(16);
        let statement_cap = if matches!(entry, CssEntryPoint::Stylesheet) {
            (source.text().len() / 24).max(8)
        } else {
            0
        };
        Self {
            bump,
            source,
            dialect,
            open: SmallVec::new(),
            tokens: bumpalo::collections::Vec::with_capacity_in(token_cap, bump),
            statements: bumpalo::collections::Vec::with_capacity_in(statement_cap, bump),
            diagnostics: Vec::new(),
            selector_sink: None,
            imports_unresolved: false,
            root_value_tree: None,
            comment_spans: Vec::new(),
            unpaired_cdo_span: None,
        }
    }

    fn finish_frozen(self) -> Result<FrozenStyleIr, CssStructureTooLarge> {
        verter_debug_assert!(self.open.is_empty(), "parser must balance IR frames");
        Ok(FrozenStyleIr {
            source: self.source,
            dialect: self.dialect,
            statements: freeze_vec(self.statements),
            diagnostics: self.diagnostics,
            imports_unresolved: self.imports_unresolved,
            comment_spans: self.comment_spans,
            unpaired_cdo_span: self.unpaired_cdo_span,
            root_value_tree: self.root_value_tree,
        })
    }

    fn token_value(token: SyntaxToken) -> ComponentValue {
        let value = ComponentToken {
            kind: token.kind(),
            span: Span::new(token.start, token.end),
        };
        match token.kind() {
            TokenKind::String | TokenKind::BadString | TokenKind::LessEscapedString => {
                ComponentValue::String(value)
            }
            TokenKind::Comment | TokenKind::LineComment => ComponentValue::Comment(value),
            _ => ComponentValue::Token(value),
        }
    }

    fn is_value_frame(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::ComponentValueList
                | SyntaxKind::AtRulePrelude
                | SyntaxKind::Function
                | SyntaxKind::ComponentValueBlock
                | SyntaxKind::Interpolation
                | SyntaxKind::AmbiguousStatement
        )
    }

    fn close_frame(&mut self, frame: OpenFrame<'b>, end: u32) {
        let span = Span::new(frame.start, end);
        let tokens = &self.tokens[frame.token_start..];
        let completeness = if frame.recovered || frame.flags.0 & NodeFlags::RECOVERED.0 != 0 {
            StyleCompleteness::Recovered
        } else {
            StyleCompleteness::Complete
        };
        let first = tokens.first().copied();
        if matches!(
            frame.kind,
            SyntaxKind::Function | SyntaxKind::ComponentValueBlock | SyntaxKind::Interpolation
        ) {
            let built_value = match (frame.kind, first) {
                (SyntaxKind::Function, Some(opener)) => {
                    let closed = tokens
                        .last()
                        .is_some_and(|token| token.kind() == TokenKind::RightParen);
                    Some(ComponentValue::Function(ComponentFunction {
                        name_span: Span::new(opener.start, opener.end.saturating_sub(1)),
                        full_span: span,
                        values: freeze_vec(trim_delimiters(frame.values, closed)),
                        complete: closed && completeness == StyleCompleteness::Complete,
                    }))
                }
                (SyntaxKind::ComponentValueBlock, Some(opener)) => {
                    let (delimiter, closer) = match opener.kind() {
                        TokenKind::LeftParen => {
                            (ComponentDelimiter::Parentheses, TokenKind::RightParen)
                        }
                        TokenKind::LeftBracket => {
                            (ComponentDelimiter::Brackets, TokenKind::RightBracket)
                        }
                        _ => (ComponentDelimiter::Braces, TokenKind::RightBrace),
                    };
                    let closed = tokens.last().is_some_and(|token| token.kind() == closer);
                    Some(ComponentValue::Block(ComponentBlock {
                        delimiter,
                        full_span: span,
                        values: freeze_vec(trim_delimiters(frame.values, closed)),
                        complete: closed && completeness == StyleCompleteness::Complete,
                    }))
                }
                (SyntaxKind::Interpolation, Some(opener)) => {
                    let closed = tokens
                        .last()
                        .is_some_and(|token| token.kind() == TokenKind::RightBrace);
                    let payload_end = if closed {
                        tokens.last().map_or(end, |token| token.start)
                    } else {
                        end
                    };
                    Some(ComponentValue::Interpolation(ValueInterpolation {
                        full_span: span,
                        payload_span: Span::new(opener.end, payload_end),
                        values: freeze_vec(trim_delimiters(frame.values, closed)),
                        complete: closed && completeness == StyleCompleteness::Complete,
                    }))
                }
                _ => None,
            };
            if let Some(value) = built_value {
                if let Some(parent) = self.open.last_mut() {
                    parent.values.push(value);
                }
            }
            return;
        }

        match frame.kind {
            SyntaxKind::ComponentValueList | SyntaxKind::AtRulePrelude => {
                let tree = ComponentValueTree {
                    span,
                    values: freeze_vec(frame.values),
                    completeness,
                };
                if let Some(parent) = self.open.last_mut() {
                    parent.value_tree = Some(tree);
                } else {
                    self.root_value_tree = Some(tree);
                }
            }
            SyntaxKind::RuleBlock | SyntaxKind::AtRuleBlock | SyntaxKind::IndentedBlock => {
                let block = StyleBlock {
                    kind: if frame.kind == SyntaxKind::IndentedBlock {
                        StyleBlockKind::Indented
                    } else {
                        StyleBlockKind::Braced
                    },
                    span,
                    statements: freeze_vec(frame.statements),
                    completeness,
                };
                if let Some(parent) = self.open.last_mut() {
                    parent.block = Some(block);
                }
            }
            SyntaxKind::Declaration
            | SyntaxKind::CustomPropertyDeclaration
            | SyntaxKind::VariableDeclaration => {
                let name_span = first.map_or(Span::new(frame.start, frame.start), |token| {
                    Span::new(token.start, token.end)
                });
                let statement = StyleStatement::Declaration(StyleDeclaration {
                    span,
                    name_span,
                    value: frame
                        .value_tree
                        .unwrap_or_else(|| ComponentValueTree::empty(end)),
                    body: frame.block,
                    completeness,
                });
                self.push_statement(statement);
            }
            SyntaxKind::QualifiedRule => match (frame.selector_list, frame.block) {
                (Some(selector_list), Some(body)) => {
                    self.push_statement(StyleStatement::Rule(StyleRule {
                        span,
                        selector_list,
                        body,
                        completeness,
                    }));
                }
                (_, body) => {
                    self.push_unknown(UnknownStatementKind::Recovery, span, body, None);
                }
            },
            SyntaxKind::GroupAtRule
            | SyntaxKind::DescriptorAtRule
            | SyntaxKind::KeyframesAtRule
            | SyntaxKind::UnknownAtRule
            | SyntaxKind::ControlDirective => {
                let head_span = first.map_or(Span::new(frame.start, frame.start), |token| {
                    Span::new(token.start, token.end)
                });
                if first.is_some_and(|token| {
                    let raw = self.source.token_text(token);
                    raw.strip_prefix('@').is_some_and(|name| {
                        ["import", "use", "forward", "plugin"]
                            .iter()
                            .any(|expected| css_identifier_eq_ignore_ascii_case(name, expected))
                    })
                }) {
                    self.imports_unresolved = true;
                }
                let opaque_args = frame
                    .value_tree
                    .unwrap_or_else(|| ComponentValueTree::empty(head_span.end));
                let prelude_text = svelte_read_value_text(&self.source, opaque_args.span());
                self.push_statement(StyleStatement::AtRule(StyleDirective {
                    span,
                    head_span,
                    opaque_args,
                    prelude_text: alloc_str(self.bump, &prelude_text),
                    body: frame.block,
                    completeness,
                }));
            }
            SyntaxKind::MixinOrFunctionHeader => {
                let head_span = first.map_or(Span::new(frame.start, frame.start), |token| {
                    Span::new(token.start, token.end)
                });
                self.push_statement(StyleStatement::MixinOrFunction(StyleMixinOrFunction {
                    span,
                    head_span,
                    opaque_args: frame
                        .value_tree
                        .unwrap_or_else(|| ComponentValueTree::empty(head_span.end)),
                    body: frame.block,
                    completeness,
                }));
            }
            SyntaxKind::AmbiguousStatement => self.push_unknown(
                UnknownStatementKind::Ambiguous,
                span,
                frame.block,
                frame.value_tree,
            ),
            SyntaxKind::Recovery => self.push_unknown(
                UnknownStatementKind::Recovery,
                span,
                frame.block,
                frame.value_tree,
            ),
            SyntaxKind::Stylesheet => {
                self.statements = frame.statements;
            }
            _ => {}
        }
    }

    fn push_statement(&mut self, statement: StyleStatement) {
        if let Some(parent) = self.open.last_mut() {
            parent.statements.push(statement);
        } else {
            self.statements.push(statement);
        }
    }

    fn push_unknown(
        &mut self,
        kind: UnknownStatementKind,
        span: Span,
        body: Option<StyleBlock>,
        opaque_values: Option<ComponentValueTree>,
    ) {
        self.push_statement(StyleStatement::Unknown(UnknownStatement {
            kind,
            span,
            body,
            opaque_values,
        }));
    }
}

fn trim_delimiters<'b>(
    mut values: bumpalo::collections::Vec<'b, ComponentValue>,
    closed: bool,
) -> bumpalo::collections::Vec<'b, ComponentValue> {
    if !values.is_empty() {
        values.remove(0);
    }
    if closed && !values.is_empty() {
        values.pop();
    }
    values
}

impl ParseEventSink for StyleSyntaxIrSink<'_> {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        if let ParseEvent::Diagnostic(diagnostic) = event {
            self.diagnostics.push(diagnostic);
            if diagnostic.kind != crate::diagnostic::CssDiagnosticKind::AmbiguousStatement {
                for frame in &mut self.open {
                    frame.recovered = true;
                }
            }
        }

        // Whole-source token facts are observed here, ABOVE the selector
        // branch, because they describe the source rather than the IR tree:
        // selector events open no IR frame and are not retained in `tokens`,
        // but a comment or a CDO/CDC in a selector prelude is still part of
        // the stylesheet. `event` is called once per event, so every token is
        // observed exactly once.
        if let ParseEvent::Token(token) = event {
            match token.kind() {
                TokenKind::Comment | TokenKind::LineComment => {
                    self.comment_spans.push(Span::new(token.start, token.end));
                }
                TokenKind::Cdo => {
                    self.unpaired_cdo_span = Some(Span::new(token.start, token.end));
                }
                TokenKind::Cdc => self.unpaired_cdo_span = None,
                _ => {}
            }
        }

        if let Some((depth, sink)) = &mut self.selector_sink {
            sink.event(event)?;
            match event {
                ParseEvent::StartNode { .. } => *depth += 1,
                ParseEvent::FinishNode { .. } => *depth = depth.saturating_sub(1),
                ParseEvent::Token(_) | ParseEvent::Diagnostic(_) => {}
            }
            if *depth == 0 {
                let (_, sink) = self.selector_sink.take().expect("selector sink exists");
                let list = sink.finish_list();
                if let Some(rule) = self
                    .open
                    .iter_mut()
                    .rev()
                    .find(|frame| frame.kind == SyntaxKind::QualifiedRule)
                {
                    notify_parse_phase("selector_clone_enter");
                    rule.selector_list = Some(list);
                    notify_parse_phase("selector_clone_exit");
                }
            }
            // Selector events are owned by SelectorSink; do not also open IR frames
            // or duplicate token storage for a tree that close_frame would discard.
            return Ok(());
        }

        if let ParseEvent::StartNode {
            kind: SyntaxKind::SelectorList,
            ..
        } = event
        {
            let mut sink = SelectorSink::new(self.bump, self.source.clone());
            sink.event(event)?;
            self.selector_sink = Some((1, sink));
            return Ok(());
        }

        match event {
            ParseEvent::StartNode { kind, flags, start } => self.open.push(OpenFrame {
                kind,
                flags,
                start,
                token_start: self.tokens.len(),
                statements: bumpalo::collections::Vec::new_in(self.bump),
                values: bumpalo::collections::Vec::new_in(self.bump),
                value_tree: None,
                selector_list: None,
                block: None,
                recovered: false,
            }),
            ParseEvent::Token(token) => {
                self.tokens.push(token);
                if self
                    .open
                    .last()
                    .is_some_and(|frame| Self::is_value_frame(frame.kind))
                {
                    let value = Self::token_value(token);
                    self.open
                        .last_mut()
                        .expect("value frame exists")
                        .values
                        .push(value);
                }
            }
            ParseEvent::FinishNode { kind, end } => {
                let frame = self.open.pop().expect("parser emits balanced IR nodes");
                verter_debug_assert_eq!(frame.kind, kind);
                self.close_frame(frame, end);
            }
            ParseEvent::Diagnostic(_) => {}
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Per-thread count of [`parse_style_ir`] executions — the routing half
    /// of the "one shared-grammar parse per `<style>` body" proof
    /// (`verter_compiler`'s Svelte CSS pipeline binds to it). Incremented
    /// inside `parse_style_ir` itself, the SOLE entry point every caller
    /// (Svelte's `analyze_style_body`, Vue's inline-style routing, direct
    /// tests) goes through, so a second call from ANYWHERE moves this
    /// counter — unlike a counter a caller bumps beside its own call site,
    /// which only proves that ONE call site ran once. Compiled only under
    /// `test-support` (a consumer dev-dependency edge — `#[cfg(test)]` alone
    /// cannot serve a cross-crate integration test), so production builds
    /// carry neither the TLS nor the increment.
    static STYLE_IR_PARSE_INVOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The number of [`parse_style_ir`] executions performed on the CALLING
/// thread. Test/guard observability only — see the thread-local's doc.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn parse_style_ir_thread_invocations() -> u64 {
    STYLE_IR_PARSE_INVOCATIONS.with(std::cell::Cell::get)
}

#[cfg(any(test, feature = "test-support"))]
mod parse_phase_probe {
    use std::cell::Cell;

    thread_local! {
        static PROBE: Cell<Option<fn(&'static str)>> = const { Cell::new(None) };
    }

    pub fn replace(probe: Option<fn(&'static str)>) -> Option<fn(&'static str)> {
        PROBE.with(|slot| slot.replace(probe))
    }

    pub fn notify(phase: &'static str) {
        PROBE.with(|slot| {
            if let Some(probe) = slot.get() {
                probe(phase);
            }
        });
    }
}

/// Installs a parse-phase observer used by allocation attribution tests and returns whatever
/// was installed before, so a caller can restore it and never leave a foreign probe armed on
/// this thread. Production builds compile this out (`test-support` is a consumer-dev feature).
#[cfg(any(test, feature = "test-support"))]
pub fn set_style_ir_parse_phase_probe(probe: Option<fn(&'static str)>) -> Option<fn(&'static str)> {
    parse_phase_probe::replace(probe)
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn notify_parse_phase(phase: &'static str) {
    parse_phase_probe::notify(phase);
}

#[cfg(not(any(test, feature = "test-support")))]
pub(crate) fn notify_parse_phase(_phase: &'static str) {}

fn ir_from_frozen(bump: Bump, frozen: FrozenStyleIr) -> StyleSyntaxIr {
    StyleSyntaxIr {
        data: Arc::new(StyleSyntaxIrData {
            _bump: bump,
            source: frozen.source,
            dialect: frozen.dialect,
            statements: frozen.statements,
            diagnostics: frozen.diagnostics,
            imports_unresolved: frozen.imports_unresolved,
            comment_spans: frozen.comment_spans,
            unpaired_cdo_span: frozen.unpaired_cdo_span,
        }),
    }
}

pub fn parse_style_ir(
    source: CssSource,
    dialect: CssDialect,
    mode: CssParseMode,
) -> Result<StyleSyntaxIr, CssParseFailure> {
    #[cfg(any(test, feature = "test-support"))]
    STYLE_IR_PARSE_INVOCATIONS.with(|count| count.set(count.get().saturating_add(1)));

    let bump = bump_for_source(source.text().len());
    let frozen = {
        let mut sink = StyleSyntaxIrSink::new(&bump, source.clone(), dialect);
        notify_parse_phase("after_sink_new");
        parse_with_sink(&source, dialect, CssEntryPoint::Stylesheet, mode, &mut sink)?;
        notify_parse_phase("after_parse_emit");
        sink.finish_frozen().map_err(CssParseFailure::Structure)?
    };
    let ir = ir_from_frozen(bump, frozen);
    notify_parse_phase("after_finish");
    Ok(ir)
}

/// A component-value parse whose child lists live in an owned bump.
pub struct OwnedComponentValueTree {
    _ir: StyleSyntaxIr,
    tree: ComponentValueTree,
}

impl std::ops::Deref for OwnedComponentValueTree {
    type Target = ComponentValueTree;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

pub fn parse_component_value_tree(
    source: CssSource,
    dialect: CssDialect,
    mode: CssParseMode,
) -> Result<OwnedComponentValueTree, CssParseFailure> {
    let origin = source.origin();
    let bump = bump_for_source(source.text().len());
    let frozen = {
        let mut sink = StyleSyntaxIrSink::with_entry_point(
            &bump,
            source.clone(),
            dialect,
            CssEntryPoint::ComponentValueList,
        );
        parse_with_sink(
            &source,
            dialect,
            CssEntryPoint::ComponentValueList,
            mode,
            &mut sink,
        )?;
        sink.finish_frozen().map_err(CssParseFailure::Structure)?
    };
    let mut frozen = frozen;
    let tree = frozen
        .root_value_tree
        .take()
        .unwrap_or_else(|| ComponentValueTree::empty(origin));
    Ok(OwnedComponentValueTree {
        _ir: ir_from_frozen(bump, frozen),
        tree,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::cst::LosslessCstSink;
    use crate::diagnostic::CssDiagnosticKind;
    use crate::parser::{parse_with_sink, CssEntryPoint};
    use crate::selector::{SelectorCompleteness, SelectorSink, SelectorStructure};

    fn ir(input: &str, dialect: CssDialect) -> StyleSyntaxIr {
        parse_style_ir(
            CssSource::new(Arc::from(input), 0).unwrap(),
            dialect,
            CssParseMode::Recover,
        )
        .unwrap()
    }

    // @ai-generated - Proves StyleSyntaxIr is projected directly from the same event stream as the CST.
    #[test]
    fn style_ir_and_lossless_cst_are_peer_event_sinks() {
        fn accepts_sink(_: &mut impl ParseEventSink) {}

        struct Peers<'a, 'b> {
            cst: &'a mut LosslessCstSink,
            ir: &'a mut StyleSyntaxIrSink<'b>,
        }

        impl ParseEventSink for Peers<'_, '_> {
            fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
                self.cst.event(event)?;
                self.ir.event(event)
            }
        }

        let input =
            ".card, #hero { color: calc(1px + var(--x)); content: \"x\"; } @import \"theme.css\";";
        let source = CssSource::new(Arc::from(input), 17).unwrap();
        let bump = bump_for_source(source.text().len());
        let mut ir_sink = StyleSyntaxIrSink::new(&bump, source.clone(), CssDialect::Css);
        accepts_sink(&mut ir_sink);
        let mut cst_sink = LosslessCstSink::new(source.clone());
        let mut peers = Peers {
            cst: &mut cst_sink,
            ir: &mut ir_sink,
        };
        parse_with_sink(
            &source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
            &mut peers,
        )
        .unwrap();
        let frozen = ir_sink.finish_frozen().unwrap();
        let ir = ir_from_frozen(bump, frozen);
        let cst = cst_sink.finish().unwrap();

        assert_eq!(cst.reconstruct(), input);
        assert_eq!(ir.source().text(), input);
        assert_eq!(ir.grammar_version(), CssSyntaxGrammarVersion::CURRENT);
        assert_eq!(ir.statements().len(), 2);
        assert!(ir.imports_unresolved());

        let StyleStatement::Rule(rule) = &ir.statements()[0] else {
            panic!("first statement must be a rule");
        };
        let components: Vec<_> = rule
            .selector_list()
            .selectors()
            .iter()
            .flat_map(|selector| selector.compounds())
            .flat_map(|compound| compound.components())
            .collect();
        assert!(components
            .iter()
            .any(|component| component.kind() == SelectorComponentKind::Class));
        assert!(components
            .iter()
            .any(|component| component.kind() == SelectorComponentKind::Id));

        let StyleStatement::Declaration(color) = &rule.body().statements()[0] else {
            panic!("rule body must contain a declaration");
        };
        assert_eq!(source.slice(color.name_span()), "color");
        assert!(color
            .value()
            .values()
            .iter()
            .any(|value| matches!(value, ComponentValue::Function(function) if source.slice(function.name_span()) == "calc")));
    }

    /// `StyleSyntaxIrSink` hands every event inside a `SelectorList` to its nested `SelectorSink` and
    /// then stops processing that event itself. A diagnostic raised inside that window is still the
    /// stylesheet's diagnostic, so it must be recorded before the selector-sink hand-off — not
    /// dropped with the rest of the event. `.a[ {}` is the discriminating input: `UnterminatedBlock`
    /// is raised while the selector sink owns events and `ExpectedRuleBlock` after it closes, so a
    /// hand-off that swallows in-window diagnostics keeps the second and loses the first.
    #[test]
    fn diagnostics_raised_inside_a_selector_list_still_reach_the_style_ir() {
        for input in ["[ {}", ".a[ {}", ":global( {}", ".a:nth-child(-2n {}"] {
            assert_eq!(
                ir(input, CssDialect::Css)
                    .diagnostics()
                    .iter()
                    .map(|diagnostic| diagnostic.kind)
                    .collect::<Vec<_>>(),
                vec![
                    CssDiagnosticKind::UnterminatedBlock,
                    CssDiagnosticKind::ExpectedRuleBlock,
                ],
                "{input}"
            );
        }

        // The same window on the LAYOUT path. Sass and Stylus reach the IR sink through the
        // indentation-aware parser, which surrounds the selector window with its own statement
        // classification, so their full sequence differs from Css's — but the selector-window
        // diagnostic itself must survive on every path, which is what the hand-off is about.
        for dialect in [
            CssDialect::Css,
            CssDialect::Scss,
            CssDialect::Less,
            CssDialect::Sass,
            CssDialect::Stylus,
        ] {
            let kinds: Vec<_> = ir(".a[ {}", dialect)
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect();
            assert!(
                kinds.contains(&CssDiagnosticKind::UnterminatedBlock),
                "{dialect:?}: selector-window diagnostic lost, got {kinds:?}"
            );
        }

        // Negative control: a well-formed stylesheet records no diagnostics at all, so the assertion
        // above is not satisfied by a sink that indiscriminately records everything.
        for dialect in [CssDialect::Css, CssDialect::Sass, CssDialect::Stylus] {
            assert!(
                ir(".a { color: red }", dialect).diagnostics().is_empty(),
                "{dialect:?}"
            );
        }
    }

    // @ai-generated - Pins pseudo-list descent to component trust, not enclosing selector completeness.
    #[test]
    fn recovered_pseudo_selector_keeps_disjoint_complete_class_fact() {
        let source = CssSource::new(Arc::from(":is(.a .b#{$x"), 0).unwrap();
        let bump = bump_for_source(source.text().len());
        let mut sink = SelectorSink::new(&bump, source.clone());
        parse_with_sink(
            &source,
            CssDialect::Scss,
            CssEntryPoint::SelectorList,
            CssParseMode::Recover,
            &mut sink,
        )
        .unwrap();
        let list = sink.finish_list();
        let structure = SelectorStructure::from_parts(bump, source.clone(), list);
        let pseudo = structure
            .components()
            .into_iter()
            .find(|component| component.kind() == SelectorComponentKind::FunctionalPseudo)
            .expect("functional pseudo component");
        let nested = pseudo
            .pseudo()
            .and_then(|pseudo| pseudo.selector_list())
            .and_then(|list| list.selectors().first())
            .expect("nested recovered selector");
        assert_eq!(
            nested.facts().completeness(),
            SelectorCompleteness::Recovered
        );

        let mut classes = Vec::new();
        collect_static_class_component(pseudo, &mut classes);
        let names: Vec<_> = classes
            .iter()
            .map(|class| source.slice(class.name_span()).to_owned())
            .collect();
        assert_eq!(names, vec!["a"]);
    }
}
