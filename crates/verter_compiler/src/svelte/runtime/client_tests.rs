//! Integration tests for the Svelte client (`svelte/internal/client`) emission.
//!
//! These drive the full pipeline (parse → lower → plan → topology → emit) and pin
//! the emitted-JS shape against the official `svelte@5.56.3` output captured via
//! the oracle. Each test is discriminating with negative assertions; the
//! fail-closed family asserts the precise typed surface + diagnostic id (never a
//! silent empty module, never a panic).

use oxc_allocator::Allocator;

use crate::svelte::parser::parse_svelte;
use crate::svelte::runtime::client::UnsupportedSvelteRuntimeSurface;
use crate::svelte::runtime::{
    compile_client, ClientCompileError, CoreOfficialValidationRule, SvelteRuntimeOptions,
};

/// Compile a Svelte source to its client JS, panicking on a lowering/unsupported
/// error (for the SUPPORTED fixtures).
fn emit(source: &str, filename: &str) -> String {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some(filename.to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false)
        .unwrap_or_else(|e| panic!("client emission failed for {filename}: {e:?}"))
        .code
}

/// Compile returning the `Result`, so a fail-closed test can assert the typed
/// surface.
fn emit_result(source: &str) -> Result<String, ClientCompileError> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false).map(|m| m.code)
}

/// The §1.2 conformance fixture.
const HELLO_INPUT: &str = "<script>\n\tlet name = $state('world');\n\tlet count = $state(0);\n</script>\n\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n";

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
    // true)`. Verified against svelte@5.56.3.
    assert!(
        js.contains("$.child(button, true)"),
        "a pure-interp text child carries the is_text flag:\n{js}"
    );
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
    // text run. Official svelte@5.56.3 emits `\`a ${$.get(c) ?? ''} b\``. RED
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
    // flag — `$.child(p)`. Verified against svelte@5.56.3 (the §1.2 `<h1>Hello
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
    // Verified against svelte@5.56.3. The static sibling is an allowlisted `<p>` (a
    // `<span>` is out of the §1.2 element allowlist).
    let src = "<script>let count = $state(0);</script>\n<div><p></p>{count}</div>\n<button onclick={() => count++}>z</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.sibling($.child(div), 1, true)"),
        "sibling-to-pure-interp-text forces explicit offset + is_text:\n{js}"
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
    // Verified against svelte@5.56.3. RED against the pre-fix walk (which emitted
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
    // count, with the literal present when > 1). Verified against svelte@5.56.3.
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
fn root_leading_text_before_dynamic_emits_pre_clone_next() {
    // CODEGEN BUG A: a ROOT-level leading static TEXT before the first named dynamic
    // position (`x<button onclick={() => c++}>{c}</button>`) is the official
    // `is_text_first` case — official emits a PRE-CLONE `$.next();` BEFORE
    // `var fragment = root();` (skipping the inserted leading anchor), then descends to
    // the button via `$.sibling($.first_child(fragment))`. Verified against
    // svelte@5.56.3. RED against the pre-fix emitter (which cloned first with NO
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
fn multi_root_fragment_local_collides_with_user_binding_renames_to_fragment_1() {
    // A user binding NAMED `fragment` in the instance script collides with the
    // synthesized multi-root clone-frame local `var fragment = root();` — two
    // declarations of `fragment` in one function scope is INVALID JS (a `SyntaxError`).
    // The official compiler routes every synthesized DOM local through a
    // collision-aware allocator (`scope.generate`) SEEDED with the user-script's
    // top-level binding names, so the clone frame becomes `var fragment_1 = root();`.
    // RED against the pre-fix tree (which hard-coded `var fragment = root();`).
    // The colliding `fragment` is a supported `$state` signal (shape-1); the allocator
    // seeds from EVERY top-level declared name, so a signal binding triggers the rename.
    let src = "<svelte:options runes={true}/>\n<script>let fragment = $state(0);</script>\n<button onclick={() => fragment++}>a</button><p>{fragment}</p>\n";
    let js = emit(src, "App.svelte");
    // The synthesized clone-frame local is renamed to avoid the user `fragment`.
    assert!(
        js.contains("var fragment_1 = root();"),
        "a user `let fragment` must push the synthesized clone local to `fragment_1`:\n{js}"
    );
    // NEGATIVE: the un-suffixed `var fragment = root();` must NOT be emitted (the
    // collision that produced invalid JS).
    assert!(
        !js.contains("var fragment = root();"),
        "the synthesized clone local must not collide with the user `fragment`:\n{js}"
    );
    // The user's own `let fragment = $.state(0);` declaration is preserved.
    assert!(
        js.contains("let fragment = $.state(0);"),
        "the user signal binding `let fragment = $.state(0);` must be preserved:\n{js}"
    );
    // The mount + walk reference the RENAMED region var, not the bare `fragment`.
    assert!(
        js.contains("$.append($$anchor, fragment_1);"),
        "the mount must reference the renamed region var:\n{js}"
    );
    // The whole module must be valid JS (no double `fragment` declaration).
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS (no duplicate `fragment` declaration):\n{js}"
    );
}

#[test]
fn module_scope_root_var_collides_with_user_binding_renames_to_root_1() {
    // A user binding named `root` collides with the MODULE-scope template factory var
    // (`var root = $.from_html(...)`). Official `scope.generate` reserves the
    // component-declared `root` GLOBALLY (across the module + function scopes), so the
    // template var is renamed to `root_1` and the clone frame calls `root_1()`. Verified
    // against svelte@5.56.3. RED against the pre-fix tree (which hard-coded `var root`).
    // The colliding `root` is a supported `$state` signal (shape-1); the
    // allocator seeds from EVERY top-level declared name (incl. `$state`), so the
    // collision fires for a signal binding too.
    let src = "<svelte:options runes={true}/>\n<script>let root = $state(0);</script>\n<button onclick={() => root++}>a</button><p>{root}</p>\n";
    let js = emit(src, "App.svelte");
    // The module-scope template factory var is renamed.
    assert!(
        js.contains("var root_1 = $.from_html("),
        "a user `let root` must push the module template var to `root_1`:\n{js}"
    );
    // The clone frame calls the renamed factory; the region var stays `fragment`
    // (no user `fragment` here).
    assert!(
        js.contains("var fragment = root_1();"),
        "the clone frame must call the renamed factory `root_1()`:\n{js}"
    );
    // NEGATIVE: the bare `var root = $.from_html` must NOT be emitted (the collision).
    assert!(
        !js.contains("var root = $.from_html("),
        "the module template var must not collide with the user `root`:\n{js}"
    );
    assert!(
        js.contains("let root = $.state(0);"),
        "the user signal binding `let root = $.state(0);` must be preserved:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS (no duplicate `root` declaration):\n{js}"
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
fn single_element_root_local_collides_with_user_binding_renames() {
    // The collision-safety covers the single-element clone-root stem too: a user
    // binding named `div` (the clone-root element's var stem) must push the
    // synthesized clone local to `div_1`, not collide. A reactive interpolation
    // inside the `<div>` keeps it dynamic so the clone frame is named. The colliding
    // `div` is a supported `$state` signal (shape-1).
    let src = "<svelte:options runes={true}/>\n<script>let div = $state(0);</script>\n<div><button onclick={() => div++}>{div}</button></div>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("var div_1 = root();"),
        "a user `let div` must push the synthesized clone-root local to `div_1`:\n{js}"
    );
    assert!(
        !js.contains("var div = root();"),
        "the synthesized clone-root local must not collide with the user `div`:\n{js}"
    );
    assert!(
        js.contains("let div = $.state(0);"),
        "the user signal binding `let div = $.state(0);` must be preserved:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS (no duplicate `div` declaration):\n{js}"
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
fn trailing_static_after_dynamic_fragment_emits_next() {
    // A dynamic node followed by TWO trailing static roots: official walks to the
    // dynamic node, then advances the cursor past the trailing static run with
    // `$.next(2)` (emitted AFTER the dynamic node's reset, BEFORE the text effect).
    // Verified against svelte@5.56.3 (`$.reset(button); $.next(2);`). RED against the
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
fn static_then_dynamic_fragment_has_no_trailing_next() {
    // NEGATIVE: a static root FOLLOWED BY a dynamic node (no trailing static) emits
    // NO `$.next()` — official walks to the dynamic node via `$.sibling` and the
    // trailing-static `skipped` count is 0. Verified against svelte@5.56.3
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
    // prop receiver, NOT a write — official `svelte@5.56.3` compiles it to a plain
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
    // key) — NOT `$$props.bar`. Verified against svelte@5.56.3. THE discriminating
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
fn props_default_referencing_a_sibling_prop_fails_closed() {
    // A `$props()` member DEFAULT is the deferral-ledger props-default surface —
    // the supported props surface is a NO-DEFAULT destructure only. A referencing
    // default (`{ a = 1, b = a }`) is one such demoted shape.
    assert_fail_closed(
        "<script>let { a = 1, b = a } = $props();</script>\n<p>{b}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() default"),
    );
}

#[test]
fn props_default_referencing_via_no_default_sibling_fails_closed() {
    // `let { a, b = a } = $props()` — `b` carries a default, so it is the demoted
    // props-default surface, regardless that the default references a sibling.
    assert_fail_closed(
        "<script>let { a, b = a } = $props();</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() default"),
    );
}

#[test]
fn props_non_literal_default_fails_closed() {
    // A non-literal `$props()` default (`[]`) is the demoted props-default surface
    // — like every default, including a constant literal.
    assert_fail_closed(
        "<script>let { a = [] } = $props();</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() default"),
    );
}

#[test]
fn props_literal_default_fails_closed() {
    // A CONSTANT-LITERAL `$props()` default (`{ a = 1 }`) is ALSO demoted — the
    // supported props surface is a NO-DEFAULT destructure only (the literal-default
    // flag-3 eager form is a deferral-ledger follow-up). The discriminating negative
    // for the no-default-only rule.
    assert_fail_closed(
        "<script>let { a = 1 } = $props();</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() default"),
    );
}
#[test]
fn primitive_state_reassigned_to_proxiable_rhs_gets_trailing_true() {
    // F9: a PRIMITIVE `$state(0)` (a `$.state` signal, NOT a StateProxy) reassigned
    // to a PROXIABLE RHS (`{ a: 1 }`) carries the trailing `, true` —
    // `$.set(o, { a: 1 }, true)`. Verified against svelte@5.56.3 (the gate is
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
/// Whether the emitted module parses as valid JavaScript (no TS syntax, valid
/// hoisting) — the F2/F3 validity gate. Parses with the runtime OXC parser at the
/// `module` grammar (the emitted module uses top-level `import`/`export`).
fn parses_as_js(code: &str) -> bool {
    let alloc = Allocator::default();
    let source_type = oxc_span::SourceType::default().with_module(true);
    let ret = oxc_parser::Parser::new(&alloc, code, source_type).parse();
    !ret.panicked && ret.errors.is_empty()
}

/// Count the DECLARED occurrences of a binding `name` (any scope) in the emitted
/// module via an OXC AST walk over `BindingIdentifier`s. A `bind:group` accumulator
/// that collides with a user binding of the same name would declare the name TWICE
/// (an invalid redeclaration); the collision-aware allocator renames the accumulator
/// so each name is declared at most once. References (`IdentifierReference`) are NOT
/// `BindingIdentifier`s, so a `$.bind_group(name, …)` USE is not counted.
fn count_declared_binding(code: &str, name: &str) -> usize {
    use oxc_ast::ast::BindingIdentifier;
    use oxc_ast_visit::Visit;
    struct Counter<'n> {
        name: &'n str,
        count: usize,
    }
    impl<'a> Visit<'a> for Counter<'_> {
        fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
            if it.name.as_str() == self.name {
                self.count += 1;
            }
        }
    }
    let alloc = Allocator::default();
    let source_type = oxc_span::SourceType::default().with_module(true);
    let ret = oxc_parser::Parser::new(&alloc, code, source_type).parse();
    let mut counter = Counter { name, count: 0 };
    counter.visit_program(&ret.program);
    counter.count
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
        let js = compile_client(base, &parsed, &opts, &alloc, false)
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
fn no_filename_derives_unknown() {
    let alloc = Allocator::default();
    let base = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let parsed = parse_svelte(base);
    let opts = SvelteRuntimeOptions::default();
    let js = compile_client(base, &parsed, &opts, &alloc, false)
        .unwrap()
        .code;
    assert!(
        js.contains("export default function _unknown_($$anchor)"),
        "no filename → _unknown_:\n{js}"
    );
}

// ── lang="ts" ────────────────────────────────────────────────────────────────
// ── Fail-closed (per surface family, asserting the exact typed surface) ───────

/// Assert that `source` fails closed with an unsupported surface matching
/// `predicate` (the discriminating typed-surface check) and carrying the
/// machine-stable `svelte-runtime-unsupported-` diagnostic id.
fn assert_fail_closed(source: &str, predicate: impl Fn(&UnsupportedSvelteRuntimeSurface) -> bool) {
    assert_fail_closed_labeled("", source, predicate);
}

/// [`assert_fail_closed`] with a CASE LABEL threaded into every panic message — so a
/// table-driven refusal loop names WHICH case leaked when one regresses (e.g. which import
/// form emitted instead of failing closed).
fn assert_fail_closed_labeled(
    label: &str,
    source: &str,
    predicate: impl Fn(&UnsupportedSvelteRuntimeSurface) -> bool,
) {
    let ctx = if label.is_empty() {
        String::new()
    } else {
        format!(" [{label}]")
    };
    match emit_result(source) {
        Err(ClientCompileError::Unsupported(surface)) => {
            // The discriminating `predicate` pins the EXACT typed surface variant (the
            // machine-stable identity), so the assertion characterizes the refusal arm by
            // its enum shape + diagnostic code, never by a plan/phase label.
            assert!(
                predicate(&surface),
                "wrong fail-closed surface{ctx}: {surface:?} (code {})",
                surface.diagnostic_code()
            );
            // The diagnostic id has the `svelte-runtime-unsupported-` prefix.
            assert!(
                surface
                    .diagnostic_code()
                    .starts_with("svelte-runtime-unsupported-"),
                "diagnostic id shape{ctx}: {}",
                surface.diagnostic_code()
            );
        }
        Ok(js) => panic!("expected fail-closed{ctx}, got a module:\n{js}"),
        Err(other) => panic!("expected an unsupported-surface error{ctx}, got: {other:?}"),
    }
}

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
    // The `{#each}` block IS supported (5e): a `$props()`-sourced array iterated with a
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

/// Assert `{@debug …}` with a non-identifier argument fails closed at lowering with the
/// `debug_tag_invalid_arguments`-mirroring diagnostic, never an emitted module (which
/// would carry an invalid object key like `a.x: $.snapshot(...)`).
fn assert_debug_invalid_arguments(source: &str) {
    match emit_result(source) {
        Err(ClientCompileError::Lowering(errs)) => assert!(
            errs.diagnostics
                .iter()
                .any(|d| d.code == "svelte-runtime-debug-invalid-arguments"),
            "expected the debug-invalid-arguments diagnostic, got {:?}",
            errs.diagnostics
        ),
        other => panic!("expected a `{{@debug}}` invalid-arguments refusal, got {other:?}"),
    }
}

#[test]
fn debug_tag_member_argument_fails_closed() {
    // Official `debug_tag_invalid_arguments`: a `{@debug}` argument must be a bare
    // identifier, never a member expression. Accepting `{@debug a.x}` would emit an
    // invalid object key (`a.x: $.snapshot(...)`).
    assert_debug_invalid_arguments(
        "<script>let a = $state(0);</script>\n{@debug a.x}\n<button onclick={() => a++}>x</button>\n",
    );
}

#[test]
fn debug_tag_binary_argument_fails_closed() {
    // A binary-expression `{@debug}` argument is the same official refusal — arguments
    // are identifiers, not arbitrary expressions.
    assert_debug_invalid_arguments(
        "<script>let a = $state(0);</script>\n{@debug a + 1}\n<button onclick={() => a++}>x</button>\n",
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
fn shadowed_block_rune_declarator_lowers_to_inner_binding() {
    // SCOPE-SAFETY (the #1-trap class): a block `{let count = $state(0)}` declarator that
    // SHADOWS an instance `count` must lower against ITS OWN binding, not a same-named
    // outer one. The outer `count` is never written (a plain `let count = 5`); the INNER
    // block `count` IS written (`count++`), so the block declarator is a `$.state(0)`
    // signal. A NAME lookup would pick the outer (plain) binding and mis-emit `let count =
    // 0`; lowering by binding id emits `let count = $.state(0)`.
    let js = emit(
        "<script>let count = $state(5);</script>\n{#if count}{let count = $state(0)}<button onclick={() => count++}>{count}</button>{/if}\n",
        "App.svelte",
    );
    assert!(
        js.contains("let count = $.state(0)"),
        "the SHADOWING block rune declarator lowers against its own (written) binding \
         (`let count = $.state(0)`), not the outer plain binding:\n{js}"
    );
    assert!(
        js.contains("let count = 5"),
        "the outer instance `count` stays the never-written plain local (`let count = 5`):\n{js}"
    );
}

#[test]
fn await_expression_in_interpolation_fails_closed() {
    // A non-identifier interpolation expression (here an IIFE wrapping an `await`) is
    // the `build_template_chunk` breadth — it fails closed at the complex-interpolation
    // gate before any async-rewrite gate. Only a bare reactive-signal /
    // no-default-prop identifier read is the supported interpolation surface.
    assert_fail_closed(
        "<script>let p = $state(0); let n = $state(0);</script>\n<button onclick={() => n++}>{(async () => await p)()}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComplexInterpolation { .. }),
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
fn invalid_passive_modifier_combinations_fail_closed() {
    // `passive` + `preventDefault` and `passive` + `nonpassive` are official
    // `event_handler_invalid_modifier_combination` compile errors — Verter keeps them
    // fail-closed / refused (matching official's rejection), in BOTH source orders.
    for src in [
        "<script>let n = $state(0);</script>\n<button on:click|passive|preventDefault={() => n++}>x</button>\n",
        "<script>let n = $state(0);</script>\n<button on:click|preventDefault|passive={() => n++}>x</button>\n",
        "<script>let n = $state(0);</script>\n<button on:click|passive|nonpassive={() => n++}>x</button>\n",
    ] {
        assert_fail_closed(src, |s| {
            matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. })
        });
    }
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

/// The behavioral jsdom event-smoke fixture sources (kept in lockstep with the
/// committed `.client.mjs` by the
/// `event_smoke_*_module_matches_the_committed_jsdom_smoke_fixture` tests).
const EVENT_SMOKE_FIXTURES: &[(&str, &str)] = &[
    (
        "event_nondelegated",
        "<script>let focused = $state(false);</script>\n<input onfocus={() => focused = true} />\n<p>{focused}</p>\n",
    ),
    (
        "event_once",
        "<script>let count = $state(0);</script>\n<button on:click|once={() => count++}>btn</button>\n<p>{count}</p>\n",
    ),
    (
        "event_prevent_default",
        "<script>let hits = $state(0);</script>\n<button on:click|preventDefault={() => hits++}>btn</button>\n<p>{hits}</p>\n",
    ),
    (
        "event_stop_propagation",
        "<script>let inner = $state(0);\nlet outer = $state(0);</script>\n<div on:click={() => outer++}><button on:click|stopPropagation={() => inner++}>btn</button></div>\n<p>{inner}-{outer}</p>\n",
    ),
    (
        "event_self",
        "<script>let count = $state(0);</script>\n<div on:click|self={() => count++}><button>child</button></div>\n<p>{count}</p>\n",
    ),
    (
        "event_capture",
        // The BUBBLE handler is registered FIRST and the CAPTURE handler SECOND, so the
        // capture-phase ordering is observable: a correct capture fires `C` before the
        // bubble `B` (→ `CB`), while a DROPPED capture arg would make both bubble-phase
        // and fire in REGISTRATION order (→ `BC`). The smoke asserts `CB`, so it now
        // discriminates a missing 4th `true`.
        "<script>let log = $state('');</script>\n<div on:click={() => log += 'B'} on:click|capture={() => log += 'C'}><button>btn</button></div>\n<p>{log}</p>\n",
    ),
];

/// The behavioral jsdom CONTROL-FLOW-BLOCK smoke fixture sources (kept in lockstep with the
/// committed `.client.mjs` by `block_smoke_modules_match_the_committed_jsdom_fixtures`). Each
/// source ALSO lives in the golden corpus (`svelte_oracle_corpus/fixtures/blocks/`), so the
/// emitted module is independently proven STRUCTURALLY conformant to the pinned official
/// compiler — the smoke adds the BEHAVIORAL (mount-and-react) proof on top.
const BLOCK_SMOKE_FIXTURES: &[(&str, &str)] = &[
    // `{#if}` — the true branch renders its body.
    (
        "block_if_single",
        "<script>\n\tlet show = $state(true);\n</script>\n\n{#if show}\n\t<p>shown</p>\n{/if}\n",
    ),
    // `{#each}` (unkeyed, `$props()`-sourced) — the body is rendered once per item and the
    // item is a SIGNAL (`$.get(row)`), proven by the per-item text reflecting the prop array.
    (
        "block_each_unkeyed",
        "<script>\n\tlet { rows } = $props();\n</script>\n\n{#each rows as row}\n\t<p>{row}</p>\n{/each}\n",
    ),
    // `{#key}` — the keyed block renders its body, and the reactive `count` read INSIDE the
    // block updates on a delegated click (no re-key needed), proving block-interior reactivity.
    (
        "block_key_reactive",
        "<script>\n\tlet selected = $state(0);\n\tlet count = $state(5);\n</script>\n\n<button onclick={() => count++}>inc</button>\n{#key selected}\n\t<p>{count}</p>\n{/key}\n",
    ),
];

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
fn special_element_global_events_stay_fail_closed() {
    // The special-element event boundary: `<svelte:window|body|document on*>` EVENTS need
    // the special-element node gate, so they fail closed at the special-element host gate
    // — never reaching the (open) regular-element event surface. Asserts the refusal is
    // preserved (a positive boundary assertion).
    for (host, src) in [
        (
            "svelte:window",
            "<script>let n = $state(0);</script>\n<svelte:window onresize={() => n++} />\n",
        ),
        (
            "svelte:body",
            "<script>let n = $state(0);</script>\n<svelte:body onclick={() => n++} />\n",
        ),
        (
            "svelte:document",
            "<script>let n = $state(0);</script>\n<svelte:document onkeydown={() => n++} />\n",
        ),
    ] {
        assert_fail_closed(
            src,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == host),
        );
    }
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
fn input_spread_emits_the_seven_argument_attribute_effect() {
    // A void / self-closing `<input>` spread emits the 7-argument form
    // `$.attribute_effect(input, () => ({ ...props }), void 0, void 0, void 0, void 0,
    // true)` — the trailing argument tail official emits for an input.
    let js = emit(
        "<script>let __rune = $state(0);</script>\n<input {...props} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect(input, () => ({ ...props }), void 0, void 0, void 0, void 0, true)"
        )),
        "an <input> spread must emit the 7-argument attribute_effect:\n{js}"
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
fn html_prop_call_payload_does_not_elide_and_thunks_the_rewritten_member() {
    // A `{@html render()}` whose `render` is a no-default `$props()` binding does NOT
    // elide: the callee rewrites to the member `$$props.render`, so the official form is
    // the THUNK over the rewritten whole expression — `$.html(div, () => $$props.render(),
    // true)`. Elision applies ONLY when the rewritten callee equals the bare name (a plain
    // / local / demoted id). Pinned svelte@5.56.3.
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
    // svelte@5.56.3: `{@html {a:1}}` → `$.html(div, () => ({ a: 1 }), true)`. Without the wrap
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
    // makes it `() => ({ html: 'x' }).html`, valid and correct. Pinned svelte@5.56.3.
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
    // Pinned svelte@5.56.3.
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
    // panics Verter's parse-domain TS-strip gate (a later block), and official svelte@5.56.3
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
fn spread_payload_identifier_collision_renames_the_dom_var() {
    // A `<p {...p}>` collides: the DOM-var stem `p` clashes with the free spread-payload
    // identifier `p`. Official renames the DOM local to `p_1` so the `...p` payload still
    // refers to the binding, not the element node. Pinned svelte@5.56.3:
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
fn style_directive_static_text_value_quotes_the_string() {
    // A `style:color="red"` (a static-TEXT directive value) folds the value as the QUOTED
    // string literal `{ color: 'red' }` — NOT a bare identifier `{ color: red }` (an
    // undefined reference). Only `style:` accepts a text value. Pinned svelte@5.56.3:
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
    // `[$.STYLE]: { color: 'red' }`. Pinned svelte@5.56.3:
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
fn valueless_attribute_in_a_spread_fold_emits_raw_true_not_an_empty_string() {
    // A VALUELESS boolean attribute (`<input {...props} disabled />`) folds as the RAW
    // boolean `disabled: true` — NOT the empty-string `disabled: ''`. The IR carries the
    // value as `Option<StaticAttrValue>` where `None` is a valueless attribute; the fold
    // emits the bare `true` token for `None` (an empty-string value is a DIFFERENT IR
    // shape — `Some("")` — covered below). Pinned svelte@5.56.3:
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
    // must preserve. Pinned svelte@5.56.3:
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
fn input_spread_with_a_default_value_reset_attr_suppresses_the_trailing_tail() {
    // An `<input>` spread fold normally takes the 7-argument tail (`…, void 0, void 0,
    // void 0, void 0, true`). The official compiler SUPPRESSES that tail when the input
    // carries an authored plain attribute named EXACTLY `defaultValue` (camelCase): the
    // reset attribute opts the element out of the value/defaultValue reset behavior the
    // tail encodes. Pinned svelte@5.56.3:
    // `$.attribute_effect(input, () => ({ ...$$props.p, defaultValue: 'x' }));` (NO tail).
    let js = emit(
        "<script>let { p } = $props();</script>\n<input {...p} defaultValue=\"x\" />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect(input, () => ({ ...$$props.p, defaultValue: 'x' }))"
        )),
        "an <input> spread with `defaultValue` must fold the attribute:\n{js}"
    );
    // NEGATIVE: the 7-argument trailing tail must be ABSENT — the reset attr suppresses it.
    assert!(
        !n.contains(&nc("void 0, void 0, void 0, void 0, true")),
        "an <input> spread carrying `defaultValue` must NOT emit the trailing tail:\n{js}"
    );
}

#[test]
fn input_spread_with_a_default_checked_reset_attr_suppresses_the_trailing_tail() {
    // The same reset rule for a valueless `defaultChecked` (camelCase). Pinned
    // svelte@5.56.3: `$.attribute_effect(input, () => ({ ...$$props.p, defaultChecked:
    // true }));` (NO tail).
    let js = emit(
        "<script>let { p } = $props();</script>\n<input {...p} defaultChecked />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect(input, () => ({ ...$$props.p, defaultChecked: true }))"
        )),
        "an <input> spread with `defaultChecked` must fold the raw boolean:\n{js}"
    );
    assert!(
        !n.contains(&nc("void 0, void 0, void 0, void 0, true")),
        "an <input> spread carrying `defaultChecked` must NOT emit the trailing tail:\n{js}"
    );
}

#[test]
fn input_spread_with_a_lowercase_defaultvalue_keeps_the_trailing_tail() {
    // The reset-attribute match is CASE-SENSITIVE on the RAW authored name: a lowercase
    // `defaultvalue` is NOT a reset attribute, so the tail STAYS. Pinned svelte@5.56.3:
    // `$.attribute_effect(input, () => ({ ...$$props.p, defaultvalue: 'x' }), void 0,
    // void 0, void 0, void 0, true);` (tail KEPT).
    let js = emit(
        "<script>let { p } = $props();</script>\n<input {...p} defaultvalue=\"x\" />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect(input, () => ({ ...$$props.p, defaultvalue: 'x' }), void 0, void 0, void 0, void 0, true)"
        )),
        "a lowercase `defaultvalue` must KEEP the 7-argument tail:\n{js}"
    );
}

#[test]
fn input_spread_with_a_value_attr_keeps_the_trailing_tail() {
    // NEGATIVE control: a plain `value` attribute is NOT a reset attribute, so the tail
    // STAYS (only the camelCase `defaultValue` / `defaultChecked` suppress it). Pinned
    // svelte@5.56.3 keeps the 7-argument tail.
    let js = emit(
        "<script>let { p } = $props();</script>\n<input {...p} value=\"x\" />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.attribute_effect(input, () => ({ ...$$props.p, value: 'x' }), void 0, void 0, void 0, void 0, true)"
        )),
        "a `value` attribute must KEEP the 7-argument tail:\n{js}"
    );
}

#[test]
fn input_default_value_with_bind_value_emits_property_write_before_bind() {
    // (5c) A static `defaultValue` CO-LOCATED with a `bind:value` on an `<input>` IS a
    // supported 5c surface: official emits the `input.defaultValue = 'x'` property write
    // BEFORE the bind, and the default attribute SUPPRESSES the `$.remove_input_defaults`
    // prelude (the default is set explicitly). Verified against svelte@5.56.3:
    //   input.defaultValue = 'x';
    //   $.bind_value(input, () => $.get(v), ($$value) => $.set(v, $$value));
    // RED against the pre-fix classifier, which fell `defaultValue` through to the
    // static-attr allowlist and refused it (`DynamicAttribute { name: "defaultValue" }`).
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input defaultValue=\"x\" bind:value={v} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("input.defaultValue = 'x'"),
        "a co-located defaultValue must emit the property write:\n{js}"
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "the bind must still emit:\n{js}"
    );
    // The property write comes BEFORE the bind call (official emission order).
    let dv = js
        .find("input.defaultValue = 'x'")
        .expect("defaultValue write");
    let bv = js.find("$.bind_value(input").expect("bind_value call");
    assert!(
        dv < bv,
        "input.defaultValue must be written BEFORE $.bind_value:\n{js}"
    );
    // The `defaultValue` SUPPRESSES the input-defaults prelude (the default is explicit).
    assert!(
        !js.contains("$.remove_input_defaults"),
        "a co-located defaultValue must suppress $.remove_input_defaults:\n{js}"
    );
}

#[test]
fn input_default_value_after_bind_still_emits_property_write_before_bind() {
    // (5c) Source attribute ORDER does not matter: `<input bind:value={v} defaultValue="x">`
    // (default attr AFTER the bind in source) still emits `input.defaultValue = 'x'` BEFORE
    // the `$.bind_value` call. Verified against svelte@5.56.3 (identical output to the
    // before-order case). RED would be an order-sensitive emission that placed the write
    // after the bind.
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={v} defaultValue=\"x\" />\n",
        "App.svelte",
    );
    let dv = js
        .find("input.defaultValue = 'x'")
        .expect("defaultValue write");
    let bv = js.find("$.bind_value(input").expect("bind_value call");
    assert!(
        dv < bv,
        "input.defaultValue must be written BEFORE $.bind_value regardless of source order:\n{js}"
    );
}

#[test]
fn input_default_checked_with_bind_checked_emits_property_write_before_bind() {
    // (5c) A valueless static `defaultChecked` CO-LOCATED with a `bind:checked` on a
    // checkbox `<input>` IS supported: official emits `input.defaultChecked = true` BEFORE
    // the bind, suppressing `$.remove_input_defaults`. Verified against svelte@5.56.3:
    //   input.defaultChecked = true;
    //   $.bind_checked(input, () => $.get(c), ($$value) => $.set(c, $$value));
    // RED against the pre-fix classifier (refused `defaultChecked` at the static-attr gate).
    let js = emit(
        "<script>let c = $state(false);</script>\n<input type=\"checkbox\" defaultChecked bind:checked={c} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("input.defaultChecked = true"),
        "a co-located defaultChecked must emit the boolean property write:\n{js}"
    );
    assert!(
        js.contains("$.bind_checked(input, () => $.get(c), ($$value) => $.set(c, $$value))"),
        "the bind:checked must still emit:\n{js}"
    );
    let dc = js
        .find("input.defaultChecked = true")
        .expect("defaultChecked write");
    let bc = js.find("$.bind_checked(input").expect("bind_checked call");
    assert!(
        dc < bc,
        "defaultChecked must be written BEFORE the bind:\n{js}"
    );
    assert!(
        !js.contains("$.remove_input_defaults"),
        "a co-located defaultChecked must suppress $.remove_input_defaults:\n{js}"
    );
}

#[test]
fn textarea_default_value_with_bind_value_emits_property_write_and_keeps_child_clear() {
    // (5c) A static `defaultValue` co-located with `bind:value` on a `<textarea>` IS
    // supported. Verified against svelte@5.56.3: the `$.remove_textarea_child` prelude is
    // NOT suppressed (only `$.remove_input_defaults` is), and the property write lands
    // between the child-clear and the bind:
    //   $.remove_textarea_child(textarea);
    //   textarea.defaultValue = 'x';
    //   $.bind_value(textarea, () => $.get(v), ($$value) => $.set(v, $$value));
    // RED against the pre-fix classifier (refused `defaultValue` at the static-attr gate).
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<textarea defaultValue=\"x\" bind:value={v}></textarea>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.remove_textarea_child(textarea)"),
        "a textarea defaultValue must NOT suppress remove_textarea_child:\n{js}"
    );
    assert!(
        js.contains("textarea.defaultValue = 'x'"),
        "a co-located textarea defaultValue must emit the property write:\n{js}"
    );
    assert!(
        js.contains("$.bind_value(textarea, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "the textarea bind must still emit:\n{js}"
    );
}

#[test]
fn standalone_default_value_without_bind_still_fails_closed() {
    // NEGATIVE control: a standalone static `defaultValue` with NO matching `bind:value`
    // STAYS fail-closed at the static-attr allowlist (`DynamicAttribute { name:
    // "defaultValue" }`). The acceptance is gated on the co-located `bind:value`, so a
    // bare `<input defaultValue="x">` (the form-default family without a bind) is NOT
    // globally whitelisted. RED would be a blanket defaultValue acceptance. (A trailing
    // `$state` keeps the component in RUNES mode so the attr gate is reached.)
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<input defaultValue=\"x\" />\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "defaultValue"),
    );
}

#[test]
fn standalone_default_checked_without_bind_still_fails_closed() {
    // F3 NEGATIVE control (DEFER-NEW, D-27): a standalone `defaultChecked` with NO matching
    // `bind:checked` STAYS fail-closed at the static-attr allowlist (`DynamicAttribute { name:
    // "defaultChecked" }`). Official svelte@5.56.3 ACCEPTS it (oracle-verified: emits
    // `input.defaultChecked = true;`), but standalone form-default PROPERTY-attribute emission
    // is OUT of 5c's ordinary-DOM `bind:*` charter (D-27). The acceptance is gated on a
    // co-located MATCHING bind, so a bare `<input defaultChecked>` is NOT whitelisted. RED
    // would be a blanket defaultChecked acceptance.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<input defaultChecked />\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "defaultChecked"),
    );
}

#[test]
fn default_checked_with_mismatched_bind_value_fails_closed() {
    // NEGATIVE control: `defaultChecked` co-located with the WRONG bind (`bind:value`,
    // not `bind:checked`) STAYS fail-closed. The acceptance pairs `defaultValue`↔`bind:value`
    // and `defaultChecked`↔`bind:checked` ONLY — a mismatched default+bind is a conservative
    // refusal (NARROWER than official, which accepts the mixed form; 5c keeps the strict
    // co-location boundary). RED would be an acceptance keyed on "any default + any bind".
    assert_fail_closed(
        "<script>let v = $state(\"\");</script>\n<input defaultChecked bind:value={v} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "defaultChecked"),
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
    // Pinned svelte@5.56.3: `$.html(div, render, true)`.
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
    // fold. Pinned svelte@5.56.3:
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
    // Pinned svelte@5.56.3: `$.set_style(div, true, styles, { color: $.get(x) })`.
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
fn html_paren_wrapped_direct_call_payload_elides_the_thunk_to_the_bare_callee() {
    // A `{@html (render)()}` (a direct zero-arg identifier call whose callee is wrapped in
    // transparent author parens) STILL elides the thunk to the bare callee `render` — the
    // parens are peeled off the OXC `ParenthesizedExpression` callee before the
    // identifier-call check (the same transparent-paren peel the spread-operand path does).
    // Pinned svelte@5.56.3: `$.html(div, render, true)` (parens gone).
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
    // walks through EVERY transparent `ParenthesizedExpression`. Pinned svelte@5.56.3:
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
fn html_paren_wrapped_prop_call_payload_thunks_the_rewritten_callee_without_author_parens() {
    // A `{@html (render)()}` whose `render` is a no-default `$props()` binding does NOT
    // elide (the callee rewrites to the member `$$props.render`), so it stays a THUNK — but
    // the thunk renders the REWRITTEN CALLEE CALL `() => $$props.render()`, NOT the blind
    // whole-source rewrite `() => ($$props.render)()` (which would keep the author parens).
    // Pinned svelte@5.56.3: `$.html(div, () => $$props.render(), true)`.
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
fn class_directive_static_text_value_refuses_as_invalid_directive_value() {
    // A `class:on="x"` (a static-TEXT CLASS directive value) is an OFFICIAL COMPILE ERROR
    // (`directive_invalid_value`): a directive value must be a JS expression in curly
    // braces; ONLY `style:` accepts a static-text value. Verter must REFUSE on the
    // official-reject rail (never emit `{ on: 'x' }`). Pinned svelte@5.56.3 throws
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
fn props_rest_spread_still_refuses_as_advanced_rune_not_the_deleted_spread_surface() {
    // A `{...rest}` whose `rest` is a `$props()` REST destructure (`let { a, ...rest } =
    // $props()`) is the advanced-rune rest-props surface (`$.rest_props` + `rest_excludes`), which
    // the script-shape gate rejects BEFORE the template. It must STILL refuse — and the
    // diagnostic must be the `$props() rest` AdvancedRune, NOT the now-deleted
    // spread-or-html surface (a regression guard that element-spread acceptance did not
    // leak into the rest-props destructure).
    let err =
        emit_result("<script>let { a, ...rest } = $props()</script>\n<div {...rest}></div>\n")
            .expect_err("a $props() rest destructure must still refuse");
    let ClientCompileError::Unsupported(surface) = err else {
        panic!("expected an Unsupported refusal, got {err:?}");
    };
    assert!(
        matches!(
            surface,
            UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if rune == "$props() rest"
        ),
        "a $props() rest destructure must refuse as the `$props() rest` AdvancedRune, \
         got {surface:?}"
    );
    assert_eq!(
        surface.diagnostic_code(),
        "svelte-runtime-unsupported-advanced-rune",
        "the rest-props refusal carries the advanced-rune diagnostic, NOT spread-or-html"
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
fn no_value_radio_group_bind_still_declares_binding_group() {
    // FIX 3: a `bind:group` WITHOUT a `value` attr. Official svelte@5.56.3 STILL
    // declares `const binding_group = []` and calls `$.bind_group(binding_group,
    // [], input, get, set)` (verified against the pinned compiler). Verter declared
    // `binding_group` ONLY when `group_values` was non-empty (the static-value
    // path) but emitted the `$.bind_group(binding_group, …)` call regardless — so a
    // no-value group emitted a reference to an UNDECLARED `binding_group` (a runtime
    // ReferenceError). RED before the fix (the call present, the declaration
    // missing); GREEN after.
    let js = emit(
        "<script>let g = $state('');</script>\n<input type=\"radio\" bind:group={g} />\n",
        "App.svelte",
    );
    // The declaration MUST be present (the bug: it was missing without a value).
    assert!(
        js.contains("const binding_group = [];"),
        "a no-value bind:group must STILL declare `const binding_group = []`:\n{js}"
    );
    // It is the first component-function body statement (component-fn scope, not
    // module scope — per-instance isolation).
    assert!(
        js.contains("export default function App($$anchor) {\n\tconst binding_group = [];"),
        "binding_group must be the first component-function body statement:\n{js}"
    );
    // The `$.bind_group(binding_group, [], …)` call references the now-declared
    // accumulator.
    assert!(
        js.contains("$.bind_group(binding_group, [], input, () => $.get(g), ($$value) => $.set(g, $$value))"),
        "the bind_group call must reference the declared binding_group:\n{js}"
    );
    // NEGATIVE: with NO value attr there is NO per-input `input.value = input.__value`
    // write (that write only exists for a static value).
    assert!(
        !js.contains(".__value = "),
        "a no-value group must NOT emit an __value write:\n{js}"
    );
    // NEGATIVE: no `bind_group(binding_group, …)` reference to an UNDECLARED
    // accumulator — the declaration must precede the call.
    let decl_idx = js.find("const binding_group = [];");
    let call_idx = js.find("$.bind_group(binding_group");
    assert!(
        matches!((decl_idx, call_idx), (Some(d), Some(c)) if d < c),
        "the binding_group declaration must precede its bind_group reference:\n{js}"
    );
}

#[test]
fn radio_group_bind_emits_component_fn_scoped_binding_group_and_per_input_value() {
    // 5c: radio `bind:group` (primitive `$state('')`) EMITS (oracle CASE `group`):
    // a component-FUNCTION-scoped `const binding_group = []`, per-input
    // `$.remove_input_defaults` + `input.value = input.__value = '<value>'`, and a
    // per-input `$.bind_group(binding_group, [], input, () => $.get(g), ($$value) =>
    // $.set(g, $$value))`. RED against the pre-5c tree (which refused `bind:group`).
    let js = emit(
        "<script>let g = $state('');</script>\n\
         <input type=\"radio\" bind:group={g} value=\"a\" />\n\
         <input type=\"radio\" bind:group={g} value=\"b\" />\n",
        "App.svelte",
    );
    // The component-FUNCTION-scoped accumulator (NOT module scope — module scope would
    // share group state across instances, a correctness bug).
    assert!(
        js.contains("export default function App($$anchor) {\n\tconst binding_group = [];"),
        "binding_group must be the first component-function body statement:\n{js}"
    );
    // It must NOT be at module scope (between the imports and the export).
    assert!(
        !js.contains("const binding_group = [];\n\nexport default")
            && !js.contains("const binding_group = [];\nexport default"),
        "binding_group must NOT be module-scoped (per-instance isolation):\n{js}"
    );
    // Per-input value writes + the two bind_group calls.
    assert!(
        js.contains("input.value = input.__value = 'a'"),
        "first input value write:\n{js}"
    );
    assert!(
        js.contains("input_1.value = input_1.__value = 'b'"),
        "second input value write:\n{js}"
    );
    assert!(
        js.contains("$.bind_group(binding_group, [], input, () => $.get(g), ($$value) => $.set(g, $$value))"),
        "first bind_group call:\n{js}"
    );
    assert!(
        js.contains("$.bind_group(binding_group, [], input_1, () => $.get(g), ($$value) => $.set(g, $$value))"),
        "second bind_group call:\n{js}"
    );
    // The static `value` must NOT appear in the cloned skeleton (pulled out to the
    // runtime __value write) — the template is a bare `<input type="radio"/>`.
    assert!(
        js.contains("$.from_html(`<input type=\"radio\"/> <input type=\"radio\"/>`"),
        "the group input skeleton must NOT bake the static value:\n{js}"
    );
    // NEGATIVE: no DOM setter carries the `, $$value, true)` proxy flag.
    assert!(
        !js.contains(", $$value, true)"),
        "a DOM bind:group setter must be 2-arg (no should_proxy flag):\n{js}"
    );
}

#[test]
fn quoted_bind_value_function_pair_still_emits_bind_value() {
    // A QUOTED single-expression function-pair (`bind:value="{get, set}"`, a `Mixed`
    // value) is official-VALID and emits `$.bind_value(input, get, set)` (verified
    // svelte@5.56.3) — the bind-expr lowering unwraps the quoted single-`{…}` inner, so
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
    // official svelte@5.56.3 renames the accumulator to `binding_group_1` (keeping the
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
fn bind_group_accumulator_keeps_binding_group_without_collision() {
    // POSITIVE CONTROL for FIX 2: with NO user `binding_group`, the accumulator keeps the
    // canonical `binding_group` name (the seeded allocator's stem is unclaimed) — the
    // rename only triggers on a real collision. Guards against the allocator spuriously
    // renaming when there is no clash.
    let js = emit(
        "<script>let selected = $state('a');</script>\n<input type=\"radio\" bind:group={selected} value=\"a\">\n",
        "App.svelte",
    );
    assert!(
        js.contains("const binding_group = [];")
            && js.contains("$.bind_group(binding_group, [], input,"),
        "with no collision the accumulator must stay `binding_group`:\n{js}"
    );
    assert_eq!(
        count_declared_binding(&js, "binding_group"),
        1,
        "the sole `binding_group` declaration is the accumulator:\n{js}"
    );
    assert_eq!(
        count_declared_binding(&js, "binding_group_1"),
        0,
        "no spurious `binding_group_1` when there is no collision:\n{js}"
    );
}

#[test]
fn independent_bind_groups_get_distinct_accumulators_in_source_order() {
    // FIX 1 (R3c): two INDEPENDENT radio groups (`bind:group={a}` ×2, `bind:group={b}` ×2)
    // must each get their OWN accumulator. Official svelte@5.56.3 emits `const binding_group =
    // []` AND `const binding_group_1 = []` — ONE accumulator per DISTINCT bound group target,
    // allocated in SOURCE ORDER (the first-appearing group is `binding_group`, the next
    // `binding_group_1`); inputs sharing a target share one. The `a`-inputs reference
    // `binding_group`, the `b`-inputs reference `binding_group_1`.
    //
    // RED before the fix: a single component-wide `group_binding_name` cross-registered EVERY
    // group onto ONE accumulator (`binding_group`) — wrong codegen (the two radio groups would
    // share selection state, so picking a `b` radio would uncheck the `a` selection).
    let js = emit(
        "<script>let a = $state('x'); let b = $state('y');</script>\n\
         <input type=\"radio\" bind:group={a} value=\"1\" />\n\
         <input type=\"radio\" bind:group={a} value=\"2\" />\n\
         <input type=\"radio\" bind:group={b} value=\"3\" />\n\
         <input type=\"radio\" bind:group={b} value=\"4\" />\n",
        "App.svelte",
    );
    // Two DISTINCT accumulators, each DECLARED exactly once (OXC-parsed `BindingIdentifier`
    // walk — a single component-wide name would declare only `binding_group`).
    assert_eq!(
        count_declared_binding(&js, "binding_group"),
        1,
        "the first group's accumulator must be declared exactly once:\n{js}"
    );
    assert_eq!(
        count_declared_binding(&js, "binding_group_1"),
        1,
        "the second INDEPENDENT group must get its OWN accumulator `binding_group_1`:\n{js}"
    );
    assert!(
        js.contains("const binding_group = [];") && js.contains("const binding_group_1 = [];"),
        "both accumulators must be declared as `[]`:\n{js}"
    );
    // SOURCE ORDER: `binding_group` (group `a`, first appearance) is declared BEFORE
    // `binding_group_1` (group `b`, second), matching official's insertion-order decl loop.
    let idx0 = js.find("const binding_group = [];").unwrap();
    let idx1 = js.find("const binding_group_1 = [];").unwrap();
    assert!(
        idx0 < idx1,
        "accumulators must be declared in source order (a before b):\n{js}"
    );
    // WIRING: the `a`-inputs (input, input_1) bind `binding_group`; the `b`-inputs (input_2,
    // input_3) bind `binding_group_1` — each group on its own accumulator.
    assert!(
        js.contains(
            "$.bind_group(binding_group, [], input, () => $.get(a), ($$value) => $.set(a, $$value))"
        ),
        "a input 0 must bind binding_group:\n{js}"
    );
    assert!(
        js.contains(
            "$.bind_group(binding_group, [], input_1, () => $.get(a), ($$value) => $.set(a, $$value))"
        ),
        "a input 1 must bind binding_group:\n{js}"
    );
    assert!(
        js.contains(
            "$.bind_group(binding_group_1, [], input_2, () => $.get(b), ($$value) => $.set(b, $$value))"
        ),
        "b input 2 must bind binding_group_1:\n{js}"
    );
    assert!(
        js.contains(
            "$.bind_group(binding_group_1, [], input_3, () => $.get(b), ($$value) => $.set(b, $$value))"
        ),
        "b input 3 must bind binding_group_1:\n{js}"
    );
    // NEGATIVE: the SECOND group's inputs must NOT cross-register onto the FIRST accumulator
    // (the pre-fix single-name bug).
    assert!(
        !js.contains("$.bind_group(binding_group, [], input_2,")
            && !js.contains("$.bind_group(binding_group, [], input_3,"),
        "the second group's inputs must NOT cross-register onto the first accumulator:\n{js}"
    );
    // NEGATIVE: no spurious THIRD accumulator (only two distinct groups exist).
    assert_eq!(
        count_declared_binding(&js, "binding_group_2"),
        0,
        "no spurious third accumulator for two distinct groups:\n{js}"
    );
    // The emitted module is valid JS.
    assert!(
        parses_as_js(&js),
        "the emitted module must parse as JS:\n{js}"
    );
}

#[test]
fn bind_group_keypath_is_whitespace_and_operator_insensitive() {
    // Finding A (R4): the `bind:group` accumulator key is the STRUCTURAL keypath
    // (svelte's `extract_all_identifiers_from_expression`, which is OPERATOR- and
    // WHITESPACE-insensitive), NOT a raw-source compare. Two computed-member group
    // targets with a NON-TRIVIAL index that the previous `target_keypath` could not
    // serialize (`g[i+j]`) fell back to the trimmed SOURCE, so a whitespace or
    // operator difference split them into TWO accumulators. Official svelte@5.56.3
    // shares ONE accumulator for `g[i+j]` / `g[i + j]` (whitespace) AND `g[i+j]` /
    // `g[i*j]` (operators are not part of the identifier keypath `g.i.j`).
    //
    // RED before the fix: the raw-source fallback (`source.trim()`) gave the two
    // spellings DIFFERENT keys → `binding_group` + `binding_group_1`.
    let whitespace = emit(
        "<script>let g = $state(0); let i = $state(0); let j = $state(0);</script>\n\
         <input type=\"checkbox\" bind:group={g[i+j]} />\n\
         <input type=\"checkbox\" bind:group={g[i + j]} />\n",
        "App.svelte",
    );
    assert_eq!(
        count_declared_binding(&whitespace, "binding_group"),
        1,
        "`g[i+j]` and `g[i + j]` are the SAME structural target → ONE accumulator:\n{whitespace}"
    );
    assert_eq!(
        count_declared_binding(&whitespace, "binding_group_1"),
        0,
        "a whitespace difference must NOT split the group (no `binding_group_1`):\n{whitespace}"
    );

    let operator = emit(
        "<script>let g = $state(0); let i = $state(0); let j = $state(0);</script>\n\
         <input type=\"checkbox\" bind:group={g[i+j]} />\n\
         <input type=\"checkbox\" bind:group={g[i*j]} />\n",
        "App.svelte",
    );
    // Official limitation pinned: `g[i+j]` and `g[i*j]` share ONE accumulator because the
    // operator is NOT in the identifier keypath (`g.i.j`). A divergent operator-preserving
    // signature would over-split here vs official.
    assert_eq!(
        count_declared_binding(&operator, "binding_group"),
        1,
        "`g[i+j]` and `g[i*j]` share ONE accumulator (operator-insensitive keypath):\n{operator}"
    );
    assert_eq!(
        count_declared_binding(&operator, "binding_group_1"),
        0,
        "an operator difference must NOT split the group (no `binding_group_1`):\n{operator}"
    );
    assert!(
        parses_as_js(&operator),
        "the emitted module must parse as JS:\n{operator}"
    );
}

#[test]
fn bind_group_keypath_distinguishes_static_member_from_computed_string() {
    // Finding A (R4): the structural keypath PRESERVES the distinctions official keeps.
    // `a.x` (static member, keypath `a.x`) and `a["x"]` (computed string index, keypath
    // `a.["x"]`) are DISTINCT group identities in svelte@5.56.3 → TWO accumulators. The
    // keypath must NOT canonicalize the two member forms together.
    let js = emit(
        "<script>let a = $state(0);</script>\n\
         <input type=\"checkbox\" bind:group={a.x} />\n\
         <input type=\"checkbox\" bind:group={a[\"x\"]} />\n",
        "App.svelte",
    );
    assert_eq!(
        count_declared_binding(&js, "binding_group"),
        1,
        "the first distinct target gets `binding_group`:\n{js}"
    );
    assert_eq!(
        count_declared_binding(&js, "binding_group_1"),
        1,
        "`a.x` and `a[\"x\"]` are DISTINCT targets → a second accumulator `binding_group_1`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must parse as JS:\n{js}"
    );
}

#[test]
fn element_bind_this_function_pair_emits_direct_bind_this() {
    // Finding C (R4): an INTRINSIC element `bind:this={get, set}` (a getter/setter
    // function-pair) is IN 5c scope. Official svelte@5.56.3 accepts it and emits
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
fn element_bind_this_identifier_still_emits_thunked_bind_this() {
    // POSITIVE CONTROL for the `This` shape refactor: the IDENTIFIER `bind:this={el}` form
    // must STILL emit the synthesized get/set thunks (`($$value) => …` / `() => …`) —
    // the refactor to `This { getset }` must not regress the identifier shape.
    let js = emit(
        "<script>let el = $state();</script>\n<div bind:this={el}>x</div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_this(div, ($$value) => $.set(el, $$value), () => $.get(el));"),
        "the identifier bind:this must keep its synthesized lvalue thunks:\n{js}"
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
fn shared_bind_group_target_shares_one_accumulator() {
    // FIX 1 (R3c): two inputs binding the SAME group target (`bind:group={g}` ×2) share ONE
    // accumulator — official svelte@5.56.3 emits a single `const binding_group = []` and both
    // `$.bind_group` calls reference it. The distinct-group key is the structural bind target +
    // scope, so the same target collapses to one slot (the positive control that the per-group
    // accumulator does NOT over-split a shared target).
    let js = emit(
        "<script>let g = $state('x');</script>\n\
         <input type=\"radio\" bind:group={g} value=\"1\" />\n\
         <input type=\"radio\" bind:group={g} value=\"2\" />\n",
        "App.svelte",
    );
    // EXACTLY ONE accumulator (OXC-parsed) — a shared target must NOT mint a second.
    assert_eq!(
        count_declared_binding(&js, "binding_group"),
        1,
        "two inputs sharing a target must share ONE accumulator:\n{js}"
    );
    assert_eq!(
        count_declared_binding(&js, "binding_group_1"),
        0,
        "a shared target must NOT mint a second accumulator:\n{js}"
    );
    assert!(
        js.contains("const binding_group = [];"),
        "the shared accumulator must be declared as `[]`:\n{js}"
    );
    // Both inputs reference the SAME accumulator.
    assert!(
        js.contains(
            "$.bind_group(binding_group, [], input, () => $.get(g), ($$value) => $.set(g, $$value))"
        ),
        "input 0 must bind binding_group:\n{js}"
    );
    assert!(
        js.contains(
            "$.bind_group(binding_group, [], input_1, () => $.get(g), ($$value) => $.set(g, $$value))"
        ),
        "input 1 must bind the SAME binding_group:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must parse as JS:\n{js}"
    );
}

#[test]
fn independent_bind_groups_renumber_past_a_user_binding_group_collision() {
    // FIX 1 (R3c) × FIX-R3b: with a user-declared `binding_group` AND two INDEPENDENT groups,
    // the collision-aware/seeded allocator bumps BOTH accumulators (`binding_group_1` /
    // `binding_group_2`) past the user's `binding_group`, each group still wired to its own.
    // Verified against svelte@5.56.3:
    //   const binding_group_1 = [];
    //   const binding_group_2 = [];
    //   let binding_group = 0;
    //   $.bind_group(binding_group_1, [], input, () => $.get(a), …);
    //   $.bind_group(binding_group_2, [], input_1, () => $.get(b), …);
    let js = emit(
        "<script>let binding_group = $state(0); let a = $state('x'); let b = $state('y');</script>\n\
         <input type=\"radio\" bind:group={a} value=\"1\" />\n\
         <input type=\"radio\" bind:group={b} value=\"2\" />\n",
        "App.svelte",
    );
    // The user's `binding_group` is the SOLE `binding_group` declaration; each group's
    // accumulator is renumbered past it (OXC-parsed declaration counts).
    assert_eq!(
        count_declared_binding(&js, "binding_group"),
        1,
        "the user's `binding_group` must be the sole `binding_group` declaration:\n{js}"
    );
    assert_eq!(
        count_declared_binding(&js, "binding_group_1"),
        1,
        "the first group's accumulator must be renumbered to `binding_group_1`:\n{js}"
    );
    assert_eq!(
        count_declared_binding(&js, "binding_group_2"),
        1,
        "the second group's accumulator must be renumbered to `binding_group_2`:\n{js}"
    );
    assert!(
        js.contains("$.bind_group(binding_group_1, [], input, () => $.get(a),"),
        "group a must bind binding_group_1:\n{js}"
    );
    assert!(
        js.contains("$.bind_group(binding_group_2, [], input_1, () => $.get(b),"),
        "group b must bind binding_group_2:\n{js}"
    );
    // NEGATIVE: no accumulator may collide with the user's `binding_group`.
    assert!(
        !js.contains("const binding_group = [];"),
        "no accumulator may be declared `const binding_group = []` (would collide):\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must parse as JS:\n{js}"
    );
}

#[test]
fn radio_group_bind_entity_decodes_the_static_value_attr() {
    // (5c) The static `bind:group` `value` attribute is ENTITY-DECODED before the
    // `input.value = input.__value` write — official runs the static value through the
    // attribute-value entity decoder, exactly like every other static attribute. Verified
    // against svelte@5.56.3 for `value="a&amp;b"`:
    //   input.value = input.__value = 'a&b';
    // RED against the pre-fix tree, which stored the RAW attribute span and quoted it
    // directly as `'a&amp;b'` (the entity left un-decoded).
    let js = emit(
        "<script>let g = $state(\"\");</script>\n<input type=\"radio\" bind:group={g} value=\"a&amp;b\" />\n",
        "App.svelte",
    );
    assert!(
        js.contains("input.value = input.__value = 'a&b'"),
        "a static bind:group value must be entity-decoded before the __value write:\n{js}"
    );
    // NEGATIVE: the raw, undecoded `&amp;` must NOT survive into the value write.
    assert!(
        !js.contains("'a&amp;b'"),
        "the raw entity must not survive un-decoded in the group value write:\n{js}"
    );
}

#[test]
fn checked_bind_now_emits_remove_input_defaults_and_bind_checked() {
    // 5c: `bind:checked` on an `<input type="checkbox">` EMITS (it used to fail
    // closed). The pinned svelte@5.56.3 shape (oracle CASE `checked`) is
    // `$.remove_input_defaults(input)` then `$.bind_checked(input, () => $.get(c),
    // ($$value) => $.set(c, $$value))`. RED against the pre-5c tree (which refused it).
    let js = emit(
        "<script>let c = $state(false);</script>\n<input type=\"checkbox\" bind:checked={c} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.remove_input_defaults(input)"),
        "checked bind must clear input defaults:\n{js}"
    );
    assert!(
        js.contains("$.bind_checked(input, () => $.get(c), ($$value) => $.set(c, $$value))"),
        "checked bind must emit the get/set $.bind_checked shape:\n{js}"
    );
    // NEGATIVE: the DOM setter must NOT carry the `, $$value, true)` proxy flag (that
    // is a component/window-host policy, never a DOM bind).
    assert!(
        !js.contains(", $$value, true)"),
        "a DOM bind:checked setter must be 2-arg (no should_proxy flag):\n{js}"
    );
}

// ── FIX 2: official HOST-ATTRIBUTE gates (typed-IR driven) ─────────────────────
//
// Several binds are valid ONLY when the host element carries a specific STATIC
// attribute; official svelte@5.56.3 raises a COMPILE ERROR otherwise. The runtime
// router only sees `(name, tag)`, so it would accept these invalid binds and emit
// a divergent / runtime-broken module. The classifier now inspects the host's
// typed `ElementIr` attributes (NEVER a source-text scan) to enforce the gates.

#[test]
fn bind_checked_without_type_attr_fails_closed() {
    // Official: "`bind:checked` can only be used with `<input type="checkbox">`".
    // An `<input bind:checked>` with NO `type` attr fails closed. RED before the
    // fix (Verter accepted it — routing only saw `(checked, input)`).
    assert_fail_closed(
        "<script>let c = $state(false);</script>\n<input bind:checked={c} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "checked"),
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
fn bind_checked_with_dynamic_type_fails_closed() {
    // A DYNAMIC `type={t}` is not a static `type="checkbox"`, so `bind:checked`
    // fails closed (the static-attr gate requires the literal value). RED before
    // the fix.
    assert_fail_closed(
        "<script>let c = $state(false); let t = $state(\"checkbox\");</script>\n<input type={t} bind:checked={c} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "checked"),
    );
}

#[test]
fn bind_checked_with_static_checkbox_type_still_emits() {
    // POSITIVE: the VALID form `<input type="checkbox" bind:checked>` must STILL
    // emit (the gate must not over-refuse). The pinned shape is
    // `$.remove_input_defaults(input)` + `$.bind_checked(input, get, set)`.
    let js = emit(
        "<script>let c = $state(false);</script>\n<input type=\"checkbox\" bind:checked={c} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_checked(input, () => $.get(c), ($$value) => $.set(c, $$value))"),
        "a static type=checkbox bind:checked must still emit:\n{js}"
    );
}

#[test]
fn bind_inner_html_without_contenteditable_fails_closed() {
    // Official: "'contenteditable' attribute is required for textContent, innerHTML
    // and innerText two-way bindings". A `<div bind:innerHTML>` with NO
    // `contenteditable` attr fails closed. RED before the fix (Verter accepted it —
    // the contract `tags: "contenteditable"` admits any element for the IDE, but the
    // RUNTIME must require the actual static attribute).
    assert_fail_closed(
        "<script>let h = $state(\"\");</script>\n<div bind:innerHTML={h}></div>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "innerHTML"),
    );
}

#[test]
fn bind_inner_text_without_contenteditable_fails_closed() {
    // The same gate for `bind:innerText`.
    assert_fail_closed(
        "<script>let t = $state(\"\");</script>\n<div bind:innerText={t}></div>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "innerText"),
    );
}

#[test]
fn bind_text_content_without_contenteditable_fails_closed() {
    // The same gate for `bind:textContent`.
    assert_fail_closed(
        "<script>let t = $state(\"\");</script>\n<div bind:textContent={t}></div>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "textContent"),
    );
}

#[test]
fn bind_inner_html_with_dynamic_contenteditable_fails_closed() {
    // Official: "'contenteditable' attribute cannot be dynamic if element uses
    // two-way binding". A DYNAMIC `contenteditable={e}` with `bind:innerHTML` fails
    // closed. RED before the fix.
    assert_fail_closed(
        "<script>let h = $state(\"\"); let e = $state(true);</script>\n<div contenteditable={e} bind:innerHTML={h}></div>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "innerHTML"),
    );
}

#[test]
fn bind_inner_html_with_static_contenteditable_still_emits() {
    // POSITIVE: the VALID form `<div contenteditable bind:innerHTML>` must STILL
    // emit `$.bind_content_editable('innerHTML', div, get, set)` (the gate must not
    // over-refuse a valueless static `contenteditable`).
    let js = emit(
        "<script>let h = $state(\"\");</script>\n<div contenteditable bind:innerHTML={h}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_content_editable('innerHTML', div, () => $.get(h), ($$value) => $.set(h, $$value))"),
        "a static contenteditable bind:innerHTML must still emit:\n{js}"
    );
}

#[test]
fn bind_inner_html_with_static_contenteditable_value_still_emits() {
    // POSITIVE: a static `contenteditable="true"` (with a literal value) also
    // satisfies the gate — official accepts a static value.
    let js = emit(
        "<script>let h = $state(\"\");</script>\n<div contenteditable=\"true\" bind:innerHTML={h}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_content_editable('innerHTML', div, () => $.get(h), ($$value) => $.set(h, $$value))"),
        "a static contenteditable=\"true\" bind:innerHTML must still emit:\n{js}"
    );
}

#[test]
fn bind_select_value_with_dynamic_multiple_fails_closed() {
    // Official: "'multiple' attribute must be static if select uses two-way
    // binding". A DYNAMIC `<select multiple={m} bind:value>` fails closed,
    // independent of the value type (verified against svelte@5.56.3 with a string
    // `$state` value). A primitive `$state('')` value reaches the bind gate (an
    // array `$state` would fail at the script gate first); the dynamic `multiple`
    // is the surface under test. RED before the fix (Verter accepted it — routing
    // only saw `(value, select)`).
    assert_fail_closed(
        "<script>let v = $state(\"\"); let m = $state(true);</script>\n<select multiple={m} bind:value={v}><option>a</option></select>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_select_value_with_static_multiple_still_emits() {
    // POSITIVE: a STATIC `multiple` with `bind:value` is valid — official emits
    // `$.bind_select_value` (verified against svelte@5.56.3 with a string value).
    // The gate must not over-refuse the static form.
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<select multiple bind:value={v}><option>a</option></select>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_select_value(select, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "a static multiple bind:value must still emit:\n{js}"
    );
}

#[test]
fn bind_select_value_single_still_emits() {
    // POSITIVE: a single (no `multiple`) `<select bind:value>` stays valid.
    let js = emit(
        "<script>let v = $state(\"a\");</script>\n<select bind:value={v}><option>a</option></select>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_select_value(select, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "a single-select bind:value must still emit:\n{js}"
    );
}

// ── official `<input type>` requirement for EVERY input bind (typed-IR driven) ──
//
// official svelte@5.56.3: "'type' attribute must be a static text value if input
// uses two-way binding". For an `<input>` bind, a `type` attribute — when PRESENT —
// must be a STATIC TEXT VALUE (`Static(Some)`). A valueless `type` (`Static(None)`)
// is invalid for EVERY input bind; a DYNAMIC `type={t}` is invalid for every input
// bind EXCEPT `bind:value` (where a dynamic type is ALLOWED). An ABSENT `type` is
// allowed (the `checked`-specific `type="checkbox"` value gate is separate). The
// runtime router only sees `(name, tag)`, so without this gate an invalid program
// emits a divergent / runtime-broken module. Driven from the typed `ElementIr`
// attributes, NEVER a source-text scan.

#[test]
fn bind_value_with_valueless_type_fails_closed() {
    // Official: `<input type bind:value={v}>` (VALUELESS type) → COMPILE ERROR. A
    // valueless `type` (`HostAttr::Static(None)`) is invalid even for `bind:value`.
    // RED before the fix (the input-type gate was checked applied to value).
    assert_fail_closed(
        "<script>let v = $state(\"\");</script>\n<input type bind:value={v} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_group_with_valueless_type_fails_closed() {
    // Official: `<input type bind:group={g} value="a">` (VALUELESS type) → COMPILE
    // ERROR. A valueless `type` is invalid for `bind:group`. RED before the fix
    // (only `bind:checked` was gated, so `bind:group` emitted a divergent module).
    assert_fail_closed(
        "<script>let g = $state(\"\");</script>\n<input type bind:group={g} value=\"a\" />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "group"),
    );
}

#[test]
fn bind_group_with_dynamic_type_fails_closed() {
    // Official: `<input type={t} bind:group={g} value="a">` (DYNAMIC type) → COMPILE
    // ERROR. A dynamic `type={t}` is invalid for `bind:group` (only `bind:value`
    // tolerates a dynamic type). RED before the fix.
    assert_fail_closed(
        "<script>let g = $state(\"\"); let t = $state(\"radio\");</script>\n<input type={t} bind:group={g} value=\"a\" />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "group"),
    );
}

#[test]
fn bind_indeterminate_with_dynamic_type_fails_closed() {
    // Official: `<input type={t} bind:indeterminate={i}>` (DYNAMIC type) → COMPILE
    // ERROR. A dynamic `type={t}` is invalid for `bind:indeterminate`. RED before
    // the fix (`bind:indeterminate` was not gated at all).
    assert_fail_closed(
        "<script>let i = $state(false); let t = $state(\"checkbox\");</script>\n<input type={t} bind:indeterminate={i} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "indeterminate"),
    );
}

#[test]
fn bind_indeterminate_with_valueless_type_fails_closed() {
    // Official: `<input type bind:indeterminate={i}>` (VALUELESS type) → COMPILE
    // ERROR. A valueless `type` is invalid for `bind:indeterminate`. RED before the fix.
    assert_fail_closed(
        "<script>let i = $state(false);</script>\n<input type bind:indeterminate={i} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "indeterminate"),
    );
}

// ── POSITIVE controls: the input-type gate must NOT over-refuse the valid forms ─

#[test]
fn bind_value_with_dynamic_type_still_emits() {
    // POSITIVE control: `<input type={t} bind:value={v}>` (DYNAMIC type) → OK
    // (official emits `$.bind_value`). A dynamic type is ALLOWED for `bind:value`
    // specifically; the gate must not over-refuse it. Verified against svelte@5.56.3.
    let js = emit(
        "<script>let v = $state(\"\"); let t = $state(\"text\");</script>\n<input type={t} bind:value={v} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "a dynamic type bind:value must still emit (dynamic type is OK for value):\n{js}"
    );
}

#[test]
fn bind_value_with_no_type_still_emits() {
    // POSITIVE control: `<input bind:value={v}>` (NO type attr) → OK. An absent type
    // is always allowed. The gate must not over-refuse the §1.2 form.
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<input bind:value={v} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "a no-type bind:value must still emit:\n{js}"
    );
}

#[test]
fn bind_group_with_static_radio_type_still_emits() {
    // POSITIVE control: `<input type="radio" bind:group={g} value="a">` (STATIC
    // type) → OK (official emits `$.bind_group`). The gate must not over-refuse the
    // valid static-type form. Verified against svelte@5.56.3.
    let js = emit(
        "<script>let g = $state(\"\");</script>\n<input type=\"radio\" bind:group={g} value=\"a\" />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_group(binding_group, [], input, () => $.get(g), ($$value) => $.set(g, $$value))"),
        "a static type=radio bind:group must still emit:\n{js}"
    );
}

// ── FIX 2: the host-gate static-attr comparison uses ENTITY-DECODED text ────────
//
// official decodes HTML entity references in static attribute text before the
// `Text.data` comparison (`decode_character_references`), so `type="check&#98;ox"`
// (`&#98;` = `b`) decodes to `"checkbox"` and `bind:checked` is ACCEPTED. Verter's
// host-gate static-text view must decode the attr value before comparing it to
// `"checkbox"` (reusing the existing `decode_attr_entities` decoder), instead of a
// raw byte compare that fails closed.

#[test]
fn bind_checked_with_entity_encoded_checkbox_type_still_emits() {
    // POSITIVE: `<input type="check&#98;ox" bind:checked={c}>` → the static `type`
    // decodes to `"checkbox"`, so official ACCEPTS it and emits `$.bind_checked`.
    // RED before the fix (the raw compare `"check&#98;ox" == "checkbox"` fails
    // closed). Verified against svelte@5.56.3.
    let js = emit(
        "<script>let c = $state(false);</script>\n<input type=\"check&#98;ox\" bind:checked={c} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_checked(input, () => $.get(c), ($$value) => $.set(c, $$value))"),
        "an entity-encoded type=checkbox bind:checked must still emit (decoded compare):\n{js}"
    );
}

// ── bind:value member-target ROOT classification (every non-`$state` root) ─────
//
// A `bind:value={member}` is supported ONLY when the member's ROOT identifier
// resolves to a `$state` binding (the value rewrite is then correct). A member
// rooted at a `$props()` prop / a `$bindable` prop / a `$derived` memo / a plain
// local / an imported binding all fail closed — official emits a distinct
// surface (a `$.prop` flag-7 accessor for a prop, a read-only memo write for a
// derived, …), so accepting them would emit a divergent module.

#[test]
fn bind_value_prop_member_fails_closed() {
    // F-α: `bind:value={obj.x}` where `obj` is a `$props()` binding. Official emits
    // `let obj = $.prop($$props,'obj',7)` + `$.bind_value(input, () => obj().x, …)`;
    // Verter would read it off the no-default-prop path (`$$props.obj.x`) — a
    // divergent module. RED against the pre-fix `Member` arm, which accepted ANY
    // member target unconditionally (the prop-bind guard only caught a BARE ident).
    assert_fail_closed(
        "<script>let { obj } = $props();</script>\n<input bind:value={obj.x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_aliased_prop_member_fails_closed() {
    // F-α: an ALIASED prop local (`{ obj: o }`) bound member `o.x` resolves the
    // same way — the root `o` is a prop, so it fails closed. A coarse
    // name-based check on the source key (`obj`) would miss the alias; the
    // scope-aware root resolution catches it.
    assert_fail_closed(
        "<script>let { obj: o } = $props();</script>\n<input bind:value={o.x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_derived_member_fails_closed() {
    // A `$derived` is demoted entirely — a component declaring `$derived` fails
    // at the rune-position gate before the member-bind gate is reached.
    assert_fail_closed(
        "<script>let c = $state(0); let d = $derived({ x: c });</script>\n<input bind:value={d.x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$derived"),
    );
}

#[test]
fn bind_value_plain_local_member_emits_plain_member_lvalue() {
    // A member rooted at a PLAIN local (`let o = {...}`, never a rune) IS a supported
    // DOM-bind target: official emits a plain read/write closure pair over the member
    // (`$.bind_value(input, () => o.x, ($$value) => o.x = $$value)`), NOT a signal
    // accessor — the plain local survives script lowering verbatim. RED against the
    // pre-widening classifier, which restricted member-rooted binds to `$state` roots
    // and failed this closed.
    let js = emit(
        "<script>let o = { x: '' }; let c = $state(0);</script>\n<input bind:value={o.x} />\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => o.x, ($$value) => o.x = $$value)"),
        "a plain-local member bind:value must emit the plain member lvalue closures:\n{js}"
    );
    // NEGATIVE: the plain local must NOT be routed through a signal accessor.
    assert!(
        !js.contains("$.get(o)") && !js.contains("$.set(o,"),
        "a plain-local member must not emit a $.get/$.set signal accessor:\n{js}"
    );
    // The plain local's declaration survives verbatim (not lowered to a `$.state`).
    assert!(
        js.contains("let o = { x: '' };") && !js.contains("$.state({"),
        "the plain-local declaration must survive verbatim, not become a signal:\n{js}"
    );
}

#[test]
fn bind_value_plain_local_ident_emits_plain_ident_lvalue() {
    // A PLAIN-local identifier bind target (`let v = "x"`, never a rune) is supported:
    // official emits `$.bind_value(input, () => v, ($$value) => v = $$value)` — plain
    // read/write closures, NOT `$.get`/`$.set`. RED against the signal-only classifier.
    // (A trailing `$state` keeps the component in RUNES mode so the bind classifier is
    // reached — a runeless component fails at the legacy-mode gate first.)
    let js = emit(
        "<script>let v = \"x\"; let c = $state(0);</script>\n<input bind:value={v} />\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => v, ($$value) => v = $$value)"),
        "a plain-local ident bind:value must emit the plain ident lvalue closures:\n{js}"
    );
    assert!(
        !js.contains("$.set(v,"),
        "a plain-local ident must not emit a $.set signal write:\n{js}"
    );
}

#[test]
fn bind_value_uninitialized_plain_local_ident_emits_plain_ident_lvalue() {
    // (5c) An UNINITIALIZED plain-local bind target (`let v;`, never a rune) is a
    // supported DOM-bind target — official keeps the bare local verbatim and emits the
    // plain read/write closures `$.bind_value(input, () => v, ($$value) => v = $$value)`,
    // identical to the initialized plain-local shape. Verified against svelte@5.56.3:
    //   let v;
    //   $.bind_value(input, () => v, ($$value) => v = $$value);
    // RED against the pre-fix tree, which admitted a no-init `let` ONLY for `bind:this`
    // and refused an ordinary DOM-bind no-init local at `instance-script-item` (construct
    // `unused bare let`). (A trailing `$state` keeps the component in RUNES mode.)
    let js = emit(
        "<script>let v; let c = $state(0);</script>\n<input bind:value={v} />\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => v, ($$value) => v = $$value)"),
        "an uninitialized plain-local ident bind:value must emit the plain ident lvalue closures:\n{js}"
    );
    // The bare local survives as `let v;` (NO init, NOT lowered to `$.state`).
    assert!(
        js.contains("let v;") && !js.contains("let v = "),
        "the uninitialized plain-local declaration must survive verbatim as `let v;`:\n{js}"
    );
    assert!(
        !js.contains("$.set(v,"),
        "an uninitialized plain-local ident must not emit a $.set signal write:\n{js}"
    );
}

#[test]
fn unused_uninitialized_bare_local_still_fails_closed() {
    // NEGATIVE control for the uninit-plain-local DOM-bind widening: an UNUSED bare
    // `let unused;` that is NOT a bind-target lvalue root stays fail-closed at the
    // instance-script-item gate (construct `unused bare let`). The no-init admission is
    // gated on the bind-lvalue-root set, so a bare local that nothing binds is still
    // refused (it is not the `bind:this` clone-root nor a DOM-bind target). RED would be
    // a wildcard "admit any no-init let".
    assert_fail_closed(
        "<script>let unused; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "unused bare let"),
    );
}

#[test]
fn bind_value_object_state_member_fails_closed_at_the_object_state_decl_gate() {
    // SCOPE BOUNDARY: a member rooted at an OBJECT `$state` (`let o = $state({...})` —
    // the deep-reactive `BareProxy` / `StateProxy` form) is NOT a DOM-bind-target gap.
    // It fails closed UPSTREAM of the bind classifier at the object/array `$state`
    // declaration gate (`state_decl_shape` accepts ONLY a primitive-literal `$state`;
    // an object/array init is the deep-reactive `$.proxy(...)` declaration surface owned
    // by the runes-completion vertical, not the bindings-breadth vertical). The bind
    // target-lvalue widening covers PLAIN-local roots (which need no `$state` decl
    // support); the object-`$state` member becomes bind-reachable only when the
    // object/array `$state` declaration form is opened by its owning vertical, at which
    // point the member rewrite (`() => o.x` for a `BareProxy`, `() => $.get(o).x` for a
    // reassigned `StateProxy`) is already correct in the planner. Discriminating: it
    // fails at the `$state()` non-primitive-init gate, NOT the bind gate.
    assert_fail_closed(
        "<script>let o = $state({ x: '' });</script>\n<input bind:value={o.x} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state() non-primitive init"),
    );
    // The REASSIGNED object `$state` (a `StateProxy`) fails at the same upstream gate.
    assert_fail_closed(
        "<script>let o = $state({ x: '' });</script>\n<input bind:value={o.x} />\n<button onclick={() => o = { x: 'y' }}>r</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state() non-primitive init"),
    );
}

#[test]
fn bind_select_value_with_array_state_fails_closed_at_the_array_state_decl_gate() {
    // SCOPE BOUNDARY: the canonical official `<select multiple>` shape binds an ARRAY
    // `$state([])` target, emitting `let v = $.state($.proxy([]))` +
    // `$.bind_select_value(...)` (verified against svelte@5.56.3). The
    // `$.bind_select_value` 3-arg helper wiring + the static-`multiple` host gate ARE
    // delivered (the PRIMITIVE `$state` form emits identically — see
    // `bind_select_value_with_static_multiple_still_emits`). But the array `$state([])`
    // DECLARATION is a non-primitive `$.proxy([])` init: it fails closed UPSTREAM of the
    // bind classifier at the `$state()` non-primitive-init gate (the deep-reactive
    // declaration surface owned by the runes-completion vertical, not the
    // bindings-breadth vertical). Discriminating: it fails at the `$state()`
    // non-primitive-init gate, NOT the bind gate — so the array-state multiple-select is
    // gated by the pre-existing non-primitive-`$state` boundary, NOT delivered here.
    assert_fail_closed(
        "<script>let v = $state([]);</script>\n<select multiple bind:value={v}><option>a</option></select>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state() non-primitive init"),
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
    // (5c) F1: `bind:group` is the SOLE identifier/member-only bind. A function-pair
    // (SequenceExpression) target on `bind:group` is the official `bind_group_invalid_expression`
    // reject — `bind:group` can only bind to an Identifier or MemberExpression (verified
    // svelte@5.56.3: `<input type="radio" bind:group={() => g, (x) => g = x}>` →
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
    // (5c) A function-pair bind referencing NAMED top-level `function` declarations
    // (`function get(){...} function set(next){...} <input bind:value={get,set}>`) IS a
    // supported 5c surface — the named functions are inside the supported 5c function-binding
    // `bind:x={get,set}` on DOM hosts. The function declarations are ADMITTED (their names
    // are exactly the function-pair-referenced set) and LOWERED with body signal reads /
    // writes rewritten; the bind passes the function IDENTS directly. Verified against
    // svelte@5.56.3:
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
    // (5c) Full-module structural golden for the named-function-pair surface. Asserts the
    // load-bearing facts in source order: the state decl, BOTH lowered function
    // declarations (bodies rewritten), the `remove_input_defaults` prelude, and the
    // `$.bind_value(input, get, set)` ident-passing call. Verified against svelte@5.56.3:
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
fn named_function_not_referenced_by_a_bind_pair_still_fails_closed() {
    // NEGATIVE control for the named-function-pair admission: a top-level `function`
    // whose name is NOT referenced by an accepted function-pair bind STAYS fail-closed at
    // the instance-script-item gate (construct `function`). The admission is gated on the
    // function-pair-referenced name set, so a plain helper that nothing binds is still
    // refused — this proves there is NO wildcard "emit any function" path. RED would be a
    // broadened function admission.
    assert_fail_closed(
        "<script>let c = $state(0); function helper(){ return 1; }</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "function"),
    );
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
    // plain-`.svelte` PARSE ERROR in official svelte@5.56.3 (`Expected token }`) — the
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
    // svelte@5.56.3 (`Unexpected token`) — a type-parameter declaration is TS syntax.
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
    // plain-`.svelte` PARSE ERROR in official svelte@5.56.3 (`Unexpected token`) — the
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
    // (`(readonly x) => …`) is a plain-`.svelte` PARSE ERROR in official svelte@5.56.3
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
    // it (verified svelte@5.56.3: `$.bind_value(input, () => $.get(value), (x = 1) =>
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
    // (verified svelte@5.56.3: `$.bind_value(input, () => $.get(value), (...x) =>
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
    // ACCEPTS it (verified svelte@5.56.3: `$.bind_value(input, () => $.get(value),
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
    // against svelte@5.56.3: `$.bind_value(input, () => $.get(v), ($$value) => $.set(v,
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

// ====================================================================================
// The plain-Svelte-JS function-pair bind lane (mjs + strict official-delta scan,
// no-strip rewrite). The function-pair element acceptance routes through the
// default-CLOSED `parse_plain_svelte_function_pair` helper: each element is parsed as
// plain Svelte JS (`SourceType::mjs()`), the exact two-element sequence shape is
// validated, and a strict official-delta scan refuses the OXC-mjs-over-Acorn residual
// (TS-only class/member fields + decorators + implements/type-params + accessor) that
// official svelte@5.56.3 REJECTS but OXC's plain-JS parse tolerates. Every outcome below
// is oracle-verified against pinned svelte@5.56.3.

/// Assert a function-pair bind source FAILS CLOSED (no module emitted), via the typed
/// `Binding` unsupported-surface — the form reached the bind classifier (it parses
/// cleanly under the upstream tsx expr gate) and the plain-Svelte-JS lane refused it
/// (an `mjs` parse error OR a strict-delta violation). The pre-fix tree silently
/// TS-stripped these and emitted a module, so the `Ok` arm is the RED-before state.
fn assert_function_pair_binding_refused(source: &str) {
    match emit_result(source) {
        Err(ClientCompileError::Unsupported(surface)) => {
            assert!(
                matches!(&surface, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
                "expected a `Binding {{ target: \"value\" }}` refusal, got: {surface:?}"
            );
        }
        Ok(js) => panic!("expected fail-closed (official rejects this), got a module:\n{js}"),
        Err(other) => panic!("expected a `Binding` unsupported surface, got: {other:?}"),
    }
}

/// Assert a function-pair bind source FAILS CLOSED via the upstream expr-parse channel —
/// the element is a plain-`.svelte` PARSE ERROR even under OXC's tsx leniency (e.g.
/// `abstract` in an expression-position class), so the template expression fails at the
/// `svelte-runtime-expr-parse` gate BEFORE the bind classifier. Official svelte@5.56.3
/// likewise REJECTS it (`Expected token }` / `Unexpected token`).
fn assert_function_pair_expr_parse_refused(source: &str) {
    match emit_result(source) {
        Err(ClientCompileError::Lowering(errs)) => {
            assert!(
                errs.diagnostics
                    .iter()
                    .any(|d| d.code == "svelte-runtime-expr-parse"),
                "expected a `svelte-runtime-expr-parse` diagnostic, got: {errs:?}"
            );
        }
        Ok(js) => panic!("expected fail-closed (official rejects this), got a module:\n{js}"),
        Err(other) => panic!("expected an expr-parse lowering error, got: {other:?}"),
    }
}

#[test]
fn bind_value_function_pair_with_class_accessibility_field_getter_fails_closed() {
    // A function-pair GETTER carrying a class with a TS accessibility modifier
    // (`{class C { public x = 1 }, set}`) is a plain-`.svelte` PARSE ERROR in official
    // svelte@5.56.3 (`Unexpected token`) — `public`/`private`/`protected` are TS-only
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
    // REJECTS it (`Unexpected token`). OXC's `mjs` parse populates
    // `PropertyDefinition.definite` (the member `!` marker, NOT an expression-position
    // non-null assertion); the strict-delta scan flags it. RED against the pre-fix tree.
    assert_function_pair_binding_refused(
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
    // ACCEPTS it (verified svelte@5.56.3: `$.bind_value(input, class C {}, (x) =>
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
    // (verified svelte@5.56.3: `$.bind_value(input, a?.b, (x) => $.set(v, x, true))`).
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
    // svelte@5.56.3 parses it as plain JS and emits the RELATIONAL form
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

#[test]
fn bind_value_import_member_fails_closed() {
    // An instance `import` is demoted (script-import) — a component with an import
    // fails at the script-hoist gate before the member-bind gate is reached.
    assert_fail_closed(
        "<script>import { store } from './s.js'; let c = $state(0);</script>\n<input bind:value={store.x} />\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ScriptImport { .. }),
    );
}
// ── form / value-bearing elements: allowlisted bind hosts whose special content /
//    attr models still fail closed ──────────────────────────────────────────────
//
// `<select>` / `<option>` / `<textarea>` ARE in the finite client-core element
// allowlist (`a` / `button` / `div` / `h1` / `input` / `p` / `video` / `textarea` /
// `select` / `option` / `audio` / `details`) — they were added as 5c `bind:value`
// hosts. So a component using them passes the ELEMENT gate; the refusal MOVES to
// their special content / attr models (a static `value` / `selected` is the
// form-control setter family 5c owns via `bind:value`, NOT a static-attr
// serializer), which fail closed at the ATTR gate. `<datalist>` is NOT allowlisted,
// so it still fails closed at the ELEMENT gate
// (`svelte-runtime-unsupported-element`) on the FIRST out-of-allowlist element.

#[test]
fn select_option_static_value_attr_fails_closed_at_the_form_control_gate() {
    // `<select>`/`<option>` are now in the element allowlist (5c bind hosts), so the
    // refusal MOVES to the static `value` attr on `<option>`: a static `value` is the
    // form-control setter family (5c emits `bind:value`, NOT the static-`value`
    // serializer), so it fails closed via the `DynamicAttribute`/form-control channel.
    // RED if the static `value` attr were silently serialized.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option value=\"a\">A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "value"),
    );
}

#[test]
fn select_static_value_attr_fails_closed_at_the_form_control_gate() {
    // `<select value="x">` — the static `value` on the now-allowed `<select>` host is
    // the form-control setter family (5c owns `bind:value`, not the static-`value`
    // attr), so it fails closed at the attr gate, NOT the element gate.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select value=\"x\"><option>A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "value"),
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

#[test]
fn textarea_static_value_attr_fails_closed_at_the_form_control_gate() {
    // `<textarea>` is now an allowed 5c bind host, so a static `value` attr (the
    // form-control setter family — 5c emits `bind:value`, not the static-`value`
    // serializer) fails closed at the attr gate. The empty content passes the
    // special-content gate; the static `value` is the refusal.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea value=\"hi\"></textarea>\n<button onclick={() => c++}>{c}</button>\n",
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
    // runes mode so the fixture reaches the component projection (not the legacy-mode gate).
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

// ── Component-host binding surfaces: the component invocation host is now SUPPORTED — a
//    component `bind:this` / `bind:prop` / function-pair emits the official
//    `$.bind_this` / getter-setter-pair shapes. Each test is DISCRIMINATING (asserts the
//    exact emitted shape + the absence of the DOM `$.bind_*` helper). ──

#[test]
fn component_bind_this_emits_bind_this_wrapper() {
    // `<Child bind:this={inst}/>` — the COMPONENT host emits `$.bind_this(Child(...), set,
    // get)`, NOT a DOM `$.bind_*` helper.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let inst = $state();</script>\n<Child bind:this={inst} />\n",
    )
    .expect("a component bind:this emits a module");
    assert!(
        js.contains("$.bind_this(Child("),
        "missing the $.bind_this wrapper:\n{js}"
    );
    // NEGATIVE: a component bind:this is NOT a DOM element `$.bind_this(node)` on a cloned
    // element, and never a `$.bind_value`-style DOM bind.
    assert!(
        !js.contains("$.bind_value"),
        "a component bind:this must not emit a DOM value bind:\n{js}"
    );
}

#[test]
fn component_bind_prop_emits_getter_setter_pair() {
    // `<Child bind:value={val}/>` — the COMPONENT host emits a getter/setter PAIR on the
    // props object (`get value()/set value($$value)` with the `$.set(val, $$value, true)`
    // should-proxy axis), NOT a `$.bind_*` helper.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let val = $state('');</script>\n<Child bind:value={val} />\n",
    )
    .expect("a component bind:prop emits a module");
    assert!(
        js.contains("get value() {return $.get(val);}")
            && js.contains("set value($$value) {$.set(val, $$value, true);}"),
        "missing the component bind:prop getter/setter pair:\n{js}"
    );
    // NEGATIVE: NOT a DOM `$.bind_value` helper.
    assert!(
        !js.contains("$.bind_value"),
        "a component bind:prop must not emit a DOM value bind:\n{js}"
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

// ── Component unit coverage: the `.svelte` default-import subset, the component-family
//    specials, and the COMPONENT-vs-ELEMENT `let:` split. ──

#[test]
fn svelte_default_import_admitted_other_import_forms_refuse() {
    // The component-import subset: a DEFAULT import of a `.svelte` module is ADMITTED
    // (hoisted to module scope as the component callee). Every OTHER import form stays the
    // broad static-import prelude (not yet supported) and fails closed at the script-import
    // gate.
    let ok = emit_result(
        "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child />\n{p}\n",
    )
    .expect("a default .svelte import is admitted");
    assert!(
        ok.contains("import Child from './Child.svelte';"),
        "the default .svelte import must be hoisted to module scope:\n{ok}"
    );
    // NEGATIVE: named / namespace / side-effect / default-NON-`.svelte` imports refuse.
    for (label, src) in [
        ("named", "import { Child } from './Child.svelte';"),
        ("namespace", "import * as C from './Child.svelte';"),
        ("side-effect", "import './setup.js';"),
        ("default-non-svelte", "import helper from './helper.js';"),
        (
            "mixed-default-named",
            "import Child, { x } from './Child.svelte';",
        ),
    ] {
        let src = format!("<script>{src} let __r = $state(0);</script>\n<p>{{__r}}</p>\n");
        assert_fail_closed_labeled(label, &src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ScriptImport {
                    construct: "import",
                    ..
                }
            )
        });
    }
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
    // The DYNAMIC-COMPONENT-VALUE half of the 5f-a `.svelte`-default-import subset: a `.svelte`
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
fn render_spread_argument_fails_closed_for_every_callee() {
    // Official `svelte@5.56.3` HARD-ERRORS on a SPREAD argument in a `{@render …}` tag
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
fn render_parenthesized_whole_call_spread_fails_closed() {
    // The render-spread refusal is closed over OUTER author parentheses wrapping the WHOLE
    // call: official `svelte@5.56.3` HARD-ERRORS on the spread
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
    // STILL emit the `$.snippet` call with the array spread PRESERVED in its argument thunk
    // (`() => [...$$props.xs]`); peeling outer author parens for the whole-call-spread
    // refusal must not over-refuse this accepted shape.
    let js = emit(
        "<script>let { row, xs } = $props();</script>\n{@render row([...xs])}\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.snippet("),
        "an array-internal spread render arg must still emit the $.snippet call:\n{js}"
    );
    assert!(
        js.contains("() => [...$$props.xs]"),
        "the array-internal spread `() => [...$$props.xs]` must be preserved:\n{js}"
    );
}

#[test]
fn svelte_self_root_placement_fails_closed() {
    // Official `svelte@5.56.3` HARD-ERRORS on a `<svelte:self>` with NO allowed enclosing
    // context (`svelte_self_invalid_placement`: it may only exist inside {#if}/{#each}/
    // {#snippet} blocks or slots passed to components). A ROOT `<svelte:self>` — bare OR
    // `bind:this` — must FAIL CLOSED with the typed component/snippet refusal, never emit
    // the recursive `App(node, {})` / `$.bind_this(App(node, {}), …)` self-call.
    for (label, src) in [
        (
            "root",
            "<script>let { depth } = $props();</script>\n<svelte:self />\n",
        ),
        (
            "root_bind_this",
            "<script>let { depth } = $props(); let x;</script>\n<svelte:self bind:this={x} />\n",
        ),
    ] {
        assert_fail_closed_labeled(label, src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. }
                    if *construct == "svelte:self at invalid placement"
            )
        });
    }
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
fn component_bind_prop_unwritable_root_fails_closed() {
    // A component `bind:value={p}` whose root resolves to a `$props()` PROP (a non-writable
    // root under the shared 5c writable-root policy) fails CLOSED — the component bind setter
    // is never synthesized from a non-writable root. The prop-bind refusal sweep scans only
    // `IrNode::Element`, so this gate is what catches a COMPONENT bind to a prop.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child bind:value={p} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn svelte_window_size_bind_fails_closed_until_special_element_hosts_are_supported() {
    // `<svelte:window bind:innerWidth={w}/>` — the `<svelte:window>` special-element
    // HOST is refused (every non-`<svelte:options>` `<svelte:*>` is a renderable 5f
    // surface). Official emits `$.bind_window_size('innerWidth', ($$value) => $.set(w,
    // $$value, true))` — a 5f shape with the window-host should_proxy flag.
    assert_fail_closed(
        "<script>let w = $state(0);</script>\n<svelte:window bind:innerWidth={w} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == "svelte:window"),
    );
}

#[test]
fn svelte_window_scroll_bind_fails_closed_until_special_element_hosts_are_supported() {
    // `<svelte:window bind:scrollX={sx}/>` — the window host is refused. Official emits
    // `$.bind_window_scroll('x', get, set)` — a 5f shape.
    assert_fail_closed(
        "<script>let sx = $state(0);</script>\n<svelte:window bind:scrollX={sx} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == "svelte:window"),
    );
}

#[test]
fn svelte_body_bind_fails_closed_until_special_element_hosts_are_supported() {
    // `<svelte:body bind:clientWidth={w}/>` — the `<svelte:body>` host is refused (5f).
    // Official emits `$.bind_element_size($.document.body, …)` — a 5f shape.
    assert_fail_closed(
        "<script>let w = $state(0);</script>\n<svelte:body bind:clientWidth={w} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == "svelte:body"),
    );
}

#[test]
fn svelte_body_scrollx_bind_is_invalid_and_fails_closed() {
    // `<svelte:body bind:scrollX={sx}/>` is an OFFICIAL COMPILE ERROR — `<svelte:body>`
    // has NO `scrollX`/`scrollY` (those belong to `<svelte:window>`). It must NEVER
    // emit; Verter refuses it at the `<svelte:body>` host gate (the host is not yet
    // supported, owned by 5f, where the official-invalid host/name pair is a
    // NEGATIVE-coverage case).
    assert_fail_closed(
        "<script>let sx = $state(0);</script>\n<svelte:body bind:scrollX={sx} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == "svelte:body"),
    );
}

#[test]
fn svelte_document_bind_fails_closed_until_special_element_hosts_are_supported() {
    // `<svelte:document bind:activeElement={el}/>` — the `<svelte:document>` host is
    // refused (5f). RED if accepted.
    assert_fail_closed(
        "<script>let el = $state();</script>\n<svelte:document bind:activeElement={el} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == "svelte:document"),
    );
}

#[test]
fn svelte_document_this_bind_fails_closed_until_special_element_hosts_are_supported() {
    // `<svelte:document bind:this={d}/>` — the `<svelte:document>` host `bind:this` is
    // refused (5f). Official emits `$.bind_this($.document, …)` — a 5f shape. RED if
    // accepted.
    assert_fail_closed(
        "<script>let d = $state();</script>\n<svelte:document bind:this={d} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == "svelte:document"),
    );
}

#[test]
fn svelte_window_this_bind_fails_closed_until_special_element_hosts_are_supported() {
    // `<svelte:window bind:this={w}/>` — the `<svelte:window>` host `bind:this` is a
    // DISTINCT special-element surface (not yet supported, owned by 5f). Official emits
    // `$.bind_this($.window, ($$value)
    // => $.set(w, $$value, true), () => $.get(w))` — a 5f shape with the window-host
    // should_proxy flag. RED if the window host `bind:this` were wrongly accepted.
    assert_fail_closed(
        "<script>let w = $state();</script>\n<svelte:window bind:this={w} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == "svelte:window"),
    );
}

#[test]
fn svelte_window_online_bind_fails_closed_until_special_element_hosts_are_supported() {
    // `<svelte:window bind:online={on}/>` — the `<svelte:window>` host is refused (5f).
    // Official emits `$.bind_online(($$value) => $.set(on, $$value, true))` — a 5f
    // shape (the D-21 zero-coverage-gap invariant requires 5c to retain this explicit
    // refusal until 5f opens the `<svelte:window>` host). RED if the online bind were
    // wrongly accepted.
    assert_fail_closed(
        "<script>let on = $state(false);</script>\n<svelte:window bind:online={on} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == "svelte:window"),
    );
}

#[test]
fn svelte_body_this_bind_fails_closed_until_special_element_hosts_are_supported() {
    // `<svelte:body bind:this={b}/>` — the `<svelte:body>` host `bind:this` is refused
    // (5f). Official emits `$.bind_this($.document.body, ($$value) => $.set(b, $$value,
    // true), () => $.get(b))` — a 5f shape (the D-21 zero-coverage-gap invariant
    // requires 5c to retain this explicit refusal until 5f opens the `<svelte:body>`
    // host). RED if the body `bind:this` were wrongly accepted.
    assert_fail_closed(
        "<script>let b = $state();</script>\n<svelte:body bind:this={b} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == "svelte:body"),
    );
}

#[test]
fn props_rest_fails_closed_not_partial() {
    // A `$props()` REST form fails closed — it must NOT partially emit.
    assert_fail_closed(
        "<script>let { name, ...rest } = $props();</script>\n<p>{name}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() rest"),
    );
}

#[test]
fn props_bindable_fails_closed() {
    assert_fail_closed(
        "<script>let { value = $bindable(0) } = $props();</script>\n<p>{value}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn state_raw_fails_closed() {
    assert_fail_closed(
        "<script>let c = $state.raw(0);</script>\n<button onclick={() => c = 1}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.raw"),
    );
}

#[test]
fn legacy_mode_fails_closed() {
    // A non-runes component (no rune calls) is legacy mode.
    assert_fail_closed(
        "<script>export let label;</script>\n<p>{label}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::LegacyMode { .. }),
    );
}

#[test]
fn top_level_style_fails_closed() {
    // F4: a top-level `<style>` (CSS scoping) fails closed — it is NOT accepted
    // as a runtime Main. RED against the pre-fix emitter (which emitted a Main and
    // silently dropped the style / its scoping).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Style { .. }),
    );
}

#[test]
fn svelte_options_custom_element_fails_closed() {
    // F4: `<svelte:options customElement>` is the custom-element axis. RED
    // against the pre-fix path (which refused it as the wrong vertical / accepted a
    // Main).
    assert_fail_closed(
        "<svelte:options customElement=\"x-foo\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }),
    );
}

#[test]
fn svelte_options_other_axis_fails_closed() {
    // F4: a `<svelte:options>` axis beyond name/runes (here `namespace`) is an unsupported options axis.
    assert_fail_closed(
        "<svelte:options namespace=\"svg\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::OptionsAxis { .. }),
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
fn effect_pre_fails_closed() {
    // F4: `$effect.pre(...)` is an advanced rune — it must fail closed, not
    // emit raw `$effect.pre` (a runtime ReferenceError). RED against the pre-fix
    // path (which emitted raw).
    assert_fail_closed(
        "<script>let c = $state(0); $effect.pre(() => console.log(c));</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$effect.pre"),
    );
}

#[test]
fn state_snapshot_in_expression_fails_closed() {
    // `$state.snapshot(x)` INSIDE an interpolation fails closed — the
    // unsupported-rune-inside-an-expression case. A primitive `$state` keeps the
    // component out of the object-state gate, so the `$state.snapshot` rune form is
    // the surface under test.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{$state.snapshot(c)}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.snapshot"),
    );
}

#[test]
fn inspect_rune_fails_closed() {
    // F4: `$inspect(...)` is an advanced rune (prod no-op form not emitted).
    assert_fail_closed(
        "<script>let c = $state(0); $inspect(c);</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$inspect"),
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
    // so the rune-form scan does NOT fire (the component is not refused as an advanced rune
    // `AdvancedRune`). The function itself is out-of-allowlist, so it fails closed at
    // the instance-script-item gate (construct `function`), NOT on the rune basis.
    // This pins the precedence: the magic / rune scans (which honor shadowing) own
    // their precise diagnostics; the generic item refusal owns the rest.
    assert_fail_closed(
        "<script>\n\tlet c = $state(0);\n\tfunction f($inspect) { return $inspect.foo; }\n</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "function"),
    );
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
fn bare_props_in_call_arg_fails_closed() {
    // A bare `$props()` as a CALL ARGUMENT (`console.log($props())`) is not the
    // single supported top-level `$props()` destructure position — fail closed,
    // never emit raw `$props()`. RED against the pre-fix scan.
    assert_fail_closed(
        "<script>console.log($props())</script>\n<p>hi</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props"),
    );
}

#[test]
fn bare_effect_in_function_body_fails_closed() {
    // An `$effect(fn)` NESTED in a function body is not a top-level instance-script
    // statement (the supported `$effect` position) — fail closed, never emit
    // raw `$effect(...)`. RED against the pre-fix scan.
    assert_fail_closed(
        "<script>let c=$state(0); function f(){ $effect(() => c); }</script>\n<p>hi</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$effect"),
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
// ── Module scripts (`<script module>`) — demoted entirely ─────────────────

#[test]
fn module_script_fails_closed_script_import() {
    // A `<script module>` is demoted ENTIRELY (script-import) — the module-script
    // hoist is a script-completion follow-up, refused before any module-rune scan.
    // Covers a module `$state` / `$derived` / `$props()` and a rune-free module body
    // (all fail at the same script-hoist gate, regardless of the module content). An
    // instance `$state` keeps the component runes-mode (so the refusal is the
    // module-script gate, not the legacy-mode gate).
    for module_body in [
        "let x=$state(0)",
        "let x=$state(0); let y=$derived(x)",
        "let {a}=$props()",
        "const K = 1",
    ] {
        let src = format!(
            "<script module>{module_body}</script>\n<script>let c = $state(0);</script>\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        assert_fail_closed(&src, |s| {
            matches!(s, UnsupportedSvelteRuntimeSurface::ScriptImport { .. })
        });
    }
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
fn non_rune_const_local_fails_closed_as_instance_script_item() {
    // A plain non-rune `const` local (`const STEP = 2`) is OUTSIDE the strict finite
    // instance-script allowlist (the three shapes are `let`-only: `$state(<primitive>)`,
    // a no-default `$props()` destructure, a bare `let el;` bind:this target). A
    // `const` / `var` declaration fails closed at the instance-script-item gate
    // (construct `const declaration`). RED against the pre-restructure tree (which
    // emitted `const STEP = 2;` verbatim). The supported `$state` is the rune that keeps
    // the component in runes mode (so the surface under test is the `const`, not the
    // legacy-mode gate).
    assert_fail_closed(
        "<script>let c = $state(0); const STEP = 2;</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "const declaration"),
    );
}
// ── Root text-node region — fail closed ───────────────────────────────────────

#[test]
fn root_text_node_region_fails_closed() {
    // A root TEXT-NODE region (a bare reactive interpolation `{count}` as the
    // component root, with no wrapping element) is the official text-first
    // (`$.text()` + `$.next()`) topology — a distinct emission shape that is a
    // deferral-ledger follow-up. It fails closed rather than emit INVALID JS (an
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

#[test]
fn empty_template_root_fails_closed() {
    // An EMPTY template (only a `<script>`, no rendered DOM) compiles in official to
    // a component fn with NO `root()` call / NO `$.append` (the body is just the
    // script lowering). Verter's clone-frame path would synthesise a `$.comment()`
    // root and then call `root()` on that NODE → `TypeError`. It fails closed
    // rather than emit an undeclared/broken clone frame. RED against the pre-fix
    // tree (which emitted `var root = $.comment();` + `var fragment = root();`).
    assert_fail_closed("<script>let c=$state(0);</script>\n", |s| {
        matches!(s, UnsupportedSvelteRuntimeSurface::RootTextRegion { .. })
    });
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
    // Two SEPARATE `$props()` statements where the second is a whole-object binding
    // (`let p = $props()`) — the whole-object form fails closed even though a
    // basic destructure preceded it. RED against scanning only the first.
    assert_fail_closed(
        "<script>let {a}=$props(); let p=$props();</script>\n<p>{a}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() whole-object"),
    );
}
// ── StateProxy member bind setter — through the rewriter, not raw text ─────────
#[test]
fn bare_identifier_signal_bind_setter_stays_set() {
    // NEGATIVE / symmetry guard for R-B: a bare-identifier signal bind keeps the
    // `$.set(name, $$value)` setter (the member-lvalue routing must not regress the
    // identifier path).
    let src = "<script>let v=$state('');</script>\n<input bind:value={v}/>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("$.bind_value(input, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "a bare-identifier signal bind keeps the $.set setter:\n{js}"
    );
}

// ── `bind:this` op order — emitted before the grouped sibling text effect ──────

#[test]
fn bind_this_emits_before_the_sibling_text_effect() {
    // R-F: `bind:this={el}` on a `<div>` followed by a reactive sibling text `{v}`.
    // Official emits `$.bind_this(div, …)` BEFORE `var text = …` and the grouped
    // `$.template_effect`; `$.bind_value` comes AFTER the text effect. RED against
    // the pre-fix emitter (which emitted ALL binds, including `bind_this`, AFTER the
    // text effect).
    let src = "<script>let v=$state(''); let el;</script>\n<input bind:value={v}/>\n<div bind:this={el}></div>\n{v}\n";
    let js = emit(src, "App.svelte");
    let bind_this = js.find("$.bind_this(div").expect("bind_this emitted");
    let text_effect = js.find("$.template_effect(").expect("text effect emitted");
    let bind_value = js.find("$.bind_value(input").expect("bind_value emitted");
    assert!(
        bind_this < text_effect,
        "bind_this must precede the grouped text effect:\n{js}"
    );
    assert!(
        text_effect < bind_value,
        "the text effect must precede bind_value:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must be valid JS:\n{js}");
}

#[test]
fn dev_codegen_request_fails_closed() {
    // F4: a DEV-MODE codegen request (`dev_codegen: true`) fails closed — the
    // dev-mode output axis is not emitted; only production output is. The dev signal
    // is distinct from `is_production` (the §1.2 default does NOT request dev). RED
    // against the pre-fix path (which ignored the dev flag and emitted prod output).
    let alloc = Allocator::default();
    let parsed = parse_svelte(HELLO_INPUT);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        dev_codegen: true,
        ..Default::default()
    };
    match compile_client(HELLO_INPUT, &parsed, &opts, &alloc, false) {
        Err(ClientCompileError::Unsupported(
            surface @ UnsupportedSvelteRuntimeSurface::DevMode { .. },
        )) => {
            assert_eq!(
                surface.diagnostic_code(),
                "svelte-runtime-unsupported-dev-mode"
            );
        }
        other => panic!("a dev-codegen request must fail closed to DevMode, got: {other:?}"),
    }
    // NEGATIVE: the SAME component WITHOUT dev_codegen emits (the default is not
    // dev — §1.2 must still compile).
    let prod_opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    assert!(
        compile_client(
            HELLO_INPUT,
            &parse_svelte(HELLO_INPUT),
            &prod_opts,
            &alloc,
            false
        )
        .is_ok(),
        "the production default must still emit (no dev fail-closed)"
    );
}

#[test]
fn ssr_fails_closed_to_block_8() {
    let alloc = Allocator::default();
    let parsed = parse_svelte(HELLO_INPUT);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    match compile_client(HELLO_INPUT, &parsed, &opts, &alloc, /*ssr*/ true) {
        Err(ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::ServerGenerate {
            ..
        })) => {}
        other => panic!("ssr must fail closed to ServerGenerate, got: {other:?}"),
    }
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
fn hello_input_module_matches_the_committed_jsdom_smoke_fixture() {
    // The committed `hello_input.client.mjs` fixture (mounted by the happy-dom
    // behavioral smoke) MUST stay equivalent to Verter's emitted §1.2 module, so
    // the smoke can never drift from the emitter. The committed copy is
    // `oxfmt`-formatted (the repo's JS formatter rewrites tabs → spaces and single
    // → double quotes — behavior-preserving cosmetics), so the comparison
    // normalizes BOTH sides by stripping insignificant whitespace and unifying the
    // quote style; any STRUCTURAL / semantic divergence (a different helper, a
    // missing call, a changed order) still fails here and forces a reviewed fixture
    // regeneration.
    let js = emit(HELLO_INPUT, "App.svelte");
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/core/test/fixtures/svelte/hello_input.client.mjs");
    let committed = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("read smoke fixture {}: {e}", fixture_path.display()));
    assert_eq!(
        normalize_js_cosmetics(&js),
        normalize_js_cosmetics(&committed),
        "the §1.2 emitted module diverged STRUCTURALLY from the committed jsdom-smoke \
         fixture; regenerate packages/core/test/fixtures/svelte/hello_input.client.mjs \
         from `compile_client` and re-run oxfmt"
    );
}

/// Assert a committed jsdom-smoke `.mjs` fixture stays equivalent (modulo cosmetics)
/// to Verter's emitted module for `source`, so the happy-dom behavioral smoke can
/// never drift from `compile_client`.
fn assert_jsdom_fixture_in_sync(source: &str, fixture_name: &str) {
    let js = emit(source, "App.svelte");
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/core/test/fixtures/svelte")
        .join(fixture_name);
    let committed = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("read smoke fixture {}: {e}", fixture_path.display()));
    assert_eq!(
        normalize_js_cosmetics(&js),
        normalize_js_cosmetics(&committed),
        "the emitted module diverged STRUCTURALLY from the committed jsdom-smoke fixture \
         {fixture_name}; regenerate it from `compile_client` and re-run oxfmt"
    );
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

// ── DOM-hosted bind behavioral fixtures ────────────────────────────────────────
//
// Each fixture below mounts the EMITTED §1.2 module (its `.client.mjs`, kept in
// lockstep by these tests) against the REAL pinned `svelte@5.56.3` runtime in the
// happy-dom behavioral spec (`svelte-client-bind-smoke.spec.ts`). The emitted
// module was verified to match the pinned-official compiler STRUCTURALLY (helper
// sequence + imports + templates) at authoring; this lockstep test keeps it from
// drifting from `compile_client`. The reflecting `<p>{x}</p>` observable lets the
// behavioral spec assert the DOM→signal write reaches the bound state.

#[test]
fn bind_textarea_value_module_matches_the_committed_jsdom_smoke_fixture() {
    // `<textarea bind:value>` → `$.remove_textarea_child(textarea)` prelude +
    // `$.bind_value(textarea, get, set)` (the textarea host of the value bind).
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet v = $state(\"\");\n</script>\n<textarea bind:value={v}></textarea>\n<p>{v}</p>\n",
        "bind_textarea_value.client.mjs",
    );
}

#[test]
fn bind_select_value_module_matches_the_committed_jsdom_smoke_fixture() {
    // `<select bind:value>` → `$.bind_select_value(select, get, set)` (no
    // `remove_input_defaults` prelude — a select is not an input).
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet v = $state(\"a\");\n</script>\n<select bind:value={v}><option>a</option><option>b</option></select>\n<p>{v}</p>\n",
        "bind_select_value.client.mjs",
    );
}

#[test]
fn bind_checked_module_matches_the_committed_jsdom_smoke_fixture() {
    // `<input type="checkbox" bind:checked>` → `$.remove_input_defaults(input)` +
    // `$.bind_checked(input, get, set)`.
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet c = $state(false);\n</script>\n<input type=\"checkbox\" bind:checked={c} />\n<p>{c}</p>\n",
        "bind_checked.client.mjs",
    );
}

#[test]
fn bind_contenteditable_module_matches_the_committed_jsdom_smoke_fixture() {
    // `<div contenteditable bind:innerHTML>` →
    // `$.bind_content_editable('innerHTML', div, get, set)` (property-named first arg).
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet h = $state(\"\");\n</script>\n<div contenteditable bind:innerHTML={h}></div>\n<p>{h}</p>\n",
        "bind_contenteditable.client.mjs",
    );
}

#[test]
fn bind_property_open_module_matches_the_committed_jsdom_smoke_fixture() {
    // `<details bind:open>` → `$.bind_property('open', 'toggle', details, set, get)` —
    // the generic DOM-property bind (read-write ⇒ getter trailing).
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet o = $state(false);\n</script>\n<details bind:open={o}></details>\n<p>{o}</p>\n",
        "bind_property_open.client.mjs",
    );
}

#[test]
fn bind_group_radio_module_matches_the_committed_jsdom_smoke_fixture() {
    // Radio `bind:group` → component-fn-scoped `const binding_group = []` + per-input
    // `input.value = input.__value = 'X'` + `$.bind_group(binding_group, [], input,
    // get, set)` per member.
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet g = $state(\"\");\n</script>\n<input type=\"radio\" bind:group={g} value=\"a\" />\n<input type=\"radio\" bind:group={g} value=\"b\" />\n<p>{g}</p>\n",
        "bind_group_radio.client.mjs",
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

// ── Additional surface gates (R1, R4, R5, R7, R8) ──────────────────────────────

#[test]
fn destructured_state_object_fails_closed_not_panic() {
    // R1: `let { a } = $state({a:1})` MUST fail closed, NEVER reach a panic.
    // Official 5.56.3 supports it (temp + proxy), but full destructured-state
    // lowering is a deferral-ledger item; a clean fail-closed is correct. RED against
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
    // invalid `$$props.foo-bar`. Verified against svelte@5.56.3
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
fn bind_value_to_call_expression_rejects_with_exact_bind_invalid_expression() {
    // F2: `bind:value={foo()}` is not a valid lvalue / 2-element pair — official svelte@5.56.3
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
fn parenthesized_identifier_bind_value_binds_typed_signal_root() {
    // F6: `bind:value={(v)}` — author parens around a SINGLE identifier (NOT a sequence).
    // Official svelte@5.56.3 ACCEPTS it and binds on the identifier ROOT `v`, IDENTICALLY
    // to the unparenthesized `{v}` (oracle-verified). Verter must derive the identifier root
    // from the typed `BindTargetFact.root_ident` (`v`), NOT `source.trim()` (`"(v)"`, which
    // is not a resolvable binding name and previously made the bind REFUSE). `v` is a
    // `$state` signal (the bind reassigns it), so the setter is `$.set(v, $$value)`.
    let js = emit(
        "<script>let v = $state('');</script>\n<input bind:value={(v)} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.set(v, $$value)"),
        "the setter must resolve the typed root `v` (not the parenthesized source):\n{js}"
    );
    assert!(
        !js.contains("(v) = $$value") && !js.contains("$.set((v)"),
        "the parenthesized source must NOT leak as the lvalue / setter argument:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn parenthesized_identifier_bind_this_binds_typed_root() {
    // F6: `bind:this={(el)}` — author parens around the bind:this identifier target.
    // Official ACCEPTS it and binds on `el`, IDENTICALLY to `{el}`. Verter must read the
    // root from the typed fact (`el`), not `source.trim()` (`"(el)"`, which would fail the
    // declared-instance-local check and refuse). `el` is a `$state` signal target.
    let js = emit(
        "<script>let el = $state();</script>\n<div bind:this={(el)}></div>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_this(div,") && js.contains("$.set(el, $$value)"),
        "bind:this={{(el)}} must be accepted and bind on the typed root `el`:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn parenthesized_sequence_bind_value_still_rejected() {
    // F6 NEGATIVE CONTROL: routing the identifier root through the typed fact must NOT
    // accept a parenthesized SEQUENCE (`bind:value={(get, set)}`) — official rejects author
    // parens around a bind sequence with `bind_invalid_parens` (unaffected by F1/F6). A
    // regression that treated `(get, set)` as an identifier would RED here.
    let err =
        emit_result("<script>let v = $state('');</script>\n<input bind:value={(get, set)} />\n")
            .expect_err("a parenthesized bind sequence must still reject");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected an OfficialReject(BindInvalidParens), got {err:?}");
    };
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::BindInvalidParens,
        "a parenthesized bind sequence must still reject as bind_invalid_parens"
    );
}

#[test]
fn bind_group_dynamic_value_emits_tracked_template_effect_update() {
    // F4: a DYNAMIC `value={label}` on a reactive `bind:group` radio. Official svelte@5.56.3
    // emits (oracle-verified): a `var input_value;` change-tracker, a guarded
    // `$.template_effect` writing `input.value = (input.__value = $.get(label)) ?? ''` (single
    // value → OUTER `?? ''`) BEFORE the `$.bind_group`, and the group getter reads the
    // dynamic-value dependency (`() => { $.get(label); return $.get(selected); }`) in official
    // order. RED before F4: a dynamic group value fell through and failed closed as a generic
    // dynamic form-control attr.
    let src = "<script>\n\tlet selected = $state(\"a\");\n\tlet label = $state(\"a\");\n</script>\n<input type=\"radio\" bind:group={selected} value={label} />\n<button onclick={() => label = \"b\"}>x</button>\n";
    let js = emit(src, "App.svelte");
    // (1) the value change-tracker var (named `<dom_var>_value`).
    assert!(
        js.contains("var input_value;"),
        "must declare the input_value change-tracker:\n{js}"
    );
    // (2) the guarded change-detection update (single value → outer `?? ''`).
    assert!(
        js.contains("if (input_value !== (input_value = $.get(label)))")
            && js.contains("input.value = (input.__value = $.get(label)) ?? ''"),
        "must emit the guarded input.value/__value update:\n{js}"
    );
    // (3) the group getter reads the value dependency first, then returns the bound target.
    assert!(
        js.contains("$.get(label);") && js.contains("return $.get(selected);"),
        "the group getter must read the value dependency before the target:\n{js}"
    );
    // (4) ORDER: the value `$.template_effect` precedes the `$.bind_group` call.
    let eff = js
        .find("$.template_effect")
        .expect("a template_effect is emitted");
    let bind = js.find("$.bind_group").expect("a bind_group is emitted");
    assert!(
        eff < bind,
        "the value $.template_effect must be emitted BEFORE $.bind_group:\n{js}"
    );
    // (5) valid JS.
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn bind_group_mixed_value_emits_template_literal_update() {
    // F4: a MIXED `value="pre-{label}"` group value — official emits the template-literal
    // value `input.value = input.__value = `pre-${$.get(label) ?? ''}`` (NO outer `?? ''` —
    // the template literal is already a string; the `?? ''` is per-interpolation), guarded by
    // the `input_value` tracker, before `$.bind_group`.
    let src = "<script>\n\tlet selected = $state(\"a\");\n\tlet label = $state(\"a\");\n</script>\n<input type=\"radio\" bind:group={selected} value=\"pre-{label}\" />\n<button onclick={() => label = \"b\"}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("var input_value;"),
        "must declare the input_value change-tracker:\n{js}"
    );
    assert!(
        js.contains("input.value = input.__value = `pre-${$.get(label) ?? ''}`"),
        "the mixed value writes the template literal with NO outer `?? ''`:\n{js}"
    );
    assert!(
        js.contains("if (input_value !== (input_value = `pre-${$.get(label) ?? ''}`))"),
        "the guard compares the template-literal value:\n{js}"
    );
    assert!(
        parses_as_js(&js),
        "the emitted module must be valid JS:\n{js}"
    );
}

#[test]
fn bind_group_static_value_stays_direct_write_without_tracker() {
    // F4 NEGATIVE CONTROL (static regression): a STATIC `value="a"` group value stays the
    // one-shot direct write `input.value = input.__value = 'a'` — NO `input_value` tracker and
    // NO `$.template_effect` for the value (the dynamic-value machinery must not fire for a
    // static literal).
    let src = "<script>let g = $state(\"\");</script>\n<input type=\"radio\" bind:group={g} value=\"a\" />\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("input.value = input.__value = 'a'"),
        "the static group value stays a one-shot direct write:\n{js}"
    );
    assert!(
        !js.contains("input_value"),
        "a static group value must NOT declare the dynamic-value tracker:\n{js}"
    );
    assert!(
        !js.contains("$.template_effect"),
        "a static group value must NOT emit a value $.template_effect:\n{js}"
    );
}

#[test]
fn bind_value_to_identifier_still_emits() {
    // R8 NEGATIVE: the §1.2 `bind:value={name}` identifier lvalue still emits the
    // bind op (the lvalue validation must not regress the supported form).
    let js = emit(
        "<script>let name = $state('');</script>\n<input bind:value={name} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(name), ($$value) => $.set(name, $$value))"),
        "a bare-identifier bind:value must still emit:\n{js}"
    );
}
#[test]
fn lang_ts_component_with_bind_targets_fails_closed() {
    // A `<script lang="ts">` component is demoted ENTIRELY (typescript), refused
    // at the parse gate BEFORE any bind / TS-wrapped-target classification. Covers a
    // clean / TS-non-null / TS-`as` / TS-wrapped-member `bind:value` and a TS-wrapped
    // `bind:this` — all fail at the same TypeScript-script gate regardless of the
    // bind shape.
    let cases = [
        "let name = $state(\"\");</script>\n<input bind:value={name} />\n<p>{name}</p>",
        "let name = $state(\"\");</script>\n<input bind:value={name!} />\n<p>{name}</p>",
        "let name = $state(\"\");</script>\n<input bind:value={name as string} />\n<p>{name}</p>",
        "let el = $state(null);</script>\n<div bind:this={el}></div>",
    ];
    for body in cases {
        let src = format!("<script lang=\"ts\">{body}\n");
        assert_fail_closed(&src, |s| {
            matches!(s, UnsupportedSvelteRuntimeSurface::TypeScript { .. })
        });
    }
}

#[test]
fn ts_wrapped_dom_bind_target_in_plain_script_fails_closed() {
    // E (lvalue-widening boundary): a TS-WRAPPED DOM-bind target (`bind:value={v!}` /
    // `{v as string}`) on an ordinary DOM host stays CLOSED — the canonical-lvalue-
    // from-TS strip is a deferral (owned by the future `lang="ts"`-script block, NOT
    // 5c). Oracle determination (svelte@5.56.3): official PARSE-REJECTS this exact
    // form in a PLAIN `<script>` (`Expected token }`); it is only valid under
    // `lang="ts"`, which Verter refuses ENTIRELY as `TypeScript` upstream. Verter's
    // plain-script parser is TSX-LENIENT, so it accepts `v!` syntactically and REACHES
    // the bind classifier — where the TS-wrapped refusal catches it. This pins that
    // refusal as a LIVE, exercised guard (NOT dead code), and is the discriminator
    // that a naive widening (formatting the setter from the raw `v!` source →
    // `$.set(v!, $$value)`) would break.
    //
    // SCOPE: the bind classifier fails a TS-wrapped target closed via the structural
    // "TS-anywhere-in-lvalue" fact (`BindTargetFact.lvalue_contains_ts`), which catches BOTH
    // a ROOT TS wrapper (`v!` / `v as T` / `(v!)`, this test) AND a NON-ROOT TS target (a
    // member-spine `o!.x`, a computed-index `a[x as T]` — characterized by
    // `nested_ts_anywhere_in_bind_target_lvalue_fails_closed`). The EXACT diagnostic-code
    // parity (`expected_token`/`js_parse_error` vs the structural fail-closed) stays D-26
    // (the shared `.mjs` template-expression parse authority), so this is the
    // `Binding`-channel refusal, not a bind-only TS code gate.
    for target in ["v!", "v as string", "(v!)"] {
        let src = format!(
            "<script>let v = $state(\"\");</script>\n<input bind:value={{{target}}} />\n<p>{{v}}</p>\n"
        );
        let err = emit_result(&src).expect_err(
            "a TS-wrapped DOM-bind target must fail closed (canonical-lvalue deferral)",
        );
        assert!(
            matches!(
                err,
                ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::Binding {
                    target: ref t,
                    ..
                }) if t == "value"
            ),
            "a TS-wrapped `bind:value={{{target}}}` must fail closed as the `value` binding surface, got {err:?}"
        );
    }
    // NEGATIVE: the clean (unwrapped) form on the SAME host is the supported 5c shape —
    // the refusal is SPECIFIC to the TS wrapper, not a blanket `bind:value` refusal.
    let clean = "<script>let v = $state(\"\");</script>\n<input bind:value={v} />\n<p>{v}</p>\n";
    let js = emit(clean, "App.svelte");
    assert!(
        js.contains("$.bind_value(input, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "the clean (non-TS-wrapped) bind:value on the same host must emit the supported shape:\n{js}"
    );
    assert!(
        !js.contains("v!"),
        "the emitted module must never contain the raw TS-wrapped lvalue `v!`:\n{js}"
    );
}

#[test]
fn nested_ts_anywhere_in_bind_target_lvalue_fails_closed() {
    // F1: a TS-ONLY operator ANYWHERE in an accepted bind-target lvalue — a member-spine
    // non-null (`o!.x`), a computed-index cast (`a[x as T]`), or a computed-index non-null
    // (`a[x!]`) — FAILS CLOSED. Official svelte@5.56.3 PARSE-REJECTS each in a plain
    // `<script>` (`expected_token` / `js_parse_error`, oracle-verified), so Verter must NOT
    // accept-and-strip them to valid JS (the prior fail-OPEN). The structural
    // "TS-anywhere-in-lvalue" fact (`BindTargetFact.lvalue_contains_ts`) walks the member
    // object spine + computed-index expressions, so a NON-ROOT TS node is caught exactly
    // like a root wrapper. The EXACT diagnostic-code parity
    // (`expected_token`/`js_parse_error` vs Verter's structural fail-closed) stays D-26 (the
    // shared template-expression parse authority owns uniform plain-script TS rejection), so
    // the refusal rides the `UnsupportedSvelteRuntimeSurface::Binding` channel — NOT a
    // bind-only TS code gate.
    for target in ["o!.x", "a[x as T]", "a[x!]"] {
        let src = format!(
            "<script>let o = $state(0); let a = $state(0); let x = $state(0);</script>\n<input bind:value={{{target}}} />\n"
        );
        let err = emit_result(&src).expect_err(
            "nested TS in a bind-target lvalue must fail closed (the TS-anywhere-in-lvalue fact)",
        );
        assert!(
            matches!(
                err,
                ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::Binding {
                    target: ref t,
                    ..
                }) if t == "value"
            ),
            "a nested-TS `bind:value={{{target}}}` must fail closed as the `value` binding surface, got {err:?}"
        );
    }
    // NEGATIVE: the CLEAN member / computed forms (no TS anywhere in the spine) stay
    // ACCEPTED and emit the bind — the fact is SPECIFIC to TS nodes, not a blanket
    // member/computed refusal.
    for target in ["o.x", "a[x]", "obj.a.b", "a[i]"] {
        let src = format!(
            "<script>let o = $state(0); let a = $state(0); let x = $state(0); let i = $state(0); let obj = $state(0);</script>\n<input bind:value={{{target}}} />\n"
        );
        let js = emit(&src, "App.svelte");
        assert!(
            js.contains("$.bind_value(input,"),
            "a CLEAN bind:value={{{target}}} (no nested TS) must still emit the bind:\n{js}"
        );
        assert!(
            parses_as_js(&js),
            "the clean form must emit valid JS:\n{js}"
        );
    }
}

#[test]
fn bind_target_index_with_type_arg_call_fails_closed() {
    // A computed-index bind target whose index is a CALL carrying TS type arguments —
    // `<input bind:value={arr[g<a,b>(c)]}>` (an OXC `CallExpression` with `type_arguments`) —
    // FAILS CLOSED via the structural `lvalue_contains_ts` fact. Under TSX the index parses as
    // a call with `<a,b>` type arguments; the TS-strip lane would DELETE them, emitting the
    // divergent index `arr[g(c)]` (a function call). Official svelte@5.56.3 instead parses the
    // same source as plain JS — the relational/comma `arr[(g < a, b > c)]` (a boolean) — so
    // accept-and-strip would be a BEHAVIORAL divergence. Failing closed (a never-wrong
    // under-accept via the SAME `value` Binding channel as a bare instantiation —
    // `bare_instantiation_bind_target_stays_fail_closed`) is correct until the shared plain-MJS
    // template-expression authority emits the relational form. The EXACT diagnostic-code parity
    // stays D-26. `arr` is a declared writable root; a `$state` drives runes mode.
    let src = "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[g<a,b>(c)]} />\n";
    let result = emit_result(src);
    assert!(
        matches!(
            &result,
            Err(ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::Binding {
                target,
                ..
            })) if target == "value"
        ),
        "a call-with-type-args index bind target must fail closed as the `value` Binding surface, got {result:?}"
    );
    // NEGATIVE: the accept-and-emit-divergent fail-open is GONE — there is no `Ok` emit
    // carrying the type-arg-stripped divergent index `arr[g(c)]`.
    assert!(
        !matches!(&result, Ok(js) if js.contains("arr[g(c)]")),
        "the accept-and-strip fail-open (emitting the divergent index `arr[g(c)]`) must be gone: {result:?}"
    );
}

#[test]
fn bind_target_type_argument_forms_fail_closed_plain_forms_accepted() {
    // The COMPLETE TSX-only type-argument expression class in a SINGLE bind-target lvalue
    // index — a CALL, a NEW, or a TAGGED-TEMPLATE carrying `type_arguments` — FAILS CLOSED via
    // the structural `lvalue_contains_ts` fact (the SAME `value` Binding channel as a bare
    // instantiation). The TSX-strip lane would otherwise DELETE the type arguments and emit a
    // divergent index (`arr[g<a,b>(c)]` -> `arr[g(c)]`), whereas official svelte@5.56.3 parses
    // the same source as plain JS (the relational/comma `arr[(g < a, b > c)]`). The fix is
    // PRECISE: only a type-argument-bearing node fails closed; a plain call / member / index
    // lvalue stays accepted and is emitted verbatim. The EXACT diagnostic-code parity stays
    // D-26 (the shared plain-MJS template-expression parse authority).

    // FAIL-CLOSED: each form, paired with the would-be type-arg-STRIPPED index it must NOT emit.
    for (src, stripped) in [
        // CALL with type arguments (the index is a `CallExpression`).
        (
            "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[g<a,b>(c)]} />\n",
            "arr[g(c)]",
        ),
        // NEW with type arguments (the index member's object is a `NewExpression`).
        (
            "<script>let s = $state(0); let data = [];</script>\n<input bind:value={data[new C<T>().k]} />\n",
            "data[new C().k]",
        ),
        // TAGGED-TEMPLATE with type arguments, as a SINGLE lvalue index (NOT a function-pair).
        (
            "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[tag<T>`x`]} />\n",
            "arr[tag`x`]",
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
            "{src} must NOT accept-and-emit the type-arg-stripped index `{stripped}`: {result:?}"
        );
    }

    // STAYS FAIL-CLOSED: the bare-instantiation arm (`arr[g<T>]` / `f<T>`, an OXC
    // `TSInstantiationExpression` with no trailing call) — a regression guard alongside its
    // dedicated coverage in `bare_instantiation_bind_target_stays_fail_closed`.
    for src in [
        "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[g<T>]} />\n",
        "<script>let s = $state(0); let f = () => 0;</script>\n<input bind:value={f<T>} />\n",
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
            "{src} (bare instantiation) must stay fail closed as the `value` Binding surface, got {result:?}"
        );
    }

    // STAYS ACCEPTED + EMITS THE EXACT INDEX BYTES (precision: only type-argument-bearing nodes
    // fail closed, never plain calls / members / indices). Plain (non-`$state`) roots emit their
    // lvalue verbatim.
    for (src, expected) in [
        // `arr` is the literal-only bind root; `i` is a free (undeclared) index identifier so it
        // emits verbatim (a plain non-root local would itself be an unrelated unsupported item).
        (
            "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[i]} />\n",
            "arr[i]",
        ),
        (
            "<script>let s = $state(0); let obj = {};</script>\n<input bind:value={obj.x} />\n",
            "obj.x",
        ),
        (
            "<script>let s = $state(0); let obj = {};</script>\n<input bind:value={obj.a.b} />\n",
            "obj.a.b",
        ),
        // CRITICAL: a plain CALL index WITHOUT type arguments stays accepted — proving only the
        // type-argument class fails closed, not all calls.
        (
            "<script>let s = $state(0); let arr = [];</script>\n<input bind:value={arr[f(c)]} />\n",
            "arr[f(c)]",
        ),
    ] {
        let js = emit(src, "App.svelte");
        assert!(
            js.contains("$.bind_value(input,"),
            "a plain (type-arg-free) bind target must stay accepted + emit the bind for {src}:\n{js}"
        );
        assert!(
            js.contains(expected),
            "the accepted bind target must emit the exact index bytes `{expected}` for {src}:\n{js}"
        );
        assert!(
            parses_as_js(&js),
            "the accepted plain bind target must emit valid JS for {src}:\n{js}"
        );
    }
}

#[test]
fn bind_target_ts_in_index_subexpression_fails_closed() {
    // A single-lvalue bind target whose computed index embeds a TS-only construct ANYWHERE in
    // a SUB-expression — a typed arrow param, a typed function-expression param, or a typed
    // local declaration inside an IIFE body — FAILS CLOSED via the structural
    // `lvalue_contains_ts` fact. The index sub-expression is otherwise valid JS, so the TSX
    // parser accepts it and the TS-strip lane would DELETE the type annotation and emit a
    // DIVERGENT setter (e.g. `arr[((x: number) => x)(0)]` -> `arr[((x) => x)(0)]`), whereas
    // official svelte@5.56.3 parses the source as plain JS and PARSE-REJECTS the TS. The scan
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
    // FAILS CLOSED via the structural `lvalue_contains_ts` fact. Official svelte@5.56.3 ALSO
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

#[test]
fn group_single_value_provably_defined_omits_outer_coalesce() {
    // A `bind:group` SINGLE value whose expression is PROVABLY DEFINED omits the outer
    // `?? ''` coercion — official svelte@5.56.3 gates the coercion on `evaluated.is_defined`,
    // NOT on single-vs-mixed. Oracle-verified (svelte@5.56.3) over the SUPPORTED 5c value
    // sources: a demoted `$state(5)`, a literal `5`, and a literal `false` all emit
    // `input.value = input.__value = V;` (NO outer `?? ''`). (A bare `let n = 5` is an
    // unsupported instance-script item in 5c, so a demoted `$state` is the identifier vehicle.)
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
    // (svelte@5.56.3) over SUPPORTED 5c value sources: a literal `null` emits
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
fn name_host_attr_invalid_intrinsic_binds_fail_closed_via_unsupported_channel() {
    // The four name/host/host-attr-invalid intrinsic binds whose TARGET is also shape-invalid
    // must fail closed via the UNSUPPORTED channel (`Binding`) — NOT a confidently-WRONG
    // `OfficialReject` shape code. Official svelte@5.56.3 reports a name/host/host-attr error
    // for each (`bind_invalid_name` / `bind_invalid_target` / `attribute_contenteditable_missing`
    // / `attribute_invalid_multiple`); Verter defers those exact codes (D-29) and routes the
    // refusal through the existing unsupported channel. RED before the fix: the official-reject
    // gate's shape scan fired `OfficialReject(BindInvalidExpression / BindInvalidParens)`
    // BEFORE the name/host/host-attr was established, so `emit_result` returned the wrong
    // `OfficialReject` rather than the unsupported-channel `Binding` refusal.
    let cases = [
        // invalid NAME (`foo` is not a DOM bind on `<div>`).
        (
            "<script>let v = $state(0);</script>\n<div bind:foo={f()}></div>\n",
            "foo",
        ),
        // unsupported HOST (`bind:value` is not valid on `<div>`).
        (
            "<script>let v = $state(0);</script>\n<div bind:value={(get, set)}></div>\n",
            "value",
        ),
        // missing host ATTR (innerHTML requires a static `contenteditable`).
        (
            "<script>let v = $state(0);</script>\n<div bind:innerHTML={f()}></div>\n",
            "innerHTML",
        ),
        // invalid host ATTR (a dynamic `multiple` on a `<select bind:value>`).
        (
            "<script>let m = $state(true);</script>\n<select multiple={m} bind:value={f()}></select>\n",
            "value",
        ),
    ];
    for (src, expected) in cases {
        let err = emit_result(src)
            .expect_err("a name/host/host-attr-invalid intrinsic bind must fail closed");
        assert!(
            matches!(
                err,
                ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::Binding {
                    ref target,
                    ..
                }) if target == expected
            ),
            "{src} must fail closed via the unsupported Binding({expected}) channel \
             (not a wrong OfficialReject shape code), got {err:?}"
        );
    }
}
// ── Additional surface gates (R4 reactive-text memoizer, R5 needs_context) ─────
#[test]
fn reactive_text_bare_signal_read_stays_inline() {
    // R4 NEGATIVE (§1.2 preservation): a bare signal read (`{count}`, no call) stays
    // the INLINE `$.set_text(text, $.get(count))` form — the memoizer is NOT used.
    // Verified against svelte@5.56.3. `count` is reassigned (a real signal), so the
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
// ── Refuse-by-default fail-closed surfaces (the structural-refactor closures) ──
//
// The emitter consumes a NARROW `SupportedClientIr` produced by a default-deny
// classifier, so a surface that is not explicitly supported has NO emission type
// and CANNOT emit-by-default. Each test asserts the precise typed surface + owning
// vertical, and (where the prior emit-by-default emitted divergent / invalid JS)
// is RED against the pre-refactor tree.

#[test]
fn binary_constant_interpolation_fails_closed() {
    // A `{1 + 1}` interpolation is a non-identifier (binary) expression — the
    // `build_template_chunk` breadth, refused at the complex-interpolation gate.
    // The component is runes-mode (the `$state` declarator) so the legacy refusal
    // does not pre-empt the interpolation classification.
    assert_fail_closed(
        "<script>let n = $state(0);</script>\n<p>{1 + 1}</p>\n<button onclick={() => n++}>{n}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComplexInterpolation { .. }),
    );
}

#[test]
fn non_reactive_const_interpolation_fails_closed() {
    // A `{C}` read of an instance-script plain `const` is a bare identifier resolving
    // to a NON-reactive binding — official static-folds it to `textContent`, a
    // distinct topology. A separate reactive `$state` drives the onclick so the
    // component reaches the interpolation classifier.
    assert_fail_closed(
        "<script>let n = $state(0); const C = 5;</script>\n<p>{C}</p>\n<button onclick={() => n++}>{n}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::StaticInterpolation { .. }),
    );
}

#[test]
fn never_reassigned_state_interpolation_fails_closed() {
    // A `{n}` read of a `$state` that is NEVER reassigned lowers (in official) to a
    // plain `let n = 5;` and a STATIC `textContent` write, not a reactive op. A
    // SEPARATE reactive `$state` drives the supported onclick (so the component is
    // runes-mode + reactive without reassigning `n`).
    assert_fail_closed(
        "<script>let n = $state(5); let c = $state(0);</script>\n<button onclick={() => c++}>{n}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::StaticInterpolation { .. }),
    );
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
fn instance_export_const_fails_closed() {
    // An instance-script `export const` is OUTSIDE the strict finite instance-script
    // allowlist (the three shapes: `$state(<primitive>)`, a no-default `$props()`
    // destructure, a bare `let el;` bind:this target). It fails closed at the
    // instance-script-item gate (`InstanceScriptItem` construct `export`) rather
    // than emitting an `export` inside the component function (invalid JS). RED against
    // the pre-restructure tree (which emitted the `export const` verbatim).
    assert_fail_closed(
        "<script>let n = $state(0); export const helper = 1;</script>\n<button onclick={() => n++}>{n}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "export"),
    );
}

#[test]
fn instance_export_function_fails_closed() {
    // An instance-script `export function` also fails closed at the instance-script-item
    // gate — an `export`-declaration statement is out-of-allowlist.
    assert_fail_closed(
        "<script>let n = $state(0); export function helper() { return 1; }</script>\n<button onclick={() => n++}>{n}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "export"),
    );
}

#[test]
fn instance_top_level_function_fails_closed() {
    // A plain top-level instance-script FUNCTION (no rune inside) is out-of-allowlist
    // — the supported `onclick` is an inline `$state`-write arrow, so a function
    // (whether a handler reference or a helper) fails closed at the instance-script-item
    // gate (construct `function`). RED against the pre-restructure tree (which
    // lowered the function body verbatim with reactive reads rewritten).
    assert_fail_closed(
        "<script>let count = $state(0); function f(obj) { ({ count } = obj); }</script>\n<button onclick={() => count++}>{count}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "function"),
    );
}

#[test]
fn instance_top_level_class_fails_closed() {
    // A plain top-level instance-script CLASS is out-of-allowlist — fail closed at the
    // instance-script-item gate (construct `class`).
    assert_fail_closed(
        "<script>let count = $state(0); class C { #x = 0; bump() { this.#x++; } }</script>\n<button onclick={() => count++}>{count}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "class"),
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
fn private_field_update_inside_a_class_method_fails_closed() {
    // The private-field passthrough (`this.#x++` inside a class method) is no longer a
    // SUPPORTED surface: a top-level class is out-of-allowlist, so the whole component
    // fails closed at the instance-script-item gate (construct `class`) — a class
    // method body never reaches the rewriter. (The pre-restructure tree lowered the
    // class body verbatim; the class is now §1.2-out-of-core. Covered alongside
    // `instance_top_level_class_fails_closed`.)
    assert_fail_closed(
        "<script>let n = $state(0); class C { #x = 0; bump() { this.#x++; } }</script>\n<button onclick={() => n++}>{n}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "class"),
    );
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
fn reserved_word_class_element_tag_fails_closed() {
    // `<class>` → `var class = root();` is likewise invalid JS — fail closed.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<class></class>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ElementName { .. }),
    );
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
fn textarea_interpolation_content_fails_closed_at_the_special_content_model_gate() {
    // `<textarea>` IS an allowed 5c `bind:value` host, so it PASSES the element
    // allowlist gate; the refusal is the SPECIAL CONTENT-MODEL gate, NOT the element
    // allowlist. A `<textarea>` with INTERPOLATION content (`<textarea>{c}</textarea>`)
    // is the official `textarea.value` / `$.template_effect` reactive-content surface
    // 5c does NOT emit — so it fails closed on the textarea content model as
    // `Element { tag: "textarea" }`, exactly like the `<option>{c}</option>` case
    // below. RED if Verter silently emitted the divergent reactive-content module.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea>{c}</textarea><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "textarea"),
    );
}

#[test]
fn option_with_interpolation_content_fails_closed_at_the_special_content_gate() {
    // `<select>`/`<option>` are now ALLOWED 5c bind hosts, but an `<option>` with an
    // INTERPOLATION child (`<option>{c}</option>`) is the official `option.__value` /
    // `option_value` reactive-tracking content surface 5c does NOT emit — so it fails
    // closed at the special-content gate as `Element { tag: "option" }` (the option's
    // content model, NOT a static-option select host). RED if Verter silently emitted
    // the divergent `option.__value` tracking module.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option>{c}</option></select><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "option"),
    );
    // A nested element child inside `<option>` is likewise not the static-option
    // interior 5c supports — it fails closed on the option content model.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option><b>{c}</b></option></select><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "option"),
    );
}

#[test]
fn static_textarea_content_fails_closed_at_the_special_content_model_gate() {
    // `<textarea>` IS an allowed 5c `bind:value` host (it passes the element
    // allowlist), so the refusal is the SPECIAL CONTENT-MODEL gate, NOT the element
    // allowlist. Even STATIC-only `<textarea>hi</textarea>` content is the official
    // raw-text `textarea` content model 5c does NOT own (5c emits `<textarea>` ONLY as
    // the empty `bind:value` host shape — `$.remove_textarea_child` then `$.bind_value`
    // — so any interior content, static or interpolated, is out of the supported
    // content model). It fails closed as `Element { tag: "textarea" }` at the
    // special-content gate; the component must NOT emit a Main. RED if Verter silently
    // serialized the static content into the cloned template.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea>hi</textarea><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "textarea"),
    );
}

#[test]
fn textarea_bind_value_with_static_text_fallback_child_emits() {
    // (5c) A `<textarea bind:value={v}>fallback</textarea>` — a `bind:value` host with a
    // STATIC-TEXT fallback child — IS a supported 5c surface: the existing
    // `$.remove_textarea_child` prelude clears the baked static child at runtime, so the
    // bind is unaffected. Verified against svelte@5.56.3 (the static text is baked into
    // the cloned skeleton, then stripped):
    //   var root = $.from_html(`<textarea>fallback</textarea>`);
    //   $.remove_textarea_child(textarea);
    //   $.bind_value(textarea, () => $.get(v), ($$value) => $.set(v, $$value));
    // RED against the pre-fix special-content gate, which blanket-refused ANY textarea
    // child (failing this closed as `Element { tag: "textarea" }`).
    let js = emit(
        "<script>let v = $state(\"\");</script>\n<textarea bind:value={v}>fallback</textarea>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.remove_textarea_child(textarea)"),
        "a textarea bind:value with a static fallback must still clear the child:\n{js}"
    );
    assert!(
        js.contains("$.bind_value(textarea, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "the bind must be unaffected by the static fallback child:\n{js}"
    );
    // The static fallback is baked into the cloned skeleton (the prelude strips it at
    // runtime) — official keeps it in the `from_html` template.
    assert!(
        js.contains("<textarea>fallback</textarea>"),
        "the static fallback child must be baked into the cloned skeleton:\n{js}"
    );
}

#[test]
fn textarea_bind_value_with_dynamic_content_child_still_fails_closed() {
    // NEGATIVE control for the F6a static-fallback narrowing (and the D-22 deferral): a
    // `<textarea bind:value={v}>{c}</textarea>` with a DYNAMIC interpolation child STAYS
    // fail-closed. Official emits `$.set_value(textarea, c)` BEFORE the bind — a textarea
    // CONTENT channel distinct from the static-fallback child (which 5c clears via
    // `remove_textarea_child`). The static-text relaxation must NOT leak into the dynamic
    // content surface, which is owned by a later content-model layer (ledger D-22). RED
    // would be a broadened "allow any textarea child" admission.
    assert_fail_closed(
        "<script>let v = $state(\"\"); let c = $state(\"hi\");</script>\n<textarea bind:value={v}>{c}</textarea>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "textarea"),
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

#[test]
fn no_arg_state_lowers_to_void_zero_not_shadowable_undefined() {
    // The no-arg `$state()` init is `undefined` — but the official compiler emits the
    // SHADOW-ROBUST `$.state(void 0)`, never the bare identifier `undefined` (which a
    // local `let undefined` would shadow, diverging the initial state). Verter emitted
    // `$.state(undefined)`. It must emit `$.state(void 0)`. RED against the pre-fix
    // tree. (The shadow-robust `void 0` is the lowering form regardless of a shadowing
    // local; a plain `let undefined = {}` is itself out-of-allowlist now, so the
    // no-arg `$state()` shape is exercised directly.)
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

/// Normalize a JS module to its cosmetic-insensitive form for the smoke-fixture
/// equivalence check. LITERAL-AWARE: collapses cosmetic whitespace OUTSIDE
/// string/template literals (so a tabs-vs-spaces / line-wrap reflow does not
/// false-fail) but PRESERVES whitespace INSIDE string + template-literal TEXT (so
/// the significant `Hello ${...}!` template space, or any meaningful text
/// whitespace, still discriminates). The string-literal DELIMITER is unified to
/// `"` (so `'world'` ≡ `"world"`, the oxfmt single→double cosmetic) WITHOUT
/// touching literal content. A trailing comma before a closing `)` / `]` / `}` is
/// dropped (the oxfmt line-wrap cosmetic). Any token / structure / literal-content
/// change still fails.
fn normalize_js_cosmetics(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    let mut tmpl: Vec<i32> = Vec::new();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    while i < n {
        let in_tmpl_text = tmpl.last().copied() == Some(0);
        if in_tmpl_text {
            let ch = chars[i];
            if ch == '\\' {
                out.push(ch);
                if i + 1 < n {
                    out.push(chars[i + 1]);
                }
                i += 2;
                continue;
            }
            if ch == '`' {
                tmpl.pop();
                out.push('`');
                i += 1;
                continue;
            }
            if ch == '$' && i + 1 < n && chars[i + 1] == '{' {
                *tmpl.last_mut().unwrap() = 1;
                out.push_str("${");
                i += 2;
                continue;
            }
            out.push(ch); // template TEXT — preserved verbatim.
            i += 1;
            continue;
        }
        let ch = chars[i];
        // String literal: unify the DELIMITER to `"`, preserve interior verbatim.
        if ch == '\'' || ch == '"' {
            let quote = ch;
            out.push('"');
            i += 1;
            while i < n && chars[i] != quote {
                if chars[i] == '\\' {
                    out.push(chars[i]);
                    if i + 1 < n {
                        out.push(chars[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                out.push(chars[i]);
                i += 1;
            }
            if i < n {
                out.push('"');
                i += 1;
            }
            continue;
        }
        if ch == '`' {
            tmpl.push(0);
            out.push('`');
            i += 1;
            continue;
        }
        if let Some(depth) = tmpl.last_mut() {
            if *depth > 0 {
                if ch == '{' {
                    *depth += 1;
                    out.push('{');
                    i += 1;
                    continue;
                }
                if ch == '}' {
                    *depth -= 1;
                    out.push('}');
                    i += 1;
                    continue;
                }
            }
        }
        // Whitespace OUTSIDE a literal is dropped entirely (the smoke check is
        // token-adjacency-insensitive — official and Verter differ in line breaks /
        // indentation but the token stream + literal content must match).
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    let collapsed = out.replace(",)", ")").replace(",]", "]").replace(",}", "}");
    // oxfmt wraps a single-EXPRESSION arrow body that is an assignment in parentheses
    // (`() => (x = y)`); the emitter (and official svelte) emit the bare `() => x = y`.
    // The parens are cosmetic, so strip a paren group that WRAPS an arrow body
    // (`=>(EXPR)` → `=>EXPR`) before comparing — this keeps the fixture oxfmt-clean
    // AND lockstep-matching without forcing the emitter to parenthesize.
    strip_redundant_arrow_parens(&collapsed)
}

/// Strip a single redundant paren group that WRAPS an arrow-function body: every
/// `=>(…)` whose `(` is the immediate arrow body and whose matching `)` ends the body
/// (the next char is a statement/expression terminator `;` `}` `)` `]` `,` or EOF) is
/// reduced to `=>…`. Operates on the already-normalized (whitespace-stripped) token
/// stream, so the `(` after `=>` is the body opener. A NON-wrapping paren (a call
/// `f()`, a grouped sub-expression that is not the whole body) is left intact.
fn strip_redundant_arrow_parens(s: &str) -> String {
    let bytes: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        // Detect `=>(` at this position.
        if i + 2 < bytes.len() && bytes[i] == '=' && bytes[i + 1] == '>' && bytes[i + 2] == '(' {
            // Find the matching `)` for the `(` at i+2 (paren-balanced).
            let mut depth = 0;
            let mut j = i + 2;
            let mut close = None;
            while j < bytes.len() {
                match bytes[j] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(j);
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if let Some(c) = close {
                // The paren wraps the whole arrow body iff the char after `)` is a body
                // terminator (or EOF). A `(` that is part of a larger expression
                // (`=>(a)+b`) is NOT a wrapping body paren.
                let after = bytes.get(c + 1).copied();
                let is_body_wrap = matches!(after, None | Some(';' | '}' | ')' | ']' | ','));
                if is_body_wrap {
                    out.push_str("=>");
                    out.extend(&bytes[(i + 3)..c]); // the inner body, sans parens
                    i = c + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

// ── dynamic attributes + boolean DOM props + class/style ─────────────
//
// Every form is pinned BYTE-FAITHFULLY (modulo cosmetics) to svelte@5.56.3 client
// output via the live-compiler probe. Each test is discriminating with a negative
// assertion (the misform that must be ABSENT). The cross-cut: a reactive dynamic
// attr/class/style joins the SAME combined `$.template_effect` as reactive text, in
// source/DOM-walk order; a NON-reactive one is a plain init statement.
//
// The event handlers are the supported inline-arrow `$state`-write shape
// (`onclick={() => v = !v}`) — a named-function handler is the event-wrapper surface.

/// Normalize an EXPECTED emitted-JS substring the same way [`normalize_js_cosmetics`]
/// normalizes the emitter output (whitespace stripped, JS string-literal quotes
/// unified to `"`), so an attribute assertion is written in NATURAL spaced single-quote form
/// and compared against the equally-normalized emitter output.
fn nc(expected: &str) -> String {
    normalize_js_cosmetics(expected)
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

// (Element spread `{...x}` → `$.attribute_effect` and `{@html}` → `$.html` emission are
// covered by `element_spread_emits_the_attribute_effect_fold`,
// `html_tag_emits_the_raw_markup_helper`, and the systematic byte-golden corpus; a
// `$props()` rest destructure stays refused — see
// `props_rest_spread_still_refuses_as_advanced_rune_not_the_deleted_spread_surface`.)

#[test]
fn prop_bind_value_still_refuses() {
    // `bind:value` to a prop is a binding (the prop-bind path) — a regression-safety negative.
    assert_fail_closed(
        "<script>let { v } = $props();</script>\n<input bind:value={v}>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
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

// ─── Value-position emission is source-preserving (author parens kept) ───
// The value/property printer keeps the author's parens verbatim. The one BEHAVIORAL
// value-position transform is the sequence wrap: a top-level `SequenceExpression` is wrapped
// in one paren pair so it stays a single value (`{@html a, b}` -> `() => (a, b)`) rather than
// splitting `b` into a positional argument. Redundant author parens around a non-sequence
// value are a behavior-preserving cosmetic difference the minifier collapses, so the
// behavioral tests below are paren-COUNT-insensitive on the value bytes.

/// Collapse runs of ASCII whitespace to a single space WITHOUT touching parens — for the
/// `{@html}` arrow-body tests, where the sequence-wrap `=>(…)` distinction is exactly what
/// `normalize_js_cosmetics` deliberately erases (`strip_redundant_arrow_parens`). A
/// paren-preserving collapse is the discriminating comparator for the sequence-wrap.
fn collapse_ws_keep_parens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    out
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
