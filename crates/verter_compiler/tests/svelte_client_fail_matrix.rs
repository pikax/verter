//! The FAIL MATRIX — the exhaustive enumeration of fail-closed client sub-shapes.
//!
//! Each row is a MINIMAL valid-Svelte component exercising ONE out-of-boundary
//! sub-shape; the gate asserts it FAILS CLOSED with the EXACT machine-stable
//! diagnostic id and emits NO `Main` module (never a silent empty module, never a
//! panic). This is the negative half of the convergence gate: a row fails if the
//! emission is ever accepted (a divergent Main) or fails closed with the wrong
//! diagnostic. Adding a row is trivial — append a `(name, source, code)` tuple.
//!
//! Most rows carry a `$state` so the component is in RUNES mode (a runeless
//! component fails closed at the legacy-mode gate first, before the surface under
//! test is reached).

use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::{compile_client, ClientCompileError, SvelteRuntimeOptions};

/// One fail-matrix row: a name, the component source, and the expected EXACT
/// machine-stable diagnostic id (`UnsupportedSvelteRuntimeSurface::diagnostic_code`).
///
/// Pinning the exact `code` — not merely the `svelte-runtime-unsupported-` prefix —
/// catches a refusal-arm drift: a row that silently changes its refusal arm (e.g. a
/// `bind:value` that starts refusing as a `DynamicAttribute` instead of a `Binding`)
/// changes its code, which the equality gate flags.
struct FailRow {
    /// The row name (the sub-shape under test).
    name: &'static str,
    /// The component source.
    source: &'static str,
    /// The expected EXACT diagnostic id (`svelte-runtime-unsupported-<surface>`).
    code: &'static str,
}

/// Compile a source, returning the typed compile result.
fn compile(source: &str) -> Result<String, ClientCompileError> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false).map(|m| m.code)
}

/// The FAIL MATRIX rows — every fail-closed sub-shape per the supported boundary.
const FAIL_MATRIX: &[FailRow] = &[
    // ── $state advanced forms ───────────────────────────────────────────
    FailRow {
        name: "state_raw",
        source: "<script>let c = $state.raw(0);</script>\n<button onclick={() => c = 1}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "state_destructure",
        source: "<script>let { a } = $state({ a: 1 });</script>\n<button onclick={() => a}>{a}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "state_snapshot",
        source: "<script>let c = $state(0); let s = $state.snapshot(c);</script>\n<button onclick={() => c++}>{s}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A module script is demoted entirely (script-import) — refused before the
        // module-rune shape gate.
        name: "state_module",
        source: "<script module>let c = $state(0);</script>\n<script>let d = $state(0);</script>\n<button onclick={() => d++}>{d}</button>\n",
        code: "svelte-runtime-unsupported-script-import",
    },
    FailRow {
        name: "state_nested_position",
        source: "<script>function f() { let c = $state(0); return c; }</script>\n<button onclick={() => f()}>x</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // `$state` accepts ZERO or ONE argument; a second argument is the official
        // `rune_invalid_arguments_length` compile error ("$state must be called with
        // zero or one arguments"). Verter previously ACCEPTED `$state(0, 1)` and
        // silently dropped the 2nd arg (`$.state(0)`) — now it fails closed.
        name: "state_extra_args",
        source: "<script>let c = $state(0, 1);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // The same arity rule for `$state.raw` (also "zero or one arguments").
        name: "state_raw_extra_args",
        source: "<script>let w = $state(0); let c = $state.raw(0, 1);</script>\n<button onclick={() => w++}>{c}{w}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A top-level `function r() { … }` is out-of-allowlist — it fails closed at the
        // instance-script-item gate (construct `function`) BEFORE its body's
        // destructuring write is ever lowered (the broad statement-rewrite path is
        // gone). The destructuring-write refusal now only fires for a write in a
        // SUPPORTED expression position (a template handler), which the supported
        // handler shape — a `$state`-write arrow — never contains.
        name: "instance_top_level_function",
        source: "<script>let c = $state(0); function r() { ({ c } = { c: 1 }); }</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-instance-script-item",
    },
    // ── $derived advanced forms ──────────────────────────────────────────────
    FailRow {
        name: "derived_destructure",
        source: "<script>let c = $state(0); let { x } = $derived({ x: c });</script>\n<button onclick={() => c++}>{x}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "derived_by_call",
        source: "<script>let d = $derived.by(123);</script>\n<p>{d}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // `$derived` is demoted entirely — refused before the async-rewrite gate.
        name: "derived_await",
        source: "<script>let c = $state(0); let d = $derived(await Promise.resolve(c));</script>\n<button onclick={() => c++}>{d}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    // ── non-`let` rune DECLARATION KIND ─────────────────────────────────
    // Official preserves the keyword but selects a DIFFERENT read helper: a `var`
    // rune read is `$.safe_get` (var hoisting), not `$.get`; a read-only `const
    // $state` compiles to an EMPTY reactive topology. Only a `let` rune declarator
    // is supported; `var`/`const` rune declarators fail closed.
    FailRow {
        name: "state_var",
        source: "<script>var c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "state_const",
        source: "<script>let w = $state(0); const c = $state(0);</script>\n<button onclick={() => w++}>{c}{w}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "derived_var",
        source: "<script>let c = $state(0); var d = $derived(c * 2);</script>\n<button onclick={() => c++}>{d}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "derived_const",
        source: "<script>let c = $state(0); const d = $derived(c * 2);</script>\n<button onclick={() => c++}>{d}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_var",
        source: "<script>let c = $state(0); var { a } = $props();</script>\n<button onclick={() => c++}>{a}{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    // ── $effect advanced forms ───────────────────────────────────────────────
    FailRow {
        name: "effect_nested",
        source: "<script>let c = $state(0); function f() { $effect(() => c); }</script>\n<button onclick={f}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "effect_pre",
        source: "<script>let c = $state(0); $effect.pre(() => { c; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // `$effect` is demoted entirely — refused before the async-rewrite gate.
        name: "effect_async",
        source: "<script>let c = $state(0); $effect(async () => { await c; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    // ── $props() advanced forms + bound ────────────────────────────
    FailRow {
        name: "props_duplicate",
        source: "<script>let { a } = $props(); let { b } = $props();</script>\n<p>{a}{b}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // `$props()` accepts ZERO arguments; ANY argument is the official
        // `rune_invalid_arguments` compile error ("$props cannot be called with
        // arguments"). Verter previously ACCEPTED `$props(1)` and emitted the prop
        // reads regardless — now it fails closed.
        name: "props_extra_args",
        source: "<script>let { a } = $props(1);</script>\n<p>{a}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_rest",
        source: "<script>let { a, ...rest } = $props();</script>\n<p>{a}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_whole",
        source: "<script>let p = $props();</script>\n<p>{p.a}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_nested",
        source: "<script>let { a: { b } } = $props();</script>\n<p>{b}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_computed",
        source: "<script>let k = 'x'; let { [k]: a } = $props();</script>\n<p>{a}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_numeric",
        source: "<script>let { 0: zero } = $props();</script>\n<p>{zero}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_bindable",
        source: "<script>let { value = $bindable(0) } = $props();</script>\n<p>{value}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_ref_default",
        source: "<script>let { a = 1, b = a } = $props();</script>\n<p>{a}{b}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_array_default",
        source: "<script>let { a = [] } = $props();</script>\n<p>{a}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A prop WRITE in the instance script (no default — so the prop-usage gate, not
        // the default gate, is the surface). The onclick is a supported `$state` arrow.
        name: "props_written",
        source: "<script>let { a } = $props(); let c = $state(0); function bump() { a += 1; }</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_bound",
        source: "<script>let { label } = $props();</script>\n<input bind:value={label} />\n<p>{label}</p>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    // ── binds ────────────────────────────────────────────────────────────
    FailRow {
        name: "bind_checked",
        source: "<script>let on = $state(false);</script>\n<input type=\"checkbox\" bind:checked={on} />\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_value_prop",
        source: "<script>let { label } = $props();</script>\n<input bind:value={label} />\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_value_prop_member",
        source: "<script>let { obj } = $props();</script>\n<input bind:value={obj.x} />\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_value_prop_member_aliased",
        source: "<script>let { obj: o } = $props();</script>\n<input bind:value={o.x} />\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        // `$derived` is demoted entirely — refused before the member-bind gate.
        name: "bind_value_derived_member",
        source: "<script>let c = $state(0); let d = $derived({ x: c });</script>\n<input bind:value={d.x} />\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "bind_value_plain_local_member",
        source: "<script>let o = { x: '' }; let c = $state(0);</script>\n<input bind:value={o.x} />\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        // An instance import is demoted — refused before the member-bind gate.
        name: "bind_value_import_member",
        source: "<script>import { store } from './s.js'; let c = $state(0);</script>\n<input bind:value={store.x} />\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-script-import",
    },
    FailRow {
        name: "bind_value_call",
        source: "<script>let c = $state(0); function f() { return c; }</script>\n<input bind:value={f()} />\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_value_sequence_pair",
        source: "<script>let c = $state(0);</script>\n<input bind:value={() => c, (v) => c = v} />\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        // A member `bind:this={refs[0]}` (a plain-local array, so the member-bind gate
        // fires, not the state-shape gate) is the deferral-ledger member-bind form.
        name: "bind_this_member",
        source: "<script>let refs = []; let c = $state(0);</script>\n<div bind:this={refs[0]}></div>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        // A component reference (a capitalized tag) is the component surface; no import is
        // used (imports are demoted) so the component node is the surface under test.
        name: "bind_this_component",
        source: "<script>let c = $state(0);</script>\n<Child bind:this={c} />\n",
        code: "svelte-runtime-unsupported-component",
    },
    // ── events ──────────────────────────────────────────────────────────
    FailRow {
        name: "event_call",
        source: "<script>let c = $state(0); function f(x) { return x; }</script>\n<button onclick={f(c)}>x</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_update",
        source: "<script>let c = $state(0);</script>\n<button onclick={c++}>x</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_assignment",
        source: "<script>let c = $state(0);</script>\n<button onclick={c = 1}>x</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_member",
        source: "<script>let c = $state(0); let obj = { fn() {} };</script>\n<button onclick={obj.fn}>{c}</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_sequence",
        source: "<script>let c = $state(0); function a() {} function b() {}</script>\n<button onclick={(a(), b())}>{c}</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_conditional",
        source: "<script>let x = $state(true); function a() {} function b() {}</script>\n<button onclick={x ? a : b}>x</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        // A bare-identifier handler (not an inline arrow) is the wrapper form; no
        // import is used (imports are demoted), so the handler shape is the surface.
        name: "event_import_identifier",
        source: "<script>let c = $state(0);</script>\n<button onclick={handler}>{c}</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_legacy_modifier",
        source: "<script>let c = $state(0);</script>\n<button on:click|preventDefault={() => c++}>x</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_capture",
        source: "<script>let c = $state(0);</script>\n<button onclickcapture={() => c++}>x</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_nondelegated",
        source: "<script>let c = $state(0);</script>\n<input onfocus={() => c++} />\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_async",
        source: "<script>let c = $state(0);</script>\n<button onclick={async () => { await c; }}>x</button>\n",
        code: "svelte-runtime-unsupported-experimental-async",
    },
    // ── text (static-interpolation / async / root-text) ─────────────────────
    FailRow {
        name: "text_static_interp",
        source: "<script>let c = $state(0); const k = 1;</script>\n<button onclick={() => c++}>{k}</button>\n",
        code: "svelte-runtime-unsupported-static-interpolation",
    },
    FailRow {
        // An `await` interpolation is a non-identifier expression — the
        // `build_template_chunk` breadth, refused before the async-rewrite gate.
        name: "text_await",
        source: "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{await Promise.resolve(c)}</button>\n",
        code: "svelte-runtime-unsupported-complex-interpolation",
    },
    FailRow {
        name: "root_static_text",
        source: "<script>let c = $state(0);</script>\nhello world\n",
        code: "svelte-runtime-unsupported-root-text-region",
    },
    FailRow {
        // A REACTIVE bare interpolation as the component root (a top-level reassignment
        // makes `c` a signal, so the interp-shape gate accepts it and the root-region
        // gate fires the text-first root-text refusal). A non-reactive root read would fail at
        // the static-interpolation gate first.
        name: "root_reactive_text",
        source: "<script>let c = $state(0); c = 1;</script>\n{c}\n",
        code: "svelte-runtime-unsupported-root-text-region",
    },
    FailRow {
        // An empty template (no rendered root) — the comment-anchor root shape. A
        // top-level `$state` reassignment keeps it runes-mode without a `$effect` (which
        // is now demoted as an advanced rune).
        name: "empty_root",
        source: "<script>let c = $state(0); c = 1;</script>\n",
        code: "svelte-runtime-unsupported-root-text-region",
    },
    // ── template / attrs ────────────────────────────────────────────────
    //
    // The client backend NOW SUPPORTS dynamic attributes, mixed attributes, boolean DOM props,
    // `class={…}` / `class:`, and `style={…}` / `style:` — those rows were removed
    // from this fail matrix (they emit a `Main`, asserted by the positive cases in
    // `svelte_element_attr_boundary.rs`). What stays fail-closed here is the form-
    // control setter family (`value` / `checked` → bindings) and the spread / `{@html}`
    // surfaces, plus the `dir` reflected-attr quirk (deferred).
    FailRow {
        // `value={v}` (a form-control setter) is the bindings-breadth surface —
        // it emits `$.set_value`, not a generic attribute. Refused through the binding
        // form-control / bindings channel. (The `dir` reflected-attr deferral has its
        // own `dir_attr` row further down.)
        name: "form_control_value_attr",
        source: "<script>let v = $state('x');</script>\n<input onclick={() => v += '!'} value={v}>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        // A `{...rest}` whose `rest` is a `$props()` REST destructure is the rest-props
        // surface (`$.rest_props` + `rest_excludes`), which the script-shape gate rejects
        // — an ADVANCED RUNE, NOT an element spread (an ordinary element spread is
        // supported; the rest-props DESTRUCTURE in the script is the unsupported part).
        name: "props_rest_spread",
        source: "<script>let { a, ...rest } = $props()</script>\n<div {...rest}></div>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // An element spread CO-LOCATED with a (non-delegated) event handler. Official folds
        // `{ ...x, onclick: f }`; Verter fails closed at the spread-incompatible-attr gate
        // (`refuse_spread_incompatible_attr`, exhaustive non-wildcard match) — the event never
        // reaches the attr-skip, so the refusal cannot silently regress to a drop. This row
        // locks the EXACT diagnostic identity of that refusal.
        name: "spread_with_event",
        source: "<script>let c = $state(0);</script>\n<div {...c} onclick={() => c++}></div>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        // An element spread co-located with a `bind:` binding — fails closed with the binding
        // diagnostic, NOT folded.
        name: "spread_with_bind",
        source: "<script>let c = $state(0);</script>\n<input {...c} bind:value={c} />\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        // An element spread co-located with a `use:` action directive — fails closed with the
        // component/snippet (directive) diagnostic, NOT folded.
        name: "spread_with_use",
        source: "<script>let c = $state(0); function act() {}</script>\n<div {...c} use:act></div>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // A HYPHENATED custom element is refused as a CUSTOM ELEMENT at the tag level
        // — the element classifier fails closed on the tag BEFORE the attr walk,
        // so the custom-element owner (not the dynamic-attribute owner) is reported,
        // whether the attribute is dynamic, static, or absent.
        name: "custom_element_attr",
        source: "<script>let c = $state(0);</script>\n<my-widget foo={c}></my-widget>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        name: "custom_element_static_attr",
        source: "<script>let c = $state(0);</script>\n<my-widget foo=\"bar\"></my-widget>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // A bare hyphenated custom element with NO attributes — the no-attribute leak
        // (was emitted as a `from_html` clone). Fails closed at the custom-element
        // owner.
        name: "custom_element_no_attr",
        source: "<script>let c = $state(0); c = 1;</script>\n<my-widget></my-widget>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        name: "customized_builtin_static_attr",
        source: "<script>let c = $state(0);</script>\n<button is=\"my-btn\" foo=\"bar\">x</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    // ── identifier-unsafe / reserved-word element tags ──────────────────
    // A reserved-word HTML tag whose synthesized DOM local var name would be the
    // reserved word (`var var = root();` / `var class = …`) is invalid JS — the
    // official compiler collision-renames it (`var_1` / `class_1`), naming breadth.
    FailRow {
        name: "element_tag_var",
        source: "<script>let c = $state(0);</script>\n<var></var>\n",
        code: "svelte-runtime-unsupported-element-name",
    },
    FailRow {
        name: "element_tag_class",
        source: "<script>let c = $state(0);</script>\n<class></class>\n",
        code: "svelte-runtime-unsupported-element-name",
    },
    // ── out-of-allowlist intrinsic elements (the strict element allowlist) ───
    // The client core emits ONLY `a` / `button` / `div` / `h1` / `input` / `p`; every
    // other intrinsic tag fails closed at the element gate (`svelte-runtime-unsupported-
    // element`) — BEFORE any attr / content classification, so the interior / attrs are
    // irrelevant to the refusal. (The exhaustive HTML-tag-universe cover is the
    // dedicated ELEMENT MATRIX in `svelte_element_attr_boundary.rs`; these rows pin a
    // representative spread of the demoted special-content / value-bearing tags.)
    FailRow {
        name: "select_option_value_element",
        source: "<script>let c = $state(0);</script>\n<select><option value=\"a\">A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-element",
    },
    FailRow {
        name: "datalist_element",
        source: "<script>let c = $state(0);</script>\n<datalist><option value=\"a\">A</option></datalist>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-element",
    },
    // (`<video>` joined the element allowlist as the `muted` media host,
    // so it is no longer a fail-closed element — a `<video muted>` emits the
    // `video.muted = true` property write. Its support is asserted by the positive
    // element/attr boundary cases.)
    FailRow {
        name: "textarea_dynamic_content_element",
        source: "<script>let c = $state(0);</script>\n<textarea>{c}</textarea><button onclick={() => c++}>x</button>\n",
        code: "svelte-runtime-unsupported-element",
    },
    FailRow {
        name: "textarea_static_content_element",
        source: "<script>let c = $state(0);</script>\n<textarea>hi</textarea><button onclick={() => c++}>x</button>\n",
        code: "svelte-runtime-unsupported-element",
    },
    FailRow {
        // A raw `<slot>` is rejected at the element gate (Verter's parser does not model
        // the official `SlotElement`, so it must never reach intrinsic emission).
        name: "raw_slot_element",
        source: "<script>let c = $state(0); c = 1;</script>\n<slot></slot>\n",
        code: "svelte-runtime-unsupported-element",
    },
    FailRow {
        // A common flow tag outside the allowlist (`<span>`) fails closed — the
        // allowlist is finite, not "any identifier-safe tag".
        name: "span_element",
        source: "<script>let c = $state(0);</script>\n<span>{c}</span><button onclick={() => c++}>x</button>\n",
        code: "svelte-runtime-unsupported-element",
    },
    FailRow {
        name: "svg_root",
        source: "<script>let c = $state(0);</script>\n<svg onclick={() => c++}><circle r=\"5\" /></svg>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    // (`autofocus` is a NON-STATIC-PROPERTY supported — `$.autofocus(input,
    // true)` for a static valueless form, `$.autofocus(input, $.get(v))` for a dynamic
    // one — so it is no longer fail-closed. Its support is asserted by the positive
    // boundary cases.)
    FailRow {
        name: "dir_attr",
        source: "<script>let c = $state(0);</script>\n<div dir=\"ltr\"><button onclick={() => c++}>{c}</button></div>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // `defaultValue` / `defaultChecked` (Svelte `NON_STATIC_PROPERTIES`) on an
        // allowlisted `<input>` fail closed BEFORE emission (the root-cause leak: the
        // pre-restructure tree ACCEPTED them at classification then DROPPED them at
        // serialization, emitting a divergent skeleton). Now the static-attr allowlist
        // rejects them at the attr gate.
        name: "input_default_value_attr",
        source: "<script>let c = $state(0);</script>\n<input defaultValue=\"x\" />\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        name: "input_default_checked_attr",
        source: "<script>let c = $state(0);</script>\n<input defaultChecked />\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A forbidden global attr on an allowlisted element (`style`) fails closed at
        // the attr gate.
        name: "style_static_attr",
        source: "<script>let c = $state(0);</script>\n<div style=\"color:red\"><button onclick={() => c++}>{c}</button></div>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // An empty `class=\"\"` is NOT a serializable static attr (the allowlist accepts
        // `class` ONLY with a non-empty value) — fail closed at the attr gate.
        name: "empty_class_static_attr",
        source: "<script>let c = $state(0);</script>\n<div class=\"\"><button onclick={() => c++}>{c}</button></div>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    // ── duplicate attributes / directives ───────────────────────────────
    // NOTE: a DUPLICATE attribute / directive (`<div id id>`, `bind:value bind:value`,
    // `class:active class:active`, …) is an OFFICIAL EXACT-CODE parse error
    // (`attribute_duplicate`, minted by the parser's open-tag attribute loop), so it now fails
    // closed through the official-reject gate (`ClientCompileError::OfficialReject`), NOT this
    // unsupported-feature matrix — its parity rows live in
    // `svelte_client_official_reject_matrix.rs` (`attribute_duplicate_id`,
    // `script_attribute_duplicate_lang`) and the exact-code rail
    // (`svelte_parse_defect_exact_codes`).
    // ── instance-script-item allowlist — non-allowlist top-level items ──────
    FailRow {
        // An instance-script `export` is out-of-allowlist (the three shapes are a
        // `$state(<primitive>)`, a no-default `$props()` destructure, a bare `let el;`).
        name: "instance_export",
        source: "<script>let c = $state(0); export const FOO = 1;</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-instance-script-item",
    },
    FailRow {
        // A plain non-rune `let x = 0` is out-of-allowlist (a template read is only a
        // reactive `$state` signal or a no-default prop, never a plain local).
        name: "instance_plain_let",
        source: "<script>let c = $state(0); let x = 0;</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-instance-script-item",
    },
    FailRow {
        // A plain `const` declaration is out-of-allowlist (the supported shapes are
        // `let`-only).
        name: "instance_const_decl",
        source: "<script>let c = $state(0); const STEP = 2;</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-instance-script-item",
    },
    FailRow {
        // A top-level `$:` reactive label is out-of-allowlist (legacy reactivity; the
        // official compiler rejects it in runes mode).
        name: "instance_reactive_label",
        source: "<script>let count = $state(0);\n$: doubled = count * 2;</script>\n<button onclick={() => count++}>{count}</button>\n",
        code: "svelte-runtime-unsupported-instance-script-item",
    },
    // NOTE: a plain top-level `enum` (or a typed `let c: number = …`) in a plain (JS)
    // `<script>` is TS-only syntax upstream parses with Acorn and REJECTS as `js_parse_error`,
    // so it now fails closed through the official-reject gate (the body-probe), NOT this
    // unsupported-feature matrix — its parity is the `ScriptBodyParse` class
    // (`svelte_client_official_reject_matrix.rs` + `svelte_parse_defect_exact_codes`).
    // NOTE: a `$`/`$$`-prefixed user binding (`let $$anchor = 1`) and the magic-object
    // references `$$props` / `$$restProps` are OFFICIAL REJECTS (the official compiler
    // compile-errors them — `dollar_prefix_invalid` / `legacy_props_invalid` /
    // `legacy_rest_props_invalid`), so they now fail closed through the official-reject
    // gate (`ClientCompileError::OfficialReject`), NOT this unsupported-feature matrix —
    // their parity rows live in `svelte_client_official_reject_matrix.rs`. Only
    // `$$slots`, which official ACCEPTS, remains a deferrable unsupported-FEATURE
    // refusal here (the magic-identifier instance-script surface).
    // ── magic identifiers — auto-injected legacy objects ────────────────────
    FailRow {
        // `$$slots` referenced in the instance script. Official ACCEPTS `$$slots` (it
        // is a valid auto-injected magic object), but Verter does not yet synthesize
        // it — a raw reference would bind an undefined identifier — so it fails closed
        // as an unsupported FEATURE, a deferrable refusal (official-accepted).
        name: "magic_dollar_slots",
        source: "<script>let count = $state(0); let s = $$slots;</script>\n<button onclick={() => count++}>{count}</button>\n",
        code: "svelte-runtime-unsupported-magic-identifier",
    },
    // ── structure (component) ─────────────────────────────────────────────
    FailRow {
        name: "block_if",
        source: "<script>let c = $state(true);</script>\n{#if c}<p>yes</p>{/if}\n",
        code: "svelte-runtime-unsupported-block",
    },
    FailRow {
        // An `{#each}` block; `items` is a plain-local array (not array-state) and
        // a trailing `$state` keeps the component runes-mode, so the block gate is the
        // surface under test.
        name: "block_each",
        source: "<script>let items = [1, 2]; let c = $state(0);</script>\n{#each items as x}<p>{x}</p>{/each}\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-block",
    },
    FailRow {
        // A component reference (a capitalized tag) is the component surface; no import is
        // used (imports are demoted) so the component node is the surface.
        name: "component",
        source: "<script>let c = $state(0);</script>\n<Foo />\n",
        code: "svelte-runtime-unsupported-component",
    },
    // ── Complex interpolations are fail-closed (complex-interpolation surface) ─
    FailRow {
        name: "interp_logical",
        source: "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count || 0} x{count}</button>\n",
        code: "svelte-runtime-unsupported-complex-interpolation",
    },
    FailRow {
        name: "interp_binary",
        source: "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count + 1}</button>\n",
        code: "svelte-runtime-unsupported-complex-interpolation",
    },
    FailRow {
        name: "interp_optional_call",
        source: "<script>let count = $state(0); let f = $state(null);</script>\n<button onclick={() => count++}>{f?.(count)}</button>\n",
        code: "svelte-runtime-unsupported-complex-interpolation",
    },
    FailRow {
        name: "interp_call",
        source: "<script>let count = $state(0); let f = $state(null);</script>\n<button onclick={() => count++}>{f(count)}</button>\n",
        code: "svelte-runtime-unsupported-complex-interpolation",
    },
    FailRow {
        name: "interp_member",
        source: "<script>let count = $state(0); let obj = $state(null);</script>\n<button onclick={() => count++}>{obj?.x}</button>\n",
        code: "svelte-runtime-unsupported-complex-interpolation",
    },
    FailRow {
        name: "interp_conditional",
        source: "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count ? count : 0}</button>\n",
        code: "svelte-runtime-unsupported-complex-interpolation",
    },
    // ── CONVERGENCE: $state primitive-only + $props default ─────────────────
    FailRow {
        name: "state_object_init",
        source: "<script>let o = $state({}); let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "state_array_init",
        source: "<script>let a = $state([]); let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "props_literal_default",
        source: "<script>let { a = 1 } = $props();</script>\n<p>{a}</p>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "derived_simple",
        source: "<script>let c = $state(0); let d = $derived(c + 1);</script>\n<button onclick={() => c++}>{d}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "effect_toplevel",
        source: "<script>let c = $state(0); $effect(() => { c; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "prop_read_instance_script",
        source: "<script>let { cb } = $props(); let c = $state(0); cb();</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "prop_write_in_if",
        source: "<script>let { a } = $props(); let c = $state(0); function r() { if (c) a = 2; }</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    // ── CONVERGENCE: demoted event-handler shapes ───────────────────────────
    FailRow {
        name: "event_function_expr",
        source: "<script>let c = $state(0);</script>\n<button onclick={function () { c++; }}>{c}</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_local_function_ident",
        source: "<script>let c = $state(0); function inc() { c++; }</script>\n<button onclick={inc}>{c}</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_arrow_call_body",
        source: "<script>let c = $state(0); function f(x) { return x; }</script>\n<button onclick={() => f(c)}>{c}</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    // ── CONVERGENCE: demoted bind shapes ────────────────────────────────────
    FailRow {
        name: "bind_value_plain_local",
        source: "<script>let plain = 'x'; let c = $state(0);</script>\n<input bind:value={plain} />\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    // ── CONVERGENCE: script imports / module scripts ────────────────────────
    FailRow {
        name: "instance_import",
        source: "<script>import { x } from './x.js'; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-script-import",
    },
    FailRow {
        name: "module_script",
        source: "<script module>const K = 1;</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-script-import",
    },
    // ── CONVERGENCE: <script lang=ts> ───────────────────────────────────────
    FailRow {
        name: "lang_ts",
        source: "<script lang=\"ts\">let count: number = $state(0);</script>\n<button onclick={() => count++}>{count}</button>\n",
        code: "svelte-runtime-unsupported-typescript",
    },
    // ── CONVERGENCE: complex text chunks ────────────────────────────────────
    FailRow {
        name: "text_entity_run",
        source: "<script>let c = $state(0);</script>\n<button onclick={() => c++}>A &amp; {c}</button>\n",
        code: "svelte-runtime-unsupported-complex-text",
    },
    FailRow {
        name: "text_repeated_space_run",
        source: "<script>let c = $state(0);</script>\n<button onclick={() => c++}>a  b {c}</button>\n",
        code: "svelte-runtime-unsupported-complex-text",
    },
    // ── free / undeclared bind:this target ──────────────────────────────────
    // A free `bind:this={button}` (no declared local) is official-accepted but outside
    // the §1.2 core (the shape-3 target is a DECLARED `let el;`); it fails closed
    // so the free target never collides with the synthesized `<button>` DOM local.
    FailRow {
        name: "bind_this_free_target",
        source: "<script>let c = $state(0);</script>\n<button bind:this={button}>x</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_this_undeclared_target",
        source: "<script>let c = $state(0);</script>\n<div bind:this={missing}></div>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    // ── $state over a SHADOWED `undefined` ──────────────────────────────────
    // `let undefined = $state(0); let x = $state(undefined)` — the shadowed `undefined`
    // is a non-literal reference init (official reads the shadow), so it fails closed as
    // an advanced rune rather than emitting the divergent `$.state(undefined)`.
    FailRow {
        name: "state_shadowed_undefined_init",
        source: "<script>let undefined = $state(0); let x = $state(undefined);</script>\n<button onclick={() => { undefined++; x++; }}>{x}{undefined}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "state_nan_init",
        source: "<script>let c = $state(NaN);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    // ── implicit `<p>` autoclose ────────────────────────────────────────────
    // `<p><div>x</div>` with NO explicit `</p>` is official-ACCEPTED (the browser
    // auto-closes the `<p>`); modeling the autoclose DOM re-parenting is outside the
    // §1.2 core, so it fails closed as an unsupported FEATURE — never an emitted
    // (wrong) Main, never an `element_unclosed` reject.
    FailRow {
        name: "paragraph_autoclose_div_implicit",
        source: "<script>let c = $state(0);</script>\n<p><div>x</div>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-paragraph-autoclose",
    },
    FailRow {
        name: "paragraph_autoclose_h1_implicit",
        source: "<script>let c = $state(0);</script>\n<p><h1>x</h1>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-paragraph-autoclose",
    },
];

#[test]
fn fail_matrix_every_row_fails_closed_with_exact_diagnostic_and_no_main() {
    let mut wrong = Vec::new();
    for row in FAIL_MATRIX {
        match compile(row.source) {
            Err(ClientCompileError::Unsupported(surface)) => {
                // The EXACT machine-stable diagnostic id — an EQUALITY check, not merely
                // the `svelte-runtime-unsupported-` prefix. This catches a refusal-arm
                // drift (a row that silently changes its refusal arm to a sibling code).
                if surface.diagnostic_code() != row.code {
                    wrong.push(format!(
                        "{}: expected code {}, got {} ({:?})",
                        row.name,
                        row.code,
                        surface.diagnostic_code(),
                        surface
                    ));
                }
            }
            // NO `Main` — an accepted module is a fail-matrix failure.
            Ok(js) => wrong.push(format!(
                "{}: expected fail-closed ({}), got an emitted Main:\n{js}",
                row.name, row.code
            )),
            Err(other) => wrong.push(format!(
                "{}: expected an unsupported-surface refusal, got {other:?}",
                row.name
            )),
        }
    }
    assert!(
        wrong.is_empty(),
        "fail-matrix rows with the wrong outcome:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn fail_matrix_row_codes_are_known_unsupported_diagnostics() {
    // Each row's expected `code` must be a KNOWN `UnsupportedSvelteRuntimeSurface`
    // diagnostic id. The table enumerates every valid `svelte-runtime-unsupported-*`
    // code; a row's `code` must be a member. This guards the TABLE itself (independently
    // of the live compiler), so a copy-paste / typo'd code fails here even before the
    // compiler is run.
    let valid_codes: &[&str] = &[
        "svelte-runtime-unsupported-dynamic-attribute",
        "svelte-runtime-unsupported-duplicate-attribute",
        "svelte-runtime-unsupported-element",
        "svelte-runtime-unsupported-binding",
        "svelte-runtime-unsupported-non-delegated-event",
        "svelte-runtime-unsupported-block",
        "svelte-runtime-unsupported-component",
        "svelte-runtime-unsupported-advanced-rune",
        "svelte-runtime-unsupported-host-custom-element",
        "svelte-runtime-unsupported-experimental-async",
        "svelte-runtime-unsupported-static-interpolation",
        "svelte-runtime-unsupported-destructuring-write",
        "svelte-runtime-unsupported-root-text-region",
        "svelte-runtime-unsupported-complex-interpolation",
        "svelte-runtime-unsupported-script-import",
        "svelte-runtime-unsupported-typescript",
        "svelte-runtime-unsupported-complex-text",
        "svelte-runtime-unsupported-element-name",
        "svelte-runtime-unsupported-instance-script-item",
        "svelte-runtime-unsupported-magic-identifier",
        "svelte-runtime-unsupported-paragraph-autoclose",
    ];
    for row in FAIL_MATRIX {
        assert!(
            valid_codes.contains(&row.code),
            "{}: declared code {} is not a known unsupported diagnostic id",
            row.name,
            row.code
        );
    }
}

#[test]
fn fail_matrix_has_no_duplicate_rows() {
    // The matrix is table-driven; a duplicate name is a copy-paste slip that would
    // silently under-count coverage.
    let mut names: Vec<&str> = FAIL_MATRIX.iter().map(|r| r.name).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "duplicate fail-matrix row names");
}

// ─────────────────────────────────────────────────────────────────────────────
// DETERMINISTIC GENERATORS — the three finite grammars.
//
// Each generator enumerates every variant of a finite grammar (event-handler
// expression node kinds, `$props()` pattern/default shapes, bind target shapes) and
// asserts EACH variant lands on the correct side of the supported boundary: a
// SUPPORTED variant emits a valid `Main`; an UNSUPPORTED variant fails closed (no
// `Main`). This is the combinatorial cover on top of the explicit matrices — a
// silent boundary drift in ANY single variant fails here. Together with the
// SUPPORTED_MATRIX (which pins the full-module topology of the supported shapes) and
// the FAIL_MATRIX (which pins the diagnostic owner), this is the convergence gate.
// ─────────────────────────────────────────────────────────────────────────────

/// The expected side of the supported boundary for a generated variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// The variant is supported — Verter emits a valid `Main`.
    Supported,
    /// The variant fails closed — Verter refuses (no `Main`).
    FailClosed,
}

/// Whether `code` parses as a valid JS module through OXC (no panic, no syntax
/// errors). A guard against a SUPPORTED variant whose emitted module is
/// structurally a `Main` but syntactically broken JS (e.g. a raw `name! = $$value`
/// TS-wrapped setter, a stray `export`, an unbalanced wrap). Mirrors the topology
/// gate's `parses_as_js` (the two live in separate test binaries).
fn parses_as_js(code: &str) -> bool {
    let alloc = Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let ret = oxc_parser::Parser::new(&alloc, code, source_type).parse();
    !ret.panicked && ret.errors.is_empty()
}

/// Assert a generated variant lands on the expected side of the boundary.
fn assert_variant(label: &str, source: &str, expected: Expected) {
    match (compile(source), expected) {
        (Ok(js), Expected::Supported) => {
            // A supported variant must emit a non-empty `Main` that is VALID JS.
            assert!(
                js.contains("export default function"),
                "{label}: expected a supported Main, got:\n{js}"
            );
            // GATE: the emitted module must OXC-parse — an invalid-JS supported
            // variant (e.g. a TS-wrapped bind target emitting `name! = $$value`) is
            // a divergence, not a pass.
            assert!(
                parses_as_js(&js),
                "{label}: supported variant emitted INVALID JS:\n{js}"
            );
        }
        (Ok(js), Expected::FailClosed) => {
            panic!("{label}: expected fail-closed, got an emitted Main:\n{js}");
        }
        (Err(ClientCompileError::Unsupported(_)), Expected::FailClosed) => {}
        (Err(e), Expected::FailClosed) => {
            panic!("{label}: expected an unsupported-surface refusal, got {e:?}");
        }
        (Err(e), Expected::Supported) => {
            panic!("{label}: expected a supported Main, got refusal {e:?}");
        }
    }
}

#[test]
fn generated_event_handler_expression_kinds_land_on_boundary() {
    // The finite grammar of an `onclick={…}` handler EXPRESSION kind. ONLY a
    // non-async INLINE ARROW whose body is a `$state` assignment / update is
    // supported (the §1.2-class handler); EVERY other handler shape — a function
    // expression, a local-function identifier, a call, a bare update/assignment
    // (not an arrow), a member, a sequence, a conditional — is the official wrapper /
    // statement-rewrite breadth and fails closed. An arrow whose body is NOT a
    // `$state` write (a call, a non-`$state` update) ALSO fails closed.
    //
    // The head declares ONLY the supported `$state` signal (the strict instance-script
    // allowlist): a `let plain`, a `function inc`, a `function f` would themselves fail
    // closed at the instance-script-item gate, masking the handler-shape boundary
    // under test. A FAIL handler that names `inc` / `f` / `plain` still fails at the
    // handler-shape gate (it is not a `$state`-write arrow) regardless of whether the
    // name resolves — the handler-shape classifier is structural, not binding-resolving.
    let head = "<script>let c = $state(0);</script>\n";
    let variants: &[(&str, &str, Expected)] = &[
        // ── supported: $state-write arrows ───────────────────────────────────────
        ("arrow_compound", "() => c += 1", Expected::Supported),
        ("arrow_postfix", "() => c++", Expected::Supported),
        ("arrow_prefix", "() => --c", Expected::Supported),
        ("arrow_assign", "() => c = c + 1", Expected::Supported),
        ("arrow_block_updates", "() => { c++; }", Expected::Supported),
        // ── demoted: non-arrow handler shapes ───────────────────────────────
        (
            "function_expr",
            "function () { c++; }",
            Expected::FailClosed,
        ),
        ("local_fn_ident", "inc", Expected::FailClosed),
        ("call", "f(c)", Expected::FailClosed),
        ("update_postfix", "c++", Expected::FailClosed),
        ("update_prefix", "++c", Expected::FailClosed),
        ("assignment", "c = 1", Expected::FailClosed),
        ("compound_assignment", "c += 1", Expected::FailClosed),
        ("member", "f.call", Expected::FailClosed),
        ("sequence", "(f(c), c++)", Expected::FailClosed),
        ("conditional", "c ? inc : f", Expected::FailClosed),
        ("logical", "c && inc", Expected::FailClosed),
        ("literal", "0", Expected::FailClosed),
        // ── demoted: arrow with a NON-$state-write body ─────────────────────
        ("arrow_call_body", "() => f(c)", Expected::FailClosed),
        ("arrow_plain_write", "() => plain = 1", Expected::FailClosed),
        ("arrow_with_param", "(e) => c++", Expected::FailClosed),
        ("arrow_block_call", "() => { f(c); }", Expected::FailClosed),
        ("arrow_empty_block", "() => {}", Expected::FailClosed),
        (
            "async_arrow",
            "async () => { await c; }",
            Expected::FailClosed,
        ),
    ];
    for (label, handler, expected) in variants {
        let source = format!("{head}<button onclick={{{handler}}}>x</button>\n");
        assert_variant(&format!("event_handler::{label}"), &source, *expected);
    }
}

#[test]
fn generated_effect_shapes_all_fail_closed() {
    // `$effect` is demoted ENTIRELY — it has NO supported position (the runes-mode
    // effect topology is a deferral-ledger follow-up, advanced-rune). EVERY `$effect(arg)`
    // shape (an arrow / function expression / identifier / call / member /
    // conditional / async) fails closed. An async body's `await` would otherwise be
    // the async surface, but the `$effect` position refusal wins first.
    let variants: &[(&str, &str)] = &[
        ("arrow_block", "$effect(() => { c; });"),
        ("arrow_expr", "$effect(() => c);"),
        ("function_expr", "$effect(function () { c; });"),
        ("identifier", "$effect(f);"),
        ("call", "$effect(f());"),
        ("member", "$effect(o.m);"),
        ("conditional", "$effect(c ? f : o.m);"),
        ("async_arrow", "$effect(async () => { await c; });"),
    ];
    for (label, stmt) in variants {
        // The `$effect(...)` statement under test is placed in the instance script with
        // ONLY the supported `$state` signal (a `function f` / `const o` helper would
        // itself fail closed at the instance-script-item gate). The `$effect` rune
        // reference is refused by the rune-form/position scan before the generic
        // item gate, so the refusal is the `$effect` position. The `f` / `o` referenced
        // by some variants need not resolve — the rune-position refusal fires on the
        // `$effect` callee regardless. A trailing reactive `{c}` read keeps it runes-mode.
        let source = format!(
            "<script>let c = $state(0); {stmt}</script>\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        assert_variant(&format!("effect::{label}"), &source, Expected::FailClosed);
    }
}

#[test]
fn generated_props_pattern_and_default_shapes_land_on_boundary() {
    // The finite grammar of a `$props()` destructure PATTERN + DEFAULT shape. ONLY a
    // read-only NO-DEFAULT basic destructure with identifier / string keys is
    // supported; ANY default (even a constant literal), a rest / whole-object /
    // computed / numeric / nested / `$bindable` / written / duplicate form fails
    // closed. (Literal defaults were supported; demoted to the §1.2-class
    // no-default core.)
    let variants: &[(&str, &str, Expected)] = &[
        // ── supported: no-default destructure ────────────────────────────────────
        ("plain", "let { a } = $props();", Expected::Supported),
        ("alias", "let { a: b } = $props();", Expected::Supported),
        (
            "string_key",
            "let { \"data-x\": x } = $props();",
            Expected::Supported,
        ),
        // ── demoted: ANY default ────────────────────────────────────────────
        (
            "literal_default_num",
            "let { a = 1 } = $props();",
            Expected::FailClosed,
        ),
        (
            "literal_default_str",
            "let { a = \"x\" } = $props();",
            Expected::FailClosed,
        ),
        (
            "literal_default_bool",
            "let { a = true } = $props();",
            Expected::FailClosed,
        ),
        (
            "ref_default",
            "let { a = 1, b = a } = $props();",
            Expected::FailClosed,
        ),
        (
            "array_default",
            "let { a = [] } = $props();",
            Expected::FailClosed,
        ),
        (
            "call_default",
            "let { a = f() } = $props();",
            Expected::FailClosed,
        ),
        // ── demoted: other out-of-boundary pattern shapes ───────────────────
        (
            "rest",
            "let { a, ...rest } = $props();",
            Expected::FailClosed,
        ),
        ("whole_object", "let p = $props();", Expected::FailClosed),
        (
            "computed",
            "let k = 'x'; let { [k]: a } = $props();",
            Expected::FailClosed,
        ),
        ("numeric", "let { 0: a } = $props();", Expected::FailClosed),
        (
            "nested",
            "let { a: { b } } = $props();",
            Expected::FailClosed,
        ),
        (
            "bindable",
            "let { a = $bindable(0) } = $props();",
            Expected::FailClosed,
        ),
        (
            "duplicate",
            "let { a } = $props(); let { b } = $props();",
            Expected::FailClosed,
        ),
    ];
    for (label, decl, expected) in variants {
        // A `$state` + a reactive `{c}` read keeps the SUPPORTED variants in RUNES
        // mode AND reactive; a SUPPORTED variant's prop is NOT read (a prop read
        // outside a bare interpolation would itself fail closed), so the prop is
        // only declared. The `$props()` declarator under test is the surface.
        let source = format!(
            "<script>let c = $state(0); {decl}</script>\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        assert_variant(&format!("props_shape::{label}"), &source, *expected);
    }
}

#[test]
fn generated_bind_target_shapes_land_on_boundary() {
    // The finite grammar of a `bind:` directive's TARGET shape, enumerated
    // EXHAUSTIVELY across (a) the directive (`value` on `<input>` vs `checked` vs a
    // non-input host vs `this`), and (b) the target expression's ROOT-binding KIND
    // (a bare ident vs a member, rooted at a `$state` / prop / bindable / derived /
    // plain-local / import binding) plus the non-resolvable / non-lvalue forms (a
    // computed member, a call, a sequence pair). `bind:value` on an `<input>` to a
    // signal/plain IDENTIFIER or a `$state`-ROOTED member is supported; element
    // `bind:this` to an identifier is supported; EVERYTHING else fails closed.
    let variants: &[(&str, &str, &str, Expected)] = &[
        // ── supported: bind:value(input)->signal-ident, bind:this->non-prop-ident ─
        (
            "value_signal_ident",
            "let v = $state('');",
            "<input bind:value={v} />",
            Expected::Supported,
        ),
        (
            "this_ident",
            "let el = $state(null);",
            "<div bind:this={el}></div>",
            Expected::Supported,
        ),
        // ── demoted: a $state MEMBER target (was supported; now a binding) ────────
        (
            "value_state_member",
            "let o = $state({ x: '' });",
            "<input bind:value={o.x} />",
            Expected::FailClosed,
        ),
        (
            "value_state_computed_member",
            "let arr = $state(['']); let i = $state(0);",
            "<input bind:value={arr[i]} />",
            Expected::FailClosed,
        ),
        // ── out-of-boundary: a non-`$state` MEMBER root ──────────────────────────
        (
            "value_prop_ident",
            "let { label } = $props();",
            "<input bind:value={label} />",
            Expected::FailClosed,
        ),
        (
            "value_prop_member",
            "let { obj } = $props();",
            "<input bind:value={obj.x} />",
            Expected::FailClosed,
        ),
        (
            "value_prop_member_aliased",
            "let { obj: o } = $props();",
            "<input bind:value={o.x} />",
            Expected::FailClosed,
        ),
        (
            "value_bindable_member",
            "let { obj = $bindable({}) } = $props();",
            "<input bind:value={obj.x} />",
            Expected::FailClosed,
        ),
        (
            "value_derived_member",
            "let c = $state(0); let d = $derived({ x: c });",
            "<input bind:value={d.x} />",
            Expected::FailClosed,
        ),
        (
            "value_plain_local_member",
            "let o = { x: '' }; let c = $state(0);",
            "<input bind:value={o.x} />",
            Expected::FailClosed,
        ),
        (
            "value_import_member",
            "import { store } from './s.js'; let c = $state(0);",
            "<input bind:value={store.x} />",
            Expected::FailClosed,
        ),
        // ── out-of-boundary: non-lvalue / non-resolvable / wrong directive ───────
        (
            "value_call",
            "let c = $state(0); function f() { return c; }",
            "<input bind:value={f()} />",
            Expected::FailClosed,
        ),
        (
            "value_call_member",
            "let c = $state(0); function f() { return { x: c }; }",
            "<input bind:value={f().x} />",
            Expected::FailClosed,
        ),
        (
            "value_sequence_pair",
            "let c = $state('');",
            "<input bind:value={() => c, (v) => c = v} />",
            Expected::FailClosed,
        ),
        (
            "value_non_input",
            "let v = $state('');",
            "<textarea bind:value={v}></textarea>",
            Expected::FailClosed,
        ),
        (
            "checked",
            "let on = $state(false);",
            "<input type=\"checkbox\" bind:checked={on} />",
            Expected::FailClosed,
        ),
        (
            "this_member",
            "let refs = $state([]);",
            "<div bind:this={refs[0]}></div>",
            Expected::FailClosed,
        ),
    ];
    for (label, decl, markup, expected) in variants {
        // The bind directive itself is the surface under test — a supported
        // `bind:value` / `bind:this` is a complete emitting component without an
        // extra reactive read (the bind op is the reactivity). The fail-closed
        // variants refuse before topology, so reactivity is irrelevant to them.
        let source = format!("<script>{decl}</script>\n{markup}\n");
        assert_variant(&format!("bind_target::{label}"), &source, *expected);
    }
}

#[test]
fn generated_lang_ts_components_all_fail_closed() {
    // A `<script lang="ts">` component is demoted ENTIRELY — the TS-strip path is a
    // script-completion follow-up, so EVERY `lang="ts"` component fails closed
    // BEFORE any bind / interpolation classification. (TS-strip was supported;
    // demoted to the §1.2-class plain-JS core.) This covers clean lvalues, TS-wrapped
    // lvalues, and TS-annotation script bodies — all refused at the parse gate.
    let variants: &[(&str, &str, &str)] = &[
        (
            "clean_ident",
            "let name = $state('');",
            "<input bind:value={name} />",
        ),
        (
            "non_null_ident",
            "let name = $state('');",
            "<input bind:value={name!} />",
        ),
        (
            "as_ident",
            "let name = $state('');",
            "<input bind:value={name as string} />",
        ),
        (
            "annotated_state",
            "let count: number = $state(0);",
            "<button onclick={() => count++}>{count}</button>",
        ),
        (
            "interface_decl",
            "interface Box { n: number } let count = $state(0);",
            "<button onclick={() => count++}>{count}</button>",
        ),
    ];
    for (label, decl, markup) in variants {
        let source = format!("<script lang=\"ts\">{decl}</script>\n{markup}\n");
        assert_variant(&format!("lang_ts::{label}"), &source, Expected::FailClosed);
    }
}

#[test]
fn generated_static_attr_shapes_land_on_boundary() {
    // The finite grammar of a STATIC attribute's `(name, host)` shape against the
    // STRICT FINITE static-attr allowlist (`SupportedStaticAttr`). The hosts here are
    // all ALLOWLISTED elements (`div` / `button` / `input` / `a`) — an out-of-allowlist
    // host fails at the element gate (covered by the element matrix), so this isolates
    // the ATTR boundary. A name in the allowlist (global `id`/`title`/`role`/non-empty
    // `class`/`data-*`/`aria-*`; per-tag `a:href`, `button:type/disabled`,
    // `input:type/disabled`) folds into the skeleton (supported); EVERY other name
    // (`is`, `defaultValue`/`defaultChecked`, `autofocus`/`muted`, `dir`, `style`,
    // input `value`/`checked`, an empty `class`, an unknown name) fails closed BEFORE
    // emission; a customized built-in (`is=`) fails at the element gate.
    let variants: &[(&str, &str, Expected)] = &[
        // ── allowlisted static attrs (folded into the skeleton) ──────────────────
        ("id", "<div id=\"x\"></div>", Expected::Supported),
        (
            "class_nonempty",
            "<div class=\"box\"></div>",
            Expected::Supported,
        ),
        ("title", "<div title=\"t\"></div>", Expected::Supported),
        ("role", "<div role=\"button\"></div>", Expected::Supported),
        (
            "data_attr",
            "<div data-id=\"5\"></div>",
            Expected::Supported,
        ),
        (
            "aria_attr",
            "<div aria-label=\"x\"></div>",
            Expected::Supported,
        ),
        ("anchor_href", "<a href=\"/x\">y</a>", Expected::Supported),
        (
            "button_type",
            "<button type=\"submit\">y</button>",
            Expected::Supported,
        ),
        (
            "button_disabled",
            "<button disabled>y</button>",
            Expected::Supported,
        ),
        ("input_type", "<input type=\"text\" />", Expected::Supported),
        ("input_disabled", "<input disabled />", Expected::Supported),
        // A static valueless `autofocus` is a NON-STATIC-PROPERTY — it
        // emits the init-only `$.autofocus(input, true)`, NOT a baked skeleton attr.
        ("autofocus", "<input autofocus />", Expected::Supported),
        // ── forbidden static attrs on an ALLOWLISTED host (fail closed) ──────────
        // `dir` (the reflected-attr quirk) and a STATIC `style` (no `style:` directive)
        // stay refused; `muted` on a NON-media element is refused (not tested here —
        // the hosts are non-media).
        ("dir", "<div dir=\"ltr\">d</div>", Expected::FailClosed),
        (
            "style",
            "<div style=\"color:red\">d</div>",
            Expected::FailClosed,
        ),
        ("input_value", "<input value=\"x\" />", Expected::FailClosed),
        ("input_checked", "<input checked />", Expected::FailClosed),
        (
            "input_default_value",
            "<input defaultValue=\"x\" />",
            Expected::FailClosed,
        ),
        (
            "input_default_checked",
            "<input defaultChecked />",
            Expected::FailClosed,
        ),
        // An empty `class=""` is not serializable (rejected by the allowlist).
        (
            "empty_class",
            "<div class=\"\">d</div>",
            Expected::FailClosed,
        ),
        // A per-tag attr on the WRONG host (`href` only on `<a>`) is rejected.
        (
            "href_on_div",
            "<div href=\"/x\">d</div>",
            Expected::FailClosed,
        ),
        // An unknown attribute name is rejected.
        (
            "unknown_name",
            "<div data-x-ok=\"1\" foobar=\"1\">d</div>",
            Expected::FailClosed,
        ),
        // ── customized built-in (`is=`) fails at the element gate ───────────
        (
            "customized_builtin_is",
            "<button is=\"my-btn\">x</button>",
            Expected::FailClosed,
        ),
        // ── custom element (`<my-widget>`) fails at the element gate ────────
        (
            "custom_element",
            "<my-widget foo=\"bar\"></my-widget>",
            Expected::FailClosed,
        ),
    ];
    for (label, markup, expected) in variants {
        // A `$state` + a reactive `{c}` read in a trailing button keeps every variant
        // in RUNES mode AND reactive (so a SUPPORTED variant reaches emission rather
        // than the static-fold / root-text path); the element under test
        // carries the static attribute being classified.
        let source = format!(
            "<script>let c = $state(0);</script>\n{markup}\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        assert_variant(&format!("static_attr::{label}"), &source, *expected);
    }
}

#[test]
fn generated_rune_declaration_kinds_land_on_boundary() {
    // The finite grammar of a rune declarator's DECLARATION KIND (`let` / `var` /
    // `const`) across the three runes (`$state` / `$derived` / `$props`). ONLY a
    // `let` rune declarator is supported; `var` / `const` rune declarators are a
    // distinct official surface (a `var` rune read is `$.safe_get`, a read-only
    // `const $state` constant-folds to an empty topology, and a `var`/`const`
    // `$props()` preserves the keyword on the emitted declarator) and fail closed.
    // A NON-rune `var` / `const` local is ALSO out-of-allowlist now (the supported
    // instance script is `let`-only — a `const` / `var` declaration fails closed at
    // the instance-script-item gate).
    let variants: &[(&str, &str, Expected)] = &[
        // ── $state — only a `let` primitive declarator is supported ──────────────
        ("state_let", "let c = $state(0);", Expected::Supported),
        ("state_var", "var c = $state(0);", Expected::FailClosed),
        // A read-only `const $state` (a written `const $state` is a svelte
        // `constant_assignment` parse error, so the declarator is read-only here).
        ("state_const", "const k = $state(0);", Expected::FailClosed),
        // ── $derived — demoted ENTIRELY (no supported kind, advanced-rune) ────────
        (
            "derived_let",
            "let d = $derived(c * 2);",
            Expected::FailClosed,
        ),
        (
            "derived_var",
            "var d = $derived(c * 2);",
            Expected::FailClosed,
        ),
        (
            "derived_const",
            "const d = $derived(c * 2);",
            Expected::FailClosed,
        ),
        // ── $props — only a `let` no-default destructure is supported ────────────
        ("props_let", "let { a } = $props();", Expected::Supported),
        ("props_var", "var { a } = $props();", Expected::FailClosed),
        (
            "props_const",
            "const { a } = $props();",
            Expected::FailClosed,
        ),
        // ── non-rune var/const locals (now demoted — `let`-only allowlist) ───────
        ("nonrune_var", "var step = 2;", Expected::FailClosed),
        ("nonrune_const", "const step = 2;", Expected::FailClosed),
    ];
    for (label, decl, expected) in variants {
        // A trailing `let kept = $state(0)` reactive read keeps every variant in RUNES
        // mode AND reactive. The declarator under test is prepended; the head's signal
        // is named `kept` (NOT `c`/`a`) so it NEVER collides with a declarator-under-
        // test's binding name — a same-name collision would be the official
        // `declaration_duplicate` / `js_parse_error` reject (which the official-reject
        // gate now catches), masking the declaration-KIND boundary under test.
        let head = "let kept = $state(0);";
        let source = format!(
            "<script>{head} {decl}</script>\n<button onclick={{() => kept++}}>{{kept}}</button>\n"
        );
        assert_variant(&format!("decl_kind::{label}"), &source, *expected);
    }
}

#[test]
fn generated_interpolation_expression_kinds_land_on_boundary() {
    // The finite grammar of an interpolation `{expr}` EXPRESSION kind. ONLY a bare
    // identifier resolving to a reactive `$state` signal read or a no-default prop
    // read is supported; EVERY non-identifier expression shape (a binary / logical /
    // conditional / call / optional-call / member / sequence / unary / `new` /
    // template / parenthesized / TS-wrapped) needs the official `build_template_chunk`
    // evaluator and fails closed. A bare identifier resolving to a non-reactive
    // binding is the static-fold deferral.
    let head = "<script>let c = $state(0); let f = $state(null); let obj = $state(0);</script>\n";
    let variants: &[(&str, &str, Expected)] = &[
        // ── supported: bare reactive identifier read ─────────────────────────────
        ("signal_ident", "{c}", Expected::Supported),
        // ── demoted: complex expression shapes (build_template_chunk) ─────────
        ("binary", "{c + 1}", Expected::FailClosed),
        ("logical", "{c || 0}", Expected::FailClosed),
        ("conditional", "{c ? c : 0}", Expected::FailClosed),
        ("call", "{f(c)}", Expected::FailClosed),
        ("optional_call", "{f?.(c)}", Expected::FailClosed),
        ("member", "{obj.x}", Expected::FailClosed),
        ("optional_member", "{obj?.x}", Expected::FailClosed),
        ("unary", "{-c}", Expected::FailClosed),
        ("sequence", "{(c, c)}", Expected::FailClosed),
        ("new_expr", "{new Date()}", Expected::FailClosed),
        ("template", "{`v${c}`}", Expected::FailClosed),
        ("parenthesized", "{(c)}", Expected::FailClosed),
        ("literal", "{1}", Expected::FailClosed),
    ];
    for (label, interp, expected) in variants {
        // The interpolation under test is the button text; the onclick keeps the
        // component runes-mode + reactive.
        let source = format!("{head}<button onclick={{() => c++}}>{interp}</button>\n");
        assert_variant(&format!("interp::{label}"), &source, *expected);
    }
}

#[test]
fn generated_state_init_shapes_land_on_boundary() {
    // The finite grammar of a `$state(init)` INITIALIZER shape. ONLY a primitive
    // literal init (string / number / boolean / null / undefined / bigint / `-1`) is
    // supported (the §1.2-class `$.state(<literal>)` signal); an object / array /
    // call / identifier init is the deep-reactive `$.proxy` form and fails closed
    //. (Object/array proxy state was supported; demoted.)
    let variants: &[(&str, &str, Expected)] = &[
        // ── supported: primitive-literal inits ───────────────────────────────────
        ("number", "let s = $state(0);", Expected::Supported),
        ("string", "let s = $state('x');", Expected::Supported),
        ("boolean", "let s = $state(true);", Expected::Supported),
        ("null", "let s = $state(null);", Expected::Supported),
        ("undefined_empty", "let s = $state();", Expected::Supported),
        ("negative", "let s = $state(-1);", Expected::Supported),
        // ── demoted: non-primitive (deep-reactive proxy) inits ──────────────
        ("object", "let s = $state({});", Expected::FailClosed),
        ("array", "let s = $state([]);", Expected::FailClosed),
        (
            "object_props",
            "let s = $state({ a: 1 });",
            Expected::FailClosed,
        ),
        ("call_init", "let s = $state(make());", Expected::FailClosed),
        (
            "template_init",
            "let s = $state(`x`);",
            Expected::FailClosed,
        ),
    ];
    for (label, decl, expected) in variants {
        // The head declares ONLY the supported `$state` signal (a `function make`
        // helper would itself fail closed at the instance-script-item gate). The
        // `$state` declarator under test is the surface — its SHAPE gate (primitive vs
        // proxy) fires at declaration. The `call_init` variant's `make()` need not
        // resolve: a CALL init is a non-primitive shape refused at the state-shape gate
        // regardless of whether `make` is declared.
        let source = format!(
            "<script>let c = $state(0); {decl}</script>\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        assert_variant(&format!("state_init::{label}"), &source, *expected);
    }
}

#[test]
fn generated_text_chunk_shapes_land_on_boundary() {
    // The finite grammar of a reactive-text run's LITERAL CHUNK. ONLY a simple-ASCII
    // significant chunk is supported; an HTML entity, an interior tab / newline, a
    // repeated-space run, or a backtick / `${` escaping need fails closed. The
    // structural inter-element / indentation whitespace is NOT a complex chunk.
    let variants: &[(&str, &str, Expected)] = &[
        // ── supported: simple-ASCII mixed runs ───────────────────────────────────
        ("simple_prefix", "Hello {c}", Expected::Supported),
        ("simple_suffix", "{c}!", Expected::Supported),
        ("simple_both", "a {c} b", Expected::Supported),
        // ── demoted: complex literal chunks ─────────────────────────────────
        ("entity_amp", "A &amp; {c}", Expected::FailClosed),
        ("entity_numeric", "x &#39; {c}", Expected::FailClosed),
        ("entity_lt", "x &lt;y {c}", Expected::FailClosed),
        ("repeated_space", "a  b {c}", Expected::FailClosed),
        ("interior_tab", "a\tb {c}", Expected::FailClosed),
    ];
    for (label, body, expected) in variants {
        // The text run is the button content (an element child, so the run is NOT a
        // root text region); the onclick keeps it runes-mode + reactive.
        let source = format!(
            "<script>let c = $state(0);</script>\n<button onclick={{() => c++}}>{body}</button>\n"
        );
        assert_variant(&format!("text_chunk::{label}"), &source, *expected);
    }
}

#[test]
fn fail_matrix_covers_every_documented_sub_shape() {
    // The matrix must enumerate the full UNSUPPORTED-FEATURE fail-closed boundary
    // (the brief's FAIL_MATRIX list). This count gate fails LOUDLY if a row is dropped
    // — the matrix is the convergence gate, so a shrinking matrix is a coverage
    // regression, not a silent pass.
    //
    // OFFICIAL REJECTS live in the official-reject parity matrix
    // (`svelte_client_official_reject_matrix.rs`), NOT here — they are MALFORMED-input
    // rejections, not unsupported features: a `let $$anchor` `$$`-prefixed binding, the
    // `$$props` / `$$restProps` magic-object reads, a DUPLICATE attribute / directive
    // (`attribute_duplicate`, ×5 removed), and TS-only syntax in a plain (JS) `<script>`
    // (`enum` / typed `let` → `js_parse_error` via the body-probe, ×1 removed). (`$$slots`,
    // which official accepts, is a deferrable unsupported-feature refusal and stays here.)
    //
    // The close-tag-structure + bind/state feature rows that official ACCEPTS but are
    // outside the §1.2 core: a free / undeclared `bind:this` target (×2, binding), a `$state`
    // over a shadowed `undefined` + a `$state(NaN)` (×2, advanced-rune), and the implicit `<p>`
    // autoclose (×2, paragraph-autoclose).
    //
    // The value/property-position emitter is SOURCE-PRESERVING: it keeps the author's
    // redundant parens verbatim (a cosmetic difference the structural corpus compare waives),
    // so there is NO value-paren refusal surface and NO `value_paren_*` fail-closed row.
    assert_eq!(
        FAIL_MATRIX.len(),
        117,
        "the fail matrix must enumerate all 117 documented unsupported-feature \
         fail-closed sub-shapes (the element-spread + `{{@html}}` surface removed the \
         `spread` / `html_tag` refusal rows now SUPPORTED, replacing them with the \
         `props_rest_spread` row PLUS the three spread-incompatible-directive rows \
         `spread_with_event` / `spread_with_bind` / `spread_with_use` that lock the \
         fail-closed identity of a spread co-located with an event/bind/use; the value \
         emitter is source-preserving, so the five `value_paren_*` rows are GONE — author \
         parens are kept verbatim, never refused)"
    );
}
