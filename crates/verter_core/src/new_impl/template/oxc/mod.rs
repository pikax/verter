//! AST-to-OXC template expression parsing pass.
//!
//! Converts the arena-based [`TemplateAst`] nodes into OXC-parsed equivalents
//! in a single forward pass over the nodes vec. Each node gets a corresponding
//! [`OxcNodeData`] entry with parsed expressions, extracted bindings, and
//! dynamism classification.

pub mod types;

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::common::Span;
use crate::utils::oxc::{
    extract_bindings_from_expression, vue::adjust_diagnostics_spans, BindingContext,
};

use self::types::*;

/// Parse a single expression from a source span.
///
/// Returns an [`OxcParsedExpression`] with:
/// - Substring-relative AST spans (not adjusted to file positions)
/// - File-relative binding positions (via `BindingContext::base_offset`)
/// - File-relative diagnostic spans (adjusted for error reporting)
/// - [`Dynamism`] classification based on binding analysis
fn parse_expression<'alloc>(
    span: Span,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
) -> OxcParsedExpression<'alloc> {
    if span.start >= span.end {
        return OxcParsedExpression {
            offset: span.start,
            expression: None,
            errors: None,
            bindings: None,
            dynamism: Dynamism::Static,
        };
    }

    let source_slice = &input[span.start as usize..span.end as usize];
    let parser = oxc_parser::Parser::new(alloc, source_slice, source_type);

    match parser.parse_expression() {
        Ok(expr) => {
            // Don't adjust expression AST spans — keep substring-relative.
            // Bindings get file-relative positions via base_offset.
            // Dynamism is computed incrementally during extraction.
            let binding_ctx = BindingContext::with_ignored(span.start, ignored.iter().copied());
            let bindings = extract_bindings_from_expression(&expr, input, &binding_ctx);

            OxcParsedExpression {
                offset: span.start,
                expression: Some(expr),
                errors: None,
                dynamism: bindings.dynamism,
                bindings: Some(bindings),
            }
        }
        Err(mut errors) => {
            adjust_diagnostics_spans(&mut errors, span.start);
            OxcParsedExpression {
                offset: span.start,
                expression: None,
                errors: Some(errors),
                bindings: None,
                dynamism: Dynamism::Static,
            }
        }
    }
}

use crate::new_impl::ast::types::{
    AstNodeKind, ChildrenFlags, ElementNode, ElementNodeConditionKind, TemplateAst,
};
use crate::utils::oxc::vue::{parse_vfor_with_bindings_sliced, parse_vslot_with_bindings_sliced};

/// Parse all expressions on a single element node.
///
/// Processes structural directives (v-if/v-for/v-slot) and regular props
/// in Vue priority order. Accumulates provided locals from v-for/v-slot
/// for children. Computes [`ExpressionFlag`] for codegen optimization.
///
/// Returns an [`OxcParsedElement`] with parsed expressions, provided locals,
/// and expression flags.
fn parse_element<'alloc>(
    element: &ElementNode,
    parent_ignored: &[&'alloc str],
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
) -> OxcParsedElement<'alloc> {
    // Fast path: plain element with no directives → empty result.
    if element.is_plain() {
        return OxcParsedElement {
            condition: None,
            v_for: None,
            v_slot: None,
            props: Vec::new(),
            provided_locals: parent_ignored.to_vec(),
            expression_flag: ExpressionFlag::empty(),
        };
    }

    let mut expression_flag = ExpressionFlag::empty();
    let mut provided_locals: Vec<&'alloc str> = parent_ignored.to_vec();

    // ── 1. v-if / v-else-if condition ───────────────────────────
    let condition = match &element.v_condition {
        Some(cond) if !matches!(cond.kind, ElementNodeConditionKind::Else) => {
            if let (Some(vs), Some(ve)) = (cond.prop.value_start, cond.prop.value_end) {
                let parsed = parse_expression(
                    Span::new(vs, ve),
                    input,
                    alloc,
                    source_type,
                    &provided_locals,
                );
                if parsed.dynamism == Dynamism::Static {
                    expression_flag = expression_flag.add(ExpressionFlags::StaticCondition);
                }
                Some(parsed)
            } else {
                None // v-if/v-else-if with no value (malformed)
            }
        }
        _ => None, // v-else (no expression) or no condition
    };

    // ── 2. v-for ────────────────────────────────────────────────
    let v_for = match &element.v_for {
        Some(prop) => {
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let parsed = parse_vfor_with_bindings_sliced(
                    alloc,
                    Span::new(vs, ve),
                    input,
                    source_type,
                    &provided_locals,
                );
                // Add v-for locals to provided_locals for subsequent parsing
                for local_span in &parsed.locals {
                    provided_locals.push(local_span.slice(input));
                }
                Some(OxcParsedVFor { parsed })
            } else {
                None
            }
        }
        None => None,
    };

    // ── 3. v-slot ───────────────────────────────────────────────
    let v_slot = match &element.v_slot {
        Some(prop) => {
            let slot_span = match (prop.value_start, prop.value_end) {
                (Some(vs), Some(ve)) => Some(Span::new(vs, ve)),
                _ => None,
            };
            let parsed = parse_vslot_with_bindings_sliced(
                alloc,
                slot_span,
                input,
                source_type,
                &provided_locals,
            );
            // Add v-slot locals to provided_locals
            for local_span in &parsed.locals {
                provided_locals.push(local_span.slice(input));
            }
            Some(OxcParsedVSlot { parsed })
        }
        None => None,
    };

    // ── 4. Regular props ────────────────────────────────────────
    let mut oxc_props: Vec<OxcParsedProp<'alloc>> = Vec::with_capacity(element.props.len());

    for (i, prop) in element.props.iter().enumerate() {
        if !prop.is_directive {
            // Static attribute — no OXC parsing needed.
            continue;
        }

        // Parse directive value expression
        let exp = match (prop.value_start, prop.value_end) {
            (Some(vs), Some(ve)) => {
                let parsed = parse_expression(
                    Span::new(vs, ve),
                    input,
                    alloc,
                    source_type,
                    &provided_locals,
                );

                // Check for expression flag based on arg name
                if parsed.dynamism == Dynamism::Static {
                    if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                        let arg_name = &input[as_ as usize..ae as usize];
                        match arg_name {
                            "class" => {
                                expression_flag =
                                    expression_flag.add(ExpressionFlags::StaticClassExpr);
                            }
                            "style" => {
                                expression_flag =
                                    expression_flag.add(ExpressionFlags::StaticStyleExpr);
                            }
                            "key" => {
                                expression_flag =
                                    expression_flag.add(ExpressionFlags::StaticKeyExpr);
                            }
                            _ => {}
                        }
                    }
                }

                Some(parsed)
            }
            _ => None,
        };

        // Parse dynamic arg expression (:[key]="value")
        let arg = match (prop.is_dynamic, prop.arg_start, prop.arg_end) {
            (Some(true), Some(as_), Some(ae)) => Some(parse_expression(
                Span::new(as_, ae),
                input,
                alloc,
                source_type,
                &provided_locals,
            )),
            _ => None,
        };

        // Only include props that have something parsed
        if exp.is_some() || arg.is_some() {
            oxc_props.push(OxcParsedProp {
                prop_index: i,
                arg,
                exp,
            });
        }
    }

    OxcParsedElement {
        condition,
        v_for,
        v_slot,
        props: oxc_props,
        provided_locals,
        expression_flag,
    }
}

/// Parse all template expressions in a single forward pass over the AST nodes vec.
///
/// Produces a parallel `OxcParsedAst` where `data[node_id.0]` contains the
/// OXC-parsed data for `ast.nodes[node_id.0]`. Parents are always at lower
/// indices than their children (allocated at `open_element`), so a forward
/// scan guarantees parent data is available when processing children.
///
/// Scope cascade: v-for/v-slot locals from a parent element are propagated
/// to children via `provided_locals`. Sibling elements do NOT share scopes.
///
/// `AllInterpolationsStatic` is set optimistically on elements with
/// interpolation children, then removed if any interpolation is non-Static.
pub fn parse_template_expressions<'alloc>(
    ast: &TemplateAst,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
) -> OxcParsedAst<'alloc> {
    let mut data: Vec<OxcNodeData<'alloc>> = Vec::with_capacity(ast.nodes.len());

    for node in &ast.nodes {
        // Get parent's provided_locals for scope cascade.
        // Parents always have lower indices, so data[pid.0] is already populated.
        let parent_locals: &[&'alloc str] = if let Some(pid) = node.parent {
            match &data[pid.0] {
                OxcNodeData::Element(el) => &el.provided_locals,
                _ => &[],
            }
        } else {
            &[] // root-level node
        };

        match &node.kind {
            AstNodeKind::Element(el) => {
                // Fast path: element with only static attributes (no directives,
                // no v-if/v-for/v-slot/v-once). Skip OXC parsing entirely —
                // no Box<OxcParsedElement> allocation needed.
                if !el.needs_expression_parsing() {
                    data.push(OxcNodeData::None);
                    continue;
                }

                let mut parsed = parse_element(el, parent_locals, input, alloc, source_type);

                // Optimistically set AllInterpolationsStatic if element has
                // interpolation children (from pre-computed children_flag).
                if el.children_flag.has(ChildrenFlags::HasInterpolation) {
                    parsed.expression_flag = parsed
                        .expression_flag
                        .add(ExpressionFlags::AllInterpolationsStatic);
                }

                data.push(OxcNodeData::Element(Box::new(parsed)));
            }
            AstNodeKind::Interpolation(interp) => {
                let expr = parse_expression(
                    Span::new(interp.inner_start, interp.inner_end),
                    input,
                    alloc,
                    source_type,
                    parent_locals,
                );

                // If non-static, remove AllInterpolationsStatic from parent.
                if expr.dynamism != Dynamism::Static {
                    if let Some(pid) = node.parent {
                        if let OxcNodeData::Element(parent_el) = &mut data[pid.0] {
                            parent_el.expression_flag = parent_el
                                .expression_flag
                                .remove(ExpressionFlags::AllInterpolationsStatic);
                        }
                    }
                }

                data.push(OxcNodeData::Interpolation(expr));
            }
            AstNodeKind::Text(_) | AstNodeKind::Comment(_) => {
                data.push(OxcNodeData::None);
            }
        }
    }

    OxcParsedAst { data }
}

#[cfg(test)]
mod parse_expression_tests {
    use super::*;
    use crate::utils::oxc::is_keyword;
    use oxc_allocator::Allocator;
    use oxc_span::SourceType;

    fn tsx() -> SourceType {
        SourceType::tsx()
    }

    // ── Test Group 1: parse_expression ──────────────────────────────

    /// Test 1: Empty span returns empty result with Static dynamism.
    #[test]
    fn empty_span_returns_static() {
        let alloc = Allocator::default();
        let result = parse_expression(Span::new(0, 0), "", &alloc, tsx(), &[]);
        assert!(result.expression.is_none());
        assert!(result.errors.is_none());
        assert!(result.bindings.is_none());
        assert_eq!(result.dynamism, Dynamism::Static);
        assert_eq!(result.offset, 0);
    }

    /// Test 2: Simple identifier is MaybeDynamic.
    #[test]
    fn simple_identifier_maybe_dynamic() {
        let alloc = Allocator::default();
        let input = "foo";
        let result = parse_expression(Span::new(0, 3), input, &alloc, tsx(), &[]);

        assert!(result.expression.is_some());
        assert!(result.errors.is_none());
        let bindings = result.bindings.as_ref().unwrap();
        assert_eq!(bindings.bindings.len(), 1);
        assert_eq!(bindings.bindings[0].name, "foo");
        assert!(!bindings.bindings[0].ignore);
        assert_eq!(result.dynamism, Dynamism::MaybeDynamic);
    }

    /// Test 3: String literal is Static.
    #[test]
    fn string_literal_static() {
        let alloc = Allocator::default();
        let input = "'hello'";
        let result = parse_expression(Span::new(0, input.len() as u32), input, &alloc, tsx(), &[]);

        assert!(result.expression.is_some());
        assert!(result.errors.is_none());
        let bindings = result.bindings.as_ref().unwrap();
        assert!(
            bindings.bindings.is_empty(),
            "String literal should have no identifier bindings"
        );
        assert_eq!(result.dynamism, Dynamism::Static);
    }

    /// Test 4: Numeric literal is Static.
    #[test]
    fn numeric_literal_static() {
        let alloc = Allocator::default();
        let input = "42";
        let result = parse_expression(Span::new(0, input.len() as u32), input, &alloc, tsx(), &[]);

        assert!(result.expression.is_some());
        let bindings = result.bindings.as_ref().unwrap();
        assert!(bindings.bindings.is_empty());
        assert_eq!(result.dynamism, Dynamism::Static);
    }

    /// Test 5: Boolean `true` is a BooleanLiteral (not identifier), so Static.
    #[test]
    fn boolean_literal_static() {
        let alloc = Allocator::default();
        let input = "true";
        let result = parse_expression(Span::new(0, input.len() as u32), input, &alloc, tsx(), &[]);

        assert!(result.expression.is_some());
        let bindings = result.bindings.as_ref().unwrap();
        // `true` is parsed as BooleanLiteral by OXC, not an identifier.
        // So it should not appear in bindings.bindings at all.
        let non_keyword_bindings: Vec<_> = bindings
            .bindings
            .iter()
            .filter(|b| !is_keyword(b.name.as_bytes()))
            .collect();
        assert!(
            non_keyword_bindings.is_empty(),
            "true should not produce non-keyword bindings"
        );
        assert_eq!(result.dynamism, Dynamism::Static);
    }

    /// Test 6: Ignored identifier (v-for local) is Dynamic.
    #[test]
    fn ignored_identifier_dynamic() {
        let alloc = Allocator::default();
        let input = "item";
        let result = parse_expression(Span::new(0, 4), input, &alloc, tsx(), &["item"]);

        assert!(result.expression.is_some());
        let bindings = result.bindings.as_ref().unwrap();
        assert_eq!(bindings.bindings.len(), 1);
        assert_eq!(bindings.bindings[0].name, "item");
        assert!(bindings.bindings[0].ignore);
        assert_eq!(result.dynamism, Dynamism::Dynamic);
    }

    /// Test 7: Member expression with ignored root is Dynamic.
    #[test]
    fn member_expr_ignored_root_dynamic() {
        let alloc = Allocator::default();
        let input = "item.name";
        let result = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            tsx(),
            &["item"],
        );

        assert!(result.expression.is_some());
        assert_eq!(result.dynamism, Dynamism::Dynamic);
    }

    /// Test 8: Binary expression of literals is Static.
    #[test]
    fn binary_literals_static() {
        let alloc = Allocator::default();
        let input = "1 + 2";
        let result = parse_expression(Span::new(0, input.len() as u32), input, &alloc, tsx(), &[]);

        assert!(result.expression.is_some());
        let bindings = result.bindings.as_ref().unwrap();
        assert!(bindings.bindings.is_empty());
        assert_eq!(result.dynamism, Dynamism::Static);
    }

    /// Test 9: Script-level identifier is MaybeDynamic.
    #[test]
    fn script_level_identifier_maybe_dynamic() {
        let alloc = Allocator::default();
        let input = "cls";
        let result = parse_expression(Span::new(0, input.len() as u32), input, &alloc, tsx(), &[]);

        assert!(result.expression.is_some());
        let bindings = result.bindings.as_ref().unwrap();
        assert_eq!(bindings.bindings.len(), 1);
        assert!(!bindings.bindings[0].ignore);
        assert_eq!(result.dynamism, Dynamism::MaybeDynamic);
    }

    /// Test 10: Mixed injected local + script-level → Dynamic (injected trumps).
    #[test]
    fn mixed_injected_and_script_dynamic() {
        let alloc = Allocator::default();
        let input = "item.name + cls";
        let result = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            tsx(),
            &["item"],
        );

        assert!(result.expression.is_some());
        // Has `item` (ignored, not keyword) → Dynamic
        assert_eq!(result.dynamism, Dynamism::Dynamic);
    }

    /// Test 11: Invalid syntax produces errors.
    #[test]
    fn invalid_syntax_errors() {
        let alloc = Allocator::default();
        let input = "if (";
        let result = parse_expression(Span::new(0, input.len() as u32), input, &alloc, tsx(), &[]);

        assert!(result.expression.is_none());
        assert!(result.errors.is_some());
        assert!(!result.errors.as_ref().unwrap().is_empty());
    }

    /// Test 12: Offset is stored correctly, expression spans are 0-based.
    #[test]
    fn offset_stored_correctly() {
        let alloc = Allocator::default();
        let input = "prefix foo suffix";
        // "foo" at offset 7..10
        let result = parse_expression(Span::new(7, 10), input, &alloc, tsx(), &[]);

        assert_eq!(result.offset, 7);
        assert!(result.expression.is_some());

        // Expression AST spans should be 0-based (substring-relative)
        let expr = result.expression.as_ref().unwrap();
        use oxc_span::GetSpan;
        let expr_span = expr.span();
        assert_eq!(
            expr_span.start, 0,
            "Expression AST span should be 0-based (substring-relative)"
        );
        assert_eq!(expr_span.end, 3, "Expression AST span end should be 3");

        // Bindings should have file-relative positions (via base_offset)
        let bindings = result.bindings.as_ref().unwrap();
        assert_eq!(bindings.bindings[0].name, "foo");
        assert_eq!(
            bindings.bindings[0].pos, 7,
            "Binding pos should be file-relative (offset 7)"
        );
    }
}

// ── Test Group 2: parse_element ─────────────────────────────────

#[cfg(test)]
mod parse_element_tests {
    use super::*;
    use crate::new_impl::ast::types::*;
    use crate::new_impl::types::{NodeProp, NodeTag};
    use oxc_allocator::Allocator;
    use oxc_span::SourceType;
    use smallvec::SmallVec;

    fn tsx() -> SourceType {
        SourceType::tsx()
    }

    /// Build an ElementNode with given configuration.
    fn make_element(
        tag_type: TagType,
        props: Vec<NodeProp>,
        v_condition: Option<ElementNodeCondition>,
        v_for: Option<NodeProp>,
        v_slot: Option<NodeProp>,
        v_once: Option<NodeProp>,
        prop_flag: PropFlag,
    ) -> ElementNode {
        ElementNode {
            tag_open: NodeTag {
                start: 0,
                end: 0,
                name_end: 0,
            },
            tag_close: None,
            tag_type,
            is_self_closing: false,
            props,
            content: None,
            v_condition,
            v_for,
            v_slot,
            v_once,
            v_ref: None,
            prop_flag,
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
        }
    }

    /// Build a plain (non-directive) attribute prop.
    fn plain_attr(
        start: u32,
        name_end: u32,
        value_start: Option<u32>,
        value_end: Option<u32>,
    ) -> NodeProp {
        NodeProp {
            start,
            name_end,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            value_start,
            value_end,
            modifiers: SmallVec::new(),
            is_dynamic: None,
        }
    }

    /// Build a directive prop with static arg.
    fn directive_prop(
        start: u32,
        name_end: u32,
        arg_start: Option<u32>,
        arg_end: Option<u32>,
        value_start: Option<u32>,
        value_end: Option<u32>,
    ) -> NodeProp {
        NodeProp {
            start,
            name_end,
            is_directive: true,
            arg_start,
            arg_end,
            value_start,
            value_end,
            modifiers: SmallVec::new(),
            is_dynamic: Some(false),
        }
    }

    /// Build a directive prop with no arg (e.g., v-once, v-if, v-for).
    fn directive_prop_no_arg(
        start: u32,
        name_end: u32,
        value_start: Option<u32>,
        value_end: Option<u32>,
    ) -> NodeProp {
        NodeProp {
            start,
            name_end,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            value_start,
            value_end,
            modifiers: SmallVec::new(),
            is_dynamic: None,
        }
    }

    // ── Test 1: Plain element, no directives ────────────────────

    /// `<div class="foo">` — plain attribute, no OXC parsing needed.
    #[test]
    fn plain_element_empty_result() {
        //  <div class="foo">
        //  0    5     12  16
        let input = r#"<div class="foo">"#;
        let el = make_element(
            TagType::Element,
            vec![plain_attr(5, 10, Some(12), Some(15))],
            None,
            None,
            None,
            None,
            PropFlag::empty().add(PropFlags::HasStaticClass),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert!(result.condition.is_none());
        assert!(result.v_for.is_none());
        assert!(result.v_slot.is_none());
        assert!(
            result.props.is_empty(),
            "Plain attributes need no OXC parsing"
        );
        assert!(result.expression_flag.is_empty());
        assert!(result.provided_locals.is_empty());
    }

    // ── Test 2: :class with dynamic expression ──────────────────

    /// `<div :class="cls">` — MaybeDynamic, no StaticClassExpr flag.
    #[test]
    fn dynamic_class_no_static_flag() {
        //  <div :class="cls">
        //  0    56    1213 17
        let input = r#"<div :class="cls">"#;
        let el = make_element(
            TagType::Element,
            vec![directive_prop(5, 11, Some(6), Some(11), Some(13), Some(16))],
            None,
            None,
            None,
            None,
            PropFlag::empty().add(PropFlags::HasDynamicClass),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert_eq!(result.props.len(), 1);
        let exp = result.props[0]
            .exp
            .as_ref()
            .expect("should have parsed exp");
        assert_eq!(exp.dynamism, Dynamism::MaybeDynamic);
        assert!(
            !result.expression_flag.has(ExpressionFlags::StaticClassExpr),
            "MaybeDynamic class should NOT set StaticClassExpr"
        );
    }

    // ── Test 3: :class with static expression ───────────────────

    /// `<div :class="'active'">` — Static, StaticClassExpr flag set.
    #[test]
    fn static_class_sets_flag() {
        //  <div :class="'active'">
        //  0    56    1213      22
        let input = r#"<div :class="'active'">"#;
        let el = make_element(
            TagType::Element,
            vec![directive_prop(5, 11, Some(6), Some(11), Some(13), Some(21))],
            None,
            None,
            None,
            None,
            PropFlag::empty().add(PropFlags::HasDynamicClass),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert_eq!(result.props.len(), 1);
        let exp = result.props[0].exp.as_ref().unwrap();
        assert_eq!(exp.dynamism, Dynamism::Static);
        assert!(
            result.expression_flag.has(ExpressionFlags::StaticClassExpr),
            "Static class should set StaticClassExpr"
        );
    }

    // ── Test 4: :style with static expression ───────────────────

    /// `<div :style="{ color: 'red' }">` — Static, StaticStyleExpr set.
    #[test]
    fn static_style_sets_flag() {
        //  <div :style="{ color: 'red' }">
        //  0    56    1213              30
        let input = r#"<div :style="{ color: 'red' }">"#;
        let el = make_element(
            TagType::Element,
            vec![directive_prop(5, 11, Some(6), Some(11), Some(13), Some(29))],
            None,
            None,
            None,
            None,
            PropFlag::empty().add(PropFlags::HasDynamicStyle),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert_eq!(result.props.len(), 1);
        let exp = result.props[0].exp.as_ref().unwrap();
        assert_eq!(exp.dynamism, Dynamism::Static);
        assert!(
            result.expression_flag.has(ExpressionFlags::StaticStyleExpr),
            "Static style should set StaticStyleExpr"
        );
    }

    // ── Test 5: :key with static expression ─────────────────────

    /// `<div :key="'my-key'">` — Static, StaticKeyExpr set.
    #[test]
    fn static_key_sets_flag() {
        //  <div :key="'my-key'">
        //  0    56  1011      20
        let input = r#"<div :key="'my-key'">"#;
        let el = make_element(
            TagType::Element,
            vec![directive_prop(5, 9, Some(6), Some(9), Some(11), Some(19))],
            None,
            None,
            None,
            None,
            PropFlag::empty().add(PropFlags::HasDynamicKey),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert_eq!(result.props.len(), 1);
        let exp = result.props[0].exp.as_ref().unwrap();
        assert_eq!(exp.dynamism, Dynamism::Static);
        assert!(
            result.expression_flag.has(ExpressionFlags::StaticKeyExpr),
            "Static key should set StaticKeyExpr"
        );
    }

    // ── Test 6: v-if with dynamic condition ─────────────────────

    /// `<div v-if="show">` — condition MaybeDynamic.
    #[test]
    fn v_if_dynamic_condition() {
        //  <div v-if="show">
        //  0    5   1011  16
        let input = r#"<div v-if="show">"#;
        let cond_prop = directive_prop_no_arg(5, 9, Some(11), Some(15));
        let el = make_element(
            TagType::Element,
            vec![],
            Some(ElementNodeCondition {
                kind: ElementNodeConditionKind::If,
                prop: cond_prop,
            }),
            None,
            None,
            None,
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        let condition = result.condition.as_ref().expect("should have condition");
        assert_eq!(condition.dynamism, Dynamism::MaybeDynamic);
        assert!(
            !result.expression_flag.has(ExpressionFlags::StaticCondition),
            "MaybeDynamic condition should NOT set StaticCondition"
        );
    }

    // ── Test 7: v-if with static condition ──────────────────────

    /// `<div v-if="true">` — condition Static, StaticCondition flag set.
    #[test]
    fn v_if_static_condition() {
        //  <div v-if="true">
        //  0    5   1011  16
        let input = r#"<div v-if="true">"#;
        let cond_prop = directive_prop_no_arg(5, 9, Some(11), Some(15));
        let el = make_element(
            TagType::Element,
            vec![],
            Some(ElementNodeCondition {
                kind: ElementNodeConditionKind::If,
                prop: cond_prop,
            }),
            None,
            None,
            None,
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        let condition = result.condition.as_ref().expect("should have condition");
        assert_eq!(condition.dynamism, Dynamism::Static);
        assert!(
            result.expression_flag.has(ExpressionFlags::StaticCondition),
            "Static condition should set StaticCondition"
        );
    }

    // ── Test 8: v-for provides locals ───────────────────────────

    /// `<div v-for="item of items">` — v_for parsed, provided_locals has "item".
    #[test]
    fn v_for_provides_locals() {
        //  <div v-for="item of items">
        //  0    5   10 12           25
        let input = r#"<div v-for="item of items">"#;
        let vfor_prop = directive_prop_no_arg(5, 10, Some(12), Some(25));
        let el = make_element(
            TagType::Element,
            vec![],
            None,
            Some(vfor_prop),
            None,
            None,
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert!(result.v_for.is_some(), "should have parsed v-for");
        assert!(
            result.provided_locals.contains(&"item"),
            "provided_locals should contain 'item', got: {:?}",
            result.provided_locals
        );
    }

    // ── Test 9: v-slot provides locals ──────────────────────────

    /// `<template #default="{ data }">` — v_slot parsed, provided_locals has "data".
    #[test]
    fn v_slot_provides_locals() {
        //  <template #default="{ data }">
        //  0         10     18 20     28
        let input = r#"<template #default="{ data }">"#;
        let vslot_prop = directive_prop(10, 18, Some(11), Some(18), Some(20), Some(28));
        let el = make_element(
            TagType::Template,
            vec![],
            None,
            None,
            Some(vslot_prop),
            None,
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert!(result.v_slot.is_some(), "should have parsed v-slot");
        assert!(
            result.provided_locals.contains(&"data"),
            "provided_locals should contain 'data', got: {:?}",
            result.provided_locals
        );
    }

    // ── Test 10: Mixed static + dynamic props ───────────────────

    /// `<div :class="'foo'" :id="computedId">` — class Static+flag, id MaybeDynamic.
    #[test]
    fn mixed_static_and_dynamic_props() {
        //  <div :class="'foo'" :id="computedId">
        //  0    56    1213  18 2021 2526        37
        let input = r#"<div :class="'foo'" :id="computedId">"#;
        let el = make_element(
            TagType::Element,
            vec![
                // :class="'foo'"  — arg "class" at 6..11, value "'foo'" at 13..18
                directive_prop(5, 11, Some(6), Some(11), Some(13), Some(18)),
                // :id="computedId" — arg "id" at 21..23, value "computedId" at 25..35
                directive_prop(20, 23, Some(21), Some(23), Some(25), Some(35)),
            ],
            None,
            None,
            None,
            None,
            PropFlag::empty()
                .add(PropFlags::HasDynamicClass)
                .add(PropFlags::HasDynamicKey), // :id not a special flag, but props still parsed
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert_eq!(result.props.len(), 2);

        // First prop: :class="'foo'" → Static → flag set
        let class_exp = result.props[0].exp.as_ref().unwrap();
        assert_eq!(class_exp.dynamism, Dynamism::Static);
        assert!(result.expression_flag.has(ExpressionFlags::StaticClassExpr));

        // Second prop: :id="computedId" → MaybeDynamic → no special flag
        let id_exp = result.props[1].exp.as_ref().unwrap();
        assert_eq!(id_exp.dynamism, Dynamism::MaybeDynamic);
    }

    // ── Test 11: v-once only ────────────────────────────────────

    /// `<div v-once>` — no expressions parsed (v-once is handled by codegen from AST).
    #[test]
    fn v_once_no_expressions() {
        //  <div v-once>
        //  0    5    10
        let input = "<div v-once>";
        let vonce_prop = directive_prop_no_arg(5, 11, None, None);
        let el = make_element(
            TagType::Element,
            vec![],
            None,
            None,
            None,
            Some(vonce_prop),
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert!(result.condition.is_none());
        assert!(result.v_for.is_none());
        assert!(result.v_slot.is_none());
        assert!(result.props.is_empty());
        assert!(result.expression_flag.is_empty());
    }

    // ── Test 12: v-once + v-if ──────────────────────────────────

    /// `<div v-once v-if="show">` — condition still parsed, v-once doesn't suppress OXC.
    #[test]
    fn v_once_with_v_if() {
        //  <div v-once v-if="show">
        //  0    5     12   1718  23
        let input = r#"<div v-once v-if="show">"#;
        let vonce_prop = directive_prop_no_arg(5, 11, None, None);
        let cond_prop = directive_prop_no_arg(12, 16, Some(18), Some(22));
        let el = make_element(
            TagType::Element,
            vec![],
            Some(ElementNodeCondition {
                kind: ElementNodeConditionKind::If,
                prop: cond_prop,
            }),
            None,
            None,
            Some(vonce_prop),
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert!(
            result.condition.is_some(),
            "v-once should not suppress v-if parsing"
        );
        assert_eq!(
            result.condition.as_ref().unwrap().dynamism,
            Dynamism::MaybeDynamic
        );
    }

    // ── Test 13: v-once + :class ────────────────────────────────

    /// `<div v-once :class="cls">` — prop still parsed, v-once doesn't make it static.
    #[test]
    fn v_once_with_class() {
        //  <div v-once :class="cls">
        //  0    5     1213   1920 24
        let input = r#"<div v-once :class="cls">"#;
        let vonce_prop = directive_prop_no_arg(5, 11, None, None);
        let el = make_element(
            TagType::Element,
            vec![directive_prop(
                12,
                18,
                Some(13),
                Some(18),
                Some(20),
                Some(23),
            )],
            None,
            None,
            None,
            Some(vonce_prop),
            PropFlag::empty().add(PropFlags::HasDynamicClass),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert_eq!(result.props.len(), 1);
        let exp = result.props[0].exp.as_ref().unwrap();
        assert_eq!(
            exp.dynamism,
            Dynamism::MaybeDynamic,
            "v-once should not affect dynamism classification"
        );
        assert!(
            !result.expression_flag.has(ExpressionFlags::StaticClassExpr),
            "v-once should not make the expression static"
        );
    }

    // ── Test 14: v-once + v-for ─────────────────────────────────

    /// `<div v-once v-for="item of items">` — v-for still parsed, v-once coexists.
    #[test]
    fn v_once_with_v_for() {
        //  <div v-once v-for="item of items">
        //  0    5    11 12  17 19           32
        let input = r#"<div v-once v-for="item of items">"#;
        let vonce_prop = directive_prop_no_arg(5, 11, None, None);
        let vfor_prop = directive_prop_no_arg(12, 17, Some(19), Some(32));
        let el = make_element(
            TagType::Element,
            vec![],
            None,
            Some(vfor_prop),
            None,
            Some(vonce_prop),
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert!(
            result.v_for.is_some(),
            "v-once should not suppress v-for parsing"
        );
        assert!(
            result.provided_locals.contains(&"item"),
            "v-for locals should still be provided, got: {:?}",
            result.provided_locals
        );
    }

    // ── Test 15: v-else — no expression ─────────────────────────

    /// `<div v-else>` — v-else has no condition expression, result.condition is None.
    // @ai-generated - Tests v-else produces no condition expression
    #[test]
    fn v_else_no_expression() {
        //  <div v-else>
        //  0    5    11
        let input = "<div v-else>";
        let cond_prop = directive_prop_no_arg(5, 11, None, None);
        let el = make_element(
            TagType::Element,
            vec![],
            Some(ElementNodeCondition {
                kind: ElementNodeConditionKind::Else,
                prop: cond_prop,
            }),
            None,
            None,
            None,
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert!(
            result.condition.is_none(),
            "v-else should produce no condition expression"
        );
        assert!(
            !result.expression_flag.has(ExpressionFlags::StaticCondition),
            "v-else should not set StaticCondition"
        );
    }

    // ── Test 16: v-else-if with dynamic condition ───────────────

    /// `<div v-else-if="count > 0">` — condition MaybeDynamic, same as v-if.
    // @ai-generated - Tests v-else-if condition parsing
    #[test]
    fn v_else_if_dynamic_condition() {
        //  <div v-else-if="count > 0">
        //  0    5       14 16       25
        let input = r#"<div v-else-if="count > 0">"#;
        let cond_prop = directive_prop_no_arg(5, 14, Some(16), Some(25));
        let el = make_element(
            TagType::Element,
            vec![],
            Some(ElementNodeCondition {
                kind: ElementNodeConditionKind::ElseIf,
                prop: cond_prop,
            }),
            None,
            None,
            None,
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        let condition = result.condition.as_ref().expect("should have condition");
        assert_eq!(condition.dynamism, Dynamism::MaybeDynamic);
        assert!(
            !result.expression_flag.has(ExpressionFlags::StaticCondition),
            "MaybeDynamic v-else-if should NOT set StaticCondition"
        );
    }

    // ── Test 17: Dynamic arg :[key]="value" ─────────────────────

    /// `<div :[attr]="val">` — both arg and exp are parsed.
    // @ai-generated - Tests dynamic arg expression parsing
    #[test]
    fn dynamic_arg_parsed() {
        //  <div :[attr]="val">
        //  0    56 7  1011 1314 1516 18
        let input = r#"<div :[attr]="val">"#;
        let el = make_element(
            TagType::Element,
            vec![NodeProp {
                start: 5,
                name_end: 12,
                is_directive: true,
                arg_start: Some(7),
                arg_end: Some(11),
                value_start: Some(14),
                value_end: Some(17),
                modifiers: SmallVec::new(),
                is_dynamic: Some(true),
            }],
            None,
            None,
            None,
            None,
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert_eq!(result.props.len(), 1);
        let prop = &result.props[0];

        // Dynamic arg should be parsed
        let arg = prop.arg.as_ref().expect("dynamic arg should be parsed");
        assert!(arg.expression.is_some());
        assert_eq!(arg.dynamism, Dynamism::MaybeDynamic);

        // Value expression should also be parsed
        let exp = prop.exp.as_ref().expect("value should be parsed");
        assert!(exp.expression.is_some());
        assert_eq!(exp.dynamism, Dynamism::MaybeDynamic);
    }

    // ── Test 18: Argless directive (v-show) ─────────────────────

    /// `<div v-show="visible">` — directive with value but no arg.
    // @ai-generated - Tests argless directive value parsing
    #[test]
    fn argless_directive_with_value() {
        //  <div v-show="visible">
        //  0    5    10 12     19
        let input = r#"<div v-show="visible">"#;
        let el = make_element(
            TagType::Element,
            vec![directive_prop_no_arg(5, 11, Some(13), Some(20))],
            None,
            None,
            None,
            None,
            PropFlag::empty().add(PropFlags::HasShow),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert_eq!(result.props.len(), 1);
        let prop = &result.props[0];
        assert!(prop.arg.is_none(), "v-show has no arg");
        let exp = prop.exp.as_ref().expect("v-show value should be parsed");
        assert!(exp.expression.is_some());
        assert_eq!(exp.dynamism, Dynamism::MaybeDynamic);
    }

    // ── Test 19: Directive with no value and no arg ──────────────

    /// `<div v-cloak>` — directive with neither arg nor value produces no OxcParsedProp.
    // @ai-generated - Tests valueless/argless directive is skipped
    #[test]
    fn directive_no_value_no_arg_skipped() {
        //  <div v-cloak>
        //  0    5     12
        let input = "<div v-cloak>";
        let el = make_element(
            TagType::Element,
            vec![directive_prop_no_arg(5, 12, None, None)],
            None,
            None,
            None,
            None,
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx());

        assert!(
            result.props.is_empty(),
            "Directive with no value and no dynamic arg should not produce OxcParsedProp"
        );
    }

    // ── Test 20: Parent-provided locals affect directive expressions ──

    /// When parent_ignored contains "item", a :class="item.cls" expression
    /// should be classified as Dynamic.
    // @ai-generated - Tests parent locals propagate to directive dynamism
    #[test]
    fn parent_locals_affect_directive_expressions() {
        //  <span :class="item.cls">
        //  0     67    1213      22
        let input = r#"<span :class="item.cls">"#;
        let el = make_element(
            TagType::Element,
            vec![directive_prop(6, 12, Some(7), Some(12), Some(14), Some(22))],
            None,
            None,
            None,
            None,
            PropFlag::empty().add(PropFlags::HasDynamicClass),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &["item"], input, &alloc, tsx());

        assert_eq!(result.props.len(), 1);
        let exp = result.props[0].exp.as_ref().unwrap();
        assert_eq!(
            exp.dynamism,
            Dynamism::Dynamic,
            "item.cls should be Dynamic because 'item' is a parent-provided local"
        );
    }
}

// ── Test Group 3: DFS traversal + scope cascade ────────────────

#[cfg(test)]
mod parse_template_expressions_tests {
    use super::*;
    use crate::new_impl::ast::builder::TemplateAstBuilder;
    use crate::new_impl::ast::types::*;
    use crate::new_impl::test_helpers::{make_root, make_tag};
    use crate::new_impl::types::NodeProp;
    use oxc_allocator::Allocator;
    use oxc_span::SourceType;
    use smallvec::SmallVec;

    fn tsx() -> SourceType {
        SourceType::tsx()
    }

    /// Find the byte offset of a substring within the input.
    fn find_pos(input: &str, needle: &str) -> u32 {
        input.find(needle).unwrap() as u32
    }

    fn directive_prop_no_arg(
        start: u32,
        name_end: u32,
        value_start: Option<u32>,
        value_end: Option<u32>,
    ) -> NodeProp {
        NodeProp {
            start,
            name_end,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            value_start,
            value_end,
            modifiers: SmallVec::new(),
            is_dynamic: None,
        }
    }

    fn directive_prop(
        start: u32,
        name_end: u32,
        arg_start: Option<u32>,
        arg_end: Option<u32>,
        value_start: Option<u32>,
        value_end: Option<u32>,
    ) -> NodeProp {
        NodeProp {
            start,
            name_end,
            is_directive: true,
            arg_start,
            arg_end,
            value_start,
            value_end,
            modifiers: SmallVec::new(),
            is_dynamic: Some(false),
        }
    }

    // ── Test 1: Root-level interpolation ─────────────────────────

    /// `{{ foo }}` at root level — Interpolation, MaybeDynamic.
    #[test]
    fn root_interpolation_maybe_dynamic() {
        let input = "{{ foo }}";
        // {{ foo }} — inner "foo" at 3..6
        let mut b = TemplateAstBuilder::new(make_root());
        b.add_interpolation(0, 9, 3, 6);
        let ast = b.finish();

        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        assert_eq!(result.data.len(), ast.nodes.len());
        match &result.data[0] {
            OxcNodeData::Interpolation(expr) => {
                assert_eq!(expr.dynamism, Dynamism::MaybeDynamic);
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    // ── Test 2: v-for scope cascades to interpolation child ──────

    /// `<div v-for="item of items">{{ item.name }}</div>`
    /// — interpolation is Dynamic because "item" is a v-for local.
    #[test]
    fn v_for_scope_cascades_to_child() {
        let input = r#"<div v-for="item of items">{{ item.name }}</div>"#;
        //             0    5   10 12           25 27 30       39 42    48

        let mut b = TemplateAstBuilder::new(make_root());

        // <div v-for="item of items">
        b.open_element(make_tag(0, 27, 4));
        b.set_v_for(directive_prop_no_arg(5, 10, Some(12), Some(25)));
        b.mark_element_content_start(27);

        // {{ item.name }}
        b.add_interpolation(27, 42, 30, 39);

        // </div>
        b.close_element(Some(make_tag(42, 48, 47)), 42);

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Node 0 = div, Node 1 = interpolation
        match &result.data[1] {
            OxcNodeData::Interpolation(expr) => {
                assert_eq!(
                    expr.dynamism,
                    Dynamism::Dynamic,
                    "item.name should be Dynamic because 'item' is a v-for local"
                );
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    // ── Test 3: v-slot scope cascades to interpolation child ─────

    /// `<template #default="{ data }">{{ data.value }}</template>`
    /// — interpolation is Dynamic because "data" is a v-slot local.
    #[test]
    fn v_slot_scope_cascades_to_child() {
        let input = r#"<template #default="{ data }">{{ data.value }}</template>"#;
        //             0         10      18 20      28 30 33         43 46         57

        let mut b = TemplateAstBuilder::new(make_root());

        // <template #default="{ data }">
        b.open_element(make_tag(0, 30, 9));
        b.set_tag_type(TagType::Template);
        b.set_v_slot(directive_prop(
            10,
            18,
            Some(11),
            Some(18),
            Some(20),
            Some(28),
        ));
        b.mark_element_content_start(30);

        // {{ data.value }}
        b.add_interpolation(30, 46, 33, 43);

        // </template>
        b.close_element(Some(make_tag(46, 57, 56)), 46);

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Node 0 = template element, Node 1 = interpolation
        match &result.data[1] {
            OxcNodeData::Interpolation(expr) => {
                assert_eq!(
                    expr.dynamism,
                    Dynamism::Dynamic,
                    "data.value should be Dynamic because 'data' is a v-slot local"
                );
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    // ── Test 4: Nested v-for + v-slot, both locals propagated ────

    /// Nested: v-for on outer div provides "item", v-slot on inner template
    /// provides "data". The interpolation child should see both as locals.
    #[test]
    fn nested_v_for_and_v_slot_locals_propagated() {
        let input = r#"<div v-for="item of items"><template #default="{ data }">{{ item }}</template></div>"#;

        let hash_pos = find_pos(input, "#default");
        let slot_value_start = find_pos(input, "{ data }");
        let slot_value_end = slot_value_start + 8;
        let tmpl_content_start = find_pos(input, ">{{ item }}") + 1;
        let interp_start = find_pos(input, "{{ item }}");
        let interp_end = interp_start + 10;
        let item_start = interp_start + 3;
        let item_end = item_start + 4;
        let tmpl_close_start = find_pos(input, "</template>");
        let tmpl_close_end = tmpl_close_start + 11;
        let div_close_start = find_pos(input, "</div>");
        let div_close_end = div_close_start + 6;

        let mut b = TemplateAstBuilder::new(make_root());

        // <div v-for="item of items">
        b.open_element(make_tag(0, 27, 4));
        b.set_v_for(directive_prop_no_arg(5, 10, Some(12), Some(25)));
        b.mark_element_content_start(27);

        // <template #default="{ data }">
        let tmpl_start = find_pos(input, "<template");
        b.open_element(make_tag(tmpl_start, tmpl_content_start, tmpl_start + 9));
        b.set_tag_type(TagType::Template);
        b.set_v_slot(directive_prop(
            hash_pos,
            hash_pos + 8,
            Some(hash_pos + 1),
            Some(hash_pos + 8),
            Some(slot_value_start),
            Some(slot_value_end),
        ));
        b.mark_element_content_start(tmpl_content_start);

        // {{ item }}
        b.add_interpolation(interp_start, interp_end, item_start, item_end);

        // </template>
        b.close_element(
            Some(make_tag(
                tmpl_close_start,
                tmpl_close_end,
                tmpl_close_end - 1,
            )),
            tmpl_close_start,
        );

        // </div>
        b.close_element(
            Some(make_tag(div_close_start, div_close_end, div_close_end - 1)),
            div_close_start,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Nodes: 0=div, 1=template, 2=interpolation
        // The interpolation should see both "item" (from v-for) and "data" (from v-slot)
        match &result.data[2] {
            OxcNodeData::Interpolation(expr) => {
                assert_eq!(
                    expr.dynamism,
                    Dynamism::Dynamic,
                    "'item' is a v-for local → Dynamic"
                );
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }

        // Template element should have both locals
        match &result.data[1] {
            OxcNodeData::Element(el) => {
                assert!(
                    el.provided_locals.contains(&"item"),
                    "template should inherit 'item' from parent v-for, got: {:?}",
                    el.provided_locals
                );
                assert!(
                    el.provided_locals.contains(&"data"),
                    "template should have 'data' from v-slot, got: {:?}",
                    el.provided_locals
                );
            }
            other => panic!("expected Element, got {:?}", std::mem::discriminant(other)),
        }
    }

    // ── Test 5: Text and Comment produce OxcNodeData::None ───────

    #[test]
    fn text_and_comment_produce_none() {
        let input = "hello <!-- comment -->";
        let mut b = TemplateAstBuilder::new(make_root());
        b.add_text(0, 6, false);
        b.add_comment(6, 22, 11, 18);
        let ast = b.finish();

        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        assert_eq!(result.data.len(), 2);
        assert!(matches!(result.data[0], OxcNodeData::None));
        assert!(matches!(result.data[1], OxcNodeData::None));
    }

    // ── Test 6: Sibling isolation — v-for scope doesn't leak ─────

    /// `<div v-for="item of items"></div><span>{{ item }}</span>`
    /// — "item" is NOT in scope for span's child (sibling isolation).
    #[test]
    fn sibling_isolation_v_for_scope_doesnt_leak() {
        let input = r#"<div v-for="item of items"></div><span>{{ item }}</span>"#;

        let span_start = find_pos(input, "<span>");
        let span_tag_end = span_start + 6;
        let interp_start = find_pos(input, "{{ item }}");
        let interp_end = interp_start + 10;
        let item_expr_start = interp_start + 3;
        let item_expr_end = item_expr_start + 4;
        let span_close_start = find_pos(input, "</span>");
        let span_close_end = span_close_start + 7;

        let mut b = TemplateAstBuilder::new(make_root());

        // <div v-for="item of items"></div>
        b.open_element(make_tag(0, 27, 4));
        b.set_v_for(directive_prop_no_arg(5, 10, Some(12), Some(25)));
        b.mark_element_content_start(27);
        b.close_element(Some(make_tag(27, 33, 32)), 27);

        // <span>{{ item }}</span>
        b.open_element(make_tag(span_start, span_tag_end, span_start + 5));
        b.mark_element_content_start(span_tag_end);
        b.add_interpolation(interp_start, interp_end, item_expr_start, item_expr_end);
        b.close_element(
            Some(make_tag(
                span_close_start,
                span_close_end,
                span_close_end - 1,
            )),
            span_close_start,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Nodes: 0=div (with v-for), 1=span, 2=interpolation (child of span)
        match &result.data[2] {
            OxcNodeData::Interpolation(expr) => {
                assert_eq!(
                    expr.dynamism,
                    Dynamism::MaybeDynamic,
                    "'item' should be MaybeDynamic for span's child (v-for scope is isolated to div)"
                );
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    // ── Test 7: AllInterpolationsStatic flag set ─────────────────

    /// `<div>{{ 'hello' }}</div>` — plain div is skipped (OxcNodeData::None),
    /// but the interpolation child is still parsed correctly.
    #[test]
    fn all_interpolations_static_flag() {
        let input = r#"<div>{{ 'hello' }}</div>"#;

        let interp_start = find_pos(input, "{{ 'hello' }}");
        let interp_end = interp_start + 13;
        let expr_start = interp_start + 3;
        let expr_end = expr_start + 7; // 'hello' is 7 chars
        let close_start = find_pos(input, "</div>");

        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_interpolation(interp_start, interp_end, expr_start, expr_end);
        b.close_element(
            Some(make_tag(close_start, close_start + 6, close_start + 5)),
            close_start,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Node 0 = div element — plain, so skipped (None)
        assert!(
            matches!(result.data[0], OxcNodeData::None),
            "Plain div should be skipped (OxcNodeData::None)"
        );

        // Node 1 = interpolation — still parsed
        match &result.data[1] {
            OxcNodeData::Interpolation(expr) => {
                assert_eq!(expr.dynamism, Dynamism::Static);
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    // ── Test 8: Mixed interpolations, plain parent skipped ─────

    /// `<div>{{ 'hello' }}{{ foo }}</div>` — plain div is skipped,
    /// but both interpolation children are still parsed correctly.
    #[test]
    fn mixed_interpolations_no_all_static_flag() {
        let input = r#"<div>{{ 'hello' }}{{ foo }}</div>"#;

        let interp1_start = find_pos(input, "{{ 'hello' }}");
        let interp1_end = interp1_start + 13;
        let expr1_start = interp1_start + 3;
        let expr1_end = expr1_start + 7;

        let interp2_start = find_pos(input, "{{ foo }}");
        let interp2_end = interp2_start + 9;
        let expr2_start = interp2_start + 3;
        let expr2_end = expr2_start + 3;

        let close_start = find_pos(input, "</div>");

        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_interpolation(interp1_start, interp1_end, expr1_start, expr1_end);
        b.add_interpolation(interp2_start, interp2_end, expr2_start, expr2_end);
        b.close_element(
            Some(make_tag(close_start, close_start + 6, close_start + 5)),
            close_start,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Node 0 = div element — plain, so skipped
        assert!(matches!(result.data[0], OxcNodeData::None));

        // Node 1 = first interpolation (static)
        match &result.data[1] {
            OxcNodeData::Interpolation(expr) => {
                assert_eq!(expr.dynamism, Dynamism::Static);
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }

        // Node 2 = second interpolation (MaybeDynamic)
        match &result.data[2] {
            OxcNodeData::Interpolation(expr) => {
                assert_eq!(expr.dynamism, Dynamism::MaybeDynamic);
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    // ── Test 9: Plain nested element inherits empty locals ───────

    /// `<div><span>{{ foo }}</span></div>` — both div and span are plain,
    /// so both are skipped (OxcNodeData::None). Interpolation child still parsed.
    // @ai-generated - Tests plain nested element scope inheritance with skip optimization
    #[test]
    fn plain_nested_element_empty_locals() {
        let input = r#"<div><span>{{ foo }}</span></div>"#;

        let span_start = find_pos(input, "<span>");
        let span_end = span_start + 6;
        let interp_start = find_pos(input, "{{ foo }}");
        let interp_end = interp_start + 9;
        let expr_start = interp_start + 3;
        let expr_end = expr_start + 3;
        let span_close_start = find_pos(input, "</span>");
        let span_close_end = span_close_start + 7;
        let div_close_start = find_pos(input, "</div>");
        let div_close_end = div_close_start + 6;

        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        b.open_element(make_tag(span_start, span_end, span_start + 5));
        b.mark_element_content_start(span_end);

        b.add_interpolation(interp_start, interp_end, expr_start, expr_end);

        b.close_element(
            Some(make_tag(
                span_close_start,
                span_close_end,
                span_close_end - 1,
            )),
            span_close_start,
        );

        b.close_element(
            Some(make_tag(div_close_start, div_close_end, div_close_end - 1)),
            div_close_start,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Nodes: 0=div, 1=span, 2=interpolation
        // Both div and span are plain → skipped
        assert!(matches!(result.data[0], OxcNodeData::None));
        assert!(matches!(result.data[1], OxcNodeData::None));

        // Interpolation child still parsed correctly (no locals from skipped parents)
        match &result.data[2] {
            OxcNodeData::Interpolation(expr) => {
                assert_eq!(expr.dynamism, Dynamism::MaybeDynamic);
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    // ── Test 10: Error propagation in interpolation ──────────────

    /// `<div>{{ if ( }}</div>` — invalid syntax in interpolation produces errors.
    // @ai-generated - Tests error propagation through parse_template_expressions
    #[test]
    fn interpolation_error_propagation() {
        let input = r#"<div>{{ if ( }}</div>"#;

        let interp_start = find_pos(input, "{{ if ( }}");
        let interp_end = interp_start + 10;
        let expr_start = interp_start + 3;
        let expr_end = interp_start + 7; // "if ("

        let close_start = find_pos(input, "</div>");

        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_interpolation(interp_start, interp_end, expr_start, expr_end);
        b.close_element(
            Some(make_tag(close_start, close_start + 6, close_start + 5)),
            close_start,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Node 1 = interpolation
        match &result.data[1] {
            OxcNodeData::Interpolation(expr) => {
                assert!(
                    expr.expression.is_none(),
                    "Invalid syntax should produce no expression"
                );
                assert!(
                    expr.errors.is_some() && !expr.errors.as_ref().unwrap().is_empty(),
                    "Invalid syntax should produce errors"
                );
            }
            other => panic!(
                "expected Interpolation, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    // ── Test 11: Element without HasInterpolation flag ────────────

    /// `<div>hello</div>` — plain element with only text child is skipped.
    // @ai-generated - Tests plain element with text child is skipped
    #[test]
    fn no_interpolation_children_no_flag() {
        let input = r#"<div>hello</div>"#;

        let close_start = find_pos(input, "</div>");

        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_text(5, 10, false);
        b.close_element(
            Some(make_tag(close_start, close_start + 6, close_start + 5)),
            close_start,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Plain div is skipped
        assert!(
            matches!(result.data[0], OxcNodeData::None),
            "Plain element should be skipped (OxcNodeData::None)"
        );
        // Text child is also None
        assert!(matches!(result.data[1], OxcNodeData::None));
    }

    // ── Test 12: v-else in full traversal ────────────────────────

    /// `<div v-if="show">A</div><div v-else>B</div>`
    /// — v-else element should have condition: None in OXC data.
    // @ai-generated - Tests v-else produces no condition in full traversal
    #[test]
    fn v_else_in_full_traversal() {
        let input = r#"<div v-if="show">A</div><div v-else>B</div>"#;

        let else_div_start = find_pos(input, "<div v-else>");
        let else_tag_end = else_div_start + 12;
        // "B" is at else_tag_end, then "</div>" follows
        let else_close_start = else_tag_end + 1;
        let else_close_end = else_close_start + 6;

        let mut b = TemplateAstBuilder::new(make_root());

        // <div v-if="show">A</div>
        b.open_element(make_tag(0, 18, 4));
        b.set_v_condition(ElementNodeCondition {
            kind: ElementNodeConditionKind::If,
            prop: directive_prop_no_arg(5, 9, Some(11), Some(15)),
        });
        b.mark_element_content_start(18);
        b.add_text(18, 19, false);
        b.close_element(Some(make_tag(19, 25, 24)), 19);

        // <div v-else>B</div>
        b.open_element(make_tag(else_div_start, else_tag_end, else_div_start + 4));
        b.set_v_condition(ElementNodeCondition {
            kind: ElementNodeConditionKind::Else,
            prop: directive_prop_no_arg(else_div_start + 5, else_div_start + 11, None, None),
        });
        b.mark_element_content_start(else_tag_end);
        b.add_text(else_tag_end, else_tag_end + 1, false);
        b.close_element(
            Some(make_tag(
                else_close_start,
                else_close_end,
                else_close_end - 1,
            )),
            else_close_start,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx());

        // Node 0 = div (v-if), Node 1 = text "A", Node 2 = div (v-else), Node 3 = text "B"
        match &result.data[0] {
            OxcNodeData::Element(el) => {
                assert!(el.condition.is_some(), "v-if should have condition");
            }
            other => panic!("expected Element, got {:?}", std::mem::discriminant(other)),
        }

        match &result.data[2] {
            OxcNodeData::Element(el) => {
                assert!(
                    el.condition.is_none(),
                    "v-else should have no condition expression"
                );
            }
            other => panic!("expected Element, got {:?}", std::mem::discriminant(other)),
        }
    }
}
