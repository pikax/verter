//! LEGACY_GATE_SELF — Phase 9 static-grep gate (plan §11.4).
//!
//! Guards against re-introduction of the legacy walker family after
//! the Phase-9 cutover. The walker's `walk_component_meta_member_surface_expr`
//! shim is RETAINED (it now delegates to the new
//! `materialize_component_meta_structure` entry); the entire inner
//! body family (`walker_cycle_key_node`,
//! `expand_generic_ref_via_scope_iteration`,
//! `walk_component_meta_member_surface_expr_with_visited`) was
//! deleted in the same commit, along with the
//! `component_meta_dispatch_iteration` module that hosted the
//! walker's visited-set helper.
//!
//! Self-exclusion: the first 5 lines of this file contain
//! `LEGACY_GATE_SELF` so the recursive walk skips this file.

use std::path::PathBuf;

const RETIRED_SYMBOLS: &[&str] = &[
    // Phase 9 cutover (plan §11.2): the inner walker helpers are
    // DELETED. Re-introduction at any site is forbidden. The legacy
    // `walk_component_meta_member_surface_expr` shim name is
    // RETAINED (it delegates to the new materialiser entry) — the
    // gate intentionally does NOT list its name.
    "walker_cycle_key_node",
    "expand_generic_ref_via_scope_iteration",
    "walk_component_meta_member_surface_expr_with_visited",
    // The dispatch-iteration module that hosted the visited-set +
    // generic-rescue helpers was deleted in the same commit.
    "component_meta_dispatch_iteration",
    "WalkerVisitedNodes",
    "VisitedPushOutcome",
    // Plan §11.2 cleanup: the legacy walker's `MaterializedMemberSurfaceDb`
    // family had zero callers post-Phase-9 (the walker shim now delegates
    // to `materialize_component_meta_structure` which publishes through
    // `MaterializeStructureDb`). Re-introducing any of these names at a
    // call site would re-wire the dead cache lane.
    "MaterializedMemberSurfaceDb",
    "MaterializedMemberSurfaceEntry",
    "MaterializedMemberSurfaceKey",
    "MaterializedMemberSurfaceTarget",
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
        // Post-cutover the inner walker helpers are DELETED — the
        // only allowed references are in historical architecture
        // documentation (`docs/arch/debt-closure/`).
        for (file, lines) in &hit_files {
            let path_str = file.to_string_lossy();
            let is_allowed = path_str.contains("docs/arch/debt-closure/")
                || path_str.contains("docs\\arch\\debt-closure\\");
            assert!(
                is_allowed,
                "Phase 9 static-grep gate (plan §11.4): retired walker-family \
                 symbol `{symbol}` reintroduced at {file:?} lines {lines:?}. \
                 Post-cutover the inner walker family is DELETED — the only \
                 allowed references are historical docs under \
                 `docs/arch/debt-closure/`."
            );
        }
    }
}
