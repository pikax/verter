//! Reading one member of a wide declaration costs work that grows no faster
//! than the declaration: an interface's property (`Big["k"]`), an `as const`
//! object's property (`(typeof o)["k"]`), the same object built with a
//! spread, and, as the whole-surface control, every key of the interface
//! (`keyof Big`).
//!
//! The read lowers the declaration's body once and projects it child by
//! child. Each child of an object (and of a spread-built object's
//! construction program) is located from the projection's cursor in a
//! bounded number of slot probes, so locating the `n` children of a body of
//! `n` members costs `n` probes, not one scan of the members before each.
//!
//! Each demand is the named demand the semantic benchmark makes (`__Probe`
//! resolved by name and projected to its wire answer), and its answer is
//! TypeScript's: the demanded member's literal, or the union of every key.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_type_engine::project_semantic_dispatch::locator_view_worklist::projection_child_slot_probes_for_tests;
use verter_type_engine::project_semantic_dispatch::raise::{
    enable_dispatch_trace_for_test, DISPATCH_TRACE,
};
use verter_type_expr::{LiteralValue, TypeExpr};

const SCENARIO: &str = "/scale/scenario.ts";

const SIZES: [usize; 3] = [250, 500, 1000];

/// One wide declaration and the demand made of it.
#[derive(Clone, Copy, Debug)]
enum Shape {
    /// `interface Big { k0: "v0"; ... }` and `Big["k<n-1>"]`.
    InterfaceMember,
    /// `const o = { k0: "v0", ... } as const` and `(typeof o)["k<n-1>"]`.
    ConstObjectMember,
    /// The same object with a leading spread, and the same read.
    SpreadConstObjectMember,
    /// The interface and `keyof Big`.
    InterfaceKeys,
}

impl Shape {
    fn source_and_probe(self, members: usize) -> (String, String) {
        let last = members - 1;
        match self {
            Self::InterfaceMember | Self::InterfaceKeys => {
                let mut source = String::from("interface Big {");
                for i in 0..members {
                    source.push_str(&format!(" k{i}: \"v{i}\";"));
                }
                source.push_str(" }\n");
                let probe = match self {
                    Self::InterfaceKeys => "keyof Big".to_string(),
                    _ => format!("Big[\"k{last}\"]"),
                };
                (source, probe)
            }
            Self::ConstObjectMember | Self::SpreadConstObjectMember => {
                let mut source = String::new();
                let spread = matches!(self, Self::SpreadConstObjectMember);
                if spread {
                    source.push_str("declare const base: { x: 1 };\n");
                }
                source.push_str("const o = {");
                if spread {
                    source.push_str(" ...base,");
                }
                for i in 0..members {
                    source.push_str(&format!(" k{i}: \"v{i}\","));
                }
                source.push_str(" } as const;\n");
                (source, format!("(typeof o)[\"k{last}\"]"))
            }
        }
    }

    /// TypeScript's answer: the last member's literal, or every key.
    fn expected(self, members: usize) -> BTreeSet<String> {
        match self {
            Self::InterfaceKeys => (0..members).map(|i| format!("k{i}")).collect(),
            _ => BTreeSet::from([format!("v{}", members - 1)]),
        }
    }
}

/// The work a demand did, by the counters a growth test compares.
type Work = [(&'static str, u64); 5];

/// Resolve `__Probe` in a fresh project holding `source`, as the
/// benchmark's probe does, and return its string literals and its work. An
/// answer that is not a string literal or a union of them fails.
fn named_probe(source: &str, probe: &str) -> (BTreeSet<String>, Work) {
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/scale", r#"{ "compilerOptions": { "strict": true } }"#)],
    );
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(SCENARIO.into()),
            input_id: SCENARIO.into(),
            source: Arc::from(format!("{source}type __Probe = {probe};\nexport {{}};\n")),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
    let nodes_before = host.retention_snapshot().semantic_nodes;
    let probes_before = projection_child_slot_probes_for_tests();
    let _trace = enable_dispatch_trace_for_test();
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
    let slot_probes = projection_child_slot_probes_for_tests() - probes_before;
    let nodes = host.retention_snapshot().semantic_nodes - nodes_before;
    let stats = host.project_type_store().semantic_graph().stats_snapshot();
    let dispatches = DISPATCH_TRACE.with(|trace| trace.borrow().len()) as u64;
    let bytes = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("the answer projects");
    let answer: TypeExpr = serde_json::from_slice(&bytes).expect("a wire answer");
    let literal = |expr: &TypeExpr| match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => value.to_string(),
        other => panic!("the answer holds `{}`, not a string literal", short(other)),
    };
    let literals = match &answer {
        TypeExpr::Union(arms) => arms.iter().map(literal).collect(),
        single => BTreeSet::from([literal(single)]),
    };
    let work = [
        ("connected work", usage.work as u64),
        ("query reads", dispatches),
        ("cold builds", stats.misses),
        ("interned nodes", nodes as u64),
        ("child slot probes", slot_probes as u64),
    ];
    (literals, work)
}

fn short(expr: &TypeExpr) -> String {
    format!("{expr:?}").chars().take(300).collect()
}

/// Between a size and a larger one each work counter may grow at most 1.5
/// times faster than the input: a quadratic term grows four times over each
/// doubling. A counter that is zero at the smaller size is compared as one.
/// The connected work, the query reads and the child slot probes must be
/// non-zero at both sizes, so a counter that stopped counting cannot pass as
/// linear.
fn assert_linear_growth(
    label: &str,
    (small, before): (usize, Work),
    (large, after): (usize, Work),
) {
    for (size, work) in [(small, &before), (large, &after)] {
        for (what, value) in [work[0], work[1], work[4]] {
            assert!(value > 0, "{label}: {what} is zero at {size}");
        }
    }
    for ((what, before), (_, after)) in before.into_iter().zip(after) {
        println!("{label}: {what} {before} at {small}, {after} at {large}");
        assert!(
            2 * after * small as u64 <= 3 * before.max(1) * large as u64,
            "{label}: {what} grew from {before} at {small} to {after} at {large}"
        );
    }
}

/// Every shape answers TypeScript's answer at each of three sizes, with
/// each work counter linear from one size to the next.
#[test]
fn reads_of_wide_declarations_grow_linearly_with_width() {
    for shape in [
        Shape::InterfaceMember,
        Shape::ConstObjectMember,
        Shape::SpreadConstObjectMember,
        Shape::InterfaceKeys,
    ] {
        let work = SIZES.map(|members| {
            let (source, probe) = shape.source_and_probe(members);
            let (answer, work) = named_probe(&source, &probe);
            assert_eq!(
                answer,
                shape.expected(members),
                "{shape:?} over {members} members"
            );
            (members, work)
        });
        let label = format!("{shape:?}");
        assert_linear_growth(&label, work[0], work[1]);
        assert_linear_growth(&label, work[1], work[2]);
    }
}
