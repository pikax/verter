//! A closed indexed access or `keyof` inside a published answer — an
//! object property, a tuple element, a union member, a signature, a type
//! argument — reduces as it does at the answer's root: the checker reads
//! the member or the keys when it builds the type, wherever the operator
//! is written.
//!
//! Each demand is the named demand the semantic benchmark makes (`__Probe`
//! resolved by name and projected to its wire answer). Every expected
//! answer is TypeScript 7.0.2's, read through its native API as the
//! benchmark's tsc arm reads it (`getTypeAtPosition` on `__Probe`, then
//! `typeToString`, a union member by member) over the same module.
//! The object, tuple, key-union and route-union reproductions cover all
//! four `strictNullChecks` / `noImplicitAny` combinations.
//!
//! An operator over a type parameter stays the deferred operator the
//! checker prints, a `keyof` inside an object or a type argument stays the
//! `keyof P` the checker prints there, and a resolution at a demand-driven
//! altitude keeps its carriers.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_type_engine::semantic_query::ProjectionMode;
use verter_type_expr::{LiteralValue, ObjectMember, TypeExpr};

const SCENARIO: &str = "/nested/scenario.ts";

/// The utility declarations the benchmark's module declares beside its
/// probe.
const UTILITIES: &str = "type Exclude<T, U> = T extends U ? never : T;\n\
                         type Extract<T, U> = T extends U ? T : never;\n";

/// Resolve `__Probe = probe` at `mode` in a fresh project holding `source`
/// and decode its wire answer, as the benchmark's probe does.
fn answer_at(source: &str, probe: &str, mode: Option<ProjectionMode>) -> TypeExpr {
    answer_with_options(source, probe, mode, true, true)
}

const STRICTNESS: [(bool, bool); 4] = [(true, true), (false, true), (true, false), (false, false)];

fn answer_with_options(
    source: &str,
    probe: &str,
    mode: Option<ProjectionMode>,
    strict_null_checks: bool,
    no_implicit_any: bool,
) -> TypeExpr {
    let config = serde_json::json!({
        "compilerOptions": {
            "strict": true,
            "strictNullChecks": strict_null_checks,
            "noImplicitAny": no_implicit_any,
        }
    })
    .to_string();
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/nested", &config)],
    );
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(SCENARIO.into()),
            input_id: SCENARIO.into(),
            source: Arc::from(format!(
                "type __BenchInit = 0;\n{UTILITIES}{source}type __Probe = {probe};\nexport {{}};\n"
            )),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
    resolve_answer(&host, SCENARIO, mode)
}

fn resolve_answer(
    host: &crate::VerterHost,
    canonical: &str,
    mode: Option<ProjectionMode>,
) -> TypeExpr {
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(canonical, "__Probe", mode)
        .into_parts();
    let node = outcome
        .expect("the named demand completes")
        .expect("the probe exists");
    let wire = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("the answer projects");
    let value: serde_json::Value = serde_json::from_slice(&wire).expect("the wire answer is JSON");
    verter_type_expr::type_expr_from_json(&value).expect("a wire TypeExpr")
}

/// The answer the benchmark's default demand reads.
fn answer(source: &str, probe: &str) -> TypeExpr {
    answer_at(source, probe, None)
}

fn display(expr: &TypeExpr) -> String {
    verter_type_expr::render_type_expr_display(expr)
        .expect("the answer renders")
        .text
}

/// A union's members as a set of prints (a union's order is not its
/// meaning); any other answer as the one print it is.
fn members(expr: &TypeExpr) -> BTreeSet<String> {
    match expr {
        TypeExpr::Union(members) => members.iter().map(display).collect(),
        other => BTreeSet::from([display(other)]),
    }
}

fn set<const N: usize>(prints: [&str; N]) -> BTreeSet<String> {
    prints.into_iter().map(String::from).collect()
}

/// The type of the property `name` of the object `answer`.
fn property<'a>(answer: &'a TypeExpr, name: &str) -> &'a TypeExpr {
    let TypeExpr::Object(object) = answer else {
        panic!("`{}` is not an object type", display(answer));
    };
    object
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if matches!(
                    &property.key,
                    verter_type_expr::AuthoredPropertyKey::String(key) if key.as_ref() == name
                ) =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{}` has no property `{name}`", display(answer)))
}

/// Measured on TypeScript 7.0.2: `typeof __v` over `declare const __v: {
/// x: { a: 1 }["a"] }` is `{ x: 1; }`, and so is the alias `O` over `type
/// O = { x: X["a"] }` with `type X = { a: 1 }`.
#[test]
fn a_closed_access_in_an_object_property_reads_the_member() {
    for (strict_null_checks, no_implicit_any) in STRICTNESS {
        let declared = answer_with_options(
            "declare const __v: { x: { a: 1 }[\"a\"] };\n",
            "typeof __v",
            None,
            strict_null_checks,
            no_implicit_any,
        );
        assert_eq!(
            property(&declared, "x"),
            &TypeExpr::Literal(LiteralValue::Number(1.0)),
            "strictNullChecks={strict_null_checks}, noImplicitAny={no_implicit_any}: `{}`",
            display(&declared)
        );
    }
    let aliased = answer("type X = { a: 1 };\ntype O = { x: X[\"a\"] };\n", "O");
    assert_eq!(display(&aliased), "{ x: 1 }");
}

/// Measured on TypeScript 7.0.2: `[[1, 2?]["length"]]` is `[1 | 2]`, and
/// `[keyof P, X["a"]]` is `[keyof P, 1]`.
#[test]
fn a_closed_access_in_a_tuple_element_reads_the_member() {
    for (strict_null_checks, no_implicit_any) in STRICTNESS {
        let tuple = answer_with_options(
            "",
            "[[1, 2?][\"length\"]]",
            None,
            strict_null_checks,
            no_implicit_any,
        );
        let TypeExpr::Tuple { elements, .. } = &tuple else {
            panic!("the answer is a tuple");
        };
        assert_eq!(elements.len(), 1);
        assert_eq!(
            members(&elements[0].ty),
            set(["1", "2"]),
            "strictNullChecks={strict_null_checks}, noImplicitAny={no_implicit_any}"
        );
    }
    let mixed = answer(
        "type P = { a: 1; b: 2 };\ntype X = { a: 1 };\n",
        "[keyof P, X[\"a\"]]",
    );
    assert_eq!(display(&mixed), "[keyof P, 1]");
}

/// Measured on TypeScript 7.0.2: `keyof P | 3` is `"a" | "b" | 3`, and
/// `keyof P | "a"` is `"a" | "b"`.
#[test]
fn a_closed_keyof_in_a_union_reduces_over_its_keys() {
    let source = "type P = { a: 1; b: 2 };\n";
    for (strict_null_checks, no_implicit_any) in STRICTNESS {
        assert_eq!(
            members(&answer_with_options(
                source,
                "keyof P | 3",
                None,
                strict_null_checks,
                no_implicit_any,
            )),
            set(["'a'", "'b'", "3"]),
            "strictNullChecks={strict_null_checks}, noImplicitAny={no_implicit_any}"
        );
    }
    assert_eq!(
        members(&answer(source, "keyof P | \"a\"")),
        set(["'a'", "'b'"])
    );
}

/// Measured on TypeScript 7.0.2: `RouteNamedMap["r19"]["params"] | string
/// | undefined` is `undefined | string | { id19: string; }` with strict
/// null checks, and omits `undefined` without them. Inside an object
/// property the same chain is `{ id19: string; }`.
#[test]
fn a_closed_access_chain_in_a_union_member_reads_the_member() {
    let source = "interface RouteNamedMap { r19: { params: { id19: string } } }\n";
    for (strict_null_checks, no_implicit_any) in STRICTNESS {
        let expected = if strict_null_checks {
            set(["{ id19: string }", "string", "undefined"])
        } else {
            set(["{ id19: string }", "string"])
        };
        assert_eq!(
            members(&answer_with_options(
                source,
                "RouteNamedMap[\"r19\"][\"params\"] | string | undefined",
                None,
                strict_null_checks,
                no_implicit_any,
            )),
            expected,
            "strictNullChecks={strict_null_checks}, noImplicitAny={no_implicit_any}"
        );
    }
    assert_eq!(
        display(&answer(source, "{ p: RouteNamedMap[\"r19\"][\"params\"] }")),
        "{ p: { id19: string } }"
    );
}

/// Identical syntax in strict and loose projects shares a graph, but a
/// published union's nullability follows its declaring project even on
/// alternating warm reads. A cold Navigate keeps the authored union;
/// an Expanded answer may subsequently backfill that narrower demand.
#[test]
fn expanded_union_nullability_follows_its_project_on_warm_reads() {
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[
            ("/strict", r#"{ "compilerOptions": { "strict": true } }"#),
            (
                "/loose",
                r#"{ "compilerOptions": { "strict": true, "strictNullChecks": false } }"#,
            ),
        ],
    );
    for canonical in ["/strict/scenario.ts", "/loose/scenario.ts"] {
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(canonical.into()),
                input_id: canonical.into(),
                source: Arc::from("type __Probe = string | undefined;\nexport {};"),
                file_language: crate::FileLanguage::script_ts(),
                aliases: vec![],
            })
            .expect("fixture admitted");
    }
    let navigated = resolve_answer(&host, "/loose/scenario.ts", Some(ProjectionMode::Navigate));
    assert!(
        members(&navigated).contains("undefined"),
        "a cold Navigate keeps the authored nullability carrier: {}",
        display(&navigated)
    );
    for canonical in [
        "/strict/scenario.ts",
        "/loose/scenario.ts",
        "/strict/scenario.ts",
        "/loose/scenario.ts",
    ] {
        let expected = if canonical.starts_with("/strict/") {
            set(["string", "undefined"])
        } else {
            set(["string"])
        };
        assert_eq!(
            members(&resolve_answer(&host, canonical, None)),
            expected,
            "{canonical}"
        );
    }
}

/// Measured on TypeScript 7.0.2: over `type X = { a: Y }` with `interface
/// Y { y: 1 }`, `{ x: X["a"] }` is `{ x: Y; }` and `X["a"] | 3` is `3 |
/// Y`; a method's return `X["a"]` with `type X = { a: 1 }` is `1`.
#[test]
fn a_closed_access_reading_a_declaration_is_that_declaration() {
    let source = "interface Y { y: 1 }\ntype X = { a: Y };\n";
    assert_eq!(display(&answer(source, "{ x: X[\"a\"] }")), "{ x: Y }");
    assert_eq!(members(&answer(source, "X[\"a\"] | 3")), set(["Y", "3"]));
    assert_eq!(
        display(&answer("type X = { a: 1 };\n", "{ f(): X[\"a\"] }")),
        "{ f(): 1 }"
    );
}

/// Measured on TypeScript 7.0.2: `G<{ a: 1 }>` over `type G<T extends { a:
/// unknown }> = { x: T["a"]; y: keyof T }` is `{ x: 1; y: "a"; }`, and in
/// `{ f: <T extends { a: unknown }>(t: T) => T["a"] | X["a"] }` the access
/// over `T` stays `T["a"]` beside the closed one's `1`.
#[test]
fn an_access_over_a_type_parameter_stays_deferred_until_instantiated() {
    let instantiated = answer(
        "type G<T extends { a: unknown }> = { x: T[\"a\"]; y: keyof T };\n",
        "G<{ a: 1 }>",
    );
    assert_eq!(display(&instantiated), "{ x: 1; y: 'a' }");
    let generic = answer(
        "type X = { a: 1 };\n",
        "{ f: <T extends { a: unknown }>(t: T) => T[\"a\"] | X[\"a\"] }",
    );
    let TypeExpr::Function(signature) = property(&generic, "f") else {
        panic!("`f` of `{}` is a function", display(&generic));
    };
    let returned = signature
        .return_type
        .as_deref()
        .expect("`f` declares its return");
    assert_eq!(
        members(returned),
        set(["1", "T['a']"]),
        "the return of `{}`",
        display(&generic)
    );
}

/// Measured on TypeScript 7.0.2: `{ k: keyof P }` is `{ k: keyof P; }`, `{
/// k: keyof P | 3 }` is `{ k: 3 | keyof P; }` and `Array<keyof P>` is
/// `(keyof P)[]`: inside an object or a type argument the checker prints a
/// declaration's key union by its `keyof` origin.
#[test]
fn a_keyof_inside_an_object_or_type_argument_keeps_its_printed_origin() {
    let source = "type P = { a: 1; b: 2 };\n";
    assert_eq!(display(&answer(source, "{ k: keyof P }")), "{ k: keyof P }");
    assert_eq!(
        members(property(&answer(source, "{ k: keyof P | 3 }"), "k")),
        set(["keyof P", "3"])
    );
    assert_eq!(display(&answer(source, "Array<keyof P>")), "Array<keyof P>");
}

/// The demand-driven altitudes publication reads (component-meta publishes
/// at `Navigate`) keep a nested operator the carrier it is: only a demand
/// for the whole answer reduces it.
#[test]
fn a_navigate_resolution_keeps_a_nested_access_as_its_carrier() {
    let navigated = answer_at(
        "type X = { a: 1 };\n",
        "{ x: X[\"a\"] }",
        Some(ProjectionMode::Navigate),
    );
    assert!(
        matches!(property(&navigated, "x"), TypeExpr::IndexedAccess { .. }),
        "a Navigate answer keeps `x` an indexed access, got `{}`",
        display(&navigated)
    );
}

/// Measured on TypeScript 7.0.2: `[X[Y["k"]], Y["k"]]` over `type X = { a:
/// 1; b: 2 }` and `type Y = { k: "a" }` is `[1, "a"]` and its reverse is
/// `["a", 1]`; `[keyof P] | keyof P | 3` and `keyof P | 3 | [keyof P]` are
/// both `"a" | "b" | 3 | [keyof P]`. One operator written at two places
/// answers by each place, whichever the projection reaches first.
#[test]
fn an_operator_written_at_two_places_answers_by_each_place() {
    let accesses = "type X = { a: 1; b: 2 };\ntype Y = { k: \"a\" };\n";
    let keys = "type P = { a: 1; b: 2 };\n";
    for (strict_null_checks, no_implicit_any) in STRICTNESS {
        let settings =
            format!("strictNullChecks={strict_null_checks}, noImplicitAny={no_implicit_any}");
        let answer = |source: &str, probe: &str| {
            answer_with_options(source, probe, None, strict_null_checks, no_implicit_any)
        };
        assert_eq!(
            display(&answer(accesses, "[X[Y[\"k\"]], Y[\"k\"]]")),
            "[1, 'a']",
            "{settings}"
        );
        assert_eq!(
            display(&answer(accesses, "[Y[\"k\"], X[Y[\"k\"]]]")),
            "['a', 1]",
            "{settings}"
        );
        for probe in ["[keyof P] | keyof P | 3", "keyof P | 3 | [keyof P]"] {
            assert_eq!(
                members(&answer(keys, probe)),
                set(["'a'", "'b'", "3", "[keyof P]"]),
                "`{probe}`, {settings}"
            );
        }
    }
}

/// Measured on TypeScript 7.0.2, under every `strictNullChecks` /
/// `noImplicitAny` combination: `X["a"] & number` and `X["a"] & {}` over
/// `type X = { a: 1 }` are `1`, `keyof P & string` over `type P = { a: 1;
/// b: 2 }` is `"a" | "b"`, `(keyof P & string) | 3` is `"a" | "b" | 3`,
/// and `Z["a"] & { d: 2 }` over `type Z = { a: { c: 1 } }` is `{ c: 1; }
/// & { d: 2; }`.
#[test]
fn a_closed_operator_in_an_answer_intersection_reduces() {
    let accesses = "type X = { a: 1 };\ntype Z = { a: { c: 1 } };\n";
    let keys = "type P = { a: 1; b: 2 };\n";
    for (strict_null_checks, no_implicit_any) in STRICTNESS {
        let settings =
            format!("strictNullChecks={strict_null_checks}, noImplicitAny={no_implicit_any}");
        let answer = |source: &str, probe: &str| {
            answer_with_options(source, probe, None, strict_null_checks, no_implicit_any)
        };
        assert_eq!(
            display(&answer(accesses, "X[\"a\"] & number")),
            "1",
            "{settings}"
        );
        assert_eq!(
            display(&answer(accesses, "X[\"a\"] & {}")),
            "1",
            "{settings}"
        );
        assert_eq!(
            display(&answer(accesses, "Z[\"a\"] & { d: 2 }")),
            "{ c: 1 } & { d: 2 }",
            "{settings}"
        );
        assert_eq!(
            members(&answer(keys, "keyof P & string")),
            set(["'a'", "'b'"]),
            "{settings}"
        );
        assert_eq!(
            members(&answer(keys, "(keyof P & string) | 3")),
            set(["'a'", "'b'", "3"]),
            "{settings}"
        );
    }
}

/// Measured on TypeScript 7.0.2, with strict null checks on and off: over
/// `type Y = { c: 1 }` and `type X = { a: { b: Y["c"] } }`, `{ x: X["a"] }`
/// is `{ x: { b: 1; }; }`, `X["a"] | 3` is `{ b: 1; } | 3` and `X["a"]` is
/// `{ b: 1; }`; with `type X = { a: [Y["c"]] }`, `[X["a"]]` is `[[1]]`. A
/// member read through an access answers as it does written in place.
#[test]
fn a_member_read_through_an_access_reads_its_own_closed_accesses() {
    let object = "type Y = { c: 1 };\ntype X = { a: { b: Y[\"c\"] } };\n";
    let tuple = "type Y = { c: 1 };\ntype X = { a: [Y[\"c\"]] };\n";
    for (strict_null_checks, no_implicit_any) in STRICTNESS {
        let settings =
            format!("strictNullChecks={strict_null_checks}, noImplicitAny={no_implicit_any}");
        let answer = |source: &str, probe: &str| {
            answer_with_options(source, probe, None, strict_null_checks, no_implicit_any)
        };
        assert_eq!(
            display(&answer(object, "{ x: X[\"a\"] }")),
            "{ x: { b: 1 } }",
            "{settings}"
        );
        assert_eq!(
            members(&answer(object, "X[\"a\"] | 3")),
            set(["{ b: 1 }", "3"]),
            "{settings}"
        );
        assert_eq!(
            display(&answer(object, "X[\"a\"]")),
            "{ b: 1 }",
            "{settings}"
        );
        assert_eq!(display(&answer(tuple, "[X[\"a\"]]")), "[[1]]", "{settings}");
    }
}

/// Measured on TypeScript 7.0.2, with strict null checks on and off: over
/// `type P = { a: 1; b: 2 }` and `type X = { a: keyof P }`, `{ x: X["a"] }`
/// is `{ x: keyof P; }` and `X["a"] | 3` is `"a" | "b" | 3`.
#[test]
fn an_access_reading_a_keyof_answers_that_keyof() {
    let source = "type P = { a: 1; b: 2 };\ntype X = { a: keyof P };\n";
    for (strict_null_checks, no_implicit_any) in STRICTNESS {
        let settings =
            format!("strictNullChecks={strict_null_checks}, noImplicitAny={no_implicit_any}");
        let answer = |probe: &str| {
            answer_with_options(source, probe, None, strict_null_checks, no_implicit_any)
        };
        assert_eq!(
            display(&answer("{ x: X[\"a\"] }")),
            "{ x: keyof P }",
            "{settings}"
        );
        assert_eq!(
            members(&answer("X[\"a\"] | 3")),
            set(["'a'", "'b'", "3"]),
            "{settings}"
        );
    }
}

/// A member whose read reaches the same access again keeps that access
/// where it reaches it, and the answer completes.
#[test]
fn an_access_reading_itself_keeps_the_access_where_it_recurs() {
    let source = "type X = { a: { b: X[\"a\"] } };\n";
    let answer = answer(source, "{ x: X[\"a\"] }");
    let TypeExpr::Object(_) = property(&answer, "x") else {
        panic!("`x` of `{}` reads the member", display(&answer));
    };
}

/// Measured on TypeScript 7.0.2, with strict null checks on and off: over
/// `interface A { y: 1; x: this["y"] }`, `{ a: A["x"] }` is `{ a: 1; }`:
/// the member read binds `this` to `A`.
#[test]
fn an_access_to_a_member_typed_by_polymorphic_this_reads_through_it() {
    let source = "interface A { y: 1; x: this[\"y\"] }\n";
    for (strict_null_checks, no_implicit_any) in STRICTNESS {
        assert_eq!(
            display(&answer_with_options(
                source,
                "{ a: A[\"x\"] }",
                None,
                strict_null_checks,
                no_implicit_any,
            )),
            "{ a: 1 }",
            "strictNullChecks={strict_null_checks}, noImplicitAny={no_implicit_any}"
        );
    }
}
