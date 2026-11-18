//! Loop-source resolution pinned to the native Vue Vapor compiler.
use super::*;
use verter_vue_conformance::canon::{canonicalize_module, Canon};
use verter_vue_conformance::{authored_identifiers, read_text_normalized, workspace_root};

fn loop_getters(code: &str) -> Vec<Canon> {
    fn visit(tree: &Canon, getters: &mut Vec<Canon>) {
        if let Canon::Node(kind, children) = tree {
            if *kind == "CallExpression"
                && matches!(children.first(), Some(Canon::ImportBinding { imported, .. }) if imported == "createFor")
            {
                getters.push(children[1].clone());
            }
            for child in children {
                visit(child, getters);
            }
        }
    }
    // Keep all spellings exact: in particular, an authored alias must not be
    // accepted as alpha-equivalent to its generated reactive accessor.
    let module = canonicalize_module(code, &authored_identifiers(code)).expect("valid module");
    let mut getters = Vec::new();
    visit(&module.tree, &mut getters);
    getters
}

#[test]
fn vapor_loop_sources_follow_pinned_vue() {
    let root = workspace_root().join("crates/verter_vue_conformance/vapor");
    let manifest: serde_json::Value =
        serde_json::from_str(&read_text_normalized(&root.join("oracle.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], "3.6.0-rc.15");
    let rows = manifest["rows"].as_array().unwrap();
    let files: std::collections::BTreeSet<_> = std::fs::read_dir(root.join("cases"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_string())
        .filter(|name| name.ends_with(".vue"))
        .collect();
    assert_eq!(
        files,
        rows.iter()
            .map(|row| row["file"].as_str().unwrap().to_string())
            .collect(),
        "every synthetic source has an oracle receipt",
    );
    let mut differences = Vec::new();
    for (id, count) in [
        ("loop-source-setup", 1),
        ("loop-source-prop", 1),
        ("loop-source-nested", 2),
        ("loop-source-callback", 1),
        ("loop-source-shorthand", 1),
        ("loop-source-defaults", 2),
        ("loop-source-declarations", 2),
        ("loop-source-computed-pattern", 2),
    ] {
        let source = read_text_normalized(&root.join(format!("cases/{id}.vue"))).unwrap();
        let golden =
            read_text_normalized(&root.join(format!("goldens/3.6.0-rc.15/{id}.js"))).unwrap();
        let row = rows
            .iter()
            .find(|row| row["file"] == format!("{id}.vue"))
            .unwrap();
        assert_eq!(
            row["sourceSha256"],
            verter_vue_conformance::sha256_hex(&source)
        );
        assert_eq!(
            row["outputSha256"],
            verter_vue_conformance::sha256_hex(&golden)
        );
        assert!(row["diagnostics"].as_array().unwrap().is_empty());
        let case = Case {
            name: id.to_string(),
            source,
        };
        let RouteResult::Compiled(view) = run_route(
            RenderRoute::Targets,
            &case,
            Target::FineGrained,
            Mode::Separate,
        ) else {
            panic!("target compiles {id}")
        };
        assert!(view.diagnostics.is_empty(), "{id}: {:?}", view.diagnostics);
        let code = &view.module.as_ref().expect("published module").0;
        let expected = loop_getters(&golden);
        assert_eq!(expected.len(), count, "oracle loop inventory: {id}");
        if loop_getters(code) != expected {
            differences.push(format!("{id}:\n{code}\nVue:\n{golden}"));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
