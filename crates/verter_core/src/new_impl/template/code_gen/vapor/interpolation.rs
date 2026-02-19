//! Vapor interpolation code generation.
//!
//! `{{ expr }}` in Vapor mode:
//! 1. A space placeholder is appended to the parent's HTML buffer (DOM text node).
//! 2. The expression is wrapped in `_toDisplayString(expr)` and recorded as a
//!    dynamic text part.
//! 3. The parent element is marked as having dynamic text, which triggers
//!    `_txt()` creation and `_renderEffect(() => _setText(...))` in the output.

use crate::new_impl::ast::types::InterpolationNode;
use crate::new_impl::template::code_gen::binding::{is_simple_ident, BindingResolver};
use crate::new_impl::template::code_gen::shared::helpers::{self, VaporHelper};
use crate::new_impl::template::code_gen::types::{
    CodeGenOutput, VaporCounters, VaporElementState, VaporTextPart,
};
use crate::new_impl::template::oxc::types::OxcParsedExpression;

/// Process an interpolation node in Vapor mode.
///
/// - Appends a space placeholder to the parent's HTML buffer.
/// - Wraps the expression in `_toDisplayString()`.
/// - Uses OXC binding data for compound expressions, falling back to
///   `resolve_simple_expr` for single identifiers.
/// - Marks the parent as having dynamic text.
/// - Allocates a text node ref if not already present.
pub fn process_interpolation<'a>(
    interp: &InterpolationNode,
    source: &str,
    oxc: &OxcParsedExpression<'_>,
    resolver: &BindingResolver<'_>,
    parent: &mut VaporElementState<'a>,
    counters: &mut VaporCounters,
    out: &mut CodeGenOutput<'a>,
) {
    // Append space placeholder to HTML (represents the text DOM node)
    parent.html.push(' ');

    // Extract expression content
    let expr = &source[interp.inner_start as usize..interp.inner_end as usize];
    let expr_trimmed = expr.trim();

    // Build _toDisplayString(expr) into local buffer, then bump-allocate
    let prefixed = build_prefixed_expr(expr_trimmed, interp.inner_start, oxc, resolver);
    let mut buf = String::with_capacity(helpers::TO_DISPLAY_STRING.len() + prefixed.len() + 2);
    buf.push_str(helpers::TO_DISPLAY_STRING);
    buf.push('(');
    buf.push_str(&prefixed);
    buf.push(')');

    // Record dynamic text part (bump-allocated)
    parent
        .text_parts
        .push(VaporTextPart::Dynamic(out.alloc_str(&buf)));

    // Ensure text node ref is allocated
    parent.ensure_text_ref(counters);

    // Record import
    out.add_vapor_import(VaporHelper::ToDisplayString);
}

/// Build a prefixed expression string using OXC binding extraction data.
///
/// For compound expressions (e.g., `a + b.x`), walks the extracted bindings
/// and inserts the correct prefix/suffix at each identifier's position.
/// For simple identifiers, falls back to `resolve_simple_expr`.
fn build_prefixed_expr(
    expr: &str,
    inner_start: u32,
    oxc: &OxcParsedExpression<'_>,
    resolver: &BindingResolver<'_>,
) -> String {
    let Some(bindings) = &oxc.bindings else {
        return resolver.resolve_simple_expr(expr);
    };
    if bindings.bindings.is_empty() {
        return resolver.resolve_simple_expr(expr);
    }

    // For a single binding that spans the entire trimmed expression, use simple resolution
    if bindings.bindings.len() == 1 && is_simple_ident(expr) {
        return resolver.resolve_simple_expr(expr);
    }

    // Walk bindings and insert prefix/suffix at expression-relative offsets.
    // Binding `pos` is file-relative; subtract `inner_start` to get expr-relative offset,
    // then adjust for leading whitespace trim.
    let expr_start = inner_start as usize;
    let trim_offset = expr.len() - expr.trim_start().len();

    let mut result = String::with_capacity(expr.len() + bindings.bindings.len() * 8);
    let trimmed = expr.trim();
    let mut last_end = 0usize;

    for binding in &bindings.bindings {
        if binding.ignore {
            continue;
        }

        // Convert file-relative pos to expr-trimmed-relative offset
        let rel_pos = (binding.pos as usize)
            .saturating_sub(expr_start)
            .saturating_sub(trim_offset);

        if rel_pos > trimmed.len() {
            continue; // out of range
        }

        // Append text before this binding
        if rel_pos > last_end {
            result.push_str(&trimmed[last_end..rel_pos]);
        }

        // Insert prefix
        let prefix = resolver.resolve_prefix(binding.name);
        result.push_str(prefix);

        // Insert the identifier itself
        result.push_str(binding.name);

        // Insert suffix
        let suffix = resolver.resolve_suffix(binding.name);
        result.push_str(suffix);

        last_end = rel_pos + binding.name.len();
    }

    // Append any remaining text after the last binding
    if last_end < trimmed.len() {
        result.push_str(&trimmed[last_end..]);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_impl::template::oxc::types::Dynamism;
    use rustc_hash::FxHashMap;

    fn make_parent() -> VaporElementState<'static> {
        VaporElementState::new()
    }

    fn make_resolver_empty() -> BindingResolver<'static> {
        BindingResolver::new(FxHashMap::default(), false)
    }

    fn make_resolver_with_ctx() -> BindingResolver<'static> {
        BindingResolver::new(FxHashMap::default(), false)
    }

    fn make_empty_oxc() -> OxcParsedExpression<'static> {
        OxcParsedExpression {
            offset: 0,
            expression: None,
            errors: None,
            bindings: None,
            dynamism: Dynamism::Static,
        }
    }

    #[test]
    fn interpolation_appends_space_to_html() {
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut parent = make_parent();
        let mut counters = VaporCounters::default();
        let resolver = make_resolver_empty();

        let interp = InterpolationNode {
            start: 0,
            end: 9,
            inner_start: 3,
            inner_end: 6,
        };
        let oxc = make_empty_oxc();
        process_interpolation(
            &interp,
            "{{ msg }}",
            &oxc,
            &resolver,
            &mut parent,
            &mut counters,
            &mut out,
        );

        assert_eq!(parent.html, " ");
    }

    #[test]
    fn interpolation_records_dynamic_part() {
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut parent = make_parent();
        let mut counters = VaporCounters::default();
        let resolver = make_resolver_with_ctx();

        let interp = InterpolationNode {
            start: 0,
            end: 9,
            inner_start: 3,
            inner_end: 6,
        };
        let oxc = make_empty_oxc();
        process_interpolation(
            &interp,
            "{{ msg }}",
            &oxc,
            &resolver,
            &mut parent,
            &mut counters,
            &mut out,
        );

        assert_eq!(parent.text_parts.len(), 1);
        assert!(parent.text_parts[0].is_dynamic());
        assert!(parent.text_parts[0].to_js().contains("_toDisplayString"));
        assert!(parent.text_parts[0].to_js().contains("_ctx.msg"));
    }

    #[test]
    fn interpolation_allocates_text_ref() {
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut parent = make_parent();
        let mut counters = VaporCounters::default();
        let resolver = make_resolver_empty();

        let interp = InterpolationNode {
            start: 0,
            end: 9,
            inner_start: 3,
            inner_end: 6,
        };
        let oxc = make_empty_oxc();
        process_interpolation(
            &interp,
            "{{ msg }}",
            &oxc,
            &resolver,
            &mut parent,
            &mut counters,
            &mut out,
        );

        assert_eq!(parent.text_node_ref, Some(0));
        assert_eq!(counters.x, 1);
    }

    #[test]
    fn interpolation_adds_import() {
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut parent = make_parent();
        let mut counters = VaporCounters::default();
        let resolver = make_resolver_empty();

        let interp = InterpolationNode {
            start: 0,
            end: 9,
            inner_start: 3,
            inner_end: 6,
        };
        let oxc = make_empty_oxc();
        process_interpolation(
            &interp,
            "{{ msg }}",
            &oxc,
            &resolver,
            &mut parent,
            &mut counters,
            &mut out,
        );

        assert!(out.vapor_imports().has(VaporHelper::ToDisplayString));
    }

    #[test]
    fn compound_expression_not_prefixed() {
        let resolver = make_resolver_empty();
        let result = resolver.resolve_simple_expr("a + b");
        assert_eq!(result, "a + b"); // Compound expressions pass through
    }
}
