use smallvec::SmallVec;
use verter_span::Span;

use crate::diagnostic::{CssDiagnostic, CssParseFailure, CssStructureTooLarge};
use crate::dialect::CssDialect;
use crate::event::{NodeFlags, ParseEvent, ParseEventSink, SyntaxKind};
use crate::parser::{parse_with_sink, CssEntryPoint, CssParseMode, CssSource};
use crate::selector::{
    SelectorComponent, SelectorComponentKind, SelectorList, SelectorSink, SelectorStructure,
};
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

#[derive(Debug, Clone)]
pub struct StyleBlock {
    kind: StyleBlockKind,
    span: Span,
    statements: Vec<StyleStatement>,
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
        &self.statements
    }

    pub const fn completeness(&self) -> StyleCompleteness {
        self.completeness
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct StyleDirective {
    span: Span,
    head_span: Span,
    opaque_args: ComponentValueTree,
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

    pub fn body(&self) -> Option<&StyleBlock> {
        self.body.as_ref()
    }

    pub const fn completeness(&self) -> StyleCompleteness {
        self.completeness
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum StyleStatement {
    Rule(StyleRule),
    Declaration(StyleDeclaration),
    AtRule(StyleDirective),
    MixinOrFunction(StyleMixinOrFunction),
    Unknown(UnknownStatement),
}

#[derive(Debug, Clone)]
pub struct ComponentValueTree {
    span: Span,
    values: Vec<ComponentValue>,
    completeness: StyleCompleteness,
}

impl ComponentValueTree {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn values(&self) -> &[ComponentValue] {
        &self.values
    }

    pub const fn completeness(&self) -> StyleCompleteness {
        self.completeness
    }

    fn empty(at: u32) -> Self {
        Self {
            span: Span::new(at, at),
            values: Vec::new(),
            completeness: StyleCompleteness::Complete,
        }
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct ComponentFunction {
    name_span: Span,
    full_span: Span,
    values: Vec<ComponentValue>,
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
        &self.values
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

#[derive(Debug, Clone)]
pub struct ComponentBlock {
    delimiter: ComponentDelimiter,
    full_span: Span,
    values: Vec<ComponentValue>,
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
        &self.values
    }

    pub const fn is_complete(&self) -> bool {
        self.complete
    }
}

#[derive(Debug, Clone)]
pub struct ValueInterpolation {
    full_span: Span,
    payload_span: Span,
    values: Vec<ComponentValue>,
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
        &self.values
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

pub struct StyleSyntaxIr {
    source: CssSource,
    dialect: CssDialect,
    statements: Vec<StyleStatement>,
    diagnostics: Vec<CssDiagnostic>,
    imports_unresolved: bool,
}

impl StyleSyntaxIr {
    pub fn source(&self) -> &CssSource {
        &self.source
    }

    pub const fn dialect(&self) -> CssDialect {
        self.dialect
    }

    /// Grammar identity that cache keys must include for this projection.
    pub const fn grammar_version(&self) -> CssSyntaxGrammarVersion {
        CssSyntaxGrammarVersion::CURRENT
    }

    pub fn statements(&self) -> &[StyleStatement] {
        &self.statements
    }

    pub fn diagnostics(&self) -> &[CssDiagnostic] {
        &self.diagnostics
    }

    pub const fn imports_unresolved(&self) -> bool {
        self.imports_unresolved
    }

    pub fn selector_components(&self) -> std::vec::IntoIter<&SelectorComponent> {
        let mut components = Vec::new();
        collect_statement_components(&self.statements, &mut components);
        components.into_iter()
    }

    pub fn complete_static_classes(&self) -> std::vec::IntoIter<StaticClassFact> {
        let mut classes = Vec::new();
        collect_complete_static_classes(&self.statements, &mut classes);
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
                collect_statement_components(&rule.body.statements, output);
            }
            StyleStatement::AtRule(value) => {
                if let Some(body) = &value.body {
                    collect_statement_components(&body.statements, output);
                }
            }
            StyleStatement::MixinOrFunction(value) => {
                if let Some(body) = &value.body {
                    collect_statement_components(&body.statements, output);
                }
            }
            StyleStatement::Unknown(value) => {
                if let Some(body) = &value.body {
                    collect_statement_components(&body.statements, output);
                }
            }
            StyleStatement::Declaration(value) => {
                if let Some(body) = &value.body {
                    collect_statement_components(&body.statements, output);
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

struct OpenFrame {
    kind: SyntaxKind,
    flags: NodeFlags,
    start: u32,
    token_start: usize,
    statements: Vec<StyleStatement>,
    values: Vec<ComponentValue>,
    value_tree: Option<ComponentValueTree>,
    selector_list: Option<SelectorList>,
    block: Option<StyleBlock>,
    recovered: bool,
}

pub struct StyleSyntaxIrSink {
    source: CssSource,
    dialect: CssDialect,
    open: SmallVec<[OpenFrame; 16]>,
    tokens: Vec<SyntaxToken>,
    statements: Vec<StyleStatement>,
    diagnostics: Vec<CssDiagnostic>,
    selector_sink: Option<(usize, SelectorSink)>,
    imports_unresolved: bool,
    root_value_tree: Option<ComponentValueTree>,
}

impl StyleSyntaxIrSink {
    pub fn new(source: CssSource, dialect: CssDialect) -> Self {
        Self {
            source,
            dialect,
            open: SmallVec::new(),
            tokens: Vec::new(),
            statements: Vec::new(),
            diagnostics: Vec::new(),
            selector_sink: None,
            imports_unresolved: false,
            root_value_tree: None,
        }
    }

    pub fn finish(self) -> Result<StyleSyntaxIr, CssStructureTooLarge> {
        verter_debug_assert!(self.open.is_empty(), "parser must balance IR frames");
        Ok(StyleSyntaxIr {
            source: self.source,
            dialect: self.dialect,
            statements: self.statements,
            diagnostics: self.diagnostics,
            imports_unresolved: self.imports_unresolved,
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

    fn close_frame(&mut self, frame: OpenFrame, end: u32) {
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
                        values: trim_delimiters(frame.values, closed),
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
                        values: trim_delimiters(frame.values, closed),
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
                        values: trim_delimiters(frame.values, closed),
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
                    values: frame.values,
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
                    statements: frame.statements,
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
                self.push_statement(StyleStatement::AtRule(StyleDirective {
                    span,
                    head_span,
                    opaque_args: frame
                        .value_tree
                        .unwrap_or_else(|| ComponentValueTree::empty(head_span.end)),
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

fn trim_delimiters(mut values: Vec<ComponentValue>, closed: bool) -> Vec<ComponentValue> {
    if !values.is_empty() {
        values.remove(0);
    }
    if closed && !values.is_empty() {
        values.pop();
    }
    values
}

impl ParseEventSink for StyleSyntaxIrSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        if let Some((depth, sink)) = &mut self.selector_sink {
            sink.event(event)?;
            match event {
                ParseEvent::StartNode { .. } => *depth += 1,
                ParseEvent::FinishNode { .. } => *depth = depth.saturating_sub(1),
                ParseEvent::Token(_) | ParseEvent::Diagnostic(_) => {}
            }
            if *depth == 0 {
                let (_, sink) = self.selector_sink.take().expect("selector sink exists");
                let structure: SelectorStructure = sink.finish();
                if let Some(rule) = self
                    .open
                    .iter_mut()
                    .rev()
                    .find(|frame| frame.kind == SyntaxKind::QualifiedRule)
                {
                    rule.selector_list = Some(structure.list().clone());
                }
            }
        } else if matches!(
            event,
            ParseEvent::StartNode {
                kind: SyntaxKind::SelectorList,
                ..
            }
        ) {
            let mut sink = SelectorSink::new(self.source.clone());
            sink.event(event)?;
            self.selector_sink = Some((1, sink));
        }

        match event {
            ParseEvent::StartNode { kind, flags, start } => self.open.push(OpenFrame {
                kind,
                flags,
                start,
                token_start: self.tokens.len(),
                statements: Vec::new(),
                values: Vec::new(),
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
            ParseEvent::Diagnostic(diagnostic) => {
                self.diagnostics.push(diagnostic);
                if diagnostic.kind != crate::diagnostic::CssDiagnosticKind::AmbiguousStatement {
                    for frame in &mut self.open {
                        frame.recovered = true;
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn parse_style_ir(
    source: CssSource,
    dialect: CssDialect,
    mode: CssParseMode,
) -> Result<StyleSyntaxIr, CssParseFailure> {
    let mut sink = StyleSyntaxIrSink::new(source.clone(), dialect);
    parse_with_sink(&source, dialect, CssEntryPoint::Stylesheet, mode, &mut sink)?;
    sink.finish().map_err(CssParseFailure::Structure)
}

pub fn parse_component_value_tree(
    source: CssSource,
    dialect: CssDialect,
    mode: CssParseMode,
) -> Result<ComponentValueTree, CssParseFailure> {
    let mut sink = StyleSyntaxIrSink::new(source.clone(), dialect);
    parse_with_sink(
        &source,
        dialect,
        CssEntryPoint::ComponentValueList,
        mode,
        &mut sink,
    )?;
    Ok(sink
        .root_value_tree
        .unwrap_or_else(|| ComponentValueTree::empty(source.origin())))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::parser::{parse_with_sink, CssEntryPoint};
    use crate::selector::{SelectorCompleteness, SelectorSink};

    // @ai-generated - Pins pseudo-list descent to component trust, not enclosing selector completeness.
    #[test]
    fn recovered_pseudo_selector_keeps_disjoint_complete_class_fact() {
        let source = CssSource::new(Arc::from(":is(.a .b#{$x"), 0).unwrap();
        let mut sink = SelectorSink::new(source.clone());
        parse_with_sink(
            &source,
            CssDialect::Scss,
            CssEntryPoint::SelectorList,
            CssParseMode::Recover,
            &mut sink,
        )
        .unwrap();
        let structure = sink.finish();
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
