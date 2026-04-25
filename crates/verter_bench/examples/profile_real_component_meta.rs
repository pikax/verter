#![allow(dead_code)]
#![allow(clippy::cloned_ref_to_slice_refs)]
//! Real-project component-meta hotpath profiler.
//!
//! Usage:
//!   cargo run -p verter_bench --example profile_real_component_meta --release --features=hotpath -- EditorSuggestionMenu
//!   cargo run -p verter_bench --example profile_real_component_meta --release --features=hotpath -- src/runtime/components/ContextMenuContent.vue
//!
//! Environment variables:
//!   VERTER_PROFILE_PROJECT_ROOT  project root (default: .integration-tests/repos/nuxt-ui)
//!   VERTER_PROFILE_REPEATS       request repeats (default: 1)

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use verter_session::{HostConfig, VerterHost};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, ProjectGraph, ViteConfigOptions};

fn normalize_lsp_style_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut normalized = if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_string()
    } else {
        normalized
    };

    if normalized.len() >= 2
        && normalized.as_bytes()[0].is_ascii_uppercase()
        && normalized.as_bytes()[1] == b':'
    {
        normalized.replace_range(0..1, &normalized[0..1].to_ascii_lowercase());
    }

    normalized
}

fn path_to_host_id(path: &Path) -> io::Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(normalize_lsp_style_path(&absolute.to_string_lossy()))
}

fn should_descend(path: &Path, root: &Path) -> bool {
    if path == root {
        return true;
    }
    let Some(name) = path.file_name().and_then(|segment| segment.to_str()) else {
        return true;
    };
    !name.starts_with('.') && name != "node_modules"
}

fn discover_vue_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| should_descend(entry.path(), root))
        .filter_map(|entry| entry.ok())
    {
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "vue") {
            files.push(entry.path().to_path_buf());
        }
    }
    files.sort();
    files
}

fn make_host(project_root: &Path) -> io::Result<VerterHost> {
    let project_root_id = path_to_host_id(project_root)?;
    let ws = FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![project_root_id.clone()],
        ..Default::default()
    });
    let graph_result = ProjectGraph::from_workspace_roots(
        &ws,
        &[project_root_id.clone()],
        &ViteConfigOptions::default(),
    );
    ws.set_project_graph(graph_result.graph);
    Ok(VerterHost::new(
        HostConfig {
            analysis_level: verter_session::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        Arc::new(ws),
    ))
}

fn parse_repeats() -> usize {
    std::env::var("VERTER_PROFILE_REPEATS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn default_project_root() -> PathBuf {
    PathBuf::from("D:/dev/personal/verter/.integration-tests/repos/nuxt-ui")
}

fn resolve_target_file(project_root: &Path, token: &str) -> io::Result<PathBuf> {
    let direct = PathBuf::from(token);
    if direct.exists() {
        return direct.canonicalize();
    }

    let relative = project_root.join(token);
    if relative.exists() {
        return relative.canonicalize();
    }

    let direct_component = project_root
        .join("src")
        .join("runtime")
        .join("components")
        .join(format!("{token}.vue"));
    if direct_component.exists() {
        return direct_component.canonicalize();
    }

    let matches: Vec<PathBuf> =
        discover_vue_files(&project_root.join("src").join("runtime").join("components"))
            .into_iter()
            .filter(|path| {
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case(token))
                    || path.to_string_lossy().replace('\\', "/").contains(token)
            })
            .collect();

    match matches.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("could not resolve component token {token}"),
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "component token {token} is ambiguous: {}",
                matches
                    .iter()
                    .take(5)
                    .map(|path| path.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

fn print_profile(
    run_index: usize,
    target_id: &str,
    analysis: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    resolved: &verter_session::meta_resolve::ResolvedComponentMetaState,
    elapsed: std::time::Duration,
) {
    eprintln!("Run {}:", run_index + 1);
    eprintln!("  target:               {}", target_id);
    eprintln!("  elapsed:              {:?}", elapsed);
    eprintln!(
        "  props/events/slots:   {}/{}/{}",
        analysis.props.len(),
        analysis.events.len(),
        analysis.slots.len()
    );
    eprintln!(
        "  resolved macros/types:{} / {}",
        resolved.resolved_macros.len(),
        resolved.resolved_type_registry.len()
    );
    eprintln!("  fact versions:        {}", resolved.fact_versions.len());
}

fn profile_one(project_root: &PathBuf, token: &str, repeats: usize) -> io::Result<()> {
    let target_file = resolve_target_file(project_root, token)?;
    let target_id = path_to_host_id(&target_file)?;

    let host = make_host(project_root)?;
    let bootstrap_started = Instant::now();
    let _ = host.ensure_loaded(&target_id);
    eprintln!("Real component-meta profiler");
    eprintln!("  root:                 {}", project_root.display());
    eprintln!("  target file:          {}", target_file.display());
    eprintln!("  target id:            {}", target_id);
    eprintln!("  repeats:              {}", repeats);
    eprintln!("  bootstrap:            {:?}", bootstrap_started.elapsed());
    eprintln!();

    for run_index in 0..repeats {
        let started = Instant::now();
        let (analysis, resolved) = host
            .get_component_meta_with_resolution(&target_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "component meta unavailable"))?;
        print_profile(
            run_index,
            &target_id,
            &analysis,
            &resolved,
            started.elapsed(),
        );
        eprintln!();
    }

    Ok(())
}

#[cfg_attr(feature = "hotpath", hotpath::main(limit = 120))]
fn main() -> io::Result<()> {
    let project_root = std::env::var("VERTER_PROFILE_PROJECT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_project_root());
    let targets: Vec<String> = std::env::args().skip(1).collect();
    if targets.is_empty() {
        eprintln!("usage: profile_real_component_meta <component> [<component> ...]");
        std::process::exit(1);
    }
    let repeats = parse_repeats();
    for target in &targets {
        profile_one(&project_root, target, repeats)?;
    }
    Ok(())
}
