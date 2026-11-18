use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::framework::vue::semantic::{VueBindingTable, VueBindingView};
use crate::template::binding::prefixed_expr::{write_prefixed_expr, StringSink};
use crate::template::oxc::parse_template_expressions;

/// Exercise the expression producer and rewriter the fine-grained target
/// consumes, including the no-reference path through TypeScript removal.
fn assert_javascript_expression(authored: &str, expected: &str) {
    let source = format!("<template><p>{{{{{authored}}}}}</p></template>");
    let ast = crate::framework_common::vue_bridge::parse_sfc(&source, None, None)
        .template_ast
        .expect("template block");
    let allocator = Allocator::default();
    let expressions =
        parse_template_expressions(&ast, &source, &allocator, SourceType::tsx(), false);
    let mut expressions = expressions.iter_expressions();
    let expression = expressions.next().expect("interpolation expression");
    assert!(expressions.next().is_none());
    assert!(expression.errors.is_none(), "{:?}", expression.errors);
    let removals = crate::strip_types::typescript::collect_ts_removal_spans(
        expression.expression.as_ref().expect("parsed expression"),
    );
    assert!(!removals.is_empty(), "fixture must exercise type removal");
    let bindings = VueBindingTable::default();
    let view = VueBindingView::new(&bindings, false, true, None, Default::default());
    let mut output = String::new();
    write_prefixed_expr(
        &mut StringSink(&mut output),
        authored,
        expression.offset,
        expression,
        &view,
        &removals,
    );
    let javascript =
        verter_parser::oxc_parse::Parser::new(&allocator, &output, SourceType::mjs()).parse();
    assert!(
        javascript.diagnostics.is_empty(),
        "type removal must produce JavaScript: {output:?}; {:?}",
        javascript.diagnostics
    );
    assert_eq!(output, expected);
}

#[test]
fn type_removal_preserves_a_prefixed_value() {
    assert_javascript_expression("value as number", "_ctx.value");
}

#[test]
fn type_removal_accepts_an_expression_without_bindings() {
    assert_javascript_expression("(1 as number)", "(1)");
}

#[test]
fn type_removal_accepts_a_statement_handler() {
    use super::expr::{record_expr, VaporBindings};
    use super::ops::Part;
    use super::test_support::with_inputs;
    use crate::assembly::emit_plan::MapMode;
    use crate::framework::vue::structure::VueAttrKind;
    use crate::template::binding::prefixed_expr::find_prop_oxc_exp;
    use crate::work_ledger::CompilerWorkLedger;
    use verter_audit::observe::Uncaptured;

    let source = "<template><button @click=\"const n: number = 1; use(n)\"/></template>";
    with_inputs(
        source,
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, _| {
            let root = inputs.structure.compile_id(inputs.topology.root());
            let button = inputs.structure.children(root)[0].node;
            let event = inputs
                .structure
                .attrs(button)
                .iter()
                .find(|attr| attr.kind == VueAttrKind::On)
                .expect("event attribute");
            let value = event.value_span.expect("handler value");
            let parsed = find_prop_oxc_exp(
                inputs
                    .semantic
                    .element_exprs(inputs.structure.template_id(button)),
                event.prop_index as usize,
            )
            .expect("parsed handler");
            assert!(parsed.errors.is_none(), "{:?}", parsed.errors);
            assert!(parsed.expression.is_none());
            assert!(parsed.multi_statement);
            assert_eq!(
                parsed
                    .statements
                    .expect("retained handler program")
                    .body
                    .len(),
                2
            );

            let view = VueBindingView::new(
                inputs.semantic.bindings(),
                false,
                true,
                None,
                Default::default(),
            );
            let mut parts = Vec::new();
            record_expr(
                &mut parts,
                inputs.allocator,
                &source[value.start as usize..value.end as usize],
                value.start,
                super::expr::Syntax::Parsed(Some(parsed)),
                &VaporBindings {
                    view: &view,
                    renames: &[],
                },
                true,
            );
            let mut rewritten = String::new();
            for part in parts {
                match part {
                    Part::Text(text) => rewritten.push_str(text),
                    Part::Source(span) => {
                        rewritten.push_str(&source[span.start as usize..span.end as usize]);
                    }
                    other => panic!("expression recording contains an operation: {other:?}"),
                }
            }
            let handler = format!("const handler = () => {{ {rewritten} }};");
            let javascript = verter_parser::oxc_parse::Parser::new(
                inputs.allocator,
                &handler,
                SourceType::mjs(),
            )
            .parse();
            assert!(
                javascript.diagnostics.is_empty(),
                "type removal must produce a JavaScript handler: {handler:?}; {:?}",
                javascript.diagnostics
            );
            assert_eq!(rewritten, "const n = 1; _ctx.use(n)");
        },
    );
}
