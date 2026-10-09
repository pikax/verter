use super::*;

#[test]
fn interpolation_basic() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ msg }}</div></template>",
        &[("msg", BindingType::SetupRef)],
    );
    assert!(
        result.contains("{ msg }"),
        "{{ msg }} should become bare identifier in TSX mode, got: {}",
        result
    );
}

#[test]
fn interpolation_expression() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ a + b }}</div></template>",
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    assert!(result.contains("{ a + b }"), "got: {}", result);
}

#[test]
fn interpolation_partial_known_binding_stays_bare_for_completion() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ cou }}</div></template>",
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        result.contains("{ cou }") || result.contains("{cou}"),
        "partial binding should stay bare for completion context, got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.cou"),
        "partial binding must not get instance prefix, got: {}",
        result
    );
}

// ── Interpolation with bindings ────────────────────────────

#[test]
fn interpolation_with_setup_ref() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ count }}</div></template>",
        &[("count", BindingType::SetupRef)],
    );
    // In TSX mode, SetupRef gets no prefix and no .value suffix (block scope handles unwrapping)
    assert!(
        result.contains("{ count }") && !result.contains("count.value"),
        "SetupRef should be bare identifier in TSX mode (no .value), got: {}",
        result
    );
}

#[test]
fn interpolation_with_setup_const() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ msg }}</div></template>",
        &[("msg", BindingType::SetupConst)],
    );
    // SetupConst in inline mode: no prefix, no suffix
    assert!(
        result.contains("{ msg }"),
        "SetupConst should have no prefix/suffix, got: {}",
        result
    );
}

#[test]
fn interpolation_with_props() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ title }}</div></template>",
        &[("title", BindingType::Props)],
    );
    // Props in inline mode: __props. prefix
    assert!(
        result.contains("__props.title"),
        "Props should get __props. prefix, got: {}",
        result
    );
}

// ── v-memo dependency mapping (F23) ───

/// F23: the `v-memo="[deps]"` dependency expression must be mapped INTO the IDE
/// surface so its identifiers type-check / hover / navigate, rather than being
/// dropped. Emitted as `data-v-memo={[deps]}` with each identifier 1:1 mapped.
#[test]
fn v_memo_dependency_expression_is_mapped_into_ide_surface() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-memo="[count]">{{ count }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    // The dependency expression reaches the surface (identifiers navigable).
    assert!(
        result.contains("data-v-memo={[") && result.contains("count"),
        "v-memo dep must be emitted into the IDE surface, got:\n{}",
        result
    );
    // NEGATIVE: the raw v-memo directive must not survive verbatim.
    assert!(
        !result.contains("v-memo=\"[count]\""),
        "raw v-memo directive must not survive in the IDE surface, got:\n{}",
        result
    );
}

#[test]
fn v_if_and_v_for_on_same_element_both_removed() {
    let result = gen_tsx_template(
        r#"<template><div v-for="item in items" v-if="item.active">{{ item.name }}</div></template>"#,
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got: {}",
        result
    );
    assert!(
        result.contains("___VERTER___flowEach"),
        "should have a v-for frame, got: {}",
        result
    );
    assert!(
        result.contains("?"),
        "should have ternary from v-if (not IIFE), got: {}",
        result
    );
    assert!(
        result.contains(": null"),
        "should have ternary null branch, got: {}",
        result
    );
}

/// Discriminating regression guard — an iterable whose only identifier is also
/// the v-for loop local (`v-for="item in item"`) parses with an OXC v-for present
/// but ZERO in-range references (the local is filtered out of the reference set).
/// That sub-case must emit the iterable VERBATIM (`{(item)`), exactly as a
/// reference-bearing resolver patch over an empty reference set would: the
/// resolver prefix is never applied because there is no reference to apply it to.
/// Routing this through the resolver-only simple-expression path instead prefixes
/// the bare identifier to `___VERTER___instance.item`, a generated-byte regression.
#[test]
fn v_for_iterable_only_loop_local_emits_verbatim_not_resolver_prefixed() {
    let result =
        gen_tsx_template(r#"<template><div v-for="item in item">{{ item }}</div></template>"#);
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    // The iterable identifier is the v-for local → OXC yields zero in-range refs.
    // Parent behavior: emit the iterable verbatim as the frame source, evaluated
    // before the alias declaration (so it reads the outer `item`).
    assert!(
        result.contains(
            "const ___VERTER___v0 = (item); { const item = ___VERTER___flowEach1(___VERTER___v0);"
        ),
        "iterable with only the loop local must stay verbatim `(item)`, got: {}",
        result
    );
    // It must NOT route through the resolver-only path (which prefixes the bare
    // identifier as an instance member access).
    assert!(
        !result.contains("___VERTER___instance.item"),
        "iterable must not be resolver-prefixed to `___VERTER___instance.item`, got: {}",
        result
    );
}

// ── v-if prop narrowing guard tests ──────────────────────────

#[test]
fn v_if_event_handler_gets_guard() {
    let result = gen_tsx_template(
        r#"<template><div v-if="show" @click="handler($event)">click</div></template>"#,
    );
    // The handler reads `handler`: it is snapshot in the branch block and
    // re-narrowed at the wrapped body's start.
    assert!(
        result.contains(
            "{(()=>{if(___VERTER___instance.show){\nconst ___VERTER___o0 = ___VERTER___instance.handler;\n"
        ),
        "the outer reference is snapshot in the branch block, got: {}",
        result
    );
    assert!(
        result.contains(
            "onClick={($event) => {if (!___VERTER___flowNarrow(___VERTER___instance.handler, ___VERTER___o0) || ___VERTER___flowExcluded(___VERTER___o0)(___VERTER___instance.handler)) throw 0; ___VERTER___instance.handler($event)}}"
        ),
        "event handler in v-if should open with the re-narrowing guard, got: {}",
        result
    );
    // Negative: v-if should not appear, and the condition is never replayed.
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got: {}",
        result
    );
    assert_eq!(
        result.matches("instance.show").count(),
        1,
        "the condition must be emitted once, got: {}",
        result
    );
}

#[test]
fn v_else_if_event_handler_gets_combined_guard() {
    let result = gen_tsx_template(
        r#"<template><div v-if="a">A</div><div v-else-if="b" @click="handler($event)">B</div></template>"#,
    );
    // The v-else-if branch block continues the chain's flow (prior negations
    // included): its handler is guarded from a snapshot taken in that block.
    let else_if = result
        .find("}else if(___VERTER___instance.b){\nconst ___VERTER___o0 = ___VERTER___instance.handler;\n")
        .expect("snapshot in the v-else-if branch block");
    let guard = result
        .find("throw 0; ___VERTER___instance.handler($event)")
        .expect("guarded handler");
    assert!(
        else_if < guard,
        "the guard follows its snapshot, got: {}",
        result
    );
    // Negative: the predecessor condition is never re-emitted as a negation.
    assert!(
        !result.contains("!((") && result.matches("instance.a").count() == 1,
        "predecessor conditions must not be replayed, got: {}",
        result
    );
}

#[test]
fn v_if_non_function_prop_no_guard() {
    let result =
        gen_tsx_template(r#"<template><div v-if="show" :class="myClass">content</div></template>"#);
    // Non-function bindings are not callbacks: no snapshot, no guard.
    assert!(
        !result.contains("throw 0") && !result.contains("___VERTER___o0"),
        "non-function prop should not be guarded, got: {}",
        result
    );
}

#[test]
fn v_bind_function_expr_gets_block_guard() {
    // Function expression: `:handler="function() { return msg.trim() }"` inside v-if
    let result = gen_tsx_template(
        r#"<template><div v-if="typeof msg === 'string'" :handler="function() { return msg.trim() }">hi</div></template>"#,
    );
    assert!(
        result.contains(
            "handler={function() {if (!___VERTER___flowNarrow(___VERTER___instance.msg, ___VERTER___o0) || ___VERTER___flowExcluded(___VERTER___o0)(___VERTER___instance.msg) || !___VERTER___flowNarrow(___VERTER___instance.msg!.trim, ___VERTER___o1) || ___VERTER___flowExcluded(___VERTER___o1)(___VERTER___instance.msg!.trim)) throw 0;  return ___VERTER___instance.msg.trim() }}"
        ),
        "function expression prop should get the guard right after its `{{`, got:\n{}",
        result
    );
}

#[test]
fn v_if_guards_relocated_spread_event_handlers() {
    // A hyphenated event spreads its handler, relocating the value. Each
    // callback form is still re-narrowed from the snapshot under `v-if`.
    let guard = "if (!___VERTER___flowNarrow(handle, ___VERTER___o0) || ___VERTER___flowExcluded(___VERTER___o0)(handle)) throw 0;";
    let bindings = [
        ("ok", BindingType::SetupConst),
        ("handle", BindingType::SetupConst),
        ("LocalComp", BindingType::SetupConst),
    ];

    let inline = gen_tsx_template_with_bindings(
        r#"<template><LocalComp v-if="ok" @my-event="handle()"/></template>"#,
        &bindings,
    );
    assert!(
        inline.contains(&format!("\"onMy-event\": () => {{{guard} handle()}}")),
        "inline spread handler must open with the guard: {inline}"
    );

    let event_param = gen_tsx_template_with_bindings(
        r#"<template><LocalComp v-if="ok" @my-event="handle($event)"/></template>"#,
        &bindings,
    );
    assert!(
        event_param.contains(&format!("{guard} handle($event)")),
        "`$event` spread handler must open with the guard: {event_param}"
    );

    let arrow_expr = gen_tsx_template_with_bindings(
        r#"<template><LocalComp v-if="ok" @my-event="() => handle()"/></template>"#,
        &bindings,
    );
    assert!(
        arrow_expr.contains(&format!("() => {{ {guard} return handle(); }}) satisfies")),
        "arrow-expression spread handler must become a guarded block: {arrow_expr}"
    );

    let arrow_block = gen_tsx_template_with_bindings(
        r#"<template><LocalComp v-if="ok" @my-event="() => { handle() }"/></template>"#,
        &bindings,
    );
    assert!(
        arrow_block.contains(&format!("=> {{{guard}  handle() }}) satisfies")),
        "arrow-block spread handler must open with the guard: {arrow_block}"
    );

    let unconditional = gen_tsx_template_with_bindings(
        r#"<template><LocalComp @my-event="handle()"/></template>"#,
        &bindings,
    );
    assert!(
        !unconditional.contains("throw 0"),
        "no condition, no guard: {unconditional}"
    );
}

#[test]
fn v_if_guarded_function_value_tsx_is_byte_equivalent() {
    // The re-narrowing guard of a function-typed value prop, per callback shape.
    // The authored function stays in place; only the guard (and, for an
    // expression body, the block that returns it) is spliced in. SetupConst →
    // bare identifier; Props → `__props.` accessor prefix.

    // Arrow-expression body, SetupConst (no prefix).
    let arrow_expr = gen_tsx_template_with_bindings(
        r#"<template><div v-if="ok" :onX="() => handle()"/></template>"#,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::SetupConst),
        ],
    );
    let guard = "if (!___VERTER___flowNarrow(handle, ___VERTER___o0) || ___VERTER___flowExcluded(___VERTER___o0)(handle)) throw 0;";
    assert!(
        arrow_expr.contains(&format!("onX={{() => {{ {guard} return handle(); }}}}")),
        "arrow-expr body must become a guarded block returning it: {arrow_expr}"
    );

    // Arrow-expression body, Props (accessor prefix on the body identifier and
    // on its snapshot; the v-if condition is resolved independently).
    let arrow_expr_props = gen_tsx_template_with_bindings(
        r#"<template><div v-if="ok" :onX="() => handle()"/></template>"#,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::Props),
        ],
    );
    assert!(
        arrow_expr_props.contains("const ___VERTER___o0 = __props.handle;")
            && arrow_expr_props.contains("throw 0; return __props.handle(); }}"),
        "arrow-expr Props body must keep its `__props.` accessor prefix after the guard: {arrow_expr_props}"
    );

    // Arrow-block body, SetupConst.
    let arrow_block = gen_tsx_template_with_bindings(
        r#"<template><div v-if="ok" :onX="() => { handle() }"/></template>"#,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::SetupConst),
        ],
    );
    assert!(
        arrow_block.contains(&format!("onX={{() => {{{guard}  handle() }}}}")),
        "arrow-block guard must open the authored block: {arrow_block}"
    );

    // Function-expression body, SetupConst.
    let fn_expr = gen_tsx_template_with_bindings(
        r#"<template><div v-if="ok" :onX="function() { handle() }"/></template>"#,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::SetupConst),
        ],
    );
    assert!(
        fn_expr.contains(&format!("onX={{function() {{{guard}  handle() }}}}")),
        "fn-expr guard must open the authored block: {fn_expr}"
    );

    // Negative (all shapes): exactly ONE guard and ONE snapshot per prop.
    for output in [&arrow_expr, &arrow_block, &fn_expr] {
        assert_eq!(
            output.matches("throw 0;").count(),
            1,
            "one guard per prop: {output}"
        );
        assert_eq!(
            output.matches("const ___VERTER___o").count(),
            1,
            "one snapshot per outer reference: {output}"
        );
    }
}

#[test]
fn v_bind_non_function_no_guard() {
    // Non-function props should NOT get any guard
    let result = gen_tsx_template(r#"<template><div v-if="show" :class="msg">hi</div></template>"#);
    let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    // Find the class prop
    let class_pos = norm.find("class={").expect("should have class prop");
    let after_class = &norm[class_pos..];
    // Should NOT have any guard
    assert!(
        !after_class.starts_with("class={()=>") && !result.contains("throw 0"),
        "non-function prop should not get guard, got:\n{}",
        result
    );
}

#[test]
fn v_model_on_native_element() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="msg" /></template>"#,
        &[("msg", BindingType::SetupRef)],
    );
    // Native input should use `value` (not `modelValue`) and native event handler
    assert!(
        result.contains("value={msg}"),
        "v-model on native input should produce value prop. Got: {}",
        result
    );
    assert!(
        !result.contains("modelValue"),
        "v-model on native input must NOT use modelValue. Got: {}",
        result
    );
    assert!(
        result.contains("onInput={"),
        "v-model on native input should use onInput event. Got: {}",
        result
    );
    // Must not have any quoted attribute names (invalid JSX)
    assert!(
        !result.contains(r#""onUpdate:"#),
        "native input must not have quoted onUpdate attribute. Got: {}",
        result
    );
    assert!(
        !result.contains("v-model"),
        "v-model attribute must be removed. Got: {}",
        result
    );
}

#[test]
fn v_model_with_explicit_change_handler_no_duplicate() {
    // v-model on <input type="checkbox"> + explicit @change should not produce
    // duplicate onChange attributes (TS17001).
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="model" type="checkbox" @change="handleChange" /></template>"#,
        &[
            ("model", BindingType::SetupRef),
            ("handleChange", BindingType::SetupConst),
        ],
    );
    let on_change_count = result.matches("onChange=").count()
        + result.matches("onChange:").count()
        + result.matches("\"onChange\"").count();
    assert_eq!(
        on_change_count, 1,
        "v-model + @change on native input should produce exactly one onChange. Got {} in: {}",
        on_change_count, result
    );
    assert!(
        !result.contains("v-model"),
        "v-model attribute must be removed. Got: {}",
        result
    );
}

#[test]
fn v_model_with_explicit_input_handler_no_duplicate() {
    // v-model on text <input> + explicit @input should not produce
    // duplicate onInput attributes.
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="text" @input="onInput" /></template>"#,
        &[
            ("text", BindingType::SetupRef),
            ("onInput", BindingType::SetupConst),
        ],
    );
    let on_input_count = result.matches("onInput=").count();
    assert_eq!(
        on_input_count, 1,
        "v-model + @input on text input should produce exactly one onInput. Got {} in: {}",
        on_input_count, result
    );
    // v-model should still produce the value prop
    assert!(
        result.contains("value={text}"),
        "v-model should still produce value prop. Got: {}",
        result
    );
}

#[test]
fn duplicate_keydown_handlers_use_spread_for_second() {
    // @keydown.space + @keydown.enter both map to onKeyDown —
    // the second must use spread syntax to avoid TS17001.
    let result = gen_tsx_template_with_bindings(
        r#"<template><td @keydown.space.prevent.stop="handleClick" @keydown.enter.prevent.stop="handleClick" /></template>"#,
        &[("handleClick", BindingType::SetupConst)],
    );
    let on_keydown_attr = result.matches("onKeyDown={").count();
    assert!(
        on_keydown_attr <= 1,
        "should have at most one onKeyDown= attribute (rest as spread). Got {} in: {}",
        on_keydown_attr,
        result
    );
    // Should still reference both handlers somehow
    assert!(
        result.contains("handleClick"),
        "handler reference should be present. Got: {}",
        result
    );
}

// ── Instance property resolution in TSX ─────────────────────────

#[test]
fn tsx_unresolved_dollar_emit_gets_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ $emit('click') }}</div></template>"#,
        &[],
    );
    assert!(
        result.contains("___VERTER___instance.$emit"),
        "Unresolved $emit should get instance prefix. Got: {}",
        result
    );
    assert!(
        !result.contains("{ $emit(") && !result.contains("{$emit("),
        "Bare $emit without prefix must not appear. Got: {}",
        result
    );
}

// ── Dynamic event names in TSX ────────────────────────────────

#[test]
fn dynamic_event_name() {
    let result = gen_tsx_template(r#"<template><div @[eventName]="handler" /></template>"#);
    assert!(
        result.contains("eventName") || result.contains("_ctx.eventName"),
        "Dynamic event should reference eventName. Got: {}",
        result
    );
    assert!(
        !result.contains("@["),
        "Dynamic event syntax must be removed. Got: {}",
        result
    );
}

#[test]
fn dynamic_event_name_with_binding() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @[eventName]="handler" /></template>"#,
        &[("eventName", BindingType::SetupRef)],
    );
    assert!(
        result.contains("eventName") && !result.contains("eventName.value"),
        "Dynamic event name on ref should be bare identifier in TSX mode (no .value). Got: {}",
        result
    );
}

#[test]
fn event_handler_simple_ident_is_source_mapped() {
    // @click="handler" — the handler identifier should have a source map token.
    let source = r#"<template><button @click="handler">click</button></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("handler", BindingType::SetupConst)]);

    assert!(
        output.contains("onClick={handler}"),
        "should emit onClick={{handler}}: {output}"
    );

    // Find the byte offset of "handler" in the @click value
    let handler_src_offset = source.find("handler").unwrap();

    let has_handler_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == handler_src_offset as u32);
    assert!(
        has_handler_token,
        "event handler should have source map token at src col {}. Tokens: {:?}",
        handler_src_offset, tokens
    );
}

#[test]
fn event_handler_fn_expr_is_source_mapped() {
    // @click="(e) => doSomething(e)" — the expression should be source-mapped.
    let source = r#"<template><button @click="(e) => doSomething(e)">click</button></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("doSomething", BindingType::SetupConst)]);

    assert!(
        output.contains("onClick={(e) => doSomething(e)}"),
        "should emit onClick with fn expr: {output}"
    );

    // Find the byte offset of the expression in the @click value
    let expr_src_offset = source.find("(e) => doSomething").unwrap();

    let has_expr_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
    assert!(
        has_expr_token,
        "fn expression should have source map token at src col {}. Tokens: {:?}",
        expr_src_offset, tokens
    );
}

#[test]
fn event_handler_inline_expr_is_source_mapped() {
    // @click="count++" — the inline expression should be source-mapped.
    // Using SetupConst to avoid .value transformation changing the text.
    let source = r#"<template><button @click="count++">click</button></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupConst)]);

    assert!(
        output.contains("count++"),
        "should contain the expression: {output}"
    );

    // Find byte offset of "count++" in the @click value
    let expr_src_offset = source.find("count++").unwrap();

    let has_expr_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
    assert!(
        has_expr_token,
        "inline expression should have source map token at src col {}. Tokens: {:?}",
        expr_src_offset, tokens
    );
}

/// The synthetic closing suffix of an in-place v-on handler (the `}}` wrapper
/// close for an inline expression, the `}` JSX-container close for a simple
/// handler) is compiler-synthesized scaffolding with no source token, so it must
/// map to None — consistent with the rest of the decomposed handler boundary
/// (prefix delete, scaffold-after-event, guard are all unmapped). A MAPPED
/// overwrite of the closing span would point the synthetic braces at the body
/// end (the close quote), which would land go-to-definition / hover on a
/// synthetic brace.
///
/// Discriminating: pre-fix the closing suffix was emitted via a mapped
/// `out.overwrite(trimmed_ve, prop_end, suffix)`, producing a token at the
/// suffix's generated column → this assertion FAILS. Post-fix the suffix is an
/// unmapped inserted chunk → no token at that column → PASSES. The generated TSX
/// text is byte-identical across the fix (pinned by
/// `event_handler_inline_expr_is_source_mapped` /
/// `event_handler_simple_ident_is_source_mapped`).
#[test]
fn v_on_handler_closing_suffix_maps_to_none() {
    // Inline expression: `@click="count++"` → `onClick={() => {count++}}`. The
    // trailing `}}` is the wrapper + container close — synthetic.
    let inline = r#"<template><button @click="count++">click</button></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(inline, &[("count", BindingType::SetupConst)]);
    assert!(
        output.contains("onClick={() => {count++}}"),
        "inline handler output must be byte-stable: {output}"
    );
    // The synthetic suffix is the FINAL `}}` run before the closing `>` of the tag.
    let gt = output.find("}}>").expect("expected `}}>` in output");
    let suffix_col = gt as u32; // generated column of the first `}` of the synthetic suffix
    assert!(
        !tokens.iter().any(|&(_, dst_col, _)| dst_col == suffix_col),
        "synthetic v-on closing suffix (gen col {suffix_col}) must NOT carry a source \
         map token (synthetic → None). Tokens: {tokens:?}"
    );

    // Simple handler: `@click="handler"` → `onClick={handler}`. The trailing `}`
    // is the JSX-container close — synthetic.
    let simple = r#"<template><button @click="handler">click</button></template>"#;
    let (o2, t2) = gen_tsx_template_with_map(simple, &[("handler", BindingType::SetupConst)]);
    assert!(
        o2.contains("onClick={handler}"),
        "simple handler output must be byte-stable: {o2}"
    );
    let gt2 = o2.find("}>").expect("expected `}>` in simple output");
    let suffix_col2 = gt2 as u32; // generated column of the synthetic `}` close
    assert!(
        !t2.iter().any(|&(_, dst_col, _)| dst_col == suffix_col2),
        "synthetic v-on closing `}}` (gen col {suffix_col2}) must NOT carry a source \
         map token (synthetic → None). Tokens: {t2:?}"
    );
}

#[test]
fn slot_outlet_no_interpolation_past_mapped_content() {
    // Simulates vue_to_tsx interpolation for the meaningful parts of the slot tag:
    // tag name (`slot`), attribute name (`name`), and attribute value (`reference`).
    // These positions must NOT land on the `(` of `?.()` — that causes `() any` hover.
    // Structural syntax (closing `"`, ` />`) may map to the `?.` operator, which is fine.
    let source = r#"<template><slot name="reference" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Find the generated position of `(` in `?.()` — this is where TSGO shows `() any`
    let call_paren_pos = output.find("?.()").unwrap() as u32 + 2; // position of `(`

    // Meaningful source positions: `<slot name="reference`
    // (excludes closing `"` and ` />` which are structural syntax)
    let tag_start = source.find("<slot").unwrap() as u32;
    let ref_end = source.find("reference").unwrap() as u32 + "reference".len() as u32;

    // Simulate vue_to_tsx for meaningful positions
    for query_col in tag_start..ref_end {
        let best = tokens
            .iter()
            .filter(|&&(_, _, sc)| sc <= query_col)
            .max_by_key(|&&(_, _, sc)| sc);

        if let Some(&(_, dst_col, src_col)) = best {
            let delta = query_col - src_col;
            let interpolated_dst = dst_col + delta;

            assert!(
                interpolated_dst < call_paren_pos,
                "source col {} interpolates to gen col {} (token src={} dst={} + delta={}), \
                 which is at/past `(` in `?.()` (gen col {}). This causes `() any` hover. Output: {}",
                query_col, interpolated_dst, src_col, dst_col, delta,
                call_paren_pos, output
            );
        }
    }
}

// ── Class/style merge source map accuracy ───────────────────────

#[test]
fn class_merge_dynamic_class_position_is_mapped() {
    // When both `class="foo"` and `:class="bar"` exist, the `:class` directive's
    // argument position should have a source map token pointing to the merged
    // `class={normalizeClass(...)}` attribute. The static `class` position is NOT
    // mapped in the codegen (the static attribute is removed from TSX); hover for
    // the static `class` is handled by the LSP hover handler which redirects the
    // TSGO query to the `:class` directive's position.
    let source = r#"<template><div class="foo" :class="bar"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Find source position of the `:` in `:class` (the directive start / overwrite origin)
    let colon_class_col = source.find(":class").unwrap() as u32;

    // Find generated position of the merged `class=` attribute
    let gen_class_pos = output.find("class=").unwrap() as u32;

    // The `:class` directive start should have a source map token mapping
    // to the merged `class=` in generated TSX. This is the redirect target
    // used by the hover handler for the static `class` attribute.
    let token_for_colon = tokens.iter().find(|&&(_, _, sc)| sc == colon_class_col);
    assert!(
        token_for_colon.is_some(),
        "`:class` at src col {} should have a source map token. \
         Generated output: {}. Tokens: {:?}",
        colon_class_col,
        output,
        tokens
    );

    let &(_, dst_col, _) = token_for_colon.unwrap();
    assert!(
        dst_col >= gen_class_pos && dst_col < gen_class_pos + 6,
        "`:class` should map to merged `class=` region (gen cols {}..{}), got gen col {}. Output: {}",
        gen_class_pos, gen_class_pos + 6, dst_col, output
    );
}

#[test]
fn v_for_numeric_expression_range_valid_tsx() {
    // Bug: v-for="i in count + 1" where count+1 might be a numeric expression
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-for="i in count" :key="i">{{ i }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    eprintln!("TSX output:\n{}", result);

    // Non-literal iterables are the frame source as-is
    assert!(
        result.contains("const ___VERTER___v0 = (count);"),
        "should evaluate non-literal iterables as the frame source, got: {}",
        result
    );
}

/// Regression: complex template (notification-like) with transition, v-show, component :is,
/// v-text, and v-html must produce valid TSX without syntax errors.
#[test]
fn notification_template_complex_no_syntax_errors() {
    let source = r#"<template>
  <transition
    :name="ns.b('fade')"
    @before-leave="onClose"
    @after-leave="$emit('destroy')"
  >
    <div
      v-show="visible"
      :id="id"
      :class="[ns.b(), customClass, horizontalClass]"
      :style="positionStyle"
      role="alert"
      @mouseenter="clearTimer"
      @mouseleave="startTimer"
      @click="onClick"
    >
      <el-icon v-if="iconComponent" :class="[ns.e('icon'), typeClass]">
        <component :is="iconComponent" />
      </el-icon>
      <div :class="ns.e('group')">
        <h2 :class="ns.e('title')" v-text="title" />
        <div
          v-show="message"
          :class="ns.e('content')"
          :style="!!title ? undefined : { margin: 0 }"
        >
          <slot>
            <p v-if="!dangerouslyUseHTMLString">{{ message }}</p>
            <!-- Caution here, message could've been compromised, never use user's input as message -->
            <p v-else v-html="message" />
          </slot>
        </div>
        <el-icon v-if="showClose" :class="ns.e('closeBtn')" @click.stop="close">
          <component :is="closeIcon" />
        </el-icon>
      </div>
    </div>
  </transition>
</template>"#;
    let result = gen_tsx_template(source);
    eprintln!("=== NOTIFICATION TEMPLATE TSX ===\n{}\n=== END ===", result);

    // Negative: no stray leaked binding prefixes
    assert!(
        !result.contains("}}___VERTER___instance."),
        "binding prefix must not leak after style closing braces. Got:\n{}",
        result
    );

    // Parse the result with OXC to check for syntax errors
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::tsx();
    let wrapped = format!("import {{}} from 'vue';\n{}", result);
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &wrapped, source_type).parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "Generated TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.diagnostics.len(),
        result
    );
}

#[test]
fn activist_card_topic_selection_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_TEST_REPOS_ROOT",
        "activist-org-activist/frontend/app/components/card/CardTopicSelection.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("CardTopicSelection.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::legacy_test_support::VerterCompileOptions::default();
    let result = crate::compile::legacy_test_support::compile(
        &source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== ACTIVIST TSX ===\n{}\n=== END ===", tsx.code);
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "Got {} errors",
        parsed.diagnostics.len()
    );
}

#[test]
fn external_notification_component_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_PRIVATE_CORPUS_ROOT",
        "packages/ui/src/components/Notifications/components/Notification.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("Notification.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::legacy_test_support::VerterCompileOptions::default();
    let result = crate::compile::legacy_test_support::compile(
        &source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    // ___VERTER___props must be declared (not just referenced)
    assert!(
        tsx.code.contains("const ___VERTER___props"),
        "Destructured defineProps should declare ___VERTER___props. Got:\n{}",
        tsx.code
    );

    // Parse with OXC to verify valid TSX
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "Got {} errors",
        parsed.diagnostics.len(),
    );
}

// ── Kebab-case event handling in spread syntax ─────────────────────────────

#[test]
fn kebab_event_with_dollar_event_emits_typed_payload_param() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="emit('clickOverlay', $event)" /></template>"#,
        &[("emit", BindingType::SetupConst)],
    );
    // Hyphenated JSX name can't be a bare attribute → spread key.
    assert!(
        result.contains(r#""onClick-overlay""#),
        "should preserve kebab-case event name as spread key: {result}"
    );
    // `$event` is bound as the handler's sole parameter, EXPLICITLY annotated with
    // an ambient DOM event-payload type — JSX contextual typing cannot flow through a
    // spread, so a bare parameter would be `any`. A non-standard native event
    // (`click-overlay`) is not a known DOM event, so the intersected `Event` index
    // fallback types it as the base `Event` (never a `ts(2339)` error).
    assert!(
        result.contains("($event:"),
        "spread $event must be an explicitly-typed parameter: {result}"
    );
    assert!(
        result.contains(r#"(GlobalEventHandlersEventMap & { [___VERTER___EventKey: string]: Event })["click-overlay"]"#),
        "spread $event type must be the ambient DOM event-map type keyed by the event name: {result}"
    );
    // Negative: the `import('vue')` indexed formula is NOT used for native spread
    // `$event` — it does not resolve under the tsgo TypeProvider.
    assert!(
        !result.contains("IntrinsicElementAttributes"),
        "native spread $event must not use the import('vue') formula: {result}"
    );
    // Negative: the generic `eventCallbacks<TArgs extends Array<any>>` helper that
    // forced `$event` to `any` is gone.
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "spread $event must NOT use the eventCallbacks wrapper: {result}"
    );
    assert!(
        !result.contains("...___VERTER___eventArgs"),
        "spread $event must NOT use the generic event-args rest param: {result}"
    );
}

#[test]
fn kebab_event_arrow_function_satisfies_native_payload() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="($event) => doSomething($event)" /></template>"#,
        &[("doSomething", BindingType::SetupConst)],
    );
    // JSX contextual typing cannot flow through a spread, so the arrow handler's
    // parameter would be implicit-`any`. The arrow is wrapped in a `satisfies` clause
    // whose target is the native event-handler signature, so `$event` is contextually
    // typed against the ambient DOM payload tuple. The user arrow stays source-mapped.
    assert!(
        result.contains(
            r#""onClick-overlay": (($event) => doSomething($event)) satisfies (...___VERTER___eventArgs: "#
        ),
        "arrow handler must be `satisfies`-wrapped on spread: {result}"
    );
    assert!(
        result.contains(
            r#"[(GlobalEventHandlersEventMap & { [___VERTER___EventKey: string]: Event })["click-overlay"]]) => unknown"#
        ),
        "satisfies target must be the native DOM payload tuple: {result}"
    );
    // Negative: the `import('vue')` indexed formula is NOT used (it does not resolve
    // under the tsgo TypeProvider).
    assert!(
        !result.contains("IntrinsicElementAttributes"),
        "native spread arrow must not use the import('vue') formula: {result}"
    );
    // Negative: should NOT double-wrap the arrow body into a synthetic block.
    assert!(
        !result.contains("($event) => {($event)"),
        "should NOT double-wrap arrow function: {result}"
    );
}

#[test]
fn kebab_event_function_expr_satisfies_native_payload() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="function($event) { doSomething($event) }" /></template>"#,
        &[("doSomething", BindingType::SetupConst)],
    );
    // Function-expression handler is `satisfies`-wrapped exactly like the arrow case.
    assert!(
        result.contains(r#""onClick-overlay": (function($event) { doSomething($event) }) satisfies (...___VERTER___eventArgs: "#),
        "function expression must be `satisfies`-wrapped on spread: {result}"
    );
    assert!(
        result.contains(r#") => unknown"#),
        "satisfies clause must close with `=> unknown`: {result}"
    );
}

#[test]
fn kebab_event_inline_expr_no_dollar_event_wraps_with_no_param() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="count++" /></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    // Inline expression without $event → () => { ... }
    assert!(
        result.contains("() => {"),
        "should wrap with () => for inline expr without $event: {result}"
    );
    assert!(
        result.contains(r#""onClick-overlay""#),
        "should preserve kebab-case event name: {result}"
    );
}

// ── Fix 1: Broken interpolation recovery ──────────────────────────

#[test]
fn broken_interpolation_preserves_identifiers() {
    // Broken expression: {{ count + }} — OXC can't parse it, but identifiers must survive
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count + }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    eprintln!("broken interpolation output: {}", result);
    // Positive: identifiers preserved
    assert!(
        result.contains("count"),
        "broken expression should preserve identifiers: {result}"
    );
    // Positive: mustache delimiters converted
    assert!(
        result.contains('{') && result.contains('}') && result.contains("count"),
        "mustache should be converted to a JSX expression with preserved identifiers: {result}"
    );
    // Negative: no raw mustache delimiters
    assert!(
        !result.contains("{{") && !result.contains("}}"),
        "mustache delimiters must be converted to JSX: {result}"
    );
    assert_valid_tsx(&result, "broken-interpolation");
}

#[test]
fn broken_interpolation_keeps_identifier_source_map_anchor() {
    let source = r#"<template><div>{{ count + }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupConst)]);

    let count_src_col = source.find("count").unwrap() as u32;
    let anchor = tokens
        .iter()
        .filter(|&&(_, _, src_col)| src_col <= count_src_col)
        .max_by_key(|&&(_, _, src_col)| src_col)
        .copied();

    assert!(
        anchor.is_some(),
        "broken interpolation should retain a usable source-map anchor before 'count', tokens: {:?}",
        tokens
    );

    let (_gen_line, gen_col, anchor_src_col) = anchor.unwrap();
    let mapped_col = gen_col + (count_src_col - anchor_src_col);
    let first_line = output.lines().next().unwrap_or("");
    assert!(
        first_line
            .get(mapped_col as usize..)
            .is_some_and(|suffix| suffix.starts_with("count")),
        "broken interpolation should keep linear mapping from the nearest anchor to 'count', got output: {output}, anchor={anchor:?}"
    );
}

#[test]
fn incomplete_v_slot_member_keeps_local_receiver_and_completion_boundary() {
    let result = gen_tsx_template(
        r#"<template><MyComp v-slot="{ title }"><span>{{ title. }}</span></MyComp></template>"#,
    );
    assert!(
        result.contains("title.valueOf"),
        "the authored member dot should remain the mapped completion boundary and receive only an unmapped member-hole placeholder, got: {result}"
    );
    assert!(
        !result.contains("___VERTER___instance.title"),
        "an incomplete scoped-slot member receiver must remain a lexical local, got: {result}"
    );
}

#[test]
fn custom_directive_non_identifier_modifier_stays_quoted() {
    // A modifier that is not a valid JS identifier must stay a quoted key to be
    // legal JavaScript; its name is still individually mapped.
    let source = r#"<template><div v-test.some-mod="val" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    assert!(
        output.contains(r#""some-mod":true"#),
        "a non-identifier modifier must stay a quoted key: {output}"
    );
    let src = source.find(".some-mod").unwrap() + 1;
    assert_mapped_run(&output, &tokens, "some-mod", src, "non-identifier modifier");
}

#[test]
fn custom_directive_on_component() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-focus /></template>"#,
        &[("MyComp", BindingType::SetupConst)],
    );
    eprintln!("custom_directive_on_component:\n{result}");

    // Should work the same on components
    assert!(
        result.contains("v-directive="),
        "should emit v-directive on component: {result}"
    );
    assert!(
        result.contains(r#"directiveAccessor["vFocus"]"#),
        "should reference vFocus: {result}"
    );
}

/// A COMPOUND value expression must map back across its WHOLE extent, not only at
/// its identifiers.
///
/// TypeScript reports the argument-type error (`TS2345`) over the COMPLETE
/// `1 + count` expression, and a strict range mapper composes a carrier range only
/// from runs contiguous in BOTH the generated and the source space. The relocated
/// sink used to emit the leading `1 + ` operand+operator UNMAPPED, so the
/// diagnostic RANGE started in a hole and was dropped whole — the user saw no
/// squiggle on a real type error. Asserting only the identifier ANCHOR (as the
/// pre-existing value-mapping test does) cannot see that.
#[test]
fn custom_directive_compound_value_expression_maps_across_its_whole_extent() {
    let source = r#"<template><div v-color="1 + count" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("vColor", BindingType::SetupConst),
            ("count", BindingType::SetupConst),
        ],
    );

    let expr_src = source.find("1 + count").expect("fixture expression");
    // The leading `1 + ` run — where the diagnostic RANGE starts.
    assert_mapped_run(
        &output,
        &tokens,
        "1 + count",
        expr_src,
        "compound value expression start",
    );

    // …and `count` owns its OWN run, anchored so the two runs are contiguous in
    // BOTH spaces (generated +4 ↔ authored +4). Without that the range still
    // fails to compose and the diagnostic is still dropped.
    let gen_off = output.find("1 + count").expect("relocated expression");
    assert_token_at(
        &output,
        &tokens,
        gen_off + "1 + ".len(),
        expr_src + "1 + ".len(),
        "compound value expression identifier",
    );
}

/// A MULTILINE value expression must own a mapped run on EVERY generated line it
/// spans, not only the first.
///
/// A source-map token anchors ONE generated line: mapping state does not carry
/// across a generated newline. A relocated verbatim slice emitted as a SINGLE
/// `InsertMapped` chunk therefore produces one token on its first line and leaves
/// every later generated line of the SAME authored expression starting unmapped.
/// The strict range mapper joins two runs across a newline only when the later run
/// begins at column 0 of BOTH the next generated line and the next source line, so
/// the trailing operand lands in a DIFFERENT compatibility component and the
/// whole-expression `TS2345` range is dropped — the user sees no squiggle on a real
/// type error. The single-line sibling test above cannot see this: it never crosses
/// a generated newline.
#[test]
fn custom_directive_multiline_value_expression_maps_on_every_generated_line() {
    let source = "<template><div v-color=\"\n    1 +\n    count\n  \" /></template>";
    let (output, tokens) = gen_tsx_template_with_line_map(
        source,
        &[
            ("vColor", BindingType::SetupConst),
            ("count", BindingType::SetupConst),
        ],
    );

    // The authored line break survives into the generated output verbatim. Collapsing
    // the expression onto one generated line would sidestep the mapping bug by
    // changing the emitted code shape, so pin the two-line emission first.
    let gen_expr = output
        .find("1 +\n    count")
        .expect("the relocated expression keeps its authored line break");

    let src_expr = source.find("1 +").expect("fixture first operand");
    let src_line2 = source
        .find("    count")
        .expect("fixture second authored line");
    let src_count = src_line2 + "    ".len();

    // Generated line 1 of the expression: the leading operand+operator.
    assert_token_at_lc(
        &output,
        source,
        &tokens,
        gen_expr,
        src_expr,
        "multiline expression first line",
    );

    // Generated line 2 STARTS with the authored indentation — it must own its own
    // mapped run anchored at column 0 of the next AUTHORED line. Without it the
    // generated line begins unmapped and the run chain breaks across the newline.
    assert_token_at_lc(
        &output,
        source,
        &tokens,
        gen_expr + "1 +\n".len(),
        src_line2,
        "multiline expression continuation line start",
    );

    // …and the trailing identifier still anchors at its own authored token, so the
    // continuation run and the identifier run stay contiguous in BOTH spaces.
    assert_token_at_lc(
        &output,
        source,
        &tokens,
        gen_expr + "1 +\n    ".len(),
        src_count,
        "multiline expression trailing identifier",
    );
}

#[test]
fn strict_slots_interpolation_child() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs>{{ msg }}</Tabs></template>",
        &[
            ("Tabs", BindingType::SetupImport),
            ("msg", BindingType::SetupRef),
        ],
    );
    // Positive: strictRenderSlot with string type for interpolation
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot for interpolation, got:\n{}",
        result
    );
    assert!(
        result.contains("as string"),
        "should have string type for interpolation, got:\n{}",
        result
    );
}

// ── Options API component alias resolution ────────────────────────────────

#[test]
fn options_api_component_alias_emits_binding() {
    let source = r#"<script lang="ts">
import { defineComponent } from 'vue'
import SomeComp from './SomeComp.vue'

export default defineComponent({
  components: { MyAlias: SomeComp },
  setup() { return {} }
})
</script>
<template>
  <MyAlias />
</template>"#;
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("Test.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::legacy_test_support::VerterCompileOptions::default();
    let result = crate::compile::legacy_test_support::compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.as_ref().expect("TSX should be generated");

    // Template should use <MyAlias> and MyAlias must be in scope
    assert!(
        tsx.code.contains("<MyAlias"),
        "template should contain <MyAlias> JSX tag:\n{}",
        tsx.code
    );
    // There must be a const alias that assigns SomeComp to MyAlias
    assert!(
        tsx.code.contains("const MyAlias = SomeComp"),
        "should emit 'const MyAlias = SomeComp' for the component alias:\n{}",
        tsx.code
    );
    // Must be valid TSX
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "TSX should have no parse errors. Got {} errors:\n{}",
        parsed.diagnostics.len(),
        tsx.code
    );
}

// ── Issue #48: $event must not be prefixed with instance ─────────────────

#[test]
fn dollar_event_standalone_not_prefixed() {
    let result = gen_tsx_template(r#"<template><div @click="$event">click</div></template>"#);
    // Positive: $event should appear bare inside the callback
    assert!(
        result.contains("$event"),
        "should contain $event in output: {result}"
    );
    // Negative: $event must NOT be prefixed with ___VERTER___instance.
    assert!(
        !result.contains("___VERTER___instance.$event"),
        "$event must NOT be prefixed with instance, got: {result}"
    );
}

#[test]
fn dollar_event_in_inline_expr_not_prefixed() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click="handleClick($event)">click</div></template>"#,
        &[("handleClick", BindingType::SetupConst)],
    );
    // Positive: handleClick and $event should both be present
    assert!(
        result.contains("handleClick"),
        "should contain handleClick: {result}"
    );
    assert!(result.contains("$event"), "should contain $event: {result}");
    // Negative: $event must NOT be prefixed
    assert!(
        !result.contains("___VERTER___instance.$event"),
        "$event must NOT be prefixed with instance, got: {result}"
    );
}

// ── Issue #46: bare @click (no value) must not emit broken binding ───────

#[test]
fn bare_event_no_value_removed() {
    let result = gen_tsx_template(r#"<template><div @click>click</div></template>"#);
    // Negative: must NOT contain onClick or any broken click binding
    assert!(
        !result.contains("onClick"),
        "bare @click should be removed, must not contain onClick: {result}"
    );
    assert!(
        !result.contains("___VERTER___ctx.click"),
        "bare @click must not produce ctx.click binding: {result}"
    );
    assert!(
        !result.contains("___VERTER___instance.click"),
        "bare @click must not produce instance.click binding: {result}"
    );
}

// ── $event type inference ────────────────────────────────────────────────

#[test]
fn event_handler_native_event_param_is_contextually_typed() {
    // Native `$event` is bound as the handler's sole parameter so it is
    // contextually typed by the JSX event prop (`onClick`) — the same mechanism
    // that types an inline-arrow parameter, so `$event` resolves to the real
    // event type rather than `any`.
    let result =
        gen_tsx_template(r#"<template><div @click="handleClick($event)">click</div></template>"#);
    assert!(
        result.contains("onClick={($event) => {"),
        "native $event handler should bind $event as the contextually-typed parameter: {result}"
    );
    assert!(
        result.contains("handleClick($event)"),
        "should still contain the $event handler body: {result}"
    );
    // Negative: native $event no longer relies on the generic eventCallbacks wrapper.
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "native $event should NOT use the generic eventCallbacks wrapper: {result}"
    );
    assert!(
        !result.contains("...___VERTER___eventArgs"),
        "native $event should NOT use the generic event-args rest param: {result}"
    );
}

#[test]
fn event_handler_component_event_param_is_contextually_typed() {
    // A component `$event` is bound the same way — contextually typed by the
    // component's JSX event prop (`onCustom`).
    let result =
        gen_tsx_template(r#"<template><MyComp @custom="handleCustom($event)" /></template>"#);
    assert!(
        result.contains("onCustom={($event) => {"),
        "component $event handler should bind $event as the contextually-typed parameter: {result}"
    );
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "component in-place $event should NOT use the generic eventCallbacks wrapper: {result}"
    );
}

#[test]
fn event_handler_without_event_param_no_event_callbacks() {
    // Simple identifier — no eventCallbacks needed
    let result = gen_tsx_template(r#"<template><div @click="handleClick">click</div></template>"#);
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "simple ident handler should NOT use eventCallbacks: {result}"
    );

    // Inline expression without $event — no eventCallbacks needed
    let result2 = gen_tsx_template(r#"<template><div @click="count++">click</div></template>"#);
    assert!(
        !result2.contains("___VERTER___eventCallbacks"),
        "inline expr without $event should NOT use eventCallbacks: {result2}"
    );
}

#[test]
fn dollar_event_inside_string_literal_is_not_treated_as_event_param() {
    // Typed-IR detection: `$event` inside a STRING LITERAL is not an identifier
    // reference, so the handler must NOT be wrapped as `($event) => …`. The former
    // `resolved_expr.contains("$event")` substring check wrongly matched here, which
    // would shadow a real outer `$event` and mis-type the handler.
    let result = gen_tsx_template_with_bindings(
        r#"<template><button @click="log('save $event now')" /></template>"#,
        &[("log", BindingType::SetupConst)],
    );
    assert!(
        !result.contains("($event)"),
        "string-literal $event must NOT trigger the $event parameter wrapper: {result}"
    );
    // A plain inline expression with no real $event → wrapped as () => { … }.
    assert!(
        result.contains("onClick={() => {"),
        "inline expression handler should use the () => {{ }} wrapper: {result}"
    );
}

#[test]
fn event_handler_spread_with_event_param_emits_typed_payload() {
    // A duplicate `@click` routes the SECOND handler through the spread path (the
    // first stays an in-place `onClick={…}`). The spread `$event` must be an
    // explicitly-typed parameter — JSX contextual typing does not flow through a
    // spread attribute, so the old generic `eventCallbacks<TArgs extends Array<any>>`
    // wrapper left it `any`. For a native element the annotation is the ambient DOM
    // event-map type keyed by the event name (`click`), which resolves under every
    // TypeProvider (unlike the `import('vue')` formula).
    let result = gen_tsx_template_with_bindings(
        r#"<template><button @click="a($event)" @click="b($event)" /></template>"#,
        &[
            ("a", BindingType::SetupConst),
            ("b", BindingType::SetupConst),
        ],
    );
    // Positive: the spread branch binds `$event` as a typed parameter (the colon is
    // exclusive to the explicitly-annotated spread param; the in-place handler emits
    // a bare `($event) =>`).
    assert!(
        result.contains("($event:"),
        "spread $event must be an explicitly-typed parameter: {result}"
    );
    assert!(
        result.contains(r#"(GlobalEventHandlersEventMap & { [___VERTER___EventKey: string]: Event })["click"]"#),
        "spread $event type must be the ambient DOM event-map type keyed by the event name: {result}"
    );
    // Negative: the `import('vue')` indexed formula is NOT used for native spread
    // `$event` — it does not resolve under the tsgo TypeProvider.
    assert!(
        !result.contains("IntrinsicElementAttributes"),
        "native spread $event must not use the import('vue') formula: {result}"
    );
    // Negative: the generic eventCallbacks helper / rest-args are gone.
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "spread event with $event must NOT use the eventCallbacks wrapper: {result}"
    );
    assert!(
        !result.contains("...___VERTER___eventArgs"),
        "spread event must NOT use the generic event-args rest param: {result}"
    );
}

#[test]
fn v_if_v_for_solo_lifts_condition_before_normal_sibling() {
    // Same as the solo case, but followed by a normal sibling. This still
    // needs the lifted `cond ? map(...) : null` shape.
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in list">{{ item }}</div><p>after</p></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_solo_lifts_condition_before_normal_sibling ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should keep the lifted condition even with a following sibling: {result}"
    );
    assert!(
        result.contains(": null"),
        "solo lifted branch should still fall back to null before the next sibling: {result}"
    );
    assert!(
        result.contains("<p"),
        "following sibling should remain present: {result}"
    );
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in list">{{ item }}</div><p>after</p></template>"#,
        "solo v-if+v-for before normal sibling",
    );
}

#[test]
fn vmodel_does_not_emit_single_overwritten_chunk() {
    // The chunk list must contain NO Overwritten chunk spanning both the synthetic
    // prefix and a user identifier. Asserted via the map: the prop-start generated
    // column must NOT carry the identifier's source mapping (a single
    // overwrite(prop.start, prop_end, "value={count}...") would map the whole run
    // — including `value={` — back to prop.start).
    let source = r#"<template><input v-model="count"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    let count_src = source.find("\"count\"").unwrap() as u32 + 1;
    let prop_start = source.find("v-model").unwrap() as u32;

    // No token may map a generated position back to the prop start.
    let any_prop_start = tokens.iter().any(|&(_, _, sc)| sc == prop_start);
    assert!(
        !any_prop_start,
        "no generated token may map back to the v-model prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );

    // The generated `value={` prefix must NOT carry count's mapping.
    let value_gen = output.find("value={").unwrap();
    let (vl, vc) = gen_offset_to_line_col(&output, value_gen);
    let value_carries_count = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == vl && dc == vc && sc == count_src);
    assert!(
        !value_carries_count,
        "the `value={{` synthetic prefix (gen {vl}:{vc}) must NOT carry count's mapping. \
         Tokens: {tokens:?}"
    );
}

#[test]
fn emit_codegen_crlf_and_tabs() {
    // P2-B: a CRLF, tab-indented fixture still maps identifiers exactly.
    let source = "<template>\r\n\t<div v-html=\"msg\" />\r\n</template>";
    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    assert!(
        output.contains("innerHTML={msg}"),
        "CRLF/tab fixture should still emit innerHTML={{msg}}: {output:?}"
    );

    // `msg` source position: byte offset of the `m` in "msg" (the file has CRLF
    // and a leading tab, so compute the absolute byte offset directly).
    let msg_src = source.find("\"msg\"").unwrap() as u32 + 1;
    // The token's src_col is the column on its source LINE; `msg` is on line 1
    // (0-based), so match by src_col == column within that line.
    let line_start = source[..msg_src as usize]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0) as u32;
    let msg_src_col = msg_src - line_start;
    let has_msg = tokens.iter().any(|&(_, _, sc)| sc == msg_src_col);
    assert!(
        has_msg,
        "msg must map to source col {msg_src_col} (line-relative) even with CRLF/tabs. \
         Tokens: {tokens:?}, output: {output:?}"
    );
}

#[test]
fn v_on_object_spread_handler_maps_to_source() {
    // <div v-on="{ mousedown: doThis }"/> → {...{ mousedown: doThis }}.
    // The handler identifier `doThis` is a navigable user expression — it MUST map
    // back to its source span. The object punctuation (`{...{`, `: `, `}}`) and the
    // event key map to None.
    let source = r#"<template><div v-on="{ mousedown: doThis }"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("doThis", BindingType::SetupConst)]);

    assert!(
        output.contains("{...{"),
        "v-on object literal should emit a spread `{{...{{ ... }}}}`: {output}"
    );
    assert!(
        !output.contains("v-on"),
        "v-on directive must be removed: {output}"
    );

    // Positive: `doThis` maps to its source byte offset.
    let handler_src = source.find("doThis").unwrap() as u32;
    assert!(
        has_token_for_src(&tokens, handler_src),
        "v-on handler `doThis` must map to source col {handler_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `{...{` spread boundary start maps to None (the old baked
    // overwrite mapped the whole run — including the handler — back to prop.start).
    let boundary_gen = output.find("{...{").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "{{...{{ spread boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the v-on prop start (the desync).
    let prop_start = source.find("v-on").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the v-on prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_on_dynamic_event_name_expr_maps() {
    // <div @[event]="handler"/> → {...{[`on${event}` as any]: handler}}.
    // BOTH the dynamic event-name expression `event` and the handler `handler` are
    // navigable user expressions — each must map back to its source span. The
    // computed-key template literal and object punctuation map to None.
    let source = r#"<template><div @[event]="handler"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("event", BindingType::SetupConst),
            ("handler", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("as any]:"),
        "dynamic event name should emit the computed-key spread `[`on${{...}}` as any]: ...`: {output}"
    );
    assert!(
        !output.contains("@["),
        "dynamic event syntax must be removed: {output}"
    );

    // Positive: both `event` (arg) and `handler` (value) map back to source.
    let event_src = source.find("[event]").unwrap() as u32 + 1; // inside the [ ]
    let handler_src = source.find("\"handler\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, event_src),
        "dynamic event-name expr `event` must map to source col {event_src}. \
         Tokens: {tokens:?}, output: {output}"
    );
    assert!(
        has_token_for_src(&tokens, handler_src),
        "dynamic event handler `handler` must map to source col {handler_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `{...{[` boundary start maps to None.
    let boundary_gen = output.find("{...{[").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "{{...{{[ boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the prop start (the desync).
    let prop_start = source.find('@').unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the @[event] prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_show_condition_maps_to_source() {
    // <div v-show="visible"/> → style={{display: visible ? undefined : 'none'}}.
    // The condition `visible` is a navigable user expression relocated into the
    // synthetic style attribute; it MUST map back. The `style={{display: ` prefix
    // and ` ? undefined : 'none'}}` suffix map to None.
    let source = r#"<template><div v-show="visible"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("visible", BindingType::SetupConst)]);

    assert!(
        output.contains("display:"),
        "v-show should emit a display style: {output}"
    );
    assert!(
        !output.contains("v-show"),
        "v-show directive must be removed: {output}"
    );

    // Positive: `visible` maps to its source byte offset.
    let visible_src = source.find("\"visible\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, visible_src),
        "v-show condition `visible` must map to source col {visible_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `style={{display: ` boundary start maps to None (the old baked
    // overwrite mapped the whole run — including `visible` — back to prop.start).
    let boundary_gen = output.find("style={{display:").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "style={{{{display: boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the v-show prop start.
    let prop_start = source.find("v-show").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the v-show prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_if_guarded_inline_handler_maps_to_source() {
    // <div v-if="ok" @click="count++"/> — an inline v-on handler under a v-if.
    // The re-narrowing guard `if (!___VERTER___flowNarrow(count, …)) throw 0; `
    // opens the wrapped handler body. The user body `count++` must map back to source; the `() => {`
    // wrapper and the guard map to None. (The von.rs handler emission already
    // preserves the body in place via the prefix/suffix boundary split; this test
    // pins that invariant so a regression that bakes the body would be caught.)
    let source = r#"<template><div v-if="ok" @click="count++"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("ok", BindingType::SetupConst),
            ("count", BindingType::SetupRef),
        ],
    );

    assert!(
        output.contains("() => {if (!___VERTER___flowNarrow(count, ___VERTER___o0)"),
        "inline handler under v-if must get the re-narrowing guard: {output}"
    );
    assert!(
        output.contains("() => {"),
        "inline handler must be wrapped in an arrow body: {output}"
    );

    // Positive: the user body identifier `count` maps back to its OWN source byte
    // offset, AND the generated `count` token sits at the generated body position
    // (preserved in place, not relocated to a foreign anchor).
    let count_src = source.find("count++").unwrap() as u32;
    let count_gen = output.find("count++").unwrap() as u32;
    let (cgl, cgc) = gen_offset_to_line_col(&output, count_gen as usize);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == cgl && dc == cgc && sc == count_src),
        "the guarded inline handler body `count` (gen {cgl}:{cgc}) must map to its own source \
         col {count_src}. Tokens: {:?}, output: {output}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );

    // Negative: the `() => {` arrow wrapper start maps to None.
    let wrapper_gen = output.find("() => {").unwrap();
    let (wl, wc) = gen_offset_to_line_col(&output, wrapper_gen);
    assert!(
        !has_token_at_gen(&tokens, wl, wc),
        "the `() => {{` arrow wrapper (gen {wl}:{wc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: the generated body identifier `count` must NOT map to the @click prop
    // start (the desync anchor). (The synthetic `onClick` prop NAME legitimately maps
    // near the event arg — this assertion targets the BODY identifier specifically.)
    let prop_start = source.find("@click").unwrap() as u32;
    assert!(
        !tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == cgl && dc == cgc && sc == prop_start),
        "the body identifier `count` (gen {cgl}:{cgc}) must not map to the @click prop start \
         (col {prop_start}). Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn migrated_sites_binding_notation_characterization() {
    // P3 characterization — pin the prop-accessor notation the migrated relocated
    // emitters produce, so it is INTENTIONAL, not accidental:
    //
    // 1. A keyword-named prop accessed as a bindingless SIMPLE identifier (e.g.
    //    `v-show="class"`) → BRACKET notation (`__props["class"]`). `emit_relocated_value`
    //    routes a bindingless simple identifier through `resolve_simple_expr`, which
    //    emits the bracket form for keywords (dot notation `__props.class` is valid TS
    //    too, but bracket matches the pre-migration shared-helper behaviour).
    let v_show_kw = gen_tsx_template_with_bindings(
        r#"<template><div v-show="class"/></template>"#,
        &[("class", BindingType::Props)],
    );
    assert!(
        v_show_kw.contains(r#"__props["class"]"#),
        "v-show keyword prop must use bracket notation `__props[\"class\"]`: {v_show_kw}"
    );
    assert!(
        !v_show_kw.contains("__props.class"),
        "v-show keyword prop must NOT use dot notation `__props.class`: {v_show_kw}"
    );

    // 2. A non-keyword prop emitted through the v-on object-spread substrate (where
    //    OXC DOES extract the binding) → DOT notation (`__props.handler`), identical to
    //    the in-place `@click="handler"` form. The migration keeps the two consistent.
    let v_on_obj = gen_tsx_template_with_bindings(
        r#"<template><div v-on="{ click: handler }"/></template>"#,
        &[("handler", BindingType::Props)],
    );
    assert!(
        v_on_obj.contains("__props.handler"),
        "v-on object-spread Props handler must use dot notation `__props.handler`: {v_on_obj}"
    );
    let at_click = gen_tsx_template_with_bindings(
        r#"<template><div @click="handler"/></template>"#,
        &[("handler", BindingType::Props)],
    );
    assert!(
        at_click.contains("__props.handler"),
        "@click Props handler uses dot notation (the spread form must match it): {at_click}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified user-expression emission — discriminating tests.
//
// These pin that every IDE user-expression emission routes through the single
// `plan_user_expr` planner + InPlace/Relocated sinks (and the object-literal layer
// above it): v-on object shorthand emits value-only (no doubled key), non-object
// `v-on="obj"` spreads a mapped expr, relocated ignored locals stay mapped, `:ref`
// values stay navigable, and a redundant native v-model still emits its modifiers.
// Each fails against the pre-unification hand-rolled emission.
// ─────────────────────────────────────────────────────────────────────────────

/// Q1 — `v-on="{ click }"` shorthand object property. The object-literal layer
/// emits the synthetic event key `onClick: `, then the shorthand VALUE in
/// value-only mode → `onClick: __props.click`. Pre-refactor the value was emitted
/// through `emit_relocated_value`, whose `emit_one_occurrence` saw the binding's
/// `is_shorthand=true` flag and re-expanded the key → the broken double
/// `onClick: click: __props.click` (or `onClick: click` when no prefix applies).
#[test]
fn v_on_object_shorthand_emits_value_only() {
    // Props binding → `__props.` prefix so the shorthand re-expansion is visible.
    let source = r#"<template><div v-on="{ click }"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("click", BindingType::Props)]);

    // Positive: exactly the value-only form. The event key `onClick:` is emitted
    // once by the object layer; the value is the resolved `__props.click`.
    assert!(
        output.contains("onClick: __props.click"),
        "v-on shorthand must emit `onClick: __props.click` (value-only): {output}"
    );
    // Negative: NO double key. The pre-refactor bug emitted the key twice.
    assert!(
        !output.contains("onClick: click: "),
        "v-on shorthand must NOT double the key (`onClick: click: __props.click`): {output}"
    );
    assert!(
        !output.contains("click: __props.click: "),
        "v-on shorthand must NOT emit a stray `click:` shorthand expansion: {output}"
    );

    // The VALUE identifier `click` maps back to its source span.
    let click_src = source.find("{ click }").unwrap() as u32 + "{ ".len() as u32;
    assert!(
        has_token_for_src(&tokens, click_src),
        "v-on shorthand value `click` must map to source col {click_src}. Tokens: {tokens:?}, output: {output}"
    );
}

/// Q1 — `v-on="handlers"` where the value is NOT an object literal. The whole
/// expression spreads as `{...<mapped user expr>}` and the identifier maps back to
/// source. Pre-refactor the non-object branch resolved through
/// `rewrite_v_on_object_literal_expr` + a flat UNMAPPED insert, so `handlers` had
/// no source mapping (ctrl+click failed).
#[test]
fn v_on_non_object_expr_spread_maps() {
    let source = r#"<template><div v-on="handlers"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("handlers", BindingType::Props)]);

    assert!(
        output.contains("{...__props.handlers}"),
        "v-on non-object expr must spread the mapped user expr `{{...__props.handlers}}`: {output}"
    );

    // Positive: `handlers` maps to its source span.
    let handlers_src = source.find("\"handlers\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, handlers_src),
        "v-on non-object expr `handlers` must map to source col {handlers_src}. Tokens: {tokens:?}, output: {output}"
    );

    // Negative: nothing maps back to the prop start.
    let prop_start = source.find("v-on").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the v-on prop start (col {prop_start}). Tokens: {tokens:?}, output: {output}"
    );
}

/// Q3 — static `ref="myRef"` → `ref={"myRef"}` is a STRING LITERAL ref (non-navigable).
/// It must be emitted as an UNMAPPED synthetic replacement (delete + unmapped insert),
/// NOT a mapped `out.overwrite` whose `ref={"…"}` run maps back to the prop start.
#[test]
fn static_ref_emits_unmapped_string_literal() {
    let source = r#"<template><div ref="myRef"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    assert!(
        output.contains(r#"ref={"myRef"}"#),
        "static ref should emit ref={{\"myRef\"}}: {output}"
    );

    // The `ref={"…"}` synthetic run is unmapped — no generated token maps back to
    // the `ref` prop start (the string literal carries no navigation).
    let prop_start = source.find("ref=").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "static ref's synthetic `ref={{\"…\"}}` must not map back to the prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

/// Q4 — a v-model whose value/event generation is redundant
/// (`has_explicit_prop && has_explicit_handler`) but which carries MODIFIERS. The
/// modifiers prop MUST still be emitted. Pre-refactor `empty_replacement = true`
/// suppressed the whole emission INCLUDING modifiers — the modifier was silently
/// dropped.
///
/// The redundancy detection is native-element-only, and a native element no longer
/// receives a `modelModifiers` prop at all (see
/// `intrinsic_vmodel_does_not_emit_model_modifiers_prop`), so the surviving
/// invariant is the COMPONENT one: modifiers are published independently of the
/// value/event pieces.
#[test]
fn vmodel_redundant_still_emits_modifiers() {
    let source = r#"<template><MyComp v-model.trim="x" :modelValue="x" @update:modelValue="v => x = v"/></template>"#;
    let (output, _tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("x", BindingType::SetupRef),
            ("MyComp", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("modelModifiers={{"),
        "a component v-model with modifiers MUST emit the modifiers prop: {output}"
    );
    assert!(
        output.contains("trim: true"),
        "the `.trim` modifier must be emitted as `trim: true`: {output}"
    );
}

/// RULING — `modelModifiers` is a COMPONENT prop (the name `defineModel()`
/// synthesizes), not a DOM attribute. Vue's own compiler handles native-element
/// v-model modifiers inside the generated `vModelText`/`vModelDynamic` runtime
/// directive; it never passes a `modelModifiers` prop to an intrinsic element.
/// Emitting one made `<input v-model.number.trim="count">` — valid Vue — fail with
/// `TS2322: Property 'modelModifiers' does not exist on type 'InputHTMLAttributes &
/// ReservedProps'`, a red squiggle on correct code.
///
/// `.number` / `.trim` on a native element are compiler-level modifiers with no TS
/// correlate, so they lower to nothing and stay unmapped — the fail-closed side of
/// the Carrier IDE TS Surface Principle, not a silently wrong prop.
#[test]
fn intrinsic_vmodel_does_not_emit_model_modifiers_prop() {
    let source = r#"<template><input v-model.number.trim="count" /></template>"#;
    let (output, _tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    assert!(
        !output.contains("modelModifiers"),
        "a native element must not receive the component-only `modelModifiers` prop: {output}"
    );
    // The value/event pieces are unaffected by the ruling.
    assert!(
        output.contains("value={"),
        "native v-model must still emit the DOM value prop: {output}"
    );
    assert!(
        output.contains("onInput={"),
        "native v-model must still emit the input handler: {output}"
    );
}

/// The control for the ruling above: a COMPONENT still publishes `modelModifiers`,
/// because that is a real prop on a component using `defineModel`.
#[test]
fn component_vmodel_still_emits_model_modifiers_prop() {
    let source = r#"<template><MyComp v-model.trim="x" /></template>"#;
    let (output, _tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("x", BindingType::SetupRef),
            ("MyComp", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("modelModifiers={{"),
        "a component v-model MUST still publish modelModifiers: {output}"
    );
    assert!(
        output.contains("trim: true"),
        "the `.trim` modifier must be emitted as `trim: true`: {output}"
    );
}

/// Q2 — the broken-interpolation recovery path's keyword-bracket case
/// (`SynthesizedResolved`) routes through the unified synthesized-shorthand emission
/// instead of a baked `out.overwrite(ident.start, ident.end, &resolved)`. `class` is
/// a JS keyword used as a Props member → `__props["class"]`; the `class` core must
/// map to its source token and must NOT collapse the whole bracket form onto the
/// prop start.
#[test]
fn broken_interpolation_keyword_member_maps_via_synthesized() {
    let source = r#"<template><div>{{ class + }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("class", BindingType::Props)]);

    // Keyword member → bracket notation (dot would be a syntax error).
    assert!(
        output.contains(r#"__props["class"]"#),
        "broken-interpolation keyword member must resolve to bracket notation: {output}"
    );

    // The `class` core inside the brackets maps to its source token.
    let class_src = source.find("class").unwrap() as u32;
    let bracket_gen = output.find(r#"__props["class"]"#).unwrap();
    let core_gen = bracket_gen + r#"__props[""#.len();
    let (cl, cc) = gen_offset_to_line_col(&output, core_gen);
    let core_maps = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == cl && dc == cc && sc == class_src);
    assert!(
        core_maps,
        "the `class` core (gen {cl}:{cc}) must map to source col {class_src}. Tokens: {tokens:?}, output: {output}"
    );

    // The `__props["` prefix start must be unmapped (a baked overwrite would map it
    // back to the identifier/prop position).
    let (pl, pc) = gen_offset_to_line_col(&output, bracket_gen);
    assert!(
        !has_token_at_gen(&tokens, pl, pc),
        "the `__props[\"` prefix start (gen {pl}:{pc}) must map to None. Tokens: {tokens:?}, output: {output}"
    );
}

/// The v-on SPREAD path (hyphenated event name → `{...{"onMy-event": …}}`) keeps
/// the event NAME navigable: the `onMy-event` string key maps to the source event
/// token so hover / go-to-definition on a component `@my-event` resolves the child's
/// `onMyEvent` payload. The handler value also maps. Byte-identical output to the
/// pre-unification baked spread (only the source map gains the event-name token).
#[test]
fn v_on_spread_event_name_and_handler_map_to_source() {
    let source = r#"<template><MyComp @my-event="handler"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("handler", BindingType::SetupConst)]);

    // Hyphenated JSX event name forces the spread form.
    assert!(
        output.contains(r#"{...{"onMy-event": handler}}"#),
        "hyphenated @my-event must spread as {{...{{\"onMy-event\": handler}}}}: {output}"
    );

    // The event-name key maps to the source `my-event` token.
    let event_src = source.find("my-event").unwrap() as u32;
    let key_gen = output.find(r#""onMy-event""#).unwrap() + 1; // past the opening quote
    let (kl, kc) = gen_offset_to_line_col(&output, key_gen);
    assert!(
        tokens.iter().any(|&(dl, dc, sc)| dl == kl && dc == kc && sc == event_src),
        "the `onMy-event` key (gen {kl}:{kc}) must map to the source event token col {event_src}. Tokens: {tokens:?}, output: {output}"
    );

    // The handler value maps to its source span.
    let handler_src = source.find("\"handler\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, handler_src),
        "spread handler `handler` must map to source col {handler_src}. Tokens: {tokens:?}, output: {output}"
    );
}

/// Q1 — atomicity / unification sanity: the v-on object-spread handler still maps
/// to source through the unified planner (kept green across the refactor).
#[test]
fn v_on_object_spread_handler_still_maps_after_unify() {
    let source = r#"<template><div v-on="{ mousedown: doThis }"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("doThis", BindingType::SetupConst)]);
    let handler_src = source.find("doThis").unwrap() as u32;
    assert!(
        has_token_for_src(&tokens, handler_src),
        "v-on object handler `doThis` must still map after unify. Tokens: {tokens:?}, output: {output}"
    );
}

/// An inline v-on handler under a v-if. The handler boundary is decomposed
/// through the typed `EmitOp` substrate (never one mapped overwrite from the
/// `@click` prop start, which would map the whole generated run to a foreign
/// anchor):
///   - the synthetic JSX wrapper (`onClick={`, `() => {`) and the re-narrowing
///     guard map to None. The guard is compiler-synthesized and has no source
///     span (consistent with the sibling `process_v_bind` guarded-value path).
///   - the event NAME `onClick` maps to the SOURCE event token (`click` arg), NOT
///     the `@click` prop start.
///   - the handler BODY `count++` stays in place, mapped to its own source span.
#[test]
fn v_if_guarded_inline_handler_guard_maps_to_none() {
    let source = r#"<template><button @click="count++" v-if="ready">x</button></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("ready", BindingType::SetupConst),
            ("count", BindingType::SetupConst),
        ],
    );

    // The re-narrowing guard is emitted.
    assert!(
        output.contains("throw 0; count++"),
        "inline handler under v-if must get the re-narrowing guard: {output}"
    );
    assert!(
        output.contains("onClick={() => {"),
        "inline handler must emit the onClick arrow wrapper: {output}"
    );

    // Positive: the handler BODY identifier `count` maps to its OWN source span, at
    // the generated body position (preserved in place, not a foreign anchor).
    let count_src = source.find("count++").unwrap() as u32;
    let count_gen = output.find("count++").unwrap();
    let (cgl, cgc) = gen_offset_to_line_col(&output, count_gen);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == cgl && dc == cgc && sc == count_src),
        "guarded inline handler body `count` (gen {cgl}:{cgc}) must map to its own source col \
         {count_src}. Tokens: {:?}, output: {output}",
        tokens.iter().map(|t| (t.0, t.1, t.2)).collect::<Vec<_>>()
    );

    // Positive: the event NAME `onClick` maps to the SOURCE event arg token (`click`),
    // exactly like the v-on spread branch maps `arg_start`.
    let event_arg_src = source.find("click").unwrap() as u32; // `click` arg of `@click`
    let onclick_gen = output.find("onClick={").unwrap();
    let (ekl, ekc) = gen_offset_to_line_col(&output, onclick_gen);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == ekl && dc == ekc && sc == event_arg_src),
        "event name `onClick` (gen {ekl}:{ekc}) must map to the source event token col \
         {event_arg_src}. Tokens: {tokens:?}, output: {output}"
    );

    // Negative (the desync): NO generated token may map back to the `@click` prop
    // start (the desync anchor). Pre-fix the baked `boundary_prefix` overwrite mapped
    // the whole `onClick={() => {…guard…} ` run to the prop start.
    let prop_start = source.find("@click").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the @click prop start (col {prop_start}) — the \
         baked-boundary desync. Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the injected guard text maps to None. The re-narrowing scaffold
    // is compiler-synthesized → unmapped.
    let guard_gen = output.find("if (!___VERTER___flowNarrow(count").unwrap();
    let (gl, gc) = gen_offset_to_line_col(&output, guard_gen);
    assert!(
        !has_token_at_gen(&tokens, gl, gc),
        "the injected guard (gen {gl}:{gc}) must map to None. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `() => {` arrow wrapper start maps to None.
    let wrapper_gen = output.find("() => {").unwrap();
    let (wl, wc) = gen_offset_to_line_col(&output, wrapper_gen);
    assert!(
        !has_token_at_gen(&tokens, wl, wc),
        "the `() => {{` arrow wrapper (gen {wl}:{wc}) must map to None. Tokens: {tokens:?}"
    );
}

/// IDE generation always runs in TSX mode, where `resolve_suffix` returns `""`.
/// A setup ref iterable therefore never gains a `.value`: `v-for="todo in todos"`
/// resolves the iterable to bare `(todos)`. End-state invariant guarding the
/// producer routing in production (output bytes are invariant by design).
#[test]
fn ide_v_for_iterable_tsx_setup_ref_never_emits_value_suffix() {
    let source = r#"<template><div v-for="todo in todos">{{ todo }}</div></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("todos", BindingType::SetupRef)]);
    assert!(
        output.contains("const ___VERTER___v0 = (todos);"),
        "TSX v-for iterable must read `(todos)`: {output}"
    );
    assert!(
        !output.contains("todos.value"),
        "TSX mode must not emit a `.value` suffix on the iterable: {output}"
    );
}

/// Structural guard — the standalone mapped resolver-prefixed expression heads
/// (IDE `v-for` iterable, dynamic `:is`) route through the shared segmented
/// producer (`build_prefixed_expr_segments` / `resolve_simple_expr_segments` →
/// `prepend_mapped_generated_text`). No standalone emitter may reintroduce the
/// per-identifier fold, keep an independent flat references mapper, or fold a
/// resolved resolver-prefixed expression into one mapped `:is` content chunk.
#[test]
fn standalone_mapped_emitters_carry_no_resolver_prefix_fold() {
    let directives_src = include_str!("../directives.rs");
    let mod_src = include_str!("../mod.rs");

    // (1) The v-for per-identifier `prefix+gap+bind_prefix+name+bind_suffix` fold
    //     must be gone — the iterable resolves through the segmented producer.
    assert!(
        !directives_src.contains(concat!(
            "format!(\"{}{}{}{}{}\", ",
            "prefix, gap, bind_prefix, name, bind_suffix)"
        )),
        "IDE v-for iterable must not fold prefix+gap+bind_prefix+name+bind_suffix into one mapped \
         chunk; route through build_prefixed_expr_segments / resolve_simple_expr_segments instead"
    );

    // (2) The independent flat references-walking resolver-prefix/suffix mapper
    //     must be deleted, not kept alongside the producer.
    assert!(
        !directives_src.contains("fn resolve_v_for_iterable"),
        "resolve_v_for_iterable (the independent resolver-prefix/suffix mapper) must be deleted; \
         the v-for iterable resolves through the shared segmented producer"
    );

    // (3) The dynamic `:is` resolved expression must not fold into one mapped
    //     content chunk; it routes through the producer + wrapped().
    assert!(
        !mod_src.contains(concat!("iife_prefix, resolved_expr, ", "ts_comment_text")),
        "dynamic :is must not fold iife_prefix + resolved_expr into one mapped chunk; route the \
         resolved expression through the shared segmented producer"
    );

    // Positive: both emitters lower through the single segmented carrier.
    assert!(
        directives_src.contains("prepend_mapped_generated_text"),
        "IDE v-for iterable must lower via prepend_mapped_generated_text"
    );
    assert!(
        mod_src.contains("prepend_mapped_generated_text"),
        "dynamic :is must lower via prepend_mapped_generated_text"
    );
}

// ── Spread-path event-typing closed matrix ────────────────────────────────
//
// The full spread-event typing matrix: {native, local component, global
// component} × {$event inline, arrow/function param} × {duplicate event key, hyphenated
// event key}. Native hyphenated rows are covered above by `kebab_event_*`. Every spread
// surface types its handler from the typed-IR — native via the ambient DOM payload,
// components via the shared `InstanceType<typeof Binding>["$props"]` inventory (local
// binding OR GlobalComponents fallback const) — never `$event: any`, never the retired
// `eventCallbacks` helper, never `import('vue').GlobalComponents[...]`.
mod spread_event_typing_matrix {
    use super::super::*;

    /// The component event-handler payload tuple for `Binding` and JSX event prop `onX`.
    fn component_params_tuple(binding: &str, on_event: &str) -> String {
        format!(
            r#"Parameters<NonNullable<Required<InstanceType<typeof {binding}>["$props"]>["{on_event}"]>>"#
        )
    }

    fn native_payload(event: &str) -> String {
        format!(
            r#"(GlobalEventHandlersEventMap & {{ [___VERTER___EventKey: string]: Event }})["{event}"]"#
        )
    }

    fn assert_no_untyped_leaks(result: &str) {
        assert!(
            !result.contains("$event: any"),
            "spread $event must be precisely typed, never `$event: any`: {result}"
        );
        assert!(
            !result.contains("___VERTER___eventCallbacks"),
            "retired generic eventCallbacks helper must be absent: {result}"
        );
    }

    // ── Native element ────────────────────────────────────────────────────

    #[test]
    fn duplicate_native_dollar_event_ambient_payload() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div @click="handle($event.clientY)" @click="handle($event.screenX)" /></template>"#,
            &[("handle", BindingType::SetupConst)],
        );
        // The SECOND @click routes through the spread path (duplicate key).
        assert!(
            result.contains(&format!(
                r#""onClick": ($event: {}) =>"#,
                native_payload("click")
            )),
            "duplicate native $event must be typed via the ambient DOM payload: {result}"
        );
        assert!(
            !result.contains("IntrinsicElementAttributes"),
            "native spread must not use the import('vue') formula: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_native_arrow_satisfies_ambient_payload() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div @click="(e) => handle(e)" @click="(e) => other(e)" /></template>"#,
            &[
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ((e) => other(e)) satisfies (...___VERTER___eventArgs: [{}]) => unknown"#,
                native_payload("click")
            )),
            "duplicate native arrow must be satisfies-wrapped against the ambient payload tuple: {result}"
        );
        assert!(
            !result.contains("IntrinsicElementAttributes"),
            "native spread arrow must not use the import('vue') formula: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    // ── Local (script-bound) component ────────────────────────────────────

    #[test]
    fn hyphenated_local_component_dollar_event_instance_type() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><LocalComp @my-event="handle($event)" /></template>"#,
            &[
                ("LocalComp", BindingType::SetupConst),
                ("handle", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onMy-event": ($event: {}[0]) =>"#,
                component_params_tuple("LocalComp", "onMy-event")
            )),
            "hyphenated local-component $event must be typed via InstanceType<typeof LocalComp>: {result}"
        );
        assert!(
            !result.contains("GlobalComponents"),
            "local component must not use the GlobalComponents indexed type: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_local_component_dollar_event_instance_type() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><LocalComp @click="handle($event)" @click="other($event)" /></template>"#,
            &[
                ("LocalComp", BindingType::SetupConst),
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ($event: {}[0]) =>"#,
                component_params_tuple("LocalComp", "onClick")
            )),
            "duplicate local-component $event must be typed via InstanceType<typeof LocalComp>: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn hyphenated_local_component_arrow_satisfies_instance_type() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><LocalComp @my-event="(e) => handle(e)" /></template>"#,
            &[
                ("LocalComp", BindingType::SetupConst),
                ("handle", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onMy-event": ((e) => handle(e)) satisfies (...___VERTER___eventArgs: {}) => unknown"#,
                component_params_tuple("LocalComp", "onMy-event")
            )),
            "hyphenated local-component arrow must satisfies-wrap the InstanceType<typeof LocalComp> tuple: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_local_component_arrow_satisfies_instance_type() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><LocalComp @click="(e) => handle(e)" @click="(e) => other(e)" /></template>"#,
            &[
                ("LocalComp", BindingType::SetupConst),
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ((e) => other(e)) satisfies (...___VERTER___eventArgs: {}) => unknown"#,
                component_params_tuple("LocalComp", "onClick")
            )),
            "duplicate local-component arrow must satisfies-wrap the InstanceType<typeof LocalComp> tuple: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    // ── Global (GlobalComponents fallback) component ──────────────────────

    #[test]
    fn hyphenated_global_component_dollar_event_fallback_const() {
        let result = gen_tsx_template_with_components(
            r#"<template><GlobalComp @my-event="handle($event)" /></template>"#,
            &[("handle", BindingType::SetupConst)],
            &["GlobalComp"],
        );
        assert!(
            result.contains(&format!(
                r#""onMy-event": ($event: {}[0]) =>"#,
                component_params_tuple("GlobalComp", "onMy-event")
            )),
            "hyphenated global-component $event must resolve via the fallback const InstanceType<typeof GlobalComp>: {result}"
        );
        // Never the direct GlobalComponents indexed type (tsgo cannot resolve it).
        assert!(
            !result.contains("GlobalComponents"),
            "global component $event must NOT use import('vue').GlobalComponents[...]: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_global_component_dollar_event_fallback_const() {
        let result = gen_tsx_template_with_components(
            r#"<template><GlobalComp @click="handle($event)" @click="other($event)" /></template>"#,
            &[
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
            &["GlobalComp"],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ($event: {}[0]) =>"#,
                component_params_tuple("GlobalComp", "onClick")
            )),
            "duplicate global-component $event must resolve via the fallback const InstanceType<typeof GlobalComp>: {result}"
        );
        assert!(
            !result.contains("GlobalComponents"),
            "global component $event must NOT use import('vue').GlobalComponents[...]: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn hyphenated_global_component_arrow_satisfies_fallback_const() {
        let result = gen_tsx_template_with_components(
            r#"<template><GlobalComp @my-event="(e) => handle(e)" /></template>"#,
            &[("handle", BindingType::SetupConst)],
            &["GlobalComp"],
        );
        assert!(
            result.contains(&format!(
                r#""onMy-event": ((e) => handle(e)) satisfies (...___VERTER___eventArgs: {}) => unknown"#,
                component_params_tuple("GlobalComp", "onMy-event")
            )),
            "hyphenated global-component arrow must satisfies-wrap the fallback-const InstanceType<typeof GlobalComp> tuple: {result}"
        );
        assert!(
            !result.contains("GlobalComponents"),
            "global component arrow must NOT use import('vue').GlobalComponents[...]: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_global_component_arrow_satisfies_fallback_const() {
        let result = gen_tsx_template_with_components(
            r#"<template><GlobalComp @click="(e) => handle(e)" @click="(e) => other(e)" /></template>"#,
            &[
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
            &["GlobalComp"],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ((e) => other(e)) satisfies (...___VERTER___eventArgs: {}) => unknown"#,
                component_params_tuple("GlobalComp", "onClick")
            )),
            "duplicate global-component arrow must satisfies-wrap the fallback-const InstanceType<typeof GlobalComp> tuple: {result}"
        );
        assert!(
            !result.contains("GlobalComponents"),
            "global component arrow must NOT use import('vue').GlobalComponents[...]: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    // ── Unresolved component: explicit `any`, never implicit ──

    #[test]
    fn unresolved_component_dollar_event_explicit_any_not_implicit() {
        // A component with no local binding and no fallback const (not in the inventory).
        let result = gen_tsx_template_with_bindings(
            r#"<template><UnknownComp @click="handle($event)" @click="other($event)" /></template>"#,
            &[
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
        );
        // Explicit `$event: any` (never a bare implicit-any parameter).
        assert!(
            result.contains(r#""onClick": ($event: any) =>"#),
            "unresolved component $event must be EXPLICIT any, never implicit: {result}"
        );
    }
}

// =========================================================================
// Multi-statement v-on handlers (IDE/TSX path)
//
// A `v-on` value is an inline STATEMENT LIST. Two invariants follow:
//   1. EVERY statement is binding-resolved, not just the first.
//   2. A multi-statement value is never a bare handler reference — it must be
//      wrapped in a handler body, or the JSX expression container holds a
//      statement list and the emitted TSX does not parse.
// =========================================================================

#[test]
fn multi_statement_handler_resolves_every_statement_tsx() {
    let output = gen_tsx_template_with_bindings(
        r#"<template><button @click="a = p; zzUnknown = 2">x</button></template>"#,
        &[("p", BindingType::Props), ("a", BindingType::SetupConst)],
    );
    assert!(
        output.contains("__props.p"),
        "first statement must resolve the prop: {output}"
    );
    assert!(
        output.contains("___VERTER___instance.zzUnknown"),
        "an unresolved identifier in the SECOND statement must take the instance prefix: {output}"
    );
    assert!(
        !output.contains("; zzUnknown = 2"),
        "bare unprefixed second statement must not be emitted: {output}"
    );
}

#[test]
fn multi_statement_handler_is_wrapped_not_treated_as_member_expression() {
    // `obj.a = 1; obj.b = 2` resolves to a string containing `.` and no `(`.
    // A text-shaped member-expression probe classifies that as a bare handler
    // reference and emits `onClick={obj.a = 1; obj.b = 2}` — a JSX expression
    // container holding two statements, which does not parse.
    let output = gen_tsx_template_with_bindings(
        r#"<template><button @click="obj.a = 1; obj.b = 2">x</button></template>"#,
        &[("obj", BindingType::SetupConst)],
    );
    assert!(
        output.contains("onClick={() => {"),
        "multi-statement handler must be wrapped in a handler body: {output}"
    );
    assert!(
        !output.contains("onClick={obj.a = 1"),
        "multi-statement handler must not be emitted as a bare handler reference: {output}"
    );
}

#[test]
fn single_statement_member_handler_stays_a_bare_reference() {
    // Control: a genuine member-expression handler keeps the unwrapped shape.
    let output = gen_tsx_template_with_bindings(
        r#"<template><button @click="obj.handler">x</button></template>"#,
        &[("obj", BindingType::SetupConst)],
    );
    assert!(
        output.contains("onClick={obj.handler}"),
        "member-expression handler must stay a bare reference: {output}"
    );
}
