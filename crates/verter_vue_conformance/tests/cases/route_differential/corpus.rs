//! The SFCs both routes compile.

use std::path::{Path, PathBuf};

use verter_vue_conformance::{
    case_sfc_paths, corpus_file, corpus_root, read_text_normalized, workspace_root,
};

/// Names the root of an external corpus: a directory whose `cases/` tree
/// holds the `.vue` files, laid out like the conformance corpus.
pub(super) const EXTERNAL_CORPUS_VARIABLE: &str = "VERTER_DIFFERENTIAL_CORPUS";

/// One SFC of the corpus.
#[derive(Debug, Clone)]
pub(super) struct Case {
    /// The path below the corpus root, with `/` separators.
    pub name: String,
    pub source: String,
}

fn cases_in(root: &Path) -> Vec<Case> {
    case_sfc_paths(root)
        .expect("the corpus root holds a readable cases tree")
        .into_iter()
        .map(|name| {
            let source = read_text_normalized(&corpus_file(root, &name))
                .expect("a listed corpus case is readable");
            Case { name, source }
        })
        .collect()
}

/// The conformance cases.
pub(super) fn conformance_cases() -> Vec<Case> {
    cases_in(&corpus_root())
}

pub(super) fn correction_cases() -> Vec<Case> {
    cases_in(&corpus_root().with_file_name("corrections"))
}

/// Where the compiler keeps its minimized regression components.
const REGRESSION_FIXTURES: &str = "crates/verter_compiler/tests/cases/sfc_regression_fixtures";

/// The compiler's regression components, one per file, named
/// `sfc_regression_fixtures/<file>`.
pub(super) fn regression_fixtures() -> Vec<Case> {
    let root = REGRESSION_FIXTURES
        .split('/')
        .fold(workspace_root(), |path, part| path.join(part));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("the regression fixture directory is readable")
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "vue"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let file = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("a fixture has a UTF-8 file name");
            Case {
                name: format!("sfc_regression_fixtures/{file}"),
                source: read_text_normalized(&path).expect("a listed fixture is readable"),
            }
        })
        .collect()
}

/// The conformance cases followed by the regression components.
pub(super) fn compared_cases() -> Vec<Case> {
    let mut cases = conformance_cases();
    cases.extend(regression_fixtures());
    cases
}

/// The external corpus, or `None` when its variable is unset.
pub(super) fn external_cases() -> Option<Vec<Case>> {
    let root = PathBuf::from(std::env::var_os(EXTERNAL_CORPUS_VARIABLE)?);
    Some(cases_in(&root))
}
