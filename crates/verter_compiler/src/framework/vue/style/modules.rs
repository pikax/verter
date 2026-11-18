//! CSS Modules: class collection, hashed names and the module rewrite.

use crate::framework::vue::style::cascade::StyleEdit;
#[cfg(test)]
use crate::framework::vue::style::cascade::{
    StyleRewriteOutcome, VueStyleFacts, VueStyleRewriteMask,
};
use crate::style_planner::{
    authored_or_parsed_ir, parse_ir, AuthoredStyleInput, StyleRewriteFailure,
    StyleRewriteFailureClass, StyleRewriteStage,
};
#[cfg(test)]
use crate::style_planner::{emit, record_input_dependencies, PlainCssInput};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use verter_css_syntax::{
    css_identifier_eq_ignore_ascii_case, CssDialect, CssSource, SelectorComponentKind,
    SelectorList, StyleCompleteness, StyleStatement, StyleSyntaxIr,
};

/// Generate a content-based hash for a CSS module class name.
pub(crate) fn hashed_class_name(component_id: &str, class_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(component_id.as_bytes());
    hasher.update(class_name.as_bytes());
    let result = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(class_name.len() + 1 + 8);
    out.push_str(class_name);
    out.push('_');
    for &byte in &result[..4] {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Collects the CSS-Modules class edits/hashed-name map from an already-parsed
/// IR, without itself calling `parse_ir`. Dialect-agnostic: the selector walk
/// (`collect_module_statements`) never depends on `CssDialect::Css` — that
/// requirement lives entirely in the RUNTIME rewrite entry points
/// (`PlainCssInput`'s construction gate), not in the walk itself. This is what
/// lets `analyze_css_module_classes` (A10a/A10b, class *analysis* only) reuse
/// the exact same walk for all five native dialects without touching runtime
/// class-name rewriting's plain-CSS-only ownership (row 19, untouched).
pub(crate) fn module_classes_and_edits_from_ir(
    ir: &StyleSyntaxIr,
    dialect: CssDialect,
    scope_id: &str,
) -> Result<(Vec<StyleEdit>, BTreeMap<String, String>), StyleRewriteFailure> {
    let mut edits = Vec::new();
    let mut classes = BTreeMap::new();
    collect_module_statements(
        ir.statements(),
        ir.source(),
        dialect,
        scope_id,
        false,
        &mut edits,
        &mut classes,
    )?;
    Ok((edits, classes))
}

pub(crate) fn dedup_static_class_names(ir: &StyleSyntaxIr) -> Vec<String> {
    let source = ir.source();
    let mut seen = HashSet::new();
    ir.complete_static_classes()
        .filter_map(|class| {
            let name = source.slice(class.name_span());
            seen.insert(name).then(|| name.to_string())
        })
        .collect()
}

/// Enumerate complete static class selectors in `code` for IDE `$style` completions.
///
/// Parse failure degrades to an empty list (never panics): completions are advisory.
pub fn complete_static_class_names(code: &str, dialect: CssDialect) -> Vec<String> {
    let Ok(ir) = parse_ir(code, dialect, StyleRewriteStage::PostPreprocessModules) else {
        return Vec::new();
    };
    dedup_static_class_names(&ir)
}

/// Native CSS-Modules class *analysis*: enumerates every class selector an
/// authored style block declares, plus its would-be hashed name, for any of
/// the five native dialects (A10a/A10b) — analysis only, never a rewrite.
/// Runtime class-name rewriting stays the cascade's CSS-Modules stage
/// (plain-CSS-only, post-preprocess); row 19's ownership question is
/// untouched. Class selectors are syntactically identical across all five
/// dialects (no dialect-specific interpolation form is a bare `.class`), so
/// the walk that already backs that cascade stage needs no dialect gate to
/// run here.
pub fn analyze_css_module_classes(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
) -> Result<Vec<(String, String)>, StyleRewriteFailure> {
    let ir = authored_or_parsed_ir(input, StyleRewriteStage::PostPreprocessModules)?;
    let (_edits, classes) = module_classes_and_edits_from_ir(&ir, input.dialect, scope_id)?;
    Ok(classes.into_iter().collect())
}

/// Read-only style facts: complete static class names plus the hashed
/// CSS-Modules names those classes would receive. Never rewrites bytes and
/// never inherits rewrite-oriented refusal for an untrusted sibling selector.
#[derive(Debug, Clone, Default)]
pub struct StyleAnalysis {
    pub static_classes: Vec<String>,
    pub module_classes: Vec<(String, String)>,
}

pub fn analyze_style(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
) -> Result<StyleAnalysis, StyleRewriteFailure> {
    let ir = authored_or_parsed_ir(input, StyleRewriteStage::PostPreprocessModules)?;
    let static_classes = dedup_static_class_names(&ir);
    let module_classes = static_classes
        .iter()
        .map(|name| (name.clone(), hashed_class_name(scope_id, name)))
        .collect();
    Ok(StyleAnalysis {
        static_classes,
        module_classes,
    })
}

/// Crate-test instrument: parse a `PlainCssInput` and materialize only the
/// CSS-Modules stage's edits.
///
/// Not a production route. Shipped `<style module>` requests plan this stage
/// through `run_vue_style_cascade` over the one `StyleSyntaxIr` the run already
/// parsed. This entry exists so crate tests can compare a merged plan against
/// applying stages one after another. It is available only under `#[cfg(test)]`,
/// never through the `test-support` Cargo feature. Compiling it into any
/// non-test library build, or giving it a production caller, reintroduces the
/// second CSS parse and the staged coordinate spaces the shared plan removed.
#[cfg(test)]
pub(crate) fn transform_vue_css_modules(
    input: PlainCssInput<'_>,
    scope_id: &str,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let ir = parse_ir(
        input.code,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessModules,
    )?;
    let (edits, classes) = module_classes_and_edits_from_ir(&ir, CssDialect::Css, scope_id)?;
    let mut facts = VueStyleFacts {
        module_classes: classes.into_iter().collect(),
        rewrites: VueStyleRewriteMask {
            css_modules: !edits.is_empty(),
            ..VueStyleRewriteMask::default()
        },
        ..VueStyleFacts::default()
    };
    record_input_dependencies(&mut facts, &ir);
    emit(
        input.code,
        input.source,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessModules,
        edits,
        facts,
        input.want_source_map,
    )
}

pub(crate) fn collect_module_statements(
    statements: &[StyleStatement],
    source: &CssSource,
    dialect: CssDialect,
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
                            dialect,
                            Some(rule.span()),
                        ));
                    }
                    collect_module_selector_list(
                        rule.selector_list(),
                        source,
                        dialect,
                        scope_id,
                        edits,
                        classes,
                    )?;
                }
                collect_module_statements(
                    rule.body().statements(),
                    source,
                    dialect,
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
                        dialect,
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
                        dialect,
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
                        dialect,
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
                        dialect,
                        Some(unknown.span()),
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn collect_module_selector_list(
    list: &SelectorList,
    source: &CssSource,
    dialect: CssDialect,
    scope_id: &str,
    edits: &mut Vec<StyleEdit>,
    classes: &mut BTreeMap<String, String>,
) -> Result<(), StyleRewriteFailure> {
    if !list.facts().is_complete_static() {
        return Err(StyleRewriteFailure::new(
            StyleRewriteFailureClass::UntrustedRewriteTarget,
            StyleRewriteStage::PostPreprocessModules,
            dialect,
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
                            dialect,
                            Some(component.span()),
                        )
                    })?;
                    let name = source.slice(name_span);
                    let hashed = classes
                        .entry(name.to_string())
                        .or_insert_with(|| hashed_class_name(scope_id, name))
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
                    collect_module_selector_list(
                        nested, source, dialect, scope_id, edits, classes,
                    )?;
                }
            }
        }
    }
    Ok(())
}
