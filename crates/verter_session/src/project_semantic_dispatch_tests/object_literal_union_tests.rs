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
    deep: { conditional: c ? { a: 1 } : { b: "x" } },
};
class Fields {
    static arr = [{ a: 1 }, { b: "x" }];
    static conditional = c ? { a: 1 } : { b: "x" };
}
function defaultArray(value = [{ a: 1 }, { b: "x" }]) { return value; }
function defaultConditional(value = c ? { a: 1 } : { b: "x" }) { return value; }
function annotatedArray(value: ({ a: number } | { b: string })[]) { return value; }
function defaultSeparate(value = separate) { return value; }
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
type StaticArray = typeof Fields.arr;
type StaticConditional = typeof Fields.conditional;
type DefaultArray = NonNullable<Parameters<typeof defaultArray>[0]>;
type DefaultConditional = NonNullable<Parameters<typeof defaultConditional>[0]>;
type AnnotatedArray = Parameters<typeof annotatedArray>[0];
type DefaultSeparate = NonNullable<Parameters<typeof defaultSeparate>[0]>;
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
                ("DeepMemberConditional", "{ a: number; } | { b: string; }"),
                ("StaticArray", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("StaticConditional", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("DefaultArray", "({ a: number; b?: undefined; } | { a?: undefined; b: string; })[]"),
                ("DefaultConditional", "{ a: number; b?: undefined; } | { a?: undefined; b: string; }"),
                ("AnnotatedArray", "({ a: number; } | { b: string; })[]"),
                ("DefaultSeparate", "({ a: number; } | { b: string; })[]"),
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
