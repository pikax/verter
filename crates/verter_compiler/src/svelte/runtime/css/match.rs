//! The Svelte selector-to-template matcher — matches the official
//! `svelte@5.56.10` `phases/2-analyze/css/css-prune.js` (`prune`,
//! `apply_selector` BACKWARD, `apply_combinator`,
//! `relative_selector_might_apply_to_node`, `attribute_matches`,
//! `test_attribute`, and the DOM-neighborhood helpers) plus the
//! `phases/2-analyze/css/utils.js` `get_possible_values` class/attribute
//! value enumeration, walking the shared `verter_css_syntax` grammar via
//! [`match_relsel`]'s [`StepView`] algebra.
//!
//! ## Why there is no separate write-back pass
//!
//! The official JS algorithm mutates `metadata.used`/`metadata.scoped`
//! through shared object references (a spread-copied selector aliases the
//! same `metadata` object as its source). [`CompoundView::origin`] always
//! resolves back to the REAL [`SelectorCompound`] in the immutable source
//! tree (never a clone — a `:has(...)`-filtered or synthetic view either
//! carries the real node or none at all), so this matcher writes directly
//! through `analysis.mark_scoped(origin)`/`analysis.mark_used(complex)`
//! during the walk — semantics-preserving because `used`/`scoped` are never
//! READ mid-walk, only written.
//!
//! [`MatchSink`] carries what has no home on the CSS-domain
//! [`CssAnalysis`](super::analyze::CssAnalysis): [`NodeId`] scoped-element
//! membership and the observability-only certainty rows.

#[path = "match_attribute.rs"]
mod attribute;
#[path = "match_certainty.rs"]
mod certainty;
#[path = "match_index.rs"]
mod index;
#[path = "match_values.rs"]
mod values;

use rustc_hash::FxHashSet;
use verter_css_syntax::{
    CombinatorKind, ComplexSelector, SelectorComponentKind, SelectorPseudo, StyleDirective,
    StyleRule, StyleStatement, StyleSyntaxIr,
};
use verter_span::Span;

use index::{
    get_ancestor_elements, get_descendant_elements, get_element_parent,
    get_possible_element_siblings, Direction, Existence, TemplateIndex,
};
use values::expression_possible_values;

use super::analyze::{
    component_name, is_outer_global, is_pseudo_class_component, is_unscoped_pseudo_class,
    relative_steps, CssAnalysis,
};
use super::match_relsel::{self as relsel, CombinatorView, ComponentView, CompoundView, StepView};
use super::types::MatchedTemplateFacts;
use crate::svelte::runtime::ir::{AttrIr, MixedAttrPart, NodeId, SvelteRuntimeIr};
use attribute::{
    ends_with_js_whitespace, starts_with_js_whitespace, test_attribute, unescape_backslashes,
    unquote,
};
pub use certainty::MatchCertainty;

/// Run the official `prune(stylesheet, elements)` walk over the analyzed CSS
/// tree and the component's template, mutating `analysis`'s `used`/`scoped`
/// side-table facts directly (see the module doc for why no deferred
/// write-back pass is needed). On success the per-element scope facts are
/// returned; an unprovable construct returns the typed fail-closed
/// [`MatcherRefusal`] and leaves `analysis` PARTIALLY written — a matcher
/// refusal never constructs a plan at all, so a caller that discards
/// `analysis` on `Err` observes no partial state either way.
pub(crate) fn match_stylesheet<'ast>(
    source: &'ast str,
    tree: &'ast StyleSyntaxIr,
    analysis: &mut CssAnalysis,
    ir: &SvelteRuntimeIr<'_>,
) -> Result<MatchedTemplateFacts, MatcherRefusal> {
    let fallback_span = Span::new(tree.source().origin(), tree.source().end());
    let index = TemplateIndex::build(ir, fallback_span)?;
    let sink = {
        let mut matcher = Matcher {
            source,
            analysis,
            index: &index,
            sink: MatchSink::default(),
        };
        let mut chain: Vec<&StyleRule> = Vec::new();
        matcher.prune_statements(tree.statements(), &mut chain)?;
        matcher.sink
    };
    // Conformance observability: the complete matcher-fact set (per-selector
    // certainty, used/scoped spans, scoped element identities) recorded into
    // the active trace, if any. Compiled out without the feature; the closure
    // (and every allocation it makes) runs only under an active capture.
    #[cfg(feature = "conformance-trace")]
    crate::svelte::runtime::conformance_trace::record(|trace| {
        trace
            .style_matches
            .push(build_style_match_trace(tree, analysis, &sink, &index));
    });
    Ok(MatchedTemplateFacts {
        scoped: sink.scoped_elements,
    })
}

/// Project one match run into the conformance trace's fact set — the
/// used/scoped verdicts are read from the FINAL [`CssAnalysis`] state (the
/// side table the walk wrote them into); a synthetic step's `origin()` never
/// inserts an entry in the first place, so no sentinel-spanned sink set needs
/// filtering here (see the module doc). Deterministic orders (sorted spans /
/// node ids; the certainty rows keep prune visit order).
#[cfg(feature = "conformance-trace")]
fn build_style_match_trace(
    tree: &StyleSyntaxIr,
    analysis: &CssAnalysis,
    sink: &MatchSink,
    index: &TemplateIndex<'_, '_>,
) -> crate::svelte::runtime::conformance_trace::StyleMatchTrace {
    use crate::svelte::runtime::conformance_trace::{
        ScopedElementFact, SelectorCertaintyFact, StyleMatchTrace,
    };
    use crate::svelte::runtime::ir::IrNode;

    let mut used_selector_spans = Vec::new();
    let mut scoped_selector_spans = Vec::new();
    collect_used_and_scoped_spans(
        tree.statements(),
        analysis,
        &mut used_selector_spans,
        &mut scoped_selector_spans,
    );
    used_selector_spans.sort_unstable_by_key(|span: &Span| (span.start, span.end));
    scoped_selector_spans.sort_unstable_by_key(|span: &Span| (span.start, span.end));

    let mut scoped_nodes: Vec<NodeId> = sink.scoped_elements.iter().copied().collect();
    scoped_nodes.sort_unstable_by_key(|node| node.0);
    let scoped_elements = scoped_nodes
        .into_iter()
        .map(|node| {
            let span = match index.ir.node(node) {
                IrNode::Element(el) => el.span,
                IrNode::Special(sp) => sp.span,
                // Every `scoped_elements` write is a matchable element by
                // construction: the two sink insertion sites take NodeIds
                // from the `TemplateIndex.elements` inventory (populated
                // only for `IrNode::Element` and `SpecialKind::Element`)
                // and from `get_element_parent` hops (filtered through
                // `is_matchable_element`). The trace is CONFORMANCE
                // AUTHORITY, so an impossible node kind FAILS CLOSED here —
                // fabricating a placeholder span would manufacture typed
                // evidence.
                other => unreachable!(
                    "scoped node {node:?} is not a matchable element (found {other:?}) — \
                     the matcher only scopes template-index elements"
                ),
            };
            ScopedElementFact {
                node: node.0,
                tag: index.element_name(node).to_string(),
                span,
            }
        })
        .collect();
    StyleMatchTrace {
        selector_certainties: sink
            .selector_certainties
            .iter()
            .map(|&(selector_span, certainty)| SelectorCertaintyFact {
                selector_span,
                certainty,
            })
            .collect(),
        used_selector_spans,
        scoped_selector_spans,
        scoped_elements,
    }
}

/// Walk every selector list reachable from `statements` (rule preludes,
/// recursing into pseudo-class argument lists), collecting the spans of
/// every complex selector marked `used` and every compound marked `scoped`
/// in `analysis`.
#[cfg(feature = "conformance-trace")]
fn collect_used_and_scoped_spans(
    statements: &[StyleStatement],
    analysis: &CssAnalysis,
    used: &mut Vec<Span>,
    scoped: &mut Vec<Span>,
) {
    for statement in statements {
        match statement {
            StyleStatement::Rule(rule) => {
                collect_selector_list_spans(rule.selector_list(), analysis, used, scoped);
                collect_used_and_scoped_spans(rule.body().statements(), analysis, used, scoped);
            }
            StyleStatement::AtRule(atrule) => {
                if let Some(block) = atrule.body() {
                    collect_used_and_scoped_spans(block.statements(), analysis, used, scoped);
                }
            }
            StyleStatement::Declaration(_)
            | StyleStatement::MixinOrFunction(_)
            | StyleStatement::Unknown(_) => {}
        }
    }
}

#[cfg(feature = "conformance-trace")]
fn collect_selector_list_spans(
    list: &verter_css_syntax::SelectorList,
    analysis: &CssAnalysis,
    used: &mut Vec<Span>,
    scoped: &mut Vec<Span>,
) {
    for complex in list.selectors() {
        if analysis.complex_facts(complex).used {
            used.push(complex.span());
        }
        for (_, compound) in relative_steps(complex) {
            if analysis.compound_facts(compound).scoped {
                scoped.push(compound.span());
            }
            for component in compound.components() {
                if let Some(args) = component.pseudo().and_then(SelectorPseudo::selector_list) {
                    collect_selector_list_spans(args, analysis, used, scoped);
                }
            }
        }
    }
}

/// Type-witness anchor: `match_stylesheet` is nominally typed against the
/// shared `StyleSyntaxIr` + `CssAnalysis` — a re-parse-and-resolve shim with
/// the same runtime shape would fail to unify here.
const _: for<'ast> fn(
    &'ast str,
    &'ast StyleSyntaxIr,
    &mut CssAnalysis,
    &SvelteRuntimeIr<'_>,
) -> Result<MatchedTemplateFacts, MatcherRefusal> = match_stylesheet;

/// Test hook: run the prune walk and return the per-TOP-LEVEL-complex-selector
/// [`MatchCertainty`] rows (prune visit order, `No` rows included) WITHOUT
/// constructing [`MatchedTemplateFacts`] — the tri-state observability the
/// differential matcher tests pin.
#[cfg(test)]
pub(crate) fn match_stylesheet_certainties_for_test<'ast>(
    source: &'ast str,
    tree: &'ast StyleSyntaxIr,
    analysis: &mut CssAnalysis,
    ir: &SvelteRuntimeIr<'_>,
) -> Result<Vec<(Span, MatchCertainty)>, MatcherRefusal> {
    let index = TemplateIndex::build(ir, Span::new(tree.source().origin(), tree.source().end()))?;
    let mut matcher = Matcher {
        source,
        analysis,
        index: &index,
        sink: MatchSink::default(),
    };
    let mut chain: Vec<&StyleRule> = Vec::new();
    matcher.prune_statements(tree.statements(), &mut chain)?;
    Ok(matcher.sink.selector_certainties)
}

/// A template or selector construct the matcher cannot PROVE equivalent to
/// the official semantics — the typed fail-closed refusal of
/// [`match_stylesheet`]: no facts are published, and the caller refuses
/// emission on the style surface (never a guessed scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatcherRefusal {
    pub(crate) span: Span,
    pub(crate) construct: &'static str,
}

impl MatcherRefusal {
    fn at(span: Span, construct: &'static str) -> Self {
        Self { span, construct }
    }
}

type MatchResult<T> = Result<T, MatcherRefusal>;

/// The sentinel span synthetic selectors carry (the official `start: -1`
/// nodes) — used only for a refusal raised against a synthetic step (dead in
/// practice: a synthetic nesting step is only ever constructed at
/// `rule_idx >= 1`, so the one refusal site that could target it is
/// unreachable by construction, exactly as upstream).
fn synthetic_span() -> Span {
    Span::new(u32::MAX, u32::MAX)
}

/// The official `whitelist_attribute_selector` — attributes the runtime may
/// toggle on these elements, always treated as matching.
fn whitelisted_attributes(element_name_lower: &str) -> &'static [&'static str] {
    match element_name_lower {
        "details" | "dialog" => &["open"],
        _ => &[],
    }
}

/// The official `case_insensitive_attributes` set — HTML attributes whose
/// enumerated values match case-insensitively.
const CASE_INSENSITIVE_ATTRIBUTES: &[&str] = &[
    "accept-charset",
    "autocapitalize",
    "autocomplete",
    "behavior",
    "charset",
    "crossorigin",
    "decoding",
    "dir",
    "direction",
    "draggable",
    "enctype",
    "enterkeyhint",
    "fetchpriority",
    "formenctype",
    "formmethod",
    "formtarget",
    "hidden",
    "http-equiv",
    "inputmode",
    "kind",
    "loading",
    "method",
    "preload",
    "referrerpolicy",
    "rel",
    "rev",
    "role",
    "rules",
    "scope",
    "shape",
    "spellcheck",
    "target",
    "translate",
    "type",
    "valign",
    "wrap",
];

/// The deferred/observability state a match run still needs beyond direct
/// [`CssAnalysis`] mutation (see the module doc): the per-element scope facts
/// (no CSS-domain home) and the certainty rows (test/trace observability
/// only).
#[derive(Default)]
struct MatchSink {
    scoped_elements: FxHashSet<NodeId>,
    #[cfg(any(test, feature = "conformance-trace"))]
    selector_certainties: Vec<(Span, MatchCertainty)>,
}

/// The matcher state: the source text, the mutable analysis side table, the
/// template index, and the sink.
struct Matcher<'ast, 'm, 'i, 'ir, 'src> {
    /// The FULL original component source (matching the absolute offsets
    /// every span in `tree` carries) — raw `source[span.start..span.end]`
    /// indexing (every `analyze::component_name` call, and this module's own
    /// attribute-span slices) requires the untrimmed full text.
    source: &'ast str,
    analysis: &'m mut CssAnalysis,
    index: &'i TemplateIndex<'ir, 'src>,
    sink: MatchSink,
}

impl<'ast> Matcher<'ast, '_, '_, '_, '_> {
    fn source_text(&self) -> &'ast str {
        self.source
    }

    fn truncate(&self, complex: &'ast ComplexSelector) -> Vec<StepView<'ast>> {
        relsel::truncate(self.source, self.analysis, complex)
    }

    fn get_relative_selectors(
        &self,
        complex: &'ast ComplexSelector,
        rule_idx: usize,
    ) -> Vec<StepView<'ast>> {
        relsel::get_relative_selectors(self.source, self.analysis, complex, rule_idx)
    }

    /// The official `prune` walk over rule-position statements: a
    /// global-block rule visits only its prelude; every other rule visits
    /// prelude + block; declarations carry no analysis.
    fn prune_statements(
        &mut self,
        statements: &'ast [StyleStatement],
        chain: &mut Vec<&'ast StyleRule>,
    ) -> MatchResult<()> {
        for statement in statements {
            match statement {
                StyleStatement::Rule(rule) => self.prune_rule(rule, chain)?,
                StyleStatement::AtRule(atrule) => self.prune_atrule(atrule, chain)?,
                StyleStatement::Declaration(_)
                | StyleStatement::MixinOrFunction(_)
                | StyleStatement::Unknown(_) => {}
            }
        }
        Ok(())
    }

    fn prune_atrule(
        &mut self,
        atrule: &'ast StyleDirective,
        chain: &mut Vec<&'ast StyleRule>,
    ) -> MatchResult<()> {
        if let Some(block) = atrule.body() {
            self.prune_statements(block.statements(), chain)?;
        }
        Ok(())
    }

    fn prune_rule(
        &mut self,
        rule: &'ast StyleRule,
        chain: &mut Vec<&'ast StyleRule>,
    ) -> MatchResult<()> {
        chain.push(rule);
        let result = (|| {
            for complex in rule.selector_list().selectors() {
                self.prune_complex_selector(complex, chain)?;
            }
            // `Rule(node, context)`: a global block visits only its prelude.
            if !self.analysis.rule_facts(rule).is_global_block {
                self.prune_statements(rule.body().statements(), chain)?;
            }
            Ok(())
        })();
        chain.pop();
        result
    }

    /// The official `ComplexSelector` visitor: apply the selector (BACKWARD)
    /// against every template element.
    fn prune_complex_selector(
        &mut self,
        complex: &'ast ComplexSelector,
        chain: &[&'ast StyleRule],
    ) -> MatchResult<()> {
        let rule_idx = chain.len() - 1;
        let selectors = self.get_relative_selectors(complex, rule_idx);
        let elements = self.index.elements.clone();
        #[cfg(any(test, feature = "conformance-trace"))]
        let mut aggregate = MatchCertainty::No;
        for element in elements {
            let matched = self.apply_selector(
                &selectors,
                chain,
                rule_idx,
                element,
                Direction::Backward,
                0,
                selectors.len(),
            )?;
            if matched.might_match() {
                self.analysis.mark_used(complex);
            }
            #[cfg(any(test, feature = "conformance-trace"))]
            {
                aggregate = aggregate.or(matched);
            }
        }
        #[cfg(any(test, feature = "conformance-trace"))]
        self.sink
            .selector_certainties
            .push((complex.span(), aggregate));
        Ok(())
    }

    /// The official `apply_selector(relative_selectors, rule, element,
    /// direction, from, to)`.
    #[allow(clippy::too_many_arguments)]
    fn apply_selector(
        &mut self,
        relative_selectors: &[StepView<'ast>],
        chain: &[&'ast StyleRule],
        rule_idx: usize,
        element: NodeId,
        direction: Direction,
        from: usize,
        to: usize,
    ) -> MatchResult<MatchCertainty> {
        if from >= to {
            return Ok(MatchCertainty::No);
        }
        let selector_index = match direction {
            Direction::Forward => from,
            Direction::Backward => to - 1,
        };
        let relative_selector = relative_selectors[selector_index];
        let (rest_from, rest_to) = match direction {
            Direction::Forward => (from + 1, to),
            Direction::Backward => (from, to - 1),
        };

        let compound = self.relative_selector_might_apply_to_node(
            relative_selector.compound,
            chain,
            rule_idx,
            element,
            direction,
        )?;
        let matched = if compound == MatchCertainty::No {
            MatchCertainty::No
        } else {
            compound.and(self.apply_combinator(
                &relative_selector,
                relative_selectors,
                rest_from,
                rest_to,
                chain,
                rule_idx,
                element,
                direction,
            )?)
        };

        if matched.might_match() {
            if let Some(origin) = relative_selector.compound.origin() {
                if !is_outer_global(self.source_text(), origin) {
                    self.analysis.mark_scoped(origin);
                }
            }
            self.sink.scoped_elements.insert(element);
        }

        Ok(matched)
    }

    /// The official `apply_combinator`.
    #[allow(clippy::too_many_arguments)]
    fn apply_combinator(
        &mut self,
        relative_selector: &StepView<'ast>,
        relative_selectors: &[StepView<'ast>],
        from: usize,
        to: usize,
        chain: &[&'ast StyleRule],
        rule_idx: usize,
        node: NodeId,
        direction: Direction,
    ) -> MatchResult<MatchCertainty> {
        let combinator_kind = match direction {
            Direction::Forward => {
                if from < to {
                    relative_selectors[from].combinator.kind()
                } else {
                    None
                }
            }
            Direction::Backward => relative_selector.combinator.kind(),
        };
        let Some(kind) = combinator_kind else {
            return Ok(MatchCertainty::Yes);
        };

        match kind {
            CombinatorKind::Descendant | CombinatorKind::Child => {
                let is_adjacent = kind == CombinatorKind::Child;
                let parents = match direction {
                    Direction::Forward => get_descendant_elements(self.index, node, is_adjacent),
                    Direction::Backward => {
                        let mut seen: FxHashSet<NodeId> = FxHashSet::default();
                        get_ancestor_elements(self.index, node, is_adjacent, &mut seen)
                    }
                };
                let mut parent_matched = MatchCertainty::No;
                for &parent in &parents {
                    parent_matched = parent_matched.or(self.apply_selector(
                        relative_selectors,
                        chain,
                        rule_idx,
                        parent,
                        direction,
                        from,
                        to,
                    )?);
                }
                if parent_matched.might_match() {
                    return Ok(parent_matched);
                }
                if direction == Direction::Backward
                    && (!is_adjacent || parents.is_empty())
                    && self.every_is_global(relative_selectors, chain, rule_idx, from, to)?
                {
                    return Ok(MatchCertainty::Yes);
                }
                Ok(MatchCertainty::No)
            }
            CombinatorKind::NextSibling | CombinatorKind::LaterSibling => {
                let is_next = kind == CombinatorKind::NextSibling;
                let mut seen: FxHashSet<NodeId> = FxHashSet::default();
                let siblings =
                    get_possible_element_siblings(self.index, node, direction, is_next, &mut seen);
                let mut sibling_matched = MatchCertainty::No;
                for (possible_sibling, existence) in siblings.entries() {
                    let branch = if self.index.is_render_tag(possible_sibling)
                        || self.index.is_component_node(possible_sibling)
                        || self.index.is_slot_node(possible_sibling)
                    {
                        let from_is_global = (to - from == 1)
                            && relative_selectors[from]
                                .compound
                                .origin()
                                .is_some_and(|c| self.analysis.compound_facts(c).is_global);
                        if from_is_global {
                            MatchCertainty::Maybe
                        } else {
                            MatchCertainty::No
                        }
                    } else {
                        let applied = self.apply_selector(
                            relative_selectors,
                            chain,
                            rule_idx,
                            possible_sibling,
                            direction,
                            from,
                            to,
                        )?;
                        match existence {
                            Existence::Definitely => applied,
                            Existence::Probably => applied.and(MatchCertainty::Maybe),
                        }
                    };
                    sibling_matched = sibling_matched.or(branch);
                }
                if sibling_matched.might_match() {
                    return Ok(sibling_matched);
                }
                if direction == Direction::Backward
                    && get_element_parent(self.index, node).is_none()
                    && self.every_is_global(relative_selectors, chain, rule_idx, from, to)?
                {
                    return Ok(MatchCertainty::Yes);
                }
                Ok(MatchCertainty::No)
            }
            // `||` — an unevaluated acceptance is a fail-open `Maybe`.
            CombinatorKind::Column => Ok(MatchCertainty::Maybe),
        }
    }

    /// The official `every_is_global(relative_selectors, from, to, rule)`.
    fn every_is_global(
        &mut self,
        relative_selectors: &[StepView<'ast>],
        chain: &[&'ast StyleRule],
        rule_idx: usize,
        from: usize,
        to: usize,
    ) -> MatchResult<bool> {
        for step in &relative_selectors[from..to] {
            if !self.is_global_with_rule(step.compound, chain, rule_idx)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The official css-prune-local `is_global(selector, rule)` — a
    /// `:global(...)`/unscopeable compound, or an `:is(...)`/`:where(...)`
    /// (or nesting reference) whose selector list contains a fully-global
    /// complex selector.
    fn is_global_with_rule(
        &mut self,
        compound: CompoundView<'ast>,
        chain: &[&'ast StyleRule],
        rule_idx: usize,
    ) -> MatchResult<bool> {
        if let Some(origin) = compound.origin() {
            let facts = self.analysis.compound_facts(origin);
            if facts.is_global || facts.is_global_like {
                return Ok(true);
            }
        }

        let mut explicitly_global = false;
        let components = compound.components(self.source, self.analysis);

        for component in &components {
            let mut selector_list: Option<(&'ast verter_css_syntax::SelectorList, usize)> = None;
            let mut can_be_global = false;

            if let ComponentView::Real(real) = component {
                if is_pseudo_class_component(real) {
                    let name = component_name(self.source_text(), real);
                    if matches!(name.as_deref(), Some("is") | Some("where")) {
                        if let Some(list) = real.pseudo().and_then(SelectorPseudo::selector_list) {
                            selector_list = Some((list, rule_idx));
                        }
                    } else {
                        can_be_global = is_unscoped_pseudo_class(self.source_text(), real);
                    }
                }
            }

            let is_nesting = matches!(component, ComponentView::SyntheticNesting)
                || matches!(component, ComponentView::Real(real) if real.kind() == SelectorComponentKind::Nesting);
            if is_nesting {
                if rule_idx == 0 {
                    let span = match component {
                        ComponentView::Real(real) => real.span(),
                        _ => synthetic_span(),
                    };
                    return Err(MatcherRefusal::at(
                        span,
                        "a nesting selector without a parent rule",
                    ));
                }
                let owner_idx = rule_idx - 1;
                selector_list = Some((chain[owner_idx].selector_list(), owner_idx));
            }

            let has_global_selectors = match selector_list {
                None => false,
                Some((list, owner_idx)) => {
                    let mut any = false;
                    for complex in list.selectors() {
                        let steps = relative_steps(complex);
                        let mut all = !steps.is_empty();
                        for (_, inner_compound) in &steps {
                            if !self.is_global_with_rule(
                                CompoundView::Parsed(inner_compound),
                                chain,
                                owner_idx,
                            )? {
                                all = false;
                                break;
                            }
                        }
                        if all {
                            any = true;
                            break;
                        }
                    }
                    any
                }
            };
            explicitly_global |= has_global_selectors;

            if !has_global_selectors && !can_be_global {
                return Ok(false);
            }
        }

        Ok(explicitly_global || components.is_empty())
    }

    /// The official `relative_selector_might_apply_to_node`.
    fn relative_selector_might_apply_to_node(
        &mut self,
        compound: CompoundView<'ast>,
        chain: &[&'ast StyleRule],
        rule_idx: usize,
        element: NodeId,
        direction: Direction,
    ) -> MatchResult<MatchCertainty> {
        let mut include_self: Option<bool> = None;
        let mut certainty = MatchCertainty::Yes;

        let components = compound.components(self.source, self.analysis);

        for component in &components {
            // `:has(...)` — treat `.x:has(.y)` like `.x .y`, walking FORWARD.
            if let ComponentView::Real(real) = component {
                if is_pseudo_class_component(real) {
                    let name = component_name(self.source_text(), real);
                    if name.as_deref() == Some("has") {
                        if let Some(args) = real.pseudo().and_then(SelectorPseudo::selector_list) {
                            if include_self.is_none() {
                                include_self =
                                    Some(self.compute_has_include_self(chain, rule_idx)?);
                            }
                            let include = include_self == Some(true);

                            let mut matched = MatchCertainty::No;
                            for complex_selector in args.selectors() {
                                let truncated = self.truncate(complex_selector);
                                let Some((first, rest)) = truncated.split_first() else {
                                    self.analysis.mark_used(complex_selector);
                                    matched = MatchCertainty::Yes;
                                    continue;
                                };

                                if include {
                                    let including_self_combinator = CombinatorView::None;
                                    let mut selector_including_self: Vec<StepView<'ast>> =
                                        Vec::with_capacity(truncated.len());
                                    selector_including_self.push(StepView {
                                        combinator: including_self_combinator,
                                        compound: first.compound,
                                    });
                                    selector_including_self.extend_from_slice(rest);
                                    let applied = self.apply_selector(
                                        &selector_including_self,
                                        chain,
                                        rule_idx,
                                        element,
                                        Direction::Forward,
                                        0,
                                        selector_including_self.len(),
                                    )?;
                                    if applied.might_match() {
                                        self.analysis.mark_used(complex_selector);
                                        matched = matched.or(applied);
                                    }
                                }

                                let excluding_self_first_combinator = match first.combinator {
                                    CombinatorView::None => CombinatorView::SyntheticDescendant,
                                    other => other,
                                };
                                let mut selector_excluding_self: Vec<StepView<'ast>> =
                                    Vec::with_capacity(truncated.len() + 1);
                                selector_excluding_self.push(StepView {
                                    combinator: CombinatorView::None,
                                    compound: CompoundView::SyntheticAny,
                                });
                                selector_excluding_self.push(StepView {
                                    combinator: excluding_self_first_combinator,
                                    compound: first.compound,
                                });
                                selector_excluding_self.extend_from_slice(rest);
                                let applied = self.apply_selector(
                                    &selector_excluding_self,
                                    chain,
                                    rule_idx,
                                    element,
                                    Direction::Forward,
                                    0,
                                    selector_excluding_self.len(),
                                )?;
                                if applied.might_match() {
                                    self.analysis.mark_used(complex_selector);
                                    matched = matched.or(applied);
                                }
                            }

                            if matched == MatchCertainty::No {
                                return Ok(MatchCertainty::No);
                            }
                            certainty = certainty.and(matched);
                            continue;
                        }
                    }
                }
            }

            match component {
                ComponentView::Percentage(_) => continue,
                ComponentView::Nth(_) => {
                    certainty = certainty.and(MatchCertainty::Maybe);
                    continue;
                }
                _ => {}
            }

            match component {
                ComponentView::Real(real) => match real.kind() {
                    SelectorComponentKind::PseudoClass
                    | SelectorComponentKind::FunctionalPseudo => {
                        let raw_name = component_name(self.source_text(), real).unwrap_or_default();
                        let name = unescape_backslashes(&raw_name);
                        if name == "host" || name == "root" {
                            return Ok(MatchCertainty::No);
                        }

                        let args = real.pseudo().and_then(SelectorPseudo::selector_list);

                        if let Some(args) =
                            args.filter(|_| name == "global" && components.len() == 1)
                        {
                            let Some(complex_selector) = args.selectors().first() else {
                                return Ok(MatchCertainty::Yes);
                            };
                            let views: Vec<StepView<'ast>> = relative_steps(complex_selector)
                                .into_iter()
                                .map(|(combinator, inner_compound)| StepView {
                                    combinator: combinator
                                        .map_or(CombinatorView::None, CombinatorView::Parsed),
                                    compound: CompoundView::Parsed(inner_compound),
                                })
                                .collect();
                            return self.apply_selector(
                                &views,
                                chain,
                                rule_idx,
                                element,
                                Direction::Backward,
                                0,
                                views.len(),
                            );
                        }

                        if name == "global" && args.is_none() {
                            return Ok(MatchCertainty::Yes);
                        }

                        if name == "not" {
                            if let Some(args) = args {
                                for complex_selector in args.selectors() {
                                    self.mark_complex_used_recursive(complex_selector);
                                    let truncated = self.truncate(complex_selector);

                                    if relative_steps(complex_selector).len() > 1 {
                                        for step in &truncated {
                                            if let Some(origin) = step.compound.origin() {
                                                self.analysis.mark_scoped(origin);
                                            }
                                        }
                                        let mut el = Some(element);
                                        while let Some(current) = el {
                                            self.sink.scoped_elements.insert(current);
                                            el = get_element_parent(self.index, current);
                                        }
                                    }
                                }
                            }
                            certainty = certainty.and(MatchCertainty::Maybe);
                            continue;
                        }

                        if let Some(args) = args.filter(|_| name == "is" || name == "where") {
                            let mut matched = MatchCertainty::No;

                            for complex_selector in args.selectors() {
                                let truncated = self.truncate(complex_selector);

                                if truncated.is_empty() {
                                    self.analysis.mark_used(complex_selector);
                                    matched = MatchCertainty::Yes;
                                } else {
                                    let applied = self.apply_selector(
                                        &truncated,
                                        chain,
                                        rule_idx,
                                        element,
                                        Direction::Backward,
                                        0,
                                        truncated.len(),
                                    )?;
                                    if applied.might_match() {
                                        self.analysis.mark_used(complex_selector);
                                        matched = matched.or(applied);
                                    } else if relative_steps(complex_selector).len() > 1 {
                                        self.analysis.mark_used(complex_selector);
                                        matched = matched.or(MatchCertainty::Maybe);
                                        for step in &truncated {
                                            if let Some(origin) = step.compound.origin() {
                                                self.analysis.mark_scoped(origin);
                                            }
                                        }
                                    }
                                }
                            }

                            if matched == MatchCertainty::No {
                                return Ok(MatchCertainty::No);
                            }
                            certainty = certainty.and(matched);
                        }
                    }

                    SelectorComponentKind::PseudoElement | SelectorComponentKind::Namespace => {}

                    SelectorComponentKind::Attribute => {
                        let Some(attr) = real.attribute() else {
                            continue;
                        };
                        let attr_name = attr
                            .name_span()
                            .and_then(|span| {
                                verter_css_syntax::decode_css_identifier(
                                    &self.source_text()[span.start as usize..span.end as usize],
                                )
                                .ok()
                            })
                            .unwrap_or_default()
                            .into_owned();
                        let matcher_str = attr.matcher().map(attribute_matcher_str);
                        // The parser already truncated an unquoted value span
                        // at Svelte's own lenient boundary (see
                        // `SelectorAttribute::value_span`'s doc) — a straight
                        // slice, never a second boundary search here.
                        let value = attr.value_span().map(|span| {
                            &self.source_text()[span.start as usize..span.end as usize]
                        });
                        let flags = attr
                            .flags_span()
                            .map(|span| &self.source_text()[span.start as usize..span.end as usize])
                            .unwrap_or("");

                        let element_name_lower = self.index.element_name(element).to_lowercase();
                        let whitelisted = whitelisted_attributes(&element_name_lower)
                            .contains(&attr_name.to_lowercase().as_str());
                        let case_insensitive = flags.contains('i')
                            || (!flags.contains('s')
                                && CASE_INSENSITIVE_ATTRIBUTES
                                    .contains(&attr_name.to_lowercase().as_str()));
                        if whitelisted {
                            certainty = certainty.and(MatchCertainty::Maybe);
                        } else {
                            let attr_result = self.attribute_matches(
                                element,
                                &attr_name,
                                value.map(unquote),
                                matcher_str,
                                case_insensitive,
                            )?;
                            if attr_result == MatchCertainty::No {
                                return Ok(MatchCertainty::No);
                            }
                            certainty = certainty.and(attr_result);
                        }
                    }

                    SelectorComponentKind::Class => {
                        let raw_name = component_name(self.source_text(), real).unwrap_or_default();
                        let name = unescape_backslashes(&raw_name);
                        let attr = self.attribute_matches(
                            element,
                            "class",
                            Some(&name),
                            Some("~="),
                            false,
                        )?;
                        if attr == MatchCertainty::No {
                            return Ok(MatchCertainty::No);
                        }
                        certainty = certainty.and(attr);
                    }

                    SelectorComponentKind::Id => {
                        let raw_name = component_name(self.source_text(), real).unwrap_or_default();
                        let name = unescape_backslashes(&raw_name);
                        let attr =
                            self.attribute_matches(element, "id", Some(&name), Some("="), false)?;
                        if attr == MatchCertainty::No {
                            return Ok(MatchCertainty::No);
                        }
                        certainty = certainty.and(attr);
                    }

                    SelectorComponentKind::Type => {
                        let raw_name = type_selector_name(self.source_text(), real);
                        let name = unescape_backslashes(&raw_name);
                        if self.index.element_name(element).to_lowercase() != name.to_lowercase()
                            && name != "*"
                        {
                            if !self.index.is_svelte_element(element) {
                                return Ok(MatchCertainty::No);
                            }
                            certainty = certainty.and(MatchCertainty::Maybe);
                        }
                    }

                    SelectorComponentKind::Nesting => {
                        let matched =
                            self.handle_nesting(real.span(), chain, rule_idx, element, direction)?;
                        if matched == MatchCertainty::No {
                            return Ok(MatchCertainty::No);
                        }
                        certainty = certainty.and(matched);
                    }

                    SelectorComponentKind::DynamicClass | SelectorComponentKind::Interpolation => {
                        // Rejected upstream of the matcher today (the
                        // admission gate never trusts a dynamic selector into
                        // analysis/match); fail closed rather than guess if
                        // that invariant is ever violated.
                        return Err(MatcherRefusal::at(
                            real.span(),
                            "an unsupported dynamic selector construct",
                        ));
                    }
                },

                ComponentView::SyntheticAnyType => {}

                ComponentView::SyntheticNesting => {
                    let matched =
                        self.handle_nesting(synthetic_span(), chain, rule_idx, element, direction)?;
                    if matched == MatchCertainty::No {
                        return Ok(MatchCertainty::No);
                    }
                    certainty = certainty.and(matched);
                }

                ComponentView::Percentage(_) | ComponentView::Nth(_) => {
                    unreachable!("percentage/nth were skipped above")
                }
            }
        }

        Ok(certainty)
    }

    /// The official `SimpleSelector::Nesting` handling shared by a real
    /// nesting component and the synthetic `&` marker.
    fn handle_nesting(
        &mut self,
        span_for_refusal: Span,
        chain: &[&'ast StyleRule],
        rule_idx: usize,
        element: NodeId,
        direction: Direction,
    ) -> MatchResult<MatchCertainty> {
        if rule_idx == 0 {
            return Err(MatcherRefusal::at(
                span_for_refusal,
                "a nesting selector without a parent rule",
            ));
        }
        let parent_idx = rule_idx - 1;
        let parent = chain[parent_idx];
        let mut matched = MatchCertainty::No;

        for complex_selector in parent.selector_list().selectors() {
            let parent_selectors = self.get_relative_selectors(complex_selector, parent_idx);
            let applied = self.apply_selector(
                &parent_selectors,
                chain,
                parent_idx,
                element,
                direction,
                0,
                parent_selectors.len(),
            )?;
            let branch = if applied.might_match() {
                applied
            } else {
                let mut all = true;
                for (_, compound) in relative_steps(complex_selector) {
                    if !self.is_global_with_rule(
                        CompoundView::Parsed(compound),
                        chain,
                        parent_idx,
                    )? {
                        all = false;
                        break;
                    }
                }
                if all {
                    MatchCertainty::Yes
                } else {
                    MatchCertainty::No
                }
            };
            if branch.might_match() {
                self.analysis.mark_used(complex_selector);
                matched = matched.or(branch);
            }
        }

        Ok(matched)
    }

    /// The `:has(...)` `include_self` computation — whether an enclosing rule
    /// is global-ish (`:root`, `:global(...)`, or fully-global selectors), in
    /// which case the element itself is a `:has` anchor candidate.
    fn compute_has_include_self(
        &mut self,
        chain: &[&'ast StyleRule],
        rule_idx: usize,
    ) -> MatchResult<bool> {
        for (idx, rule) in chain[..=rule_idx].iter().enumerate() {
            for complex in rule.selector_list().selectors() {
                for (_, compound) in relative_steps(complex) {
                    if self.is_global_with_rule(CompoundView::Parsed(compound), chain, idx)? {
                        return Ok(true);
                    }
                }
            }
        }
        let root_rule = chain[0];
        for complex in root_rule.selector_list().selectors() {
            for (_, compound) in relative_steps(complex) {
                for component in compound.components() {
                    if is_pseudo_class_component(component) {
                        let name = component_name(self.source_text(), component);
                        let has_args = component
                            .pseudo()
                            .and_then(SelectorPseudo::selector_list)
                            .is_some();
                        if name.as_deref() == Some("root")
                            || (name.as_deref() == Some("global") && has_args)
                        {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        Ok(false)
    }

    /// The official recursive `:not` used-marking walk (`ComplexSelector`
    /// visitor with `context.next()`).
    fn mark_complex_used_recursive(&mut self, complex: &'ast ComplexSelector) {
        self.analysis.mark_used(complex);
        for (_, compound) in relative_steps(complex) {
            for component in compound.components() {
                if is_pseudo_class_component(component) {
                    if let Some(args) = component.pseudo().and_then(SelectorPseudo::selector_list) {
                        for inner in args.selectors() {
                            self.mark_complex_used_recursive(inner);
                        }
                    }
                }
            }
        }
    }

    /// The official `attribute_matches(node, name, expected_value, operator,
    /// case_insensitive)` over the IR attribute families — no CSS-AST
    /// dependency; identical to `match.rs`'s.
    fn attribute_matches(
        &mut self,
        node: NodeId,
        name: &str,
        expected_value: Option<&str>,
        operator: Option<&str>,
        case_insensitive: bool,
    ) -> MatchResult<MatchCertainty> {
        let name_lower = name.to_lowercase();

        for attribute in self.index.attrs_of(node) {
            match attribute {
                AttrIr::Spread { .. } => return Ok(MatchCertainty::Maybe),
                AttrIr::Bind { target, .. } if target == name => return Ok(MatchCertainty::Maybe),
                AttrIr::Style { .. } if name_lower == "style" => return Ok(MatchCertainty::Maybe),
                AttrIr::Class {
                    name: class_name, ..
                } if name_lower == "class" => {
                    if operator == Some("~=") {
                        if Some(class_name.as_str()) == expected_value {
                            return Ok(MatchCertainty::Maybe);
                        }
                    } else {
                        return Ok(MatchCertainty::Maybe);
                    }
                }
                _ => {}
            }

            let (attr_name, kind) = match attribute {
                AttrIr::Static { name, value } => (name, AttrValueKind::Static(value.as_ref())),
                AttrIr::Dynamic { name, expr } => (name, AttrValueKind::Expr(*expr)),
                AttrIr::Mixed { name, parts } => (name, AttrValueKind::Mixed(parts)),
                _ => continue,
            };
            if attr_name.to_lowercase() != name_lower {
                continue;
            }

            let value = match kind {
                AttrValueKind::Static(None) => {
                    return Ok(if operator.is_none() {
                        MatchCertainty::Yes
                    } else {
                        MatchCertainty::No
                    })
                }
                other => other,
            };
            let Some(expected_value) = expected_value else {
                return Ok(match value {
                    AttrValueKind::Static(_) | AttrValueKind::Mixed(_) => MatchCertainty::Yes,
                    AttrValueKind::Expr(_) => MatchCertainty::Maybe,
                });
            };

            if let AttrValueKind::Static(Some(static_value)) = value {
                let operator = operator.ok_or_else(|| {
                    MatcherRefusal::at(
                        self.index.fallback_span,
                        "an attribute value test without an operator",
                    )
                })?;
                let matches = test_attribute(
                    operator,
                    expected_value,
                    case_insensitive,
                    static_value.value.as_str(),
                )?;
                if !matches && (name_lower == "class" || name_lower == "style") {
                    continue;
                }
                return Ok(if matches {
                    MatchCertainty::Yes
                } else {
                    MatchCertainty::No
                });
            }

            let chunks: Vec<ValueChunk<'_>> = match value {
                AttrValueKind::Expr(expr) => vec![ValueChunk::Expr(expr)],
                AttrValueKind::Mixed(parts) => parts
                    .iter()
                    .map(|part| match part {
                        MixedAttrPart::Literal(text) => ValueChunk::Text(text),
                        MixedAttrPart::Expr(expr) => ValueChunk::Expr(*expr),
                    })
                    .collect(),
                AttrValueKind::Static(_) => unreachable!("handled above"),
            };

            let mut possible_values: FxHashSet<String> = FxHashSet::default();
            let mut prev_values: Vec<String> = Vec::new();

            for chunk in &chunks {
                let current_possible_values =
                    self.get_possible_values(chunk, name_lower == "class")?;
                let Some(current_possible_values) = current_possible_values else {
                    return Ok(MatchCertainty::Maybe);
                };

                if !prev_values.is_empty() {
                    let mut start_with_space: Vec<String> = Vec::new();
                    let mut remaining: Vec<String> = Vec::new();
                    for value in &current_possible_values {
                        if starts_with_js_whitespace(value) {
                            start_with_space.push(value.clone());
                        } else {
                            remaining.push(value.clone());
                        }
                    }
                    if !remaining.is_empty() {
                        if !start_with_space.is_empty() {
                            for prev in &prev_values {
                                possible_values.insert(prev.clone());
                            }
                        }
                        let mut combined: Vec<String> =
                            Vec::with_capacity(prev_values.len() * remaining.len());
                        for prev in &prev_values {
                            for value in &remaining {
                                combined.push(format!("{prev}{value}"));
                            }
                        }
                        prev_values = combined;
                        for value in &start_with_space {
                            if ends_with_js_whitespace(value) {
                                possible_values.insert(value.clone());
                            } else {
                                prev_values.push(value.clone());
                            }
                        }
                        continue;
                    }
                    for prev in prev_values.drain(..) {
                        possible_values.insert(prev);
                    }
                }

                for value in &current_possible_values {
                    if ends_with_js_whitespace(value) {
                        possible_values.insert(value.clone());
                    } else {
                        prev_values.push(value.clone());
                    }
                }
                if prev_values.len() < current_possible_values.len() {
                    prev_values.push(" ".to_string());
                }
                if prev_values.len() > 20 {
                    return Ok(MatchCertainty::Maybe);
                }
            }
            for prev in prev_values {
                possible_values.insert(prev);
            }

            let operator = operator.ok_or_else(|| {
                MatcherRefusal::at(
                    self.index.fallback_span,
                    "an attribute value test without an operator",
                )
            })?;
            for value in &possible_values {
                if test_attribute(operator, expected_value, case_insensitive, value)? {
                    return Ok(MatchCertainty::Maybe);
                }
            }
        }

        Ok(MatchCertainty::No)
    }

    /// The official `get_possible_values(chunk, is_class)` — identical to
    /// `match.rs`'s; no CSS-AST dependency.
    fn get_possible_values(
        &self,
        chunk: &ValueChunk<'_>,
        is_class: bool,
    ) -> MatchResult<Option<Vec<String>>> {
        match chunk {
            ValueChunk::Text(text) => Ok(Some(vec![(*text).to_string()])),
            ValueChunk::Expr(expr) => {
                let analyzed = self.index.ir.analysis.expressions.get(*expr);
                expression_possible_values(&analyzed.matcher_expr, is_class)
                    .map_err(|construct| MatcherRefusal::at(self.index.fallback_span, construct))
            }
        }
    }
}

/// The decoded name of a `Type`-kind component: the shared grammar recognizes
/// the universal selector `*` as a `Type` component with NO `name_span` (its
/// token is `Delim`, not `Ident` — see `selector_name_span` in
/// `verter_css_syntax::selector`), so a missing name span on a `Type`
/// component means `*` (the only shape `parse_selector_name_component`
/// builds a nameless `Type` component for).
fn type_selector_name<'a>(
    source: &'a str,
    component: &verter_css_syntax::SelectorComponent,
) -> std::borrow::Cow<'a, str> {
    match component_name(source, component) {
        Some(name) => name,
        None => std::borrow::Cow::Borrowed("*"),
    }
}

fn attribute_matcher_str(matcher: verter_css_syntax::AttributeMatcher) -> &'static str {
    use verter_css_syntax::AttributeMatcher;
    match matcher {
        AttributeMatcher::Exact => "=",
        AttributeMatcher::Includes => "~=",
        AttributeMatcher::DashMatch => "|=",
        AttributeMatcher::Prefix => "^=",
        AttributeMatcher::Suffix => "$=",
        AttributeMatcher::Substring => "*=",
    }
}

/// One attribute value chunk — the official `Text | ExpressionTag`.
enum ValueChunk<'a> {
    Text(&'a str),
    Expr(crate::svelte::runtime::ir::ExprId),
}

/// The attribute value families the `Attribute` arm distinguishes.
enum AttrValueKind<'a> {
    Static(Option<&'a crate::svelte::runtime::ir::StaticAttrValue>),
    Expr(crate::svelte::runtime::ir::ExprId),
    Mixed(&'a [MixedAttrPart]),
}

#[cfg(test)]
#[path = "match_tests.rs"]
mod match_tests;
