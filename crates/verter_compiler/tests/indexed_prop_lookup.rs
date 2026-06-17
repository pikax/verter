//! Guard: prop lookups go through the dense `OxcParsedElement::prop` index, not a
//! linear scan over the sparse `props` vec by `prop_index`.
//!
//! Every consumer that needs the OXC-parsed expression for an `ElementNode.props`
//! index must resolve it via the O(1) `prop_lookup` table. A surviving
//! `p.prop_index == i` scan over `OxcParsedElement::props` is a second lookup path
//! and a perf regression on prop-heavy templates.

use std::path::{Path, PathBuf};

/// Walk `crates/verter_compiler/src` collecting production `.rs` files (test
/// modules are exempt — they may read `prop_index` directly for assertions).
fn production_rs_files() -> Vec<PathBuf> {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    collect(&src, &mut out);
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read src dir");
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect(&path, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".rs") {
            continue;
        }
        // Exempt test modules (inline `tests.rs` / `*_tests.rs`).
        if name == "tests.rs" || name.ends_with("_tests.rs") {
            continue;
        }
        out.push(path);
    }
}

/// Detect the forbidden linear-scan comparison: a `.prop_index ==` / `.prop_index !=`
/// member comparison, which only appears when scanning `props` for a matching index.
fn contains_prop_index_scan(contents: &str) -> bool {
    let mut search_from = 0;
    while let Some(rel) = contents[search_from..].find(".prop_index") {
        let after = search_from + rel + ".prop_index".len();
        let rest = contents[after..].trim_start();
        if rest.starts_with("==") || rest.starts_with("!=") {
            return true;
        }
        search_from = after;
    }
    false
}

#[test]
fn no_linear_prop_index_scan_in_production_code() {
    let mut offenders = Vec::new();
    for path in production_rs_files() {
        let contents = std::fs::read_to_string(&path).expect("read source file");
        if contains_prop_index_scan(&contents) {
            offenders.push(path.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "linear `.prop_index ==` scans must be replaced with `OxcParsedElement::prop`; \
         offenders:\n{}",
        offenders.join("\n")
    );
}

/// Discriminating self-test: the detector must FIRE on the exact pre-change scan
/// shape and must NOT fire on a dense-index access or a field initializer.
#[test]
fn detector_discriminates_scan_from_indexed_access() {
    assert!(
        contains_prop_index_scan("oxc.props.iter().find(|p| p.prop_index == prop_index)"),
        "must catch the linear scan"
    );
    assert!(
        contains_prop_index_scan("if oxc_prop.prop_index != i {"),
        "must catch the inequality form"
    );
    assert!(
        !contains_prop_index_scan("oxc_el.and_then(|e| e.prop(i))"),
        "must NOT flag the dense-index accessor"
    );
    assert!(
        !contains_prop_index_scan("OxcParsedProp { prop_index: i, arg, exp }"),
        "must NOT flag a field initializer"
    );
    assert!(
        !contains_prop_index_scan("if skip_prop_index == Some(idx) {"),
        "must NOT flag an unrelated local named *_prop_index"
    );
}
