//! Pinned Vue render contracts for minimal route-correction cases.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use verter_vue_conformance::canon::{canonicalize_module, Canon};
use verter_vue_conformance::{
    authored_identifiers, corpus_file, corpus_root, read_text_normalized, sha256_hex,
};

use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Probe {
    id: String,
    targets: Vec<String>,
    source_sha256: String,
    vue_version: String,
    goldens: Vec<Golden>,
    checks: BTreeMap<String, Check>,
}

#[derive(Deserialize)]
struct Golden {
    mode: String,
    path: String,
    sha256: String,
    diagnostics: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Check {
    status: Status,
    reason: String,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Corrected,
    Matched,
    Excluded,
    Pending,
}

fn probes() -> Vec<Probe> {
    let root = corpus_root().with_file_name("corrections");
    serde_json::from_str(&read_text_normalized(&root.join("probes.json")).unwrap()).unwrap()
}

fn cell(name: &str) -> (Target, Mode) {
    match name {
        "client" => (Target::ClientTree, Mode::Separate),
        "client-inline" => (Target::ClientTree, Mode::Inline),
        "server" => (Target::Server, Mode::Separate),
        "fine-grained" => (Target::FineGrained, Mode::Separate),
        _ => panic!("unknown correction target: {name}"),
    }
}

#[test]
fn synthetic_correction_artifacts_are_fresh_and_fully_discovered() {
    let root = corpus_root().with_file_name("corrections");
    let cases = corpus::correction_cases();
    let probes = probes();
    assert!(!cases.is_empty());
    let listed: BTreeSet<_> = probes
        .iter()
        .map(|probe| format!("cases/{}.vue", probe.id))
        .collect();
    assert_eq!(listed.len(), probes.len(), "duplicate correction case");
    assert_eq!(listed, cases.iter().map(|case| case.name.clone()).collect());
    for probe in probes {
        let case = cases
            .iter()
            .find(|case| case.name == format!("cases/{}.vue", probe.id))
            .unwrap();
        assert_eq!(
            sha256_hex(&case.source),
            probe.source_sha256,
            "{}",
            probe.id
        );
        assert_eq!(probe.vue_version, "3.6.0-rc.15");
        assert!(!probe.goldens.is_empty());
        for golden in &probe.goldens {
            let code = read_text_normalized(&corpus_file(&root, &golden.path)).unwrap();
            assert_eq!(sha256_hex(&code), golden.sha256, "{}", golden.path);
            assert!(golden.diagnostics.is_empty(), "{}", golden.path);
        }
        assert_eq!(
            probe.targets.iter().collect::<BTreeSet<_>>(),
            probe.checks.keys().collect()
        );
        for (target, check) in &probe.checks {
            assert!(!check.reason.is_empty());
            let (target, mode) = cell(target);
            let row = expected::rows(target, mode)
                .iter()
                .find(|row| row.case == case.name);
            if check.status == Status::Corrected {
                assert!(
                    matches!(
                        row.map(|row| row.kind),
                        Some(ExpectedKind::Corrected { .. })
                    ),
                    "{}",
                    probe.id
                );
            } else {
                assert!(
                    row.is_none(),
                    "no correction row for {}: {}",
                    probe.id,
                    check.reason
                );
            }
        }
    }
}

// Parse and resolve both complete modules before selecting the complete
// render function. Helper references retain their import family and every
// render child stays exact; setup/module scaffolds are outside this check.
fn render_tree(code: &str, inline: bool, exact: &BTreeSet<String>) -> Canon {
    fn find(node: &Canon, kind: &str) -> Option<Canon> {
        let Canon::Node(name, children) = node else {
            return None;
        };
        let named_render = matches!(children.first(), Some(Canon::Leaf("ident", name)) if name == "render" || name == "ssrRender");
        if *name == kind && (kind == "ArrowFunctionExpression" || named_render) {
            return Some(node.clone());
        }
        children.iter().find_map(|child| find(child, kind))
    }
    let module = canonicalize_module(code, exact).expect("valid JavaScript module");
    find(
        &module.tree,
        if inline {
            "ArrowFunctionExpression"
        } else {
            "FunctionDeclaration"
        },
    )
    .expect("a complete render function")
}

#[test]
fn native_server_slot_children_follow_vue() {
    let cases = corpus::compared_cases();
    let root = corpus_root().with_file_name("corrections");
    let stem = "goldens/3.6.0-rc.15/server/server-slot-handler-one-statement";
    let metadata: serde_json::Value = serde_json::from_str(
        &read_text_normalized(&root.join(format!("{stem}.meta.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(metadata["vueVersion"], "3.6.0-rc.15");
    for (name, path) in [
        (
            "cases/components/slots.vue",
            corpus_root().join("goldens/3.6.0-rc.15/ssr/components/slots.js"),
        ),
        (
            "sfc_regression_fixtures/server_slot_handler_one_statement.vue",
            root.join(format!("{stem}.js")),
        ),
    ] {
        let case = cases.iter().find(|case| case.name == name).unwrap();
        let vue = read_text_normalized(&path).unwrap();
        if name.starts_with("sfc_regression_fixtures/") {
            assert_eq!(metadata["sourceSha256"], sha256_hex(&case.source));
            assert_eq!(metadata["codeSha256"], sha256_hex(&vue));
            assert_eq!(metadata["diagnostics"], serde_json::json!([]));
        }
        let RouteResult::Compiled(target) =
            run_route(RenderRoute::Targets, case, Target::Server, Mode::Separate)
        else {
            panic!("the server target compiles {name}");
        };
        let RouteResult::Compiled(legacy) = run_route(
            RenderRoute::EditInPlace,
            case,
            Target::Server,
            Mode::Separate,
        ) else {
            panic!("the legacy server compiles {name}");
        };
        assert!(target.diagnostics.is_empty(), "{name}");
        let target = &target.module.as_ref().unwrap().0;
        let legacy = &legacy.module.as_ref().unwrap().0;
        let exact = authored_identifiers(&format!("{}\n{target}\n{legacy}\n{vue}", case.source));
        assert_eq!(
            render_tree(target, false, &exact),
            render_tree(&vue, false, &exact),
            "the complete server render follows Vue: {name}"
        );
        assert_ne!(
            render_tree(legacy, false, &exact),
            render_tree(&vue, false, &exact),
            "the oracle detects the legacy slot scope defect: {name}"
        );
    }
}

/// Picks the construct a probe pins out of a canonical module.
type Select = fn(&Canon) -> bool;

/// Every subtree of the module `code` that `select` picks, in source order.
fn constructs(code: &str, exact: &BTreeSet<String>, select: Select) -> Vec<Canon> {
    fn visit(node: &Canon, select: Select, found: &mut Vec<Canon>) {
        if select(node) {
            found.push(node.clone());
        }
        if let Canon::Node(_, children) = node {
            for child in children {
                visit(child, select, found);
            }
        }
    }
    let module = canonicalize_module(code, exact).expect("valid JavaScript module");
    let mut found = Vec::new();
    visit(&module.tree, select, &mut found);
    found
}

fn model_update_listener(node: &Canon) -> bool {
    matches!(node, Canon::Node("ObjectProperty", children)
        if matches!(children.first(), Some(Canon::Leaf("str", key)) if key == "onUpdate:modelValue"))
}

fn server_slot_outlet(node: &Canon) -> bool {
    matches!(node, Canon::Node("CallExpression", children)
        if matches!(children.first(), Some(Canon::ImportBinding { imported, .. }) if imported == "ssrRenderSlot"))
}

/// Cells whose complete render still differs from Vue in an unrelated
/// construct pin the corrected construct alone, in every render leg.
#[test]
fn synthetic_constructs_follow_pinned_vue() {
    let root = corpus_root().with_file_name("corrections");
    let cases = corpus::correction_cases();
    let probes = probes();
    let pinned: [(&str, &str, Select); 2] = [
        ("merged-model-statement", "server", model_update_listener),
        ("scoped-forwarded-outlet", "server", server_slot_outlet),
    ];
    for (id, name, select) in pinned {
        let probe = probes.iter().find(|probe| probe.id == id).unwrap();
        assert!(probe.checks[name].status == Status::Excluded, "{id}");
        let case = cases
            .iter()
            .find(|case| case.name == format!("cases/{id}.vue"))
            .unwrap();
        let golden = probe
            .goldens
            .iter()
            .find(|golden| golden.mode == name)
            .unwrap();
        let vue = read_text_normalized(&corpus_file(&root, &golden.path)).unwrap();
        let (target, mode) = cell(name);
        let module = |route: RenderRoute| {
            let RouteResult::Compiled(view) = run_route(route, case, target, mode) else {
                panic!("{route:?} compiles {id}");
            };
            assert!(view.diagnostics.is_empty(), "{route:?} {id}");
            view.module.expect("a published module").0
        };
        let code = module(RenderRoute::Targets);
        let legacy = module(RenderRoute::EditInPlace);
        let exact = authored_identifiers(&format!("{}\n{code}\n{legacy}\n{vue}", case.source));
        let expected = constructs(&vue, &exact, select);
        assert!(!expected.is_empty(), "{id}: the oracle has the construct");
        assert_eq!(
            constructs(&code, &exact, select),
            expected,
            "{id} ({name}) follows {}:\n{code}",
            golden.path
        );
        assert_ne!(
            constructs(&legacy, &exact, select),
            expected,
            "the Vue oracle must detect the legacy defect in {id} ({name})"
        );
    }
}

/// Why one pinned cell does not follow Vue, or `None` when it does.
fn pinned_cell_failure(
    root: &std::path::Path,
    probe: &Probe,
    case: &Case,
    name: &str,
    check: &Check,
) -> Option<String> {
    let (target, mode) = cell(name);
    let report = run_target(target, &[mode], std::slice::from_ref(case));
    assert_eq!(report.total, 1);
    if !report.failures.is_empty()
        || !report.not_implemented.is_empty()
        || !report.pending.is_empty()
    {
        return Some(report.summary(target));
    }
    let RouteResult::Compiled(view) = run_route(RenderRoute::Targets, case, target, mode) else {
        return Some("the target does not compile".to_string());
    };
    if !view.diagnostics.is_empty() {
        return Some(format!("target diagnostics: {:?}", view.diagnostics));
    }
    let code = &view.module.as_ref().expect("published target module").0;
    let golden = probe
        .goldens
        .iter()
        .find(|golden| golden.mode == name)
        .unwrap();
    let vue = read_text_normalized(&corpus_file(root, &golden.path)).unwrap();
    // Exact names are stricter than alpha equivalence and avoid scope
    // ordinal differences between the two compilers' module scaffolds.
    let exact = authored_identifiers(&format!("{}\n{code}\n{vue}", case.source));
    if render_tree(code, mode == Mode::Inline, &exact)
        != render_tree(&vue, mode == Mode::Inline, &exact)
    {
        return Some(format!(
            "the target render differs from {}\n--- target ---\n{code}\n--- Vue ---\n{vue}",
            golden.path
        ));
    }
    if check.status == Status::Corrected {
        let RouteResult::Compiled(legacy) = run_route(RenderRoute::EditInPlace, case, target, mode)
        else {
            return Some("the legacy comparison does not compile".to_string());
        };
        let legacy_code = &legacy.module.as_ref().expect("legacy control module").0;
        let mut exact = exact.clone();
        exact.extend(authored_identifiers(legacy_code));
        if render_tree(legacy_code, mode == Mode::Inline, &exact)
            == render_tree(&vue, mode == Mode::Inline, &exact)
        {
            return Some("the Vue oracle does not detect the legacy defect".to_string());
        }
    }
    None
}

#[test]
fn synthetic_correction_renders_follow_pinned_vue() {
    let root = corpus_root().with_file_name("corrections");
    let cases = corpus::correction_cases();
    let mut checked = 0;
    let mut failures = Vec::new();
    for probe in probes() {
        let case = cases
            .iter()
            .find(|case| case.name == format!("cases/{}.vue", probe.id))
            .unwrap();
        for (name, check) in &probe.checks {
            if !matches!(check.status, Status::Corrected | Status::Matched) {
                continue;
            }
            match pinned_cell_failure(&root, &probe, case, name, check) {
                Some(failure) => failures.push(format!("{} ({name}): {failure}", case.name)),
                None => println!("PINNED_VUE {} ({name})", case.name),
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "the pinned Vue contracts executed");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
