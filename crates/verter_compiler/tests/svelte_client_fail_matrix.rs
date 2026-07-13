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
//! component routes through the legacy per-surface dispatch first, before the surface under
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
    compile_client(source, &parsed, &opts, &alloc, false, false).map(|m| m.code)
}

/// The FAIL MATRIX rows — every fail-closed sub-shape per the supported boundary.
const FAIL_MATRIX: &[FailRow] = &[
    // ── $state advanced forms ───────────────────────────────────────────
    FailRow {
        name: "state_destructure",
        source: "<script>let { a } = $state({ a: 1 });</script>\n<button onclick={() => a}>{a}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A module-scope `$state` declarator is a NON-import module item — the
        // admitted `<script module>` is import-only, so it refuses with the precise
        // module-item diagnostic before the module-rune shape gate.
        name: "state_module",
        source: "<script module>let c = $state(0);</script>\n<script>let d = $state(0);</script>\n<button onclick={() => d++}>{d}</button>\n",
        code: "svelte-runtime-unsupported-module-script-item",
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
    // ── $effect family fail-closed remainder ─────────────────────────────────
    // (The supported family boundary is POSITION-SENSITIVE per member:
    // `$effect(fn)` / `$effect.pre(fn)` are STATEMENT-ONLY — official rejects
    // every value position with `effect_invalid_placement` — while
    // `$effect.root(fn)` / `$effect.tracking()` are expression-valued
    // (statement AND declarator-init positions both lower). The positive
    // topology lives in the `runes/effect_*` + `matrix/effect_arrow`
    // emit-corpus goldens and the `effect_*` client tests. The rows here
    // enumerate the surviving fail-closed sub-shapes: the async re-home, the
    // non-call / uncalled / malformed forms, the 5j member, and the
    // VALUE-POSITION user-effect calls the statement gate refuses.)
    FailRow {
        // An AWAITING effect callback is the experimental-async surface (5j): the
        // effect-statement carrier routes the body through the shared rewriter,
        // whose `await` gate refuses — never a sync `$.user_effect(async …)` with
        // a live `await` on the 5j boundary.
        name: "effect_async",
        source: "<script>let c = $state(0); $effect(async () => { await c; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-experimental-async",
    },
    FailRow {
        // A TOP-LEVEL declarator-init `$effect(...)` (official
        // `effect_invalid_placement` — the user-effect members are
        // statement-only): the position gate refuses under the precise family
        // label, no longer the incidental `const declaration` item refusal.
        name: "effect_value_position_const_decl",
        source: "<script>let c = $state(0); const e = $effect(() => { console.log(c) });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // The `.pre` twin of the declarator-init value position.
        name: "effect_pre_value_position_const_decl",
        source: "<script>let c = $state(0); const p = $effect.pre(() => { console.log(c) });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A value-position `$effect(...)` as an EXPRESSION-bodied handler arrow's
        // concise body (official `effect_invalid_placement`).
        name: "effect_value_position_handler_concise_body",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => $effect(() => { console.log(c) })}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A value-position `$effect(...)` as a DECLARATOR INIT inside an accepted
        // `$effect.root` callback body.
        name: "effect_value_position_root_body_decl_init",
        source: "<script>let c = $state(0); const stop = $effect.root(() => { const s2 = $effect(() => { console.log(c) }); return () => {}; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A value-position `$effect(...)` as a `return` ARGUMENT inside an
        // accepted `$effect.root` callback body.
        name: "effect_value_position_root_body_return",
        source: "<script>let c = $state(0); const stop = $effect.root(() => { return $effect(() => {}); });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A value-position `$effect.pre(...)` as a DECLARATOR INIT inside an
        // accepted `$effect` body.
        name: "effect_pre_value_position_effect_body_decl_init",
        source: "<script>let c = $state(0); $effect(() => { const q = $effect.pre(() => {}); });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // An OPTIONAL-call user effect (`$effect?.(fn)`) — official rejects it
        // with `effect_invalid_placement` (the `?.` chain sits between the call
        // and its statement parent), oracle-verified. The optional forms of the
        // statement-only members stay fail-closed; only `.root` / `.tracking`
        // admit optional invocations (normalized, in the accept suite).
        name: "effect_optional_call",
        source: "<script>let c = $state(0); $effect?.(() => { console.log(c) });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // The `.pre` twin of the optional-call rejection (`$effect.pre?.(fn)` —
        // official `effect_invalid_placement`, oracle-verified).
        name: "effect_pre_optional_call",
        source: "<script>let c = $state(0); $effect.pre?.(() => { console.log(c) });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // An OPTIONAL member receiver on a user-effect member (`$effect?.pre(fn)`
        // — official `effect_invalid_placement`, oracle-verified).
        name: "effect_optional_member_pre",
        source: "<script>let c = $state(0); $effect?.pre(() => { console.log(c) });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A NON-CALL `$effect` value reference has no supported position (only
        // the called family forms lower).
        name: "effect_bare_ref",
        source: "<script>let c = $state(0); foo($effect);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // An UNCALLED family member reference (official `rune_missing_parentheses`).
        name: "effect_uncalled_pre",
        source: "<script>let c = $state(0); const f = $effect.pre;</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A MALFORMED family call (official `rune_invalid_arguments`): the
        // zero-arg `$effect.tracking` contract rejects an argument.
        name: "effect_tracking_with_arg",
        source: "<script>let c = $state(0); const t = $effect.tracking(c);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A TS TYPE-ARGUMENT on a family call in a PLAIN (non-TS) script.
        // Official plain-script parsing reads `$effect<number>(fn)` as a
        // COMPARISON chain (the rune reference is left uncalled) and rejects
        // with `rune_missing_parentheses` — the shared family classifier
        // treats ANY type-argumented family call as malformed, so Verter
        // never TS-strips-and-emits a spelling official rejects.
        name: "effect_type_args",
        source: "<script>let c = $state(0); $effect<number>(() => { console.log(c) });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // The `.pre` twin of the type-argument rejection.
        name: "effect_pre_type_args",
        source: "<script>let c = $state(0); $effect.pre<number>(() => { console.log(c) });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // The `.root` STATEMENT twin of the type-argument rejection.
        name: "effect_root_type_args_stmt",
        source: "<script>let c = $state(0); $effect.root<number>(() => { return () => {}; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // The `.root` DECLARATOR-INIT twin of the type-argument rejection.
        // (`$effect.tracking<number>()` has NO row here: as plain JS it is a
        // parse error — `… < number > ()` — so it fails closed at the
        // OFFICIAL-reject rail (`js_parse_error`); its pin lives in the
        // reject corpus, `rejects/block4_core/effect_tracking_type_args`.)
        name: "effect_root_type_args_init",
        source: "<script>let c = $state(0); const s = $effect.root<number>(() => { return () => {}; });</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // `$effect.pending` is the experimental-async member (5j) — NOT part of
        // the supported family; it must not ride the family call exemption.
        name: "effect_pending",
        source: "<script>let c = $state(0); const p = $effect.pending();</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-experimental-async",
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
    // (`props_bindable` / `props_ref_default` / `props_array_default` removed —
    // a `$bindable(...)` default and plain `$props()` defaults are now the
    // supported `$.prop` prop-source surface; their positive topology is pinned
    // by the oracle-backed client tests.)
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
    // (`bind:checked` on `<input type="checkbox">` is now SUPPORTED by 5c — it emits
    // `$.remove_input_defaults` + `$.bind_checked`; its positive coverage is the
    // `checked_bind_now_emits_*` client test + the `matrix/bind_checked` golden.)
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
    // NOTE: a bare CALL `bind:value={f()}` (NOT a valid lvalue and NOT a two-element
    // `{get, set}` pair) now fails closed through the OFFICIAL-reject gate with the EXACT
    // code `bind_invalid_expression` (a bind-target SHAPE reject, the same class as
    // bind_group / bind_parens), so it is NOT an unsupported-feature row — its parity lives
    // in `svelte_client_official_reject_matrix.rs` + the `bind_invalid_expression` reject
    // corpus row.
    FailRow {
        // A member `bind:this={refs[0]}` (a plain-local array, so the member-bind gate
        // fires, not the state-shape gate) is the deferral-ledger member-bind form.
        name: "bind_this_member",
        source: "<script>let refs = []; let c = $state(0);</script>\n<div bind:this={refs[0]}></div>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    // (`bind_this_component` removed — a component `bind:this` is the 5f-a surface and now
    // emits `$.bind_this(Child(...), set, get)`.)
    // ── runtime-unsupported DEDICATED-helper binds (fail closed at the runtime router) ──
    // Each of these has a real IDE contract row whose OFFICIAL svelte@5.56.3 helper is a
    // DEDICATED helper (a generic `$.bind_property` form would emit RUNTIME-BROKEN output
    // — the wrong helper). The native client runtime does not emit these yet, so the
    // contract records the REAL official helper + `RuntimeSupport::Unsupported` and the
    // runtime router fails them closed. Verified against the pinned compiler: `bind:files`
    // → `$.bind_files`, `bind:playbackRate` → `$.bind_playback_rate`, `bind:volume` →
    // `$.bind_volume`, `bind:muted` → `$.bind_muted`, the resize-observer family →
    // `$.bind_resize_observer`.
    FailRow {
        name: "bind_files_wrong_helper",
        source: "<script>let f = $state();</script>\n<input type=\"file\" bind:files={f} />\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_playback_rate_wrong_helper",
        source: "<script>let r = $state(1);</script>\n<audio bind:playbackRate={r}></audio>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_volume_wrong_helper",
        source: "<script>let v = $state(1);</script>\n<audio bind:volume={v}></audio>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_muted_wrong_helper",
        source: "<script>let m = $state(false);</script>\n<video bind:muted={m}></video>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_content_rect_wrong_helper",
        source: "<script>let cr = $state();</script>\n<div bind:contentRect={cr}></div>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_content_box_size_wrong_helper",
        source: "<script>let cb = $state();</script>\n<div bind:contentBoxSize={cb}></div>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_border_box_size_wrong_helper",
        source: "<script>let bb = $state();</script>\n<div bind:borderBoxSize={bb}></div>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_device_pixel_content_box_size_wrong_helper",
        source: "<script>let dp = $state();</script>\n<div bind:devicePixelContentBoxSize={dp}></div>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    // ── runtime-unsupported GENERIC-property binds (fail closed at the runtime router) ──
    // Each of these has a real IDE contract row whose OFFICIAL helper IS the generic
    // `$.bind_property` form (the right helper), yet the native client runtime does not
    // emit it yet — so the contract records the real `Property` official helper +
    // `RuntimeSupport::Unsupported` and the runtime router fails it closed (refusal rides
    // support, NOT the emittable helper identity). Each host is the name's
    // empirically-pinned `binding_properties.valid_elements` member (svelte@5.56.3
    // `phases/bindings.js`) that is ALSO in the client element allowlist — so the row
    // is reachable (it fails at the BIND gate, not the element gate). `naturalWidth` /
    // `naturalHeight` are `<img>`-only and `<img>` is NOT allowlisted, so they are
    // router-level only (covered by the `bind_contract` router test, NOT a fail-matrix
    // row that would fail at the element gate instead of the bind gate).
    FailRow {
        name: "bind_indeterminate_unsupported",
        source: "<script>let i = $state(false);</script>\n<input bind:indeterminate={i} />\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_buffered_unsupported",
        source: "<script>let b = $state();</script>\n<audio bind:buffered={b}></audio>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_seekable_unsupported",
        source: "<script>let s = $state();</script>\n<audio bind:seekable={s}></audio>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_seeking_unsupported",
        source: "<script>let s = $state(false);</script>\n<audio bind:seeking={s}></audio>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_ended_unsupported",
        source: "<script>let e = $state(false);</script>\n<audio bind:ended={e}></audio>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_ready_state_unsupported",
        source: "<script>let r = $state(0);</script>\n<audio bind:readyState={r}></audio>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_video_width_unsupported",
        source: "<script>let w = $state(0);</script>\n<video bind:videoWidth={w}></video>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    FailRow {
        name: "bind_video_height_unsupported",
        source: "<script>let h = $state(0);</script>\n<video bind:videoHeight={h}></video>\n",
        code: "svelte-runtime-unsupported-binding",
    },
    // (`bind:focused` was an EXPLICIT runtime-unsupported registry row here; 5f-b flips it to
    // `RuntimeSupport::Supported` — `<input bind:focused>` now emits `$.bind_focused(input,
    // ($$value) => $.set(fo, $$value))`, so its fail-closed row was removed.)
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
        // A bare-identifier handler (not an inline arrow) is the wrapper form; the
        // handler SHAPE is the surface (no import involved).
        name: "event_import_identifier",
        source: "<script>let c = $state(0);</script>\n<button onclick={handler}>{c}</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    // NOTE: regular-element non-delegated `$.event`, capture-phase, and legacy
    // modifier-bearing events are now a SUPPORTED surface (they EMIT the official
    // `$.event(...)` shape) — their former fail-closed rows moved to the positive
    // `events/*` emit-topology corpus + the behavioral smoke. An INVALID legacy modifier
    // (an unknown modifier / `passive`+`preventDefault` / `passive`+`nonpassive`) stays
    // fail-closed via `event_invalid_modifier_combo` below.
    FailRow {
        name: "event_invalid_modifier_combo",
        source: "<script>let c = $state(0);</script>\n<button on:click|passive|preventDefault={() => c++}>x</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    FailRow {
        name: "event_unknown_modifier",
        source: "<script>let c = $state(0);</script>\n<button on:click|stop={() => c++}>x</button>\n",
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
    // (`spread_with_use` is GONE — 5f-c opened the spread + lifecycle co-location gate:
    // official folds `{...p}` alongside `use:` / `transition:`, emitting
    // `$.attribute_effect` → `$.action` → `$.transition` in source order, and Verter now
    // matches it — the positive `lifecycle/spread_lifecycle` golden pins the order. A
    // spread co-located with an event / `bind:` / `let:` stays fail-closed above.)
    // ── 5f-c lifecycle-directive fail-closed boundary ─────────────────────
    FailRow {
        // CHILD-form `{@attach}` (`<div>{@attach fn}</div>`) — official `svelte@5.56.3`
        // REJECTS it at parse (`expected_tag`): `{@attach}` is attribute-position-only.
        // Verter keeps the child-form `TagIr::Attach` fail-closed (`refuse_tag`), while
        // the ELEMENT-position `<div {@attach fn}>` is the accepted form (the positive
        // `lifecycle/attach_element` golden).
        name: "attach_child_form",
        source: "<script>let c = $state(0);</script>\n<div onclick={() => c++}>{@attach fn}</div>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // `{@attach}` on a COMPONENT — official ACCEPTS it as a computed-key prop
        // (`Comp($$anchor, { [$.attachment()]: fn })`); that component-attachment
        // forwarding is DEFERRED (ledger D-38), so Verter fails it closed at the
        // component-call projection rather than dropping the attachment.
        name: "component_attach",
        source: "<script>import Comp from './Comp.svelte'; let c = $state(0);</script>\n<Comp {@attach fn} />\n<button onclick={() => c++}>x</button>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // `use:` on a COMPONENT — official REJECTS (`component_invalid_directive`:
        // a component is not a DOM element host). Verter fails closed at the
        // component-call projection.
        name: "component_use",
        source: "<script>import Comp from './Comp.svelte'; let c = $state(0);</script>\n<Comp use:foo />\n<button onclick={() => c++}>x</button>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // `animate:` on an element NOT inside an each — official REJECTS
        // (`animation_invalid_placement`: the animated element must be the only child
        // of a keyed `{#each}`).
        name: "animate_outside_each",
        source: "<script>let c = $state(0);</script>\n<div animate:flip onclick={() => c++}></div>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // `animate:` inside an UNKEYED each — official REJECTS (`animation_missing_key`).
        name: "animate_unkeyed_each",
        source: "<script>let c = $state(0);</script>\n{#each items as item}<div animate:flip onclick={() => c++}>{item}</div>{/each}\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // `animate:` sharing the keyed-each body with a SIBLING ELEMENT — official
        // REJECTS (`animation_invalid_placement`: the animated element must be the
        // ONLY child). A `{@const}` / `{const}` / `{let}` declaration-tag sibling is
        // IGNORED by the official check (the positive `lifecycle/animate_keyed_const`
        // golden); a sibling ELEMENT stays significant and refuses.
        name: "animate_sibling_element",
        source: "<script>let { items } = $props();</script>\n{#each items as item (item.id)}<span></span><div animate:flip>{item.n}</div>{/each}\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // TWO `animate:` directives on one element — official REJECTS
        // (`animation_duplicate`: an element can only have one `animate` directive).
        // The parser's duplicate-attribute key EXCLUDES animate (official parity), so
        // this reaches the runtime animate gate, not the parse duplicate mint.
        name: "animate_duplicate",
        source: "<script>let c = $state(0);</script>\n{#each items as item (item.id)}<div animate:flip animate:fade onclick={() => c++}>{item}</div>{/each}\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // `in:` alongside an existing `transition:` on one element — official REJECTS
        // (`transition_conflict`: the intro halves overlap). The overlap rule also
        // covers `out:`+`transition:` and same-kind duplicates (`transition_duplicate`);
        // `in:`+`out:` do NOT overlap and stay accepted (the positive FLAG-map goldens).
        name: "transition_conflict",
        source: "<script>let c = $state(0);</script>\n<div in:fade transition:fade onclick={() => c++}></div>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // An `await` inside a lifecycle expression (`{@attach await p}`) — the
        // async/blocker wrapping of lifecycle expressions (official experimental-async
        // `run_after_blockers`) is DEFERRED (ledger D-40): the shared fallible
        // rewriter refuses the `await` before the plan exists.
        name: "lifecycle_async_expr",
        source: "<script>let c = $state(0);</script>\n<div {@attach await p} onclick={() => c++}></div>\n",
        code: "svelte-runtime-unsupported-experimental-async",
    },
    FailRow {
        // A lifecycle directive on a `<svelte:element>` dynamic element — official
        // ACCEPTS it (the callback body emits `$.action($$element, …)`), but the
        // dynamic-element lifecycle surface is DEFERRED (ledger D-39): Verter fails it
        // closed at the `<svelte:element>` attr gate rather than emitting a divergent
        // callback body.
        name: "svelte_element_use",
        source: "<script>let t = $state('div');</script>\n<svelte:element this={t} use:foo onclick={() => t}></svelte:element>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // `use:` on a GLOBAL host (`<svelte:window>`) — official ACCEPTS it
        // (`$.action($.window, …)` in the init body), but the global-host lifecycle
        // surface is DEFERRED (ledger D-39): Verter fails it closed at the
        // global-host attr gate (`classify_special_host`) rather than emitting a
        // divergent init body. This row locks the fail-closed intent (NOT reject
        // parity — official accepts).
        name: "svelte_window_use",
        source: "<script>let c = $state(0);</script>\n<svelte:window use:foo />\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // `{@attach}` on a GLOBAL host (`<svelte:body>`) — official ACCEPTS it
        // (`$.attach($.document.body, …)`), but the global-host lifecycle surface is
        // DEFERRED (ledger D-39): Verter fails it closed at the same global-host
        // attr gate. Fail-closed intent, not reject parity.
        name: "svelte_body_attach",
        source: "<script>let c = $state(0);</script>\n<svelte:body {@attach fn} />\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // `transition:` on a `<svelte:element>` dynamic element — official ACCEPTS it
        // (`$.transition(3, $$element, …)` in the element callback), but the
        // dynamic-element lifecycle surface is DEFERRED (ledger D-39): Verter fails
        // it closed at the `<svelte:element>` attr gate — the transition sibling of
        // the `svelte_element_use` row (same refusal arm, distinct directive family).
        name: "svelte_element_transition",
        source: "<script>let t = $state('div');</script>\n<svelte:element this={t} transition:fade onclick={() => t}></svelte:element>\n",
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
    // ── out-of-allowlist intrinsic elements + special-content / form-control gates ─
    // The client core emits the §1.2 set plus the 5c bindings-breadth hosts
    // (`textarea`/`select`/`option`/`audio`/`details`); every other intrinsic tag
    // fails closed at the element gate (`svelte-runtime-unsupported-element`). The 5c
    // bind hosts are accepted as ELEMENTS but their non-bind special-content /
    // form-control attr forms fail closed at the content / attr gate. (The exhaustive
    // HTML-tag-universe cover is the dedicated ELEMENT MATRIX in
    // `svelte_element_attr_boundary.rs`.)
    FailRow {
        // `<select>`/`<option>` are now allowed bind hosts, so a STATIC `value` attr on
        // `<option>` (the form-control setter family — 5c owns `bind:value`, not the
        // static-`value` serializer) fails closed at the form-control attr gate.
        name: "select_option_static_value_attr",
        source: "<script>let c = $state(0);</script>\n<select><option value=\"a\">A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
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
    // ── `slot` attribute placement / validity (official slot_attribute_* errors) ──
    FailRow {
        // A `slot="a"` on an element that is NOT a direct component child (top level) —
        // the official `slot_attribute_invalid_placement` compile error stays fail-closed.
        name: "slot_attr_outside_component_child",
        source: "<script>let c = $state(0);</script>\n<div slot=\"a\">{c}</div>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A `slot="a"` NESTED inside a component child (not a DIRECT child) — the same
        // official placement error; the accept set is exactly the SOURCE-LEVEL direct
        // slot-declaring children of the component.
        name: "slot_attr_nested_in_component_child",
        source: "<script>import Child from './Child.svelte'; let c = $state(0);</script>\n<Child><div><span slot=\"a\">{c}</span></div></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A NAMED `slot="bar"` NESTED inside a transparent `<svelte:fragment slot="foo">`
        // — the fragment's children are HOISTED into the `foo` region at lowering, but
        // hoisting does NOT make them direct slot-declaring component children: the inner
        // `slot="bar"` is the same official `slot_attribute_invalid_placement` error and
        // must fail closed, never bake `slot="bar"` into the `foo` callback or mint a
        // `bar` slot.
        name: "slot_attr_nested_in_slotted_fragment",
        source: "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><svelte:fragment slot=\"foo\"><span slot=\"bar\">{x}</span></svelte:fragment></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // The same nested-placement error with `slot="default"` inside a slotted
        // fragment — `default` gets NO special dispensation: a hoisted fragment child
        // is not a direct component child, so the inner `slot="default"` fails closed.
        name: "slot_attr_default_nested_in_slotted_fragment",
        source: "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><svelte:fragment slot=\"foo\"><span slot=\"default\">{x}</span></svelte:fragment></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A DYNAMIC `slot={x}` — the official `slot_attribute_invalid` compile error
        // ("slot attribute must be a static value"); never a generic `$.set_attribute`.
        name: "slot_attr_dynamic",
        source: "<script>import Child from './Child.svelte'; let c = $state('a');</script>\n<Child><span slot={c}>hi</span></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A MIXED `slot="a{x}"` — the same official `slot_attribute_invalid` error.
        name: "slot_attr_mixed",
        source: "<script>import Child from './Child.svelte'; let c = $state('a');</script>\n<Child><span slot=\"a{c}\">hi</span></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // TWO children carrying the SAME `slot` name — the official
        // `slot_attribute_duplicate` compile error; never a silently merged region.
        name: "slot_attr_duplicate",
        source: "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"a\">{x}</span><p slot=\"a\">2</p></Child>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // An explicit `slot=\"default\"` child ALONGSIDE implicit default content — the
        // official `slot_default_duplicate` compile error.
        name: "slot_attr_default_conflict",
        source: "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child>{x}<span slot=\"default\">1</span></Child>\n",
        code: "svelte-runtime-unsupported-component",
    },
    // ── `slot` attribute on COMPONENT / `<svelte:*>` hosts (unified choke-point) ──
    // Official's three-class disposition: a STATIC `slot` on a DIRECT component-family
    // child routes into `$$slots` (accepted — positive coverage lives in the emission
    // tests + oracle corpus); a `slot` on a NON-direct component-family host is an
    // ordinary plain prop (accepted); everything else — a dynamic/mixed `slot` on a
    // DIRECT child, any `slot` on a non-filler special, an element outside filler
    // placement — fails closed here.
    FailRow {
        // A DYNAMIC `slot={x}` on a DIRECT component child — the official
        // `slot_attribute_invalid` compile error ("must be a static value" fires at
        // `owner === parent`); the NON-direct dynamic form is a plain prop instead.
        name: "slot_attr_dynamic_on_component",
        source: "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let x = $state('a');</script>\n<Child><Inner slot={x}/></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // The MIXED `slot="a{x}"` form on a DIRECT component child — the same official
        // `slot_attribute_invalid` reject.
        name: "slot_attr_mixed_on_component",
        source: "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let x = $state('a');</script>\n<Child><Inner slot=\"a{x}\"/></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A DYNAMIC `slot={x}` on a DIRECT `<svelte:component>` child — the same
        // official `slot_attribute_invalid` reject.
        name: "slot_attr_dynamic_on_svelte_component_child",
        source: "<script>import Child from './Child.svelte'; let x = $state('a');</script>\n<Child><svelte:component this={Child} slot={x}/></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A DYNAMIC `slot={x}` on a DIRECT `<svelte:self>` child — the same official
        // `slot_attribute_invalid` reject.
        name: "slot_attr_dynamic_on_svelte_self_child",
        source: "<script>import Child from './Child.svelte'; let x = $state('a');</script>\n<Child><svelte:self slot={x}/></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A DYNAMIC `slot={x}` on a DIRECT `<svelte:element>` child — the same official
        // `slot_attribute_invalid` reject (the static direct form folds into
        // `$.attribute_effect` instead).
        name: "slot_attr_dynamic_on_svelte_element_child",
        source: "<script>import Child from './Child.svelte'; let x = $state('a');</script>\n<Child><svelte:element this=\"div\" slot={x}/></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // An explicit `slot=\"default\"` on a DIRECT COMPONENT child — official's
        // `slot_default_duplicate` walk exempts only a RegularElement /
        // `<svelte:fragment>` sibling carrying a `slot` attribute, so a
        // component-family `slot=\"default\"` child conflicts with ITSELF.
        name: "slot_attr_default_on_component_child",
        source: "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Child><Inner slot=\"default\"/></Child>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // A STATIC `slot="a"` on a TOP-LEVEL `<svelte:element>` — the official
        // `slot_attribute_invalid_placement` reject (the element family is a filler
        // host ONLY at direct component-child placement, never a plain-prop host).
        name: "slot_attr_on_svelte_element",
        source: "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n<svelte:element this=\"div\" slot=\"a\"></svelte:element>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A DYNAMIC `slot={c}` on a top-level `<svelte:element>` — same refusal.
        name: "slot_attr_dynamic_on_svelte_element",
        source: "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n<svelte:element this=\"div\" slot={c}></svelte:element>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    // ── D-43 custom-element-host / native-slotting over-refusals (official
    //    ACCEPTS the whole class; DEFERRED, ledger D-43). The class is scoped at
    //    the ROOT-limitation level on TWO rails — rail 1: the custom-element
    //    HOST gate (any hyphenated / `is=`-carrying participant refuses as
    //    `host-custom-element` before attribute or child classification); rail 2:
    //    the `validate_slot_placement` choke-point (a slot-bearing child owned
    //    by `<svelte:element>` refuses as `dynamic-attribute`). The class is
    //    pinned by the generic custom-element host-gate rows
    //    (`custom_element_attr` / `custom_element_static_attr` /
    //    `custom_element_no_attr` / `customized_builtin_static_attr`) plus the
    //    `validate_slot_placement_disposition_is_exhaustive_per_host_kind` unit
    //    proof; the rows below are REPRESENTATIVE smoke probes, intentionally
    //    NON-exhaustive. ─────────────────────────────────────────────────────
    FailRow {
        // RAIL 1 (custom-element host gate): a slot-bearing child under a
        // hyphenated custom-element owner. Representative smoke probe, not exhaustive.
        name: "slot_attr_under_custom_element_owner",
        source: "<script>let c = $state(0);</script>\n<my-element><div slot=\"x\">a</div></my-element>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // RAIL 1 (custom-element host gate): a slot-bearing child under a
        // customized-built-in (`is=`) owner. Representative smoke probe, not exhaustive.
        name: "slot_attr_under_customized_builtin_owner",
        source: "<script>let c = $state(0);</script>\n<button is=\"my-btn\"><div slot=\"x\">a</div></button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // RAIL 1 (custom-element host gate): a custom-element host as a DIRECT
        // component slot filler. Representative smoke probe, not exhaustive.
        name: "custom_element_as_direct_slot_filler",
        source: "<script>import Child from './Child.svelte'; let c = $state(0);</script>\n<Child><my-element slot=\"x\">a</my-element></Child>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // RAIL 1 (custom-element host gate): a component-family `slot` plain-prop
        // filler under a custom-element owner. Representative smoke probe, not exhaustive.
        name: "component_filler_under_custom_element_owner",
        source: "<script>import Inner from './Inner.svelte'; let c = $state(0);</script>\n<my-element><Inner slot=\"x\" /></my-element>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // RAIL 2 (`validate_slot_placement` choke-point): a slot-bearing child
        // whose owner is `<svelte:element>`. Representative smoke probe, not exhaustive.
        name: "slot_attr_under_svelte_element_owner",
        source: "<script>let t = $state('div');</script>\n<svelte:element this={t}><div slot=\"x\">a</div></svelte:element>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // RAIL 2 (`validate_slot_placement` choke-point): a `<svelte:element slot>`
        // child whose owner is itself a `<svelte:element>`. Representative smoke probe.
        name: "svelte_element_slot_under_svelte_element_owner",
        source: "<script>let t = $state('div');</script>\n<svelte:element this={t}><svelte:element this=\"span\" slot=\"x\">a</svelte:element></svelte:element>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    // (NO `<svelte:options slot>` row: `<svelte:options>` attributes are validated at
    // PARSE — `slot` is refused as the official `svelte_options_unknown_attribute`
    // reject-parity defect (`svelte_parse_defect_exact_codes.rs`) before lowering ever
    // runs, so the node can never reach the slot choke-point and a row here would be
    // non-discriminating. The choke-point still covers the `Options` kind at the
    // function level via the per-kind unit proof.)
    // (NO `<svelte:window|document|body slot>` rows either: the global-host classifier
    // already refuses EVERY non-event/bind attribute with the SAME
    // `svelte-runtime-unsupported-dynamic-attribute` code, so an SFC row would pass
    // identically pre/post choke-point — non-discriminating. The per-kind unit proof
    // covers all three kinds at the choke-point itself.)
    FailRow {
        // A `slot="x"` on `<svelte:boundary>` — formerly refused as a generic boundary
        // attribute (`svelte-runtime-unsupported-component`); the choke-point now owns
        // the refusal with the slot diagnostic (slot validation runs BEFORE per-host
        // attribute acceptance).
        name: "slot_attr_on_svelte_boundary",
        source: "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n<svelte:boundary slot=\"x\"><p>b</p></svelte:boundary>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A `slot="x"` on `<svelte:head>` — formerly the head-attribute reject-parity
        // arm (`svelte-runtime-unsupported-component`); the choke-point now owns the
        // refusal with the slot diagnostic.
        name: "slot_attr_on_svelte_head",
        source: "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n<svelte:head slot=\"x\"><title>t</title></svelte:head>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    FailRow {
        // A `slot="x"` on a DIRECT-child `<svelte:boundary>` — a boundary is never a
        // slot host (official `svelte_boundary_invalid_attribute`), so the direct
        // component-child placement gets no filler dispensation.
        name: "slot_attr_on_svelte_boundary_child",
        source: "<script>import Child from './Child.svelte'; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n<Child><svelte:boundary slot=\"x\"><p>b</p></svelte:boundary></Child>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    // (NO direct-child `<svelte:head slot>` row: a `<svelte:head>` nested inside a
    // component child is refused on the OFFICIAL-reject rail with the exact official
    // code `svelte_meta_invalid_placement` BEFORE the slot choke-point can run, so it
    // never reaches this unsupported-feature matrix — the emission-level test
    // `svelte_head_inside_component_child_rejects_as_meta_placement` pins that rail.)
    FailRow {
        // A `slot="x"` on a STANDALONE `<svelte:fragment>` (not a component child) —
        // formerly the transparent-fragment construct refusal
        // (`svelte-runtime-unsupported-component`); the choke-point now owns the
        // refusal with the slot diagnostic.
        name: "slot_attr_on_standalone_fragment",
        source: "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n<svelte:fragment slot=\"x\">hi</svelte:fragment>\n",
        code: "svelte-runtime-unsupported-dynamic-attribute",
    },
    // (`span_element` removed — `<span>` joined the element allowlist as the plain
    // structural host the component-slot / `{#snippet}`-body fixtures need, so a `<span>`
    // no longer fails closed. `<ul>` / `<li>` did NOT join and remain rejected.)
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
        // An instance-script `export const` is the `$$exports` component-export
        // surface — its OWN fail-closed identity (any mode), never the generic
        // instance-script-item residual and never a prop surface.
        name: "instance_export",
        source: "<script>let c = $state(0); export const FOO = 1;</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-component-export-binding",
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
    // NOTE: a `$:` reactive label in a RUNES-mode component (`$state` + `$:`) is an
    // OFFICIAL EXACT-CODE compile error (`legacy_reactive_statement_invalid`), so it
    // fails closed through the official-reject channel
    // (`ClientCompileError::OfficialReject`), NOT this unsupported-feature matrix — its
    // parity rows live in `svelte_client_official_reject_matrix.rs`
    // (`runes_reactive_statement`, `runes_reactive_statement_inferred`). The LEGACY-mode
    // `$:` is SUPPORTED (it lowers through `$.legacy_pre_effect` — the positive goldens
    // are the `legacy/reactive_*` corpus rows), and a `$:` dependency CYCLE is the
    // official `reactive_declaration_cycle` reject (`reactive_declaration_cycle` corpus
    // row).
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
    // (`snippet_in_if_block` + `render_in_each_block` removed — a `{#snippet}` declaration
    // and a `{@render}` tag inside a control-flow block body are the 5f-a surface and now
    // emit a block-local snippet const / `$.snippet` render.)
    // ── Declaration-tag PLACEMENT (5e) — non-region-root fails closed ────────
    FailRow {
        // A `{@const}` NESTED inside an element (not a block-body region root). The official
        // compiler rejects this placement (`const_tag_invalid_placement`); Verter fails
        // closed rather than the roots-only hoist that silently DROPPED a nested `{@const}`.
        name: "const_tag_nested_in_element",
        source: "<script>let { items } = $props();</script>\n{#each items as item}<p>{@const x = item}</p>{/each}\n",
        code: "svelte-runtime-unsupported-block",
    },
    FailRow {
        // A bare `{let …}` declaration tag NESTED inside an element. This is NOT a defect:
        // the official compiler ACCEPTS a nested DeclarationTag via per-element
        // `BlockStatement` scoping (element-local scope + a `$.template_effect` split).
        // Verter does not emit that element-local lowering yet, so it fails closed here — a
        // ratified DEFER deferral (codex DECIDER ruling) owned by the nested element-scope
        // codegen axis (D-36), never the silent drop the roots-only hoist produced.
        name: "decl_let_nested_in_element",
        source: "<script>let { items } = $props();</script>\n{#each items as item}<p>{let x = item}</p>{/each}\n",
        code: "svelte-runtime-unsupported-block",
    },
    FailRow {
        // A bare `{const …}` declaration tag NESTED inside an element — same ratified DEFER
        // deferral as the `{let …}` form (nested element-scope codegen axis, D-36): the
        // official accepts it via per-element `BlockStatement` scoping; Verter fails closed
        // until that element-local lowering lands, never the silent drop the roots-only hoist
        // produced.
        name: "decl_const_nested_in_element",
        source: "<script>let { items } = $props();</script>\n{#each items as item}<p>{const x = item}</p>{/each}\n",
        code: "svelte-runtime-unsupported-block",
    },
    FailRow {
        // A `{@const}` at the COMPONENT ROOT (not a block-body region root). The official
        // compiler rejects this placement — the component root is not a `{@const}` valid
        // parent. (A bare `{const}` / `{let}` at the component root is VALID and accepted.)
        name: "const_tag_at_component_root",
        source: "<script>let c = $state(0);</script>\n{@const x = 1}\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-block",
    },
    FailRow {
        // A MEMBER read inside reactive text WITHIN a block body (`{item.x}`): block-body
        // content is scoped to the BARE-signal reactive-text surface, so a member read is
        // the interpolation-breadth surface (owned by the reactive-text/interpolation
        // completion work) and fails closed — it is NOT emitted, matching the topology
        // corpus comment that excludes `{item.label}` block-body member reads.
        name: "block_body_member_interpolation",
        source: "<script>let { items } = $props();</script>\n{#each items as item}<p>{item.x}</p>{/each}\n",
        code: "svelte-runtime-unsupported-complex-interpolation",
    },
    // (`component` removed — a component reference is the 5f-a surface and now emits a
    // direct `Foo($$anchor, {})` call.)
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
    FailRow {
        name: "derived_simple",
        source: "<script>let c = $state(0); let d = $derived(c + 1);</script>\n<button onclick={() => c++}>{d}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    // (`effect_toplevel` removed — a top-level `$effect(fn);` statement is now a
    // supported instance-script item; its positive topology is pinned by the
    // `matrix/effect_arrow` golden + the `effect_toplevel_statement_lowers_with_frame`
    // client test, and by `generated_effect_shapes_land_on_boundary` below.)
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
        name: "event_arrow_call_body",
        source: "<script>let c = $state(0); function f(x) { return x; }</script>\n<button onclick={() => f(c)}>{c}</button>\n",
        code: "svelte-runtime-unsupported-non-delegated-event",
    },
    // ── CONVERGENCE: module scripts (import-only admitted; items deferred) ───
    FailRow {
        // A non-import module item (`const K = 1`) — an HONEST DEFERRAL, not
        // official parity: official svelte@5.56.3 ACCEPTS a non-import
        // `<script module>` item (an `export const K = 1;`, a plain statement, an
        // empty statement — oracle-probed), while the admitted `<script module>` is
        // IMPORT-ONLY here and everything else fails closed under
        // `ModuleScriptItem` until module-item lowering lands (the script/module-item
        // completion block; see the module-shape row in
        // `docs/arch/svelte-native-compiler-plan.md`). Static imports themselves
        // (both slots) are POSITIVE `imports/*` corpus rows now, and a module-item
        // that COLLIDES with an official reject (a cross-script duplicate, a same-body
        // duplicate) carries its exact official code instead of this surface.
        name: "module_script",
        source: "<script module>const K = 1;</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-module-script-item",
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
    // ── $state over a REACTIVE-shadowed `undefined` ─────────────────────────
    // `let undefined = $state(0); let x = $state(undefined)` — `undefined` is shadowed
    // by a REACTIVE $state SIGNAL, so its read at the init site is `$.get(undefined)`
    // (a CallExpression) which official PROXIES (`$.state($.proxy($.get(undefined)))`).
    // Verter's `expr_is_proxiable` hardcodes the `undefined` identifier non-proxiable
    // and would omit the `$.proxy`, so it fails closed. (A PLAIN-local `undefined`
    // shadow reads plain and is NOT refused at the state gate — see the F5 tests.)
    FailRow {
        name: "state_reactive_shadowed_undefined_init",
        source: "<script>let undefined = $state(0); let x = $state(undefined);</script>\n<button onclick={() => { undefined++; x++; }}>{x}{undefined}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    // ── $state / $state.raw with a SPREAD argument ──────────────────────────
    // `$state(...x)` / `$state.raw(...x)` is the official `rune_invalid_spread` compile
    // error ("`$state` cannot be called with a spread argument"). A single spread arg is
    // `arguments.len() == 1` with no `as_expression`, so it slips past the arity /
    // init-shape gates and would emit `void 0` — it MUST fail closed instead (F2).
    FailRow {
        name: "state_spread_argument",
        source: "<script>let a = [1]; let x = $state(...a);</script>\n<button onclick={() => x = 2}>{x}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        name: "state_raw_spread_argument",
        source: "<script>let a = [1]; let x = $state.raw(...a);</script>\n<button onclick={() => x = 2}>{x}</button>\n",
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
    // ── `{@render …(…spread)}` — a SPREAD argument in a render tag ───────────
    // Official `svelte@5.56.3` HARD-ERRORS (`render_tag_invalid_spread_argument`:
    // "cannot use spread arguments in {@render ...} tags"). Verter must FAIL CLOSED
    // rather than silently DROP the spread args and emit a wrong-arity `$.snippet`
    // call. The refusal is NARROW: a non-spread render arg (`{@render row(item)}`)
    // still emits the argument thunk. Covers a PROP, a LOCAL-snippet, and a DYNAMIC
    // (optional-call) callee — every callee shape over-accepts the spread today.
    FailRow {
        name: "render_spread_prop_callee",
        source: "<script>let { row, xs } = $props();</script>\n{@render row(...xs)}\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        name: "render_spread_local_snippet_callee",
        source: "<script>let { xs } = $props();</script>\n{#snippet row()}<span>x</span>{/snippet}\n{@render row(...xs)}\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        name: "render_spread_dynamic_callee",
        source: "<script>let { row, xs } = $props();</script>\n{@render row?.(...xs)}\n",
        code: "svelte-runtime-unsupported-component",
    },
    // The same render-spread refusal is closed over OUTER author parens wrapping the WHOLE
    // call: official HARD-ERRORS on the spread regardless of how many parens wrap the call,
    // so a parenthesized (single, nested, or optional) whole call — and a parenthesized
    // LOCAL-snippet callee — must fail closed identically to the bare form, never peel to a
    // non-call node and silently drop the spread.
    FailRow {
        name: "render_spread_paren_whole_call",
        source: "<script>let { row, xs } = $props();</script>\n{@render (row(...xs))}\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        name: "render_spread_double_paren_whole_call",
        source: "<script>let { row, xs } = $props();</script>\n{@render ((row(...xs)))}\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        name: "render_spread_paren_optional_call",
        source: "<script>let { row, xs } = $props();</script>\n{@render (row?.(...xs))}\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        name: "render_spread_paren_local_snippet",
        source: "<script>let { xs } = $props();</script>\n{#snippet row()}<span>x</span>{/snippet}\n{@render (row(...xs))}\n",
        code: "svelte-runtime-unsupported-component",
    },
    // ── `<svelte:self>` at INVALID (root) placement ─────────────────────────
    // Official `svelte@5.56.3` HARD-ERRORS (`svelte_self_invalid_placement`:
    // "<svelte:self> can only exist inside {#if}/{#each}/{#snippet} blocks or slots
    // passed to components"). A ROOT `<svelte:self>` (bare or `bind:this`) has NO
    // allowed enclosing context — Verter must FAIL CLOSED rather than emit the
    // recursive `App(node, …)` / `$.bind_this(App(node, …), …)` self-call. Valid
    // placement (inside a block / component slot) still emits the recursive call.
    FailRow {
        name: "svelte_self_root_placement",
        source: "<script>let { depth } = $props();</script>\n<svelte:self />\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        name: "svelte_self_root_bind_this",
        source: "<script>let { depth } = $props(); let x;</script>\n<svelte:self bind:this={x} />\n",
        code: "svelte-runtime-unsupported-component",
    },
    // ── `$host` malformed / out-of-context forms ─────────────────────────────
    // The ONLY supported `$host` form is the ZERO-ARG, NON-OPTIONAL, bare-callee
    // `$host()` call inside an ACTIVE customElement component (it lowers to
    // `$$props.$$host`). Every other spelling fails closed. The bare-reference and
    // paren-callee rows use the RUNES two-button isolation form (a real `$state`
    // puts the component in runes mode and the `$host` handler sits on its OWN
    // element, isolated from other refusable surfaces); a template `$host`
    // occurrence is itself a runes-mode indicator too (the scriptless runeless
    // twins are pinned below as the template-`$host` inference rows), so the
    // isolation here only keeps each row's refusal surface singular.
    FailRow {
        // An UNCALLED bare `$host` reference (official `rune_missing_parentheses`).
        // All unshadowed rune roots fail closed outside a supported position; `$host`
        // has NO supported bare position.
        name: "host_bare_reference_runes_isolated",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => $host}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A PARENTHESIZED-callee call `($host)()` (official `host_invalid_placement`
        // outside a custom element; the strict supported spelling does not peel).
        // The inner bare `$host` fails the position scan.
        name: "host_paren_callee_call_runes_isolated",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => ($host)()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A well-formed `$host()` call OUTSIDE a customElement component (official
        // `host_invalid_placement`) — stays the host/custom-element refusal.
        name: "host_call_outside_custom_element",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => $host()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$host(x)` — arity 1 (official `rune_invalid_arguments`), outside a
        // customElement context.
        name: "host_call_arity_one",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => $host(1)}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$host(x, y)` — arity 2+ (official `rune_invalid_arguments`).
        name: "host_call_arity_two",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => $host(1, 2)}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$host(...a)` — a spread argument (official `rune_invalid_spread`).
        name: "host_call_spread_argument",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => $host(...[1])}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$host?.()` — an OPTIONAL call is not the supported spelling.
        name: "host_optional_call",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => $host?.()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$host.x` — a member access on the rune root (official rejects; the
        // member classifier's `$rune.<member>` arm).
        name: "host_member_access",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => $host.focus}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // `$host?.x` — an OPTIONAL member access on the rune root.
        name: "host_optional_member_access",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => $host?.focus}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A bare `$host` in STATEMENT position inside a handler BLOCK body
        // (`() => { $host; }`) — the position scan refuses the uncalled reference
        // regardless of statement-vs-value position (the value-position twin is
        // `host_bare_reference_runes_isolated`).
        name: "host_bare_statement_position_in_handler",
        source: "<script>let c = $state(0);</script>\n<button onfocus={() => { $host; }}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A bare `$host;` STATEMENT in the instance script — the instance-script
        // position scan owns it (no supported bare position exists anywhere).
        name: "host_bare_statement_in_instance_script",
        source: "<script>let c = $state(0); $host;</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A bare `$host` as a DECLARATOR INIT (`const h = $host`) — the decl-init
        // position is not a supported bare position either.
        name: "host_bare_decl_init_in_instance_script",
        source: "<script>let c = $state(0); const h = $host;</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // PARAM-SHADOW invariant: `function f($host) { return $host; }` — the
        // param genuinely shadows the rune, so the body reference is USER JS and
        // is NOT rune-refused; the function itself stays the GENERIC
        // instance-script-item refusal (any rune-coded refusal here would mean the
        // shadow scope was ignored).
        name: "host_param_shadow_function_stays_generic_refusal",
        source: "<script>let c = $state(0); function f($host) { return $host; }</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-instance-script-item",
    },
    // ── `$host` malformed siblings INSIDE an active customElement ────────────
    // The customElement descriptor ADMITS exactly the zero-arg, non-optional,
    // bare-callee `$host()` call in a supported handler/template expression
    // position; every malformed sibling fails closed IN CONTEXT too (the accept
    // must not fail-open its own context).
    FailRow {
        // The uncalled bare reference inside the admitting context.
        name: "host_bare_reference_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // The parenthesized-callee spelling inside the admitting context — the
        // strict supported spelling does not peel parens.
        name: "host_paren_callee_call_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => ($host)()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // `$host(1)` — arity 1 inside the admitting context (official
        // `rune_invalid_arguments`).
        name: "host_call_arity_one_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host(1)}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$host(1, 2)` — arity 2+ inside the admitting context.
        name: "host_call_arity_two_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host(1, 2)}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$host(...a)` — a spread argument inside the admitting context
        // (official `rune_invalid_spread`).
        name: "host_call_spread_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host(...[1])}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$host?.()` — the OPTIONAL call is not the supported spelling, even in
        // context.
        name: "host_optional_call_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host?.()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$host.x` — a member access on the rune root inside the admitting
        // context (the `$rune.<member>` arm).
        name: "host_member_access_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host.focus}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // `$host?.x` — the optional member twin.
        name: "host_optional_member_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host?.focus}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-advanced-rune",
    },
    FailRow {
        // A WELL-FORMED `$host()` as a const-declarator init inside the admitting
        // context (the unused instance-top form): the SCAN admits the call,
        // but a const declaration is not an admitted instance item — the generic
        // item refusal keeps it closed (official's own output for this form
        // references `$$props` with NO binding — runtime-broken; Verter never
        // emits it).
        name: "host_call_const_decl_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0); const h = $host();</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-instance-script-item",
    },
    FailRow {
        // A WELL-FORMED `$host();` bare STATEMENT in the instance script inside
        // the admitting context — an expression statement is not an admitted
        // instance item.
        name: "host_call_bare_statement_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0); $host();</script>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-instance-script-item",
    },
    FailRow {
        // A WELL-FORMED `$host()` in an INTERPOLATION (`<p>{$host()}</p>`) inside
        // the admitting context — the interpolation position does not lower the
        // host read today; it fails closed at the interpolation classifier
        // (never a raw `$host` in emitted output).
        name: "host_call_interpolation_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<p>{$host()}</p>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-complex-interpolation",
    },
    // ── `$host` DEGENERATE-UNBOUND residue (well-formed, admitted, un-bound) ──
    // Official `svelte@5.56.3` rewrites every ADMITTED `$host()` to
    // `$$props.$$host`, but binds the `$$props` PARAMETER only when an
    // independent props-parameter trigger exists: a REAL props binder
    // (`$props()` / `$bindable(...)` / legacy prop) or a `needs_context`
    // reason — a member on the `$host()` call result ITSELF (`$host().x`,
    // `$host()?.x`, `$host()['x']`, `$host().m()`, a `{@render $host().snip()}`
    // dynamic callee) IS such a reason (a call-result-rooted member is never a
    // "safe identifier"). With NEITHER, official emits `function App($$anchor)`
    // (NO `$$props` param) while the body still references `$$props.$$host` — a
    // runtime `ReferenceError` residue. Verter refuses that degenerate class
    // instead of silently repairing the binding:
    // `refuse iff host_used && !props_param_bound`.
    FailRow {
        // The bare-RESULT handler (`() => $host()`): the call result is
        // discarded — no binder, no context reason, no member access.
        name: "host_bare_result_degenerate_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // The bare-STATEMENT handler (`() => { $host(); }`) — same degenerate
        // class in statement position.
        name: "host_bare_statement_degenerate_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => { $host(); }}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // ASSIGNED but never member-accessed (`const h = $host();` in a handler
        // block): assignment alone binds nothing in official either.
        name: "host_assigned_unused_degenerate_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => { const h = $host(); }}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // ALIAS-member (`const h = $host(); h.x;`) is NOT a binding trigger:
        // official still emits the unbound `function App($$anchor)` degenerate
        // for it (first-hand pinned probe) — member access binds ONLY on the
        // `$host()` call result ITSELF, never through a local alias. The local
        // `h` is a plain (safe) identifier for the context scan too, so no
        // `needs_context` reason exists either.
        name: "host_alias_member_degenerate_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => { const h = $host(); h.x; }}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // ARG-position (`() => sink($host())`): passing the host value to a
        // SAFE (global) callee is not a member access and not a context
        // reason — official stays degenerate-unbound.
        name: "host_arg_position_degenerate_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => sink($host())}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // A customElement `props: {...}` DECLARATION alone is NOT a props
        // binder (official binds only for `$props()` / `$bindable` / legacy
        // props — the CE accessor declaration never binds `$$props`): a bare
        // `$host()` beside it stays the degenerate refusal.
        name: "host_ce_props_declaration_only_degenerate",
        source: "<svelte:options customElement={{ tag: 'x-m', props: { count: { reflect: true } } }} />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `$props.id()` is NOT a props binder (the id const is a plain one-shot
        // local, not a `$$props`-parameter trigger) — a bare `$host()` beside
        // it stays the degenerate refusal.
        name: "host_props_id_only_degenerate_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0); const uid = $props.id();</script>\n<button onfocus={() => $host()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    // ── TEMPLATE-`$host` runes inference (no script rune to mask the mode) ───
    // A `$host` occurrence in a TEMPLATE expression is a runes-mode indicator by
    // itself (official `metadata.runes === true` for a scriptless template-only
    // `$host()` component), so these RUNELESS forms reach the `$host` gates —
    // NOT the legacy per-surface dispatch.
    FailRow {
        // A well-formed template `$host()` with NO customElement and NO script:
        // runes mode is inferred FROM the template `$host` reference, and the
        // out-of-context call then refuses (official `host_invalid_placement`).
        // (`onfocus` keeps the handler on the DIRECT `$.event` surface, the
        // same isolation every `$host` row uses.)
        name: "host_call_non_custom_element_template_runeless",
        source: "<button onfocus={() => $host()}>go</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // The scriptless template-only BARE `$host()` customElement: runes mode
        // is inferred from the template `$host`, the call is admitted, and the
        // component then has NO props-parameter trigger at all (no binder, no
        // context reason, no member access) — official emits the
        // degenerate-unbound `function App($$anchor)` residue; Verter refuses.
        name: "host_template_only_bare_degenerate_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<button onfocus={() => $host()}>go</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    // ── `{@render}` dynamic-callee `$host` MALFORMED SIBLINGS ────────────────
    // The accepted form is `{@render $host().snip()}` (a well-formed zero-arg
    // `$host()` member callee — it opens the context frame and binds `$$props`
    // through `needs_context`). Every malformed sibling around that accept
    // fails closed: a malformed INNER rune call (arity / spread / optional)
    // refuses at the rune scan exactly as its handler-position twin does; an
    // UNCALLED render reference and a BARE-SPREAD snippet argument refuse at
    // the render projection (official `render_tag_invalid_expression` /
    // `render_tag_invalid_spread_argument` parity).
    FailRow {
        // `{@render $host(1).snip()}` — inner-rune arity 1 (official
        // `rune_invalid_arguments`) in render-callee position.
        name: "render_callee_host_arity_one_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n{@render $host(1).snip()}\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `{@render $host(1, 2).snip()}` — inner-rune arity 2+.
        name: "render_callee_host_arity_two_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n{@render $host(1, 2).snip()}\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `{@render $host(...[1]).snip()}` — inner-rune spread argument
        // (official `rune_invalid_spread`).
        name: "render_callee_host_spread_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n{@render $host(...[1]).snip()}\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `{@render $host?.().snip()}` — the OPTIONAL inner call is not the
        // supported `$host` spelling, in render position either.
        name: "render_callee_host_optional_call_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n{@render $host?.().snip()}\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-host-custom-element",
    },
    FailRow {
        // `{@render $host().snip}` — an UNCALLED render reference (no terminal
        // snippet call; official `render_tag_invalid_expression`): the render
        // projection refuses the non-call expression.
        name: "render_uncalled_host_member_reference_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n{@render $host().snip}\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-component",
    },
    FailRow {
        // `{@render $host().snip(...[1])}` — a BARE SPREAD snippet argument
        // (official `render_tag_invalid_spread_argument`) on the member-callee
        // accept: the spread refusal fires regardless of the callee shape.
        name: "render_host_member_callee_spread_snippet_arg_inside_custom_element",
        source: "<svelte:options customElement=\"x-m\" />\n<script>let c = $state(0);</script>\n{@render $host().snip(...[1])}\n<button onclick={() => c++}>{c}</button>\n",
        code: "svelte-runtime-unsupported-component",
    },
    // ── `$store` SCOPED subscriptions — official `store_invalid_scoped_subscription` ──
    // A `$NAME` reference whose BASE resolves in the expression's real lexical
    // scope to a NON-top-level binding — a `{#each as x}` alias, a `{#snippet}`
    // parameter, an `{#await then x}` binding, a script function parameter —
    // is an official COMPILE ERROR ("Cannot subscribe to stores that are not
    // declared at the top level of the component"), NEVER a subscription over
    // the shadowed top-level store. Each row pairs a top-level store base with
    // a same-name template-block / script-local binding; the scope-aware store
    // classifier rejects it (fail-closed, matching the official reject).
    FailRow {
        // `{#each items as x}{$x}` with a top-level `const x = writable(1)`:
        // the each ALIAS owns `x` inside the body — official rejects.
        name: "store_scoped_subscription_each_alias_shadow",
        source: "<script>import { writable } from 'svelte/store'; const x = writable(1); let { items } = $props();</script>\n{#each items as x}<p>{$x}</p>{/each}\n",
        code: "svelte-runtime-unsupported-store-scoped-subscription",
    },
    FailRow {
        // `{#snippet snip(x)}{$x}` — the snippet PARAMETER owns `x`.
        name: "store_scoped_subscription_snippet_param_shadow",
        source: "<script>import { writable } from 'svelte/store'; const x = writable(1);</script>\n{#snippet snip(x)}<p>{$x}</p>{/snippet}\n{@render snip(2)}\n",
        code: "svelte-runtime-unsupported-store-scoped-subscription",
    },
    FailRow {
        // `{#await p then x}{$x}` — the await THEN binding owns `x`.
        name: "store_scoped_subscription_await_then_shadow",
        source: "<script>import { writable } from 'svelte/store'; const x = writable(1); let { p } = $props();</script>\n{#await p then x}<p>{$x}</p>{/await}\n",
        code: "svelte-runtime-unsupported-store-scoped-subscription",
    },
    FailRow {
        // A SCRIPT-side base shadow: `function f(x) { return $x; }` — the
        // function parameter owns `x` at the `$x` reference (probe-verified:
        // official rejects the scoped subscription in script positions too).
        name: "store_scoped_subscription_script_fn_param_shadow",
        source: "<script>import { writable } from 'svelte/store'; const x = writable(1); function f(x) { return $x; }</script>\n<button onclick={f}>go</button>\n",
        code: "svelte-runtime-unsupported-store-scoped-subscription",
    },
    FailRow {
        // A TEMPLATE-EXPRESSION-internal base shadow: `onclick={(x) => $x}` —
        // the arrow parameter INSIDE the analyzed expression owns `x` at the
        // `$x` read (the per-reference base-shadow fact rejects it).
        name: "store_scoped_subscription_template_arrow_param_shadow",
        source: "<script>import { writable } from 'svelte/store'; const x = writable(1);</script>\n<button onclick={(x) => $x}>go</button>\n",
        code: "svelte-runtime-unsupported-store-scoped-subscription",
    },
    // ── rune-USAGE × store-SUBSCRIPTION name collision — fail closed ──────
    // A rune-root-NAMED store accessor (`$state` over `const state = writable(0)`)
    // subscribed in the template WHILE the SAME rune root has live admitted usage
    // (`let n = $state(1)`) is a DIVERGENT-mode case: official treats EVERY
    // `$state` reference as the store accessor and compiles LEGACY
    // (`let n = $state()(1);`), a lowering this backend does not implement. It
    // must fail closed with the precise instance-script-item diagnostic rather
    // than mis-emit a rune-lowered (`$.state`/`$.mutable_source`) module.
    FailRow {
        name: "store_rune_named_state_accessor_collides_with_state_rune_usage",
        source: "<script>import { writable } from 'svelte/store'; const state = writable(0); let n = $state(1);</script>\n<p>{$state}</p>\n<button onclick={() => n++}>{n}</button>\n",
        code: "svelte-runtime-unsupported-instance-script-item",
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
        "svelte-runtime-unsupported-module-script-item",
        "svelte-runtime-unsupported-typescript",
        "svelte-runtime-unsupported-complex-text",
        "svelte-runtime-unsupported-element-name",
        "svelte-runtime-unsupported-instance-script-item",
        "svelte-runtime-unsupported-component-export-binding",
        "svelte-runtime-unsupported-magic-identifier",
        "svelte-runtime-unsupported-paragraph-autoclose",
        "svelte-runtime-unsupported-store-scoped-subscription",
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

#[test]
fn valid_regular_element_and_fragment_fillers_still_compile() {
    // POSITIVE control for the unified slot choke-point: the SUPPORTED static
    // named-slot filler routes stay ACCEPTED — a direct regular-element filler and a
    // `<svelte:fragment slot>` filler both compile to a `Main` carrying the `$$slots`
    // region. Fail-closing the component/special `slot=` class must NOT close the
    // valid element/fragment route.
    let js = compile(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><span slot=\"foo\">{x}</span></Child>\n",
    )
    .expect("a direct element named-slot filler must still compile");
    assert!(
        js.contains("$$slots: {foo: ($$anchor, $$slotProps) =>"),
        "missing the $$slots region for the element filler:\n{js}"
    );
    // The `slot` attribute BAKES into the cloned skeleton (the official output keeps
    // it in the template HTML) — the accepted route is the filler, not a dropped attr.
    assert!(
        js.contains("<span slot=\"foo\">"),
        "the element filler's slot attribute must bake into the skeleton:\n{js}"
    );
    let fragment_js = compile(
        "<script>import Child from './Child.svelte'; let { x } = $props();</script>\n<Child><svelte:fragment slot=\"foo\">hello {x}</svelte:fragment></Child>\n",
    )
    .expect("a fragment named-slot filler must still compile");
    assert!(
        fragment_js.contains("$$slots: {foo: ($$anchor, $$slotProps) =>"),
        "missing the $$slots region for the fragment filler:\n{fragment_js}"
    );
    // A COMPONENT filler routes into `$$slots` AND keeps the `slot` prop on its own
    // call (the official direct-component-child disposition).
    let component_js = compile(
        "<script>import Child from './Child.svelte'; import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Child><Inner slot=\"foo\"/></Child>\n",
    )
    .expect("a direct component named-slot filler must compile");
    assert!(
        component_js.contains("$$slots: {foo: ($$anchor, $$slotProps) =>")
            && component_js.contains("Inner($$anchor, {slot: 'foo'})"),
        "the component filler must route into $$slots and keep the slot prop:\n{component_js}"
    );
    // A NON-direct component `slot` is an ordinary plain prop (no $$slots minted).
    let plain_js = compile(
        "<script>import Inner from './Inner.svelte'; let { x } = $props();</script>\n<Inner slot=\"top\"/>\n",
    )
    .expect("a top-level component slot prop must compile");
    assert!(
        plain_js.contains("Inner($$anchor, {slot: 'top'})") && !plain_js.contains("$$slots"),
        "the non-direct component slot must be a plain prop:\n{plain_js}"
    );
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
        // A fail-closed variant refuses with NO `Main` — through EITHER the
        // unsupported-feature channel (`Unsupported`) OR the official-reject channel
        // (`OfficialReject`, e.g. a `bind:value={a, b, c}` 3+-element sequence →
        // `bind_invalid_expression`). Both are valid fail-closed boundaries; only an emitted
        // `Main` (handled above) or a non-refusal error (e.g. `Lowering`) is a failure.
        (Err(ClientCompileError::Unsupported(_)), Expected::FailClosed) => {}
        (Err(ClientCompileError::OfficialReject(_)), Expected::FailClosed) => {}
        (Err(e), Expected::FailClosed) => {
            panic!("{label}: expected a fail-closed refusal (Unsupported or OfficialReject), got {e:?}");
        }
        (Err(e), Expected::Supported) => {
            panic!("{label}: expected a supported Main, got refusal {e:?}");
        }
    }
}

#[test]
fn generated_event_handler_expression_kinds_land_on_boundary() {
    // The finite grammar of a DELEGATED `onclick={…}` handler EXPRESSION kind. `onclick`
    // is a delegatable event, so its handler routes through the NARROW delegated boundary:
    // ONLY a non-async INLINE ARROW whose body is a `$state` assignment / update is
    // supported (the §1.2-class handler); EVERY other handler shape — a function
    // expression, a local-function identifier, a call, a bare update/assignment
    // (not an arrow), a member, a sequence, a conditional — needs the official wrapper /
    // statement-rewrite breadth and fails closed on the delegated path. An arrow whose
    // body is NOT a `$state` write (a call, a non-`$state` update) ALSO fails closed.
    //
    // These rows characterize the DELEGATED `onclick` handler-EXPRESSION boundary, NOT
    // the whole event-handler surface: the non-delegated DIRECT (`$.event`) path admits
    // any non-async inline arrow / function expression (proven by the `events/*` emit
    // corpus), and a bare identifier fails closed on BOTH paths.
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
fn generated_effect_shapes_land_on_boundary() {
    // INVERTED (was `generated_effect_shapes_all_fail_closed`): a WELL-FORMED
    // top-level `$effect(arg)` statement is SUPPORTED for EVERY single-argument
    // shape — oracle-verified against svelte@5.56.3: each accepts as
    // `$.user_effect(<arg passthrough>)` (an identifier / call / member /
    // conditional argument flows through verbatim modulo signal rewrites; the
    // undeclared `f` / `o` are runtime globals official also passes through).
    // Only the ASYNC+AWAIT body stays fail-closed — re-homed to the
    // experimental-async surface (5j), asserted by exact code below.
    let variants: &[(&str, &str, Expected)] = &[
        ("arrow_block", "$effect(() => { c; });", Expected::Supported),
        ("arrow_expr", "$effect(() => c);", Expected::Supported),
        (
            "function_expr",
            "$effect(function () { c; });",
            Expected::Supported,
        ),
        ("identifier", "$effect(f);", Expected::Supported),
        ("call", "$effect(f());", Expected::Supported),
        ("member", "$effect(o.m);", Expected::Supported),
        ("conditional", "$effect(c ? f : o.m);", Expected::Supported),
        // Oracle parity: an async callback with NO `await` accepts
        // (`$.user_effect(async () => { $.get(c); })`) — the await gate fires on
        // `await`, not on the `async` keyword.
        (
            "async_arrow_no_await",
            "$effect(async () => { c; });",
            Expected::Supported,
        ),
        (
            "async_arrow",
            "$effect(async () => { await c; });",
            Expected::FailClosed,
        ),
    ];
    for (label, stmt, expected) in variants {
        let source = format!(
            "<script>let c = $state(0); {stmt}</script>\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        assert_variant(&format!("effect::{label}"), &source, *expected);
        if *expected == Expected::Supported {
            // Every supported shape must actually mint the user-effect helper (a
            // silently-dropped statement would still pass the Main check).
            let js = compile(&source).expect("supported effect shape compiles");
            assert!(
                js.contains("$.user_effect("),
                "effect::{label}: the effect statement lowers to `$.user_effect`:\n{js}"
            );
            assert!(
                !js.contains("$effect"),
                "effect::{label}: no raw `$effect` rune survives:\n{js}"
            );
        }
    }
    // The RE-HOMED async refusal: `$effect(async () => {{ await c; }})` fails
    // closed on the experimental-async surface (5j) with the EXACT code — no
    // longer the advanced-rune position refusal.
    let async_source = "<script>let c = $state(0); $effect(async () => { await c; });</script>\n<button onclick={() => c++}>{c}</button>\n";
    match compile(async_source) {
        Err(ClientCompileError::Unsupported(surface)) => assert_eq!(
            surface.diagnostic_code(),
            "svelte-runtime-unsupported-experimental-async",
            "the awaiting effect re-homes to the experimental-async refusal: {surface:?}"
        ),
        other => panic!("expected the experimental-async refusal, got {other:?}"),
    }
}

#[test]
fn generated_props_pattern_and_default_shapes_land_on_boundary() {
    // The finite grammar of a `$props()` destructure PATTERN + DEFAULT shape. A
    // basic destructure with identifier / string keys is supported WITH or
    // WITHOUT defaults — plain and `$bindable(...)` defaults lower through the
    // shared `$.prop` prop-source path — and a rest (`{ …, ...rest }`) / whole-object
    // (`let p = $props()`) capture is supported through the `$.rest_props` path, while
    // a computed / numeric / nested / duplicate form fails closed.
    let variants: &[(&str, &str, Expected)] = &[
        // ── supported: no-default destructure ────────────────────────────────────
        ("plain", "let { a } = $props();", Expected::Supported),
        ("alias", "let { a: b } = $props();", Expected::Supported),
        (
            "string_key",
            "let { \"data-x\": x } = $props();",
            Expected::Supported,
        ),
        // ── supported: the `$.prop` prop-source defaults ─────────────────────
        (
            "literal_default_num",
            "let { a = 1 } = $props();",
            Expected::Supported,
        ),
        (
            "literal_default_str",
            "let { a = \"x\" } = $props();",
            Expected::Supported,
        ),
        (
            "literal_default_bool",
            "let { a = true } = $props();",
            Expected::Supported,
        ),
        (
            "ref_default",
            "let { a = 1, b = a } = $props();",
            Expected::Supported,
        ),
        (
            "array_default",
            "let { a = [] } = $props();",
            Expected::Supported,
        ),
        (
            "call_default",
            "let { a = f() } = $props();",
            Expected::Supported,
        ),
        (
            "bindable",
            "let { a = $bindable(0) } = $props();",
            Expected::Supported,
        ),
        // ── supported: the `$.rest_props` capture forms ──────────────────────
        (
            "rest",
            "let { a, ...rest } = $props();",
            Expected::Supported,
        ),
        ("whole_object", "let p = $props();", Expected::Supported),
        // ── demoted: out-of-boundary pattern shapes ──────────────────────────
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
    // plain-local / import binding) plus the function-pair + non-resolvable / non-lvalue
    // forms (a computed member, a call). A `bind:value` on an `<input>` to a SIGNAL or
    // PLAIN-local IDENTIFIER, to a PLAIN-local-ROOTED member, or a two-element
    // FUNCTION-PAIR `{get, set}`, plus element `bind:this` to an identifier, is
    // SUPPORTED; a `$props()` / `$bindable` / `$derived` / import root (a divergent
    // official accessor protocol), a non-lvalue call, and an object/array `$state` root
    // (whose declaration fails closed upstream at the `$state()` non-primitive gate)
    // all fail closed.
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
        // ── supported: a PLAIN-local IDENT / MEMBER target (the target-lvalue widening) ─
        (
            "value_plain_local_ident",
            "let v = 'x'; let c = $state(0);",
            "<input bind:value={v} />",
            Expected::Supported,
        ),
        (
            "value_plain_local_member",
            "let o = { x: '' }; let c = $state(0);",
            "<input bind:value={o.x} />",
            Expected::Supported,
        ),
        // ── supported: a two-element FUNCTION-PAIR `{get, set}` (inline arrows) ────
        (
            "value_function_pair",
            "let c = $state('');",
            "<input bind:value={() => c, (v) => c = v} />",
            Expected::Supported,
        ),
        // A THREE-element sequence is NOT a valid `{get, set}` pair — official rejects
        // a non-two-element sequence, so it fails closed (the exactly-two boundary).
        (
            "value_three_element_sequence",
            "let c = $state('');",
            "<input bind:value={() => c, (v) => c = v, c} />",
            Expected::FailClosed,
        ),
        // ── supported: object/array `$state` MEMBER bind (the deep-reactive proxy
        //    declarator is now lowered; a never-reassigned root is a `BareProxy` whose
        //    member setter is a PLAIN assignment `o.x = $$value`) ────────────────────
        (
            "value_object_state_member",
            "let o = $state({ x: '' });",
            "<input bind:value={o.x} />",
            Expected::Supported,
        ),
        (
            "value_object_state_computed_member",
            "let arr = $state(['']); let i = $state(0);",
            "<input bind:value={arr[i]} />",
            Expected::Supported,
        ),
        // ── out-of-boundary: a $props() / $bindable / $derived / import MEMBER root
        //    (a divergent official accessor protocol — NOT a plain assignment) ─────────
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
        // A MEMBER rooted at an import is ACCEPTED (official emits the plain member
        // closures + the context frame; only the BARE import root rejects).
        (
            "value_import_member",
            "import { store } from './s.js'; let c = $state(0);",
            "<input bind:value={store.x} />",
            Expected::Supported,
        ),
        // The BARE import root stays rejected (non-writable root — official
        // `constant_binding`, "Cannot bind to import").
        (
            "value_import_bare_root",
            "import { store } from './s.js'; let c = $state(0);",
            "<input bind:value={store} />",
            Expected::FailClosed,
        ),
        // ── out-of-boundary: non-lvalue / non-resolvable / wrong directive. A bare
        //    call `bind:value={f()}` and a call-rooted member `bind:value={f().x}` are
        //    NOT valid lvalues and NOT the two-element `{get, set}` pair — official
        //    PARSE-REJECTS them ("Can only bind to an Identifier or MemberExpression or
        //    a `{get, set}` pair"), so they stay fail-closed (distinct from the
        //    two-element function-pair, which IS accepted). ──────────────────────────
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
            // 5c: `bind:value` on a `<textarea>` is now SUPPORTED (`$.bind_value`
            // after `$.remove_textarea_child`) — the value bind is NOT input-only.
            "value_textarea",
            "let v = $state('');",
            "<textarea bind:value={v}></textarea>",
            Expected::Supported,
        ),
        (
            // 5c: `bind:checked` on an `<input type="checkbox">` is now SUPPORTED.
            "checked",
            "let on = $state(false);",
            "<input type=\"checkbox\" bind:checked={on} />",
            Expected::Supported,
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
    // The finite grammar of a `$state(init)` INITIALIZER shape. A primitive literal
    // init (string / number / boolean / null / undefined / bigint / `-1` / a
    // no-substitution template) is the `$.state(<literal>)` signal; a proxiable object /
    // array / call / `NaN` / `Infinity` init is the deep-reactive `$.proxy` form. BOTH
    // are SUPPORTED — the declarator emitter lowers each per its resolved `StateLowering`.
    // Only a destructure, an over-arity call, and the narrowed shadowed-`undefined` init
    // fail closed (covered by the fail matrix).
    let variants: &[(&str, &str, Expected)] = &[
        // ── supported: primitive-literal inits ───────────────────────────────────
        ("number", "let s = $state(0);", Expected::Supported),
        ("string", "let s = $state('x');", Expected::Supported),
        ("boolean", "let s = $state(true);", Expected::Supported),
        ("null", "let s = $state(null);", Expected::Supported),
        ("undefined_empty", "let s = $state();", Expected::Supported),
        ("negative", "let s = $state(-1);", Expected::Supported),
        ("template_init", "let s = $state(`x`);", Expected::Supported),
        // ── supported: proxiable (deep-reactive `$.proxy`) inits ─────────────────
        ("object", "let s = $state({});", Expected::Supported),
        ("array", "let s = $state([]);", Expected::Supported),
        (
            "object_props",
            "let s = $state({ a: 1 });",
            Expected::Supported,
        ),
        ("call_init", "let s = $state(make());", Expected::Supported),
    ];
    for (label, decl, expected) in variants {
        // The head declares the supported `$state` signal. A proxiable init is lowered
        // to `$.proxy(...)` (never reassigned → `BareProxy`); its init routes through the
        // shared rewriter. The `call_init` variant's `make()` need not resolve — the
        // rewriter passes an unresolved identifier through, so the declarator still emits.
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
    //
    // The runtime-unsupported DEDICATED-helper binds add 8 rows (`bind_files_wrong_helper`,
    // `bind_playback_rate_wrong_helper`, `bind_volume_wrong_helper`,
    // `bind_muted_wrong_helper`, and the four resize-observer rows
    // `bind_content_rect_wrong_helper` / `bind_content_box_size_wrong_helper` /
    // `bind_border_box_size_wrong_helper` /
    // `bind_device_pixel_content_box_size_wrong_helper`): each has a real IDE contract row
    // whose official helper is a DEDICATED helper (a generic `$.bind_property` would be the
    // wrong helper), and the native runtime does not emit it yet, so the contract records
    // the real official helper + `RuntimeSupport::Unsupported` and the runtime router fails
    // it closed. (5f-b flipped `bind:focused` to `RuntimeSupport::Supported`, so its former
    // `bind_focused_unsupported_fails_closed` row was removed — `<input bind:focused>` now
    // emits `$.bind_focused`.) +8 rows.
    //
    // The runtime-unsupported GENERIC-property binds add 8 MORE rows
    // (`bind_indeterminate_unsupported` on `<input>`;
    // `bind_buffered_unsupported` / `bind_seekable_unsupported` /
    // `bind_seeking_unsupported` / `bind_ended_unsupported` /
    // `bind_ready_state_unsupported` on `<audio>`;
    // `bind_video_width_unsupported` / `bind_video_height_unsupported` on
    // `<video>`): each official helper IS the generic `$.bind_property` form, but the
    // native runtime does not emit it yet, so the contract records the real `Property`
    // official helper + `RuntimeSupport::Unsupported` and the runtime router fails it
    // closed (refusal rides support, not the emittable helper). Each host is the name's
    // pinned `binding_properties.valid_elements` member that is ALSO in the element
    // allowlist, so the row is reachable at the BIND gate (not the element gate).
    // `naturalWidth` / `naturalHeight` are `<img>`-only and `<img>` is NOT allowlisted, so
    // they stay router-level only (the `bind_contract`
    // `unsupported_correct_helper_rows_fail_closed_at_the_runtime_router` test covers
    // them) — NO fail-matrix row, which would fail at the element gate instead of the bind
    // gate. +8 rows.
    // The render reject-parity + `<svelte:self>` placement rows add NINE fail-closed rows
    // (net +9, 127 → 136), STRENGTHENING the matrix: SEVEN `{@render …(…spread)}` rows refuse
    // a SPREAD argument the official compiler hard-errors
    // (`render_tag_invalid_spread_argument`) instead of silently dropping it — the three
    // direct forms (`render_spread_prop_callee` / `render_spread_local_snippet_callee` /
    // `render_spread_dynamic_callee`) PLUS the four parenthesized-whole-call forms
    // (`render_spread_paren_whole_call` / `render_spread_double_paren_whole_call` /
    // `render_spread_paren_optional_call` / `render_spread_paren_local_snippet`), which peel
    // ALL outer author parens before the spread scan so a parenthesized whole call fails
    // closed identically to the bare form rather than peeling to a non-call node and dropping
    // the spread. Two `<svelte:self>` ROOT-placement rows (`svelte_self_root_placement` /
    // `svelte_self_root_bind_this`) refuse a self-reference with NO allowed enclosing context
    // (the official `svelte_self_invalid_placement`) instead of emitting the recursive
    // self-call — both formerly over-accepted (emitted a divergent Main).
    // The 5f-c lifecycle vertical FLIPPED `spread_with_use` positive (official folds a
    // spread alongside `use:`/`transition:` — the `lifecycle/spread_lifecycle` golden pins
    // the `$.attribute_effect` → `$.action` → `$.transition` order) and added EIGHT
    // fail-closed rows: the child-form `{@attach}` (`attach_child_form`, official
    // `expected_tag` — attribute-position-only), the DEFERRED component attachment
    // (`component_attach`, D-38) and `<svelte:element>` lifecycle (`svelte_element_use`,
    // D-39), the official-reject parity rows `component_use`
    // (`component_invalid_directive`), `animate_outside_each`
    // (`animation_invalid_placement`), `animate_unkeyed_each` (`animation_missing_key`),
    // `animate_duplicate` (`animation_duplicate`), and `transition_conflict`
    // (`transition_conflict`), plus the DEFERRED async-lifecycle-expression row
    // (`lifecycle_async_expr`, D-40) — net +8 rows, 136 → 144. The animate
    // only-child refinement (declaration-tag siblings are IGNORED per official —
    // the positive `lifecycle/animate_keyed_const` golden) added the
    // `animate_sibling_element` reject-parity row locking that a sibling ELEMENT
    // still refuses (`animation_invalid_placement`) — +1 row, 144 → 145. The D-39
    // fail-closed surface is locked by THREE more rows (official ACCEPTS all three;
    // Verter defers them fail-closed, NOT reject parity): `svelte_window_use` +
    // `svelte_body_attach` (global-host lifecycle/attach at the
    // `classify_special_host` gate) and `svelte_element_transition` (the transition
    // sibling of `svelte_element_use`) — +3 rows, 145 → 148.
    // The regular-element / `<svelte:fragment>` named-slot completion added SIX
    // `slot`-attribute rows (the static `slot="x"` is now ACCEPTED only at valid
    // component-child slot placement, so the whole refusal boundary is enumerated):
    // `slot_attr_outside_component_child` + `slot_attr_nested_in_component_child`
    // (official `slot_attribute_invalid_placement`), `slot_attr_dynamic` +
    // `slot_attr_mixed` (official `slot_attribute_invalid`), `slot_attr_duplicate`
    // (official `slot_attribute_duplicate`), and `slot_attr_default_conflict`
    // (official `slot_default_duplicate`) — +6 rows, 148 → 154.
    // The slot-placement gate keys on the SOURCE-LEVEL slot-attribute OWNER set (the
    // direct slot-declaring component children recorded at lowering), NOT on lowered
    // slot-region-root membership — a transparent `<svelte:fragment slot>`'s hoisted
    // children never inherit slot-placement validity. TWO rows lock that boundary
    // (both formerly fail-OPEN — the inner `slot=` was wrongly baked into the outer
    // slot's callback): `slot_attr_nested_in_slotted_fragment` (a named `slot="bar"`
    // inside a slotted fragment) and `slot_attr_default_nested_in_slotted_fragment`
    // (a `slot="default"` inside a slotted fragment) — +2 rows, 154 → 156.
    // The unified slot-validation choke-point (`validate_slot_placement` at
    // `classify_node` entry — the sole `slot=` authority, run before every per-kind
    // accept/fold/prop projection) implements the official THREE-CLASS `slot=`
    // disposition, and this matrix enumerates exactly its REJECT class. The former
    // component/special fail-closed rows `slot_attr_on_component_child` /
    // `slot_attr_on_nested_component` / `slot_attr_on_svelte_component` /
    // `slot_attr_on_svelte_self` FLIPPED to accepted-positive (official ACCEPTS them:
    // a direct component-family filler routes into `$$slots` AND keeps the `slot`
    // prop; a non-direct component-family `slot` is an ordinary plain prop — the
    // emission tests + `components/slot_*` oracle goldens pin the shapes) — −4 rows.
    // The reject class holds: the dynamic/mixed DIRECT-child rows
    // (`slot_attr_dynamic_on_component` / `slot_attr_mixed_on_component` /
    // `slot_attr_dynamic_on_svelte_component_child` /
    // `slot_attr_dynamic_on_svelte_self_child` /
    // `slot_attr_dynamic_on_svelte_element_child` — official
    // `slot_attribute_invalid` fires at `owner === parent`), the component-family
    // explicit-default self-conflict (`slot_attr_default_on_component_child` —
    // official `slot_default_duplicate` exempts only element/fragment siblings), the
    // top-level/non-direct `<svelte:element>` forms (`slot_attr_on_svelte_element` /
    // `slot_attr_dynamic_on_svelte_element` — official
    // `slot_attribute_invalid_placement`), and the never-a-slot-host specials
    // (`slot_attr_on_svelte_boundary` + its direct-child variant
    // `slot_attr_on_svelte_boundary_child`, `slot_attr_on_svelte_head`,
    // `slot_attr_on_standalone_fragment`) — +6 new reject rows. `<svelte:options
    // slot>` is refused at PARSE (`svelte_options_unknown_attribute` reject parity),
    // the direct-child `<svelte:head slot>` at the OFFICIAL-reject rail
    // (`svelte_meta_invalid_placement`), and the global hosts
    // (`<svelte:window|document|body>`) already refuse `slot` with the same code, so
    // those kinds have NO row here (non-discriminating) and are covered by the
    // per-kind choke-point unit proof — net +2 rows, 166 → 168.
    // The D-43 custom-element-host / native-slotting over-refusal class (official
    // ACCEPTS every shape; Verter defers fail-closed, NOT reject parity) is scoped
    // at the ROOT-limitation level on two rails and is protected by the generic
    // custom-element host-gate rows plus the slot-disposition unit proof; the
    // listed rows are representative smoke probes, intentionally non-exhaustive:
    // rail 1, the custom-element HOST gate (`host-custom-element`) —
    // `slot_attr_under_custom_element_owner`,
    // `slot_attr_under_customized_builtin_owner`,
    // `custom_element_as_direct_slot_filler`,
    // `component_filler_under_custom_element_owner`; rail 2, the unified slot
    // choke-point (`dynamic-attribute`) — `slot_attr_under_svelte_element_owner`,
    // `svelte_element_slot_under_svelte_element_owner` — +6 rows, 168 → 174.
    // The 5g-b state-family vertical REMOVED FIVE rows (now accepted-positive with
    // emission goldens): `state_raw` (a `$state.raw` opt-out signal), `state_object_init`
    // + `state_array_init` (the deep-reactive `$.proxy` object/array declarator),
    // `state_nan_init` (a `NaN` proxiable init), and `state_snapshot` (now a supported
    // expression rune rewritten to `$.snapshot`) — net −5 rows, 174 → 169. The 5g-b
    // fix-cycle then ADDED TWO rows (`state_spread_argument` + `state_raw_spread_argument`
    // — the `rune_invalid_spread` fail-close) — net +2 rows, 169 → 171.
    // ── 5g-c effect family ──────────────────────────────────────────────
    // The effect-family fail-closed remainder holds 18 rows across five
    // categories (the accepted call-position topology lives in the
    // `runes/effect_*` + `matrix/effect_arrow` goldens, the `effect_*` client
    // tests, and `generated_effect_shapes_land_on_boundary`):
    // - the await-gate re-home: `effect_async` (an AWAITING callback is the
    //   experimental-async surface — the await gate owns it, not the position
    //   scan).
    // - the non-call / uncalled / malformed forms plus the 5j member:
    //   `effect_bare_ref`, `effect_uncalled_pre`, `effect_tracking_with_arg`,
    //   `effect_pending`.
    // - the VALUE-POSITION user-effect calls (official
    //   `effect_invalid_placement`: `$effect` / `$effect.pre` are
    //   statement-only; `.root` / `.tracking` are expression-valued):
    //   `effect_value_position_const_decl`,
    //   `effect_pre_value_position_const_decl`,
    //   `effect_value_position_handler_concise_body`,
    //   `effect_value_position_root_body_decl_init`,
    //   `effect_value_position_root_body_return`,
    //   `effect_pre_value_position_effect_body_decl_init`.
    // - the OPTIONAL invocations of the statement-only members (official
    //   `effect_invalid_placement` — the `?.` chain sits between the call and
    //   its statement parent; the expression-valued members admit optional
    //   invocations normalized, on the accept side): `effect_optional_call`,
    //   `effect_pre_optional_call`, `effect_optional_member_pre`.
    // - the plain-script TS TYPE-ARGUMENT forms (official plain-script parsing
    //   reads `$effect<T>(fn)` as a comparison chain and rejects
    //   `rune_missing_parentheses`): `effect_type_args`,
    //   `effect_pre_type_args`, `effect_root_type_args_stmt`,
    //   `effect_root_type_args_init` (the tracking spelling is a plain-JS
    //   parse error pinned at the official-reject rail instead).
    // The `$props()` default / `$bindable` prop-source surface removed FOUR rows
    // (now accepted-positive with oracle-backed goldens): `props_literal_default`
    // (`{ a = 1 }` → the eager flag-3 `$.prop`), `props_ref_default`
    // (`{ a = 1, b = a }` → the lazy bare-getter carrier), `props_array_default`
    // (`{ a = [] }` → the lazy thunk), and `props_bindable`
    // (`{ value = $bindable(0) }` → the flag-11 prop source with the context
    // frame) — net −4 rows.
    // With the effect-family remainder in place the matrix pins 178 rows. The 5g-e
    // `$props()` rest + whole-object capture vertical removed THREE rows now
    // accepted-positive through the `$.rest_props` path (covered by the client-tests
    // positives + the boundary matrix): `props_rest` (`{ a, ...rest }`), `props_whole`
    // (`let p = $props()`), and `props_rest_spread` (`<div {...rest}>` element spread)
    // — net −3 rows.
    // ── `$host` malformed / out-of-context forms ─────────────────────────
    // The `$host` fail-closed boundary adds NINE rows. TWO close a former
    // FAIL-OPEN (both previously emitted a raw `$.event('focus', button, () =>
    // $host…)` Main in runes mode): `host_bare_reference_runes_isolated` (the
    // uncalled bare reference — official `rune_missing_parentheses`; all
    // unshadowed rune roots now fail the position scan outside a supported
    // position) and `host_paren_callee_call_runes_isolated` (the
    // parenthesized-callee `($host)()` — official `host_invalid_placement`; the
    // strict supported call spelling does not peel). SEVEN pin the surviving
    // refusals around the supported zero-arg in-customElement `$host()` call:
    // `host_call_outside_custom_element` (well-formed call, no customElement),
    // `host_call_arity_one` / `host_call_arity_two` (official
    // `rune_invalid_arguments`), `host_call_spread_argument` (official
    // `rune_invalid_spread`), `host_optional_call` (`$host?.()`),
    // `host_member_access` (`$host.x`) and `host_optional_member_access`
    // (`$host?.x`, both the `$rune.<member>` arm) — +9 rows, 178 → 187.
    // The customElement ACCEPT completes the `$host` sibling enumeration with
    // FIFTEEN more rows: FOUR outside-context position/shadow rows —
    // `host_bare_statement_position_in_handler` (`() => { $host; }`, closing the
    // statement-position twin of the bare-reference fail-open),
    // `host_bare_statement_in_instance_script` (`$host;`),
    // `host_bare_decl_init_in_instance_script` (`const h = $host`), and
    // `host_param_shadow_function_stays_generic_refusal` (the shadow invariant:
    // generic item refusal, never rune-coded) — plus ELEVEN inside-customElement
    // siblings of the admitted zero-arg call: the bare reference, the
    // parenthesized callee, arity one / two, the spread argument, the optional
    // call, the member / optional-member access, the const-decl init
    // (`const h = $host()` — the instance-top form whose OFFICIAL output is
    // runtime-broken), the bare `$host();` statement, and the interpolation
    // position (`{$host()}`) — +15 rows, 187 → 202.
    // The `$host` DEGENERATE-UNBOUND props-parameter gate adds SEVEN rows —
    // the well-formed ADMITTED `$host()` whose component has NO
    // props-parameter trigger (no real props binder, no `needs_context`
    // reason, no member access on the `$host()` call result itself), which
    // official emits as the runtime-broken `function App($$anchor)` +
    // unbound `$$props.$$host` residue and Verter refuses:
    // `host_bare_result_degenerate_inside_custom_element`,
    // `host_bare_statement_degenerate_inside_custom_element`,
    // `host_assigned_unused_degenerate_inside_custom_element`,
    // `host_alias_member_degenerate_inside_custom_element` (an alias is NOT
    // the call result itself), `host_arg_position_degenerate_inside_custom_element`,
    // `host_ce_props_declaration_only_degenerate` (a CE `props: {...}`
    // declaration is not a binder), and
    // `host_props_id_only_degenerate_inside_custom_element` (`$props.id()` is
    // not a binder) — +7 rows, 202 → 209.
    // The TEMPLATE-`$host` runes-mode inference adds TWO runeless rows (a
    // template `$host` occurrence flips the component to runes mode by itself,
    // so these scriptless forms reach the `$host` gates, not the legacy-mode
    // gate): `host_call_non_custom_element_template_runeless` (official
    // `host_invalid_placement`) and
    // `host_template_only_bare_degenerate_inside_custom_element` (the
    // scriptless degenerate-unbound residue) — +2 rows, 209 → 211.
    // The `{@render}` dynamic-callee `$host` accept (`{@render $host().snip()}`
    // — the peeled callee opens the context frame and binds `$$props`) adds
    // SIX malformed-sibling rows: the inner-rune arity one / two / spread /
    // optional-call forms in render-callee position
    // (`render_callee_host_arity_one_inside_custom_element`,
    // `render_callee_host_arity_two_inside_custom_element`,
    // `render_callee_host_spread_inside_custom_element`,
    // `render_callee_host_optional_call_inside_custom_element`), the UNCALLED
    // render reference
    // (`render_uncalled_host_member_reference_inside_custom_element` —
    // official `render_tag_invalid_expression`), and the bare-spread snippet
    // argument on the member-callee accept
    // (`render_host_member_callee_spread_snippet_arg_inside_custom_element` —
    // official `render_tag_invalid_spread_argument`) — +6 rows, 211 → 217.
    // The static-import prelude removed TWO rows (now accepted-positive with
    // `imports/*` oracle goldens): `instance_import` (every static import form is
    // admitted) and `bind_value_import_member` (a member of an import is an accepted
    // bind lvalue with the context frame) — 217 → 215.
    // The `$store` auto-subscription surface removed ONE row (now
    // accepted-positive with `stores/*` oracle goldens): `event_local_function_ident`
    // (a bare-identifier handler naming a top-level function declaration is passed
    // by reference — `$.delegated('click', button, inc)` — with the function body
    // rewritten through the shared rewriter) — 215 → 214.
    // The SCOPE-AWARE store-subscription base resolution adds FIVE rows — the
    // official `store_invalid_scoped_subscription` reject class (a `$NAME`
    // whose base resolves to a NON-top-level binding), one per shadowing
    // surface: the `{#each as x}` alias, the `{#snippet}` parameter, the
    // `{#await then x}` binding, a script function parameter, and a
    // template-expression arrow parameter — 214 → 219.
    // The rune-USAGE × store-SUBSCRIPTION name-collision refusal adds ONE row:
    // a rune-root-named store accessor (`$state` over `const state = writable(0)`)
    // subscribed WHILE the same rune root has live admitted usage
    // (`let n = $state(1)`) is a divergent-mode case (official compiles LEGACY to
    // the store accessor `$state()(1)`) that fails closed with the precise
    // instance-script-item diagnostic — 219 → 220.
    // The legacy reactivity substrate removed ONE row: `instance_reactive_label`
    // (a `$state` + `$:` component is RUNES mode, where `$:` is the official
    // EXACT-CODE `legacy_reactive_statement_invalid` compile error — its parity
    // rows moved to the reject corpus) — 220 → 219. The `instance_export` row
    // stays but under the own-identity component-export-binding code, and
    // `instance_plain_let` keeps pinning that a RUNES plain `let` never promotes
    // (the legacy `$.mutable_source` promotion is legacy-mode-only).
    assert_eq!(
        FAIL_MATRIX.len(),
        218,
        "the fail matrix pins 218 fail-closed rows — one documented \
         unsupported-feature sub-shape per row, EXCEPT the D-43 custom-element-host / \
         native-slotting rows, which are REPRESENTATIVE smoke probes for that \
         root-scoped over-refusal class (protected by the generic host-gate rows plus \
         the slot-disposition unit proof), NOT an exhaustive enumeration of it. \
         The 5f-a component/snippet/slot vertical removed FIVE rows \
         (now accepted-positive with topology/emit goldens): `component` (a `<Foo />` direct \
         call), `bind_this_component` (a component `bind:this` → `$.bind_this`), \
         `snippet_in_if_block` (a block-body `{{#snippet}}` const), `render_in_each_block` (a \
         block-body `{{@render}}` → `$.snippet`), and `span_element` (only `<span>` \
         joined the element allowlist as the plain slot/snippet structural host; `<ul>` / \
         `<li>` remain rejected) — net −5 \
         rows from the prior 133. The declaration-tag PLACEMENT gate adds four rows: a \
         `{{@const}}` / `{{const}}` / `{{let}}` NESTED inside an element \
         (`const_tag_nested_in_element` / `decl_const_nested_in_element` / \
         `decl_let_nested_in_element`) fails closed (`svelte-runtime-unsupported-block`) \
         rather than the roots-only hoist that silently DROPPED a non-region-root \
         declaration tag, plus a `{{@const}}` at the COMPONENT ROOT \
         (`const_tag_at_component_root`) which the official also rejects (the component root \
         is not a `{{@const}}` valid parent) — +4 rows. A MEMBER read inside reactive text \
         within a block body (`block_body_member_interpolation`, `{{item.x}}`) fails closed \
         as the interpolation-breadth surface — +1 row. The regular-element event surface is \
         now SUPPORTED, so \
         its three former rows moved to the positive `events/*` corpus + smoke, replaced \
         by the two still-fail-closed rows `event_invalid_modifier_combo` and \
         `event_unknown_modifier` — net −1 row. The DOM bind target-lvalue \
         widening moved three rows \
         from fail-closed to accepted-positive (with topology goldens): \
         `bind_value_plain_local` (a `bind:value={{v}}` to a PLAIN local — official \
         emits `$.bind_value(input, () => v, ($$value) => v = $$value)`), \
         `bind_value_plain_local_member` (a `bind:value={{o.x}}` rooted at a plain local \
         — official emits the plain member lvalue closures), and `bind_value_sequence_pair` \
         (a `bind:value={{get, set}}` two-element function-pair — official passes the \
         supplied get/set DIRECTLY). The `bind_value_call` row is GONE — a bare \
         `bind:value={{f()}}` is not a valid lvalue and not a `{{get, set}}` pair, so it \
         now fails closed through the OFFICIAL-reject gate (`bind_invalid_expression`, the \
         exact code), NOT this unsupported-feature matrix; object/array `$state`-rooted \
         members stay closed upstream at the `$state()` non-primitive-init gate. −4 rows. \
         The remaining \
         rows: the element-spread + `{{@html}}` surface removed the \
         `spread` / `html_tag` refusal rows now SUPPORTED, replacing them with the \
         `props_rest_spread` row PLUS the two spread-incompatible-directive rows \
         `spread_with_event` / `spread_with_bind` that lock the fail-closed identity of \
         a spread co-located with an event/bind (5f-c flipped `spread_with_use` \
         positive — spread + lifecycle co-exist per official); the value \
         emitter is source-preserving, so the five `value_paren_*` rows are GONE — author \
         parens are kept verbatim, never refused; the `bind_checked` row is GONE — 5c \
         now emits `$.remove_input_defaults` + `$.bind_checked` for `bind:checked`; the \
         8 runtime-unsupported DEDICATED-helper bind rows (files / playbackRate / volume / \
         muted + the four resize-observer binds) fail closed at the runtime router rather \
         than emit the wrong generic `$.bind_property` helper (5f-b flipped `bind:focused` \
         to supported, so its former fail-closed row was removed) \
         — +8 rows; PLUS the 8 runtime-unsupported GENERIC-property bind rows \
         (indeterminate on input; buffered / seekable / seeking / ended / readyState on \
         audio; videoWidth / videoHeight on video) that fail closed because the native \
         runtime does not emit them yet — naturalWidth / naturalHeight stay router-only \
         since `<img>` is not allowlisted — +8 rows; the effect-family fail-closed \
         remainder holds eighteen rows: the await-gate re-home `effect_async` (an \
         awaiting callback is the experimental-async surface), the non-call / \
         uncalled / malformed forms plus the 5j member `effect_bare_ref` / \
         `effect_uncalled_pre` / `effect_tracking_with_arg` / `effect_pending`, \
         the six value-position user-effect rows (official \
         `effect_invalid_placement`: `$effect` / `$effect.pre` are \
         statement-only, `.root` / `.tracking` expression-valued) \
         `effect_value_position_const_decl` / \
         `effect_pre_value_position_const_decl` / \
         `effect_value_position_handler_concise_body` / \
         `effect_value_position_root_body_decl_init` / \
         `effect_value_position_root_body_return` / \
         `effect_pre_value_position_effect_body_decl_init`, the three \
         optional-invocation rows `effect_optional_call` / \
         `effect_pre_optional_call` / `effect_optional_member_pre` (official \
         `effect_invalid_placement` — the expression-valued members admit optional \
         invocations normalized, in the accept suite), and the four plain-script \
         TS-type-argument rows `effect_type_args` / `effect_pre_type_args` / \
         `effect_root_type_args_stmt` / `effect_root_type_args_init` (official \
         `rune_missing_parentheses` — the tracking spelling is a plain-JS parse \
         error pinned at the official-reject rail))"
    );
}
