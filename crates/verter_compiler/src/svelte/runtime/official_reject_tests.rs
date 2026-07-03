use super::*;
use crate::svelte::parser::parse_svelte;

/// Run the gate over a component source, returning the violated RULE class (the
/// exact official code is asserted separately where it matters).
fn gate(source: &str) -> Option<CoreOfficialValidationRule> {
    let parsed = parse_svelte(source);
    official_reject_gate(source, &parsed).map(|r| r.rule)
}

/// Run the gate, returning the FULL rejection (rule + exact official code) — for the
/// rules whose committed exact code must be asserted, not merely the rule class.
fn gate_full(source: &str) -> Option<OfficialRejection> {
    let parsed = parse_svelte(source);
    official_reject_gate(source, &parsed)
}

// ── BindGroupInvalidExpression (bind:group function-pair / sequence target) ───────

#[test]
fn bind_group_function_pair_target_rejects_with_exact_code() {
    // `<input type="radio" bind:group={get, set}>` — a two-element function-pair on
    // `bind:group`. Official svelte@5.56.3 rejects it with `bind_group_invalid_expression`
    // (`bind:group` accepts only an Identifier or MemberExpression). RED before the fix:
    // Verter fail-OPENED (the function-pair was accepted as a clean FunctionPair for
    // every DOM bind). The EXACT code is asserted, not just "an error".
    assert_eq!(
        gate_full("<input type=\"radio\" bind:group={get, set}>"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindGroupInvalidExpression,
            "bind_group_invalid_expression",
        )),
        "bind:group with a function-pair target must reject as the exact \
             bind_group_invalid_expression code"
    );
}

#[test]
fn bind_group_three_element_sequence_target_rejects_with_exact_code() {
    // `bind:group={a, b, c}` — a 3-element sequence is STILL a sequence: official throws
    // `bind_group_invalid_expression` BEFORE the two-element length check, so ANY
    // SequenceExpression arity is rejected (not just the valid 2-element pair shape).
    assert_eq!(
        gate_full("<input type=\"radio\" bind:group={a, b, c}>"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindGroupInvalidExpression,
            "bind_group_invalid_expression",
        )),
        "bind:group with a 3-element sequence target must reject as the exact code"
    );
}

#[test]
fn bind_group_identifier_target_passes_the_gate() {
    // NEGATIVE: a bare-identifier `bind:group={g}` is NOT a sequence — the gate must NOT
    // reject it (it falls through to the normal bind classifier, which official accepts
    // when `g` is state/props). RED would be a false reject of a valid group bind.
    assert_eq!(gate("<input type=\"radio\" bind:group={g}>"), None);
    // A member target is likewise allowed (Identifier OR MemberExpression).
    assert_eq!(gate("<input type=\"radio\" bind:group={o.x}>"), None);
}

#[test]
fn non_group_function_pair_binds_pass_the_gate() {
    // POSITIVE CONTROL: the identifier/member-only policy is `bind:group`-ONLY. A
    // function-pair on a NON-group bind (`bind:value` / `bind:checked`) is official-VALID
    // (verified svelte@5.56.3 emits `$.bind_value` / `$.bind_checked`), so the gate must
    // NOT reject it. A regression that broadened the policy to all binds would RED here.
    assert_eq!(
        gate("<input bind:value={() => v, (x) => v = x}>"),
        None,
        "bind:value function-pair must pass the gate (not identifier/member-only)"
    );
    assert_eq!(
        gate("<input type=\"checkbox\" bind:checked={() => c, (x) => c = x}>"),
        None,
        "bind:checked function-pair must pass the gate (not identifier/member-only)"
    );
}

#[test]
fn bind_group_quoted_function_pair_target_rejects_with_exact_code() {
    // `<input type="radio" bind:group="{get, set}">` — a QUOTED single-expression
    // function-pair: a `SvelteAttributeValue::Mixed` value (`"{…}"`), NOT a bare
    // `Expression`. Official svelte@5.56.3 rejects it IDENTICALLY to the bare form with
    // `bind_group_invalid_expression`. RED before the fix: the policy scan matched ONLY
    // `SvelteAttributeValue::Expression`, SKIPPING the quoted `Mixed` form, so the quoted
    // group fail-OPENED (classified as a clean function-pair → wrong `$.bind_group`
    // emission). The quoted body must be scanned identically to the bare `{expr}` (the
    // single-expression inner is extracted through the SHARED JS-aware brace scanner).
    assert_eq!(
        gate_full("<input type=\"radio\" bind:group=\"{get, set}\">"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindGroupInvalidExpression,
            "bind_group_invalid_expression",
        )),
        "a QUOTED bind:group function-pair target must reject as the exact \
         bind_group_invalid_expression code (the policy scan must handle Mixed)"
    );
    // A 3-element QUOTED sequence is still a sequence → still rejected (any arity).
    assert_eq!(
        gate("<input type=\"radio\" bind:group=\"{a, b, c}\">"),
        Some(CoreOfficialValidationRule::BindGroupInvalidExpression),
        "a QUOTED 3-element group sequence target must also reject"
    );
}

#[test]
fn quoted_non_group_function_pair_binds_pass_the_gate() {
    // POSITIVE CONTROL: the Mixed-aware policy scan stays `bind:group`-ONLY. A QUOTED
    // function-pair on a NON-group bind (`bind:value="{get,set}"`) is official-VALID
    // (verified svelte@5.56.3 emits `$.bind_value`), so the gate must NOT reject it. A
    // regression that rejected ALL Mixed function-pairs (over-broad Mixed scan) would
    // RED here. A quoted group identifier (`bind:group="{g}"`) is likewise NOT a
    // sequence and must pass the gate.
    assert_eq!(
        gate("<input bind:value=\"{() => v, (x) => v = x}\">"),
        None,
        "a QUOTED bind:value function-pair must pass the gate"
    );
    assert_eq!(
        gate("<input type=\"radio\" bind:group=\"{g}\">"),
        None,
        "a QUOTED bind:group bare-identifier target is not a sequence — must pass the gate"
    );
}

// ── BindInvalidParens (author parens around a bind sequence) ──────────────────

#[test]
fn bind_value_parenthesized_function_pair_rejects_with_exact_code() {
    // Finding B (R4): `<input bind:value={(get, set)}>` — author PARENS around a
    // function-pair sequence. Official svelte@5.56.3 rejects it with the EXACT code
    // `bind_invalid_parens` (a `(` between `{` and the sequence start). RED before the fix:
    // Verter fail-OPENED (the author parens were transparently unwrapped → the pair was
    // accepted as a clean `FunctionPair` → wrong `$.bind_value(el, get, set)` emission).
    assert_eq!(
        gate_full("<input bind:value={(get, set)}>"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindInvalidParens,
            "bind_invalid_parens",
        )),
        "a parenthesized bind:value function-pair must reject as the exact bind_invalid_parens code"
    );
}

#[test]
fn bind_value_quoted_parenthesized_function_pair_rejects_with_exact_code() {
    // The QUOTED single-expression form `"{(get, set)}"` (a `Mixed` value) rejects
    // IDENTICALLY — the paren scan unwraps the quoted inner through the shared brace
    // scanner, the SAME as the bare `{…}` form.
    assert_eq!(
        gate_full("<input bind:value=\"{(get, set)}\">"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindInvalidParens,
            "bind_invalid_parens",
        )),
        "a QUOTED parenthesized bind:value function-pair must reject as bind_invalid_parens"
    );
}

#[test]
fn bind_this_parenthesized_function_pair_rejects_with_exact_code() {
    // `bind:this={(get, set)}` — author parens around the bind:this getter/setter pair —
    // is ALSO `bind_invalid_parens` (the paren scan is bind-name-agnostic; the bare
    // `bind:this={get, set}` element form stays accepted by the runtime classifier).
    assert_eq!(
        gate_full("<div bind:this={(get, set)}></div>"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindInvalidParens,
            "bind_invalid_parens",
        )),
        "a parenthesized bind:this function-pair must reject as bind_invalid_parens"
    );
}

#[test]
fn bind_group_parenthesized_sequence_stays_group_invalid_expression() {
    // ORDERING: `bind:group={(get, set)}` is BOTH a parenthesized sequence AND a group
    // sequence. Official's group policy throws `bind_group_invalid_expression` FIRST (the
    // group scan precedes the paren scan), so the group code wins — NOT bind_invalid_parens.
    assert_eq!(
        gate_full("<input type=\"radio\" bind:group={(get, set)}>"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindGroupInvalidExpression,
            "bind_group_invalid_expression",
        )),
        "a parenthesized bind:group sequence must reject as bind_group_invalid_expression (group \
         policy precedes the paren scan)"
    );
}

#[test]
fn bare_function_pair_and_parenthesized_non_sequence_pass_the_paren_gate() {
    // NEGATIVE CONTROLS. A BARE (un-parenthesized) function-pair is official-VALID — the
    // paren scan must NOT reject it.
    assert_eq!(
        gate("<input bind:value={get, set}>"),
        None,
        "a BARE bind:value function-pair must pass the paren gate"
    );
    // A parenthesized NON-sequence (`{(v)}`) is official-ACCEPTED (the paren scan fires only
    // for a parenthesized SEQUENCE, matching svelte's `SequenceExpression`-only check). A
    // regression that rejected all author parens would RED here.
    assert_eq!(
        gate("<input bind:value={(v)}>"),
        None,
        "a parenthesized NON-sequence bind value must pass the paren gate"
    );
    assert_eq!(
        gate("<div bind:this={(el)}></div>"),
        None,
        "a parenthesized NON-sequence bind:this target must pass the paren gate"
    );
}

// ── BindInvalidExpression (non-lvalue / 3+-sequence bind target) ─────────────

#[test]
fn bind_value_call_expression_rejects_with_exact_code() {
    // `<input bind:value={f()}>` — a CALL is neither a valid lvalue (Identifier/Member) nor a
    // 2-element function-pair. Official svelte@5.56.3 rejects it with the EXACT code
    // `bind_invalid_expression` (the same bind-SHAPE class as bind_group / bind_parens, NOT
    // D-26 TS grammar parity). RED before the scan: the gate returned None (the call fell
    // through to the runtime classifier's generic Binding refusal).
    assert_eq!(
        gate_full("<input bind:value={f()}>"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindInvalidExpression,
            "bind_invalid_expression",
        )),
        "a call-expression bind target must reject as the exact bind_invalid_expression code"
    );
}

#[test]
fn bind_value_three_element_sequence_rejects_with_exact_code() {
    // `<input bind:value={a, b, c}>` — a 3+-element sequence is neither a valid lvalue nor a
    // 2-element pair. Official rejects it `bind_invalid_expression` (the bind:group sequence
    // case is `bind_group_invalid_expression`, caught earlier by the group-policy scan).
    assert_eq!(
        gate_full("<input bind:value={a, b, c}>"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindInvalidExpression,
            "bind_invalid_expression",
        ))
    );
}

#[test]
fn bind_invalid_expression_covers_literal_binary_optional_chain_and_member_call() {
    // The non-lvalue family — a literal, a binary, an optional-chain member, a member-call —
    // all `bind_invalid_expression` (oracle-verified svelte@5.56.3). An optional chain
    // (`obj?.x`) is NOT an assignable lvalue, so it joins the family.
    for src in [
        "<input bind:value={1}>",
        "<input bind:value={a + b}>",
        "<input bind:value={obj?.x}>",
        "<input bind:value={a.b.c()}>",
    ] {
        assert_eq!(
            gate(src),
            Some(CoreOfficialValidationRule::BindInvalidExpression),
            "{src} must reject as bind_invalid_expression"
        );
    }
}

#[test]
fn bind_invalid_expression_applies_to_every_bind_name() {
    // The shape check is bind-name-agnostic (official's `BindDirective` analysis): a call on
    // bind:this / bind:checked / bind:group all reject `bind_invalid_expression` (the group
    // case only short-circuits to bind_group_invalid_expression for a SEQUENCE, not a call).
    for src in [
        "<input bind:this={f()}>",
        "<input type=\"checkbox\" bind:checked={f()}>",
        "<input type=\"radio\" bind:group={f()}>",
    ] {
        assert_eq!(
            gate(src),
            Some(CoreOfficialValidationRule::BindInvalidExpression),
            "{src}"
        );
    }
}

#[test]
fn valid_lvalue_and_function_pair_bind_targets_pass_the_invalid_expression_gate() {
    // NEGATIVE CONTROLS: a bare identifier, a member, a computed member, a 2-element
    // function-pair, and a parenthesized NON-sequence are all valid bind shapes — the
    // invalid-expression scan must NOT reject them. A regression that over-fired on any
    // valid target would RED here.
    for src in [
        "<input bind:value={v}>",
        "<input bind:value={o.x}>",
        "<input bind:value={a[i]}>",
        "<input bind:value={get, set}>",
        "<input bind:value={(v)}>",
    ] {
        assert_eq!(
            gate(src),
            None,
            "{src} must pass the invalid-expression gate"
        );
    }
}

#[test]
fn bind_invalid_expression_ordered_after_group_and_parens() {
    // ORDERING: a bind:group sequence stays `bind_group_invalid_expression`; a parenthesized
    // pair stays `bind_invalid_parens` — the invalid-expression scan runs AFTER both scans,
    // so the more-specific codes win.
    assert_eq!(
        gate_full("<input type=\"radio\" bind:group={a, b, c}>"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindGroupInvalidExpression,
            "bind_group_invalid_expression",
        )),
        "a bind:group sequence stays bind_group_invalid_expression"
    );
    assert_eq!(
        gate_full("<input bind:value={(get, set)}>"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindInvalidParens,
            "bind_invalid_parens",
        )),
        "a parenthesized pair stays bind_invalid_parens"
    );
}

#[test]
fn ts_wrapped_bind_target_is_not_bind_invalid_expression() {
    // DISCRIMINATOR (F2 vs F1/D-26): a TS-wrapped target (`name as T`) is NOT
    // bind_invalid_expression — it is the parse-error / D-26 class. The invalid-expression
    // scan EXCLUDES TS-containing lvalues (`lvalue_contains_ts`), so the gate does NOT mint
    // bind_invalid_expression for it (official PARSE-rejects it with `expected_token`;
    // Verter's fail-closed for it is the runtime Binding channel, D-26-owned for exact code).
    assert_eq!(
        gate("<input bind:value={name as T}>"),
        None,
        "a TS-wrapped target must NOT mint bind_invalid_expression (it is the D-26 class)"
    );
}

// ── Bind name/host/host-attr gate: shape scans run ONLY for binds official carries to
//    expression validation (intrinsic name/host/host-attr valid; every component bind) ──

#[test]
fn invalid_name_intrinsic_bind_does_not_mint_a_shape_code() {
    // `<div bind:foo={f()}>` — `foo` is not a valid intrinsic DOM bind name. Official
    // svelte@5.56.3 rejects it `bind_invalid_name` (a NAME error) BEFORE expression-shape
    // validation. The exact name/host/attr codes are deferred (D-29); such a bind fails
    // closed via the unsupported channel — so the shape scan must NOT mint the wrong
    // `bind_invalid_expression`. RED before the fix: the call-shape scan fired
    // `BindInvalidExpression` (a confidently-WRONG exact code).
    let r = gate_full("<div bind:foo={f()}></div>");
    assert!(
        !matches!(
            r,
            Some(OfficialRejection {
                rule: CoreOfficialValidationRule::BindInvalidExpression,
                ..
            })
        ),
        "an invalid-NAME intrinsic bind must not mint bind_invalid_expression (deferred to D-29): {r:?}"
    );
    assert_eq!(
        r, None,
        "the gate carries no shape reject for an invalid-name bind (it fails closed downstream)"
    );
}

#[test]
fn unsupported_host_intrinsic_bind_does_not_mint_bind_invalid_parens() {
    // `<div bind:value={(get, set)}>` — `bind:value` is not valid on a `<div>` host. Official
    // rejects it `bind_invalid_target` (a HOST error) BEFORE the paren scan, so the shape scan
    // must NOT mint `bind_invalid_parens`. RED before the fix: the paren scan fired
    // BindInvalidParens.
    let r = gate_full("<div bind:value={(get, set)}></div>");
    assert!(
        !matches!(
            r,
            Some(OfficialRejection {
                rule: CoreOfficialValidationRule::BindInvalidParens,
                ..
            })
        ),
        "an unsupported-host bind must not mint bind_invalid_parens (deferred to D-29): {r:?}"
    );
    assert_eq!(r, None);
}

#[test]
fn contenteditable_missing_intrinsic_bind_does_not_mint_a_shape_code() {
    // `<div bind:innerHTML={f()}>` — innerHTML requires a static `contenteditable` host attr.
    // Official rejects it `attribute_contenteditable_missing` (a HOST-ATTR error) BEFORE the
    // expression-shape scan, so the shape scan must NOT mint bind_invalid_expression. RED
    // before the fix.
    let r = gate_full("<div bind:innerHTML={f()}></div>");
    assert!(
        !matches!(
            r,
            Some(OfficialRejection {
                rule: CoreOfficialValidationRule::BindInvalidExpression,
                ..
            })
        ),
        "an innerHTML-without-contenteditable bind must not mint bind_invalid_expression: {r:?}"
    );
    assert_eq!(r, None);
}

#[test]
fn dynamic_multiple_select_bind_does_not_mint_a_shape_code() {
    // `<select multiple={m} bind:value={f()}>` — a DYNAMIC `multiple` makes `bind:value` on a
    // `<select>` invalid. Official rejects it `attribute_invalid_multiple` (a HOST-ATTR error)
    // BEFORE the expression-shape scan, so the shape scan must NOT mint
    // bind_invalid_expression. RED before the fix.
    let r = gate_full("<select multiple={m} bind:value={f()}></select>");
    assert!(
        !matches!(
            r,
            Some(OfficialRejection {
                rule: CoreOfficialValidationRule::BindInvalidExpression,
                ..
            })
        ),
        "a dynamic-multiple select bind must not mint bind_invalid_expression: {r:?}"
    );
    assert_eq!(r, None);
}

#[test]
fn valid_intrinsic_name_host_attr_binds_still_mint_shape_codes() {
    // POSITIVE CONTROL: a VALID name/host/host-attr intrinsic bind STILL reaches the shape
    // scans (official carries it to expression validation). Verified svelte@5.56.3 — each
    // rejects `bind_invalid_expression`:
    //  - `<input bind:value={f()}>` (valid name/host, type absent);
    //  - `<select bind:value={f()}>` (valid host, `multiple` absent → static-OK);
    //  - `<select multiple bind:value={f()}>` (STATIC `multiple` is valid);
    //  - `<div contenteditable bind:innerHTML={f()}>` (a static `contenteditable` satisfies
    //    the contenteditable gate). A regression that over-gated would RED here.
    for src in [
        "<input bind:value={f()}>",
        "<select bind:value={f()}></select>",
        "<select multiple bind:value={f()}></select>",
        "<div contenteditable bind:innerHTML={f()}></div>",
    ] {
        assert_eq!(
            gate(src),
            Some(CoreOfficialValidationRule::BindInvalidExpression),
            "{src} (valid name/host/host-attr) must still reach the shape scan"
        );
    }
}

#[test]
fn component_bind_shape_codes_are_preserved() {
    // The name/host/host-attr gate is INTRINSIC-only: a COMPONENT bind has no DOM
    // name/host/host-attr check (official carries every component bind straight to expression
    // validation), so a shape-invalid component bind STILL mints its shape code — verified
    // svelte@5.56.3: `<Foo bind:value={f()}>` → bind_invalid_expression, `<Foo
    // bind:value={(g, s)}>` → bind_invalid_parens. A regression that gated the shape scan to
    // DOM-only validity would RED here (it would drop the correct component shape code).
    assert_eq!(
        gate("<Foo bind:value={f()} />"),
        Some(CoreOfficialValidationRule::BindInvalidExpression),
        "a component invalid-expression bind keeps its shape code"
    );
    assert_eq!(
        gate("<Foo bind:value={(g, s)} />"),
        Some(CoreOfficialValidationRule::BindInvalidParens),
        "a component parenthesized-pair bind keeps its shape code"
    );
}

#[test]
fn earlier_bind_shape_reject_beats_later_group_in_document_order() {
    // ORDERING (the single document/attribute-order pass): the FIRST violating bind wins. An
    // EARLIER `bind_invalid_parens` / `bind_invalid_expression` beats a LATER
    // `bind_group_invalid_expression`, matching official's document-order BindDirective walk.
    // RED before the fix: the three category scans ran group-FIRST across the whole tree, so
    // the LATER group sequence wrongly won.
    assert_eq!(
        gate_full("<input bind:value={(get, set)} /><input type=\"radio\" bind:group={a, b, c} />"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindInvalidParens,
            "bind_invalid_parens",
        )),
        "an earlier parens bind must beat a later group sequence (document order)"
    );
    assert_eq!(
        gate_full("<input bind:value={f()} /><input type=\"radio\" bind:group={a, b, c} />"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindInvalidExpression,
            "bind_invalid_expression",
        )),
        "an earlier invalid-expression bind must beat a later group sequence (document order)"
    );
    // CONTROL: the reverse order (group EARLIER) still reports the group code first.
    assert_eq!(
        gate_full("<input type=\"radio\" bind:group={a, b, c} /><input bind:value={(get, set)} />"),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::BindGroupInvalidExpression,
            "bind_group_invalid_expression",
        )),
        "an earlier group sequence still reports the group code first"
    );
}

// ── DollarPrefixInvalid (declaration position) ───────────────────────────────

#[test]
fn dollar_prefixed_props_destructure_local_is_dollar_prefix_invalid() {
    // `let { a: $foo } = $props()` — the DESTRUCTURE-position `$foo` binding is the
    // official `dollar_prefix_invalid` (a declaration, caught at the binder).
    assert_eq!(
        gate("<script>let { a: $foo } = $props();</script>\n<p>{$foo}</p>\n"),
        Some(CoreOfficialValidationRule::DollarPrefixInvalid)
    );
}

#[test]
fn dollar_prefixed_identifier_declarator_is_dollar_prefix_invalid() {
    // `let $$anchor = 1` — an IDENTIFIER-position `$$`-prefixed binding.
    assert_eq!(
            gate("<script>let c = $state(0); let $$anchor = 1;</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::DollarPrefixInvalid)
        );
}

#[test]
fn dollar_prefixed_import_locals_are_dollar_prefix_invalid() {
    // `import $inspect from './x.svelte'` — a `$`-prefixed imported LOCAL binding is
    // official `dollar_prefix_invalid` ("The $ prefix is reserved, and cannot be used
    // for variables and imports"). RED before the fix: the declarator scan covered
    // top-level VariableDeclarations only, so the import slipped through to
    // `$inspect`-elision (fail-open on invalid input). All three local-binding forms:
    // default, named-`as` local, namespace.
    assert_eq!(
        gate_full(
            "<script>import $inspect from './x.svelte'; let c = $state(0); $inspect(c);</script>\n<button onclick={() => c++}>{c}</button>\n"
        ),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::DollarPrefixInvalid,
            "dollar_prefix_invalid",
        )),
        "a `$`-prefixed DEFAULT import local must reject with the exact code"
    );
    assert_eq!(
        gate("<script>import { foo as $bar } from './m'; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
        Some(CoreOfficialValidationRule::DollarPrefixInvalid),
        "a `$`-prefixed NAMED-`as` import local must reject"
    );
    assert_eq!(
        gate("<script>import * as $ns from './m'; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
        Some(CoreOfficialValidationRule::DollarPrefixInvalid),
        "a `$`-prefixed NAMESPACE import local must reject"
    );
}

#[test]
fn plain_import_locals_are_not_dollar_prefix_invalid() {
    // NEGATIVE: plain (non-`$`) import locals never trip the dollar-prefix scan —
    // the §1.2 component-import surface (`import Child from './Child.svelte'`) must
    // keep passing the gate cleanly.
    assert_eq!(
        gate("<script>import Child from './Child.svelte'; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
        None,
        "a plain default import local must pass the gate"
    );
    assert_eq!(
        gate("<script>import { foo, bar as baz } from './m'; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
        None,
        "plain named import locals must pass the gate"
    );
}

#[test]
fn plain_named_declarations_are_not_dollar_prefix_invalid() {
    // NEGATIVE: a plain (non-`$`) declaration is never a dollar-prefix violation —
    // the §1.2 fixture's `let name`/`let count` must pass the gate cleanly.
    assert_eq!(
            gate("<script>let name = $state('world'); let count = $state(0);</script>\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n"),
            None
        );
}

// ── InspectTraceInvalidPlacement (`$inspect.trace()` placement) ──────────────

#[test]
fn inspect_trace_non_first_statement_is_invalid_placement() {
    // A trace call as a NON-first statement of an `$effect` arrow body — official
    // svelte@5.56.3 hard-errors `inspect_trace_invalid_placement` ("must be the first
    // statement of a function body"). RED before the fix: Verter silently DROPPED the
    // statement (over-acceptance).
    assert_eq!(
        gate_full(
            "<script>let c = $state(0); $effect(() => { c++; $inspect.trace(); });</script>\n<p>{c}</p>\n"
        ),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::InspectTraceInvalidPlacement,
            "inspect_trace_invalid_placement",
        )),
        "a non-first-statement `$inspect.trace()` must reject with the exact code"
    );
}

#[test]
fn inspect_trace_nested_in_if_or_block_is_invalid_placement() {
    // A trace call nested inside an `if` consequent of a handler arrow — the statement
    // is first in the IF's block, but that block is not a FUNCTION body, so official
    // still errors. Same for a bare nested `{ }` block.
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={() => { if (c > 0) { $inspect.trace(); } c++; }}>{c}</button>\n"
        ),
        Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
        "a trace inside an `if` consequent must reject"
    );
    assert_eq!(
        gate(
            "<script>let c = $state(0); $effect(() => { { $inspect.trace(); } c++; });</script>\n<p>{c}</p>\n"
        ),
        Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
        "a trace inside a bare nested block must reject"
    );
}

#[test]
fn inspect_trace_top_level_is_invalid_placement() {
    // A TOP-LEVEL script `$inspect.trace();` — the program body is not a function
    // body, so official hard-errors. (Previously refused as a generic unsupported
    // instance-script item; the exact official code is the parity improvement.)
    assert_eq!(
        gate_full(
            "<script>let c = $state(0); $inspect.trace();</script>\n<button onclick={() => c++}>{c}</button>\n"
        ),
        Some(OfficialRejection::with_code(
            CoreOfficialValidationRule::InspectTraceInvalidPlacement,
            "inspect_trace_invalid_placement",
        )),
        "a top-level `$inspect.trace()` must reject with the exact code"
    );
}

#[test]
fn inspect_trace_concise_arrow_is_invalid_placement() {
    // A CONCISE-arrow expression body (`() => $inspect.trace()`) is an EXPRESSION
    // position, not the first statement of a function body — official hard-errors.
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={() => c++} onfocus={() => $inspect.trace()}>{c}</button>\n"
        ),
        Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
        "a concise-arrow `$inspect.trace()` body must reject"
    );
}

#[test]
fn inspect_trace_first_statement_positions_pass_the_gate() {
    // NEGATIVE: the ONE legal position — the first statement of a function body — for
    // each owning function form. The gate must NOT reject these (official accepts and
    // drops the call under `dev:false`); downstream the rewriter elides them.
    // (a) First statement of an `$effect` BLOCK arrow.
    assert_eq!(
        gate(
            "<script>let c = $state(0); $effect(() => { $inspect.trace(); c++; });</script>\n<p>{c}</p>\n"
        ),
        None,
        "first statement of an $effect arrow body is the legal position"
    );
    // (b) First statement of a handler BLOCK arrow.
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={() => { $inspect.trace(); c++; }}>{c}</button>\n"
        ),
        None,
        "first statement of a handler arrow body is the legal position"
    );
    // (c) First statement of a `function` DECLARATION body (the gate must not flag it
    // even though the top-level function itself fails closed downstream as an
    // unsupported instance-script item — a different, non-official-reject channel).
    assert_eq!(
        gate(
            "<script>let c = $state(0); function tick() { $inspect.trace(); c = c + 1; }</script>\n<button onclick={() => c++}>{c}</button>\n"
        ),
        None,
        "first statement of a function declaration body is the legal position"
    );
    // (d) First statement of a function EXPRESSION body.
    assert_eq!(
        gate(
            "<script>let c = $state(0); const tick = function () { $inspect.trace(); c = c + 1; };</script>\n<button onclick={() => c++}>{c}</button>\n"
        ),
        None,
        "first statement of a function expression body is the legal position"
    );
}

#[test]
fn inspect_trace_parenthesized_first_statement_passes_the_gate() {
    // NEGATIVE: a PARENTHESIZED first-statement trace — `($inspect.trace());` /
    // `(($inspect.trace()));` as `statements[0]` of a handler arrow body. The paren
    // wrapper is transparent: official svelte@5.56.3 ACCEPTS (and drops) both. RED
    // before the fix: the allow-set required a BARE `CallExpression`, so the inner
    // call span was collected as illegal (a false reject of valid Svelte).
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={() => { ($inspect.trace()); c++; }}>{c}</button>\n"
        ),
        None,
        "a single-parenthesized first-statement trace must pass the gate"
    );
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={() => { (($inspect.trace())); c++; }}>{c}</button>\n"
        ),
        None,
        "a double-parenthesized first-statement trace must pass the gate"
    );
    // POSITIVE: parens never LEGALIZE a NON-first trace — official still errors.
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={() => { c++; ($inspect.trace()); }}>{c}</button>\n"
        ),
        Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
        "a parenthesized NON-first trace must still reject"
    );
}

#[test]
fn inspect_trace_param_shadow_is_local_not_rune() {
    // A `$inspect` PARAMETER is VALID Svelte — svelte@5.56.3 ACCEPTS `($inspect) => {
    // ... }` and `function get($inspect) { ... }` (a `$`-prefixed PARAM is legal; only a
    // `const $inspect` LOCAL is `dollar_prefix_invalid`). Under that param
    // `$inspect.trace()` is an ORDINARY local method call, NOT the rune trace. The
    // placement scan must be SCOPE-AWARE (mirroring the `DollarRefScan` `ShadowStack`):
    // a param-shadowed `$inspect.trace()` is ignored, so a NON-first-statement local
    // trace does NOT trip the placement reject. RED before the fix: the scan treated
    // EVERY `$inspect.trace()` as a rune trace, so `c++; $inspect.trace();` under a
    // `$inspect` param false-rejected as `inspect_trace_invalid_placement`.
    // (a) an ARROW with a `$inspect` param.
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={($inspect) => { c++; $inspect.trace(); }}>{c}</button>\n"
        ),
        None,
        "a `$inspect` arrow PARAM makes `$inspect.trace()` an ordinary local call"
    );
    // (b) a function DECLARATION with a `$inspect` param.
    assert_eq!(
        gate(
            "<script>let c = $state(0); function get($inspect){ c++; $inspect.trace(); }</script>\n<button onclick={() => c++}>{c}</button>\n"
        ),
        None,
        "a `$inspect` function-decl PARAM makes `$inspect.trace()` an ordinary local call"
    );
    // POSITIVE control: WITHOUT the param shadow the SAME non-first `$inspect.trace()`
    // is the rune trace and still rejects — scope-awareness must not disable the rule.
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={() => { c++; $inspect.trace(); }}>{c}</button>\n"
        ),
        Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
        "without a `$inspect` local, a non-first trace still rejects"
    );
}

#[test]
fn inspect_trace_in_generator_body_is_invalid_placement() {
    // A GENERATOR function is NOT a legal trace host — official svelte@5.56.3 rejects a
    // generator-body first-statement `$inspect.trace()` with `inspect_trace_generator`
    // (a SEPARATE official rule); Verter rejects it via the placement rule (both
    // fail-closed). RED before the fix: the allow-set admitted a generator body's first
    // statement, so the generator trace false-passed the gate.
    assert_eq!(
        gate(
            "<script>let c = $state(0); function* g(){ $inspect.trace(); yield c; }</script>\n<button onclick={() => c++}>{c}</button>\n"
        ),
        Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
        "a generator-body first-statement trace is not a legal position"
    );
    // POSITIVE control: an ASYNC (non-generator) first-statement trace stays LEGAL
    // (official accepts + drops it) — the generator exclusion must not over-reach.
    assert_eq!(
        gate(
            "<script>let c = $state(0); async function tick(){ $inspect.trace(); await 0; }</script>\n<button onclick={() => c++}>{c}</button>\n"
        ),
        None,
        "an async (non-generator) first-statement trace is still legal"
    );
}

#[test]
fn inspect_trace_in_block_head_is_invalid_placement() {
    // A `$inspect.trace()` in a block HEAD / clause / key expression is not a
    // function-body first statement — official svelte@5.56.3 rejects
    // `inspect_trace_invalid_placement`. RED before the fix: the shared
    // template-expression collection walked block/clause CHILDREN but skipped the block
    // head / clause / `{#await}` subject / `{#key}` expression, so the trace scan never
    // saw those positions (they only fell to a generic downstream refusal, not the exact
    // official code).
    for src in [
        "<script>let c = $state(0);</script>\n{#if $inspect.trace()}<p>{c}</p>{/if}\n",
        "<script>let c = $state(0);</script>\n{#if c}<p>a</p>{:else if $inspect.trace()}<p>b</p>{/if}\n",
        "<script>let c = $state(0);</script>\n{#await $inspect.trace()}<p>a</p>{/await}\n",
        "<script>let c = $state(0);</script>\n{#key $inspect.trace()}<p>{c}</p>{/key}\n",
    ] {
        assert_eq!(
            gate(src),
            Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
            "a block-head trace must reject: {src}"
        );
    }
}

#[test]
fn inspect_trace_object_parenthesized_is_placement_aware() {
    // `($inspect).trace()` — parens around the member OBJECT. The paren wrapper is
    // transparent: official svelte@5.56.3 ACCEPTS (and drops) it as a first statement
    // and REJECTS it non-first. RED before the fix: the trace-shape check required a
    // BARE `$inspect` identifier as the member object, so `($inspect).trace()` was not
    // recognised as the rune trace at all (a first-statement one failed closed in the
    // rewriter instead of dropping; a non-first one escaped the exact placement code).
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={() => { ($inspect).trace(); c++; }}>{c}</button>\n"
        ),
        None,
        "an object-parenthesized first-statement trace passes the gate"
    );
    assert_eq!(
        gate(
            "<script>let c = $state(0);</script>\n<button onclick={() => { c++; ($inspect).trace(); }}>{c}</button>\n"
        ),
        Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
        "an object-parenthesized NON-first trace still rejects with the exact code"
    );
}

#[test]
fn await_then_catch_dollar_bindings_are_not_global_references() {
    // `{:then $foo}` / `{:catch $err}` bind a `$`-prefixed name — official svelte@5.56.3
    // ACCEPTS these await-clause bindings (they are BINDINGS, not references). The block
    // head-expression collection must NOT feed a `{:then}` / `{:catch}` BINDING span into
    // the global-`$`-reference scan (only an `{:else if}` CONDITION is an expression). RED
    // before the fix: collecting every `clause.expr` fed the binding `$foo` to the
    // reference scan, which false-rejected it as `global_reference_invalid`.
    assert_eq!(
        gate("<script>let p = $state(Promise.resolve(1));</script>\n{#await p}<i>w</i>{:then $foo}<b>x</b>{/await}\n"),
        None,
        "a dollar-prefixed then-binding is not a global reference"
    );
    assert_eq!(
        gate("<script>let p = $state(Promise.resolve(1));</script>\n{#await p}<i>w</i>{:catch $err}<b>x</b>{/await}\n"),
        None,
        "a dollar-prefixed catch-binding is not a global reference"
    );
    // POSITIVE control: an `{:else if}` CONDITION is still collected — a misplaced trace
    // there still rejects.
    assert_eq!(
        gate("<script>let c = $state(0);</script>\n{#if c}<p>a</p>{:else if $inspect.trace()}<p>b</p>{/if}\n"),
        Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
        "an else-if condition trace still rejects"
    );
}

#[test]
fn inspect_trace_in_each_key_is_invalid_placement() {
    // `{#each list as item (KEY)}` — the KEY is an EXPRESSION position stored SEPARATELY
    // from the block head (`SvelteBlockKind::Each { key }`), not in `head_expr`. Official
    // svelte@5.56.3 rejects a `$inspect.trace()` in the each-key with
    // `inspect_trace_invalid_placement`. RED before the fix: the each-key span was not
    // collected, so the trace escaped the exact placement code (generic refusal only).
    assert_eq!(
        gate("<script>let c = $state(0);</script>\n{#each [1] as i ($inspect.trace())}<p>{i}</p>{/each}\n"),
        Some(CoreOfficialValidationRule::InspectTraceInvalidPlacement),
        "a misplaced trace in an each-key rejects with the exact code"
    );
}

// ── ScriptBodyParse (same-scope redeclaration) ───────────────────────────────

#[test]
fn duplicate_state_declaration_is_script_body_parse() {
    // `let a = $state(0); let a = $state(1);` — a same-lexical-scope `let` redeclaration
    // Acorn (and the OXC body-probe) rejects in the PARSE phase: `js_parse_error`, owned by
    // the body-parse slot (NOT a later analyze-phase `declaration_duplicate`).
    assert_eq!(
            gate("<script>let a = $state(0); let a = $state(1);</script>\n<button onclick={() => a++}>{a}</button>\n"),
            Some(CoreOfficialValidationRule::ScriptBodyParse)
        );
}

#[test]
fn distinct_names_are_not_a_body_parse_error() {
    // NEGATIVE: distinct declarator names never collide — the body parses cleanly.
    assert_eq!(
            gate("<script>let a = $state(0); let b = $state(1);</script>\n<button onclick={() => a++}>{a}{b}</button>\n"),
            None
        );
}

// ── GlobalReferenceInvalid + the rune exclusion ──────────────────────────────

#[test]
fn runes_are_not_global_reference_violations() {
    // The CRITICAL negative: `$state` / `$derived` / `$props` / `$effect` etc. are
    // RUNE references, NOT undeclared store subscriptions — the gate must NOT flag
    // them as global-reference violations (the official `is_rune(name)` exclusion).
    // A component that ONLY uses runes passes cleanly.
    assert_eq!(
        gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
        None
    );
}

#[test]
fn undeclared_dollar_foo_reference_is_global_reference_invalid() {
    // `{$foo}` — an undeclared lowercase-initial `$foo` store subscription in runes
    // mode is `global_reference_invalid`.
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>x{$foo}{c}</button>\n"),
            Some(CoreOfficialValidationRule::GlobalReferenceInvalid)
        );
}

#[test]
fn double_dollar_reference_is_global_reference_invalid() {
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>x{$$bar}{c}</button>\n"),
            Some(CoreOfficialValidationRule::GlobalReferenceInvalid)
        );
}

#[test]
fn dollar_slots_reference_is_not_a_global_violation() {
    // NEGATIVE: `$$slots` is ACCEPTED by official (a valid magic object) — the gate
    // must NOT flag it as a global-reference reject (it is a deferrable unsupported
    // FEATURE handled downstream, not an official reject).
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>x{$$slots}{c}</button>\n"),
            None
        );
}

#[test]
fn uppercase_dollar_reference_is_not_a_global_violation() {
    // NEGATIVE: `$Foo` (uppercase-initial store name) is accepted by official (the
    // `/[a-z]/` lowercase-initial rule), so it is not a global-reference violation.
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>x{$Foo}{c}</button>\n"),
            None
        );
}

#[test]
fn dollar_props_magic_read_is_global_reference_invalid() {
    // `$$props` in the script — the official `legacy_props_invalid` class (mapped to
    // the GlobalReferenceInvalid rule).
    assert_eq!(
            gate("<script>let c = $state(0); let p = $$props;</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::GlobalReferenceInvalid)
        );
}

#[test]
fn shadowed_dollar_name_is_not_a_global_violation() {
    // NEGATIVE: a `$`-name bound by a local (an arrow param) is shadowed — not a
    // global reference. (`$foo` declared as a param shadows the global.)
    assert_eq!(
        gate("<script>let c = $state(0);</script>\n<button onclick={($foo) => c++}>{c}</button>\n"),
        None
    );
}

// ── bind:this targets ────────────────────────────────────────────────────────

#[test]
fn dollar_prefixed_bind_this_target_is_global_reference_invalid() {
    // `bind:this={$foo}` (no declaration) — the `$foo` REFERENCE is the official
    // `global_reference_invalid` class.
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<div bind:this={$foo}></div>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::GlobalReferenceInvalid)
        );
}

#[test]
fn undeclared_plain_bind_this_target_is_accepted() {
    // NEGATIVE: `bind:this={missing}` (an undeclared PLAIN identifier) is ACCEPTED by
    // official (the binding is implicitly created) — the gate must NOT reject it.
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<div bind:this={missing}></div>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
}

// ── HTML placement ───────────────────────────────────────────────────────────

#[test]
fn nested_button_is_node_invalid_placement() {
    assert_eq!(
        gate("<script>let c = $state(0);</script>\n<button><button>x</button></button>\n"),
        Some(CoreOfficialValidationRule::NodeInvalidPlacement)
    );
}

#[test]
fn nested_anchor_is_node_invalid_placement() {
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<a href=\"/\"><a href=\"/x\">x</a></a>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::NodeInvalidPlacement)
        );
}

#[test]
fn heading_in_heading_is_node_invalid_placement() {
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<h1><h1>x</h1></h1>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::NodeInvalidPlacement)
        );
}

#[test]
fn paragraph_with_block_descendant_and_explicit_close_is_element_autoclosed() {
    // `<p><div>…</div></p>` and `<p><p>…</p></p>` — a `<p>` auto-closed by a block
    // child WITH a surviving EXPLICIT `</p>`: official
    // `element_invalid_closing_tag_autoclosed`.
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<p><div>x</div></p>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ElementInvalidClosingTagAutoclosed)
        );
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<p><p>x</p></p>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ElementInvalidClosingTagAutoclosed)
        );
}

#[test]
fn paragraph_with_block_descendant_but_no_explicit_close_is_not_a_reject() {
    // FALSE-POSITIVE FIX: `<p><div>x</div>` with NO explicit `</p>` is official-
    // ACCEPTED (the browser auto-closes the `<p>`, a warning). It must NOT be an
    // official reject — neither `element_invalid_closing_tag_autoclosed` NOR
    // `element_unclosed` (the parser sees the `<p>` as unclosed, but official
    // auto-closes it). The gate returns None; the implicit case fails closed as an
    // unsupported FEATURE downstream.
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<p><div>x</div>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<p><h1>x</h1>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
}

// ── close-tag well-formedness rules ──────────────────────────────────────────

#[test]
fn unclosed_button_is_element_unclosed() {
    assert_eq!(
        gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}"),
        Some(CoreOfficialValidationRule::ElementUnclosed)
    );
}

#[test]
fn stray_close_is_element_invalid_closing_tag() {
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n</div>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ElementInvalidClosingTag)
        );
}

#[test]
fn mismatched_close_is_element_invalid_closing_tag() {
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}><div>{c}</span></button>\n"),
            Some(CoreOfficialValidationRule::ElementInvalidClosingTag)
        );
}

#[test]
fn void_element_explicit_close_is_void_invalid_content() {
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<input></input>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::VoidElementInvalidContent)
        );
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<input>x</input>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::VoidElementInvalidContent)
        );
}

#[test]
fn well_formed_section_1_2_records_no_close_tag_reject() {
    // NEGATIVE: the §1.2 headline shape (well-formed, all closed, void `<input>`
    // self-closed) is NOT a close-tag violation.
    assert_eq!(
            gate("<script>let name = $state('world'); let count = $state(0);</script>\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n"),
            None
        );
}

#[test]
fn button_inside_anchor_is_accepted() {
    // NEGATIVE: `<a><button>` is VALID (official accepts it) — the gate must NOT
    // reject every nested element, only the disallowed-descendant families.
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<a href=\"/\"><button onclick={() => c++}>{c}</button></a>\n"),
            None
        );
}

#[test]
fn sibling_supported_elements_are_accepted() {
    // NEGATIVE: the §1.2-class sibling element layout (`<h1>` + `<input>` +
    // `<button>` at the root) is a valid placement — no violation.
    assert_eq!(
            gate("<script>let name = $state('world'); let count = $state(0);</script>\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n"),
            None
        );
}

// ── script-domain rules ──────────────────────────────────────────────────────

#[test]
fn duplicate_instance_script_is_script_duplicate() {
    assert_eq!(
            gate("<script>let c = $state(0);</script>\n<script>let d = $state(0);</script>\n<button onclick={() => c++}>{c}{d}</button>\n"),
            Some(CoreOfficialValidationRule::ScriptDuplicate)
        );
}

#[test]
fn invalid_script_context_is_script_invalid_context() {
    assert_eq!(
            gate("<script context=\"bad\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ScriptInvalidContext)
        );
}

#[test]
fn reserved_script_attribute_is_script_reserved_attribute() {
    // `<script server>` — a RESERVED script attribute: official `script_reserved_attribute`.
    assert_eq!(
            gate("<script server>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ScriptReservedAttribute)
        );
}

#[test]
fn duplicate_script_attribute_is_attribute_duplicate() {
    // `<script lang="js" lang="js">` — a DUPLICATE script attribute: official
    // `attribute_duplicate` (the element-attribute loop runs for the top-level script).
    assert_eq!(
            gate("<script lang=\"js\" lang=\"js\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::AttributeDuplicate)
        );
}

#[test]
fn capitalized_context_attribute_name_is_not_a_reject() {
    // FALSE-POSITIVE FIX: `<script Context="bad">` — `Context` (capital C) is an
    // UNKNOWN attribute (official emits a `script_unknown_attribute` WARNING and
    // ACCEPTS), NOT `script_invalid_context`. The attribute NAME match is
    // case-sensitive, so the gate must NOT over-reject it.
    assert_eq!(
            gate("<script Context=\"bad\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
}

#[test]
fn valued_module_attribute_is_script_invalid_context() {
    // A valued `module="x"` is the official `script_invalid_attribute_value` (mapped
    // to the ScriptInvalidContext rule), and it wins over the duplicate-script
    // refusal (official validates per-script attributes first).
    assert_eq!(
        gate("<script module=\"x\">const K = 1;</script>\n<button>x</button>\n"),
        Some(CoreOfficialValidationRule::ScriptInvalidContext)
    );
}

#[test]
fn valid_module_context_is_accepted() {
    // NEGATIVE: a valid `context="module"` / `<script module>` is not a violation.
    assert_eq!(
            gate("<script context=\"module\">const K = 1;</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
    assert_eq!(
            gate("<script module>const K = 1;</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
}

// ── from_unsupported_surface mapping ─────────────────────────────────────────

#[test]
fn from_unsupported_surface_maps_only_the_official_reject_surfaces() {
    use crate::svelte::runtime::UnsupportedSvelteRuntimeSurface;
    let span = verter_span::Span::new(0, 0);
    // OptionsAxis (a NON-duplicate unsupported options axis) maps; an unsupported FEATURE
    // does not. (A template `attribute_duplicate` and a duplicate `<svelte:options>` are
    // now EXACT-CODE parser facts carried by the official-reject gate, NOT mapped from an
    // unsupported surface, so there is no `DuplicateAttribute` surface to map.)
    assert_eq!(
        CoreOfficialValidationRule::from_unsupported_surface(
            &UnsupportedSvelteRuntimeSurface::OptionsAxis { span }
        ),
        Some(CoreOfficialValidationRule::OptionsInvalid)
    );
    // A pure unsupported FEATURE (a `{#if}` block) is NOT an official reject.
    assert_eq!(
        CoreOfficialValidationRule::from_unsupported_surface(
            &UnsupportedSvelteRuntimeSurface::Block {
                construct: "if",
                span,
            }
        ),
        None
    );
    // An AdvancedRune surface is NOT auto-mapped (ambiguous: official-reject arity
    // vs deferrable `$state.raw`).
    assert_eq!(
        CoreOfficialValidationRule::from_unsupported_surface(
            &UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$state.raw",
                span,
            }
        ),
        None
    );
}

#[test]
fn rule_names_round_trip() {
    for &rule in CoreOfficialValidationRule::ALL {
        assert_eq!(
            CoreOfficialValidationRule::from_name(rule.name()),
            Some(rule)
        );
    }
    assert_eq!(CoreOfficialValidationRule::from_name("NotARule"), None);
}
