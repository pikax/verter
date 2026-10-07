use super::*;

#[test]
fn hello_input_emits_the_full_section_1_2_module() {
    // The headline §1.2 conformance target. Asserts the load-bearing structural
    // facts that must match official byte-for-byte where they are not cosmetic.
    let js = emit(HELLO_INPUT, "App.svelte");

    // (1) Imports — the disclose-version side effect + the client namespace.
    assert!(
        js.contains("import 'svelte/internal/disclose-version';"),
        "missing disclose-version import:\n{js}"
    );
    assert!(
        js.contains("import * as $ from 'svelte/internal/client';"),
        "missing client namespace import:\n{js}"
    );

    // (2) The template factory — the 3-root fragment with the trailing `1` flag.
    assert!(
        js.contains("$.from_html(`<h1> </h1> <input/> <button> </button>`, 1)"),
        "template factory drift (must match official skeleton + fragment flag):\n{js}"
    );

    // (3) The export shape — `export default function App($$anchor)` (no $$props).
    assert!(
        js.contains("export default function App($$anchor) {"),
        "export shape drift:\n{js}"
    );
    // NEGATIVE: no `$$props` param (this component has no props).
    assert!(
        !js.contains("App($$anchor, $$props)"),
        "a propless component must NOT thread $$props:\n{js}"
    );

    // (4) The state declarations — both reassigned primitives → `$.state(init)`.
    assert!(
        js.contains("let name = $.state('world');"),
        "name state decl:\n{js}"
    );
    assert!(
        js.contains("let count = $.state(0);"),
        "count state decl:\n{js}"
    );

    // (5) The clone frame — a 3-root fragment clones via `var fragment = root();`.
    assert!(
        js.contains("var fragment = root();"),
        "fragment clone frame:\n{js}"
    );

    // (6) The walk — first_child(fragment), child(h1), reset(h1), sibling(h1, 2),
    //     remove_input_defaults(input), sibling(input, 2), child(button),
    //     reset(button). The sibling OFFSETS (2) skip the inter-root text nodes.
    assert!(js.contains("$.first_child(fragment)"), "first_child:\n{js}");
    assert!(js.contains("$.sibling(h1, 2)"), "sibling(h1, 2):\n{js}");
    assert!(
        js.contains("$.sibling(input, 2)"),
        "sibling(input, 2):\n{js}"
    );
    assert!(js.contains("$.reset(h1)"), "reset(h1):\n{js}");
    assert!(js.contains("$.reset(button)"), "reset(button):\n{js}");

    // (7) `$.remove_input_defaults(input)` — emitted AFTER the input is named and
    //     BEFORE `$.bind_value`.
    let rid = js
        .find("$.remove_input_defaults(input)")
        .expect("remove_input_defaults");
    let bind = js.find("$.bind_value(input").expect("bind_value");
    assert!(
        rid < bind,
        "remove_input_defaults must precede bind_value:\n{js}"
    );

    // (8) ONE grouped `$.template_effect` containing BOTH set_text writes (mixed
    //     text → the `?? ''` template-literal form).
    assert_eq!(
        js.matches("$.template_effect(").count(),
        1,
        "exactly one grouped template_effect:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, `Hello ${$.get(name) ?? ''}!`)"),
        "h1 mixed-text effect:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text_1, `clicks: ${$.get(count) ?? ''}`)"),
        "button mixed-text effect:\n{js}"
    );

    // (9) The bind + the delegated event.
    assert!(
        js.contains("$.bind_value(input, () => $.get(name), ($$value) => $.set(name, $$value))"),
        "bind_value shape:\n{js}"
    );
    assert!(
        js.contains("$.delegated('click', button, () => $.set(count, $.get(count) + 1))"),
        "delegated event shape:\n{js}"
    );

    // (10) The mount + the delegate epilogue.
    assert!(
        js.contains("$.append($$anchor, fragment);"),
        "append mount:\n{js}"
    );
    assert!(
        js.contains("$.delegate(['click']);"),
        "delegate epilogue:\n{js}"
    );

    // NEGATIVES: no $.push/$.pop (no $effect); no $.first_child applied twice.
    assert!(!js.contains("$.push("), "no $effect → no $.push:\n{js}");
    assert!(!js.contains("$.pop("), "no $effect → no $.pop:\n{js}");
}

#[test]
fn pure_single_interpolation_has_no_nullish_coalesce() {
    // A PURE single `{count}` interpolation emits `$.set_text(text, $.get(count))`
    // — NOT `$.set_text(text, \`${$.get(count) ?? ''}\`)`. (Verified against the
    // oracle: the `?? ''` is mixed-text-only.)
    let src = "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, $.get(count))"),
        "pure interpolation is a direct value:\n{js}"
    );
    assert!(
        !js.contains("?? ''"),
        "a pure single interpolation must NOT get the `?? ''` mixed-text form:\n{js}"
    );
    // The increment lowers to `$.update`.
    assert!(
        js.contains("$.delegated('click', button, () => $.update(count))"),
        "update:\n{js}"
    );
    // F11: the pure-interp text child carries the `is_text` flag `$.child(button,
    // true)`. Verified against svelte@5.56.10.
    assert!(
        js.contains("$.child(button, true)"),
        "a pure-interp text child carries the is_text flag:\n{js}"
    );
}

#[test]
fn multi_root_fragment_without_collision_keeps_bare_fragment_name() {
    // NEGATIVE (collision-rename fires ONLY on a real collision): a multi-root
    // fragment whose user script has NO binding named `fragment` keeps the bare
    // `var fragment = root();` clone frame byte-identical — the seeded allocator
    // returns the preferred stem unchanged when it is free.
    let src = "<script>let count = $state(0);</script>\n<button onclick={() => count++}>a</button><p>{count}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("var fragment = root();"),
        "a non-colliding multi-root fragment must keep the bare `fragment` name:\n{js}"
    );
    assert!(
        !js.contains("fragment_1"),
        "no collision → no `_N` suffix:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn section_1_2_fragment_local_stays_byte_identical_under_seeded_allocator() {
    // NEGATIVE (§1.2 byte-equivalence): the §1.2 example has `let name` / `let count`
    // — NO user binding named `fragment` / `text` / `h1` / `input` / `button` / `root`
    // — so routing the multi-root clone frame through the seeded allocator must yield
    // the SAME synthesized names. The clone frame stays `var fragment = root();` and
    // the text locals stay `text` / `text_1` exactly.
    let js = emit(HELLO_INPUT, "App.svelte");
    assert!(
        js.contains("var fragment = root();"),
        "§1.2 clone frame must stay byte-identical (`var fragment = root();`):\n{js}"
    );
    assert!(
        js.contains("$.append($$anchor, fragment);"),
        "§1.2 mount must stay `$.append($$anchor, fragment);`:\n{js}"
    );
    // The text-run locals are unchanged (no collision pushes them to `_N`).
    assert!(
        js.contains("$.set_text(text, `Hello ${$.get(name) ?? ''}!`)"),
        "§1.2 first text local must stay `text`:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text_1, `clicks: ${$.get(count) ?? ''}`)"),
        "§1.2 second text local must stay `text_1`:\n{js}"
    );
}

#[test]
fn props_non_literal_default_lowers_lazy_thunk() {
    // A non-simple `$props()` default (`[]`) is the LAZY flag-19 thunk form
    // (`$.prop($$props, 'a', 19, () => [])`). Verified against svelte@5.56.10.
    let src = "<script>let { a = [] } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 19, () => [])"),
        "a non-simple default is the lazy thunk carrier:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, a())"),
        "a prop-source read is the getter call:\n{js}"
    );
    // NEGATIVE: a PLAIN (non-bindable) lazy default never proxies.
    assert!(
        !js.contains("$.proxy"),
        "a plain lazy default must not proxy:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn await_expression_in_interpolation_fails_closed() {
    // The interpolation carrier is supported, but the nested `await` retains
    // its own experimental-async refusal instead of being misreported as a
    // generic interpolation-shape failure.
    assert_fail_closed(
        "<script>let p = $state(0); let n = $state(0);</script>\n<button onclick={() => n++}>{(async () => await p)()}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ExperimentalAsync { surface, .. } if *surface == "await"),
    );
}

#[test]
fn capture_event_emits_the_capture_positional_arg() {
    // A CAPTURE-phase event (`onclickcapture`) is a non-delegated `$.event` with the
    // capture flag as the 4th positional `true` (official `build_event`). It NO LONGER
    // fails closed — the regular-element capture surface is supported.
    let js = emit(
        "<script>let n = $state(0);</script>\n<button onclickcapture={() => n++}>x</button>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.event('click', button, () => $.update(n), true)")),
        "a capture event must emit the 4th positional `true`:\n{js}"
    );
    // Negative: a capture event is NEVER delegated (no `$.delegated`, no `$.delegate`).
    assert!(
        !js.contains("$.delegated(") && !js.contains("$.delegate("),
        "a capture event must not delegate:\n{js}"
    );
}

#[test]
fn legacy_on_unknown_modifier_event_fails_closed() {
    // A legacy `on:click|stop` directive carries an UNRECOGNIZED modifier (`stop` is
    // not in the official `EVENT_MODIFIERS` set) — the official
    // `event_handler_invalid_modifier` compile error. Verter keeps it fail-closed /
    // refused (the VALID legacy modifiers are supported; an invalid one is not).
    assert_fail_closed(
        "<script>let n = $state(0);</script>\n<button on:click|stop={() => n++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. }),
    );
}

#[test]
fn event_smoke_modules_match_the_committed_jsdom_fixtures() {
    // Each behavioral event-smoke fixture's emitted module stays in lockstep with the
    // committed `.client.mjs` the happy-dom spec (`svelte-client-events-smoke.spec.ts`)
    // mounts — so the behavioral smoke can never drift from `compile_client`.
    for (name, src) in EVENT_SMOKE_FIXTURES {
        assert_jsdom_fixture_in_sync(src, &format!("{name}.client.mjs"));
    }
}

#[test]
#[ignore = "generator: writes the committed event smoke fixtures (run once, then oxfmt)"]
fn regen_event_smoke_fixtures() {
    for (name, src) in EVENT_SMOKE_FIXTURES {
        let js = emit(src, "App.svelte");
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/svelte-runtime-tests/test/fixtures/svelte")
            .join(format!("{name}.client.mjs"));
        std::fs::write(&path, js).unwrap();
        println!("wrote {}", path.display());
    }
}

#[test]
fn animate_only_child_check_ignores_const_and_declaration_tags() {
    // The official `animate:` "only child of a keyed each" check IGNORES `{@const}`
    // and the `{const …}` / `{let …}` declaration tags (svelte@5.56.10
    // `2-analyze/visitors/shared/element.js`) — a keyed each whose body is a
    // declaration tag + ONE animated element is ACCEPTED. (The `{@const}` variant
    // also READS the binding — the `lifecycle/animate_keyed_const` golden shape;
    // the `{const}`/`{let}` variants keep a static body: the inert-declaration
    // read surface is a separate boundary, not the placement check under test.)
    for (label, tag, body) in [
        ("legacy {@const}", "{@const l = item.n}", "{l}"),
        ("declaration {const}", "{const l = item.n}", "x"),
        ("declaration {let}", "{let l = item.n}", "x"),
    ] {
        let src = format!(
            "<script>let {{ items }} = $props();</script>\n{{#each items as item (item.id)}}{tag}<div animate:flip>{body}</div>{{/each}}\n"
        );
        let js = emit(&src, "App.svelte");
        assert!(
            js.contains("$.animation(div, () => flip, null);"),
            "{label}: a declaration-tag sibling must not refuse the animate placement:\n{js}"
        );
    }
    // NEGATIVE discriminators — official keeps these SIGNIFICANT (rejects), and so
    // must Verter: a `{@debug}` sibling, a sibling ELEMENT, and non-whitespace TEXT.
    for (label, body) in [
        (
            "{@debug} sibling",
            "{@debug item}<div animate:flip>{item.n}</div>",
        ),
        (
            "sibling element",
            "<span></span><div animate:flip>{item.n}</div>",
        ),
        ("sibling text", "text<div animate:flip>{item.n}</div>"),
    ] {
        let src = format!(
            "<script>let {{ items }} = $props();</script>\n{{#each items as item (item.id)}}{body}{{/each}}\n"
        );
        let err = emit_result(&src).expect_err(label);
        assert!(
            matches!(
                err,
                ClientCompileError::Unsupported(
                    UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { .. }
                )
            ),
            "{label}: a significant sibling must still refuse the animate placement: {err:?}"
        );
    }
}

#[test]
fn use_action_effect_wraps_each_nondelegated_event() {
    // A `use:` action co-located with LEGACY `on:` events wraps EACH event
    // registration in its OWN `$.effect(() => $.event(…))`, emitted in the INIT
    // domain after `$.action` and before `$.transition` — the official
    // action-triggered effect wrap (svelte@5.56.10). The wrap trigger is the
    // LEGACY origin + the action host: a MODERN non-delegated event beside
    // `use:` stays a BARE `$.event` (the `lifecycle/use_modern_nondelegated_event`
    // golden + `lifecycle_event_origin_gates_effect_wrap_and_directive_batch_order`).
    let js = emit(
        "<script>let c = $state(0);</script>\n<div use:foo on:click={() => c++} on:keydown={() => c++}>x</div>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc(
            "$.effect(() => $.event('click', div, () => $.update(c)))"
        )),
        "use:+on:click must effect-wrap the click registration:\n{js}"
    );
    assert!(
        norm.contains(&nc(
            "$.effect(() => $.event('keydown', div, () => $.update(c)))"
        )),
        "use:+on:keydown must effect-wrap the keydown registration in its OWN effect:\n{js}"
    );
    // Order: `$.action` precedes both event effects (source order: use: first).
    let action = js.find("$.action(").expect("emits the action");
    let click = js.find("$.event('click'").expect("emits the click event");
    let keydown = js
        .find("$.event('keydown'")
        .expect("emits the keydown event");
    assert!(
        action < click && click < keydown,
        "init order must be action → effect(click) → effect(keydown):\n{js}"
    );
    // NEGATIVE: no BARE (unwrapped) `$.event` registration statement remains — every
    // `$.event` on this element is inside an effect wrap.
    assert!(
        !js.contains("\t$.event("),
        "a use:-hosted legacy on: event must not ALSO emit a bare $.event:\n{js}"
    );
}

#[test]
fn use_action_effect_wrap_orders_before_transition() {
    // `use:` + `transition:` + `on:click` — the official init order is
    // `$.action` → `$.effect(() => $.event(…))` → `$.transition(3, …)`.
    let js = emit(
        "<script>let c = $state(0);</script>\n<div use:foo transition:fade on:click={() => c++}>x</div>\n",
        "App.svelte",
    );
    let action = js.find("$.action(").expect("emits the action");
    let effect = js
        .find("$.effect(() => $.event('click'")
        .expect("effect-wraps the click event");
    let transition = js.find("$.transition(3, ").expect("emits the transition");
    assert!(
        action < effect && effect < transition,
        "init order must be action → effect(event) → transition:\n{js}"
    );
}

#[test]
fn use_action_effect_wraps_non_this_bind() {
    // A non-`this` DOM bind on a `use:` action host wraps its registration in its
    // OWN `$.effect(() => $.bind_*(...))` in the INIT domain at the bind's
    // attribute source position — after the source-first `$.action` (official
    // svelte@5.56.10 `RegularElement.js` under `has_use`). `bind:this` is NEVER
    // wrapped (`lifecycle/use_bind_this` pins the unwrapped inline interleave).
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input use:foo bind:value={v} />\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc(
            "$.effect(() => $.bind_value(input, () => $.get(v), ($$value) => $.set(v, $$value)))"
        )),
        "use:+bind:value must effect-wrap the bind registration:\n{js}"
    );
    // Order: the source-first `$.action` precedes the wrapped bind.
    let action = js.find("$.action(").expect("emits the action");
    let bind = js.find("$.bind_value(").expect("emits the bind");
    assert!(
        action < bind,
        "init order must be action → effect(bind_value):\n{js}"
    );
    // NEGATIVE: no BARE (unwrapped) `$.bind_value` statement remains — the sole
    // registration on this element lives inside the effect wrap.
    assert!(
        !js.contains("\t$.bind_value("),
        "a use:-hosted non-this bind must not ALSO emit a bare $.bind_value:\n{js}"
    );
}

#[test]
fn transition_and_bind_preserve_source_order_in_batch() {
    // A non-`this` bind on a lifecycle host WITHOUT `use:` joins the element's
    // after-update DIRECTIVE BATCH, source-ordered with `$.transition` — BARE,
    // never effect-wrapped, in BOTH source directions (official svelte@5.56.10
    // batches the bind with `other_directives`).
    let first = emit(
        "<script>let v = $state(\"\");</script>\n<input transition:fade bind:value={v} />\n",
        "App.svelte",
    );
    let t = first
        .find("$.transition(3, ")
        .expect("emits the transition");
    let b = first.find("$.bind_value(").expect("emits the bind");
    assert!(
        t < b,
        "a source-first transition precedes the bind in the batch:\n{first}"
    );
    assert!(
        first.contains("\t$.bind_value("),
        "the batch bind is a BARE statement:\n{first}"
    );
    // NEGATIVE: no effect wrap without `use:`.
    assert!(
        !first.contains("$.effect(() => $.bind_value("),
        "a non-use: host must not effect-wrap the bind:\n{first}"
    );
    // The reverse source order reverses the emission.
    let second = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={v} transition:fade />\n",
        "App.svelte",
    );
    let b = second.find("$.bind_value(").expect("emits the bind");
    let t = second
        .find("$.transition(3, ")
        .expect("emits the transition");
    assert!(
        b < t,
        "a source-first bind precedes the transition in the batch:\n{second}"
    );
    assert!(
        !second.contains("$.effect(() => $.bind_value("),
        "the reverse source order must not effect-wrap either:\n{second}"
    );
}

#[test]
fn legacy_event_and_bind_source_order_without_lifecycle() {
    // WITHOUT any lifecycle directive, a bare LEGACY `on:` event and a non-`this`
    // bind share the after-update batch in attribute SOURCE order — the batch is
    // not lifecycle-gated (a bind/event element with no `use:`/`transition:` still
    // batches), and neither registration wraps.
    let event_first = emit(
        "<script>let c = $state(0); let v = $state(\"\");</script>\n<input on:input={() => c++} bind:value={v} />\n",
        "App.svelte",
    );
    let e = event_first
        .find("$.event('input'")
        .expect("emits the event");
    let b = event_first.find("$.bind_value(").expect("emits the bind");
    assert!(
        e < b,
        "a source-first legacy on:input precedes the bind:\n{event_first}"
    );
    // NEGATIVE: both BARE — no effect wrap on either registration (no `use:`).
    assert!(
        !event_first.contains("$.effect(() =>"),
        "neither registration may effect-wrap without use::\n{event_first}"
    );
    // The reverse source order reverses the emission.
    let bind_first = emit(
        "<script>let c = $state(0); let v = $state(\"\");</script>\n<input bind:value={v} on:input={() => c++} />\n",
        "App.svelte",
    );
    let b = bind_first.find("$.bind_value(").expect("emits the bind");
    let e = bind_first.find("$.event('input'").expect("emits the event");
    assert!(
        b < e,
        "a source-first bind precedes the legacy on:input:\n{bind_first}"
    );
}

#[test]
fn modern_nondelegated_event_emits_before_bind() {
    // A MODERN non-delegated `on*` attribute pushes its bare `$.event` BEFORE the
    // element's after-update batch, so it precedes the bind even when the bind is
    // authored FIRST — the modern event never joins the batch and never wraps.
    let js = emit(
        "<script>let c = $state(0); let v = $state(\"\");</script>\n<input bind:value={v} onmouseenter={() => c++} />\n",
        "App.svelte",
    );
    let e = js.find("$.event('mouseenter'").expect("emits the event");
    let b = js.find("$.bind_value(").expect("emits the bind");
    assert!(
        e < b,
        "the modern non-delegated event precedes the source-first bind:\n{js}"
    );
    // NEGATIVE: neither registration effect-wraps (no `use:` host).
    assert!(
        !js.contains("$.effect(() =>"),
        "no effect wrap without use::\n{js}"
    );
}

#[test]
fn use_action_modern_event_and_bind_ordering() {
    // Mixed slots on one `use:` host: the init domain emits `$.action` then the
    // WRAPPED bind (at its attribute source position), and the MODERN
    // non-delegated event emits post-walk — the wrapped bind (init) BEFORE the
    // bare modern `$.event`, even though the event is authored before the bind.
    let js = emit(
        "<script>let c = $state(0); let v = $state(\"\");</script>\n<input use:foo onmouseenter={() => c++} bind:value={v} />\n",
        "App.svelte",
    );
    let action = js.find("$.action(").expect("emits the action");
    let bind = js
        .find("$.effect(() => $.bind_value(")
        .expect("effect-wraps the bind");
    let event = js
        .find("$.event('mouseenter'")
        .expect("emits the modern event");
    assert!(
        action < bind && bind < event,
        "order must be action → effect(bind_value) → event('mouseenter'):\n{js}"
    );
    // NEGATIVE: no bare bind statement remains, and the modern event stays
    // UNwrapped (the wrap is bind/legacy-`on:`-scoped, never a modern event).
    assert!(
        !js.contains("\t$.bind_value("),
        "the use:-hosted bind must not also emit bare:\n{js}"
    );
    assert!(
        !js.contains("$.effect(() => $.event("),
        "a modern event never effect-wraps:\n{js}"
    );
}

#[test]
fn euler_parent_transition_precedes_after_child_modern_event() {
    // Euler-tour nesting, batch × modern event: the CHILD's modern non-delegated
    // event joins the after-update stream at the child's ENTER rank, while the
    // PARENT's transition (its own directive batch) merges at the parent's EXIT
    // rank — every descendant position precedes the parent's exit, so the child's
    // `$.event` emits BEFORE the parent's `$.transition` even though the
    // transition is authored first (official svelte@5.56.10:
    // `$.event('mouseenter', input, …)` then `$.transition(3, div, …)`).
    let js = emit(
        "<script>let c = $state(0);</script>\n<div transition:fade><input onmouseenter={() => c++} /></div>\n",
        "App.svelte",
    );
    let event = js
        .find("$.event('mouseenter'")
        .expect("emits the child's modern event");
    let transition = js
        .find("$.transition(3, div")
        .expect("emits the parent's transition");
    assert!(
        event < transition,
        "the child's modern event (ENTER) must precede the parent's transition (EXIT):\n{js}"
    );
    // NEGATIVE: the reverse order does NOT hold — no transition is emitted
    // anywhere before the child's event (a flat source-order model would put the
    // parent's authored-first transition first).
    assert!(
        !js[..event].contains("$.transition("),
        "no $.transition may precede the child's modern event:\n{js}"
    );
    // NEGATIVE: the modern event stays a BARE direct registration — never
    // effect-wrapped, never delegated (mouseenter is not in the delegated set).
    assert!(
        !js.contains("$.effect(() => $.event(") && !js.contains("$.delegated("),
        "the modern non-delegated event stays a bare $.event:\n{js}"
    );
}

#[test]
fn euler_parent_bind_emits_after_child_modern_event() {
    // Euler-tour nesting, bind × modern event: the PARENT's non-`this` bind
    // (`bind:clientWidth` — its directive batch) merges at the parent's EXIT
    // rank, AFTER the child's ENTER-ranked modern event (official svelte@5.56.10:
    // `$.event('mouseenter', input, …)` then
    // `$.bind_element_size(div, 'clientWidth', …)`).
    let js = emit(
        "<script>let c = $state(0); let w = $state(0);</script>\n<div bind:clientWidth={w}><input onmouseenter={() => c++} /></div>\n",
        "App.svelte",
    );
    let event = js
        .find("$.event('mouseenter'")
        .expect("emits the child's modern event");
    let bind = js
        .find("$.bind_element_size(div")
        .expect("emits the parent's bind");
    assert!(
        event < bind,
        "the child's modern event (ENTER) must precede the parent's bind (EXIT):\n{js}"
    );
    // NEGATIVE: the reverse order does NOT hold — no bind registration is emitted
    // anywhere before the child's event.
    assert!(
        !js[..event].contains("$.bind_element_size("),
        "no $.bind_element_size may precede the child's modern event:\n{js}"
    );
    // NEGATIVE: neither registration effect-wraps (no `use:` host anywhere).
    assert!(
        !js.contains("$.effect(() =>"),
        "no effect wrap without use::\n{js}"
    );
}

#[test]
fn euler_parent_modern_event_precedes_child_transition() {
    // Euler-tour nesting, the other direction: the PARENT's modern event joins
    // the stream at the parent's ENTER rank — BEFORE every child position — while
    // the CHILD's transition merges at the child's EXIT rank, so the parent's
    // `$.event` emits FIRST even though a child's batch beats a PARENT's batch
    // (official svelte@5.56.10: `$.event('mouseenter', div, …)` then
    // `$.transition(3, span, …)`).
    let js = emit(
        "<script>let c = $state(0);</script>\n<div onmouseenter={() => c++}><span transition:fade></span></div>\n",
        "App.svelte",
    );
    let event = js
        .find("$.event('mouseenter'")
        .expect("emits the parent's modern event");
    let transition = js
        .find("$.transition(3, span")
        .expect("emits the child's transition");
    assert!(
        event < transition,
        "the parent's modern event (ENTER) must precede the child's transition (EXIT):\n{js}"
    );
    // NEGATIVE: the reverse order does NOT hold — no transition is emitted
    // anywhere before the parent's event (a child-batch-always-first model would
    // put the span's transition first).
    assert!(
        !js[..event].contains("$.transition("),
        "no $.transition may precede the parent's modern event:\n{js}"
    );
}

#[test]
fn euler_sibling_transition_precedes_sibling_modern_event() {
    // Euler-tour SIBLINGS keep document order: the first sibling's transition
    // (its EXIT rank) precedes the second sibling's modern event (its ENTER rank)
    // — enter/exit pairs of disjoint subtrees never interleave (official
    // svelte@5.56.10: `$.transition(3, span, …)` then
    // `$.event('mouseenter', input, …)`). A phase-split model that hoists every
    // modern event before every batch item would reverse this.
    let js = emit(
        "<script>let c = $state(0);</script>\n<div><span transition:fade></span><input onmouseenter={() => c++} /></div>\n",
        "App.svelte",
    );
    let transition = js
        .find("$.transition(3, span")
        .expect("emits the first sibling's transition");
    let event = js
        .find("$.event('mouseenter'")
        .expect("emits the second sibling's modern event");
    assert!(
        transition < event,
        "the first sibling's transition must precede the second sibling's modern event (document order):\n{js}"
    );
    // NEGATIVE: the reverse order does NOT hold — no modern-event registration is
    // emitted anywhere before the transition.
    assert!(
        !js[..transition].contains("$.event('mouseenter'"),
        "no $.event may precede the sibling transition:\n{js}"
    );
}

#[test]
fn nondelegated_event_without_use_action_stays_bare() {
    // The effect wrap is triggered SPECIFICALLY by a `use:` action on a LEGACY
    // `on:` event — a co-located `transition:`, `{@attach}`, or reactive attribute
    // does NOT wrap: the legacy event stays the BARE `$.event(…)` registration
    // (in the post-transition directive batch). And a MODERN delegated event stays
    // delegated even beside `use:`.
    for (label, src) in [
        (
            "transition:+on:",
            "<script>let c = $state(0);</script>\n<div transition:fade on:click={() => c++}>x</div>\n",
        ),
        (
            "attach+on:",
            "<script>let c = $state(0);</script>\n<div {@attach fn} on:click={() => c++}>x</div>\n",
        ),
        (
            "reactive-attr+on:",
            "<script>let c = $state(0);</script>\n<div id={c} on:click={() => c++}>x</div>\n",
        ),
    ] {
        let js = emit(src, "App.svelte");
        assert!(
            js.contains("\t$.event('click', div, "),
            "{label}: a non-use element keeps the BARE $.event registration:\n{js}"
        );
        assert!(
            !js.contains("$.effect(() => $.event("),
            "{label}: only a `use:` action triggers the effect wrap:\n{js}"
        );
    }
    // MODERN delegated events are unaffected by `use:` — still `$.delegate`, no wrap.
    let js = emit(
        "<script>let c = $state(0);</script>\n<div use:foo onclick={() => c++}>x</div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegate(['click']);"),
        "use:+MODERN onclick stays delegated:\n{js}"
    );
    assert!(
        !js.contains("$.effect(() => $.event("),
        "a delegated event must never effect-wrap:\n{js}"
    );
}

#[test]
fn nondelegated_event_emits_a_direct_event_listener() {
    // A non-bubbling event (`onfocus`, not in the delegated set) is a DIRECT
    // `$.event('focus', node, handler)` — never delegated, no trailing args.
    let js = emit(
        "<script>let n = $state(0);</script>\n<button onfocus={() => n++}>x</button>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc("$.event('focus', button, () => $.update(n))")),
        "a non-delegated event must emit a direct $.event:\n{js}"
    );
    assert!(
        !js.contains("$.delegated(") && !js.contains("$.delegate("),
        "a non-delegated event must not delegate:\n{js}"
    );
}

#[test]
fn nondelegated_function_expression_handler_emits_a_direct_event_listener() {
    // A non-delegated DIRECT event whose handler is an inline FUNCTION EXPRESSION (not an
    // arrow) is accepted and passed through to `$.event`, with its `$state`-write body
    // lowered through the shared rewriter (`n++` → `$.update(n)`) — matching the official
    // `$.event('focus', button, function () { $.update(n); })`. This pins that the
    // accepted direct-handler surface includes the function-expression form (the
    // `events/nondelegated_funcexpr` structural golden is the full-module oracle).
    let js = emit(
        "<script>let n = $state(0);</script>\n<button onfocus={function () { n++; }}>x</button>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc("$.event('focus', button, function")),
        "a function-expression handler must reach a direct $.event:\n{js}"
    );
    assert!(
        norm.contains(&nc("$.update(n)")),
        "the function-expression body's $state write must be rewritten:\n{js}"
    );
    assert!(
        !js.contains("$.delegated(") && !js.contains("$.delegate("),
        "a non-delegated function-expression handler must not delegate:\n{js}"
    );
}

#[test]
fn bare_identifier_direct_event_handler_fails_closed() {
    // A bare-identifier DIRECT event handler (`onfocus={s}`) is refused here rather than
    // emitted unproven, because this surface lacks the binding-aware event-handler split
    // that official Svelte applies to such a handler. There is NO single fixed official
    // emission for `onfocus={s}`: `build_event_handler` inspects the binding, so a demoted
    // (non-reactive) value can pass straight through as the bare `s`, while a still-reactive
    // signal is wrapped — `function (...$$args) { $.get(s)?.apply(this, $$args) }` — so the
    // value is unwrapped per call instead of read once at registration. This surface owns
    // neither arm of that split and cannot prove which form a given binding warrants;
    // passing the raw binding through as the `$.event` 3rd argument would be a value, not the
    // correct per-binding handler. So the bare-identifier shape fails closed, matching the
    // delegated path (which never accepted bare identifiers). Discriminating: a direct
    // classifier broad enough to accept this shape would emit an unproven handler value;
    // fail-closing it is the correct boundary. (A `$props()`-member identifier is not
    // exercised here: the native client path does not yet support `$props()`, so such a
    // component would refuse at the instance-script gate rather than at the handler-shape
    // gate under test.)
    for src in [
        "<script>let s = $state(0);</script>\n<button onfocus={s}>x</button>\n",
        "<script>let s = $state(0);</script>\n<div onmouseenter={s}>x</div>\n",
    ] {
        assert_fail_closed(src, |s| {
            matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. })
        });
    }
}

#[test]
fn multiple_events_on_one_element_each_resolve_to_their_own_registration() {
    // An element carrying TWO events — a DELEGATED `onclick` and a non-delegated
    // `onfocus`, each with its OWN handler — emits BOTH registrations with the correct
    // per-event handler. The per-event shape fact is keyed by (node, event type, handler
    // expr), so the second event does not collapse onto the element's first recorded
    // event. (No delegated regression: the delegated click still emits `$.delegated` plus
    // the `$.delegate(['click'])` epilogue.)
    let js = emit(
        "<script>let a = $state(0);\nlet b = $state(0);</script>\n<button onclick={() => a++} onfocus={() => b++}>x</button>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc("$.delegated('click', button, () => $.update(a))")),
        "the delegated click must emit with its OWN handler:\n{js}"
    );
    assert!(
        norm.contains(&nc("$.event('focus', button, () => $.update(b))")),
        "the non-delegated focus must emit with its OWN handler:\n{js}"
    );
    assert!(
        js.contains("$.delegate(['click'])"),
        "the delegated click epilogue must remain (no delegated-path regression):\n{js}"
    );
}

#[test]
fn each_legacy_modifier_wraps_the_handler_in_its_official_helper() {
    // Each individual legacy modifier wraps the handler in its official
    // `svelte/internal/client` helper (`$.<modifier>(handler)`).
    for (modifier, helper) in [
        ("preventDefault", "preventDefault"),
        ("stopPropagation", "stopPropagation"),
        ("stopImmediatePropagation", "stopImmediatePropagation"),
        ("self", "self"),
        ("trusted", "trusted"),
        ("once", "once"),
    ] {
        let src = format!(
            "<script>let n = $state(0);</script>\n<button on:click|{modifier}={{() => n++}}>x</button>\n"
        );
        let js = emit(&src, "App.svelte");
        let norm = normalize_js_cosmetics(&js);
        let expected = format!("$.event('click', button, $.{helper}(() => $.update(n)))");
        assert!(
            norm.contains(&nc(&expected)),
            "the `{modifier}` modifier must wrap via $.{helper}:\n{js}"
        );
    }
}

#[test]
fn delegated_onclick_is_unchanged_with_no_trailing_positional_args() {
    // No regression: a delegated modern `onclick` still emits `$.delegated('click',
    // node, handler)` (no capture/passive trailing args) + the `$.delegate(['click'])`
    // epilogue.
    let js = emit(
        "<script>let n = $state(0);</script>\n<button onclick={() => n++}>x</button>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc("$.delegated('click', button, () => $.update(n))")),
        "a delegated onclick must be unchanged:\n{js}"
    );
    assert!(
        js.contains("$.delegate(['click'])"),
        "a delegated onclick must register the delegate epilogue:\n{js}"
    );
    // Negative: a plain delegated click has NO trailing capture/passive positional.
    assert!(
        !norm.contains(&nc("$.delegated('click', button, () => $.update(n), ")),
        "a plain delegated onclick must emit no trailing positional args:\n{js}"
    );
}

#[test]
fn special_element_global_events_emit_direct_global_registrations() {
    // `<svelte:window|body|document on*>` EVENTS emit a DIRECT `$.event('<type>', <host>,
    // handler)` in the init body — NEVER `$.delegated`, NEVER a node var, and the no-DOM
    // host root emits NO template / NO `$.from_html` / NO `$.append` / NO `$.comment`.
    for (host_expr, event, src) in [
        (
            "$.window",
            "resize",
            "<script>let n = $state(0);</script>\n<svelte:window onresize={() => n++} />\n",
        ),
        (
            "$.document.body",
            "click",
            "<script>let n = $state(0);</script>\n<svelte:body onclick={() => n++} />\n",
        ),
        (
            "$.document",
            "keydown",
            "<script>let n = $state(0);</script>\n<svelte:document onkeydown={() => n++} />\n",
        ),
    ] {
        let js = emit(src, "App.svelte");
        let n = normalize_js_cosmetics(&js);
        assert!(
            n.contains(&nc(&format!(
                "$.event('{event}', {host_expr}, () => $.update(n))"
            ))),
            "host event must emit a direct $.event against {host_expr}:\n{js}"
        );
        // NEGATIVE: a global event is never delegated, and the no-DOM host root has no clone
        // frame / mount / comment anchor.
        assert!(
            !js.contains("$.delegated"),
            "host event is never delegated:\n{js}"
        );
        assert!(
            !js.contains("$.delegate("),
            "host event registers no delegate epilogue:\n{js}"
        );
        assert!(
            !js.contains("$.from_html"),
            "host root clones no template:\n{js}"
        );
        assert!(!js.contains("$.append"), "host root mounts nothing:\n{js}");
        assert!(
            !js.contains("$.comment"),
            "host root has no comment anchor:\n{js}"
        );
        assert!(
            parses_as_js(&js),
            "host event module must be valid JS:\n{js}"
        );
    }
}

#[test]
fn spread_payload_identifier_collision_renames_the_dom_var() {
    // A `<p {...p}>` collides: the DOM-var stem `p` clashes with the free spread-payload
    // identifier `p`. Official renames the DOM local to `p_1` so the `...p` payload still
    // refers to the binding, not the element node. Pinned svelte@5.56.10:
    // `var p_1 = ...; $.attribute_effect(p_1, () => ({ ...p }))`.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<p {...p}></p>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.attribute_effect(p_1, () => ({ ...p }))")),
        "a colliding {{...p}} payload must rename the DOM var to p_1:\n{js}"
    );
    // NEGATIVE: the DOM var must NOT shadow the payload as a bare `p`.
    assert!(
        !n.contains(&nc("$.attribute_effect(p, () => ({ ...p }))")),
        "the DOM var must not shadow the spread payload identifier:\n{js}"
    );
    assert!(
        !n.contains(&nc("var p = ")),
        "the colliding element must not declare `var p`:\n{js}"
    );
}

#[test]
fn spread_element_with_event_still_refuses() {
    // A spread element that ALSO carries an event handler is outside the decided fold
    // surface (the event-handler hoist the fold does not model) — it must refuse, not
    // silently fold the event. Routed through the event channel.
    let err = emit_result(
        "<script>let c = $state(0);</script>\n<div {...p} onclick={() => c++}></div>\n",
    )
    .expect_err("a spread element with an event must refuse");
    let ClientCompileError::Unsupported(surface) = err else {
        panic!("expected an Unsupported refusal, got {err:?}");
    };
    assert!(
        matches!(
            surface,
            UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. }
        ),
        "a spread element with an event must refuse via the event channel, got {surface:?}"
    );
}

#[test]
fn quoted_bind_value_function_pair_still_emits_bind_value() {
    // A QUOTED single-expression function-pair (`bind:value="{get, set}"`, a `Mixed`
    // value) is official-VALID and emits `$.bind_value(input, get, set)` (verified
    // svelte@5.56.10) — the bind-expr lowering unwraps the quoted single-`{…}` inner, so
    // the function-pair classification is identical to the bare form. This is the
    // POSITIVE CONTROL for FIX 1: the Mixed-aware group-reject gate + the defensive
    // identifier/member-only classifier check must NOT over-refuse a NON-group quoted
    // function-pair. Bare `bind:value={get, set}` is covered by
    // `bind_value_named_function_pair_lowers_decls_and_passes_idents`.
    let js = emit(
        "<script>let value = $state(0); function get(){ return value; } function set(next){ value = next; }</script>\n<input bind:value=\"{get, set}\" />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, get, set)"),
        "a QUOTED bind:value function-pair must still emit $.bind_value(input, get, set):\n{js}"
    );
    // NEGATIVE: it must NOT fail closed / drop the bind, and must NOT reject.
    assert!(
        js.contains("function get()") && js.contains("function set(next)"),
        "the quoted function-pair's named get/set declarations must be lowered:\n{js}"
    );
}

#[test]
fn bind_group_accumulator_renames_on_user_binding_collision() {
    // FIX 2: the `bind:group` accumulator must be allocated through the SAME seeded,
    // collision-aware name allocator the DOM-var stems use — NOT a hardcoded
    // `binding_group` constant. When the user declares their OWN `binding_group`,
    // official svelte@5.56.10 renames the accumulator to `binding_group_1` (keeping the
    // user's `binding_group`); verified shape:
    //   let binding_group = 0;
    //   const binding_group_1 = [];
    //   $.bind_group(binding_group_1, [], input, () => $.get(selected), ($$value) => …);
    // RED before the fix: the emitter used the hardcoded `binding_group` const for BOTH
    // the user's local AND the accumulator → a DUPLICATE `binding_group` declaration in
    // the component function scope (invalid JS, wrong routing).
    let js = emit(
        "<script>let binding_group = $state(0); let selected = $state('a');</script>\n<input type=\"radio\" bind:group={selected} value=\"a\">\n<input type=\"radio\" bind:group={selected} value=\"b\">\n",
        "App.svelte",
    );
    // OXC-PARSED no-duplicate proof: the name `binding_group` is DECLARED exactly once
    // (the user's `let`), and the accumulator is the renamed `binding_group_1`.
    assert_eq!(
        count_declared_binding(&js, "binding_group"),
        1,
        "the user's `binding_group` must be the SOLE `binding_group` declaration (no \
         colliding accumulator declaration):\n{js}"
    );
    assert_eq!(
        count_declared_binding(&js, "binding_group_1"),
        1,
        "the accumulator must be renamed to `binding_group_1` (one declaration):\n{js}"
    );
    // The renamed accumulator is declared as `[]` and is what the bind_group calls use.
    assert!(
        js.contains("const binding_group_1 = [];"),
        "the renamed accumulator must be declared `const binding_group_1 = [];`:\n{js}"
    );
    assert!(
        js.contains("$.bind_group(binding_group_1, [], input,")
            && js.contains("$.bind_group(binding_group_1, [], input_1,"),
        "both bind_group calls must reference the renamed `binding_group_1`:\n{js}"
    );
    // NEGATIVE: the colliding `const binding_group = [];` accumulator must NOT appear.
    assert!(
        !js.contains("const binding_group = [];"),
        "the accumulator must NOT collide with the user's `binding_group`:\n{js}"
    );
}

#[test]
fn element_bind_this_function_pair_emits_direct_bind_this() {
    // Finding C (R4): an INTRINSIC element `bind:this={get, set}` (a getter/setter
    // function-pair) is IN the DOM-bind backend's scope. Official svelte@5.56.10 accepts it and emits
    // `$.bind_this(div, <set>, <get>)` — the user-supplied get/set passed DIRECTLY (setter
    // slot FIRST, getter slot SECOND), NO synthesized `($$value) =>` / `() =>` thunk wrapper.
    //
    // RED before the fix: the `bind:this` classifier accepted ONLY an identifier target, so
    // a function-pair `bind:this` fell to the `_ => Err(refuse())` arm → the whole component
    // failed closed (the `emit` helper would panic).
    let js = emit(
        "<script>let el = $state(null);</script>\n\
         <div bind:this={() => el, (v) => el = v}></div>\n",
        "App.svelte",
    );
    // The user-supplied arrows are passed DIRECTLY (signal-rewritten), setter slot first.
    assert!(
        js.contains("$.bind_this(div, (v) => $.set(el, v, true), () => $.get(el));"),
        "element bind:this function-pair must emit the direct `$.bind_this(el, set, get)`:\n{js}"
    );
    // NEGATIVE: the function-pair form does NOT synthesize the identifier-target `($$value)
    // =>` setter thunk (that wrapper is the identifier `bind:this={el}` shape, not this one).
    assert!(
        !js.contains("$.bind_this(div, ($$value) =>"),
        "the function-pair form must NOT wrap the setter in a synthesized `($$value) =>` thunk:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must parse as JS:\n{js}"
    );
}

#[test]
fn element_bind_this_named_function_pair_emits_direct_bind_this() {
    // Finding C (R4): the NAMED getter/setter form `bind:this={getEl, setEl}` — the named
    // `function getEl`/`function setEl` declarations are admitted (the function-pair
    // name-collector now includes `bind:this`), and official emits `$.bind_this(div, setEl,
    // getEl)` (setter slot first, getter slot second, passed directly).
    let js = emit(
        "<script>\n\tlet el = $state(null);\n\tfunction getEl() { return el; }\n\
         \tfunction setEl(v) { el = v; }\n</script>\n<div bind:this={getEl, setEl}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_this(div, setEl, getEl);"),
        "the named bind:this pair must emit `$.bind_this(div, setEl, getEl)`:\n{js}"
    );
    // The named function declarations are admitted (lowered into the component body).
    assert!(
        js.contains("function getEl()") && js.contains("function setEl("),
        "the named get/set function declarations must be admitted:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must parse as JS:\n{js}"
    );
}

#[test]
fn component_bind_this_function_pair_emits_get_set_args() {
    // A COMPONENT `bind:this={get, set}` emits `$.bind_this(<call>, set, get)` with the
    // function-pair's two arrow elements as the (setter, getter) args (the official
    // `build_bind_this` sequence form).
    let js = emit_result(
        "<script>import MyComponent from './MyComponent.svelte'; let el = $state(null);</script>\n\
         <MyComponent bind:this={() => el, (v) => el = v} />\n",
    )
    .expect("a component bind:this function-pair emits a module");
    assert!(
        js.contains("$.bind_this(MyComponent("),
        "missing the $.bind_this wrapper around the component call:\n{js}"
    );
    // The function-pair get/set arrows are the (setter, getter) args, signal-rewritten.
    assert!(
        js.contains("(v) => $.set(el, v") && js.contains("() => $.get(el)"),
        "missing the function-pair (setter, getter) args:\n{js}"
    );
}

#[test]
fn bind_checked_with_non_checkbox_type_fails_closed() {
    // Official: same error for `<input type="text" bind:checked>`. A non-checkbox
    // static `type` fails closed. RED before the fix.
    assert_fail_closed(
        "<script>let c = $state(false);</script>\n<input type=\"text\" bind:checked={c} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "checked"),
    );
}

#[test]
fn bind_value_inline_function_pair_emits_helper_with_rewritten_closures() {
    // A DOM-host FUNCTION binding `bind:value={get, set}` — a 2-element sequence of
    // get/set expressions. Official passes the supplied get/set DIRECTLY to the helper
    // (NOT re-wrapped in generated lvalue thunks), rewriting any signal read/write
    // INSIDE them: `$.bind_value(input, () => $.get(v), (x) => $.set(v, x, true))`.
    // RED against the classifier that refused every sequence get/set pair.
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={() => v, (x) => v = x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(v), (x) => $.set(v, x, true))"),
        "an inline function-pair bind:value must pass the rewritten get/set directly:\n{js}"
    );
    // NEGATIVE: the supplied functions must NOT be re-wrapped as `() => (() => ...)`
    // generated lvalue thunks (the directly-passed form has no extra wrapper).
    assert!(
        !js.contains("() => () =>") && !js.contains("($$value) => () =>"),
        "a function-pair must not double-wrap the supplied get/set in lvalue thunks:\n{js}"
    );
}

#[test]
fn bind_group_function_pair_refuses_while_non_group_function_pairs_emit() {
    // F1: `bind:group` is the SOLE identifier/member-only bind. A function-pair
    // (SequenceExpression) target on `bind:group` is the official `bind_group_invalid_expression`
    // reject — `bind:group` can only bind to an Identifier or MemberExpression (verified
    // svelte@5.56.10: `<input type="radio" bind:group={() => g, (x) => g = x}>` →
    // `bind_group_invalid_expression`). RED before the fix: `bind:group` fail-OPENED, accepting
    // the function-pair as a clean FunctionPair like every other DOM bind. The exact official
    // code is asserted, not just "an error".
    let err = emit_result(
        "<script>let g = $state(\"\");</script>\n<input type=\"radio\" bind:group={() => g, (x) => g = x} value=\"a\" />\n",
    )
    .expect_err("a function-pair bind:group must refuse (identifier/member-only)");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected an OfficialReject for a bind:group function-pair, got {err:?}");
    };
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::BindGroupInvalidExpression,
        "a function-pair bind:group must reject via the BindGroupInvalidExpression rule"
    );
    assert_eq!(
        rejection.official_code, "bind_group_invalid_expression",
        "the rejection mirrors the exact official `bind_group_invalid_expression` code"
    );

    // POSITIVE CONTROLS: the SAME function-pair form on a NON-group bind (`bind:value` /
    // `bind:checked`) is official-VALID and must STILL EMIT — the identifier/member-only policy
    // is `bind:group`-only, not a broad function-pair refusal. A regression that broadened the
    // policy to every bind would RED here.
    let value_js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={() => v, (x) => v = x} />\n",
        "App.svelte",
    );
    assert!(
        value_js.contains("$.bind_value("),
        "a bind:value function-pair must still emit $.bind_value:\n{value_js}"
    );
    let checked_js = emit(
        "<script>let c = $state(false);</script>\n<input type=\"checkbox\" bind:checked={() => c, (x) => c = x} />\n",
        "App.svelte",
    );
    assert!(
        checked_js.contains("$.bind_checked("),
        "a bind:checked function-pair must still emit $.bind_checked:\n{checked_js}"
    );
}

#[test]
fn bind_value_named_function_pair_lowers_decls_and_passes_idents() {
    // A function-pair bind referencing NAMED top-level `function` declarations
    // (`function get(){...} function set(next){...} <input bind:value={get,set}>`) IS a
    // supported surface — the named functions are inside the supported function-binding
    // `bind:x={get,set}` on DOM hosts. The function declarations are ADMITTED (their names
    // are exactly the function-pair-referenced set) and LOWERED with body signal reads /
    // writes rewritten; the bind passes the function IDENTS directly. Verified against
    // svelte@5.56.10:
    //   function get() { return $.get(value); }
    //   function set(next) { $.set(value, next, true); }
    //   $.bind_value(input, get, set);
    // RED against the pre-fix tree, which refused ALL top-level `function` declarations at
    // the instance-script-item gate (only INLINE sequence pairs worked).
    let js = emit(
        "<script>let value = $state(0); function get(){ return value; } function set(next){ value = next; }</script>\n<input bind:value={get, set} />\n",
        "App.svelte",
    );
    // The function declarations are lowered with body reads/writes rewritten.
    assert!(
        js.contains("function get()") && js.contains("return $.get(value)"),
        "the named getter must be lowered with its signal read rewritten:\n{js}"
    );
    assert!(
        js.contains("function set(next)") && js.contains("$.set(value, next, true)"),
        "the named setter must be lowered with its signal write rewritten:\n{js}"
    );
    // The bind passes the function IDENTS directly (no lvalue-thunk wrap, no re-decl).
    assert!(
        js.contains("$.bind_value(input, get, set)"),
        "a named function-pair must pass the function idents directly to the helper:\n{js}"
    );
    // NEGATIVE: the function bodies must NOT leak an un-rewritten bare `value` read where
    // the rewrite belongs (the getter returns `$.get(value)`, never `return value`).
    assert!(
        !js.contains("return value;") && !js.contains("value = next;"),
        "the function bodies must be lowered, not emitted verbatim:\n{js}"
    );
}

#[test]
fn bind_value_named_function_pair_full_module_matches_official_structure() {
    // Full-module structural golden for the named-function-pair surface. Asserts the
    // load-bearing facts in source order: the state decl, BOTH lowered function
    // declarations (bodies rewritten), the `remove_input_defaults` prelude, and the
    // `$.bind_value(input, get, set)` ident-passing call. Verified against svelte@5.56.10:
    //   let value = $.state(0);
    //   function get() { return $.get(value); }
    //   function set(next) { $.set(value, next, true); }
    //   $.remove_input_defaults(input);
    //   $.bind_value(input, get, set);
    // (Cosmetic JS carrier formatting — e.g. `(){` vs `() {` brace spacing — is waived;
    // the helper choice / args / signal rewrites / source order are structural.)
    let js = emit(
        "<script>let value = $state(0); function get(){ return value; } function set(next){ value = next; }</script>\n<input bind:value={get, set} />\n",
        "App.svelte",
    );
    // Imports + template factory.
    assert!(
        js.contains("import * as $ from 'svelte/internal/client';"),
        "missing client namespace import:\n{js}"
    );
    assert!(
        js.contains("var root = $.from_html(`<input/>`);"),
        "the input skeleton must be the bare clone root:\n{js}"
    );
    // The state decl precedes the functions, which precede the DOM walk (source order).
    let state_pos = js.find("let value = $.state(0);").expect("state decl");
    let get_pos = js.find("function get()").expect("getter decl");
    let set_pos = js.find("function set(next)").expect("setter decl");
    let bind_pos = js.find("$.bind_value(input, get, set)").expect("bind call");
    assert!(
        state_pos < get_pos && get_pos < set_pos && set_pos < bind_pos,
        "items must emit in source order (state, get, set, bind):\n{js}"
    );
    // The bodies are lowered (signal read in get, signal write in set).
    assert!(
        js.contains("return $.get(value);"),
        "the getter body must rewrite the signal read:\n{js}"
    );
    assert!(
        js.contains("$.set(value, next, true);"),
        "the setter body must rewrite the signal write (with the proxy flag):\n{js}"
    );
    // The prelude clears input defaults; the bind passes the idents directly.
    assert!(
        js.contains("$.remove_input_defaults(input);"),
        "the input-defaults prelude must emit:\n{js}"
    );
    // NEGATIVE: no lvalue-thunk wrap around the function idents, no re-declared functions.
    assert!(
        !js.contains("() => get") && !js.contains("($$value) => set"),
        "the function idents must pass directly (no lvalue-thunk wrap):\n{js}"
    );
}

#[test]
fn ordinary_named_function_is_preserved() {
    // NEGATIVE control for the named-function-pair admission: a top-level `function`
    // whose name is NOT referenced by an accepted function-pair bind STAYS fail-closed at
    // the instance-script-item gate (construct `function`). The admission is gated on the
    // function-pair-referenced name set, so a plain helper that nothing binds is still
    // refused — this proves there is NO wildcard "emit any function" path. RED would be a
    // broadened function admission.
    let js = emit(
        "<script>let c = $state(0); function helper(){ return c + 1; }</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("function helper(){ return $.get(c) + 1; }"),
        "ordinary function or its reactive rewrite missing:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn bind_checked_inline_function_pair_passes_get_set_directly() {
    // The function-pair form generalizes across the DOM-host bind family: a
    // `bind:checked={get, set}` on a checkbox passes the get/set directly to
    // `$.bind_checked(input, get, set)` (here the rewritten inline arrows). RED against
    // the sequence-pair refusal.
    let js = emit(
        "<script>let c = $state(false);</script>\n<input type=\"checkbox\" bind:checked={() => c, (x) => c = x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_checked(input, () => $.get(c), (x) => $.set(c, x, true))"),
        "a function-pair bind:checked must pass the rewritten get/set directly:\n{js}"
    );
}

#[test]
fn bind_clientwidth_inline_function_pair_passes_setter_only() {
    // A SETTER-ONLY DOM-host helper (`$.bind_element_size`) with a function-pair: the
    // dimension name stays a string-literal arg and only the SET function is passed
    // directly (no getter), matching official
    // `$.bind_element_size(div, 'clientWidth', set)`. Here `set` is the rewritten arrow.
    let js = emit(
        "<script>let w = $state(0);</script>\n<div bind:clientWidth={() => w, (x) => w = x}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_element_size(div, 'clientWidth', (x) => $.set(w, x, true))"),
        "a function-pair bind:clientWidth must pass only the rewritten setter:\n{js}"
    );
    // NEGATIVE: a setter-only helper must not also emit a getter closure.
    assert!(
        !js.contains("() => $.get(w)"),
        "a setter-only function-pair must not emit a getter:\n{js}"
    );
}

#[test]
fn bind_open_inline_function_pair_passes_property_set_then_get() {
    // The generic property form with a function-pair: official emits
    // `$.bind_property('open', 'toggle', details, set, get)` — set BEFORE get, both
    // passed directly (the rewritten arrows). RED against the sequence-pair refusal.
    let js = emit(
        "<script>let o = $state(false);</script>\n<details bind:open={() => o, (x) => o = x}></details>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "$.bind_property('open', 'toggle', details, (x) => $.set(o, x, true), () => $.get(o))"
        ),
        "a function-pair bind:open must pass the property set-then-get directly:\n{js}"
    );
}

#[test]
fn bind_value_function_pair_with_ts_as_on_getter_fails_closed() {
    // A function-pair element carrying a TS `as` operator (`{get as any, set}`) is a
    // plain-`.svelte` PARSE ERROR in official svelte@5.56.10 (`Expected token }`) — the
    // template expression is parsed as plain JS, so a TS operator anywhere in either
    // element fails. Verter parses the element with TSX leniency and would silently
    // STRIP the `as any`, accepting a form official rejects; the function-pair TS gate
    // refuses it closed instead. RED before the gate (the TS was stripped + accepted).
    assert_fail_closed(
        "<script>let value = $state(0); function set(next){ value = next; }</script>\n<input bind:value={get as any, set} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_ts_as_on_setter_fails_closed() {
    // SYMMETRY: the TS operator on the SECOND element (`{get, set as any}`) is equally a
    // plain-`.svelte` PARSE ERROR (`Expected token }`). The gate checks BOTH elements,
    // not only the first. RED before the gate.
    assert_fail_closed(
        "<script>let value = $state(0); function get(){ return value; }</script>\n<input bind:value={get, set as any} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_non_null_assertion_fails_closed() {
    // A function-pair element carrying a TS non-null assertion (`{get!, set}`) is a
    // plain-`.svelte` PARSE ERROR (`Expected token }`). The non-null operator carries no
    // type operand, so the gate must catch it via the TS-expression node directly. RED
    // before the gate.
    assert_fail_closed(
        "<script>let value = $state(0); function set(next){ value = next; }</script>\n<input bind:value={get!, set} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_typed_setter_param_fails_closed() {
    // A function-pair whose setter arrow has a TYPED parameter (`(x: number) => …`) is a
    // plain-`.svelte` PARSE ERROR (`Unexpected token`) — a param type annotation is TS
    // syntax. The gate flags the typed param structurally (its `type_annotation`), not by
    // a text scan. RED before the gate (the annotation was stripped + accepted).
    assert_fail_closed(
        "<script>let value = $state(0);</script>\n<input bind:value={() => value, (x: number) => value = x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_typed_getter_param_fails_closed() {
    // SYMMETRY: a typed parameter on the GETTER-side arrow (`(x: number) => value`) is
    // equally a plain-`.svelte` PARSE ERROR (`Unexpected token`). The gate scans both
    // elements' arrow params. RED before the gate.
    assert_fail_closed(
        "<script>let value = $state(0);</script>\n<input bind:value={(x: number) => value, (y) => value = y} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_nested_ts_in_arrow_body_fails_closed() {
    // DEEP: a TS operator NESTED inside an arrow body (`(x) => value = (x as any)`) is
    // STILL a plain-`.svelte` PARSE ERROR (`Unexpected token`) — official parses the
    // whole template expression as plain JS, so a TS construct ANYWHERE in the element
    // fails, not only on the lvalue/param spine. The gate visits the full element
    // subtree, not just the top level. RED before the gate (the nested TS was stripped).
    assert_fail_closed(
        "<script>let value = $state(0);</script>\n<input bind:value={() => value, (x) => value = (x as any)} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_generic_arrow_param_fails_closed() {
    // A function-pair whose SETTER arrow carries a GENERIC type-parameter list with a
    // TRAILING comma (`<T,>(x) => …`) is a plain-`.svelte` PARSE ERROR in official
    // svelte@5.56.10 (`Unexpected token`) — a type-parameter declaration is TS syntax.
    // A CONSTRAINT-LESS `<T,>` carries NO `TSType` inside it (the param has no
    // `constraint`/`default`), so the type-`TSType` hook alone misses it; the gate must
    // flag the `TSTypeParameterDeclaration` node directly. RED before the
    // type-param-declaration override (today the empty type-param list is silently
    // stripped at TSX-lenient parse + the pair is ACCEPTED as a module).
    assert_fail_closed(
        "<script>let value = $state(0);</script>\n<input bind:value={() => value, <T,>(x) => value = x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_generic_arrow_param_on_getter_fails_closed() {
    // SYMMETRY: a generic type-parameter list on the GETTER-side arrow
    // (`<T,>() => value`) is equally a plain-`.svelte` PARSE ERROR (`Unexpected
    // token`). The gate scans BOTH elements' type-parameter declarations. RED before
    // the override.
    assert_fail_closed(
        "<script>let value = $state(0);</script>\n<input bind:value={<T,>() => value, (x) => value = x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_multi_generic_arrow_param_fails_closed() {
    // A MULTI-parameter generic list (`<T, U>(x) => …`) is a plain-`.svelte` PARSE
    // ERROR (`Unexpected token`) just like the single trailing-comma form. The
    // type-parameter-declaration node is flagged regardless of arity / constraints.
    // RED before the override.
    assert_fail_closed(
        "<script>let value = $state(0);</script>\n<input bind:value={() => value, <T, U>(x) => value = x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_optional_setter_param_fails_closed() {
    // A function-pair whose SETTER arrow has an OPTIONAL parameter (`(x?) => …`) is a
    // plain-`.svelte` PARSE ERROR in official svelte@5.56.10 (`Unexpected token`) — the
    // `?` optional marker is TS-only param syntax. OXC parses it CLEANLY under TSX
    // leniency (`optional = true`, NO recovery diagnostic), so the element reaches the
    // function-pair TS scan and would otherwise be silently accepted with the `?`
    // stripped. The scan flags the param's `optional` field structurally. RED before
    // the `visit_formal_parameter` override (today the optional marker is stripped +
    // the pair ACCEPTED).
    assert_fail_closed(
        "<script>let value = $state(0);</script>\n<input bind:value={() => value, (x?) => value = x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_optional_getter_param_fails_closed() {
    // SYMMETRY: an OPTIONAL parameter on the GETTER-side arrow (`(x?) => value`) is
    // equally a plain-`.svelte` PARSE ERROR (`Unexpected token`). The scan checks both
    // elements' arrow params. RED before the override.
    assert_fail_closed(
        "<script>let value = $state(0);</script>\n<input bind:value={(x?) => value, (x) => value = x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_function_pair_with_readonly_param_fails_closed() {
    // A function-pair whose setter arrow has a `readonly` param-property MODIFIER
    // (`(readonly x) => …`) is a plain-`.svelte` PARSE ERROR in official svelte@5.56.10
    // (`Unexpected token`) — a param modifier is TS parameter-property syntax. OXC does
    // NOT parse it cleanly: it recovers the node WITH a `'readonly' modifier cannot
    // appear on a parameter` diagnostic, so the template expression fails closed EARLY
    // via the parse-error channel (`svelte-runtime-expr-parse`) before the function-pair
    // gate. The strict official-delta scan's wildcard-free `FormalParameter` destructure
    // also flags the recovered `readonly` field directly as defense in depth (so the scan
    // stays a complete TS detector even if a future parser tolerates the modifier
    // silently). This end-to-end test pins the official contract: the whole param-modifier
    // family is REFUSED, never silently emitted with the modifier stripped.
    match emit_result(
        "<script>let value = $state(0);</script>\n<input bind:value={() => value, (readonly x) => value = x} />\n",
    ) {
        Err(ClientCompileError::Lowering(errs)) => {
            assert!(
                errs.diagnostics
                    .iter()
                    .any(|d| d.code == "svelte-runtime-expr-parse"),
                "a `readonly` param modifier must fail closed via the expr-parse channel:\n{errs:?}"
            );
        }
        Ok(js) => panic!("expected fail-closed for a `readonly` param modifier, got a module:\n{js}"),
        Err(other) => panic!("expected an expr-parse lowering error, got: {other:?}"),
    }
}

#[test]
fn bind_value_function_pair_with_default_param_stays_accepted() {
    // POSITIVE CONTROL: a DEFAULT param (`(x = 1) => …`) is plain JS — official ACCEPTS
    // it (verified svelte@5.56.10: `$.bind_value(input, () => $.get(value), (x = 1) =>
    // $.set(value, x, true))`). The new `visit_formal_parameter` override must NOT
    // over-reject it (a default is the `initializer` field, not a TS field). The pair
    // stays accepted and emits the directly-passed rewritten get/set.
    let js = emit(
        "<script>let value = $state(0);</script>\n<input bind:value={() => value, (x = 1) => value = x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(value), (x = 1) => $.set(value, x, true))"),
        "a default-param function-pair must stay accepted (plain JS), not fail closed:\n{js}"
    );
}

#[test]
fn bind_value_function_pair_with_rest_param_stays_accepted() {
    // POSITIVE CONTROL: a REST param (`(...x) => …`) is plain JS — official ACCEPTS it
    // (verified svelte@5.56.10: `$.bind_value(input, () => $.get(value), (...x) =>
    // $.set(value, x[0], true))`). The override must NOT over-reject it (rest is a
    // plain `pattern`, not a TS field).
    let js = emit(
        "<script>let value = $state(0);</script>\n<input bind:value={() => value, (...x) => value = x[0]} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(value), (...x) => $.set(value, x[0], true))"),
        "a rest-param function-pair must stay accepted (plain JS), not fail closed:\n{js}"
    );
}

#[test]
fn bind_value_function_pair_with_destructured_param_stays_accepted() {
    // POSITIVE CONTROL: a DESTRUCTURED param (`({a}) => …`) is plain JS — official
    // ACCEPTS it (verified svelte@5.56.10: `$.bind_value(input, () => $.get(value),
    // ({ a }) => $.set(value, a, true))`). The override must NOT over-reject it
    // (destructuring is a plain `pattern`, not a TS field). Verter passes the setter
    // through with its own carrier whitespace (`({a})` vs official's `({ a })`) — an
    // intra-expression cosmetic difference that conformance waives; the structural
    // contract is the directly-passed `$.set` setter in the `$.bind_value` call.
    let js = emit(
        "<script>let value = $state(0);</script>\n<input bind:value={() => value, ({a}) => value = a} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(value), ({a}) => $.set(value, a, true))"),
        "a destructured-param function-pair must stay accepted (plain JS), not fail closed:\n{js}"
    );
}

#[test]
fn bind_value_clean_inline_function_pair_stays_accepted_control() {
    // POSITIVE CONTROL pinned alongside the function-pair TS-rejection family: a CLEAN
    // inline function-pair (no TS construct in either element) MUST stay accepted and
    // emit the directly-passed rewritten get/set — the TS gate (including the new
    // type-parameter-declaration flag) must NOT over-refuse a plain pair. Verified
    // against svelte@5.56.10: `$.bind_value(input, () => $.get(v), ($$value) => $.set(v,
    // $$value))` for a bare `value = x` setter.
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={() => v, (x) => v = x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(v), (x) => $.set(v, x, true))"),
        "a clean inline function-pair bind:value must stay accepted (directly-passed get/set):\n{js}"
    );
    // NEGATIVE: the clean pair must NOT route through the fail-closed refusal (no empty
    // module / no missing helper call).
    assert!(
        js.contains("$.bind_value(input"),
        "the clean function-pair must emit the bind_value helper, not fail closed:\n{js}"
    );
}

#[test]
fn bind_value_function_pair_with_class_accessibility_field_getter_fails_closed() {
    // A function-pair GETTER carrying a class with a TS accessibility modifier
    // (`{class C { public x = 1 }, set}`) is a plain-`.svelte` PARSE ERROR in official
    // svelte@5.56.10 (`Unexpected token`) — `public`/`private`/`protected` are TS-only
    // class-member syntax. OXC's plain-JS (`mjs`) parse TOLERATES it (populating
    // `PropertyDefinition.accessibility`) WITHOUT a `TSType` node, so the pre-fix
    // enumerated scan (which only watched `TSType`/`TSNonNull`/type-param/formal-param
    // hooks) never fired — the class was accepted and the modifier silently stripped.
    // The strict official-delta scan flags `accessibility` structurally. RED against the
    // pre-fix tree (a module was emitted); now refused.
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { public x = 1 }, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_class_accessibility_field_setter_fails_closed() {
    // SYMMETRY: the TS class-member modifier on the SETTER element
    // (`{() => v, class C { private x = 1 }}`) is equally a plain-`.svelte` PARSE ERROR
    // (`Unexpected token`). The scan visits BOTH elements, so a TS construct in the
    // setter position fails closed too. RED against the pre-fix tree.
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={() => v, class C { private x = 1 }} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_class_readonly_field_fails_closed() {
    // A `readonly` class field (`{class C { readonly x = 1 }, set}`) is TS-only —
    // official REJECTS it (`Unexpected token`). OXC's `mjs` parse populates
    // `PropertyDefinition.readonly`; the strict-delta scan flags it. RED against the pre-fix tree.
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { readonly x = 1 }, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_class_optional_field_fails_closed() {
    // An OPTIONAL class field (`{class C { x? }, set}`) is TS-only — official REJECTS it
    // (`Unexpected token`). OXC's `mjs` parse populates `PropertyDefinition.optional`
    // (the `?` field marker, distinct from the JS optional-chaining `?.` operator on a
    // member expression); the strict-delta scan flags it. RED against the pre-fix tree.
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { x? }, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_class_definite_field_fails_closed() {
    // A DEFINITE-assignment class field (`{class C { x! }, set}`) is TS-only — official
    // REJECTS it (`Unexpected token`). Since oxc 0.151 a definite field without a type
    // annotation is an OXC parse error even under tsx (TS: "Declarations with definite
    // assignment assertions must also have type annotations"), so the template expression
    // fails at the `svelte-runtime-expr-parse` gate before the bind classifier. RED against
    // the pre-fix tree.
    assert_function_pair_expr_parse_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { x! }, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_class_declare_field_fails_closed() {
    // A `declare` class field (`{class C { declare x }, set}`) is TS-only — official
    // REJECTS it (`Unexpected token`). OXC's `mjs` parse populates
    // `PropertyDefinition.declare`; the strict-delta scan flags it. RED against the pre-fix tree.
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { declare x }, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_field_decorator_fails_closed() {
    // A class-FIELD decorator (`{class C { @dec x = 1 }, set}`) is not plain ECMAScript
    // the official Acorn parser accepts — official REJECTS it (`Unexpected character
    // '@'`). OXC's `mjs` parse TOLERATES the decorator (populating
    // `PropertyDefinition.decorators`); the strict-delta scan flags a non-empty
    // decorator list. RED against the pre-fix tree.
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { @dec x = 1 }, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_class_decorator_fails_closed() {
    // A CLASS decorator (`{@dec class C {}, set}`) is not plain ECMAScript official
    // accepts — official REJECTS it. OXC's `mjs` parse TOLERATES it (populating
    // `Class.decorators`); the strict-delta scan flags it. RED against the pre-fix tree.
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={@dec class C {}, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_class_implements_fails_closed() {
    // A class `implements` clause (`{class C implements I {}, set}`) is TS-only —
    // official REJECTS it (`Unexpected token`). OXC's `mjs` parse ERRORS on `implements`
    // (the parse-error gate refuses); the recovered AST also populates
    // `Class.implements`, which the strict-delta scan flags as defense in depth. RED
    // before the fix (tsx leniency stripped the clause + accepted).
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C implements I {}, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_class_override_member_fails_closed() {
    // An `override` member (`{class C { override m() {} }, set}`) is TS-only — official
    // REJECTS it (`Unexpected token`). OXC's `mjs` parse populates
    // `MethodDefinition.override`; the strict-delta scan flags it. RED against the pre-fix tree.
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { override m() {} }, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_accessor_member_fails_closed() {
    // An auto-accessor (`{class C { accessor x = 1 }, set}`) is not plain ECMAScript
    // official accepts (it is part of the TC39 decorators proposal) — official
    // REJECTS it (`Unexpected token`). OXC's `mjs` parse produces an `AccessorProperty`
    // node; the strict-delta scan flags the node's very existence (the `accessor`
    // keyword is itself non-plain-JS). RED against the pre-fix tree.
    assert_function_pair_binding_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { accessor x = 1 }, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_abstract_class_fails_closed() {
    // An `abstract` class in expression position (`{abstract class C {}, set}`) is
    // TS-only AND not even a valid class EXPRESSION — official REJECTS it (`Expected
    // token }`). OXC errors on it under tsx too, so it fails at the upstream
    // `svelte-runtime-expr-parse` gate (before the bind classifier). RED-before is moot
    // for the delta-scan here (this characterizes the official reject via the
    // parse-error channel); the load-bearing fact is that it is REFUSED, never emitted.
    assert_function_pair_expr_parse_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={abstract class C {}, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_abstract_member_fails_closed() {
    // An `abstract` member (`{class C { abstract m() {} }, set}`) is TS-only — official
    // REJECTS it (`Unexpected token`). OXC errors on `abstract` member under tsx too, so
    // it fails at the upstream expr-parse gate. REFUSED, never emitted.
    assert_function_pair_expr_parse_refused(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { abstract m() {} }, (x) => v = x} />\n",
    );
}

#[test]
fn bind_value_function_pair_with_plain_class_getter_stays_accepted() {
    // POSITIVE CONTROL: a plain `class C {}` with NO TS modifiers is plain JS — official
    // ACCEPTS it (verified svelte@5.56.10: `$.bind_value(input, class C {}, (x) =>
    // $.set(v, x, true))`). The strict-delta scan must NOT over-reject a clean class
    // (the carrier-stop is for TS-only fields, not the class construct itself). The pair
    // stays accepted; the class getter passes through the plain-JS rewrite lane
    // unchanged (no signal reads inside it).
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C {}, (x) => v = x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, class C {}, (x) => $.set(v, x, true))"),
        "a plain-class function-pair getter must stay accepted (plain JS):\n{js}"
    );
}

#[test]
fn bind_value_function_pair_with_plain_class_members_stays_accepted() {
    // POSITIVE CONTROL: a class with PLAIN (non-TS) fields + methods + static + private
    // + a static block is plain JS — official ACCEPTS it. The strict-delta scan must
    // flag NONE of these (a plain field `value`, a method, `static`, `#private`, a
    // `static {}` block carry no TS-only field). The pair stays accepted.
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={class C { x = 1; m() {} static s = 2; #p = 3; static { 1 } }, (x) => v = x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, class C") && js.contains("(x) => $.set(v, x, true))"),
        "a plain-member class function-pair must stay accepted (plain JS):\n{js}"
    );
    // NEGATIVE: the accepted class must NOT be routed through the refusal (no empty
    // module / missing helper).
    assert!(
        js.contains("$.bind_value(input"),
        "the plain-member class pair must emit the bind_value helper:\n{js}"
    );
}

#[test]
fn bind_value_function_pair_with_optional_chaining_getter_stays_accepted() {
    // POSITIVE CONTROL: optional chaining (`a?.b`) is plain JS — official ACCEPTS it
    // (verified svelte@5.56.10: `$.bind_value(input, a?.b, (x) => $.set(v, x, true))`).
    // The strict-delta scan must NOT confuse the JS optional-chaining `?.` operator
    // (a `MemberExpression.optional` field) with the TS optional-member `?` marker
    // (a `PropertyDefinition.optional` field) — only the latter is flagged.
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={a?.b, (x) => v = x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, a?.b, (x) => $.set(v, x, true))"),
        "an optional-chaining function-pair getter must stay accepted (plain JS):\n{js}"
    );
}

#[test]
fn bind_value_function_pair_with_object_and_array_literal_stays_accepted() {
    // POSITIVE CONTROL: object/array literals are plain JS — official ACCEPTS them. The
    // pair stays accepted (the strict-delta scan flags neither). Verter preserves the
    // author's intra-expression whitespace (`{a:1}` vs official's `{ a: 1 }`) — a
    // cosmetic difference conformance waives; the structural fact is the accepted pair.
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={[1, 2], (x) => v = x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, [1, 2], (x) => $.set(v, x, true))"),
        "an array-literal function-pair getter must stay accepted (plain JS):\n{js}"
    );
}

#[test]
fn bind_value_function_pair_tag_type_arg_is_not_ts_stripped() {
    // TRAP2 DISCRIMINATOR: a valid plain-JS RELATIONAL expression that LOOKS like a
    // tagged-template-with-type-arguments (``tag<string>`x` ``) must be rewritten from
    // the plain-JS (`mjs`) AST WITHOUT TS-stripping. Under TSX, OXC reinterprets
    // ``tag<string>`x` `` as a tagged template whose `<string>` is TS type arguments;
    // the TS strip then removes them, corrupting the expression to ``tag`x` `` — a
    // BEHAVIORAL change (a relational compare becomes a tagged-template call). Official
    // svelte@5.56.10 parses it as plain JS and emits the RELATIONAL form
    // (`$.bind_value(input, tag < string > `x`, …)`), keeping the `<string>` operands.
    // The plain-JS rewrite lane must reproduce that (Verter keeps the author's
    // no-whitespace bytes `tag<string>`x``), and MUST NOT emit the stripped ``tag`x` ``.
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={tag<string>`x`, (x) => v = x} />\n",
        "App.svelte",
    );
    // POSITIVE: the relational `<string>` operands survive (not stripped as type args).
    assert!(
        js.contains("tag<string>`x`"),
        "the relational `tag<string>`x`` must survive the plain-JS rewrite (no TS-strip):\n{js}"
    );
    // NEGATIVE: the type-arg-stripped tagged-template form must NOT be emitted (the
    // pre-fix tsx+strip lane produced exactly this corruption).
    assert!(
        !js.contains("tag`x`"),
        "the plain-JS rewrite lane must NOT TS-strip `tag<string>`x`` into `tag`x``:\n{js}"
    );
    // The pair is accepted and routed through the bind_value helper.
    assert!(
        js.contains("$.bind_value(input, tag<string>`x`, (x) => $.set(v, x, true))"),
        "the discriminator pair must emit bind_value with the relational getter:\n{js}"
    );
}
// ── form / value-bearing elements: allowlisted bind hosts whose special content /
//    attr models still fail closed ──────────────────────────────────────────────
//
// `<select>` / `<option>` / `<textarea>` ARE in the finite client-core element
// allowlist (`a` / `button` / `div` / `h1` / `input` / `p` / `video` / `textarea` /
// `select` / `option` / `audio` / `details`) — they were added as DOM-bind `bind:value`
// hosts. So a component using them passes the ELEMENT gate; the refusal MOVES to
// their special content / attr models (a static `value` / `selected` is the
// form-control setter family the DOM-bind backend owns via `bind:value`, NOT a static-attr
// serializer), which fail closed at the ATTR gate. `<datalist>` is NOT allowlisted,
// so it still fails closed at the ELEMENT gate
// (`svelte-runtime-unsupported-element`) on the FIRST out-of-allowlist element.

#[test]
fn select_option_static_value_attr_fails_closed_at_the_form_control_gate() {
    // `<select>`/`<option>` are in the element allowlist (DOM-bind hosts), and a
    // static `value` on an `<option>` is the VALUE-CHANNEL surface — emitted as the
    // official init-only `option.value = option.__value = 'A'` write (see
    // `select_option_static_value_attr_emits_the_official_value_channel_write`).
    // A VALUELESS `<option value>` is still the form-control deferral: official's
    // valueless arm would write the odd `__value = true`, so Verter keeps failing
    // closed through the `DynamicAttribute`/form-control channel instead.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option value>A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "value"),
    );
}

#[test]
fn option_static_selected_attr_fails_closed_at_the_form_control_gate() {
    // A static `selected` on the now-allowed `<option>` is the form-control setter
    // family (`selected` rides the form-control deferral channel alongside
    // `value`/`checked`), so it fails closed at the attr gate. RED if `selected=""`
    // were silently serialized into the cloned template.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option selected>A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "selected"),
    );
}

#[test]
fn component_function_binding_emits_bind_get_set_locals() {
    // `<Child bind:x={get, set}/>` — a component FUNCTION binding hoists `var bind_get` /
    // `var bind_set` locals and the prop getter/setter call them.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let v = $state(0);</script>\n<Child bind:x={() => v, (nv) => v = nv} />\n",
    )
    .expect("a component function binding emits a module");
    assert!(
        js.contains("var bind_get = () => $.get(v);")
            && js.contains("var bind_set = (nv) => $.set(v, nv, true);"),
        "missing the function-pair bind locals:\n{js}"
    );
    assert!(
        js.contains("get x() {return bind_get();}")
            && js.contains("set x($$value) {bind_set($$value);}"),
        "missing the prop getter/setter calling the bind locals:\n{js}"
    );
}

#[test]
fn component_multi_function_binding_emits_distinct_bind_locals() {
    // TWO function-pair binds on ONE component allocate UNIQUE locals: the first pair is
    // `bind_get`/`bind_set`, the SECOND `bind_get_1`/`bind_set_1` (the component-function
    // name uniquing). A shared pair would make BOTH props call the LAST getter/setter — the
    // codegen-correctness bug this guards against.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let v = $state(0); let w = $state(1);</script>\n<Child bind:value={() => v, (nv) => v = nv} bind:other={() => w, (nw) => w = nw} />\n",
    )
    .expect("two component function bindings emit a module");
    // The first pair drives `value`.
    assert!(
        js.contains("var bind_get = () => $.get(v);")
            && js.contains("get value() {return bind_get();}"),
        "missing the first function-pair locals wired to `value`:\n{js}"
    );
    // The SECOND pair drives `other` with the SUFFIXED `_1` names.
    assert!(
        js.contains("var bind_get_1 = () => $.get(w);")
            && js.contains("var bind_set_1 = (nw) => $.set(w, nw, true);")
            && js.contains("get other() {return bind_get_1();}")
            && js.contains("set other($$value) {bind_set_1($$value);}"),
        "missing the suffixed second function-pair locals wired to `other`:\n{js}"
    );
    // NEGATIVE: the two binds must NOT alias the same local — `other`'s setter is the
    // suffixed `bind_set_1`, NEVER the first pair's `bind_set`.
    assert!(
        !js.contains("set other($$value) {bind_set($$value);}"),
        "the two function binds must not alias the same `bind_set` local:\n{js}"
    );
}

#[test]
fn component_function_binding_renames_past_user_bind_get_collision() {
    // A USER local named `bind_get` must NOT collide with the generated function-pair bind
    // local. The names are minted through the shared scope-aware allocator (seeded with every
    // user binding), so the getter local renames to `bind_get_1` — emitting VALID JS with a
    // SINGLE `bind_get` declaration. A bare counter (the pre-fix path) mints `bind_get`
    // unconditionally, producing `let bind_get …; var bind_get …` = invalid duplicate-binding
    // JS for a valid component. `bind_set` is free, so it keeps its stem (the allocator reserves
    // each stem INDEPENDENTLY, matching official `scope.generate`).
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let bind_get = $state(0); let v = $state(1);</script>\n<Child bind:x={() => v, (nv) => v = nv} />\n",
    )
    .expect("a component function binding with a colliding user local emits a module");
    // The user `bind_get` local is declared (a `let`, distinct from the generated `var`s).
    assert!(
        js.contains("let bind_get = "),
        "missing the user `bind_get` local declaration:\n{js}"
    );
    // The generated getter local RENAMES past the user `bind_get` → `bind_get_1`.
    assert!(
        js.contains("var bind_get_1 = () => $.get(v);")
            && js.contains("get x() {return bind_get_1();}"),
        "the generated getter local must rename to `bind_get_1`:\n{js}"
    );
    // The setter local keeps the free `bind_set` stem.
    assert!(
        js.contains("var bind_set = (nv) => $.set(v, nv, true);")
            && js.contains("set x($$value) {bind_set($$value);}"),
        "the setter local must keep the free `bind_set` stem:\n{js}"
    );
    // DISCRIMINATOR: there must be NO generated `var bind_get` (that would duplicate the user
    // `let bind_get` → invalid JS). The generated local is the suffixed `var bind_get_1`.
    assert!(
        !js.contains("var bind_get = "),
        "the generated bind local must not duplicate the user `bind_get` declaration:\n{js}"
    );
}

#[test]
fn dynamic_import_expression_is_not_a_static_import_and_keeps_its_own_path() {
    // A DYNAMIC `import('./x.js')` expression is NOT a static import declaration —
    // it must not route through the static-import prelude. It rides the unchanged
    // handler-shape path (a call-bearing arrow body is that surface's own
    // fail-closed breadth today), and the refusal is NOT a script-import /
    // module-item diagnostic.
    let err = emit_result(
        "<script>let c = $state(0);</script>\n<button onclick={() => import('./x.js')}>{c}</button>\n",
    )
    .expect_err("a dynamic import() handler body is not yet an emittable handler shape");
    match err {
        ClientCompileError::Unsupported(surface) => {
            assert!(
                !matches!(
                    surface,
                    UnsupportedSvelteRuntimeSurface::ScriptImport { .. }
                        | UnsupportedSvelteRuntimeSurface::ModuleScriptItem { .. }
                ),
                "a dynamic import() must not classify as a static script import: {surface:?}"
            );
        }
        other => panic!("expected an unsupported-surface refusal, got {other:?}"),
    }
}

#[test]
fn import_redeclaration_rejects_with_official_parse_parity() {
    // An import local that COLLIDES with another top-level binding of the same
    // script is an official acorn PARSE error ("Identifier 'x' has already been
    // declared", `js_parse_error`, oracle-confirmed vs the pinned compiler) — the
    // module-scope duplicate-binding early error. Every collision family must
    // reject with the EXACT official code, never compile to a Main:
    //   - import + `let` (default and named locals),
    //   - duplicate import locals across two declarations,
    //   - import + `var` (an import binds lexically, so the `var`/`var` exemption
    //     does not apply),
    //   - import + `function` (a module-scope function declaration binds lexically).
    for (label, script) in [
        (
            "default-import-then-let",
            "import x from './m.js'; let x = $state(0);",
        ),
        (
            "named-import-then-let",
            "import { x } from './m.js'; let x = $state(0);",
        ),
        (
            "duplicate-import-locals",
            "import { x } from './a.js'; import { x } from './b.js'; let s = $state(0);",
        ),
        (
            "import-then-var",
            "import { x } from './m.js'; var x = 1; let s = $state(0);",
        ),
        (
            "import-then-function",
            "import { x } from './m.js'; function x() {} let s = $state(0);",
        ),
    ] {
        let src = format!("<script>{script}</script>\n<p>hi</p>\n");
        let err =
            emit_result(&src).expect_err("an import redeclaration must not compile to a Main");
        assert!(
            matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "js_parse_error"),
            "[{label}] expected the js_parse_error parse-parity reject, got {err:?}"
        );
    }
    // The MODULE slot rejects identically — each script body runs its own
    // duplicate-binding probe.
    let err = emit_result(
        "<script module>import { x } from './a.js'; import { x } from './b.js';</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect_err("a module-slot import redeclaration must not compile to a Main");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "js_parse_error"),
        "a module-slot duplicate import local is the same js_parse_error reject, got {err:?}"
    );
    // CONTROL (no false-reject): a DISTINCT import local + `let` name compiles, and
    // the import statement survives onto the module prelude.
    let js = emit(
        "<script>import { x } from './m.js'; let y = $state(0);</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("import { x } from './m.js';"),
        "the distinct-name control must keep compiling with its import:\n{js}"
    );
}

#[test]
fn cross_script_redeclaration_rejects_with_exact_official_codes() {
    // A binding declared in BOTH `<script>` slots — the per-body parse probe cannot
    // see it (each body parses clean alone), so it is the component-level
    // DeclarationDuplicate scan's. Official svelte@5.56.10 rejects each with an EXACT
    // code (oracle-probed): an instance value declaration over a MODULE import is
    // `declaration_duplicate_module_import`; an instance import colliding with a
    // module-slot binding is `declaration_duplicate` (the binder hoists the instance
    // import into the module scope). RED before the fix: every one of these compiled
    // to a Main (the cross-script fail-open).
    for (label, source, expected_code) in [
        (
            "module-default-import + instance-let-state",
            "<script module>import x from './m.js';</script>\n<script>let x = $state(0);</script>\n<p>hi</p>\n",
            "declaration_duplicate_module_import",
        ),
        (
            "module-named-import + instance-import",
            "<script module>import { x } from './m.js';</script>\n<script>import { x } from './n.js';\nlet s = $state(0);</script>\n<p>{s}</p>\n",
            "declaration_duplicate",
        ),
        (
            "module-named-import + instance-let-state",
            "<script module>import { x } from './m.js';</script>\n<script>let x = $state(0);</script>\n<p>hi</p>\n",
            "declaration_duplicate_module_import",
        ),
    ] {
        let err = emit_result(source)
            .expect_err("a cross-script redeclaration must not compile to a Main");
        assert!(
            matches!(
                &err,
                ClientCompileError::OfficialReject(r)
                    if r.rule == CoreOfficialValidationRule::DeclarationDuplicate
                        && r.official_code == expected_code
            ),
            "[{label}] expected the exact {expected_code} cross-script reject, got {err:?}"
        );
    }
    // SAME-body regression: a within-body duplicate stays the parse-phase
    // `js_parse_error` (the body probe), never re-attributed to the cross-script scan.
    let err = emit_result(
        "<script>import { x } from './a.js'; import { x } from './b.js'; let s = $state(0);</script>\n<p>hi</p>\n",
    )
    .expect_err("a same-body duplicate must not compile to a Main");
    assert!(
        matches!(
            &err,
            ClientCompileError::OfficialReject(r)
                if r.rule == CoreOfficialValidationRule::ScriptBodyParse
                    && r.official_code == "js_parse_error"
        ),
        "a same-body duplicate must stay the js_parse_error parse-parity reject, got {err:?}"
    );
    // CONTROLS (no over-reject; oracle-probed ACCEPTs):
    // (a) distinct names across the two slots keep compiling — the module import emits.
    let js = emit(
        "<script module>import { x } from './m.js';</script>\n<script>let y = $state(0);</script>\n<button onclick={() => y++}>{y}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("import { x } from './m.js';"),
        "the distinct-name cross-script control must keep compiling with its module import:\n{js}"
    );
    // (b) cross-script `var`/`var` and a module `var` + instance import are official-
    // ACCEPTED (a prior `var` never trips the binder's duplicate check).
    for (label, source) in [
        (
            "var + var",
            "<script module>var x = 1;</script>\n<script>var x = 2;\nlet s = $state(0);</script>\n<p>{s}</p>\n",
        ),
        (
            "module-var + instance-import",
            "<script module>var x = 1;</script>\n<script>import { x } from './b.js';\nlet s = $state(0);</script>\n<p>{s}</p>\n",
        ),
    ] {
        let js = emit_result(source).expect("an official-accepted var combination compiles");
        assert!(
            js.contains("export default function"),
            "[{label}] an official-accepted `var` combination emits a module:\n{js}"
        );
    }
}

#[test]
fn import_member_write_in_handler_is_a_plain_member_write_with_frame() {
    // The accepted write sibling: a MEMBER write rooted at an import (`x.k = c`) is
    // a plain member mutation — official accepts it (the import binding itself is
    // untouched) and the member access opens the context frame.
    let js = emit(
        "<script>import { x } from './m.js'; let c = $state(0);</script>\n<button onmouseenter={() => x.k = c} onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.event('mouseenter', button, () => x.k = $.get(c))"),
        "the member write stays plain with the signal RHS rewritten (the official \
         direct-event form):\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true)"),
        "the imported-member access must open the context frame:\n{js}"
    );
}

#[test]
fn render_non_spread_argument_still_emits_a_snippet_call() {
    // The spread refusal is NARROWLY scoped to a SPREAD argument: a NON-spread render
    // arg (`{@render row(item)}`) must STILL emit the `$.snippet(node, callee, () => …)`
    // call carrying its argument thunk, never fail closed.
    let js = emit(
        "<script>let { row, item } = $props();</script>\n{@render row(item)}\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snippet("),
        "a NON-spread render argument must still emit the $.snippet call:\n{js}"
    );
    // NEGATIVE: the argument thunk survives as the PRECISE `() => $$props.item` thunk —
    // not merely an incidental `item` substring of the `$props()` destructure — proving the
    // spread refusal did not collapse the non-spread arg path.
    assert!(
        js.contains("() => $$props.item"),
        "the non-spread render argument thunk `() => $$props.item` must survive:\n{js}"
    );
}

#[test]
fn a_member_bind_rooted_at_a_declaration_tag_derived_rune_is_accepted() {
    // A GENUINE `$derived(...)` rune reference, reached through the `{let x =
    // $derived(e)}` TEMPLATE DECLARATION TAG — `declaration_tag_lowering::lower_declaration_tag`
    // reclassifies the inert declarator via `state_prep::classify_block_rune_declarator`,
    // which sets `BindingRuntimeKind::Derived` directly; this path never passes through
    // `rune_scan.rs::classify_rune_position` (`declaration_tag_lowering.rs` pushes only the
    // call's ARGUMENT span into the template-expression list `push_expr` populates, so the
    // blanket template-expression rune scan re-parses `item`, never `$derived(item)`, and
    // the `$derived` identifier is never seen there). So unlike the top-level instance-script
    // form, this construct DOES reach the Member-bind classifier carrying a real
    // `$derived(...)` rune, not a `let:`-slot-prop stand-in.
    //
    // Oracle-verified against svelte@5.56.10, `runes: true` (`$state` in the same component
    // forces runes mode): official ACCEPTS `bind:value={doubled.x}` and emits the identical
    // `$.get(root).field` read/write shape as the `let:` slot-prop and `{@const}` cases:
    //   $.bind_value(input, () => $.get(doubled).x, ($$value) => $.get(doubled).x = $$value)
    let js = emit_result(
        "<script>let items = $state([{x:'a'}]);</script>\n{#each items as item}\n{let doubled = $derived(item)}\n<input bind:value={doubled.x}/>\n{/each}\n",
    )
    .expect("a member bind rooted at a declaration-tag $derived rune must be accepted");
    assert!(
        js.contains(
            "$.bind_value(input, () => $.get(doubled).x, ($$value) => $.get(doubled).x = $$value)"
        ),
        "a member bind on a declaration-tag $derived rune must read/write through $.get(doubled).x:\n{js}"
    );
    // POSITIVE: official also ACCEPTS a bare declaration-tag `$derived`-root identifier
    // bind (svelte@5.56.10) and emits Svelte 5's "overridable derived" shape:
    //   $.bind_value(input, () => $.get(doubled), ($$value) => $.set(doubled, $$value))
    // This is the genuine-rune `Derived` kind, distinct from the `let:` slot-prop's
    // `SlotPropDerived` kind (which stays refused here — see
    // `a_member_bind_rooted_at_a_derived_binding_is_accepted` above).
    let js = emit_result(
        "<script>let items = $state([{x:'a'}]);</script>\n{#each items as item}\n{let doubled = $derived(item)}\n<input bind:value={doubled}/>\n{/each}\n",
    )
    .expect("a bare-Identifier bind rooted at a declaration-tag $derived rune must be accepted");
    assert!(
        js.contains("$.bind_value(input, () => $.get(doubled), ($$value) => $.set(doubled, $$value))"),
        "a bare-Identifier bind on a declaration-tag $derived rune must read/write through $.get(doubled) / $.set(doubled, …):\n{js}"
    );
}

#[test]
fn svelte_body_dimension_bind_emits_bind_element_size_against_body() {
    // `<svelte:body bind:clientWidth={w}/>` → `$.bind_element_size($.document.body,
    // 'clientWidth', ($$value) => $.set(w, $$value, true))` — the ELEMENT dimension helper
    // reused with the `$.document.body` host AND the `should_proxy` flag (the element form
    // has NO proxy flag).
    let js = emit(
        "<script>let w = $state(0);</script>\n<svelte:body bind:clientWidth={w} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.bind_element_size($.document.body, 'clientWidth', ($$value) => $.set(w, $$value, true))"
        )),
        "body bind:clientWidth must emit bind_element_size against $.document.body:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_element_attrs_fold_into_attribute_effect_with_hoisted_handler() {
    // `<svelte:element this={tag} class={cls} onclick={() => n++}>` → the `on*` handler hoists
    // to a stable `var event_handler = …;` local, then the single `$.attribute_effect($$element,
    // () => ({ class: cls, onclick: event_handler }))` fold.
    let js = emit(
        "<script>let tag = $state('div');let cls = $state('a');let n = $state(0);</script>\n<svelte:element this={tag} class={cls} onclick={() => n++}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("var event_handler = () => $.update(n);")),
        "the event handler must hoist to a stable local:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.attribute_effect($$element, () => ({ class: cls, onclick: event_handler }))"
        )),
        "attrs + the hoisted handler fold into one attribute_effect:\n{js}"
    );
    // NEGATIVE: the handler is NOT inlined into the fold object.
    assert!(
        !n.contains(&nc("onclick: () => $.update(n)")),
        "the handler must be hoisted, not inlined in the fold:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_mutation_attribute_emits_getter_prop() {
    // WRITE half of official's `has_state`: an assignment/update in the `failed={…}` value
    // is a mutation ⇒ has_state ⇒ the prop emits the GETTER accessor, not the plain init —
    // for a member target (`obj.x = 1`), a bare-local target (`plain = 1`), and an update
    // (`obj.x++`). Verified against pinned svelte@5.56.10 (all three → `get failed()`).
    // The unrelated `$state` pins runes mode; the `bind:value` inputs admit the plain
    // locals `obj` / `plain` as DOM bind-target roots (the plain-let admission gate).
    for (expr, label) in [
        ("obj.x = 1", "member assignment"),
        ("plain = 1", "bare-local assignment"),
        ("obj.x++", "member update"),
    ] {
        let src = format!(
            "<script>let k = $state(0);\nlet obj = {{ x: 0 }};\nlet plain = 0;</script>\n<input bind:value={{obj.x}} />\n<input bind:value={{plain}} />\n<svelte:boundary failed={{{expr}}}><p>hi</p></svelte:boundary>\n"
        );
        let js = emit(&src, "App.svelte");
        let n = normalize_js_cosmetics(&js);
        assert!(
            n.contains(&nc(&format!("get failed() {{ return {expr}; }}"))),
            "a boundary failed={{{expr}}} ({label}) mutation is a GETTER props member:\n{js}"
        );
        assert!(
            !n.contains(&nc(&format!("failed: {expr}"))),
            "a mutation value must NOT stay a plain init ({label}):\n{js}"
        );
        assert!(
            parses_as_js(&js),
            "module must be valid JS ({label}):\n{js}"
        );
    }
}

#[test]
fn svelte_boundary_global_mutation_attribute_stays_plain_init() {
    // Over-fire guard at the boundary prop site: a GLOBAL-target write in `failed={…}` is
    // PURE ⇒ a PLAIN init (`failed: globalThis.x = 1`), NOT a `get failed()` getter.
    // Verified against pinned svelte@5.56.10
    // (`$.boundary(node, { failed: globalThis.x = 1 }, …)`).
    let js = emit(
        "<script>let k = $state(0);</script>\n<svelte:boundary failed={globalThis.x = 1}><p>hi</p></svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        !n.contains(&nc("get failed()")),
        "a global-target mutation must NOT emit a getter prop:\n{js}"
    );
    assert!(
        n.contains(&nc("failed: globalThis.x = 1")),
        "a global-target mutation stays a plain init:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn dynamic_attribute_mutation_is_stateful_only_for_binding_targets() {
    // End-to-end has_state at the dynamic-attribute call site (`RegularElement.js`
    // `has_state ? template_effect : init`). A binding-rooted write joins
    // `$.template_effect(() => $.set_attribute(div, 'data-x', …))`; a GLOBAL-target write
    // stays a one-shot bare `$.set_attribute(div, 'data-x', …)` init. Verified against
    // pinned svelte@5.56.10. The `bind:value` inputs admit `obj`/`plain` as plain-let DOM
    // bind-target roots (the plain-let admission gate); the binds themselves emit no
    // `$.template_effect`, so the wrapped-set_attribute discriminator is exact.
    let pre = "<script>let k = $state(0);\nlet obj = { x: 0 };\nlet plain = 0;</script>\n<input bind:value={obj.x} />\n<input bind:value={plain} />\n";

    for expr in ["obj.x = 1", "plain = 1", "obj.x++"] {
        let js = emit(
            &format!("{pre}<div data-x={{{expr}}}>hi</div>\n"),
            "App.svelte",
        );
        let n = normalize_js_cosmetics(&js);
        assert!(
            n.contains(&nc("$.template_effect(() => $.set_attribute(div, 'data-x'")),
            "a binding-rooted attribute mutation {expr} must wrap set_attribute in $.template_effect:\n{js}"
        );
        assert!(parses_as_js(&js), "module must be valid JS ({expr}):\n{js}");
    }

    for expr in ["globalThis.x = 1", "foo = 1", "globalThis.x++"] {
        let js = emit(
            &format!("{pre}<div data-x={{{expr}}}>hi</div>\n"),
            "App.svelte",
        );
        let n = normalize_js_cosmetics(&js);
        assert!(
            n.contains(&nc("$.set_attribute(div, 'data-x'")),
            "a global-target attribute mutation {expr} still emits set_attribute:\n{js}"
        );
        assert!(
            !n.contains(&nc("$.template_effect(() => $.set_attribute(div, 'data-x'")),
            "a global-target attribute mutation {expr} must stay a plain init (no template_effect wrap):\n{js}"
        );
        assert!(parses_as_js(&js), "module must be valid JS ({expr}):\n{js}");
    }
}

#[test]
fn props_uncalled_and_value_position_fail_closed() {
    // Uncalled/bare `$props` (not a destructure binding) and a value-position
    // `$props()` (not a declarator init) both fail closed — neither establishes the
    // props capture, so neither may silently emit.
    for (label, src) in [
        (
            "uncalled-bare",
            "<script>let all = $props;</script>\n<p>x</p>\n",
        ),
        (
            "value-position",
            "<script>let x = 0;</script>\n<p>{$props()}</p>\n",
        ),
        ("bare-statement", "<script>$props();</script>\n<p>x</p>\n"),
    ] {
        assert!(
            emit_result(src).is_err(),
            "the non-destructure `$props` form [{label}] must fail closed",
        );
    }
}

#[test]
fn non_global_css_artifact_reports_has_global_false_and_no_undemanded_map() {
    // A/B negative: css without `:global` publishes `has_global == false`,
    // and an undemanded map stays `None` (`module_result` compiles with
    // `want_source_map` off).
    let module = module_result(
        "<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button class=\"r\" onclick={() => c++}>{c}</button>\n",
    )
    .expect("a provable external style compiles");
    let css = module.css.as_ref().expect("an external css artifact");
    assert!(
        !css.has_global,
        "css without `:global` must not claim has_global"
    );
    assert_eq!(css.source_map, None, "no demand, no css map");
}

#[test]
fn client_source_map_is_on_demand_and_exact_for_rune_reads_and_writes() {
    let source = "<script>let count = $state(0);</script>\n\
<button onclick={() => count += 1}>{count}</button>\n";
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("src/components/Counter.svelte".to_string()),
        ..Default::default()
    };

    let mapped_alloc = Allocator::default();
    let mapped = compile_client(source, &parsed, &opts, &mapped_alloc, false, true)
        .expect("the supported component compiles with a map");
    let plain_alloc = Allocator::default();
    let plain = compile_client(source, &parsed, &opts, &plain_alloc, false, false)
        .expect("the supported component compiles without a map");
    assert_eq!(mapped.code, plain.code, "map demand never changes JS bytes");
    assert_eq!(plain.source_map, None, "no demand produces no JS map");

    let map_json = mapped
        .source_map
        .as_deref()
        .expect("map demand produces the client-module source map");
    let map =
        oxc_sourcemap::OwnedSourceMap::from_json_string(map_json).expect("valid source-map JSON");
    assert_eq!(map.get_file(), Some("Counter.svelte"));
    assert_eq!(map.get_sources().collect::<Vec<_>>(), ["Counter.svelte"]);
    assert_eq!(map.get_source_content(0), Some(source));

    let write_marker = "$.set(count";
    let write_start = mapped
        .code
        .find(write_marker)
        .expect("the event write is lowered structurally");
    let generated_write_count = write_start + "$.set(".len();
    let source_write_count = source.find("count += 1").expect("the authored event write");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &mapped.code,
        generated_write_count,
        source_write_count,
    );

    let effect_start = mapped
        .code
        .find("$.template_effect")
        .expect("the interpolation emits through the template effect");
    let effect_read_rel = mapped.code[effect_start..]
        .find("$.get(count)")
        .expect("the effect contains the rewritten rune read");
    let generated_read_count = effect_start + effect_read_rel + "$.get(".len();
    let source_read_count = source
        .rfind("count}")
        .expect("the authored interpolation read");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &mapped.code,
        generated_read_count,
        source_read_count,
    );

    use crate::framework_common::sourcemap_e2e_helpers as helpers;
    let (line, column) = helpers::byte_offset_to_line_col(&mapped.code, effect_start);
    let scaffolding_mapping = map.lookup_token(&helpers::build_lookup_table(&map), line, column);
    assert!(
        scaffolding_mapping.is_none_or(|token| token.get_source_id().is_none()),
        "synthesized template-effect scaffolding must not claim authored provenance"
    );
}

#[test]
fn a_state_declaration_carries_its_authored_name_provenance() {
    // The emitted `let count = $.state(0);` must map its generated `count` token
    // back to the AUTHORED `count` token, not to the statement start and not to
    // the `$.state` wrapper it is nested in.
    // The declaration is WRITTEN (the click handler), so the write-gated
    // promotion resolves it to the `$.state` signal form rather than a plain
    // `let` — the shape whose name token this test pins.
    let source = "<script>\nlet count = $state(0);\n</script>\n<button onclick={() => count += 1}>{count}</button>\n";
    let (code, map) = compile_with_map(source, "Counter.svelte");

    let decl = code
        .find("let count = $.state(")
        .expect("the `$state` declarator lowers to a `$.state` declaration");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &code,
        decl + "let ".len(),
        source
            .find("let count = $state(0)")
            .expect("the authored declaration")
            + "let ".len(),
    );
    // NEGATIVE: the `$.state(` wrapper is generated-only.
    assert_generated_offset_is_unmapped(
        &map,
        &code,
        decl + "let count = ".len(),
        "the `$.state` wrapper",
    );
}

#[test]
fn an_export_let_prop_declaration_carries_its_authored_name_provenance() {
    // The emitted `let title = $.prop($$props, 'title', …);` must map its
    // generated `title` token back to the AUTHORED `title` token in the
    // `export let` declaration.
    let source = "<script>\nexport let title = \"Untitled\";\n</script>\n<p>{title}</p>\n";
    let (code, map) = compile_with_map(source, "Panel.svelte");

    let decl = code
        .find("let title = $.prop(")
        .expect("the `export let` prop lowers to a `$.prop` declaration");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &code,
        decl + "let ".len(),
        source
            .find("export let title")
            .expect("the authored export declaration")
            + "export let ".len(),
    );
    // NEGATIVE: the `$.prop(` accessor construction is generated-only.
    assert_generated_offset_is_unmapped(
        &map,
        &code,
        decl + "let title = ".len(),
        "the `$.prop` accessor",
    );
}

#[test]
fn an_if_block_test_carries_its_authored_expression_provenance() {
    // The `{#if count > 0}` test is emitted INLINE into the `$.if` selector; its
    // rewritten `$.get(count)` read must carry the authored `count` position, so
    // the condition is not a provenance hole the way it was when the whole
    // selector was written as unmapped text.
    let source = "<script>\nlet count = $state(0);\n</script>\n<button onclick={() => count += 1}>go</button>\n{#if count > 0}<p>y</p>{:else}<p>n</p>{/if}\n";
    let (code, map) = compile_with_map(source, "Gate.svelte");

    let test = code
        .find("if ($.get(count)")
        .expect("the `{#if}` test is emitted into the `$.if` selector");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &code,
        test + "if ($.get(".len(),
        source
            .find("{#if count > 0}")
            .expect("the authored `{#if}` test")
            + "{#if ".len(),
    );
    // NEGATIVE: the `$.if(` / `$$render` selector scaffolding is generated-only.
    let selector = code
        .find("$.if(")
        .expect("the if block emits through the `$.if` selector");
    assert_generated_offset_is_unmapped(&map, &code, selector, "the `$.if` selector");
}

#[test]
fn a_call_bearing_if_test_carries_its_authored_expression_provenance() {
    // A call-bearing `{#if}` test does NOT emit inline: it hoists a
    // `var d = $.derived(() => <test>);` and the selector reads `$.get(d)`. The
    // authored expression lives in the HOISTED thunk, so that is where its
    // provenance has to land — the inline path's mapping does not cover it.
    let source = "<script>\n  let { ok } = $props();\n</script>\n{#if ok()}<p>x</p>{/if}\n";
    let (code, map) = compile_with_map(source, "Gate.svelte");

    let thunk = code
        .find("$.derived(() => ")
        .expect("a call-bearing test hoists a derived");
    let read = code[thunk..]
        .find("$$props.ok")
        .expect("the rewritten prop read is inside the derived thunk");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &code,
        thunk + read + "$$props.".len(),
        source.find("ok()}").expect("the authored test"),
    );
    // NEGATIVE: the `$.get(d)` selector read is generated-only.
    let get = code
        .find("$.get(d")
        .expect("the selector reads the derived");
    assert_generated_offset_is_unmapped(&map, &code, get, "the `$.get(d)` selector read");
}

#[test]
fn a_function_declaration_carries_its_authored_name_provenance() {
    // The generated `function onClick` must map back to the AUTHORED `onClick`
    // token. Today the generated name is emitted from an unmapped fragment, so
    // no segment lands on it.
    let source = "<script>\nlet { ontoggle } = $props();\nfunction onClick() {\n  ontoggle?.(1);\n}\n</script>\n<button onclick={onClick}>go</button>\n";
    let (code, map) = compile_with_map(source, "Toggle.svelte");
    let decl = code
        .find("function onClick")
        .expect("the instance-script function declaration is emitted");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &code,
        decl + "function ".len(),
        source
            .find("function onClick")
            .expect("the authored function declaration")
            + "function ".len(),
    );
}

#[test]
fn an_async_function_declaration_carries_its_authored_name_provenance() {
    // `async` shifts the name token off the fixed `"function "` literal offset
    // — the mapping must follow the REAL parsed offset, not a guessed prefix.
    let source = "<script>\nlet { ontoggle } = $props();\nasync function onClick() {\n  ontoggle?.(1);\n}\n</script>\n<button onclick={onClick}>go</button>\n";
    let (code, map) = compile_with_map(source, "Toggle.svelte");
    let decl = code
        .find("async function onClick")
        .expect("the instance-script function declaration is emitted");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &code,
        decl + "async function ".len(),
        source
            .find("async function onClick")
            .expect("the authored function declaration")
            + "async function ".len(),
    );
}

#[test]
fn a_generator_function_declaration_carries_its_authored_name_provenance() {
    // `function*` — same real-offset requirement as the `async` case above.
    let source = "<script>\nlet { ontoggle } = $props();\nfunction* onClick() {\n  ontoggle?.(1);\n}\n</script>\n<button onclick={onClick}>go</button>\n";
    let (code, map) = compile_with_map(source, "Toggle.svelte");
    let decl = code
        .find("function* onClick")
        .expect("the instance-script function declaration is emitted");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &code,
        decl + "function* ".len(),
        source
            .find("function* onClick")
            .expect("the authored function declaration")
            + "function* ".len(),
    );
}

#[test]
fn client_source_map_preserves_member_and_memoized_call_interpolation_tokens() {
    let source = "<script>let value = $state({ label: 'x' }); function label(v) { return v.label; }</script>\n<p>{value.label} {label(value)}</p><button onclick={() => value = { label: 'y' }}>+</button>\n";
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("Interpolation.svelte".to_string()),
        ..Default::default()
    };
    let alloc = Allocator::default();
    let module = compile_client(source, &parsed, &opts, &alloc, false, true)
        .expect("member and call interpolations compile with a map");
    let map = oxc_sourcemap::OwnedSourceMap::from_json_string(
        module.source_map.as_deref().expect("demanded JS map"),
    )
    .expect("valid JS map");

    let generated_member = module
        .code
        .find("$.get(value).label")
        .expect("member interpolation keeps its rewritten signal root")
        + "$.get(".len();
    let source_member = source
        .find("value.label}")
        .expect("authored member interpolation");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &module.code,
        generated_member,
        source_member,
    );

    let generated_call = module
        .code
        .find("label($.get(value))")
        .expect("call interpolation is represented by the dependency thunk")
        + "label($.get(".len();
    let source_call = source
        .find("label(value)}")
        .expect("authored call interpolation")
        + "label(".len();
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &module.code,
        generated_call,
        source_call,
    );
}

#[test]
fn generated_client_module_validation_fails_closed_with_a_typed_error() {
    let alloc = Allocator::default();
    let error = crate::svelte::runtime::client_compile::validate_generated_client_module(
        "export default function Broken( {",
        &alloc,
    )
    .expect_err("invalid generated JavaScript is never published");
    assert!(matches!(
        error,
        ClientCompileError::GeneratedModuleInvalid { .. }
    ));
}

#[test]
fn matcher_unprovable_template_fails_closed_on_the_selector_surface() {
    // A template construct the selector-to-template matcher cannot PROVE (a
    // `<svelte:head>` `<title>` is decomposed out of the runtime IR fragment)
    // keeps the style fail-closed on the selector surface — never a guessed
    // scope, never unscoped output.
    assert_fail_closed(
        "<svelte:head><title>t</title></svelte:head>\n<div>x</div>\n<style>div { color: red; }</style>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::StyleSelectorUnsupported { .. }
            )
        },
    );
}

#[test]
fn injected_css_mode_option_inlines_css_and_produces_no_external_artifact() {
    // A `<svelte:options css="injected">` component with a `<style>` records
    // the INJECTED css mode: the compiled module hoists `const $$css = { hash,
    // code }` and prepends `$.append_styles($$anchor, $$css)` to the component
    // body, and the external css artifact is NULL (the official
    // `inject_styles` routing).
    let module = module_result(
        "<svelte:options css=\"injected\" />\n<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button class=\"r\" onclick={() => c++}>{c}</button>\n",
    )
    .expect("an injected-mode style compiles");
    assert!(
        module.code.contains(
            "const $$css = { hash: 'svelte-n50uah', code: '.r.svelte-n50uah{color:red}' };"
        ),
        "the module hoists the $$css object:\n{}",
        module.code
    );
    assert!(
        module.code.contains("$.append_styles($$anchor, $$css);"),
        "the component body prepends the append_styles call:\n{}",
        module.code
    );
    // The static bake still runs — the two injection sites agree in injected
    // mode too.
    assert!(
        module.code.contains("<button class=\"r svelte-n50uah\">"),
        "the skeleton still bakes the scope class:\n{}",
        module.code
    );
    // NEGATIVE: injected mode produces NO external artifact.
    assert!(
        module.css.is_none(),
        "injected css must not publish an external artifact"
    );
}

#[test]
fn scoped_svelte_element_set_class_precedes_measurement_binds_and_events() {
    // ORDER: the synthesized `$.set_class` runs BEFORE the measurement bind
    // (the official init → after_update order) and BEFORE a legacy `on:`
    // registration.
    let js = emit(
        "<script>let tag = $state('div');\nlet w = $state(0);</script>\n<svelte:element this={tag} bind:clientWidth={w}>x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    let set_class_pos = n
        .find(&nc("$.set_class($$element, 0, 'svelte-n50uah');"))
        .unwrap_or_else(|| panic!("scoped bind-only <svelte:element> emits the set_class:\n{js}"));
    let bind_pos = n
        .find(&nc("$.bind_element_size($$element, 'clientWidth',"))
        .unwrap_or_else(|| panic!("the measurement bind emits:\n{js}"));
    assert!(
        set_class_pos < bind_pos,
        "set_class precedes the measurement bind (official init order):\n{js}"
    );

    let js = emit(
        "<script>let tag = $state('div');\nlet n = $state(0);</script>\n<svelte:element this={tag} on:click={() => n++}>x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    let set_class_pos = norm
        .find(&nc("$.set_class($$element, 0, 'svelte-n50uah');"))
        .unwrap_or_else(|| panic!("scoped legacy-on <svelte:element> emits the set_class:\n{js}"));
    let event_pos = norm
        .find(&nc("$.event('click', $$element,"))
        .unwrap_or_else(|| panic!("the legacy on: registration emits:\n{js}"));
    assert!(
        set_class_pos < event_pos,
        "set_class precedes the legacy on: registration:\n{js}"
    );
}

#[test]
fn injected_css_string_expression_option_runs_css_analysis() {
    // `<svelte:options css={'injected'}>` / `css={"injected"}` carry the SAME
    // static string the official compiler accepts as `css="injected"` (its
    // `get_static_value` reads a single string-literal expression), so the css
    // body must route through the scoping analysis: an invalid `:global`
    // placement refuses on the ANALYSIS surface. NEGATIVE: never the css-MODE
    // surface — that would mean the mode detection dropped the expression form
    // and the analysis never ran.
    for (label, src) in [
        (
            "single-quote",
            "<svelte:options css={'injected'} />\n<script>let c = $state(0);</script>\n<style>:global.x { color: red; }</style>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "double-quote",
            "<svelte:options css={\"injected\"} />\n<script>let c = $state(0);</script>\n<style>:global.x { color: red; }</style>\n<button onclick={() => c++}>{c}</button>\n",
        ),
    ] {
        assert_fail_closed_labeled(label, src, |s| {
            matches!(s, UnsupportedSvelteRuntimeSurface::StyleCssAnalysis { .. })
        });
    }
}

#[test]
fn injected_css_string_expression_option_inlines_css_like_the_text_form() {
    // The string-expression option forms (`css={'injected'}` / `css={"injected"}`)
    // carry the SAME static string official accepts as `css="injected"`, so a
    // clean body compiles down the SAME injected route — `$$css` hoist +
    // `$.append_styles`, no external artifact — exact parity with the Text form.
    for (label, src) in [
        (
            "single-quote",
            "<svelte:options css={'injected'} />\n<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button class=\"r\" onclick={() => c++}>{c}</button>\n",
        ),
        (
            "double-quote",
            "<svelte:options css={\"injected\"} />\n<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button class=\"r\" onclick={() => c++}>{c}</button>\n",
        ),
    ] {
        let module = module_result(src)
            .unwrap_or_else(|e| panic!("[{label}] an injected-mode style compiles: {e:?}"));
        assert!(
            module.code.contains("$.append_styles($$anchor, $$css);"),
            "[{label}] the injected prelude is emitted:\n{}",
            module.code
        );
        assert!(
            module.css.is_none(),
            "[{label}] injected css must not publish an external artifact"
        );
    }
}

#[test]
fn dynamic_css_option_value_stays_the_official_options_reject() {
    // A NON-static `css` value (`css={someVar}`) is NOT a resolved static
    // string: upstream rejects it (`svelte_options_invalid_attribute_value`),
    // and the official-reject gate keeps minting that exact code. NEGATIVE: the
    // static string-expression acceptance must never widen to a dynamic
    // expression — a dynamic value is never silently treated as injected.
    let err = emit_result(
        "<svelte:options css={someVar} />\n<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect_err("a dynamic css option value must not compile");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected the official options reject, got {err:?}");
    };
    assert_eq!(
        rejection.official_code,
        "svelte_options_invalid_attribute_value"
    );
}

#[test]
fn svelte_head_title_non_text_child_fails_closed() {
    // F4: a `<title>` containing a nested element / block / comment is the official
    // `title_invalid_content` error — fail closed rather than SILENTLY dropping it. RED
    // against the pre-fix `_ => {}` arm in `lower_title_chunks`.
    for src in [
        "<script>let n = $state(0);</script>\n<svelte:head><title><b>x</b></title></svelte:head>\n",
        "<script>let a = $state(true);</script>\n<svelte:head><title>{#if a}x{/if}</title></svelte:head>\n",
        "<script>let n = $state(0);</script>\n<svelte:head><title>hi<!--c--></title></svelte:head>\n",
    ] {
        assert_fail_closed(src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:head <title> non-text content",
                    ..
                }
            )
        });
    }
    // A pure text + interpolation title is still ACCEPTED (regression).
    assert!(emit_result(
        "<script>let n = $state(0);</script>\n<svelte:head><title>Hi {n}</title></svelte:head>\n"
    )
    .is_ok());
}

#[test]
fn svelte_window_host_legacy_on_modifiers_reach_the_direct_event() {
    // F5 (host audit): a LEGACY `on:` directive WITH modifiers on `<svelte:window>` carries the
    // modifier wrapper / capture through to the direct `$.event(...)` registration end-to-end
    // (the global-host path already routes modifiers via `render_event_registration` — this
    // locks the whole class, not just `<svelte:element>`).
    let js = emit(
        "<script>\n\tlet count = $state(0);\n</script>\n\n<svelte:window on:resize|preventDefault={() => count++} on:keydown|capture={() => count++} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.event('resize', $.window, $.preventDefault(() => $.update(count)))"
        )),
        "host on:resize|preventDefault → wrapped direct event:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.event('keydown', $.window, () => $.update(count), true)"
        )),
        "host on:keydown|capture → capture 4th arg:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_element_legacy_on_directive_emits_direct_event_with_modifiers() {
    // F5: a LEGACY `on:` directive on `<svelte:element>` emits a DIRECT `$.event('type',
    // $$element, <wrapped>[, capture][, passive])` (the official `OnDirective` → `after_update`
    // path) carrying the modifier wrapper / capture — NOT an `$.attribute_effect` fold that
    // silently dropped them. RED against the pre-fix fold.
    let js = emit(
        "<script>\n\tlet tag = $state('div');\n\tlet count = $state(0);\n</script>\n\n<svelte:element this={tag} on:click|preventDefault={() => count++} on:keydown|capture={() => count++}>hi</svelte:element>\n",
        "special/svelte_element_on_modifier.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.event('click', $$element, $.preventDefault(() => $.update(count)))"
        )),
        "legacy on:click|preventDefault → wrapped direct event:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.event('keydown', $$element, () => $.update(count), true)"
        )),
        "legacy on:keydown|capture → capture 4th arg:\n{js}"
    );
    // NEGATIVE: the legacy `on:` is NOT folded into `$.attribute_effect` (that is the modern
    // `onclick={…}` form), and the modifier is never dropped.
    assert!(
        !js.contains("attribute_effect"),
        "legacy on: is a direct event, not an attribute_effect fold:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_element_modern_onclick_stays_attribute_effect() {
    // F5 (do-not-touch guard): a MODERN `onclick={…}` on `<svelte:element>` STAYS an
    // `$.attribute_effect` fold entry (NOT a `$.event`) — only the legacy `on:` form changed.
    let js = emit(
        "<script>let tag = $state('div');let c = $state(0);</script>\n<svelte:element this={tag} onclick={() => c++}>hi</svelte:element>\n",
        "App.svelte",
    );
    assert!(
        js.contains("attribute_effect") && js.contains("onclick:"),
        "modern onclick stays in the attribute_effect fold:\n{js}"
    );
    assert!(
        !js.contains("$.event("),
        "modern onclick is NOT a direct $.event:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_legacy_on_error_with_modifier_fails_closed() {
    // A LEGACY `on:error|<modifier>` on `<svelte:boundary>` is the official
    // `svelte_boundary_invalid_attribute` reject: official's `Pw` accept-list ({onerror, failed,
    // pending}) is gated on `type === "Attribute"`, and a legacy `on:error|…` is an `OnDirective`,
    // never an `Attribute`. It fails closed on the lowering-recorded LEGACY `origin` (NOT the
    // modifier-presence heuristic), so the reject is uniform with a BARE `on:error`. RED against
    // the pre-fix classify that accepted the bare form and dropped the modifier.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<svelte:boundary on:error|preventDefault={() => c++}><p>x</p></svelte:boundary>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:boundary legacy on: directive",
                    ..
                }
            )
        },
    );
    // The MODERN `onerror={…}` (no modifiers) still emits (regression).
    assert!(emit_result("<script>let c = $state(0);</script>\n<svelte:boundary onerror={() => c++}><p>x</p></svelte:boundary>\n").is_ok());
}

#[test]
fn svelte_boundary_legacy_on_error_fails_closed() {
    // G2: a BARE LEGACY `on:error={h}` on `<svelte:boundary>` is the official
    // `svelte_boundary_invalid_attribute` reject — official gates boundary attributes on `type ===
    // "Attribute" && name ∈ {onerror, failed, pending}`, and a legacy `on:error` is an
    // `OnDirective`, never an `Attribute`. A bare `on:error` collapses to the SAME `AttrIr::Event`
    // shape as the modern `onerror` (no modifiers / capture / passive), so the lowering-recorded
    // `origin` is the ONLY faithful discriminator. RED against the pre-fix modifier-heuristic
    // classify that accepted the bare form and MIS-EMITTED it as a modern `{ onerror: h }`.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<svelte:boundary on:error={() => c++}><p>x</p></svelte:boundary>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:boundary legacy on: directive",
                    ..
                }
            )
        },
    );
    // NEGATIVE: the bare legacy form never reaches emission (no mis-emitted `onerror` props member).
    assert!(emit_result(
        "<script>let c = $state(0);</script>\n<svelte:boundary on:error={() => c++}><p>x</p></svelte:boundary>\n"
    )
    .is_err());
}

#[test]
fn svelte_window_host_groups_events_before_binds() {
    // F6: a `<svelte:window>` with INTERLEAVED events + binds emits ALL `$.event(...)` BEFORE
    // ALL `$.bind_*` (the official `visit_special_element` grouping), NOT source order. RED
    // against the pre-fix source-order emission (bind-before-event).
    let js = emit(
        "<script>\n\tlet width = $state(0);\n\tlet scroll = $state(0);\n</script>\n\n<svelte:window bind:innerWidth={width} onresize={() => scroll++} bind:scrollX={scroll} onkeydown={() => width++} />\n",
        "special/svelte_window_event_and_bind.svelte",
    );
    // Both events precede both binds in the emitted order.
    let resize = js.find("$.event('resize'").expect("resize event");
    let keydown = js.find("$.event('keydown'").expect("keydown event");
    let inner = js.find("$.bind_window_size").expect("innerWidth bind");
    let scrollx = js.find("$.bind_window_scroll").expect("scrollX bind");
    assert!(
        resize < inner && resize < scrollx && keydown < inner && keydown < scrollx,
        "host events must precede host binds (events-before-binds grouping):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn custom_element_non_empty_attribute_string_still_emits() {
    // The non-empty twin: `attribute: "foo"` KEEPS the field —
    // `{ a: { attribute: 'foo' } }` (oracle-verified against pinned
    // `svelte@5.56.10`). Locks the empty-attribute omission to the EMPTY
    // string only.
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-attr', props: { a: { attribute: \"foo\" } } }} />\n<script>\n\tlet { a } = $props();\n</script>\n\n<p>{a}</p>\n",
    )
    .expect("a customElement with a non-empty attribute string compiles");
    assert!(
        js.contains(
            "customElements.define('x-attr', $.create_custom_element(App, { a: { attribute: 'foo' } }, [], [], { mode: 'open' }));"
        ),
        "a non-empty attribute string keeps the field:\n{js}"
    );
}

#[test]
fn custom_element_host_call_in_handler_lowers_to_props_host() {
    // The FRAMED `$host()` handler form: the zero-arg call inside an active
    // customElement lowers to `$$props.$$host` (the official CallExpression
    // rewrite), the `new CustomEvent(…)` forces the component context
    // (`$.push($$props, true)` … `$.pop()`, statement form — no `$$exports`
    // without props), and `$$props` is BOUND wherever `$$props.$$host` is
    // emitted. The `$host` handler rides a DIRECT (`$.event`) surface (`onfocus`
    // — a non-delegatable type, the same isolation the fail-matrix rows use);
    // the sibling button keeps the delegated reactive surface, so the define
    // epilogue follows the `$.delegate` line.
    let js = emit_result(
        "<svelte:options customElement=\"x-host\" />\n<script>\n\tlet c = $state(0);\n</script>\n\n<button onfocus={() => $host().dispatchEvent(new CustomEvent('boop'))}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a framed $host() handler inside a customElement compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "$$props must be bound where $$props.$$host is emitted:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "missing the context push:\n{js}"
    );
    assert!(
        js.contains("$.event('focus', button, () => $$props.$$host.dispatchEvent(new CustomEvent('boop')));"),
        "the $host() call lowers to $$props.$$host inside the direct handler:\n{js}"
    );
    assert!(
        js.contains("\t$.pop();\n"),
        "a no-props CE frame closes with the statement pop:\n{js}"
    );
    assert!(
        js.contains("$.delegate(['click']);\ncustomElements.define('x-host', $.create_custom_element(App, {}, [], [], { mode: 'open' }));"),
        "the define epilogue follows the delegate line:\n{js}"
    );
    // NEGATIVE: no raw `$host` survives the rewrite (every `$host` byte-run in
    // the module is the `$$props.$$host` member, never the rune), and no
    // `$$exports` frame exists without props.
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
    assert!(
        !js.contains("$$exports"),
        "no $$exports without props:\n{js}"
    );
}

#[test]
fn render_dynamic_callee_new_expression_frames_and_binds_props() {
    // A NON-`$host` unsafe render callee, no customElement anywhere:
    // `{@render (new Date())()}` — the peeled callee contains a
    // `NewExpression`, the unconditional `needs_context` trigger. Official
    // `svelte@5.56.10` binds `$$props` and opens the frame.
    let js = emit_result(
        "<script>let __r = $state(0);</script>\n{@render (new Date())()}\n<button onclick={() => __r++}>{__r}</button>\n",
    )
    .expect("a new-expression render callee compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the new-expression render callee must bind $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the new-expression render callee opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$.pop();"),
        "the frame closes with the statement pop:\n{js}"
    );
    assert!(
        js.contains("new Date()"),
        "the callee expression survives into the snippet thunk:\n{js}"
    );
    assert!(
        js.contains("$.snippet("),
        "the dynamic render rides $.snippet:\n{js}"
    );
}

#[test]
fn non_custom_element_rest_props_do_not_exclude_the_host_key() {
    // NEGATIVE twin: a plain (non-customElement) component's rest excludes stay
    // the three-key official prefix — no `'$$host'`.
    let js = emit_result("<script>let { a, ...rest } = $props();</script>\n<p>{a}</p>\n")
        .expect("a plain $props() rest capture compiles");
    assert!(
        js.contains("var rest_excludes = new Set(['$$slots', '$$events', '$$legacy', 'a']);"),
        "a plain component keeps the three-key exclude prefix:\n{js}"
    );
    assert!(
        !js.contains("$$host"),
        "no `$$host` surfaces outside a custom element:\n{js}"
    );
}

#[test]
fn custom_element_compile_option_creates_without_define() {
    // The `customElement: true` COMPILE OPTION (no `<svelte:options>` value):
    // the component compiles as a custom element with NO registration — the bare
    // create statement, no define (there is no tag).
    let source = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let alloc = Allocator::default();
    let parsed = crate::svelte::parser::parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        custom_element: true,
        ..Default::default()
    };
    let js = compile_client(source, &parsed, &opts, &alloc, false, false)
        .expect("the customElement compile option compiles")
        .code;
    assert!(
        js.contains("$.create_custom_element(App, {}, [], [], { mode: 'open' });"),
        "the compile option emits the bare create statement:\n{js}"
    );
    assert!(
        !js.contains("customElements.define"),
        "the compile option never defines (no tag):\n{js}"
    );
}

#[test]
fn state_snapshot_in_handler_expression_rewrites_to_dollar_snapshot() {
    // INVERTED (was `state_snapshot_in_expression_fails_closed`): `$state.snapshot(x)`
    // in an expression is no longer refused as an advanced rune — the client
    // expression rewriter rewrites the callee to `$.snapshot`. Here a primitive
    // `$state` target write carries the snapshot RHS in a delegated handler.
    let js = emit(
        "<script>let c = $state(0);\nlet snap = $state(null);</script>\n<button onclick={() => { c++; snap = $state.snapshot(c); }}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snapshot($.get(c))"),
        "`$state.snapshot(c)` rewrites to `$.snapshot(<c read>)`:\n{js}"
    );
    // NEGATIVE: the advanced-rune refusal is gone; no raw `$state.snapshot` remains.
    assert!(
        !js.contains("$state.snapshot"),
        "the raw `$state.snapshot` rune member must be gone:\n{js}"
    );
}

#[test]
fn inspect_trace_dropped_in_event_arrow() {
    // `$inspect.trace();` as a STATEMENT inside an event-handler BLOCK arrow is
    // DROPPED IN PLACE (production elision): the surrounding body statements are
    // preserved (`c++` → `$.update(c)`) and inspect forces NO frame. RED against
    // the pre-elision `$inspect.<member>` fail-closed arm.
    let js = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => { $inspect.trace(); c++; }}>{c}</button>\n",
        "App.svelte",
    );
    assert!(!js.contains("inspect"), "the trace call is dropped:\n{js}");
    assert!(!js.contains("trace"), "no trace token survives:\n{js}");
    assert!(
        js.contains("$.update(c)"),
        "the rest of the handler body is preserved:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {"),
        "`$inspect.trace()` must NOT force the component frame:\n{js}"
    );
    assert!(!js.contains("$.push"), "no push frame from trace:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // CONTROL: the SAME handler WITHOUT the trace call emits the identical module
    // modulo whitespace — the drop is a TARGETED span removal, not a blanket body
    // rewrite (nothing else in the body was touched).
    let control = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => { c++; }}>{c}</button>\n",
        "App.svelte",
    );
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(
        norm(&js),
        norm(&control),
        "the dropped-trace module must equal the no-trace control modulo \
         whitespace\nDROPPED:\n{js}\nCONTROL:\n{control}"
    );
}

#[test]
fn inspect_trace_non_first_statement_rejects() {
    // `$effect(() => { c++; $inspect.trace(); })` — the trace is NOT the first
    // statement of the function body, an official HARD ERROR
    // (`inspect_trace_invalid_placement`). RED before the fix: Verter silently
    // DROPPED the statement and emitted a Main (over-acceptance + unsafe span-drop).
    assert_inspect_trace_invalid_placement(
        "<script>let c = $state(0); $effect(() => { c++; $inspect.trace(); });</script>\n<p>{c}</p>\n",
    );
    // The same non-first position in a HANDLER arrow body.
    assert_inspect_trace_invalid_placement(
        "<script>let c = $state(0);</script>\n<button onclick={() => { c++; $inspect.trace(); }}>{c}</button>\n",
    );
}

#[test]
fn inspect_in_expression_position_fails_closed() {
    // An `$inspect` reference OUTSIDE statement position is NOT part of the
    // supported elision surface — official emits broken `() => ;` for a concise
    // `() => $inspect(c)`, so it fails closed at the shared rewriter (never a raw
    // `$inspect` reference, a runtime ReferenceError). Guards the fail-open hole
    // the rune-scan relaxation would otherwise leave. The DIRECT (`$.event`) host
    // (`onfocus`) admits any inline arrow, so the body reaches the rewriter — the
    // refusal is the rewriter's. (The `onclick` writer keeps `{c}` reactive so the
    // interpolation gate does not fire first.)
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button onclick={() => c++} onfocus={() => $inspect(c)}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$inspect"),
    );
    // A concise `() => $inspect.trace()` is the official HARD ERROR
    // (`inspect_trace_invalid_placement`) — it rejects with the EXACT official code
    // (previously the generic advanced-rune refusal; the exact-code disposition is
    // the parity improvement).
    assert_inspect_trace_invalid_placement(
        "<script>let c = $state(0);</script>\n<button onclick={() => c++} onfocus={() => $inspect.trace()}>{c}</button>\n",
    );
    // The DELEGATED narrow gate (§1.2 nullary state-write arrow) refuses a
    // non-elidable `$inspect` body BEFORE the rewriter — still fail-closed, at
    // the handler-shape gate (never raw emission).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button onclick={() => $inspect(c)}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { event_type, .. } if event_type == "click"),
    );
}

#[test]
fn effect_in_function_body_is_position_exempt() {
    // INVERTED (was `bare_effect_in_function_body_fails_closed`): a well-formed
    // `$effect(fn)` call is a supported position at ANY depth — official lowers
    // it in a nested function body, so the rune scan no longer refuses it there.
    //
    // (a) A plain top-level `function f` hosting the effect still fails closed —
    // but at the instance-script-item gate (a function not referenced by a
    // function-pair bind is out of the allowlist), NOT on the `$effect` rune
    // basis. The disposition moving OFF the rune gate is the position-exemption
    // proof.
    let js = emit(
        "<script>let c=$state(0); function f(){ $effect(() => c); }</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("function f(){ $.user_effect(() => c); }"),
        "nested effect in an ordinary function did not lower:\n{js}"
    );
    assert!(!js.contains("$effect"), "raw effect rune leaked:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
    // (b) The nested-fn body Verter DOES lower — a function-pair bind function —
    // hosts the effect and EMITS the nested-function-body topology (oracle-verified:
    // `function get() { $.user_effect(...); return $.get(v); }` + the frame from
    // the nested `$effect`).
    let js = emit(
        "<script>\n\tlet v = $state('');\n\tfunction get() { $effect(() => console.log(v)); return v; }\n\tfunction set(x) { v = x; }\n</script>\n<input bind:value={get, set} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(() => console.log($.get(v)))"),
        "the nested-fn `$effect` lowers inside the function-pair body:\n{js}"
    );
    assert!(
        js.contains("return $.get(v);"),
        "the rest of the function body still rewrites:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);") && js.contains("$.pop();"),
        "the nested `$effect` forces the runes frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}
#[test]
fn non_rune_const_local_is_preserved() {
    // A plain non-rune `const` local is carried as an ordinary canonical
    // statement. The `$state` keeps the component in runes mode.
    let js = emit(
        "<script>let c = $state(0); const STEP = 2;</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const STEP = 2;"),
        "ordinary const missing:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}
// ── Root text-node region — fail closed ───────────────────────────────────────

#[test]
fn root_text_node_region_fails_closed() {
    // A root TEXT-NODE region (a bare reactive interpolation `{count}` as the
    // component root, with no wrapping element) is the official text-first
    // (`$.text()` + `$.next()`) topology — a distinct emission shape Verter does
    // not yet produce. It fails closed rather than emit INVALID JS (an
    // undeclared `text` var). RED against the pre-fix tree (which emitted
    // `$.set_text(text, …)` referencing an undeclared `text`).
    assert_fail_closed(
        "<script>let count=$state(0); function inc(){count+=1;}</script>\n{count}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::RootTextRegion { .. }),
    );
}

#[test]
fn interpolation_inside_an_element_is_not_refused_as_root_text() {
    // NEGATIVE / non-vacuity for the root-text refusal: an interpolation INSIDE an
    // element (`<p>{count}</p>`) is the supported reactive-text surface and must
    // STILL emit — the root-text refusal must target ONLY the root text-node
    // region, never a child interpolation.
    let src = "<script>let count=$state(0);</script>\n<p>{count}</p>\n<button onclick={() => count++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, $.get(count))"),
        "a child interpolation must still emit reactive text:\n{js}"
    );
}

#[test]
fn from_html_element_root_still_emits_after_root_region_refusal() {
    // NEGATIVE / non-vacuity for the broadened root-region refusal: a `from_html`
    // ELEMENT root (`<button>{count}</button>`) is the SUPPORTED clone-root path and
    // must STILL emit (the refusal targets ONLY the `$.text()` / `$.comment()` root
    // shapes whose clone frame would call `root()` on a node). The emitted module
    // keeps the real `var root = $.from_html(...)` factory + the `root()` clone call.
    let src =
        "<script>let count=$state(0);</script>\n<button onclick={() => count++}>{count}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.from_html(`<button> </button>`)"),
        "a from_html element root must still emit its factory:\n{js}"
    );
    assert!(
        js.contains("var button = root();"),
        "a from_html element root must still clone via `root()`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn from_html_fragment_root_still_emits_after_root_region_refusal() {
    // NEGATIVE / non-vacuity: a MULTI-ROOT `from_html` FRAGMENT (`<p>{count}</p>` +
    // a `<button>`) is the supported fragment clone-root path and must STILL emit —
    // the broadened root-region refusal must not touch a `from_html` fragment.
    let src = "<script>let count=$state(0);</script>\n<p>{count}</p>\n<button onclick={() => count++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("var fragment = root();"),
        "a from_html fragment root must still clone via `root()`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn two_onclick_handlers_emit_one_click_in_the_delegate_array() {
    // Two delegated `onclick` handlers → ONE `click` in the `$.delegate([...])`
    // epilogue (de-duplicated, first-seen order).
    let src = "<script>let a = $state(0); let b = $state(0);</script>\n<button onclick={() => a++}>{a}</button>\n<button onclick={() => b++}>{b}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.delegate(['click']);"),
        "single click in epilogue:\n{js}"
    );
    assert_eq!(
        js.matches("$.delegate(").count(),
        1,
        "exactly one delegate epilogue:\n{js}"
    );
    // Two delegated registrations.
    assert_eq!(
        js.matches("$.delegated('click'").count(),
        2,
        "two delegated regs:\n{js}"
    );
}

#[test]
fn bind_function_pair_value_module_matches_the_committed_jsdom_smoke_fixture() {
    // A DOM-host FUNCTION-PAIR `bind:value={() => value, (next) => value = next}` →
    // `$.bind_value(input, () => $.get(value), (next) => $.set(value, next, true))` —
    // the supplied get/set passed DIRECTLY (signal-rewritten, no synthesized thunk
    // wrapper). The reflecting `<p>{value}</p>` reads the SIGNAL, so the behavioral
    // smoke can assert the full DOM→signal→DOM round-trip (typing reaches the setter,
    // which updates the signal, which re-renders the reflection).
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet value = $state(\"\");\n</script>\n<input bind:value={() => value, (next) => value = next} />\n<p>{value}</p>\n",
        "bind_function_pair_value.client.mjs",
    );
}

#[test]
fn select_option_static_value_attr_emits_the_official_value_channel_write() {
    // A static `value="X"` on an `<option>` is the official OPTION VALUE-CHANNEL
    // (`needs_special_value_handling` in `RegularElement.js`): the attribute is
    // SKIPPED in the baked skeleton and emitted instead as an init-only
    // `option.value = option.__value = 'X'` write at the option's DOM-walk
    // position (svelte@5.56.10 `build_element_special_value_attribute`,
    // `evaluated.is_defined` → no `?? ''` for a string literal). The walk names
    // each value-carrying option (`var option = $.child(select)` …) and closes
    // with `$.reset(select)`. RED against the prior fail-closed
    // `DynamicAttribute { name: "value" }` refusal.
    let js = emit(
        "<script>let v = $state(\"a\");</script>\n<select bind:value={v}><option value=\"\">none</option><option value=\"a\">a</option><option value=\"b\">b</option></select>\n<p>{v}</p>\n",
        "App.svelte",
    );
    // The skeleton keeps the option ELEMENTS but strips the value attrs.
    assert!(
        js.contains("$.from_html(`<select><option>none</option><option>a</option><option>b</option></select>"),
        "the option value attrs must be pulled out of the cloned skeleton:\n{js}"
    );
    assert!(
        !js.contains("<option value"),
        "no option value attr may stay baked:\n{js}"
    );
    // The per-option init-only value-channel writes at the walk positions.
    assert!(
        js.contains("var option = $.child(select);\n\toption.value = option.__value = '';"),
        "the first option's value-channel write must follow its walk naming:\n{js}"
    );
    assert!(
        js.contains(
            "var option_1 = $.sibling(option);\n\toption_1.value = option_1.__value = 'a';"
        ),
        "the second option's value-channel write must follow its walk naming:\n{js}"
    );
    assert!(
        js.contains(
            "var option_2 = $.sibling(option_1);\n\toption_2.value = option_2.__value = 'b';"
        ),
        "the third option's value-channel write must follow its walk naming:\n{js}"
    );
    // The named-option walk closes the select region.
    assert!(
        js.contains("$.reset(select);"),
        "the walked select region must reset:\n{js}"
    );
    // The bind itself is untouched.
    assert!(
        js.contains("$.bind_select_value(select, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "the select bind call must stay:\n{js}"
    );
}

#[test]
fn select_option_static_value_attr_without_bind_still_emits_the_value_channel() {
    // The value-channel is TAG-KEYED in official (any `<option>` carrying a static
    // `value`), not bind-gated — a bare `<select><option value="x">` bakes the bare
    // option and writes the init-only `__value` channel exactly the same.
    let js = emit(
        "<select><option value=\"a\">A</option></select>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.from_html(`<select><option>A</option></select>`"),
        "the skeleton keeps the option without the value attr:\n{js}"
    );
    assert!(
        js.contains("option.value = option.__value = 'a';"),
        "the init-only value-channel write must emit:\n{js}"
    );
}

#[test]
fn option_value_channel_write_follows_the_elements_own_init_writes() {
    // Official `RegularElement.js` pushes `build_element_special_value_attribute`
    // LAST in the element's statement run — after its init-domain attribute writes
    // (`$.autofocus`, a `class:` directive's `$.set_class`) and after its child
    // block — so an `<option>` that carries one of those writes emits the
    // value-channel write after it, not ahead of it.
    for (source, init_write) in [
        (
            "<select><option autofocus value=\"a\">a</option></select>\n",
            "\t$.autofocus(option, true);\n",
        ),
        (
            "<script>let x = $state(true);</script>\n<select><option value=\"a\" class:hi={x}>a</option></select>\n",
            "\t$.set_class(option, 1, '', null, {}, { hi: x });\n",
        ),
    ] {
        let js = emit(source, "App.svelte");
        let init_at = js
            .find(init_write)
            .unwrap_or_else(|| panic!("the option's init write must emit:\n{js}"));
        let value_at = js
            .find("\toption.value = option.__value = 'a';\n")
            .unwrap_or_else(|| panic!("the option value-channel write must emit:\n{js}"));
        let reset_at = js
            .find("\t$.reset(select);\n")
            .unwrap_or_else(|| panic!("the walked select region must reset:\n{js}"));
        assert!(
            init_at < value_at && value_at < reset_at,
            "the value-channel write must close the option's own statement run:\n{js}"
        );
    }
}
#[test]
fn bind_value_to_call_expression_rejects_with_exact_bind_invalid_expression() {
    // F2: `bind:value={foo()}` is not a valid lvalue / 2-element pair — official svelte@5.56.10
    // rejects it with the EXACT code `bind_invalid_expression`. This is bind-target SHAPE
    // validation (the same class as `bind_group_invalid_expression` / `bind_invalid_parens`),
    // so Verter rejects it on the OFFICIAL-reject rail with the exact code — NOT the
    // `UnsupportedSvelteRuntimeSurface::Binding` channel it used before. The component is
    // runes-mode (a `$state` declarator) so the bind gate is reached.
    let err = emit_result(
        "<script>let n = $state(0); function foo() { return 1; }</script>\n<input bind:value={foo()} />\n",
    )
    .expect_err("a call-expression bind target must reject");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected an OfficialReject(BindInvalidExpression), got {err:?}");
    };
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::BindInvalidExpression,
        "a call-expression bind target must reject via the BindInvalidExpression rule"
    );
    assert_eq!(
        rejection.official_code, "bind_invalid_expression",
        "the rejection mirrors the official `bind_invalid_expression` code"
    );
}

#[test]
fn bind_target_ts_in_index_subexpression_fails_closed() {
    // A single-lvalue bind target whose computed index embeds a TS-only construct ANYWHERE in
    // a SUB-expression — a typed arrow param, a typed function-expression param, or a typed
    // local declaration inside an IIFE body — FAILS CLOSED via the structural
    // `lvalue_contains_ts` fact. The index sub-expression is otherwise valid JS, so the TSX
    // parser accepts it and the TS-strip lane would DELETE the type annotation and emit a
    // DIVERGENT setter (e.g. `arr[((x: number) => x)(0)]` -> `arr[((x) => x)(0)]`), whereas
    // official svelte@5.56.10 parses the source as plain JS and PARSE-REJECTS the TS. The scan
    // is a WHOLESALE plain-Svelte-JS-faithfulness check (any TS / non-ECMAScript node fails
    // closed), so the class is closed by construction — not an enumerated per-form arm. The
    // EXACT diagnostic-code parity stays D-26. `arr` is a declared writable root.
    for (src, stripped) in [
        // Typed arrow param inside the index callee.
        (
            "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[((x: number) => x)(0)]} />\n",
            "arr[((x) => x)(0)]",
        ),
        // Typed function-expression param inside the index callee.
        (
            "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[(function(y: number){ return y; })(0)]} />\n",
            "arr[(function(y){ return y; })(0)]",
        ),
        // Typed local declaration inside an IIFE body in the index.
        (
            "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[(() => { const k: number = 0; return k; })()]} />\n",
            "arr[(() => { const k = 0; return k; })()]",
        ),
    ] {
        let result = emit_result(src);
        assert!(
            matches!(
                &result,
                Err(ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::Binding {
                    target,
                    ..
                })) if target == "value"
            ),
            "{src} must fail closed as the `value` Binding surface, got {result:?}"
        );
        assert!(
            !matches!(&result, Ok(js) if js.contains(stripped)),
            "{src} must NOT accept-and-emit the TS-stripped index `{stripped}`: {result:?}"
        );
    }

    // PRECISION: a plain (untyped) IIFE index has NO TS node, so it STAYS ACCEPTED and is
    // emitted verbatim — the wholesale scan never over-refuses valid JS.
    let untyped = "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[(() => 0)()]} />\n";
    let js = emit(untyped, "App.svelte");
    assert!(
        js.contains("$.bind_value(input,"),
        "an untyped IIFE index bind target must stay accepted + emit the bind:\n{js}"
    );
    assert!(
        js.contains("arr[(() => 0)()]"),
        "the untyped IIFE index must emit its exact bytes `arr[(() => 0)()]`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the accepted untyped IIFE index bind must emit valid JS:\n{js}"
    );
}

#[test]
fn bare_instantiation_bind_target_stays_fail_closed() {
    // A BARE instantiation bind target — `arr[g<T>]` (instantiation INDEX) / `f<T>`
    // (instantiation ROOT), each an OXC `TSInstantiationExpression` with NO trailing call —
    // FAILS CLOSED via the structural `lvalue_contains_ts` fact. Official svelte@5.56.10 ALSO
    // rejects both in a plain `<script>` (`js_parse_error` — they do not parse as plain Svelte
    // JS), so the fail-close AGREES with official. This is the SAFETY value of the
    // instantiation arm: dropping it would classify `arr[g<T>]` as a clean Member lvalue and
    // emit a TS-stripped setter for an input official rejects — an accept-and-strip fail-open
    // (the exact class F1 closed). The EXACT diagnostic-code parity (`js_parse_error` vs the
    // structural `Binding` refusal) stays D-26. `arr` is a declared writable root, so the
    // refusal is the TS instantiation — NOT an unresolved/non-writable root.
    for src in [
        "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[g<T>]} />\n",
        "<script>let s = $state(0); let f = () => 0;</script>\n<input bind:value={f<T>} />\n",
    ] {
        let err = emit_result(src)
            .expect_err("a bare-instantiation bind target must fail closed (lvalue_contains_ts)");
        assert!(
            matches!(
                err,
                ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::Binding {
                    ref target,
                    ..
                }) if target == "value"
            ),
            "{src} must fail closed as the `value` Binding surface (official also js_parse_errors it), got {err:?}"
        );
    }
}
// ── Refuse-by-default fail-closed surfaces (the structural-refactor closures) ──
//
// The emitter consumes a NARROW `SupportedClientIr` produced by a default-deny
// classifier, so a surface that is not explicitly supported has NO emission type
// and CANNOT emit-by-default. Each test asserts the precise typed surface + owning
// vertical, and (where the prior emit-by-default emitted divergent / invalid JS)
// is RED against the pre-refactor tree.

// @ai-generated - D-35 live/static interpolation parity against pinned Svelte 5.56.10.
#[test]
fn interpolation_expression_families_lower_through_the_shared_rewriter() {
    let cases = [
        (
            "member",
            "<script>let value = $state({ label: 'x' });</script>\n<p>{value.label}</p>\n",
            "$.set_text(text,value.label)",
            false,
        ),
        (
            "optional-member",
            "<script>let value = $state({ label: 'x' });</script>\n<p>{value?.label}</p>\n",
            "$.set_text(text,value?.label)",
            false,
        ),
        (
            "call",
            "<script>let value = $state(0); function label(v) { return v; }</script>\n<p>{label(value)}</p><button onclick={() => value++}>+</button>\n",
            "$.set_text(text,$0),[()=>label($.get(value))]",
            true,
        ),
        (
            "binary",
            "<script>let value = $state(0);</script>\n<p>{value + 1}</p><button onclick={() => value++}>+</button>\n",
            "$.set_text(text,$.get(value)+1)",
            false,
        ),
        (
            "logical",
            "<script>let value = $state(0);</script>\n<p>{value && 'yes'}</p><button onclick={() => value++}>+</button>\n",
            "$.set_text(text,$.get(value)&&'yes')",
            false,
        ),
        (
            "conditional",
            "<script>let value = $state(0);</script>\n<p>{value ? 'yes' : 'no'}</p><button onclick={() => value++}>+</button>\n",
            "$.set_text(text,$.get(value)?'yes':'no')",
            false,
        ),
        (
            "template",
            "<script>let value = $state(0);</script>\n<p>{`v=${value}`}</p><button onclick={() => value++}>+</button>\n",
            "$.set_text(text,`v=${$.get(value)}`)",
            false,
        ),
        (
            "new",
            "<script>let value = $state(0); class Box { constructor(v) { this.label = v; } }</script>\n<p>{new Box(value).label}</p><button onclick={() => value++}>+</button>\n",
            "$.set_text(text,new Box($.get(value)).label)",
            false,
        ),
    ];
    for (name, source, expected, memoized) in cases {
        let js = emit(source, &format!("{name}.svelte"));
        let normalized = normalize_js_cosmetics(&js);
        assert!(
            normalized.contains(&nc(expected)),
            "{name} interpolation must use the official rewritten text update:\n{js}"
        );
        assert_eq!(
            normalized.contains("$.template_effect(($0)=>"),
            memoized,
            "only call-bearing interpolation values use the deps-array memoizer:\n{js}"
        );
        assert!(parses_as_js(&js), "{name} output must be valid JS:\n{js}");
    }
}

// @ai-generated - D-35 static interpolation topology against pinned Svelte 5.56.10.
#[test]
fn static_interpolation_uses_text_content_or_node_value_without_an_effect() {
    let sole = emit(
        "<script>const C = 5;</script>\n<p>a {C} b</p>\n",
        "sole.svelte",
    );
    let normalized = normalize_js_cosmetics(&sole);
    assert!(
        normalized.contains(&nc("$.from_html(`<p></p>`)"))
            && normalized.contains(&nc("p.textContent='a 5 b'")),
        "a sole static text run must use the element textContent topology:\n{sole}"
    );
    assert!(
        !normalized.contains("$.child(p)") && !normalized.contains("$.template_effect"),
        "a sole static text run must not create a text-node walk or effect:\n{sole}"
    );

    let sibling = emit(
        "<div><span>a</span>{1 + 2}<span>b</span></div>\n",
        "sibling.svelte",
    );
    let normalized = normalize_js_cosmetics(&sibling);
    assert!(
        normalized.contains(&nc("text.nodeValue='3'")),
        "a static run between siblings must initialize the reached text node:\n{sibling}"
    );
    assert!(
        !normalized.contains("$.template_effect"),
        "a static sibling run must not create an effect:\n{sibling}"
    );
    assert!(parses_as_js(&sole) && parses_as_js(&sibling));
}

// @ai-generated - D-35 folded/live run composition and block-alias rewriting.
#[test]
fn interpolation_folding_composes_with_live_runs_and_block_aliases() {
    let mixed = emit(
        "<script>let value = $state(0);</script>\n<p>sum {1 + 2}: {value}</p><button onclick={() => value++}>+</button>\n",
        "mixed.svelte",
    );
    let normalized = normalize_js_cosmetics(&mixed);
    assert!(
        normalized.contains(&nc("$.set_text(text,`sum 3: ${$.get(value)??''}`)")),
        "the static chunk must fold into the live text run without a second update:\n{mixed}"
    );
    assert_eq!(
        normalized.matches("$.set_text(text,").count(),
        1,
        "one DOM text run must produce exactly one update:\n{mixed}"
    );

    let each = emit(
        "<script>let items = $state([{ label: 'x' }]);</script>\n{#each items as item}<p>got {item.label}</p>{/each}\n",
        "each.svelte",
    );
    let normalized = normalize_js_cosmetics(&each);
    assert!(
        normalized.contains(&nc("$.set_text(text,`got ${$.get(item).label??''}`)")),
        "an each alias member must preserve the signal read before the member access:\n{each}"
    );
    assert!(
        !normalized.contains("$.get(item.label)"),
        "the member itself is never treated as the signal cell:\n{each}"
    );

    let await_block = emit(
        "<script>let promise = $state(Promise.resolve({ label: 'x' }));</script>\n{#await promise then value}<p>got {value.label}</p>{/await}\n",
        "await.svelte",
    );
    let normalized = normalize_js_cosmetics(&await_block);
    assert!(
        normalized.contains(&nc("$.set_text(text,`got ${$.get(value).label??''}`)")),
        "an await alias member must preserve the alias signal read:\n{await_block}"
    );
    assert!(parses_as_js(&mixed) && parses_as_js(&each) && parses_as_js(&await_block));
}

#[test]
fn reactive_state_interpolation_still_emits() {
    // NEGATIVE: a genuinely reactive `{n}` (n IS reassigned) still emits the
    // reactive-text op — the non-reactive fail-closed must not regress the
    // supported reactive surface.
    let js = emit(
        "<script>let n = $state(0);</script>\n<button onclick={() => n++}>{n}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.template_effect(() => $.set_text(text, $.get(n)))"),
        "a reactive interpolation must still emit the reactive-text op:\n{js}"
    );
}

#[test]
fn instance_export_function_fails_closed() {
    // An instance-script `export function` also fails closed under the
    // `$$exports` component-export identity (`ComponentExportBinding`
    // construct `function`).
    assert_fail_closed(
        "<script>let n = $state(0); export function helper() { return 1; }</script>\n<button onclick={() => n++}>{n}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentExportBinding { construct, .. } if *construct == "function"),
    );
}

#[test]
fn instance_top_level_function_preserves_precise_body_refusal() {
    // Ordinary functions are admitted, but a destructuring write inside the body
    // retains its precise typed refusal rather than leaking through the carrier.
    assert_fail_closed(
        "<script>let count = $state(0); function f(obj) { ({ count } = obj); }</script>\n<button onclick={() => count++}>{count}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn ts_wrapped_update_target_in_handler_fails_closed() {
    // An onclick arrow whose body is a TS-wrapped update (`count!++`) is NOT a clean
    // `$state` assignment / update — the update target is a TS-non-null wrapper, not a
    // bare identifier, so the handler-shape gate refuses it. Only a clean
    // `$state` write body is the supported §1.2-class handler.
    assert_fail_closed(
        "<script>let count = $state(0);</script>\n<button onclick={() => { count!++; }}>{count}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. }),
    );
}

#[test]
fn textarea_interpolation_content_fails_closed_at_the_special_content_model_gate() {
    // `<textarea>` IS an allowed DOM-bind `bind:value` host, so it PASSES the element
    // allowlist gate; the refusal is the SPECIAL CONTENT-MODEL gate, NOT the element
    // allowlist. A `<textarea>` with INTERPOLATION content (`<textarea>{c}</textarea>`)
    // is the official `textarea.value` / `$.template_effect` reactive-content surface
    // the DOM-bind backend does NOT emit — so it fails closed on the textarea content model as
    // `Element { tag: "textarea" }`, exactly like the `<option>{c}</option>` case
    // below. RED if Verter silently emitted the divergent reactive-content module.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea>{c}</textarea><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "textarea"),
    );
}

#[test]
fn option_with_interpolation_content_fails_closed_at_the_special_content_gate() {
    // `<select>`/`<option>` are now ALLOWED DOM-bind hosts, but an `<option>` with an
    // INTERPOLATION child (`<option>{c}</option>`) is the official `option.__value` /
    // `option_value` reactive-tracking content surface the DOM-bind backend does NOT emit — so it fails
    // closed at the special-content gate as `Element { tag: "option" }` (the option's
    // content model, NOT a static-option select host). RED if Verter silently emitted
    // the divergent `option.__value` tracking module.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option>{c}</option></select><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "option"),
    );
    // A nested element child inside `<option>` is likewise not the static-option
    // interior the DOM-bind backend supports — it fails closed on the option content model.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option><b>{c}</b></option></select><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "option"),
    );
}

#[test]
fn muted_dynamic_on_video_emits_property_write() {
    // `muted={v}` on `<video>` → `video.muted = $.get(v)` (special-cased property).
    let src = "<script>let v = $state(false);</script>\n<video onclick={() => v = !v} muted={v}></video>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.template_effect(() => video.muted = $.get(v))")),
        "`muted` must be a property write:\n{js}"
    );
    assert!(
        !n.contains(&nc("$.set_attribute(video, 'muted'")),
        "`muted` must NOT use set_attribute:\n{js}"
    );
}

#[test]
fn class_expression_wraps_in_clsx_and_set_class() {
    // `class={c}` → `$.set_class(button, 1, $.clsx($.get(c)))`.
    let src = "<script>let c = $state('a');</script>\n<button onclick={() => c += '!'} class={c}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.template_effect(() => $.set_class(button, 1, $.clsx($.get(c))))"
        )),
        "`class={{c}}` must be set_class with clsx:\n{js}"
    );
    // NEGATIVE: class is NOT a baked static attr here.
    assert!(
        !n.contains(&nc("$.set_attribute(button, 'class'")),
        "a dynamic class must NOT use set_attribute:\n{js}"
    );
}

#[test]
fn style_expression_emits_set_style() {
    // `style={s}` → `$.set_style(button, $.get(s))`.
    let src = "<script>let s = $state('color:red');</script>\n<button onclick={() => s = 'color:blue'} style={s}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.template_effect(() => $.set_style(button, $.get(s)))"
        )),
        "`style={{s}}` must be set_style:\n{js}"
    );
    assert!(
        !n.contains(&nc("$.set_attribute(button, 'style'")),
        "a dynamic style must NOT use set_attribute:\n{js}"
    );
}

#[test]
fn dynamic_muted_on_non_media_element_emits_property_write() {
    // `muted` is a DOM property on ANY element — official `is_dom_property('muted')`
    // is element-agnostic (`muted` ∈ `DOM_BOOLEAN_ATTRIBUTES` → `DOM_PROPERTIES`, no
    // host check) — so `<div muted={v}>` emits `div.muted = $.get(v)` exactly like a
    // `<video>` host (NOT a refusal, NOT a `$.set_attribute`).
    let src =
        "<script>let v = $state(false);</script>\n<div onclick={() => v = !v} muted={v}></div>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.template_effect(() => div.muted = $.get(v))")),
        "`muted` on a `<div>` must be a property write:\n{js}"
    );
    assert!(
        !n.contains(&nc("$.set_attribute(div, 'muted'")),
        "`muted` must NOT use set_attribute:\n{js}"
    );
}

// ─── Invalid attribute name → official reject ───
// Official rejects a plain attribute on an intrinsic element (or `<svelte:element>`) whose
// name starts with a digit / `-` / `.` or contains an operator char — `attribute_invalid_name`.

#[test]
fn invalid_attribute_name_rejects_on_a_plain_element() {
    // `<div 1foo="x">` REJECTS with `attribute_invalid_name` — a name starting with a digit.
    let err = emit_result("<script>let c = $state(0);</script>\n<div 1foo=\"x\"><button onclick={() => c++}>{c}</button></div>\n")
        .expect_err("an invalid attribute name must fail closed");
    match err {
        ClientCompileError::OfficialReject(rej) => assert_eq!(
            rej.rule,
            CoreOfficialValidationRule::AttributeInvalidName,
            "a digit-initial attribute name must reject as AttributeInvalidName:\n{rej:?}"
        ),
        other => panic!("expected an OfficialReject(AttributeInvalidName), got {other:?}"),
    }
}

// ── Function-pair component-bind locals must rename past EVERY TEMPLATE-SCOPE binding
//    that can share the emitted closure/body scope (not just top-level script locals). The
//    `bind_get`/`bind_set` stems are minted through the shared scope-aware allocator seeded
//    with the COMPLETE binding-name universe — `declared_roots` (script) ∪ the analysis
//    binding table (template scopes) ∪ free template references — so a generated `var
//    bind_get` never duplicates a lexical local (invalid JS) nor clobbers a callback param.
//    Each pre-fix tree emitted a bare `var bind_get` colliding with the template binding. ──

#[test]
fn component_function_bind_renames_past_slot_let_local_collision() {
    // A `<Child let:bind_get>` slot prop becomes `const bind_get = $.derived(() =>
    // $$slotProps.bind_get)` AT THE TOP of the default-slot callback; a function-pair bind on
    // a component NESTED in that slot (`<Grand bind:x={get, set}>`) emits `var bind_get` in
    // the SAME callback scope. A lexical `const` + a `var` of the same name in one scope is
    // INVALID JS — so the generated getter must rename to `bind_get_1`. The slot-let local has
    // NO free-reference row (nothing reads it), so only the binding-table seed catches it.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; import Grand from './Grand.svelte'; let v = $state(0);</script>\n<Child let:bind_get><Grand bind:x={() => v, (nv) => v = nv} /></Child>\n",
    )
    .expect("a slot let: with a nested function-pair bind emits a module");
    // The slot-let derived (the user binding) is preserved unchanged.
    assert!(
        js.contains("const bind_get = $.derived(() => $$slotProps.bind_get)"),
        "the slot-let derived local must be preserved:\n{js}"
    );
    // The generated getter RENAMES past the slot-let local → `bind_get_1`; the free `bind_set`
    // keeps its stem.
    assert!(
        js.contains("var bind_get_1 = () => $.get(v)")
            && js.contains("get x() {return bind_get_1();}")
            && js.contains("var bind_set = (nv) => $.set(v, nv, true)"),
        "the generated getter must rename to `bind_get_1`, the setter keep `bind_set`:\n{js}"
    );
    // DISCRIMINATOR: there must be NO generated `var bind_get` (that would duplicate the
    // lexical slot-let `const bind_get` → invalid JS). `var bind_get_1 = ` does not match.
    assert!(
        !js.contains("var bind_get = "),
        "the generated bind local must not duplicate the slot-let declaration:\n{js}"
    );
}

#[test]
fn component_function_bind_renames_past_const_decl_tag_local_collision() {
    // A `{const bind_get = v}` region-root declaration tag emits a lexical `const bind_get =
    // $.get(v)` in the component-fn body; a function-pair bind in the same scope emits `var
    // bind_get` → a `const` + `var` duplicate (invalid JS). The decl-tag local must reserve the
    // stem so the generated getter renames to `bind_get_1`.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let v = $state(0);</script>\n{const bind_get = v}<Child bind:x={() => v, (nv) => v = nv} />\n",
    )
    .expect("a {const} decl tag with a function-pair bind emits a module");
    assert!(
        js.contains("const bind_get = $.get(v)"),
        "the {{const}} decl-tag local must be preserved:\n{js}"
    );
    assert!(
        js.contains("var bind_get_1 = () => $.get(v)")
            && js.contains("get x() {return bind_get_1();}"),
        "the generated getter must rename to `bind_get_1`:\n{js}"
    );
    assert!(
        !js.contains("var bind_get = "),
        "the generated bind local must not duplicate the {{const}} decl-tag declaration:\n{js}"
    );
}

#[test]
fn component_function_bind_renames_past_let_decl_tag_local_collision() {
    // The `{let bind_get = v}` declaration-tag variant of the decl-tag collision: a lexical
    // `let bind_get = $.get(v)` + a function-pair `var bind_get` is the same invalid-JS
    // duplicate, so the generated getter renames to `bind_get_1`.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let v = $state(0);</script>\n{let bind_get = v}<Child bind:x={() => v, (nv) => v = nv} />\n",
    )
    .expect("a {let} decl tag with a function-pair bind emits a module");
    assert!(
        js.contains("let bind_get = $.get(v)"),
        "the {{let}} decl-tag local must be preserved:\n{js}"
    );
    assert!(
        js.contains("var bind_get_1 = () => $.get(v)")
            && js.contains("get x() {return bind_get_1();}"),
        "the generated getter must rename to `bind_get_1`:\n{js}"
    );
    assert!(
        !js.contains("var bind_get = "),
        "the generated bind local must not duplicate the {{let}} decl-tag declaration:\n{js}"
    );
}

#[test]
fn component_function_bind_renames_past_snippet_param_collision() {
    // A `{#snippet s(bind_get)}` PARAMETER is the snippet arrow's first declared local
    // (`($$anchor, bind_get = $.noop) => …`); a function-pair bind in the snippet body emits
    // `var bind_get`, which CLOBBERS the param (the prop getter would call the reassigned var,
    // not the snippet arg — a correctness bug official `scope.generate` avoids by renaming). The
    // snippet is rendered (`{@render s(v)}`) so the body reaches emit. The generated getter must
    // rename to `bind_get_1`, leaving the param intact.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let v = $state(0);</script>\n{#snippet s(bind_get)}<Child bind:x={() => v, (nv) => v = nv} />{/snippet}\n{@render s(v)}\n",
    )
    .expect("a snippet with a param-colliding function-pair bind emits a module");
    // The snippet param is preserved as the arrow's first declared local.
    assert!(
        js.contains("($$anchor, bind_get = $.noop) =>"),
        "the snippet param `bind_get` must be preserved:\n{js}"
    );
    assert!(
        js.contains("var bind_get_1 = () => $.get(v)")
            && js.contains("get x() {return bind_get_1();}"),
        "the generated getter must rename to `bind_get_1`:\n{js}"
    );
    // DISCRIMINATOR: no `var bind_get` reassigning the snippet param.
    assert!(
        !js.contains("var bind_get = "),
        "the generated bind local must not clobber the snippet param:\n{js}"
    );
}

#[test]
fn component_function_bind_renames_past_each_item_binding_collision() {
    // COMPREHENSIVENESS (a binding kind BEYOND the named slot-let / decl-tag / snippet-param
    // cases): an `{#each items as bind_get}` ITEM binding is the each callback's param
    // (`($$anchor, bind_get) => …`); a function-pair bind in the each body emits `var bind_get`,
    // clobbering the item param. Because the seed is the COMPLETE binding table — not a patch
    // for the three named kinds — the each-item is reserved too, and the generated getter
    // renames to `bind_get_1`. This proves the seed forecloses the WHOLE collision class.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { items } = $props(); let v = $state(0);</script>\n{#each items as bind_get}<Child bind:x={() => v, (nv) => v = nv} />{/each}\n",
    )
    .expect("an each-item-colliding function-pair bind emits a module");
    // The each-item param is preserved.
    assert!(
        js.contains("($$anchor, bind_get) =>"),
        "the each-item param `bind_get` must be preserved:\n{js}"
    );
    assert!(
        js.contains("var bind_get_1 = () => $.get(v)")
            && js.contains("get x() {return bind_get_1();}"),
        "the generated getter must rename to `bind_get_1`:\n{js}"
    );
    // DISCRIMINATOR: no `var bind_get` clobbering the each-item param.
    assert!(
        !js.contains("var bind_get = "),
        "the generated bind local must not clobber the each-item binding:\n{js}"
    );
}

// ─── Regular-element / `<svelte:fragment>` named-slot lowering + slot-attribute gate ───

#[test]
fn named_slot_on_regular_element_emits_dollar_slots_entry() {
    // A STATIC `slot="foo-bar"` on an allowlisted regular element that is a DIRECT
    // component child is the official NAMED-SLOT form: the element becomes the
    // `$$slots: { 'foo-bar': ($$anchor, $$slotProps) => {…} }` callback region and the
    // `slot` attribute BAKES into the cloned skeleton (`<span slot="foo-bar"> </span>`),
    // exactly the pinned svelte@5.56.10 output.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"foo-bar\">{x}</span></Child>\n",
    )
    .expect("a static named-slot span emits a module");
    assert!(
        js.contains("$$slots: {'foo-bar': ($$anchor, $$slotProps) =>"),
        "missing the quoted named-slot callback entry:\n{js}"
    );
    assert!(
        js.contains("$.from_html(`<span slot=\"foo-bar\"> </span>`)"),
        "the slot attribute must bake into the cloned skeleton:\n{js}"
    );
    // NEGATIVE: a NAMED-slot region body has NO leading `$.next()` (official emits the
    // cursor advance for default-children / snippet / each callbacks, NOT named slots).
    assert!(
        !js.contains("$.next()"),
        "a named-slot region must not emit a leading $.next():\n{js}"
    );
    // NEGATIVE: the named content must not leak into a `children:` default region.
    assert!(
        !js.contains("children:"),
        "named-slot content must not produce a default children prop:\n{js}"
    );
}

// ─── Component / `<svelte:*>`-special `slot=` disposition (official three-class rule) ───
//
// Official `svelte@5.56.10` (`validate_slot_attribute`, `is_component = true` for the
// Component / SvelteComponent / SvelteSelf hosts):
// - A STATIC `slot` on a DIRECT component-family child routes the filler into the
//   parent's `$$slots.NAME` AND keeps the `slot` prop on the child call.
// - A `slot` (static OR dynamic/mixed) on a NON-direct component-family host is an
//   ordinary PLAIN PROP (`{ slot: 'x' }` / a getter).
// - A dynamic/mixed `slot` on a DIRECT child is the `slot_attribute_invalid` reject.

#[test]
fn slot_on_direct_component_child_routes_to_named_slot_and_keeps_prop() {
    // `<Child><Inner slot="foo"/></Child>` — official routes the component filler into
    // `$$slots.foo` AND emits the `slot` prop on the inner call:
    //   Child($$anchor, { $$slots: { foo: ($$anchor, $$slotProps) => {
    //     Inner($$anchor, { slot: 'foo' }); } } });
    let js = emit_result(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Child><Inner slot=\"foo\"/></Child>\n",
    )
    .expect("a slot=-bearing direct component child emits a module");
    assert!(
        js.contains("$$slots: {foo: ($$anchor, $$slotProps) =>"),
        "missing the $$slots.foo filler callback:\n{js}"
    );
    assert!(
        js.contains("Inner($$anchor, {slot: 'foo'})"),
        "the inner call must keep the slot prop:\n{js}"
    );
    // NEGATIVE: the filler is NOT default-children content.
    assert!(
        !js.contains("children:"),
        "a named component filler must not leak into the children prop:\n{js}"
    );
}

#[test]
fn slot_on_direct_component_child_keeps_sibling_props_in_source_order() {
    // `<Inner slot="foo" label="L" value={v}/>` — official emits the props object in
    // SOURCE order with `slot` a plain member among them:
    //   Inner($$anchor, { slot: 'foo', label: 'L', get value() {…} });
    let js = emit_result(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { v } = $props();</script>\n<Child><Inner slot=\"foo\" label=\"L\" value={v}/></Child>\n",
    )
    .expect("a slot filler with sibling props emits a module");
    assert!(
        js.contains("Inner($$anchor, {slot: 'foo', label: 'L', get value() {return $$props.v;}})"),
        "the slot prop must ride the ordinary props object in source order:\n{js}"
    );
}

#[test]
fn slot_on_direct_svelte_component_child_routes_to_named_slot_and_keeps_prop() {
    // `<Child><svelte:component this={C} slot="foo"/></Child>` — the dynamic-component
    // filler: `$$slots.foo` wraps the `$.component(node, () => C, ($$anchor, $$component)
    // => { $$component($$anchor, { slot: 'foo' }); })` call.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { C } = $props();</script>\n<Child><svelte:component this={C} slot=\"foo\"/></Child>\n",
    )
    .expect("a slot=-bearing direct svelte:component child emits a module");
    assert!(
        js.contains("$$slots: {foo: ($$anchor, $$slotProps) =>"),
        "missing the $$slots.foo filler callback:\n{js}"
    );
    assert!(
        js.contains("$$component($$anchor, {slot: 'foo'})"),
        "the dynamic-component call must keep the slot prop:\n{js}"
    );
    // The callback's comment-anchor frame is `fragment_1`, NOT `fragment`: official
    // RESERVES the `fragment` name for the standalone ROOT region (its
    // `Fragment.js` runs `scope.generate('fragment')` before the standalone branch),
    // so the nested frame mints the bumped suffix (pinned by the
    // `components/slot_filler_svelte_component_child` oracle golden).
    assert!(
        js.contains("var fragment_1 = $.comment();"),
        "the nested filler frame must mint the reserved-bumped fragment_1:\n{js}"
    );
    assert!(
        !js.contains("var fragment = $.comment();"),
        "the standalone root's reserved `fragment` name must not be reused:\n{js}"
    );
}

#[test]
fn slot_on_direct_svelte_self_child_routes_to_named_slot_and_keeps_prop() {
    // `<Child><svelte:self slot="foo"/></Child>` — a direct component child IS a slot
    // passed to a component (a valid `<svelte:self>` placement officially), and the
    // recursive self-call keeps the `slot` prop inside the `$$slots.foo` callback.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><svelte:self slot=\"foo\"/></Child>\n",
    )
    .expect("a slot=-bearing direct svelte:self child emits a module");
    assert!(
        js.contains("$$slots: {foo: ($$anchor, $$slotProps) =>"),
        "missing the $$slots.foo filler callback:\n{js}"
    );
    assert!(
        js.contains("App(node, {slot: 'foo'})"),
        "the recursive self-call must keep the slot prop on the call itself:\n{js}"
    );
}

#[test]
fn slot_on_direct_svelte_element_child_folds_slot_and_routes_to_named_slot() {
    // `<Child><svelte:element this="div" slot="foo"/></Child>` — the dynamic-element
    // filler: `$$slots.foo` wraps the `$.element(…)` call and the `slot` attribute
    // FOLDS into the runtime `$.attribute_effect($$element, () => ({ slot: 'foo' }))`
    // (never a component prop — the element has no props object).
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><svelte:element this=\"div\" slot=\"foo\"/></Child>\n",
    )
    .expect("a slot=-bearing direct svelte:element child emits a module");
    assert!(
        js.contains("$$slots: {foo: ($$anchor, $$slotProps) =>"),
        "missing the $$slots.foo filler callback:\n{js}"
    );
    assert!(
        js.contains("$.element("),
        "the filler body must emit the dynamic-element call:\n{js}"
    );
    assert!(
        js.contains("$.attribute_effect($$element, () => ({ slot: 'foo' }));"),
        "the slot attribute must fold into the attribute_effect:\n{js}"
    );
}

#[test]
fn plain_slot_prop_on_top_level_component() {
    // `<Inner slot="top"/>` at the ROOT — no component-family ancestor consumes it, so
    // official emits the plain prop `Inner($$anchor, { slot: 'top' })`.
    let js = emit_result(
        "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Inner slot=\"top\"/>\n",
    )
    .expect("a top-level component slot prop emits a module");
    assert!(
        js.contains("Inner($$anchor, {slot: 'top'})"),
        "the top-level slot must be a plain prop:\n{js}"
    );
    // NEGATIVE: no $$slots routing — there is no consuming parent.
    assert!(
        !js.contains("$$slots"),
        "a plain slot prop must not mint a $$slots object:\n{js}"
    );
}

#[test]
fn plain_slot_prop_on_component_nested_in_element() {
    // `<div><Inner slot="bar"/></div>` — the component is NOT a direct component child
    // (its parent is an element), so the slot is a plain prop: `Inner(node, { slot: 'bar' })`.
    let js = emit_result(
        "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n<div><Inner slot=\"bar\"/></div>\n",
    )
    .expect("an element-nested component slot prop emits a module");
    assert!(
        js.contains("{slot: 'bar'}"),
        "the nested component slot must be a plain prop:\n{js}"
    );
    assert!(
        !js.contains("$$slots"),
        "a plain slot prop must not mint a $$slots object:\n{js}"
    );
}

#[test]
fn plain_slot_prop_dynamic_and_mixed_on_nondirect_component() {
    // A DYNAMIC `slot={x}` on a NON-direct component host is officially ACCEPTED as an
    // ordinary reactive prop (`get slot() { return x; }`) — the `slot_attribute_invalid`
    // static-value rule applies ONLY to a DIRECT component child.
    let js = emit_result(
        "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Inner slot={x}/>\n",
    )
    .expect("a dynamic slot prop on a top-level component emits a module");
    assert!(
        js.contains("get slot() {return $$props.x;}"),
        "the dynamic slot must be a reactive getter prop:\n{js}"
    );
    // The MIXED `slot="a{x}"` form is the same plain-prop route (a template-literal getter).
    let mixed = emit_result(
        "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Inner slot=\"a{x}\"/>\n",
    )
    .expect("a mixed slot prop on a top-level component emits a module");
    assert!(
        mixed.contains("get slot() {return `a${"),
        "the mixed slot must be a template-literal getter prop:\n{mixed}"
    );
    assert!(
        !js.contains("$$slots") && !mixed.contains("$$slots"),
        "a plain slot prop must not mint a $$slots object"
    );
}

#[test]
fn plain_slot_prop_on_top_level_svelte_component() {
    // `<svelte:component this={C} slot="a"/>` at the root — the plain prop rides the
    // `$$component($$anchor, { slot: 'a' })` call.
    let js = emit_result(
        "<script>let { C } = $props();</script>\n<svelte:component this={C} slot=\"a\"/>\n",
    )
    .expect("a top-level svelte:component slot prop emits a module");
    assert!(
        js.contains("$$component($$anchor, {slot: 'a'})"),
        "the svelte:component slot must be a plain prop:\n{js}"
    );
    assert!(
        !js.contains("$$slots"),
        "a plain slot prop must not mint a $$slots object:\n{js}"
    );
}

#[test]
fn plain_slot_prop_on_nondirect_svelte_self() {
    // `{#if depth > 0}<svelte:self slot="a"/>{/if}` — a validly-placed NON-direct
    // `<svelte:self>`: the slot is a plain prop on the recursive self-call.
    let js = emit_result(
        "<script>let { depth } = $props();</script>\n{#if depth > 0}<svelte:self slot=\"a\"/>{/if}\n",
    )
    .expect("a non-direct svelte:self slot prop emits a module");
    assert!(
        js.contains("App(node_1, {slot: 'a'})"),
        "the svelte:self slot must be a plain prop on the self-call itself:\n{js}"
    );
    assert!(
        !js.contains("$$slots"),
        "a plain slot prop must not mint a $$slots object:\n{js}"
    );
}

#[test]
fn plain_slot_prop_on_component_inside_block() {
    // `{#if show}<Inner slot="c"/>{/if}` — a block body is NOT direct-child placement,
    // so the slot is a plain prop inside the consequent callback.
    let js = emit_result(
        "<script>import Inner from './Inner.svelte'; let { show } = $props();</script>\n{#if show}<Inner slot=\"c\"/>{/if}\n",
    )
    .expect("a block-nested component slot prop emits a module");
    assert!(
        js.contains("Inner($$anchor, {slot: 'c'})"),
        "the block-nested slot must be a plain prop:\n{js}"
    );
}

#[test]
fn plain_slot_prop_on_component_hoisted_from_slotted_fragment() {
    // `<Child><svelte:fragment slot="head"><Inner slot="x"/></svelte:fragment></Child>`
    // — the fragment's children are HOISTED into the `head` region, but hoisting does
    // NOT make Inner a direct component child (officially its owner is `Child`, its
    // PARENT is the fragment — `owner !== parent` + `is_component` ⇒ plain prop):
    //   $$slots: { head: (…) => { Inner($$anchor, { slot: 'x' }); } }
    let js = emit_result(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Child><svelte:fragment slot=\"head\"><Inner slot=\"x\"/></svelte:fragment></Child>\n",
    )
    .expect("a component slot prop inside a slotted fragment emits a module");
    assert!(
        js.contains("$$slots: {head: ($$anchor, $$slotProps) =>"),
        "missing the fragment's $$slots.head callback:\n{js}"
    );
    assert!(
        js.contains("Inner($$anchor, {slot: 'x'})"),
        "the hoisted component's slot must be a plain prop:\n{js}"
    );
    // NEGATIVE: the inner `slot="x"` must NOT mint an `x` slot entry on Child.
    assert!(
        !js.contains("x: ($$anchor, $$slotProps)"),
        "the hoisted component's slot must not mint a $$slots entry:\n{js}"
    );
}

#[test]
fn dynamic_or_mixed_slot_on_direct_component_child_fails_closed() {
    // A DYNAMIC / MIXED `slot` on a DIRECT component child is the official
    // `slot_attribute_invalid` compile error ("slot attribute must be a static value")
    // — the plain-prop acceptance is strictly the NON-direct placement.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let x = $state('a');</script>\n<Child><Inner slot={x}/></Child>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"),
    );
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let x = $state('a');</script>\n<Child><Inner slot=\"a{x}\"/></Child>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"),
    );
}

#[test]
fn dynamic_slot_on_direct_special_children_fails_closed() {
    // The same official `slot_attribute_invalid` reject for a dynamic `slot` on the
    // DIRECT `<svelte:component>` / `<svelte:self>` / `<svelte:element>` children.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let x = $state('a');</script>\n<Child><svelte:component this={Child} slot={x}/></Child>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"),
    );
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let x = $state('a');</script>\n<Child><svelte:self slot={x}/></Child>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"),
    );
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let x = $state('a');</script>\n<Child><svelte:element this=\"div\" slot={x}/></Child>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"),
    );
}

#[test]
fn explicit_default_slot_on_component_family_child_fails_closed() {
    // Official's `slot_default_duplicate` walk exempts ONLY a RegularElement /
    // SvelteFragment sibling carrying a `slot` attribute — a COMPONENT-family child
    // bearing `slot="default"` conflicts with ITSELF (it is default content that is not
    // an exempt node), so `<Child><Inner slot="default"/></Child>` is a hard official
    // reject even with no other content.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Child><Inner slot=\"default\"/></Child>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "default slot conflict",
                    ..
                }
            )
        },
    );
    // The `<svelte:element slot="default">` form self-conflicts identically.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><svelte:element this=\"div\" slot=\"default\"/></Child>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "default slot conflict",
                    ..
                }
            )
        },
    );
}

#[test]
fn default_slot_conflict_matches_official_exemption_rule() {
    // Official conflicts an explicit `slot="default"` with EVERY sibling fragment node
    // that is not a whitespace-only text and not a RegularElement / SvelteFragment
    // carrying a `slot` attribute. A COMMENT, a `{#snippet}` def, and a slot-bearing
    // COMPONENT sibling all conflict; a slot-bearing ELEMENT sibling is exempt.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"default\">{x}</span><!-- c --></Child>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "default slot conflict",
                    ..
                }
            )
        },
    );
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"default\">{x}</span>{#snippet s()}<p>y</p>{/snippet}</Child>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "default slot conflict",
                    ..
                }
            )
        },
    );
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Child><span slot=\"default\">{x}</span><Inner slot=\"foo\"/></Child>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "default slot conflict",
                    ..
                }
            )
        },
    );
    // POSITIVE control: a slot-bearing regular-element sibling is exempt — the pair
    // compiles to `children:` + the named `foo` entry.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"default\">{x}</span><p slot=\"foo\">y</p></Child>\n",
    )
    .expect("an exempt slot-bearing element sibling must not conflict");
    assert!(
        js.contains("children: ($$anchor, $$slotProps) =>"),
        "the explicit default content must become the children prop:\n{js}"
    );
    assert!(
        js.contains("foo: ($$anchor, $$slotProps) =>"),
        "the named element filler must keep its $$slots entry:\n{js}"
    );
}

// ─── `{#snippet}`-body DIRECT-child `slot=` disposition ───────────────────────

#[test]
fn static_slot_on_element_snippet_child_bakes_into_skeleton() {
    // `{#snippet foo()}<span slot="x">hi</span>{/snippet}` — official svelte@5.56.10
    // validates a `{#snippet}` direct child as component-owned placement
    // (`is_component = true` in `validate_slot_attribute`), so a STATIC `slot` on an
    // element snippet child is ACCEPTED and bakes into the cloned skeleton verbatim.
    let js = emit_result(
        "<script>let { x } = $props();</script>\n{#snippet foo()}<span slot=\"x\">hi</span>{/snippet}\n{@render foo()}\n",
    )
    .expect("a static slot on an element snippet child must emit a module");
    assert!(
        js.contains("$.from_html(`<span slot=\"x\">hi</span>`)"),
        "the slot attribute must bake into the cloned skeleton:\n{js}"
    );
    // NEGATIVE: a snippet child is NOT a slot filler — no `$$slots` region is minted
    // and no runtime slot attribute write appears.
    assert!(
        !js.contains("$$slots"),
        "a snippet child must not route into $$slots:\n{js}"
    );
    assert!(
        !js.contains("$.set_attribute"),
        "the baked slot must not also emit a runtime attribute write:\n{js}"
    );
}

#[test]
fn static_slot_on_component_snippet_child_stays_plain_prop() {
    // CONFORMANCE / CONSISTENCY CONTROL — NOT a RED→GREEN discriminator: this shape
    // already passed PRE-FIX through the old owner-less plain-prop shortcut
    // (`plain_component_slot_prop_host` with no direct-child membership), with the
    // IDENTICAL `{slot: 'x'}` emission. It pins that routing the acceptance through
    // the `snippet_static` arm keeps the component-family emission unchanged; the
    // DISCRIMINATING proof for the component family is
    // `dynamic_slot_on_direct_snippet_child_fails_closed`.
    //
    // `<Inner slot="x"/>` as a direct snippet child — official accepts and the `slot`
    // rides the component call as an ordinary prop (`Inner($$anchor, { slot: 'x' })`).
    let js = emit_result(
        "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n{#snippet foo()}<Inner slot=\"x\"/>{/snippet}\n{@render foo()}\n",
    )
    .expect("a static slot on a component snippet child must emit a module");
    assert!(
        js.contains("Inner($$anchor, {slot: 'x'})"),
        "the snippet-child component must keep the slot as a plain prop:\n{js}"
    );
    // NEGATIVE: no `$$slots` region — snippet children are not fillers.
    assert!(
        !js.contains("$$slots"),
        "a snippet child must not route into $$slots:\n{js}"
    );
}

#[test]
fn static_slot_on_svelte_component_snippet_child_stays_plain_prop() {
    // CONFORMANCE / CONSISTENCY CONTROL — NOT a RED→GREEN discriminator (passes
    // pre-fix too via the plain-prop shortcut; the discriminating component-family
    // proof is `dynamic_slot_on_direct_snippet_child_fails_closed`).
    //
    // `<svelte:component this={Inner} slot="x"/>` as a direct snippet child — the
    // dynamic-component wrapper keeps the `slot` prop on the inner `$$component` call.
    let js = emit_result(
        "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n{#snippet foo()}<svelte:component this={Inner} slot=\"x\"/>{/snippet}\n{@render foo()}\n",
    )
    .expect("a static slot on a svelte:component snippet child must emit a module");
    assert!(
        js.contains("$$component($$anchor, {slot: 'x'})"),
        "the svelte:component snippet child must keep the slot as a plain prop:\n{js}"
    );
    assert!(
        !js.contains("$$slots"),
        "a snippet child must not route into $$slots:\n{js}"
    );
}

#[test]
fn static_slot_on_svelte_self_snippet_child_stays_plain_prop() {
    // CONFORMANCE / CONSISTENCY CONTROL — NOT a RED→GREEN discriminator (passes
    // pre-fix too via the plain-prop shortcut; the discriminating component-family
    // proof is `dynamic_slot_on_direct_snippet_child_fails_closed`).
    //
    // `<svelte:self slot="x"/>` as a direct snippet child (a valid `<svelte:self>`
    // placement — snippets count) — the recursive self-call keeps the plain prop.
    let js = emit_result(
        "<script>let { x } = $props();</script>\n{#snippet foo()}<svelte:self slot=\"x\"/>{/snippet}\n{@render foo()}\n",
    )
    .expect("a static slot on a svelte:self snippet child must emit a module");
    assert!(
        js.contains("App(node, {slot: 'x'})"),
        "the svelte:self snippet child must keep the slot as a plain prop on the exact recursive self-call:\n{js}"
    );
    assert!(
        !js.contains("$$slots"),
        "a snippet child must not route into $$slots:\n{js}"
    );
}

#[test]
fn static_slot_on_svelte_element_snippet_child_folds_into_attribute_effect() {
    // `<svelte:element this="span" slot="x">` as a direct snippet child — the dynamic
    // element folds the static `slot` into its runtime `$.attribute_effect` object
    // (the element has no props object; the fold is the official output shape).
    let js = emit_result(
        "<script>let { x } = $props();</script>\n{#snippet foo()}<svelte:element this=\"span\" slot=\"x\">hi</svelte:element>{/snippet}\n{@render foo()}\n",
    )
    .expect("a static slot on a svelte:element snippet child must emit a module");
    assert!(
        js.contains("$.element("),
        "the snippet child must emit the dynamic-element call:\n{js}"
    );
    assert!(
        js.contains("$.attribute_effect($$element, () => ({ slot: 'x' }));"),
        "the slot attribute must fold into the attribute_effect:\n{js}"
    );
    assert!(
        !js.contains("$$slots"),
        "a snippet child must not route into $$slots:\n{js}"
    );
}

#[test]
fn dynamic_slot_on_direct_snippet_child_fails_closed() {
    // THE false-accept regression: the plain-prop shortcut ACCEPTED a dynamic
    // `slot={x}` on a component-family direct snippet child and quietly emitted
    // `Inner($$anchor, {slot: x()})` — official svelte@5.56.10 hard-errors
    // (`slot_attribute_invalid`: the slot attribute must be a static value; the rule
    // fires at `owner === parent`, and a `{#snippet}` body IS the owner). The dynamic
    // source is a snippet PARAM (an instance `let` is refused as InstanceScriptItem).
    let src = "<script>import Inner from './Inner.svelte'; let { p } = $props();</script>\n{#snippet foo(x)}<Inner slot={x}/>{/snippet}\n{@render foo(p)}\n";
    match emit_result(src) {
        Ok(js) => {
            // NEGATIVE: the false-accept shape must NEVER emit — neither a `slot:`
            // prop on the inner call nor any module at all.
            assert!(
                !js.contains("slot:"),
                "a dynamic snippet-child slot must not emit a slot prop:\n{js}"
            );
            panic!("a dynamic slot={{x}} on a direct snippet child must fail closed, got a module:\n{js}");
        }
        Err(ClientCompileError::Unsupported(surface)) => {
            assert!(
                matches!(
                    &surface,
                    UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"
                ),
                "wrong fail-closed surface: {surface:?}"
            );
            assert_eq!(
                surface.diagnostic_code(),
                "svelte-runtime-unsupported-dynamic-attribute",
                "the dynamic snippet-child slot must reject on the slot gate"
            );
        }
        Err(other) => panic!("expected the typed unsupported surface, got: {other:?}"),
    }
    // The component-family specials close identically; the element-family rows were
    // already rejected pre-fix and stay rejected (keep-reject controls).
    for (label, src) in [
        (
            "svelte_component_dynamic",
            "<script>import Inner from './Inner.svelte'; let { p } = $props();</script>\n{#snippet foo(x)}<svelte:component this={Inner} slot={x}/>{/snippet}\n{@render foo(p)}\n",
        ),
        (
            "svelte_self_dynamic",
            "<script>let { p } = $props();</script>\n{#snippet foo(x)}<svelte:self slot={x}/>{/snippet}\n{@render foo(p)}\n",
        ),
        (
            "element_dynamic_control",
            "<script>let { p } = $props();</script>\n{#snippet foo(x)}<span slot={x}>hi</span>{/snippet}\n{@render foo(p)}\n",
        ),
        (
            "svelte_element_dynamic_control",
            "<script>let { p } = $props();</script>\n{#snippet foo(x)}<svelte:element this=\"span\" slot={x}>hi</svelte:element>{/snippet}\n{@render foo(p)}\n",
        ),
    ] {
        assert_fail_closed_labeled(label, src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"
            )
        });
    }
}

#[test]
fn valueless_slot_on_element_snippet_child_fails_closed() {
    // Official svelte@5.56.10 accepts the snippet-child `slot` ONLY as a single
    // static TEXT-VALUED attribute (`is_text_attribute`): a valueless / boolean
    // `slot` (`<span slot>`) is the `slot_attribute_invalid` compile error. Pre-fix
    // Verter FALSE-ACCEPTED this shape and baked `slot=""` into the cloned skeleton;
    // the `snippet_static` arm now requires a PRESENT text value.
    let src = "<script>let { x } = $props();</script>\n{#snippet foo()}<span slot>hi</span>{/snippet}\n{@render foo()}\n";
    match emit_result(src) {
        Ok(js) => {
            // NEGATIVE: the false-accept shape must NEVER emit — no baked empty
            // `slot=""` and no slot attribute on the skeleton at all.
            assert!(
                !js.contains("slot=\"\""),
                "a valueless snippet-child slot must not bake an empty slot attribute:\n{js}"
            );
            assert!(
                !js.contains("<span slot"),
                "a valueless snippet-child slot must not bake into the skeleton:\n{js}"
            );
            panic!(
                "a valueless slot on an element snippet child must fail closed, got a module:\n{js}"
            );
        }
        Err(ClientCompileError::Unsupported(surface)) => {
            assert!(
                matches!(
                    &surface,
                    UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"
                ),
                "wrong fail-closed surface: {surface:?}"
            );
            assert_eq!(
                surface.diagnostic_code(),
                "svelte-runtime-unsupported-dynamic-attribute",
                "the valueless snippet-child slot must reject on the slot gate"
            );
        }
        Err(other) => panic!("expected the typed unsupported surface, got: {other:?}"),
    }
}

#[test]
fn valueless_slot_on_component_snippet_child_fails_closed() {
    // `<Inner slot/>` as a direct snippet child — official rejects
    // (`slot_attribute_invalid`: the snippet-child `slot` must be a static
    // TEXT-VALUED attribute, `is_text_attribute`). Pre-fix Verter FALSE-ACCEPTED
    // and emitted the boolean plain prop `{slot: true}`.
    let src = "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n{#snippet foo()}<Inner slot/>{/snippet}\n{@render foo()}\n";
    match emit_result(src) {
        Ok(js) => {
            // NEGATIVE: the boolean-prop false-accept shape must NEVER emit.
            assert!(
                !js.contains("{slot: true}"),
                "a valueless snippet-child slot must not emit a boolean slot prop:\n{js}"
            );
            panic!(
                "a valueless slot on a component snippet child must fail closed, got a module:\n{js}"
            );
        }
        Err(ClientCompileError::Unsupported(surface)) => {
            assert!(
                matches!(
                    &surface,
                    UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"
                ),
                "wrong fail-closed surface: {surface:?}"
            );
            assert_eq!(
                surface.diagnostic_code(),
                "svelte-runtime-unsupported-dynamic-attribute",
                "the valueless snippet-child slot must reject on the slot gate"
            );
        }
        Err(other) => panic!("expected the typed unsupported surface, got: {other:?}"),
    }
    // The component-family specials close identically (valueless rows).
    for (label, src) in [
        (
            "svelte_component_valueless",
            "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n{#snippet foo()}<svelte:component this={Inner} slot/>{/snippet}\n{@render foo()}\n",
        ),
        (
            "svelte_self_valueless",
            "<script>let { x } = $props();</script>\n{#snippet foo()}<svelte:self slot/>{/snippet}\n{@render foo()}\n",
        ),
    ] {
        assert_fail_closed_labeled(label, src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"
            )
        });
    }
}

#[test]
fn valueless_slot_on_toplevel_component_class_b_still_accepts() {
    // POSITIVE CONTROL (scope lock): a valueless `slot` on a TOP-LEVEL component
    // (`<Inner slot/>` — NOT a snippet child, NOT a direct component child) is a
    // GENUINE official svelte@5.56.10 ACCEPT — the Class B plain prop,
    // `Inner($$anchor, {slot: true})`. The valueless-reject fix is SNIPPET-ONLY;
    // this test pins that no whole-class valueless guard leaked into the plain-prop
    // path (it passes both PRE-fix and POST-fix — a fail here means the fix was
    // mis-scoped into a fail-closed regression).
    let js = emit_result(
        "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Inner slot/>\n",
    )
    .expect("a valueless slot on a top-level component must stay an accepted plain prop");
    assert!(
        js.contains("Inner($$anchor, {slot: true})"),
        "the top-level valueless slot must ride the component call as a boolean prop:\n{js}"
    );
    // NEGATIVE: no `$$slots` routing — a plain prop is not a filler.
    assert!(
        !js.contains("$$slots"),
        "a plain-prop slot must not route into $$slots:\n{js}"
    );
}

#[test]
fn static_slot_on_svelte_fragment_snippet_child_stays_fail_closed() {
    // `<svelte:fragment slot="x">` as a direct snippet child — official rejects
    // (`svelte_fragment_invalid_placement`); Verter fails closed on the slot gate.
    // `SpecialKind::Fragment` is NOT a filler host — the kind gate is exactly what
    // keeps the snippet-static acceptance off the non-host specials.
    assert_fail_closed(
        "<script>let { x } = $props();</script>\n{#snippet foo()}<svelte:fragment slot=\"x\">hi</svelte:fragment>{/snippet}\n{@render foo()}\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"
            )
        },
    );
}

#[test]
fn static_slot_on_svelte_boundary_snippet_child_stays_fail_closed() {
    // `<svelte:boundary slot="x">` as a direct snippet child — official rejects
    // (`svelte_boundary_invalid_attribute`); Verter fails closed on the slot gate
    // (`SpecialKind::Boundary` is NOT a filler host).
    assert_fail_closed(
        "<script>let { x } = $props();</script>\n{#snippet foo()}<svelte:boundary slot=\"x\"><p>hi</p></svelte:boundary>{/snippet}\n{@render foo()}\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"
            )
        },
    );
}

#[test]
fn validate_slot_placement_disposition_is_exhaustive_per_host_kind() {
    // The per-kind EXHAUSTIVE proof for the unified slot choke-point
    // (`validate_slot_placement`, run at `classify_node` entry for EVERY node),
    // pinning the official disposition per host kind × placement:
    //
    // - FILLER hosts (regular element, component, `<svelte:component>` /
    //   `<svelte:self>` / `<svelte:element>`): a STATIC `slot` is accepted EXACTLY
    //   when the node is a lowering-recorded direct static-slot filler OR a direct
    //   `{#snippet}`-body child carrying a TEXT VALUE (the static text-valued
    //   snippet branch — a plain attr/prop, never a filler; a valueless/boolean
    //   `slot` on a snippet child rejects).
    // - PLAIN-PROP hosts (component, `<svelte:component>` / `<svelte:self>`): any
    //   `slot` form is accepted at OWNER-LESS placement (neither a direct component
    //   child nor a direct snippet child); a dynamic/mixed `slot` on a DIRECT child
    //   of EITHER owner fails closed (official `slot_attribute_invalid`).
    // - Every OTHER kind (head / boundary / fragment / options / the global hosts)
    //   fails closed for every form — even when the placement sets claim membership.
    use crate::svelte::runtime::client_surface::{validate_slot_placement, SlotPlacementFacts};
    use crate::svelte::runtime::expr::ScopeId;
    use crate::svelte::runtime::ir::{
        AttrIr, ComponentIrNode, ComponentSlots, ElementIr, ExprId, IrNode, MixedAttrPart, NodeId,
        SpecialElementIr, SpecialKind, StaticAttrValue,
    };
    use verter_span::Span;

    let span = Span::new(0, 0);
    let static_slot = || AttrIr::Static {
        name: "slot".to_string(),
        value: Some(StaticAttrValue {
            value: crate::svelte::runtime::entity_decode::DecodedAttrValue::decode(
                "x",
                &mut |_| {},
            ),
        }),
    };
    let dynamic_slot = || AttrIr::Dynamic {
        name: "slot".to_string(),
        expr: ExprId(0),
    };
    let mixed_slot = || AttrIr::Mixed {
        name: "slot".to_string(),
        parts: vec![
            MixedAttrPart::Literal("a".to_string()),
            MixedAttrPart::Expr(ExprId(0)),
        ],
    };
    // A VALUELESS / boolean `slot` (official `is_text_attribute` fails: no text
    // value) — static in form, but NOT snippet-static-acceptable.
    let valueless_slot = || AttrIr::Static {
        name: "slot".to_string(),
        value: None,
    };
    let empty = rustc_hash::FxHashSet::default();
    let member: rustc_hash::FxHashSet<NodeId> = std::iter::once(NodeId(0)).collect();

    let special = |kind: SpecialKind, attrs: Vec<AttrIr>| {
        IrNode::Special(SpecialElementIr {
            kind,
            span,
            attrs,
            this_expr: None,
            static_tag: None,
            children: Vec::new(),
            scope: ScopeId(0),
            slots: ComponentSlots::default(),
            body_region: None,
            head_title: None,
        })
    };
    let component = |attrs: Vec<AttrIr>| {
        IrNode::Component(ComponentIrNode {
            name: "Inner".to_string(),
            span,
            attrs,
            children: Vec::new(),
            scope: ScopeId(0),
            slots: ComponentSlots::default(),
        })
    };
    let element = |attrs: Vec<AttrIr>| {
        IrNode::Element(ElementIr {
            tag: "span".to_string(),
            span,
            attrs,
            children: Vec::new(),
            scope: ScopeId(0),
        })
    };

    let rejects = |node: &IrNode,
                   fillers: &rustc_hash::FxHashSet<NodeId>,
                   direct: &rustc_hash::FxHashSet<NodeId>,
                   snippets: &rustc_hash::FxHashSet<NodeId>,
                   label: &str| {
        let placement = SlotPlacementFacts {
            static_slot_filler_hosts: fillers,
            direct_slot_attr_child_hosts: direct,
            direct_snippet_slot_attr_child_hosts: snippets,
        };
        let err = validate_slot_placement(node, NodeId(0), placement)
            .expect_err(&format!("{label}: the slot-bearing node must fail closed"));
        assert!(
            matches!(
                &err,
                UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"
            ),
            "{label}: wrong refusal surface: {err:?}"
        );
        assert_eq!(
            err.diagnostic_code(),
            "svelte-runtime-unsupported-dynamic-attribute",
            "{label}: wrong diagnostic id"
        );
    };
    let accepts = |node: &IrNode,
                   fillers: &rustc_hash::FxHashSet<NodeId>,
                   direct: &rustc_hash::FxHashSet<NodeId>,
                   snippets: &rustc_hash::FxHashSet<NodeId>,
                   label: &str| {
        let placement = SlotPlacementFacts {
            static_slot_filler_hosts: fillers,
            direct_slot_attr_child_hosts: direct,
            direct_snippet_slot_attr_child_hosts: snippets,
        };
        assert!(
            validate_slot_placement(node, NodeId(0), placement).is_ok(),
            "{label}: the slot placement must be accepted"
        );
    };

    // EXHAUSTIVE `SpecialKind` coverage — the wildcard-free match forces a compile
    // error here when a new kind is added without extending this proof.
    let all_kinds = [
        SpecialKind::Head,
        SpecialKind::Window,
        SpecialKind::Document,
        SpecialKind::Body,
        SpecialKind::Element,
        SpecialKind::Boundary,
        SpecialKind::Options,
        SpecialKind::Component,
        SpecialKind::SelfRef,
        SpecialKind::Fragment,
    ];
    for kind in all_kinds {
        match kind {
            SpecialKind::Head
            | SpecialKind::Window
            | SpecialKind::Document
            | SpecialKind::Body
            | SpecialKind::Element
            | SpecialKind::Boundary
            | SpecialKind::Options
            | SpecialKind::Component
            | SpecialKind::SelfRef
            | SpecialKind::Fragment => {}
        }
        match kind {
            // The component-family PLAIN-PROP specials: static/dynamic/mixed accepted
            // at NON-direct placement; static accepted as a direct FILLER; dynamic and
            // mixed refused on a direct child.
            SpecialKind::Component | SpecialKind::SelfRef => {
                accepts(
                    &special(kind, vec![static_slot()]),
                    &empty,
                    &empty,
                    &empty,
                    &format!("special {kind:?} static slot, non-direct"),
                );
                accepts(
                    &special(kind, vec![dynamic_slot()]),
                    &empty,
                    &empty,
                    &empty,
                    &format!("special {kind:?} dynamic slot, non-direct"),
                );
                accepts(
                    &special(kind, vec![mixed_slot()]),
                    &empty,
                    &empty,
                    &empty,
                    &format!("special {kind:?} mixed slot, non-direct"),
                );
                accepts(
                    &special(kind, vec![static_slot()]),
                    &member,
                    &member,
                    &empty,
                    &format!("special {kind:?} static slot, direct filler"),
                );
                rejects(
                    &special(kind, vec![dynamic_slot()]),
                    &empty,
                    &member,
                    &empty,
                    &format!("special {kind:?} dynamic slot, direct child"),
                );
                rejects(
                    &special(kind, vec![mixed_slot()]),
                    &empty,
                    &member,
                    &empty,
                    &format!("special {kind:?} mixed slot, direct child"),
                );
            }
            // `<svelte:element>` is a FILLER host but NOT a plain-prop host: static
            // accepted ONLY as a direct filler; dynamic refused everywhere.
            SpecialKind::Element => {
                accepts(
                    &special(kind, vec![static_slot()]),
                    &member,
                    &member,
                    &empty,
                    "svelte:element static slot, direct filler",
                );
                rejects(
                    &special(kind, vec![static_slot()]),
                    &empty,
                    &empty,
                    &empty,
                    "svelte:element static slot, non-direct",
                );
                rejects(
                    &special(kind, vec![dynamic_slot()]),
                    &member,
                    &member,
                    &empty,
                    "svelte:element dynamic slot, direct filler",
                );
                rejects(
                    &special(kind, vec![dynamic_slot()]),
                    &empty,
                    &empty,
                    &empty,
                    "svelte:element dynamic slot, non-direct",
                );
            }
            // Every remaining special is NEVER a slot host: static and dynamic refuse
            // even when ALL the placement sets claim membership — component filler,
            // direct child, AND direct snippet child (the kind gate is the
            // no-residual-member proof; snippet membership must NOT flip a
            // `<svelte:fragment>` / `<svelte:boundary>` / meta host to accepted).
            SpecialKind::Head
            | SpecialKind::Window
            | SpecialKind::Document
            | SpecialKind::Body
            | SpecialKind::Boundary
            | SpecialKind::Options
            | SpecialKind::Fragment => {
                rejects(
                    &special(kind, vec![static_slot()]),
                    &member,
                    &member,
                    &member,
                    &format!("special {kind:?} static slot"),
                );
                rejects(
                    &special(kind, vec![dynamic_slot()]),
                    &member,
                    &member,
                    &member,
                    &format!("special {kind:?} dynamic slot"),
                );
            }
        }
    }
    // A COMPONENT: plain-prop accepted at non-direct placement (every form), the
    // static direct FILLER accepted, and dynamic/mixed refused on a direct child. A
    // direct child whose static slot was NOT recorded as a filler (the defensive
    // set-desync posture, e.g. a valueless `slot`) stays refused.
    accepts(
        &component(vec![static_slot()]),
        &empty,
        &empty,
        &empty,
        "component static slot, non-direct",
    );
    accepts(
        &component(vec![dynamic_slot()]),
        &empty,
        &empty,
        &empty,
        "component dynamic slot, non-direct",
    );
    accepts(
        &component(vec![mixed_slot()]),
        &empty,
        &empty,
        &empty,
        "component mixed slot, non-direct",
    );
    accepts(
        &component(vec![static_slot()]),
        &member,
        &member,
        &empty,
        "component static slot, direct filler",
    );
    rejects(
        &component(vec![static_slot()]),
        &empty,
        &member,
        &empty,
        "component static slot, direct non-filler",
    );
    rejects(
        &component(vec![dynamic_slot()]),
        &empty,
        &member,
        &empty,
        "component dynamic slot, direct child",
    );
    rejects(
        &component(vec![mixed_slot()]),
        &empty,
        &member,
        &empty,
        "component mixed slot, direct child",
    );
    // A regular ELEMENT: static accepted ONLY as a direct filler (the official
    // `slot_attribute_invalid_placement` otherwise); dynamic/mixed refused even there
    // (the official `slot_attribute_invalid`); never a plain-prop host.
    accepts(
        &element(vec![static_slot()]),
        &member,
        &member,
        &empty,
        "element static slot, direct filler",
    );
    rejects(
        &element(vec![static_slot()]),
        &empty,
        &empty,
        &empty,
        "element static slot, non-filler placement",
    );
    rejects(
        &element(vec![dynamic_slot()]),
        &member,
        &member,
        &empty,
        "element dynamic slot, direct filler",
    );
    rejects(
        &element(vec![mixed_slot()]),
        &member,
        &member,
        &empty,
        "element mixed slot, direct filler",
    );
    // SNIPPET placement: a DIRECT `{#snippet}`-body child (the snippet set claims
    // membership; the component sets stay empty). A static TEXT-VALUED `slot` is
    // accepted on every filler-capable host kind as a plain attr/prop
    // (snippet_static — official `is_text_attribute`); a dynamic/mixed `slot`
    // REJECTS on every kind — snippet membership DISABLES the plain-prop path a
    // component-family host would otherwise take (the pre-fix false-accept leak) —
    // and a VALUELESS/boolean `slot` REJECTS too (the second false-accept leak:
    // static in form, but not text-valued).
    accepts(
        &element(vec![static_slot()]),
        &empty,
        &empty,
        &member,
        "element static slot, direct snippet child",
    );
    rejects(
        &element(vec![dynamic_slot()]),
        &empty,
        &empty,
        &member,
        "element dynamic slot, direct snippet child",
    );
    rejects(
        &element(vec![mixed_slot()]),
        &empty,
        &empty,
        &member,
        "element mixed slot, direct snippet child",
    );
    rejects(
        &element(vec![valueless_slot()]),
        &empty,
        &empty,
        &member,
        "element valueless slot, direct snippet child (the valueless false-accept closure)",
    );
    accepts(
        &component(vec![static_slot()]),
        &empty,
        &empty,
        &member,
        "component static slot, direct snippet child",
    );
    rejects(
        &component(vec![dynamic_slot()]),
        &empty,
        &empty,
        &member,
        "component dynamic slot, direct snippet child (the false-accept closure)",
    );
    rejects(
        &component(vec![mixed_slot()]),
        &empty,
        &empty,
        &member,
        "component mixed slot, direct snippet child",
    );
    rejects(
        &component(vec![valueless_slot()]),
        &empty,
        &empty,
        &member,
        "component valueless slot, direct snippet child (the valueless false-accept closure)",
    );
    for kind in [
        SpecialKind::Component,
        SpecialKind::SelfRef,
        SpecialKind::Element,
    ] {
        accepts(
            &special(kind, vec![static_slot()]),
            &empty,
            &empty,
            &member,
            &format!("special {kind:?} static slot, direct snippet child"),
        );
        rejects(
            &special(kind, vec![dynamic_slot()]),
            &empty,
            &empty,
            &member,
            &format!("special {kind:?} dynamic slot, direct snippet child"),
        );
        rejects(
            &special(kind, vec![mixed_slot()]),
            &empty,
            &empty,
            &member,
            &format!("special {kind:?} mixed slot, direct snippet child"),
        );
        rejects(
            &special(kind, vec![valueless_slot()]),
            &empty,
            &empty,
            &member,
            &format!("special {kind:?} valueless slot, direct snippet child"),
        );
    }
    // SCOPE LOCK (Class B unit control): the valueless-reject is SNIPPET-ONLY — a
    // valueless `slot` on an OWNER-LESS component-family host stays the accepted
    // plain prop (official accepts `<Inner slot/>` at top level as `{slot: true}`).
    accepts(
        &component(vec![valueless_slot()]),
        &empty,
        &empty,
        &empty,
        "component valueless slot, non-direct (Class B plain prop)",
    );
    // Negative controls: a slot-free attr inventory validates trivially on every
    // attr-bearing kind, and a node kind with no attribute surface always validates.
    let plain_attr = || AttrIr::Static {
        name: "class".to_string(),
        value: Some(StaticAttrValue {
            value: crate::svelte::runtime::entity_decode::DecodedAttrValue::decode(
                "c",
                &mut |_| {},
            ),
        }),
    };
    let no_placement = SlotPlacementFacts {
        static_slot_filler_hosts: &empty,
        direct_slot_attr_child_hosts: &empty,
        direct_snippet_slot_attr_child_hosts: &empty,
    };
    assert!(
        validate_slot_placement(&component(vec![plain_attr()]), NodeId(0), no_placement).is_ok(),
        "a slot-free component attr inventory must validate"
    );
    assert!(
        validate_slot_placement(&element(vec![plain_attr()]), NodeId(0), no_placement).is_ok(),
        "a slot-free element attr inventory must validate"
    );
    assert!(
        validate_slot_placement(
            &special(SpecialKind::Element, vec![plain_attr()]),
            NodeId(0),
            no_placement
        )
        .is_ok(),
        "a slot-free special attr inventory must validate"
    );
    let text = IrNode::Text {
        span,
        text: "hi".to_string(),
    };
    assert!(
        validate_slot_placement(&text, NodeId(0), no_placement).is_ok(),
        "a text node has no attribute surface"
    );
}

#[test]
fn render_dynamic_prop_callees_stay_on_the_snippet_route() {
    // NEGATIVE CONTROLS: a PROP-resolving callee is DYNAMIC — official keeps the
    // `$.snippet` route for both the plain and the optional form (the static-name
    // fast path applies ONLY to a resolved local `{#snippet}` binding).
    let plain = emit_result("<script>let { row } = $props();</script>\n{@render row(1)}\n")
        .expect("a plain prop render callee emits a module");
    assert!(
        plain.contains("$.snippet(node, () => $$props.row, () => 1);"),
        "a plain prop callee must stay on the $.snippet route:\n{plain}"
    );
    let optional = emit_result("<script>let { row } = $props();</script>\n{@render row?.(1)}\n")
        .expect("an optional prop render callee emits a module");
    assert!(
        optional.contains("$.snippet(node, () => $$props.row ?? $.noop, () => 1);"),
        "an optional prop callee must stay on the $.snippet + noop route:\n{optional}"
    );
    assert!(
        !optional.contains("row?.($$anchor"),
        "an optional prop callee must not take the static direct-call form:\n{optional}"
    );
}

#[test]
#[should_panic(expected = "after-update op target not ranked")]
fn modern_event_emit_panics_loudly_when_its_target_rank_is_missing() {
    // The ENTER-rank half: a regular-element MODERN `on*` event streams at
    // `after_update_pre_rank` (`client_emit.rs`). Driving the real emit with the
    // event target's rank entry removed must panic AT THE CALL SITE with the
    // invariant message — reverting `after_update_pre_rank` to
    // `.unwrap_or(u32::MAX)` makes this emit succeed (a silent tail sort) and this
    // test fail (no panic), so it catches a re-introduced silent MAX where the
    // direct-helper test above cannot.
    let _ = emit_with_after_update_ranks_cleared(
        "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    );
}

#[test]
fn state_snapshot_in_event_handler_rewrites_to_dollar_snapshot() {
    // `$state.snapshot(x)` in an event-handler body rewrites its callee to
    // `$.snapshot(x)`; the argument passes through (a BareProxy `o` reads plain).
    // The handler is a `$state` write (`snap = …`) so the delegated narrow path
    // admits it.
    let js = emit(
        "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = $state.snapshot(o)}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snapshot(o)"),
        "`$state.snapshot(o)` rewrites to `$.snapshot(o)`:\n{js}"
    );
    // NEGATIVE: never the raw rune member (a runtime ReferenceError).
    assert!(
        !js.contains("$state.snapshot"),
        "the raw `$state.snapshot` rune member must be gone:\n{js}"
    );
}

#[test]
fn state_snapshot_in_instance_script_expression_rewrites() {
    // `$state.snapshot(x)` in an INSTANCE-SCRIPT expression (a `$state.raw` init) is
    // rewritten too: `let snap = $state.raw($state.snapshot(o))` → `let snap =
    // $.snapshot(o);` (a never-reassigned raw init is a `PlainLet`, and its init
    // routes through the rewriter).
    let js = emit(
        "<script>let o = $state({ a: 1 });\nlet snap = $state.raw($state.snapshot(o));</script>\n<button onclick={() => o.a++}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let snap = $.snapshot(o);"),
        "a `$state.snapshot` inside an instance-script init rewrites:\n{js}"
    );
    assert!(
        !js.contains("$state.snapshot"),
        "no raw `$state.snapshot` may remain:\n{js}"
    );
}

#[test]
fn state_snapshot_in_template_interpolation_uses_the_memoized_value_carrier() {
    let js = emit(
        "<script>let o = $state({ a: 1 });</script>\n<button onclick={() => o.a++}>{$state.snapshot(o)}</button>\n",
        "App.svelte",
    );
    let normalized = normalize_js_cosmetics(&js);
    assert!(
        normalized.contains(&nc(
            "$.template_effect(($0)=>$.set_text(text,$0),[()=>$.snapshot(o)])"
        )),
        "snapshot interpolation must lower through the call-bearing deps carrier:\n{js}"
    );
    assert!(
        !js.contains("$state.snapshot"),
        "no raw rune survives:\n{js}"
    );
    assert!(parses_as_js(&js));
}

#[test]
fn uncalled_state_snapshot_value_position_fails_closed_as_rune() {
    // F1 (fail-open fix): an UNCALLED `$state.snapshot` in a VALUE position
    // (`x = $state.snapshot`) is NOT the supported call form — only `$state.snapshot(x)`
    // rewrites to `$.snapshot`. Official errors on the parenthesis-less rune
    // (`rune_missing_parentheses`). Before F1 the rune scan blanket-exempted ANY
    // `$state.snapshot` member (call or not), so this slipped past and emitted a raw
    // `$state.snapshot` (a runtime ReferenceError). It MUST fail closed as an advanced
    // rune.
    assert_fail_closed(
        "<script>let x = $state(0);</script>\n<button onclick={() => x = $state.snapshot}>b</button>\n",
        |s| matches!(
            s,
            UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.snapshot"
        ),
    );
    // A bare-identifier call-argument value position also refuses.
    assert_fail_closed(
        "<script>let x = $state(0); function foo(_v){}</script>\n<button onclick={() => { foo($state.snapshot); x++; }}>b</button>\n",
        |s| matches!(
            s,
            UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.snapshot"
        ),
    );
}

#[test]
fn state_snapshot_single_non_spread_arg_still_rewrites_after_arity_gate() {
    // G1 negative half (DISCRIMINATING): the WELL-FORMED single-non-spread-arg form
    // `$state.snapshot(o)` must STILL rewrite to `$.snapshot(o)` (the arity/spread gate
    // must NOT regress the valid form). Official emits `$.snapshot(o)` (oracle-verified).
    let js = emit(
        "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = $state.snapshot(o)}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snapshot(o)"),
        "the well-formed `$state.snapshot(o)` still rewrites to `$.snapshot(o)`:\n{js}"
    );
    // NEGATIVE: no raw rune member, and the arity gate did not turn the valid form into
    // a bogus zero/multi-arg `$.snapshot`.
    assert!(
        !js.contains("$state.snapshot"),
        "no raw `$state.snapshot` may remain:\n{js}"
    );
    assert!(
        !js.contains("$.snapshot()") && !js.contains("$.snapshot(o, o)"),
        "the well-formed rewrite must be exactly `$.snapshot(o)`:\n{js}"
    );
}

#[test]
fn uncalled_paren_state_snapshot_value_position_fails_closed_as_rune() {
    // An UNCALLED parenthesized `($state.snapshot)` / `(($state.snapshot))` in a
    // VALUE position — official `rune_missing_parentheses` (oracle-verified against
    // svelte@5.56.10). The wrapper parens are transparent: there is no enclosing
    // call to exempt the member, so the member handler still owns the uncalled
    // value position and records the precise `$state.snapshot` refusal.
    let cases: &[(&str, &str)] = &[
        (
            "paren_uncalled",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = ($state.snapshot)}>x</button>\n",
        ),
        (
            "double_paren_uncalled",
            "<script>let o = $state({ a: 1 });\nlet snap = $state(null);</script>\n<button onclick={() => snap = (($state.snapshot))}>x</button>\n",
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
fn raw_state_object_member_write_in_handler_emits_get_member_mutation() {
    // F4: a `$state.raw({ x: 0 })` reassigned is a raw SIGNAL. A delegated handler
    // mutating a MEMBER (`o.x++`) AND reassigning the root (`o = { x: 2 }`) must be
    // ADMITTED: the member mutation lowers to `$.get(o).x++` (the raw signal read),
    // and the reassign lowers to `$.set(o, { x: 2 })` with NO trailing `, true` (raw
    // never proxies — Q6). Before F4 the member write refused at the handler shape
    // gate.
    let js = emit(
        "<script>let o = $state.raw({ x: 0 });</script>\n<button onclick={() => { o.x++; o = { x: 2 }; }}>b</button>\n",
        "App.svelte",
    );
    // The declaration: a raw signal box, NO `$.proxy`.
    assert!(
        js.contains("let o = $.state({ x: 0 });"),
        "raw object $state reassigned is a `$.state(...)` box with no proxy:\n{js}"
    );
    // The member mutation reads the signal via `$.get(o).x++`.
    assert!(
        js.contains("$.get(o).x++"),
        "the raw signal member mutation lowers to `$.get(o).x++`:\n{js}"
    );
    // The reassign is `$.set(o, { x: 2 })` with NO trailing `, true` (raw never
    // proxies — Q6).
    assert!(
        js.contains("$.set(o, { x: 2 })"),
        "the raw signal reassign lowers to a bare `$.set(o, ...)`:\n{js}"
    );
    assert!(
        !js.contains("$.set(o, { x: 2 }, true)"),
        "a raw signal reassign must NOT carry the proxy `, true` flag (Q6):\n{js}"
    );
    // NEGATIVE: the raw object init must not be wrapped in `$.proxy`.
    assert!(
        !js.contains("$.proxy"),
        "a `$state.raw` object must never emit `$.proxy`:\n{js}"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// The native effect family: plain `$effect` + `$effect.pre` + `$effect.root` +
// `$effect.tracking` (position parity with svelte@5.56.10 — `$effect` /
// `$effect.pre` are STATEMENT-ONLY (`effect_invalid_placement`), `.root` /
// `.tracking` are expression-valued; non-call / malformed / unknown-member /
// value-position forms stay fail-closed).
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn effect_in_direct_handler_lowers_with_frame() {
    // A well-formed `$effect(fn)` call INSIDE a direct-host (`$.event`) handler
    // arrow lowers through the shared rewriter (`$.user_effect`) and forces the
    // component frame (`$.push($$props, true)` / `$.pop()` + the `$$props`
    // param) — the official call-position parity for a handler closure.
    // Oracle-verified: official lowers `$effect` wherever it appears in runes
    // scope.
    let js = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => c++} onfocus={() => { $effect(() => console.log(c)); }}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_effect(() => console.log($.get(c)))"),
        "the handler-nested `$effect` lowers to `$.user_effect` with the body rewritten:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the `$effect` forces the `$$props` param:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the `$effect` forces the runes frame open:\n{js}"
    );
    assert!(js.contains("$.pop();"), "the frame closes:\n{js}");
    assert!(
        !js.contains("$effect"),
        "no raw `$effect` rune survives:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_family_non_call_forms_fail_closed() {
    // Non-call `$effect` value references and uncalled member references stay
    // fail-closed (official emits the raw reference — a runtime ReferenceError —
    // for some of these; Verter refuses instead, never emitting a raw rune).
    let cases: &[(&str, &str, &str)] = &[
        (
            "bare_ref_arg",
            "<script>let c = $state(0); foo($effect);</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect",
        ),
        (
            "bare_statement",
            "<script>let c = $state(0); $effect;</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect",
        ),
        (
            "uncalled_pre_member",
            "<script>let c = $state(0); const f = $effect.pre;</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect.pre",
        ),
        (
            "uncalled_root_member",
            "<script>let c = $state(0); const f = $effect.root;</script>\n<button onclick={() => c++}>{c}</button>\n",
            "$effect.root",
        ),
        (
            "uncalled_tracking_member",
            "<script>let c = $state(0); const f = $effect.tracking;</script>\n<button onclick={() => c++}>{c}</button>\n",
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
fn effect_tracking_carrier_rejected_declaration_shapes_fail_closed() {
    // The declaration shapes the effect-rune-init carrier REJECTS — a `var`
    // keyword, a multi-declarator declaration, a TS-annotated declarator — fail
    // closed at their existing declaration gates AND mint no
    // `EffectTrackingConst` binding fact (the minting shares the carrier's
    // exact shape predicate; the no-fact half is pinned by the
    // `state_scan::tests` minting unit test).
    let cases: &[(&str, &str, &str)] = &[
        (
            "var_tracking_decl",
            "<script>let c = $state(0); var t = $effect.tracking();</script>\n<button onclick={() => c++}>{c}</button>\n",
            "var declaration",
        ),
        (
            "multi_declarator_tracking",
            "<script>let c = $state(0); let a = $effect.tracking(), b = 0;</script>\n<button onclick={() => c++}>{c}</button>\n",
            "multi-declarator let",
        ),
    ];
    for (label, source, construct_label) in cases {
        assert_fail_closed_labeled(
            label,
            source,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if construct == construct_label),
        );
    }
    // The TS-annotated form in a PLAIN `<script>` rejects even EARLIER — the
    // official-reject parity gate (`js_parse_error`: official parses a plain
    // script as JS, where a type annotation is a parse error) — still
    // fail-closed, never a minted fact.
    let err = emit_result(
        "<script>let c = $state(0); let t: boolean = $effect.tracking();</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect_err("a TS-annotated tracking declarator in a plain script must reject");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected the js_parse_error official reject, got {err:?}");
    };
    assert_eq!(
        rejection.official_code, "js_parse_error",
        "wrong official-reject code: {rejection:?}"
    );
}

#[test]
fn effect_root_is_assignable_expression_with_nested_effects_and_cleanup() {
    // `$effect.root(fn)` is an EXPRESSION: `const stop = $effect.root(...)`
    // preserves the `stop` binding; a nested `$effect` / `$effect.pre` inside the
    // root callback lowers through the IDENTICAL callee rewrite (no separate
    // root-recursion gate); the `return () => {};` cleanup flows through
    // verbatim. Oracle-verified against svelte@5.56.10.
    let js = emit(
        "<script>\n\tlet c = $state(0);\n\tconst stop = $effect.root(() => {\n\t\t$effect(() => console.log(c));\n\t\treturn () => {};\n\t});\n</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const stop = $.effect_root(() => {"),
        "the root call is an assignable expression with the `stop` binding preserved:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(() => console.log($.get(c)));"),
        "the nested `$effect` lowers inside the root body:\n{js}"
    );
    assert!(
        js.contains("return () => {};"),
        "the cleanup return flows through verbatim:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);") && js.contains("$.pop();"),
        "the NESTED `$effect` forces the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The nested-PRE variant: `$.user_pre_effect` inside the root body (and NOT
    // the plain `$.user_effect` — the helper-rename discriminator).
    let js = emit(
        "<script>\n\tlet c = $state(0);\n\tconst stop = $effect.root(() => {\n\t\t$effect.pre(() => console.log(c));\n\t\treturn () => {};\n\t});\n</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.user_pre_effect(() => console.log($.get(c)));"),
        "the nested `$effect.pre` lowers inside the root body:\n{js}"
    );
    assert!(
        !js.contains("$.user_effect("),
        "the nested `.pre` must NOT lower to the plain `$.user_effect`:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the nested `$effect.pre` forces the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_tracking_inline_template_interpolation_uses_the_memoized_value_carrier() {
    let js = emit(
        "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n<p>{$effect.tracking()}</p>\n",
        "App.svelte",
    );
    let normalized = normalize_js_cosmetics(&js);
    assert!(
        normalized.contains(&nc("$.set_text(text_1,$0)"))
            && normalized.contains(&nc("[()=>($.effect_tracking())]")),
        "tracking interpolation must lower through the call-bearing deps carrier:\n{js}"
    );
    assert_eq!(
        normalized.matches("$.template_effect(").count(),
        1,
        "adjacent text updates must share one template effect:\n{js}"
    );
    assert!(
        !js.contains("$effect.tracking"),
        "no raw rune survives:\n{js}"
    );
    assert!(parses_as_js(&js));
}

#[test]
fn effect_rune_init_var_declaration_still_fails_closed() {
    // A `var`-declared effect-rune init stays OUT of the carrier (official `var`
    // semantics — `$.safe_get` reads — are a distinct surface): the existing
    // var-declaration refusal owns it.
    assert_fail_closed(
        "<script>let c = $state(0); var stop = $effect.root(() => { return () => {}; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "var declaration"),
    );
}

#[test]
fn effect_in_delegated_handler_lowers_with_frame() {
    // The delegated-handler topology (oracle-verified): a well-formed user-effect
    // call statement inside a DELEGATED onclick block arrow is admitted alongside
    // the `$state` writes — `$.delegated('click', button, () => {
    // $.user_effect(...); $.update(c); });` — and the `$effect` forces the frame.
    let js = emit(
        "<script>\n\tlet c = $state(0);\n</script>\n<button onclick={() => { $effect(() => console.log(c)); c++; }}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegated('click', button, () => {"),
        "the handler stays on the delegated path:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(() => console.log($.get(c)));"),
        "the handler-nested `$effect` lowers:\n{js}"
    );
    assert!(
        js.contains("$.update(c);"),
        "the state write beside the effect still lowers:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor, $$props) {")
            && js.contains("$.push($$props, true);")
            && js.contains("$.pop();"),
        "the handler-nested `$effect` forces the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // NEGATIVE (the boundary holds): a plain non-effect call statement in a
    // delegated block arrow still fails closed — the admission is the well-formed
    // user-effect family call, not "any call".
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button onclick={() => { f(c); }}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { event_type, .. } if event_type == "click"),
    );
}

#[test]
fn effect_paren_statement_in_delegated_handler_lowers() {
    // A paren-wrapped `($effect(fn));` statement inside a DELEGATED handler block
    // arrow is the same statement to official (paren-transparent direct-parent
    // rule) — the handler statement gate must admit it, keep the delegated
    // topology, and force the frame (oracle-verified).
    let js = emit(
        "<script>\n\tlet c2 = $state(0);\n</script>\n<button onclick={() => { ($effect(() => { console.log(c2) })); c2++; }}>{c2}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegated('click', button, () => {"),
        "the handler stays on the delegated path:\n{js}"
    );
    assert!(
        js.contains("$.user_effect(() => {"),
        "the paren-wrapped handler-nested `$effect` lowers:\n{js}"
    );
    assert!(
        js.contains("$.update(c2);"),
        "the state write beside the effect still lowers:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);") && js.contains("$.pop();"),
        "the handler-nested `$effect` forces the frame:\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn paren_wrapped_non_family_call_statements_stay_plain() {
    // NEGATIVE CONTROL for the paren-transparent classifier: author parens around
    // a NON-family call statement are not classified as an effect-family form —
    // the call lowers as an ordinary rewritten statement (no helper rewrite), the
    // author parens stay (a behavior-preserving cosmetic the comparator waives),
    // and the component still compiles.
    let js = emit(
        "<script>let x = $state(0); $effect(() => { (console.log(x)); });</script>\n<button onclick={() => x++}>{x}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("(console.log($.get(x)));"),
        "the paren-wrapped plain call stays a plain rewritten call:\n{js}"
    );
    assert_eq!(
        js.matches("$.user_effect(").count(),
        1,
        "exactly the ONE authored effect lowers (the plain call is never classified):\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_root_in_delegated_handler_lowers_without_frame() {
    // A well-formed `$effect.root(...)` STATEMENT inside a DELEGATED onclick
    // block arrow — official lowers it in place inside the `$.delegated`
    // closure with NO component frame (root alone never forces it),
    // oracle-verified: `$.delegated('click', button, () => { $.effect_root(...);
    // $.update(c2); });`.
    let js = emit(
        "<script>let c2 = $state(0);</script>\n<button onclick={() => { $effect.root(() => { return () => {}; }); c2++; }}>{c2}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegated('click', button, () => {"),
        "the handler stays on the delegated path:\n{js}"
    );
    assert!(
        js.contains("$.effect_root(() => {"),
        "the handler-nested root statement lowers in place:\n{js}"
    );
    assert!(
        js.contains("return () => {};"),
        "the cleanup flows through verbatim:\n{js}"
    );
    assert!(
        js.contains("$.update(c2);"),
        "the state write beside the root still lowers:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {") && !js.contains("$.push"),
        "root alone must NOT force the frame (sig `($$anchor)`, no `$.push`):\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The paren-receiver spelling composes (`($effect).root(...)` — official
    // accepts, same emission).
    let js = emit(
        "<script>let c2 = $state(0);</script>\n<button onclick={() => { ($effect).root(() => { return () => {}; }); c2++; }}>{c2}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegated('click', button, () => {") && js.contains("$.effect_root(() => {"),
        "the paren-receiver root handler statement lowers delegated:\n{js}"
    );
    assert!(!js.contains("$.push"), "no frame from root:\n{js}");
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The optional-call spelling composes (`$effect.root?.(...)` — official
    // accepts and normalizes the `?.` away).
    let js = emit(
        "<script>let c2 = $state(0);</script>\n<button onclick={() => { $effect.root?.(() => { return () => {}; }); c2++; }}>{c2}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegated('click', button, () => {") && js.contains("$.effect_root(() => {"),
        "the optional-call root handler statement lowers delegated:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head is normalized away:\n{js}"
    );
    assert!(!js.contains("$.push"), "no frame from root:\n{js}");
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn effect_tracking_in_delegated_handler_lowers_without_frame() {
    // A well-formed `$effect.tracking();` STATEMENT inside a DELEGATED onclick
    // block arrow — official lowers it in place inside the `$.delegated`
    // closure with NO component frame (oracle-verified:
    // `$.delegated('click', button, () => { $.effect_tracking(); $.update(c2); });`).
    let js = emit(
        "<script>let c2 = $state(0);</script>\n<button onclick={() => { $effect.tracking(); c2++; }}>{c2}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegated('click', button, () => {"),
        "the handler stays on the delegated path:\n{js}"
    );
    assert!(
        js.contains("$.effect_tracking();"),
        "the handler-nested tracking statement lowers in place:\n{js}"
    );
    assert!(
        js.contains("$.update(c2);"),
        "the state write beside the tracking call still lowers:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {") && !js.contains("$.push"),
        "tracking alone must NOT force the frame (sig `($$anchor)`, no `$.push`):\n{js}"
    );
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The optional-call spelling composes (`$effect.tracking?.();` — official
    // accepts and normalizes the `?.` away).
    let js = emit(
        "<script>let c2 = $state(0);</script>\n<button onclick={() => { $effect.tracking?.(); c2++; }}>{c2}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegated('click', button, () => {") && js.contains("$.effect_tracking();"),
        "the optional-call tracking handler statement lowers delegated:\n{js}"
    );
    assert!(
        !js.contains("?."),
        "the optional-call head is normalized away:\n{js}"
    );
    assert!(!js.contains("$.push"), "no frame from tracking:\n{js}");
    assert!(!js.contains("$effect"), "no raw rune survives:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn props_id_value_position_fails_closed() {
    // Template value placement (`{$props.id()}`) — official
    // `props_id_invalid_placement`.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<p>{$props.id()}{c}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
    );
}

#[test]
fn props_id_statement_position_fails_closed() {
    // Statement placement (`$props.id();`) — official `props_id_invalid_placement`.
    assert_fail_closed(
        "<script>$props.id();</script>\n<p>x</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
    );
}

#[test]
fn props_id_call_arg_position_fails_closed() {
    // Call-argument placement (`foo($props.id())`) — the init is a `foo(...)`
    // call, not a `$props.id()` declarator init.
    assert_fail_closed(
        "<script>const x = foo($props.id());</script>\n<p>{x}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
    );
}

#[test]
fn props_id_nested_value_position_fails_closed() {
    // A nested value position (`const x = [$props.id()];`) is not a declarator
    // INIT — official `props_id_invalid_placement`.
    assert_fail_closed(
        "<script>const x = [$props.id()];</script>\n<p>{x}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
    );
}

#[test]
fn props_id_function_body_position_fails_closed() {
    // Function-body placement — official `props_id_invalid_placement` (only the
    // component top level is legal).
    assert_fail_closed(
        "<script>function f(){ return $props.id(); }</script>\n<p>x</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
    );
}

#[test]
fn props_id_non_literal_sibling_declarator_fails_closed() {
    // A multi-declarator with a NON-literal sibling init (`const a = foo(), uid =
    // $props.id();`) stays fail-closed — the split carrier admits literal-only
    // sibling declarators.
    assert_fail_closed(
        "<script>const a = foo(), uid = $props.id();</script>\n<p>x</p>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::InstanceScriptItem { .. }
            )
        },
    );
}

#[test]
fn props_bindable_lazy_object_default_mutated_lowers_flag_31_mutation_call() {
    // A deep-MUTATED bindable object default is flag-31, and the member mutation
    // wraps in the setter with the mutation flag (`v(v().a++, true)`). Verified
    // against svelte@5.56.10.
    let src = "<script>let { v = $bindable({ a: 1 }) } = $props();</script>\n<button onclick={() => v.a++}>{v}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 31, () => $.proxy({ a: 1 }));"),
        "a mutated bindable object default is flag-31:\n{js}"
    );
    assert!(
        js.contains("v(v().a++, true)"),
        "a bindable member mutation wraps in the setter with the mutation flag:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn bindable_bare_reference_in_handler_fails_closed() {
    // The same fail-open pin for a TEMPLATE expression: a bare `$bindable` in a
    // handler must fail closed, never emit raw.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button onclick={() => console.log($bindable, c)}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_non_props_destructure_default_fails_closed() {
    // A `$bindable()` default in a destructure of a NON-`$props()` init —
    // official `bindable_invalid_location`.
    assert_fail_closed(
        "<script>let c = $state(0); let { v = $bindable(0) } = foo;</script>\n<p>{c}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_function_param_default_fails_closed() {
    // A `$bindable()` default in a FUNCTION-parameter destructure — official
    // `bindable_invalid_location`.
    assert_fail_closed(
        "<script>let c = $state(0);\nfunction f({ v = $bindable(0) }){ return v; }</script>\n<p>{c}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_template_value_position_fails_closed() {
    // `{$bindable(0)}` in a template value position — official
    // `bindable_invalid_location`.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<p>{$bindable(0)}{c}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_shadowed_handler_param_is_not_a_rune_reference() {
    // A handler PARAMETER named `$bindable` shadows the rune — the call inside is
    // a PLAIN call, NOT a rune. The component still refuses, but on the
    // PRE-EXISTING parametered-delegated-arrow boundary (`NonDelegatedEvent`),
    // never the `$bindable` rune arm — which discriminates the scan's
    // shadow-awareness (a shadow-blind scan would refuse `$bindable` FIRST, since
    // the rune scan runs before the template-shape walk).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button onclick={($bindable) => $bindable(0)}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { event_type, .. } if event_type == "click"),
    );
}

#[test]
fn props_function_expression_default_with_inner_self_read_stays_raw_flag_3() {
    // `let { a = function () { return a; } }` — a FUNCTION-EXPRESSION default
    // is simple over the visited node: the inner prop READ rewrites to the
    // getter inside the body while the initial passes RAW — flags 3 (a read
    // never sets UPDATED). Verified against svelte@5.56.10.
    let src = "<script>let { a = function () { return a; } } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 3, function () { return a(); });"),
        "a function-expression default stays a raw simple initial (flags 3):\n{js}"
    );
    assert!(
        !js.contains("'a', 19,") && !js.contains("=> function"),
        "a function-expression default must not ride the LAZY thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_function_expression_default_with_inner_self_write_stays_raw_flag_7() {
    // `let { a = function () { a = 1; } }` — the deferred write inside the
    // function body sets UPDATED and rewrites through the setter; the
    // function-expression initial stays RAW (flags 7, no LAZY). Verified
    // against svelte@5.56.10.
    let src = "<script>let { a = function () { a = 1; } } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 7, function () { a(1); });"),
        "a written function-expression default stays raw with UPDATED (flags 7):\n{js}"
    );
    assert!(
        !js.contains("'a', 23,") && !js.contains("=> function"),
        "the function-expression default must not set LAZY / ride a thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_conditional_default_with_function_part_stays_raw_flag_7() {
    // `let { a = 1 ? () => (a = 1) : 0 }` — a conditional over SIMPLE parts is
    // itself simple; the arrow PART is opaque-simple (its inner rewrite never
    // changes the part's node kind), so the whole initial stays RAW — flags 7.
    // Verified against svelte@5.56.10.
    let src = "<script>let { a = 1 ? () => (a = 1) : 0 } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 7, 1 ? () => (a(1)) : 0);"),
        "a conditional over simple parts stays a raw initial (flags 7):\n{js}"
    );
    assert!(
        !js.contains("'a', 23,") && !js.contains("7, () => 1 ?"),
        "the simple conditional must not set LAZY / ride a thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_conditional_default_with_unrewritten_leaf_and_function_part_stays_raw_flag_7() {
    // `let { a = undefined ? () => (a = 1) : 0 }` — the identifier leaf
    // `undefined` never rewrites; the arrow part is opaque-simple even though
    // its body carries the setter rewrite: the visited conditional stays
    // simple → RAW, flags 7. Verified against svelte@5.56.10.
    let src =
        "<script>let { a = undefined ? () => (a = 1) : 0 } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 7, undefined ? () => (a(1)) : 0);"),
        "an unrewritten-leaf conditional with a function part stays raw (flags 7):\n{js}"
    );
    assert!(
        !js.contains("'a', 23,"),
        "the unrewritten-leaf conditional must not set LAZY:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_logical_default_with_function_part_stays_raw_flag_7() {
    // `let { a = (() => (a = 1)) || null }` — a logical over SIMPLE parts
    // (arrow, null literal) stays simple over the visited node: RAW initial,
    // flags 7 (the inner write sets UPDATED). Verified against svelte@5.56.10.
    let src = "<script>let { a = (() => (a = 1)) || null } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 7, (() => (a(1))) || null);"),
        "a logical over simple parts stays a raw initial (flags 7):\n{js}"
    );
    assert!(
        !js.contains("'a', 23,") && !js.contains("() => () =>"),
        "the simple logical must not set LAZY / double-thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_conditional_default_with_rewritten_leaf_and_function_part_stays_lazy_flag_19() {
    // CONTROL (green pre-fix): `let { a = b ? () => 1 : 0, b = 0 }` — a
    // function PART cannot rescue a conditional whose leaf `b` rewrites: the
    // visited test is a getter CALL, so the initial rides the LAZY thunk
    // (flags 19). Verified against svelte@5.56.10.
    let src = "<script>let { a = b ? () => 1 : 0, b = 0 } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("a = $.prop($$props, 'a', 19, () => b() ? () => 1 : 0)"),
        "a rewritten-leaf conditional with a function part stays LAZY (flags 19):\n{js}"
    );
    assert!(
        !js.contains("'a', 3,"),
        "the rewritten-leaf conditional must not pass raw:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn paren_wrapped_handler_is_passed_by_reference() {
    // `onclick={(inc)}` — official svelte@5.56.10 ACCEPTS a parenthesized-but-plain
    // handler (classifies `(inc)` as a `FunctionReference` and passes `inc` by
    // reference). The typed handler classifier peels the parentheses while retaining
    // the authored source at emission.
    let js = emit(
        "<script>let n = $state(0); function inc() { n++; }</script>\n<button onclick={(inc)}>{n}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegated('click', button, (inc))"),
        "parenthesized handler reference did not emit by reference:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
    // Discriminating control: the unparenthesized form also compiles and passes the
    // same function by reference.
    let control = emit(
        "<script>let n = $state(0); function inc() { n++; }</script>\n<button onclick={inc}>{n}</button>\n",
        "App.svelte",
    );
    assert!(
        control.contains("$.delegated('click', button, inc)"),
        "the unparenthesized `onclick={{inc}}` control must pass `inc` by reference:\n{control}"
    );
}

#[test]
fn store_accessor_order_is_first_subscription_order_not_declaration_order() {
    // DISCRIMINATING: `{$b}` before `{$a}` (declarations a-then-b) mints the
    // `$b` accessor FIRST — reference order, not declaration order
    // (oracle-verified against svelte@5.56.10).
    let js = emit(
        "<script>import { writable } from 'svelte/store'; const a = writable(1); const b = writable(2);</script>\n<p>{$b}</p>\n<p>{$a}</p>\n",
        "App.svelte",
    );
    let b_pos = js
        .find("const $b = () => $.store_get(b, '$b', $$stores);")
        .expect("the $b accessor is emitted");
    let a_pos = js
        .find("const $a = () => $.store_get(a, '$a', $$stores);")
        .expect("the $a accessor is emitted");
    let setup = js
        .find("$.setup_stores()")
        .expect("the shared setup is emitted");
    assert!(
        b_pos < a_pos && a_pos < setup,
        "accessors are first-subscription-ordered BEFORE one shared setup:\n{js}"
    );
    assert_eq!(
        js.matches("$.setup_stores()").count(),
        1,
        "exactly ONE shared `$.setup_stores()` for multiple stores:\n{js}"
    );
}

#[test]
fn undeclared_store_subscription_stays_the_official_global_reference_reject() {
    // `{$count}` with NO declared `count` is the official
    // `global_reference_invalid` compile error — it must route through the
    // official-reject gate, never the store classifier.
    let src = "<script>let x = 1;</script>\n<p>{$count}</p>\n";
    let alloc = Allocator::default();
    let parsed = parse_svelte(src);
    match compile_client(
        src,
        &parsed,
        &SvelteRuntimeOptions::default(),
        &alloc,
        false,
        false,
    ) {
        Err(ClientCompileError::OfficialReject(rejection)) => {
            assert_eq!(rejection.official_code, "global_reference_invalid");
        }
        other => panic!("an undeclared `$count` must official-reject, got {other:?}"),
    }
}

#[test]
fn arbitrary_const_without_subscription_is_preserved() {
    // The demand boundary: a call-initialized `const x = …` with NO `$x`
    // subscription is NOT a store source — the const gate keeps it fail-closed.
    // (The init calls a GLOBAL so the const itself is the first refused item.)
    let js = emit(
        "<script>const x = JSON.parse('1'); let n = $state(0);</script>\n<button onclick={() => n++}>{n}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const x = JSON.parse('1');"),
        "ordinary call-initialized const missing:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn runes_named_function_handler_is_passed_by_reference() {
    // INVERTED (was the `event_local_function_ident` fail row): a bare-identifier
    // handler naming a top-level function declaration passes the reference
    // through, with the function body rewritten by the shared rewriter
    // (oracle-verified against svelte@5.56.10).
    let js = emit(
        "<script>let c = $state(0); function inc() { c++; }</script>\n<button onclick={inc}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.delegated('click', button, inc);"),
        "the named handler is passed by reference:\n{js}"
    );
    assert!(
        js.contains("function inc() { $.update(c); }"),
        "the handler body rewrites through the shared rewriter:\n{js}"
    );
    // NEGATIVE: no wrapper closure around the reference, and no store machinery.
    assert!(
        !js.contains("() => inc") && !js.contains("setup_stores"),
        "the reference is not wrapped and no store machinery appears:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn nonconst_store_source_declaration_kind_is_supported() {
    // A `$store` base whose top-level declaration is `let` — NOT the single-declarator
    // `const NAME = init` store-source form Verter admits. svelte@5.56.10 ACCEPTS this and
    // emits the `const $c = () => $.store_get(c, '$c', $$stores)` accessor path
    // (oracle-probed against svelte@5.56.10, 2026-07-06). Verter admits a store SOURCE only
    // as a single-declarator `const`, so the `let` declaration falls through to the
    // instance-script `plain let with call init` gate and fails CLOSED — never an admitted
    // (mis-emitted) subscription. `assert_fail_closed` panics on ANY emitted module, so it
    // doubles as the negative "no subscription mis-emitted" assertion. This declaration-KIND
    // completeness gap fails closed; the follow-up is tracked in
    // `.claude/skills/compiler-codegen/SKILL.md`.
    let js = emit(
        "<script>import { writable } from 'svelte/store'; let c = writable(0);</script>\n<p>{$c}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let c = writable(0);"),
        "store source missing:\n{js}"
    );
    assert!(
        js.contains("const $c = () => $.store_get(c, '$c', $$stores);"),
        "store accessor missing:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn in_source_runes_option_overrides_the_runes_compile_option() {
    // Official svelte@5.56.10: an in-source `<svelte:options runes={…}>` wins
    // over the caller's `runes` compile option in BOTH directions (oracle-
    // adjudicated). The `export let` legacy surface discriminates the mode.

    // Compile option FALSE + in-source runes={true}: the component IS runes
    // mode — an `export let` is the official legacy_export_invalid reject.
    let err = emit_result_with_runes(
        "<svelte:options runes={true} />\n<script>export let foo = 1;</script>\n<p>{foo}</p>\n",
        Some(false),
    )
    .expect_err("in-source runes={true} must override the runes:false compile option");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "legacy_export_invalid"),
        "in-source runes={{true}} over runes:false must reject the legacy export: {err:?}"
    );

    // Compile option TRUE + in-source runes={false}: the component IS legacy
    // mode — the SAME `export let` compiles through the legacy prop source.
    let js = emit_result_with_runes(
        "<svelte:options runes={false} />\n<script>export let foo = 1;</script>\n<p>{foo}</p>\n",
        Some(true),
    )
    .unwrap_or_else(|e| {
        panic!("in-source runes={{false}} must override the runes:true compile option: {e:?}")
    });
    assert!(
        js.contains("let foo = $.prop($$props, 'foo', 8, 1);"),
        "the forced-legacy component lowers `export let` as the legacy prop source:\n{js}"
    );
    // NEGATIVE: no runes-mode flags import in the forced-legacy module.
    assert!(
        js.contains("import 'svelte/internal/flags/legacy';"),
        "the forced-legacy module carries the legacy flags import:\n{js}"
    );

    // SANITY (option-only, no in-source directive): the compile option still
    // decides the mode on its own — runes:false compiles the legacy export,
    // runes:true rejects it.
    let js = emit_result_with_runes(
        "<script>export let foo = 1;</script>\n<p>{foo}</p>\n",
        Some(false),
    )
    .expect("runes:false with no in-source directive compiles legacy");
    assert!(
        js.contains("let foo = $.prop($$props, 'foo', 8, 1);"),
        "runes:false alone keeps the legacy lowering:\n{js}"
    );
    let err = emit_result_with_runes(
        "<script>export let foo = 1;</script>\n<p>{foo}</p>\n",
        Some(true),
    )
    .expect_err("runes:true with no in-source directive is runes mode");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "legacy_export_invalid"),
        "runes:true alone must reject the legacy export: {err:?}"
    );
}

#[test]
fn legacy_export_let_multiple_props_emit_one_declaration_each() {
    // Multiple `export let` statements lower to ONE `let <local> = $.prop(...);`
    // per prop, in source order (oracle-verified: official splits per declarator).
    let js = emit(
        "<script>\nexport let a;\nexport let b = 1;\n</script>\n<p>{a}</p>\n<p>{b}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let a = $.prop($$props, 'a', 8);"),
        "first prop declaration:\n{js}"
    );
    assert!(
        js.contains("let b = $.prop($$props, 'b', 8, 1);"),
        "second prop declaration with default:\n{js}"
    );
    let a_pos = js.find("let a = $.prop").unwrap();
    let b_pos = js.find("let b = $.prop").unwrap();
    assert!(a_pos < b_pos, "prop declarations keep source order:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_let_promotes_to_mutable_source_on_handler_write() {
    // A top-level legacy `let` written by a template handler and read in the
    // template promotes to a `$.mutable_source` signal: reads `$.get`, updates
    // `$.update`, and the component takes NO `$$props` and NO frame
    // (oracle-verified).
    let js = emit(
        "<script>\nlet count = 0;\n</script>\n<button onclick={() => count++}>{count}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let count = $.mutable_source(0);"),
        "a written legacy let promotes to $.mutable_source:\n{js}"
    );
    assert!(
        js.contains("$.update(count)"),
        "the increment lowers through $.update:\n{js}"
    );
    assert!(
        js.contains("$.get(count)"),
        "the template read lowers through $.get:\n{js}"
    );
    assert!(
        js.contains("($$anchor)") && !js.contains("$$props"),
        "a promoted let threads no $$props:\n{js}"
    );
    assert!(
        !js.contains("$.push") && !js.contains("$.pop"),
        "a promoted let opens no component context frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_let_promotes_via_function_body_write() {
    // The write may live in an admitted instance-script FUNCTION body (an
    // `onclick={inc}` referent): the compound assign rewrites through the shared
    // rewriter (`$.set(count, $.get(count) + 1)`) — oracle-verified.
    let js = emit(
        "<script>\nlet count = 0;\nfunction inc() { count += 1; }\n</script>\n<button onclick={inc}>{count}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let count = $.mutable_source(0);"),
        "a function-body-written let promotes:\n{js}"
    );
    assert!(
        js.contains("$.set(count, $.get(count) + 1)"),
        "the compound assign lowers through $.set over $.get:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_member_mutation_handler_wraps_in_mutate() {
    // A member mutation in a handler (`o.x++`) promotes the root and wraps the
    // whole update in `$.mutate(o, $.get(o).x++)` (oracle-verified).
    let js = emit(
        "<script>\nlet o = { x: 0 };\n</script>\n<button onclick={() => o.x++}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let o = $.mutable_source({ x: 0 });"),
        "a member-mutated let promotes:\n{js}"
    );
    assert!(
        js.contains("$.mutate(o, $.get(o).x++)"),
        "the member update wraps in $.mutate:\n{js}"
    );
    // NEGATIVE: no bare (unwrapped) member update on the raw binding.
    assert!(
        !js.contains("o.x++"),
        "the raw member update must not survive:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn reactive_expression_statement_wraps_verbatim() {
    // `$: console.log(x);` — a non-assignment expression statement takes the
    // wrap-verbatim shape: no synthesized declaration, the whole statement is
    // the effect body.
    let js = emit(
        "<script>let x = 0; $: console.log(x);</script>\n<p>{x}</p>\n<button onclick={() => x++}>b</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.legacy_pre_effect(() => ($.get(x)), () => { console.log($.get(x)); });"),
        "the expression statement wraps verbatim:\n{js}"
    );
    assert_eq!(
        js.matches("$.mutable_source").count(),
        1,
        "only the written `let x` mints a cell:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_onclick_prop_is_a_slot_property_not_a_dom_event() {
    // An `onclick={f}` ATTRIBUTE on a `<slot>` is the slot prop `onclick`
    // (official `SlotElement` treats it as a plain Attribute), NEVER a DOM event.
    let js = emit(
        "<script>export let f;</script>\n<div><slot onclick={f} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("get onclick() {") && js.contains("return f();"),
        "the onclick slot prop is a getter member:\n{js}"
    );
    // NEGATIVE: no event registration, no delegation epilogue.
    assert!(
        !js.contains("$.delegated(") && !js.contains("$.event(") && !js.contains("$.delegate(["),
        "a slot onclick prop must not register a DOM event:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_fallback_region_emits_an_anchor_callback_with_postorder_hoist() {
    // `<slot><p>fb</p></slot>`: the non-empty fallback visits as its OWN
    // `($$anchor) => { … }` region — the fallback template hoists BEFORE the
    // parent's (post-order: `root` = `<p>fb</p>`, `root_1` = the parent div).
    let js = emit("<div><slot><p>fb</p></slot></div>\n", "App.svelte");
    assert!(
        js.contains("var root = $.from_html(`<p>fb</p>`);")
            && js.contains("var root_1 = $.from_html(`<div><!></div>`);"),
        "the fallback template hoists before the parent (post-order):\n{js}"
    );
    assert!(
        js.contains("$.slot(node, $$props, 'default', {}, ($$anchor) => {"),
        "the fallback is an anchor callback:\n{js}"
    );
    assert!(
        js.contains("var p = root();") && js.contains("$.append($$anchor, p);"),
        "the fallback region clones + mounts its own template:\n{js}"
    );
    // NEGATIVE: the fallback is NOT `null` and NOT inlined into the skeleton.
    assert!(
        !js.contains(", null);") && !js.contains("<p>fb</p></div>"),
        "a non-empty fallback is never null / never skeleton-inlined:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_before_trailing_static_sibling_advances_the_hydration_cursor() {
    // `<div><slot name="n"><p>fb</p></slot><span>after</span></div>`: the slot is
    // the last NAMED position; the trailing static `<span>` advances the cursor
    // (`$.next();`) before `$.reset(div)` — oracle-pinned svelte@5.56.10.
    let js = emit(
        "<div><slot name=\"n\"><p>fb</p></slot><span>after</span></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.next();"),
        "the trailing static sibling advances the hydration cursor:\n{js}"
    );
    assert!(
        js.contains("$.reset(div);"),
        "the parent resets after its child walk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_wrap_deps_include_nested_function_captures() {
    // Oracle parity (svelte@5.56.10): nested fn/arrow bodies do NOT set the sync
    // wrap trigger, but their free bindings REMAIN `metadata.references`
    // dependencies when another part of the expression triggers the wrap.
    // `<Comp foo={(a.x, () => b.y)} />` — the sync member `a.x` triggers; the
    // arrow-captured `b` still joins the visible dep reads. Oracle:
    //   ($.deep_read_state(a()), $.deep_read_state(b()), $.untrack(() => (a().x, () => b().y)))
    let js = emit(
        "<script>import Child from './Child.svelte';\nexport let a;\nexport let b;</script>\n<Child foo={(a.x, () => b.y)} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.deep_read_state(a()), $.deep_read_state(b()), $.untrack("),
        "the wrap deps include the arrow-captured `b` after the sync `a.x` trigger, \
         in first-reference source order:\n{js}"
    );
    assert!(
        js.contains("a().x, () => b().y"),
        "the untracked payload keeps the authored sequence:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn explicit_runes_false_option_applies_legacy_wrap() {
    // `<svelte:options runes={false} />` FORCES definite-legacy (official
    // `runes_option !== false` gate on maybe-runes), so the wrap applies even
    // with no `export let` / `$:` (oracle):
    //   let $0 = $.derived_safe_equal(() => ($.deep_read_state(fn), $.untrack(() => fn(1))));
    let js = emit(
        "<svelte:options runes={false} />\n<script>import { fn } from './x.js';</script>\n<div><slot foo={fn(1)} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "let $0 = $.derived_safe_equal(() => ($.deep_read_state(fn), $.untrack(() => fn(1))));"
        ),
        "the forced-legacy component wraps (the import deep-reads):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_dom_attr_non_call_member_wraps_inline() {
    // A NON-call member attribute value wraps INLINE (wrap precedes the
    // memoize decision — official applies `build_expression` BEFORE
    // `Memoizer.add`, and a no-call value is never memoized). Oracle:
    //   $.template_effect(() => $.set_attribute(div, 'title',
    //     ($.deep_read_state(obj()), $.untrack(() => obj().x))));
    let js = emit(
        "<script>export let obj;</script>\n<div title={obj.x}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "$.set_attribute(div, 'title', ($.deep_read_state(obj()), $.untrack(() => obj().x)))"
        ),
        "the member value wraps inline in the write:\n{js}"
    );
    assert!(
        !js.contains("$0"),
        "a non-call value never memoizes into a deps slot:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_non_reactive_attr_member_write_wraps_inline_untracked() {
    // A NON-reactive (stateless) member value still wraps at the init write —
    // official applies `build_expression` regardless of reactivity; an
    // unresolved global root contributes no dep read (oracle):
    //   $.set_attribute(div, 'title', ($.untrack(() => globalObj.x)));
    let js = emit(
        "<script>export let p;</script>\n<div title={globalObj.x}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.set_attribute(div, 'title', ($.untrack(() => globalObj.x)))"),
        "the init member write wraps untracked with empty deps:\n{js}"
    );
    assert!(
        !js.contains("$.deep_read_state"),
        "an unresolved global never joins the deps:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn attribute_effect_event_handler_arrow_hoists_stable_id_identifier_stays_inline() {
    // The attribute-effect handler-stability rule on its reachable
    // `<svelte:element>` host (the regular-spread modern-event surface is a
    // pre-existing classifier fail-closed refusal, not a wrap fail-open).
    // Oracle: var event_handler = () => obj().m();
    //   $.attribute_effect($$element, ($0) => ({ onclick: event_handler, title: $0 }), [() => (wrap)]);
    // An IDENTIFIER handler stays inline (`onclick: fn`) — no hoist, no memo.
    let js = emit(
        &format!("{LEGACY_OBJ}<svelte:element this={{'div'}} onclick={{() => obj.m()}} title={{obj.m()}}></svelte:element>\n"),
        "App.svelte",
    );
    assert!(
        js.contains("var event_handler = () => obj().m();"),
        "an arrow event-attribute handler hoists to a stable id:\n{js}"
    );
    assert!(
        js.contains("onclick: event_handler"),
        "the fold references the hoisted handler by name:\n{js}"
    );
    let js2 = emit(
        "<script>import { fn } from './x.js';</script>\n<svelte:element this={'div'} onclick={fn} title=\"t\"></svelte:element>\n",
        "App.svelte",
    );
    assert!(
        js2.contains("onclick: fn") && !js2.contains("event_handler"),
        "an identifier handler stays inline without a hoist:\n{js2}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_if_call_condition_gains_outer_derived() {
    // Oracle: var d = $.derived(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));
    //         $.if(node, ($$render) => { if ($.get(d)) $$render(consequent); });
    let js = emit(
        &format!("{LEGACY_OBJ}{{#if obj.m()}}<p>a</p>{{/if}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "var d = $.derived(() => ({}));",
            obj_wrap("obj().m()")
        )),
        "the call-bearing test hoists the wrapped $.derived:\n{js}"
    );
    assert!(
        js.contains("if ($.get(d)) $$render(consequent);"),
        "the test reads $.get(d):\n{js}"
    );
    // NEGATIVE: the raw tracked call must not survive as the test, and the
    // condition derived is `$.derived` (mode-independent), never safe_equal.
    assert!(
        !js.contains("if (obj().m())"),
        "no raw tracked test in definite legacy:\n{js}"
    );
    assert!(
        !js.contains("$.derived_safe_equal(() => ($.deep_read_state"),
        "the condition derived is plain $.derived, not safe_equal:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_if_member_condition_wraps_inline() {
    // Oracle: if (($.deep_read_state(obj()), $.untrack(() => obj().x))) $$render(consequent);
    let js = emit(
        &format!("{LEGACY_OBJ}{{#if obj.x}}<p>a</p>{{/if}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "if (({})) $$render(consequent);",
            obj_wrap("obj().x")
        )),
        "the member-only test wraps inline (no derived):\n{js}"
    );
    assert!(
        !js.contains("$.derived"),
        "a member-only test never hoists a derived:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn maybe_runes_if_call_condition_gains_underived_raw() {
    // The condition $.derived is UNCONDITIONAL on mode; the wrap is not.
    // Oracle (maybe-runes): var d = $.derived(() => $s().m());
    let js = emit(
        "<script>import { writable } from 'svelte/store';\nconst s = writable({});</script>\n{#if $s.m()}<p>a</p>{/if}\n",
        "App.svelte",
    );
    assert!(
        js.contains("var d = $.derived(() => ($s().m()));")
            || js.contains("var d = $.derived(() => $s().m());"),
        "the maybe-runes call test hoists the RAW $.derived (cosmetic parens waived):\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "the legacy wrap never applies in maybe-runes mode:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_if_call_condition_gains_underived_raw() {
    // Oracle (runes): var d = $.derived(() => $$props.obj.m());
    let js = emit(
        "<script>let { obj } = $props();</script>\n{#if obj.m()}<p>a</p>{/if}\n",
        "App.svelte",
    );
    assert!(
        js.contains("var d = $.derived(() => ($$props.obj.m()));")
            || js.contains("var d = $.derived(() => $$props.obj.m());"),
        "the runes call test hoists the RAW $.derived (cosmetic parens waived):\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "no legacy wrap machinery in a runes module:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_if_assignment_condition_wraps_inline() {
    // Oracle: if (($.deep_read_state(obj()), $.untrack(() => obj(obj().x = 1, true)))) …
    let js = emit(
        &format!("{LEGACY_OBJ}{{#if (obj.x = 1)}}<p>a</p>{{/if}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains("$.deep_read_state(obj()), $.untrack(() => "),
        "the assignment-bearing test wraps:\n{js}"
    );
    assert!(
        !js.contains("$.derived"),
        "an assignment-only test never memoizes:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_each_collection_call_wraps_thunk() {
    // Oracle: $.each(node, 1, () => ($.deep_read_state(obj()), $.untrack(() => obj().m())), $.index, …
    let js = emit(
        &format!("{LEGACY_OBJ}{{#each obj.m() as item}}<p>{{item}}</p>{{/each}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "$.each(node, 1, () => ({}), $.index",
            obj_wrap("obj().m()")
        )),
        "the each collection wraps inside its thunk:\n{js}"
    );
    assert!(
        !js.contains("() => obj().m(), $.index"),
        "no raw tracked collection thunk in definite legacy:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_each_keyed_key_stays_raw_collection_wraps() {
    // Oracle: $.each(node, 1, () => (wrap), (item) => item.id, …
    let js = emit(
        &format!("{LEGACY_OBJ}{{#each obj.m() as item (item.id)}}<p>{{item}}</p>{{/each}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains("(item) => item.id"),
        "the keyed-each key callback stays raw:\n{js}"
    );
    assert!(
        js.contains(&format!("() => ({})", obj_wrap("obj().m()"))),
        "the keyed collection still wraps:\n{js}"
    );
    assert!(
        !js.contains("(item) => ($.deep_read_state"),
        "the key expression is never legacy-wrapped:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn runes_each_collection_call_stays_raw() {
    // Oracle (runes): $.each(node, 17, () => $$props.obj.m(), $.index, …
    let js = emit(
        "<script>let { obj } = $props();</script>\n{#each obj.m() as item}<p>{item}</p>{/each}\n",
        "App.svelte",
    );
    assert!(
        js.contains("() => $$props.obj.m(), $.index"),
        "the runes collection thunk stays raw:\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "no legacy wrap machinery in a runes module:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn each_item_reactivity_matches_the_official_predicate_on_every_axis() {
    // The official `EACH_ITEM_REACTIVE` rule has FOUR independent inputs — does
    // the collection depend on any declared binding, is the component in runes
    // mode, is the key the item binding itself, and does the collection
    // subscribe a store. Each row below was measured against the pinned
    // official compiler through the conformance harness's own
    // `compileSvelteFixture`, so these are the official values, not Verter's.
    //
    // SCOPE: this asserts the FLAGS ARGUMENT and nothing else. A row's flags
    // agreeing with official is NOT a claim that the whole emitted program
    // does — the single-name-destructure row is exactly such a case, and its
    // independent emit divergence is characterized by
    // `a_single_name_destructure_each_binds_the_item_not_its_field`. The
    // flag/read-form COUPLING is pinned separately by
    // `the_each_item_flag_and_its_read_form_move_together`.
    //
    // DISCRIMINATION, measured rather than asserted: restoring the superseded
    // predicate ("the item binding is a signal kind", which is TRUE for every
    // `{#each}` item and therefore always ORs the bit in) turns rows 1, 2 and 5
    // RED — 17/25/17 against 16/24/16. The remaining rows already expect the
    // bit, which is exactly why the matrix needs both polarities: rows 1/2/5
    // catch an over-eager predicate, rows 3/4/9 catch a key-shape reading that
    // is too permissive, row 6 catches a lost runes distinction, row 7 catches
    // a lost store distinction, row 5 catches a lost dependency check, and
    // row 8 catches a dependency set that wrongly includes expression-local
    // bindings.
    const CASES: &[(&str, &str, u32)] = &[
        // 1. runes, keyed, key IS the item, one external dependency.
        //    EACH_ITEM_IMMUTABLE only.
        (
            "runes/keyed/key-is-item",
            "<script>\n  let items = $state(['a']);\n</script>\n{#each items as item (item)}<li>{item}</li>{/each}\n",
            16,
        ),
        // 2. Same, plus an `animate:` directive and a ternary collection.
        //    EACH_ITEM_IMMUTABLE | EACH_IS_ANIMATED.
        (
            "runes/keyed/key-is-item/animated",
            "<script>\n\tlet { fx } = $props();\n\tlet flipped = $state(false);\n</script>\n\n<button onclick={() => flipped = !flipped}>swap</button>\n{#each (flipped ? ['b', 'a'] : ['a', 'b']) as item (item)}\n\t<p animate:fx>{item}</p>\n{/each}\n",
            24,
        ),
        // 3. runes, keyed, key is NOT the item (`item.id`) — the item is reactive.
        (
            "runes/keyed/key-is-not-item",
            "<script>\n  let items = $state([{id:1}]);\n</script>\n{#each items as item (item.id)}<li>{item.id}</li>{/each}\n",
            17,
        ),
        // 4. runes, UNKEYED — there is no key, so the key is not the item.
        (
            "runes/unkeyed",
            "<script>\n  let items = $state(['a']);\n</script>\n{#each items as item}<li>{item}</li>{/each}\n",
            17,
        ),
        // 5. runes, keyed, key IS the item, but the collection is a LITERAL with
        //    no declared dependency at all — official's dependency loop never
        //    runs, so the bit stays clear.
        (
            "runes/keyed/literal-collection",
            "<script>\n  let n = $state(0);\n</script>\n<p>{n}</p>\n{#each ['a','b'] as item (item)}<li>{item}</li>{/each}\n",
            16,
        ),
        // 6. LEGACY, keyed, key IS the item — non-runes always sets the bit, and
        //    never sets EACH_ITEM_IMMUTABLE.
        (
            "legacy/keyed/key-is-item",
            "<script>\n  export let items = [];\n</script>\n{#each items as item (item)}<li>{item}</li>{/each}\n",
            1,
        ),
        // 7. A STORE subscription in the collection — the bit is set even though
        //    the key is the item.
        (
            "store/keyed/key-is-item",
            "<script>\n  import { writable } from 'svelte/store';\n  const s = writable(['a']);\n</script>\n{#each $s as item (item)}<li>{item}</li>{/each}\n",
            1,
        ),
        // 8. runes, keyed, key IS the item, and the collection's ONLY binding is
        //    EXPRESSION-LOCAL (an arrow parameter). Official records it as a
        //    dependency and then SKIPS it by function depth, so the bit stays
        //    clear; Verter never sees it because expression-local names are
        //    removed from the analyzed reference set. Same answer, and this row
        //    is what proves the two routes agree.
        (
            "runes/keyed/expression-local-dependency",
            "<script>\n  let n = $state(0);\n</script>\n<p>{n}</p>\n{#each ((x) => [x])(1) as item (item)}<li>{item}</li>{/each}\n",
            16,
        ),
        // 9. runes, keyed, key IS the item but written through a TYPESCRIPT-only
        //    wrapper. Official erases TypeScript before the transform, so
        //    `(item!)` is the identifier `item` when it decides `key_is_item`.
        (
            "runes/keyed/ts-non-null-key",
            "<script lang=\"ts\">\n  let items = $state(['a']);\n</script>\n{#each items as item (item!)}<li>{item}</li>{/each}\n",
            16,
        ),
        // 10. Same, through a TS `as` assertion.
        (
            "runes/keyed/ts-as-key",
            "<script lang=\"ts\">\n  let items = $state(['a']);\n</script>\n{#each items as item (item as string)}<li>{item}</li>{/each}\n",
            16,
        ),
        // 11. Same, through a TS instantiation expression and a `satisfies`.
        //     Together with rows 9-10 these are the FIVE TypeScript-only wrapper
        //     forms official erases before the transform runs, so the set is
        //     covered rather than sampled.
        (
            "runes/keyed/ts-instantiation-key",
            "<script lang=\"ts\">\n  let items = $state(['a']);\n</script>\n{#each items as item (item<string>)}<li>{item}</li>{/each}\n",
            16,
        ),
        (
            "runes/keyed/ts-satisfies-key",
            "<script lang=\"ts\">\n  let items = $state(['a']);\n</script>\n{#each items as item (item satisfies string)}<li>{item}</li>{/each}\n",
            16,
        ),
        // 12. runes, keyed, but the each CONTEXT is a single-name DESTRUCTURE —
        //    not an identifier context, so the key is not the item. This row is
        //    what makes the name-count reading wrong: `{ id }` declares exactly
        //    one name and is still not a bare identifier.
        (
            "runes/keyed/destructured-context",
            "<script>\n  let items = $state([{id:1}]);\n</script>\n{#each items as { id } (id)}<li>{id}</li>{/each}\n",
            17,
        ),
    ];
    let mut wrong = Vec::new();
    for (label, source, official) in CASES {
        let js = emit(source, "App.svelte");
        let flags = each_flags(&js);
        if flags != *official {
            wrong.push(format!(
                "{label}: emitted {flags}, official emits {official}"
            ));
        }
        assert!(parses_as_js(&js), "{label}: module must be valid JS:\n{js}");
    }
    assert!(
        wrong.is_empty(),
        "the `$.each` flags diverge from the pinned official compiler:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn a_non_reactive_each_item_is_still_not_a_writable_bind_root() {
    // The item-reactivity demotion changes how an each item is READ, never
    // whether it may be WRITTEN. An each item is not an assignment target in
    // either state: official redirects a written item through
    // `collection[$$index]` rather than assigning the render-callback
    // parameter, so a `bind:` to it must keep failing closed instead of
    // emitting a setter that writes the parameter and never reaches the
    // collection.
    //
    // The collection here is a GLOBAL, so the item is non-reactive and IS
    // demoted — the exact state in which a plain-local reading of the binding
    // would wrongly admit the bind.
    assert_fail_closed(
        "<svelte:options runes={false}/>\n{#each globalThis.items as item (item)}<input bind:value={item}/>{/each}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
    // And the reactive item — not demoted — is refused by the same gate, so the
    // two states agree.
    assert_fail_closed(
        "<script>\n  let items = $state([{id:1}]);\n</script>\n{#each items as item (item.id)}<input bind:value={item}/>{/each}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn a_non_ascii_identifier_compiles_instead_of_panicking() {
    let js = emit(
        "<script>\n  let \u{441}\u{447}\u{451}\u{442} = $state(0);\n</script>\n<button onclick={() => \u{441}\u{447}\u{451}\u{442} += 1}>{\u{441}\u{447}\u{451}\u{442}}</button>\n",
        "App.svelte",
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn each_item_immutable_clears_when_a_runes_collection_subscribes_a_store() {
    // Measured against the pinned official compiler through the conformance
    // harness: this source emits `$.each(node, 1, …)` — EACH_ITEM_REACTIVE
    // alone, with EACH_ITEM_IMMUTABLE CLEARED because the collection reads a
    // store. Verter emits 17 (it ORs EACH_ITEM_IMMUTABLE in from
    // `mode == Runes` without consulting the store fact).
    //
    // Discriminating in both directions: it also fails if the store fact starts
    // clearing the bit for a runes component that does NOT read a store, which
    // the sibling matrix test pins at 16.
    let js = emit(
        "<script>\n  import { writable } from 'svelte/store';\n  const s = writable(['a']);\n  let n = $state(0);\n</script>\n<p>{n}</p>\n{#each $s as item (item)}<li>{item}</li>{/each}\n",
        "App.svelte",
    );
    assert_eq!(
        each_flags(&js),
        1,
        "a runes component whose each collection subscribes a store must not carry \
         EACH_ITEM_IMMUTABLE:\n{js}"
    );
}

#[test]
fn legacy_key_expression_call_wraps_thunk() {
    // Oracle: $.key(node, () => ($.deep_read_state(obj()), $.untrack(() => obj().m())), ($$anchor) => …
    let js = emit(
        &format!("{LEGACY_OBJ}{{#key obj.m()}}<p>a</p>{{/key}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("$.key(node, () => ({})", obj_wrap("obj().m()"))),
        "the key expression wraps inside its thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_const_plain_item_expression_stays_unwrapped_safe_equal() {
    // No call/member/assignment trigger: safe_equal helper, NO wrap.
    // Oracle: const y = $.derived_safe_equal(() => 1 + 2);
    let js = emit(
        &format!("{LEGACY_OBJ}{{#each [1] as item}}{{@const y = 1 + 2}}<p>{{y}}</p>{{/each}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains("const y = $.derived_safe_equal(() => (1 + 2));")
            || js.contains("const y = $.derived_safe_equal(() => 1 + 2);"),
        "an untriggered legacy {{@const}} stays unwrapped under safe_equal (cosmetic parens waived):\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "no wrap without a call/member/assignment trigger:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_declaration_tag_initializer_stays_raw() {
    // Control: a `{const}` declaration-tag initializer is RAW and inert.
    // Oracle: const y = obj().m();
    let js = emit(
        &format!("{LEGACY_OBJ}{{#each [1] as item}}{{const y = obj.m()}}x{{/each}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains("const y = obj().m();"),
        "the declaration-tag initializer stays raw and inert:\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "a declaration-tag initializer is never legacy-wrapped:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_use_action_arg_stays_raw() {
    // Control: `use:` arguments are RAW (official visits them without
    // build_expression). Oracle: $.action(div, ($$node, $$action_arg) => act?.($$node, $$action_arg), () => obj().m())
    let js = emit(
        "<script>import { act } from './x.js';\nexport let obj;</script>\n<div use:act={obj.m()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("() => obj().m())") || js.contains("() => (obj().m()))"),
        "the action argument thunk stays raw (cosmetic parens waived):\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "a use: argument is never legacy-wrapped:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}
