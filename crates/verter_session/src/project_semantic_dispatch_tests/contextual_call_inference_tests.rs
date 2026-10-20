//! Call inference the checker performs beyond one argument at a time:
//!
//! - several literal candidates of one base primitive for a type parameter
//!   constrained to it infer their union, a candidate that is itself a
//!   union of such literals included;
//! - a method of an object literal argument typed through a type
//!   parameter's constraint reads the inferences its sibling members made,
//!   and the reading of the argument that withholds the method infers
//!   nothing for the parameter it lands in;
//! - a generic function argument handed to a callee returning one
//!   non-generic signature keeps its own type parameters, which the
//!   returned signature declares;
//! - two references to one generic interface infer from their type
//!   arguments, though the interface's body never mentions them;
//! - an overloaded function argument infers from its last overload whether
//!   or not that overload is assignable, and the call answers with that
//!   inference.
//!
//! Each rule is pinned on the semantic benchmark scenario that reported it
//! (Verter against TypeScript 7.0.2 over the benchmark's own library), its
//! body and probe verbatim, and on neighbouring shapes the same rule
//! decides. Every expected answer is TypeScript 7.0.2's, measured with
//! `tsc --ignoreConfig --noEmit --noLib --noErrorTruncation` over that
//! library under each `strictNullChecks` × `noImplicitAny` setting, all
//! four alike: read through the differential lane in every setting, and
//! through the named `__Probe` demand the benchmark itself makes, in every
//! setting.

use std::sync::Arc;

use super::differential_harness_tests::{canonical_text, Matrix, Read, ALL};

/// The library both arms of the semantic benchmark read.
const BENCHMARK_LIBRARY: &str =
    include_str!("../../../../scripts/benchmark/semantic-perf/lib/bench-globals.d.ts");

/// The literal-candidate scenario, verbatim.
const LITERAL_CANDIDATES: &str = r#"
type __Flat<T> = [T] extends [infer U] ? U : never;
type __D<T> = T extends unknown ? T : never;
type __Obj<T> = { [K in keyof T]: T[K] };
type NoInfer<T> = intrinsic;
declare function f<T extends string>(vals: T[], def: NoInfer<T>): T;
declare function g<T extends string>(vals: T[], def: T): T;
const r = f(["a", "b"], "a");
const s = g(["a", "b"], "c");
"#;

/// The contextual-constraint scenario, verbatim: Redux Toolkit's
/// `createSlice` shape.
const CONSTRAINT_METHOD_SCENARIO: &str = r#"
declare function make<S, R extends { [k: string]: (s: S, a: { payload: any }) => void }>(o: { init: S; reducers: R }): R;
const r = make({ init: { n: 0 }, reducers: { inc(state, action: { payload: number }) { state.n = action.payload; } } });
"#;

/// The higher-order scenario, verbatim.
const HIGHER_ORDER_SCENARIO: &str = r#"
declare function wrap<P>(f: (p: P) => void): (p: P) => P;
const g = wrap(<T extends string>(p: { v: T }) => {});
"#;

/// The empty-generic-interface scenario, verbatim.
const INJECTION_KEY_SCENARIO: &str = r#"
interface Key<T> {}
declare function inject<T>(k: Key<T>): T;
declare const k: Key<{ a: number }>;
const u = inject(k);
"#;

/// The overloaded-function scenario, verbatim.
const OVERLOAD_SCENARIO: &str = r#"
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;
type Uppercase<S extends string> = intrinsic;
type Lowercase<S extends string> = intrinsic;
type Capitalize<S extends string> = intrinsic;
type Uncapitalize<S extends string> = intrinsic;
declare function f(x: string): 1;
declare function f(x: number): 2;
"#;

/// Overloaded functions whose last overload infers and an earlier one is
/// assignable to the `extends` type that inference instantiates.
const OVERLOADS_UNDER_INFERENCE: &str = r#"
declare function q(x: string): 2;
declare function q(x: number): 2;
declare function k(x: string): 1;
declare function k(x: string): 2;
"#;

/// A slice builder whose reducers are typed through their constraint, in
/// the shapes Redux Toolkit's `createSlice` and Pinia declare.
const CONSTRAINED_METHODS: &str = r#"
declare function createSlice<S, R extends { [k: string]: (state: S, action: { payload: number }) => void }>(o: { initialState: S; reducers: R }): { reducers: R };
const slice = createSlice({ initialState: { n: 0 }, reducers: { inc(state, action) {} } });
declare function validated<S, R extends { [k: string]: (state: S, action: { payload: number }) => void }>(o: { initialState: S; reducers: R & {} }): { reducers: R };
const checked = validated({ initialState: { n: 0 }, reducers: { inc(state, action: { payload: number }) {} } });
declare function named<S, R extends { inc: (a: S) => void }>(o: { s: S; r: R }): R;
const sibling = named({ s: 1, r: { inc(a) {} } });
"#;

/// An authored `any` member of a context-sensitive literal beside a
/// method typed by the parameter it infers.
const AUTHORED_ANY_SIBLING: &str = r#"
declare const anyv: any;
declare function f<T, U>(o: { a: T; m: (x: T) => U }): U;
const r1 = f({ a: anyv, m(x) { return x; } });
const r2 = f({ a: anyv, m: (x) => x });
declare function g<T extends (x: number) => number, U>(o: { a: T; b: T; m: (x: T) => U }): U;
const r3 = g({ a: anyv, b(x) { return x; }, m(x) { return x; } });
declare function h<T, U>(o: { a: T; b: T; m: (x: T) => U }): U;
const r4 = h({ a: anyv, b(x) { return x; }, m(x) { return x; } });
"#;

/// A higher-order call whose contextual signature's parameters name no
/// type parameter of the callee.
const HIGHER_ORDER_RETURN_ONLY: &str = r#"
declare function memo<R>(fn: (x: string) => R): () => R;
declare function idf<T>(x: T): T;
const m = memo(idf);
"#;

/// A higher-order call over a generic function argument.
const HIGHER_ORDER: &str = r#"
declare function wrap<A extends unknown[], R>(fn: (...args: A) => R): (...args: A) => R;
declare function apply<A extends unknown[], R>(fn: (...args: A) => R): R;
declare function id<T extends string>(x: { v: T }): { v: T };
const w = wrap(id);
const a = apply(id);
"#;

/// Inference through an empty generic interface.
const INJECTION_KEY: &str = r#"
interface InjectionKey<T> {}
declare const key: InjectionKey<{ a: number }>;
declare function inject<T>(key: InjectionKey<T>): T;
const v = inject(key);
"#;

/// An overloaded argument whose last overload infers and then fails.
const OVERLOADED_ARGUMENT: &str = r#"
declare function o(x: string): 1;
declare function o(x: number): 0;
declare function callS<R>(f: (x: string) => R): R;
const h = callS(o);
"#;

/// Every `(source, probe, checker print)` row, as the benchmark's named
/// `__Probe` demand answers it in a project of the benchmark library and
/// `source` under each of the four settings, whose answer does not print
/// as the checker's (member separators aside, which the wire rendering
/// omits).
fn named_mismatches(rows: &[(&str, &str, &str)]) -> Vec<String> {
    let spelling = |text: &str| canonical_text(&text.replace(';', ""));
    ALL.iter()
        .flat_map(|setting| {
            rows.iter().filter_map(move |(source, probe, checker)| {
                let answer = named_answer(setting.options, source, probe);
                (spelling(&answer) != spelling(checker)).then(|| {
                    format!(
                        "{}: `{probe}`: the checker answers `{checker}`, the named demand `{answer}`",
                        setting.label
                    )
                })
            })
        })
        .collect()
}

/// `__Probe = probe` resolved by name beside `source` in a project whose
/// `compilerOptions` are `options` with `noLib`, its wire answer rendered
/// with the checker's string quotes.
fn named_answer(options: &str, source: &str, probe: &str) -> String {
    let mut options: serde_json::Value =
        serde_json::from_str(options).expect("the setting's options are JSON");
    options["noLib"] = serde_json::Value::Bool(true);
    let tsconfig = serde_json::json!({
        "compilerOptions": options,
        "files": ["lib.bench.d.ts", "scenario.ts"],
    })
    .to_string();
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/bench", tsconfig.as_str())],
    );
    let library = "/bench/lib.bench.d.ts";
    let scenario = "/bench/scenario.ts";
    for (canonical, text, language) in [
        (
            library,
            BENCHMARK_LIBRARY.to_owned(),
            crate::LanguageRegistry::global()
                .classify_static(library)
                .static_resolution(),
        ),
        (
            scenario,
            format!("type __BenchInit = 0;\n{source}type __Probe = {probe};\nexport {{}};\n"),
            crate::FileLanguage::script_ts(),
        ),
    ] {
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(canonical.into()),
                input_id: canonical.into(),
                source: Arc::from(text),
                file_language: language,
                aliases: vec![],
            })
            .expect("the file is admitted");
    }
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(scenario, "__Probe", None)
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
        .replace('\'', "\"")
}

/// `g(["a", "b"], "c")` over `g<T extends string>(vals: T[], def: T)`
/// infers `"a" | "b" | "c"`: the array's candidate `"a" | "b"` and `"c"`
/// share the base primitive `string`, so their common supertype is their
/// union, not the first candidate the others are no subtype of.
#[test]
fn literal_candidates_of_one_base_infer_their_union() {
    let failures = Matrix::new(LITERAL_CANDIDATES)
        .lib(BENCHMARK_LIBRARY)
        .same(&[(Read::Type("typeof s"), r#""a" | "b" | "c""#)]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let failures = named_mismatches(&[(LITERAL_CANDIDATES, "typeof s", r#""a" | "b" | "c""#)]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A reducer method typed through `R`'s constraint reads `S` as
/// `initialState` inferred it, never `any`: the first-pass reading of the
/// argument holds `any` for the withheld method, which infers nothing for
/// `R`, and `R`'s constraint is read with `S` instantiated. An intersection
/// holding `R` types the method the same way, and a named member's
/// constraint reads a sibling member's inference.
#[test]
fn a_method_typed_through_a_constraint_reads_sibling_inferences() {
    let scenario = [(
        "Parameters<typeof r.inc>",
        "[state: { n: number; }, action: { payload: number; }]",
    )];
    let failures = Matrix::new(CONSTRAINT_METHOD_SCENARIO)
        .lib(BENCHMARK_LIBRARY)
        .types(&scenario);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let rows = [
        (
            "Parameters<typeof slice.reducers.inc>",
            "[state: { n: number; }, action: { payload: number; }]",
        ),
        (
            "Parameters<typeof checked.reducers.inc>",
            "[state: { n: number; }, action: { payload: number; }]",
        ),
        ("Parameters<typeof sibling.inc>", "[a: number]"),
    ];
    let failures = Matrix::new(CONSTRAINED_METHODS)
        .lib(BENCHMARK_LIBRARY)
        .types(&rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // An authored `any` member is a candidate, whether or not a withheld
    // method's placeholder lands in the same parameter; only the
    // placeholder is none.
    let failures = Matrix::new(AUTHORED_ANY_SIBLING)
        .lib(BENCHMARK_LIBRARY)
        .types(&[
            ("typeof r1", "any"),
            ("typeof r2", "any"),
            ("typeof r3", "any"),
            ("typeof r4", "any"),
        ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let named: Vec<_> = scenario
        .iter()
        .map(|(probe, checker)| (CONSTRAINT_METHOD_SCENARIO, *probe, *checker))
        .chain(
            rows.iter()
                .map(|(probe, checker)| (CONSTRAINED_METHODS, *probe, *checker)),
        )
        .collect();
    let failures = named_mismatches(&named);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `wrap(<T extends string>(p: { v: T }) => {})` over `wrap<P>(f: (p: P)
/// => void): (p: P) => P` keeps the argument's type parameter, which the
/// signature `wrap` returns declares, as `wrap(id)` keeps `id`'s; `apply(id)`, whose callee returns no signature, reads
/// `id` instantiated in its parameter's context, as does `memo(idf)`,
/// whose parameter types infer nothing from `idf`'s.
#[test]
fn a_generic_function_argument_keeps_its_type_parameters() {
    let scenario = "<T extends string>(p: { v: T; }) => { v: T; }";
    let failures = Matrix::new(HIGHER_ORDER_SCENARIO)
        .lib(BENCHMARK_LIBRARY)
        .types(&[("typeof g", scenario)]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let failures = Matrix::new(HIGHER_ORDER).lib(BENCHMARK_LIBRARY).types(&[
        ("typeof w", "<T extends string>(x: { v: T; }) => { v: T; }"),
        ("typeof a", "{ v: string; }"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let failures = Matrix::new(HIGHER_ORDER_RETURN_ONLY)
        .lib(BENCHMARK_LIBRARY)
        .types(&[("typeof m", "() => string")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let failures = named_mismatches(&[
        (HIGHER_ORDER_SCENARIO, "typeof g", scenario),
        (
            HIGHER_ORDER,
            "typeof w",
            "<T extends string>(...args: [x: { v: T }]) => { v: T }",
        ),
        (HIGHER_ORDER, "typeof a", "{ v: string; }"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `inject(k)` infers `T` from `k: Key<{ a: number }>` by the type
/// arguments of the two references to `Key`, whose empty body gives a
/// structural comparison nothing to infer from; `InjectionKey` alike.
#[test]
fn an_empty_generic_interface_infers_from_its_type_arguments() {
    let failures = Matrix::new(INJECTION_KEY_SCENARIO)
        .lib(BENCHMARK_LIBRARY)
        .types(&[("typeof u", "{ a: number; }")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let failures = Matrix::new(INJECTION_KEY)
        .lib(BENCHMARK_LIBRARY)
        .types(&[("typeof v", "{ a: number; }")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let failures = named_mismatches(&[
        (INJECTION_KEY_SCENARIO, "typeof u", "{ a: number; }"),
        (INJECTION_KEY, "typeof v", "{ a: number; }"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `callS(o)` infers `R` from `o`'s last overload `(x: number): 0`, which
/// `(x: string) => R` does not accept, and answers `0` though the call
/// then fails. A conditional type's `infer` reads the same overload but
/// keeps its relation's verdict: `typeof f` is not assignable to `(x:
/// string) => 2`, so `typeof f extends (x: string) => infer R ? R : 0`
/// answers its false branch `0`, as `typeof o`'s answers `unknown`. Over
/// `q` overloaded `(x: string): 2; (x: number): 2`, the first overload is
/// assignable to `(x: string) => 2`, so the true branch answers `2`.
#[test]
fn an_overloaded_argument_infers_from_its_last_overload() {
    let scenario = [("typeof f extends (x: string) => infer R ? R : 0", "0")];
    let failures = Matrix::new(OVERLOAD_SCENARIO)
        .lib(BENCHMARK_LIBRARY)
        .types(&scenario);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let instantiated = [
        ("typeof q extends (x: string) => infer R ? R : 0", "2"),
        ("typeof k extends (x: string) => infer R ? R : 0", "2"),
    ];
    let failures = Matrix::new(OVERLOADS_UNDER_INFERENCE)
        .lib(BENCHMARK_LIBRARY)
        .types(&instantiated);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let rows = [
        ("typeof h", "0"),
        (
            "typeof o extends (x: string) => infer R ? R : unknown",
            "unknown",
        ),
    ];
    let failures = Matrix::new(OVERLOADED_ARGUMENT)
        .lib(BENCHMARK_LIBRARY)
        .types(&rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let named: Vec<_> = scenario
        .iter()
        .map(|(probe, checker)| (OVERLOAD_SCENARIO, *probe, *checker))
        .chain(
            rows.iter()
                .map(|(probe, checker)| (OVERLOADED_ARGUMENT, *probe, *checker)),
        )
        .collect();
    let failures = named_mismatches(&named);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
