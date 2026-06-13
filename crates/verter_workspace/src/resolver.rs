//! Project-aware import resolver.
//!
//! Resolves import specifiers against tsconfig paths, project references,
//! workspace aliases, node_modules (package.json exports/imports), and
//! relative/absolute paths. Produces [`ResolveResult`] containing both the
//! source path and the provider-graph path used by the type provider.

use std::collections::HashMap;
use std::path::Path;

use crate::types::PackageManifest;
use crate::types::{
    ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase, ResolveRequest,
    ResolveRequestKind, ResolveResult,
};

// ── Types ──

/// A workspace alias maps a prefix (e.g. `@/`) to a filesystem replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAlias {
    pub find: String,
    pub replacement: String,
}

/// Compiler options extracted from a tsconfig for resolution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IdeProjectCompilerOptions {
    pub base_url: Option<String>,
    pub paths: Vec<(String, Vec<String>)>,
}

/// Membership filter for a tsconfig project.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProjectMembership {
    #[default]
    MatchAll,
    IncludeExclude {
        files: Vec<String>,
        include: Vec<String>,
        exclude: Vec<String>,
    },
}

/// Configuration for a single IDE project (tsconfig-backed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeProjectConfig {
    pub root: String,
    pub workspace_root: String,
    pub tsconfig_path: Option<String>,
    pub provider_root: String,
    pub workspace_aliases: Vec<WorkspaceAlias>,
    pub compiler_options: IdeProjectCompilerOptions,
    pub references: Vec<String>,
    pub membership: ProjectMembership,
}

impl IdeProjectConfig {
    pub fn new(root: String, workspace_root: String, tsconfig_path: Option<String>) -> Self {
        let provider_root = root.clone();
        Self {
            root,
            workspace_root,
            tsconfig_path,
            provider_root,
            workspace_aliases: Vec::new(),
            compiler_options: IdeProjectCompilerOptions::default(),
            references: Vec::new(),
            membership: ProjectMembership::MatchAll,
        }
    }

    /// Whether `file_id` is a member of this project.
    ///
    /// Architecture-guard exception: the `/node_modules/` substring check
    /// below IS the primitive that the public `WorkspaceAccess::is_workspace_owned`
    /// and `is_package_backed` accessors are built on (see
    /// `Engine::is_workspace_owned` in engine.rs). Calling the typed API from
    /// inside its own implementation would be circular. See exception
    /// class (1) in `crates/verter_session/tests/architecture_guards.rs` →
    /// `no_node_modules_substring_outside_workspace_api`.
    pub fn matches_file(&self, file_id: &str) -> bool {
        let normalized_file = normalize_canonical_id(file_id);
        if normalized_file.contains("/node_modules/") {
            return false;
        }
        if !normalized_starts_with(file_id, &self.root) {
            return false;
        }

        match &self.membership {
            ProjectMembership::MatchAll => true,
            ProjectMembership::IncludeExclude {
                files,
                include,
                exclude,
            } => {
                if matches_any_pattern_for_root(&normalized_file, &self.root, exclude) {
                    return false;
                }

                if files
                    .iter()
                    .map(|candidate| {
                        normalize_project_membership_entry(&self.root, candidate, false)
                    })
                    .any(|candidate| candidate == normalized_file)
                {
                    return true;
                }

                if !include.is_empty() {
                    return matches_any_pattern_for_root(&normalized_file, &self.root, include);
                }

                !exclude.is_empty()
            }
        }
    }
}

// ── ProjectResolver ──

/// The main project resolver. Holds a sorted list of IDE project configs
/// and resolves import specifiers against them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectResolver {
    projects: Vec<IdeProjectConfig>,
}

/// Backward-compatible type alias for [`ProjectResolver`].
///
/// Kept for downstream crates that reference the original name from
/// the verter_semantic::analysis era.
pub type NativeProjectResolver = ProjectResolver;

impl ProjectResolver {
    pub fn new(projects: Vec<IdeProjectConfig>) -> Self {
        let mut projects = projects;
        projects.sort_by(compare_projects);
        Self { projects }
    }

    pub fn owner_for_file(&self, file_id: &str) -> Option<&IdeProjectConfig> {
        // Collect every configured project whose membership claims the file.
        let configured: Vec<&IdeProjectConfig> = self
            .projects
            .iter()
            .filter(|project| project.tsconfig_path.is_some() && project.matches_file(file_id))
            .collect();

        // Nearest-root effective ownership (same rule as
        // `WorkspaceSnapshot::configured_owner_resolution_for_file`): a
        // configured candidate whose root is a STRICT ANCESTOR of another
        // matching candidate's root loses. `extends`/breadth at an ancestor
        // root must not make a descendant package file ambiguous when a
        // descendant configured project also claims it.
        let effective: Vec<&IdeProjectConfig> = configured
            .iter()
            .copied()
            .filter(|candidate| {
                let candidate_root = normalize_canonical_id(&candidate.root);
                !configured.iter().any(|other| {
                    if std::ptr::eq(*other, *candidate) {
                        return false;
                    }
                    let other_root = normalize_canonical_id(&other.root);
                    // `other` strictly under `candidate` ⇒ candidate is an
                    // ancestor ⇒ drop the ancestor candidate. The length check
                    // makes containment STRICT (equal roots are not ancestors).
                    other_root.len() > candidate_root.len()
                        && normalized_starts_with(&other_root, &candidate_root)
                })
            })
            .collect();

        match effective.as_slice() {
            // Unique effective configured owner.
            [only] => return Some(only),
            // Same-root / incomparable-root overlap → genuine ambiguity.
            [_, ..] => return None,
            // No configured owner → fall through to fallback selection.
            [] => {}
        }

        // No configured owner: a single fallback may own the file, but two
        // overlapping fallbacks stay ambiguous.
        let mut fallback: Option<&IdeProjectConfig> = None;
        let mut fallback_ambiguous = false;
        for project in &self.projects {
            if project.tsconfig_path.is_some() || !project.matches_file(file_id) {
                continue;
            }
            if fallback.is_some() {
                fallback_ambiguous = true;
            } else {
                fallback = Some(project);
            }
        }

        (!fallback_ambiguous).then_some(fallback).flatten()
    }

    fn project_for_ownership(
        &self,
        owner: &crate::types::ProjectOwnership,
    ) -> Option<&IdeProjectConfig> {
        let normalized_root = normalize_canonical_id(&owner.project_root);
        let normalized_tsconfig = owner
            .tsconfig_path
            .as_ref()
            .map(|path| normalize_canonical_id(path));
        let mut matched: Option<&IdeProjectConfig> = None;

        for project in &self.projects {
            if normalize_canonical_id(&project.root) != normalized_root {
                continue;
            }
            let project_tsconfig = project
                .tsconfig_path
                .as_ref()
                .map(|path| normalize_canonical_id(path));
            if project_tsconfig != normalized_tsconfig {
                continue;
            }
            if matched.is_some() {
                return None;
            }
            matched = Some(project);
        }

        matched
    }

    /// Map a source file path to the provider-graph path used by the type provider.
    ///
    /// For `.vue` files this appends `.ts` (the public API shim); for non-Vue
    /// files the source path is returned as-is. This is a pure path transform
    /// that does not require project ownership — callers that need ownership
    /// must check it separately via `owner_for_file()`.
    pub fn provider_id_for_source(&self, source_id: &str) -> Option<String> {
        let normalized_source = normalize_canonical_id(source_id);
        if normalized_source.ends_with(".vue") {
            Some(format!("{}.ts", normalized_source))
        } else {
            Some(normalized_source)
        }
    }

    /// Map a `.vue` source path to the IDE artifact path (`.vue.tsx` or `.vue.jsx`).
    ///
    /// Returns `None` for non-Vue files. This is a pure path transform that
    /// does not require project ownership — callers that need ownership must
    /// check it separately via `owner_for_file()`.
    /// `is_jsx` selects between `.tsx` (TypeScript) and `.jsx` (JavaScript) output.
    pub fn provider_ide_id_for_source(&self, source_id: &str, is_jsx: bool) -> Option<String> {
        let normalized_source = normalize_canonical_id(source_id);
        if !normalized_source.ends_with(".vue") {
            return None;
        }

        let ext = if is_jsx { ".jsx" } else { ".tsx" };
        Some(format!("{}{}", normalized_source, ext))
    }

    /// Reverse-map a provider-graph path back to its source file path.
    ///
    /// Strips `.tsx`, `.jsx`, or `.ts` suffixes from Vue virtual paths
    /// (e.g., `foo.vue.tsx` -> `foo.vue`). For non-Vue paths, returns the
    /// path as-is if owned by a project.
    pub fn source_id_from_provider_id(&self, provider_id: &str) -> Option<String> {
        let normalized = normalize_canonical_id(provider_id);

        // Vue virtual paths: strip .tsx/.jsx/.ts suffix to get the .vue source
        if normalized.ends_with(".vue.tsx") || normalized.ends_with(".vue.jsx") {
            let candidate = &normalized[..normalized.len() - 4]; // strip .tsx/.jsx
                                                                 // Verify the candidate is owned by a project
            if self.owner_for_file(candidate).is_some() {
                return Some(candidate.to_string());
            }
        }
        if normalized.ends_with(".vue.ts") {
            let candidate = &normalized[..normalized.len() - 3]; // strip .ts
            if self.owner_for_file(candidate).is_some() {
                return Some(candidate.to_string());
            }
        }

        // Non-Vue: provider path == source path (if owned by a project)
        if self.owner_for_file(&normalized).is_some() {
            return Some(normalized);
        }

        None
    }

    /// Resolve an import specifier against all configured projects.
    ///
    /// Tries workspace aliases, tsconfig paths, baseUrl, project references,
    /// `package.json` `#imports`, and `node_modules` in order. Returns both
    /// the source path and the provider-graph path for the type provider.
    ///
    /// When the importer has no owning project, owner-independent branches
    /// (relative/absolute, `#imports`, `node_modules`) are still attempted.
    /// Only alias-based resolution (workspace aliases, tsconfig paths, baseUrl,
    /// project references) requires an owning project.
    pub fn resolve_with_reader(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        request: &ResolveRequest,
    ) -> Option<ResolveResult> {
        let importer_owner = self.owner_for_file(&request.importer_id);
        let ctx = ResolutionContext {
            phase: request.phase,
            kind: request.kind,
        };

        let (source_id, resolution_kind) = match importer_owner {
            Some(owner) => self.resolve_source_id(
                reader,
                owner,
                &request.importer_id,
                &request.specifier,
                ctx,
            )?,
            None => {
                // No owning project — try owner-independent branches only
                self.resolve_source_id_unowned(
                    reader,
                    &request.importer_id,
                    &request.specifier,
                    ctx,
                )?
            }
        };

        Some(self.build_resolve_result(request, source_id, resolution_kind))
    }

    pub fn resolve_for_project_with_reader(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        owner: &crate::types::ProjectOwnership,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> Option<ResolveResult> {
        let project = self.project_for_ownership(owner)?;
        let (source_id, resolution_kind) =
            self.resolve_source_id_for_project(reader, project, specifier, ctx)?;
        Some(self.build_project_resolve_result(specifier, source_id, resolution_kind))
    }

    /// Build a [`ResolveResult`] from a resolved source path.
    ///
    /// Looks up `owner_for_file()` on the **target** (not importer) for correct
    /// `provider_id`/`provider_specifier`/`provider_target`/`owner_tsconfig_path`.
    fn build_resolve_result(
        &self,
        request: &ResolveRequest,
        source_id: String,
        resolution_kind: ResolutionKind,
    ) -> ResolveResult {
        let target_owner = self.owner_for_file(&source_id);
        let provider_id = target_owner
            .and_then(|_| self.provider_id_for_source(&source_id))
            .unwrap_or_else(|| source_id.clone());
        let provider_target = match target_owner {
            Some(_) if normalize_canonical_id(&source_id).ends_with(".vue") => {
                ProviderTarget::VuePublicApi
            }
            Some(_) => ProviderTarget::ShadowSourceFile,
            None => ProviderTarget::SourceFile,
        };
        let provider_specifier = if target_owner.is_some() {
            match provider_target {
                // Vue imports must target the synthetic public API file for cross-file
                // component typing inside the provider project.
                ProviderTarget::VuePublicApi => {
                    let importer_provider_id = self
                        .provider_id_for_source(&request.importer_id)
                        .unwrap_or_else(|| normalize_canonical_id(&request.importer_id));
                    relative_specifier(&importer_provider_id, &provider_id)
                }
                // Non-Vue workspace files stay on the original source specifier so the
                // provider project obeys the workspace tsconfig and does not introduce
                // explicit `.ts` imports that trigger TS5097 under bundler resolution.
                ProviderTarget::ShadowSourceFile | ProviderTarget::SourceFile => {
                    request.specifier.clone()
                }
            }
        } else {
            request.specifier.clone()
        };

        ResolveResult {
            owner_tsconfig_path: target_owner.and_then(|project| project.tsconfig_path.clone()),
            source_id,
            provider_id,
            provider_specifier,
            provider_target,
            resolution_kind,
        }
    }

    fn build_project_resolve_result(
        &self,
        specifier: &str,
        source_id: String,
        resolution_kind: ResolutionKind,
    ) -> ResolveResult {
        let target_owner = self.owner_for_file(&source_id);
        let provider_id = target_owner
            .and_then(|_| self.provider_id_for_source(&source_id))
            .unwrap_or_else(|| source_id.clone());
        let provider_target = match target_owner {
            Some(_) if normalize_canonical_id(&source_id).ends_with(".vue") => {
                ProviderTarget::VuePublicApi
            }
            Some(_) => ProviderTarget::ShadowSourceFile,
            None => ProviderTarget::SourceFile,
        };

        ResolveResult {
            owner_tsconfig_path: target_owner.and_then(|project| project.tsconfig_path.clone()),
            source_id,
            provider_id,
            provider_specifier: specifier.to_string(),
            provider_target,
            resolution_kind,
        }
    }

    /// Resolve owner-independent branches only (no tsconfig paths, aliases, etc.).
    ///
    /// Used when the importer has no owning project. Only attempts:
    /// - Relative/absolute path resolution
    /// - `#imports` (package.json imports field)
    /// - `node_modules` resolution
    fn resolve_source_id_unowned(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        importer_id: &str,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> Option<(String, ResolutionKind)> {
        // Relative / absolute
        if is_relative_specifier(specifier) || is_absolute_specifier(specifier) {
            let importer_dir = parent_dir(importer_id);
            let base = if is_absolute_specifier(specifier) {
                normalize_canonical_id(specifier)
            } else {
                join_paths(&importer_dir, specifier)
            };
            let resolved = probe_path_for_context(reader, &base, ctx)?;
            if !package_follow_is_confirmed(reader, importer_id, &resolved) {
                return None;
            }
            return Some((resolved, ResolutionKind::Relative));
        }

        // #imports (unowned — unbounded walk)
        if specifier.starts_with('#') {
            if let Some(resolved) =
                resolve_package_imports(reader, importer_id, specifier, ctx, None)
            {
                return Some((resolved, ResolutionKind::PackageImports));
            }
            return None;
        }

        // node_modules (unowned — unbounded walk)
        if let Some((resolved, resolution_kind)) =
            resolve_node_modules_package(reader, importer_id, specifier, ctx, None)
        {
            return Some((resolved, resolution_kind));
        }

        None
    }

    fn resolve_source_id(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        importer_owner: &IdeProjectConfig,
        importer_id: &str,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> Option<(String, ResolutionKind)> {
        if is_relative_specifier(specifier) || is_absolute_specifier(specifier) {
            let importer_dir = parent_dir(importer_id);
            let base = if is_absolute_specifier(specifier) {
                normalize_canonical_id(specifier)
            } else {
                join_paths(&importer_dir, specifier)
            };
            let resolved = probe_path_for_context(reader, &base, ctx)?;
            if !package_follow_is_confirmed(reader, importer_id, &resolved) {
                return None;
            }
            return Some((resolved, ResolutionKind::Relative));
        }

        for alias in sorted_workspace_aliases(&importer_owner.workspace_aliases) {
            if !specifier.starts_with(&alias.find) {
                continue;
            }
            let remainder = &specifier[alias.find.len()..];
            let base = join_paths(&alias.replacement, remainder);
            if let Some(resolved) = resolve_path_mapping_target(reader, &base, ctx) {
                return Some((resolved, ResolutionKind::WorkspaceAlias));
            }
        }

        if let Some(resolved) = resolve_tsconfig_paths(reader, importer_owner, specifier, ctx) {
            return Some((resolved, ResolutionKind::TsConfigPath));
        }

        if let Some(base_url) = importer_owner.compiler_options.base_url.as_deref() {
            let base = join_paths(base_url, specifier);
            if let Some(resolved) = resolve_path_mapping_target(reader, &base, ctx) {
                return Some((resolved, ResolutionKind::TsConfigPath));
            }
        }

        if let Some(resolved) =
            self.resolve_project_references(reader, importer_owner, specifier, ctx)
        {
            return Some((resolved, ResolutionKind::ProjectReference));
        }

        // #imports (owned — bounded by workspace_root)
        if specifier.starts_with('#') {
            if let Some(resolved) = resolve_package_imports(
                reader,
                importer_id,
                specifier,
                ctx,
                Some(&importer_owner.workspace_root),
            ) {
                return Some((resolved, ResolutionKind::PackageImports));
            }
            return None;
        }

        // node_modules (owned — bounded by workspace_root)
        if let Some((resolved, resolution_kind)) = resolve_node_modules_package(
            reader,
            importer_id,
            specifier,
            ctx,
            Some(&importer_owner.workspace_root),
        ) {
            return Some((resolved, resolution_kind));
        }

        None
    }

    fn resolve_source_id_for_project(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        project: &IdeProjectConfig,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> Option<(String, ResolutionKind)> {
        if is_relative_specifier(specifier) || is_absolute_specifier(specifier) {
            let base = if is_absolute_specifier(specifier) {
                normalize_canonical_id(specifier)
            } else {
                join_paths(&project.root, specifier)
            };
            let resolved = probe_path_for_context(reader, &base, ctx)?;
            return Some((resolved, ResolutionKind::Relative));
        }

        for alias in sorted_workspace_aliases(&project.workspace_aliases) {
            if !specifier.starts_with(&alias.find) {
                continue;
            }
            let remainder = &specifier[alias.find.len()..];
            let base = join_paths(&alias.replacement, remainder);
            if let Some(resolved) = resolve_path_mapping_target(reader, &base, ctx) {
                return Some((resolved, ResolutionKind::WorkspaceAlias));
            }
        }

        if let Some(resolved) = resolve_tsconfig_paths(reader, project, specifier, ctx) {
            return Some((resolved, ResolutionKind::TsConfigPath));
        }

        if let Some(base_url) = project.compiler_options.base_url.as_deref() {
            let base = join_paths(base_url, specifier);
            if let Some(resolved) = resolve_path_mapping_target(reader, &base, ctx) {
                return Some((resolved, ResolutionKind::TsConfigPath));
            }
        }

        if let Some(resolved) = self.resolve_project_references(reader, project, specifier, ctx) {
            return Some((resolved, ResolutionKind::ProjectReference));
        }

        // #imports (project-scoped — bounded by workspace_root)
        if specifier.starts_with('#') {
            if let Some(resolved) = resolve_package_imports_from_dir(
                reader,
                &project.root,
                specifier,
                ctx,
                Some(&project.workspace_root),
            ) {
                return Some((resolved, ResolutionKind::PackageImports));
            }
            return None;
        }

        // node_modules (project-scoped — bounded by workspace_root)
        if let Some((resolved, resolution_kind)) = resolve_node_modules_package_from_dir(
            reader,
            &project.root,
            specifier,
            ctx,
            Some(&project.workspace_root),
        ) {
            return Some((resolved, resolution_kind));
        }

        None
    }

    /// Compute the preferred import specifier for a target file relative to an importer.
    ///
    /// Returns the shortest alias-based specifier (tsconfig paths or workspace aliases)
    /// that round-trips back to the original target via `resolve_with_reader()`.
    /// Returns `None` if no alias matches or the importer has no owning project.
    pub fn preferred_specifier(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        importer_id: &str,
        target_id: &str,
    ) -> Option<String> {
        let owner = self.owner_for_file(importer_id)?;
        let normalized_target = normalize_canonical_id(target_id);
        let mut candidates: Vec<String> = Vec::new();

        // 1. Collect candidates from tsconfig paths
        let base_url = owner
            .compiler_options
            .base_url
            .as_deref()
            .unwrap_or(owner.root.as_str());

        for (pattern, targets) in &owner.compiler_options.paths {
            for target_template in targets {
                if let Some(specifier) =
                    reverse_tsconfig_path(base_url, pattern, target_template, &normalized_target)
                {
                    candidates.push(specifier);
                }
            }
        }

        // 2. Collect candidates from workspace aliases
        for alias in &owner.workspace_aliases {
            let mut replacement = normalize_canonical_id(&alias.replacement);
            // Ensure replacement ends with '/' for consistent prefix matching.
            // Vite normalization stores replacement without trailing slash but
            // find with trailing slash (e.g., find="@/", replacement="/workspace/src").
            if !replacement.ends_with('/') {
                replacement.push('/');
            }
            if let Some(remainder) = normalized_target.strip_prefix(replacement.as_str()) {
                // find already has trailing slash from Vite normalization —
                // remainder has no leading slash, so concatenation is clean.
                let specifier = format!("{}{}", alias.find, remainder);
                candidates.push(specifier);
            }
        }

        // 3. Round-trip verify and pick shortest
        let mut best: Option<String> = None;
        for candidate in candidates {
            let request = crate::types::ResolveRequest {
                importer_id: importer_id.to_string(),
                specifier: candidate.clone(),
                kind: crate::types::ResolveRequestKind::EsmImport,
                phase: crate::types::ResolvePhase::CodegenBlocker,
            };
            if let Some(result) = self.resolve_with_reader(reader, &request) {
                if normalize_canonical_id(&result.source_id) == normalized_target {
                    match &best {
                        Some(current) if current.len() <= candidate.len() => {}
                        _ => best = Some(candidate),
                    }
                }
            }
        }

        best
    }

    fn resolve_project_references(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        importer_owner: &IdeProjectConfig,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> Option<String> {
        for reference in &importer_owner.references {
            let Some(project) = self
                .projects
                .iter()
                .find(|candidate| candidate.tsconfig_path.as_deref() == Some(reference.as_str()))
            else {
                continue;
            };

            for alias in sorted_workspace_aliases(&project.workspace_aliases) {
                if !specifier.starts_with(&alias.find) {
                    continue;
                }
                let remainder = &specifier[alias.find.len()..];
                let base = join_paths(&alias.replacement, remainder);
                if let Some(resolved) = resolve_path_mapping_target(reader, &base, ctx) {
                    return Some(resolved);
                }
            }

            if let Some(resolved) = resolve_tsconfig_paths(reader, project, specifier, ctx) {
                return Some(resolved);
            }

            if let Some(base_url) = project.compiler_options.base_url.as_deref() {
                let base = join_paths(base_url, specifier);
                if let Some(resolved) = resolve_path_mapping_target(reader, &base, ctx) {
                    return Some(resolved);
                }
            }

            if let Some(resolved) = self.resolve_project_references(reader, project, specifier, ctx)
            {
                return Some(resolved);
            }
        }

        None
    }
}

// ── Private helpers ──

fn normalized_starts_with(path: &str, prefix: &str) -> bool {
    let normalized = normalize_canonical_id(path);
    let prefix = normalize_canonical_id(prefix);
    normalized.starts_with(&prefix)
        && (normalized.len() == prefix.len()
            || prefix.ends_with('/')
            || normalized.as_bytes().get(prefix.len()) == Some(&b'/'))
}

fn matches_any_pattern_for_root(path: &str, root: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .map(|pattern| normalize_project_membership_entry(root, pattern, true))
        .filter_map(|pattern| glob::Pattern::new(&pattern).ok())
        .any(|pattern| pattern.matches(path))
}

fn resolve_tsconfig_paths(
    reader: &dyn crate::traits::WorkspaceRead,
    project: &IdeProjectConfig,
    specifier: &str,
    ctx: ResolutionContext,
) -> Option<String> {
    let base_url = project
        .compiler_options
        .base_url
        .as_deref()
        .unwrap_or(project.root.as_str());

    for (pattern, targets) in &project.compiler_options.paths {
        let Some(captured) = capture_tsconfig_pattern(pattern, specifier) else {
            continue;
        };

        for target in targets {
            let candidate = apply_tsconfig_target(base_url, target, captured);
            if let Some(resolved) = resolve_path_mapping_target(reader, &candidate, ctx) {
                return Some(resolved);
            }
        }
    }

    None
}

fn resolve_path_mapping_target(
    reader: &dyn crate::traits::WorkspaceRead,
    candidate: &str,
    ctx: ResolutionContext,
) -> Option<String> {
    let normalized = normalize_canonical_id(candidate);
    let package_json_path = join_paths(&normalized, "package.json");
    if let Some(package_json) = read_package_manifest_if_present(reader, &package_json_path) {
        if let Some(exports) = package_json.exports.as_ref() {
            if let Some(resolved) = resolve_package_exports(reader, &normalized, exports, ".", ctx)
            {
                return Some(resolved);
            }
            if prefers_declaration_files(ctx) {
                if let Some(types_entry) =
                    resolve_manifest_types_entry(reader, &normalized, &package_json)
                {
                    return Some(types_entry);
                }
            }
        }

        if let Some(resolved) = resolve_legacy_package(reader, &normalized, &package_json, "", ctx)
        {
            return Some(resolved);
        }
    }

    probe_path_for_context(reader, &normalized, ctx)
}

fn capture_tsconfig_pattern<'a>(pattern: &'a str, specifier: &'a str) -> Option<&'a str> {
    if let Some(star) = pattern.find('*') {
        let prefix = &pattern[..star];
        let suffix = &pattern[star + 1..];
        if !specifier.starts_with(prefix) || !specifier.ends_with(suffix) {
            return None;
        }
        let captured_end = specifier.len().saturating_sub(suffix.len());
        if prefix.len() > captured_end {
            return None;
        }
        Some(&specifier[prefix.len()..captured_end])
    } else if pattern == specifier {
        Some("")
    } else {
        None
    }
}

fn apply_tsconfig_target(base_url: &str, target: &str, captured: &str) -> String {
    let replaced = if let Some(star) = target.find('*') {
        format!("{}{}{}", &target[..star], captured, &target[star + 1..])
    } else {
        target.to_string()
    };
    if is_absolute_specifier(&replaced) {
        normalize_canonical_id(&replaced)
    } else {
        join_paths(base_url, &replaced)
    }
}

/// Reverse a tsconfig path mapping: given a target file path, reconstruct the
/// import specifier that would match the given pattern → target template.
fn reverse_tsconfig_path(
    base_url: &str,
    pattern: &str,
    target_template: &str,
    target_id: &str,
) -> Option<String> {
    // Compute the absolute target prefix and suffix from the template
    let (target_prefix, target_suffix) = if let Some(star) = target_template.find('*') {
        let prefix_part = &target_template[..star];
        let suffix_part = &target_template[star + 1..];
        let mut prefix = if is_absolute_specifier(prefix_part) {
            normalize_canonical_id(prefix_part)
        } else {
            join_paths(base_url, prefix_part)
        };
        // The template's trailing slash (`/workspace/src/*`) is a directory
        // boundary marker; the canonical-id normalizer strips trailing slashes,
        // so restore the boundary the template carried — otherwise the captured
        // remainder would keep a leading `/`.
        if prefix_part.ends_with('/') && !prefix.ends_with('/') {
            prefix.push('/');
        }
        (prefix, suffix_part.to_string())
    } else {
        // No wildcard — exact match only
        let abs = if is_absolute_specifier(target_template) {
            normalize_canonical_id(target_template)
        } else {
            join_paths(base_url, target_template)
        };
        return if normalize_canonical_id(target_id) == abs {
            // Pattern without wildcard: return the pattern itself
            Some(pattern.to_string())
        } else {
            None
        };
    };

    // Check if target_id matches the prefix + ... + suffix shape
    let normalized_target = normalize_canonical_id(target_id);
    if !normalized_target.starts_with(&target_prefix) {
        return None;
    }
    if !target_suffix.is_empty() && !normalized_target.ends_with(&target_suffix) {
        return None;
    }
    let captured_end = normalized_target.len().saturating_sub(target_suffix.len());
    if target_prefix.len() > captured_end {
        return None;
    }
    let captured = &normalized_target[target_prefix.len()..captured_end];

    // Reconstruct specifier from pattern
    if let Some(star) = pattern.find('*') {
        Some(format!(
            "{}{}{}",
            &pattern[..star],
            captured,
            &pattern[star + 1..]
        ))
    } else {
        Some(pattern.to_string())
    }
}

fn sorted_workspace_aliases(aliases: &[WorkspaceAlias]) -> Vec<&WorkspaceAlias> {
    let mut aliases = aliases.iter().collect::<Vec<_>>();
    aliases.sort_by(|a, b| {
        b.find
            .len()
            .cmp(&a.find.len())
            .then_with(|| a.find.cmp(&b.find))
    });
    aliases
}

fn probe_path(reader: &dyn crate::traits::WorkspaceRead, base: &str) -> Option<String> {
    let base = normalize_canonical_id(base);
    let has_extension = Path::new(&base).extension().is_some();

    if has_extension {
        if let Some(resolved) = resolve_existing_path(reader, &base) {
            return Some(resolved);
        }
    } else {
        for extension in probe_extensions() {
            let candidate = format!("{base}{extension}");
            if let Some(resolved) = resolve_existing_path(reader, &candidate) {
                return Some(resolved);
            }
        }
    }

    for index_name in probe_index_files() {
        let candidate = format!("{}/{}", base.trim_end_matches('/'), index_name);
        if let Some(resolved) = resolve_existing_path(reader, &candidate) {
            return Some(resolved);
        }
    }

    None
}

fn probe_path_for_context(
    reader: &dyn crate::traits::WorkspaceRead,
    base: &str,
    ctx: ResolutionContext,
) -> Option<String> {
    let normalized = normalize_canonical_id(base);
    if prefers_declaration_files(ctx) {
        if let Some(resolved) = resolve_declaration_companion(reader, &normalized) {
            return Some(resolved);
        }
    }
    probe_path(reader, &normalized)
}

fn resolve_existing_path(
    reader: &dyn crate::traits::WorkspaceRead,
    candidate: &str,
) -> Option<String> {
    let normalized = normalize_canonical_id(candidate);
    if !reader.file_exists(&normalized) {
        return None;
    }
    Some(
        reader
            .realpath(&normalized)
            .map(|path| normalize_canonical_id(&path))
            .unwrap_or(normalized),
    )
}

pub(crate) fn probe_extensions() -> &'static [&'static str] {
    &[
        ".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs", ".cts", ".cjs", ".vue", ".d.ts", ".d.mts",
        ".d.cts",
    ]
}

fn probe_index_files() -> &'static [&'static str] {
    &[
        "index.ts",
        "index.tsx",
        "index.js",
        "index.jsx",
        "index.mts",
        "index.mjs",
        "index.cts",
        "index.cjs",
        "index.vue",
        "index.d.ts",
        "index.d.mts",
        "index.d.cts",
    ]
}

fn prefers_declaration_files(ctx: ResolutionContext) -> bool {
    matches!(
        (ctx.phase, ctx.kind),
        (ResolvePhase::CodegenBlocker, ResolveRequestKind::TypeImport)
            | (ResolvePhase::ProviderGraph, _)
    )
}

fn is_declaration_file(path: &str) -> bool {
    let normalized = normalize_canonical_id(path);
    normalized.ends_with(".d.ts")
        || normalized.ends_with(".d.mts")
        || normalized.ends_with(".d.cts")
}

fn resolve_manifest_types_entry(
    reader: &dyn crate::traits::WorkspaceRead,
    package_dir: &str,
    package_json: &PackageManifest,
) -> Option<String> {
    for target in [
        package_json.types.as_deref(),
        package_json.typings.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(resolved) = probe_path(reader, &resolve_package_path(package_dir, target, None))
        {
            return Some(resolved);
        }
    }
    None
}

fn resolve_declaration_companion(
    reader: &dyn crate::traits::WorkspaceRead,
    candidate: &str,
) -> Option<String> {
    let normalized = normalize_canonical_id(candidate);
    let (runtime_ext, companion_exts): (&str, &[&str]) = if normalized.ends_with(".mjs") {
        (".mjs", &[".d.mts", ".d.ts"])
    } else if normalized.ends_with(".cjs") {
        (".cjs", &[".d.cts", ".d.ts"])
    } else if normalized.ends_with(".jsx") {
        (".jsx", &[".d.ts"])
    } else if normalized.ends_with(".js") {
        (".js", &[".d.ts"])
    } else {
        return None;
    };

    let stem = normalized.strip_suffix(runtime_ext)?;

    for companion_ext in companion_exts {
        let companion = format!("{stem}{companion_ext}");
        if let Some(resolved) = resolve_existing_path(reader, &companion) {
            return Some(resolved);
        }
    }

    None
}

fn package_follow_is_confirmed(
    reader: &dyn crate::traits::WorkspaceRead,
    importer_id: &str,
    resolved: &str,
) -> bool {
    let Some(package_dir) = candidate_package_dir_for_importer(importer_id) else {
        return true;
    };
    let package_json_path = join_paths(&package_dir, "package.json");
    if read_package_manifest_if_present(reader, &package_json_path).is_none() {
        return false;
    }
    normalized_starts_with(resolved, &package_dir)
}

fn candidate_package_dir_for_importer(importer_id: &str) -> Option<String> {
    let normalized = normalize_canonical_id(importer_id);
    let node_modules_marker = "/node_modules/";
    let marker_index = normalized.rfind(node_modules_marker)?;
    let package_start = marker_index + node_modules_marker.len();
    let package_path = &normalized[package_start..];

    let mut parts = package_path.split('/');
    let first = parts.next()?;
    let package_rel = if first.starts_with('@') {
        let second = parts.next()?;
        format!("{first}/{second}")
    } else {
        first.to_string()
    };

    Some(format!(
        "{}{node_modules_marker}{package_rel}",
        &normalized[..marker_index]
    ))
}

fn relative_specifier(from_file: &str, to_file: &str) -> String {
    let from_dir = parent_dir(from_file);
    let from_dir = normalize_canonical_id(&from_dir);
    let to_file = normalize_canonical_id(to_file);
    let from_parts = split_path_parts(&from_dir);
    let to_parts = split_path_parts(&to_file);

    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let mut segments = Vec::new();
    for _ in common..from_parts.len() {
        segments.push("..".to_string());
    }
    for part in &to_parts[common..] {
        segments.push(part.clone());
    }

    match segments.as_slice() {
        [] => "./".to_string(),
        _ => {
            let joined = segments.join("/");
            if joined.starts_with("../") || joined == ".." {
                joined
            } else {
                format!("./{joined}")
            }
        }
    }
}

fn split_path_parts(path: &str) -> Vec<String> {
    normalize_canonical_id(path)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_project_membership_entry(
    root: &str,
    value: &str,
    allow_directory_glob: bool,
) -> String {
    let normalized_value = normalize_canonical_id(value);
    let resolved = if Path::new(&normalized_value).is_absolute()
        || normalized_value.as_bytes().get(1) == Some(&b':')
        || normalized_value.starts_with('/')
    {
        normalized_value
    } else {
        format!(
            "{}/{}",
            normalize_canonical_id(root).trim_end_matches('/'),
            normalized_value
                .trim_start_matches("./")
                .trim_start_matches('/')
        )
    };

    if !allow_directory_glob
        || resolved.contains('*')
        || resolved.contains('?')
        || resolved.contains('[')
        || Path::new(&resolved).extension().is_some()
    {
        return resolved;
    }

    format!("{resolved}/**/*")
}

fn compare_projects(a: &IdeProjectConfig, b: &IdeProjectConfig) -> std::cmp::Ordering {
    normalize_canonical_id(&b.root)
        .len()
        .cmp(&normalize_canonical_id(&a.root).len())
        .then_with(|| project_rank(a).cmp(&project_rank(b)))
        .then_with(|| a.tsconfig_path.cmp(&b.tsconfig_path))
        .then_with(|| a.root.cmp(&b.root))
}

fn project_rank(project: &IdeProjectConfig) -> u8 {
    match project.tsconfig_path.as_deref() {
        Some(path) if normalize_canonical_id(path).ends_with("/tsconfig.json") => 0,
        Some(_) => 1,
        None => 2,
    }
}

fn resolve_package_imports(
    reader: &dyn crate::traits::WorkspaceRead,
    importer_id: &str,
    specifier: &str,
    ctx: ResolutionContext,
    boundary: Option<&str>,
) -> Option<String> {
    for directory in ancestor_dirs(importer_id, boundary) {
        let Some(package_json) =
            read_package_manifest_if_present(reader, &join_paths(&directory, "package.json"))
        else {
            continue;
        };
        let Some(imports) = package_json
            .imports
            .as_ref()
            .and_then(|value| value.as_object())
        else {
            continue;
        };
        let Some((entry, captured)) = match_package_mapping(imports, specifier) else {
            continue;
        };
        if let Some(resolved) =
            resolve_package_target(reader, &directory, entry, captured.as_deref(), ctx)
        {
            return Some(resolved);
        }
    }

    None
}

fn resolve_package_imports_from_dir(
    reader: &dyn crate::traits::WorkspaceRead,
    start_dir: &str,
    specifier: &str,
    ctx: ResolutionContext,
    boundary: Option<&str>,
) -> Option<String> {
    for directory in ancestor_dirs_from_dir(start_dir, boundary) {
        let Some(package_json) =
            read_package_manifest_if_present(reader, &join_paths(&directory, "package.json"))
        else {
            continue;
        };
        let Some(imports) = package_json
            .imports
            .as_ref()
            .and_then(|value| value.as_object())
        else {
            continue;
        };
        let Some((entry, captured)) = match_package_mapping(imports, specifier) else {
            continue;
        };
        if let Some(resolved) =
            resolve_package_target(reader, &directory, entry, captured.as_deref(), ctx)
        {
            return Some(resolved);
        }
    }

    None
}

fn resolve_node_modules_package(
    reader: &dyn crate::traits::WorkspaceRead,
    importer_id: &str,
    specifier: &str,
    ctx: ResolutionContext,
    boundary: Option<&str>,
) -> Option<(String, ResolutionKind)> {
    resolve_node_modules_package_from_dirs(
        reader,
        ancestor_dirs(importer_id, boundary),
        specifier,
        ctx,
    )
}

fn resolve_node_modules_package_from_dir(
    reader: &dyn crate::traits::WorkspaceRead,
    start_dir: &str,
    specifier: &str,
    ctx: ResolutionContext,
    boundary: Option<&str>,
) -> Option<(String, ResolutionKind)> {
    resolve_node_modules_package_from_dirs(
        reader,
        ancestor_dirs_from_dir(start_dir, boundary),
        specifier,
        ctx,
    )
}

fn resolve_node_modules_package_from_dirs<I>(
    reader: &dyn crate::traits::WorkspaceRead,
    directories: I,
    specifier: &str,
    ctx: ResolutionContext,
) -> Option<(String, ResolutionKind)>
where
    I: IntoIterator<Item = String>,
{
    let (package_name, subpath) = split_package_specifier(specifier)?;
    for directory in directories {
        let package_dir = join_paths(&join_paths(&directory, "node_modules"), &package_name);
        let package_json_path = join_paths(&package_dir, "package.json");

        if let Some(package_json) = read_package_manifest_if_present(reader, &package_json_path) {
            if let Some(exports) = package_json.exports.as_ref() {
                let export_key = if subpath.is_empty() {
                    ".".to_string()
                } else {
                    format!("./{subpath}")
                };
                if let Some(resolved) =
                    resolve_package_exports(reader, &package_dir, exports, &export_key, ctx)
                {
                    if subpath.is_empty()
                        && prefers_declaration_files(ctx)
                        && !is_declaration_file(&resolved)
                    {
                        if let Some(types_entry) =
                            resolve_manifest_types_entry(reader, &package_dir, &package_json)
                        {
                            return Some((types_entry, ResolutionKind::NodeModules));
                        }
                    }
                    return Some((resolved, ResolutionKind::PackageExports));
                }

                if subpath.is_empty() && prefers_declaration_files(ctx) {
                    if let Some(types_entry) =
                        resolve_manifest_types_entry(reader, &package_dir, &package_json)
                    {
                        return Some((types_entry, ResolutionKind::NodeModules));
                    }
                }

                continue;
            }

            if let Some(resolved) =
                resolve_legacy_package(reader, &package_dir, &package_json, subpath, ctx)
            {
                return Some((resolved, ResolutionKind::NodeModules));
            }
        } else {
            let base = if subpath.is_empty() {
                package_dir.clone()
            } else {
                join_paths(&package_dir, subpath)
            };
            if let Some(resolved) = probe_path_for_context(reader, &base, ctx) {
                return Some((resolved, ResolutionKind::NodeModules));
            }
        }
    }

    None
}

fn read_package_manifest_if_present(
    reader: &dyn crate::traits::WorkspaceRead,
    canonical_id: &str,
) -> Option<PackageManifest> {
    let normalized = normalize_canonical_id(canonical_id);
    if !reader.file_exists(&normalized) {
        return None;
    }
    reader.read_package_manifest(&normalized)
}

fn resolve_package_exports(
    reader: &dyn crate::traits::WorkspaceRead,
    package_dir: &str,
    exports: &serde_json::Value,
    export_key: &str,
    ctx: ResolutionContext,
) -> Option<String> {
    match exports {
        serde_json::Value::String(_) | serde_json::Value::Array(_) => {
            if export_key == "." {
                resolve_package_target(reader, package_dir, exports, None, ctx)
            } else {
                None
            }
        }
        serde_json::Value::Object(map) => {
            if !map.keys().any(|key| key.starts_with('.')) {
                if export_key == "." {
                    return resolve_package_target(reader, package_dir, exports, None, ctx);
                }
                return None;
            }

            let (entry, captured) = match_package_mapping(map, export_key)?;
            resolve_package_target(reader, package_dir, entry, captured.as_deref(), ctx)
        }
        _ => None,
    }
}

fn resolve_legacy_package(
    reader: &dyn crate::traits::WorkspaceRead,
    package_dir: &str,
    package_json: &PackageManifest,
    subpath: &str,
    ctx: ResolutionContext,
) -> Option<String> {
    if !subpath.is_empty() {
        return probe_path_for_context(reader, &join_paths(package_dir, subpath), ctx);
    }

    let keys: &[&str] = match (ctx.phase, ctx.kind) {
        (_, ResolveRequestKind::RequireCall) => &["main"],
        (
            ResolvePhase::CodegenBlocker,
            ResolveRequestKind::EsmImport | ResolveRequestKind::SfcSrcAttr,
        ) => &["module", "main"],
        _ => &["types", "typings", "main"],
    };
    for key in keys {
        let target = match *key {
            "main" => package_json.main.as_deref(),
            "module" => package_json.module.as_deref(),
            "types" => package_json.types.as_deref(),
            "typings" => package_json.typings.as_deref(),
            _ => None,
        };
        let Some(target) = target else {
            continue;
        };
        if let Some(resolved) = probe_path_for_context(
            reader,
            &resolve_package_path(package_dir, target, None),
            ctx,
        ) {
            return Some(resolved);
        }
    }

    probe_path_for_context(reader, &join_paths(package_dir, "index"), ctx)
}

fn resolve_package_target(
    reader: &dyn crate::traits::WorkspaceRead,
    package_dir: &str,
    value: &serde_json::Value,
    captured: Option<&str>,
    ctx: ResolutionContext,
) -> Option<String> {
    match value {
        serde_json::Value::String(target) => probe_path_for_context(
            reader,
            &resolve_package_path(package_dir, target, captured),
            ctx,
        ),
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|item| resolve_package_target(reader, package_dir, item, captured, ctx)),
        serde_json::Value::Object(map) => {
            for condition in package_conditions(ctx) {
                let Some(entry) = map.get(*condition) else {
                    continue;
                };
                if let Some(resolved) =
                    resolve_package_target(reader, package_dir, entry, captured, ctx)
                {
                    return Some(resolved);
                }
            }
            None
        }
        _ => None,
    }
}

fn package_conditions(ctx: ResolutionContext) -> &'static [&'static str] {
    match (ctx.phase, ctx.kind) {
        (_, ResolveRequestKind::RequireCall) => &["require", "default"],
        (
            ResolvePhase::CodegenBlocker,
            ResolveRequestKind::EsmImport | ResolveRequestKind::SfcSrcAttr,
        ) => &["import", "default"],
        (ResolvePhase::CodegenBlocker, ResolveRequestKind::TypeImport) => {
            &["types", "import", "default"]
        }
        (ResolvePhase::ProviderGraph, _) => &["types", "import", "default"],
    }
}

fn resolve_package_path(package_dir: &str, target: &str, captured: Option<&str>) -> String {
    let replaced = match captured {
        Some(captured) if target.contains('*') => {
            let star = target.find('*').unwrap_or(0);
            format!("{}{}{}", &target[..star], captured, &target[star + 1..])
        }
        _ => target.to_string(),
    };

    if is_absolute_specifier(&replaced) {
        normalize_canonical_id(&replaced)
    } else {
        join_paths(package_dir, &replaced)
    }
}

fn split_package_specifier(specifier: &str) -> Option<(String, &str)> {
    if specifier.is_empty() || is_relative_specifier(specifier) || is_absolute_specifier(specifier)
    {
        return None;
    }

    if let Some(rest) = specifier.strip_prefix('@') {
        let mut parts = rest.splitn(3, '/');
        let scope = parts.next()?;
        let name = parts.next()?;
        let subpath = parts.next().unwrap_or("");
        return Some((format!("@{scope}/{name}"), subpath));
    }

    let mut parts = specifier.splitn(2, '/');
    let package_name = parts.next()?.to_string();
    let subpath = parts.next().unwrap_or("");
    Some((package_name, subpath))
}

fn ancestor_dirs(path: &str, boundary: Option<&str>) -> Vec<String> {
    let boundary_norm = boundary.map(normalize_canonical_id);
    let mut result = Vec::new();
    let mut current = parent_dir(path);
    while !current.is_empty() {
        result.push(current.clone());
        if let Some(ref b) = boundary_norm {
            if current == *b {
                break;
            }
        }
        let next = parent_dir(&current);
        if next == current {
            break;
        }
        current = next;
    }
    result
}

fn ancestor_dirs_from_dir(path: &str, boundary: Option<&str>) -> Vec<String> {
    let boundary_norm = boundary.map(normalize_canonical_id);
    let mut result = Vec::new();
    let mut current = normalize_canonical_id(path);
    while !current.is_empty() {
        result.push(current.clone());
        if let Some(ref b) = boundary_norm {
            if current == *b {
                break;
            }
        }
        let next = parent_dir(&current);
        if next == current {
            break;
        }
        current = next;
    }
    result
}

fn match_package_mapping<'a>(
    mappings: &'a serde_json::Map<String, serde_json::Value>,
    specifier: &str,
) -> Option<(&'a serde_json::Value, Option<String>)> {
    let mut best: Option<(&serde_json::Value, Option<String>, usize, bool)> = None;
    for (pattern, value) in mappings {
        let Some(captured) = capture_tsconfig_pattern(pattern, specifier) else {
            continue;
        };
        let exact = !pattern.contains('*');
        let score = pattern.replace('*', "").len();
        match &best {
            Some((_, _, best_score, best_exact))
                if *best_score > score || (*best_score == score && *best_exact && !exact) =>
            {
                continue;
            }
            _ => {
                best = Some((
                    value,
                    (!captured.is_empty()).then(|| captured.to_string()),
                    score,
                    exact,
                ));
            }
        }
    }

    best.map(|(value, captured, _, _)| (value, captured))
}

// ── Public path helpers (used by downstream crates) ──

/// Normalize a canonical ID.
///
/// Delegates to the single canonical-path owner (`verter_span::path` via the
/// crate's `canonical_path` re-export): backslash→slash, `//?/UNC/`/`//?/`
/// extended-prefix stripping, lowercase Windows drive letter, and trailing-slash
/// stripping (except the roots `/` and `x:/`) — no divergent second normalizer.
///
/// The `\\?\` / `//?/` extended-length prefixes the owner strips are the ones
/// `std::fs::canonicalize()` produces on Windows; this normalizer never touches
/// disk itself (the `NativeFs` boundary owns all `std::fs::` access).
pub fn normalize_canonical_id(value: &str) -> String {
    crate::canonical_path::canonicalize_path(value)
}

/// Collapse `.` and `..` segments from a path.
pub fn collapse_path(value: &str) -> String {
    let normalized = normalize_canonical_id(value);

    // UNC paths (`//host/share/...`): the `//host/share` portion is the immutable
    // root — preserved verbatim, and `..` can NEVER escape above the share (just
    // as `..` can't escape `/` or a drive root). Splitting `//` as ordinary
    // segments would either flatten it to `/host/...` or let `..` pop the host /
    // share, both of which split UNC file identity. Handle it as a dedicated
    // branch: peel off host + share as the root, collapse only the tail below it.
    if let Some(after) = normalized.strip_prefix("//") {
        let mut segs = after.split('/').filter(|s| !s.is_empty());
        let mut root = String::from("//");
        if let Some(host) = segs.next() {
            root.push_str(host);
        }
        if let Some(share) = segs.next() {
            root.push('/');
            root.push_str(share);
        }
        let mut parts: Vec<&str> = Vec::new();
        for part in segs {
            match part {
                "." => {}
                // Bounded at the share root: popping an empty stack is a no-op,
                // so `..` never escapes `//host/share`.
                ".." => {
                    parts.pop();
                }
                p => parts.push(p),
            }
        }
        return if parts.is_empty() {
            root
        } else {
            format!("{root}/{}", parts.join("/"))
        };
    }

    let (prefix, rest) = if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        (normalized[..2].to_string(), normalized[2..].to_string())
    } else {
        (String::new(), normalized.clone())
    };

    let absolute = rest.starts_with('/');
    let mut parts = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if let Some(last) = parts.last() {
                    if *last != ".." {
                        parts.pop();
                    } else if !absolute {
                        parts.push("..");
                    }
                } else if !absolute {
                    parts.push("..");
                }
            }
            part => parts.push(part),
        }
    }

    let mut result = String::new();
    if !prefix.is_empty() {
        result.push_str(&prefix);
    }
    if absolute {
        result.push('/');
    }
    result.push_str(&parts.join("/"));

    if result.is_empty() {
        if absolute {
            "/".to_string()
        } else {
            ".".to_string()
        }
    } else if result.len() == 2 && result.as_bytes()[1] == b':' {
        format!("{result}/")
    } else {
        result
    }
}

/// Join two path segments, collapsing `.`/`..`.
pub fn join_paths(base: &str, path: &str) -> String {
    if path.is_empty() {
        return normalize_canonical_id(base);
    }
    if is_absolute_specifier(path) {
        return collapse_path(path);
    }

    let normalized_base = normalize_canonical_id(base)
        .trim_end_matches('/')
        .to_string();
    let normalized_path = normalize_canonical_id(path);
    collapse_path(&format!(
        "{}/{}",
        normalized_base,
        normalized_path
            .trim_start_matches("./")
            .trim_start_matches('/')
    ))
}

/// Return the parent directory of a path.
pub fn parent_dir(path: &str) -> String {
    let normalized = normalize_canonical_id(path);
    normalized
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

/// Check if a specifier is relative (`./` or `../`).
pub fn is_relative_specifier(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

/// Check if a specifier is an absolute path.
pub fn is_absolute_specifier(specifier: &str) -> bool {
    specifier.starts_with('/')
        || Path::new(specifier).is_absolute()
        || specifier.as_bytes().get(1) == Some(&b':')
}

// ── Known-file helpers (used by verter_semantic::analysis for module reference resolution) ──

pub fn build_known_file_index(known_ids: &[String]) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for known_id in known_ids {
        index
            .entry(normalize_known_file_id(known_id))
            .or_insert_with(|| known_id.clone());
    }
    index
}

pub fn resolve_known_dependency_id(
    owner_id: &str,
    specifier: &str,
    known_index: &HashMap<String, String>,
    extensions: &[String],
) -> Option<String> {
    let resolved_base = resolve_known_dependency_base(owner_id, specifier)?;
    if let Some(match_id) = known_index.get(&normalize_known_file_id(&resolved_base)) {
        return Some(match_id.clone());
    }

    let mut seen = std::collections::HashSet::new();
    for extension in extensions {
        if extension.is_empty() {
            continue;
        }

        let with_extension = format!("{resolved_base}{extension}");
        if seen.insert(with_extension.clone()) {
            if let Some(match_id) = known_index.get(&normalize_known_file_id(&with_extension)) {
                return Some(match_id.clone());
            }
        }

        let with_index = format!("{}/index{extension}", resolved_base.trim_end_matches('/'));
        if seen.insert(with_index.clone()) {
            if let Some(match_id) = known_index.get(&normalize_known_file_id(&with_index)) {
                return Some(match_id.clone());
            }
        }
    }

    None
}

pub fn resolve_known_dependency_base(owner_id: &str, specifier: &str) -> Option<String> {
    if is_relative_specifier(specifier) {
        return Some(join_paths(&parent_dir(owner_id), specifier));
    }
    if is_absolute_specifier(specifier) {
        return Some(collapse_path(specifier));
    }
    None
}

pub fn normalize_known_file_id(file_id: &str) -> String {
    collapse_path(file_id)
}

#[cfg(test)]
#[path = "resolver_tests.rs"]
mod resolver_tests;
