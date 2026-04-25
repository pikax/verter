//! F1 (Plan §3 Step 3 Test 4) — consumer-audit grep gate.
//!
//! Asserts that every TypeScript / JavaScript consumer of `meta.origin`
//! across `packages/` honors the audit-only contract D34 — the field is
//! `undefined` unless the host is configured for audit. Bare reads
//! like `meta.origin.foo` would NPE on default-mode hosts.
//!
//! Acceptable patterns at any grep hit:
//! 1. Path matches `**/audit/**` or filename contains `audit` (the
//!    consumer is itself audit-only and its inputs are audit-on by
//!    construction).
//! 2. Within ~6 lines of context above the read, an `audit_enabled`
//!    guard or branch is visible (the consumer gates its read).
//! 3. Optional-chaining read: `meta.origin?.X` or `meta.origin && …`.
//! 4. Type annotation / type-only reference (`origin?:`, `Origin =`,
//!    type position) — no runtime read.
//!
//! Pre-fix: a hypothetical bare `meta.origin.X` read would have been
//! tolerated. Post-fix: any new bare read trips this test.

use std::process::Command;

#[test]
fn no_bare_origin_reads_outside_audit_paths() {
    let workspace_root = workspace_root();
    let output = Command::new("git")
        .arg("grep")
        .arg("-nE")
        .arg("--")
        // Search inside the strings above.
        .arg(r"meta\.origin\b|\borigin\.(nodes|edges|metaStrings)\b")
        .arg("packages/")
        .current_dir(&workspace_root)
        .output()
        .expect("git grep must succeed (run from a git repo)");

    if !output.status.success() && !output.stdout.is_empty() {
        panic!(
            "git grep exited non-zero: status={:?}\nstderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut violations: Vec<String> = Vec::new();

    for line in stdout.lines() {
        // git grep output: `path:linenum:content`
        let Some((path_lineno, content)) = line
            .split_once(':')
            .and_then(|(p, rest)| rest.split_once(':').map(|(ln, c)| (format!("{p}:{ln}"), c)))
        else {
            continue;
        };
        let path_lower = path_lineno.to_lowercase();

        // (1) Audit paths are exempt by construction.
        if path_lower.contains("/audit") || path_lower.contains("audit") {
            continue;
        }

        // (3) Optional-chain or null-guard reads are safe.
        if content.contains(".origin?.")
            || content.contains(".origin ?")
            || content.contains(".origin &&")
            || content.contains(".origin == null")
            || content.contains(".origin === null")
            || content.contains(".origin == undefined")
            || content.contains(".origin === undefined")
            || content.contains(".origin !== undefined")
            || content.contains(".origin !== null")
            || content.contains(".origin || ")
            || content.contains(".origin ?? ")
        {
            continue;
        }

        // (4) Type annotations / declarations don't read at runtime.
        if content.contains("origin?:")
            || content.contains("origin?: ")
            || content.contains("type ")
            || content.contains("interface ")
            || content.contains("import")
            || content.contains("export")
            || content.contains(": NativeOriginGraph")
            || content.trim_start().starts_with("//")
            || content.trim_start().starts_with("*")
        {
            continue;
        }

        // (2) Audit-gating context check: read 6 lines of preceding
        // context from the file. If `audit_enabled` appears in that
        // window, the read is gated. (Fast cheap heuristic — false
        // positives are acceptable; this is a tightening over a
        // missing gate.)
        let parts: Vec<&str> = path_lineno.split(':').collect();
        if parts.len() == 2 {
            let path = parts[0];
            let lineno: usize = parts[1].parse().unwrap_or(0);
            if let Ok(file) = std::fs::read_to_string(workspace_root.join(path)) {
                let lines: Vec<&str> = file.lines().collect();
                let start = lineno.saturating_sub(6).max(1);
                let end = lineno.min(lines.len());
                let context = &lines[start - 1..end].join("\n");
                if context.contains("audit_enabled")
                    || context.contains("auditEnabled")
                    || context.contains("audit?")
                    // `origin !== undefined && ...` / `origin != null && ...`
                    // / `if (origin)` block guards.
                    || context.contains("origin !== undefined")
                    || context.contains("origin !== null")
                    || context.contains("origin != null")
                    || context.contains("origin != undefined")
                    || context.contains("if (origin)")
                    || context.contains("if (origin &&")
                    || context.contains("origin === undefined")
                    || context.contains("origin === null")
                    // Fixture-construction context — assigning a NativeOriginGraph
                    // | undefined typed variable already proves the consumer
                    // is undefined-aware.
                    || context.contains(": NativeOriginGraph | undefined")
                    || context.contains("as NativeOriginGraph | undefined")
                {
                    continue;
                }
            }
        }

        // No exemption matched — the read is a bare runtime access.
        violations.push(format!("{path_lineno} :: {}", content.trim()));
    }

    assert!(
        violations.is_empty(),
        "F1 D34 consumer contract: bare `meta.origin.X` / `origin.{{nodes,edges,metaStrings}}` \
         reads must be gated by `audit_enabled` or use optional-chaining. \
         New violations:\n  {}",
        violations.join("\n  "),
    );
}

fn workspace_root() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
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
