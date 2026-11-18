use super::test_support::{assert_legacy, render};

#[test]
fn conditional_slot_outlets_keep_each_branch_inside_its_condition() {
    let output = render("<template><slot v-if=\"ok\" name=\"header\"/><slot v-else/></template>");
    assert!(
        output.code.contains("_createIf(() => (_ctx.ok)"),
        "{}",
        output.code
    );
    assert_eq!(
        output.code.matches("_createSlot(").count(),
        2,
        "{}",
        output.code
    );
    assert!(
        output.code.find("_createIf(") < output.code.find("_createSlot("),
        "{}",
        output.code
    );
}

#[test]
fn a_slot_fallback_condition_does_not_consume_the_enclosing_chain() {
    let output = render(
        "<template><slot v-if=\"ok\"/><slot v-else><p v-if=\"fallback\"/></slot></template>",
    );
    assert_eq!(
        output.code.matches("_createSlot(").count(),
        2,
        "{}",
        output.code
    );
    assert_eq!(
        output.code.matches("_createIf(").count(),
        2,
        "{}",
        output.code
    );
}

#[test]
fn a_slot_loop_keeps_its_payload_and_alias_inside_the_callback() {
    for (source, payload, flags) in [
        (
            "<template><slot v-for=\"item in items\" :value=\"item\"/></template>",
            "_createSlot(",
            "16 /* IS_FRAGMENT */",
        ),
        (
            "<template><slot v-if=\"ok\" v-for=\"item in items\" :value=\"item\"/></template>",
            "_createSlot(",
            "16 /* IS_FRAGMENT */",
        ),
        (
            "<template><template v-for=\"item in items\"><p v-if=\"item.ok\"/></template></template>",
            "_createIf(",
            "80 /* IS_FRAGMENT, WRAPPED_ROWS */",
        ),
    ] {
        let output = render(source);
        assert!(output.code.contains("_for_item0.value"), "{}", output.code);
        assert!(
            output.code.contains(&format!(", undefined, {flags})")),
            "{}",
            output.code
        );
        assert!(
            output.code.find("_createFor(") < output.code.find(payload),
            "{}",
            output.code
        );
    }
}

#[test]
fn conditional_loop_opens_and_closes_its_own_alias_frame() {
    let output = render("<template><p v-if=\"waiting\"/><Child v-else v-for=\"item in items\" :value=\"item\"/><p>{{ item }}</p></template>");
    assert!(
        output.code.contains("_createIf(() => (_ctx.waiting)"),
        "{}",
        output.code
    );
    assert!(
        output.code.contains("_createFor(() => (_ctx.items)"),
        "{}",
        output.code
    );
    assert!(output.code.contains("_for_item0.value"), "{}", output.code);
    assert!(
        output.code.contains("_toDisplayString(_ctx.item)"),
        "{}",
        output.code
    );
    assert!(
        output.code.contains(", undefined, 2 /* IS_COMPONENT */)"),
        "{}",
        output.code
    );
}

#[test]
fn loop_defaults_remove_types_before_entering_the_rename_frame() {
    let output = render(
        "<template><p v-for=\"[title = (fallback as string)] in items\">{{ title }}</p></template>",
    );
    assert!(!output.code.contains(" as string"), "{}", output.code);
    assert!(output.code.contains("_getDefaultValue("), "{}", output.code);
}

#[test]
fn generic_loop_sources_use_file_relative_type_ranges() {
    for expression in ["getItems<string>()", "new Set<string>()", "factory<string>"] {
        let source =
            format!("<template><p v-for=\"item in {expression}\">{{{{ item }}}}</p></template>");
        let output = render(&source);
        assert!(!output.code.contains("<string>"), "{}", output.code);
    }
}

#[test]
fn transparent_branches_donate_a_single_structural_child() {
    assert_legacy(
        "<template><template v-if=\"ok\"><p v-for=\"item in items\">{{ item }}</p></template></template>",
    );
    // The conditional's anchor sits where the conditional is, before its
    // later siblings, exactly as Vue's static template places it.
    assert_eq!(
        render("<template><div><p v-if=\"ok\"/><span>tail</span></div></template>").code,
        "const t0 = _template(\"<p>\", 2)\n\
         const t1 = _template(\"<div><!><span>tail\", 1)\n\
         function render(_ctx, $props, $emit, $attrs, $slots) {\n  \
         const n4 = t1()\n  \
         const n3 = _child(n4)\n  \
         _setInsertionState(n4, n3)\n  \
         const n0 = _createIf(() => (_ctx.ok), () => {\n    \
         const n2 = t0()\n    \
         return n2\n  \
         }, null, 33 /* TRUE_SINGLE_ROOT, TRUE_NO_SCOPE */)\n  \
         return n4\n\
         }"
    );
}

#[test]
fn loop_sources_and_keys_remove_types_under_force_javascript() {
    for (source, expected) in [
        (
            "<template><p v-for=\"item in (items as string[])\">{{ item }}</p></template>",
            "_createFor(() => ((_ctx.items))",
        ),
        (
            "<template><p v-for=\"item in items\" :key=\"item!\"/></template>",
            "(item) => (item)",
        ),
    ] {
        let output = render(source);
        assert!(output.code.contains(expected), "{}", output.code);
    }
}

#[test]
fn a_template_loop_row_is_wrapped_unless_it_is_one_plain_element() {
    // Vue's `WRAPPED_ROWS` is independent of the row's returned kind.
    for (content, flags) in [
        ("<p/>", "8 /* IS_SINGLE_NODE */"),
        ("{{ item }}", "72 /* IS_SINGLE_NODE, WRAPPED_ROWS */"),
        ("<!-- c -->", "72 /* IS_SINGLE_NODE, WRAPPED_ROWS */"),
        ("<a/><b/>", "64 /* WRAPPED_ROWS */"),
    ] {
        let output = render(&format!(
            "<template><template v-for=\"item in items\">{content}</template></template>"
        ));
        assert!(
            output.code.contains(&format!(", undefined, {flags})")),
            "{content}
{}",
            output.code
        );
    }
}

#[test]
fn a_destructured_key_parameter_shadows_an_enclosing_alias() {
    let output = render(
        "<template><div v-for=\"item in groups\"><span v-for=\"{ ['id']: item } in rows\" :key=\"item\"/></div></template>",
    );
    assert!(
        output.code.contains("({ ['id']: item }) => (item)"),
        "{}",
        output.code
    );
}

#[test]
fn a_loop_key_does_not_see_its_element_slot_parameter() {
    let output = render(
        "<template><div v-for=\"item in groups\"><Panel v-for=\"row in rows\" v-slot=\"item\" :key=\"item.id\"><i/></Panel></div></template>",
    );
    assert!(
        output.code.contains("(row) => (_for_item0.value.id)"),
        "{}",
        output.code
    );
}
