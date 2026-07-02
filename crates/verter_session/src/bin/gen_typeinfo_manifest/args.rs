//! CLI argument parsing for the generator bin.

use std::collections::BTreeSet;
use std::process::exit;

/// Returns `true` for check-only mode. Accepts `--check` / `--verify`;
/// rejects anything else with exit 2.
pub(crate) fn parse_args() -> bool {
    let flags: BTreeSet<String> = std::env::args().skip(1).collect();
    let unknown: Vec<&str> = flags
        .iter()
        .map(String::as_str)
        .filter(|f| *f != "--check" && *f != "--verify")
        .collect();
    if !unknown.is_empty() {
        eprintln!(
            "error: unknown argument(s): {}. Usage: gen-typeinfo-manifest [--check|--verify]",
            unknown.join(", ")
        );
        exit(2);
    }
    !flags.is_empty()
}
