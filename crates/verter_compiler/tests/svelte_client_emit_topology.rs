//! Emitted-JS normalized-topology gate for the native Svelte client backend.
//!
//! For each supported fixture, this compiles the component with Verter
//! (`compile_client`), normalizes the EMITTED JS into the SAME topology shape the
//! `scripts/svelte-golden-lib.mjs` extractors derive (the helper sequence/set/
//! counts, the import topology, the export-fn shape, the `from_html` template
//! skeletons + fragment flag, and the delegated event set), and compares it to the
//! COMMITTED official golden JSON (regenerated from the pinned `svelte@5.56.3` by
//! `scripts/gen-svelte-goldens.mjs`). It is BEHAVIOR/topology parity, NOT byte
//! identity — whitespace and walk-strategy details are not pinned. Local IDENTIFIER
//! SPELLINGS, however, ARE structural for this comparator (`expr_sig` signs `Id(name)`
//! and `binding_sig` signs the binding name), so a consistent alpha-rename FAILS — the
//! oracle does not implement scope-aware alpha-equivalence, so it must not silently
//! pass a rename it cannot prove is a behavior-preserving private binding.
//!
//! Hermetic: the only inputs are the vendored fixtures + the committed goldens, so
//! the gate runs with no live `svelte` present. The golden is the oracle; a
//! Verter emitted-topology drift fails here.

use std::path::PathBuf;

use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::{
    compile_client, live_fallback_ledger, ClientCompileError, SvelteRuntimeOptions,
    UnsupportedSvelteRuntimeSurface,
};

/// The SUPPORTED fixtures (the §1.2 headline conformance target + the rune /
/// template surface) this gate covers. Every slug here resolves to a committed
/// `<slug>.client.json` golden, and Verter EMITS a module for it (it does not fail
/// closed). The latter group are the F1–F11 robustness fixtures whose FULL-MODULE
/// comparison catches argument/offset/identifier drift the helper-name sequence
/// misses.
const SUPPORTED_FIXTURES: &[&str] = &[
    // The §1.2 headline conformance target.
    "runes/hello_input",
    // A RUNES-mode `createEventDispatcher` component: the preserved `svelte`
    // import + verbatim dispatcher declaration, the runes frame
    // (`$.push($$props, true)` … `$.pop()`), and NO legacy `$.init()`.
    "runes/dispatcher",
    // A template-literal RHS in a `$state`-assign onclick body
    // (`onclick={() => { msg = `v${n}`; }}`).
    "runes/template_literal_handler",
    // An aliased no-default `$props()` destructure read in a bare interpolation
    // (`let { foo: bar } = $props(); {bar}`).
    "runes/props_alias",
    // A primitive `$state` reassigned to an object literal in an onclick body
    // (`onclick={() => o = { a: 1 }}`) — the `should_proxy(rhs)` gating.
    "runes/proxy_gating",
    // A pure single interpolation as an element child (`<p>{count}</p>`) — the
    // `is_text` clone flag (`$.child(p, true)`).
    "runes/is_text_flag",
    // `bind:value` + `bind:this` on sibling elements with a sibling reactive text.
    // The op-order oracle: `$.bind_this` is a RENDER-side binding emitted INLINE
    // during the walk (BEFORE the grouped sibling `$.template_effect`), while
    // `$.bind_value` is emitted post-walk (AFTER the text effect).
    "runes/bind_value_and_this",
    // A STATIC no-dynamic multi-root fragment (`<p>a</p><p>b</p>`) — the official
    // text-first `$.next()` cursor advance between the clone frame and `$.append`
    // (`var fragment = root(); $.next(); $.append(...)`). Catches a regression that
    // drops the trailing-static-run cursor advance (a hydration end-node divergence).
    "runes/static_fragment",
    // A STATIC single-element root (`<p>a</p>`) — the `is_single_element` clone-root
    // path: official clones the element directly (`var p = root(); $.append(...)`)
    // with NO `$.next()`. The negative golden for the fragment `$.next()` advance —
    // a single-element root must NOT emit it.
    "runes/static_single_root",
    // ── 5g-a production `$inspect` elision ──
    // A top-level `$inspect(c);` statement — production-ELIDED with NO frame
    // (`App($$anchor)`, no push/pop, no inspect helper/import; official leaves a
    // cosmetic `;;` residue the statement-list comparator filters).
    "runes/inspect_standalone",
    // `$inspect(c).with(console.log);` — elided, BUT the `.with` chain FORCES the
    // official production frame: `App($$anchor, $$props)` + `$.push($$props,
    // true)` first + `$.pop()` last (the golden pins the params + push/pop
    // helper topology).
    "runes/inspect_with",
    // `$inspect.trace();` as the first statement of a delegated onclick BLOCK
    // arrow — the trace call is dropped in place, the rest of the body lowers
    // (`$.update(c)`), and no frame is added.
    "runes/inspect_trace_handler",
    // ── 5g-b state family ($state.raw + proxied object-$state + $state.snapshot) ──
    // A `$state.raw({...})` reassigned → `$.state({...})` (NO `$.proxy`) + `$.set(o,
    // rhs)` with NO trailing `, true` (the raw-aware reassignment flag).
    "runes/state_raw_object",
    // A `$state.raw(0)` reassigned → byte-identical to `$state(0)` (`$.state(0)`).
    "runes/state_raw_primitive",
    // A proxied `$state({...})` reassigned → `$.state($.proxy({...}))` + `$.set(o,
    // rhs, true)` (the proxy-RHS flag).
    "runes/state_proxy_reassign",
    // A raw + a proxied object `$state` in ONE component — per-binding correct: the
    // raw one is `$.state({...})` (no flag on reassign), the proxied one
    // `$.state($.proxy({...}))` (flag on reassign).
    "runes/state_mixed_raw_proxy",
    // `$state.snapshot(o)` in a `$state`-write handler → `$.snapshot(o)`.
    "runes/state_snapshot",
    // A `$state(NaN)` reassigned → `$.state($.proxy(NaN))` (a bare global-identifier
    // init IS proxiable) — the discriminating positive for the NaN/Infinity proxiable
    // init class (F3): the proxy wrap must be present, the reassign `$.set(x, 1)` with
    // NO `, true` (a primitive RHS).
    "runes/state_nan",
    // A computed-member `bind:value={arr[i]}` where the INDEX `i` is a reactive signal
    // (reassigned) and `arr` is a never-reassigned proxy: pins the get/set thunk shape
    // `() => arr[$.get(i)]` / `($$value) => arr[$.get(i)] = $$value` (F9 — the
    // computed-index signal read `$.get(i)` is rewritten, the proxy root reads plain).
    "runes/bind_value_computed_member",
    // RUNES control for the legacy value wrap: a call-bearing attr dep stays
    // the RAW rewritten expression (`[() => $$props.obj.m()]`) — NO
    // `deep_read_state`, NO `untrack` (the wrap is legacy-mode-only).
    "runes/attr_call_control",
    // ── 5g-c effect family ($effect / $effect.pre / $effect.root / $effect.tracking) ──
    // A top-level `$effect.pre(fn);` statement → `$.user_pre_effect` + the runes
    // frame (`$.push($$props, true)` / `$.pop()` + the `$$props` param).
    "runes/effect_pre_toplevel",
    // `const stop = $effect.root(() => { $effect(...); return () => {}; });` — the
    // assignable `$.effect_root` expression, the NESTED `$.user_effect` recursion,
    // the verbatim cleanup return, and the frame (from the NESTED effect).
    "runes/effect_root_nested_effect",
    // The nested-PRE variant: `$.user_pre_effect` inside the `$.effect_root` body.
    "runes/effect_root_nested_pre",
    // A root callback with NO nested user effect → NO frame (sig `($$anchor)`, no
    // `$.push` / `$.pop`) — the negative golden for the frame policy.
    "runes/effect_root_only",
    // An UNASSIGNED bare `$effect.root(...);` statement (the unassigned
    // side-effect form) → `$.effect_root(...)` as a bare statement, NO frame —
    // the statement-carrier twin of the assigned declarator form.
    "runes/effect_root_bare_statement",
    // An UNASSIGNED bare `$effect.tracking();` statement → `$.effect_tracking();`,
    // NO frame.
    "runes/effect_tracking_bare_statement",
    // `const t = $effect.tracking();` + a template `{t}` read → the plain-const
    // read inside `$.template_effect(() => $.set_text(text, t))`, NO frame.
    "runes/effect_tracking_const",
    // `const t = $effect.tracking();` + an ATTRIBUTE read (`disabled={t}`) → the
    // property write joins the template effect
    // (`$.template_effect(() => input.disabled = t)`, plain const read) — the
    // attribute-path twin of the text-read disposition (a call-init const cannot
    // be static-folded, so its read is has_state).
    "runes/effect_tracking_attr_const",
    // An INLINE `$effect.tracking()` in an ATTRIBUTE value → the memoized
    // deps-array form (`$.template_effect(($0) => input.disabled = $0,
    // [() => $.effect_tracking()])` — official `is_pure` special-cases the
    // tracking rune as impure ⇒ has_call, re-evaluated INSIDE the tracking
    // context).
    "runes/effect_tracking_attr_inline",
    // A user effect INSIDE a delegated onclick block arrow:
    // `$.delegated('click', button, () => { $.user_effect(...); $.update(c); })`
    // + the frame.
    "runes/effect_in_handler",
    // A bare `$effect.root(...)` STATEMENT inside a delegated onclick block
    // arrow: `$.delegated('click', button, () => { $.effect_root(...);
    // $.update(c2); })` — NO frame (root alone never forces it).
    "runes/effect_root_in_handler",
    // A bare `$effect.tracking();` STATEMENT inside a delegated onclick block
    // arrow: `$.delegated('click', button, () => { $.effect_tracking();
    // $.update(c2); })` — NO frame.
    "runes/effect_tracking_in_handler",
    // ── 5g-e `$props()` rest + whole-object capture (`$.rest_props`) ──
    // `let { a, b, ...rest } = $props()` — the module `rest_excludes` Set (prefix +
    // source keys) + the `let rest = $.rest_props($$props, rest_excludes)` capture;
    // bare named reads de-localize to `$$props.KEY`, NO frame.
    "runes/props_rest_basic",
    // A lone `let { ...rest } = $props()` — the prefix-only Set + `$.rest_props`, the
    // `<div {...rest}>` element spread folds to `$.attribute_effect`, NO frame.
    "runes/props_rest_lone",
    // `let { a = 1, ...rest } = $props()` — composes the 5g-d `$.prop` default with
    // the rest capture into ONE comma-joined `let`, rest LAST.
    "runes/props_rest_default",
    // Whole-object `let all = $props()` — the prefix-only Set + `let all =
    // $.rest_props(...)`; the `{...all}` spread + bare `{all}` interpolation both stay
    // the real local, NO frame.
    "runes/props_whole_object",
    // A NON-excluded rest MEMBER read in a state-write handler (`() => sink += rest.x`)
    // — de-localizes to `$$props.x` AND opens the component frame ($.push/$.pop).
    "runes/props_rest_member_handler",
    // A NON-excluded rest MEMBER as the WHOLE RHS of a COMPOUND `+=` whose target is
    // a $state MEMBER (`() => objS.p += rest.y`) — svelte does NOT pre-rewrite a
    // member target, so the RHS stays the verbatim `rest.y` (the coarse
    // Assignment-child guard). The read/write span sets must keep it local, NOT
    // over-de-localize it to `$$props.y` (the signal-id target IS de-localized —
    // see `props_rest_member_handler` above).
    "runes/props_rest_member_compound",
    // 5g-e REOPEN: OPTIONAL rest member reads preserve the `?.` — `rest?.x` →
    // `$$props?.x`, `rest?.x.y` → `$$props?.x.y` (attr context, batched into one
    // template_effect). De-localization replaces ONLY the object identifier, so the
    // optional axis + downstream chain stay verbatim. Structurally RED at ff1ca89a1
    // (whole-member replacement dropped the `?.` → `$$props.x`).
    "runes/props_rest_member_optional",
    // 5g-e REOPEN: the whole-object equivalent — `all?.x` → `$$props?.x`.
    "runes/props_whole_object_optional",
];

/// The SUPPORTED MATRIX — the exhaustive enumeration of supported client
/// sub-shapes, the positive half of the convergence gate. Each row is a minimal
/// component exercising ONE supported sub-shape; the gate (shared with
/// [`SUPPORTED_FIXTURES`] above) asserts each compiles, OXC-parses, and its
/// normalized FULL MODULE equals the official golden. A row fails if the topology
/// drifts — so the matrix discriminates every supported sub-shape, not just the
/// headline §1.2 example. (Adding a row is trivial: drop a `matrix/<name>.svelte`
/// fixture, regenerate the golden, append the slug.)
const SUPPORTED_MATRIX: &[&str] = &[
    // $state primitive-literal declarator + the §1.2-class onclick write forms
    // (assign / compound / postfix-update / prefix-update to the signal).
    "matrix/state_signal_assign",
    "matrix/state_signal_compound",
    "matrix/state_update_postfix",
    "matrix/state_update_prefix",
    // $props() no-default read-only / string-key alias (bare-interpolation reads).
    "matrix/props_readonly",
    "matrix/props_alias_string_key",
    // bind:value on an <input> to a reactive $state identifier.
    "matrix/bind_value_signal_ident",
    "matrix/bind_value_plain_ident",
    // ── 5c DOM bind TARGET-LVALUE breadth (the target-lvalue widening) ──
    // bind:value to a PLAIN local ident (`let draft = "x"`) — official emits the plain
    // read/write closures `$.bind_value(input, () => draft, ($$value) => draft =
    // $$value)`, and the plain local survives script lowering VERBATIM (not `$.state`).
    "matrix/bind_value_plain_local",
    // bind:value to a PLAIN-local MEMBER (`let form = { name: "" }`) — the plain member
    // lvalue closures (`() => form.name` / `form.name = $$value`).
    "matrix/bind_value_plain_local_member",
    // bind:value to a two-element FUNCTION-PAIR `{get, set}` — the user-supplied get/set
    // (signal-rewritten inline arrows) passed DIRECTLY: `$.bind_value(input, () =>
    // $.get(value), (next) => $.set(value, next, true))`.
    "matrix/bind_value_function_pair",
    // element bind:this to a two-element FUNCTION-PAIR `{get, set}` — the user-supplied
    // get/set passed DIRECTLY (setter slot first, getter slot second, no synthesized
    // thunk): `$.bind_this(div, (v) => $.set(el, v, true), () => $.get(el))`.
    "matrix/bind_this_function_pair",
    // ── 5c DOM-hosted bind family (the bindings-breadth additions) ──
    // textarea bind:value — `$.remove_textarea_child` + `$.bind_value` (oracle
    // CASE textarea_value).
    "matrix/bind_textarea_value",
    // select bind:value (single) — `$.bind_select_value` (oracle CASE select_value).
    "matrix/bind_select_value",
    // bind:checked — `$.remove_input_defaults` + `$.bind_checked` (oracle CASE checked).
    "matrix/bind_checked",
    // media: dedicated `$.bind_current_time` (single, audio host) + the non-uniform
    // multi-row video case (`currentTime`/`paused` dedicated, `duration` read-only via
    // `$.bind_property('duration','durationchange',…)`, `played` setter-only via
    // `$.bind_played`) — oracle CASEs media_currenttime + media_multi.
    "matrix/bind_media_currenttime",
    "matrix/bind_media_multi",
    // dimensions: `$.bind_element_size(el, 'name', set)` setter-only, multi (oracle
    // CASE dimension_multi).
    "matrix/bind_dimension_multi",
    // contenteditable: `$.bind_content_editable('name', el, get, set)` (oracle CASEs
    // contenteditable_innerhtml / _textcontent / _innertext).
    "matrix/bind_contenteditable_innerhtml",
    "matrix/bind_contenteditable_textcontent",
    "matrix/bind_contenteditable_innertext",
    // generic DOM property: `$.bind_property('open','toggle',el,set,get)` read-write
    // (oracle CASE property_open).
    "matrix/bind_property_open",
    // radio bind:group — the component-fn-scoped `const binding_group = []`, per-input
    // `$.remove_input_defaults` + `input.value = input.__value = '<value>'`, and the
    // per-input `$.bind_group(binding_group, [], input, get, set)` (oracle CASE group).
    "matrix/bind_group_radio",
    // radio bind:group with a DYNAMIC `value={expr}` — the `var input_value;` change-tracker,
    // the guarded `$.template_effect` writing `input.value = (input.__value = expr) ?? ''`
    // before `$.bind_group`, and the group getter's dynamic-value dependency read (F4).
    "matrix/bind_group_radio_dynamic",
    // radio bind:group with a MIXED `value="pre-{expr}"` — the template-literal value
    // (`input.value = input.__value = `pre-${expr ?? ''}``) under the same tracked update (F4).
    "matrix/bind_group_radio_mixed",
    // a delegated onclick arrow with a $state-write body.
    "matrix/event_arrow",
    // a top-level `$effect(fn);` statement — `$.user_effect` + the runes frame
    // (`$.push($$props, true)` / `$.pop()` + the `$$props` param).
    "matrix/effect_arrow",
    // reactive text (single / multi / mixed) — bare signal reads, simple-ASCII chunks.
    "matrix/text_single_read",
    "matrix/text_multi_read",
    "matrix/text_mixed",
    // static template (single root / fragment / serialized attrs).
    "matrix/static_single_root",
    "matrix/static_fragment",
    "matrix/static_attrs_serialized",
    // a ROOT-level leading static TEXT before the first named dynamic position
    // (`x<button onclick={…}>{c}</button>`) — the official `is_text_first` PRE-CLONE
    // `$.next();` emitted BEFORE `var fragment = root();` (codegen bug A). The negative
    // is the in-element leading text (the §1.2 `<button>clicks: {count}</button>`),
    // which must NOT emit a pre-clone `$.next()` — covered by `runes/hello_input`.
    "matrix/root_leading_text",
    // ── static script-import prelude (the broad top-level import breadth) ──
    // A bare imported-ident read `{x}` is LIVE (`$.template_effect` + `$.set_text(text,
    // x)`, plain — never `$.get`, never a static fold) with NO context frame.
    "imports/bare_import_read_no_frame",
    // `bind:value={x.k}` where `x` is an import: a MEMBER of an import is an ACCEPTED
    // bind lvalue (`$.bind_value(input, () => x.k, ($$value) => x.k = $$value)`) and the
    // member read opens the context frame (`$.push($$props, true)` / `$.pop()`). Only
    // the BARE root `bind:value={x}` is rejected (non-writable import root).
    "imports/bind_member_of_import",
    // The `.svelte`-component default import consumed as a `<Child />` callee — the
    // component-callee subset rides the SAME broadened carrier.
    "imports/default_component_callee",
    // A COMBINED default + named clause (`import d, { n as m }`) — one statement, two
    // locals, both read live.
    "imports/default_named_mixed",
    // A COMBINED default + namespace clause (`import d, * as ns`) — one statement, the
    // namespace member read frames.
    "imports/default_namespace",
    // Two imports from the SAME source stay TWO statements (official does not merge).
    "imports/duplicate_source_unmerged",
    // An EMPTY named clause (`import {} from …`) binds nothing — official emits the
    // side-effect form (`import '…';`), captured as the golden.
    "imports/empty_named_side_effect",
    // `with { type: 'json' }` import attributes are preserved on emission.
    "imports/import_attributes_json",
    // Mixed default + named + namespace forms across sources, source order preserved.
    "imports/mixed_import_forms",
    // The two-slot ordering: `<script module>` imports emit BEFORE `import * as $`,
    // instance imports AFTER it.
    "imports/module_and_instance_slot_order",
    // A module-slot namespace MEMBER read `{NS.z}` frames (`$.push($$props, true)`).
    "imports/module_namespace_member_frames",
    // An import-only `<script module>` is ADMITTED (side-effect + named, module slot).
    "imports/module_script_import_only",
    // Three imports from three sources keep source order.
    "imports/multi_import_source_order",
    // Named imports (`{ x, y }`).
    "imports/named_import",
    // A named-alias import (`{ a as b }`) — the LOCAL binds; reads emit the local.
    "imports/named_import_alias",
    // An instance namespace import with a member read `{NS.z}` — live + frames.
    "imports/namespace_import",
    // A side-effect import (`import './setup.js'`) — no bindings, instance slot.
    "imports/side_effect_import",
];

/// The native-client ATTRIBUTE corpus — the `attributes/*` fixtures exercising the
/// dynamic-attribute / boolean-DOM-property / `class:`-`style:` directive / autofocus
/// surface. Each row runs through the IDENTICAL compile + OXC-parse + BYTE-PRECISE
/// full-module comparison gate as the matrix above: the committed `attributes/<slug>.
/// client.json` is the official oracle (regenerated from the pinned `svelte@5.56.3`),
/// and Verter's normalized emitted module must equal `clientModule` exactly. This is
/// the argument/offset/identifier-precise oracle for the attribute surface — the
/// substring/helper-name checks the corpus shipped with could not see a
/// `set_attribute` arg drift, a dropped literal chunk in a mixed property, the
/// single-vs-fragment clone-root choice, or a JS-escaping divergence.
const SUPPORTED_ATTRIBUTES: &[&str] = &[
    // A static-only attribute set baked into the `from_html` template (`id="x"
    // disabled class="base" type="submit"`) — no per-attribute dynamic op.
    "attributes/static_baked",
    // A single dynamic string attribute (`id={id}`) — `$.set_attribute` inside a
    // `template_effect`.
    "attributes/dynamic_attr",
    // Boolean DOM properties (`disabled={d}` / `readonly={r}`) — the `el.<prop> =`
    // property write, NOT `set_attribute`.
    "attributes/boolean_property",
    // The `contenteditable` / `hidden` set-attribute-vs-property split.
    "attributes/set_attribute_props",
    // `muted` on a `<video>` — the media-only DOM property, AND the lone-`<video>`
    // single clone-root (flag `2`, NOT a fragment) — the [P0] crash fixture.
    "attributes/muted_video",
    // `class={expr}` dynamic class — `$.set_class` / class accumulator.
    "attributes/class_expression",
    // `class:foo={on}` directives, with and without a base `class=`.
    "attributes/class_directives",
    // `style={expr}` dynamic style.
    "attributes/style_expression",
    // `style:color={c}` / `style:--x={x}` / `style:color|important={c}` directives.
    "attributes/style_directives",
    // `autofocus` (static + dynamic) — the `$.autofocus` init.
    "attributes/autofocus",
    // A single element carrying `id` + `class` + `style` dynamic attrs in one
    // `template_effect` (the combined-effect coalescing order).
    "attributes/combined_effect",
    // A `$props()`-backed dynamic attr (`id={who}`) — props reads are REACTIVE, so
    // the `$.set_attribute` joins the `$.template_effect` (not a one-shot init).
    "attributes/props_dynamic_attr",
    // A MIXED property value (`<video muted="pre-{v}-post">`) — the full literal+expr
    // template literal `video.muted = \`pre-${$.get(v) ?? ''}-post\``, never dropping
    // the literal chunks.
    "attributes/mixed_property",
    // `muted` on a NON-media element (`<div muted={v}>`) — `muted` is a DOM property
    // on ANY element (`is_dom_property` is element-agnostic), so `div.muted = $.get(v)`.
    "attributes/muted_on_div",
    // A call-expression attr (`id={String(v)}`) — the official memoized deps-array
    // `$.template_effect(($0) => $.set_attribute(el, 'id', $0), [() => String($.get(v))])`.
    "attributes/call_expr_attr",
    // JS-escaping edges — a static `class`/`style` base with an HTML ENTITY (decoded
    // for the JS-string / template chunk) and a `$` adjacent to an interpolation.
    "attributes/escaping_edges",
    // A call-expression CLASS base (`class={String(c)}`) — the base `$.clsx(...)` arg
    // is MEMOIZED into the deps-array form `[() => $.clsx(String($.get(c)))]`.
    "attributes/class_call_expr",
    // A call-expression STYLE directive (`style:width={String(c)}`) — the directives
    // object arg is memoized, parenthesized as `() => ({ width: String($.get(c)) })`.
    "attributes/style_directive_call",
    // ── Escaping edges: the JS-string + template-literal serializer surface ──
    // A static `class` BASE carrying a literal NEWLINE (`class="a\nb"`) consumed as a
    // single-quoted runtime string — the official esrap `quote` escapes `\n` → `\n`
    // (a raw newline inside `'…'` is invalid JS). Discriminates the `js_single_quoted`
    // newline escape.
    "attributes/static_class_newline",
    // A static `class` BASE carrying CR / TAB / BACKSLASH / QUOTE — the esrap-exact
    // escape set: `\r`→`\r`, `\`→`\\`, `'`→`\'`, and a TAB passes through VERBATIM
    // (a raw tab is valid inside `'…'`, and the official serializer leaves it).
    "attributes/static_string_escapes",
    // A REACTIVE mixed attribute whose decoded literal text contains `${` (`id="a${b{v}"`,
    // the `${` an entity-decoded literal) — emitted as a template literal, so the
    // official `sanitize_template_string` escapes `${` → `\${`. Discriminates the
    // `escape_template_text` `${` escape (an unescaped `${` is invalid/misparsed JS).
    "attributes/mixed_template_dollar",
    // ── Memoization granularity: per-expression-part, not whole-template ──
    // A REACTIVE mixed `class` base with a CALL expression (`class="a{String(c)}b"`) —
    // official memoizes the EXPRESSION PART (`` `a${$0 ?? ''}b` ``, dep `() => String(c)`),
    // NOT the whole rendered template. Discriminates the structured class base.
    "attributes/mixed_class_call",
    // The same per-part memoize for a mixed `style` base with a call.
    "attributes/mixed_style_call",
    // TWO reactive directives (a `class:` + a `style:`) on one node — the combined
    // BLOCK-body `$.template_effect(() => { classes = …; styles = …; })` with both
    // accumulators. Discriminates the multi-write memoized form + the normalizer's
    // bracket-hugging-whitespace symmetry.
    "attributes/reactive_multi_directive",
    // ── `bind:this` + init-domain ordering ──
    // `bind:this` sharing a node with `autofocus` + a `class:` directive — official
    // emits the init-domain writes (`$.autofocus`, `$.set_class`) BEFORE `$.bind_this`.
    "attributes/bind_this_init_order",
    // `bind:this` sharing a node with a one-shot dynamic attr (`id={who}`, `who` a
    // demoted `$state`) — `$.set_attribute` (init) BEFORE `$.bind_this`.
    "attributes/bind_this_dynamic_attr",
    // ── `has_call` reactive trigger (independent of `has_state`) ──
    // A DEMOTED `$state` call-expr property (`readonly={Boolean(v)}`, `v` never
    // written → `let v`) — official STILL memoizes into `$.template_effect(($0) =>
    // input.readOnly = $0, [() => Boolean(v)])` because the value `has_call`. The
    // official rule is `has_state || has_call`, NOT a rune-binding check.
    "attributes/call_expr_property_demoted",
    // Its REACTIVE counterpart (`v` written → `$.state`) — the same memoized shape
    // with a `$.get(v)` read; proves the demoted/reactive pair both byte-match.
    "attributes/call_expr_property_reactive",
    // An OPTIONAL-CHAIN method call in an attr (`readonly={v?.startsWith?.('x')}`) +
    // in a `class:` directive (`class:active={w?.endsWith?.('y')}`). Both `v`/`w` are
    // DEMOTED `$state` (read-only → plain `let`), so there is NO `has_state` reactive
    // read — the SOLE reason official memoizes both into the deps array (`[() =>
    // v?.startsWith?.('x'), () => ({ active: w?.endsWith?.('y') })]`) is `has_call`
    // (the optional method calls' callees root at a declared binding → `is_pure ===
    // false`). In OXC an optional call is an `Expression::ChainExpression` wrapping
    // `ChainElement::CallExpression`; discriminates the `has_call` scan's chain-
    // wrapped-call detection (a plain optional MEMBER like `c?.x` is NOT a call and
    // must not memoize).
    "attributes/optional_chain_call",
    // ── `has_call` is PER-CALL in SOURCE order (the `deps > 0` half) ──
    // A PURE optional call BEFORE its first dependency in source order
    // (`readonly={(globalThis?.check?.() ?? false) || flag}`, `flag` a demoted
    // `$state` → plain `let`, so there is NO `has_state` and `has_call` is the SOLE
    // memoize lever). Official's `dependencies` set accumulates AS the expression is
    // walked and the call's `has_call` check runs against the deps-SO-FAR: at the pure
    // call, zero deps have accumulated (the `flag` dependency is observed LATER), and
    // the callee roots at a global → NOT `has_call`. So official emits an INLINE
    // one-shot `input.readOnly = (globalThis?.check?.() ?? false) || flag`, NOT the
    // memoized deps-array form. Discriminates the per-call source-order rule: a
    // whole-expression "references any binding" precompute over-memoizes this into a
    // `$.template_effect`. (A boolean-property attr, not `class=`, so the clsx-wrap
    // path is not involved — the source-order memoize decision is isolated.)
    "attributes/pure_call_before_dep",
    // Its mirror — the SAME parts with the dependency FIRST
    // (`readonly={flag || (globalThis?.check?.() ?? false)}`). At the pure call, `flag`
    // has already accumulated (deps > 0) → `has_call` → official MEMOIZES into
    // `$.template_effect(($0) => input.readOnly = $0, [() => flag || (globalThis?.check?.() ?? false)])`.
    // The positive half of the source-order discrimination pair.
    "attributes/dep_before_pure_call",
];

/// The native-client EVENT corpus — the `events/*` fixtures exercising the
/// regular-element event surface: non-delegated `$.event`, capture-phase (the 4th
/// positional `true`), the legacy modifier WRAPPERS (`$.preventDefault` /
/// `$.stopPropagation` / … in the fixed inner→outer order), the `passive` / `nonpassive`
/// 5th-positional boolean (with the `void 0` capture-slot placeholder), and the
/// `is_passive_event` default on the modern delegated `touchstart` / `touchmove` path.
/// Each row runs the IDENTICAL compile + OXC-parse + AST-structural full-module
/// comparison as the matrix/attribute corpora — the committed `events/<slug>.client.json`
/// is the official `svelte@5.56.3` oracle. The full-module structural comparison signs
/// every call argument (booleans, `void 0`, nested wrapper calls), so a wrong wrapper
/// ORDER, a missing/extra capture/passive positional, or a delegated-vs-direct mode drift
/// fails here.
const SUPPORTED_EVENTS: &[&str] = &[
    // Non-delegated direct `$.event` — a nullary $state-write arrow, an EMPTY arrow,
    // a PARAM arrow, and an inline FUNCTION EXPRESSION (the last three discriminate the
    // broadened direct handler classifier — the narrow delegated classifier rejects
    // them). The function-expression row pins that a non-arrow inline handler is passed
    // through to `$.event` with its `$state` body rewritten, matching official.
    "events/nondelegated_focus",
    "events/nondelegated_empty_arrow",
    "events/nondelegated_param_arrow",
    "events/nondelegated_funcexpr",
    // Capture phase — the modern `*capture` suffix and the legacy `|capture` modifier,
    // both emitting the 4th positional `true`.
    "events/capture_suffix",
    "events/capture_legacy",
    // Each individual legacy modifier wrapper.
    "events/modifier_prevent_default",
    "events/modifier_stop_propagation",
    "events/modifier_stop_immediate",
    "events/modifier_self",
    "events/modifier_trusted",
    "events/modifier_once",
    // A two-modifier STACK and the all-six stack — the fixed inner→outer wrapper order.
    "events/modifier_stack",
    "events/modifier_all",
    // The passive / nonpassive 5th-positional boolean + the `void 0` capture slot.
    "events/modifier_passive",
    "events/modifier_nonpassive",
    // Capture combined with a modifier wrapper (the 4th positional `true` + the wrapper).
    "events/modifier_capture_combo",
    // The modern delegated `touchstart` / `touchmove` passive-by-default positional
    // (`$.delegated(..., void 0, true)`) — `is_passive_event` on the delegated path.
    "events/passive_touchstart_modern",
    "events/passive_touchmove_modern",
];

/// The SUPPORTED CONTROL-FLOW BLOCKS — `{#if}` / `{#each}` / `{#await}` / `{#key}`.
/// Each row runs the IDENTICAL compile + OXC-parse + AST-structural full-module
/// comparison as the other corpora — the committed `blocks/<slug>.client.json` is the
/// official `svelte@5.56.3` oracle. The structural comparison signs the block helper
/// (`$.if`/`$.each`/`$.await`/`$.key`), its argument arity (the each FLAG literal, the
/// `$.index` vs keyed key callback, the pending/then/catch slots, the `{:else}` fallback
/// arrow), the branch-render `$$render(consequent, …)` chain, and the per-region
/// signal-read rewrite (`$.get(item)` in the body, plain `item` in a keyed key callback,
/// plain `i` for an unkeyed index vs `$.get(i)` for a keyed index), so any structural or
/// signal-rewrite drift fails here.
//
// 5e scopes block-body CONTENT to the existing reactive-text surface: DIRECT
// bare-signal reactive reads (`{row}`/`{value}`/`{count}`/`{local}`/`{tripled}`),
// static text, simple mixed literal+interpolation runs whose interpolations are
// accepted bare-signal / no-default-prop reads (`got {value}`), and member-write /
// index-read EVENT HANDLERS (`onclick={() => row.x++}` / `onclick={() => n = i}`).
// Member READS inside reactive text (`{item.label}`, `{error.message}`) and
// compile-time const-folds are the global reactive-text/interpolation breadth —
// owned by the reactive-text/interpolation completion surface, with the const-fold
// sub-contract its own narrow piece — refused fail-closed, not emitted — so the
// coarse vendored `blocks/if_each_key` (member `{item.label}`) and
// `blocks/await_block` (member `{error.message}`) fixtures are NOT wired here; they
// land when that interpolation-breadth surface opens.
const SUPPORTED_BLOCKS: &[&str] = &[
    // `{#if}` chain (primary + `{:else if}` + `{:else}`) — the `$.if(node, ($$render) =>
    // { … })` branch-closure chain (`consequent` / `consequent_1` / `alternate`, with the
    // `$$render(fn, ordinal)` index args: none / `1` / `-1`).
    "blocks/if_chain",
    // `{#if}` with no else — a lone `if (test) $$render(consequent);`.
    "blocks/if_single",
    // Unkeyed `{#each}` — `$.each(node, 17, () => rows, $.index, ($$anchor, row) => …)`;
    // the item is a signal (`$.get(row)`), the source is a `() => rows` thunk.
    "blocks/each_unkeyed",
    // Unkeyed `{#each}` WITH an index read in a handler — the index is INERT (plain `i`,
    // NOT `$.get(i)`), flag 17 (no `EACH_INDEX_REACTIVE`); render callback `($$anchor,
    // row, i)`.
    "blocks/each_index",
    // Keyed `{#each}` WITH an index read in a handler — the index IS a signal (`$.get(i)`),
    // flag 19 (`EACH_INDEX_REACTIVE` set); the key callback is the PLAIN `(row) => row.id`.
    "blocks/each_keyed_index",
    // `{#each}…{:else}` — the trailing `($$anchor) => { … }` fallback arrow on `$.each`.
    "blocks/each_else",
    // `{#each}` with an item MEMBER WRITE in a handler (`row.x++` → `$.get(row).x++`) —
    // the official adds the otherwise-omitted `$$index` render param when the item is
    // mutated.
    "blocks/each_write",
    // Nested `{#each}` — the inner source reads the outer item signal (`() => $.get(cells)`);
    // each body item is its own signal.
    "blocks/each_nested",
    // `{#await p then v}` inline-then form — `$.await(node, () => p, null, thenFn)` (the
    // pending slot is `null`); the then value is a signal (`$.get(value)`).
    "blocks/await_inline_then",
    // `{#await p}<pending>{:then v}{/await}` — pending + then, no catch arg.
    "blocks/await_then_pending",
    // `{#await p}<pending>{:catch e}{/await}` — pending + catch, NO then: the absent
    // MIDDLE then slot is the `void 0` sentinel (`$.await(node, get, pending, void 0,
    // catch)`), distinct from the absent-pending `null`.
    "blocks/await_pending_catch",
    // `{#await p}{:catch e}{/await}` — catch only: an EMPTY pending arrow
    // `($$anchor) => {}` (the present-but-empty pending region), the absent then slot
    // the `void 0` sentinel, and the catch closure.
    "blocks/await_catch_only",
    // `{#key expr}` — `$.key(node, () => expr, ($$anchor) => { … })`; the body reads a
    // reactive `$.get(count)`.
    "blocks/key_reactive",
    // ── Text-first block bodies (a LONE text node / LONE accepted-interpolation run, NO
    // wrapping element) — the official `$.text(...)` in-closure topology, NOT a hoisted
    // clone factory. The owning-block-kind `$.next()` prelude is pinned per body here.
    // `{#if show}shown{/if}` — a lone STATIC-text consequent: `var text = $.text('shown');
    // $.append($$anchor, text);` (NO `$.next()` prelude for an `{#if}` body).
    "blocks/if_lone_text",
    // `{#if show}shown{:else}hidden{/if}` — a lone STATIC-text consequent AND alternate:
    // `var text = $.text('shown')` / `var text_1 = $.text('hidden')`, each `$.append(…)`
    // with NO `$.next()` prelude — pins the `{#if}` ALTERNATE "no-`$.next()`" cell (the
    // alternate is an `($$anchor) => { … }` arrow, no advance), distinct from the
    // consequent-only `if_lone_text` row above.
    "blocks/if_else_lone_text",
    // `{#each rows as row}x{/each}` — a lone STATIC-text each body: `$.next(); var text =
    // $.text('x'); $.append($$anchor, text);` (the `{#each}` body/else `$.next()` prelude).
    "blocks/each_lone_text",
    // `{#each rows as row}x{:else}empty{/each}` — a lone STATIC-text each body AND `{:else}`
    // fallback: BOTH the body (`$.next(); var text = $.text('x'); …`) and the trailing
    // fallback arrow (`$.next(); var text_1 = $.text('empty'); …`) carry the each `$.next()`
    // prelude — pins the each-`{:else}` `$.next()` cell, distinct from the each-body cell.
    "blocks/each_else_lone_text",
    // `{#each rows as row}{row}{/each}` — a lone REACTIVE-interp each body (the each-item
    // `EachSignal`): `$.next(); var text = $.text(); $.template_effect(() => $.set_text(text,
    // $.get(row))); $.append($$anchor, text);` — the bound `text` local, NOT an unbound one.
    "blocks/each_lone_interp",
    // `{#each xs as x}{x}{/each}{#each ys as y}{y}{/each}` — TWO reactive text-first sibling
    // each bodies: the first binds `text`, the SECOND a DISTINCT `text_1`, and its reactive
    // op is `$.set_text(text_1, $.get(y))` — pins the bound-`text` rebinding ACROSS sibling
    // text-first regions (a no-op'd binding would fall back to the out-of-scope literal
    // `text` and diverge on the second region's `$.set_text` argument).
    "blocks/each_sibling_interp",
    // `{#key k}x{/key}` — a lone STATIC-text key body: `var text = $.text('x');
    // $.append($$anchor, text);` (NO `$.next()` prelude for a `{#key}` body).
    "blocks/key_lone_text",
    // `{#await p}loading{:then v}{v}{:catch e}failed{/await}` — lone-text pending/catch +
    // lone-interp then bodies, all text-first, NONE with a `$.next()` prelude.
    "blocks/await_lone_text",
    // `{#each rows as row}hi {row}{/each}` — an accepted MIXED single-TextRun each body
    // (static `hi ` + bare-signal `{row}`): `$.next(); var text = $.text();
    // $.template_effect(() => $.set_text(text, `hi ${$.get(row) ?? ''}`)); $.append(…);`.
    "blocks/each_mixed_text",
    // `{#each rows as row}{@debug row}{row}{/each}` — a `{@debug}` INSIDE a text-first each
    // body: the debug `$.template_effect` emits AFTER `var text`, before the reactive
    // `$.set_text` effect (the walk-before-ops order), under the each `$.next()` prelude.
    "blocks/each_debug_text",
    // `{#if show}{@debug a}shown{/if}` — a `{@debug}` INSIDE a text-first if body: the debug
    // effect emits after `var text = $.text('shown')`, with NO `$.next()` prelude.
    "blocks/if_debug_text",
];

/// The SUPPORTED DECLARATION / `{@const}` / `{@debug}` TAGS. `{@const}` (runes mode) is a
/// block-local `const x = $.derived(() => …)` (reads `$.get(x)`); the `{const …}`/`{let …}`
/// declaration tag is an INERT block-local declaration UNLESS its declarator carries a
/// rune (`{let local = $state(0)}` registers a state transformer — reads `$.get(local)`,
/// writes `$.update(local)` — like an instance-script `$state`); `{@debug a, b}` is a
/// `$.template_effect(() => { console.log({ a: $.snapshot(a), b: $.snapshot(b) }); debugger; })`.
const SUPPORTED_DECLARATION_TAGS: &[&str] = &[
    // `{@const total = item.qty * item.price}` inside a `{#each}` — block-local derived.
    "declaration_tags/const_tag",
    // `{@const tripled = base * 3}` inside an `{#if}` branch — block-local derived.
    "declaration_tags/const_in_if",
    // `{const doubled = item * 2}` declaration tag — INERT plain `const doubled = …` (read
    // plain, NOT `$.get(doubled)`), initializer signal-rewritten (`$.get(item) * 2`).
    "declaration_tags/decl_plain",
    // `{let local = $state(0)}` rune-carrying declaration tag — classified through the
    // instance-script rune/state pipeline (`let local = $.state(0)`, `$.get`/`$.update`).
    "declaration_tags/decl_rune",
    // `{let doubled = $derived(item)}` rune-carrying declaration tag — a block-local
    // derived memo (`let doubled = $.derived(() => $.get(item))`, read `$.get(doubled)`).
    "declaration_tags/decl_derived",
    // `{@debug a, b}` — the reactive-effect snapshot log (`a` is a written signal →
    // `$.snapshot($.get(a))`; `b` is plain → `$.snapshot(b)`).
    "declaration_tags/debug_multi",
    // `<button/>{@debug a}` — a `{@debug}` AFTER a single-element clone-root sibling: the
    // effect emits at its document position (after the element's subtree), NOT hoisted.
    "declaration_tags/debug_after_sibling",
    // `<div>{@debug v}</div><button/>` — a `{@debug}` NESTED inside an element: the effect
    // interleaves into the element's child walk at its source-order slot.
    "declaration_tags/debug_in_element",
    // `{#if a}<button/>{@debug a}{/if}` — a `{@debug}` after a sibling INSIDE a block body:
    // the effect emits at its document position within the consequent region.
    "declaration_tags/debug_in_if",
    // `{#if a}{@debug a}{/if}` — a `{@debug}`-ONLY block body: the consequent emits JUST the
    // effect (no clone frame, no `$.append`) — the no-DOM-skeleton region shape.
    "declaration_tags/debug_only_in_if",
];

/// The native-client COMPONENT / SNIPPET / SLOT corpus — the 5f-a vertical's structural
/// oracle: static component calls, `{#snippet}` definitions, `{@render}` (static + dynamic),
/// component `let:` slot props, named slots / `<svelte:fragment>`, component bindings
/// (`bind:this` / `bind:prop` / function-pair), and the component-family specials
/// (`<svelte:component>` / `<svelte:self>` / `<svelte:fragment>`).
///
/// `components/standalone_child` is EXCLUDED — it is a LEGACY-mode (no-rune) component (Block
/// 5i), NOT a runes 5f-a conformance target (it carries goldens for the directory guard, but
/// the legacy import-flag prelude is not the runes-mode 5f-a surface).
///
/// `components/snippet_capture_state` and `components/child_and_snippet` are ALSO excluded —
/// they emit correctly through the 5f-a snippet/component machinery, but their PINNED goldens
/// require surfaces OUTSIDE the 5f-a vertical: `snippet_capture_state` const-folds a
/// never-mutated `$state` interpolation (`<span>{count}</span>` → `span.textContent = '0'`,
/// the reactive-text const-fold completion surface), and `child_and_snippet` uses an array
/// `$state([1, 2, 3])` → `$.proxy([…])` (the advanced non-primitive-`$state` surface, Block
/// 5g — gated at `state_decl_shape`). Both carry committed goldens for the directory guard;
/// they become runes 5f-a conformance targets once those deferred surfaces land.
const SUPPORTED_COMPONENTS: &[&str] = &[
    // A plain element + `{$props}` interpolation child — the imported child the component
    // fixtures call (already emittable by the 5a-5e element/prop surface; registered so the
    // child's own emission stays pinned).
    "components/Child",
    // `<Child label="hi" {name} count={1 + 2} />` — static / reactive-shorthand / constant-expr
    // props: `label: 'hi'`, `get name() { return $$props.name; }`, `count: 1 + 2` (sole-root
    // standalone, no template clone).
    "components/component_props",
    // The full member-order oracle: `<Child a="s" b={x} bind:value={v} onclick={() => x}>body
    // {x}</Child>` — plain attrs, then the bind get/set pair, then `children`, then `$$slots`.
    "components/component_full",
    // `<Child>hello {x}</Child>` — the default-slot `children: ($$anchor, $$slotProps) => {…}`
    // callback + `$$slots: { default: true }`.
    "components/component_children_default",
    // `<Child>{#snippet header(item)}…{/snippet} default {p}</Child>` — a component-nested
    // snippet def (local block const + `header` shorthand prop + `$$slots.header: true`) plus
    // the default slot.
    "components/component_snippet_children",
    // `<Child bind:value />` — component `bind:prop`: `get value()/set value()` with the
    // `$.set(value, $$value, true)` should-proxy axis.
    "components/component_bind_prop",
    // `<Child bind:this={child} />` — `$.bind_this(Child($$anchor, {}), set, get)`.
    "components/component_bind_this",
    // `<Child bind:value={() => v, (nv) => v = nv} />` — function-pair bind: `var bind_get` /
    // `var bind_set` hoists + `get value() { return bind_get(); }`.
    "components/component_bind_function",
    // `<Child bind:value={…} bind:other={…} />` — TWO function-pair binds: the second pair
    // allocates UNIQUE `bind_get_1` / `bind_set_1` locals (the component-function-scoped name
    // uniquing) so the two props' getters/setters never alias the same `var`.
    "components/component_bind_function_multi",
    // `<Child {...rest} />` — `Child($$anchor, $.spread_props(() => $$props.rest))`.
    "components/component_spread",
    // `<Child on:foo={…} />` — the legacy `on:` directive forwards as `$$events: { foo: … }`.
    "components/component_on_event",
    // `<Child onfoo={() => p} />` — a runes callback prop is a PLAIN init (`onfoo: () =>
    // $$props.p`), NOT `$$events`.
    "components/component_callback_prop",
    // `<Child let:item>{item}</Child>` — `let:` slot prop: `$$slots.default` callback with
    // `const item = $.derived(() => $$slotProps.item)` + `children: $.invalid_default_snippet`.
    "components/component_let",
    // `<Child let:item={value}>{value}</Child>` — an ALIASED `let:` slot prop: the slot prop
    // `item` renames to the local `value` (`const value = $.derived(() => $$slotProps.item)`).
    "components/component_let_alias",
    // `<Alpha /><Beta />{p}` — multiple imported components in SOURCE ORDER (`import Alpha`
    // then `import Beta`, both after `* as $`) + the multi-root `<!><!> ` comment-anchor
    // template.
    "components/multi_component_import",
    // `{#snippet pair(a, b)}<span>{a} {b}</span>{/snippet}` capturing only its params — a
    // MODULE-scope `const pair = ($$anchor, a = $.noop, b = $.noop) => {…}` between the imports
    // and the `$.from_html` hoists; reads params as thunks (`a()`).
    "components/snippet_multi_param",
    // `{#snippet tmpl(item)}…{/snippet}<Child {tmpl} />` — a snippet passed as a prop via the
    // shorthand getter `get tmpl() { return tmpl; }`.
    "components/snippet_to_component",
    // `{@render children?.()}` — dynamic optional render: `$.snippet(node, () => $$props.children
    // ?? $.noop)` (the `?? $.noop` is the ChainExpression form).
    "components/render_optional",
    // `{@render (cond ? a : b)()}` — dynamic ternary render: `$.snippet(node, () => cond ?
    // $$props.a : $$props.b)` (NO `?? $.noop`, not a chain).
    "components/render_dynamic_ternary",
    // `{@render row(item)}` with `let { row, item } = $props()` — a PROP-callee render with an
    // argument: the prop callee is NOT a `{#snippet}` name, so it stays the dynamic
    // `$.snippet(node, () => $$props.row, () => $$props.item)` shape — the callee thunk PLUS the
    // argument thunk (no `?? $.noop`, the callee is a non-optional prop read).
    "components/render_dynamic_prop_arg",
    // `{@render row?.(item)}` — an OPTIONAL prop-callee render with an argument:
    // `$.snippet(node, () => $$props.row ?? $.noop, () => $$props.item)` — the `?? $.noop`
    // ChainExpression guard on the callee thunk, PLUS the argument thunk.
    "components/render_dynamic_optional_arg",
    // `<svelte:component this={comp} label="hi" />` — `$.component(node, () => $$props.comp,
    // ($$anchor, $$component) => { $$component($$anchor, { label: 'hi' }); })`.
    "components/svelte_component",
    // `<svelte:component this={comp} bind:this={inst} />` — a DYNAMIC component with `bind:this`:
    // the inner `$$component($$anchor, {})` call is wrapped in `$.bind_this(<call>, ($$value) =>
    // inst = $$value, () => inst)` (the SAME `$.bind_this(call, set, get)` wrap the static callee
    // uses), proving the dynamic-host bind:this is not dropped.
    "components/svelte_component_bind_this",
    // `<svelte:component this={Child} {label} />` with `import Child from './Child.svelte'` — a
    // `.svelte` DEFAULT IMPORT consumed as the DYNAMIC component value: the import is admitted to
    // the prelude (`import Child from './Child.svelte';` after `import * as $`) and the `this`
    // expression resolves the non-reactive `ComponentImport` binding to the BARE local
    // (`$.component(node, () => Child, …)`, NOT `$.get`), while the threaded prop routes through
    // `$$props.label` — the binding-kind contrast that proves the import is NOT treated as a
    // reactive read. This is the dynamic-component-value half of the 5f-a `.svelte`-default-import
    // subset (the static-callee half is `multi_component_import` / `component_full`).
    "components/svelte_component_import",
    // `{#if depth > 0}<svelte:self depth={depth - 1} />{/if}` — a recursive self-call using the
    // compile-option name (`svelte_self(node_1, { get depth() { return $.get($0); } })`).
    "components/svelte_self",
    // `<Child><svelte:fragment slot="header"><span>h</span></svelte:fragment></Child>` — a
    // named slot via `<svelte:fragment slot>`: `$$slots: { header: ($$anchor, $$slotProps) =>
    // {…} }`.
    "components/svelte_fragment",
    // `<Child aria-label={x} />` — a HYPHENATED component prop key: a non-identifier prop name
    // must QUOTE the getter accessor (`get 'aria-label'() { return $$props.x; }`), not emit the
    // bare `get aria-label()` (unparseable JS).
    "components/component_prop_hyphen",
    // `<Child on:foo-bar={() => x} />` — a HYPHENATED legacy event key: the `$$events` entry key
    // must QUOTE (`$$events: { 'foo-bar': () => $$props.x }`), not the bare `foo-bar:`
    // (unparseable JS).
    "components/component_event_hyphen",
    // `<Child><svelte:fragment slot="foo-bar">{x}</svelte:fragment></Child>` — a HYPHENATED
    // named-slot key (via the SUPPORTED `<svelte:fragment slot>` path): the `$$slots` entry key
    // must QUOTE (`$$slots: { 'foo-bar': ($$anchor, $$slotProps) => {…} }`), not the bare
    // `foo-bar:` (unparseable JS).
    "components/fragment_slot_hyphen",
    // `<Child><span slot="foo-bar">{x}</span></Child>` — a REGULAR-ELEMENT named slot: the
    // element becomes the `$$slots: { 'foo-bar': (…) => {…} }` callback region AND its
    // `slot` attribute BAKES into the cloned skeleton (`<span slot="foo-bar"> </span>`);
    // the named region body has NO leading `$.next()`.
    "components/named_slot_span",
    // `<Child><span slot="foo&amp;bar">{x}</span></Child>` — an ENTITY-ENCODED named-slot
    // name: the slot name is the DECODED semantic key, so the `$$slots` entry is
    // `'foo&bar'` (official decodes attribute values at parse), while the baked skeleton
    // keeps the re-escaped HTML form (`<span slot="foo&amp;bar"> </span>`).
    "components/named_slot_entity",
    // `<Child><svelte:fragment slot="foo">hello {x}</svelte:fragment></Child>` — a
    // TEXT-FIRST fragment named slot: official emits the callback body WITHOUT the
    // `$.next()` cursor advance (`var text = $.text();` directly) — the named-slot region
    // is not an each/children-style render callback.
    "components/fragment_slot_text_first",
    // `{@render row([...xs])}` — a `has_call`-bearing render ARGUMENT (a spread counts as
    // a call): official memoizes it into a wrapping-block `let $0 = $.derived(() =>
    // [...$$props.xs]);` hoist and passes the `() => $.get($0)` thunk.
    "components/render_spread_arg",
    // `{@render (row)(1)}` — a paren-wrapped LOCAL-snippet callee peels to the bare
    // identifier and emits the DIRECT static call `row($$anchor, () => 1)` (never the
    // dynamic `$.snippet` route).
    "components/render_paren_callee",
    // `{@render row?.(1)}` on a LOCAL snippet — the DIRECT optional call
    // `row?.($$anchor, () => 1)` (the official `b.maybe_call` form; no `?? $.noop`).
    "components/render_optional_local",
    // `{@render Snips.row()}` with `import Snips from './Snips.svelte'` — an
    // IMPORT-rooted member render callee is never a "safe identifier", so
    // `needs_context` fires: official binds `$$props` and opens the frame
    // (`$.push($$props, true)` … `$.pop()`) around `$.snippet(node, () =>
    // Snips.row)`.
    "components/render_imported_member",
    // `{@render (new Date())()}` — a `new`-expression render callee: the
    // unconditional `needs_context` trigger; official binds `$$props` and
    // opens the frame around `$.snippet(node, () => new Date())`.
    "components/render_new_expression",
    // The `Inner.svelte` leaf the slot-disposition fixtures import.
    "components/Inner",
    // ── The official `slot=` three-class disposition ──
    // `<Child><Inner slot="foo" label={x}/></Child>` — a DIRECT component filler:
    // routed into `$$slots.foo` AND the inner call keeps the `slot` prop in source
    // order (`Inner($$anchor, { slot: 'foo', get label() {…} })`).
    "components/slot_filler_component_child",
    // `<Child><svelte:component this={comp} slot="foo"/></Child>` — the dynamic
    // component filler: `$$slots.foo` wraps `$.component(node, () => comp, ($$anchor,
    // $$component) => { $$component($$anchor, { slot: 'foo' }); })`.
    "components/slot_filler_svelte_component_child",
    // `<Child><svelte:self slot="foo" depth={depth - 1}/></Child>` — the recursive
    // self filler: `$$slots.foo` wraps the self-call with the `slot` prop + the
    // memoized `depth` derived.
    "components/slot_filler_svelte_self_child",
    // `<Child><svelte:element this="div" slot="foo" id={x}/></Child>` — the dynamic
    // element filler: `$$slots.foo` wraps `$.element(…)` and the `slot` FOLDS into
    // `$.attribute_effect($$element, () => ({ slot: 'foo', id: x }))` in source order.
    "components/slot_filler_svelte_element_child",
    // `<Inner slot="top" label={x}/>` at the ROOT — a NON-direct component `slot` is
    // an ordinary plain prop (`Inner($$anchor, { slot: 'top', … })`; no `$$slots`).
    "components/slot_prop_component_top_level",
    // `<div><Inner slot="bar" label={x}/></div>` — nested in an ELEMENT: still the
    // plain-prop route (`Inner(node, { slot: 'bar', … })`).
    "components/slot_prop_component_nested",
    // `<Child><div><Inner slot="bar" label={x}/></div></Child>` — nested in an
    // element INSIDE a component's default content: the element breaks direct-child
    // placement, so the `slot` stays a plain prop inside the `children:` callback
    // (`Inner(node, { slot: 'bar', … })`) and mints NO `bar` slot entry.
    "components/slot_prop_component_nested_in_component",
    // `<Inner slot={x}/>` — a DYNAMIC `slot` on a NON-direct component host is an
    // ordinary reactive prop (`get slot() { return $$props.x; }`) — official's
    // static-value rule fires only on a DIRECT component child.
    "components/slot_prop_component_dynamic",
    // `<svelte:component this={comp} slot="a"/>` at the root — the plain prop rides
    // the `$$component($$anchor, { slot: 'a' })` call.
    "components/slot_prop_svelte_component_top_level",
    // `{#if depth > 0}<svelte:self slot="a" depth={depth - 1}/>{/if}` — a validly
    // placed NON-direct `<svelte:self>`: the `slot` is a plain prop on the self-call.
    "components/slot_prop_svelte_self_nondirect",
    // `<Child><svelte:fragment slot="head"><Inner slot="x" …/></svelte:fragment></Child>`
    // — a component HOISTED out of a slotted fragment is NOT a direct component child
    // (officially `owner !== parent`), so its `slot="x"` stays a plain prop inside the
    // `head` callback and mints NO `x` slot entry.
    "components/slot_prop_component_in_slotted_fragment",
    // `{#if show}<Inner slot="c"/>{/if}` — a block body is not direct-child
    // placement: the plain prop inside the consequent callback.
    "components/slot_prop_component_in_block",
];

/// The native-client element LIFECYCLE-directive corpus (5f-c) — `use:` actions,
/// `transition:`/`in:`/`out:` transitions (with the `|global`/`|local` FLAG arithmetic),
/// keyed-each `animate:` animations, and element-position `{@attach}` attachments. Each
/// row runs the IDENTICAL compile + OXC-parse + AST-structural full-module comparison as
/// the other corpora — the committed `lifecycle/<slug>.client.json` is the official
/// `svelte@5.56.3` oracle. The structural comparison signs every helper call argument
/// (the `$.transition` FLAG integer literal, the `$.animation` 3-arg arity with its
/// literal `null`, the `$.action` `$$node`/`$$action_arg` closure params, the getter
/// thunks), so a wrong flag, a wrong helper family (`$.transition` for an `animate:`),
/// a dropped params thunk, or a phase-order drift (init-domain action/attach vs
/// post-event transition/animation) fails here.
const SUPPORTED_LIFECYCLE: &[&str] = &[
    // `use:foo` no-arg — `$.action(div, ($$node) => foo?.($$node))` (2 args,
    // optional-chained callee), emitted in the INIT domain (before events).
    "lifecycle/use_noarg",
    // `use:foo={c}` — 3 args: the closure gains `$$action_arg` and the 3rd arg is the
    // getter thunk `() => $.get(c)` (the arg rides the shared signal rewrite).
    "lifecycle/use_arg",
    // `use:obj.foo` — the dotted callee is preserved literally (`obj.foo?.($$node)`).
    "lifecycle/use_dotted",
    // ── The transition FLAG map: TRANSITION_IN(1) | TRANSITION_OUT(2) | TRANSITION_GLOBAL(4) ──
    // `transition:fade` → FLAG 3 (IN|OUT); no params → 3 args (no getParams thunk).
    "lifecycle/transition_both",
    // `in:fade` → FLAG 1.
    "lifecycle/transition_in",
    // `out:fade` → FLAG 2.
    "lifecycle/transition_out",
    // `in:fade|global` → FLAG 5 (1|4).
    "lifecycle/transition_in_global",
    // `out:fade|global` → FLAG 6 (2|4).
    "lifecycle/transition_out_global",
    // `transition:fade|global` → FLAG 7 (3|4).
    "lifecycle/transition_both_global",
    // `transition:fade|local` → FLAG 3 — `|local` is the DEFAULT (no +4), identical to
    // the bare `transition:fade`.
    "lifecycle/transition_local",
    // `transition:fade={{ duration: 200 }}` → the 4th getParams thunk
    // `() => ({ duration: 200 })` (present IFF params are given).
    "lifecycle/transition_params",
    // `animate:flip` in a KEYED each — `$.animation(div, () => flip, null)` (ALWAYS
    // 3 args; no params → the literal `null`), and the each FLAGS gain
    // `EACH_IS_ANIMATED` (8): 16 immutable + 1 item-reactive + 8 animated = 25.
    "lifecycle/animate_keyed",
    // `animate:flip={{ duration: 200 }}` — the 3rd arg becomes the getParams thunk.
    "lifecycle/animate_keyed_params",
    // `{@const l = item.n}` + `<div animate:flip>` in a keyed each — the official
    // "only child" placement check IGNORES `{@const}` / declaration-tag siblings
    // (`2-analyze/visitors/shared/element.js`), so the animate is ACCEPTED and the
    // each keeps the ANIMATED flag widening (25).
    "lifecycle/animate_keyed_const",
    // `<div {@attach fn}>` — element-position attachment: `$.attach(div, () => fn)`
    // (2 args, getter thunk), init-domain (before events).
    "lifecycle/attach_element",
    // `<div class="x" {@attach fn}>` — the static attr bakes into the `from_html`
    // template; the attach call is unchanged.
    "lifecycle/attach_colocated",
    // `<div {...c} use:foo transition:fade>` — spread + lifecycle CO-EXIST: the emission
    // order is `$.attribute_effect` (spread fold) → `$.action` → `$.transition`.
    "lifecycle/spread_lifecycle",
    // `<div use:foo on:click={…}>` — a `use:` action co-located with a NON-DELEGATED
    // event wraps the registration in its OWN effect: `$.action(div, …)` then
    // `$.effect(() => $.event('click', div, …))` in the init domain (the official
    // action-triggered effect wrap; without `use:` the same event emits BARE `$.event`).
    "lifecycle/use_legacy_event",
    // Two non-delegated events beside `use:` — one `$.effect(() => $.event(…))` PER
    // event (never a single effect grouping both registrations).
    "lifecycle/use_legacy_event_multi",
    // `use:` + `transition:` + `on:click` — the init-order proof: `$.action` →
    // `$.effect(() => $.event(…))` → `$.transition(3, …)`.
    "lifecycle/use_transition_legacy_event",
    // `<div use:foo bind:this={el}>` — the official SOURCE-ORDER interleave of the
    // init-domain render ops: `$.action` (first in source) then `$.bind_this` (the
    // reverse source order reverses the emission). Pinned by the full-module
    // comparator.
    "lifecycle/use_bind_this",
    // ── DYNAMIC-children placement: an inline render op on an element WITH a child
    // walk emits AFTER the element's entire child block (`$.child` descents /
    // `$.reset`), immediately before the post-walk effects — NOT before the walk.
    // (A static-children element has an empty child block, so its op stays right
    // after the clone frame — the rows above pin that half.) ──
    // `<div use:foo onclick><span>{c}</span></div>` — `$.action(div, …)` emits after
    // `$.reset(div)`, before the grouped `$.template_effect`.
    "lifecycle/use_dynamic_child",
    // `<div use:foo onclick>{c}</div>` — a pure-interp TEXT child still walks
    // (`$.child(div, true)` + `$.reset`); the action follows the reset.
    "lifecycle/use_text_child",
    // `<div {@attach foo} onclick><span>{c}</span></div>` — `$.attach` after the walk.
    "lifecycle/attach_dynamic_child",
    // `<div bind:this={el} onclick><span>{c}</span></div>` — `$.bind_this` after the
    // walk (the render-side binding follows the element's child block, not its inits).
    "lifecycle/bind_this_dynamic_child",
    // `<div use:foo on:click><span>{c}</span></div>` — the action-host effect-wrapped
    // non-delegated event rides the SAME post-child-walk slot: `$.reset(div)` →
    // `$.action` → `$.effect(() => $.event(…))` → `$.template_effect`.
    "lifecycle/use_event_dynamic_child",
    // `<div><span use:foo><span>{c}</span></span><span>{c}</span></div>` — a NESTED
    // action host: `$.action(span, …)` emits after `$.reset(span)` and BEFORE the
    // `$.sibling(span)` descent (the op stays at the element's walk position).
    "lifecycle/use_nested_sibling",
    // `<div transition:fade onclick><span>{c}</span></div>` — the POST-EVENT phase op
    // keeps its official slot with a dynamic child: `$.template_effect` →
    // `$.delegated` → `$.transition` → `$.append`.
    "lifecycle/transition_dynamic_child",
    // ── The event ORIGIN split: the official effect wrap AND the event↔transition
    // ordering key on the LEGACY `on:` origin, NOT on delegation. A MODERN
    // non-delegated `on*` attribute NEVER wraps and emits BEFORE the directive batch;
    // a bare LEGACY `on:` event joins the per-element directive batch (source-ordered
    // with `transition:` / `animate:`, child batches before the parent's). ──
    // `<div use:foo onmouseenter>` — a MODERN non-delegated event on a `use:` host
    // stays a BARE `$.event(…)` (the effect wrap is legacy-`on:`-only): `$.action` →
    // `$.event`, NO `$.effect`.
    "lifecycle/use_modern_nondelegated_event",
    // `<div transition:fade on:click>` — a bare LEGACY `on:` event joins the directive
    // batch in source order: `$.transition(3, …)` THEN `$.event('click', …)`.
    "lifecycle/transition_legacy_event_order",
    // `{#each … (key)}<div animate:flip on:click>` — the same batch order with an
    // animation: `$.animation(…)` THEN `$.event('click', …)`.
    "lifecycle/animate_legacy_event_order",
    // `<div transition:fade onmouseenter>` — the MODERN non-delegated event emits
    // BEFORE the batch: `$.event('mouseenter', …)` THEN `$.transition(3, …)`.
    "lifecycle/transition_modern_nondelegated_event",
    // `<div use:foo on:mouseenter>` — the LEGACY event on a `use:` host wraps:
    // `$.action` → `$.effect(() => $.event('mouseenter', …))`.
    "lifecycle/use_legacy_nondelegated_event",
    // `<div use:foo use:bar>` — two `$.action` calls in source order.
    "lifecycle/multiple_use",
    // `<div in:foo out:bar>` — `$.transition(1, …)` then `$.transition(2, …)`.
    "lifecycle/in_out_same",
    // `<div use:foo>{#if c}…{/if}</div>` — the inline render op emits after the
    // element's child block descent.
    "lifecycle/use_nested_if_child",
    // `{#if c}<div use:foo>x</div>{/if}` — a lifecycle element inside a block body.
    "lifecycle/lifecycle_in_if",
    // `<div on:click transition:fade>` — the batch is SOURCE-ordered, not a hard
    // events-after-transitions phase: `$.event('click', …)` THEN `$.transition(3, …)`.
    "lifecycle/legacy_event_before_transition",
    // `<div transition:fade><span on:click>x</span></div>` — element batches merge
    // POST-ORDER (the official `…child_state.after_update, …element_state.after_update`
    // merge): the CHILD's `$.event` precedes the PARENT's `$.transition`.
    "lifecycle/transition_parent_legacy_event_child",
    // `<div on:click><span transition:fade>x</span></div>` — the reverse nesting: the
    // CHILD's `$.transition` precedes the PARENT's `$.event`.
    "lifecycle/legacy_event_parent_transition_child",
    // ── The non-`this` DOM bind linearization: a bind on a `use:` action host wraps
    // in its OWN init-domain `$.effect(() => $.bind_*(…))` at its attribute source
    // position; without `use:` it joins the after-update directive batch (bare,
    // source-ordered with `$.transition`). `bind:this` NEVER wraps (use_bind_this). ──
    // `<input use:foo bind:value={v}>` — `$.action` then
    // `$.effect(() => $.bind_value(…))` in the init domain.
    "lifecycle/use_bind_value",
    // `<input transition:fade bind:value={v}>` — BOTH bare in the batch, source
    // order: `$.transition(3, …)` THEN `$.bind_value(…)`.
    "lifecycle/transition_bind_value",
];

/// The native-client SPECIAL-element corpus (5f-b) — the host / renderable specials
/// (`<svelte:window|document|body|element|boundary|head>`). Each row runs through the
/// IDENTICAL compile + OXC-parse + full-module-comparison gate as the matrix above: the
/// committed `special/<slug>.client.json` is the official oracle, and Verter's normalized
/// emitted module must equal `clientModule` exactly (argument/identifier-precise — the
/// `$.window` / `$.document` host expressions, the `$.set(…, true)` proxy flags, the
/// per-helper bind shapes, and the no-DOM init-only topology).
const SUPPORTED_SPECIALS: &[&str] = &[
    // <svelte:window> events — `$.event('<type>', $.window, handler)` in the init body, NO
    // template / NO `$.append`.
    "special/svelte_window_events",
    // <svelte:window> bind family — `$.bind_window_size('<name>', set)` (set-only, name
    // first), `$.bind_window_scroll('x', get, set)` (axis remapped, get+set),
    // `$.bind_online(set)`, `$.bind_focused($.window, set)`, and
    // `$.bind_property('devicePixelRatio', 'resize', $.window, set)` — every setter carrying
    // the `$.set(…, true)` window-host proxy flag.
    "special/svelte_window_binds",
    // <svelte:window bind:this> — `$.bind_this($.window, set, get)`.
    "special/svelte_window_this_bind",
    // <svelte:document> event + binds — `$.event('visibilitychange', $.document, h)`,
    // `$.bind_active_element(set)` (dedicated, set-only), and
    // `$.bind_property('<name>', '<event>', $.document, set)` (fullscreenElement /
    // visibilityState).
    "special/svelte_document_binds",
    // <svelte:document bind:this> — `$.bind_this($.document, set, get)`.
    "special/svelte_document_this_bind",
    // <svelte:body> event + dimension binds — `$.event('click', $.document.body, h)` and
    // `$.bind_element_size($.document.body, '<name>', set)` (host expr + the proxy flag; the
    // element form has NO proxy flag).
    "special/svelte_body_binds",
    // <svelte:body bind:this> — `$.bind_this($.document.body, set, get)`.
    "special/svelte_body_this_bind",
    // ── <svelte:element this={…}> dynamic element ──
    // STATIC tag — `$.element(node, () => 'div', false, cb)` (the literal thunk) + a
    // dimension bind against `$$element`.
    "special/svelte_element_static",
    // DYNAMIC tag — `$.element(node, () => tag, false, cb)` (comment-anchor + body region).
    "special/svelte_element_dynamic",
    // attrs + events — the `$.attribute_effect($$element, () => ({…}))` fold with the
    // hoisted `var event_handler = …` handler-stability local.
    "special/svelte_element_attrs",
    // bind:this — `$.bind_this($$element, set, get)` in the callback.
    "special/svelte_element_bind_this",
    // dimension bind — `$.bind_element_size($$element, 'clientWidth', set)` in the callback.
    "special/svelte_element_dimension",
    // empty inner body — the OMITTED 3-argument `$.element(node, () => tag, false)` call.
    "special/svelte_element_empty",
    // element child — the body region clones a `$.from_html` template inside the callback.
    "special/svelte_element_child",
    // static class + `class:` directive — the official lone-class fast path WITH the
    // directive object: `$.set_class($$element, 0, 'card', null, {}, { active: x })` (NOT
    // an `$.attribute_effect` fold).
    "special/svelte_element_class_directive",
    // MIXED-CASE static `CLASS` + `class:` directive — official matches the plain class
    // attribute name case-insensitively (`attributes[0].name.toLowerCase() === 'class'`),
    // so the fast path still fires: `$.set_class($$element, 0, 'card', null, {},
    // { active: x })` (NOT a case-preserved `CLASS: 'card'` attribute_effect fold).
    "special/svelte_element_class_mixed_case",
    // static class + `style:` directive — the directive synthesizes the empty `style`
    // attribute, so the element routes to the fold: `$.attribute_effect($$element, () =>
    // ({ class: 'x', style: '', [$.STYLE]: { color: c } }))`.
    "special/svelte_element_style_directive",
    // bind:this + an attribute fold — `$.bind_this($$element, …)` is a ref capture
    // emitted BEFORE the `$.attribute_effect` fold (the official init order).
    "special/svelte_element_this_and_fold",
    // ── <svelte:boundary> error boundary ──
    // plain — `$.boundary(node, {}, cb)` (empty props) + a reactive body region.
    "special/svelte_boundary_plain",
    // onerror — `$.boundary(node, { onerror: … }, cb)` (the state-bearing props member).
    "special/svelte_boundary_onerror",
    // failed snippet — the hoisted `const failed = ($$anchor, error = $.noop, reset = $.noop)
    // => {…}` in the wrapping block + `{ failed }` props shorthand.
    "special/svelte_boundary_failed",
    // pending snippet — the hoisted `const pending = ($$anchor) => {…}` + `{ pending }`.
    "special/svelte_boundary_pending",
    // onerror + failed + pending — both snippets hoisted, `{ onerror, failed, pending }`.
    "special/svelte_boundary_full",
    // failed ATTRIBUTE expression (a state-bearing prop ref) — the getter props member
    // `{ get failed() { return $$props.failed; } }` (NO snippet hoist, NO wrapping block).
    "special/svelte_boundary_failed_attr",
    // failed ATTRIBUTE rooted at a PLAIN-LOCAL member (`failed={obj.failed}`, `obj` a
    // bind-target local) — the MEMBER-ROOT half of official's `has_state` promotes the
    // prop to the getter `{ get failed() { return obj.failed; } }`.
    "special/svelte_boundary_failed_member",
    // failed ATTRIBUTE carrying a SPREAD expression (`failed={[...xs]}`, `xs` a prop) —
    // official emits the plain UNMEMOIZED getter `{ get failed() { return [...$$props.xs];
    // } }` (`has_state` promotes to `b.get`; boundary props are never `$.derived`-hoisted).
    "special/svelte_boundary_spread",
    // pending ATTRIBUTE expression — the getter props member `{ get pending() { return
    // $$props.pending; } }`.
    "special/svelte_boundary_pending_attr",
    // onerror + failed + pending ALL as attributes, in source order — a NON-state onerror
    // arrow stays the plain `onerror: …` init, the state-bearing failed/pending become
    // getters: `{ onerror: …, get failed() {…}, get pending() {…} }`.
    "special/svelte_boundary_all_attrs",
    // a failed ATTRIBUTE + a `{#snippet pending}` CHILD — the getter attr prop precedes the
    // hoisted-snippet shorthand: `{ get failed() {…}, pending }` in a wrapping block.
    "special/svelte_boundary_mixed_attr_snippet",
    // CONFLICT: a failed ATTRIBUTE + a `{#snippet failed}` CHILD both present — official emits
    // BOTH keys (getter then shorthand) in source order: `{ get failed() {…}, failed }` (a
    // duplicate-key object, valid ES2015+). Verter matches official parity, no dedupe.
    "special/svelte_boundary_conflict_attr_snippet",
    // ── <svelte:head> ──
    // static title — `$.head(hash, ($$anchor) => { $.effect(() => { $.document.title =
    // 'literal'; }); })` (has_state false ⇒ `$.effect`), a head-only root (NO body skeleton).
    "special/svelte_head_static_title",
    // prop title — `$.deferred_template_effect(() => { $.document.title = $$props.t ?? ''; })`
    // (has_state true ⇒ deferred; `?? ''` since the value is not provably defined).
    "special/svelte_head_prop_title",
    // mutated-$state title — `$.deferred_template_effect(() => { $.document.title = `page
    // ${$.get(t) ?? ''}`; })` (a multi-chunk template literal, per-interpolation `?? ''`).
    "special/svelte_head_state_title",
    // title + single meta — the title effect (after_update) BETWEEN the meta clone and its
    // `$.append` (single meta ⇒ NO `$.next()`).
    "special/svelte_head_title_meta",
    // meta-only (two roots) — `var fragment = root(); $.next(2); $.append(...)` in the
    // callback, NO title effect.
    "special/svelte_head_meta",
    // head + body sibling — the `$.head(...)` at its SOURCE position (before the sibling `<p>`
    // clone's `$.append`).
    "special/svelte_head_body_sibling",
    // pre-existing fixture — folded `$state` title (`$.effect` + literal) + a `{@html}` body.
    "special/svelte_head_html",
];

/// The native-client CUSTOM-ELEMENT corpus — the `options/custom_element_*`
/// fixtures exercising the `<svelte:options customElement>` accept surface: the
/// conditional 6-arg `$.create_custom_element(Cmp, props, slots, accessors,
/// shadowRootInit, extend)` module epilogue, the `customElements.define`
/// presence rule (string/`{tag}` define; `{}`/compile-option create-only;
/// `{null}` no-op), the fact-driven body frame (`$.push($$props, true)` +
/// `$$exports` get/set accessors + `return $.pop($$exports)` ONLY when prop
/// accessors exist), and the `$host()` → `$$props.$$host` handler lowering.
/// Each row runs the IDENTICAL compile + OXC-parse + full-module structural
/// comparison as the corpora above — the committed
/// `options/<slug>.client.json` is the pinned `svelte@5.56.3` oracle.
const SUPPORTED_OPTIONS: &[&str] = &[
    // A string tag (`customElement="my-el"`) — the 5-arg open-shadow default
    // (`{ mode: 'open' }` arg5) + `customElements.define` AFTER the
    // `$.delegate` epilogue; NO body frame (no props).
    "options/custom_element_string_tag",
    // `{ tag, shadow: 'none' }` — the arg5-OMITTED 4-arg call.
    "options/custom_element_shadow_none",
    // `{ tag, shadow: 'open', props: { count: { reflect, type } } }` + a
    // defaulted `$props()` member — the explicit prop-definition object in
    // arg2, the accessor-forced `$.prop($$props, 'count', 7, 7)` (flags 7 =
    // IMMUTABLE|RUNES|UPDATED + the default), the `$$exports` get/set pair with
    // the RAW setter default (`set count($$value = 7)`), and the
    // `return $.pop($$exports)` close.
    "options/custom_element_props",
    // `{ tag: 'x-id' }` + a NON-identifier `$props()` source key
    // (`let { 'data-id': dataId }`) — every surfaced key is QUOTED (the
    // official `b.key(name)` rule): the arg2 inferred definition
    // (`{ 'data-id': {} }`), the `$$exports` accessor names
    // (`get 'data-id'()` / `set 'data-id'($$value)`), and the
    // `$.prop($$props, 'data-id', 7)` source key. A raw unquoted key is
    // invalid JS.
    "options/custom_element_string_prop_key",
    // `{ tag, props: { a: { attribute: "" } } }` — an EMPTY `attribute` string
    // OMITS the field entirely (the official transform pushes `attribute` only
    // for a truthy string): the arg2 definition is `{ a: {} }`, never
    // `attribute: ''`.
    "options/custom_element_empty_attribute",
    // A SCRIPTLESS template-only `$host()` customElement (`onfocus={() =>
    // $host().dispatchEvent(...)}`, no `<script>` at all): the template `$host`
    // reference alone infers RUNES mode, and the member-accessed host binds
    // `$$props` + pushes the frame (`$.event('focus', button, () =>
    // $$props.$$host.dispatchEvent(new CustomEvent('boop')));`).
    "options/custom_element_host_template_only",
    // `{ tag, shadow: 'none', extend: (c) => c }` — the 6-arg call: arg5 is the
    // `void 0` placeholder, arg6 the verbatim extend expression.
    "options/custom_element_extend",
    // `{}` (no tag) — the bare `$.create_custom_element(…)` statement, NO
    // define (registration is the user's).
    "options/custom_element_no_tag",
    // `{null}` — the Svelte-3 backwards-compat NO-OP: a plain component (no
    // create, no define).
    "options/custom_element_null",
    // The `customElement: true` COMPILE OPTION (see `compile_options_for` +
    // `FIXTURE_COMPILE_OPTIONS` in `gen-svelte-goldens.mjs`) — create, no
    // define.
    "options/custom_element_option_true",
    // The FRAMED `$host()` handler form: `$.push($$props, true)` + the
    // `$$props.$$host.dispatchEvent(…)` lowering inside the direct `$.event`
    // handler + the statement `$.pop();` close (no props ⇒ no `$$exports`).
    "options/custom_element_host_handler",
    // `{ tag, shadow: { mode: 'open', delegatesFocus: true } }` — the
    // `ShadowRootInit` object expression passed VERBATIM as arg5.
    "options/custom_element_shadow_object",
    // `({ tag: 'x-paren' })` — the PARENTHESIZED descriptor object: upstream's
    // `read_expression` returns `remove_parens(node)` before `read_options`, so
    // author parens are transparent and the emission is identical to the
    // unwrapped `{ tag }` spelling (define + 5-arg open-shadow default).
    "options/custom_element_paren_object",
    // `{@render $host().snip()}` — a `$host()`-member DYNAMIC render callee:
    // the peeled callee is a call-result-rooted member (never a "safe
    // identifier"), so `needs_context` fires — official binds `$$props` AND
    // opens the frame (`$.push($$props, true)` … `$.pop()`) around the
    // `$.snippet(node, () => $$props.$$host.snip)` render.
    "options/custom_element_render_host_member",
    // A LEGACY custom element with an `export let` prop: the accessors force
    // composes UPDATED onto the legacy base (`$.prop($$props, 'label', 12,
    // 'x')`), the `$$exports` get/set pair reads and writes through the
    // accessor (the setter takes NO default param), the frame is
    // `$.push($$props, false)` … `return $.pop($$exports);`, and NO `$.init()`
    // — the `$$exports` frame reason alone never warrants the legacy init
    // hook.
    "options/custom_element_legacy_export",
];

/// The `$store` auto-subscription corpus — the mode-independent client store
/// surface: the per-store accessor thunk (`const $NAME = () => $.store_get(NAME,
/// '$NAME', $$stores);`), the shared `const [$$stores, $$cleanup] =
/// $.setup_stores();` body-top setup, the trailing `$$cleanup();` finalizer, the
/// write lowerings (`$.store_set` / `$.update_store` / `$.update_pre_store`),
/// and the mode-sensitive component frame (`$.push($$props, false)` legacy /
/// `true` runes — the frame itself driven by the EXISTING `needs_context`
/// triggers, never by store presence).
const SUPPORTED_STORES: &[&str] = &[
    // The IMPORTED-writable legacy case (button handler updates through the
    // store object): push=false + flags/legacy + `$.init()` + setup/cleanup.
    "stores/store_auto_subscribe",
    // RUNES + store: `$.push($$props, true)` (the imported `writable(0)` call
    // trips `needs_context`), NO `svelte/internal/flags/legacy`, NO `$.init()`,
    // store accessor alongside a `$state` signal.
    "stores/store_runes_mode",
    // The MINIMAL legacy imported store (interpolation-only): push=false +
    // flags/legacy + `$.init()`.
    "stores/store_legacy_only",
    // MAYBE-RUNES control for the legacy value wrap (store-only component —
    // the official in-between mode): the call-bearing attr dep stays RAW
    // (`[() => $s().m()]`) — no `deep_read_state`, no `untrack`.
    "stores/maybe_runes_attr_call",
    // Store WRITES: `$c = 5` in a named handler and `$c = 0` in an inline
    // arrow handler — both lower to `$.store_set(c, …)`.
    "stores/store_write",
    // The COMPOUND write forms: `$c++` → `$.update_store(c, $c())`, `$c--` →
    // `$.update_store(c, $c(), -1)`, `++$c` → `$.update_pre_store(c, $c())`,
    // `$c += 2` → `$.store_set(c, $c() + 2)`.
    "stores/store_compound",
    // TWO stores: ordered accessor consts (`$a` then `$b`, first-subscription
    // order) before ONE shared `$.setup_stores()`.
    "stores/store_multiple",
    // `derived(a, ($a) => $a * 2)`: accessor for `$doubled` ONLY — the callback
    // param `$a` is scope-shadowed and mints NO accessor; the un-subscribed
    // `const a = writable(1)` is admitted as a store DEPENDENCY.
    "stores/store_derived_shadowed",
    // A LOCAL hand-rolled store factory (no `svelte/store` import, no `new`,
    // no imported call): store lowering IDENTICAL to the imported case BUT NO
    // component frame (no `$.push`/`$.pop`, no `$$props` param) — the frame is
    // `needs_context`-driven, never store-presence-driven.
    "stores/store_local_factory",
    // `bind:value={$c}`: the getter is the BARE accessor thunk `$c`, the
    // setter the `($$value) => $.store_set(c, $$value)` closure.
    "stores/store_bind_value",
    // A store whose NAME is a rune-root word, LEGACY mode: `const state =
    // writable(0)` + `{$state}` subscribes (`const $state = () =>
    // $.store_get(state, '$state', $$stores)`) — base resolution decides
    // store-vs-rune, never the name (official emits the subscription).
    "stores/store_rune_named_state",
    // The SAME rune-root-named store under FORCED runes mode
    // (`<svelte:options runes={true} />`): still a subscription — `$state`
    // over a declared non-rune-init base is a store accessor in EVERY mode.
    "stores/store_rune_named_state_runes",
    // A `$derived`-named store, legacy: `const derived = writable(0)` +
    // `{$derived}` subscribes identically.
    "stores/store_rune_named_derived",
    // The each-body NON-shadow control: `{#each items as x}<p>{$y}</p>{/each}`
    // subscribes `$y` from inside the block body (the each alias `x` shadows
    // nothing — only a base-name collision rejects, per
    // `store_invalid_scoped_subscription`).
    "stores/store_each_nonshadow",
    // A LOCAL CLASS-based store: `class S { subscribe(fn){…} } const c = new
    // S(); {$c}` — the class is admitted into the store-dependency closure (a
    // store factory reached transitively from the `const c = new S()` source of
    // the `$c` subscription), emitted VERBATIM, and `$c` subscribes identically.
    "stores/store_class_local",
    // A store component that ALSO carries custom-element `$$exports` prop
    // accessors: the close uses the official PRE-RETURN finalizer slot `var $$pop
    // = $.pop($$exports); $$cleanup(); return $$pop;` (the store `$$cleanup()`
    // runs BEFORE the captured export return — a bare `return $.pop($$exports);`
    // would strand the cleanup). SUPPORTED, not fail-closed.
    "stores/store_custom_element",
];

/// The LEGACY (non-runes) reactivity corpus: `export let` props through the
/// shared `$.prop` prop-source substrate (legacy base flags, accessor-call
/// reads, the official default algorithm) and the demand-driven `let` →
/// `$.mutable_source` promotion (handler/function/bind writes, `$.get`/`$.set`/
/// `$.update` routing, the `$.mutate` deep-mutation wrap, the un-proxied legacy
/// special-host setter).
const SUPPORTED_LEGACY: &[&str] = &[
    // ── legacy (non-runes) reactivity: `export let` props + promoted `let` ──
    // A bare legacy `export let` prop: `let label = $.prop($$props, 'label', 8)`
    // (legacy base flags 8 — BINDABLE by default), accessor-call reads
    // (`label()`), the `$$props` param, NO context frame.
    "legacy/export_let_bare",
    // A DEFAULT-bearing export let: the simple literal default passes RAW as the
    // 4th `$.prop` arg (no lazy thunk, no lazy bit).
    "legacy/export_let_default",
    // A template-MUTATED export let: flags 12 (8 | UPDATED 4), the increment
    // through `$.update_prop(count)`, reads still accessor calls.
    "legacy/export_let_mutated",
    // A bare REASSIGNED export let: flags 12, the write through the setter call
    // (`v(2)`).
    "legacy/export_let_reassigned",
    // TWO export-let props: ONE `let <local> = $.prop(...)` declaration per
    // prop in source order (official splits per declarator), the grouped
    // two-read `$.template_effect`.
    "legacy/export_let_multiple",
    // A default READING a SIBLING prop (`export let b = a`): the sibling read
    // rewrites to the getter and collapses to the BARE getter as the LAZY
    // carrier — `$.prop($$props, 'b', 24, a)` (flags 24 = BINDABLE 8 | LAZY 16).
    "legacy/export_let_sibling_default",
    // A promoted legacy `let` written in an ADMITTED function body
    // (`onclick={inc}`): `$.mutable_source(0)`, the compound assign
    // `$.set(count, $.get(count) + 1)`, reads `$.get`.
    "legacy/let_function_write",
    // A promoted legacy `let` written by an inline handler: `$.mutable_source(0)`,
    // `$.update(count)`, `$.get(count)`, NO `$$props`, NO frame.
    "legacy/let_handler_write",
    // A bind-target legacy `let`: `$.mutable_source('x')` + the
    // `$.bind_value(input, () => $.get(v), ($$value) => $.set(v, $$value))`
    // thunks (the former "legacy let binding" refusal, now supported).
    "legacy/let_bind_value",
    // The UNINITIALIZED bind-target form: the ZERO-ARG `$.mutable_source()`.
    "legacy/let_bind_uninit",
    // A MEMBER bind target rooted at a promoted object let: the setter wraps in
    // the official deep-mutation helper
    // (`($$value) => $.mutate(o, $.get(o).x = $$value)`).
    "legacy/let_bind_member",
    // A member MUTATION in a handler: `$.mutate(o, $.get(o).x++)`.
    "legacy/let_member_mutate",
    // A special-host (window) bind target under legacy: the setter carries NO
    // proxy flag (`($$value) => $.set(y, $$value)` — the runes-only `, true` is
    // absent).
    "legacy/let_bind_window",
    // ── `$:` reactive statements (`$.legacy_pre_effect` registrations) ──
    // A bare-ident reactive assignment (`$: y = x + 1`): the synthesized
    // zero-arg `const y = $.mutable_source();`, the dep thunk `() =>
    // ($.get(x))`, the `$.set(y, …)` body, ONE trailing
    // `$.legacy_pre_effect_reset()`, push/pop WITHOUT `$.init()`.
    "legacy/reactive_assign",
    // A block-bodied `$:` (`$: { t = x * 2; console.log(t); }`): ONE effect
    // wrapping the whole block; `t` is read inside so it joins the dep thunk
    // in first-mention order (t, x).
    "legacy/reactive_block",
    // An `if`-bodied `$:`: the statement wraps verbatim; the pure
    // assignment-LHS `big` is NOT a dependency.
    "legacy/reactive_if",
    // TWO `$:` statements in reverse dependency order (`$: z = y + 1; $: y =
    // x + 1;`): declarations in source order, REGISTRATIONS topologically
    // ordered (the y-assigner registers first).
    "legacy/reactive_topo_order",
    // A prop + store + `$:` combination: all three dep wrappers in one thunk
    // (`$.deep_read_state(p())`, the bare accessor `$c()`), store setup, AND
    // `$.init()` (the store-factory call is an unsafe imported call).
    "legacy/reactive_prop_store",
    // A prop + `$:` with NO store: `$.deep_read_state(p())` in the thunk,
    // `$.push($$props, false)` frame, NO `$.init()` (the reactive statement
    // opens the frame without the legacy init reason).
    "legacy/reactive_prop_only",
    // A prop WRITTEN by a `$:` statement (`export let p; $: p = x;`): the
    // declaration composes UPDATED onto the legacy base — flags 12 — and the
    // effect body writes through the setter call (`p($.get(x))`); no colliding
    // cell synthesizes for the declared prop target.
    "legacy/reactive_prop_write",
    // A `$:` for-of whose head binding SHADOWS an outer reactive `let`
    // (`let i = 0; $: for (const i of [1, 2]) { console.log(i); }`): the dep
    // thunk is EMPTY (`() => {}`) — the loop-local `i` never records a
    // dependency on the outer cell — and the body keeps the loop-local reads
    // bare.
    "legacy/reactive_for_shadow",
    // A PARENTHESIZED reactive assignment (`$: (y = x + 1)`): the paren wrapper
    // is transparent to the implicit-target declaration pass — `y` still
    // synthesizes the zero-arg `const y = $.mutable_source();` and the body
    // writes through `$.set(y, …)` (the redundant paren carrier is waived by
    // the structural comparator).
    "legacy/reactive_paren_assign",
    // ── the `<slot>` outlet (`$.slot`) ──
    // A default `<slot />` inside an element: the `<div><!></div>` anchor
    // skeleton, `$.slot(node, $$props, 'default', {}, null)`, the `$$props`
    // param with NO frame and NO `$.init()`.
    "legacy/slot_default",
    // A NAMED slot with a reactive + a static prop: `$.slot(node, $$props, 'x',
    // { get foo() { return a(); }, bar: 'b' }, null)` — the legacy prop
    // accessor read inside the getter, the static init, the consumed `name`.
    "legacy/slot_named_props",
    // A spread slot: `$.spread_props({ a: '1' }, rest)` — the ONE leading
    // ordinary object, the unthunked zero-arg accessor spread.
    "legacy/slot_spread",
    // A CALL-BEARING legacy slot prop: the official legacy memo topology —
    // `let $0 = $.derived_safe_equal(() => ($.deep_read_state(obj()),
    // $.untrack(() => obj().m())));` + the `$.get($0)` getter — plus the
    // unsafe-call component frame (`$.push($$props, false)` … `$.init()` …
    // `$.pop()`).
    "legacy/slot_prop_call",
    // A CALL-BEARING legacy slot SPREAD: official `SlotElement.js` never
    // memoizes and never wraps a spread — the plain thunk
    // `$.spread_props({}, () => obj().m())` (+ the unsafe-call frame).
    "legacy/slot_spread_call",
    // A CALL-BEARING legacy COMPONENT prop — the shared-owner memo topology
    // twin of `slot_prop_call` (the same `DerivedMemoizer` + legacy wrap route):
    // `$.derived_safe_equal` over the deep-read/untrack sequence, `$.get($0)`
    // getter, unsafe-call frame.
    "legacy/component_prop_call",
    // A MEMBER-bearing (non-call) legacy component prop: NOT memoized, but the
    // getter legacy-wraps — `get foo() { return ($.deep_read_state(obj()),
    // $.untrack(() => obj().x)); }` (+ the unsafe-member frame).
    "legacy/component_member_prop",
    // A non-empty fallback: the fallback template hoists BEFORE the parent
    // (post-order `root` / `root_1`) and renders as the `($$anchor) => { … }`
    // callback region.
    "legacy/slot_fallback",
    // A sole-root slot with fallback: the `$.comment()` anchor frame (no
    // `from_html` for the root; the fallback hoists its own template).
    "legacy/slot_root_fallback",
    // ── `createEventDispatcher` (the legacy component-event surface) ──
    // A used dispatcher: the preserved instance-slot `svelte` import, the
    // verbatim `const dispatch = createEventDispatcher();`, the plain
    // `dispatch('go', 1)` call, and the legacy frame (`$.push($$props, false)`
    // … `$.init()` … `$.pop()`) driven by the imported-call `needs_context`.
    "legacy/dispatcher",
    // An UNUSED dispatcher import: the import is preserved, NO frame, NO
    // `$.init()`, NO `$$props` param.
    "legacy/dispatcher_unused",
    // ── the legacy value wrap on the `$.template_effect` attr/text surface ──
    // The combined DOM-attribute + text probe: the call-bearing attr memoizes
    // its wrapped sequence (`[() => ($.deep_read_state(obj()), $.untrack(() =>
    // obj().m()))]`) while the imported-member text wraps INLINE in the same
    // effect — `deep_read_state` × 2 / `untrack` × 2 pinned.
    "legacy/attr_text_call_wrap",
    // A NON-call member attr value wraps INLINE (wrap precedes the memoize
    // decision): `$.set_attribute(div, 'title', ($.deep_read_state(obj()),
    // $.untrack(() => obj().x)))` — no deps array.
    "legacy/attr_member_inline_wrap",
    // TWO positions of the identical call: one independent wrapper sequence
    // per dep — no cross-position dedup, no cross-dep deep-read merge.
    "legacy/attr_two_positions_call",
    // An IMPORTED zero-arg callee: the import joins the dep as
    // `$.deep_read_state(helper)` and the call untracks BY REFERENCE.
    "legacy/attr_imported_call",
    // A PLAIN-LOCAL callee negative: untracked (`[() => ($.untrack(m))]`) with
    // NO fabricated `deep_read_state`.
    "legacy/attr_plain_local_call",
    // A MIXED attribute: the call chunk memoizes its own wrapped sequence into
    // `$0` while the member chunk stays inline-wrapped with `?? ''` — per-chunk
    // granularity.
    "legacy/attr_mixed_call_member",
    // GUARDRAIL control: the `class:` directive OBJECT is a SYNTHESIZED
    // memoizer value — official emits it RAW (`[() => ({ foo: obj().m() })]`),
    // no wrap; a blanket memoizer-level wrap would break this golden.
    "legacy/class_directive_call_raw",
    // ── the unified authored-value preparation (the remaining wrap surfaces) ──
    // The class single base wraps INSIDE the synthesized `$.clsx` (memoized
    // call + inline member twin).
    "legacy/wrap_class_clsx_call",
    // The style single base memoizes wrapped; the `style:` directive inner
    // values wrap inside the memoized `[normal, important]` object pair.
    "legacy/wrap_style_directive_call",
    // The `{#if}` member test wraps inline; the `{:else if}` call test hoists
    // the mode-independent `var d = $.derived(() => (wrap))` + `$.get(d)`.
    "legacy/wrap_if_elseif_call",
    // The keyed `{#each}` collection wraps inside its thunk; the key callback
    // stays raw.
    "legacy/wrap_each_collection_call",
    // The `{#await}` promise + `{#key}` expression wrap inside their thunks.
    "legacy/wrap_await_key_call",
    // The `{@html}` payload wraps inside its getter (no thunk elision across
    // a required wrap).
    "legacy/wrap_html_call",
    // The `{@const}` initializer wraps inside `$.derived_safe_equal` (the
    // non-runes helper).
    "legacy/wrap_const_call",
    // The element `{@attach}` payload wraps inside its thunk.
    "legacy/wrap_attach_call",
    // The `<title>` mixed chunk memoizes its wrapped sequence into the
    // deferred-effect deps array.
    "legacy/wrap_title_mixed_call",
    // The spread fold: co-located attr wraps + memoizes, the `style:` inner
    // value wraps inside its memoized object, the `class:` condition stays
    // raw inside its memoized object — one ordered memoizer + deps row.
    "legacy/wrap_spread_colocated_call",
    // The `<svelte:element>` fold rides the SAME item substrate: co-located
    // attr wraps + memoizes; the `style:` inner value wraps.
    "legacy/wrap_svelte_element_style_dir",
];

/// The css scoping corpus (5l) — a top-level `<style>` compiles to the scoped
/// module: the scope class bakes into the static skeleton / threads through
/// `$.set_class` and the spread `$.attribute_effect`, the scoped `css.code`
/// routes external-vs-injected, unused selectors prune, and `@keyframes`
/// rename. Every fixture asserts the FULL emitted-module structural equality
/// against the official golden (hash-masked on both sides) plus the css-field
/// parity below.
const SUPPORTED_CSS: &[&str] = &[
    // The two-injection-site agreement fixture: a STATIC `<h2>` (no class)
    // synthesizes the baked scope class while the `class:active` div threads
    // the SAME hash through `$.set_class`'s value literal.
    "css/scoped_styles",
    // NO filename ⇒ the css-hash input falls back to the css TEXT (the golden
    // css.hash pins the fallback — a filename-hash regression diverges).
    "css/scope_hash_fallback_no_filename",
    // A DYNAMIC `class={c}` on a scoped element ⇒ the hash rides the
    // `css_hash` argument (`$.set_class(button, 1, $.clsx($.get(c)),
    // 'svelte-<hash>')`), NOT the value literal.
    "css/dynamic_class_expression",
    // `:global(.x)` unwraps un-scoped; the `:global { … }` block comment-wraps
    // its wrapper; the `.card` rule still scopes.
    "css/global_selectors",
    // Child combinator + attribute operator/case-insensitivity matching
    // (`.list > li[data-kind="a" i]`, `p[title^="no"]`).
    "css/combinators_attributes",
    // `{#if}` / `{#each}` bodies: block-nested elements match through the
    // existence-probability walk; `.gone` prunes.
    "css/blocks_existence",
    // Unused-selector pruning (`.missing` comment-wraps) beside a used rule
    // and a used `:hover` variant.
    "css/unused_pruning",
    // `@keyframes` rename + `-global-` prefix strip + `animation` /
    // `animation-name` token rewrite.
    "css/keyframes_animation",
    // `<svelte:options css="injected">` ⇒ the module hoists `$$css` (the
    // MINIFIED payload) + prepends `$.append_styles`; NO external artifact.
    "css/injected_mode",
    // A SCOPED spread element ⇒ the hash rides `$.attribute_effect`'s
    // `css_hash` argument slot.
    "css/spread_scoped",
    // `<style></style>` ⇒ empty render, but the artifact STILL publishes —
    // official `compiled.css` is NON-null (`{ code: '', hasGlobal: false,
    // map }`) on BOTH backends; only the ABSENCE of a style block yields
    // `css === null`. No inject machinery.
    "css/style_empty_body",
    // An UPPERCASE `&#X20;` prefix is NOT a character reference (the official
    // numeric-entity pattern accepts a lowercase `x` only): the class value
    // stays the literal `a&#X20;b` (markup-escaped `a&amp;#X20;b`), `.b`
    // prunes as unused, and the div is NOT scoped — the two-sided
    // discriminator against `entity_class_scoped` (`&#32;` decodes → scoped).
    "css/entity_uppercase_x_not_decoded",
    // A SCOPED class-less `<svelte:element>` ⇒ the synthesized empty class
    // takes the lone-class route: `$.set_class($$element, 0, 'svelte-<hash>')`.
    "css/svelte_element_scoped",
    // A SCOPED `<svelte:element>` WITH a static attr ⇒ the fold appends the
    // synthetic `class: ''` and threads the hash as `$.attribute_effect`'s
    // official 6th positional argument.
    "css/svelte_element_scoped_attr",
    // A SCOPED spread `<svelte:element>` ⇒ NO synthetic class (the runtime
    // spread path appends the hash itself); the hash rides the 6th argument.
    "css/svelte_element_scoped_spread",
    // INJECTED minify: the `:global { … }` wrapper tokens are REMOVED outright
    // (the external artifact comment-wraps them); the body stays unscoped.
    "css/injected_global_block",
    // INJECTED minify: unused rules and empty rules are REMOVED outright
    // (never comment-wrapped).
    "css/injected_unused_empty",
    // INJECTED minify: a mixed used/unused selector LIST prunes by REMOVAL —
    // the mid-list and leading unused selectors drop with their commas.
    "css/injected_selector_list_prune",
    // INJECTED minify: local `@keyframes` rename + `-global-` strip +
    // animation token rewrite — the keyframes body and the animation
    // declaration keep their whitespace (the official minify strips only
    // NON-animation declarations and never recurses into keyframes).
    "css/injected_keyframes",
    // INJECTED minify: a custom `--` property PRESERVES its value whitespace
    // (the official Chromium `--foo: ;` caveat) while sibling declarations
    // collapse.
    "css/injected_custom_property",
    // INJECTED minify: pseudo-class ARGUMENT lists (`:is`/`:not`) recurse —
    // an unused `:is(...)` arg prunes by removal and the used arg scopes with
    // the inner `:where(.svelte-<hash>)`.
    "css/injected_nested_pseudo",
    // ENTITY-DECODED class matching: `class="a&#32;b"` decodes to the word
    // list `a b`, so `.b` retains+scopes (the div bakes the decoded
    // `class="a b svelte-<hash>"`) and the mismatching `.c` prunes — the
    // matcher and the skeleton emitter consume the SAME decoded value.
    "css/entity_class_scoped",
    // DECODE-ONCE protection: `class="a&amp;#32;b"` decodes ONCE to the single
    // token `a&#32;b` (the `&amp;` protects the `#32;` — NO space), so `.b`
    // prunes `/* (unused) */`, the div is NOT scoped, and the skeleton
    // re-escapes the decoded `&` back to `class="a&amp;#32;b"`. A consumer
    // DOUBLE-decode would yield the word list `a b` → `.b` wrongly scopes and
    // the markup bakes `a b svelte-<hash>` — byte-divergent on BOTH the
    // css.code and the template golden.
    "css/entity_amp_escaped_class_not_double_decoded",
    // ENTITY-DECODED id matching: `id="a&#45;b"` decodes to `a-b`, so `#a-b`
    // retains+scopes and `#a-z` prunes.
    "css/entity_id_scoped",
    // ENTITY-DECODED attribute-selector matching: `data-token="a&#32;b"`
    // decodes to the word list `a b`, so `[data-token~="b"]` retains+scopes
    // and `[data-token~="z"]` prunes.
    "css/entity_attr_selector_scoped",
    // INJECTED minify over the ENTITY-DECODED class: the unused `.c` rule is
    // REMOVED outright (not comment-wrapped) while `.b` scopes and the div
    // bakes the decoded `class="a b svelte-<hash>"`.
    "css/injected_entity_class_scoped",
    // JS-`\s` (Unicode) WHITESPACE in the declaration-property scan: NBSP
    // between `animation` and `:` still parses the property as `animation`
    // (the official `/[\s:]/` read stops at NBSP), so the keyframes rename
    // rewrites BOTH the `@keyframes` name and the value reference — a
    // byte-ASCII property scan fails open (renamed keyframes, un-renamed
    // reference).
    "css/nbsp_keyframes_animation",
    // JS-`\s` (Unicode) WHITESPACE closing an UNQUOTED attribute value: NBSP
    // in `[data-x=a\u{a0}b]` ends the value at `a` (official
    // `REGEX_CLOSING_BRACKET /[\s\]]/`) with `b` read as flags, so the `a`
    // rule matches `data-x="a"` (retains+scopes) and the `z` rule prunes — a
    // byte-ASCII close reads value `a\u{a0}b` and wrongly prunes both.
    "css/nbsp_attr_selector_value",
    // JS-`\s` (Unicode) WHITESPACE inside `REGEX_NTH_OF`: NBSP around the An+B
    // offset (`li:nth-child(2n\u{a0}+\u{a0}1)`) is matched by the official
    // `\s*[+-]\s*` so the rule scopes; a byte-ASCII nth scan misses the offset,
    // the reject reader falls through to the digit-leading identifier reject,
    // and the whole component is WRONGLY refused (css_expected_identifier).
    // Exercises the reject-gate + scoping path end-to-end (both CSS parsers).
    "css/nbsp_nth_child",
    // NON-ASCII in an UNQUOTED attribute-selector value (`[data-x=café]`): the
    // reject-reader + the scoping parser must step whole UTF-8 chars (a byte
    // step lands `codepoint_at` on a continuation byte → char-boundary panic).
    // svelte@5.56.3 accepts + scopes; the div retains + bakes the hash.
    "css/nonascii_attr_selector_value",
    // ── the `<slot>` outlet projection (official `SlotElement` block semantics) ──
    // A selector matching an element INSIDE the slot fallback: kept + the
    // fallback `<p>` scoped; the unused `.absent` prunes; the slot itself never
    // receives the scope hash (`<!>` anchor, no element).
    "css/slot_fallback_scoped",
    // `.outer + .inner` where `.inner` is the FIRST fallback element — the
    // sibling walk climbs OUT of the fallback fragment to the definite outer
    // sibling (kept, both scoped).
    "css/slot_adjacent_outer_before",
    // `.last + .after` where `.last` is the LAST fallback element and `.after`
    // follows the slot — the slot projects its fallback boundary candidates
    // NON-exhaustively (kept fail-open, both scoped).
    "css/slot_adjacent_fallback_last",
    // `.a + .b` with an EMPTY-fallback slot between — the walk records the slot
    // PROBABLY and keeps stepping to the definite `.a` (kept); the `.zz + .b`
    // twin still prunes.
    "css/slot_adjacent_empty_between",
    // `:global(.x) + p` over a slot sibling — the official SlotElement
    // uncertainty arm keeps the all-global remainder (the slot may render a
    // `.x`); the non-global `.x + p` twin prunes.
    "css/slot_global_sibling",
    // The INJECTED-mode variant of the fallback-scoped fixture: the same
    // matcher verdicts route through the `$$css` inline artifact.
    "css/injected_slot_fallback_scoped",
];

/// The repository root (two levels up from this crate's `tests/` dir).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// The vendored fixture source for a slug.
fn fixture_source(slug: &str) -> String {
    let path = repo_root()
        .join("crates/verter_compiler/tests/svelte_oracle_corpus/fixtures")
        .join(format!("{slug}.svelte"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {slug}: {e}"))
}

/// The committed official client golden JSON for a slug.
fn client_golden(slug: &str) -> serde_json::Value {
    let path = repo_root()
        .join("crates/verter_compiler/tests/svelte_oracle_corpus/goldens")
        .join(format!("{slug}.client.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read golden {slug}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse golden {slug}: {e}"))
}

/// The `componentNameFor` slug → component-name rule the golden generator uses
/// (so Verter compiles under the same `name`).
fn component_name_for(slug: &str) -> String {
    let stem = slug.rsplit('/').next().unwrap_or(slug);
    let sanitized: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{sanitized}")
    } else {
        sanitized
    }
}

/// The per-fixture COMPILE OPTIONS mirror of `FIXTURE_COMPILE_OPTIONS` in
/// `scripts/gen-svelte-goldens.mjs` — the few fixtures whose surface is a
/// compile option rather than in-source syntax compile under the SAME options
/// the golden was generated with.
fn compile_options_for(slug: &str) -> SvelteRuntimeOptions {
    SvelteRuntimeOptions {
        // The css-hash FALLBACK fixture compiles with NO filename (the golden
        // was generated with `filename: undefined`, so its css hash is the
        // css-TEXT fallback, not the filename hash).
        filename: (slug != "css/scope_hash_fallback_no_filename").then(|| format!("{slug}.svelte")),
        name: Some(component_name_for(slug)),
        // The `customElement: true` compile-option fixture.
        custom_element: slug == "options/custom_element_option_true",
        ..Default::default()
    }
}

/// Compile a fixture to its emitted client JS.
fn emit(slug: &str) -> String {
    let source = fixture_source(slug);
    let alloc = Allocator::default();
    let parsed = parse_svelte(&source);
    let opts = compile_options_for(slug);
    compile_client(&source, &parsed, &opts, &alloc, false, false)
        .unwrap_or_else(|e| panic!("client emission failed for {slug}: {e:?}"))
        .code
}

// ── Topology extraction (a faithful Rust port of the svelte-golden-lib concepts) ──

/// Mask every `svelte-<hash>` scope token to the golden's `svelte-<scoped>`
/// placeholder — the Rust port of the golden lib's `maskScopeHash`
/// (`/svelte-[0-9a-z]+/g`).
fn mask_scope_hash(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if text[i..].starts_with("svelte-") {
            let start = i + "svelte-".len();
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_digit() || bytes[end].is_ascii_lowercase())
            {
                end += 1;
            }
            if end > start {
                out.push_str("svelte-<scoped>");
                i = end;
                continue;
            }
        }
        let ch = text[i..].chars().next().expect("in-bounds char");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Mask the non-code regions of a JS module — string literals, template-literal
/// TEXT spans, and line/block comments — to spaces, so a `$.<helper>` scan keys on
/// real code only (a helper-shaped token inside a string/template cannot
/// false-match). Mirrors `maskNonCodeRegions` (template `${...}` interpolations
/// are kept as code).
fn mask_non_code(code: &str) -> String {
    let bytes: Vec<char> = code.chars().collect();
    let n = bytes.len();
    let mut out = bytes.clone();
    // Template-literal nesting: each frame tracks the `${}` interpolation depth.
    let mut tmpl: Vec<i32> = Vec::new();
    let mut i = 0;
    let mask = |out: &mut Vec<char>, idx: usize| {
        if out[idx] != '\n' && out[idx] != '\r' {
            out[idx] = ' ';
        }
    };
    while i < n {
        let in_tmpl_text = tmpl.last().copied() == Some(0);
        if in_tmpl_text {
            let ch = bytes[i];
            if ch == '\\' {
                mask(&mut out, i);
                if i + 1 < n {
                    mask(&mut out, i + 1);
                }
                i += 2;
                continue;
            }
            if ch == '`' {
                tmpl.pop();
                i += 1;
                continue;
            }
            if ch == '$' && i + 1 < n && bytes[i + 1] == '{' {
                *tmpl.last_mut().unwrap() = 1;
                i += 2;
                continue;
            }
            mask(&mut out, i);
            i += 1;
            continue;
        }
        let ch = bytes[i];
        let next = if i + 1 < n { bytes[i + 1] } else { '\0' };
        if ch == '/' && next == '/' {
            while i < n && bytes[i] != '\n' {
                mask(&mut out, i);
                i += 1;
            }
            continue;
        }
        if ch == '/' && next == '*' {
            mask(&mut out, i);
            mask(&mut out, i + 1);
            i += 2;
            while i < n && !(bytes[i] == '*' && i + 1 < n && bytes[i + 1] == '/') {
                mask(&mut out, i);
                i += 1;
            }
            if i < n {
                mask(&mut out, i);
                mask(&mut out, i + 1);
                i += 2;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            let quote = ch;
            i += 1;
            while i < n && bytes[i] != quote {
                if bytes[i] == '\\' {
                    mask(&mut out, i);
                    if i + 1 < n {
                        mask(&mut out, i + 1);
                    }
                    i += 2;
                    continue;
                }
                mask(&mut out, i);
                i += 1;
            }
            if i < n {
                i += 1;
            }
            continue;
        }
        if ch == '`' {
            tmpl.push(0);
            i += 1;
            continue;
        }
        if let Some(depth) = tmpl.last_mut() {
            if *depth > 0 {
                if ch == '{' {
                    *depth += 1;
                    i += 1;
                    continue;
                }
                if ch == '}' {
                    *depth -= 1;
                    i += 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// The ORDERED `$.<helper>` reference sequence over the code-only view.
fn helper_sequence(code: &str) -> Vec<String> {
    let masked = mask_non_code(code);
    let mut seq = Vec::new();
    let chars: Vec<char> = masked.chars().collect();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '$' && chars[i + 1] == '.' {
            let mut j = i + 2;
            let mut name = String::new();
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                name.push(chars[j]);
                j += 1;
            }
            if !name.is_empty() {
                seq.push(name);
                i = j;
                continue;
            }
        }
        i += 1;
    }
    seq
}

/// The committed golden's `helperSequence` as a `Vec<String>`.
fn golden_sequence(golden: &serde_json::Value) -> Vec<String> {
    golden["helperSequence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// The golden's `delegatedEvents`.
fn golden_delegated(golden: &serde_json::Value) -> Vec<String> {
    golden["delegatedEvents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// The emitted module's delegated event set (the `$.delegate([...])` literals).
fn emitted_delegated(code: &str) -> Vec<String> {
    let Some(start) = code.find("$.delegate([") else {
        return Vec::new();
    };
    let body = &code[start + "$.delegate([".len()..];
    let Some(end) = body.find(']') else {
        return Vec::new();
    };
    body[..end]
        .split(',')
        .filter_map(|s| {
            let t = s.trim().trim_matches(|c| c == '\'' || c == '"');
            (!t.is_empty()).then(|| t.to_string())
        })
        .collect()
}

/// The emitted `from_html` template literals + fragment flags, as `(html, flag)`.
fn emitted_templates(code: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut search = code;
    while let Some(idx) = search.find("$.from_html(`") {
        let after = &search[idx + "$.from_html(`".len()..];
        let Some(close) = after.find('`') else { break };
        let html = after[..close].to_string();
        // The trailing flag (if any): between the closing backtick and the `)`.
        let rest = &after[close + 1..];
        let flag = rest
            .strip_prefix(", ")
            .and_then(|r| r.split(')').next())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.starts_with(')'));
        out.push((html, flag));
        search = rest;
    }
    out
}

/// The golden's `templates` as `(html, flag)`.
fn golden_templates(golden: &serde_json::Value) -> Vec<(String, Option<String>)> {
    golden["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["html"].as_str().unwrap().to_string(),
                t["flag"].as_str().map(|s| s.to_string()),
            )
        })
        .collect()
}

/// The emitted module's default-export fn name + param list.
fn emitted_export(code: &str) -> (String, Vec<String>) {
    let marker = "export default function ";
    let idx = code.find(marker).expect("an export default function");
    let after = &code[idx + marker.len()..];
    let name = after.split('(').next().unwrap().trim().to_string();
    let params_str = after.split('(').nth(1).unwrap().split(')').next().unwrap();
    let params = params_str
        .split(',')
        .map(|p| p.split('=').next().unwrap().trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    (name, params)
}

/// The golden's `exportDefault` name + params.
fn golden_export(golden: &serde_json::Value) -> (String, Vec<String>) {
    let name = golden["exportDefault"]["name"]
        .as_str()
        .unwrap()
        .to_string();
    let params = golden["exportDefault"]["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    (name, params)
}

/// Normalize a FULL JS module for the emitted-JS equivalence comparison — the
/// Rust port of `scripts/svelte-golden-lib.mjs::normalizeModuleForComparison`.
///
/// Collapses cosmetic whitespace OUTSIDE string/template/HTML literals (so a
/// tabs-vs-spaces / line-wrap / blank-line reflow does not false-fail), while
/// preserving whitespace INSIDE string / template-literal literals BYTE-EXACT (so
/// `$$props.bar` vs `.foo`, raw `count` vs `$.get(count)`, a dropped `$.child(_,
/// true)` arg, a sibling-offset drift, or significant template TEXT whitespace
/// still fails). Comments are dropped. This is the FIDELITY the helper-name
/// sequence misses; it MUST stay byte-equivalent to the JS lib so the committed
/// `clientModule` (produced by the lib) and Verter's normalized output compare.
fn normalize_module_for_comparison(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    // Template-literal frames: each tracks the `${}` interpolation depth (0 = in
    // template TEXT).
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
            // Template TEXT — copied verbatim (significant DOM whitespace).
            out.push(ch);
            i += 1;
            continue;
        }
        let ch = chars[i];
        let next = if i + 1 < n { chars[i + 1] } else { '\0' };
        if ch == '/' && next == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && next == '*' {
            i += 2;
            while i < n && !(chars[i] == '*' && i + 1 < n && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if ch == '\'' || ch == '"' {
            let quote = ch;
            out.push(ch);
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
                out.push(chars[i]);
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
        if ch.is_whitespace() {
            while i < n && chars[i].is_whitespace() {
                i += 1;
            }
            // Suppress a collapsed space that HUGS a bracket: a space right after an
            // opener (`(` `[` `{`) or right before a closer (`)` `]` `}`). This makes
            // the official multi-line call wrap (`$.template_effect(\n\t($0) => …\n)`
            // → `$.template_effect( ($0) => … )`) byte-comparable with the single-line
            // form (`$.template_effect(($0) => …)`). Symmetric (both sides) and
            // cosmetic-only: a token difference INSIDE the brackets still fails, and
            // string/template literals are copied verbatim above.
            let prev_is_opener = matches!(out.chars().last(), Some('(') | Some('[') | Some('{'));
            let next_is_closer = matches!(chars.get(i), Some(')') | Some(']') | Some('}'));
            if prev_is_opener || next_is_closer {
                continue;
            }
            out.push(' ');
            continue;
        }
        out.push(ch);
        i += 1;
    }
    // Trim the leading/trailing space the outside-literal collapse may have left.
    // (A `split_whitespace`-style collapse here would DESTROY the whitespace inside
    // string / template literals the per-char loop deliberately preserved.)
    out.trim().to_string()
}

/// The golden's `clientModule` (the normalized official full-module oracle).
fn golden_client_module(golden: &serde_json::Value) -> String {
    golden["clientModule"]
        .as_str()
        .expect("a client golden carries `clientModule`")
        .to_string()
}

/// Whether the emitted module carries every side-effect import (`import 'src';` —
/// the disclose-version / flag imports plus user side-effect imports) and every
/// namespace import (`import * as LOCAL from 'src';` — the runtime `$` plus user
/// namespace imports, each under the golden's recorded LOCAL name) the golden's
/// import topology records. Named / default user imports are covered by the
/// full-module AST-structural comparison (which signs the whole `ImportDeclaration`
/// family, `with`-clause included).
fn emitted_imports_ok(code: &str, golden: &serde_json::Value) -> bool {
    let imports = golden["imports"].as_array().unwrap();
    imports.iter().all(|imp| {
        let source = imp["source"].as_str().unwrap();
        match imp["kind"].as_str().unwrap() {
            "sideEffect" => code.contains(&format!("import '{source}';")),
            "namespace" => {
                let local = imp["names"]
                    .as_array()
                    .and_then(|names| names.first())
                    .and_then(|name| name.as_str())
                    .unwrap_or("$");
                code.contains(&format!("import * as {local} from '{source}';"))
            }
            _ => true,
        }
    })
}

/// Whether `code` parses as a valid JS module through OXC (no panic, no syntax
/// errors). A guard against an emitted module that is structurally valid topology
/// but syntactically broken JS (a stray `export` inside a fn, an unbalanced wrap).
fn parses_as_js(code: &str) -> bool {
    let alloc = Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let ret = oxc_parser::Parser::new(&alloc, code, source_type).parse();
    !ret.panicked && ret.errors.is_empty()
}

// ─────────────────────────────────────────────────────────────────────────────
// AST-level paren-INSENSITIVE structural module comparison.
//
// The value emitter is SOURCE-PRESERVING: it keeps the author's redundant parens that the
// official AST printer drops (`() => ((a, b))` vs `() => (a, b)`, `id: (c ? a : b)` vs
// `id: c ? a : b`). Those are behavior-preserving COSMETIC differences the minifier collapses,
// so they must NOT fail the convergence gate. But every BEHAVIORAL / structural difference —
// a changed helper name, a changed call ARGUMENT COUNT, a `SequenceExpression` split into
// separate arguments, a changed string / template literal content, a changed operator, a
// changed identifier — MUST still fail.
//
// The comparator parses BOTH modules with OXC and compares a canonical STRUCTURAL SIGNATURE
// that transparently UNWRAPS every `ParenthesizedExpression` on BOTH sides (so `(X)` ≡ `X` at
// every position) while encoding everything else faithfully: statement kinds + order,
// declaration kinds + binding names, call callee + per-argument structure (so a sequence as
// ONE argument is distinct from two separate arguments — unwrapping parens never merges
// them), member access (object + property + computed), operators, string/template CONTENTS
// (byte-exact), object/array element structure, and identifier names. Whitespace is irrelevant
// at the AST level. The signature is path-deterministic, so a paren-only diff yields IDENTICAL
// signatures and any structural diff yields a divergence the assertion reports.
// ─────────────────────────────────────────────────────────────────────────────

use oxc_ast::ast::{
    Argument, ArrayExpressionElement, BindingPattern, Class, ClassElement, Declaration, Decorator,
    Directive, Expression, ForStatementInit, ForStatementLeft, FormalParameters, FunctionBody,
    ImportDeclarationSpecifier as IDS, ModuleExportName as MEN, ObjectPropertyKind, PropertyKey,
    Statement, SwitchCase, VariableDeclarationKind,
};

/// Peel every transparent `ParenthesizedExpression` wrapper, returning the inner node.
fn unwrap_parens<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    let mut e = expr;
    while let Expression::ParenthesizedExpression(p) = e {
        e = &p.expression;
    }
    e
}

/// Strip the position NOISE (`span: Span { … }`, `node_id: Cell { … }`, `reference_id`,
/// `scope_id`, `symbol_id`) from an OXC `{:?}` Debug rendering, so a conservative Debug
/// fallback compares two STRUCTURALLY-identical nodes at different byte offsets as EQUAL
/// (the fallback must not false-fail on span drift). The non-noise structure (operators,
/// names, literal values, nesting) survives, so a genuine structural difference still differs.
fn strip_debug_noise(debug: &str) -> String {
    let mut out = String::with_capacity(debug.len());
    let bytes = debug.as_bytes();
    let mut i = 0;
    // The noise keys whose `Key { … }` / `Key: <scalar>` payload is dropped.
    const BRACED: [&str; 2] = ["span: Span {", "node_id: Cell {"];
    const SCALARS: [&str; 4] = [
        "reference_id: Cell {",
        "scope_id: Cell {",
        "symbol_id: Cell {",
        "flags: ReferenceFlags",
    ];
    'outer: while i < bytes.len() {
        for key in BRACED.iter().chain(SCALARS.iter()) {
            if debug[i..].starts_with(key) {
                // Skip the balanced `{ … }` that opens at the last `{` in the key.
                let mut depth = 0i32;
                let mut j = i;
                // Advance to the first `{` of this key's brace group.
                while j < bytes.len() && bytes[j] != b'{' {
                    j += 1;
                }
                while j < bytes.len() {
                    match bytes[j] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                j += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                // Drop a trailing `, ` separator after the removed field.
                while j < bytes.len() && (bytes[j] == b',' || bytes[j] == b' ') {
                    j += 1;
                }
                i = j;
                continue 'outer;
            }
        }
        let ch = debug[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

// COMPARATOR SCOPE — the `program_sig` / `expr_sig` / `params_sig` / `binding_sig` / `stmt_sig` /
// `function_body_sig` family encodes every expression / parameter / binding / object-property /
// statement axis a source-preserved author expression or function body can carry: param ORDER and
// param DEFAULTS (`FormalParameter.initializer`), recursive destructuring binding defaults
// (`binding_sig`'s `ObjectPattern`/`ArrayPattern`/`AssignmentPattern` arms route each default through
// the paren-transparent `expr_sig`), function `r#async` / `generator` bits, the full object-property
// shape (`ObjectProperty.{kind,method,computed,shorthand}` plus key + value), optional-chain flags,
// member/computed shape, call-arg spread, literal forms, the full per-specifier import KIND /
// imported-name / local-name PLUS import `phase` + `with`-clause attributes, the full
// `ExportNamedDeclaration` surface (inline declaration / specifiers / re-export source / export-kind /
// `with`-clause), `ExportAllDeclaration` (source / namespace rename / export-kind / `with`-clause),
// the program `hashbang`, the strict-mode-reachable ordinary-JS statement set (control-flow: `for` /
// `for-in` / `for-of` / `while` / `do-while` / `switch` / `try`/`catch`/`finally` / `throw` /
// `break` / `continue` / `labeled` / `debugger` / `empty` / class declarations; a `WithStatement` arm
// is also encoded but `with` is INVALID under `SourceType::mjs()` (module/strict), so that arm is
// unreachable for emitted mjs and DEFENSIVE-ONLY — it proves nothing about emitted output), the
// `FunctionBody.directives` AND `Program.directives` directive prologues, a bounded class skeleton
// (`class_sig` / `class_element_sig` — a method signs the COMPLETE runtime shape
// kind + static + computed + ASYNC + GENERATOR + key + PARAMS + body; class-level AND per-member
// decorators are ENCODED via `decorators_sig`, see `class_sig`), and the client-reachable EXPRESSION
// residuals (`ClassExpression` → `class_sig`, `YieldExpression`, dynamic `ImportExpression`,
// `MetaProperty` — `import.meta` / `new.target`, `PrivateInExpression`, `Super`). The
// in-contract SEMANTIC-COMMENT boundary is ENFORCED on RAW module pairs by the semantic-comment
// signature layered alongside the AST signature (`conformance_sig` → `comment_sig`): tool-consumed /
// framework-significant comments — PURE-family (`/*@__PURE__*/`), license/preserve (`/*! … */`),
// source-map (`//# sourceURL=` / `//# sourceMappingURL=` and the legacy `//@ …` forms), TS-directives
// (triple-slash references, `@ts-check`/`@ts-nocheck`/`@ts-ignore`/`@ts-expect-error`), and JSDoc —
// are compared as an OCCURRENCE-PATH multiset (each comment keyed by its deterministic AST
// occurrence path + `pos=Leading|Trailing`, so a drop / corruption / MOVE — even between
// structurally-identical positions — FAILS), while NON-SEMANTIC comments (`// note`, `/* note */`,
// unknown `@foo`) remain a WAIVED cosmetic axis (dropped from the signature). The occurrence path is
// produced by a GENERIC OXC child-span anchor walker (`CommentAnchorIndex`, an `oxc_ast_visit::Visit`
// impl) and is deterministic and collision-resistant over the NORMALIZED comparator view
// (`CommentAnchorIndex` indexes statements by the same empty-filtered logical view as
// `statements_sig`, node-types the segments, and gives comments attached to normalized-away empty
// statements an explicit synthetic `empty_gap[<logical>.<empty_ordinal>]` anchor); a future anchor
// collapse inside that normalized view is a comparator bug. `CommentAnchorIndex` walks top-level
// `Program.directives` and `program.body`; descendants are reached through the generic
// `oxc_ast_visit::Visit` walker, including `FunctionBody.directives` and every nested statement list
// (the `visit_statements` override applies the same empty-filtered normalization there). A future
// semantic-comment anchor collapse inside those walked nodes is a comparator bug, not D-17 debt.
//
// GOLDEN-SIDE ORACLE CAVEAT — the `comment_sig` enforcement above is proven on RAW inputs by the
// discrimination guard `svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`,
// but the COMMITTED FIXTURE goldens (`*.client.json`) serialize `clientModule` through
// `scripts/svelte-golden-lib.mjs::normalizeModuleForComparison`, which DROPS every JS comment. So at
// the two fixture gate sites (`emitted_client_topology_matches_official_goldens`,
// `emitted_codegen_corpus_matches_official_goldens`) the GOLDEN side's `comment_sig` is ALWAYS
// EMPTY — the fixture gate does NOT yet prove official-POSITIVE semantic-comment preservation (an
// official semantic comment Verter dropped would compare EQUAL because the golden was stripped at
// generation). This is a golden-DATA oracle gap, NOT a comparator-logic gap. (Tracked:
// svelte-native-compiler-plan.md §8 D-19.)
//
// GOLDEN-SIDE REGEX-LITERAL NOTE — the same `normalizeModuleForComparison` (and its Rust mirror
// `normalize_module_for_comparison`) is NOT a JS lexer and would mangle whitespace / `//` inside a
// code-position REGEX LITERAL baked into a committed golden. The comparator-side `RegExpLiteral.raw`
// axis (see `expr_sig`'s `Expression::RegExpLiteral` arm) is CORRECT and in-contract; the gap is
// purely golden-DATA. The committed corpus currently contains ZERO code-position regex literals, so
// there is no current false-pass; the invariant is PINNED by
// `svelte_goldens_in_sync::committed_client_goldens_carry_no_code_position_regex_literal` (it FAILS
// if a future golden introduces one, forcing the lexer-backed normalizer fix at that point).
// (Tracked: svelte-native-compiler-plan.md §8 D-19.)
//
// REACHABLE axes NOW ENCODED above (each reachable through a source-preserved author expression, so
// dropping them was a silent structural false-PASS risk, not an unreachable surface): the
// object-property axes (`ObjectProperty.{kind,method,computed,shorthand}`), parameter defaults
// (`FormalParameter.initializer`), function `r#async` / `generator` bits, named-export function
// BODIES (`decl_sig`'s `Declaration::FunctionDeclaration` arm signs `body={function_body_sig(...)}`,
// matching the `Statement::FunctionDeclaration` and `ExportDefaultFn` arms), recursive destructuring
// binding defaults via `binding_sig` (the `ObjectPattern`/`ArrayPattern`/`AssignmentPattern` arms
// route each default through the paren-transparent `expr_sig`, and `assignment_target_sig` does the
// same for assignment-EXPRESSION destructuring targets), the strict-mode-reachable ordinary-JS
// statement set (`for` / `for-in` / `for-of` / `while` / `do-while` / `switch` /
// `try`/`catch`/`finally` / `throw` / `break` / `continue` / `labeled` / `debugger` / `empty` / class
// declarations; the `WithStatement` arm is encoded but `with` is invalid under `SourceType::mjs()` so
// it is unreachable/defensive-only), the module import/export ORACLE family (import `phase` +
// `with`-clause attributes, the full `ExportNamedDeclaration` surface, `ExportAllDeclaration`, the
// program `hashbang`),
// the `FunctionBody.directives` AND `Program.directives` directive prologues (via `function_body_sig`
// / `program_sig`), a BOUNDED class skeleton (`class_sig` / `class_element_sig` — a method signs the
// COMPLETE runtime shape kind + static + computed + ASYNC + GENERATOR + key + PARAMS + body, each
// sub-part reducing to an existing terminal helper; the TS-only method axes are stripped before emit
// and class-level AND per-member decorators are ENCODED via `decorators_sig`), and the
// client-reachable EXPRESSION residuals (`ClassExpression` → the same `class_sig`;
// `YieldExpression` — `delegate` + arg;
// `ImportExpression` — dynamic `import()` source/options/phase; `MetaProperty` — `import.meta` /
// `new.target`; `PrivateInExpression` — `#x in obj`; `Super`). Every new statement / expression
// sub-part routes through an existing terminal helper (`expr_sig` / `binding_sig` / `params_sig` /
// `class_sig` / `assignment_target_sig` / `decl_var_sig` / `statements_sig` / `function_body_sig`);
// the only NEW primitive leaves are directive text and labels. A stray no-op `EmptyStatement` (`;`)
// in a statement LIST is filtered by `statements_sig` (printer-dropped cosmetic no-op) AND the
// comment-anchor index mirrors that filter (a filtered empty gets a synthetic
// `empty_gap[<logical>.<empty_ordinal>]` anchor — see
// `CommentAnchorIndex::normalize_statement_list`); an `EmptyStatement` in a REQUIRED child
// position (loop/if/with/labeled body) stays signed via `stmt_sig` (behavior-bearing) and is never
// filtered. `class_sig` encodes `Class.r#type`, decorators, id, super-class, and the runtime-bearing
// members (method kind/static/computed/async/generator/params/body, property/accessor key+value,
// static blocks) for `SourceType::mjs()`-parseable emitted classes; TS-only member axes
// (abstract/accessor-type/index-signature) are stripped pre-emit and ignored.
//
// MODULE IMPORT/EXPORT is an ORACLE family, NOW ENCODED (NOT a residual): OFFICIAL Svelte client
// output CAN contain module-script imports/exports — the `matrix/module_import_export` golden carries
// a `clientModule` with `import {base} from "./base.js"; … export const VERSION = 1;` — so the
// comparator (the gate's oracle) MUST compare these forms correctly even though native Verter
// currently REFUSES `<script module>` in this branch (so `module_import_export` is NOT yet a gated
// `SUPPORTED_MATRIX` slug; it enters the gate only when native module-script emission opens). The
// encoded axes: `ImportDeclaration` `phase` + `with`-clause attributes; the full
// `ExportNamedDeclaration` surface (inline declaration / specifier list / re-export source /
// export-kind / `with`-clause), including specifier-only `export { a as x } from "m"`;
// `ExportAllDeclaration` (source / namespace rename / export-kind / `with`-clause); the program
// `hashbang`.
//
// REMAINING structural gaps (honest residual): genuinely TS/module-only declaration/statement forms
// that are NOT parseable under `SourceType::mjs()` (e.g. `TSTypeAliasDeclaration`,
// `TSInterfaceDeclaration`, `TSEnumDeclaration`, `TSModuleDeclaration`, `TSImportEqualsDeclaration`,
// `TSExportAssignment`) collapse to the discriminant-only `Stmt(discriminant)` / `Decl(discriminant)`
// fallback (restricted to TS/module-only forms, NOT ordinary control-flow and NOT the import/export
// family). The separate golden-DATA semantic-comment gap (the fixture goldens are comment-stripped at
// generation) is tracked as D-19, NOT here. The class skeleton, the client-reachable EXPRESSION
// surface, and the module import/export family are NO LONGER residual (all encoded). The expression
// `other =>` fallback is an EXPLICITLY-CLASSIFIED conservative fallback over only the TS-only wrappers
// / JSX / V8-intrinsic forms — none parseable under `SourceType::mjs()` — NOT a generic
// remaining-kind catch-all. DECORATORS are ENCODED, NOT a residual: OXC parses
// ECMAScript decorators under `SourceType::mjs()` (verified — the parse succeeds with errors=0), the runtime
// strips TS but NOT decorators, and a decorated class/member in a source-preserved `{@html}`/dynamic
// value is byte-copied to emitted client JS, so `class_sig` (class-level) and each `class_element_sig`
// member arm (per-member) sign `decorators` via `decorators_sig` through the paren-transparent
// `expr_sig`. The always-emitted import axis is independently pinned by `emitted_imports_ok` at both
// gate sites.
//
// BASIS: the comparator is the gate's ORACLE, so it must compare ANY official output correctly — it
// is NOT scoped to what native Verter currently emits. The module import/export family is encoded for
// exactly this reason (official client output can carry module-script imports/exports per the
// `module_import_export` golden), even though native Verter refuses `<script module>` in this branch.
// The ONLY remaining discriminant-only collapse is the genuinely TS/module-only declaration/statement
// set that does not parse under `SourceType::mjs()`; if a source-preserved construct outside that set
// reached emitted client output and the comparator dropped its axis, two bodies that differ only
// below the gap would compare equal — a silent structural false-PASS. The semantic-comment anchor is
// deterministic and collision-resistant over the NORMALIZED comparator view (`CommentAnchorIndex`
// indexes statements by the same empty-filtered logical view as `statements_sig`, node-types the
// segments, and gives comments attached to normalized-away empty statements an explicit synthetic
// `empty_gap[<logical>.<empty_ordinal>]` anchor): `CommentAnchorIndex` walks top-level
// `Program.directives` and `program.body` and every nested statement list; descendants are reached
// through the generic `oxc_ast_visit::Visit` walker, including `FunctionBody.directives`. A future
// anchor collapse inside that normalized view is a comparator bug, not D-17 debt.
//
// CONTRACT: the owner change that makes any listed residual family accepted-positive must either
// prove it is refused with explicit fail-closed tests, OR encode that family structurally in the SAME
// change, reusing `program_sig` / `expr_sig` / `params_sig` / `binding_sig` / `decl_var_sig` /
// `statements_sig` / `function_body_sig` for nested structure and adding a discriminator test per
// newly covered family. (Tracked: svelte-native-compiler-plan.md §8 D-17 — this comment is the mirror
// that row requires; it must NOT
// claim any remaining axis is unreachable.)
//
/// The canonical paren-insensitive STRUCTURAL signature of an expression. Two expressions that
/// differ ONLY by redundant parens produce the SAME signature; any other structural difference
/// (operator / literal content / identifier / member / call-arg count or per-arg shape)
/// produces a DIFFERENT signature.
fn expr_sig(expr: &Expression) -> String {
    let expr = unwrap_parens(expr);
    match expr {
        // A paren wrapper is already peeled above; this arm is unreachable but keeps the match
        // total for the variant.
        Expression::ParenthesizedExpression(p) => expr_sig(&p.expression),
        Expression::Identifier(id) => format!("Id({})", id.name),
        Expression::StringLiteral(s) => format!("Str({:?})", s.value),
        Expression::NumericLiteral(n) => {
            format!("Num({})", n.raw.as_ref().map(|r| r.as_str()).unwrap_or(""))
        }
        Expression::BigIntLiteral(b) => format!(
            "BigInt({})",
            b.raw.as_ref().map(|r| r.as_str()).unwrap_or("")
        ),
        Expression::BooleanLiteral(b) => format!("Bool({})", b.value),
        Expression::NullLiteral(_) => "Null".to_string(),
        Expression::RegExpLiteral(r) => format!(
            "RegExp({})",
            r.raw.as_ref().map(|r| r.as_str()).unwrap_or("")
        ),
        Expression::TemplateLiteral(t) => {
            // An UNTAGGED template's COOKED value is its RUNTIME string — the escape representation in
            // the raw text is a cosmetic carrier (exactly like a `StringLiteral`, which signs cooked
            // `.value`, and the directive cooked-value treatment). So sign the COOKED value per quasi;
            // `cooked` is `None` only for an UNCOOKABLE escape (lone surrogate / bad escape), a
            // distinct behavior-relevant case — fall back to a raw marker there so two
            // differently-uncookable templates stay distinct. The interpolated `${…}` expressions are
            // signed through the paren-transparent `expr_sig`. Both the cooked literal value and the
            // interpolated expressions are significant.
            let quasis: Vec<String> = t
                .quasis
                .iter()
                .map(|q| match &q.value.cooked {
                    Some(cooked) => format!("{:?}", cooked.as_str()),
                    None => format!("raw:{:?}", q.value.raw.as_str()),
                })
                .collect();
            let exprs: Vec<String> = t.expressions.iter().map(expr_sig).collect();
            format!(
                "Tmpl(quasis=[{}],exprs=[{}])",
                quasis.join(","),
                exprs.join(",")
            )
        }
        Expression::StaticMemberExpression(m) => {
            // `m.optional` is SEMANTICS-BEARING: `a?.b` short-circuits to `undefined` when `a`
            // is nullish, `a.b` throws — so the signature MUST distinguish them (a redundant
            // paren is cosmetic, an optional-chain `?` is not).
            format!(
                "Member({}.{}{})",
                expr_sig(&m.object),
                if m.optional { "?" } else { "" },
                m.property.name
            )
        }
        Expression::ComputedMemberExpression(m) => {
            format!(
                "Computed({}{}[{}])",
                expr_sig(&m.object),
                if m.optional { "?" } else { "" },
                expr_sig(&m.expression)
            )
        }
        Expression::PrivateFieldExpression(m) => {
            format!(
                "Private({}.{}#{})",
                expr_sig(&m.object),
                if m.optional { "?" } else { "" },
                m.field.name
            )
        }
        Expression::CallExpression(c) => {
            format!(
                "Call(callee={},optional={},args=[{}])",
                expr_sig(&c.callee),
                c.optional,
                arguments_sig(&c.arguments),
            )
        }
        Expression::NewExpression(c) => {
            format!(
                "New(callee={},args=[{}])",
                expr_sig(&c.callee),
                arguments_sig(&c.arguments)
            )
        }
        Expression::BinaryExpression(b) => {
            format!(
                "Bin({} {} {})",
                expr_sig(&b.left),
                b.operator.as_str(),
                expr_sig(&b.right)
            )
        }
        Expression::LogicalExpression(l) => {
            format!(
                "Logic({} {} {})",
                expr_sig(&l.left),
                l.operator.as_str(),
                expr_sig(&l.right)
            )
        }
        Expression::UnaryExpression(u) => {
            format!("Unary({}{})", u.operator.as_str(), expr_sig(&u.argument))
        }
        Expression::UpdateExpression(u) => {
            // The argument is a SimpleAssignmentTarget; render it via its source-free shape.
            format!(
                "Update(prefix={},{}{})",
                u.prefix,
                u.operator.as_str(),
                simple_target_sig(&u.argument)
            )
        }
        Expression::ConditionalExpression(c) => {
            format!(
                "Cond({} ? {} : {})",
                expr_sig(&c.test),
                expr_sig(&c.consequent),
                expr_sig(&c.alternate)
            )
        }
        Expression::SequenceExpression(s) => {
            // EACH element is a distinct signature entry — a sequence is NEVER merged with a
            // surrounding argument list (this is what catches the arg-split regression).
            let parts: Vec<String> = s.expressions.iter().map(expr_sig).collect();
            format!("Seq([{}])", parts.join(","))
        }
        Expression::AssignmentExpression(a) => {
            format!(
                "Assign({} {} {})",
                assignment_target_sig(&a.left),
                a.operator.as_str(),
                expr_sig(&a.right)
            )
        }
        Expression::ArrayExpression(arr) => {
            let parts: Vec<String> = arr
                .elements
                .iter()
                .map(|el| match el {
                    ArrayExpressionElement::SpreadElement(s) => {
                        format!("Spread({})", expr_sig(&s.argument))
                    }
                    ArrayExpressionElement::Elision(_) => "Hole".to_string(),
                    other => other
                        .as_expression()
                        .map(expr_sig)
                        .unwrap_or_else(|| "?".to_string()),
                })
                .collect();
            format!("Arr([{}])", parts.join(","))
        }
        Expression::ObjectExpression(obj) => {
            let parts: Vec<String> = obj
                .properties
                .iter()
                .map(|p| match p {
                    ObjectPropertyKind::ObjectProperty(op) => {
                        // Encode EVERY behavior-bearing object-property axis, not just key:value.
                        // `kind` (init/get/set), `method`, `computed`, and `shorthand` are all
                        // SEMANTIC: a getter `{ get x(){} }`, a method `{ x(){} }`, and a value
                        // `{ x: () => {} }` are distinct runtime shapes; `shorthand` is not cosmetic
                        // (`{ __proto__ }` shorthand vs `{ __proto__: __proto__ }` differ in proto
                        // semantics). Object literals are source-preserved (an author object inside a
                        // `{@html}` / dynamic-attr / class/style value is byte-copied by the emitter),
                        // so all four axes are reachable.
                        format!(
                            "prop(kind={:?},method={},computed={},shorthand={},key={},value={})",
                            op.kind,
                            op.method,
                            op.computed,
                            op.shorthand,
                            property_key_sig(&op.key),
                            expr_sig(&op.value)
                        )
                    }
                    ObjectPropertyKind::SpreadProperty(sp) => {
                        format!("...{}", expr_sig(&sp.argument))
                    }
                })
                .collect();
            format!("Obj({{{}}})", parts.join(","))
        }
        Expression::ArrowFunctionExpression(a) => {
            let params = params_sig(&a.params);
            // The arrow body: either a single expression (an `() => EXPR`) or a block of
            // statements. Both forms are encoded so a body shape change is caught.
            let body = if a.expression {
                // An expression body is one ExpressionStatement in the function body (no directive
                // prologue is possible in an expression-body arrow).
                a.body
                    .statements
                    .first()
                    .map(|s| stmt_sig(s))
                    .unwrap_or_else(|| "<empty>".to_string())
            } else {
                // A block-body arrow can carry a `FunctionBody.directives` prologue, so sign the full
                // function body (ordered directives + ordered statements).
                function_body_sig(&a.body)
            };
            // `r#async` is SEMANTIC (`async () => 1` returns a Promise; `() => 1` returns `1`).
            // Arrows can never be generators, so only the async bit applies here. Reachable via
            // source-preserved function literals in dynamic values.
            format!(
                "Arrow(async={},params={},expr={},body={})",
                a.r#async, params, a.expression, body
            )
        }
        Expression::FunctionExpression(f) => {
            // `r#async` and `generator` are both SEMANTIC (`async function(){}` returns a Promise;
            // `function*(){}` returns an iterator). Reachable via source-preserved function literals.
            format!(
                "Fn(async={},generator={},name={},params={},body={})",
                f.r#async,
                f.generator,
                f.id.as_ref().map(|i| i.name.as_str()).unwrap_or(""),
                params_sig(&f.params),
                f.body
                    .as_ref()
                    .map(|b| function_body_sig(b))
                    .unwrap_or_default()
            )
        }
        Expression::ThisExpression(_) => "This".to_string(),
        Expression::TaggedTemplateExpression(t) => {
            // `tag`fragment`…`` — the tag callee + the template quasis and interpolated expressions.
            // A TAGGED template's tag function observes BOTH `strings` (cooked) and `strings.raw`, so
            // for a tagged template the RAW escape representation IS in-contract (unlike an untagged
            // template, whose raw is cosmetic) — sign BOTH `raw` and `cooked` per quasi so a raw-only
            // OR a cooked-only difference is caught. (`cooked` is `None` for an uncookable escape — the
            // tag sees `undefined` for that cooked slot, a distinct case, marked `<none>`.) Span-FREE so
            // the same tagged template at different byte offsets compares EQUAL.
            let quasis: Vec<String> = t
                .quasi
                .quasis
                .iter()
                .map(|q| {
                    let cooked = match &q.value.cooked {
                        Some(c) => format!("{:?}", c.as_str()),
                        None => "<none>".to_string(),
                    };
                    format!("raw:{:?}/cooked:{cooked}", q.value.raw.as_str())
                })
                .collect();
            let exprs: Vec<String> = t.quasi.expressions.iter().map(expr_sig).collect();
            format!(
                "Tagged(tag={},quasis=[{}],exprs=[{}])",
                expr_sig(&t.tag),
                quasis.join(","),
                exprs.join(",")
            )
        }
        Expression::AwaitExpression(a) => format!("Await({})", expr_sig(&a.argument)),
        Expression::ChainExpression(c) => match &c.expression {
            oxc_ast::ast::ChainElement::CallExpression(call) => format!(
                "Chain(Call(callee={},optional={},args=[{}]))",
                expr_sig(&call.callee),
                call.optional,
                arguments_sig(&call.arguments)
            ),
            // The TOP element of the chain carries the OUTER optional bit (`a?.b.c` vs
            // `a?.b?.c` differ ONLY here — the top member's `optional`), which is
            // semantics-bearing and MUST be encoded; inner members recurse through `expr_sig`
            // (the top-level member arms, which also encode `optional`).
            oxc_ast::ast::ChainElement::StaticMemberExpression(m) => {
                format!(
                    "Chain(Member({}.{}{}))",
                    expr_sig(&m.object),
                    if m.optional { "?" } else { "" },
                    m.property.name
                )
            }
            oxc_ast::ast::ChainElement::ComputedMemberExpression(m) => {
                format!(
                    "Chain(Computed({}{}[{}]))",
                    expr_sig(&m.object),
                    if m.optional { "?" } else { "" },
                    expr_sig(&m.expression)
                )
            }
            oxc_ast::ast::ChainElement::PrivateFieldExpression(m) => {
                format!(
                    "Chain(Private({}.{}#{}))",
                    expr_sig(&m.object),
                    if m.optional { "?" } else { "" },
                    m.field.name
                )
            }
            other => format!("Chain(?{:?})", std::mem::discriminant(other)),
        },
        // ── Client-reachable expression forms reached through a source-preserved `{@html}`/
        // dynamic-value function body the value emitter byte-copies. Each routes its sub-parts
        // through an existing terminal helper (`class_sig` / `expr_sig`), so it is paren-transparent
        // and FAILS on any in-contract structural difference. The pre-fix `other =>` Debug fallback
        // collapsed these — a false-PASS (a dropped structural axis Debug does not print) AND a
        // false-FAIL (a cosmetic paren Debug DOES print) risk this closes.
        //
        // A class EXPRESSION routes through the (params/async/generator-complete) `class_sig`, closing
        // both the cosmetic-paren false-FAIL (`var C = class { m(){ return (x); } }`) and the
        // method-shape false-PASS for class expressions.
        Expression::ClassExpression(c) => class_sig(c),
        // `yield <arg>` / `yield* <arg>` — the `delegate` bit (delegation iterates the operand) is
        // semantics-bearing; the argument is paren-transparent via `expr_sig`. Client-reachable
        // through generator function literals.
        Expression::YieldExpression(y) => format!(
            "Yield(delegate={},arg={})",
            y.delegate,
            y.argument
                .as_ref()
                .map(expr_sig)
                .unwrap_or_else(|| "<none>".into())
        ),
        // Dynamic `import(<source>, <options>)` — ordinary source-preservable JS. The source and
        // options are paren-transparent via `expr_sig`; the import `phase` (source/defer) is
        // semantics-bearing.
        Expression::ImportExpression(i) => format!(
            "Import(source={},options={},phase={:?})",
            expr_sig(&i.source),
            i.options
                .as_ref()
                .map(expr_sig)
                .unwrap_or_else(|| "<none>".into()),
            i.phase
        ),
        // `import.meta` / `new.target` — DISTINCT meta-properties with different runtime meaning.
        Expression::MetaProperty(m) => format!("Meta({}.{})", m.meta.name, m.property.name),
        // `#x in obj` brand check — ordinary JS inside class bodies; the private-field identifier is
        // semantics-bearing, the operand is paren-transparent via `expr_sig`.
        Expression::PrivateInExpression(p) => {
            format!("PrivateIn(#{} in {})", p.left.name, expr_sig(&p.right))
        }
        // `super` — reachable inside class methods; a bare leaf signed EXPLICITLY (not Debug), so a
        // `super.x` vs `this.x` member object difference is encoded.
        Expression::Super(_) => "Super".to_string(),
        // EXPLICITLY-CLASSIFIED conservative fallback — NOT a generic "remaining kind" catch-all. The
        // ONLY expression forms that reach here are the TS-only wrappers (`TSAsExpression` /
        // `TSInstantiationExpression` / `TSNonNullExpression` / `TSSatisfiesExpression` /
        // `TSTypeAssertion`), JSX (`JSXElement` / `JSXFragment`), and the V8-intrinsic
        // (`V8IntrinsicExpression`). NONE is present in accepted Svelte client JS: TypeScript is
        // stripped before client emit, JSX is not the Svelte client surface, and — decisively — under
        // the comparator's `SourceType::mjs()` parse NONE of them is even parseable in a successfully
        // emitted module (`parses_as_js` / `conformance_sig` reject a torn module before signing). The
        // span-stripped Debug here is a conservative classification of an unreachable surface, NOT a
        // behavioral check — these forms are NOT claimed to be behaviorally compared.
        other => format!("Other({})", strip_debug_noise(&format!("{:?}", other))),
    }
}

/// The signature of a call/`new` argument list — each argument is a DISTINCT entry, so a
/// `SequenceExpression` passed as ONE argument (`f((a, b))`) is structurally different from
/// two separate arguments (`f(a, b)`). Unwrapping parens never collapses the boundary.
fn arguments_sig(args: &oxc_allocator::Vec<'_, Argument<'_>>) -> String {
    let parts: Vec<String> = args
        .iter()
        .map(|a| match a {
            Argument::SpreadElement(s) => format!("Spread({})", expr_sig(&s.argument)),
            other => other
                .as_expression()
                .map(expr_sig)
                .unwrap_or_else(|| "?".to_string()),
        })
        .collect();
    parts.join(",")
}

/// The signature of an object property KEY (a static name, a string/numeric literal, or a
/// computed `[expr]`).
fn property_key_sig(key: &PropertyKey) -> String {
    match key {
        PropertyKey::StaticIdentifier(id) => format!("k:{}", id.name),
        PropertyKey::PrivateIdentifier(id) => format!("k:#{}", id.name),
        PropertyKey::StringLiteral(s) => format!("k:{:?}", s.value),
        PropertyKey::NumericLiteral(n) => format!("k:{}", n.value),
        other => other
            .as_expression()
            .map(|e| format!("k:[{}]", expr_sig(e)))
            .unwrap_or_else(|| "k:?".to_string()),
    }
}

/// The signature of a simple assignment / update target (an identifier or member).
fn simple_target_sig(target: &oxc_ast::ast::SimpleAssignmentTarget) -> String {
    use oxc_ast::ast::SimpleAssignmentTarget as T;
    match target {
        T::AssignmentTargetIdentifier(id) => format!("Id({})", id.name),
        T::StaticMemberExpression(m) => {
            format!("Member({}.{})", expr_sig(&m.object), m.property.name)
        }
        T::ComputedMemberExpression(m) => {
            format!(
                "Computed({}[{}])",
                expr_sig(&m.object),
                expr_sig(&m.expression)
            )
        }
        T::PrivateFieldExpression(m) => {
            format!("Private({}.#{})", expr_sig(&m.object), m.field.name)
        }
        other => format!("Target({})", strip_debug_noise(&format!("{:?}", other))),
    }
}

/// The canonical structural signature of an ASSIGNMENT TARGET — a simple identifier/member OR a
/// destructuring pattern. Reachable through a source-preserved assignment EXPRESSION
/// (`({a, b} = x)`), which the emitter byte-copies, so the destructuring arms are encoded
/// RECURSIVELY and PAREN-TRANSPARENTLY (mirroring `binding_sig`), not via Debug: an
/// `ObjectAssignmentTarget` is its ORDERED property list, an `ArrayAssignmentTarget` is its ORDERED
/// elements (a `None` element is an array `hole`), and an `AssignmentTargetWithDefault` default
/// (`{ a = 1 } = x`) signs its `init` (an `Expression`) via the paren-transparent `expr_sig`, so a
/// redundant paren around a destructuring-assignment default compares EQUAL while a reorder /
/// rename / default change stays distinct. Only the remaining TS-cast wrapper variants
/// (`TSAsExpression` / `TSSatisfiesExpression` / `TSNonNullExpression` / `TSTypeAssertion`) keep a
/// span-stripped Debug fallback — they are not destructuring and not the reachable cosmetic family.
fn assignment_target_sig(target: &oxc_ast::ast::AssignmentTarget) -> String {
    use oxc_ast::ast::AssignmentTarget as T;
    match target {
        T::AssignmentTargetIdentifier(id) => format!("Id({})", id.name),
        T::StaticMemberExpression(m) => {
            format!("Member({}.{})", expr_sig(&m.object), m.property.name)
        }
        T::ComputedMemberExpression(m) => {
            format!(
                "Computed({}[{}])",
                expr_sig(&m.object),
                expr_sig(&m.expression)
            )
        }
        T::PrivateFieldExpression(m) => {
            format!("Private({}.#{})", expr_sig(&m.object), m.field.name)
        }
        T::ArrayAssignmentTarget(a) => {
            let elems: Vec<String> = a
                .elements
                .iter()
                .map(|e| {
                    e.as_ref()
                        .map(assignment_target_maybe_default_sig)
                        .unwrap_or_else(|| "hole".to_string())
                })
                .collect();
            let rest = a
                .rest
                .as_ref()
                .map(|r| assignment_target_sig(&r.target))
                .unwrap_or_else(|| "<none>".to_string());
            format!("ArrPat(elems=[{}],rest={})", elems.join(","), rest)
        }
        T::ObjectAssignmentTarget(o) => {
            let props: Vec<String> = o
                .properties
                .iter()
                .map(assignment_target_property_sig)
                .collect();
            let rest = o
                .rest
                .as_ref()
                .map(|r| assignment_target_sig(&r.target))
                .unwrap_or_else(|| "<none>".to_string());
            format!("ObjPat(props=[{}],rest={})", props.join(","), rest)
        }
        // The remaining TS-cast wrapper variants (`TSAsExpression` / `TSSatisfiesExpression` /
        // `TSNonNullExpression` / `TSTypeAssertion`) are not destructuring and not the reachable
        // cosmetic family — a span-stripped Debug fallback is fine for them.
        other => format!("Target({})", strip_debug_noise(&format!("{:?}", other))),
    }
}

/// The signature of an `AssignmentTargetMaybeDefault` (an array element or an object property's
/// binding). The `AssignmentTargetWithDefault` arm signs the default `init` (an `Expression`) via
/// the paren-transparent `expr_sig`; every other variant is an inherited plain `AssignmentTarget`
/// re-dispatched through `assignment_target_sig`.
fn assignment_target_maybe_default_sig(m: &oxc_ast::ast::AssignmentTargetMaybeDefault) -> String {
    use oxc_ast::ast::AssignmentTargetMaybeDefault as M;
    match m {
        M::AssignmentTargetWithDefault(d) => format!(
            "Default(left={},right={})",
            assignment_target_sig(&d.binding),
            expr_sig(&d.init)
        ),
        // The inherited plain-`AssignmentTarget` variants share `AssignmentTarget`'s discriminants;
        // re-dispatch through `assignment_target_sig` (an unexpected non-inherited variant — none
        // exists today — degrades to the `?` token rather than a panic).
        other => other
            .as_assignment_target()
            .map(assignment_target_sig)
            .unwrap_or_else(|| "?".to_string()),
    }
}

/// The signature of ONE `AssignmentTargetProperty` of an object destructuring assignment target —
/// the shorthand-identifier form (`{ a }` / `{ a = 1 }`) or the renamed form (`{ a: b }` /
/// `{ a: b = 1 }`). The shorthand `init` (`{ a = 1 } = x`) and the renamed binding's default are
/// both paren-transparent via `expr_sig` / `assignment_target_maybe_default_sig`.
fn assignment_target_property_sig(p: &oxc_ast::ast::AssignmentTargetProperty) -> String {
    use oxc_ast::ast::AssignmentTargetProperty as P;
    match p {
        P::AssignmentTargetPropertyIdentifier(id) => {
            let init = id
                .init
                .as_ref()
                .map(expr_sig)
                .unwrap_or_else(|| "<none>".to_string());
            format!("propId(binding={},init={})", id.binding.name, init)
        }
        P::AssignmentTargetPropertyProperty(pp) => {
            format!(
                "propKV(computed={},key={},binding={})",
                pp.computed,
                property_key_sig(&pp.name),
                assignment_target_maybe_default_sig(&pp.binding)
            )
        }
    }
}

/// The canonical structural signature of a BINDING PATTERN — the declared name(s) of a
/// `var`/`let`/`const` declarator OR a function/arrow parameter. The encoding is RECURSIVE and
/// PAREN-TRANSPARENT: a `BindingIdentifier` is its name; an `ObjectPattern` is its ORDERED property
/// list (each property's `computed`/`shorthand` flags, key via `property_key_sig`, value via
/// `binding_sig`) plus a rest marker; an `ArrayPattern` is its ORDERED elements (each via
/// `binding_sig`, a `None` element is an array `hole`) plus a rest marker; an `AssignmentPattern`
/// default (`{ a = 1 }`) signs its `left` via `binding_sig` and its `right` (an `Expression`) via the
/// paren-transparent `expr_sig`, so a redundant paren around a destructuring default (`{ a = (1) }`)
/// compares EQUAL to the bare one while a reorder / rename / default drop / default-value change
/// stays distinct. Reachable: an author destructuring pattern in a source-preserved value position
/// (a `{@html}` / dynamic-value arrow the emitter byte-copies) carries these defaults/parens, so a
/// Debug fallback that does not peel the paren wrapper was a cosmetic false-FAIL.
fn binding_sig(pattern: &BindingPattern) -> String {
    match pattern {
        BindingPattern::BindingIdentifier(id) => format!("name:{}", id.name),
        BindingPattern::ObjectPattern(o) => {
            let props: Vec<String> = o
                .properties
                .iter()
                .map(|p| {
                    format!(
                        "prop(computed={},shorthand={},key={},value={})",
                        p.computed,
                        p.shorthand,
                        property_key_sig(&p.key),
                        binding_sig(&p.value)
                    )
                })
                .collect();
            let rest = o
                .rest
                .as_ref()
                .map(|r| binding_sig(&r.argument))
                .unwrap_or_else(|| "<none>".to_string());
            format!("ObjPat(props=[{}],rest={})", props.join(","), rest)
        }
        BindingPattern::ArrayPattern(a) => {
            let elems: Vec<String> = a
                .elements
                .iter()
                .map(|e| {
                    e.as_ref()
                        .map(binding_sig)
                        .unwrap_or_else(|| "hole".to_string())
                })
                .collect();
            let rest = a
                .rest
                .as_ref()
                .map(|r| binding_sig(&r.argument))
                .unwrap_or_else(|| "<none>".to_string());
            format!("ArrPat(elems=[{}],rest={})", elems.join(","), rest)
        }
        BindingPattern::AssignmentPattern(d) => {
            // The default initializer is an `Expression` → paren-transparent `expr_sig`.
            format!(
                "Default(left={},right={})",
                binding_sig(&d.left),
                expr_sig(&d.right)
            )
        }
    }
}

/// The canonical structural signature of a FUNCTION/ARROW PARAMETER LIST — the ORDERED
/// binding identities of each parameter (name + position) plus a rest marker. Parameter NAMES
/// and their ORDER are SEMANTIC for the emitted client code: a memoized `$.template_effect`
/// arrow `($0, $1) => … ${$0} … ${$1} …` binds each `$N` deps-array slot positionally, so a
/// param-order swap (`($1, $0) =>`) re-binds the dep values with a byte-identical body — a real
/// behavior change a count-only signature would silently pass. Encoding the ordered patterns
/// (via `binding_sig`) closes that hole while staying paren/span-insensitive.
fn params_sig(params: &FormalParameters) -> String {
    let mut parts: Vec<String> = params
        .items
        .iter()
        .map(|p| {
            // The DEFAULT initializer (`(a = 1)`) is SEMANTIC — `(a = 1)`, `(a = 2)`, and `(a)`
            // bind different values when the argument is `undefined`. Encode the default expression
            // (via `expr_sig`) when present, and an explicit `<none>` marker when absent so a
            // dropped/added default is caught. Reachable via source-preserved function literals
            // (`{@html () => …}`, dynamic-value arrows) the emitter byte-copies.
            let init = p
                .initializer
                .as_ref()
                .map(|e| expr_sig(e))
                .unwrap_or_else(|| "<none>".to_string());
            format!("{},init={}", binding_sig(&p.pattern), init)
        })
        .collect();
    if let Some(rest) = &params.rest {
        // The rest binding (`...rest`) is a `BindingRestElement { argument: BindingPattern }` →
        // encode its pattern recursively (paren-transparent, like every other binding) rather than
        // via Debug, so a rest destructuring default/paren stays consistent with the rest of the sig.
        parts.push(format!("...{}", binding_sig(&rest.rest.argument)));
    }
    format!("[{}]", parts.join(","))
}

/// The structural signature of a `ModuleExportName` (the `imported` side of a named import
/// specifier). Distinguishes the identifier/reference/string forms and their names, so a different
/// imported symbol from the same module is a divergence.
fn module_export_name_sig(name: &MEN<'_>) -> String {
    match name {
        MEN::IdentifierName(n) => format!("Id({:?})", n.name.as_str()),
        MEN::IdentifierReference(n) => format!("Ref({:?})", n.name.as_str()),
        MEN::StringLiteral(s) => format!("Str({:?})", s.value),
    }
}

/// The structural signature of ONE import specifier — the specifier KIND (named / default /
/// namespace), its imported-name, and its local binding name. A namespace import and a named
/// import with the same source + count are now DISTINCT (`Namespace(*)` vs `Named(...)`), and an
/// imported-name or local-name drift over the same source is caught. The rule treats imports as
/// STRUCTURAL, so all three sub-axes are encoded.
fn import_specifier_sig(spec: &IDS<'_>) -> String {
    match spec {
        IDS::ImportSpecifier(s) => format!(
            "Named(kind={:?},imported={},local={:?})",
            s.import_kind,
            module_export_name_sig(&s.imported),
            s.local.name.as_str()
        ),
        IDS::ImportDefaultSpecifier(s) => {
            format!(
                "Default(imported=default,local={:?})",
                s.local.name.as_str()
            )
        }
        IDS::ImportNamespaceSpecifier(s) => {
            format!("Namespace(imported=*,local={:?})", s.local.name.as_str())
        }
    }
}

/// One import attribute (`type: "json"`): the key (identifier OR string) + the cooked string value.
fn import_attribute_sig(a: &oxc_ast::ast::ImportAttribute<'_>) -> String {
    let key = match &a.key {
        oxc_ast::ast::ImportAttributeKey::Identifier(id) => id.name.as_str().to_string(),
        oxc_ast::ast::ImportAttributeKey::StringLiteral(s) => format!("{:?}", s.value),
    };
    format!("{}={:?}", key, a.value.value)
}

/// A `with { ... }` / `assert { ... }` clause: the keyword + ordered attributes. `None` → `<none>`.
/// The import-attribute family is an ORACLE axis: official Svelte client output can carry
/// module-script imports/exports, so the comparator must compare a `with`-clause keyword + each
/// attribute key/value even though native Verter currently refuses `<script module>`.
fn with_clause_sig(w: &Option<oxc_allocator::Box<'_, oxc_ast::ast::WithClause<'_>>>) -> String {
    match w {
        None => "<none>".to_string(),
        Some(w) => format!(
            "{:?}[{}]",
            w.keyword,
            w.with_entries
                .iter()
                .map(import_attribute_sig)
                .collect::<Vec<_>>()
                .join(";")
        ),
    }
}

/// One export specifier (`a as value`): export-kind + local + exported (both via
/// `module_export_name_sig`). A specifier-only export (`export { a as value }`, no inline
/// declaration) is signed through this so a local/exported/kind drift FAILS.
fn export_specifier_sig(s: &oxc_ast::ast::ExportSpecifier<'_>) -> String {
    format!(
        "kind={:?},local={},exported={}",
        s.export_kind,
        module_export_name_sig(&s.local),
        module_export_name_sig(&s.exported)
    )
}

/// The structural signature of ONE directive-prologue entry (`"use strict";`). Only the COOKED
/// value (`expression.value`) is in contract; the raw carrier formatting — quote style (`"use
/// strict"` vs `'use strict'`) and escape representation (`"\x75se strict"` vs `"use strict"`) — is
/// cosmetic the official printer normalizes, so the raw token (`directive`) is NOT signed. A
/// dropped / added / re-text-ed directive still diverges because the cooked value differs (e.g.
/// `"use strict"` flips strict mode and differs from `"use asm"`).
fn directive_sig(d: &Directive) -> String {
    format!("Dir(value={:?})", d.expression.value)
}

/// The ORDERED directive prologue of a function/program body. Directive ORDER and CONTENT are both
/// significant.
fn directives_sig(dirs: &oxc_allocator::Vec<'_, Directive<'_>>) -> String {
    format!(
        "[{}]",
        dirs.iter().map(directive_sig).collect::<Vec<_>>().join(";")
    )
}

/// The structural signature of a `FunctionBody` — its ORDERED directive prologue
/// (`FunctionBody.directives`) AND its ORDERED statement list. The directive half is the axis the
/// pre-fix sign sites (`statements_sig(&body.statements)` only) DROPPED, so a directive-bearing
/// source-preserved arrow/function body (`() => { "use strict"; … }` byte-copied by the value
/// emitter) collapsed with an absent/different prologue — a structural false-PASS this closes.
fn function_body_sig(body: &FunctionBody) -> String {
    format!(
        "dirs={},stmts={}",
        directives_sig(&body.directives),
        statements_sig(&body.statements)
    )
}

/// The structural signature of a `for` statement's init slot. A `VariableDeclaration` init signs
/// through `decl_var_sig`; an expression init (an inherited `Expression` variant) signs through the
/// paren-transparent `expr_sig`; an absent init signs `<none>`.
fn for_init_sig(init: &Option<ForStatementInit>) -> String {
    match init {
        None => "<none>".to_string(),
        Some(ForStatementInit::VariableDeclaration(d)) => decl_var_sig(d.kind, &d.declarations),
        Some(other) => other
            .as_expression()
            .map(expr_sig)
            .unwrap_or_else(|| "?".to_string()),
    }
}

/// The structural signature of a `for-in` / `for-of` left slot. A `VariableDeclaration` left signs
/// through `decl_var_sig`; an assignment-target left (an inherited `AssignmentTarget` variant) signs
/// through the paren-transparent `assignment_target_sig`.
fn for_left_sig(left: &ForStatementLeft) -> String {
    match left {
        ForStatementLeft::VariableDeclaration(d) => decl_var_sig(d.kind, &d.declarations),
        other => other
            .as_assignment_target()
            .map(assignment_target_sig)
            .unwrap_or_else(|| "?".to_string()),
    }
}

/// The structural signature of ONE `switch` case — its test (`None` = the `default` arm) plus its
/// ORDERED consequent statement list.
fn switch_case_sig(c: &SwitchCase) -> String {
    format!(
        "case(test={},cons={})",
        c.test
            .as_ref()
            .map(expr_sig)
            .unwrap_or_else(|| "default".into()),
        statements_sig(&c.consequent)
    )
}

/// A BOUNDED structural skeleton of a class — its name, super-class (paren-transparent `expr_sig`),
/// and ordered member skeletons. CONSERVATIVE structural encoding by design: it signs the
/// behavior-bearing member skeleton — for a method, the COMPLETE runtime shape
/// (kind + static + computed + async + generator + key + params + body) — and STOPS there. It does
/// NOT open a member-type expansion, and it does NOT sign the TS-only method axes (abstract /
/// override / optional / accessibility / type-params / return-type / this-param / definite /
/// readonly / declare), which are stripped before client emit. Each signed sub-part reduces to an
/// existing terminal helper (`expr_sig` / `property_key_sig` / `params_sig` / `function_body_sig` /
/// `statements_sig`). Reachable via a class expression/declaration inside a source-preserved
/// arrow/function body the value emitter byte-copies.
///
/// DECORATORS are ENCODED (class-level here via `decorators_sig`, per-member in `class_element_sig`).
/// This is REACHABLE, not fail-closed: OXC parses ECMAScript decorators under the comparator's
/// `SourceType::mjs()` parse (verified — the parse succeeds with errors=0, so `conformance_sig` signs a
/// decorated class structurally rather than refusing it), the Svelte runtime strips TS but NOT
/// decorators, and the value-expression refusal set does NOT refuse decorators/classes — so a decorated
/// class in a source-preserved `{@html}`/dynamic value is byte-copied to emitted client JS, where the
/// decorator executes and can alter runtime behavior. The class skeleton therefore covers decorators.
///
/// `Class.r#type` (`ClassDeclaration` vs `ClassExpression`) IS signed: it is behavior-bearing at
/// `export default` — `export default class C {}` binds `C` in module scope while
/// `export default (class C {})` is a class EXPRESSION whose `C` is visible only inside the class body
/// (so a later `var y = C` throws). The TS-only member axes (the `MethodDefinition` abstract flag, the
/// `AccessorProperty` accessor-type, index signatures) are stripped before client emit and are
/// ignored — they are not runtime-bearing for emitted JS.
fn class_sig(c: &Class) -> String {
    format!(
        "Class(type={:?},decorators={},name={},super={},members=[{}])",
        c.r#type,
        decorators_sig(&c.decorators),
        c.id.as_ref().map(|i| i.name.as_str()).unwrap_or(""),
        c.super_class
            .as_ref()
            .map(expr_sig)
            .unwrap_or_else(|| "<none>".into()),
        c.body
            .body
            .iter()
            .map(class_element_sig)
            .collect::<Vec<_>>()
            .join(";")
    )
}

/// The ordered signature of a decorator list. Each decorator is its expression signed through the
/// paren-transparent `expr_sig` (a decorator IS an expression — `@foo`, `@foo.bar`, `@foo(arg)`), so a
/// redundant paren in a decorator argument is cosmetic-EQUAL while a different decorator
/// expression/name/argument is a behavioral divergence (the decorator executes and can alter runtime
/// behavior). Order is significant (decorators apply bottom-up). An empty list signs `[]`. Reachable:
/// OXC parses ECMAScript decorators in plain `SourceType::mjs()` (verified), the Svelte runtime strips TS
/// but NOT decorators, and a decorated class in a source-preserved `{@html}`/dynamic value is
/// byte-copied to emitted client JS.
fn decorators_sig(decorators: &oxc_allocator::Vec<'_, Decorator<'_>>) -> String {
    format!(
        "[{}]",
        decorators
            .iter()
            .map(|d| expr_sig(&d.expression))
            .collect::<Vec<_>>()
            .join(";")
    )
}

/// The structural skeleton of ONE class member — NOT a full member-type expansion (the closure
/// boundary). A METHOD signs the COMPLETE runtime shape
/// (kind + static + computed + async + generator + key + params + body); property/accessor/static-block
/// arms have no params/async/generator. Each sub-part reduces to an existing terminal helper
/// (`property_key_sig` / `params_sig` / `function_body_sig` / `expr_sig` / `statements_sig`). The
/// only TS-only member form (`TSIndexSignature`) is an explicitly-classified marker, NOT a generic
/// discriminant collapse. The TS-only method axes (abstract / override / optional / accessibility /
/// type-params / return-type / this-param / definite / readonly / declare) are stripped before client
/// emit and are safely ignored. DECORATORS are ENCODED (each member arm signs `decorators` via
/// `decorators_sig` through the paren-transparent `expr_sig`): OXC parses ECMAScript decorators under the
/// comparator's `SourceType::mjs()` parse (verified), the runtime strips TS but NOT decorators, and a
/// decorated member in a source-preserved class body is byte-copied to emitted client JS — reachable.
fn class_element_sig(el: &ClassElement) -> String {
    match el {
        ClassElement::StaticBlock(b) => format!("static_block({})", statements_sig(&b.body)),
        ClassElement::MethodDefinition(m) => format!(
            "method(decorators={},kind={:?},static={},computed={},async={},generator={},key={},params={},body={})",
            decorators_sig(&m.decorators),
            m.kind,
            m.r#static,
            m.computed,
            m.value.r#async,
            m.value.generator,
            property_key_sig(&m.key),
            params_sig(&m.value.params),
            m.value
                .body
                .as_ref()
                .map(|b| function_body_sig(b))
                .unwrap_or_default()
        ),
        ClassElement::PropertyDefinition(p) => format!(
            "prop(decorators={},static={},computed={},key={},value={})",
            decorators_sig(&p.decorators),
            p.r#static,
            p.computed,
            property_key_sig(&p.key),
            p.value
                .as_ref()
                .map(expr_sig)
                .unwrap_or_else(|| "<none>".into())
        ),
        ClassElement::AccessorProperty(a) => format!(
            "accessor(decorators={},static={},computed={},key={},value={})",
            decorators_sig(&a.decorators),
            a.r#static,
            a.computed,
            property_key_sig(&a.key),
            a.value
                .as_ref()
                .map(expr_sig)
                .unwrap_or_else(|| "<none>".into())
        ),
        // TS-only member form: an explicitly-classified marker (NOT accepted client-positive today;
        // a TS index signature has no client runtime surface). NOT a generic discriminant collapse.
        ClassElement::TSIndexSignature(_) => "ts_index_sig".to_string(),
    }
}

/// The canonical structural signature of ONE statement.
fn stmt_sig(stmt: &Statement) -> String {
    match stmt {
        Statement::ExpressionStatement(s) => format!("Expr({})", expr_sig(&s.expression)),
        Statement::VariableDeclaration(d) => decl_var_sig(d.kind, &d.declarations),
        Statement::ReturnStatement(r) => format!(
            "Return({})",
            r.argument.as_ref().map(expr_sig).unwrap_or_default()
        ),
        Statement::IfStatement(s) => format!(
            "If(test={},cons={},alt={})",
            expr_sig(&s.test),
            stmt_sig(&s.consequent),
            s.alternate.as_ref().map(stmt_sig).unwrap_or_default()
        ),
        Statement::BlockStatement(b) => format!("Block({})", statements_sig(&b.body)),
        Statement::FunctionDeclaration(f) => format!(
            "FnDecl(async={},generator={},name={},params={},body={})",
            f.r#async,
            f.generator,
            f.id.as_ref().map(|i| i.name.as_str()).unwrap_or(""),
            params_sig(&f.params),
            f.body
                .as_ref()
                .map(|b| function_body_sig(b))
                .unwrap_or_default()
        ),
        // ── Ordinary-JS control-flow statements. Each is reachable inside a source-preserved
        // arrow/function body the value emitter byte-copies (e.g. `{@html () => { for(...) … }}`), so
        // every behavior-bearing sub-part is signed through an existing terminal helper. The pre-fix
        // `Stmt(discriminant)` fallback collapsed all of these to a discriminant-only signature — a
        // structural false-PASS this closes.
        Statement::BreakStatement(s) => format!(
            "Break({})",
            s.label.as_ref().map(|l| l.name.as_str()).unwrap_or("")
        ),
        Statement::ContinueStatement(s) => format!(
            "Continue({})",
            s.label.as_ref().map(|l| l.name.as_str()).unwrap_or("")
        ),
        Statement::DebuggerStatement(_) => "Debugger".to_string(),
        Statement::EmptyStatement(_) => "Empty".to_string(),
        Statement::DoWhileStatement(s) => {
            format!(
                "DoWhile(body={},test={})",
                stmt_sig(&s.body),
                expr_sig(&s.test)
            )
        }
        Statement::WhileStatement(s) => {
            format!(
                "While(test={},body={})",
                expr_sig(&s.test),
                stmt_sig(&s.body)
            )
        }
        Statement::ForStatement(s) => format!(
            "For(init={},test={},update={},body={})",
            for_init_sig(&s.init),
            s.test
                .as_ref()
                .map(expr_sig)
                .unwrap_or_else(|| "<none>".into()),
            s.update
                .as_ref()
                .map(expr_sig)
                .unwrap_or_else(|| "<none>".into()),
            stmt_sig(&s.body)
        ),
        Statement::ForInStatement(s) => format!(
            "ForIn(left={},right={},body={})",
            for_left_sig(&s.left),
            expr_sig(&s.right),
            stmt_sig(&s.body)
        ),
        Statement::ForOfStatement(s) => format!(
            "ForOf(await={},left={},right={},body={})",
            s.r#await,
            for_left_sig(&s.left),
            expr_sig(&s.right),
            stmt_sig(&s.body)
        ),
        Statement::SwitchStatement(s) => format!(
            "Switch(disc={},cases=[{}])",
            expr_sig(&s.discriminant),
            s.cases
                .iter()
                .map(switch_case_sig)
                .collect::<Vec<_>>()
                .join(";")
        ),
        Statement::TryStatement(s) => format!(
            "Try(block={},handler={},finalizer={})",
            statements_sig(&s.block.body),
            s.handler
                .as_ref()
                .map(|h| format!(
                    "catch(param={},body={})",
                    h.param
                        .as_ref()
                        .map(|p| binding_sig(&p.pattern))
                        .unwrap_or_else(|| "<none>".into()),
                    statements_sig(&h.body.body)
                ))
                .unwrap_or_else(|| "<none>".into()),
            s.finalizer
                .as_ref()
                .map(|f| statements_sig(&f.body))
                .unwrap_or_else(|| "<none>".into())
        ),
        Statement::ThrowStatement(s) => format!("Throw({})", expr_sig(&s.argument)),
        Statement::LabeledStatement(s) => {
            format!("Label({}:{})", s.label.name.as_str(), stmt_sig(&s.body))
        }
        Statement::WithStatement(s) => {
            format!(
                "With(object={},body={})",
                expr_sig(&s.object),
                stmt_sig(&s.body)
            )
        }
        Statement::ClassDeclaration(c) => class_sig(c),
        Statement::ImportDeclaration(i) => {
            // The import SOURCE (byte-significant) + the import KIND (value/type) + the import PHASE
            // (`source`/`defer`) + the `with`-clause import attributes + the full per-specifier
            // STRUCTURAL encoding (kind + imported-name + local-name, in order). A bare `import 'x'`
            // side-effect import (`None` specifiers) encodes `SideEffect`, distinct from
            // `import {} from 'x'` (`Some([])`). The rule treats imports as structural, so a
            // namespace-vs-named / imported-name / local-name / phase / attribute drift over the same
            // source FAILS. An ORACLE axis: official module-script output can carry these.
            let specs = match &i.specifiers {
                None => "SideEffect".to_string(),
                Some(specs) => format!(
                    "[{}]",
                    specs
                        .iter()
                        .map(import_specifier_sig)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            };
            format!(
                "Import(src={:?},kind={:?},phase={:?},with={},specs={})",
                i.source.value,
                i.import_kind,
                i.phase,
                with_clause_sig(&i.with_clause),
                specs
            )
        }
        // The full `ExportNamedDeclaration` surface: an inline declaration (`export const a = 1`) OR
        // a specifier list (`export { a as value }`) OR a re-export source (`export { a } from "x"`),
        // plus the export-kind and `with`-clause. An ORACLE axis (official module-script output can
        // carry these) — a specifier/source/kind/with drift over an otherwise identical export FAILS.
        Statement::ExportNamedDeclaration(e) => format!(
            "ExportNamed(decl={},specs=[{}],source={},kind={:?},with={})",
            e.declaration
                .as_ref()
                .map(decl_sig)
                .unwrap_or_else(|| "<none>".into()),
            e.specifiers
                .iter()
                .map(export_specifier_sig)
                .collect::<Vec<_>>()
                .join(";"),
            e.source
                .as_ref()
                .map(|s| format!("{:?}", s.value))
                .unwrap_or_else(|| "<none>".into()),
            e.export_kind,
            with_clause_sig(&e.with_clause)
        ),
        // `export * from "x"` / `export * as ns from "x"` — an ORACLE axis. The source, the optional
        // namespace rename (`exported`), the export-kind, and the `with`-clause are all signed; pre-
        // fix this fell to the `Stmt(discriminant)` fallback (a false-PASS over different sources).
        Statement::ExportAllDeclaration(e) => format!(
            "ExportAll(exported={},source={:?},kind={:?},with={})",
            e.exported
                .as_ref()
                .map(module_export_name_sig)
                .unwrap_or_else(|| "<none>".into()),
            e.source.value,
            e.export_kind,
            with_clause_sig(&e.with_clause)
        ),
        Statement::ExportDefaultDeclaration(e) => {
            use oxc_ast::ast::ExportDefaultDeclarationKind as K;
            match &e.declaration {
                K::FunctionDeclaration(f) => format!(
                    "ExportDefaultFn(async={},generator={},name={},params={},body={})",
                    f.r#async,
                    f.generator,
                    f.id.as_ref().map(|i| i.name.as_str()).unwrap_or(""),
                    params_sig(&f.params),
                    f.body
                        .as_ref()
                        .map(|b| function_body_sig(b))
                        .unwrap_or_default()
                ),
                K::ClassDeclaration(c) => format!("ExportDefault({})", class_sig(c)),
                other => other
                    .as_expression()
                    .map(|e| format!("ExportDefault({})", expr_sig(e)))
                    .unwrap_or_else(|| "ExportDefault?".to_string()),
            }
        }
        // Discriminant-only collapse for genuinely TS/module-only statement forms not parseable under
        // `SourceType::mjs()` (e.g. `TSModuleDeclaration`, `TSImportEqualsDeclaration`,
        // `TSExportAssignment`, and the TS-only declaration forms reached via the
        // `ExportNamedDeclaration` inner-declaration path, e.g. `export type X = …`). The ordinary-JS
        // statement set AND the module import/export family are encoded above, so this arm is
        // unreachable for mjs-parseable input. Tracked: svelte-native-compiler-plan.md §8 D-17 — the
        // first change that makes one of these accepted client-positive must prove it refused
        // (fail-closed) or encode it structurally in the same change.
        other => format!("Stmt({:?})", std::mem::discriminant(other)),
    }
}

/// The signature of a `var`/`let`/`const` declaration's declarator list.
fn decl_var_sig(
    kind: VariableDeclarationKind,
    declarators: &oxc_allocator::Vec<'_, oxc_ast::ast::VariableDeclarator<'_>>,
) -> String {
    let parts: Vec<String> = declarators
        .iter()
        .map(|d| {
            format!(
                "{},init={}",
                binding_sig(&d.id),
                d.init.as_ref().map(expr_sig).unwrap_or_default()
            )
        })
        .collect();
    format!("Var({:?},[{}])", kind, parts.join(";"))
}

/// The signature of a (named-export) declaration.
fn decl_sig(decl: &Declaration) -> String {
    match decl {
        Declaration::VariableDeclaration(d) => decl_var_sig(d.kind, &d.declarations),
        Declaration::FunctionDeclaration(f) => format!(
            // The BODY is encoded (via `function_body_sig`, including `FunctionBody.directives`),
            // matching the `Statement::FunctionDeclaration` and `ExportDefaultFn` arms — two
            // named-export functions with the same signature but different bodies
            // (`export function f(){a();}` vs `…{b();}`) must NOT collapse to the same signature (a
            // structural false-PASS). An ORACLE axis: official Svelte client module output can carry
            // module-script export declarations, so the comparator must be correct here even though
            // native Verter currently refuses `<script module>`.
            "FnDecl(async={},generator={},name={},params={},body={})",
            f.r#async,
            f.generator,
            f.id.as_ref().map(|i| i.name.as_str()).unwrap_or(""),
            params_sig(&f.params),
            f.body
                .as_ref()
                .map(|b| function_body_sig(b))
                .unwrap_or_default()
        ),
        Declaration::ClassDeclaration(c) => class_sig(c),
        // Discriminant-only collapse for genuinely TS-only declaration forms that are NOT accepted
        // client-positive today (e.g. `TSTypeAliasDeclaration`, `TSInterfaceDeclaration`,
        // `TSEnumDeclaration`, `TSModuleDeclaration`, `TSImportEqualsDeclaration`). The reachable JS
        // declaration forms (`VariableDeclaration`, `FunctionDeclaration`, `ClassDeclaration`) are
        // encoded above. Tracked: svelte-native-compiler-plan.md §8 D-17 — the first change that
        // makes one of these accepted client-positive must prove it refused (fail-closed) or encode
        // it structurally in the same change.
        other => format!("Decl({:?})", std::mem::discriminant(other)),
    }
}

/// The signature of a statement LIST (in order).
fn statements_sig(stmts: &oxc_allocator::Vec<'_, Statement<'_>>) -> String {
    // A stray no-op `EmptyStatement` (`;`) in a statement LIST is a printer-dropped cosmetic no-op —
    // filter it so `{ ; return x; }` and `{ return x; }` compare EQUAL. (An EmptyStatement in a
    // REQUIRED child position — a loop/if/with/labeled body like `for(;;);` — is NOT filtered: it is
    // reached through `stmt_sig(Empty)` directly, where an empty vs non-empty body IS behavior-bearing.)
    // ASI is a non-issue: once it parsed as an AST `EmptyStatement` in a list, it is a removable no-op.
    let parts: Vec<String> = stmts
        .iter()
        .filter(|s| !matches!(s, Statement::EmptyStatement(_)))
        .map(stmt_sig)
        .collect();
    format!("[{}]", parts.join(";"))
}

/// The structural signature of a whole PROGRAM — its top-level directive prologue
/// (`Program.directives`, e.g. a module-level `"use strict";`) AND its ordered statement body. The
/// pre-fix `module_sig` was `statements_sig(&program.body)` only, which DROPPED `Program.directives`,
/// so a directive-bearing module collapsed with an absent/different prologue — a structural
/// false-PASS this closes. (`Program.directives` is ALSO walked for comment anchors by
/// `CommentAnchorIndex`; this adds it to the STRUCTURAL signature too.)
fn program_sig(program: &oxc_ast::ast::Program) -> String {
    format!(
        "Program(hashbang={},dirs={},body={})",
        program
            .hashbang
            .as_ref()
            .map(|h| format!("{:?}", h.value))
            .unwrap_or_else(|| "<none>".into()),
        directives_sig(&program.directives),
        statements_sig(&program.body)
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// SEMANTIC-COMMENT signature — the in-contract comment boundary the
// `Compiled-Output Conformance (CRITICAL)` rule keeps in contract.
//
// The AST module signature (`statements_sig`) drops ALL comments (OXC's `program.body` carries
// no comment nodes). NON-SEMANTIC comments (`// note`, `/* note */`, unknown `@foo`) ARE a waived
// cosmetic axis and stay dropped. But SEMANTIC / tool-consumed comments — PURE-family
// (`/*@__PURE__*/`), license/preserve (`/*! … */`, `@license`, `@preserve`), source-map
// (`//# sourceURL=` / `//# sourceMappingURL=`), TS-directives (triple-slash references,
// `@ts-check`/`@ts-nocheck`/`@ts-ignore`/`@ts-expect-error`), JSDoc, and the other tool
// directives OXC classifies (Webpack/Vite/Turbopack/CoverageIgnore) — remain IN CONTRACT, so a
// drop / corruption / move of one is a conformance divergence the comparator MUST catch.
//
// The signature is an OCCURRENCE-PATH multiset (not ordered-text-only): each semantic comment is
// keyed by a deterministic AST OCCURRENCE PATH (`pos/stmt[i]/edge[j]/…` — the indexed child-edge
// chain from the top-level statement down to the deepest node whose span boundary matches the
// comment's anchor byte), NOT by structural signatures (`stmt_sig` / `expr_sig` are NOT the anchor
// basis). The path reads AST topology (not bytes), so the anchor is whitespace- and redundant-paren
// insensitive, yet POSITION-sensitive: a comment MOVED to a different statement/expression — even a
// structurally-identical sibling — changes an index segment of its path. Classifying an
// already-EXTRACTED comment's text via a total membership predicate is in scope (it is NOT a
// regex-on-type-text semantic engine); slicing the raw code for the already-identified comment token
// payload is acceptable (NOT a raw full-module comparison).
// ─────────────────────────────────────────────────────────────────────────────

use oxc_ast::ast::{Comment, CommentContent, CommentPosition};
use oxc_span::GetSpan;

/// The full conformance signature of a parsed module — the AST module signature AND the
/// semantic-comment signature, computed from the SAME OXC parse. Equality of both halves is the
/// gate's oracle.
struct ModuleConformanceSig {
    module_sig: String,
    comment_sig: Vec<String>,
}

impl ModuleConformanceSig {
    fn equals(&self, other: &Self) -> bool {
        self.module_sig == other.module_sig && self.comment_sig == other.comment_sig
    }
}

/// Classify a comment as SEMANTIC (in-contract) and return its class label, or `None` if it is a
/// waived cosmetic comment. The classification is a TOTAL membership predicate over OXC's
/// `CommentContent` plus a text predicate for the categories OXC does not have a `CommentContent`
/// variant for (source-map directives and TS directives). `raw` is the comment's exact token text
/// INCLUDING delimiters (`/* … */` or `// …`).
fn semantic_comment_class(comment: &Comment, raw: &str) -> Option<&'static str> {
    // 1. OXC-classified annotation comments that are in contract.
    match comment.content {
        CommentContent::Pure => return Some("Pure"),
        CommentContent::PureNotApplied => return Some("PureNotApplied"),
        CommentContent::NoSideEffects => return Some("NoSideEffects"),
        CommentContent::Legal => return Some("Legal"),
        CommentContent::JsdocLegal => return Some("JsdocLegal"),
        // JSDoc is in contract for THIS client-JS oracle (excluding it would contradict the rule).
        CommentContent::Jsdoc => return Some("Jsdoc"),
        CommentContent::Webpack => return Some("Webpack"),
        CommentContent::Vite => return Some("Vite"),
        CommentContent::Turbopack => return Some("Turbopack"),
        CommentContent::CoverageIgnore => return Some("CoverageIgnore"),
        // `None` => not OXC-classified; fall through to the text predicate.
        CommentContent::None => {}
    }
    // 2. The text predicate for categories OXC does not have a `CommentContent` variant for. The
    //    inner text (delimiters stripped) is matched against a small, total set of known directive
    //    forms. This classifies an already-extracted comment's text — it is NOT a semantic engine.
    let inner = if let Some(rest) = raw.strip_prefix("/*") {
        rest.strip_suffix("*/").unwrap_or(rest)
    } else if let Some(rest) = raw.strip_prefix("//") {
        rest
    } else {
        raw
    };
    let t = inner.trim_start();
    // Source-comment directives: the modern `//# sourceURL=` / `//# sourceMappingURL=` forms (the
    // leading `#` survives the `//`/`/*` strip above) AND the deprecated legacy `//@ sourceURL=` /
    // `//@ sourceMappingURL=` `@`-prefixed forms (still emitted by older tooling and consumed by
    // browsers). Both classify as a source-map directive.
    for sigil in ['#', '@'] {
        if let Some(after_sigil) = t.strip_prefix(sigil) {
            let a = after_sigil.trim_start();
            if a.starts_with("sourceURL=") || a.starts_with("sourceMappingURL=") {
                return Some("SourceMap");
            }
        }
    }
    // TS triple-slash reference directive: a triple-slash directive is a LINE comment whose RAW
    // text opens with EXACTLY three slashes (`/// <reference …`). Require `raw.strip_prefix("///")`
    // on the RAW token — NOT a single leading `/` on the already-`//`-stripped inner text, which
    // would also accept `// / <reference …` (a line comment whose body merely begins `/ <reference`)
    // as a false directive. After the `///` opener, trim whitespace, then require `<reference`
    // followed by a BOUNDARY (whitespace / `/` / `>`), so `/// <referencee path=…>` (a token-
    // boundary lookalike) does NOT classify either.
    if let Some(after_triple) = raw.strip_prefix("///") {
        let after_triple = after_triple.trim_start();
        if let Some(rest) = after_triple.strip_prefix("<reference") {
            if reference_boundary(rest) {
                return Some("TsTripleSlash");
            }
        }
    }
    // TS pragma directives — split by family, because the two families have DIFFERENT valid forms:
    //
    // `@ts-check` / `@ts-nocheck` are file-level mode pragmas valid ONLY as a `//` LINE comment
    // (TypeScript ignores them in `/* */` block form), so they classify only when the RAW token
    // opens with `//` (NOT `/*`), AND with a TS-pragma boundary = end / whitespace / `:` (so
    // `// @ts-check/foo` does NOT classify — the `/foo` continues the token). A `/* @ts-check */`
    // block comment and a `// @ts-check/foo` lookalike both stay WAIVED.
    //
    // `@ts-ignore` / `@ts-expect-error` are line-suppression directives valid in BOTH `//` line and
    // `/* */` block forms in TS, so they keep the looser identifier-token boundary
    // (`directive_boundary`) and match regardless of the raw delimiter.
    let is_line_comment = raw.starts_with("//") && !raw.starts_with("/*");
    if is_line_comment {
        for d in ["@ts-check", "@ts-nocheck"] {
            if let Some(rest) = t.strip_prefix(d) {
                if ts_pragma_boundary(rest) {
                    return Some("TsDirective");
                }
            }
        }
    }
    for d in ["@ts-ignore", "@ts-expect-error"] {
        if let Some(rest) = t.strip_prefix(d) {
            if directive_boundary(rest) {
                return Some("TsDirective");
            }
        }
    }
    None
}

/// Whether the text after a `@ts-*` directive token is a TOKEN BOUNDARY — end-of-text or a
/// non-identifier char (the directive's identifier-like token does not continue). `@ts-ignore-me`
/// fails (`-` continues the token); `@ts-ignore` / `@ts-ignore foo` / `@ts-ignore: x` pass.
fn directive_boundary(rest: &str) -> bool {
    match rest.chars().next() {
        None => true,
        // An ASCII letter/digit/`_`/`$`/`-` continues an identifier-like directive token → NOT a
        // boundary (so `@ts-ignore-me` ≠ `@ts-ignore`).
        Some(c) => !(c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '-'),
    }
}

/// Whether the text after a `@ts-check` / `@ts-nocheck` pragma token is a TS-PRAGMA boundary —
/// STRICTER than `directive_boundary`: end-of-text, ASCII whitespace, or a `:`. TypeScript treats
/// `// @ts-check` (and a following comment payload like `// @ts-check: note`) as the mode pragma,
/// but `// @ts-check/foo` is a different token (the `/foo` continues it) and must NOT classify.
fn ts_pragma_boundary(rest: &str) -> bool {
    match rest.chars().next() {
        None => true,
        Some(c) => c.is_ascii_whitespace() || c == ':',
    }
}

/// Whether the text after `<reference` is a triple-slash-reference BOUNDARY (whitespace / `/` /
/// `>` / end), so `<reference path=…` passes but `<referencee …` (the `e` continues the token)
/// does NOT classify.
fn reference_boundary(rest: &str) -> bool {
    match rest.chars().next() {
        None => true,
        Some(c) => c.is_whitespace() || c == '/' || c == '>',
    }
}

// ── Comment-anchor OCCURRENCE PATH ───────────────────────────────────────────
// CONSISTENCY INVARIANT: the comment-anchor occurrence path MUST be computed over the same normalized
// AST view as the structural signature; any structural normalization (empty-statement filtering, paren
// transparency, future list rewrites) must be mirrored in anchor indexing OR introduce a synthetic
// carrier for comments attached to removed nodes. (Today: `statements_sig` filters list-context
// `EmptyStatement`, and `CommentAnchorIndex::normalize_statement_list` mirrors that filter — a real
// statement is indexed by its LOGICAL filtered index and a comment attached to a filtered empty gets a
// synthetic `empty_gap[<logical>.<empty_ordinal>]:EmptyStatement` anchor (the per-gap empty ordinal
// disambiguates CONSECUTIVE filtered empties at the same gap); `ParenthesizedExpression` is transparent
// on both sides. A divergence between the two normalizations would let a comment MOVE onto/off a
// normalized-away node compare EQUAL — the exact false-PASS this invariant forbids.)
//
// A comment's anchor is keyed by a DETERMINISTIC AST OCCURRENCE PATH — the sequence of child
// segments from the top-level statement INDEX down to the deepest node whose span boundary matches
// the comment's anchor byte — NOT by a structural shape (`stmt_sig`/`expr_sig`). The old
// structural-shape key collided whenever a semantic comment MOVED between two STRUCTURALLY-
// IDENTICAL positions (`/*@__PURE__*/ f(); f();` vs `f(); /*@__PURE__*/ f();` compared EQUAL,
// a false-PASS), and an intra-statement move inside e.g. a `$.template_effect(() => { … })` body
// collapsed onto the top-level statement's shape. The occurrence path makes every reachable anchor
// distinct: identical structural positions at different INDEXES produce different paths, and a move
// to a structurally-identical sibling changes an index segment.
//
// The path is built by a GENERIC OXC child-span walker (`CommentAnchorIndex`, an
// `oxc_ast_visit::Visit` impl) that walks top-level `Program.directives` and `program.body` and
// descends EVERY child node in ONE pass. The anchor is deterministic and collision-resistant over the
// NORMALIZED comparator view: `CommentAnchorIndex` indexes statements by the same empty-filtered
// logical view as `statements_sig`, node-types the segments, and gives comments attached to
// normalized-away empty statements an explicit synthetic `empty_gap[<logical>.<empty_ordinal>]` anchor.
// `CommentAnchorIndex` walks top-level `Program.directives` and `program.body`; descendants are reached
// through the generic `oxc_ast_visit::Visit` walker, including `FunctionBody.directives` and every
// nested statement list (the `visit_statements` override applies the same empty-filtered normalization
// there). A future anchor collapse inside that normalized view is a comparator bug, not D-17 debt.
// There is no hand-enumerated per-edge descent and therefore no per-edge round can reopen the class.
//
// PATH SCHEME: a top-level DIRECTIVE prologue node (`Program.directives[<i>]`) keeps the segment
// `dir[<i>]` and a top-level statement node keeps the segment `stmt[<logical>]:<AstType>` (its LOGICAL
// empty-filtered index in `program.body`, node-typed); the two are independent index spaces with
// distinct prefixes (no collision). A list-context `EmptyStatement` that `statements_sig` filters
// consumes NO statement/child index and instead records a synthetic
// `empty_gap[<logical>.<empty_ordinal>]:EmptyStatement` candidate (the `<empty_ordinal>` keeps
// CONSECUTIVE filtered empties at the same gap distinct), so a comment attached to a filtered empty
// anchors distinctly from the next real statement and from a sibling empty in the same gap.
// BELOW either, each ENTERED OXC node contributes a `/`-joined segment
// `child[<k>]:<AstType>` where `<k>` is the zero-based ENTERED-CHILD INDEX under the current parent
// (children are counted in `Visit` enter order, EXCLUDING filtered list-context empties) and `<AstType>`
// is `format!("{:?}", kind.ty())` (`Debug` on the plain `AstType` enum, NOT on `AstKind`, which carries
// node payload + spans). So a nested object value, a computed/static key, a formal param, a
// param/destructuring default, a yield argument, an arrow/function body statement, and a named-export
// function body statement ALL get a concrete deep path; a move between two structurally-identical
// siblings changes a `child[<k>]` (or `stmt[<logical>]`) index and FAILS. `child[<k>]:<AstType>` is
// unique within a parent (the entered child index never repeats), and identical structural siblings
// differ by an ancestor `child[<k>]`/`stmt[<logical>]` — so the path is deterministic and
// collision-resistant over the normalized comparator view.
//
// PAREN TRANSPARENCY (the cosmetic-redundant-paren waiver): a `ParenthesizedExpression` is descended
// WITHOUT recording a candidate and WITHOUT consuming a child index — its inner expression is
// visited under the SAME parent path/child-index, so `(a)` and `a` produce the same anchor. For a
// LEADING comment whose `attached_to` byte is an outer paren's START, the leading resolver aliases
// that paren start to the innermost unwrapped node's start (recorded as the walk descends each
// paren), so `/*! keep */ (a)` anchors exactly like `/*! keep */ a`.
//
// The path reads AST topology, not bytes, so the anchor is whitespace- and redundant-paren
// insensitive, yet POSITION-sensitive (a move changes an index). The structural `stmt_sig`/`expr_sig`
// are NOT used for identity here — the path plus `pos=Leading|Trailing` (plus the per-anchor `ord`)
// fully identifies the occurrence.

use oxc_ast::AstKind;
use oxc_ast_visit::Visit;

/// One recorded anchor candidate: the node's span boundaries, its path DEPTH (number of segments,
/// for the deepest-wins tiebreak), and its full occurrence path
/// (`stmt[<logical>]:<AstType>/child[<k>]:<AstType>/…`, or a synthetic
/// `empty_gap[<logical>.<empty_ordinal>]:EmptyStatement`).
#[derive(Clone)]
struct AnchorCandidate {
    start: u32,
    end: u32,
    depth: usize,
    path: String,
}

/// Which TOP-LEVEL segment space the current depth-0 walk belongs to — a `Program.directives[<i>]`
/// prologue (`dir[<i>]`) or a `program.body` statement (`stmt[<logical>]:<AstType>`, the LOGICAL
/// empty-filtered index). The two are independent index spaces with distinct prefixes, so a directive
/// anchor and a body anchor never collide.
#[derive(Clone, Copy)]
enum TopSegmentKind {
    Directive,
    Stmt,
}

/// A GENERIC OXC child-span anchor index, built ONCE per module. It walks top-level
/// `Program.directives` then `program.body` with [`oxc_ast_visit::Visit`], recording a candidate for
/// EVERY entered node (keyed by the node's span via [`oxc_span::GetSpan`]) plus, for each
/// `ParenthesizedExpression`, an alias from the paren start to the innermost unwrapped node start. A
/// comment's anchor is resolved from the recorded candidates by `(attached_to / comment_start,
/// CommentPosition)` — see [`CommentAnchorIndex::anchor_for`].
struct CommentAnchorIndex {
    candidates: Vec<AnchorCandidate>,
    /// `paren_start -> innermost-non-paren inner start`, so a LEADING comment whose `attached_to`
    /// lands on a redundant outer paren resolves to the unwrapped node (the cosmetic-paren waiver).
    paren_alias: std::collections::HashMap<u32, u32>,
    /// The path segments from the current top-level node down to the node being entered.
    path_stack: Vec<String>,
    /// One entered-child counter per frame on `path_stack` (the count of children entered so far
    /// under that frame's node). `child_counter.len() == path_stack.len()` between nodes.
    child_counter: Vec<u32>,
    /// The index of the top-level node currently being walked (drives the depth-0 segment): a
    /// directive's SOURCE index for `dir[<i>]`, or a body statement's LOGICAL empty-filtered index for
    /// `stmt[<logical>]:<AstType>` (set by `normalize_statement_list`).
    top_index: usize,
    /// Whether the current depth-0 walk is a directive prologue or a body statement (drives the
    /// depth-0 segment PREFIX — `dir[` vs `stmt[`).
    top_segment_kind: TopSegmentKind,
}

impl CommentAnchorIndex {
    /// Build the index for a module's TOP-LEVEL nodes. The directive prologue (`Program.directives`)
    /// is walked FIRST as the `dir[<i>]` segment space, then the statement body (`program.body`) is
    /// normalized through `normalize_statement_list` as the `stmt[<logical>]:<AstType>` segment space
    /// (list-context empties filtered to synthetic `empty_gap[<logical>.<empty_ordinal>]` anchors) — the
    /// two are independent index spaces with distinct prefixes. Each top-level node is walked in source
    /// order with its index + segment-kind established; the generic `Visit` impl records every descendant
    /// node's `(span, path)` candidate (so `FunctionBody.directives` are reached once a function body is
    /// descended).
    fn build(program: &oxc_ast::ast::Program<'_>) -> Self {
        let mut index = CommentAnchorIndex {
            candidates: Vec::new(),
            paren_alias: std::collections::HashMap::new(),
            path_stack: Vec::new(),
            child_counter: Vec::new(),
            top_index: 0,
            top_segment_kind: TopSegmentKind::Stmt,
        };
        // Directive prologue FIRST (`dir[<i>]`). `visit_directive` fires `enter_node` on the
        // `AstKind::Directive` node at depth 0 → segment `dir[<i>]` (gated on `top_segment_kind`);
        // its inner string literal enters at depth 1 → `child[<k>]`.
        for (i, dir) in program.directives.iter().enumerate() {
            index.top_index = i;
            index.top_segment_kind = TopSegmentKind::Directive;
            debug_assert!(index.path_stack.is_empty() && index.child_counter.is_empty());
            index.visit_directive(dir);
        }
        // Statement body SECOND (`stmt[<logical>]:<AstType>`). The body is normalized by the SAME
        // per-list helper the nested-list `visit_statements` override uses: list-context
        // `EmptyStatement`s are filtered (each gets a synthetic `empty_gap[<logical>.<empty_ordinal>]`
        // anchor and consumes no index) so the anchor index is computed over the SAME empty-filtered view
        // as `statements_sig`.
        // `visit_statement` dispatches straight to the concrete statement node's `visit_*` (the
        // `Statement` enum has no `AstKind`), so the FIRST `enter_node` is the statement node itself at
        // depth 0 → segment `stmt[<logical>]:<AstType>`; descendants enter at depth ≥ 1 → `child[<k>]`.
        // (`build` iterates the top-level body MANUALLY rather than via `visit_program`, so the
        // `visit_statements` override fires only for the NESTED lists reached during descent — both use
        // `normalize_statement_list`, so they cannot drift.)
        index.top_segment_kind = TopSegmentKind::Stmt;
        debug_assert!(index.path_stack.is_empty() && index.child_counter.is_empty());
        index.normalize_statement_list(&program.body);
        index
    }

    /// Record a candidate for an entered node at the CURRENT path.
    fn record(&mut self, span: oxc_span::Span) {
        self.candidates.push(AnchorCandidate {
            start: span.start,
            end: span.end,
            depth: self.path_stack.len(),
            path: self.path_stack.join("/"),
        });
    }

    /// Record a SYNTHETIC candidate for a list-context `EmptyStatement` that `statements_sig` filters
    /// out, so a semantic comment attached to that removed empty resolves to an explicit
    /// `empty_gap[<logical>.<empty_ordinal>]:EmptyStatement` anchor (distinct from the next real
    /// statement's `stmt[<logical>]:<AstType>` / `child[<k>]:<AstType>`) instead of collapsing onto an
    /// unrelated node. The `<empty_ordinal>` is the position of this empty within its run of CONSECUTIVE
    /// filtered empties at the same `<logical>` gap, so a semantic comment moved among `;;` carriers
    /// keeps a DISTINCT anchor (the consecutive empties cannot collide on one `<logical>` index). The
    /// synthetic segment is pushed onto the path (so the recorded path carries its full ancestor chain)
    /// but consumes NO `child_counter` index and is NOT descended — a filtered empty has no children and
    /// occupies no child/statement slot, mirroring `statements_sig`'s filter.
    fn record_empty_gap(&mut self, span: oxc_span::Span, logical: usize, empty_ordinal: usize) {
        self.path_stack.push(format!(
            "empty_gap[{logical}.{empty_ordinal}]:EmptyStatement"
        ));
        self.record(span);
        self.path_stack.pop();
    }

    /// Walk a statement LIST under the SAME normalization `statements_sig` applies: list-context
    /// `EmptyStatement` nodes are filtered (they consume no statement/child index and get a synthetic
    /// `empty_gap[<logical>.<empty_ordinal>]` anchor), and each REAL statement is indexed by its LOGICAL
    /// (empty-filtered) index. `top_index` is set to the running `logical_index` before each real
    /// statement so a TOP-LEVEL statement's depth-0 segment is `stmt[<logical>]:<AstType>` (`top_index`
    /// is ignored at depth ≥ 1, where `enter_node` uses the `child_counter`). This is the SINGLE per-list
    /// normalizer shared by the manual top-level `build` loop and the `visit_statements` override for
    /// nested lists, so the two paths cannot drift. (REQUIRED-position empties — `for(;;);` / `while(c);`
    /// / `if(c);` — are reached via `stmt_sig`/the concrete-node visit, NOT a statement list, so they are
    /// never seen here and stay signed.)
    fn normalize_statement_list<'a>(&mut self, stmts: &oxc_allocator::Vec<'a, Statement<'a>>) {
        let mut logical_index = 0usize;
        // The per-gap ordinal of the current run of consecutive filtered empties at `logical_index`
        // (reset to 0 each time a real statement consumes a logical index). This disambiguates a
        // semantic comment's position AMONG consecutive no-op `;` carriers in the SAME gap, so
        // `/*c*/ ; ; f()` and `; /*c*/ ; f()` resolve DISTINCT anchors (the per-gap empty ordinal).
        let mut empty_ordinal = 0usize;
        for stmt in stmts.iter() {
            if let Statement::EmptyStatement(e) = stmt {
                self.record_empty_gap(e.span, logical_index, empty_ordinal);
                empty_ordinal += 1;
                continue; // consumes no logical index — mirrors `statements_sig`'s filter.
            }
            self.top_index = logical_index;
            self.visit_statement(stmt);
            logical_index += 1;
            empty_ordinal = 0;
        }
    }

    /// The deterministic occurrence-path anchor for a comment. `Leading` resolves the DEEPEST
    /// candidate whose `span.start` equals `attached_to` (after aliasing a redundant outer-paren
    /// start to the unwrapped inner start). `Trailing` resolves the PRECEDING node — among candidates
    /// with `span.end <= comment_start`, the one with the largest `end` (then deeper path, then
    /// lexically-larger path) — because OXC leaves `attached_to = 0` for trailing comments (anchoring
    /// on it would collapse every trailing comment onto the first statement). A comment with no
    /// matching candidate (an EOF directive on an empty module, or a leading byte that starts no node)
    /// anchors to `<tail>`.
    fn anchor_for(
        &self,
        attached_to: u32,
        comment_start: u32,
        position: CommentPosition,
    ) -> String {
        let (pos, body) = match position {
            CommentPosition::Leading => {
                // Alias a redundant outer-paren start to the innermost unwrapped node start, so a
                // leading comment on `(a)` anchors like one on bare `a` (the cosmetic-paren waiver).
                let target = self
                    .paren_alias
                    .get(&attached_to)
                    .copied()
                    .unwrap_or(attached_to);
                let mut best: Option<&AnchorCandidate> = None;
                for c in &self.candidates {
                    if c.start != target {
                        continue;
                    }
                    // Deepest wins; tie → larger `end`; tie → lexically-larger path.
                    let better = match best {
                        None => true,
                        Some(prev) => {
                            (c.depth, c.end, &c.path) > (prev.depth, prev.end, &prev.path)
                        }
                    };
                    if better {
                        best = Some(c);
                    }
                }
                ("Leading", best.map(|c| c.path.clone()))
            }
            CommentPosition::Trailing => {
                let mut best: Option<&AnchorCandidate> = None;
                for c in &self.candidates {
                    if c.end > comment_start {
                        continue;
                    }
                    // Closest preceding node = largest `end` (selected by `end`, NOT path depth, so
                    // it is correct for ≥10 siblings: a depth/lexicographic tiebreak would let
                    // `stmt[1]` beat `stmt[10]`); tie → deeper path; tie → lexically-larger path.
                    let better = match best {
                        None => true,
                        Some(prev) => {
                            (c.end, c.depth, &c.path) > (prev.end, prev.depth, &prev.path)
                        }
                    };
                    if better {
                        best = Some(c);
                    }
                }
                ("Trailing", best.map(|c| c.path.clone()))
            }
        };
        let body = body.unwrap_or_else(|| "<tail>".to_string());
        format!("pos={pos}/{body}")
    }
}

impl<'a> Visit<'a> for CommentAnchorIndex {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        // The segment for THIS node: a depth-0 node is the top-level directive prologue (`dir[<i>]`)
        // or statement (`stmt[<i>]:<AstType>`), prefixed by the active `top_segment_kind`; a deeper
        // node is `child[<k>]:<AstType>` for the next entered-child index `k` of its parent. The
        // top-level statement segment uses the LOGICAL (empty-filtered) index (`top_index` is set to
        // the per-list `logical_index` by `normalize_statement_list`) and node-types the segment, so it
        // is computed over the SAME normalized view as `statements_sig` (which filters list-context
        // empties) and matches the `child[<k>]:<AstType>` scheme.
        let segment = if self.path_stack.is_empty() {
            match self.top_segment_kind {
                TopSegmentKind::Directive => format!("dir[{}]", self.top_index),
                TopSegmentKind::Stmt => format!("stmt[{}]:{:?}", self.top_index, kind.ty()),
            }
        } else {
            let counter = self
                .child_counter
                .last_mut()
                .expect("child_counter has a frame for the parent node");
            let k = *counter;
            *counter += 1;
            format!("child[{k}]:{:?}", kind.ty())
        };
        self.path_stack.push(segment);
        self.child_counter.push(0);
        self.record(kind.span());
    }

    fn leave_node(&mut self, _kind: AstKind<'a>) {
        self.path_stack.pop();
        self.child_counter.pop();
    }

    fn visit_parenthesized_expression(&mut self, it: &oxc_ast::ast::ParenthesizedExpression<'a>) {
        // PAREN TRANSPARENCY (leading + trailing): do NOT enter the paren node (no candidate, no
        // `child[<k>]` segment, no child-index bump) — descend its inner expression under the SAME
        // parent frame so `(a)` and `a` produce the same anchor. Record the paren-start →
        // innermost-unwrapped-start alias so a LEADING comment whose `attached_to` is the paren start
        // resolves to the unwrapped node.
        let inner = unwrap_parens(&it.expression);
        let inner_end = inner.span().end;
        self.paren_alias.insert(it.span.start, inner.span().start);
        // Visit the immediate inner expression directly (a nested paren recurses through this same
        // override, chaining its own alias to the same innermost start). `visit_expression` does not
        // enter an `AstKind` for the `Expression` enum wrapper — it dispatches to the inner concrete
        // node's `enter_node`, which takes the next child index under the current parent.
        //
        // Then make trailing transparency SYMMETRIC. A TRAILING comment resolves by the closest
        // preceding node's `end`. The inner expression's candidates end BEFORE the outer `)`, but the
        // ancestor carrier ends AT `)`, so a trailing comment after `(a)` would otherwise pick the
        // ancestor while the same comment after bare `a` picks the inner node — a cosmetic-paren
        // false-FAIL (reachable when the comment is newline/ASI-terminated, where OXC keeps it
        // Trailing). For each candidate recorded DURING this paren's inner descent whose `end` is the
        // inner expression's end, add a synthetic copy with the SAME path/depth but re-ended at the
        // outer paren's `)`. The trailing resolver sorts by `(end, depth, path)`, so the synthetic
        // candidate at `end = )` with the deeper inner path beats the ancestor at `end = )` via the
        // depth tiebreak — the inner node is also reachable as the closest-preceding candidate at the
        // paren `)`, matching the bare side's anchor exactly.
        let before = self.candidates.len();
        self.visit_expression(&it.expression);
        let trailing_aliases: Vec<AnchorCandidate> = self.candidates[before..]
            .iter()
            .filter(|c| c.end == inner_end)
            .map(|c| AnchorCandidate {
                start: c.start,
                end: it.span.end,
                depth: c.depth,
                path: c.path.clone(),
            })
            .collect();
        self.candidates.extend(trailing_aliases);
    }

    fn visit_statements(&mut self, it: &oxc_allocator::Vec<'a, Statement<'a>>) {
        // NESTED list-context normalization. `oxc_ast_visit` routes EVERY nested statement list —
        // `BlockStatement.body`, `FunctionBody.statements`, `SwitchCase.consequent`, `StaticBlock.body`
        // — through `visit_statements`, so overriding it (instead of the default `walk_statements`)
        // applies the SAME empty-filtered normalization `statements_sig` uses for those lists: a
        // list-context `EmptyStatement` is filtered (synthetic `empty_gap[<logical>.<empty_ordinal>]`
        // anchor, no child index) and each real statement gets its LOGICAL (empty-filtered) `child[<k>]`
        // index. Routed through the shared `normalize_statement_list` so the nested and top-level views
        // cannot drift. NOT calling `walk_statements` is deliberate — the default would re-walk empties
        // with raw child indices, reopening the filter/anchor mismatch inside nested bodies.
        // (Required-position empties are NOT lists and never reach here.)
        self.normalize_statement_list(it);
    }
}

/// The semantic-comment signature of a parsed module — a SORTED multiset of
/// `Comment(class=…,text=…,anchor=…,ord=…)` entries, one per SEMANTIC comment. Cosmetic comments
/// are dropped. The `anchor` is the comment's DETERMINISTIC AST OCCURRENCE PATH (resolved from the
/// generic `CommentAnchorIndex` child-span walker), which carries `pos=Leading|Trailing`: a `Leading`
/// comment anchors via OXC's `attached_to` byte, a `Trailing` comment via the PRECEDING node's
/// occurrence path (OXC computes `attached_to` for
/// LEADING comments only — it leaves trailing comments' `attached_to = 0`, so anchoring a trailing
/// comment on it would collapse every trailing comment onto the first statement). `ord` is the
/// per-anchor ordinal (source order of semantic comments sharing the SAME occurrence path), so two
/// distinct semantic comments at the same anchor stay distinct and order is preserved within an
/// anchor. Sorting makes the comparison a multiset (occurrence identity, not global text order, is
/// what matters) while `ord` keeps per-anchor order significant. `code` is the raw module source;
/// comment token text is compared EXACTLY (delimiters included) modulo line-ending normalization.
fn comment_sig(
    code: &str,
    comments: &oxc_allocator::Vec<'_, Comment>,
    program: &oxc_ast::ast::Program<'_>,
) -> Vec<String> {
    // Stable source order for deterministic ordinals.
    let mut ordered: Vec<&Comment> = comments.iter().collect();
    ordered.sort_by_key(|c| (c.span.start, c.span.end));

    // Build the generic child-span anchor index ONCE per module (not per comment): it walks the
    // top-level directive prologue then the statement body and records every descendant node's
    // occurrence-path candidate, then each comment's anchor is resolved from those candidates by
    // `(attached_to / span.start, position)`.
    let anchor_index = CommentAnchorIndex::build(program);

    let mut ord_at_anchor: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut entries: Vec<String> = Vec::new();
    for c in ordered {
        let raw = &code[c.span.start as usize..c.span.end as usize];
        let Some(class) = semantic_comment_class(c, raw) else {
            continue; // cosmetic — waived, dropped from the signature.
        };
        // Branch on `CommentPosition`: leading resolves the deepest candidate starting at
        // `attached_to` (paren-aliased); trailing resolves the preceding node keyed off the comment's
        // start byte (OXC leaves `attached_to = 0` for trailing comments — see `anchor_for`).
        let anchor = anchor_index.anchor_for(c.attached_to, c.span.start, c.position);
        let ord = {
            let slot = ord_at_anchor.entry(anchor.clone()).or_insert(0);
            let v = *slot;
            *slot += 1;
            v
        };
        // Exact comment text, line-ending normalized (CRLF -> LF) so a checked-out EOL difference
        // does not false-fail; NO trim / whitespace-collapse (license/JSDoc/source-map payload
        // bytes are in contract).
        let text = raw.replace("\r\n", "\n").replace('\r', "\n");
        entries.push(format!(
            "Comment(class={class},text={text:?},anchor={anchor:?},ord={ord})"
        ));
    }
    entries.sort();
    entries
}

/// The full conformance signature of a module — parses `code` ONCE with OXC and derives BOTH the
/// AST module signature and the semantic-comment signature from the same parse. A parse failure
/// panics with a clear message (callers already guard `parses_as_js`, so a failure here is a torn
/// module).
fn conformance_sig(code: &str, side: &str) -> ModuleConformanceSig {
    let alloc = Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let ret = oxc_parser::Parser::new(&alloc, code, source_type).parse();
    assert!(
        !ret.panicked && ret.errors.is_empty(),
        "the {side} module did not parse as JS (a hard FAIL):\n{code}\nerrors: {:?}",
        ret.errors
    );
    ModuleConformanceSig {
        module_sig: program_sig(&ret.program),
        comment_sig: comment_sig(code, &ret.program.comments, &ret.program),
    }
}

/// Assert the emitted module and the golden are STRUCTURALLY equal modulo redundant parens AND
/// carry the SAME in-contract semantic comments at the same occurrence-path anchors. A behavioral /
/// structural divergence (helper name, call-arg count, sequence split, literal content, operator,
/// identifier, member, statement order, import specifier) OR a semantic-comment drop / corruption /
/// move fails with a message naming the slug and showing both raw modules.
///
/// GOLDEN-SIDE CAVEAT: the `comment_sig` half is fully enforced on RAW module pairs (proven by
/// `svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`), but the COMMITTED
/// FIXTURE goldens are comment-STRIPPED at generation (`normalizeModuleForComparison`), so when this
/// helper runs against a committed golden the golden side's `comment_sig` is EMPTY — the FIXTURE gate
/// does NOT yet prove official-POSITIVE semantic-comment preservation. That golden-DATA oracle gap is
/// tracked by svelte-native-compiler-plan.md §8 D-19; nothing here implies the fixture gate has
/// official-positive semantic-comment coverage today.
fn assert_modules_structurally_equal(slug: &str, emitted: &str, golden: &str) {
    let emitted_sig = conformance_sig(emitted, "emitted");
    let golden_sig = conformance_sig(golden, "golden");
    assert_eq!(
        emitted_sig.module_sig, golden_sig.module_sig,
        "STRUCTURAL drift for codegen cell {slug} (paren-insensitive, but argument/identifier/\
         operator/literal/structure/import-specifier-precise):\n--- emitted (raw) ---\n{emitted}\n\
         --- golden (raw) ---\n{golden}"
    );
    assert_eq!(
        emitted_sig.comment_sig, golden_sig.comment_sig,
        "SEMANTIC-COMMENT drift for codegen cell {slug} (the in-contract comment boundary: \
         PURE-family / license-preserve / source-map / TS-directive / JSDoc; non-semantic \
         comments are waived):\n--- emitted (raw) ---\n{emitted}\n--- golden (raw) ---\n{golden}"
    );
}

// ── §1a comparator discrimination tests ──────────────────────────────────────
// These prove the structural comparator is NOT a gerrymander: a redundant-paren-only diff
// PASSES (the cosmetic-paren waiver), but every BEHAVIORAL / structural diff — a changed
// helper name, a changed call ARG COUNT, a sequence split into separate args, a changed
// string / template content, a changed operator, a changed identifier, an import-specifier
// drift, a semantic-comment drop/corruption/move — FAILS. Each test would catch the bug it
// names: it FAILS against the (intentionally diverged) "regression" module and PASSES against
// the (cosmetic-only-diverged) module.

/// Whether two JS modules have the SAME FULL conformance signature (paren-insensitive AST
/// structure AND in-contract semantic comments). Non-semantic comments are waived.
fn sigs_equal(a: &str, b: &str) -> bool {
    conformance_sig(a, "a").equals(&conformance_sig(b, "b"))
}

#[test]
fn struct_compare_waives_redundant_parens() {
    // A redundant author paren the official printer drops is a COSMETIC difference — the
    // comparator must treat it as EQUAL.
    assert!(
        sigs_equal(
            "$.html(node, () => (a, b));",
            "$.html(node, () => ((a, b)));"
        ),
        "a double-paren-wrapped sequence must compare EQUAL to a single-paren-wrapped one"
    );
    assert!(
        sigs_equal(
            "$.set_attribute(div, 'id', c ? a : b);",
            "$.set_attribute(div, 'id', (c ? a : b));"
        ),
        "a parenthesized conditional value must compare EQUAL to the bare one"
    );
    assert!(
        sigs_equal("var x = a + b + c;", "var x = (a + b) + c;"),
        "a same-precedence left-operand paren must compare EQUAL (the printer drops it)"
    );
    assert!(
        sigs_equal("var x = a + (b + c);", "var x = (a + (b + c));"),
        "a redundant outer paren around a kept inner paren must compare EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_changed_helper_name() {
    // A different helper name (`$.set_text` vs `$.set_attribute`) is a BEHAVIORAL divergence.
    assert!(
        !sigs_equal("$.set_text(div, x);", "$.set_attribute(div, x);"),
        "a changed helper name must FAIL the structural comparison"
    );
}

#[test]
fn struct_compare_fails_on_changed_call_arg_count() {
    // A different argument COUNT (the trailing `true` on `$.html`) is a BEHAVIORAL divergence.
    assert!(
        !sigs_equal("$.html(n, () => h);", "$.html(n, () => h, true);"),
        "a changed call argument count must FAIL the structural comparison"
    );
}

#[test]
fn struct_compare_fails_on_sequence_arg_split() {
    // The arg-split regression: a `SequenceExpression` as ONE argument
    // (`$.html(n, () => (a, b), true)` — 3 args) is structurally DISTINCT from two separate
    // arguments (`$.html(n, () => a, b, true)` — 4 args). Unwrapping parens must NOT merge the
    // sequence into the argument list. THIS is the planted behavioral diff the brief names.
    let three_args = "$.html(n, () => (a, b), true);";
    let four_args = "$.html(n, () => a, b, true);";
    assert!(
        !sigs_equal(three_args, four_args),
        "a 3-arg sequence call must FAIL against a 4-arg split call (the arg-split regression)"
    );
}

#[test]
fn struct_compare_fails_on_changed_string_or_template_content() {
    // A changed STRING content (`$$props.foo` vs `$$props.bar`) is a BEHAVIORAL divergence.
    assert!(
        !sigs_equal("var x = $$props.foo;", "var x = $$props.bar;"),
        "a changed member property must FAIL the structural comparison"
    );
    // A changed string literal value.
    assert!(
        !sigs_equal("var x = 'Hello';", "var x = 'Goodbye';"),
        "a changed string literal must FAIL the structural comparison"
    );
    // A changed TEMPLATE-literal quasi (text) content.
    assert!(
        !sigs_equal("var x = `Hello ${y}`;", "var x = `Goodbye ${y}`;"),
        "a changed template-literal quasi must FAIL the structural comparison"
    );
}

#[test]
fn struct_compare_fails_on_changed_operator() {
    // A changed binary OPERATOR (`a + b` vs `a - b`) is a BEHAVIORAL divergence.
    assert!(
        !sigs_equal("var x = a + b;", "var x = a - b;"),
        "a changed binary operator must FAIL the structural comparison"
    );
}

#[test]
fn struct_compare_fails_on_changed_identifier() {
    // A changed identifier — a raw `count` read vs the `$.get(count)`-rewritten one — is a
    // BEHAVIORAL divergence (a dropped reactive read).
    assert!(
        !sigs_equal(
            "$.set_text(node, count);",
            "$.set_text(node, $.get(count));"
        ),
        "a raw identifier vs a $.get-wrapped read must FAIL the structural comparison"
    );
}

#[test]
fn struct_compare_fails_on_swapped_arrow_param_order() {
    // PARAMETER-ORDER swap: a memoized `$.template_effect` arrow binds each `$N` deps-array
    // slot POSITIONALLY, so `($0, $1) => … ${$0} … ${$1}` and `($1, $0) => … ${$0} … ${$1}`
    // have a BYTE-IDENTICAL body but RE-BIND the dep values — a real behavior change. The
    // structural comparator MUST distinguish them (a count-only param signature would not).
    let correct =
        "$.template_effect(($0, $1) => $.set_text(t, `${$0} ${$1}`), [() => a(), () => b()]);";
    let swapped =
        "$.template_effect(($1, $0) => $.set_text(t, `${$0} ${$1}`), [() => a(), () => b()]);";
    assert!(
        !sigs_equal(correct, swapped),
        "a swapped arrow PARAMETER ORDER must FAIL the structural comparison (positional dep re-bind)"
    );
    // A renamed parameter is likewise distinct (the name is part of the binding identity).
    assert!(
        !sigs_equal("($0) => $0 + 1;", "($x) => $x + 1;"),
        "a renamed arrow parameter must FAIL the structural comparison"
    );
    // SANITY: identical params still compare EQUAL modulo a redundant body paren (no false fail).
    assert!(
        sigs_equal("($0, $1) => ($0 + $1);", "($0, $1) => $0 + $1;"),
        "identical params with a redundant body paren must still compare EQUAL"
    );
}

#[test]
fn struct_compare_waives_redundant_paren_in_destructuring_default() {
    // The DESTRUCTURING-DEFAULT cosmetic-paren waiver (the false-FAIL fix). A destructuring binding
    // default (`{ a = 1 }`) carries an `AssignmentPattern.right` that is an `Expression`; a redundant
    // author paren around that default (`{ a = (1) }`) is a COSMETIC difference the official printer
    // drops. The pre-fix `binding_sig` encoded the whole pattern via span-stripped Debug, which does
    // NOT peel a `ParenthesizedExpression` wrapper, so `{ a = (1) }` and `{ a = 1 }` produced DIFFERENT
    // signatures — a cosmetic false-FAIL. The recursive `binding_sig` routes the default through the
    // paren-transparent `expr_sig`, so they now compare EQUAL. Reachable: an author destructuring
    // default inside a source-preserved value position (a `{@html}` / dynamic-value arrow the emitter
    // byte-copies) carries exactly this redundant paren.
    assert!(
        sigs_equal("var { a = (1) } = o;", "var { a = 1 } = o;"),
        "a redundant paren around an OBJECT-destructuring default must compare EQUAL (the default is \
         encoded via the paren-transparent expr_sig; the pre-fix Debug fallback false-FAILED it)"
    );
    // The ARRAY-destructuring default variant (`[a = (1)]` vs `[a = 1]`) is likewise paren-transparent.
    assert!(
        sigs_equal("var [ a = (1) ] = o;", "var [ a = 1 ] = o;"),
        "a redundant paren around an ARRAY-destructuring default must also compare EQUAL"
    );
    // A NESTED-pattern default (`{ a: { b = (1) } }`) recurses through `binding_sig` and stays
    // paren-transparent at every nesting level (the recursion is not shallow).
    assert!(
        sigs_equal("var { a: { b = (1) } } = o;", "var { a: { b = 1 } } = o;"),
        "a redundant paren around a NESTED destructuring default must compare EQUAL (binding_sig \
         recurses into the nested pattern and routes each default through expr_sig)"
    );
    // An assignment-EXPRESSION destructuring target (`({ a = (1) } = o)`) reaches
    // `assignment_target_sig`, which codex requires to be paren-transparent too (not Debug). The
    // `AssignmentTargetWithDefault.init` default `(1)` must compare EQUAL to the bare `1`.
    assert!(
        sigs_equal("({ a = (1) } = o);", "({ a = 1 } = o);"),
        "a redundant paren in an assignment-EXPRESSION destructuring default must compare EQUAL \
         (assignment_target_sig routes the default through expr_sig; the pre-fix Debug fallback \
         false-FAILED it)"
    );
}

#[test]
fn struct_compare_fails_on_destructuring_property_reorder() {
    // TRUE-POSITIVE preserved: a real destructuring PROPERTY REORDER must still FAIL. Two object
    // patterns differing only by property ORDER (`{a, b}` vs `{b, a}`) bind the same names but are a
    // structural difference the recursive `binding_sig` must keep distinct (it encodes the ordered
    // property list, not a name set).
    assert!(
        !sigs_equal("var { a, b } = o;", "var { b, a } = o;"),
        "an OBJECT-destructuring property reorder must FAIL (binding_sig encodes the ordered property \
         list, not an unordered name set)"
    );
    // The ARRAY-destructuring element REORDER (`[a, b]` vs `[b, a]`) is likewise distinct — array
    // position is binding identity.
    assert!(
        !sigs_equal("var [ a, b ] = o;", "var [ b, a ] = o;"),
        "an ARRAY-destructuring element reorder must FAIL (array element position is binding identity)"
    );
    // A nested-pattern KEY rename (`{ a: { b } }` vs `{ a: { c } }`) recurses and stays distinct.
    assert!(
        !sigs_equal("var { a: { b } } = o;", "var { a: { c } } = o;"),
        "a nested destructuring binding rename must FAIL (binding_sig recurses into the nested value \
         pattern)"
    );
}

#[test]
fn struct_compare_fails_on_destructuring_default_drop() {
    // TRUE-POSITIVE preserved: dropping a destructuring DEFAULT must still FAIL. `{ a = 1 }` binds
    // `a` to `1` when the source value is `undefined`; `{ a }` does not — a real behavior change the
    // recursive `binding_sig` must keep distinct (the `AssignmentPattern` wrapper vs a bare
    // `BindingIdentifier`).
    assert!(
        !sigs_equal("var { a = 1 } = o;", "var { a } = o;"),
        "an OBJECT-destructuring default DROP must FAIL (a defaulted binding is structurally distinct \
         from a bare binding)"
    );
    // The ARRAY-destructuring default drop (`[a = 1]` vs `[a]`) is likewise distinct.
    assert!(
        !sigs_equal("var [ a = 1 ] = o;", "var [ a ] = o;"),
        "an ARRAY-destructuring default DROP must FAIL"
    );
    // And the default VALUE is significant (`{ a = 1 }` vs `{ a = 2 }`) — a different default binds a
    // different value, caught by the paren-transparent `expr_sig` on the default.
    assert!(
        !sigs_equal("var { a = 1 } = o;", "var { a = 2 } = o;"),
        "a changed destructuring default VALUE must FAIL (the default expr_sig differs)"
    );
}

#[test]
fn struct_compare_waives_leading_comment_redundant_paren() {
    // A semantic comment leading a REDUNDANT outer paren (`/*! keep */ (a)`) must anchor IDENTICALLY
    // to the same comment leading the bare inner node (`/*! keep */ a`): the redundant paren is a
    // COSMETIC difference the official printer drops, so a paren-only difference carrying the SAME
    // semantic comment must compare EQUAL (the cosmetic-paren waiver). The OLD `expr_anchor` peeled
    // the paren BEFORE the leading-anchor probe, so `(a)` collapsed to `<tail>` while `a` anchored to
    // `decl[0].init` — a false-FAIL the paren-transparent leading remap fixes.
    assert!(
        sigs_equal("var x = /*! keep */ (a);", "var x = /*! keep */ a;"),
        "a redundant-paren-only difference carrying the same leading semantic comment must compare \
         EQUAL (paren-transparent leading anchor; the pre-fix descent false-FAILED this)"
    );
    // A NESTED redundant-paren variant must likewise remap transparently to the inner node.
    assert!(
        sigs_equal("var x = /*! keep */ ((a));", "var x = /*! keep */ a;"),
        "a nested redundant-paren leading semantic comment must also compare EQUAL (every transparent \
         paren layer is peeled without adding a segment)"
    );
    // A paren leading a deeper expression still descends into the unwrapped node (no collapse): the
    // comment leads `(f(a))`, which remaps to `f(a)` and descends to the callee, matching `f(a)`.
    assert!(
        sigs_equal("var x = /*! keep */ (f(a));", "var x = /*! keep */ f(a);"),
        "a leading semantic comment on a parenthesized call must remap to the unwrapped call and \
         still descend (no shallow collapse)"
    );
}

#[test]
fn struct_compare_waives_trailing_comment_redundant_paren_before_asi() {
    // A semantic comment TRAILING a redundant-parenthesized expression, NEWLINE/ASI-terminated (where
    // OXC keeps the comment `Trailing`), must anchor IDENTICALLY to the bare-expression form — a
    // redundant paren is cosmetic. Pre-fix the trailing resolver picked the ancestor carrier (ending
    // at `)`) on the `(a)` side but the inner node (ending at `a`) on the bare side → different anchor
    // → a cosmetic-paren FALSE-FAIL. The symmetric trailing paren-transparency aliases close it.
    let paren = "var x = (a) /*! keep */\nvar y = 1;";
    let bare = "var x = a /*! keep */\nvar y = 1;";
    // Both must be genuinely Trailing (the mechanism under test).
    assert!(
        first_semantic_comment_anchor(paren).starts_with("pos=Trailing/"),
        "paren side must be Trailing"
    );
    assert!(
        first_semantic_comment_anchor(bare).starts_with("pos=Trailing/"),
        "bare side must be Trailing"
    );
    assert!(
        sigs_equal(paren, bare),
        "a trailing semantic comment after a redundant paren (ASI-terminated) must compare EQUAL to \
         the bare form (symmetric trailing paren-transparency)"
    );
    // And the anchors must be the SAME string (pin it, so the test is RED if the trailing aliases are removed).
    assert_eq!(
        first_semantic_comment_anchor(paren),
        first_semantic_comment_anchor(bare),
        "the trailing-comment anchor must be identical for `(a)` and bare `a` (paren-normalized end)"
    );
    // TRUE-POSITIVE preserved: a real trailing-comment MOVE to a DIFFERENT statement still FAILs.
    // Both sides are genuinely Trailing (newline/ASI-terminated), so this discriminates on two
    // distinct TRAILING anchors (not merely pos=Trailing-vs-Leading) — the exact axis the synthetic
    // trailing aliases could threaten if they over-collapsed. The synthetic candidate inherits the
    // inner node's full path (carrying the `stmt[N]` prefix), so a move across statements cannot
    // collapse.
    let move_a = "var x = (a) /*! keep */\nvar y = b\n";
    let move_b = "var x = a\nvar y = (b) /*! keep */\n";
    assert!(
        first_semantic_comment_anchor(move_a).starts_with("pos=Trailing/"),
        "move_a must be Trailing"
    );
    assert!(
        first_semantic_comment_anchor(move_b).starts_with("pos=Trailing/"),
        "move_b must be Trailing"
    );
    assert_ne!(
        first_semantic_comment_anchor(move_a),
        first_semantic_comment_anchor(move_b),
        "a real trailing-comment move across statements must produce DIFFERENT trailing anchors"
    );
    assert!(
        !sigs_equal(move_a, move_b),
        "a real trailing-comment move to a different statement must still FAIL (distinct trailing anchors)"
    );
    // NESTED variant: the symmetry must hold for nested paren descents too — a trailing comment after a
    // redundant paren INSIDE a function body (ASI-terminated, so OXC keeps it Trailing) anchors to the
    // deep inner node on BOTH sides. The synthetic trailing aliases minted by the inner paren descent
    // re-end at the inner paren's `)`, so the inner node is reachable as the closest-preceding
    // candidate regardless of nesting depth.
    let nested_paren = "var f = function () { return (a) /*! keep */\nreturn 1; };";
    let nested_bare = "var f = function () { return a /*! keep */\nreturn 1; };";
    assert!(
        first_semantic_comment_anchor(nested_paren).starts_with("pos=Trailing/"),
        "nested paren side must be Trailing"
    );
    assert!(
        first_semantic_comment_anchor(nested_bare).starts_with("pos=Trailing/"),
        "nested bare side must be Trailing"
    );
    assert_eq!(
        first_semantic_comment_anchor(nested_paren),
        first_semantic_comment_anchor(nested_bare),
        "the nested trailing-comment anchor must be identical for `(a)` and bare `a` (the symmetry \
         holds through nested paren descents)"
    );
    assert!(
        sigs_equal(nested_paren, nested_bare),
        "a nested trailing semantic comment after a redundant paren (ASI-terminated) must compare \
         EQUAL to the bare form"
    );
}

#[test]
fn struct_compare_fails_on_semantic_comment_move_between_computed_keys() {
    // The anchor descent walks object property KEYS (computed-key expressions) BEFORE values, so a
    // semantic comment MOVED between two structurally-identical COMPUTED keys anchors to a different
    // `object.prop[i].key.expr` path and FAILS. The pre-fix descent walked only `op.value` + spreads,
    // so a comment inside a computed key collapsed to `<tail>` and false-PASSED a move. Author object
    // literals inside source-preserved dynamic attr/class/style/`{@html}` expressions are byte-copied
    // by the emitter, so `{ [/*@__PURE__*/ k]: v }` is reachable.
    assert!(
        !sigs_equal(
            "var o = { [/*@__PURE__*/ f()]: v, [f()]: v };",
            "var o = { [f()]: v, [/*@__PURE__*/ f()]: v };"
        ),
        "a semantic comment moved between two structurally-identical COMPUTED keys must FAIL \
         (object property keys are descended; the pre-fix descent false-PASSED this)"
    );
    // SANITY (no false-FAIL): the SAME computed-key comment at the SAME key with a whitespace-only
    // difference still compares EQUAL.
    assert!(
        sigs_equal(
            "var o = { [/*@__PURE__*/ f()]: v };",
            "var o = { [  /*@__PURE__*/  f()]: v };"
        ),
        "the same computed-key semantic comment differing only by whitespace must still compare EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_semantic_comment_move_between_params() {
    // The anchor descent walks formal PARAMS / binding identifiers, so a semantic comment MOVED
    // between two params anchors to a different `arrow.params[k]` path and FAILS. The pre-fix descent
    // did not walk params, so a comment on a param collapsed to `<tail>` and false-PASSED a move.
    // Author dynamic values can contain `((/*! keep */ a) => a)`, source-preserved by the emitter.
    assert!(
        !sigs_equal(
            "var f = (/*! keep */ a, b) => a;",
            "var f = (a, /*! keep */ b) => a;"
        ),
        "a semantic comment moved between two arrow PARAMS must FAIL (formal params are descended; \
         the pre-fix descent false-PASSED this)"
    );
    // A function-expression param move is likewise distinct (`fn.params[k]`).
    assert!(
        !sigs_equal(
            "var f = function (/*! keep */ a, b) { return a; };",
            "var f = function (a, /*! keep */ b) { return a; };"
        ),
        "a semantic comment moved between two function-expression PARAMS must FAIL (fn.params[k] is \
         descended)"
    );
    // SANITY (no false-FAIL): the SAME comment at the SAME param position with a whitespace-only
    // difference still compares EQUAL.
    assert!(
        sigs_equal(
            "var f = (/*! keep */ a, b) => a;",
            "var f = (  /*! keep */  a, b) => a;"
        ),
        "the same param semantic comment differing only by whitespace must still compare EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_semantic_comment_move_between_param_defaults() {
    // The anchor descent now descends each `FormalParameter.initializer` at `arrow.params[k].init`
    // (mirroring `params_sig`, which already SIGNS `init=<expr_sig>`). A semantic comment MOVED
    // between two structurally-identical param DEFAULT expressions therefore anchors to a different
    // `[k].init` path and FAILS. The pre-fix `params_anchor` walked only the binding PATTERN, never
    // the initializer expression, so a comment inside a param default had NO occurrence path and
    // collapsed to `<tail>` on BOTH sides — a false-PASS for the move. Author dynamic values can
    // contain `(a = /*@__PURE__*/ g(), b = g()) => a`, source-preserved (byte-copied) by the emitter.
    assert!(
        !sigs_equal(
            "var f = (a = /*@__PURE__*/ g(), b = g()) => a;",
            "var f = (a = g(), b = /*@__PURE__*/ g()) => a;"
        ),
        "a semantic comment moved between two structurally-identical param DEFAULTS must FAIL \
         (FormalParameter.initializer is descended at arrow.params[k].init; the pre-fix descent \
         false-PASSED this)"
    );
    // The same defect on a FUNCTION-EXPRESSION param default (`fn.params[k].init`).
    assert!(
        !sigs_equal(
            "var f = function (a = /*@__PURE__*/ g(), b = g()) { return a; };",
            "var f = function (a = g(), b = /*@__PURE__*/ g()) { return a; };"
        ),
        "a semantic comment moved between two function-expression param DEFAULTS must FAIL \
         (fn.params[k].init is descended)"
    );
    // A comment on the binding ID stays DISTINCT from a comment in that same param's default — they
    // anchor at `arrow.params[k]` vs `arrow.params[k].init`, so the two positions do not collapse.
    assert!(
        !sigs_equal(
            "var f = (/*! keep */ a = g()) => a;",
            "var f = (a = /*! keep */ g()) => a;"
        ),
        "a comment on the param binding id vs inside its default must FAIL (arrow.params[k] vs \
         arrow.params[k].init are distinct anchors)"
    );
    // SANITY (no false-FAIL): the SAME default comment at the SAME default position with a
    // whitespace-only difference still compares EQUAL.
    assert!(
        sigs_equal(
            "var f = (a = /*@__PURE__*/ g(), b = g()) => a;",
            "var f = (a =   /*@__PURE__*/   g(), b = g()) => a;"
        ),
        "the same param-default semantic comment differing only by whitespace must still compare \
         EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_semantic_comment_move_between_destructuring_param_defaults() {
    // The GENERIC child-span anchor walker (`CommentAnchorIndex`) descends EVERY child node, so a
    // semantic comment inside a DESTRUCTURING param's default expression now has a concrete
    // occurrence path (it anchors to the default-expression node deep inside the `ObjectPattern`'s
    // `AssignmentPattern`). A comment MOVED between two structurally-identical destructuring defaults
    // therefore anchors to a different child-index path and FAILS. The pre-fix hand-enumerated
    // `binding_pattern_anchor` only offered the WHOLE pattern's span for any non-identifier pattern
    // (no descent into `ObjectPattern` property defaults), so the comment collapsed to the same
    // pattern anchor on BOTH sides — a false-PASS for the move. Author dynamic values can carry
    // `({ a = /*! keep */ g(), b = g() }) => a`, source-preserved (byte-copied) by the emitter. A
    // license `/*! keep */` (not a PURE annotation) is used so OXC sets NO `pure` node flag — the two
    // structural signatures stay byte-identical and the ANCHOR is the SOLE discriminator (a PURE
    // annotation would also flip a `pure` boolean the Debug fallback leaks, masking the anchor axis).
    assert!(
        !sigs_equal(
            "var f = ({ a = /*! keep */ g(), b = g() }) => a;",
            "var f = ({ a = g(), b = /*! keep */ g() }) => a;"
        ),
        "a semantic comment moved between two structurally-identical DESTRUCTURING param defaults \
         must FAIL (the generic child-span walker descends into the ObjectPattern default \
         expressions; the pre-fix hand descent false-PASSED this)"
    );
    // SANITY (no false-FAIL): the SAME destructuring-default comment at the SAME default with a
    // whitespace-only difference still compares EQUAL (the path reads AST topology, not bytes).
    assert!(
        sigs_equal(
            "var f = ({ a = /*! keep */ g(), b = g() }) => a;",
            "var f = ({ a =   /*! keep */   g(), b = g() }) => a;"
        ),
        "the same destructuring-param-default semantic comment differing only by whitespace must \
         still compare EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_semantic_comment_move_between_yield_arguments() {
    // The GENERIC child-span anchor walker descends EVERY child node, so a `YieldExpression`
    // argument is descended (the pre-fix hand-enumerated `expr_child_anchor` had NO `YieldExpression`
    // arm — a yield argument never descended, so a comment inside it collapsed to a shallower node).
    // A semantic comment MOVED between two structurally-identical `yield <arg>` arguments inside a
    // `function*` therefore anchors to a different child-index path and FAILS. Author dynamic values
    // can carry a source-preserved generator function literal the emitter byte-copies.
    assert!(
        !sigs_equal(
            "var f = function* () { yield /*! keep */ g(); yield g(); };",
            "var f = function* () { yield g(); yield /*! keep */ g(); };"
        ),
        "a semantic comment moved between two structurally-identical yield arguments inside a \
         function* must FAIL (the generic child-span walker descends the YieldExpression argument; \
         the pre-fix descent had no YieldExpression arm and false-PASSED this)"
    );
    // SANITY (no false-FAIL): the SAME yield-argument comment at the SAME yield with a
    // whitespace-only difference still compares EQUAL.
    assert!(
        sigs_equal(
            "var f = function* () { yield /*! keep */ g(); yield g(); };",
            "var f = function* () { yield   /*! keep */   g(); yield g(); };"
        ),
        "the same yield-argument semantic comment differing only by whitespace must still compare \
         EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_named_export_function_body_structural_diff() {
    // `decl_sig`'s `Declaration::FunctionDeclaration` arm now encodes the BODY
    // (`body={statements_sig(...)}`), matching the `Statement::FunctionDeclaration` and
    // `ExportDefaultFn` arms. Two named-export functions with the SAME name + params but DIFFERENT
    // bodies (`a()` vs `b()`) are a genuine structural divergence — the pre-fix arm encoded `params=`
    // only and collapsed them to the SAME signature (a structural false-PASS, and an honesty
    // mismatch: the comparator-scope mirror claimed named-export bodies were encoded). An ORACLE axis:
    // official Svelte client module output can carry module-script export declarations, so the
    // comparator must be correct here even though native Verter currently refuses `<script module>`.
    assert!(
        !sigs_equal(
            "export function f() { a(); }",
            "export function f() { b(); }"
        ),
        "two named-export functions with the same signature but different bodies must FAIL \
         (decl_sig now encodes the function body; the pre-fix params-only arm false-PASSED this)"
    );
    // SANITY (no false-FAIL): a byte-identical named-export function compares EQUAL, and a
    // redundant-paren-only body difference still compares EQUAL (the body is signed via the
    // paren-insensitive `statements_sig` → `expr_sig`).
    assert!(
        sigs_equal(
            "export function f() { return (a + b); }",
            "export function f() { return a + b; }"
        ),
        "a redundant-paren-only named-export function body difference must still compare EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_named_export_function_body_comment_move() {
    // The GENERIC child-span anchor walker descends nested statements (a function-declaration body's
    // statements get `child[k]:Statement-kind` segments), so a semantic comment MOVED between two
    // structurally-identical statements INSIDE a named-export function body anchors to a different
    // child-index path and FAILS. The pre-fix hand-enumerated `decl_anchor` only descended a
    // `VariableDeclaration` initializer (never a `FunctionDeclaration` body), so the comment
    // collapsed to `<tail>` on BOTH sides — a false-PASS for the move. An ORACLE axis: official client
    // module output can carry module-script export declarations, so the comparator must be correct
    // here even though native Verter currently refuses `<script module>`.
    assert!(
        !sigs_equal(
            "export function f() { /*@__PURE__*/ a(); b(); }",
            "export function f() { a(); /*@__PURE__*/ b(); }"
        ),
        "a semantic comment moved between two statements inside a named-export function body must \
         FAIL (the generic child-span walker descends the function body statements; the pre-fix \
         decl_anchor collapsed them and false-PASSED this)"
    );
    // SANITY (no false-FAIL): the SAME body comment at the SAME body statement with a
    // whitespace-only difference still compares EQUAL.
    assert!(
        sigs_equal(
            "export function f() { /*@__PURE__*/ a(); b(); }",
            "export function f() {   /*@__PURE__*/   a(); b(); }"
        ),
        "the same named-export-body semantic comment differing only by whitespace must still \
         compare EQUAL"
    );
}

#[test]
fn semantic_comment_class_requires_true_triple_slash_opener() {
    // A `// / <reference …` line comment (a line comment whose body merely begins `/ <reference` —
    // only TWO leading slashes) is NOT a triple-slash directive: it must stay WAIVED, so adding it to
    // one side compares EQUAL. The pre-fix classifier stripped `//` then accepted any single leading
    // `/`, so it wrongly classified this lookalike as a `<reference` directive (a cosmetic false-FAIL).
    assert!(
        sigs_equal("var x = 1;", "// / <reference path=\"x\" />\nvar x = 1;"),
        "`// / <reference …` is a non-directive lookalike (only two leading slashes) and must compare \
         EQUAL (true `///` opener required; the pre-fix classifier false-FAILED this)"
    );
    // A REAL triple-slash reference (exact `///` opener + `<reference` + boundary) is semantic: a
    // DROP must FAIL.
    assert!(
        !sigs_equal("/// <reference path=\"x\" />\nvar x = 1;", "var x = 1;"),
        "a dropped REAL `/// <reference …` triple-slash directive must FAIL (the `///` opener still \
         classifies the genuine directive)"
    );
}

#[test]
fn struct_compare_fails_on_static_object_key_comment_move() {
    // STATIC object-property KEY anchor: a semantic comment leading a STATIC key
    // (`{ /*! keep */ a: v, a: v }`) anchors to `object.prop[0].key`; moved before the second key
    // (`{ a: v, /*! keep */ a: v }`) it anchors to `object.prop[1].key`, so the MOVE must FAIL. The
    // pre-fix anchor descent walked only COMPUTED keys + values, so a comment before a static key
    // collapsed to `<tail>` on BOTH sides and false-PASSED the move. Object literals are
    // source-preserved (an author object inside a `{@html}` / dynamic-attr / class/style value is
    // byte-copied), so this is reachable.
    assert!(
        !sigs_equal(
            "var o = { /*! keep */ a: v, a: v };",
            "var o = { a: v, /*! keep */ a: v };"
        ),
        "a semantic comment moved between two structurally-identical STATIC object keys must FAIL \
         (object.prop[i].key anchor; the pre-fix descent false-PASSED this)"
    );
    // SANITY (no false-FAIL): the SAME static-key comment at the SAME key with a whitespace-only
    // difference still compares EQUAL.
    assert!(
        sigs_equal(
            "var o = { /*! keep */ a: v };",
            "var o = {   /*! keep */   a: v };"
        ),
        "the same static-key semantic comment differing only by whitespace must still compare EQUAL"
    );
}

/// Parse `code` and return the DETERMINISTIC anchor path of its FIRST semantic comment (mirrors the
/// exact parse + `CommentAnchorIndex` resolution `comment_sig` makes). Test-only helper for asserting
/// an anchor PATH directly when a `!sigs_equal` pair cannot isolate the path axis (e.g. the static-key
/// vs property-entry specificity, where any structural difference would mask the anchor difference).
fn first_semantic_comment_anchor(code: &str) -> String {
    let alloc = Allocator::default();
    let ret = oxc_parser::Parser::new(&alloc, code, oxc_span::SourceType::mjs()).parse();
    assert!(
        !ret.panicked && ret.errors.is_empty(),
        "the module did not parse as JS:\n{code}\nerrors: {:?}",
        ret.errors
    );
    let anchor_index = CommentAnchorIndex::build(&ret.program);
    let mut ordered: Vec<&Comment> = ret.program.comments.iter().collect();
    ordered.sort_by_key(|c| (c.span.start, c.span.end));
    for c in ordered {
        let raw = &code[c.span.start as usize..c.span.end as usize];
        if semantic_comment_class(c, raw).is_none() {
            continue; // cosmetic — skipped, exactly as `comment_sig` does.
        }
        return anchor_index.anchor_for(c.attached_to, c.span.start, c.position);
    }
    panic!("no semantic comment found in:\n{code}");
}

#[test]
fn struct_compare_anchors_static_key_comment_more_specifically_than_property_entry() {
    // SPECIFICITY (in generic-scheme terms): a leading comment immediately before a STATIC key
    // (`{ /*! keep */ a: v }`) starts at the SAME byte as the property itself (a plain `key: value`
    // property has no leading accessor token, so the `ObjectProperty` and its key `IdentifierName`
    // both START at the comment's `attached_to` byte). The GENERIC child-span walker records BOTH as
    // candidates at that byte, and the leading resolver picks the DEEPEST — the key `IdentifierName`
    // node (one segment deeper than the `ObjectProperty` it nests under). A leading comment before a
    // `get` ACCESSOR token starts at the property byte but the key `a` starts LATER (after `get `), so
    // the deepest candidate at the comment byte is the `ObjectProperty` node itself — strictly
    // SHALLOWER. The invariant the comparator preserves: a static-key comment anchors STRICTLY DEEPER
    // (a more specific node) than an accessor-token comment, so the accessor path is a PROPER PREFIX
    // of the static-key path.
    //
    // A `!sigs_equal` pair cannot isolate THIS axis: any object pair where the comment anchors at the
    // key on one side and the property entry on the other must differ structurally too (a plain
    // `a: v` vs a `get a(){}`), so the structural signature — not the anchor — would carry the FAIL.
    // The discriminating assertion is therefore the SPECIFICITY RELATIONSHIP between the two anchor
    // paths (expressed generically, robust to the exact `child[k]:AstType` segment naming), NOT a
    // literal curated path.
    let static_key_anchor = first_semantic_comment_anchor("var o = { /*! keep */ a: v };");
    let accessor_anchor = first_semantic_comment_anchor("var o = { /*! keep */ get a() {} };");
    // The two anchors are DISTINCT — the static key resolves to a different (more specific) node than
    // the accessor-token entry.
    assert_ne!(
        static_key_anchor, accessor_anchor,
        "the static-key anchor must be DISTINCT from the accessor-token entry anchor (the static key \
         resolves one node DEEPER); got static={static_key_anchor:?} accessor={accessor_anchor:?}"
    );
    // The accessor-token anchor is a PROPER PREFIX of the static-key anchor (the static key is the
    // accessor's `ObjectProperty` path PLUS one more child segment — the key node). This is the
    // scheme-independent form of "the static key is strictly more specific (deeper) than the property
    // entry": the entry path is an ancestor of the key path.
    assert!(
        static_key_anchor.starts_with(&accessor_anchor)
            && static_key_anchor.len() > accessor_anchor.len(),
        "the static-key anchor must be STRICTLY DEEPER than the accessor-token entry anchor (the \
         entry path is a proper prefix of the key path); got static={static_key_anchor:?} \
         accessor={accessor_anchor:?}"
    );
    // And concretely: the static-key path has MORE `/`-separated segments than the accessor path (it
    // descends one extra child node — the key identifier).
    let static_depth = static_key_anchor.matches('/').count();
    let accessor_depth = accessor_anchor.matches('/').count();
    assert!(
        static_depth > accessor_depth,
        "the static-key anchor path must have strictly more segments than the accessor-token entry \
         path (static_depth={static_depth} accessor_depth={accessor_depth}); got \
         static={static_key_anchor:?} accessor={accessor_anchor:?}"
    );
    // BEHAVIORAL sanity (the existing MOVE still FAILS, and no false-FAIL on whitespace): the
    // more-specific anchor keeps a static-key MOVE distinct and a whitespace-only diff EQUAL.
    assert!(
        !sigs_equal(
            "var o = { /*! keep */ a: v, a: v };",
            "var o = { a: v, /*! keep */ a: v };"
        ),
        "a static-key comment MOVE must still FAIL (the moved comment anchors under a different \
         object-property child index)"
    );
    assert!(
        sigs_equal(
            "var o = { /*! keep */ a: v };",
            "var o = {  /*! keep */  a: v };"
        ),
        "the same static-key comment differing only by whitespace must still compare EQUAL"
    );
}

#[test]
fn struct_compare_anchors_top_level_directive_comment_and_fails_on_move() {
    // The TOP-LEVEL-DIRECTIVE comment-anchor hole. A directive prologue (`"use strict";`) is stored
    // in `Program.directives`, NOT `Program.body`, so the pre-fix `CommentAnchorIndex::build` (which
    // walked only the body) gave a SEMANTIC comment leading/trailing a top-level directive NO anchor
    // candidate — it collapsed to `<tail>`. That made the in-source closure claim ("no walked node can
    // collapse to `<tail>`") FALSE for directive-bearing modules. Walking `Program.directives` (as
    // `dir[<i>]`) restores a real anchor. Use a license comment (`/*! keep */`) — NOT `/*@__PURE__*/`,
    // whose `pure` node flag can leak through other paths and mask the anchor axis.
    //
    // MOVE between two real anchors: the SAME semantic comment leading the directive (module A) vs
    // leading the first body statement (module B) anchors to DIFFERENT occurrence paths (`dir[0]`
    // vs `stmt[0]`-subtree), so the two modules are UNEQUAL. Pre-fix, A's comment collapsed to
    // `<tail>` (directive not walked), so the MOVE could false-PASS — this asserts it FAILS.
    let module_a = "/*! keep */ \"use strict\"; var x = 1;";
    let module_b = "\"use strict\"; /*! keep */ var x = 1;";
    assert!(
        !sigs_equal(module_a, module_b),
        "a semantic comment MOVED from leading a top-level directive to leading the first body \
         statement must FAIL (the directive is now a walked anchor `dir[0]`; the pre-fix build \
         walked only the body so A's comment collapsed to `<tail>` and the move false-PASSED)"
    );
    // The comment leading the directive resolves to a REAL (non-`<tail>`) directive anchor — direct
    // proof the directive is walked (the closure claim is now TRUE for directive-bearing modules).
    let anchor_a = first_semantic_comment_anchor(module_a);
    assert!(
        !anchor_a.ends_with("<tail>"),
        "a semantic comment leading a top-level directive must resolve to a real directive anchor, \
         NOT `<tail>` (the directive is walked); got {anchor_a:?}"
    );
    // And concretely it anchors at the FIRST directive segment (`dir[0]`), distinct from any body
    // `stmt[...]` segment — the directive index space is walked first and prefixed distinctly.
    assert!(
        anchor_a.contains("dir[0]"),
        "the directive-leading comment must anchor at the `dir[0]` directive segment; got {anchor_a:?}"
    );
    // SANITY (no false-FAIL): the SAME directive-leading comment differing only by whitespace still
    // compares EQUAL (the anchor is whitespace-insensitive).
    assert!(
        sigs_equal(
            "/*! keep */ \"use strict\"; var x = 1;",
            "/*! keep */   \"use strict\";   var x = 1;"
        ),
        "the same directive-leading semantic comment differing only by whitespace must still compare \
         EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_object_property_kind_method_computed_shorthand() {
    // `expr_sig`'s object arm now encodes EVERY behavior-bearing property axis
    // (`prop(kind=..,method=..,computed=..,shorthand=..,key=..,value=..)`). Each pair below collapsed
    // to the SAME signature under the pre-fix `key:value`-only encoding and now DIFFERS. Each subcase
    // names the axis it ISOLATES; where a pair would also differ on another axis (key shape, value
    // shape), an extra SAME-BODY/SAME-KEY pair pins the named axis as the SOLE discriminator.

    // KIND (get vs method) — ISOLATED: a getter `{ get x(){} }` and a method `{ x(){} }` with the
    // SAME EMPTY body have the SAME key (`x`) and the SAME (empty) value body, so the ONLY axes that
    // differ are `op.kind` (Get vs Init) and `op.method` (false vs true). The pre-fix `key:value`
    // encoding signed both as `k:x:<empty fn>` and collapsed them; the new axes split them.
    assert!(
        !sigs_equal("var o = { get x() {} };", "var o = { x() {} };"),
        "a getter vs a same-empty-body method at the same key must FAIL — kind/method is the SOLE \
         discriminator (key and body are identical)"
    );

    // METHOD (method-shorthand vs value) — ISOLATED: a method `{ x() {} }` and a value with a
    // function expression `{ x: function() {} }` share the SAME key and an empty function body; the
    // distinguishing axis is `op.method` (true vs false). (Both are `op.kind=Init`, so kind does NOT
    // discriminate here — method does.)
    assert!(
        !sigs_equal("var o = { x() {} };", "var o = { x: function() {} };"),
        "a method-shorthand property vs a function-value property (same empty body) must FAIL — \
         op.method is the discriminator"
    );

    // KIND (get vs set) — ISOLATED at the same key: a getter `{ get x() {} }` and a setter
    // `{ set x(v) {} }` are distinct accessor halves; `op.kind` is Get vs Set. (A setter takes one
    // param and a getter takes none, but the discriminating axis the rule cares about is `op.kind`.)
    assert!(
        !sigs_equal("var o = { get x() {} };", "var o = { set x(v) {} };"),
        "a getter vs a setter at the same key must FAIL (op.kind get/set is structural)"
    );

    // COMPUTED vs STATIC at the same NAME text: `{ x: v }` (static identifier key) vs `{ [x]: v }`
    // (computed identifier key). This pair is ALSO distinguished by `property_key_sig` alone
    // (`k:x` vs `k:[Id(x)]`), so `op.computed` is NOT the sole discriminator here — it is a REDUNDANT
    // second encoding of the same axis. The behavior difference is real (`{ x: v }` keys on the
    // literal name `x`; `{ [x]: v }` keys on the VALUE of `x`), and BOTH the key-shape and the
    // `computed` boolean record it; this assertion confirms the pair FAILS (not that `computed` is
    // the only signal).
    assert!(
        !sigs_equal("var o = { x: v };", "var o = { [x]: v };"),
        "a static key vs a computed key (same name text) must FAIL (the key shape AND op.computed \
         both record the axis)"
    );

    // SHORTHAND vs longhand — ISOLATED via `__proto__` (where the axis is semantically load-bearing):
    // `{ __proto__ }` shorthand sets an OWN property named `__proto__`, while `{ __proto__: x }`
    // longhand sets the prototype. The key is `__proto__` on both sides; the value differs by
    // shorthand (`op.shorthand` true → value is the same-name reference) vs longhand. `op.shorthand`
    // records the axis; the example uses a same-name longhand so the names match and shorthand is the
    // distinguishing axis.
    assert!(
        !sigs_equal("var o = { __proto__ };", "var o = { __proto__: __proto__ };"),
        "a shorthand property vs a same-name longhand property must FAIL (op.shorthand is structural; \
         __proto__ shorthand sets an own property, longhand sets the prototype)"
    );

    // SANITY (no false-FAIL): a byte-identical object compares EQUAL, and a redundant-paren-only
    // value difference still compares EQUAL.
    assert!(
        sigs_equal("var o = { x: (a) };", "var o = { x: a };"),
        "a redundant-paren-only object-property value difference must still compare EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_param_default() {
    // `params_sig` now encodes `FormalParameter.initializer` (`init=<expr_sig>` / `init=<none>`).
    // `(a = 1)`, `(a = 2)`, and `(a)` bind different values when the argument is `undefined` — all
    // reachable via source-preserved function literals (`{@html () => …}`, dynamic-value arrows).
    assert!(
        !sigs_equal("var f = (a = 1) => a;", "var f = (a = 2) => a;"),
        "a changed param DEFAULT VALUE must FAIL (FormalParameter.initializer is structural)"
    );
    assert!(
        !sigs_equal("var f = (a = 1) => a;", "var f = (a) => a;"),
        "a present param default vs an absent one must FAIL (init present/absent is structural)"
    );
    // SANITY (no false-FAIL): identical params (no defaults) still compare EQUAL.
    assert!(
        sigs_equal("var f = (a) => a;", "var f = (a) => a;"),
        "identical params with no defaults must compare EQUAL"
    );
    // SANITY: the SAME default differing only by a redundant paren still compares EQUAL (the default
    // expression is signed via `expr_sig`, which is paren-insensitive).
    assert!(
        sigs_equal("var f = (a = (1)) => a;", "var f = (a = 1) => a;"),
        "the same param default differing only by a redundant paren must still compare EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_async_and_generator() {
    // `expr_sig` / `stmt_sig` / `decl_sig` now encode `r#async` (arrow + function) and `generator`
    // (function only). `async () => 1` returns a Promise; `() => 1` returns `1`. `function*(){}`
    // returns an iterator; `function(){}` returns `undefined`. Reachable via source-preserved
    // function literals.

    // ASYNC arrow vs sync arrow.
    assert!(
        !sigs_equal("var f = async () => 1;", "var f = () => 1;"),
        "an async arrow vs a sync arrow must FAIL (r#async is structural)"
    );

    // GENERATOR function-declaration vs plain (in an export-default form so a top-level statement
    // carries it): `export default function* f(){}` vs `export default function f(){}`.
    assert!(
        !sigs_equal(
            "export default function* f() {}",
            "export default function f() {}"
        ),
        "a generator function vs a plain function must FAIL (generator is structural)"
    );

    // ASYNC function expression vs plain function expression.
    assert!(
        !sigs_equal(
            "var f = async function () { return 1; };",
            "var f = function () { return 1; };"
        ),
        "an async function expression vs a plain one must FAIL (r#async is structural)"
    );

    // SANITY (no false-FAIL): a byte-identical async arrow compares EQUAL.
    assert!(
        sigs_equal("var f = async () => 1;", "var f = async () => 1;"),
        "a byte-identical async arrow must compare EQUAL"
    );
}

// ── Terminal statement-encoder / directive discriminators ────────────────────
// These close the present-day reachable structural FALSE-PASS adjudicated by codex: a
// source-preserved `{@html () => { … }}` / dynamic-value arrow/function body byte-copies ordinary
// control-flow statements AND a directive prologue (`"use strict"`) into emitted `clientModule`,
// but the pre-fix comparator dropped `FunctionBody.directives` and collapsed every control-flow
// statement to `Stmt(discriminant)`. Each UNEQUAL test below is a real behavioral diff that the
// pre-fix comparator collapsed to EQUAL (the false-PASS) — they are the literal defect being
// closed. The wrapper is the reachable form (`var f = () => { … };` — the arrow body the value
// emitter source-preserves), so each diff routes through the same `function_body_sig` → `stmt_sig`
// path the gate signs.

#[test]
fn struct_compare_fails_on_function_body_directive() {
    // FUNCTION-BODY DIRECTIVE present-vs-absent: `() => { "use strict"; return x; }` carries a
    // `FunctionBody.directives` prologue the official printer byte-preserves; the pre-fix comparator
    // signed the body via `statements_sig(&body.statements)` ONLY (directives dropped), so the two
    // bodies collapsed to the SAME signature — the false-PASS. `function_body_sig` now signs the
    // ordered directive prologue too.
    assert!(
        !sigs_equal(
            "var f = () => { \"use strict\"; return x; };",
            "var f = () => { return x; };"
        ),
        "a function body with a `\"use strict\"` directive prologue vs one without must FAIL \
         (function_body_sig signs FunctionBody.directives; the pre-fix statements-only sign \
         DROPPED them = the false-PASS)"
    );
    // A DIFFERENT directive TEXT (`"use strict"` vs `"use asm"`) must also FAIL — the cooked value is
    // signed, not just presence.
    assert!(
        !sigs_equal(
            "var f = () => { \"use strict\"; return x; };",
            "var f = () => { \"use asm\"; return x; };"
        ),
        "two function bodies with DIFFERENT directive text must FAIL (the cooked directive value is \
         signed)"
    );
    // TOP-LEVEL `Program.directives`: a module-level `"use strict"` prologue present vs absent must
    // FAIL — the pre-fix `module_sig` was `statements_sig(&program.body)` (program directives
    // dropped). `program_sig` now prepends `directives_sig(&program.directives)`.
    assert!(
        !sigs_equal("\"use strict\"; var x = 1;", "var x = 1;"),
        "a top-level Program.directives `\"use strict\"` prologue present vs absent must FAIL \
         (program_sig signs Program.directives; the pre-fix module_sig dropped them)"
    );
}

#[test]
fn struct_compare_fails_on_for_body() {
    // `for` body differs: `() => { for(;;) a(); }` vs `() => { for(;;) b(); }`. Pre-fix both
    // collapsed to `Stmt(discriminant(ForStatement))` (body dropped) → EQUAL = the false-PASS. The
    // `For(...)` arm now signs init/test/update/body, and the body routes through `stmt_sig` →
    // `expr_sig`, so the `a()` vs `b()` call diff is caught.
    assert!(
        !sigs_equal(
            "var f = () => { for (;;) a(); };",
            "var f = () => { for (;;) b(); };"
        ),
        "a `for` body call difference must FAIL (the For arm signs its body via stmt_sig; the \
         pre-fix discriminant collapse made it EQUAL = the false-PASS)"
    );
    // A `for` INIT difference (`let i=0` vs `let i=1`) must also FAIL (for_init_sig → decl_var_sig).
    assert!(
        !sigs_equal(
            "var f = () => { for (let i = 0; ; ) a(); };",
            "var f = () => { for (let i = 1; ; ) a(); };"
        ),
        "a `for` init difference must FAIL (for_init_sig signs the init declaration)"
    );
}

#[test]
fn struct_compare_fails_on_while_and_do_while() {
    // `while` test differs: pre-fix both collapsed to `Stmt(discriminant(WhileStatement))` → EQUAL.
    // The `While(...)` arm now signs test (expr_sig) + body (stmt_sig).
    assert!(
        !sigs_equal(
            "var f = () => { while (a) c(); };",
            "var f = () => { while (b) c(); };"
        ),
        "a `while` test difference must FAIL (the While arm signs test via expr_sig; pre-fix \
         discriminant collapse = EQUAL = the false-PASS)"
    );
    // `do-while` body differs: pre-fix discriminant collapse → EQUAL. The `DoWhile(...)` arm signs
    // body + test.
    assert!(
        !sigs_equal(
            "var f = () => { do a(); while (x); };",
            "var f = () => { do b(); while (x); };"
        ),
        "a `do-while` body difference must FAIL (the DoWhile arm signs its body via stmt_sig)"
    );
}

#[test]
fn struct_compare_fails_on_for_in_vs_for_of_and_await_flag() {
    // `for-in` vs `for-of` are DIFFERENT statement families — pre-fix they were distinct discriminants
    // already, so the load-bearing diff here is the for-of `await` flag flip, which the discriminant
    // collapse CANNOT see (same discriminant). `for await (x of y)` vs `for (x of y)` must FAIL — the
    // `ForOf(await=…)` arm signs `r#await`.
    assert!(
        !sigs_equal(
            "var f = async () => { for await (const x of y) a(x); };",
            "var f = async () => { for (const x of y) a(x); };"
        ),
        "a for-of `await` flag flip must FAIL (the ForOf arm signs r#await; the pre-fix \
         discriminant collapse could not distinguish them = the false-PASS)"
    );
    // And a for-of RIGHT-operand difference (the iterated source) must FAIL (for-of right via expr_sig).
    assert!(
        !sigs_equal(
            "var f = () => { for (const x of y) a(x); };",
            "var f = () => { for (const x of z) a(x); };"
        ),
        "a for-of right-operand difference must FAIL (the ForOf arm signs right via expr_sig)"
    );
}

#[test]
fn struct_compare_fails_on_switch() {
    // `switch` case CONSEQUENT differs: pre-fix discriminant collapse → EQUAL. `switch (n) { case 1:
    // a(); }` vs `{ case 1: b(); }` must FAIL — `switch_case_sig` signs each case's test + consequent.
    assert!(
        !sigs_equal(
            "var f = () => { switch (n) { case 1: a(); } };",
            "var f = () => { switch (n) { case 1: b(); } };"
        ),
        "a `switch` case consequent difference must FAIL (switch_case_sig signs the consequent; \
         pre-fix discriminant collapse = EQUAL = the false-PASS)"
    );
    // A case TEST difference (`case 1` vs `case 2`) must also FAIL (switch_case_sig signs the test).
    assert!(
        !sigs_equal(
            "var f = () => { switch (n) { case 1: a(); } };",
            "var f = () => { switch (n) { case 2: a(); } };"
        ),
        "a `switch` case test difference must FAIL (switch_case_sig signs the case test)"
    );
}

#[test]
fn struct_compare_fails_on_try_catch_finally() {
    // `try`/catch — catch-PARAM rename: pre-fix discriminant collapse → EQUAL. `try {} catch (e) {}`
    // vs `catch (err) {}` must FAIL — the `Try(...)` arm signs the catch param via `binding_sig`.
    assert!(
        !sigs_equal(
            "var f = () => { try { a(); } catch (e) { h(e); } };",
            "var f = () => { try { a(); } catch (err) { h(err); } };"
        ),
        "a `try`/catch param rename must FAIL (the Try arm signs catch param via binding_sig; \
         pre-fix discriminant collapse = EQUAL = the false-PASS)"
    );
    // FINALIZER presence differs: `try {} catch {} finally {}` vs without — the `Try(...)` arm signs
    // `finalizer={…|<none>}`.
    assert!(
        !sigs_equal(
            "var f = () => { try { a(); } catch (e) {} finally { c(); } };",
            "var f = () => { try { a(); } catch (e) {} };"
        ),
        "a `try` finalizer present vs absent must FAIL (the Try arm signs the finalizer block)"
    );
}

#[test]
fn struct_compare_fails_on_throw_argument() {
    // `throw` argument differs: pre-fix discriminant collapse → EQUAL. `throw e;` vs `throw f;` must
    // FAIL — the `Throw(...)` arm signs the argument via `expr_sig`.
    assert!(
        !sigs_equal("var f = () => { throw e; };", "var f = () => { throw g; };"),
        "a `throw` argument difference must FAIL (the Throw arm signs the argument via expr_sig; \
         pre-fix discriminant collapse = EQUAL = the false-PASS)"
    );
}

#[test]
fn struct_compare_fails_on_labeled_statement() {
    // `labeled` label NAME differs: pre-fix discriminant collapse → EQUAL. `outer: for(;;) a();` vs
    // `inner: for(;;) a();` must FAIL — the `Label(...)` arm signs the label name + body.
    assert!(
        !sigs_equal(
            "var f = () => { outer: for (;;) a(); };",
            "var f = () => { inner: for (;;) a(); };"
        ),
        "a labeled-statement label-name difference must FAIL (the Label arm signs the label name; \
         pre-fix discriminant collapse = EQUAL = the false-PASS)"
    );
}

#[test]
fn struct_compare_fails_on_class_in_body_and_declaration() {
    // `class` in a preserved body — SUPER-CLASS differs: pre-fix `ClassDeclaration` collapsed to
    // `Stmt(discriminant)` → EQUAL. `class C extends A {}` vs `class C extends B {}` must FAIL —
    // `class_sig` signs `super=<expr_sig>`.
    assert!(
        !sigs_equal(
            "var f = () => { class C extends A {} return C; };",
            "var f = () => { class C extends B {} return C; };"
        ),
        "a class super-class difference (in a preserved body) must FAIL (class_sig signs super via \
         expr_sig; pre-fix discriminant collapse = EQUAL = the false-PASS)"
    );
    // A class MEMBER KEY difference as a top-level DECLARATION: `class C { a() {} }` vs `class C { b()
    // {} }` must FAIL — `class_element_sig` signs each method key via `property_key_sig`. (This routes
    // through `stmt_sig`'s `ClassDeclaration` arm → `class_sig` for a top-level class statement.)
    assert!(
        !sigs_equal("class C { a() {} }", "class C { b() {} }"),
        "a class member-key difference (top-level declaration) must FAIL (class_element_sig signs \
         the method key; pre-fix discriminant collapse = EQUAL = the false-PASS)"
    );
}

#[test]
fn struct_compare_waives_redundant_paren_in_encoded_statements() {
    // COSMETIC SANITY (no false-FAIL): the NEW statement arms route every sub-expression through the
    // paren-transparent `expr_sig`, so a redundant author paren INSIDE one of these statements is the
    // cosmetic-paren waiver the official printer drops — it must compare EQUAL pre AND post (this
    // characterizes no-regression for the new arms, not the false-PASS closure).
    assert!(
        sigs_equal(
            "var f = () => { throw (e); };",
            "var f = () => { throw e; };"
        ),
        "a redundant paren around a `throw` argument must compare EQUAL (the Throw arm routes the \
         argument through the paren-transparent expr_sig)"
    );
    // A redundant paren around a `for` body call (`for(;;) (a)();` vs `for(;;) a();`) is likewise
    // paren-transparent (the body routes through stmt_sig → expr_sig).
    assert!(
        sigs_equal(
            "var f = () => { for (;;) (a)(); };",
            "var f = () => { for (;;) a(); };"
        ),
        "a redundant paren around a `for` body call must compare EQUAL (the For body routes through \
         the paren-transparent expr_sig)"
    );
}

#[test]
fn semantic_comment_class_ts_check_is_line_only_with_strict_boundary() {
    // `@ts-check` / `@ts-nocheck` are valid ONLY as `//` LINE comments with a strict boundary
    // (end / whitespace / `:`). The pre-fix classifier stripped BOTH `//` and `/* */` and accepted
    // any non-identifier boundary, so a `/* @ts-check */` block and a `// @ts-check/foo` lookalike
    // wrongly classified as semantic (false-FAILs).

    // WAIVED: a `/* @ts-check */` BLOCK comment is NOT a valid `@ts-check` pragma — it must stay
    // waived, so adding it to one side compares EQUAL. The pre-fix classifier false-FAILED this.
    assert!(
        sigs_equal("var x = 1;", "/* @ts-check */\nvar x = 1;"),
        "`/* @ts-check */` in BLOCK form is not a valid pragma and must compare EQUAL (line-only; \
         the pre-fix classifier false-FAILED this)"
    );

    // WAIVED: `// @ts-check/foo` is a LOOKALIKE — the `/foo` continues the token past the strict
    // pragma boundary — so it stays waived. The pre-fix boundary accepted `/` and false-FAILED this.
    assert!(
        sigs_equal("var x = 1;", "// @ts-check/foo\nvar x = 1;"),
        "`// @ts-check/foo` is a non-pragma lookalike (strict boundary) and must compare EQUAL (the \
         pre-fix boundary false-FAILED this)"
    );

    // SEMANTIC: a genuine `// @ts-check` line pragma dropped must FAIL (the split must not
    // over-narrow the real line pragma).
    assert!(
        !sigs_equal("// @ts-check\nvar x = 1;", "var x = 1;"),
        "a dropped genuine `// @ts-check` line pragma must FAIL (line-form pragma stays semantic)"
    );
    // SEMANTIC: `// @ts-nocheck` (the other line pragma) dropped must FAIL.
    assert!(
        !sigs_equal("// @ts-nocheck\nvar x = 1;", "var x = 1;"),
        "a dropped genuine `// @ts-nocheck` line pragma must FAIL (line-form pragma stays semantic)"
    );
    // SEMANTIC (unchanged): `@ts-ignore` / `@ts-expect-error` remain valid in BLOCK form (the split
    // keeps their looser boundary) — a dropped `/* @ts-expect-error */` block must still FAIL.
    assert!(
        !sigs_equal("/* @ts-expect-error */\nvar x = 1;", "var x = 1;"),
        "a dropped `/* @ts-expect-error */` block directive must still FAIL (block form stays valid \
         for @ts-ignore / @ts-expect-error)"
    );
}

#[test]
fn struct_compare_fails_on_class_method_arity() {
    // `class_element_sig`'s `MethodDefinition` arm now signs the COMPLETE runtime method shape —
    // `params` (via the terminal `params_sig` → `binding_sig`) alongside kind/static/computed/key/body.
    // Two methods with the SAME name + body but a DIFFERENT param COUNT are a genuine behavioral
    // divergence (the arity changes the call contract); the pre-fix arm signed kind/static/computed/
    // key/body ONLY and collapsed them to the SAME signature (a structural false-PASS). A class is
    // reachable via a source-preserved `{@html}`/dynamic-value arrow/function body the value emitter
    // byte-copies.
    assert!(
        !sigs_equal(
            "class C { m(a) { return 1; } }",
            "class C { m(a, b) { return 1; } }"
        ),
        "two class methods with the same name + body but different param ARITY must FAIL \
         (class_element_sig now signs method params; the pre-fix arm false-PASSED this)"
    );
    // SANITY (no false-FAIL): a redundant-paren-only method body difference still compares EQUAL
    // (the body is signed via the paren-insensitive `function_body_sig` → `expr_sig`).
    assert!(
        sigs_equal(
            "class C { m(a) { return (a); } }",
            "class C { m(a) { return a; } }"
        ),
        "a redundant-paren-only class method body difference must still compare EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_class_method_async() {
    // `class_element_sig`'s `MethodDefinition` arm now signs `async` (`m.value.r#async`). An `async`
    // method and a plain method with the SAME name + params + body differ behaviorally (an async
    // method returns a Promise); the pre-fix arm dropped the async bit and collapsed them. Reachable
    // through a source-preserved class body.
    assert!(
        !sigs_equal(
            "class C { async m() { return 1; } }",
            "class C { m() { return 1; } }"
        ),
        "an async class method vs a plain one (same name/params/body) must FAIL (class_element_sig \
         now signs method async; the pre-fix arm false-PASSED this)"
    );
}

#[test]
fn struct_compare_fails_on_class_method_generator() {
    // `class_element_sig`'s `MethodDefinition` arm now signs `generator` (`m.value.generator`). A
    // generator method `*m(){}` and a plain method with the SAME name + params + body differ
    // behaviorally (a generator returns an iterator); the pre-fix arm dropped the generator bit and
    // collapsed them. Reachable through a source-preserved class body.
    assert!(
        !sigs_equal(
            "class C { *m() { return 1; } }",
            "class C { m() { return 1; } }"
        ),
        "a generator class method vs a plain one (same name/params/body) must FAIL (class_element_sig \
         now signs method generator; the pre-fix arm false-PASSED this)"
    );
}

#[test]
fn struct_compare_waives_class_expression_redundant_parens() {
    // `expr_sig` now routes a `ClassExpression` through `class_sig` (paren-transparent — the method
    // body is signed via `function_body_sig` → `expr_sig`, which peels redundant parens). A redundant
    // author paren inside a class-expression method body is COSMETIC, so two such class expressions
    // compare EQUAL. Pre-fix, `ClassExpression` Debug-collapsed via the `other =>` fallback, which
    // leaks the inner paren's span/Debug shape — a cosmetic-paren false-FAIL this closes. A class
    // expression is reachable via a source-preserved dynamic value (`var C = class { … }`).
    assert!(
        sigs_equal(
            "var C = class { m() { return (x); } };",
            "var C = class { m() { return x; } };"
        ),
        "a redundant-paren-only class EXPRESSION method body difference must compare EQUAL \
         (expr_sig now routes class expressions through the paren-transparent class_sig; the pre-fix \
         Debug fallback false-FAILED this)"
    );
}

#[test]
fn struct_compare_fails_on_class_expression_method_arity() {
    // A class EXPRESSION now routes through the (fixed) `class_sig`, so a method-arity difference in a
    // class expression FAILS exactly like a class declaration — proving class expressions reach the
    // fixed encoder, not the Debug fallback. Pre-fix the `other =>` Debug fallback might happen to
    // catch the arity (Debug prints params), but it ALSO false-FAILed cosmetic parens (previous test);
    // the pair together pins "class expr routes through class_sig". Reachable via a source-preserved
    // dynamic value.
    assert!(
        !sigs_equal(
            "var C = class { m(a) {} };",
            "var C = class { m(a, b) {} };"
        ),
        "two class EXPRESSIONS with a method arity difference must FAIL (class expressions route \
         through the fixed class_sig)"
    );
}

#[test]
fn struct_compare_fails_on_class_level_decorator_presence() {
    // `class_sig` now signs CLASS-level decorators (`decorators=<decorators_sig>`). A decorated class
    // and the same class without the decorator differ behaviorally (the decorator executes and can
    // alter the class). OXC parses ECMAScript decorators under the comparator's `SourceType::mjs()`
    // (verified — the parse succeeds with errors=0), the Svelte runtime strips TS but NOT decorators,
    // and a decorated class in a source-preserved `{@html}`/dynamic value is byte-copied to emitted
    // client JS — so this is REACHABLE. The pre-fix `class_sig` ignored `Class.decorators`, collapsing
    // the two to the SAME signature (a reachable false-PASS this closes). The class is a top-level
    // declaration (program-body), which routes through `stmt_sig`'s `ClassDeclaration` arm → `class_sig`.
    assert!(
        !sigs_equal("@dec class C {}\nvar x = 1;", "class C {}\nvar x = 1;"),
        "a decorated class vs the same undecorated class must FAIL (class_sig now signs class-level \
         decorators; the pre-fix arm ignored Class.decorators = the false-PASS)"
    );
}

#[test]
fn struct_compare_fails_on_class_level_decorator_name_differs() {
    // Two classes decorated with DIFFERENT decorators (`@a` vs `@b`) differ behaviorally — different
    // decorator functions run. `class_sig` signs each decorator through the paren-transparent
    // `expr_sig`, so the decorator IDENTIFIER difference is captured. Pre-fix both collapsed to the
    // SAME (unsigned-decorator) signature.
    assert!(
        !sigs_equal("@a class C {}\nvar x = 1;", "@b class C {}\nvar x = 1;"),
        "two classes decorated with different decorators (@a vs @b) must FAIL (class_sig signs each \
         decorator via the paren-transparent expr_sig)"
    );
}

#[test]
fn struct_compare_fails_on_member_method_decorator_presence() {
    // `class_element_sig`'s `MethodDefinition` arm now signs the per-member decorators
    // (`decorators=<decorators_sig>`). A decorated method and the same method without the decorator
    // differ behaviorally (the member decorator executes). Pre-fix the method arm ignored
    // `m.decorators`, collapsing them — a reachable false-PASS this closes. Reachable via a
    // source-preserved class body the value emitter byte-copies.
    assert!(
        !sigs_equal("class C { @dec m() {} }", "class C { m() {} }"),
        "a decorated method vs the same undecorated method must FAIL (class_element_sig now signs \
         method decorators; the pre-fix arm ignored m.decorators = the false-PASS)"
    );
}

#[test]
fn struct_compare_fails_on_member_method_decorator_name_differs() {
    // A method decorated with DIFFERENT decorators (`@a` vs `@b`) differs behaviorally. The
    // `MethodDefinition` arm signs each decorator through the paren-transparent `expr_sig`, so the
    // decorator IDENTIFIER difference is captured. Pre-fix both collapsed to the SAME signature.
    assert!(
        !sigs_equal("class C { @a m() {} }", "class C { @b m() {} }"),
        "a method decorated with different decorators (@a vs @b) must FAIL (class_element_sig signs \
         each method decorator via the paren-transparent expr_sig)"
    );
}

#[test]
fn struct_compare_fails_on_member_property_decorator_presence() {
    // `class_element_sig`'s `PropertyDefinition` arm now signs the per-member decorators. A decorated
    // property and the same property without the decorator differ behaviorally (the property decorator
    // executes). Pre-fix the prop arm ignored `p.decorators`, collapsing them — a reachable false-PASS.
    assert!(
        !sigs_equal("class C { @dec x = 1; }", "class C { x = 1; }"),
        "a decorated property vs the same undecorated property must FAIL (class_element_sig now signs \
         property decorators; the pre-fix arm ignored p.decorators = the false-PASS)"
    );
}

#[test]
fn struct_compare_decorator_arg_paren_transparent_but_value_significant() {
    // Each decorator is signed through the paren-transparent `expr_sig` (a decorator IS an expression —
    // `@foo`, `@foo.bar`, `@foo(arg)`). This locks BOTH directions:
    //
    // PAREN-EQUAL (cosmetic / no-false-FAIL): a redundant paren around a decorator ARGUMENT is the
    // cosmetic-paren waiver the official printer drops — it must compare EQUAL (the arg is signed via
    // the paren-transparent `expr_sig`). (Pre-fix this is also EQUAL because decorators were unsigned;
    // the value-FAIL below is what makes the EQUAL here a MEANINGFUL post-fix transparency assertion.)
    assert!(
        sigs_equal("@dec((x)) class C {}\nvar y = 1;", "@dec(x) class C {}\nvar y = 1;"),
        "a redundant paren around a decorator argument must compare EQUAL (the decorator arg is signed \
         via the paren-transparent expr_sig)"
    );
    // VALUE-FAIL (the arg is actually signed): a DIFFERENT decorator argument value (`@dec(a)` vs
    // `@dec(b)`) is behavior-bearing and must FAIL — proving the arg is genuinely signed, not dropped.
    // Pre-fix both collapsed to the SAME (unsigned-decorator) signature = the false-PASS.
    assert!(
        !sigs_equal("@dec(a) class C {}\nvar y = 1;", "@dec(b) class C {}\nvar y = 1;"),
        "two classes with a different decorator ARGUMENT value (@dec(a) vs @dec(b)) must FAIL (the \
         decorator argument is signed via expr_sig; pre-fix the unsigned decorator false-PASSED this)"
    );
}

#[test]
fn struct_compare_waives_stray_empty_statement_in_list() {
    // `statements_sig` now FILTERS a stray no-op `EmptyStatement` (`;`) in a statement LIST — the
    // official printer drops it, so `{ ; return x; }` and `{ return x; }` are a cosmetic-only
    // difference and must compare EQUAL. Pre-fix `stmt_sig` signed `Empty` for the stray `;`, so the
    // list signatures differed — a false-FAIL this closes. Reachable via a source-preserved
    // arrow/function body the value emitter byte-copies.
    assert!(
        sigs_equal(
            "var f = () => { ; return x; };",
            "var f = () => { return x; };"
        ),
        "a stray no-op `;` in a statement LIST must compare EQUAL to the same list without it \
         (statements_sig filters list-context EmptyStatement; the pre-fix Empty arm false-FAILED this)"
    );
}

#[test]
fn struct_compare_fails_on_empty_vs_nonempty_required_loop_body() {
    // The EmptyStatement list-context filter must NOT over-reach into REQUIRED child positions. A
    // loop BODY is a required position reached through `stmt_sig` DIRECTLY (NOT `statements_sig`), so
    // an empty loop body (`for(;;);`) vs a non-empty one (`for(;;) a();`) is behavior-bearing and
    // stays UNEQUAL. This locks the contract: the filter only drops no-op `;` from LISTS, never from a
    // required body. (Should be UNEQUAL pre AND post — its job is to characterize the no-over-filter
    // boundary, not to prove a defect closed.)
    assert!(
        !sigs_equal(
            "var f = () => { for (;;); };",
            "var f = () => { for (;;) a(); };"
        ),
        "an EMPTY required loop body vs a non-empty one must stay UNEQUAL (the list-context filter \
         must NOT over-reach into the required loop-body position reached via stmt_sig directly)"
    );
}

#[test]
fn struct_compare_fails_on_class_decl_vs_expr_at_export_default() {
    // `class_sig` now signs `Class.r#type` (ClassDeclaration vs ClassExpression). The decl-vs-expr
    // distinction is BEHAVIOR-BEARING at `export default`: `export default class C {}` BINDS the name
    // `C` in module scope (so `var y = C;` succeeds), while `export default (class C {});` is a class
    // EXPRESSION whose `C` is visible ONLY inside the class body, NOT module scope (so `var y = C;`
    // throws a ReferenceError). Both flow through the export-default arm → `class_sig`. Pre-fix
    // `class_sig` dropped `Class.r#type`, so the two signed IDENTICALLY — a behavior-bearing false-PASS
    // this closes. (The trailing `var y = C;` keeps the two modules' bodies structurally identical so
    // the `Class.r#type` axis is the SOLE difference carrying the FAIL.)
    assert!(
        !sigs_equal(
            "export default class C {}\nvar y = C;",
            "export default (class C {});\nvar y = C;"
        ),
        "an export-default class DECLARATION (binds C) vs a class EXPRESSION (does not bind C) must \
         FAIL (class_sig now signs Class.r#type; the pre-fix arm dropped it = the false-PASS)"
    );
}

#[test]
fn struct_compare_fails_on_jsdoc_on_leading_filtered_empty_vs_leading_real_statement() {
    // The empty-statement filter ↔ comment-anchor index MISMATCH (false-PASS). `statements_sig`
    // FILTERS a list-context `EmptyStatement`, so the comment-anchor index MUST be computed over the
    // SAME normalized view: a real statement is indexed by its LOGICAL (empty-filtered) index, and a
    // semantic comment attached to a FILTERED empty gets a SYNTHETIC
    // `empty_gap[<logical>.<empty_ordinal>]:EmptyStatement` anchor — distinct from the next real
    // statement's `stmt[N]:<AstType>`. So a JSDoc on the leading EMPTY (`/** @type {number} */; var x`)
    // anchors `empty_gap[0.0]:EmptyStatement` while a JSDoc on the leading VAR
    // (`/** @type {number} */ var x`) anchors `stmt[0]:VariableDeclaration` → UNEQUAL.
    // Pre-fix the index walked UNFILTERED `program.body` with bare `stmt[<raw i>]`, so both got the SAME
    // anchor + the same filtered module_sig → the JSDoc MOVE compared EQUAL (the false-PASS this closes).
    assert!(
        !sigs_equal(
            "/** @type {number} */; var x = \"s\";",
            "/** @type {number} */ var x = \"s\";"
        ),
        "a JSDoc semantic comment on a leading FILTERED empty statement vs on the leading real \
         statement must FAIL (the anchor index normalizes over the same empty-filtered view: \
         empty_gap[0.0] vs stmt[0]; the pre-fix raw-index walk collapsed both = the false-PASS)"
    );
    // Pin the SPECIFIC anchors (not just inequality) so this test genuinely characterizes the
    // empty_gap synthetic anchor: a JSDoc on the filtered empty resolves to
    // `empty_gap[0.0]:EmptyStatement` (NOT `<tail>`, NOT the next real node), and a JSDoc on the real var
    // resolves to its node-typed logical-index segment. This makes the test RED if `record_empty_gap` is
    // removed (the empty would otherwise collapse to `<tail>`).
    assert_eq!(
        first_semantic_comment_anchor("/** @type {number} */; var x = \"s\";"),
        "pos=Leading/empty_gap[0.0]:EmptyStatement",
        "a JSDoc on a leading FILTERED empty statement must resolve to the synthetic empty_gap[0.0] anchor"
    );
    assert_eq!(
        first_semantic_comment_anchor("/** @type {number} */ var x = \"s\";"),
        "pos=Leading/stmt[0]:VariableDeclaration",
        "a JSDoc on the leading real var must resolve to its node-typed logical-index segment"
    );
}

#[test]
fn struct_compare_waives_cosmetic_semicolon_before_semantic_comment() {
    // The SAME mismatch, the false-FAIL direction. A cosmetic leading `;` (a list-context empty the
    // printer drops) before a semantic comment that leads a REAL statement must NOT shift that
    // comment's anchor: `; /*! keep */ f();` and `/*! keep */ f();` BOTH anchor the license comment to
    // `f()`'s `stmt[0]:ExpressionStatement` → EQUAL. Pre-fix the unfiltered index gave the comment raw
    // `stmt[1]` vs `stmt[0]`, so the cosmetic `;` made them UNEQUAL (the false-FAIL this closes).
    assert!(
        sigs_equal("; /*! keep */ f();", "/*! keep */ f();"),
        "a cosmetic leading `;` before a semantic comment on a real statement must compare EQUAL \
         (the anchor index normalizes the empty away so the comment anchors the same real statement; \
         the pre-fix raw-index walk shifted the anchor = the false-FAIL)"
    );
}

#[test]
fn struct_compare_normalizes_empty_gap_anchor_inside_nested_statement_list() {
    // The empty-gap normalization is RECURSIVE: `statements_sig` filters list-context empties inside
    // nested lists (block/function bodies, switch consequents, static blocks) too, so the anchor index
    // must apply the SAME normalization there — done via the `visit_statements` override that mirrors
    // the top-level loop. INSIDE a function body:
    //
    // MOVE (false-PASS direction): a JSDoc on a nested FILTERED empty (`{ /** @type {number} */;
    // return x; }`) anchors a nested `empty_gap[0.0]:EmptyStatement` while a JSDoc on the nested return
    // (`{ /** @type {number} */ return x; }`) anchors a nested `stmt[0]:ReturnStatement` → UNEQUAL.
    assert!(
        !sigs_equal(
            "function g(){ /** @type {number} */; return x; }",
            "function g(){ /** @type {number} */ return x; }"
        ),
        "a JSDoc on a nested FILTERED empty vs on the nested real statement must FAIL (the \
         visit_statements override normalizes nested lists the same way: empty_gap[0.0] vs stmt[0])"
    );
    // Pin the SPECIFIC nested anchors so this test genuinely characterizes the RECURSIVE empty_gap
    // normalization: a JSDoc on the nested filtered empty resolves to a nested
    // `empty_gap[0.0]:EmptyStatement` segment under the function-body chain (NOT the next real nested
    // node, NOT `<tail>`), while a JSDoc on the nested real return resolves to its `child[0]` logical
    // segment. This makes the test RED if `record_empty_gap` is removed (the nested empty would
    // otherwise collapse to the surrounding node / `<tail>`).
    assert_eq!(
        first_semantic_comment_anchor("function g(){ /** @type {number} */; return x; }"),
        "pos=Leading/stmt[0]:Function/child[2]:FunctionBody/empty_gap[0.0]:EmptyStatement",
        "a JSDoc on a nested FILTERED empty must resolve to the synthetic nested empty_gap[0.0] anchor"
    );
    assert_eq!(
        first_semantic_comment_anchor("function g(){ /** @type {number} */ return x; }"),
        "pos=Leading/stmt[0]:Function/child[2]:FunctionBody/child[0]:ReturnStatement",
        "a JSDoc on the nested real return must resolve to its nested logical-index segment"
    );
    // COSMETIC nested `;` (false-FAIL direction): a cosmetic leading `;` inside the body before a
    // semantic comment on a real nested statement must compare EQUAL (the nested empty is normalized
    // away, so the comment anchors the same nested statement).
    assert!(
        sigs_equal(
            "function g(){ ; /*! keep */ h(); }",
            "function g(){ /*! keep */ h(); }"
        ),
        "a cosmetic leading `;` inside a function body before a semantic comment on a real nested \
         statement must compare EQUAL (the visit_statements override normalizes the nested empty away)"
    );
}

#[test]
fn struct_compare_fails_on_semantic_comment_moved_between_consecutive_filtered_empties() {
    // The per-gap EMPTY ORDINAL. CONSECUTIVE list-context `EmptyStatement`s (`;;`) are all filtered by
    // `statements_sig` (cosmetic no-ops) and share the same LOGICAL gap index, so a per-gap ordinal is
    // required to keep a semantic comment's POSITION among them structural. A `/*@__PURE__*/` comment
    // moved from BEFORE the first empty to BETWEEN the two empties must compare UNEQUAL — the comment
    // anchors `empty_gap[0.0]` in one form and `empty_gap[0.1]` in the other. Pre-fix both collapsed to
    // `empty_gap[0]` (the consecutive empties collided), so the MOVE compared EQUAL — the comparator's
    // own anchor-consistency invariant ("a future anchor collapse inside the normalized view is a
    // comparator bug") was violated. This test RED→GREEN characterizes the per-gap empty ordinal.
    assert!(
        !sigs_equal("/*@__PURE__*/ ; ; f();", "; /*@__PURE__*/ ; f();"),
        "a semantic (PURE) comment moved between two CONSECUTIVE filtered empty statements must FAIL \
         (the per-gap empty ordinal makes empty_gap[0.0] vs empty_gap[0.1] distinct; the pre-fix \
         single-index anchor collapsed both consecutive empties = the cosmetic-corner false-PASS)"
    );
    // Pin the SPECIFIC anchors so the test genuinely characterizes the ordinal (not just inequality):
    // the leading-on-first-empty form anchors `empty_gap[0.0]`, the moved-between form anchors
    // `empty_gap[0.1]`. RED if the ordinal is removed (both collapse to `empty_gap[0]`).
    assert_eq!(
        first_semantic_comment_anchor("/*@__PURE__*/ ; ; f();"),
        "pos=Leading/empty_gap[0.0]:EmptyStatement",
        "a PURE comment leading the FIRST of two consecutive filtered empties anchors empty_gap[0.0]"
    );
    assert_eq!(
        first_semantic_comment_anchor("; /*@__PURE__*/ ; f();"),
        "pos=Leading/empty_gap[0.1]:EmptyStatement",
        "a PURE comment leading the SECOND of two consecutive filtered empties anchors empty_gap[0.1]"
    );
    // SANITY (no false-FAIL): the SAME comment at the SAME position differing only by whitespace
    // between the empties must still compare EQUAL (the ordinal is position-structural, not whitespace).
    assert!(
        sigs_equal("/*@__PURE__*/ ; ; f();", "/*@__PURE__*/   ;   ; f();"),
        "the same PURE comment at the same empty position differing only by whitespace must compare EQUAL"
    );
}

#[test]
fn struct_compare_waives_untagged_template_cooked_equal_escape() {
    // An UNTAGGED `TemplateLiteral`'s COOKED value is its RUNTIME string; the raw escape representation
    // is a cosmetic carrier (exactly like a `StringLiteral`, which already signs cooked `.value`). So
    // ``\x41`` and ``A`` (untagged) produce the SAME runtime string `"A"` and must compare EQUAL.
    // Pre-fix `expr_sig`'s `TemplateLiteral` arm signed `q.value.raw`, so the escape difference made
    // them UNEQUAL (the false-FAIL this closes). The fix signs the COOKED value per quasi.
    assert!(
        sigs_equal("var s = `\\x41`;", "var s = `A`;"),
        "two UNTAGGED templates with the same COOKED value but different raw escapes must compare \
         EQUAL (the untagged cooked value is the runtime string; raw is cosmetic carrier)"
    );
    // VALUE-FAIL (the cooked value is actually signed): a DIFFERENT cooked value must still FAIL,
    // proving the fix signs the cooked content rather than dropping it.
    assert!(
        !sigs_equal("var s = `A`;", "var s = `B`;"),
        "two UNTAGGED templates with different COOKED values must FAIL (the cooked value is signed)"
    );
}

#[test]
fn struct_compare_keeps_tagged_template_raw_significant() {
    // A TAGGED template's tag function observes `strings.raw`, so the RAW escape representation IS
    // in-contract for a tagged template — ``tag`\x41`` and ``tag`A`` differ in what the tag sees and
    // must stay UNEQUAL. The tagged arm signs BOTH raw and cooked (so a cooked-only diff is also
    // caught), but the raw difference alone keeps this pair UNEQUAL. (Should hold pre AND post — it
    // characterizes that the untagged cooked fix did NOT leak into the tagged arm.)
    assert!(
        !sigs_equal("var s = tag`\\x41`;", "var s = tag`A`;"),
        "two TAGGED templates with different raw escapes must stay UNEQUAL (the tag observes \
         strings.raw, so raw is in-contract for tagged templates)"
    );
}

#[test]
fn struct_compare_handles_yield_paren_value_and_delegate() {
    // `expr_sig` now encodes `YieldExpression` (`Yield(delegate=..,arg=..)`) with the argument routed
    // through the paren-transparent `expr_sig`. Pre-fix yield Debug-collapsed via the `other =>`
    // fallback (leaking the inner paren span + a Debug shape). Reachable through a source-preserved
    // generator function literal the emitter byte-copies.

    // PAREN-EQUAL (cosmetic): a redundant paren around the yield argument is COSMETIC and compares
    // EQUAL (the arg is signed via the paren-transparent `expr_sig`). Pre-fix Debug-collapse leaks the
    // paren → false-FAIL.
    assert!(
        sigs_equal(
            "var f = function* () { yield (x); };",
            "var f = function* () { yield x; };"
        ),
        "a redundant paren around a yield argument must compare EQUAL (expr_sig now signs the yield \
         arg paren-transparently; the pre-fix Debug fallback false-FAILED this)"
    );
    // VALUE-FAIL: a DIFFERENT yield argument is a behavioral divergence and FAILS.
    assert!(
        !sigs_equal(
            "var f = function* () { yield a; };",
            "var f = function* () { yield b; };"
        ),
        "a yield with a different argument value must FAIL (the yield arg is signed)"
    );
    // DELEGATE-FAIL: `yield* g()` (delegating) vs `yield g()` (non-delegating) differ behaviorally
    // (delegation iterates the operand) and FAIL — the `delegate` bit is signed.
    assert!(
        !sigs_equal(
            "var f = function* () { yield* g(); };",
            "var f = function* () { yield g(); };"
        ),
        "a delegating `yield*` vs a non-delegating `yield` must FAIL (the delegate bit is signed)"
    );
}

#[test]
fn struct_compare_fails_on_import_expression_source() {
    // `expr_sig` now encodes a dynamic `ImportExpression` (`Import(source=..,options=..,phase=..)`)
    // with the source routed through `expr_sig`. Two `import(...)` calls with DIFFERENT sources are a
    // behavioral divergence (they load different modules) and FAIL; pre-fix the import expression
    // Debug-collapsed via the `other =>` fallback. Dynamic import is ordinary source-preservable JS
    // reachable through a `{@html}`/dynamic-value body.
    assert!(
        !sigs_equal("var p = import(\"a\");", "var p = import(\"b\");"),
        "two dynamic import() expressions with different sources must FAIL (the import source is \
         signed; the pre-fix Debug fallback collapsed it)"
    );
    // SANITY (no false-FAIL): the SAME dynamic import with a redundant paren around the source still
    // compares EQUAL (the source is signed via the paren-transparent `expr_sig`).
    assert!(
        sigs_equal("var p = import((\"a\"));", "var p = import(\"a\");"),
        "a redundant paren around a dynamic import source must compare EQUAL"
    );
}

#[test]
fn struct_compare_fails_on_meta_property() {
    // `expr_sig` encodes a `MetaProperty` (`Meta(meta.property)`). `import.meta` and `new.target` are
    // DISTINCT meta-properties with different runtime meaning. The wrapper is IDENTICAL on both sides
    // (`function f(){ return <X>; }` — `new.target` is only valid inside a function) and ONLY the
    // meta-property differs, so this isolates the `MetaProperty` axis: it would compare EQUAL (false-
    // PASS) if the `MetaProperty` arm were unsigned. Reachable through ordinary source-preservable JS.
    assert!(
        !sigs_equal(
            "function f(){ return import.meta; }",
            "function f(){ return new.target; }"
        ),
        "import.meta vs new.target must FAIL — the MetaProperty meta/property names are the SOLE \
         difference (identical function wrapper)"
    );
}

#[test]
fn struct_compare_fails_on_private_in_expression() {
    // `expr_sig` encodes a `PrivateInExpression` (`PrivateIn(#name in <right>)`). BOTH classes declare
    // the SAME member inventory (`#a; #b;`) and an IDENTICAL method, varying ONLY the brand checked in
    // the `in` expression (`#a in o` vs `#b in o`), so this isolates the `PrivateInExpression.left`
    // axis: it would compare EQUAL (false-PASS) if `PrivateInExpression` were unsigned. `#x in obj` is
    // only valid inside a class body — reachable through a source-preserved class body.
    assert!(
        !sigs_equal(
            "class C { #a; #b; m(o){ return #a in o; } }",
            "class C { #a; #b; m(o){ return #b in o; } }"
        ),
        "#a in o vs #b in o must FAIL — the PrivateInExpression.left brand is the SOLE difference \
         (identical class member inventory on both sides)"
    );
}

#[test]
fn struct_compare_signs_super_in_method() {
    // `expr_sig` now encodes `Super` as the bare leaf `Super` (not a Debug collapse). A method that
    // calls `super.m()` differs from one that calls a plain `m()` — the member OBJECT is `Super` on
    // one side and an identifier on the other — so they FAIL. Super is a leaf; one light assertion.
    // Reachable inside a source-preserved class method body.
    assert!(
        !sigs_equal(
            "class C extends B { m() { return super.x; } }",
            "class C extends B { m() { return this.x; } }"
        ),
        "a method member access through `super` vs `this` must FAIL (Super is signed as a distinct \
         leaf object)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Module import/export ORACLE axes. OFFICIAL Svelte client output CAN carry
// module-script imports/exports (the `matrix/module_import_export` golden proves a
// `clientModule` with `import {base} from "./base.js"; … export const VERSION = 1;`),
// so the comparator — the gate ORACLE — must compare these forms even though native
// Verter currently REFUSES `<script module>` in this branch. The import/export family
// is signed IN FULL (`ImportDeclaration` source/kind/phase/with-clause/specifiers;
// `ExportNamedDeclaration` declaration/specifiers/source/export-kind/with-clause;
// `ExportAllDeclaration` exported/source/export-kind/with-clause). These guards isolate
// the axes that PARSE under `SourceType::mjs()`: import-attribute key/value + the
// `with`/`assert` keyword, specifier-only export local/exported, export source,
// export-all source + namespace rename, hashbang. The `import type` / `export type`
// kind is TS-only syntax that is NOT parseable under the comparator's `SourceType::mjs()`
// parse, and the `import defer` phase ties to the namespace-import form, so those two
// fields are signed DEFENSIVELY (encoded, no planted positive discriminator) — not a
// reachable gap for the emitted-mjs oracle.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn struct_compare_fails_on_import_attribute_value_and_presence() {
    // `import d from "x" with { type: "json" }` vs `… with { type: "css" }` differ ONLY in the
    // import-attribute VALUE — the import source, specifiers, and clause keyword are identical — so
    // they must FAIL. The `with_clause`/`ImportAttribute` axes are signed.
    assert!(
        !sigs_equal(
            "import d from \"x\" with { type: \"json\" };",
            "import d from \"x\" with { type: \"css\" };"
        ),
        "two imports differing ONLY in the with-clause attribute value must FAIL (the import \
         attribute value is the SOLE difference)"
    );
    // Attribute PRESENCE: a bare import vs the SAME import carrying a `with`-clause must FAIL — the
    // clause is the SOLE difference.
    assert!(
        !sigs_equal(
            "import d from \"x\";",
            "import d from \"x\" with { type: \"json\" };"
        ),
        "an import with a `with`-clause vs the same import without one must FAIL (the with-clause \
         presence is the SOLE difference)"
    );
}

#[test]
fn struct_compare_fails_on_import_attribute_keyword() {
    // The import-attributes KEYWORD (`with` vs the legacy `assert`) is signed by `with_clause_sig`
    // (it signs `WithClause.keyword`). Two imports identical except the keyword must FAIL — the
    // keyword selects the attribute semantics (`with` = import attributes; `assert` = the legacy
    // import-assertions form). Both parse under `SourceType::mjs()`.
    assert!(
        !sigs_equal(
            "import d from \"x\" with { type: \"json\" };",
            "import d from \"x\" assert { type: \"json\" };"
        ),
        "an import-attributes `with` vs `assert` keyword (same source + attributes) must FAIL (the \
         WithClause keyword is the SOLE difference)"
    );
}

#[test]
fn struct_compare_fails_on_specifier_only_export() {
    // `export { a as value }` vs `export { b as value }` — both have NO inline declaration, an
    // IDENTICAL surrounding `const a=1,b=2;`, and the same exported name; only the LOCAL specifier
    // differs. Pre-fix the declaration-only `ExportNamed` arm dropped specifiers → both empty →
    // EQUAL (a false-PASS). The `ExportSpecifier` axis is signed.
    assert!(
        !sigs_equal(
            "const a=1,b=2; export { a as value };",
            "const a=1,b=2; export { b as value };"
        ),
        "two specifier-only exports differing ONLY in the local binding (`a as value` vs \
         `b as value`) must FAIL (the export specifier local is the SOLE difference)"
    );
}

#[test]
fn struct_compare_fails_on_export_source_reexport() {
    // `export { a } from "./x.js"` vs `… from "./y.js"` — a re-export differing ONLY in the source
    // module loads a DIFFERENT module, so they must FAIL. Pre-fix the `ExportNamed` arm dropped the
    // source → EQUAL. The `ExportNamedDeclaration.source` axis is signed.
    assert!(
        !sigs_equal(
            "export { a } from \"./x.js\";",
            "export { a } from \"./y.js\";"
        ),
        "two re-exports differing ONLY in the source module must FAIL (the export source is the SOLE \
         difference)"
    );
}

#[test]
fn struct_compare_fails_on_export_all_source_and_namespace() {
    // `export * from "./a.js"` vs `… from "./b.js"` — pre-fix BOTH fell to the `Stmt(discriminant)`
    // fallback → EQUAL (a false-PASS). The `ExportAllDeclaration` arm signs the source.
    assert!(
        !sigs_equal(
            "export * from \"./a.js\";",
            "export * from \"./b.js\";"
        ),
        "two export-all re-exports differing ONLY in the source must FAIL (the export-all source is \
         the SOLE difference)"
    );
    // A bare `export *` vs a NAMESPACE-renamed `export * as ns` over the SAME source differ ONLY in
    // the `exported` rename — they must FAIL.
    assert!(
        !sigs_equal(
            "export * from \"./a.js\";",
            "export * as ns from \"./a.js\";"
        ),
        "an `export *` vs `export * as ns` over the same source must FAIL (the namespace rename is \
         the SOLE difference)"
    );
}

#[test]
fn struct_compare_fails_on_hashbang() {
    // A `#!/usr/bin/env node` hashbang present vs absent over the SAME body must FAIL. Pre-fix
    // `program_sig` dropped `Program.hashbang` → EQUAL (a false-PASS). The hashbang axis is signed.
    assert!(
        !sigs_equal("#!/usr/bin/env node\nvar x = 1;", "var x = 1;"),
        "a `#!/usr/bin/env node` hashbang present vs absent must FAIL (the hashbang is the SOLE \
         difference)"
    );
}

#[test]
fn struct_compare_waives_directive_carrier_formatting_but_fails_on_directive_text() {
    // Directive CARRIER FORMATTING (quote style, escape representation) is cosmetic — the official
    // printer normalizes it, and only the COOKED value is in contract. `"use strict"` vs
    // `'use strict'` carry the SAME cooked value (OXC's raw directive token strips the surrounding
    // quotes), so they compare EQUAL both before and after the fix.
    assert!(
        sigs_equal("\"use strict\"; var x=1;", "'use strict'; var x=1;"),
        "a `\"use strict\"` vs `'use strict'` directive (same cooked value, different quote style) \
         must compare EQUAL (quote style is cosmetic)"
    );
    // ESCAPE REPRESENTATION is the genuine false-FAIL the cooked-value-only signature fixes:
    // `"\x75se strict"` and `"use strict"` have an IDENTICAL cooked value (`use strict`) but a
    // DIFFERENT raw token (`\x75se strict` vs `use strict`). Pre-fix `directive_sig` signed the raw
    // token → UNEQUAL (a false-FAIL); signing only the cooked value compares them EQUAL.
    assert!(
        sigs_equal("\"\\x75se strict\"; var x=1;", "\"use strict\"; var x=1;"),
        "an escape-equivalent directive (`\"\\x75se strict\"` vs `\"use strict\"`, same cooked value) \
         must compare EQUAL (escape representation is cosmetic carrier formatting)"
    );
    // A DIFFERENT directive TEXT still diverges — the cooked value differs.
    assert!(
        !sigs_equal("\"use strict\";", "\"use asm\";"),
        "a `\"use strict\"` vs `\"use asm\"` directive must FAIL (the cooked directive value differs)"
    );
}

/// THE guard for the `Compiled-Output Conformance (CRITICAL)` rule. It asserts the LANDED
/// structural comparator `sigs_equal` (the `conformance_sig` → `expr_sig`/`params_sig`/
/// `binding_sig`/`stmt_sig`/`statements_sig` AST family PLUS the `comment_sig` semantic-comment
/// signature over the Svelte client backend's emitted JS) DISCRIMINATES a cosmetic-only diff from
/// a behavioral / structural divergence — exactly the rule's bar: "a cosmetic-only diff passes; a
/// behavioral or structural divergence fails".
///
/// The cosmetic axes this parser-based oracle WAIVES are (1) outer/intra-expression whitespace (OXC
/// parses each side, so formatting is gone), (2) behavior-preserving redundant parentheses
/// (`unwrap_parens` peels transparent wrappers), and (3) NON-SEMANTIC comment trivia ONLY — plain
/// `// note` / `/* note */` and unknown `@foo` annotations. The rule's IN-CONTRACT comment boundary
/// (directive/pragma `/*@__PURE__*/`, license/preserve, source-map/`sourceURL`, TS-directive,
/// JSDoc) IS ENFORCED by the `comment_sig` semantic-comment signature: a semantic comment dropped,
/// corrupted, or MOVED to a different anchor compares UNEQUAL (the FAIL asserts below). The PASS-4
/// assertion below covers the WAIVED (non-semantic) half — a non-semantic comment still passes —
/// while the semantic-comment FAIL asserts cover the enforced half.
/// Per the rule's identifier clause, generated LOCAL IDENTIFIER SPELLINGS are waived ONLY when the
/// backend oracle
/// implements scope-aware alpha-equivalence for private bindings; this oracle does NOT — `expr_sig`
/// encodes `Id(name)` and `binding_sig` encodes `name:{id}` — so for THIS comparator identifiers are
/// STRUCTURAL and a consistent alpha-rename FAILS. That is the correct, rule-consistent behavior:
/// the comparator must not silently pass a rename, because nothing here proves the rename is a
/// behavior-preserving private binding.
///
/// This is a discrimination gate, not a passthrough: it would catch a comparator regression that
/// either started false-passing a structural drift (a loosened signature) OR started false-failing
/// a cosmetic diff (a paren/whitespace leak). Every input string parses as JS (the comparator
/// panics on a torn module). The NON-VACUITY assert (a byte-identical pair compares EQUAL) proves
/// the FAIL asserts genuinely discriminate rather than the comparator being trivially-unequal.
#[test]
fn svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs() {
    // ── NON-VACUITY: a byte-identical pair MUST compare EQUAL ──────────────────────────────────
    // Proves the comparator is not trivially-unequal: the FAIL asserts below genuinely
    // discriminate (an always-unequal comparator would pass them for the wrong reason).
    assert!(
        sigs_equal("var p = root();", "var p = root();"),
        "NON-VACUITY: a byte-identical module pair must compare EQUAL"
    );

    // ── PASS cases — cosmetic only (assert EQUAL) ─────────────────────────────────────────────

    // PASS 1: outer/intra-expression whitespace-only difference — the SAME call with identical
    // identifiers, re-spaced/newlined/tabbed. OXC discards formatting, so these are EQUAL.
    assert!(
        sigs_equal(
            "$.set_attribute(div,'id',a+b);",
            "$.set_attribute(\n\tdiv, 'id', a + b\n);"
        ),
        "PASS: an outer/intra-expression whitespace-only reformat (identical identifiers) must \
         compare EQUAL (whitespace is a waived cosmetic axis)"
    );

    // PASS 2: behavior-preserving redundant parentheses only — including a NESTED redundant-paren
    // variant — with identical identifiers. `unwrap_parens` peels the transparent wrappers.
    assert!(
        sigs_equal("$.set_text(t, a + b);", "$.set_text(t, (((a + b))));"),
        "PASS: a nested redundant-paren wrap the official printer drops (identical identifiers) \
         must compare EQUAL (redundant parens are a waived cosmetic axis)"
    );

    // PASS 3: paren + whitespace TOGETHER — an explicit third PASS proving both waived axes
    // compose. Same structure, identical identifiers, differing only by parens + formatting.
    assert!(
        sigs_equal(
            "$.set_attribute(div,'id',c?a:b);",
            "$.set_attribute(\n  div,\n  'id',\n  (c ? a : b)\n);"
        ),
        "PASS: a combined redundant-paren + whitespace reformat (identical identifiers) must \
         compare EQUAL (both waived cosmetic axes compose)"
    );

    // PASS 4: a plainly NON-SEMANTIC JS comment on one side only — a `// plain note` line comment
    // and a `/* note */` block comment. A non-semantic comment IS a waived cosmetic axis: the
    // semantic-comment signature (`comment_sig`) classifies these as cosmetic and DROPS them, so
    // the two sides compare EQUAL. The ENFORCED half — the in-contract semantic-comment boundary
    // (`/*@__PURE__*/` PURE-family, license/preserve, source-map/`sourceURL`, TS-directive, JSDoc)
    // — is exercised by the dedicated semantic-comment FAIL/PASS block further below (a semantic
    // drop / corruption / move FAILS; an unknown `@foo` annotation stays waived).
    assert!(
        sigs_equal(
            "$.set_text(t, a + b);",
            "// plain note\n$.set_text(t, /* note */ a + b);"
        ),
        "PASS: a plainly non-semantic JS comment (line + block) on one side only must compare EQUAL \
         (non-semantic comments are a waived cosmetic axis; the semantic-comment signature drops \
         them — the in-contract boundary is enforced separately below)"
    );

    // ── FAIL cases — behavioral / structural drift (assert UNEQUAL) ───────────────────────────

    // FAIL 1: consistent generated-local/param ALPHA-RENAME (same body rename). The identifier
    // axis: this oracle treats generated identifier spellings as STRUCTURAL (no scope-aware
    // alpha-equivalence), so a rename FAILS — consistent with the rule text.
    assert!(
        !sigs_equal(
            "($0) => $.set_text(t, $0);",
            "($x) => $.set_text(t, $x);"
        ),
        "FAIL: a consistent generated-param alpha-rename must FAIL (identifier spellings are \
         structural for this oracle; the rule waives them only under scope-aware alpha-equivalence)"
    );

    // FAIL 2: a renamed generated `var` local (`p` vs `q`) — `binding_sig` encodes `name:{id}`.
    assert!(
        !sigs_equal("var p = root();", "var q = root();"),
        "FAIL: a renamed generated `var` local must FAIL (the binding name is structural)"
    );

    // FAIL 3: swapped memo/effect param ORDER over the same deps, byte-identical body. `params_sig`
    // is order-sensitive, so the positional dep re-bind is caught.
    assert!(
        !sigs_equal(
            "$.template_effect(($0, $1) => $.set_text(t, `${$0} ${$1}`), [() => a(), () => b()]);",
            "$.template_effect(($1, $0) => $.set_text(t, `${$0} ${$1}`), [() => a(), () => b()]);"
        ),
        "FAIL: a swapped memo/effect param ORDER must FAIL (positional dep re-bind; param order is \
         structural)"
    );

    // FAIL 4: helper-name CHOICE change (`$.set_text` vs `$.set_attribute`).
    assert!(
        !sigs_equal("$.set_text(div, x);", "$.set_attribute(div, x);"),
        "FAIL: a changed helper NAME must FAIL (helper choice is structural)"
    );

    // FAIL 5: helper SEQUENCE reorder — two helpers emitted in swapped order. `statements_sig`
    // joins statements IN ORDER, so statement order is structural.
    assert!(
        !sigs_equal(
            "$.set_text(a, x); $.set_attribute(b, 'id', y);",
            "$.set_attribute(b, 'id', y); $.set_text(a, x);"
        ),
        "FAIL: a helper SEQUENCE reorder must FAIL (statement order is structural)"
    );

    // FAIL 6: missing `$.get` reactive read (`$.get(c)` dropped to a bare `c`).
    assert!(
        !sigs_equal("$.set_text(n, $.get(c));", "$.set_text(n, c);"),
        "FAIL: a dropped `$.get` reactive read must FAIL (a missing call is structural)"
    );

    // FAIL 7: derived/effect statement TOPOLOGY divergence — one side hoists a `var d =
    // $.derived(...)` declaration feeding `$.template_effect(... $.get(d) ...)`, the other inlines
    // the same computation directly into the `$.template_effect(...)` with NO `$.derived` decl. The
    // extra `$.derived` STATEMENT on one side changes `statements_sig` length + content (a genuine
    // memo/effect-topology difference), not merely an inner-expression shape. Both sides are valid
    // JS.
    assert!(
        !sigs_equal(
            "var d = $.derived(() => a() + b()); $.template_effect(() => $.set_text(t, $.get(d)));",
            "$.template_effect(() => $.set_text(t, a() + b()));"
        ),
        "FAIL: a hoisted `$.derived` memo decl feeding the effect vs an inlined direct effect must \
         FAIL (the memo/effect-topology axis: a different derived/effect statement topology)"
    );

    // FAIL 8: ATTR-vs-PROPERTY route (`$.set_attribute(input,'readonly',r)` vs the property
    // assignment `input.readOnly = r`) — a CallExpression vs an AssignmentExpression statement.
    assert!(
        !sigs_equal(
            "$.set_attribute(input, 'readonly', r);",
            "input.readOnly = r;"
        ),
        "FAIL: an attribute-helper route vs a direct property assignment must FAIL (distinct \
         routing; distinct statement shape)"
    );

    // FAIL 9: EVENT DELEGATION divergence — a dropped trailing `$.delegate([...])` call.
    assert!(
        !sigs_equal(
            "$.template_effect(() => $.set_text(t, x)); $.delegate(['click']);",
            "$.template_effect(() => $.set_text(t, x));"
        ),
        "FAIL: a dropped `$.delegate` event-delegation call must FAIL (a missing statement is \
         structural)"
    );

    // FAIL 10: HYDRATION/TEMPLATE topology divergence — a single clone-root path (`$.first_child`)
    // vs a multi-root fragment walk (`$.first_child` then `$.sibling`). Distinct walk topology.
    assert!(
        !sigs_equal(
            "var n = $.first_child(root);",
            "var n = $.sibling($.first_child(root));"
        ),
        "FAIL: a single clone-root walk vs a multi-root fragment walk must FAIL (distinct \
         hydration/template topology)"
    );

    // FAIL 11: OPTIONAL flag / extra call arg dropped (`$.child(p, true)` vs `$.child(p)`).
    assert!(
        !sigs_equal("$.child(p, true);", "$.child(p);"),
        "FAIL: a dropped optional flag / extra call arg must FAIL (call-arg count is structural)"
    );

    // FAIL 12: REJECT/DIAGNOSTIC ORDER — a reordered ordered statement sequence of the kinds the
    // client subset actually emits. Two `ExpressionStatement`s with distinct string-literal
    // payloads in swapped order (NOT bare `throw`s, which the comparator collapses to a
    // discriminant-only fallback). `Expr(Call(... Str("a")))` discriminates by payload + order.
    assert!(
        !sigs_equal("$.warn('a'); $.warn('b');", "$.warn('b'); $.warn('a');"),
        "FAIL: a reordered diagnostic/reject sequence must FAIL (ordered statement-payload order \
         is structural)"
    );

    // FAIL 13a: a call-arg-COUNT change not already covered (a trailing `true` added).
    assert!(
        !sigs_equal("$.html(n, () => h);", "$.html(n, () => h, true);"),
        "FAIL: a changed call-argument COUNT must FAIL (arg count is structural)"
    );

    // FAIL 13b: an operator change (`a + b` vs `a - b`) and a literal change (`'on'` vs `'off'`).
    assert!(
        !sigs_equal("var x = a + b;", "var x = a - b;"),
        "FAIL: a changed binary operator must FAIL (the operator is structural)"
    );
    assert!(
        !sigs_equal(
            "$.set_attribute(div, 'state', 'on');",
            "$.set_attribute(div, 'state', 'off');"
        ),
        "FAIL: a changed string-literal payload must FAIL (literal payload bytes are in contract)"
    );

    // ── IMPORT-SPECIFIER axis — the `stmt_sig` `ImportDeclaration` arm now encodes specifier
    // KIND / imported-name / local-name (no longer source + COUNT only). The rule treats imports
    // as STRUCTURAL, so a specifier-shape drift over the SAME source + count MUST FAIL. ────────

    // SANITY PASS: a byte-identical import compares EQUAL, and a whitespace-only reformat of the
    // SAME import compares EQUAL (the encoding stays whitespace-insensitive — it reads the AST,
    // not the bytes).
    assert!(
        sigs_equal(
            "import { a as $ } from 'svelte/internal/client';",
            "import { a as $ } from 'svelte/internal/client';"
        ),
        "PASS: a byte-identical specifier-bearing import must compare EQUAL"
    );
    assert!(
        sigs_equal(
            "import {a as $} from 'svelte/internal/client';",
            "import {\n  a as $\n} from 'svelte/internal/client';"
        ),
        "PASS: a whitespace-only import reformat (same source + specifiers) must compare EQUAL"
    );

    // FAIL — NAMESPACE-vs-NAMED over the same source + specifier count (1): the OLD source+count
    // encoding collapsed these to `Import(src,specs=1)`; the structural encoding distinguishes
    // `Namespace(*)` from `Named(...)`.
    assert!(
        !sigs_equal(
            "import * as $ from 'svelte/internal/client';",
            "import { a as $ } from 'svelte/internal/client';"
        ),
        "FAIL: a namespace import vs a named import (same source + count) must FAIL (specifier KIND \
         is structural)"
    );

    // FAIL — IMPORTED-NAME drift (`a` vs `b`) over the same local binding `$`: a different
    // imported helper from the SAME module is a behavioral divergence.
    assert!(
        !sigs_equal(
            "import { a as $ } from 'svelte/internal/client';",
            "import { b as $ } from 'svelte/internal/client';"
        ),
        "FAIL: an imported-name drift (`a as $` vs `b as $`) must FAIL (the imported symbol is \
         structural)"
    );

    // FAIL — LOCAL-NAME drift (`$` vs `_`) over the same imported symbol `a`: the local binding
    // name is part of the import's structural identity.
    assert!(
        !sigs_equal(
            "import { a as $ } from 'svelte/internal/client';",
            "import { a as _ } from 'svelte/internal/client';"
        ),
        "FAIL: a local-name drift (`a as $` vs `a as _`) must FAIL (the local binding is structural)"
    );

    // ── SEMANTIC-COMMENT axis — the in-contract comment boundary the rule keeps in contract
    // (`/*@__PURE__*/` PURE-family, license/preserve, source-map/`sourceURL`, TS-directive,
    // JSDoc) is now ENFORCED by the semantic-comment signature layered alongside `module_sig`.
    // A NON-SEMANTIC comment diff (`// note`, `/* note */`) stays WAIVED (compares EQUAL); a
    // SEMANTIC comment drop / corruption / move MUST FAIL. ──────────────────────────────────────

    // PASS (waived): the plainly-non-semantic comment case (a `// plain note` line + a `/* note */`
    // block on one side only compares EQUAL) is asserted once above as PASS 4 — not duplicated here.
    // PASS (waived): an UNKNOWN `@`-annotation comment (`/* @foo */`) is NOT semantic — OXC
    // classifies it `None`, the text predicate does not match — so it stays waived.
    assert!(
        sigs_equal("var x = f();", "var x = /* @foo */ f();"),
        "PASS: an unknown `@foo` annotation comment is NOT semantic and must compare EQUAL (waived)"
    );

    // FAIL — SEMANTIC DROP: a `/*@__PURE__*/` PURE annotation on one side dropped on the other.
    // The PURE annotation suppresses tree-shaking side-effect retention — dropping it is a real
    // behavioral / tool-consumed divergence.
    assert!(
        !sigs_equal("var x = /*@__PURE__*/ f();", "var x = f();"),
        "FAIL: a dropped `/*@__PURE__*/` PURE annotation must FAIL (PURE is an in-contract \
         semantic comment)"
    );

    // FAIL — SEMANTIC CORRUPTION: a license/preserve `/*! … */` whose payload bytes differ. The
    // exact preserve-form text is in contract (license bytes carry payload).
    assert!(
        !sigs_equal("/*! keep */\nvar x = 1;", "/*! changed */\nvar x = 1;"),
        "FAIL: a corrupted license/preserve `/*! … */` comment must FAIL (license payload bytes \
         are in contract)"
    );
    // FAIL — SEMANTIC DROP of a license comment entirely.
    assert!(
        !sigs_equal("/*! @license MIT */\nvar x = 1;", "var x = 1;"),
        "FAIL: a dropped license/preserve comment must FAIL (license is in contract)"
    );

    // FAIL — SEMANTIC MOVE: the SAME `/*@__PURE__*/` comment text anchored to a DIFFERENT
    // statement/expression. Both sides carry the identical PURE annotation and identical
    // statements, but on different anchors — a sequence-only oracle would MISS this; the
    // structural-anchor signature catches it.
    assert!(
        !sigs_equal(
            "var a = /*@__PURE__*/ f(); var b = g();",
            "var a = f(); var b = /*@__PURE__*/ g();"
        ),
        "FAIL: a semantic PURE comment moved to a different anchor must FAIL (anchor-precise \
         comment signature)"
    );

    // FAIL — SOURCE-MAP directive drop (`//# sourceMappingURL=…`): a tool-consumed source-comment.
    assert!(
        !sigs_equal("var x = 1;\n//# sourceMappingURL=x.js.map", "var x = 1;"),
        "FAIL: a dropped `//# sourceMappingURL=` source-map directive must FAIL (source comments \
         are in contract)"
    );
    // FAIL — TS-directive drop (`// @ts-nocheck`): a tool-consumed TS directive.
    assert!(
        !sigs_equal("// @ts-nocheck\nvar x = 1;", "var x = 1;"),
        "FAIL: a dropped `// @ts-nocheck` TS directive must FAIL (TS directives are in contract)"
    );
    // FAIL — JSDoc drop (`/** … */`): per the rule, JSDoc is in contract for this client-JS
    // oracle.
    assert!(
        !sigs_equal("/** @type {number} */\nvar x = 1;", "var x = 1;"),
        "FAIL: a dropped JSDoc `/** … */` comment must FAIL (JSDoc is in contract for this oracle)"
    );

    // SANITY (non-vacuity for the semantic axis): an IDENTICAL semantic comment at the SAME anchor
    // on both sides compares EQUAL — the semantic-comment signature is not trivially-unequal.
    assert!(
        sigs_equal("var x = /*@__PURE__*/ f();", "var x = /*@__PURE__*/ f();"),
        "NON-VACUITY: identical semantic comments at the same anchor must compare EQUAL"
    );

    // ── OCCURRENCE-PATH anchor — a semantic comment MOVED between two STRUCTURALLY-IDENTICAL
    // positions must FAIL. The OLD structural-shape anchor keyed by `stmt_sig`/`expr_sig`, so a
    // move between two identically-shaped statements/expressions collided to the same anchor+ord
    // and false-PASSED. The occurrence path keys by AST INDEX, so the move changes the path. ─────

    // FAIL — DUPLICATE-ANCHOR MOVE across two STRUCTURALLY-IDENTICAL statements. `/*@__PURE__*/ f();
    // f();` vs `f(); /*@__PURE__*/ f();`: both modules have two identical `f();` statements and one
    // PURE comment, differing ONLY in WHICH statement the PURE annotation leads. A structural-shape
    // anchor would map both `f()` calls to the identical anchor and compare EQUAL (a false-PASS);
    // the occurrence path (`stmt[0]` vs `stmt[1]`) distinguishes them.
    assert!(
        !sigs_equal("/*@__PURE__*/ f(); f();", "f(); /*@__PURE__*/ f();"),
        "FAIL: a PURE comment moved between two structurally-identical statements must FAIL \
         (occurrence-path anchor; the structural-shape anchor false-PASSED this)"
    );

    // FAIL — ARROW-BODY MOVE: an intra-statement move of a PURE comment between two
    // structurally-identical statements INSIDE a `$.template_effect(() => { … })` arrow body. The
    // top-level statement is byte-for-byte the same shape on both sides; only the arrow-body
    // statement index the comment leads differs (`a()` vs `b()`). The OLD anchor collapsed the
    // intra-arrow position onto the single top-level statement shape and false-PASSED; the
    // occurrence path descends `…/arrow.body.stmt[0]` vs `…/arrow.body.stmt[1]`.
    assert!(
        !sigs_equal(
            "$.template_effect(() => { /*@__PURE__*/ a(); b(); });",
            "$.template_effect(() => { a(); /*@__PURE__*/ b(); });"
        ),
        "FAIL: a PURE comment moved between two statements inside an arrow body must FAIL \
         (occurrence-path descends arrow.body.stmt[k]; the structural-shape anchor false-PASSED this)"
    );

    // FAIL — COMPUTED-KEY MOVE: a PURE comment moved between two structurally-identical COMPUTED
    // object-property keys. The descent walks `object.prop[k].key.expr` (mirroring `property_key_sig`'s
    // `as_expression()` path), so the move anchors to a different key path and FAILS; the pre-fix
    // descent walked only `op.value` + spreads and false-PASSED.
    assert!(
        !sigs_equal(
            "var o = { [/*@__PURE__*/ f()]: v, [f()]: v };",
            "var o = { [f()]: v, [/*@__PURE__*/ f()]: v };"
        ),
        "FAIL: a PURE comment moved between two structurally-identical computed object keys must FAIL \
         (occurrence-path descends object.prop[k].key.expr; the pre-fix descent false-PASSED this)"
    );

    // FAIL — PARAM MOVE: a semantic comment moved between two arrow PARAMS. The descent walks
    // `arrow.params[k]` (binding identifiers), so the move anchors to a different param path and
    // FAILS; the pre-fix descent did not walk params and false-PASSED.
    assert!(
        !sigs_equal(
            "var f = (/*! keep */ a, b) => a;",
            "var f = (a, /*! keep */ b) => a;"
        ),
        "FAIL: a semantic comment moved between two arrow params must FAIL \
         (occurrence-path descends arrow.params[k]; the pre-fix descent false-PASSED this)"
    );

    // PASS (waived) — LEADING-PAREN REMAP: a semantic comment leading a REDUNDANT outer paren
    // (`/*! keep */ (a)`) anchors IDENTICALLY to the same comment leading the bare node — a paren-only
    // difference is cosmetic, so it compares EQUAL. The pre-fix `expr_anchor` peeled the paren BEFORE
    // the leading probe and collapsed `(a)` to `<tail>` (a false-FAIL).
    assert!(
        sigs_equal("var x = /*! keep */ (a);", "var x = /*! keep */ a;"),
        "PASS: a leading semantic comment on a redundant paren must compare EQUAL to the bare node \
         (paren-transparent leading anchor; the pre-fix descent false-FAILED this)"
    );

    // SANITY (no false-FAIL): the SAME semantic comment at the SAME structural position with a
    // whitespace + redundant-paren-only difference still compares EQUAL — the occurrence path reads
    // AST topology, not bytes, and peels transparent parens without adding a segment.
    assert!(
        sigs_equal(
            "var x = /*@__PURE__*/ f((a));",
            "var x =   /*@__PURE__*/   f(a);"
        ),
        "SANITY: the same PURE comment at the same occurrence path differing only by whitespace + a \
         redundant paren must still compare EQUAL (occurrence path is byte/paren insensitive)"
    );

    // ── TRAILING `CommentPosition` — OXC computes `attached_to` for LEADING comments only and
    // leaves a TRAILING comment's `attached_to = 0`. Anchoring a trailing comment on `attached_to`
    // would collapse every trailing comment onto the first statement; `comment_sig` branches on
    // `comment.position` and anchors a trailing comment via the PRECEDING node's occurrence path. ──

    // FAIL — TRAILING SEMANTIC DROP: a trailing `//# sourceMappingURL=` directive dropped. A
    // trailing source-map directive after the last statement is in contract; dropping it must FAIL.
    assert!(
        !sigs_equal("var x = 1; //# sourceMappingURL=a.map", "var x = 1;"),
        "FAIL: a dropped TRAILING `//# sourceMappingURL=` directive must FAIL (trailing comments \
         anchor via the preceding node, not byte 0)"
    );

    // FAIL — TRAILING SEMANTIC MOVE to a different preceding statement. The SAME trailing PURE
    // comment trails `f();` (stmt[0]) on one side and `g();` (stmt[1]) on the other; both modules
    // have identical statements + the identical trailing comment text. A trailing `h();` keeps the
    // comment mid-module so OXC classifies BOTH as `pos=Trailing` with `attached_to = 0` (verified:
    // attached_to-only anchoring maps BOTH to byte 0 → the first statement → false-PASS, the FIX-2
    // bug). The Trailing→preceding-node occurrence path anchors side A on `stmt[0]` and side B on
    // `stmt[1]`, so the move FAILS.
    assert!(
        !sigs_equal(
            "f(); /*@__PURE__*/\ng();\nh();",
            "f();\ng(); /*@__PURE__*/\nh();"
        ),
        "FAIL: a trailing semantic comment moved to a different preceding statement must FAIL \
         (trailing anchor follows the preceding node, not byte 0)"
    );

    // FAIL — MULTI-DIGIT-INDEX trailing MOVE: the closest-preceding-node selection must be by the
    // matched node's `span.end`, NOT by lexicographic path order. With ≥10 preceding `stmt[N]`
    // siblings, the lexicographically-largest path among the preceding set is `stmt[9]/…` for BOTH
    // a comment after stmt[10] (preceding {0..10}) AND a comment after stmt[9] (preceding {0..9}) —
    // so a depth/lexicographic tiebreak COLLIDES the two genuinely-different positions onto stmt[9]
    // (a false-PASS). The `span.end`-keyed selection anchors the comment to the immediately-
    // preceding node — stmt[10] vs stmt[9] — so the move correctly FAILS. Build N identical `x();`
    // statements with the trailing PURE inserted after exactly `n` of them; a trailing `z();` keeps
    // OXC classifying the comment `pos=Trailing`.
    {
        let trailing_after = |n: usize| -> String {
            format!(
                "{}/*@__PURE__*/\n{} z();",
                "x(); ".repeat(n),
                "x();".repeat(12 - n)
            )
        };
        let after_stmt10 = trailing_after(11); // PURE trails the 11th statement = stmt[10].
        let after_stmt9 = trailing_after(10); // PURE trails the 10th statement = stmt[9].
        assert!(
            !sigs_equal(&after_stmt10, &after_stmt9),
            "FAIL: a trailing comment after stmt[10] vs stmt[9] must FAIL (closest-preceding-node \
             selection is by span.end, not lexicographic path order — both collide onto stmt[9] \
             under a buggy lexicographic tiebreak)"
        );
    }

    // SANITY: a LEADING vs a TRAILING semantic comment at the SAME node (stmt[0] `f()`) are DISTINCT
    // positions — `pos=Leading` vs `pos=Trailing` keeps them apart. Side A leads `f()`
    // (`pos=Leading`); side B trails `f()` (`pos=Trailing`, attached_to=0, kept mid-module by the
    // following statements). A leading-vs-trailing-at-the-same-node difference is a real position
    // change, so it must FAIL.
    assert!(
        !sigs_equal(
            "/*@__PURE__*/ f(); g(); h();",
            "f(); /*@__PURE__*/\ng();\nh();"
        ),
        "FAIL: a leading vs a trailing semantic comment at the same node must FAIL (pos is part of \
         the anchor identity)"
    );

    // ── TAXONOMY token boundaries + legacy source-map forms ──────────────────────────────────────

    // PASS (waived): `@ts-ignore-me` is a LOOKALIKE, not the `@ts-ignore` directive — the `-me`
    // continues the token past the directive boundary, so it stays WAIVED (compares EQUAL).
    assert!(
        sigs_equal("var x = f();", "var x = /* @ts-ignore-me */ f();"),
        "PASS: `@ts-ignore-me` is a non-directive lookalike (token boundary) and must compare EQUAL"
    );

    // PASS (waived): `/// <referencee path=…>` is a LOOKALIKE, not a triple-slash reference — the
    // trailing `e` continues the token past `<reference`, so it stays WAIVED.
    assert!(
        sigs_equal(
            "var x = 1;",
            "/// <referencee path=\"x\" />\nvar x = 1;"
        ),
        "PASS: `<referencee …>` is a non-directive lookalike (token boundary) and must compare EQUAL"
    );

    // FAIL — LEGACY source-map directive drop (`//@ sourceMappingURL=`): the deprecated `@`-prefixed
    // legacy form is in contract alongside the modern `//#` form; dropping it must FAIL.
    assert!(
        !sigs_equal("var x = 1;\n//@ sourceMappingURL=x.js.map", "var x = 1;"),
        "FAIL: a dropped legacy `//@ sourceMappingURL=` directive must FAIL (legacy source-map forms \
         are in contract)"
    );
    // FAIL — LEGACY `//@ sourceURL=` drop likewise.
    assert!(
        !sigs_equal("var x = 1;\n//@ sourceURL=x.js", "var x = 1;"),
        "FAIL: a dropped legacy `//@ sourceURL=` directive must FAIL (legacy source-map forms are in \
         contract)"
    );
    // SANITY: the genuine `@ts-ignore` directive (exact token, then a boundary) is STILL semantic —
    // dropping it must FAIL (the boundary fix must not over-narrow the real directive).
    assert!(
        !sigs_equal("// @ts-ignore\nvar x = 1;", "var x = 1;"),
        "FAIL: a dropped genuine `// @ts-ignore` directive must still FAIL (boundary fix keeps the \
         real directive semantic)"
    );

    // ── NEWLY-ENCODED SOURCE-PRESERVED AXES — object-property shape, param defaults, async/
    // generator. Each pair collapsed to the SAME signature under the pre-fix encoding (a silent
    // structural false-PASS) and now DISCRIMINATES; the `// @ts-check` LINE-only pragma split FAILs
    // a dropped real pragma while WAIVING the block/lookalike forms. Reachable via source-preserved
    // author expressions the value-position emitter byte-copies. ────────────────────────────────

    // FAIL — OBJECT-PROPERTY KIND: a getter vs a value at the same key (op.kind get vs init).
    assert!(
        !sigs_equal(
            "var o = { get x() { return 1; } };",
            "var o = { x: () => 1 };"
        ),
        "FAIL: a getter vs a value object property must FAIL (op.kind is structural)"
    );
    // FAIL — OBJECT-PROPERTY METHOD: a method-shorthand vs a function-value (op.method).
    assert!(
        !sigs_equal(
            "var o = { x() { return 1; } };",
            "var o = { x: function() { return 1; } };"
        ),
        "FAIL: a method-shorthand vs a function-value property must FAIL (op.method is structural)"
    );
    // FAIL — OBJECT-PROPERTY SHORTHAND: `{ a }` vs `{ a: a }` (op.shorthand; __proto__ semantics).
    assert!(
        !sigs_equal("var o = { a };", "var o = { a: a };"),
        "FAIL: a shorthand vs a longhand property must FAIL (op.shorthand is structural)"
    );
    // FAIL — OBJECT-PROPERTY COMPUTED: a static key vs a computed key (op.computed).
    assert!(
        !sigs_equal("var o = { x: v };", "var o = { [x]: v };"),
        "FAIL: a static key vs a computed key must FAIL (op.computed is structural)"
    );
    // FAIL — STATIC-KEY COMMENT MOVE: a semantic comment moved between two static object keys
    // anchors to a different `object.prop[i].key` and FAILs (the pre-fix descent false-PASSED this).
    assert!(
        !sigs_equal(
            "var o = { /*! keep */ a: v, a: v };",
            "var o = { a: v, /*! keep */ a: v };"
        ),
        "FAIL: a semantic comment moved between two static object keys must FAIL \
         (object.prop[i].key anchor; the pre-fix descent false-PASSED this)"
    );
    // FAIL — PARAM DEFAULT: a changed/absent param default (FormalParameter.initializer).
    assert!(
        !sigs_equal("var f = (a = 1) => a;", "var f = (a = 2) => a;"),
        "FAIL: a changed param default value must FAIL (FormalParameter.initializer is structural)"
    );
    assert!(
        !sigs_equal("var f = (a = 1) => a;", "var f = (a) => a;"),
        "FAIL: a present vs absent param default must FAIL (init present/absent is structural)"
    );
    // FAIL — ASYNC arrow vs sync arrow (r#async).
    assert!(
        !sigs_equal("var f = async () => 1;", "var f = () => 1;"),
        "FAIL: an async arrow vs a sync arrow must FAIL (r#async is structural)"
    );
    // FAIL — GENERATOR vs plain function (generator), in an export-default form so a top-level
    // statement carries the bit.
    assert!(
        !sigs_equal(
            "export default function* f() {}",
            "export default function f() {}"
        ),
        "FAIL: a generator function vs a plain function must FAIL (generator is structural)"
    );

    // PASS (waived) — `/* @ts-check */` BLOCK form is not a valid pragma (line-only) and stays
    // waived; `// @ts-check/foo` is a lookalike past the strict boundary and stays waived.
    assert!(
        sigs_equal("var x = 1;", "/* @ts-check */\nvar x = 1;"),
        "PASS: `/* @ts-check */` block form is not a valid pragma and must compare EQUAL (line-only)"
    );
    assert!(
        sigs_equal("var x = 1;", "// @ts-check/foo\nvar x = 1;"),
        "PASS: `// @ts-check/foo` is a non-pragma lookalike (strict boundary) and must compare EQUAL"
    );
    // FAIL — a dropped genuine `// @ts-check` LINE pragma must still FAIL (split keeps it semantic).
    assert!(
        !sigs_equal("// @ts-check\nvar x = 1;", "var x = 1;"),
        "FAIL: a dropped genuine `// @ts-check` line pragma must FAIL (line-form pragma stays \
         semantic after the split)"
    );

    // SANITY (non-vacuity for the new axes): byte-identical pairs of each new shape compare EQUAL.
    assert!(
        sigs_equal(
            "var o = { get x() { return 1; } };",
            "var o = { get x() { return 1; } };"
        ) && sigs_equal("var f = (a = 1) => a;", "var f = (a = 1) => a;")
            && sigs_equal("var f = async () => 1;", "var f = async () => 1;"),
        "NON-VACUITY: byte-identical getter / param-default / async-arrow pairs must compare EQUAL"
    );
}

/// Every supported emission slug — the headline / robustness fixtures
/// ([`SUPPORTED_FIXTURES`]), the exhaustive supported-sub-shape matrix
/// ([`SUPPORTED_MATRIX`]), and the per-surface corpora (attributes, events,
/// blocks, declaration tags, components, specials, lifecycle, custom-element
/// options). Every group runs through the identical compile + OXC-parse +
/// full-module-comparison gate.
fn all_supported_slugs() -> Vec<&'static str> {
    SUPPORTED_FIXTURES
        .iter()
        .chain(SUPPORTED_MATRIX.iter())
        .chain(SUPPORTED_ATTRIBUTES.iter())
        .chain(SUPPORTED_EVENTS.iter())
        .chain(SUPPORTED_BLOCKS.iter())
        .chain(SUPPORTED_DECLARATION_TAGS.iter())
        .chain(SUPPORTED_COMPONENTS.iter())
        .chain(SUPPORTED_SPECIALS.iter())
        .chain(SUPPORTED_LIFECYCLE.iter())
        .chain(SUPPORTED_OPTIONS.iter())
        .chain(SUPPORTED_STORES.iter())
        .chain(SUPPORTED_LEGACY.iter())
        .chain(SUPPORTED_CSS.iter())
        .copied()
        .collect()
}

/// Every corpus const MUST be wired into `all_supported_slugs()`. The per-corpus
/// coverage tests iterate their const DIRECTLY, and the full-topology suite
/// (`emitted_client_topology_matches_official_goldens`,
/// `every_supported_fixture_emits_valid_js`) is the ONLY consumer that actually
/// compiles each fixture — so a dropped `.chain(CONST.iter())` silently removes
/// that whole corpus's behavioral/topology coverage while every other test stays
/// green. This guard fails closed on a missing chain: it asserts each const's
/// slugs are enumerated by `all_supported_slugs()` AND that the aggregate length
/// is exactly the sum of the corpora (a dropped/duplicated chain shifts the
/// count), so no sub-corpus can fall out of the full suite unnoticed.
#[test]
fn all_supported_slugs_wires_every_corpus_const() {
    let corpora: &[(&str, &[&str])] = &[
        ("SUPPORTED_FIXTURES", SUPPORTED_FIXTURES),
        ("SUPPORTED_MATRIX", SUPPORTED_MATRIX),
        ("SUPPORTED_ATTRIBUTES", SUPPORTED_ATTRIBUTES),
        ("SUPPORTED_EVENTS", SUPPORTED_EVENTS),
        ("SUPPORTED_BLOCKS", SUPPORTED_BLOCKS),
        ("SUPPORTED_DECLARATION_TAGS", SUPPORTED_DECLARATION_TAGS),
        ("SUPPORTED_COMPONENTS", SUPPORTED_COMPONENTS),
        ("SUPPORTED_SPECIALS", SUPPORTED_SPECIALS),
        ("SUPPORTED_LIFECYCLE", SUPPORTED_LIFECYCLE),
        ("SUPPORTED_OPTIONS", SUPPORTED_OPTIONS),
        ("SUPPORTED_STORES", SUPPORTED_STORES),
        ("SUPPORTED_LEGACY", SUPPORTED_LEGACY),
        ("SUPPORTED_CSS", SUPPORTED_CSS),
    ];
    let all = all_supported_slugs();
    let all_set: std::collections::HashSet<&str> = all.iter().copied().collect();
    for (name, corpus) in corpora {
        for &slug in *corpus {
            assert!(
                all_set.contains(slug),
                "corpus const `{name}` slug `{slug}` is not enumerated by \
                 all_supported_slugs() — a `.chain({name}.iter())` wiring is missing, \
                 silently dropping this corpus's full-topology coverage"
            );
        }
    }
    let total: usize = corpora.iter().map(|(_, corpus)| corpus.len()).sum();
    assert_eq!(
        all.len(),
        total,
        "all_supported_slugs() enumerates {} slugs but the corpus consts sum to {} \
         — a `.chain(...)` was dropped or duplicated",
        all.len(),
        total
    );
}

#[test]
fn supported_stores_cover_the_full_store_corpus() {
    // The store corpus is the structural oracle for the `$store` auto-subscription
    // surface (accessor thunk, setup_stores/$$cleanup, store_set/update_store
    // writes, the mode-sensitive frame); a dropped row is a coverage regression.
    assert_eq!(
        SUPPORTED_STORES.len(),
        16,
        "the store corpus must enumerate all 16 `stores/*` fixtures (auto_subscribe, \
         runes_mode, legacy_only, maybe_runes_attr_call, write, compound, multiple, \
         derived_shadowed, local_factory, bind_value, rune_named_state, \
         rune_named_state_runes, rune_named_derived, each_nonshadow, class_local, \
         custom_element)"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_STORES {
        assert!(
            slug.starts_with("stores/"),
            "supported-store slug {slug} must be a stores/* fixture"
        );
        assert!(seen.insert(slug), "duplicate supported-store slug {slug}");
    }
    // Directory coverage: every `stores/*` fixture on disk is enumerated — a new
    // fixture cannot silently skip the gate.
    let fixtures =
        repo_root().join("crates/verter_compiler/tests/svelte_oracle_corpus/fixtures/stores");
    for entry in std::fs::read_dir(&fixtures).expect("read stores fixtures") {
        let name = entry.expect("dir entry").file_name();
        let name = name.to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".svelte") else {
            continue;
        };
        let slug = format!("stores/{stem}");
        assert!(
            seen.contains(slug.as_str()),
            "stores fixture {slug} is not enumerated in SUPPORTED_STORES"
        );
    }
}

#[test]
fn supported_lifecycle_covers_the_full_lifecycle_corpus() {
    // The lifecycle-directive corpus (5f-c) is the structural oracle for `use:` /
    // `transition:`/`in:`/`out:` / `animate:` / element `{@attach}`; a dropped row is a
    // coverage regression. This count gate fails LOUDLY if a row is dropped, and the
    // no-duplicate check guards against a typo.
    assert_eq!(
        SUPPORTED_LIFECYCLE.len(),
        42,
        "the lifecycle corpus must enumerate all 42 `lifecycle/*` 5f-c fixtures (use \
         noarg/arg/dotted, the 8-cell transition FLAG map both/in/out/in-global/out-global/\
         both-global/local/params, animate keyed/keyed-params/keyed-const, attach \
         element/colocated, the spread co-location row, the `use:`+legacy-event \
         effect-wrap rows single/multi/transition-order, the use:↔bind:this \
         source-interleave row, the 7 dynamic-children placement rows use-dynamic/\
         use-text/attach-dynamic/bind-this-dynamic/use-event-dynamic/use-nested-sibling/\
         transition-dynamic, the 12 event-ORIGIN rows: use-modern-nondelegated/\
         transition-legacy-order/animate-legacy-order/transition-modern-nondelegated/\
         use-legacy-nondelegated/multiple-use/in-out-same/use-nested-if-child/\
         lifecycle-in-if/legacy-before-transition/transition-parent-legacy-child/\
         legacy-parent-transition-child, and the 2 non-this bind linearization rows: \
         use-bind-value/transition-bind-value)"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_LIFECYCLE {
        assert!(
            seen.insert(slug),
            "duplicate supported-lifecycle slug {slug}"
        );
    }
}

#[test]
fn supported_legacy_cover_the_full_legacy_corpus() {
    // The legacy corpus is the structural oracle for the legacy reactivity
    // surface (`export let` prop-source lowering + the demand-driven `let` →
    // `$.mutable_source` promotion); a dropped row is a coverage regression.
    assert_eq!(
        SUPPORTED_LEGACY.len(),
        51,
        "the legacy corpus must enumerate all 51 `legacy/*` fixtures (export_let \
         bare/default/mutated/reassigned/multiple/sibling_default, let function_write/\
         handler_write/bind_value/bind_uninit/bind_member/member_mutate/bind_window, \
         reactive assign/block/if/topo_order/prop_store/prop_only/prop_write/for_shadow/\
         paren_assign, slot default/named_props/spread/fallback/root_fallback/prop_call/\
         spread_call, component prop_call/member_prop, dispatcher used/unused, the \
         template-effect value-wrap rows attr_text_call_wrap/attr_member_inline_wrap/\
         attr_two_positions_call/attr_imported_call/attr_plain_local_call/\
         attr_mixed_call_member/class_directive_call_raw, and the unified-preparation \
         wrap rows wrap_class_clsx_call/wrap_style_directive_call/wrap_if_elseif_call/\
         wrap_each_collection_call/wrap_await_key_call/wrap_html_call/wrap_const_call/\
         wrap_attach_call/wrap_title_mixed_call/wrap_spread_colocated_call/\
         wrap_svelte_element_style_dir)"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_LEGACY {
        assert!(
            slug.starts_with("legacy/"),
            "supported-legacy slug {slug} must be a legacy/* fixture"
        );
        assert!(seen.insert(slug), "duplicate supported-legacy slug {slug}");
    }
    // Directory coverage: every `legacy/*` fixture on disk is enumerated — a new
    // fixture must join the corpus (or be consciously excluded here).
    let dir = repo_root().join("crates/verter_compiler/tests/svelte_oracle_corpus/fixtures/legacy");
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read legacy fixture dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "svelte").unwrap_or(false))
        .map(|p| format!("legacy/{}", p.file_stem().unwrap().to_string_lossy()))
        .collect();
    on_disk.sort();
    let mut enumerated: Vec<String> = SUPPORTED_LEGACY.iter().map(|s| s.to_string()).collect();
    enumerated.sort();
    assert_eq!(
        enumerated, on_disk,
        "every legacy/* fixture on disk must be enumerated in SUPPORTED_LEGACY"
    );
}

#[test]
fn lifecycle_transition_flag_map_is_the_official_bit_arithmetic() {
    // The transition FLAG map is semantic, not cosmetic: `in`=1, `out`=2, `transition`
    // (both)=3; `|global` adds 4 (→ 5/6/7); `|local` is the default (NO +4, so
    // `transition|local`=3). Assert the exact integer literal in each emitted module —
    // a wrong flag flips the runtime intro/outro/global behavior while keeping the
    // helper topology identical, so the sequence oracle alone cannot catch it.
    for (slug, flag) in [
        ("lifecycle/transition_both", 3),
        ("lifecycle/transition_in", 1),
        ("lifecycle/transition_out", 2),
        ("lifecycle/transition_in_global", 5),
        ("lifecycle/transition_out_global", 6),
        ("lifecycle/transition_both_global", 7),
        ("lifecycle/transition_local", 3),
    ] {
        let code = emit(slug);
        let expected = format!("$.transition({flag}, ");
        assert!(
            code.contains(&expected),
            "{slug} must emit `{expected}…` (the official FLAG arithmetic):\n{code}"
        );
        // NEGATIVE: exactly one transition call, and no other flag integer leaked in.
        assert_eq!(
            code.matches("$.transition(").count(),
            1,
            "{slug} must emit exactly ONE `$.transition` call:\n{code}"
        );
    }
    // The params thunk is present IFF params are given: the bare form emits 3 args
    // (no trailing thunk), the params form emits the 4th `() => ({ … })` getter.
    let bare = emit("lifecycle/transition_both");
    assert!(
        bare.contains("$.transition(3, div, () => fade);"),
        "the no-params transition emits exactly 3 args (no getParams thunk):\n{bare}"
    );
    let params = emit("lifecycle/transition_params");
    assert!(
        params.contains("$.transition(3, div, () => fade, ") && params.contains("duration: 200"),
        "the params transition emits the 4th getParams thunk:\n{params}"
    );
}

#[test]
fn lifecycle_animate_emits_animation_not_transition_and_widens_each_flags() {
    // `animate:` is its OWN op family: `$.animation(el, () => fn, PARAMS)` — ALWAYS
    // 3 args (`null` when no params) — NEVER a `$.transition` masquerade. And the
    // KEYED each hosting the animated element widens its FLAGS by `EACH_IS_ANIMATED`
    // (8): the runes keyed base here is 17 (EACH_ITEM_IMMUTABLE 16 | EACH_ITEM_REACTIVE
    // 1), so the animate each pins 25.
    let code = emit("lifecycle/animate_keyed");
    assert!(
        code.contains("$.animation(div, () => flip, null);"),
        "animate_keyed must emit the 3-arg `$.animation` with the literal `null`:\n{code}"
    );
    assert!(
        !code.contains("$.transition("),
        "an `animate:` directive must NOT emit `$.transition`:\n{code}"
    );
    assert!(
        code.contains("$.each(node, 25, "),
        "the keyed-animate each must OR in EACH_IS_ANIMATED=8 (16|1|8 = 25):\n{code}"
    );
    let params = emit("lifecycle/animate_keyed_params");
    assert!(
        params.contains("$.animation(div, () => flip, () => ({ duration: 200 }));"),
        "animate_keyed_params must emit the getParams thunk as the 3rd arg:\n{params}"
    );
    assert!(
        params.contains("$.each(node, 25, "),
        "the params variant keeps the ANIMATED flag widening:\n{params}"
    );
    // NEGATIVE: the non-animated keyed corpus stays UN-widened (flag 8 absent) — the
    // widening is animate-scoped, not a blanket keyed-each change.
    let keyed_plain = emit("blocks/each_keyed_index");
    assert!(
        keyed_plain.contains("$.each(node, 19, "),
        "a keyed each WITHOUT `animate:` keeps its un-widened flags (19):\n{keyed_plain}"
    );
}

#[test]
fn lifecycle_attach_is_attribute_position_and_action_callee_shapes_hold() {
    // Element-position `{@attach expr}` emits the 2-arg `$.attach(el, () => expr)` getter
    // thunk over the PREPARED payload (the official `b.thunk` shape — `() => fn`);
    // the action closures use the EXACT official param names `$$node` /
    // `$$action_arg` with the optional-chained callee.
    let attach = emit("lifecycle/attach_element");
    assert!(
        attach.contains("$.attach(div, () => fn);"),
        "attach_element must emit the 2-arg getter-thunk `$.attach`:\n{attach}"
    );
    let noarg = emit("lifecycle/use_noarg");
    assert!(
        noarg.contains("$.action(div, ($$node) => foo?.($$node));"),
        "use_noarg must emit the official `$$node` closure + optional-chain call:\n{noarg}"
    );
    let arg = emit("lifecycle/use_arg");
    assert!(
        arg.contains("$.action(div, ($$node, $$action_arg) => foo?.($$node, $$action_arg), "),
        "use_arg must emit the `$$action_arg` closure + the 3rd getter-thunk arg:\n{arg}"
    );
    let dotted = emit("lifecycle/use_dotted");
    assert!(
        dotted.contains("obj.foo?.($$node)"),
        "use_dotted must preserve the dotted callee literally:\n{dotted}"
    );
    // Spread co-location: `$.attribute_effect` (the spread fold) precedes the action,
    // which precedes the transition — the official init-order for fixture #18.
    let spread = emit("lifecycle/spread_lifecycle");
    let fold = spread
        .find("$.attribute_effect(")
        .expect("spread_lifecycle emits the spread fold");
    let action = spread
        .find("$.action(")
        .expect("spread_lifecycle emits the action");
    let transition = spread
        .find("$.transition(")
        .expect("spread_lifecycle emits the transition");
    assert!(
        fold < action && action < transition,
        "spread co-location order must be attribute_effect → action → transition:\n{spread}"
    );
}

#[test]
fn lifecycle_event_origin_gates_effect_wrap_and_directive_batch_order() {
    // The effect wrap AND the event↔transition/animation ordering key on the LEGACY
    // `on:` ORIGIN, not on delegation (official `RegularElement.js`: only an
    // `OnDirective` joins `other_directives` — a modern `on*` attribute pushes its
    // `$.event` BEFORE the element's directive batch and never effect-wraps).
    //
    // A MODERN non-delegated event on a `use:` host stays a BARE `$.event(…)` —
    // the wrap trigger is legacy-`on:`-only.
    let modern_use = emit("lifecycle/use_modern_nondelegated_event");
    assert!(
        modern_use.contains("$.event('mouseenter', div, "),
        "use:+modern non-delegated must emit the BARE $.event:\n{modern_use}"
    );
    assert!(
        !modern_use.contains("$.effect(() => $.event("),
        "use:+modern non-delegated must NOT be effect-wrapped:\n{modern_use}"
    );
    // The LEGACY `on:` form on the same host DOES wrap (the positive contrast).
    let legacy_use = emit("lifecycle/use_legacy_nondelegated_event");
    assert!(
        legacy_use.contains("$.effect(() => $.event('mouseenter', div, "),
        "use:+legacy on: must effect-wrap the registration:\n{legacy_use}"
    );
    // A bare LEGACY `on:` event joins the directive batch: `$.transition` /
    // `$.animation` BEFORE `$.event` when the directive precedes it in source.
    let legacy_transition = emit("lifecycle/transition_legacy_event_order");
    let t = legacy_transition
        .find("$.transition(")
        .expect("transition_legacy_event_order emits the transition");
    let e = legacy_transition
        .find("$.event(")
        .expect("transition_legacy_event_order emits the event");
    assert!(
        t < e,
        "a source-first transition precedes the bare legacy on: event:\n{legacy_transition}"
    );
    let legacy_animate = emit("lifecycle/animate_legacy_event_order");
    let a = legacy_animate
        .find("$.animation(")
        .expect("animate_legacy_event_order emits the animation");
    let e = legacy_animate
        .find("$.event(")
        .expect("animate_legacy_event_order emits the event");
    assert!(
        a < e,
        "a source-first animation precedes the bare legacy on: event:\n{legacy_animate}"
    );
    // A MODERN non-delegated event stays BEFORE the transition (the pre-batch phase).
    let modern_transition = emit("lifecycle/transition_modern_nondelegated_event");
    let e = modern_transition
        .find("$.event(")
        .expect("transition_modern_nondelegated_event emits the event");
    let t = modern_transition
        .find("$.transition(")
        .expect("transition_modern_nondelegated_event emits the transition");
    assert!(
        e < t,
        "a modern non-delegated event precedes the transition:\n{modern_transition}"
    );
    // The batch is SOURCE-ordered, not a hard events-after-transitions phase: a
    // source-first legacy event precedes the transition.
    let legacy_first = emit("lifecycle/legacy_event_before_transition");
    let e = legacy_first
        .find("$.event(")
        .expect("legacy_event_before_transition emits the event");
    let t = legacy_first
        .find("$.transition(")
        .expect("legacy_event_before_transition emits the transition");
    assert!(
        e < t,
        "a source-first bare legacy on: event precedes the transition:\n{legacy_first}"
    );
    // Element batches merge POST-ORDER: a CHILD's batch item precedes the PARENT's,
    // in both nesting directions.
    let n1 = emit("lifecycle/transition_parent_legacy_event_child");
    let e = n1
        .find("$.event('click', span, ")
        .expect("transition_parent_legacy_event_child emits the child event");
    let t = n1
        .find("$.transition(")
        .expect("transition_parent_legacy_event_child emits the parent transition");
    assert!(
        e < t,
        "the child's bare legacy event precedes the parent's transition:\n{n1}"
    );
    let n2 = emit("lifecycle/legacy_event_parent_transition_child");
    let t = n2
        .find("$.transition(")
        .expect("legacy_event_parent_transition_child emits the child transition");
    let e = n2
        .find("$.event('click', div, ")
        .expect("legacy_event_parent_transition_child emits the parent event");
    assert!(
        t < e,
        "the child's transition precedes the parent's bare legacy event:\n{n2}"
    );
}

#[test]
fn supported_specials_cover_the_special_host_corpus() {
    // The special-element corpus (5f-b) is the structural oracle for the host / renderable
    // specials; a dropped row is a coverage regression. This count gate fails LOUDLY if a row
    // is dropped, and the no-duplicate check guards against a typo.
    assert_eq!(
        SUPPORTED_SPECIALS.len(),
        37,
        "the special corpus must enumerate the 7 host-special + 11 svelte:element + 12 \
         svelte:boundary + 7 svelte:head `special/*` 5f-b fixtures (window/document/body \
         events+binds+this, svelte_element static/dynamic/attrs/bind_this/dimension/empty/child/\
         class_directive/class_mixed_case/style_directive/this_and_fold, svelte_boundary \
         plain/onerror/failed/pending/full + the failed/pending/all ATTRIBUTE forms + \
         failed_member + spread + mixed-attr-snippet + conflict-attr-snippet, and svelte_head \
         static_title/prop_title/state_title/title_meta/meta/body_sibling/html)"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_SPECIALS {
        assert!(seen.insert(slug), "duplicate supported-special slug {slug}");
    }
}

#[test]
fn supported_components_cover_the_full_component_corpus() {
    // The component / snippet / slot corpus is the structural oracle for the 5f-a vertical; a
    // dropped row is a coverage regression. This count gate fails LOUDLY if a row is dropped,
    // and the no-duplicate check guards against a typo. THREE `components/*` fixtures are
    // EXCLUDED — standalone_child (legacy-mode), snippet_capture_state (reactive-text
    // const-fold) and child_and_snippet (array-$state / $.proxy) — so the count is the 53
    // `components/*` fixtures minus 3.
    assert_eq!(
        SUPPORTED_COMPONENTS.len(),
        50,
        "the component corpus must enumerate the 50 runes-mode `components/*` 5f-a conformance \
         fixtures (Child / component_props / component_full / component_children_default / \
         component_snippet_children / component_bind_prop / component_bind_this / \
         component_bind_function / component_bind_function_multi / component_spread / \
         component_on_event / component_callback_prop / component_let / component_let_alias / \
         multi_component_import / snippet_multi_param / snippet_to_component / render_optional / \
         render_dynamic_ternary / render_dynamic_prop_arg / render_dynamic_optional_arg / \
         svelte_component / svelte_component_bind_this / svelte_component_import / svelte_self / \
         svelte_fragment / component_prop_hyphen / component_event_hyphen / fragment_slot_hyphen / \
         named_slot_span / named_slot_entity / fragment_slot_text_first / render_spread_arg / \
         render_paren_callee / render_optional_local / render_imported_member / \
         render_new_expression, plus the `slot=` disposition rows: Inner / \
         slot_filler_component_child / slot_filler_svelte_component_child / \
         slot_filler_svelte_self_child / slot_filler_svelte_element_child / \
         slot_prop_component_top_level / slot_prop_component_nested / \
         slot_prop_component_nested_in_component / \
         slot_prop_component_dynamic / slot_prop_svelte_component_top_level / \
         slot_prop_svelte_self_nondirect / slot_prop_component_in_slotted_fragment / \
         slot_prop_component_in_block), \
         excluding \
         the legacy-mode standalone_child (Block 5i) AND the two deferred-surface fixtures \
         snippet_capture_state (reactive-text const-fold) + child_and_snippet (array-$state / \
         $.proxy, Block 5g)"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_COMPONENTS {
        assert!(
            seen.insert(slug),
            "duplicate supported-component slug {slug}"
        );
    }
}

#[test]
fn supported_blocks_cover_the_full_block_corpus() {
    // The control-flow block corpus is the structural oracle for `{#if}`/`{#each}`/
    // `{#await}`/`{#key}`; a dropped row is a coverage regression. This count gate fails
    // LOUDLY if a row is dropped, and the no-duplicate check guards against a typo.
    assert_eq!(
        SUPPORTED_BLOCKS.len(),
        24,
        "the block corpus must enumerate all 24 `blocks/*` supported fixtures (if chain / \
         if single / each unkeyed / each index / each keyed-index / each else / each write / \
         each nested / await inline-then / await then+pending / await pending+catch / \
         await catch-only / key / if lone-text / if else lone-text / each lone-text / \
         each else lone-text / each lone-interp / each sibling-interp / key lone-text / \
         await lone-text / each mixed-text / each debug-text / if debug-text)"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_BLOCKS {
        assert!(seen.insert(slug), "duplicate supported-block slug {slug}");
    }
}

#[test]
fn supported_declaration_tags_cover_the_full_tag_corpus() {
    // The declaration/`{@const}`/`{@debug}` tag corpus is the structural oracle for the
    // 5e tag surface; a dropped row is a coverage regression. This count gate fails LOUDLY
    // if a row is dropped, and the no-duplicate check guards against a typo.
    assert_eq!(
        SUPPORTED_DECLARATION_TAGS.len(),
        10,
        "the declaration-tag corpus must enumerate all 10 `declaration_tags/*` supported \
         fixtures ({{@const}} in each / {{@const}} in if / inert decl / rune $state decl / \
         rune $derived decl / {{@debug}} multi / {{@debug}} after sibling / {{@debug}} in \
         element / {{@debug}} in if-body / {{@debug}}-only if-body)"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_DECLARATION_TAGS {
        assert!(
            seen.insert(slug),
            "duplicate supported-declaration-tag slug {slug}"
        );
    }
}

#[test]
fn supported_events_cover_the_full_event_corpus() {
    // The native-client event corpus is the structural oracle for the regular-element
    // event surface (non-delegated / capture / legacy modifiers / passive); a dropped
    // row is a coverage regression. This count gate fails LOUDLY if a row is dropped.
    assert_eq!(
        SUPPORTED_EVENTS.len(),
        19,
        "the event corpus must enumerate all 19 `events/*` supported fixtures"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_EVENTS {
        assert!(seen.insert(slug), "duplicate supported-event slug {slug}");
    }
}

#[test]
fn supported_options_cover_the_custom_element_corpus() {
    // The custom-element corpus is the structural oracle for the
    // `<svelte:options customElement>` accept surface (create/define topology,
    // conditional shadow/extend args, the fact-driven frame, the `$host()`
    // lowering); a dropped row is a coverage regression. Every
    // `options/custom_element_*` fixture must be enumerated (the remaining
    // `options/` fixture — `svelte_options_namespace` — is the fail-closed
    // options-axis vertical, deliberately NOT a supported row).
    assert_eq!(
        SUPPORTED_OPTIONS.len(),
        15,
        "the custom-element corpus must enumerate all 15 `options/custom_element_*` fixtures"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_OPTIONS {
        assert!(
            slug.starts_with("options/custom_element_"),
            "supported-options slug {slug} must be an options/custom_element_* fixture"
        );
        assert!(seen.insert(slug), "duplicate supported-options slug {slug}");
    }
    // Directory coverage: every `options/custom_element_*` fixture on disk is
    // enumerated — a new fixture cannot silently skip the gate.
    let fixtures =
        repo_root().join("crates/verter_compiler/tests/svelte_oracle_corpus/fixtures/options");
    for entry in std::fs::read_dir(&fixtures).expect("read options fixtures") {
        let name = entry.expect("dir entry").file_name();
        let name = name.to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".svelte") else {
            continue;
        };
        if !stem.starts_with("custom_element_") {
            continue;
        }
        let slug = format!("options/{stem}");
        assert!(
            seen.contains(slug.as_str()),
            "options fixture {slug} is not enumerated in SUPPORTED_OPTIONS"
        );
    }
}

#[test]
fn every_supported_fixture_emits_valid_js() {
    // GATE: every emitted SUPPORTED fixture module must be VALID JS (OXC-parses
    // clean). Catches a syntactically-broken emission (a stray `export` inside the
    // component fn, an unbalanced expression wrap) that the topology comparison
    // alone would not flag.
    for &slug in &all_supported_slugs() {
        let code = emit(slug);
        assert!(
            parses_as_js(&code),
            "emitted client module for {slug} must be valid JS:\n{code}"
        );
    }
}

#[test]
fn supported_matrix_enumerates_every_documented_sub_shape() {
    // The supported matrix is the positive half of the convergence gate; a
    // shrinking matrix is a coverage regression. This count gate fails LOUDLY if a
    // row is dropped.
    assert_eq!(
        SUPPORTED_MATRIX.len(),
        51,
        "the supported matrix must enumerate all 51 documented supported sub-shapes \
         (16 §1.2/rune/attr + the 11 5c DOM-hosted bind-family rows incl. radio group + \
         the 3 DOM bind TARGET-LVALUE rows: plain-local ident, plain-local member, and \
         the two-element function-pair `{{get, set}}` + the element `bind:this={{get, set}}` \
         function-pair row + the 2 F4 dynamic/mixed `bind:group` value rows: \
         `bind_group_radio_dynamic` and `bind_group_radio_mixed` + the `effect_arrow` \
         top-level `$effect(fn);` statement row + the 17 `imports/*` static script-import \
         prelude rows: bare/named/alias/namespace/side-effect/mixed/duplicate-unmerged/\
         multi-source-order/import-attributes/default-component-callee, the combined-\
         clause rows default_named_mixed/default_namespace, the empty-named side-effect \
         row empty_named_side_effect, the module-slot rows module_script_import_only/\
         module_and_instance_slot_order/module_namespace_member_frames, and the \
         member-of-import bind row bind_member_of_import)"
    );
    // No duplicate slugs across the matrix.
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_MATRIX {
        assert!(seen.insert(slug), "duplicate supported-matrix slug {slug}");
    }
}

#[test]
fn supported_attributes_cover_the_full_attribute_corpus() {
    // The native-client attribute corpus is the byte-precise oracle for the
    // dynamic-attribute / boolean-property / class-style-directive / autofocus
    // surface; a dropped row is a coverage regression. This count gate fails LOUDLY
    // if a row is dropped, and the no-duplicate check guards against a typo.
    assert_eq!(
        SUPPORTED_ATTRIBUTES.len(),
        31,
        "the attribute corpus must enumerate all 31 `attributes/*` fixtures"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_ATTRIBUTES {
        assert!(
            seen.insert(slug),
            "duplicate supported-attribute slug {slug}"
        );
    }
}

#[test]
fn emitted_client_topology_matches_official_goldens() {
    for &slug in &all_supported_slugs() {
        // The scope hash is masked to `svelte-<scoped>` on BOTH sides (the
        // golden pipeline masks it at generation; Verter's emitted module
        // carries the real hash) — the comparisons pin the scope-class
        // TOPOLOGY, and the hash VALUE is pinned by the css-field parity gate
        // (`css_client_artifact_matches_golden_and_native_server_fails_closed`).
        let code = mask_scope_hash(&emit(slug));
        let golden = client_golden(slug);

        // (1) The helper SEQUENCE (the load-bearing oracle: the helper families
        // and the order they are emitted).
        assert_eq!(
            helper_sequence(&code),
            golden_sequence(&golden),
            "helper sequence drift for {slug}:\n--- emitted ---\n{code}"
        );

        // (2) The import topology (disclose-version + the client namespace).
        assert!(
            emitted_imports_ok(&code, &golden),
            "import topology drift for {slug}:\n{code}"
        );

        // (3) The export-fn shape (name + params).
        assert_eq!(
            emitted_export(&code),
            golden_export(&golden),
            "export shape drift for {slug}:\n{code}"
        );

        // (4) The `from_html` template skeletons + fragment flags.
        assert_eq!(
            emitted_templates(&code),
            golden_templates(&golden),
            "template skeleton drift for {slug}:\n{code}"
        );

        // (5) The delegated event set (first-seen order).
        assert_eq!(
            emitted_delegated(&code),
            golden_delegated(&golden),
            "delegated-event drift for {slug}:\n{code}"
        );

        // (6) THE FULL-MODULE comparison — Verter's emitted module vs the official golden,
        // compared by PARSED STRUCTURAL EQUIVALENCE (the same AST-structural comparator the
        // codegen corpus uses): the argument/offset/identifier-precise oracle that catches a
        // `$$props.bar` vs `.foo`, a raw `count` vs `$.get(count)`, a dropped `$.child(_, true)`
        // arg, a sibling-offset drift, a sequence split into separate args, or significant
        // template TEXT — while WAIVING a behavior-preserving redundant paren the official
        // printer drops (the cosmetics-waived doctrine). The waiver is by parsed structure, NOT
        // a string scrub: a transparent `ParenthesizedExpression` wrapper is ignored, but a
        // SEMANTIC paren (`(a, b)` as one arg vs two, `(a ?? b).c` vs `a ?? b.c`) still parses
        // to a different tree and fails. This is what makes the unconditional concise-arrow-body
        // wrap (`() => (EXPR)`) — which over-wraps a non-object memoizer dep
        // cosmetically (`[() => (String(x))]` vs official `[() => String(x)]`) — invisible HERE
        // exactly as it is in the codegen corpus, with no shape predicate at the wrap site.
        assert_modules_structurally_equal(slug, &code, &golden_client_module(&golden));
    }
}

/// The 5c DOM-hosted bind slugs — the bindings-breadth additions whose emitted
/// constructs the D-17 verification below proves are FULLY structurally signed.
const BIND_5C_SLUGS: &[&str] = &[
    "matrix/bind_textarea_value",
    "matrix/bind_select_value",
    "matrix/bind_checked",
    "matrix/bind_media_currenttime",
    "matrix/bind_media_multi",
    "matrix/bind_dimension_multi",
    "matrix/bind_contenteditable_innerhtml",
    "matrix/bind_contenteditable_textcontent",
    "matrix/bind_contenteditable_innertext",
    "matrix/bind_property_open",
    "matrix/bind_group_radio",
    // F4 dynamic/mixed group value — the NEW constructs (the `var input_value;` no-init
    // declaration, the guarded `if (input_value !== (input_value = …)) { … }` IfStatement
    // inside the `$.template_effect` arrow body) sign through `decl_var_sig` (`Var(...)`) and
    // `stmt_sig`'s `If(...)` arm respectively — NOT a lossy `Decl(`/`Stmt(` fallback.
    "matrix/bind_group_radio_dynamic",
    "matrix/bind_group_radio_mixed",
];

#[test]
fn bind_5c_emit_hits_no_lossy_comparator_axis_d17() {
    // D-17 OBLIGATION VERIFICATION (with evidence from the ACTUAL 5c emit).
    //
    // D-17 requires: if any 5c-NEW emitted construct hits a DROPPED/LOSSY comparator
    // axis — an `Other(` (expr) / `Stmt(` (statement) / `Decl(` (declaration)
    // discriminant-only fallback — for a node that PARSES under `SourceType::mjs()`,
    // that axis must be encoded + discriminator-tested in the same change. The
    // fallbacks are correct ONLY for TS-only / JSX / V8-intrinsic forms that do NOT
    // parse under `SourceType::mjs()`.
    //
    // The DOM-hosted bind emit introduces exactly these NEW constructs beyond the base bind:value/bind:this emit:
    //   - chained member assignment `input.value = input.__value = 'X'`
    //     (`ExpressionStatement` → `AssignmentExpression` whose RHS is itself an
    //     `AssignmentExpression`, LHS a `StaticMemberExpression`);
    //   - `const binding_group = [];` (a `const` `VariableDeclaration` with an
    //     `ArrayExpression` init);
    //   - the `$.bind_select_value` / `$.bind_checked` / `$.bind_property` /
    //     `$.bind_content_editable` / `$.bind_element_size` / `$.bind_group` /
    //     `$.bind_played` / `$.remove_textarea_child` calls (`CallExpression`s with
    //     identifier / string-literal / arrow args).
    //
    // EVERY one of these signs through an EXISTING structural arm of `program_sig`
    // (`ExpressionStatement` → `expr_sig` → `AssignmentExpression`; `assignment_target_sig`
    // → `StaticMemberExpression`; `decl_var_sig` → `ArrayExpression`; `CallExpression` →
    // `expr_sig` over each arg). This test PROVES that empirically: it emits each 5c
    // bind slug, computes the AST-structural `module_sig` (the comparator's oracle
    // signature over `SourceType::mjs()`), and asserts NO lossy-fallback token appears.
    // It is the DISCRIMINATOR a future regression (a 5c construct collapsing to a
    // discriminant-only signature) would trip.
    for &slug in BIND_5C_SLUGS {
        let code = emit(slug);
        let sig = conformance_sig(&code, "emitted").module_sig;
        for token in ["Other(", "Stmt(", "Decl("] {
            assert!(
                !sig.contains(token),
                "5c bind emit for {slug} hit the LOSSY comparator axis `{token}` — a \
                 dropped structural axis (D-17): encode it + add a discriminator. \
                 module_sig:\n{sig}\n--- emitted ---\n{code}"
            );
        }
    }
}

#[test]
fn component_emit_hits_no_lossy_comparator_axis() {
    // The 5f-a component / snippet / slot / render / special emit introduces NEW
    // constructs beyond the element surface — a `Child($$anchor, {…})` call, getter/setter
    // prop members (`get value() {…}` / `set value($$value) {…}`), arrow slot callbacks
    // (`($$anchor, $$slotProps) => {…}`), `$.spread_props` / `$.snippet` / `$.component` /
    // `$.bind_this` calls, and the `let $0 = $.derived(…)` prop deriveds. This DISCRIMINATOR
    // proves emission is STRUCTURAL (every construct signs through a real `program_sig` arm),
    // never an opaque `Other(` (expr) / `Stmt(` (statement) / `Decl(` (declaration)
    // discriminant-only passthrough — the same D-17 obligation the 5c bind emit carries.
    for &slug in SUPPORTED_COMPONENTS {
        let code = emit(slug);
        let sig = conformance_sig(&code, "emitted").module_sig;
        for token in ["Other(", "Stmt(", "Decl("] {
            assert!(
                !sig.contains(token),
                "component emit for {slug} hit the LOSSY comparator axis `{token}` — a \
                 dropped structural axis: encode it + add a discriminator. \
                 module_sig:\n{sig}\n--- emitted ---\n{code}"
            );
        }
    }
}

#[test]
fn full_module_gate_discriminates_the_pre_fix_defects() {
    // DISCRIMINATION proof for the full-module gate: the normalized full-module
    // comparison REJECTS representative pre-fix output shapes for KEPT supported
    // fixtures, proving the gate is non-vacuous (it would have FAILED a defective
    // tree). For each, the defective normalized output differs from the committed
    // official golden; the current emitter matches (asserted by the main gate test).
    //
    // props alias: a defect reading `$$props.bar` (the alias) instead of the source
    // key `$$props.foo`.
    let alias_golden = golden_client_module(&client_golden("runes/props_alias"));
    let pre_fix_alias = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<p> </p>`);\n\
         export default function props_alias($$anchor, $$props) {\n\
           var p = root();\n\
           var text = $.child(p, true);\n\
           $.reset(p);\n\
           $.template_effect(() => $.set_text(text, $$props.bar));\n\
           $.append($$anchor, p);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_alias, alias_golden,
        "the gate MUST reject the alias-keyed `$$props.bar` output"
    );

    // is_text: a defect dropping the `true` arg on a pure-interp text child
    // (`$.child(p)` instead of `$.child(p, true)`).
    let is_text_golden = golden_client_module(&client_golden("runes/is_text_flag"));
    let pre_fix_is_text = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<p> </p> <button>x</button>`, 1);\n\
         export default function is_text_flag($$anchor) {\n\
           let count = $.state(0);\n\
           var fragment = root();\n\
           var p = $.first_child(fragment);\n\
           var text = $.child(p);\n\
           $.reset(p);\n\
           var button = $.sibling(p, 2);\n\
           $.template_effect(() => $.set_text(text, $.get(count)));\n\
           $.delegated('click', button, () => $.update(count));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_is_text, is_text_golden,
        "the gate MUST reject the `$.child(p)` (missing is_text) output"
    );

    // [P0] lone `<video>` (template flag `2` = USE_IMPORT_NODE, NOT a fragment): the
    // pre-fix defect treated ANY trailing `from_html` flag as a multi-root fragment,
    // so it cloned via `$.first_child(root())` — which is `null` for a single
    // element → a runtime `TypeError`. Official takes the single clone-root path
    // (`var video = root();`). The gate MUST reject the fragment-walk shape.
    let muted_golden = golden_client_module(&client_golden("attributes/muted_video"));
    let pre_fix_muted = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<video></video>`, 2);\n\
         export default function muted_video($$anchor) {\n\
           let muted = $.state(false);\n\
           var fragment = root();\n\
           var video = $.first_child(fragment);\n\
           $.template_effect(() => video.muted = $.get(muted));\n\
           $.delegated('click', video, () => $.set(muted, !$.get(muted)));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_muted, muted_golden,
        "the gate MUST reject the lone-`<video>` fragment-walk (`$.first_child`) output"
    );

    // Emission ORDER: the pre-fix defect emitted ALL element walk vars first, THEN
    // all the `let classes;` accumulator decls in one post-walk block. Official
    // INTERLEAVES each `let classes;` immediately after its element's walk var. The
    // gate MUST reject the non-interleaved order.
    let class_dirs_golden = golden_client_module(&client_golden("attributes/class_directives"));
    let pre_fix_class_dirs = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<button></button> <button></button>`, 1);\n\
         export default function class_directives($$anchor) {\n\
           let on = $.state(false);\n\
           let off = $.state(false);\n\
           let c = 'a';\n\
           var fragment = root();\n\
           var button = $.first_child(fragment);\n\
           var button_1 = $.sibling(button, 2);\n\
           let classes;\n\
           let classes_1;\n\
           $.template_effect(() => {\n\
             classes = $.set_class(button, 1, 'base', null, classes, { foo: $.get(on) });\n\
             classes_1 = $.set_class(button_1, 1, $.clsx(c), null, classes_1, { foo: $.get(on), bar: !$.get(off) });\n\
           });\n\
           $.delegated('click', button, () => $.set(on, !$.get(on)));\n\
           $.delegated('click', button_1, () => $.set(off, !$.get(off)));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_class_dirs, class_dirs_golden,
        "the gate MUST reject the non-interleaved (post-walk-block) `let classes;` order"
    );

    // Emission ORDER: the pre-fix defect emitted all walk vars (+ the `$.reset`) and
    // THEN the `$.autofocus(...)` inits in a post-walk block. Official emits each
    // `$.autofocus(node, …)` inline immediately after the node's walk var. The gate
    // MUST reject the post-walk autofocus block.
    let autofocus_golden = golden_client_module(&client_golden("attributes/autofocus"));
    let pre_fix_autofocus = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/> <input/> <button> </button>`, 1);\n\
         export default function autofocus($$anchor) {\n\
           let c = $.state(0);\n\
           let on = $.state(true);\n\
           var fragment = root();\n\
           var input = $.first_child(fragment);\n\
           var input_1 = $.sibling(input, 2);\n\
           var button = $.sibling(input_1, 2);\n\
           var text = $.child(button, true);\n\
           $.reset(button);\n\
           $.autofocus(input, true);\n\
           $.autofocus(input_1, $.get(on));\n\
           $.template_effect(() => $.set_text(text, $.get(c)));\n\
           $.delegated('click', input_1, () => $.set(on, !$.get(on)));\n\
           $.delegated('click', button, () => $.update(c));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_autofocus, autofocus_golden,
        "the gate MUST reject the post-walk `$.autofocus(...)` block order"
    );

    // ESCAPING — literal `${` in a mixed template: the pre-fix `escape_template_text`
    // left `${` UNescaped, producing `` `a${b${…}` `` (the literal `${b` opens a
    // bogus interpolation = invalid JS). Official escapes it to `\${`.
    let dollar_golden = golden_client_module(&client_golden("attributes/mixed_template_dollar"));
    let pre_fix_dollar = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/> <button>go</button>`, 1);\n\
         export default function mixed_template_dollar($$anchor) {\n\
           let v = $.state(0);\n\
           var fragment = root();\n\
           var input = $.first_child(fragment);\n\
           var button = $.sibling(input, 2);\n\
           $.template_effect(() => $.set_attribute(input, 'id', `a${b${$.get(v) ?? ''}`));\n\
           $.delegated('click', button, () => $.update(v));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_dollar, dollar_golden,
        "the gate MUST reject the unescaped dollar-brace template (escape_template_text)"
    );

    // ESCAPING — newline in a single-quoted static class base: the pre-fix
    // `js_single_quoted` escaped only `\\` and `'`, emitting a RAW newline inside
    // `'…'` (invalid JS). Official escapes it to `\\n`. The pre-fix shape carries a
    // real newline in the literal (preserved byte-exact by the normalizer), so it
    // differs from the `'a\\nb'` golden.
    let newline_golden = golden_client_module(&client_golden("attributes/static_class_newline"));
    let pre_fix_newline = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<div></div>`);\n\
         export default function static_class_newline($$anchor) {\n\
           let c = false;\n\
           var div = root();\n\
           $.set_class(div, 1, 'a\nb', null, {}, { x: c });\n\
           $.append($$anchor, div);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_newline, newline_golden,
        "the gate MUST reject a raw newline inside a single-quoted base (`js_single_quoted`)"
    );

    // MEMOIZATION GRANULARITY — a mixed class base with a call: the pre-fix path
    // memoized the WHOLE rendered template (`$0` as the value, the template in the
    // dep), instead of the EXPRESSION PART (the template in the body, the call in
    // the dep). Official: `($0) => $.set_class(div, 1, `a${$0 ?? ''}b`), [() =>
    // String($.get(c))]`.
    let mixed_call_golden = golden_client_module(&client_golden("attributes/mixed_class_call"));
    let pre_fix_mixed_call = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<div></div>`);\n\
         export default function mixed_class_call($$anchor) {\n\
           let c = $.state('x');\n\
           var div = root();\n\
           $.template_effect(($0) => $.set_class(div, 1, $0), [() => `a${String($.get(c)) ?? ''}b`]);\n\
           $.delegated('click', div, () => $.set(c, $.get(c) + '!'));\n\
           $.append($$anchor, div);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_mixed_call, mixed_call_golden,
        "the gate MUST reject whole-template memoization of a mixed class base"
    );

    // ORDERING — `bind:this` + an init-domain dynamic attr: the pre-fix order emitted
    // `$.bind_this(...)` BEFORE the `$.set_attribute(...)` init. Official emits the
    // init-domain write first, then `$.bind_this`.
    let bind_order_golden =
        golden_client_module(&client_golden("attributes/bind_this_dynamic_attr"));
    let pre_fix_bind_order = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/>`);\n\
         export default function bind_this_dynamic_attr($$anchor) {\n\
           let el;\n\
           let who = 'a';\n\
           var input = root();\n\
           $.bind_this(input, ($$value) => el = $$value, () => el);\n\
           $.set_attribute(input, 'id', who);\n\
           $.append($$anchor, input);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_bind_order, bind_order_golden,
        "the gate MUST reject `$.bind_this` emitted before the init-domain attr write"
    );

    // `has_call` REACTIVE TRIGGER — a demoted `$state` call-expr property: the pre-fix
    // gate (reactive iff `has_state`) emitted a one-shot `input.readOnly = Boolean(v)`
    // init. Official memoizes any `has_call` value into the effect:
    // `$.template_effect(($0) => input.readOnly = $0, [() => Boolean(v)])`.
    let demoted_golden =
        golden_client_module(&client_golden("attributes/call_expr_property_demoted"));
    let pre_fix_demoted = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/>`);\n\
         export default function call_expr_property_demoted($$anchor) {\n\
           let v = false;\n\
           var input = root();\n\
           input.readOnly = Boolean(v);\n\
           $.append($$anchor, input);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_demoted, demoted_golden,
        "the gate MUST reject the one-shot (non-memoized) demoted-$state call-expr init"
    );

    // `has_call` SOURCE ORDER — a PURE call BEFORE its first dependency: the pre-fix
    // `has_call` used a WHOLE-EXPRESSION "references any binding" precompute, so it saw
    // the (later) `flag` dependency and OVER-MEMOIZED the pure-call-before-dep value
    // into the deps-array effect form. Official accumulates `dependencies` in source
    // order and checks PER CALL against the deps-so-far: at the pure call, zero deps
    // have accumulated → NOT `has_call` → an INLINE `input.readOnly = … || flag` init.
    // The gate MUST reject the over-memoized `$.template_effect(($0) => …, [() => …])`
    // shape for the before-dep value.
    let before_golden = golden_client_module(&client_golden("attributes/pure_call_before_dep"));
    let pre_fix_before = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/>`);\n\
         export default function pure_call_before_dep($$anchor) {\n\
           let flag = false;\n\
           var input = root();\n\
           $.template_effect(($0) => input.readOnly = $0, [() => (globalThis?.check?.() ?? false) || flag]);\n\
           $.append($$anchor, input);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_before, before_golden,
        "the gate MUST reject over-memoizing a pure call that precedes its first dependency"
    );
    // And the AFTER-dep mirror is genuinely the memoized form (a confidence check that
    // the discrimination pair is not symmetric): the pre-fix INLINE shape for the
    // after-dep value differs from the memoized golden, so a regression that
    // UNDER-memoizes the after-dep case is also caught.
    let after_golden = golden_client_module(&client_golden("attributes/dep_before_pure_call"));
    let pre_fix_after_inline = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/>`);\n\
         export default function dep_before_pure_call($$anchor) {\n\
           let flag = false;\n\
           var input = root();\n\
           input.readOnly = flag || (globalThis?.check?.() ?? false);\n\
           $.append($$anchor, input);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_after_inline, after_golden,
        "the gate MUST reject under-memoizing a pure call that follows a dependency"
    );
}

#[test]
fn structural_full_module_gate_rejects_pre_fix_defects() {
    // The full-module gate now compares by PARSED STRUCTURAL EQUIVALENCE
    // (`assert_modules_structurally_equal`). This proves THAT comparator — not only the
    // legacy `normalize_module_for_comparison` string compare — REJECTS representative pre-fix
    // defect shapes against the official goldens (so the paren-waiver did not blind the gate to
    // any real divergence: identifier/member, dropped arg, statement order, memoization shape).
    let rejects_structurally = |golden_slug: &str, pre_fix: &str| {
        let golden = golden_client_module(&client_golden(golden_slug));
        assert!(
            !sigs_equal(pre_fix, &golden),
            "the STRUCTURAL gate MUST reject the pre-fix defect for {golden_slug}:\n{pre_fix}"
        );
    };
    // Wrong member (`$$props.bar` vs `.foo`).
    rejects_structurally(
        "runes/props_alias",
        "import 'svelte/internal/disclose-version'; import * as $ from 'svelte/internal/client'; \
         var root = $.from_html(`<p> </p>`); export default function props_alias($$anchor, $$props) { \
         var p = root(); var text = $.child(p, true); $.reset(p); \
         $.template_effect(() => $.set_text(text, $$props.bar)); $.append($$anchor, p); }",
    );
    // Dropped `true` arg on `$.child`.
    rejects_structurally(
        "runes/is_text_flag",
        "import 'svelte/internal/disclose-version'; import * as $ from 'svelte/internal/client'; \
         var root = $.from_html(`<p> </p> <button>x</button>`, 1); \
         export default function is_text_flag($$anchor) { let count = $.state(0); var fragment = root(); \
         var p = $.first_child(fragment); var text = $.child(p); $.reset(p); var button = $.sibling(p, 2); \
         $.template_effect(() => $.set_text(text, $.get(count))); \
         $.delegated('click', button, () => $.update(count)); $.append($$anchor, fragment); } $.delegate(['click']);",
    );
    // Statement ORDER (non-interleaved accumulator decls) — a structural statement-list diff.
    rejects_structurally(
        "attributes/class_directives",
        "import 'svelte/internal/disclose-version'; import * as $ from 'svelte/internal/client'; \
         var root = $.from_html(`<button></button> <button></button>`, 1); \
         export default function class_directives($$anchor) { let on = $.state(false); let off = $.state(false); \
         let c = 'a'; var fragment = root(); var button = $.first_child(fragment); var button_1 = $.sibling(button, 2); \
         let classes; let classes_1; $.template_effect(() => { \
         classes = $.set_class(button, 1, 'base', null, classes, { foo: $.get(on) }); \
         classes_1 = $.set_class(button_1, 1, $.clsx(c), null, classes_1, { foo: $.get(on), bar: !$.get(off) }); }); \
         $.delegated('click', button, () => $.set(on, !$.get(on))); \
         $.delegated('click', button_1, () => $.set(off, !$.get(off))); $.append($$anchor, fragment); } $.delegate(['click']);",
    );
    // Memoization GRANULARITY (whole-template vs expression-part) — a structural call-shape diff.
    rejects_structurally(
        "attributes/mixed_class_call",
        "import 'svelte/internal/disclose-version'; import * as $ from 'svelte/internal/client'; \
         var root = $.from_html(`<div></div>`); export default function mixed_class_call($$anchor) { \
         let c = $.state('x'); var div = root(); \
         $.template_effect(($0) => $.set_class(div, 1, $0), [() => `a${String($.get(c)) ?? ''}b`]); \
         $.delegated('click', div, () => $.set(c, $.get(c) + '!')); $.append($$anchor, div); } $.delegate(['click']);",
    );

    // INVALID-JS defects (an UNESCAPED `${` opening a bogus interpolation; a RAW newline inside a
    // single-quoted literal): the structural comparator's `conformance_sig` ASSERTS the side parses,
    // so an invalid-JS emission HARD-FAILS the gate. Here we prove the defect string is NOT valid JS
    // (the parse-assert would fire), i.e. the gate cannot silently pass it.
    assert!(
        !parses_as_js(
            "export default function f($$anchor) { let v = $.state(0); \
             $.set_attribute(input, 'id', `a${b${$.get(v) ?? ''}`); }"
        ),
        "an unescaped `${{` template defect must be INVALID JS (the structural gate parse-assert fires)"
    );
    assert!(
        !parses_as_js("export default function f() { $.set_class(div, 1, 'a\nb'); }"),
        "a raw-newline single-quoted defect must be INVALID JS (the structural gate parse-assert fires)"
    );
}

#[test]
fn structural_gate_discriminates_event_wrapper_order_and_passive_topology() {
    // The full-module structural comparator MUST discriminate the
    // wrapper NESTING ORDER + the passive/capture POSITIONAL args of an event
    // registration (a wrong wrapper order or a dropped/relocated passive is a
    // BEHAVIORAL bug, not a cosmetic diff). This proves the comparator (which signs every
    // call argument — nested calls, boolean literals, `void 0`) catches each, by
    // mutating the official golden into a deliberately-wrong variant and asserting the
    // STRUCTURAL signature no longer matches.
    let mutated_is_rejected = |slug: &str, find: &str, wrong: &str| {
        let golden = golden_client_module(&client_golden(slug));
        assert!(
            golden.contains(find),
            "precondition: the {slug} golden must contain `{find}`:\n{golden}"
        );
        let mutated = golden.replace(find, wrong);
        assert_ne!(
            mutated, golden,
            "precondition: the mutation must change the {slug} module"
        );
        assert!(
            !sigs_equal(&mutated, &golden),
            "the STRUCTURAL gate MUST reject the wrong event topology for {slug}:\n{mutated}"
        );
    };

    // (a) WRONG WRAPPER ORDER — swapping `preventDefault`/`stopPropagation` nesting
    // (the official inner→outer order is stopPropagation INNER, preventDefault OUTER).
    mutated_is_rejected(
        "events/modifier_stack",
        "$.preventDefault($.stopPropagation(() => $.update(count)))",
        "$.stopPropagation($.preventDefault(() => $.update(count)))",
    );

    // (b) DROPPED PASSIVE — removing the `void 0, true` 5th-positional passive arg.
    mutated_is_rejected(
        "events/modifier_passive",
        "$.event('click', button, () => $.update(count), void 0, true)",
        "$.event('click', button, () => $.update(count))",
    );

    // (c) WRONG PASSIVE BOOLEAN — `true` (passive) vs `false` (nonpassive).
    mutated_is_rejected(
        "events/modifier_passive",
        "() => $.update(count), void 0, true)",
        "() => $.update(count), void 0, false)",
    );

    // (d) WRONG CAPTURE SLOT — emitting `true` in the capture slot instead of the
    // `void 0` placeholder (a passive-only registration must keep the capture slot
    // `void 0`, NOT `true`).
    mutated_is_rejected(
        "events/modifier_passive",
        "() => $.update(count), void 0, true)",
        "() => $.update(count), true, true)",
    );

    // (e) DROPPED CAPTURE POSITIONAL — removing the 4th-positional `true` from a
    // capture event.
    mutated_is_rejected(
        "events/capture_suffix",
        "$.event('click', button, () => $.update(count), true)",
        "$.event('click', button, () => $.update(count))",
    );
}

#[test]
fn normalizer_preserves_whitespace_inside_literals() {
    // F12: the normalizer collapses cosmetic whitespace OUTSIDE literals but
    // PRESERVES it inside string / template-literal TEXT — so meaningful text
    // whitespace drift (a `Hello  world` vs `Hello world` in a template, or a
    // changed string literal) still fails. RED against the old normalizer that
    // stripped ALL whitespace including inside literals.
    let a = normalize_module_for_comparison("var x =  `Hello  ${y}  world`;");
    let b = normalize_module_for_comparison("var x = `Hello ${y} world`;");
    assert_ne!(
        a, b,
        "whitespace INSIDE a template literal must be preserved (not collapsed)"
    );
    // Outer whitespace RUNS are collapsed to a single space, so two modules
    // differing ONLY in outer whitespace-run length (tabs / newlines / multiple
    // spaces vs one) normalize equal.
    let c = normalize_module_for_comparison("var x  =\n\t`t`;");
    let d = normalize_module_for_comparison("var x = `t`;");
    assert_eq!(c, d, "cosmetic whitespace OUTSIDE literals is collapsed");
    // A string literal's interior whitespace is preserved.
    assert_ne!(
        normalize_module_for_comparison("var s = 'a  b';"),
        normalize_module_for_comparison("var s = 'a b';"),
        "whitespace inside a string literal must be preserved"
    );
}

#[test]
fn helper_sequence_masking_ignores_helper_shaped_strings() {
    // DISCRIMINATING self-test for the masker: a `$.foo` token inside a STRING or
    // a template-literal TEXT span is NOT a helper reference. (Guards the gate
    // against a naive regex that would mis-count.)
    let code = "var x = '$.fake'; var y = `text $.alsofake ${$.real(1)}`; $.outer();";
    let seq = helper_sequence(code);
    assert_eq!(seq, vec!["real".to_string(), "outer".to_string()]);
    assert!(!seq.iter().any(|h| h == "fake" || h == "alsofake"));
}

// ===========================================================================
// The SYSTEMATIC CODEGEN CORPUS — the native-client codegen corpus
// (`scripts/gen-svelte-codegen-corpus.mjs`).
//
// The generator mechanically enumerates the native-client codegen surface over three
// orthogonal axes — value-expression SHAPE × TARGET × REACTIVITY — and pins the
// OFFICIAL pinned-`svelte@5.56.3` module of every cell as the golden (under the
// `codegen/` subtree: `<slug>.svelte` + `<slug>.client.json` together). This gate
// recompiles every cell with Verter, normalizes its emitted module the SAME way, and
// asserts byte-equality (the argument/offset/identifier-precise oracle), plus the
// helper-topology fields. A COVERAGE gate (reading the manifest) asserts every
// required value-shape / target / reactivity axis contributes ≥1 committed row, so a
// dropped enumerator fails HARD. This is the convergence tool: the per-edge byte tail
// is closed by a systematic enumeration, not whack-a-mole.
// ===========================================================================

/// The `codegen/` corpus directory (fixtures + goldens together).
fn codegen_dir() -> PathBuf {
    repo_root().join("crates/verter_compiler/tests/svelte_oracle_corpus/codegen")
}

/// Every BYTE-MATCH codegen cell slug (a `<slug>.client.json` golden under `codegen/`
/// whose Verter output must equal official's byte-for-byte). A `refuse` cell has NO
/// `.client.json` (official compile-fails it) and a `live-fallback` cell has NO
/// `.client.json` (Verter emits the LIVE form, not official's folded literal — and that
/// literal can be a lone surrogate a strict JSON reader rejects), so neither appears here.
/// Sorted lexicographically; `manifest.json` is excluded.
fn codegen_slugs() -> Vec<String> {
    let dir = codegen_dir();
    let mut slugs: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read codegen dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".client.json").map(|s| s.to_string())
        })
        .collect();
    slugs.sort();
    slugs
}

/// Every `refuse`-bucket cell slug (a `<slug>.refuse.json` marker — an official-rejected
/// const-fold throw Verter must ALSO refuse). Sorted.
fn codegen_refuse_slugs() -> Vec<String> {
    let dir = codegen_dir();
    let mut slugs: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read codegen dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".refuse.json").map(|s| s.to_string())
        })
        .collect();
    slugs.sort();
    slugs
}

/// Every `live-fallback`-bucket cell slug (a `<slug>.live.json` marker — official folds but
/// Verter emits the LIVE form). Sorted.
fn codegen_live_fallback_slugs() -> Vec<String> {
    let dir = codegen_dir();
    let mut slugs: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read codegen dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".live.json").map(|s| s.to_string())
        })
        .collect();
    slugs.sort();
    slugs
}

/// The `.refuse.json` / `.live.json` record for a bucket cell slug.
fn codegen_bucket_record(slug: &str, ext: &str) -> serde_json::Value {
    let path = codegen_dir().join(format!("{slug}.{ext}"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read codegen {ext} record {slug}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse codegen {ext} record {slug}: {e}"))
}

/// The fixture source for a codegen cell slug.
fn codegen_fixture_source(slug: &str) -> String {
    let path = codegen_dir().join(format!("{slug}.svelte"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read codegen fixture {slug}: {e}"))
}

/// The committed official client golden for a codegen cell slug.
fn codegen_golden(slug: &str) -> serde_json::Value {
    let path = codegen_dir().join(format!("{slug}.client.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read codegen golden {slug}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse codegen golden {slug}: {e}"))
}

/// Compile a codegen cell to its emitted client JS (the `codegen/` fixtures live in
/// their own subtree, so this reuses the shared `component_name_for` rule but reads
/// from `codegen_dir`).
fn codegen_emit(slug: &str) -> String {
    codegen_try_emit(slug)
        .unwrap_or_else(|e| panic!("codegen client emission failed for {slug}: {e:?}"))
}

/// The FALLIBLE Verter compile of a codegen cell — `Ok(code)` when Verter emits a module,
/// `Err(ClientCompileError)` when it refuses (the refuse-bucket gate asserts the `Err`
/// carries the `const-fold-throw` diagnostic).
fn codegen_try_emit(slug: &str) -> Result<String, ClientCompileError> {
    let source = codegen_fixture_source(slug);
    let alloc = Allocator::default();
    let parsed = parse_svelte(&source);
    let opts = SvelteRuntimeOptions {
        filename: Some(format!("{slug}.svelte")),
        name: Some(component_name_for(slug)),
        ..Default::default()
    };
    compile_client(&source, &parsed, &opts, &alloc, false, false).map(|m| m.code)
}

/// The codegen corpus manifest (the coverage authority).
fn codegen_manifest() -> serde_json::Value {
    let path = codegen_dir().join("manifest.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("read codegen manifest: {e}\n(run scripts/gen-svelte-codegen-corpus.mjs)")
    });
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse codegen manifest: {e}"))
}

#[test]
fn codegen_corpus_is_nonempty_and_every_cell_emits_valid_js() {
    let slugs = codegen_slugs();
    assert!(
        slugs.len() >= 200,
        "the codegen corpus must be substantial (got {} cells); regenerate with \
         scripts/gen-svelte-codegen-corpus.mjs",
        slugs.len()
    );
    // Every emitted codegen cell must be VALID JS (OXC-parses clean) — catches a
    // syntactically-broken emission the topology comparison alone would not flag.
    for slug in &slugs {
        let code = codegen_emit(slug);
        assert!(
            parses_as_js(&code),
            "emitted codegen module for {slug} must be valid JS:\n{code}"
        );
    }
}

#[test]
fn codegen_corpus_covers_every_required_axis() {
    // COVERAGE GATE: the manifest declares the required value-shape / target /
    // reactivity axes; every one must contribute ≥1 committed cell. A dropped
    // enumerator (the corpus silently losing a finite axis) fails HARD here, mirroring
    // the JS generator's own coverage check. This is what makes the corpus a REAL gate
    // rather than a hand-curated sample.
    let manifest = codegen_manifest();
    let required = |key: &str| -> Vec<String> {
        manifest[key]
            .as_array()
            .unwrap_or_else(|| panic!("manifest missing {key}"))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    let counts = |key: &str| -> serde_json::Map<String, serde_json::Value> {
        manifest[key]
            .as_object()
            .unwrap_or_else(|| panic!("manifest missing {key}"))
            .clone()
    };

    // (The const-fold tri-state buckets — `fold-exact` / `refuse` / `live-fallback` — are
    // covered by `const_fold_buckets_cover_every_required_family_and_eagerness`.)
    for (req_key, count_key, label) in [
        ("required_shape_axes", "shape_counts", "value-shape"),
        ("required_target_axes", "target_counts", "target"),
        (
            "required_reactivity_axes",
            "reactivity_counts",
            "reactivity",
        ),
        ("required_content_axes", "content_counts", "content"),
        ("required_container_axes", "container_counts", "container"),
        // The element-spread fold / `{@html}` anchor + payload / compose axes.
        (
            "required_spread_fold_axes",
            "spread_fold_counts",
            "spread-fold",
        ),
        (
            "required_html_anchor_axes",
            "html_anchor_counts",
            "html-anchor",
        ),
        (
            "required_html_payload_axes",
            "html_payload_counts",
            "html-payload",
        ),
        ("required_compose_axes", "compose_counts", "compose"),
        (
            "required_directive_text_axes",
            "directive_text_counts",
            "directive-text",
        ),
        (
            "required_class_value_paren_axes",
            "class_value_paren_counts",
            "class-value-paren",
        ),
        // The `$.template_effect` MEMOIZER-DEPS axis — the second concise-arrow-from-payload
        // embedding surface (an object dep `() => ({ … })` from a call-bearing directive).
        ("required_memo_deps_axes", "memo_deps_counts", "memo-deps"),
    ] {
        let req = required(req_key);
        let cnts = counts(count_key);
        for axis in &req {
            let n = cnts.get(axis).and_then(|v| v.as_u64()).unwrap_or(0);
            assert!(
                n >= 1,
                "codegen corpus is missing the {label} axis `{axis}` (0 committed cells); \
                 regenerate with scripts/gen-svelte-codegen-corpus.mjs"
            );
        }
    }

    // The committed manifest total matches the on-disk cell count across ALL buckets
    // (byte-match `fold-exact`/PASS1/PASS2 cells + `refuse` markers + `live-fallback` markers)
    // — no orphan / missing cell the discovery walk would silently include or exclude.
    // (`codegen_slugs()` already excludes the marker-only cells, which are counted separately.)
    let total = manifest["total"].as_u64().unwrap() as usize;
    let on_disk =
        codegen_slugs().len() + codegen_refuse_slugs().len() + codegen_live_fallback_slugs().len();
    assert_eq!(
        total,
        on_disk,
        "manifest `total` must equal the committed cell count across all buckets \
         (byte-match {} + refuse {} + live-fallback {})",
        codegen_slugs().len(),
        codegen_refuse_slugs().len(),
        codegen_live_fallback_slugs().len(),
    );

    // HARDCODED axis anchor: the manifest's `required_*` lists are DERIVED from the
    // generator's `SHAPES`/`TARGETS`/`REACTIVITIES`, so dropping an enumerator there also
    // shrinks the `required_*` list — which the ≥1-row loop above would then vacuously
    // satisfy. Pinning the full codegen axis vocabulary HERE (independent of the
    // manifest) makes a generator-side axis drop fail the Rust gate, not only the JS
    // generator's own check. These are the architect-specified native-client codegen axes.
    let shapes: std::collections::BTreeSet<String> =
        required("required_shape_axes").into_iter().collect();
    for shape in [
        "literal",
        "binary",
        "template",
        "member",
        "call_pure",
        "call_impure",
        "optional_call",
        "optional_member",
        "conditional",
        "logical_and",
        "logical_or",
        "logical_nullish",
        "sequence",
        "object",
        "array",
        "call_arg_spread",
        "array_spread",
        "object_spread",
        "new",
        "tagged_template",
    ] {
        assert!(
            shapes.contains(shape),
            "the codegen corpus must declare the `{shape}` value-shape axis \
             (a dropped generator enumerator)"
        );
    }
    let targets: std::collections::BTreeSet<String> =
        required("required_target_axes").into_iter().collect();
    for target in [
        "attr",
        "boolean",
        "class",
        "class_directive",
        "style",
        "style_directive",
    ] {
        assert!(
            targets.contains(target),
            "the codegen corpus must declare the `{target}` target axis"
        );
    }
    let reactivities: std::collections::BTreeSet<String> =
        required("required_reactivity_axes").into_iter().collect();
    for react in ["state", "props", "demoted", "pure"] {
        assert!(
            reactivities.contains(react),
            "the codegen corpus must declare the `{react}` reactivity axis"
        );
    }

    // The CONTENT sub-axis (the inner value-shape a container's hole holds) and the
    // CONTAINER axis (the value-form with a content hole) — the fix-#6 nesting class.
    // Pinned HERE (independent of the manifest) so a generator-side enumerator drop fails
    // the Rust gate, not only the JS generator's own check.
    let content_axes: std::collections::BTreeSet<String> =
        required("required_content_axes").into_iter().collect();
    for content in [
        "identifier",
        "binary",
        "logical_and",
        "logical_or",
        "logical_nullish",
        "conditional",
        "unary",
        "sequence",
        "member",
        "call",
    ] {
        assert!(
            content_axes.contains(content),
            "the codegen corpus must declare the `{content}` content sub-axis"
        );
    }
    let container_axes: std::collections::BTreeSet<String> =
        required("required_container_axes").into_iter().collect();
    for container in ["tmpl", "cond", "log", "call_arg"] {
        assert!(
            container_axes.contains(container),
            "the codegen corpus must declare the `{container}` container axis"
        );
    }

    // The ELEMENT-SPREAD fold axis (the `$.attribute_effect` fold composition + payload
    // kind + element kind). Pinned HERE (independent of the manifest) so a generator-side
    // enumerator drop fails the Rust gate, not only the JS check.
    let spread_fold_axes: std::collections::BTreeSet<String> =
        required("required_spread_fold_axes").into_iter().collect();
    for axis in [
        "alone",
        "static_before",
        "static_after",
        "static_around",
        "dynamic_before",
        "dynamic_after",
        "mixed_before",
        "mixed_after",
        "classdir_shorthand",
        "classdir_cond",
        "classdir_before",
        "styledir_expr",
        "styledir_important_expr",
        // A STATIC-TEXT style directive value folds as the quoted string (the SOLE directive
        // family that accepts a text value).
        "styledir_text",
        "class_attr_static",
        "class_attr_dyn",
        "style_attr_static",
        // A VALUELESS boolean attribute folds as the raw `true` (NOT the empty-string `''`).
        "valueless",
        "valueless_input",
        "class_attr_and_dir",
        "multi_classdir",
        "multi_styledir",
        "both_dirs",
        "two_spreads",
        "three_spreads",
        "spread_attr_spread",
        "spread_dyn_spread",
        "payload_member",
        "payload_call",
        "payload_optional_call",
        "payload_conditional",
        "payload_logical",
        // A SequenceExpression payload KEEPS its parens; a colliding payload identifier
        // renames the DOM var — both the paren-preservation / collision boundaries.
        "payload_sequence",
        "payload_object_literal",
        "payload_props",
        "payload_collision_p",
        // Author transparent parens around a fold value drop (`id={(a ? b : c)}` →
        // `id: a ? b : c`); a mixed text+interpolation style directive under a spread folds
        // the template literal.
        "payload_dyn_paren",
        "payload_class_dir_paren",
        "payload_style_dir_paren",
        "payload_style_mixed",
        // An authored `defaultValue` / `defaultChecked` reset attr on an `<input>` spread
        // SUPPRESSES the 7-argument tail (camelCase, case-sensitive); a lowercase
        // `defaultvalue` KEEPS it.
        "input_default_value",
        "input_default_checked",
        "input_default_value_dyn",
        "input_lc_defaultvalue",
        // A ROOT paren around a member-spread payload (`{...(obj.attrs)}`) peels cleanly
        // and is accepted (the member spread is emitted unchanged, not failed closed).
        "payload_paren_member_spread",
        "element_a",
        "element_button",
        "element_h1",
        "element_input",
        "element_p",
    ] {
        assert!(
            spread_fold_axes.contains(axis),
            "the codegen corpus must declare the `{axis}` element-spread fold axis"
        );
    }

    // The `{@html}` ANCHOR-topology axis (only-child / sibling / root / nested / interleave).
    let html_anchor_axes: std::collections::BTreeSet<String> =
        required("required_html_anchor_axes").into_iter().collect();
    for axis in [
        "only_child",
        "sibling_text_before",
        "sibling_text_after",
        "sibling_text_both",
        "two_adjacent",
        "nested_in_element",
        "root",
        "root_with_sibling",
        "two_root",
    ] {
        assert!(
            html_anchor_axes.contains(axis),
            "the codegen corpus must declare the `{axis}` {{@html}} anchor axis"
        );
    }

    // The `{@html}` PAYLOAD-kind axis (the thunk + the direct-identifier-call elision).
    let html_payload_axes: std::collections::BTreeSet<String> =
        required("required_html_payload_axes").into_iter().collect();
    for axis in [
        "static_string",
        "identifier",
        "member",
        "call_elision",
        // A prop callee thunks the rewritten member; args / optional callees do NOT elide.
        "call_prop_thunk",
        "call_with_args",
        "call_optional",
        "member_call",
        // A PAREN-WRAPPED direct call STILL elides (the callee parens are peeled); a
        // paren-wrapped PROP callee thunks the rewritten callee call (author parens dropped).
        "call_paren",
        "call_doubleparen",
        "call_paren_prop",
        "conditional",
        "template",
        // Author transparent parens around the payload drop (`{@html (c ? a : b)}` →
        // `() => c ? a : b`); a bare sequence KEEPS one paren pair.
        "paren_conditional",
        "paren_member",
        "bare_sequence",
        // An OBJECT-LITERAL payload wraps the concise-arrow body so a leading `{` is an object
        // expression, not a block body returning `undefined` — including a member / computed
        // index / method call whose leftmost leaf is the object, and an author-parenthesized
        // object (which keeps its own single pair, no double wrap).
        "object_literal",
        "member_of_object_literal",
        "index_of_object_literal",
        "call_on_object_literal",
        "paren_object",
        // An OPTIONAL-CHAIN access on an object literal — OXC wraps it in a `ChainExpression`,
        // but the chain's leftmost leaf is still the object, so the whole body wraps; without it
        // the body is non-parsing JS (a block followed by a stray `?.`).
        "opt_member_of_object_literal",
        "opt_index_of_object_literal",
        "opt_call_on_object_literal",
        "opt_callee_of_object_literal",
        // A MULTI-MEMBER optional chain whose TOP member is also optional — locks the
        // optional-flag discrimination on the OUTER chain element (`?.o.p` vs `?.o?.p`).
        "chain_top_optional",
        // The systematic object-leading × control axis for the UNCONDITIONAL concise-arrow-body
        // wrap. An object-LEFT logical (`{a:1} || b`) and a tagged template whose tag
        // leftmost leaf is an object member (`` {f}.f`tpl` ``) DISCRIMINATE "revert the wrap"
        // (without the outer paren the body parses `{` as a block). The non-object controls
        // (array / arrow / unary / new) are no-spurious-wrap anchors. (An object-LEFT binary
        // `{a:1} + 2` is absent: official itself emits NON-PARSING JS there, so there is no
        // parseable golden — the TS-skin and binary cases are covered by the Verter-only unit
        // test `html_ts_wrapper_object_payload_*` instead.)
        "object_left_logical",
        "tagged_template_object",
        "array_payload",
        "arrow_payload",
        "unary_payload",
        "new_payload",
    ] {
        assert!(
            html_payload_axes.contains(axis),
            "the codegen corpus must declare the `{axis}` {{@html}} payload axis"
        );
    }

    // The directive STANDALONE axis (a `class:` / `style:` directive on a NON-spread
    // element → the coalesced `$.set_class` / `$.set_style`): the static-text style value
    // family AND the valueless-base `class` / `style` family (the raw `true` base). Pinned
    // HERE (independent of the manifest) so a generator-side enumerator drop fails the Rust
    // gate, not only the JS check.
    let directive_text_axes: std::collections::BTreeSet<String> =
        required("required_directive_text_axes")
            .into_iter()
            .collect();
    for axis in [
        "style_text",
        "style_text_important",
        "style_text_hyphen",
        "class_valueless_base",
        "style_valueless_base",
        // A MIXED text+interpolation `style:` directive folds the reactive template literal
        // (`|important` uses the array form); author transparent parens around a standalone
        // style-directive value drop.
        "style_mixed_live",
        "style_mixed_important",
        "style_dir_paren",
    ] {
        assert!(
            directive_text_axes.contains(axis),
            "the codegen corpus must declare the `{axis}` directive standalone axis"
        );
    }

    // The COMPOSE axis (spread + `{@html}` on the same element).
    let compose_axes: std::collections::BTreeSet<String> =
        required("required_compose_axes").into_iter().collect();
    for axis in ["spread_html_static", "spread_html_reactive"] {
        assert!(
            compose_axes.contains(axis),
            "the codegen corpus must declare the `{axis}` compose axis"
        );
    }

    // The standalone `class={…}` clsx-decision axis (a PAREN-WRAPPED class value — the
    // clsx decision is computed on the UNWRAPPED root kind). Pinned HERE (independent of
    // the manifest) so a generator-side enumerator drop fails the Rust gate, not only the
    // JS check.
    let class_value_paren_axes: std::collections::BTreeSet<String> =
        required("required_class_value_paren_axes")
            .into_iter()
            .collect();
    for axis in [
        // A parenthesized literal / binary / template stays NO-clsx; a parenthesized
        // conditional DOES clsx (the clsx-YES boundary).
        "class_paren_literal",
        "class_paren_binary",
        "class_paren_template",
        "class_paren_conditional",
    ] {
        assert!(
            class_value_paren_axes.contains(axis),
            "the codegen corpus must declare the `{axis}` class-value-paren axis"
        );
    }

    // The `$.template_effect` MEMOIZER-DEPS axis — the SECOND concise-arrow-from-payload
    // embedding surface (a call-bearing reactive `class:`/`style:` directive memoizes its
    // directives OBJECT into a deps-array slot `[() => ({ … })]`; the same unconditional
    // concise-arrow-body wrap keeps that object dep an expression). Pinned HERE (independent of
    // the manifest) so a generator-side enumerator drop fails the Rust gate, not only the JS
    // check — the regression anchor for the memoizer-site under-wrap class.
    let memo_deps_axes: std::collections::BTreeSet<String> =
        required("required_memo_deps_axes").into_iter().collect();
    for axis in ["class_dir_object_call", "style_dir_object_call"] {
        assert!(
            memo_deps_axes.contains(axis),
            "the codegen corpus must declare the `{axis}` memo-deps axis"
        );
    }

    // The `fold-exact` bucket families (the EXACT-value rows pinning official's
    // `scope.evaluate` coercion / globals / operator semantics that Verter folds
    // byte-identically). Pinned HERE so a generator-side enumerator drop fails the Rust
    // gate, not only the JS check. (The `refuse` / `live-fallback` bucket families are
    // pinned in `const_fold_buckets_cover_every_required_family_and_eagerness`.)
    let fold_exact_families: std::collections::BTreeSet<String> =
        required("required_fold_exact_families")
            .into_iter()
            .collect();
    for family in [
        "bigint",
        "number_coerce",
        "string_coerce",
        "global_call",
        "global_const",
        "tricky_number",
    ] {
        assert!(
            fold_exact_families.contains(family),
            "the fold-exact bucket must declare the `{family}` const-fold edge family \
             (a dropped generator enumerator)"
        );
    }

    // THE DOCTRINE: every container × content combination MUST have a committed cell (a
    // missing container×content cell is a generator bug). Each such cell's slug is
    // `<container>_<content>[_multi]__<target>__<reactivity>`, so a `<container>_<content>__`
    // prefix proves the combination exists.
    let slugs = codegen_slugs();
    for container in &container_axes {
        for content in &content_axes {
            let prefix = format!("{container}_{content}__");
            let multi_prefix = format!("{container}_{content}_multi__");
            assert!(
                slugs
                    .iter()
                    .any(|s| s.starts_with(&prefix) || s.starts_with(&multi_prefix)),
                "the codegen corpus is missing the container×content cell `{container}×{content}` \
                 (no `{prefix}…` slug); regenerate with scripts/gen-svelte-codegen-corpus.mjs"
            );
        }
    }
}

#[test]
fn emitted_codegen_corpus_matches_official_goldens() {
    // THE convergence gate: for every systematic codegen cell, Verter's normalized
    // emitted module must equal the OFFICIAL normalized golden byte-for-byte (modulo
    // the cosmetic-whitespace collapse), plus the helper-topology fields. A divergence
    // in helper choice, memoization shape, effect/thunk structure, dependency tracking,
    // prop/property routing, or class/style normalization fails here.
    for slug in &codegen_slugs() {
        let code = codegen_emit(slug);
        let golden = codegen_golden(slug);

        // (1) The helper SEQUENCE (families + emission order).
        assert_eq!(
            helper_sequence(&code),
            golden_sequence(&golden),
            "helper sequence drift for codegen cell {slug}:\n--- emitted ---\n{code}"
        );
        // (2) The import topology.
        assert!(
            emitted_imports_ok(&code, &golden),
            "import topology drift for codegen cell {slug}:\n{code}"
        );
        // (3) The export-fn shape.
        assert_eq!(
            emitted_export(&code),
            golden_export(&golden),
            "export shape drift for codegen cell {slug}:\n{code}"
        );
        // (4) The `from_html` template skeletons + fragment flags.
        assert_eq!(
            emitted_templates(&code),
            golden_templates(&golden),
            "template skeleton drift for codegen cell {slug}:\n{code}"
        );
        // (5) The delegated event set.
        assert_eq!(
            emitted_delegated(&code),
            golden_delegated(&golden),
            "delegated-event drift for codegen cell {slug}:\n{code}"
        );
        // (6) THE FULL-MODULE STRUCTURAL comparison (paren-insensitive, but argument /
        // identifier / operator / literal-content / structure-precise). The value emitter is
        // source-preserving, so a behavior-preserving redundant author paren the official
        // printer drops (`() => ((a, b))` vs `() => (a, b)`, `id: (c ? a : b)` vs
        // `id: c ? a : b`) is WAIVED — but every behavioral / structural divergence still fails.
        assert_modules_structurally_equal(slug, &code, &golden_client_module(&golden));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The const-fold TRI-STATE contract — the three corpus buckets.
//
// `fold-exact` cells are byte-compared in `emitted_codegen_corpus_matches_official_goldens`
// (they carry a `.client.json` golden Verter must match). The `refuse` and `live-fallback`
// buckets need DISTINCT gates: a refuse cell has NO official output (official compile-fails)
// and Verter must REFUSE; a live-fallback cell has an official FOLDED golden but Verter
// emits the LIVE form (deliberately NOT byte-equal).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn refuse_bucket_cells_are_refused_with_const_fold_throw() {
    // Each `refuse`-bucket cell is an official-rejected const-fold throw (the generator
    // confirmed pinned svelte COMPILE-FAILS it). Verter must ALSO refuse — deterministically,
    // with the `const-fold-throw` diagnostic — never emit live code (which would turn the
    // compile-failure into a runtime crash).
    let slugs = codegen_refuse_slugs();
    assert!(
        slugs.len() >= 10,
        "the refuse bucket must be substantial (got {} cells); regenerate with \
         scripts/gen-svelte-codegen-corpus.mjs",
        slugs.len()
    );
    for slug in &slugs {
        // The generator's `.refuse.json` records official's compile-failure.
        let record = codegen_bucket_record(slug, "refuse.json");
        assert_eq!(
            record["officialRejected"].as_bool(),
            Some(true),
            "refuse cell {slug} must record official rejection"
        );
        // Verter must REFUSE.
        match codegen_try_emit(slug) {
            Err(ClientCompileError::Unsupported(surface)) => {
                assert!(
                    matches!(surface, UnsupportedSvelteRuntimeSurface::ConstFoldThrow { .. }),
                    "refuse cell {slug} must refuse with ConstFoldThrow, got {surface:?}"
                );
                assert_eq!(
                    surface.diagnostic_code(),
                    "svelte-runtime-unsupported-const-fold-throw",
                    "refuse cell {slug} must carry the const-fold-throw diagnostic code"
                );
            }
            Ok(code) => panic!(
                "refuse cell {slug} was EMITTED by Verter (official compile-FAILS it — emitting \
                 live code turns a compile error into a runtime crash):\n{code}"
            ),
            Err(other) => panic!(
                "refuse cell {slug} must refuse with ConstFoldThrow, got a different error: {other:?}"
            ),
        }
    }
}

#[test]
fn live_fallback_bucket_cells_emit_live_not_the_folded_literal() {
    // Each `live-fallback`-bucket cell folds in official, but Verter cannot prove byte-exact
    // emission so it emits the LIVE expression. The gate asserts: (a) the ledger reason is a
    // checked-in `live_fallback_ledger()` label, (b) official FOLDED the chunk (recorded —
    // no `${` interpolation in official's module), (c) Verter EMITS (a non-throwing value —
    // not a refusal), (d) Verter's output is valid JS, and (e) Verter's output is the LIVE
    // form (a `${…}` interpolation a folded literal never has) — the structural proof that
    // Verter did NOT fold the not-byte-exact value. (No byte comparison against official's
    // folded literal: Verter deliberately differs, and the literal can be a lone surrogate a
    // strict JSON reader rejects.)
    let slugs = codegen_live_fallback_slugs();
    assert!(
        slugs.len() >= 10,
        "the live-fallback bucket must be substantial (got {} cells); regenerate with \
         scripts/gen-svelte-codegen-corpus.mjs",
        slugs.len()
    );
    let ledger_labels: std::collections::BTreeSet<String> = live_fallback_ledger()
        .into_iter()
        .map(|row| row.label.to_string())
        .collect();
    for slug in &slugs {
        let record = codegen_bucket_record(slug, "live.json");
        // (a) The reason is a checked-in ledger label.
        let reason = record["reason"]
            .as_str()
            .unwrap_or_else(|| panic!("live cell {slug} must record a ledger reason"));
        assert!(
            ledger_labels.contains(reason),
            "live cell {slug} reason `{reason}` must be a checked-in live_fallback_ledger() \
             label (got ledger {ledger_labels:?})"
        );
        // (b) Official FOLDED the chunk (its module inlined the value, no `${` for it).
        assert_eq!(
            record["officialModuleHasInterpolation"].as_bool(),
            Some(false),
            "live cell {slug}: official must FOLD the chunk (no live `${{` in its module) — \
             the contrast the bucket documents"
        );
        // (c) Verter EMITS (a live-fallback is a non-throwing value — never a refusal).
        let code = match codegen_try_emit(slug) {
            Ok(code) => code,
            Err(e) => panic!(
                "live-fallback cell {slug} must EMIT the live form (official folds a \
                 non-throwing value), but Verter refused: {e:?}"
            ),
        };
        // (d) Valid JS.
        assert!(
            parses_as_js(&code),
            "live-fallback cell {slug} must emit valid JS:\n{code}"
        );
        // (e) The emitted module must contain a LIVE template interpolation (a `${…}` over
        // the chunk) — the structural proof Verter emitted live, NOT a folded
        // `$.set_attribute(.., 'a <lit> b')`. A folded literal never has `${`.
        assert!(
            code.contains("${"),
            "live-fallback cell {slug} must emit a live template interpolation (`${{…}}`) — a \
             folded output (no `${{`) would mean Verter wrongly folded a not-byte-exact \
             value:\n{code}"
        );
    }
}

#[test]
fn const_fold_buckets_cover_every_required_family_and_eagerness() {
    // The strengthened coverage gate: each of the THREE buckets contributes its required
    // families, the eagerness refuse rows are present, and the buckets are crossed with the
    // class/style/boolean targets (target-independence of the tri-state decision).
    let manifest = codegen_manifest();
    let required = |key: &str| -> Vec<String> {
        manifest[key]
            .as_array()
            .unwrap_or_else(|| panic!("manifest missing {key}"))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    let counts = |key: &str| -> serde_json::Map<String, serde_json::Value> {
        manifest[key]
            .as_object()
            .unwrap_or_else(|| panic!("manifest missing {key}"))
            .clone()
    };

    // Every required family in each bucket has ≥1 row.
    for (req_key, count_key, label) in [
        (
            "required_fold_exact_families",
            "fold_exact_counts",
            "fold-exact",
        ),
        ("required_refuse_families", "refuse_counts", "refuse"),
        (
            "required_live_fallback_families",
            "live_fallback_counts",
            "live-fallback",
        ),
    ] {
        let req = required(req_key);
        assert!(
            !req.is_empty(),
            "the {label} bucket must declare at least one required family"
        );
        let cnts = counts(count_key);
        for fam in &req {
            let n = cnts.get(fam).and_then(|v| v.as_u64()).unwrap_or(0);
            assert!(
                n >= 1,
                "the {label} bucket is missing family `{fam}` (0 rows); regenerate with \
                 scripts/gen-svelte-codegen-corpus.mjs"
            );
        }
    }

    // The EAGERNESS refuse family MUST be present (a throw in a non-selected logical operand
    // / conditional branch — the `false && (1n/0n)` / `true ? 1 : (1n/0n)` rows).
    let refuse_fams: std::collections::BTreeSet<String> =
        required("required_refuse_families").into_iter().collect();
    assert!(
        refuse_fams.contains("refuse_eager"),
        "the refuse bucket must include the `refuse_eager` family (eagerness throws)"
    );

    // The three buckets exist on disk and are non-trivial.
    assert!(
        codegen_refuse_slugs().len() >= 10,
        "the refuse bucket must have ≥10 cells"
    );
    assert!(
        codegen_live_fallback_slugs().len() >= 10,
        "the live-fallback bucket must have ≥10 cells"
    );

    // The buckets are CROSSED with class / style / boolean targets (a `__class` / `__style`
    // / `__boolean` slug suffix proves target-independence of the const-fold decision).
    for (slugs, bucket) in [
        (codegen_refuse_slugs(), "refuse"),
        (codegen_live_fallback_slugs(), "live-fallback"),
    ] {
        for axis in ["class", "style", "boolean"] {
            let suffix = format!("__{axis}");
            assert!(
                slugs.iter().any(|s| s.ends_with(&suffix)),
                "the {bucket} bucket must cross a representative row over the `{axis}` target \
                 (no `…{suffix}` slug)"
            );
        }
    }

    // An eagerness refuse cell MUST actually be present on disk and refuse (cross-check the
    // family declaration against a concrete cell).
    let refuse_slugs = codegen_refuse_slugs();
    assert!(
        refuse_slugs.iter().any(|s| s.contains("eager")),
        "the refuse bucket must contain a concrete eagerness cell"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// The css scoping corpus (5l): coverage, css-field parity (BOTH backends), the
// two-injection-site agreement, and the external-vs-injected routing proof.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn supported_css_covers_the_full_css_corpus() {
    // The css corpus is the structural oracle for scope-class injection +
    // scoped-css routing; a dropped row is a coverage regression. The count
    // gate fails LOUDLY if a row is dropped; the directory walk catches a new
    // fixture that silently skips the gate.
    assert_eq!(
        SUPPORTED_CSS.len(),
        36,
        "the css corpus must enumerate all 36 `css/*` fixtures"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_CSS {
        assert!(slug.starts_with("css/"), "css slug {slug} must be css/*");
        assert!(seen.insert(slug), "duplicate css slug {slug}");
    }
    let fixtures =
        repo_root().join("crates/verter_compiler/tests/svelte_oracle_corpus/fixtures/css");
    for entry in std::fs::read_dir(&fixtures).expect("read css fixtures") {
        let name = entry.expect("dir entry").file_name();
        let name = name.to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".svelte") else {
            continue;
        };
        let slug = format!("css/{stem}");
        assert!(
            seen.contains(slug.as_str()),
            "css fixture {slug} is not enumerated in SUPPORTED_CSS"
        );
    }
}

/// Compile a css fixture to the FULL `ClientModule` (code + external css
/// artifact) under the SAME options the golden was generated with.
fn compile_css_fixture(slug: &str) -> verter_compiler::svelte::runtime::client::ClientModule {
    let source = fixture_source(slug);
    let alloc = Allocator::default();
    let parsed = parse_svelte(&source);
    let opts = compile_options_for(slug);
    compile_client(&source, &parsed, &opts, &alloc, false, false)
        .unwrap_or_else(|e| panic!("client emission failed for {slug}: {e:?}"))
}

/// The committed SERVER golden JSON for a slug.
fn server_golden(slug: &str) -> serde_json::Value {
    let path = repo_root()
        .join("crates/verter_compiler/tests/svelte_oracle_corpus/goldens")
        .join(format!("{slug}.server.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read golden {slug}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse golden {slug}: {e}"))
}

/// Normalize line endings (CRLF → LF) for the cross-platform byte compare.
fn normalize_eol(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[test]
fn css_client_artifact_matches_golden_and_native_server_fails_closed() {
    // Three claims, kept distinct so the test discriminates real behavior:
    //   (1) GOLDEN-CORPUS invariant — svelte's OWN scoped css artifact is
    //       backend-independent (`.client.json` `css` == `.server.json`
    //       `css`); a golden regen that diverged them is a corpus bug. This
    //       is a property of the committed goldens, NOT of Verter's server
    //       backend.
    //   (2) VERTER CLIENT parity — Verter's external css artifact matches the
    //       committed client golden byte-exactly: the REAL hash against the
    //       golden's UNMASKED `css.hash`, and the masked + EOL-normalized
    //       `css.code` against the golden's code.
    //   (3) VERTER NATIVE SERVER is FAIL-CLOSED — native server css emission
    //       is a dedicated future surface (SSR), so `ssr=true` refuses with
    //       `ServerGenerate` BEFORE any css work rather than the test
    //       silently golden-matching a native-server path Verter does not yet
    //       emit. When native server emission lands, this refusal flips and
    //       forces the test to assert real native-server css output.
    for &slug in SUPPORTED_CSS {
        let client = client_golden(slug);
        let server = server_golden(slug);
        // (1) golden-corpus backend-independence.
        assert_eq!(
            client["css"], server["css"],
            "the golden css field must be backend-independent for {slug}"
        );

        // (3) Verter's native server path fails closed today.
        let source = fixture_source(slug);
        let alloc = Allocator::default();
        let parsed = parse_svelte(&source);
        let opts = compile_options_for(slug);
        match compile_client(&source, &parsed, &opts, &alloc, true, false) {
            Err(ClientCompileError::Unsupported(
                UnsupportedSvelteRuntimeSurface::ServerGenerate { .. },
            )) => {}
            other => panic!(
                "native server css must fail closed to ServerGenerate for {slug} \
                 (native server SSR emission is a dedicated future surface), got: {other:?}"
            ),
        }

        // (2) Verter client artifact parity.
        let module = compile_css_fixture(slug);
        let present = client["css"]["present"].as_bool().expect("css.present");
        match (&module.css, present) {
            (Some(css), true) => {
                // The golden hash is the FIRST observable `svelte-<hash>`
                // token in the official `css.code` — NULL when the code
                // carries none (an EMPTY render, or a stylesheet whose every
                // rule pruned unscoped). A null-hash golden pins nothing
                // about the artifact's hash (it is UNOBSERVABLE in official
                // output); the masked-code compare below stays fully
                // discriminating either way.
                match client["css"]["hash"].as_str() {
                    Some(golden_hash) => assert_eq!(
                        css.hash, golden_hash,
                        "the scope hash must match the oracle hash for {slug}"
                    ),
                    None => assert!(
                        !mask_scope_hash(&css.code).contains("svelte-<scoped>"),
                        "a null-hash golden means NO observable hash token in the css code for {slug}: {}",
                        css.code
                    ),
                }
                let golden_code = client["css"]["code"].as_str().expect("css.code");
                assert_eq!(
                    normalize_eol(&mask_scope_hash(&css.code)),
                    normalize_eol(golden_code),
                    "the scoped css.code must match the oracle render for {slug}"
                );
            }
            (None, false) => {}
            (got, want) => panic!(
                "css artifact presence diverges for {slug}: Verter {}, golden {want}",
                if got.is_some() { "present" } else { "absent" }
            ),
        }
    }
}

#[test]
fn scoped_styles_two_injection_sites_agree_on_one_hash() {
    // THE two-injection-site agreement proof (`css/scoped_styles`): the STATIC
    // site bakes `svelte-c4vjvh` (the filename hash of
    // `css/scoped_styles.svelte`) into the `<h2>` skeleton, and the DYNAMIC
    // site threads the SAME hash through the `class:active` div's
    // `$.set_class` value literal. A divergence between the sites is a
    // mis-scope and fails here.
    let module = compile_css_fixture("css/scoped_styles");
    // The STATIC bake (the synthesized class on the class-less scoped <h2>).
    assert!(
        module
            .code
            .contains("<h2 class=\"svelte-c4vjvh\">title</h2>"),
        "the static site bakes the hash into the skeleton:\n{}",
        module.code
    );
    // The DYNAMIC set_class (the literal-string arm: base + ' ' + hash), with
    // the directive `null` placeholder and the `{ active }` next object.
    assert!(
        module
            .code
            .contains("$.set_class(div, 1, 'card svelte-c4vjvh', null, {}, { active })"),
        "the dynamic site threads the SAME hash through $.set_class:\n{}",
        module.code
    );
    // The artifact carries the SAME hash (third reader of the one fact).
    let css = module.css.as_ref().expect("external artifact");
    assert_eq!(css.hash, "svelte-c4vjvh");
    // NEGATIVE: every `svelte-<hash>` token in the emitted module is the ONE
    // scope hash — no second hash, no `(unknown)` fallback (which would hash the
    // css text to a DIFFERENT value). Collected from the UNMASKED code so a
    // divergent token stays visible (masking would collapse them together).
    let distinct_hashes: std::collections::BTreeSet<&str> = module
        .code
        .match_indices("svelte-")
        .map(|(i, _)| {
            let start = i + "svelte-".len();
            let len = module.code[start..]
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(module.code.len() - start);
            &module.code[i..start + len]
        })
        .collect();
    assert_eq!(
        distinct_hashes,
        std::collections::BTreeSet::from(["svelte-c4vjvh"]),
        "exactly one distinct scope hash (no second hash, no `(unknown)` leak):\n{}",
        module.code
    );
    // NEGATIVE: the unmatched `<p>` is NOT scoped.
    assert!(
        module.code.contains("<p>body</p>"),
        "the unmatched <p> stays unscoped:\n{}",
        module.code
    );
}

#[test]
fn external_vs_injected_routing_matches_the_goldens() {
    // EXTERNAL (`css/scoped_styles`): the artifact publishes, the module has
    // NO inject machinery; the golden css.present is true on BOTH backends.
    let external = compile_css_fixture("css/scoped_styles");
    assert!(external.css.is_some());
    assert!(!external.code.contains("$$css"));
    assert!(!external.code.contains("$.append_styles"));
    assert_eq!(
        client_golden("css/scoped_styles")["css"]["present"],
        serde_json::json!(true)
    );
    assert_eq!(
        server_golden("css/scoped_styles")["css"]["present"],
        serde_json::json!(true)
    );

    // INJECTED (`css/injected_mode`): the module hoists `$$css` + prepends
    // `$.append_styles($$anchor, $$css)`, publishes NO artifact; the golden
    // css.present is false on BOTH backends (compiled.css is null).
    let injected = compile_css_fixture("css/injected_mode");
    assert!(injected.css.is_none());
    assert!(injected.code.contains("const $$css = {"));
    assert!(injected.code.contains("$.append_styles($$anchor, $$css);"));
    assert_eq!(
        client_golden("css/injected_mode")["css"]["present"],
        serde_json::json!(false)
    );
    assert_eq!(
        server_golden("css/injected_mode")["css"]["present"],
        serde_json::json!(false)
    );
}

#[test]
fn keyframes_rename_reaches_the_positive_golden() {
    // `css/keyframes_animation` is the POSITIVE @keyframes golden: the local
    // `spin` renames to `<hash>-spin` (at-rule + both animation references),
    // the `-global-fade` prefix strips WITHOUT a rename, and the scoped
    // `.spinner` rule carries the scope class.
    let module = compile_css_fixture("css/keyframes_animation");
    let css = module.css.as_ref().expect("external artifact");
    let masked = mask_scope_hash(&css.code);
    assert!(
        masked.contains("@keyframes svelte-<scoped>-spin"),
        "the local keyframes renames: {masked}"
    );
    assert!(
        masked.contains("animation: svelte-<scoped>-spin 1s linear infinite;"),
        "the animation shorthand token rewrites: {masked}"
    );
    assert!(
        masked.contains("animation-name: svelte-<scoped>-spin;"),
        "the animation-name token rewrites: {masked}"
    );
    assert!(
        masked.contains("@keyframes fade"),
        "the -global- prefix strips without a rename: {masked}"
    );
    // NEGATIVE: no un-renamed local reference survives, and the global name
    // is never hash-prefixed.
    assert!(!masked.contains("animation-name: spin"), "{masked}");
    assert!(!masked.contains("-global-"), "{masked}");
    assert!(!masked.contains("svelte-<scoped>-fade"), "{masked}");
}
