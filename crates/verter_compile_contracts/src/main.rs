//! Standalone owner for compile-fail contracts.
//!
//! These fixtures deliberately invoke Cargo through trybuild, so they are not
//! Rust test cases and never enter the nextest inventory. CI selects exactly
//! one owner feature per process to preserve that owner's dependency features.

use std::fs;
use std::path::{Path, PathBuf};

const PASS_FIXTURES: &[&str] = &[
    "no_typeexpr_good_carrier.rs",
    "no_storedspan_good_carrier.rs",
    "recursive_self_good_recursive_carrier.rs",
    "script_fact_partial_exact_syntax.rs",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("compile-contract crate must live below the workspace root")
        .to_path_buf()
}

fn selected_owner() -> (&'static str, &'static str, &'static str) {
    let selected = [
        (
            cfg!(feature = "audit"),
            "audit",
            "crates/verter_audit",
            "tests/cases/compile-fail",
        ),
        (
            cfg!(feature = "compiler-default"),
            "compiler-default",
            "crates/verter_compiler",
            "tests/cases/compile-fail",
        ),
        (
            cfg!(feature = "css-syntax"),
            "css-syntax",
            "crates/verter_css_syntax",
            "tests/compile-fail",
        ),
        (
            cfg!(feature = "identity"),
            "identity",
            "crates/verter_identity",
            "tests/compile-fail",
        ),
        (
            cfg!(feature = "language"),
            "language",
            "crates/verter_language",
            "tests/compile-fail",
        ),
        (
            cfg!(feature = "semantic"),
            "semantic",
            "crates/verter_semantic",
            "tests/compile-fail",
        ),
        (
            cfg!(feature = "session"),
            "session",
            "crates/verter_session",
            "tests/cases/compile-fail",
        ),
        (
            cfg!(feature = "type-runtime"),
            "type-runtime",
            "crates/verter_type_runtime",
            "tests/cases/compile-fail",
        ),
        (
            cfg!(feature = "workspace"),
            "workspace",
            "crates/verter_workspace",
            "tests/compile-fail",
        ),
    ];
    let enabled: Vec<_> = selected
        .into_iter()
        .filter_map(|(enabled, name, root, path)| enabled.then_some((name, root, path)))
        .collect();
    assert_eq!(
        enabled.len(),
        1,
        "enable exactly one compile-contract owner feature"
    );
    enabled[0]
}

fn main() {
    let (owner, relative_owner_root, relative_dir) = selected_owner();
    let owner_root = workspace_root().join(relative_owner_root);
    let fixture_dir = owner_root.join(relative_dir);
    let mut fixtures: Vec<_> = fs::read_dir(&fixture_dir)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", fixture_dir.display()))
        .map(|entry| entry.expect("compile-contract directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .map(|path| {
            path.strip_prefix(&owner_root)
                .expect("fixture must be below its owner crate")
                .to_path_buf()
        })
        .collect();
    fixtures.sort();

    if owner == "compiler-default" {
        fixtures.retain(|path| {
            !matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("pending_nav_request_unreachable.rs")
                    | Some("segmented_overwrite_authority_unreachable.rs")
            )
        });
    }
    if owner == "session" {
        fixtures.retain(|path| {
            !matches!(
                path.file_name().and_then(|name| name.to_str()),
                // These fixtures import `verter_session::for_tests`, which
                // exists only under `cfg(test)` or the `test-support`
                // feature. This runner is a plain binary, so trybuild cannot
                // infer that feature from a test binary's fingerprint and the
                // import fails to resolve — the pinned privacy diagnostics
                // become `E0432 unresolved import` and every one mismatches.
                // They still run, and still hold their seals, under the
                // canonical gate where the feature is on.
                Some("instantiate_key_context_not_extractable.rs")
                    | Some("scanners_replacement_raw_parser_public.rs")
                    | Some("complete_flow_result_constructor_is_private.rs")
                    | Some("flow_solve_sealed_witnesses_not_constructible.rs")
                    | Some("flow_solve_plan_and_spec_no_struct_literal.rs")
                    | Some("flow_solve_plan_and_spec_are_sealed.rs")
            )
        });
    }
    assert!(
        !fixtures.is_empty(),
        "compile-contract owner {owner} has no fixtures"
    );

    eprintln!(
        "compile contracts: owner={owner}, fixtures={}",
        fixtures.len()
    );
    // trybuild normally derives this from the test binary's package. This
    // standalone executable selects the owner explicitly so fixture-relative
    // paths, dependency features, and pinned diagnostics remain unchanged.
    std::env::set_var("CARGO_MANIFEST_DIR", &owner_root);
    std::env::set_current_dir(&owner_root).expect("could not enter compile-contract owner root");
    let tests = trybuild::TestCases::new();
    for fixture in fixtures {
        let basename = fixture.file_name().and_then(|name| name.to_str());
        if PASS_FIXTURES.contains(&basename.unwrap_or_default()) {
            tests.pass(&fixture);
        } else {
            tests.compile_fail(&fixture);
        }
    }
}
