use super::*;

#[test]
fn comment_between_interps_keeps_both_in_one_run() {
    // `{a}<!--x-->{b}` — the comment is dropped, so BOTH interpolations stay in one
    // run with NO space between them. Official emits
    // `\`${$.get(a) ?? ''}${$.get(b) ?? ''}\``. Discriminates the dedup-by-text-var
    // path omitting a later interpolation after a dropped comment.
    let src = "<script>let a = $state(0); let b = $state(0);</script>\n<button onclick={() => {a++;b++}}>{a}<!--x-->{b}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, `${$.get(a) ?? ''}${$.get(b) ?? ''}`)"),
        "both interps stay in one run across a dropped comment:\n{js}"
    );
    // NEGATIVE: a single-interp run (the later interp omitted) must NOT appear.
    assert!(
        !js.contains("$.set_text(text, `${$.get(a) ?? ''}`)"),
        "the second interpolation must not be omitted after the comment:\n{js}"
    );
}

#[test]
fn pure_single_interp_with_trailing_comment_stays_pure() {
    // `{c}<!--x-->` — the trailing comment is dropped, so the run is a PURE single
    // interpolation: `$.set_text(text, $.get(c))`, NOT the `?? ''` mixed form.
    // Preserves the pure-single-vs-mixed distinction across a dropped comment.
    let src =
        "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}<!--x--></button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.set_text(text, $.get(c))"),
        "a trailing comment leaves a pure single interpolation pure:\n{js}"
    );
    // NEGATIVE: a pure single must NOT acquire the mixed `?? ''` form.
    assert!(
        !js.contains("?? ''"),
        "a pure single interpolation must NOT get the mixed `?? ''` form:\n{js}"
    );
}

// ── Static-fragment `$.next()` cursor advance (the official `process_children`
// `skipped` accounting: trailing static positions advance the hydration cursor) ──

#[test]
fn static_no_dynamic_fragment_emits_next_between_clone_and_append() {
    // A STATIC no-dynamic multi-root fragment (`<p>a</p><p>b</p>`) clones the whole
    // fragment but has NO dynamic walk. Official advances the hydration cursor past
    // the static fragment with `$.next()` between the clone frame and `$.append`
    // (`var fragment = root(); $.next(); $.append(...)`). CSR-mount works without it,
    // but hydration records the WRONG end node — a helper-topology divergence.
    // Verified against svelte@5.56.10. RED against the pre-fix walk (which emitted
    // `var fragment = root();` directly followed by `$.append`, no `$.next()`). The
    // `$state` declarator makes it runes-mode (a bare `<p>a</p><p>b</p>` is legacy,
    // refused as legacy mode) but `c` is unused so it stays a pure-static template.
    let src = "<script>let c = $state(0);</script>\n<p>a</p><p>b</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("var fragment = root();") && js.contains("$.next();"),
        "a static no-dynamic fragment must emit `$.next()` after the clone frame:\n{js}"
    );
    // The `$.next()` is BETWEEN the clone frame and the `$.append` (the official
    // cursor-advance order).
    let clone_at = js.find("var fragment = root();").unwrap();
    let next_at = js.find("$.next();").unwrap();
    let append_at = js.find("$.append($$anchor, fragment);").unwrap();
    assert!(
        clone_at < next_at && next_at < append_at,
        "`$.next()` must sit between the clone frame and `$.append`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn static_three_root_fragment_emits_next_with_count() {
    // Three trailing static roots → official emits `$.next(2)` (the `skipped - 1`
    // count, with the literal present when > 1). Verified against svelte@5.56.10.
    // DISCRIMINATING: a builder that always emits a bare `$.next()` (count 1) would
    // record the wrong cursor offset for 3+ static roots.
    let src = "<script>let c = $state(0);</script>\n<p>a</p><p>b</p><p>c</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.next(2);"),
        "three trailing static roots must emit `$.next(2)`:\n{js}"
    );
    assert!(
        !js.contains("$.next();"),
        "the count form (`$.next(2)`) must not also emit a bare `$.next()`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn case_differing_data_attrs_are_not_duplicates_and_both_lowercase() {
    // Two `data-*` attributes that differ ONLY in case (`data-Foo` / `data-foo`) are
    // NOT a duplicate — the official `attribute_duplicate` key is CASE-SENSITIVE on the
    // raw name. Both serialize into the skeleton lowercased, so the cloned HTML carries
    // `data-foo="a" data-foo="b"` (matching official byte-for-byte). This pins the
    // INTERACTION of the case-sensitive duplicate gate with the lowercase serializer.
    let src = "<script>let c = $state(0);</script>\n<div data-Foo=\"a\" data-foo=\"b\"><button onclick={() => c++}>{c}</button></div>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("data-foo=\"a\" data-foo=\"b\""),
        "case-differing data attrs must both lowercase into the skeleton (no duplicate refusal):\n{js}"
    );
    assert!(
        !js.contains("data-Foo"),
        "the mixed-case `data-Foo` must be lowercased in the skeleton:\n{js}"
    );
}

#[test]
fn trailing_static_after_dynamic_fragment_emits_next() {
    // A dynamic node followed by TWO trailing static roots: official walks to the
    // dynamic node, then advances the cursor past the trailing static run with
    // `$.next(2)` (emitted AFTER the dynamic node's reset, BEFORE the text effect).
    // Verified against svelte@5.56.10 (`$.reset(button); $.next(2);`). RED against the
    // pre-fix walk (which emitted no `$.next()` for the trailing static run).
    let src = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button><p>a</p><p>b</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.next(2);"),
        "trailing static run after a dynamic node must emit `$.next(2)`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn static_then_dynamic_fragment_has_no_trailing_next() {
    // NEGATIVE: a static root FOLLOWED BY a dynamic node (no trailing static) emits
    // NO `$.next()` — official walks to the dynamic node via `$.sibling` and the
    // trailing-static `skipped` count is 0. Verified against svelte@5.56.10
    // (`var button = $.sibling($.first_child(fragment));` with no `$.next()`).
    let src =
        "<script>let c = $state(0);</script>\n<p>a</p><button onclick={() => c++}>{c}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        !js.contains("$.next("),
        "a static-then-dynamic fragment (no trailing static) must NOT emit `$.next()`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}
#[test]
fn compound_assign_never_carries_trailing_true() {
    // F9: a COMPOUND assign never proxies (official never adds `, true` to a
    // compound assignment), even for a StateProxy.
    let src = "<script>let o = $state({ a: 1 });</script>\n<button onclick={() => o = o}>{o.a}</button>\n";
    let _ = src;
    // Use a numeric state for an unambiguous compound assign.
    let src2 = "<script>let n = $state(0);</script>\n<button onclick={() => n += 1}>{n}</button>\n";
    let js = emit(src2, "App.svelte");
    assert!(
        js.contains("$.set(n, $.get(n) + 1)"),
        "compound assign lowers to set(get + ...):\n{js}"
    );
    // NEGATIVE: the compound `$.set` carries no proxy-true (an unrelated `, true`
    // walk flag is fine, so key on the `$.set(n, …, true)` shape).
    assert!(
        !js.contains("$.set(n, $.get(n) + 1, true)"),
        "a compound assign must NEVER carry the trailing true:\n{js}"
    );
}

#[test]
fn no_filename_derives_unknown() {
    let alloc = Allocator::default();
    let base = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let parsed = parse_svelte(base);
    let opts = SvelteRuntimeOptions::default();
    let js = compile_client(base, &parsed, &opts, &alloc, false, false)
        .unwrap()
        .code;
    assert!(
        js.contains("export default function _unknown_($$anchor)"),
        "no filename → _unknown_:\n{js}"
    );
}

#[test]
fn debug_tag_no_arguments_logs_empty_object() {
    // A no-argument `{@debug}` logs the empty object (official `console.log({})`), NOT a
    // fail-closed refusal.
    let js = emit(
        "<script>let a = $state(0);</script>\n{@debug}\n<button onclick={() => a++}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("console.log({});"),
        "a no-argument debug logs the empty object:\n{js}"
    );
}

#[test]
fn debug_tag_key_comes_from_parsed_identifier_not_raw_source() {
    // The `{@debug}` object key is the PARSED identifier NAME, not a raw source-text
    // slice. A Unicode-escaped identifier makes the two derivations DIVERGE: the raw
    // source bytes are the six-char escape sequence backslash-u-0-0-6-1 while the
    // parsed `IdentifierReference.name` decodes to `a`. The official object key is the
    // decoded identifier name (`a`); a `source.trim()` derivation would wrongly emit the
    // raw escape sequence as the key. This DISCRIMINATES the typed-fact derivation from
    // the raw-slice one.
    let js = emit(
        "<script>let a = $state(0);</script>\n{@debug \\u0061}\n<button onclick={() => a++}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("console.log({a: $.snapshot("),
        "the debug key must be the PARSED identifier name `a`, not the raw `\\u0061` slice:\n{js}"
    );
    assert!(
        !js.contains("\\u0061:"),
        "the debug key must NOT be the raw `\\u0061` source slice used as an object key:\n{js}"
    );
}

#[test]
fn block_smoke_modules_match_the_committed_jsdom_fixtures() {
    // Each behavioral block-smoke fixture's emitted module stays in lockstep with the
    // committed `.client.mjs` the happy-dom spec (`svelte-client-blocks-smoke.spec.ts`)
    // mounts — so the behavioral smoke can never drift from `compile_client`.
    for (name, src) in BLOCK_SMOKE_FIXTURES {
        assert_jsdom_fixture_in_sync(src, &format!("{name}.client.mjs"));
    }
}

#[test]
fn lifecycle_smoke_modules_match_the_committed_jsdom_fixtures() {
    // Each behavioral lifecycle-smoke fixture's emitted module stays in lockstep with
    // the committed `.client.mjs` the happy-dom spec
    // (`svelte-client-lifecycle-smoke.spec.ts`) mounts — so the behavioral smoke can
    // never drift from `compile_client`.
    for (name, src) in LIFECYCLE_SMOKE_FIXTURES {
        assert_jsdom_fixture_in_sync(src, &format!("{name}.client.mjs"));
    }
}

#[test]
#[ignore = "generator: writes the committed lifecycle smoke fixtures (run once, then oxfmt)"]
fn regen_lifecycle_smoke_fixtures() {
    for (name, src) in LIFECYCLE_SMOKE_FIXTURES {
        let js = emit(src, "App.svelte");
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/svelte-runtime-tests/test/fixtures/svelte")
            .join(format!("{name}.client.mjs"));
        std::fs::write(&path, js).unwrap();
        println!("wrote {}", path.display());
    }
}

#[test]
fn client_breadth_smoke_modules_match_the_committed_runtime_fixtures() {
    for (name, source) in CLIENT_BREADTH_SMOKE_FIXTURES {
        assert_jsdom_fixture_in_sync(source, &format!("{name}.client.mjs"));
    }
}

#[test]
#[ignore = "generator: writes the committed client breadth fixtures (run once, then oxfmt)"]
fn regen_client_breadth_smoke_fixtures() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/svelte-runtime-tests/test/fixtures/svelte");
    for (name, source) in CLIENT_BREADTH_SMOKE_FIXTURES {
        let allocator = Allocator::default();
        let parsed = parse_svelte(source);
        let options = SvelteRuntimeOptions {
            filename: Some("App.svelte".to_string()),
            ..Default::default()
        };
        let module = compile_client(source, &parsed, &options, &allocator, false, false)
            .unwrap_or_else(|error| panic!("client emission failed for {name}: {error:?}"));
        let module_path = fixture_dir.join(format!("{name}.client.mjs"));
        std::fs::write(&module_path, module.code).expect("write client breadth module");
        if *name == "breadth_scoped_css" {
            let css = module.css.expect("external scoped CSS is published");
            std::fs::write(fixture_dir.join("breadth_scoped_css.css"), css.code)
                .expect("write scoped CSS fixture");
        }
        println!("wrote {}", module_path.display());
    }
}

#[test]
fn modifier_stack_wraps_inner_to_outer_in_fixed_order_independent_of_source_order() {
    // A modifier STACK wraps in the FIXED official order (stopPropagation innermost,
    // preventDefault outer) — INDEPENDENT of source order. Both source orderings emit
    // the IDENTICAL nesting `$.preventDefault($.stopPropagation(handler))`.
    let expected =
        nc("$.event('click', button, $.preventDefault($.stopPropagation(() => $.update(n))))");
    for src in [
        "<script>let n = $state(0);</script>\n<button on:click|preventDefault|stopPropagation={() => n++}>x</button>\n",
        "<script>let n = $state(0);</script>\n<button on:click|stopPropagation|preventDefault={() => n++}>x</button>\n",
    ] {
        let js = emit(src, "App.svelte");
        let norm = normalize_js_cosmetics(&js);
        assert!(
            norm.contains(&expected),
            "the modifier stack must wrap inner→outer in fixed order:\n{js}"
        );
        // Negative: the WRONG (source-order) nesting must NOT appear.
        assert!(
            !norm.contains(&nc(
                "$.stopPropagation($.preventDefault(() => $.update(n)))"
            )),
            "the wrapper nesting must not follow source order:\n{js}"
        );
    }
}

#[test]
fn all_modifiers_wrap_in_the_full_fixed_order() {
    // All six wrappers, scrambled in source, emit the full fixed-order nesting:
    // once(trusted(self(preventDefault(stopImmediatePropagation(stopPropagation(h)))))).
    let js = emit(
        "<script>let n = $state(0);</script>\n<button on:click|once|trusted|self|preventDefault|stopImmediatePropagation|stopPropagation={() => n++}>x</button>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc(
            "$.event('click', button, $.once($.trusted($.self($.preventDefault($.stopImmediatePropagation($.stopPropagation(() => $.update(n))))))))"
        )),
        "all modifiers must wrap in the full fixed order:\n{js}"
    );
}

#[test]
fn capture_and_modifier_combine_capture_positional_with_a_wrapper() {
    // `on:click|capture|preventDefault` ⇒ the handler wrapped in `$.preventDefault`
    // AND the 4th positional capture `true`.
    let js = emit(
        "<script>let n = $state(0);</script>\n<button on:click|capture|preventDefault={() => n++}>x</button>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc(
            "$.event('click', button, $.preventDefault(() => $.update(n)), true)"
        )),
        "capture + modifier must combine the wrapper and the capture positional:\n{js}"
    );
}

#[test]
fn modern_touchstart_delegates_with_the_passive_by_default_positional() {
    // A MODERN `ontouchstart` is delegated (touchstart is delegatable) AND passive by
    // default (`is_passive_event`): `$.delegated('touchstart', div, handler, void 0,
    // true)` + the `$.delegate(['touchstart'])` epilogue. Passive applies to the
    // delegated path too.
    let js = emit(
        "<script>let n = $state(0);</script>\n<div ontouchstart={() => n++}>x</div>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc(
            "$.delegated('touchstart', div, () => $.update(n), void 0, true)"
        )),
        "modern touchstart must delegate with the passive-by-default positional:\n{js}"
    );
    assert!(
        js.contains("$.delegate(['touchstart'])"),
        "modern touchstart must register the delegate epilogue:\n{js}"
    );
}

#[test]
fn legacy_touchstart_is_direct_without_a_passive_default() {
    // A LEGACY `on:touchstart` is ALWAYS direct AND derives passive from its modifiers
    // ONLY (it does NOT apply `is_passive_event`): `$.event('touchstart', div,
    // handler)` with NO passive arg. Discriminates the modern-vs-legacy passive rule.
    let js = emit(
        "<script>let n = $state(0);</script>\n<div on:touchstart={() => n++}>x</div>\n",
        "App.svelte",
    );
    let norm = normalize_js_cosmetics(&js);
    assert!(
        norm.contains(&nc("$.event('touchstart', div, () => $.update(n))")),
        "legacy touchstart must be a direct $.event with no passive arg:\n{js}"
    );
    // Negative: no passive default, no delegation.
    assert!(
        !norm.contains(&nc("void 0")) && !js.contains("$.delegated("),
        "legacy touchstart must not apply the modern passive default or delegate:\n{js}"
    );
}

#[test]
fn unused_uninitialized_bare_local_is_preserved() {
    // NEGATIVE control for the uninit-plain-local DOM-bind widening: an UNUSED bare
    // `let unused;` that is NOT a bind-target lvalue root stays fail-closed at the
    // instance-script-item gate (construct `unused bare let`). The no-init admission is
    // gated on the bind-lvalue-root set, so a bare local that nothing binds is still
    // refused (it is not the `bind:this` clone-root nor a DOM-bind target). RED would be
    // a wildcard "admit any no-init let".
    let js = emit(
        "<script>let unused; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let unused;"),
        "ordinary bare local missing:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn bare_import_read_is_live_and_frame_free() {
    // A BARE imported-ident read `{x}` is LIVE — it joins the region's
    // `$.template_effect`, read PLAIN — and does NOT open the context frame
    // (official: only a member/call rooted at an import frames).
    let js = emit(
        "<script>import { x } from './m.js'; let c = $state(0);</script>\n<p>{x}</p>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.set_text(text, x)"),
        "the bare import read must be a live plain read inside the template effect:\n{js}"
    );
    assert!(
        js.contains("$.template_effect"),
        "the import read must join a template effect (never static-folded):\n{js}"
    );
    // NEGATIVES: never `$.get`, never a static `textContent` fold, and NO frame.
    assert!(
        !js.contains("$.get(x)"),
        "an import read must stay plain (never `$.get`):\n{js}"
    );
    assert!(
        !js.contains("textContent"),
        "an import read must not static-fold:\n{js}"
    );
    assert!(
        !js.contains("$.push"),
        "a bare import read must NOT open the context frame:\n{js}"
    );
}

#[test]
fn instance_namespace_member_read_frames_and_stays_plain() {
    // An INSTANCE-slot namespace MEMBER read `{NS.z}` is live + plain AND opens the
    // context frame (`$.push($$props, true)` / `$.pop()` + the `$$props` param).
    let js = emit(
        "<script>import * as NS from './m.js'; let c = $state(0);</script>\n<p>{NS.z}</p>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.set_text(text, NS.z)"),
        "the member read must stay the plain member expression:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true)") && js.contains("$.pop()"),
        "the imported-member read must open the context frame:\n{js}"
    );
}

#[test]
fn computed_optional_and_call_member_interpolations_use_the_shared_value_carrier() {
    for (label, interp) in [
        ("computed", "{NS['z']}"),
        ("optional", "{NS?.z}"),
        ("call", "{NS.z()}"),
    ] {
        let src = format!(
            "<script>import * as NS from './m.js'; let c = $state(0);</script>\n<p>{interp}</p>\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        let js = emit(&src, &format!("{label}.svelte"));
        assert!(
            js.contains("NS") && js.contains("$.set_text("),
            "[{label}] the accepted member family must emit a text update:\n{js}"
        );
        assert!(
            parses_as_js(&js),
            "[{label}] output must be valid JS:\n{js}"
        );
    }
    let js = emit(
        "<script>let d = 1; let c = $state(0);</script>\n<p>{d.x}</p>\n<button onclick={() => c++}>{c}</button>\n",
        "plain-local.svelte",
    );
    assert!(
        js.contains("$.set_text(text, d.x)"),
        "a plain-local member read uses the same live carrier:\n{js}"
    );
    assert!(parses_as_js(&js));
}

#[test]
fn svelte_self_special_emits_a_recursive_call() {
    // `<svelte:self>` emits a recursive call through the component's COMPILE-NAME (the
    // filename-derived `App` here), NOT a `$.component` (it is a static self-reference).
    let js = emit_result(
        "<script>let { depth } = $props();</script>\n{#if depth > 0}<svelte:self depth={depth - 1} />{/if}\n",
    )
    .expect("svelte:self emits a module");
    // The fixture compiles under `App.svelte` (the test harness filename) → callee `App`.
    assert!(
        js.contains("App(node"),
        "missing the recursive svelte:self call through the compile-name:\n{js}"
    );
    // NEGATIVE: svelte:self is a STATIC self-reference, NOT a dynamic `$.component`.
    assert!(
        !js.contains("$.component("),
        "svelte:self must not route through $.component:\n{js}"
    );
}

#[test]
fn svelte_boundary_plain_emits_comment_anchored_boundary_call() {
    // `<svelte:boundary><p>{x}</p></svelte:boundary>` → the comment-anchor frame + `$.boundary(
    // node, {}, ($$anchor) => { <body> })` + `$.append`. Empty props, NO wrapping block.
    let js = emit(
        "<script>let { x } = $props();</script>\n<svelte:boundary><p>{x}</p></svelte:boundary>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("var fragment = $.comment();")),
        "comment anchor:\n{js}"
    );
    assert!(
        n.contains(&nc("$.boundary(node, {}, ($$anchor) =>")),
        "boundary call with empty props:\n{js}"
    );
    assert!(
        n.contains(&nc("$.append($$anchor, fragment);")),
        "mount:\n{js}"
    );
    // NEGATIVE: no wrapping block (no hoisted snippet), no $.event for a (absent) onerror.
    assert!(
        !js.contains("$.event("),
        "no $.event for the boundary:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_head_title_meta_places_title_before_append() {
    // A title + a SINGLE meta → the meta rides the `from_html` region INSIDE the callback and the
    // title effect (after_update) sits BETWEEN the meta clone and its `$.append`. A single meta
    // (single root) ⇒ NO `$.next()`.
    let js = emit(
        "<script>\n\tlet { t } = $props();\n</script>\n\n<svelte:head>\n\t<title>{t}</title>\n\t<meta name=\"x\" content=\"y\">\n</svelte:head>\n",
        "special/svelte_head_title_meta.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.from_html(`<meta name=\"x\" content=\"y\"/>`)")),
        "single-root meta template:\n{js}"
    );
    // The title effect emits AFTER the clone and BEFORE the append (the after_update slot).
    assert!(
        n.contains(&nc(
            "var meta = root(); $.deferred_template_effect(() => {$.document.title = $$props.t ?? '';}); $.append($$anchor, meta);"
        )),
        "title effect between the meta clone and its append:\n{js}"
    );
    // NEGATIVE: a single meta root does NOT advance the fragment cursor.
    assert!(!js.contains("$.next("), "single meta ⇒ no $.next():\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_head_meta_only_emits_fragment_next_append() {
    // A meta-only head with TWO roots (meta + link, whitespace between) → `var fragment = root();
    // $.next(2); $.append($$anchor, fragment);` in the callback, with NO title effect.
    let js = emit(
        "<script>\n\tlet { locale } = $props();\n</script>\n\n<svelte:head>\n\t<meta name=\"x\" content=\"y\">\n\t<link rel=\"stylesheet\" href=\"a.css\">\n</svelte:head>\n",
        "special/svelte_head_meta.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.from_html(`<meta name=\"x\" content=\"y\"/> <link rel=\"stylesheet\" href=\"a.css\"/>`, 1)"
        )),
        "multi-root meta+link template with the fragment flag:\n{js}"
    );
    assert!(
        n.contains(&nc(
            "$.head('w8zktq', ($$anchor) => {var fragment = root(); $.next(2); $.append($$anchor, fragment);})"
        )),
        "meta-only callback body (fragment + $.next(2) + append):\n{js}"
    );
    // NEGATIVE: no title effect for a meta-only head.
    assert!(
        !js.contains("$.document.title"),
        "no title write for a meta-only head:\n{js}"
    );
    assert!(
        !js.contains("effect(") && !js.contains("deferred_template_effect"),
        "no title effect for a meta-only head:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_head_body_sibling_emits_head_at_source_position() {
    // `<svelte:head>…</svelte:head><p>hi</p>` → the sibling `<p>` clones its own `from_html`
    // region and the `$.head(...)` emits at its SOURCE position (before the `<p>` sibling's
    // `$.append`). The head itself is excluded from the body skeleton (no `<!>` anchor).
    let js = emit(
        "<script>\n\tlet { t } = $props();\n</script>\n\n<svelte:head>\n\t<title>{t}</title>\n</svelte:head>\n\n<p>hi</p>\n",
        "special/svelte_head_body_sibling.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.from_html(`<p>hi</p>`)")),
        "sibling `<p>` template:\n{js}"
    );
    // The head sits at its source position: after the `<p>` clone, before the append.
    assert!(
        n.contains(&nc(
            "var p = root(); $.head('rlmige', ($$anchor) => {$.deferred_template_effect(() => {$.document.title = $$props.t ?? '';});}); $.append($$anchor, p);"
        )),
        "head op at source position (between the sibling clone and its append):\n{js}"
    );
    // NEGATIVE: the head is NOT a comment-anchored body node (no `<!>` skeleton for it).
    assert!(
        !js.contains("$.comment()"),
        "head is excluded from the body skeleton (no comment anchor):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn inspect_standalone_elided() {
    // A top-level `$inspect(x);` statement is production-ELIDED: official
    // `svelte@5.56.10` (`dev:false`) removes the whole statement (leaving only a
    // cosmetic `;;` empty-statement residue). Verter emits NO helper, NO import,
    // NO dev form, and the statement forces NO component frame — the signature
    // stays `App($$anchor)`. RED against the pre-elision fail-closed arm
    // (`AdvancedRune { rune: "$inspect" }`).
    let js = emit(
        "<script>let c = $state(0); $inspect(c);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("export default function App($$anchor) {"),
        "a plain `$inspect(x);` must NOT force the component frame:\n{js}"
    );
    // NEGATIVES: no `$$props` param, no push/pop frame, no inspect token of ANY
    // form (`$inspect`, `$.inspect(`, an inspect import).
    assert!(
        !js.contains("$$props"),
        "no `$$props` threading for a propless component with `$inspect`:\n{js}"
    );
    assert!(!js.contains("$.push"), "no `$.push` frame:\n{js}");
    assert!(!js.contains("$.pop"), "no `$.pop` frame:\n{js}");
    assert!(
        !js.contains("inspect"),
        "no inspect token of any form:\n{js}"
    );
    // The surrounding script still lowers (state decl + handler preserved).
    assert!(js.contains("let c = $.state(0);"), "state decl:\n{js}");
    assert!(
        js.contains("$.update(c)"),
        "onclick update preserved:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn inspect_with_elided_forces_frame() {
    // `$inspect(x).with(fn);` — the chain is ELIDED like the plain form, BUT the
    // `.with(...)` FORCES the component frame in official production output:
    // `App($$anchor, $$props)` + `$.push($$props, true)` first + `$.pop()` last
    // (verified first-hand against svelte@5.56.10, even for an empty `() => {}`
    // callback). RED against the pre-elision fail-closed arm.
    let js = emit(
        "<script>let c = $state(0); $inspect(c).with(console.log);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "`.with` forces the `$$props` param:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "`.with` forces the runes push frame:\n{js}"
    );
    assert!(js.contains("$.pop();"), "`.with` forces the pop:\n{js}");
    // NEGATIVES: the chain (including its callback argument) is fully elided —
    // no helper call, no import, no inspect token.
    assert!(
        !js.contains("inspect"),
        "no inspect token of any form:\n{js}"
    );
    assert!(
        !js.contains("console.log"),
        "the `.with` callback is elided with the chain:\n{js}"
    );
    assert!(js.contains("let c = $.state(0);"), "state decl:\n{js}");
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn inspect_trace_param_shadow_local_call_survives() {
    // A `$inspect` PARAMETER shadows the rune: `$inspect.trace()` under it is an
    // ORDINARY local method call. Official svelte@5.56.10 ACCEPTS
    // `($inspect) => { c++; $inspect.trace(); }` and EMITS the call FAITHFULLY
    // (oracle-verified: `.trace(` survives in the official output). The
    // scope-aware pipeline must NOT drop it as a production-elided rune trace, and must
    // NOT reject it as a misplaced rune trace. `onfocus` is a NON-delegated (DIRECT)
    // event, so the arrow lowers through the shared expression rewriter. RED before the
    // fix: the placement scan false-rejected the whole component (`emit` panics).
    let js = emit(
        "<script>let c = $state(0);</script>\n<button onfocus={($inspect) => { c++; $inspect.trace(); }}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$inspect.trace()"),
        "the param-shadowed local `$inspect.trace()` must survive (not dropped):\n{js}"
    );
    assert!(
        js.contains("$.update(c)"),
        "the rest of the handler body still lowers:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn group_single_value_provably_defined_omits_outer_coalesce() {
    // A `bind:group` SINGLE value whose expression is PROVABLY DEFINED omits the outer
    // `?? ''` coercion — official svelte@5.56.10 gates the coercion on `evaluated.is_defined`,
    // NOT on single-vs-mixed. Oracle-verified (svelte@5.56.10) over the SUPPORTED DOM-bind value
    // sources: a demoted `$state(5)`, a literal `5`, and a literal `false` all emit
    // `input.value = input.__value = V;` (NO outer `?? ''`). (A bare `let n = 5` is an
    // unsupported instance-script item in the DOM-bind backend, so a demoted `$state` is the identifier vehicle.)
    // RED before the fix: every `AttrValue::Single` group value emitted the inert
    // `(input.__value = V) ?? ''` regardless of definedness.
    let cases = [
        // A never-reassigned `$state(5)` demotes to a plain local whose initializer the
        // evaluator proves defined (the existing `mixed_chunk_nullish_wrap` demoted-$state path).
        (
            "let n = $state(5);",
            "n",
            "input.value = input.__value = n;",
        ),
        // A literal number / boolean is trivially provably defined — no declaration needed.
        ("", "5", "input.value = input.__value = 5;"),
        ("", "false", "input.value = input.__value = false;"),
    ];
    for (decl, value, expected) in cases {
        let src = format!(
            "<script>let sel = $state(\"\"); {decl}</script>\n<input type=\"radio\" bind:group={{sel}} value={{{value}}} />\n"
        );
        let js = emit(&src, "App.svelte");
        assert!(
            js.contains(expected),
            "a provably-defined single group value must emit `{expected}` (no outer `?? ''`):\n{js}"
        );
        assert!(
            !js.contains(&format!("(input.__value = {value}) ?? ''")),
            "a provably-defined single group value must NOT carry the inert outer `?? ''`:\n{js}"
        );
    }
}

#[test]
fn group_single_value_not_provably_defined_keeps_outer_coalesce() {
    // NEGATIVE CONTROL: a `bind:group` SINGLE value that is NOT provably defined KEEPS the
    // outer `?? ''` (official keeps it for a null / undefined / reactive value). Oracle-verified
    // (svelte@5.56.10) over SUPPORTED DOM-bind value sources: a literal `null` emits
    // `input.value = (input.__value = null) ?? '';`, and a demoted `$state(null)` emits
    // `input.value = (input.__value = n) ?? '';`. This guards against over-suppression — GREEN
    // before AND after the fix (a control that the definedness gate is not blanket-applied).
    // (The reactive `$.get(...)` single case keeps `?? ''` too — pinned by the
    // `bind_group_radio_dynamic` golden.)
    let cases = [
        ("", "null", "(input.__value = null) ?? ''"),
        ("let n = $state(null);", "n", "(input.__value = n) ?? ''"),
    ];
    for (decl, value, expected) in cases {
        let src = format!(
            "<script>let sel = $state(\"\"); {decl}</script>\n<input type=\"radio\" bind:group={{sel}} value={{{value}}} />\n"
        );
        let js = emit(&src, "App.svelte");
        assert!(
            js.contains(expected),
            "a not-provably-defined single group value (`{value}`) must keep the outer `?? ''`:\n{js}"
        );
    }
}

#[test]
fn autofocus_dynamic_emits_init_only_autofocus_helper() {
    // `autofocus={v}` → init-only `$.autofocus(input, $.get(v))` — NOT a template_effect.
    let src =
        "<script>let v = $state(true);</script>\n<input onclick={() => v = !v} autofocus={v}>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.autofocus(input, $.get(v))")),
        "`autofocus={{v}}` must emit the init-only autofocus helper:\n{js}"
    );
    // NEGATIVE: autofocus is NOT wrapped in a template_effect and is NOT a property.
    assert!(
        !n.contains(&nc("template_effect(() => $.autofocus")) && !n.contains("input.autofocus="),
        "`autofocus` is init-only, not reactive / not a property:\n{js}"
    );
}

#[test]
fn autofocus_static_valueless_emits_autofocus_true() {
    // A static valueless `autofocus` → `$.autofocus(input, true)` init, and is NOT
    // baked into the from_html skeleton.
    let src = "<script>let c = $state(0);</script>\n<input autofocus>\n<button onclick={() => c++}>{c}</button>\n";
    let js = emit(src, "App.svelte");
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.autofocus(input, true)")),
        "a static valueless autofocus must emit `$.autofocus(input, true)`:\n{js}"
    );
    // NEGATIVE: autofocus is never in the cloned skeleton.
    assert!(
        !js.contains("$.from_html(`<input autofocus"),
        "autofocus must NOT be baked into the from_html skeleton:\n{js}"
    );
}

// ─── after-update rank completeness + fail-loud unranked-op invariant ───

#[test]
fn every_after_update_op_target_is_ranked_across_representative_scopes() {
    // The RANKING-COMPLETENESS invariant: every op the after-update stream sorts —
    // a non-init-domain lifecycle op (`$.transition` / `$.animation`), a modern
    // `on*` event with a Node target, a bind op — targets a node the Euler-tour
    // rank map covers, ACROSS region-body scopes (an `{#each}` body, a `{#snippet}`
    // body, a NAMED-slot region, a `<svelte:boundary>` body) as well as the root.
    // A miss would silently tail-sort the op (the retired `u32::MAX` fallback) —
    // now a hard error at emit, and a coverage failure here.
    use super::super::super::client_lifecycle;
    use super::super::super::client_plan::ClientRuntimeOp;
    use super::super::super::client_plan_types::EventEmitTarget;
    use super::super::super::ir::{NodeId, TemplateScopeId};
    for (label, source) in [
        (
            "root elements",
            "<script>let c = $state(0);</script>\n<div transition:fade><button onclick={() => c++}>{c}</button></div>\n",
        ),
        (
            "each body",
            "<script>let { items } = $props(); let c = $state(0);</script>\n{#each items as item}<button onclick={() => c++}>{item}</button>{/each}\n",
        ),
        (
            "snippet body + render",
            "<script>let c = $state(0);</script>\n{#snippet row(a)}<button onclick={() => c++}>{a}</button>{/snippet}\n{@render row(1)}\n",
        ),
        (
            "named-slot region",
            "<script>import Child from './Child.svelte'; let c = $state(0);</script>\n<Child><span slot=\"a\"><button onclick={() => c++}>{c}</button></span></Child>\n",
        ),
        (
            "boundary body",
            "<script>let c = $state(0);</script>\n<svelte:boundary onerror={() => {}}><button onclick={() => c++}>{c}</button></svelte:boundary>\n",
        ),
    ] {
        let alloc = Allocator::default();
        let parsed = parse_svelte(source);
        let opts = SvelteRuntimeOptions {
            filename: Some("App.svelte".to_string()),
            ..Default::default()
        };
        let ir = crate::svelte::runtime::lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc)
            .unwrap_or_else(|e| panic!("[{label}] lowering: {e:?}"));
        let classified = super::super::super::client_surface::ClientSyntaxSurface::classify(&ir)
            .unwrap_or_else(|e| panic!("[{label}] classify: {e:?}"));
        let plan = super::super::super::client_plan::SupportedClientIr::build(&classified, &ir, None)
            .unwrap_or_else(|e| panic!("[{label}] plan build: {e:?}"));
        let ranks = client_lifecycle::after_update_ranks(&plan);
        let mut streamed_ops = 0usize;
        for idx in 0..plan.build.ir.template_scopes.len() {
            let scope_id = TemplateScopeId(idx as u32);
            for op in plan.ops_in(scope_id) {
                let target = match op {
                    ClientRuntimeOp::Lifecycle(l) if !l.is_init_domain() => Some(l.target()),
                    ClientRuntimeOp::Event { emit, .. } => match emit.target {
                        EventEmitTarget::Node(id) => Some(id),
                        _ => None,
                    },
                    ClientRuntimeOp::Bind { target, .. } => Some(*target),
                    _ => None,
                };
                if let Some(t) = target {
                    streamed_ops += 1;
                    assert!(
                        ranks.contains_key(&NodeId(t.0)),
                        "[{label}] after-update op target node {} must be ranked",
                        t.0
                    );
                }
            }
        }
        assert!(
            streamed_ops > 0,
            "[{label}] the fixture must exercise at least one rankable op"
        );
    }
}

#[test]
fn ranked_lookup_returns_the_assigned_rank() {
    // The fail-loud lookup returns the ASSIGNED rank for a present node.
    use super::super::super::client_lifecycle::{require_after_update_rank, AfterUpdateRank};
    use super::super::super::ir::NodeId;
    let mut map = rustc_hash::FxHashMap::default();
    map.insert(NodeId(3), AfterUpdateRank { pre: 4, post: 9 });
    let rank = require_after_update_rank(&map, NodeId(3));
    assert_eq!((rank.pre, rank.post), (4, 9));
}

#[test]
#[should_panic(expected = "after-update op target not ranked")]
fn directive_batch_emit_panics_loudly_when_its_target_rank_is_missing() {
    // The EXIT-rank half: a bare LEGACY `on:` event joins the directive batch at
    // `after_update_post_rank` (`client_emit.rs`) — the fixture's SOLE streamed op,
    // so no pre-rank lookup can mask a reverted post call site. Same discrimination
    // as the pre-rank test — reverting `after_update_post_rank` to
    // `.unwrap_or(u32::MAX)` makes this test fail (no panic).
    let _ = emit_with_after_update_ranks_cleared(
        "<script>let c = $state(0);</script>\n<div on:click={() => c++}>{c}</div>\n",
    );
}

#[test]
fn legacy_export_let_sibling_default_lowers_lazy_getter_carrier() {
    // `export let a = 1; export let b = a;` — a legacy default referencing a
    // SIBLING prop is part of the prop DECLARATION (exactly as the runes
    // `let { a = 1, b = a } = $props()` destructure default), never an
    // instance-script prop usage. The sibling read rewrites to the getter and
    // collapses to the BARE getter as the LAZY carrier: flags 24 (BINDABLE 8 |
    // LAZY 16). Verified against svelte@5.56.10.
    let result = emit_result(
        "<script>\nexport let a = 1;\nexport let b = a;\n</script>\n<p>{a}</p>\n<p>{b}</p>\n",
    );
    let js = match result {
        Ok(js) => js,
        Err(e) => {
            panic!("a sibling-default legacy prop must compile (not a prop-usage refusal): {e:?}")
        }
    };
    assert!(
        js.contains("let a = $.prop($$props, 'a', 8, 1);"),
        "the literal-default sibling stays the eager flag-8 legacy prop source:\n{js}"
    );
    assert!(
        js.contains("let b = $.prop($$props, 'b', 24, a);"),
        "the sibling-reference default is the LAZY (24 = 8 | 16) bare-getter carrier:\n{js}"
    );
    // NEGATIVE: the carrier is the bare getter — never a thunk over the getter
    // call, never the raw call, and never a `$$props` member thunk.
    assert!(
        !js.contains("24, () => a") && !js.contains("24, a()") && !js.contains("() => $$props.a"),
        "the zero-arg getter call collapses to the bare getter:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn export_specifier_list_stays_the_export_residual() {
    // `export { a };` (official: an aliased prop surface) is OUT of the
    // export-let scope — the generic export construct refusal owns it. (The
    // export statement comes FIRST so the item gate reaches it before the
    // sibling plain-let refusal.)
    assert_fail_closed(
        "<script>export { a };\nlet a = 1;</script>\n<p>hi</p>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
                    construct: "export",
                    ..
                }
            )
        },
    );
}

#[test]
fn legacy_unwritten_let_is_preserved_without_promotion() {
    // DEMAND-DRIVEN promotion: a plain legacy `let` that is neither written nor
    // a bind target stays the fail-closed plain-let refusal — promotion never
    // becomes a blanket "accept every top-level let".
    let js = emit(
        "<script>let unused = 5;</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let unused = 5;"),
        "ordinary legacy local missing:\n{js}"
    );
    assert!(
        !js.contains("mutable_source"),
        "unwritten local was promoted:\n{js}"
    );
    // A mutating METHOD CALL (`arr.push(2)`) is NOT a promotion seed — official
    // keeps the let verbatim-plain (no mutable_source), so Verter keeps the
    // fail-closed refusal rather than over-promoting. (The write rides an
    // ADMITTED handler-referent function body so the item gate — not the
    // narrow inline-handler gate — owns the refusal; an over-promoting
    // implementation would compile this component.)
    let js = emit(
        "<script>let arr = [1];\nfunction f() { arr.push(2); }</script>\n<button onclick={f}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let arr = [1];"),
        "ordinary legacy local missing:\n{js}"
    );
    assert!(js.contains("arr.push(2);"), "method call missing:\n{js}");
    assert!(
        !js.contains("mutable_source"),
        "method call over-promoted local:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

#[test]
fn legacy_nested_arrow_only_read_without_trigger_stays_raw() {
    // NEGATIVE control: an expression whose ONLY reactive read lives inside a
    // nested arrow, with NO sync call/member/assignment, never fires the wrap
    // trigger — the deferred read is not a sync trigger even though it would be
    // a dep if some other part triggered. Oracle: `foo: () => b().y` (a plain
    // init — no `$.untrack`, no `$.deep_read_state`).
    let js = emit(
        "<script>import Child from './Child.svelte';\nexport let b;</script>\n<Child foo={() => b.y} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("foo: () => b().y"),
        "the deferred-only value stays a plain prop init:\n{js}"
    );
    assert!(
        !js.contains("$.untrack("),
        "no wrap without a sync trigger:\n{js}"
    );
    assert!(
        !js.contains("$.deep_read_state("),
        "no dep reads without a wrap:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_import_zero_arg_call_untracks_by_reference() {
    // The official `b.thunk` unthunk inside the wrap: a zero-arg identifier
    // call (`fn()`) untracks BY REFERENCE (`$.untrack(fn)`), and the imported
    // callee joins the deps as a `$.deep_read_state` live read (oracle):
    //   let $0 = $.derived_safe_equal(() => ($.deep_read_state(fn), $.untrack(fn)));
    let js = emit(
        "<script>export let obj;\nimport { fn } from './x.js';</script>\n<div><slot foo={fn()} bar={obj.x} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let $0 = $.derived_safe_equal(() => ($.deep_read_state(fn), $.untrack(fn)));"),
        "the zero-arg call untracks by reference (import dep deep-read):\n{js}"
    );
    assert!(
        !js.contains("$.untrack(() => fn())"),
        "no redundant thunk around the bare callee:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_global_member_value_wraps_untracked_with_empty_deps() {
    // A GLOBAL-rooted member (`Math.PI`) still sets the official
    // `has_member_expression` trigger: the init value wraps in `$.untrack`
    // with NO dependency reads (BA-shape oracle): `foo: ($.untrack(() => Math.PI))`.
    let js = emit(
        "<script>export let obj;</script>\n<div><slot foo={Math.PI} bar={obj.x} /></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("foo: ($.untrack(() => Math.PI))"),
        "the global member init wraps untracked with empty deps:\n{js}"
    );
    assert!(
        js.contains("get bar() { return ($.deep_read_state(obj()), $.untrack(() => obj().x)); }"),
        "the sibling prop-rooted member wraps with its dep:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_autofocus_call_value_wraps_untracked_in_init() {
    // The init-only `$.autofocus` value ALSO wraps (official applies
    // `build_expression` at expression-build time, not per effect) — oracle:
    //   $.autofocus(input, ($.deep_read_state(obj()), $.untrack(() => obj().m())));
    let js = emit(
        "<script>export let obj;</script>\n<input autofocus={obj.m()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.autofocus(input, ($.deep_read_state(obj()), $.untrack(() => obj().m())))"),
        "the autofocus init value wraps inline:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn unused_dispatcher_import_emits_no_frame_and_no_init() {
    // An UNUSED `createEventDispatcher` import: official preserves the import and
    // emits NO frame, NO `$.init()`, and NO `$$props` param.
    let js = emit(
        "<script>import { createEventDispatcher } from 'svelte';</script>\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("import { createEventDispatcher } from 'svelte';"),
        "the unused import is preserved:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {"),
        "no $$props param without a trigger:\n{js}"
    );
    assert!(
        !js.contains("$.push") && !js.contains("$.init()") && !js.contains("$.pop()"),
        "an unused import forces no frame/init:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_attach_call_wraps_thunk() {
    // Oracle: $.attach(div, () => ($.deep_read_state(obj()), $.untrack(() => obj().m())));
    let js = emit(
        &format!("{LEGACY_OBJ}<div {{@attach obj.m()}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("$.attach(div, () => ({}))", obj_wrap("obj().m()"))),
        "the attachment payload wraps inside its thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_attach_member_wraps_thunk() {
    // Oracle: $.attach(div, () => ($.deep_read_state(obj()), $.untrack(() => obj().x)));
    let js = emit(
        &format!("{LEGACY_OBJ}<div {{@attach obj.x}}></div>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("$.attach(div, () => ({}))", obj_wrap("obj().x"))),
        "the member attachment payload wraps inside its thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_title_call_memoizes_wrapped_deferred() {
    // Oracle: $.deferred_template_effect(($0) => { $.document.title = $0 ?? ''; },
    //   [() => ($.deep_read_state(obj()), $.untrack(() => obj().m()))]);
    let js = emit(
        &format!("{LEGACY_OBJ}<svelte:head><title>{{obj.m()}}</title></svelte:head>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!("[() => ({})]", obj_wrap("obj().m()"))),
        "the call-bearing title chunk memoizes wrapped:\n{js}"
    );
    assert!(
        js.contains("$.document.title = $0 ?? ''"),
        "the title RHS reads the opaque memo slot:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_title_member_wraps_inline_coalesced() {
    // Oracle: $.deferred_template_effect(() => { $.document.title = ($.deep_read_state(obj()), $.untrack(() => obj().x)) ?? ''; });
    let js = emit(
        &format!("{LEGACY_OBJ}<svelte:head><title>{{obj.x}}</title></svelte:head>\n"),
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "$.document.title = ({}) ?? ''",
            obj_wrap("obj().x")
        )),
        "the member title chunk wraps inline with the bare coalesce:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_title_mixed_chunk_memoizes_wrapped() {
    // Oracle: $.document.title = `page ${$0 ?? ''}`; deps [() => (wrap)]
    let js = emit(
        &format!("{LEGACY_OBJ}<svelte:head><title>page {{obj.m()}}</title></svelte:head>\n"),
        "App.svelte",
    );
    assert!(
        js.contains("$.document.title = `page ${$0 ?? ''}`"),
        "the mixed title chunk memoizes into the template slot:\n{js}"
    );
    assert!(
        js.contains(&format!("[() => ({})]", obj_wrap("obj().m()"))),
        "the memoized title dep is the wrapped sequence:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_mixed_wrapped_defined_chunk_keeps_bare_coalesce() {
    // A mixed-attr chunk whose AUTHOR expression is PROVABLY DEFINED (a
    // template literal always evaluates to a string) but legacy-WRAPPED (the
    // `p.a` member trigger): official never proves the WRAPPED sequence
    // defined, so the bare `?? ''` coalesce still applies — the definedness of
    // the authored expression must NOT leak through the wrap and elide it.
    let js = emit(
        "<script>export let p;</script>\n<div title=\"a {`x${p.a}`} b\"></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("?? ''"),
        "the wrapped defined chunk keeps the bare `?? ''` coalesce:\n{js}"
    );
    assert!(
        js.contains("$.untrack("),
        "the member-trigger chunk wraps in definite legacy:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}
