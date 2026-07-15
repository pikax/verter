//! Architecture guard: the framework carrier parser has exactly ONE counted
//! chokepoint in `verter_session` production code.
//!
//! `MetaProvenance::carrier_parses` is the dedup suite's framework-neutral rail
//! for "how many carrier parses did this path run" (with the Vue compatibility
//! rail `sfc_parses` bumped when the dispatched carrier is Vue) — the cold
//! per-file artifact-build contract pins exact counts on it. The rail is sound
//! only if EVERY `CarrierCompiler::parse` execution in this crate increments
//! it, which is guaranteed by routing every call through
//! `crate::parse::parse_carrier_counted` (the increment lives inside the
//! chokepoint, not at call sites). A direct `compiler.parse(` / registry
//! `.parse()` call anywhere else in production source is an uncounted parse the
//! suite cannot see.
//!
//! The companion compiler-side guard keeps Vue's raw `parse_sfc` confined to
//! the Vue bridge (`verter_compiler`); this guard owns the `verter_session`
//! host-side carrier-parse chokepoint only.
//!
//! Test files (`*_tests.rs`) are exempt: they may parse fixtures directly
//! without touching a host's provenance rail.
//!
//! Honest coverage limits: the rail is textual. The call matcher is the
//! `.parse(` substring on a `CarrierCompiler` receiver (`compiler.parse(`); the
//! counted wrapper's own name (`parse_carrier_counted(`) does not contain it.
//! Not covered: a re-export of the trait method under a different name, and
//! inline `#[cfg(test)]` modules inside production files (their raw calls FAIL
//! the guard — fail-closed; route them through a `*_tests.rs` file).

use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("guard must read dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("guard must read entry: {e}"));
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn carrier_parse_routes_through_the_counted_chokepoint() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_root, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "guard must enumerate the crate's production sources",
    );

    let mut raw_call_sites: Vec<String> = Vec::new();
    let mut chokepoint_file_raw_calls = 0usize;
    let mut chokepoint_defined = false;

    for file in &files {
        let file_name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        // Test modules are exempt — they parse fixtures directly and do
        // not feed the per-host provenance rail.
        if file_name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("guard must read {}: {e}", file.display()));
        let is_chokepoint_file = file_name == "parse.rs";
        if is_chokepoint_file && text.contains("pub(crate) fn parse_carrier_counted(") {
            chokepoint_defined = true;
        }
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            // Call syntax only: a `CarrierCompiler` receiver's `.parse(`. The
            // counted wrapper's own name (`parse_carrier_counted(`) does not
            // contain `.parse(`, and the `parse_sfc(` Vue-bridge call lives in
            // `verter_compiler`, not here.
            if !line.contains("compiler.parse(") {
                continue;
            }
            if is_chokepoint_file {
                chokepoint_file_raw_calls += 1;
                continue;
            }
            raw_call_sites.push(format!("{}:{}: {}", file.display(), idx + 1, line.trim()));
        }
    }

    assert!(
        chokepoint_defined,
        "anti-vacuity: the counted chokepoint `parse::parse_carrier_counted` \
         must exist (the guard would otherwise pass on a tree with no \
         counting at all)",
    );
    assert_eq!(
        chokepoint_file_raw_calls, 1,
        "parse.rs must contain exactly ONE raw `compiler.parse(` call — the \
         body of the counted chokepoint (got {chokepoint_file_raw_calls})",
    );
    assert!(
        raw_call_sites.is_empty(),
        "every framework carrier parse in verter_session production code must \
         route through the counted chokepoint `parse::parse_carrier_counted` \
         (an uncounted parse is invisible to the `carrier_parses` dedup rail). \
         Raw call sites found:\n{}",
        raw_call_sites.join("\n"),
    );
}
