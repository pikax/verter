//! LEGACY_GATE_SELF — Phase 9 static-grep gate (plan §11.4).
//!
//! Guards against re-introduction of the legacy walker family after
//! the Phase-9 cutover. The walker's outer shim and entire inner
//! body family (`walker_cycle_key_node`,
//! `expand_generic_ref_via_scope_iteration`,
//! and the visited-set helper variant) were deleted, along with the
//! `component_meta_dispatch_iteration` module that hosted the
//! walker's visited-set helper. Plan §6.8 (commit G) deletes the
//! outer shim; commit I finalises the RETIRED_SYMBOLS list to
//! include the outer shim's name.
//!
//! Self-exclusion: the first 5 lines of this file contain
//! `LEGACY_GATE_SELF` so the recursive walk skips this file.

use std::path::PathBuf;

const RETIRED_SYMBOLS: &[&str] = &[
    // Phase 9 cutover (plan §11.2): the inner walker helpers are
    // DELETED. Re-introduction at any site is forbidden.
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
    // Commit E (plan §6.6) — inline-registry-route legacy chain.
    "walk_member_route_via_alias_body",
    "materialize_inline_registry_member_route_from_decl_body",
    "materialize_inline_registry_member_route_if_materializable",
    // Commit D (plan §6.5) — TypeExpr legacy package-ref check (the
    // `_node` graph-native variant is retained).
    "component_meta_ref_resolves_to_package",
    // Commit F (plan §6.7) — TypeExpr legacy cycle walker.
    "decl_body_reaches_cycle_via_walker",
    // Commit G (plan §6.8) — walker shim outer entry.
    "walk_component_meta_member_surface_expr",
    // Commit I (plan §6.10 sub-task 4 / §4.19) — unconditionally
    // retired post-§4.19 deterministic deletion. The composition
    // predicate had zero production callers post-Phase-9 cutover; its
    // sole consumer was a unit test that has also been deleted.
    "registry_member_route_inline_materializable_node",
    // Commit I (plan §6.10 sub-task 4 / §4.19) — `raw_member_path_leaf`
    // was retired in commit E. The shared object-member navigation
    // logic that `explicit_object_member` provided is now inlined
    // into `component_meta_registry_raw_member_path_surface`'s body
    // as the private nested `navigate_object_member` helper.
    "raw_member_path_leaf",
    "explicit_object_member",
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
    //
    // Plan §6.10 sub-task 3 — identifier-boundary matcher: a retired
    // symbol matches ONLY when its occurrence is bounded by characters
    // that can NOT extend an identifier (i.e., not [A-Za-z0-9_]).
    // This prevents false positives like
    // `component_meta_ref_resolves_to_package` matching the kept
    // `_node` variant `component_meta_ref_resolves_to_package_node`,
    // and `walk_component_meta_member_surface_expr` matching the
    // already-retired `_with_visited` variant.
    fn line_contains_identifier(line: &str, ident: &str) -> bool {
        let bytes = line.as_bytes();
        let needle = ident.as_bytes();
        let n = needle.len();
        if n == 0 || bytes.len() < n {
            return false;
        }
        let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut i = 0usize;
        while i + n <= bytes.len() {
            if &bytes[i..i + n] == needle {
                let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
                let after_ok = i + n == bytes.len() || !is_ident_char(bytes[i + n]);
                if before_ok && after_ok {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    for symbol in RETIRED_SYMBOLS {
        let mut hit_files: Vec<(PathBuf, Vec<usize>)> = Vec::new();
        for file in &files {
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            let lines: Vec<usize> = text
                .lines()
                .enumerate()
                .filter_map(|(i, l)| {
                    if line_contains_identifier(l, symbol) {
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
