use super::*;

#[test]
fn standard_structural_html_hosts_emit_without_a_runtime_refusal() {
    let rows = [
        ("main", "<main><div></div></main>"),
        ("section", "<section></section>"),
        ("header", "<header></header>"),
        ("footer", "<footer></footer>"),
        ("h3", "<h3>heading</h3>"),
        (
            "img",
            r#"<img data-intrinsic="img-tag" alt="intrinsic-img" src="x.png" />"#,
        ),
        (
            "form",
            r#"<form data-intrinsic="form-tag" action="/submit"></form>"#,
        ),
    ];

    for (tag, source) in rows {
        let js = emit(source, "StructuralHost.svelte");
        assert!(
            js.contains(&format!("<{tag}")),
            "the emitted template must retain the standard `{tag}` host:\n{js}"
        );
        assert!(
            js.contains("export default function StructuralHost"),
            "the standard `{tag}` host must produce a client component:\n{js}"
        );
    }
}

#[test]
fn pure_interp_text_child_emits_is_text_flag() {
    // F11: `<p>{count}</p>` (a PURE single interpolation text child) → official
    // emits `$.child(p, true)`. RED against the pre-fix descent builder (which
    // emitted `$.child(p)`, dropping the hydration is_text flag). Two roots so the
    // walk descends (a single-root `<p>` clones into `p` then descends to its text
    // child).
    let src = "<script>let count = $state(0);</script>\n<p>{count}</p>\n<button onclick={() => count++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.child(p, true)"),
        "pure-interp text child carries is_text:\n{js}"
    );
}

#[test]
fn mixed_text_run_child_has_no_is_text_flag() {
    // F11 NEGATIVE: a MIXED text run (`<p>x {count}</p>`) does NOT carry the is_text
    // flag — `$.child(p)`. Verified against svelte@5.56.10 (the §1.2 `<h1>Hello
    // {name}!</h1>` mixed run is `$.child(h1)`). Discriminates a builder that would
    // flag every text child.
    let src = "<script>let count = $state(0);</script>\n<p>x {count}</p>\n<button onclick={() => count++}>y</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.child(p)") && !js.contains("$.child(p, true)"),
        "a mixed text run gets no is_text flag:\n{js}"
    );
}

#[test]
fn sibling_to_pure_interp_text_forces_explicit_offset_and_is_text() {
    // F11: a sibling descent landing on a pure-interp text node forces the explicit
    // offset (even 1) + the trailing true: `$.sibling($.child(div), 1, true)`.
    // Verified against svelte@5.56.10. The static sibling is an allowlisted `<p>` (a
    // `<span>` is out of the §1.2 element allowlist).
    let src = "<script>let count = $state(0);</script>\n<div><p></p>{count}</div>\n<button onclick={() => count++}>z</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.sibling($.child(div), 1, true)"),
        "sibling-to-pure-interp-text forces explicit offset + is_text:\n{js}"
    );
}

#[test]
fn root_leading_text_before_dynamic_emits_pre_clone_next() {
    // CODEGEN BUG A: a ROOT-level leading static TEXT before the first named dynamic
    // position (`x<button onclick={() => c++}>{c}</button>`) is the official
    // `is_text_first` case — official emits a PRE-CLONE `$.next();` BEFORE
    // `var fragment = root();` (skipping the inserted leading anchor), then descends to
    // the button via `$.sibling($.first_child(fragment))`. Verified against
    // svelte@5.56.10. RED against the pre-fix emitter (which cloned first with NO
    // pre-clone `$.next()`).
    let src = "<script>let c = $state(0);</script>\nx<button onclick={() => c++}>{c}</button>\n";
    let js = emit(src, "App.svelte");
    let clone_at = js
        .find("var fragment = root();")
        .expect("root fragment clone frame");
    let next_at = js.find("$.next();").expect("pre-clone $.next()");
    assert!(
        next_at < clone_at,
        "the root text-first `$.next();` must be emitted BEFORE `var fragment = root();`:\n{js}"
    );
    // The dynamic button is still reached via `$.sibling($.first_child(fragment))`.
    assert!(
        js.contains("$.sibling($.first_child(fragment))"),
        "the dynamic button must descend via `$.sibling($.first_child(fragment))`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn in_element_leading_text_does_not_emit_pre_clone_next() {
    // NEGATIVE / DISCRIMINATING: leading text INSIDE an element (the §1.2-class
    // `<button>clicks: {count}</button>`) is NOT the root `is_text_first` case — the
    // in-element walk reaches the text via `$.child(button)`, with NO pre-clone
    // `$.next()`. This guards the codegen-A fix from over-firing on in-element leading
    // text (which would diverge from official). The single-element root clones the
    // button directly (`var button = root();`), so a stray `$.next()` would be a
    // spurious cursor advance.
    let src = "<script>let count = $state(0);</script>\n<button onclick={() => count += 1}>clicks: {count}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        !js.contains("$.next();") && !js.contains("$.next("),
        "in-element leading text must NOT emit a pre-clone `$.next()` (§1.2 byte parity):\n{js}"
    );
    assert!(
        js.contains("var button = root();"),
        "the single-element root clones the button directly:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn data_attribute_name_is_lowercased_in_static_skeleton() {
    // The official client template serializer lowercases a static attribute NAME on
    // an HTML element (`template.js`: `is_html ? key.toLowerCase() : key`). A mixed-
    // case `data-FooBar` attribute folds into the skeleton as `data-foobar`. RED
    // against the pre-fix serializer (which emitted the raw `data-FooBar`).
    let src = "<script>let c = $state(0);</script>\n<div data-FooBar=\"x\"><button onclick={() => c++}>{c}</button></div>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("data-foobar=\"x\""),
        "a static `data-FooBar` attr name must be lowercased to `data-foobar` in the skeleton:\n{js}"
    );
    // NEGATIVE: the raw mixed-case name must NOT appear in the skeleton.
    assert!(
        !js.contains("data-FooBar"),
        "the raw mixed-case attr name must not survive into the skeleton:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn aria_attribute_name_is_lowercased_in_static_skeleton() {
    // Same lowercase rule for the `aria-*` family — `aria-LabelledBy` → `aria-labelledby`.
    let src = "<script>let c = $state(0);</script>\n<div aria-LabelledBy=\"x\"><button onclick={() => c++}>{c}</button></div>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("aria-labelledby=\"x\""),
        "a static `aria-LabelledBy` attr name must be lowercased to `aria-labelledby`:\n{js}"
    );
    assert!(
        !js.contains("aria-LabelledBy"),
        "the raw mixed-case aria attr name must not survive into the skeleton:\n{js}"
    );
}

#[test]
fn static_single_element_root_has_no_next() {
    // NEGATIVE: a single static element root (`<p>a</p>`) is the `is_single_element`
    // clone-root path — official clones the element directly (`var p = root();
    // $.append(...)`) with NO `$.next()`. The `$.next()` cursor advance is a
    // FRAGMENT-walk concern only. (Runes-mode via the unused `$state`.)
    let src = "<script>let c = $state(0);</script>\n<p>a</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("var p = root();"),
        "a single static element root clones directly:\n{js}"
    );
    assert!(
        !js.contains("$.next("),
        "a single static element root must NOT emit `$.next()`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn dynamic_attribute_now_emits_set_attribute() {
    // a dynamic attribute (`id={id}`) now EMITS `$.set_attribute` (was a
    // per-attribute refusal previously). The reactive handler keeps `id` a real signal.
    let js = emit(
        "<script>let id = $state('x');</script>\n<div onclick={() => id += '!'} id={id}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.template_effect(() => $.set_attribute(div, 'id', $.get(id)))"
        )),
        "a dynamic attribute must now emit set_attribute:\n{js}"
    );
}

#[test]
fn html_tag_emits_the_raw_markup_helper() {
    // A `{@html}` as the sole child of an element emits `$.html(el, () => h, true)` +
    // `$.reset(el)` — the controlled-child raw-markup form (the third arg `true`).
    let js = emit(
        "<script>let h = $state('<b>x</b>');</script>\n<div>{@html h}</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.html(div, () => h, true)")),
        "a {{@html}} sole child must emit the controlled $.html form:\n{js}"
    );
    assert!(
        n.contains(&nc("$.reset(div)")),
        "the {{@html}} sole-child form must reset the element after:\n{js}"
    );
    // NEGATIVE: the removed refusal must be gone — no spread-or-html diagnostic surfaces.
    assert!(
        !js.contains("svelte-runtime-unsupported-spread-or-html"),
        "the deleted spread-or-html refusal must not surface:\n{js}"
    );
}

#[test]
fn element_spread_folds_static_dynamic_directives_in_source_order() {
    // The fold order: plain attrs / spreads in SOURCE order, then the merged `[$.CLASS]`,
    // then `[$.STYLE]`. A static `class` attribute stays a `class:` key (NOT computed);
    // a `class:` shorthand directive folds into `[$.CLASS]` as object shorthand; a
    // `style:` expression directive folds into `[$.STYLE]`. The static `class="c"` is NOT
    // baked into the template (the spread switches the whole strategy) — the skeleton is
    // bare.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div class=\"c\" {...props} class:on style:width={w}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect(div, () => ({ class: 'c', ...props, [$.CLASS]: { on }, [$.STYLE]: { width: w } }))"
        )),
        "the fold must order plain attrs/spreads in source order then merged CLASS/STYLE:\n{js}"
    );
    // NEGATIVE: the static class is NOT baked into the cloned skeleton.
    assert!(
        n.contains(&nc("$.from_html(`<div></div>`)")),
        "a spread element's static attrs must NOT be baked into the template:\n{js}"
    );
}

#[test]
fn html_direct_call_payload_elides_the_thunk_to_the_bare_callee() {
    // A `{@html render()}` (a direct, non-optional, zero-argument identifier call) elides
    // the `() => …` thunk to the bare callee `render` — the official CallExpression
    // elision. A member call / optional call / args is NOT elided (covered by the corpus).
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div>{@html render()}</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.html(div, render, true)")),
        "a direct identifier-call {{@html}} payload must elide to the bare callee:\n{js}"
    );
    // NEGATIVE: it must NOT wrap the call in a thunk.
    assert!(
        !n.contains(&nc("$.html(div, () => render(), true)")),
        "the elided payload must not be a thunk:\n{js}"
    );
}

#[test]
fn spread_payload_sequence_stays_one_wrapped_value() {
    // A SequenceExpression spread payload `{...(a, b)}` stays ONE spread value: the
    // BEHAVIORAL sequence wrap keeps it parenthesized so it does NOT split into two object
    // entries (`...a, b`), which would be a semantic change. Source-preserving keeps the
    // author paren and the sequence wrap re-wraps it, so the emitted operand is a wrapped
    // sequence (`...(a, b)`, modulo a behavior-preserving redundant outer paren the minifier
    // collapses — this assertion is paren-COUNT-insensitive on purpose).
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div {...(a, b)}></div>\n",
        "App.svelte",
    );
    // Use the paren-preserving collapse (`normalize_js_cosmetics` strips arrow-body parens,
    // which would erase the sequence wrap we are asserting). Source-preserving keeps the
    // author paren and the sequence wrap re-wraps it, so the operand is a wrapped sequence
    // (`...((a, b))` — a redundant outer paren the minifier collapses). Assert the spread
    // operand carries the wrapped sequence, paren-COUNT-insensitively.
    let n = collapse_ws_keep_parens(&js);
    assert!(
        n.contains("...(") && n.contains("(a, b)"),
        "a sequence-expression spread payload must stay a wrapped single value:\n{js}"
    );
    // NEGATIVE (the behavioral discriminator): the sequence must NOT be split into two
    // entries — `b` must not leak as a second object entry.
    assert!(
        !n.contains("...a, b)") && !n.contains("...a, b }"),
        "a sequence-expression spread payload must NOT be split into two entries:\n{js}"
    );
}

#[test]
fn html_object_literal_payload_wraps_arrow_body_as_object() {
    // An OBJECT-LITERAL `{@html}` payload wraps the concise-arrow body in one paren pair so
    // `() => { … }` is an OBJECT expression, not a block body returning `undefined`. Pinned
    // svelte@5.56.10: `{@html {a:1}}` → `$.html(div, () => ({ a: 1 }), true)`. Without the wrap
    // the body parses as a block (`{ a: 1 }` is a labeled statement) and returns `undefined` —
    // a SILENT behavioral miscompile (the markup goes blank), exactly like the sequence wrap is
    // behavioral.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div>{@html {a:1}}</div>\n",
        "App.svelte",
    );
    // Use the paren-preserving collapse (`normalize_js_cosmetics` strips arrow-body parens,
    // which would erase the object wrap we are asserting). The paren after `() => ` is
    // LOAD-BEARING, so assert the literal `() => ({` body.
    let n = collapse_ws_keep_parens(&js);
    assert!(
        n.contains("$.html(div, () => ({"),
        "an object-literal {{@html}} payload must wrap the arrow body as an object:\n{js}"
    );
    // NEGATIVE (the behavioral discriminator): it must NOT emit the bare block-body form
    // `() => {a:1}` / `() => { a: 1 }` (a block returning `undefined`).
    assert!(
        !n.contains("() => {a:1}") && !n.contains("() => { a: 1 }") && !n.contains("() => {a: 1}"),
        "the object-literal payload must NOT emit a bare block body returning undefined:\n{js}"
    );
}

#[test]
fn html_member_of_object_literal_payload_reparses_as_valid_js() {
    // A MEMBER access ON an object literal (`{@html {html:'x'}.html}`) is the NON-PARSING case:
    // without the wrap the body is `() => {html:'x'}.html` — a block statement followed by a
    // stray `.html`, which is INVALID JS (a hard syntax error, not just a wrong value). The wrap
    // makes it `() => ({ html: 'x' }).html`, valid and correct. Pinned svelte@5.56.10.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div>{@html {html:\"<b>x</b>\"}.html}</div>\n",
        "App.svelte",
    );
    // POSITIVE: the whole emitted module RE-PARSES as valid JS (the wrap defuses the
    // block-then-`.html` syntax error). This is THE load-bearing assertion: the unwrapped form
    // `() => {html:'x'}.html` is a hard JS syntax error, so a passing re-parse proves the wrap.
    assert!(
        parses_as_js(&js),
        "a member-of-object-literal {{@html}} payload must emit re-parsable JS:\n{js}"
    );
    // POSITIVE: the object literal opens immediately after the arrow with an opening paren
    // (`() => ({`), and the `.html` member access is present — the body is the parenthesized
    // member expression, not a bare block-then-member. The exact paren-close position
    // (`({obj}).html` vs `({obj}.html)`) is a behavior-preserving redundant-paren difference the
    // minifier collapses; both return `obj.html`. Assert paren-position-insensitively.
    let n = collapse_ws_keep_parens(&js);
    assert!(
        n.contains("() => ({") && n.contains(".html)"),
        "the member-of-object-literal payload must wrap the leading object literal:\n{js}"
    );
    // NEGATIVE (the discriminator): it must NOT emit the unwrapped block-then-member form
    // `() => {html:"<b>x</b>"}.html` (the non-parsing miscompile).
    assert!(
        !n.contains("() => {html:"),
        "the member-of-object-literal payload must NOT emit a bare block-then-member body:\n{js}"
    );
}

#[test]
fn html_optional_chain_object_literal_payload_wraps_arrow_body() {
    // An OPTIONAL-CHAIN member ON an object literal (`{@html {html:'x'}?.html}`) is wrapped by
    // OXC in a `ChainExpression`, but the chain's leftmost leaf is still the object literal, so
    // the whole concise-arrow body must wrap. Without the wrap the body is `() => {html:'x'}?.html`
    // — a block statement followed by a stray `?.html`, which is INVALID JS (a hard syntax error,
    // not just a wrong value). The wrap makes it `() => ({ html: 'x' })?.html`, valid and correct.
    // Pinned svelte@5.56.10.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div>{@html {html:\"<b>x</b>\"}?.html}</div>\n",
        "App.svelte",
    );
    // POSITIVE: the whole emitted module RE-PARSES as valid JS (the wrap defuses the
    // block-then-`?.html` syntax error). This is THE load-bearing assertion: the unwrapped form
    // `() => {html:'x'}?.html` is a hard JS syntax error, so a passing re-parse proves the wrap.
    assert!(
        parses_as_js(&js),
        "an optional-chain-on-object-literal {{@html}} payload must emit re-parsable JS:\n{js}"
    );
    // POSITIVE: the object literal opens immediately after the arrow with an opening paren
    // (`() => ({`), and the `?.html` optional member access is present — the body is the
    // parenthesized optional-chain member expression, not a bare block-then-chain. The exact
    // paren-close position is a behavior-preserving redundant-paren difference the minifier
    // collapses; both return `obj?.html`. Assert paren-position-insensitively.
    let n = collapse_ws_keep_parens(&js);
    assert!(
        n.contains("() => ({") && n.contains("?.html"),
        "the optional-chain-on-object-literal payload must wrap the leading object literal:\n{js}"
    );
    // NEGATIVE (the discriminator): it must NOT emit the unwrapped block-then-chain form
    // `() => {html:"<b>x</b>"}?.html` (the non-parsing miscompile the missing `ChainExpression`
    // arm produced).
    assert!(
        !n.contains("() => {html:"),
        "the optional-chain payload must NOT emit a bare block-then-chain body:\n{js}"
    );
}

#[test]
fn html_ts_wrapper_object_payload_wraps_arrow_body_unconditionally() {
    // A TS-WRAPPER over an object literal (`{a:1} as any`, `… satisfies …`, `…!`) is the case a
    // shape-based left-spine wrap predicate would UNDER-wrap: a top-level `as`/`satisfies`/`!` skin
    // is NOT an object-literal root, so a leftmost-leaf-is-object decision returns `false` and the
    // body emits as the bare block-body form `() => {a:1}` (a block returning `undefined` — a
    // SILENT behavioral miscompile). Because the concise-arrow payload body is always
    // parenthesized (`() => (EXPR)`), after the rewriter strips the TS skin the object literal
    // is parenthesized and returns correctly — complete-by-construction, no shape predicate.
    //
    // This is a PLAIN-`<script>` form on purpose (NOT a corpus cell): the `lang="ts"` variant
    // panics Verter's parse-domain TS-strip gate, and official svelte@5.56.10
    // REJECTS the plain-`<script>` TS-in-template form (no golden) while Verter ACCEPTS it (the
    // template expr parses as TSX and the rewriter strips the TS skin) — so it can only be locked
    // by a unit test on the accepted form, never an official-golden corpus row.
    let payloads = [
        "{a:1} as any",
        "{a:1} satisfies Record<string,number>",
        "{a:1}!",
        "{a:1} as any as any",
        // Stacked transparent skins in BOTH orders — non-null-then-`as` and an inner-`as`
        // under a non-null — each must peel to the parenthesized object, never a bare block.
        "{a:1}! as any",
        "({a:1} as any)!",
    ];
    for payload in payloads {
        let source =
            format!("<script>let __rune = $state(0);</script>\n<div>{{@html {payload}}}</div>\n");
        let js = emit(&source, "App.svelte");
        // Keep arrow-body parens (the wrap is exactly what we assert; `normalize_js_cosmetics`
        // would strip it).
        let n = collapse_ws_keep_parens(&js);
        // LOAD-BEARING: the whole emitted module re-parses as valid JS. (The bare-block form
        // `() => {a:1}` ALSO re-parses — `{a:1}` is a labeled statement — so re-parse alone does
        // NOT discriminate the TS-skin object case; the no-bare-block negative below does.)
        assert!(
            parses_as_js(&js),
            "a TS-wrapper-of-object {{@html}} payload `{payload}` must emit re-parsable JS:\n{js}"
        );
        // POSITIVE: the wrapped object thunk — the arrow body opens with `(` and the object
        // literal is parenthesized (`({`), so it RETURNS the object instead of parsing a block
        // body. (`({a:1} as any)!` over-wraps to `() => (({a:1}))` — still parenthesized, never a
        // bare block — so the assertion is on the object's `({` wrap, paren-COUNT-insensitive.)
        assert!(
            n.contains("$.html(div, () => (") && n.contains("({"),
            "a TS-wrapper-of-object {{@html}} payload `{payload}` must wrap the arrow body as an object:\n{js}"
        );
        // NEGATIVE (the discriminator that FAILS without the unconditional wrap): it must NOT
        // emit the bare block-body form `() => {a:1}` / `() => { a: 1 }` (a block returning
        // `undefined`).
        assert!(
            !n.contains("() => {a:1}")
                && !n.contains("() => { a: 1 }")
                && !n.contains("() => {a: 1}"),
            "a TS-wrapper-of-object {{@html}} payload `{payload}` must NOT emit a bare block body returning undefined:\n{js}"
        );
    }
}

#[test]
fn valueless_attribute_in_a_spread_fold_emits_raw_true_not_an_empty_string() {
    // A VALUELESS boolean attribute (`<input {...props} disabled />`) folds as the RAW
    // boolean `disabled: true` — NOT the empty-string `disabled: ''`. The IR carries the
    // value as `Option<StaticAttrValue>` where `None` is a valueless attribute; the fold
    // emits the bare `true` token for `None` (an empty-string value is a DIFFERENT IR
    // shape — `Some("")` — covered below). Pinned svelte@5.56.10:
    // `$.attribute_effect(input, () => ({ ...props, disabled: true }), …, true)`.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<input {...props} disabled />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect(input, () => ({ ...props, disabled: true }), void 0, void 0, void 0, void 0, true)"
        )),
        "a valueless attribute in a spread fold must emit the raw `true`:\n{js}"
    );
    // NEGATIVE: the empty-string form must be ABSENT (the pre-fix bug emitted `: ''`).
    assert!(
        !n.contains(&nc("disabled: ''")),
        "a valueless attribute must NOT fold as an empty string:\n{js}"
    );
}

#[test]
fn present_empty_string_attribute_in_a_spread_fold_stays_an_empty_string() {
    // A PRESENT-but-empty attribute (`disabled=""`, IR `Some(StaticAttrValue{value:""})`)
    // is DISTINCT from a valueless attribute: it folds as the empty-string `disabled: ''`,
    // NOT `disabled: true`. This pins the `None`-vs-`Some("")` boundary the valueless fix
    // must preserve. Pinned svelte@5.56.10:
    // `$.attribute_effect(input, () => ({ ...props, disabled: '' }), …, true)`.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<input {...props} disabled=\"\" />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect(input, () => ({ ...props, disabled: '' }), void 0, void 0, void 0, void 0, true)"
        )),
        "a present empty-string attribute in a spread fold must stay an empty string:\n{js}"
    );
    // NEGATIVE: it must NOT become the raw `true` (that is the VALUELESS form).
    assert!(
        !n.contains(&nc("disabled: true")),
        "a present empty-string attribute must NOT fold as the raw `true`:\n{js}"
    );
}

#[test]
fn html_paren_member_callee_emits_source_preserving_thunk() {
    // A `{@html (o.render)()}` is NOT a bare-identifier call (the callee `(o.render)` peels
    // to a MEMBER, not an Identifier), so it does NOT elide to a bare callee; it routes to the
    // value-thunk path, which is SOURCE-PRESERVING (the author paren is kept verbatim) and then
    // gets the unconditional concise-arrow-body wrap, so the thunk is the correct ZERO-ARG member
    // call `() => ((o.render)())` (the `$state o` is never reassigned here, so it demotes to a
    // plain `o`). The extra outer paren over a complete call expression is behavior-preserving and
    // collapses in the minifier (official drops both redundant parens); the BEHAVIORAL bar is a
    // correct thunk with a correct zero-arg member call, asserted here paren-COUNT-insensitively.
    let js = emit(
        "<script>let o = $state(0);</script>\n<div>{@html (o.render)()}</div>\n",
        "App.svelte",
    );
    let n = collapse_ws_keep_parens(&js);
    // POSITIVE (paren-COUNT-insensitive): the thunk leads with `() => (` and its body is the
    // zero-arg member call `(o.render)()` (redundant outer parens are behavior-preserving).
    assert!(
        n.contains("$.html(div, () => (") && n.contains("(o.render)(") && n.contains("), true)"),
        "a paren-member {{@html}} callee must emit the source-preserving zero-arg thunk:\n{js}"
    );
    // NEGATIVE (behavioral): it must NOT elide to a bare callee (the callee is a member, not a
    // bare identifier), and it must NOT leak an argument (the call stays zero-arg).
    assert!(
        !n.contains("$.html(div, o.render, true)")
            && !n.contains("$.html(div, () => (o.render)(o)")
            && !n.contains("$.html(div, () => ((o.render)(o)"),
        "a paren-member {{@html}} callee must stay a thunked zero-arg call:\n{js}"
    );
}

#[test]
fn html_bare_identifier_call_elides_to_the_bare_callee() {
    // A bare-identifier `{@html render()}` (a direct, non-optional, zero-arg identifier call
    // whose callee rewrites UNCHANGED) ELIDES the `() => …` thunk to the bare callee `render`.
    // Pinned svelte@5.56.10: `$.html(div, render, true)`.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div>{@html render()}</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.html(div, render, true)")),
        "a bare-identifier {{@html}} call must elide to the bare callee:\n{js}"
    );
}

#[test]
fn html_paren_wrapped_direct_call_payload_elides_the_thunk_to_the_bare_callee() {
    // A `{@html (render)()}` (a direct zero-arg identifier call whose callee is wrapped in
    // transparent author parens) STILL elides the thunk to the bare callee `render` — the
    // parens are peeled off the OXC `ParenthesizedExpression` callee before the
    // identifier-call check (the same transparent-paren peel the spread-operand path does).
    // Pinned svelte@5.56.10: `$.html(div, render, true)` (parens gone).
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div>{@html (render)()}</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.html(div, render, true)")),
        "a paren-wrapped direct identifier-call {{@html}} payload must elide to the bare callee:\n{js}"
    );
    // NEGATIVE: it must NOT keep the author parens in a thunk (the pre-fix bug).
    assert!(
        !n.contains(&nc("$.html(div, () => (render)(), true)")),
        "a paren-wrapped elided payload must not keep the author parens in a thunk:\n{js}"
    );
}

#[test]
fn html_double_paren_wrapped_direct_call_payload_elides_the_thunk() {
    // A `{@html ((render))()}` (a doubly-paren-wrapped callee) ALSO elides — the peel
    // walks through EVERY transparent `ParenthesizedExpression`. Pinned svelte@5.56.10:
    // `$.html(div, render, true)`.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div>{@html ((render))()}</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.html(div, render, true)")),
        "a double-paren-wrapped direct identifier-call {{@html}} payload must elide:\n{js}"
    );
    // NEGATIVE: no nested-paren thunk.
    assert!(
        !n.contains(&nc("$.html(div, () => ((render))(), true)")),
        "a double-paren-wrapped elided payload must not keep the author parens:\n{js}"
    );
}

#[test]
fn datalist_element_fails_closed_at_the_element_allowlist() {
    // A `<datalist>` is out of the allowlist — the component fails at the element gate
    // on `<datalist>`.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<datalist><option value=\"a\">A</option></datalist>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "datalist"),
    );
}

// ── static attrs on custom / customized-built-in elements ──────────────────────
//
// A custom element (hyphenated tag) or a customized built-in (`is=`) sets its
// attributes via PROPERTIES at runtime: official omits non-`is` attrs from the
// skeleton and emits `$.set_custom_element_data(node, name, value)`. Verter omits
// the attr from the skeleton (custom-element serializer rule) AND emits no setter
// — the attr silently VANISHES. Fail closed.

#[test]
fn custom_element_static_attr_fails_closed() {
    // F-γ: `<my-widget foo="bar">` → official `$.set_custom_element_data(my_widget,
    // 'foo', 'bar')`. RED: Verter dropped `foo` entirely (no skeleton entry, no
    // setter).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<my-widget foo=\"bar\"></my-widget>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }),
    );
}

#[test]
fn customized_builtin_static_attr_fails_closed() {
    // F-γ: a customized built-in (`is=`) with a non-`is` static attr — official
    // `$.set_custom_element_data(button, 'foo', 'bar')`. Fail closed.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button is=\"my-btn\" foo=\"bar\">x</button>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }),
    );
}

#[test]
fn customized_builtin_is_only_now_fails_closed_at_the_element_gate() {
    // DEMOTION proof: a customized built-in with ONLY the `is` attr USED to serialize
    // `is="my-btn"` and emit a Main. Under the strict allowlist, ANY element carrying
    // an `is` attribute is rejected at the element gate (`host-custom-element`)
    // BEFORE the attr walk — so an `is`-only `<button>` now fails closed (no Main).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button is=\"my-btn\">x</button>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }),
    );
}

// ── static-import malformed/adjacent siblings — every accepted import/bind/read
//    form's siblings either fail closed or take their own correct path ──

#[test]
fn import_assert_attribute_keyword_rejects_with_official_parse_parity() {
    // The deprecated `assert { type: 'json' }` attribute keyword is an OFFICIAL
    // acorn parse-REJECT in a plain script (`js_parse_error` "Unexpected token",
    // oracle-probed; acorn has no `assert` clause) — only `with { … }` is
    // admitted+preserved. The body probe carries the EXACT official code (OXC itself
    // is assert-lenient, so the probe discriminates the keyword structurally);
    // Verter must not emit — and must not silently normalize `assert` to `with`.
    for (label, src) in [
        (
            "instance slot",
            "<script>import data from './d.json' assert { type: 'json' }; let c = $state(0);</script>\n<p>{data}</p>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "module slot",
            "<script module>import data from './d.json' assert { type: 'json' };</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
    ] {
        let err =
            emit_result(src).expect_err("an `assert { … }` import attribute must fail closed");
        assert!(
            matches!(
                &err,
                ClientCompileError::OfficialReject(r)
                    if r.rule == CoreOfficialValidationRule::ScriptBodyParse
                        && r.official_code == "js_parse_error"
            ),
            "[{label}] expected the exact js_parse_error parse-parity reject for the \
             `assert` keyword, got {err:?}"
        );
    }
    // CONTROL: a `lang=\"ts\"` script parses `assert` fine (official ACCEPTS it there,
    // oracle-probed — the TS grammar keeps the legacy clause), so the Js-grammar
    // parse-parity rule must NOT fire; the component stays owned by the
    // TypeScript-script refusal (fail-closed, non-official-code channel).
    let js = emit(
        "<script lang=\"ts\">import data from './d.json' assert { type: 'json' }; let c = $state(0);</script>\n<p>{data}</p>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("import data from './d.json' with { type: 'json' }"),
        "a TypeScript import assertion must normalize to `with`:\n{js}"
    );
    assert!(
        !js.contains(" assert {"),
        "legacy assertion syntax leaked:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn import_with_attribute_is_preserved_verbatim() {
    // The accepted sibling: `with { type: 'json' }` is official-accepted and MUST
    // survive onto the emitted import statement (dropping it changes module-load
    // semantics for JSON/CSS modules).
    let js = emit(
        "<script>import data from './d.json' with { type: 'json' }; let c = $state(0);</script>\n<p>{data}</p>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("import data from './d.json' with { type: 'json' };"),
        "the `with` import attributes must be preserved verbatim:\n{js}"
    );
    // NEGATIVE: never rewritten to the deprecated `assert` spelling.
    assert!(
        !js.contains("assert {"),
        "the `with` clause must not be emitted as `assert`:\n{js}"
    );
}

#[test]
fn element_let_directive_still_fails_closed() {
    // A `let:` directive on a PLAIN ELEMENT is invalid Svelte (the slot-prop surface is
    // COMPONENT/fragment-only). The element-context `let:` MUST stay fail-closed — the
    // component `let:` path is the component-attr slot-prop classifier, NOT the element
    // refusal.
    assert_fail_closed(
        "<script>let __r = $state(0);</script>\n<div let:item>{__r}</div>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "let-directive",
                    ..
                }
            )
        },
    );
}

#[test]
fn svelte_element_dynamic_tag_emits_comment_anchored_element_call() {
    // `<svelte:element this={tag}>hi</svelte:element>` → the comment-anchor frame + `$.element(
    // node, () => tag, false, ($$element, $$anchor) => { … })` + `$.append`. The get-tag thunk
    // wraps the rewritten `this` expression; the children are the callback's body region.
    let js = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("var fragment = $.comment();")),
        "comment anchor:\n{js}"
    );
    assert!(
        n.contains(&nc("var node = $.first_child(fragment);")),
        "first_child:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.element(node, () => tag, false, ($$element, $$anchor) =>"
        )),
        "element call with get-tag thunk + is_svg=false + callback:\n{js}"
    );
    assert!(
        n.contains(&nc("$.append($$anchor, fragment);")),
        "mount:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_element_empty_body_omits_the_callback() {
    // `<svelte:element this={tag} />` with no attrs/binds/children → the OMITTED 3-argument
    // `$.element(node, () => tag, false)` call (no callback).
    let js = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.element(node, () => tag, false);")),
        "an empty-bodied dynamic element omits the callback (3-arg call):\n{js}"
    );
    // NEGATIVE: no 4th callback argument.
    assert!(
        !n.contains(&nc("$.element(node, () => tag, false, (")),
        "the empty-body call must NOT carry a callback:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn unscoped_svelte_element_emits_no_hash_anywhere() {
    // The NON-scoped control: an UNMATCHED selector leaves the dynamic element
    // unscoped — the 2-argument fold, NO set_class, NO hash token anywhere in
    // the module code.
    let module = module_result(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} id=\"x\">x</svelte:element>\n<style>.zzz { color: blue; }</style>\n",
    )
    .expect("an unmatched selector still compiles");
    let n = normalize_js_cosmetics(&module.code);
    assert!(
        n.contains(&nc("$.attribute_effect($$element, () => ({ id: 'x' }));")),
        "a non-scoped <svelte:element> keeps the 2-argument fold:\n{}",
        module.code
    );
    assert!(
        !module.code.contains("svelte-n50uah"),
        "no hash token reaches an unscoped dynamic element:\n{}",
        module.code
    );
    assert!(
        !module.code.contains("$.set_class"),
        "no synthetic class on an unscoped attr-bearing element:\n{}",
        module.code
    );

    // And a fully bare non-scoped `<svelte:element>` (no attrs at all) emits
    // NO attribute machinery whatsoever.
    let bare = module_result(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag}>x</svelte:element>\n<style>.zzz { color: blue; }</style>\n",
    )
    .expect("an unmatched selector still compiles");
    assert!(
        !bare.code.contains("$.set_class") && !bare.code.contains("$.attribute_effect"),
        "an unscoped attr-less dynamic element emits no class machinery:\n{}",
        bare.code
    );
}

#[test]
fn svelte_head_attribute_fails_closed_matching_official_reject() {
    // Official REJECTS any attribute / directive on `<svelte:head>`: its `SvelteHead` analyze
    // visitor throws `svelte_head_illegal_attribute` ("`<svelte:head>` cannot have attributes nor
    // directives") on every attribute. Verter fails closed on a head-borne attribute
    // (`ComponentOrSnippet { construct: "svelte:head attribute" }`) — PARITY with official's
    // reject, NOT a deviation. A head-with-attribute REFUSES; a plain head still EMITS.
    assert_fail_closed(
        "<script>let n = $state(0);</script>\n<svelte:head foo=\"bar\"><title>T</title></svelte:head>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:head attribute",
                    ..
                }
            )
        },
    );
    // An attribute-less head still emits (only the illegal attribute is rejected).
    assert!(emit(
        "<script>let n = $state(0);</script>\n<svelte:head><title>T</title></svelte:head>\n",
        "App.svelte"
    )
    .contains("$.head("));
}

#[test]
fn svelte_boundary_invalid_attribute_fails_closed() {
    // G2: the WHOLE boundary-attribute class outside official's `Pw` accept-list fails closed.
    // A legacy `on:click` directive (an `OnDirective`) rejects on the LEGACY `origin`.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<svelte:boundary on:click={() => c++}><p>x</p></svelte:boundary>\n",
        |s| matches!(
            s,
            UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "svelte:boundary legacy on: directive",
                ..
            }
        ),
    );
    // A modern non-`onerror` event (`onclick`), a modern `onerrorcapture` (a capture-suffixed name
    // that is NOT the exact `onerror` in `Pw`), and an arbitrary static attribute all fail closed
    // as `svelte:boundary attribute` (official rejects every non-{onerror,failed,pending} name).
    for src in [
        "<script>let c = $state(0);</script>\n<svelte:boundary onclick={() => c++}><p>x</p></svelte:boundary>\n",
        "<script>let c = $state(0);</script>\n<svelte:boundary onerrorcapture={() => c++}><p>x</p></svelte:boundary>\n",
        "<script>let c = $state(0);</script>\n<svelte:boundary foo=\"x\">{c}<p>y</p></svelte:boundary>\n",
    ] {
        assert_fail_closed(src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:boundary attribute",
                    ..
                }
            )
        });
    }
}

#[test]
fn svelte_window_element_sibling_emits_both() {
    // F2: a MIXED root `<svelte:window/>` + `<p>` sibling emits BOTH — the `<p>` clones through
    // the normal body path and the window `$.event(...)` interleaves (the host special is
    // TRANSPARENT to root classification). RED against the concern that the host bypass drops
    // the sibling.
    let js = emit(
        "<script>\n\tlet count = $state(0);\n</script>\n\n<svelte:window onresize={() => count++} />\n<p>hi</p>\n",
        "special/svelte_window_element_sibling.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    // The `<p>` sibling clones from its OWN `$.from_html` template factory and is instantiated —
    // precise, not the near-always-true `root()` disjunct the pre-tighten assertion used.
    assert!(
        js.contains("$.from_html(`<p>hi</p>`)"),
        "the <p> sibling clones from its own template factory:\n{js}"
    );
    assert!(
        n.contains(&nc("var p = root();")),
        "the <p> sibling is instantiated from the template:\n{js}"
    );
    // The window event emits as a DIRECT global `$.event` listener (the full call, not a prefix).
    assert!(
        n.contains(&nc("$.event('resize', $.window, () => $.update(count));")),
        "the window resize event emits a direct global listener:\n{js}"
    );
    // The sibling mounts precisely — the `p` node is appended to the render anchor.
    assert!(
        n.contains(&nc("$.append($$anchor, p);")),
        "the <p> sibling mounts to the anchor:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_options_custom_element_string_tag_defines_custom_element() {
    // `<svelte:options customElement="x-foo">` compiles the component as a
    // custom element: the module epilogue registers it via
    // `customElements.define(tag, $.create_custom_element(Cmp, {}, [], [],
    // { mode: 'open' }))` (the 5-arg open-shadow default). The body frame is
    // FACT-DRIVEN: a no-props component keeps the plain frame (no `$.push` /
    // `$.pop`, no `$$exports`).
    let js = emit_result(
        "<svelte:options customElement=\"x-foo\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a valid string-tag customElement compiles");
    assert!(
        js.contains(
            "customElements.define('x-foo', $.create_custom_element(App, {}, [], [], { mode: 'open' }));"
        ),
        "missing the define epilogue:\n{js}"
    );
    // NEGATIVE: the fact-driven frame — no props/exports ⇒ no component context.
    assert!(!js.contains("$.push"), "no-props CE must not push:\n{js}");
    assert!(!js.contains("$.pop"), "no-props CE must not pop:\n{js}");
    assert!(
        !js.contains("$$exports"),
        "no-props CE carries no $$exports:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {"),
        "no-props CE takes no $$props param:\n{js}"
    );
}

#[test]
fn custom_element_explicit_raw_source_key_covers_the_aliased_member() {
    // The dual of the aliased-explicit case: the explicit entry names the RAW
    // SOURCE key (`bar`) of an aliased member (`let { bar: foo }`). No local
    // binding `bar` exists, so the explicit entry emits under its own name; the
    // inferred loop then SKIPS the member (its emitted key `bar` IS a raw
    // explicit-entry name) — a SINGLE entry, official parity (oracle-verified
    // against pinned `svelte@5.56.10`).
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-raw', props: { bar: { reflect: true } } }} />\n<script>\n\tlet { bar: foo } = $props();\n</script>\n\n<p>{foo}</p>\n",
    )
    .expect("a customElement with a raw-source-key explicit prop compiles");
    assert!(
        js.contains(
            "customElements.define('x-raw', $.create_custom_element(App, { bar: { reflect: true } }, [], [], { mode: 'open' }));"
        ),
        "the explicit entry covers the aliased member — a single `bar` entry:\n{js}"
    );
    // NEGATIVE: no inferred `bar: {}` duplicate — the member's emitted key
    // matches the raw explicit-entry name, so the remainder loop skips it.
    assert!(
        !js.contains("bar: {}"),
        "no inferred duplicate when the explicit entry names the raw source key:\n{js}"
    );
    assert!(
        js.contains("get bar() { return foo(); }"),
        "the accessor still rides the source key over the local:\n{js}"
    );
}

#[test]
fn custom_element_empty_attribute_string_omits_the_field() {
    // An EMPTY `attribute: ""` descriptor value: pinned `svelte@5.56.10` emits
    // `{ a: {} }` — the `attribute` field is OMITTED entirely (the official
    // transform pushes the field only for a truthy string), never
    // `attribute: ''`. Sibling fields on the same entry survive the omission
    // (`{ attribute: "", reflect: true }` → `{ reflect: true }`,
    // oracle-verified).
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-empty', props: { a: { attribute: \"\" } } }} />\n<script>\n\tlet { a } = $props();\n</script>\n\n<p>{a}</p>\n",
    )
    .expect("a customElement with an empty attribute string compiles");
    assert!(
        js.contains(
            "customElements.define('x-empty', $.create_custom_element(App, { a: {} }, [], [], { mode: 'open' }));"
        ),
        "an empty attribute string omits the field (official `{{ a: {{}} }}`):\n{js}"
    );
    // NEGATIVE: no empty-string attribute field anywhere.
    assert!(
        !js.contains("attribute: ''"),
        "no `attribute: ''` field may be emitted for an empty string:\n{js}"
    );
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-em2', props: { a: { attribute: \"\", reflect: true } } }} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a customElement with an empty attribute string + reflect compiles");
    assert!(
        js.contains(
            "customElements.define('x-em2', $.create_custom_element(App, { a: { reflect: true } }, [], [], { mode: 'open' }));"
        ),
        "the sibling `reflect` field survives the empty-attribute omission:\n{js}"
    );
    assert!(
        !js.contains("attribute:"),
        "no attribute field survives an empty string:\n{js}"
    );
}

#[test]
fn custom_element_bare_host_alone_fails_closed_as_official_degenerate() {
    // ORACLE NOTE (pinned `svelte@5.56.10`, first-hand): a customElement whose
    // SOLE `$host()` use is BARE (result discarded), with NO real props binder
    // (`$props()` / `$bindable(...)` / legacy prop), NO `needs_context` reason,
    // and NO member access on the `$host()` call result itself, emits the
    // DEGENERATE-UNBOUND official output — the component signature DROPS the
    // `$$props` parameter yet the body still references it:
    //
    //   export default function App($$anchor) {
    //       ...
    //       $.event('focus', button, () => $$props.$$host);
    //   }
    //
    // (`$$props` is UNBOUND — a runtime `ReferenceError` when the handler
    // fires.) Verter must NOT silently repair that output by force-binding the
    // `$$props` parameter (a blanket `uses_props |= host_used` would): the
    // degenerate residue fails closed BEFORE emission instead. The refusal
    // fires iff `host_used && !props_param_bound`, where `props_param_bound =
    // real_props_binder || needs_context` (a member on the `$host()` call
    // result is itself a `needs_context` reason; the bare call here has none).
    let err = emit_result(
        "<svelte:options customElement=\"x-solo\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect_err("a bare-only $host() with no props-parameter binder fails closed");
    let ClientCompileError::Unsupported(surface) = err else {
        panic!("expected the typed unsupported surface, got: {err:?}");
    };
    assert!(
        matches!(
            &surface,
            UnsupportedSvelteRuntimeSurface::HostOrCustomElement { surface, .. }
                if *surface == "$host"
        ),
        "the degenerate bare host refuses through the $host surface: {surface:?}"
    );
    assert_eq!(
        surface.diagnostic_code(),
        "svelte-runtime-unsupported-host-custom-element",
        "the refusal carries the host/custom-element diagnostic id"
    );
}

#[test]
fn custom_element_mixed_bare_and_member_host_accepts_both() {
    // ONE member-accessed host use ADMITS a sibling BARE host use (official
    // parity: the member access binds the props parameter for the whole
    // component, and the bare sibling rides the same bound `$$props.$$host`).
    let js = emit_result(
        "<svelte:options customElement=\"x-mix\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host()}>a</button>\n<button onblur={() => $host().foo}>b</button>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a bare host sibling of a member-accessed host compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the member-accessed sibling binds $$props for the component:\n{js}"
    );
    assert!(
        js.contains("$.event('focus', button, () => $$props.$$host);"),
        "the bare sibling lowers to the bound host read:\n{js}"
    );
    assert!(
        js.contains("$.event('blur', button_1, () => $$props.$$host.foo);"),
        "the member sibling lowers to the bound host member:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the member-on-call-result opens the context frame:\n{js}"
    );
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
}

#[test]
fn custom_element_inspect_with_admits_bare_host() {
    // A `$inspect(c).with(() => {})` chain sets `needs_context` (the elided
    // statement still forces the official production frame): a sibling BARE
    // `$host()` handler is admitted through it — official binds `$$props` and
    // pushes the frame while the handler stays the bare bound read.
    let js = emit_result(
        "<svelte:options customElement=\"x-iw\" />\n<script>\n\tlet c = $state(0);\n\t$inspect(c).with(() => {});\n</script>\n<button onfocus={() => $host()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("an $inspect().with sibling admits a bare $host()");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "needs_context binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the inspect-with chain opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$.event('focus', button, () => $$props.$$host);"),
        "the bare host lowers to the bound host read:\n{js}"
    );
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
}

#[test]
fn custom_element_shadow_none_omits_the_shadow_argument() {
    // `shadow: 'none'` with NO `extend`: arg5 is OMITTED entirely — the official
    // 4-arg `create_custom_element` call.
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-none', shadow: 'none' }} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a shadow:'none' customElement compiles");
    assert!(
        js.contains("customElements.define('x-none', $.create_custom_element(App, {}, [], []));"),
        "shadow:'none' emits the 4-arg call (arg5 omitted):\n{js}"
    );
    // NEGATIVE: no shadow-root init and no `void 0` placeholder.
    assert!(!js.contains("{ mode: 'open' }"), "no shadow init:\n{js}");
    assert!(
        !js.contains("void 0"),
        "no void-0 placeholder without arg6:\n{js}"
    );
}

#[test]
fn custom_element_extend_rides_arg6_with_void_0_shadow_placeholder() {
    // `shadow: 'none'` WITH `extend`: arg5 is the `void 0` placeholder and the
    // verbatim extend expression rides arg6 — the official 6-arg shape.
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-ext', shadow: 'none', extend: (c) => c }} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a shadow:'none' + extend customElement compiles");
    assert!(
        js.contains(
            "customElements.define('x-ext', $.create_custom_element(App, {}, [], [], void 0, (c) => c));"
        ),
        "extend rides arg6 behind the void-0 arg5 placeholder:\n{js}"
    );
    assert!(
        !js.contains("{ mode: 'open' }"),
        "shadow:'none' never emits the open shadow init:\n{js}"
    );
}

#[test]
fn custom_element_object_shadow_passes_verbatim_as_arg5() {
    // A `ShadowRootInit` OBJECT shadow passes through VERBATIM as arg5 (the
    // official accepts rich shadow objects — it does not reject them).
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-shadow', shadow: { mode: 'open', delegatesFocus: true } }} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("an object-shadow customElement compiles");
    assert!(
        js.contains(
            "customElements.define('x-shadow', $.create_custom_element(App, {}, [], [], { mode: 'open', delegatesFocus: true }));"
        ),
        "the shadow object expression rides arg5 verbatim:\n{js}"
    );
}

#[test]
fn custom_element_no_tag_object_creates_without_define() {
    // `customElement={{}}` (no tag): the bare `$.create_custom_element(…)`
    // statement is emitted — registration is left to the user, so there is NO
    // `customElements.define`.
    let js = emit_result(
        "<svelte:options customElement={{}} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a no-tag object customElement compiles");
    assert!(
        js.contains("$.create_custom_element(App, {}, [], [], { mode: 'open' });"),
        "the bare create statement is emitted:\n{js}"
    );
    assert!(
        !js.contains("customElements.define"),
        "a no-tag descriptor never defines:\n{js}"
    );
}

#[test]
fn custom_element_null_value_falls_back_to_the_compile_option() {
    // `customElement={null}` + the `customElement: true` compile option: the
    // null value sets NOTHING, so the official `customElementOptions ??
    // customElement` precedence falls back to the option — create, no define.
    let source = "<svelte:options customElement={null} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let alloc = Allocator::default();
    let parsed = crate::svelte::parser::parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        custom_element: true,
        ..Default::default()
    };
    let js = compile_client(source, &parsed, &opts, &alloc, false, false)
        .expect("null customElement + compile option compiles")
        .code;
    assert!(
        js.contains("$.create_custom_element(App, {}, [], [], { mode: 'open' });"),
        "the null value falls back to the compile option:\n{js}"
    );
    assert!(
        !js.contains("customElements.define"),
        "no tag, no define:\n{js}"
    );
}

#[test]
fn custom_element_options_value_wins_over_the_compile_option() {
    // An in-source `<svelte:options customElement>` value WINS over the compile
    // option (the official `customElementOptions ?? customElement` precedence):
    // the string tag defines, even with the option set.
    let source = "<svelte:options customElement=\"x-wins\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let alloc = Allocator::default();
    let parsed = crate::svelte::parser::parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        custom_element: true,
        ..Default::default()
    };
    let js = compile_client(source, &parsed, &opts, &alloc, false, false)
        .expect("the options value + compile option compiles")
        .code;
    assert!(
        js.contains("customElements.define('x-wins', $.create_custom_element(App, {}, [], [], { mode: 'open' }));"),
        "the options tag wins and defines:\n{js}"
    );
}

#[test]
fn custom_element_param_shadowed_host_stays_user_js() {
    // A handler PARAM shadowing `$host` inside an active customElement: the call
    // is USER JS on the local binding — never rune-rewritten, no `$$props`
    // forced. Official accepts and emits the verbatim handler; so does Verter.
    let js = emit_result(
        "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={($host) => $host()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a param-shadowed $host handler compiles");
    assert!(
        js.contains("$.event('focus', button, ($host) => $host());"),
        "the shadowed $host call stays verbatim user JS:\n{js}"
    );
    // NEGATIVE: the shadowed call is NOT rewritten to the host member, and the
    // shadowed handler alone does not force the `$$props` binding.
    assert!(
        !js.contains("$$props.$$host"),
        "a shadowed $host is never the rune:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {"),
        "no $$props binding is forced by a shadowed $host:\n{js}"
    );
}

#[test]
fn custom_element_duplicate_descriptor_axis_takes_the_first_entry() {
    // DUPLICATE descriptor keys (`{ tag: 'x-a', tag: 'x-b' }`): upstream's
    // `read_options` reads each axis via `properties.find(...)` — the FIRST
    // entry wins. The retained descriptor must come from the SAME single
    // validate+extract walk the official-reject gate runs, so the emitted tag is
    // `'x-a'` — a last-wins re-extraction would define `'x-b'`.
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-a', tag: 'x-b' }} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a duplicate-tag descriptor compiles");
    assert!(
        js.contains("customElements.define('x-a', $.create_custom_element(App, {}, [], [], { mode: 'open' }));"),
        "the FIRST duplicate axis entry wins (official find-first):\n{js}"
    );
    assert!(
        !js.contains("'x-b'"),
        "the later duplicate entry never surfaces:\n{js}"
    );
}

#[test]
fn pure_static_text_root_fails_closed() {
    // A PURE STATIC-TEXT root (`hello world` as the component root, no wrapping
    // element) is the official text-first topology — official emits `$.next(); var
    // text = $.text('hello world'); $.append(...)` (a `$.text()` NODE root reached
    // via `$.next()`), a distinct emission shape from the `from_html`-clone path.
    // Verter's clone-frame path would emit `var text = root();` where `root` is
    // bound to a `$.text(...)` NODE (not a factory function) → `TypeError: root is
    // not a function` at mount. It fails closed rather than emit that broken
    // module. RED against the pre-fix tree (which emitted `var root = $.text(...)`
    // followed by `var <region> = root();`).
    assert_fail_closed("<script>let c=$state(0);</script>hello world\n", |s| {
        matches!(s, UnsupportedSvelteRuntimeSurface::RootTextRegion { .. })
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Identifier-unsafe element tags + special-content-model reactive interior +
// the no-arg `$state()` shadow-robust `void 0` emission.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn custom_element_no_attr_fails_closed() {
    // A bare hyphenated CUSTOM element (`<my-widget></my-widget>`, no attributes) is
    // already in the demote list (the official compiler clones it via `importNode`
    // and sets its attributes via `$.set_custom_element_data` — web-components
    // breadth). A custom element with an UNSUPPORTED attribute already fails closed
    //; the no-attribute case was leaking through the element classifier and
    // being emitted. It must fail closed at the custom-element owner, never an
    // accepted Main. RED against the pre-fix tree (which emitted a `from_html`
    // `var fragment = root()` clone for it).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<my-widget></my-widget>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }
            )
        },
    );
}

#[test]
fn reserved_word_element_tag_fails_closed_not_invalid_js() {
    // A reserved-word HTML tag (`<var>`) whose synthesized DOM local var name would
    // be the reserved word `var` is accepted-and-emitted as `var var = root();` —
    // INVALID JS (a `SyntaxError`). The official compiler collision-renames the local
    // (`var_1`), which is naming breadth, not the §1.2-class core. It must fail closed
    // at the element-naming owner, never emit invalid JS. RED against the pre-fix
    // tree (which emitted `var var = root();`).
    assert_fail_closed("<script>let c = $state(0);</script>\n<var></var>\n", |s| {
        matches!(s, UnsupportedSvelteRuntimeSurface::ElementName { .. })
    });
}

#[test]
fn standard_identifier_safe_element_tags_still_emit() {
    // NEGATIVE (§1.2 preservation): a standard allowlist tag (`<div>`) whose local var
    // name is a valid JS identifier (`var div = root();`) must STILL emit — the
    // element fail-close must not over-reach into the §1.2 core allowlist (`a` /
    // `button` / `div` / `h1` / `input` / `p`). A reactive interpolation inside the
    // single-element `<div>` root keeps it runes-mode + named (`var div = root();`).
    let js = emit(
        "<script>let c = $state(0);</script>\n<div><button onclick={() => c++}>{c}</button></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("var div = root();"),
        "a standard identifier-safe element tag must still emit its clone frame:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor)"),
        "a supported standard-element component must emit a Main:\n{js}"
    );
}

#[test]
fn dynamic_flow_element_content_still_emits() {
    // NEGATIVE (§1.2 preservation): a reactive interpolation inside a NORMAL flow
    // element (`<div>{c}</div>`) is NOT special content-model — it must still emit the
    // §1.2-class `$.set_text` reactive-text op (the special-content fail-close must not
    // over-reach into normal flow elements).
    let js = emit(
        "<script>let c = $state(0);</script>\n<div>{c}</div><button onclick={() => c++}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.template_effect(() => $.set_text(text, $.get(c)))"),
        "a reactive flow-element interpolation must still emit set_text:\n{js}"
    );
}

// ── the dynamic-attribute / class / style surface negative boundary: deferred surfaces STILL refuse ─────────────────

#[test]
fn plain_value_attr_still_refuses() {
    // `value={v}` (a plain form-control setter) is a binding, NOT a plain attribute — it must still refuse
    // (through the binding-owning form-control / bindings channel).
    assert_fail_closed(
        "<script>let v = $state('x');</script>\n<input onclick={() => v += '!'} value={v}>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn plain_checked_attr_still_refuses() {
    assert_fail_closed(
        "<script>let v = $state(false);</script>\n<input onclick={() => v = !v} checked={v}>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "checked"),
    );
}

#[test]
fn dynamic_dir_attr_still_refuses() {
    // `dir={d}` is the special reflected-attr quirk (`el.dir = el.dir`) — DEFERRED, so
    // it must still fail closed rather than mis-emit a plain set_attribute.
    assert_fail_closed(
        "<script>let d = $state('ltr');</script>\n<div onclick={() => d = 'rtl'} dir={d}>x</div>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "dir"),
    );
}

#[test]
fn no_dynamic_attr_emits_the_boolean_set_attribute_misform() {
    // A global discriminating negative: NO attribute form ever emits the forbidden boolean
    // `$.set_attribute(el, name, value, true)` 4th-arg signature (the 4th arg is
    // hydration-warning suppression, never emitted in normal output).
    for src in [
        "<script>let v=$state(false);</script>\n<button onclick={() => v = !v} disabled={v}></button>\n",
        "<script>let v=$state(false);</script>\n<button onclick={() => v = !v} hidden={v}></button>\n",
        "<script>let v=$state('x');</script>\n<button onclick={() => v += '!'} id={v}></button>\n",
    ] {
        let js = emit(src, "App.svelte");
        // No `$.set_attribute(..., true)` 4th-arg signature.
        assert!(
            !normalize_js_cosmetics(&js).contains(",true))") || !js.contains("$.set_attribute"),
            "no set_attribute boolean 4th-arg misform:\n{js}"
        );
    }
}

#[test]
fn html_thunk_keeps_sequence_as_one_wrapped_value() {
    // `{@html (a, b)}` → the BEHAVIORAL sequence wrap keeps the top-level sequence as ONE
    // value (`() => (a, b)`, modulo a behavior-preserving redundant outer paren the minifier
    // collapses — this assertion is paren-COUNT-insensitive). Dropping the wrap would leak `b`
    // as a 3rd positional `$.html` arg, structurally breaking the call. The free `a`/`b`
    // demote to bare reads.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n{@html (a, b)}<button onclick={() => __rune++}>x</button>\n",
        "App.svelte",
    );
    let n = collapse_ws_keep_parens(&js);
    // The emitted thunk wraps the sequence (`() => ((a, b))` — source paren kept plus the
    // behavioral wrap, a redundant outer paren the minifier collapses). Assert the thunk body
    // is a single wrapped sequence value, paren-COUNT-insensitively.
    assert!(
        n.contains("$.html(node, () => (") && n.contains("(a, b)"),
        "a bare-sequence {{@html}} thunk must keep the sequence wrapped as one value:\n{js}"
    );
    // NEGATIVE (the behavioral discriminator): the sequence must NOT be unwrapped into a 3rd
    // positional `$.html` argument.
    assert!(
        !n.contains("$.html(node, () => a, b)"),
        "a bare-sequence {{@html}} thunk must NOT split the sequence into a 3rd arg:\n{js}"
    );
}

#[test]
fn invalid_attribute_name_rejects_under_a_spread() {
    // `<div {...p} 1foo="x">` REJECTS with `attribute_invalid_name` — the spread fold must
    // NOT swallow the invalid co-located name.
    let err = emit_result("<script>let p = $state({}), c = $state(0);</script>\n<div {...p} 1foo=\"x\"><button onclick={() => { c++; p = {}; }}>{c}</button></div>\n")
        .expect_err("an invalid attribute name under a spread must fail closed");
    match err {
        ClientCompileError::OfficialReject(rej) => assert_eq!(
            rej.rule,
            CoreOfficialValidationRule::AttributeInvalidName,
            "a digit-initial name under a spread must reject as AttributeInvalidName:\n{rej:?}"
        ),
        other => panic!("expected an OfficialReject(AttributeInvalidName), got {other:?}"),
    }
}

#[test]
fn invalid_attribute_name_rejects_an_operator_char() {
    // `<div @foo="x">` REJECTS — the name contains the `@` operator char.
    let err = emit_result(
        "<script>let c = $state(0);</script>\n<div @foo=\"x\"><button onclick={() => c++}>{c}</button></div>\n",
    )
    .expect_err("an operator-char attribute name must fail closed");
    assert!(
        matches!(
            err,
            ClientCompileError::OfficialReject(rej) if rej.rule == CoreOfficialValidationRule::AttributeInvalidName
        ),
        "an `@`-containing attribute name must reject as AttributeInvalidName:\n{err:?}"
    );
}

#[test]
fn valid_attribute_names_still_accept() {
    // NEGATIVE side: `data-x` / `aria-label` / `_foo` / `foo:bar` are VALID names — they must
    // NOT reject (a colon name + a leading underscore + mid-name hyphens are all accepted).
    for src in [
        "<script>let p = $state({}), c = $state(0);</script>\n<div {...p} data-x=\"1\"><button onclick={() => { c++; p = {}; }}>{c}</button></div>\n",
        "<script>let c = $state(0);</script>\n<div aria-label=\"x\"><button onclick={() => c++}>{c}</button></div>\n",
        "<script>let c = $state(0);</script>\n<div _foo=\"x\"><button onclick={() => c++}>{c}</button></div>\n",
    ] {
        let r = emit_result(src);
        assert!(
            !matches!(
                &r,
                Err(ClientCompileError::OfficialReject(rej)) if rej.rule == CoreOfficialValidationRule::AttributeInvalidName
            ),
            "a valid attribute name must NOT reject as AttributeInvalidName:\n{src}\n{r:?}"
        );
    }
}

#[test]
fn legacy_custom_element_export_let_emits_accessor_pairs() {
    // A legacy CUSTOM ELEMENT with an `export let` prop: the accessors force
    // makes the prop UPDATED (flags 12), the `$$exports` get/set pair reads and
    // writes through the accessor, the setter takes NO default param (legacy
    // shape — unlike the runes `$$value = <default>` form), and the frame is the
    // legacy `$.push($$props, false)` (oracle-verified).
    let js = emit(
        "<svelte:options customElement=\"my-el\" />\n<script>\nexport let label = 'x';\n</script>\n<p>{label}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let label = $.prop($$props, 'label', 12, 'x');"),
        "the CE accessors force composes UPDATED onto the legacy base:\n{js}"
    );
    assert!(
        js.contains("get label() { return label(); }"),
        "the export getter returns the accessor call:\n{js}"
    );
    assert!(
        js.contains("set label($$value) { label($$value); $.flush(); }"),
        "the export setter writes through the accessor + flushes:\n{js}"
    );
    // NEGATIVE: the legacy CE setter param carries NO default.
    assert!(
        !js.contains("set label($$value = 'x')"),
        "the legacy CE setter must not carry the runes default param:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, false)") && js.contains("return $.pop($$exports);"),
        "the CE exports frame is the legacy push flag + $$exports pop:\n{js}"
    );
    // NEGATIVE: the `$$exports` frame reason does NOT warrant the legacy init
    // hook — official gates `$.init()` on the needs-context analysis alone
    // (oracle-verified: this exact component emits push/pop($$exports) with NO
    // `$.init()`).
    assert!(
        !js.contains("$.init()"),
        "the exports-only frame must not emit `$.init()`:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_template_read_only_let_static_folds_to_text_content() {
    let js = emit("<script>let a = 1;</script>\n<p>{a}</p>\n", "App.svelte");
    let normalized = normalize_js_cosmetics(&js);
    assert!(
        normalized.contains(&nc("$.from_html(`<p></p>`)"))
            && normalized.contains(&nc("p.textContent='1'")),
        "a known read-only legacy local must use the static textContent topology:\n{js}"
    );
    assert!(
        !js.contains("$.template_effect"),
        "static text needs no effect:\n{js}"
    );
    assert!(parses_as_js(&js));
}

// ── The legacy value wrap on the `$.template_effect` DOM-attr / text surface ─

#[test]
fn legacy_two_attr_positions_same_call_wrap_independently_no_dedup() {
    // TWO positions of the IDENTICAL call expression: one INDEPENDENT wrapper
    // sequence per memoized dep — no cross-position dedup of the expression,
    // no cross-dep `deep_read_state` merge (oracle):
    //   $.template_effect(($0, $1) => { … }, [
    //     () => ($.deep_read_state(obj()), $.untrack(() => obj().m())),
    //     () => ($.deep_read_state(obj()), $.untrack(() => obj().m()))
    //   ]);
    let js = emit(
        "<script>export let obj;</script>\n<div title={obj.m()} data-x={obj.m()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "[() => ($.deep_read_state(obj()), $.untrack(() => obj().m())), () => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]"
        ),
        "both deps wrap independently in collection order:\n{js}"
    );
    assert_eq!(
        js.matches("$.deep_read_state(obj())").count(),
        2,
        "one deep-read per dep — never merged across `$0`/`$1`:\n{js}"
    );
    assert_eq!(
        js.matches("$.untrack(() => obj().m())").count(),
        2,
        "one untracked authored value per dep:\n{js}"
    );
    // The raw TRACKED dependency thunk must not survive in a definite-legacy
    // deps array (a bare `[() => obj().m()` / `, () => obj().m()` entry).
    assert!(
        !js.contains("[() => obj().m()") && !js.contains(", () => obj().m()"),
        "no raw tracked dependency survives:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_text_imported_member_wraps_inline() {
    // The accepted imported-member TEXT shape (`{NS.z}`) wraps inline — the
    // import deep-reads, the authored member untracks (oracle):
    //   $.template_effect(() => $.set_text(text,
    //     ($.deep_read_state(NS), $.untrack(() => NS.z))));
    let js = emit(
        "<script>import * as NS from './x.js';\nexport let p;</script>\n<p>{NS.z}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.set_text(text, ($.deep_read_state(NS), $.untrack(() => NS.z)))"),
        "the imported-member text value wraps inline:\n{js}"
    );
    assert!(
        !js.contains("$0"),
        "a non-call value never memoizes into a deps slot:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_attr_imported_call_dep_deep_reads_import() {
    // An IMPORTED zero-arg callee joins the dep as a `$.deep_read_state` live
    // read, and the authored call untracks BY REFERENCE (oracle):
    //   [() => ($.deep_read_state(helper), $.untrack(helper))]
    let js = emit(
        "<script>import { helper } from './h.js';\nexport let p;</script>\n<div title={helper()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("[() => ($.deep_read_state(helper), $.untrack(helper))]"),
        "the imported callee deep-reads and untracks by reference:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_attr_plain_local_call_untracks_without_deep_read() {
    // A PLAIN-LOCAL callee (official `normal` non-import) never joins the
    // deps: the authored call is untracked but NO `$.deep_read_state` is
    // fabricated (oracle): [() => ($.untrack(m))]. (The `on:click={m}` sibling
    // admits the top-level function through the handler-referent gate.)
    let js = emit(
        "<script>export let p;\nfunction m() { return 1; }</script>\n<button on:click={m}>x</button>\n<div title={m()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("[() => ($.untrack(m))]"),
        "the plain-local call memoizes untracked:\n{js}"
    );
    assert!(
        !js.contains("$.deep_read_state"),
        "no fabricated dependency read for a plain local:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_mixed_attr_call_and_member_chunks_wrap_independently() {
    // A MIXED attribute wraps each expression chunk on its own: the call
    // chunk memoizes the wrapped sequence into `$0`; the member chunk stays
    // INLINE-wrapped inside the template with the `?? ''` coercion (oracle):
    //   `p${$0 ?? ''}q${($.deep_read_state(b()), $.untrack(() => b().x)) ?? ''}r`
    //   [() => ($.deep_read_state(a()), $.untrack(() => a().f()))]
    let js = emit(
        "<script>export let a;\nexport let b;</script>\n<div title=\"p{a.f()}q{b.x}r\"></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("`p${$0 ?? ''}q${($.deep_read_state(b()), $.untrack(() => b().x)) ?? ''}r`"),
        "the member chunk wraps inline with `?? ''`; the call chunk reads `$0`:\n{js}"
    );
    assert!(
        js.contains("[() => ($.deep_read_state(a()), $.untrack(() => a().f()))]"),
        "the call chunk memoizes its own wrapped sequence:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_mixed_text_member_wraps_inline_with_coalesce() {
    // A mixed TEXT run wraps the accepted imported-member interpolation inline
    // inside the template literal (oracle):
    //   $.set_text(text, `x ${($.deep_read_state(NS), $.untrack(() => NS.z)) ?? ''} y`)
    let js = emit(
        "<script>import * as NS from './x.js';\nexport let p;</script>\n<p>x {NS.z} y</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("`x ${($.deep_read_state(NS), $.untrack(() => NS.z)) ?? ''} y`"),
        "the mixed-text member chunk wraps inline with `?? ''`:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_spread_operand_call_memoizes_raw() {
    // The spread operand is RAW w.r.t. build_expression but MEMOIZABLE.
    // Oracle: $.attribute_effect(div, ($0) => ({ ...$0 }), [() => obj().m()]);
    let js = emit(
        &format!("{LEGACY_OBJ}<div {{...obj.m()}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains("$.attribute_effect(div, ($0) => ({ ...$0 }), [() => (obj().m())])")
            || js.contains("$.attribute_effect(div, ($0) => ({ ...$0 }), [() => obj().m()])"),
        "the call-bearing spread operand memoizes RAW into $0 (cosmetic dep parens waived):\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "a spread operand is never legacy-wrapped:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_spread_colocated_attr_call_memoizes_wrapped() {
    // Oracle: $.attribute_effect(div, ($0) => ({ ...p(), title: $0 }), [() => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]);
    let js = emit(
        "<script>export let p; export let obj;</script>\n<div {...p} title={obj.m()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "$.attribute_effect(div, ($0) => ({{ ...p(), title: $0 }}), [() => ({})])",
            obj_wrap("obj().m()")
        )),
        "the co-located attr value wraps AND memoizes in the fold:\n{js}"
    );
    assert!(
        !js.contains("title: obj().m()"),
        "no raw fold value survives:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_spread_colocated_attr_member_wraps_inline() {
    // Oracle: $.attribute_effect(div, () => ({ ...p(), title: ($.deep_read_state(obj()), $.untrack(() => obj().x)) }));
    let js = emit(
        "<script>export let p; export let obj;</script>\n<div {...p} title={obj.x}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains(&format!("title: ({})", obj_wrap("obj().x"))),
        "the non-call co-located value wraps inline in the fold:\n{js}"
    );
    assert!(
        !js.contains("($0)"),
        "a member-only fold takes no memo params:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_spread_mixed_attr_chunk_memoizes_wrapped() {
    // Oracle: $.attribute_effect(div, ($0) => ({ ...p(), title: `a${$0 ?? ''}b` }), [() => (wrap)]);
    let js = emit(
        "<script>export let p; export let obj;</script>\n<div {...p} title=\"a{obj.m()}b\"></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("title: `a${$0 ?? ''}b`"),
        "the mixed fold chunk memoizes into the $0 template slot:\n{js}"
    );
    assert!(
        js.contains(&format!("[() => ({})]", obj_wrap("obj().m()"))),
        "the memoized chunk dep is the wrapped sequence:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_svelte_element_colocated_attr_memoizes_wrapped() {
    // Oracle: $.attribute_effect($$element, ($0) => ({ title: $0 }), [() => (wrap)]);
    let js = emit(
        &format!(
            "{LEGACY_OBJ}<svelte:element this={{'div'}} title={{obj.m()}}></svelte:element>\n"
        ),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "$.attribute_effect($$element, ($0) => ({{ title: $0 }}), [() => ({})])",
            obj_wrap("obj().m()")
        )),
        "the dynamic-element co-located attr wraps AND memoizes:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_element_this_call_stays_raw() {
    // Control: the `<svelte:element this={…}>` tag expression is RAW.
    // Oracle: $.element(node, () => obj().m(), false, …)
    let js = emit(
        &format!("{LEGACY_OBJ}<svelte:element this={{obj.m()}} title=\"t\"></svelte:element>\n"),
        "App.svelte",
    );
    assert!(
        js.contains("$.element(node, () => obj().m(), false"),
        "the dynamic tag expression stays raw:\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "the tag expression is never legacy-wrapped:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_html_call_wraps_thunk_no_elision() {
    // Oracle: $.html(node, () => ($.deep_read_state(obj()), $.untrack(() => obj().m())));
    let js = emit(&format!("{LEGACY_OBJ}{{@html obj.m()}}\n"), "App.svelte");
    assert!(
        js.contains(&format!("$.html(node, () => ({}))", obj_wrap("obj().m()"))),
        "the html payload wraps inside its getter:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_html_local_zero_arg_call_unthunks_inside_untrack() {
    // The legacy wrap PRECEDES thunk elision: an imported zero-arg call
    // becomes the deep-read + untracked-by-reference getter, NOT the bare
    // elided callee.
    // Oracle: $.html(node, () => ($.deep_read_state(render), $.untrack(render)));
    let js = emit(
        "<script>import { render } from './x.js';\nexport let obj;</script>\n{@html render()}\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.html(node, () => ($.deep_read_state(render), $.untrack(render)))"),
        "the legacy-wrapped call unthunks INSIDE $.untrack with the import dep:\n{js}"
    );
    assert!(
        !js.contains("$.html(node, render)"),
        "a legacy-wrapped call is never elided to the bare callee:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}
