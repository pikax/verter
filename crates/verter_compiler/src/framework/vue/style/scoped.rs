//! Scoped-style planning: `<style scoped>` selector rewriting, `:deep()`,
//! `:global()`, `:slotted()` and scoped keyframes.

#[cfg(test)]
use crate::framework::vue::style::cascade::StyleRewriteOutcome;
use crate::framework::vue::style::cascade::{StyleEdit, VueStyleFacts};
#[cfg(test)]
use crate::style_planner::{emit, parse_ir, record_input_dependencies, PlainCssInput};
use crate::style_planner::{StyleRewriteFailure, StyleRewriteFailureClass, StyleRewriteStage};
use verter_css_syntax::{
    css_identifier_eq_ignore_ascii_case, CombinatorKind, ComplexSelector, ComplexSelectorPart,
    ComponentValue, CssDialect, CssSource, SelectorComponent, SelectorComponentKind, SelectorList,
    SelectorPseudo, StyleCompleteness, StyleDeclaration, StyleDirective, StyleStatement,
    StyleSyntaxIr, TokenKind, UnknownStatement,
};
use verter_span::Span;

/// Collects the scoped-selector/keyframes edits and facts from an
/// already-parsed IR, without itself calling `parse_ir` — the shared building
/// block `run_vue_style_cascade` (reuses a retained IR across stages) and the
/// test-only isolated scoped instrument (parses then delegates here) both
/// route through.
pub(crate) fn scoped_edits_and_facts_from_ir(
    ir: &StyleSyntaxIr,
    scope_id: &str,
) -> Result<(Vec<StyleEdit>, VueStyleFacts), StyleRewriteFailure> {
    let mut planner = VueScopePlanner {
        source: ir.source(),
        scope_attr: format!("[data-v-{scope_id}]"),
        slotted_attr: format!("[data-v-{scope_id}-s]"),
        scope_id,
        edits: Vec::new(),
        facts: VueStyleFacts::default(),
        keyframes: Vec::new(),
    };
    planner.collect_keyframes(ir.statements())?;
    planner.plan_statements(ir.statements(), false)?;
    Ok((planner.edits, planner.facts))
}

/// Crate-test instrument: parse a `PlainCssInput` and materialize only the
/// scoped-selector stage's edits.
///
/// Not a production route. Shipped `<style scoped>` requests plan this stage
/// through `run_vue_style_cascade` over the one `StyleSyntaxIr` the run already
/// parsed. This entry exists so crate tests can compare a merged plan against
/// applying stages one after another. It is available only under `#[cfg(test)]`,
/// never through the `test-support` Cargo feature. Compiling it into any
/// non-test library build, or giving it a production caller, reintroduces the
/// second CSS parse and the staged coordinate spaces the shared plan removed.
#[cfg(test)]
pub(crate) fn transform_vue_scoped_css(
    input: PlainCssInput<'_>,
    scope_id: &str,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let ir = parse_ir(
        input.code,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessScoping,
    )?;
    let (edits, mut facts) = scoped_edits_and_facts_from_ir(&ir, scope_id)?;
    record_input_dependencies(&mut facts, &ir);
    emit(
        input.code,
        input.source,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessScoping,
        edits,
        facts,
        input.want_source_map,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VueSpecialPseudo {
    Global,
    Deep,
    Slotted,
}

/// Whether `component` is the functional pseudo-class `:name(...)`, matched by
/// the component's kind first so a class or type selector that happens to
/// share the name (`.where`, `.deep`) is never read as the pseudo.
fn is_functional_pseudo_named(
    source: &CssSource,
    component: &SelectorComponent,
    name: &str,
) -> bool {
    component.kind() == SelectorComponentKind::FunctionalPseudo
        && component.name_span().is_some_and(|span| {
            css_identifier_eq_ignore_ascii_case(source.slice(span).trim_end_matches('('), name)
        })
}

pub(crate) struct VueScopePlanner<'a> {
    source: &'a CssSource,
    scope_attr: String,
    slotted_attr: String,
    scope_id: &'a str,
    edits: Vec<StyleEdit>,
    facts: VueStyleFacts,
    keyframes: Vec<(String, String)>,
}

impl VueScopePlanner<'_> {
    fn collect_keyframes(
        &mut self,
        statements: &[StyleStatement],
    ) -> Result<(), StyleRewriteFailure> {
        for statement in statements {
            match statement {
                StyleStatement::AtRule(rule) => {
                    if self.is_keyframes(rule) {
                        if rule.completeness() != StyleCompleteness::Complete
                            || rule.opaque_args().completeness() != StyleCompleteness::Complete
                        {
                            return Err(self.untrusted(rule.span()));
                        }
                        let name = rule
                            .opaque_args()
                            .values()
                            .iter()
                            .find_map(|value| match value {
                                ComponentValue::Token(token)
                                    if token.kind() == TokenKind::Ident =>
                                {
                                    Some((self.source.slice(token.span()), token.span()))
                                }
                                _ => None,
                            })
                            .ok_or_else(|| self.untrusted(rule.head_span()))?;
                        let renamed = format!("{}-{}", name.0, self.scope_id);
                        self.edits.push(StyleEdit::Overwrite {
                            span: name.1,
                            content: renamed.clone(),
                        });
                        self.keyframes.push((name.0.to_string(), renamed));
                        self.facts.rewrites.keyframes = true;
                    }
                    if let Some(body) = rule.body() {
                        self.collect_keyframes(body.statements())?;
                    }
                }
                StyleStatement::Rule(rule) => self.collect_keyframes(rule.body().statements())?,
                StyleStatement::Declaration(rule) => {
                    if let Some(body) = rule.body() {
                        self.collect_keyframes(body.statements())?;
                    }
                }
                StyleStatement::MixinOrFunction(rule) => {
                    if let Some(body) = rule.body() {
                        self.collect_keyframes(body.statements())?;
                    }
                }
                StyleStatement::Unknown(rule) => {
                    if let Some(body) = rule.body() {
                        self.collect_keyframes(body.statements())?;
                    }
                }
            }
        }
        Ok(())
    }

    fn plan_statements(
        &mut self,
        statements: &[StyleStatement],
        inside_keyframes: bool,
    ) -> Result<(), StyleRewriteFailure> {
        for statement in statements {
            match statement {
                StyleStatement::Rule(rule) => {
                    let edits_checkpoint = self.edits.len();
                    let facts_checkpoint = self.facts.clone();
                    let refusal_checkpoint = facts_checkpoint.refusals.len();
                    let planned = (|| {
                        if !inside_keyframes {
                            if rule.completeness() != StyleCompleteness::Complete
                                || !selector_list_is_trusted_for_scoping(rule.selector_list())
                            {
                                return Err(self.untrusted(rule.selector_list().span()));
                            }
                            for selector in rule.selector_list().selectors() {
                                self.plan_selector(selector)?;
                            }
                        }
                        self.plan_statements(rule.body().statements(), inside_keyframes)?;
                        if self.facts.refusals.len() > refusal_checkpoint {
                            return Err(self
                                .facts
                                .refusals
                                .last()
                                .cloned()
                                .unwrap_or_else(|| self.untrusted(rule.span())));
                        }
                        Ok(())
                    })();
                    if let Err(refusal) = planned {
                        self.edits.truncate(edits_checkpoint);
                        self.facts = facts_checkpoint;
                        self.edits.push(StyleEdit::Overwrite {
                            span: rule.span(),
                            content: String::new(),
                        });
                        self.facts.refusals.push(refusal);
                    }
                }
                StyleStatement::Declaration(declaration) => {
                    let edits_checkpoint = self.edits.len();
                    let facts_checkpoint = self.facts.clone();
                    let planned = (|| {
                        self.plan_animation_declaration(declaration)?;
                        if let Some(body) = declaration.body() {
                            self.plan_statements(body.statements(), inside_keyframes)?;
                        }
                        Ok(())
                    })();
                    if let Err(refusal) = planned {
                        self.edits.truncate(edits_checkpoint);
                        self.facts = facts_checkpoint;
                        self.edits.push(StyleEdit::Overwrite {
                            span: declaration.span(),
                            content: String::new(),
                        });
                        self.facts.refusals.push(refusal);
                    }
                }
                StyleStatement::AtRule(rule) => {
                    if let Some(body) = rule.body() {
                        let edits_checkpoint = self.edits.len();
                        let facts_checkpoint = self.facts.clone();
                        if let Err(refusal) = self.plan_statements(
                            body.statements(),
                            inside_keyframes || self.is_keyframes(rule),
                        ) {
                            self.edits.truncate(edits_checkpoint);
                            self.facts = facts_checkpoint;
                            self.edits.push(StyleEdit::Overwrite {
                                span: rule.span(),
                                content: String::new(),
                            });
                            self.facts.refusals.push(refusal);
                        }
                    }
                }
                StyleStatement::MixinOrFunction(rule) => {
                    if let Some(body) = rule.body() {
                        self.plan_statements(body.statements(), inside_keyframes)?;
                    }
                }
                StyleStatement::Unknown(unknown) => {
                    if !inside_keyframes && unknown_may_contain_selector(unknown) {
                        return Err(self.untrusted(unknown.span()));
                    }
                    if let Some(body) = unknown.body() {
                        self.plan_statements(body.statements(), inside_keyframes)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn plan_selector(&mut self, selector: &ComplexSelector) -> Result<(), StyleRewriteFailure> {
        if !selector_is_trusted_for_scoping(selector) {
            return Err(self.untrusted(selector.span()));
        }
        let checkpoint = self.edits.len();
        let mut edits = std::mem::take(&mut self.edits);
        let has_special = self.collect_special_selector_edits(selector, &mut edits)?;
        if has_special {
            self.edits = edits;
            self.facts.rewrites.scoped_selector = true;
            return Ok(());
        }
        edits.truncate(checkpoint);
        let scope_attr = self.scope_attr.clone();
        let planned = self.collect_selector_scope_edits(selector, &scope_attr, &mut edits);
        self.edits = edits;
        planned?;
        self.facts.rewrites.scoped_selector = true;
        Ok(())
    }

    fn collect_special_selector_edits(
        &mut self,
        selector: &ComplexSelector,
        edits: &mut Vec<StyleEdit>,
    ) -> Result<bool, StyleRewriteFailure> {
        let mut found = false;
        let edits_checkpoint = edits.len();
        let mut previous_compound = None;
        let parts = selector.parts();
        let mut index = 0;
        while index < parts.len() {
            let part = &parts[index];
            match part {
                ComplexSelectorPart::Compound(compound) => {
                    for (component_index, component) in compound.components().iter().enumerate() {
                        let Some(pseudo) = component.pseudo() else {
                            continue;
                        };
                        let special_name = self.vue_special_pseudo_name(component);
                        if special_name == Some(VueSpecialPseudo::Global) {
                            // `:global()` with nothing to unwrap is a refusal,
                            // not an erasure: dropping the selector would leave
                            // the declarations attached to whatever followed.
                            let argument = self.trusted_first_selector_argument(pseudo)?;
                            edits.truncate(edits_checkpoint);
                            self.collect_unwrapped_argument_edits(
                                selector_content_span(selector),
                                argument.span(),
                                String::new(),
                                edits,
                            );
                            self.facts.rewrites.global = true;
                            return Ok(true);
                        }
                        if special_name == Some(VueSpecialPseudo::Deep) {
                            let argument = self.trusted_selector_argument_allowing_empty(pseudo)?;
                            let same_compound_anchor = component_index > 0;
                            let anchor = same_compound_anchor
                                .then_some(compound)
                                .or(previous_compound);
                            let prefix = if let Some(anchor) = anchor {
                                edits.push(StyleEdit::Insert {
                                    at: self.scope_insertion(anchor),
                                    content: self.scope_attr.clone(),
                                });
                                if same_compound_anchor {
                                    " ".to_string()
                                } else {
                                    String::new()
                                }
                            } else {
                                format!("{} ", self.scope_attr)
                            };
                            if let Some(argument) = argument {
                                self.collect_unwrapped_argument_edits(
                                    component.span(),
                                    argument.span(),
                                    prefix,
                                    edits,
                                );
                            } else {
                                edits.push(StyleEdit::Overwrite {
                                    span: component.span(),
                                    content: prefix,
                                });
                            }
                            self.facts.rewrites.deep = true;
                            return Ok(true);
                        }
                        if special_name == Some(VueSpecialPseudo::Slotted) {
                            self.collect_slotted_component_edits(component, pseudo, edits)?;
                            self.facts.rewrites.slotted = true;
                            return Ok(true);
                        }

                        let Some(selector_list) = pseudo.selector_list() else {
                            continue;
                        };
                        if !selector_list_is_trusted_for_scoping(selector_list) {
                            return Err(self.untrusted(selector_list.span()));
                        }
                        for nested in selector_list.selectors() {
                            found |= self.collect_special_selector_edits(nested, edits)?;
                        }
                    }
                    previous_compound = Some(compound);
                }
                ComplexSelectorPart::Combinator(combinator) => {
                    let mut deep_span = None;
                    if combinator.kind() == CombinatorKind::Child && index + 2 < parts.len() {
                        if let (
                            ComplexSelectorPart::Combinator(second),
                            ComplexSelectorPart::Combinator(third),
                        ) = (&parts[index + 1], &parts[index + 2])
                        {
                            if second.kind() == CombinatorKind::Child
                                && third.kind() == CombinatorKind::Child
                            {
                                let before = previous_compound
                                    .ok_or_else(|| self.untrusted(selector.span()))?;
                                let after = parts.get(index + 3).and_then(|part| match part {
                                    ComplexSelectorPart::Compound(compound) => Some(compound),
                                    ComplexSelectorPart::Combinator(_) => None,
                                });
                                let after = after.ok_or_else(|| self.untrusted(selector.span()))?;
                                deep_span =
                                    Some((Span::new(before.span().end, after.span().start), 3));
                            }
                        }
                    }
                    if let Some((span, consumed)) = deep_span {
                        if !found {
                            let compound =
                                previous_compound.ok_or_else(|| self.untrusted(selector.span()))?;
                            edits.push(StyleEdit::Insert {
                                at: self.scope_insertion(compound),
                                content: self.scope_attr.clone(),
                            });
                        }
                        edits.push(StyleEdit::Overwrite {
                            span,
                            content: " ".to_string(),
                        });
                        self.facts.rewrites.deep = true;
                        found = true;
                        index += consumed;
                        continue;
                    }
                }
            }
            index += 1;
        }
        Ok(found)
    }

    fn collect_unwrapped_argument_edits(
        &self,
        outer: Span,
        argument: Span,
        prefix: String,
        edits: &mut Vec<StyleEdit>,
    ) {
        if outer.start < argument.start || !prefix.is_empty() {
            edits.push(StyleEdit::Overwrite {
                span: Span::new(outer.start, argument.start),
                content: prefix,
            });
        }
        if argument.end < outer.end {
            edits.push(StyleEdit::Overwrite {
                span: Span::new(argument.end, outer.end),
                content: String::new(),
            });
        }
    }

    /// Rewrites `:slotted(<arg>)` to `<arg>` with the slotted scope attribute
    /// inserted, expressed entirely as edits against the outer source: the
    /// `:slotted(` prefix and the `)` suffix are deleted in place, and the
    /// argument's scope-attribute inserts (already carrying absolute source
    /// offsets) go into the same outer edit vector verbatim. The argument is
    /// never rendered to an intermediate string, so no occurrence-local
    /// allocator, transform, or splice exists — the outer `emit` remains the
    /// sole edit applier and source-map producer.
    ///
    /// The three edit groups cannot overlap: the prefix overwrite ends at
    /// `argument_span.start`, every argument insert lands inside
    /// `[argument_span.start, argument_span.end]`, and the suffix overwrite
    /// starts at `argument_span.end`; inserts are zero-width, so `emit`'s
    /// `start < previous_end` guard accepts the boundary-touching cases.
    fn collect_slotted_component_edits(
        &self,
        component: &SelectorComponent,
        pseudo: &SelectorPseudo,
        edits: &mut Vec<StyleEdit>,
    ) -> Result<(), StyleRewriteFailure> {
        // `:slotted()` with nothing to unwrap refuses for the same reason
        // `:global()` does — there is no selector to carry the declarations.
        let selector = self.trusted_first_selector_argument(pseudo)?;
        let component_span = component.span();
        let argument_span = selector.span();
        edits.push(StyleEdit::Overwrite {
            span: Span::new(component_span.start, argument_span.start),
            content: String::new(),
        });
        self.collect_selector_scope_edits(selector, &self.slotted_attr, edits)?;
        edits.push(StyleEdit::Overwrite {
            span: Span::new(argument_span.end, component_span.end),
            content: String::new(),
        });
        Ok(())
    }

    fn collect_selector_scope_edits(
        &self,
        selector: &ComplexSelector,
        scope_attr: &str,
        edits: &mut Vec<StyleEdit>,
    ) -> Result<(), StyleRewriteFailure> {
        if !selector_is_trusted_for_scoping(selector) {
            return Err(self.untrusted(selector.span()));
        }
        let compound = selector
            .parts()
            .iter()
            .rev()
            .find_map(|part| match part {
                ComplexSelectorPart::Compound(value) => Some(value),
                ComplexSelectorPart::Combinator(_) => None,
            })
            .ok_or_else(|| self.untrusted(selector.span()))?;
        let components = compound.components();
        if components.len() == 1 {
            let component = &components[0];
            if is_functional_pseudo_named(self.source, component, "is")
                || is_functional_pseudo_named(self.source, component, "where")
            {
                let nested = component
                    .pseudo()
                    .and_then(SelectorPseudo::selector_list)
                    .ok_or_else(|| self.untrusted(component.span()))?;
                for selector in nested.selectors() {
                    self.collect_selector_scope_edits(selector, scope_attr, edits)?;
                }
                return Ok(());
            }
        }
        edits.push(StyleEdit::Insert {
            at: self.scope_insertion(compound),
            content: scope_attr.to_string(),
        });
        Ok(())
    }

    fn scope_insertion(&self, compound: &verter_css_syntax::SelectorCompound) -> u32 {
        compound
            .components()
            .iter()
            .find(|component| {
                matches!(
                    component.kind(),
                    SelectorComponentKind::PseudoClass
                        | SelectorComponentKind::PseudoElement
                        | SelectorComponentKind::FunctionalPseudo
                )
            })
            .map_or(compound.span().end, |component| component.span().start)
    }

    fn plan_animation_declaration(
        &mut self,
        declaration: &StyleDeclaration,
    ) -> Result<(), StyleRewriteFailure> {
        if self.keyframes.is_empty() {
            return Ok(());
        }
        let property = self.source.slice(declaration.name_span());
        if !css_identifier_eq_ignore_ascii_case(property, "animation")
            && !css_identifier_eq_ignore_ascii_case(property, "animation-name")
            && !css_identifier_eq_ignore_ascii_case(property, "-webkit-animation")
            && !css_identifier_eq_ignore_ascii_case(property, "-webkit-animation-name")
        {
            return Ok(());
        }
        if declaration.completeness() != StyleCompleteness::Complete
            || declaration.value().completeness() != StyleCompleteness::Complete
        {
            return Err(self.untrusted(declaration.span()));
        }
        collect_animation_edits(
            declaration.value().values(),
            self.source,
            &self.keyframes,
            &mut self.edits,
        );
        Ok(())
    }

    fn is_keyframes(&self, rule: &StyleDirective) -> bool {
        let head = self.source.slice(rule.head_span());
        css_identifier_eq_ignore_ascii_case(head, "@keyframes")
            || css_identifier_eq_ignore_ascii_case(head, "@-webkit-keyframes")
    }

    fn vue_special_pseudo_name(&self, component: &SelectorComponent) -> Option<VueSpecialPseudo> {
        component.pseudo()?.selector_list()?;
        if is_functional_pseudo_named(self.source, component, "global")
            || is_functional_pseudo_named(self.source, component, "v-global")
        {
            Some(VueSpecialPseudo::Global)
        } else if is_functional_pseudo_named(self.source, component, "deep")
            || is_functional_pseudo_named(self.source, component, "v-deep")
        {
            Some(VueSpecialPseudo::Deep)
        } else if is_functional_pseudo_named(self.source, component, "slotted")
            || is_functional_pseudo_named(self.source, component, "v-slotted")
        {
            Some(VueSpecialPseudo::Slotted)
        } else {
            None
        }
    }

    /// The pseudo's first argument selector, for the pseudos whose rewrite has
    /// no meaning without one. An absent or untrusted argument is a refusal,
    /// so callers get a selector or an error and never a third case to spell.
    fn trusted_first_selector_argument<'a>(
        &self,
        pseudo: &'a SelectorPseudo,
    ) -> Result<&'a ComplexSelector, StyleRewriteFailure> {
        let argument_span = pseudo.argument_span();
        self.trusted_selector_argument_allowing_empty(pseudo)?
            .ok_or_else(|| self.untrusted(argument_span))
    }

    /// The `::v-deep`/`:deep()` variant: an argument-less `:deep` is the
    /// documented bare form (`:deep .child`), so an empty argument list over
    /// whitespace-only bytes is a real answer rather than a refusal. Anything
    /// else inside the parentheses that did not parse into a trusted selector
    /// still refuses.
    fn trusted_selector_argument_allowing_empty<'a>(
        &self,
        pseudo: &'a SelectorPseudo,
    ) -> Result<Option<&'a ComplexSelector>, StyleRewriteFailure> {
        let argument_span = pseudo.argument_span();
        let selector_list = pseudo
            .selector_list()
            .ok_or_else(|| self.untrusted(argument_span))?;
        if !selector_list_is_trusted_for_scoping(selector_list) {
            return Err(self.untrusted(argument_span));
        }
        match selector_list.selectors().first() {
            Some(selector) => Ok(Some(selector)),
            None if self.source.slice(argument_span).trim().is_empty() => Ok(None),
            None => Err(self.untrusted(argument_span)),
        }
    }

    fn untrusted(&self, span: Span) -> StyleRewriteFailure {
        StyleRewriteFailure::new(
            StyleRewriteFailureClass::UntrustedRewriteTarget,
            StyleRewriteStage::PostPreprocessScoping,
            CssDialect::Css,
            Some(span),
        )
    }
}

pub(crate) fn collect_animation_edits(
    values: &[ComponentValue],
    source: &CssSource,
    keyframes: &[(String, String)],
    edits: &mut Vec<StyleEdit>,
) {
    for value in values {
        match value {
            ComponentValue::Token(token) if token.kind() == TokenKind::Ident => {
                let text = source.slice(token.span());
                if let Some((_, renamed)) = keyframes.iter().find(|(name, _)| name == text) {
                    edits.push(StyleEdit::Overwrite {
                        span: token.span(),
                        content: renamed.clone(),
                    });
                }
            }
            ComponentValue::Function(function) => {
                collect_animation_edits(function.values(), source, keyframes, edits)
            }
            ComponentValue::Block(block) => {
                collect_animation_edits(block.values(), source, keyframes, edits)
            }
            ComponentValue::Token(_)
            | ComponentValue::String(_)
            | ComponentValue::Comment(_)
            | ComponentValue::Interpolation(_) => {}
        }
    }
}

pub(crate) fn unknown_may_contain_selector(unknown: &UnknownStatement) -> bool {
    unknown.span().start < unknown.span().end
}

pub(crate) fn selector_content_span(selector: &ComplexSelector) -> Span {
    let end = selector
        .parts()
        .last()
        .map_or(selector.span().end, |part| match part {
            ComplexSelectorPart::Compound(compound) => compound.span().end,
            ComplexSelectorPart::Combinator(combinator) => combinator.span().end,
        });
    Span::new(selector.span().start, end)
}

pub(crate) fn selector_list_is_trusted_for_scoping(selector_list: &SelectorList) -> bool {
    matches!(
        selector_list.facts().completeness(),
        verter_css_syntax::SelectorCompleteness::Complete
    ) && selector_list
        .selectors()
        .iter()
        .all(selector_is_trusted_for_scoping)
}

pub(crate) fn selector_is_trusted_for_scoping(selector: &ComplexSelector) -> bool {
    matches!(
        selector.facts().completeness(),
        verter_css_syntax::SelectorCompleteness::Complete
    ) && selector.parts().iter().all(|part| match part {
        ComplexSelectorPart::Combinator(_) => true,
        ComplexSelectorPart::Compound(compound) => compound.components().iter().all(|component| {
            !matches!(
                component.kind(),
                SelectorComponentKind::DynamicClass | SelectorComponentKind::Interpolation
            ) && component
                .pseudo()
                .and_then(SelectorPseudo::selector_list)
                .is_none_or(selector_list_is_trusted_for_scoping)
        }),
    })
}
