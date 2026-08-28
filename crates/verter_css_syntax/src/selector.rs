//! Selector IR.
//!
//! Public bump-backed nodes (`SelectorCompound`, `SelectorList`,
//! `SelectorComponent`, `SelectorPseudo`, `ComplexSelector`, …) are not
//! independently `Clone` or `Copy`. Borrow them from [`crate::StyleSyntaxIr`]
//! or [`SelectorStructure`].

use bumpalo::Bump;
use smallvec::SmallVec;
use verter_span::Span;

use crate::arena::{bump_for_source, freeze_vec, BumpSlice, FrozenBump};
use crate::diagnostic::{CssParseFailure, CssStructureTooLarge};
use crate::dialect::CssDialect;
use crate::event::{ParseEvent, ParseEventSink, SyntaxKind};
use crate::lexer::{codepoint_at, is_js_whitespace_codepoint};
use crate::parser::{parse_with_sink, CssEntryPoint, CssParseMode, CssSource};
use crate::style_ir::notify_parse_phase;
use crate::svelte_compat::{
    classify_argument_is_empty, classify_svelte_nth_arg, svelte_nth_of_selector_span,
    svelte_percentage_selector_span, svelte_trailing_type_selector_span,
    svelte_unclassified_expected_identifier,
};
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

/// Svelte-compat classification of a `:nth-child` / `:nth-last-child`
/// argument, decided ONCE at parse time from the already-delimited
/// [`SelectorPseudo::argument_span`]. Reject projection reads this fact
/// and never re-derives it from argument bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteNthArg {
    /// Empty argument (`:nth-child()`).
    Empty,
    /// Official `REGEX_NTH_OF` consumes the argument, or a prefix ending
    /// with `of` (the following selector is the nested `of` list).
    Formula,
    /// The entire argument is one complete Svelte-compat identifier.
    TrailingIdentifier,
    /// Argument starts with `-?\d` — official `read_identifier` reject.
    LeadingHyphenOrDigit,
    /// None of the above.
    Other,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SelectorComponent {
    kind: SelectorComponentKind,
    facts: SelectorFacts,
    full_span: Span,
    name_span: Option<Span>,
    attribute: Option<SelectorAttribute>,
    pseudo: Option<SelectorPseudo>,
    static_fragments: BumpSlice<Span>,
    interpolations: BumpSlice<SelectorInterpolation>,
    nested_components: BumpSlice<SelectorComponent>,
}

impl SelectorComponent {
    #[inline]
    pub const fn kind(&self) -> SelectorComponentKind {
        self.kind
    }

    #[inline]
    pub const fn facts(&self) -> SelectorFacts {
        self.facts
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
        self.static_fragments.as_slice()
    }

    #[inline]
    pub fn interpolations(&self) -> &[SelectorInterpolation] {
        self.interpolations.as_slice()
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
        self.nested_components.as_slice()
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
    value_quoted: bool,
    flags_span: Option<Span>,
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

    /// Whether the value token found by [`Self::value_span`] was a quoted
    /// CSS `String` token (`[attr="v"]`/`[attr='v']`) rather than an
    /// unquoted value token (`[attr=v]`) — a fact recorded at parse time
    /// from the token's own kind, never inferred downstream from the byte
    /// preceding the (already quote-stripped) value span. `false` when
    /// there is no value at all.
    #[inline]
    pub const fn value_was_quoted(&self) -> bool {
        self.value_quoted
    }

    /// The trailing case-sensitivity flags run (`i` / `s`), e.g.
    /// `[attr~="v" i]` — Svelte's `SimpleSelector::Attribute.flags`. `None`
    /// when the attribute selector carries no flags.
    pub const fn flags_span(&self) -> Option<Span> {
        self.flags_span
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SelectorPseudo {
    span: Span,
    argument_span: Span,
    kind: PseudoFunctionKind,
    selector_count: u32,
    nth: Option<NthExpression>,
    selector_list: Option<SelectorList>,
    /// Svelte-compat nth-argument shape. `Some` only for
    /// [`PseudoFunctionKind::NthChild`] / [`NthLastChild`](PseudoFunctionKind::NthLastChild).
    svelte_nth_arg: Option<SvelteNthArg>,
    /// Whether the argument is empty or trivia/comment-only. Minted at
    /// parse time for every pseudo; reject projection reads it for
    /// non-nth functional pseudos and never re-strips argument bytes.
    argument_is_empty: bool,
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
        self.selector_list.as_ref()
    }

    /// The parse-time Svelte nth-argument classification. `None` when this
    /// is not `:nth-child` / `:nth-last-child`.
    #[inline]
    pub const fn svelte_nth_arg(&self) -> Option<SvelteNthArg> {
        self.svelte_nth_arg
    }

    /// Whether the already-delimited argument is empty or trivia-only.
    #[inline]
    pub const fn argument_is_empty(&self) -> bool {
        self.argument_is_empty
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

#[derive(Debug, PartialEq, Eq)]
pub enum ComplexSelectorPart {
    Compound(SelectorCompound),
    Combinator(SelectorCombinator),
}

/// The classification of a [`SelectorCompound`]'s UNCLAIMED trailing byte
/// run — the compound's own span minus whatever bytes its recognized
/// [`components()`](SelectorCompound::components) claimed. The general CSS3
/// grammar this crate implements admits a handful of shapes Svelte's own
/// hand-rolled reader accepts leniently (a keyframe-step percentage, an
/// An+B pseudo-argument formula, a bare trailing type selector with no
/// combinator) by simply closing the compound without a typed component for
/// that run, leaving raw bytes unclassified. Decided ONCE, at parse time,
/// when the compound's own node is built (see [`SelectorCompound::tail`]) —
/// a consumer reads the stored fact and never re-derives it from source
/// bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompoundTail {
    /// No unclaimed trailing bytes — every byte in the compound's span
    /// belongs to a recognized component.
    #[default]
    Claimed,
    /// Zero recognized components; the WHOLE compound span is Svelte's
    /// keyframe-step percentage shape (`50%`).
    Percentage(Span),
    /// Zero recognized components; the WHOLE compound span is Svelte's
    /// lenient An+B pseudo-argument shape (e.g. `2n+1` as a bare pseudo
    /// argument, `:is(2n+1)`).
    NthOf(Span),
    /// At least one recognized component; the run after the LAST one is a
    /// single, complete bare identifier with no combinator — Svelte's
    /// lenient implicit-type-selector shape (the `div` in
    /// `:global(.x)div`).
    TrailingIdentifier(Span),
    /// Unclaimed trailing bytes are present but match none of the lenient
    /// shapes above.
    Unclassified {
        span: Span,
        /// Whether the run begins with a literal `.` — Svelte's
        /// malformed-class-selector shape (`.1bad`). Only meaningful when
        /// the compound has zero recognized components (the run then covers
        /// the whole compound span).
        starts_with_dot: bool,
        /// Whether Svelte's `read_identifier` would reject this run
        /// (`css_expected_identifier`): a leading `.`, a leading `-?\d`,
        /// or a delim `@`/`#` with no following name. Minted at parse
        /// time; reject projection reads this flag and never re-inspects
        /// the run's bytes.
        expected_identifier: bool,
    },
}

/// Classify `compound_span`'s unclaimed trailing byte run against Svelte's
/// lenient grammar-gap shapes — see [`CompoundTail`]. The SOLE place this
/// crate reads raw source bytes to decide a compound's grammar-gap shape;
/// [`SelectorSink::build_node`] calls this exactly once per compound, at
/// parse time, and stores the result on [`SelectorCompound::tail`].
///
/// A zero-component compound tries Svelte's keyframe-step percentage shape,
/// then its lenient An+B pseudo-argument shape, over the compound's WHOLE
/// span (see [`svelte_percentage_selector_span`] / [`svelte_nth_of_selector_span`]'s
/// docs — the general grammar leaves a `50%` or bare `2n+1` compound with
/// zero typed components, so the compound span is all that is available). A
/// compound with at least one component tries the trailing-identifier shape
/// over the run after its LAST component (see
/// [`svelte_trailing_type_selector_span`]'s doc: a type/universal selector
/// must be FIRST in a compound, so a trailing bare identifier like
/// `:global(.x)div`'s `div` never becomes a typed component).
fn classify_compound_tail(
    source: &CssSource,
    compound_span: Span,
    last_component: Option<&SelectorComponent>,
) -> CompoundTail {
    match last_component {
        None => {
            if compound_span.start >= compound_span.end {
                return CompoundTail::Claimed;
            }
            if let Some(matched) = svelte_percentage_selector_span(source, compound_span) {
                if matched == compound_span {
                    return CompoundTail::Percentage(matched);
                }
            }
            if let Some(matched) = svelte_nth_of_selector_span(source, compound_span) {
                if matched == compound_span {
                    return CompoundTail::NthOf(matched);
                }
            }
            let starts_with_dot = source.slice(compound_span).starts_with('.');
            CompoundTail::Unclassified {
                span: compound_span,
                starts_with_dot,
                expected_identifier: svelte_unclassified_expected_identifier(
                    source,
                    compound_span,
                    starts_with_dot,
                ),
            }
        }
        Some(last) => {
            let span = Span::new(last.span().end, compound_span.end);
            if span.start >= span.end {
                return CompoundTail::Claimed;
            }
            match svelte_trailing_type_selector_span(source, span) {
                Some(matched) => CompoundTail::TrailingIdentifier(matched),
                None => {
                    let starts_with_dot = false;
                    CompoundTail::Unclassified {
                        span,
                        starts_with_dot,
                        expected_identifier: svelte_unclassified_expected_identifier(
                            source,
                            span,
                            starts_with_dot,
                        ),
                    }
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SelectorCompound {
    span: Span,
    components: BumpSlice<SelectorComponent>,
    facts: SelectorFacts,
    tail: CompoundTail,
}

impl SelectorCompound {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn components(&self) -> &[SelectorComponent] {
        self.components.as_slice()
    }

    pub const fn facts(&self) -> SelectorFacts {
        self.facts
    }

    /// The compound's grammar-gap [`CompoundTail`] classification, decided
    /// once at parse time — see [`CompoundTail`]'s doc.
    pub const fn tail(&self) -> CompoundTail {
        self.tail
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ComplexSelector {
    kind: SelectorKind,
    span: Span,
    parts: BumpSlice<ComplexSelectorPart>,
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
        self.parts.as_slice()
    }

    pub fn compounds(&self) -> Vec<&SelectorCompound> {
        self.parts()
            .iter()
            .filter_map(|part| match part {
                ComplexSelectorPart::Compound(value) => Some(value),
                ComplexSelectorPart::Combinator(_) => None,
            })
            .collect()
    }

    pub fn combinators(&self) -> Vec<&SelectorCombinator> {
        self.parts()
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

    /// Parser-minted pairing of each compound with its leading combinator —
    /// the official `RelativeSelector` shape. Interleaved `parts()` stay the
    /// stored authority; this projection does not rescan source.
    #[must_use]
    pub fn relative_steps(&self) -> Vec<(Option<&SelectorCombinator>, &SelectorCompound)> {
        let mut steps = Vec::new();
        let mut pending: Option<&SelectorCombinator> = None;
        for part in self.parts() {
            match part {
                ComplexSelectorPart::Combinator(combinator) => pending = Some(combinator),
                ComplexSelectorPart::Compound(compound) => {
                    steps.push((pending.take(), compound));
                }
            }
        }
        steps
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SelectorList {
    span: Span,
    selectors: BumpSlice<ComplexSelector>,
    facts: SelectorFacts,
}

impl SelectorList {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub fn selectors(&self) -> &[ComplexSelector] {
        self.selectors.as_slice()
    }

    pub const fn facts(&self) -> SelectorFacts {
        self.facts
    }
}

pub struct SelectorStructure {
    source: CssSource,
    list: SelectorList,
    /// Frozen last so the bump outlives bump-backed child slices on drop.
    _bump: FrozenBump,
}

impl SelectorStructure {
    pub const fn span(&self) -> Span {
        self.list.span
    }

    pub fn list(&self) -> &SelectorList {
        &self.list
    }

    #[cfg(test)]
    pub(crate) fn from_parts(bump: Bump, source: CssSource, list: SelectorList) -> Self {
        Self {
            source,
            list,
            _bump: FrozenBump::freeze(bump),
        }
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
    for selector in list.selectors() {
        for part in selector.parts() {
            if let ComplexSelectorPart::Compound(compound) = part {
                for component in compound.components() {
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
    for nested in component.nested_components() {
        output.push(nested);
        collect_nested_components(nested, output);
    }
}

fn collect_combinators<'a>(list: &'a SelectorList, output: &mut Vec<&'a SelectorCombinator>) {
    for selector in list.selectors() {
        for part in selector.parts() {
            match part {
                ComplexSelectorPart::Compound(compound) => {
                    for component in compound.components() {
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

enum BuiltSelectorNode<'b> {
    List(SelectorList),
    Complex(ComplexSelector),
    Compound(SelectorCompound),
    Combinator(SelectorCombinator),
    Component(SelectorComponent),
    Container(bumpalo::collections::Vec<'b, BuiltSelectorNode<'b>>),
}

struct OpenNode<'b> {
    kind: SyntaxKind,
    start: u32,
    token_start: usize,
    children: bumpalo::collections::Vec<'b, BuiltSelectorNode<'b>>,
    recovered: bool,
}

pub(crate) struct SelectorSink<'b> {
    bump: &'b Bump,
    source: CssSource,
    open: SmallVec<[OpenNode<'b>; 16]>,
    tokens: bumpalo::collections::Vec<'b, SyntaxToken>,
    list: Option<SelectorList>,
}

impl<'b> SelectorSink<'b> {
    pub(crate) fn new(bump: &'b Bump, source: CssSource) -> Self {
        Self {
            bump,
            source,
            open: SmallVec::new(),
            tokens: bumpalo::collections::Vec::with_capacity_in(16, bump),
            list: None,
        }
    }

    pub(crate) fn finish_list(self) -> SelectorList {
        self.list.unwrap_or_else(|| SelectorList {
            span: Span::new(self.source.origin(), self.source.end()),
            selectors: BumpSlice::empty(),
            facts: SelectorFacts::default(),
        })
    }

    fn build_node(&self, open: OpenNode<'b>, end: u32) -> Option<BuiltSelectorNode<'b>> {
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
                let mut selectors = bumpalo::collections::Vec::new_in(self.bump);
                for child in open.children {
                    if let BuiltSelectorNode::Complex(value) = child {
                        selectors.push(value);
                    }
                }
                let facts = selectors.iter().fold(recovered_facts, |facts, selector| {
                    facts.combine(selector.facts)
                });
                Some(BuiltSelectorNode::List(SelectorList {
                    span,
                    selectors: freeze_vec(selectors),
                    facts,
                }))
            }
            SyntaxKind::Selector => {
                let mut parts = bumpalo::collections::Vec::new_in(self.bump);
                for child in open.children {
                    match child {
                        BuiltSelectorNode::Compound(value) => {
                            parts.push(ComplexSelectorPart::Compound(value));
                        }
                        BuiltSelectorNode::Combinator(value) => {
                            parts.push(ComplexSelectorPart::Combinator(value));
                        }
                        _ => {}
                    }
                }
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
                    parts: freeze_vec(parts),
                    facts,
                }))
            }
            SyntaxKind::CompoundSelector => {
                let mut components = bumpalo::collections::Vec::new_in(self.bump);
                for child in open.children {
                    if let BuiltSelectorNode::Component(value) = child {
                        components.push(value);
                    }
                }
                let facts = components.iter().fold(recovered_facts, |facts, component| {
                    facts.combine(component.facts)
                });
                let tail = classify_compound_tail(&self.source, span, components.last());
                Some(BuiltSelectorNode::Compound(SelectorCompound {
                    span,
                    components: freeze_vec(components),
                    facts,
                    tail,
                }))
            }
            SyntaxKind::Combinator => Some(BuiltSelectorNode::Combinator(SelectorCombinator {
                kind: combinator_kind(tokens, &self.source),
                span,
            })),
            kind if selector_component_kind(kind).is_some() => {
                let mut nested_components = bumpalo::collections::Vec::new_in(self.bump);
                let mut nested_list = None;
                for child in open.children {
                    match child {
                        BuiltSelectorNode::Component(component) => {
                            nested_components.push(component);
                        }
                        BuiltSelectorNode::List(value) if nested_list.is_none() => {
                            notify_parse_phase("selector_clone_enter");
                            nested_list = Some(value);
                            notify_parse_phase("selector_clone_exit");
                        }
                        _ => {}
                    }
                }
                let mut interpolations = bumpalo::collections::Vec::new_in(self.bump);
                for component in nested_components.iter() {
                    if component.kind == SelectorComponentKind::Interpolation {
                        if let Some(first) = component.interpolations.as_slice().first() {
                            interpolations.push(*first);
                        }
                    }
                }
                let mut component_kind = selector_component_kind(kind).unwrap();
                if kind == SyntaxKind::ClassSelector && !interpolations.is_empty() {
                    component_kind = SelectorComponentKind::DynamicClass;
                }
                let name_span = selector_name_span(component_kind, tokens);
                let static_fragments = if component_kind == SelectorComponentKind::DynamicClass {
                    let mut fragments = bumpalo::collections::Vec::new_in(self.bump);
                    for token in tokens {
                        if token.kind() == TokenKind::Ident
                            && !interpolations.iter().any(|interpolation| {
                                interpolation.full_span.start <= token.start
                                    && token.end <= interpolation.full_span.end
                            })
                        {
                            fragments.push(Span::new(token.start, token.end));
                        }
                    }
                    freeze_vec(fragments)
                } else {
                    BumpSlice::empty()
                };
                let attribute = (kind == SyntaxKind::AttributeSelector).then(|| {
                    let value_quoted = attribute_value_quoted(tokens);
                    let value_span = attribute_value_span(tokens).map(|value_span| {
                        if value_quoted {
                            value_span
                        } else {
                            truncate_unquoted_attribute_value(&self.source, value_span)
                        }
                    });
                    SelectorAttribute {
                        span,
                        matcher: attribute_matcher(tokens, &self.source),
                        name_span: attribute_name_span(tokens),
                        value_span,
                        value_quoted,
                        flags_span: attribute_flags_span(tokens),
                    }
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
                    let selector_count = nested_list.as_ref().map_or(0, |list| {
                        u32::try_from(list.selectors.len()).unwrap_or(u32::MAX)
                    });
                    let nth = matches!(
                        pseudo_kind,
                        PseudoFunctionKind::NthChild | PseudoFunctionKind::NthLastChild
                    )
                    .then(|| parse_an_plus_b_tokens(tokens, &self.source))
                    .flatten();
                    let argument_span = pseudo_argument_span(tokens, span);
                    let svelte_nth_arg = matches!(
                        pseudo_kind,
                        PseudoFunctionKind::NthChild | PseudoFunctionKind::NthLastChild
                    )
                    .then(|| classify_svelte_nth_arg(&self.source, argument_span));
                    let argument_is_empty = classify_argument_is_empty(
                        &self.source,
                        argument_span,
                        nested_list.as_ref(),
                    );
                    SelectorPseudo {
                        span,
                        argument_span,
                        kind: pseudo_kind,
                        selector_count,
                        nth,
                        selector_list: nested_list,
                        svelte_nth_arg,
                        argument_is_empty,
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
                let own_trust = match component_kind {
                    SelectorComponentKind::DynamicClass => SelectorTrust::DynamicSelector,
                    SelectorComponentKind::Interpolation | SelectorComponentKind::Nesting => {
                        SelectorTrust::EvaluationRequired
                    }
                    _ => SelectorTrust::Static,
                };
                let mut facts = SelectorFacts {
                    trust: own_trust,
                    completeness: if recovered {
                        SelectorCompleteness::Recovered
                    } else {
                        SelectorCompleteness::Complete
                    },
                };
                for nested in nested_components.iter() {
                    facts = facts.combine(nested.facts);
                }
                if let Some(list) = pseudo.as_ref().and_then(SelectorPseudo::selector_list) {
                    facts = facts.combine(list.facts);
                }
                Some(BuiltSelectorNode::Component(SelectorComponent {
                    kind: component_kind,
                    facts,
                    full_span: span,
                    name_span,
                    attribute,
                    pseudo,
                    static_fragments,
                    interpolations: freeze_vec(interpolations),
                    nested_components: freeze_vec(nested_components),
                }))
            }
            _ if !open.children.is_empty() => Some(BuiltSelectorNode::Container(open.children)),
            _ => None,
        }
    }
}

impl ParseEventSink for SelectorSink<'_> {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        match event {
            ParseEvent::StartNode { kind, start, .. } => self.open.push(OpenNode {
                kind,
                start,
                token_start: self.tokens.len(),
                children: bumpalo::collections::Vec::new_in(self.bump),
                recovered: false,
            }),
            ParseEvent::Token(token) => self.tokens.push(token),
            ParseEvent::FinishNode { kind, end } => {
                let open = self
                    .open
                    .pop()
                    .expect("parser emits balanced selector nodes");
                verter_debug_assert_eq!(open.kind, kind);
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

#[allow(dead_code)] // test parse facade; production has no caller
pub(crate) fn parse_selector_structure(
    source: &CssSource,
    dialect: CssDialect,
) -> Result<SelectorStructure, CssParseFailure> {
    #[cfg(any(test, feature = "test-support"))]
    SELECTOR_STRUCTURE_PARSE_INVOCATIONS.with(|count| count.set(count.get() + 1));
    let bump = bump_for_source(source.text().len());
    let list = {
        let mut sink = SelectorSink::new(&bump, source.clone());
        parse_with_sink(
            source,
            dialect,
            CssEntryPoint::SelectorList,
            CssParseMode::Strict,
            &mut sink,
        )?;
        sink.finish_list()
    };
    Ok(SelectorStructure {
        source: source.clone(),
        list,
        _bump: FrozenBump::freeze(bump),
    })
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static SELECTOR_STRUCTURE_PARSE_INVOCATIONS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

/// Per-thread count of [`parse_selector_structure`] executions. Test observability only.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn parse_selector_structure_thread_invocations() -> u64 {
    SELECTOR_STRUCTURE_PARSE_INVOCATIONS.with(std::cell::Cell::get)
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

/// Whether the SAME value token [`attribute_value_span`] locates is a quoted
/// CSS `String` token — the identical token-kind check that function already
/// performs to decide whether to strip quote bytes, captured as its own fact
/// so [`SelectorAttribute::value_was_quoted`] never needs a caller to infer
/// quoted-ness from raw source bytes.
fn attribute_value_quoted(tokens: &[SyntaxToken]) -> bool {
    let Some(matcher) = attribute_matcher_token_index(tokens) else {
        return false;
    };
    tokens[matcher + 1..]
        .iter()
        .find(|token| !token.kind().is_trivia() && token.kind() != TokenKind::Delim)
        .is_some_and(|token| token.kind() == TokenKind::String)
}

/// Svelte's `read_attribute_value` unquoted-value truncation
/// (`REGEX_CLOSING_BRACKET = /[\s\]]/`): stop at the first JS-whitespace
/// CODEPOINT (NBSP, U+00A0, included), never a byte scan. The general CSS
/// Syntax tokenizer treats NBSP as a valid non-ASCII name-continuation
/// codepoint, so an unquoted attribute value token can run past the point
/// Svelte's own lenient reader would have stopped (`[data-x=a<NBSP>b]` reads
/// the whole `a<NBSP>b` as one token, while Svelte truncates at the NBSP).
/// Applied here, at parse time, so [`SelectorAttribute::value_span`] already
/// carries the truncated span — a reader never re-scans it. A QUOTED value
/// is never truncated this way (the caller only applies this to an
/// UNQUOTED value token).
fn truncate_unquoted_attribute_value(source: &CssSource, span: Span) -> Span {
    let text = source.slice(span);
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while let Some((codepoint, width)) = codepoint_at(bytes, index) {
        if is_js_whitespace_codepoint(codepoint) {
            break;
        }
        index += width;
    }
    Span::new(
        span.start,
        span.start
            .saturating_add(u32::try_from(index).unwrap_or(u32::MAX)),
    )
}

/// The trailing case-sensitivity flags run (`i` / `s`, e.g. `[attr~="v" i]`)
/// — the official `read_attribute_flags` (`REGEX_ATTRIBUTE_FLAGS =
/// /[a-zA-Z]+/y`), read as ONE `Ident` token immediately after the value
/// token (when a matcher/value is present) or after the attribute name (when
/// it is not — upstream's reader runs `read_attribute_flags` unconditionally
/// after the optional matcher+value, so a value-less `[attr i]` still reads a
/// flags run). `None` when no such trailing identifier is present.
fn attribute_flags_span(tokens: &[SyntaxToken]) -> Option<Span> {
    let start_search = match attribute_matcher_token_index(tokens) {
        Some(matcher) => {
            let value_index = tokens[matcher + 1..]
                .iter()
                .position(|token| !token.kind().is_trivia() && token.kind() != TokenKind::Delim)
                .map(|offset| matcher + 1 + offset)?;
            value_index + 1
        }
        None => {
            let name_index = tokens
                .iter()
                .position(|token| token.kind() == TokenKind::Ident)?;
            name_index + 1
        }
    };
    let mut index = start_search;
    while tokens
        .get(index)
        .is_some_and(|token| token.kind().is_trivia())
    {
        index += 1;
    }
    let token = tokens.get(index)?;
    (token.kind() == TokenKind::Ident).then(|| Span::new(token.start, token.end))
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
