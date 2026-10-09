use super::*;

#[test]
fn ref_dynamic_binding_converts_to_jsx_expression() {
    let result =
        gen_tsx_template(r#"<template><div :ref="el => (myRef = el)">content</div></template>"#);
    assert!(
        result.contains("ref={"),
        "dynamic :ref should become ref={{expr}}, got: {}",
        result
    );
    // The :ref prefix must be removed
    assert!(
        !result.contains(":ref"),
        ":ref prefix must not appear in output, got: {}",
        result
    );
}

#[test]
fn tsx_known_setup_binding_stays_bare() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        !result.contains("___VERTER___instance.count"),
        "Known binding should NOT get instance prefix. Got: {}",
        result
    );
}

// ── Data/Options binding instance prefix in TSX mode ─────────────

#[test]
fn data_binding_uses_instance_prefix() {
    let source = r#"<template><div>{{ count }}</div></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("count", BindingType::Data)]);

    // Positive: Data bindings should use ___VERTER___instance. prefix
    assert!(
        output.contains("___VERTER___instance.count"),
        "Data binding should use instance prefix in TSX mode: {output}"
    );
    // Negative: should NOT contain bare `{count}` without instance prefix
    assert!(
        !output.contains("{count}"),
        "Data binding should not be bare — must use instance prefix: {output}"
    );
}

#[test]
fn options_binding_uses_instance_prefix() {
    let source = r#"<template><div>{{ total }}</div></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("total", BindingType::Options)]);

    // Positive: Options bindings should use ___VERTER___instance. prefix
    assert!(
        output.contains("___VERTER___instance.total"),
        "Options binding should use instance prefix in TSX mode: {output}"
    );
}

/// Q1 — an IGNORED local binding (a v-slot scoped local) used in a RELOCATED value
/// (here a v-on object handler) must emit the MAPPED bare identifier — no accessor
/// prefix/suffix, but a source-map token so ctrl+click lands on the local. The
/// relocated-value path must map ignored locals, not emit them unmapped.
#[test]
fn relocated_value_ignored_local_binding_maps() {
    // `item` is a v-slot scoped local; inside `v-on="{ click: item }"` it is an
    // ignored binding. It stays bare (no prefix) AND maps back to its source span.
    let source =
        r#"<template><Comp v-slot="{ item }"><div v-on="{ click: item }"/></Comp></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // The handler value `item` is a scoped local → bare (no `__props.` / `_ctx.`).
    assert!(
        output.contains("onClick: item"),
        "ignored local handler must stay bare `onClick: item`: {output}"
    );
    assert!(
        !output.contains("onClick: _ctx.item") && !output.contains("onClick: __props.item"),
        "ignored local must NOT be prefixed: {output}"
    );

    // The `item` INSIDE the v-on object must map to its source span. Two `item`
    // tokens exist (the v-slot destructure + the handler); assert a token maps to
    // the handler occurrence specifically.
    let handler_item_src = source.find("click: item").unwrap() as u32 + "click: ".len() as u32;
    assert!(
        has_token_for_src(&tokens, handler_item_src),
        "ignored local handler `item` must map to source col {handler_item_src} (emitted MAPPED, not unmapped). Tokens: {tokens:?}, output: {output}"
    );
}
