use std::sync::Arc;

use verter_css_syntax::{
    parse_component_value_tree, parse_style_ir, AttributeMatcher, CombinatorKind, ComplexSelector,
    ComplexSelectorPart, ComponentValue, ComponentValueTree, CssDialect, CssParseMode, CssSource,
    PseudoFunctionKind, SelectorCompleteness, SelectorComponent, SelectorComponentKind,
    SelectorTrust, SpecialSelectorListPseudo, StyleCompleteness, StyleStatement, TokenKind,
};
use verter_span::Span;

use super::style::{
    compute_structured_specificity, AnalyzedAtRule, AnalyzedColorCandidate, AnalyzedCssClass,
    AnalyzedCssId, AnalyzedCustomProperty, AnalyzedDeclaration, AnalyzedSelector,
    AnalyzedSpecialPseudo, AnalyzedVarUsage, AtRuleKind, AttributeOperator, AttributeSelector,
    ColorCandidateKind, CompoundSelector, CssAnalysis, CssVarFallback, CssVarReference, NumericArg,
    SelectorCombinator, SelectorPseudoClass, SpecialPseudoKind, StructuredSelector,
};

pub(super) fn parse_style_block(
    css_content: &str,
    content_offset: u32,
    dialect: CssDialect,
) -> Option<verter_css_syntax::StyleSyntaxIr> {
    let source = CssSource::new(Arc::from(css_content), content_offset).ok()?;
    parse_style_ir(source, dialect, CssParseMode::Recover).ok()
}

pub(super) fn project_style_from_ir(
    ir: &verter_css_syntax::StyleSyntaxIr,
) -> (CssAnalysis, Vec<AnalyzedSpecialPseudo>) {
    let mut projection = Projection {
        source: ir.source(),
        analysis: CssAnalysis::default(),
        special_pseudos: Vec::new(),
    };
    projection.statements(ir.statements(), None, false);
    (projection.analysis, projection.special_pseudos)
}

pub(super) fn parse_selector_authority(selector_text: &str) -> Option<StructuredSelector> {
    let wrapped = format!("{selector_text}{{}} ");
    let source = CssSource::new(Arc::from(wrapped), 0).ok()?;
    let ir = parse_style_ir(source.clone(), CssDialect::Css, CssParseMode::Recover).ok()?;
    let rule = ir
        .statements()
        .iter()
        .find_map(|statement| match statement {
            StyleStatement::Rule(rule) => Some(rule),
            _ => None,
        })?;
    let selector = rule.selector_list().selectors().first()?;
    if rule.selector_list().selectors().len() != 1 || !selector.facts().is_complete_static() {
        return None;
    }
    convert_complex(&source, selector)
}

pub(super) fn extract_var_references_authority(
    value_text: &str,
    offset_in_css: u32,
    content_offset: u32,
) -> Vec<CssVarReference> {
    let Some(origin) = content_offset.checked_add(offset_in_css) else {
        return Vec::new();
    };
    let Ok(source) = CssSource::new(Arc::from(value_text), origin) else {
        return Vec::new();
    };
    let Ok(tree) =
        parse_component_value_tree(source.clone(), CssDialect::Css, CssParseMode::Recover)
    else {
        return Vec::new();
    };
    collect_var_references(&source, tree.values())
}

struct Projection<'a> {
    source: &'a CssSource,
    analysis: CssAnalysis,
    special_pseudos: Vec<AnalyzedSpecialPseudo>,
}

impl Projection<'_> {
    fn statements(
        &mut self,
        statements: &[StyleStatement],
        parent_selector: Option<u32>,
        inside_keyframes: bool,
    ) {
        for statement in statements {
            match statement {
                StyleStatement::Rule(rule) if !inside_keyframes => {
                    self.analysis.rule_count = self.analysis.rule_count.saturating_add(1);
                    let mut first_selector = None;
                    for selector in rule.selector_list().selectors() {
                        if selector.facts().completeness() != SelectorCompleteness::Complete {
                            continue;
                        }
                        let structure = convert_complex(self.source, selector);
                        if selector.facts().trust() == SelectorTrust::Static && structure.is_none()
                        {
                            continue;
                        }
                        let selector_index = u32::try_from(self.analysis.selectors.len()).ok();
                        first_selector = first_selector.or(selector_index);
                        self.collect_selector_facts(selector, selector_index);
                        let span = trim_span(self.source, selector.span());
                        self.analysis.selectors.push(AnalyzedSelector {
                            text: self.source.slice(span).to_owned(),
                            specificity: structure.as_ref().map_or_else(
                                || selector_specificity(selector),
                                compute_structured_specificity,
                            ),
                            span,
                            structure,
                            rule_body_span: (rule.body().completeness()
                                == StyleCompleteness::Complete)
                                .then(|| rule.body().span()),
                        });
                    }
                    self.statements(rule.body().statements(), first_selector, false);
                }
                StyleStatement::Rule(_) => {}
                StyleStatement::Declaration(declaration) => {
                    if declaration.completeness() == StyleCompleteness::Complete {
                        self.declaration(
                            declaration.name_span(),
                            declaration.value(),
                            parent_selector,
                        );
                    }
                    if let Some(body) = declaration.body() {
                        self.statements(body.statements(), parent_selector, inside_keyframes);
                    }
                }
                StyleStatement::AtRule(directive) => {
                    let raw_name = self.source.slice(directive.head_span());
                    let directive_name = raw_name.trim_start_matches('@').to_ascii_lowercase();
                    let kind = classify_at_rule(&directive_name);
                    let name = if matches!(
                        kind,
                        AtRuleKind::Keyframes | AtRuleKind::Container | AtRuleKind::Property
                    ) {
                        first_opaque_atom(self.source, directive.opaque_args())
                            .unwrap_or(directive_name)
                    } else {
                        directive_name
                    };
                    self.analysis.at_rules.push(AnalyzedAtRule {
                        kind,
                        name: name.clone(),
                    });
                    if let Some(body) = directive.body() {
                        self.statements(
                            body.statements(),
                            parent_selector,
                            matches!(kind, AtRuleKind::Keyframes),
                        );
                    }
                }
                StyleStatement::MixinOrFunction(value) => {
                    if let Some(body) = value.body() {
                        self.statements(body.statements(), parent_selector, inside_keyframes);
                    }
                }
                StyleStatement::Unknown(value) => {
                    if let Some(body) = value.body() {
                        self.statements(body.statements(), parent_selector, inside_keyframes);
                    }
                }
            }
        }
    }

    fn collect_selector_facts(&mut self, selector: &ComplexSelector, selector_index: Option<u32>) {
        for part in selector.parts() {
            let ComplexSelectorPart::Compound(compound) = part else {
                continue;
            };
            for component in compound.components() {
                self.collect_component_fact(component, selector_index);
            }
        }
    }

    fn collect_component_fact(
        &mut self,
        component: &SelectorComponent,
        selector_index: Option<u32>,
    ) {
        match component.kind() {
            SelectorComponentKind::Class if component.facts().is_complete_static() => {
                if let Some(span) = component.name_span() {
                    self.analysis.classes.push(AnalyzedCssClass {
                        name: self.source.slice(span).to_owned(),
                        span,
                        selector_index,
                    });
                }
            }
            SelectorComponentKind::Id if component.facts().is_complete_static() => {
                if let Some(span) = component.name_span() {
                    self.analysis.ids.push(AnalyzedCssId {
                        name: self.source.slice(span).to_owned(),
                        span,
                    });
                }
            }
            SelectorComponentKind::PseudoClass | SelectorComponentKind::FunctionalPseudo => {
                self.collect_special_pseudo(component, selector_index);
            }
            _ => {}
        }
        for nested in component.nested_components() {
            self.collect_component_fact(nested, selector_index);
        }
        if let Some(list) = component.pseudo().and_then(|pseudo| pseudo.selector_list()) {
            for nested in list.selectors() {
                self.collect_selector_facts(nested, selector_index);
            }
        }
    }

    fn collect_special_pseudo(
        &mut self,
        component: &SelectorComponent,
        _selector_index: Option<u32>,
    ) {
        let Some(name_span) = component.name_span() else {
            return;
        };
        let ident = self.source.slice(name_span).trim_start_matches(':');
        let kind = match SpecialSelectorListPseudo::from_ident(ident) {
            Some(SpecialSelectorListPseudo::Deep) => SpecialPseudoKind::Deep,
            Some(SpecialSelectorListPseudo::Global) => SpecialPseudoKind::Global,
            Some(SpecialSelectorListPseudo::Slotted) => SpecialPseudoKind::Slotted,
            None => return,
        };
        let inner_span = component
            .pseudo()
            .map(|pseudo| trim_span(self.source, pseudo.argument_span()));
        let inner = inner_span
            .filter(|span| span.start < span.end)
            .map(|span| self.source.slice(span).to_owned());
        self.special_pseudos.push(AnalyzedSpecialPseudo {
            kind,
            start: component.span().start,
            end: component.span().end,
            inner,
        });
    }

    fn declaration(
        &mut self,
        name_span: Span,
        value: &ComponentValueTree,
        selector_index: Option<u32>,
    ) {
        let name = self.source.slice(name_span).to_owned();
        let value_span = trim_value_span(self.source, value);
        let var_references = collect_var_references(self.source, value.values());
        let color_candidates = collect_color_candidates(self.source, value.values());
        self.analysis.declarations.push(AnalyzedDeclaration {
            name_span,
            value_span,
            selector_index,
            color_candidates,
        });
        if name.starts_with("--") {
            self.analysis
                .custom_properties
                .push(AnalyzedCustomProperty {
                    name,
                    name_span,
                    value: self.source.slice(value_span).to_owned(),
                    value_span,
                    var_references,
                    selector_index,
                });
        } else {
            self.analysis
                .var_usages
                .extend(
                    var_references
                        .into_iter()
                        .map(|reference| AnalyzedVarUsage {
                            property_name: name.clone(),
                            reference,
                            selector_index,
                        }),
                );
        }
    }
}

fn selector_specificity(selector: &ComplexSelector) -> (u32, u32, u32) {
    let mut total = (0u32, 0u32, 0u32);
    for compound in selector.compounds() {
        for component in compound.components() {
            add_specificity(&mut total, component_specificity(component));
        }
    }
    total
}

fn component_specificity(component: &SelectorComponent) -> (u32, u32, u32) {
    match component.kind() {
        SelectorComponentKind::Id => (1, 0, 0),
        SelectorComponentKind::Class
        | SelectorComponentKind::Attribute
        | SelectorComponentKind::PseudoClass => (0, 1, 0),
        SelectorComponentKind::Type | SelectorComponentKind::PseudoElement => (0, 0, 1),
        SelectorComponentKind::FunctionalPseudo => {
            let Some(pseudo) = component.pseudo() else {
                return (0, 1, 0);
            };
            let nested = pseudo.selector_list().map_or((0, 0, 0), |list| {
                list.selectors()
                    .iter()
                    .map(selector_specificity)
                    .max()
                    .unwrap_or((0, 0, 0))
            });
            match pseudo.kind() {
                PseudoFunctionKind::Where => (0, 0, 0),
                PseudoFunctionKind::Is | PseudoFunctionKind::Not | PseudoFunctionKind::Has => {
                    nested
                }
                PseudoFunctionKind::NthChild | PseudoFunctionKind::NthLastChild => {
                    let mut value = (0, 1, 0);
                    add_specificity(&mut value, nested);
                    value
                }
                PseudoFunctionKind::Unknown => (0, 1, 0),
            }
        }
        SelectorComponentKind::DynamicClass
        | SelectorComponentKind::Namespace
        | SelectorComponentKind::Nesting
        | SelectorComponentKind::Interpolation => (0, 0, 0),
    }
}

fn add_specificity(total: &mut (u32, u32, u32), value: (u32, u32, u32)) {
    total.0 = total.0.saturating_add(value.0);
    total.1 = total.1.saturating_add(value.1);
    total.2 = total.2.saturating_add(value.2);
}

fn convert_complex(source: &CssSource, selector: &ComplexSelector) -> Option<StructuredSelector> {
    if selector.facts().trust() != SelectorTrust::Static {
        return None;
    }
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    for part in selector.parts() {
        match part {
            ComplexSelectorPart::Compound(compound) => {
                let mut converted = CompoundSelector::default();
                for component in compound.components() {
                    convert_component(source, component, &mut converted)?;
                }
                compounds.push(converted);
            }
            ComplexSelectorPart::Combinator(combinator) => {
                combinators.push(match combinator.kind() {
                    CombinatorKind::Descendant => SelectorCombinator::Descendant,
                    CombinatorKind::Child => SelectorCombinator::Child,
                    CombinatorKind::NextSibling => SelectorCombinator::NextSibling,
                    CombinatorKind::LaterSibling => SelectorCombinator::LaterSibling,
                    CombinatorKind::Column => return None,
                })
            }
        }
    }
    (!compounds.is_empty()).then_some(StructuredSelector {
        compounds,
        combinators,
    })
}

fn convert_component(
    source: &CssSource,
    component: &SelectorComponent,
    output: &mut CompoundSelector,
) -> Option<()> {
    let name = || {
        component
            .name_span()
            .map(|span| source.slice(span).to_owned())
    };
    match component.kind() {
        SelectorComponentKind::Type => output.element = name(),
        SelectorComponentKind::Class => output.classes.push(name()?),
        SelectorComponentKind::Id => output.id = name(),
        SelectorComponentKind::Attribute => {
            let attribute = component.attribute()?;
            let operator = attribute.matcher().map(|matcher| match matcher {
                AttributeMatcher::Exact => AttributeOperator::Equal,
                AttributeMatcher::Includes => AttributeOperator::Includes,
                AttributeMatcher::DashMatch => AttributeOperator::DashMatch,
                AttributeMatcher::Prefix => AttributeOperator::Prefix,
                AttributeMatcher::Suffix => AttributeOperator::Suffix,
                AttributeMatcher::Substring => AttributeOperator::Substring,
            });
            output.attributes.push(AttributeSelector {
                name: source.slice(attribute.name_span()?).to_owned(),
                operator,
                value: attribute
                    .value_span()
                    .map(|span| source.slice(span).trim_matches(['"', '\'']).to_owned()),
            });
        }
        SelectorComponentKind::PseudoElement => output.has_pseudo_element = true,
        SelectorComponentKind::PseudoClass | SelectorComponentKind::FunctionalPseudo => {
            let pseudo_name = name()?.trim_start_matches(':').to_owned();
            let pseudo = component.pseudo();
            let kind = pseudo.map_or(PseudoFunctionKind::Unknown, |value| value.kind());
            let convert_nested = || -> Option<Vec<StructuredSelector>> {
                pseudo?
                    .selector_list()?
                    .selectors()
                    .iter()
                    .map(|selector| convert_complex(source, selector))
                    .collect()
            };
            output.pseudo_classes.push(match kind {
                PseudoFunctionKind::Not => SelectorPseudoClass::Not(convert_nested()?),
                PseudoFunctionKind::Is => SelectorPseudoClass::Is(convert_nested()?),
                PseudoFunctionKind::Where => SelectorPseudoClass::Where(convert_nested()?),
                PseudoFunctionKind::Has => return None,
                _ => SelectorPseudoClass::Runtime(pseudo_name),
            });
        }
        SelectorComponentKind::Namespace => {}
        SelectorComponentKind::DynamicClass
        | SelectorComponentKind::Nesting
        | SelectorComponentKind::Interpolation => return None,
    }
    Some(())
}

fn classify_at_rule(name: &str) -> AtRuleKind {
    match name {
        "media" => AtRuleKind::Media,
        "keyframes" | "-webkit-keyframes" => AtRuleKind::Keyframes,
        "supports" => AtRuleKind::Supports,
        "import" | "use" | "forward" | "plugin" => AtRuleKind::Import,
        "layer" => AtRuleKind::Layer,
        "container" => AtRuleKind::Container,
        "font-face" => AtRuleKind::FontFace,
        "property" => AtRuleKind::Property,
        "scope" => AtRuleKind::Scope,
        _ => AtRuleKind::Other,
    }
}

fn first_opaque_atom(source: &CssSource, tree: &ComponentValueTree) -> Option<String> {
    tree.values().iter().find_map(|value| match value {
        ComponentValue::Token(token) if !token.kind().is_trivia() => {
            Some(source.slice(token.span()).to_owned())
        }
        ComponentValue::String(token) => Some(source.slice(token.span()).to_owned()),
        _ => None,
    })
}

fn collect_var_references(source: &CssSource, values: &[ComponentValue]) -> Vec<CssVarReference> {
    let mut output = Vec::new();
    for value in values {
        match value {
            ComponentValue::Function(function) => {
                if function.is_complete()
                    && source
                        .slice(function.name_span())
                        .eq_ignore_ascii_case("var")
                {
                    if let Some(reference) =
                        build_var_reference(source, function.full_span(), function.values())
                    {
                        output.push(reference);
                    }
                } else {
                    output.extend(collect_var_references(source, function.values()));
                }
            }
            ComponentValue::Block(block) => {
                output.extend(collect_var_references(source, block.values()));
            }
            ComponentValue::Interpolation(interpolation) => {
                output.extend(collect_var_references(source, interpolation.values()));
            }
            ComponentValue::Token(_) | ComponentValue::String(_) | ComponentValue::Comment(_) => {}
        }
    }
    output
}

/// Walks a declaration value's own `ComponentValue` tree for color-literal
/// candidates: `#`-prefixed hash tokens and `rgb`/`rgba`/`hsl`/`hsla`
/// function calls (matched case-insensitively). Comment and string exclusion
/// is structural — the match below has no arm for
/// `ComponentValue::Comment`/`ComponentValue::String`, so their content is
/// never visited, never a byte mask ported from a raw-substring scan.
fn collect_color_candidates(
    source: &CssSource,
    values: &[ComponentValue],
) -> Vec<AnalyzedColorCandidate> {
    let mut output = Vec::new();
    collect_color_candidates_into(source, values, &mut output);
    output
}

fn collect_color_candidates_into(
    source: &CssSource,
    values: &[ComponentValue],
    output: &mut Vec<AnalyzedColorCandidate>,
) {
    for value in values {
        match value {
            ComponentValue::Token(token) if token.kind() == TokenKind::Hash => {
                output.push(AnalyzedColorCandidate {
                    span: token.span(),
                    kind: ColorCandidateKind::Hex,
                    function_name: None,
                    numeric_args: Vec::new(),
                });
            }
            ComponentValue::Function(function) => {
                let name = source.slice(function.name_span());
                if is_color_function_name(name) {
                    output.push(AnalyzedColorCandidate {
                        span: function.full_span(),
                        kind: ColorCandidateKind::Function,
                        function_name: Some(name.to_ascii_lowercase()),
                        numeric_args: extract_numeric_args(source, function.values())
                            .unwrap_or_default(),
                    });
                } else {
                    // Not a color function itself — a color literal may
                    // still be nested inside its arguments (e.g.
                    // `linear-gradient(rgb(255, 0, 0), ...)`).
                    collect_color_candidates_into(source, function.values(), output);
                }
            }
            ComponentValue::Block(block) => {
                collect_color_candidates_into(source, block.values(), output);
            }
            ComponentValue::Interpolation(interpolation) => {
                collect_color_candidates_into(source, interpolation.values(), output);
            }
            ComponentValue::Token(_) | ComponentValue::String(_) | ComponentValue::Comment(_) => {}
        }
    }
}

fn is_color_function_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("rgb")
        || name.eq_ignore_ascii_case("rgba")
        || name.eq_ignore_ascii_case("hsl")
        || name.eq_ignore_ascii_case("hsla")
}

/// Reads numeric arguments directly from the function's own
/// `ComponentValue` list, skipping `Comment` entries structurally — never by
/// re-slicing the candidate's raw byte span and `.split(',')`/`.parse()`ing
/// it (the bug this record fixes: a comment between arguments breaks a
/// raw-substring parse and silently drops every argument after it).
///
/// Returns `None` — invalidating the WHOLE candidate, never a truncated
/// partial list — the instant ANY component isn't a `Number` token, a
/// `Percentage` token, whitespace, a comma delimiter, or a comment: an
/// identifier (`from`, CSS relative-color syntax), a nested function
/// (`calc()`, `min()`), a block, an interpolation, or a string are all out
/// of scope, and a candidate built from only the numbers that happen to
/// surround an out-of-scope component would fabricate a color for a shape
/// this producer does not actually support (e.g. `rgb(from red 255 0 0)`
/// must not resolve to `rgb(255, 0, 0)`). Percentage tokens preserve their
/// `%` suffix as [`NumericArg::Percentage`] (the `%` is stripped from the
/// text but never divided out) so the caller can normalize a percentage
/// scale distinctly from a bare number.
fn extract_numeric_args(source: &CssSource, values: &[ComponentValue]) -> Option<Vec<NumericArg>> {
    let mut args = Vec::new();
    for value in values {
        match value {
            ComponentValue::Token(token) => match token.kind() {
                TokenKind::Number => {
                    let parsed = source.slice(token.span()).parse::<f64>().ok()?;
                    args.push(NumericArg::Number(parsed));
                }
                TokenKind::Percentage => {
                    let text = source.slice(token.span());
                    let parsed = text.trim_end_matches('%').parse::<f64>().ok()?;
                    args.push(NumericArg::Percentage(parsed));
                }
                TokenKind::Whitespace | TokenKind::Comma => {}
                _ => return None,
            },
            ComponentValue::Comment(_) => {}
            ComponentValue::Function(_)
            | ComponentValue::Block(_)
            | ComponentValue::Interpolation(_)
            | ComponentValue::String(_) => return None,
        }
    }
    Some(args)
}

fn build_var_reference(
    source: &CssSource,
    full_span: Span,
    values: &[ComponentValue],
) -> Option<CssVarReference> {
    let comma = values.iter().position(
        |value| matches!(value, ComponentValue::Token(token) if token.kind() == TokenKind::Comma),
    );
    let name_span =
        values[..comma.unwrap_or(values.len())]
            .iter()
            .find_map(|value| match value {
                ComponentValue::Token(token)
                    if token.kind() == TokenKind::Ident
                        && source.slice(token.span()).starts_with("--") =>
                {
                    Some(token.span())
                }
                _ => None,
            })?;
    let fallback = comma.and_then(|index| {
        let fallback_values = &values[index + 1..];
        let span = span_of_values(source, fallback_values)?;
        Some(CssVarFallback {
            text: source.slice(span).to_owned(),
            span,
            nested_var_references: collect_var_references(source, fallback_values),
        })
    });
    Some(CssVarReference {
        name: source.slice(name_span).to_owned(),
        span: full_span,
        name_span,
        fallback,
    })
}

fn trim_value_span(source: &CssSource, tree: &ComponentValueTree) -> Span {
    span_of_values(source, tree.values())
        .unwrap_or_else(|| Span::new(tree.span().end, tree.span().end))
}

fn span_of_values(source: &CssSource, values: &[ComponentValue]) -> Option<Span> {
    let first = values.iter().find(|value| !value_is_trivia(value))?;
    let last = values.iter().rfind(|value| !value_is_trivia(value))?;
    Some(trim_span(
        source,
        Span::new(first.span().start, last.span().end),
    ))
}

fn value_is_trivia(value: &ComponentValue) -> bool {
    matches!(value, ComponentValue::Token(token) if token.kind() == TokenKind::Whitespace)
}

fn trim_span(source: &CssSource, span: Span) -> Span {
    let text = source.slice(span);
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len();
    Span::new(
        span.start
            .saturating_add(u32::try_from(start).unwrap_or(u32::MAX)),
        span.start
            .saturating_add(u32::try_from(end).unwrap_or(u32::MAX)),
    )
}
