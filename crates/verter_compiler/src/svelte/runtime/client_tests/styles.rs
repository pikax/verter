use super::*;

#[test]
fn scoped_css_smoke_artifact_matches_the_committed_runtime_fixture() {
    let source = CLIENT_BREADTH_SMOKE_FIXTURES
        .iter()
        .find_map(|(name, source)| (*name == "breadth_scoped_css").then_some(*source))
        .expect("the scoped-css breadth fixture is registered");
    let allocator = Allocator::default();
    let parsed = parse_svelte(source);
    let options = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    let module = compile_client(source, &parsed, &options, &allocator, false, false)
        .expect("the scoped-css breadth fixture compiles");
    let css = module.css.expect("external scoped CSS is published");
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/svelte-runtime-tests/test/fixtures/svelte/breadth_scoped_css.css");
    let committed = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|error| panic!("read smoke fixture {}: {error}", fixture_path.display()));
    assert_eq!(css.code, committed);
}

#[test]
fn class_directive_now_emits_set_class() {
    // a `class:` directive now EMITS the merged `$.set_class` (was a per-attribute
    // refusal previously).
    let js = emit(
        "<script>let on = $state(true);</script>\n<div onclick={() => on = !on} class:active={on}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.set_class(div, 1, '', null, classes, { active: $.get(on) })"
        )),
        "a class: directive must now emit the merged set_class:\n{js}"
    );
}

#[test]
fn style_directive_static_text_value_quotes_the_string() {
    // A `style:color="red"` (a static-TEXT directive value) folds the value as the QUOTED
    // string literal `{ color: 'red' }` — NOT a bare identifier `{ color: red }` (an
    // undefined reference). Only `style:` accepts a text value. Pinned svelte@5.56.10:
    // `$.set_style(div, '', {}, { color: 'red' })`.
    let js = emit(
        "<script>let x = $state(0);</script>\n<div style:color=\"red\" onclick={() => x++}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_style(div, '', {}, { color: 'red' })")),
        "a static-text style directive value must be a quoted string:\n{js}"
    );
    // NEGATIVE: it must NOT emit the bare (undefined) identifier.
    assert!(
        !n.contains(&nc("{ color: red }")),
        "a static-text style directive must NOT emit a bare identifier:\n{js}"
    );
}

#[test]
fn style_directive_static_text_value_in_a_spread_fold_quotes_the_string() {
    // The same static-text style directive INSIDE a spread fold folds as the quoted
    // `[$.STYLE]: { color: 'red' }`. Pinned svelte@5.56.10:
    // `$.attribute_effect(div, () => ({ ...p, [$.STYLE]: { color: 'red' } }))`.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<div {...p} style:color=\"red\"></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect(div, () => ({ ...p, [$.STYLE]: { color: 'red' } }))"
        )),
        "a static-text style directive in a spread fold must quote the string:\n{js}"
    );
    // NEGATIVE: the bare identifier form must be absent.
    assert!(
        !n.contains(&nc("[$.STYLE]: { color: red }")),
        "a static-text style directive in a spread fold must NOT emit a bare identifier:\n{js}"
    );
}

#[test]
fn class_value_paren_literal_does_not_clsx() {
    // `class={('x')}` — the class-clsx decision is computed on the UNWRAPPED root (a literal),
    // so NO `$.clsx` wrap (the behavioral fact survives the source-preserving rollback). The
    // author paren is kept verbatim (`('x')`) — a behavior-preserving cosmetic difference the
    // minifier collapses, so the value assertion is paren-insensitive.
    let js = emit(
        "<script>let a = $state(0);</script>\n<div class={('x')} onclick={() => a++}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class(div, 1,")) && n.contains(&nc("'x'")),
        "a parenthesized literal class must emit the raw literal value:\n{js}"
    );
    // NEGATIVE (the behavioral class-clsx discriminator): no clsx wrap for a literal class value.
    assert!(
        !n.contains(&nc("$.clsx")),
        "a parenthesized literal class must NOT be clsx-wrapped:\n{js}"
    );
}

#[test]
fn class_value_paren_binary_does_not_clsx() {
    // `class={((a + b))}` — the class-clsx decision sees the unwrapped binary root → NO clsx.
    // The author parens are kept verbatim (source-preserving) — paren-insensitive value
    // assertion; the behavioral discriminator is the ABSENCE of the clsx wrap.
    let js = emit(
        "<script>let a = $state(0); let b = $state(0);</script>\n<div class={((a + b))} onclick={() => { a++; b++; }}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class(div, 1,")) && n.contains(&nc("$.get(a) + $.get(b)")),
        "a parenthesized binary class must emit the raw binary value:\n{js}"
    );
    assert!(
        !n.contains(&nc("$.clsx")),
        "a parenthesized binary class must NOT be clsx-wrapped:\n{js}"
    );
}

#[test]
fn class_value_paren_template_does_not_clsx() {
    // `` class={(`x${a}`)} `` — the class-clsx decision sees the unwrapped template root → NO
    // clsx. Author parens kept verbatim (paren-insensitive value assertion).
    let js = emit(
        "<script>let a = $state(0);</script>\n<div class={(`x${a}`)} onclick={() => a++}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class(div, 1,")) && n.contains(&nc("`x${$.get(a)}`")),
        "a parenthesized template class must emit the raw template value:\n{js}"
    );
    assert!(
        !n.contains(&nc("$.clsx")),
        "a parenthesized template class must NOT be clsx-wrapped:\n{js}"
    );
}

#[test]
fn class_value_paren_conditional_does_clsx() {
    // `class={(a ? 'x' : 'y')}` — the class-clsx decision sees the unwrapped conditional root →
    // DOES clsx (the clsx-YES boundary; the behavioral fact survives). The author paren is
    // kept INSIDE the clsx arg (source-preserving) — `$.clsx(($.get(a) ? 'x' : 'y'))`, a
    // behavior-preserving cosmetic difference; the behavioral discriminator is the PRESENCE of
    // the clsx wrap around the conditional.
    let js = emit(
        "<script>let a = $state(0);</script>\n<div class={(a ? 'x' : 'y')} onclick={() => a++}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class(div, 1, $.clsx(")) && n.contains(&nc("$.get(a) ? 'x' : 'y'")),
        "a parenthesized conditional class must be clsx-wrapped around the conditional:\n{js}"
    );
}

#[test]
fn valueless_class_base_in_set_class_emits_raw_true_not_an_empty_string() {
    // A VALUELESS `class` attribute consumed as the `$.set_class` BASE value (`<div class
    // class:on={x}>`) emits the RAW boolean `true` as the base argument — NOT `''`. The
    // valueless `class` carries `value: None`, so the base is `true`, mirroring the spread
    // fold. Pinned svelte@5.56.10:
    // `$.set_class(div, 1, true, null, classes, { on: $.get(x) })`.
    let js = emit(
        "<script>let x = $state(0);</script>\n<div class class:on={x} onclick={() => x++}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.set_class(div, 1, true, null, classes, { on: $.get(x) })"
        )),
        "a valueless class base must emit the raw `true`:\n{js}"
    );
    // NEGATIVE: the empty-string base must be ABSENT (the pre-fix bug emitted `'', `).
    assert!(
        !n.contains(&nc("$.set_class(div, 1, '', null")),
        "a valueless class base must NOT emit an empty-string base:\n{js}"
    );
}

#[test]
fn valueless_style_base_in_set_style_emits_raw_true_not_an_empty_string() {
    // A VALUELESS `style` attribute consumed as the `$.set_style` BASE value (`<div style
    // style:color={x}>`) emits the RAW boolean `true` as the base argument — NOT `''`.
    // Pinned svelte@5.56.10: `$.set_style(div, true, styles, { color: $.get(x) })`.
    let js = emit(
        "<script>let x = $state(0);</script>\n<div style style:color={x} onclick={() => x++}></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_style(div, true, styles, { color: $.get(x) })")),
        "a valueless style base must emit the raw `true`:\n{js}"
    );
    // NEGATIVE: the empty-string base must be ABSENT.
    assert!(
        !n.contains(&nc("$.set_style(div, '', styles")),
        "a valueless style base must NOT emit an empty-string base:\n{js}"
    );
}

#[test]
fn class_directive_static_text_value_refuses_as_invalid_directive_value() {
    // A `class:on="x"` (a static-TEXT CLASS directive value) is an OFFICIAL COMPILE ERROR
    // (`directive_invalid_value`): a directive value must be a JS expression in curly
    // braces; ONLY `style:` accepts a static-text value. Verter must REFUSE on the
    // official-reject rail (never emit `{ on: 'x' }`). Pinned svelte@5.56.10 throws
    // `directive_invalid_value` at the parse phase.
    let err = emit_result(
        "<script>let x = $state(0);</script>\n<div class:on=\"x\" onclick={() => x++}></div>\n",
    )
    .expect_err("a static-text class directive must refuse");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected an OfficialReject refusal, got {err:?}");
    };
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::DirectiveInvalidValue,
        "a static-text class directive must reject via the DirectiveInvalidValue rule"
    );
    assert_eq!(
        rejection.official_code, "directive_invalid_value",
        "the rejection mirrors the official `directive_invalid_value` code"
    );
}

#[test]
fn svelte_element_class_directive_takes_set_class_fast_path() {
    // The official lone-class fast path fires WITH co-located `class:` directives: a
    // `<svelte:element class="card" class:active={x}>` emits the directive-object form
    // `$.set_class($$element, 0, 'card', null, {}, { active: x })` (verified against
    // pinned svelte@5.56.10), NOT an `$.attribute_effect` fold with a `[$.CLASS]` entry.
    let js = emit(
        "<script>let tag = $state('div');let x = $state(false);</script>\n<svelte:element this={tag} class=\"card\" class:active={x}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.set_class($$element, 0, 'card', null, {}, { active: x })"
        )),
        "class + class: directive takes the set_class fast path with the directive object:\n{js}"
    );
    assert!(
        !js.contains("attribute_effect"),
        "class + class: directive must NOT fold into attribute_effect:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");

    // INVERSE: a SECOND plain attribute disqualifies the fast path — the class + the
    // `[$.CLASS]` directive entry fold into `$.attribute_effect` (official parity:
    // `{ 'data-x': y, class: 'card', [$.CLASS]: { active: x } }`).
    let js2 = emit(
        "<script>let tag = $state('div');let x = $state(false);let y = $state(1);</script>\n<svelte:element this={tag} data-x={y} class=\"card\" class:active={x}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n2 = normalize_js_cosmetics(&js2);
    assert!(
        n2.contains(&nc(
            "$.attribute_effect($$element, () => ({ 'data-x': y, class: 'card', [$.CLASS]: { active: x } }))"
        )),
        "a non-lone class + directive folds into attribute_effect:\n{js2}"
    );
    assert!(
        !n2.contains(&nc("$.set_class")),
        "the non-lone case must NOT take the set_class fast path:\n{js2}"
    );
    assert!(parses_as_js(&js2), "module must be valid JS:\n{js2}");
}

#[test]
fn svelte_element_pure_class_directive_synthesizes_empty_class_base() {
    // A `class:` directive with NO class attribute synthesizes the empty class base
    // (official's analyze-phase empty-class synthesis): `$.set_class($$element, 0, '',
    // null, {}, { active: x })` — verified against pinned svelte@5.56.10. NOT a bare
    // `[$.CLASS]` attribute_effect fold.
    let js = emit(
        "<script>let tag = $state('div');let x = $state(false);</script>\n<svelte:element this={tag} class:active={x}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.set_class($$element, 0, '', null, {}, { active: x })"
        )),
        "a pure class: directive synthesizes the empty class base:\n{js}"
    );
    assert!(
        !js.contains("attribute_effect"),
        "a pure class: directive must NOT fold into attribute_effect:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_element_mixed_case_class_takes_set_class_fast_path() {
    // Official matches the plain `class` attribute NAME case-insensitively
    // (`SvelteElement.js`: `attributes[0].name.toLowerCase() === 'class'`), so a
    // mixed-case `<svelte:element CLASS="card">` takes the lone-class fast path
    // `$.set_class($$element, 0, 'card')` — verified against pinned svelte@5.56.10 —
    // NOT an `$.attribute_effect` fold carrying a case-preserved `CLASS: 'card'`.
    let js = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} CLASS=\"card\">hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class($$element, 0, 'card')")),
        "a lone mixed-case CLASS takes the set_class fast path:\n{js}"
    );
    assert!(
        !js.contains("attribute_effect"),
        "a lone mixed-case CLASS must NOT fold into attribute_effect:\n{js}"
    );
    assert!(
        !js.contains("CLASS: 'card'"),
        "a lone mixed-case CLASS must NOT emit a case-preserved generic fold entry:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");

    // WITH a co-located `class:` directive the fast path still fires (official passes
    // the class directives into `build_set_class`): `$.set_class($$element, 0, 'card',
    // null, {}, { active: x })` — and NO spurious analyze-phase `class: ''` synthesis
    // (official's `has_class ||= attribute.name.toLowerCase() === 'class'` sees the
    // mixed-case attribute).
    let js2 = emit(
        "<script>let tag = $state('div');let x = $state(false);</script>\n<svelte:element this={tag} CLASS=\"card\" class:active={x}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n2 = normalize_js_cosmetics(&js2);
    assert!(
        n2.contains(&nc(
            "$.set_class($$element, 0, 'card', null, {}, { active: x })"
        )),
        "mixed-case CLASS + class: directive takes the set_class fast path with the directive object:\n{js2}"
    );
    assert!(
        !js2.contains("attribute_effect"),
        "mixed-case CLASS + class: directive must NOT fold into attribute_effect:\n{js2}"
    );
    assert!(
        !n2.contains(&nc("class: ''")),
        "the mixed-case CLASS suppresses the empty-class synthesis:\n{js2}"
    );
    assert!(parses_as_js(&js2), "module must be valid JS:\n{js2}");
}

#[test]
fn regular_element_mixed_case_class_merges_into_set_class_base() {
    // The regular-element surface shares the same case-insensitive class-name rule
    // (official normalizes HTML attribute names via `get_attribute_name` →
    // `normalize_attribute` before routing): `<div CLASS="card" class:active={x}>`
    // emits `$.set_class(div, 1, 'card', null, {}, { active: x })` with the class
    // PULLED OUT of the skeleton (`<div>hi</div>`) — verified against pinned
    // svelte@5.56.10.
    let js = emit(
        "<script>let x = $state(false);</script>\n<div CLASS=\"card\" class:active={x}>hi</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class(div, 1, 'card', null, {}, { active: x })")),
        "mixed-case CLASS + class: directive merges into the set_class base:\n{js}"
    );
    // The base is pulled OUT of the cloned skeleton — no baked class attribute (which
    // would double-apply the class), and no case-preserved generic entry anywhere.
    assert!(
        js.contains("$.from_html(`<div>hi</div>`)"),
        "the class base must be pulled out of the skeleton:\n{js}"
    );
    assert!(
        !js.contains("CLASS: 'card'") && !js.contains("class=\"card\""),
        "the mixed-case CLASS must NOT bake into the skeleton or fold as a generic entry:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn regular_element_uppercase_dynamic_class_pins_divergent_emission() {
    // PINS A DIVERGENCE: official svelte@5.56.10 normalizes the HTML attribute name
    // (`get_attribute_name` → `normalize_attribute`, lowercase) BEFORE routing, so a
    // lone `<div CLASS={k}>` emits a SINGLE `$.set_class(div, 1, k)` — with NO
    // `$.clsx` (the clsx wrap is case-dependent: lowercase `class={k}` gets it,
    // uppercase does not) and NO `$.set_attribute`. Verter's dynamic-attr routing
    // recognizes only the lowercase spelling as the class channel, so the uppercase
    // name takes the generic path: `$.set_attribute(div, 'class', k)` (the NAME
    // lowercases at emission, but the routing decision already missed `$.set_class`).
    // This is a TEMPORARY non-parity divergence owned by the general typed-setter/parser
    // class/style attribute-identity + emission-routing layer and tracked as
    // debt-ledger row D-37 (.claude/skills/compiler-codegen/SKILL.md). This test pins
    // the current divergent shape and MUST fail (go RED) when that convergence lands.
    let js = emit(
        "<script>let k = $state('x');</script>\n<div CLASS={k}>hi</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_attribute(div, 'class', k)")),
        "a lone dynamic uppercase CLASS currently routes to the generic $.set_attribute channel:\n{js}"
    );
    assert!(
        !js.contains("$.set_class("),
        "a lone dynamic uppercase CLASS currently misses the $.set_class routing (official emits $.set_class(div, 1, k)):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");

    // Co-located with a `class:` directive, the divergence DOUBLE-EMITS: the merge
    // into the `$.set_class` base fires (with the lowercase-authored `$.clsx` wrap),
    // AND a redundant generic `$.set_attribute` survives for the same attribute.
    // Official emits ONLY the merged `$.set_class`.
    let js2 = emit(
        "<script>let c = $state('x');let x = $state(true);</script>\n<div CLASS={c} class:active={x}>hi</div>\n",
        "App.svelte",
    );
    let n2 = normalize_js_cosmetics(&js2);
    assert!(
        n2.contains(&nc("$.set_attribute(div, 'class', c)")),
        "the co-located uppercase CLASS currently keeps a redundant $.set_attribute:\n{js2}"
    );
    assert!(
        n2.contains(&nc(
            "$.set_class(div, 1, $.clsx(c), null, {}, { active: x })"
        )),
        "the class: directive merge still emits the $.set_class base:\n{js2}"
    );
    assert!(parses_as_js(&js2), "module must be valid JS:\n{js2}");
}

#[test]
fn regular_element_uppercase_static_lone_class_pins_fail_closed() {
    // PINS A DIVERGENCE: official svelte@5.56.10 ACCEPTS a static lone uppercase
    // `<div CLASS="card">` and bakes the LOWERCASED `class="card"` into the static
    // skeleton (`<div class="card">hi</div>`). Verter's static-attribute allowlist is
    // case-SENSITIVE, so the unrecognized uppercase spelling falls through to the
    // dynamic-attr classifier and FAILS CLOSED as the `DynamicAttribute` surface with
    // the authored name. This fail-close is a TEMPORARY non-parity divergence owned
    // by the general typed-setter/parser static-attribute identity/serializer layer and tracked
    // as debt-ledger row D-37 (.claude/skills/compiler-codegen/SKILL.md). This test
    // pins the current refusal and MUST fail (go RED — start accepting) when that
    // convergence lands. (The fixture carries a rune so mode inference lands on runes
    // mode; a script-less carrier is legacy mode, whose per-surface dispatch owns it.)
    let result = emit_result("<script>let k = $state(0);</script>\n<div CLASS=\"card\">hi</div>\n");
    assert!(
        matches!(
            &result,
            Err(ClientCompileError::Unsupported(
                UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. }
            )) if name == "CLASS"
        ),
        "a static lone uppercase CLASS currently fails closed as the DynamicAttribute surface with the authored name, got {result:?}"
    );

    // POSITIVE CONTRAST: the lowercase spelling is the supported static-class path —
    // it bakes into the cloned skeleton with no runtime class helper at all.
    let js = emit(
        "<script>let k = $state(0);</script>\n<div class=\"card\">hi</div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.from_html(`<div class=\"card\">hi</div>`)"),
        "the lowercase static class bakes into the skeleton:\n{js}"
    );
    assert!(
        !js.contains("$.set_class(") && !js.contains("$.set_attribute("),
        "a baked static class needs no runtime class helper:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_element_mixed_case_style_attr_suppresses_empty_style_synthesis() {
    // Official's analyze-phase empty-style synthesis checks the attribute name
    // case-insensitively (`has_style ||= attribute.name.toLowerCase() === 'style'`), so
    // `<svelte:element STYLE="background: blue" style:color={c}>` folds WITHOUT a
    // synthesized `style: ''` entry: `$.attribute_effect($$element, () => ({ STYLE:
    // 'background: blue', [$.STYLE]: { color: c } }))` — the generic entry keeps the
    // authored case — verified against pinned svelte@5.56.10.
    let js = emit(
        "<script>let tag = $state('div');let c = $state('red');</script>\n<svelte:element this={tag} STYLE=\"background: blue\" style:color={c}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect($$element, () => ({ STYLE: 'background: blue', [$.STYLE]: { color: c } }))"
        )),
        "mixed-case STYLE folds with the authored case and the style-directive entry:\n{js}"
    );
    assert!(
        !n.contains(&nc("style: ''")),
        "the mixed-case STYLE suppresses the empty-style synthesis:\n{js}"
    );
    assert!(
        !js.contains("set_style"),
        "a <svelte:element> style surface folds — it never emits $.set_style:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn uppercase_style_directive_prefix_is_not_a_style_directive() {
    // NEGATIVE RAIL: the DIRECTIVE prefix stays case-SENSITIVE — official recognizes
    // only lowercase `style:`. An uppercase `STYLE:color={c}` is NOT a style directive
    // in either compiler. Official ACCEPTS it and FOLDS it as a GENERIC attribute named
    // `STYLE:color`; Verter's directive-kind table is lowercase-only, so the
    // unknown-directive candidate FAILS CLOSED (never a `[$.STYLE]` / `$.set_style`
    // emission). This fail-close is a TEMPORARY non-parity divergence — official accepts
    // the generic-attribute fold, Verter refuses — owned by the general typed-setter/parser
    // attribute-name case-normalization follow-up and tracked as debt-ledger row D-37
    // (.claude/skills/compiler-codegen/SKILL.md). It is NOT official parity. The
    // conservative refusal is fail-close-safe until that convergence lands.
    match emit_result(
        "<script>let tag = $state('div');let c = $state('red');</script>\n<svelte:element this={tag} STYLE:color={c}>hi</svelte:element>\n",
    ) {
        Err(ClientCompileError::Lowering(errs)) => {
            assert!(
                errs.diagnostics
                    .iter()
                    .any(|d| d.code == "svelte-runtime-unknown-directive"),
                "an uppercase STYLE: prefix must fail closed as an unknown directive:\n{errs:?}"
            );
        }
        Ok(js) => {
            panic!("an uppercase STYLE: prefix must NOT compile as a style directive:\n{js}")
        }
        Err(other) => panic!("expected an unknown-directive lowering error, got: {other:?}"),
    }

    // POSITIVE CONTRAST: the lowercase `style:` prefix IS the style directive.
    let js = emit(
        "<script>let tag = $state('div');let c = $state('red');</script>\n<svelte:element this={tag} style:color={c}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("[$.STYLE]: { color: c }")),
        "the lowercase style: prefix is the style directive:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_element_style_directive_synthesizes_empty_style_fold_entry() {
    // A `style:` directive synthesizes the empty `style` attribute (official's
    // analyze-phase synthesis), which routes the element to the FOLD even when the only
    // real attribute is a static-text class: `$.attribute_effect($$element, () => ({
    // class: 'x', style: '', [$.STYLE]: { color: c } }))` — verified against pinned
    // svelte@5.56.10. The lone-class set_class fast path must NOT fire.
    let js = emit(
        "<script>let tag = $state('div');let c = $state('red');</script>\n<svelte:element this={tag} class=\"x\" style:color={c}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect($$element, () => ({ class: 'x', style: '', [$.STYLE]: { color: c } }))"
        )),
        "class + style: directive folds with the synthesized empty style entry:\n{js}"
    );
    assert!(
        !n.contains(&nc("$.set_class")),
        "a style: directive must disqualify the set_class fast path:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");

    // A PURE `style:` directive (no plain attrs) still synthesizes the empty style
    // entry: `{ style: '', [$.STYLE]: { color: c } }` (official parity).
    let js2 = emit(
        "<script>let tag = $state('div');let c = $state('red');</script>\n<svelte:element this={tag} style:color={c}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n2 = normalize_js_cosmetics(&js2);
    assert!(
        n2.contains(&nc(
            "$.attribute_effect($$element, () => ({ style: '', [$.STYLE]: { color: c } }))"
        )),
        "a pure style: directive folds with the synthesized empty style entry:\n{js2}"
    );
    assert!(parses_as_js(&js2), "module must be valid JS:\n{js2}");
}

#[test]
fn provable_external_style_compiles_with_scoped_css_artifact() {
    // A PROVABLE external-mode `<style>` compiles: the module bakes the scope
    // class into the matched element's skeleton, and the scoped `css.code` +
    // hash publish as the module's EXTERNAL css artifact. `App.svelte` hashes
    // to `svelte-n50uah` (the pinned djb2 vector).
    let module = module_result(
        "<script>let c = $state(0);</script>\n<style>.r{color:red}\nbutton{padding:0}</style>\n<button class=\"r\" onclick={() => c++}>{c}</button>\n",
    )
    .expect("a provable external style compiles");
    // The STATIC injection site: the skeleton's literal class carries the hash.
    assert!(
        module.code.contains("<button class=\"r svelte-n50uah\">"),
        "the skeleton bakes the scope class into the static class literal:\n{}",
        module.code
    );
    // The EXTERNAL routing: the scoped stylesheet + the SAME hash on the artifact.
    let css = module.css.as_ref().expect("an external css artifact");
    assert_eq!(css.hash, "svelte-n50uah");
    assert!(
        css.code.contains(".r.svelte-n50uah{color:red}"),
        "the class rule is scope-classed: {}",
        css.code
    );
    assert!(
        css.code.contains("button.svelte-n50uah{padding:0}"),
        "the type rule is scope-classed: {}",
        css.code
    );
    // NEGATIVE: external mode inlines nothing — no `$$css`, no `$.append_styles`.
    assert!(!module.code.contains("$$css"), "{}", module.code);
    assert!(!module.code.contains("$.append_styles"), "{}", module.code);
}

#[test]
fn external_css_artifact_carries_has_global_and_the_demanded_source_map() {
    // The artifact is the plan-mandated `{ hash, code, map, has_global }`
    // payload: `:global(...)` css marks `has_global`, and the
    // `want_source_map` demand rides `compile_client` into the artifact's
    // `source_map`.
    let source = "<script>let c = $state(0);</script>\n<style>.r{color:red}\n:global(.x){margin:0}</style>\n<button class=\"r\" onclick={() => c++}>{c}</button>\n";
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    let module = compile_client(source, &parsed, &opts, &alloc, false, true)
        .expect("a provable global+scoped style compiles");
    let css = module.css.as_ref().expect("an external css artifact");
    assert!(
        css.has_global,
        "`:global(.x)` css must mark the artifact has_global"
    );
    let map = css
        .source_map
        .as_deref()
        .expect("the demanded css source map rides the artifact");
    let parsed_map =
        oxc_sourcemap::OwnedSourceMap::from_json_string(map).expect("valid source-map JSON");
    assert_eq!(
        parsed_map.get_sources().collect::<Vec<_>>(),
        ["App.svelte"],
        "the css map names the component source"
    );
}

#[test]
fn css_analysis_failure_beats_a_template_lowering_failure() {
    // The css-first diagnostic order: a component whose css body FAILS the
    // scoping analysis AND whose template fails to lower (the `readonly`
    // param modifier trips the expr-parse lowering channel) reports the css
    // failure (the analysis runs before lowering). NEGATIVE: never the
    // lowering error, never the selector surface.
    let err = emit_result(
        "<script>let value = $state(0);</script>\n<style>.a :global(.x) .b { color: red }</style>\n<input bind:value={() => value, (readonly x) => value = x} />\n",
    )
    .expect_err("a bad css body + an unlowerable template must not compile");
    let ClientCompileError::Unsupported(surface) = &err else {
        panic!("expected the css-analysis refusal, got {err:?}");
    };
    assert!(
        matches!(
            surface,
            UnsupportedSvelteRuntimeSurface::StyleCssAnalysis { .. }
        ),
        "the css-analysis failure is reported FIRST, got {surface:?}"
    );
}

#[test]
fn style_analysis_failure_fails_closed_with_the_css_analysis_surface() {
    // A css body the scoping analyzer REJECTS (`:global.x` — the official
    // `css_global_block_invalid_modifier_start`) refuses on the ANALYSIS
    // surface, distinct from the clean-analyzed selector-emission refusal.
    // (The body PARSES clean, so the official-reject CSS body-parse gate does
    // not fire first.)
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<style>:global.x { color: red; }</style>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::StyleCssAnalysis { .. }),
    );
}

// ─── the SCOPED `<svelte:element>` dynamic-element injection family ─────────
//
// Official svelte@5.56.10 scopes a dynamic element exactly like a regular
// element: the analyze pass synthesizes an empty `class` for a scoped node
// with no spread and no class attribute (`phases/2-analyze/index.js`), the
// lone synthetic class routes to `$.set_class($$element, 0, …)` with the hash
// folded per the `build_set_class` 3-way, and the `$.attribute_effect` fold
// carries the hash as its OFFICIAL 6th positional argument
// (`build_attribute_effect` — `shared/element.js`). Every test pins the exact
// emitted argument topology (`App.svelte` ⇒ the pinned djb2 hash
// `svelte-n50uah`) plus a NEGATIVE (no hash for the non-scoped control, no
// stray second route).

#[test]
fn scoped_svelte_element_without_attrs_emits_the_bare_hash_set_class() {
    // A SCOPED class-less `<svelte:element>` synthesizes the empty class and
    // takes the lone-class fast path — the empty literal becomes the bare
    // hash. Official: `$.set_class($$element, 0, 'svelte-n50uah');`.
    let js = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag}>x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class($$element, 0, 'svelte-n50uah');")),
        "a scoped class-less <svelte:element> takes the set_class fast path with the bare hash:\n{js}"
    );
    // NEGATIVE: the lone synthetic class never folds — no attribute_effect.
    assert!(
        !js.contains("$.attribute_effect"),
        "the lone synthetic class routes to set_class, not the fold:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn scoped_svelte_element_with_static_attr_folds_synthetic_class_and_hash_sixth_arg() {
    // A SCOPED `<svelte:element>` WITH another attribute folds: the synthetic
    // `class: ''` appends AFTER the real attributes and the hash rides the
    // official 6th positional `$.attribute_effect` argument (the intermediate
    // sync/async/blockers slots are `void 0`). Official:
    // `$.attribute_effect($$element, () => ({ id: 'x', class: '' }), void 0,
    // void 0, void 0, 'svelte-n50uah');`.
    let js = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} id=\"x\">x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect($$element, () => ({ id: 'x', class: '' }), void 0, void 0, void 0, 'svelte-n50uah');"
        )),
        "the fold appends the synthetic class and threads the hash as the 6th arg:\n{js}"
    );
    // NEGATIVE: no set_class (the fold consumed the synthetic), and the class
    // entry is EMPTY (the hash lands in the 6th arg, never the class value).
    assert!(!js.contains("$.set_class"), "no second class route:\n{js}");
    assert!(
        !n.contains(&nc("class: 'svelte-n50uah'")),
        "the hash never folds into the class VALUE on the fold route:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn scoped_svelte_element_spread_threads_hash_without_synthesizing_class() {
    // A SCOPED spread `<svelte:element>`: the spread suppresses the synthetic
    // class (official `!has_spread` guard — the runtime spread path appends
    // the hash itself) and the hash rides the 6th argument. Official:
    // `$.attribute_effect($$element, () => ({ ...$$props.p }), void 0, void 0,
    // void 0, 'svelte-n50uah');`.
    let js = emit(
        "<script>let tag = $state('div');\nlet { p } = $props();</script>\n<svelte:element this={tag} {...p}>x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect($$element, () => ({ ...$$props.p }), void 0, void 0, void 0, 'svelte-n50uah');"
        )),
        "the scoped spread fold threads the hash as the 6th arg:\n{js}"
    );
    // NEGATIVE: no synthetic class beside a spread.
    assert!(
        !n.contains(&nc("class: ''")),
        "a spread suppresses the synthetic class:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn scoped_svelte_element_class_directive_merges_hash_into_set_class_value() {
    // A SCOPED `<svelte:element>` with a lone `class:` directive: the
    // synthesized empty class becomes the bare hash VALUE and the directive
    // rides the `next` object with the official `null` css_hash placeholder +
    // `{}` prev. Official (demoted non-reactive `on`):
    // `$.set_class($$element, 0, 'svelte-n50uah', null, {}, { on });`.
    let js = emit(
        "<script>let tag = $state('div');\nlet on = $state(false);</script>\n<svelte:element this={tag} class:on>x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.set_class($$element, 0, 'svelte-n50uah', null, {}, { on });"
        )),
        "the scoped class-directive set_class folds the hash into the value:\n{js}"
    );
    assert!(!js.contains("$.attribute_effect"), "no fold route:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn scoped_svelte_element_valueless_class_folds_boolean_true_with_hash_arg() {
    // A VALUELESS `class` on a scoped `<svelte:element>` is the RAW boolean
    // `class: true` (has_class ⇒ NO synthesis; not a text attribute ⇒ the fold
    // route), so the hash CANNOT fold into the value — it rides the 6th arg.
    // Official: `$.attribute_effect($$element, () => ({ class: true }), void 0,
    // void 0, void 0, 'svelte-n50uah');`.
    let js = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} class>x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect($$element, () => ({ class: true }), void 0, void 0, void 0, 'svelte-n50uah');"
        )),
        "a valueless class folds as `true` with the hash in the 6th arg:\n{js}"
    );
    assert!(!js.contains("$.set_class"), "no set_class route:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn scoped_svelte_element_empty_text_class_takes_the_bare_hash_set_class() {
    // An EMPTY-text `class=""` on a scoped `<svelte:element>` is a lone text
    // class ⇒ the fast path; the empty literal becomes the bare hash (the
    // official `value === '' → value = hash` arm). Official:
    // `$.set_class($$element, 0, 'svelte-n50uah');`.
    let js = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} class=\"\">x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class($$element, 0, 'svelte-n50uah');")),
        "an empty text class becomes the bare hash:\n{js}"
    );
    assert!(!js.contains("$.attribute_effect"), "no fold route:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn scoped_svelte_element_lone_text_class_appends_hash_to_the_literal() {
    // A NON-empty lone text class on a scoped `<svelte:element>` appends
    // ` <hash>` to the literal (the official literal arm). Official:
    // `$.set_class($$element, 0, 'known svelte-n50uah');`.
    let js = emit(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} class=\"known\">x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.set_class($$element, 0, 'known svelte-n50uah');")),
        "the literal class appends the hash:\n{js}"
    );
    assert!(!js.contains("$.attribute_effect"), "no fold route:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn scoped_svelte_element_dynamic_class_folds_raw_with_hash_sixth_arg() {
    // A DYNAMIC `class={cls}` on a scoped `<svelte:element>` folds RAW (the
    // official fold rule — no `$.clsx` wrap in the fold) and the hash rides
    // the 6th argument (never the value). Official:
    // `$.attribute_effect($$element, () => ({ class: cls }), void 0, void 0,
    // void 0, 'svelte-n50uah');`.
    let js = emit(
        "<script>let tag = $state('div');\nlet cls = $state('a');</script>\n<svelte:element this={tag} class={cls}>x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect($$element, () => ({ class: cls }), void 0, void 0, void 0, 'svelte-n50uah');"
        )),
        "a dynamic class folds raw with the hash in the 6th arg:\n{js}"
    );
    assert!(!js.contains("$.set_class"), "no set_class route:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn scoped_svelte_element_style_directive_synthesizes_class_before_style() {
    // A SCOPED `<svelte:element>` with a lone `style:` directive synthesizes
    // BOTH the scoped empty class AND the directive empty style, in the
    // official analyze order (class BEFORE style), then the `[$.STYLE]` entry
    // — with the hash 6th. Official: `$.attribute_effect($$element, () =>
    // ({ class: '', style: '', [$.STYLE]: { color: c } }), void 0, void 0,
    // void 0, 'svelte-n50uah');`.
    let js = emit(
        "<script>let tag = $state('div');\nlet c = $state('red');</script>\n<svelte:element this={tag} style:color={c}>x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect($$element, () => ({ class: '', style: '', [$.STYLE]: { color: c } }), void 0, void 0, void 0, 'svelte-n50uah');"
        )),
        "the scoped style-directive fold synthesizes class BEFORE style with the hash 6th:\n{js}"
    );
    assert!(!js.contains("$.set_class"), "no set_class route:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn scoped_svelte_element_class_directive_beside_attr_folds_with_hash() {
    // A SCOPED `<svelte:element>` with a `class:` directive BESIDE another
    // attribute: the fold appends the synthetic `class: ''` after the real
    // attributes, then the `[$.CLASS]` directive object, hash 6th. Official:
    // `$.attribute_effect($$element, () => ({ id: 'x', class: '', [$.CLASS]:
    // { on } }), void 0, void 0, void 0, 'svelte-n50uah');`.
    let js = emit(
        "<script>let tag = $state('div');\nlet on = $state(false);</script>\n<svelte:element this={tag} id=\"x\" class:on>x</svelte:element>\n<style>div { color: blue; }</style>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect($$element, () => ({ id: 'x', class: '', [$.CLASS]: { on } }), void 0, void 0, void 0, 'svelte-n50uah');"
        )),
        "the fold orders real attrs, synthetic class, [$.CLASS], hash 6th:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn style_refusal_surfaces_pin_their_diagnostic_codes() {
    // The three precise style surfaces carry machine-stable diagnostic ids:
    // the two plan-failure surfaces CARRY the failure's precise code (the
    // official css code for an analysis/render failure; the fixed selector
    // id for a matcher refusal), and the pre-plan mode surface keeps its own.
    let span = verter_span::Span::new(0, 0);
    assert_eq!(
        UnsupportedSvelteRuntimeSurface::StyleCssAnalysis {
            code: "css_global_invalid_placement",
            span
        }
        .diagnostic_code(),
        "css_global_invalid_placement"
    );
    assert_eq!(
        UnsupportedSvelteRuntimeSurface::StyleSelectorUnsupported {
            code: "svelte-runtime-unsupported-style-selector",
            span,
            construct: Some("a legacy `<slot>` element")
        }
        .diagnostic_code(),
        "svelte-runtime-unsupported-style-selector"
    );
    assert_eq!(
        UnsupportedSvelteRuntimeSurface::StyleCssModeUnsupported { span }.diagnostic_code(),
        "svelte-runtime-unsupported-style-css-mode"
    );
}

#[test]
fn style_css_analysis_refusal_carries_the_precise_css_code_and_span() {
    // Code/span propagation (analysis failure): the css-analysis refusal
    // carries the PRECISE official css code (`css_global_invalid_placement`)
    // and the offending construct's exact span, threaded unchanged from the
    // typed plan failure through `compile_client` into the diagnostic surface.
    // NEGATIVE: never the generic fixed `svelte-runtime-unsupported-style-css-analysis`.
    let source = "<script>let c = $state(0);</script>\n<style>.a :global(.x) .b { color: red }</style>\n<button onclick={() => c++}>{c}</button>\n";
    let err = emit_result(source).expect_err("an invalid :global placement must not compile");
    let ClientCompileError::Unsupported(surface) = err else {
        panic!("expected the css-analysis refusal, got {err:?}");
    };
    assert!(
        matches!(
            surface,
            UnsupportedSvelteRuntimeSurface::StyleCssAnalysis { .. }
        ),
        "the analysis failure refuses on the css-analysis surface: {surface:?}"
    );
    assert_eq!(surface.diagnostic_code(), "css_global_invalid_placement");
    let global = source.find(":global(.x)").unwrap() as u32;
    assert_eq!(
        surface.span(),
        verter_span::Span::new(global, global + ":global(.x)".len() as u32),
        "the refusal span is the offending `:global(...)` node's own span"
    );
}

#[test]
fn custom_element_records_the_injected_css_mode() {
    // A custom element ALWAYS injects its styles (the official `inject_styles =
    // css === 'injected' || is_custom_element`): the resolved descriptor RECORDS
    // the mode for the style pipeline. Style compilation itself stays
    // fail-closed until the CSS vertical lands — see
    // `custom_element_with_style_block_still_fails_closed`.
    let source = "<svelte:options customElement=\"x-css\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let alloc = Allocator::default();
    let parsed = crate::svelte::parser::parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    let ir = crate::svelte::runtime::lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc)
        .expect("a customElement component lowers");
    let descriptor = ir
        .component
        .custom_element
        .as_ref()
        .expect("the descriptor is retained");
    assert!(
        descriptor.inject_styles,
        "the resolved customElement CSS mode records style injection"
    );
}

#[test]
fn custom_element_with_style_block_injects_its_css() {
    // A customElement ALWAYS injects its styles (the official `inject_styles =
    // css === 'injected' || is_custom_element` rule): the compiled module
    // carries the `$$css` hoist + the `$.append_styles` prelude alongside the
    // custom-element epilogue, and NO external css artifact.
    let module = module_result(
        "<svelte:options customElement=\"x-css\" />\n<script>let c = $state(0);</script>\n<style>p { color: red; }</style>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a customElement with a style compiles down the injected route");
    assert!(
        module.code.contains("$.append_styles($$anchor, $$css);"),
        "the injected prelude is emitted:\n{}",
        module.code
    );
    assert!(
        module
            .code
            .contains("const $$css = { hash: 'svelte-n50uah', code:"),
        "the $$css object hoists with the scope hash:\n{}",
        module.code
    );
    assert!(
        module.code.contains("$.create_custom_element"),
        "the custom-element epilogue survives:\n{}",
        module.code
    );
    // NEGATIVE: no external artifact; the unused `p` rule is comment-pruned in
    // the inlined payload, not published separately.
    assert!(module.css.is_none());
}

#[test]
fn attr_class_style_module_matches_the_committed_jsdom_smoke_fixture() {
    // The dynamic-attribute / class / style behavioral fixture (a dynamic attr + dynamic class + a static-base
    // style with a `style:` directive, all reactive in ONE combined effect) stays
    // equivalent to `compile_client`'s output.
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet id = $state('a');\n\tlet cls = $state('box');\n\tlet color = $state('red');\n</script>\n\n<button onclick={() => { id += '!'; cls += ' on'; color = 'blue'; }} id={id} class={cls} style=\"font-weight:bold\" style:color={color}>go</button>\n",
        "attr_class_style.client.mjs",
    );
}

#[test]
fn mixed_class_call_module_matches_the_committed_jsdom_smoke_fixture() {
    // The mixed-class-with-a-call behavioral fixture (`class="a{String(c)}b"`) — the
    // base memoizes the EXPRESSION PART (the `String(c)` call → a `$0` dep, the
    // `` `a${$0 ?? ''}b` `` template in the body), and on a delegated click the class
    // re-renders. Stays equivalent to `compile_client`'s output, so the jsdom smoke
    // can never drift from the per-part memoization codegen.
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet c = $state('x');\n</script>\n\n<button onclick={() => c += '!'} class=\"a{String(c)}b\">go</button>\n",
        "mixed_class_call.client.mjs",
    );
}

#[test]
fn instance_top_level_class_is_preserved() {
    // An ordinary class retains source order and private-field syntax.
    let js = emit(
        "<script>let count = $state(0); class C { #x = 0; bump() { this.#x++; } }</script>\n<button onclick={() => count++}>{count}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("class C { #x = 0; bump() { this.#x++; } }"),
        "ordinary class missing:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn private_field_update_inside_a_class_method_is_preserved() {
    // Private-field updates inside an admitted ordinary class remain valid
    // JavaScript and are not misclassified as component reactive writes.
    let js = emit(
        "<script>let n = $state(0); class C { #x = 0; bump() { this.#x++; } }</script>\n<button onclick={() => n++}>{n}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("this.#x++"),
        "private-field update missing:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn reserved_word_class_element_tag_fails_closed() {
    // `<class>` → `var class = root();` is likewise invalid JS — fail closed.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<class></class>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ElementName { .. }),
    );
}

#[test]
fn class_directive_only_emits_empty_base_and_null_hash() {
    // `class:foo={on}` (no base) → `$.set_class(div, 1, '', null, classes, { foo: … })`.
    let src = "<script>let on = $state(false);</script>\n<div onclick={() => on = !on} class:foo={on}></div>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "classes = $.set_class(div, 1, '', null, classes, { foo: $.get(on) })"
        )),
        "a directive-only class must emit base '' and css_hash null:\n{js}"
    );
}

#[test]
fn style_base_with_directive_merges_into_set_style() {
    // `style="font-weight:bold" style:color={color}` → one merged set_style with the
    // base value + the directive object, using the `let styles;` accumulator.
    let src = "<script>let color = $state('red');</script>\n<button onclick={() => color = 'blue'} style=\"font-weight:bold\" style:color={color}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("let styles;")),
        "a reactive style directive needs the accumulator:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "styles = $.set_style(button, 'font-weight:bold', styles, { color: $.get(color) })"
        )),
        "the merged set_style shape (base/styles/directives) is wrong:\n{js}"
    );
    assert!(
        !js.contains("$.from_html(`<button style=\"font-weight:bold\""),
        "a style with a directive must pull the base style OUT of the skeleton:\n{js}"
    );
}

#[test]
fn style_important_modifier_wraps_in_normal_important_array() {
    // `style="display:block" style:--x={x} style:color|important={color}` → the 4th arg
    // is a `[normal, important]` array.
    let src = "<script>let x = $state('1'); let color = $state('red');</script>\n<button onclick={() => { x += '1'; color = 'blue'; }} style=\"display:block\" style:--x={x} style:color|important={color}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "styles = $.set_style(button, 'display:block', styles, [{ '--x': $.get(x) }, { color: $.get(color) }])"
        )),
        "the |important modifier must split into a [normal, important] array:\n{js}"
    );
}

#[test]
fn class_and_style_shorthand_directives_synthesize_the_implied_identifier() {
    // A SHORTHAND `class:active` / `style:color` (no `={…}`) synthesizes the implied
    // same-named identifier as the condition / value, so the merged call carries
    // `{ active: $.get(active) }` / `{ color: $.get(color) }`.
    let class_js = emit(
        "<script>let active = $state(false);</script>\n<div onclick={() => active = !active} class:active></div>\n",
        "App.svelte",
    );
    assert!(
        normalize_js_cosmetics(&class_js).contains(&nc(
            "$.set_class(div, 1, '', null, classes, { active: $.get(active) })"
        )),
        "a `class:active` shorthand must synthesize the `active` condition:\n{class_js}"
    );
    let style_js = emit(
        "<script>let color = $state('red');</script>\n<div onclick={() => color = 'blue'} style:color></div>\n",
        "App.svelte",
    );
    assert!(
        normalize_js_cosmetics(&style_js)
            .contains(&nc("$.set_style(div, '', styles, { color: $.get(color) })")),
        "a `style:color` shorthand must synthesize the `color` value:\n{style_js}"
    );
}

#[test]
fn mixed_style_directive_important_uses_the_array_form() {
    // `<div style:color|important="a{x}b">` → the `[normal, important]` array form
    // `$.set_style(div, '', styles, [{}, { color: `a${$.get(x) ?? ''}b` }])`.
    let js = emit(
        "<script>let x = $state(0);</script>\n<div style:color|important=\"a{x}b\"><button onclick={() => x++}>b</button></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.set_style(div, '', styles, [{}, { color: `a${$.get(x) ?? ''}b` }])"
        )),
        "an `|important` mixed-style directive must use the array form:\n{js}"
    );
}

#[test]
fn mixed_style_directive_under_a_spread_folds_into_the_style_object() {
    // `<div {...props} style:color="a{x}b">` → `[$.STYLE]: { color: `a${$.get(x) ?? ''}b` }`
    // (the free `props` demotes to a bare spread; the reassigned `$state x` is reactive).
    let js = emit(
        "<script>let x = $state(0);</script>\n<div {...props} style:color=\"a{x}b\"><button onclick={() => x++}>b</button></div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("[$.STYLE]: { color: `a${$.get(x) ?? ''}b` }")),
        "a mixed-style directive under a spread must fold into the [$.STYLE] object:\n{js}"
    );
}

#[test]
fn mixed_class_directive_value_rejects_as_directive_invalid_value() {
    // `class:on="a{x}b"` is NOT a style directive, so a multi-chunk mixed value is the
    // official `directive_invalid_value` reject (only `style:` accepts a text-ish value).
    let err = emit_result(
        "<script>let x = $state(0);</script>\n<div class:on=\"a{x}b\"><button onclick={() => x++}>b</button></div>\n",
    )
    .expect_err("a mixed class-directive value must fail closed");
    assert!(
        matches!(
            err,
            ClientCompileError::OfficialReject(rej) if rej.rule == CoreOfficialValidationRule::DirectiveInvalidValue
        ),
        "a mixed class-directive value must reject as DirectiveInvalidValue:\n{err:?}"
    );
}

#[test]
fn legacy_class_single_base_call_wraps_inside_clsx_and_memoizes() {
    // Oracle: $.template_effect(($0) => $.set_class(div, 1, $0), [
    //   () => $.clsx(($.deep_read_state(obj()), $.untrack(() => obj().m())))
    // ]);
    let js = emit(
        &format!("{LEGACY_OBJ}<div class={{obj.m()}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("$.clsx(({}))", obj_wrap("obj().m()"))),
        "the authored class base wraps INSIDE the synthesized $.clsx:\n{js}"
    );
    assert!(
        js.contains("$.set_class(div, 1, $0)"),
        "the memoized clsx whole lands in the $0 slot:\n{js}"
    );
    // NEGATIVE: no raw (unwrapped) clsx dep, and the wrap is never applied
    // AROUND the synthesized $.clsx composite.
    assert!(
        !js.contains("=> $.clsx(obj().m())"),
        "a definite-legacy class call dep must not stay raw:\n{js}"
    );
    assert!(
        !js.contains("$.untrack(() => $.clsx("),
        "the wrap applies to the authored expression, never the synthesized $.clsx:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_class_single_base_member_wraps_inline_inside_clsx() {
    // Oracle: $.template_effect(() => $.set_class(div, 1, $.clsx(($.deep_read_state(obj()), $.untrack(() => obj().x)))));
    let js = emit(
        &format!("{LEGACY_OBJ}<div class={{obj.x}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "$.set_class(div, 1, $.clsx(({})))",
            obj_wrap("obj().x")
        )),
        "the non-call class base wraps inline inside $.clsx:\n{js}"
    );
    assert!(
        !js.contains("$.clsx(obj.x)") && !js.contains("$.clsx(obj().x)"),
        "no raw base survives:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_class_base_binary_no_clsx_wraps_plain() {
    // A binary class base skips $.clsx (official `needs_clsx` false) but still
    // wraps. Oracle: $.set_class(div, 1, ($.deep_read_state(obj()), $.untrack(() => obj().a + 'x')))
    let js = emit(
        &format!("{LEGACY_OBJ}<div class={{obj.a + 'x'}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "$.set_class(div, 1, ({}))",
            obj_wrap("obj().a + 'x'")
        )),
        "the no-clsx binary base wraps plain:\n{js}"
    );
    assert!(
        !js.contains("$.clsx"),
        "a binary base never wraps in $.clsx:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_style_single_base_call_memoizes_wrapped() {
    // Oracle: $.template_effect(($0) => $.set_style(div, $0), [() => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]);
    let js = emit(
        &format!("{LEGACY_OBJ}<div style={{obj.m()}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("[() => ({})]", obj_wrap("obj().m()"))),
        "the style base memoizes the wrapped sequence:\n{js}"
    );
    assert!(
        js.contains("$.set_style(div, $0)"),
        "the base reads the $0 slot:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_style_single_base_member_wraps_inline() {
    // Oracle: $.template_effect(() => $.set_style(div, ($.deep_read_state(obj()), $.untrack(() => obj().x))));
    let js = emit(
        &format!("{LEGACY_OBJ}<div style={{obj.x}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("$.set_style(div, ({}))", obj_wrap("obj().x"))),
        "the non-call style base wraps inline:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_style_directive_call_wraps_inside_memoized_object() {
    // Oracle: $.template_effect(($0) => styles = $.set_style(div, '', styles, $0), [
    //   () => ({ color: ($.deep_read_state(obj()), $.untrack(() => obj().m())) })
    // ]);
    let js = emit(
        &format!("{LEGACY_OBJ}<div style:color={{obj.m()}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("{{ color: ({}) }}", obj_wrap("obj().m()"))),
        "the style-directive inner value wraps inside the object:\n{js}"
    );
    assert!(
        js.contains("styles = $.set_style(div, '', styles, $0)"),
        "the whole directives object memoizes into the $0 slot:\n{js}"
    );
    assert!(
        !js.contains("{ color: obj().m() }"),
        "no raw style-directive value survives in definite legacy:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_style_directive_member_wraps_inline_object() {
    // Oracle: $.template_effect(() => styles = $.set_style(div, '', styles, { color: ($.deep_read_state(obj()), $.untrack(() => obj().x)) }));
    let js = emit(
        &format!("{LEGACY_OBJ}<div style:color={{obj.x}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("{{ color: ({}) }}", obj_wrap("obj().x"))),
        "the non-call style-directive value wraps inline in the object:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_style_directive_important_wraps_inside_array_form() {
    // Oracle dep: () => [{}, { color: ($.deep_read_state(obj()), $.untrack(() => obj().m())) }]
    let js = emit(
        &format!("{LEGACY_OBJ}<div style:color|important={{obj.m()}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("[{{}}, {{ color: ({}) }}]", obj_wrap("obj().m()"))),
        "the |important array form wraps the inner value:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_style_directive_mixed_chunk_wraps_in_template() {
    // Oracle dep: () => ({ width: `a${($.deep_read_state(obj()), $.untrack(() => obj().m())) ?? ''}b` })
    let js = emit(
        &format!("{LEGACY_OBJ}<div style:width=\"a{{obj.m()}}b\"></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("`a${{({}) ?? ''}}b`", obj_wrap("obj().m()"))),
        "the mixed style-directive chunk wraps inside the template:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_class_directive_inner_stays_raw_style_directive_inner_wraps() {
    // The class:/style: asymmetry (official `build_class_directives_object`
    // visits raw; `build_style_directives_object` routes build_attribute_value):
    //   [() => ({ foo: obj().m() }), () => ({ color: ($.deep_read_state(obj()), $.untrack(() => obj().f())) })]
    let js = emit(
        &format!("{LEGACY_OBJ}<div class:foo={{obj.m()}} style:color={{obj.f()}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains("{ foo: obj().m() }"),
        "the class-directive inner condition stays RAW:\n{js}"
    );
    assert!(
        js.contains(&format!("{{ color: ({}) }}", obj_wrap("obj().f()"))),
        "the style-directive inner value WRAPS:\n{js}"
    );
    // GUARDRAIL: the synthesized directive OBJECTS are never wrapped as a whole.
    assert!(
        !js.contains("$.untrack(() => ({"),
        "no wrap is fabricated around a synthesized directives object:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_spread_style_directive_wraps_inner_memoizes_object() {
    // Oracle: $.attribute_effect(div, ($0) => ({ ...p(), [$.STYLE]: $0 }), [
    //   () => ({ color: ($.deep_read_state(obj()), $.untrack(() => obj().m())) })
    // ]);
    let js = emit(
        "<script>export let p; export let obj;</script>\n<div {...p} style:color={obj.m()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("[$.STYLE]: $0"),
        "the whole [$.STYLE] object memoizes into $0:\n{js}"
    );
    assert!(
        js.contains(&format!("{{ color: ({}) }}", obj_wrap("obj().m()"))),
        "the style-directive inner value wraps inside the memoized object:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_spread_class_directive_raw_inner_memoizes_object() {
    // Oracle: $.attribute_effect(div, ($0) => ({ ...p(), [$.CLASS]: $0 }), [() => ({ on: obj().m() })]);
    let js = emit(
        "<script>export let p; export let obj;</script>\n<div {...p} class:on={obj.m()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("[$.CLASS]: $0"),
        "the whole [$.CLASS] object memoizes into $0:\n{js}"
    );
    assert!(
        js.contains("[() => ({ on: obj().m() })]"),
        "the class-directive inner condition stays RAW in the memoized object:\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "a class-directive condition is never legacy-wrapped:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_svelte_element_style_directive_wraps_inner() {
    // Oracle: $.attribute_effect($$element, ($0) => ({ style: '', [$.STYLE]: $0 }), [
    //   () => ({ color: (wrap) })
    // ]);
    let js = emit(
        &format!("{LEGACY_OBJ}<svelte:element this={{'div'}} style:color={{obj.m()}}></svelte:element>\n"),
        "App.svelte",
    );
    assert!(
        js.contains("style: '', [$.STYLE]: $0"),
        "the synthesized style entry stays raw and the object memoizes:\n{js}"
    );
    assert!(
        js.contains(&format!("{{ color: ({}) }}", obj_wrap("obj().m()"))),
        "the dynamic-element style-directive inner value wraps:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn css_hash_override_replaces_the_default_scope_class_in_html_and_css() {
    // The resolved override string is used VERBATIM as the scope class in BOTH
    // the baked HTML skeleton (the `$.from_html` static class attribute) and the
    // external css artifact (its `hash` + the scoped selector), and it PRESERVES
    // the override bytes exactly (no `svelte-` prefix, no re-hash).
    let module =
        emit_module_with_css_override(CSS_OVERRIDE_INPUT, "App.svelte", Some("zzoverride1"));

    // (external css) the artifact hash IS the resolved override, byte-exact.
    let css = module
        .css
        .as_ref()
        .expect("a styled component publishes external css");
    assert_eq!(
        css.hash, "zzoverride1",
        "the external css hash must be the resolved override byte-exact"
    );
    assert!(
        css.code.contains("zzoverride1"),
        "the scoped css selector must carry the override class:\n{}",
        css.code
    );
    // NEGATIVE: the default `svelte-` scope class must NOT appear once overridden.
    assert!(
        !css.code.contains("svelte-"),
        "the default svelte- scope class must be fully replaced in css:\n{}",
        css.code
    );

    // (baked HTML) the from_html static skeleton carries the override class.
    assert!(
        module.code.contains("zzoverride1"),
        "the baked static class must carry the override:\n{}",
        module.code
    );
    assert!(
        !module.code.contains("class=\"card svelte-"),
        "the baked static class must not carry the default svelte- scope:\n{}",
        module.code
    );
}

#[test]
fn css_hash_override_absent_keeps_the_default_djb2_scope_class() {
    // Absent an override, the default `svelte-<djb2>` derivation is UNCHANGED
    // (the oracle-pinned filename hash for `App.svelte`).
    let default_class =
        crate::svelte::runtime::css::hash::css_scope_hash(Some("App.svelte"), ".card{color:blue}");
    assert_eq!(
        default_class, "svelte-n50uah",
        "oracle-pinned default hash for App.svelte"
    );
    let module = emit_module_with_css_override(CSS_OVERRIDE_INPUT, "App.svelte", None);
    let css = module
        .css
        .as_ref()
        .expect("a styled component publishes external css");
    assert_eq!(
        css.hash, default_class,
        "absent override → default hash unchanged"
    );
    assert!(
        module.code.contains(&default_class),
        "baked class uses the default:\n{}",
        module.code
    );
}

#[test]
fn distinct_css_hash_overrides_produce_distinct_scope_classes_same_source() {
    // Two DIFFERENT resolved overrides over the SAME source produce DIFFERENT
    // scope classes in both HTML and css — the property the cache identity
    // relies on (identical source, different override ⇒ different output).
    let a = emit_module_with_css_override(CSS_OVERRIDE_INPUT, "App.svelte", Some("aaone"));
    let b = emit_module_with_css_override(CSS_OVERRIDE_INPUT, "App.svelte", Some("bbtwo"));
    assert_ne!(
        a.css.as_ref().unwrap().hash,
        b.css.as_ref().unwrap().hash,
        "distinct overrides must yield distinct css hashes"
    );
    assert!(a.code.contains("aaone") && !a.code.contains("bbtwo"));
    assert!(b.code.contains("bbtwo") && !b.code.contains("aaone"));
}
