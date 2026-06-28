//! Svelte IDE TSX projection snapshots with NEGATIVE assertions.
//!
//! Each test pins a matrix row's projected TSX shape AND asserts the original
//! Svelte block/tag syntax left NO residue (`{#if`, `{@render`, `<script`,
//! `class:`, …). The clean-type-check gate (through TSGO) lives in the
//! session-side fixtures; these characterize the syntactic transform.

use super::projector::project_svelte_ide;
use super::DiagnosticSeverity;
use crate::svelte::parser::parse_svelte;

/// Project a source and return the generated TSX code.
fn project(source: &str) -> String {
    let parsed = parse_svelte(source);
    project_svelte_ide(source, &parsed, Some("Comp.svelte"), false).code
}

/// The projected render body (everything AFTER the unmapped prelude) — residue
/// assertions on directive prefixes target the body, not the prelude's own
/// `// transition:fn` / `// animate:fn` doc comments.
fn render_body(code: &str) -> &str {
    code.find("function __verter_render()")
        .map(|i| &code[i..])
        .unwrap_or(code)
}

#[test]
fn prelude_opens_the_projection_and_no_script_tag_survives() {
    let code = project("<script lang=\"ts\">let a = 1;</script>\n<div>{a}</div>");
    assert!(code.starts_with("/** @jsxImportSource @verter/svelte-jsx */"));
    assert!(code.contains("let a = 1;"));
    assert!(!code.contains("<script"));
    assert!(!code.contains("</script>"));
    // The render scope function wraps the template.
    assert!(code.contains("function __verter_render()"));
}

#[test]
fn ide_carrier_exports_public_facade_default_with_typed_props() {
    // The IDE carrier (`Comp.svelte.tsx`) is the self-diagnostics surface; it
    // composes the component's PUBLIC type as a clean `export default` — a
    // constructable component whose instance carries `$props`/`$events`/
    // `$slots`. (The bare-import target is the `Comp.d.svelte.ts` declaration
    // carrier, not this IDE carrier.) `$props` is derived SYNTACTICALLY from the
    // instance script's `$props()` annotation.
    let code = project(
        "<script lang=\"ts\">let { msg }: { msg: string } = $props();</script>\n<div>{msg}</div>",
    );
    assert!(
        code.contains("export default __VerterPublicComponent;"),
        "IDE carrier must export the public component facade as default:\n{code}"
    );
    assert!(
        code.contains("type __VerterPublicProps = { msg: string };"),
        "public facade $props must carry the syntactic annotation:\n{code}"
    );
    assert!(
        code.contains("$props: __VerterPublicProps;")
            && code.contains("$events: Record<string, unknown>;")
            && code.contains("$slots: Record<string, unknown>;"),
        "public instance must surface $props/$events/$slots:\n{code}"
    );
    // The bare `export {};` module marker is REPLACED by the facade default —
    // the `export default` already makes the file a module.
    assert!(
        !code.contains("export {};"),
        "the bare `export {{}}` marker must be replaced by the facade default:\n{code}"
    );
    // Template internals stay LOCAL (non-exported): the render scope fn is a
    // plain local function, never the public default.
    assert!(
        code.contains("function __verter_render()")
            && !code.contains("export default __verter_render")
            && !code.contains("export { __verter_render as default }"),
        "template internals (__verter_render) must NOT be the public default:\n{code}"
    );
}

#[test]
fn ide_carrier_facade_degrades_untyped_props_to_permissive_record() {
    // An untyped `$props()` (or no instance script) degrades the public facade
    // `$props` to a permissive `Record<string, unknown>` (LOCAL — no resolver).
    let code = project("<div>{x}</div>");
    assert!(
        code.contains("type __VerterPublicProps = Record<string, unknown>;"),
        "untyped/absent $props must degrade to a permissive Record:\n{code}"
    );
    assert!(
        code.contains("export default __VerterPublicComponent;"),
        "a template-only Svelte component must still export a public facade:\n{code}"
    );
}

#[test]
fn interpolation_is_kept_verbatim() {
    let code = project("<div>{count}</div>");
    assert!(code.contains("{count}"));
}

#[test]
fn if_block_projects_to_a_ternary_with_no_residue() {
    let code = project("<div>{#if cond}<span>a</span>{:else}<span>b</span>{/if}</div>");
    assert!(!code.contains("{#if"), "no {{#if residue: {code}");
    assert!(!code.contains("{:else}"), "no else residue");
    assert!(!code.contains("{/if}"), "no close residue");
    assert!(code.contains("cond ?"), "ternary present: {code}");
}

#[test]
fn empty_else_clause_leaves_no_raw_residue() {
    // P1-1: an EMPTY `{:else}` (no expr, no children) anchors at offset 0 under
    // the retired reverse-scan and never gets rewritten → raw `{:else}` leaks.
    // The `tag_span`-driven overwrite rewrites it regardless.
    let code = project("<div>{#if c}a{:else}{/if}</div>");
    assert!(!code.contains("{:else}"), "no empty-else residue: {code}");
    assert!(!code.contains("{/if}"), "no close residue: {code}");
    assert!(code.contains(") : (<>"), "else ternary arm present: {code}");
}

#[test]
fn empty_then_and_catch_clauses_leave_no_raw_residue() {
    // P1-1: empty `{:then}` / `{:catch}` (no binding, no children) must still be
    // rewritten — no raw `{:then}` / `{:catch}` leaks into the projected TSX.
    let code = project("<div>{#await p}load{:then}{:catch}{/await}</div>");
    assert!(!code.contains("{:then}"), "no empty-then residue: {code}");
    assert!(!code.contains("{:catch}"), "no empty-catch residue: {code}");
    assert!(!code.contains("{/await}"), "no /await residue: {code}");
    // Synthetic bindings appear for the empty clauses.
    assert!(
        code.contains("__verter_v: __VA"),
        "synthetic then binding: {code}"
    );
    assert!(
        code.contains("__verter_e: unknown"),
        "synthetic catch binding: {code}"
    );
}

#[test]
fn each_block_projects_to_map_with_no_residue() {
    let code = project("<ul>{#each items as item, i}<li>{item}</li>{/each}</ul>");
    assert!(!code.contains("{#each"), "no each residue: {code}");
    assert!(!code.contains("{/each}"));
    assert!(!code.contains(" as item"), "no ` as` residue: {code}");
    assert!(code.contains(".map((item, i) =>"), "map present: {code}");
}

#[test]
fn each_without_item_uses_a_synthetic_param() {
    let code = project("<ul>{#each list}<li>x</li>{/each}</ul>");
    assert!(!code.contains("{#each"));
    assert!(code.contains(".map(("), "map present: {code}");
}

#[test]
fn render_tag_projects_to_a_call_with_no_residue() {
    let code = project("<div>{@render row(item)}</div>");
    assert!(!code.contains("{@render"), "no @render residue: {code}");
    assert!(code.contains("{row(item)}"), "call present: {code}");
}

#[test]
fn snippet_block_projects_to_a_branded_declarator_hoisted_to_scope_top() {
    let code = project("<div>{@render row()}{#snippet row()}<span>hi</span>{/snippet}</div>");
    assert!(!code.contains("{#snippet"), "no #snippet residue: {code}");
    assert!(!code.contains("{/snippet}"));
    assert!(
        code.contains("const row = __verter_snippet("),
        "branded declarator present: {code}"
    );
    // The render call referencing `row` is still present.
    assert!(code.contains("{row()}"), "render call present: {code}");
    // Ordering: the declarator precedes the `return (` of the render fn.
    let decl_idx = code.find("const row = __verter_snippet(").unwrap();
    let return_idx = code.find("return (<>").unwrap();
    assert!(
        decl_idx < return_idx,
        "snippet declarator must be hoisted ABOVE the render return (TDZ): {code}"
    );
}

#[test]
fn legacy_on_event_on_intrinsic_element_dom_rewrites_verbatim_lowercase() {
    // F13: an INTRINSIC element's `on:click` keeps the verbatim DOM `onclick`
    // rewrite — only a COMPONENT-kind element routes to the checked event helper.
    let code = project("<button on:click={handle}>x</button>");
    let body = render_body(&code);
    assert!(!body.contains("on:click"), "no on: residue: {body}");
    assert!(
        body.contains("onclick={handle}"),
        "intrinsic DOM rewrite: {body}"
    );
    assert!(
        !body.contains("onClick"),
        "the onClick rename is RETIRED: {body}"
    );
    // An intrinsic element never routes through the component event helper
    // (the prelude declares the helper; the render body must not CALL it).
    assert!(
        !body.contains("__verter_event("),
        "intrinsic `on:` must NOT route the component event helper: {body}"
    );
}

#[test]
fn legacy_on_event_on_component_routes_the_checked_event_helper() {
    // F13: a COMPONENT-kind element's `on:select={h}` routes through the checked
    // `__verter_event(Child, "select", h)` helper (NOT the loose `on:`→`onclick`
    // verbatim rewrite). The component reference (`Child`), the static event name,
    // and the handler all flow into the call so TSGO checks the handler against
    // the component's `$events["select"]` payload.
    let code = project("<Child on:select={handle}>x</Child>");
    let body = render_body(&code);
    assert!(!body.contains("on:select"), "no on: residue: {body}");
    // The loose `onselect`/`onclick` verbatim rewrite must NOT be emitted for a
    // component element.
    assert!(
        !body.contains("onselect="),
        "a component `on:` must NOT verbatim-rewrite to `onselect=`: {body}"
    );
    assert!(
        body.contains("__verter_event(Child, \"select\", handle)"),
        "component `on:` routes the checked event helper: {body}"
    );
    // The check is emitted as a no-prop JSX spread so it contributes no attribute.
    assert!(
        body.contains("{...(__verter_event(Child, \"select\", handle), {})}"),
        "the event check is a no-prop spread: {body}"
    );
}

#[test]
fn svelte5_event_attribute_stays_verbatim_lowercase() {
    let code = project("<button onclick={handle}>x</button>");
    assert!(code.contains("onclick={handle}"));
    assert!(!code.contains("onClick"));
}

#[test]
fn css_custom_property_attribute_is_stripped_from_jsx_position() {
    let code = project("<div --track-color={c}>x</div>");
    // A `--`-prefixed name is not a valid JSX attribute identifier — no
    // `--track-color` residue survives in the projection.
    assert!(
        !code.contains("--track-color"),
        "no `--` custom-property attribute residue: {code}"
    );
    // The value expression stays present, void-checked via the spread (mapped,
    // checkable) — no `--` JSX attribute survives.
    assert!(
        code.contains("__verter_void(c)"),
        "value void-checked present: {code}"
    );
}

#[test]
fn bind_value_projects_to_a_checkable_attribute() {
    let code = project("<input bind:value={name} />");
    assert!(!code.contains("bind:value"), "no bind: residue: {code}");
    assert!(
        code.contains("value={name}"),
        "checkable value pair: {code}"
    );
}

#[test]
fn bind_this_on_an_intrinsic_projects_a_host_instance_assignment_check() {
    // F4: `bind:this={el}` on an intrinsic → a host-instance INVARIANT check via
    // `(el = (null! as Host)), __verter_bind_this_assignable<Host, typeof el>()`.
    // NO diagnostic, NO residue, NO bare `this={…}` attribute.
    let source = "<input bind:this={inputEl} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.diagnostics.is_empty(),
        "bind:this is now fully supported — no diagnostics: {:?}",
        projection.diagnostics
    );
    assert!(
        !render_body(&projection.code).contains("bind:this"),
        "no bind:this residue: {}",
        projection.code
    );
    assert!(
        !projection.code.contains("this={inputEl}"),
        "must NOT leak a bare this={{…}} attribute: {}",
        projection.code
    );
    assert!(
        projection.code.contains(
            "(inputEl = (null! as __VerterHostEl<\"input\">)), \
             __verter_bind_this_assignable<__VerterHostEl<\"input\">, typeof inputEl>()"
        ),
        "host-instance invariant check present: {}",
        projection.code
    );
}

#[test]
fn bind_this_on_an_element_access_lvalue_uses_the_read_bearing_invariant() {
    // F4: a `bind:this={refs[i]}` element-access lvalue is NOT `typeof`-safe
    // (`typeof refs[i]` parses `i` as a type), so it routes through the
    // read-bearing invariant `refs[i] = __verter_bind_rw<Host>(refs[i])` — NOT
    // the `typeof`-based assert.
    let code = project("<input bind:this={refs[i]} />");
    assert!(
        !render_body(&code).contains("bind:this"),
        "no residue: {code}"
    );
    assert!(
        code.contains("(refs[i] = __verter_bind_rw<__VerterHostEl<\"input\">>(refs[i]))"),
        "element-access bind:this uses the read-bearing invariant: {code}"
    );
    assert!(
        !code.contains("typeof refs[i]"),
        "must NOT emit an invalid `typeof refs[i]` type query: {code}"
    );
}

#[test]
fn bind_group_on_a_radio_input_projects_the_radio_checker() {
    // F4: `bind:group` (default radio) → the radio array-shape checker, NO
    // residue, NO `__verter_void`.
    let source = "<input bind:group={selected} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code.starts_with("svelte-unsupported")),
        "no unsupported diagnostic for bind:group: {:?}",
        projection.diagnostics
    );
    assert!(!render_body(&projection.code).contains("bind:group"));
    assert!(
        projection
            .code
            .contains("(selected = __verter_bind_group_radio(selected))"),
        "radio group checker present: {}",
        projection.code
    );
}

#[test]
fn bind_group_on_a_checkbox_input_projects_the_checkbox_checker() {
    // F4: `bind:group` on a `type="checkbox"` → the checkbox array-shape checker.
    let code = project("<input type=\"checkbox\" bind:group={selected} />");
    assert!(
        !render_body(&code).contains("bind:group"),
        "no residue: {code}"
    );
    assert!(
        code.contains("(selected = __verter_bind_group_checkbox(selected))"),
        "checkbox group checker present: {code}"
    );
}

#[test]
fn bind_group_on_a_non_input_tag_falls_through_to_an_attribute() {
    // F4: `bind:group` is special ONLY on an `<input>` (its contract tag). On a
    // `<div>` it is an unknown binding — it must NOT take the group checker; it
    // falls through to the plain attribute path (`group={x}`), which the
    // intrinsic table then rejects naturally (no synthetic group checker).
    let code = project("<div bind:group={x}></div>");
    assert!(
        !render_body(&code).contains("__verter_bind_group"),
        "non-input bind:group must NOT use the group checker: {code}"
    );
    assert!(
        code.contains("group={x}"),
        "falls through to an attribute: {code}"
    );
}

#[test]
fn bind_this_with_a_comma_value_does_not_leak_the_host_placeholder() {
    // A stray `bind:this={a, b}` is dispatched to the `this` handler FIRST (it is
    // never a function binding), so the `{HOST}` placeholder never leaks through
    // the generic F5 path as a literal type argument.
    let code = project("<input bind:this={a, b} />");
    assert!(!code.contains("{HOST}"), "no HOST placeholder leak: {code}");
    assert!(
        !render_body(&code).contains("__verter_bind_fn"),
        "bind:this must not route through the F5 checker: {code}"
    );
}

#[test]
fn bind_current_time_projects_a_read_write_value_check() {
    // F4 (writable media): `bind:currentTime` → an invariant value-type check.
    let code = project("<video bind:currentTime={t}></video>");
    assert!(!code.contains("bind:currentTime"), "no residue: {code}");
    assert!(
        code.contains("(t = __verter_bind_rw<HTMLMediaElement[\"currentTime\"]>(t))"),
        "read-write media check present: {code}"
    );
}

#[test]
fn bind_duration_projects_a_read_direction_check() {
    // F4 (readonly media): `bind:duration` → a read-direction assignment INTO the
    // local (`__verter_bind_read<…>()`) — DOM → local, the write-rejection path.
    let code = project("<video bind:duration={d}></video>");
    assert!(!code.contains("bind:duration"), "no residue: {code}");
    assert!(
        code.contains("(d = __verter_bind_read<HTMLMediaElement[\"duration\"]>())"),
        "read-direction media check present: {code}"
    );
}

#[test]
fn bind_client_width_projects_a_read_direction_number_check() {
    // F4 (readonly dimension): `bind:clientWidth` → a read-direction `number`.
    let code = project("<div bind:clientWidth={w}></div>");
    assert!(!code.contains("bind:clientWidth"), "no residue: {code}");
    assert!(
        code.contains("(w = __verter_bind_read<number>())"),
        "read-direction dimension check present: {code}"
    );
}

#[test]
fn bind_open_on_details_projects_a_boolean_read_write_check() {
    // F4 (`<details bind:open>`): a boolean read-write check.
    let code = project("<details bind:open={isOpen}></details>");
    assert!(!code.contains("bind:open"), "no residue: {code}");
    assert!(
        code.contains("(isOpen = __verter_bind_rw<boolean>(isOpen))"),
        "details open check present: {code}"
    );
}

#[test]
fn bind_inner_html_projects_a_string_read_write_check() {
    // F4 (contenteditable): `bind:innerHTML` → a string read-write check.
    let code = project("<div contenteditable bind:innerHTML={html}></div>");
    assert!(!code.contains("bind:innerHTML"), "no residue: {code}");
    assert!(
        code.contains("(html = __verter_bind_rw<string>(html))"),
        "contenteditable check present: {code}"
    );
}

#[test]
fn bind_files_projects_a_filelist_read_write_check() {
    // F4 (`bind:files`): a `FileList | null` read-write check.
    let code = project("<input type=\"file\" bind:files={fs} />");
    assert!(!code.contains("bind:files"), "no residue: {code}");
    assert!(
        code.contains("(fs = __verter_bind_rw<FileList | null>(fs))"),
        "files check present: {code}"
    );
}

#[test]
fn bind_checked_stays_supported_no_diagnostic() {
    // DISCRIMINATING: `bind:checked` is SUPPORTED — no diagnostic, projects to a
    // checkable `checked={…}` pair.
    let source = "<input bind:checked={on} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.diagnostics.is_empty(),
        "no diagnostic for the supported bind:checked: {:?}",
        projection.diagnostics
    );
    assert!(
        !projection.code.contains("bind:checked"),
        "no bind: residue"
    );
    assert!(
        projection.code.contains("checked={on}"),
        "checkable pair: {}",
        projection.code
    );
}

#[test]
fn bind_this_on_a_component_projects_an_instancetype_assignment_check() {
    // F4: `bind:this` on a COMPONENT binds the instance — checked against
    // `InstanceType<typeof MyComp>` (NOT the `$props` attribute path). NO bare
    // `this={…}` attribute, NO residue, NO diagnostic.
    let source = "<MyComp bind:this={ref} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code.starts_with("svelte-unsupported")),
        "no unsupported diagnostic for component bind:this: {:?}",
        projection.diagnostics
    );
    assert!(
        !render_body(&projection.code).contains("bind:this"),
        "no bind:this residue"
    );
    assert!(
        !projection.code.contains("this={ref}"),
        "must NOT leak a bare this={{ref}} attribute: {}",
        projection.code
    );
    assert!(
        projection.code.contains(
            "(ref = (null! as InstanceType<typeof MyComp>)), \
             __verter_bind_this_assignable<InstanceType<typeof MyComp>, typeof ref>()"
        ),
        "component instance invariant check present: {}",
        projection.code
    );
}

#[test]
fn bind_value_shorthand_projects_to_a_self_bound_pair() {
    // The valueless shorthand `bind:value` binds the same-named local — it must
    // become `value={value}`, NOT a bare `value` (which would be boolean `true`
    // and not check the bound variable).
    let code = project("<input bind:value />");
    assert!(!code.contains("bind:value"), "no bind: residue: {code}");
    assert!(
        code.contains("value={value}"),
        "self-bound pair present: {code}"
    );
}

#[test]
fn component_bind_prop_stays_supported_no_diagnostic() {
    // DISCRIMINATING: component `bind:prop` ($bindable) is SUPPORTED — a
    // capitalised tag name takes the supported path regardless of local name.
    let source = "<MyInput bind:custom={v} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.diagnostics.is_empty(),
        "no diagnostic for the supported component bind:prop: {:?}",
        projection.diagnostics
    );
    assert!(!projection.code.contains("bind:custom"), "no bind: residue");
    assert!(projection.code.contains("custom={v}"), "prop pair present");
}

#[test]
fn style_directive_strips_and_void_checks_its_value_no_residue() {
    // F1: `style:color={c}` — SUPPORTED. Stripped from the JSX position (no
    // `style:` residue, a `style:`-prefixed name is invalid JSX), the value
    // void-checked. No typed-unsupported diagnostic (the row is now supported).
    let source = "<div style:color={c}>x</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-unsupported-style-directive"),
        "the supported style: directive must NOT emit an out-of-scope diagnostic: {:?}",
        projection.diagnostics
    );
    assert!(
        !projection.code.contains("style:color"),
        "no style: residue: {}",
        projection.code
    );
    assert!(
        projection.code.contains("__verter_void(c)"),
        "value void-checked: {}",
        projection.code
    );
}

#[test]
fn style_directive_with_important_modifier_strips_and_void_checks() {
    // F1: `style:color|important={c}` — the `|important` modifier is
    // presentational; the directive still strips + void-checks the value.
    let source = "<div style:color|important={c}>x</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        !projection.code.contains("style:color") && !projection.code.contains("|important"),
        "no style:/modifier residue: {}",
        projection.code
    );
    assert!(
        projection.code.contains("__verter_void(c)"),
        "value void-checked: {}",
        projection.code
    );
}

#[test]
fn style_directive_shorthand_projects_the_implied_binding() {
    // F1: shorthand `style:color` (no value) projects the implied `color`
    // binding identifier (valid identifier) — void-checked, no residue.
    let source = "<div style:color>x</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        !projection.code.contains("style:color"),
        "no style: residue: {}",
        projection.code
    );
    assert!(
        projection.code.contains("__verter_void(color)"),
        "implied binding void-checked: {}",
        projection.code
    );
}

#[test]
fn transition_directive_projects_to_the_transition_checker_no_residue() {
    // F2: `transition:fn={p}` / `in:` / `out:` (+`|local`/`|global`) →
    // `{...(__verter_transition(NODE_HINT, fn, p), {})}` — stripped + spread-
    // merged, the fn + params checked against the host element instance type.
    for (src, fn_name, param) in [
        (
            "<div transition:fade={{ duration: d }}>x</div>",
            "fade",
            "d",
        ),
        ("<div in:fly={{ y: a }}>x</div>", "fly", "a"),
        ("<div out:slide={{ x: b }}>x</div>", "slide", "b"),
        (
            "<div transition:fade|local={{ duration: d }}>x</div>",
            "fade",
            "d",
        ),
    ] {
        let parsed = parse_svelte(src);
        let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
        let body = render_body(&projection.code);
        assert!(
            !body.contains("transition:")
                && !body.contains("in:")
                && !body.contains("out:")
                && !body.contains("|local")
                && !body.contains("|global"),
            "no directive/modifier residue for {src}: {}",
            projection.code
        );
        // The directive is projected to a REAL CALL of the transition function on
        // the host element instance + the params expression: the call result is
        // routed through the `__verter_transition` result-shape checker.
        assert!(
            projection.code.contains(&format!(
                "__verter_transition({fn_name}((null! as __VerterHostEl<\"div\">), "
            )),
            "transition function called on host element for {src}: {}",
            projection.code
        );
        assert!(
            projection.code.contains(param),
            "params present for {src}: {}",
            projection.code
        );
    }
}

#[test]
fn transition_on_a_component_falls_back_to_the_element_node_hint() {
    // F2: a `transition:` on a COMPONENT (unknown host element) falls back to the
    // `Element` node hint (no precise DOM instance type).
    let source = "<MyComp transition:fade={p} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .code
            .contains("__verter_transition(fade((null! as Element), "),
        "component host falls back to Element hint: {}",
        projection.code
    );
}

#[test]
fn animate_directive_projects_to_the_animate_checker_no_residue() {
    // F3: `animate:flip={p}` →
    // `{...(__verter_animate(flip(HINT, DIRECTIONS, p)), {})}`.
    let source = "<div animate:flip={{ delay: e }}>x</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        !render_body(&projection.code).contains("animate:"),
        "no animate: residue: {}",
        projection.code
    );
    assert!(
        projection
            .code
            .contains("__verter_animate(flip((null! as __VerterHostEl<\"div\">), (null! as { from: DOMRect; to: DOMRect }), "),
        "animate function called on host element + directions: {}",
        projection.code
    );
    assert!(
        projection.code.contains('e'),
        "params present: {}",
        projection.code
    );
}

#[test]
fn valueless_transition_and_animate_call_the_function_without_params() {
    // F2/F3: a valueless `transition:fade` / `animate:flip` (no `={…}`) calls the
    // function on the host node (and directions for animate) without a params arg.
    let t = project("<div transition:fade>x</div>");
    assert!(
        t.contains("__verter_transition(fade((null! as __VerterHostEl<\"div\">)))"),
        "valueless transition calls fn without params: {t}"
    );
    let a = project("<div animate:flip>x</div>");
    assert!(
        a.contains(
            "__verter_animate(flip((null! as __VerterHostEl<\"div\">), (null! as { from: DOMRect; to: DOMRect })))"
        ),
        "valueless animate calls fn without params: {a}"
    );
}

#[test]
fn class_directive_projects_to_a_data_attribute() {
    let code = project("<div class:active={isActive}>x</div>");
    assert!(!code.contains("class:active"), "no class: residue: {code}");
    assert!(code.contains("{isActive}"), "value present: {code}");
}

#[test]
fn html_tag_projects_to_a_string_checkable_position() {
    let code = project("<div>{@html markup}</div>");
    assert!(!code.contains("{@html"), "no @html residue: {code}");
    assert!(code.contains("markup"), "value present: {code}");
}

#[test]
fn attach_tag_projects_to_the_checker_argument() {
    let code = project("<canvas {@attach draw}></canvas>");
    // `{@attach draw}` as a tag is rendered through __verter_attach.
    assert!(!code.contains("{@attach"), "no @attach residue: {code}");
}

#[test]
fn debug_tag_projects_to_a_void_reference() {
    let code = project("<div>{@debug a, b}</div>");
    assert!(!code.contains("{@debug"), "no @debug residue: {code}");
    assert!(code.contains("__verter_void"), "void ref present: {code}");
}

#[test]
fn const_declaration_tag_hoists_to_a_sibling_visible_statement() {
    let code = project("<div>{const total = a + b}{total}</div>");
    // No raw declaration-tag residue (`{const total` directly in the markup).
    assert!(
        !code.contains("<div>{const total"),
        "no declaration-tag residue: {code}"
    );
    // The const is HOISTED to a real statement (sibling-run scope) — it
    // precedes the render `return` so the sibling `{total}` reference resolves.
    assert!(
        code.contains("const") && code.contains("total = a + b"),
        "const present: {code}"
    );
    let decl_idx = code.find("total = a + b").unwrap();
    let return_idx = code.find("return (<>").unwrap();
    assert!(
        decl_idx < return_idx,
        "the declaration must hoist ABOVE the render return (sibling-visible): {code}"
    );
    // The sibling `{total}` reference is still present.
    assert!(
        code.contains("{total}"),
        "sibling reference present: {code}"
    );
}

#[test]
fn component_style_block_is_stripped() {
    let code = project("<div>x</div>\n<style>.a { color: red; }</style>");
    assert!(
        !code.contains("color: red"),
        "style content stripped: {code}"
    );
    assert!(!code.contains("<style"), "no style tag: {code}");
}

#[test]
fn await_block_projects_with_no_residue_and_no_await_expression_diagnostic() {
    // The `{#await}` TEMPLATE BLOCK (no await-EXPRESSION in its head) projects to
    // the IIFE state holder with NO residue and — since there is no `await`
    // keyword anywhere — NO await-experimental diagnostic.
    let source = "<div>{#await p}loading{:then v}{v}{:catch e}{e}{/await}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(!projection.code.contains("{#await"), "no #await residue");
    assert!(!projection.code.contains("{:then"), "no :then residue");
    assert!(!projection.code.contains("{/await}"), "no /await residue");
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "the `{{#await}}` block head `p` has no await-EXPRESSION: {:?}",
        projection.diagnostics
    );
}

#[test]
fn function_binding_on_an_element_projects_the_fn_checker_with_the_table_type() {
    // F5: `bind:value={get, set}` on an `<input>` → the get/set checker keyed by
    // the element's bind-target type. `value`/`checked` are NOT in the wide-family
    // table, so this element function-binding leaves `V` inferred (the checker
    // enforces get/set mutual consistency alone). NO residue, NO diagnostic.
    let source = "<input bind:files={getFiles, setFiles} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.diagnostics.is_empty(),
        "function bindings are now supported — no diagnostic: {:?}",
        projection.diagnostics
    );
    assert!(!projection.code.contains("bind:files"), "no residue");
    // `bind:files` IS in the table → its value type pins the checker.
    assert!(
        projection
            .code
            .contains("__verter_bind_fn<FileList | null>(getFiles, setFiles)"),
        "element function-binding checker present with the table type: {}",
        projection.code
    );
}

#[test]
fn function_binding_on_a_component_projects_the_instancetype_props_target() {
    // F5: a component function binding derives `V` in TS from
    // `InstanceType<typeof Child>["$props"]["prop"]` — NO Rust resolver call.
    let source = "<Child bind:size={getSize, setSize} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.diagnostics.is_empty(),
        "component function bindings are supported — no diagnostic: {:?}",
        projection.diagnostics
    );
    assert!(!projection.code.contains("bind:size"), "no residue");
    assert!(
        projection.code.contains(
            "__verter_bind_fn<InstanceType<typeof Child>[\"$props\"][\"size\"]>(getSize, setSize)"
        ),
        "component function-binding props-target checker present: {}",
        projection.code
    );
}

#[test]
fn function_binding_write_only_projects_the_null_get_checker() {
    // F5 (write-only `{null, set}`): the `null` get + the `set` are passed
    // verbatim — `__verter_bind_fn` accepts `null` for `get`.
    let code = project("<input bind:files={null, setFiles} />");
    assert!(!code.contains("bind:files"), "no residue: {code}");
    assert!(
        code.contains("__verter_bind_fn<FileList | null>(null, setFiles)"),
        "write-only function-binding checker present: {code}"
    );
}

#[test]
fn function_binding_on_value_derives_the_target_type_from_the_intrinsic_table() {
    // F5: `bind:value={get,set}` (not in the wide-family table) derives `V` from
    // `SvelteHTMLElements["input"]["value"]` so a DOM-wrong get/set pair fails —
    // typed entirely in the projected TSX (no Rust resolver), NOT left inferred.
    let code = project("<input bind:value={getV, setV} />");
    assert!(!code.contains("bind:value"), "no residue: {code}");
    assert!(
        code.contains(
            "__verter_bind_fn<import(\"svelte/elements\").SvelteHTMLElements[\"input\"][\"value\"]>(getV, setV)"
        ),
        "intrinsic-table-derived function-binding target present: {code}"
    );
}

#[test]
fn await_expression_in_interpolation_projects_the_promise_like_helper() {
    // F6: a markup `{await thing}` is REWRITTEN to `{__verter_await_expr(thing)}`
    // — `__verter_render` stays SYNC (no raw `await` in the render fn), and the
    // resolved value type flows through the PromiseLike-constrained helper. ONE
    // informational diagnostic is recorded.
    let source = "<div>{await thing}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("__verter_await_expr(thing)"),
        "markup await projects through the PromiseLike helper: {body}"
    );
    // The original `thing` bytes are preserved (hover / mapping); the `await `
    // keyword is gone from the render body (no raw await — render stays sync).
    assert!(
        !body.contains("await thing") && !body.contains("await "),
        "no raw `await` keyword survives in the render body: {body}"
    );
    let diag = projection
        .diagnostics
        .iter()
        .find(|d| d.code == "svelte-await-experimental")
        .expect("await-experimental diagnostic present");
    assert_eq!(
        diag.severity,
        DiagnosticSeverity::Information,
        "the experimental await-expression is INFORMATIONAL, not an error"
    );
    assert!(
        diag.message.contains("experimental") && diag.message.contains("type-checked here"),
        "the informational message uses the ratified copy: {}",
        diag.message
    );
}

#[test]
fn await_in_instance_script_top_level_records_the_diagnostic() {
    // Await at instance-script top level (position 1).
    let source = "<script lang=\"ts\">const x = await fetchThing();</script>\n<div>x</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "script-top-level await diagnostic: {:?}",
        projection.diagnostics
    );
}

#[test]
fn await_inside_derived_arg_records_the_diagnostic() {
    // P2-2: await inside `$derived(...)` / `$derived.by(...)` args (position 2),
    // in BOTH script and markup.
    let script_src = "<script lang=\"ts\">const v = $derived(await load());</script>\n<div>x</div>";
    let parsed = parse_svelte(script_src);
    let projection = project_svelte_ide(script_src, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "$derived(await …) in script: {:?}",
        projection.diagnostics
    );

    // A DIRECT await in the derived arg (the experimental reactive form) IS
    // flagged in markup AND REWRITTEN through the same helper:
    // `$derived(await load())` → `$derived(__verter_await_expr(load()))`. An
    // await nested inside an async arrow is NOT flagged (ordinary TS) — the
    // discriminating async-fn case is covered separately.
    let markup_src = "<div>{$derived(await load())}</div>";
    let parsed = parse_svelte(markup_src);
    let projection = project_svelte_ide(markup_src, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "$derived(await …) in markup: {:?}",
        projection.diagnostics
    );
    let body = render_body(&projection.code);
    assert!(
        body.contains("$derived(__verter_await_expr(load()))"),
        "markup `$derived(await …)` routes through the same helper: {body}"
    );
}

#[test]
fn nested_await_in_markup_records_the_diagnostic() {
    // P2-2: a NESTED markup await `{foo(await bar())}` (not a leading prefix) is
    // caught by the word-boundary scan — the retired leading-prefix check missed
    // it.
    let source = "<div>{foo(await bar())}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "nested markup await diagnostic: {:?}",
        projection.diagnostics
    );
    // The nested await is rewritten in place: `foo(await bar())` →
    // `foo(__verter_await_expr(bar()))`.
    let body = render_body(&projection.code);
    assert!(
        body.contains("foo(__verter_await_expr(bar()))"),
        "the nested markup await is rewritten through the helper: {body}"
    );
}

#[test]
fn await_inside_an_async_function_body_is_not_flagged() {
    // An `await` inside an `async function` / `async () =>` body in the script
    // is ORDINARY TypeScript — NOT the experimental Svelte await-expression. It
    // must NOT be flagged. DISCRIMINATING against a top-level await (which IS).
    for src in [
        "<script lang=\"ts\">async function f() { await load(); }</script>\n<div>x</div>",
        "<script lang=\"ts\">const g = async () => { await load(); };</script>\n<div>x</div>",
        "<script lang=\"ts\">const o = { async m() { await load(); } };</script>\n<div>x</div>",
    ] {
        let parsed = parse_svelte(src);
        let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
        assert!(
            !projection
                .diagnostics
                .iter()
                .any(|d| d.code == "svelte-await-experimental"),
            "await inside an async fn must NOT be flagged for {src}: {:?}",
            projection.diagnostics
        );
    }

    // DISCRIMINATING: a TOP-LEVEL script await IS flagged.
    let top = "<script lang=\"ts\">const x = await load();</script>\n<div>x</div>";
    let parsed = parse_svelte(top);
    let projection = project_svelte_ide(top, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "a top-level script await MUST be flagged: {:?}",
        projection.diagnostics
    );
}

#[test]
fn await_in_async_arrow_expression_body_is_not_flagged() {
    // An async arrow with an EXPRESSION body (no braces) — `async () => await x`
    // — is ordinary TS; its await must NOT be flagged.
    for src in [
        "<script lang=\"ts\">const f = async () => await load();</script>\n<div>x</div>",
        "<div>{$derived.by(async () => await load())}</div>",
    ] {
        let parsed = parse_svelte(src);
        let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
        assert!(
            !projection
                .diagnostics
                .iter()
                .any(|d| d.code == "svelte-await-experimental"),
            "await in an async arrow expr body must NOT be flagged for {src}: {:?}",
            projection.diagnostics
        );
    }
}

#[test]
fn await_inside_a_template_literal_interpolation_is_scanned() {
    // A top-level template-literal `${await …}` IS the experimental form and
    // must be flagged (the scan recurses into `${}`).
    let top = "<script lang=\"ts\">const x = `${await load()}`;</script>\n<div>x</div>";
    let parsed = parse_svelte(top);
    let projection = project_svelte_ide(top, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "a top-level `${{await}}` must be flagged: {:?}",
        projection.diagnostics
    );

    // DISCRIMINATING: the same inside an async arrow's template literal is NOT
    // flagged (ordinary TS).
    let shadowed =
        "<script lang=\"ts\">const f = async () => `${await load()}`;</script>\n<div>x</div>";
    let parsed = parse_svelte(shadowed);
    let projection = project_svelte_ide(shadowed, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "an `${{await}}` inside an async arrow must NOT be flagged: {:?}",
        projection.diagnostics
    );
}

#[test]
fn nested_async_arrows_do_not_unshadow_an_outer_async_body() {
    // `async () => wrap(async () => await a()) + await b()` — BOTH awaits are
    // inside the OUTER async arrow body and must NOT be flagged (the inner arrow
    // must not lose the outer's async shadow). Grammar-correct via OXC scoping.
    let src =
        "<script lang=\"ts\">const f = async () => wrap(async () => await a()) + await b();</script>\n<div>x</div>";
    let parsed = parse_svelte(src);
    let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "neither await in nested async arrows may be flagged: {:?}",
        projection.diagnostics
    );
}

#[test]
fn asi_terminated_async_arrow_does_not_swallow_a_following_top_level_await() {
    // Semicolon-less ASI: `const f = async () => await a()\nconst x = await b()`
    // — `await b()` is a real top-level experimental await and MUST be flagged
    // (the async arrow body ended at the statement boundary). Grammar-correct.
    let src =
        "<script lang=\"ts\">const f = async () => await a()\nconst x = await b()</script>\n<div>x</div>";
    let parsed = parse_svelte(src);
    let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
    let count = projection
        .diagnostics
        .iter()
        .filter(|d| d.code == "svelte-await-experimental")
        .count();
    assert_eq!(
        count, 1,
        "exactly the top-level `await b()` must flag (ASI ends the arrow body): {:?}",
        projection.diagnostics
    );
}

#[test]
fn template_literal_close_does_not_swallow_a_following_top_level_await() {
    // A completed template literal must not leave the scanner in template mode —
    // a following top-level await is still flagged.
    let src =
        "<script lang=\"ts\">const s = `${foo}`; const y = await load();</script>\n<div>x</div>";
    let parsed = parse_svelte(src);
    let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "the top-level await after a template literal must be flagged: {:?}",
        projection.diagnostics
    );
}

#[test]
fn async_arrow_expr_shadows_the_whole_body_not_just_the_first_subexpr() {
    // `async () => (await a) + await b` — BOTH awaits are inside the async arrow
    // expression body and must NOT be flagged (a `(await a)` close must not end
    // the arrow-expr shadow).
    let shadowed =
        "<script lang=\"ts\">const f = async () => (await a) + await b;</script>\n<div>x</div>";
    let parsed = parse_svelte(shadowed);
    let projection = project_svelte_ide(shadowed, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "neither await in the async arrow body may be flagged: {:?}",
        projection.diagnostics
    );

    // DISCRIMINATING: `const a = async () => await x, b = await y;` — the arrow
    // body ends at the comma, so `await y` (a top-level initializer) IS flagged
    // while `await x` (inside the arrow) is NOT — exactly one diagnostic.
    let mixed =
        "<script lang=\"ts\">const a = async () => await x, b = await y;</script>\n<div>x</div>";
    let parsed = parse_svelte(mixed);
    let projection = project_svelte_ide(mixed, &parsed, Some("C.svelte"), false);
    let count = projection
        .diagnostics
        .iter()
        .filter(|d| d.code == "svelte-await-experimental")
        .count();
    assert_eq!(
        count, 1,
        "exactly the top-level `await y` must flag (arrow body ends at the comma): {:?}",
        projection.diagnostics
    );
}

#[test]
fn dotted_component_bind_prop_stays_supported() {
    // A dotted/namespaced component `<ns.Widget bind:custom={v} />` is a
    // COMPONENT (parser classifies dotted tags as components) — its bind:prop is
    // supported, no diagnostic.
    let source = "<ns.Widget bind:custom={v} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.diagnostics.is_empty(),
        "dotted component bind:prop must be supported: {:?}",
        projection.diagnostics
    );
    assert!(!projection.code.contains("bind:custom"), "no bind: residue");
    assert!(projection.code.contains("custom={v}"), "prop pair present");
}

#[test]
fn await_word_boundary_does_not_false_positive_on_identifiers_or_strings() {
    // DISCRIMINATING: `awaited`/`myawait` identifiers and an `await` inside a
    // string literal must NOT trigger the diagnostic.
    for src in [
        "<div>{awaited}</div>",
        "<div>{myawait + awaiter}</div>",
        "<div>{\"await this\"}</div>",
    ] {
        let parsed = parse_svelte(src);
        let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
        assert!(
            !projection
                .diagnostics
                .iter()
                .any(|d| d.code == "svelte-await-experimental"),
            "no false-positive await diagnostic for {src}: {:?}",
            projection.diagnostics
        );
    }
}

#[test]
fn svelte_markup_await_projects_promise_like_helper() {
    // ARCHITECTURE GUARD (F6): a PURE-markup await projects `__verter_await_expr`
    // and `__verter_render` STAYS SYNC — there is NO raw `await` keyword anywhere
    // in the render fn (a raw `await` outside an async fn would be INVALID TSX).
    let source = "<div>{await fetchUser()}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("__verter_await_expr(fetchUser())"),
        "the PromiseLike helper wraps the awaited argument: {body}"
    );
    // The render fn is declared exactly once and is NOT async.
    assert!(
        body.contains("function __verter_render()"),
        "render fn present: {body}"
    );
    assert!(
        !body.contains("async function __verter_render"),
        "`__verter_render` must STAY SYNC: {body}"
    );
    // No raw `await ` keyword survives in the render body — the rewrite removed it.
    assert!(
        !body.contains("await "),
        "no raw `await` keyword in the render body (render stays sync): {body}"
    );
    // The prelude declares the PromiseLike-constrained helper.
    assert!(
        projection.code.contains(
            "declare function __verter_await_expr<T extends PromiseLike<unknown>>(value: T): Awaited<T>;"
        ),
        "the prelude declares the PromiseLike-constrained helper: {}",
        projection.code
    );
}

#[test]
fn svelte_await_diagnostic_is_informational() {
    // ARCHITECTURE GUARD (F6): the `svelte-await-experimental` diagnostic carries
    // `Information` severity (the construct is REAL-checked). DISCRIMINATING: an
    // unknown-unsupported construct stays `Error`.
    let await_src = "<div>{await thing}</div>";
    let parsed = parse_svelte(await_src);
    let projection = project_svelte_ide(await_src, &parsed, Some("C.svelte"), false);
    let await_diag = projection
        .diagnostics
        .iter()
        .find(|d| d.code == "svelte-await-experimental")
        .expect("await diagnostic present");
    assert_eq!(
        await_diag.severity,
        DiagnosticSeverity::Information,
        "the await-experimental diagnostic is INFORMATIONAL"
    );

    // DISCRIMINATING: an unrecognised `{@unknown …}` tag stays an ERROR.
    let unknown_src = "<div>{@unknown foo}</div>";
    let parsed = parse_svelte(unknown_src);
    let projection = project_svelte_ide(unknown_src, &parsed, Some("C.svelte"), false);
    let err_diag = projection
        .diagnostics
        .iter()
        .find(|d| d.code == "svelte-unsupported-construct")
        .expect("unknown-construct diagnostic present");
    assert_eq!(
        err_diag.severity,
        DiagnosticSeverity::Error,
        "an unsupported construct stays an ERROR (discriminating vs Information)"
    );
}

#[test]
fn await_expression_in_an_attribute_value_is_rewritten_through_the_helper() {
    // F6: an await in an ATTRIBUTE value position is ALSO rewritten — a raw
    // `await` left in the sync render fn would be invalid TSX. `__verter_render`
    // stays sync.
    let source = "<img src={await fetchUrl()} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("src={__verter_await_expr(fetchUrl())}"),
        "the attribute-value await is rewritten in place: {body}"
    );
    assert!(
        !body.contains("await "),
        "no raw `await` keyword survives in the render body: {body}"
    );
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "the attribute await records the informational diagnostic: {:?}",
        projection.diagnostics
    );
}

#[test]
fn full_clause_await_block_with_destructuring_bindings_has_no_residue() {
    // FOLD-IN regression: the FULL-clause `{#await p}{:then {a,b}}{:catch {e}}`
    // form (separate clauses, NOT inline) must NOT be affected by the inline
    // close-brace search — the clause bindings live AFTER the open tag and are
    // excluded from `search_from`. No raw await residue, no stranded `(<>}`.
    let source = "{#await p}<span>loading</span>{:then { a, b }}<span>{a}{b}</span>{:catch { message }}<span>{message}</span>{/await}";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        !body.contains("{#await") && !body.contains("{:then") && !body.contains("{/await}"),
        "no raw await-block residue in the full-clause destructuring form: {body}"
    );
    assert!(
        !body.contains("(<>}"),
        "no stranded close brace producing `(<>}}`: {body}"
    );
}

#[test]
fn dynamic_component_projects_through_the_helper_with_no_residue() {
    // F8: `<svelte:component this={C} prop={x} />` flips to SUPPORTED — it
    // projects through the `__verter_dynamic_component` prelude checker (the
    // `this` value is checked class-shaped, the props bag against
    // `InstanceType<...>["$props"]`). No `<svelte:component` residue, no
    // out-of-scope diagnostic.
    let source = "<svelte:component this={Dynamic} label={title} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.diagnostics.is_empty(),
        "no out-of-scope diagnostic for the now-supported svelte:component: {:?}",
        projection.diagnostics
    );
    let body = render_body(&projection.code);
    assert!(
        !body.contains("svelte:component"),
        "no svelte:component residue: {body}"
    );
    assert!(
        body.contains("__verter_dynamic_component((Dynamic))"),
        "dynamic-component helper present over `this` (parenthesized): {body}"
    );
    // The remaining attribute stays a checkable JSX attribute on the synthesized
    // component local.
    assert!(body.contains("label={title}"), "prop attr kept: {body}");
    // The old out-of-scope diagnostic codes are gone entirely.
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-deprecated-special-element"),
        "the deprecated-special-element diagnostic is retired"
    );
}

#[test]
fn dynamic_component_this_expression_is_parenthesized() {
    // P1: a `this={a, b}` sequence/comma expression must be parenthesized when
    // interpolated into the helper call, so it stays ONE argument
    // (`__verter_dynamic_component((a, b))`) — a bare `(a, b)` would split into
    // two arguments and false-fail a valid component-valued `this`.
    let code = project("<svelte:component this={tick(), Dyn} label={\"ok\"} />");
    let body = render_body(&code);
    assert!(
        body.contains("__verter_dynamic_component((tick(), Dyn))"),
        "the this expression is parenthesized to stay one argument: {body}"
    );
    assert!(
        !body.contains("__verter_dynamic_component(tick(), Dyn)"),
        "no bare sequence expression splitting into two args: {body}"
    );
}

#[test]
fn dynamic_component_this_await_rewrites_through_the_helper_and_render_stays_sync() {
    // F6/F8: a markup `await` in the `<svelte:component this={await load()}>` value
    // is a MARKUP-EXPRESSION position — `__verter_render` STAYS SYNC, so a raw
    // `await` left in the dynamic-component IIFE would be INVALID TSX. The text
    // path must route the await rewrite: `await load()` → `__verter_await_expr(
    // load())`, NO raw `await` anywhere in the render body.
    let source = "<svelte:component this={await load()} label={\"ok\"} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("__verter_dynamic_component((__verter_await_expr(load())))"),
        "the dynamic-component `this` await routes through the helper: {body}"
    );
    // No raw `await` keyword survives in the render body (render stays sync).
    assert!(
        !body.contains("await load") && !body.contains("await "),
        "no raw `await` keyword leaks into the sync render body: {body}"
    );
    assert!(
        !projection.code.contains("async function __verter_render"),
        "`__verter_render` must stay sync: {}",
        projection.code
    );
    // The informational diagnostic is emitted for THIS markup position too.
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "the dynamic-component `this` await records the informational diagnostic: {:?}",
        projection.diagnostics
    );
}

#[test]
fn dynamic_component_this_await_diagnostic_is_informational() {
    // The await-experimental diagnostic at the dynamic-component `this` position is
    // INFORMATIONAL (a hint), not an error — same severity as every other markup
    // await position.
    let source = "<svelte:component this={await load()} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let diag = projection
        .diagnostics
        .iter()
        .find(|d| d.code == "svelte-await-experimental")
        .expect("the dynamic-component this await diagnostic is present");
    assert_eq!(
        diag.severity,
        DiagnosticSeverity::Information,
        "the dynamic-component `this` await diagnostic is INFORMATIONAL"
    );
}

#[test]
fn const_tag_await_value_rewrites_through_the_helper_and_render_stays_sync() {
    // F6: a markup `await` in a `{@const x = await load()}` declaration VALUE is a
    // markup-expression position — the hoisted inner is text-rewritten, so the
    // await must route the helper. NO raw `await` survives (render stays sync).
    let source = "<div>{@const c = await load()}<span>{c}</span></div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.code.contains("__verter_await_expr(load())"),
        "the `{{@const}}` await value routes through the helper: {}",
        projection.code
    );
    // No raw `await` keyword survives AFTER the prelude (the prelude's own helper
    // doc-comments legitimately mention `await`; the hoisted value + render body
    // must be raw-await-free since `__verter_render` stays sync).
    let body = after_prelude(&projection.code);
    assert!(
        !body.contains("await "),
        "no raw `await` keyword leaks into the hoisted value / render body: {body}"
    );
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "the `{{@const}}` await records the informational diagnostic: {:?}",
        projection.diagnostics
    );
}

#[test]
fn dynamic_component_with_children_wraps_them_in_the_component_local() {
    // A non-self-closing `<svelte:component this={C}>CHILDREN</svelte:component>`
    // renders CHILDREN under the synthesized component local; no close residue.
    let source = "<svelte:component this={Comp}>{value}</svelte:component>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        !body.contains("svelte:component"),
        "no open/close svelte:component residue: {body}"
    );
    assert!(body.contains("{value}"), "child interpolation kept: {body}");
}

#[test]
fn self_reference_projects_against_the_local_self_contract() {
    // F8: `<svelte:self prop={x} />` checks against a LOCAL self-component
    // contract derived from the current component's own props — NO metadata
    // resolution. It routes through the same dynamic-component helper over a
    // synthesized self value typed by `__VerterSelfProps`.
    let source = "<script lang=\"ts\">\n\
         interface Props { count: number }\n\
         let { count }: Props = $props();\n\
         </script>\n\
         <svelte:self count={count} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.diagnostics.is_empty(),
        "svelte:self is now supported (no diagnostic): {:?}",
        projection.diagnostics
    );
    let body = render_body(&projection.code);
    assert!(
        !body.contains("svelte:self"),
        "no svelte:self residue: {body}"
    );
    // The self contract is a module-scope type derived from the props
    // annotation (LOCAL, no resolver), and the self value flows through the
    // dynamic-component helper.
    assert!(
        projection.code.contains("__VerterSelfProps"),
        "the local self-props contract type is emitted: {}",
        projection.code
    );
    assert!(
        body.contains("__verter_dynamic_component"),
        "self routes through the dynamic-component helper: {body}"
    );
}

#[test]
fn fragment_children_are_unwrapped_transparently() {
    // F9: `<svelte:fragment slot="x">…</svelte:fragment>` projects its children
    // UNWRAPPED (transparent like `{#key}`); the `slot` literal is void-checked;
    // no `<svelte:fragment` residue, no out-of-scope diagnostic.
    let source = "<svelte:fragment slot=\"footer\"><span>{label}</span></svelte:fragment>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection.diagnostics.is_empty(),
        "svelte:fragment is now supported (no diagnostic): {:?}",
        projection.diagnostics
    );
    let body = render_body(&projection.code);
    assert!(
        !body.contains("svelte:fragment"),
        "no svelte:fragment open/close residue: {body}"
    );
    // Children survive transparently.
    assert!(body.contains("<span>"), "child element kept: {body}");
    assert!(body.contains("{label}"), "child interpolation kept: {body}");
    // The slot literal is void-checked (preserved as a checked value).
    assert!(
        body.contains("__verter_void(\"footer\")"),
        "slot literal void-checked: {body}"
    );
    // The retired legacy-fragment diagnostic is gone.
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-legacy-fragment"),
        "the legacy-fragment diagnostic is retired"
    );
}

#[test]
fn fragment_with_dynamic_slot_void_checks_the_expression() {
    // A dynamic `slot={name}` is void-checked (the expression stays mapped).
    let source = "<svelte:fragment slot={name}>x</svelte:fragment>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("__verter_void(name)"),
        "dynamic slot expression void-checked: {body}"
    );
    assert!(!body.contains("svelte:fragment"), "no residue: {body}");
}

#[test]
fn svelte_options_svg_namespace_selects_the_svg_pragma_and_strips_the_element() {
    // F10: `<svelte:options namespace="svg">` selects the svg shim entrypoint
    // via the per-file `@jsxImportSource` pragma and STRIPS the options element
    // (compiler metadata, no JSX surface).
    let source = "<svelte:options namespace=\"svg\" />\n<svg><circle r={5} /></svg>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .code
            .starts_with("/** @jsxImportSource @verter/svelte-jsx/svg */"),
        "svg pragma variant leads: {}",
        &projection.code[..projection.code.len().min(80)]
    );
    let body = render_body(&projection.code);
    assert!(
        !body.contains("svelte:options"),
        "the options element is stripped: {body}"
    );
    assert!(body.contains("<svg>"), "the svg markup survives: {body}");
}

#[test]
fn svelte_options_mathml_namespace_selects_the_mathml_pragma() {
    // F10: `<svelte:options namespace="mathml">` selects the mathml shim
    // entrypoint pragma variant.
    let source = "<svelte:options namespace=\"mathml\" />\n<math><mrow /></math>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .code
            .starts_with("/** @jsxImportSource @verter/svelte-jsx/mathml */"),
        "mathml pragma variant leads: {}",
        &projection.code[..projection.code.len().min(80)]
    );
}

#[test]
fn default_namespace_keeps_the_base_pragma() {
    // DISCRIMINATING: a component WITHOUT a namespace option keeps the base
    // `@verter/svelte-jsx` pragma (no svg/mathml variant).
    let source = "<div>x</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .code
            .starts_with("/** @jsxImportSource @verter/svelte-jsx */"),
        "base pragma kept: {}",
        &projection.code[..projection.code.len().min(80)]
    );
}

#[test]
fn bound_expression_maps_back_to_the_original_source_byte() {
    // A `bind:value={expr}` bound token maps back to
    // the original source byte through the projection source map — hover /
    // go-to on the bound identifier lands on the original `name`. The bind
    // projection strips only the `bind:` prefix, so the value expression keeps
    // its source span.
    use oxc_sourcemap::SourceMap;

    let source = "<input bind:value={name} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("Comp.svelte"), false);
    assert!(
        !projection.source_map.is_empty(),
        "the projection emits a source map"
    );

    // The original byte offset of the `name` identifier in the SOURCE.
    let src_off = source.find("name").expect("`name` in source") as u32;
    let (src_line, src_col) = byte_offset_to_line_col(source, src_off);

    // The byte offset of `name` in the projected OUTPUT — the occurrence in the
    // render body (the prelude declares `name?` params, so scope to after the
    // render fn header).
    let render_start = projection
        .code
        .find("function __verter_render()")
        .expect("render fn present");
    let out_off = (render_start
        + projection.code[render_start..]
            .find("name")
            .expect("`name` in render body")) as u32;
    let (out_line, out_col) = byte_offset_to_line_col(&projection.code, out_off);

    let map = SourceMap::from_json_string(&projection.source_map).expect("decode map");
    // The CodeTransform map is chunk-granular: the bound-value Original chunk
    // (`value={name}`) maps as a UNIT to its source origin, so hover within it
    // interpolates by the within-chunk offset. Find the covering token (the
    // token at or immediately before the output `name`), assert it sits on the
    // SAME source line, and that applying the offset-preserving within-chunk
    // delta lands EXACTLY on the original `name` byte — i.e. the bound
    // expression kept its source span and maps back byte-accurately (the bind
    // projection stripped only the `bind:` prefix, never relocated the value).
    let token = map
        .get_tokens()
        .filter(|t| t.get_dst_line() == out_line && t.get_dst_col() <= out_col)
        .max_by_key(|t| t.get_dst_col())
        .expect("a token covering the bound identifier");
    assert_eq!(
        token.get_src_line(),
        src_line,
        "the bound token maps to the original source line"
    );
    let delta = out_col - token.get_dst_col();
    assert_eq!(
        token.get_src_col() + delta,
        src_col,
        "the bound `name` maps back byte-accurately to the original source \
         (token src {}:{} + dst delta {delta} = expected src col {src_col})",
        token.get_src_line(),
        token.get_src_col()
    );
}

/// Convert a byte offset to a zero-based (line, column) pair (UTF-16-agnostic
/// for the ASCII fixtures here — columns are byte columns).
fn byte_offset_to_line_col(text: &str, offset: u32) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if i as u32 >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[test]
fn empty_template_still_produces_a_valid_module() {
    let code = project("<script lang=\"ts\">export let x: number;</script>");
    assert!(code.contains("export let x: number;"));
    assert!(code.contains("function __verter_render()"));
}

#[test]
fn function_binding_comma_scanner_skips_string_literals() {
    // A comma INSIDE a string literal in a single-getter `bind:x`
    // value must NOT be mistaken for the `get, set` function-binding separator —
    // `bind:value={pick("a,b")}` is a plain value binding, not a function
    // binding. (If it were misread as a function binding it would project
    // through `__verter_bind_fn`; as a plain value it stays `value={…}`.)
    let code = project("<input bind:value={pick(\"a,b\")} />");
    let body = render_body(&code);
    assert!(
        !body.contains("__verter_bind_fn"),
        "a comma inside a string literal is not the get/set separator: {body}"
    );
    assert!(
        body.contains("value={pick(\"a,b\")}"),
        "the plain value binding is kept: {body}"
    );
}

#[test]
fn self_props_contract_uses_the_annotation_form() {
    // The LOCAL self-props contract derives from a `: Type = $props()`
    // annotation (no resolver).
    let source = "<script lang=\"ts\">\n\
         type P = { a: number };\n\
         let { a }: P = $props();\n\
         </script>\n\
         <svelte:self a={a} />";
    let code = project(source);
    assert!(
        code.contains("type __VerterSelfProps = P;"),
        "self-props contract uses the annotation type: {code}"
    );
}

#[test]
fn self_props_contract_uses_the_generic_form() {
    // The generic `$props<T>()` form contributes the type argument.
    let source = "<script lang=\"ts\">\n\
         let props = $props<{ b: string }>();\n\
         </script>\n\
         <svelte:self b={props.b} />";
    let code = project(source);
    assert!(
        code.contains("type __VerterSelfProps = { b: string };"),
        "self-props contract uses the generic argument: {code}"
    );
}

#[test]
fn self_props_contract_degrades_to_permissive_when_untyped() {
    // An untyped `$props()` (no annotation, no generic) → a permissive
    // `Record<string, unknown>` contract (no resolver, no crash).
    let source = "<script lang=\"ts\">\n\
         let props = $props();\n\
         </script>\n\
         <svelte:self />";
    let code = project(source);
    assert!(
        code.contains("type __VerterSelfProps = Record<string, unknown>;"),
        "untyped $props() degrades to a permissive self contract: {code}"
    );
}

#[test]
fn self_props_contract_ignores_a_member_call_props_before_the_real_rune() {
    // P1: an earlier `$props.id()` member call (NOT the props rune) must NOT
    // poison the contract — the SYNTACTIC scan binds the real `$props()` call's
    // declarator annotation, so a wrong self-prop still FAILS against `Props`.
    let source = "<script lang=\"ts\">\n\
         const id = $props.id();\n\
         interface Props { count: number }\n\
         let { count }: Props = $props();\n\
         </script>\n\
         <svelte:self count={count} />";
    let code = project(source);
    assert!(
        code.contains("type __VerterSelfProps = Props;"),
        "the real `$props()` declarator annotation is used, not `$props.id()`: {code}"
    );
    assert!(
        !code.contains("type __VerterSelfProps = Record<string, unknown>;"),
        "a member-call `$props.id()` must not degrade the contract: {code}"
    );
}

#[test]
fn self_props_contract_ignores_props_inside_a_string_or_comment() {
    // P1: a `$props` substring inside a string literal or comment is NOT the
    // rune call — the SYNTACTIC scan skips it and binds the real annotated call.
    let source = "<script lang=\"ts\">\n\
         const note = \"call $props() here\"; // also $props in a comment\n\
         type P = { a: number };\n\
         let { a }: P = $props();\n\
         </script>\n\
         <svelte:self a={a} />";
    let code = project(source);
    assert!(
        code.contains("type __VerterSelfProps = P;"),
        "a `$props` inside a string/comment must not pre-empt the real rune: {code}"
    );
}

#[test]
fn fragment_static_slot_literal_is_js_escaped() {
    // P1: a single-quoted slot value containing a double quote must JS-escape
    // when injected into the void-check so the projected TSX stays VALID — a raw
    // `"foo"bar"` double-quote wrap is invalid TSX.
    let source = "<svelte:fragment slot='foo\"bar'>x</svelte:fragment>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("__verter_void(\"foo\\\"bar\")"),
        "the slot literal is JS-escaped (valid TSX string), not raw-wrapped: {body}"
    );
    assert!(
        !body.contains("__verter_void(\"foo\"bar\")"),
        "no invalid raw double-quote wrap: {body}"
    );
}

#[test]
fn fragment_static_slot_literal_with_backslash_is_js_escaped() {
    // P1: a backslash in a static slot value must also escape so the void-check
    // string is valid TSX.
    let source = "<svelte:fragment slot=\"a\\b\">x</svelte:fragment>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("__verter_void(\"a\\\\b\")"),
        "the backslash is escaped: {body}"
    );
}

#[test]
fn fragment_close_tag_inside_a_descendant_string_literal_is_not_mistaken_for_the_real_close() {
    // P1: a child interpolation containing the LITERAL text `</svelte:fragment>`
    // inside a string must NOT be mistaken for the element's real close tag. The
    // parser is the close-tag authority (its child walk is string/brace-aware);
    // the projector reads the parser-recorded close span — a literal-unaware
    // source byte-scan would close at the in-string occurrence, swallowing the
    // real children + the `<div>after</div>` sibling and corrupting the string.
    let source = "<svelte:fragment slot=\"a\">{\"x </svelte:fragment> y\"}<span>{tail}</span></svelte:fragment><div>after</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    // (a) No `<svelte:fragment` open-tag residue (the open tag was overwritten by
    //     the void-check). The string `svelte:fragment` only survives INSIDE the
    //     preserved descendant string literal, never as element syntax.
    assert!(
        !body.contains("<svelte:fragment"),
        "no svelte:fragment open-tag residue: {body}"
    );
    // The REAL close tag was removed — the ONLY `</svelte:fragment>` left is the
    // one inside the preserved string literal (exactly one occurrence).
    assert_eq!(
        body.matches("</svelte:fragment>").count(),
        1,
        "exactly one `</svelte:fragment>` survives — the in-string literal, not a \
         structural close-tag residue: {body}"
    );
    // (b) The real children survived and the sibling `<div>after</div>` was NOT
    //     swallowed by an in-string close match.
    assert!(body.contains("<span>"), "child span kept: {body}");
    assert!(body.contains("{tail}"), "child interpolation kept: {body}");
    assert!(
        body.contains("<div>after</div>"),
        "the sibling AFTER the real close tag is preserved (not swallowed): {body}"
    );
    // (c) The user's string `"x </svelte:fragment> y"` is preserved un-corrupted
    //     (the literal-unaware byte-scan would splice the close tag out of it).
    assert!(
        body.contains("\"x </svelte:fragment> y\""),
        "the descendant string literal is preserved un-corrupted: {body}"
    );
}

#[test]
fn component_close_tag_inside_a_descendant_string_literal_is_not_mistaken_for_the_real_close() {
    // P1 (shared scanner): the close-tag span read also drives `<svelte:component>`
    // close-tag NAME rewrite. A `</svelte:component>` inside a descendant string
    // must not be rewritten — the rewrite must land on the REAL close tag, and
    // the in-string text must stay verbatim.
    let source = "<svelte:component this={Dyn}>{\"q </svelte:component> r\"}<span>{x}</span></svelte:component><div>tail</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    // No `<svelte:component` open-tag residue (open rewritten to the dynamic
    // local). The string only survives inside the preserved literal.
    assert!(
        !body.contains("<svelte:component"),
        "no svelte:component open-tag residue: {body}"
    );
    // The REAL close tag was rewritten to the dynamic local — the only
    // `</svelte:component>` left is the in-string literal (exactly one).
    assert_eq!(
        body.matches("</svelte:component>").count(),
        1,
        "exactly one `</svelte:component>` survives — the in-string literal, not a \
         structural close-tag residue: {body}"
    );
    // The real close was rewritten to the dynamic local carrier.
    assert!(
        body.contains("</__VerterDyn>"),
        "the REAL close tag was rewritten to the dynamic local: {body}"
    );
    // The in-string text stays verbatim (the rewrite did NOT corrupt it).
    assert!(
        body.contains("\"q </svelte:component> r\""),
        "descendant string literal preserved un-corrupted: {body}"
    );
    // The sibling after the real close survives.
    assert!(
        body.contains("<div>tail</div>"),
        "sibling after the real close is preserved: {body}"
    );
}

// --- F11 store auto-subscription + F12 legacy magic objects ---

/// Everything AFTER the ambient prelude — the hoisted script bodies + the render
/// fn. The prelude's own doc comments / `declare`s reference the F11 helper names
/// and the `$$`-magic names, so store/magic assertions must target the projected
/// USER code, not the prelude. The F11 `__verter_store_update` declaration is the
/// LAST F11-helper prelude line (its body mentions `__verter_store_get`, so the
/// anchor must sit AFTER it); the legacy-magic block, when present, follows it but
/// carries no store-rewrite tokens — only `declare const $$…` declarations the
/// magic-decl tests check on the full `code`.
fn after_prelude(code: &str) -> &str {
    let marker = "declare function __verter_store_update";
    match code
        .find(marker)
        .and_then(|i| code[i..].find('\n').map(|j| i + j + 1))
    {
        Some(start) => &code[start..],
        None => code,
    }
}

#[test]
fn store_read_in_script_rewrites_only_the_dollar_byte() {
    // F11: a `$count` READ in the script body becomes `__verter_store_get(count)`
    // — the `$` byte is overwritten, the `count` identifier bytes are preserved
    // (so hover lands on the original identifier). NO `$count` residue.
    let body = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0); const v = $count + 1;</script>\n<div>{v}</div>",
    );
    assert!(
        body.contains("__verter_store_get(count)"),
        "the read sub is rewritten to the store-get helper preserving `count`: {body}"
    );
    assert!(
        !body.contains("$count"),
        "no `$count` residue (the `$` byte was rewritten): {body}"
    );
}

#[test]
fn store_write_in_script_rewrites_dollar_and_equals_only() {
    // F11: a `$count = 5` WRITE becomes `__verter_store_set(count, 5)` — `$` and
    // the `=` operator are overwritten, the `count` identifier and the `5` RHS
    // bytes are preserved. NO `$count` residue.
    let body = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0); $count = 5;</script>\n<div>x</div>",
    );
    // The `count` identifier + `5` RHS bytes are preserved (whitespace around the
    // rewritten `=`→`,` is the original source whitespace).
    assert!(
        body.contains("__verter_store_set(count") && body.contains(", 5)"),
        "the write sub is rewritten to the store-set helper preserving `count`/`5`: {body}"
    );
    assert!(!body.contains("$count"), "no `$count` residue: {body}");
}

#[test]
fn store_read_in_markup_interpolation_rewrites() {
    // F11: a `{$count}` markup interpolation rewrites the same way.
    let body = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0);</script>\n<div>{$count}</div>",
    );
    assert!(
        body.contains("__verter_store_get(count)"),
        "a markup `{{$count}}` rewrites to the store-get helper: {body}"
    );
    assert!(
        !render_body(&body).contains("$count"),
        "no `$count` residue in body: {body}"
    );
}

#[test]
fn runes_are_not_rewritten_as_store_subs() {
    // DISCRIMINATING negative: `$state`/`$props`/`$derived` rune call sites stay
    // VERBATIM (the prelude types them) — they must NOT be rewritten as store
    // subs (no `__verter_store_get($state`).
    let code = project(
        "<script lang=\"ts\">let s = $state(0); const p = $props(); let d = $derived(s);</script>\n<div>{s}</div>",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains("$state(0)"),
        "the $state rune call stays verbatim: {body}"
    );
    assert!(
        body.contains("$props()"),
        "the $props rune call stays verbatim: {body}"
    );
    assert!(
        body.contains("$derived(s)"),
        "the $derived rune call stays verbatim: {body}"
    );
    assert!(
        !body.contains("__verter_store_get"),
        "no rune was rewritten as a store-sub: {body}"
    );
    assert!(
        !body.contains("__verter_store_set"),
        "no store-set was emitted for a rune: {body}"
    );
}

#[test]
fn double_dollar_magic_is_not_rewritten_as_store_subs() {
    // DISCRIMINATING negative: `$$props`/`$$slots` magic stays VERBATIM — they
    // are F12 prelude declarations, NEVER store subs.
    let code = project(
        "<script lang=\"ts\">const a = $$props; const has = $$slots.foo;</script>\n<div>{a}</div>",
    );
    let body = after_prelude(&code);
    // The USER's verbatim magic references survive (discriminating substrings not
    // present in the prelude `declare const` lines).
    assert!(
        body.contains("const a = $$props"),
        "the $$props magic stays verbatim: {body}"
    );
    assert!(
        body.contains("const has = $$slots.foo"),
        "the $$slots magic stays verbatim: {body}"
    );
    assert!(
        !body.contains("__verter_store_get") && !body.contains("__verter_store_set"),
        "no $$-magic was rewritten as a store-sub: {body}"
    );
}

#[test]
fn a_local_dollar_binding_is_not_rewritten() {
    // DISCRIMINATING negative: a local `let $x` binding is an ordinary variable
    // — its references stay verbatim (NOT a store-sub).
    let code =
        project("<script lang=\"ts\">let $x = 1; const y = $x + 1;</script>\n<div>{y}</div>");
    let body = after_prelude(&code);
    assert!(
        body.contains("let $x = 1"),
        "the local binding stays verbatim: {body}"
    );
    assert!(
        body.contains("$x + 1"),
        "the local reference stays verbatim: {body}"
    );
    assert!(
        !body.contains("__verter_store_get") && !body.contains("__verter_store_set"),
        "a local `$x` binding must NOT be rewritten as a store-sub: {body}"
    );
}

#[test]
fn legacy_component_emits_the_magic_object_declarations() {
    // F12: a legacy (no-rune) component's prelude declares `$$props`/
    // `$$restProps`/`$$slots`.
    let code = project("<div>{$$props.x}</div>");
    assert!(
        code.contains("declare const $$props: Record<string, any>"),
        "{code}"
    );
    assert!(
        code.contains("declare const $$restProps: Record<string, any>"),
        "{code}"
    );
    assert!(
        code.contains("declare const $$slots: Record<string, boolean>"),
        "{code}"
    );
}

#[test]
fn a_runes_component_omits_the_legacy_magic_object_declarations() {
    // F12: a runes-mode component (uses a rune) is NOT legacy — the F12 magic
    // declarations are OMITTED (they do not exist in runes mode; emitting their
    // loose `any` surface would pollute a runes-mode file).
    let code = project("<script lang=\"ts\">let s = $state(0);</script>\n<div>{s}</div>");
    assert!(
        !code.contains("declare const $$props"),
        "a runes-mode component must NOT carry the legacy magic declarations: {code}"
    );
}

#[test]
fn store_sub_identifier_maps_back_to_the_original_source_byte() {
    // F11 sourcemap e2e: a rewritten `$store` keeps the `store` identifier bytes
    // as an Original chunk (only the `$` byte was overwritten), so hover /
    // go-to-definition on the projected `store` lands on the ORIGINAL `store`
    // identifier byte in the source. This is the sourcemap-accuracy guarantee of
    // the `$`-span-only rewrite.
    use oxc_sourcemap::SourceMap;

    let source =
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0); const v = $count;</script>\n<div>{v}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("Comp.svelte"), false);
    assert!(
        !projection.source_map.is_empty(),
        "the projection emits a source map"
    );

    // The original `count` identifier byte — the byte right AFTER the `$` in the
    // `$count` READ (`const v = $count`). The `$` precedes it; the identifier
    // `count` keeps its source span across the rewrite.
    let read_at = source.find("$count").expect("$count read in source");
    let src_off = (read_at + 1) as u32; // skip the `$`
    let (src_line, src_col) = byte_offset_to_line_col(source, src_off);

    // The `store` identifier in the projected output — inside
    // `__verter_store_get(count)`.
    let wrap_at = projection
        .code
        .find("__verter_store_get(count)")
        .expect("the read was rewritten to the store-get helper");
    let out_off = (wrap_at + "__verter_store_get(".len()) as u32;
    let (out_line, out_col) = byte_offset_to_line_col(&projection.code, out_off);

    let map = SourceMap::from_json_string(&projection.source_map).expect("decode map");
    // The `count` Original chunk maps as a unit to its source origin; find the
    // covering token and assert the within-chunk delta lands EXACTLY on the
    // original `count` byte (the rewrite touched only the `$` span — the
    // identifier kept its source position).
    let token = map
        .get_tokens()
        .filter(|t| t.get_dst_line() == out_line && t.get_dst_col() <= out_col)
        .max_by_key(|t| t.get_dst_col())
        .expect("a token covering the store identifier");
    assert_eq!(
        token.get_src_line(),
        src_line,
        "the store identifier maps to the original source line"
    );
    let delta = out_col - token.get_dst_col();
    assert_eq!(
        token.get_src_col() + delta,
        src_col,
        "the rewritten `$store` maps back byte-accurately to the original `store` \
         identifier (token src {}:{} + dst delta {delta} = expected src col {src_col})",
        token.get_src_line(),
        token.get_src_col()
    );
}

#[test]
fn store_sub_in_block_condition_and_attribute_is_rewritten() {
    // F11: store-subs in markup expression positions BEYOND the bare
    // interpolation — an `{#if $on}` condition, an `{#each $items as x}` iterable,
    // and a plain attribute value `data-x={$flag}` — are all rewritten.
    let if_body = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const on = writable(true);</script>\n<div>{#if $on}<span>y</span>{/if}</div>",
    );
    assert!(
        if_body.contains("__verter_store_get(on)"),
        "the `{{#if $on}}` condition store-sub is rewritten: {if_body}"
    );

    let each_body = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const items = writable([1]);</script>\n<ul>{#each $items as it}<li>{it}</li>{/each}</ul>",
    );
    assert!(
        each_body.contains("__verter_store_get(items)"),
        "the `{{#each $items}}` iterable store-sub is rewritten: {each_body}"
    );

    let attr_body = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const flag = writable(\"x\");</script>\n<div data-x={$flag}>y</div>",
    );
    assert!(
        attr_body.contains("__verter_store_get(flag)"),
        "the `data-x={{$flag}}` attribute store-sub is rewritten: {attr_body}"
    );
}

#[test]
fn store_sub_in_a_trailing_moved_script_is_rewritten_in_place() {
    // A TRAILING `<script>` (after the markup) is MOVED above the render fn. The
    // store rewrite runs BEFORE that move, so the store-sub is rewritten IN PLACE
    // (the rewritten chunk moves WITH the body) — NOT dropped, and no stray
    // `__verter_store_get()` chunk is appended at the output end.
    let body = project(
        "<div>{x}</div>\n<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0); const x = $count;</script>",
    );
    assert!(
        body.contains("const x = __verter_store_get(count);"),
        "the trailing-script store-sub is rewritten in the moved body: {body}"
    );
    assert!(
        !body.contains("$count"),
        "no raw `$count` survives the move: {body}"
    );
    assert!(
        !body.contains("__verter_store_get()"),
        "no stray empty store-get chunk is stranded at the output end: {body}"
    );
}

#[test]
fn a_script_declared_dollar_binding_is_not_rewritten_in_markup() {
    // DISCRIMINATING (cross-fragment lexical scope): a `let $x` declared in the
    // SCRIPT makes `{$x}` in the markup an ORDINARY local, NOT a store-sub —
    // even though the markup parses as a separate fragment. The script-declared
    // `$`-names are threaded into the markup scan.
    let code = project("<script lang=\"ts\">let $x = 1;</script>\n<div>{$x}</div>");
    let body = after_prelude(&code);
    assert!(
        !body.contains("__verter_store_get"),
        "a script-declared `$x` must NOT be store-rewritten in markup: {body}"
    );
    // The markup `{$x}` reference stays verbatim.
    assert!(
        body.contains("{$x}"),
        "the markup `$x` reference stays verbatim: {body}"
    );
}

#[test]
fn store_sub_in_a_style_directive_value_is_rewritten() {
    // F11: a store-sub in a void-checked directive value (`style:color={$c}`) is
    // rewritten (no raw `$c` left in the projected void-check).
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const c = writable(\"red\");</script>\n<div style:color={$c}>x</div>",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains("__verter_store_get(c)"),
        "the `style:color={{$c}}` store-sub is rewritten: {body}"
    );
    // Discriminating: NO raw `$c` identifier residue survives anywhere. (The
    // previous `… || body.contains("get(c)")` escape hatch was always-true since
    // the rewrite always emits `get(c)` — it passed even with raw `$c` residue.)
    assert!(!body.contains("$c"), "no raw `$c` residue: {body}");
}

#[test]
fn a_compound_store_assignment_projects_a_writable_read_set() {
    // F11: `$count += 1` → `__verter_store_set(count, __verter_store_get(count) +
    // (1))` — a Writable-checked read+set (NOT the invalid `__verter_store_get(
    // count) += 1`). The original `count` keeps its source span; the second
    // occurrence is injected read machinery.
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0); $count += 1;</script>\n<div>x</div>",
    );
    let body = after_prelude(&code);
    // Whitespace around the rewritten operator is the original source whitespace.
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        normalized.contains("__verter_store_set(count , __verter_store_get(count) + ( 1))")
            || normalized.contains("__verter_store_set(count, __verter_store_get(count) + (1))"),
        "a compound store-assignment projects a writable read+set: {body}"
    );
    assert!(
        !body.contains("__verter_store_get(count) +="),
        "the compound target is NOT a bare read-wrap (invalid TS): {body}"
    );
    assert!(!body.contains("$count"), "no raw `$count` residue: {body}");
}

#[test]
fn an_update_store_expression_projects_a_writable_read_set() {
    // F11: `$count++` → `__verter_store_set(count, __verter_store_get(count) + 1)`.
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0); $count++;</script>\n<div>x</div>",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains(
            "__verter_store_set(count, __verter_store_update(__verter_store_get(count)))"
        ),
        "an update store-expression projects a writable update read+set: {body}"
    );
    assert!(!body.contains("$count"), "no raw `$count` residue: {body}");
}

#[test]
fn forced_runes_option_omits_the_legacy_magic_even_without_a_rune_call() {
    // F12: an explicit `<svelte:options runes={true}>` forces RUNES mode even when
    // the script uses NO rune — the legacy `$$props`/`$$restProps`/`$$slots` magic
    // (and its `any`) must NOT be emitted.
    let code = project(
        "<svelte:options runes={true} />\n<script lang=\"ts\">let x = 1;</script>\n<div>{x}</div>",
    );
    assert!(
        !code.contains("declare const $$props"),
        "a forced-runes component must NOT carry the legacy magic: {code}"
    );
    // DISCRIMINATING: the SAME script WITHOUT the forced-runes option is legacy
    // (no rune used) and DOES carry the magic.
    let legacy = project("<script lang=\"ts\">let x = 1;</script>\n<div>{x}</div>");
    assert!(
        legacy.contains("declare const $$props"),
        "the same script without the forced-runes option is legacy: {legacy}"
    );
}

#[test]
fn store_subs_in_tag_await_and_legacy_on_surfaces_are_rewritten() {
    // F11: store-subs in `{@html $x}`, `{#await $p}`, and a legacy
    // `on:click={$h}` value are all rewritten (no raw `$store` residue in those
    // projected positions).
    let html = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const m = writable(\"\");</script>\n<div>{@html $m}</div>",
    );
    assert!(
        after_prelude(&html).contains("__verter_store_get(m)"),
        "the @html tag store-sub is rewritten: {html}"
    );

    let awaited = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const p = writable(Promise.resolve(1));</script>\n<div>{#await $p}loading{:then v}{v}{/await}</div>",
    );
    assert!(
        after_prelude(&awaited).contains("__verter_store_get(p)"),
        "the #await head store-sub is rewritten: {awaited}"
    );

    let on = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const h = writable(() => {});</script>\n<button on:click={$h}>x</button>",
    );
    assert!(
        after_prelude(&on).contains("__verter_store_get(h)"),
        "the legacy on: handler store-sub is rewritten: {on}"
    );
}

#[test]
fn object_shorthand_store_sub_inserts_the_key() {
    // F11: a shorthand `{ $count }` store-sub becomes
    // `{ $count: __verter_store_get(count) }` (the key is inserted) — NOT the
    // invalid bare `{ __verter_store_get(count) }`.
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0); const o = { $count };</script>\n<div>{o}</div>",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains("{ $count: __verter_store_get(count) }"),
        "the shorthand store-sub inserts the key: {body}"
    );
    assert!(
        !body.contains("{ __verter_store_get(count) }"),
        "no invalid bare-call shorthand slot: {body}"
    );
}

#[test]
fn store_sub_in_a_bind_value_is_rewritten() {
    // F11 (P1-1): a store-sub in a `bind:value={$store}` value is rewritten. The
    // value stays a mapped chunk (the `bind:` prefix is stripped), so the
    // `$`-span overwrite composes with the strip.
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const name = writable(\"\");</script>\n<input bind:value={$name} />",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains("value={__verter_store_get(name)}"),
        "the bind:value store-sub is rewritten + the bind prefix stripped: {body}"
    );
    assert!(
        !body.contains("bind:value") && !body.contains("$name"),
        "no `bind:` residue and no raw `$name`: {body}"
    );
}

#[test]
fn store_sub_in_a_function_binding_value_is_rewritten() {
    // F11 (P1-1): a store-sub in an F5 function-binding `get, set` pair
    // (`bind:value={() => $name, v => v}`) is rewritten — the mapped pair composes
    // with the `__verter_bind_fn(` wrap.
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const name = writable(\"\");</script>\n<input bind:value={() => $name, (v) => { void v; }} />",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains("__verter_store_get(name)") && body.contains("__verter_bind_fn"),
        "the function-binding store-sub is rewritten inside the bind-fn wrap: {body}"
    );
}

#[test]
fn store_sub_in_a_declaration_tag_value_is_rewritten_move_safely() {
    // F11 (P1-2): a store-sub in a `{@const x = $store}` value is rewritten
    // MOVE-SAFELY — the hoisted `const x = __verter_store_get(count)` carries the
    // store-get + closing paren WITH the move (no stranded `)` at the original,
    // now-removed tag position).
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0);</script>\n<div>{@const doubled = $count}{doubled}</div>",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains("const doubled = __verter_store_get(count)"),
        "the @const store value is rewritten move-safely (paren travels with the \
         hoist): {body}"
    );
    assert!(
        !body.contains("$count"),
        "no raw `$count` survives the move: {body}"
    );
    assert!(
        !body.contains("__verter_store_get(count));") && !body.contains("__verter_store_get()"),
        "no stranded / empty store-get chunk at the original tag position: {body}"
    );
}

#[test]
fn store_sub_in_a_declaration_tag_with_trailing_store_is_rewritten_move_safely() {
    // F11 (P1-2): the stranding-sensitive case — the store is the LAST token of
    // the `{@const}` inner (`$count` ends the inner). The closing `)` must travel
    // with the move, not strand at the original boundary.
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0);</script>\n<div>{@const c = $count}{c}</div>",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains("const c = __verter_store_get(count)"),
        "the trailing-store @const value is rewritten with the paren moved: {body}"
    );
    assert!(
        !body.contains("$count") && !body.contains("__verter_store_get()"),
        "no raw `$count` and no stranded empty get: {body}"
    );
}

#[test]
fn store_sub_in_a_declaration_tag_with_multiple_and_nested_subs_is_rewritten() {
    // F11 (P1-2): the text-path @const rewrite reuses the CodeTransform ops (not
    // hand-rolled offset arithmetic), so MULTIPLE subs and a NESTED store-read
    // inside a store-write both rewrite correctly (no offset drift / overlap bug).
    // Two reads:
    let two = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const a = writable(1); const b = writable(2);</script>\n<div>{@const s = $a + $b}{s}</div>",
    );
    let two_body = after_prelude(&two);
    assert!(
        two_body.contains("const s = __verter_store_get(a) + __verter_store_get(b)")
            && !two_body.contains("$a")
            && !two_body.contains("$b"),
        "two @const reads both rewrite: {two_body}"
    );
}

#[test]
fn store_sub_in_a_dynamic_component_this_is_rewritten() {
    // F11 (P1-3): a store-sub in `<svelte:component this={$Cmp}>` is rewritten —
    // the F8 IIFE re-emits the `this` value as text, so the store-get is spliced
    // into the interpolated component expression.
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const Cmp = writable<any>(null);</script>\n<svelte:component this={$Cmp} />",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains("__verter_dynamic_component((__verter_store_get(Cmp)))"),
        "the dynamic-component `this={{$Cmp}}` store-sub is rewritten in the IIFE: \
         {body}"
    );
    assert!(!body.contains("$Cmp"), "no raw `$Cmp` residue: {body}");
}

#[test]
fn store_sub_in_a_bind_value_maps_back_to_the_original_source_byte() {
    // F11 (P1-1) sourcemap accuracy: the `bind:value={$store}` rewrite keeps the
    // `store` identifier bytes as an Original chunk (only the `$` byte was
    // overwritten + the `bind:` prefix stripped), so hover on the projected
    // `store` lands on the ORIGINAL identifier byte.
    use oxc_sourcemap::SourceMap;

    let source =
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const name = writable(\"\");</script>\n<input bind:value={$name} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("Comp.svelte"), false);
    assert!(!projection.source_map.is_empty());

    let read_at = source.find("$name}").expect("$name in bind value");
    let src_off = (read_at + 1) as u32; // skip the `$`
    let (src_line, src_col) = byte_offset_to_line_col(source, src_off);

    let wrap_at = projection
        .code
        .find("__verter_store_get(name)")
        .expect("the bind value was rewritten");
    let out_off = (wrap_at + "__verter_store_get(".len()) as u32;
    let (out_line, out_col) = byte_offset_to_line_col(&projection.code, out_off);

    let map = SourceMap::from_json_string(&projection.source_map).expect("decode map");
    let token = map
        .get_tokens()
        .filter(|t| t.get_dst_line() == out_line && t.get_dst_col() <= out_col)
        .max_by_key(|t| t.get_dst_col())
        .expect("a token covering the store identifier in the bind value");
    assert_eq!(token.get_src_line(), src_line);
    let delta = out_col - token.get_dst_col();
    assert_eq!(
        token.get_src_col() + delta,
        src_col,
        "the bind-value `$name` maps back byte-accurately to the original `name`"
    );
}

#[test]
fn member_write_on_a_store_sub_degrades_to_a_read_base() {
    // F11 documented bounded boundary: `$obj.x = 1` projects the BASE `$obj` as a
    // READ (`__verter_store_get(obj).x = 1`) — a relaxed safe-degrade (it mutates
    // the read object's member; valid TSX, does not require `Writable`). NOT a
    // whole-object store set.
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const obj = writable({ x: 0 });</script>\n<div>{(() => { $obj.x = 1; })()}</div>",
    );
    let body = after_prelude(&code);
    assert!(
        body.contains("__verter_store_get(obj).x = 1"),
        "a `$obj.x = 1` member write projects the base as a READ (relaxed \
         safe-degrade): {body}"
    );
    assert!(
        !body.contains("__verter_store_set(obj"),
        "the member write does NOT emit a whole-object store set: {body}"
    );
}

// --- store-subs in bind-TARGET (lvalue) positions (BLOCKER D) ---
//
// A `$store` as a `bind:this` / `bind:group` / readonly-table-bind TARGET is
// invalid Svelte (you bind into a writable LOCAL, never into a store
// auto-subscription). The projector must NEVER leak raw `$`-identifier residue —
// a raw `$store` would surface a phantom `TS2304/2552 Cannot find name '$store'`.
// The store-sub is therefore rewritten to its READ-helper form
// (`__verter_store_get(store)`) so the round-trip assignment (`LOCAL =
// checker(LOCAL)`) is SYNTACTICALLY VALID and surfaces the CORRECT lvalue error
// (assignment to a call result), NOT a phantom name error. These were the three
// previously-`#[ignore]`'d R10 ledger entries — now GREEN (real residue-free
// handling), per the orchestrator no-raw-`$`-residue ruling.

#[test]
fn bind_this_store_target_emits_no_raw_dollar_residue() {
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const el = writable<any>(null);</script>\n<input bind:this={$el} />",
    );
    let body = after_prelude(&code);
    assert!(
        !body.contains("$el"),
        "a `bind:this={{$el}}` target must not leak raw `$el` residue: {body}"
    );
    // The store-sub is rewritten to the read-helper form (valid TSX surfacing a
    // real lvalue error, not a phantom name error).
    assert!(
        body.contains("__verter_store_get(el)"),
        "the `bind:this={{$el}}` target is rewritten to the read helper: {body}"
    );
}

#[test]
fn bind_group_store_target_emits_no_raw_dollar_residue() {
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const g = writable<any>(null);</script>\n<input type=\"checkbox\" bind:group={$g} />",
    );
    let body = after_prelude(&code);
    assert!(
        !body.contains("$g"),
        "a `bind:group={{$g}}` target must not leak raw `$g` residue: {body}"
    );
    assert!(
        body.contains("__verter_store_get(g)"),
        "the `bind:group={{$g}}` target is rewritten to the read helper: {body}"
    );
}

#[test]
fn table_bind_store_target_emits_no_raw_dollar_residue() {
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const w = writable(0);</script>\n<div bind:clientWidth={$w}></div>",
    );
    let body = after_prelude(&code);
    assert!(
        !body.contains("$w"),
        "a `bind:clientWidth={{$w}}` target must not leak raw `$w` residue: {body}"
    );
    assert!(
        body.contains("__verter_store_get(w)"),
        "the `bind:clientWidth={{$w}}` target is rewritten to the read helper: {body}"
    );
}

// --- store-subs vs markup block bindings (BLOCKER A) ---
//
// A `$`-prefixed identifier INTRODUCED by a markup block binding (`{#each … as
// $item}` / `{#await p then $v}` / `{:catch $e}` / `{#snippet n($p)}` / `let:$x`)
// is an ORDINARY local in that block's subtree, NOT a store auto-subscription.
// The classifier must NOT rewrite it to `__verter_store_get(item)` (which leaves
// a raw `item` name reference and invalid TSX). A GENUINE store-sub in the SAME
// template still rewrites. These DISCRIMINATE the block-binding scope split.

#[test]
fn an_each_block_dollar_binding_is_not_store_rewritten() {
    let code = project("<ul>{#each list as $item}<li>{$item}</li>{/each}</ul>");
    let body = render_body(&code);
    assert!(
        !body.contains("__verter_store_get(item)"),
        "a `{{#each list as $item}}` binding must NOT be store-rewritten: {body}"
    );
    assert!(
        body.contains("{$item}"),
        "the `$item` block binding stays a verbatim reference: {body}"
    );
}

#[test]
fn an_each_index_dollar_binding_is_not_store_rewritten() {
    let code = project("<ul>{#each list as item, $i}<li>{$i}</li>{/each}</ul>");
    let body = render_body(&code);
    assert!(
        !body.contains("__verter_store_get(i)"),
        "a `{{#each … as item, $i}}` index binding must NOT be store-rewritten: {body}"
    );
}

#[test]
fn a_destructured_each_dollar_binding_is_not_store_rewritten() {
    let code = project("<ul>{#each rows as { $a, $b }}<li>{$a}{$b}</li>{/each}</ul>");
    let body = render_body(&code);
    assert!(
        !body.contains("__verter_store_get(a)") && !body.contains("__verter_store_get(b)"),
        "a destructured `{{#each rows as {{ $a, $b }}}}` binding must NOT be \
         store-rewritten: {body}"
    );
}

#[test]
fn an_each_destructuring_pattern_close_brace_does_not_strand_the_block_tail() {
    // An `{#each rows as { x }}` DESTRUCTURING pattern contains its OWN `}` — the
    // each-open's closing `}` is AFTER it. The projector must search for the
    // open-close `}` PAST the binding span, else it stops at the pattern's inner
    // `}` and strands a malformed `(<>}` tail. Assert the well-formed
    // `.map(({ x }) => (<>` head with the body directly after (no stray `}`).
    let code = project("<ul>{#each rows as { x }}<li>{x}</li>{/each}</ul>");
    let body = render_body(&code);
    assert!(
        body.contains(".map(({ x }) => (<>") && body.contains("<li>"),
        "the each-destructuring head must be well-formed `.map(({{ x }}) => (<>`: {body}"
    );
    assert!(
        !body.contains("(<>}"),
        "the each-destructuring projection must NOT strand a `}}` after `(<>` (the \
         close-brace search must skip the pattern's inner `}}`): {body}"
    );
}

#[test]
fn an_each_destructuring_store_default_is_read_rewritten_residue_free() {
    // `{#each rows as { x = $store }}` — the binding NAME `x` stays a local, the
    // DEFAULT `$store` is an ordinary READ rewritten to `__verter_store_get(store)`
    // with NO raw `$store` residue and a well-formed head.
    let code = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const store = writable(0);\n\
         </script>\n\
         {#each rows as { x = $store }}<li>{x}</li>{/each}",
    );
    let body = render_body(&code);
    assert!(
        body.contains("__verter_store_get(store)") && !body.contains("$store"),
        "the each block-binding default `$store` must be read-rewritten (no raw \
         `$store`): {body}"
    );
    assert!(
        !body.contains("(<>}"),
        "the each-destructuring-with-default head must be well-formed (no stranded \
         `}}`): {body}"
    );
}

#[test]
fn an_await_then_dollar_binding_is_not_store_rewritten() {
    let code = project("{#await p then $v}<span>{$v}</span>{/await}");
    let body = render_body(&code);
    assert!(
        !body.contains("__verter_store_get(v)"),
        "a `{{#await p then $v}}` binding must NOT be store-rewritten: {body}"
    );
}

#[test]
fn an_await_catch_dollar_binding_is_not_store_rewritten() {
    let code = project("{#await p}<span>x</span>{:catch $e}<span>{$e}</span>{/await}");
    let body = render_body(&code);
    assert!(
        !body.contains("__verter_store_get(e)"),
        "a `{{:catch $e}}` binding must NOT be store-rewritten: {body}"
    );
}

#[test]
fn a_snippet_param_dollar_binding_is_not_store_rewritten() {
    // The snippet body is HOISTED to module scope (before the render fn), so
    // assert against the post-prelude code (which includes the hoisted
    // declarator), not just the render body.
    let code = project("{#snippet row($item)}<li>{$item}</li>{/snippet}");
    let body = after_prelude(&code);
    assert!(
        !body.contains("__verter_store_get(item)"),
        "a `{{#snippet row($item)}}` param must NOT be store-rewritten: {body}"
    );
    assert!(
        body.contains("{$item}"),
        "the `$item` snippet param stays a verbatim reference: {body}"
    );
}

#[test]
fn a_let_directive_dollar_binding_is_not_store_rewritten() {
    let code = project("<Comp let:item={$row}>{$row}</Comp>");
    let body = render_body(&code);
    assert!(
        !body.contains("__verter_store_get(row)"),
        "a `let:item={{$row}}` slot-prop binding must NOT be store-rewritten: {body}"
    );
}

#[test]
fn a_real_store_sub_in_a_block_with_a_dollar_binding_still_rewrites() {
    // DISCRIMINATING: the `$item` each-binding is a local (NOT rewritten), while
    // a genuine `$count` store-sub in the SAME each body IS rewritten. The
    // block-binding scope must not over-suppress a real store-sub.
    let code = project(
        "<script lang=\"ts\">import { writable } from \"svelte/store\"; const count = writable(0);</script>\n<ul>{#each list as $item}<li>{$item}-{$count}</li>{/each}</ul>",
    );
    let body = render_body(&code);
    assert!(
        !body.contains("__verter_store_get(item)"),
        "the `$item` each-binding is NOT rewritten: {body}"
    );
    assert!(
        body.contains("__verter_store_get(count)"),
        "a real `$count` store-sub in the SAME each body IS rewritten: {body}"
    );
}

#[test]
fn an_each_dollar_binding_does_not_leak_to_a_sibling_block() {
    // The `$item` binding scopes to its OWN each block; a `$item` reference in a
    // SIBLING block (where `item` is NOT bound) is a real store-sub and rewrites.
    // DISCRIMINATING via the EXACT occurrence count: post-fix there is EXACTLY
    // ONE `__verter_store_get(item)` (the sibling) — the each-body `$item` stays a
    // verbatim local. Pre-fix BOTH `$item`s rewrite (count == 2), so this fails.
    let code = project("<ul>{#each a as $item}<li>{$item}</li>{/each}</ul>\n<div>{$item}</div>");
    let body = render_body(&code);
    let helper_count = body.matches("__verter_store_get(item)").count();
    assert_eq!(
        helper_count, 1,
        "EXACTLY ONE `$item` (the sibling outside the each) is a store-sub; the \
         each-body `$item` is a local (no leak, no over-rewrite): {body}"
    );
    // The each-body `$item` stays a verbatim local reference inside the `.map`.
    assert!(
        body.contains("(<><li>{$item}</li></>)"),
        "the each-body `$item` stays a verbatim local reference: {body}"
    );
}

#[test]
fn each_destructure_default_await_rewrites_through_the_helper_and_arrow_stays_sync() {
    // F6 (COMPREHENSIVE-AUDIT — the binding-pattern-default markup-await position):
    // an `await` in an `{#each xs as { x = await load() }}` destructuring DEFAULT is
    // sliced into a SYNC `.map((PARAMS) => …)` arrow head — a raw `await` there
    // would be INVALID TSX. The pattern-default text path must route the await
    // rewrite: `__verter_await_expr(load())`, NO raw `await` in the arrow param.
    let source = "{#each xs as { x = await load() }}<span>{x}</span>{/each}";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("{ x = __verter_await_expr(load()) }"),
        "the each-destructure default await routes through the helper: {body}"
    );
    assert!(
        !body.contains("await "),
        "no raw `await` keyword leaks into the sync `.map` arrow head: {body}"
    );
    // The informational diagnostic is emitted for this binding-default position too.
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "the binding-default await records the informational diagnostic: {:?}",
        projection.diagnostics
    );
}

#[test]
fn fragment_dynamic_slot_await_rewrites_through_the_helper_and_render_stays_sync() {
    // F6/F9 (COMPREHENSIVE-AUDIT — the dynamic `<svelte:fragment slot={…}>` value
    // markup-await position): the dynamic slot value is void-checked in place via
    // `{__verter_void(TEXT)}` in the SYNC render fn — a raw `await` there would be
    // INVALID TSX. The slot-expression text path must route the await rewrite.
    let source = "<svelte:fragment slot={await load()}><span>x</span></svelte:fragment>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("__verter_void(__verter_await_expr(load()))"),
        "the dynamic-slot await routes through the helper: {body}"
    );
    assert!(
        !body.contains("await "),
        "no raw `await` keyword leaks into the sync render body: {body}"
    );
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "the dynamic-slot await records the informational diagnostic: {:?}",
        projection.diagnostics
    );
}

#[test]
fn fragment_dynamic_slot_store_sub_is_rewritten() {
    // F9/F11 (COMPREHENSIVE-AUDIT side effect): a store-sub in a dynamic
    // `<svelte:fragment slot={$name}>` value must ALSO be rewritten through the
    // text path — a raw `$name` would surface a phantom `Cannot find name`.
    let source = "<script lang=\"ts\">import { writable } from \"svelte/store\"; const name = writable(\"a\");</script>\n<svelte:fragment slot={$name}><span>x</span></svelte:fragment>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    let body = render_body(&projection.code);
    assert!(
        body.contains("__verter_void(__verter_store_get(name))"),
        "the dynamic-slot store-sub is rewritten through the text path: {body}"
    );
}

// ── ISSUE-7 parity: an unused Svelte top-level `let` is NOT kept artificially
// live ────────────────────────────────────────────────────────────────────
//
// Unlike Vue's `<script setup>` lowering (which built a `___VERTER___unwrapped`
// object that value-read every binding and suppressed TS6133), the Svelte IDE
// projector keeps script bodies as ORIGINAL chunks at their source spans and
// never synthesises a per-binding value-read. So an unused top-level `let foo`
// stays a plain module-level decl that TypeScript naturally flags as unused —
// no Vue-style keep-alive. This pins that parity; no Svelte codegen change is
// expected for ISSUE-7.
#[test]
fn unused_top_level_let_is_not_value_read_kept_alive() {
    let code = project("<script lang=\"ts\">let foo = 1;</script>\n<div>hello</div>");

    // The decl stays at its source span (mapped chunk), not rewritten.
    assert!(
        code.contains("let foo = 1;"),
        "the unused top-level `let foo` must stay a plain source decl: {code}"
    );

    // No Vue-style synthetic value-read keep-alive of `foo`.
    assert!(
        !code.contains("foo: foo as unknown as typeof foo"),
        "Svelte must NOT emit a Vue-style value-read keep-alive for `foo`: {code}"
    );
    assert!(
        !code.contains("shallowUnwrapRef"),
        "Svelte projection has no shallowUnwrapRef unwrap object: {code}"
    );
    // `foo` is used nowhere — the template references nothing, so the render
    // body must not reference `foo` either (no synthetic use).
    let body = render_body(&code);
    assert!(
        !body.contains("foo"),
        "an unused `foo` must not be referenced by the render body: {body}"
    );
}

// The Svelte IDE projector publishes the typed `x_verter_helper_preamble_end`
// source-map boundary — the SAME contract Vue's IDE projection already meets.
// The Svelte prelude is the module INTRO (the `@jsxImportSource` pragma must be
// the leading output bytes), so the boundary is captured on the intro block of
// the shared `generate_map_with_preamble` walk. This is the producer half of the
// LSP fail-closed auto-import preamble classifier: without the boundary a legit
// `.svelte` zero-width auto-import strict-mapping near the carrier top is
// over-dropped by the absent-boundary fuse.
//
// DISCRIMINATING: against the pre-change projector (which generated the map via
// `generate_map_json` and never registered the prelude as the helper preamble),
// assertions (b)/(c) FAIL — the member is absent. With the producer fix the
// projector routes through `generate_map_json_with_preamble` over a
// preamble-registered intro, so the member is present at the exact post-prelude
// generated position.
#[test]
fn svelte_ide_projection_publishes_helper_preamble_end_boundary() {
    let source = "<script lang=\"ts\">let a = 1;</script>\n<div>{a}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);

    // The component prelude in Html namespace + legacy mode (this source uses NO
    // rune, so it is legacy). This is the exact INTRO the projector prepends.
    let prelude = super::prelude::render_prelude(super::prelude::SvelteJsxNamespace::Html, true);

    // (a) Byte-unchanged TSX: the projected code leads with the prelude verbatim
    // and equals the prelude concatenated with the rest the projector produced.
    // Compare the `.code` against itself reconstructed (prelude + suffix) — the
    // producer change adds ONLY source-map metadata, never a TSX byte.
    assert!(
        projection.code.starts_with(&prelude),
        "the projected TSX must lead with the unmapped prelude verbatim: {:?}",
        &projection.code[..projection.code.len().min(prelude.len() + 40)]
    );
    let suffix = &projection.code[prelude.len()..];
    let reconstructed = format!("{prelude}{suffix}");
    assert_eq!(
        projection.code, reconstructed,
        "the projected TSX bytes must be exactly prelude ++ projection suffix (byte-unchanged)"
    );
    // The boundary recording must NOT double-prepend the prelude: the pragma line appears EXACTLY
    // once (a stray second intro insertion would duplicate it). This pins that
    // `prepend_helper_preamble_content` adds only metadata, never a second TSX copy of the prelude.
    assert_eq!(
        projection
            .code
            .matches("/** @jsxImportSource @verter/svelte-jsx */")
            .count(),
        1,
        "the prelude pragma must appear exactly once (no double-prepend): {}",
        projection.code
    );
    // The render scope still wraps the template (the projection is intact).
    assert!(
        projection.code.contains("function __verter_render()"),
        "the render scope function must still be emitted: {}",
        projection.code
    );

    // (b) The map now carries the typed preamble-end boundary member (ABSENT
    // pre-fix because the projector used `generate_map_json`).
    assert!(
        projection
            .source_map
            .contains("x_verter_helper_preamble_end"),
        "the Svelte IDE source map must publish the x_verter_helper_preamble_end boundary: {}",
        projection.source_map
    );

    // (c) The boundary VALUE is exact: the generated position immediately after
    // the rendered prelude. The prelude is pure insertion (the intro), so its end
    // is line == the count of `\n` in the prelude, column 0 (every prelude
    // fragment ends with `\n`, so the last line is empty).
    let prelude_newlines = prelude.matches('\n').count() as u64;
    assert!(
        prelude.ends_with('\n'),
        "the prelude is expected to end with a newline (boundary column 0)"
    );
    let map: serde_json::Value =
        serde_json::from_str(&projection.source_map).expect("the source map is valid JSON");
    let boundary = map
        .get("x_verter_helper_preamble_end")
        .expect("the boundary member is present");
    assert_eq!(
        boundary.get("line").and_then(serde_json::Value::as_u64),
        Some(prelude_newlines),
        "the boundary line must be the count of prelude newlines, got {boundary:?}"
    );
    assert_eq!(
        boundary
            .get("character")
            .and_then(serde_json::Value::as_u64),
        Some(0),
        "the boundary column must be 0 (the prelude ends with a newline), got {boundary:?}"
    );
}
