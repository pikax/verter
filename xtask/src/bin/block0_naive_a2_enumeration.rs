//! Block 0 naive-A2 enumeration binary.
//!
//! Creates a temporary worktree of the current branch, applies the
//! eager-invalidation deletion (the block under `host_upsert.rs` lines
//! 432-437 and 449-471), runs `cargo test --workspace --tests --verbose`,
//! parses failing test names, emits a markdown summary to `--out`
//! (default: `D:/tmp/block0-naive-a2-failures.md`), then cleans up the
//! temp worktree.
//!
//! **Safety**: this binary NEVER modifies the live worktree at `D:/wt/s0/`.
//! All mutations happen in a temporary directory created via
//! `std::env::temp_dir()`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse --show-toplevel failed");
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim().to_owned())
}

fn current_branch(root: &Path) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .expect("git rev-parse HEAD failed");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Create a new git worktree in a temp dir and return (worktree_path, worktree_name).
fn create_temp_worktree(root: &Path, branch: &str) -> PathBuf {
    let tmp_dir = std::env::temp_dir().join(format!("verter-b0-a2-{}", std::process::id()));
    let status = Command::new("git")
        .current_dir(root)
        .args([
            "worktree",
            "add",
            "--detach",
            &tmp_dir.to_string_lossy(),
            "HEAD",
        ])
        .status()
        .expect("git worktree add failed");
    if !status.success() {
        panic!("Failed to create temporary worktree at {}", tmp_dir.display());
    }
    eprintln!("Created temp worktree: {} (branch: {})", tmp_dir.display(), branch);
    tmp_dir
}

/// Remove the temp worktree.
fn remove_temp_worktree(root: &Path, tmp_dir: &Path) {
    let _ = Command::new("git")
        .current_dir(root)
        .args(["worktree", "remove", "--force", &tmp_dir.to_string_lossy()])
        .status();
    eprintln!("Removed temp worktree: {}", tmp_dir.display());
}

/// Apply the eager-invalidation deletion in `host_upsert.rs` in the temp worktree.
/// The deletion covers lines 432-437 and 449-471 (per the B0.1 audit).
/// We use a conservative string-replacement approach to avoid line-number drift.
fn apply_eager_invalidation_deletion(tmp_dir: &Path) -> bool {
    let host_upsert = tmp_dir.join("crates/verter_session/src/host_upsert.rs");
    let content = match std::fs::read_to_string(&host_upsert) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Warning: could not read host_upsert.rs: {e}");
            return false;
        }
    };

    // The eager-invalidation block is identifiable by its distinctive comment.
    // We remove the block between the two sentinel markers that the audit identified.
    // Strategy: remove lines containing the eager cascade / smart_invalidate call.
    let modified = remove_eager_invalidation_block(&content);
    if modified == content {
        eprintln!("Warning: eager-invalidation block not found in host_upsert.rs — \
                   the A2 enumeration may produce incomplete results.");
        return false;
    }
    std::fs::write(&host_upsert, &modified)
        .expect("failed to write modified host_upsert.rs");
    eprintln!("Applied eager-invalidation deletion to host_upsert.rs");
    true
}

/// Remove the eager cascade block from `host_upsert.rs`.
/// Targets the `smart_invalidate_dependents` call and the R3 eager-invalidation
/// comment block that follows the `set_import_dependencies` call.
fn remove_eager_invalidation_block(content: &str) -> String {
    // Find the section by looking for the eager-invalidation comment.
    // The block to delete is identified by the audit as the `smart_invalidate_dependents`
    // calls plus their guard/comment at lines ~432-437 and ~449-471.
    // We look for the function call pattern and its surrounding braces.
    let marker_start = "smart_invalidate_dependents";
    if !content.contains(marker_start) {
        return content.to_owned();
    }

    // Remove each line that calls smart_invalidate_dependents (and its enclosing
    // if-let guard lines) by filtering out the relevant block.
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::with_capacity(lines.len());
    let mut skip_depth: i32 = 0;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Detect the start of an eager-invalidation block.
        // The audit shows lines like:
        //   smart_invalidate_dependents(self, &changed_files, &snapshot);
        // or wrapped in an `if !changed_files.is_empty() {` guard.
        if trimmed.contains("smart_invalidate_dependents") && skip_depth == 0 {
            // Skip this line only (function call on one line).
            i += 1;
            continue;
        }

        if skip_depth > 0 {
            if trimmed.starts_with('}') {
                skip_depth -= 1;
                if skip_depth == 0 {
                    i += 1;
                    continue; // skip the closing brace too
                }
            } else if trimmed.ends_with('{') {
                skip_depth += 1;
            }
            i += 1;
            continue;
        }

        result.push(line);
        i += 1;
    }

    result.join("\n")
}

/// Run `cargo test --workspace --tests --verbose` in the temp worktree.
/// Returns (exit_success, stdout+stderr combined).
fn run_tests(tmp_dir: &Path) -> (bool, String) {
    eprintln!("Running cargo test in {}...", tmp_dir.display());
    let output = Command::new("cargo")
        .current_dir(tmp_dir)
        .args(["test", "--workspace", "--tests", "--verbose"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to spawn cargo test");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), combined)
}

/// Parse failing test names from cargo test output.
fn parse_failures(output: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let mut in_failures_section = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "failures:" {
            in_failures_section = true;
            continue;
        }
        if in_failures_section {
            if trimmed.is_empty() || trimmed.starts_with("test result:") {
                in_failures_section = false;
                continue;
            }
            // Lines in the failures section look like "    test_name"
            if !trimmed.is_empty() {
                failures.push(trimmed.to_owned());
            }
        }
    }
    failures
}

fn write_report(out_path: &Path, applied: bool, success: bool, failures: &[String], output: &str) {
    let mut file = std::fs::File::create(out_path).expect("failed to create output file");
    writeln!(file, "# Block 0 Naive-A2 Enumeration Report").unwrap();
    writeln!(file).unwrap();
    writeln!(file, "**Eager-invalidation deletion applied**: {applied}").unwrap();
    writeln!(file, "**Test suite result**: {}", if success { "PASS" } else { "FAIL" }).unwrap();
    writeln!(file).unwrap();
    if failures.is_empty() {
        writeln!(file, "No test failures detected.").unwrap();
    } else {
        writeln!(file, "## Failing Tests ({} total)", failures.len()).unwrap();
        writeln!(file).unwrap();
        for f in failures {
            writeln!(file, "- `{f}`").unwrap();
        }
    }
    writeln!(file).unwrap();
    writeln!(file, "## Raw Output (truncated to 20 000 chars)").unwrap();
    writeln!(file, "```").unwrap();
    let truncated = &output[..output.len().min(20_000)];
    writeln!(file, "{truncated}").unwrap();
    writeln!(file, "```").unwrap();
}

fn main() {
    let mut out_path = PathBuf::from("D:/tmp/block0-naive-a2-failures.md");
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--out" && i + 1 < args.len() {
            out_path = PathBuf::from(&args[i + 1]);
            i += 2;
        } else {
            i += 1;
        }
    }

    let root = repo_root();
    let branch = current_branch(&root);
    eprintln!("Repo root: {}", root.display());
    eprintln!("Branch:    {branch}");
    eprintln!("Output:    {}", out_path.display());

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).expect("failed to create output dir");
    }

    let tmp_dir = create_temp_worktree(&root, &branch);
    let applied = apply_eager_invalidation_deletion(&tmp_dir);
    let (success, output) = run_tests(&tmp_dir);
    let failures = parse_failures(&output);

    write_report(&out_path, applied, success, &failures, &output);
    remove_temp_worktree(&root, &tmp_dir);

    eprintln!("Report written to {}", out_path.display());
    if !success {
        eprintln!("{} failing tests found", failures.len());
    }
}
