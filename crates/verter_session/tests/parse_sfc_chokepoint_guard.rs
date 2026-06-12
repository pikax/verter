//! Architecture guard: the SFC structure parser has exactly ONE counted
//! chokepoint in `verter_session` production code.
//!
//! `MetaProvenance::sfc_parses` is the dedup suite's rail for "how many
//! SFC structure parses did this path run" — the cold per-file
//! artifact-build contract pins exact counts on it. The rail is sound
//! only if EVERY `verter_compiler::compile::parse_sfc` execution in this
//! crate increments it, which is guaranteed by routing every call
//! through `crate::parse::parse_sfc_counted` (the increment lives inside
//! the chokepoint, not at call sites). A raw `parse_sfc(` call anywhere
//! else in production source is an uncounted parse the suite cannot see.
//!
//! Test files (`*_tests.rs`) are exempt: they may parse fixtures
//! directly without touching a host's provenance rail.
//!
//! Honest coverage limits: the rail is textual. The call matcher is
//! the `parse_sfc(` substring (module-qualified calls like
//! `compile::parse_sfc(` still contain it); an `as`-renamed import
//! (`use …::parse_sfc as ps;`) would detach calls from the substring,
//! so the guard separately rejects ANY `parse_sfc as ` aliasing in
//! production source. Not covered: a re-export of `parse_sfc` under a
//! different name in some other crate, and inline `#[cfg(test)]`
//! modules inside production files (their raw calls FAIL the guard —
//! fail-closed; route them through a `*_tests.rs` file).

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
fn sfc_structure_parse_routes_through_the_counted_chokepoint() {
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
        if is_chokepoint_file && text.contains("pub(crate) fn parse_sfc_counted(") {
            chokepoint_defined = true;
        }
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            // `as`-renamed imports detach calls from the `parse_sfc(`
            // substring rail (`use …::parse_sfc as ps;` then `ps(…)`),
            // so the aliasing itself is rejected everywhere in
            // production source, chokepoint file included.
            if line.contains("parse_sfc as ") {
                raw_call_sites.push(format!(
                    "{}:{}: `parse_sfc` must not be `as`-renamed (it \
                     detaches calls from the textual chokepoint rail): {}",
                    file.display(),
                    idx + 1,
                    line.trim()
                ));
                continue;
            }
            // Call syntax only: `parse_sfc(`. The counted wrapper's own
            // name (`parse_sfc_counted(`) does not contain this
            // substring, and `use ...::parse_sfc;` has no paren.
            if !line.contains("parse_sfc(") {
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
        "anti-vacuity: the counted chokepoint `parse::parse_sfc_counted` \
         must exist (the guard would otherwise pass on a tree with no \
         counting at all)",
    );
    assert_eq!(
        chokepoint_file_raw_calls, 1,
        "parse.rs must contain exactly ONE raw `parse_sfc(` call — the \
         body of the counted chokepoint (got {chokepoint_file_raw_calls})",
    );
    assert!(
        raw_call_sites.is_empty(),
        "every SFC structure parse in verter_session production code must \
         route through the counted chokepoint `parse::parse_sfc_counted` \
         (an uncounted parse is invisible to the `sfc_parses` dedup rail). \
         Raw call sites found:\n{}",
        raw_call_sites.join("\n"),
    );
}
