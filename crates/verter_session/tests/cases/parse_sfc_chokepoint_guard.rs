//! B2 architecture guard: the elected store leader is the sole registered
//! carrier projector/parser boundary.

use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn elected_store_leader_is_the_sole_registered_projector_caller() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_root, &mut files);
    files.sort();

    let old_symbol = ["parse_carrier", "_counted"].concat();
    let projector = ["__project_registered_carrier", "_for_store_leader("].concat();
    let mut projector_calls = Vec::new();
    let mut raw_calls = Vec::new();
    let mut counter_sites = Vec::new();
    let mut forbidden_concat = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            !text.contains(&old_symbol),
            "retired B1 producer remains in {}",
            file.display()
        );
        for (index, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let site = || format!("{}:{}: {}", file.display(), index + 1, line.trim());
            if line.contains(&projector) {
                projector_calls.push(site());
            }
            if line.contains("compiler.parse(") {
                raw_calls.push(site());
            }
            if line.contains("carrier_parses.fetch_add(") {
                counter_sites.push(site());
            }
            if line.contains("merged_source")
                && (line.contains("compile_bundle(")
                    || line.contains("template_data(")
                    || line.contains(&projector))
            {
                forbidden_concat.push(site());
            }
        }
    }
    assert_eq!(
        projector_calls.len(),
        1,
        "sole projector calls: {projector_calls:#?}"
    );
    assert!(projector_calls[0].contains("carrier_publication_store"));
    assert!(
        raw_calls.is_empty(),
        "raw registered parses: {raw_calls:#?}"
    );
    assert_eq!(counter_sites.len(), 1, "counter sites: {counter_sites:#?}");
    assert!(counter_sites[0].contains("carrier_publication_store"));
    assert!(
        forbidden_concat.is_empty(),
        "B2 concat violations: {forbidden_concat:#?}"
    );
}
