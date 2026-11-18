use super::test_support::{assert_legacy, assert_legacy_with, render};
use crate::types::BindingType;

#[test]
fn events_preserve_handler_shapes_and_listener_options() {
    for (attribute, expected) in [
        ("@click=\"click\"", "_on(n0, \"click\", e => _ctx.click(e))"),
        (
            "@click.delegate=\"click\"",
            "n0.$evtclick = _createInvoker(e => _ctx.click(e))",
        ),
        (
            "@focus=\"obj.focus\"",
            "_on(n0, \"focus\", e => _ctx.obj.focus(e))",
        ),
        (
            "@click.delegate.once.capture=\"click\"",
            "_on(n0, \"click\", e => _ctx.click(e), { once: true, capture: true })",
        ),
        (
            "@[event].delegate=\"click\"",
            "_on(n0, \"[event]\", e => _ctx.click(e))",
        ),
        (
            "@click.stop=\"click\"",
            "_withModifiers(e => _ctx.click(e), [\"stop\"])",
        ),
        (
            "@keyup.enter=\"key\"",
            "_withKeys(e => _ctx.key(e), [\"enter\"])",
        ),
        ("@click=\"count++;\"", "() => (_ctx.count++)"),
        (
            "@click=\"count++; save()\"",
            "() => { _ctx.count++; _ctx.save() }",
        ),
        ("@click=\"save($event)\"", "$event => (_ctx.save($event))"),
        ("@click=\"\"", "$event => {}"),
    ] {
        let source = format!("<template><button {attribute}/></template>");
        let output = assert_legacy(&source);
        assert!(
            output.code.contains(expected),
            "{attribute}\n{}",
            output.code
        );
    }
}

#[test]
fn constant_handlers_stay_bare_and_delegation_is_deduplicated() {
    let output = assert_legacy_with(
        "<template><button @click=\"click\"/></template>",
        &[("click", BindingType::SetupConst)],
    );
    assert!(output.code.contains("_on(n0, \"click\", _ctx.click)"));
    let output = assert_legacy("<template><button @click.delegate=\"click\"/><button @click.delegate=\"click\"/></template>");
    assert_eq!(output.code.matches("_delegateEvents(\"click\")").count(), 1);
}

#[test]
fn native_models_choose_helpers_and_preserve_modifier_order() {
    for (tag, helper) in [
        ("input", "_applyTextModel"),
        ("textarea", "_applyTextModel"),
        ("select", "_applySelectModel"),
        ("input type=\"checkbox\"", "_applyCheckboxModel"),
        ("input type=\"radio\"", "_applyRadioModel"),
    ] {
        let source = format!("<template><{tag} v-model.trim.number.lazy=\"value\"/></template>");
        let output = assert_legacy(&source);
        assert!(output.code.contains(&format!("{helper}(n0, () => (_ctx.value), _value => (_ctx.value = _value), {{ trim: true, number: true, lazy: true }})")), "{}", output.code);
    }
    // Vue's `genSetTemplateRef`: a static ref outside a loop calls the
    // imported `setStaticTemplateRef`.
    let output = assert_legacy("<template><input ref=\"field\" v-model=\"text\"/></template>");
    assert!(
        output.code.contains("_setStaticTemplateRef(n0, \"field\")"),
        "{}",
        output.code
    );
    assert!(output.helpers.contains(&"_setStaticTemplateRef"));
}

#[test]
fn statement_handlers_remove_types_through_the_shared_owner() {
    let output = render("<template><button @click=\"const n: number = 1; use(n)\"/></template>");
    assert!(
        output.code.contains("() => { const n = 1; _ctx.use(n) }"),
        "{}",
        output.code
    );
}
#[test]
fn statement_handlers_strip_type_declarations_and_nested_control_flow() {
    for handler in [
        "type T = number; use(1)",
        "interface T { value: number }; use(1)",
        "for (let i: number = 0; i < 1; i++) use(i)",
        "function run(value: number): number { return value as number }; run(1)",
        "try { use(1 as number) } catch (error: unknown) { use(error) }",
    ] {
        let source = format!("<template><button @click=\"{handler}\"/></template>");
        super::test_support::render(&source);
    }
}
