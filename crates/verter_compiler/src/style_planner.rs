//! Typed framework style rewrite stages over the shared style syntax IR.

use std::{collections::BTreeMap, sync::Arc};

use oxc_allocator::Allocator;
use verter_css_syntax::{
    css_identifier_eq_ignore_ascii_case, parse_style_ir, CombinatorKind, ComplexSelector,
    ComplexSelectorPart, ComponentValue, ComponentValueTree, CssDialect, CssParseMode, CssSource,
    SelectorComponent, SelectorComponentKind, SelectorList, SelectorPseudo, StyleCompleteness,
    StyleDeclaration, StyleDirective, StyleStatement, StyleSyntaxIr, TokenKind, UnknownStatement,
    UnknownStatementKind,
};
use verter_span::Span;

use crate::code_transform::{CodeTransform, SourceMapOptions};
pub use crate::css::types::generate_var_name;
use crate::css::types::VBindVar;
use crate::framework_common::{RuntimeOutputDescriptor, SourceMapFidelity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleRewriteStage {
    AuthoredVBind,
    PostPreprocessModules,
    PostPreprocessScoping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleRewriteFailureClass {
    StageRequiresPlainCss,
    ParseFailure,
    UntrustedRewriteTarget,
    OverlappingEdits,
    IndentedLayoutMutation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRewriteFailure {
    pub class: StyleRewriteFailureClass,
    pub stage: StyleRewriteStage,
    pub dialect: CssDialect,
    pub span: Option<Span>,
}

impl StyleRewriteFailure {
    fn new(
        class: StyleRewriteFailureClass,
        stage: StyleRewriteStage,
        dialect: CssDialect,
        span: Option<Span>,
    ) -> Self {
        Self {
            class,
            stage,
            dialect,
            span,
        }
    }
}

impl std::fmt::Display for StyleRewriteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "style rewrite {:?} refused {:?} input with {:?} at {:?}",
            self.stage, self.dialect, self.class, self.span
        )
    }
}

impl std::error::Error for StyleRewriteFailure {}

#[derive(Debug, Clone, Copy)]
struct StyleSourceIdentity<'a> {
    source_name: &'a str,
    source_space_token: &'a str,
    content_artifact_token: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct AuthoredStyleInput<'a> {
    code: &'a str,
    dialect: CssDialect,
    source: StyleSourceIdentity<'a>,
}

impl<'a> AuthoredStyleInput<'a> {
    #[must_use]
    pub const fn new(
        code: &'a str,
        dialect: CssDialect,
        source_name: &'a str,
        source_space_token: &'a str,
        content_artifact_token: &'a str,
    ) -> Self {
        Self {
            code,
            dialect,
            source: StyleSourceIdentity {
                source_name,
                source_space_token,
                content_artifact_token,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlainCssInput<'a> {
    code: &'a str,
    source: StyleSourceIdentity<'a>,
}

impl<'a> PlainCssInput<'a> {
    pub fn try_new(
        code: &'a str,
        dialect: CssDialect,
        source_name: &'a str,
        source_space_token: &'a str,
        content_artifact_token: &'a str,
    ) -> Result<Self, StyleRewriteFailure> {
        if dialect != CssDialect::Css {
            return Err(StyleRewriteFailure::new(
                StyleRewriteFailureClass::StageRequiresPlainCss,
                StyleRewriteStage::PostPreprocessScoping,
                dialect,
                None,
            ));
        }
        Ok(Self {
            code,
            source: StyleSourceIdentity {
                source_name,
                source_space_token,
                content_artifact_token,
            },
        })
    }

    #[must_use]
    pub const fn code(&self) -> &'a str {
        self.code
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VueStyleRewriteMask {
    pub v_bind: bool,
    pub css_modules: bool,
    pub scoped_selector: bool,
    pub keyframes: bool,
    pub deep: bool,
    pub slotted: bool,
    pub global: bool,
}

#[derive(Debug, Clone, Default)]
pub struct VueStyleFacts {
    pub v_bind_vars: Vec<VBindVar>,
    pub module_classes: Vec<(String, String)>,
    pub refusals: Vec<StyleRewriteFailure>,
    pub rewrites: VueStyleRewriteMask,
}

#[derive(Debug, Clone)]
pub enum StyleRewriteOutcome {
    Unchanged {
        facts: VueStyleFacts,
    },
    Rewritten {
        code: String,
        source_map: String,
        facts: VueStyleFacts,
        output_descriptor: Box<RuntimeOutputDescriptor>,
    },
}

#[derive(Debug, Clone)]
enum StyleEdit {
    Overwrite { span: Span, content: String },
    Insert { at: u32, content: String },
}

impl StyleEdit {
    const fn start(&self) -> u32 {
        match self {
            Self::Overwrite { span, .. } => span.start,
            Self::Insert { at, .. } => *at,
        }
    }

    const fn end(&self) -> u32 {
        match self {
            Self::Overwrite { span, .. } => span.end,
            Self::Insert { at, .. } => *at,
        }
    }
}

fn parse_ir(
    code: &str,
    dialect: CssDialect,
    stage: StyleRewriteStage,
) -> Result<StyleSyntaxIr, StyleRewriteFailure> {
    let source = CssSource::new(Arc::from(code), 0).map_err(|_| {
        StyleRewriteFailure::new(StyleRewriteFailureClass::ParseFailure, stage, dialect, None)
    })?;
    parse_style_ir(source, dialect, CssParseMode::Recover).map_err(|_| {
        StyleRewriteFailure::new(StyleRewriteFailureClass::ParseFailure, stage, dialect, None)
    })
}

fn emit(
    code: &str,
    source: StyleSourceIdentity<'_>,
    dialect: CssDialect,
    stage: StyleRewriteStage,
    mut edits: Vec<StyleEdit>,
    facts: VueStyleFacts,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    if edits.is_empty() {
        return Ok(StyleRewriteOutcome::Unchanged { facts });
    }
    edits.sort_by_key(|edit| (edit.start(), edit.end()));
    let mut previous_end = 0;
    for edit in &edits {
        if edit.start() < previous_end {
            return Err(StyleRewriteFailure::new(
                StyleRewriteFailureClass::OverlappingEdits,
                stage,
                dialect,
                Some(Span::new(edit.start(), edit.end())),
            ));
        }
        previous_end = previous_end.max(edit.end());
    }

    let allocator = Allocator::new();
    let mut transform = CodeTransform::new(code, &allocator);
    for edit in edits {
        match edit {
            StyleEdit::Overwrite { span, content } => {
                let content = allocator.alloc_str(&content);
                transform.overwrite(span.start, span.end, content);
            }
            StyleEdit::Insert { at, content } => {
                let content = allocator.alloc_str(&content);
                transform.prepend_left(at, content);
            }
        }
    }
    let source_map = transform
        .generate_map(SourceMapOptions::new().with_source(source.source_name))
        .to_json_string();
    let output = transform.build_string();
    let output_descriptor = RuntimeOutputDescriptor::generated(
        &output,
        Some(&source_map),
        &[(source.source_space_token, source.content_artifact_token)],
        SourceMapFidelity::Exact,
    );
    Ok(StyleRewriteOutcome::Rewritten {
        code: output,
        source_map,
        facts,
        output_descriptor: Box::new(output_descriptor),
    })
}

pub fn transform_vue_v_bind(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let ir = parse_ir(input.code, input.dialect, StyleRewriteStage::AuthoredVBind)?;
    let mut edits = Vec::new();
    let mut vars = Vec::new();
    collect_v_bind_statements(
        ir.statements(),
        ir.source(),
        input.dialect,
        scope_id,
        true,
        &mut edits,
        &mut vars,
    )?;
    let facts = VueStyleFacts {
        rewrites: VueStyleRewriteMask {
            v_bind: !edits.is_empty(),
            ..VueStyleRewriteMask::default()
        },
        v_bind_vars: vars,
        ..VueStyleFacts::default()
    };
    emit(
        input.code,
        input.source,
        input.dialect,
        StyleRewriteStage::AuthoredVBind,
        edits,
        facts,
    )
}

fn collect_v_bind_statements(
    statements: &[StyleStatement],
    source: &CssSource,
    dialect: CssDialect,
    scope_id: &str,
    trusted_ancestor: bool,
    edits: &mut Vec<StyleEdit>,
    vars: &mut Vec<VBindVar>,
) -> Result<(), StyleRewriteFailure> {
    let mut trusted_context = trusted_ancestor;
    for statement in statements {
        match statement {
            StyleStatement::Declaration(declaration) => {
                let trusted = trusted_context
                    && declaration.completeness() == StyleCompleteness::Complete
                    && declaration.value().completeness() == StyleCompleteness::Complete;
                collect_v_bind_values(
                    declaration.value(),
                    source,
                    dialect,
                    scope_id,
                    trusted,
                    edits,
                    vars,
                )?;
                if let Some(body) = declaration.body() {
                    collect_v_bind_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        trusted && body.completeness() == StyleCompleteness::Complete,
                        edits,
                        vars,
                    )?;
                }
            }
            StyleStatement::Rule(rule) => {
                let trusted = trusted_context
                    && rule.completeness() == StyleCompleteness::Complete
                    && rule.body().completeness() == StyleCompleteness::Complete;
                collect_v_bind_statements(
                    rule.body().statements(),
                    source,
                    dialect,
                    scope_id,
                    trusted,
                    edits,
                    vars,
                )?;
            }
            StyleStatement::AtRule(rule) => {
                if let Some(body) = rule.body() {
                    collect_v_bind_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        trusted_context
                            && rule.completeness() == StyleCompleteness::Complete
                            && body.completeness() == StyleCompleteness::Complete,
                        edits,
                        vars,
                    )?;
                }
            }
            StyleStatement::MixinOrFunction(rule) => {
                if let Some(body) = rule.body() {
                    collect_v_bind_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        trusted_context
                            && rule.completeness() == StyleCompleteness::Complete
                            && body.completeness() == StyleCompleteness::Complete,
                        edits,
                        vars,
                    )?;
                }
            }
            StyleStatement::Unknown(unknown) => {
                if let Some(values) = unknown.opaque_values() {
                    let trusted = trusted_context
                        && dialect == CssDialect::Stylus
                        && unknown.kind() == UnknownStatementKind::Ambiguous
                        && values.completeness() == StyleCompleteness::Complete;
                    collect_v_bind_values(values, source, dialect, scope_id, trusted, edits, vars)?;
                }
                if let Some(body) = unknown.body() {
                    collect_v_bind_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        false,
                        edits,
                        vars,
                    )?;
                }
                if unknown.kind() == UnknownStatementKind::Recovery {
                    trusted_context = false;
                }
            }
        }
    }
    Ok(())
}

fn collect_v_bind_values(
    tree: &ComponentValueTree,
    source: &CssSource,
    dialect: CssDialect,
    scope_id: &str,
    trusted: bool,
    edits: &mut Vec<StyleEdit>,
    vars: &mut Vec<VBindVar>,
) -> Result<(), StyleRewriteFailure> {
    for value in tree.values() {
        match value {
            ComponentValue::Function(function) => {
                let name = source.slice(function.name_span());
                if css_identifier_eq_ignore_ascii_case(name, "v-bind") {
                    if !trusted || !function.is_complete() {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    if matches!(dialect, CssDialect::Sass | CssDialect::Stylus)
                        && source.slice(function.full_span()).contains(['\r', '\n'])
                    {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::IndentedLayoutMutation,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    if values_contain_interpolation(function.values(), source, dialect) {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    let (expression, expression_span) = v_bind_expression(source, function)?;
                    let var_name = generate_var_name(scope_id, expression);
                    edits.push(StyleEdit::Overwrite {
                        span: function.full_span(),
                        content: format!("var({var_name})"),
                    });
                    vars.push(VBindVar {
                        expression: expression.to_string(),
                        var_name,
                        expr_start: expression_span.start,
                        expr_end: expression_span.end,
                    });
                } else {
                    let nested = ComponentValueTreeRef {
                        values: function.values(),
                    };
                    collect_v_bind_value_slice(
                        nested.values,
                        source,
                        dialect,
                        scope_id,
                        trusted && function.is_complete(),
                        edits,
                        vars,
                    )?;
                }
            }
            ComponentValue::Block(block) => collect_v_bind_value_slice(
                block.values(),
                source,
                dialect,
                scope_id,
                trusted && block.is_complete(),
                edits,
                vars,
            )?,
            ComponentValue::Interpolation(_) => {}
            ComponentValue::Token(_) | ComponentValue::String(_) | ComponentValue::Comment(_) => {}
        }
    }
    Ok(())
}

struct ComponentValueTreeRef<'a> {
    values: &'a [ComponentValue],
}

fn collect_v_bind_value_slice(
    values: &[ComponentValue],
    source: &CssSource,
    dialect: CssDialect,
    scope_id: &str,
    trusted: bool,
    edits: &mut Vec<StyleEdit>,
    vars: &mut Vec<VBindVar>,
) -> Result<(), StyleRewriteFailure> {
    let tree = ComponentValueTreeRef { values };
    for value in tree.values {
        match value {
            ComponentValue::Function(function) => {
                let name = source.slice(function.name_span());
                if css_identifier_eq_ignore_ascii_case(name, "v-bind") {
                    if !trusted || !function.is_complete() {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    if matches!(dialect, CssDialect::Sass | CssDialect::Stylus)
                        && source.slice(function.full_span()).contains(['\r', '\n'])
                    {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::IndentedLayoutMutation,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    if values_contain_interpolation(function.values(), source, dialect) {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    let (expression, expression_span) = v_bind_expression(source, function)?;
                    let var_name = generate_var_name(scope_id, expression);
                    edits.push(StyleEdit::Overwrite {
                        span: function.full_span(),
                        content: format!("var({var_name})"),
                    });
                    vars.push(VBindVar {
                        expression: expression.to_string(),
                        var_name,
                        expr_start: expression_span.start,
                        expr_end: expression_span.end,
                    });
                } else {
                    collect_v_bind_value_slice(
                        function.values(),
                        source,
                        dialect,
                        scope_id,
                        trusted && function.is_complete(),
                        edits,
                        vars,
                    )?;
                }
            }
            ComponentValue::Block(block) => collect_v_bind_value_slice(
                block.values(),
                source,
                dialect,
                scope_id,
                trusted && block.is_complete(),
                edits,
                vars,
            )?,
            ComponentValue::Interpolation(_) => {}
            ComponentValue::Token(_) | ComponentValue::String(_) | ComponentValue::Comment(_) => {}
        }
    }
    Ok(())
}

fn values_contain_interpolation(
    values: &[ComponentValue],
    source: &CssSource,
    dialect: CssDialect,
) -> bool {
    values.iter().any(|value| match value {
        ComponentValue::Interpolation(_) => true,
        ComponentValue::Function(function) => {
            values_contain_interpolation(function.values(), source, dialect)
        }
        ComponentValue::Block(block) => {
            values_contain_interpolation(block.values(), source, dialect)
        }
        ComponentValue::String(token) => {
            let text = source.slice(token.span());
            match dialect {
                CssDialect::Scss | CssDialect::Sass => text.contains("#{"),
                CssDialect::Less => text.contains("@{"),
                CssDialect::Css | CssDialect::Stylus => false,
            }
        }
        ComponentValue::Token(_) | ComponentValue::Comment(_) => false,
    })
}

fn v_bind_expression<'a>(
    source: &'a CssSource,
    function: &verter_css_syntax::ComponentFunction,
) -> Result<(&'a str, Span), StyleRewriteFailure> {
    let full = function.full_span();
    let raw_span = Span::new(function.name_span().end + 1, full.end.saturating_sub(1));
    let raw = source.slice(raw_span);
    let trimmed = raw.trim();
    let leading = u32::try_from(trimmed.as_ptr() as usize - raw.as_ptr() as usize).unwrap_or(0);
    let mut span = Span::new(
        raw_span.start + leading,
        raw_span.start + leading + u32::try_from(trimmed.len()).unwrap_or(u32::MAX),
    );
    let expression = if trimmed.len() >= 2
        && ((trimmed.starts_with('\'') && trimmed.ends_with('\''))
            || (trimmed.starts_with('"') && trimmed.ends_with('"')))
    {
        span = Span::new(span.start + 1, span.end - 1);
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    Ok((expression, span))
}

pub fn transform_vue_css_modules(
    input: PlainCssInput<'_>,
    scope_id: &str,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let ir = parse_ir(
        input.code,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessModules,
    )?;
    let mut edits = Vec::new();
    let mut classes = BTreeMap::new();
    collect_module_statements(
        ir.statements(),
        ir.source(),
        scope_id,
        false,
        &mut edits,
        &mut classes,
    )?;
    let facts = VueStyleFacts {
        module_classes: classes.into_iter().collect(),
        rewrites: VueStyleRewriteMask {
            css_modules: !edits.is_empty(),
            ..VueStyleRewriteMask::default()
        },
        ..VueStyleFacts::default()
    };
    emit(
        input.code,
        input.source,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessModules,
        edits,
        facts,
    )
}

fn collect_module_statements(
    statements: &[StyleStatement],
    source: &CssSource,
    scope_id: &str,
    inside_keyframes: bool,
    edits: &mut Vec<StyleEdit>,
    classes: &mut BTreeMap<String, String>,
) -> Result<(), StyleRewriteFailure> {
    for statement in statements {
        match statement {
            StyleStatement::Rule(rule) => {
                if !inside_keyframes {
                    if rule.completeness() != StyleCompleteness::Complete
                        || !rule.selector_list().facts().is_complete_static()
                    {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::PostPreprocessModules,
                            CssDialect::Css,
                            Some(rule.span()),
                        ));
                    }
                    collect_module_selector_list(
                        rule.selector_list(),
                        source,
                        scope_id,
                        edits,
                        classes,
                    )?;
                }
                collect_module_statements(
                    rule.body().statements(),
                    source,
                    scope_id,
                    inside_keyframes,
                    edits,
                    classes,
                )?;
            }
            StyleStatement::AtRule(rule) => {
                if let Some(body) = rule.body() {
                    let head = source.slice(rule.head_span());
                    let keyframes = css_identifier_eq_ignore_ascii_case(head, "@keyframes")
                        || css_identifier_eq_ignore_ascii_case(head, "@-webkit-keyframes");
                    collect_module_statements(
                        body.statements(),
                        source,
                        scope_id,
                        inside_keyframes || keyframes,
                        edits,
                        classes,
                    )?;
                }
            }
            StyleStatement::Declaration(declaration) => {
                if let Some(body) = declaration.body() {
                    collect_module_statements(
                        body.statements(),
                        source,
                        scope_id,
                        inside_keyframes,
                        edits,
                        classes,
                    )?;
                }
            }
            StyleStatement::MixinOrFunction(rule) => {
                if let Some(body) = rule.body() {
                    collect_module_statements(
                        body.statements(),
                        source,
                        scope_id,
                        inside_keyframes,
                        edits,
                        classes,
                    )?;
                }
            }
            StyleStatement::Unknown(unknown) => {
                if !inside_keyframes {
                    return Err(StyleRewriteFailure::new(
                        StyleRewriteFailureClass::UntrustedRewriteTarget,
                        StyleRewriteStage::PostPreprocessModules,
                        CssDialect::Css,
                        Some(unknown.span()),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn collect_module_selector_list(
    list: &SelectorList,
    source: &CssSource,
    scope_id: &str,
    edits: &mut Vec<StyleEdit>,
    classes: &mut BTreeMap<String, String>,
) -> Result<(), StyleRewriteFailure> {
    if !list.facts().is_complete_static() {
        return Err(StyleRewriteFailure::new(
            StyleRewriteFailureClass::UntrustedRewriteTarget,
            StyleRewriteStage::PostPreprocessModules,
            CssDialect::Css,
            Some(list.span()),
        ));
    }
    for selector in list.selectors() {
        for compound in selector.compounds() {
            for component in compound.components() {
                if component.kind() == SelectorComponentKind::Class {
                    let name_span = component.name_span().ok_or_else(|| {
                        StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::PostPreprocessModules,
                            CssDialect::Css,
                            Some(component.span()),
                        )
                    })?;
                    let name = source.slice(name_span);
                    let hashed = classes
                        .entry(name.to_string())
                        .or_insert_with(|| crate::css::modules::hashed_class_name(scope_id, name))
                        .clone();
                    edits.push(StyleEdit::Overwrite {
                        span: name_span,
                        content: hashed,
                    });
                }
                if let Some(nested) = component
                    .pseudo()
                    .and_then(verter_css_syntax::SelectorPseudo::selector_list)
                {
                    collect_module_selector_list(nested, source, scope_id, edits, classes)?;
                }
            }
        }
    }
    Ok(())
}

pub fn transform_vue_scoped_css(
    input: PlainCssInput<'_>,
    scope_id: &str,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let ir = parse_ir(
        input.code,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessScoping,
    )?;
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
    emit(
        input.code,
        input.source,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessScoping,
        planner.edits,
        planner.facts,
    )
}

struct VueScopePlanner<'a> {
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
        let mut special_edits = Vec::new();
        let has_special = self.collect_special_selector_edits(selector, &mut special_edits)?;
        if has_special {
            self.edits.extend(special_edits);
            self.facts.rewrites.scoped_selector = true;
            return Ok(());
        }

        let scope_attr = self.scope_attr.clone();
        let mut scope_edits = Vec::new();
        self.collect_selector_scope_edits(selector, &scope_attr, &mut scope_edits)?;
        self.edits.extend(scope_edits);
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
                        if special_name
                            .as_deref()
                            .is_some_and(|name| matches!(name, "global" | "v-global"))
                        {
                            let argument = self.render_special_argument(pseudo, false, false)?;
                            edits.truncate(edits_checkpoint);
                            edits.push(StyleEdit::Overwrite {
                                span: selector_content_span(selector),
                                content: argument,
                            });
                            self.facts.rewrites.global = true;
                            return Ok(true);
                        }
                        if special_name
                            .as_deref()
                            .is_some_and(|name| matches!(name, "deep" | "v-deep"))
                        {
                            let argument = self.render_special_argument(pseudo, false, true)?;
                            let same_compound_anchor = component_index > 0;
                            let anchor = same_compound_anchor
                                .then_some(compound)
                                .or(previous_compound);
                            let content = if let Some(anchor) = anchor {
                                edits.push(StyleEdit::Insert {
                                    at: self.scope_insertion(anchor),
                                    content: self.scope_attr.clone(),
                                });
                                if same_compound_anchor {
                                    format!(" {argument}")
                                } else {
                                    argument
                                }
                            } else {
                                format!("{} {argument}", self.scope_attr)
                            };
                            edits.push(StyleEdit::Overwrite {
                                span: component.span(),
                                content,
                            });
                            self.facts.rewrites.deep = true;
                            return Ok(true);
                        }
                        if special_name
                            .as_deref()
                            .is_some_and(|name| matches!(name, "slotted" | "v-slotted"))
                        {
                            let argument = self.render_special_argument(pseudo, true, false)?;
                            edits.push(StyleEdit::Overwrite {
                                span: component.span(),
                                content: argument,
                            });
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

    fn render_special_argument(
        &mut self,
        pseudo: &SelectorPseudo,
        slotted: bool,
        allow_empty: bool,
    ) -> Result<String, StyleRewriteFailure> {
        let Some(selector) = self.trusted_first_selector_argument(pseudo, allow_empty)? else {
            return Ok(String::new());
        };
        let span = selector.span();
        let mut edits = Vec::new();
        if slotted {
            let slotted_attr = self.slotted_attr.clone();
            self.collect_selector_scope_edits(selector, &slotted_attr, &mut edits)?;
        }
        if edits.is_empty() {
            return Ok(self.source.slice(span).to_string());
        }
        edits.sort_by_key(|edit| (edit.start(), edit.end()));
        let allocator = Allocator::new();
        let mut transform = CodeTransform::new(self.source.slice(span), &allocator);
        let mut previous_end = span.start;
        for edit in edits {
            if edit.start() < previous_end || edit.end() > span.end {
                return Err(self.untrusted(span));
            }
            previous_end = previous_end.max(edit.end());
            match edit {
                StyleEdit::Overwrite {
                    span: edit_span,
                    content,
                } => {
                    let content = allocator.alloc_str(&content);
                    transform.overwrite(
                        edit_span.start - span.start,
                        edit_span.end - span.start,
                        content,
                    );
                }
                StyleEdit::Insert { at, content } => {
                    let content = allocator.alloc_str(&content);
                    transform.prepend_left(at - span.start, content);
                }
            }
        }
        Ok(transform.build_string())
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
            if self
                .pseudo_name(component)
                .is_some_and(|name| matches!(name.as_str(), "is" | "where"))
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

    fn pseudo_name(&self, component: &SelectorComponent) -> Option<String> {
        let span = component.name_span()?;
        Some(
            self.source
                .slice(span)
                .trim_end_matches('(')
                .to_ascii_lowercase(),
        )
    }

    fn vue_special_pseudo_name(&self, component: &SelectorComponent) -> Option<String> {
        component.pseudo()?.selector_list()?;
        let name = self.pseudo_name(component)?;
        matches!(
            name.as_str(),
            "deep" | "v-deep" | "slotted" | "v-slotted" | "global" | "v-global"
        )
        .then_some(name)
    }

    fn trusted_first_selector_argument<'a>(
        &self,
        pseudo: &'a SelectorPseudo,
        allow_empty: bool,
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
            None if allow_empty && self.source.slice(argument_span).trim().is_empty() => Ok(None),
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

fn collect_animation_edits(
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

fn unknown_may_contain_selector(unknown: &UnknownStatement) -> bool {
    unknown.span().start < unknown.span().end
}

fn selector_content_span(selector: &ComplexSelector) -> Span {
    let end = selector
        .parts()
        .last()
        .map_or(selector.span().end, |part| match part {
            ComplexSelectorPart::Compound(compound) => compound.span().end,
            ComplexSelectorPart::Combinator(combinator) => combinator.span().end,
        });
    Span::new(selector.span().start, end)
}

fn selector_list_is_trusted_for_scoping(selector_list: &SelectorList) -> bool {
    matches!(
        selector_list.facts().completeness(),
        verter_css_syntax::SelectorCompleteness::Complete
    ) && selector_list
        .selectors()
        .iter()
        .all(selector_is_trusted_for_scoping)
}

fn selector_is_trusted_for_scoping(selector: &ComplexSelector) -> bool {
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
