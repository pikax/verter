//! `_useCssVars()` injection helper.
//!
//! Builds the `_useCssVars()` call and pushes it as a prepend to
//! [`CodeGenOutput`]. Used when `<style>` blocks contain `v-bind()` expressions.

use rustc_hash::FxHashMap;

use crate::css::types::VBindVar;
use crate::template::code_gen::binding::{is_simple_ident, BindingType};
use crate::template::code_gen::types::CodeGenOutput;

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
    if v_binds.is_empty() {
        return;
    }

    let mut needs_unref = false;
    let mut buf = String::with_capacity(64 + v_binds.len() * 48);

    buf.push_str("\n_useCssVars(_ctx => ({\n");

    for (i, v_bind) in v_binds.iter().enumerate() {
        buf.push_str("  \"");
        buf.push_str(&v_bind.var_name);
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
            // Complex expression (template literal, function call, etc.):
            // output as-is. Identifiers within are not rewritten yet.
            buf.push_str(expr);
        }

        buf.push(')');
        if i < v_binds.len() - 1 {
            buf.push(',');
        }
        buf.push('\n');
    }

    buf.push_str("}))\n");

    out.prepend_alloc(insert_pos, &buf);
    imports.push("_useCssVars");

    if needs_unref {
        imports.push("_unref");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    fn make_v_bind(expr: &str, var_name: &str) -> VBindVar {
        VBindVar {
            expression: expr.to_string(),
            var_name: var_name.to_string(),
        }
    }

    #[test]
    fn empty_v_binds_no_output() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let bindings = FxHashMap::default();

        inject_use_css_vars(&[], &bindings, 0, &mut out, &mut imports);

        assert!(out.prepends.is_empty());
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

        assert_eq!(out.prepends.len(), 1);
        let content = out.prepends[0].1;
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

        let content = out.prepends[0].1;
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

        let content = out.prepends[0].1;
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

        let content = out.prepends[0].1;
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

        let content = out.prepends[0].1;
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

        let content = out.prepends[0].1;
        assert!(content.contains("count.value"), "content: {}", content);
        assert!(content.contains("__props.color"), "content: {}", content);
        // Should have commas between entries (except last)
        assert!(content.contains("),\n"), "content: {}", content);
    }

    #[test]
    fn complex_expression_output_as_is() {
        // Template literals and other non-identifier expressions should be
        // output as-is without _ctx. prefix (which would create invalid JS).
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut imports = Vec::new();
        let bindings = FxHashMap::default();

        let v_binds = vec![make_v_bind("`scale(${scale})`", "abc-scale")];
        inject_use_css_vars(&v_binds, &bindings, 0, &mut out, &mut imports);

        let content = out.prepends[0].1;
        // Should NOT have _ctx. prefix on template literal
        assert!(
            !content.contains("_ctx."),
            "Complex expr should not have _ctx prefix: {}",
            content
        );
        assert!(
            content.contains("`scale(${scale})`"),
            "Should contain raw expression: {}",
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

        assert_eq!(out.prepends[0].0, 42);
    }
}
