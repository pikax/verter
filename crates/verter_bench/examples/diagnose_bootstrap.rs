//! Bisect bootstrap to find which step takes minutes.
//!
//! Times each phase of `ProjectGraph::from_workspace_roots` against a
//! real Vue/Nuxt project. Steps:
//!   1. FilesystemWorkspace::new
//!   2. discover_tsconfigs (glob walk)
//!   3. load_project_membership / load_compiler_options /
//!      load_project_references for each tsconfig
//!   4. analyze_vite_config (if vite_opts.enabled and no tsconfigs)
//!   5. ws.set_project_graph
//!   6. host.ensure_loaded(target)
//!   7. host.get_component_meta_with_resolution(target)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use verter_session::{HostConfig, VerterHost};
use verter_workspace::config::{
    discover_tsconfigs, load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::{
    FilesystemOptions, FilesystemWorkspace, ProjectGraph, ViteConfigOptions, WorkspaceAccess,
};

fn normalize(path: &str) -> String {
    let mut n = path.replace('\\', "/");
    if let Some(rest) = n.strip_prefix("//?/UNC/") {
        n = format!("//{rest}");
    } else if let Some(rest) = n.strip_prefix("//?/") {
        n = rest.to_string();
    }
    if n.len() >= 2 && n.as_bytes()[0].is_ascii_uppercase() && n.as_bytes()[1] == b':' {
        n.replace_range(0..1, &n[0..1].to_ascii_lowercase());
    }
    n
}

fn id_of(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap().join(path)
    };
    normalize(&abs.to_string_lossy())
}

fn main() {
    let project_root = std::env::var("VERTER_AUDIT_PROJECT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Absolute repo-root-anchored default; the corpus is gitignored.
            // Parent-traversal (NOT textual `../..`): downstream host-id /
            // canonicalize-path does not collapse `..`, which would split
            // canonical identity from realpath. `CARGO_MANIFEST_DIR` is
            // always `<repo>/crates/verter_bench`.
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap() // -> <repo>/crates
                .parent()
                .unwrap() // -> <repo>
                .join(".integration-tests/repos/nuxt-ui")
        });
    let target_name = std::env::args().nth(1).unwrap_or_else(|| "Badge".into());

    let project_root_id = id_of(&project_root);
    eprintln!("project_root_id = {}", project_root_id);

    let t0 = Instant::now();
    let ws = FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![project_root_id.clone()],
        ..Default::default()
    });
    eprintln!("[1] FilesystemWorkspace::new          {:?}", t0.elapsed());

    // [2] glob walk for tsconfigs
    let t1 = Instant::now();
    let tsconfig_entries = discover_tsconfigs(Path::new(&project_root_id));
    eprintln!(
        "[2] discover_tsconfigs                 {:?} ({} entries)",
        t1.elapsed(),
        tsconfig_entries.len()
    );
    for entry in &tsconfig_entries {
        eprintln!("      • {}", entry.path);
    }

    // [3] per-tsconfig loaders
    for (i, entry) in tsconfig_entries.iter().enumerate() {
        let t = Instant::now();
        let _ = load_project_membership(&ws, &entry.path);
        eprintln!(
            "[3.{i}.a] load_project_membership({:?}) {:?}",
            entry.path,
            t.elapsed()
        );
        let t = Instant::now();
        let _ = load_compiler_options(&ws, &entry.path);
        eprintln!("[3.{i}.b] load_compiler_options          {:?}", t.elapsed());
        let t = Instant::now();
        let _ = load_project_references(&ws, &entry.path);
        eprintln!("[3.{i}.c] load_project_references        {:?}", t.elapsed());
    }

    // [4] full ProjectGraph::from_workspace_roots
    let t4 = Instant::now();
    let graph_result = ProjectGraph::from_workspace_roots(
        &ws,
        std::slice::from_ref(&project_root_id),
        &ViteConfigOptions::default(),
    );
    eprintln!(
        "[4] ProjectGraph::from_workspace_roots {:?}  ({} trust_required)",
        t4.elapsed(),
        graph_result.trust_required.len()
    );

    // [5] set_project_graph
    let t5 = Instant::now();
    ws.set_project_graph(graph_result.graph);
    eprintln!("[5] set_project_graph                 {:?}", t5.elapsed());

    // [6] VerterHost::new
    let t6 = Instant::now();
    let ws_access: Arc<dyn WorkspaceAccess> = Arc::new(ws);
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws_access));
    eprintln!("[6] VerterHost::new                   {:?}", t6.elapsed());

    // [7] ensure_loaded
    let target_path = project_root
        .join("src/runtime/components")
        .join(format!("{target_name}.vue"));
    let target_id = id_of(&target_path);
    eprintln!("target_id = {}", target_id);
    let t7 = Instant::now();
    let loaded = host.ensure_loaded(&target_id);
    eprintln!(
        "[7] ensure_loaded({target_name})          {:?} -> {}",
        t7.elapsed(),
        loaded
    );

    // [8] component-meta
    let t8 = Instant::now();
    let result = host.get_component_meta_with_resolution(&target_id);
    eprintln!(
        "[8] get_component_meta_with_resolution {:?} -> {:?}",
        t8.elapsed(),
        result.is_some()
    );

    eprintln!("TOTAL                                 {:?}", t0.elapsed());
}
