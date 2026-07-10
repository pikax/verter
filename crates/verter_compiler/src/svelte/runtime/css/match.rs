//! The Svelte selector-to-template matcher — a faithful port of the official
//! `svelte@5.56.3` `phases/2-analyze/css/css-prune.js` (`prune`,
//! `apply_selector` BACKWARD, `apply_combinator`,
//! `relative_selector_might_apply_to_node`, `attribute_matches`,
//! `test_attribute`, and the DOM-neighborhood helpers) plus the
//! `phases/2-analyze/css/utils.js` `get_possible_values` class/attribute
//! value enumeration (UNKNOWN bails to "may match anything", exactly as
//! upstream).
//!
//! Inputs are the analyzed span-bearing CSS AST and the runtime IR
//! ([`SvelteRuntimeIr`]) — the IR is consumed STRICTLY read-only. The IR has
//! no parent/sibling/path accessor, so the matcher builds its own
//! [`TemplateIndex`] in ONE downward walk from `root_scope().roots`: a
//! per-node `path` (the ancestor chain interleaving fragment + container
//! entries, root→parent, exactly the official `metadata.path` shape), an
//! ordered node list per fragment (element children and template-scope
//! roots), the snippet⇄site links (`SnippetBlock.metadata.sites` /
//! `RenderTag.metadata.snippets` / `Component.metadata.snippets`), and the
//! element inventory the official `analysis.elements` iterable carries.
//!
//! Outcomes are FAIL-CLOSED: any template or selector construct whose
//! official neighborhood semantics cannot be PROVEN from the IR (a legacy
//! `<slot>` element, a named-slot filler whose lowered child order diverges
//! from the source fragment order, `<svelte:fragment slot>` hoisting that
//! erases the official climb-out boundary, a `<svelte:head>` `<title>` that
//! the IR decomposes away, a `{@render}` spread argument, a value literal
//! whose JS stringification cannot be reproduced exactly) aborts the match
//! with the typed [`MatcherRefusal`] — never a guessed scope, never an
//! over-approximated `used` verdict.
//!
//! The JS algorithm mutates `metadata.used` / `metadata.scoped` in place
//! through shared object references (spread-copied selectors alias the same
//! `metadata` object). The port collects those writes in a [`MatchSink`]
//! keyed by node span — a spread-copy keeps the original's span, so the
//! deferred write lands on the aliased original exactly as the JS mutation
//! does — and applies them after the walk; the algorithm never reads its own
//! `used`/`scoped` writes, so deferral is semantics-preserving. Synthetic
//! selectors (the official module-level `nesting_selector` / `any_selector`)
//! carry a sentinel span no AST node owns, so their writes drop, matching the
//! upstream write-to-singleton no-op.

// The template-neighborhood index + DOM-neighborhood helpers and the
// expression-value enumeration live in the sibling submodules; this file owns
// the selector-matching walk itself.
#[path = "match_index.rs"]
mod index;
#[path = "match_values.rs"]
mod values;

use std::borrow::Cow;

use rustc_hash::FxHashSet;
use verter_span::Span;

use index::{
    get_ancestor_elements, get_descendant_elements, get_element_parent,
    get_possible_element_siblings, Direction, TemplateIndex,
};
use values::expression_possible_values;

use super::analyze::{is_outer_global, is_unscoped_pseudo_class};
use super::types::{
    Atrule, Block, BlockChild, Combinator, ComplexSelector, MatchedTemplateFacts, RelativeSelector,
    Rule, SelectorList, SimpleSelector, StyleChild, StyleSheet,
};
use crate::svelte::runtime::ir::{AttrIr, MixedAttrPart, NodeId, SvelteRuntimeIr};

/// Run the official `prune(stylesheet, elements)` walk over the analyzed CSS
/// AST and the component's template. On success the `used` / `scoped`
/// selector verdicts are written onto the AST metadata and the per-element
/// scope facts are returned; an unprovable construct returns the typed
/// fail-closed [`MatcherRefusal`] and writes NO facts.
pub(crate) fn match_stylesheet(
    ast: &mut StyleSheet,
    ir: &SvelteRuntimeIr<'_>,
) -> Result<MatchedTemplateFacts, MatcherRefusal> {
    let fallback_span = ast.span;
    let index = TemplateIndex::build(ir, fallback_span)?;
    let sink = {
        let mut matcher = Matcher {
            index: &index,
            sink: MatchSink::default(),
        };
        let mut chain: Vec<&Rule> = Vec::new();
        matcher.prune_children(&ast.children, &mut chain)?;
        matcher.sink
    };
    apply_sink_to_children(&mut ast.children, &sink);
    Ok(MatchedTemplateFacts {
        scoped: sink.scoped_elements,
    })
}

/// A template or selector construct the matcher cannot PROVE equivalent to
/// the official semantics — the typed fail-closed refusal of
/// [`match_stylesheet`]: no facts are published, no plan is constructed, and
/// the caller refuses emission on the style surface (never a guessed scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatcherRefusal {
    /// The byte span of the offending construct (absolute in the component
    /// source; the css body span when the construct carries no span of its
    /// own).
    pub(crate) span: Span,
    /// A stable description of the unprovable construct class.
    pub(crate) construct: &'static str,
}

impl MatcherRefusal {
    fn at(span: Span, construct: &'static str) -> Self {
        Self { span, construct }
    }
}

type MatchResult<T> = Result<T, MatcherRefusal>;

/// The deferred metadata writes of one match run (see the module docs for the
/// span-keyed aliasing rationale).
#[derive(Default)]
struct MatchSink {
    /// `ComplexSelector.metadata.used = true` writes, keyed by node span.
    used_selectors: FxHashSet<Span>,
    /// `RelativeSelector.metadata.scoped = true` writes, keyed by node span.
    scoped_selectors: FxHashSet<Span>,
    /// `element.metadata.scoped = true` writes (the per-element scope facts).
    scoped_elements: FxHashSet<NodeId>,
}

// ─────────────────────────────────────────────────────────────────────────────
// The selector matcher (css-prune.js head): prune / apply_selector /
// apply_combinator / relative_selector_might_apply_to_node.
// ─────────────────────────────────────────────────────────────────────────────

/// A relative selector VIEW — the original AST node or an official-style
/// spread copy (combinator swap, `:root`-filter, synthetic). A copy keeps the
/// original's span, so deferred metadata writes alias the original exactly as
/// the JS shared-`metadata` object does; synthetics carry [`synthetic_span`].
type RelView<'ast> = Cow<'ast, RelativeSelector>;

/// The sentinel span synthetic selectors carry (the official `start: -1`
/// nodes) — no AST node owns it, so sink writes to it drop.
fn synthetic_span() -> Span {
    Span::new(u32::MAX, u32::MAX)
}

/// The official module-level `descendant_combinator`.
fn descendant_combinator() -> Combinator {
    Combinator {
        span: synthetic_span(),
        name: " ".to_string(),
    }
}

/// The official module-level `nesting_selector` (`&`).
fn nesting_selector() -> RelativeSelector {
    RelativeSelector {
        span: synthetic_span(),
        combinator: None,
        selectors: vec![SimpleSelector::Nesting {
            span: synthetic_span(),
        }],
        metadata: Default::default(),
    }
}

/// The official module-level `any_selector` (`*`).
fn any_selector() -> RelativeSelector {
    RelativeSelector {
        span: synthetic_span(),
        combinator: None,
        selectors: vec![SimpleSelector::Type {
            span: synthetic_span(),
            name: "*".to_string(),
        }],
        metadata: Default::default(),
    }
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

/// The matcher state: the template index plus the deferred metadata writes.
struct Matcher<'i, 'ir, 'src> {
    index: &'i TemplateIndex<'ir, 'src>,
    sink: MatchSink,
}

impl<'ast> Matcher<'_, '_, '_> {
    /// The official `prune` walk over rule-position children: a global-block
    /// rule visits only its prelude; every other rule visits prelude + block.
    fn prune_children(
        &mut self,
        children: &'ast [StyleChild],
        chain: &mut Vec<&'ast Rule>,
    ) -> MatchResult<()> {
        for child in children {
            match child {
                StyleChild::Rule(rule) => self.prune_rule(rule, chain)?,
                StyleChild::Atrule(atrule) => self.prune_atrule(atrule, chain)?,
            }
        }
        Ok(())
    }

    fn prune_atrule(
        &mut self,
        atrule: &'ast Atrule,
        chain: &mut Vec<&'ast Rule>,
    ) -> MatchResult<()> {
        if let Some(block) = &atrule.block {
            self.prune_block(block, chain)?;
        }
        Ok(())
    }

    fn prune_block(&mut self, block: &'ast Block, chain: &mut Vec<&'ast Rule>) -> MatchResult<()> {
        for child in &block.children {
            match child {
                BlockChild::Declaration(_) => {}
                BlockChild::Rule(rule) => self.prune_rule(rule, chain)?,
                BlockChild::Atrule(atrule) => self.prune_atrule(atrule, chain)?,
            }
        }
        Ok(())
    }

    fn prune_rule(&mut self, rule: &'ast Rule, chain: &mut Vec<&'ast Rule>) -> MatchResult<()> {
        chain.push(rule);
        let result = (|| {
            for complex in &rule.prelude.children {
                self.prune_complex_selector(complex, chain)?;
            }
            // `Rule(node, context)`: a global block visits only its prelude.
            if !rule.metadata.is_global_block {
                self.prune_block(&rule.block, chain)?;
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
        chain: &[&'ast Rule],
    ) -> MatchResult<()> {
        let rule_idx = chain.len() - 1;
        let selectors = get_relative_selectors(complex, rule_idx);
        let elements = self.index.elements.clone();
        for element in elements {
            if self.apply_selector(
                &selectors,
                chain,
                rule_idx,
                element,
                Direction::Backward,
                0,
                selectors.len(),
            )? {
                self.sink.used_selectors.insert(complex.span);
            }
        }
        Ok(())
    }

    /// The official `apply_selector(relative_selectors, rule, element,
    /// direction, from, to)`.
    #[allow(clippy::too_many_arguments)]
    fn apply_selector(
        &mut self,
        relative_selectors: &[RelView<'_>],
        chain: &[&Rule],
        rule_idx: usize,
        element: NodeId,
        direction: Direction,
        from: usize,
        to: usize,
    ) -> MatchResult<bool> {
        if from >= to {
            return Ok(false);
        }
        let selector_index = match direction {
            Direction::Forward => from,
            Direction::Backward => to - 1,
        };
        let relative_selector = &relative_selectors[selector_index];
        let (rest_from, rest_to) = match direction {
            Direction::Forward => (from + 1, to),
            Direction::Backward => (from, to - 1),
        };

        let matched = self.relative_selector_might_apply_to_node(
            relative_selector,
            chain,
            rule_idx,
            element,
            direction,
        )? && self.apply_combinator(
            relative_selector,
            relative_selectors,
            rest_from,
            rest_to,
            chain,
            rule_idx,
            element,
            direction,
        )?;

        if matched {
            if !is_outer_global(relative_selector.as_ref()) {
                self.sink
                    .scoped_selectors
                    .insert(relative_selector.as_ref().span);
            }
            self.sink.scoped_elements.insert(element);
        }

        Ok(matched)
    }

    /// The official `apply_combinator`.
    #[allow(clippy::too_many_arguments)]
    fn apply_combinator(
        &mut self,
        relative_selector: &RelView<'_>,
        relative_selectors: &[RelView<'_>],
        from: usize,
        to: usize,
        chain: &[&Rule],
        rule_idx: usize,
        node: NodeId,
        direction: Direction,
    ) -> MatchResult<bool> {
        let combinator = match direction {
            Direction::Forward => {
                if from < to {
                    relative_selectors[from].as_ref().combinator.as_ref()
                } else {
                    None
                }
            }
            Direction::Backward => relative_selector.as_ref().combinator.as_ref(),
        };
        let Some(combinator) = combinator else {
            return Ok(true);
        };

        match combinator.name.as_str() {
            " " | ">" => {
                let is_adjacent = combinator.name == ">";
                let parents = match direction {
                    Direction::Forward => get_descendant_elements(self.index, node, is_adjacent),
                    Direction::Backward => {
                        let mut seen: FxHashSet<NodeId> = FxHashSet::default();
                        get_ancestor_elements(self.index, node, is_adjacent, &mut seen)
                    }
                };
                let mut parent_matched = false;
                for &parent in &parents {
                    if self.apply_selector(
                        relative_selectors,
                        chain,
                        rule_idx,
                        parent,
                        direction,
                        from,
                        to,
                    )? {
                        parent_matched = true;
                    }
                }
                Ok(parent_matched
                    || (direction == Direction::Backward
                        && (!is_adjacent || parents.is_empty())
                        && self.every_is_global(relative_selectors, chain, rule_idx, from, to)?))
            }
            "+" | "~" => {
                let mut seen: FxHashSet<NodeId> = FxHashSet::default();
                let siblings = get_possible_element_siblings(
                    self.index,
                    node,
                    direction,
                    combinator.name == "+",
                    &mut seen,
                );
                let mut sibling_matched = false;
                for possible_sibling in siblings.keys() {
                    if self.index.is_render_tag(possible_sibling)
                        || self.index.is_component_node(possible_sibling)
                    {
                        // `{@render foo()}<p>foo</p>` with `:global(.x) + p`
                        // is a match.
                        if to - from == 1 && relative_selectors[from].as_ref().metadata.is_global {
                            sibling_matched = true;
                        }
                    } else if self.apply_selector(
                        relative_selectors,
                        chain,
                        rule_idx,
                        possible_sibling,
                        direction,
                        from,
                        to,
                    )? {
                        sibling_matched = true;
                    }
                }
                Ok(sibling_matched
                    || (direction == Direction::Backward
                        && get_element_parent(self.index, node).is_none()
                        && self.every_is_global(relative_selectors, chain, rule_idx, from, to)?))
            }
            // Other combinators (`||`) are accepted as upstream does.
            _ => Ok(true),
        }
    }

    /// The official `every_is_global(relative_selectors, from, to, rule)`.
    fn every_is_global(
        &mut self,
        relative_selectors: &[RelView<'_>],
        chain: &[&Rule],
        rule_idx: usize,
        from: usize,
        to: usize,
    ) -> MatchResult<bool> {
        for selector in &relative_selectors[from..to] {
            if !self.is_global_with_rule(selector.as_ref(), chain, rule_idx)? {
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
        selector: &RelativeSelector,
        chain: &[&Rule],
        rule_idx: usize,
    ) -> MatchResult<bool> {
        if selector.metadata.is_global || selector.metadata.is_global_like {
            return Ok(true);
        }

        let mut explicitly_global = false;

        for simple in &selector.selectors {
            let mut selector_list: Option<(&SelectorList, usize)> = None;
            let mut can_be_global = false;

            if let SimpleSelector::PseudoClass { name, args, .. } = simple {
                if (name == "is" || name == "where") && args.is_some() {
                    selector_list = args.as_ref().map(|list| (list, rule_idx));
                } else {
                    can_be_global = is_unscoped_pseudo_class(simple);
                }
            }

            if matches!(simple, SimpleSelector::Nesting { .. }) {
                if rule_idx == 0 {
                    // The analyzer only accepts a top-level `&` as the lone
                    // `:global(&)`, which truncation removes before this arm
                    // — a parentless nesting reference here is unprovable.
                    return Err(MatcherRefusal::at(
                        selector.span,
                        "a nesting selector without a parent rule",
                    ));
                }
                let owner_idx = rule_idx - 1;
                selector_list = Some((&chain[owner_idx].prelude, owner_idx));
            }

            let has_global_selectors = match selector_list {
                None => false,
                Some((list, owner_idx)) => {
                    let mut any = false;
                    for complex in &list.children {
                        let mut all = !complex.children.is_empty();
                        for relative in &complex.children {
                            if !self.is_global_with_rule(relative, chain, owner_idx)? {
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

        Ok(explicitly_global || selector.selectors.is_empty())
    }

    /// The official `relative_selector_might_apply_to_node`.
    fn relative_selector_might_apply_to_node(
        &mut self,
        relative_selector: &RelView<'_>,
        chain: &[&Rule],
        rule_idx: usize,
        element: NodeId,
        direction: Direction,
    ) -> MatchResult<bool> {
        let mut include_self: Option<bool> = None;

        // Borrow the selector list for the whole loop (a spread copy owns its
        // filtered list; the borrow lives on the view either way).
        let selectors: &[SimpleSelector] = &relative_selector.as_ref().selectors;

        for selector in selectors {
            // `:has(...)` — treat `.x:has(.y)` like `.x .y`, walking FORWARD.
            if let SimpleSelector::PseudoClass {
                name,
                args: Some(args),
                ..
            } = selector
            {
                if name == "has" {
                    if include_self.is_none() {
                        include_self = Some(self.compute_has_include_self(chain, rule_idx)?);
                    }
                    let include = include_self == Some(true);

                    let mut matched = false;
                    for complex_selector in &args.children {
                        let truncated = truncate(complex_selector);
                        let Some((first, rest)) = truncated.split_first() else {
                            // Just a `:global(...)`.
                            self.sink.used_selectors.insert(complex_selector.span);
                            matched = true;
                            continue;
                        };

                        if include {
                            let mut selector_including_self: Vec<RelView<'_>> =
                                Vec::with_capacity(truncated.len());
                            if first.as_ref().combinator.is_some() {
                                let mut owned = first.as_ref().clone();
                                owned.combinator = None;
                                selector_including_self.push(Cow::Owned(owned));
                            } else {
                                selector_including_self.push(first.clone());
                            }
                            selector_including_self.extend(rest.iter().cloned());
                            if self.apply_selector(
                                &selector_including_self,
                                chain,
                                rule_idx,
                                element,
                                Direction::Forward,
                                0,
                                selector_including_self.len(),
                            )? {
                                self.sink.used_selectors.insert(complex_selector.span);
                                matched = true;
                            }
                        }

                        let mut selector_excluding_self: Vec<RelView<'_>> =
                            Vec::with_capacity(truncated.len() + 1);
                        selector_excluding_self.push(Cow::Owned(any_selector()));
                        if first.as_ref().combinator.is_some() {
                            selector_excluding_self.push(first.clone());
                        } else {
                            let mut owned = first.as_ref().clone();
                            owned.combinator = Some(descendant_combinator());
                            selector_excluding_self.push(Cow::Owned(owned));
                        }
                        selector_excluding_self.extend(rest.iter().cloned());
                        if self.apply_selector(
                            &selector_excluding_self,
                            chain,
                            rule_idx,
                            element,
                            Direction::Forward,
                            0,
                            selector_excluding_self.len(),
                        )? {
                            self.sink.used_selectors.insert(complex_selector.span);
                            matched = true;
                        }
                    }

                    if !matched {
                        return Ok(false);
                    }
                    continue;
                }
            }

            if matches!(
                selector,
                SimpleSelector::Percentage { .. } | SimpleSelector::Nth { .. }
            ) {
                continue;
            }

            match selector {
                SimpleSelector::PseudoClass { name, args, .. } => {
                    let name = unescape_backslashes(name);
                    if name == "host" || name == "root" {
                        return Ok(false);
                    }

                    if name == "global" && args.is_some() && selectors.len() == 1 {
                        let args = args.as_ref().expect("checked is_some above");
                        let Some(complex_selector) = args.children.first() else {
                            return Ok(true);
                        };
                        let views: Vec<RelView<'_>> = complex_selector
                            .children
                            .iter()
                            .map(Cow::Borrowed)
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

                    // We came across a `:global` — everything beyond it is a
                    // potential match.
                    if name == "global" && args.is_none() {
                        return Ok(true);
                    }

                    // `:not(...)` contents stay unscoped; complex arguments
                    // with descendants are assumed to match (missing prune is
                    // the only drawback).
                    if name == "not" {
                        if let Some(args) = args {
                            for complex_selector in &args.children {
                                self.mark_complex_used_recursive(complex_selector);
                                let truncated = truncate(complex_selector);

                                if complex_selector.children.len() > 1 {
                                    for selector in &truncated {
                                        self.sink.scoped_selectors.insert(selector.as_ref().span);
                                    }

                                    let mut el = Some(element);
                                    while let Some(current) = el {
                                        self.sink.scoped_elements.insert(current);
                                        el = get_element_parent(self.index, current);
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    if (name == "is" || name == "where") && args.is_some() {
                        let args = args.as_ref().expect("checked is_some above");
                        let mut matched = false;

                        for complex_selector in &args.children {
                            let truncated = truncate(complex_selector);

                            if truncated.is_empty() {
                                // It was just a `:global(...)`.
                                self.sink.used_selectors.insert(complex_selector.span);
                                matched = true;
                            } else if self.apply_selector(
                                &truncated,
                                chain,
                                rule_idx,
                                element,
                                Direction::Backward,
                                0,
                                truncated.len(),
                            )? {
                                self.sink.used_selectors.insert(complex_selector.span);
                                matched = true;
                            } else if complex_selector.children.len() > 1 {
                                // `foo :is(bar baz)` can also mean `bar` is an
                                // ancestor and `baz` a descendant — assume it
                                // matches.
                                self.sink.used_selectors.insert(complex_selector.span);
                                matched = true;
                                for selector in &truncated {
                                    self.sink.scoped_selectors.insert(selector.as_ref().span);
                                }
                            }
                        }

                        if !matched {
                            return Ok(false);
                        }
                    }
                }

                SimpleSelector::PseudoElement { .. } => {}

                SimpleSelector::Attribute {
                    name: attr_name,
                    matcher,
                    value,
                    flags,
                    ..
                } => {
                    let element_name_lower = self.index.element_name(element).to_lowercase();
                    let whitelisted = whitelisted_attributes(&element_name_lower)
                        .contains(&attr_name.to_lowercase().as_str());
                    let flags = flags.as_deref().unwrap_or("");
                    let case_insensitive = flags.contains('i')
                        || (!flags.contains('s')
                            && CASE_INSENSITIVE_ATTRIBUTES
                                .contains(&attr_name.to_lowercase().as_str()));
                    if !whitelisted
                        && !self.attribute_matches(
                            element,
                            attr_name,
                            value.as_deref().map(unquote),
                            matcher.as_deref(),
                            case_insensitive,
                        )?
                    {
                        return Ok(false);
                    }
                }

                SimpleSelector::Class { name, .. } => {
                    let name = unescape_backslashes(name);
                    if !self.attribute_matches(element, "class", Some(&name), Some("~="), false)? {
                        return Ok(false);
                    }
                }

                SimpleSelector::Id { name, .. } => {
                    let name = unescape_backslashes(name);
                    if !self.attribute_matches(element, "id", Some(&name), Some("="), false)? {
                        return Ok(false);
                    }
                }

                SimpleSelector::Type { name, .. } => {
                    let name = unescape_backslashes(name);
                    // The official compare is `element.name.toLowerCase() !==
                    // name.toLowerCase()` — the FULL Unicode fold (an ASCII
                    // fold wrongly prunes `x-CAFÉ` on `<x-café>`).
                    if self.index.element_name(element).to_lowercase() != name.to_lowercase()
                        && name != "*"
                        && !self.index.is_svelte_element(element)
                    {
                        return Ok(false);
                    }
                }

                SimpleSelector::Nesting { span } => {
                    if rule_idx == 0 {
                        return Err(MatcherRefusal::at(
                            *span,
                            "a nesting selector without a parent rule",
                        ));
                    }
                    let parent_idx = rule_idx - 1;
                    let parent = chain[parent_idx];
                    let mut matched = false;

                    for complex_selector in &parent.prelude.children {
                        let parent_selectors = get_relative_selectors(complex_selector, parent_idx);
                        let applied = self.apply_selector(
                            &parent_selectors,
                            chain,
                            parent_idx,
                            element,
                            direction,
                            0,
                            parent_selectors.len(),
                        )?;
                        let all_global = if applied {
                            true
                        } else {
                            let mut all = true;
                            for relative in &complex_selector.children {
                                if !self.is_global_with_rule(relative, chain, parent_idx)? {
                                    all = false;
                                    break;
                                }
                            }
                            all
                        };
                        if applied || all_global {
                            self.sink.used_selectors.insert(complex_selector.span);
                            matched = true;
                        }
                    }

                    if !matched {
                        return Ok(false);
                    }
                }

                SimpleSelector::Percentage { .. } | SimpleSelector::Nth { .. } => {
                    unreachable!("percentage/nth were skipped above")
                }
            }
        }

        // Possible match.
        Ok(true)
    }

    /// The `:has(...)` `include_self` computation — whether an enclosing rule
    /// is global-ish (`:root`, `:global(...)`, or fully-global selectors), in
    /// which case the element itself is a `:has` anchor candidate.
    fn compute_has_include_self(&mut self, chain: &[&Rule], rule_idx: usize) -> MatchResult<bool> {
        // `get_parent_rules(rule)` — rule first, root last; the chain slice
        // carries the same rules.
        for (idx, rule) in chain[..=rule_idx].iter().enumerate() {
            for complex in &rule.prelude.children {
                for relative in &complex.children {
                    if self.is_global_with_rule(relative, chain, idx)? {
                        return Ok(true);
                    }
                }
            }
        }
        // `rules[rules.length - 1]` — the ROOT rule.
        let root_rule = chain[0];
        for complex in &root_rule.prelude.children {
            for relative in &complex.children {
                for simple in &relative.selectors {
                    if let SimpleSelector::PseudoClass { name, args, .. } = simple {
                        if name == "root" || (name == "global" && args.is_some()) {
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
    fn mark_complex_used_recursive(&mut self, complex: &ComplexSelector) {
        self.sink.used_selectors.insert(complex.span);
        for relative in &complex.children {
            for simple in &relative.selectors {
                if let SimpleSelector::PseudoClass {
                    args: Some(args), ..
                } = simple
                {
                    for inner in &args.children {
                        self.mark_complex_used_recursive(inner);
                    }
                }
            }
        }
    }

    /// The official `attribute_matches(node, name, expected_value, operator,
    /// case_insensitive)` over the IR attribute families.
    fn attribute_matches(
        &mut self,
        node: NodeId,
        name: &str,
        expected_value: Option<&str>,
        operator: Option<&str>,
        case_insensitive: bool,
    ) -> MatchResult<bool> {
        let name_lower = name.to_lowercase();

        for attribute in self.index.attrs_of(node) {
            match attribute {
                AttrIr::Spread { .. } => return Ok(true),
                AttrIr::Bind { target, .. } if target == name => return Ok(true),
                AttrIr::Style { .. } if name_lower == "style" => return Ok(true),
                AttrIr::Class {
                    name: class_name, ..
                } if name_lower == "class" => {
                    if operator == Some("~=") {
                        if Some(class_name.as_str()) == expected_value {
                            return Ok(true);
                        }
                    } else {
                        return Ok(true);
                    }
                }
                _ => {}
            }

            // `if (attribute.type !== 'Attribute') continue;`
            let (attr_name, kind) = match attribute {
                AttrIr::Static { name, value } => (name, AttrValueKind::Static(value.as_ref())),
                AttrIr::Dynamic { name, expr } => (name, AttrValueKind::Expr(*expr)),
                AttrIr::Mixed { name, parts } => (name, AttrValueKind::Mixed(parts)),
                _ => continue,
            };
            if attr_name.to_lowercase() != name_lower {
                continue;
            }

            // `if (attribute.value === true) return operator === null;`
            let value = match kind {
                AttrValueKind::Static(None) => return Ok(operator.is_none()),
                other => other,
            };
            let Some(expected_value) = expected_value else {
                return Ok(true);
            };

            // A single text chunk tests directly.
            if let AttrValueKind::Static(Some(static_value)) = value {
                let operator = operator.ok_or_else(|| {
                    MatcherRefusal::at(
                        self.index.fallback_span,
                        "an attribute value test without an operator",
                    )
                })?;
                // The DECODED semantic value (the producer boundary owns the
                // decode) — the SAME text the emitters serialize, so the match
                // verdict and the emitted attribute can never disagree
                // (`class="a&#32;b"` tests the word list `a b`).
                let matches = test_attribute(
                    operator,
                    expected_value,
                    case_insensitive,
                    static_value.value.as_str(),
                )?;
                // Continue if we still may match against a class/style
                // directive.
                if !matches && (name_lower == "class" || name_lower == "style") {
                    continue;
                }
                return Ok(matches);
            }

            // The chunked (expression / mixed) value: enumerate possible
            // values, bailing to "matches" on UNKNOWN.
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
                    // Impossible to find out all combinations.
                    return Ok(true);
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
                        // A FINITE combination product enumerates fully — the
                        // exponential bail below guards only the fresh-append
                        // growth path, exactly as upstream (whose combine
                        // branch `continue`s past the check).
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
                    // Might grow exponentially — bail out.
                    return Ok(true);
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
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// The official `get_possible_values(chunk, is_class)` — a text chunk is
    /// itself; an expression chunk enumerates via `gather_possible_values`
    /// (`None` = UNKNOWN, the "may match anything" bail).
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

// ─────────────────────────────────────────────────────────────────────────────
// get_relative_selectors / truncate.
// ─────────────────────────────────────────────────────────────────────────────

/// The official `get_relative_selectors(node)` — the truncated relative
/// selectors, with an implicit `& ` (nesting + descendant) prepended for a
/// nested rule without an explicit `&`.
fn get_relative_selectors(complex: &ComplexSelector, rule_idx: usize) -> Vec<RelView<'_>> {
    let mut selectors = truncate(complex);

    // `node.metadata.rule?.metadata.parent_rule && selectors.length > 0`.
    if rule_idx >= 1 && !selectors.is_empty() {
        let mut has_explicit_nesting_selector = false;
        for selector in &selectors {
            if selectors_contain_nesting(&selector.as_ref().selectors) {
                has_explicit_nesting_selector = true;
                break;
            }
        }

        if !has_explicit_nesting_selector {
            if selectors[0].as_ref().combinator.is_none() {
                let mut owned = selectors[0].as_ref().clone();
                owned.combinator = Some(descendant_combinator());
                selectors[0] = Cow::Owned(owned);
            }
            selectors.insert(0, Cow::Owned(nesting_selector()));
        }
    }

    selectors
}

/// The official nesting-selector search (the zimmerframe `NestingSelector`
/// walk — recursive through pseudo-class argument lists).
fn selectors_contain_nesting(selectors: &[SimpleSelector]) -> bool {
    for simple in selectors {
        match simple {
            SimpleSelector::Nesting { .. } => return true,
            SimpleSelector::PseudoClass {
                args: Some(args), ..
            } => {
                for complex in &args.children {
                    for relative in &complex.children {
                        if selectors_contain_nesting(&relative.selectors) {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// The official `truncate(node)` — discard trailing `:global(...)` selectors,
/// and reduce a `:root...:has(...)` compound to its `:has` selectors.
fn truncate(complex: &ComplexSelector) -> Vec<RelView<'_>> {
    let last_scoped = complex.children.iter().rposition(|child| {
        let first = child.selectors.first();
        let first_is_bare_global = matches!(
            first,
            Some(SimpleSelector::PseudoClass { name, args: None, .. }) if name == "global"
        );
        // Not after a `:global` selector, not a bare `:global`, not a
        // `:global(...)` without a scoped modifier.
        !child.metadata.is_global_like && !first_is_bare_global && !child.metadata.is_global
    });

    let upto = last_scoped.map_or(0, |i| i + 1);
    complex.children[..upto]
        .iter()
        .map(|child| {
            // In `:root.y:has(...)`, `y` is unscoped but the `:has(...)`
            // contents stay scoped — keep only the `:has` selectors.
            let has_root = child
                .selectors
                .iter()
                .any(|s| matches!(s, SimpleSelector::PseudoClass { name, .. } if name == "root"));
            if !has_root || child.metadata.is_global_like {
                return Cow::Borrowed(child);
            }
            let mut owned = child.clone();
            owned
                .selectors
                .retain(|s| matches!(s, SimpleSelector::PseudoClass { name, .. } if name == "has"));
            Cow::Owned(owned)
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// test_attribute + the JS string/number semantics helpers.
// ─────────────────────────────────────────────────────────────────────────────

/// The official `test_attribute(operator, expected_value, case_insensitive,
/// value)`.
fn test_attribute(
    operator: &str,
    expected_value: &str,
    case_insensitive: bool,
    value: &str,
) -> MatchResult<bool> {
    let (expected, value) = if case_insensitive {
        (expected_value.to_lowercase(), value.to_lowercase())
    } else {
        (expected_value.to_string(), value.to_string())
    };
    match operator {
        "=" => Ok(value == expected),
        // `value.split(/\s/).includes(expected)` — split on EVERY single JS
        // whitespace char (empty pieces included).
        "~=" => Ok(value.split(is_js_whitespace).any(|part| part == expected)),
        "|=" => Ok(format!("{value}-").starts_with(&format!("{expected}-"))),
        "^=" => Ok(value.starts_with(&expected)),
        "$=" => Ok(value.ends_with(&expected)),
        "*=" => Ok(value.contains(&expected)),
        // The parser only produces the six operators; anything else is
        // unprovable rather than the official throw.
        _ => Err(MatcherRefusal::at(
            synthetic_span(),
            "an unknown attribute matcher operator",
        )),
    }
}

/// The JS `\s` regex class (NOT Rust `char::is_whitespace`: JS includes
/// U+FEFF and excludes U+0085).
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `regex_starts_with_whitespace` (`/^\s/`).
fn starts_with_js_whitespace(s: &str) -> bool {
    s.chars().next().is_some_and(is_js_whitespace)
}

/// `regex_ends_with_whitespace` (`/\s$/`).
fn ends_with_js_whitespace(s: &str) -> bool {
    s.chars().last().is_some_and(is_js_whitespace)
}

/// The official `regex_backslash_and_following_character` unescape
/// (`name.replace(/\\(.)/g, '$1')` — the dot does NOT match line
/// terminators, exactly as the JS regex).
fn unescape_backslashes(name: &str) -> String {
    if !name.contains('\\') {
        return name.to_string();
    }
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&next) if !matches!(next, '\n' | '\r' | '\u{2028}' | '\u{2029}') => {
                    out.push(next);
                    chars.next();
                }
                _ => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The official `unquote(str)` (the parser already strips quote marks; this
/// stays for exactness on any residual quoted payload).
fn unquote(s: &str) -> &str {
    let chars: Vec<char> = s.chars().collect();
    let Some(&first) = chars.first() else {
        return s;
    };
    let last = *chars.last().expect("non-empty checked via first");
    if (first == last && first == '\'') || first == '"' {
        let mut iter = s.char_indices();
        let Some((_, first_char)) = iter.next() else {
            return s;
        };
        let start = first_char.len_utf8();
        let end = s.len() - last.len_utf8();
        if start <= end {
            return &s[start..end];
        }
    }
    s
}

// ─────────────────────────────────────────────────────────────────────────────
// Deferred metadata write-back.
// ─────────────────────────────────────────────────────────────────────────────

/// Apply the collected `used` / `scoped` writes onto the AST metadata —
/// span-keyed, so official spread-copy aliasing lands on the original node.
fn apply_sink_to_children(children: &mut [StyleChild], sink: &MatchSink) {
    for child in children {
        match child {
            StyleChild::Rule(rule) => apply_sink_to_rule(rule, sink),
            StyleChild::Atrule(atrule) => apply_sink_to_atrule(atrule, sink),
        }
    }
}

fn apply_sink_to_atrule(atrule: &mut Atrule, sink: &MatchSink) {
    if let Some(block) = &mut atrule.block {
        apply_sink_to_block(block, sink);
    }
}

fn apply_sink_to_block(block: &mut Block, sink: &MatchSink) {
    for child in &mut block.children {
        match child {
            BlockChild::Declaration(_) => {}
            BlockChild::Rule(rule) => apply_sink_to_rule(rule, sink),
            BlockChild::Atrule(atrule) => apply_sink_to_atrule(atrule, sink),
        }
    }
}

fn apply_sink_to_rule(rule: &mut Rule, sink: &MatchSink) {
    apply_sink_to_selector_list(&mut rule.prelude, sink);
    apply_sink_to_block(&mut rule.block, sink);
}

fn apply_sink_to_selector_list(list: &mut SelectorList, sink: &MatchSink) {
    for complex in &mut list.children {
        if sink.used_selectors.contains(&complex.span) {
            complex.metadata.used = true;
        }
        for relative in &mut complex.children {
            if sink.scoped_selectors.contains(&relative.span) {
                relative.metadata.scoped = true;
            }
            for simple in &mut relative.selectors {
                if let SimpleSelector::PseudoClass {
                    args: Some(args), ..
                } = simple
                {
                    apply_sink_to_selector_list(args, sink);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "match_tests.rs"]
mod match_tests;
