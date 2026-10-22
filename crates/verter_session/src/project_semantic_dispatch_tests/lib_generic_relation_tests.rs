//! Relations against the library's generic interfaces, as the checker
//! decides them: two applications of one global interface (`Promise<T>`,
//! `Map<K, V>`, `Set<T>`) relate by their type arguments under its measured
//! variance, and either expands to the interface's members; `keyof` an array
//! is the keys of its apparent `Array` interface; and inference reads
//! through `Promise<infer O>`. The generic-method relations those
//! measurements make are pinned on authored interfaces of the same shape.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --noLib --noEmit --strict
//! --noErrorTruncation` over the library as a root file, under each
//! `strictNullChecks` × `noImplicitAny` setting (alike in all four):
//! `declare const s: [...Probes]; const n: never = s;` read off the TS2322
//! message.

use std::collections::BTreeSet;

use super::differential_global_library_tests::GLOBALS_LIB;
use super::differential_harness_tests::{Matrix, Setting, ALL};
use verter_type_expr::{AuthoredPropertyKey, LiteralValue, ObjectMember, PrimitiveName, TypeExpr};

/// Two applications of one library interface relate by their arguments under
/// the interface's variance, either expands to the interface's members against
/// any other target, and `keyof` an array holds the `number` its apparent
/// `Array`'s index signature contributes, so a numeric-string literal is no
/// key of it.
#[test]
fn library_interfaces_relate_with_any_arguments_and_array_keys_exclude_numeric_strings() {
    let failures = Matrix::new("").lib(GLOBALS_LIB).types(&[
        ("Promise<2> extends Promise<any> ? 1 : 0", "1"),
        ("Map<1, 2> extends Map<any, any> ? 1 : 0", "1"),
        ("\"3\" extends keyof string[] ? 1 : 0", "0"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Promise`, `Set` and `Map` are covariant in every parameter: an
/// application relates to another by its arguments.
#[test]
fn library_interface_applications_relate_by_their_variance() {
    let failures = Matrix::new("").lib(GLOBALS_LIB).types(&[
        ("Promise<2> extends Promise<number> ? 1 : 0", "1"),
        ("Promise<2> extends Promise<string> ? 1 : 0", "0"),
        ("Set<2> extends Set<number> ? 1 : 0", "1"),
        ("Set<2> extends Set<string> ? 1 : 0", "0"),
        ("Map<1, 2> extends Map<number, string> ? 1 : 0", "0"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An application expands to the library interface's members against
/// another target (`PromiseLike`, `object`); `Promise<infer U>` infers the
/// argument; and an array's keys are its `Array` interface's — `length`
/// and `number`, a readonly array's `ReadonlyArray`'s.
#[test]
fn library_interface_applications_expand_infer_and_key_as_the_checker_reads_them() {
    let failures = Matrix::new("").lib(GLOBALS_LIB).types(&[
        ("Promise<2> extends PromiseLike<any> ? 1 : 0", "1"),
        ("PromiseLike<2> extends Promise<any> ? 1 : 0", "0"),
        ("Promise<2> extends object ? 1 : 0", "1"),
        (
            "Promise<Promise<\"x\">> extends Promise<infer U> ? U : 0",
            "Promise<\"x\">",
        ),
        ("\"length\" extends keyof string[] ? 1 : 0", "1"),
        ("3 extends keyof string[] ? 1 : 0", "1"),
        ("\"3\" extends keyof readonly string[] ? 1 : 0", "0"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The library `lib.es2015.symbol.d.ts` and `lib.es2015.symbol.wellknown.d.ts`
/// add to [`GLOBALS_LIB`] (the declarations these rows read, verbatim,
/// comments dropped): `Array`'s `[Symbol.unscopables]` member maps over
/// `keyof any[]`, the very keys `keyof string[]` reads.
fn unscopables_lib() -> String {
    format!(
        "{GLOBALS_LIB}
interface Symbol {{ toString(): string; valueOf(): symbol; }}
interface SymbolConstructor {{ readonly prototype: Symbol; readonly unscopables: unique symbol; }}
declare var Symbol: SymbolConstructor;
interface Array<T> {{ readonly [Symbol.unscopables]: {{ [K in keyof any[]]?: boolean; }}; }}
interface ReadonlyArray<T> {{ readonly [Symbol.unscopables]: {{ [K in keyof readonly any[]]?: boolean; }}; }}
"
    )
}

/// `keyof` an array decides over a library whose `Array` declares a member
/// mapped over its own keys: the keys are read without evaluating member
/// values, so the `[Symbol.unscopables]` member's `keyof any[]` never reads
/// the keys being read.
#[test]
fn array_keys_decide_beside_a_member_mapped_over_them() {
    let lib = unscopables_lib();
    let failures = Matrix::new("").lib(&lib).types(&[
        ("\"3\" extends keyof string[] ? 1 : 0", "0"),
        ("\"length\" extends keyof string[] ? 1 : 0", "1"),
        ("3 extends keyof string[] ? 1 : 0", "1"),
        ("\"3\" extends keyof readonly string[] ? 1 : 0", "0"),
        (
            "typeof Symbol.unscopables extends keyof string[] ? 1 : 0",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Generic interfaces whose methods are generic in a parameter defaulted to
/// the interface's own (`then<R = T>`), as `Promise`'s and `PromiseLike`'s
/// are; methods taking callbacks; and a thenable whose callbacks return
/// `R | Thenable<R>`.
const METHODS: &str = r#"
interface MyPL<T> { then<R1 = T, R2 = never>(onfulfilled?: ((value: T) => R1 | MyPL<R1>) | undefined | null, onrejected?: ((reason: any) => R2 | MyPL<R2>) | undefined | null): MyPL<R1 | R2>; }
interface MyP<T> { then<R1 = T, R2 = never>(onfulfilled?: ((value: T) => R1 | MyPL<R1>) | undefined | null, onrejected?: ((reason: any) => R2 | MyPL<R2>) | undefined | null): MyP<R1 | R2>; catch<R = never>(onrejected?: ((reason: any) => R | MyPL<R>) | undefined | null): MyP<T | R>; }
interface Box<T> { v: T }
interface A3<T> { then<R = T>(f?: (v: T) => R): A3<R> }
interface B3<T> { then<R = T>(f?: (v: T) => R): B3<R> }
interface A4<T> { then<R = T>(f: (v: T) => R): A4<R> }
interface B4<T> { then<R = T>(f: (v: T) => R): B4<R> }
interface Z1<T> { then<R1 = T>(f?: (v: T) => R1): Z1<R1> }
interface Z4<T> { then<R1 = T, R2 = never>(f: (v: T) => R1, g: (e: any) => R2): Z4<R1 | R2> }
interface C1<T> { m(f: (v: T) => void): void }
interface C2<T> { m(f?: (v: T) => void): void }
interface C3<T> { m(f: ((v: T) => void) | null): void }
interface C4<T> { f: (g: (v: T) => void) => void }
interface K1<T> { add(cb: () => T): void }
interface K2<T> { add(cb: (x: T) => T): void }
interface K3<T> { add: (cb: () => T) => void }
"#;

/// A method generic in a parameter defaulted to its interface's relates to
/// another interface's method of the same shape by one parameter identity at
/// every level of the recursion its return makes (`A4<R>` against `B4<R>`).
#[test]
fn generic_methods_relate_through_their_defaulted_parameters() {
    let failures = Matrix::new(METHODS).lib(GLOBALS_LIB).types(&[
        ("A3<2> extends B3<any> ? 1 : 0", "1"),
        ("A4<2> extends B4<any> ? 1 : 0", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The variance of `Z4` in `settings`, read through its `then`, generic in
/// `R1 = T` and `R2 = never` as `Promise`'s is: a method parameter whose
/// default no instantiation moves (`R2`) is the target's own, never fixed to
/// its default.
fn defaulted_method_variance_in(settings: &[Setting]) -> Vec<String> {
    Matrix::new(METHODS)
        .settings(settings)
        .lib(GLOBALS_LIB)
        .types(&[
            ("Z4<2> extends Z4<number> ? 1 : 0", "1"),
            ("Z4<number> extends Z4<2> ? 1 : 0", "0"),
        ])
}

/// [`defaulted_method_variance_in`] the two settings with `noImplicitAny`;
/// each measures the variance afresh, so the other two are a test of their
/// own.
#[test]
fn a_method_generic_in_defaulted_parameters_measures_its_variance() {
    let failures = defaulted_method_variance_in(&ALL[..2]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// [`defaulted_method_variance_in`] the two settings without `noImplicitAny`.
#[test]
fn a_method_generic_in_defaulted_parameters_measures_its_variance_without_no_implicit_any() {
    let failures = defaulted_method_variance_in(&ALL[2..]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A callback parameter is related contravariantly even where a method
/// relates its parameters bivariantly, so `then(f: (v: T) => R)` and
/// `m(f: (v: T) => void)` are covariant in `T`, optional or nullable alike.
#[test]
fn callback_parameters_relate_contravariantly_in_methods() {
    let failures = Matrix::new(METHODS).lib(GLOBALS_LIB).types(&[
        ("Z1<number> extends Z1<2> ? 1 : 0", "0"),
        ("Z1<2> extends Z1<number> ? 1 : 0", "1"),
        ("C1<number> extends C1<2> ? 1 : 0", "0"),
        ("C1<2> extends C1<number> ? 1 : 0", "1"),
        ("C2<number> extends C2<2> ? 1 : 0", "0"),
        ("C3<number> extends C3<2> ? 1 : 0", "0"),
        ("C4<number> extends C4<2> ? 1 : 0", "0"),
        ("C4<2> extends C4<number> ? 1 : 0", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Where a method — or any signature without `strictFunctionTypes` —
/// relates its parameters bivariantly, a callback parameter's return
/// relates in either direction while the callback's own parameters stay
/// contravariant (the checker's `BivariantCallback`): `add(cb: () => T)`
/// is bivariant in `T`, `add(cb: (x: T) => T)` covariant, and the property
/// `add: (cb: () => T) => void` contravariant under `strictFunctionTypes`.
#[test]
fn a_method_relates_its_callbacks_returns_bivariantly() {
    let failures = Matrix::new(METHODS).lib(GLOBALS_LIB).types(&[
        ("K1<2> extends K1<number> ? 1 : 0", "1"),
        ("K1<number> extends K1<2> ? 1 : 0", "1"),
        (
            "{ m(cb: () => 2): void } extends { m(cb: () => number): void } ? 1 : 0",
            "1",
        ),
        ("K2<2> extends K2<number> ? 1 : 0", "1"),
        ("K2<number> extends K2<2> ? 1 : 0", "0"),
        ("K3<2> extends K3<number> ? 1 : 0", "0"),
        ("K3<number> extends K3<2> ? 1 : 0", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Without `strictFunctionTypes` a callback's parameters still relate
/// contravariantly — only its return turns bivariant — so `Promise` stays
/// covariant, as does a method or property taking `(v: T) => void`.
#[test]
fn callback_parameters_stay_contravariant_without_strict_function_types() {
    use super::differential_harness_tests::STRICT_FUNCTION_TYPES_OFF;
    let failures = Matrix::new(METHODS)
        .settings(&[STRICT_FUNCTION_TYPES_OFF])
        .lib(GLOBALS_LIB)
        .types(&[
            ("Promise<number> extends Promise<2> ? 1 : 0", "0"),
            ("Promise<2> extends Promise<number> ? 1 : 0", "1"),
            ("C1<number> extends C1<2> ? 1 : 0", "0"),
            ("C4<number> extends C4<2> ? 1 : 0", "0"),
            ("K2<number> extends K2<2> ? 1 : 0", "0"),
            ("K3<2> extends K3<number> ? 1 : 0", "1"),
        ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A callback returning `R | Thenable<R>` infers `R` from another
/// `Thenable<R'>` of the same declaration — by its type arguments, the
/// checker's `inferToMultipleTypes` — never the whole union.
#[test]
fn a_union_with_a_naked_parameter_infers_from_a_reference_it_matches() {
    let failures = Matrix::new(METHODS).lib(GLOBALS_LIB).types(&[
        ("MyP<2> extends MyPL<any> ? 1 : 0", "1"),
        ("MyP<2> extends MyP<number> ? 1 : 0", "1"),
        (
            "(<R>(f: (v: 2) => R | Box<R>) => R) extends ((f: (v: any) => 9 | Box<9>) => 9) ? 1 : 0",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A thenable related to another declaration's thenable infers its
/// callback's `R` through the other's structure (the checker's
/// `inferFromObjectTypes` between two references to different targets).
#[test]
fn thenables_of_different_declarations_infer_through_their_structure() {
    let failures = Matrix::new(METHODS).lib(GLOBALS_LIB).types(&[
        ("Promise<2> extends MyPL<any> ? 1 : 0", "1"),
        ("MyP<2> extends PromiseLike<any> ? 1 : 0", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The router the benchmark's `lib-trpc-router-wide-*` scenarios probe: each
/// procedure is callable and resolves to its output, read back through
/// `Promise<infer O>`.
fn router(width: usize) -> String {
    let mut source = String::from(
        "interface ProcedureDef<I, O> { _input: I; _output: O; }\n\
         interface Procedure<I, O> { _def: ProcedureDef<I, O>; (opts: { input: I }): Promise<O>; }\n\
         type Output<P> = P extends (...args: any[]) => Promise<infer O> ? O : never;\n\
         type Router = {\n",
    );
    for index in 0..width {
        source.push_str(&format!(
            "  p{index}: Procedure<{{}}, {{ tag: \"p{index}\"; v: number }}>;\n"
        ));
    }
    source.push_str(
        "};\ntype __Probe = { [K in keyof Router]: Output<Router[K]> }[keyof Router];\nexport {};\n",
    );
    source
}

/// Resolve the type alias `name` of `source` in a project whose library is
/// [`GLOBALS_LIB`], as the benchmark's probe does
/// (`resolve_named_symbol_with_audit`), and return its wire answer.
fn resolve_alias(source: &str, name: &str) -> Result<TypeExpr, String> {
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/bench", r#"{ "compilerOptions": { "strict": true } }"#)],
    );
    let canonical = "/bench/scenario.ts";
    crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::register_lib_environment(
        &host,
        canonical,
        "lib.bench.d.ts",
        GLOBALS_LIB,
    );
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        canonical,
        source,
        crate::FileLanguage::script_ts(),
    );
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(canonical, name, None)
        .into_parts();
    let node = outcome
        .map_err(|fault| format!("the probe faulted: {fault:?}"))?
        .ok_or_else(|| "the probe resolved no type".to_owned())?;
    let bytes = host
        .project_node_to_type_expr_json_bytes(&node)
        .ok_or_else(|| "the answer does not project".to_owned())?;
    Ok(serde_json::from_slice(&bytes).expect("a wire answer"))
}

/// Resolve the router's `__Probe` ([`resolve_alias`]) and return the tag of
/// each `{ tag: "pN"; v: number }` member of the answer.
fn router_output_tags(source: &str) -> Result<BTreeSet<String>, String> {
    let answer = resolve_alias(source, "__Probe")?;
    let TypeExpr::Union(arms) = &answer else {
        return Err(format!("the answer is not a union: {answer:?}"));
    };
    arms.iter()
        .map(|arm| {
            let TypeExpr::Object(object) = arm else {
                return Err(format!("a member is not an object: {arm:?}"));
            };
            let mut tag = None;
            let mut value = None;
            for member in &object.properties {
                let ObjectMember::Property(property) = member else {
                    return Err(format!("a member holds a non-property: {member:?}"));
                };
                match (&property.key, &property.ty) {
                    (
                        AuthoredPropertyKey::String(name),
                        TypeExpr::Literal(LiteralValue::String(text)),
                    ) if name.as_ref() == "tag" => {
                        tag = Some(text.to_string());
                    }
                    (AuthoredPropertyKey::String(name), ty) if name.as_ref() == "v" => {
                        value = Some(ty.clone());
                    }
                    _ => return Err(format!("a member holds another property: {property:?}")),
                }
            }
            match (tag, value) {
                (Some(tag), Some(TypeExpr::Primitive(PrimitiveName::Number))) => Ok(tag),
                other => Err(format!("a member is not `{{ tag; v: number }}`: {other:?}")),
            }
        })
        .collect()
}

/// A router twenty procedures wide resolves to the union of their outputs,
/// as the checker answers it: each procedure's call signature relates to
/// `(...args: any[]) => Promise<infer O>`, inferring its output from
/// `Promise<O>`, never faulting on an undecided relation.
#[test]
fn a_router_resolves_to_the_union_of_its_procedure_outputs() {
    let width = 20;
    let tags = router_output_tags(&router(width)).unwrap_or_else(|error| panic!("{error}"));
    let expected: BTreeSet<String> = (0..width).map(|index| format!("p{index}")).collect();
    assert_eq!(tags, expected);
}

/// `Promise<number> extends PromiseLike<infer U>` infers `U` through
/// `then`'s callback, the conditional's own inference (tsc answers
/// `number`). The relation instantiates `Promise`'s generic `then` in the
/// context of `PromiseLike<U>`'s, whose callback reads `U`: that
/// instantiation's session takes only `then`'s parameters, so it does not
/// settle, and the conditional stays undecided — never the false branch the
/// uninstantiated pair would decide.
#[test]
fn an_inference_through_a_generic_method_it_cannot_settle_is_undecided() {
    use super::differential_harness_tests::{Read, STRICT};
    let rows: Vec<(Read<'_>, Vec<&str>)> = vec![(
        Read::Type("Promise<number> extends PromiseLike<infer U> ? U : 0"),
        vec!["number"],
    )];
    let verdicts = Matrix::new("")
        .lib(GLOBALS_LIB)
        .settings(&[STRICT])
        .verdicts(&rows);
    let verdict = &verdicts[0][0];
    assert!(
        verdict.matched || verdict.class == "GAP",
        "{} {}",
        verdict.class,
        verdict.lane
    );
}

/// The same inference read as a type alias — directly, and through a
/// generic alias distributing over `Promise<string>` — answers what the
/// checker infers (`number`, `string`) or stays a deferred conditional; it
/// never takes the false branch.
#[test]
fn an_inference_through_a_generic_method_read_as_an_alias_never_takes_the_false_branch() {
    let source = "type R6 = Promise<number> extends PromiseLike<infer U> ? U : 0;\n\
                  type U1<T> = T extends PromiseLike<infer U> ? U : T;\n\
                  type R17 = U1<Promise<string>>;\n\
                  export {};\n";
    for (name, inferred) in [
        ("R6", PrimitiveName::Number),
        ("R17", PrimitiveName::String),
    ] {
        let answer = resolve_alias(source, name).unwrap_or_else(|error| panic!("{error}"));
        assert!(
            answer == TypeExpr::Primitive(inferred)
                || matches!(answer, TypeExpr::Conditional { .. }),
            "{name}: {answer:?}"
        );
    }
}
