//! VDOM interpolation code generation.
//!
//! Transforms `{{ expr }}` into `(expr)` by overwriting the delimiters.
//! The parent's `leave_element` adds the `_toDisplayString` prefix.
//!
//! Binding patches (accessor prefixes/suffixes) are collected from the
//! resolver and pushed into `CodeGenOutput.prepends`.

use crate::ast::types::InterpolationNode;
use crate::template::oxc::types::OxcParsedExpression;

use super::super::binding::BindingResolver;
use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};

/// Process an interpolation node for VDOM codegen.
///
/// Overwrites the `{{ ` prefix (start..inner_start) with `(` and the
/// ` }}` suffix (inner_end..end) with `)`. Collects binding patches
/// from the resolver for all extracted identifiers in the expression.
///
/// Returns a [`ChildRecord`] with `ChildKind::Interpolation`.
/// The parent's `leave_element` adds `_toDisplayString` as a content prefix.
pub fn process_interpolation<'alloc>(
    interp: &InterpolationNode,
    oxc: &OxcParsedExpression<'alloc>,
    resolver: &BindingResolver<'alloc>,
    out: &mut CodeGenOutput<'alloc>,
) -> ChildRecord {
    // Overwrite {{ prefix → (
    out.overwrite(interp.start, interp.inner_start, "(");

    // Overwrite }} suffix → )
    out.overwrite(interp.inner_end, interp.end, ")");

    // Collect binding patches (accessor prefixes/suffixes)
    if let Some(bindings) = &oxc.bindings {
        resolver.collect_binding_patches(bindings, out);
    }

    ChildRecord {
        start: interp.start,
        end: interp.end,
        kind: ChildKind::Interpolation,
        condition: None,
        condition_prefix: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::oxc::{BindingExtractionResult, Dynamism};
    use oxc_allocator::Allocator;
    use rustc_hash::FxHashMap;

    use super::super::super::binding::BindingType;

    fn make_interp(start: u32, inner_start: u32, inner_end: u32, end: u32) -> InterpolationNode {
        InterpolationNode {
            start,
            end,
            inner_start,
            inner_end,
        }
    }

    fn make_resolver(
        entries: &[(&'static str, BindingType)],
        is_inline: bool,
    ) -> BindingResolver<'static> {
        let mut map = FxHashMap::default();
        for &(name, bt) in entries {
            map.insert(name as &str, bt);
        }
        BindingResolver::new(map, is_inline)
    }

    // ==================== Delimiter overwriting ====================

    #[test]
    fn overwrites_delimiters_to_parens() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let resolver = make_resolver(&[], false);

        // Source: "{{ msg }}" → inner is "msg" at 3..6
        let interp = make_interp(0, 3, 6, 9);
        let oxc = OxcParsedExpression {
            offset: 3,
            expression: None,
            errors: None,
            bindings: None,
            dynamism: Dynamism::Dynamic,
        };

        let record = process_interpolation(&interp, &oxc, &resolver, &mut out);

        assert_eq!(record.kind, ChildKind::Interpolation);
        assert_eq!(record.start, 0);
        assert_eq!(record.end, 9);

        // Two overwrites: {{ → ( and }} → )
        assert_eq!(out.overwrites.len(), 2);
        // {{ prefix overwrite
        assert_eq!(out.overwrites[0], (0, 3, "("));
        // }} suffix overwrite
        assert_eq!(out.overwrites[1], (6, 9, ")"));
    }

    #[test]
    fn overwrites_tight_delimiters() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let resolver = make_resolver(&[], false);

        // Source: "{{msg}}" (no spaces) → inner is "msg" at 2..5
        let interp = make_interp(0, 2, 5, 7);
        let oxc = OxcParsedExpression {
            offset: 2,
            expression: None,
            errors: None,
            bindings: None,
            dynamism: Dynamism::Dynamic,
        };

        process_interpolation(&interp, &oxc, &resolver, &mut out);

        assert_eq!(out.overwrites[0], (0, 2, "("));
        assert_eq!(out.overwrites[1], (5, 7, ")"));
    }

    // ==================== Binding patches ====================

    #[test]
    fn collects_binding_prefix_for_unresolved() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let resolver = make_resolver(&[], false); // standalone, no bindings

        let interp = make_interp(0, 3, 6, 9);
        let oxc = OxcParsedExpression {
            offset: 3,
            expression: None,
            errors: None,
            bindings: Some(BindingExtractionResult {
                bindings: vec![crate::utils::oxc::Binding {
                    name: "msg",
                    span: crate::common::RelativeSpan::new(3, 6),
                    pos: 3,
                    ignore: false,
                    is_shorthand: false,
                }],
                functions: vec![],
                literals: vec![],
                has_errors: false,
                dynamism: Dynamism::Dynamic,
            }),
            dynamism: Dynamism::Dynamic,
        };

        process_interpolation(&interp, &oxc, &resolver, &mut out);

        // Should have _ctx. prefix at pos 3
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.prepends[0], (3, "_ctx."));
    }

    #[test]
    fn collects_binding_suffix_for_setup_ref_inline() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);

        let interp = make_interp(0, 3, 8, 11);
        let oxc = OxcParsedExpression {
            offset: 3,
            expression: None,
            errors: None,
            bindings: Some(BindingExtractionResult {
                bindings: vec![crate::utils::oxc::Binding {
                    name: "count",
                    span: crate::common::RelativeSpan::new(3, 8),
                    pos: 3,
                    ignore: false,
                    is_shorthand: false,
                }],
                functions: vec![],
                literals: vec![],
                has_errors: false,
                dynamism: Dynamism::Dynamic,
            }),
            dynamism: Dynamism::Dynamic,
        };

        process_interpolation(&interp, &oxc, &resolver, &mut out);

        // SetupRef inline: no prefix (empty), suffix ".value" at pos 3+5=8
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.prepends[0], (8, ".value"));
    }

    #[test]
    fn no_patches_when_bindings_is_none() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let resolver = make_resolver(&[("msg", BindingType::SetupConst)], true);

        let interp = make_interp(0, 3, 6, 9);
        let oxc = OxcParsedExpression {
            offset: 3,
            expression: None,
            errors: None,
            bindings: None, // No bindings extracted
            dynamism: Dynamism::Static,
        };

        process_interpolation(&interp, &oxc, &resolver, &mut out);

        // Only overwrites for delimiters, no prepends
        assert_eq!(out.overwrites.len(), 2);
        assert!(out.prepends.is_empty());
    }

    // ==================== Offset handling ====================

    #[test]
    fn interpolation_at_offset_uses_correct_positions() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let resolver = make_resolver(&[], false);

        // Interpolation starts at offset 10 in the source
        let interp = make_interp(10, 13, 16, 19);
        let oxc = OxcParsedExpression {
            offset: 13,
            expression: None,
            errors: None,
            bindings: None,
            dynamism: Dynamism::Dynamic,
        };

        let record = process_interpolation(&interp, &oxc, &resolver, &mut out);

        assert_eq!(record.start, 10);
        assert_eq!(record.end, 19);
        assert_eq!(out.overwrites[0], (10, 13, "("));
        assert_eq!(out.overwrites[1], (16, 19, ")"));
    }
}
