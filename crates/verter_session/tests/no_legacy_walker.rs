//! LEGACY_GATE_SELF — Phase 9 static-grep gate (plan §11.4).
//!
//! Guards against re-introduction of the legacy walker family after
//! the Phase-9 cutover. The walker's `walk_component_meta_member_surface_expr`
//! shim is RETAINED (it now delegates to the new
//! `materialize_component_meta_structure` entry); its inner helpers
//! (`walker_cycle_key_node`, `expand_generic_ref_via_scope_iteration`,
//! `walk_component_meta_member_surface_expr_with_visited`) are
//! retained `#[allow(dead_code)]` pending follow-up deletion.
//!
//! The gate detects RE-INTRODUCTION of the inner symbols at additional
//! call sites — once the body deletions land, this file's
//! `RETIRED_SYMBOLS` list will tighten to ban every name in the
//! walker family.
//!
//! Self-exclusion: the first 5 lines of this file contain
//! `LEGACY_GATE_SELF` so the recursive walk skips this file.

use std::path::PathBuf;

const RETIRED_SYMBOLS: &[&str] = &[
    // Phase 9 cutover (plan §11.2): the inner helpers are dead post-
    // cutover. Re-introduction at any non-walker site would break
    // the cutover. The legacy `walk_component_meta_member_surface_expr`
    // shim is RETAINED (it delegates to the new materialiser entry)
    // so this gate intentionally does NOT list its name — we want to
    // be alerted only when *new* references to the inner helpers
    // appear.
    "walker_cycle_key_node",
    "expand_generic_ref_via_scope_iteration",
    "walk_component_meta_member_surface_expr_with_visited",
];

const SCAN_DIRS: &[&str] = &["crates", ".claude/skills", "docs"];

const SCAN_FILES: &[&str] = &["CLAUDE.md", "AGENTS.md", "MEMORY.md"];

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("CLAUDE.md").exists() && p.join("crates").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate workspace root from {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

fn is_self_excluded(path: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.lines().take(5).any(|l| l.contains("LEGACY_GATE_SELF"))
}

fn is_changelog(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("CHANGELOG.md"))
        .unwrap_or(false)
}

fn collect_text_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip target/ and node_modules/ — these don't contain hand-authored source.
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            collect_text_files(&path, out);
        } else if path.is_file() {
            let ext_ok = matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "ts" | "tsx" | "js" | "vue" | "md")
            );
            if ext_ok && !is_self_excluded(&path) && !is_changelog(&path) {
                out.push(path);
            }
        }
    }
}

#[test]
fn no_legacy_walker_inner_helpers_outside_their_definitions() {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in SCAN_DIRS {
        let p = root.join(dir);
        if p.exists() {
            collect_text_files(&p, &mut files);
        }
    }
    for file in SCAN_FILES {
        let p = root.join(file);
        if p.exists() && !is_self_excluded(&p) {
            files.push(p);
        }
    }

    // Tight scan: each retired symbol must appear AT MOST in its own
    // definition site (`fn <name>(`) — a re-introduction at another
    // site would mean the symbol has more than one definition + 1 call.
    // Pre-cutover the inner helpers had ~10+ call sites; post-cutover
    // they have ZERO callers (their bodies are unused, gated by
    // `#[allow(dead_code)]`).
    for symbol in RETIRED_SYMBOLS {
        let pattern = symbol.to_string();
        let mut hit_files: Vec<(PathBuf, Vec<usize>)> = Vec::new();
        for file in &files {
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            let lines: Vec<usize> = text
                .lines()
                .enumerate()
                .filter_map(|(i, l)| {
                    if l.contains(&pattern) {
                        Some(i + 1)
                    } else {
                        None
                    }
                })
                .collect();
            if !lines.is_empty() {
                hit_files.push((file.clone(), lines));
            }
        }
        // The symbol's own definition file is meta_resolve.rs — exactly
        // ONE file is allowed to contain references to it (the
        // definition + the dead-code annotation). Anywhere else is a
        // re-introduction.
        let allowed_file_suffix = "meta_resolve.rs";
        for (file, lines) in &hit_files {
            let path_str = file.to_string_lossy();
            // Allow:
            //   - the definition file itself (meta_resolve.rs)
            //   - this test file (self-excluded by marker, defensive)
            //   - historical docs that document the renamed/retired
            //     symbol's history (`docs/arch/debt-closure/`)
            let is_allowed = path_str.ends_with(allowed_file_suffix)
                || path_str.contains("docs/arch/debt-closure/")
                || path_str.contains("docs\\arch\\debt-closure\\");
            assert!(
                is_allowed,
                "Phase 9 static-grep gate (plan §11.4): retired walker-family \
                 symbol `{symbol}` re-introduced at {file:?} lines {lines:?}. \
                 Post-cutover this symbol has NO callers; its definition is \
                 retained `#[allow(dead_code)]` pending follow-up deletion."
            );
        }
    }
}
