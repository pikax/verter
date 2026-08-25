//! The official `get_relative_selectors` / `truncate` relative-selector list
//! construction over the shared `verter_css_syntax` grammar: discard trailing
//! `:global(...)`, reduce a `:root...:has(...)` compound to its `:has`
//! selectors, and prepend the implicit `& ` (nesting + descendant) for a
//! nested rule without an explicit `&`.
//!
//! ## Why a view algebra instead of synthesized selector nodes
//!
//! A naive port of the official algorithm would synthesize brand-new
//! `RelativeSelector`/`SimpleSelector`-shaped values at match time (a
//! sentinel-spanned `&`/`*` marker, a `:has(...)` compound filtered down to
//! only its `:has(...)` components) — a `Cow<'ast, RelativeSelector>` that
//! either borrows a real AST node or owns a cloned/edited one.
//! `verter_css_syntax`'s shared CST types (`SelectorCompound`,
//! `SelectorComponent`) are deliberately NOT publicly constructible (private
//! fields, built only by the parse sink), so that pattern cannot target them
//! directly.
//!
//! [`StepView`] is the alternative: a small `Copy` value that is EITHER a
//! real `(combinator, compound)` pair borrowed from the tree, a real compound
//! filtered to a component subset ([`CompoundView::OnlyHas`]), or a wholly
//! synthetic marker with no real span at all
//! ([`CompoundView::SyntheticAny`] / [`CompoundView::SyntheticNesting`]).
//! Because every variant is `Copy`, "swap the combinator" becomes
//! constructing a new `StepView` value with a different [`CombinatorView`] —
//! no synthetic AST node is ever built. This is never a competing
//! grammar/AST: it independently parses nothing; it only borrows
//! already-parsed CST nodes or carries a zero-span synthetic tag for the
//! algorithm's own bookkeeping.
//!
//! ## Why this module takes `&CssAnalysis`
//!
//! The official `truncate` reads `child.metadata.is_global_like` /
//! `.metadata.is_global` directly off the `RelativeSelector` node (a field
//! the analyzer populated earlier in the pipeline). Under the side-table
//! design those facts live in [`super::analyze::CssAnalysis`], keyed
//! by [`SelectorCompound`] span — so this module needs read access to the
//! SAME analysis the matcher holds.

use verter_css_syntax::{
    CombinatorKind, ComplexSelector, SelectorCombinator, SelectorComponent, SelectorComponentKind,
    SelectorCompound, SelectorPseudo,
};
use verter_span::Span;

use super::analyze::{
    component_name, is_pseudo_class_component, relative_steps, CompoundTail, CssAnalysis,
};

/// The combinator half of one match step — a real combinator token, an
/// explicit absence (the first compound of a top-level selector), or the
/// synthetic descendant combinator the two prepend sites (`get_relative_selectors`'s
/// implicit `& `, `apply_selector`'s `:has(...)` "excluding self" anchor) need.
#[derive(Debug, Clone, Copy)]
pub(super) enum CombinatorView<'ast> {
    Parsed(&'ast SelectorCombinator),
    None,
    SyntheticDescendant,
}

impl CombinatorView<'_> {
    /// The combinator kind constraining `apply_combinator`'s walk, or `None`
    /// for an absent combinator (the walk terminates — nothing left to
    /// constrain).
    pub(super) fn kind(&self) -> Option<CombinatorKind> {
        match self {
            Self::Parsed(combinator) => Some(combinator.kind()),
            Self::None => None,
            Self::SyntheticDescendant => Some(CombinatorKind::Descendant),
        }
    }
}

/// The compound half of one match step — see the module doc for why this is
/// a view over the real tree rather than an owned/cloned node.
#[derive(Debug, Clone, Copy)]
pub(super) enum CompoundView<'ast> {
    /// A real compound, iterated in full.
    Parsed(&'ast SelectorCompound),
    /// A real compound (`truncate`'s `:root...:has(...)` reduction), but the
    /// component walk sees only its `:has(...)` components — the compound's
    /// own `scoped`/`is_global`/`is_global_like` facts stay keyed on the
    /// WHOLE compound (its own span), unaffected by which components the
    /// walk considers.
    OnlyHas(&'ast SelectorCompound),
    /// `*` (`apply_selector`'s `:has(...)` "excluding self" anchor) — no
    /// real compound backs this.
    SyntheticAny,
    /// `&` (`get_relative_selectors`'s implicit-nesting prepend) — no real
    /// compound backs this.
    SyntheticNesting,
}

impl<'ast> CompoundView<'ast> {
    /// The real `SelectorCompound` a `scoped`/`is_global`/`is_global_like`
    /// fact lookup or write targets — `Some` only for `Parsed`/`OnlyHas`;
    /// `None` for a synthetic step (nothing to write back to, matching the
    /// official algorithm's own write-to-a-sentinel-span no-op).
    pub(super) fn origin(&self) -> Option<&'ast SelectorCompound> {
        match self {
            Self::Parsed(compound) | Self::OnlyHas(compound) => Some(*compound),
            Self::SyntheticAny | Self::SyntheticNesting => None,
        }
    }

    /// The components the walk sees: real components for `Parsed`, the
    /// `:has(...)`-only subset for `OnlyHas`, one synthetic marker for the
    /// two synthetic variants (a real, grammar-recognized `%`/`An+B` compound
    /// with zero typed components — see [`ComponentView`] — also yields a
    /// single classified marker rather than an empty list).
    pub(super) fn components(
        &self,
        full_source: &str,
        analysis: &CssAnalysis,
    ) -> Vec<ComponentView<'ast>> {
        match self {
            Self::SyntheticAny => vec![ComponentView::SyntheticAnyType],
            Self::SyntheticNesting => vec![ComponentView::SyntheticNesting],
            Self::Parsed(compound) => {
                let all: Vec<&SelectorComponent> = compound.components().iter().collect();
                classify_components(analysis, compound, &all)
            }
            Self::OnlyHas(compound) => {
                let has: Vec<&SelectorComponent> = compound
                    .components()
                    .iter()
                    .filter(|component| {
                        is_pseudo_class_component(component)
                            && component_name(full_source, component).as_deref() == Some("has")
                    })
                    .collect();
                classify_components(analysis, compound, &has)
            }
        }
    }
}

/// One simple-selector-equivalent the matcher walk considers.
#[derive(Debug, Clone, Copy)]
pub(super) enum ComponentView<'ast> {
    /// A real, typed component.
    Real(&'ast SelectorComponent),
    /// `*` (matches the official upstream `SimpleSelector::Type { name: "*",
    /// .. }` synthetic marker).
    SyntheticAnyType,
    /// `&` (matches the official upstream `SimpleSelector::Nesting`
    /// synthetic marker).
    SyntheticNesting,
    /// A keyframe percentage step (`50%`) — the shared grammar recognizes no
    /// typed component for this shape (see
    /// [`verter_css_syntax::svelte_percentage_selector_span`]'s doc); the compound closes with
    /// zero components and this classifies the compound's own raw span. The
    /// carried span is never read on the match-decision path today (the
    /// matcher only needs to know THAT a step is a percentage marker to skip
    /// it, never its exact bytes) — kept structurally (`#[allow(dead_code)]`
    /// on the field, not degraded to a unit variant) because a Span-carrying
    /// variant is the natural anchor for a future diagnostic.
    #[allow(dead_code)]
    Percentage(Span),
    /// Svelte's lenient "any pseudo-class argument that looks like an An+B
    /// formula becomes an opaque token" rule (see
    /// [`verter_css_syntax::svelte_nth_of_selector_span`]'s doc), for a compound that itself has
    /// zero typed components — the sole practical route here is a compound
    /// inside an `:is`/`:where`/`:has`/`:not` argument list whose content is
    /// not a real selector at all (`:is(2n+1)`); `:nth-child`/`:nth-last-child`
    /// itself is never unwrapped by the matcher (its args are read
    /// structurally via `SelectorPseudo::nth()` at the producer boundary, not
    /// walked as a nested selector list). Same unread-span rationale as
    /// [`Percentage`](Self::Percentage).
    #[allow(dead_code)]
    Nth(Span),
}

/// Classify `components` (already the components the walk should see —
/// either a compound's own, or the `:has(...)`-only subset) against the
/// official upstream `SimpleSelector` shape: a REAL typed component for each
/// recognized entry, or — only when `components` is empty (the general
/// grammar recognized nothing at all) — the keyframe-percentage /
/// lenient-nth-of grammar-gap classification the analyzer already decided
/// and cached on `compound`'s [`CompoundTail`](super::analyze::CompoundTail)
/// fact (see its doc for why the compound's own span is a sound proxy for
/// the pseudo argument span these classifiers were designed against: a
/// compound reached this way is always the WHOLE content of its enclosing
/// selector-list, so its span and the enclosing pseudo's `argument_span()`
/// cover the identical byte range). This function reads that cached fact —
/// it never re-derives the classification from raw source bytes itself.
fn classify_components<'ast>(
    analysis: &CssAnalysis,
    compound: &'ast SelectorCompound,
    components: &[&'ast SelectorComponent],
) -> Vec<ComponentView<'ast>> {
    if components.is_empty() {
        return match analysis.compound_facts(compound).tail {
            CompoundTail::Percentage(span) => vec![ComponentView::Percentage(span)],
            CompoundTail::NthOf(span) => vec![ComponentView::Nth(span)],
            _ => Vec::new(),
        };
    }
    components
        .iter()
        .map(|component| ComponentView::Real(component))
        .collect()
}

/// One match step: the official `RelativeSelector` node's (combinator,
/// compound) shape.
#[derive(Debug, Clone, Copy)]
pub(super) struct StepView<'ast> {
    pub(super) combinator: CombinatorView<'ast>,
    pub(super) compound: CompoundView<'ast>,
}

/// The official `get_relative_selectors(node)` — the truncated relative
/// selectors, with an implicit `& ` (nesting + descendant) prepended for a
/// nested rule without an explicit `&`.
pub(super) fn get_relative_selectors<'ast>(
    full_source: &str,
    analysis: &CssAnalysis,
    complex: &'ast ComplexSelector,
    rule_idx: usize,
) -> Vec<StepView<'ast>> {
    let mut steps = truncate(full_source, analysis, complex);

    // `node.metadata.rule?.metadata.parent_rule && selectors.length > 0`.
    if rule_idx >= 1 && !steps.is_empty() {
        let has_explicit_nesting_selector = steps
            .iter()
            .any(|step| step_contains_nesting(step.compound));

        if !has_explicit_nesting_selector {
            let first_combinator = match steps[0].combinator {
                CombinatorView::None => CombinatorView::SyntheticDescendant,
                other => other,
            };
            steps[0] = StepView {
                combinator: first_combinator,
                compound: steps[0].compound,
            };
            steps.insert(
                0,
                StepView {
                    combinator: CombinatorView::None,
                    compound: CompoundView::SyntheticNesting,
                },
            );
        }
    }

    steps
}

/// The official nesting-selector search (the zimmerframe `NestingSelector`
/// walk — recursive through pseudo-class argument lists), over a compound
/// view rather than a raw component slice so the search reaches `OnlyHas`'s
/// filtered subset the same way the official algorithm's own
/// `Vec<SimpleSelector>` retain does.
fn step_contains_nesting(compound: CompoundView<'_>) -> bool {
    // The compound-view kind never affects reachability here — a `:has(...)`
    // filter only removes NON-`:has` components (nesting can only occur
    // inside a `:has(...)` pseudo's own args, which stay reachable), and the
    // synthetic markers carry no nesting of their own (the prepended `&`
    // step itself is never re-examined by this search — it is inserted only
    // when this search already returned `false`).
    let Some(real) = compound.origin() else {
        return false;
    };
    real.components().iter().any(component_contains_nesting)
}

fn component_contains_nesting(component: &SelectorComponent) -> bool {
    if component.kind() == SelectorComponentKind::Nesting {
        return true;
    }
    if let Some(list) = component.pseudo().and_then(SelectorPseudo::selector_list) {
        for complex in list.selectors() {
            for (_, compound) in relative_steps(complex) {
                if compound.components().iter().any(component_contains_nesting) {
                    return true;
                }
            }
        }
    }
    false
}

/// The official `truncate(node)` — discard trailing `:global(...)` selectors,
/// and reduce a `:root...:has(...)` compound to its `:has` selectors.
pub(super) fn truncate<'ast>(
    full_source: &str,
    analysis: &CssAnalysis,
    complex: &'ast ComplexSelector,
) -> Vec<StepView<'ast>> {
    let steps = relative_steps(complex);

    let last_scoped = steps.iter().rposition(|(_, compound)| {
        let components = compound.components();
        let first = components.first();
        let first_is_bare_global = first.is_some_and(|component| {
            is_pseudo_class_component(component)
                && component
                    .pseudo()
                    .and_then(SelectorPseudo::selector_list)
                    .is_none()
                && component_name(full_source, component).as_deref() == Some("global")
        });
        let facts = analysis.compound_facts(compound);
        // Not after a `:global` selector, not a bare `:global`, not a
        // `:global(...)` without a scoped modifier.
        !facts.is_global_like && !first_is_bare_global && !facts.is_global
    });

    let upto = last_scoped.map_or(0, |i| i + 1);
    steps[..upto]
        .iter()
        .map(|(combinator, compound)| {
            let combinator_view = combinator.map_or(CombinatorView::None, CombinatorView::Parsed);
            // In `:root.y:has(...)`, `y` is unscoped but the `:has(...)`
            // contents stay scoped — keep only the `:has` selectors.
            let components = compound.components();
            let has_root = components.iter().any(|component| {
                is_pseudo_class_component(component)
                    && component_name(full_source, component).as_deref() == Some("root")
            });
            let facts = analysis.compound_facts(compound);
            let compound_view = if !has_root || facts.is_global_like {
                CompoundView::Parsed(compound)
            } else {
                CompoundView::OnlyHas(compound)
            };
            StepView {
                combinator: combinator_view,
                compound: compound_view,
            }
        })
        .collect()
}
