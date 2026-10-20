//! Declaration inference normalizes fresh object literals against their siblings.

use super::*;
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
use crate::{FileLanguage, UpsertRequest};

const SOURCE: &str = r#"
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;
type Uppercase<S extends string> = intrinsic;
type Lowercase<S extends string> = intrinsic;
type Capitalize<S extends string> = intrinsic;
type Uncapitalize<S extends string> = intrinsic;
type __BenchInit = 0;
declare const c: boolean;
const arr = [{ a: 1 }, { b: "x" }];
const cond = c ? { a: 1 } : { b: "x" };
const nested = [{ o: { a: 1 } }, { o: { b: "x" } }];
interface A { a: number; }
interface B { b: string; }
declare const a: A;
declare const b: B;
const declared = [a, b];
const declaredCond = c ? a : b;
const empty = [{}, { a: 1 }];
const emptyCond = c ? {} : { a: 1 };
const nonFreshEmpty = [{} as {}, { a: 1 }];
const mixed = [a, { b: "x" }];
const asserted = [{ a: 1 } as { a: number; }, { b: "x" } as { b: string; }];
const separateA = { a: 1 };
const separateB = { b: "x" };
const separate = [separateA, separateB];
const separateCond = c ? separateA : separateB;
const spreadA = { ...separateA };
const spreadB = { ...separateB };
const separateSpread = [spreadA, spreadB];
const inlineSpread = [{ ...separateA }, { b: "x" }];
function emptyReturn(flag: boolean) { return flag ? {} : { a: 1 }; }
const holder = {
    arr: [{ a: 1 }, { b: "x" }],
    conditional: c ? { a: 1 } : { b: "x" },
    deep: {
        conditional: c ? { a: 1 } : { b: "x" },
        arr: [{ a: 1 }, { b: "x" }],
    },
};
class Fields {
    arr = [{ a: 1 }, { b: "x" }];
    static arr = [{ a: 1 }, { b: "x" }];
    static conditional = c ? { a: 1 } : { b: "x" };
}
function defaultArray(value = [{ a: 1 }, { b: "x" }]) { return value; }
function defaultConditional(value = c ? { a: 1 } : { b: "x" }) { return value; }
function annotatedArray(value: ({ a: number } | { b: string })[]) { return value; }
function defaultSeparate(value = separate) { return value; }
const arrow = (value = [{ a: 1 }, { b: "x" }]) => value;
const methods = { method(value = [{ a: 1 }, { b: "x" }]) { return value; } };
const expression = function(value = [{ a: 1 }, { b: "x" }]) { return value; };
const tuple = [c ? { a: 1 } : { b: "x" }] as const;
const constTuple = [{ a: 1 }, { b: "x" }] as const;
const spreadHolder = { ...{}, arr: [{ a: 1 }, { b: "x" }], conditional: c ? { a: 1 } : { b: "x" } };
const spreadNested = { ...{}, deep: { arr: [{ a: 1 }, { b: "x" }], conditional: c ? { a: 1 } : { b: "x" } } };
const spreadArray = [{ ...{}, arr: [{ a: 1 }, { b: "x" }] }];
const spreadOperand = { ...{ arr: [{ a: 1 }, { b: "x" }] } };
const conditionalSpread = { ...(c ? { a: 1 } : { b: "x" }) };
const nestedConditional = { deep: { conditional: c ? { a: 1 } : { b: "x" } } };
const nestedWithNull = { deep: { conditional: c ? { a: 1 } : { b: "x" }, n: null as null } };
const nestedWithNumbers = { deep: { conditional: c ? { a: 1 } : { b: "x" }, arr: [1] } };
const nestedWithObject = { deep: { conditional: c ? { a: 1 } : { b: "x" }, z: { y: 1 } } };
class ConstructorDefault { constructor(value = [{ a: 1 }, { b: "x" }]) {} }
class ConstructorConditional { constructor(value = c ? { a: 1 } : { b: "x" }) {} }
class ConstructorAnnotated { constructor(value: ({ a: number } | { b: string })[] = []) {} }
type Args<T> = T extends new (...args: infer P) => any ? P : never;
type TupleConditional = typeof tuple;
type TupleKeys = keyof (typeof tuple)[0];
type TupleMissingA = Extract<(typeof tuple)[0], { b: unknown }>["a"];
type TupleMissingB = Extract<(typeof tuple)[0], { a: unknown }>["b"];
type ConstTuple = typeof constTuple;
type SpreadArray = typeof spreadHolder.arr;
type SpreadConditional = typeof spreadHolder.conditional;
type SpreadNestedConditional = typeof spreadNested.deep.conditional;
type ArraySpreadInitializer = (typeof spreadArray)[number]["arr"];
type SpreadOperand = typeof spreadOperand.arr;
type ConditionalSpread = typeof conditionalSpread;
type NestedConditional = typeof nestedConditional.deep.conditional;
type NestedWithNull = typeof nestedWithNull.deep.conditional;
type NestedWithNumbers = typeof nestedWithNumbers.deep.conditional;
type NestedWithObject = typeof nestedWithObject.deep.conditional;
type ConstructorParameter = NonNullable<Args<typeof ConstructorDefault>[0]>;
type ConditionalConstructorParameter = NonNullable<Args<typeof ConstructorConditional>[0]>;
type AnnotatedConstructorParameter = NonNullable<Args<typeof ConstructorAnnotated>[0]>;
type NestedKeys = keyof typeof nestedConditional.deep.conditional;
function nestedReturn(flag: boolean) { return { o: { p: flag ? { a: 1 } : { a: 1, b: 2 } } }; }
type NestedReturnKeys = keyof ReturnType<typeof nestedReturn>["o"]["p"];
type __Probe = typeof arr;
type Conditional = typeof cond;
type Nested = typeof nested;
type Declared = typeof declared;
type DeclaredConditional = typeof declaredCond;
type Empty = typeof empty;
type EmptyConditional = typeof emptyCond;
type NonFreshEmptyKeys = keyof (typeof nonFreshEmpty)[number];
type Mixed = typeof mixed;
type Asserted = typeof asserted;
type Separate = typeof separate;
type SeparateConditional = typeof separateCond;
type ExplicitUnion = typeof separateA | typeof separateB;
type SeparateSpread = typeof separateSpread;
type InlineSpread = typeof inlineSpread;
type EmptyReturn = ReturnType<typeof emptyReturn>;
type MemberArray = typeof holder.arr;
type MemberConditional = typeof holder.conditional;
type DeepMemberConditional = typeof holder.deep.conditional;
type DeepMemberArray = typeof holder.deep.arr;
type Holder = typeof holder;
type InstanceArray = Fields["arr"];
type StaticIndexedArray = (typeof Fields)["arr"];
type StaticArray = typeof Fields.arr;
type StaticConditional = typeof Fields.conditional;
type DefaultArray = NonNullable<Parameters<typeof defaultArray>[0]>;
type DefaultConditional = NonNullable<Parameters<typeof defaultConditional>[0]>;
type AnnotatedArray = Parameters<typeof annotatedArray>[0];
type DefaultSeparate = NonNullable<Parameters<typeof defaultSeparate>[0]>;
type ArrowParameter = NonNullable<Parameters<typeof arrow>[0]>;
type MethodParameter = NonNullable<Parameters<typeof methods.method>[0]>;
type ExpressionParameter = NonNullable<Parameters<typeof expression>[0]>;
export {};
"#;

#[test]
fn inferred_object_literal_unions_normalize_only_fresh_members() {
    let mut failures = Vec::new();
    for strict in [false, true] {
        for no_implicit_any in [false, true] {
            let options =
                format!(r#"{{"strictNullChecks":{strict},"noImplicitAny":{no_implicit_any}}}"#);
            let project = super::checker_probe_lane_tests::ProbeProject {
                compiler_options: Some(&options),
                ..Default::default()
            };
            let host = super::checker_probe_lane_tests::probe_host(project);
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(super::checker_probe_lane_tests::PROBE_FILE.to_owned()),
                    input_id: super::checker_probe_lane_tests::PROBE_FILE.to_owned(),
                    source: Arc::from(SOURCE),
                    file_language: FileLanguage::script_ts(),
                    aliases: Vec::new(),
                })
                .expect("upsert literal union fixture");
            for (name, expected) in [
                (
                    "__Probe",
                    "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]",
                ),
                (
                    "Conditional",
                    "{ a: number; b?: undefined; } | { a?: undefined; b: string; }",
                ),
                (
                    "Nested",
                    "({ o: { a: number; b?: undefined; }; } | { o: { a?: undefined; b: string; }; })[]",
                ),
                ("Declared", "(A | B)[]"),
                ("DeclaredConditional", "A | B"),
                ("Empty", "({ a?: undefined; } | { a: number; })[]"),
                ("EmptyConditional", "{ a?: undefined; } | { a: number; }"),
                ("NonFreshEmptyKeys", "never"),
                ("Mixed", "(A | { b: string; })[]"),
                ("Asserted", "({ a: number; } | { b: string; })[]"),
                ("Separate", "({ a: number; } | { b: string; })[]"),
                ("SeparateConditional", "{ a: number; } | { b: string; }"),
                ("ExplicitUnion", "{ a: number; } | { b: string; }"),
                ("SeparateSpread", "({ a: number; } | { b: string; })[]"),
                ("InlineSpread", "({ a: number; b?: undefined; } | { b: string; })[]"),
                ("EmptyReturn", "{ a?: undefined; } | { a: number; }"),
                ("MemberArray", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("MemberConditional", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("DeepMemberConditional", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("DeepMemberArray", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("Holder", "{ arr: ({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]; conditional: { a: number; b?: undefined; } | { a?: undefined; b: string; }; deep: { conditional: { a: number; b?: undefined; } | { a?: undefined; b: string; }; arr: ({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]; }; }"),
                ("InstanceArray", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("StaticIndexedArray", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("StaticArray", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("StaticConditional", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("DefaultArray", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("DefaultConditional", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("AnnotatedArray", "({ a: number; } | { b: string; })[]"),
                ("DefaultSeparate", "({ a: number; } | { b: string; })[]"),
                ("ArrowParameter", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("MethodParameter", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("ExpressionParameter", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                // The missing properties are independent of primitive literal
                // widening inside the conditional's const assertion context.
                ("TupleKeys", "\"a\" | \"b\""),
                ("TupleMissingA", "undefined"),
                ("TupleMissingB", "undefined"),
                ("ConstTuple", "readonly [{ readonly a: 1; }, { readonly b: \"x\"; }]"),
                ("SpreadArray", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("SpreadConditional", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("SpreadNestedConditional", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("ArraySpreadInitializer", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("SpreadOperand", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("ConditionalSpread", "{ a: number; } | { b: string; }"),
                ("NestedConditional", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("NestedWithNull", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("NestedWithNumbers", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("NestedWithObject", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("ConstructorParameter", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("ConditionalConstructorParameter", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("AnnotatedConstructorParameter", "({ a: number; } | { b: string; })[]"),
                ("NestedKeys", "\"a\" | \"b\""),
                ("NestedReturnKeys", "\"a\" | \"b\""),
            ] {
                let (outcome, _) = host
                    .resolve_named_symbol_with_audit(
                        super::checker_probe_lane_tests::PROBE_FILE,
                        name,
                        Some(verter_type_engine::semantic_query::ProjectionMode::Expanded),
                    )
                    .into_parts();
                let node = outcome.expect("query succeeds").expect("alias resolves");
                let view = host.resolver_store_view_read().into_owned_view();
                let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
                let ctx = crate::resolver_core::HostResolverContext::new(&host, &view, overlay);
                let dispatch = ProjectSemanticDispatch::new(&ctx);
                let _construction = dispatch.enter_construction_scope();
                let node = dispatch
                    .normalize_node_keeping_declaration_refs_for_tests(
                        node.id(),
                        verter_type_engine::semantic_query::ProjectionReductionContext::published(
                            verter_type_engine::semantic_query::ProjectionMode::Expanded,
                        ),
                    )
                    .into_usable_node()
                    .expect("the named type materializes");
                let expected = checker_syntax::parse(expected).expect("checker print parses");
                if !checker_syntax::matches_node(&dispatch, node, &expected, 0) {
                    failures.push(format!(
                        "{name}, strictNullChecks={strict}, noImplicitAny={no_implicit_any}: {}",
                        render_node(&dispatch, node, 0)
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
