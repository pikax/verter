//! Svelte IDE TSX projection snapshots with NEGATIVE assertions.
//!
//! Each test pins a matrix row's projected TSX shape AND asserts the original
//! Svelte block/tag syntax left NO residue (`{#if`, `{@render`, `<script`,
//! `class:`, …). The clean-type-check gate (through TSGO) lives in the
//! session-side fixtures; these characterize the syntactic transform.

use super::projector::project_svelte_ide;
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
    // D-ap ordering: the declarator precedes the `return (` of the render fn.
    let decl_idx = code.find("const row = __verter_snippet(").unwrap();
    let return_idx = code.find("return (<>").unwrap();
    assert!(
        decl_idx < return_idx,
        "snippet declarator must be hoisted ABOVE the render return (D-ap TDZ): {code}"
    );
}

#[test]
fn legacy_on_event_projects_verbatim_lowercase_never_oncamel() {
    let code = project("<button on:click={handle}>x</button>");
    assert!(!code.contains("on:click"), "no on: residue: {code}");
    assert!(
        code.contains("onclick={handle}"),
        "verbatim lowercase: {code}"
    );
    assert!(
        !code.contains("onClick"),
        "the onClick rename is RETIRED: {code}"
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
    // `--track-color` residue survives in the projection (D-ap).
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
    // The const is HOISTED to a real statement (D-ap sibling-run scope) — it
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
fn await_block_out_of_scope_expression_emits_diagnostic_but_projects() {
    let source = "<div>{#await p}loading{:then v}{v}{:catch e}{e}{/await}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(!projection.code.contains("{#await"), "no #await residue");
    assert!(!projection.code.contains("{:then"), "no :then residue");
    assert!(!projection.code.contains("{/await}"), "no /await residue");
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
fn await_expression_in_interpolation_records_the_experimental_diagnostic() {
    let source = "<div>{await thing}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "await-experimental diagnostic present: {:?}",
        projection.diagnostics
    );
}

#[test]
fn await_in_instance_script_top_level_records_the_diagnostic() {
    // P2-2: await at instance-script top level (D-bg position 1).
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
    // flagged in markup; an await nested inside an async arrow is NOT (ordinary
    // TS) — the discriminating async-fn case is covered separately.
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
    // B8e test-3 (fast-follow): a `bind:value={expr}` bound token maps back to
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
    // B8e NIT: a comma INSIDE a string literal in a single-getter `bind:x`
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
