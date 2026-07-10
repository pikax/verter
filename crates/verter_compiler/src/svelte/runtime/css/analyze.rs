//! The Svelte CSS scoping analysis — a faithful port of the official
//! `svelte@5.56.3` `phases/2-analyze/css/css-analyze.js` (the zimmerframe
//! visitor walk), `phases/2-analyze/css/utils.js` (`is_global` /
//! `is_unscoped_pseudo_class` / `is_outer_global`), and `phases/css.js`
//! (`is_keyframes_node` / `remove_css_prefix`).
//!
//! The JS visitors read ancestor context through `context.path` /
//! `state.rule` / `metadata.parent_rule` object pointers; this port carries
//! the same facts on an explicit ancestor-rule frame stack (pushed before
//! each rule's block descent), preserving the official VISIT ORDER — the
//! rule-level global-block loop first, then the prelude (per relative
//! selector: the leading-combinator check, the selector metadata, the
//! argument descent; then the complex-selector placement checks + metadata),
//! then the rule facts, then the block. A validation failure surfaces as a
//! typed [`CssAnalysisError`] carrying the exact official `e.css_*` code —
//! never a panic, never a silent accept.

use verter_span::Span;

use super::types::{
    Atrule, Block, BlockChild, ComplexSelector, GlobalKeyframeName, KeyframeName, RelativeSelector,
    Rule, SimpleSelector, StyleChild, StyleSheet,
};

/// A typed CSS analysis failure: the official validation code + the byte
/// span of the offending node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssAnalysisError {
    /// The official validation code (`css_global_invalid_placement` /
    /// `css_nesting_selector_invalid_placement` / …).
    pub code: &'static str,
    /// The byte span of the offending node (absolute in the component
    /// source).
    pub span: Span,
}

impl CssAnalysisError {
    fn at(code: &'static str, span: Span) -> Self {
        Self { code, span }
    }
}

type AnalyzeResult = Result<(), CssAnalysisError>;

/// The analysis FACTS extracted from one stylesheet (selector metadata lands
/// inline on the AST).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CssAnalysis {
    /// The LOCAL `@keyframes` rename list (the official
    /// `analysis.css.keyframes`), source order.
    pub keyframes: Vec<KeyframeName>,
    /// The `-global-`-prefixed `@keyframes` names (prefix-strip list), source
    /// order.
    pub global_keyframes: Vec<GlobalKeyframeName>,
    /// Whether the component's css includes GLOBAL css (the official
    /// `analysis.css.has_global`).
    pub has_global: bool,
}

/// Analyze `stylesheet` in place: validate `:global` / nesting placement,
/// populate the selector metadata, and collect the keyframes + global facts.
pub fn analyze_stylesheet(
    source: &str,
    stylesheet: &mut StyleSheet,
) -> Result<CssAnalysis, CssAnalysisError> {
    let mut analyzer = Analyzer {
        source,
        analysis: CssAnalysis::default(),
    };
    let mut rules = Vec::new();
    analyzer.analyze_children(&mut stylesheet.children, &mut rules)?;
    Ok(analyzer.analysis)
}

/// One ancestor-rule frame — the facts the official visitors read through
/// `metadata.parent_rule` / `context.path`, computed BEFORE descending into
/// the rule's block (matching the official visit order, where a rule's
/// prelude metadata is complete before its block is visited).
struct RuleFrame {
    /// `metadata.is_global_block`.
    is_global_block: bool,
    /// Whether the frame's rule itself has a parent rule
    /// (`metadata.parent_rule !== null` one level up).
    is_nested: bool,
    /// `metadata.has_global_selectors`.
    has_global_selectors: bool,
    /// `prelude.children[0].children.length === 1 &&
    /// prelude.children[0].children[0].selectors.length === 1` — the
    /// lone-simple first selector shape the nesting-in-global-block check
    /// reads.
    prelude_first_is_lone_simple: bool,
    /// `prelude.children.some(c => c.children.length === 1 &&
    /// c.children[0].metadata.is_global)` — the nesting-scope used-marking's
    /// `parent_is_global`.
    prelude_has_lone_global: bool,
}

/// The facts of the rule whose prelude is CURRENTLY being visited (the
/// official `context.state.rule`), needed by the nesting-selector placement
/// check.
struct OwnRuleFacts {
    /// Whether the rule has a parent rule.
    is_nested: bool,
    /// The span of the one nesting selector a TOP-LEVEL rule may legally
    /// carry — the `:global(&)` form's inner `&` — when the prelude has that
    /// exact lone shape.
    allowed_nesting_span: Option<Span>,
}

struct Analyzer<'src> {
    source: &'src str,
    analysis: CssAnalysis,
}

impl Analyzer<'_> {
    /// Visit a rule-position child list (the stylesheet top level, or an
    /// at-rule block).
    fn analyze_children(
        &mut self,
        children: &mut [StyleChild],
        rules: &mut Vec<RuleFrame>,
    ) -> AnalyzeResult {
        for child in children {
            match child {
                StyleChild::Rule(rule) => self.analyze_rule(rule, rules)?,
                StyleChild::Atrule(atrule) => self.analyze_atrule(atrule, rules)?,
            }
        }
        Ok(())
    }

    /// The official `Atrule` visitor: keyframes collection (with the
    /// `-global-` / `:global {}`-block exclusions), then the block descent.
    fn analyze_atrule(&mut self, atrule: &mut Atrule, rules: &mut Vec<RuleFrame>) -> AnalyzeResult {
        if is_keyframes_node(atrule) {
            let in_global_block = rules.iter().any(|frame| frame.is_global_block);
            if !atrule.prelude.starts_with("-global-") && !in_global_block {
                let name_span = keyframes_name_token_span(self.source, atrule);
                self.analysis.keyframes.push(KeyframeName {
                    name: atrule.prelude.clone(),
                    name_span,
                });
            } else if atrule.prelude.starts_with("-global-") {
                // The keyframe is global even if its block is empty.
                self.analysis.has_global |= is_unscoped(rules);
                let name_span = keyframes_name_token_span(self.source, atrule);
                self.analysis.global_keyframes.push(GlobalKeyframeName {
                    name: atrule.prelude["-global-".len()..].to_string(),
                    name_span,
                });
            }
        }
        // `context.next()` — descend into the block (nested rules inside
        // `@media`, the keyframe step rules, …).
        if let Some(block) = &mut atrule.block {
            self.analyze_block(block, rules)?;
        }
        Ok(())
    }

    /// Visit a block's children: declarations carry no analysis; rules and
    /// at-rules recurse.
    fn analyze_block(&mut self, block: &mut Block, rules: &mut Vec<RuleFrame>) -> AnalyzeResult {
        for child in &mut block.children {
            match child {
                BlockChild::Declaration(_) => {}
                BlockChild::Rule(rule) => self.analyze_rule(rule, rules)?,
                BlockChild::Atrule(atrule) => self.analyze_atrule(atrule, rules)?,
            }
        }
        Ok(())
    }

    /// The official `Rule` visitor.
    fn analyze_rule(&mut self, rule: &mut Rule, rules: &mut Vec<RuleFrame>) -> AnalyzeResult {
        rule.metadata.is_nested = !rules.is_empty();

        // The first Declaration child (the global-block declaration check
        // target), read up front so the prelude loop below borrows only the
        // prelude.
        let first_declaration_span = rule.block.children.iter().find_map(|child| match child {
            BlockChild::Declaration(declaration) => Some(declaration.span),
            _ => None,
        });

        // ── The `:global {}`-block detection loop (official: runs BEFORE the
        // prelude visit; `:global x, :global y` is allowed because CSS
        // preprocessors generate it from `:global { x, y {...} }`). ──
        for complex_index in 0..rule.prelude.children.len() {
            let mut is_global_block = false;

            for selector_index in 0..rule.prelude.children[complex_index].children.len() {
                let child = &rule.prelude.children[complex_index].children[selector_index];
                let global_idx = child.selectors.iter().position(is_global_block_selector);

                if is_global_block {
                    // Every selector after `:global` is unscoped.
                    rule.prelude.children[complex_index].children[selector_index]
                        .metadata
                        .is_global_like = true;
                }

                match global_idx {
                    Some(0) => {
                        let child = &rule.prelude.children[complex_index].children[selector_index];
                        if child.selectors.len() > 1
                            && selector_index == 0
                            && !rule.metadata.is_nested
                        {
                            return Err(CssAnalysisError::at(
                                "css_global_block_invalid_modifier_start",
                                child.selectors[1].span(),
                            ));
                        }
                        // `child` starts with `:global` — a global block.
                        rule.metadata.is_global_block = true;
                        is_global_block = true;

                        let child =
                            &mut rule.prelude.children[complex_index].children[selector_index];
                        for i in 1..child.selectors.len() {
                            mark_argument_selectors_used_shallow(&mut child.selectors[i]);
                        }

                        let child = &rule.prelude.children[complex_index].children[selector_index];
                        if let Some(combinator) = &child.combinator {
                            if combinator.name != " " {
                                return Err(CssAnalysisError::at(
                                    "css_global_block_invalid_combinator",
                                    child.span,
                                ));
                            }
                        }

                        let complex = &rule.prelude.children[complex_index];
                        let is_lone_global =
                            complex.children.len() == 1 && complex.children[0].selectors.len() == 1; // just `:global`, not e.g. `:global x`

                        if is_lone_global && rule.prelude.children.len() > 1 {
                            // `:global, :global x { z { ... } }` would become
                            // `x { z { ... } }`, constraining `z` in a way the
                            // author did not intend.
                            return Err(CssAnalysisError::at(
                                "css_global_block_invalid_list",
                                rule.prelude.span,
                            ));
                        }

                        if let Some(declaration_span) = first_declaration_span {
                            // `:global { color: red; }` is invalid, but
                            // `foo :global { color: red; }` is valid.
                            if rule.prelude.children.len() == 1 && is_lone_global {
                                return Err(CssAnalysisError::at(
                                    "css_global_block_invalid_declaration",
                                    declaration_span,
                                ));
                            }
                        }
                    }
                    Some(idx) => {
                        return Err(CssAnalysisError::at(
                            "css_global_block_invalid_modifier",
                            rule.prelude.children[complex_index].children[selector_index].selectors
                                [idx]
                                .span(),
                        ));
                    }
                    None => {}
                }
            }

            if rule.metadata.is_global_block && !is_global_block {
                return Err(CssAnalysisError::at(
                    "css_global_block_invalid_list",
                    rule.prelude.span,
                ));
            }
        }

        // ── The prelude visit (SelectorList → ComplexSelector →
        // RelativeSelector, with `state.rule` = this rule). ──
        let own = OwnRuleFacts {
            is_nested: rule.metadata.is_nested,
            allowed_nesting_span: allowed_top_level_nesting_span(rule),
        };
        for complex in &mut rule.prelude.children {
            self.analyze_complex_selector(complex, rules, &own, false)?;
        }

        // ── The post-prelude rule facts. ──
        for selector in &rule.prelude.children {
            rule.metadata.has_global_selectors |= selector.metadata.is_global;
            rule.metadata.has_local_selectors |= !selector.metadata.is_global;
        }

        // If this rule has a ComplexSelector whose RelativeSelector children
        // are all `:global(...)`, and the rule contains DECLARATIONS (rather
        // than just nested rules), the component as a whole includes global
        // CSS.
        let declaration_count = rule
            .block
            .children
            .iter()
            .filter(|child| matches!(child, BlockChild::Declaration(_)))
            .count();
        self.analysis.has_global |=
            rule.metadata.has_global_selectors && declaration_count > 0 && is_unscoped(rules);

        // ── The block visit, with this rule's frame pushed. ──
        rules.push(RuleFrame {
            is_global_block: rule.metadata.is_global_block,
            is_nested: rule.metadata.is_nested,
            has_global_selectors: rule.metadata.has_global_selectors,
            prelude_first_is_lone_simple: rule.prelude.children.first().is_some_and(|complex| {
                complex.children.len() == 1 && complex.children[0].selectors.len() == 1
            }),
            prelude_has_lone_global: rule.prelude.children.iter().any(|complex| {
                complex.children.len() == 1 && complex.children[0].metadata.is_global
            }),
        });
        let result = self.analyze_block(&mut rule.block, rules);
        rules.pop();
        result
    }

    /// The official `ComplexSelector` visitor (children first — the
    /// `context.next()` head call — then the `:global` placement checks, then
    /// the metadata).
    fn analyze_complex_selector(
        &mut self,
        complex: &mut ComplexSelector,
        rules: &[RuleFrame],
        own: &OwnRuleFacts,
        in_pseudo_args: bool,
    ) -> AnalyzeResult {
        // `context.next()` — the RelativeSelector children.
        for index in 0..complex.children.len() {
            self.analyze_relative_selector(complex, index, rules, own, in_pseudo_args)?;
        }

        // ── `:global` placement over the whole complex selector. ──
        if let Some(global_idx) = complex.children.iter().position(is_global) {
            let global = &complex.children[global_idx];
            let first_span = global.selectors[0].span();
            let first_args_present = matches!(
                &global.selectors[0],
                SimpleSelector::PseudoClass { args: Some(_), .. }
            );
            // A block-form `:global` nested inside a pseudo-class is invalid.
            if in_pseudo_args && !first_args_present {
                return Err(CssAnalysisError::at(
                    "css_global_block_invalid_placement",
                    first_span,
                ));
            }
            // `:global(...)` must not sit in the MIDDLE of a selector
            // (multiple `:global(...)` in sequence are ok).
            if first_args_present && global_idx != 0 && global_idx != complex.children.len() - 1 {
                for i in (global_idx + 1)..complex.children.len() {
                    if !is_global(&complex.children[i]) {
                        return Err(CssAnalysisError::at(
                            "css_global_invalid_placement",
                            first_span,
                        ));
                    }
                }
            }
        }

        // ── `:global(...)` must not lead to invalid css once removed. ──
        for relative_selector in &complex.children {
            for i in 0..relative_selector.selectors.len() {
                let selector = &relative_selector.selectors[i];
                let SimpleSelector::PseudoClass { name, args, span } = selector else {
                    continue;
                };
                if name != "global" {
                    continue;
                }
                // `:global(element)` must be at the first position in a
                // compound selector.
                let arg_first = args
                    .as_ref()
                    .and_then(|list| list.children.first())
                    .and_then(|first| first.children.first());
                if let Some(arg_first) = arg_first {
                    if matches!(
                        arg_first.selectors.first(),
                        Some(SimpleSelector::Type { .. })
                    ) && i != 0
                    {
                        return Err(CssAnalysisError::at(
                            "css_global_invalid_selector_list",
                            *span,
                        ));
                    }
                }
                // `:global(.class)` must not be followed by a type selector
                // (`:global(.class)element`).
                if let Some(next) = relative_selector.selectors.get(i + 1) {
                    if matches!(next, SimpleSelector::Type { .. }) {
                        return Err(CssAnalysisError::at(
                            "css_type_selector_invalid_placement",
                            next.span(),
                        ));
                    }
                }
                // `:global(...)` must contain a single selector (a standalone
                // `:global()` with multiple selectors is OK).
                if let Some(args) = args {
                    if args.children.len() > 1
                        && (complex.children.len() > 1 || relative_selector.selectors.len() > 1)
                    {
                        return Err(CssAnalysisError::at("css_global_invalid_selector", *span));
                    }
                }
            }
        }

        // ── The metadata. ──
        complex.metadata.is_global = complex
            .children
            .iter()
            .all(|child| child.metadata.is_global || child.metadata.is_global_like);
        complex.metadata.used |= complex.metadata.is_global;

        // Mark `&:hover` in `:global(.foo) { &:hover { color: green } }` as
        // used.
        if own.is_nested
            && matches!(
                complex
                    .children
                    .first()
                    .and_then(|child| child.selectors.first()),
                Some(SimpleSelector::Nesting { .. })
            )
        {
            let first = complex.children[0].selectors.get(1);
            let no_nesting_scope = match first {
                Some(selector @ SimpleSelector::PseudoClass { .. }) => {
                    is_unscoped_pseudo_class(selector)
                }
                _ => true,
            };
            let parent_is_global = rules
                .last()
                .is_some_and(|frame| frame.prelude_has_lone_global);
            if no_nesting_scope && parent_is_global {
                complex.metadata.used = true;
            }
        }

        Ok(())
    }

    /// The official `RelativeSelector` visitor (the leading-combinator check,
    /// the `is_global` / `is_global_like` metadata, the used-marking, then
    /// the `context.next()` descent into pseudo-class arguments + nesting
    /// selectors).
    fn analyze_relative_selector(
        &mut self,
        complex: &mut ComplexSelector,
        index: usize,
        rules: &[RuleFrame],
        own: &OwnRuleFacts,
        in_pseudo_args: bool,
    ) -> AnalyzeResult {
        {
            let relative = &complex.children[index];
            // A leading combinator is invalid on a top-level rule's first
            // compound (legal inside a pseudo-class's arguments and inside a
            // nested rule).
            if let Some(combinator) = &relative.combinator {
                if !own.is_nested && index == 0 && !in_pseudo_args {
                    return Err(CssAnalysisError::at(
                        "css_selector_invalid",
                        combinator.span,
                    ));
                }
            }
        }

        // ── The metadata. ──
        let (relative_is_global, relative_is_global_like) = {
            let relative = &complex.children[index];
            let global = !relative.selectors.is_empty() && is_global(relative);

            let mut global_like = relative.metadata.is_global_like;
            if !relative.selectors.is_empty()
                && relative.selectors.iter().all(|selector| {
                    matches!(
                        selector,
                        SimpleSelector::PseudoClass { .. } | SimpleSelector::PseudoElement { .. }
                    )
                })
            {
                global_like |= match &relative.selectors[0] {
                    SimpleSelector::PseudoClass { name, .. } => name == "host",
                    SimpleSelector::PseudoElement { name, .. } => matches!(
                        name.as_str(),
                        "view-transition"
                            | "view-transition-group"
                            | "view-transition-old"
                            | "view-transition-new"
                            | "view-transition-image-pair"
                    ),
                    _ => false,
                };
            }

            // `:root.y:has(.x)` is not global: while `.y` is unscoped, the
            // `.x` inside `:has(...)` should stay scoped.
            global_like |= relative.selectors.iter().any(
                |selector| matches!(selector, SimpleSelector::PseudoClass { name, .. } if name == "root"),
            ) && !relative.selectors.iter().any(
                |selector| matches!(selector, SimpleSelector::PseudoClass { name, .. } if name == "has"),
            );

            (global, global_like)
        };
        {
            let relative = &mut complex.children[index];
            relative.metadata.is_global = relative_is_global;
            relative.metadata.is_global_like = relative_is_global_like;

            if relative.metadata.is_global_like || relative.metadata.is_global {
                // So that nested selectors like `:root:not(.x)` are not
                // marked as unused.
                for selector in &mut relative.selectors {
                    mark_argument_selectors_used_recursive(selector);
                }
            }
        }

        // `context.next()` — the simple-selector children: nesting-selector
        // placement + the pseudo-class argument selector lists.
        for selector_index in 0..complex.children[index].selectors.len() {
            match &complex.children[index].selectors[selector_index] {
                SimpleSelector::Nesting { span } => {
                    let span = *span;
                    self.check_nesting_selector_placement(span, rules, own)?;
                }
                SimpleSelector::PseudoClass { args: Some(_), .. } => {
                    let SimpleSelector::PseudoClass {
                        args: Some(args), ..
                    } = &mut complex.children[index].selectors[selector_index]
                    else {
                        unreachable!("matched a pseudo-class with args above");
                    };
                    // Detach the argument list to walk it without aliasing
                    // the enclosing prelude, then re-attach.
                    let mut detached = std::mem::take(&mut args.children);
                    for inner in &mut detached {
                        self.analyze_complex_selector(inner, rules, own, true)?;
                    }
                    let SimpleSelector::PseudoClass {
                        args: Some(args), ..
                    } = &mut complex.children[index].selectors[selector_index]
                    else {
                        unreachable!("the selector kind is unchanged");
                    };
                    args.children = detached;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// The official `NestingSelector` visitor: a `&` is legal inside a nested
    /// rule; at the top level only as the lone `:global(&)`; inside a lone
    /// `:global {}` block a `&` modifier is invalid.
    fn check_nesting_selector_placement(
        &mut self,
        span: Span,
        rules: &[RuleFrame],
        own: &OwnRuleFacts,
    ) -> AnalyzeResult {
        match rules.last() {
            None => {
                // No parent rule: the one legal form is the lone `:global(&)`
                // (see MDN's "using `&` outside a nested rule").
                if own.allowed_nesting_span != Some(span) {
                    return Err(CssAnalysisError::at(
                        "css_nesting_selector_invalid_placement",
                        span,
                    ));
                }
            }
            Some(parent) => {
                // `:global { &.foo { ... } }` is invalid.
                if parent.is_global_block
                    && !parent.is_nested
                    && parent.prelude_first_is_lone_simple
                {
                    return Err(CssAnalysisError::at(
                        "css_global_block_invalid_modifier_start",
                        span,
                    ));
                }
            }
        }
        Ok(())
    }
}

// ── the `phases/css.js` helpers ──────────────────────────────────────────────

/// The official `remove_css_prefix`: strip a `-webkit-` / `-moz-` / `-o-` /
/// `-ms-` browser prefix.
pub(super) fn remove_css_prefix(name: &str) -> &str {
    for prefix in ["-webkit-", "-moz-", "-o-", "-ms-"] {
        if let Some(stripped) = name.strip_prefix(prefix) {
            return stripped;
        }
    }
    name
}

/// The official `is_keyframes_node`.
pub(super) fn is_keyframes_node(atrule: &Atrule) -> bool {
    remove_css_prefix(&atrule.name) == "keyframes"
}

/// The span of the keyframes NAME token inside the at-rule prelude — the
/// official renderer's scan (skip the spaces after the at-rule name, then
/// collect to the first `{` or space), anchored on the parsed `name_span`
/// end (positionally exact even for an escaped at-rule name).
pub(super) fn keyframes_name_token_span(source: &str, atrule: &Atrule) -> Span {
    let bytes = source.as_bytes();
    let mut start = atrule.name_span.end as usize;
    while bytes.get(start) == Some(&b' ') {
        start += 1;
    }
    let mut end = start;
    while end < bytes.len() && bytes[end] != b'{' && bytes[end] != b' ' {
        end += 1;
    }
    Span::new(start as u32, end as u32)
}

// ── the `css-analyze.js` local helpers ───────────────────────────────────────

/// The official `is_global_block_selector`: an ARGUMENT-LESS `:global`.
fn is_global_block_selector(selector: &SimpleSelector) -> bool {
    matches!(
        selector,
        SimpleSelector::PseudoClass {
            name,
            args: None,
            ..
        } if name == "global"
    )
}

/// The official `is_unscoped` over the ancestor-rule path: every ancestor
/// rule has global selectors (vacuously true at the top level).
fn is_unscoped(rules: &[RuleFrame]) -> bool {
    rules.iter().all(|frame| frame.has_global_selectors)
}

/// The span of the one legal top-level nesting selector — the inner `&` of a
/// lone `:global(&)` prelude — or `None` when the prelude has any other
/// shape.
fn allowed_top_level_nesting_span(rule: &Rule) -> Option<Span> {
    let children = &rule.prelude.children;
    if children.len() != 1 {
        return None;
    }
    let selectors = &children.first()?.children.first()?.selectors;
    if selectors.len() != 1 {
        return None;
    }
    let SimpleSelector::PseudoClass {
        name,
        args: Some(args),
        ..
    } = selectors.first()?
    else {
        return None;
    };
    if name != "global" {
        return None;
    }
    Some(
        args.children
            .first()?
            .children
            .first()?
            .selectors
            .first()?
            .span(),
    )
}

/// The rule-loop used-marking (`walk(selector, { ComplexSelector(node) {
/// node.metadata.used = true } })` — the visitor does NOT continue the walk,
/// so only the OUTERMOST argument selectors are marked).
fn mark_argument_selectors_used_shallow(selector: &mut SimpleSelector) {
    if let SimpleSelector::PseudoClass {
        args: Some(args), ..
    } = selector
    {
        for complex in &mut args.children {
            complex.metadata.used = true;
        }
    }
}

/// The relative-selector used-marking (`walk(child, { ComplexSelector(node,
/// context) { node.metadata.used = true; context.next(); } })` — the visitor
/// CONTINUES the walk, marking every nested argument selector).
fn mark_argument_selectors_used_recursive(selector: &mut SimpleSelector) {
    if let SimpleSelector::PseudoClass {
        args: Some(args), ..
    } = selector
    {
        for complex in &mut args.children {
            complex.metadata.used = true;
            for relative in &mut complex.children {
                for inner in &mut relative.selectors {
                    mark_argument_selectors_used_recursive(inner);
                }
            }
        }
    }
}

// ── the `css/utils.js` ports ─────────────────────────────────────────────────

/// The official `is_global`: `:global(...)` or `:global` first, and no
/// SCOPED pseudo-class after it (only unscoped pseudo-classes /
/// pseudo-elements keep the whole compound global — `:global(button).x` is
/// still scoped because of the `.x`).
pub fn is_global(relative_selector: &RelativeSelector) -> bool {
    let Some(SimpleSelector::PseudoClass { name, args, .. }) = relative_selector.selectors.first()
    else {
        return false;
    };
    name == "global"
        && (args.is_none()
            || relative_selector.selectors.iter().all(|selector| {
                is_unscoped_pseudo_class(selector)
                    || matches!(selector, SimpleSelector::PseudoElement { .. })
            }))
}

/// The official `is_unscoped_pseudo_class`: a pseudo-class that cannot be
/// (or is not) scoped. `:has` / `:is` / `:where` make the selector scoped;
/// `:not` stays unscoped unless an argument contains a multi-compound
/// selector; any of them is unscoped when every argument selector is global.
pub fn is_unscoped_pseudo_class(selector: &SimpleSelector) -> bool {
    let SimpleSelector::PseudoClass { name, args, .. } = selector else {
        return false;
    };
    let scoped_family_ok = name != "has"
        && name != "is"
        && name != "where"
        // `:not` inverses the result, so it stays unscoped — except with a
        // multi-compound argument (`:not(.x .y)`), where `.x`/`.y` should be
        // scoped.
        && (name != "not"
            || args.is_none()
            || args
                .as_ref()
                .is_some_and(|list| list.children.iter().all(|c| c.children.len() == 1)));
    scoped_family_ok
        || args.is_none()
        || args.as_ref().is_some_and(|list| {
            list.children
                .iter()
                .all(|c| c.children.iter().all(is_global))
        })
}

/// The official `is_outer_global`: `:global(...)` or `:global` first,
/// irrespective of scoped pseudo-classes after it — `:global(x):has(y)` is
/// outer-global but NOT `is_global`. Part of the faithful `css/utils.js`
/// port; its production consumer is the selector-to-template matcher
/// (`css-prune.js`'s `apply_selector`), which skips the `scoped` mark on an
/// outer-global compound when walking a complex selector against the
/// template.
pub fn is_outer_global(relative_selector: &RelativeSelector) -> bool {
    let Some(SimpleSelector::PseudoClass { name, args, .. }) = relative_selector.selectors.first()
    else {
        return false;
    };
    name == "global"
        && (args.is_none()
            || relative_selector.selectors.iter().all(|selector| {
                matches!(
                    selector,
                    SimpleSelector::PseudoClass { .. } | SimpleSelector::PseudoElement { .. }
                )
            }))
}

#[cfg(test)]
#[path = "analyze_tests.rs"]
mod analyze_tests;
