//! Rewrites the committed `tests/cases/mod.rs` of every crate whose
//! integration binary discovers its entries from that directory.
//!
//! Generation is a COMMAND, never a test: each crate's freshness test renders
//! in memory and compares, so no check can make itself pass by writing the
//! file it is checking. Run it with `pnpm gen:test-cases` after adding,
//! removing or renaming a case.

include!("../../../tools/rust/test_case_discovery.rs");

use std::process::ExitCode;

fn main() -> ExitCode {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives directly under the workspace root");
    let mut failed = false;
    for package in DISCOVERED_PACKAGES {
        let cases_dir = workspace
            .join("crates")
            .join(package)
            .join("tests")
            .join("cases");
        let rendered = match render_cases_module(package, &cases_dir, &rules_for(package)) {
            Ok(rendered) => rendered,
            Err(error) => {
                eprintln!("{package}: {error}");
                failed = true;
                continue;
            }
        };
        let target = cases_dir.join("mod.rs");
        if let Err(error) = fs::write(&target, rendered) {
            eprintln!("could not write {}: {error}", target.display());
            failed = true;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
