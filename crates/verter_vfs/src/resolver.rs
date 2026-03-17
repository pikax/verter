//! Project-aware import resolver.
//!
//! Resolves import specifiers against tsconfig paths, project references,
//! workspace aliases, node_modules (package.json exports/imports), and
//! relative/absolute paths. Produces [`ResolveResult`] containing both the
//! source path and the provider-graph path used by the type provider.

use std::collections::HashMap;
use std::path::Path;

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
/// the verter_analysis era.
pub type NativeProjectResolver = ProjectResolver;

impl ProjectResolver {
    pub fn new(projects: Vec<IdeProjectConfig>) -> Self {
        let mut projects = projects;
        projects.sort_by(compare_projects);
        Self { projects }
    }

    pub fn owner_for_file(&self, file_id: &str) -> Option<&IdeProjectConfig> {
        self.projects
            .iter()
            .find(|project| project.matches_file(file_id))
    }

    /// Map a source file path to the provider-graph path used by the type provider.
    ///
    /// For `.vue` files this appends `.ts` (the public API shim); for non-Vue
    /// files the source path is returned as-is. Returns `None` if the file is
    /// not owned by any project.
    pub fn provider_id_for_source(&self, source_id: &str) -> Option<String> {
        let _project = self.owner_for_file(source_id)?;
        let normalized_source = normalize_canonical_id(source_id);
        if normalized_source.ends_with(".vue") {
            Some(format!("{}.ts", normalized_source))
        } else {
            Some(normalized_source)
        }
    }

    /// Map a `.vue` source path to the IDE artifact path (`.vue.tsx` or `.vue.jsx`).
    ///
    /// Returns `None` for non-Vue files or files not owned by any project.
    /// `is_jsx` selects between `.tsx` (TypeScript) and `.jsx` (JavaScript) output.
    pub fn provider_ide_id_for_source(&self, source_id: &str, is_jsx: bool) -> Option<String> {
        let _project = self.owner_for_file(source_id)?;
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
        reader: &dyn crate::traits::WorkspaceAccess,
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

    /// Resolve owner-independent branches only (no tsconfig paths, aliases, etc.).
    ///
    /// Used when the importer has no owning project. Only attempts:
    /// - Relative/absolute path resolution
    /// - `#imports` (package.json imports field)
    /// - `node_modules` resolution
    fn resolve_source_id_unowned(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
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
            return probe_path(reader, &base).map(|resolved| (resolved, ResolutionKind::Relative));
        }

        // #imports
        if specifier.starts_with('#') {
            if let Some(resolved) = resolve_package_imports(reader, importer_id, specifier, ctx) {
                return Some((resolved, ResolutionKind::PackageImports));
            }
            return None;
        }

        // node_modules
        if let Some((resolved, resolution_kind)) =
            resolve_node_modules_package(reader, importer_id, specifier, ctx)
        {
            return Some((resolved, resolution_kind));
        }

        None
    }

    fn resolve_source_id(
        &self,
        reader: &dyn crate::traits::WorkspaceAccess,
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
            return probe_path(reader, &base).map(|resolved| (resolved, ResolutionKind::Relative));
        }

        for alias in sorted_workspace_aliases(&importer_owner.workspace_aliases) {
            if !specifier.starts_with(&alias.find) {
                continue;
            }
            let remainder = &specifier[alias.find.len()..];
            let base = join_paths(&alias.replacement, remainder);
            if let Some(resolved) = probe_path(reader, &base) {
                return Some((resolved, ResolutionKind::WorkspaceAlias));
            }
        }

        if let Some(resolved) = resolve_tsconfig_paths(reader, importer_owner, specifier) {
            return Some((resolved, ResolutionKind::TsConfigPath));
        }

        if let Some(base_url) = importer_owner.compiler_options.base_url.as_deref() {
            let base = join_paths(base_url, specifier);
            if let Some(resolved) = probe_path(reader, &base) {
                return Some((resolved, ResolutionKind::TsConfigPath));
            }
        }

        if let Some(resolved) = self.resolve_project_references(reader, importer_owner, specifier) {
            return Some((resolved, ResolutionKind::ProjectReference));
        }

        if specifier.starts_with('#') {
            if let Some(resolved) = resolve_package_imports(reader, importer_id, specifier, ctx) {
                return Some((resolved, ResolutionKind::PackageImports));
            }
            return None;
        }

        if let Some((resolved, resolution_kind)) =
            resolve_node_modules_package(reader, importer_id, specifier, ctx)
        {
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
        reader: &dyn crate::traits::WorkspaceAccess,
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
        reader: &dyn crate::traits::WorkspaceAccess,
        importer_owner: &IdeProjectConfig,
        specifier: &str,
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
                if let Some(resolved) = probe_path(reader, &base) {
                    return Some(resolved);
                }
            }

            if let Some(resolved) = resolve_tsconfig_paths(reader, project, specifier) {
                return Some(resolved);
            }

            if let Some(base_url) = project.compiler_options.base_url.as_deref() {
                let base = join_paths(base_url, specifier);
                if let Some(resolved) = probe_path(reader, &base) {
                    return Some(resolved);
                }
            }

            if let Some(resolved) = self.resolve_project_references(reader, project, specifier) {
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
    reader: &dyn crate::traits::WorkspaceAccess,
    project: &IdeProjectConfig,
    specifier: &str,
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
            if let Some(resolved) = probe_path(reader, &candidate) {
                return Some(resolved);
            }
        }
    }

    None
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
        (
            if is_absolute_specifier(prefix_part) {
                normalize_canonical_id(prefix_part)
            } else {
                join_paths(base_url, prefix_part)
            },
            suffix_part.to_string(),
        )
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

fn probe_path(reader: &dyn crate::traits::WorkspaceAccess, base: &str) -> Option<String> {
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

fn resolve_existing_path(
    reader: &dyn crate::traits::WorkspaceAccess,
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

fn probe_extensions() -> &'static [&'static str] {
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
    reader: &dyn crate::traits::WorkspaceAccess,
    importer_id: &str,
    specifier: &str,
    ctx: ResolutionContext,
) -> Option<String> {
    for directory in ancestor_dirs(importer_id) {
        let Some(package_json) = read_json(reader, &join_paths(&directory, "package.json")) else {
            continue;
        };
        let Some(imports) = package_json
            .get("imports")
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
    reader: &dyn crate::traits::WorkspaceAccess,
    importer_id: &str,
    specifier: &str,
    ctx: ResolutionContext,
) -> Option<(String, ResolutionKind)> {
    let (package_name, subpath) = split_package_specifier(specifier)?;
    for directory in ancestor_dirs(importer_id) {
        let package_dir = join_paths(&join_paths(&directory, "node_modules"), &package_name);
        let package_json_path = join_paths(&package_dir, "package.json");

        if let Some(package_json) = read_json(reader, &package_json_path) {
            if let Some(exports) = package_json.get("exports") {
                let export_key = if subpath.is_empty() {
                    ".".to_string()
                } else {
                    format!("./{subpath}")
                };
                if let Some(resolved) =
                    resolve_package_exports(reader, &package_dir, exports, &export_key, ctx)
                {
                    return Some((resolved, ResolutionKind::PackageExports));
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
            if let Some(resolved) = probe_path(reader, &base) {
                return Some((resolved, ResolutionKind::NodeModules));
            }
        }
    }

    None
}

fn resolve_package_exports(
    reader: &dyn crate::traits::WorkspaceAccess,
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
    reader: &dyn crate::traits::WorkspaceAccess,
    package_dir: &str,
    package_json: &serde_json::Value,
    subpath: &str,
    ctx: ResolutionContext,
) -> Option<String> {
    if !subpath.is_empty() {
        return probe_path(reader, &join_paths(package_dir, subpath));
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
        let Some(target) = package_json.get(*key).and_then(|value| value.as_str()) else {
            continue;
        };
        if let Some(resolved) = probe_path(reader, &resolve_package_path(package_dir, target, None))
        {
            return Some(resolved);
        }
    }

    probe_path(reader, &join_paths(package_dir, "index"))
}

fn resolve_package_target(
    reader: &dyn crate::traits::WorkspaceAccess,
    package_dir: &str,
    value: &serde_json::Value,
    captured: Option<&str>,
    ctx: ResolutionContext,
) -> Option<String> {
    match value {
        serde_json::Value::String(target) => {
            probe_path(reader, &resolve_package_path(package_dir, target, captured))
        }
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

fn ancestor_dirs(path: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = parent_dir(path);
    while !current.is_empty() {
        result.push(current.clone());
        let next = parent_dir(&current);
        if next == current {
            break;
        }
        current = next;
    }
    result
}

fn read_json(
    reader: &dyn crate::traits::WorkspaceAccess,
    canonical_id: &str,
) -> Option<serde_json::Value> {
    let text = reader.read_file(canonical_id)?;
    serde_json::from_str(&text).ok()
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

/// Normalize a canonical ID: backslash to slash, lowercase drive letter,
/// strip Windows extended-length prefix.
pub fn normalize_canonical_id(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    // Strip Windows extended-length path prefix (`//?/` or `\\?\`)
    // produced by `std::fs::canonicalize()` on Windows.
    let normalized = if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_string()
    } else {
        normalized
    };
    if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        let mut chars = normalized.chars();
        if let Some(first) = chars.next() {
            return format!("{}{}", first.to_ascii_lowercase(), chars.as_str());
        }
    }
    normalized
}

/// Collapse `.` and `..` segments from a path.
pub fn collapse_path(value: &str) -> String {
    let normalized = normalize_canonical_id(value);
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

// ── Known-file helpers (used by verter_analysis for module reference resolution) ──

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
