use super::*;

mod parse_expression_tests {
    use super::*;
    use crate::utils::oxc::{is_global, is_keyword};
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
        let result = parse_expression(Span::new(0, 0), "", &alloc, tsx(), &[], false);
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
        let result = parse_expression(Span::new(0, 3), input, &alloc, tsx(), &[], false);

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
        let result = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            tsx(),
            &[],
            false,
        );

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
        let result = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            tsx(),
            &[],
            false,
        );

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
        let result = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            tsx(),
            &[],
            false,
        );

        assert!(result.expression.is_some());
        let bindings = result.bindings.as_ref().unwrap();
        // `true` is parsed as BooleanLiteral by OXC, not an identifier.
        // So it should not appear in bindings.bindings at all.
        let non_keyword_bindings: Vec<_> = bindings
            .bindings
            .iter()
            .filter(|b| !is_keyword(b.name.as_bytes()) && !is_global(b.name.as_bytes()))
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
        let result = parse_expression(Span::new(0, 4), input, &alloc, tsx(), &["item"], false);

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
            false,
        );

        assert!(result.expression.is_some());
        assert_eq!(result.dynamism, Dynamism::Dynamic);
    }

    /// Test 8: Binary expression of literals is Static.
    #[test]
    fn binary_literals_static() {
        let alloc = Allocator::default();
        let input = "1 + 2";
        let result = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            tsx(),
            &[],
            false,
        );

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
        let result = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            tsx(),
            &[],
            false,
        );

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
            false,
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
        let result = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            tsx(),
            &[],
            false,
        );

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
        let result = parse_expression(Span::new(7, 10), input, &alloc, tsx(), &[], false);

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

mod parse_element_tests {
    use super::*;
    use crate::ast::types::*;
    use crate::types::{NodeProp, NodeTag};
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
            is_fully_static: false,
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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

        assert!(result.condition.is_none());
        assert!(result.v_for.is_none());
        assert!(result.v_slot.is_none());
        assert!(
            result.props.is_empty(),
            "Plain attributes need no OXC parsing"
        );
        assert!(result.expression_flag.is_empty());
        assert!(result.provided_locals.is_none());
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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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

    // ── Dense prop lookup invariant ─────────────────────────────

    /// The dense `prop_lookup` table spans the FULL `ElementNode.props` (not the
    /// sparse parsed `props`): static attributes leave `None` gaps at their exact
    /// positions, directives with expressions point at their slot, and
    /// `OxcParsedElement::prop` resolves each index to the prop whose `prop_index`
    /// matches — with byte-exact expression offsets.
    #[test]
    fn dense_prop_lookup_spans_element_props_with_none_gaps() {
        //  <div a="1" :b="v" c="2" :d="w">
        //  0    5     11    18    24
        //  a value "1" @8, :b value "v" @15, c value "2" @21, :d value "w" @28
        let input = r#"<div a="1" :b="v" c="2" :d="w">"#;
        let el = make_element(
            TagType::Element,
            vec![
                plain_attr(5, 6, Some(8), Some(9)),
                directive_prop(11, 13, Some(12), Some(13), Some(15), Some(16)),
                plain_attr(18, 19, Some(21), Some(22)),
                directive_prop(24, 26, Some(25), Some(26), Some(28), Some(29)),
            ],
            None,
            None,
            None,
            None,
            PropFlag::empty(),
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

        // Dense table length matches the FULL element props, not the sparse parsed set.
        assert_eq!(
            result.prop_lookup.len(),
            el.props.len(),
            "prop_lookup must be dense over ElementNode.props"
        );
        assert_eq!(result.props.len(), 2, "only the two directives are parsed");
        // None gaps at exactly the static-attr positions; Some at the directives.
        assert_eq!(result.prop_lookup, vec![None, Some(0), None, Some(1)]);

        // Static attrs resolve to nothing through the index.
        assert!(result.prop(0).is_none(), "static attr → no parsed prop");
        assert!(result.prop(2).is_none(), "static attr → no parsed prop");
        // Out-of-range indices are safe and resolve to None.
        assert!(result.prop(99).is_none());

        // Directive props resolve to the slot whose prop_index matches, with the
        // expression offset landing byte-exactly on the directive value.
        let b = result.prop(1).expect(":b should resolve");
        assert_eq!(b.prop_index, 1);
        let b_exp = b.exp.as_ref().expect(":b has a parsed expression");
        assert_eq!(b_exp.offset, 15, ":b value 'v' begins at byte 15");

        let d = result.prop(3).expect(":d should resolve");
        assert_eq!(d.prop_index, 3);
        let d_exp = d.exp.as_ref().expect(":d has a parsed expression");
        assert_eq!(d_exp.offset, 28, ":d value 'w' begins at byte 28");
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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

        assert!(result.v_for.is_some(), "should have parsed v-for");
        let locals = result
            .provided_locals
            .as_ref()
            .expect("v-for should produce provided_locals");
        assert!(
            locals.contains(&"item"),
            "provided_locals should contain 'item', got: {:?}",
            locals
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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

        assert!(result.v_slot.is_some(), "should have parsed v-slot");
        let locals = result
            .provided_locals
            .as_ref()
            .expect("v-slot should produce provided_locals");
        assert!(
            locals.contains(&"data"),
            "provided_locals should contain 'data', got: {:?}",
            locals
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
                .add(PropFlags::HasDynamicBinding), // :id is a generic dynamic binding
        );
        let alloc = Allocator::default();
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

        assert!(
            result.v_for.is_some(),
            "v-once should not suppress v-for parsing"
        );
        let locals = result
            .provided_locals
            .as_ref()
            .expect("v-for should produce provided_locals");
        assert!(
            locals.contains(&"item"),
            "v-for locals should still be provided, got: {:?}",
            locals
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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &[], input, &alloc, tsx(), false);

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
        let result = parse_element(&el, &["item"], input, &alloc, tsx(), false);

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

mod parse_template_expressions_tests {
    use super::*;
    use crate::ast::builder::TemplateAstBuilder;
    use crate::ast::types::*;
    use crate::test_helpers::{make_root, make_tag};
    use crate::types::NodeProp;
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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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

    /// Build a static (non-directive) attribute prop.
    fn static_attr(
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

    /// Structural directives (v-for) and `ref` are removed into cached element
    /// fields by the parser BEFORE ordinary props, so they never occupy a
    /// `prop_index`. The dense lookup is built over `ElementNode.props` only, so
    /// the remaining static/dynamic props still correlate to the correct indices
    /// even when a v-for and a ref are present on the same element.
    #[test]
    fn prop_index_correlates_through_structural_directive_and_ref() {
        //  <div v-for="x in xs" ref="r" a="1" :b="v"></div>
        let input = r#"<div v-for="x in xs" ref="r" a="1" :b="v"></div>"#;
        let v_for_val = (find_pos(input, "x in xs"), find_pos(input, "x in xs") + 7);
        let a_val = (find_pos(input, r#""1""#) + 1, find_pos(input, r#""1""#) + 2);
        let b_val = (find_pos(input, r#""v""#) + 1, find_pos(input, r#""v""#) + 2);

        let mut b = TemplateAstBuilder::new(make_root());
        let open_end = find_pos(input, ">") + 1;
        b.open_element(make_tag(0, open_end, 4));
        // v-for and ref go into cached fields, NOT into props.
        b.set_v_for(directive_prop_no_arg(
            5,
            10,
            Some(v_for_val.0),
            Some(v_for_val.1),
        ));
        b.set_v_ref(static_attr(21, 24, Some(26), Some(27)));
        // Ordinary props, in source order → prop_index 0 (static), 1 (dynamic).
        b.push_prop_to_current(static_attr(29, 30, Some(a_val.0), Some(a_val.1)));
        b.push_prop_to_current(directive_prop(
            35,
            37,
            Some(36),
            Some(37),
            Some(b_val.0),
            Some(b_val.1),
        ));
        let close_start = find_pos(input, "</div>");
        b.mark_element_content_start(open_end);
        b.close_element(
            Some(make_tag(close_start, close_start + 6, close_start + 5)),
            open_end,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

        let OxcNodeData::Element(el) = &result.data[0] else {
            panic!("expected Element at node 0");
        };

        // Two ordinary props survive in ElementNode.props (v-for / ref excluded).
        assert_eq!(el.prop_lookup.len(), 2);
        assert_eq!(el.prop_lookup, vec![None, Some(0)]);

        // The static attr resolves to nothing; the directive resolves to its slot
        // with a byte-exact expression offset for the value "v".
        assert!(el.prop(0).is_none(), "static attr a → None");
        let dir = el.prop(1).expect(":b should resolve via the index");
        assert_eq!(dir.prop_index, 1);
        let exp = dir.exp.as_ref().expect(":b has a parsed expression");
        assert_eq!(exp.offset, b_val.0, ":b value offset must be byte-exact");
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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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

        // Template element should have both locals (it has v-slot → Some)
        match &result.data[1] {
            OxcNodeData::Element(el) => {
                let locals = el
                    .provided_locals
                    .as_ref()
                    .expect("v-slot element should have provided_locals");
                assert!(
                    locals.contains(&"item"),
                    "template should inherit 'item' from parent v-for, got: {:?}",
                    locals
                );
                assert!(
                    locals.contains(&"data"),
                    "template should have 'data' from v-slot, got: {:?}",
                    locals
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
        b.add_text(0, 6, false, false);
        b.add_comment(6, 22, 11, 18);
        let ast = b.finish();

        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
        b.add_text(5, 10, false, false);
        b.close_element(
            Some(make_tag(close_start, close_start + 6, close_start + 5)),
            close_start,
        );

        let ast = b.finish();
        let alloc = Allocator::default();
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
        b.add_text(18, 19, false, false);
        b.close_element(Some(make_tag(19, 25, 24)), 19);

        // <div v-else>B</div>
        b.open_element(make_tag(else_div_start, else_tag_end, else_div_start + 4));
        b.set_v_condition(ElementNodeCondition {
            kind: ElementNodeConditionKind::Else,
            prop: directive_prop_no_arg(else_div_start + 5, else_div_start + 11, None, None),
        });
        b.mark_element_content_start(else_tag_end);
        b.add_text(else_tag_end, else_tag_end + 1, false, false);
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
        let result = parse_template_expressions(&ast, input, &alloc, tsx(), false);

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
