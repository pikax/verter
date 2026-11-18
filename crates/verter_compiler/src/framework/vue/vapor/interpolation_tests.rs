use super::{effect::TextPart, ops::Part, test_support::with_inputs, VaporCompiler};
use crate::assembly::emit_plan::MapMode;
use crate::ast::types::AstNodeKind;
use crate::template::code_gen::shared::helpers::VaporHelper;
use crate::template::topology::VueTemplateNodeKind;
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

fn with_interpolation(run: impl for<'c> FnOnce(VaporCompiler<'c>)) {
    with_inputs(
        "<template><p>{{ msg }}</p></template>",
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, _| {
            let template = inputs
                .topology
                .nodes()
                .find(|node| inputs.topology.kind(*node) == VueTemplateNodeKind::Interpolation)
                .unwrap();
            let syntax = inputs.topology.syntax(template).unwrap();
            let AstNodeKind::Interpolation(interp) = &inputs.topology.ast().nodes[syntax.0].kind
            else {
                panic!("interpolation fixture")
            };
            let mut compiler = VaporCompiler::new(inputs);
            compiler.element_stack.push(Default::default());
            super::interpolation::process_interpolation(
                &mut compiler,
                interp,
                inputs.semantic.interpolation(template).unwrap(),
            );
            run(compiler);
        },
    );
}
#[test]
fn interpolation_records_dynamic_part() {
    with_interpolation(|compiler| {
        let scope = compiler.element_stack.last().unwrap();
        let [TextPart::Dynamic(run)] = scope.text_parts.as_slice() else {
            panic!("dynamic text row")
        };
        let parts = compiler.state.runs.get(*run);
        assert_eq!(parts.first(), Some(&Part::Text("_toDisplayString(")));
        let Part::Run(expression) = parts[1] else {
            panic!("expression run")
        };
        assert!(compiler
            .state
            .runs
            .get(expression)
            .contains(&Part::Text("_ctx.")));
        assert_eq!(parts.last(), Some(&Part::Text(")")));
    });
}
#[test]
fn interpolation_allocates_text_ref() {
    with_interpolation(|mut compiler| {
        let scope = compiler.element_stack.last().unwrap();
        assert_eq!(scope.text_node_ref, Some(0));
        assert_eq!(scope.node_ref, Some(0));
        assert_eq!(compiler.state.counters.next_node(), 1);
    });
}
#[test]
fn interpolation_adds_import() {
    with_interpolation(|compiler| {
        assert!(compiler.state.helpers().has(VaporHelper::ToDisplayString))
    });
}
#[test]
fn compound_expression_without_binding_data_keeps_its_source() {
    let allocator = oxc_allocator::Allocator::default();
    let table = crate::framework::vue::semantic::VueBindingTable::default();
    let view = crate::framework::vue::semantic::VueBindingView::new(
        &table,
        false,
        true,
        None,
        Default::default(),
    );
    let mut parts = Vec::new();
    super::expr::record_expr(
        &mut parts,
        &allocator,
        "a + b",
        0,
        super::expr::Syntax::Parsed(None),
        &super::expr::VaporBindings {
            view: &view,
            renames: &[],
        },
        true,
    );
    assert_eq!(parts, [Part::Source(verter_span::Span::new(0, 5))]);
}
