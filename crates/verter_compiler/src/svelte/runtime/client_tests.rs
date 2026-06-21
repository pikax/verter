//! Integration tests for the Svelte client (`svelte/internal/client`) emission.
//!
//! These drive the full pipeline (parse → lower → plan → topology → emit) and pin
//! the emitted-JS shape against the official `svelte@5.56.3` output captured via
//! the oracle. Each test is discriminating with negative assertions; the
//! fail-closed family asserts the precise typed surface + owning vertical (never a
//! silent empty module, never a panic).

use oxc_allocator::Allocator;

use crate::svelte::parser::parse_svelte;
use crate::svelte::runtime::client::UnsupportedSvelteRuntimeSurface;
use crate::svelte::runtime::{compile_client, ClientCompileError, SvelteRuntimeOptions};

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
    // refused at 5i) but `c` is unused so it stays a pure-static template.
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
fn props_default_referencing_a_sibling_prop_fails_closed_to_5g() {
    // A `$props()` member DEFAULT is the deferral-ledger props-default surface (5g) —
    // the supported props surface is a NO-DEFAULT destructure only. A referencing
    // default (`{ a = 1, b = a }`) is one such demoted shape.
    assert_fail_closed(
        "<script>let { a = 1, b = a } = $props();</script>\n<p>{b}</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() default"),
    );
}

#[test]
fn props_default_referencing_via_no_default_sibling_fails_closed_to_5g() {
    // `let { a, b = a } = $props()` — `b` carries a default, so it is the demoted
    // props-default surface (5g), regardless that the default references a sibling.
    assert_fail_closed(
        "<script>let { a, b = a } = $props();</script>\n<p>{a}</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() default"),
    );
}

#[test]
fn props_non_literal_default_fails_closed_to_5g() {
    // A non-literal `$props()` default (`[]`) is the demoted props-default surface
    // (5g) — like every default, including a constant literal.
    assert_fail_closed(
        "<script>let { a = [] } = $props();</script>\n<p>{a}</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() default"),
    );
}

#[test]
fn props_literal_default_fails_closed_to_5g() {
    // A CONSTANT-LITERAL `$props()` default (`{ a = 1 }`) is ALSO demoted (5g) — the
    // supported props surface is a NO-DEFAULT destructure only (the literal-default
    // flag-3 eager form is a deferral-ledger follow-up). The discriminating negative
    // for the no-default-only rule.
    assert_fail_closed(
        "<script>let { a = 1 } = $props();</script>\n<p>{a}</p>\n",
        "5g",
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
// ── Fail-closed (per surface family, with the right owning vertical) ─────────

/// Assert that `source` fails closed with a surface matching `predicate` whose
/// `owning_block` is `block`.
fn assert_fail_closed(
    source: &str,
    block: &str,
    predicate: impl Fn(&UnsupportedSvelteRuntimeSurface) -> bool,
) {
    match emit_result(source) {
        Err(ClientCompileError::Unsupported(surface)) => {
            assert!(
                predicate(&surface),
                "wrong fail-closed surface: {surface:?} (code {})",
                surface.diagnostic_code()
            );
            assert_eq!(
                surface.owning_block(),
                block,
                "wrong owning vertical for {surface:?}"
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
fn if_block_fails_closed_to_5e() {
    assert_fail_closed(
        "<script>let c = $state(true);</script>\n{#if c}<p>yes</p>{/if}\n",
        "5e",
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
fn each_block_fails_closed_to_5e() {
    // An `{#each}` block is an unsupported control-flow block (5e). `items` is a
    // plain-local array + a trailing reactive `$state` keeps the component
    // runes-mode, so the block gate is the surface (not the state-shape gate).
    assert_fail_closed(
        "<script>let items = [1, 2]; let c = $state(0);</script>\n{#each items as x}<p>{x}</p>{/each}\n<button onclick={() => c++}>{c}</button>\n",
        "5e",
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
fn await_block_fails_closed_to_5e() {
    // An `{#await}` block is an unsupported control-flow block (5e) — fail closed.
    // `p` is a plain-local promise + a trailing reactive `$state` keeps runes-mode,
    // so the block gate is the surface (not the non-primitive state-shape gate).
    assert_fail_closed(
        "<script>let p = Promise.resolve(1); let c = $state(0);</script>\n{#await p}<p>loading</p>{:then v}<p>{v}</p>{/await}\n<button onclick={() => c++}>{c}</button>\n",
        "5e",
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
fn await_expression_in_interpolation_fails_closed_to_5r() {
    // A non-identifier interpolation expression (here an IIFE wrapping an `await`) is
    // the `build_template_chunk` breadth — it fails closed at the complex-interpolation
    // gate (5r) before any async-rewrite gate. Only a bare reactive-signal /
    // no-default-prop identifier read is the supported interpolation surface.
    assert_fail_closed(
        "<script>let p = $state(0); let n = $state(0);</script>\n<button onclick={() => n++}>{(async () => await p)()}</button>\n",
        "5r",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComplexInterpolation { .. }),
    );
}

#[test]
fn capture_event_fails_closed_to_5d() {
    // A CAPTURE-phase event (`onclickcapture`) is a non-delegated event (5d) — fail
    // closed (only modern delegated, non-capture, modifier-free events are
    // supported).
    assert_fail_closed(
        "<script>let n = $state(0);</script>\n<button onclickcapture={() => n++}>x</button>\n",
        "5d",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. }),
    );
}

#[test]
fn legacy_on_modifier_event_fails_closed_to_5d() {
    // A legacy `on:click|stop` directive (a modifier-bearing event) is non-delegated
    // (5d) — fail closed.
    assert_fail_closed(
        "<script>let n = $state(0);</script>\n<button on:click|stop={() => n++}>x</button>\n",
        "5d",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. }),
    );
}

#[test]
fn dynamic_attribute_fails_closed_to_5a() {
    assert_fail_closed(
        "<script>let id = $state('x');</script>\n<div id={id}></div>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { .. }),
    );
}

#[test]
fn class_directive_fails_closed_to_5a() {
    assert_fail_closed(
        "<script>let on = $state(true);</script>\n<div class:active={on}></div>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "class:active"),
    );
}

#[test]
fn html_tag_fails_closed_to_5b() {
    assert_fail_closed(
        "<script>let h = $state('<b>x</b>');</script>\n<div>{@html h}</div>\n",
        "5b",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::SpreadOrHtml { .. }),
    );
}

#[test]
fn spread_fails_closed_to_5b() {
    // A spread attribute (5b); `rest` is a plain-local object + a trailing reactive
    // `$state` keeps runes-mode, so the spread gate is the surface.
    assert_fail_closed(
        "<script>let rest = {}; let c = $state(0);</script>\n<div {...rest}></div>\n<button onclick={() => c++}>{c}</button>\n",
        "5b",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::SpreadOrHtml { .. }),
    );
}

#[test]
fn checked_bind_fails_closed_to_5c() {
    assert_fail_closed(
        "<script>let on = $state(false);</script>\n<input type=\"checkbox\" bind:checked={on} />\n",
        "5c",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "checked"),
    );
}

// ── bind:value member-target ROOT classification (every non-`$state` root) ─────
//
// A `bind:value={member}` is supported ONLY when the member's ROOT identifier
// resolves to a `$state` binding (the value rewrite is then correct). A member
// rooted at a `$props()` prop / a `$bindable` prop / a `$derived` memo / a plain
// local / an imported binding all fail closed (5c) — official emits a distinct
// surface (a `$.prop` flag-7 accessor for a prop, a read-only memo write for a
// derived, …), so accepting them would emit a divergent module.

#[test]
fn bind_value_prop_member_fails_closed_to_5c() {
    // F-α: `bind:value={obj.x}` where `obj` is a `$props()` binding. Official emits
    // `let obj = $.prop($$props,'obj',7)` + `$.bind_value(input, () => obj().x, …)`;
    // Verter would read it off the no-default-prop path (`$$props.obj.x`) — a
    // divergent module. RED against the pre-fix `Member` arm, which accepted ANY
    // member target unconditionally (the prop-bind guard only caught a BARE ident).
    assert_fail_closed(
        "<script>let { obj } = $props();</script>\n<input bind:value={obj.x} />\n",
        "5c",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_aliased_prop_member_fails_closed_to_5c() {
    // F-α: an ALIASED prop local (`{ obj: o }`) bound member `o.x` resolves the
    // same way — the root `o` is a prop, so it fails closed (5c). A coarse
    // name-based check on the source key (`obj`) would miss the alias; the
    // scope-aware root resolution catches it.
    assert_fail_closed(
        "<script>let { obj: o } = $props();</script>\n<input bind:value={o.x} />\n",
        "5c",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_derived_member_fails_closed_to_5g() {
    // A `$derived` is demoted entirely (5g) — a component declaring `$derived` fails
    // at the rune-position gate before the member-bind gate is reached.
    assert_fail_closed(
        "<script>let c = $state(0); let d = $derived({ x: c });</script>\n<input bind:value={d.x} />\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$derived"),
    );
}

#[test]
fn bind_value_plain_local_member_fails_closed_to_5c() {
    // F-α: a member rooted at a PLAIN local (`let o = {...}`, never a rune) is not
    // a reactive surface; the value-bind boundary is `$state`-rooted only, so it
    // fails closed (5c).
    assert_fail_closed(
        "<script>let o = { x: '' }; let c = $state(0);</script>\n<input bind:value={o.x} />\n<button onclick={() => c++}>{c}</button>\n",
        "5c",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn bind_value_import_member_fails_closed_to_5s() {
    // An instance `import` is demoted (5s script-import) — a component with an import
    // fails at the script-hoist gate before the member-bind gate is reached.
    assert_fail_closed(
        "<script>import { store } from './s.js'; let c = $state(0);</script>\n<input bind:value={store.x} />\n<button onclick={() => c++}>{c}</button>\n",
        "5s",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ScriptImport { .. }),
    );
}
// ── form / value-bearing elements demoted by the strict element allowlist ──────
//
// `<select>` / `<option>` / `<datalist>` / `<textarea>` are NOT in the finite
// client-core element allowlist (`a` / `button` / `div` / `h1` / `input` / `p`), so
// a component using ANY of them fails closed at the ELEMENT gate (5a,
// `svelte-runtime-unsupported-element`) on the FIRST out-of-allowlist element —
// regardless of its attrs or interior. (The pre-restructure tree accepted these
// elements and gated only specific attrs; the strict allowlist demotes them whole.)

#[test]
fn select_option_element_fails_closed_at_the_element_allowlist() {
    // A `<select><option value="a">` component fails at the element gate (5a) on the
    // first out-of-allowlist element (`<select>`), not at the `value` attr.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option value=\"a\">A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
    );
}

#[test]
fn select_value_element_fails_closed_at_the_element_allowlist() {
    // `<select value="x">` likewise fails at the element gate (the host element, not
    // the `value`).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select value=\"x\"><option>A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
    );
}

#[test]
fn datalist_element_fails_closed_at_the_element_allowlist() {
    // A `<datalist>` is out of the allowlist — the component fails at the element gate
    // (5a) on `<datalist>`.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<datalist><option value=\"a\">A</option></datalist>\n<button onclick={() => c++}>{c}</button>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "datalist"),
    );
}

#[test]
fn textarea_value_element_now_fails_closed_at_the_element_allowlist() {
    // DEMOTION proof: a static `value` on `<textarea>` USED to serialize verbatim and
    // emit a Main; `<textarea>` is now out of the allowlist, so the component fails
    // closed at the element gate (5a) and emits NO Main.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea value=\"hi\"></textarea>\n<button onclick={() => c++}>{c}</button>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "textarea"),
    );
}

#[test]
fn option_selected_element_now_fails_closed_at_the_element_allowlist() {
    // DEMOTION proof: a static `selected` on `<option>` USED to serialize
    // `selected=""`; `<select>` / `<option>` are now out of the allowlist, so the
    // component fails closed at the element gate (5a) on `<select>`.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option selected>A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
    );
}

// ── static attrs on custom / customized-built-in elements ──────────────────────
//
// A custom element (hyphenated tag) or a customized built-in (`is=`) sets its
// attributes via PROPERTIES at runtime: official omits non-`is` attrs from the
// skeleton and emits `$.set_custom_element_data(node, name, value)`. Verter omits
// the attr from the skeleton (custom-element serializer rule) AND emits no setter
// — the attr silently VANISHES. Fail closed (5h).

#[test]
fn custom_element_static_attr_fails_closed_to_5h() {
    // F-γ: `<my-widget foo="bar">` → official `$.set_custom_element_data(my_widget,
    // 'foo', 'bar')`. RED: Verter dropped `foo` entirely (no skeleton entry, no
    // setter).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<my-widget foo=\"bar\"></my-widget>\n<button onclick={() => c++}>{c}</button>\n",
        "5h",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }),
    );
}

#[test]
fn customized_builtin_static_attr_fails_closed_to_5h() {
    // F-γ: a customized built-in (`is=`) with a non-`is` static attr — official
    // `$.set_custom_element_data(button, 'foo', 'bar')`. Fail closed (5h).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button is=\"my-btn\" foo=\"bar\">x</button>\n<button onclick={() => c++}>{c}</button>\n",
        "5h",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }),
    );
}

#[test]
fn customized_builtin_is_only_now_fails_closed_at_the_element_gate() {
    // DEMOTION proof: a customized built-in with ONLY the `is` attr USED to serialize
    // `is="my-btn"` and emit a Main. Under the strict allowlist, ANY element carrying
    // an `is` attribute is rejected at the element gate (5h, `host-custom-element`)
    // BEFORE the attr walk — so an `is`-only `<button>` now fails closed (no Main).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button is=\"my-btn\">x</button>\n<button onclick={() => c++}>{c}</button>\n",
        "5h",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }),
    );
}

#[test]
fn component_fails_closed_to_5f() {
    // A component reference (a capitalized tag) is the 5f vertical. No import is used
    // (imports are demoted to 5s) so the component node is the surface under test.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<Foo />\n",
        "5f",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { .. }
            )
        },
    );
}

#[test]
fn props_rest_fails_closed_to_5g_not_partial() {
    // A `$props()` REST form fails closed (5g) — it must NOT partially emit.
    assert_fail_closed(
        "<script>let { name, ...rest } = $props();</script>\n<p>{name}</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() rest"),
    );
}

#[test]
fn props_bindable_fails_closed_to_5g() {
    assert_fail_closed(
        "<script>let { value = $bindable(0) } = $props();</script>\n<p>{value}</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn state_raw_fails_closed_to_5g() {
    assert_fail_closed(
        "<script>let c = $state.raw(0);</script>\n<button onclick={() => c = 1}>{c}</button>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.raw"),
    );
}

#[test]
fn legacy_mode_fails_closed_to_5i() {
    // A non-runes component (no rune calls) is legacy mode → 5i.
    assert_fail_closed(
        "<script>export let label;</script>\n<p>{label}</p>\n",
        "5i",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::LegacyMode { .. }),
    );
}

#[test]
fn top_level_style_fails_closed_to_5l() {
    // F4: a top-level `<style>` (CSS scoping) fails closed (5l) — it is NOT accepted
    // as a runtime Main. RED against the pre-fix emitter (which emitted a Main and
    // silently dropped the style / its scoping).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button onclick={() => c++}>{c}</button>\n",
        "5l",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Style { .. }),
    );
}

#[test]
fn svelte_options_custom_element_fails_closed_to_5h() {
    // F4: `<svelte:options customElement>` is the custom-element axis (5h). RED
    // against the pre-fix path (which refused it as the wrong vertical / accepted a
    // Main).
    assert_fail_closed(
        "<svelte:options customElement=\"x-foo\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "5h",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }),
    );
}

#[test]
fn svelte_options_other_axis_fails_closed_to_5m() {
    // F4: a `<svelte:options>` axis beyond name/runes (here `namespace`) is 5m.
    assert_fail_closed(
        "<svelte:options namespace=\"svg\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "5m",
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
fn effect_pre_fails_closed_to_5g() {
    // F4: `$effect.pre(...)` is an advanced rune (5g) — it must fail closed, not
    // emit raw `$effect.pre` (a runtime ReferenceError). RED against the pre-fix
    // path (which emitted raw).
    assert_fail_closed(
        "<script>let c = $state(0); $effect.pre(() => console.log(c));</script>\n<button onclick={() => c++}>{c}</button>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$effect.pre"),
    );
}

#[test]
fn state_snapshot_in_expression_fails_closed_to_5g() {
    // `$state.snapshot(x)` INSIDE an interpolation fails closed (5g) — the
    // unsupported-rune-inside-an-expression case. A primitive `$state` keeps the
    // component out of the object-state gate, so the `$state.snapshot` rune form is
    // the surface under test.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{$state.snapshot(c)}</button>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state.snapshot"),
    );
}

#[test]
fn inspect_rune_fails_closed_to_5g() {
    // F4: `$inspect(...)` is the 5g vertical (prod no-op form not emitted).
    assert_fail_closed(
        "<script>let c = $state(0); $inspect(c);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$inspect"),
    );
}

#[test]
fn host_rune_fails_closed_to_5h() {
    // F4: `$host()` is the custom-element-only API (5h).
    assert_fail_closed(
        "<script>let c = $state(0); const el = $host();</script>\n<button onclick={() => c++}>{c}</button>\n",
        "5h",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::HostOrCustomElement { surface, .. } if *surface == "$host"),
    );
}

#[test]
fn shadowed_rune_name_is_not_refused_as_advanced_rune() {
    // F4 DISCRIMINATION: a function PARAM named like a rune (`function f($inspect) {
    // return $inspect.foo }`) is SHADOWED — its member access is NOT a rune reference,
    // so the rune-form scan does NOT fire (the component is not refused as 5g
    // `AdvancedRune`). The function itself is out-of-allowlist, so it fails closed at
    // the instance-script-item gate (5w, construct `function`), NOT on the rune basis.
    // This pins the precedence: the magic / rune scans (which honor shadowing) own
    // their precise diagnostics; the generic item refusal owns the rest.
    assert_fail_closed(
        "<script>\n\tlet c = $state(0);\n\tfunction f($inspect) { return $inspect.foo; }\n</script>\n<button onclick={() => c++}>{c}</button>\n",
        "5w",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "function"),
    );
}

// ── Position-sensitive bare-rune classification (a bare rune is supported ONLY
//    in its exact legal position; refuse everywhere else) ──────────────────────

#[test]
fn bare_state_in_default_param_fails_closed_to_5g() {
    // A bare `$state(0)` in a function DEFAULT-PARAM position is NOT a supported
    // rune position (the supported `$state` position is the init of a top-level
    // instance-script identifier declarator). It must fail closed (5g), never emit
    // raw `$state(0)` (a runtime ReferenceError). RED against the pre-fix scan,
    // which skipped bare `$state` calls ("they carry their own emission").
    assert_fail_closed(
        "<script>let count=$state(0); function f(x = $state(0)) {}</script>\n<p>hi</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state"),
    );
}

#[test]
fn bare_props_in_call_arg_fails_closed_to_5g() {
    // A bare `$props()` as a CALL ARGUMENT (`console.log($props())`) is not the
    // single supported top-level `$props()` destructure position — fail closed (5g),
    // never emit raw `$props()`. RED against the pre-fix scan.
    assert_fail_closed(
        "<script>console.log($props())</script>\n<p>hi</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props"),
    );
}

#[test]
fn bare_effect_in_function_body_fails_closed_to_5g() {
    // An `$effect(fn)` NESTED in a function body is not a top-level instance-script
    // statement (the supported `$effect` position) — fail closed (5g), never emit
    // raw `$effect(...)`. RED against the pre-fix scan.
    assert_fail_closed(
        "<script>let c=$state(0); function f(){ $effect(() => c); }</script>\n<p>hi</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$effect"),
    );
}

#[test]
fn bare_derived_in_call_arg_fails_closed_to_5g() {
    // A bare `$derived(...)` as a CALL ARGUMENT (`foo($derived(c))`) is not the
    // supported top-level identifier-declarator-init position — fail closed (5g).
    // RED against the pre-fix scan.
    assert_fail_closed(
        "<script>let c=$state(0); foo($derived(c));</script>\n<p>hi</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$derived"),
    );
}

#[test]
fn bare_derived_in_nested_block_fails_closed_to_5g() {
    // A `$derived(...)` declarator nested in a BLOCK statement (`{ let d =
    // $derived(c); }`) is not a TOP-LEVEL declarator — fail closed (5g). Official
    // lowers it; our supported subset is narrower (deferral ledger). RED against
    // the pre-fix scan.
    assert_fail_closed(
        "<script>let c=$state(0); { let d = $derived(c); }</script>\n<p>hi</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$derived"),
    );
}

#[test]
fn bare_rune_identifier_reference_fails_closed_to_5g() {
    // A bare rune-name IDENTIFIER reference (`foo($state)`) — the rune function
    // passed by reference, not called in its supported position — fails closed
    // (5g). RED against the pre-fix scan (which only saw the declarator init).
    assert_fail_closed(
        "<script>let c=$state(0); foo($state);</script>\n<p>hi</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$state"),
    );
}
// ── Module scripts (`<script module>`) — demoted entirely (5s) ─────────────────

#[test]
fn module_script_fails_closed_to_5s_script_import() {
    // A `<script module>` is demoted ENTIRELY (5s script-import) — the module-script
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
        assert_fail_closed(&src, "5s", |s| {
            matches!(s, UnsupportedSvelteRuntimeSurface::ScriptImport { .. })
        });
    }
}

#[test]
fn var_state_declarator_fails_closed_to_5g() {
    // A `var` `$state` declarator is a distinct official surface — a `var` rune read
    // is `$.safe_get(c)` (var hoisting), not `$.get(c)`. Verter does not emit the
    // `$.safe_get` form, so it fails closed (5g) rather than emitting `$.get`. RED
    // against the pre-fix classifier (which accepted `var`/`const` rune declarators).
    assert_fail_closed(
        "<script>var c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "5g",
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
fn const_state_declarator_fails_closed_to_5g_not_static_fold() {
    // A read-only `const` `$state` compiles to an EMPTY reactive topology in
    // official (the value is constant-folded), a divergent surface — fail closed at
    // the decl-kind gate (5g), NOT as a static-interpolation fold (5n). RED against
    // the pre-fix flow (which reached the 5n static-fold check for the `{c}` read).
    assert_fail_closed(
        "<script>let w = $state(0); const c = $state(0);</script>\n<button onclick={() => w++}>{c}{w}</button>\n",
        "5g",
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
fn var_derived_declarator_fails_closed_to_5g() {
    // A `var` `$derived` declarator reads with `$.safe_get` in official — fail closed
    // (5g) rather than emit the `$.get` form Verter produces.
    assert_fail_closed(
        "<script>let c = $state(0); var d = $derived(c * 2);</script>\n<button onclick={() => c++}>{d}</button>\n",
        "5g",
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
fn const_derived_declarator_fails_closed_to_5g() {
    // A `const` `$derived` declarator — even though official reads it with `$.get`,
    // the supported client surface accepts ONLY `let` rune declarators, so it fails
    // closed (5g) until the const/var rune-declarator forms are lowered faithfully.
    assert_fail_closed(
        "<script>let c = $state(0); const d = $derived(c * 2);</script>\n<button onclick={() => c++}>{d}</button>\n",
        "5g",
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
    // `const` / `var` declaration fails closed at the instance-script-item gate (5w,
    // construct `const declaration`). RED against the pre-restructure tree (which
    // emitted `const STEP = 2;` verbatim). The supported `$state` is the rune that keeps
    // the component in runes mode (so the surface under test is the `const`, not the
    // legacy-mode gate).
    assert_fail_closed(
        "<script>let c = $state(0); const STEP = 2;</script>\n<button onclick={() => c++}>{c}</button>\n",
        "5w",
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
        "5q",
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
fn pure_static_text_root_fails_closed_to_5q() {
    // A PURE STATIC-TEXT root (`hello world` as the component root, no wrapping
    // element) is the official text-first topology — official emits `$.next(); var
    // text = $.text('hello world'); $.append(...)` (a `$.text()` NODE root reached
    // via `$.next()`), a distinct emission shape from the `from_html`-clone path.
    // Verter's clone-frame path would emit `var text = root();` where `root` is
    // bound to a `$.text(...)` NODE (not a factory function) → `TypeError: root is
    // not a function` at mount. It fails closed (5q) rather than emit that broken
    // module. RED against the pre-fix tree (which emitted `var root = $.text(...)`
    // followed by `var <region> = root();`).
    assert_fail_closed(
        "<script>let c=$state(0);</script>hello world\n",
        "5q",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::RootTextRegion { .. }),
    );
}

#[test]
fn empty_template_root_fails_closed_to_5q() {
    // An EMPTY template (only a `<script>`, no rendered DOM) compiles in official to
    // a component fn with NO `root()` call / NO `$.append` (the body is just the
    // script lowering). Verter's clone-frame path would synthesise a `$.comment()`
    // root and then call `root()` on that NODE → `TypeError`. It fails closed (5q)
    // rather than emit an undeclared/broken clone frame. RED against the pre-fix
    // tree (which emitted `var root = $.comment();` + `var fragment = root();`).
    assert_fail_closed("<script>let c=$state(0);</script>\n", "5q", |s| {
        matches!(s, UnsupportedSvelteRuntimeSurface::RootTextRegion { .. })
    });
}

#[test]
fn options_runes_with_static_text_root_fails_closed_to_5q() {
    // A `<svelte:options runes />hello` (runes forced via the options element, with
    // a bare static-text root) is the same text-first topology — official emits
    // `$.next(); var text = $.text('hello'); $.append(...)`. It fails closed (5q),
    // never the broken `root()`-on-a-node clone frame.
    assert_fail_closed("<svelte:options runes={true} />hello\n", "5q", |s| {
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
fn second_props_declarator_with_computed_key_fails_closed_to_5g() {
    // `let {a}=$props(), {[k]:b}=$props();` — the first basic destructure must NOT
    // admit the file while the second (a COMPUTED key) slips through and emits a
    // raw prop read. ALL `$props()` declarators are scanned; the computed-key one
    // fails closed (5g). RED against the pre-fix `props_shape`, which returned after
    // the FIRST declarator.
    assert_fail_closed(
        "<script>let k='x'; let {a}=$props(), {[k]:b}=$props();</script>\n<p>{b}</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() computed key"),
    );
}

#[test]
fn second_props_call_whole_object_fails_closed_to_5g() {
    // Two SEPARATE `$props()` statements where the second is a whole-object binding
    // (`let p = $props()`) — the whole-object form fails closed (5g) even though a
    // basic destructure preceded it. RED against scanning only the first.
    assert_fail_closed(
        "<script>let {a}=$props(); let p=$props();</script>\n<p>{a}</p>\n",
        "5g",
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
fn dev_codegen_request_fails_closed_to_5k() {
    // F4: a DEV-MODE codegen request (`dev_codegen: true`) fails closed (5k) — the
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
            assert_eq!(surface.owning_block(), "5k");
            assert_eq!(
                surface.diagnostic_code(),
                "svelte-runtime-unsupported-dev-mode"
            );
        }
        other => panic!("a dev-codegen request must fail closed to DevMode (5k), got: {other:?}"),
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

// ── Additional surface gates (R1, R4, R5, R7, R8) ──────────────────────────────

#[test]
fn destructured_state_object_fails_closed_to_5g_not_panic() {
    // R1: `let { a } = $state({a:1})` MUST fail closed (5g), NEVER reach a panic.
    // Official 5.56.3 supports it (temp + proxy), but full destructured-state
    // lowering is a deferral-ledger item; a clean fail-closed is correct. RED against
    // the prior `unreachable!()` (which PANICKED on this valid input).
    assert_fail_closed(
        "<script>let { a } = $state({ a: 1 });</script>\n<p>{a}</p>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }),
    );
}

#[test]
fn destructured_state_array_fails_closed_to_5g_not_panic() {
    // R1: `let [x] = $state([1])` — the array-destructure form also fails closed.
    assert_fail_closed(
        "<script>let [x] = $state([1]);</script>\n<p>{x}</p>\n",
        "5g",
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
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }),
    );
}

#[test]
fn props_nested_destructure_fails_closed_not_partial() {
    // R7b: a nested `$props()` destructure (`{ a: { b } }`) is rejected by official
    // (`props_invalid_pattern`); Verter fails closed.
    assert_fail_closed(
        "<script>let { a: { b } } = $props();</script>\n<p>{b}</p>\n",
        "5g",
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
fn bind_value_to_call_expression_fails_closed_to_5c() {
    // R8: `bind:value={foo()}` is not a valid lvalue — official raises
    // `bind_invalid_expression`; Verter fails closed (never emits `foo() = $$value`).
    // RED against the prior path (which validated only tag/target, not the
    // bound expression, and emitted invalid JS). The component is runes-mode (a
    // `$state` declarator) so the bind validation is reached (not pre-empted by the
    // legacy-mode refusal).
    assert_fail_closed(
        "<script>let n = $state(0); function foo() { return 1; }</script>\n<input bind:value={foo()} />\n",
        "5c",
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
fn lang_ts_component_with_bind_targets_fails_closed_to_5t() {
    // A `<script lang="ts">` component is demoted ENTIRELY (5t typescript), refused
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
        assert_fail_closed(&src, "5t", |s| {
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
fn binary_constant_interpolation_fails_closed_to_5r() {
    // A `{1 + 1}` interpolation is a non-identifier (binary) expression — the
    // `build_template_chunk` breadth (5r), refused at the complex-interpolation gate.
    // The component is runes-mode (the `$state` declarator) so the legacy refusal
    // does not pre-empt the interpolation classification.
    assert_fail_closed(
        "<script>let n = $state(0);</script>\n<p>{1 + 1}</p>\n<button onclick={() => n++}>{n}</button>\n",
        "5r",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComplexInterpolation { .. }),
    );
}

#[test]
fn non_reactive_const_interpolation_fails_closed_to_5n() {
    // A `{C}` read of an instance-script plain `const` is a bare identifier resolving
    // to a NON-reactive binding — official static-folds it to `textContent`, a
    // distinct topology (5n). A separate reactive `$state` drives the onclick so the
    // component reaches the interpolation classifier.
    assert_fail_closed(
        "<script>let n = $state(0); const C = 5;</script>\n<p>{C}</p>\n<button onclick={() => n++}>{n}</button>\n",
        "5n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::StaticInterpolation { .. }),
    );
}

#[test]
fn never_reassigned_state_interpolation_fails_closed() {
    // A `{n}` read of a `$state` that is NEVER reassigned lowers (in official) to a
    // plain `let n = 5;` and a STATIC `textContent` write, not a reactive op (5n). A
    // SEPARATE reactive `$state` drives the supported onclick (so the component is
    // runes-mode + reactive without reassigning `n`).
    assert_fail_closed(
        "<script>let n = $state(5); let c = $state(0);</script>\n<button onclick={() => c++}>{n}</button>\n",
        "5n",
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
    // instance-script-item gate (5w, `InstanceScriptItem` construct `export`) rather
    // than emitting an `export` inside the component function (invalid JS). RED against
    // the pre-restructure tree (which emitted the `export const` verbatim).
    assert_fail_closed(
        "<script>let n = $state(0); export const helper = 1;</script>\n<button onclick={() => n++}>{n}</button>\n",
        "5w",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "export"),
    );
}

#[test]
fn instance_export_function_fails_closed() {
    // An instance-script `export function` also fails closed at the instance-script-item
    // gate (5w) — an `export`-declaration statement is out-of-allowlist.
    assert_fail_closed(
        "<script>let n = $state(0); export function helper() { return 1; }</script>\n<button onclick={() => n++}>{n}</button>\n",
        "5w",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "export"),
    );
}

#[test]
fn instance_top_level_function_fails_closed() {
    // A plain top-level instance-script FUNCTION (no rune inside) is out-of-allowlist
    // — the supported `onclick` is an inline `$state`-write arrow, so a function
    // (whether a handler reference or a helper) fails closed at the instance-script-item
    // gate (5w, construct `function`). RED against the pre-restructure tree (which
    // lowered the function body verbatim with reactive reads rewritten).
    assert_fail_closed(
        "<script>let count = $state(0); function f(obj) { ({ count } = obj); }</script>\n<button onclick={() => count++}>{count}</button>\n",
        "5w",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "function"),
    );
}

#[test]
fn instance_top_level_class_fails_closed() {
    // A plain top-level instance-script CLASS is out-of-allowlist — fail closed at the
    // instance-script-item gate (5w, construct `class`).
    assert_fail_closed(
        "<script>let count = $state(0); class C { #x = 0; bump() { this.#x++; } }</script>\n<button onclick={() => count++}>{count}</button>\n",
        "5w",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "class"),
    );
}

#[test]
fn multi_declarator_state_with_destructure_fails_closed() {
    // A multi-declarator statement where a LATER declarator destructures `$state`
    // (`let ok = $state(0); let { a } = $state({ a: 1 })`) must fail closed (5g) —
    // the gate scans ALL `$state` declarators, not just the first. RED against the
    // pre-refactor gate (which classified only the first declarator and silently
    // dropped the destructured one → a runtime `ReferenceError` on `a`).
    assert_fail_closed(
        "<script>let ok = $state(0); let { a } = $state({ a: 1 });</script>\n<button onclick={() => ok++}>{ok}{a}</button>\n",
        "5g",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }),
    );
}

#[test]
fn ts_wrapped_update_target_in_handler_fails_closed_to_5d() {
    // An onclick arrow whose body is a TS-wrapped update (`count!++`) is NOT a clean
    // `$state` assignment / update — the update target is a TS-non-null wrapper, not a
    // bare identifier, so the handler-shape gate refuses it (5d). Only a clean
    // `$state` write body is the supported §1.2-class handler.
    assert_fail_closed(
        "<script>let count = $state(0);</script>\n<button onclick={() => { count!++; }}>{count}</button>\n",
        "5d",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. }),
    );
}

#[test]
fn private_field_update_inside_a_class_method_fails_closed() {
    // The private-field passthrough (`this.#x++` inside a class method) is no longer a
    // SUPPORTED surface: a top-level class is out-of-allowlist, so the whole component
    // fails closed at the instance-script-item gate (5w, construct `class`) — a class
    // method body never reaches the rewriter. (The pre-restructure tree lowered the
    // class body verbatim; the class is now §1.2-out-of-core. Covered alongside
    // `instance_top_level_class_fails_closed`.)
    assert_fail_closed(
        "<script>let n = $state(0); class C { #x = 0; bump() { this.#x++; } }</script>\n<button onclick={() => n++}>{n}</button>\n",
        "5w",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, .. } if *construct == "class"),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Identifier-unsafe element tags + special-content-model reactive interior +
// the no-arg `$state()` shadow-robust `void 0` emission.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn custom_element_no_attr_fails_closed_to_5h() {
    // A bare hyphenated CUSTOM element (`<my-widget></my-widget>`, no attributes) is
    // already in the demote list (the official compiler clones it via `importNode`
    // and sets its attributes via `$.set_custom_element_data` — web-components
    // breadth). A custom element with an UNSUPPORTED attribute already fails closed
    // (5h); the no-attribute case was leaking through the element classifier and
    // being emitted. It must fail closed at the custom-element owner (5h), never an
    // accepted Main. RED against the pre-fix tree (which emitted a `from_html`
    // `var fragment = root()` clone for it).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<my-widget></my-widget>\n",
        "5h",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::HostOrCustomElement { .. }
            )
        },
    );
}

#[test]
fn reserved_word_element_tag_fails_closed_to_5v_not_invalid_js() {
    // A reserved-word HTML tag (`<var>`) whose synthesized DOM local var name would
    // be the reserved word `var` is accepted-and-emitted as `var var = root();` —
    // INVALID JS (a `SyntaxError`). The official compiler collision-renames the local
    // (`var_1`), which is naming breadth, not the §1.2-class core. It must fail closed
    // at the element-naming owner (5v), never emit invalid JS. RED against the pre-fix
    // tree (which emitted `var var = root();`).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<var></var>\n",
        "5v",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ElementName { .. }),
    );
}

#[test]
fn reserved_word_class_element_tag_fails_closed_to_5v() {
    // `<class>` → `var class = root();` is likewise invalid JS — fail closed (5v).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<class></class>\n",
        "5v",
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
    // / `div` / `h1` / `input` / `p`), so it fails closed at the ELEMENT gate (5a,
    // `svelte-runtime-unsupported-element`) — BEFORE any content / value-handling
    // classification. (Demoted by the strict-allowlist restructure: a static
    // `<textarea>` interior used to emit; it is now §1.2-out-of-core.) RED against the
    // pre-restructure tree (which accepted `<textarea>` and emitted a clone frame).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea>{c}</textarea><button onclick={() => c++}>x</button>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "textarea"),
    );
}

#[test]
fn select_and_option_elements_fail_closed_at_the_element_allowlist() {
    // `<select>` / `<option>` are NOT in the element allowlist — a component using them
    // fails closed at the element gate (5a, `svelte-runtime-unsupported-element`) on
    // the FIRST out-of-allowlist element (`<select>`), regardless of its interior. RED
    // against the pre-restructure tree (which accepted them).
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option>{c}</option></select><button onclick={() => c++}>x</button>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
    );
    // A nested reactive interior is irrelevant — the element gate fires first.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select><option><b>{c}</b></option></select><button onclick={() => c++}>x</button>\n",
        "5a",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "select"),
    );
}

#[test]
fn static_textarea_content_now_fails_closed_at_the_element_allowlist() {
    // NEGATIVE / demotion proof: even a STATIC-only `<textarea>hi</textarea>` (which
    // the pre-restructure tree serialized verbatim and EMITTED) now fails closed at the
    // element gate (5a) — `<textarea>` is out of the finite allowlist, so it has no
    // emission path. The component must NOT emit a Main.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea>hi</textarea><button onclick={() => c++}>x</button>\n",
        "5a",
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
    out.replace(",)", ")").replace(",]", "]").replace(",}", "}")
}
