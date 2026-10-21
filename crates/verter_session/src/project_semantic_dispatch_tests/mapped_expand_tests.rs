//! A homomorphic mapped type over a concrete source, and a remapped one
//! over a concrete key domain, resolve to the object type the checker
//! resolves them to:
//!
//! - over a class built from a stack of mixins, every layer's members;
//! - over the intersection a builder accumulates one single-key mapped type
//!   at a time, one object of every key;
//! - over an interface's keys remapped by a recursive template conditional,
//!   every key renamed;
//! - over a generic interface heritage chain, every level's member at the
//!   type argument it inherits;
//! - over a Redux Toolkit slice's case reducers, every reducer's action
//!   creator, whose action is the reducer's payload action.
//!
//! Each demand is the named demand the semantic benchmark makes (`__Probe`
//! resolved by name and projected to its wire answer, the
//! `skr-mapped-expand` evidence run's scenarios at the sizes a debug build
//! answers in seconds). Every expected answer is TypeScript 7.0.2's,
//! measured with `tsc --ignoreConfig --noEmit --strict --noErrorTruncation`
//! on the same program: the object's properties are compared by name, as
//! the benchmark's canonical comparison compares them.
//!
//! A mapped type over a type parameter stays the deferred mapped type the
//! checker prints, and the source's `readonly` and `?` modifiers, the
//! mapping's own `+`/`-` modifiers and a remap that drops keys apply as the
//! checker applies them.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::checker_probe_lane_tests::mismatches;
use verter_type_expr::{ObjectMember, TypeExpr};

const SCENARIO: &str = "/expand/scenario.ts";

/// The work a demand did, by the counters a growth proof compares.
type Work<const N: usize = 3> = [(&'static str, u64); N];

/// A named demand's wire answer and the work it took.
pub(super) struct Answer {
    wire: Vec<u8>,
    pub(super) answer: TypeExpr,
    work: Work,
    flow_obligations: u64,
    name_remaps: u64,
}

/// Resolve `__Probe = probe` in a fresh project holding `source`, as the
/// benchmark's probe does, and decode its wire answer as the benchmark
/// decodes it: with no depth limit, since a deep answer (a heritage chain's
/// tuple nested a hundred levels) nests deeper than a default decoder
/// allows.
fn named_answer(source: &str, probe: &str) -> Answer {
    named_answer_in_mode(source, probe, None)
}

fn named_answer_in_mode(
    source: &str,
    probe: &str,
    mode: Option<verter_type_engine::semantic_query::ProjectionMode>,
) -> Answer {
    named_answer_with_library(source, probe, mode, false)
}

pub(super) fn named_answer_with_library(
    source: &str,
    probe: &str,
    mode: Option<verter_type_engine::semantic_query::ProjectionMode>,
    benchmark_library: bool,
) -> Answer {
    named_answer_in_union_order(source, probe, mode, benchmark_library, false)
}

/// [`named_answer_with_library`], on a host whose store orders union
/// members by the reversed stable key when `reverse_union_order`: the
/// order-only counterfactual, in a cache namespace of its own.
fn named_answer_in_union_order(
    source: &str,
    probe: &str,
    mode: Option<verter_type_engine::semantic_query::ProjectionMode>,
    benchmark_library: bool,
    reverse_union_order: bool,
) -> Answer {
    let config = if benchmark_library {
        r#"{ "compilerOptions": { "strict": true, "noLib": true }, "files": ["lib.bench.d.ts", "scenario.ts"] }"#
    } else {
        r#"{ "compilerOptions": { "strict": true } }"#
    };
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/expand", config)],
    );
    if reverse_union_order {
        host.project_type_store()
            .semantic_graph()
            .reverse_union_order_for_tests();
    }
    if benchmark_library {
        let library = "/expand/lib.bench.d.ts";
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(library.into()),
                input_id: library.into(),
                source: Arc::from(include_str!(
                    "../../../../scripts/benchmark/semantic-perf/lib/bench-globals.d.ts"
                )),
                file_language: crate::LanguageRegistry::global()
                    .classify_static(library)
                    .static_resolution(),
                aliases: vec![],
            })
            .expect("benchmark library admitted");
    }
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(SCENARIO.into()),
            input_id: SCENARIO.into(),
            source: Arc::from(format!(
                "type __BenchInit = 0;\n{source}type __Probe = {probe};\nexport {{}};\n"
            )),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
    let obligations_before =
        verter_type_engine::project_semantic_dispatch::flow_solve::planned_obligations_for_tests();
    let remaps_before = verter_type_engine::project_semantic_dispatch::build::mapped_name_remap_evaluations_for_tests();
    let ((outcome, _), usage) =
        verter_type_engine::project_semantic_dispatch::connected_demand::with_connected_work_capture_for_tests(
            || {
                host.resolve_named_symbol_with_audit(SCENARIO, "__Probe", mode)
                    .into_parts()
            },
        );
    let node = outcome
        .expect("the named demand completes")
        .expect("the probe exists");
    let stats = host.project_type_store().semantic_graph().stats_snapshot();
    let wire = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("the answer projects");
    let mut decoder = serde_json::Deserializer::from_slice(&wire);
    decoder.disable_recursion_limit();
    let value = <serde_json::Value as serde::Deserialize>::deserialize(&mut decoder)
        .expect("the wire answer is JSON");
    let answer = verter_type_expr::type_expr_from_json(&value).expect("a wire TypeExpr");
    let work = [
        ("connected work", usage.work as u64),
        ("cold builds", stats.misses),
        ("substitutions", stats.substitute_memo_misses),
    ];
    let flow_obligations =
        verter_type_engine::project_semantic_dispatch::flow_solve::planned_obligations_for_tests()
            - obligations_before;
    let name_remaps = verter_type_engine::project_semantic_dispatch::build::mapped_name_remap_evaluations_for_tests() - remaps_before;
    Answer {
        wire,
        answer,
        work,
        flow_obligations,
        name_remaps,
    }
}

pub(super) fn display(expr: &TypeExpr) -> String {
    verter_type_expr::render_type_expr_display(expr)
        .expect("the answer renders")
        .text
}

/// One property of an answer: its value's print and its modifiers.
#[derive(Debug, PartialEq, Eq)]
struct Property {
    value: String,
    optional: bool,
    readonly: bool,
}

fn property(value: impl Into<String>) -> Property {
    Property {
        value: value.into(),
        optional: false,
        readonly: false,
    }
}

/// The answer's properties by name, failing on an answer that is not an
/// object of named properties alone (an unexpanded mapped type, an
/// intersection, an index signature).
fn properties(answer: &TypeExpr) -> BTreeMap<String, Property> {
    let TypeExpr::Object(object) = answer else {
        panic!("`{}` is not an object type", short(answer));
    };
    object
        .properties
        .iter()
        .map(|member| match member {
            ObjectMember::Property(property) => {
                let verter_type_expr::AuthoredPropertyKey::String(name) = &property.key else {
                    panic!("a property keyed by `{:?}`", property.key);
                };
                (
                    name.to_string(),
                    Property {
                        value: display(&property.ty),
                        optional: property.optional,
                        readonly: property.readonly,
                    },
                )
            }
            other => panic!("the answer holds `{other:?}`, not a property"),
        })
        .collect()
}

fn short(expr: &TypeExpr) -> String {
    display(expr).chars().take(300).collect()
}

fn assert_properties(label: &str, answer: &TypeExpr, expected: BTreeMap<String, Property>) {
    let got = properties(answer);
    let missing: Vec<_> = expected
        .iter()
        .filter(|(name, value)| got.get(*name) != Some(*value))
        .take(5)
        .collect();
    let extra: Vec<_> = got
        .iter()
        .filter(|(name, _)| !expected.contains_key(*name))
        .take(5)
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty() && got.len() == expected.len(),
        "{label}: {} properties; differ from the checker's {missing:?}, unexpected {extra:?}",
        got.len()
    );
}

/// Between a size and a larger one each work counter may grow at most 1.5
/// times faster than the input: a quadratic term grows with the square of
/// the step.
fn assert_linear_growth<const N: usize>(
    label: &str,
    (small, before): (usize, Work<N>),
    (large, after): (usize, Work<N>),
) {
    for ((what, before), (_, after)) in before.into_iter().zip(after) {
        println!("{label}: {what} {before} at {small}, {after} at {large}");
        assert!(
            2 * after * small as u64 <= 3 * before.max(1) * large as u64,
            "{label}: {what} grew from {before} at {small} to {after} at {large}"
        );
    }
}

/// A setup closure returns ref state and actions capturing that state. The
/// remap tests every key, excludes actions, and unwraps each retained ref.
fn setup_store(count: usize) -> String {
    let mut source = String::from(
        "interface Ref<T> { value: T }\n\
         declare function ref<T>(value: T): Ref<T>;\n\
         type State<S> = { [K in keyof S as S[K] extends Ref<any> ? K : never]: S[K] extends Ref<infer V> ? V : never };\n\
         const setup = () => {\n",
    );
    for i in 0..count {
        source.push_str(&format!("const n{i} = ref(0); const s{i} = ref('');\n"));
    }
    source.push_str("return {");
    for i in 0..count {
        source.push_str(&format!(
            "n{i}: ref([n{i}.value] as const), s{i}: ref([s{i}.value] as const), action{i}: () => n{i}.value,"
        ));
    }
    source.push_str("}; };\ntype Store = State<ReturnType<typeof setup>>;\n");
    source
}

#[test]
fn setup_store_remap_answers_and_grows_linearly() {
    let work = [5, 10, 20, 80].map(|count| {
        let answer =
            named_answer_with_library(&setup_store(count), "Store[keyof Store]", None, true);
        println!(
            "setup store {count}: {} {:?}",
            display(&answer.answer),
            answer.work
        );
        assert_eq!(
            display(&answer.answer),
            "readonly [number] | readonly [string]"
        );
        let work = [
            answer.work[0],
            answer.work[1],
            answer.work[2],
            ("key remaps", answer.name_remaps),
            ("flow obligations", answer.flow_obligations),
        ];
        (count, work)
    });
    assert_linear_growth("setup store", work[0], work[1]);
    assert_linear_growth("setup store", work[1], work[2]);
    assert_linear_growth("setup store", work[2], work[3]);
}

#[test]
fn setup_store_selected_state_and_action_keep_their_capture_values() {
    let source = setup_store(2);
    let failures = super::differential_harness_tests::Matrix::new(&source)
        .settings(&super::differential_harness_tests::ALL)
        .types(&[(
            "Store[keyof Store]",
            "readonly [number] | readonly [string]",
        )]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for (probe, expected) in [
        (
            "ReturnType<typeof setup>['n1']['value']",
            "readonly [number]",
        ),
        ("ReturnType<ReturnType<typeof setup>['action1']>", "number"),
    ] {
        assert_eq!(
            display(&named_answer(&source, probe).answer),
            expected,
            "{probe}"
        );
    }
    for mode in [
        verter_type_engine::semantic_query::ProjectionMode::Shallow,
        verter_type_engine::semantic_query::ProjectionMode::Navigate,
    ] {
        let answer = named_answer_with_library(&source, "Store[keyof Store]", Some(mode), true);
        assert_eq!(
            display(&answer.answer),
            "readonly [number] | readonly [string]",
            "{mode:?}"
        );
    }
}

#[test]
fn filtered_mapped_indices_preserve_modifiers_and_rename_fallbacks() {
    let source = "interface Src { readonly a: 1; b?: 2; c: 3 }\n\
                  interface Plain { readonly a: 1; b: 2; c: 3 }\n\
                  type Filter<T> = { [K in keyof T as K extends 'c' ? never : K]: readonly [T[K]] };\n\
                  type RequiredFilter<T> = { -readonly [K in keyof T as K extends 'c' ? never : K]-?: readonly [T[K]] };\n\
                  type Collide<T> = { [K in keyof T as 'a']: K };\n\
                  type Rename<T> = { [K in keyof T as K extends 'a' ? 'b' : K extends 'b' ? 'a' : K]: K };";
    let failures = mismatches(
        source,
        &[
            (
                "Filter<Src>[keyof Filter<Src>]",
                "readonly [1] | readonly [2 | undefined] | undefined",
            ),
            (
                "RequiredFilter<Src>[keyof RequiredFilter<Src>]",
                "readonly [1] | readonly [2 | undefined]",
            ),
            ("Collide<Plain>['a']", "\"a\" | \"b\" | \"c\""),
            ("Rename<Plain>['a' | 'c']", "\"b\" | \"c\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A class built by `count` mixins over `Base`, and the homomorphic mapping
/// over its keys.
fn mixin_stack(count: usize) -> String {
    let mut source = String::from(
        "type Ctor<T = {}> = new (...args: any[]) => T;\nclass Base { base = -1 as const; }\n",
    );
    let mut call = String::from("Base");
    for i in 0..count {
        source.push_str(&format!(
            "function M{i}<TBase extends Ctor>(B: TBase) {{ return class extends B {{ p{i} = {i} as const; }}; }}\n"
        ));
        call = format!("M{i}({call})");
    }
    source.push_str(&format!(
        "class C extends {call} {{}}\ndeclare const __v: {{ [K in keyof C]: C[K] }};\n"
    ));
    source
}

/// A builder that accumulates `count` single-key mapped types into an
/// intersection, then maps over it.
fn builder_chain(count: usize) -> String {
    let mut source = String::from(
        "interface Builder<T> {\n\
         \x20 add<K extends string, V extends string | number | boolean>(k: K, v: V): Builder<T & { [P in K]: V }>;\n\
         \x20 build(): { [K in keyof T]: T[K] };\n\
         }\n\
         declare function builder(): Builder<{}>;\n\
         const r = builder()",
    );
    for i in 0..count {
        source.push_str(&format!(".add(\"a{i}\", {i})"));
    }
    source.push_str(".build();\n");
    source
}

/// An interface of `count` keys remapped by a recursive template
/// conditional.
fn kebab_remap(count: usize) -> String {
    let mut source = String::from("interface Model {");
    for i in 0..count {
        source.push_str(&format!(" field_{i}_name_x: {i};"));
    }
    source.push_str(
        " }\n\
         type Kebab<S extends string> = S extends `${infer A}_${infer B}` ? `${A}-${Kebab<B>}` : S;\n\
         declare const __v: { [K in keyof Model as Kebab<K & string>]: Model[K] };\n",
    );
    source
}

/// A generic interface heritage chain `depth` levels deep, each level
/// wrapping the type argument it passes on in a tuple, and the
/// homomorphic mapping over the last level's keys.
fn heritage_chain(depth: usize) -> String {
    let mut source = String::from("interface L0<T> { p0: T }\n");
    for i in 1..depth {
        source.push_str(&format!(
            "interface L{i}<T> extends L{}<[T]> {{ p{i}: T }}\n",
            i - 1
        ));
    }
    let last = depth - 1;
    source.push_str(&format!(
        "declare const __v: {{ [K in keyof L{last}<0>]: L{last}<0>[K] }};\n"
    ));
    source
}

/// The mapping over a class built by mixins holds every layer's member and
/// the base's, as the checker reads the stack: each mixin's constructor
/// mixes its instance into the base constructor's
/// (`resolveIntersectionTypeMembers`), however the applications nest.
/// Measured: `{ base: -1; p0: 0; …; p9: 9; }` over ten mixins, and on to
/// `p39: 39` over forty.
#[test]
fn mapping_a_mixin_stack_holds_every_layer() {
    let work = [10, 40].map(|count| {
        let answer = named_answer(&mixin_stack(count), "typeof __v");
        let expected = std::iter::once(("base".to_string(), property("-1")))
            .chain((0..count).map(|i| (format!("p{i}"), property(i.to_string()))))
            .collect();
        assert_properties(&format!("{count} mixins"), &answer.answer, expected);
        (count, answer.work)
    });
    assert_linear_growth("mixins", work[0], work[1]);
}

/// The builder's accumulated intersection maps to one object of every key
/// it added. Measured: `{ a0: 0; a1: 1; …; a119: 119; }` after a hundred
/// and twenty calls.
#[test]
fn mapping_an_accumulated_intersection_holds_every_key() {
    let answer = named_answer(&builder_chain(120), "typeof r");
    let expected = (0..120)
        .map(|i| (format!("a{i}"), property(i.to_string())))
        .collect();
    assert_properties("120 calls", &answer.answer, expected);
}

/// An intersection of single-key mapped types `count` arms wide.
fn single_key_intersection(count: usize) -> String {
    let arms = (0..count)
        .map(|i| format!("{{ [P in 'a{i}']: {i} }}"))
        .collect::<Vec<_>>()
        .join(" & ");
    format!("type I = {{}} & {arms};\n")
}

/// An intersection of a hundred closed single-key mapped types has a closed
/// key domain, so the mapping over it maps every key, in work linear in the
/// arms. Measured: `{ a0: 0; …; a99: 99; }`.
#[test]
fn mapping_an_intersection_of_a_hundred_single_key_mappings_holds_every_key() {
    let answer = named_answer(&single_key_intersection(100), "{ [K in keyof I]: I[K] }");
    let expected = (0..100)
        .map(|i| (format!("a{i}"), property(i.to_string())))
        .collect();
    assert_properties("100 arms", &answer.answer, expected);
    let work = [30, 100].map(|count| {
        let answer = named_answer(&single_key_intersection(count), "{ [K in keyof I]: I[K] }");
        (count, answer.work)
    });
    assert_linear_growth("single-key arms", work[0], work[1]);
}

/// The remap renames every key through the recursive template conditional,
/// one key at a time. Measured: `{ "field-0-name-x": 0; …;
/// "field-99-name-x": 99; }` over a hundred keys.
#[test]
fn remapping_through_a_recursive_template_conditional_renames_every_key() {
    let answer = named_answer(&kebab_remap(100), "typeof __v");
    let expected = (0..100)
        .map(|i| (format!("field-{i}-name-x"), property(i.to_string())))
        .collect();
    assert_properties("100 keys", &answer.answer, expected);
    let work = [30, 100].map(|count| (count, named_answer(&kebab_remap(count), "typeof __v").work));
    assert_linear_growth("remap", work[0], work[1]);
}

/// The mapping over a heritage chain holds every level's member, each at
/// the type argument it inherits, however deep the chain: the chain is read
/// level by level, never one nested evaluation per level. The answer nests
/// deeper than a default JSON decoder allows and survives its round trip.
/// Measured: `{ p0: [[0]]; p1: [0]; p2: 0; }` three levels deep, and so on
/// to `p0` wrapped 99 times a hundred levels deep.
#[test]
fn mapping_a_heritage_chain_holds_every_level() {
    for depth in [10, 40, 100] {
        let answer = named_answer(&heritage_chain(depth), "typeof __v");
        let expected = (0..depth)
            .map(|i| {
                let wraps = depth - 1 - i;
                (
                    format!("p{i}"),
                    property(format!("{}0{}", "[".repeat(wraps), "]".repeat(wraps))),
                )
            })
            .collect();
        assert_properties(&format!("{depth} levels"), &answer.answer, expected);
        let round_trip = serde_json::to_vec(&answer.answer).expect("the answer encodes");
        assert_eq!(
            round_trip, answer.wire,
            "{depth} levels: the wire answer survives its JSON round trip"
        );
    }
}

/// A mapping over a type parameter stays the deferred mapped type the
/// checker prints, with its modifiers and its remap. Measured:
/// `<S>(s: S) => { [K in keyof S]: S[K]; }`, `<S>(s: S) => { -readonly [K
/// in keyof S]-?: S[K]; }`, `<S>(s: S) => { [K in keyof S as Exclude<K,
/// "x">]: S[K]; }` and `<S>(s: S) => { [K in keyof (S & { a: 1; })]: (S & {
/// a: 1; })[K]; }`.
#[test]
fn a_mapping_over_a_type_parameter_stays_deferred() {
    for (probe, checker) in [
        (
            "<S>(s: S) => { [K in keyof S]: S[K] }",
            "<S>(s: S) => { [K in keyof S]: S[K]; }",
        ),
        (
            "<S>(s: S) => { -readonly [K in keyof S]-?: S[K] }",
            "<S>(s: S) => { -readonly [K in keyof S]-?: S[K]; }",
        ),
        (
            "<S>(s: S) => { [K in keyof S as Exclude<K, 'x'>]: S[K] }",
            "<S>(s: S) => { [K in keyof S as Exclude<K, \"x\">]: S[K]; }",
        ),
        // `keyof (S & { a: 1 })` is `keyof S | 'a'`: the keys `S` brings
        // are unknown until it is instantiated, so the mapping stays deferred
        // over the whole intersection, never over `'a'` alone.
        (
            "<S>(s: S) => { [K in keyof (S & { a: 1 })]: (S & { a: 1 })[K] }",
            "<S>(s: S) => { [K in keyof (S & { a: 1; })]: (S & { a: 1; })[K]; }",
        ),
    ] {
        let printed = display(&named_answer("", probe).answer);
        // The checker separates members with `;` and quotes with `"`.
        let normalized = checker.replace("; }", " }").replace('"', "'");
        assert_eq!(printed, normalized, "`{probe}` stays the deferred mapping");
    }
}

/// A homomorphic mapping keeps the source's `readonly` and `?`, its own
/// `-`/`+` modifiers remove or add them, and a remap keeps the modifiers of
/// the key it renames and drops a key it maps to `never`; a remap to a union
/// that applies a helper renames every key through it and adds the union's
/// other names. Measured on `interface Src { readonly a: 1; b?: 2; c: 3 }`
/// and `interface Model { field_0_name_x: 0; field_1_y: 1; plain: 2 }`.
#[test]
fn a_concrete_mapping_applies_the_checker_modifiers_and_drops_never_keys() {
    let source = "interface Src { readonly a: 1; b?: 2; c: 3 }\n\
                  interface Model { field_0_name_x: 0; field_1_y: 1; plain: 2 }\n\
                  type Kebab<S extends string> = S extends `${infer A}_${infer B}` ? `${A}-${Kebab<B>}` : S;\n\
                  type Pre<S extends string> = `p_${S}`;";
    let failures = mismatches(
        source,
        &[
            (
                "{ [K in keyof Src]: Src[K] }",
                "{ readonly a: 1; b?: 2 | undefined; c: 3; }",
            ),
            (
                "{ -readonly [K in keyof Src]-?: Src[K] }",
                "{ a: 1; b: 2; c: 3; }",
            ),
            (
                "{ +readonly [K in keyof Src]+?: Src[K] }",
                "{ readonly a?: 1 | undefined; readonly b?: 2 | undefined; readonly c?: 3 | undefined; }",
            ),
            ("{ [K in keyof Src as never]: Src[K] }", "{}"),
            (
                "{ [K in keyof Src as K extends 'a' ? never : K]: Src[K] }",
                "{ b?: 2 | undefined; c: 3; }",
            ),
            (
                "{ [K in keyof Src as `get_${K & string}`]: Src[K] }",
                "{ readonly get_a: 1; get_b?: 2 | undefined; get_c: 3; }",
            ),
            (
                "{ [K in keyof Model as Kebab<K & string> | 'extra']: Model[K] }",
                "{ extra: 0 | 1 | 2; \"field-0-name-x\": 0; \"field-1-y\": 1; plain: 2; }",
            ),
            (
                "{ [K in keyof Model as Kebab<K & string> | Pre<K & string>]: Model[K] }",
                "{ \"field-0-name-x\": 0; p_field_0_name_x: 0; \"field-1-y\": 1; p_field_1_y: 1; plain: 2; p_plain: 2; }",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A mapping over a concrete source with index signatures maps each one to
/// an index signature, as it maps each property: the value is the mapping's
/// at the signature's key type, `readonly` follows the source's or the
/// mapping's modifier, `+?` adds `undefined` to it, and an `as` clause keys
/// it by the type the remap produces. Measured on the sources below.
#[test]
fn a_concrete_mapping_maps_the_source_index_signatures() {
    let source = "interface Dict { [k: string]: number; fixed: 1 }\n\
                  interface Maybe { [k: string]: number | undefined; b?: 2 }\n\
                  interface Frozen { readonly [k: string]: number; b?: 2 }\n\
                  interface Keyed { [k: number]: 1; [k: `t_${string}`]: 2; s: 3 }\n\
                  interface Numbered { [k: number]: 1 }\n\
                  interface Only { [k: string]: number }\n\
                  interface Mixed { readonly [k: number]: 1; [k: `t_${string}`]: 2 }\n\
                  interface Mixed2 { [k: `t_${string}`]: 2; readonly [k: number]: 1 }\n\
                  interface RoStr { readonly [k: string]: 1; [k: number]: 1 }\n\
                  class Holder { x = 1 as const }\n\
                  declare const Dynamic: new () => Dict;\n\
                  class Derived extends Dynamic {}";
    let failures = mismatches(
        source,
        &[
            (
                "{ [K in keyof Dict]: Dict[K] }",
                "{ [x: string]: number; fixed: 1; }",
            ),
            (
                "{ [K in keyof Derived]: Derived[K] }",
                "{ [x: string]: number; fixed: 1; }",
            ),
            (
                "{ [K in keyof (Holder & Numbered)]: 1 }",
                "{ [x: number]: 1; x: 1; }",
            ),
            (
                "{ [K in keyof Maybe]-?: Maybe[K] }",
                "{ [x: string]: number | undefined; b: 2; }",
            ),
            (
                "{ [K in keyof Dict]+?: Dict[K] }",
                "{ [x: string]: number | undefined; fixed?: 1 | undefined; }",
            ),
            (
                "{ [K in keyof Frozen]: K }",
                "{ readonly [x: string]: string; b?: \"b\" | undefined; }",
            ),
            (
                "{ readonly [K in keyof Dict]: Dict[K] }",
                "{ readonly [x: string]: number; readonly fixed: 1; }",
            ),
            (
                "{ [K in keyof Keyed]: Keyed[K] }",
                "{ [x: number]: 1; [x: `t_${string}`]: 2; s: 3; }",
            ),
            (
                "{ [K in keyof Keyed as K extends number ? never : K]: Keyed[K] }",
                "{ [x: `t_${string}`]: 2; s: 3; }",
            ),
            (
                "{ [K in keyof Dict as `g_${K & string}`]: Dict[K] }",
                "{ [x: `g_${string}`]: number; g_fixed: 1; }",
            ),
            // A key type remapped to a property name is a property of the
            // value at that key type, with only the mapping's modifiers.
            ("{ [K in keyof Only as 'x']: Only[K] }", "{ x: number; }"),
            ("{ [K in keyof Dict as 'x']: Dict[K] }", "{ x: number; }"),
            (
                "{ [K in keyof Only as K extends string ? 's' : 'n']: Only[K] }",
                "{ s: number; }",
            ),
            (
                "{ [K in keyof Only as 'x']+?: Only[K] }",
                "{ x?: number | undefined; }",
            ),
            (
                "{ [K in keyof Dict as 'x' | K]: Dict[K] }",
                "{ [x: string]: number; fixed: 1; x: 1; }",
            ),
            // A remapped signature keeps the `readonly` of the source
            // signature that applies to the key type it produces, in either
            // declaration order.
            (
                "{ [K in keyof Mixed as string]: Mixed[K] }",
                "{ [x: string]: 1 | 2; }",
            ),
            (
                "{ [K in keyof Mixed2 as string]: Mixed2[K] }",
                "{ [x: string]: 1 | 2; }",
            ),
            (
                "{ [K in keyof RoStr as number]: RoStr[K] }",
                "{ [x: number]: 1; }",
            ),
            (
                "{ [K in keyof Mixed as `${number}`]: Mixed[K] }",
                "{ readonly [x: `${number}`]: 1 | 2; }",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A conditional nested in another's true branch references the outer
/// clause's `infer` binder by name in its own `extends` clause, as the
/// checker prints it; a binder declared twice in one clause is declared
/// both times.
#[test]
fn a_nested_conditional_references_the_outer_infer_binder() {
    let source = "declare function j<T>(): T extends { a: infer U } ? T extends { b: U } ? 1 : 2 : never;\n\
                  declare function k<T>(): T extends { a: infer U; b: infer U } ? [U] extends [infer V] ? T extends { c: V; d: U } ? V : 0 : 1 : 2;\n";
    assert_eq!(
        display(&named_answer(source, "typeof j").answer),
        "<T>() => T extends { a: infer U } ? T extends { b: U } ? 1 : 2 : never"
    );
    assert_eq!(
        display(&named_answer(source, "typeof k").answer),
        "<T>() => T extends { a: infer U; b: infer U } ? [U] extends [infer V] ? T extends { c: V; d: U } ? V : 0 : 1 : 2"
    );
}

/// A deferred conditional over a type parameter prints each `infer` binder
/// as `infer` only where its `extends` clause declares it, and a recursive
/// reference with its type arguments: the remap the mapped type above
/// applies per key, left open, reads `${A}` and `Kebab<B>` in its true
/// branch. The checker prints the application by its alias, `<S extends
/// string>(s: S) => Kebab<S>`; the expanded body is the same type.
#[test]
fn a_deferred_recursive_conditional_names_its_binders_and_arguments() {
    let kebab = "type Kebab<S extends string> = S extends `${infer A}_${infer B}` ? `${A}-${Kebab<B>}` : S;\n";
    let printed = display(&named_answer(kebab, "<S extends string>(s: S) => Kebab<S>").answer);
    assert_eq!(
        printed,
        "<S extends string>(s: S) => S extends `${infer A}_${infer B}` ? `${A}-${Kebab<B>}` : S"
    );
}

/// Redux Toolkit's `createSlice` declarations (2.x), trimmed to the members
/// its action creators read, and a slice of `count` case reducers whose
/// payloads alternate a literal and `number`; `arrow` writes each reducer
/// as an arrow-valued property instead of a method. Neither form annotates
/// the reducer's `state`, which the slice's own state types.
fn rtk_slice(count: usize, arrow: bool) -> String {
    let mut source = String::from(include_str!("fixtures/rtk_slice.ts"));
    source.push_str("const slice = createSlice({ name: 's', initialState: { n: 0 }, reducers: {\n");
    for i in 0..count {
        let payload = if i % 2 == 0 {
            format!("{{ v{i}: 'l{i}' }}")
        } else {
            format!("{{ v{i}: number }}")
        };
        if arrow {
            source.push_str(&format!(
                "r{i}: (state, action: PayloadAction<{payload}>) => {{}},\n"
            ));
        } else {
            source.push_str(&format!(
                "r{i}(state, action: PayloadAction<{payload}>) {{}},\n"
            ));
        }
    }
    source.push_str("} });\ntype Actions = typeof slice.actions;\n");
    source
}

/// The actions of [`rtk_slice`]'s action creators, as the checker prints
/// each member of their union: `PayloadAction<payload, "s/r{i}">` resolved.
fn rtk_actions(count: usize) -> std::collections::BTreeSet<String> {
    (0..count)
        .map(|i| {
            let payload = if i % 2 == 0 {
                format!("'l{i}'")
            } else {
                "number".to_string()
            };
            format!("{{ payload: {{ v{i}: {payload} }}; type: 's/r{i}' }}")
        })
        .collect()
}

/// The action every action creator of a Redux Toolkit slice returns is the
/// complete union of each reducer's payload action, and the work it takes
/// grows with the reducer count, not faster.
fn assert_rtk_slice_actions_grow_linearly(arrow: bool) {
    let label = if arrow {
        "arrow reducers"
    } else {
        "method reducers"
    };
    let work = [3, 6, 12].map(|count| {
        let answer = named_answer_with_library(
            &rtk_slice(count, arrow),
            "ReturnType<Actions[keyof Actions]>",
            None,
            true,
        );
        let TypeExpr::Union(members) = &answer.answer else {
            panic!(
                "{label} {count}: `{}` is not a union",
                short(&answer.answer)
            );
        };
        let got: std::collections::BTreeSet<String> = members.iter().map(display).collect();
        assert_eq!(got, rtk_actions(count), "{label} {count}");
        let work = [
            answer.work[0],
            answer.work[1],
            answer.work[2],
            ("flow obligations", answer.flow_obligations),
        ];
        (count, work)
    });
    assert_linear_growth(label, work[0], work[1]);
    assert_linear_growth(label, work[1], work[2]);
}

#[test]
fn rtk_slice_method_reducer_actions_answer_completely_and_grow_linearly() {
    assert_rtk_slice_actions_grow_linearly(false);
}

#[test]
fn rtk_slice_arrow_reducer_actions_answer_completely_and_grow_linearly() {
    assert_rtk_slice_actions_grow_linearly(true);
}

/// The action union of a ten-reducer slice differs from the checker's only
/// in its member order (semantic-difference ledger, `VerterStableV1`
/// order-induced). The checker prints the members in reducer order, `"s/r0"`
/// to `"s/r9"`; the live answer holds the same members in the stable-key
/// order. On a fresh host whose store reverses ONLY the union order, the
/// members are unchanged and their order is exactly reversed, so no member
/// depends on the order; a fresh host under the stable order answers the
/// stable order again.
#[test]
fn rtk_slice_actions_differ_from_the_checker_only_in_union_order() {
    const COUNT: usize = 10;
    let members = |reverse_union_order: bool| -> Vec<String> {
        let answer = named_answer_in_union_order(
            &rtk_slice(COUNT, false),
            "ReturnType<Actions[keyof Actions]>",
            None,
            true,
            reverse_union_order,
        );
        let TypeExpr::Union(members) = &answer.answer else {
            panic!("`{}` is not a union", short(&answer.answer));
        };
        members.iter().map(display).collect()
    };
    let checker: Vec<String> = (0..COUNT)
        .map(|i| {
            let payload = if i % 2 == 0 {
                format!("'l{i}'")
            } else {
                "number".to_string()
            };
            format!("{{ payload: {{ v{i}: {payload} }}; type: 's/r{i}' }}")
        })
        .collect();
    let stable = members(false);
    let sorted = |members: &[String]| {
        members
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(sorted(&stable), sorted(&checker), "the checker's members");
    assert_eq!(sorted(&stable), rtk_actions(COUNT), "the checker's members");
    assert_ne!(stable, checker, "the stable order is not the checker's");
    let reversed = members(true);
    let mut expected_reversed = stable.clone();
    expected_reversed.reverse();
    assert_eq!(
        reversed, expected_reversed,
        "with only the union order reversed, the same members in the reversed order"
    );
    assert_eq!(members(false), stable, "the stable order again");
}
