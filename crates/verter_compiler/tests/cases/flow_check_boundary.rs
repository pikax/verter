//! Boundary of the flow-transparent callback check.
//!
//! 1. The generator and its typed seam compile in THIS crate, which links
//!    `verter_compiler` as an external dependency: their only compiler input
//!    is the public `CodeTransform`, supplied through a local `deps` module.
//!    A generator that reached for any compiler-internal module (a parser,
//!    scope, resolver or projection type) would not resolve here, so this
//!    integration binary would fail to build. The standalone copy must also
//!    produce exactly the output of the in-crate one.
//! 2. No production dependency edge can activate the `test-support` feature
//!    that compiles the check into `verter_compiler`: it is not default-on,
//!    no normal or build dependency requests it, and no workspace feature
//!    forwards it. Only dev-dependency (test) edges may.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

mod deps {
    pub(super) use oxc_allocator::Allocator;
    pub(super) use verter_compiler::code_transform::CodeTransform;
}

#[allow(dead_code)]
#[path = "../../src/ide/template/flow_check/seam.rs"]
mod seam;

#[allow(dead_code)]
#[path = "../../src/ide/template/flow_check/generator.rs"]
mod generator;

use verter_compiler::flow_check as in_crate;

const SOURCE: &str =
    "<template><p v-if=\"a\" @click=\"() => take(a)\"></p><p v-else></p></template>";

fn standalone_plan() -> seam::CheckPlan {
    use seam::*;
    let at = |needle: &str| SOURCE.find(needle).unwrap() as u32;
    let condition = at("a\"");
    let function = at("() => take(a)");
    let body = function + "() => ".len() as u32;
    let read = at("take(a)") + "take(".len() as u32;
    let expr = |start: u32, end: u32| ResolvedExpr {
        span: Span::new(start, end),
        pieces: vec![
            Piece::Synthetic("__props.".into()),
            Piece::Authored(Span::new(start, end)),
        ],
    };
    CheckPlan {
        declarations:
            "declare const __props: { a?: string };\ndeclare function take(value: string): void;\n"
                .into(),
        parameters: "()".into(),
        root: ScopeKey(0),
        items: vec![Item::Chain(Chain {
            branches: vec![
                Branch {
                    key: BranchKey(1),
                    kind: BranchKind::If,
                    predecessor: None,
                    condition: Some(expr(condition, condition + 1)),
                    scope: ScopeKey(0),
                    items: vec![Item::Callback(Callback {
                        scope: ScopeKey(0),
                        contract: "() => void".into(),
                        function: CallbackFunction::Authored {
                            expr: ResolvedExpr {
                                span: Span::new(function, function + 13),
                                pieces: vec![
                                    Piece::Authored(Span::new(function, read)),
                                    Piece::Synthetic("__props.".into()),
                                    Piece::Authored(Span::new(read, function + 13)),
                                ],
                            },
                            guard: GuardSite::Expression { before: body },
                        },
                        outer_refs: vec![OuterRef {
                            text: "__props.a".into(),
                            occurrence: Span::new(read, read + 1),
                        }],
                    })],
                },
                Branch {
                    key: BranchKey(2),
                    kind: BranchKind::Else,
                    predecessor: Some(BranchKey(1)),
                    condition: None,
                    scope: ScopeKey(0),
                    items: Vec::new(),
                },
            ],
        })],
    }
}

/// The same plan through the in-crate seam types.
fn in_crate_plan() -> in_crate::seam::CheckPlan {
    use in_crate::seam::*;
    fn span(s: seam::Span) -> Span {
        Span::new(s.start, s.end)
    }
    fn expr(e: &seam::ResolvedExpr) -> ResolvedExpr {
        ResolvedExpr {
            span: span(e.span),
            pieces: e
                .pieces
                .iter()
                .map(|p| match p {
                    seam::Piece::Authored(s) => Piece::Authored(span(*s)),
                    seam::Piece::Synthetic(t) => Piece::Synthetic(t.clone()),
                })
                .collect(),
        }
    }
    let plan = standalone_plan();
    let seam::Item::Chain(chain) = &plan.items[0] else {
        unreachable!()
    };
    let seam::Item::Callback(callback) = &chain.branches[0].items[0] else {
        unreachable!()
    };
    let seam::CallbackFunction::Authored {
        expr: function,
        guard: seam::GuardSite::Expression { before },
    } = &callback.function
    else {
        unreachable!()
    };
    CheckPlan {
        declarations: plan.declarations.clone(),
        parameters: plan.parameters.clone(),
        root: ScopeKey(0),
        items: vec![Item::Chain(Chain {
            branches: vec![
                Branch {
                    key: BranchKey(1),
                    kind: BranchKind::If,
                    predecessor: None,
                    condition: chain.branches[0].condition.as_ref().map(expr),
                    scope: ScopeKey(0),
                    items: vec![Item::Callback(Callback {
                        scope: ScopeKey(0),
                        contract: callback.contract.clone(),
                        function: CallbackFunction::Authored {
                            expr: expr(function),
                            guard: GuardSite::Expression { before: *before },
                        },
                        outer_refs: callback
                            .outer_refs
                            .iter()
                            .map(|r| OuterRef {
                                text: r.text.clone(),
                                occurrence: span(r.occurrence),
                            })
                            .collect(),
                    })],
                },
                Branch {
                    key: BranchKey(2),
                    kind: BranchKind::Else,
                    predecessor: Some(BranchKey(1)),
                    condition: None,
                    scope: ScopeKey(0),
                    items: Vec::new(),
                },
            ],
        })],
    }
}

#[test]
fn the_generator_compiles_against_code_transform_alone_and_matches_the_crate() {
    let standalone = generator::generate(&standalone_plan(), SOURCE).expect("valid plan");
    let crate_check = in_crate::generator::generate(&in_crate_plan(), SOURCE).expect("valid plan");
    assert_eq!(standalone.code, crate_check.code);
    let pairs = |m: &[(u32, u32, u32, u32)]| m.to_vec();
    assert_eq!(
        pairs(
            &standalone
                .mappings
                .iter()
                .map(|m| (
                    m.generated.start,
                    m.generated.end,
                    m.source.start,
                    m.source.end
                ))
                .collect::<Vec<_>>()
        ),
        pairs(
            &crate_check
                .mappings
                .iter()
                .map(|m| (
                    m.generated.start,
                    m.generated.end,
                    m.source.start,
                    m.source.end
                ))
                .collect::<Vec<_>>()
        )
    );
    // The condition is emitted once and the callback re-narrows `__props.a`.
    assert_eq!(standalone.code.matches("if (__props.a)").count(), 1);
    assert!(standalone.code.contains("const __verter_o0 = __props.a;"));
}

// ── Feature activation ───────────────────────────────────────────

const COMPILER: &str = "verter_compiler";
const FEATURE: &str = "test-support";

/// Every way a production build could activate `verter_compiler/test-support`.
fn production_activations(metadata: &serde_json::Value) -> Vec<String> {
    let mut found = Vec::new();
    let members: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("workspace members")
        .iter()
        .filter_map(|m| m.as_str())
        .collect();
    for package in metadata["packages"].as_array().expect("packages") {
        let id = package["id"].as_str().unwrap_or_default();
        if !members.contains(id) {
            continue;
        }
        let name = package["name"].as_str().unwrap_or_default();
        let features: BTreeMap<String, Vec<String>> =
            serde_json::from_value(package["features"].clone()).unwrap_or_default();
        if name == COMPILER {
            let mut closure = BTreeSet::new();
            let mut stack = vec!["default".to_string()];
            while let Some(feature) = stack.pop() {
                if closure.insert(feature.clone()) {
                    stack.extend(features.get(&feature).cloned().unwrap_or_default());
                }
            }
            if closure.contains(FEATURE) {
                found.push(format!("{COMPILER}: `default` enables `{FEATURE}`"));
            }
        }
        for (feature, enables) in &features {
            for token in enables {
                let forwarded = token
                    .strip_prefix(COMPILER)
                    .and_then(|rest| rest.strip_prefix('/').or_else(|| rest.strip_prefix("?/")));
                if forwarded == Some(FEATURE) {
                    found.push(format!("{name}: feature `{feature}` forwards `{token}`"));
                }
            }
        }
        for dependency in package["dependencies"].as_array().into_iter().flatten() {
            let production = matches!(dependency["kind"].as_str(), None | Some("build"));
            if production
                && dependency["name"].as_str() == Some(COMPILER)
                && dependency["features"]
                    .as_array()
                    .is_some_and(|f| f.iter().any(|f| f.as_str() == Some(FEATURE)))
            {
                found.push(format!("{name}: production dependency enables `{FEATURE}`"));
            }
        }
    }
    found
}

fn workspace_metadata() -> serde_json::Value {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .expect("cargo metadata must spawn");
    assert!(
        output.status.success(),
        "cargo metadata failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata emits JSON")
}

#[test]
fn no_production_edge_activates_the_check() {
    let metadata = workspace_metadata();
    let compiler_has_feature = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == COMPILER && p["features"].get(FEATURE).is_some());
    assert!(compiler_has_feature, "`{COMPILER}` declares `{FEATURE}`");
    let dev_edges = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["dependencies"].as_array().into_iter().flatten())
        .filter(|d| d["name"] == COMPILER && d["kind"] == "dev")
        .count();
    assert!(
        dev_edges > 0,
        "test builds reach the check through dev edges"
    );
    let activations = production_activations(&metadata);
    assert!(activations.is_empty(), "{activations:#?}");
}

#[test]
fn the_activation_check_rejects_each_production_route() {
    let metadata = |features: serde_json::Value, dependency_kind: serde_json::Value| {
        serde_json::json!({
            "workspace_members": ["c", "l"],
            "packages": [
                { "id": "c", "name": COMPILER, "features": features, "dependencies": [] },
                { "id": "l", "name": "consumer", "features": {}, "dependencies": [
                    { "name": COMPILER, "kind": dependency_kind, "features": [FEATURE] }
                ]}
            ]
        })
    };
    let clean = metadata(serde_json::json!({ FEATURE: [] }), "dev".into());
    assert!(production_activations(&clean).is_empty());
    let default_on = metadata(
        serde_json::json!({ "default": ["extra"], "extra": [FEATURE], FEATURE: [] }),
        "dev".into(),
    );
    assert_eq!(production_activations(&default_on).len(), 1);
    let normal_edge = metadata(serde_json::json!({ FEATURE: [] }), serde_json::Value::Null);
    assert_eq!(production_activations(&normal_edge).len(), 1);
    let build_edge = metadata(serde_json::json!({ FEATURE: [] }), "build".into());
    assert_eq!(production_activations(&build_edge).len(), 1);
    let mut forwarded = clean.clone();
    forwarded["packages"][1]["features"] =
        serde_json::json!({ "extra": [format!("{COMPILER}/{FEATURE}")] });
    assert_eq!(production_activations(&forwarded).len(), 1);
}
