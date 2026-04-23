#![allow(dead_code)]
#![allow(clippy::cloned_ref_to_slice_refs)]
//! Self-contained component-meta hotpath profiler.
//!
//! Generates a synthetic Vue project with local node_modules packages that
//! mimic the slow imported-type path:
//! - bare package imports resolved through package.json
//! - `.d.ts` companions preferred over runtime `.js`
//! - deep re-export chains
//! - large imported interfaces
//! - local generic wrappers around imported utility-heavy types
//! - local `typeof` and double `<script>` visibility in the target SFC
//!
//! Usage:
//!   cargo run -p verter_bench --example profile_component_meta --release --features=hotpath
//!
//! Environment variables:
//!   VERTER_META_SYNTHETIC_LAYERS           default 320
//!   VERTER_META_SYNTHETIC_FIELDS_PER_LAYER default 32
//!   VERTER_META_SYNTHETIC_REEXPORT_HOPS    default 24
//!   VERTER_META_SYNTHETIC_EXTRA_VUE_FILES  default 256
//!   VERTER_META_PROFILE_BOOTSTRAP          selective|eager (default selective)
//!   VERTER_META_PROFILE_REPEATS            default 2
//!   VERTER_META_PROFILE_INCLUDE_RESOLVE    0|1 (default 1)
//!   VERTER_META_DEBUG=1                    enables native per-dependency debug

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use verter_semantic::analysis::AnalyzedMacroKind;
use verter_session::{FileAnalysisSnapshot, HostConfig, VerterHost};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, ProjectGraph, ViteConfigOptions};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BootstrapMode {
    Selective,
    Eager,
}

#[derive(Clone, Debug)]
struct ScenarioConfig {
    layers: usize,
    fields_per_layer: usize,
    reexport_hops: usize,
    extra_vue_files: usize,
    repeats: usize,
    include_resolve_imported: bool,
    bootstrap_mode: BootstrapMode,
}

#[derive(Debug)]
struct SyntheticProject {
    _tempdir: tempfile::TempDir,
    root: PathBuf,
    target_id: String,
}

#[derive(Clone, Debug, Default)]
struct SnapshotCounts {
    props: usize,
    emits: usize,
    slots: usize,
    resolved_local_types: usize,
    macro_type_deps: usize,
    prop_names: Vec<String>,
    emit_names: Vec<String>,
    slot_names: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct EvaluatedCounts {
    props: usize,
    emits: usize,
    slot_bindings: usize,
    prop_names: Vec<String>,
}

#[derive(Debug)]
struct RequestProfile {
    analysis_elapsed: Duration,
    resolve_imported_elapsed: Option<Duration>,
    evaluate_elapsed: Duration,
    resolved_imported_types: usize,
    analysis_counts: Option<SnapshotCounts>,
    evaluated_counts: Option<EvaluatedCounts>,
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"),
        Err(_) => default,
    }
}

fn bootstrap_mode_from_env() -> BootstrapMode {
    match std::env::var("VERTER_META_PROFILE_BOOTSTRAP")
        .unwrap_or_else(|_| "selective".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "eager" => BootstrapMode::Eager,
        _ => BootstrapMode::Selective,
    }
}

fn scenario_from_env() -> ScenarioConfig {
    ScenarioConfig {
        layers: env_usize("VERTER_META_SYNTHETIC_LAYERS", 320),
        fields_per_layer: env_usize("VERTER_META_SYNTHETIC_FIELDS_PER_LAYER", 32),
        reexport_hops: env_usize("VERTER_META_SYNTHETIC_REEXPORT_HOPS", 24),
        extra_vue_files: env_usize("VERTER_META_SYNTHETIC_EXTRA_VUE_FILES", 256),
        repeats: env_usize("VERTER_META_PROFILE_REPEATS", 2),
        include_resolve_imported: env_bool("VERTER_META_PROFILE_INCLUDE_RESOLVE", true),
        bootstrap_mode: bootstrap_mode_from_env(),
    }
}

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

fn write_text(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

fn layer_interface(
    name: &str,
    extends_name: Option<&str>,
    layer: usize,
    fields_per_layer: usize,
) -> String {
    let mut body = String::new();
    if let Some(base) = extends_name {
        body.push_str(&format!("export interface {name} extends {base} {{\n"));
    } else {
        body.push_str(&format!("export interface {name} {{\n"));
    }
    for field in 0..fields_per_layer {
        body.push_str(&format!("  field_{layer}_{field}?: string\n"));
    }
    body.push_str("}\n");
    body
}

fn build_layer_block(type_prefix: &str, layers: usize, fields_per_layer: usize) -> String {
    let mut block = String::new();
    for layer in 0..layers {
        let current_type = format!("{type_prefix}{layer}");
        let extends_name = layer
            .checked_sub(1)
            .map(|prev| format!("{type_prefix}{prev}"));
        block.push_str(&layer_interface(
            &current_type,
            extends_name.as_deref(),
            layer,
            fields_per_layer,
        ));
        block.push('\n');
    }
    block
}

fn write_reka_ui_package(root: &Path, config: &ScenarioConfig) -> io::Result<()> {
    let package_root = root.join("node_modules/reka-ui");
    let dist_dir = package_root.join("dist");
    std::fs::create_dir_all(&dist_dir)?;

    write_text(
        &package_root.join("package.json"),
        r#"{
  "name": "reka-ui",
  "type": "module",
  "types": "./dist/index.d.ts",
  "exports": {
    ".": {
      "types": "./dist/index.d.ts",
      "import": "./dist/index.js",
      "default": "./dist/index.js"
    }
  }
}
"#,
    )?;

    let mut index_dts = build_layer_block("AccordionLayer", config.layers, config.fields_per_layer);
    index_dts.push_str(&format!(
        "export interface AccordionRootProps extends AccordionLayer{} {{\n\
  dir?: 'ltr' | 'rtl'\n\
  orientation?: 'horizontal' | 'vertical'\n\
  disabled?: boolean\n\
  collapsible?: boolean\n\
}}\n\n\
export interface AccordionRootEmits {{\n\
  (event: 'openChange', value: boolean): void\n\
  (event: 'close'): void\n\
}}\n",
        config.layers.saturating_sub(1),
    ));
    for hop in 0..config.reexport_hops {
        index_dts.push_str(&format!(
            "export type AccordionRootPropsHop{hop} = AccordionRootProps\nexport type AccordionRootEmitsHop{hop} = AccordionRootEmits\n"
        ));
    }
    write_text(&dist_dir.join("index.d.ts"), &index_dts)?;
    write_text(
        &dist_dir.join("index.js"),
        "export function AccordionRoot() { return null }\n",
    )?;
    Ok(())
}

fn write_nuxt_schema_package(root: &Path, config: &ScenarioConfig) -> io::Result<()> {
    let package_root = root.join("node_modules/@nuxt/schema");
    let dist_dir = package_root.join("dist");
    std::fs::create_dir_all(&dist_dir)?;

    write_text(
        &package_root.join("package.json"),
        r#"{
  "name": "@nuxt/schema",
  "type": "module",
  "types": "./dist/index.d.ts",
  "exports": {
    ".": {
      "types": "./dist/index.d.ts",
      "import": "./dist/index.js",
      "default": "./dist/index.js"
    }
  }
}
"#,
    )?;

    let mut index_dts = build_layer_block("AppConfigLayer", config.layers, config.fields_per_layer);
    index_dts.push_str(&format!(
        "export interface AppConfig extends AppConfigLayer{} {{\n\
  accordion?: {{\n\
    density?: 'compact' | 'comfortable'\n\
    animation?: boolean\n\
  }}\n\
}}\n",
        config.layers.saturating_sub(1),
    ));
    for hop in 0..config.reexport_hops {
        index_dts.push_str(&format!("export type AppConfigHop{hop} = AppConfig\n"));
    }
    write_text(&dist_dir.join("index.d.ts"), &index_dts)?;
    write_text(&dist_dir.join("index.js"), "export const appConfig = {}\n")?;
    Ok(())
}

fn write_target_component(root: &Path) -> io::Result<PathBuf> {
    let target = root.join("src/runtime/components/Accordion.vue");
    let source = r#"<script lang="ts">
const localUi = {
  header: "header",
  content: "content",
  item: "item",
} as const

type SelectRoot<T> = Pick<T, "dir" | "orientation" | "disabled">

interface AccordionSlots {
  default?: (props: { open: boolean; localUi: typeof localUi }) => any
}
</script>

<script setup lang="ts">
import type { AccordionRootProps, AccordionRootEmits } from "reka-ui"
import type { AppConfig } from "@nuxt/schema"

defineProps<SelectRoot<AccordionRootProps> & {
  appConfig?: AppConfig
  localUi: typeof localUi
  tone?: "neutral" | "brand"
}>()

defineEmits<AccordionRootEmits>()
defineSlots<AccordionSlots>()
</script>

<template>
  <div />
</template>
"#;
    write_text(&target, source)?;
    Ok(target)
}

fn write_extra_vue_files(root: &Path, count: usize) -> io::Result<()> {
    let extra_dir = root.join("src/runtime/components/generated");
    std::fs::create_dir_all(&extra_dir)?;
    for index in 0..count {
        let source = format!(
            "<script setup lang=\"ts\">\n\
defineProps<{{ label: string; index: number }}>()\n\
</script>\n\
<template><div>{{{{ label }}}}-{} </div></template>\n",
            index
        );
        write_text(&extra_dir.join(format!("Generated{index}.vue")), &source)?;
    }
    Ok(())
}

fn generate_synthetic_project(config: &ScenarioConfig) -> io::Result<SyntheticProject> {
    let tempdir = tempfile::tempdir()?;
    let root = tempdir.path().join("component-meta-hotpath");
    std::fs::create_dir_all(&root)?;

    write_text(
        &root.join("package.json"),
        r#"{
  "name": "component-meta-hotpath",
  "private": true,
  "type": "module"
}
"#,
    )?;
    write_text(
        &root.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "strict": true,
    "module": "ESNext",
    "target": "ESNext",
    "moduleResolution": "Bundler"
  },
  "include": ["src/**/*.vue", "src/**/*.ts"]
}
"#,
    )?;

    write_reka_ui_package(&root, config)?;
    write_nuxt_schema_package(&root, config)?;
    write_extra_vue_files(&root, config.extra_vue_files)?;
    let target = write_target_component(&root)?;

    Ok(SyntheticProject {
        _tempdir: tempdir,
        root,
        target_id: path_to_host_id(&target)?,
    })
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

fn discover_vue_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| should_descend(entry.path(), root))
        .filter_map(|entry| entry.ok())
    {
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "vue") {
            if let Ok(id) = path_to_host_id(entry.path()) {
                files.push(id);
            }
        }
    }
    files.sort();
    files
}

fn make_host(project_root: &Path) -> io::Result<VerterHost> {
    let project_root_id = path_to_host_id(project_root)?;
    let tsconfig_id = path_to_host_id(&project_root.join("tsconfig.json"))?;
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
    let host = VerterHost::new(
        HostConfig {
            analysis_level: verter_session::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        Arc::new(ws),
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            project_root_id.clone(),
            project_root_id,
            Some(tsconfig_id),
        ),
    ]);
    Ok(host)
}

fn bootstrap_project(
    host: &VerterHost,
    project: &SyntheticProject,
    mode: BootstrapMode,
) -> Vec<String> {
    match mode {
        BootstrapMode::Selective => {
            let _ = host.ensure_loaded(&project.target_id);
            vec![project.target_id.clone()]
        }
        BootstrapMode::Eager => {
            let discovered = discover_vue_files(&project.root);
            for file_id in &discovered {
                let _ = host.ensure_loaded(file_id);
            }
            discovered
        }
    }
}

fn collect_snapshot_counts(snapshot: &FileAnalysisSnapshot) -> SnapshotCounts {
    let mut prop_names = BTreeSet::new();
    let mut emit_names = BTreeSet::new();
    let mut slot_names = BTreeSet::new();
    let mut counts = SnapshotCounts {
        macro_type_deps: snapshot.macro_type_deps.len(),
        ..SnapshotCounts::default()
    };

    for mac in snapshot.macros.iter() {
        counts.resolved_local_types += mac.resolved_local_types.len();
        match mac.kind {
            AnalyzedMacroKind::DefineProps => {
                counts.props += mac.prop_fields.len();
                for field in &mac.prop_fields {
                    prop_names.insert(field.name.clone());
                }
            }
            AnalyzedMacroKind::DefineEmits => {
                counts.emits += mac.emit_fields.len();
                for field in &mac.emit_fields {
                    emit_names.insert(field.name.clone());
                }
            }
            AnalyzedMacroKind::DefineSlots => {
                counts.slots += mac.slot_fields.len();
                for field in &mac.slot_fields {
                    slot_names.insert(field.name.clone());
                }
            }
            _ => {}
        }
    }

    counts.prop_names = prop_names.into_iter().collect();
    counts.emit_names = emit_names.into_iter().collect();
    counts.slot_names = slot_names.into_iter().collect();
    counts
}

fn collect_evaluated_counts(
    evaluated: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
) -> EvaluatedCounts {
    let mut prop_names = BTreeSet::new();
    for field in &evaluated.props {
        prop_names.insert(field.name.clone());
    }
    EvaluatedCounts {
        props: evaluated.props.len(),
        emits: evaluated.emits.len(),
        slot_bindings: evaluated.slot_bindings.len(),
        prop_names: prop_names.into_iter().collect(),
    }
}

fn run_component_meta_request(
    host: &VerterHost,
    target_id: &str,
    include_resolve_imported: bool,
) -> RequestProfile {
    let analysis_started = Instant::now();
    let analysis = host.get_analysis(target_id);
    let analysis_elapsed = analysis_started.elapsed();

    let (resolve_imported_elapsed, resolved_imported_types) = if include_resolve_imported {
        let resolve_started = Instant::now();
        let resolved =
            host.resolve_component_meta(target_id, verter_session::ProjectionMode::Expanded);
        let resolved_count = resolved
            .map(|state| state.resolved_macros.len())
            .unwrap_or(0);
        (Some(resolve_started.elapsed()), resolved_count)
    } else {
        (None, 0)
    };

    let evaluate_started = Instant::now();
    let evaluated = host.evaluate_types(target_id);
    let evaluate_elapsed = evaluate_started.elapsed();

    RequestProfile {
        analysis_elapsed,
        resolve_imported_elapsed,
        evaluate_elapsed,
        resolved_imported_types,
        analysis_counts: analysis.as_ref().map(collect_snapshot_counts),
        evaluated_counts: evaluated.as_ref().map(collect_evaluated_counts),
    }
}

fn format_presence(names: &[String], expected: &[&str]) -> String {
    let name_set: BTreeSet<&str> = names.iter().map(|name| name.as_str()).collect();
    expected
        .iter()
        .map(|name| {
            format!(
                "{name}={}",
                if name_set.contains(name) { "yes" } else { "no" }
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn print_profile(run_index: usize, profile: &RequestProfile) {
    eprintln!("Request {}:", run_index + 1);
    eprintln!("  get_analysis:         {:?}", profile.analysis_elapsed);
    if let Some(resolve_elapsed) = profile.resolve_imported_elapsed {
        eprintln!(
            "  resolve_imported:     {:?} ({} types)",
            resolve_elapsed, profile.resolved_imported_types
        );
    }
    eprintln!("  evaluate_types:       {:?}", profile.evaluate_elapsed);

    if let Some(analysis_counts) = &profile.analysis_counts {
        eprintln!(
            "  analysis counts:      props={} emits={} slots={} macro_deps={} resolved_local_types={}",
            analysis_counts.props,
            analysis_counts.emits,
            analysis_counts.slots,
            analysis_counts.macro_type_deps,
            analysis_counts.resolved_local_types,
        );
        eprintln!(
            "  analysis presence:    {}",
            format_presence(
                &analysis_counts.prop_names,
                &[
                    "dir",
                    "orientation",
                    "disabled",
                    "appConfig",
                    "localUi",
                    "tone"
                ],
            )
        );
    } else {
        eprintln!("  analysis counts:      <missing>");
    }

    if let Some(evaluated_counts) = &profile.evaluated_counts {
        eprintln!(
            "  evaluated counts:     props={} emits={} slot_bindings={}",
            evaluated_counts.props, evaluated_counts.emits, evaluated_counts.slot_bindings,
        );
        eprintln!(
            "  evaluated presence:   {}",
            format_presence(
                &evaluated_counts.prop_names,
                &[
                    "dir",
                    "orientation",
                    "disabled",
                    "appConfig",
                    "localUi",
                    "tone"
                ],
            )
        );
    } else {
        eprintln!("  evaluated counts:     <missing>");
    }
}

#[cfg_attr(feature = "hotpath", hotpath::main(limit = 80))]
fn main() {
    let config = scenario_from_env();
    let project = generate_synthetic_project(&config).expect("synthetic project should be created");
    let host = make_host(&project.root).expect("host should be created");

    eprintln!("Synthetic component-meta profiler");
    eprintln!("  root:                 {}", project.root.display());
    eprintln!("  target:               {}", project.target_id);
    eprintln!("  layers:               {}", config.layers);
    eprintln!("  fields/layer:         {}", config.fields_per_layer);
    eprintln!("  reexport hops:        {}", config.reexport_hops);
    eprintln!("  extra vue files:      {}", config.extra_vue_files);
    eprintln!(
        "  bootstrap mode:       {}",
        match config.bootstrap_mode {
            BootstrapMode::Selective => "selective",
            BootstrapMode::Eager => "eager",
        }
    );
    eprintln!(
        "  compat-like sequence: get_analysis{}evaluate_types",
        if config.include_resolve_imported {
            " -> resolve_component_meta(expanded) -> "
        } else {
            " -> "
        }
    );
    eprintln!("  note:                 set VERTER_META_DEBUG=1 for native dependency timings");
    eprintln!();

    let bootstrap_started = Instant::now();
    let loaded_files = bootstrap_project(&host, &project, config.bootstrap_mode);
    eprintln!("Bootstrap:");
    eprintln!(
        "  loaded files:         {} in {:?}",
        loaded_files.len(),
        bootstrap_started.elapsed()
    );
    eprintln!();

    for run_index in 0..config.repeats {
        let profile =
            run_component_meta_request(&host, &project.target_id, config.include_resolve_imported);
        print_profile(run_index, &profile);
        eprintln!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_vue_files_skips_hidden_and_node_modules_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        write_text(&root.join("src/App.vue"), "<template />").unwrap();
        write_text(&root.join("src/components/Button.vue"), "<template />").unwrap();
        write_text(&root.join(".nuxt/generated/Hidden.vue"), "<template />").unwrap();
        write_text(
            &root.join("node_modules/example-package/ShouldNotLoad.vue"),
            "<template />",
        )
        .unwrap();

        let discovered = discover_vue_files(root);
        assert_eq!(discovered.len(), 2);
    }

    #[test]
    fn synthetic_component_meta_request_smoke_test_runs() {
        let config = ScenarioConfig {
            layers: 3,
            fields_per_layer: 2,
            reexport_hops: 2,
            extra_vue_files: 4,
            repeats: 1,
            include_resolve_imported: true,
            bootstrap_mode: BootstrapMode::Selective,
        };
        let project = generate_synthetic_project(&config).unwrap();
        let host = make_host(&project.root).unwrap();

        let loaded = bootstrap_project(&host, &project, config.bootstrap_mode);
        assert_eq!(loaded.len(), 1);

        let profile =
            run_component_meta_request(&host, &project.target_id, config.include_resolve_imported);

        assert!(profile.analysis_counts.is_some());
        assert!(profile.evaluated_counts.is_some());
        assert!(profile.resolved_imported_types <= 3);
    }
}
