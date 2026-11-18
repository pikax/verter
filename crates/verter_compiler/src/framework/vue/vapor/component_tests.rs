use super::test_support::{assert_legacy, with_inputs};
use crate::assembly::emit_plan::MapMode;
use crate::target_plan::TargetExecutor;
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

#[test]
fn component_bindings_use_the_snapshot_candidate_forms() {
    for (tag, binding, expected) in [
        ("some-component", "someComponent", "_ctx.someComponent"),
        ("icons.Button", "icons", "_ctx.icons.Button"),
    ] {
        let source = format!("<template><{tag}/></template>");
        let output = super::test_support::render_with(
            &source,
            &[(binding, crate::types::BindingType::SetupConst)],
        );
        assert!(
            output
                .code
                .contains(&format!("_createComponent({expected},")),
            "{}",
            output.code
        );
    }
}

#[test]
fn typed_slot_parameters_are_javascript_after_emission() {
    let output = super::test_support::render("<template><Outer><template #default=\"props: { title: string }\"><p>{{ props.title }}</p></template></Outer></template>");
    assert!(
        output.code.contains("\"default\": (props) =>"),
        "{}",
        output.code
    );
}

#[test]
fn slot_defaults_remove_types_before_entering_the_rename_frame() {
    let output = super::test_support::render("<template><Outer><template #default=\"{ title = (fallback as string) }\"><p>{{ title }}</p></template></Outer></template>");
    assert!(!output.code.contains(" as string"), "{}", output.code);
    assert!(output.code.contains("_getDefaultValue("), "{}", output.code);
}

#[test]
fn component_static_strings_preserve_literal_control_bytes() {
    assert_legacy("<template><Child title=\"a\tb\"/></template>");
}

#[test]
fn slot_default_type_ranges_use_the_authored_window() {
    for expression in [
        "factory<string>()",
        "(fallback satisfies string)",
        "factory<string>",
        "((value: string): string => value)",
    ] {
        let source = format!("<template><Outer><template #default=\"{{ title = {expression} }}\"><p>{{{{ title }}}}</p></template></Outer></template>");
        let output = super::test_support::render(&source);
        assert!(output.code.contains("_getDefaultValue("), "{}", output.code);
        assert!(!output.code.contains("string"), "{}", output.code);
    }
}

#[test]
fn nested_components_keep_each_statement_in_its_closure() {
    // Vue resolves a component used inside a slot closure once, at the top of
    // `render`, before the components the render block creates directly.
    for source in [
        "<template><Outer><Inner/></Outer></template>",
        "<template><Outer><template #header><Inner/></template></Outer></template>",
    ] {
        let code = super::test_support::render(source).code;
        assert!(
            code.contains(
                "function render(_ctx, $props, $emit, $attrs, $slots) {\n  \
                 const _component_Inner = _resolveComponent(\"Inner\")\n  \
                 const _component_Outer = _resolveComponent(\"Outer\")\n"
            ),
            "{code}"
        );
        assert_eq!(code.matches("_resolveComponent(").count(), 2, "{code}");
    }
    for source in [
        "<template><component :is=\"view\" :title=\"title\"/></template>",
        "<template><component :is/></template>",
    ] {
        assert_legacy(source);
    }
    // A forwarded outlet or dynamic component has no stable root: Vue
    // 3.6.0-rc.15 marks the slot `{ _: 1 /* NON_STABLE */ }`, the flag the
    // runtime's `shouldUseSlotFragment` reads as `slot._ === 1`.
    for source in [
        "<template><Outer><slot/></Outer></template>",
        "<template><Outer><component :is=\"view\"/></Outer></template>",
    ] {
        let code = super::test_support::render(source).code;
        assert!(code.contains("}, { _: 1 /* NON_STABLE */ })"), "{code}");
    }
    // A slot body made only of a construct has no container: Vue 3.6.0-rc.15
    // returns the construct itself (`const n0 = _createIf(...)`, `return n0`),
    // where an empty template would be no node at all.
    for source in [
        "<template><Outer><template v-if=\"ok\"><slot/></template></Outer></template>",
        "<template><Outer><p v-for=\"item in items\">{{ item }}</p></Outer></template>",
    ] {
        let code = super::test_support::render(source).code;
        assert!(!code.contains("_template(\"\")"), "{code}");
        assert!(!code.contains("_setInsertionState("), "{code}");
        assert!(code.contains("      return n0\n"), "{code}");
        // No root is stable, so the slot is NON_STABLE.
        assert!(code.contains("_extend(() => {"), "{code}");
    }
}

#[test]
fn a_slot_fallback_reserves_its_entry_id_before_its_children() {
    let output = assert_legacy("<template><slot name=\"header\"><p>fallback</p></slot></template>");
    assert!(
        output
            .code
            .contains("const n0 = _createSlot(\"header\", null, () => {\n    const n2 = t0()"),
        "{}",
        output.code
    );
}

#[test]
fn the_slot_name_quote_maps_to_the_attribute_name() {
    let source = "<template><slot name=\"header\"/></template>";
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
            let quote = written.text().find("\"header\"").unwrap() as u32;
            let attribute = source.find("name=").unwrap() as u32;
            assert_eq!(written.map().unwrap().lookup(quote), Some((0, attribute)));
            assert!(written
                .map()
                .unwrap()
                .segments()
                .iter()
                .any(|segment| segment.generated.contains(&quote)
                    && segment.kind == crate::emission::MapSegmentKind::Anchor));
        },
    );
}

#[test]
fn component_event_references_and_inline_handlers_follow_their_parsed_root() {
    let code = super::test_support::render(
        "<template><Panel @x=\"handlers[current]\" @y=\"a?.b\" @z=\"handler = () => count++\"/></template>",
    )
    .code;
    // Computed and optional members are references the getter returns as
    // read; an assignment whose value is an arrow is an inline handler that
    // runs only when the event fires.
    for expected in [
        "onX: () => (_ctx.handlers[_ctx.current])",
        "onY: () => (_ctx.a?.b)",
        "const _on_z = () => (_ctx.handler = () => _ctx.count++)",
        "onZ: () => _on_z",
    ] {
        assert!(
            code.contains(expected),
            "{expected}
{code}"
        );
    }
    assert!(!code.contains("const _on_x"), "{code}");
}

#[test]
fn a_hoisted_handler_never_shadows_a_slot_parameter() {
    let code = super::test_support::render(
        "<template><Outer><template #default=\"_on_x\"><Panel @x=\"use(_on_x)\"/></template></Outer></template>",
    )
    .code;
    assert!(
        code.contains("const _on_x1 = () => (_ctx.use(_on_x))"),
        "{code}"
    );
    assert!(code.contains("onX: () => _on_x1"), "{code}");
}

#[test]
fn distinct_component_tags_keep_distinct_assets() {
    let code =
        super::test_support::render("<template><Outer><Foo.Bar/></Outer><Foo-Bar/></template>")
            .code;
    assert!(
        code.contains("const _component_Foo_Bar = _resolveComponent(\"Foo.Bar\")"),
        "{code}"
    );
    assert!(
        code.contains("const _component_Foo_Bar1 = _resolveComponent(\"Foo-Bar\")"),
        "{code}"
    );
    assert!(
        code.contains("_createComponent(_component_Foo_Bar1"),
        "{code}"
    );
    assert_eq!(code.matches("_resolveComponent(").count(), 3, "{code}");
}

#[test]
fn a_handler_value_is_a_reference_or_a_function_only_by_its_parsed_root() {
    let code = super::test_support::render(
        "<template><Panel @x=\"undefined\" @y=\"(function () { save() })\"/><div @click=\"undefined\"/></template>",
    )
    .code;
    // `undefined` is never a handler reference, and a parenthesized function
    // is still the handler itself.
    for expected in [
        "const _on_x = () => (undefined)",
        "const _on_y = (function () { _ctx.save() })",
        "onY: () => _on_y",
    ] {
        assert!(code.contains(expected), "{expected}\n{code}");
    }
    assert!(!code.contains("undefined(e)"), "{code}");
}

#[test]
fn a_hoisted_handler_never_shadows_the_component_loop_alias() {
    let code = super::test_support::render(
        "<template><Outer><template #default=\"_on_x\"><Panel v-for=\"{ ['id']: _on_x1 } in rows\" @x=\"use(_on_x1)\"/></template></Outer></template>",
    )
    .code;
    assert!(code.contains("const _on_x2 = () => (_ctx.use("), "{code}");
    assert!(code.contains("onX: () => _on_x2"), "{code}");
    assert!(!code.contains("const _on_x1"), "{code}");
}

#[test]
fn a_component_model_passes_its_prop_listener_and_modifiers() {
    // Vue 3.6.0-rc.15: `title: () => (_ctx.title)`, `"onUpdate:title": () =>
    // _on_update_title`, `titleModifiers: { trim: true }`, and the default
    // model as `modelValue` with `modelModifiers`.
    let code = super::test_support::render(
        "<template><Field v-model:title.trim=\"title\" v-model.number=\"name\"/></template>",
    )
    .code;
    for expected in [
        "const _on_update_title = _value => (_ctx.title = _value)",
        "const _on_update_modelValue = _value => (_ctx.name = _value)",
        "title: () => (_ctx.title), \"onUpdate:title\": () => _on_update_title, \
         titleModifiers: { trim: true }",
        "modelValue: () => (_ctx.name), \"onUpdate:modelValue\": () => _on_update_modelValue, \
         modelModifiers: { number: true }",
    ] {
        assert!(code.contains(expected), "{expected}\n{code}");
    }
}

#[test]
fn conditional_slot_templates_are_one_dynamic_slot_entry() {
    // Vue 3.6.0-rc.15: `$: [() => (_ctx.ok ? { name: "a", fn: s || (s = ...) }
    // : { name: "b", fn: s1 || (s1 = ...) })]`.
    let code = super::test_support::render(
        "<template><Table><template v-if=\"ok\" #a>x</template>\
         <template v-else #b>y</template></Table></template>",
    )
    .code;
    for expected in [
        "let _slot0\n",
        "let _slot1\n",
        "$: [() => ((_ctx.ok) ? { name: \"a\", fn: _slot0 || (_slot0 = () => {",
        " : { name: \"b\", fn: _slot1 || (_slot1 = () => {",
    ] {
        assert!(code.contains(expected), "{expected}\n{code}");
    }
    assert!(!code.contains("void 0"), "{code}");
}

#[test]
fn a_component_model_with_a_dynamic_argument_is_refused() {
    // The model would otherwise be dropped: the dynamic argument needs Vue's
    // dynamic props list, which this target does not write for models.
    with_inputs(
        "<template><Field v-model:[prop]=\"value\"/></template>",
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            assert!(matches!(
                super::VueVaporExecutor::execute(inputs, ledger),
                Err(
                    crate::framework::vue::target::VueTargetRefusal::Unsupported {
                        code: "dynamic-component-model-argument",
                        ..
                    }
                )
            ));
        },
    );
}
