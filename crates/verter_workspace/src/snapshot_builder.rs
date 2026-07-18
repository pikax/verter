//! Builder for [`WorkspaceSnapshot`]: one tsconfig discovery pass,
//! exact membership materialization, atomic snapshot construction.
//!
//! The builder runs off-thread (spawn_blocking) and produces a complete
//! immutable snapshot before publication.

use rustc_hash::FxHashSet;

use crate::canonical_path::CanonicalPath;
use crate::membership::{
    typescript_default_excludes, ConfiguredMembership, FallbackMembership, StaticMembershipSpec,
    SupportedExtensions,
};
use crate::resolver::{IdeProjectConfig, ProjectMembership, ProjectResolver};
use crate::workspace_snapshot::{
    compare_project_precedence, OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration,
    WorkspaceSnapshot,
};

/// Result of building a workspace snapshot from workspace roots.
#[cfg(not(target_arch = "wasm32"))]
pub struct SnapshotBuildResult {
    pub snapshot: WorkspaceSnapshot,
    /// Vite configs that need user trust approval.
    pub trust_required: Vec<crate::vite_config::ViteConfigTrustInfo>,
}

/// Build a [`WorkspaceSnapshot`] from workspace roots.
///
/// Discovers tsconfigs, loads membership/compiler options, materializes
/// exact configured file sets, creates fallback projects, sorts by
/// precedence, and builds the resolver.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_workspace_snapshot(
    ws: &dyn crate::traits::WorkspaceAccess,
    roots: &[String],
    generation: SnapshotGeneration,
    vite_opts: &crate::vite_config::ViteConfigOptions,
) -> SnapshotBuildResult {
    use crate::config::{
        discover_tsconfigs, is_project_config, load_compiler_options, load_project_membership,
        load_project_references,
    };
    use crate::vite_config::{analyze_vite_config, ViteConfigAnalysis, ViteConfigTrustInfo};
    use std::path::PathBuf;

    let mut projects = Vec::new();
    let mut trust_required = Vec::new();
    let mut next_id: u32 = 0;

    for root_str in roots {
        let canonical_root = CanonicalPath::new(root_str);
        let root_path = PathBuf::from(canonical_root.as_str());

        // ── Discover tsconfigs ──
        let tsconfig_entries = discover_tsconfigs(&root_path);

        // Pre-pass: collect every config that is the resolved target of a
        // TypeScript `references[].path` from any discovered config. Such
        // targets are project configs even if their own JSON declares no
        // files/include/exclude. `load_project_references` returns already
        // resolved+normalized paths; key the set by `normalize_canonical_id`
        // so `is_project_config`'s lookup matches.
        let reference_targets: FxHashSet<String> = tsconfig_entries
            .iter()
            .flat_map(|entry| load_project_references(ws, &entry.path))
            .map(|target| crate::resolver::normalize_canonical_id(&target))
            .collect();

        for entry in &tsconfig_entries {
            // Only configs with structural file-ownership intent (or default
            // name / reference targeting) become owning projects. Parse-only
            // `extends` fragments (e.g. a compilerOptions-only
            // `tsconfig.base.json`) are still parsed for inheritance through
            // `extends`, but never registered as file owners.
            if !is_project_config(ws, &entry.path, &reference_targets) {
                continue;
            }

            let tsconfig_path = CanonicalPath::new(&entry.path);
            let project_root = CanonicalPath::new(&entry.root);
            let raw_membership = load_project_membership(ws, &entry.path);
            let compiler_options = load_compiler_options(ws, &entry.path);
            let raw_references = load_project_references(ws, &entry.path);

            let supported = supported_extensions_for(&compiler_options);
            let spec = membership_to_spec(&project_root, &raw_membership, &supported);
            let materialized_files = materialize_from_spec(&spec, &project_root, Some(ws));

            let references = raw_references
                .into_iter()
                .map(|r| CanonicalPath::new(&r))
                .collect();

            let id = ProjectId(next_id);
            next_id += 1;

            projects.push(OwnershipProject {
                id,
                root: project_root,
                workspace_root: canonical_root.clone(),
                payload: ProjectPayload::Configured {
                    tsconfig_path,
                    membership: ConfiguredMembership {
                        spec,
                        materialized_files,
                    },
                    compiler_options,
                    references,
                    workspace_aliases: Vec::new(),
                },
            });
        }

        // ── Fallback project ──
        let has_tsconfigs = !tsconfig_entries.is_empty();

        // Collect trust-required notifications for complex vite configs.
        // Alias resolution is handled by the VFS ProjectGraph's
        // `from_workspace_roots` which stores aliases on the fallback project.
        if vite_opts.enabled && !has_tsconfigs {
            if let ViteConfigAnalysis::Complex {
                config_path,
                reason,
            } = analyze_vite_config(ws, canonical_root.as_str())
            {
                let is_trusted = crate::vite_config::vite_config_is_trusted(
                    &vite_opts.trusted_files,
                    &config_path,
                );
                if !is_trusted {
                    trust_required.push(ViteConfigTrustInfo {
                        config_path,
                        workspace_root: canonical_root.as_str().to_string(),
                        reason,
                    });
                }
            }
        }

        let id = ProjectId(next_id);
        next_id += 1;

        projects.push(OwnershipProject {
            id,
            root: canonical_root.clone(),
            workspace_root: canonical_root.clone(),
            payload: ProjectPayload::Fallback {
                membership: FallbackMembership {
                    root: canonical_root,
                    exclude: typescript_default_excludes(&CanonicalPath::new(root_str)),
                },
            },
        });
    }

    // ── Sort by precedence ──
    projects.sort_by(compare_project_precedence);

    // Re-assign IDs after sorting (IDs must match index position)
    for (i, project) in projects.iter_mut().enumerate() {
        project.id = ProjectId(i as u32);
    }

    let resolver = build_resolver_from_projects(&projects);

    SnapshotBuildResult {
        snapshot: WorkspaceSnapshot {
            owners_memo: Default::default(),
            projects,
            resolver,
            generation,
        },
        trust_required,
    }
}

/// Build a workspace snapshot for tests/WASM (no vite, no disk).
pub fn build_workspace_snapshot_simple(
    projects: Vec<OwnershipProject>,
    generation: SnapshotGeneration,
) -> WorkspaceSnapshot {
    let mut projects = projects;
    projects.sort_by(compare_project_precedence);

    for (i, project) in projects.iter_mut().enumerate() {
        project.id = ProjectId(i as u32);
    }

    let resolver = build_resolver_from_projects(&projects);

    WorkspaceSnapshot {
        owners_memo: Default::default(),
        projects,
        resolver,
        generation,
    }
}

/// The carrier extensions the live `LanguageRegistry` registers (`.vue`,
/// `.svelte`, …) WITHOUT a leading dot, the framework-agnostic authority for
/// `extraFileExtensions`-style membership. Never a hardcoded list.
pub fn registry_carrier_extensions() -> Vec<String> {
    verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .iter()
        .map(|e| (*e).to_string())
        .collect()
}

/// Build the supported-extension set for a configured project from its parsed
/// compiler options (for the `allowJs`/`checkJs` JS-family gate) and the
/// registered carrier extensions.
pub fn supported_extensions_for(
    compiler_options: &crate::resolver::IdeProjectCompilerOptions,
) -> SupportedExtensions {
    SupportedExtensions::new(
        compiler_options.js_is_member(),
        &registry_carrier_extensions(),
    )
}

/// Convert the old `ProjectMembership` enum to the new `StaticMembershipSpec`,
/// applying the supported-extension expansion rule.
///
/// Fills in TypeScript defaults when the old representation was `MatchAll`. A
/// no-extension directory / bare-star include glob expands into one glob per
/// supported extension; an extension-specific glob is kept verbatim; `files`
/// are exact and immune; `exclude` is literal.
pub fn membership_to_spec(
    project_root: &CanonicalPath,
    membership: &ProjectMembership,
    supported: &SupportedExtensions,
) -> StaticMembershipSpec {
    match membership {
        ProjectMembership::MatchAll => {
            StaticMembershipSpec::with_supported_extension_defaults(project_root, supported)
        }
        ProjectMembership::IncludeExclude {
            files,
            include,
            exclude,
        } => {
            let files_refs: Vec<&str> = files.iter().map(String::as_str).collect();
            let include_refs: Vec<&str> = include.iter().map(String::as_str).collect();
            if exclude.is_empty() {
                // No explicit exclude → TS defaults (node_modules etc.).
                let mut spec = StaticMembershipSpec::from_includes(
                    project_root,
                    &files_refs,
                    &include_refs,
                    &[],
                    supported,
                );
                spec.exclude = typescript_default_excludes(project_root);
                spec
            } else {
                let exclude_refs: Vec<&str> = exclude.iter().map(String::as_str).collect();
                StaticMembershipSpec::from_includes(
                    project_root,
                    &files_refs,
                    &include_refs,
                    &exclude_refs,
                    supported,
                )
            }
        }
    }
}

/// Build the exact [`ConfiguredMembership`] from the raw parsed
/// `files`/`include`/`exclude` membership, applying the supported-extension
/// expansion. The materialized set is left empty (bridge mode — `contains`
/// falls to the static spec), for callers with no filesystem walk (the legacy
/// `ProjectGraph` path and resolver-config tests). One membership
/// representation, no second glob evaluator.
pub fn configured_membership_from_raw(
    root: &str,
    membership: &ProjectMembership,
    compiler_options: &crate::resolver::IdeProjectCompilerOptions,
) -> ConfiguredMembership {
    let supported = supported_extensions_for(compiler_options);
    let spec = membership_to_spec(&CanonicalPath::new(root), membership, &supported);
    ConfiguredMembership {
        spec,
        materialized_files: FxHashSet::default(),
    }
}

/// Materialize the configured file set from a static membership spec.
///
/// When `ws` is `Some` and the workspace supports `walk()`, walks the
/// project root directory to expand `include` patterns (minus `exclude`).
/// `files` entries are always included — they are immune to `exclude`.
///
/// When `ws` is `None` or `walk()` returns `UnsupportedOperation` (a
/// filesystem-less environment — the in-memory Engine path or WASM), only
/// `files` entries are materialized; `ConfiguredMembership::contains` then
/// decides the rest through the compiled spec globs.
fn materialize_from_spec(
    spec: &StaticMembershipSpec,
    project_root: &CanonicalPath,
    ws: Option<&dyn crate::traits::WorkspaceAccess>,
) -> FxHashSet<CanonicalPath> {
    let mut result = FxHashSet::default();

    // files entries are always members (immune to exclude)
    for file in &spec.files {
        result.insert(file.clone());
    }

    // Walk the project root and check each file against include - exclude
    if !spec.include.is_empty() {
        if let Some(ws) = ws {
            // Use exclude patterns to skip entire directories during walk.
            // This avoids descending into node_modules/, dist/, etc.
            let exclude_globs = &spec.exclude;
            let filter_dir = |dir: &str| {
                let dir_cp = CanonicalPath::new(dir);
                // If any exclude glob matches this directory (as a prefix),
                // prune the entire subtree. We check by appending a dummy
                // file name to see if the directory itself is excluded.
                !exclude_globs.iter().any(|glob| {
                    glob.matches(&CanonicalPath::new(&format!("{}/x", dir_cp.as_str())))
                })
            };

            if let Ok(entries) = ws.walk(project_root.as_str(), &filter_dir, &|_| true) {
                for entry_path in entries {
                    let cp = CanonicalPath::new(&entry_path);
                    if spec.matches(&cp) {
                        result.insert(cp);
                    }
                }
            }
            // If walk() returns Err (UnsupportedOperation, NotFound, etc.),
            // gracefully fall through — only files entries are materialized.
        }
    }

    result
}

/// Build a `ProjectResolver` from ownership projects.
fn build_resolver_from_projects(projects: &[OwnershipProject]) -> ProjectResolver {
    let ide_configs: Vec<IdeProjectConfig> = projects
        .iter()
        .map(|p| match &p.payload {
            ProjectPayload::Configured {
                tsconfig_path,
                compiler_options,
                references,
                workspace_aliases,
                membership,
            } => {
                let mut config = IdeProjectConfig::new(
                    p.root.as_str().to_string(),
                    p.workspace_root.as_str().to_string(),
                    Some(tsconfig_path.as_str().to_string()),
                );
                config.compiler_options = compiler_options.clone();
                config.references = references.iter().map(|r| r.as_str().to_string()).collect();
                config.workspace_aliases = workspace_aliases.clone();
                // Carry the EXACT configured membership onto the resolver
                // config — the same materialized set the snapshot's
                // `configured_owner_resolution_for_file` consults. No lossy
                // exact→glob round-trip, no second membership engine.
                config.membership = membership.clone();
                config
            }
            ProjectPayload::Fallback { .. } => IdeProjectConfig::new(
                p.root.as_str().to_string(),
                p.workspace_root.as_str().to_string(),
                None,
            ),
        })
        .collect();

    ProjectResolver::new(ide_configs)
}

/// Bridge: Convert a legacy `VfsProjectConfig` to an `OwnershipProject`.
///
/// Used during the migration period by `Engine::rebuild_and_publish()` to
/// build a `WorkspaceSnapshot` from the legacy `ProjectGraph`.
pub fn ownership_project_from_vfs_config(
    config: &crate::project_graph::VfsProjectConfig,
    id: ProjectId,
) -> OwnershipProject {
    let root = CanonicalPath::new(&config.root);
    let workspace_root = CanonicalPath::new(&config.workspace_root);

    if let Some(ref tsconfig) = config.tsconfig_path {
        // Configured project — the legacy `VfsProjectConfig` already carries the
        // exact `ConfiguredMembership`, so no re-conversion / re-materialization.
        OwnershipProject {
            id,
            root,
            workspace_root,
            payload: ProjectPayload::Configured {
                tsconfig_path: CanonicalPath::new(tsconfig),
                membership: config.membership.clone(),
                compiler_options: config.compiler_options.clone(),
                references: config
                    .references
                    .iter()
                    .map(|r| CanonicalPath::new(r))
                    .collect(),
                workspace_aliases: config.workspace_aliases.clone(),
            },
        }
    } else {
        // Fallback project
        OwnershipProject {
            id,
            root: root.clone(),
            workspace_root,
            payload: ProjectPayload::Fallback {
                membership: FallbackMembership {
                    root: root.clone(),
                    exclude: typescript_default_excludes(&root),
                },
            },
        }
    }
}

#[cfg(test)]
#[path = "snapshot_builder_tests.rs"]
mod tests;
