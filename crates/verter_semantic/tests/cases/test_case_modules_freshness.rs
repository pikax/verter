//! The committed `tests/cases/mod.rs` is byte-equal to the module list
//! rendered from the entries beside it.
//!
//! The file is generated, never edited by hand, and this check renders in
//! memory without writing: a check that regenerates the file it is checking
//! can only pass. Regenerate deliberately with the command in the failure
//! message.

#[allow(dead_code)]
mod discovery {
    include!("../../../../tools/rust/test_case_discovery.rs");
}

use std::path::Path;

#[test]
fn committed_case_module_list_matches_the_entries_beside_it() {
    let cases_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let package = env!("CARGO_PKG_NAME");
    let rendered =
        discovery::render_cases_module(package, &cases_dir, &discovery::rules_for(package))
            .unwrap_or_else(|error| panic!("test case discovery failed: {error}"));
    let committed = discovery::normalized_text(&cases_dir.join("mod.rs"))
        .unwrap_or_else(|error| panic!("`tests/cases/mod.rs` must be readable: {error}"));
    assert!(
        committed == rendered,
        "`tests/cases/mod.rs` is out of date with the entries beside it; regenerate it with \
         `{}` and commit the result",
        discovery::REGENERATE_COMMAND
    );
}
