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
    match emit_result(source) {
        Err(ClientCompileError::Unsupported(surface)) => {
            // The discriminating `predicate` pins the EXACT typed surface variant (the
            // machine-stable identity), so the assertion characterizes the refusal arm by
            // its enum shape + diagnostic code, never by a plan/phase label.
            assert!(
                predicate(&surface),
                "wrong fail-closed surface: {surface:?} (code {})",
                surface.diagnostic_code()
            );
            // The diagnostic id has the `svelte-runtime-unsupported-` prefix.
            assert!(
                surface
                    .diagnostic_code()
                    .starts_with("svelte-runtime-unsupported-"),
                "diagnostic id shape: {}",
                surface.diagnostic_code()
            );
        }
        Ok(js) => panic!("expected fail-closed, got a module:\n{js}"),
        Err(other) => panic!("expected an unsupported-surface error, got: {other:?}"),
    }
}

#[test]
fn if_block_fails_closed() {
    assert_fail_closed(
        "<script>let c = $state(true);</script>\n{#if c}<p>yes</p>{/if}\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::Block {
                    construct: "if",
                    ..
                }
            )
        },
    );
}

#[test]
fn each_block_fails_closed() {
    // An `{#each}` block is an unsupported control-flow block. `items` is a
    // plain-local array + a trailing reactive `$state` keeps the component
    // runes-mode, so the block gate is the surface (not the state-shape gate).
    assert_fail_closed(
        "<script>let items = [1, 2]; let c = $state(0);</script>\n{#each items as x}<p>{x}</p>{/each}\n<button onclick={() => c++}>{c}</button>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::Block {
                    construct: "each",
                    ..
                }
            )
        },
    );
}

#[test]
fn await_block_fails_closed() {
    // An `{#await}` block is an unsupported control-flow block — fail closed.
    // `p` is a plain-local promise + a trailing reactive `$state` keeps runes-mode,
    // so the block gate is the surface (not the non-primitive state-shape gate).
    assert_fail_closed(
        "<script>let p = Promise.resolve(1); let c = $state(0);</script>\n{#await p}<p>loading</p>{:then v}<p>{v}</p>{/await}\n<button onclick={() => c++}>{c}</button>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::Block {
                    construct: "await",
                    ..
                }
            )
        },
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
fn capture_event_fails_closed() {
    // A CAPTURE-phase event (`onclickcapture`) is a non-delegated event — fail
    // closed (only modern delegated, non-capture, modifier-free events are
    // supported).
    assert_fail_closed(
        "<script>let n = $state(0);</script>\n<button onclickcapture={() => n++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. }),
    );
}

#[test]
fn legacy_on_modifier_event_fails_closed() {
    // A legacy `on:click|stop` directive (a modifier-bearing event) is non-delegated
    // — fail closed.
    assert_fail_closed(
        "<script>let n = $state(0);</script>\n<button on:click|stop={() => n++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. }),
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
fn component_spread_still_refuses_as_component_surface() {
    // A component spread `<Foo {...p}>` is the component surface (component attrs
    // route differently); element-spread acceptance must NOT leak into it. The unused
    // `$state` marker forces runes mode so the component (not legacy mode) is the surface.
    let err = emit_result("<script>let __rune = $state(0);</script>\n<Foo {...p} />\n")
        .expect_err("a component spread must still refuse");
    let ClientCompileError::Unsupported(surface) = err else {
        panic!("expected an Unsupported refusal, got {err:?}");
    };
    assert!(
        matches!(
            surface,
            UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { .. }
        ),
        "a component spread must refuse as the component surface, got {surface:?}"
    );
}

#[test]
fn html_inside_if_block_still_refuses_as_block_surface() {
    // A `{@html}` INSIDE an `{#if}` block is the control-flow block-body surface; `{@html}`
    // acceptance is scoped to the element/fragment/root context that stands alone, so a
    // block-wrapped `{@html}` must still refuse via the block refusal.
    let err = emit_result(
        "<script>let h = $state('<b>x</b>'); let on = $state(true);</script>\n{#if on}{@html h}{/if}\n",
    )
    .expect_err("a {@html} inside an {#if} block must still refuse");
    let ClientCompileError::Unsupported(surface) = err else {
        panic!("expected an Unsupported refusal, got {err:?}");
    };
    assert!(
        matches!(surface, UnsupportedSvelteRuntimeSurface::Block { .. }),
        "a block-wrapped {{@html}} must refuse as the block surface, got {surface:?}"
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
fn checked_bind_fails_closed() {
    assert_fail_closed(
        "<script>let on = $state(false);</script>\n<input type=\"checkbox\" bind:checked={on} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "checked"),
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
fn bind_value_plain_local_member_fails_closed() {
    // F-α: a member rooted at a PLAIN local (`let o = {...}`, never a rune) is not
    // a reactive surface; the value-bind boundary is `$state`-rooted only, so it
    // fails closed.
    assert_fail_closed(
        "<script>let o = { x: '' }; let c = $state(0);</script>\n<input bind:value={o.x} />\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
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
// ── form / value-bearing elements demoted by the strict element allowlist ──────
//
// `<select>` / `<option>` / `<datalist>` / `<textarea>` are NOT in the finite
// client-core element allowlist (`a` / `button` / `div` / `h1` / `input` / `p`), so
// a component using ANY of them fails closed at the ELEMENT gate
// (`svelte-runtime-unsupported-element`) on the FIRST out-of-allowlist element —
// regardless of its attrs or interior. (The pre-restructure tree accepted these
// elements and gated only specific attrs; the strict allowlist demotes them whole.)

#[test]
fn select_option_element_fails_closed_at_the_element_allowlist() {
    // A `<select><option value="a">` component fails at the element gate on the
    // first out-of-allowlist element (`<select>`), not at the `value` attr.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option value=\"a\">A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
    );
}

#[test]
fn select_value_element_fails_closed_at_the_element_allowlist() {
    // `<select value="x">` likewise fails at the element gate (the host element, not
    // the `value`).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select value=\"x\"><option>A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
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
fn textarea_value_element_now_fails_closed_at_the_element_allowlist() {
    // DEMOTION proof: a static `value` on `<textarea>` USED to serialize verbatim and
    // emit a Main; `<textarea>` is now out of the allowlist, so the component fails
    // closed at the element gate and emits NO Main.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea value=\"hi\"></textarea>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "textarea"),
    );
}

#[test]
fn option_selected_element_now_fails_closed_at_the_element_allowlist() {
    // DEMOTION proof: a static `selected` on `<option>` USED to serialize
    // `selected=""`; `<select>` / `<option>` are now out of the allowlist, so the
    // component fails closed at the element gate on `<select>`.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option selected>A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
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
fn component_fails_closed() {
    // A component reference (a capitalized tag) is the component surface. No import is used
    // (imports are demoted as script-import) so the component node is the surface under test.
    assert_fail_closed("<script>let c = $state(0);</script>\n<Foo />\n", |s| {
        matches!(
            s,
            UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { .. }
        )
    });
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
fn bind_value_to_call_expression_fails_closed() {
    // R8: `bind:value={foo()}` is not a valid lvalue — official raises
    // `bind_invalid_expression`; Verter fails closed (never emits `foo() = $$value`).
    // RED against the prior path (which validated only tag/target, not the
    // bound expression, and emitted invalid JS). The component is runes-mode (a
    // `$state` declarator) so the bind validation is reached (not pre-empted by the
    // legacy-mode refusal).
    assert_fail_closed(
        "<script>let n = $state(0); function foo() { return 1; }</script>\n<input bind:value={foo()} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
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
fn textarea_element_fails_closed_at_the_element_allowlist() {
    // `<textarea>` is NOT in the finite client-core element allowlist (`a` / `button`
    // / `div` / `h1` / `input` / `p`), so it fails closed at the ELEMENT gate
    // (`svelte-runtime-unsupported-element`) — BEFORE any content / value-handling
    // classification. (Demoted by the strict-allowlist restructure: a static
    // `<textarea>` interior used to emit; it is now §1.2-out-of-core.) RED against the
    // pre-restructure tree (which accepted `<textarea>` and emitted a clone frame).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea>{c}</textarea><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "textarea"),
    );
}

#[test]
fn select_and_option_elements_fail_closed_at_the_element_allowlist() {
    // `<select>` / `<option>` are NOT in the element allowlist — a component using them
    // fails closed at the element gate (`svelte-runtime-unsupported-element`) on
    // the FIRST out-of-allowlist element (`<select>`), regardless of its interior. RED
    // against the pre-restructure tree (which accepted them).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option>{c}</option></select><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
    );
    // A nested reactive interior is irrelevant — the element gate fires first.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option><b>{c}</b></option></select><button onclick={() => c++}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
    );
}

#[test]
fn static_textarea_content_now_fails_closed_at_the_element_allowlist() {
    // NEGATIVE / demotion proof: even a STATIC-only `<textarea>hi</textarea>` (which
    // the pre-restructure tree serialized verbatim and EMITTED) now fails closed at the
    // element gate — `<textarea>` is out of the finite allowlist, so it has no
    // emission path. The component must NOT emit a Main.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea>hi</textarea><button onclick={() => c++}>x</button>\n",
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
