use std::path::Path;
use std::sync::Arc;

use verter_host::FileKind;

/// Resolution kind for an import or external source request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveRequestKind {
    EsmImport,
    TypeImport,
    RequireCall,
    SfcSrcAttr,
}

/// Which dependency graph is asking for resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvePhase {
    CodegenBlocker,
    ProviderGraph,
}

/// Where the resolved file should be exposed to the provider layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTarget {
    SourceFile,
    VuePublicApi,
    ShadowSourceFile,
}

/// High-level category describing how the specifier resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionKind {
    Relative,
    TsConfigPath,
    ProjectReference,
    NodeModules,
    PackageExports,
    PackageImports,
    WorkspaceAlias,
    Bundler,
    PlaygroundMap,
}

/// Input for native project resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveRequest {
    pub importer_id: String,
    pub specifier: String,
    pub kind: ResolveRequestKind,
    pub phase: ResolvePhase,
}

/// Output from native project resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveResult {
    pub source_id: String,
    pub provider_id: String,
    pub provider_specifier: String,
    pub file_kind: FileKind,
    pub provider_target: ProviderTarget,
    pub resolution_kind: ResolutionKind,
    pub owner_tsconfig_path: Option<String>,
}

pub trait ProjectResolverReader: Send + Sync {
    fn read_text(&self, canonical_id: &str) -> Option<Arc<str>>;
    fn file_exists(&self, canonical_id: &str) -> bool;
    fn realpath(&self, canonical_id: &str) -> Option<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAlias {
    pub find: String,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IdeProjectCompilerOptions {
    pub base_url: Option<String>,
    pub paths: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectMembership {
    MatchAll,
    IncludeExclude {
        files: Vec<String>,
        include: Vec<String>,
        exclude: Vec<String>,
    },
}

impl Default for ProjectMembership {
    fn default() -> Self {
        Self::MatchAll
    }
}

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
        let provider_root = build_provider_root(&root, tsconfig_path.as_deref());
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NativeProjectResolver {
    projects: Vec<IdeProjectConfig>,
}

impl NativeProjectResolver {
    pub fn new(projects: Vec<IdeProjectConfig>) -> Self {
        let mut projects = projects;
        projects.sort_by(|a, b| compare_projects(a, b));
        Self { projects }
    }

    pub fn owner_for_file(&self, file_id: &str) -> Option<&IdeProjectConfig> {
        self.projects
            .iter()
            .find(|project| project.matches_file(file_id))
    }

    pub fn provider_id_for_source(&self, source_id: &str) -> Option<String> {
        let project = self.owner_for_file(source_id)?;
        let normalized_source = normalize_canonical_id(source_id);
        let normalized_root = normalize_canonical_id(&project.root);
        let relative = normalized_source
            .strip_prefix(&normalized_root)
            .unwrap_or(normalized_source.as_str());
        let relative = relative.trim_start_matches('/');
        if normalized_source.ends_with(".vue") {
            return Some(format!("{}/{relative}.ts", project.provider_root));
        }

        let shadow_name = shadow_file_name(relative);
        Some(format!("{}/{}", project.provider_root, shadow_name))
    }

    pub fn provider_ide_id_for_source(&self, source_id: &str, is_jsx: bool) -> Option<String> {
        let project = self.owner_for_file(source_id)?;
        let normalized_source = normalize_canonical_id(source_id);
        if !normalized_source.ends_with(".vue") {
            return None;
        }

        let normalized_root = normalize_canonical_id(&project.root);
        let relative = normalized_source
            .strip_prefix(&normalized_root)
            .unwrap_or(normalized_source.as_str())
            .trim_start_matches('/');
        let ext = if is_jsx { ".jsx" } else { ".tsx" };
        Some(format!("{}/{}{}", project.provider_root, relative, ext))
    }

    pub fn source_id_from_provider_id(&self, provider_id: &str) -> Option<String> {
        let normalized = normalize_canonical_id(provider_id);
        for project in &self.projects {
            let provider_root = normalize_canonical_id(&project.provider_root);
            if !normalized_starts_with(&normalized, &provider_root) {
                continue;
            }

            let relative = normalized
                .strip_prefix(&provider_root)
                .unwrap_or(normalized.as_str())
                .trim_start_matches('/');

            if relative.ends_with(".vue.ts") {
                let source_relative = relative.strip_suffix(".ts").unwrap_or(relative);
                return Some(join_paths(&project.root, source_relative));
            }
            if relative.ends_with(".vue.tsx") {
                let source_relative = relative.strip_suffix(".tsx").unwrap_or(relative);
                return Some(join_paths(&project.root, source_relative));
            }
            if relative.ends_with(".vue.jsx") {
                let source_relative = relative.strip_suffix(".jsx").unwrap_or(relative);
                return Some(join_paths(&project.root, source_relative));
            }
            if let Some(source_relative) = restore_shadow_file_name(relative) {
                return Some(join_paths(&project.root, &source_relative));
            }
        }

        None
    }

    pub fn resolve_with_reader(
        &self,
        reader: &dyn ProjectResolverReader,
        request: &ResolveRequest,
    ) -> Option<ResolveResult> {
        let importer_owner = self.owner_for_file(&request.importer_id)?;
        let (source_id, resolution_kind) = self.resolve_source_id(
            reader,
            importer_owner,
            &request.importer_id,
            &request.specifier,
            request.kind,
        )?;

        let file_kind = file_kind_for_path(&source_id);
        let target_owner = self.owner_for_file(&source_id);
        let provider_id = target_owner
            .and_then(|_| self.provider_id_for_source(&source_id))
            .unwrap_or_else(|| source_id.clone());
        let provider_target = match target_owner {
            Some(_) if file_kind == FileKind::VueSfc => ProviderTarget::VuePublicApi,
            Some(_) => ProviderTarget::ShadowSourceFile,
            None => ProviderTarget::SourceFile,
        };
        let provider_specifier = if target_owner.is_some() {
            let importer_provider_id = self
                .provider_id_for_source(&request.importer_id)
                .unwrap_or_else(|| normalize_canonical_id(&request.importer_id));
            relative_specifier(&importer_provider_id, &provider_id)
        } else {
            request.specifier.clone()
        };

        Some(ResolveResult {
            owner_tsconfig_path: target_owner.and_then(|project| project.tsconfig_path.clone()),
            source_id,
            provider_id,
            provider_specifier,
            file_kind,
            provider_target,
            resolution_kind,
        })
    }

    fn resolve_source_id(
        &self,
        reader: &dyn ProjectResolverReader,
        importer_owner: &IdeProjectConfig,
        importer_id: &str,
        specifier: &str,
        kind: ResolveRequestKind,
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
            if let Some(resolved) = resolve_package_imports(reader, importer_id, specifier, kind) {
                return Some((resolved, ResolutionKind::PackageImports));
            }
            return None;
        }

        if let Some((resolved, resolution_kind)) =
            resolve_node_modules_package(reader, importer_id, specifier, kind)
        {
            return Some((resolved, resolution_kind));
        }

        None
    }

    fn resolve_project_references(
        &self,
        reader: &dyn ProjectResolverReader,
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

fn normalized_starts_with(path: &str, prefix: &str) -> bool {
    let normalized = normalize_canonical_id(path);
    let prefix = normalize_canonical_id(prefix);
    normalized.starts_with(&prefix)
        && (normalized.len() == prefix.len()
            || prefix.ends_with('/')
            || normalized.as_bytes().get(prefix.len()) == Some(&b'/'))
}

fn build_provider_root(root: &str, tsconfig_path: Option<&str>) -> String {
    let key = tsconfig_path.unwrap_or(root);
    let stable = stable_hash_hex(key);
    format!("{}/.verter/ide/{}", normalize_canonical_id(root), stable)
}

fn shadow_file_name(relative: &str) -> String {
    let path = Path::new(relative);
    let parent = path
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| relative.to_string());
    let ext = path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_else(|| ".ts".to_string());
    let file_name = format!("{stem}.__verter__{ext}");
    match parent {
        Some(parent) if !parent.is_empty() => format!("{parent}/{file_name}"),
        _ => file_name,
    }
}

fn restore_shadow_file_name(relative: &str) -> Option<String> {
    let (parent, file_name) = relative
        .rsplit_once('/')
        .map(|(parent, file_name)| (Some(parent), file_name))
        .unwrap_or((None, relative));
    let restored = file_name.replacen(".__verter__.", ".", 1);
    if restored == file_name {
        return None;
    }

    Some(match parent {
        Some(parent) => format!("{parent}/{restored}"),
        None => restored,
    })
}

fn matches_any_pattern_for_root(path: &str, root: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .map(|pattern| normalize_project_membership_entry(root, pattern, true))
        .filter_map(|pattern| glob::Pattern::new(&pattern).ok())
        .any(|pattern| pattern.matches(path))
}

fn resolve_tsconfig_paths(
    reader: &dyn ProjectResolverReader,
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

fn probe_path(reader: &dyn ProjectResolverReader, base: &str) -> Option<String> {
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

fn resolve_existing_path(reader: &dyn ProjectResolverReader, candidate: &str) -> Option<String> {
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
        ".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs", ".cts", ".cjs", ".vue",
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
    ]
}

fn file_kind_for_path(path: &str) -> FileKind {
    if normalize_canonical_id(path).ends_with(".vue") {
        FileKind::VueSfc
    } else {
        FileKind::NonSfc
    }
}

fn is_relative_specifier(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

fn is_absolute_specifier(specifier: &str) -> bool {
    specifier.starts_with('/')
        || Path::new(specifier).is_absolute()
        || specifier.as_bytes().get(1) == Some(&b':')
}

fn parent_dir(path: &str) -> String {
    let normalized = normalize_canonical_id(path);
    normalized
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

fn join_paths(base: &str, path: &str) -> String {
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

fn collapse_path(value: &str) -> String {
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

fn normalize_canonical_id(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        let mut chars = normalized.chars();
        if let Some(first) = chars.next() {
            return format!("{}{}", first.to_ascii_lowercase(), chars.as_str());
        }
    }
    normalized
}

fn stable_hash_hex(input: &str) -> String {
    let mut hash = 14695981039346656037u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{hash:016x}")
}

fn resolve_package_imports(
    reader: &dyn ProjectResolverReader,
    importer_id: &str,
    specifier: &str,
    kind: ResolveRequestKind,
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
            resolve_package_target(reader, &directory, entry, captured.as_deref(), kind)
        {
            return Some(resolved);
        }
    }

    None
}

fn resolve_node_modules_package(
    reader: &dyn ProjectResolverReader,
    importer_id: &str,
    specifier: &str,
    kind: ResolveRequestKind,
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
                    resolve_package_exports(reader, &package_dir, exports, &export_key, kind)
                {
                    return Some((resolved, ResolutionKind::PackageExports));
                }

                continue;
            }

            if let Some(resolved) =
                resolve_legacy_package(reader, &package_dir, &package_json, subpath)
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
    reader: &dyn ProjectResolverReader,
    package_dir: &str,
    exports: &serde_json::Value,
    export_key: &str,
    kind: ResolveRequestKind,
) -> Option<String> {
    match exports {
        serde_json::Value::String(_) | serde_json::Value::Array(_) => {
            if export_key == "." {
                resolve_package_target(reader, package_dir, exports, None, kind)
            } else {
                None
            }
        }
        serde_json::Value::Object(map) => {
            if !map.keys().any(|key| key.starts_with('.')) {
                if export_key == "." {
                    return resolve_package_target(reader, package_dir, exports, None, kind);
                }
                return None;
            }

            let (entry, captured) = match_package_mapping(map, export_key)?;
            resolve_package_target(reader, package_dir, entry, captured.as_deref(), kind)
        }
        _ => None,
    }
}

fn resolve_legacy_package(
    reader: &dyn ProjectResolverReader,
    package_dir: &str,
    package_json: &serde_json::Value,
    subpath: &str,
) -> Option<String> {
    if !subpath.is_empty() {
        return probe_path(reader, &join_paths(package_dir, subpath));
    }

    for key in ["types", "typings", "main"] {
        let Some(target) = package_json.get(key).and_then(|value| value.as_str()) else {
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
    reader: &dyn ProjectResolverReader,
    package_dir: &str,
    value: &serde_json::Value,
    captured: Option<&str>,
    kind: ResolveRequestKind,
) -> Option<String> {
    match value {
        serde_json::Value::String(target) => {
            probe_path(reader, &resolve_package_path(package_dir, target, captured))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|item| resolve_package_target(reader, package_dir, item, captured, kind)),
        serde_json::Value::Object(map) => {
            for condition in package_conditions(kind) {
                let Some(entry) = map.get(*condition) else {
                    continue;
                };
                if let Some(resolved) =
                    resolve_package_target(reader, package_dir, entry, captured, kind)
                {
                    return Some(resolved);
                }
            }
            None
        }
        _ => None,
    }
}

fn package_conditions(kind: ResolveRequestKind) -> &'static [&'static str] {
    match kind {
        ResolveRequestKind::RequireCall => &["types", "require", "default"],
        ResolveRequestKind::EsmImport
        | ResolveRequestKind::TypeImport
        | ResolveRequestKind::SfcSrcAttr => &["types", "import", "default"],
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

fn read_json(reader: &dyn ProjectResolverReader, canonical_id: &str) -> Option<serde_json::Value> {
    let text = reader.read_text(canonical_id)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn project(
        root: &str,
        workspace_root: &str,
        tsconfig_path: Option<&str>,
        membership: ProjectMembership,
    ) -> IdeProjectConfig {
        let mut project = IdeProjectConfig::new(
            root.to_string(),
            workspace_root.to_string(),
            tsconfig_path.map(str::to_string),
        );
        project.membership = membership;
        project
    }

    #[derive(Default)]
    struct TestReader {
        files: HashSet<String>,
        texts: HashMap<String, Arc<str>>,
        realpaths: HashMap<String, String>,
    }

    impl TestReader {
        fn with_files(paths: &[&str]) -> Self {
            let mut reader = Self::default();
            for path in paths {
                let normalized = normalize_canonical_id(path);
                reader.files.insert(normalized.clone());
                reader
                    .texts
                    .insert(normalized, Arc::<str>::from("// test file"));
            }
            reader
        }

        fn add_file(&mut self, path: &str, text: &str) {
            let normalized = normalize_canonical_id(path);
            self.files.insert(normalized.clone());
            self.texts
                .insert(normalized, Arc::<str>::from(text.to_string()));
        }

        fn add_realpath(&mut self, path: &str, realpath: &str) {
            self.realpaths.insert(
                normalize_canonical_id(path),
                normalize_canonical_id(realpath),
            );
        }
    }

    impl ProjectResolverReader for TestReader {
        fn read_text(&self, canonical_id: &str) -> Option<Arc<str>> {
            self.texts
                .get(&normalize_canonical_id(canonical_id))
                .cloned()
        }

        fn file_exists(&self, canonical_id: &str) -> bool {
            self.files.contains(&normalize_canonical_id(canonical_id))
        }

        fn realpath(&self, canonical_id: &str) -> Option<String> {
            self.realpaths
                .get(&normalize_canonical_id(canonical_id))
                .cloned()
                .or_else(|| {
                    self.file_exists(canonical_id)
                        .then(|| normalize_canonical_id(canonical_id))
                })
        }
    }

    #[test]
    fn owner_selection_ignores_solution_style_root_membership() {
        let resolver = NativeProjectResolver::new(vec![
            project(
                "/workspace",
                "/workspace",
                Some("/workspace/tsconfig.json"),
                ProjectMembership::IncludeExclude {
                    files: Vec::new(),
                    include: Vec::new(),
                    exclude: Vec::new(),
                },
            ),
            project(
                "/workspace",
                "/workspace",
                Some("/workspace/tsconfig.app.json"),
                ProjectMembership::IncludeExclude {
                    files: Vec::new(),
                    include: vec!["src/**/*".to_string()],
                    exclude: vec!["tests/**/*".to_string()],
                },
            ),
        ]);

        let owner = resolver
            .owner_for_file("/workspace/src/App.vue")
            .expect("src/App.vue should have an owner project");

        assert_eq!(
            owner.tsconfig_path.as_deref(),
            Some("/workspace/tsconfig.app.json"),
            "membership-aware owner selection should skip solution-style tsconfig.json"
        );
        assert_ne!(
            owner.tsconfig_path.as_deref(),
            Some("/workspace/tsconfig.json"),
            "solution-style tsconfig.json must not win when it owns no files"
        );
    }

    #[test]
    fn provider_paths_use_synthetic_workspace_project_for_unmatched_files() {
        let resolver = NativeProjectResolver::new(vec![
            project(
                "/workspace",
                "/workspace",
                Some("/workspace/tsconfig.app.json"),
                ProjectMembership::IncludeExclude {
                    files: Vec::new(),
                    include: vec!["src/**/*".to_string()],
                    exclude: Vec::new(),
                },
            ),
            project(
                "/workspace",
                "/workspace",
                None,
                ProjectMembership::MatchAll,
            ),
        ]);

        let provider_id = resolver
            .provider_id_for_source("/workspace/scripts/tool.ts")
            .expect("unmatched file should still receive a provider shadow path");

        assert!(
            provider_id.contains("/.verter/ide/"),
            "provider path should use a synthetic workspace root: {provider_id}"
        );
        assert!(
            provider_id.ends_with("/scripts/tool.__verter__.ts"),
            "non-Vue workspace files should be materialized as shadow source files: {provider_id}"
        );
        assert!(
            !provider_id.ends_with("/scripts/tool.ts"),
            "provider path must not use the raw workspace source path: {provider_id}"
        );
    }

    #[test]
    fn provider_paths_keep_vue_as_public_api_targets() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);

        let provider_id = resolver
            .provider_id_for_source("/workspace/src/App.vue")
            .expect("vue files should be rewritten to public API provider paths");

        assert!(
            provider_id.ends_with("/src/App.vue.ts"),
            "Vue files should resolve to .vue.ts in the provider graph: {provider_id}"
        );
        assert!(
            !provider_id.ends_with("/src/App.vue"),
            "provider graph must not expose raw .vue source IDs"
        );
    }

    #[test]
    fn provider_paths_keep_vue_ide_files_under_synthetic_project_roots() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);

        let provider_id = resolver
            .provider_ide_id_for_source("/workspace/src/App.vue", false)
            .expect("vue IDE files should receive synthetic provider IDs");

        assert!(
            provider_id.contains("/.verter/ide/"),
            "IDE provider path should live under the synthetic provider root: {provider_id}"
        );
        assert!(
            provider_id.ends_with("/src/App.vue.tsx"),
            "Vue IDE provider paths should use .vue.tsx for TypeScript files: {provider_id}"
        );
        assert!(
            !provider_id.starts_with("/workspace/src/App.vue"),
            "provider IDE path must not expose the raw workspace source path"
        );
    }

    #[test]
    fn provider_paths_round_trip_back_to_source_ids() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);

        let vue_api = resolver
            .provider_id_for_source("/workspace/src/App.vue")
            .expect("vue API file should resolve");
        let vue_ide = resolver
            .provider_ide_id_for_source("/workspace/src/App.vue", true)
            .expect("vue IDE file should resolve");
        let shadow = resolver
            .provider_id_for_source("/workspace/src/utils.ts")
            .expect("shadow file should resolve");

        assert_eq!(
            resolver.source_id_from_provider_id(&vue_api).as_deref(),
            Some("/workspace/src/App.vue")
        );
        assert_eq!(
            resolver.source_id_from_provider_id(&vue_ide).as_deref(),
            Some("/workspace/src/App.vue")
        );
        assert_eq!(
            resolver.source_id_from_provider_id(&shadow).as_deref(),
            Some("/workspace/src/utils.ts")
        );
    }

    #[test]
    fn resolve_relative_vue_import_returns_real_source_and_provider_api() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);
        let reader = TestReader::with_files(&["/workspace/src/Foo.vue"]);

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "./Foo.vue".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("relative .vue import should resolve");

        assert_eq!(resolved.source_id, "/workspace/src/Foo.vue");
        assert_eq!(resolved.file_kind, FileKind::VueSfc);
        assert_eq!(resolved.provider_target, ProviderTarget::VuePublicApi);
        assert_eq!(resolved.resolution_kind, ResolutionKind::Relative);
        assert_eq!(resolved.provider_specifier, "./Foo.vue.ts");
        assert!(
            resolved.provider_id.ends_with("/src/Foo.vue.ts"),
            "provider graph should target the materialized .vue.ts API file: {}",
            resolved.provider_id
        );
    }

    #[test]
    fn resolve_workspace_alias_rewrites_to_shadow_provider_file() {
        let mut app_project = project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        );
        app_project.workspace_aliases = vec![WorkspaceAlias {
            find: "@/".to_string(),
            replacement: "/workspace/src/".to_string(),
        }];
        let resolver = NativeProjectResolver::new(vec![app_project]);
        let reader = TestReader::with_files(&["/workspace/src/utils.ts"]);

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "@/utils".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("workspace alias should resolve");

        assert_eq!(resolved.source_id, "/workspace/src/utils.ts");
        assert_eq!(resolved.file_kind, FileKind::NonSfc);
        assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
        assert_eq!(resolved.resolution_kind, ResolutionKind::WorkspaceAlias);
        assert_eq!(resolved.provider_specifier, "./utils.__verter__.ts");
        assert!(
            resolved.provider_id.ends_with("/src/utils.__verter__.ts"),
            "non-Vue workspace files should resolve to provider shadow files: {}",
            resolved.provider_id
        );
    }

    #[test]
    fn resolve_tsconfig_paths_before_base_url() {
        let mut app_project = project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        );
        app_project.compiler_options = IdeProjectCompilerOptions {
            base_url: Some("/workspace/src".to_string()),
            paths: vec![(
                "shared".to_string(),
                vec!["../generated/shared".to_string()],
            )],
        };
        let resolver = NativeProjectResolver::new(vec![app_project]);
        let reader =
            TestReader::with_files(&["/workspace/generated/shared.ts", "/workspace/src/shared.ts"]);

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "shared".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("tsconfig paths should resolve before baseUrl fallback");

        assert_eq!(resolved.source_id, "/workspace/generated/shared.ts");
        assert_eq!(resolved.resolution_kind, ResolutionKind::TsConfigPath);
        assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
        assert_eq!(
            resolved.provider_specifier,
            "../generated/shared.__verter__.ts"
        );
        assert!(
            !resolved.provider_id.ends_with("/src/shared.__verter__.ts"),
            "baseUrl fallback must not win when tsconfig paths has a match: {}",
            resolved.provider_id
        );
    }

    #[test]
    fn resolve_base_url_when_no_paths_match() {
        let mut app_project = project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        );
        app_project.compiler_options = IdeProjectCompilerOptions {
            base_url: Some("/workspace/src".to_string()),
            paths: Vec::new(),
        };
        let resolver = NativeProjectResolver::new(vec![app_project]);
        let reader = TestReader::with_files(&["/workspace/src/shared.ts"]);

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "shared".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("baseUrl fallback should resolve when paths has no match");

        assert_eq!(resolved.source_id, "/workspace/src/shared.ts");
        assert_eq!(resolved.resolution_kind, ResolutionKind::TsConfigPath);
        assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
        assert_eq!(resolved.provider_specifier, "./shared.__verter__.ts");
    }

    #[test]
    fn resolve_relative_paths_use_realpath_normalization() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);
        let mut reader = TestReader::with_files(&["/workspace/src/linked/util.ts"]);
        reader.add_realpath(
            "/workspace/src/linked/util.ts",
            "/workspace/src/shared/util.ts",
        );

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "./linked/util".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("relative import should resolve through the reader realpath");

        assert_eq!(resolved.source_id, "/workspace/src/shared/util.ts");
        assert_eq!(resolved.resolution_kind, ResolutionKind::Relative);
        assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
        assert_eq!(resolved.provider_specifier, "./shared/util.__verter__.ts");
        assert!(
            resolved
                .provider_id
                .ends_with("/src/shared/util.__verter__.ts"),
            "provider path should be derived from the canonical realpath target: {}",
            resolved.provider_id
        );
    }

    #[test]
    fn resolve_project_references_after_local_tsconfig_options() {
        let mut app_project = project(
            "/workspace/packages/app",
            "/workspace",
            Some("/workspace/packages/app/tsconfig.json"),
            ProjectMembership::MatchAll,
        );
        app_project.compiler_options = IdeProjectCompilerOptions {
            base_url: Some("/workspace/packages/app/src".to_string()),
            paths: Vec::new(),
        };
        app_project.references = vec!["/workspace/packages/shared/tsconfig.json".to_string()];

        let mut shared_project = project(
            "/workspace/packages/shared",
            "/workspace",
            Some("/workspace/packages/shared/tsconfig.json"),
            ProjectMembership::MatchAll,
        );
        shared_project.compiler_options = IdeProjectCompilerOptions {
            base_url: Some("/workspace/packages/shared/src".to_string()),
            paths: vec![("shared".to_string(), vec!["index".to_string()])],
        };

        let resolver = NativeProjectResolver::new(vec![app_project, shared_project]);
        let reader = TestReader::with_files(&["/workspace/packages/shared/src/index.ts"]);

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/packages/app/src/App.ts".to_string(),
                    specifier: "shared".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("project references should be consulted after local tsconfig resolution");

        let expected_provider_id = resolver
            .provider_id_for_source("/workspace/packages/shared/src/index.ts")
            .expect("referenced source should receive a provider path");
        let expected_importer_provider_id = resolver
            .provider_id_for_source("/workspace/packages/app/src/App.ts")
            .expect("importer should receive a provider path");

        assert_eq!(
            resolved.source_id,
            "/workspace/packages/shared/src/index.ts"
        );
        assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
        assert_eq!(resolved.provider_id, expected_provider_id);
        assert_eq!(
            resolved.provider_specifier,
            relative_specifier(&expected_importer_provider_id, &expected_provider_id)
        );
        assert_eq!(
            resolved.owner_tsconfig_path.as_deref(),
            Some("/workspace/packages/shared/tsconfig.json")
        );
    }

    #[test]
    fn resolve_package_imports_from_nearest_package_json() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);
        let mut reader = TestReader::with_files(&["/workspace/src/utils.ts"]);
        reader.add_file(
            "/workspace/package.json",
            r##"{
                "imports": {
                    "#utils": "./src/utils.ts"
                }
            }"##,
        );

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "#utils".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("package imports should resolve through the nearest package.json");

        assert_eq!(resolved.source_id, "/workspace/src/utils.ts");
        assert_eq!(resolved.resolution_kind, ResolutionKind::PackageImports);
        assert_eq!(resolved.provider_target, ProviderTarget::ShadowSourceFile);
        assert_eq!(resolved.provider_specifier, "./utils.__verter__.ts");
    }

    #[test]
    fn resolve_package_exports_prefers_types_for_root_imports() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);
        let mut reader = TestReader::with_files(&[
            "/workspace/node_modules/lib/dist/index.d.ts",
            "/workspace/node_modules/lib/dist/index.mjs",
            "/workspace/node_modules/lib/dist/index.cjs",
        ]);
        reader.add_file(
            "/workspace/node_modules/lib/package.json",
            r#"{
                "exports": {
                    ".": {
                        "types": "./dist/index.d.ts",
                        "import": "./dist/index.mjs",
                        "require": "./dist/index.cjs",
                        "default": "./dist/index.mjs"
                    }
                }
            }"#,
        );

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "lib".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("package exports should resolve package root imports");

        assert_eq!(
            resolved.source_id,
            "/workspace/node_modules/lib/dist/index.d.ts"
        );
        assert_eq!(resolved.resolution_kind, ResolutionKind::PackageExports);
        assert_eq!(resolved.provider_target, ProviderTarget::SourceFile);
        assert_eq!(resolved.provider_specifier, "lib");
        assert_eq!(resolved.provider_id, resolved.source_id);
    }

    #[test]
    fn resolve_package_exports_distinguishes_import_and_require() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);
        let mut reader = TestReader::with_files(&[
            "/workspace/node_modules/lib/dist/feature.mjs",
            "/workspace/node_modules/lib/dist/feature.cjs",
        ]);
        reader.add_file(
            "/workspace/node_modules/lib/package.json",
            r#"{
                "exports": {
                    "./feature": {
                        "import": "./dist/feature.mjs",
                        "require": "./dist/feature.cjs",
                        "default": "./dist/feature.mjs"
                    }
                }
            }"#,
        );

        let esm = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "lib/feature".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("ESM import should resolve package exports");
        let require = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "lib/feature".to_string(),
                    kind: ResolveRequestKind::RequireCall,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("require call should resolve package exports");

        assert_eq!(
            esm.source_id,
            "/workspace/node_modules/lib/dist/feature.mjs"
        );
        assert_eq!(
            require.source_id,
            "/workspace/node_modules/lib/dist/feature.cjs"
        );
        assert_eq!(esm.resolution_kind, ResolutionKind::PackageExports);
        assert_eq!(require.resolution_kind, ResolutionKind::PackageExports);
        assert_ne!(
            esm.source_id, require.source_id,
            "import and require must be able to choose different export conditions"
        );
    }

    #[test]
    fn resolve_node_modules_prefers_typings_before_main() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);
        let mut reader = TestReader::with_files(&[
            "/workspace/node_modules/legacy/dist/index.d.ts",
            "/workspace/node_modules/legacy/dist/index.js",
        ]);
        reader.add_file(
            "/workspace/node_modules/legacy/package.json",
            r#"{
                "typings": "./dist/index.d.ts",
                "main": "./dist/index.js"
            }"#,
        );

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "legacy".to_string(),
                    kind: ResolveRequestKind::EsmImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("legacy package resolution should prefer typings before main");

        assert_eq!(
            resolved.source_id,
            "/workspace/node_modules/legacy/dist/index.d.ts"
        );
        assert_eq!(resolved.resolution_kind, ResolutionKind::NodeModules);
        assert_eq!(resolved.provider_target, ProviderTarget::SourceFile);
        assert_eq!(resolved.provider_specifier, "legacy");
    }

    #[test]
    fn resolve_node_modules_falls_back_to_main_without_type_entries() {
        let resolver = NativeProjectResolver::new(vec![project(
            "/workspace",
            "/workspace",
            Some("/workspace/tsconfig.app.json"),
            ProjectMembership::MatchAll,
        )]);
        let mut reader =
            TestReader::with_files(&["/workspace/node_modules/legacy-main/dist/index.js"]);
        reader.add_file(
            "/workspace/node_modules/legacy-main/package.json",
            r#"{
                "main": "./dist/index.js"
            }"#,
        );

        let resolved = resolver
            .resolve_with_reader(
                &reader,
                &ResolveRequest {
                    importer_id: "/workspace/src/App.ts".to_string(),
                    specifier: "legacy-main".to_string(),
                    kind: ResolveRequestKind::RequireCall,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
            .expect("legacy package resolution should fall back to main when no types exist");

        assert_eq!(
            resolved.source_id,
            "/workspace/node_modules/legacy-main/dist/index.js"
        );
        assert_eq!(resolved.resolution_kind, ResolutionKind::NodeModules);
        assert_eq!(resolved.provider_target, ProviderTarget::SourceFile);
        assert_eq!(resolved.provider_specifier, "legacy-main");
    }
}
