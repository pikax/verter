//! Builds `verter_semantic::analysis::RouteAnalysisInputs` from a live
//! `WorkspaceRead` — the caller-side half of the snapshot conversion.
//!
//! `analysis/routes.rs`'s six route-analysis functions took `&dyn
//! verter_workspace::WorkspaceRead` before this conversion; they now take
//! `&RouteAnalysisInputs`, an immutable snapshot with no handle. Snapshot
//! construction belongs to the higher-level LSP/MCP orchestration callers,
//! so the WALK that builds that snapshot
//! lives HERE — `verter_session` is the shared host-backed loading
//! authority both `verter_lsp` and `verter_mcp` already depend on
//! (CLAUDE.md: "`verter_session` ... is the authority for host-backed
//! loading"), so this is the one place that walk needs to exist, not
//! duplicated per consumer crate.
//!
//! Mirrors exactly the directory/file set `build_route_analysis`'s own
//! internal branching will consult: `package.json` (framework detection),
//! the pages directory the DETECTED framework selects (recursively), the
//! `layouts/` directory (recursively, unconditionally — `discover_layouts`
//! runs regardless of framework), and the fixed router-config candidate
//! paths (`ROUTER_CONFIG_CANDIDATES`, probed for existence, read if
//! present). A directory/file never captured here answers `is_dir ==
//! false` / `read_file == None` on the `RouteAnalysisInputs` side, which
//! is the desired collapse: route analysis has no error-vs-absent
//! distinction to preserve (unlike module resolution's witnessed
//! `PathProbe`).

use verter_semantic::analysis::{
    detect_routing_framework, RouteAnalysisInputs, RouteDirEntry, RoutingFramework,
    ROUTER_CONFIG_CANDIDATES,
};

/// Builds the complete `RouteAnalysisInputs` snapshot for `project_root`.
#[must_use]
pub fn build_route_analysis_inputs(
    workspace: &dyn verter_workspace::WorkspaceRead,
    project_root: &str,
) -> RouteAnalysisInputs {
    let mut inputs = RouteAnalysisInputs::new();
    let trimmed_root = project_root.trim_end_matches('/');

    // package.json — framework detection's own input.
    let pkg_path = format!("{trimmed_root}/package.json");
    if let Some(content) = workspace.read_file(&pkg_path) {
        inputs.insert_file(pkg_path, content);
    }

    // Framework detection itself is pure — run it against the snapshot
    // we've captured so far, exactly mirroring what `build_route_analysis`
    // will do with the SAME inputs later.
    let framework = detect_routing_framework(&inputs, project_root);

    // Pages directory — mirrors `build_route_analysis`'s own branching
    // (`crates/verter_semantic/src/analysis/routes.rs`).
    match framework {
        RoutingFramework::NuxtPages => {
            walk_dir_recursive(workspace, &format!("{trimmed_root}/pages"), &mut inputs);
        }
        RoutingFramework::UnpluginVueRouter => {
            let src_pages = format!("{trimmed_root}/src/pages");
            if workspace.is_dir(&src_pages) {
                walk_dir_recursive(workspace, &src_pages, &mut inputs);
            } else {
                walk_dir_recursive(workspace, &format!("{trimmed_root}/pages"), &mut inputs);
            }
        }
        RoutingFramework::VueRouter | RoutingFramework::Unknown => {
            // Programmatic route extraction reads only the router-config
            // candidates below, not a pages directory.
        }
    }

    // layouts/ — `discover_layouts` runs unconditionally, independent of
    // the detected framework.
    walk_dir_recursive(workspace, &format!("{trimmed_root}/layouts"), &mut inputs);

    // Router config candidates — fixed, known paths; probe existence and
    // capture content for any that are present.
    for candidate in ROUTER_CONFIG_CANDIDATES {
        let path = format!("{trimmed_root}/{candidate}");
        if let Some(content) = workspace.read_file(&path) {
            inputs.insert_file(path, content);
        }
    }

    inputs
}

/// Recursively walks `dir` via `workspace`, capturing every directory's
/// listing and every file's content into `inputs`. A `dir` that is not a
/// directory (absent, or a file) captures nothing — matching
/// `RouteAnalysisInputs::is_dir`'s "never inserted = does not exist" fold.
fn walk_dir_recursive(
    workspace: &dyn verter_workspace::WorkspaceRead,
    dir: &str,
    inputs: &mut RouteAnalysisInputs,
) {
    if !workspace.is_dir(dir) {
        return;
    }
    let Ok(entries) = workspace.read_dir(dir) else {
        return;
    };
    // One-way projection from the live VFS row onto the dependency-neutral
    // route-analysis input IR — `verter_semantic` never names the workspace
    // `DirEntry` type; this is the caller-side boundary that performs the
    // mapping.
    let projected: Vec<RouteDirEntry> = entries
        .iter()
        .map(|entry| RouteDirEntry {
            path: entry.path.clone(),
            is_dir: entry.is_dir,
        })
        .collect();
    inputs.insert_directory(dir.to_string(), projected);
    for entry in &entries {
        if entry.is_dir {
            walk_dir_recursive(workspace, &entry.path, inputs);
        } else if let Some(content) = workspace.read_file(&entry.path) {
            inputs.insert_file(entry.path.clone(), content);
        }
    }
}

#[cfg(test)]
#[path = "route_analysis_inputs_tests.rs"]
mod route_analysis_inputs_tests;
