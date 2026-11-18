//! `_useCssVars()` injection helper.
//!
//! Builds the `_useCssVars()` call and pushes it as a prepend to
//! [`CodeGenOutput`]. Used when `<style>` blocks contain `v-bind()` expressions.

use oxc_allocator::Allocator;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;
use verter_parser::oxc_parse::Parser;

use crate::style_planner::VBindVar;
use crate::template::binding::{is_simple_ident, BindingType};
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::bindings::{extract_bindings_from_expression, BindingContext};

#[cfg(test)]
thread_local! {
    static EXPRESSION_PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn take_expression_parse_count() -> usize {
    EXPRESSION_PARSE_COUNT.with(|count| count.replace(0))
}

/// Parsed references of the CSS expressions shared by a component's runtime products.
pub(crate) struct PreparedCssVarExpressions {
    expressions: FxHashMap<String, Vec<CssVarBinding>>,
}

struct CssVarBinding {
    name: String,
    start: usize,
    end: usize,
    ignore: bool,
}

impl PreparedCssVarExpressions {
    pub(crate) fn new(v_binds: &[VBindVar]) -> Self {
        let mut expressions = FxHashMap::default();
        for var in v_binds {
            if !is_simple_ident(&var.expression) {
                expressions
                    .entry(var.expression.clone())
                    .or_insert_with(|| parse_complex_css_var_expr(&var.expression));
            }
        }
        Self { expressions }
    }
}

/// Build and inject a `_useCssVars()` call at the given position.
///
/// For each v-bind variable, resolves the expression using the binding map
/// to determine the correct accessor (`.value`, `_unref()`, `__props.`, etc.).
///
/// ```js
/// _useCssVars(_ctx => ({
///   "scope-var": (count.value),
///   "scope-color": (__props.color),
/// }))
/// ```
///
/// Returns `true` if `_unref` is needed (for `SetupMaybeRef` / `SetupLet` bindings).
pub fn inject_use_css_vars<'alloc>(
    v_binds: &[VBindVar],
    bindings: &FxHashMap<&str, BindingType>,
    insert_pos: u32,
    out: &mut CodeGenOutput<'alloc>,
    imports: &mut Vec<&'static str>,
) {
    inject_use_css_vars_in(v_binds, bindings, insert_pos, out, imports, None);
}

pub(crate) fn inject_prepared_use_css_vars<'alloc>(
    v_binds: &[VBindVar],
    bindings: &FxHashMap<&str, BindingType>,
    insert_pos: u32,
    out: &mut CodeGenOutput<'alloc>,
    imports: &mut Vec<&'static str>,
    prepared: &PreparedCssVarExpressions,
) {
    inject_use_css_vars_in(v_binds, bindings, insert_pos, out, imports, Some(prepared));
}

fn inject_use_css_vars_in<'alloc>(
    v_binds: &[VBindVar],
    bindings: &FxHashMap<&str, BindingType>,
    insert_pos: u32,
    out: &mut CodeGenOutput<'alloc>,
    imports: &mut Vec<&'static str>,
    prepared: Option<&PreparedCssVarExpressions>,
) {
    if v_binds.is_empty() {
        return;
    }

    let mut needs_unref = false;
    let mut buf = String::with_capacity(64 + v_binds.len() * 48);

    buf.push_str("\n_useCssVars(_ctx => ({\n");

    // Deduplicate by var_name (same v-bind() expression may appear multiple times in CSS)
    let mut seen = rustc_hash::FxHashSet::default();
    let mut first = true;

    for v_bind in v_binds.iter() {
        if !seen.insert(&v_bind.var_name) {
            continue; // skip duplicate var_name
        }

        if !first {
            buf.push(',');
            buf.push('\n');
        }
        first = false;

        // The client `useCssVars` runtime (`setVarsOnNode`) always prepends
        // `--` itself when it applies the value (`style.setProperty(`--${key}`,
        // ...)`), so the JS-registered key here must be bare — never the
        // `--`-prefixed CSS custom-property name the same `var_name` carries
        // for the CSS-side `var(--x-color)` reference. Baking `--` into both
        // sides means the runtime double-prepends it (`----hash-color`) and
        // the binding silently never applies (A10h).
        let js_key = v_bind
            .var_name
            .strip_prefix("--")
            .unwrap_or(&v_bind.var_name);
        buf.push_str("  \"");
        buf.push_str(js_key);
        buf.push_str("\": (");

        // Resolve expression using binding metadata.
        // Complex expressions (template literals, member access, etc.) are
        // output as-is; only simple identifiers are looked up in the binding
        // map. Full expression rewriting for complex cases (like rewriting
        // identifiers inside template literals) requires JS parsing — this
        // is a TODO for correctness but the output is syntactically valid.
        let expr = &v_bind.expression;
        if is_simple_ident(expr) {
            if let Some(bt) = bindings.get(expr.as_str()) {
                match bt {
                    BindingType::SetupRef => {
                        // Definitively a ref: access .value directly
                        buf.push_str(expr);
                        buf.push_str(".value");
                    }
                    BindingType::SetupMaybeRef | BindingType::SetupLet => {
                        // Might be a ref: wrap with _unref()
                        needs_unref = true;
                        buf.push_str("_unref(");
                        buf.push_str(expr);
                        buf.push(')');
                    }
                    BindingType::Props | BindingType::PropsAliased => {
                        buf.push_str("__props.");
                        buf.push_str(expr);
                    }
                    _ => {
                        // SetupConst, SetupReactiveConst, LiteralConst: direct access
                        buf.push_str(expr);
                    }
                }
            } else {
                // Unknown simple identifier: use _ctx. prefix as fallback
                buf.push_str("_ctx.");
                buf.push_str(expr);
            }
        } else {
            // Complex expression: parse with OXC to find identifiers and
            // apply binding prefix resolution (e.g., count → count.value).
            let resolved = resolve_complex_css_var_expr(expr, bindings, &mut needs_unref, prepared);
            buf.push_str(&resolved);
        }

        buf.push(')');
    }

    buf.push('\n');

    buf.push_str("}))\n");

    out.prepend_alloc(insert_pos, &buf);
    imports.push("_useCssVars");

    if needs_unref {
        imports.push("_unref");
    }
}

/// Resolve identifiers in a complex CSS v-bind expression.
///
/// Parses the expression with OXC, walks identifiers, and applies binding
/// prefix resolution (`.value`, `__props.`, `_unref()`, `_ctx.`).
///
/// Falls back to the raw expression if OXC parsing fails.
fn resolve_complex_css_var_expr(
    expr: &str,
    bindings: &FxHashMap<&str, BindingType>,
    needs_unref: &mut bool,
    prepared: Option<&PreparedCssVarExpressions>,
) -> String {
    let parsed;
    let references = if let Some(prepared) = prepared {
        prepared
            .expressions
            .get(expr)
            .expect("CSS expressions were prepared for these style bindings")
    } else {
        parsed = parse_complex_css_var_expr(expr);
        &parsed
    };
    if references.is_empty() {
        return expr.to_string();
    }

    // Build resolved string by walking bindings in source order.
    let mut result = String::with_capacity(expr.len() + references.len() * 8);
    let mut last_end = 0usize;

    for binding in references {
        if binding.ignore {
            continue;
        }

        // The prepared references already use expression-relative offsets.
        let rel_start = binding.start;
        let rel_end = binding.end;
        if rel_start < last_end || rel_end > expr.len() {
            continue;
        }

        // Append text before this identifier
        result.push_str(&expr[last_end..rel_start]);

        let name = binding.name.as_str();
        if let Some(bt) = bindings.get(name) {
            match bt {
                BindingType::SetupRef => {
                    result.push_str(name);
                    result.push_str(".value");
                }
                BindingType::SetupMaybeRef | BindingType::SetupLet => {
                    *needs_unref = true;
                    result.push_str("_unref(");
                    result.push_str(name);
                    result.push(')');
                }
                BindingType::Props | BindingType::PropsAliased => {
                    result.push_str("__props.");
                    result.push_str(name);
                }
                _ => {
                    result.push_str(name);
                }
            }
        } else {
            // Unknown identifier: _ctx. prefix
            result.push_str("_ctx.");
            result.push_str(name);
        }

        last_end = rel_end;
    }

    // Append remaining text after last identifier
    if last_end < expr.len() {
        result.push_str(&expr[last_end..]);
    }

    result
}

fn parse_complex_css_var_expr(expr: &str) -> Vec<CssVarBinding> {
    #[cfg(test)]
    EXPRESSION_PARSE_COUNT.with(|count| count.set(count.get() + 1));
    // Keep the established CSS parser's expression context and fallback.
    let wrapper = format!("var __v = {}", expr);
    let alloc = Allocator::default();
    let parser_ret = Parser::new(&alloc, &wrapper, SourceType::tsx()).parse();
    if !parser_ret.diagnostics.is_empty() || parser_ret.program.body.is_empty() {
        return Vec::new();
    }
    let init_expr = match &parser_ret.program.body[0] {
        oxc_ast::ast::Statement::VariableDeclaration(decl) => {
            decl.declarations.first().and_then(|d| d.init.as_ref())
        }
        _ => None,
    };
    let Some(init_expr) = init_expr else {
        return Vec::new();
    };
    let extraction = extract_bindings_from_expression(init_expr, &wrapper, BindingContext::new(0));
    let prefix = "var __v = ".len() as u32;
    extraction
        .bindings
        .into_iter()
        .map(|binding| CssVarBinding {
            name: binding.name.to_owned(),
            start: binding.span.start.saturating_sub(prefix) as usize,
            end: binding.span.end.saturating_sub(prefix) as usize,
            ignore: binding.ignore,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    fn make_v_bind(expr: &str, var_name: &str) -> VBindVar {
        VBindVar {
            expression: expr.to_string(),
            var_name: var_name.to_string(),
            expr_start: 0,
            expr_end: 0,
        }
    }

    #[test]
    fn empty_v_binds_no_output() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let bindings = FxHashMap::default();

        inject_use_css_vars(&[], &bindings, 0, &mut out, &mut imports);

        assert!(out.edits.prepends.is_empty());
        assert!(imports.is_empty());
    }

    #[test]
    fn setup_ref_uses_dot_value() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("count", BindingType::SetupRef);

        let v_binds = vec![make_v_bind("count", "abc-count")];
        inject_use_css_vars(&v_binds, &bindings, 10, &mut out, &mut imports);

        assert_eq!(out.edits.prepends.len(), 1);
        let content = out.edits.prepends[0].1;
        assert!(content.contains("count.value"), "content: {}", content);
        assert!(imports.contains(&"_useCssVars"));
        assert!(!imports.contains(&"_unref"));
    }

    #[test]
    fn setup_maybe_ref_uses_unref() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("data", BindingType::SetupMaybeRef);

        let v_binds = vec![make_v_bind("data", "abc-data")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(content.contains("_unref(data)"), "content: {}", content);
        assert!(imports.contains(&"_unref"));
    }

    #[test]
    fn props_uses_dunder_props_prefix() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("color", BindingType::Props);

        let v_binds = vec![make_v_bind("color", "abc-color")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(content.contains("__props.color"), "content: {}", content);
    }

    #[test]
    fn setup_const_direct_access() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("theme", BindingType::SetupConst);

        let v_binds = vec![make_v_bind("theme", "abc-theme")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(
            content.contains("\"abc-theme\": (theme)"),
            "content: {}",
            content
        );
    }

    #[test]
    fn unknown_binding_uses_ctx_prefix() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let bindings = FxHashMap::default();

        let v_binds = vec![make_v_bind("unknown", "abc-unknown")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(content.contains("_ctx.unknown"), "content: {}", content);
    }

    #[test]
    fn multiple_v_binds() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("count", BindingType::SetupRef);
        bindings.insert("color", BindingType::Props);

        let v_binds = vec![
            make_v_bind("count", "abc-count"),
            make_v_bind("color", "abc-color"),
        ];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(content.contains("count.value"), "content: {}", content);
        assert!(content.contains("__props.color"), "content: {}", content);
        // Should have commas between entries (except last)
        assert!(content.contains("),\n"), "content: {}", content);
    }

    /// @ai-generated - duplicate v-bind vars with same var_name should be deduplicated
    #[test]
    fn duplicate_v_bind_vars_deduplicated() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("duration", BindingType::SetupConst);

        let v_binds = vec![
            make_v_bind("duration", "abc-duration"),
            make_v_bind("duration", "abc-duration"),
        ];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        // Should only contain abc-duration once
        let count = content.matches("abc-duration").count();
        assert_eq!(
            count, 1,
            "Duplicate var_name should appear only once. Got: {}",
            content
        );
    }

    /// @ai-generated - duplicate v-bind vars interspersed with unique ones
    #[test]
    fn duplicate_v_bind_vars_preserves_unique() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("duration", BindingType::SetupConst);
        bindings.insert("color", BindingType::Props);

        let v_binds = vec![
            make_v_bind("duration", "abc-duration"),
            make_v_bind("color", "abc-color"),
            make_v_bind("duration", "abc-duration"), // duplicate
        ];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        let dur_count = content.matches("abc-duration").count();
        let color_count = content.matches("abc-color").count();
        assert_eq!(
            dur_count, 1,
            "Duplicate should be removed. Got: {}",
            content
        );
        assert_eq!(color_count, 1, "Unique should be kept. Got: {}", content);
        // The trailing comma logic should still be correct
        assert!(
            content.contains("),\n  \"abc-color\"") || content.contains("),\n}"),
            "Comma formatting should be correct. Got: {}",
            content
        );
    }

    #[test]
    fn complex_expression_template_literal_no_ctx_prefix() {
        // Template literals should not get _ctx. prefix on the literal itself.
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let bindings = FxHashMap::default();

        let v_binds = vec![make_v_bind("`scale(${scale})`", "abc-scale")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        // Template literal should be preserved
        assert!(
            content.contains("`scale("),
            "Should contain template literal: {}",
            content
        );
    }

    #[test]
    fn complex_expr_ref_gets_dot_value() {
        // v-bind(count + 1) where count is a ref → (count.value + 1)
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("count", BindingType::SetupRef);

        let v_binds = vec![make_v_bind("count + 1", "abc-count")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(
            content.contains("count.value + 1"),
            "ref in complex expr should get .value: {}",
            content
        );
        assert!(
            !content.contains("_ctx."),
            "should not have _ctx prefix: {}",
            content
        );
    }

    #[test]
    fn complex_expr_props_gets_dunder_props() {
        // v-bind(color ?? 'red') where color is a prop → (__props.color ?? 'red')
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("color", BindingType::Props);

        let v_binds = vec![make_v_bind("color ?? 'red'", "abc-color")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(
            content.contains("__props.color ?? 'red'"),
            "prop in complex expr should get __props. prefix: {}",
            content
        );
    }

    #[test]
    fn complex_expr_multiple_identifiers_resolved() {
        // v-bind(count + offset) where both are refs
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("count", BindingType::SetupRef);
        bindings.insert("offset", BindingType::SetupRef);

        let v_binds = vec![make_v_bind("count + offset", "abc-expr")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(
            content.contains("count.value + offset.value"),
            "both refs should get .value: {}",
            content
        );
    }

    #[test]
    fn complex_expr_maybe_ref_uses_unref() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("data", BindingType::SetupMaybeRef);

        let v_binds = vec![make_v_bind("data * 2", "abc-data")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(
            content.contains("_unref(data) * 2"),
            "MaybeRef in complex expr should use _unref: {}",
            content
        );
        assert!(imports.contains(&"_unref"));
    }

    #[test]
    fn complex_expr_member_access_preserved() {
        // v-bind(obj.color) — member access should NOT prefix the object
        // when the object is a known SetupConst binding
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let mut bindings = FxHashMap::default();
        bindings.insert("obj", BindingType::SetupConst);

        let v_binds = vec![make_v_bind("obj.color", "abc-color")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.edits.prepends[0].1;
        assert!(
            content.contains("obj.color"),
            "member access should be preserved: {}",
            content
        );
        // Should NOT have _ctx.obj.color since obj is known
        assert!(
            !content.contains("_ctx."),
            "known binding should not have _ctx prefix: {}",
            content
        );
    }

    #[test]
    fn inserts_at_correct_position() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let bindings = FxHashMap::default();

        let v_binds = vec![make_v_bind("x", "abc-x")];
        inject_use_css_vars(&v_binds, &bindings, 42, &mut out, &mut imports);

        assert_eq!(out.edits.prepends[0].0, 42);
    }
    #[test]
    fn prepared_css_expressions_are_reused_with_each_binding_view() {
        let vars = vec![
            make_v_bind("theme.color", "--first"),
            make_v_bind("theme.color", "--second"),
        ];
        take_expression_parse_count();
        let prepared = PreparedCssVarExpressions::new(&vars);
        assert_eq!(take_expression_parse_count(), 1);
        for (binding, expression, helpers) in [
            (
                BindingType::SetupMaybeRef,
                "_unref(theme).color",
                vec!["_useCssVars", "_unref"],
            ),
            (
                BindingType::Props,
                "__props.theme.color",
                vec!["_useCssVars"],
            ),
        ] {
            let alloc = Allocator::default();
            let mut out = CodeGenOutput::new(&alloc);
            let mut imports = Vec::new();
            let mut bindings = FxHashMap::default();
            bindings.insert("theme", binding);
            inject_prepared_use_css_vars(&vars, &bindings, 0, &mut out, &mut imports, &prepared);
            let code = out.edits.prepends[0].1;
            assert_eq!(code, format!("\n_useCssVars(_ctx => ({{\n  \"first\": ({expression}),\n  \"second\": ({expression})\n}}))\n"));
            assert_eq!(imports, helpers);
            let parsed =
                verter_parser::oxc_parse::Parser::new(&alloc, code, SourceType::mjs()).parse();
            assert!(
                parsed.diagnostics.is_empty(),
                "{:?}\n{code}",
                parsed.diagnostics
            );
        }
        assert_eq!(take_expression_parse_count(), 0);
    }
}
