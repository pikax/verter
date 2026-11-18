use super::test_support::{assert_legacy, with_inputs};
use crate::assembly::emit_plan::MapMode;
use crate::target_plan::TargetExecutor;
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

#[test]
fn member_reads_are_recorded_at_binding_positions() {
    let output = assert_legacy("<template><p>{{ count > 1 ? a.b : count }}</p></template>");
    assert!(output.code.contains("const _count = _ctx.count"));
    assert!(output
        .code
        .contains("_toDisplayString(_count > 1 ? _ctx.a.b : _count)"));
}

#[test]
fn call_targets_preserve_their_receivers() {
    let output = assert_legacy("<template><div :title=\"fn() + fn()\"/></template>");
    assert!(output.code.contains("_ctx.fn() + _ctx.fn()"));
    assert!(!output.code.contains("const _fn"));
}

#[test]
fn quoted_reads_do_not_count_as_occurrences() {
    let output = assert_legacy("<template><p>{{ '_ctx.count' + count }}</p></template>");
    assert!(output.code.contains("'_ctx.count' + _ctx.count"));
    assert!(!output.code.contains("const _count"));
}

#[test]
fn longer_binding_names_have_distinct_read_identities() {
    let output = assert_legacy("<template><p>{{ x_ctx.count + count }}</p></template>");
    assert!(output.code.contains("_ctx.x_ctx.count + _ctx.count"));
    assert!(!output.code.contains("const _count"));
}

#[test]
fn a_single_read_keeps_a_concise_effect() {
    let output = assert_legacy("<template><p>{{ count }}</p></template>");
    assert!(output
        .code
        .contains("_renderEffect(() => _setText(x0, _toDisplayString(_ctx.count)))"));
}

#[test]
fn repeated_reads_across_text_parts_share_one_declaration() {
    let output =
        assert_legacy("<template><p>{{ count }} / {{ count > 1 ? 'many' : 'one' }}</p></template>");
    assert!(output.code.contains("const _count = _ctx.count"));
    assert!(output.code.contains("_setText(x0, _toDisplayString(_count) + \" / \" + _toDisplayString(_count > 1 ? 'many' : 'one'))"));
}

#[test]
fn different_effect_kinds_share_one_read_declaration() {
    let output =
        assert_legacy("<template><div :title=\"label\" :aria-label=\"label\"/></template>");
    assert!(output.code.contains("const _label = _ctx.label"));
    assert!(output.code.contains("_setProp(n0, \"title\", _label)"));
    assert!(output.code.contains("_setAttr(n0, \"aria-label\", _label)"));
}

#[test]
fn a_hoisted_identifier_maps_to_its_first_occurrence() {
    let source = "<template><p>{{ count }} / {{ count + 1 }}</p></template>";
    with_inputs(
        source,
        MapMode::Map,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let output = super::VueVaporExecutor::execute(inputs, ledger).unwrap();
            let written = output.render.write(ledger).unwrap();
            super::test_support::assert_javascript(written.text());
            let generated = written.text().find("_ctx.count").unwrap() as u32 + 5;
            assert_eq!(
                written.map().unwrap().lookup(generated),
                Some((0, source.find("count").unwrap() as u32))
            );
            assert_eq!(
                written.map().unwrap().lookup(generated + 4),
                Some((0, source.find("count").unwrap() as u32 + 4))
            );
        },
    );
}

#[test]
fn template_literals_and_erased_type_syntax_keep_read_boundaries() {
    let template = assert_legacy("<template><p>{{ `${count}` + count }}</p></template>");
    assert!(!template.code.contains("const _count"));
    let calls = assert_legacy("<template><div :title=\"fn!() + fn!()\"/></template>");
    assert!(!calls.code.contains("const _fn"));
    let members = assert_legacy("<template><div :title=\"obj!.value + obj!.value\"/></template>");
    assert!(
        members.code.contains("const _obj_value = _ctx.obj.value"),
        "{}",
        members.code
    );
}

// Reads optional audit capture: runs in the instrumentation lane.
#[cfg(feature = "semantic-observe")]
#[test]
fn graph_size_is_recorded_and_linear() {
    use crate::work_ledger::WorkKind;
    let counts = |size: usize| {
        let mut source = String::from("<template><div");
        for index in 0..size {
            source.push_str(&format!(" :title{index}=\"shared + shared\""));
        }
        source.push_str("/></template>");
        let mut ledger = CompilerWorkLedger::<verter_audit::observe::Captured>::new();
        with_inputs(
            &source,
            MapMode::NoMap,
            false,
            &[],
            &mut ledger,
            |inputs, ledger| {
                let output = super::VueVaporExecutor::execute(inputs, ledger).unwrap();
                let written = output.render.write(ledger).unwrap();
                super::test_support::assert_javascript(written.text());
                assert_eq!(
                    written
                        .text()
                        .matches("const _shared = _ctx.shared")
                        .count(),
                    1
                );
            },
        );
        let snapshot = ledger.snapshot().expect("capture is compiled in");
        (
            snapshot.count(WorkKind::PlanningEffectNodes),
            snapshot.count(WorkKind::PlanningEffectEdges),
        )
    };
    assert_eq!(counts(8), (10, 16));
    assert_eq!(counts(16), (18, 32));
}
