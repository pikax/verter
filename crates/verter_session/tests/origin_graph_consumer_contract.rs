//! F1 (Plan §3 Step 3 Test 4) — consumer-audit grep gate.
//!
//! Asserts that every TypeScript / JavaScript consumer of `meta.origin`
//! across `packages/` honors the audit-only contract D34 — the field is
//! `undefined` unless the host is configured for audit. Bare reads
//! like `meta.origin.foo` would NPE on default-mode hosts.
//!
//! Acceptable patterns at any grep hit:
//! 1. The hit is in an audit-scoped file path (`**/audit/**`).
//! 2. Optional-chain or null-guard on the read line itself.
//! 3. A type position (annotation / declaration / import) — no runtime read.
//! 4. The 6-line preceding context contains an audit gate or
//!    `origin !== undefined` / `if (origin && ...)` block guard.
//!
//! Pre-fix: a hypothetical bare `meta.origin.X` read was tolerated.
//! Post-fix: any new bare read trips this test.

use std::path::{Path, PathBuf};
use std::process::Command;

const ORIGIN_PATTERN: &str = r"meta\.origin\b|\borigin\.(nodes|edges|metaStrings)\b";

/// Patterns that, when present on the same line as the grep hit,
/// prove the read uses optional-chaining or a null/undefined check
/// before deref. Order does not matter; we test each as a substring.
const ON_LINE_GUARD_NEEDLES: &[&str] = &[
    ".origin?.",
    ".origin ?",
    ".origin &&",
    ".origin == null",
    ".origin === null",
    ".origin == undefined",
    ".origin === undefined",
    ".origin !== undefined",
    ".origin !== null",
    ".origin || ",
    ".origin ?? ",
];

/// Patterns that mark the line as a type position (declaration,
/// import, type annotation) rather than a runtime field access.
const TYPE_POSITION_NEEDLES: &[&str] = &[
    "origin?:",
    ": NativeOriginGraph",
    "type ",
    "interface ",
    "import",
    "export",
];

/// Patterns that, when present in the 6 lines preceding the hit,
/// prove the surrounding block already gates on audit configuration
/// or on `origin !== undefined`. Aliased reads like
/// `const origin = meta.origin; if (origin) origin.edges` follow this
/// shape and are safe.
const BLOCK_GUARD_NEEDLES: &[&str] = &[
    "audit_enabled",
    "auditEnabled",
    "audit?",
    "origin !== undefined",
    "origin !== null",
    "origin != null",
    "origin != undefined",
    "origin === undefined",
    "origin === null",
    "if (origin)",
    "if (origin &&",
    ": NativeOriginGraph | undefined",
    "as NativeOriginGraph | undefined",
];

#[test]
fn no_bare_origin_reads_outside_audit_paths() {
    let workspace_root = workspace_root();
    let output = Command::new("git")
        .args(["grep", "-nE", "--", ORIGIN_PATTERN, "packages/"])
        .current_dir(&workspace_root)
        .output()
        .expect("git grep must succeed (run from a git repo)");

    // git grep exits 1 when no matches found; treat that as "no
    // violations, no work to do" rather than a panic.
    if !output.status.success() && !output.stdout.is_empty() {
        panic!(
            "git grep exited non-zero with output: status={:?}\nstderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut violations: Vec<String> = Vec::new();

    for line in stdout.lines() {
        let Some(hit) = parse_grep_hit(line) else {
            continue;
        };
        if is_audit_path(hit.path)
            || any_needle(hit.content, ON_LINE_GUARD_NEEDLES)
            || any_needle(hit.content, TYPE_POSITION_NEEDLES)
            || hit.content.trim_start().starts_with("//")
            || hit.content.trim_start().starts_with("*")
        {
            continue;
        }
        if has_preceding_block_guard(&workspace_root, &hit) {
            continue;
        }
        violations.push(format!(
            "{}:{} :: {}",
            hit.path,
            hit.lineno,
            hit.content.trim()
        ));
    }

    assert!(
        violations.is_empty(),
        "F1 D34 consumer contract: bare `meta.origin.X` / \
         `origin.{{nodes,edges,metaStrings}}` reads must be gated by \
         `audit_enabled`, optional-chained, or block-guarded by \
         `origin !== undefined`. New violations:\n  {}",
        violations.join("\n  "),
    );
}

struct GrepHit<'a> {
    path: &'a str,
    lineno: usize,
    content: &'a str,
}

fn parse_grep_hit(line: &str) -> Option<GrepHit<'_>> {
    // git grep output: `path:linenum:content`. Paths under `packages/`
    // never contain a colon, so a simple two-step split suffices.
    let (path, rest) = line.split_once(':')?;
    let (lineno_str, content) = rest.split_once(':')?;
    let lineno = lineno_str.parse().ok()?;
    Some(GrepHit {
        path,
        lineno,
        content,
    })
}

fn is_audit_path(path: &str) -> bool {
    // Match a path SEGMENT named `audit` or starting with `audit-` /
    // `audit_`, not any incidental substring. Forward slashes only —
    // git grep on Windows still emits forward slashes.
    path.split('/').any(|segment| {
        segment == "audit"
            || segment.starts_with("audit-")
            || segment.starts_with("audit_")
            || segment.contains(".audit.")
    })
}

fn any_needle(content: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| content.contains(n))
}

fn has_preceding_block_guard(workspace_root: &Path, hit: &GrepHit<'_>) -> bool {
    let Ok(file) = std::fs::read_to_string(workspace_root.join(hit.path)) else {
        return false;
    };
    let lines: Vec<&str> = file.lines().collect();
    if hit.lineno == 0 || hit.lineno > lines.len() {
        return false;
    }
    let start = hit.lineno.saturating_sub(6).max(1);
    let context = lines[start - 1..hit.lineno].join("\n");
    any_needle(&context, BLOCK_GUARD_NEEDLES)
}

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("packages").is_dir() && p.join(".git").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "could not find workspace root (no `.git` + `packages/` ancestor of {})",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}
