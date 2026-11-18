//! Contract of the discovery that renders each crate's `tests/cases/mod.rs`.

#[allow(dead_code)]
mod discovery {
    include!("../../../../tools/rust/test_case_discovery.rs");
}

use discovery::{render_cases_module, DiscoveryError, DiscoveryRules};
use std::fs;
use std::path::PathBuf;

/// A scratch `tests/cases` tree, removed on drop.
struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new(files: &[&str]) -> Self {
        let root = verter_test_support::unique_temp_dir("case_discovery");
        fs::create_dir_all(&root).expect("create scratch root");
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dirs");
            fs::write(&path, "").expect("write file");
        }
        Self { root }
    }

    fn render(&self, rules: &DiscoveryRules<'_>) -> Result<String, DiscoveryError> {
        render_cases_module("scratch", &self.root, rules)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const NO_RULES: DiscoveryRules<'static> = DiscoveryRules {
    ignored_dirs: &[],
    renames: &[],
    cfg_gates: &[],
};

/// The module lines of a rendered file, without its header.
fn body(rendered: &str) -> &str {
    rendered
        .split_once("differs from the directory.\n\n")
        .expect("rendered file carries its header")
        .1
}

#[test]
fn files_and_directories_become_sorted_modules_without_support_or_data() {
    let tree = Tree::new(&[
        "zeta.rs",
        "mod.rs",
        "alpha.rs",
        ".DS_Store",
        "grouped/mod.rs",
        "plain_group/one.rs",
        "plain_group/nested/two.rs",
        "support/helper.rs",
        "compile-fail/case.rs",
        "compile-fail/case.stderr",
        "owner.rs",
        "owner/child.rs",
        "data_dir/rows.json",
    ]);
    let rendered = tree.render(&NO_RULES).expect("discovery succeeds");
    assert_eq!(
        body(&rendered),
        "mod alpha;\n\
         mod grouped;\n\
         mod owner;\n\
         mod plain_group {\n    mod nested {\n        mod two;\n    }\n    mod one;\n}\n\
         mod zeta;\n"
    );
}

#[test]
fn renames_and_cfg_gates_apply_to_top_level_entries_only() {
    let tree = Tree::new(&["guards/mod.rs", "observed.rs", "plain.rs", "grp/inner.rs"]);
    let rendered = tree
        .render(&DiscoveryRules {
            ignored_dirs: &[],
            renames: &[("guards", "renamed_guards")],
            cfg_gates: &[("observed", "feature = \"observe\""), ("inner", "never")],
        })
        .expect("discovery succeeds");
    assert_eq!(
        body(&rendered),
        "mod grp {\n    mod inner;\n}\n\
         #[cfg(feature = \"observe\")]\n\
         mod observed;\n\
         mod plain;\n\
         #[path = \"guards/mod.rs\"]\n\
         mod renamed_guards;\n"
    );
}

#[test]
fn a_file_and_a_hand_written_directory_with_one_name_are_rejected() {
    let tree = Tree::new(&["twin.rs", "twin/mod.rs"]);
    let error = tree.render(&NO_RULES).expect_err("duplicate entry");
    assert!(
        matches!(error, DiscoveryError::DuplicateEntry { ref module, .. } if module == "twin"),
        "{error}"
    );
}

#[test]
fn entries_differing_only_by_letter_case_are_rejected() {
    let tree = Tree::new(&["alpha.rs", "Beta.rs"]);
    let error = tree
        .render(&DiscoveryRules {
            ignored_dirs: &[],
            renames: &[("Beta", "ALPHA")],
            cfg_gates: &[],
        })
        .expect_err("case collision");
    assert!(
        matches!(error, DiscoveryError::CaseCollision { .. }),
        "{error}"
    );
}
