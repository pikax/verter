use super::test_support::{legacy, render, with_inputs};
use crate::assembly::emit_plan::MapMode;
use crate::framework::vue::structure::VueChildKind;
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

#[test]
fn rendered_children_condense_a_leading_space_in_static_markup() {
    let source = "<template><div> <span/>tail</div></template>";
    let previous = legacy(source);
    assert!(
        previous.contains("_template(\"<div> <span>tail\""),
        "{previous}"
    );
    let corrected = render(source);
    assert!(
        corrected.code.contains("_template(\"<div><span>tail\", 3)"),
        "{}",
        corrected.code
    );
    with_inputs(
        source,
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, _| {
            let root = inputs.structure.compile_id(inputs.topology.root());
            let div = inputs.structure.children(root)[0].node;
            let kinds: Vec<_> = inputs
                .structure
                .children(div)
                .iter()
                .map(|child| child.kind)
                .collect();
            assert_eq!(kinds, [VueChildKind::Element, VueChildKind::Text]);
        },
    );
}
