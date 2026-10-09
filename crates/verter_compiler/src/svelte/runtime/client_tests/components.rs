use super::*;

#[test]
fn props_no_default_reads_off_props_member() {
    // A NO-DEFAULT prop is NOT declared via `$.prop` — it is read directly off
    // `$$props.name` (official optimization). DISCRIMINATING: no `$.prop` line.
    let src = "<script>let { name } = $props();</script>\n<p>{name}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, $$props.name)"),
        "no-default prop reads off $$props:\n{js}"
    );
    assert!(
        !js.contains("$.prop("),
        "a no-default prop must NOT emit $.prop:\n{js}"
    );
}
#[test]
fn prop_method_call_in_attr_value_is_a_read_not_a_written_prop() {
    // A METHOD CALL on a prop in a template VALUE (`id={p.toString()}`) is a READ of the
    // prop receiver, NOT a write — official `svelte@5.56.10` compiles it to a plain
    // `$$props.p.toString()` read inside a `$.template_effect`, with `$.push`/`$.pop` for
    // context. A method call must NOT be misclassified as a `DeepMutate` write (which
    // would refuse it as a "written prop"). DISCRIMINATING: RED against the pre-fix
    // classifier that treated `obj.method()` as a deep-mutation write.
    let src = "<script>let { p } = $props();</script>\n<div id={p.toString()}></div>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$$props.p.toString()"),
        "a prop method call reads off $$props (not refused as a written prop):\n{js}"
    );
    // The value `has_call` ⇒ it is memoized into the deps-array effect form.
    assert!(
        js.contains("$.template_effect("),
        "a prop method-call attr value memoizes into a template_effect:\n{js}"
    );
}

#[test]
fn props_alias_no_default_reads_source_key_off_props() {
    // F6: `let { foo: bar } = $props()` (no default) reads `$$props.foo` (the SOURCE
    // key) — NOT `$$props.bar`. Verified against svelte@5.56.10. THE discriminating
    // alias regression: RED against the emitter that read `$$props.<local>`.
    let src = "<script>let { foo: bar } = $props();</script>\n<p>{bar}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, $$props.foo)"),
        "a no-default aliased prop reads the SOURCE key off $$props:\n{js}"
    );
    // NEGATIVE: never the local alias as the props member.
    assert!(
        !js.contains("$$props.bar"),
        "must read the source key `foo`, never the alias `bar`:\n{js}"
    );
    assert!(
        !js.contains("$.prop("),
        "a no-default prop is not declared via $.prop:\n{js}"
    );
}

#[test]
fn props_default_referencing_a_sibling_prop_lowers_getter_carrier() {
    // `let { a = 1, b = a } = $props()` — the sibling-reference default lowers via
    // the official prop-source path: the default reads the sibling GETTER, and the
    // zero-arg getter call collapses to the bare getter as the LAZY carrier
    // (`$.prop($$props, 'b', 19, a)`). Verified against svelte@5.56.10.
    let src = "<script>let { a = 1, b = a } = $props();</script>\n<p>{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 3, 1)"),
        "the literal-default sibling is an eager flag-3 prop source:\n{js}"
    );
    assert!(
        js.contains("$.prop($$props, 'b', 19, a)"),
        "the sibling-reference default is the LAZY bare-getter carrier:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, b())"),
        "a prop-source read is the getter call:\n{js}"
    );
    // NEGATIVE: the getter carrier is the bare getter, never a thunk over the
    // getter call, and never the raw sibling read.
    assert!(
        !js.contains("19, () => a") && !js.contains("19, a()"),
        "the zero-arg getter call collapses to the bare getter:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_default_referencing_a_no_default_sibling_lowers_props_member_thunk() {
    // `let { a, b = a } = $props()` — the referenced sibling has NO default (a
    // `$$props.a` member read), so the default is the LAZY member thunk
    // `$.prop($$props, 'b', 19, () => $$props.a)`. Verified against svelte@5.56.10.
    let src = "<script>let { a, b = a } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'b', 19, () => $$props.a)"),
        "a no-default sibling reference is the lazy `$$props` member thunk:\n{js}"
    );
    // The no-default sibling itself emits NO `$.prop` and reads off `$$props`.
    assert!(
        !js.contains("$.prop($$props, 'a'"),
        "a no-default unwritten prop is not a prop source:\n{js}"
    );
    assert!(
        js.contains("$$props.a"),
        "the no-default sibling reads off $$props:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_literal_default_lowers_eager_flag_3_prop_source() {
    // A CONSTANT-LITERAL `$props()` default (`{ a = 1 }`) is the eager flag-3
    // `$.prop($$props, 'a', 3, 1)` form, read via the `a()` getter. Verified
    // against svelte@5.56.10.
    let src = "<script>let { a = 1 } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 3, 1);"),
        "a literal default is the eager flag-3 prop source:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, a())"),
        "a default-bearing prop reads via the getter:\n{js}"
    );
    // NEGATIVE: a plain-default component opens NO context frame, and the getter
    // read replaces the direct `$$props.a` member read.
    assert!(
        !js.contains("$.push") && !js.contains("$.pop"),
        "a plain default must not force the component context frame:\n{js}"
    );
    assert!(
        !js.contains("$$props.a"),
        "a prop-source read never reads $$props directly:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

// ── Naming (built from the oracle's actual official output) ──────────────────

#[test]
fn component_naming_matches_official() {
    // The official rule: get_component_name (capitalize first; index→parent-dir
    // unless `src`) then scope.generate (`[^A-Za-z0-9_$]`→`_`; leading digit→`_`).
    let base = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let name_for = |filename: &str, name_opt: Option<&str>| -> String {
        let alloc = Allocator::default();
        let parsed = parse_svelte(base);
        let opts = SvelteRuntimeOptions {
            filename: Some(filename.to_string()),
            name: name_opt.map(|s| s.to_string()),
            ..Default::default()
        };
        let js = compile_client(base, &parsed, &opts, &alloc, false, false)
            .unwrap()
            .code;
        let after = js.split("export default function ").nth(1).unwrap();
        after.split('(').next().unwrap().to_string()
    };
    assert_eq!(name_for("app.svelte", None), "App");
    assert_eq!(name_for("App.svelte", None), "App");
    assert_eq!(name_for("my-widget.svelte", None), "My_widget");
    assert_eq!(name_for("1x.svelte", None), "_x");
    assert_eq!(name_for("foo/index.svelte", None), "Foo");
    assert_eq!(name_for("src/index.svelte", None), "Index");
    assert_eq!(name_for("index.svelte", None), "Index");
    assert_eq!(name_for("foo.bar.svelte", None), "Foo_bar");
    // An explicit name OVERRIDES the filename and is NOT capitalized (only
    // identifier-sanitized): `2bad` → `_bad`.
    assert_eq!(name_for("App.svelte", Some("2bad")), "_bad");
    assert_eq!(name_for("whatever.svelte", Some("App")), "App");
}

#[test]
fn passive_and_nonpassive_modifiers_emit_the_void0_capture_slot_plus_passive_boolean() {
    // `passive` ⇒ 5th positional `true` with the capture slot `void 0`; `nonpassive`
    // ⇒ 5th positional `false` with `void 0`. Passive/nonpassive are NOT wrappers.
    let passive = normalize_js_cosmetics(&emit(
        "<script>let n = $state(0);</script>\n<button on:click|passive={() => n++}>x</button>\n",
        "App.svelte",
    ));
    assert!(
        passive.contains(&nc(
            "$.event('click', button, () => $.update(n), void 0, true)"
        )),
        "passive must emit `void 0, true`:\n{passive}"
    );
    let nonpassive = normalize_js_cosmetics(&emit(
        "<script>let n = $state(0);</script>\n<button on:click|nonpassive={() => n++}>x</button>\n",
        "App.svelte",
    ));
    assert!(
        nonpassive.contains(&nc(
            "$.event('click', button, () => $.update(n), void 0, false)"
        )),
        "nonpassive must emit `void 0, false`:\n{nonpassive}"
    );
}

#[test]
fn html_prop_call_payload_does_not_elide_and_thunks_the_rewritten_member() {
    // A `{@html render()}` whose `render` is a no-default `$props()` binding does NOT
    // elide: the callee rewrites to the member `$$props.render`, so the official form is
    // the THUNK over the rewritten whole expression — `$.html(div, () => $$props.render(),
    // true)`. Elision applies ONLY when the rewritten callee equals the bare name (a plain
    // / local / demoted id). Pinned svelte@5.56.10.
    let js = emit(
        "<script>let { render } = $props()</script>\n<div>{@html render()}</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.html(div, () => $$props.render(), true)")),
        "a prop-callee {{@html}} call must thunk the rewritten member, not elide:\n{js}"
    );
    // NEGATIVE: it must NOT elide to the bare callee (the prior reparse-bug emitted the
    // un-rewritten `render`).
    assert!(
        !n.contains(&nc("$.html(div, render, true)"))
            && !n.contains(&nc("$.html(div, $$props.render, true)")),
        "a prop-callee {{@html}} call must NOT elide to a bare callee:\n{js}"
    );
}

#[test]
fn html_paren_wrapped_prop_call_payload_thunks_the_rewritten_callee_without_author_parens() {
    // A `{@html (render)()}` whose `render` is a no-default `$props()` binding does NOT
    // elide (the callee rewrites to the member `$$props.render`), so it stays a THUNK — but
    // the thunk renders the REWRITTEN CALLEE CALL `() => $$props.render()`, NOT the blind
    // whole-source rewrite `() => ($$props.render)()` (which would keep the author parens).
    // Pinned svelte@5.56.10: `$.html(div, () => $$props.render(), true)`.
    let js = emit(
        "<script>let { render } = $props()</script>\n<div>{@html (render)()}</div>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.html(div, () => $$props.render(), true)")),
        "a paren-wrapped prop-callee {{@html}} call must thunk the rewritten callee without parens:\n{js}"
    );
    // NEGATIVE: the author parens must NOT survive into the thunk (the pre-fix bug), and it
    // must NOT elide to a bare callee.
    assert!(
        !n.contains(&nc("$.html(div, () => ($$props.render)(), true)")),
        "a paren-wrapped prop-callee thunk must NOT keep the author parens:\n{js}"
    );
    assert!(
        !n.contains(&nc("$.html(div, render, true)"))
            && !n.contains(&nc("$.html(div, $$props.render, true)")),
        "a paren-wrapped prop-callee {{@html}} call must NOT elide to a bare callee:\n{js}"
    );
}

#[test]
fn component_emits_a_direct_call() {
    // A component reference (a capitalized tag) imported from a `.svelte` module emits a
    // DIRECT `Foo($$anchor, {})` call (the component surface), NOT a `$.get` on the callee
    // (the imported local is a non-reactive value binding).
    // The `$props()` rune forces runes mode (an import-only component is legacy mode, 5i).
    let js = emit_result(
        "<script>import Foo from './Foo.svelte'; let { x } = $props();</script>\n<Foo />\n",
    )
    .expect("a component reference emits a module");
    assert!(
        js.contains("import Foo from './Foo.svelte';"),
        "missing the component import:\n{js}"
    );
    assert!(
        js.contains("Foo($$anchor, {})"),
        "missing the direct component call:\n{js}"
    );
    // NEGATIVE: the imported callee is read as a bare name, NEVER `$.get(Foo)`.
    assert!(
        !js.contains("$.get(Foo)"),
        "the component callee must be a bare name, not $.get:\n{js}"
    );
}

#[test]
fn component_named_import_from_a_barrel_emits_a_direct_call() {
    // Svelte component values are routinely re-exported through plain TS/JS
    // barrels. At runtime a named import is still an immutable live import
    // binding and is therefore a sound static component callee; the shallow
    // compiler must not require the immediate specifier to end in `.svelte`.
    let js = emit_result(
        "<script>import { Child } from './public'; let { label } = $props();</script>\n<Child label={label} />\n",
    )
    .expect("a barrel-imported component emits a module");
    assert!(
        js.contains("import { Child } from './public';"),
        "missing the authored barrel import:\n{js}"
    );
    assert!(
        js.contains("Child($$anchor, {get label()") && js.contains("return $$props.label;"),
        "missing the direct barrel-imported component call:\n{js}"
    );
    assert!(
        !js.contains("$.get(Child)"),
        "an imported component callee is a bare live import read:\n{js}"
    );
}

#[test]
fn component_unbound_callee_fails_closed() {
    // The callee-resolution DISCRIMINATOR: a capitalized component tag whose name
    // resolves to NO admitted `.svelte`-component import is an unsupported component SOURCE
    // — it fails CLOSED, NOT a coincidental bare `Foo($$anchor, {})` call on an unbound
    // global. This is the SAME fixture as `component_emits_a_direct_call` MINUS the import,
    // so the only difference is whether `Foo` resolves to a `ComponentImport` binding — the
    // `!$.get(Foo)` assertion alone is non-discriminating (an unbound global also emits bare
    // `Foo`). The `$props()` rune forces runes mode.
    assert_fail_closed("<script>let { p } = $props();</script>\n<Foo />\n", |s| {
        matches!(
            s,
            UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "component",
                ..
            }
        )
    });
}

#[test]
fn component_dotted_callee_fails_closed() {
    // A DOTTED static component name (`<Foo.Bar/>`) is a namespace/member-component source —
    // an advanced form this vertical does not model. Only a BARE identifier resolving to an
    // admitted `.svelte`-component import is authorized; the whole-name gate fails CLOSED on
    // the dot even though the HEAD segment `Foo` IS an admitted `ComponentImport`. A default
    // `.svelte` import is a component FUNCTION (not a namespace object), so `Foo.Bar` would be
    // a likely-undefined member access — emitting `Foo.Bar($$anchor, …)` is wrong. This is
    // the DISCRIMINATOR vs `component_emits_a_direct_call`: same admitted `Foo` import, but
    // the dotted tag must refuse where the bare `<Foo/>` emits. The `$props()` rune forces
    // runes mode so the fixture reaches the component projection (not the legacy dispatch).
    assert_fail_closed(
        "<script>import Foo from './Foo.svelte'; let { p } = $props();</script>\n<Foo.Bar />\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "component",
                    ..
                }
            )
        },
    );
}

// ── Component unit coverage: the `.svelte` default-import subset, the component-family
//    specials, and the COMPONENT-vs-ELEMENT `let:` split. ──

#[test]
fn every_static_import_form_is_admitted_to_the_instance_prelude_slot() {
    // The static-import prelude admits EVERY static import form. The `.svelte`
    // component default stays the component-callee subset; named / aliased /
    // namespace / side-effect / non-`.svelte` default / mixed forms are hoisted to
    // the INSTANCE slot of the module prelude (AFTER `import * as $`), each emitted
    // verbatim in source order.
    let ok = emit_result(
        "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child />\n{p}\n",
    )
    .expect("a default .svelte import is admitted");
    assert!(
        ok.contains("import Child from './Child.svelte';"),
        "the default .svelte import must be hoisted to module scope:\n{ok}"
    );
    for (label, import_stmt, expected) in [
        (
            "named",
            "import { helper } from './helpers.js';",
            "import { helper } from './helpers.js';",
        ),
        (
            "named-alias",
            "import { a as b } from './helpers.js';",
            "import { a as b } from './helpers.js';",
        ),
        (
            "namespace",
            "import * as NS from './ns.js';",
            "import * as NS from './ns.js';",
        ),
        (
            "side-effect",
            "import './setup.js';",
            "import './setup.js';",
        ),
        (
            "default-non-svelte",
            "import helper from './helper.js';",
            "import helper from './helper.js';",
        ),
        (
            "mixed-default-named",
            "import Child, { x } from './Child.svelte';",
            "import Child, { x } from './Child.svelte';",
        ),
    ] {
        let src = format!(
            "<script>{import_stmt} let __r = $state(0);</script>\n<button onclick={{() => __r++}}>{{__r}}</button>\n"
        );
        let js = emit_result(&src)
            .unwrap_or_else(|e| panic!("[{label}] a static import form must be admitted: {e:?}"));
        // The import lands in the INSTANCE slot: after the runtime namespace import.
        let ns_at = js
            .find("import * as $ from 'svelte/internal/client';")
            .unwrap_or_else(|| panic!("[{label}] missing the runtime namespace:\n{js}"));
        let user_at = js
            .find(expected)
            .unwrap_or_else(|| panic!("[{label}] missing the hoisted user import:\n{js}"));
        assert!(
            user_at > ns_at,
            "[{label}] an instance import must emit AFTER the runtime namespace:\n{js}"
        );
        // NEGATIVE: the admitted form must not leak the old script-import refusal
        // diagnostic anywhere (it compiled), and the import is module-scope — before
        // the component function.
        assert!(
            user_at < js.find("export default function").unwrap(),
            "[{label}] the user import must be module-scope (above the component fn):\n{js}"
        );
    }
}

#[test]
fn module_namespace_member_read_frames_like_the_instance_slot() {
    // The MODULE-slot twin: `<script module>import * as NS …</script>` + `{NS.z}`
    // frames identically (module imports are unsafe roots for the shared
    // `needs_context` analysis, resolving up the lexical chain).
    let js = emit(
        "<script module>import * as NS from './m.js';</script>\n<script>let c = $state(0);</script>\n<p>{NS.z}</p>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.set_text(text, NS.z)") && js.contains("$.push($$props, true)"),
        "the module-slot member read must frame like the instance slot:\n{js}"
    );
    // The module import emits BEFORE the runtime namespace (the module slot).
    let module_at = js.find("import * as NS from './m.js';").unwrap();
    let ns_at = js
        .find("import * as $ from 'svelte/internal/client';")
        .unwrap();
    assert!(
        module_at < ns_at,
        "a module-slot import must emit BEFORE the runtime namespace:\n{js}"
    );
}

#[test]
fn component_prop_value_from_import_emits_the_getter_form() {
    // A component-prop value reading an IMPORT (`b={x}`) `has_state` (imports are
    // live bindings, not statically known), so official emits the GETTER accessor
    // `get b() { return x; }` — never the plain init `b: x` (oracle-verified
    // against svelte@5.56.10).
    let js = emit(
        "<script>import Child from './Child.svelte'; import { x } from './m.js'; let c = $state(0);</script>\n<Child b={x} />\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("get b() {") && js.contains("return x;"),
        "an import-valued component prop must emit the getter accessor:\n{js}"
    );
    assert!(
        !js.contains("b: x"),
        "an import-valued component prop must NOT emit the plain init form:\n{js}"
    );
}

#[test]
fn svelte_component_special_emits_dollar_component() {
    // `<svelte:component this={comp}>` (a DYNAMIC component) emits the 3-arg
    // `$.component(node, () => $$props.comp, ($$anchor, $$component) => { $$component(...) })`.
    let js = emit_result(
        "<script>let { comp } = $props();</script>\n<svelte:component this={comp} label=\"hi\" />\n",
    )
    .expect("svelte:component emits a module");
    assert!(
        js.contains("$.component(node, () => $$props.comp, ($$anchor, $$component) =>"),
        "missing the $.component(node, () => this, callback) shape:\n{js}"
    );
    assert!(
        js.contains("$$component($$anchor, {label: 'hi'})"),
        "missing the inner $$component call with the props:\n{js}"
    );
}

#[test]
fn svelte_component_special_with_imported_default_uses_bare_callee() {
    // The DYNAMIC-COMPONENT-VALUE half of the `.svelte`-default-import subset: a `.svelte`
    // DEFAULT import (`import Child from './Child.svelte'`) consumed as the `<svelte:component
    // this={Child}>` selector. The import is admitted to the prelude REGARDLESS of being used as a
    // dynamic value (not a static `<Child/>` callee), and the `this` expression resolves the
    // non-reactive `ComponentImport` binding to the BARE local — `$.component(node, () => Child,
    // …)`, NEVER `$.get(Child)` / `() => $$props.Child`. The `$props()` rune forces runes mode (an
    // import-only component is legacy mode, 5i).
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { label } = $props();</script>\n<svelte:component this={Child} {label} />\n",
    )
    .expect("svelte:component with an imported default emits a module");
    // (a) The `.svelte` default import is ADMITTED to the module prelude.
    assert!(
        js.contains("import Child from './Child.svelte';"),
        "missing the admitted `.svelte` default import in the prelude:\n{js}"
    );
    // (b) The imported local drives the dynamic component value as a BARE name.
    assert!(
        js.contains("$.component(node, () => Child, ($$anchor, $$component) =>"),
        "missing the bare-import dynamic component value `() => Child`:\n{js}"
    );
    // NEGATIVE: the imported callee is a non-reactive value binding — never a `$.get` read and
    // never routed through `$$props` (which is what a PROP-sourced `this={comp}` would emit).
    assert!(
        !js.contains("$.get(Child)"),
        "the imported dynamic component value must be a bare name, not $.get:\n{js}"
    );
    assert!(
        !js.contains("() => $$props.Child"),
        "the imported dynamic component value must not route through $$props:\n{js}"
    );
    // CONTRAST: the threaded prop `label` DOES route through `$$props.label` — proving the
    // rewriter discriminates the import binding (bare) from a reactive prop read (so the bare
    // `Child` is the ComponentImport binding-kind decision, not an everything-emits-bare accident).
    assert!(
        js.contains("$$props.label"),
        "the threaded prop must route through $$props (the binding-kind contrast):\n{js}"
    );
}

#[test]
fn render_spread_argument_fails_closed_for_every_callee() {
    // Official `svelte@5.56.10` HARD-ERRORS on a SPREAD argument in a `{@render …}` tag
    // (`render_tag_invalid_spread_argument`: "cannot use spread arguments in {@render
    // ...} tags"). Verter must FAIL CLOSED with the typed component/snippet refusal —
    // never silently DROP the spread and emit a wrong-arity `$.snippet(node, () => row)`
    // call. Covers a PROP callee, a LOCAL-`{#snippet}` callee, and a DYNAMIC (optional-
    // call) callee: every callee shape over-accepted the spread before this fix.
    for (label, src) in [
        (
            "prop_callee",
            "<script>let { row, xs } = $props();</script>\n{@render row(...xs)}\n",
        ),
        (
            "local_snippet_callee",
            "<script>let { xs } = $props();</script>\n{#snippet row()}<span>x</span>{/snippet}\n{@render row(...xs)}\n",
        ),
        (
            "dynamic_callee",
            "<script>let { row, xs } = $props();</script>\n{@render row?.(...xs)}\n",
        ),
    ] {
        assert_fail_closed_labeled(label, src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. }
                    if *construct == "{@render} spread argument"
            )
        });
    }
}

#[test]
fn render_parenthesized_whole_call_spread_fails_closed() {
    // The render-spread refusal is closed over OUTER author parentheses wrapping the WHOLE
    // call: official `svelte@5.56.10` HARD-ERRORS on the spread
    // (`render_tag_invalid_spread_argument`) no matter how many parens wrap the call, so a
    // parenthesized whole call must FAIL CLOSED exactly like the bare form — never peel to a
    // non-call node and silently DROP the spread into a wrong-arity `$.snippet(node, () =>
    // row)` emit. Covers a single paren, nested parens, a parenthesized OPTIONAL call, and a
    // parenthesized LOCAL-`{#snippet}` callee.
    for (label, src) in [
        (
            "paren_whole_call",
            "<script>let { row, xs } = $props();</script>\n{@render (row(...xs))}\n",
        ),
        (
            "double_paren_whole_call",
            "<script>let { row, xs } = $props();</script>\n{@render ((row(...xs)))}\n",
        ),
        (
            "paren_optional_whole_call",
            "<script>let { row, xs } = $props();</script>\n{@render (row?.(...xs))}\n",
        ),
        (
            "paren_local_snippet",
            "<script>let { xs } = $props();</script>\n{#snippet row()}<span>x</span>{/snippet}\n{@render (row(...xs))}\n",
        ),
    ] {
        assert_fail_closed_labeled(label, src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. }
                    if *construct == "{@render} spread argument"
            )
        });
    }
}

#[test]
fn render_array_internal_spread_argument_still_emits() {
    // NARROWNESS CONTROL: an ARRAY-INTERNAL spread (`{@render row([...xs])}`) is a normal
    // array-expression argument, NOT a call-argument spread — official ACCEPTS it. It must
    // STILL emit the `$.snippet` call; peeling outer author parens for the whole-call-spread
    // refusal must not over-refuse this accepted shape. The spread argument is
    // `has_call`-bearing (official `SpreadElement` analysis), so it rides the memoized
    // `let $0 = $.derived(() => [...$$props.xs]);` hoist and the `() => $.get($0)` thunk —
    // the DYNAMIC-callee `$.snippet` form memoizes exactly like the static call.
    let js = emit(
        "<script>let { row, xs } = $props();</script>\n{@render row([...xs])}\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snippet("),
        "an array-internal spread render arg must still emit the $.snippet call:\n{js}"
    );
    assert!(
        js.contains("let $0 = $.derived(() => ([...$$props.xs]));"),
        "the array-internal spread must memoize into the derived hoist:\n{js}"
    );
    assert!(
        js.contains("$.snippet(node, () => $$props.row, () => $.get($0));"),
        "the $.snippet call must read the memoized $.get thunk:\n{js}"
    );
    // NEGATIVE: never the un-memoized inline spread thunk.
    assert!(
        !js.contains("$.snippet(node, () => $$props.row, () => [...$$props.xs])"),
        "the spread arg must not stay an un-memoized inline thunk:\n{js}"
    );
}

#[test]
fn component_destructuring_let_alias_fails_closed() {
    // A DESTRUCTURING `let:item={…}` alias is a broader decomposition this vertical does not
    // model — it fails CLOSED, never a silent drop. The refusal keys on the parsed pattern
    // NODE KIND (only a bare binding identifier is a one-name rename), NOT the collected-name
    // COUNT: a count gate wrongly accepts SINGLE-name destructures (`{ a }` / `[a]` each
    // collect exactly one name) and emits `const a = $.derived(() => $$slotProps.item)`,
    // silently swallowing the destructure. Every object/array pattern — single- OR multi-name
    // — must refuse.
    for (label, src) in [
        (
            "multi-name object",
            "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child let:item={{ a, b }}>x</Child>\n",
        ),
        (
            "single-name object",
            "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child let:item={{ a }}>x</Child>\n",
        ),
        (
            "single-name array",
            "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child let:item={[a]}>x</Child>\n",
        ),
    ] {
        assert_fail_closed_labeled(label, src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "let-directive",
                    ..
                }
            )
        });
    }
}

#[test]
fn component_class_directive_fails_closed() {
    // A `class:` directive on a COMPONENT is invalid Svelte (a component is not a DOM host) —
    // it fails CLOSED, NOT silently dropped (a silent no-op would emit `<Child class:foo={x}/>`
    // as `Child($$anchor, {})`, dropping the directive).
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let x = $state(0);</script>\n<Child class:foo={x} />\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "directive",
                    ..
                }
            )
        },
    );
}

#[test]
fn component_style_directive_fails_closed() {
    // A `style:` directive on a COMPONENT is likewise invalid — fail CLOSED, never a silent
    // drop (sibling to the `class:` / `use:` / `transition:` component-directive refusal).
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let x = $state(0);</script>\n<Child style:color={x} />\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "directive",
                    ..
                }
            )
        },
    );
}

#[test]
fn svelte_boundary_onerror_emits_the_props_member() {
    // `<svelte:boundary onerror={() => n++}>` → `$.boundary(node, { onerror: () => $.update(n)
    // }, cb)` — the onerror is a PROPS member (NOT a `$.event` listener, NOT hoisted).
    let js = emit(
        "<script>let n = $state(0);</script>\n<svelte:boundary onerror={() => n++}><p>x</p></svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.boundary(node, {onerror: () => $.update(n)}, ($$anchor) =>"
        )),
        "onerror is a props member of $.boundary:\n{js}"
    );
    assert!(
        !js.contains("$.event("),
        "onerror is not a $.event listener:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_boundary_call_bearing_prop_stays_raw_through_the_prepared_carrier() {
    // BoundaryProp is policy=Raw: official `SvelteBoundary.js` visits the
    // attribute expression RAW (no `build_expression`), so a call-bearing
    // `failed={obj.m()}` in a DEFINITELY-legacy component must NOT legacy-wrap
    // even though the wrap trigger (`has_call`) fires. The prop still routes
    // through the sole preparation entry as a raw prepared carrier.
    let js = emit(
        "<script>export let obj;\nexport let fail;</script>\n<svelte:boundary failed={obj.m()} pending={fail}><p>c</p></svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        !js.contains("$.untrack("),
        "a boundary prop never legacy-wraps (official visits it raw):\n{js}"
    );
    assert!(
        !js.contains("$.deep_read_state("),
        "no wrap dep-reads on the raw boundary surface:\n{js}"
    );
    // The call-bearing value still emits as the STATE-BEARING getter over the
    // RAW rewritten expression (the legacy prop accessor, untouched).
    assert!(
        n.contains(&nc("get failed() { return obj().m(); }")),
        "raw getter body over the legacy prop accessor:\n{js}"
    );
    assert!(
        n.contains(&nc("get pending() { return fail(); }")),
        "the state-bearing shorthand prop keeps the raw getter:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_failed_snippet_hoists_into_the_wrapping_block() {
    // `{#snippet failed(error, reset)}…{/snippet}` → the snippet hoists to a `const failed =
    // ($$anchor, error = $.noop, reset = $.noop) => {…}` in a wrapping `{ … }` block above the
    // call, passed by NAME (object shorthand) in the props.
    let js = emit(
        "<script>let { x } = $props();</script>\n<svelte:boundary><p>{x}</p>{#snippet failed(error, reset)}<p>oops</p>{/snippet}</svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "const failed = ($$anchor, error = $.noop, reset = $.noop) =>"
        )),
        "failed snippet hoists with $.noop-defaulted params:\n{js}"
    );
    assert!(
        n.contains(&nc("$.boundary(node, {failed}, ($$anchor) =>")),
        "the failed snippet is passed by name (shorthand) in props:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_full_hoists_both_snippets_with_onerror() {
    // onerror + failed + pending → both snippets hoist as consts, props `{ onerror: …, failed,
    // pending }`.
    let js = emit(
        "<script>let n = $state(0);</script>\n<svelte:boundary onerror={() => n++}><p>content</p>{#snippet failed(error, reset)}<p>oops</p>{/snippet}{#snippet pending()}<p>loading</p>{/snippet}</svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "const failed = ($$anchor, error = $.noop, reset = $.noop) =>"
        )),
        "failed hoist:\n{js}"
    );
    assert!(
        n.contains(&nc("const pending = ($$anchor) =>")),
        "pending hoist:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.boundary(node, {onerror: () => $.update(n), failed, pending}, ($$anchor) =>"
        )),
        "props carry onerror + both snippet shorthands in source order:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_hoists_nonspecial_snippet_above_call() {
    // Official hoists ALL of a boundary's `{#snippet}` decls into the wrapping `{ … }` block
    // ABOVE the `$.boundary(...)` call — only `failed`/`pending` are boundary PROPS; every
    // other snippet (`foo`) is a hoisted const referenced from the body (`foo($$anchor)`),
    // NOT a body-local const and NOT a prop. Verified against pinned svelte@5.56.10. The
    // unrelated `$state` pins runes mode.
    let js = emit(
        "<script>let k = $state(0);</script>\n<svelte:boundary>{#snippet failed(err)}<p>oops</p>{/snippet}{#snippet foo()}<span>F</span>{/snippet}{@render foo()}</svelte:boundary>\n",
        "App.svelte",
    );
    let boundary_at = js.find("$.boundary(").expect("emits a boundary call");
    let foo_const_at = js.find("const foo =").expect("emits a foo snippet const");
    assert!(
        foo_const_at < boundary_at,
        "the non-special `foo` snippet const must HOIST above the boundary call, not into the body callback:\n{js}"
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.boundary(node, {failed}, ($$anchor) =>")),
        "only `failed` is passed as a boundary prop shorthand (foo is hoisted, not propped):\n{js}"
    );
    assert!(
        n.contains(&nc("foo($$anchor)")),
        "the body renders the hoisted foo snippet by call:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_only_nonspecial_snippet_still_wraps_and_hoists() {
    // A boundary with ONLY a non-failed/pending snippet still opens the wrapping block and
    // hoists the const above the call with an EMPTY props object — official:
    // `{ const foo = …; $.boundary(node, {}, ($$anchor) => { foo($$anchor); }); }`. The
    // unrelated `$state` pins runes mode.
    let js = emit(
        "<script>let k = $state(0);</script>\n<svelte:boundary>{#snippet foo()}<span>F</span>{/snippet}{@render foo()}</svelte:boundary>\n",
        "App.svelte",
    );
    let boundary_at = js.find("$.boundary(").expect("emits a boundary call");
    let foo_const_at = js.find("const foo =").expect("emits a foo snippet const");
    assert!(
        foo_const_at < boundary_at,
        "foo hoists above the boundary call:\n{js}"
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.boundary(node, {}, ($$anchor) =>")),
        "the boundary props object is empty (foo is hoisted, not a prop):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_failed_attribute_emits_getter_prop() {
    // MODERN `failed={expr}` ATTRIBUTE form (`expr` a state-bearing prop ref) → the props
    // object carries the GETTER accessor `get failed() { return $$props.failed; }` — official's
    // `has_state ? b.get(name, [b.return(expr)]) : b.init(name, expr)` rule (SvelteBoundary.js).
    // NO `{#snippet}` hoist, NO wrapping block (the attribute form does not create a hoisted
    // const). RED against cycle-2's conservative fail-close.
    let js = emit(
        "<script>let { failed } = $props();</script>\n<svelte:boundary failed={failed}><p>content</p></svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.boundary(node, {get failed() { return $$props.failed; }}, ($$anchor) =>"
        )),
        "failed attr expr is a GETTER props member of $.boundary:\n{js}"
    );
    // NEGATIVE: the pure-attribute form hoists NO snippet const + opens NO wrapping block, and
    // never emits the direct `{ failed }` snippet shorthand (that is the CHILD form).
    assert!(
        !n.contains(&nc("const failed =")),
        "the attribute form must NOT hoist a snippet const:\n{js}"
    );
    assert!(
        !n.contains(&nc("$.boundary(node, {failed}")),
        "the attribute form must NOT emit the direct failed shorthand (that is the child form):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_member_rooted_attribute_emits_getter_prop() {
    // `failed={obj.failed}` over a PLAIN-LOCAL object (admitted as a DOM bind-target
    // root) — the MEMBER-ROOT half of official's `has_state` (`MemberExpression.js`
    // `!is_pure`): a member rooted at ANY declared binding is state-bearing, so the
    // prop emits the GETTER member (verified against pinned svelte@5.56.10:
    // `get failed() { return obj.failed; }`), NOT the plain `failed: obj.failed`
    // init. The unrelated `$state` pins runes mode.
    let js = emit(
        "<script>let k = $state(0);\nlet obj = { failed: null };</script>\n<input bind:value={obj.failed} />\n<svelte:boundary failed={obj.failed}><p>hi</p></svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.boundary(node, {get failed() { return obj.failed; }}, ($$anchor) =>"
        )),
        "a member-rooted failed value is a GETTER props member:\n{js}"
    );
    // NEGATIVE: never the plain init for a member-rooted value.
    assert!(
        !n.contains(&nc("failed: obj.failed")),
        "a member-rooted failed value must NOT stay a plain init:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");

    // INVERSE: a member read DEFERRED inside an arrow body stays the plain init (the
    // sync-only rule) — official emits `onerror: () => obj.failed`.
    let js2 = emit(
        "<script>let k = $state(0);\nlet obj = { failed: null };</script>\n<input bind:value={obj.failed} />\n<svelte:boundary onerror={() => obj.failed}><p>hi</p></svelte:boundary>\n",
        "App.svelte",
    );
    let n2 = normalize_js_cosmetics(&js2);
    assert!(
        n2.contains(&nc(
            "$.boundary(node, {onerror: () => obj.failed}, ($$anchor) =>"
        )),
        "a deferred member read stays a plain onerror init:\n{js2}"
    );
    assert!(
        !n2.contains(&nc("get onerror()")),
        "a deferred member read must NOT promote to a getter:\n{js2}"
    );
    assert!(parses_as_js(&js2), "module must be valid JS:\n{js2}");
}

#[test]
fn component_member_rooted_prop_emits_getter_not_init() {
    // `<C x={obj.y}>` with `obj` a PLAIN LOCAL (admitted as a DOM bind-target root) —
    // the SAME shared `prop_value_has_state` predicate drives the `Component.js`
    // getter-vs-init decision, so the member-rooted value emits the GETTER
    // `get x() { return obj.y; }` (verified against pinned svelte@5.56.10), NOT the
    // plain `x: obj.y` init. Locks the shared predicate on the component surface. The
    // unrelated `$state` pins runes mode.
    let js = emit(
        "<script>import C from './C.svelte';\nlet k = $state(0);\nlet obj = { y: '' };</script>\n<input bind:value={obj.y} />\n<C x={obj.y} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("get x() { return obj.y; }")),
        "a member-rooted component prop is a GETTER member:\n{js}"
    );
    assert!(
        !n.contains(&nc("x: obj.y")),
        "a member-rooted component prop must NOT stay a plain init:\n{js}"
    );
    // INVERSE: a member read DEFERRED inside an arrow prop value stays the plain init
    // (verified against pinned svelte@5.56.10: `C(node, { x: () => obj.y })`).
    let js2 = emit(
        "<script>import C from './C.svelte';\nlet k = $state(0);\nlet obj = { y: '' };</script>\n<input bind:value={obj.y} />\n<C x={() => obj.y} />\n",
        "App.svelte",
    );
    let n2 = normalize_js_cosmetics(&js2);
    assert!(
        n2.contains(&nc("C(node, {x: () => obj.y})")),
        "a deferred member read stays a plain prop init:\n{js2}"
    );
    assert!(
        !n2.contains(&nc("get x()")),
        "a deferred member read must NOT promote to a getter:\n{js2}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
    assert!(parses_as_js(&js2), "module must be valid JS:\n{js2}");
}

#[test]
fn svelte_boundary_pending_attribute_emits_getter_prop() {
    // MODERN `pending={expr}` ATTRIBUTE form → `get pending() { return $$props.pending; }`.
    let js = emit(
        "<script>let { pending } = $props();</script>\n<svelte:boundary pending={pending}><p>content</p></svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.boundary(node, {get pending() { return $$props.pending; }}, ($$anchor) =>"
        )),
        "pending attr expr is a GETTER props member of $.boundary:\n{js}"
    );
    assert!(
        !n.contains(&nc("const pending =")),
        "the attribute form must NOT hoist a snippet const:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_mixed_attribute_and_snippet_child() {
    // A `failed={expr}` ATTRIBUTE + a `{#snippet pending}` CHILD together: the getter attr prop
    // precedes the hoisted-snippet shorthand — `{ get failed() {…}, pending }` — inside the
    // wrapping block that hoists the pending snippet const.
    let js = emit(
        "<script>let { failed } = $props();</script>\n<svelte:boundary failed={failed}><p>content</p>{#snippet pending()}<p>loading</p>{/snippet}</svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("const pending = ($$anchor) =>")),
        "the pending snippet still hoists to a const:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.boundary(node, {get failed() { return $$props.failed; }, pending}, ($$anchor) =>"
        )),
        "getter attr prop precedes the snippet shorthand, in source order:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_boundary_conflict_attribute_and_snippet_same_name() {
    // CONFLICT (official parity): BOTH a `failed={expr}` ATTRIBUTE and a `{#snippet failed}`
    // CHILD. Official emits BOTH keys — the getter (from the attribute, source-first) then the
    // shorthand (from the hoisted snippet) — a duplicate-key object literal `{ get failed() {…},
    // failed }` (valid ES2015+, last-wins). Verter matches official; it does NOT dedupe, drop,
    // or error. The snippet still hoists its const in the wrapping block.
    let js = emit(
        "<script>let { failedProp } = $props();</script>\n<svelte:boundary failed={failedProp}><p>content</p>{#snippet failed(error, reset)}<p>oops</p>{/snippet}</svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "const failed = ($$anchor, error = $.noop, reset = $.noop) =>"
        )),
        "the failed snippet still hoists its const:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.boundary(node, {get failed() { return $$props.failedProp; }, failed}, ($$anchor) =>"
        )),
        "official parity: BOTH the getter (attr) and the shorthand (snippet) keys, source order:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "a duplicate-key object literal is valid ES2015+ module JS:\n{js}"
    );
}

#[test]
fn svelte_head_prop_title_emits_deferred_with_nullish() {
    // A PROP `<title>{t}</title>` (`t` not provably defined) → `$.deferred_template_effect(() =>
    // { $.document.title = $$props.t ?? ''; })` — `has_state` true ⇒ deferred, `?? ''` since the
    // value is not provably defined.
    let js = emit(
        "<script>\n\tlet { t } = $props();\n</script>\n\n<svelte:head>\n\t<title>{t}</title>\n</svelte:head>\n",
        "special/svelte_head_prop_title.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.head('16e2757', ($$anchor) =>")),
        "head wrapper:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.deferred_template_effect(() => {$.document.title = $$props.t ?? '';})"
        )),
        "deferred title with `?? ''`:\n{js}"
    );
    // NEGATIVE: a stateful title is NOT a plain `$.effect`.
    assert!(
        !js.contains("$.effect("),
        "stateful title must defer, not $.effect:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_rest_basic_lowers_rest_props_capture_with_delocalized_named_read() {
    // INVERTED (was `props_rest_fails_closed_not_partial`, which pinned the now-
    // SUPPORTED rest surface): `let { name, ...rest } = $props()` lowers the module
    // `rest_excludes` Set (fixed prefix + the source key `name`) + the instance
    // `let rest = $.rest_props($$props, rest_excludes)` capture; the bare `{name}`
    // read de-localizes to `$$props.name` and — being a BARE named read — opens NO
    // component context frame. Verified against svelte@5.56.10.
    let js = emit(
        "<script>let { name, ...rest } = $props();</script>\n<p>{name}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("var rest_excludes = new Set(['$$slots', '$$events', '$$legacy', 'name']);"),
        "missing the hoisted rest_excludes Set (prefix + source key):\n{js}"
    );
    assert!(
        js.contains("let rest = $.rest_props($$props, rest_excludes);"),
        "missing the $.rest_props capture declarator:\n{js}"
    );
    assert!(
        js.contains("$$props.name"),
        "the bare named read must de-localize to $$props.name:\n{js}"
    );
    // NEGATIVE: a BARE named read opens NO context frame, and the rest name never
    // mis-lowers to `$$props.rest` (the pre-classified `Prop` read-rewrite trap).
    assert!(
        !js.contains("$.push($$props, true)") && !js.contains("$.pop()"),
        "a bare named read must NOT open the component context frame:\n{js}"
    );
    assert!(
        !js.contains("$$props.rest"),
        "the rest binding must stay the real local, never `$$props.rest`:\n{js}"
    );
}

// ── native Svelte client `$props()` rest + whole-object capture ──
// Emission POSITIVES (each pinned against svelte@5.56.10; discriminating with a
// NEGATIVE assertion), then the §10a malformed-sibling fail-closed enumeration.

#[test]
fn props_rest_lone_lowers_prefix_only_set() {
    // POSITIVE 1: a lone `{ ...rest }` (no named siblings) hoists the PREFIX-ONLY
    // `rest_excludes` Set and lowers the `$.rest_props` capture. The bare `{...rest}`
    // element spread opens NO context frame.
    let js = emit(
        "<script>let { ...rest } = $props();</script>\n<div {...rest}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("var rest_excludes = new Set(['$$slots', '$$events', '$$legacy']);"),
        "a lone rest hoists the prefix-only Set:\n{js}"
    );
    assert!(
        js.contains("let rest = $.rest_props($$props, rest_excludes);"),
        "missing the $.rest_props capture:\n{js}"
    );
    // NEGATIVE: no named source key leaked into the prefix-only Set, no context frame.
    assert!(
        !js.contains("'legacy', '"),
        "a lone rest Set must carry NO named keys after the prefix:\n{js}"
    );
    assert!(
        !js.contains("$.push($$props, true)"),
        "a lone rest with only a spread opens NO context frame:\n{js}"
    );
}

#[test]
fn props_rest_named_siblings_delocalize_no_context() {
    // POSITIVE 2: `{ a, b, ...rest }` with bare `{a}{b}` reads — the Set carries the
    // prefix then the source keys `a`,`b` IN SOURCE ORDER; each bare named read
    // de-localizes to `$$props.KEY`; a bare named read opens NO context frame.
    let js = emit(
        "<script>let { a, b, ...rest } = $props();</script>\n<p>{a}{b}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("var rest_excludes = new Set(['$$slots', '$$events', '$$legacy', 'a', 'b']);"),
        "the Set carries the source keys in source order:\n{js}"
    );
    assert!(
        js.contains("let rest = $.rest_props($$props, rest_excludes);"),
        "missing the $.rest_props capture:\n{js}"
    );
    assert!(
        js.contains("$$props.a") && js.contains("$$props.b"),
        "bare named reads de-localize to $$props.KEY:\n{js}"
    );
    // NEGATIVE: no $.prop declarations (no defaults/writes), no context frame.
    assert!(
        !js.contains("$.prop($$props"),
        "no-default named siblings emit NO $.prop declaration:\n{js}"
    );
    assert!(
        !js.contains("$.push($$props, true)"),
        "bare named reads open NO context frame:\n{js}"
    );
}

#[test]
fn props_rest_alias_excludes_source_key() {
    // POSITIVE 3: `{ a: local, ...rest }` — the exclude Set + the read both use the
    // SOURCE key `a`, not the alias `local`.
    let js = emit(
        "<script>let { a: local, ...rest } = $props();</script>\n<p>{local}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("var rest_excludes = new Set(['$$slots', '$$events', '$$legacy', 'a']);"),
        "the alias exclude uses the SOURCE key `a`:\n{js}"
    );
    assert!(
        js.contains("$$props.a"),
        "the aliased read uses the source key `$$props.a`:\n{js}"
    );
    // NEGATIVE: never the alias local name in the Set or the read.
    assert!(
        !js.contains("'local'") && !js.contains("$$props.local"),
        "the alias local name must NOT appear in the Set or the read:\n{js}"
    );
}

#[test]
fn props_rest_string_key_excludes_and_bracket_reads() {
    // POSITIVE 4: `{ 'data-x': dx, ...rest }` — the exclude Set carries the string
    // key `data-x` and the read is BRACKET access (`$$props['data-x']`, not dotted).
    let js = emit(
        "<script>let { 'data-x': dx, ...rest } = $props();</script>\n<p>{dx}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("var rest_excludes = new Set(['$$slots', '$$events', '$$legacy', 'data-x']);"),
        "the string key `data-x` is excluded:\n{js}"
    );
    assert!(
        js.contains("$$props['data-x']"),
        "a non-identifier source key reads via bracket access:\n{js}"
    );
    // NEGATIVE: never dotted access for a hyphenated key (invalid JS).
    assert!(
        !js.contains("$$props.data"),
        "a hyphenated key must NOT read via dotted access:\n{js}"
    );
}

#[test]
fn props_rest_composes_with_default() {
    // POSITIVE 5: `{ a = 1, ...rest }` composes the `$.prop` default with the
    // rest capture into ONE `let` — the default decl FIRST, the rest decl LAST (its
    // source position), comma-joined.
    let js = emit(
        "<script>let { a = 1, ...rest } = $props();</script>\n<p>{a}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "let a = $.prop($$props, 'a', 3, 1), rest = $.rest_props($$props, rest_excludes);"
        ),
        "the default + rest compose into one comma-joined `let`, rest last:\n{js}"
    );
    assert!(
        js.contains("var rest_excludes = new Set(['$$slots', '$$events', '$$legacy', 'a']);"),
        "the defaulted key `a` is still excluded:\n{js}"
    );
    // NEGATIVE: a defaulted prop reads as the getter, never `$$props.a`.
    assert!(
        !js.contains("$$props.a"),
        "a defaulted prop reads via the getter `a()`, never `$$props.a`:\n{js}"
    );
}

#[test]
fn props_rest_whole_read_stays_local_no_context() {
    // POSITIVE 6: a WHOLE `rest` read — a bare template interpolation `{rest}` AND a
    // bare state-write handler RHS (`() => sink = rest`) — stays the real local `rest`
    // and opens NO context frame (only a MEMBER read through the binding does).
    let js = emit(
        "<script>let { ...rest } = $props(); let sink = $state(null);</script>\n<button onclick={() => sink = rest}>{rest}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.set_text(text, rest)"),
        "a bare template read stays the real local `rest`:\n{js}"
    );
    assert!(
        js.contains("$.set(sink, rest"),
        "a bare handler-RHS read stays the real local `rest`:\n{js}"
    );
    // NEGATIVE: never `$$props.rest` (the pre-classified `Prop` read trap), and a
    // whole read opens NO context frame.
    assert!(
        !js.contains("$$props.rest"),
        "a whole read must stay `rest`, never `$$props.rest`:\n{js}"
    );
    assert!(
        !js.contains("$.push($$props, true)"),
        "a whole read opens NO context frame:\n{js}"
    );
}

#[test]
fn props_rest_component_spread_lowers_spread_props() {
    // POSITIVE 8: a component spread `<Child {...rest} />` lowers to the bare-thunk
    // `$.spread_props(() => rest)` (NOT the element `$.attribute_effect` fold).
    let js = emit(
        "<script>import Child from './Child.svelte'; let { a, ...rest } = $props();</script>\n<Child {...rest} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.spread_props(() => rest)"),
        "a component spread lowers to $.spread_props(() => rest):\n{js}"
    );
    // NEGATIVE: never the element-spread fold, never `$$props.rest`.
    assert!(
        !js.contains("$.attribute_effect") && !js.contains("$$props.rest"),
        "a component spread is NOT the element attribute_effect fold:\n{js}"
    );
}

#[test]
fn props_rest_nonexcluded_member_read_delocalizes_with_context() {
    // POSITIVE 9: `{ a, ...rest }` with a NON-excluded member read `rest.x` (in a
    // state-write handler — the rewriter path; a TEMPLATE member interpolation is the
    // pre-existing reactive-text-completion deferral, not a rest/whole-capture surface) —
    // de-localizes to `$$props.x` AND opens the context frame (a member read through
    // the rest binding). Oracle: `rest.member-in-handler`.
    let js = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += rest.x}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$$props.x"),
        "a non-excluded rest member de-localizes to $$props.x:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true)") && js.contains("$.pop()"),
        "a rest member read opens the context frame:\n{js}"
    );
    // NEGATIVE: the source member `rest.x` is REWRITTEN away (not left verbatim).
    assert!(
        !js.contains("rest.x"),
        "a non-excluded rest member must NOT stay `rest.x`:\n{js}"
    );
}

#[test]
fn props_rest_excluded_member_read_stays_local_with_context() {
    // POSITIVE 10: `{ a, ...rest }` with an EXCLUDED member read `rest.a` (state-write
    // handler) — STAYS the verbatim `rest.a` (semantically `undefined` — the rest
    // object excludes `a`) AND still opens the context frame. Oracle:
    // `rest.named-member-via-rest`.
    let js = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += rest.a}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("rest.a"),
        "an excluded rest member stays the verbatim `rest.a`:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true)") && js.contains("$.pop()"),
        "a rest member read opens the context frame:\n{js}"
    );
    // NEGATIVE: an excluded key is NOT de-localized to `$$props.a`.
    assert!(
        !js.contains("$$props.a"),
        "an excluded rest member must NOT de-localize to $$props.a:\n{js}"
    );
}

#[test]
fn props_named_members_and_rest_share_one_declarator_plan() {
    // FIX-5 unification anchor: a `$props()` destructure with a DEFAULT-bearing
    // named member (`a = 1` → prop source), a no-default named member (`b` →
    // `$$props.b`), AND a rest binding drives ALL THREE consumers off the ONE
    // `PropsDeclaratorPlan` — the read forms, the `$.rest_props` hoist (excludes
    // derived from the SAME named members), and the `$.prop` destructure
    // lowering — coherently. Behavior-preserving; oracle `svelte@5.56.10`:
    // `new Set(['$$slots','$$events','$$legacy','a','b'])`, `let a = $.prop(...)`,
    // `$$props.b`, `a()`, `rest.c` → `$$props.c`.
    let js = emit(
        "<script>let { a = 1, b, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += a + b + rest.c}>x</button>\n",
        "App.svelte",
    );
    // (1) The rest hoist excludes are the fixed prefix THEN each named source key
    // in source order — derived from the same plan's members (single authority).
    assert!(
        js.contains("new Set(['$$slots', '$$events', '$$legacy', 'a', 'b'])"),
        "the rest_excludes Set derives its keys from the named members:\n{js}"
    );
    // (2) The default-bearing named member is a prop source (`$.prop`, flag 3,
    // default 1), read as the getter `a()`.
    assert!(
        js.contains("let a = $.prop($$props, 'a', 3, 1)") && js.contains("a()"),
        "the default-bearing named member lowers to a $.prop source read as a getter:\n{js}"
    );
    // (3) The rest capture declarator rides the SAME hoisted Set name.
    assert!(
        js.contains("rest = $.rest_props($$props, rest_excludes)"),
        "the rest capture declarator references the hoisted rest_excludes Set:\n{js}"
    );
    // (4) The no-default named member reads off `$$props`; the non-excluded rest
    // member de-localizes to `$$props.c`.
    assert!(
        js.contains("$$props.b") && js.contains("$$props.c"),
        "a no-default member reads $$props.b and a non-excluded rest member de-localizes to $$props.c:\n{js}"
    );
    // NEGATIVE: the non-excluded rest member is NOT left verbatim `rest.c`.
    assert!(
        !js.contains("rest.c"),
        "a non-excluded rest member must de-localize, not stay verbatim `rest.c`:\n{js}"
    );
}

#[test]
fn props_whole_object_member_delocalizes_with_context() {
    // POSITIVE 11a: whole-object capture `let all = $props()` with a member read
    // `all.a` (state-write handler) — prefix-only Set + `let all = $.rest_props(...)` +
    // the member de-localizes to `$$props.a` + context frame. (Prefix-only means EVERY
    // non-`$$` key de-localizes.) Oracle: `baseline.whole-object` (member read).
    let js = emit(
        "<script>let all = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += all.a}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("var rest_excludes = new Set(['$$slots', '$$events', '$$legacy']);"),
        "a whole-object capture hoists the prefix-only Set:\n{js}"
    );
    assert!(
        js.contains("let all = $.rest_props($$props, rest_excludes);"),
        "a whole-object capture lowers `let all = $.rest_props(...)`:\n{js}"
    );
    assert!(
        js.contains("$$props.a"),
        "a whole-object member de-localizes to $$props.a:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true)") && js.contains("$.pop()"),
        "a whole-object member read opens the context frame:\n{js}"
    );
    // NEGATIVE: the member `all.a` is rewritten away, and the bare-`all` read trap
    // (`$$props.all`) never fires.
    assert!(
        !js.contains("all.a") && !js.contains("$$props.all"),
        "a whole-object member must de-localize; the bare-read trap must not fire:\n{js}"
    );
}

#[test]
fn props_whole_object_spread_and_bare_read_stay_local_no_context() {
    // POSITIVE 11b: whole-object capture bare + spread reads stay the real local
    // `all` and open NO context frame (an element spread `{...all}` folds to
    // `$.attribute_effect`; a bare interpolation `{all}` stays `all`).
    let js = emit(
        "<script>let all = $props();</script>\n<div {...all}></div>\n<p>{all}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.attribute_effect(div, () => ({ ...all }))"),
        "a whole-object element spread folds to attribute_effect over `all`:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, all)"),
        "a whole-object bare interpolation stays the real local `all`:\n{js}"
    );
    // NEGATIVE: never `$$props.all`, and spreads/bare reads open NO context frame.
    assert!(
        !js.contains("$$props.all"),
        "a whole-object bare/spread read must stay `all`, never `$$props.all`:\n{js}"
    );
    assert!(
        !js.contains("$.push($$props, true)"),
        "whole-object spread + bare reads open NO context frame:\n{js}"
    );
}

#[test]
fn props_whole_object_component_spread_lowers_spread_props() {
    // POSITIVE 11c: whole-object capture into a component spread `<Child {...all} />`
    // → `$.spread_props(() => all)`.
    let js = emit(
        "<script>import Child from './Child.svelte'; let all = $props();</script>\n<Child {...all} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.spread_props(() => all)"),
        "a whole-object component spread lowers to $.spread_props(() => all):\n{js}"
    );
    assert!(
        !js.contains("$$props.all"),
        "the spread payload is the real local `all`:\n{js}"
    );
}

// ── §10a: malformed-sibling fail-closed enumeration (one discriminating test per
// sibling; each fails if its refusal arm were removed). ──

#[test]
fn props_rest_call_with_arg_fails_closed() {
    // `$props(x)` — the only accepted call shape is zero-arg. One arg is the
    // surviving invalid-arguments arm.
    assert_fail_closed(
        "<script>let { a, ...rest } = $props(x);</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() invalid arguments"),
    );
}

#[test]
fn props_rest_call_with_two_args_fails_closed() {
    // `$props(x, y)` — invalid arguments (surviving arm).
    assert_fail_closed(
        "<script>let { a, ...rest } = $props(x, y);</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() invalid arguments"),
    );
}

#[test]
fn props_rest_call_with_spread_arg_fails_closed() {
    // `$props(...x)` — invalid arguments (surviving arm; a spread arg is non-empty).
    assert_fail_closed(
        "<script>let { a, ...rest } = $props(...x);</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() invalid arguments"),
    );
}

#[test]
fn props_whole_object_computed_key_member_stays_local() {
    // A COMPUTED member on the whole-object binding (`all['x']`, in a state-write
    // handler) stays the verbatim real local — EXACT oracle svelte@5.56.10 parity,
    // NOT a deferral: the key-aware de-localization is STATIC-member-only, and the
    // oracle likewise keeps a computed member (`all['x']`) verbatim even with a
    // static string-literal key (de-localizing a computed member would REGRESS
    // against the oracle). The load-bearing invariant here is that it must NOT
    // mis-fire the bare-read `$$props.all` trap (NUANCE-2), and still opens the
    // context frame (a member expression rooted at the unsafe binding).
    let js = emit(
        "<script>let all = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += all['x']}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("all['x']") && js.contains("$.push($$props, true)"),
        "a computed member stays `all['x']` with the context frame:\n{js}"
    );
    assert!(
        !js.contains("$$props.all"),
        "a computed member must not mis-lower to the bare-read `$$props.all`:\n{js}"
    );
}

#[test]
fn props_rest_parenthesized_member_read_delocalizes() {
    // FIX-1: author parens around the IMMEDIATE member object are transparent —
    // `(rest).x` / `((rest)).x` / `(all).a` de-localize to `$$props.KEY` exactly
    // like the bare form. Oracle svelte@5.56.10: `$$props.x` / `$$props.a`. RED at
    // c0c2ff2aa (the Identifier guard blocked the paren, leaving `(rest).x`).
    let rest = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += (rest).x + ((rest)).z}>x</button>\n",
        "App.svelte",
    );
    assert!(
        rest.contains("$$props.x") && rest.contains("$$props.z"),
        "a parenthesized rest member de-localizes to $$props.KEY:\n{rest}"
    );
    assert!(
        !rest.contains("(rest).x") && !rest.contains("((rest)).z"),
        "the parenthesized object must not survive verbatim:\n{rest}"
    );
    // Whole-object capture through parens.
    let whole = emit(
        "<script>let all = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += (all).a}>x</button>\n",
        "App.svelte",
    );
    assert!(
        whole.contains("$$props.a") && !whole.contains("(all).a"),
        "a parenthesized whole-object member de-localizes to $$props.a:\n{whole}"
    );
}

#[test]
fn props_rest_parenthesized_member_does_not_rekey_nested_chain() {
    // FIX-1 control: the paren-peel is OBJECT-ONLY — a nested chain `(rest).x.y`
    // stays keyed on its ROOT property `x` (the inner member de-localizes `x`),
    // never re-keyed to `y`. Oracle: `$$props.x.y`.
    let js = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += (rest).x.y}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$$props.x.y"),
        "a paren-wrapped nested chain keys on the root property x:\n{js}"
    );
    // NEGATIVE: never re-keyed to `y` alone, never left verbatim.
    assert!(
        !js.contains("$$props.y") && !js.contains("(rest).x.y"),
        "the chain must not re-key to y nor stay verbatim:\n{js}"
    );
}

#[test]
fn props_rest_member_write_target_stays_verbatim() {
    // FIX-2: de-localization is READ-only. A DIRECT member-write target — plain
    // `=`, prefix/postfix `++`/`--`, compound `+=`, logical `??=` — stays the
    // verbatim `rest.KEY`. RED at c0c2ff2aa (which de-localized the write LHS to
    // `$$props.x = 1`).
    //
    // First-hand oracle re-probe (svelte@5.56.10): EVERY row is emitted FULLY
    // verbatim, including the compound/logical forms:
    //   `rest.x += 1` → `rest.x += 1`   `rest.x ??= 1` → `rest.x ??= 1`
    // svelte's `build_assignment_value` pre-rewrite (`+=` → `rest.x = rest.x + 1`,
    // which would de-localize the injected RHS read) fires ONLY when the target
    // FORCES a transform (a signal / `$state` LHS). `rest` is a PLAIN local
    // (`let rest = $.rest_props(...)`), so the assignment is left untouched and the
    // `rest.x` grand-parent stays the AssignmentExpression → the coarse guard keeps
    // BOTH sides local. There is NO injected `rest.x = $$props.x + 1`; the assertion
    // below (full verbatim, no `$$props.x`) IS oracle parity — invalid-input write
    // to a read-only rest prop, kept locally (never a silent `$$props` mutation).
    for (label, stmt, verbatim) in [
        ("assign", "rest.x = 1", "rest.x = 1"),
        ("postfix", "rest.x++", "rest.x++"),
        ("prefix", "++rest.x", "++rest.x"),
        ("compound", "rest.x += 1", "rest.x += 1"),
        ("logical", "rest.x ??= 1", "rest.x ??= 1"),
    ] {
        let src = format!(
            "<script>let {{ a, ...rest }} = $props(); let sink = $state(0);</script>\n<button onclick={{() => {{ {stmt}; sink += 1; }}}}>x</button>\n"
        );
        let js = emit(&src, "App.svelte");
        assert!(
            js.contains(verbatim),
            "[{label}] the member-write target stays verbatim `{verbatim}`:\n{js}"
        );
        // NEGATIVE: the write target never de-localizes to the raw `$$props` bag.
        assert!(
            !js.contains("$$props.x"),
            "[{label}] a member-write target must NOT de-localize to $$props.x:\n{js}"
        );
    }
}

#[test]
fn props_rest_paren_write_verbatim_but_paren_read_delocalizes() {
    // FIX-1 + FIX-2 order-coupling: after the paren-peel exposes `(rest).x` to the
    // disposition, a paren-wrapped WRITE target still stays verbatim while a
    // paren-wrapped READ de-localizes — the read/write split fires THROUGH the
    // paren. Oracle: write `(rest).x = 1` keeps the local; read `(rest).x` →
    // `$$props.x`.
    let js = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => { (rest).x = 1; sink += (rest).x; }}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("(rest).x = 1"),
        "the paren-wrapped write target stays local:\n{js}"
    );
    assert!(
        js.contains("$$props.x"),
        "the paren-wrapped read de-localizes to $$props.x:\n{js}"
    );
}

#[test]
fn props_rest_deep_lvalue_root_member_delocalizes() {
    // FIX-2 deep lvalue: for `rest.x.y = 1` the DIRECT write target is `rest.x.y`
    // (verbatim), but its ROOT member `rest.x` is a READ sub-expression that
    // de-localizes — so the whole lvalue lowers to `$$props.x.y = 1`. Oracle
    // svelte@5.56.10.
    let js = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => { rest.x.y = 1; sink += 1; }}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$$props.x.y = 1"),
        "a deep lvalue de-localizes its root member: $$props.x.y = 1:\n{js}"
    );
    assert!(
        !js.contains("rest.x.y = 1"),
        "the deep lvalue root member must not stay verbatim:\n{js}"
    );
}

#[test]
fn props_rest_plain_assign_bare_member_rhs_stays_verbatim() {
    // FIX-6: the oracle's `rest`→`$$props` de-localization is a COARSE,
    // position-based guard (svelte@5.56.10 `Identifier.js`): the `rest` identifier
    // rewrites to `$$props` only when `grand_parent.type !== 'AssignmentExpression'
    // && grand_parent.type !== 'UpdateExpression'`. A single static `rest.KEY`
    // that is the ENTIRE right-hand side of a PLAIN `=` therefore stays VERBATIM —
    // the `=` operator returns `right` unchanged from `build_assignment_value`, so
    // the member's grand-parent stays the AssignmentExpression and the guard fails.
    // Paren-transparent (official ESTree has no paren node) and LHS-agnostic (the
    // guard never inspects the target). First-hand oracle svelte@5.56.10:
    // `sink = rest.y` → `$.set(sink, rest.y, true)`. RED at ba4af31bc (Verter's
    // read/write split over-de-localized the bare-RHS READ to `$$props.y`).

    // (1) Bare `rest.KEY` as the entire RHS of a plain `=` → verbatim `rest.y`.
    let js = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink = rest.y}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("rest.y"),
        "a plain-`=` bare-member RHS must stay verbatim `rest.y`:\n{js}"
    );
    assert!(
        !js.contains("$$props.y"),
        "a plain-`=` bare-member RHS must NOT de-localize to `$$props.y`:\n{js}"
    );

    // (2) Paren-transparent — `sink = (rest.y)` is the SAME AST → verbatim.
    let paren = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink = (rest.y)}>x</button>\n",
        "App.svelte",
    );
    assert!(
        paren.contains("rest.y") && !paren.contains("$$props.y"),
        "a paren-wrapped plain-`=` bare-member RHS stays verbatim:\n{paren}"
    );

    // (3) CONTROL — compound `+=` RHS. `sink` is a signal, so svelte pre-rewrites
    // the assignment (`$.set(sink, $.get(sink) + rest.x)`), moving `rest.x` under a
    // Binary node → the guard passes → de-localizes. First-hand oracle: `$$props.x`.
    let compound = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += rest.x}>x</button>\n",
        "App.svelte",
    );
    assert!(
        compound.contains("$$props.x") && !compound.contains("rest.x"),
        "a compound-`+=` RHS must de-localize to `$$props.x` (NOT verbatim):\n{compound}"
    );

    // (4) CONTROL — binary operand: `sink = rest.y + 1`. The member is under a
    // Binary, NOT the direct RHS → de-localizes. First-hand oracle: `$$props.y + 1`.
    let binary = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink = rest.y + 1}>x</button>\n",
        "App.svelte",
    );
    assert!(
        binary.contains("$$props.y") && !binary.contains("rest.y"),
        "a binary-operand RHS must de-localize to `$$props.y` (NOT verbatim):\n{binary}"
    );

    // (5) CONTROL — excluded key: `sink = rest.a` stays verbatim because `a` is a
    // rest EXCLUDE (a distinct reason from the coarse Assignment-guard, same
    // verbatim result). First-hand oracle: `rest.a`.
    let excluded = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink = rest.a}>x</button>\n",
        "App.svelte",
    );
    assert!(
        excluded.contains("rest.a") && !excluded.contains("$$props.a"),
        "an excluded-key bare RHS stays verbatim `rest.a`:\n{excluded}"
    );

    // (6) whole-object equivalents — `all = $props()` behaves identically: the bare
    // RHS stays verbatim, the binary operand de-localizes. First-hand oracle:
    // `sink = all.a` → `$.set(sink, all.a, true)`; `sink = all.a + 1` → `$$props.a`.
    let whole = emit(
        "<script>let all = $props(); let sink = $state(0);</script>\n<button onclick={() => sink = all.a}>x</button>\n",
        "App.svelte",
    );
    assert!(
        whole.contains("all.a") && !whole.contains("$$props.a"),
        "a whole-object plain-`=` bare-member RHS stays verbatim `all.a`:\n{whole}"
    );
    let whole_binary = emit(
        "<script>let all = $props(); let sink = $state(0);</script>\n<button onclick={() => sink = all.a + 1}>x</button>\n",
        "App.svelte",
    );
    assert!(
        whole_binary.contains("$$props.a") && !whole_binary.contains("all.a"),
        "a whole-object binary-operand RHS de-localizes to `$$props.a`:\n{whole_binary}"
    );
}

// ── optional-chain rest/whole member reads PRESERVE the `?.` ──
// De-localization replaces ONLY the object identifier (`rest`/`all` → `$$props`),
// never the whole member span, so the optional axis, property spelling, and any
// downstream chain stay verbatim from source. Each correctness test is RED at
// ff1ca89a1 (whole-member replacement dropped the `?.`) and GREEN after the fix.

#[test]
fn props_rest_optional_member_read_preserves_optional_chain() {
    // Oracle svelte@5.56.10: `rest?.x` → `$$props?.x` (attr AND handler); the CONTROL
    // `rest.x` → `$$props.x` (dotted). RED at ff1ca89a1: whole-member replacement
    // emitted the `?.`-dropped `$$props.x` for the optional form.
    let opt = emit(
        "<script>let { a, ...rest } = $props();</script>\n<div title={rest?.x}></div>\n",
        "App.svelte",
    );
    assert!(
        opt.contains("$$props?.x"),
        "an optional rest member preserves `?.` → $$props?.x:\n{opt}"
    );
    // NEGATIVE (the pre-fix miscompile): the `?.`-dropped `$$props.x` must be ABSENT.
    assert!(
        !opt.contains("$$props.x"),
        "the optional axis must NOT be dropped to `$$props.x`:\n{opt}"
    );

    // CONTROL — the non-optional `rest.x` is byte-identical to before (dotted).
    let ctrl = emit(
        "<script>let { a, ...rest } = $props();</script>\n<div title={rest.x}></div>\n",
        "App.svelte",
    );
    assert!(
        ctrl.contains("$$props.x") && !ctrl.contains("$$props?.x"),
        "the non-optional control stays the dotted `$$props.x`:\n{ctrl}"
    );

    // Handler context (`onclick={() => sink += rest?.x}`) — the same rewriter path.
    let hdlr = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += rest?.x}>x</button>\n",
        "App.svelte",
    );
    assert!(
        hdlr.contains("$$props?.x") && !hdlr.contains("$$props.x"),
        "an optional rest member in a handler preserves `?.`:\n{hdlr}"
    );
}

#[test]
fn props_whole_object_optional_member_preserves_optional_chain() {
    // Whole-object capture `let all = $props()` behaves identically: `all?.x` →
    // `$$props?.x`. Oracle svelte@5.56.10. RED at ff1ca89a1 (dropped to `$$props.x`).
    let js = emit(
        "<script>let all = $props();</script>\n<div title={all?.x}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$$props?.x"),
        "a whole-object optional member preserves `?.` → $$props?.x:\n{js}"
    );
    assert!(
        !js.contains("$$props.x") && !js.contains("$$props.all"),
        "the whole-object optional axis must not drop, nor fire the bare-read trap:\n{js}"
    );
    // Whole-object equivalent in a handler.
    let hdlr = emit(
        "<script>let all = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += all?.x}>x</button>\n",
        "App.svelte",
    );
    assert!(
        hdlr.contains("$$props?.x") && !hdlr.contains("$$props.x"),
        "a whole-object optional member in a handler preserves `?.`:\n{hdlr}"
    );
}

#[test]
fn props_rest_optional_member_chain_preserves_every_optional_hop() {
    // A downstream chain stays verbatim from source: `rest?.x.y` → `$$props?.x.y`
    // (the inner `?.x` de-localizes the ROOT, `.y` verbatim); a CHAINED optional
    // `rest?.x?.y` → `$$props?.x?.y` keeps BOTH `?.`. Oracle svelte@5.56.10. RED at
    // ff1ca89a1 (dropped the first `?.`: `$$props.x.y` / `$$props.x?.y`).
    let mixed = emit(
        "<script>let { a, ...rest } = $props();</script>\n<div title={rest?.x.y}></div>\n",
        "App.svelte",
    );
    assert!(
        mixed.contains("$$props?.x.y"),
        "an optional-then-static chain preserves the head `?.`: $$props?.x.y:\n{mixed}"
    );
    assert!(
        !mixed.contains("$$props.x.y"),
        "the head `?.` must not drop to `$$props.x.y`:\n{mixed}"
    );

    let chained = emit(
        "<script>let { a, ...rest } = $props();</script>\n<div title={rest?.x?.y}></div>\n",
        "App.svelte",
    );
    assert!(
        chained.contains("$$props?.x?.y"),
        "a chained optional preserves BOTH `?.`: $$props?.x?.y:\n{chained}"
    );
    // NEGATIVE: the pre-fix first-hop-dropped form `$$props.x?.y` must be absent.
    assert!(
        !chained.contains("$$props.x?.y"),
        "the first `?.` must not drop to `$$props.x?.y`:\n{chained}"
    );
}

#[test]
fn props_rest_optional_receiver_call_preserves_optional_chain() {
    // §10a sibling — an optional-RECEIVER call `rest?.x()` in ATTR context (the
    // handler-statement form hits the pre-existing NonDelegatedEvent deferral, out of
    // scope) de-localizes the callee to `$$props?.x()`. Oracle svelte@5.56.10:
    // `$$props?.x()`. RED at ff1ca89a1 (emitted `$$props.x()`).
    let js = emit(
        "<script>let { a, ...rest } = $props();</script>\n<div title={rest?.x()}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$$props?.x()"),
        "an optional-receiver call preserves `?.`: $$props?.x():\n{js}"
    );
    assert!(
        !js.contains("$$props.x()"),
        "the optional-receiver `?.` must not drop to `$$props.x()`:\n{js}"
    );
}

#[test]
fn props_rest_optional_computed_member_stays_verbatim() {
    // §10a boundary control — an OPTIONAL COMPUTED member `rest?.['x']` is NOT the
    // static-member path (a distinct AST node), so it stays the verbatim real local,
    // EXACTLY like the non-optional computed `rest['x']`. Oracle svelte@5.56.10:
    // `rest?.['x']` verbatim. Guards the fix against leaking into the computed path.
    let js = emit(
        "<script>let { a, ...rest } = $props();</script>\n<div title={rest?.['x']}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("rest?.['x']"),
        "an optional computed member stays verbatim `rest?.['x']`:\n{js}"
    );
    // NEGATIVE: it must NOT de-localize to any `$$props`-rooted computed access.
    assert!(
        !js.contains("$$props?.['x']") && !js.contains("$$props['x']") && !js.contains("$$props.x"),
        "an optional computed member must not de-localize:\n{js}"
    );
}

#[test]
fn props_rest_optional_excluded_member_stays_verbatim() {
    // §10a boundary control — an EXCLUDED key under `?.` (`rest?.a` where `a` is a
    // named prop the rest binding excludes) stays the verbatim `rest?.a` via the
    // untouched `!excludes.contains(key)` guard. Oracle svelte@5.56.10: `rest?.a`.
    let js = emit(
        "<script>let { a, ...rest } = $props();</script>\n<div title={rest?.a}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("rest?.a"),
        "an excluded key under `?.` stays verbatim `rest?.a`:\n{js}"
    );
    assert!(
        !js.contains("$$props?.a") && !js.contains("$$props.a"),
        "an excluded key must not de-localize even under `?.`:\n{js}"
    );
}

#[test]
fn props_rest_optional_illegal_name_member_fails_closed() {
    // §10a sibling — a `$$`-prefixed member under `?.` (`rest?.$$slots` / `all?.$$slots`)
    // is the reserved magic namespace: an OFFICIAL compile error fired ABOVE the
    // rewrite regardless of the optional axis. Oracle svelte@5.56.10: reject
    // `props_illegal_name`. The `?.` does not relax the refuse.
    for src in [
        "<script>let { a, ...rest } = $props();</script>\n<div title={rest?.$$slots}></div>\n",
        "<script>let all = $props();</script>\n<div title={all?.$$slots}></div>\n",
    ] {
        let err = emit_result(src).expect_err("an optional $$-member must fail closed");
        assert!(
            matches!(&err, ClientCompileError::OfficialReject(r)
                if r.rule == CoreOfficialValidationRule::PropsIllegalName
                && r.official_code == "props_illegal_name"),
            "an optional $$-member is OfficialReject(props_illegal_name), got {err:?}"
        );
        // NEGATIVE: never a silently-emitted module, never the magic-identifier surface.
        assert!(
            !matches!(
                &err,
                ClientCompileError::Unsupported(
                    UnsupportedSvelteRuntimeSurface::MagicIdentifier { .. }
                )
            ),
            "an optional $$-member must not route through the magic-identifier surface: {err:?}"
        );
    }
}

#[test]
fn props_rest_optional_write_target_fails_closed() {
    // §10a sibling — an optional-chain expression is NOT a valid assignment target in
    // JavaScript, so `rest?.x = 1` / `rest?.x += 1` are JS PARSE ERRORS that never
    // reach the rewriter. Oracle svelte@5.56.10: `js_parse_error`. Verter fails closed
    // through the template-expression parse channel (`svelte-runtime-expr-parse`),
    // never a silent module.
    for stmt in ["rest?.x = 1", "rest?.x += 1"] {
        let src = format!(
            "<script>let {{ a, ...rest }} = $props();</script>\n<div title={{{stmt}}}></div>\n"
        );
        match emit_result(&src) {
            Err(ClientCompileError::Lowering(errs)) => {
                assert!(
                    errs.diagnostics
                        .iter()
                        .any(|d| d.code == "svelte-runtime-expr-parse"),
                    "[{stmt}] an optional-chain assignment target must fail via the expr-parse channel:\n{errs:?}"
                );
            }
            Ok(js) => panic!(
                "[{stmt}] expected fail-closed for an optional-chain write, got a module:\n{js}"
            ),
            Err(other) => panic!("[{stmt}] expected an expr-parse lowering error, got: {other:?}"),
        }
    }
}

#[test]
fn props_rest_compound_assign_nonsignal_target_stays_verbatim() {
    // FIX-7 (class closure): the oracle's coarse Assignment-child guard keeps a bare
    // `rest.KEY` / `all.KEY` that is the WHOLE RHS of an assignment VERBATIM unless
    // svelte's own `AssignmentExpression` pre-rewrite re-parents the RHS under a
    // Binary/Logical. That pre-rewrite (`build_assignment_value`) fires ONLY for a
    // bare reassignable-SIGNAL-identifier compound target (`sig OP= rhs` →
    // `$.set(sig, $.get(sig) OP rhs)`); a plain-local identifier, ANY object-member
    // target (incl. `$state`), and a rest-member target are NOT pre-rewritten, so the
    // `rest.KEY` grand-parent stays the AssignmentExpression → the guard keeps it
    // verbatim. FIX-6 already covered plain `=` (target-agnostic); FIX-7 extends the
    // verbatim record to a compound/logical `OP=` when `classify_target(left) ∈
    // {PlainIdent, Member}`. RED at 82580a74a (Verter gated the verbatim record on
    // `operator == Assign` ONLY, so it over-de-localized every compound/logical RHS to
    // `$$props.KEY` regardless of target). First-hand oracle svelte@5.56.10 confirms
    // every positive below stays verbatim and every control below de-localizes.

    // (1) $state MEMBER target, compound `+=` — svelte does NOT re-wrap a member
    // target → `objS.p += rest.y` stays verbatim. Oracle: `objS.p += rest.y`.
    let member = emit(
        "<script>let { a, ...rest } = $props(); let objS = $state({ p: 0 });</script>\n<button onclick={() => objS.p += rest.y}>x</button>\n",
        "App.svelte",
    );
    assert!(
        member.contains("objS.p += rest.y") && !member.contains("$$props.y"),
        "a compound `+=` to a $state MEMBER target keeps the RHS verbatim `rest.y`:\n{member}"
    );

    // (2) PLAIN-LOCAL identifier target (block-local in a `$effect`), compound `+=` —
    // svelte early-returns (no re-wrap) → `m += rest.y` stays verbatim. Oracle:
    // `m += rest.y`.
    let plain_ident = emit(
        "<script>let { a, ...rest } = $props(); $effect(() => { let m = 0; m += rest.y; console.log(m); });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        plain_ident.contains("m += rest.y") && !plain_ident.contains("$$props.y"),
        "a compound `+=` to a plain-local IDENT target keeps the RHS verbatim `rest.y`:\n{plain_ident}"
    );

    // (3) PLAIN-LOCAL identifier target, LOGICAL `??=` — same non-re-wrap path.
    // Oracle: `m ??= rest.y`.
    let logical = emit(
        "<script>let { a, ...rest } = $props(); $effect(() => { let m = null; m ??= rest.y; console.log(m); });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        logical.contains("m ??= rest.y") && !logical.contains("$$props.y"),
        "a logical `??=` to a plain-local IDENT target keeps the RHS verbatim `rest.y`:\n{logical}"
    );

    // (4) PLAIN-LOCAL object-MEMBER target — a member target, never re-wrapped.
    // Oracle: `obj.p += rest.y`.
    let plain_member = emit(
        "<script>let { a, ...rest } = $props(); $effect(() => { let obj = { p: 0 }; obj.p += rest.y; console.log(obj); });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        plain_member.contains("obj.p += rest.y") && !plain_member.contains("$$props.y"),
        "a compound `+=` to a plain-local object-MEMBER target keeps the RHS verbatim `rest.y`:\n{plain_member}"
    );

    // (5) whole-object `all.KEY` behaves identically — MEMBER target. Oracle:
    // `objS.p += all.y`.
    let whole_member = emit(
        "<script>let all = $props(); let objS = $state({ p: 0 });</script>\n<button onclick={() => objS.p += all.y}>x</button>\n",
        "App.svelte",
    );
    assert!(
        whole_member.contains("objS.p += all.y") && !whole_member.contains("$$props.y"),
        "a whole-object compound `+=` to a $state MEMBER target keeps the RHS verbatim `all.y`:\n{whole_member}"
    );

    // (6) whole-object `all.KEY` — PLAIN-LOCAL ident target. Oracle: `m += all.y`.
    let whole_ident = emit(
        "<script>let all = $props(); $effect(() => { let m = 0; m += all.y; console.log(m); });</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        whole_ident.contains("m += all.y") && !whole_ident.contains("$$props.y"),
        "a whole-object compound `+=` to a plain-local IDENT target keeps the RHS verbatim `all.y`:\n{whole_ident}"
    );

    // (7) CONTROL — a bare reassignable-SIGNAL identifier target ($state) MUST still
    // de-localize: svelte pre-rewrites `sink += rest.y` → `$.set(sink, $.get(sink) +
    // rhs)`, re-parenting the RHS under a Binary → the guard passes → `$$props.y`.
    // Proves FIX-7 does NOT over-correct. Oracle: `$.set(sink, $.get(sink) + $$props.y)`.
    let signal_compound = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += rest.y}>x</button>\n",
        "App.svelte",
    );
    assert!(
        signal_compound.contains("$$props.y") && !signal_compound.contains("rest.y"),
        "a compound `+=` to a SIGNAL id target must still de-localize to `$$props.y`:\n{signal_compound}"
    );

    // (8) CONTROL — SIGNAL id target, LOGICAL `??=` — same re-wrap → de-localize.
    // Oracle: `$.set(sink, $.get(sink) ?? $$props.y, true)`.
    let signal_logical = emit(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink ??= rest.y}>x</button>\n",
        "App.svelte",
    );
    assert!(
        signal_logical.contains("$$props.y") && !signal_logical.contains("rest.y"),
        "a logical `??=` to a SIGNAL id target must still de-localize to `$$props.y`:\n{signal_logical}"
    );
}

#[test]
fn props_rest_parenthesized_dollar_member_no_longer_fails_open() {
    // FIX-1 closes the FAIL-OPEN: `(rest).$$slots` / `(all).$$slots` reached the
    // verbatim leaf at c0c2ff2aa (emitted an undefined read, NO error). After the
    // paren-peel the `$$`-member reject fires THROUGH the paren, so the component
    // REJECTS. (The exact official code `props_illegal_name` is asserted by the
    // FIX-3 negatives.)
    assert!(
        emit_result(
            "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += (rest).$$slots}>x</button>\n"
        )
        .is_err(),
        "a parenthesized rest $$-member must reject, not fail open"
    );
    assert!(
        emit_result(
            "<script>let all = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += (all).$$slots}>x</button>\n"
        )
        .is_err(),
        "a parenthesized whole-object $$-member must reject, not fail open"
    );
}

#[test]
fn props_rest_nested_pattern_fails_closed() {
    // `{ a: { ...inner } }` — a nested destructure member is the surviving nested arm.
    assert_fail_closed(
        "<script>let { a: { ...inner } } = $props();</script>\n<p>x</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() nested destructure"),
    );
}

#[test]
fn props_rest_computed_sibling_fails_closed() {
    // `{ [k]: v, ...rest }` — a computed-key sibling is the surviving computed arm.
    assert_fail_closed(
        "<script>let { [k]: v, ...rest } = $props();</script>\n<p>{v}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() computed key"),
    );
}

#[test]
fn props_rest_dollar_member_fails_closed() {
    // `rest.$$slots` (in a state-write handler — the rewriter path) — accessing a
    // `$$`-prefixed member of the rest binding is an OFFICIAL compile error
    // (`props_illegal_name`), NOT a deferrable unsupported feature. Verter fails
    // closed carrying the exact official code through the official-reject quadrant.
    // RED at c0c2ff2aa (which routed it as `Unsupported(MagicIdentifier)`).
    let err = emit_result(
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += rest.$$slots}>x</button>\n",
    )
    .expect_err("a $$-member of the rest binding must fail closed");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r)
            if r.rule == CoreOfficialValidationRule::PropsIllegalName
            && r.official_code == "props_illegal_name"),
        "a rest $$-member is OfficialReject(props_illegal_name), got {err:?}"
    );
    // NEGATIVE: no longer the internal magic-identifier unsupported surface.
    assert!(
        !matches!(
            &err,
            ClientCompileError::Unsupported(
                UnsupportedSvelteRuntimeSurface::MagicIdentifier { .. }
            )
        ),
        "the rest $$-member must NOT route through the magic-identifier surface: {err:?}"
    );
}

#[test]
fn props_whole_object_dollar_member_fails_closed() {
    // `all.$$slots` (state-write handler) — same official `props_illegal_name`
    // reject on the whole-object binding.
    let err = emit_result(
        "<script>let all = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += all.$$slots}>x</button>\n",
    )
    .expect_err("a $$-member of the whole-object binding must fail closed");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r)
            if r.rule == CoreOfficialValidationRule::PropsIllegalName
            && r.official_code == "props_illegal_name"),
        "a whole-object $$-member is OfficialReject(props_illegal_name), got {err:?}"
    );
    assert!(
        !matches!(
            &err,
            ClientCompileError::Unsupported(
                UnsupportedSvelteRuntimeSurface::MagicIdentifier { .. }
            )
        ),
        "the whole-object $$-member must NOT route through the magic-identifier surface: {err:?}"
    );
}

#[test]
fn props_rest_parenthesized_dollar_member_is_official_reject() {
    // FIX-1 + FIX-3: the PAREN form `(rest).$$slots` / `(all).$$slots` — a FAIL-OPEN
    // at c0c2ff2aa (emitted an undefined read, NO error) — now rejects THROUGH the
    // paren-peel with the SAME official `props_illegal_name` code as the direct
    // form. Closes the fail-open with the correct quadrant + code.
    for src in [
        "<script>let { a, ...rest } = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += (rest).$$slots}>x</button>\n",
        "<script>let all = $props(); let sink = $state(0);</script>\n<button onclick={() => sink += (all).$$slots}>x</button>\n",
    ] {
        let err = emit_result(src).expect_err("a parenthesized $$-member must fail closed");
        assert!(
            matches!(&err, ClientCompileError::OfficialReject(r)
                if r.rule == CoreOfficialValidationRule::PropsIllegalName
                && r.official_code == "props_illegal_name"),
            "a parenthesized $$-member is OfficialReject(props_illegal_name), got {err:?}"
        );
    }
}

#[test]
fn props_rest_authored_restprops_fails_closed() {
    // Authored `$$restProps` (source, not generated) — the UNTOUCHED runes-mode
    // official-reject gate (`legacy_rest_props_invalid`). Never relaxed by rest
    // support (generated `$$props.KEY` reads never pass the authored-source scan).
    let err =
        emit_result("<script>let { a, ...rest } = $props();</script>\n<p>{$$restProps.x}</p>\n")
            .expect_err("authored $$restProps must fail closed");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "legacy_rest_props_invalid"),
        "authored $$restProps must fail closed with the runes-mode legacy_rest_props_invalid code, got {err:?}"
    );
}

#[test]
fn props_rest_dollar_prefixed_sibling_fails_closed() {
    // `{ $$bad, ...rest }` — a `$$`-prefixed destructure NAME is the UNTOUCHED
    // dollar-prefix official-reject gate.
    let err = emit_result("<script>let { $$bad, ...rest } = $props();</script>\n<p>x</p>\n")
        .expect_err("a $$-prefixed destructure name must fail closed");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "dollar_prefix_invalid"),
        "a $$-prefixed sibling must fail closed at the untouched dollar-prefix gate, got {err:?}"
    );
}

#[test]
fn props_rest_malformed_patterns_fail_closed() {
    // The parser-refused rest siblings — double rest, rest-with-default, and
    // rest-not-last — are rejected by the JS parser (`js_parse_error`); each must
    // fail closed (never a silently emitted module that drops the props).
    for (label, src) in [
        (
            "double-rest",
            "<script>let { ...a, ...b } = $props();</script>\n<p>x</p>\n",
        ),
        (
            "rest-with-default",
            "<script>let { ...rest = {} } = $props();</script>\n<p>x</p>\n",
        ),
        (
            "rest-not-last",
            "<script>let { ...rest, a } = $props();</script>\n<p>x</p>\n",
        ),
    ] {
        assert!(
            emit_result(src).is_err(),
            "the malformed rest pattern [{label}] must fail closed, not emit a module",
        );
    }
}

#[test]
fn svelte_boundary_failed_pending_attribute_form_emits_getter_props() {
    // Official accepts `failed` / `pending` as modern ATTRIBUTE forms
    // (`<svelte:boundary failed={snip}>`) whose value is a state-bearing reference (a snippet
    // ref / prop / signal). Verter now EMITS them through the SAME state-bearing-attribute →
    // props path `onerror` uses: a state-bearing attribute value becomes the getter accessor
    // `get <name>() { return <expr>; }` (SvelteBoundary.js's `has_state ? b.get : b.init`). This
    // is the positive successor to cycle-2's conservative fail-close: the attribute form is a
    // first-class boundary surface, distinct from the `{#snippet}` CHILD form.
    for (src, getter) in [
        (
            "<script>let { snip } = $props();</script>\n<svelte:boundary failed={snip}><p>x</p></svelte:boundary>\n",
            "get failed() { return $$props.snip; }",
        ),
        (
            "<script>let { snip } = $props();</script>\n<svelte:boundary pending={snip}><p>x</p></svelte:boundary>\n",
            "get pending() { return $$props.snip; }",
        ),
    ] {
        let js = emit_result(src).expect("boundary failed/pending attribute form now emits");
        let n = normalize_js_cosmetics(&js);
        assert!(
            n.contains(&nc(&format!("$.boundary(node, {{{getter}}}, ($$anchor) =>"))),
            "the {getter} attribute expr is a getter props member:\n{js}"
        );
        // NEGATIVE: no snippet-const hoist for the pure-attribute form.
        assert!(
            !n.contains(&nc("= ($$anchor, error = $.noop")),
            "the attribute form must NOT hoist a snippet const:\n{js}"
        );
        assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
    }
    // POSITIVE regression: the `{#snippet failed/pending}` CHILD form remains the direct
    // `{ failed }` shorthand surface (the attribute-form support does NOT change the child form).
    let child = emit_result(
        "<script>let { x } = $props();</script>\n<svelte:boundary><p>{x}</p>{#snippet failed(error, reset)}<p>oops</p>{/snippet}</svelte:boundary>\n"
    )
    .expect("the snippet child form still emits");
    assert!(
        normalize_js_cosmetics(&child).contains(&nc("$.boundary(node, {failed}, ($$anchor) =>")),
        "the snippet CHILD form is still the direct failed shorthand (unchanged):\n{child}"
    );
}

#[test]
fn custom_element_props_emit_the_exports_accessor_frame() {
    // A customElement with an explicit `props` definition + a `$props()` member:
    // the accessor frame is FACT-DRIVEN — `$.push($$props, true)` (runes flag
    // `true`), the member becomes a prop SOURCE with the `UPDATED` flag (the
    // official `analysis.accessors` force: flags 7 = IMMUTABLE|RUNES|UPDATED, no
    // default), the `$$exports` get/set pair rides the body (setter param
    // spelled `$$value`, `$.flush()` after the write), and the close returns it
    // (`return $.pop($$exports)`). The define epilogue carries the explicit prop
    // definition object in arg2; arg3/arg4 stay `[]` (accessors ride
    // `$$exports`, never arg4).
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-props', shadow: 'open', props: { count: { reflect: true, type: 'Number' } } }} />\n<script>\n\tlet { count } = $props();\n</script>\n\n<h1>{count}</h1>\n",
    )
    .expect("a customElement with props compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the props frame binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "missing the runes context push:\n{js}"
    );
    assert!(
        js.contains("let count = $.prop($$props, 'count', 7);"),
        "the CE prop source carries flags 7 (IMMUTABLE|RUNES|UPDATED):\n{js}"
    );
    assert!(
        js.contains(
            "var $$exports = { get count() { return count(); }, set count($$value) { count($$value); $.flush(); } };"
        ),
        "missing the $$exports accessor pair:\n{js}"
    );
    assert!(
        js.contains("return $.pop($$exports);"),
        "the context close returns $$exports:\n{js}"
    );
    assert!(
        js.contains(
            "customElements.define('x-props', $.create_custom_element(App, { count: { reflect: true, type: 'Number' } }, [], [], { mode: 'open' }));"
        ),
        "missing the define epilogue with the explicit prop definition:\n{js}"
    );
    // NEGATIVE: the setter param is `$$value` (never `$$v`), and the reactive
    // read goes through the getter (`count()`), not `$$props.count`.
    assert!(!js.contains("$$v)"), "setter param is $$value:\n{js}");
    assert!(
        js.contains("$.set_text(text, count())"),
        "the CE prop read is the getter call:\n{js}"
    );
    assert!(
        !js.contains("$$props.count"),
        "a CE prop source never reads off $$props:\n{js}"
    );
}

#[test]
fn custom_element_string_prop_key_quotes_prop_object_and_accessor_names() {
    // A NON-identifier `$props()` source key under a customElement must emit
    // QUOTED JS everywhere the key surfaces — the official `b.key(name)` rule
    // (identifier-safe names stay bare, anything else becomes a string-literal
    // key). Oracle-adjudicated against pinned `svelte@5.56.10`, which emits:
    //   let dataId = $.prop($$props, 'data-id', 7);
    //   var $$exports = { get 'data-id'() { … }, set 'data-id'($$value) { … } };
    //   customElements.define('x-id', $.create_custom_element(App, { 'data-id': {} }, …));
    // A RAW `data-id` key (`{ data-id: {} }` / `get data-id()`) is INVALID JS.
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-id' }} />\n<script>\n\tlet { 'data-id': dataId } = $props();\n</script>\n\n<p>{dataId}</p>\n",
    )
    .expect("a customElement with a string-keyed $props() member compiles");
    assert!(
        js.contains("let dataId = $.prop($$props, 'data-id', 7);"),
        "the prop source keys off the quoted source key:\n{js}"
    );
    assert!(
        js.contains(
            "var $$exports = { get 'data-id'() { return dataId(); }, set 'data-id'($$value) { dataId($$value); $.flush(); } };"
        ),
        "the $$exports accessor pair quotes the non-identifier accessor name:\n{js}"
    );
    assert!(
        js.contains(
            "customElements.define('x-id', $.create_custom_element(App, { 'data-id': {} }, [], [], { mode: 'open' }));"
        ),
        "the inferred prop-definition key is quoted in the create call:\n{js}"
    );
    // NEGATIVE: no RAW (unquoted) non-identifier key anywhere — each raw form is
    // a JS syntax error.
    assert!(
        !js.contains("get data-id(") && !js.contains("set data-id("),
        "no raw non-identifier accessor name:\n{js}"
    );
    assert!(
        !js.contains("{ data-id:") && !js.contains(", data-id:"),
        "no raw non-identifier prop-object key:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn custom_element_aliased_string_key_explicit_prop_quotes_both_entries() {
    // An EXPLICIT descriptor entry matched by LOCAL name whose member is aliased
    // to a NON-identifier source key: the explicit entry emits under the QUOTED
    // source key, and the inferred remainder loop (which skips only members
    // whose EMITTED key equals a RAW explicit-entry name — the official
    // `if (ce_props[key]) continue`) appends the quoted inferred `{}` entry
    // too. Pinned `svelte@5.56.10` emits BOTH (a valid ES2015+ duplicate-key
    // object literal, last-wins at runtime):
    //   { 'data-id': { reflect: true }, 'data-id': {} }
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-alias-id', props: { dataId: { reflect: true } } }} />\n<script>\n\tlet { 'data-id': dataId } = $props();\n</script>\n\n<p>{dataId}</p>\n",
    )
    .expect("a customElement with an aliased string-keyed explicit prop compiles");
    assert!(
        js.contains(
            "customElements.define('x-alias-id', $.create_custom_element(App, { 'data-id': { reflect: true }, 'data-id': {} }, [], [], { mode: 'open' }));"
        ),
        "the explicit + inferred entries both emit under the quoted source key (official parity):\n{js}"
    );
    assert!(
        js.contains("get 'data-id'() { return dataId(); }"),
        "the accessor name is the quoted source key:\n{js}"
    );
    // NEGATIVE: the explicit entry's LOCAL lookup name never leaks as a key, and
    // no raw unquoted key survives.
    assert!(
        !js.contains("dataId: {"),
        "the explicit entry emits under the member's source key, never the local name:\n{js}"
    );
    assert!(
        !js.contains("{ data-id:") && !js.contains(", data-id:"),
        "no raw non-identifier prop-object key:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn custom_element_aliased_explicit_prop_double_emits_like_official() {
    // F2 adjudication (oracle-grounded): an explicit descriptor entry named by
    // the LOCAL binding of an ALIASED member. Pinned `svelte@5.56.10` emits the
    // explicit metadata under the SOURCE key AND the inferred `{}` remainder for
    // the same key — the inferred loop checks the member's emitted key against
    // the RAW explicit-entry names (`if (ce_props[key]) continue`), which the
    // aliased key misses. The duplicate-key object literal is official parity
    // (valid ES2015+; last entry wins at runtime), NOT a Verter defect —
    // deduplicating here would diverge from the pinned oracle's behavior.
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-alias', props: { foo: { reflect: true } } }} />\n<script>\n\tlet { bar: foo } = $props();\n</script>\n\n<p>{foo}</p>\n",
    )
    .expect("a customElement with an aliased explicit prop compiles");
    assert!(
        js.contains("let foo = $.prop($$props, 'bar', 7);"),
        "the aliased member reads off its SOURCE key:\n{js}"
    );
    assert!(
        js.contains(
            "customElements.define('x-alias', $.create_custom_element(App, { bar: { reflect: true }, bar: {} }, [], [], { mode: 'open' }));"
        ),
        "explicit metadata + inferred `{{}}` both emit under the source key (official parity):\n{js}"
    );
    assert!(
        js.contains(
            "var $$exports = { get bar() { return foo(); }, set bar($$value) { foo($$value); $.flush(); } };"
        ),
        "the accessor pair keys the SOURCE key over the LOCAL binding:\n{js}"
    );
    // NEGATIVE: the explicit entry's lookup name (`foo`) never emits as a key,
    // and the explicit metadata is emitted exactly once.
    assert!(
        !js.contains("foo: {"),
        "the explicit entry surfaces under the source key, never the local name:\n{js}"
    );
    assert_eq!(
        js.matches("bar: { reflect: true }").count(),
        1,
        "the explicit metadata emits exactly once:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn custom_element_multi_entry_prop_object_preserves_source_order_alias_and_duplicates() {
    // The prop-definition object is emitted in DESCRIPTOR SOURCE ORDER (never
    // sorted, never deduplicated) with the alias + duplicate-key semantics
    // intact — oracle-verified against pinned `svelte@5.56.10`, which emits
    // exactly: `{ zb: { reflect: true }, aa: { type: 'Number' },
    // bar: { attribute: 'f' }, bar: {}, mm: {} }` for this fixture:
    //   - `zb` (no matching local) emits under its own name, FIRST (source
    //     order — `zb` before `aa` proves no sorting);
    //   - `aa` matches the local `aa` (source key `aa`);
    //   - `foo` matches the ALIASED local (`let { bar: foo }`) and emits under
    //     the SOURCE key `bar`;
    //   - the inferred remainder appends in MEMBER order: `bar: {}` (the
    //     aliased member's emitted key misses the RAW explicit-entry names —
    //     the official duplicate-key parity) then `mm: {}`.
    let js = emit_result(
        "<svelte:options customElement={{ tag: 'x-order', props: { zb: { reflect: true }, aa: { type: 'Number' }, foo: { attribute: 'f' } } }} />\n<script>\n\tlet { aa, bar: foo, mm } = $props();\n</script>\n\n<p>{aa}{foo}{mm}</p>\n",
    )
    .expect("a customElement with a multi-entry props descriptor compiles");
    assert!(
        js.contains(
            "customElements.define('x-order', $.create_custom_element(App, { zb: { reflect: true }, aa: { type: 'Number' }, bar: { attribute: 'f' }, bar: {}, mm: {} }, [], [], { mode: 'open' }));"
        ),
        "the prop object must keep descriptor source order + alias + duplicate keys:\n{js}"
    );
    // NEGATIVE: no sorted spelling (`aa` before `zb`) and no deduplicated
    // single-`bar` spelling may appear.
    assert!(
        !js.contains("{ aa: { type: 'Number' }, zb:"),
        "the entries must not be sorted:\n{js}"
    );
    assert_eq!(
        js.matches("bar: {").count(),
        2,
        "the aliased explicit entry + the inferred remainder both emit (no dedupe):\n{js}"
    );
    // NEGATIVE: the explicit entry's LOCAL lookup name never leaks as a key.
    assert!(
        !js.contains("foo: {"),
        "the aliased entry emits under the source key, never the local name:\n{js}"
    );
}

#[test]
fn render_dynamic_callee_host_member_with_snippet_args_frames() {
    // Snippet-call ARITY is not validated for a dynamic callee (official
    // compiles any arg count): `{@render $host().snip(1, 2)}` keeps the frame
    // AND thunks both arguments (`$.snippet(node, callee, () => 1, () => 2)`).
    let js =
        emit_result("<svelte:options customElement=\"x-ra\" />\n{@render $host().snip(1, 2)}\n")
            .expect("a render-dynamic-callee $host() member with args compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the $host() member callee binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the call-result-rooted callee opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$.snippet(node, () => $$props.$$host.snip, () => 1, () => 2);"),
        "both snippet args ride as thunks on the $.snippet call:\n{js}"
    );
    assert!(
        !js.contains("$host("),
        "no raw $host() rune survives the rewrite:\n{js}"
    );
}

#[test]
fn render_dynamic_callee_paren_wrapped_host_member_frames() {
    // The PARENTHESIZED-callee spelling `{@render ($host().snip)()}`: author
    // parens are transparent to the peel (official's estree AST has no paren
    // nodes), so the member-rooted callee still opens the frame and binds
    // `$$props`.
    let js = emit_result("<svelte:options customElement=\"x-rp\" />\n{@render ($host().snip)()}\n")
        .expect("a paren-wrapped render-dynamic-callee $host() member compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the paren-wrapped $host() member callee binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the paren-wrapped callee still opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$$props.$$host.snip"),
        "the callee rewrites through $$props.$$host:\n{js}"
    );
    assert!(
        !js.contains("$host("),
        "no raw $host() rune survives the rewrite:\n{js}"
    );
}

#[test]
fn render_dynamic_callee_children_optional_stays_frame_free() {
    // NEGATIVE (non-CE, so no `$$exports`/ce-accessor confound is possible):
    // `{@render children?.()}` — the peeled callee is the bare identifier
    // `children`; an identifier read is NEVER a `needs_context` trigger even
    // though `children` is a prop (the OUTER snippet call is excluded from the
    // unsafe-call check). Official `svelte@5.56.10` emits NO frame; `$$props`
    // is bound by the REAL props binder alone.
    let js = emit_result("<script>let { children } = $props();</script>\n{@render children?.()}\n")
        .expect("an optional prop render callee compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the real props binder binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.snippet(node, () => $$props.children ?? $.noop);"),
        "the optional prop callee rides the $.snippet + noop route:\n{js}"
    );
    assert!(
        !js.contains("$.push("),
        "a prop-identifier render callee must NOT open the context frame:\n{js}"
    );
    assert!(!js.contains("$.pop"), "no context frame pop:\n{js}");
}

#[test]
fn render_dynamic_callee_ternary_stays_frame_free() {
    // NEGATIVE: `{@render (cond ? a : b)()}` — the peeled callee is a ternary
    // of bare identifiers (props + a local signal); identifier reads never
    // trigger `needs_context`. Official emits NO frame.
    let js = emit_result(
        "<script>let { a, b } = $props(); let cond = $state(true);</script>\n{@render (cond ? a : b)()}\n",
    )
    .expect("a ternary render callee compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the real props binder binds $$props:\n{js}"
    );
    assert!(
        !js.contains("$.push("),
        "a ternary-of-identifiers render callee must NOT open the frame:\n{js}"
    );
    assert!(!js.contains("$.pop"), "no context frame pop:\n{js}");
    assert!(
        js.contains("$.snippet("),
        "the dynamic render rides $.snippet:\n{js}"
    );
}

#[test]
fn render_dynamic_callee_safe_local_member_stays_frame_free() {
    // NEGATIVE: `{@render o.snip()}` where `o` is a LOCAL (a snippet param) —
    // a member rooted at a local/global-safe binding is a safe identifier, so
    // no frame opens; with no props binder either, the component keeps the
    // bare `($$anchor)` signature. (The outer arg `globalThis.snips` roots at
    // a GLOBAL — safe too; the unused `$state` only keeps the component in
    // runes mode, the same isolation `render_paren_callee` uses.)
    let js = emit_result(
        "<script>let __r = $state(0);</script>\n{#snippet wrap(o)}{@render o.snip()}{/snippet}\n{@render wrap(globalThis.snips)}\n",
    )
    .expect("a safe-local-member render callee compiles");
    assert!(
        js.contains("($$anchor) {"),
        "no props binder and no context reason — the bare signature stays:\n{js}"
    );
    assert!(
        !js.contains("$.push("),
        "a local-rooted member render callee must NOT open the frame:\n{js}"
    );
    assert!(!js.contains("$.pop"), "no context frame pop:\n{js}");
    assert!(
        js.contains("() => o().snip"),
        "the local-member callee rides its $.snippet thunk (the snippet param reads through its getter):\n{js}"
    );
}

#[test]
fn render_arg_arrow_param_shadowing_import_stays_frame_free() {
    // SHADOW rail (render ARGUMENTS scan normally, scope-aware): an arrow
    // param shadowing the import makes the arg's member SAFE — official emits
    // NO frame. The unshadowed twin below proves the same member DOES frame,
    // so this pair discriminates the shadow handling, not the member rule.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let __r = $state(0);</script>\n{#snippet s(cb)}<p>hi</p>{/snippet}\n{@render s((Child) => Child.x)}\n<button onclick={() => __r++}>{__r}</button>\n",
    )
    .expect("a shadowed-import render arg compiles");
    assert!(
        !js.contains("$.push("),
        "a shadowed-import render arg must NOT open the frame:\n{js}"
    );
    assert!(!js.contains("$.pop"), "no context frame pop:\n{js}");
}

#[test]
fn render_arg_unshadowed_import_member_frames() {
    // The unshadowed twin: the arg arrow body reads `Child.x` with NO
    // shadowing param — an import-rooted member — so `needs_context` fires
    // through the ARGUMENT scan (args are never exempted). Official binds
    // `$$props` and opens the frame.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let __r = $state(0);</script>\n{#snippet s(cb)}<p>hi</p>{/snippet}\n{@render s(() => Child.x)}\n<button onclick={() => __r++}>{__r}</button>\n",
    )
    .expect("an unshadowed-import render arg compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the import-rooted render arg binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the import-rooted render arg opens the context frame:\n{js}"
    );
}

#[test]
fn custom_element_rest_props_exclude_the_host_key() {
    // A `$props()` REST capture inside a custom element: the official
    // `rest_excludes` prefix appends `'$$host'` after `'$$legacy'` (the host
    // element rides `$$props.$$host`, and a rest spread must never surface it) —
    // BEFORE the member source keys.
    let js = emit_result(
        "<svelte:options customElement=\"x-rest\" />\n<script>let { a, ...rest } = $props();</script>\n<p>{a}</p>\n",
    )
    .expect("a customElement with a $props() rest capture compiles");
    assert!(
        js.contains(
            "var rest_excludes = new Set(['$$slots', '$$events', '$$legacy', '$$host', 'a']);"
        ),
        "the CE rest excludes carry '$$host' in the official position:\n{js}"
    );
    // The rest of the CE surface stays intact around the rest capture: the `a`
    // member is an accessor-forced prop source and the inferred `a: {{}}` rides
    // the create call.
    assert!(
        js.contains("customElements.define('x-rest', $.create_custom_element(App, { a: {} }, [], [], { mode: 'open' }));"),
        "the inferred `a: {{}}` prop definition rides the create call:\n{js}"
    );
    assert!(
        js.contains("return $.pop($$exports);"),
        "the accessor frame closes through $$exports:\n{js}"
    );
}

#[test]
fn custom_element_null_value_is_a_plain_component_no_op() {
    // `customElement={null}` (the Svelte-3 backwards-compat spelling) sets
    // NOTHING: with no compile option the component compiles PLAIN — no create,
    // no define, no accessor frame.
    let js = emit_result(
        "<svelte:options customElement={null} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a customElement={null} component compiles as a plain component");
    assert!(
        !js.contains("create_custom_element"),
        "null customElement creates nothing:\n{js}"
    );
    assert!(
        !js.contains("customElements.define"),
        "null customElement defines nothing:\n{js}"
    );
    assert!(!js.contains("$$exports"), "no accessor frame:\n{js}");
    assert!(
        js.contains("export default function App($$anchor) {"),
        "the plain component shape survives:\n{js}"
    );
}

#[test]
fn bare_props_in_call_arg_fails_closed() {
    // A bare `$props()` as a CALL ARGUMENT (`console.log($props())`) is not the
    // single supported top-level `$props()` destructure position — fail closed,
    // never emit raw `$props()`. RED against the pre-fix scan.
    assert_fail_closed(
        "<script>console.log($props())</script>\n<p>hi</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props"),
    );
}
// ── Module scripts (`<script module>`) — canonical statement lowering ─────

#[test]
fn module_script_import_only_is_admitted_to_the_module_prelude_slot() {
    // An IMPORT-ONLY `<script module>` is ADMITTED: its imports hoist to the MODULE
    // slot of the prelude — BEFORE `import * as $` (the official two-slot order) —
    // while an instance import stays AFTER it. The instance `$state` keeps the
    // component runes-mode (an import-only component is legacy mode).
    let js = emit(
        "<script module>import { m } from './base.js';</script>\n<script>import { i } from './inst.js'; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    let module_at = js
        .find("import { m } from './base.js';")
        .unwrap_or_else(|| panic!("missing the module-slot import:\n{js}"));
    let ns_at = js
        .find("import * as $ from 'svelte/internal/client';")
        .unwrap_or_else(|| panic!("missing the runtime namespace:\n{js}"));
    let instance_at = js
        .find("import { i } from './inst.js';")
        .unwrap_or_else(|| panic!("missing the instance-slot import:\n{js}"));
    assert!(
        module_at < ns_at && ns_at < instance_at,
        "the two-slot order is module imports → `import * as $` → instance imports:\n{js}"
    );
    // NEGATIVE: no module-script refusal diagnostic — the component compiled.
    assert!(
        js.contains("export default function"),
        "the module script must compile to a Main:\n{js}"
    );
}

// ── Scan ALL `$props()` declarators (one supported shape; reject the rest) ─────

#[test]
fn second_props_declarator_with_computed_key_fails_closed() {
    // `let {a}=$props(), {[k]:b}=$props();` — the first basic destructure must NOT
    // admit the file while the second (a COMPUTED key) slips through and emits a
    // raw prop read. ALL `$props()` declarators are scanned; the computed-key one
    // fails closed. RED against the pre-fix `props_shape`, which returned after
    // the FIRST declarator.
    assert_fail_closed(
        "<script>let k='x'; let {a}=$props(), {[k]:b}=$props();</script>\n<p>{b}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() computed key"),
    );
}

#[test]
fn second_props_call_whole_object_fails_closed() {
    // Two SEPARATE `$props()` statements (a basic destructure + a now-supported
    // whole-object binding) — TWO `$props()` calls fail closed as `$props()
    // duplicate` (the surviving duplicate arm), regardless of each call's individual
    // (now-basic) shape. RED against scanning only the first.
    assert_fail_closed(
        "<script>let {a}=$props(); let p=$props();</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() duplicate"),
    );
}

#[test]
fn boolean_props_module_matches_the_committed_jsdom_smoke_fixture() {
    // The boolean-DOM-property behavioral fixture (`readonly={off}` → `input.readOnly =
    // $.get(off)`, toggled by a SEPARATE button so the disabled/readonly state never
    // blocks the toggle click) stays equivalent to `compile_client`'s output.
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet off = $state(false);\n</script>\n\n<input readonly={off} />\n<button onclick={() => off = !off}>toggle</button>\n",
        "boolean_props.client.mjs",
    );
}

#[test]
fn props_computed_key_fails_closed_not_partial() {
    // R7b: a computed-key `$props()` destructure (`{ [k]: a }`) is rejected by
    // official (`props_invalid_pattern`); Verter fails closed rather than reading
    // the wrong key. RED against the prior classifier (which accepted it as
    // basic).
    assert_fail_closed(
        "<script>const k = 'x'; let { [k]: a } = $props();</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }),
    );
}

#[test]
fn props_nested_destructure_fails_closed_not_partial() {
    // R7b: a nested `$props()` destructure (`{ a: { b } }`) is rejected by official
    // (`props_invalid_pattern`); Verter fails closed.
    assert_fail_closed(
        "<script>let { a: { b } } = $props();</script>\n<p>{b}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }),
    );
}

#[test]
fn props_string_literal_key_reads_via_bracket_access() {
    // R7a: a no-default string-literal-key prop reads via BRACKET access, not the
    // invalid `$$props.foo-bar`. Verified against svelte@5.56.10
    // (`$$props['foo-bar']`).
    let js = emit(
        "<script>let { \"foo-bar\": bar } = $props();</script>\n<p>{bar}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$$props['foo-bar']") || js.contains("$$props[\"foo-bar\"]"),
        "a string-literal key prop must read via bracket access:\n{js}"
    );
    // NEGATIVE: the invalid dotted form must NOT appear.
    assert!(
        !js.contains("$$props.foo-bar"),
        "the invalid `$$props.foo-bar` dotted access must be gone:\n{js}"
    );
}

#[test]
fn boolean_dom_property_disabled_emits_direct_property_write() {
    // `disabled={v}` → `button.disabled = $.get(v)` (is_dom_property), NOT
    // `$.set_attribute(..., true)`.
    let src = "<script>let v = $state(false);</script>\n<button onclick={() => v = !v} disabled={v}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.template_effect(() => button.disabled = $.get(v))")),
        "a boolean DOM property must be a direct property write:\n{js}"
    );
    // NEGATIVE: the forbidden boolean set_attribute signature is ABSENT.
    assert!(
        !n.contains(&nc("$.set_attribute(button, 'disabled'")),
        "a DOM-boolean property must NOT use set_attribute:\n{js}"
    );
}

#[test]
fn boolean_dom_property_readonly_aliases_to_readonly_property() {
    // `readonly={v}` → `input.readOnly = $.get(v)` (normalize_attribute alias).
    let src =
        "<script>let v = $state(false);</script>\n<input onclick={() => v = !v} readonly={v}>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.template_effect(() => input.readOnly = $.get(v))")),
        "`readonly` must alias to the `readOnly` property write:\n{js}"
    );
    assert!(
        !n.contains("input.readonly=") && !n.contains(&nc("$.set_attribute(input, 'readonly'")),
        "must use the camelCase `readOnly` property, not the attribute / lowercase:\n{js}"
    );
}

#[test]
fn contenteditable_dynamic_uses_set_attribute_not_property() {
    // `contenteditable={v}` is NOT a DOM property → `$.set_attribute(div, 'contenteditable', …)`.
    let src = "<script>let v = $state('true');</script>\n<div onclick={() => v = 'false'} contenteditable={v}></div>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.template_effect(() => $.set_attribute(div, 'contenteditable', $.get(v)))"
        )),
        "`contenteditable` must use set_attribute:\n{js}"
    );
    assert!(
        !n.contains("div.contenteditable="),
        "`contenteditable` is NOT a DOM property:\n{js}"
    );
}

#[test]
fn hidden_dynamic_uses_set_attribute_not_property() {
    // `hidden={v}` is NOT in DOM_PROPERTIES → `$.set_attribute(button, 'hidden', …)`.
    let src = "<script>let v = $state(false);</script>\n<button onclick={() => v = !v} hidden={v}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.template_effect(() => $.set_attribute(button, 'hidden', $.get(v)))"
        )),
        "`hidden` must use set_attribute:\n{js}"
    );
    assert!(
        !n.contains("button.hidden="),
        "`hidden` is NOT a DOM property:\n{js}"
    );
}

#[test]
fn style_custom_property_quotes_the_key() {
    // `style:--x={x}` (no base) → `$.set_style(button, '', styles, { '--x': $.get(x) })`.
    let src = "<script>let x = $state('1');</script>\n<button onclick={() => x += '1'} style:--x={x}></button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "styles = $.set_style(button, '', styles, { '--x': $.get(x) })"
        )),
        "a custom property must quote the `--x` key and use an empty base:\n{js}"
    );
}

#[test]
fn fragment_named_slot_text_first_body_has_no_leading_next() {
    // A TEXT-FIRST `<svelte:fragment slot="foo">` named-slot body: official emits the
    // callback WITHOUT the leading `$.next()` cursor advance (`var text = $.text();`
    // directly) — the named-slot region is NOT an each/children-style render callback.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><svelte:fragment slot=\"foo\">hello {x}</svelte:fragment></Child>\n",
    )
    .expect("a text-first fragment named slot emits a module");
    assert!(
        js.contains("$$slots: {foo: ($$anchor, $$slotProps) =>"),
        "missing the bare-identifier named-slot callback entry:\n{js}"
    );
    // The DISCRIMINATOR: no `$.next()` prelude in the named region body.
    assert!(
        !js.contains("$.next()"),
        "a text-first named-slot region must not emit the $.next() prelude:\n{js}"
    );
}

#[test]
fn snippet_and_default_children_text_first_bodies_keep_next_prelude() {
    // NEGATIVE CONTROLS for the named-slot `$.next()` removal: a `{#snippet}`-as-child
    // named slot AND a text-first default-children region BOTH keep the official
    // `$.next()` cursor advance — the removal is scoped to `slots.named` ONLY.
    let snippet_js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child>{#snippet foo()}hello {x}{/snippet}</Child>\n",
    )
    .expect("a snippet named slot emits a module");
    assert!(
        snippet_js.contains("$.next()"),
        "a text-first {{#snippet}} body must keep the $.next() prelude:\n{snippet_js}"
    );
    let children_js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child>hello {x}</Child>\n",
    )
    .expect("text-first default children emit a module");
    assert!(
        children_js.contains("children: ($$anchor, $$slotProps) =>"),
        "missing the default-children callback:\n{children_js}"
    );
    assert!(
        children_js.contains("$.next()"),
        "a text-first default-children body must keep the $.next() prelude:\n{children_js}"
    );
}

#[test]
fn slot_attr_outside_component_child_placement_still_fails_closed() {
    // A `slot="a"` on an element that is NOT a direct component child is the official
    // `slot_attribute_invalid_placement` compile error — Verter keeps it fail-closed
    // (the slot attribute is accepted ONLY at valid component-child slot placement).
    // Top-level:
    assert_fail_closed(
        "<script>let x = $state(0);</script>\n<div slot=\"a\">{x}</div>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"),
    );
    // Nested INSIDE a component child (still invalid placement — official errors):
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let x = $state(0);</script>\n<Child><div><span slot=\"a\">{x}</span></div></Child>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"),
    );
}

#[test]
fn dynamic_or_mixed_slot_attr_fails_closed() {
    // A DYNAMIC `slot={x}` (and the mixed `slot="a{x}"`) is the official
    // `slot_attribute_invalid` compile error ("slot attribute must be a static value").
    // It must fail CLOSED — never be accepted as a generic `$.set_attribute` and never
    // mis-place the element into the default children region.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let x = $state('a');</script>\n<Child><span slot={x}>hi</span></Child>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"),
    );
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let x = $state('a');</script>\n<Child><span slot=\"a{x}\">hi</span></Child>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "slot"),
    );
}

#[test]
fn duplicate_named_slot_fails_closed() {
    // Two component children carrying the SAME `slot` name is the official
    // `slot_attribute_duplicate` compile error — Verter fails closed rather than
    // silently MERGING the two groups into one region (an output official refuses).
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"a\">{x}</span><p slot=\"a\">2</p></Child>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "duplicate slot name",
                    ..
                }
            )
        },
    );
    // The `<svelte:fragment>` form duplicates identically (official errors the same).
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><svelte:fragment slot=\"a\">{x}</svelte:fragment><svelte:fragment slot=\"a\">2</svelte:fragment></Child>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "duplicate slot name",
                    ..
                }
            )
        },
    );
}

#[test]
fn entity_encoded_static_slot_name_decodes_to_the_official_key() {
    // Official decodes attribute values at parse (`decode_character_references`), so a
    // `slot="foo&amp;bar"` child names the slot `foo&bar` — the slot name is a DECODED
    // semantic key, never the raw escape bytes — and the emitted `$$slots` entry key is
    // the quoted `'foo&bar'` (matching svelte@5.56.10; pinned corpus-wide by the
    // `components/named_slot_entity` oracle golden).
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"foo&amp;bar\">{x}</span></Child>\n",
    )
    .expect("an entity-encoded named slot emits a module");
    assert!(
        js.contains("$$slots: {'foo&bar': ($$anchor, $$slotProps) =>"),
        "the $$slots key must be the DECODED slot name:\n{js}"
    );
    // NEGATIVE: the raw entity bytes are never the key.
    assert!(
        !js.contains("'foo&amp;bar':"),
        "the raw entity bytes must not leak into the $$slots key:\n{js}"
    );
    // The slot ATTRIBUTE still bakes into the skeleton in its re-escaped HTML form
    // (decode + re-escape round-trips `&amp;` — the skeleton is HTML, the key is JS).
    assert!(
        js.contains("slot=\"foo&amp;bar\""),
        "the baked skeleton keeps the re-escaped HTML attribute form:\n{js}"
    );
}

#[test]
fn plain_static_slot_name_is_not_over_decoded() {
    // NEGATIVE guard for the decode step: a plain `slot="foo"` (no entity) still emits
    // the bare identifier key `foo:` — the decoder must be a no-op on entity-free names.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"foo\">{x}</span></Child>\n",
    )
    .expect("a plain named slot emits a module");
    assert!(
        js.contains("$$slots: {foo: ($$anchor, $$slotProps) =>"),
        "a plain slot name stays the bare identifier key:\n{js}"
    );
    assert!(
        !js.contains("'foo':"),
        "an entity-free identifier slot name must not be quoted:\n{js}"
    );
}

#[test]
fn entity_encoded_static_component_prop_value_decodes() {
    // The static-prop entity decode is GENERAL, not slot-specific: official decodes
    // attribute values at parse (`decode_character_references`), so ANY static
    // component prop carrying an entity emits its DECODED value. Verified against
    // svelte@5.56.10: `<Child label="foo&amp;bar" />` → `Child($$anchor, { label:
    // 'foo&bar' })`. This pins the non-slot path of the same decoder the `$$slots`
    // key / retained `slot` prop use.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let c = $state(0);</script>\n<Child label=\"foo&amp;bar\" />\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a component with an entity-encoded static prop emits a module");
    assert!(
        js.contains("label: 'foo&bar'"),
        "a static component prop value must be entity-decoded:\n{js}"
    );
    // NEGATIVE: the raw entity bytes must not survive into the prop value.
    assert!(
        !js.contains("foo&amp;bar"),
        "the raw entity must not leak un-decoded into the emitted module:\n{js}"
    );
}

#[test]
fn entity_and_literal_slot_names_denote_the_same_slot() {
    // `slot="a&amp;b"` and `slot="a&b"` DECODE to the same semantic name (`a&b` — the
    // legacy no-`;` `&b` is not a named reference, so it stays literal), so the pair is
    // the official `slot_attribute_duplicate` compile error: the duplicate gate compares
    // DECODED names. Pre-decode, the raw spans differ and the pair would silently
    // become TWO distinct slot regions — an output official refuses.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"a&amp;b\">{x}</span><p slot=\"a&b\">2</p></Child>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "duplicate slot name",
                    ..
                }
            )
        },
    );
}

#[test]
fn explicit_default_slot_with_implicit_content_fails_closed() {
    // An explicit `slot="default"` child ALONGSIDE implicit default content is the
    // official `slot_default_duplicate` compile error — fail closed, never merge.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child>{x}<span slot=\"default\">1</span></Child>\n",
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
fn slot_default_named_child_routes_to_children_prop() {
    // A `slot="default"` child is the DEFAULT slot in official output: it emits the
    // `children:` callback + the `$$slots: { default: true }` marker — NOT a
    // `$$slots: { default: (…) => {…} }` callback entry (the named-slot form).
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"default\">{x}</span></Child>\n",
    )
    .expect("a slot=\"default\" child emits a module");
    assert!(
        js.contains("children: ($$anchor, $$slotProps) =>"),
        "slot=\"default\" content must become the children prop:\n{js}"
    );
    assert!(
        js.contains("$$slots: {default: true}"),
        "the default marker must be the `default: true` form:\n{js}"
    );
    assert!(
        js.contains("$.from_html(`<span slot=\"default\"> </span>`)"),
        "the slot=\"default\" attribute must bake into the skeleton:\n{js}"
    );
    // NEGATIVE: never the named-callback form for `default`.
    assert!(
        !js.contains("default: ($$anchor"),
        "slot=\"default\" must not emit a named-slot callback entry:\n{js}"
    );
}

#[test]
fn entity_slot_name_decodes_retained_prop_consistently_with_slots_key() {
    // Class-A filler: `<Child><Inner slot="foo&amp;bar"/></Child>` — official
    // (svelte@5.56.10) entity-decodes the slot name ONCE at parse and emits BOTH the
    // `$$slots` grouping key AND the retained `slot:` prop as the decoded `foo&bar`:
    //   Child($$anchor, { $$slots: { 'foo&bar': ($$anchor, $$slotProps) => {
    //     Inner($$anchor, { slot: 'foo&bar' }); } } });
    // The key and the prop must ride the SAME decode — a raw `foo&amp;bar` prop
    // alongside a decoded `foo&bar` key is an observably-wrong split.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Child><Inner slot=\"foo&amp;bar\"/></Child>\n",
    )
    .expect("an entity-named slot filler emits a module");
    assert!(
        js.contains("$$slots: {'foo&bar': ($$anchor, $$slotProps) =>"),
        "the $$slots grouping key must be the decoded quoted form:\n{js}"
    );
    assert!(
        js.contains("Inner($$anchor, {slot: 'foo&bar'})"),
        "the retained slot prop must carry the SAME decoded value as the key:\n{js}"
    );
    // NEGATIVE: the raw entity spelling must appear NOWHERE in the module.
    assert!(
        !js.contains("foo&amp;bar"),
        "the un-decoded entity spelling must not survive into the output:\n{js}"
    );

    // Class-B plain prop (non-consumed): `<Child><div><Inner slot="foo&amp;bar"/></div></Child>`
    // — official emits the plain prop as the SAME decoded form: `Inner(node, { slot: 'foo&bar' })`.
    let nested = emit_result(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Child><div><Inner slot=\"foo&amp;bar\"/></div></Child>\n",
    )
    .expect("an entity-named plain slot prop emits a module");
    assert!(
        nested.contains("{slot: 'foo&bar'}"),
        "the non-consumed plain slot prop must be entity-decoded:\n{nested}"
    );
    assert!(
        !nested.contains("foo&amp;bar"),
        "the un-decoded entity spelling must not survive into the output:\n{nested}"
    );
    // NEGATIVE: the nested prop carrier never mints a named $$slots entry.
    assert!(
        !nested.contains("'foo&bar': ($$anchor"),
        "a plain slot prop must not mint a named $$slots callback:\n{nested}"
    );

    // A second entity family (`&lt;` → `<`) proves the decode is the general shared
    // decoder, not an `&amp;`-only special case: official emits key `'a<b'` AND prop
    // `slot: 'a<b'` for `<Child><Inner slot="a&lt;b"/></Child>`.
    let lt = emit_result(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Child><Inner slot=\"a&lt;b\"/></Child>\n",
    )
    .expect("an `&lt;`-named slot filler emits a module");
    assert!(
        lt.contains("$$slots: {'a<b': ($$anchor, $$slotProps) =>"),
        "the $$slots key must decode the named `&lt;` reference:\n{lt}"
    );
    assert!(
        lt.contains("Inner($$anchor, {slot: 'a<b'})"),
        "the retained slot prop must decode the named `&lt;` reference:\n{lt}"
    );
    assert!(
        !lt.contains("a&lt;b"),
        "the un-decoded `&lt;` spelling must not survive into the output:\n{lt}"
    );
}

#[test]
fn svelte_head_inside_component_child_rejects_as_meta_placement() {
    // A `<svelte:head>` nested inside a component child (with or without `slot=`) is
    // the official `svelte_meta_invalid_placement` compile error — Verter refuses it
    // on the OFFICIAL-reject rail with the exact code BEFORE the slot choke-point
    // runs, so a `slot="x"` on it can never reach filler routing.
    let err = emit_result(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><svelte:head slot=\"x\"><title>t</title></svelte:head></Child>\n",
    )
    .expect_err("a component-nested svelte:head must reject");
    assert!(
        matches!(
            &err,
            ClientCompileError::OfficialReject(rej)
                if rej.official_code == "svelte_meta_invalid_placement"
        ),
        "expected the official svelte_meta_invalid_placement reject: {err:?}"
    );
}

#[test]
fn svelte_head_slot_snippet_child_stays_official_meta_placement_reject() {
    // `<svelte:head slot="x">` inside a `{#snippet}` — the PARSE-phase official reject
    // (`svelte_meta_invalid_placement`: `<svelte:head>` must be at the top level)
    // fires before the slot gate ever runs, and must stay.
    let err = emit_result(
        "<script>let { x } = $props();</script>\n{#snippet foo()}<svelte:head slot=\"x\"><title>t</title></svelte:head>{/snippet}\n{@render foo()}\n",
    )
    .expect_err("a snippet-nested <svelte:head> must refuse");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected an OfficialReject refusal, got {err:?}");
    };
    assert_eq!(
        rejection.official_code, "svelte_meta_invalid_placement",
        "the rejection mirrors the official `svelte_meta_invalid_placement` code"
    );
}

#[test]
fn custom_element_static_slot_snippet_child_fails_closed_at_host_gate() {
    // Official ACCEPTS `<my-el slot="x">` as a snippet child (native slotting via the
    // `importNode` clone + `$.set_custom_element_data`); Verter's slot gate accepts
    // the PLACEMENT and the D-43 custom-element HOST gate then fails closed — the
    // reject identity is `host-custom-element`, NOT the slot gate's DynamicAttribute.
    let src = "<script>let { x } = $props();</script>\n{#snippet foo()}<my-el slot=\"x\">hi</my-el>{/snippet}\n{@render foo()}\n";
    match emit_result(src) {
        Err(ClientCompileError::Unsupported(refusal)) => {
            assert!(
                matches!(
                    &refusal,
                    UnsupportedSvelteRuntimeSurface::HostOrCustomElement { surface, .. }
                        if *surface == "custom element"
                ),
                "wrong fail-closed surface: {refusal:?}"
            );
            assert_eq!(
                refusal.diagnostic_code(),
                "svelte-runtime-unsupported-host-custom-element",
                "the custom-element snippet child must reject on the D-43 host gate"
            );
        }
        Ok(js) => panic!("a custom-element snippet child must fail closed, got a module:\n{js}"),
        Err(other) => panic!("expected the typed unsupported surface, got: {other:?}"),
    }
}

#[test]
fn render_arg_simple_prop_read_stays_inline_thunk() {
    // NEGATIVE CONTROL: a SIMPLE identifier render arg (`row(xs)`) has no call and no
    // await — official passes the plain `() => $$props.xs` thunk with NO memoization
    // (the Memoizer's `memoize_if_state` is false for render args, so has_state alone
    // never memoizes).
    let js = emit_result(
        "<script>let { xs } = $props();</script>\n{#snippet row(items)}<p>{items}</p>{/snippet}\n{@render row(xs)}\n",
    )
    .expect("a simple render arg emits a module");
    assert!(
        js.contains("row($$anchor, () => $$props.xs);"),
        "a simple render arg stays the inline thunk:\n{js}"
    );
    assert!(
        !js.contains("$.derived"),
        "a simple render arg must not be memoized:\n{js}"
    );
}

// ─── `{@render}` static-callee paren peel + static-optional direct call ───

#[test]
fn render_paren_wrapped_local_snippet_callee_emits_direct_call() {
    // `{@render (row)(1)}` — the transparent author parens around the callee peel to the
    // bare identifier, which resolves to the LOCAL `{#snippet row}` — official emits the
    // DIRECT static call `row($$anchor, () => 1);`, NOT the dynamic `$.snippet` route.
    let js = emit_result(
        "<script>let __r = $state(0);</script>\n{#snippet row(a)}<p>{a}</p>{/snippet}\n{@render (row)(1)}\n",
    )
    .expect("a paren-wrapped local-snippet callee emits a module");
    assert!(
        js.contains("row($$anchor, () => 1);"),
        "a peeled static callee must emit the direct call:\n{js}"
    );
    // NEGATIVE: no `$.snippet` dynamic route for a resolved static callee.
    assert!(
        !js.contains("$.snippet("),
        "a resolved static callee must not route through $.snippet:\n{js}"
    );
}

#[test]
fn render_optional_local_snippet_call_emits_direct_optional_call() {
    // `{@render row?.(1)}` on a LOCAL `{#snippet row}` — official emits the DIRECT
    // optional call `row?.($$anchor, () => 1);` (the `b.maybe_call` form), NOT the
    // dynamic `$.snippet(node, () => row ?? $.noop, …)` route.
    let js = emit_result(
        "<script>let __r = $state(0);</script>\n{#snippet row(a)}<p>{a}</p>{/snippet}\n{@render row?.(1)}\n",
    )
    .expect("an optional local-snippet call emits a module");
    assert!(
        js.contains("row?.($$anchor, () => 1);"),
        "a static-optional callee must emit the direct optional call:\n{js}"
    );
    // NEGATIVE: neither the `$.snippet` route nor the `?? $.noop` dynamic fallback.
    assert!(
        !js.contains("$.snippet("),
        "a resolved static-optional callee must not route through $.snippet:\n{js}"
    );
    assert!(
        !js.contains("?? $.noop"),
        "a resolved static-optional callee must not carry the noop fallback:\n{js}"
    );
}

// ─── spread PRESENCE ⇒ `has_state` on component / boundary props (getter form) ───

#[test]
fn component_array_spread_prop_emits_memoized_getter() {
    // `items={[...globalThis.things]}` — spread presence is has_state AND has_call
    // (official `SpreadElement.js`), so the prop memoizes into the wrapping-block
    // derived AND surfaces as the reactive GETTER accessor — official emits
    // `let $0 = $.derived(() => [...globalThis.things]); … get items() { return $.get($0); }`.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let __r = $state(0);</script>\n<Child items={[...globalThis.things]} />\n",
    )
    .expect("an array-spread component prop emits a module");
    assert!(
        js.contains("let $0 = $.derived(() => ([...globalThis.things]));"),
        "missing the memoized spread-prop derived:\n{js}"
    );
    assert!(
        js.contains("get items() {return $.get($0);}"),
        "a spread prop must surface as the reactive getter accessor:\n{js}"
    );
    // NEGATIVE: never the one-shot init member (`items: $.get($0)`).
    assert!(
        !js.contains("items: $.get($0)"),
        "a spread prop must not emit the one-shot init form:\n{js}"
    );
}

#[test]
fn component_object_spread_prop_emits_valid_memoized_getter() {
    // The OBJECT-spread prop (`items={{ ...globalThis.things }}`): the memoized derived
    // must embed the object literal as a CONCISE ARROW BODY (`() => ({ … })`) — the
    // bare `() => { ...x }` would parse as a broken block body — and the prop surfaces
    // as the getter.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let __r = $state(0);</script>\n<Child items={{ ...globalThis.things }} />\n",
    )
    .expect("an object-spread component prop emits a module");
    assert!(
        js.contains("let $0 = $.derived(() => ({ ...globalThis.things }));"),
        "the object-spread derived must paren-wrap its arrow body:\n{js}"
    );
    assert!(
        js.contains("get items() {return $.get($0);}"),
        "an object-spread prop must surface as the reactive getter:\n{js}"
    );
    // NEGATIVE: the broken block-body arrow must be absent.
    assert!(
        !js.contains("$.derived(() => { ...globalThis.things })"),
        "the derived arrow body must not be a bare object literal (broken JS):\n{js}"
    );
}

#[test]
fn component_arrow_prop_stays_plain_init() {
    // NEGATIVE CONTROL: an ARROW-function prop value (`cb={_=>{}}`) has no spread in
    // EVALUATED position (the body is deferred) — it stays the plain one-shot init
    // (`cb: _=>{}`), never a getter and never memoized.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let __r = $state(0);</script>\n<Child cb={_=>{}} />\n",
    )
    .expect("an arrow prop emits a module");
    assert!(
        js.contains("cb: _=>{}"),
        "an arrow prop stays the plain init member:\n{js}"
    );
    assert!(
        !js.contains("get cb()") && !js.contains("$.derived"),
        "an arrow prop must not become a getter nor memoize:\n{js}"
    );
}

#[test]
fn boundary_spread_attr_prop_emits_getter() {
    // `<svelte:boundary failed={[...xs]}>` (xs a prop) — the boundary attr-prop rides
    // the SAME `prop_value_has_state` decision: spread presence ⇒ the getter accessor.
    // Official svelte@5.56.10 emits the identical UNMEMOIZED getter (`get failed() {
    // return [...$$props.xs]; }` — boundary props are never `$.derived`-hoisted),
    // pinned full-module by the committed `special/svelte_boundary_spread` oracle
    // golden (`svelte_client_emit_topology.rs`).
    let js = emit_result(
        "<script>let { xs } = $props();</script>\n<svelte:boundary onerror={() => {}}><p>hi</p></svelte:boundary>\n",
    )
    .expect("a plain boundary emits a module");
    // Control: the boundary itself emits.
    assert!(js.contains("$.boundary("), "boundary call expected:\n{js}");
    let js = emit_result(
        "<script>let { xs } = $props();</script>\n<svelte:boundary onerror={() => {}} failed={[...xs]}><p>hi</p></svelte:boundary>\n",
    )
    .expect("a spread boundary attr prop emits a module");
    assert!(
        js.contains("get failed() { return [...$$props.xs]; }"),
        "a spread boundary attr prop must surface as the getter accessor:\n{js}"
    );
    // NEGATIVE: never the one-shot init form.
    assert!(
        !js.contains("failed: [...$$props.xs]"),
        "a spread boundary attr prop must not emit the one-shot init form:\n{js}"
    );
}

// ── $props.id() — hoisted body-top const + fail-closed siblings ───────────────

#[test]
fn props_id_hoists_const_with_zero_arg_helper() {
    // `let uid = $props.id();` → hoisted body-top `const uid = $.props_id();`
    // (the source `let` still emits `const`), a zero-arg helper. Verified against
    // svelte@5.56.10.
    let src = "<script>let uid = $props.id();</script>\n<p>{uid}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("const uid = $.props_id();"),
        "the id decl is a `const` + zero-arg `$.props_id()`:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {"),
        "an id-only component keeps the `($$anchor)` signature:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, uid)"),
        "the uid read stays a PLAIN name inside the template effect:\n{js}"
    );
    // NEGATIVE: `$props.id()` pulls in NO `$$props` and NO context frame, and the
    // raw rune member never survives.
    assert!(
        !js.contains("$$props"),
        "an id-only component must not thread $$props:\n{js}"
    );
    assert!(
        !js.contains("$.push") && !js.contains("$.pop"),
        "`$props.id()` must not force the component context frame:\n{js}"
    );
    assert!(
        !js.contains("$props.id"),
        "no raw rune member survives:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_id_splits_multi_declarator_and_hoists_above_siblings() {
    // `const a = 1, uid = $props.id();` splits: the uid const hoists to the
    // FUNCTION-BODY TOP (above every other body statement), the literal sibling
    // stays a separate declaration in its source slot with its source keyword.
    // Verified against svelte@5.56.10.
    let src = "<script>let s = $state(0); const a = 1, uid = $props.id();</script>\n<button onclick={() => s = 2}>{uid}{s}</button>\n";
    let js = emit(src, "App.svelte");
    let uid = js.find("const uid = $.props_id();").expect("uid hoist");
    let state = js.find("let s = $.state(0);").expect("state decl");
    let sibling = js.find("const a = 1;").expect("literal sibling decl");
    assert!(
        uid < state && state < sibling,
        "hoist order: uid const above the state decl, sibling in its source slot:\n{js}"
    );
    // NEGATIVE: the multi-declarator must not survive joined.
    assert!(
        !js.contains("const a = 1, uid"),
        "the multi-declarator must split:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_id_coexists_with_props_destructure() {
    // `$props.id()` is independent of the `$props()` destructure: the signature
    // carries `$$props` (from the destructure), `$.props_id()` stays zero-arg, and
    // no context frame opens. Verified against svelte@5.56.10.
    let src = "<script>let { a } = $props(); const uid = $props.id();</script>\n<p>{a}{uid}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the destructure threads $$props:\n{js}"
    );
    assert!(
        js.contains("const uid = $.props_id();"),
        "the id decl stays the zero-arg helper:\n{js}"
    );
    assert!(
        js.contains("$$props.a"),
        "the no-default prop reads off $$props:\n{js}"
    );
    // NEGATIVE: no context frame, and never `$.props_id($$props)`.
    assert!(
        !js.contains("$.push") && !js.contains("$.pop"),
        "no context frame from `$props.id()` + a plain destructure:\n{js}"
    );
    assert!(
        !js.contains("$.props_id($$props)"),
        "`$.props_id` stays zero-arg:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_id_hoists_above_context_frame_and_prop_sources() {
    // With a bindable present the hoist ordering is: `const uid = $.props_id();`
    // FIRST, then `$.push($$props, true);`, then the `$.prop` declarations.
    // Verified against svelte@5.56.10 (the id const hoists above the frame push).
    let src = "<script>let { v = $bindable('hi') } = $props(); const uid = $props.id();</script>\n<p>{v}{uid}</p>\n";
    let js = emit(src, "App.svelte");
    let uid = js.find("const uid = $.props_id();").expect("uid hoist");
    let push = js.find("$.push($$props, true);").expect("frame push");
    let prop = js
        .find("$.prop($$props, 'v', 11, 'hi')")
        .expect("prop source");
    assert!(
        uid < push && push < prop,
        "hoist order: uid const, then the frame push, then the prop source:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_id_module_script_fails_closed() {
    // Module-script placement remains a precise `ModuleScriptItem` refusal;
    // ordinary module statements cannot make a module-scope rune legal.
    assert_fail_closed(
        "<script module>const uid = $props.id();</script>\n<script>let c = $state(0);</script>\n<p>{c}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ModuleScriptItem { construct, .. } if *construct == "variable declaration"),
    );
}

#[test]
fn props_id_uncalled_member_fails_closed() {
    // An uncalled `$props.id` (no parens) — official `rune_missing_parentheses`.
    assert_fail_closed(
        "<script>const f = $props.id;</script>\n<p>{f}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
    );
}

#[test]
fn props_id_with_arguments_fails_closed() {
    // `$props.id('x')` — official `rune_invalid_arguments` (zero args only), even
    // in the valid declarator position.
    assert_fail_closed(
        "<script>const uid = $props.id('x');</script>\n<p>{uid}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
    );
}

#[test]
fn props_id_with_spread_argument_fails_closed() {
    // `$props.id(...a)` — official `rune_invalid_spread`.
    assert_fail_closed(
        "<script>const uid = $props.id(...[1]);</script>\n<p>{uid}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
    );
}

#[test]
fn props_wrong_member_fails_closed() {
    // `$props.foo()` — official `rune_invalid_name` (the member wildcard arm).
    assert_fail_closed(
        "<script>const x = $props.foo();</script>\n<p>{x}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.<member>"),
    );
}

#[test]
fn props_id_duplicate_use_fails_closed() {
    // Two `$props.id()` uses — official `props_duplicate` ("Cannot use
    // `$props.id()` more than once"). Both are in the valid position; the SECOND
    // use is the refusal.
    assert_fail_closed(
        "<script>const u1 = $props.id(); const u2 = $props.id();</script>\n<p>{u1}{u2}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id duplicate"),
    );
}

#[test]
fn props_id_parenthesized_spellings_fail_closed() {
    // Verter locks every parenthesized `$props.id` spelling fail-closed (the
    // strict declarator-init shape is the sole accepted spelling).
    for (label, src) in [
        (
            "paren-callee",
            "<script>const uid = ($props.id)();</script>\n<p>{uid}</p>\n",
        ),
        (
            "paren-object",
            "<script>const uid = ($props).id();</script>\n<p>{uid}</p>\n",
        ),
        (
            "paren-init",
            "<script>const uid = ($props.id());</script>\n<p>{uid}</p>\n",
        ),
    ] {
        assert_fail_closed_labeled(
            label,
            src,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
        );
    }
}

#[test]
fn props_id_optional_chain_spellings_fail_closed() {
    // `$props.id?.()` / `$props?.id()` — official `props_id_invalid_placement`;
    // Verter locks both optional spellings fail-closed.
    for (label, src) in [
        (
            "optional-call",
            "<script>const uid = $props.id?.();</script>\n<p>{uid}</p>\n",
        ),
        (
            "optional-member",
            "<script>const uid = $props?.id();</script>\n<p>{uid}</p>\n",
        ),
    ] {
        assert_fail_closed_labeled(
            label,
            src,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
        );
    }
}

#[test]
fn props_id_shadowed_root_remains_an_ordinary_call() {
    // A function PARAMETER named `$props` shadows the rune: `$props.id()` inside
    // is a PLAIN member call, NOT a rune — so the refusal is the instance-script
    // ITEM gate (an unadmitted function), never the `$props.id` rune arm. This
    // discriminates the scan's shadow-awareness.
    let js = emit(
        "<script>let c = $state(0);\nfunction f($props){ return $props.id(); }</script>\n<button onclick={() => c = 1}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("function f($props){ return $props.id(); }"),
        "shadowed `$props` call must remain ordinary JavaScript:\n{js}"
    );
    assert!(
        !js.contains("$.props_id"),
        "shadowed call was rune-lowered:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn props_id_var_keyword_fails_closed() {
    // A `var` `$props.id()` declarator stays fail-closed — the same non-`let`
    // rune-declarator boundary the other runes keep (`var` read semantics are a
    // distinct official surface).
    assert_fail_closed(
        "<script>var uid = $props.id();</script>\n<p>{uid}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props.id"),
    );
}

#[test]
fn props_id_ts_annotated_declarator_erases_the_annotation() {
    // A TS-annotated `$props.id()` declarator stays fail-closed: a plain
    // `<script>` rejects the annotation at the official script-body parse gate,
    // and a `lang="ts"` script is the fail-closed TypeScript boundary pinned
    // here (the item carrier additionally admits only the unannotated shape).
    let js = emit(
        "<script lang=\"ts\">const uid: string = $props.id();</script>\n<p>x</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("const uid = $.props_id();"),
        "typed props id did not lower:\n{js}"
    );
    assert!(!js.contains(": string"), "type annotation leaked:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

// ── plain `$props()` defaults — the shared `$.prop` substrate ─────────────────

#[test]
fn props_plain_mutated_default_lowers_flag_7() {
    // A mutated plain default (`a++`) sets UPDATED: flags 7, `$.update_prop(a)`,
    // and NO context frame (a plain prop write never forces it). Verified against
    // svelte@5.56.10.
    let src =
        "<script>let { a = 1 } = $props();</script>\n<button onclick={() => a++}>{a}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 7, 1);"),
        "a mutated plain default is the flag-7 prop source:\n{js}"
    );
    assert!(
        js.contains("$.update_prop(a)"),
        "the postfix update is the prop update helper:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "a plain prop write opens no context frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_plain_prefix_decrement_lowers_update_pre_prop() {
    // `--a` (prefix) lowers to `$.update_pre_prop(a, -1)` — the prefix prop
    // update helper with the decrement literal. Verified against svelte@5.56.10.
    let src =
        "<script>let { a = 1 } = $props();</script>\n<button onclick={() => --a}>{a}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.update_pre_prop(a, -1)"),
        "a prefix decrement is the pre-update prop helper:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_plain_compound_assign_lowers_getter_setter_form() {
    // `a += 1` lowers to the getter/setter compound form `a(a() + 1)`. Verified
    // against svelte@5.56.10.
    let src =
        "<script>let { a = 1 } = $props();</script>\n<button onclick={() => a += 1}>{a}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("a(a() + 1)"),
        "a compound assign reads through the getter and writes the setter:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_plain_object_default_thunk_parenthesizes_object_body() {
    // An OBJECT default thunk parenthesizes its body (`() => ({ x: 1 })`) — the
    // required arrow-body syntax. Verified against svelte@5.56.10.
    let src = "<script>let { a = { x: 1 } } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 19, () => ({ x: 1 }))"),
        "an object default thunk parenthesizes the body:\n{js}"
    );
    // NEGATIVE: a plain lazy default never proxies.
    assert!(
        !js.contains("$.proxy"),
        "a plain lazy default must not proxy:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_zero_arg_call_default_collapses_to_bare_callee() {
    // `{ a = foo() }` — the zero-arg identifier-callee default collapses to the
    // BARE callee as the lazy carrier (`$.prop($$props, 'a', 19, foo)`), never a
    // thunk. Verified against svelte@5.56.10.
    let src = "<script>let { a = foo() } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 19, foo)"),
        "a zero-arg call default collapses to the bare callee:\n{js}"
    );
    assert!(
        !js.contains("19, () =>"),
        "never a thunk for the zero-arg callee optimization:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_getter_call_sibling_default_thunks_the_call() {
    // `{ a = 1, b = a() }` — calling the sibling PROP VALUE rewrites the callee to
    // the getter (`a()`), so the rewritten call is `a()()` and must ride a FULL
    // thunk (the zero-arg collapse applies only to a bare identifier callee).
    // The prop-rooted call also forces the context frame (the official unsafe-call
    // rule). Verified against svelte@5.56.10.
    let src = "<script>let { a = 1, b = a() } = $props();</script>\n<p>{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'b', 19, () => a()())"),
        "a sibling-value call default thunks the rewritten call:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "a prop-rooted call forces the context frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_mixed_getter_and_member_reads_split_per_member() {
    // `{ a = 1, b }` — the default-bearing member is a `$.prop` getter; the
    // no-default member stays the direct `$$props.b` read; ONE `$.prop` total.
    let src = "<script>let { a = 1, b } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 3, 1)"),
        "the default-bearing member is a prop source:\n{js}"
    );
    assert!(
        js.contains("$$props.b"),
        "the no-default member reads direct:\n{js}"
    );
    assert!(
        !js.contains("$.prop($$props, 'b'"),
        "the no-default member emits no $.prop:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_no_default_mutated_lowers_flag_7_without_default_arg() {
    // A NO-default plain prop that is written becomes a prop source (`updated`)
    // with NO default argument: `$.prop($$props, 'a', 7)`; reads flip to the
    // getter. Verified against svelte@5.56.10.
    let src = "<script>let { a } = $props();</script>\n<button onclick={() => a++}>{a}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 7);"),
        "a written no-default prop is `$.prop($$props, 'a', 7)`:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, a())"),
        "reads flip to the getter:\n{js}"
    );
    assert!(
        !js.contains("$$props.a"),
        "a prop-source read never reads $$props directly:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_no_default_deep_mutated_plain_prop_stays_raw_member_write() {
    // A PLAIN (non-bindable) prop deep-mutation (`a.x++`) keeps the RAW member
    // write over the getter base (`a().x++`) — the setter-with-mutation-flag wrap
    // is bindable-only — and the member root forces the context frame. Verified
    // against svelte@5.56.10.
    let src = "<script>let { a } = $props();</script>\n<button onclick={() => a.x++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 7);"),
        "a deep-mutated no-default prop is flag-7:\n{js}"
    );
    assert!(
        js.contains("a().x++"),
        "the plain-prop member mutation stays a raw member write:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "a prop-rooted member forces the context frame:\n{js}"
    );
    // NEGATIVE: the bindable-only mutation wrap never applies to a plain prop.
    assert!(
        !js.contains("a(a().x++, true)"),
        "the setter mutation wrap is bindable-only:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_object_default_single_member_update_locks_flag_23() {
    // A SINGLE-LEVEL member update over an object default (`a.x++`) pins the
    // full UPDATED | LAZY flag composition: exactly 23 (3 | 4 | 16), with the
    // parenthesized thunk and the getter-based member write. Verified against
    // svelte@5.56.10.
    let src = "<script>let { a = { x: 1 } } = $props();</script>\n<button onclick={() => a.x++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 23, () => ({ x: 1 }));"),
        "a member-updated object default is flags 23 with the thunk:\n{js}"
    );
    assert!(
        js.contains("a().x++"),
        "the member update reads through the getter base:\n{js}"
    );
    // NEGATIVE: neither flag axis may drop — not LAZY-only (19), not bare (3).
    assert!(
        !js.contains("'a', 19,"),
        "the member update must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(
        !js.contains("'a', 3,"),
        "the lazy object default must never emit bare flags 3:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_plain_default_nested_member_update_sets_updated_flag_7() {
    // A NESTED member update (`a.b.c++`, depth ≥ 2) is a DEEP MUTATION of the
    // ROOT prop `a` exactly like an immediate one (`a.x++`): UPDATED is set,
    // flags 7 with the raw simple default. Verified against svelte@5.56.10.
    let src = "<script>let { a = 1 } = $props();</script>\n<button onclick={() => a.b.c++}>{a}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 7, 1);"),
        "a nested member update sets UPDATED (flags 7):\n{js}"
    );
    assert!(
        js.contains("a().b.c++"),
        "the nested member write reads through the getter base:\n{js}"
    );
    // NEGATIVE: the un-updated flag value must be gone.
    assert!(
        !js.contains("$.prop($$props, 'a', 3, 1)"),
        "the nested write must not drop UPDATED to flags 3:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_object_default_nested_member_update_sets_updated_flag_23() {
    // A nested member update over an OBJECT default keeps the parenthesized
    // thunk AND sets UPDATED: flags 23 (3 | UPDATED 4 | LAZY 16). Verified
    // against svelte@5.56.10.
    let src = "<script>let { a = { b: { c: 0 } } } = $props();</script>\n<button onclick={() => a.b.c++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 23, () => ({ b: { c: 0 } }));"),
        "a nested member update over an object default is flags 23 with the thunk:\n{js}"
    );
    assert!(
        js.contains("a().b.c++"),
        "the nested member write reads through the getter base:\n{js}"
    );
    // NEGATIVE: the un-updated flag value must be gone.
    assert!(
        !js.contains("'a', 19,"),
        "the nested write must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_object_default_nested_member_assignment_sets_updated_flag_23() {
    // The nested ASSIGNMENT form (`a.b.c = 1`) is the same deep mutation of the
    // root prop as the update form: UPDATED set, flags 23. Verified against
    // svelte@5.56.10.
    let src = "<script>let { a = { b: { c: 0 } } } = $props();</script>\n<button onclick={() => a.b.c = 1}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 23, () => ({ b: { c: 0 } }));"),
        "a nested member assignment sets UPDATED (flags 23):\n{js}"
    );
    assert!(
        js.contains("a().b.c = 1"),
        "the nested assignment reads through the getter base:\n{js}"
    );
    // NEGATIVE: the un-updated flag value must be gone.
    assert!(
        !js.contains("'a', 19,"),
        "the nested assignment must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_computed_root_nested_update_sets_updated_and_keeps_key_read() {
    // A COMPUTED link inside the mutation chain (`a[k].c++`) still attributes
    // the deep mutation to the ROOT `a` (flags 23), while the computed KEY `k`
    // stays a READ: `k` keeps flags 3 and its reference rewrites to the getter
    // (`a()[k()].c++`). Verified against svelte@5.56.10.
    let src = "<script>let { a = { b: { c: 0 } }, k = 'b' } = $props();</script>\n<button onclick={() => a[k].c++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 23, () => ({ b: { c: 0 } }))"),
        "the computed-link chain still sets UPDATED on the root (flags 23):\n{js}"
    );
    assert!(
        js.contains("$.prop($$props, 'k', 3, 'b')"),
        "the computed key stays a read-only prop (flags 3):\n{js}"
    );
    assert!(
        js.contains("a()[k()].c++"),
        "the computed key resolves as a getter READ inside the write target:\n{js}"
    );
    // NEGATIVE: the root must not stay un-updated; the key must not gain UPDATED.
    assert!(
        !js.contains("'a', 19,"),
        "the computed-link write must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(
        !js.contains("'k', 7,"),
        "the computed KEY is a read, never an updated prop:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_computed_leaf_nested_update_sets_updated_and_keeps_key_read() {
    // A computed OUTERMOST member (`a.b[k]++`) roots the deep mutation at `a`
    // (flags 23) through the static link, and the computed key `k` stays a
    // getter READ (`a().b[k()]++`). Verified against svelte@5.56.10.
    let src = "<script>let { a = { b: { c: 0 } }, k = 'c' } = $props();</script>\n<button onclick={() => a.b[k]++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 23, () => ({ b: { c: 0 } }))"),
        "the computed-leaf chain still sets UPDATED on the root (flags 23):\n{js}"
    );
    assert!(
        js.contains("$.prop($$props, 'k', 3, 'c')"),
        "the computed key stays a read-only prop (flags 3):\n{js}"
    );
    assert!(
        js.contains("a().b[k()]++"),
        "the computed key resolves as a getter READ inside the write target:\n{js}"
    );
    // NEGATIVE: the root must not stay un-updated.
    assert!(
        !js.contains("'a', 19,"),
        "the computed-leaf write must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_sequence_default_thunk_parenthesizes_sequence_body() {
    // A SEQUENCE-expression default (`= (1, 2)`) embeds as ONE parenthesized
    // thunk body — `() => (1, 2)` — so the comma expression stays a single
    // `$.prop` argument and the thunk returns the sequence value (2). Verified
    // against svelte@5.56.10.
    let src = "<script>let { a = (1, 2) } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    // The FULL call text pins the arity: exactly four `$.prop` arguments.
    assert!(
        js.contains("let a = $.prop($$props, 'a', 19, () => (1, 2));"),
        "a sequence default is one parenthesized lazy thunk:\n{js}"
    );
    // NEGATIVE: the bare embedding would splice a stray fifth `$.prop` argument
    // and truncate the thunk body to `() => 1`.
    assert!(
        !js.contains("() => 1, 2"),
        "the sequence must never embed unparenthesized:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_string_key_default_quotes_source_key() {
    // `{ 'a-b': ab = 1 }` — the non-identifier SOURCE key stays the quoted prop
    // key. Verified against svelte@5.56.10.
    let src = "<script>let { 'a-b': ab = 1 } = $props();</script>\n<p>{ab}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let ab = $.prop($$props, 'a-b', 3, 1);"),
        "the string source key stays quoted:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_arrow_default_is_simple_and_passes_raw() {
    // An ARROW default is a SIMPLE expression (official `is_simple_expression`):
    // it passes RAW with NO lazy bit (`3, () => 1`). Verified against
    // svelte@5.56.10.
    let src = "<script>let { cb = () => 1 } = $props();</script>\n<p>{cb}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'cb', 3, () => 1)"),
        "an arrow default is simple → raw, flags 3:\n{js}"
    );
    assert!(
        !js.contains("'cb', 19"),
        "an arrow default never sets LAZY:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_binary_default_is_simple_and_passes_raw() {
    // A literal BINARY default (`1 + 2`) is simple → raw, flags 3. Verified
    // against svelte@5.56.10.
    let src = "<script>let { a = 1 + 2 } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 3, 1 + 2)"),
        "a literal binary default is simple → raw:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_template_literal_default_is_lazy() {
    // A TEMPLATE-LITERAL default is NOT simple (official excludes it): lazy thunk,
    // flags 19. Verified against svelte@5.56.10.
    let src = "<script>let { a = `x` } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 19, () => `x`)"),
        "a template-literal default is the lazy thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_undefined_default_passes_raw() {
    // `{ a = undefined }` — the `undefined` identifier is simple → raw, flags 3
    // (the initial argument survives). Verified against svelte@5.56.10.
    let src = "<script>let { a = undefined } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 3, undefined)"),
        "an `undefined` default is simple → raw:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_parenthesized_default_peels_transparently() {
    // `{ a = (1) }` — author parens around a default VALUE are transparent
    // (official's ESTree has no paren nodes): simple → raw, flags 3, emitted `1`.
    // Verified against svelte@5.56.10.
    let src = "<script>let { a = (1) } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.prop($$props, 'a', 3, 1)"),
        "a parenthesized literal default peels to the raw literal:\n{js}"
    );
    assert!(
        !js.contains("3, (1)"),
        "the transparent parens never survive into the carrier:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_signal_reading_default_thunks_the_getter_read() {
    // A default reading a LIVE `$state` signal rewrites the read (`$.get(s)`)
    // and rides the lazy thunk; the declarations keep source order. Verified
    // against svelte@5.56.10.
    let src = "<script>let s = $state(1); let { a = s } = $props();</script>\n<button onclick={() => s = 2}>{a}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 19, () => $.get(s));"),
        "a signal-reading default thunks the rewritten read:\n{js}"
    );
    let s_decl = js.find("let s = $.state(1);").expect("state decl");
    let a_decl = js.find("let a = $.prop(").expect("prop decl");
    assert!(s_decl < a_decl, "declarations keep source order:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_ts_wrapped_default_erases_the_wrapper() {
    // A TS-wrapped default (`1 as number`) cannot reach emission: a plain
    // `<script>` rejects it at the official script-body parse gate
    // (`js_parse_error` parity), and a `lang="ts"` script is the fail-closed
    // TypeScript boundary pinned here. (The props lowering carries its own
    // defensive `$props() ts-wrapped default` refusal for when that boundary
    // opens — the same layered rail as the `$state()` ts-wrapped init.)
    let js = emit(
        "<script lang=\"ts\">let { a = 1 as number } = $props();</script>\n<p>{a}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let a = $.prop($$props, 'a', 3, 1);"),
        "typed props default did not lower:\n{js}"
    );
    assert!(!js.contains(" as number"), "type wrapper leaked:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn instance_script_prop_reads_lower_to_the_official_accessor_shapes() {
    // The committed conformance fixture: a `$props()` destructure whose locals
    // are read from a function in the instance script. Every named shape below
    // was read off the PINNED official compiler's own client output for these
    // exact bytes, so this test pins agreement with official rather than with
    // whatever Verter happens to emit.
    //
    // Official:
    //   let disabled = $.prop($$props, 'disabled', 3, false);
    //   function onClick() { $$props.ontoggle?.(!disabled()); }
    //   $.template_effect(() => { button.disabled = disabled(); $.set_text(text, $$props.label); });
    //   $.delegated('click', button, onClick);
    let js = emit(
        "<script>\n  let { label, disabled = false, ontoggle } = $props();\n\n  function onClick() {\n    ontoggle?.(!disabled);\n  }\n</script>\n\n<button {disabled} onclick={onClick}>{label}</button>\n",
        "PropsEvents.svelte",
    );
    // A DEFAULTED prop becomes a `$.prop` source and every read is its getter CALL.
    assert!(
        js.contains("let disabled = $.prop($$props, 'disabled', 3, false);"),
        "the defaulted prop must declare the official `$.prop` accessor:\n{js}"
    );
    // A NO-DEFAULT prop is read directly off `$$props`, never through an accessor.
    assert!(
        js.contains("$$props.ontoggle?.(!disabled())"),
        "the instance-script read must lower to the official optional-call shape:\n{js}"
    );
    assert!(
        js.contains("$$props.label"),
        "the no-default `label` prop must read directly off `$$props`:\n{js}"
    );
    // NEGATIVE — the failure mode this correction must not produce: an
    // unrewritten body that reads the authored locals bare.
    assert!(
        !js.contains("ontoggle?.(!disabled)"),
        "the instance-script body must be REWRITTEN, not emitted verbatim:\n{js}"
    );
    assert!(
        !js.contains("$.prop($$props, 'ontoggle'") && !js.contains("$.prop($$props, 'label'"),
        "a prop with no default must not be given an accessor official does not emit:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn the_prop_write_gate_reads_the_member_root_and_honours_shadowing() {
    // The gate attributes a member mutation to the CHAIN ROOT. Reading only the
    // immediate object attributes `o.x.y = 1` to `o.x` — not an identifier — and
    // silently lets a prop write through as a plain unnotified member write.
    for source in [
        // nested static member
        "<script>let { o } = $props(); function f() { o.x.y = 1; }</script>\n<button onclick={f}>go</button>\n",
        // computed hop in the chain
        "<script>let { o, k } = $props(); function f() { o[k].y = 1; }</script>\n<button onclick={f}>go</button>\n",
        // nested member UPDATE
        "<script>let { o } = $props(); function f() { o.x.y++; }</script>\n<button onclick={f}>go</button>\n",
    ] {
        assert_fail_closed(source, |s| {
            matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. }
                if *rune == "$props() non-interpolation usage")
        });
    }

    // SHADOWING, the other direction: a write to a same-named LOCAL is not a
    // prop write. `catch` params and `for` declarations bind names exactly like
    // a function parameter does, and the gate must model them or it refuses
    // components the official compiler accepts.
    for source in [
        // catch parameter
        "<script>let { o } = $props(); function f() { try {} catch (o) { o.x = 1; } }</script>\n<button onclick={f}>{o.x}</button>\n",
        // for-of declaration
        "<script>let { o, xs } = $props(); function f() { for (const o of xs) { o.x = 1; } }</script>\n<button onclick={f}>{o.x}</button>\n",
        // classic for declaration
        "<script>let { o } = $props(); function f() { for (let o = 0; o < 2; o++) {} }</script>\n<button onclick={f}>{o}</button>\n",
    ] {
        let js = emit(source, "App.svelte");
        assert!(
            parses_as_js(&js),
            "a write to a shadowing local must not refuse the component:\n{js}"
        );
    }
}

#[test]
fn props_instance_script_prop_write_stays_fail_closed() {
    // SURVIVING refusal: an INSTANCE-SCRIPT WRITE to a prop local stays the
    // fail-closed prop-usage boundary — official lowers it through the prop
    // SETTER (`a(a() + 1)`, plus the bindable `, true` notify), which this
    // backend does not emit, so it must refuse rather than emit a plain
    // unnotified write.
    assert_fail_closed(
        "<script>let { a = 1 } = $props(); $effect(() => { a += 1; });</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() non-interpolation usage"),
    );
    // The COMPLEMENT, in the same test so the boundary cannot silently widen in
    // either direction: the byte-identical shape with a READ instead of a write
    // compiles, and its prop read lowers through the getter accessor.
    let js = emit(
        "<script>let { a = 1 } = $props(); $effect(() => { console.log(a); });</script>\n<p>{a}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("console.log(a())"),
        "an instance-script prop READ must lower through its `$.prop` getter:\n{js}"
    );
}

#[test]
fn props_plain_default_ts_nonnull_member_update_fails_closed() {
    // `a!.b.c++` on a PLAIN default prop (no `$bindable`) — the flags path
    // shares the same fail-closed chain classification as the bindable wrap
    // path (one gate, not per-lane checks).
    assert_fail_closed(
        "<script>let { a = { b: { c: 0 } } } = $props();</script>\n<button onclick={() => a!.b.c++}>{a}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_plain_computed_key_ts_nonnull_update_fails_closed() {
    // `v.a[k!]++` — a non-null-wrapped computed key deeper in the chain, on a
    // PLAIN default prop (no `$bindable`): the flags path shares the same
    // fail-closed key inspection as the bindable wrap path.
    assert_fail_closed(
        "<script>let { v = { a: { b: 0 } }, k = 'b' } = $props();</script>\n<button onclick={() => v.a[k!]++}>{v}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_plain_default_private_field_update_sets_updated_flag_23() {
    // `a.#x++` through a PLAIN object-default prop (no `$bindable`): official
    // sets UPDATED — flags 23 (3 | UPDATED 4 | LAZY 16) — and the write reads
    // through the getter base (`a().#x++`). Verified against svelte@5.56.10.
    let src = "<script>let { a = {} } = $props();</script>\n<button onfocus={() => { class C { static #x = 0; static m() { a.#x++; } } C.m(); }}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 23, () => ({}));"),
        "a private-field update over an object default sets UPDATED (flags 23):\n{js}"
    );
    assert!(
        js.contains("a().#x++"),
        "the private-field write reads through the getter base:\n{js}"
    );
    // NEGATIVE: the un-updated flag value must be gone.
    assert!(
        !js.contains("'a', 19,"),
        "the private-field write must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_private_field_write_to_local_root_keeps_prop_read_only_flag_19() {
    // CONTROL: the SAME class/private-field shape mutating a handler LOCAL
    // (`o.#x++`) attributes nothing to the prop — `a` stays a read-only lazy
    // default (flags 19, no UPDATED), the local write stays raw, and the prop
    // read stays the getter call. Verified against svelte@5.56.10.
    let src = "<script>let { a = {} } = $props();</script>\n<button onfocus={() => { const o = { n: 0 }; class C { static #x = 0; static m() { o.#x++; } } C.m(); console.log(a); }}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 19, () => ({}));"),
        "a local-rooted private-field write leaves the prop read-only (flags 19):\n{js}"
    );
    assert!(
        js.contains("o.#x++"),
        "the local private-field write stays a raw member write:\n{js}"
    );
    assert!(
        js.contains("console.log(a())"),
        "the prop read stays the getter call:\n{js}"
    );
    // NEGATIVE: the local write must not leak UPDATED onto the prop.
    assert!(
        !js.contains("'a', 23,"),
        "a local-rooted private-field write must not set UPDATED on the prop:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

// ---------------------------------------------------------------------------
// UPDATED-bit + context-frame parity for writes INSIDE `$props()` DEFAULT
// expressions — the default expressions are an accepted prop-write surface
// (the write rewrites through the prop setter/getter), so the `updated` flag
// axis (bit 4) and the `is_prop_source` flip must observe them exactly like
// template-expression writes. Every expectation below is oracle-pinned against
// svelte@5.56.10.
// ---------------------------------------------------------------------------

#[test]
fn props_self_assign_default_sets_updated_flag_23() {
    // `let { a = (a = 1) }` — the self-assignment INSIDE the default rewrites
    // through the setter (`a(1)`) AND sets UPDATED: flags 23 (3 | 4 | 16), no
    // context frame (a plain reassign never forces `$.push`). Verified against
    // svelte@5.56.10.
    let src = "<script>let { a = (a = 1) } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 23, () => a(1));"),
        "a self-assign default sets UPDATED (flags 23) with the setter rewrite:\n{js}"
    );
    // NEGATIVE: the write must not be dropped from the updated axis, and a
    // plain reassign must not grow the context frame.
    assert!(
        !js.contains("'a', 19,"),
        "the self-assign default must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "a plain reassign inside a default never forces the context frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_self_update_default_sets_updated_flag_23() {
    // `let { a = (a++, 2) }` — the self-update inside the default rewrites to
    // `$.update_prop(a)` and sets UPDATED: flags 23. Verified against
    // svelte@5.56.10.
    let src = "<script>let { a = (a++, 2) } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 23, () => ($.update_prop(a), 2));"),
        "a self-update default sets UPDATED (flags 23) with the update_prop rewrite:\n{js}"
    );
    assert!(
        !js.contains("'a', 19,"),
        "the self-update default must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "a self-update inside a default never forces the context frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_default_sibling_member_write_marks_written_sibling_not_writer() {
    // `let { a = (b.x++), b = { x: 0 } }` — the member write inside `a`'s
    // default deep-mutates the SIBLING `b`: `b` gains UPDATED (flags 23), the
    // WRITER `a` stays read-only-lazy (flags 19), and the prop-rooted member
    // write forces the context frame. Verified against svelte@5.56.10.
    let src = "<script>let { a = (b.x++), b = { x: 0 } } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("a = $.prop($$props, 'a', 19, () => b().x++)"),
        "the writer member stays flags 19 with the getter-based member write:\n{js}"
    );
    assert!(
        js.contains("b = $.prop($$props, 'b', 23, () => ({ x: 0 }))"),
        "the WRITTEN sibling gains UPDATED (flags 23):\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "a prop-rooted member write inside a default forces the context frame:\n{js}"
    );
    // NEGATIVE: the updated mark lands on the written sibling, never the writer.
    assert!(
        !js.contains("'b', 19,"),
        "the written sibling must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(
        !js.contains("'a', 23,"),
        "the writer must not inherit the sibling's UPDATED mark:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_default_sibling_member_write_marks_sibling_writer_second() {
    // The SAME sibling member write with the member order swapped
    // (`let { b = { x: 0 }, a = (b.x++) }`) — the mark is order-independent:
    // `b` is 23, `a` is 19, frame present. Verified against svelte@5.56.10.
    let src = "<script>let { b = { x: 0 }, a = (b.x++) } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("b = $.prop($$props, 'b', 23, () => ({ x: 0 }))"),
        "the written sibling gains UPDATED regardless of member order:\n{js}"
    );
    assert!(
        js.contains("a = $.prop($$props, 'a', 19, () => b().x++)"),
        "the writer stays flags 19 regardless of member order:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the context frame is order-independent:\n{js}"
    );
    assert!(
        !js.contains("'b', 19,"),
        "the written sibling must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_default_sibling_reassign_marks_sibling_flag_7_without_frame() {
    // `let { a = (b = 5), b = 0 }` — a PLAIN sibling reassign inside a default
    // sets UPDATED on `b` (flags 7 — its own default `0` is simple/raw, no
    // LAZY) and, unlike a member write, never forces the context frame.
    // Verified against svelte@5.56.10.
    let src = "<script>let { a = (b = 5), b = 0 } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("a = $.prop($$props, 'a', 19, () => b(5))"),
        "the writer stays flags 19 with the setter rewrite:\n{js}"
    );
    assert!(
        js.contains("b = $.prop($$props, 'b', 7, 0)"),
        "the reassigned sibling gains UPDATED (flags 7) with the raw simple default:\n{js}"
    );
    assert!(
        !js.contains("'b', 3,"),
        "the reassigned sibling must not drop UPDATED to flags 3:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "a plain sibling reassign never forces the context frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_default_write_to_no_default_sibling_makes_it_prop_source_flag_7() {
    // `let { a = (b = 5), b }` — the sibling reassign makes the NO-default `b`
    // a PROP SOURCE (`is_prop_source` = updated): it emits `$.prop($$props,
    // 'b', 7)` with NO default argument and its reads flip to the getter (no
    // direct `$$props.b` read survives). Verified against svelte@5.56.10.
    let src = "<script>let { a = (b = 5), b } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("b = $.prop($$props, 'b', 7);"),
        "a no-default sibling written in a default becomes a flag-7 prop source:\n{js}"
    );
    assert!(
        js.contains("a = $.prop($$props, 'a', 19, () => b(5))"),
        "the writer rewrites through the sibling's setter:\n{js}"
    );
    assert!(
        !js.contains("$$props.b"),
        "a prop-source read never reads $$props directly:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "a plain sibling reassign never forces the context frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_self_member_write_default_sets_updated_and_context_frame() {
    // `let { a = a.x++ ?? { x: 0 } }` — the SELF member write inside the
    // default deep-mutates `a`: UPDATED set (flags 23), the write reads
    // through the getter base, and the prop-rooted member forces the context
    // frame (`$.push($$props, true)` / `$.pop()`). Verified against
    // svelte@5.56.10.
    let src = "<script>let { a = a.x++ ?? { x: 0 } } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 23, () => a().x++ ?? { x: 0 });"),
        "a self member-write default sets UPDATED (flags 23) over the getter base:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "a prop-rooted member write inside a default forces the context frame:\n{js}"
    );
    assert!(
        js.contains("$.pop();"),
        "the context frame closes with $.pop():\n{js}"
    );
    assert!(
        !js.contains("'a', 19,"),
        "the self member write must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_self_member_assign_default_sets_updated_and_context_frame() {
    // `let { a = (a.x = 1) }` — the self member ASSIGNMENT variant: flags 23,
    // getter-based member assign, context frame present. Verified against
    // svelte@5.56.10.
    let src = "<script>let { a = (a.x = 1) } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 23, () => a().x = 1);"),
        "a self member-assign default sets UPDATED (flags 23) over the getter base:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "a prop-rooted member assign inside a default forces the context frame:\n{js}"
    );
    assert!(
        !js.contains("'a', 19,"),
        "the self member assign must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_aliased_local_self_assign_default_sets_updated_flag_23() {
    // `let { a: local = (local = 1) }` — the updated axis keys on the LOCAL
    // binding name under aliasing: flags 23 on source key 'a' with the aliased
    // setter rewrite. Verified against svelte@5.56.10.
    let src = "<script>let { a: local = (local = 1) } = $props();</script>\n<p>{local}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let local = $.prop($$props, 'a', 23, () => local(1));"),
        "an aliased self-assign default sets UPDATED (flags 23) on the local:\n{js}"
    );
    assert!(
        !js.contains("'a', 19,"),
        "the aliased self-assign must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_default_shadowed_param_write_keeps_prop_read_only_flag_19() {
    // CONTROL: `let { a = ((a) => (a = 1))(0) }` — the write inside the
    // default hits the SHADOWING arrow parameter `a`, never the prop: the prop
    // stays read-only-lazy (flags 19, no UPDATED). The IIFE's non-identifier
    // callee root still forces the context frame (official
    // `is_safe_identifier` returns false for a non-identifier leaf). Verified
    // against svelte@5.56.10.
    let src = "<script>let { a = ((a) => (a = 1))(0) } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("'a', 19,"),
        "a shadowed-param write leaves the prop read-only (flags 19):\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the non-identifier IIFE callee root forces the context frame:\n{js}"
    );
    // NEGATIVE: the shadowed write must not leak UPDATED onto the prop, and
    // the param write must stay a raw assignment (no setter call).
    assert!(
        !js.contains("'a', 23,"),
        "a shadowed-param write must not set UPDATED on the prop:\n{js}"
    );
    assert!(
        !js.contains("a(1)"),
        "the shadowed-param write must not rewrite through the prop setter:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

// ── Destructuring-assignment WRITES inside `$props()` defaults — fail-closed ──
//
// PINS A DELIBERATE BOUNDARY: official svelte@5.56.10 ACCEPTS a destructuring-
// assignment WRITE inside a `$props()` default and lowers the write through
// the prop SETTER inside an UPDATED (flags 23) IIFE thunk. Verter deliberately
// fails EVERY destructuring-assignment write closed — the single shared
// `DestructuringWrite` refusal rail covers all expression surfaces, with no
// per-surface carve-outs — so this officially-accepted form refuses too (a
// fail-safe over-refusal, never a wrong emit). These pins keep the exclusion
// EXPLICIT rather than silent: they are GREEN by design (they pin the current
// fail-closed behavior), and their discrimination target is a future change
// that silently starts ACCEPTING and EMITTING destructuring writes without
// the rail's typed refusal.

#[test]
fn props_default_object_destructuring_write_fails_closed() {
    // `let { a = ({ a } = { a: 1 }) } = $props();` — official svelte@5.56.10
    // ACCEPTS the OBJECT-pattern destructuring-assignment write inside the
    // default, assigning through the prop setter (oracle-verified emit):
    //   let a = $.prop($$props, 'a', 23, () => (($$value) => {
    //       a($$value.a);
    //       return $$value;
    //   })({ a: 1 }));
    // Verter deliberately fails ALL destructuring-assignment writes closed
    // through the shared `DestructuringWrite` rail, so this officially-
    // accepted form refuses too — the pin documents the boundary so the
    // exclusion stays explicit, not silent. It must go RED if destructuring
    // writes ever start emitting WITHOUT the rail's typed refusal.
    assert_fail_closed(
        "<script>let { a = ({ a } = { a: 1 }) } = $props();</script>\n<p>{a}</p>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }
            )
        },
    );
}

#[test]
fn props_default_array_destructuring_write_fails_closed() {
    // `let { a = ([a] = [1]) } = $props();` — the ARRAY-pattern form. Official
    // svelte@5.56.10 ACCEPTS it, assigning through the prop setter over the
    // destructured array (oracle-verified emit):
    //   let a = $.prop($$props, 'a', 23, () => (($$value) => {
    //       var $$array = $.to_array($$value, 1);
    //       a($$array[0]);
    //       return $$value;
    //   })([1]));
    // Verter fails it closed through the same shared `DestructuringWrite`
    // rail — both pattern kinds sit on one explicit, documented boundary; a
    // silent acceptance/emission without the typed refusal turns this RED.
    assert_fail_closed(
        "<script>let { a = ([a] = [1]) } = $props();</script>\n<p>{a}</p>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }
            )
        },
    );
}

// ---------------------------------------------------------------------------
// Simple/lazy decision over the VISITED initializer — official
// `is_simple_expression` runs on the `$props()` default AFTER reference
// rewriting (`initial = context.visit(binding.initial)` in
// `VariableDeclaration.js`), so a FUNCTION-LITERAL default stays a RAW simple
// initial (no LAZY bit) even when its body carries rewrites (the outer node
// kind survives visiting), while a rewritten identifier LEAF (a getter call /
// `$$props` member / signal read) breaks simplicity. Every expectation below
// is oracle-pinned against svelte@5.56.10.
// ---------------------------------------------------------------------------

#[test]
fn props_arrow_default_with_inner_self_write_stays_raw_simple_flag_7() {
    // `let { a = () => (a = 1) }` — the arrow's OUTER node kind survives the
    // inner setter rewrite, so the default stays a RAW simple initial: flags 7
    // (3 | 4 — the deferred write sets UPDATED, NO LAZY bit) with the single
    // rewritten arrow. Official prints `() => a(1)`; Verter's source-
    // preserving carrier keeps the author's body parens (`() => (a(1))`) — a
    // waived cosmetic difference. Verified against svelte@5.56.10.
    let src = "<script>let { a = () => (a = 1) } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 7, () => (a(1)));"),
        "an arrow default with an inner self-write stays a raw simple initial (flags 7):\n{js}"
    );
    // NEGATIVE: no double thunk over the already-function default, no LAZY
    // bit, and the UPDATED bit is kept.
    assert!(
        !js.contains("() => () =>"),
        "a function-literal default must never ride an extra thunk:\n{js}"
    );
    assert!(
        !js.contains("'a', 23,") && !js.contains("'a', 19,"),
        "a function-literal default must not set LAZY:\n{js}"
    );
    assert!(
        !js.contains("'a', 3,"),
        "the inner self-write must keep UPDATED:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_arrow_default_referencing_sibling_stays_raw_with_getter_body() {
    // `let { a = () => b, b = 0 }` — the sibling GETTER rewrite lands INSIDE
    // the arrow body; the arrow default stays a RAW simple initial (flags 3,
    // `() => b()`), never the lazy thunk over it. Verified against
    // svelte@5.56.10.
    let src = "<script>let { a = () => b, b = 0 } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("a = $.prop($$props, 'a', 3, () => b())"),
        "an arrow default over a sibling read stays raw (flags 3):\n{js}"
    );
    assert!(
        js.contains("b = $.prop($$props, 'b', 3, 0)"),
        "the sibling keeps its simple literal default:\n{js}"
    );
    assert!(
        !js.contains("'a', 19,") && !js.contains("() => () =>"),
        "the sibling-reading arrow must not set LAZY / double-thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_conditional_default_with_rewritten_leaf_stays_lazy_flag_19() {
    // CONTROL (green pre-fix): `let { a = b ? 1 : 2, b = 0 }` — the sibling
    // leaf `b` REWRITES to the getter call, so the visited conditional is NOT
    // simple: LAZY thunk, flags 19. Discriminates the visited-node decision
    // from one keyed on the ORIGINAL node kind alone (which would wrongly pass
    // `b() ? 1 : 2` raw). Verified against svelte@5.56.10.
    let src = "<script>let { a = b ? 1 : 2, b = 0 } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("a = $.prop($$props, 'a', 19, () => b() ? 1 : 2)"),
        "a conditional over a rewritten leaf rides the LAZY thunk (flags 19):\n{js}"
    );
    assert!(
        !js.contains("'a', 3,") && !js.contains("'a', 7,"),
        "a rewritten-leaf conditional must not pass raw:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_array_default_with_sibling_getter_stays_lazy_flag_19() {
    // CONTROL (green pre-fix): `let { a = [b], b = 0 }` — an ARRAY default is
    // structurally non-simple regardless of rewrites: LAZY thunk with the
    // sibling getter inside (flags 19). Verified against svelte@5.56.10.
    let src = "<script>let { a = [b], b = 0 } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("a = $.prop($$props, 'a', 19, () => [b()])"),
        "an array default stays the LAZY thunk (flags 19):\n{js}"
    );
    assert!(
        !js.contains("'a', 3,"),
        "an array default must never pass raw:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_logical_default_with_unrewritten_ident_leaf_stays_raw_flag_3() {
    // CONTROL (green pre-fix): `let { a = undefined ?? 1 }` — the identifier
    // leaf `undefined` never rewrites, so the visited logical stays simple:
    // raw initial, flags 3. Verified against svelte@5.56.10.
    let src = "<script>let { a = undefined ?? 1 } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 3, undefined ?? 1);"),
        "an unrewritten-ident-leaf logical default stays raw (flags 3):\n{js}"
    );
    assert!(
        !js.contains("'a', 19,"),
        "an unrewritten-ident-leaf logical must not set LAZY:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_export_let_props_lower_through_the_prop_source_substrate() {
    // A bare legacy `export let` is a prop source with base flags 8 (BINDABLE —
    // legacy props are bindable by default) and reads as the accessor CALL
    // (oracle-verified against svelte@5.56.10).
    let js = emit(
        "<script>\nexport let label;\n</script>\n<p>{label}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let label = $.prop($$props, 'label', 8);"),
        "bare export let lowers to the 3-arg $.prop with flags 8:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, label())"),
        "a legacy prop ALWAYS reads as the accessor call:\n{js}"
    );
    assert!(
        js.contains("($$anchor, $$props)"),
        "a prop-bearing component threads $$props:\n{js}"
    );
    // NEGATIVE: never `$.get(label)`, never a bare `$$props.label` read, no frame.
    assert!(
        !js.contains("$.get(label)") && !js.contains("$$props.label"),
        "a legacy prop read is the accessor call, not $.get / $$props member:\n{js}"
    );
    assert!(
        !js.contains("$.push") && !js.contains("$.pop"),
        "a legacy export-let prop opens no component context frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");

    // A DEFAULT-bearing export let carries the default as the 4th arg (a simple
    // literal passes RAW — no lazy thunk, no lazy bit).
    let js = emit(
        "<script>\nexport let label = 'hi';\n</script>\n<p>{label}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let label = $.prop($$props, 'label', 8, 'hi');"),
        "default export let lowers the default as the 4th arg:\n{js}"
    );
    // NEGATIVE: no thunk wrap for a simple literal default.
    assert!(
        !js.contains("() => 'hi'"),
        "a simple literal default must pass raw (no lazy thunk):\n{js}"
    );
}

#[test]
fn legacy_export_let_mutated_prop_carries_updated_flag_and_update_prop() {
    // A template-written legacy prop composes UPDATED (+4) onto the legacy base 8
    // → 12, writes through the prop update helper, and still reads as the
    // accessor call (oracle case: `export let count = 0` + `count++`).
    let js = emit(
        "<script>\nexport let count = 0;\n</script>\n<button onclick={() => count++}>{count}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let count = $.prop($$props, 'count', 12, 0);"),
        "a mutated legacy prop carries flags 12 (8 | UPDATED 4):\n{js}"
    );
    assert!(
        js.contains("$.update_prop(count)"),
        "a prop increment lowers through $.update_prop:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, count())"),
        "the mutated prop still reads as the accessor call:\n{js}"
    );
    // NEGATIVE: never the signal family on a prop.
    assert!(
        !js.contains("$.update(count)") && !js.contains("$.set(count"),
        "a prop write must not use the signal $.update/$.set family:\n{js}"
    );

    // A bare REASSIGNMENT writes through the setter call (`v = 2` → `v(2)`).
    let js = emit(
        "<script>\nexport let v;\n</script>\n<button onclick={() => v = 2}>{v}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let v = $.prop($$props, 'v', 12);"),
        "a reassigned bare prop carries flags 12 with no default:\n{js}"
    );
    assert!(
        js.contains("v(2)"),
        "a prop reassignment writes through the setter call:\n{js}"
    );
}

#[test]
fn instance_script_prop_writes_fail_closed_in_both_modes() {
    // A SCRIPT-BODY prop write (`function inc() { count += 1; }`) is an
    // official-ACCEPTS surface Verter does not lower yet (official rewrites the
    // body write through the prop accessor — `count(count() + 1)`; a bindable
    // member write additionally notifies with `, true`). The WHOLE surface —
    // runes `$props()` AND legacy `export let` — must refuse through the ONE
    // prop-usage gate with the precise diagnostic, never silently accept and
    // never emit a raw unrewritten body.

    // LEGACY: `export let` + a function-body compound assign.
    let err = emit_result(
        "<script>\nexport let count = 0;\nfunction inc() { count += 1; }\n</script>\n<button onclick={inc}>{count}</button>\n",
    )
    .expect_err("a legacy `export let` script-body prop write must fail closed");
    assert!(
        matches!(
            &err,
            ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. })
                if *rune == "$props() non-interpolation usage"
        ),
        "the legacy script-body prop write must refuse via the prop-usage gate: {err:?}"
    );

    // RUNES: the SAME shape through a `$props()` destructure member.
    let err = emit_result(
        "<script>\nlet { count = 0 } = $props();\nfunction inc() { count += 1; }\n</script>\n<button onclick={inc}>{count}</button>\n",
    )
    .expect_err("a runes `$props()` script-body prop write must fail closed");
    assert!(
        matches!(
            &err,
            ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. })
                if *rune == "$props() non-interpolation usage"
        ),
        "the runes script-body prop write must refuse via the prop-usage gate: {err:?}"
    );

    // The coupled bindable MEMBER-write notify (`o.x++` → official
    // `o(o().x++, true)`) rides the same deferral: a script-body member
    // mutation on a prop object must ALSO refuse — not emit a notify-less
    // member write.
    let err = emit_result(
        "<script>\nexport let o = { x: 0 };\nfunction bump() { o.x += 1; }\n</script>\n<button onclick={bump}>go</button>\n",
    )
    .expect_err("a script-body prop MEMBER write must fail closed");
    assert!(
        matches!(
            &err,
            ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. })
                if *rune == "$props() non-interpolation usage"
        ),
        "the script-body prop member write must refuse via the prop-usage gate: {err:?}"
    );
}

// ── The `<slot>` element surface (`$.slot(...)`) ────────────────────────────

#[test]
fn slot_default_empty_in_element_emits_dollar_slot_anchor_topology() {
    // `<div><slot /></div>` (legacy — no runes): the slot keeps its `<!>` anchor
    // even as the sole child (NEVER controlled), and emits
    // `$.slot(node, $$props, 'default', {}, null)` at its document position.
    let js = emit("<div><slot /></div>\n", "App.svelte");
    assert!(
        js.contains("$.from_html(`<div><!></div>`)"),
        "the slot serializes as a `<!>` hydration anchor:\n{js}"
    );
    assert!(
        js.contains("var node = $.child(div);"),
        "the walk descends to the slot anchor:\n{js}"
    );
    assert!(
        js.contains("$.slot(node, $$props, 'default', {}, null);"),
        "the default slot call topology:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "a slot-bearing component binds the `$$props` param:\n{js}"
    );
    assert!(
        js.contains("import 'svelte/internal/flags/legacy';"),
        "a non-runes slot component is legacy mode:\n{js}"
    );
    // NEGATIVE: no `<slot>` DOM element in the skeleton, no frame, no `$.init`.
    assert!(
        !js.contains("<slot") && !js.contains("$.push") && !js.contains("$.init()"),
        "the slot is not a DOM element and forces no frame/init:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_as_sole_root_uses_the_comment_anchor_frame() {
    // `<slot />` as the whole template: the official comment-anchor root
    // (`var fragment = $.comment(); var node = $.first_child(fragment); …`).
    let js = emit("<slot />\n", "App.svelte");
    assert!(
        js.contains("var fragment = $.comment();"),
        "a sole-root slot mounts a comment anchor:\n{js}"
    );
    assert!(
        js.contains("var node = $.first_child(fragment);"),
        "the walk reaches the anchor:\n{js}"
    );
    assert!(
        js.contains("$.slot(node, $$props, 'default', {}, null);"),
        "the slot call:\n{js}"
    );
    assert!(
        js.contains("$.append($$anchor, fragment);"),
        "the fragment mounts:\n{js}"
    );
    // NEGATIVE: no `$.from_html` hoist for a slot-only root.
    assert!(
        !js.contains("$.from_html"),
        "a slot-only root hoists no template:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_named_emits_the_semantic_name() {
    let js = emit("<div><slot name=\"x\" /></div>\n", "App.svelte");
    assert!(
        js.contains("$.slot(node, $$props, 'x', {}, null);"),
        "the named slot passes its semantic name:\n{js}"
    );
    // NEGATIVE: the `name` attribute is CONSUMED (never a slot prop / DOM attr).
    assert!(
        !js.contains("name:") && !js.contains("set_attribute"),
        "the name attribute is consumed, not projected:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_static_and_boolean_props_emit_plain_inits() {
    let js = emit("<div><slot foo=\"bar\" on /></div>\n", "App.svelte");
    assert!(
        js.contains("$.slot(node, $$props, 'default', { foo: 'bar', on: true }, null);"),
        "static + boolean slot props are plain inits:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_spread_props_emit_ordinary_object_first_then_thunks() {
    // Official slot spread topology: `$.spread_props({ ordinary }, thunk, …)` —
    // ONE leading ordinary-prop object, then every spread thunk in source order.
    // A legacy-prop spread (`{...rest}` where `rest` is a prop accessor) unthunks
    // to the bare accessor (`rest`) — the official `b.thunk` call-unwrap.
    let js = emit(
        "<script>export let rest;</script>\n<div><slot a=\"1\" {...rest} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.slot(node, $$props, 'default', $.spread_props({ a: '1' }, rest), null);"),
        "the spread props topology (object first, unthunked accessor):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_member_spread_emits_a_thunk() {
    // `{...o.x}` (a member spread over a legacy prop) keeps the arrow thunk
    // (`() => o().x`) — only a bare zero-arg call unthunks.
    let js = emit(
        "<script>export let o;</script>\n<div><slot {...o.x} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.spread_props({}, () => o().x)"),
        "a member spread keeps its thunk (empty leading object):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_whitespace_only_fallback_emits_an_empty_callback_not_null() {
    // `<slot>   </slot>`: the RAW fragment is non-empty (a whitespace text node)
    // ⇒ a callback, but the cleaned region emits nothing ⇒ `($$anchor) => {}`.
    let js = emit("<div><slot>   </slot></div>\n", "App.svelte");
    assert!(
        js.contains("$.slot(node, $$props, 'default', {}, ($$anchor) => {});"),
        "a whitespace-only fallback is an EMPTY callback (not null):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_empty_element_form_fallback_is_null() {
    // `<slot></slot>` (no children at all): the official `null` fallback slot.
    let js = emit("<div><slot></slot></div>\n", "App.svelte");
    assert!(
        js.contains("$.slot(node, $$props, 'default', {}, null);"),
        "an empty fallback is the literal null:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_text_first_fallback_uses_the_in_closure_text_frame_without_next() {
    // `<slot>{a}</slot>`: the fallback region is text-first — the in-closure
    // `$.text()` frame with NO leading `$.next()` (a slot fallback is not an
    // `{#each}` render callback).
    let js = emit(
        "<script>export let a;</script>\n<div><slot>{a}</slot></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("($$anchor) => {") && js.contains("var text = $.text();"),
        "the text-first fallback frame:\n{js}"
    );
    assert!(
        js.contains("$.template_effect(() => $.set_text(text, a()));"),
        "the fallback text effect reads the prop accessor:\n{js}"
    );
    assert!(
        !js.contains("$.next();\n\tvar text"),
        "a slot fallback emits NO `$.next()` prelude:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn two_named_slots_walk_sibling_anchors() {
    let js = emit(
        "<div><slot name=\"a\" /><slot name=\"b\" /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.from_html(`<div><!><!></div>`)"),
        "two slots serialize two anchors:\n{js}"
    );
    assert!(
        js.contains("$.slot(node, $$props, 'a', {}, null);")
            && js.contains("var node_1 = $.sibling(node);")
            && js.contains("$.slot(node_1, $$props, 'b', {}, null);"),
        "each slot targets its own walked anchor:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn slot_mixed_prop_emits_the_template_literal_getter() {
    // `foo="p{a}s"` — a mixed value with state ⇒ a getter over the template
    // literal with the `?? ''` coercion (official `build_template_chunk`).
    let js = emit(
        "<script>export let a;</script>\n<div><slot foo=\"p{a}s\" /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("get foo() {") && js.contains("return `p${a() ?? ''}s`;"),
        "the mixed slot prop getter:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

// ── `<slot>` official-invalid forms → EXACT official rejects ────────────────

#[test]
fn slot_dynamic_name_rejects_as_slot_element_invalid_name() {
    for source in [
        "<slot name={x} />\n",
        "<slot name=\"a{x}\" />\n",
        "<slot name />\n",
    ] {
        let err = emit_result(source).expect_err("a non-static slot name must reject");
        let ClientCompileError::OfficialReject(rejection) = err else {
            panic!("expected an OfficialReject refusal for {source:?}, got {err:?}");
        };
        assert_eq!(
            rejection.rule,
            CoreOfficialValidationRule::SlotElementInvalidName,
            "a non-static slot name rejects via SlotElementInvalidName"
        );
        assert_eq!(rejection.official_code, "slot_element_invalid_name");
    }
}

#[test]
fn slot_name_default_rejects_as_slot_element_invalid_name_default() {
    let err =
        emit_result("<slot name=\"default\" />\n").expect_err("`name=\"default\"` must reject");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected an OfficialReject refusal, got {err:?}");
    };
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::SlotElementInvalidNameDefault,
    );
    assert_eq!(rejection.official_code, "slot_element_invalid_name_default");
}

#[test]
fn slot_directives_reject_as_slot_element_invalid_attribute() {
    // Official: `<slot>` can only receive attributes / spreads / `let:` — every
    // other directive family is `slot_element_invalid_attribute`.
    for source in [
        "<slot class:on={x} />\n",
        "<slot on:click={f} />\n",
        "<slot bind:this={el} />\n",
        "<slot style:color=\"red\" />\n",
    ] {
        let err = emit_result(source).expect_err("a slot directive must reject");
        let ClientCompileError::OfficialReject(rejection) = err else {
            panic!("expected an OfficialReject refusal for {source:?}, got {err:?}");
        };
        assert_eq!(
            rejection.rule,
            CoreOfficialValidationRule::SlotElementInvalidAttribute,
            "a slot directive rejects via SlotElementInvalidAttribute for {source:?}"
        );
        assert_eq!(rejection.official_code, "slot_element_invalid_attribute");
    }
}

#[test]
fn slot_duplicate_attribute_rejects_as_attribute_duplicate() {
    let err = emit_result("<slot foo=\"1\" foo=\"2\" />\n")
        .expect_err("a duplicate slot attribute must reject");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected an OfficialReject refusal, got {err:?}");
    };
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::AttributeDuplicate
    );
    assert_eq!(rejection.official_code, "attribute_duplicate");
}

// ── `<slot>` fail-closed sub-shapes (unsupported, never fail-open) ──────────

#[test]
fn slot_let_unbound_fails_closed_with_dedicated_diagnostic() {
    // `<slot let:x>` (the producer-side provider `let:` binding) is
    // official-ACCEPTED, but the pinned svelte@5.56.10 compiler ITSELF emits
    // BROKEN output for it — a component-instance-scope
    // `const x = $.derived_safe_equal(() => $$slotProps.x);` reading an
    // UNDECLARED `$$slotProps` (`$$slotProps` is bound only inside a component
    // slot-content callback), a guaranteed runtime `ReferenceError`. Verter
    // REFUSES rather than shipping invalid runtime code — an accepted
    // fail-closed upstream-bug divergence (the same class as the bare
    // `$host()` disposition), carried by the DEDICATED `SlotLetUnbound`
    // surface with the authored DIRECTIVE span (not the whole slot span).
    let src = "<div><slot let:x /></div>\n";
    let result = emit_result(src);
    let Err(ClientCompileError::Unsupported(surface)) = result else {
        panic!("a `<slot let:x>` provider binding must fail closed, got {result:?}");
    };
    // The dedicated variant + machine-stable code (NOT the component/snippet
    // family — this is neither a component nor a snippet failure).
    assert!(
        matches!(
            &surface,
            UnsupportedSvelteRuntimeSurface::SlotLetUnbound { .. }
        ),
        "the refusal is the dedicated SlotLetUnbound surface: {surface:?}"
    );
    assert_eq!(
        surface.diagnostic_code(),
        "svelte-runtime-unsupported-slot-let-unbound",
        "the machine-stable dedicated diagnostic code"
    );
    // The message states the upstream defect: the pinned official compiler
    // emits an unbound `$$slotProps` reference, and Verter refuses rather
    // than shipping invalid runtime code.
    let message = surface.message();
    assert!(
        message.contains("$$slotProps") && message.contains("refus"),
        "the message names the unbound `$$slotProps` emission and the refusal: {message}"
    );
    // The span is the authored `let:x` DIRECTIVE span, not the enclosing slot.
    let start = src.find("let:x").expect("fixture contains the directive") as u32;
    assert_eq!(
        surface.span(),
        verter_span::Span::new(start, start + "let:x".len() as u32),
        "the refusal reports the authored directive span"
    );
    // PINNED oracle evidence (svelte@5.56.10 over this exact fixture): the
    // official client module reads `$$slotProps` exactly once and never
    // declares it — the upstream bug this refusal diverges from. If a future
    // re-pin changes this output, this pin fails and the divergence must be
    // re-ruled (see the decision-log row beside the `$host()` disposition).
    const PINNED_OFFICIAL_CLIENT: &str = "import 'svelte/internal/disclose-version';\nimport 'svelte/internal/flags/legacy';\nimport * as $ from 'svelte/internal/client';\n\nvar root = $.from_html(`<div><!></div>`);\n\nexport default function App($$anchor, $$props) {\n\tvar div = root();\n\tvar node = $.child(div);\n\tconst x = $.derived_safe_equal(() => $$slotProps.x);\n\n\t$.slot(node, $$props, 'default', {}, null);\n\t$.reset(div);\n\t$.append($$anchor, div);\n}";
    assert_eq!(
        PINNED_OFFICIAL_CLIENT.matches("$$slotProps").count(),
        1,
        "official reads the magic object exactly once"
    );
    assert!(
        PINNED_OFFICIAL_CLIENT.contains("$.derived_safe_equal(() => $$slotProps.x)"),
        "the lone occurrence is the undeclared instance-scope READ"
    );
    assert!(
        !PINNED_OFFICIAL_CLIENT.contains("const $$slotProps")
            && !PINNED_OFFICIAL_CLIENT.contains("let $$slotProps")
            && !PINNED_OFFICIAL_CLIENT.contains("var $$slotProps")
            && !PINNED_OFFICIAL_CLIENT.contains("$$slotProps)"),
        "official never declares `$$slotProps` (no decl, no parameter binding)"
    );
}

// ── The LEGACY value memo topology (`$.derived_safe_equal` + the official ────
// `build_expression` deep-read/untrack wrap — svelte@5.56.10
// `shared/utils.js` `build_expression` + `Memoizer.deriveds(runes)`).
//
// The shared `DerivedMemoizer` picks the helper BY MODE (runes → `$.derived`,
// non-runes → `$.derived_safe_equal`); the SEPARATE shared legacy value wrap
// (`(dep reads…, $.untrack(() => value))`) applies only in a DEFINITELY-legacy
// component (official `!runes && !maybe_runes`) when the value has a call, a
// member expression, or an assignment. Every assertion below is pinned against
// a direct pinned-oracle compile of the same fixture.

#[test]
fn legacy_component_call_bearing_prop_memoizes_safe_equal_with_deep_read_untrack() {
    // THE shared-owner regression: a legacy component prop `<Child foo={obj.m()}/>`
    // routes through the SAME `DerivedMemoizer` as slot props. Oracle:
    //   let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));
    //   Child($$anchor, { get foo() { return $.get($0); } });
    let js = emit(
        "<script>import Child from './Child.svelte';\nexport let obj;</script>\n<Child foo={obj.m()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));"
        ),
        "the legacy memo is `$.derived_safe_equal` over the deep-read/untrack wrap:\n{js}"
    );
    assert!(
        js.contains("get foo() {return $.get($0);}"),
        "the prop getter reads the memo:\n{js}"
    );
    // NEGATIVE: the runes helper never appears in a legacy module, and the
    // authored call executes ONLY inside the `$.untrack` thunk (one tracked
    // occurrence would re-run the memo on every dependency of the call body).
    assert!(
        !js.contains("$.derived("),
        "no plain `$.derived(` in a legacy module:\n{js}"
    );
    assert_eq!(
        js.matches("obj().m()").count(),
        1,
        "the authored call appears exactly once (inside $.untrack):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_component_member_prop_wraps_untracked_getter_without_memo() {
    // A member-bearing NON-call legacy prop (`foo={obj.x}`) is NOT memoized
    // (no `has_call`) but still legacy-wraps at the getter. Oracle:
    //   get foo() { return ($.deep_read_state(obj()), $.untrack(() => obj().x)); }
    let js = emit(
        "<script>import Child from './Child.svelte';\nexport let obj;</script>\n<Child foo={obj.x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("get foo() {return ($.deep_read_state(obj()), $.untrack(() => obj().x));}"),
        "the non-memoized legacy getter wraps deep-read + untrack:\n{js}"
    );
    assert!(
        !js.contains("$.derived"),
        "a member-only value never memoizes:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_iife_call_deps_include_arrow_captured_prop() {
    // Oracle parity (svelte@5.56.10): an IIFE `(() => obj.x)()` fires the
    // `has_call` trigger; its ONLY reference (`obj`) sits inside the arrow body
    // yet still joins the deps. Oracle:
    //   ($.deep_read_state(obj()), $.untrack(() => (() => obj().x)()))
    let js = emit(
        "<script>import Child from './Child.svelte';\nexport let obj;</script>\n<Child foo={(() => obj.x)()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.deep_read_state(obj()), $.untrack("),
        "the IIFE wrap deep-reads the arrow-captured `obj`:\n{js}"
    );
    assert!(
        js.contains("(() => obj().x)()"),
        "the untracked payload keeps the authored IIFE:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_call_bearing_slot_prop_memoizes_official_legacy_topology() {
    // The slot twin of the component regression above (the former fail-closed
    // refusal, now SUPPORTED). Oracle:
    //   let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));
    //   $.slot(node, $$props, 'default', { get foo() { return $.get($0); } }, null);
    let js = emit(
        "<script>export let obj;</script>\n<div><slot foo={obj.m()} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));"
        ),
        "the legacy slot memo is `$.derived_safe_equal` over the wrap:\n{js}"
    );
    assert!(
        js.contains("get foo() { return $.get($0); }"),
        "the slot prop getter reads the memo:\n{js}"
    );
    assert!(
        !js.contains("$.derived("),
        "no plain `$.derived(` in a legacy module:\n{js}"
    );
    assert_eq!(
        js.matches("obj().m()").count(),
        1,
        "the authored call appears exactly once (inside $.untrack):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_call_bearing_slot_spread_stays_plain_thunk() {
    // Official `SlotElement.js` NEVER memoizes a slot spread (unlike component
    // spreads) and never legacy-wraps it. Oracle:
    //   $.slot(node, $$props, 'default', $.spread_props({}, () => obj().m()), null);
    let js = emit(
        "<script>export let obj;</script>\n<div><slot {...obj.m()} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.spread_props({}, () => obj().m())"),
        "the slot spread stays the plain thunk:\n{js}"
    );
    assert!(
        !js.contains("$.derived") && !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "a slot spread neither memoizes nor wraps:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_mixed_slot_prop_call_chunk_memoizes_wrapped() {
    // A mixed slot value's call-bearing chunk memoizes the WRAPPED expression;
    // the template literal reads `$.get($0) ?? ''`. Oracle (I-shape):
    //   let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));
    //   get foo() { return `a${$.get($0) ?? ''}b`; }
    let js = emit(
        "<script>export let obj;</script>\n<div><slot foo=\"a{obj.m()}b\" /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));"
        ),
        "the mixed chunk memoizes the wrapped value:\n{js}"
    );
    assert!(
        js.contains("`a${$.get($0) ?? ''}b`"),
        "the template literal reads the memo:\n{js}"
    );
    assert!(!js.contains("$.derived("), "no runes helper:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_quoted_single_slot_prop_call_memoizes_wrapped() {
    // The QUOTED single-chunk form (`foo="{obj.m()}"`) is the same memoize
    // surface as the bare `foo={obj.m()}` (official `build_attribute_value`
    // single-chunk branch).
    let js = emit(
        "<script>export let obj;</script>\n<div><slot foo=\"{obj.m()}\" /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));"
        ),
        "the quoted single-chunk call memoizes wrapped:\n{js}"
    );
    assert!(
        js.contains("get foo() { return $.get($0); }"),
        "the getter reads the memo:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_component_mixed_call_chunk_memoizes_wrapped() {
    // The component mixed-value twin (K-shape): the call chunk memoizes the
    // wrapped value; the getter renders the template literal over `$.get($0)`.
    let js = emit(
        "<script>import Child from './Child.svelte';\nexport let obj;</script>\n<Child foo=\"a{obj.m()}b\" />\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));"
        ),
        "the component mixed call chunk memoizes wrapped:\n{js}"
    );
    assert!(
        js.contains("`a${$.get($0) ?? ''}b`"),
        "the template literal reads the memo:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_component_spread_call_memoizes_without_wrap() {
    // A component spread MEMOIZES on `has_call` but NEVER legacy-wraps
    // (official visits a `SpreadAttribute` without `build_expression`).
    // Oracle (J-shape):
    //   let $0 = $.derived_safe_equal(() => obj().m());
    //   Child($$anchor, $.spread_props(() => $.get($0)));
    let js = emit(
        "<script>import Child from './Child.svelte';\nexport let obj;</script>\n<Child {...obj.m()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.derived_safe_equal(() => (obj().m()))")
            || js.contains("$.derived_safe_equal(() => obj().m())"),
        "the spread memo holds the UNWRAPPED value:\n{js}"
    );
    assert!(
        js.contains("$.spread_props(() => $.get($0))"),
        "the spread thunk reads the memo:\n{js}"
    );
    assert!(
        !js.contains("$.untrack") && !js.contains("$.deep_read_state"),
        "a component spread never legacy-wraps:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_component_spread_prop_read_unthunks_bare_accessor() {
    // A NON-call component spread of a legacy prop unthunks the zero-arg
    // accessor read (official `b.thunk(rest())` → `rest`). Oracle (CA-shape):
    //   Child($$anchor, $.spread_props(rest));
    let js = emit(
        "<script>import Child from './Child.svelte';\nexport let rest;</script>\n<Child {...rest} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.spread_props(rest)"),
        "the zero-arg accessor spread unthunks to the bare callee:\n{js}"
    );
    assert!(
        !js.contains("() => rest()"),
        "no redundant thunk around the bare accessor:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_render_arg_call_memoizes_wrapped() {
    // A `{@render}` argument in a definitely-legacy component memoizes the
    // WRAPPED value through the same shared memoizer (X-shape oracle):
    //   let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));
    //   s($$anchor, () => $.get($0));
    let js = emit(
        "<script>export let obj;</script>\n{#snippet s(v)}<p>{v}</p>{/snippet}\n{@render s(obj.m())}\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()), $.untrack(() => obj().m())));"
        ),
        "the legacy render arg memoizes wrapped:\n{js}"
    );
    assert!(
        js.contains("s($$anchor, () => $.get($0))"),
        "the arg thunk reads the memo:\n{js}"
    );
    assert!(!js.contains("$.derived("), "no runes helper:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_slot_assignment_value_wraps_with_target_dep() {
    // An assignment-bearing value wraps (official `has_assignment`), the
    // mutable-source TARGET joins the deps as a plain `$.get` read, and the
    // legacy prop deep-reads — first-reference source order (S-shape oracle):
    //   get foo() { return ($.get(c), $.deep_read_state(obj()), $.untrack(() => $.set(c, obj().x))); }
    let js = emit(
        "<script>export let obj;\nlet c = 0;</script>\n<div><slot foo={c = obj.x} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "get foo() { return ($.get(c), $.deep_read_state(obj()), $.untrack(() => $.set(c, obj().x))); }"
        ),
        "the assignment value wraps with ordered deps:\n{js}"
    );
    assert!(
        !js.contains("$.derived"),
        "no memo for a non-call value:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_muted_property_call_value_memoizes_wrapped() {
    // A DOM-property write (`video.muted = $0`) memoizes the wrapped sequence
    // exactly like `$.set_attribute` (oracle):
    //   $.template_effect(($0) => video.muted = $0,
    //     [() => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]);
    let js = emit(
        "<script>export let obj;</script>\n<video muted={obj.m()}></video>\n",
        "App.svelte",
    );
    assert!(
        js.contains("video.muted = $0"),
        "the property write reads the memoized slot:\n{js}"
    );
    assert!(
        js.contains("[() => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]"),
        "the property value memoizes wrapped:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

// GUARDRAIL control for the SYNTHESIZED class/style directive OBJECTS: the
// class:/style: asymmetry (class inner RAW, style inner WRAPPED, never a wrap
// around the synthesized object as a whole) is pinned by
// `legacy_class_directive_inner_stays_raw_style_directive_inner_wraps`.

// ── The `createEventDispatcher` legacy component-event surface ──────────────

#[test]
fn legacy_dispatcher_component_emits_push_init_pop_topology() {
    // The oracle topology (svelte@5.56.10): the authored `svelte` import stays in
    // the INSTANCE import slot (after the runtime namespace), the dispatcher
    // declaration + the handler function emit as PLAIN statements, and the
    // imported call sets `needs_context` — `$.push($$props, false)` … `$.init()`
    // … `$.pop()` around the template.
    let js = emit(
        "<script>import { createEventDispatcher } from 'svelte';\nconst dispatch = createEventDispatcher();\nfunction fire() { dispatch('go', 1); }</script>\n<button onclick={fire}>go</button>\n",
        "App.svelte",
    );
    let ns = js
        .find("import * as $ from 'svelte/internal/client';")
        .expect("runtime namespace import");
    let user = js
        .find("import { createEventDispatcher } from 'svelte';")
        .expect("the authored svelte import is preserved");
    assert!(
        user > ns,
        "the instance-slot import follows the runtime namespace import:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, false);"),
        "the legacy frame opens:\n{js}"
    );
    assert!(
        js.contains("const dispatch = createEventDispatcher();"),
        "the dispatcher declaration stays a plain call:\n{js}"
    );
    assert!(
        js.contains("dispatch('go', 1);"),
        "the dispatch call stays plain:\n{js}"
    );
    assert!(js.contains("$.init();"), "the legacy init hook:\n{js}");
    assert!(js.contains("$.pop();"), "the frame closes:\n{js}");
    assert!(
        js.contains("$.delegated('click', button, fire);"),
        "the handler passes by reference:\n{js}"
    );
    // ORDER: dispatcher declaration BEFORE `$.init()` (instance statements first).
    let decl_at = js
        .find("const dispatch = createEventDispatcher();")
        .unwrap();
    let init_at = js.find("$.init();").unwrap();
    assert!(
        decl_at < init_at,
        "instance statements precede $.init():\n{js}"
    );
    // NEGATIVE: the dispatcher is never rewritten to a runtime helper.
    assert!(
        !js.contains("$.createEventDispatcher") && !js.contains("$.dispatch"),
        "dispatcher calls stay plain (no runtime-helper rewrite):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}
