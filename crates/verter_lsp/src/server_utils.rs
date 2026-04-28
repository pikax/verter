use super::*;

/// Resolve a relative import path against an importer's directory.
///
/// Handles `./foo.vue`, `../bar/baz.vue`, etc.
/// Check whether a requested `context.only` filter includes the given code action kind.
///
/// Check whether a canonical ID (or URI string) refers to a config file
/// that should trigger a project registry rebuild when changed on disk.
///
/// Matches: `tsconfig*.json`, `.verterrc.json`, `vite.config.{ts,js,...}`, `package.json`.
pub(super) fn is_config_file(path: &str) -> bool {
    // No config file inside node_modules should trigger a registry rebuild.
    if path.contains("/node_modules/") {
        return false;
    }
    // Extract the filename (last segment after '/')
    let filename = path.rsplit('/').next().unwrap_or(path);
    if filename.starts_with("tsconfig") && filename.ends_with(".json") {
        return true;
    }
    if filename == ".verterrc.json" || filename == "package.json" {
        return true;
    }
    // vite.config.{ts,js,mjs,cjs,mts,cts}
    if let Some(ext) = filename.strip_prefix("vite.config.") {
        return matches!(ext, "ts" | "js" | "mjs" | "cjs" | "mts" | "cts");
    }
    false
}

/// Check whether a canonical ID (or URI string) refers to a `.vue` file.
pub(super) fn is_vue_file(path: &str) -> bool {
    path.ends_with(".vue")
}

/// When `only` is `None` (no filter), all kinds are wanted.
/// Otherwise, checks for hierarchical prefix matching (LSP spec):
/// `"quickfix"` matches `"quickfix.foo"` and vice-versa.
pub(super) fn wants_code_action_kind(only: Option<&[CodeActionKind]>, kind: &str) -> bool {
    match only {
        None => true,
        Some(kinds) => kinds.iter().any(|k| {
            let k = k.as_str();
            k == kind
                || kind.starts_with(k) && kind.as_bytes().get(k.len()) == Some(&b'.')
                || k.starts_with(kind) && k.as_bytes().get(kind.len()) == Some(&b'.')
        }),
    }
}

/// Does NOT handle alias imports (e.g., `@/components/Foo.vue`).
/// Build the list of workspace components available for auto-import.
///
/// Scans all known .vue files in the host, derives PascalCase names from filenames,
/// and computes relative import paths from the current file.
pub(super) fn build_workspace_components(
    host: &verter_session::VerterHost,
    current_file_id: &str,
) -> Vec<crate::features::completion::WorkspaceComponent> {
    let files = host.list_files();
    let current_dir = current_file_id
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("");

    let mut components = Vec::new();

    for (file_id, kind) in &files {
        // Only .vue files
        if *kind != verter_session::FileKind::VueSfc {
            continue;
        }
        // Skip the current file
        if file_id == current_file_id {
            continue;
        }
        // Skip node_modules
        if file_id.contains("node_modules") {
            continue;
        }

        // Derive component name from filename: `src/components/MyButton.vue` → `MyButton`
        let filename = file_id.rsplit('/').next().unwrap_or(file_id);
        let stem = filename.strip_suffix(".vue").unwrap_or(filename);
        if stem.is_empty() {
            continue;
        }

        // Convert to PascalCase: `my-button` → `MyButton`, `index` stays `Index`
        let component_name = to_pascal_case(stem);

        // Prefer alias-based path (e.g. @/components/Foo.vue) over relative
        let import_path = host
            .preferred_specifier(current_file_id, file_id)
            .unwrap_or_else(|| compute_relative_path(current_dir, file_id));

        components.push(crate::features::completion::WorkspaceComponent {
            name: component_name,
            import_path,
        });
    }

    components
}

/// Convert a kebab-case or mixed-case filename stem to PascalCase.
pub(super) fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Compute a relative path from `from_dir` to `to_file`.
pub(super) fn compute_relative_path(from_dir: &str, to_file: &str) -> String {
    let from_parts: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to_file.split('/').filter(|s| !s.is_empty()).collect();

    // Find common prefix length
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let ups = from_parts.len() - common;
    let remaining = &to_parts[common..];

    if ups == 0 {
        format!("./{}", remaining.join("/"))
    } else {
        let up_str = "../".repeat(ups);
        format!("{}{}", up_str, remaining.join("/"))
    }
}

pub(crate) fn quote_wrapped_specifier(raw_text: &str, specifier: &str) -> String {
    let quote = match raw_text.chars().next() {
        Some('\'') => '\'',
        Some('"') => '"',
        Some('`') => '`',
        _ => '\'',
    };
    format!("{quote}{specifier}{quote}")
}

pub(super) fn provider_ide_path_for_source(
    resolver: &crate::project_resolver::NativeProjectResolver,
    canonical_id: &str,
    is_jsx: bool,
) -> Option<String> {
    resolver.provider_ide_id_for_source(canonical_id, is_jsx)
}

#[cfg(test)]
pub(super) fn provider_api_path_for_source(
    resolver: &crate::project_resolver::NativeProjectResolver,
    canonical_id: &str,
) -> Option<String> {
    resolver.provider_id_for_source(canonical_id)
}

pub(super) fn source_id_from_provider_vue_path(
    resolver: &crate::project_resolver::NativeProjectResolver,
    host: &verter_session::VerterHost,
    provider_path: &str,
) -> Option<String> {
    let candidate = resolver.source_id_from_provider_id(provider_path)?;
    // Collision guard: verify backing .vue source exists in host.
    // A real .vue.tsx on disk under a project root would incorrectly match
    // project ownership for .vue even though no .vue file was compiled.
    if candidate.ends_with(".vue") && host.get_source(&candidate).is_none() {
        return None;
    }
    Some(candidate)
}

/// Extract the identifier word surrounding a byte offset within a given span.
/// Returns `None` if the offset is not on an identifier character.
pub(super) fn extract_word_at_offset(
    source: &[u8],
    offset: u32,
    span: verter_span::Span,
) -> Option<String> {
    let off = offset as usize;
    let start_bound = span.start as usize;
    let end_bound = span.end as usize;
    if off >= source.len() || off < start_bound || off >= end_bound {
        return None;
    }
    if !source[off].is_ascii_alphanumeric() && source[off] != b'_' && source[off] != b'$' {
        return None;
    }
    let mut word_start = off;
    while word_start > start_bound
        && (source[word_start - 1].is_ascii_alphanumeric()
            || source[word_start - 1] == b'_'
            || source[word_start - 1] == b'$')
    {
        word_start -= 1;
    }
    let mut word_end = off;
    while word_end < end_bound
        && word_end < source.len()
        && (source[word_end].is_ascii_alphanumeric()
            || source[word_end] == b'_'
            || source[word_end] == b'$')
    {
        word_end += 1;
    }
    if word_start == word_end {
        return None;
    }
    String::from_utf8(source[word_start..word_end].to_vec()).ok()
}

pub(super) struct LspProjectResolverReader<'a> {
    pub(super) documents: &'a DocumentRegistry,
}

impl<'a> LspProjectResolverReader<'a> {
    pub(super) fn new(documents: &'a DocumentRegistry) -> Self {
        Self { documents }
    }
}

impl verter_workspace::WorkspaceAccess for LspProjectResolverReader<'_> {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        // Try host cache first (already-upserted files), then workspace (disk).
        self.documents
            .host()
            .get_source(canonical_id)
            .or_else(|| self.documents.host().workspace().read_file(canonical_id))
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.documents.host().get_source(canonical_id).is_some()
            || self.documents.host().workspace().file_exists(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        if self.documents.host().get_source(canonical_id).is_some() {
            return Some(canonical_id.replace('\\', "/"));
        }
        self.documents.host().workspace().realpath(canonical_id)
    }

    // ── Reader-only stub overrides for reverse-graph methods (R6/R7) ──
    //
    // Rationale (§2.16b): `LspProjectResolverReader` is a thin file-read
    // adapter passed to the project resolver for source/manifest reads
    // during import resolution. It does not participate in dep-flow; the
    // host's workspace owns those edges.
    fn record_parsed_edges(&self, _canonical_id: &str, _edges: &[verter_workspace::ParsedEdge]) {}

    fn reverse_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    fn forward_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    fn set_exact_resolutions(
        &self,
        _canonical_id: &str,
        _resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        verter_workspace::ExactResolutionResult::default()
    }

    fn replace_semantic_transitive(
        &self,
        _canonical_id: &str,
        _deps: std::collections::BTreeSet<String>,
    ) {
    }

    fn set_default_resolve_extensions(&self, _host_extensions: Vec<String>) {}

    fn dependency_snapshot(
        &self,
        _canonical_id: &str,
    ) -> Option<verter_workspace::DependencySnapshotView> {
        None
    }

    fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
}

pub(crate) fn rewrite_non_vue_source_with_resolver(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_workspace::WorkspaceAccess,
    importer_id: &str,
    source: &str,
    module_references: &[verter_session::ScriptModuleReference],
) -> String {
    let mut rewritten = source.to_string();
    let mut replacements: Vec<(usize, usize, String)> = module_references
        .iter()
        .filter_map(|reference| {
            if reference.analyzability
                != verter_semantic::analysis::ModuleReferenceAnalyzability::Exact
            {
                return None;
            }

            let specifier = reference.literal_specifier.as_ref()?;
            let resolved = resolver.resolve_with_reader(
                reader,
                &crate::project_resolver::ResolveRequest {
                    importer_id: importer_id.to_string(),
                    specifier: specifier.clone(),
                    kind: module_reference_request_kind(reference),
                    phase: crate::project_resolver::ResolvePhase::ProviderGraph,
                },
            )?;

            let start = reference.expr_span.start as usize;
            let end = reference.expr_span.end as usize;
            source.get(start..end)?;

            Some((
                start,
                end,
                quote_wrapped_specifier(&reference.raw_text, &resolved.provider_specifier),
            ))
        })
        .collect();

    replacements.sort_by(|left, right| right.0.cmp(&left.0));
    for (start, end, replacement) in replacements {
        rewritten.replace_range(start..end, &replacement);
    }

    rewritten
}

pub(crate) fn prepare_non_vue_provider_sync(
    snapshot: Option<&super::PublishedResolverSnapshot>,
    reader: &dyn verter_workspace::WorkspaceAccess,
    importer_id: &str,
    source: &str,
    module_references: &[verter_session::ScriptModuleReference],
) -> Option<PreparedNonVueProviderSync> {
    let snapshot = snapshot?;
    let provider_path = snapshot.resolver.provider_id_for_source(importer_id)?;
    let rewritten = rewrite_non_vue_source_with_resolver(
        &snapshot.resolver,
        reader,
        importer_id,
        source,
        module_references,
    );
    let resolved_dependencies = collect_resolved_provider_dependencies(
        &snapshot.resolver,
        reader,
        importer_id,
        module_references,
    );

    Some(PreparedNonVueProviderSync {
        provider_path,
        rewritten,
        resolved_dependencies,
    })
}

pub(crate) fn collect_resolved_provider_dependencies(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_workspace::WorkspaceAccess,
    importer_id: &str,
    module_references: &[verter_session::ScriptModuleReference],
) -> Vec<crate::project_resolver::ResolveResult> {
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();

    for reference in module_references {
        let kind = module_reference_request_kind(reference);
        match reference.analyzability {
            verter_semantic::analysis::ModuleReferenceAnalyzability::Exact => {
                if let Some(specifier) = &reference.literal_specifier {
                    if let Some(result) = resolver.resolve_with_reader(
                        reader,
                        &crate::project_resolver::ResolveRequest {
                            importer_id: importer_id.to_string(),
                            specifier: specifier.clone(),
                            kind,
                            phase: crate::project_resolver::ResolvePhase::ProviderGraph,
                        },
                    ) {
                        let key = (result.source_id.clone(), result.provider_id.clone());
                        if seen.insert(key) {
                            resolved.push(result);
                        }
                    }
                }
            }
            verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet => {
                for specifier in &reference.finite_specifiers {
                    if let Some(result) = resolver.resolve_with_reader(
                        reader,
                        &crate::project_resolver::ResolveRequest {
                            importer_id: importer_id.to_string(),
                            specifier: specifier.clone(),
                            kind,
                            phase: crate::project_resolver::ResolvePhase::ProviderGraph,
                        },
                    ) {
                        let key = (result.source_id.clone(), result.provider_id.clone());
                        if seen.insert(key) {
                            resolved.push(result);
                        }
                    }
                }
            }
            verter_semantic::analysis::ModuleReferenceAnalyzability::UnknownDynamic => {}
        }
    }

    resolved
}

pub(super) fn collect_resolved_provider_dependencies_from_analyzed_refs(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_workspace::WorkspaceAccess,
    importer_id: &str,
    module_references: &[verter_semantic::analysis::AnalyzedModuleReference],
) -> Vec<crate::project_resolver::ResolveResult> {
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();

    for reference in module_references {
        let specifiers: Vec<&str> = if let Some(specifier) = reference.literal_specifier.as_deref()
        {
            vec![specifier]
        } else {
            reference
                .finite_specifiers
                .iter()
                .map(String::as_str)
                .collect()
        };

        for specifier in specifiers {
            if let Some(result) = resolver.resolve_with_reader(
                reader,
                &crate::project_resolver::ResolveRequest {
                    importer_id: importer_id.to_string(),
                    specifier: specifier.to_string(),
                    kind: analyzed_module_reference_request_kind(reference),
                    phase: crate::project_resolver::ResolvePhase::ProviderGraph,
                },
            ) {
                let key = (result.source_id.clone(), result.provider_id.clone());
                if seen.insert(key) {
                    resolved.push(result);
                }
            }
        }
    }

    resolved
}

pub(crate) fn module_reference_request_kind(
    reference: &verter_session::ScriptModuleReference,
) -> crate::project_resolver::ResolveRequestKind {
    if reference.is_type_only {
        crate::project_resolver::ResolveRequestKind::TypeImport
    } else if reference.semantics == verter_semantic::analysis::ModuleReferenceSemantics::Require {
        crate::project_resolver::ResolveRequestKind::RequireCall
    } else {
        crate::project_resolver::ResolveRequestKind::EsmImport
    }
}

pub(super) fn analyzed_module_reference_request_kind(
    reference: &verter_semantic::analysis::AnalyzedModuleReference,
) -> crate::project_resolver::ResolveRequestKind {
    if reference.is_type_only {
        crate::project_resolver::ResolveRequestKind::TypeImport
    } else if reference.semantics == verter_semantic::analysis::ModuleReferenceSemantics::Require {
        crate::project_resolver::ResolveRequestKind::RequireCall
    } else {
        crate::project_resolver::ResolveRequestKind::EsmImport
    }
}

/// Check if a resolved import path matches a target file path.
///
/// Handles cases where the import source omits the `.vue` extension:
/// - `./Popup` → matches `./Popup.vue`
/// - `./Popover` → matches `./Popover/index.vue` or `./Popover/Popover.vue`
pub(super) fn import_resolved_matches_target(resolved: &str, target: &str) -> bool {
    if resolved == target {
        return true;
    }
    // Skip if resolved already has .vue extension — no fuzzy matching needed
    if resolved.ends_with(".vue") {
        return false;
    }
    // Try: resolved + ".vue"
    if target == format!("{resolved}.vue") {
        return true;
    }
    // Try: resolved/index.vue
    if target == format!("{resolved}/index.vue") {
        return true;
    }
    // Try: resolved/Name.vue where Name is the last segment of resolved
    if let Some(last) = resolved.rsplit('/').next() {
        if !last.is_empty() && target == format!("{resolved}/{last}.vue") {
            return true;
        }
    }
    false
}

/// Resolve a component's analysis snapshot from an import source.
///
/// Extracted as a free function so both `VerterLanguageServer` and `SyncCoordinator`
/// can resolve component types for diagnostic computation.
pub(crate) fn resolve_component_for(
    host: &verter_session::VerterHost,
    parent_canonical_id: &str,
    import_source: &str,
) -> Option<verter_session::FileAnalysisSnapshot> {
    let read_component_analysis = |canonical_id: &str| {
        let mut analysis = host.get_analysis(canonical_id);

        if analysis.is_none() && host.ensure_loaded(canonical_id) {
            analysis = host.get_analysis(canonical_id);
        }

        if canonical_id.ends_with(".vue")
            && analysis
                .as_ref()
                .is_some_and(|analysis| analysis.template.is_none())
        {
            let profile = verter_session::CompileProfile {
                target: verter_session::CompileTarget::ANALYSIS,
                ..Default::default()
            };
            let _ = host.ensure_compiled(canonical_id, &profile);
            analysis = host.get_analysis(canonical_id);
        }

        analysis
    };

    // Try 1: Relative import
    if import_source.starts_with('.') {
        let parts: Vec<&str> = parent_canonical_id.split('/').collect();
        let dir = parts[..parts.len().saturating_sub(1)].join("/");
        let resolved = resolve_import_path(&dir, import_source);
        if let Some(a) = read_component_analysis(&resolved) {
            return Some(a);
        }
    }

    // Try 2: VFS resolution (path aliases, tsconfig paths, disk probing)
    if let Some(resolved_path) =
        host.resolve_import_via_workspace(parent_canonical_id, import_source)
    {
        if let Some(a) = read_component_analysis(&resolved_path) {
            return Some(a);
        }
    }

    // Try 3: Direct lookup
    read_component_analysis(import_source)
}

pub(super) fn location_from_span(
    uri: &Uri,
    line_index: &LineIndex,
    span: verter_span::Span,
) -> Option<Location> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    Some(Location {
        uri: uri.clone(),
        range: Range {
            start: line_index.offset_to_position(span.start)?,
            end: line_index.offset_to_position(span.end)?,
        },
    })
}

pub(super) fn goto_response_from_locations(locations: Vec<Location>) -> GotoDefinitionResponse {
    if locations.len() == 1 {
        GotoDefinitionResponse::Scalar(locations.into_iter().next().unwrap())
    } else {
        GotoDefinitionResponse::Array(locations)
    }
}

pub(super) fn event_name_match_rank(requested: &str, candidate: &str) -> Option<u8> {
    if requested == candidate {
        return Some(0);
    }

    (normalized_event_name(requested) == normalized_event_name(candidate)).then_some(1)
}

pub(super) fn normalized_event_name(name: &str) -> String {
    let mut parts = name.splitn(2, ':');
    let head = parts.next().unwrap_or_default();
    match parts.next() {
        Some(tail) => format!(
            "{}:{}",
            camelize_event_segment(head),
            camelize_event_segment(tail)
        ),
        None => camelize_event_segment(head),
    }
}

pub(super) fn event_name_variants(name: &str) -> Vec<String> {
    let mut variants = vec![name.to_string()];
    let normalized = normalized_event_name(name);
    if normalized != name {
        variants.push(normalized);
    }

    let mut parts = name.splitn(2, ':');
    let head = parts.next().unwrap_or_default();
    let hyphenated = match parts.next() {
        Some(tail) => format!(
            "{}:{}",
            hyphenate_event_segment(head),
            hyphenate_event_segment(tail)
        ),
        None => hyphenate_event_segment(head),
    };
    if !hyphenated.is_empty() && !variants.iter().any(|variant| variant == &hyphenated) {
        variants.push(hyphenated);
    }

    variants
}

pub(super) fn listener_prop_candidates(event_name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    for variant in event_name_variants(event_name) {
        let candidate = format!("on{}", capitalize_first(&variant));
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

pub(super) fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub(super) fn camelize_event_segment(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut capitalize_next = false;
    for ch in value.chars() {
        if ch == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

pub(super) fn hyphenate_event_segment(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

pub(super) fn push_unique_location(
    locations: &mut Vec<Location>,
    seen: &mut HashSet<(String, u32, u32, u32, u32)>,
    location: Location,
) {
    let key = (
        location.uri.as_str().to_string(),
        location.range.start.line,
        location.range.start.character,
        location.range.end.line,
        location.range.end.character,
    );
    if seen.insert(key) {
        locations.push(location);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DidOpenStartupPolicy {
    pub(super) sync_imported_vue_files: bool,
    pub(super) publish_diagnostics: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DidOpenProviderSyncPolicy {
    pub(super) await_ide_sync: bool,
    pub(super) await_api_sync: bool,
    pub(super) background_api_sync: bool,
}

pub(super) fn did_open_startup_policy(kind: crate::TypeProviderKind) -> DidOpenStartupPolicy {
    DidOpenStartupPolicy {
        // When a type provider is active, eagerly sync imported .vue files so that
        // hover/completions/go-to-definition work on <ChildComponent> immediately.
        sync_imported_vue_files: !matches!(kind, crate::TypeProviderKind::None),
        // Diagnostics are pushed by the sync coordinator after open/change settles.
        publish_diagnostics: false,
    }
}

pub(super) fn did_open_provider_sync_policy(
    kind: crate::TypeProviderKind,
) -> DidOpenProviderSyncPolicy {
    match kind {
        crate::TypeProviderKind::Tsgo => DidOpenProviderSyncPolicy {
            await_ide_sync: true,
            await_api_sync: true,
            background_api_sync: false,
        },
        crate::TypeProviderKind::Tsserver => DidOpenProviderSyncPolicy {
            await_ide_sync: true,
            await_api_sync: false,
            background_api_sync: true,
        },
        crate::TypeProviderKind::None => DidOpenProviderSyncPolicy {
            await_ide_sync: true,
            await_api_sync: false,
            background_api_sync: false,
        },
    }
}

/// Standalone version of `resolve_import_specifier` that takes shared state
/// explicitly instead of `&self`. Used by `resync_aliased_imports_for_open_files`
/// in `background_init` after the project registry becomes available.
pub(super) fn resolve_import_specifier_standalone(
    host: &verter_session::VerterHost,
    parent_canonical_id: &str,
    specifier: &str,
) -> Option<String> {
    host.resolve_import_via_workspace(parent_canonical_id, specifier)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn collect_imported_vue_priority_ids(
    analysis: &verter_semantic::analysis::ScriptAnalysisSnapshot,
) -> Vec<String> {
    collect_imported_vue_priority_ids_from_imports(&analysis.imports)
}

pub(super) fn collect_imported_vue_priority_ids_from_imports(
    imports: &[verter_semantic::analysis::AnalyzedImport],
) -> Vec<String> {
    collect_imported_vue_priority_ids_from_imports_with_fallback(
        imports,
        None,
        |_parent, _specifier| None,
    )
}

pub(super) fn collect_imported_vue_priority_ids_from_imports_with_fallback<F>(
    imports: &[verter_semantic::analysis::AnalyzedImport],
    parent_canonical_id: Option<&str>,
    mut resolve_import: F,
) -> Vec<String>
where
    F: FnMut(&str, &str) -> Option<String>,
{
    let mut seen = HashSet::new();
    let mut ids = Vec::new();

    for import in imports {
        let canonical_id = import.resolved_canonical_id.clone().or_else(|| {
            parent_canonical_id.and_then(|parent| resolve_import(parent, &import.source))
        });
        let Some(canonical_id) = canonical_id.as_ref() else {
            continue;
        };
        if !canonical_id.ends_with(".vue") {
            continue;
        }
        if seen.insert(canonical_id.clone()) {
            ids.push(canonical_id.clone());
        }
    }

    ids
}

pub(super) fn collect_priority_vue_targets_from_module_references(
    snapshot: Option<&super::PublishedResolverSnapshot>,
    reader: &dyn verter_workspace::WorkspaceAccess,
    importer_id: &str,
    module_references: &[verter_semantic::analysis::AnalyzedModuleReference],
) -> Vec<String> {
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut ids = Vec::new();

    for reference in module_references {
        let specifiers = if let Some(specifier) = reference.literal_specifier.as_deref() {
            vec![specifier.to_string()]
        } else {
            reference.finite_specifiers.clone()
        };

        for specifier in specifiers {
            let request = crate::project_resolver::ResolveRequest {
                importer_id: importer_id.to_string(),
                specifier,
                kind: analyzed_module_reference_request_kind(reference),
                phase: crate::project_resolver::ResolvePhase::ProviderGraph,
            };
            let Some(resolved) = snapshot.resolver.resolve_with_reader(reader, &request) else {
                continue;
            };
            if resolved.provider_target == crate::project_resolver::ProviderTarget::VuePublicApi
                && seen.insert(resolved.source_id.clone())
            {
                ids.push(resolved.source_id);
            }
        }
    }

    ids
}

/// Compute verter diagnostics (host errors + lint rules + component usage) for a document.
///
/// Extracted as a free function so both `VerterLanguageServer::compute_verter_diagnostics()`
/// and the `SyncCoordinator` can produce diagnostics using the same cache policy.
/// Results are cached per `(document version, host diagnostics generation)` in
/// `cached_verter_diags` to avoid redundant re-computation after host-driven
/// recompiles that do not change the editor document version.
///
/// ## Lint resolution
///
/// Uses the published `LspViews` from the VFS workspace for per-project lint.
/// If no published snapshot exists or the file has no owner, uses a default linter.
pub(crate) fn compute_verter_diagnostics_for_with_views(
    documents: &DocumentRegistry,
    uri: &Uri,
    cached_verter_diags: &DashMap<String, CachedVerterDiagEntry>,
    vfs_workspace: Option<&verter_workspace::FilesystemWorkspace>,
) -> Vec<Diagnostic> {
    // Check cache: if version AND diagnostics generation both match, return cached.
    let uri_str = uri.as_str();
    let canonical_id = uri_to_canonical_id(uri);
    let current_diag_gen = documents
        .host()
        .get_diagnostics_generation(&canonical_id)
        .unwrap_or(0);
    if let Some(doc) = documents.get(uri) {
        if let Some(cached) = cached_verter_diags.get(uri_str) {
            if cached.0 == doc.version && cached.1 == current_diag_gen {
                return cached.2.clone();
            }
        }
    }

    let mut diags = if let Some(doc) = documents.get(uri) {
        let host_diags = documents.get_diagnostics(uri);
        match host_diags {
            Some(snapshot) => map_diagnostics(&snapshot, &doc.line_index),
            None => vec![],
        }
    } else {
        vec![]
    };

    // Run the diagnostics engine (lint rules: CSS, template, a11y, etc.)
    if let Some(doc) = documents.get(uri) {
        if let Some(analysis) = documents.get_analysis(uri) {
            let canonical_id = uri_to_canonical_id(uri);

            // Use published LspViews from VFS workspace for per-project lint.
            let published = vfs_workspace.and_then(|ws| ws.load_published());
            let views_lint = published.as_ref().and_then(|p| {
                let views = p.ext::<crate::workspace_state::LspViews>()?;
                let view = views.linter_view_for_file(&p.snapshot, &canonical_id)?;
                Some((view.lint_explicitly_configured, &view.linter))
            });

            let lint_explicitly_configured;
            if let Some((explicit, linter)) = views_lint {
                lint_explicitly_configured = explicit;
                diags.extend(crate::features::diagnostics_bridge::run_linter(
                    linter,
                    &analysis,
                    &doc.source,
                    &doc.line_index,
                ));
            } else {
                // No published snapshot or file not owned — use default linter.
                lint_explicitly_configured = false;
                let default_linter = verter_diagnostics::Linter::default();
                diags.extend(crate::features::diagnostics_bridge::run_linter(
                    &default_linter,
                    &analysis,
                    &doc.source,
                    &doc.line_index,
                ));
            }

            // Component usage diagnostics (unknown props, unknown v-models).
            let host = documents.host();
            diags.extend(
                crate::features::component_diagnostics::component_usage_diagnostics(
                    &analysis,
                    &doc.line_index,
                    &|import_source| resolve_component_for(host, &canonical_id, import_source),
                ),
            );

            // When lint is not explicitly configured, suppress lint diagnostics but
            // keep component usage diagnostics (type-level, not lint rules).
            if !lint_explicitly_configured {
                diags.retain(|d| match &d.code {
                    Some(NumberOrString::String(code)) => {
                        if code == "verter/unknown-prop" || code == "verter/unknown-model" {
                            return true;
                        }
                        !code.starts_with("verter/")
                    }
                    _ => true,
                });
            }
        }
    }

    // Cache the result
    if let Some(doc) = documents.get(uri) {
        cached_verter_diags.insert(
            uri_str.to_string(),
            (doc.version, current_diag_gen, diags.clone()),
        );
    }

    diags
}

pub(super) fn resolve_import_path(importer_dir: &str, import_source: &str) -> String {
    if !import_source.starts_with('.') {
        // Not a relative import — return as-is (alias import)
        return import_source.to_string();
    }

    let mut parts: Vec<&str> = importer_dir.split('/').filter(|s| !s.is_empty()).collect();

    for segment in import_source.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }

    // Reconstruct: preserve drive letter on Windows (e.g., "C:/...")
    if importer_dir.chars().nth(1) == Some(':') {
        parts.join("/")
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Extract a TypeScript type annotation from a hover markdown string.
///
/// Handles formats like:
/// - "```typescript\nconst x: number\n```"
/// - "(property) x: string"
/// - "let x: Ref<number>"
pub(super) fn extract_type_from_hover(contents: &str, binding_name: &str) -> Option<String> {
    // Look for pattern: `name: type` or `name = value`
    let patterns = [format!("{binding_name}: "), format!("{binding_name}:")];

    for line in contents.lines() {
        let trimmed = line.trim().trim_start_matches("```typescript").trim();
        for pattern in &patterns {
            if let Some(idx) = trimmed.find(pattern.as_str()) {
                let after = &trimmed[idx + pattern.len()..];
                let type_str = after.trim().trim_end_matches("```").trim();
                if !type_str.is_empty() {
                    return Some(type_str.to_string());
                }
            }
        }
    }

    None
}

pub(super) fn identifier_prefix_before_offset(content: &str, offset: usize) -> Option<&str> {
    if offset == 0 || offset > content.len() {
        return None;
    }

    let bytes = content.as_bytes();
    let mut start = offset;
    while start > 0 {
        let byte = bytes[start - 1];
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' {
            start -= 1;
        } else {
            break;
        }
    }

    if start == offset {
        return None;
    }

    let prefix = &content[start..offset];
    let first = prefix.as_bytes()[0];
    if first.is_ascii_alphabetic() || first == b'_' || first == b'$' {
        Some(prefix)
    } else {
        None
    }
}

pub(super) fn is_immediately_after_member_access_dot(content: &str, offset: usize) -> bool {
    if offset == 0 || offset > content.len() {
        return false;
    }

    let bytes = content.as_bytes();
    let mut i = offset;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }

    i > 0 && bytes[i - 1] == b'.' && (i < 2 || bytes[i - 2] != b'.')
}

pub(super) fn is_identifier_prefix_completion_kind(
    kind: crate::tsgo::protocol::CompletionKind,
) -> bool {
    matches!(
        kind,
        crate::tsgo::protocol::CompletionKind::Variable
            | crate::tsgo::protocol::CompletionKind::Function
            | crate::tsgo::protocol::CompletionKind::Method
            | crate::tsgo::protocol::CompletionKind::Property
            | crate::tsgo::protocol::CompletionKind::Field
            | crate::tsgo::protocol::CompletionKind::Constant
            | crate::tsgo::protocol::CompletionKind::EnumMember
    )
}

pub(super) fn is_member_access_completion_kind(
    kind: crate::tsgo::protocol::CompletionKind,
) -> bool {
    matches!(
        kind,
        crate::tsgo::protocol::CompletionKind::Property
            | crate::tsgo::protocol::CompletionKind::Field
            | crate::tsgo::protocol::CompletionKind::Method
            | crate::tsgo::protocol::CompletionKind::Constant
            | crate::tsgo::protocol::CompletionKind::EnumMember
    )
}

pub(super) fn filter_type_provider_completion_result(
    type_result: &mut crate::tsgo::protocol::CompletionResult,
    expr_context: Option<&ExpressionContext>,
    identifier_prefix: Option<&str>,
    verter_items: Option<&Vec<CompletionItem>>,
) {
    if matches!(expr_context, Some(ExpressionContext::MemberAccess)) {
        let before = type_result.items.len();
        type_result
            .items
            .retain(|item| item.kind.is_some_and(is_member_access_completion_kind));
        tracing::debug!(
            "completion: filtered type provider for MemberAccess context: {} -> {} items",
            before,
            type_result.items.len()
        );
    } else if let Some(prefix) = identifier_prefix {
        let before = type_result.items.len();
        type_result.items.retain(|item| {
            item.label.starts_with(prefix)
                && item.kind.is_some_and(is_identifier_prefix_completion_kind)
        });
        tracing::debug!(
            "completion: filtered type provider for IdentifierExpected prefix {:?}: {} -> {} items",
            prefix,
            before,
            type_result.items.len()
        );
    } else if matches!(expr_context, Some(ExpressionContext::Unknown)) {
        let allowlist: std::collections::HashSet<&str> = verter_items
            .map(|items| items.iter().map(|i| i.label.as_str()).collect())
            .unwrap_or_default();
        let before = type_result.items.len();
        type_result
            .items
            .retain(|item| allowlist.contains(item.label.as_str()));
        tracing::debug!(
            "completion: filtered type provider for Unknown context: {} -> {} items",
            before,
            type_result.items.len()
        );
    }
}

/// Extract a debug snippet around `offset` in `content`, returning `(before_cursor, after_cursor)`.
/// Returns `None` if the offset is out of bounds.
pub(super) fn debug_snippet(content: &str, offset: usize) -> Option<(String, String)> {
    if offset > content.len() {
        return None;
    }
    // Snap to char boundaries so we never slice inside a multi-byte UTF-8 sequence
    let snippet_start = content.floor_char_boundary(offset.saturating_sub(20));
    let snippet_end = content.ceil_char_boundary((offset + 30).min(content.len()));
    let cursor = content.floor_char_boundary(offset);
    if snippet_end <= snippet_start || cursor < snippet_start || cursor > snippet_end {
        return None;
    }
    let before = &content[snippet_start..cursor];
    let after = &content[cursor..snippet_end];
    Some((before.to_string(), after.to_string()))
}
