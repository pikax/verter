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
//!   type argument it inherits.
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
type Work = [(&'static str, u64); 3];

/// A named demand's wire answer and the work it took.
struct Answer {
    wire: Vec<u8>,
    answer: TypeExpr,
    work: Work,
}

/// Resolve `__Probe = probe` in a fresh project holding `source`, as the
/// benchmark's probe does, and decode its wire answer as the benchmark
/// decodes it: with no depth limit, since a deep answer (a heritage chain's
/// tuple nested a hundred levels) nests deeper than a default decoder
/// allows.
fn named_answer(source: &str, probe: &str) -> Answer {
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/expand", r#"{ "compilerOptions": { "strict": true } }"#)],
    );
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
    let ((outcome, _), usage) =
        verter_type_engine::project_semantic_dispatch::connected_demand::with_connected_work_capture_for_tests(
            || {
                host.resolve_named_symbol_with_audit(SCENARIO, "__Probe", None)
                    .into_parts()
            },
        );
    let node = outcome
        .expect("the named demand completes")
        .expect("the probe exists");
    let stats = host.project_type_store().semantic_graph().stats_snapshot();
    let wire = host
        .project_node_to_type_expr_json_bytes(node)
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
    Answer { wire, answer, work }
}

fn display(expr: &TypeExpr) -> String {
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
fn assert_linear_growth(
    label: &str,
    (small, before): (usize, Work),
    (large, after): (usize, Work),
) {
    for ((what, before), (_, after)) in before.into_iter().zip(after) {
        println!("{label}: {what} {before} at {small}, {after} at {large}");
        assert!(
            2 * after * small as u64 <= 3 * before.max(1) * large as u64,
            "{label}: {what} grew from {before} at {small} to {after} at {large}"
        );
    }
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
/// in keyof S]-?: S[K]; }` and `<S>(s: S) => { [K in keyof S as Exclude<K,
/// "x">]: S[K]; }`.
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
    ] {
        let printed = display(&named_answer("", probe).answer);
        // The checker separates members with `;` and quotes with `"`.
        let normalized = checker.replace("; }", " }").replace('"', "'");
        assert_eq!(printed, normalized, "`{probe}` stays the deferred mapping");
    }
}

/// A homomorphic mapping keeps the source's `readonly` and `?`, its own
/// `-`/`+` modifiers remove or add them, and a remap keeps the modifiers of
/// the key it renames and drops a key it maps to `never`. Measured on
/// `interface Src { readonly a: 1; b?: 2; c: 3 }`.
#[test]
fn a_concrete_mapping_applies_the_checker_modifiers_and_drops_never_keys() {
    let source = "interface Src { readonly a: 1; b?: 2; c: 3 }";
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
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
