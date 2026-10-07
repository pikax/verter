use super::*;

#[test]
fn snippet_inside_if_block_emits_a_block_local_const() {
    // A `{#snippet}` DECLARATION inside a (supported) `{#if}` body is the snippet surface —
    // it emits a BLOCK-LOCAL `const foo = ($$anchor, …) => {…}` inside the consequent region
    // (the official `context.state.snippets`).
    let js = emit_result(
        "<script>let on = $state(true);</script>\n{#if on}{#snippet foo()}<p>x</p>{/snippet}{/if}\n",
    )
    .expect("a {#snippet} inside a supported {#if} block emits a module");
    assert!(js.contains("$.if("), "missing the {{#if}} block:\n{js}");
    assert!(
        js.contains("const foo = ($$anchor"),
        "missing the block-local snippet const:\n{js}"
    );
    // NEGATIVE: the snippet declaration must NOT refuse (it is no longer the closed surface).
    assert!(
        !js.contains("svelte-runtime-unsupported"),
        "a {{#snippet}} in a block body must not refuse:\n{js}"
    );
}

#[test]
fn render_inside_each_block_emits_a_dynamic_snippet_call() {
    // A `{@render}` tag inside a (supported) `{#each}` body is the render surface — a
    // dynamic-callee render emits `$.snippet(node, () => foo, …)` per item.
    let js = emit_result(
        "<script>let { items } = $props();</script>\n{#each items as x}{@render foo(x)}{/each}\n",
    )
    .expect("a {@render} inside a supported {#each} block emits a module");
    assert!(js.contains("$.each("), "missing the {{#each}} block:\n{js}");
    assert!(
        js.contains("$.snippet("),
        "missing the dynamic-render $.snippet call:\n{js}"
    );
}

#[test]
fn each_block_emits_supported_surface() {
    // The `{#each}` block IS supported: a `$props()`-sourced array iterated with a
    // reactive item body emits `$.each(...)` — NOT a fail-closed block refusal.
    let js = emit_result(
        "<script>let { items } = $props();</script>\n{#each items as x}<p>{x}</p>{/each}\n",
    )
    .expect("a supported {#each} block emits a module");
    assert!(
        js.contains("$.each("),
        "the each block lowers to `$.each(...)`:\n{js}"
    );
    assert!(
        js.contains("$.get(x)"),
        "the each ITEM is a signal (`$.get(x)`):\n{js}"
    );
}

#[test]
fn await_block_emits_supported_surface() {
    // The `{#await}` block IS supported: a `$props()`-sourced promise emits `$.await`, and
    // the THEN branch reactively reads the resolved value (`$.get(...)`) — not a static
    // textContent write. The pending slot is ABSENT here (a then-only `{#await p then v}`),
    // so the pending sentinel is `null`, and the then closure is PRESENT (NOT the `void 0`
    // missing-then sentinel).
    let js = emit_result(
        "<script>let { p } = $props();</script>\n{#await p then v}<p>{v}</p>{/await}\n",
    )
    .expect("a supported {#await} block emits a module");
    assert!(
        js.contains("$.await("),
        "the await block lowers to `$.await(...)`:\n{js}"
    );
    assert!(
        js.contains("$.get("),
        "the then branch reactively reads the resolved value (`$.get(...)`):\n{js}"
    );
    assert!(
        !js.contains("void 0"),
        "a then-PRESENT await carries a real then closure, never the `void 0` missing-then \
         sentinel:\n{js}"
    );
}

#[test]
fn await_catch_only_emits_void_zero_missing_then_sentinel() {
    // A CATCH-ONLY `{#await p}{:catch e}…{/await}` (no `then`): the then slot is ABSENT but
    // FOLLOWED by a catch, so official emits the `void 0` missing-then sentinel (distinct
    // from the absent-PENDING `null`), an EMPTY pending arrow `($$anchor) => {}` (the
    // present-but-content-free pending region), and the catch closure. This pins the
    // then-before-catch sentinel — emitting `null` here would mis-slot the catch.
    let js = emit_result(
        "<script>let { p } = $props();</script>\n{#await p}{:catch e}<p>oops</p>{/await}\n",
    )
    .expect("a supported catch-only {#await} block emits a module");
    assert!(
        js.contains("$.await("),
        "the catch-only await block lowers to `$.await(...)`:\n{js}"
    );
    assert!(
        js.contains("void 0"),
        "a then-absent-but-catch-present await emits the `void 0` missing-then sentinel:\n{js}"
    );
    assert!(
        js.contains("($$anchor) => {}"),
        "the present-but-empty pending region is an empty arrow `($$anchor) => {{}}`:\n{js}"
    );
}

#[test]
fn html_sibling_reaches_its_own_comment_anchor_without_the_reset_third_arg() {
    // A `{@html}` with a text sibling reaches its OWN `<!>` anchor (NOT the only-child
    // form): `var node = $.sibling($.child(div)); $.html(node, () => h);` (NO third arg),
    // and the `<!>` placeholder is injected into the template.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div>before {@html h} after</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.from_html(`<div>before <!> after</div>`)")),
        "a sibling {{@html}} must inject a <!> placeholder into the template:\n{js}"
    );
    assert!(
        n.contains(&nc("$.html(node, () => h)")),
        "a sibling {{@html}} must reach its own anchor with no third arg:\n{js}"
    );
    // NEGATIVE: the sibling form must NOT use the only-child third-arg / parent target.
    assert!(
        !n.contains(&nc("$.html(node, () => h, true)")),
        "the sibling form must not carry the only-child third arg:\n{js}"
    );
}

#[test]
fn html_inside_if_block_emits_into_the_branch_region() {
    // A `{@html}` INSIDE an `{#if}` block (both supported: `{@html}` is the raw-markup tag,
    // `{#if}` a control-flow block) emits its `$.html(...)` into the BRANCH region — the
    // per-region op routing assigns the `{@html}` op to its owning block-body scope, NOT the
    // root region.
    let js = emit_result(
        "<script>let h = $state('<b>x</b>'); let on = $state(true);</script>\n{#if on}{@html h}{/if}\n",
    )
    .expect("a {@html} inside a supported {#if} block emits a module");
    let if_at = js
        .find("$.if(")
        .expect("the if block lowers to `$.if(...)`");
    let html_at = js
        .find("$.html(")
        .expect("the branch `{@html}` lowers to `$.html(...)`");
    // STRUCTURAL proof the `$.html` is in the BRANCH region, not the root: the branch's
    // consequent closure (which CONTAINS the `$.html`) is emitted BEFORE the `$.if(node, …)`
    // call. A `{@html}` mis-routed to the ROOT region would instead emit its `$.html` in the
    // root's post-walk ops — AFTER the `$.if(` call. So `$.html(` preceding `$.if(` discriminates
    // correct branch-region routing from the root-region regression.
    assert!(
        html_at < if_at,
        "the branch `{{@html}}` must emit inside the consequent closure (before the `$.if(` \
         call), not the root region (after it):\n{js}"
    );
    // The root region carries NO reactive op of its own — its only content is the if block,
    // so the sole `$.html` is the branch one (no root-level `$.html`).
    assert_eq!(
        js.matches("$.html(").count(),
        1,
        "exactly one `$.html` (the branch one) — no duplicate root-region routing:\n{js}"
    );
}

#[test]
fn svelte_self_inside_each_block_still_emits() {
    // The placement gate refuses ONLY the no-valid-context case: a `<svelte:self>`
    // validly placed inside an `{#each}` block (an allowed enclosing context, exercised
    // alongside the existing `{#if}` positive control) must STILL emit the recursive
    // self-call — the gate's valid-ancestor propagation must not over-reject a block body.
    let js = emit(
        "<script>let { items } = $props();</script>\n{#each items as item}<svelte:self />{/each}\n",
        "App.svelte",
    );
    assert!(
        js.contains("App("),
        "a svelte:self inside an {{#each}} block must still emit the recursive call:\n{js}"
    );
    // NEGATIVE: still a STATIC self-reference, never a dynamic `$.component`.
    assert!(
        !js.contains("$.component("),
        "an in-block svelte:self must not route through $.component:\n{js}"
    );
}

#[test]
fn slot_inside_if_block_body_emits_in_the_branch_region() {
    let js = emit(
        "<script>export let open;</script>\n{#if open}<div><slot /></div>{/if}\n",
        "App.svelte",
    );
    assert!(
        js.contains("var consequent = ($$anchor) => {"),
        "the branch closure:\n{js}"
    );
    assert!(
        js.contains("$.slot(node_1, $$props, 'default', {}, null);"),
        "the slot emits inside the branch region against its own anchor:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_each_item_dep_reads_plain_get_in_wrap() {
    // An `{#each}` item is the official `each` binding kind — a PLAIN `$.get`
    // dependency read, NOT deep-read (L-shape oracle):
    //   let $0 = $.derived_safe_equal(() => ($.get(item), $.untrack(() => $.get(item).m())));
    let js = emit(
        "<script>export let items;</script>\n{#each items as item}<div><slot foo={item.m()} /></div>{/each}\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "let $0 = $.derived_safe_equal(() => ($.get(item), $.untrack(() => $.get(item).m())));"
        ),
        "the each-item dep is a plain signal read:\n{js}"
    );
    assert!(
        !js.contains("$.deep_read_state($.get(item))"),
        "an each item is never deep-read:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn the_each_item_flag_and_its_read_form_move_together() {
    // The `EACH_ITEM_REACTIVE` flag and the item's READ form are two halves of
    // ONE decision: with the flag clear the runtime hands the render callback
    // the RAW item, so a `$.get(item)` read would dereference a non-signal;
    // with it set the callback receives a signal, so a plain read would yield
    // the signal object instead of its value. Either half alone is a module
    // that mounts and renders the wrong thing.
    //
    // Both rows were measured against the pinned official compiler.

    // Key IS the item ⇒ flag 16 (immutable only) and a PLAIN item read.
    let plain = emit(
        "<script>\n  let items = $state(['a']);\n</script>\n{#each items as item (item)}<li>{item}</li>{/each}\n",
        "App.svelte",
    );
    assert_eq!(
        each_flags(&plain),
        16,
        "the non-reactive flags moved:\n{plain}"
    );
    assert!(
        plain.contains("$.set_text(text, item)"),
        "a NON-reactive each item must be read plainly:\n{plain}"
    );
    assert!(
        !plain.contains("$.get(item)"),
        "a non-reactive each item must never be dereferenced as a signal:\n{plain}"
    );

    // Key is NOT the item ⇒ flag 17 (immutable | item-reactive) and a `$.get`
    // read. This is the row that stops the correction from being applied
    // universally.
    let signal = emit(
        "<script>\n  let items = $state([{id:1}]);\n</script>\n{#each items as item (item.id)}<li>{item.id}</li>{/each}\n",
        "App.svelte",
    );
    assert_eq!(
        each_flags(&signal),
        17,
        "the reactive flags moved:\n{signal}"
    );
    assert!(
        signal.contains("$.get(item)"),
        "a REACTIVE each item must be read through `$.get`:\n{signal}"
    );
    assert!(parses_as_js(&plain) && parses_as_js(&signal));
}

#[test]
fn an_each_item_destructure_with_a_renamed_property_refuses() {
    // `{ id: foo }` declares ONE name (`foo`) but `foo` is NOT the property
    // key — reading `$$item.foo` would be silently wrong.
    assert_each_item_destructure_shape_refuses(
        "<script>\n  let items = $state([{id:1}]);\n</script>\n{#each items as { id: foo } (foo)}<li>{foo}</li>{/each}\n",
    );
}

#[test]
fn an_each_item_array_destructure_refuses() {
    // `[id]` declares ONE name (`id`) but it is an ARRAY INDEX read, not a
    // property-key read.
    assert_each_item_destructure_shape_refuses(
        "<script>\n  let items = $state([['a']]);\n</script>\n{#each items as [id] (id)}<li>{id}</li>{/each}\n",
    );
}

#[test]
fn an_each_item_rest_destructure_refuses() {
    // `{ ...rest }` declares ONE name (`rest`) but it is the REMAINDER
    // object, not a single property read.
    assert_each_item_destructure_shape_refuses(
        "<script>\n  let items = $state([{id:1, x:2}]);\n</script>\n{#each items as { ...rest }}<li>{rest.id}</li>{/each}\n",
    );
}

#[test]
fn a_multi_name_each_item_destructure_refuses_with_its_real_authored_span() {
    // A genuinely multi-name destructure (`{ a, b }`) refuses (correctly — this shape
    // isn't supported), through `pattern_single_binding`'s generic multi-binding arm.
    // That arm carries the pattern's own real authored span — every syntactic pattern
    // is interned WITH a backing span — matching the single-name shapes pinned above.
    let source = "<script>\n  let items = $state([{a:1,b:2}]);\n</script>\n{#each items as { a, b }}<li>{a}{b}</li>{/each}\n";
    let err = emit_result(source).expect_err("a multi-name each-item destructure must refuse");
    let ClientCompileError::Unsupported(surface) = &err else {
        panic!("expected a typed unsupported refusal, got {err:?}");
    };
    let UnsupportedSvelteRuntimeSurface::Block { construct, span } = surface else {
        panic!("expected a Block refusal, got {surface:?}");
    };
    assert_eq!(*construct, "destructuring-binding");
    let pattern_start = source
        .find("{ a, b }")
        .expect("source contains the pattern");
    assert_eq!(
        (span.start, span.end),
        (
            pattern_start as u32,
            (pattern_start + "{ a, b }".len()) as u32
        ),
        "the refusal must carry the pattern's REAL authored span, not a placeholder:\n{source}"
    );
}

#[test]
fn an_each_keyed_by_its_own_index_is_unkeyed_for_official() {
    // Official emits `$.each(node, 17, …, $.index, ($$anchor, item, i) => …)`
    // and reads `i` plainly; Verter emits flags 19 with `(item, i) => i` and
    // `$.get(i)`.
    let js = emit(
        "<script>\n  let items = $state(['a']);\n</script>\n{#each items as item, i (i)}<li>{i}{item}</li>{/each}\n",
        "App.svelte",
    );
    assert_eq!(
        each_flags(&js),
        17,
        "an index-keyed each is not keyed:\n{js}"
    );
    assert!(
        js.contains("$.index"),
        "an index-keyed each uses the official `$.index` key:\n{js}"
    );
}

#[test]
fn legacy_await_promise_call_wraps_thunk() {
    // Oracle: $.await(node, () => ($.deep_read_state(obj()), $.untrack(() => obj().m())), null, ($$anchor, v) => …
    let js = emit(
        &format!("{LEGACY_OBJ}{{#await obj.m() then v}}<p>{{v}}</p>{{/await}}\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "$.await(node, () => ({}), null",
            obj_wrap("obj().m()")
        )),
        "the await promise wraps inside its thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}
