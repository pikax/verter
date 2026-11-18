//! `v-bind()` in style blocks: CSS variable naming, extraction and edits.

use crate::framework::vue::style::cascade::{StyleEdit, StyleRewriteOutcome, VueStyleFacts};
use crate::style_planner::{
    authored_or_parsed_ir, emit, record_input_dependencies, AuthoredStyleInput, ParsedStyleIr,
    StyleRewriteFailure, StyleRewriteFailureClass, StyleRewriteStage,
};
use sha2::{Digest, Sha256};
use verter_css_syntax::{
    css_identifier_eq_ignore_ascii_case, ComponentValue, ComponentValueTree, CssDialect, CssSource,
    StyleCompleteness, StyleStatement, StyleSyntaxIr, UnknownStatementKind,
};
use verter_span::Span;

/// Generate a Vue CSS variable name from a scope ID and authored expression.
///
/// The name is the identity of the custom property BOTH emitted sides agree
/// on: the `<style>` block's `var(--NAME)` reference and the client
/// `_useCssVars` registration (which registers it bare, because the runtime
/// prepends `--` itself). It must therefore be spellable as a CSS custom
/// property AND distinct per authored expression.
///
/// The encoding has two disjoint halves. An expression built solely from
/// characters a custom property can carry verbatim (`[A-Za-z0-9-]`, in
/// particular no `_`) keeps the plain `scope-expression` name. Every other
/// expression — anything that folds, and anything containing `_` — also
/// carries a digest of its authored text. The halves cannot meet: a digest
/// name always contains `_`, a plain name never does, and two digest names
/// only collide when their (different) authored texts share a sha256 digest.
/// Folding alone is lossy (`obj.tone` and `obj+tone` both spell `obj_tone`),
/// and gating the digest on folding alone still leaves a collision: the
/// authored identifier `obj_tone_05eec45d` would spell exactly the name
/// `obj.tone` digests to, and the second registration would be dropped as a
/// duplicate. `_` therefore joins the digest half too.
#[must_use]
pub fn generate_var_name(scope_id: &str, expression: &str) -> String {
    let mut out = String::with_capacity(2 + scope_id.len() + 1 + expression.len());
    out.push_str("--");
    out.push_str(scope_id);
    out.push('-');
    let mut digest_needed = false;
    for character in expression.chars() {
        if character.is_ascii_alphanumeric() || character == '-' {
            out.push(character);
        } else {
            // `_` passes through unchanged but moves the expression into the
            // digest half; every other character folds to `_` as before.
            digest_needed = true;
            out.push('_');
        }
    }
    if digest_needed {
        out.push('_');
        push_expression_digest(&mut out, expression);
    }
    out
}

/// Append a short, stable hex digest of the authored expression.
pub(crate) fn push_expression_digest(out: &mut String, expression: &str) {
    let mut hasher = Sha256::new();
    hasher.update(expression.as_bytes());
    let digest = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &byte in &digest[..4] {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

pub(crate) fn css_var_reference(var_name: &str) -> String {
    let mut out = String::with_capacity(5 + var_name.len());
    out.push_str("var(");
    out.push_str(var_name);
    out.push(')');
    out
}

/// A `v-bind()` expression replaced with a CSS variable.
#[derive(Debug, Clone)]
pub struct VBindVar {
    /// The original expression text (e.g. "color" or "theme.color").
    pub expression: String,
    /// The generated CSS variable name (e.g. "--a4f2eed6-color").
    pub var_name: String,
    /// Byte offset of the quote-stripped expression start within the style content.
    pub expr_start: u32,
    /// Byte offset of the quote-stripped expression end within the style content.
    pub expr_end: u32,
}

pub(crate) fn relocate_edits(
    edits: Vec<StyleEdit>,
    origin: u32,
    stage: StyleRewriteStage,
    dialect: CssDialect,
) -> Result<Vec<StyleEdit>, StyleRewriteFailure> {
    if origin == 0 {
        return Ok(edits);
    }
    edits
        .into_iter()
        .map(|edit| match edit {
            StyleEdit::Overwrite { span, content } => {
                let start = span.start.checked_sub(origin).ok_or_else(|| {
                    StyleRewriteFailure::new(
                        StyleRewriteFailureClass::ParseFailure,
                        stage,
                        dialect,
                        Some(span),
                    )
                })?;
                let end = span.end.checked_sub(origin).ok_or_else(|| {
                    StyleRewriteFailure::new(
                        StyleRewriteFailureClass::ParseFailure,
                        stage,
                        dialect,
                        Some(span),
                    )
                })?;
                Ok(StyleEdit::Overwrite {
                    span: Span::new(start, end),
                    content,
                })
            }
            StyleEdit::Insert { at, content } => {
                let at = at.checked_sub(origin).ok_or_else(|| {
                    StyleRewriteFailure::new(
                        StyleRewriteFailureClass::ParseFailure,
                        stage,
                        dialect,
                        Some(Span::new(at, at)),
                    )
                })?;
                Ok(StyleEdit::Insert { at, content })
            }
        })
        .collect()
}

pub(crate) fn relocate_v_bind_vars(vars: Vec<VBindVar>, origin: u32) -> Vec<VBindVar> {
    if origin == 0 {
        return vars;
    }
    vars.into_iter()
        .map(|mut var| {
            var.expr_start = var.expr_start.saturating_sub(origin);
            var.expr_end = var.expr_end.saturating_sub(origin);
            var
        })
        .collect()
}

/// Plan the authored-`v-bind()` stage's edits and variable inventory from one
/// parsed IR, in real source coordinates.
///
/// Every route that plans v-bind edits calls this — the public authored-only
/// transform, the cascade's authored-only stage, and the shared multi-stage
/// plan — rather than each computing them independently, so a fix to v-bind
/// planning cannot land on one path and silently miss the others.
pub(crate) fn plan_authored_v_bind_edits(
    ir: &ParsedStyleIr,
    dialect: CssDialect,
    scope_id: &str,
) -> Result<(Vec<StyleEdit>, Vec<VBindVar>), StyleRewriteFailure> {
    let origin = ir.source().origin();
    let (edits, vars) = v_bind_edits_from_ir(ir, dialect, scope_id)?;
    let edits = relocate_edits(edits, origin, StyleRewriteStage::AuthoredVBind, dialect)?;
    let vars = relocate_v_bind_vars(vars, origin);
    Ok((edits, vars))
}

pub fn transform_vue_v_bind(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let ir = authored_or_parsed_ir(input, StyleRewriteStage::AuthoredVBind)?;
    let (edits, vars) = plan_authored_v_bind_edits(&ir, input.dialect, scope_id)?;
    let mut facts = VueStyleFacts::authored_v_bind(!edits.is_empty(), vars);
    record_input_dependencies(&mut facts, &ir);
    emit(
        input.code,
        input.source,
        input.dialect,
        StyleRewriteStage::AuthoredVBind,
        edits,
        facts,
        input.want_source_map,
    )
}

/// Fact-only v-bind extraction over an already-parsed IR. Does not itself
/// call `parse_style_ir`. `Err` means at least one `v-bind()` target in this
/// block was too ambiguous to trust — callers must fail OPEN (never treat
/// this as "no v-binds present").
pub fn v_bind_vars_from_parsed_ir(
    ir: &StyleSyntaxIr,
    dialect: CssDialect,
    scope_id: &str,
) -> Result<Vec<VBindVar>, StyleRewriteFailure> {
    v_bind_edits_from_ir(ir, dialect, scope_id).map(|(_edits, vars)| vars)
}

/// Collects the authored v-bind edits/vars from an already-parsed IR, without
/// itself calling `parse_ir` — the shared building block `transform_vue_v_bind`
/// (parses then delegates here) and `run_vue_style_cascade` (reuses a
/// retained IR across stages) both route through.
pub(crate) fn v_bind_edits_from_ir(
    ir: &StyleSyntaxIr,
    dialect: CssDialect,
    scope_id: &str,
) -> Result<(Vec<StyleEdit>, Vec<VBindVar>), StyleRewriteFailure> {
    let mut edits = Vec::new();
    let mut vars = Vec::new();
    collect_v_bind_statements(
        ir.statements(),
        ir.source(),
        dialect,
        scope_id,
        true,
        &mut edits,
        &mut vars,
    )?;
    Ok((edits, vars))
}

pub(crate) fn collect_v_bind_statements(
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

pub(crate) fn collect_v_bind_values(
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
                        content: css_var_reference(&var_name),
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

pub(crate) struct ComponentValueTreeRef<'a> {
    values: &'a [ComponentValue],
}

pub(crate) fn collect_v_bind_value_slice(
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
                        content: css_var_reference(&var_name),
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

pub(crate) fn values_contain_interpolation(
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

pub(crate) fn v_bind_expression<'a>(
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
