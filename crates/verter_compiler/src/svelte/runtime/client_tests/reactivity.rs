use super::*;

#[test]
fn general_instance_statement_preserves_attached_semantic_comment() {
    const SOURCE: &str = include_str!(
        "../../../../tests/svelte_oracle_corpus/fixtures/matrix/script_instance_general.svelte"
    );
    const EXPECTED: &str = "Comment(class=Legal,text=\"/*! general-license */\",anchor=\"pos=Leading/stmt[3]:ExportDefaultDeclaration/child[0]:Function/child[2]:FunctionBody/child[2]:VariableDeclaration\",ord=0)";

    let js = emit(SOURCE, "script_instance_general.svelte");
    let signature = crate::svelte_semantic_comments::semantic_comment_signature(&js)
        .expect("native client output must remain valid JavaScript");

    assert_eq!(signature, [EXPECTED]);
}
#[test]
fn reactive_mixed_text_run_without_entities_is_unchanged() {
    // NEGATIVE / no-regression: an entity-FREE reactive mixed run is emitted exactly
    // as before the decode was added — the decode is a no-op on text with no `&`.
    let src =
        "<script>let name = $state('x');</script>\n<button onclick={() => name = 'y'}>Hi {name}!</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, `Hi ${$.get(name) ?? ''}!`)"),
        "an entity-free reactive run is unchanged by the decode:\n{js}"
    );
}

#[test]
fn comment_after_interp_does_not_truncate_the_reactive_text_run() {
    // A `<!--x-->` comment between an interpolation and trailing static text must
    // NOT break the run: `clean_nodes` DROPS comments, so `a {c}<!--x--> b` is one
    // text run. Official svelte@5.56.10 emits `\`a ${$.get(c) ?? ''} b\``. RED
    // pre-fix: `owning_text_run` reconstructed the run from RAW children and treated
    // the comment as a run break, dropping the trailing static " b".
    let src = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>a {c}<!--x--> b</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, `a ${$.get(c) ?? ''} b`)"),
        "a comment must not truncate the run — the trailing ` b` stays:\n{js}"
    );
    // NEGATIVE: the truncated (comment-broke-the-run) form must be absent.
    assert!(
        !js.contains("$.set_text(text, `a ${$.get(c) ?? ''}`)"),
        "the run must NOT stop at the comment (trailing static dropped):\n{js}"
    );
}

#[test]
fn comment_before_interp_does_not_truncate_the_reactive_text_run() {
    // A leading `<!--x-->` is dropped; the run is `a {c}`. Official emits
    // `\`a ${$.get(c) ?? ''}\``.
    let src =
        "<script>let c = $state(0);</script>\n<button onclick={() => c++}><!--x-->a {c}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, `a ${$.get(c) ?? ''}`)"),
        "a leading comment is dropped; the run is `a {{c}}`:\n{js}"
    );
}

#[test]
fn multiple_comments_do_not_truncate_the_reactive_text_run() {
    // `a {c}<!--x--><!--y--> b` — both comments are dropped; the run is `a {c} b`.
    // Official emits `\`a ${$.get(c) ?? ''} b\``.
    let src = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>a {c}<!--x--><!--y--> b</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, `a ${$.get(c) ?? ''} b`)"),
        "multiple comments are all dropped; the trailing ` b` stays:\n{js}"
    );
}

#[test]
fn real_element_after_comment_still_breaks_the_reactive_text_run() {
    // `a {c}<!--x--><div></div> b` — the comment is dropped, but the REAL `<div>`
    // sibling still breaks the run, so the run is just `a {c}` and `<div></div> b`
    // becomes skeleton. Official emits `\`a ${$.get(c) ?? ''}\`` and the template
    // `<button> <div></div> b</button>`. Discriminates a fix that drops EVERY
    // non-text sibling (it must keep dropping comments but still break on elements).
    // `<div>` is in the client allowlist; `<span>` is not (it would fail-close).
    let src = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>a {c}<!--x--><div></div> b</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, `a ${$.get(c) ?? ''}`)"),
        "a real element still breaks the run after a dropped comment:\n{js}"
    );
    // NEGATIVE: the run must NOT swallow the trailing ` b` past the `<div>`.
    assert!(
        !js.contains("?? ''} b`"),
        "the run must stop at the real element, not absorb ` b`:\n{js}"
    );
    // The skeleton carries the `<div></div> b` after the text node.
    assert!(
        js.contains("`<button> <div></div> b</button>`"),
        "skeleton keeps the element + trailing static after the run:\n{js}"
    );
}
#[test]
fn primitive_state_reassigned_to_proxiable_rhs_gets_trailing_true() {
    // F9: a PRIMITIVE `$state(0)` (a `$.state` signal, NOT a StateProxy) reassigned
    // to a PROXIABLE RHS (`{ a: 1 }`) carries the trailing `, true` —
    // `$.set(o, { a: 1 }, true)`. Verified against svelte@5.56.10 (the gate is
    // `should_proxy(rhs)`, NOT `is_state_proxy(binding)`). RED against the
    // binding-keyed gate (which only added `, true` for a StateProxy binding).
    let src =
        "<script>let o = $state(0);</script>\n<button onclick={() => o = { a: 1 }}>{o}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let o = $.state(0);"),
        "a primitive init is a bare signal:\n{js}"
    );
    assert!(
        js.contains("$.delegated('click', button, () => $.set(o, { a: 1 }, true))"),
        "a proxiable RHS reassign carries the trailing true:\n{js}"
    );
}

#[test]
fn debug_tag_identifier_arguments_emit_snapshot_effect() {
    // The POSITIVE shape: bare-identifier arguments emit the reactive snapshot log
    // `$.template_effect(() => {console.log({ a: $.snapshot(...), b: $.snapshot(...) }); debugger;})`.
    let js = emit(
        "<script>let a = $state(0); let b = $state(0);</script>\n{@debug a, b}\n<button onclick={() => a++}>x</button>\n<button onclick={() => b++}>y</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("console.log({a: $.snapshot(") && js.contains("b: $.snapshot("),
        "identifier debug arguments emit one snapshot entry each:\n{js}"
    );
    assert!(
        js.contains("debugger;"),
        "the debug effect carries the `debugger;` statement:\n{js}"
    );
}

#[test]
fn block_object_state_declarator_fails_closed() {
    // A block `{let o = $state({})}` declarator carries an OBJECT (proxy) `$state` — the
    // deep-reactive proxy form is a deferred surface, so it fails closed as an advanced
    // rune rather than mis-emitting the literal `$state({})` call (which references the
    // un-imported `$state`).
    assert_fail_closed(
        "<script>let { items } = $props();</script>\n{#each items as item}{let o = $state({})}<button onclick={() => o.k++}>x</button>{/each}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }),
    );
}

#[test]
fn element_spread_emits_the_attribute_effect_fold() {
    // An element spread `{...props}` (a free-identifier payload) emits the single
    // `$.attribute_effect(el, () => ({ ...props }))` fold — NOT a refusal, NOT a
    // per-attribute path. The unused `$state` marker forces runes mode (a no-script
    // component compiles legacy).
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div {...props}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.attribute_effect(div, () => ({ ...props }))")),
        "an element spread must emit the attribute_effect fold:\n{js}"
    );
    // NEGATIVE: the element gets NO separate $.set_attribute and the deleted diagnostic
    // is absent.
    assert!(
        !js.contains("$.set_attribute"),
        "a spread element must NOT emit a separate $.set_attribute:\n{js}"
    );
    assert!(
        !js.contains("svelte-runtime-unsupported-spread-or-html"),
        "the deleted spread-or-html refusal must not surface:\n{js}"
    );
}

#[test]
fn spread_and_html_compose_attribute_effect_then_html_then_reset() {
    // A spread + `{@html}` on the same element compose: `$.attribute_effect` (attrs)
    // first, then `$.html(div, () => h, true)` (children), then `$.reset(div)`.
    let js = emit(
        "<script>let h = $state(\"\");</script>\n<div {...props}>{@html h}</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    let effect = n
        .find(&nc("$.attribute_effect(div, () => ({ ...props }))"))
        .expect("the attribute_effect fold must be present");
    let html = n
        .find(&nc("$.html(div, () => h, true)"))
        .expect("the html op must be present");
    let reset = n
        .find(&nc("$.reset(div)"))
        .expect("the reset must be present");
    assert!(
        effect < html && html < reset,
        "the compose order must be attribute_effect → html → reset:\n{js}"
    );
}

#[test]
fn props_rest_element_spread_lowers_to_attribute_effect() {
    // REPLACED (was `props_rest_spread_still_refuses_as_advanced_rune_not_the_deleted_
    // spread_surface`, which pinned the now-SUPPORTED rest surface): a `{...rest}`
    // whose `rest` is a `$props()` REST capture (`let { a, ...rest } = $props()`) is
    // the element-spread `$.attribute_effect(div, () => ({ ...rest }))` fold, with
    // the hoisted `rest_excludes` Set (prefix + `a`). The bare `rest` flows through
    // the spread verbatim (a real local), and an element SPREAD opens NO context
    // frame. Verified against svelte@5.56.10.
    let js = emit(
        "<script>let { a, ...rest } = $props()</script>\n<div {...rest}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("var rest_excludes = new Set(['$$slots', '$$events', '$$legacy', 'a']);"),
        "missing the hoisted rest_excludes Set:\n{js}"
    );
    assert!(
        js.contains("$.attribute_effect(div, () => ({ ...rest }))"),
        "missing the element-spread attribute_effect fold over the verbatim rest:\n{js}"
    );
    // NEGATIVE: the spread payload is the real local `rest`, never `$$props.rest`,
    // and a bare-spread read opens NO context frame.
    assert!(
        !js.contains("$$props.rest"),
        "the spread payload must be the real local `rest`:\n{js}"
    );
    assert!(
        !js.contains("$.push($$props, true)"),
        "an element spread must NOT open the component context frame:\n{js}"
    );
}

#[test]
fn component_spread_emits_spread_props_not_attribute_effect() {
    // A component spread `<Foo {...rest}>` is the component surface — it emits
    // `$.spread_props(() => $$props.rest)`, NOT the element-spread `$.attribute_effect`
    // fold (the two spread surfaces are distinct and must not leak into each other).
    let js = emit_result(
        "<script>import Foo from './Foo.svelte'; let { rest } = $props();</script>\n<Foo {...rest} />\n",
    )
    .expect("a component spread emits a module");
    assert!(
        js.contains("$.spread_props(() => $$props.rest)"),
        "missing the component $.spread_props call:\n{js}"
    );
    // NEGATIVE: a component spread is NOT the element `$.attribute_effect` fold.
    assert!(
        !js.contains("$.attribute_effect"),
        "a component spread must NOT emit the element $.attribute_effect fold:\n{js}"
    );
}

#[test]
fn duplicate_same_source_imports_stay_two_statements() {
    // Official does NOT merge two imports from the same source — the prelude keeps
    // TWO statements in source order.
    let js = emit(
        "<script>import { a } from './m.js'; import { b } from './m.js'; let c = $state(0);</script>\n<p>{a} {b}</p>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("import { a } from './m.js';") && js.contains("import { b } from './m.js';"),
        "both same-source imports must emit:\n{js}"
    );
    // NEGATIVE: never merged into one statement.
    assert!(
        !js.contains("import { a, b }"),
        "same-source imports must stay UNMERGED (two statements):\n{js}"
    );
}

#[test]
fn dynamic_attr_from_import_joins_the_template_effect() {
    // `disabled={x}` from an import is REACTIVE (imports are live bindings): the
    // property write joins the `$.template_effect`, read plain — never a one-shot
    // init, never `$.get` (oracle-verified against svelte@5.56.10).
    let js = emit(
        "<script>import { x } from './m.js'; let c = $state(0);</script>\n<button disabled={x} onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("button.disabled = x"),
        "the import-valued property write must read plain:\n{js}"
    );
    let effect_at = js.find("$.template_effect").unwrap_or(usize::MAX);
    let write_at = js.find("button.disabled = x").unwrap_or(0);
    assert!(
        effect_at < write_at,
        "the import-valued property write must join the template effect:\n{js}"
    );
    assert!(
        !js.contains("$.get(x)"),
        "an import read must stay plain (never `$.get`):\n{js}"
    );
}

#[test]
fn component_let_directive_emits_a_slot_prop_derived() {
    // A COMPONENT `let:item` is the slot-prop surface: the default slot becomes a
    // `$$slots.default` callback prepending `const item = $.derived(() => $$slotProps.item)`,
    // and `children` becomes the `$.invalid_default_snippet` sentinel.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child let:item>{item}</Child>\n",
    )
    .expect("a component let: emits a module");
    assert!(
        js.contains("const item = $.derived(() => $$slotProps.item)"),
        "missing the let: slot-prop derived:\n{js}"
    );
    assert!(
        js.contains("children: $.invalid_default_snippet"),
        "missing the invalid_default_snippet sentinel:\n{js}"
    );
}

#[test]
fn component_let_alias_directive_emits_an_aliased_slot_prop_derived() {
    // An ALIASED component `let:item={value}` renames the slot prop `item` to the local
    // `value`: the default-slot callback prepends `const value = $.derived(() =>
    // $$slotProps.item)` (key `item`, local `value`), and a read `{value}` resolves to it.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child let:item={value}>{value}</Child>\n",
    )
    .expect("an aliased component let: emits a module");
    assert!(
        js.contains("const value = $.derived(() => $$slotProps.item)"),
        "missing the aliased let: slot-prop derived (local `value`, key `item`):\n{js}"
    );
    // NEGATIVE: the local is the ALIAS `value`, NOT the slot-prop key `item`.
    assert!(
        !js.contains("const item = $.derived(() => $$slotProps.item)"),
        "the aliased let: must bind the local `value`, not the key `item`:\n{js}"
    );
}

#[test]
fn svelte_element_lone_static_class_emits_set_class_not_attribute_effect() {
    // F7: the official `SvelteElement` LONE-static-class fast path — a `<svelte:element>` whose
    // ONLY plain attribute is a static-text `class` emits `$.set_class($$element, 0, '<cls>')`
    // (the `is_html` false ⇒ `0` flags arg), NOT the `$.attribute_effect` fold. Hardens the
    // fast-path routing beyond the corpus golden.
    let js = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} class=\"card\">hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class($$element, 0, 'card')")),
        "a lone static class takes the set_class fast path:\n{js}"
    );
    // NEGATIVE: the lone static class does NOT route through the attribute_effect fold.
    assert!(
        !js.contains("attribute_effect"),
        "a lone static class must NOT emit an attribute_effect fold:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");

    // INVERSE (non-lone): a SECOND plain attribute (`id="x"`) disqualifies the fast path — the
    // class folds into `$.attribute_effect` as a `class: '…'` entry, and NO lone `$.set_class`
    // is emitted.
    let js2 = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} class=\"card\" id=\"x\">hi</svelte:element>\n",
        "App.svelte",
    );
    let n2 = normalize_js_cosmetics(&js2);
    assert!(
        js2.contains("attribute_effect") && n2.contains(&nc("class: 'card'")),
        "a non-lone class folds into the attribute_effect:\n{js2}"
    );
    assert!(
        !n2.contains(&nc("$.set_class($$element, 0, 'card')")),
        "the non-lone case must NOT take the lone set_class fast path:\n{js2}"
    );
    assert!(parses_as_js(&js2), "module must be valid JS:\n{js2}");
}

#[test]
fn svelte_element_reactive_class_directive_uses_accumulator_effect() {
    // A LIVE-signal `class:` directive on the fast path — official wraps the call in the
    // accumulator effect: `let classes;` + `$.template_effect(() => classes =
    // $.set_class($$element, 0, 'card', null, classes, { active: $.get(x) }))` (verified
    // against pinned svelte@5.56.10), with the legacy `on:` registration AFTER the effect.
    let js = emit(
        "<script>let tag = $state('div');let x = $state(false);</script>\n<svelte:element this={tag} class=\"card\" class:active={x} on:click={() => (x = !x)}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("let classes;")),
        "the reactive directive path declares the accumulator:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.template_effect(() => classes = $.set_class($$element, 0, 'card', null, classes, { active: $.get(x) }))"
        )),
        "the reactive set_class joins a template_effect with the accumulator:\n{js}"
    );
    // The legacy `on:` registration stays AFTER the class effect (official after_update).
    let effect_pos = n.find("template_effect").expect("class effect");
    let event_pos = n
        .find(&nc("$.event('click'"))
        .expect("legacy on: registration");
    assert!(
        effect_pos < event_pos,
        "the legacy on: registration must follow the class effect:\n{js}"
    );
    assert!(
        !js.contains("attribute_effect"),
        "the reactive fast path must NOT fold into attribute_effect:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_all_attribute_forms_order_and_state_gate() {
    // `onerror={() => n++} failed={failed} pending={pending}` — ALL three as attributes in
    // SOURCE order. The onerror arrow is NOT state-bearing (its read is inside the arrow body,
    // so official's `has_state` is false) ⇒ stays the PLAIN `onerror: …` init; the state-bearing
    // failed/pending become GETTERS. So the props object is exactly `{ onerror: …, get failed()
    // {…}, get pending() {…} }` — matching official's single attribute loop.
    let js = emit(
        "<script>let n = $state(0);\nlet { failed, pending } = $props();</script>\n<svelte:boundary onerror={() => n++} failed={failed} pending={pending}><p>content</p></svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.boundary(node, {onerror: () => $.update(n), get failed() { return $$props.failed; }, get pending() { return $$props.pending; }}, ($$anchor) =>"
        )),
        "all-attribute boundary: plain onerror init THEN failed/pending getters in source order:\n{js}"
    );
    // NEGATIVE: the non-state onerror is NOT promoted to a getter (state-gate discriminates).
    assert!(
        !n.contains(&nc("get onerror()")),
        "a non-state-bearing onerror arrow stays a plain init, not a getter:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

// ── <svelte:head> ──────────────────────────────────────────────────────────

#[test]
fn svelte_head_static_title_emits_effect_with_literal() {
    // A STATIC (constant-foldable) `<title>` → `$.head(hash(filename), ($$anchor) => { $.effect(
    // () => { $.document.title = 'literal'; }); })` — `has_state` false ⇒ `$.effect` (NOT
    // `$.deferred_template_effect`). A head-only root emits ONLY the `$.head(...)` op (no body
    // skeleton, no `$.from_html`, no `$.append`).
    let js = emit(
        "<script>\n\tlet { locale } = $props();\n</script>\n\n<svelte:head>\n\t<title>Dashboard</title>\n</svelte:head>\n",
        "special/svelte_head_static_title.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    // The `$.head` wrapper with the djb2-XOR hash of the filename (STRUCTURAL literal).
    assert!(
        n.contains(&nc("$.head('63bkss', ($$anchor) =>")),
        "head wrapper with the filename hash:\n{js}"
    );
    assert!(
        n.contains(&nc("$.effect(() => {$.document.title = 'Dashboard';})")),
        "static title effect ($.effect + folded literal):\n{js}"
    );
    // NEGATIVE: a static title is NOT deferred, and a head-only root has NO body skeleton.
    assert!(
        !js.contains("deferred_template_effect"),
        "static title must not defer:\n{js}"
    );
    assert!(
        !js.contains("$.from_html") && !js.contains("$.append") && !js.contains("$.comment"),
        "head-only root emits no body skeleton:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_head_state_title_emits_template_literal() {
    // A MULTI-chunk `<title>page {t}</title>` over a MUTATED `$state` → `$.deferred_template_effect
    // (() => { $.document.title = `page ${$.get(t) ?? ''}`; })` — a template literal with the
    // per-interpolation `?? ''` (NOT an outer wrap; the template is provably defined). The `t`
    // is a real signal (mutated by the button handler ⇒ `$.get(t)`, not a folded literal).
    let js = emit(
        "<script>\n\tlet count = $state(0);\n</script>\n\n<svelte:head>\n\t<title>page {count}</title>\n</svelte:head>\n\n<button onclick={() => count++}>inc</button>\n",
        "special/svelte_head_state_title.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.head('1523ehv', ($$anchor) =>")),
        "head wrapper:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.deferred_template_effect(() => {$.document.title = `page ${$.get(count) ?? ''}`;})"
        )),
        "deferred template-literal title:\n{js}"
    );
    assert!(
        !js.contains("$.effect("),
        "mutated-state title must defer:\n{js}"
    );
    // The head op emits at its SOURCE position — before the sibling button's delegated event.
    assert!(
        n.contains(&nc("$.head('1523ehv',"))
            && n.find(&nc("$.head('1523ehv',")) < n.find(&nc("$.delegated('click'")),
        "head op precedes the sibling button's delegated event:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_head_html_folded_state_title_emits_effect_and_html_body() {
    // The pre-existing `special/svelte_head_html` fixture: a `<title>{title}</title>` over an
    // UNMUTATED `$state('page')` folds to a KNOWN literal ⇒ `has_state` false ⇒ `$.effect(() => {
    // $.document.title = 'page'; })`, alongside a `<div>{@html markup}</div>` body. The head op
    // emits at its source position (after the `<div>` clone, before the div's `$.html`).
    let js = emit(
        "<script>\n\tlet title = $state('page');\n\tlet markup = $state('<b>bold</b>');\n</script>\n\n<svelte:head>\n\t<title>{title}</title>\n</svelte:head>\n\n<div>{@html markup}</div>\n",
        "special/svelte_head_html.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.head('1tufvvq', ($$anchor) => {$.effect(() => {$.document.title = 'page';});})"
        )),
        "folded `$state` title ⇒ `$.effect` + literal:\n{js}"
    );
    // The `{@html}` body still emits its raw-markup insertion.
    assert!(
        n.contains(&nc("$.html(div, () => markup, true)")),
        "the `{{@html}}` body:\n{js}"
    );
    // NEGATIVE: a folded (known) title is NOT deferred.
    assert!(
        !js.contains("deferred_template_effect"),
        "a folded/known title must not defer:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn state_raw_emits_signal_no_proxy() {
    // INVERTED (was `state_raw_fails_closed`): `$state.raw` is no longer an advanced
    // rune — a reassigned `$state.raw(0)` lowers to a bare `$.state(0)` signal
    // (`$.set(c, 1)`, `$.get(c)`), with NO `$.proxy` anywhere (raw is the deep-reactive
    // opt-out).
    let js = emit(
        "<script>let c = $state.raw(0);</script>\n<button onclick={() => c = 1}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let c = $.state(0);") && js.contains("$.set(c, 1)") && js.contains("$.get(c)"),
        "a reassigned `$state.raw(0)` is a bare `$.state` signal:\n{js}"
    );
    // NEGATIVE: raw never proxies, and the advanced-rune refusal is gone.
    assert!(
        !js.contains("$.proxy"),
        "a `$state.raw` binding must never proxy:\n{js}"
    );
}

// ── Special-host bind review-fix regression coverage (F1–F12) ──

#[test]
fn svelte_head_call_title_emits_memoized_deferred_template_effect() {
    // F1: a `has_call` `<title>` chunk MEMOIZES into the official deps-array form
    // `$.deferred_template_effect(($0) => { $.document.title = $0 ?? ''; }, [() => <call>])`
    // (the `TitleElement` `memoizer.apply()` params + `sync_values()` deps) — NOT a rejection
    // and NOT a non-memoized effect. RED against the pre-fix `reject_memoized_title_chunk`.
    let js = emit(
        "<script>\n\tlet count = $state(0);\n</script>\n\n<svelte:head>\n\t<title>{String(count)}</title>\n</svelte:head>\n\n<button onclick={() => count++}>inc</button>\n",
        "special/svelte_head_title_call.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.head('mt25c0', ($$anchor) =>")),
        "head wrapper with the filename hash:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.deferred_template_effect(($0) => {$.document.title = $0 ?? '';}"
        )),
        "memoized deferred effect with the `$0` placeholder param + RHS:\n{js}"
    );
    // The memoized dependency is the call itself (`() => String($.get(count))`), paren-tolerant.
    assert!(
        n.contains("String($.get(count))"),
        "the call is hoisted into the deps array:\n{js}"
    );
    // NEGATIVE: never a rejection, never a non-memoized `$.effect`, never inlining the call
    // into the assignment RHS (the memoizer replaces it with `$0`).
    assert!(
        !js.contains("$.effect("),
        "a call title defers (never $.effect):\n{js}"
    );
    assert!(
        !js.contains("$.document.title = String("),
        "the call must be memoized to `$0`, not inlined in the RHS:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_head_mixed_call_state_title_memoizes_only_the_call() {
    // F1: a mixed `<title>n {String(count)} {count}</title>` memoizes ONLY the call chunk
    // (`$0`) while the plain state read stays inline (`$.get(count)`) — the official
    // per-chunk memoize rule (`has_call` ⇒ hoist, live-state ⇒ inline).
    let js = emit(
        "<script>\n\tlet count = $state(0);\n</script>\n\n<svelte:head>\n\t<title>n {String(count)} {count}</title>\n</svelte:head>\n\n<button onclick={() => count++}>inc</button>\n",
        "special/svelte_head_title_call_mixed.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.document.title = `n ${$0 ?? ''} ${$.get(count) ?? ''}`"
        )),
        "the call memoizes to `$0` while the state read stays inline:\n{js}"
    );
    assert!(
        n.contains("String($.get(count))"),
        "the call is the single memoized dep:\n{js}"
    );
    // NEGATIVE: only ONE placeholder (the state read is NOT memoized to `$1`).
    assert!(
        !js.contains("$1"),
        "the live state read stays inline, not memoized:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_window_with_reactive_text_sibling_fails_closed() {
    // F2: a MIXED root with a bare REACTIVE-TEXT sibling (`<svelte:window/>{x}`) fails closed
    // as a `RootTextRegion` — EXACTLY as a reactive-text root WITHOUT a host special (the host
    // is transparent). RED against the pre-fix over-broad bypass that accepted it and emitted a
    // divergent shape (missing `$.next()` / wrong op order).
    assert_fail_closed(
        "<script>let x = $state('hi');</script>\n<svelte:window onresize={() => x = 'y'} />{x}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::RootTextRegion { .. }),
    );
}

#[test]
fn custom_element_bare_host_with_instance_effect_needs_context_accepts() {
    // A BARE `$host();` statement inside a top-level `$effect(fn)` body: the
    // user effect sets `needs_context` — that alone BINDS the props parameter
    // (official: `function App($$anchor, $$props)` + `$.push($$props, true)` +
    // `$.user_effect(() => { $$props.$$host; })`, first-hand pinned
    // `svelte@5.56.10`). The bare host use itself is NOT the trigger.
    let js = emit_result(
        "<svelte:options customElement=\"x-fxb\" />\n<script>\n\tlet c = $state(0);\n\t$effect(() => { $host(); });\n</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a bare $host() inside a user effect compiles via needs_context");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "needs_context binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the user effect opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$$props.$$host;"),
        "the effect-body bare host lowers to the bound host read:\n{js}"
    );
    assert!(
        js.contains("$.user_effect("),
        "the effect rides the user-effect helper:\n{js}"
    );
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
}

#[test]
fn svelte_options_runes_only_is_supported_and_emits() {
    // F4 NEGATIVE: a `<svelte:options runes={true}>` carries ONLY the supported
    // runes axis — it is consumed by mode inference and must NOT fail closed. The
    // component emits a Main. (Guards the over-refusal that would block the
    // supported axis.)
    let src = "<svelte:options runes={true} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("export default function App($$anchor)"),
        "a runes-only options element is supported and emits:\n{js}"
    );
    assert!(js.contains("$.state(0)"), "the state decl emits:\n{js}");
}

#[test]
fn effect_pre_toplevel_lowers_with_frame() {
    // INVERTED (was `effect_pre_fails_closed`): a top-level `$effect.pre(fn)`
    // statement lowers to `$.user_pre_effect` and forces the runes frame
    // (`$.push($$props, true)` / `$.pop()` + the `$$props` param), matching
    // svelte@5.56.10.
    let js = emit(
        "<script>let c = $state(0); $effect.pre(() => console.log(c));</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_pre_effect(() => console.log($.get(c)));"),
        "the `$effect.pre` statement lowers with its body rewritten:\n{js}"
    );
    // The HELPER-RENAME discriminator: `.pre` must NOT lower to the plain
    // `$.user_effect` (a re-label bug would still contain an effect helper).
    assert!(
        !js.contains("$.user_effect"),
        "`$effect.pre` must lower to `$.user_pre_effect`, never `$.user_effect`:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "`$effect.pre` forces the `$$props` param:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "`$effect.pre` forces the runes frame open:\n{js}"
    );
    assert!(js.contains("$.pop();"), "the frame closes:\n{js}");
    assert!(
        !js.contains("$effect"),
        "no raw `$effect.pre` rune survives:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn inspect_trace_first_statement_drops() {
    // POSITIVE (stays green across the placement fix): the ONE legal position — the
    // FIRST statement of a function body — never trips the placement reject.
    // (a) EMIT drop on a DIRECT (`$.event`) host: first statement of an `onfocus`
    // block arrow — the trace is dropped in place by the shared body rewriter, the
    // rest of the body lowers. (The DELEGATED-host drop is pinned by
    // `inspect_trace_dropped_in_event_arrow`.)
    let js = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => c++} onfocus={() => { $inspect.trace(); c++; }}>{c}</button>\n",
        "App.svelte",
    );
    assert!(!js.contains("inspect"), "the trace call is dropped:\n{js}");
    assert!(!js.contains("trace"), "no trace token survives:\n{js}");
    assert!(
        js.contains("focus"),
        "the onfocus handler body is preserved:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // (b) INVERTED (the `$effect` arm was an orthogonal fail-closed fixture while
    // `$effect` was refused wholesale): a first-statement trace in an `$effect`
    // arrow body is the ONE legal trace position — oracle-verified
    // (svelte@5.56.10, `dev:false`): the trace call is DROPPED in place and the
    // surrounding effect is KEPT (`$.user_effect(() => { $.update(c); });`).
    let js = emit(
        "<script>let c = $state(0); $effect(() => { $inspect.trace(); c++; });</script>\n<p>{c}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(() => {"),
        "the surrounding `$effect` lowers (not refused, not dropped):\n{js}"
    );
    assert!(
        js.contains("$.update(c)"),
        "the rest of the effect body is preserved:\n{js}"
    );
    assert!(!js.contains("inspect"), "the trace call is dropped:\n{js}");
    assert!(!js.contains("trace"), "no trace token survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // (c) A first-statement trace in a top-level `function` declaration body must
    // NOT reject as the placement error. The fixture still fails closed on its
    // own, ORTHOGONAL unsupported-FEATURE channel (a top-level `function` not
    // referenced by a function-pair bind is out of the instance-item allowlist) —
    // the discriminating fact is the refusal channel: a mis-firing placement scan
    // would surface `OfficialReject(InspectTraceInvalidPlacement)` instead. (The
    // gate-level `None` positives live in `official_reject_tests.rs`.)
    let js = emit(
        "<script>let c = $state(0); function tick() { $inspect.trace(); c = c + 1; }</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("function tick()") && js.contains("$.set(c, $.get(c) + 1);"),
        "ordinary function body did not lower correctly:\n{js}"
    );
    assert!(
        !js.contains("inspect") && !js.contains("trace"),
        "trace call leaked:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn inspect_trace_parenthesized_first_statement_drops() {
    // A PARENTHESIZED first-statement trace (`($inspect.trace());`) is the SAME
    // legal position — official svelte@5.56.10 ACCEPTS and drops it under
    // `dev:false`. The whole statement (parens included) is dropped in place; the
    // rest of the handler body lowers. RED before the fix: false-rejected as
    // `inspect_trace_invalid_placement` (the allow-set required a bare call).
    let js = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => { ($inspect.trace()); c++; }}>{c}</button>\n",
        "App.svelte",
    );
    assert!(!js.contains("inspect"), "the trace call is dropped:\n{js}");
    assert!(!js.contains("trace"), "no trace token survives:\n{js}");
    assert!(
        js.contains("$.update(c)"),
        "the rest of the handler body is preserved:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // CONTROL: the module equals the no-trace control modulo whitespace — the
    // WHOLE parenthesized statement is dropped (no stray `()` residue).
    let control = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => { c++; }}>{c}</button>\n",
        "App.svelte",
    );
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(
        norm(&js),
        norm(&control),
        "the dropped parenthesized-trace module must equal the no-trace control \
         modulo whitespace\nDROPPED:\n{js}\nCONTROL:\n{control}"
    );

    // POSITIVE: parens never legalize a NON-first trace — still the exact reject.
    assert_inspect_trace_invalid_placement(
        "<script>let c = $state(0);</script>\n<button onclick={() => { c++; ($inspect.trace()); }}>{c}</button>\n",
    );
}

#[test]
fn inspect_trace_object_parenthesized_first_statement_drops() {
    // Parens around the `$inspect` RECEIVER (`($inspect).trace();`) are equally
    // transparent — official svelte@5.56.10 ACCEPTS and drops it as a first statement.
    // The whole statement drops in place; the rest of the handler body lowers. RED
    // before the fix: the trace shape-check required a BARE `$inspect` member object, so
    // `($inspect).trace()` was not recognised — the `$inspect` reference failed closed
    // in the rewriter instead of dropping.
    let js = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => { ($inspect).trace(); c++; }}>{c}</button>\n",
        "App.svelte",
    );
    assert!(!js.contains("inspect"), "the trace call is dropped:\n{js}");
    assert!(!js.contains("trace"), "no trace token survives:\n{js}");
    assert!(
        js.contains("$.update(c)"),
        "the rest of the handler body is preserved:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
    // CONTROL: equals the no-trace control modulo whitespace (whole statement dropped).
    let control = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => { c++; }}>{c}</button>\n",
        "App.svelte",
    );
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(
        norm(&js),
        norm(&control),
        "DROPPED:\n{js}\nCONTROL:\n{control}"
    );
}

#[test]
fn host_rune_fails_closed() {
    // F4: `$host()` is the custom-element-only API.
    assert_fail_closed(
        "<script>let c = $state(0); const el = $host();</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { surface, .. } if *surface == "$host"),
    );
}

#[test]
fn shadowed_rune_name_is_not_refused_as_advanced_rune() {
    // F4 DISCRIMINATION: a function PARAM named like a rune (`function f($inspect) {
    // return $inspect.foo }`) is SHADOWED — its member access is NOT a rune reference,
    // so the rune-form scan does not fire. The ordinary function is preserved and
    // the shadowed member stays ordinary JavaScript.
    let js = emit(
        "<script>\n\tlet c = $state(0);\n\tfunction f($inspect) { return $inspect.foo; }\n</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("function f($inspect) { return $inspect.foo; }"),
        "shadowed rune-like parameter must remain ordinary JavaScript:\n{js}"
    );
    assert!(
        !js.contains("$.inspect"),
        "shadowed parameter was treated as a rune:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

// ── Position-sensitive bare-rune classification (a bare rune is supported ONLY
//    in its exact legal position; refuse everywhere else) ──────────────────────

#[test]
fn bare_state_in_default_param_fails_closed() {
    // A bare `$state(0)` in a function DEFAULT-PARAM position is NOT a supported
    // rune position (the supported `$state` position is the init of a top-level
    // instance-script identifier declarator). It must fail closed, never emit
    // raw `$state(0)` (a runtime ReferenceError). RED against the pre-fix scan,
    // which skipped bare `$state` calls ("they carry their own emission").
    assert_fail_closed(
        "<script>let count=$state(0); function f(x = $state(0)) {}</script>\n<p>hi</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state"),
    );
}

#[test]
fn bare_derived_in_call_arg_fails_closed() {
    // A bare `$derived(...)` as a CALL ARGUMENT (`foo($derived(c))`) is not the
    // supported top-level identifier-declarator-init position — fail closed.
    // RED against the pre-fix scan.
    assert_fail_closed(
        "<script>let c=$state(0); foo($derived(c));</script>\n<p>hi</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$derived"),
    );
}

#[test]
fn bare_derived_in_nested_block_fails_closed() {
    // A `$derived(...)` declarator nested in a BLOCK statement (`{ let d =
    // $derived(c); }`) is not a TOP-LEVEL declarator — fail closed. Official
    // lowers it; our supported subset is narrower (deferral ledger). RED against
    // the pre-fix scan.
    assert_fail_closed(
        "<script>let c=$state(0); { let d = $derived(c); }</script>\n<p>hi</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$derived"),
    );
}

#[test]
fn bare_rune_identifier_reference_fails_closed() {
    // A bare rune-name IDENTIFIER reference (`foo($state)`) — the rune function
    // passed by reference, not called in its supported position — fails closed
    //. RED against the pre-fix scan (which only saw the declarator init).
    assert_fail_closed(
        "<script>let c=$state(0); foo($state);</script>\n<p>hi</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state"),
    );
}

#[test]
fn module_script_statements_emit_while_module_runes_fail_closed() {
    // Module-scope runes retain the precise `ModuleScriptItem` refusal while
    // ordinary statements and exports emit. An instance `$state` keeps the
    // component in runes mode.
    for module_body in [
        "let x=$state(0)",
        "let x=$state(0); let y=$derived(x)",
        "let {a}=$props()",
    ] {
        let src = format!(
            "<script module>{module_body}</script>\n<script>let c = $state(0);</script>\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        assert_fail_closed(&src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ModuleScriptItem {
                    construct: "variable declaration",
                    ..
                }
            )
        });
    }
    // A supported statement before an unsupported module rune must not steal the
    // diagnostic span. The refusal points at the rune-owning declaration.
    let module_body = "const K = 1; let x = $state(0);";
    let source = format!(
        "<script module>{module_body}</script>\n<script>let c = $state(0);</script>\n<button onclick={{() => c++}}>{{c}}</button>\n"
    );
    let err = emit_result(&source).expect_err("a module rune must fail closed");
    let ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::ModuleScriptItem {
        construct,
        span,
    }) = err
    else {
        panic!("expected a module-item refusal, got {err:?}");
    };
    assert_eq!(construct, "variable declaration");
    assert_eq!(span.start as usize, module_body.find("let x").unwrap());

    // Ordinary declarations after an import and re-exports both emit.
    let js = emit(
        "<script module>import { m } from './base.js'; const K = 1;</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const K = 1;"),
        "module declaration missing:\n{js}"
    );
    let js = emit(
        "<script module>export { m } from './base.js';</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("export { m } from './base.js';"),
        "module re-export missing:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn var_state_declarator_fails_closed() {
    // A `var` `$state` declarator is a distinct official surface — a `var` rune read
    // is `$.safe_get(c)` (var hoisting), not `$.get(c)`. Verter does not emit the
    // `$.safe_get` form, so it fails closed rather than emitting `$.get`. RED
    // against the pre-fix classifier (which accepted `var`/`const` rune declarators).
    assert_fail_closed(
        "<script>var c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune: "non-let $state declarator",
                    ..
                }
            )
        },
    );
}

#[test]
fn const_state_declarator_fails_closed_not_static_fold() {
    // A read-only `const` `$state` compiles to an EMPTY reactive topology in
    // official (the value is constant-folded), a divergent surface — fail closed at
    // the decl-kind gate, NOT as a static-interpolation fold. RED against
    // the pre-fix flow (which reached the static-interpolation fold check for the `{c}` read).
    assert_fail_closed(
        "<script>let w = $state(0); const c = $state(0);</script>\n<button onclick={() => w++}>{c}{w}</button>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune: "non-let $state declarator",
                    ..
                }
            )
        },
    );
}

#[test]
fn var_derived_declarator_fails_closed() {
    // A `var` `$derived` declarator reads with `$.safe_get` in official — fail closed
    // rather than emit the `$.get` form Verter produces.
    assert_fail_closed(
        "<script>let c = $state(0); var d = $derived(c * 2);</script>\n<button onclick={() => c++}>{d}</button>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune: "non-let $derived declarator",
                    ..
                }
            )
        },
    );
}

#[test]
fn const_derived_declarator_fails_closed() {
    // A `const` `$derived` declarator — even though official reads it with `$.get`,
    // the supported client surface accepts ONLY `let` rune declarators, so it fails
    // closed until the const/var rune-declarator forms are lowered faithfully.
    assert_fail_closed(
        "<script>let c = $state(0); const d = $derived(c * 2);</script>\n<button onclick={() => c++}>{d}</button>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune: "non-let $derived declarator",
                    ..
                }
            )
        },
    );
}

#[test]
fn options_runes_with_static_text_root_fails_closed() {
    // A `<svelte:options runes />hello` (runes forced via the options element, with
    // a bare static-text root) is the same text-first topology — official emits
    // `$.next(); var text = $.text('hello'); $.append(...)`. It fails closed,
    // never the broken `root()`-on-a-node clone frame.
    assert_fail_closed("<svelte:options runes={true} />hello\n", |s| {
        matches!(s, UnsupportedSvelteRuntimeSurface::RootTextRegion { .. })
    });
}

// ── Additional surface gates (R1, R4, R5, R7, R8) ──────────────────────────────

#[test]
fn destructured_state_object_fails_closed_not_panic() {
    // R1: `let { a } = $state({a:1})` MUST fail closed, NEVER reach a panic.
    // Official 5.56.10 supports it (temp + proxy); Verter does not lower
    // destructured state yet, so a clean fail-closed is correct. RED against
    // the prior `unreachable!()` (which PANICKED on this valid input).
    assert_fail_closed(
        "<script>let { a } = $state({ a: 1 });</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }),
    );
}

#[test]
fn destructured_state_array_fails_closed_not_panic() {
    // R1: `let [x] = $state([1])` — the array-destructure form also fails closed.
    assert_fail_closed(
        "<script>let [x] = $state([1]);</script>\n<p>{x}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }),
    );
}

#[test]
fn lang_ts_direct_reactive_lvalues_erase_wrappers_and_lower_writes() {
    let js = emit(
        "<script lang=\"ts\">let count: number = $state(0); function advance(delta: number): void { count! += delta!; count!++; }</script>\n<button onclick={advance}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("function advance(delta)"),
        "function annotations were not erased:\n{js}"
    );
    assert!(
        js.contains("$.set(count, $.get(count) + delta);") && js.contains("$.update(count);"),
        "TypeScript-wrapped signal writes did not lower through the reactive helpers:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}
// ── Additional surface gates (R4 reactive-text memoizer, R5 needs_context) ─────
#[test]
fn reactive_text_bare_signal_read_stays_inline() {
    // R4 NEGATIVE (§1.2 preservation): a bare signal read (`{count}`, no call) stays
    // the INLINE `$.set_text(text, $.get(count))` form — the memoizer is NOT used.
    // Verified against svelte@5.56.10. `count` is reassigned (a real signal), so the
    // read is `$.get(count)`.
    let src = "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count}</button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains("$.template_effect(()=>$.set_text(text,$.get(count)))"),
        "a bare signal read must stay the inline form:\n{js}"
    );
    assert!(
        !n.contains("[()=>$.get(count)]"),
        "a bare signal read must NOT be memoized:\n{js}"
    );
}

#[test]
fn multi_declarator_state_with_destructure_fails_closed() {
    // A multi-declarator statement where a LATER declarator destructures `$state`
    // (`let ok = $state(0); let { a } = $state({ a: 1 })`) must fail closed —
    // the gate scans ALL `$state` declarators, not just the first. RED against the
    // pre-refactor gate (which classified only the first declarator and silently
    // dropped the destructured one → a runtime `ReferenceError` on `a`).
    assert_fail_closed(
        "<script>let ok = $state(0); let { a } = $state({ a: 1 });</script>\n<button onclick={() => ok++}>{ok}{a}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }),
    );
}

#[test]
fn no_arg_state_lowers_to_void_zero_not_shadowable_undefined() {
    // The no-arg `$state()` init is `undefined` — but the official compiler emits the
    // SHADOW-ROBUST `$.state(void 0)`, never the bare identifier `undefined` (which a
    // local `let undefined` would shadow, diverging the initial state). Verter emitted
    // `$.state(undefined)`. It must emit `$.state(void 0)`. RED against the pre-fix
    // tree. The shadow-robust `void 0` is the lowering form regardless of a
    // shadowing local.
    let js = emit(
        "<script>let c = $state();</script>\n<button onclick={() => c = 1}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.state(void 0)"),
        "no-arg $state() must lower to the shadow-robust `$.state(void 0)`:\n{js}"
    );
    // NEGATIVE: never the bare shadowable identifier as the `$.state` argument.
    assert!(
        !js.contains("$.state(undefined)"),
        "no-arg $state() must NOT emit the shadowable `undefined`:\n{js}"
    );
}

#[test]
fn explicit_undefined_state_arg_matches_official_undefined() {
    // NEGATIVE / oracle-fidelity: an EXPLICIT `$state(undefined)` argument is preserved
    // by the official compiler as `$.state(undefined)` (it references the same global
    // binding the user wrote — no divergence), so ONLY the no-arg case is rewritten to
    // `void 0`. Verter must match: explicit `undefined` stays `undefined`.
    let js = emit(
        "<script>let c = $state(undefined);</script>\n<button onclick={() => c = 1}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.state(undefined)"),
        "explicit $state(undefined) must stay `$.state(undefined)` (matching official):\n{js}"
    );
    // The explicit-arg case must NOT be force-rewritten to `void 0`.
    assert!(
        !js.contains("$.state(void 0)"),
        "explicit $state(undefined) must NOT be rewritten to `void 0`:\n{js}"
    );
}

#[test]
fn dynamic_attr_reactive_emits_set_attribute_in_template_effect() {
    let src = "<script>let id = $state('x');</script>\n<button onclick={() => id += '!'} id={id}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    // The exact official form. `nc` normalizes the expected form identically (strips
    // whitespace, unifies JS string-literal quotes to `"`), so it is written naturally.
    assert!(
        n.contains(&nc(
            "$.template_effect(() => $.set_attribute(button, 'id', $.get(id)))"
        )),
        "reactive dynamic attr must be a set_attribute in a template_effect:\n{js}"
    );
    // NEGATIVE: never a boolean 4th-arg misform, never a property write for `id`.
    assert!(
        !n.contains(&nc("$.set_attribute(button, 'id', $.get(id), true)")),
        "no hydration-suppression 4th arg for a plain attr:\n{js}"
    );
    assert!(
        !n.contains("button.id="),
        "`id` is NOT a DOM property — must use set_attribute, not a property write:\n{js}"
    );
}

// NOTE: a NON-REACTIVE dynamic attribute / class / style value (the official
// `state.init` half of the `has_state ? update : init` split) is NOT exercisable in
// the §1.2-class supported subset: every template-readable local is a `$state`
// signal, a `$props()` read (also reactive in the output), or a `bind:this` ref — a
// plain non-rune `let v = 'x'` fails closed at the instance-script-item gate ("plain
// let", script-import). The non-reactive INIT path is still implemented (and is exercised by the
// init-only `$.autofocus` cases below); a non-reactive `$.set_attribute` /
// `$.set_class` / `$.set_style` init becomes testable once plain-local support lands.

#[test]
fn mixed_reactive_attr_emits_template_literal_in_effect() {
    // A reactive mixed value (`id="pre-{v}-post"`, v=$state) →
    // `` $.set_attribute(div, 'id', `pre-${$.get(v) ?? ''}-post`) `` in the effect.
    let src = "<script>let v = $state('x');</script>\n<div onclick={() => v += '!'} id=\"pre-{v}-post\"></div>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.template_effect(() => $.set_attribute(div, 'id', `pre-${$.get(v) ?? ''}-post`))"
        )),
        "a reactive mixed attr must be a template-literal set_attribute in the effect:\n{js}"
    );
}

#[test]
fn class_base_with_reactive_directive_uses_accumulator() {
    // `class="base" class:foo={on}` (reactive directive) → the `let classes;`
    // accumulator + `$.set_class(button, 1, 'base', null, classes, { foo: $.get(on) })`.
    let src = "<script>let on = $state(false);</script>\n<button onclick={() => on = !on} class=\"base\" class:foo={on}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("let classes;")),
        "a reactive class directive needs the accumulator:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "classes = $.set_class(button, 1, 'base', null, classes, { foo: $.get(on) })"
        )),
        "the merged set_class shape (base/null/classes/directives) is wrong:\n{js}"
    );
    // NEGATIVE: the static base `class="base"` is pulled OUT of the skeleton.
    assert!(
        !js.contains("$.from_html(`<button class=\"base\""),
        "a class with a directive must pull the base class OUT of the skeleton:\n{js}"
    );
}

#[test]
fn combined_reactive_attr_class_style_share_one_template_effect() {
    // All three reactive → ONE combined `$.template_effect` with the set_attribute /
    // set_class / set_style in source order.
    let src = "<script>let id=$state('a'); let c=$state('b'); let s=$state('c');</script>\n<button onclick={() => { id+='!'; c+='!'; s+='!'; }} id={id} class={c} style={s}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    // The single combined block in source order.
    assert!(
        n.contains(&nc(
            "$.template_effect(() => { $.set_attribute(button, 'id', $.get(id)); $.set_class(button, 1, $.clsx($.get(c))); $.set_style(button, $.get(s)); })"
        )),
        "reactive attr/class/style must share ONE template_effect in source order:\n{js}"
    );
    // NEGATIVE: there is exactly ONE template_effect (no per-attribute effects).
    assert_eq!(
        n.matches("template_effect").count(),
        1,
        "reactive attr/class/style must NOT emit separate template_effects:\n{js}"
    );
}

#[test]
fn reactive_attr_and_reactive_text_share_one_template_effect() {
    // The cross-cut: a reactive attr and reactive text on the same region share ONE
    // combined `$.template_effect`, in DOM-walk order (attr first, then text).
    let src = "<script>let id=$state('a'); let t=$state('hi');</script>\n<button onclick={() => { id+='!'; t+='!'; }} id={id}>{t}</button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.template_effect(() => { $.set_attribute(button, 'id', $.get(id)); $.set_text(text, $.get(t)); })"
        )),
        "a reactive attr and reactive text must share ONE template_effect:\n{js}"
    );
    assert_eq!(
        n.matches("template_effect").count(),
        1,
        "attr + text must NOT emit two template_effects:\n{js}"
    );
}

// ─── Mixed text+interpolation style directive ───
// `style:color="a{x}b"` (the SOLE directive family that accepts a text body) folds the
// template-literal `{ color: `a${x ?? ''}b` }`; a NON-reassigned $state const-folds (the
// static path), so the live cells reassign `x`.

#[test]
fn mixed_style_directive_folds_a_reactive_template_literal() {
    // `<div style:color="a{x}b">` (x reassigned) → `$.set_style(div, '', styles, { color:
    // `a${$.get(x) ?? ''}b` })` inside a template_effect.
    let js = emit(
        "<script>let x = $state(0);</script>\n<div style:color=\"a{x}b\"><button onclick={() => x++}>b</button></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.set_style(div, '', styles, { color: `a${$.get(x) ?? ''}b` })"
        )),
        "a mixed-style directive must fold the reactive template literal:\n{js}"
    );
    // NEGATIVE: it must NOT mis-parse `a{x}b` as a lone expression / object literal.
    assert!(
        !n.contains(&nc("{ color: a{x}b }")) && !n.contains(&nc("color: $.get(a)")),
        "a mixed-style directive must NOT mis-parse the concatenation:\n{js}"
    );
}

// ─── `{@render}` argument memoization (the official per-RenderTag `Memoizer`) ───

#[test]
fn render_arg_spread_hoists_local_derived() {
    // A `has_call`-bearing render argument (`[...xs]` — a spread counts as a call, the
    // official `SpreadElement` analysis) is MEMOIZED: official hoists a local
    // `let $0 = $.derived(() => [...$$props.xs]);` in a wrapping block and passes the
    // `() => $.get($0)` thunk — NOT the un-memoized inline `() => [...$$props.xs]`
    // (which would rebuild the array identity on every read).
    let js = emit_result(
        "<script>let { xs } = $props();</script>\n{#snippet row(items)}<p>{items}</p>{/snippet}\n{@render row([...xs])}\n",
    )
    .expect("a spread render arg emits a module");
    assert!(
        js.contains("let $0 = $.derived(() => ([...$$props.xs]));"),
        "missing the memoized render-arg derived hoist:\n{js}"
    );
    assert!(
        js.contains("row($$anchor, () => $.get($0));"),
        "the render call must pass the memoized $.get thunk:\n{js}"
    );
    // NEGATIVE: the un-memoized inline thunk must be GONE.
    assert!(
        !js.contains("row($$anchor, () => [...$$props.xs])"),
        "the render arg must not stay an un-memoized inline thunk:\n{js}"
    );
}

#[test]
fn nonprimitive_state_array_init_emits_proxy_and_hoists_render_spread() {
    // INVERTED (was `nonprimitive_state_array_init_fails_closed_at_rune_gate`): a
    // `$state([1])` ARRAY init now compiles as a deep-reactive `$.proxy([1])` (a
    // never-reassigned `BareProxy`), and the `{@render row([...xs])}` spread arg hoists
    // into a local `$.derived` memo read via `$.get` — matching svelte@5.56.10 (modulo a
    // cosmetic paren around the spread).
    let js = emit(
        "<script>let xs = $state([1]);</script>\n{#snippet row(items)}<p>{items}</p>{/snippet}\n{@render row([...xs])}\n",
        "App.svelte",
    );
    assert!(
        js.contains("let xs = $.proxy([1]);"),
        "a never-reassigned array `$state` is a bare `$.proxy`:\n{js}"
    );
    // NEGATIVE: no `$.state` signal box for a never-reassigned BareProxy.
    assert!(
        !js.contains("$.state("),
        "a BareProxy must not be a `$.state` signal:\n{js}"
    );
    // The spread render arg hoists into a local `$.derived` memo (paren-tolerant).
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.derived(() => [...xs])")),
        "the spread render arg hoists into a `$.derived` memo:\n{js}"
    );
}

// ─── `$state.raw` + proxied object-`$state` + `$state.snapshot` ───
//
// The STATE FAMILY: the deep-reactive object/array `$state` declarator (BareProxy /
// StateProxy), the `$state.raw` opt-out (RawStateSignal / PlainLet), the raw-aware
// reassignment flag (Q6), and the `$state.snapshot(x)` → `$.snapshot(x)` expression
// rewrite. Every emitted shape verified against pinned `svelte@5.56.10`.

#[test]
fn state_raw_object_reassign_emits_state_no_proxy_no_flag() {
    // MANDATORY object discriminator + Q6 trap: a `$state.raw({ a: 1 })` that is
    // REASSIGNED lowers to `let o = $.state({ a: 1 })` — NO `$.proxy` wrapper — and
    // the reassign is `$.set(o, { a: 2 })` with NO trailing `, true` (raw NEVER
    // proxies its RHS). Verified against svelte@5.56.10.
    let js = emit(
        "<script>let o = $state.raw({ a: 1 });</script>\n<button onclick={() => o = { a: 2 }}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let o = $.state({ a: 1 });"),
        "raw object reassign is a bare `$.state(<init>)` signal:\n{js}"
    );
    // NEGATIVE: never a `$.proxy` wrapper on a raw binding.
    assert!(
        !js.contains("$.proxy"),
        "a `$state.raw` object must NOT proxy:\n{js}"
    );
    assert!(
        js.contains("$.set(o, { a: 2 })"),
        "raw reassign lowers to a bare `$.set(o, rhs)`:\n{js}"
    );
    // NEGATIVE (Q6): a raw binding NEVER gets the trailing proxy flag, even for an
    // object RHS.
    assert!(
        !js.contains("$.set(o, { a: 2 }, true)"),
        "a raw object reassign must NOT carry the trailing `, true`:\n{js}"
    );
}

#[test]
fn state_raw_array_reassign_emits_state_no_proxy() {
    // The array variant of the raw discriminator: `$state.raw([1])` reassigned →
    // `let o = $.state([1])`, `$.set(o, [2])` (no proxy, no flag).
    let js = emit(
        "<script>let o = $state.raw([1]);</script>\n<button onclick={() => o = [2]}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let o = $.state([1]);"),
        "raw array reassign is a bare `$.state([1])`:\n{js}"
    );
    assert!(
        !js.contains("$.proxy") && !js.contains("$.set(o, [2], true)"),
        "a raw array must NOT proxy and NOT carry the flag:\n{js}"
    );
    assert!(
        js.contains("$.set(o, [2])"),
        "raw array reassign lowers to `$.set(o, [2])`:\n{js}"
    );
}

#[test]
fn state_proxy_object_reassign_emits_state_proxy_with_flag() {
    // MANDATORY object discriminator (proxied side): a plain `$state({ a: 1 })` that
    // is REASSIGNED lowers to `let o = $.state($.proxy({ a: 1 }))` and the reassign
    // is `$.set(o, { a: 2 }, true)` — WITH the trailing proxy flag.
    let js = emit(
        "<script>let o = $state({ a: 1 });</script>\n<button onclick={() => o = { a: 2 }}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let o = $.state($.proxy({ a: 1 }));"),
        "proxied object reassign is `$.state($.proxy(<init>))`:\n{js}"
    );
    assert!(
        js.contains("$.set(o, { a: 2 }, true)"),
        "a proxied object reassign carries the trailing `, true`:\n{js}"
    );
}

#[test]
fn state_bare_proxy_no_reassign_emits_proxy_with_plain_reads() {
    // A proxiable `$state({ a: 1 })` that is DEEP-MUTATED but never REASSIGNED is a
    // BareProxy: `let o = $.proxy({ a: 1 })` (no `$.state` signal box), and the
    // member mutation stays PLAIN (`o.a++`) — a proxy is not a signal, reads never
    // `$.get`.
    let js = emit(
        "<script>let o = $state({ a: 1 });</script>\n<button onclick={() => o.a++}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let o = $.proxy({ a: 1 });"),
        "a never-reassigned object `$state` is a bare `$.proxy(<init>)`:\n{js}"
    );
    // NEGATIVES: a BareProxy is not a signal — no `$.state(` box for `o`, no `$.get`
    // read, no `$.set` write.
    assert!(
        !js.contains("$.state(") && !js.contains("$.get(o)") && !js.contains("$.set(o"),
        "a BareProxy must never be a signal (no $.state/$.get/$.set):\n{js}"
    );
    assert!(
        js.contains("o.a++"),
        "a BareProxy member mutation stays plain `o.a++`:\n{js}"
    );
}

#[test]
fn state_raw_primitive_is_byte_identical_to_plain_state() {
    // Primitive equivalence: `$state.raw(0)` and `$state(0)` emit BYTE-IDENTICAL
    // client output (both `let c = $.state(0)`, `$.set(c, 1)`, `$.get(c)`) — a raw
    // primitive is just a signal (proxy never applies to a primitive).
    let raw = emit(
        "<script>let c = $state.raw(0);</script>\n<button onclick={() => c = 1}>{c}</button>\n",
        "App.svelte",
    );
    let plain = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => c = 1}>{c}</button>\n",
        "App.svelte",
    );
    assert_eq!(
        raw, plain,
        "a raw primitive `$state.raw(0)` must be byte-identical to `$state(0)`:\nraw:\n{raw}\nplain:\n{plain}"
    );
    assert!(
        raw.contains("let c = $.state(0);") && raw.contains("$.set(c, 1)"),
        "raw primitive is a plain `$.state(0)` signal:\n{raw}"
    );
    assert!(
        !raw.contains("$.proxy"),
        "a raw primitive must never proxy:\n{raw}"
    );
}

#[test]
fn state_proxy_object_init_reads_signal_via_get() {
    // The object `$state` init routes through the shared expression rewriter, so a
    // signal read INSIDE the init is `$.get`-rewritten (matching official
    // `$.proxy({ x: $.get(a) })`). Discriminating: proves the init is not emitted
    // verbatim.
    let js = emit(
        "<script>let a = $state(0);\nlet o = $state({ x: a });</script>\n<button onclick={() => { a = 1; o.x = 2; }}>x</button>\n",
        "App.svelte",
    );
    // `a` is reassigned → a StateSignal; `o` is deep-mutated only → a BareProxy whose
    // init reads `a` reactively.
    assert!(
        js.contains("let o = $.proxy({ x: $.get(a) });"),
        "an object `$state` init `$.get`-rewrites a signal read:\n{js}"
    );
    // NEGATIVE: the init must NOT be emitted verbatim (`{ x: a }`).
    assert!(
        !js.contains("$.proxy({ x: a })"),
        "the init must not read the signal verbatim:\n{js}"
    );
}

#[test]
fn state_snapshot_nested_rewrites_every_occurrence() {
    // Nested `$state.snapshot($state.snapshot(o))` rewrites BOTH callees. `o` is a
    // reassigned StateProxy, so the innermost argument reads `$.get(o)`.
    let js = emit(
        "<script>let o = $state({ a: 1 });</script>\n<button onclick={() => o = $state.snapshot($state.snapshot(o))}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snapshot($.snapshot($.get(o)))"),
        "a nested snapshot rewrites both occurrences:\n{js}"
    );
    assert!(
        !js.contains("$state.snapshot"),
        "no raw `$state.snapshot` may remain:\n{js}"
    );
}

#[test]
fn state_snapshot_argument_signal_read_still_rewrites() {
    // The snapshot argument is recursed: a signal read inside it still lowers to
    // `$.get`. Here `o` is reassigned → a StateProxy, so `$state.snapshot(o)` →
    // `$.snapshot($.get(o))`.
    let js = emit(
        "<script>let o = $state({ a: 1 });</script>\n<button onclick={() => o = $state.snapshot(o)}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snapshot($.get(o))"),
        "a signal read inside the snapshot arg still rewrites:\n{js}"
    );
}

#[test]
fn called_state_snapshot_still_rewrites_after_uncalled_gate() {
    // F1 negative half: the SUPPORTED called form `$state.snapshot(x)` in a handler
    // must STILL rewrite to `$.snapshot(x)` (the uncalled-gate must not regress it).
    let js = emit(
        "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = $state.snapshot(o)}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snapshot(o)"),
        "the called `$state.snapshot(o)` still rewrites to `$.snapshot(o)`:\n{js}"
    );
    assert!(
        !js.contains("$state.snapshot"),
        "no raw `$state.snapshot` may remain:\n{js}"
    );
}

#[test]
fn state_snapshot_zero_args_fails_closed_as_rune() {
    // G1 (fail-open fix): `$state.snapshot()` with ZERO arguments is the official
    // `rune_invalid_arguments_length` compile error ("`$state.snapshot` must be called
    // with exactly one argument" — oracle-verified against `svelte@5.56.10`). Only the
    // WELL-FORMED single-non-spread-arg form rewrites to `$.snapshot(<expr>)`. Before G1
    // the rune scan exempted EVERY called `$state.snapshot(...)` form and the rewriter
    // rewrote the callee unconditionally, so this emitted a raw `$.snapshot()`
    // (a fail-open miscompile). It MUST fail closed as an advanced rune.
    assert_fail_closed(
        "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = $state.snapshot()}>x</button>\n",
        |s| matches!(
            s,
            UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.snapshot"
        ),
    );
}

#[test]
fn state_snapshot_two_args_fails_closed_as_rune() {
    // G1 (fail-open fix): `$state.snapshot(a, b)` with TWO arguments is the official
    // `rune_invalid_arguments_length` compile error (oracle-verified). Before G1 this
    // emitted a raw `$.snapshot(a, b)` (a fail-open miscompile). It MUST fail closed as
    // an advanced rune.
    assert_fail_closed(
        "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = $state.snapshot(o, o)}>x</button>\n",
        |s| matches!(
            s,
            UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.snapshot"
        ),
    );
}

#[test]
fn state_snapshot_spread_arg_fails_closed_as_rune() {
    // G1 (fail-open fix): `$state.snapshot(...o)` with a SPREAD argument is the official
    // `rune_invalid_spread` compile error ("`$state.snapshot` cannot be called with a
    // spread argument" — oracle-verified against `svelte@5.56.10`). Before G1 this emitted
    // a raw `$.snapshot(...o)` (a fail-open miscompile). It MUST fail closed as an
    // advanced rune.
    assert_fail_closed(
        "<script>let arr = $state([1]);\nlet snap = $state(null);</script>\n<button onclick={() => snap = $state.snapshot(...arr)}>x</button>\n",
        |s| matches!(
            s,
            UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.snapshot"
        ),
    );
}

#[test]
fn state_snapshot_in_instance_script_call_initializer_is_rewritten() {
    // F6 (documented deferral): `let s = $state.snapshot(c)` as a plain-CALL instance
    // -script initializer fails closed at the plain-call-initializer CARRIER gate
    // (`InstanceScriptItem { construct: "plain let with call init" }`), NOT as a rune
    // refusal. Official emits it via `let s = $.snapshot($.get(c))`, but any `let s =
    // foo()` instance-script call initializer rides the same pre-existing carrier — a
    // separate deferred surface. Discriminating: the disposition is the CARRIER, not
    // `AdvancedRune`.
    // TODO(follow-up): support snapshot (and any plain-call) instance-script call
    // initializers — owned by the instance-script-call-init carrier surface.
    let js = emit(
        "<script>let c = $state({ a: 1 });\nlet s = $state.snapshot(c);</script>\n<button onclick={() => c.a++}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let s = $.snapshot(c);"),
        "state snapshot call initializer did not rewrite:\n{js}"
    );
    assert!(!js.contains("$state.snapshot"), "raw rune leaked:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn paren_receiver_state_snapshot_wellformed_rewrites_to_dollar_snapshot() {
    // Receiver parens are TRANSPARENT to official (its ESTree AST has no paren
    // nodes): `($state).snapshot(o)` compiles to `$.snapshot(o)` exactly like the
    // plain spelling (oracle-verified against svelte@5.56.10). The scan's
    // well-formed-call exemption and the rewriter agree on the peeled receiver,
    // and the rewrite overwrites the paren-INCLUSIVE callee member span, so no
    // receiver-paren residue survives into the emission.
    let js = emit(
        "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state).snapshot(o)}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snapshot(o)"),
        "`($state).snapshot(o)` rewrites to `$.snapshot(o)`:\n{js}"
    );
    // NEGATIVE: no raw rune member and no receiver-paren residue.
    assert!(
        !js.contains("$state.snapshot") && !js.contains("($state)"),
        "no raw `$state.snapshot` / `($state)` residue may remain:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn paren_receiver_state_snapshot_malformed_fails_closed_as_rune() {
    // A MALFORMED `($state).snapshot(...)` call — zero args / two args (official
    // `rune_invalid_arguments_length`, oracle-verified against svelte@5.56.10 for
    // the parenthesized spelling too) or a spread argument (official
    // `rune_invalid_spread`) — refuses under the SAME `$state.snapshot` label as
    // the plain spellings, never the coarse bare-`$state` position refusal the
    // receiver identifier would record on its own.
    let cases: &[(&str, &str)] = &[
        (
            "paren_snapshot_zero_args",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state).snapshot()}>x</button>\n",
        ),
        (
            "paren_snapshot_two_args",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state).snapshot(o, o)}>x</button>\n",
        ),
        (
            "paren_snapshot_spread_arg",
            "<script>let arr = $state([1]);\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state).snapshot(...arr)}>x</button>\n",
        ),
    ];
    for (label, source) in cases {
        assert_fail_closed_labeled(label, source, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.snapshot"
            )
        });
    }
}

#[test]
fn paren_callee_state_snapshot_wellformed_rewrites_to_dollar_snapshot() {
    // Whole-CALLEE parens are TRANSPARENT to official (its ESTree AST has no paren
    // nodes): `($state.snapshot)(o)` compiles to `$.snapshot(o)` exactly like the
    // plain spelling — oracle-verified against svelte@5.56.10 for every nesting:
    // single, doubled, receiver-parens-inside-callee-parens, and a whole-call
    // wrapper around the paren-callee call. The scan's well-formed-call exemption
    // and the rewriter peel the SAME callee, and the rewrite overwrites the
    // paren-INCLUSIVE whole-callee span, so no callee-paren residue survives.
    let cases: &[(&str, &str)] = &[
        (
            "callee_paren",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state.snapshot)(o)}>x</button>\n",
        ),
        (
            "double_callee_paren",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = (($state.snapshot))(o)}>x</button>\n",
        ),
        (
            "receiver_paren_inside_callee_paren",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = (($state).snapshot)(o)}>x</button>\n",
        ),
        (
            "whole_call_around_paren_callee",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = (($state.snapshot)(o))}>x</button>\n",
        ),
    ];
    for (label, source) in cases {
        let js = emit(source, "App.svelte");
        assert!(
            js.contains("$.snapshot(o)"),
            "[{label}] the paren-callee snapshot call rewrites to `$.snapshot(o)`:\n{js}"
        );
        // NEGATIVE: no raw rune text and no callee-paren residue around the helper
        // (a callee-member-only overwrite would leave `($.snapshot)(o)`).
        assert!(
            !js.contains("$state") && !js.contains("($.snapshot)"),
            "[{label}] no raw `$state` / `($.snapshot)` callee residue may remain:\n{js}"
        );
        assert!(
            parses_as_js(&js),
            "[{label}] emitted module must parse as JS:\n{js}"
        );
    }
}

#[test]
fn double_paren_receiver_state_snapshot_wellformed_rewrites_to_dollar_snapshot() {
    // DOUBLED receiver parens `(($state)).snapshot(o)` — the shared peel is a
    // loop, so every nesting depth reports the same member surface as the plain
    // spelling (oracle-verified accept against svelte@5.56.10: `$.snapshot(o)`).
    let js = emit(
        "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = (($state)).snapshot(o)}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snapshot(o)"),
        "`(($state)).snapshot(o)` rewrites to `$.snapshot(o)`:\n{js}"
    );
    assert!(
        !js.contains("$state") && !js.contains("($.snapshot)"),
        "no raw `$state` / paren residue may remain:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn whole_call_paren_state_snapshot_stays_accepted() {
    // Parens around the WHOLE call `($state.snapshot(o))` — the wrapper is a
    // transparent expression the walk descends through, so the call exemption and
    // the callee rewrite own the inner call unchanged (oracle-verified accept
    // against svelte@5.56.10; official prints `$.snapshot(o)` — a surviving
    // behavior-preserving wrapper paren is cosmetic, never contract).
    let js = emit(
        "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state.snapshot(o))}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snapshot(o)"),
        "`($state.snapshot(o))` keeps the `$.snapshot(o)` call topology:\n{js}"
    );
    assert!(
        !js.contains("$state") && !js.contains("($.snapshot)"),
        "no raw `$state` / callee-paren residue may remain:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn paren_callee_state_snapshot_malformed_fails_closed_as_rune() {
    // A MALFORMED snapshot call at EVERY paren position — zero args / two args
    // (official `rune_invalid_arguments_length`) or a spread argument (official
    // `rune_invalid_spread`), oracle-verified against svelte@5.56.10 for the
    // whole-callee-paren, doubled-receiver-paren, and whole-call-paren spellings —
    // refuses under the SAME `$state.snapshot` label as the plain spellings; the
    // paren transparency must never let a malformed form slip past the exemption
    // into a raw `$.snapshot()` / `$.snapshot(o, o)` / `$.snapshot(...arr)`.
    let cases: &[(&str, &str)] = &[
        (
            "callee_paren_zero_args",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state.snapshot)()}>x</button>\n",
        ),
        (
            "callee_paren_two_args",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state.snapshot)(o, o)}>x</button>\n",
        ),
        (
            "callee_paren_spread_arg",
            "<script>let arr = $state([1]);\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state.snapshot)(...arr)}>x</button>\n",
        ),
        (
            "double_receiver_paren_zero_args",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = (($state)).snapshot()}>x</button>\n",
        ),
        (
            "double_receiver_paren_spread_arg",
            "<script>let arr = $state([1]);\nlet snap = $state(null);</script>\n<button onclick={() => snap = (($state)).snapshot(...arr)}>x</button>\n",
        ),
        (
            "whole_call_paren_zero_args",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state.snapshot())}>x</button>\n",
        ),
        (
            "whole_call_paren_spread_arg",
            "<script>let arr = $state([1]);\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state.snapshot(...arr))}>x</button>\n",
        ),
    ];
    for (label, source) in cases {
        assert_fail_closed_labeled(label, source, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.snapshot"
            )
        });
    }
}

#[test]
fn state_nan_proxies_but_raw_nan_does_not() {
    // F3: `$state(NaN)` is a bare global-identifier init official PROXIES →
    // `$.state($.proxy(NaN))`. `$state.raw(NaN)` NEVER proxies → `$.state(NaN)`. The
    // discriminating emission (the fail-matrix retag only checked the shape, not the
    // output): assert the proxy wrap IS present for proxied NaN and ABSENT for raw NaN.
    let proxied = emit(
        "<script>let x = $state(NaN);</script>\n<button onclick={() => x = 1}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        proxied.contains("let x = $.state($.proxy(NaN));"),
        "$state(NaN) reassigned proxies to `$.state($.proxy(NaN))`:\n{proxied}"
    );
    // NEGATIVE: the reassign to a primitive `1` carries NO proxy `, true` flag.
    assert!(
        proxied.contains("$.set(x, 1)") && !proxied.contains("$.set(x, 1, true)"),
        "the primitive reassign is a bare `$.set(x, 1)`:\n{proxied}"
    );

    let raw = emit(
        "<script>let x = $state.raw(NaN);</script>\n<button onclick={() => x = 1}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        raw.contains("let x = $.state(NaN);"),
        "$state.raw(NaN) reassigned is a bare `$.state(NaN)`:\n{raw}"
    );
    // NEGATIVE: a raw NaN must never proxy.
    assert!(
        !raw.contains("$.proxy"),
        "$state.raw(NaN) must never emit `$.proxy`:\n{raw}"
    );
}

#[test]
fn state_raw_no_arg_reassigned_emits_state_void_0() {
    // F8: a no-arg `$state.raw()` reassigned emits `$.state(void 0)` — byte-identical to
    // the no-arg `$state()` form (official emits `$.state(void 0)` for both, verified
    // against the oracle). NO proxy, and the shadow-robust `void 0` (never the bare
    // identifier `undefined`).
    let raw = emit(
        "<script>let x = $state.raw();</script>\n<button onclick={() => x = 1}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        raw.contains("let x = $.state(void 0);"),
        "no-arg $state.raw() reassigned emits `$.state(void 0)`:\n{raw}"
    );
    assert!(
        !raw.contains("$.proxy") && !raw.contains("$.state(undefined)"),
        "no-arg $state.raw() must not proxy nor use the bare `undefined` identifier:\n{raw}"
    );
    // The plain `$state()` no-arg form matches byte-for-byte.
    let plain = emit(
        "<script>let x = $state();</script>\n<button onclick={() => x = 1}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        plain.contains("let x = $.state(void 0);"),
        "no-arg $state() reassigned emits `$.state(void 0)`:\n{plain}"
    );
}

#[test]
fn state_over_plain_local_undefined_shadow_emits() {
    // F5: a `$state(undefined)` over a PLAIN-local `undefined` shadow (`let undefined =
    // 5`) is non-reactive — it reads plain and would lower to `$.state(undefined)`, so
    // the `$state`-shape gate no longer over-refuses it (proven at the unit level by
    // `state_init_reactive_shadowed_undefined_conservative_failclose_discriminates_subcases`).
    //
    // DISCRIMINATING at the full-module level: before F5 this component failed closed
    // as an `AdvancedRune` ("$state() shadowed undefined init") at the state gate;
    // after F5 the state gate PASSES and the disposition MOVES to the ORTHOGONAL
    // plain-let carrier (`InstanceScriptItem { construct: "plain let" }` for the bare
    // `let undefined = 5;`, a pre-existing unsupported instance-script item unrelated
    // to `$state`). So the failure is no longer a rune refusal.
    let js = emit(
        "<script>let undefined = 5;\nlet x = $state(undefined);</script>\n<button onclick={() => x = 1}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let undefined = 5;"),
        "shadowing local missing:\n{js}"
    );
    assert!(
        js.contains("let x = $.state(undefined);"),
        "shadowed undefined must be treated as the local value:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_malformed_calls_fail_closed() {
    // Malformed family calls — wrong arity (official `rune_invalid_arguments_length`
    // / `rune_invalid_arguments`) or a spread argument (official
    // `rune_invalid_spread`) — fail closed under the precise family label, never
    // slipping past the call exemption into a raw `$.user_effect()` /
    // `$.effect_tracking(x)` miscompile.
    let cases: &[(&str, &str, &str)] = &[
        (
            "effect_zero_args",
            "<script>let c = $state(0); $effect();</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect",
        ),
        (
            "effect_two_args",
            "<script>let c = $state(0); $effect(a, b);</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect",
        ),
        (
            "effect_spread_arg",
            "<script>let c = $state(0); $effect(...args);</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect",
        ),
        (
            "pre_zero_args",
            "<script>let c = $state(0); $effect.pre();</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect.pre",
        ),
        (
            "tracking_with_arg",
            "<script>let c = $state(0); const t = $effect.tracking(c);</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect.tracking",
        ),
        (
            "root_zero_args",
            "<script>let c = $state(0); const stop = $effect.root();</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect.root",
        ),
    ];
    for (label, source, rune_label) in cases {
        assert_fail_closed_labeled(
            label,
            source,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if rune == rune_label),
        );
    }
}

#[test]
fn effect_unknown_member_still_fails_closed_via_generic_fallback() {
    // `$effect.foo()` is not a family member — the generic `$effect.<member>`
    // fallback owns it (official `rune_invalid_name`), unchanged by the family
    // call exemption.
    assert_fail_closed(
        "<script>let c = $state(0); $effect.foo(() => {});</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$effect.<member>"),
    );
}

#[test]
fn effect_pending_still_fails_closed_experimental_async() {
    // `$effect.pending` is the experimental-async member (5j) — it must NOT ride
    // the family call exemption; both the called and uncalled forms stay on the
    // `ExperimentalAsync` refusal.
    assert_fail_closed(
        "<script>let c = $state(0); const p = $effect.pending();</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ExperimentalAsync { surface, .. } if *surface == "$effect.pending"),
    );
    assert_fail_closed(
        "<script>let c = $state(0); const p = $effect.pending;</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ExperimentalAsync { surface, .. } if *surface == "$effect.pending"),
    );
}

#[test]
fn paren_receiver_effect_pending_fails_closed_experimental_async() {
    // Receiver parens are TRANSPARENT to official (its ESTree AST has no paren
    // nodes), so `($effect).pending` is the SAME experimental-async member
    // surface as `$effect.pending` — both the called and uncalled spellings
    // refuse under the `ExperimentalAsync` label, never the coarse bare-`$effect`
    // position refusal the receiver identifier would record on its own.
    assert_fail_closed(
        "<script>let c = $state(0); const p = ($effect).pending();</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ExperimentalAsync { surface, .. } if *surface == "$effect.pending"),
    );
    assert_fail_closed(
        "<script>let c = $state(0); const p = ($effect).pending;</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ExperimentalAsync { surface, .. } if *surface == "$effect.pending"),
    );
}

#[test]
fn paren_receiver_effect_unknown_member_fails_closed_via_generic_fallback() {
    // `($effect).foo()` is the same unknown member as `$effect.foo()` (official
    // `rune_invalid_name` — oracle-verified against svelte@5.56.10 for the
    // parenthesized spelling too): the generic `$effect.<member>` fallback owns
    // it, not the coarse bare-`$effect` position refusal.
    assert_fail_closed(
        "<script>let c = $state(0); ($effect).foo(() => {});</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$effect.<member>"),
    );
}

#[test]
fn paren_receiver_effect_family_uncalled_members_fail_closed() {
    // UNCALLED family members behind a parenthesized receiver refuse under the
    // SAME precise member labels as the plain spellings (official
    // `rune_missing_parentheses` for every one of them — oracle-verified against
    // svelte@5.56.10), never the coarse bare-`$effect` position refusal.
    let cases: &[(&str, &str, &str)] = &[
        (
            "paren_uncalled_pre_member",
            "<script>let c = $state(0); const f = ($effect).pre;</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect.pre",
        ),
        (
            "paren_uncalled_root_member",
            "<script>let c = $state(0); const f = ($effect).root;</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect.root",
        ),
        (
            "paren_uncalled_tracking_member",
            "<script>let c = $state(0); const f = ($effect).tracking;</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect.tracking",
        ),
    ];
    for (label, source, rune_label) in cases {
        assert_fail_closed_labeled(
            label,
            source,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if rune == rune_label),
        );
    }
}

#[test]
fn paren_receiver_effect_pre_statement_call_still_accepts() {
    // The ACCEPT control for the member-receiver paren transparency: a
    // statement-position `($effect).pre(fn)` call is the SAME statement as
    // `$effect.pre(fn)` to official (oracle-verified: svelte@5.56.10 emits
    // `$.user_pre_effect` with the component frame) and must keep lowering —
    // the member-form refusal classification owns ONLY uncalled references,
    // never an admitted family call (whose callee the call visitor consumes).
    let js = emit(
        "<script>let c = $state(0); ($effect).pre(() => { console.log(c); });</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_pre_effect(() => {"),
        "the parenthesized-receiver `($effect).pre(fn)` statement lowers to `$.user_pre_effect`:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);") && js.contains("$.pop();"),
        "the pre effect forces the component frame:\n{js}"
    );
    // NEGATIVE: the paren-inclusive callee span is overwritten — no raw rune and
    // no receiver-paren residue survive.
    assert!(
        !js.contains("$effect") && !js.contains("($.user_pre_effect"),
        "no raw `$effect` and no paren residue may remain:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_value_positions_fail_closed_statement_gate() {
    // Official svelte@5.56.10 rejects EVERY value-position `$effect(...)` /
    // `$effect.pre(...)` with `effect_invalid_placement` ("`$effect()` can only
    // be used as an expression statement") — the user-effect members are
    // STATEMENT-ONLY. Each value position below must FAIL CLOSED under the
    // precise family label: rewriting the callee there (`$.user_effect`) would
    // be fail-open against an official compile error. Oracle-verified across
    // the declarator-init, handler, root-body, and effect-body value positions.
    let cases: &[(&str, &str, &str)] = &[
        // An EXPRESSION-bodied handler arrow: the call is the arrow's concise
        // body (an expression position), not a statement.
        (
            "handler_concise_arrow_body",
            "<script>let x = $state(0);</script>\n<button onfocus={() => $effect(() => { console.log(x) })}>hi</button>\n<button onclick={() => x++}>{x}</button>\n",
            "$effect",
        ),
        // A declarator INIT inside an accepted `$effect.root` callback.
        (
            "root_body_declarator_init",
            "<script>let x = $state(0); const stop = $effect.root(() => { const s2 = $effect(() => { console.log(x) }); return () => {}; });</script>\n<button onclick={() => x++}>{x}</button>\n",
            "$effect",
        ),
        // A `return` argument inside an accepted `$effect.root` callback.
        (
            "root_body_return_argument",
            "<script>let x = $state(0); const stop = $effect.root(() => { return $effect(() => {}); });</script>\n<button onclick={() => x++}>{x}</button>\n",
            "$effect",
        ),
        // A CALL argument inside a `$effect.root` callback body.
        (
            "root_body_call_argument",
            "<script>let x = $state(0); $effect.root(() => { console.log($effect(() => {})); return () => {}; });</script>\n<button onclick={() => x++}>{x}</button>\n",
            "$effect",
        ),
        // A declarator INIT inside an accepted `$effect` body (the `.pre` label).
        (
            "effect_body_declarator_init_pre",
            "<script>let x = $state(0); $effect(() => { const q = $effect.pre(() => {}); });</script>\n<button onclick={() => x++}>{x}</button>\n",
            "$effect.pre",
        ),
        // A TOP-LEVEL declarator init: the position gate owns the refusal (its
        // precise family diagnostic wins over the generic `const declaration`
        // item refusal that previously caught it incidentally).
        (
            "toplevel_const_declarator",
            "<script>let x = $state(0); const e = $effect(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
            "$effect",
        ),
        (
            "toplevel_const_declarator_pre",
            "<script>let x = $state(0); const p = $effect.pre(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
            "$effect.pre",
        ),
        // A SEQUENCE expression statement: the call's parent is the sequence,
        // not the statement (official rejects — the ESTree direct-parent rule).
        (
            "sequence_statement",
            "<script>let x = $state(0); $effect(() => {}), x;</script>\n<button onclick={() => x++}>{x}</button>\n",
            "$effect",
        ),
    ];
    for (label, source, rune_label) in cases {
        assert_fail_closed_labeled(
            label,
            source,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if rune == rune_label),
        );
    }
}

#[test]
fn effect_root_bare_statement_lowers_without_frame() {
    // An UNASSIGNED bare `$effect.root(...);` expression statement — official
    // ACCEPTS it (oracle-verified): `$.effect_root(...)` as a bare statement, the
    // callback body rewritten, NO component frame (sig `($$anchor)`, no
    // `$.push` / `$.pop` — root alone never forces the frame). Verter's
    // statement carrier admits it exactly like the assigned form.
    let js = emit(
        "<script>\n\tlet x = $state(0);\n\t$effect.root(() => {\n\t\tconsole.log(x);\n\t\treturn () => {};\n\t});\n</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the bare root statement lowers:\n{js}"
    );
    assert!(
        js.contains("console.log($.get(x));"),
        "the root body's signal read rewrites:\n{js}"
    );
    assert!(
        js.contains("return () => {};"),
        "the cleanup return flows through verbatim:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {"),
        "root alone must NOT force the `$$props` param:\n{js}"
    );
    assert!(
        !js.contains("$.push") && !js.contains("$.pop"),
        "root alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_tracking_bare_statement_lowers_without_frame() {
    // An UNASSIGNED bare `$effect.tracking();` expression statement — official
    // ACCEPTS it (oracle-verified): `$.effect_tracking();` as a bare statement, NO
    // component frame. Verter's statement carrier admits it exactly like the
    // assigned declarator form.
    let js = emit(
        "<script>\n\tlet x = $state(0);\n\t$effect.tracking();\n</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_tracking();"),
        "the bare tracking statement lowers:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {"),
        "tracking alone must NOT force the `$$props` param:\n{js}"
    );
    assert!(
        !js.contains("$.push") && !js.contains("$.pop"),
        "tracking alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_tracking_const_attribute_read_joins_template_effect() {
    // A tracking-CONST read in an ATTRIBUTE value (`disabled={t}`) — official
    // wraps the property write in the template effect
    // (`$.template_effect(() => input.disabled = t)`): a call-init const cannot
    // be static-folded (`Identifier.js` `!is_known`), so its read is
    // `has_state`. The read stays PLAIN (`t`, never `$.get`) — the same
    // disposition the text path (`{t}`) already has. Oracle-verified against
    // svelte@5.56.10.
    let js = emit(
        "<script>\n\tconst t = $effect.tracking();\n</script>\n<input disabled={t} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("const t = $.effect_tracking();"),
        "the tracking const lowers:\n{js}"
    );
    assert!(
        js.contains("$.template_effect(") && js.contains("input.disabled = t"),
        "the const-read attribute write joins the template effect:\n{js}"
    );
    assert!(
        !js.contains("$.get(t)"),
        "the tracking const is NOT a signal read:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The combined-effect variant (a reactive text sibling): the write joins the
    // SAME region effect as the `$.set_text` — never a one-shot outside it.
    let js = emit(
        "<script>\n\tlet x = $state(0);\n\tconst t = $effect.tracking();\n</script>\n<input disabled={t} />\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("input.disabled = t"),
        "the const-read property write is emitted:\n{js}"
    );
    let effect_start = js
        .find("$.template_effect(")
        .expect("a template effect exists");
    let write_at = js.find("input.disabled = t").expect("the write exists");
    assert!(
        write_at > effect_start,
        "the write lands INSIDE the template effect, not as a one-shot before it:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_tracking_inline_attribute_memoizes_into_template_effect() {
    // An INLINE `$effect.tracking()` call in an ATTRIBUTE value — official
    // memoizes it into the deps-array template-effect form
    // (`$.template_effect(($0) => input.disabled = $0, [() => $.effect_tracking()])`):
    // `is_pure` explicitly special-cases `$effect.tracking` as IMPURE, so the
    // call is `has_call` (and re-evaluates INSIDE the tracking context — a
    // construction-time one-shot would return a DIFFERENT boolean, a SEMANTIC
    // divergence, never acceptable). Oracle-verified against svelte@5.56.10.
    let js = emit(
        "<script>\n\tconst t = $effect.tracking();\n</script>\n<input disabled={$effect.tracking()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.template_effect(") && js.contains("input.disabled = $0"),
        "the inline tracking call memoizes into the deps-array effect slot:\n{js}"
    );
    // The dep thunk body may carry Verter's unconditional arrow-body paren wrap
    // (`(EXPR)`) — a behavior-preserving redundant paren the structural
    // comparator waives (cosmetic carrier formatting, not topology).
    assert!(
        js.contains("[() => $.effect_tracking()]") || js.contains("[() => ($.effect_tracking())]"),
        "the memoized dep re-evaluates the tracking call inside the effect:\n{js}"
    );
    assert!(
        !js.contains("input.disabled = $.effect_tracking()"),
        "the inline tracking call is NEVER a construction-time one-shot (semantic divergence):\n{js}"
    );
    assert!(!js.contains("$effect."), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The combined variant (a reactive text sibling): the memoized write and the
    // `$.set_text` share ONE region effect with the deps array (oracle-verified).
    let js = emit(
        "<script>\n\tlet c = $state(0);\n</script>\n<input disabled={$effect.tracking()} />\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("input.disabled = $0")
            && (js.contains("[() => $.effect_tracking()]")
                || js.contains("[() => ($.effect_tracking())]")),
        "the combined effect memoizes the tracking dep:\n{js}"
    );
    assert!(
        !js.contains("input.disabled = $.effect_tracking()"),
        "no construction-time one-shot in the combined form:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_statement_positions_stay_accepted_paren_transparent() {
    // The PARITY CONTROL for the statement-position gate: official ESTree has no
    // parenthesized-expression nodes, so a paren-wrapped `($effect(fn));`
    // statement inside an accepted `$effect.root` callback body IS the same
    // expression statement (official's `effect_invalid_placement` is a
    // direct-parent rule over the paren-free AST) — the gate must admit it, not
    // refuse on the paren wrapper. (The bare block-bodied handler statement and
    // the root-with-nested-effect controls are pinned by
    // `effect_in_direct_handler_lowers_with_frame` and
    // `effect_root_is_assignable_expression_with_nested_effects_and_cleanup`.)
    let js = emit(
        "<script>\n\tlet c = $state(0);\n\tconst stop = $effect.root(() => {\n\t\t($effect(() => console.log(c)));\n\t\treturn () => {};\n\t});\n</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(() => console.log($.get(c)))"),
        "the paren-wrapped statement-position `$effect` still lowers:\n{js}"
    );
    assert!(
        !js.contains("$effect"),
        "no raw `$effect` rune survives:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_toplevel_statement_lowers_with_frame() {
    // A top-level `$effect(fn);` statement is a supported instance-script item:
    // it lowers to `$.user_effect(fn)` with the body rewritten and forces the
    // runes frame, matching svelte@5.56.10 (the `matrix/effect_arrow` topology).
    let js = emit(
        "<script>\n\tlet count = $state(0);\n\t$effect(() => { console.log(count); });\n</script>\n<button onclick={() => count++}>{count}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(() => {"),
        "the top-level `$effect` statement lowers:\n{js}"
    );
    assert!(
        js.contains("console.log($.get(count));"),
        "the effect body's signal read rewrites:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor, $$props) {")
            && js.contains("$.push($$props, true);")
            && js.contains("$.pop();"),
        "the `$effect` forces the runes frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_tracking_const_reads_plain_in_template_effect() {
    // The R4d topology: `const t = $effect.tracking();` + a template `{t}` read.
    // The const lowers to `$.effect_tracking()`; the template reads the PLAIN
    // const inside the region's `$.template_effect` (official cannot static-fold
    // a call-init const) — NOT `$.get(t)`, NOT a second `$.effect_tracking()` in
    // the template. NO frame: sig stays `($$anchor)`.
    let js = emit(
        "<script>\n\tconst t = $effect.tracking();\n</script>\n<p>{t}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const t = $.effect_tracking();"),
        "the tracking const lowers:\n{js}"
    );
    assert!(
        js.contains("$.template_effect(() => $.set_text(text, t));"),
        "the template reads the plain const inside the template effect:\n{js}"
    );
    assert!(
        !js.contains("$.get(t)"),
        "the tracking const is NOT a signal read:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {"),
        "tracking alone must NOT force the `$$props` param:\n{js}"
    );
    assert!(!js.contains("$.push"), "no frame open:\n{js}");
    assert!(!js.contains("$.pop"), "no frame close:\n{js}");
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The `let` declarator flavour is equally official-legal and preserves the
    // keyword (`let t = $.effect_tracking();`).
    let js = emit(
        "<script>\n\tlet t = $effect.tracking();\n</script>\n<p>{t}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let t = $.effect_tracking();"),
        "the let-declared tracking const preserves the keyword:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_tracking_inside_effect_body_lowers() {
    // `$effect.tracking()` inside an effect body rewrites through the same callee
    // rewrite (oracle: `console.log($.effect_tracking(), $.get(c))`); the frame
    // comes from the surrounding `$effect`.
    let js = emit(
        "<script>let c = $state(0); $effect(() => { console.log($effect.tracking(), c); });</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("console.log($.effect_tracking(), $.get(c));"),
        "the tracking call rewrites inside the effect body:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(() => {") && js.contains("$.push($$props, true);"),
        "the surrounding effect lowers with the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_framing_matrix() {
    // The five-row framing matrix (oracle-verified): only `$effect` /
    // `$effect.pre` — at any depth, including nested inside a root callback —
    // force the component frame; `$effect.root` / `$effect.tracking` alone never
    // do. Negative assertions per row: a frameless row has NO `$.push`, NO
    // `$.pop`, NO `$$props` param.
    let framed: &[(&str, &str)] = &[
        (
            "plain_effect_only",
            "<script>let c = $state(0); $effect(() => { console.log(c); });</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "pre_only",
            "<script>let c = $state(0); $effect.pre(() => { console.log(c); });</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "root_with_nested_effect",
            "<script>let c = $state(0); const stop = $effect.root(() => { $effect(() => console.log(c)); return () => {}; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "root_with_nested_pre",
            "<script>let c = $state(0); const stop = $effect.root(() => { $effect.pre(() => console.log(c)); return () => {}; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
    ];
    for (label, source) in framed {
        let js = emit(source, "App.svelte");
        assert!(
            js.contains("export default function App($$anchor, $$props) {"),
            "[{label}] the frame threads `$$props`:\n{js}"
        );
        assert!(
            js.contains("$.push($$props, true);") && js.contains("$.pop();"),
            "[{label}] the frame opens and closes:\n{js}"
        );
    }
    let frameless: &[(&str, &str)] = &[
        (
            "root_only",
            "<script>let c = $state(0); const stop = $effect.root(() => { console.log(c); return () => {}; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "tracking_only",
            "<script>const t = $effect.tracking();</script>\n<p>{t}</p>\n",
        ),
    ];
    for (label, source) in frameless {
        let js = emit(source, "App.svelte");
        assert!(
            js.contains("export default function App($$anchor) {"),
            "[{label}] no `$$props` param without a user effect:\n{js}"
        );
        assert!(
            !js.contains("$.push") && !js.contains("$.pop"),
            "[{label}] no frame without a user effect:\n{js}"
        );
        assert!(
            !js.contains("$$props"),
            "[{label}] no `$$props` threading at all:\n{js}"
        );
    }
}

#[test]
fn effect_in_iife_and_effect_in_effect_lower_recursively() {
    // An IIFE-hosted call position: a `$effect` nested inside an IIFE inside a lowered effect body
    // rewrites through the same recursive callee rewrite (oracle-verified:
    // `$.user_effect(() => { (() => { $.user_effect(...); })(); });`).
    let js = emit(
        "<script>let c = $state(0); $effect(() => { (() => { $effect(() => console.log(c)); })(); });</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("(() => {") && js.contains("$.user_effect(() => console.log($.get(c)));"),
        "the IIFE-nested `$effect` lowers:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // An effect-in-effect call position: an effect nested in an effect body (oracle-verified:
    // `$.user_effect(() => { $.user_effect(() => console.log($.get(c))); });`).
    let js = emit(
        "<script>let c = $state(0); $effect(() => { $effect(() => console.log(c)); });</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    let effect_count = js.matches("$.user_effect(").count();
    assert_eq!(
        effect_count, 2,
        "both the outer and the nested effect lower (found {effect_count}):\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_async_await_re_homes_to_experimental_async() {
    // The awaiting effect callbacks fail closed on the EXPERIMENTAL-ASYNC surface
    // (5j) — no longer the advanced-rune position refusal — in every carrier: the
    // plain statement, the `.pre` statement, and the nested-in-root form.
    let cases: &[(&str, &str)] = &[
        (
            "plain_async_await",
            "<script>let c = $state(0); $effect(async () => { await c; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "pre_async_await",
            "<script>let c = $state(0); $effect.pre(async () => { await c; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "root_nested_async_await",
            "<script>let c = $state(0); const stop = $effect.root(() => { $effect(async () => { await c; }); });</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
    ];
    for (label, source) in cases {
        assert_fail_closed_labeled(label, source, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ExperimentalAsync {
                    surface: "await",
                    ..
                }
            )
        });
    }
    // ORACLE-PARITY POSITIVE: an async callback with NO `await` accepts —
    // official emits `$.user_effect(async () => { $.get(c); })`; the await gate
    // fires on `await`, not on the `async` keyword.
    let js = emit(
        "<script>let c = $state(0); $effect(async () => { c; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(async () => {"),
        "the async-no-await effect lowers (oracle parity):\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_paren_statement_forms_lower_normalized() {
    // Official svelte@5.56.10 parses with an ESTree AST that has NO
    // parenthesized-expression nodes, so author parens around a WHOLE
    // effect-family call statement are transparent (oracle-verified: all four
    // members accept) and the emitted helper call carries NO wrapping parens.
    //
    // Plain `$effect` — frame forced.
    let js = emit(
        "<script>let x = $state(0); ($effect(() => { console.log(x) }));</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(() => {"),
        "the paren-wrapped `$effect` statement lowers:\n{js}"
    );
    assert!(
        !js.contains("($.user_effect"),
        "the source parens must NOT wrap the emitted helper call:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);") && js.contains("$.pop();"),
        "the paren-wrapped `$effect` still forces the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // `$effect.pre` — frame forced.
    let js = emit(
        "<script>let x = $state(0); ($effect.pre(() => { console.log(x) }));</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_pre_effect(() => {"),
        "the paren-wrapped `$effect.pre` statement lowers:\n{js}"
    );
    assert!(
        !js.contains("($.user_pre_effect"),
        "the source parens must NOT wrap the emitted helper call:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the paren-wrapped `$effect.pre` still forces the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Bare `$effect.root(...)` statement — NO frame.
    let js = emit(
        "<script>($effect.root(() => { return () => {}; }));</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the paren-wrapped bare `$effect.root` statement lowers:\n{js}"
    );
    assert!(
        !js.contains("($.effect_root"),
        "the source parens must NOT wrap the emitted helper call:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {") && !js.contains("$.push"),
        "root alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Bare `$effect.tracking()` statement — NO frame.
    let js = emit(
        "<script>($effect.tracking());</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_tracking();"),
        "the paren-wrapped bare `$effect.tracking` statement lowers:\n{js}"
    );
    assert!(
        !js.contains("($.effect_tracking"),
        "the source parens must NOT wrap the emitted helper call:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "tracking alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_paren_declarator_inits_lower_normalized() {
    // Author parens around a WHOLE root/tracking declarator INIT are transparent
    // (oracle-verified accept) and the emitted init carries no wrapping parens.
    let js = emit(
        "<script>const stop = ($effect.root(() => { return () => {}; }));</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const stop = $.effect_root(() => {"),
        "the paren-wrapped root init lowers assigned:\n{js}"
    );
    assert!(
        !js.contains("($.effect_root"),
        "the source parens must NOT wrap the emitted helper call:\n{js}"
    );
    assert!(
        js.contains("return () => {};"),
        "the cleanup flows through verbatim:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {") && !js.contains("$.push"),
        "root alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>const t = ($effect.tracking());</script>\n<p>{t}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const t = $.effect_tracking();"),
        "the paren-wrapped tracking init lowers assigned:\n{js}"
    );
    assert!(
        !js.contains("($.effect_tracking"),
        "the source parens must NOT wrap the emitted helper call:\n{js}"
    );
    assert!(
        js.contains("$.template_effect(() => $.set_text(text, t));"),
        "the tracking-const read joins the template effect (oracle topology):\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "tracking alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_paren_member_receivers_lower_normalized() {
    // Author parens around the member RECEIVER (`($effect).pre(...)`) are
    // transparent (oracle-verified: official accepts and normalizes) — the whole
    // parenthesized callee is replaced by the helper, so no paren survives.
    let js = emit(
        "<script>let x = $state(0); ($effect).pre(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_pre_effect(() => {"),
        "the paren-receiver `.pre` statement lowers:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);") && js.contains("$.pop();"),
        "the paren-receiver `.pre` still forces the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>const stop = ($effect).root(() => { return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const stop = $.effect_root(() => {"),
        "the paren-receiver `.root` init lowers assigned:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {") && !js.contains("$.push"),
        "root alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>const t = ($effect).tracking();</script>\n<p>{t}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const t = $.effect_tracking();"),
        "the paren-receiver `.tracking` init lowers assigned:\n{js}"
    );
    assert!(
        js.contains("$.template_effect(() => $.set_text(text, t));"),
        "the tracking-const read joins the template effect:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "tracking alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_paren_callee_forms_lower_normalized() {
    // Author parens around the whole CALLEE (`($effect)(...)` /
    // `($effect.root)(...)`) are transparent too (oracle-verified: official
    // accepts all four members and normalizes the emission). The rewrite replaces
    // the OUTERMOST callee span, so the parens never survive.
    let js = emit(
        "<script>let x = $state(0); ($effect)(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(() => {") && !js.contains("($.user_effect"),
        "the paren-callee `$effect` statement lowers normalized:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the paren-callee `$effect` still forces the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>let x = $state(0); ($effect.pre)(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_pre_effect(() => {") && !js.contains("($.user_pre_effect"),
        "the paren-callee `.pre` statement lowers normalized:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the paren-callee `.pre` still forces the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>const stop = ($effect.root)(() => { return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const stop = $.effect_root(() => {") && !js.contains("($.effect_root"),
        "the paren-callee `.root` init lowers normalized:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "root alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>const t = ($effect.tracking)();</script>\n<p>{t}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const t = $.effect_tracking();") && !js.contains("($.effect_tracking"),
        "the paren-callee `.tracking` init lowers normalized:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "tracking alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_optional_call_root_tracking_lower_normalized() {
    // Official svelte@5.56.10 ACCEPTS an optional-CALL `$effect.root?.(...)` /
    // `$effect.tracking?.()` in statement AND init positions and NORMALIZES the
    // `?.` away (`$.effect_root(...)` / `$.effect_tracking()` — plain, no `?.`)
    // — oracle-verified. A `$.effect_root?.(...)` emission would be a structural
    // divergence, so the no-`?.` assertions are load-bearing.
    //
    // Statement forms.
    let js = emit(
        "<script>$effect.root?.(() => { return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the optional-call bare root statement lowers:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head is normalized away (no `?.` in the emission):\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "root alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>$effect.tracking?.();</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_tracking();"),
        "the optional-call bare tracking statement lowers:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head is normalized away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Declarator-init forms.
    let js = emit(
        "<script>const s = $effect.root?.(() => { return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(() => {"),
        "the optional-call root init lowers assigned:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head is normalized away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>const t = $effect.tracking?.();</script>\n<p>{t}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const t = $.effect_tracking();"),
        "the optional-call tracking init lowers assigned:\n{js}"
    );
    assert!(
        js.contains("$.template_effect(() => $.set_text(text, t));"),
        "the tracking-const read joins the template effect:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head is normalized away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_optional_member_receivers_lower_normalized() {
    // Official svelte@5.56.10 ACCEPTS the optional MEMBER receiver forms
    // (`$effect?.root(...)` / `$effect?.tracking()`) for the expression-valued
    // members and normalizes the `?.` away (oracle-verified) — the `?.` sits
    // inside the replaced callee span. (The user-effect members REJECT every
    // optional form — pinned in the fail matrix.)
    let js = emit(
        "<script>$effect?.root(() => { return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the optional-receiver bare root statement lowers:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional receiver is normalized away:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "root alone must NOT force the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>const s = $effect?.root(() => { return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(() => {"),
        "the optional-receiver root init lowers assigned:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional receiver is normalized away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    let js = emit(
        "<script>const t = $effect?.tracking();</script>\n<p>{t}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const t = $.effect_tracking();"),
        "the optional-receiver tracking init lowers assigned:\n{js}"
    );
    assert!(
        js.contains("$.template_effect(() => $.set_text(text, t));"),
        "the tracking-const read joins the template effect:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional receiver is normalized away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Author parens around the callee INSIDE an optional call
    // (`($effect.tracking)?.()`) compose with the head normalization
    // (oracle-verified accept).
    let js = emit(
        "<script>const t = ($effect.tracking)?.();</script>\n<p>{t}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const t = $.effect_tracking();"),
        "the paren-callee optional-call tracking init lowers assigned:\n{js}"
    );
    assert!(
        !js.contains("?.") && !js.contains("($.effect_tracking"),
        "both the parens and the optional head normalize away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_head_rewrites_preserve_comment_trivia() {
    // The invocation-head rewrites are TRIVIA-PRESERVING with ONE canonical
    // slot: comment trivia inside an overwritten head range re-emits INSIDE
    // the emitted helper call, immediately after the opening paren — NEVER
    // call-leading. Call-leading relocation is unsafe, not cosmetic: a leading
    // `/*#__PURE__*/` would ANNOTATE the emitted helper call (a minifier may
    // then drop the effect registration as pure), and a leading `//` line
    // comment after `return` arms ASI against the emitted call. Official
    // svelte@5.56.10 reattaches these comments to unrelated neighboring nodes
    // (esrap trivia reattachment) — matching that placement would be
    // cosmetic-carrier mimicry; INERT call-internal survival is the contract.
    //
    // Plain arg-leading: a comment already after `(` stays in place, exactly
    // once (the head overwrite ends at the opening paren and re-emits only
    // head-interior trivia — never a duplicate).
    let js = emit(
        "<script>let x = $state(0); $effect(/*#__PURE__*/ () => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(/*#__PURE__*/ () => {"),
        "a plain arg-leading annotation survives in place:\n{js}"
    );
    assert_eq!(
        js.matches("/*#__PURE__*/").count(),
        1,
        "a plain arg-leading annotation is preserved exactly once:\n{js}"
    );
    assert!(
        !js.contains("/*#__PURE__*/ $.user_effect"),
        "the annotation is never call-leading:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Callee-to-paren gap: `$effect /*KEEP*/ (fn)` — the head rewrite runs
    // through the opening call paren, so the gap comment relocates into the
    // helper call (neither call-leading nor stranded between helper and paren).
    let js = emit(
        "<script>let x = $state(0); $effect /*KEEP*/ (() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(/*KEEP*/ () => {"),
        "a callee-to-paren gap comment relocates into the helper call:\n{js}"
    );
    assert!(
        !js.contains("/*KEEP*/ $.user_effect") && !js.contains("$.user_effect /*KEEP*/"),
        "the gap comment is neither call-leading nor left in the gap:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Member-gap annotation: `$effect /*#__PURE__*/ .pre(fn)` — the annotation
    // sits inside the replaced head range and must land INSIDE the helper
    // parens (call-leading would pure-mark the pre-effect registration).
    let js = emit(
        "<script>let x = $state(0); $effect /*#__PURE__*/ .pre(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_pre_effect(/*#__PURE__*/ () => {"),
        "a member-gap annotation relocates into the helper call:\n{js}"
    );
    assert!(
        !js.contains("/*#__PURE__*/ $.user_pre_effect"),
        "the annotation is never call-leading:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Peeled callee parens: `(/*#__PURE__*/ $effect)(fn)` — the paren-inclusive
    // callee span is replaced; its interior annotation lands inside the call.
    let js = emit(
        "<script>let x = $state(0); (/*#__PURE__*/ $effect)(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(/*#__PURE__*/ () => {"),
        "a callee-paren annotation relocates into the helper call:\n{js}"
    );
    assert!(
        !js.contains("/*#__PURE__*/ $.user_effect") && !js.contains("($.user_effect"),
        "the annotation is never call-leading and the callee parens normalize away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Optional-head arg-leading annotation:
    // `$effect.root?.(/*#__PURE__*/ fn)` — the head overwrite ends at the
    // opening paren, so the annotation survives in place ahead of the
    // argument, exactly once (the head relocation never duplicates it).
    let js = emit(
        "<script>const s = $effect.root?.(/*#__PURE__*/ () => { return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(/*#__PURE__*/ () => {"),
        "an arg-leading annotation survives in place through the optional-head normalization:\n{js}"
    );
    assert_eq!(
        js.matches("/*#__PURE__*/").count(),
        1,
        "an arg-leading annotation is preserved exactly once (never duplicated):\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head still normalizes away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Zero-arg optional tracking: the head overwrite ends AT the opening paren
    // — never the whole call — so an annotation INSIDE the empty parens
    // survives in place: `$effect.tracking?.(/*#__PURE__*/)` →
    // `$.effect_tracking(/*#__PURE__*/)`.
    let js = emit(
        "<script>const t = $effect.tracking?.(/*#__PURE__*/);</script>\n<p>{t}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const t = $.effect_tracking(/*#__PURE__*/);"),
        "a zero-arg interior annotation survives inside the empty helper parens:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head still normalizes away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Return-position ASI safety: `return $effect //KEEP\n .root(fn)` must not
    // emit `return //…` (ASI would return undefined and orphan the call) — the
    // line comment relocates INSIDE the helper call, so the `return` keeps the
    // helper call as its argument on the same line.
    let js = emit(
        "<script>const stop = $effect.root(() => { return $effect //KEEP\n .root(() => { return () => {}; }); });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        !js.contains("return //"),
        "a head line comment must never become a return-leading comment (ASI):\n{js}"
    );
    assert!(
        js.contains("return $.effect_root(//KEEP\n"),
        "the return keeps the helper call as its argument, comment inside the call:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Paren-in-comment mask: the head-open-paren scan is comment-MASKED — a
    // `(` INSIDE a gap comment is never the call-paren token. `$effect /*(*/
    // (fn)` must rewrite through the REAL opening paren after the comment (an
    // unmasked scan would end the head inside the comment and mangle the
    // emission).
    let js = emit(
        "<script>let x = $state(0); $effect /*(*/ (() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(/*(*/ () => {"),
        "a `(`-bearing gap comment is masked and relocates call-internal:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The optional-call gap composes with the mask:
    // `$effect.root ?./*(*/ (fn)` — the `(` inside the comment is not the
    // paren token; the optional head normalizes plain with the comment
    // call-internal.
    let js = emit(
        "<script>const s = $effect.root ?./*(*/ (() => { return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(/*(*/ () => {"),
        "the optional-gap `(`-bearing comment is masked and relocates call-internal:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head still normalizes away:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // No-comment controls: the comment-free heads emit without trivia
    // artifacts (no stray interior drift on either rewrite arm).
    let js = emit(
        "<script>let x = $state(0); $effect.pre(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_pre_effect(() => {") && !js.contains("/*KEEP*/"),
        "a comment-free member head emits without trivia artifacts:\n{js}"
    );
    let js = emit(
        "<script>const s = $effect.root?.(() => { return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(() => {") && !js.contains("?."),
        "a comment-free optional head emits without trivia artifacts:\n{js}"
    );
}

#[test]
fn effect_family_wrapper_paren_head_trivia_relocates_inertly() {
    // A transparent author-paren WRAPPER around a carried effect-family call
    // (`(/*#__PURE__*/ $effect(fn));`) is normalized away by the instance-item
    // carrier slice (official's ESTree AST has no paren nodes), but its
    // interior HEAD trivia must NOT be silently dropped with it: the carrier
    // pre-collects the wrapper-head comments and the rewriter re-emits them
    // INSIDE the emitted helper call — never call-leading (a leading
    // `/*#__PURE__*/` would pure-mark the effect registration itself).
    let js = emit(
        "<script>let x = $state(0); (/*#__PURE__*/ $effect(() => { console.log(x) }));</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(/*#__PURE__*/ () => {"),
        "the wrapper-head annotation relocates inertly into the helper call:\n{js}"
    );
    assert!(
        !js.contains("/*#__PURE__*/ $.user_effect") && !js.contains("($.user_effect"),
        "the annotation is never call-leading and the wrapper parens stay normalized away:\n{js}"
    );
    assert_eq!(
        js.matches("/*#__PURE__*/").count(),
        1,
        "the wrapper-head annotation survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The assignable declarator-init carrier normalizes the same wrapper shape
    // (`const stop = (/*#__PURE__*/ $effect.root(fn));`) — same inert slot.
    let js = emit(
        "<script>const stop = (/*#__PURE__*/ $effect.root(() => { return () => {}; }));</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const stop = $.effect_root(/*#__PURE__*/ () => {"),
        "the init wrapper-head annotation relocates inertly into the helper call:\n{js}"
    );
    assert!(
        !js.contains("/*#__PURE__*/ $.effect_root") && !js.contains("($.effect_root"),
        "the annotation is never call-leading on the init carrier either:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_general_path_wrapper_head_trivia_relocates_inertly() {
    // The GENERAL expression-rewrite path (handler bodies, nested function /
    // effect / root bodies — everywhere OUTSIDE the top-level instance-item
    // carriers) keeps a transparent author-paren WRAPPER around an
    // effect-family call (behavior-preserving redundant parens are waived),
    // but its wrapper-GAP comments must not stay in their source slot: left
    // between the wrapper `(` and the rewritten helper call they would sit
    // call-leading (a `/*#__PURE__*/` would pure-mark the effect registration
    // — a minifier could then drop it). The gap comments are REMOVED from the
    // gap and re-emitted INSIDE the emitted helper call, ahead of the head's
    // own trivia (source order) — the same canonical call-internal slot the
    // carriers use.
    //
    // Handler position: `(/*#__PURE__*/ $effect.root(fn));` inside a
    // delegated onclick block arrow.
    let js = emit(
        "<script>let x = $state(0);</script>\n<button onclick={() => { (/*#__PURE__*/ $effect.root(() => { return () => {}; })); x++; }}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_root(/*#__PURE__*/ () => {"),
        "the handler wrapper-gap annotation relocates inertly into the helper call:\n{js}"
    );
    assert!(
        !js.contains("/*#__PURE__*/ $.effect_root"),
        "the annotation is never left call-leading inside the surviving wrapper:\n{js}"
    );
    assert_eq!(
        js.matches("/*#__PURE__*/").count(),
        1,
        "the wrapper-gap annotation survives exactly once (never duplicated):\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Nested-in-root-body occurrence: the wrapped user-effect statement inside
    // an accepted `$effect.root` callback rides the SAME relocation (the root
    // init is a carrier, but its BODY statements lower through the general
    // rewriter path).
    let js = emit(
        "<script>const stop = $effect.root(() => { (/*#__PURE__*/ $effect(() => { console.log(1) })); return () => {}; });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(/*#__PURE__*/ () => {"),
        "the nested wrapper-gap annotation relocates inertly into the nested helper call:\n{js}"
    );
    assert!(
        !js.contains("/*#__PURE__*/ $.user_effect"),
        "the nested annotation is never call-leading:\n{js}"
    );
    assert_eq!(
        js.matches("/*#__PURE__*/").count(),
        1,
        "the nested wrapper-gap annotation survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Nested wrappers: `(/*a*/ (/*b*/ $effect.root(fn)));` — every wrapper-gap
    // comment collects, in source order, into the one call-internal slot.
    let js = emit(
        "<script>let x = $state(0);</script>\n<button onclick={() => { (/*a*/ (/*b*/ $effect.root(() => { return () => {}; }))); x++; }}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_root(/*a*/ /*b*/ () => {"),
        "nested wrapper-gap comments collect in source order into the helper call:\n{js}"
    );
    assert_eq!(
        js.matches("/*a*/").count(),
        1,
        "the outer-gap comment survives exactly once:\n{js}"
    );
    assert_eq!(
        js.matches("/*b*/").count(),
        1,
        "the inner-gap comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Wrapper TAIL disposition on the general path: the wrapper parens
    // SURVIVE here (no carrier normalization), so a comment between the call
    // end and the wrapper `)` already sits inertly inside the parens — it
    // stays in place, untouched and never duplicated.
    let js = emit(
        "<script>let x = $state(0);</script>\n<button onclick={() => { ($effect.root(() => { return () => {}; }) /*!tail*/); x++; }}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*!tail*/)"),
        "a general-path wrapper TAIL comment stays in place inside the surviving parens:\n{js}"
    );
    assert_eq!(
        js.matches("/*!tail*/").count(),
        1,
        "the tail comment survives exactly once (never duplicated):\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // COMBINED head + tail on one general-path wrapper: the two dispositions
    // compose — the gap annotation relocates call-internal, the tail comment
    // stays in place inside the surviving parens, each exactly once.
    let js = emit(
        "<script>let x = $state(0);</script>\n<button onclick={() => { (/*#__PURE__*/ $effect.root(() => { return () => {}; }) /*!lic*/); x++; }}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_root(/*#__PURE__*/ () => {"),
        "the combined wrapper's head annotation relocates call-internal:\n{js}"
    );
    assert!(
        js.contains(") /*!lic*/)"),
        "the combined wrapper's tail comment stays in place inside the surviving parens:\n{js}"
    );
    assert!(
        !js.contains("/*#__PURE__*/ $.effect_root"),
        "the combined head annotation is never left call-leading:\n{js}"
    );
    assert_eq!(
        js.matches("/*#__PURE__*/").count(),
        1,
        "the combined head annotation survives exactly once:\n{js}"
    );
    assert_eq!(
        js.matches("/*!lic*/").count(),
        1,
        "the combined tail comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // A LINE-comment wrapper gap (`(//w⏎ $effect.root(fn))`) relocates with
    // its newline terminator, so the helper call's argument is never
    // commented out.
    let js = emit(
        "<script>let x = $state(0);</script>\n<button onclick={() => { (//w\n$effect.root(() => { return () => {}; })); x++; }}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.effect_root(//w\n"),
        "the line-comment wrapper gap relocates call-internal with its newline terminator:\n{js}"
    );
    assert_eq!(
        js.matches("//w").count(),
        1,
        "the line-comment gap survives exactly once:\n{js}"
    );
    assert!(
        js.contains("return () => {};"),
        "the argument body still lowers live (never commented out):\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_carrier_wrapper_tail_trivia_survives() {
    // The instance-item carriers normalize a transparent author-paren WRAPPER
    // away (the carried slice is the CALL span), so a comment in the wrapper
    // TAIL range — between the call end and the wrapper `)` — must not be
    // silently dropped with the parens: `($effect.root(fn) /*!license*/);`
    // carries a license-class comment, which stays in contract. The carrier
    // pre-renders the tail trivia and the projection re-emits it AFTER the
    // rewritten call payload, before the generated `;`.
    //
    // Statement carrier (bare unassigned root):
    let js = emit(
        "<script>($effect.root(() => { return () => {}; }) /*!license*/);</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the statement wrapper-tail license comment re-emits after the call payload, before the generated `;`:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the statement tail comment survives exactly once (never duplicated):\n{js}"
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the wrapped root statement still lowers through the carrier:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Declarator-init carrier: `const s = ($effect.root(fn) /*!license*/);`.
    let js = emit(
        "<script>const s = ($effect.root(() => { return () => {}; }) /*!license*/);</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(() => {"),
        "the wrapped root init still lowers assigned through the carrier:\n{js}"
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the init wrapper-tail license comment re-emits after the call payload, before the generated `;`:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the init tail comment survives exactly once (never duplicated):\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // A LINE-comment tail keeps its newline terminator, so the generated `;`
    // can never be commented out: `($effect(fn) // note\n);`.
    let js = emit(
        "<script>let x = $state(0); ($effect(() => { console.log(x) }) // note\n);</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("// note\n;"),
        "a line-comment tail re-emits with its newline terminator ahead of the generated `;`:\n{js}"
    );
    assert_eq!(
        js.matches("// note").count(),
        1,
        "the line-comment tail survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Head + tail combined on one wrapper: the two ranges are disjoint — the
    // head annotation relocates call-internal, the tail comment re-emits after
    // the payload, neither duplicates.
    let js = emit(
        "<script>let x = $state(0); (/*#__PURE__*/ $effect(() => { console.log(x) }) /*!license*/);</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(/*#__PURE__*/ () => {"),
        "the combined wrapper's head annotation still relocates call-internal:\n{js}"
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the combined wrapper's tail comment still re-emits after the call payload:\n{js}"
    );
    assert_eq!(
        js.matches("/*#__PURE__*/").count(),
        1,
        "the combined head annotation survives exactly once:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the combined tail comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_carrier_unwrapped_tail_trivia_survives() {
    // The carrier TAIL range runs from the CALL end to the ENCLOSING statement
    // end — NOT merely to a transparent wrapper's `)` — so a comment trailing
    // an UNWRAPPED carried call (`$effect.root(fn) /*!license*/;`: between the
    // call end and the statement `;`, no wrapper anywhere) survives exactly
    // like the wrapped form's interior tail: pre-rendered by the carrier,
    // re-emitted AFTER the rewritten call payload, before the generated `;`.
    //
    // Statement carrier (bare unassigned root, no wrapper):
    let js = emit(
        "<script>$effect.root(() => { return () => {}; }) /*!license*/;</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the unwrapped statement tail license comment re-emits after the call payload, before the generated `;`:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the unwrapped statement tail comment survives exactly once:\n{js}"
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the unwrapped root statement still lowers through the carrier:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Declarator-init carrier, no wrapper: `const s = $effect.root(fn) /*!license*/;`.
    let js = emit(
        "<script>const s = $effect.root(() => { return () => {}; }) /*!license*/;</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(() => {"),
        "the unwrapped root init still lowers assigned through the carrier:\n{js}"
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the unwrapped init tail license comment re-emits after the call payload, before the generated `;`:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the unwrapped init tail comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // An unwrapped LINE-comment tail (the `;` must sit on the NEXT source
    // line — a same-line `;` would be inside the comment): the re-emission
    // keeps the newline terminator, so the generated `;` is never commented
    // out.
    let js = emit(
        "<script>let x = $state(0); $effect(() => { console.log(x) }) // note\n;</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("// note\n;"),
        "an unwrapped line-comment tail re-emits with its newline terminator ahead of the generated `;`:\n{js}"
    );
    assert_eq!(
        js.matches("// note").count(),
        1,
        "the unwrapped line-comment tail survives exactly once:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(() => {"),
        "the effect statement still lowers through the carrier:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Wrapped CONTROL: ONE tail range covers the wrapper interior AND the
    // post-wrapper segment uniformly — `($effect.root(fn) /*!in*/) /*!out*/;`
    // re-emits BOTH, in source order, each exactly once (the wrapped form
    // neither regresses nor duplicates).
    let js = emit(
        "<script>($effect.root(() => { return () => {}; }) /*!in*/) /*!out*/;</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*!in*/ /*!out*/ ;"),
        "the wrapper-interior and post-wrapper tail comments re-emit in source order before the generated `;`:\n{js}"
    );
    assert_eq!(
        js.matches("/*!in*/").count(),
        1,
        "the wrapper-interior tail comment survives exactly once (no duplication):\n{js}"
    );
    assert_eq!(
        js.matches("/*!out*/").count(),
        1,
        "the post-wrapper tail comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_carrier_asi_tail_trivia_survives() {
    // An ASI-TERMINATED (semicolon-less) carrier statement/declaration ends its
    // OXC span AT the call/init end, so an AST-span-only tail bound sees an
    // EMPTY tail range and a same-line trailing comment silently drops. The
    // carrier tail is LEXICAL: same-line trailing comments after the call end
    // collect up to an explicit `;`, an ASI line terminator, or EOF —
    // license-class trailing comments stay in contract (oracle-verified:
    // svelte@5.56.10 preserves every shape below).
    //
    // Statement carrier, ASI at EOF: `$effect.root(fn) /*!license*/`.
    let js = emit(
        "<script>$effect.root(() => { return () => {}; }) /*!license*/</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the ASI-terminated statement tail license comment re-emits after the call payload:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the ASI statement tail comment survives exactly once:\n{js}"
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the semicolon-less root statement still lowers through the carrier:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Declarator-init carrier, ASI at EOF: `const s = $effect.root(fn) /*!license*/`.
    let js = emit(
        "<script>const s = $effect.root(() => { return () => {}; }) /*!license*/</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(() => {"),
        "the semicolon-less root init still lowers assigned through the carrier:\n{js}"
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the ASI-terminated init tail license comment re-emits after the call payload:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the ASI init tail comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // ASI mid-script (a next statement follows on the next line): the same-line
    // tail collects, the next statement still lowers on its own.
    let js = emit(
        "<script>let x = $state(0); $effect.root(() => { return () => {}; }) /*!license*/\n$effect(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the mid-script ASI statement tail comment re-emits after the call payload:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the mid-script ASI tail comment survives exactly once:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(() => {"),
        "the next-line effect statement still lowers on its own:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // ASI line-comment tail at EOF: `$effect(fn) // note` — the re-emission
    // keeps the newline terminator, so the generated `;` is never commented
    // out.
    let js = emit(
        "<script>let x = $state(0); $effect(() => { console.log(x) }) // note</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("// note\n;"),
        "an ASI line-comment tail re-emits with its newline terminator ahead of the generated `;`:\n{js}"
    );
    assert_eq!(
        js.matches("// note").count(),
        1,
        "the ASI line-comment tail survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // ASI post-wrapper tail: `($effect.root(fn)) /*!license*/` — the lexical
    // tail is uniform across the wrapper boundary exactly like the explicit-`;`
    // form.
    let js = emit(
        "<script>($effect.root(() => { return () => {}; })) /*!license*/</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the ASI post-wrapper tail comment re-emits after the call payload:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the ASI post-wrapper tail comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Two same-line block comments, ASI at EOF: both collect, in source order,
    // each exactly once.
    let js = emit(
        "<script>$effect.root(() => { return () => {}; }) /*a*/ /*b*/</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*a*/ /*b*/ ;"),
        "both same-line ASI tail comments re-emit in source order:\n{js}"
    );
    assert_eq!(
        js.matches("/*a*/").count(),
        1,
        "the first ASI tail comment survives exactly once:\n{js}"
    );
    assert_eq!(
        js.matches("/*b*/").count(),
        1,
        "the second ASI tail comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_asi_tail_never_steals_next_statement_leading_trivia() {
    // The lexical carrier tail collects SAME-LINE trailing comments only — a
    // comment that begins after a line break is the NEXT statement's leading
    // trivia and must never be stolen into the previous carrier's tail
    // (oracle-verified: official attaches it ahead of the next statement).
    //
    // Unwrapped next-line lead: `$effect.root(fn)⏎ /*lead*/ $effect(...)` —
    // the comment never lands in the root's tail slot.
    let js = emit(
        "<script>let x = $state(0); $effect.root(() => { return () => {}; })\n/*lead*/ $effect(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        !js.contains(") /*lead*/"),
        "a next-line leading comment is never collected into the previous carrier's tail:\n{js}"
    );
    assert!(
        js.contains("$.effect_root(() => {") && js.contains("$.user_effect(() => {"),
        "both statements still lower on their own:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Next-line comment INSIDE the next statement's call parens
    // (`⏎$effect(/*lead*/ () => …);` — a `(`-headed next statement would NOT
    // trigger ASI, so the comment rides inside the next CALL): it stays with
    // the next statement — verbatim inside that carrier's call slice — and
    // appears EXACTLY ONCE across the whole emission (a tail collector running
    // past the line break would steal a second copy into the root's tail).
    let js = emit(
        "<script>let x = $state(0); $effect.root(() => { return () => {}; })\n$effect(/*lead*/ () => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(/*lead*/"),
        "the next statement's leading comment stays with the next statement (call-internal):\n{js}"
    );
    assert!(
        !js.contains(") /*lead*/"),
        "the next statement's leading comment never ALSO lands in the previous carrier's tail:\n{js}"
    );
    assert_eq!(
        js.matches("/*lead*/").count(),
        1,
        "the next-statement leading comment survives exactly once across the whole emission:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Explicit-`;` stop: a comment AFTER the statement's own `;` is beyond the
    // carrier tail (next-region trivia) — the lexical collector stops at the
    // explicit terminator.
    let js = emit(
        "<script>$effect.root(() => { return () => {}; }); /*after*/</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        !js.contains(") /*after*/"),
        "a post-`;` trailing comment is never collected into the carrier tail:\n{js}"
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the root statement still lowers through the carrier:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // In-span control: a comment INSIDE the statement's own AST extent — after
    // a line break but before the explicit `;` (`$effect.root(fn)⏎ /*x*/;`) —
    // is the statement's OWN trailing trivia (never next-statement trivia) and
    // stays collected exactly as today (oracle-verified: official preserves
    // it). The same-line rule bounds only the lexical extension BEYOND the AST
    // span.
    let js = emit(
        "<script>$effect.root(() => { return () => {}; })\n/*x*/;</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*x*/ ;"),
        "an in-span multi-line tail comment (before the explicit `;`) stays collected:\n{js}"
    );
    assert_eq!(
        js.matches("/*x*/").count(),
        1,
        "the in-span multi-line tail comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_multiline_block_tail_never_steals_next_statement_lead_trivia() {
    // A same-line BLOCK comment whose TEXT holds a line terminator
    // (`/*tail⏎*/`) is ITSELF the statement's ASI terminator (ECMA-262: a
    // multi-line comment containing a line terminator is a LineTerminator to
    // the syntactic grammar) — it is the LAST tail comment: the statement
    // ends AT it, and everything beyond it is the NEXT statement's territory.
    // The next statement's tokens can sit newline-free on the terminator
    // comment's closing line, so a walk inspecting only the GAPS between
    // comments would run past the boundary and steal the next statement's
    // trivia into the previous carrier's tail.
    //
    // Between-statements lead: `… /*tail⏎*/ /*lead*/ $effect(…)` — the lead
    // comment never lands in the root's tail slot (the between-statements
    // disposition then matches the newline-separated equivalent above:
    // itemized carriers do not carry inter-statement trivia).
    let js = emit(
        "<script>let x = $state(0); $effect.root(() => { return () => {}; }) /*tail\n*/ /*lead*/ $effect(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*tail\n*/ ;"),
        "the terminator-bearing block comment is itself the LAST collected tail comment:\n{js}"
    );
    assert!(
        !js.contains("*/ /*lead*/"),
        "the next statement's leading comment is never stolen into the previous carrier's tail:\n{js}"
    );
    assert!(
        js.contains("$.effect_root(() => {") && js.contains("$.user_effect(() => {"),
        "both statements still lower on their own:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Call-head lead — the next statement's leading trivia in its
    // carrier-preserved position (`… /*tail⏎*/ $effect(/*lead*/ …)`): the
    // lead comment stays with the NEXT statement, exactly once across the
    // whole emission (a walk running past the terminator comment steals a
    // second copy into the root's tail).
    let js = emit(
        "<script>let x = $state(0); $effect.root(() => { return () => {}; }) /*tail\n*/ $effect(/*lead*/ () => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*tail\n*/ ;"),
        "the root tail holds exactly the terminator-bearing comment:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(/*lead*/"),
        "the next statement's leading comment stays with the next statement:\n{js}"
    );
    assert_eq!(
        js.matches("/*lead*/").count(),
        1,
        "the next-statement leading comment survives exactly once across the whole emission:\n{js}"
    );
    assert!(
        !js.contains("*/ /*lead*/"),
        "the leading comment never ALSO lands in the previous carrier's tail:\n{js}"
    );
    assert_eq!(
        js.matches("/*tail").count(),
        1,
        "the terminator-bearing tail comment itself survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_multiline_block_tail_never_steals_next_statement_call_internal_trivia() {
    // Past a terminator-bearing block tail comment the following gaps hold
    // the NEXT statement's TOKENS (` $effect(() => {` — newline-free), so a
    // walk that only inspects gaps would reach INSIDE the next statement's
    // call and steal its internal comments into the previous carrier's tail —
    // DUPLICATING them (the next carrier's call slice carries them verbatim).
    // The tail stops at the terminator comment; the internal comment lands
    // call-internal in the NEXT statement's helper, exactly once.
    //
    // Statement carrier: `$effect.root(fn) /*tail⏎*/ $effect(() => { /*inner*/ … });`.
    let js = emit(
        "<script>let x = $state(0); $effect.root(() => { return () => {}; }) /*tail\n*/ $effect(() => { /*inner*/ console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*tail\n*/ ;"),
        "the root tail holds exactly the terminator-bearing comment:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(() => { /*inner*/"),
        "the internal comment lands call-internal in the NEXT statement's helper:\n{js}"
    );
    assert_eq!(
        js.matches("/*inner*/").count(),
        1,
        "the call-internal comment survives exactly once across the whole emission:\n{js}"
    );
    assert!(
        !js.contains("*/ /*inner*/"),
        "the call-internal comment never lands in the previous carrier's tail:\n{js}"
    );
    assert_eq!(
        js.matches("/*tail").count(),
        1,
        "the terminator-bearing tail comment itself survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Declarator-init carrier, same boundary through the ONE shared
    // collector: `const s = $effect.root(fn) /*tail⏎*/ $effect(() => { /*inner*/ … });`.
    let js = emit(
        "<script>let x = $state(0); const s = $effect.root(() => { return () => {}; }) /*tail\n*/ $effect(() => { /*inner*/ console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(() => {"),
        "the semicolon-less root init still lowers assigned through the carrier:\n{js}"
    );
    assert!(
        js.contains(") /*tail\n*/ ;"),
        "the init tail holds exactly the terminator-bearing comment:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(() => { /*inner*/"),
        "the internal comment lands call-internal in the NEXT statement's helper:\n{js}"
    );
    assert_eq!(
        js.matches("/*inner*/").count(),
        1,
        "the call-internal comment survives exactly once across the whole emission:\n{js}"
    );
    assert!(
        !js.contains("*/ /*inner*/"),
        "the call-internal comment never lands in the previous carrier's tail:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_terminator_free_same_line_tail_chain_still_collects_whole() {
    // CONTROL: only a comment that is ITSELF a statement terminator (a line
    // comment, or a block comment whose text holds a line terminator) ends
    // the lexical tail. A chain of terminator-FREE same-line block comments
    // still collects WHOLE — the walk never stops early on an ordinary block
    // comment — and the line break ahead of the next statement still bounds
    // it exactly as before.
    let js = emit(
        "<script>let x = $state(0); $effect.root(() => { return () => {}; }) /*a*/ /*b*/\n$effect(() => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*a*/ /*b*/ ;"),
        "the whole terminator-free same-line tail chain still collects in source order:\n{js}"
    );
    assert_eq!(
        js.matches("/*a*/").count(),
        1,
        "the first tail comment survives exactly once:\n{js}"
    );
    assert_eq!(
        js.matches("/*b*/").count(),
        1,
        "the second tail comment survives exactly once:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(() => {"),
        "the next-line effect statement still lowers on its own:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_later_line_wrapper_close_asi_tail_trivia_survives() {
    // A transparent author-paren wrapper whose close `)` sits on a LATER line
    // than the call end (`($effect.root(fn)⏎) /*!license*/`) puts a line
    // terminator INSIDE the carrier's own statement/declaration span — between
    // the inner call `)` and the wrapper `)`. That interior newline is NOT an
    // ASI statement terminator (the expression is not complete until the
    // wrapper `)`), so a semicolon-less carrier's genuinely-trailing same-line
    // comment AFTER the wrapper close must still collect into the tail —
    // license-class comments stay in contract (oracle-verified: svelte@5.56.10
    // emits `$.effect_root(…); /*!license*/`). Only a line terminator at or
    // after the span end is a real ASI boundary.
    //
    // Statement carrier, ASI at EOF: `($effect.root(fn)⏎) /*!license*/`.
    let js = emit(
        "<script>($effect.root(() => { return () => {}; })\n) /*!license*/</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the later-line-wrapper-close statement tail license comment re-emits after the call payload:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the statement tail license comment survives exactly once:\n{js}"
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the wrapped semicolon-less root statement still lowers through the carrier:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Declarator-init carrier, ASI at EOF:
    // `const s = ($effect.root(fn)⏎) /*!license*/`.
    let js = emit(
        "<script>const s = ($effect.root(() => { return () => {}; })\n) /*!license*/</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const s = $.effect_root(() => {"),
        "the wrapped semicolon-less root init still lowers assigned through the carrier:\n{js}"
    );
    assert!(
        js.contains(") /*!license*/ ;"),
        "the later-line-wrapper-close init tail license comment re-emits after the call payload:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the init tail license comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // NEXT-LINE guard: the ASI gate is relaxed ONLY for the wrapper interior,
    // never past the span end — a comment on the NEXT line after the collected
    // tail is the next statement's territory. `($effect.root(fn)⏎) /*!lic*/⏎
    // $effect(/*lead*/ …);`: the tail holds exactly `/*!lic*/`, the next
    // statement keeps `/*lead*/` call-internal, each exactly once.
    let js = emit(
        "<script>let x = $state(0); ($effect.root(() => { return () => {}; })\n) /*!lic*/\n$effect(/*lead*/ () => { console.log(x) });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains(") /*!lic*/ ;"),
        "the later-line-wrapper-close tail comment still collects mid-script:\n{js}"
    );
    assert_eq!(
        js.matches("/*!lic*/").count(),
        1,
        "the tail comment survives exactly once:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(/*lead*/"),
        "the next statement's leading comment stays with the next statement (call-internal):\n{js}"
    );
    assert_eq!(
        js.matches("/*lead*/").count(),
        1,
        "the next-statement leading comment survives exactly once across the whole emission:\n{js}"
    );
    assert!(
        !js.contains("*/ /*lead*/"),
        "the next statement's leading comment is never stolen into the previous carrier's tail:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // POST-`;` guard on the SAME wrapper shape (`($effect.root(fn)⏎);
    // /*after*/`): the statement's own `;` sits INSIDE its OXC span, so a
    // beyond-span scan that starts at the span end would skip it and steal
    // the post-terminator comment — the explicit-`;` boundary holds over the
    // WHOLE gap, interior included.
    let js = emit(
        "<script>($effect.root(() => { return () => {}; })\n); /*after*/</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        !js.contains(") /*after*/"),
        "a post-`;` trailing comment is never collected into the later-line-wrapper-close carrier tail:\n{js}"
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the wrapped root statement still lowers through the carrier:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // Explicit-`;` CONTROL (`($effect.root(fn)⏎) /*!license*/;`): the comment
    // sits INSIDE the statement span (ahead of the `;`), collected by the
    // unconditional interior branch — pinned to its exact emitted form, byte
    // for byte, proving the interior semantics and the beyond-span ASI gate
    // agree.
    let js = emit(
        "<script>($effect.root(() => { return () => {}; })\n) /*!license*/;</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("\t$.effect_root(() => { return () => {}; }) /*!license*/ ;\n"),
        "the explicit-`;` sibling emits its pinned exact byte form:\n{js}"
    );
    assert_eq!(
        js.matches("/*!license*/").count(),
        1,
        "the explicit-`;` tail license comment survives exactly once:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_tracking_optional_inline_attribute_memoizes_normalized() {
    // The inline-attribute optional form `disabled={$effect.tracking?.()}` —
    // official ACCEPTS and memoizes it exactly like the plain inline call
    // (oracle-verified: `$.template_effect(($0) => button.disabled = $0,
    // [() => $.effect_tracking()])` — no `?.` in the dep thunk). A
    // `$.effect_tracking?.()` dep emission would be a structural divergence.
    let js = emit(
        "<script>\n\tconst t = $effect.tracking();\n</script>\n<input disabled={$effect.tracking?.()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.template_effect(") && js.contains("input.disabled = $0"),
        "the optional inline tracking call memoizes into the deps-array effect slot:\n{js}"
    );
    assert!(
        js.contains("[() => $.effect_tracking()]") || js.contains("[() => ($.effect_tracking())]"),
        "the memoized dep re-evaluates the tracking call inside the effect:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head is normalized away in the dep thunk:\n{js}"
    );
    assert!(
        !js.contains("input.disabled = $.effect_tracking()"),
        "the inline tracking call is NEVER a construction-time one-shot:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

// ── `$store` auto-subscriptions: the malformed-sibling / edge matrix ─────────
//
// Every accepted store form's malformed / edge sibling has a fail-closed OR
// correct-behavior discriminating test here (the corpus goldens own the happy
// paths). Each test asserts BOTH what SHOULD appear and what should NOT.

#[test]
fn store_push_flag_is_mode_sensitive_and_frame_is_needs_context_driven() {
    // RUNES + imported store: the frame opens via the EXISTING imported-call
    // `needs_context` trigger with the RUNES flag — `$.push($$props, true)`,
    // never `false`.
    let runes = emit(
        "<script>import { writable } from 'svelte/store'; const c = writable(0); let n = $state(0);</script>\n<p>{$c}</p>\n<button onclick={() => n++}>{n}</button>\n",
        "App.svelte",
    );
    assert!(
        runes.contains("$.push($$props, true);"),
        "a runes store component frames with the `true` flag:\n{runes}"
    );
    assert!(
        !runes.contains("$.push($$props, false)"),
        "a runes component must never push the legacy `false` flag:\n{runes}"
    );
    assert!(
        !runes.contains("$.init()"),
        "a RUNES framing component emits NO `$.init()` (legacy-frame-only):\n{runes}"
    );

    // LEGACY + imported store: the same trigger with the LEGACY flag — `false`,
    // never `true` — plus the legacy-frame `$.init()` after the script items.
    let legacy = emit(
        "<script>import { writable } from 'svelte/store'; const c = writable(0);</script>\n<p>{$c}</p>\n",
        "App.svelte",
    );
    assert!(
        legacy.contains("$.push($$props, false);"),
        "a legacy store component frames with the `false` flag:\n{legacy}"
    );
    assert!(
        !legacy.contains("$.push($$props, true)"),
        "a legacy component must never push the runes `true` flag:\n{legacy}"
    );
    assert!(
        legacy.contains("\t$.init();\n"),
        "a LEGACY framing component emits `$.init()`:\n{legacy}"
    );
    assert!(
        legacy.contains("$.pop();\n\t$$cleanup();"),
        "the `$$cleanup()` finalizer runs AFTER `$.pop()` when a frame exists:\n{legacy}"
    );

    // CLEAN LOCAL store (object-literal factory, no `new`, no imported call):
    // setup/cleanup WITHOUT any frame — store presence never opens the frame.
    let local = emit(
        "<script>function w(v) { const subs = []; return { subscribe(f) { subs.push(f); f(v); return () => {}; } }; } const c = w(0);</script>\n<p>{$c}</p>\n",
        "App.svelte",
    );
    assert!(
        local.contains("const [$$stores, $$cleanup] = $.setup_stores();")
            && local.contains("$$cleanup();"),
        "a clean local store still emits setup_stores + $$cleanup:\n{local}"
    );
    assert!(
        !local.contains("$.push(") && !local.contains("$.pop("),
        "a clean local store must NOT open the component frame:\n{local}"
    );
    assert!(
        !local.contains("$$props"),
        "a clean local store binds NO `$$props` parameter:\n{local}"
    );
    assert!(
        !local.contains("$.init()"),
        "a frame-less legacy store emits NO `$.init()`:\n{local}"
    );
}

#[test]
fn store_free_component_emits_no_setup_stores_or_cleanup() {
    // NEGATIVE (`$$cleanup`): a component with NO `$name` subscription emits
    // neither the registry setup nor the finalizer.
    let js = emit(
        "<script>let n = $state(0);</script>\n<button onclick={() => n++}>{n}</button>\n",
        "App.svelte",
    );
    assert!(
        !js.contains("setup_stores") && !js.contains("$$cleanup") && !js.contains("store_get"),
        "a store-free component carries no store machinery:\n{js}"
    );
}

#[test]
fn store_lowering_is_import_provenance_independent() {
    // The IMPORTED-writable and the LOCAL-factory store lower through the SAME
    // accessor/setup shape — provenance changes the FRAME (needs_context), never
    // the store lowering itself.
    let imported = emit(
        "<script>import { writable } from 'svelte/store'; const c = writable(0);</script>\n<p>{$c}</p>\n",
        "App.svelte",
    );
    let local = emit(
        "<script>function w(v) { const subs = []; return { subscribe(f) { subs.push(f); f(v); return () => {}; } }; } const c = w(0);</script>\n<p>{$c}</p>\n",
        "App.svelte",
    );
    let accessor = "const $c = () => $.store_get(c, '$c', $$stores);";
    let setup = "const [$$stores, $$cleanup] = $.setup_stores();";
    for (label, js) in [("imported", &imported), ("local", &local)] {
        assert!(
            js.contains(accessor) && js.contains(setup) && js.contains("$.set_text(text, $c())"),
            "{label} store lowering must use the identical accessor/setup/read shape:\n{js}"
        );
    }
}

#[test]
fn store_class_local_emits_class_verbatim_and_subscribes() {
    // A local CLASS-based store (`class S { subscribe(fn){…} } const c = new
    // S(); {$c}`): the class is admitted into the store-dependency closure
    // (reached from the `const c = new S()` source of the `$c` subscription) and
    // emitted VERBATIM; `$c` subscribes identically to any other store. This was
    // the class-based local store edge (`InstanceScriptItem{construct:"class"}`) —
    // now SUPPORTED (oracle-verified against svelte@5.56.10: the class frames the
    // store via `new`).
    let js = emit(
        "<script>class S { subscribe(fn) { fn(1); return () => {}; } } const c = new S();</script>\n<p>{$c}</p>\n",
        "App.svelte",
    );
    // The class body is emitted verbatim (its `subscribe` method survives).
    assert!(
        js.contains("class S") && js.contains("subscribe(fn)"),
        "the store class body must emit verbatim:\n{js}"
    );
    // `const c = new S();` frames the store instance.
    assert!(
        js.contains("const c = new S();"),
        "the `new S()` store instance must be emitted:\n{js}"
    );
    // The `$c` subscription accessor + shared setup + finalizer.
    assert!(
        js.contains("const $c = () => $.store_get(c, '$c', $$stores);")
            && js.contains("const [$$stores, $$cleanup] = $.setup_stores();")
            && js.contains("$$cleanup();")
            && js.contains("$.set_text(text, $c())"),
        "the `$c` subscription/setup/read shape must be present:\n{js}"
    );
    // NEGATIVE: the class is a STORE dependency, never a subscribed base — no
    // `$.store_get(S, …)` accessor is minted for the class name itself, and the
    // class is NOT rune-lowered (no `$.state` / `$.proxy`).
    assert!(
        !js.contains("store_get(S,") && !js.contains("$S"),
        "the class NAME must not be subscribed as a store:\n{js}"
    );
    assert!(
        !js.contains("$.state(") && !js.contains("$.proxy("),
        "a store class must not be rune-lowered:\n{js}"
    );
    // NEGATIVE: the old fail-closed refusal is gone (emit succeeded above; assert
    // no residual refusal marker leaked into the module).
    assert!(
        !js.contains("svelte-runtime-unsupported"),
        "no fail-closed refusal residue may appear:\n{js}"
    );
}

#[test]
fn store_class_with_inner_reactive_reference_is_rewritten() {
    // A local store CLASS whose method body carries an INNER `$`-store reactive
    // reference (`class S { m() { return $a; } }` over a top-level store `a`):
    // official svelte@5.56.10 REWRITES the inner `$a` read to `$a()` (and an inner
    // `$a = v` write to `$.store_set(a, v)`) inside class method/getter/setter
    // bodies, field initializers, and static blocks. The canonical statement
    // rewriter applies the same binding-aware edits inside the class body.
    let js = emit(
        "<script>import { writable } from 'svelte/store'; const a = writable(1); class S { m() { return $a; } } const c = new S();</script>\n<p>{$c}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("class S { m() { return $a(); } }"),
        "inner store read in class body did not rewrite:\n{js}"
    );
    assert!(
        !js.contains("return $a;"),
        "raw inner store read leaked:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
    // Discriminating control: the same store class without an inner `$` reference
    // also compiles and `$c` subscribes.
    let control = emit(
        "<script>class S { subscribe(fn) { fn(1); return () => {}; } } const c = new S();</script>\n<p>{$c}</p>\n",
        "App.svelte",
    );
    assert!(
        control.contains("class S")
            && control.contains("subscribe(fn)")
            && control.contains("const $c = () => $.store_get(c, '$c', $$stores);"),
        "the simple store class control (no inner $-reactive surface) must stay supported:\n{control}"
    );
    assert!(
        !control.contains("svelte-runtime-unsupported"),
        "the control must not carry a refusal residue:\n{control}"
    );
}

#[test]
fn store_plus_custom_element_uses_pre_return_pop_finalizer_slot() {
    // A store component that ALSO carries custom-element `$$exports` prop
    // accessors: official emits the PRE-RETURN finalizer slot `var $$pop =
    // $.pop($$exports); $$cleanup(); return $$pop;` — the store `$$cleanup()`
    // runs BEFORE the captured export return. This was the store + custom-element
    // finalizer edge (refused as `HostOrCustomElement{surface:"store
    // subscription"}` with a FACTUALLY-WRONG "no post-return finalizer slot"
    // rationale) — now SUPPORTED
    // (oracle-verified against svelte@5.56.10).
    let js = emit(
        "<svelte:options customElement=\"my-el\" />\n<script>import { writable } from 'svelte/store'; const c = writable(0); let { label } = $props();</script>\n<p>{$c}{label}</p>\n",
        "App.svelte",
    );
    // The PRE-RETURN finalizer slot, in order: capture → cleanup → return.
    let pop_capture = js.find("var $$pop = $.pop($$exports);");
    let cleanup = js.find("$$cleanup();");
    let ret = js.find("return $$pop;");
    assert!(
        pop_capture.is_some() && cleanup.is_some() && ret.is_some(),
        "the store+CE close must emit the `$$pop` capture / `$$cleanup()` / \
         `return $$pop` finalizer slot:\n{js}"
    );
    assert!(
        pop_capture < cleanup && cleanup < ret,
        "the finalizer order must be capture → cleanup → return (store `$$cleanup()` \
         runs BEFORE the captured export return):\n{js}"
    );
    // The `$$exports` accessor object + the custom-element registration survive.
    assert!(
        js.contains("var $$exports = {")
            && js.contains("$.create_custom_element(App, { label: {} }")
            && js.contains("customElements.define('my-el',"),
        "the custom-element `$$exports`/registration must be present:\n{js}"
    );
    // NEGATIVE: the old refusal is gone (emit succeeded) AND the STRANDED shape —
    // a bare `return $.pop($$exports);` that would leave `$$cleanup()` unreachable
    // after the return — must NOT be emitted.
    assert!(
        !js.contains("return $.pop($$exports);"),
        "the stranded `return $.pop($$exports);` (unreachable cleanup) must NOT appear:\n{js}"
    );
    assert!(
        !js.contains("svelte-runtime-unsupported"),
        "no fail-closed refusal residue may appear:\n{js}"
    );
}

#[test]
fn store_write_to_derived_store_lowers_to_store_set() {
    // A write to a DERIVED store compiles to `$.store_set` (oracle-verified:
    // official accepts it at compile time — the failure is a runtime concern).
    let js = emit(
        "<script>import { writable, derived } from 'svelte/store'; const a = writable(1); const d = derived(a, ($a) => $a * 2); function w() { $d = 9; }</script>\n<button onclick={w}>{$d}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("function w() { $.store_set(d, 9); }"),
        "a derived-store write lowers to `$.store_set`:\n{js}"
    );
    // NEGATIVE: the shadowed `$a` callback param mints NO accessor; only `$d`.
    assert!(
        js.contains("const $d = () => $.store_get(d, '$d', $$stores);"),
        "the `$d` accessor is minted:\n{js}"
    );
    assert!(
        !js.contains("store_get(a,"),
        "the shadowed `$a` callback param must NOT mint an accessor:\n{js}"
    );
}

#[test]
fn store_compound_writes_pin_all_four_helper_shapes() {
    // The four compound forms lower to their exact helpers (oracle-verified):
    // postfix `++`/`--` → `$.update_store` (with `-1` for decrement), prefix →
    // `$.update_pre_store`, compound-assign → `$.store_set` over the accessor
    // read — never the signal `$.update` family.
    let js = emit(
        "<script>import { writable } from 'svelte/store'; const c = writable(0); function a() { $c++; } function b() { $c--; } function d() { ++$c; } function e() { $c += 2; }</script>\n<button onclick={a}>{$c}</button>\n<button onclick={b}>b</button>\n<button onclick={d}>d</button>\n<button onclick={e}>e</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("function a() { $.update_store(c, $c()); }"),
        "postfix increment:\n{js}"
    );
    assert!(
        js.contains("function b() { $.update_store(c, $c(), -1); }"),
        "postfix decrement carries -1:\n{js}"
    );
    assert!(
        js.contains("function d() { $.update_pre_store(c, $c()); }"),
        "prefix increment uses update_pre_store:\n{js}"
    );
    assert!(
        js.contains("function e() { $.store_set(c, $c() + 2); }"),
        "compound assign uses store_set over the accessor read:\n{js}"
    );
    // NEGATIVE: never the signal update family on a store, and no `$.get`.
    assert!(
        !js.contains("$.update(c") && !js.contains("$.update_pre(c") && !js.contains("$.get("),
        "a store write must never use the signal `$.update`/`$.get` family:\n{js}"
    );
}

// ── legacy (non-runes) reactivity substrate: `export let` props + promoted `let` ──

#[test]
fn runes_mode_export_let_is_the_official_legacy_export_invalid_reject() {
    // Under EXPLICIT runes mode an `export let` is the official svelte@5.56.10
    // COMPILE ERROR `legacy_export_invalid` — never an unsupported-feature
    // refusal and never the incidental static-interpolation misattribution.
    let err = emit_result(
        "<svelte:options runes={true} />\n<script>export let foo = 1;</script>\n<p>{foo}</p>\n",
    )
    .expect_err("a runes-mode `export let` must reject");
    let ClientCompileError::OfficialReject(rejection) = &err else {
        panic!("expected the official legacy_export_invalid reject, got {err:?}");
    };
    assert_eq!(rejection.official_code, "legacy_export_invalid");
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::LegacyExportInvalid
    );
    // NEGATIVE: not the unsupported quadrant, so no static-interpolation code.
    assert!(
        !matches!(&err, ClientCompileError::Unsupported(_)),
        "must not surface as an unsupported feature: {err:?}"
    );
    // A DESTRUCTURED `export let` under runes is the SAME official reject.
    let err = emit_result(
        "<svelte:options runes={true} />\n<script>export let { a } = { a: 1 };</script>\n<p>hi</p>\n",
    )
    .expect_err("a runes-mode destructured `export let` must reject");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "legacy_export_invalid"),
        "destructured export let under runes must be legacy_export_invalid: {err:?}"
    );
}

#[test]
fn runes_mode_reactive_statement_is_the_official_reject_explicit_and_inferred() {
    // H: explicit `<svelte:options runes={true}>` + `$:` → the official
    // `legacy_reactive_statement_invalid` compile error.
    let err = emit_result(
        "<svelte:options runes={true} />\n<script>let c = $state(0); $: d = c * 2;</script>\n<p>{d}</p>\n",
    )
    .expect_err("a runes-mode `$:` must reject");
    let ClientCompileError::OfficialReject(rejection) = &err else {
        panic!("expected the official legacy_reactive_statement_invalid reject, got {err:?}");
    };
    assert_eq!(rejection.official_code, "legacy_reactive_statement_invalid");
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::LegacyReactiveStatementInvalid
    );
    // I: the SAME reject when runes mode is INFERRED from rune presence (no
    // explicit option) — the inference gate must fire for `$state` usage.
    let err = emit_result("<script>let c = $state(0); $: d = c * 2;</script>\n<p>{d}</p>\n")
        .expect_err("an inferred-runes `$:` must reject");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "legacy_reactive_statement_invalid"),
        "inferred-runes `$:` must be legacy_reactive_statement_invalid: {err:?}"
    );
}

// ── `$:` legacy reactive statements: lowering + the malformed-sibling matrix ──
//
// The `$:` labeled statement lowers to `$.legacy_pre_effect(<deps>, <body>)`
// registrations plus ONE trailing `$.legacy_pre_effect_reset()` (oracle-verified
// against svelte@5.56.10). Every accepted form's malformed / edge sibling has a
// fail-closed OR correct-behavior discriminating test here; each asserts BOTH
// what SHOULD appear and what should NOT.

#[test]
fn reactive_assignment_synthesizes_mutable_source_and_registers_pre_effect() {
    // `$: y = x + 1` (bare-ident assignment, `y` undeclared): synthesizes the
    // implicit `const y = $.mutable_source();` (NO init arg), registers the
    // effect with the dep thunk over the statement's reads, and rewrites the
    // body assignment through the shared signal rewriter.
    let js = emit(
        "<script>let x = 0; $: y = x + 1;</script>\n<p>{y}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const y = $.mutable_source();"),
        "the implicit assignment target declares the zero-arg cell:\n{js}"
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => ($.get(x)), () => { $.set(y, $.get(x) + 1); });"),
        "the effect registers with the dep thunk + rewritten body:\n{js}"
    );
    assert!(
        js.contains("$.legacy_pre_effect_reset();"),
        "the reset finalizer is emitted:\n{js}"
    );
    assert_eq!(
        js.matches("$.legacy_pre_effect_reset()").count(),
        1,
        "the reset is emitted ONCE per component:\n{js}"
    );
    // The synthesized binding reads through the signal rewriter elsewhere.
    assert!(
        js.contains("$.set_text(text, $.get(y))"),
        "a template read of the synthesized target is a signal read:\n{js}"
    );
    // NEGATIVE: never `$.derived` (effect registration, not value memoization);
    // never the const-with-init form; the synthesized decl is `const`, not `let`.
    assert!(
        !js.contains("$.derived"),
        "a reactive statement must NEVER lower to $.derived:\n{js}"
    );
    assert!(
        !js.contains("const y = $.mutable_source(0)") && !js.contains("let y = $.mutable_source"),
        "the synthesized cell takes no init and is a const:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_block_wraps_the_whole_statement_as_one_effect() {
    // `$: { t = x * 2; console.log(t); }` — the block body wraps VERBATIM
    // (rewritten) as ONE effect; `t` is read inside the block so it joins the
    // dep thunk in first-mention order (t, then x — oracle-pinned).
    let js = emit(
        "<script>let x = 0; let t = 0; $: { t = x * 2; console.log(t); }</script>\n<p>{t}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "$.legacy_pre_effect(() => ($.get(t), $.get(x)), () => { $.set(t, $.get(x) * 2); console.log($.get(t)); });"
        ),
        "the block wraps as one effect with the (t, x) dep order:\n{js}"
    );
    assert_eq!(
        js.matches("$.legacy_pre_effect(").count(),
        1,
        "a multi-statement block registers exactly ONE effect:\n{js}"
    );
    // NEGATIVE: `t` is declared (`let t = 0`) — promoted, never re-synthesized.
    assert!(
        js.contains("let t = $.mutable_source(0);") && !js.contains("const t = $.mutable_source"),
        "a declared target stays the promoted let, no spurious synth const:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_if_statement_wraps_verbatim_with_read_only_deps() {
    // `$: if (x > 5) { big = true; }` — the `if` wraps verbatim; `big` is only
    // ever a pure `=`-assignment LHS so it is NOT a dependency (oracle-pinned).
    let js = emit(
        "<script>let x = 0; let big = false; $: if (x > 5) { big = true; }</script>\n<p>{big}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => ($.get(x)), () => { if ($.get(x) > 5) { $.set(big, true); } });"),
        "the if statement wraps verbatim with x as the only dep:\n{js}"
    );
    assert!(
        !js.contains("$.get(big), $.get(x)") && !js.contains("($.get(big)"),
        "a pure assignment-LHS name must not join the dep thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_registrations_emit_in_dependency_order_not_source_order() {
    // `$: z = y + 1; $: y = x + 1;` — declarations stay in SOURCE order
    // (z, then y) but the registrations emit in DEPENDENCY order: the
    // y-assigner registers FIRST even though it appears later (oracle case:
    // official topologically orders the reactive statements).
    let js = emit(
        "<script>let x = 0; $: z = y + 1; $: y = x + 1;</script>\n<p>{z}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    let z_decl = js
        .find("const z = $.mutable_source();")
        .expect("z synthesizes");
    let y_decl = js
        .find("const y = $.mutable_source();")
        .expect("y synthesizes");
    assert!(
        z_decl < y_decl,
        "synthesized declarations stay in source order (z before y):\n{js}"
    );
    let y_effect = js
        .find("$.legacy_pre_effect(() => ($.get(x)), () => { $.set(y, $.get(x) + 1); });")
        .expect("the y-assigner registers");
    let z_effect = js
        .find("$.legacy_pre_effect(() => ($.get(y)), () => { $.set(z, $.get(y) + 1); });")
        .expect("the z-assigner registers");
    assert!(
        y_effect < z_effect,
        "the y-assigner's registration must precede the z-reader's (dependency order):\n{js}"
    );
    let reset = js.find("$.legacy_pre_effect_reset();").expect("reset");
    assert!(
        reset > z_effect,
        "the single reset follows every registration:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_dependency_cycle_is_the_official_reject() {
    // `$: a = b + x; $: b = a + 1;` — a dependency cycle among reactive
    // statements is the OFFICIAL compile error `reactive_declaration_cycle`
    // ("Cyclical dependency detected: a → b → a" — probed first-hand against
    // svelte@5.56.10), routed through the official-reject channel.
    let err = emit_result(
        "<script>let x = 0; $: a = b + x; $: b = a + 1;</script>\n<p>{a}</p>\n<button onclick={() => x++}>b</button>\n",
    )
    .expect_err("a reactive dependency cycle must reject");
    let ClientCompileError::OfficialReject(rejection) = &err else {
        panic!("expected the official reactive_declaration_cycle reject, got {err:?}");
    };
    assert_eq!(rejection.official_code, "reactive_declaration_cycle");
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::ReactiveDeclarationCycle
    );
    // NEGATIVE: a SELF-dependency (`$: x = x + 1`) is NOT a cycle — official
    // excludes self-assigned deps from the edge set.
    let js = emit(
        "<script>let x = 0; $: x = x + 1;</script>\n<p>{x}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => ($.get(x)), () => { $.set(x, $.get(x) + 1); });"),
        "a self-dependent statement compiles (not a cycle):\n{js}"
    );
}

#[test]
fn reactive_statement_opens_frame_without_legacy_init_while_unsafe_call_still_inits() {
    // THE FRAME-REASON DISCRIMINATOR. A bare-`$:`-only legacy component (no
    // store, no `new`, no unsafe call/member) OPENS the push/pop context frame
    // but must NOT emit `$.init()` (oracle-verified: push/pop present,
    // `$.init()` absent). A component whose frame reason is an UNSAFE IMPORTED
    // CALL still emits `$.init()`. Collapsing the two frame reasons into one
    // boolean breaks one of the two halves.
    let bare = emit(
        "<script>let x = 0; $: y = x + 1;</script>\n<p>{y}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        bare.contains("$.push($$props, false);") && bare.contains("$.pop();"),
        "a `$:`-only legacy component opens the context frame:\n{bare}"
    );
    assert!(
        !bare.contains("$.init()"),
        "a `$:`-only frame must NOT emit `$.init()` (frame reason is the \
         reactive statement, not the legacy-init trigger):\n{bare}"
    );
    // CONTROL: the unsafe-imported-call reason (a store factory call) still
    // warrants `$.init()` — the two reasons must stay separately tracked.
    let unsafe_call = emit(
        "<script>import { writable } from 'svelte/store'; const c = writable(0);</script>\n<p>{$c}</p>\n",
        "App.svelte",
    );
    assert!(
        unsafe_call.contains("$.push($$props, false);") && unsafe_call.contains("\t$.init();\n"),
        "an unsafe-call frame still emits `$.init()`:\n{unsafe_call}"
    );
}

#[test]
fn reactive_prop_only_deep_reads_without_legacy_init() {
    // `export let p; $: y = p + 1;` — the prop dep deep-reads the getter call,
    // the frame opens (`$.push($$props, false)`), and NO `$.init()` is emitted
    // (a legacy prop read alone is not an unsafe-call trigger) — oracle-pinned.
    let js = emit(
        "<script>export let p; $: y = p + 1;</script>\n<p>{y}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "$.legacy_pre_effect(() => ($.deep_read_state(p())), () => { $.set(y, p() + 1); });"
        ),
        "the prop dep deep-reads the getter call:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, false);"),
        "the reactive statement opens the legacy frame:\n{js}"
    );
    assert!(
        !js.contains("$.init()"),
        "a prop + `$:` component without an unsafe call emits NO `$.init()`:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_prop_write_composes_updated_onto_the_legacy_prop_flags() {
    // `export let p; $: p = x;` — a prop WRITTEN by a `$:` reactive statement
    // is the official `updated` axis exactly like a template write: the
    // declaration composes UPDATED (+4) onto the legacy base 8 → 12, the
    // effect body writes through the SETTER call (`p($.get(x))`), and no
    // colliding cell synthesizes for the already-declared prop target
    // (oracle-verified against svelte@5.56.10).
    let js = emit(
        "<script>export let p; let x = 0; $: p = x;</script>\n<p>{p}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let p = $.prop($$props, 'p', 12);"),
        "a `$:`-written prop carries flags 12 (8 | UPDATED 4):\n{js}"
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => ($.get(x)), () => { p($.get(x)); });"),
        "the effect body writes through the prop setter call:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, p())"),
        "the written prop still reads as the accessor call:\n{js}"
    );
    // NEGATIVE: the assignment target is the DECLARED prop — no colliding
    // synthesized cell — and the prop write never routes through the signal
    // family.
    assert!(
        !js.contains("const p = $.mutable_source"),
        "no colliding cell synthesizes for the prop target:\n{js}"
    );
    assert!(
        !js.contains("$.set(p,") && !js.contains("$.set(p "),
        "a prop write must not use the signal $.set family:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_parenthesized_assignment_still_synthesizes_the_target() {
    // `$: (y = x + 1)` — a PARENTHESIZED reactive assignment is the same
    // implicit-target declaration as the bare form (standard JS paren
    // semantics; official svelte@5.56.10 accepts it and synthesizes `y`): the
    // zero-arg `const y = $.mutable_source();` cell, the `$.set(y, …)` body
    // write, and the signal template read.
    let js = emit(
        "<script>let x = 0; $: (y = x + 1);</script>\n<p>{y}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const y = $.mutable_source();"),
        "the parenthesized assignment target declares the zero-arg cell:\n{js}"
    );
    assert!(
        js.contains("$.set(y, $.get(x) + 1)"),
        "the body assignment writes through the signal rewriter:\n{js}"
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => ($.get(x)),"),
        "the dep thunk reads the promoted `x`:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, $.get(y))"),
        "a template read of the synthesized target is a signal read:\n{js}"
    );
    // NEGATIVE: never the const-with-init form; the cell is a const, not a let.
    assert!(
        !js.contains("const y = $.mutable_source(0)") && !js.contains("let y = $.mutable_source"),
        "the synthesized cell takes no init and is a const:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_for_of_loop_local_shadows_the_outer_dep() {
    // `let i = 0; $: for (const i of [1, 2]) { console.log(i); }` — the for-of
    // HEAD binding shadows the outer reactive `i` across the whole statement,
    // so the dep thunk is EMPTY and the body's loop-local reads stay bare
    // (oracle-verified against svelte@5.56.10: `() => {}`).
    let js = emit(
        "<script>let i = 0; $: for (const i of [1, 2]) { console.log(i); }</script>\n<p>{i}</p>\n<button onclick={() => i++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "$.legacy_pre_effect(() => {}, () => { for (const i of [1, 2]) { console.log(i); } });"
        ),
        "the shadowed loop local records no outer dependency:\n{js}"
    );
    // NEGATIVE: the deps thunk never reads the OUTER cell, and the loop-local
    // read is never rewritten to a signal read.
    assert!(
        !js.contains("() => ($.get(i))"),
        "the outer `i` must not join the dep thunk:\n{js}"
    );
    assert!(
        !js.contains("console.log($.get(i))"),
        "the loop-local read stays bare:\n{js}"
    );
    // The OUTER `i` stays live elsewhere: the template reads the cell.
    assert!(
        js.contains("$.set_text(text, $.get(i))"),
        "the outer cell still drives the template read:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_for_head_and_catch_param_scopes_shadow_outer_deps() {
    // The remaining `$:` binding-scope heads mirror the for-of shadow rule: a
    // classic for-head `let`, a for-in head `const`, and a catch-clause param
    // each shadow an outer reactive name across their statement, so none
    // records the outer dependency (empty dep thunk).
    for (label, source, effect) in [
        (
            "classic for head",
            "<script>let j = 0; $: for (let j = 0; j < 2; j += 1) { console.log(j); }</script>\n<p>{j}</p>\n<button onclick={() => j++}>b</button>\n",
            "$.legacy_pre_effect(() => {}, () => { for (let j = 0; j < 2; j += 1) { console.log(j); } });",
        ),
        (
            "for-in head",
            "<script>let k = 0; $: for (const k in { a: 1 }) { console.log(k); }</script>\n<p>{k}</p>\n<button onclick={() => k++}>b</button>\n",
            "$.legacy_pre_effect(() => {}, () => { for (const k in { a: 1 }) { console.log(k); } });",
        ),
        (
            "catch param",
            "<script>let e = 0; $: try { console.log('t'); } catch (e) { console.log(e); }</script>\n<p>{e}</p>\n<button onclick={() => e++}>b</button>\n",
            "$.legacy_pre_effect(() => {}, () => { try { console.log('t'); } catch (e) { console.log(e); } });",
        ),
    ] {
        let js = emit(source, "App.svelte");
        assert!(
            js.contains(effect),
            "{label}: the shadowed head/param records no outer dependency:\n{js}"
        );
        assert!(
            !js.contains("() => ($.get("),
            "{label}: no outer name joins the dep thunk:\n{js}"
        );
        assert!(parses_as_js(&js), "{label}: module must be valid JS:\n{js}");
    }
}

#[test]
fn reactive_empty_statement_registers_an_empty_effect() {
    // `$:;` — official compiles the empty labeled statement to an effect with
    // an empty dep thunk and an empty body (probed first-hand against
    // svelte@5.56.10); it is handled, not fail-closed.
    let js = emit(
        "<script>let x = 0; $:;</script>\n<p>{x}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => {}, () => {});"),
        "the empty statement registers the empty-thunk effect:\n{js}"
    );
    assert!(
        js.contains("$.legacy_pre_effect_reset();"),
        "the reset still finalizes:\n{js}"
    );
    // NEGATIVE: nothing synthesizes for an empty statement.
    assert!(
        !js.contains("const  = ") && js.matches("$.mutable_source").count() == 1,
        "only the written `let x` mints a cell (no spurious synth):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_member_assignment_wraps_verbatim_without_synth_const() {
    // `$: obj.v = x * 2` — a MEMBER (non-ident) assignment target takes the
    // wrap-verbatim shape: NO synthesized declaration for `obj` (it is a
    // declared, promoted let), the body lowers through the deep-mutation wrap,
    // and `obj` is NOT a dependency (a member-assignment target root under `=`
    // is walked up and skipped — oracle-pinned: deps are `x` only).
    let js = emit(
        "<script>let x = 0; let obj = { v: 0 }; $: obj.v = x * 2;</script>\n<p>{x}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => ($.get(x)), () => { $.mutate(obj, $.get(obj).v = $.get(x) * 2); });"),
        "the member assignment wraps verbatim through the mutation helper:\n{js}"
    );
    assert!(
        !js.contains("const obj = $.mutable_source"),
        "no spurious const synthesizes for a member target:\n{js}"
    );
    assert!(
        !js.contains("() => ($.get(obj)") && !js.contains("$.get(obj), $.get(x))"),
        "the member-assignment target root is not a dependency:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_undeclared_rhs_name_stays_a_plain_global_read() {
    // `$: y = x + zzz` with `zzz` never declared — official treats `zzz` as a
    // GLOBAL (probed first-hand: it compiles; the read stays verbatim and never
    // joins the dep thunk), exactly like `console` in `$: console.log(x)`.
    let js = emit(
        "<script>let x = 0; $: y = x + zzz;</script>\n<p>{y}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => ($.get(x)), () => { $.set(y, $.get(x) + zzz); });"),
        "the undeclared name reads verbatim as a global:\n{js}"
    );
    assert!(
        !js.contains("$.get(zzz)") && !js.contains("zzz()"),
        "an undeclared name is never wrapped as a signal or accessor:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_destructuring_assignment_fails_closed() {
    // `$: ({ a } = { a: x });` — official lowers a destructuring reactive
    // assignment through a `$$value` closure (a distinct lowering); the shared
    // rewriter fails a destructuring write target closed, so the statement
    // refuses with the PRECISE destructuring diagnostic — never a mis-emitted
    // module, never the retired blanket reactive-statement refusal.
    let err = emit_result(
        "<script>let x = 0; let a; $: ({ a } = { a: x });</script>\n<p>{a}</p>\n<button onclick={() => x++}>b</button>\n",
    )
    .expect_err("a destructuring reactive assignment must fail closed");
    let ClientCompileError::Unsupported(surface) = &err else {
        panic!("expected the destructuring-write refusal, got {err:?}");
    };
    assert_eq!(
        surface.diagnostic_code(),
        "svelte-runtime-unsupported-destructuring-write",
        "the refusal is the precise destructuring diagnostic: {surface:?}"
    );
}

#[test]
fn reactive_store_dep_registers_the_bare_accessor_call() {
    // A `$:` reading ONLY a store accessor: the dep thunk carries the bare
    // accessor call (`$c()`), the body rewrites the store read, and the store
    // machinery (setup/cleanup) coexists with the reset finalizer. (This is the
    // former blanket legacy-`$:` refusal case, now lowering.)
    let js = emit(
        "<script>import { writable } from 'svelte/store'; const c = writable(0); $: doubled = $c * 2;</script>\n<p>{doubled}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => ($c()), () => { $.set(doubled, $c() * 2); });"),
        "the store dep is the bare accessor call:\n{js}"
    );
    assert!(
        js.contains("const [$$stores, $$cleanup] = $.setup_stores();")
            && js.contains("$$cleanup();"),
        "the store machinery still emits:\n{js}"
    );
    assert!(
        js.contains("const doubled = $.mutable_source();"),
        "the assignment target synthesizes:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_reactive_prop_emits_a_getter_over_the_legacy_prop_accessor() {
    // `foo={a}` where `a` is a legacy `export let` prop: state-bearing ⇒ a getter
    // reading the prop accessor (`a()`), never a plain init and never a DOM write.
    let js = emit(
        "<script>export let a;</script>\n<div><slot foo={a} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("get foo() {") && js.contains("return a();"),
        "a state-bearing slot prop is a getter over the accessor:\n{js}"
    );
    // NEGATIVE: not a DOM attribute write.
    assert!(
        !js.contains("$.set_attribute"),
        "a slot prop is never a DOM attribute write:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_mode_slot_emits_without_legacy_flags() {
    // `<slot>` in a RUNES component (official: deprecated but compilable): the
    // same `$.slot` topology, NO legacy flags import, and a never-reassigned
    // `$state` demotes to a plain init prop.
    let js = emit(
        "<script>let n = $state(1);</script>\n<div><slot foo={n} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.slot(node, $$props, 'default', { foo: n }, null);"),
        "the runes-mode slot prop (demoted plain init):\n{js}"
    );
    assert!(
        !js.contains("svelte/internal/flags/legacy"),
        "a runes component imports no legacy flags:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn maybe_runes_slot_call_memoizes_safe_equal_without_legacy_wrap() {
    // A store-only component (no `export let`, no `$:`) is the official
    // MAYBE-RUNES in-between mode: the memo helper stays `$.derived_safe_equal`
    // (non-runes) but the legacy wrap does NOT apply (BO-shape oracle):
    //   let $0 = $.derived_safe_equal(() => $s().m());
    let js = emit(
        "<script>import { writable } from 'svelte/store';\nconst s = writable({});</script>\n<div><slot foo={$s.m()} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.derived_safe_equal(() => ($s().m()))")
            || js.contains("$.derived_safe_equal(() => $s().m())"),
        "a maybe-runes memo keeps the safe-equal helper, unwrapped:\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "the legacy wrap never applies in maybe-runes mode:\n{js}"
    );
    assert!(
        !js.contains("$.derived("),
        "non-runes never uses `$.derived`:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_call_bearing_component_prop_still_memoizes_plain_derived() {
    // RUNES control: the memoizer stays `$.derived` and no legacy machinery
    // appears (G-shape oracle): let $0 = $.derived(() => $$props.obj.m());
    let js = emit(
        "<script>import Child from './Child.svelte';\nlet { obj } = $props();</script>\n<Child foo={obj.m()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.derived(() => ($$props.obj.m()))")
            || js.contains("$.derived(() => $$props.obj.m())"),
        "a runes call-bearing prop memoizes with `$.derived`:\n{js}"
    );
    assert!(
        !js.contains("derived_safe_equal")
            && !js.contains("$.untrack")
            && !js.contains("$.deep_read_state"),
        "no legacy memo machinery in a runes module:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_call_bearing_slot_prop_still_memoizes_plain_derived() {
    // RUNES slot control (P-shape oracle): let $0 = $.derived(() => $$props.obj.m());
    let js = emit(
        "<script>let { obj } = $props();</script>\n<div><slot foo={obj.m()} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.derived(() => ($$props.obj.m()))")
            || js.contains("$.derived(() => $$props.obj.m())"),
        "a runes slot call-bearing prop memoizes with `$.derived`:\n{js}"
    );
    assert!(
        !js.contains("derived_safe_equal")
            && !js.contains("$.untrack")
            && !js.contains("$.deep_read_state"),
        "no legacy memo machinery in a runes module:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_component_spread_call_memoizes_plain_derived() {
    // RUNES spread control (BF-shape oracle): the spread memoizes on
    // `has_call` in BOTH modes; runes keeps `$.derived`.
    let js = emit(
        "<script>import Child from './Child.svelte';\nlet { obj } = $props();</script>\n<Child {...obj.m()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.derived(() => ($$props.obj.m()))")
            || js.contains("$.derived(() => $$props.obj.m())"),
        "a runes call-bearing spread memoizes with `$.derived`:\n{js}"
    );
    assert!(
        js.contains("$.spread_props(() => $.get($0))"),
        "the spread thunk reads the memo:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_combined_attr_call_and_text_member_wrap_in_one_effect() {
    // The combined DOM-attribute + TEXT probe on the accepted text surface:
    // the call-bearing attribute memoizes its wrapped sequence while the
    // imported-member text interpolation wraps INLINE — both inside ONE
    // combined `$.template_effect`, each with its own wrapper sequence
    // (oracle):
    //   $.template_effect(($0) => {
    //     $.set_attribute(div, 'title', $0);
    //     $.set_text(text, ($.deep_read_state(NS), $.untrack(() => NS.z)));
    //   }, [() => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]);
    // (A call/member-bearing text interpolation like `{obj.m()}` stays the
    // fail-closed complex-interpolation breadth — the classifier refuses it,
    // so the deps-array fail-open cannot arise there.)
    let js = emit(
        "<script>import * as NS from './x.js';\nexport let obj;</script>\n<div title={obj.m()}></div>{NS.z}\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.set_text(text, ($.deep_read_state(NS), $.untrack(() => NS.z)))"),
        "the text member wraps inline in the shared effect:\n{js}"
    );
    assert!(
        js.contains("[() => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]"),
        "the attr call memoizes its own wrapped sequence:\n{js}"
    );
    assert!(
        js.contains("$.set_attribute(div, 'title', $0)"),
        "the attr write reads the memoized slot:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_group_dynamic_value_call_wraps_effect_dep_and_getter_read() {
    // The `bind:group` dynamic value wraps in BOTH consumers: the guarded
    // change-detection effect memoizes the wrapped sequence; the `$.bind_group`
    // getter's dependency read is the FULL inline wrapped sequence (oracle):
    //   [() => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]
    //   () => { ($.deep_read_state(obj()), $.untrack(() => obj().m())); return $.get(sel); }
    let js = emit(
        "<script>export let obj;\nlet sel = [];</script>\n<input type=\"checkbox\" bind:group={sel} value={obj.m()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("[() => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]"),
        "the group-value effect dep memoizes wrapped:\n{js}"
    );
    assert!(
        js.contains("($.deep_read_state(obj()), $.untrack(() => obj().m()));"),
        "the bind_group getter dep read is the inline wrapped sequence:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_attr_call_dep_stays_raw() {
    // RUNES control: the template-effect dep stays the RAW rewritten
    // expression — no `$.deep_read_state`, no `$.untrack`.
    let js = emit(
        "<script>let { obj } = $props();</script>\n<div title={obj.m()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("[() => $$props.obj.m()]") || js.contains("[() => ($$props.obj.m())]"),
        "the runes dep stays the raw expression:\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "no legacy wrap machinery in a runes module:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn maybe_runes_attr_call_dep_stays_raw() {
    // MAYBE-RUNES control (a store-only component — no `export let` / `$:` —
    // the official in-between mode): the wrap does NOT apply; the call still
    // memoizes raw (oracle): [() => $s().m()]
    let js = emit(
        "<script>import { writable } from 'svelte/store';\nconst s = writable({});</script>\n<div title={$s.m()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("[() => $s().m()]") || js.contains("[() => ($s().m())]"),
        "the maybe-runes dep stays the raw expression:\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "the legacy wrap never applies in maybe-runes mode:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_dispatcher_frames_without_legacy_init() {
    // A RUNES-mode dispatcher component: the same preserved import + plain
    // declaration, the runes frame (`$.push($$props, true)`), and NO legacy
    // `$.init()` (oracle-verified against svelte@5.56.10 with the fn-ref handler).
    let js = emit(
        "<script>import { createEventDispatcher } from 'svelte';\nlet n = $state(0);\nconst dispatch = createEventDispatcher();\nfunction fire() { dispatch('go', n); }</script>\n<button onclick={fire}>go</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the runes frame opens:\n{js}"
    );
    assert!(
        js.contains("const dispatch = createEventDispatcher();"),
        "the dispatcher declaration stays plain:\n{js}"
    );
    assert!(
        js.contains("dispatch('go', n)"),
        "the handler dispatch call stays plain:\n{js}"
    );
    assert!(
        !js.contains("$.init()"),
        "a runes component emits NO legacy $.init():\n{js}"
    );
    assert!(
        !js.contains("svelte/internal/flags/legacy"),
        "a runes component imports no legacy flags:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_class_single_base_call_stays_raw_in_clsx() {
    // RUNES control. Oracle: [() => $.clsx($$props.obj.m())]
    let js = emit(
        "<script>let { obj } = $props();</script>\n<div class={obj.m()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.clsx($$props.obj.m())"),
        "the runes class dep stays the raw clsx (cosmetic dep parens waived):\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "no legacy wrap machinery in a runes module:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_else_if_call_gets_own_derived() {
    // Oracle: the first (member) test wraps inline; the {:else if} call test
    // hoists its own derived and reads `$.get(d)`.
    let js = emit(
        &format!("{LEGACY_OBJ}{{#if obj.x}}<p>a</p>{{:else if obj.m()}}<p>b</p>{{/if}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "if (({})) $$render(consequent);",
            obj_wrap("obj().x")
        )),
        "the first member test wraps inline:\n{js}"
    );
    assert!(
        js.contains(&format!(
            "var d = $.derived(() => ({}));",
            obj_wrap("obj().m()")
        )),
        "the else-if call test hoists its own wrapped derived:\n{js}"
    );
    assert!(
        js.contains("else if ($.get(d)) $$render(consequent_1, 1);"),
        "the else-if reads $.get(d):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_html_local_zero_arg_call_elides() {
    // RUNES control: the raw direct zero-arg call elides to the bare callee.
    // Oracle: $.html(node, local);
    let js = emit(
        "<script>import { local } from './x.js';\nlet { q } = $props();</script>\n{@html local()}\n<p>{q}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.html(node, local)"),
        "the runes raw call elides the thunk:\n{js}"
    );
    assert!(
        !js.contains("$.untrack"),
        "no wrap machinery in a runes module:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_const_initializer_wraps_in_safe_equal_derived() {
    // Oracle: const y = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));
    let js = emit(
        &format!("{LEGACY_OBJ}{{#each [1] as item}}{{@const y = obj.m()}}<p>{{y}}</p>{{/each}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "const y = $.derived_safe_equal(() => ({}));",
            obj_wrap("obj().m()")
        )),
        "the legacy {{@const}} wraps inside $.derived_safe_equal:\n{js}"
    );
    assert!(
        !js.contains("$.derived("),
        "a non-runes {{@const}} never uses plain $.derived:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn maybe_runes_const_uses_safe_equal_without_wrap() {
    // Oracle (maybe-runes): const y = $.derived_safe_equal(() => $s().m());
    let js = emit(
        "<script>import { writable } from 'svelte/store';\nconst s = writable({});</script>\n{#each [1] as item}{@const y = $s.m()}<p>{y}</p>{/each}\n",
        "App.svelte",
    );
    assert!(
        js.contains("const y = $.derived_safe_equal(() => ($s().m()));")
            || js.contains("const y = $.derived_safe_equal(() => $s().m());"),
        "the maybe-runes {{@const}} keeps safe_equal, unwrapped (cosmetic parens waived):\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "the legacy wrap never applies in maybe-runes mode:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_const_uses_plain_derived() {
    // Oracle (runes): const y = $.derived(() => $$props.obj.m());
    let js = emit(
        "<script>let { obj } = $props();</script>\n{#each [1] as item}{@const y = obj.m()}<p>{y}</p>{/each}\n",
        "App.svelte",
    );
    assert!(
        js.contains("const y = $.derived(() => ($$props.obj.m()));")
            || js.contains("const y = $.derived(() => $$props.obj.m());"),
        "the runes {{@const}} uses plain $.derived (cosmetic parens waived):\n{js}"
    );
    assert!(
        !js.contains("$.derived_safe_equal"),
        "runes never uses safe_equal:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn every_reactive_text_node_carries_exactly_one_prepared_op() {
    // The reactive-text accept path is TOTAL: every projected
    // `ClientNode::ReactiveText` node carries exactly ONE prepared
    // `ClientRuntimeOp::ReactiveText` op — the sole source `reactive_text_for`
    // serves the emitter from. No interpolation reaches emission without a
    // prepared carrier, so the emitter has no raw-source path (an absent op is
    // an internal routing defect that fails CLOSED, never a silent raw
    // emission).
    use crate::svelte::runtime::client_plan::SupportedClientIr;
    use crate::svelte::runtime::client_plan_types::{ClientNode, ClientRuntimeOp};
    use crate::svelte::runtime::client_surface::ClientSyntaxSurface;
    use crate::svelte::runtime::lower_parsed_svelte_to_ir;
    for source in [
        // A pure single interpolation.
        "<script>let { x } = $props();</script>\n<p>{x}</p>\n",
        // A MIXED literal/interpolation run (two interps in one text node).
        "<script>let { x, y } = $props();</script>\n<p>a {x} b {y} c</p>\n",
        // The definitely-legacy imported-member shape (the wrap path).
        "<script>import * as NS from './x.js';\nexport let p;</script>\n<p>{NS.z}</p>\n",
        // An interpolation inside a nested block region.
        "<script>let { x, c } = $props();</script>\n{#if c}<p>{x}</p>{/if}\n",
        // A top-level interpolation (root fragment).
        "<script>let { x } = $props();</script>\n<p>t</p>\n{x}\n",
    ] {
        let alloc = Allocator::default();
        let parsed = parse_svelte(source);
        let opts = SvelteRuntimeOptions {
            filename: Some("App.svelte".to_string()),
            ..Default::default()
        };
        let ir =
            lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc).expect("lowering succeeds");
        let classified = ClientSyntaxSurface::classify(&ir).expect("the surface classifies");
        let plan = SupportedClientIr::build(&classified, &ir, None).expect("the plan builds");
        let mut reactive_nodes = 0usize;
        for (idx, node) in plan.nodes.iter().enumerate() {
            if matches!(node, ClientNode::ReactiveText { .. }) {
                reactive_nodes += 1;
                let ops = plan
                    .all_ops()
                    .filter(|op| {
                        matches!(op, ClientRuntimeOp::ReactiveText { target, .. }
                            if target.0 == idx as u32)
                    })
                    .count();
                assert_eq!(
                    ops, 1,
                    "reactive interpolation node {idx} must carry exactly one prepared \
                     ReactiveText op in:\n{source}"
                );
            }
        }
        assert!(
            reactive_nodes > 0,
            "the fixture must exercise reactive text: {source}"
        );
    }
}
