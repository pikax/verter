//! @ai-generated - discriminated-union narrowing contracts.
//!
//! Each test pins ONE TS7 emission for discriminated-union narrowing
//! across `s.kind === "..."` / `switch (s.kind)` / multi-property
//! discriminants — the single most common TS pattern in real codebases
//! (Redux, AST visitors, Vue events). Scenarios cover string-literal,
//! number-literal, boolean-literal, template-literal, and `in`-guard
//! discriminants; nested guards; exhaustiveness checks; per-arm joins;
//! fall-through; shared properties; destructure correlation; and
//! reassignment re-narrowing.
//!
//! Fixture emissions verified against tsgo 7.0.0-dev.20260523.1 via IsExactly
//! probes BEFORE encoding the Rust assertions.
//! Generated switch sizes and unit controls are checked against pinned
//! TypeScript 7.0.2 with exact-type probes.
//!
//! Each scenario is one `*Result = ReturnType<typeof duXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.

use super::support::*;
use std::fmt::Write;

const NARROW_DISCRIMINATED_UNION: &str = include_str!("fixtures/narrow_discriminated_union.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(
        host,
        "/fixtures/narrow_discriminated_union.ts",
        NARROW_DISCRIMINATED_UNION,
    );
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/narrow_discriminated_union.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

fn resolve_switch(source: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    let path = "/fixtures/switch.ts";
    upsert_ts(&host, path, source);
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(path),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from("f"),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let flow = host.get_flow_return_type_with_audit(
        &identity,
        verter_type_engine::semantic_query::ReturnProjectionDemand::whole_return(),
    );
    let fact = flow.as_result().expect("switch flow must resolve").fact();
    assert!(
        matches!(
            fact,
            verter_type_engine::semantic_query::FactResult::Complete(_)
        ),
        "switch must be complete: {fact:?}"
    );
    resolve_expr(&host, path, "Result", &[], ProjectionMode::Expanded).0
}

fn assert_switch_literals(expr: TypeExpr, expected: &[i32], undefined: bool) {
    let arms = match &expr {
        TypeExpr::Union(arms) => arms.to_vec(),
        other => vec![other.clone()],
    };
    let mut values = Vec::new();
    let mut has_undefined = false;
    for arm in arms {
        match arm {
            TypeExpr::Literal(verter_type_expr::LiteralValue::Number(value)) => {
                assert_eq!(value.fract(), 0.0, "integer literal");
                values.push(value as i32);
            }
            TypeExpr::Primitive(PrimitiveName::Undefined) => has_undefined = true,
            other => panic!("expected exact switch return literals, got {other:?}"),
        }
    }
    values.sort_unstable();
    assert_eq!(values, expected);
    assert_eq!(has_undefined, undefined);
}

fn assert_large_switch(count: i32) {
    let mut source = String::from("type U = ");
    for index in 0..count {
        if index != 0 {
            source.push_str(" | ");
        }
        write!(source, "{{ kind: \"k{index}\"; v{index}: {index} }}").unwrap();
    }
    source.push_str("; function f(x: U) { switch (x.kind) {");
    for index in 0..count {
        write!(source, "case \"k{index}\": return x.v{index};").unwrap();
    }
    source.push_str("} } type Result = ReturnType<typeof f>;");
    assert_switch_literals(
        resolve_switch(&source),
        &(0..count).collect::<Vec<_>>(),
        false,
    );
}

#[test]
fn switch_union_50_cases() {
    assert_large_switch(50);
}

#[test]
fn switch_union_300_cases() {
    assert_large_switch(300);
}

#[test]
fn switch_union_1000_cases() {
    assert_large_switch(1000);
}

#[test]
fn switch_union_3000_cases() {
    assert_large_switch(3000);
}

#[test]
fn switch_union_controls() {
    let declaration =
        "type U = {kind: 'a'; value: 1} | {kind: 'b'; value: 2} | {kind: 'c'; value: 3};";
    for (body, expected, undefined) in [
        ("case 'a': case 'b': return x.value; case 'c': return x.value;", vec![1, 2, 3], false),
        ("case 'a': return x.value; default: return x.value;", vec![1, 2, 3], false),
        ("default: return x.value; case 'a': return x.value;", vec![1, 2, 3], false),
        ("case 'a': return x.value;", vec![1], true),
        ("case 'a': return x.value; case 'a': return x.value; case 'b': return x.value; case 'c': return x.value;", vec![1, 2, 3], false),
    ] {
        let source = format!("{declaration} function f(x: U) {{ switch (x.kind) {{ {body} }} }} type Result = ReturnType<typeof f>;");
        assert_switch_literals(resolve_switch(&source), &expected, undefined);
    }
    // A reference after a non-exhaustive switch reads the unhandled member.
    let source = format!("{declaration} function f(x: U) {{ switch (x.kind) {{ case 'a': return 10; case 'b': return 20; }} return x.value; }} type Result = ReturnType<typeof f>;");
    assert_switch_literals(resolve_switch(&source), &[3, 10, 20], false);
    let source = format!("{declaration} function f(x: U) {{ switch (true) {{ case x.kind === 'a': return x.value; case x.kind === 'b': return x.value; default: return x.value; }} }} type Result = ReturnType<typeof f>;");
    assert_switch_literals(resolve_switch(&source), &[1, 2, 3], false);
}

#[test]
fn switch_fallback_refines_the_case_tag() {
    let source = "type U = {kind: 'a' | 'b'; value: 1} | {kind: 'c'; value: 2}; function f(x: U) { switch (x.kind) { case 'a': return x.kind; default: return 'handled' as const; } } type Result = ReturnType<typeof f>;";
    assert_literal_union(&resolve_switch(source), &["a", "handled"]);
}

#[test]
fn switch_fallback_subtracts_handled_tags() {
    for (body, tail) in [
        ("case 'a': return 'handled' as const;", "return x.kind;"),
        (
            "case 'a': return 'handled' as const; default: return x.kind;",
            "",
        ),
    ] {
        let source = format!("type U = {{kind: 'a' | 'b'; value: 1}} | {{kind: 'c'; value: 2}}; function f(x: U) {{ switch (x.kind) {{ {body} }} {tail} }} type Result = ReturnType<typeof f>;");
        assert_literal_union(&resolve_switch(&source), &["b", "c", "handled"]);
    }
}

#[test]
fn switch_preserves_exact_discriminant_writes() {
    for (parameter, discriminant) in [("U", "x.kind"), ("{payload: U}", "x.payload.kind")] {
        for (declaration, written, expected) in [
            (
                "{kind: 'a'; value: 1} | {kind: 'b'; value: 2}",
                "a",
                vec!["b", "handled"],
            ),
            (
                "{kind: 'a'; value: 1} | {kind: 'b'; value: 2} | {kind: 'c'; value: 3}",
                "b",
                vec!["a", "c", "handled"],
            ),
        ] {
            let source = format!("type U = {declaration}; function f(x: {parameter}) {{ {discriminant} = '{written}'; switch ({discriminant}) {{ case '{written}': return 'handled' as const; default: return {discriminant}; }} }} type Result = ReturnType<typeof f>;");
            assert_literal_union(&resolve_switch(&source), &expected);
        }
    }
}

#[test]
fn switch_unit_discriminant_write_recovery() {
    // TS2678 diagnoses the incompatible label but declaration emission still
    // contributes the selected parent's unit tag from that authored case.
    for (parameter, discriminant) in [("U", "x.kind"), ("{payload: U}", "x.payload.kind")] {
        let source = format!("type U = {{kind: 'a'; value: 1}} | {{kind: 'b'; value: 2}}; function f(x: {parameter}) {{ {discriminant} = 'a'; switch ({discriminant}) {{ case 'b': return {discriminant}; default: return 'handled' as const; }} }} type Result = ReturnType<typeof f>;");
        assert_literal_union(&resolve_switch(&source), &["b", "handled"]);
    }
}

#[test]
fn switch_preserves_union_valued_discriminant_writes() {
    for (parameter, discriminant) in [("U", "x.kind"), ("{payload: U}", "x.payload.kind")] {
        for body in [
            // TS2678 is expected; unlike a unit tag, the union-valued tag
            // retains the write and contributes never from the case read.
            "case 'a': return DISCRIMINANT; default: return 'handled' as const;",
            "case 'b': return 'handled' as const; default: return DISCRIMINANT;",
            "case 'b': return 'handled' as const;",
        ] {
            let body = body.replace("DISCRIMINANT", discriminant);
            let source = format!("type U = {{kind: 'a' | 'b'; value: 1}} | {{kind: 'c'; value: 2}}; function f(x: {parameter}) {{ {discriminant} = 'b'; switch ({discriminant}) {{ {body} }} }} type Result = ReturnType<typeof f>;");
            assert_string_literal(&resolve_switch(&source), "handled");
        }
    }
}

#[test]
fn switch_exhaustiveness_reads_the_written_discriminant() {
    let source = "type U = {kind: 'a'; value: 1} | {kind: 'b'; value: 2} | {kind: 'c'; value: 3}; function f(x: U) { x.kind = 'b'; switch (x.kind) { case 'b': return 'handled' as const; } } type Result = ReturnType<typeof f>;";
    assert_string_literal(&resolve_switch(source), "handled");
}

#[test]
fn parameter_assignment_updates_member_reads() {
    for (body, expected) in [
        ("x = y; return x.value;", vec![2]),
        (
            "switch (x.kind) { case 'a': x = y; break; case 'b': return 20; } return x.value;",
            vec![2, 20],
        ),
    ] {
        let source = format!("type U = {{kind: 'a' | 'b'; value: 1}} | {{kind: 'c'; value: 2}}; function f(x: U, y: {{kind: 'c'; value: 2}}) {{ {body} }} type Result = ReturnType<typeof f>;");
        assert_switch_literals(resolve_switch(&source), &expected, false);
    }
}

#[test]
fn named_union_local_assignments_select_the_declared_member() {
    for declaration in [
        "type U = {kind: 'a'; value: 1} | {kind: 'b'; value: 2}; type Named = U;",
        "type U<T> = {kind: 'a'; value: T} | {kind: 'b'; value: 2}; type Named = U<1>;",
        "type U<T> = {kind: 'a'; value: T} | {kind: 'b'; value: 2}; type Forward<T> = U<T>; type Named = Forward<1>;",
        "type U<T = 1> = {kind: 'a'; value: T} | {kind: 'b'; value: 2}; type Named = U;",
    ] {
        for body in [
            "let x: Named = y; return x.value;",
            "let x: Named; x = y; return x.value;",
        ] {
            let source = format!("{declaration} function f(y: {{kind: 'b'; value: 2}}) {{ {body} }} type Result = ReturnType<typeof f>;");
            assert_switch_literals(resolve_switch(&source), &[2], false);
        }
    }
}

#[test]
fn switch_fallback_multi_literal_fallout() {
    let declaration = "type U = {kind: 'a' | 'b'; value: 1} | {kind: 'c'; value: 2};";
    for (body, expected) in [
        ("case 'a': return 10; case 'b': return 20;", vec![2, 10, 20]),
        ("case 'a': return 10;", vec![1, 2, 10]),
        ("case 'a': break; case 'b': return 20;", vec![1, 2, 20]),
        (
            "case 'a': x = {kind: 'c', value: 2}; return 10; case 'b': return 20;",
            vec![2, 10, 20],
        ),
    ] {
        let source = format!("{declaration} function f(x: U) {{ switch (x.kind) {{ {body} }} return x.value; }} type Result = ReturnType<typeof f>;");
        assert_switch_literals(resolve_switch(&source), &expected, false);
    }
    let source = format!("{declaration} function f(x: U) {{ switch (x.kind) {{ case 'a': return 10; case 'b': return 20; default: return x.value; }} }} type Result = ReturnType<typeof f>;");
    assert_switch_literals(resolve_switch(&source), &[2, 10, 20], false);
    let source = format!("{declaration} function f(x: {{payload: U}}) {{ switch (x.payload.kind) {{ case 'a': return 10; case 'b': return 20; }} return x.payload.value; }} type Result = ReturnType<typeof f>;");
    assert_switch_literals(resolve_switch(&source), &[2, 10, 20], false);
}

#[test]
fn switch_fallback_enum_fallout() {
    let declaration = "enum Kind { A = 'a', B = 'b', C = 'c' } type U = {kind: Kind.A; value: 1} | {kind: Kind.B; value: 2} | {kind: Kind.C; value: 3};";
    for (body, tail) in [
        (
            "case Kind.A: return 10; case Kind.B: return 20;",
            "return x.value;",
        ),
        (
            "case Kind.A: return 10; case Kind.B: return 20; default: return x.value;",
            "",
        ),
    ] {
        let source = format!("{declaration} function f(x: U) {{ switch (x.kind) {{ {body} }} {tail} }} type Result = ReturnType<typeof f>;");
        assert_switch_literals(resolve_switch(&source), &[3, 10, 20], false);
    }
}

#[test]
fn switch_union_unit_discriminants_and_shared_labels() {
    for (declaration, discriminant, cases) in [
        (
            "{kind: 0; value: 1} | {kind: 1; value: 2}",
            "x.kind",
            "case -0: return x.value; case 1: return x.value;",
        ),
        (
            "{kind: true; value: 1} | {kind: false; value: 2}",
            "x.kind",
            "case true: return x.value; case false: return x.value;",
        ),
        (
            "{kind: null; value: 1} | {kind: undefined; value: 2}",
            "x.kind",
            "case null: return x.value; case undefined: return x.value;",
        ),
        (
            "{kind: 'a'; value: 1} | {kind: 'a'; value: 2} | {kind: 'b'; value: 2}",
            "x.kind",
            "case 'a': return x.value; default: return x.value;",
        ),
        (
            "{kind: 'a'; value: 1} | {kind: 'b'; value: 2}",
            "x.payload.kind",
            "case 'a': return x.payload.value; case 'b': return x.payload.value;",
        ),
    ] {
        let parameter = if discriminant == "x.payload.kind" {
            "{payload: U}"
        } else {
            "U"
        };
        let source = format!("type U = {declaration}; function f(x: {parameter}) {{ switch ({discriminant}) {{ {cases} }} }} type Result = ReturnType<typeof f>;");
        assert_switch_literals(resolve_switch(&source), &[1, 2], false);
    }
}

// ----- 1) if (s.kind === "a") on {kind:"a";a:string} | {kind:"b";b:number}
// TS7: if-branch sees the a-arm (returns string), else sees the b-arm
// (returns number). Joined: string | number.
#[test]
fn narrow_discriminated_union_du01_if_equality_discriminant() {
    let expr = resolve_alias("Du01Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 2) switch(s.kind) over "a" | "b" --------------------------------
// TS7: case "a" returns string, case "b" returns number. Joined: string | number.
#[test]
fn narrow_discriminated_union_du02_switch_discriminant() {
    let expr = resolve_alias("Du02Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 3) switch with default: never exhaustiveness check ---------------
// TS7: cases return their arm-specific types; default is unreachable
// (`const _exhaustive: never = s`) and contributes `never` (absorbed).
// Joined: string | number.
#[test]
fn narrow_discriminated_union_du03_switch_default_never() {
    let expr = resolve_alias("Du03Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    // never absorbed: the joined union must NOT contain `never`.
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_never = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::Never)));
    assert!(
        !has_never,
        "switch-default-never joined return must absorb never from the unreachable default; got {expr:?}"
    );
}

// ----- 4) if (s.kind !== "a") ------------------------------------------
// TS7: negated if-branch returns s.b (number); else returns s.a (string).
// Joined: string | number.
#[test]
fn narrow_discriminated_union_du04_negated_discriminant() {
    let expr = resolve_alias("Du04Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 5) Multi-property discriminant kind === "a" && tag === 1 --------
// TS7: compound `&&` guard narrows to the SINGLE first arm
// `{kind:"a"; tag:1; a1:string}`. if-branch returns s.a1 (string).
// else returns null. Joined: string | null.
#[test]
fn narrow_discriminated_union_du05_multi_property_discriminant() {
    let expr = resolve_alias("Du05Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}

// ----- 6) Nested discriminants -----------------------------------------
// TS7: outer `s.outer === "o1"` narrows to the first outer arm, then
// `s.inner.kind === "ia"` narrows the inner union. if-branch returns
// s.inner.ia (string). else returns null. Joined: string | null.
#[test]
fn narrow_discriminated_union_du06_nested_discriminant() {
    let expr = resolve_alias("Du06Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}

// ----- 7) Discriminant on number-literal type --------------------------
// TS7: if-branch (kind === 1) returns s.a (string); else returns s.b (number).
// Joined: string | number.
#[test]
fn narrow_discriminated_union_du07_number_literal_discriminant() {
    let expr = resolve_alias("Du07Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 8) Discriminant on boolean-literal type -------------------------
// TS7: `if (s.ok)` narrows on truthiness of a boolean-literal-typed property.
// if-branch (ok === true) returns s.data (string); else (ok === false)
// returns s.err (number). Joined: string | number.
#[test]
fn narrow_discriminated_union_du08_boolean_literal_discriminant() {
    let expr = resolve_alias("Du08Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 9) Discriminant via property destructure ------------------------
// TS7 quirk: destructuring `const { kind } = s` PROPAGATES the
// discriminated-union correlation between `kind` and `s` — the if-branch
// narrows BOTH the destructured `kind` AND the original `s`. if-branch
// returns s.a (string); else returns s.b (number). Joined: string | number.
#[test]
fn narrow_discriminated_union_du09_destructure_correlation() {
    let expr = resolve_alias("Du09Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 10) Discriminant present in only one arm -----------------------
// TS7: second arm has no `kind` property, so `("kind" in s)` narrows out
// the second arm first; then `s.kind === "a"` selects the first.
// if-branch returns s.a (string); else returns null. Joined: string | null.
#[test]
fn narrow_discriminated_union_du10_in_guard_plus_discriminant() {
    let expr = resolve_alias("Du10Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}

// ----- 11) Switch returning per-arm types -----------------------------
// TS7: case "a" -> string, case "b" -> number. Joined: string | number.
// Identical structure to scenario 2; pinned as the joined-return contract.
#[test]
fn narrow_discriminated_union_du11_switch_per_arm_join() {
    let expr = resolve_alias("Du11Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 12) Switch with fall-through case "a": case "b": --------------
// TS7: fall-through `case "a": case "b":` block narrows s to the union
// of those two arms; s.payload is `string | number`. case "c" returns
// s.flag (boolean). Joined: string | number | boolean.
#[test]
fn narrow_discriminated_union_du12_switch_fall_through() {
    let expr = resolve_alias("Du12Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
}

// ----- 13) Discriminated union with shared property ------------------
// TS7: both arms carry `shared: string`. Inside the if-branch s.a (1)
// and s.shared (string) are both accessible; the branch returns
// `{ v: 1; sh: string }`. else returns `{ v: 2; sh: string }`.
// Joined: `{ v: 1; sh: string } | { v: 2; sh: string }`.
#[test]
fn narrow_discriminated_union_du13_shared_property() {
    let expr = resolve_alias("Du13Result");
    // Joined: union of two object arms, each carrying `v` and `sh`.
    assert_union_has_object_arm(&expr, &["sh", "v"]);
}

// ----- 14) Re-narrowing after reassignment ---------------------------
// TS7 quirk: inside the if-branch `s = { kind: "b", b: 0 }` re-narrows s
// to the static type of the new value. The if-body returns s.b (number);
// the else returns s.b (number, from the original "b" arm). Joined: number.
#[test]
#[ignore = "typeinfo currently does not propagate re-narrowing after reassignment inside a discriminated-union branch through `ReturnType<typeof fn>`; keep as the future Du14 reassignment-re-narrowing contract"]
fn narrow_discriminated_union_du14_reassignment_re_narrowing() {
    let expr = resolve_alias("Du14Result");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 15) Tagged-template-style discriminant ------------------------
// TS7 quirk: `kind: \`prefix-${string}\`` is a template-literal type.
// Comparing against the concrete `"prefix-foo"` literal (which IS
// assignable to `\`prefix-${string}\``) selects the first arm in the
// if-branch. if-branch returns s.a (string); else returns null.
// Joined: string | null.
#[test]
#[ignore = "typeinfo currently does not propagate template-literal discriminant narrowing (`s.kind === \"prefix-foo\"` against `kind: \\`prefix-${string}\\``) through `ReturnType<typeof fn>`; keep as the future Du15 template-literal-discriminant contract"]
fn narrow_discriminated_union_du15_template_literal_discriminant() {
    let expr = resolve_alias("Du15Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
}
