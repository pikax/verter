//! An intersection written as an alias's body is the type the checker
//! constructs from it (`getTypeFromIntersectionTypeNode`): conflicting
//! discriminant literals are `never`, an intersection with a discriminated
//! union keeps the member whose discriminant matches, `{}` removes `null`
//! and `undefined`, the written `string & {}` survives in a literal union,
//! and `(A & B) & (A | B)` absorbs to `A & B`.
//!
//! Each demand is the named demand the semantic benchmark makes (`__Probe`
//! resolved by name and projected to its wire answer). Every expected answer
//! is TypeScript 7.0.2's, measured with `tsc -p` over the benchmark's
//! tsconfig and measuring suffix on the same module, with
//! `strictNullChecks` on and off.

use std::sync::Arc;

const SCENARIO: &str = "/reduce/scenario.ts";

/// The utility declarations every benchmark scenario module carries.
const PRELUDE: &str = "\
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;
type Uppercase<S extends string> = intrinsic;
type Lowercase<S extends string> = intrinsic;
type Capitalize<S extends string> = intrinsic;
type Uncapitalize<S extends string> = intrinsic;
interface A { a: 1 }
interface B { b: 1 }
";

/// The printed answer to `type __Probe = probe;` in a module of `PRELUDE`,
/// resolved by name under `compiler_options`.
fn named_answer(compiler_options: &str, probe: &str) -> String {
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[(
            "/reduce",
            &format!(r#"{{ "compilerOptions": {compiler_options} }}"#),
        )],
    );
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(SCENARIO.into()),
            input_id: SCENARIO.into(),
            source: Arc::from(format!(
                "type __BenchInit = 0;\n{PRELUDE}type __Probe = {probe};\nexport {{}};\n"
            )),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(SCENARIO, "__Probe", None)
        .into_parts();
    let node = outcome
        .expect("the named demand completes")
        .expect("the probe exists");
    let wire = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("the answer projects");
    let value: serde_json::Value = serde_json::from_slice(&wire).expect("the wire answer is JSON");
    let answer = verter_type_expr::type_expr_from_json(&value).expect("a wire TypeExpr");
    verter_type_expr::render_type_expr_display(&answer)
        .expect("the answer renders")
        .text
}

/// Every `(probe, expected)` row whose named answer under
/// `compiler_options` prints otherwise, with what it printed.
fn mismatches(compiler_options: &str, rows: &[(&str, &str)]) -> Vec<String> {
    rows.iter()
        .filter_map(|(probe, expected)| {
            let answer = named_answer(compiler_options, probe);
            (answer != *expected).then(|| {
                format!("{compiler_options} `{probe}`: expected `{expected}`, answered `{answer}`")
            })
        })
        .collect()
}

const STRICT: &str = r#"{ "strict": true }"#;
const NULLS_ERASED: &str = r#"{ "strict": true, "strictNullChecks": false }"#;

/// Rows TypeScript answers alike with `strictNullChecks` on and off.
///
/// Measured on TypeScript 7.0.2: `{ kind: "a"; x: 1 } & { kind: "b" }` is
/// `never`; `{ k: "b" } & ({ k: "a"; x: 1 } | { k: "b"; y: 2 })` is `{ k:
/// "b"; } & { k: "b"; y: 2; }` and the same arms the other way round `{ k:
/// "b"; y: 2; } & { k: "b"; }`; `(string & {}) | "a" | "b"` keeps all three
/// members, and so does `({} & string) | "a"`; `{ p: (string & {}) | "a" }`
/// keeps its property's union; `(A & B) & (A | B)` is `A & B`; `string &
/// {}` and `A & B` stay as written. The kept pair is decided on the
/// written arms: `{} & string & {}`, `(string & {}) & {}`, `string &
/// unknown & {}` and `string & {} & string` are each `string`.
const IN_EVERY_NULL_SETTING: &[(&str, &str)] = &[
    (r#"{ kind: "a"; x: 1 } & { kind: "b" }"#, "never"),
    (
        r#"{ k: "b" } & ({ k: "a"; x: 1 } | { k: "b"; y: 2 })"#,
        "{ k: 'b' } & { k: 'b'; y: 2 }",
    ),
    (
        r#"({ k: "a"; x: 1 } | { k: "b"; y: 2 }) & { k: "b" }"#,
        "{ k: 'b'; y: 2 } & { k: 'b' }",
    ),
    (r#"(string & {}) | "a" | "b""#, "string & {} | 'a' | 'b'"),
    (r#"({} & string) | "a""#, "{} & string | 'a'"),
    (r#"{ p: (string & {}) | "a" }"#, "{ p: string & {} | 'a' }"),
    ("(A & B) & (A | B)", "A & B"),
    ("string & {}", "string & {}"),
    ("A & B", "A & B"),
    ("{} & string & {}", "string"),
    ("(string & {}) & {}", "string"),
    ("string & unknown & {}", "string"),
    ("string & {} & string", "string"),
];

#[test]
fn a_written_intersection_reduces_as_the_checker_constructs_it() {
    let mut failures = mismatches(STRICT, IN_EVERY_NULL_SETTING);
    failures.extend(mismatches(NULLS_ERASED, IN_EVERY_NULL_SETTING));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `{}` removes `null` and `undefined` from the union it intersects; with
/// `strictNullChecks` off the union never held them, and `string & {}` is
/// the written pair the checker keeps.
///
/// Measured on TypeScript 7.0.2: `(string | null | undefined) & {}` is
/// `string` with `strictNullChecks` on and `string & {}` with it off.
#[test]
fn an_empty_object_removes_null_and_undefined_from_a_written_union() {
    let probe = "(string | null | undefined) & {}";
    let mut failures = mismatches(STRICT, &[(probe, "string")]);
    failures.extend(mismatches(NULLS_ERASED, &[(probe, "string & {}")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
