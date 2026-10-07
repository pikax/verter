use super::*;

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
    // against svelte@5.56.10. RED against the pre-fix tree (which hard-coded `var root`).
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
fn nested_parent_child_binds_validate_post_order() {
    // Cross-element batch order is POST-ORDER (the official after_update merge:
    // `…child_state.after_update, …element_state.after_update`): the CHILD's bind
    // registration precedes the PARENT's even though the parent's attribute is
    // authored first. Bind-only elements still batch — batching is not gated on a
    // co-located transition/event.
    let js = emit(
        "<script>let v = $state(\"\"); let w = $state(0);</script>\n<div bind:clientWidth={w}><input bind:value={v} /></div>\n",
        "App.svelte",
    );
    let child = js.find("$.bind_value(input").expect("emits the child bind");
    let parent = js
        .find("$.bind_element_size(div")
        .expect("emits the parent bind");
    assert!(
        child < parent,
        "the child's bind must precede the parent's (post-order batch merge):\n{js}"
    );
    // NEGATIVE: neither bind effect-wraps (no `use:` anywhere).
    assert!(
        !js.contains("$.effect(() =>"),
        "no effect wrap without use::\n{js}"
    );
}

#[test]
fn global_host_bind_this_creates_no_stale_inline_index_entry() {
    // `bind_emission_slot` is the LITERAL single authority for `bind:this`
    // placement: `build_inline_render_index` admits ONLY the `InlineThis` slot.
    // A GLOBAL-host `bind:this` (`<svelte:window|document|body>` — the
    // `SpecialHost` slot) gets NO inline entry: a global host renders no element,
    // so an indexed entry could never drain (a stale index row); its registration
    // emits post-walk in the init body instead.
    use crate::svelte::runtime::client_lifecycle::{
        action_host_nodes, build_inline_render_index, InlineRenderOp,
    };
    use crate::svelte::runtime::client_plan::SupportedClientIr;
    use crate::svelte::runtime::client_plan_types::ClientNode;
    use crate::svelte::runtime::client_surface::ClientSyntaxSurface;
    use crate::svelte::runtime::ir::NodeId;
    use crate::svelte::runtime::lower_parsed_svelte_to_ir;

    let source = "<script>let w = $state(0); let d = $state(0);</script>\n<svelte:window bind:this={w} />\n<div bind:this={d}></div>\n";
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc).expect("lowering succeeds");
    let classified = ClientSyntaxSurface::classify(&ir).expect("the surface classifies");
    let plan = SupportedClientIr::build(&classified, &ir, None).expect("the plan builds");
    let hosts = action_host_nodes(&plan);
    let index = build_inline_render_index(&plan, &hosts);
    // NEGATIVE: the global-host node has NO inline render sequence at all.
    let host = plan
        .nodes
        .iter()
        .position(|n| matches!(n, ClientNode::SpecialHost { .. }))
        .expect("the plan carries the global-host node");
    assert!(
        !index.contains_key(&NodeId(host as u32)),
        "a global-host bind:this must not create an inline index entry (it can never drain)"
    );
    // POSITIVE: the regular `<div bind:this>` stays inline-indexed — exactly ONE
    // BindThis entry across the whole index.
    let bind_this_entries = index
        .values()
        .flatten()
        .filter(|op| matches!(op, InlineRenderOp::BindThis { .. }))
        .count();
    assert_eq!(
        bind_this_entries, 1,
        "exactly the regular-element bind:this is indexed inline"
    );
    // The emitted module still registers the global-host bind post-walk (the
    // routing change is output-preserving).
    let js = emit(source, "App.svelte");
    assert!(
        js.contains("$.bind_this($.window, "),
        "the global-host bind:this still emits in the init body:\n{js}"
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
fn input_spread_with_a_default_value_reset_attr_suppresses_the_trailing_tail() {
    // An `<input>` spread fold normally takes the 7-argument tail (`…, void 0, void 0,
    // void 0, void 0, true`). The official compiler SUPPRESSES that tail when the input
    // carries an authored plain attribute named EXACTLY `defaultValue` (camelCase): the
    // reset attribute opts the element out of the value/defaultValue reset behavior the
    // tail encodes. Pinned svelte@5.56.10:
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
    // svelte@5.56.10: `$.attribute_effect(input, () => ({ ...$$props.p, defaultChecked:
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
    // `defaultvalue` is NOT a reset attribute, so the tail STAYS. Pinned svelte@5.56.10:
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
    // svelte@5.56.10 keeps the 7-argument tail.
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
    // A static `defaultValue` CO-LOCATED with a `bind:value` on an `<input>` IS a
    // supported surface: official emits the `input.defaultValue = 'x'` property write
    // BEFORE the bind, and the default attribute SUPPRESSES the `$.remove_input_defaults`
    // prelude (the default is set explicitly). Verified against svelte@5.56.10:
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
    // Source attribute ORDER does not matter: `<input bind:value={v} defaultValue="x">`
    // (default attr AFTER the bind in source) still emits `input.defaultValue = 'x'` BEFORE
    // the `$.bind_value` call. Verified against svelte@5.56.10 (identical output to the
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
    // A valueless static `defaultChecked` CO-LOCATED with a `bind:checked` on a
    // checkbox `<input>` IS supported: official emits `input.defaultChecked = true` BEFORE
    // the bind, suppressing `$.remove_input_defaults`. Verified against svelte@5.56.10:
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
    // A static `defaultValue` co-located with `bind:value` on a `<textarea>` IS
    // supported. Verified against svelte@5.56.10: the `$.remove_textarea_child` prelude is
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
    // "defaultChecked" }`). Official svelte@5.56.10 ACCEPTS it (oracle-verified: emits
    // `input.defaultChecked = true;`), but standalone form-default PROPERTY-attribute emission
    // is OUT of the DOM-bind backend's ordinary-DOM `bind:*` charter (D-27). The acceptance is gated on a
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
    // refusal (NARROWER than official, which accepts the mixed form; the DOM-bind backend keeps the strict
    // co-location boundary). RED would be an acceptance keyed on "any default + any bind".
    assert_fail_closed(
        "<script>let v = $state(\"\");</script>\n<input defaultChecked bind:value={v} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "defaultChecked"),
    );
}

#[test]
fn no_value_radio_group_bind_still_declares_binding_group() {
    // FIX 3: a `bind:group` WITHOUT a `value` attr. Official svelte@5.56.10 STILL
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
    // DOM-bind backend — radio `bind:group` (primitive `$state('')`) EMITS (oracle CASE `group`):
    // a component-FUNCTION-scoped `const binding_group = []`, per-input
    // `$.remove_input_defaults` + `input.value = input.__value = '<value>'`, and a
    // per-input `$.bind_group(binding_group, [], input, () => $.get(g), ($$value) =>
    // $.set(g, $$value))`. RED against the pre-DOM-bind tree (which refused `bind:group`).
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
    // must each get their OWN accumulator. Official svelte@5.56.10 emits `const binding_group =
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
    // operator difference split them into TWO accumulators. Official svelte@5.56.10
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
    // `a.["x"]`) are DISTINCT group identities in svelte@5.56.10 → TWO accumulators. The
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
fn shared_bind_group_target_shares_one_accumulator() {
    // FIX 1 (R3c): two inputs binding the SAME group target (`bind:group={g}` ×2) share ONE
    // accumulator — official svelte@5.56.10 emits a single `const binding_group = []` and both
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
    // Verified against svelte@5.56.10:
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
    // The static `bind:group` `value` attribute is ENTITY-DECODED before the
    // `input.value = input.__value` write — official runs the static value through the
    // attribute-value entity decoder, exactly like every other static attribute. Verified
    // against svelte@5.56.10 for `value="a&amp;b"`:
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
    // DOM-bind backend — `bind:checked` on an `<input type="checkbox">` EMITS (it used to fail
    // closed). The pinned svelte@5.56.10 shape (oracle CASE `checked`) is
    // `$.remove_input_defaults(input)` then `$.bind_checked(input, () => $.get(c),
    // ($$value) => $.set(c, $$value))`. RED against the pre-DOM-bind tree (which refused it).
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
// attribute; official svelte@5.56.10 raises a COMPILE ERROR otherwise. The runtime
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
    // independent of the value type (verified against svelte@5.56.10 with a string
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
    // `$.bind_select_value` (verified against svelte@5.56.10 with a string value).
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
// official svelte@5.56.10: "'type' attribute must be a static text value if input
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
    // specifically; the gate must not over-refuse it. Verified against svelte@5.56.10.
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
    // valid static-type form. Verified against svelte@5.56.10.
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
    // closed). Verified against svelte@5.56.10.
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
    // reached — a runeless component routes through the legacy per-surface dispatch first.)
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
    // An UNINITIALIZED plain-local bind target (`let v;`, never a rune) is a
    // supported DOM-bind target — official keeps the bare local verbatim and emits the
    // plain read/write closures `$.bind_value(input, () => v, ($$value) => v = $$value)`,
    // identical to the initialized plain-local shape. Verified against svelte@5.56.10:
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
fn bind_value_object_state_member_emits_proxy_member_setter() {
    // INVERTED (was `bind_value_object_state_member_fails_closed_at_the_object_state_decl_gate`):
    // once the object/array `$state` declarator gate opens, a member bind rooted at an
    // object `$state` IS representable — the bind planner consumes the member read/write
    // forms (empirically confirmed). A NEVER-reassigned root is a `BareProxy`: the
    // getter/setter stay PLAIN (`() => o.x` / `o.x = $$value`).
    let bare = emit(
        "<script>let o = $state({ x: '' });</script>\n<input bind:value={o.x} />\n",
        "App.svelte",
    );
    assert!(
        bare.contains("let o = $.proxy({ x: '' });"),
        "a never-reassigned object `$state` is a bare `$.proxy`:\n{bare}"
    );
    assert!(
        bare.contains("$.bind_value(input, () => o.x, ($$value) => o.x = $$value)"),
        "a BareProxy member bind stays PLAIN (`o.x`):\n{bare}"
    );
    // NEGATIVE: a BareProxy is not a signal — its member bind never `$.get`s.
    assert!(
        !bare.contains("$.get(o)"),
        "a BareProxy member bind must not `$.get`:\n{bare}"
    );
    // The REASSIGNED object `$state` (a `StateProxy`) reads/writes through the signal
    // (`() => $.get(o).x`), and the reassign carries the proxy flag.
    let proxy = emit(
        "<script>let o = $state({ x: '' });</script>\n<input bind:value={o.x} />\n<button onclick={() => o = { x: 'y' }}>r</button>\n",
        "App.svelte",
    );
    assert!(
        proxy.contains("let o = $.state($.proxy({ x: '' }));"),
        "a reassigned object `$state` is `$.state($.proxy(…))`:\n{proxy}"
    );
    assert!(
        proxy.contains("$.bind_value(input, () => $.get(o).x, ($$value) => $.get(o).x = $$value)"),
        "a StateProxy member bind reads/writes via `$.get(o).x`:\n{proxy}"
    );
    assert!(
        proxy.contains("$.set(o, { x: 'y' }, true)"),
        "the StateProxy reassign carries the proxy flag:\n{proxy}"
    );
}

#[test]
fn bind_select_value_with_array_state_emits_proxy_signal() {
    // INVERTED (was `bind_select_value_with_array_state_fails_closed_at_the_array_state_decl_gate`):
    // the canonical official `<select multiple>` shape binds an ARRAY `$state([])`
    // target. Once the object/array `$state` gate opens, the array `$state([])` is a
    // reassigned `StateProxy` (the two-way bind writes back the bare identifier), so it
    // emits `let v = $.state($.proxy([]))` + `$.bind_select_value(select, () => $.get(v),
    // ($$value) => $.set(v, $$value))` — verified against svelte@5.56.10.
    let js = emit(
        "<script>let v = $state([]);</script>\n<select multiple bind:value={v}><option>a</option></select>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let v = $.state($.proxy([]));"),
        "an array `$state([])` bind target is `$.state($.proxy([]))`:\n{js}"
    );
    assert!(
        js.contains("$.bind_select_value(select, () => $.get(v), ($$value) => $.set(v, $$value))"),
        "the bare-identifier select bind reads/writes the signal:\n{js}"
    );
}

#[test]
fn bind_value_member_of_import_admitted_with_frame() {
    // A MEMBER of an import is an ACCEPTED bind lvalue — official svelte@5.56.10
    // emits the plain member closures `$.bind_value(input, () => store.x,
    // ($$value) => store.x = $$value)` AND the member read opens the component
    // context frame (`$.push($$props, true)` / `$.pop()` + the `$$props` param).
    // Only the BARE import root `bind:value={store}` is rejected (the
    // non-writable-import-root gate; official `constant_binding`).
    let js = emit(
        "<script>import { store } from './s.js'; let c = $state(0);</script>\n<input bind:value={store.x} />\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => store.x, ($$value) => store.x = $$value)"),
        "the member-of-import bind must emit the plain member closures:\n{js}"
    );
    assert!(
        js.contains("import { store } from './s.js';"),
        "the import must be hoisted to the module prelude:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true)") && js.contains("$.pop()"),
        "the imported-member read must open the component context frame:\n{js}"
    );
    assert!(
        js.contains("($$anchor, $$props)"),
        "the frame must bind the $$props parameter:\n{js}"
    );
    // NEGATIVE: the import binding is never a signal — no `$.get(store)`, and the
    // setter is the plain member assignment, never `$.set`.
    assert!(
        !js.contains("$.get(store)"),
        "an import read must stay plain (never `$.get`):\n{js}"
    );
    assert!(
        !js.contains("$.set(store"),
        "an import member setter must stay a plain member write (never `$.set`):\n{js}"
    );
}

#[test]
fn bind_value_bare_import_root_fails_closed_at_the_bind_lvalue_gate() {
    // The BARE import root `bind:value={store}` REJECTS at the bind-lvalue-root
    // gate (an import is a NON-writable root — official `constant_binding`,
    // "Cannot bind to import"), NOT at the script gate: the import itself is
    // admitted to the prelude, the bind target is the refusal.
    assert_fail_closed(
        "<script>import { store } from './s.js'; let c = $state(0);</script>\n<input bind:value={store} />\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn select_static_value_attr_fails_closed_at_the_form_control_gate() {
    // `<select value="x">` — the static `value` on the now-allowed `<select>` host is
    // the form-control setter family (the DOM-bind backend owns `bind:value`, not the static-`value`
    // attr), so it fails closed at the attr gate, NOT the element gate.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<select value=\"x\"><option>A</option></select>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "value"),
    );
}

#[test]
fn textarea_static_value_attr_fails_closed_at_the_form_control_gate() {
    // `<textarea>` is now an allowed DOM-bind host, so a static `value` attr (the
    // form-control setter family — the DOM-bind backend emits `bind:value`, not the static-`value`
    // serializer) fails closed at the attr gate. The empty content passes the
    // special-content gate; the static `value` is the refusal.
    assert_fail_closed(
        "<script>let c = $state(0);</script>\n<textarea value=\"hi\"></textarea>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DynamicAttribute { name, .. } if name == "value"),
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
fn component_bind_prop_unwritable_root_fails_closed() {
    // A component `bind:value={p}` whose root resolves to a `$props()` PROP (a non-writable
    // root under the shared DOM-bind writable-root policy) fails CLOSED — the component bind setter
    // is never synthesized from a non-writable root. The prop-bind refusal sweep scans only
    // `IrNode::Element`, so this gate is what catches a COMPONENT bind to a prop.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child bind:value={p} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn a_member_bind_rooted_at_an_each_item_is_accepted_for_a_component_bind() {
    // The COMPONENT-bind sibling of `a_member_bind_rooted_at_an_each_item_is_accepted_by_official`:
    // official ACCEPTS a member-rooted bind on a custom component the same way it accepts one on
    // a DOM element — `<Child bind:value={item.x}/>` inside an `{#each}` is a plain deep-write
    // through the item's own referenced value (oracle-verified against svelte@5.56.10, cosmetic
    // formatting aside):
    //   get value() { return item.x; }
    //   set value($$value) { item.x = $$value; }
    // `component_bind_root_is_writable`'s Member arm routes through the same widened
    // `bind_member_root_is_writable_target` the DOM-bind classifier uses (not the narrower
    // `bind_root_is_writable_target`, which rejects an each-item root).
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let items = $state([{x:'a'}]);</script>\n{#each items as item (item)}<Child bind:value={item.x}/>{/each}\n",
    )
    .expect("a member bind on an each item must be accepted for a component bind");
    assert!(
        js.contains("get value() {return item.x;}")
            && js.contains("set value($$value) {item.x = $$value;}"),
        "a member bind on an each item must lower to the official component getter AND setter:\n{js}"
    );
    // NEGATIVE: a bare each-item bind (no member) must still refuse — the widening applies to
    // the Member arm only, never the Identifier arm.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let items = $state([{x:'a'}]);</script>\n{#each items as item (item)}<Child bind:value={item}/>{/each}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn a_member_bind_rooted_at_a_snippet_param_is_accepted() {
    // A member bind rooted at a `{#snippet}` PARAMETER (`item`) is a plain deep-write
    // through the parameter's own referenced value — official ACCEPTS
    // `bind:value={item.x}` (oracle-verified against svelte@5.56.10).
    let js = emit_result(
        "<script>let items = $state([{x:'a'}]);</script>\n{#snippet row(item)}<input bind:value={item.x}/>{/snippet}\n{@render row(items[0])}\n",
    )
    .expect("a member bind rooted at a snippet param must be accepted");
    // A snippet param is itself a thunk (`item = $.noop`), so a member read/write
    // through it calls the thunk first: `item().x`, not the bare `item.x` a plain
    // local would emit.
    assert!(
        js.contains("$.bind_value(input, () => item().x, ($$value) => item().x = $$value)"),
        "a member bind on a snippet param must read/write through item().x:\n{js}"
    );
    // NEGATIVE: a bare snippet-param identifier bind must still refuse — the widening
    // applies to the Member arm only, never the Identifier arm.
    assert_fail_closed(
        "<script></script>\n{#snippet row(item)}<input bind:value={item}/>{/snippet}\n{@render row('a')}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn a_member_bind_rooted_at_an_each_destructured_field_is_accepted() {
    // A member bind rooted at an `{#each items as {x}}` DESTRUCTURED FIELD (`x`) is a
    // plain deep-write through the field's own referenced value — official ACCEPTS
    // `bind:value={x.y}` (oracle-verified against svelte@5.56.10).
    let js = emit_result(
        "<script>let items = $state([{x:{y:'a'}}]);</script>\n{#each items as {x}}<input bind:value={x.y}/>{/each}\n",
    )
    .expect("a member bind rooted at an each-destructured field must be accepted");
    assert!(
        js.contains("$.bind_value(input, () => x().y, ($$value) => x().y = $$value)"),
        "a member bind on a destructured field must read/write through x().y:\n{js}"
    );
    // NEGATIVE: a bare destructured-field identifier bind must still refuse.
    assert_fail_closed(
        "<script>let items = $state([{x:{y:'a'}}]);</script>\n{#each items as {x}}<input bind:value={x}/>{/each}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn a_member_bind_rooted_at_an_await_then_binding_is_accepted() {
    // A member bind rooted at an `{#await p then item}` binding is a plain deep-write
    // through the item's own referenced value — official ACCEPTS `bind:value={item.x}`
    // (oracle-verified against svelte@5.56.10).
    let js = emit_result(
        "<script>let p = Promise.resolve({x:'a'});</script>\n{#await p then item}<input bind:value={item.x}/>{/await}\n",
    )
    .expect("a member bind rooted at an await-then binding must be accepted");
    assert!(
        js.contains(
            "$.bind_value(input, () => $.get(item).x, ($$value) => $.get(item).x = $$value)"
        ),
        "a member bind on an await-then binding must read/write through $.get(item).x:\n{js}"
    );
    // NEGATIVE: a bare await-then identifier bind must still refuse.
    assert_fail_closed(
        "<script>let p = Promise.resolve({x:'a'});</script>\n{#await p then item}<input bind:value={item}/>{/await}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn a_member_bind_rooted_at_a_legacy_const_derived_binding_is_accepted() {
    // A member bind rooted at a `{@const alias = item}` derived local is a plain
    // deep-write through the alias's own referenced value — official ACCEPTS
    // `bind:value={alias.x}` (oracle-verified against svelte@5.56.10).
    let js = emit_result(
        "<script>let items = $state([{x:'a'}]);</script>\n{#each items as item}{@const alias = item}<input bind:value={alias.x}/>{/each}\n",
    )
    .expect("a member bind rooted at a legacy const-derived binding must be accepted");
    assert!(
        js.contains(
            "$.bind_value(input, () => $.get(alias).x, ($$value) => $.get(alias).x = $$value)"
        ),
        "a member bind on a const-derived alias must read/write through $.get(alias).x:\n{js}"
    );
    // NEGATIVE: a bare const-derived identifier bind must still refuse.
    assert_fail_closed(
        "<script>let items = $state([{x:'a'}]);</script>\n{#each items as item}{@const alias = item}<input bind:value={alias}/>{/each}\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn a_member_bind_rooted_at_a_derived_binding_is_accepted() {
    // A member bind rooted at a `SlotPropDerived` binding is a plain deep-write through
    // the derived's own referenced value — official ACCEPTS `bind:value={item.x}` where
    // `item` is a component `let:` slot-prop (lowered to `const item =
    // $.derived(() => $$slotProps.item)`, `BindingRuntimeKind::SlotPropDerived` — the
    // same signal-read shape as a genuine `$derived(...)` rune declarator's `Derived`
    // kind, but a distinct kind at the bare-Identifier bind-writability gate —
    // oracle-verified against svelte@5.56.10, `runes: true`):
    //   $.bind_value(input, () => $.get(item).x, ($$value) => $.get(item).x = $$value)
    // The exact same `$.get(root).field` shape already implemented for `AwaitSignal` /
    // `LegacyConstDerived` roots in this classifier.
    //
    // A top-level `let d = $derived(e)` INSTANCE-SCRIPT rune declarator is NOT used as
    // the test vehicle here: that form fails closed at the earlier, unrelated
    // rune-position gate (`rune_scan.rs::classify_rune_position` — "`$derived` has NO
    // supported position"), so it could never reach — and could never discriminate —
    // this classifier. The `let:` slot-prop construct reaches this classifier through a
    // path that gate does not cover. A genuine `$derived(...)` rune ALSO reaches this
    // classifier through a second, separate path the rune-position gate does not cover
    // either — a `{let x = $derived(e)}` TEMPLATE declaration tag
    // (`declaration_tag_lowering.rs::lower_declaration_tag`, reclassified by
    // `state_prep::classify_block_rune_declarator`); see
    // `a_member_bind_rooted_at_a_declaration_tag_derived_rune_is_accepted` below for that
    // path exercised directly.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child let:item><input bind:value={item.x}/></Child>\n",
    )
    .expect("a member bind rooted at a derived binding must be accepted");
    assert!(
        js.contains(
            "$.bind_value(input, () => $.get(item).x, ($$value) => $.get(item).x = $$value)"
        ),
        "a member bind on a derived binding must read/write through $.get(item).x:\n{js}"
    );
    // NEGATIVE: a bare `SlotPropDerived`-root identifier bind must still refuse —
    // official REJECTS `bind:value={item}` here (`constant_binding`, oracle-verified);
    // the widening applies to the Member arm only, never the Identifier arm, and a
    // `let:` slot-prop never mints the genuine-rune `Derived` kind that IS admitted at
    // the Identifier arm (see `a_member_bind_rooted_at_a_declaration_tag_derived_rune_is_accepted`).
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; let { p } = $props();</script>\n<Child let:item><input bind:value={item}/></Child>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn a_member_bind_rooted_at_an_import_is_accepted_for_a_component_bind() {
    // The COMPONENT-bind sibling of `bind_value_member_of_import_admitted_with_frame`:
    // official ACCEPTS `<Child bind:value={store.x}/>` the same way it accepts
    // `<input bind:value={store.x}/>` — a plain deep-write through the imported value's
    // own referenced value (oracle-verified against svelte@5.56.10). Before the fix this
    // REFUSED because `component_bind_root_is_writable`'s Member arm never ported the
    // DOM-bind Member arm's `root_is_import` OR-clause.
    //
    // The fixture carries an unused `let { p } = $props();` to force RUNES mode: with no
    // rune usage at all the component compiles in LEGACY mode instead, whose
    // component-bind codegen omits the `$$legacy: true` props-object marker, which
    // would otherwise ride along unasserted in what this test checks.
    let js = emit_result(
        "<script>import Child from './Child.svelte'; import { store } from './s.js'; let { p } = $props();</script>\n<Child bind:value={store.x} />\n",
    )
    .expect("a member bind rooted at an import must be accepted for a component bind");
    assert!(
        js.contains("get value() {return store.x;}")
            && js.contains("set value($$value) {store.x = $$value;}"),
        "a member bind on an import must lower to the official component getter AND setter:\n{js}"
    );
    // NEGATIVE: a bare import identifier component bind must still refuse.
    assert_fail_closed(
        "<script>import Child from './Child.svelte'; import { store } from './s.js'; let { p } = $props();</script>\n<Child bind:value={store} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

#[test]
fn svelte_window_size_bind_emits_bind_window_size() {
    // `<svelte:window bind:innerWidth={w}/>` → `$.bind_window_size('innerWidth', ($$value) =>
    // $.set(w, $$value, true))` — the dimension NAME first, NO host expr, setter-only, with
    // the window-host `should_proxy` flag.
    let js = emit(
        "<script>let w = $state(0);</script>\n<svelte:window bind:innerWidth={w} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.bind_window_size('innerWidth', ($$value) => $.set(w, $$value, true))"
        )),
        "bind:innerWidth must emit bind_window_size with the proxy flag:\n{js}"
    );
    // NEGATIVE: no host expr passed to bind_window_size, no DOM clone/append.
    assert!(
        !n.contains(&nc("$.bind_window_size($.window")),
        "no host expr for window_size:\n{js}"
    );
    assert!(
        !js.contains("$.from_html") && !js.contains("$.append"),
        "no DOM frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_window_scroll_bind_emits_bind_window_scroll() {
    // `<svelte:window bind:scrollX={sx}/>` → `$.bind_window_scroll('x', () => $.get(sx),
    // ($$value) => $.set(sx, $$value, true))` — the axis name REMAPPED to `'x'`, READ-WRITE
    // (get+set), with the proxy flag.
    let js = emit(
        "<script>let sx = $state(0);</script>\n<svelte:window bind:scrollX={sx} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.bind_window_scroll('x', () => $.get(sx), ($$value) => $.set(sx, $$value, true))"
        )),
        "bind:scrollX must emit bind_window_scroll('x', get, set):\n{js}"
    );
    // NEGATIVE: the runtime name is 'x', NEVER the literal 'scrollX'.
    assert!(
        !n.contains(&nc("'scrollX'")),
        "scrollX must remap to 'x', never 'scrollX':\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_body_scrollx_bind_is_invalid_and_fails_closed() {
    // `<svelte:body bind:scrollX={sx}/>` is an OFFICIAL COMPILE ERROR — `<svelte:body>` has
    // NO `scrollX`/`scrollY` (those are window-only). It must NEVER emit; Verter refuses it
    // at the HOST-SCOPED bind contract (`scrollX` is window-only, so the body host has no
    // routing) → the generic `Binding` refusal. The EXACT official `bind_invalid_target`
    // code/order is the D-29 deferral; the bind STILL fails closed (never emits).
    assert_fail_closed(
        "<script>let sx = $state(0);</script>\n<svelte:body bind:scrollX={sx} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "scrollX"),
    );
}

#[test]
fn svelte_document_active_element_bind_emits_bind_active_element() {
    // `<svelte:document bind:activeElement={el}/>` → `$.bind_active_element(($$value) =>
    // $.set(el, $$value, true))` — the DEDICATED setter-only helper, NO name, NO host expr
    // (NOT `$.bind_property`).
    let js = emit(
        "<script>let el = $state(0);</script>\n<svelte:document bind:activeElement={el} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.bind_active_element(($$value) => $.set(el, $$value, true))"
        )),
        "bind:activeElement must emit the dedicated bind_active_element:\n{js}"
    );
    // NEGATIVE: activeElement is NOT routed through $.bind_property.
    assert!(
        !n.contains(&nc("$.bind_property('activeElement'")),
        "activeElement is not bind_property:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_document_this_bind_emits_bind_this_against_document() {
    // `<svelte:document bind:this={d}/>` → `$.bind_this($.document, ($$value) => $.set(d,
    // $$value, true), () => $.get(d))`.
    let js = emit(
        "<script>let d = $state(0);</script>\n<svelte:document bind:this={d} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.bind_this($.document, ($$value) => $.set(d, $$value, true), () => $.get(d))"
        )),
        "document bind:this must emit bind_this against $.document:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_window_this_bind_emits_bind_this_against_window() {
    // `<svelte:window bind:this={w}/>` → `$.bind_this($.window, ($$value) => $.set(w,
    // $$value, true), () => $.get(w))` — the window-host should_proxy setter.
    let js = emit(
        "<script>let w = $state(0);</script>\n<svelte:window bind:this={w} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.bind_this($.window, ($$value) => $.set(w, $$value, true), () => $.get(w))"
        )),
        "window bind:this must emit bind_this against $.window:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_window_online_bind_emits_bind_online() {
    // `<svelte:window bind:online={on}/>` → `$.bind_online(($$value) => $.set(on, $$value,
    // true))` — setter-only, NO name, NO host.
    let js = emit(
        "<script>let on = $state(false);</script>\n<svelte:window bind:online={on} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc("$.bind_online(($$value) => $.set(on, $$value, true))")),
        "bind:online must emit the setter-only bind_online:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_body_this_bind_emits_bind_this_against_body() {
    // `<svelte:body bind:this={b}/>` → `$.bind_this($.document.body, ($$value) => $.set(b,
    // $$value, true), () => $.get(b))`.
    let js = emit(
        "<script>let b = $state(0);</script>\n<svelte:body bind:this={b} />\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    assert!(
        n.contains(&nc(
            "$.bind_this($.document.body, ($$value) => $.set(b, $$value, true), () => $.get(b))"
        )),
        "body bind:this must emit bind_this against $.document.body:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn svelte_host_bind_invalid_name_or_wrong_host_pairs_fail_closed() {
    // The §1.8 wrong-host / unknown-name negatives — each is an official compile error
    // (`bind_invalid_target` / `bind_invalid_name`); Verter fails them closed at the
    // HOST-SCOPED bind contract (the bind never emits). The EXACT official code/order is the
    // D-29 deferral, so the discriminator is the generic `Binding` refusal naming the bad
    // bind target.
    for (src, target) in [
        // bind name valid on ANOTHER host, used on the WRONG host (bind_invalid_target).
        (
            "<script>let sx = $state(0);</script>\n<svelte:document bind:scrollX={sx} />\n",
            "scrollX",
        ),
        (
            "<script>let w = $state(0);</script>\n<svelte:document bind:innerWidth={w} />\n",
            "innerWidth",
        ),
        (
            "<script>let vs = $state(0);</script>\n<svelte:body bind:visibilityState={vs} />\n",
            "visibilityState",
        ),
        // a dimension bind (window-INVALID per svelte's invalid_elements) on window
        // (bind_invalid_name — clientWidth is not a window binding).
        (
            "<script>let cw = $state(0);</script>\n<svelte:window bind:clientWidth={cw} />\n",
            "clientWidth",
        ),
        // a totally-unknown bind name (bind_invalid_name, no host).
        (
            "<script>let x = $state(0);</script>\n<svelte:window bind:fooBar={x} />\n",
            "fooBar",
        ),
    ] {
        assert_fail_closed(
            src,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target: t, .. } if t == target),
        );
    }
}

#[test]
fn svelte_element_binds_run_against_the_element_callback_param() {
    // `bind:this` / dimension binds on `<svelte:element>` run against the `$$element` callback
    // param with the proxied host setter.
    let this_js = emit(
        "<script>let tag = $state('div');let el = $state(0);</script>\n<svelte:element this={tag} bind:this={el}>hi</svelte:element>\n",
        "App.svelte",
    );
    assert!(
        normalize_js_cosmetics(&this_js).contains(&nc(
            "$.bind_this($$element, ($$value) => $.set(el, $$value, true), () => $.get(el))"
        )),
        "element bind:this runs against $$element:\n{this_js}"
    );
    let dim_js = emit(
        "<script>let tag = $state('div');let w = $state(0);</script>\n<svelte:element this={tag} bind:clientWidth={w}>hi</svelte:element>\n",
        "App.svelte",
    );
    assert!(
        normalize_js_cosmetics(&dim_js).contains(&nc(
            "$.bind_element_size($$element, 'clientWidth', ($$value) => $.set(w, $$value, true))"
        )),
        "element dimension bind runs against $$element with the proxy flag:\n{dim_js}"
    );
}

#[test]
fn svelte_element_bind_this_precedes_the_attribute_fold() {
    // Official `<svelte:element>` setup order: `bind:this` is a REF CAPTURE pushed into
    // the init body DURING the attribute loop, so it precedes the `$.attribute_effect`
    // fold; measurement/property binds are `after_update` and FOLLOW the fold (verified
    // against pinned svelte@5.56.10).
    let js = emit(
        "<script>let tag = $state('div');let w = $state(1);let el = $state(null);</script>\n<svelte:element this={tag} data-x={w} bind:this={el}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n = normalize_js_cosmetics(&js);
    let bind_pos = n.find("$.bind_this($$element").expect("bind_this emitted");
    let fold_pos = n
        .find("$.attribute_effect($$element")
        .expect("fold emitted");
    assert!(
        bind_pos < fold_pos,
        "bind:this must be emitted BEFORE the attribute fold:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");

    // INVERSE lock: a MEASUREMENT bind stays AFTER the fold (already-official order —
    // must not be dragged before the fold by the bind:this reorder).
    let js2 = emit(
        "<script>let tag = $state('div');let w = $state(1);let cw = $state(0);</script>\n<svelte:element this={tag} data-x={w} bind:clientWidth={cw}>hi</svelte:element>\n",
        "App.svelte",
    );
    let n2 = normalize_js_cosmetics(&js2);
    let fold_pos2 = n2
        .find("$.attribute_effect($$element")
        .expect("fold emitted");
    let dim_pos = n2
        .find("$.bind_element_size($$element")
        .expect("dimension bind emitted");
    assert!(
        fold_pos2 < dim_pos,
        "a measurement bind must stay AFTER the attribute fold:\n{js2}"
    );
    assert!(parses_as_js(&js2), "module must be valid JS:\n{js2}");
}

#[test]
fn svelte_element_invalid_binds_fail_closed() {
    // §1.8 element negatives — a bind valid only on ANOTHER host (`bind:value` on input/
    // textarea/select, `bind:devicePixelRatio` on `<svelte:window>`) used on the generic
    // dynamic-element host is an official `bind_invalid_target`; Verter fails it closed at the
    // HOST-SCOPED bind contract (the bind never emits; the exact code is D-29).
    for (src, target) in [
        (
            "<script>let tag = $state('div');let v = $state(0);</script>\n<svelte:element this={tag} bind:value={v} />\n",
            "value",
        ),
        (
            "<script>let tag = $state('div');let d = $state(0);</script>\n<svelte:element this={tag} bind:devicePixelRatio={d} />\n",
            "devicePixelRatio",
        ),
    ] {
        assert_fail_closed(
            src,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target: t, .. } if t == target),
        );
    }
}

#[test]
fn props_bindable_default_lowers_prop_source_with_context_frame() {
    // INVERTED (was `props_bindable_fails_closed`, which pinned the now-SUPPORTED
    // valid position): a read-only `$bindable(0)` default in a `$props()`
    // destructure is the flag-11 prop source (`IMMUTABLE|RUNES|BINDABLE`), read via
    // the getter, with the component context frame (`$.push($$props, true)` /
    // `$.pop()`) the `$bindable` call forces. Verified against svelte@5.56.10.
    let src = "<script>let { value = $bindable(0) } = $props();</script>\n<p>{value}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let value = $.prop($$props, 'value', 11, 0);"),
        "a read-only bindable default is the flag-11 prop source:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, value())"),
        "a bindable prop-source read is the getter call:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);") && js.contains("$.pop();"),
        "a bindable component opens the context frame:\n{js}"
    );
    // NEGATIVE: never the direct member read, and a read-only bindable never
    // emits an update helper.
    assert!(
        !js.contains("$$props.value"),
        "a bindable prop source never reads $$props directly:\n{js}"
    );
    assert!(
        !js.contains("$.update_prop"),
        "a read-only bindable emits no update helper:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn unmatched_selectors_still_compile_and_prune_unused() {
    // A style whose selector matches NO template element compiles: nothing is
    // scoped in the skeleton, and the unused rule is comment-pruned in the
    // artifact (never silently dropped, never unscoped output).
    let module = module_result(
        "<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("an unmatched selector still compiles");
    assert!(
        !module.code.contains("svelte-n50uah"),
        "no element is scoped, so the skeleton carries no hash:\n{}",
        module.code
    );
    let css = module.css.as_ref().expect("the artifact still publishes");
    assert!(
        css.code.contains("/* (unused) .r{color:red}*/"),
        "the unused rule is comment-pruned: {}",
        css.code
    );
}

#[test]
fn a_shorthand_attribute_binding_carries_its_authored_name_provenance() {
    // `<button {disabled}>` lowers to `button.disabled = <read>`; the generated
    // read must map back to the AUTHORED `disabled` inside the shorthand braces.
    let source = "<script>\nlet { disabled = false } = $props();\n</script>\n<button {disabled}>go</button>\n";
    let (code, map) = compile_with_map(source, "Btn.svelte");
    let write = code
        .find(".disabled = ")
        .expect("the shorthand attribute lowers to a property write");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &code,
        write + ".disabled = ".len(),
        source
            .find("{disabled}>")
            .expect("the authored shorthand binding")
            + "{".len(),
    );
}

#[test]
fn style_selector_refusal_carries_the_construct_span_and_selector_code() {
    // Code/span propagation (matcher refusal): the selector-unprovable
    // refusal reports the UNPROVABLE CONSTRUCT's exact span (the
    // `<svelte:head>` open tag whose `<title>` is decomposed out of the
    // runtime IR) with the fixed selector-surface code. NEGATIVE: never the
    // whole `<style>` content span (the pre-F4 secondary-check span).
    let source =
        "<svelte:head><title>t</title></svelte:head>\n<div>x</div>\n<style>div { color: red; }</style>\n";
    let err = emit_result(source).expect_err("an unprovable template must not compile");
    let ClientCompileError::Unsupported(surface) = err else {
        panic!("expected the selector refusal, got {err:?}");
    };
    assert!(
        matches!(
            surface,
            UnsupportedSvelteRuntimeSurface::StyleSelectorUnsupported { .. }
        ),
        "the matcher refusal lands on the selector surface: {surface:?}"
    );
    assert_eq!(
        surface.diagnostic_code(),
        "svelte-runtime-unsupported-style-selector"
    );
    let head = source.find("<svelte:head>").unwrap() as u32;
    assert_eq!(
        surface.span(),
        verter_span::Span::new(head, head + "<svelte:head>".len() as u32),
        "the refusal span is the unprovable construct's own span"
    );
}

#[test]
fn regular_element_bind_focused_emits_unproxied_setter() {
    // F12: a regular-element `bind:focused` emits `$.bind_focused(el, ($$value) => $.set(x,
    // $$value))` — NO proxy 3rd arg (the proxy is HOST-driven and false on a regular element,
    // true only on `<svelte:window>`). The positive regular-element shape for the flipped
    // `focused` support.
    let js = emit(
        "<script>\n\tlet focused = $state(false);\n</script>\n\n<input bind:focused={focused} />\n",
        "bindings/bind_focused.svelte",
    );
    assert!(
        js.contains("$.bind_focused(input, ($$value) => $.set(focused, $$value))"),
        "regular bind:focused → unproxied setter:\n{js}"
    );
    // NEGATIVE: a regular element bind is NOT proxied (no `, true)` 3rd arg on the setter).
    assert!(
        !js.contains("$.set(focused, $$value, true)"),
        "a regular element bind:focused is NOT proxied:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn custom_element_host_member_read_binds_props_and_context() {
    // A DIRECT member read on the `$host()` call result (`$host().foo`) with no
    // other binder: official BINDS `$$props` AND opens the component context
    // (the member-on-call-result expression is not a "safe identifier", so
    // `needs_context` fires). Pinned `svelte@5.56.10` emits
    // `$.event('focus', button, () => $$props.$$host.foo);` under
    // `function App($$anchor, $$props)` + `$.push($$props, true)`.
    let js = emit_result(
        "<svelte:options customElement=\"x-mr\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host().foo}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a direct member read on the $host() result compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the member-accessed host binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the member-on-call-result opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$.event('focus', button, () => $$props.$$host.foo);"),
        "the member read lowers onto the rewritten host member:\n{js}"
    );
    assert!(
        js.contains("\t$.pop();\n"),
        "the no-props frame closes with the statement pop:\n{js}"
    );
    // NEGATIVE: no raw `$host` survives and no `$$exports` frame exists
    // without props.
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
    assert!(
        !js.contains("$$exports"),
        "no $$exports without props:\n{js}"
    );
}

#[test]
fn custom_element_host_optional_member_binds_props() {
    // The OPTIONAL member form on the call result (`$host()?.foo`) is a direct
    // member access too — official binds `$$props` and pushes the frame,
    // emitting `() => $$props.$$host?.foo`.
    let js = emit_result(
        "<svelte:options customElement=\"x-om\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host()?.foo}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("an optional member on the $host() result compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the optional-member host binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the member-on-call-result opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$.event('focus', button, () => $$props.$$host?.foo);"),
        "the optional member lowers onto the rewritten host member:\n{js}"
    );
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
}

#[test]
fn custom_element_host_computed_member_binds_props() {
    // The COMPUTED member form on the call result (`$host()['foo']`) is a
    // direct member access — official binds `$$props` and pushes the frame,
    // emitting `() => $$props.$$host['foo']`.
    let js = emit_result(
        "<svelte:options customElement=\"x-cm\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host()['foo']}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a computed member on the $host() result compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the computed-member host binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the member-on-call-result opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$.event('focus', button, () => $$props.$$host['foo']);"),
        "the computed member lowers onto the rewritten host member:\n{js}"
    );
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
}

#[test]
fn custom_element_host_method_call_binds_props() {
    // A METHOD call on the `$host()` result (`$host().focus()`) with NO `new`
    // expression anywhere: the member access itself is the trigger — official
    // binds `$$props` and pushes, emitting
    // `() => $$props.$$host.focus()` (pinned `svelte@5.56.10`).
    let js = emit_result(
        "<svelte:options customElement=\"x-mc\" />\n<script>let c = $state(0);</script>\n<button onfocus={() => $host().focus()}>hi</button>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a method call on the $host() result compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the method-called host binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the member-on-call-result opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$.event('focus', button, () => $$props.$$host.focus());"),
        "the method call lowers onto the rewritten host member:\n{js}"
    );
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
}

#[test]
fn custom_element_props_binder_admits_bare_host() {
    // A REAL props binder (`$props()` destructure) admits a BARE `$host()`:
    // official binds `$$props` (the binder is the trigger — the host use itself
    // stays bare) and the CE accessor frame rides `$$exports`.
    let js = emit_result(
        "<svelte:options customElement=\"x-pb\" />\n<script>let { label } = $props();</script>\n<button onfocus={() => $host()}>hi</button>\n<p>{label}</p>\n",
    )
    .expect("a real $props() binder admits a bare $host()");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the props binder binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.event('focus', button, () => $$props.$$host);"),
        "the bare host lowers to the bound host read:\n{js}"
    );
    assert!(
        js.contains("let label = $.prop($$props, 'label', 7);"),
        "the CE prop source rides the accessor-forced flags:\n{js}"
    );
    assert!(
        js.contains("return $.pop($$exports);"),
        "the accessor frame closes through $$exports:\n{js}"
    );
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
}

#[test]
fn custom_element_bindable_binder_admits_bare_host() {
    // A `$bindable(...)` member default is a REAL props binder too: a bare
    // `$host()` sibling is admitted (official binds `$$props`; the bindable
    // prop rides `$.prop($$props, 'v', 15, 0)` and the `$$exports` frame).
    let js = emit_result(
        "<svelte:options customElement=\"x-bb\" />\n<script>let { v = $bindable(0) } = $props();</script>\n<button onfocus={() => $host()}>hi</button>\n<p>{v}</p>\n",
    )
    .expect("a $bindable binder admits a bare $host()");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the bindable binder binds $$props:\n{js}"
    );
    assert!(
        js.contains("let v = $.prop($$props, 'v', 15, 0);"),
        "the bindable prop source carries the bindable flags:\n{js}"
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
fn custom_element_template_only_host_member_infers_runes_and_binds_props() {
    // MODE-INFERENCE completion: a customElement whose ONLY rune is a
    // TEMPLATE-expression `$host()` (NO script at all — nothing masks the
    // inference with a script rune). Official `svelte@5.56.10` treats the
    // template `$host` reference as a runes-mode indicator
    // (`metadata.runes === true`) and, because the host use is
    // member-accessed, binds `$$props` and pushes the frame:
    //
    //   export default function App($$anchor, $$props) {
    //       $.push($$props, true);
    //       var button = root();
    //       $.event('focus', button, () => $$props.$$host.dispatchEvent(new CustomEvent('boop')));
    //       $.append($$anchor, button);
    //       $.pop();
    //   }
    //
    // A script-source-only runes inference misreads this component as LEGACY
    // and over-refuses it with the legacy-mode surface — the WRONG reason.
    // (The handler rides the DIRECT `$.event` surface — `onfocus`, a
    // non-delegatable type — the same isolation every `$host` row uses.)
    let js = emit_result(
        "<svelte:options customElement=\"x-tmpl-host\" />\n\n<button onfocus={() => $host().dispatchEvent(new CustomEvent('boop'))}>go</button>\n",
    )
    .expect("a template-only $host() customElement infers runes and compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the member-accessed template host binds $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the member-on-call-result opens the context frame:\n{js}"
    );
    assert!(
        js.contains(
            "$.event('focus', button, () => $$props.$$host.dispatchEvent(new CustomEvent('boop')));"
        ),
        "the template host call lowers to the bound host member:\n{js}"
    );
    assert!(
        js.contains(
            "customElements.define('x-tmpl-host', $.create_custom_element(App, {}, [], [], { mode: 'open' }));"
        ),
        "the define epilogue rides the module tail:\n{js}"
    );
    // NEGATIVE: no raw `$host` survives, and the component is NOT the legacy
    // lowering (no `$.mutable_source` legacy substrate).
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn custom_element_host_in_instance_effect_binds_props() {
    // The INSTANCE-SCRIPT `$host()` position: a zero-arg call inside a
    // top-level `$effect(fn)` body. The `$host` usage FACT must come from the
    // instance script too (not only template expressions): `$$props` is bound,
    // and the effect body lowers the call to `$$props.$$host` (pinned
    // `svelte@5.56.10` emits `$.user_effect(() => { $$props.$$host.dispatchEvent(…); })`
    // with the `$$props` parameter present).
    let js = emit_result(
        "<svelte:options customElement=\"x-fx\" />\n<script>\n\tlet c = $state(0);\n\t$effect(() => { $host().dispatchEvent(new CustomEvent('tick')); });\n</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect("a $host() call inside an instance $effect body compiles under a customElement");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the instance-script $host usage forces the $$props parameter:\n{js}"
    );
    assert!(
        js.contains("$$props.$$host.dispatchEvent(new CustomEvent('tick'));"),
        "the effect-body $host() call lowers to $$props.$$host:\n{js}"
    );
    // NEGATIVE: no raw `$host` survives anywhere in the module.
    assert!(
        !js.replace("$$host", "").contains("$host"),
        "no raw $host in the module:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

// ─── `{@render}` DYNAMIC-callee `needs_context` (the peeled-callee scan) ───
//
// The official `needs_context` analysis excludes the OUTER snippet call of a
// `{@render}` dynamic callee from the "unsafe call" trigger (a prop-rooted
// `children?.()` stays frame-free), but the CALLEE expression inside it scans
// NORMALLY: a member/call/`new`-rooted callee (`$host().snip`, `imported.snip`,
// `(new Date())`) opens the component context frame exactly as the same
// expression would in a handler. Oracle-pinned first-hand against
// `svelte@5.56.10`: UNSAFE render callees emit `$.push($$props, true)` …
// `$.pop()` and bind the `$$props` parameter; SAFE callees (identifier /
// ternary-of-identifiers / member rooted at a local) stay frame-free. These
// e2e assertions live in THIS `*_tests.rs` sibling — not a scanner-pin module
// beside `rune_scan.rs` — because a full `compile_client` topology pin
// belongs beside the other client emission pins, and the sibling placement
// keeps the carrier-codegen CodeTransform text-scan guard scoped to
// production sources. Structural token checks over the built string only —
// never a post-hoc munge of it.

#[test]
fn render_dynamic_callee_host_member_frames_and_binds_props() {
    // `{@render $host().snip()}` under an active customElement: the peeled
    // callee `$host().snip` is a member rooted at a call result — never a
    // "safe identifier" — so `needs_context` fires: official
    // `svelte@5.56.10` binds `$$props` AND opens the component context frame
    // (`$.push($$props, true)` … `$.pop()`) around the snippet render.
    let js = emit_result("<svelte:options customElement=\"x-rc\" />\n{@render $host().snip()}\n")
        .expect("a render-dynamic-callee $host() member compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the $host() member in the render callee must bind $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the call-result-rooted render callee opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$.pop();"),
        "the no-props frame closes with the statement pop:\n{js}"
    );
    assert!(
        js.contains("$.snippet(node, () => $$props.$$host.snip);"),
        "the render callee rewrites through $$props.$$host:\n{js}"
    );
    // NEGATIVE: no raw `$host()` rune survives the rewrite — a STRUCTURAL
    // token check over the built string (never a post-hoc munge of it): a
    // leaked rune spells the bare CALL `$host(`, which the emitted
    // `$$props.$$host.snip` residue (asserted positively above) never does.
    assert!(
        !js.contains("$host("),
        "no raw $host() rune survives the rewrite (only the $$host residue):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn render_dynamic_callee_imported_member_frames_and_binds_props() {
    // A member callee rooted at an IMPORT (`{@render Snips.row()}` with
    // `import Snips from './Snips.svelte'`): an import-rooted member is not a
    // "safe identifier", so official binds `$$props` and opens the frame.
    let js = emit_result(
        "<script>\n\timport Snips from './Snips.svelte';\n\tlet __r = $state(0);\n</script>\n{@render Snips.row()}\n<button onclick={() => __r++}>{__r}</button>\n",
    )
    .expect("an imported-member render callee compiles");
    assert!(
        js.contains("export default function App($$anchor, $$props) {"),
        "the import-rooted render callee must bind $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the import-rooted render callee opens the context frame:\n{js}"
    );
    assert!(
        js.contains("$.snippet(node, () => Snips.row);"),
        "the member callee rides the $.snippet thunk verbatim:\n{js}"
    );
}

#[test]
fn import_dollar_prefixed_binding_rejects() {
    // `import $inspect from './x.svelte'; $inspect(c);` — a `$`-prefixed imported
    // LOCAL binding is official `dollar_prefix_invalid` ("The $ prefix is reserved,
    // and cannot be used for variables and imports"). RED before the fix: the
    // top-level declarator scan covered VariableDeclarations only, so the invalid
    // import slipped through to `$inspect`-elision (fail-open on invalid input —
    // an emitted Main for a source official compile-errors).
    let err = emit_result(
        "<script>import $inspect from './x.svelte'; let c = $state(0); $inspect(c);</script>\n<button onclick={() => c++}>{c}</button>\n",
    )
    .expect_err("a `$`-prefixed import local must reject");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected an OfficialReject(DollarPrefixInvalid), got {err:?}");
    };
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::DollarPrefixInvalid,
        "wrong official-reject rule: {rejection:?}"
    );
    assert_eq!(
        rejection.official_code, "dollar_prefix_invalid",
        "wrong official code: {rejection:?}"
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
        .join("../../packages/svelte-runtime-tests/test/fixtures/svelte/hello_input.client.mjs");
    let committed = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("read smoke fixture {}: {e}", fixture_path.display()));
    assert_eq!(
        normalize_js_cosmetics(&js),
        normalize_js_cosmetics(&committed),
        "the §1.2 emitted module diverged STRUCTURALLY from the committed jsdom-smoke \
         fixture; regenerate packages/svelte-runtime-tests/test/fixtures/svelte/hello_input.client.mjs \
         from `compile_client` and re-run oxfmt"
    );
}

// ── DOM-hosted bind behavioral fixtures ────────────────────────────────────────
//
// Each fixture below mounts the EMITTED §1.2 module (its `.client.mjs`, kept in
// lockstep by these tests) against the REAL pinned `svelte@5.56.10` runtime in the
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
fn bind_input_value_module_matches_the_committed_jsdom_smoke_fixture() {
    // A plain `<input bind:value={v}>` with a NON-EMPTY initial signal plus a
    // state-driving `clear` button — the fixture behind the update/cleanup smoke arm
    // (initial value, typing, repeated typing, state→DOM to the EMPTY string, and
    // unmount/remount freshness). `$.remove_input_defaults` prelude +
    // `$.bind_value(input, get, set)` + the delegated `$.set(v, "")` click.
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet v = $state(\"init\");\n</script>\n<input bind:value={v} />\n<p>{v}</p>\n<button onclick={() => v = \"\"}>clear</button>\n",
        "bind_input_value.client.mjs",
    );
}

#[test]
fn bind_select_value_channel_module_matches_the_committed_jsdom_smoke_fixture() {
    // `<select bind:value>` whose options carry STATIC `value` attributes (including
    // the EMPTY-string `value=""`) — the OPTION VALUE-CHANNEL: the skeleton bakes
    // BARE `<option>`s and the emitter writes the init-only
    // `option.value = option.__value = 'X'` at each option's walk position, closing
    // the select region with `$.reset(select)`. The `clear` button drives the
    // state→DOM arm (back to the empty-string option). Verified structurally
    // identical to pinned-official svelte@5.56.10 output at authoring.
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet v = $state(\"a\");\n</script>\n<select bind:value={v}><option value=\"\">none</option><option value=\"a\">a</option><option value=\"b\">b</option></select>\n<p>{v}</p>\n<button onclick={() => v = \"\"}>clear</button>\n",
        "bind_select_value_channel.client.mjs",
    );
}

#[test]
fn bind_checked_update_module_matches_the_committed_jsdom_smoke_fixture() {
    // `<input type="checkbox" bind:checked>` plus a state-driving `toggle` button —
    // the fixture behind the checked update/cleanup smoke arm (user toggles, repeated
    // toggles, state→DOM to FALSE, unmount/remount freshness).
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet c = $state(false);\n</script>\n<input type=\"checkbox\" bind:checked={c} />\n<p>{c}</p>\n<button onclick={() => c = !c}>toggle</button>\n",
        "bind_checked_update.client.mjs",
    );
}

#[test]
fn bind_pair_setter_module_matches_the_committed_jsdom_smoke_fixture() {
    // A function-pair `bind:value` whose setter BLOCK forwards to a PROP callback
    // before writing the signal — `$.bind_value(input, get, (next) => {
    // $$props.onSet(next); $.set(v, next, true); })`. The injected `onSet` prop is
    // the smoke's OBSERVABLE setter tap: exactly one call per input event (the
    // duplicate-listener control) and ZERO calls after unmount (the stale-
    // subscription control). Verified structurally identical to pinned-official
    // svelte@5.56.10 output at authoring.
    assert_jsdom_fixture_in_sync(
        "<script>\n\tlet { onSet } = $props();\n\tlet v = $state(\"\");\n</script>\n<input bind:value={() => v, (next) => { onSet(next); v = next; }} />\n<p>{v}</p>\n",
        "bind_pair_setter.client.mjs",
    );
}

#[test]
fn parenthesized_identifier_bind_value_binds_typed_signal_root() {
    // F6: `bind:value={(v)}` — author parens around a SINGLE identifier (NOT a sequence).
    // Official svelte@5.56.10 ACCEPTS it and binds on the identifier ROOT `v`, IDENTICALLY
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
    // F4: a DYNAMIC `value={label}` on a reactive `bind:group` radio. Official svelte@5.56.10
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
fn lang_ts_component_with_bind_targets_erases_wrappers() {
    // The TypeScript grammar canonicalizes clean, non-null, `as`, and wrapped
    // member bind targets to their runtime lvalues before emission.
    let cases = [
        ("name", "let name = $state(\"\");", "<input bind:value={name} />\n<p>{name}</p>"),
        ("name!", "let name = $state(\"\");", "<input bind:value={name!} />\n<p>{name}</p>"),
        ("name as string", "let name = $state(\"\");", "<input bind:value={name as string} />\n<p>{name}</p>"),
        ("model!.value", "let model = $state({ value: '' });", "<input bind:value={model!.value} />\n<button onclick={() => model = { value: 'x' }}>x</button>"),
    ];
    for (target, declaration, markup) in cases {
        let src = format!("<script lang=\"ts\">{declaration}</script>\n{markup}\n");
        let js = emit(&src, "App.svelte");
        assert!(
            !js.contains("name!") && !js.contains(" as string") && !js.contains("model!."),
            "TS wrapper leaked for `{target}`:\n{js}"
        );
        assert!(
            js.contains("$.bind_value("),
            "bind:value missing for `{target}`:\n{js}"
        );
        assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
    }
}

#[test]
fn ts_wrapped_dom_bind_target_in_plain_script_fails_closed() {
    // E (lvalue-widening boundary): a TS-WRAPPED DOM-bind target (`bind:value={v!}` /
    // `{v as string}`) on an ordinary DOM host stays CLOSED — the canonical-lvalue-
    // from-TS strip is a deferral (owned by the future `lang="ts"`-script block, NOT
    // the DOM-bind backend). Oracle determination (svelte@5.56.10): official PARSE-REJECTS this exact
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
    // NEGATIVE: the clean (unwrapped) form on the SAME host is the supported shape —
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
    // (`a[x!]`) — FAILS CLOSED. Official svelte@5.56.10 PARSE-REJECTS each in a plain
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
    // divergent index `arr[g(c)]` (a function call). Official svelte@5.56.10 instead parses the
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
    // divergent index (`arr[g<a,b>(c)]` -> `arr[g(c)]`), whereas official svelte@5.56.10 parses
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
fn name_host_attr_invalid_intrinsic_binds_fail_closed_via_unsupported_channel() {
    // The four name/host/host-attr-invalid intrinsic binds whose TARGET is also shape-invalid
    // must fail closed via the UNSUPPORTED channel (`Binding`) — NOT a confidently-WRONG
    // `OfficialReject` shape code. Official svelte@5.56.10 reports a name/host/host-attr error
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

#[test]
fn static_textarea_content_fails_closed_at_the_special_content_model_gate() {
    // `<textarea>` IS an allowed DOM-bind `bind:value` host (it passes the element
    // allowlist), so the refusal is the SPECIAL CONTENT-MODEL gate, NOT the element
    // allowlist. Even STATIC-only `<textarea>hi</textarea>` content is the official
    // raw-text `textarea` content model the DOM-bind backend does NOT own (it emits `<textarea>` ONLY as
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
    // A `<textarea bind:value={v}>fallback</textarea>` — a `bind:value` host with a
    // STATIC-TEXT fallback child — IS a supported surface: the existing
    // `$.remove_textarea_child` prelude clears the baked static child at runtime, so the
    // bind is unaffected. Verified against svelte@5.56.10 (the static text is baked into
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
    // CONTENT channel distinct from the static-fallback child (which the DOM-bind backend clears via
    // `remove_textarea_child`). The static-text relaxation must NOT leak into the dynamic
    // content surface, which is owned by a later content-model layer (ledger D-22). RED
    // would be a broadened "allow any textarea child" admission.
    assert_fail_closed(
        "<script>let v = $state(\"\"); let c = $state(\"hi\");</script>\n<textarea bind:value={v}>{c}</textarea>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Element { tag, .. } if tag == "textarea"),
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
fn state_mixed_raw_and_proxy_bindings_are_per_binding_correct() {
    // Per-binding decision: a raw and a proxied object `$state` COEXIST in one
    // component — the raw one gets no proxy + no flag; the proxied one gets both.
    let js = emit(
        "<script>let r = $state.raw({ a: 1 });\nlet p = $state({ b: 2 });</script>\n<button onclick={() => { r = { a: 9 }; p = { b: 9 }; }}>x</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let r = $.state({ a: 1 });"),
        "the raw binding is a bare `$.state(<init>)`:\n{js}"
    );
    assert!(
        js.contains("let p = $.state($.proxy({ b: 2 }));"),
        "the proxied binding is `$.state($.proxy(<init>))`:\n{js}"
    );
    assert!(
        js.contains("$.set(r, { a: 9 })") && !js.contains("$.set(r, { a: 9 }, true)"),
        "the raw reassign carries NO flag:\n{js}"
    );
    assert!(
        js.contains("$.set(p, { b: 9 }, true)"),
        "the proxied reassign carries the flag:\n{js}"
    );
}

#[test]
fn shadowed_effect_binding_is_not_the_rune() {
    // A function-pair-bind function PARAM named `$effect` shadows the rune in an
    // ACCEPTED context (the `FunctionDecl` carrier lowers the body through the
    // shared rewriter). Official ACCEPTS the component (oracle-verified): the
    // local call is emitted RAW (`$effect(() => {})`), NEVER rewritten to
    // `$.user_effect`, and the shadowed call does NOT force the component frame
    // (sig `($$anchor)`, no `$.push`). This is the discriminating shadow
    // observable: if shadow handling broke (the rewriter or the `needs_context`
    // scan treated the local as the rune), the emission would flip to
    // `$.user_effect` and mint the frame.
    let js = emit(
        "<script>\n\tlet v = $state('');\n\tfunction get($effect) { $effect(() => {}); return v; }\n\tfunction set(x) { v = x; }\n</script>\n<input bind:value={get, set} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$effect(() => {});"),
        "the shadowed local call is emitted RAW:\n{js}"
    );
    assert!(
        !js.contains("$.user_effect"),
        "the shadowed call is NOT rewritten to `$.user_effect`:\n{js}"
    );
    assert!(
        js.contains("export default function App($$anchor) {") && !js.contains("$.push"),
        "the shadowed call does NOT force the component frame:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");

    // The instance-script variant: a function PARAM named `$effect` inside a
    // top-level `function` declaration — its calls are ordinary local calls, so
    // the rune gate does NOT fire; the component fails closed at the
    // instance-script-item gate (construct `function`), NOT on a rune basis
    // (the same precedence the `$inspect` shadowing test pins).
    let js = emit(
        "<script>\n\tlet c = $state(0);\n\tfunction f($effect) { $effect.pre(1); }\n</script>\n<button onclick={() => c++}>{c}</button>\n",
        "App.svelte",
    );
    assert!(
        js.contains("function f($effect) { $effect.pre(1); }"),
        "shadowed effect-like call must remain ordinary JavaScript:\n{js}"
    );
    assert!(
        !js.contains("$.user_pre_effect"),
        "shadowed call was rune-lowered:\n{js}"
    );
    assert!(parses_as_js(&js), "emitted module must parse as JS:\n{js}");
}

// ── $bindable — the `$.prop` substrate + fail-closed siblings ─────────────────

#[test]
fn props_bindable_assigned_lowers_flag_15_setter_call() {
    // A reassigned bindable (`v = 9`) is the flag-15 prop source
    // (`IMMUTABLE|RUNES|UPDATED|BINDABLE`) and the write is the SETTER CALL
    // `v(9)`. Verified against svelte@5.56.10.
    let src = "<script>let { v = $bindable(0) } = $props();</script>\n<button onclick={() => v = 9}>{v}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 15, 0);"),
        "a reassigned bindable is the flag-15 prop source:\n{js}"
    );
    assert!(js.contains("v(9)"), "the write is the setter call:\n{js}");
    assert!(
        js.contains("$.push($$props, true);") && js.contains("$.pop();"),
        "a bindable component opens the context frame:\n{js}"
    );
    // NEGATIVE: an identifier reassignment never carries the mutation `true`.
    assert!(
        !js.contains("v(9, true)"),
        "an identifier reassign has no mutation flag:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_update_lowers_update_prop_helper() {
    // `v++` on a bindable prop source lowers to `$.update_prop(v)` (the prop
    // update helper, not `$.update`). Verified against svelte@5.56.10.
    let src = "<script>let { v = $bindable(0) } = $props();</script>\n<button onclick={() => v++}>{v}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 15, 0);"),
        "an updated bindable is the flag-15 prop source:\n{js}"
    );
    assert!(
        js.contains("$.update_prop(v)"),
        "the postfix update is the prop update helper:\n{js}"
    );
    // NEGATIVE: never the signal update helper.
    assert!(
        !js.contains("$.update(v)"),
        "a prop update never uses the signal `$.update`:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_lazy_object_default_readonly_lowers_flag_27_proxy_thunk() {
    // A read-only bindable OBJECT default is the LAZY flag-27 proxy thunk
    // (`() => $.proxy({ a: 1 })`) — the `$.proxy` wrap is BINDABLE-only.
    // Verified against svelte@5.56.10.
    let src = "<script>let { v = $bindable({ a: 1 }) } = $props();</script>\n<p>{v}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 27, () => $.proxy({ a: 1 }));"),
        "a bindable object default is the lazy flag-27 proxy thunk:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, v())"),
        "reads go through the getter:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_readonly_no_default_emits_no_prop_source() {
    // `is_prop_source` — a read-only bindable with NO default is NOT a prop
    // source: NO `$.prop` anywhere; reads go DIRECT `$$props.value`; the context
    // frame still opens (the `$bindable` call forces it). Verified against
    // svelte@5.56.10.
    let src = "<script>let { value = $bindable() } = $props();</script>\n<p>{value}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        !js.contains("$.prop("),
        "a read-only no-default bindable emits NO $.prop:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, $$props.value)"),
        "reads go direct off $$props:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);") && js.contains("$.pop();"),
        "the `$bindable` call still forces the context frame:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_no_default_assigned_lowers_flag_15_without_default_arg() {
    // A no-default bindable that IS reassigned becomes a prop source with NO
    // default argument: `$.prop($$props, 'v', 15)`. Verified against
    // svelte@5.56.10.
    let src = "<script>let { v = $bindable() } = $props();</script>\n<button onclick={() => v = 1}>{v}</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 15);"),
        "a reassigned no-default bindable is `$.prop($$props, 'v', 15)`:\n{js}"
    );
    assert!(js.contains("v(1)"), "the write is the setter call:\n{js}");
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_aliased_uses_source_key() {
    // `{ value: local = $bindable(0) }` keys the prop by its SOURCE key
    // (`'value'`) bound as the LOCAL (`local`). Verified against svelte@5.56.10.
    let src = "<script>let { value: local = $bindable(0) } = $props();</script>\n<p>{local}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let local = $.prop($$props, 'value', 11, 0);"),
        "the prop keys by the SOURCE key, bound as the local:\n{js}"
    );
    assert!(
        js.contains("$.set_text(text, local())"),
        "reads use the local getter:\n{js}"
    );
    // NEGATIVE: never the local name as the prop key.
    assert!(
        !js.contains("'local'"),
        "the local alias is never the prop key:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_ident_default_proxies_lazily() {
    // A bindable IDENTIFIER default that does not resolve to a known
    // non-proxiable initializer proxies lazily (`() => $.proxy(base)`, flag 27) —
    // the official scope-follow defaults to proxiable. Verified against
    // svelte@5.56.10.
    let src = "<script>let { v = $bindable(base) } = $props();</script>\n<p>{v}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 27, () => $.proxy(base));"),
        "an unresolvable identifier default proxies lazily:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn bindable_forces_context_frame_props_id_does_not() {
    // The `needs_context` policy trigger discriminator: a `$bindable(...)` call
    // forces `$.push($$props, true)` / `$.pop()` — even with NO `$.prop` emission
    // (read-only no-default) — while `$props.id()` alone never does.
    let bindable = emit(
        "<script>let { value = $bindable() } = $props();</script>\n<p>{value}</p>\n",
        "App.svelte",
    );
    assert!(
        bindable.contains("$.push($$props, true);") && bindable.contains("$.pop();"),
        "a bindable component opens the frame:\n{bindable}"
    );
    let id_only = emit(
        "<script>const uid = $props.id();</script>\n<p>{uid}</p>\n",
        "App.svelte",
    );
    assert!(
        !id_only.contains("$.push") && !id_only.contains("$.pop"),
        "a `$props.id()`-only component opens NO frame:\n{id_only}"
    );
}

#[test]
fn bindable_two_arguments_fails_closed() {
    // `$bindable(1, 2)` — official `rune_invalid_arguments_length` (zero or one),
    // even in the valid default position.
    assert_fail_closed(
        "<script>let { value = $bindable(1, 2) } = $props();</script>\n<p>{value}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_spread_argument_fails_closed() {
    // `$bindable(...a)` — official `rune_invalid_spread`.
    assert_fail_closed(
        "<script>let { value = $bindable(...[1]) } = $props();</script>\n<p>{value}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_standalone_statement_fails_closed() {
    // A standalone `$bindable(0);` statement — official
    // `bindable_invalid_location`.
    assert_fail_closed(
        "<script>$bindable(0);</script>\n<p>x</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_standalone_declarator_fails_closed() {
    // A standalone `const x = $bindable(0);` declarator — official
    // `bindable_invalid_location` (only a `$props()` destructure default).
    assert_fail_closed(
        "<script>const x = $bindable(0);</script>\n<p>{x}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_call_argument_fails_closed() {
    // `foo($bindable(0))` — official `bindable_invalid_location`.
    assert_fail_closed(
        "<script>foo($bindable(0));</script>\n<p>x</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_bare_reference_standalone_fails_closed() {
    // An UNCALLED bare `$bindable` reference (`const x = $bindable;`) — official
    // `rune_missing_parentheses`. Locks the reference-position fail-open closed.
    assert_fail_closed(
        "<script>const x = $bindable;</script>\n<p>{x}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_bare_reference_in_effect_body_fails_closed() {
    // The uncalled-`$bindable` FAIL-OPEN pin: a bare `$bindable` inside a lowered
    // `$effect` body previously slipped every arm and emitted RAW (a runtime
    // `ReferenceError`). It must fail closed.
    assert_fail_closed(
        "<script>let c = $state(0); $effect(() => { const x = $bindable; console.log(x, c); });</script>\n<p>{c}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_uncalled_in_default_fails_closed() {
    // `{ value = $bindable }` (uncalled, in the default position) — official
    // `rune_missing_parentheses`. Must be refused explicitly, never lowered as a
    // literal default value.
    assert_fail_closed(
        "<script>let { value = $bindable } = $props();</script>\n<p>{value}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn bindable_nested_destructure_default_fails_closed() {
    // `{ a: { b = $bindable(0) } }` — official `props_invalid_pattern` (nested
    // properties); the nested-destructure gate owns the refusal.
    assert_fail_closed(
        "<script>let { a: { b = $bindable(0) } } = $props();</script>\n<p>{b}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$props() nested destructure"),
    );
}

#[test]
fn bindable_parenthesized_spellings_fail_closed() {
    // Verter locks the parenthesized `$bindable` spellings fail-closed (the
    // strict `$bindable(...)` default shape is the sole accepted spelling).
    for (label, src) in [
        (
            "paren-callee",
            "<script>let { v = ($bindable)(0) } = $props();</script>\n<p>{v}</p>\n",
        ),
        (
            "paren-call",
            "<script>let { v = ($bindable(0)) } = $props();</script>\n<p>{v}</p>\n",
        ),
    ] {
        assert_fail_closed_labeled(
            label,
            src,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
        );
    }
}

#[test]
fn bindable_optional_call_fails_closed() {
    // `$bindable?.(0)` — official `bindable_invalid_location`; the optional
    // spelling is locked fail-closed.
    assert_fail_closed(
        "<script>let { v = $bindable?.(0) } = $props();</script>\n<p>{v}</p>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$bindable"),
    );
}

#[test]
fn props_bindable_object_default_nested_member_update_sets_updated_flag_31() {
    // A nested member update of a BINDABLE prop sets UPDATED on top of
    // BINDABLE | LAZY (flags 31) and keeps the bindable setter-with-mutation
    // wrap `v(v().b.c++, true)`. Verified against svelte@5.56.10.
    let src = "<script>let { v = $bindable({ b: { c: 0 } }) } = $props();</script>\n<button onclick={() => v.b.c++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 31, () => $.proxy({ b: { c: 0 } }));"),
        "a nested-mutated bindable is flags 31 with the proxied thunk:\n{js}"
    );
    assert!(
        js.contains("v(v().b.c++, true)"),
        "the bindable nested mutation rides the setter mutation wrap:\n{js}"
    );
    // NEGATIVE: the un-updated flag value must be gone.
    assert!(
        !js.contains("'v', 27,"),
        "the nested write must not drop UPDATED to flags 27:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_sequence_default_proxy_parenthesizes_sequence_argument() {
    // A BINDABLE sequence default (`$bindable((1, { x: 1 }))`) parenthesizes
    // the sequence inside the proxy call — `$.proxy((1, { x: 1 }))` — keeping
    // `$.proxy` a ONE-argument call. Verified against svelte@5.56.10.
    let src = "<script>let { v = $bindable((1, { x: 1 })) } = $props();</script>\n<p>{v}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 27, () => $.proxy((1, { x: 1 })));"),
        "a bindable sequence default proxies the parenthesized sequence:\n{js}"
    );
    // NEGATIVE: the bare embedding would make the sequence a 2-arg `$.proxy`.
    assert!(
        !js.contains("$.proxy(1,"),
        "the sequence must never split into proxy arguments:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bind_value_to_bindable_prop_stays_fail_closed() {
    // SURVIVING refusal: `bind:value={bindableProp}` (official's 2-arg
    // `$.bind_value(input, value)` form) stays fail-closed — the prop-bind gate is
    // out of this substrate's scope.
    assert_fail_closed(
        "<script>let { value = $bindable(0) } = $props();</script>\n<input bind:value={value} />\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
    );
}

// ── TS-only syntax in a prop-rooted template WRITE chain — fail-closed ─────────
//
// Official svelte@5.56.10 PARSE-REJECTS TypeScript syntax in a plain-`<script>`
// component's template (`expected_token`), so a template write whose member
// chain carries a TS-only wrapper (`v!.a++` / `(v as any).a++`) must never
// emit. Verter's TypeScript stripper can lower the same structural target for a
// `lang="ts"` component, so the write-target classifier uses the authoritative
// script grammar: plain-script targets fail closed, TypeScript targets lower.

#[test]
fn props_bindable_ts_nonnull_member_update_fails_closed() {
    // `v!.a++` on a bindable prop — the TS non-null wrapper sits between the
    // write target and its prop root. Official parse-rejects the template
    // expression outright; Verter fails the TS-wrapped reactive write closed.
    assert_fail_closed(
        "<script>let { v = $bindable({ a: 0 }) } = $props();</script>\n<button onclick={() => v!.a++}>{v}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_bindable_ts_nonnull_member_assignment_fails_closed() {
    // `v!.a = 1` — the ASSIGNMENT form of the same TS-wrapped bindable member
    // write shares the fail-closed classification.
    assert_fail_closed(
        "<script>let { v = $bindable({ a: 0 }) } = $props();</script>\n<button onclick={() => v!.a = 1}>{v}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_bindable_ts_nonnull_mid_chain_member_update_fails_closed() {
    // `v.a!.b++` — the TS wrapper sits MID-chain (not on the root identifier);
    // the chain walk still sees it and fails closed.
    assert_fail_closed(
        "<script>let { v = $bindable({ a: { b: 0 } }) } = $props();</script>\n<button onclick={() => v.a!.b++}>{v}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_bindable_ts_as_cast_member_update_fails_closed() {
    // `(v as any).a++` — an `as`-cast in the target chain, on the DIRECT
    // (`$.event`) handler lane (`onfocus` is non-delegated, so the body reaches
    // the shared rewriter rather than the narrow delegated shape gate).
    assert_fail_closed(
        "<script>let { v = $bindable({ a: 0 }) } = $props();</script>\n<button onfocus={() => (v as any).a++}>{v}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

// ── TS-only syntax in a prop-rooted write target's COMPUTED KEY — fail-closed ──
//
// The fail-closed classification inspects the WHOLE member lvalue, not just the
// object spine: a COMPUTED KEY anywhere along a prop-rooted write target
// (`v[k as any]++` / `v[k!].b++`) carries the same officially-parse-rejected
// TypeScript syntax (svelte@5.56.10 `expected_token` in a plain-`<script>`
// component's template), and accepting it would strip the wrapper and emit a
// write official never compiles. The key inspection is RECURSIVE — a TS node
// nested at any depth inside the key (`v[f(k as any)]++`) fails closed too —
// through the same single member-write funnel, never a per-lane check.

#[test]
fn props_bindable_computed_key_ts_as_update_fails_closed() {
    // `v[k as any]++` — the `as`-cast rides the OUTERMOST computed KEY of a
    // bindable-rooted update target (the key is not part of the object spine).
    assert_fail_closed(
        "<script>let { v = $bindable({ a: 0 }), k = 'a' } = $props();</script>\n<button onclick={() => v[k as any]++}>{v}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_bindable_computed_key_ts_as_assignment_fails_closed() {
    // `v[k as any] = 1` — the ASSIGNMENT form of the same TS-keyed bindable
    // write shares the fail-closed classification (one funnel, both target
    // classifiers).
    assert_fail_closed(
        "<script>let { v = $bindable({ a: 0 }), k = 'a' } = $props();</script>\n<button onclick={() => v[k as any] = 1}>{v}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_bindable_computed_key_nested_ts_as_update_fails_closed() {
    // `v[String(k as any)]++` — the TS node sits DEEP inside the key (a call
    // argument), not at the key's top level; a shallow top-level-only key check
    // would leave the same hole one level down.
    assert_fail_closed(
        "<script>let { v = $bindable({ a: 0 }), k = 'a' } = $props();</script>\n<button onclick={() => v[String(k as any)]++}>{v}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_bindable_spine_computed_key_ts_nonnull_update_fails_closed() {
    // `v[k!].b++` — the TS-keyed computed hop sits on the object SPINE (the
    // chain walk reaches it while descending to the root), not on the write
    // target's outermost hop.
    assert_fail_closed(
        "<script>let { v = $bindable({ a: { b: 0 } }), k = 'a' } = $props();</script>\n<button onclick={() => v[k!].b++}>{v}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_bindable_plain_computed_key_update_keeps_key_read() {
    // CONTROL: a PLAIN computed key on a bindable-rooted update (`v[k]++`) stays
    // accepted — the mutation wraps in the setter with the mutation flag
    // (`v(v()[k()]++, true)`) and the key stays a getter READ (flags 3) — the
    // TS-key gate never widens onto TS-free computed keys.
    let src = "<script>let { v = $bindable({ a: 0 }), k = 'a' } = $props();</script>\n<button onclick={() => v[k]++}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("v(v()[k()]++, true)"),
        "a TS-free computed-key bindable mutation stays the setter wrap with the key a getter READ:\n{js}"
    );
    assert!(
        js.contains("$.prop($$props, 'k', 3, 'a')"),
        "the computed key stays a read-only prop (flags 3):\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

// ── Prop-rooted PRIVATE-FIELD member writes set UPDATED ────────────────────────
//
// A `#private`-field member write whose target roots at a `$props()` prop
// (`v.#x++` / `v.#x = 1` inside a template-handler class body — the only
// position where a private name is in scope) is the SAME deep mutation of the
// root binding as a static/computed member write: official svelte@5.56.10 sets
// the UPDATED flag bit and applies the bindable setter-with-mutation wrap
// (`v(v().#x++, true)`) identically to `v.x++`. The reference collector must
// classify the private-field target arm through the same root-walked
// deep-mutate fact as the static/computed arms — the write-ACCEPT classifiers
// already admit the target shape, so a collector miss under-flags the prop
// (27-instead-of-31 / 19-instead-of-23) rather than refusing. The handlers
// ride the DIRECT (`$.event`) lane (`onfocus` is non-delegated) — a class
// declaration statement is outside the narrow delegated body-shape subset, so
// the non-delegated lane is where these bodies reach the shared rewriter.

#[test]
fn props_bindable_private_field_update_sets_updated_flag_31() {
    // `v.#x++` through a bindable prop, inside a handler-declared class body.
    // Official: flags 31 (IMMUTABLE|RUNES|UPDATED|BINDABLE|LAZY), proxied
    // thunk, and the setter mutation wrap. Verified against svelte@5.56.10.
    let src = "<script>let { v = $bindable({}) } = $props();</script>\n<button onfocus={() => { class C { static #x = 0; static m() { v.#x++; } } C.m(); }}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 31, () => $.proxy({}));"),
        "a private-field-mutated bindable is flags 31 with the proxied thunk:\n{js}"
    );
    assert!(
        js.contains("v(v().#x++, true)"),
        "the bindable private-field mutation rides the setter mutation wrap:\n{js}"
    );
    // NEGATIVE: the un-updated flag value and the non-source read path must be gone.
    assert!(
        !js.contains("'v', 27,"),
        "the private-field write must not drop UPDATED to flags 27:\n{js}"
    );
    assert!(
        !js.contains("$$props.v"),
        "the mutated bindable must read through the prop source, not $$props.v:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_private_field_assignment_sets_updated_flag_31() {
    // The ASSIGNMENT form (`v.#x = 1`) is the same deep mutation of the root
    // prop as the update form: flags 31 and the wrap `v(v().#x = 1, true)`.
    // Verified against svelte@5.56.10.
    let src = "<script>let { v = $bindable({}) } = $props();</script>\n<button onfocus={() => { class C { static #x = 0; static m() { v.#x = 1; } } C.m(); }}>x</button>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 31, () => $.proxy({}));"),
        "a private-field-assigned bindable is flags 31 with the proxied thunk:\n{js}"
    );
    assert!(
        js.contains("v(v().#x = 1, true)"),
        "the bindable private-field assignment rides the setter mutation wrap:\n{js}"
    );
    // NEGATIVE: the un-updated flag value must be gone.
    assert!(
        !js.contains("'v', 27,"),
        "the private-field assignment must not drop UPDATED to flags 27:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_ts_nonnull_private_field_update_fails_closed() {
    // CONTROL: `v!.#x++` — a TS non-null wrapper on the private-field target's
    // OBJECT spine, rooting at a bindable prop. Official svelte@5.56.10
    // parse-rejects the TS syntax in a plain-`<script>` component's template
    // (`js_parse_error`); the shared member-write funnel keeps the TS-wrapped
    // reactive write fail-closed — the private-field arm must not widen it.
    assert_fail_closed(
        "<script>let { v = $bindable({}) } = $props();</script>\n<button onfocus={() => { class C { static #x = 0; static m() { v!.#x++; } } C.m(); }}>x</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::DestructuringWrite { .. }),
    );
}

#[test]
fn props_bindable_self_assign_default_sets_updated_flag_31() {
    // `let { a = $bindable(a = 1) }` — the self-assign inside the bindable
    // default sets UPDATED on top of BINDABLE | LAZY: flags 31 (3 | 4 | 8 |
    // 16), with the proxied setter rewrite; `$bindable` itself forces the
    // context frame. Verified against svelte@5.56.10.
    let src = "<script>let { a = $bindable(a = 1) } = $props();</script>\n<p>{a}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let a = $.prop($$props, 'a', 31, () => $.proxy(a(1)));"),
        "a bindable self-assign default sets UPDATED (flags 31) with the proxied setter:\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "a bindable prop always forces the context frame:\n{js}"
    );
    assert!(
        !js.contains("'a', 27,"),
        "the bindable self-assign must not drop UPDATED to flags 27:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_default_sibling_member_write_marks_sibling_flag_23() {
    // `let { a = $bindable(b.x++), b = { x: 0 } }` — the member write inside
    // the BINDABLE default deep-mutates the plain sibling `b`: `b` gains
    // UPDATED (flags 23) while the bindable writer stays un-updated (flags
    // 27), frame present. Verified against svelte@5.56.10.
    let src =
        "<script>let { a = $bindable(b.x++), b = { x: 0 } } = $props();</script>\n<p>{a}{b}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("a = $.prop($$props, 'a', 27, () => $.proxy(b().x++))"),
        "the bindable writer stays flags 27 with the proxied getter-based write:\n{js}"
    );
    assert!(
        js.contains("b = $.prop($$props, 'b', 23, () => ({ x: 0 }))"),
        "the written plain sibling gains UPDATED (flags 23):\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "the bindable + prop-rooted member write forces the context frame:\n{js}"
    );
    assert!(
        !js.contains("'b', 19,"),
        "the written sibling must not drop UPDATED to flags 19:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_arrow_default_with_inner_self_write_stays_raw_flag_15() {
    // `let { v = $bindable(() => (v = 1)) }` — official `should_proxy` is
    // FALSE for a function literal, so the bindable arrow default skips the
    // `$.proxy` wrap AND stays a raw simple initial: flags 15 (3 | 4 | 8 —
    // UPDATED from the deferred write, BINDABLE, no LAZY), single rewritten
    // arrow. Verified against svelte@5.56.10.
    let src = "<script>let { v = $bindable(() => (v = 1)) } = $props();</script>\n<p>{v}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 15, () => (v(1)));"),
        "a bindable arrow default with an inner self-write stays raw (flags 15):\n{js}"
    );
    assert!(
        js.contains("$.push($$props, true);"),
        "a bindable prop always forces the context frame:\n{js}"
    );
    assert!(
        !js.contains("$.proxy"),
        "a function-valued bindable default never proxies:\n{js}"
    );
    assert!(
        !js.contains("'v', 31,") && !js.contains("() => () =>"),
        "the bindable arrow must not set LAZY / double-thunk:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn props_bindable_arrow_default_stays_raw_unproxied_flag_11() {
    // CONTROL (green pre-fix): `let { v = $bindable(() => 1) }` — official
    // `should_proxy` is false for a function literal and the arrow is simple:
    // raw initial, flags 11 (3 | 8), no `$.proxy`, no LAZY. Verified against
    // svelte@5.56.10.
    let src = "<script>let { v = $bindable(() => 1) } = $props();</script>\n<p>{v}</p>\n";
    let js = emit(src, "App.svelte");
    assert!(
        js.contains("let v = $.prop($$props, 'v', 11, () => 1);"),
        "a bindable arrow default is raw flags 11:\n{js}"
    );
    assert!(
        !js.contains("$.proxy") && !js.contains("'v', 27,"),
        "a function-valued bindable default never proxies:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn store_bind_value_passes_bare_accessor_and_store_set_closure() {
    // `bind:value={$c}`: the getter is the BARE accessor thunk (`$c`, already a
    // zero-arg function — official passes it unwrapped) and the setter the
    // complete `($$value) => $.store_set(c, $$value)` closure.
    let js = emit(
        "<script>import { writable } from 'svelte/store'; const c = writable('');</script>\n<input bind:value={$c} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, $c, ($$value) => $.store_set(c, $$value));"),
        "the store bind passes the bare accessor + store_set closure:\n{js}"
    );
    // NEGATIVE: the getter is NOT re-wrapped in a synthesized thunk.
    assert!(
        !js.contains("() => $c,") && !js.contains("() => $c()"),
        "the store bind getter must not be re-thunked:\n{js}"
    );
}

#[test]
fn store_bind_this_target_fails_closed() {
    // `bind:this={$c}` is NOT a supported store position — the `bind:this`
    // classifier only admits a DECLARED instance local / function pair, so the
    // store-accessor target fails closed rather than emitting a broken ref bind.
    assert_fail_closed(
        "<script>import { writable } from 'svelte/store'; const c = writable(0); let n = $state(0);</script>\n<div bind:this={$c}></div>\n<button onclick={() => n++}>{n}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "this"),
    );
}

#[test]
fn legacy_bind_target_let_promotes_to_mutable_source() {
    // A DOM bind-target legacy `let` ALSO promotes (a bind writes its target):
    // `bind:value={v}` emits the `$.get`/`$.set` thunks over the mutable source
    // (oracle-verified) — never a verbatim runes-shaped plain local.
    let js = emit(
        "<script>let v = 'x';</script>\n<input bind:value={v} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("let v = $.mutable_source('x');"),
        "a bind-target legacy let promotes to $.mutable_source:\n{js}"
    );
    assert!(
        js.contains("$.bind_value(input, () => $.get(v), ($$value) => $.set(v, $$value));"),
        "the bind thunks read/write the mutable source:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");

    // The UNINITIALIZED form lowers to the zero-arg `$.mutable_source()`.
    let js = emit(
        "<script>let v;</script>\n<input bind:value={v} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("let v = $.mutable_source();"),
        "an uninitialized bind-target let lowers zero-arg:\n{js}"
    );
    assert!(
        !js.contains("$.mutable_source(void 0)") && !js.contains("$.mutable_source(undefined)"),
        "the uninitialized form is the ZERO-ARG call:\n{js}"
    );
}

#[test]
fn legacy_member_bind_target_writes_through_mutate() {
    // A MEMBER bind target rooted at a promoted object let writes through
    // `$.mutate(root, …)` with the root read rewritten (oracle-verified:
    // `($$value) => $.mutate(o, $.get(o).x = $$value)`).
    let js = emit(
        "<script>let o = { x: '' };</script>\n<input bind:value={o.x} />\n",
        "App.svelte",
    );
    assert!(
        js.contains("let o = $.mutable_source({ x: '' });"),
        "the object-init bind-target let promotes with its init:\n{js}"
    );
    assert!(
        js.contains("() => $.get(o).x"),
        "the member getter reads through $.get on the root:\n{js}"
    );
    assert!(
        js.contains("($$value) => $.mutate(o, $.get(o).x = $$value)"),
        "the member setter wraps in $.mutate:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_window_bind_setter_carries_no_proxy_flag() {
    // A special-host bind under LEGACY mode emits the plain `$.set(local,
    // $$value)` setter — the runes-only proxy flag (`, true`) must NOT appear
    // (oracle-verified: `$.bind_window_scroll('y', () => $.get(y), ($$value) =>
    // $.set(y, $$value))`).
    let js = emit(
        "<script>\nlet y = 0;\n</script>\n<svelte:window bind:scrollY={y} />\n<p>hi</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains("let y = $.mutable_source(0);"),
        "the window bind target promotes:\n{js}"
    );
    assert!(
        js.contains("$.bind_window_scroll('y', () => $.get(y), ($$value) => $.set(y, $$value))"),
        "the legacy special-host setter is un-proxied:\n{js}"
    );
    assert!(
        !js.contains("$$value, true)"),
        "the runes-only proxy flag must not appear under legacy mode:\n{js}"
    );
}

#[test]
fn component_export_bindings_fail_closed_with_their_own_identity() {
    // `export const` / `export function` / `export class` (the official
    // `$$exports`/`$.bind_prop` readonly-export mechanism) fail closed under
    // their OWN diagnostic identity in BOTH modes — never the deleted blanket
    // legacy-export refusal, never the generic `export` construct label.
    for (label, source) in [
        (
            "legacy export const",
            "<script>export const c = 1;</script>\n<p>hi</p>\n",
        ),
        (
            "legacy export function",
            "<script>export function f() {}</script>\n<p>hi</p>\n",
        ),
        (
            "legacy export class",
            "<script>export class K {}</script>\n<p>hi</p>\n",
        ),
        (
            "runes export const",
            "<script>let c = $state(0); export const FOO = 1;</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "runes export function",
            "<script>let c = $state(0); export function f() {}</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
    ] {
        let err = emit_result(source).expect_err(label);
        let ClientCompileError::Unsupported(surface) = &err else {
            panic!("{label}: expected the fail-closed unsupported quadrant, got {err:?}");
        };
        assert_eq!(
            surface.diagnostic_code(),
            "svelte-runtime-unsupported-component-export-binding",
            "{label}: must refuse under the component-export-binding identity, got {:?}",
            surface
        );
    }
}

#[test]
fn reactive_prop_store_combined_wraps_each_dep_by_binding_kind() {
    // `export let p` + `$c` store + `$: y = p + $c + 1` — the three dep
    // wrappers in one thunk: a legacy PROP dep deep-reads the getter call
    // (`$.deep_read_state(p())`), a STORE dep is the bare accessor call
    // (`$c()`), and the frame carries `$.init()` (the store factory call is an
    // unsafe imported call). Oracle-pinned against svelte@5.56.10.
    let js = emit(
        "<script>import { writable } from 'svelte/store'; export let p; const c = writable(0); $: y = p + $c + 1;</script>\n<p>{y}</p>\n",
        "App.svelte",
    );
    assert!(
        js.contains(
            "$.legacy_pre_effect(() => ($.deep_read_state(p()), $c()), () => { $.set(y, p() + $c() + 1); });"
        ),
        "the prop dep deep-reads, the store dep is the bare accessor call:\n{js}"
    );
    assert!(
        js.contains("\t$.init();\n"),
        "the unsafe store-factory call still warrants `$.init()`:\n{js}"
    );
    // The reset precedes `$.init()` (official emits registrations + reset at
    // the end of the instance body, then the legacy init hook).
    let reset = js.find("$.legacy_pre_effect_reset();").expect("reset");
    let init = js.find("\t$.init();").expect("init");
    assert!(
        reset < init,
        "`$.legacy_pre_effect_reset()` precedes `$.init()`:\n{js}"
    );
    // NEGATIVE: the prop dep is never a bare `$.get(p)` and the store dep is
    // never deep-read.
    assert!(
        !js.contains("$.get(p)") && !js.contains("$.deep_read_state($c())"),
        "dep wrappers are driven by binding kind:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn legacy_spread_input_keeps_remove_defaults_tail_after_deps() {
    // Oracle: $.attribute_effect(input, ($0) => ({ ...p(), title: $0 }), [() => (wrap)], void 0, void 0, void 0, true);
    let js = emit(
        "<script>export let p; export let obj;</script>\n<input {...p} title={obj.m()} />\n",
        "App.svelte",
    );
    assert!(
        js.contains(&format!(
            "[() => ({})], void 0, void 0, void 0, true)",
            obj_wrap("obj().m()")
        )),
        "the deps array rides the sync slot before the input tail:\n{js}"
    );
    assert!(parses_as_js(&js), "module must be valid JS:\n{js}");
}

#[test]
fn a_single_name_destructure_each_binds_the_item_not_its_field() {
    // Measured against the pinned official compiler:
    //   $.each(node, 17, () => items, ({ id }) => id, ($$anchor, $$item) => {
    //     let id = () => $.get($$item).id;
    //     $.template_effect(() => $.set_text(text, id()));
    //   });
    // Verter emits `(id) => id` as the key and `$.set_text(text, $.get(id))`,
    // so the rendered text is the ITEM object, not its `id` field.
    let js = emit(
        "<script>\n  let items = $state([{id:1}]);\n</script>\n{#each items as { id } (id)}<li>{id}</li>{/each}\n",
        "App.svelte",
    );
    assert!(
        js.contains("({ id }) => id"),
        "the key callback must destructure the item the way official does:\n{js}"
    );
    assert!(
        !js.contains("$.set_text(text, $.get(id))"),
        "the body must read the FIELD, not the whole item object:\n{js}"
    );
    assert!(
        js.contains("let id = () => $.get($$item).id;"),
        "the body must bind a getter thunk reading the FIELD off the whole item:\n{js}"
    );
}

#[test]
fn a_member_bind_rooted_at_an_each_item_is_accepted_by_official() {
    let js = emit(
        "<script>\n  let items = $state([{x:'a'}]);\n</script>\n{#each items as item (item)}<input bind:value={item.x}/>{/each}\n",
        "App.svelte",
    );
    assert!(
        js.contains("$.bind_value(input, () => item.x, ($$value) => item.x = $$value)"),
        "a member bind on an each item must lower to the official getter AND setter closures:\n{js}"
    );
}
