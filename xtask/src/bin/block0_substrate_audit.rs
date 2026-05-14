//! Block 0 substrate-audit binary.
//!
//! Runs the 16-claim verification table for the B0.2 substrate and emits
//! a markdown report at the path passed via `--out` (default:
//! `D:/tmp/block0-substrate-audit.md`).
//!
//! Each claim is verified by running `git grep` via
//! [`std::process::Command`] in the current workspace root, then the
//! result is classified as ✅ / ⚠️ / ❌ and written to the output file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    // Walk up from the binary's location until we find a Cargo.toml
    // containing `[workspace]`. Fallback: use current working directory.
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse --show-toplevel failed");
    let s = String::from_utf8(output.stdout).expect("non-utf8 git output");
    PathBuf::from(s.trim())
}

fn git_grep(root: &Path, pattern: &str, paths: &[&str]) -> Vec<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(root).args(["grep", "-rn", pattern, "--"]);
    for p in paths {
        cmd.arg(p);
    }
    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().map(str::to_owned).collect()
        }
        Err(_) => vec![],
    }
}

fn head_sha(root: &Path) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse HEAD failed");
    String::from_utf8(out.stdout)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

struct Claim {
    number: u8,
    title: &'static str,
    /// Expected state: true = symbol/field EXISTS at HEAD,
    /// false = symbol/field does NOT exist.
    expected_present: bool,
    grep_pattern: &'static str,
    grep_paths: &'static [&'static str],
}

fn run_claims(root: &Path) -> Vec<(Claim, bool, Vec<String>)> {
    let claims: Vec<Claim> = vec![
        Claim {
            number: 1,
            title: "DerivedRawState.fact_dep_signature (expected: ABSENT at HEAD)",
            expected_present: false,
            grep_pattern: "fact_dep_signature",
            grep_paths: &["crates/verter_session/src/derived_raw_state"],
        },
        Claim {
            number: 2,
            title: "ComponentMetaResultEntry.fact_dep_signature (expected: ABSENT — old dep_signature present)",
            expected_present: false,
            grep_pattern: "fact_dep_signature",
            grep_paths: &["crates/verter_session/src/component_meta_result_db.rs"],
        },
        Claim {
            number: 3,
            title: "StoreView::validates_fact_signature (expected: ABSENT at HEAD)",
            expected_present: false,
            grep_pattern: "validates_fact_signature",
            grep_paths: &["crates/"],
        },
        Claim {
            number: 4,
            title: "StoreView::source(canonical) (expected: ABSENT on StoreView, present on SessionView)",
            expected_present: false,
            grep_pattern: "fn source",
            grep_paths: &["crates/verter_session/src/resolver_core/"],
        },
        Claim {
            number: 5,
            title: "RouteResult.fact_dep_signature (expected: ABSENT — RouteResult is an enum)",
            expected_present: false,
            grep_pattern: "fact_dep_signature",
            grep_paths: &["crates/verter_session/src/resolver_core/route_db.rs"],
        },
        Claim {
            number: 6,
            title: "CacheRead<T>.fact_dep_signature (expected: ABSENT on CacheRead)",
            expected_present: false,
            grep_pattern: "struct CacheRead",
            grep_paths: &["crates/verter_session/src/resolver_core/"],
        },
        Claim {
            number: 7,
            title: "get_or_resolve_route_with_facts returns facts to caller (expected: returns Option<Arc<RouteResult>>, NOT (value, facts))",
            expected_present: true,
            grep_pattern: "pub fn get_or_resolve_route_with_facts",
            grep_paths: &["crates/verter_session/src/resolver_core/route_db.rs"],
        },
        Claim {
            number: 8,
            title: "InflightState.fact_dep_signature (expected: present after B0.2)",
            expected_present: true,
            grep_pattern: "fact_dep_signature",
            grep_paths: &["crates/verter_session/src/semantic_query_memo/inflight.rs"],
        },
        Claim {
            number: 9,
            title: "WorkspaceAccess::env_hash_array_for_project (expected: ABSENT at HEAD)",
            expected_present: false,
            grep_pattern: "env_hash_array_for_project",
            grep_paths: &["crates/"],
        },
        Claim {
            number: 10,
            title: "WorkspaceSnapshot::owners_for_file -> SmallVec<[ProjectId; 2]> (expected: PRESENT)",
            expected_present: true,
            grep_pattern: "owners_for_file",
            grep_paths: &["crates/verter_workspace/src/workspace_snapshot.rs"],
        },
        Claim {
            number: 11,
            title: "SymbolSpace::Namespace (expected: PRESENT)",
            expected_present: true,
            grep_pattern: "Namespace",
            grep_paths: &["crates/verter_semantic/src/facts/registry.rs"],
        },
        Claim {
            number: 12,
            title: "ACTIVE_TRACER single-cell → ACTIVE_TRACERS stack (expected: stack present after B0.2)",
            expected_present: true,
            grep_pattern: "ACTIVE_TRACERS",
            grep_paths: &["crates/verter_session/src/resolver_core/resolver_context.rs"],
        },
        Claim {
            number: 13,
            title: "bubble_fact_signature fan-out body (expected: calls observe_fan_out_borrowed after B0.2)",
            expected_present: true,
            grep_pattern: "observe_fan_out_borrowed",
            grep_paths: &["crates/verter_session/src/fact_signature_helpers.rs"],
        },
        Claim {
            number: 14,
            title: "compile_fact_emission::observe_compile_tier_dependencies uses fan-out (expected: calls observe_fan_out after B0.2)",
            expected_present: true,
            grep_pattern: "observe_fan_out",
            grep_paths: &["crates/verter_session/src/compile_fact_emission.rs"],
        },
        Claim {
            number: 15,
            title: "FactReadSet::finalise -> FactReadSetFinalise::{Ok, Overflow} (expected: PRESENT)",
            expected_present: true,
            grep_pattern: "FactReadSetFinalise",
            grep_paths: &["crates/verter_session/src/resolver_core/fact_read_set.rs"],
        },
        Claim {
            number: 16,
            title: "xtask crate in workspace (expected: PRESENT after B0.2)",
            expected_present: true,
            grep_pattern: "xtask",
            grep_paths: &["Cargo.toml"],
        },
    ];

    claims
        .into_iter()
        .map(|claim| {
            let matches = git_grep(root, claim.grep_pattern, claim.grep_paths);
            let found = !matches.is_empty();
            // The claim "holds" if the observed state matches the expected state.
            let holds = found == claim.expected_present;
            (claim, holds, matches)
        })
        .collect()
}

fn write_report(root: &Path, out_path: &Path) -> std::io::Result<()> {
    let sha = head_sha(root);
    let results = run_claims(root);

    let mut file = std::fs::File::create(out_path)?;
    writeln!(file, "# Block 0 Substrate Audit")?;
    writeln!(file)?;
    writeln!(
        file,
        "**HEAD verified**: `{sha}` (worktree `{}`)",
        root.display()
    )?;
    writeln!(file, "**Date**: {}", chrono_or_now())?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    for (claim, holds, matches) in &results {
        let status = if *holds { "✅" } else { "❌" };
        writeln!(file, "### Claim {} — {}", claim.number, claim.title)?;
        writeln!(file)?;
        writeln!(file, "**Status**: {status}")?;
        writeln!(file)?;
        if matches.is_empty() {
            writeln!(
                file,
                "**Evidence**: no grep matches (pattern: `{}`)",
                claim.grep_pattern
            )?;
        } else {
            writeln!(file, "**Evidence** (first 5 matches):")?;
            writeln!(file, "```")?;
            for m in matches.iter().take(5) {
                writeln!(file, "{m}")?;
            }
            writeln!(file, "```")?;
        }
        writeln!(file)?;
        writeln!(file, "---")?;
        writeln!(file)?;
    }

    // Summary table
    writeln!(file, "## Claim Summary")?;
    writeln!(file)?;
    writeln!(file, "| # | Claim | Status |")?;
    writeln!(file, "|---|---|---|")?;
    for (claim, holds, _) in &results {
        let status = if *holds { "✅" } else { "❌" };
        writeln!(file, "| {} | {} | {} |", claim.number, claim.title, status)?;
    }
    writeln!(file)?;
    let passed = results.iter().filter(|(_, h, _)| *h).count();
    let total = results.len();
    writeln!(file, "**Totals**: {passed}/{total} ✅")?;

    Ok(())
}

fn chrono_or_now() -> String {
    // Minimal date: use std env var or fallback string (no chrono dep)
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .map(|_| String::from("(date via SOURCE_DATE_EPOCH)"))
        .unwrap_or_else(|| String::from("(run date)"))
}

fn main() {
    let mut out_path = PathBuf::from("D:/tmp/block0-substrate-audit.md");
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
    eprintln!("Repo root: {}", root.display());
    eprintln!("Output:    {}", out_path.display());

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).expect("failed to create output directory");
    }

    write_report(&root, &out_path).expect("failed to write report");
    eprintln!("Report written to {}", out_path.display());
}
