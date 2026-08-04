use std::sync::Arc;

use verter_css_syntax::{
    parse_component_value_tree, parse_selector_structure, parse_style_ir, AttributeMatcher,
    CombinatorKind, ComplexSelector, ComplexSelectorPart, ComponentValue, ComponentValueTree,
    CssDialect, CssParseMode, CssSource, PseudoFunctionKind, SelectorComponent,
    SelectorComponentKind, SelectorTrust, StyleCompleteness, StyleStatement, TokenKind,
};
use verter_span::Span;

use super::style::{
    compute_structured_specificity, AnalyzedAtRule, AnalyzedCssClass, AnalyzedCssId,
    AnalyzedCustomProperty, AnalyzedSelector, AnalyzedSpecialPseudo, AnalyzedVarUsage, AtRuleKind,
    AttributeOperator, AttributeSelector, CompoundSelector, CssAnalysis, CssVarFallback,
    CssVarReference, SelectorCombinator, SelectorPseudoClass, SpecialPseudoKind,
    StructuredSelector,
};

pub(super) fn project_style(
    css_content: &str,
    content_offset: u32,
    dialect: CssDialect,
) -> Option<(CssAnalysis, Vec<AnalyzedSpecialPseudo>)> {
    let source = CssSource::new(Arc::from(css_content), content_offset).ok()?;
    let ir = parse_style_ir(source.clone(), dialect, CssParseMode::Recover).ok()?;
    let mut projection = Projection {
        source: &source,
        analysis: CssAnalysis::default(),
        special_pseudos: Vec::new(),
    };
    projection.statements(ir.statements(), None, false);
    Some((projection.analysis, projection.special_pseudos))
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
                        if !selector.facts().is_complete_static() {
                            continue;
                        }
                        let Some(structure) = convert_complex(self.source, selector) else {
                            continue;
                        };
                        let selector_index = u32::try_from(self.analysis.selectors.len()).ok();
                        first_selector = first_selector.or(selector_index);
                        self.collect_selector_facts(selector, selector_index);
                        let span = trim_span(self.source, selector.span());
                        self.analysis.selectors.push(AnalyzedSelector {
                            text: self.source.slice(span).to_owned(),
                            specificity: compute_structured_specificity(&structure),
                            span,
                            structure: Some(structure),
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
            SelectorComponentKind::Class => {
                if let Some(span) = component.name_span() {
                    self.analysis.classes.push(AnalyzedCssClass {
                        name: self.source.slice(span).to_owned(),
                        span,
                        selector_index,
                    });
                }
            }
            SelectorComponentKind::Id => {
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
        selector_index: Option<u32>,
    ) {
        let Some(name_span) = component.name_span() else {
            return;
        };
        let kind = match self.source.slice(name_span).trim_start_matches(':') {
            name if name.eq_ignore_ascii_case("deep") => SpecialPseudoKind::Deep,
            name if name.eq_ignore_ascii_case("global") => SpecialPseudoKind::Global,
            name if name.eq_ignore_ascii_case("slotted") => SpecialPseudoKind::Slotted,
            _ => return,
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
        if let Some(span) = inner_span.filter(|span| span.start < span.end) {
            if let Ok(source) = CssSource::new(Arc::from(self.source.slice(span)), span.start) {
                if let Ok(structure) = parse_selector_structure(&source, CssDialect::Css) {
                    for nested in structure.list().selectors() {
                        if nested.facts().is_complete_static() {
                            self.collect_selector_facts(nested, selector_index);
                        }
                    }
                }
            }
        }
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
