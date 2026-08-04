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
    Type,
    Class,
    DynamicClass,
    Id,
    Namespace,
    Attribute,
    Nesting,
    PseudoClass,
    PseudoElement,
    FunctionalPseudo,
    Interpolation,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorComponent {
    kind: SelectorComponentKind,
    full_span: Span,
    name_span: Option<Span>,
    attribute: Option<SelectorAttribute>,
    pseudo: Option<SelectorPseudo>,
    static_fragments: Vec<Span>,
    interpolations: Vec<SelectorInterpolation>,
    nested_components: Vec<SelectorComponent>,
}

impl SelectorComponent {
    #[inline]
    pub const fn kind(&self) -> SelectorComponentKind {
        self.kind
    }

    #[inline]
    pub const fn span(&self) -> Span {
        self.full_span
    }

    #[inline]
    pub const fn full_span(&self) -> Span {
        self.full_span
    }

    #[inline]
    pub const fn name_span(&self) -> Option<Span> {
        self.name_span
    }

    #[inline]
    pub fn static_fragments(&self) -> &[Span] {
        &self.static_fragments
    }

    #[inline]
    pub fn interpolations(&self) -> &[SelectorInterpolation] {
        &self.interpolations
    }

    #[inline]
    pub fn attribute(&self) -> Option<&SelectorAttribute> {
        self.attribute.as_ref()
    }

    #[inline]
    pub fn pseudo(&self) -> Option<&SelectorPseudo> {
        self.pseudo.as_ref()
    }

    pub fn nested_components(&self) -> &[SelectorComponent] {
        &self.nested_components
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorInterpolation {
    full_span: Span,
    payload_span: Span,
    complete: bool,
}

impl SelectorInterpolation {
    pub const fn full_span(&self) -> Span {
        self.full_span
    }

    pub const fn payload_span(&self) -> Span {
        self.payload_span
    }

    pub const fn is_complete(&self) -> bool {
        self.complete
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
    name_span: Option<Span>,
    value_span: Option<Span>,
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

    pub const fn name_span(&self) -> Option<Span> {
        self.name_span
    }

    pub const fn value_span(&self) -> Option<Span> {
        self.value_span
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorPseudo {
    span: Span,
    argument_span: Span,
    kind: PseudoFunctionKind,
    selector_count: u32,
    nth: Option<NthExpression>,
    selector_list: Option<Box<SelectorList>>,
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

    #[inline]
    pub fn selector_list(&self) -> Option<&SelectorList> {
        self.selector_list.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorCompleteness {
    Complete,
    Recovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorTrust {
    Static,
    DynamicSelector,
    EvaluationRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorFacts {
    trust: SelectorTrust,
    completeness: SelectorCompleteness,
}

impl SelectorFacts {
    pub const fn trust(self) -> SelectorTrust {
        self.trust
    }

    pub const fn completeness(self) -> SelectorCompleteness {
        self.completeness
    }

    pub const fn is_complete_static(self) -> bool {
        matches!(self.trust, SelectorTrust::Static)
            && matches!(self.completeness, SelectorCompleteness::Complete)
    }

    pub const fn is_dynamic(self) -> bool {
        !matches!(self.trust, SelectorTrust::Static)
    }

    fn combine(self, other: Self) -> Self {
        let trust = match (self.trust, other.trust) {
            (SelectorTrust::EvaluationRequired, _) | (_, SelectorTrust::EvaluationRequired) => {
                SelectorTrust::EvaluationRequired
            }
            (SelectorTrust::DynamicSelector, _) | (_, SelectorTrust::DynamicSelector) => {
                SelectorTrust::DynamicSelector
            }
            _ => SelectorTrust::Static,
        };
        let completeness = if matches!(
            (self.completeness, other.completeness),
            (
                SelectorCompleteness::Complete,
                SelectorCompleteness::Complete
            )
        ) {
            SelectorCompleteness::Complete
        } else {
            SelectorCompleteness::Recovered
        };
        Self {
            trust,
            completeness,
        }
    }
}

impl Default for SelectorFacts {
    fn default() -> Self {
        Self {
            trust: SelectorTrust::Static,
            completeness: SelectorCompleteness::Complete,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComplexSelectorPart {
    Compound(SelectorCompound),
    Combinator(SelectorCombinator),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorCompound {
    span: Span,
    components: Vec<SelectorComponent>,
    facts: SelectorFacts,
}

impl SelectorCompound {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn components(&self) -> &[SelectorComponent] {
        &self.components
    }

    pub const fn facts(&self) -> SelectorFacts {
        self.facts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexSelector {
    kind: SelectorKind,
    span: Span,
    parts: Vec<ComplexSelectorPart>,
    facts: SelectorFacts,
}

impl ComplexSelector {
    pub const fn kind(&self) -> SelectorKind {
        self.kind
    }

    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn parts(&self) -> &[ComplexSelectorPart] {
        &self.parts
    }

    pub fn compounds(&self) -> Vec<&SelectorCompound> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                ComplexSelectorPart::Compound(value) => Some(value),
                ComplexSelectorPart::Combinator(_) => None,
            })
            .collect()
    }

    pub fn combinators(&self) -> Vec<&SelectorCombinator> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                ComplexSelectorPart::Combinator(value) => Some(value),
                ComplexSelectorPart::Compound(_) => None,
            })
            .collect()
    }

    pub const fn facts(&self) -> SelectorFacts {
        self.facts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorList {
    span: Span,
    selectors: Vec<ComplexSelector>,
    facts: SelectorFacts,
}

impl SelectorList {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn selectors(&self) -> &[ComplexSelector] {
        &self.selectors
    }

    pub const fn facts(&self) -> SelectorFacts {
        self.facts
    }
}

pub struct SelectorStructure {
    source: CssSource,
    list: SelectorList,
}

impl SelectorStructure {
    pub const fn span(&self) -> Span {
        self.list.span
    }

    pub fn list(&self) -> &SelectorList {
        &self.list
    }

    pub fn top_level_selector_count(&self) -> u32 {
        u32::try_from(self.list.selectors.len()).unwrap_or(u32::MAX)
    }

    pub fn source(&self) -> &CssSource {
        &self.source
    }

    pub const fn facts(&self) -> SelectorFacts {
        self.list.facts
    }

    pub fn all_components(&self) -> Vec<&SelectorComponent> {
        let mut output = Vec::new();
        collect_components(&self.list, &mut output);
        output
    }

    pub fn components(&self) -> Vec<&SelectorComponent> {
        self.all_components()
    }

    pub fn all_combinators(&self) -> Vec<&SelectorCombinator> {
        let mut output = Vec::new();
        collect_combinators(&self.list, &mut output);
        output
    }

    pub fn combinators(&self) -> Vec<&SelectorCombinator> {
        self.all_combinators()
    }

    pub fn all_attributes(&self) -> Vec<&SelectorAttribute> {
        self.all_components()
            .into_iter()
            .filter_map(SelectorComponent::attribute)
            .collect()
    }

    pub fn attributes(&self) -> Vec<&SelectorAttribute> {
        self.all_attributes()
    }

    pub fn all_pseudos(&self) -> Vec<&SelectorPseudo> {
        self.all_components()
            .into_iter()
            .filter_map(SelectorComponent::pseudo)
            .collect()
    }

    pub fn pseudos(&self) -> Vec<&SelectorPseudo> {
        self.all_components()
            .into_iter()
            .filter(|component| component.kind == SelectorComponentKind::FunctionalPseudo)
            .filter_map(SelectorComponent::pseudo)
            .collect()
    }
}

fn collect_components<'a>(list: &'a SelectorList, output: &mut Vec<&'a SelectorComponent>) {
    for selector in &list.selectors {
        for part in &selector.parts {
            if let ComplexSelectorPart::Compound(compound) = part {
                for component in &compound.components {
                    output.push(component);
                    collect_nested_components(component, output);
                    if let Some(nested) = component
                        .pseudo
                        .as_ref()
                        .and_then(SelectorPseudo::selector_list)
                    {
                        collect_components(nested, output);
                    }
                }
            }
        }
    }
}

fn collect_nested_components<'a>(
    component: &'a SelectorComponent,
    output: &mut Vec<&'a SelectorComponent>,
) {
    for nested in &component.nested_components {
        output.push(nested);
        collect_nested_components(nested, output);
    }
}

fn collect_combinators<'a>(list: &'a SelectorList, output: &mut Vec<&'a SelectorCombinator>) {
    for selector in &list.selectors {
        for part in &selector.parts {
            match part {
                ComplexSelectorPart::Compound(compound) => {
                    for component in &compound.components {
                        if let Some(nested) = component
                            .pseudo
                            .as_ref()
                            .and_then(SelectorPseudo::selector_list)
                        {
                            collect_combinators(nested, output);
                        }
                    }
                }
                ComplexSelectorPart::Combinator(combinator) => output.push(combinator),
            }
        }
    }
}

#[derive(Debug)]
enum BuiltSelectorNode {
    List(SelectorList),
    Complex(ComplexSelector),
    Compound(SelectorCompound),
    Combinator(SelectorCombinator),
    Component(SelectorComponent),
    Container(Vec<BuiltSelectorNode>),
}

#[derive(Debug)]
struct OpenNode {
    kind: SyntaxKind,
    start: u32,
    token_start: usize,
    children: Vec<BuiltSelectorNode>,
    recovered: bool,
}

pub(crate) struct SelectorSink {
    source: CssSource,
    open: SmallVec<[OpenNode; 16]>,
    tokens: Vec<SyntaxToken>,
    list: Option<SelectorList>,
}

impl SelectorSink {
    pub(crate) fn new(source: CssSource) -> Self {
        Self {
            source,
            open: SmallVec::new(),
            tokens: Vec::new(),
            list: None,
        }
    }

    pub(crate) fn finish(self) -> SelectorStructure {
        let list = self.list.unwrap_or_else(|| SelectorList {
            span: Span::new(self.source.origin(), self.source.end()),
            selectors: Vec::new(),
            facts: SelectorFacts::default(),
        });
        SelectorStructure {
            source: self.source,
            list,
        }
    }

    fn build_node(&self, open: OpenNode, end: u32) -> Option<BuiltSelectorNode> {
        let span = Span::new(open.start, end);
        let tokens = &self.tokens[open.token_start..];
        let recovered = open.recovered;
        let recovered_facts = SelectorFacts {
            trust: SelectorTrust::Static,
            completeness: if recovered {
                SelectorCompleteness::Recovered
            } else {
                SelectorCompleteness::Complete
            },
        };
        match open.kind {
            SyntaxKind::SelectorList => {
                let selectors: Vec<_> = open
                    .children
                    .into_iter()
                    .filter_map(|child| match child {
                        BuiltSelectorNode::Complex(value) => Some(value),
                        _ => None,
                    })
                    .collect();
                let facts = selectors.iter().fold(recovered_facts, |facts, selector| {
                    facts.combine(selector.facts)
                });
                Some(BuiltSelectorNode::List(SelectorList {
                    span,
                    selectors,
                    facts,
                }))
            }
            SyntaxKind::Selector => {
                let parts: Vec<_> = open
                    .children
                    .into_iter()
                    .filter_map(|child| match child {
                        BuiltSelectorNode::Compound(value) => {
                            Some(ComplexSelectorPart::Compound(value))
                        }
                        BuiltSelectorNode::Combinator(value) => {
                            Some(ComplexSelectorPart::Combinator(value))
                        }
                        _ => None,
                    })
                    .collect();
                let facts = parts.iter().fold(recovered_facts, |facts, part| {
                    let child = match part {
                        ComplexSelectorPart::Compound(value) => value.facts,
                        ComplexSelectorPart::Combinator(_) => SelectorFacts::default(),
                    };
                    facts.combine(child)
                });
                Some(BuiltSelectorNode::Complex(ComplexSelector {
                    kind: SelectorKind::Complex,
                    span,
                    parts,
                    facts,
                }))
            }
            SyntaxKind::CompoundSelector => {
                let components: Vec<_> = open
                    .children
                    .into_iter()
                    .filter_map(|child| match child {
                        BuiltSelectorNode::Component(value) => Some(value),
                        _ => None,
                    })
                    .collect();
                let facts = components.iter().fold(recovered_facts, |facts, component| {
                    let trust = match component.kind {
                        SelectorComponentKind::DynamicClass => SelectorTrust::DynamicSelector,
                        SelectorComponentKind::Interpolation | SelectorComponentKind::Nesting => {
                            SelectorTrust::EvaluationRequired
                        }
                        _ => SelectorTrust::Static,
                    };
                    facts.combine(SelectorFacts {
                        trust,
                        completeness: SelectorCompleteness::Complete,
                    })
                });
                Some(BuiltSelectorNode::Compound(SelectorCompound {
                    span,
                    components,
                    facts,
                }))
            }
            SyntaxKind::Combinator => Some(BuiltSelectorNode::Combinator(SelectorCombinator {
                kind: combinator_kind(tokens, &self.source),
                span,
            })),
            kind if selector_component_kind(kind).is_some() => {
                let nested_components: Vec<_> = open
                    .children
                    .iter()
                    .filter_map(|child| match child {
                        BuiltSelectorNode::Component(component) => Some(component.clone()),
                        _ => None,
                    })
                    .collect();
                let mut interpolations: Vec<_> = nested_components
                    .iter()
                    .filter(|component| component.kind == SelectorComponentKind::Interpolation)
                    .filter_map(|component| component.interpolations.first().copied())
                    .collect();
                let mut component_kind = selector_component_kind(kind).unwrap();
                if kind == SyntaxKind::ClassSelector && !interpolations.is_empty() {
                    component_kind = SelectorComponentKind::DynamicClass;
                }
                let name_span = selector_name_span(component_kind, tokens);
                let static_fragments = if component_kind == SelectorComponentKind::DynamicClass {
                    tokens
                        .iter()
                        .filter(|token| {
                            token.kind() == TokenKind::Ident
                                && !interpolations.iter().any(|interpolation| {
                                    interpolation.full_span.start <= token.start
                                        && token.end <= interpolation.full_span.end
                                })
                        })
                        .map(|token| Span::new(token.start, token.end))
                        .collect()
                } else {
                    Vec::new()
                };
                let attribute =
                    (kind == SyntaxKind::AttributeSelector).then(|| SelectorAttribute {
                        span,
                        matcher: attribute_matcher(tokens, &self.source),
                        name_span: attribute_name_span(tokens),
                        value_span: attribute_value_span(tokens),
                    });
                let pseudo = matches!(
                    kind,
                    SyntaxKind::PseudoClass
                        | SyntaxKind::PseudoElement
                        | SyntaxKind::PseudoSelectorList
                        | SyntaxKind::NthSelector
                        | SyntaxKind::UnknownPseudoFunction
                )
                .then(|| {
                    let pseudo_kind = pseudo_kind(tokens, &self.source);
                    let selector_list = open.children.iter().find_map(|child| match child {
                        BuiltSelectorNode::List(value) => Some(Box::new(value.clone())),
                        _ => None,
                    });
                    let selector_count = selector_list.as_ref().map_or(0, |list| {
                        u32::try_from(list.selectors.len()).unwrap_or(u32::MAX)
                    });
                    let nth = matches!(
                        pseudo_kind,
                        PseudoFunctionKind::NthChild | PseudoFunctionKind::NthLastChild
                    )
                    .then(|| parse_an_plus_b_tokens(tokens, &self.source))
                    .flatten();
                    SelectorPseudo {
                        span,
                        argument_span: pseudo_argument_span(tokens, span),
                        kind: pseudo_kind,
                        selector_count,
                        nth,
                        selector_list,
                    }
                });
                if kind == SyntaxKind::Interpolation {
                    let opener_end = tokens.first().map_or(span.start, |token| token.end);
                    let closed = tokens
                        .last()
                        .is_some_and(|token| token.kind() == TokenKind::RightBrace);
                    let payload_end = if closed {
                        tokens.last().map_or(end, |token| token.start)
                    } else {
                        end
                    };
                    interpolations.push(SelectorInterpolation {
                        full_span: span,
                        payload_span: Span::new(opener_end, payload_end),
                        complete: closed && !recovered,
                    });
                }
                Some(BuiltSelectorNode::Component(SelectorComponent {
                    kind: component_kind,
                    full_span: span,
                    name_span,
                    attribute,
                    pseudo,
                    static_fragments,
                    interpolations,
                    nested_components,
                }))
            }
            _ if !open.children.is_empty() => Some(BuiltSelectorNode::Container(open.children)),
            _ => None,
        }
    }
}

impl ParseEventSink for SelectorSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        match event {
            ParseEvent::StartNode { kind, start, .. } => self.open.push(OpenNode {
                kind,
                start,
                token_start: self.tokens.len(),
                children: Vec::new(),
                recovered: false,
            }),
            ParseEvent::Token(token) => self.tokens.push(token),
            ParseEvent::FinishNode { kind, end } => {
                let open = self
                    .open
                    .pop()
                    .expect("parser emits balanced selector nodes");
                debug_assert_eq!(open.kind, kind);
                if let Some(built) = self.build_node(open, end) {
                    if let Some(parent) = self.open.last_mut() {
                        match built {
                            BuiltSelectorNode::Container(children) => {
                                parent.children.extend(children);
                            }
                            value => parent.children.push(value),
                        }
                    } else if let BuiltSelectorNode::List(list) = built {
                        self.list = Some(list);
                    }
                }
            }
            ParseEvent::Diagnostic(_) => {
                for open in &mut self.open {
                    open.recovered = true;
                }
            }
        }
        Ok(())
    }
}

fn selector_component_kind(kind: SyntaxKind) -> Option<SelectorComponentKind> {
    match kind {
        SyntaxKind::TypeSelector => Some(SelectorComponentKind::Type),
        SyntaxKind::ClassSelector => Some(SelectorComponentKind::Class),
        SyntaxKind::IdSelector => Some(SelectorComponentKind::Id),
        SyntaxKind::NamespaceSelector => Some(SelectorComponentKind::Namespace),
        SyntaxKind::AttributeSelector => Some(SelectorComponentKind::Attribute),
        SyntaxKind::NestingSelector => Some(SelectorComponentKind::Nesting),
        SyntaxKind::PseudoClass => Some(SelectorComponentKind::PseudoClass),
        SyntaxKind::PseudoElement => Some(SelectorComponentKind::PseudoElement),
        SyntaxKind::PseudoSelectorList
        | SyntaxKind::NthSelector
        | SyntaxKind::UnknownPseudoFunction => Some(SelectorComponentKind::FunctionalPseudo),
        SyntaxKind::Interpolation => Some(SelectorComponentKind::Interpolation),
        _ => None,
    }
}

fn selector_name_span(kind: SelectorComponentKind, tokens: &[SyntaxToken]) -> Option<Span> {
    match kind {
        SelectorComponentKind::Type | SelectorComponentKind::Class => tokens
            .iter()
            .find(|token| token.kind() == TokenKind::Ident)
            .map(|token| Span::new(token.start, token.end)),
        SelectorComponentKind::Id => tokens
            .iter()
            .find(|token| token.kind() == TokenKind::Hash)
            .map(|token| Span::new(token.start.saturating_add(1), token.end)),
        SelectorComponentKind::PseudoClass
        | SelectorComponentKind::PseudoElement
        | SelectorComponentKind::FunctionalPseudo => tokens
            .iter()
            .find(|token| matches!(token.kind(), TokenKind::Ident | TokenKind::Function))
            .map(|token| {
                let end = if token.kind() == TokenKind::Function {
                    token.end.saturating_sub(1)
                } else {
                    token.end
                };
                Span::new(token.start, end)
            }),
        _ => None,
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

fn attribute_name_span(tokens: &[SyntaxToken]) -> Option<Span> {
    let matcher = attribute_matcher_token_index(tokens);
    tokens[..matcher.unwrap_or(tokens.len())]
        .iter()
        .rev()
        .find(|token| token.kind() == TokenKind::Ident)
        .map(|token| Span::new(token.start, token.end))
}

fn attribute_value_span(tokens: &[SyntaxToken]) -> Option<Span> {
    let matcher = attribute_matcher_token_index(tokens)?;
    let token = tokens[matcher + 1..]
        .iter()
        .find(|token| !token.kind().is_trivia() && token.kind() != TokenKind::Delim)?;
    let mut span = Span::new(token.start, token.end);
    if token.kind() == TokenKind::String && token.end.saturating_sub(token.start) >= 2 {
        span.start = span.start.saturating_add(1);
        span.end = span.end.saturating_sub(1);
    }
    Some(span)
}

fn attribute_matcher_token_index(tokens: &[SyntaxToken]) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind() {
            TokenKind::Function | TokenKind::LeftParen | TokenKind::LeftBrace => depth += 1,
            TokenKind::RightParen | TokenKind::RightBrace if depth > 0 => depth -= 1,
            TokenKind::Delim if depth == 0 => {
                let next = tokens.get(index + 1);
                if next.is_some_and(|next| next.kind() == TokenKind::Delim) {
                    continue;
                }
                if next.is_some_and(|next| next.kind() != TokenKind::RightBracket) {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
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
