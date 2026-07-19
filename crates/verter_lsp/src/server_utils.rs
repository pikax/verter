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
///
/// Architecture-guard exception: this is a filesystem-event handler. It
/// fires on `DidChangeWatchedFilesParams` URIs before the workspace
/// registry may have published a snapshot, so it cannot consult
/// `WorkspaceAccess::is_package_backed` (which returns `false` for every
/// path until the first publish). See exception class (2) in
/// `crates/verter_session/tests/cases/architecture_guards.rs` →
/// `no_node_modules_substring_outside_workspace_api`.
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
    // The JavaScript project config — same ownership authority as tsconfig.json
    // for JS-only trees; edits must rebuild the registry identically.
    if filename == "jsconfig.json" {
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

#[path = "server_utils/debug.rs"]
mod debug;
pub(super) use debug::debug_snippet;

/// Registry-backed carrier classification for a canonical ID (or URI
/// string): `Some(language)` when the path classifies as a framework
/// CARRIER row (`.vue`, `.svelte`, …), `None` for plain scripts and
/// unknown extensions. A carrier row without a registered carrier
/// implementation still classifies here — its requests surface the
/// typed unsupported-language error and produce no provider sync state.
pub(crate) fn carrier_language_for(path: &str) -> Option<verter_session::FileLanguage> {
    let language = verter_session::LanguageRegistry::global()
        .classify_static(path)
        .static_resolution();
    language.is_framework_carrier().then_some(language)
}

/// Registry-backed adapter-MODULE classification for a canonical ID (or URI
/// string): `Some(language)` when the path classifies as a standalone non-
/// component adapter module (`.svelte.ts` / `.svelte.js` rune module), `None`
/// otherwise (carriers, plain scripts, unknown extensions). An adapter module
/// is NOT a carrier — it serves its OWN-path provider buffer with a synthetic
/// rune prelude, not an IDE TSX projection.
pub(crate) fn adapter_module_language_for(path: &str) -> Option<verter_session::FileLanguage> {
    let language = verter_session::LanguageRegistry::global()
        .classify_static(path)
        .static_resolution();
    verter_session::framework::svelte_rune_module_source_type(&language).map(|_| language)
}

/// Registry-backed SELF-FILE classification for a canonical ID (or URI
/// string): `Some(language)` when the path serves an OWN-path provider
/// buffer — a Svelte rune module (`<rune prelude> + <bytes>`) OR a plain
/// TS-family script (bytes verbatim, zero-line prelude). `None` for
/// framework carriers (they project a generated IDE companion) and for
/// unknown extensions (no registered language row — never serve a
/// `.md`/`.css`/extensionless document to the TypeScript provider).
pub(crate) fn self_file_language_for(path: &str) -> Option<verter_session::FileLanguage> {
    let classification = verter_session::LanguageRegistry::global().classify_static(path);
    if matches!(
        classification,
        verter_session::StaticClassification::Unknown
    ) {
        return None;
    }
    let language = classification.static_resolution();
    verter_session::framework::serves_self_file_provider_buffer(&language).then_some(language)
}

/// Whether `path` is a framework CARRIER whose default export IS the
/// component value (`.vue`, `.svelte`, …). Every framework carrier shares
/// default-export component semantics: a default import of the carrier binds
/// the component, so the "name won't match script bindings, retry with
/// `default`" navigation fallback and the component-target resolution gates
/// apply to ANY carrier — none of it is Vue-intrinsic.
///
/// This is the registry-backed replacement for the hardcoded
/// `ends_with(".vue")` default-export / component-target gates across the
/// definition / navigation / component-resolution feature layer.
pub(crate) fn is_default_export_component_carrier(path: &str) -> bool {
    carrier_language_for(path).is_some()
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
    let mut files = host.list_files();
    let current_dir = current_file_id
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("");

    // DETERMINISTIC ENUMERATION ORDER. `host.list_files()` is backed by a DashMap,
    // so its iteration order is non-deterministic run-to-run. Two files can
    // sanitize to the SAME identifier (e.g. `Model.Named.vue` and `ModelNamed.vue`
    // → `ModelNamed`); the downstream completion synthesizers dedup by label and
    // keep the FIRST candidate, so a non-deterministic order would make collision
    // resolution AND completion-item ordering flap between runs. Sorting by the
    // canonical file id gives a single stable precedence: the first canonical path
    // wins a collision, and emitted candidates are in a fixed order.
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut components = Vec::new();

    for (file_id, kind) in &files {
        // Only framework CARRIER files (`.vue`, `.svelte`, …).
        if !kind.is_framework_carrier() {
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

        // Derive component name from filename via the registry-backed carrier
        // strip: `src/components/MyButton.vue` → `MyButton`,
        // `src/components/MyButton.svelte` → `MyButton`.
        let filename = file_id.rsplit('/').next().unwrap_or(file_id);
        let stem = verter_workspace::strip_carrier_extension(filename);
        if stem.is_empty() {
            continue;
        }

        // Convert the filename stem to a VALID JS identifier in PascalCase:
        // `my-button` → `MyButton`, `Model.Named` → `ModelNamed`, `index` → `Index`.
        // This is identifier FORMATTING of a filename (the single source of truth
        // for both the tag insert text and the auto-import binding), NOT semantic
        // type logic — string ops are appropriate here.
        let component_name = to_pascal_case(stem);
        if component_name.is_empty() {
            continue;
        }

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

/// Convert a filename stem to a valid JS-identifier PascalCase name.
///
/// `-`, `_`, `.`, and any character outside the ASCII identifier set
/// `[A-Za-z0-9_$]` act as word separators feeding PascalCase
/// (`my-button` → `MyButton`, `Model.Named` → `ModelNamed`,
/// `weird@name` → `WeirdName`). `$` is kept verbatim (a valid ASCII identifier
/// char that is NOT a separator: `My$Comp` → `My$Comp`). If the result would
/// begin with a digit it is prefixed with `_` so it remains a valid identifier
/// start (`2cool` → `_2cool`). Empty/all-separator input yields `""`.
///
/// ASCII-only by design: a non-ASCII char (e.g. `café`'s `é`, a CJK glyph) is
/// treated as a separator and dropped from the derived name, even though JS
/// permits Unicode identifiers. This keeps the helper free of the Unicode
/// `ID_Start`/`ID_Continue` property tables; the on-disk component-name domain
/// is overwhelmingly ASCII, and the result is ALWAYS a valid ASCII identifier
/// usable verbatim as both the tag insert text and the auto-import binding. A
/// stem with no ASCII identifier chars yields `""` and the carrier is skipped
/// upstream in `build_workspace_components` rather than emitting a bad name.
///
/// This is identifier formatting of a filename, not semantic type logic.
pub(super) fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for ch in s.chars() {
        // `_` is identifier-valid but ALSO acts as a word separator, so the
        // existing snake_case behavior (`my_comp` → `MyComp`) is preserved. `$`
        // is identifier-valid and is NOT a separator: it is kept verbatim
        // (`My$Comp` → `My$Comp`).
        if ch == '-' || ch == '_' || ch == '.' || !is_ident_char(ch) {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    // A valid identifier must not start with a digit.
    if result.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        result.insert(0, '_');
    }
    result
}

/// Whether `ch` is valid inside a JS identifier (ASCII-only, matching the
/// component-name domain produced from on-disk filenames).
fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
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

pub(super) fn source_id_from_provider_carrier_path(
    resolver: &crate::project_resolver::NativeProjectResolver,
    host: &verter_session::VerterHost,
    provider_path: &str,
) -> Option<String> {
    let candidate = resolver.source_id_from_provider_id(provider_path)?;
    // Collision guard, generalized to every registry CARRIER extension
    // (`.vue` / `.svelte` — never `.vue` hardcoded): a `{name}.{carrier-ext}`
    // virtual/derived path is only valid when the backing `{name}.{carrier-ext}`
    // SOURCE actually exists in the host. Ownership is decided by project
    // membership, not file existence, so the resolver happily strips
    // `store.svelte.ts` → `store.svelte` even when no `store.svelte` component
    // was ever compiled. Without this guard a real `store.svelte.ts` rune module
    // (or a real `weird.vue.tsx` on disk) reverse-maps to a phantom carrier.
    if verter_workspace::path_is_carrier(&candidate) && host.get_source(&candidate).is_none() {
        // The stripped carrier candidate is a phantom. If the ORIGINAL provider
        // path is itself a real owned source (the `.svelte.ts`/`.svelte.js` rune
        // module case, or any owned `{name}.{carrier-ext}.{x}` file with no
        // backing carrier), it maps to ITSELF — never to the phantom carrier.
        // We consult ownership + host directly here rather than re-stripping
        // through `source_id_from_provider_id` (which would re-derive the same
        // phantom carrier).
        let normalized = verter_workspace::resolver::normalize_canonical_id(provider_path);
        if host.get_source(&normalized).is_some()
            && resolver.nearest_config_for_path(&normalized).is_some()
        {
            return Some(normalized);
        }
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

impl verter_workspace::WorkspaceRead for LspProjectResolverReader<'_> {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        // Try host cache first (already-upserted files), then workspace (disk).
        self.documents.host().get_source(canonical_id).or_else(|| {
            self.documents
                .host()
                .workspace_read()
                .read_file(canonical_id)
        })
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.documents.host().get_source(canonical_id).is_some()
            || self
                .documents
                .host()
                .workspace_read()
                .file_exists(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        if self.documents.host().get_source(canonical_id).is_some() {
            return Some(canonical_id.replace('\\', "/"));
        }
        self.documents
            .host()
            .workspace_read()
            .realpath(canonical_id)
    }

    fn reverse_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    fn forward_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    fn dependency_snapshot(
        &self,
        _canonical_id: &str,
    ) -> Option<verter_workspace::DependencySnapshotView> {
        None
    }
}

impl verter_workspace::WorkspaceAccess for LspProjectResolverReader<'_> {
    // ── Reader-only stub overrides for reverse-graph methods (R6/R7) ──
    //
    // Rationale (§2.16b): `LspProjectResolverReader` is a thin file-read
    // adapter passed to the project resolver for source/manifest reads
    // during import resolution. It does not participate in dep-flow; the
    // host's workspace owns those edges.
    fn record_parsed_edges(&self, _canonical_id: &str, _edges: &[verter_workspace::ParsedEdge]) {}

    fn set_exact_resolutions(
        &self,
        _canonical_id: &str,
        _resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        verter_workspace::ExactResolutionResult::default()
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        _canonical_id: &str,
        _edges: &[verter_workspace::ParsedEdge],
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

    fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
}

/// Compute the import-specifier replacement spans for a non-carrier source,
/// each `(byte_start, byte_end, replacement_text)` indexing into `source` (the
/// pre-rewrite bytes). The replacement text is the quote-wrapped provider
/// specifier. The spans are returned in ASCENDING byte order so a consumer can
/// translate them to per-line (line, column) segments (the self-file position
/// mapper); apply them with [`apply_specifier_replacements`] (which sorts
/// descending so earlier in-place edits don't shift later spans).
pub(crate) fn compute_specifier_replacements(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_workspace::WorkspaceRead,
    importer_id: &str,
    source: &str,
    module_references: &[verter_session::ScriptModuleReference],
) -> Vec<(usize, usize, String)> {
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
    replacements.sort_by_key(|replacement| replacement.0);
    replacements
}

/// Apply the specifier replacements to `source`, producing the rewritten
/// provider bytes. Edits are applied in descending byte order so an in-place
/// replacement never shifts the spans of replacements earlier in the file.
pub(crate) fn apply_specifier_replacements(
    source: &str,
    replacements: &[(usize, usize, String)],
) -> String {
    let mut rewritten = source.to_string();
    let mut ordered: Vec<&(usize, usize, String)> = replacements.iter().collect();
    ordered.sort_by_key(|replacement| std::cmp::Reverse(replacement.0));
    for (start, end, replacement) in ordered {
        rewritten.replace_range(*start..*end, replacement);
    }
    rewritten
}

/// Sync an OPEN self-file document's provider buffer to the type provider as
/// UNRESOLVED open-document state, keyed at the document's OWN canonical path
/// (the Shadow provider path).
///
/// A self-file document is a Svelte rune module (`<rune prelude> + <rewritten
/// module bytes>`) or a plain TS-family script (its bytes verbatim — the
/// provider buffer IS the document source). This is the SHARED self-file
/// shadow-sync primitive, called from BOTH the server's `did_open`/`did_change`
/// handler and the debounced [`SyncCoordinator`] tick — so a debounced sync
/// routes a self-file document through the SAME projection path the editor
/// ingress uses (NOT the carrier-miss `preserve_open_unresolved_carrier`,
/// which would clobber the Shadow state with an IDE-path state and break
/// did_close cleanup).
///
/// Returns `true` when the self-file buffer was synced and the Shadow state
/// committed; `false` when the path is not a self-file document, the document
/// is gone, or the provider sync failed. It refreshes the document's
/// rewrite-aware projection from the same replacements it applies, so
/// own-buffer position mapping stays exact.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn sync_self_file_shadow_state(
    documents: &DocumentRegistry,
    project_sync: &crate::type_provider::project_sync::ProjectSync,
    provider_sync_states: &DashMap<String, crate::provider_sync::ProviderSyncState>,
    snapshot: Option<&super::PublishedResolverSnapshot>,
    uri: &Uri,
    canonical_id: &str,
    file_language: &verter_session::FileLanguage,
) -> bool {
    documents.host().ensure_loaded(canonical_id);

    let Some(source) = documents.get(uri).map(|d| d.source.clone()) else {
        return false;
    };

    // Compute the import-specifier rewrites (resolver-backed when a snapshot
    // exists; empty otherwise), refine the document's rewrite-aware projection,
    // then build the provider buffer from the SAME replacements.
    let module_references: Vec<verter_session::ScriptModuleReference> = documents
        .host()
        .get_analysis(canonical_id)
        .map(|analysis| {
            analysis
                .module_references
                .iter()
                .map(verter_session::ScriptModuleReference::from)
                .collect()
        })
        .unwrap_or_default();
    let replacements = if let Some(snapshot) = snapshot {
        let ws = documents.host().workspace_read();
        compute_specifier_replacements(
            &snapshot.resolver,
            ws.as_ref(),
            canonical_id,
            &source,
            &module_references,
        )
    } else {
        Vec::new()
    };
    documents.refresh_self_file_rewrites(uri, &replacements);

    let rewritten = apply_specifier_replacements(&source, &replacements);
    let Some(built) =
        verter_session::framework::self_file_provider_content(file_language, &rewritten)
    else {
        return false;
    };

    let mut state = provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .unwrap_or_else(|| crate::provider_sync::ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ..Default::default()
        });
    // Bootstrap UNRESOLVED open-document state — force `Unresolved` so a stale
    // `Owned` binding from a prior committed state is never re-committed here
    // (mirrors the carrier unresolved-sync discipline).
    state.owner_binding = crate::provider_sync::ProviderOwnerBinding::Unresolved;

    let needs_open =
        state.shadow_path.as_deref() != Some(canonical_id) || !state.shadow_background_loaded;
    let result = if needs_open {
        project_sync.load_file(canonical_id, &built.content).await
    } else {
        project_sync.sync_file(canonical_id, &built.content).await
    };

    match result {
        Ok(()) => {
            // Record a fresh Shadow generation pinning the EXACT provider buffer
            // bytes just synced, paired with the rewrite-aware mapper refreshed
            // from the SAME replacements above — the surface interactive queries
            // capture. Recorded ONLY on a successful provider sync (fail-closed:
            // a failed sync records nothing). The stamped `map_hash` is the
            // mapper's structural identity (the self-file analogue of the
            // carrier map-JSON hash) so a mapper-only re-sync is a DISTINCT
            // capture the byte-match honored gate rejects.
            let source_map = documents.get_position_mapper(uri);
            let map_hash = match &source_map {
                Some(crate::documents::provider_projection::ProviderPositionMapper::SelfFile(
                    mapper,
                )) => mapper.identity_hash16(),
                _ => [0u8; 16],
            };
            documents
                .provider_surfaces()
                .record(crate::provider_surface_store::RecordSurface {
                    provider_path: canonical_id.to_string(),
                    kind: crate::provider_surface_store::ProviderSurfaceKind::Shadow,
                    source_canonical: canonical_id.to_string(),
                    provider_content: std::sync::Arc::from(built.content.as_str()),
                    source_map,
                    carrier_source: source,
                    map_hash,
                    project_owner: None,
                    regen_key: None,
                    engine_recheck: None,
                });
            state.shadow_path = Some(canonical_id.to_string());
            state.shadow_background_loaded = true;
            crate::provider_sync::commit_sync_transition(provider_sync_states, canonical_id, state);
            true
        }
        Err(error) => {
            tracing::warn!("sync_self_file_shadow_state: failed for {canonical_id}: {error}");
            false
        }
    }
}

pub(crate) fn rewrite_non_carrier_source_with_resolver(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_workspace::WorkspaceRead,
    importer_id: &str,
    source: &str,
    module_references: &[verter_session::ScriptModuleReference],
) -> String {
    let replacements =
        compute_specifier_replacements(resolver, reader, importer_id, source, module_references);
    apply_specifier_replacements(source, &replacements)
}

pub(crate) fn prepare_non_carrier_provider_sync(
    snapshot: Option<&super::PublishedResolverSnapshot>,
    reader: &dyn verter_workspace::WorkspaceRead,
    importer_id: &str,
    source: &str,
    module_references: &[verter_session::ScriptModuleReference],
) -> Option<PreparedNonCarrierProviderSync> {
    let snapshot = snapshot?;
    let provider_path = snapshot.resolver.provider_id_for_source(importer_id)?;
    let rewritten = rewrite_non_carrier_source_with_resolver(
        &snapshot.resolver,
        reader,
        importer_id,
        source,
        module_references,
    );
    // Channel B: a standalone Svelte rune module (`.svelte.ts`/
    // `.svelte.js`) serves `<module rune prelude> + <bytes>` from its OWN
    // canonical path so a consumer resolving it from disk sees the inferred
    // rune-derived exported types. The prelude is module-local (`export {};`),
    // so it does NOT leak the runes into a plain `.ts`/`.js` (which is fed its
    // bytes verbatim — `rune_module_provider_content` returns `None` for it).
    let language = verter_session::LanguageRegistry::global()
        .classify_static(importer_id)
        .static_resolution();
    let rewritten =
        match verter_session::framework::rune_module_provider_content(&language, &rewritten) {
            Some(built) => built.content,
            None => rewritten,
        };
    let resolved_dependencies = collect_resolved_provider_dependencies(
        &snapshot.resolver,
        reader,
        importer_id,
        module_references,
    );

    Some(PreparedNonCarrierProviderSync {
        provider_path,
        rewritten,
        resolved_dependencies,
    })
}

pub(crate) fn collect_resolved_provider_dependencies(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn verter_workspace::WorkspaceRead,
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
    reader: &dyn verter_workspace::WorkspaceRead,
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
/// Handles cases where the import source omits the framework CARRIER
/// extension. For every registry carrier extension (`vue`, `svelte`, …):
/// - `./Popup` → matches `./Popup.{ext}`
/// - `./Popover` → matches `./Popover/index.{ext}` or `./Popover/Popover.{ext}`
pub(super) fn import_resolved_matches_target(resolved: &str, target: &str) -> bool {
    if resolved == target {
        return true;
    }
    // Skip if resolved already has a carrier extension — no fuzzy matching
    // needed.
    if verter_workspace::path_is_carrier(resolved) {
        return false;
    }
    let last_segment = resolved.rsplit('/').next().filter(|s| !s.is_empty());
    for ext in verter_session::LanguageRegistry::global().carrier_extensions() {
        // Try: resolved + ".{ext}"
        if target == format!("{resolved}.{ext}") {
            return true;
        }
        // Try: resolved/index.{ext}
        if target == format!("{resolved}/index.{ext}") {
            return true;
        }
        // Try: resolved/Name.{ext} where Name is the last segment of resolved.
        if let Some(last) = last_segment {
            if target == format!("{resolved}/{last}.{ext}") {
                return true;
            }
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

        if carrier_language_for(canonical_id).is_some()
            && analysis
                .as_ref()
                .is_some_and(|analysis| analysis.template.is_none())
        {
            let profile = verter_session::CompileProfile {
                target: verter_session::CompileTarget::ANALYSIS,
                ..Default::default()
            };
            // ANALYSIS-facing (NOT IDE-sync): this drives the shared compile to
            // populate the file's analysis/template-data, then reads
            // `get_analysis` — it does NOT consume IDE TSX. The result is
            // ignored; the compile side-effect populates the analysis store
            // before the (Main-less-carrier) `MissingVirtualNode`, so a Svelte
            // carrier's analysis still lands. Kept on `ensure_compiled`
            // deliberately: it wants the analysis surface, not the IDE surface.
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
    attr_name_match_rank(requested, candidate)
}

/// Rank an authored template attribute / directive arg name against a declaration
/// name. Exact match wins; otherwise kebab ↔ camel equivalence (Vue's HTML-case
/// contract) ranks as a secondary hit. Used for props, events, and slots.
pub(super) fn attr_name_match_rank(requested: &str, candidate: &str) -> Option<u8> {
    if requested == candidate {
        return Some(0);
    }

    (normalized_event_name(requested) == normalized_event_name(candidate)).then_some(1)
}

/// Select the best-matching candidate by `(rank, span)`: lowest rank wins and
/// equal ranks fall to the earliest span. Shared by every attr-name
/// best-candidate loop (prop fields, template prop definitions, slot fields,
/// defined slots, slot bindings, rename declarations) so the rank/span
/// tie-break cannot drift between call sites. The candidate `value` rides
/// along so a caller can keep a borrowed field instead of re-looking it up.
pub(super) fn select_best_ranked_candidate<T>(
    candidates: impl IntoIterator<Item = (u8, verter_span::Span, T)>,
) -> Option<(u8, verter_span::Span, T)> {
    let mut best: Option<(u8, verter_span::Span, T)> = None;
    for (rank, span, value) in candidates {
        let better = match &best {
            None => true,
            Some((best_rank, best_span, _)) => {
                rank < *best_rank
                    || (rank == *best_rank
                        && (span.start, span.end) < (best_span.start, best_span.end))
            }
        };
        if better {
            best = Some((rank, span, value));
        }
    }
    best
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

#[cfg(test)]
mod attr_name_match_tests {
    use super::*;

    #[test]
    fn attr_name_exact_rank_zero() {
        assert_eq!(attr_name_match_rank("myProp", "myProp"), Some(0));
        assert_eq!(attr_name_match_rank("my-prop", "my-prop"), Some(0));
    }

    #[test]
    fn attr_name_kebab_camel_rank_one() {
        assert_eq!(attr_name_match_rank("my-prop", "myProp"), Some(1));
        assert_eq!(attr_name_match_rank("myProp", "my-prop"), Some(1));
        assert_eq!(attr_name_match_rank("my-event", "myEvent"), Some(1));
        assert_eq!(attr_name_match_rank("my-slot", "mySlot"), Some(1));
    }

    #[test]
    fn attr_name_unrelated_misses() {
        assert_eq!(attr_name_match_rank("title", "count"), None);
        assert_eq!(attr_name_match_rank("my-prop", "myFlag"), None);
    }

    #[test]
    fn select_best_ranked_candidate_prefers_lowest_rank_then_earliest_span() {
        let span = |start: u32, end: u32| verter_span::Span { start, end };
        // Exact (rank 0) beats kebab/camel (rank 1) regardless of order.
        let best = select_best_ranked_candidate([
            (1u8, span(4, 8), "case-fold"),
            (0u8, span(20, 24), "exact"),
        ]);
        assert_eq!(best, Some((0u8, span(20, 24), "exact")));
        // Equal rank: the earlier span wins, even when seen second.
        let best = select_best_ranked_candidate([
            (1u8, span(20, 24), "later"),
            (1u8, span(4, 8), "earlier"),
        ]);
        assert_eq!(best, Some((1u8, span(4, 8), "earlier")));
        // Empty candidate set fails closed.
        assert_eq!(
            select_best_ranked_candidate(Vec::<(u8, verter_span::Span, ())>::new()),
            None
        );
    }
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
    pub(super) sync_imported_carrier_apis: bool,
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
        // When a type provider is active, eagerly sync imported carrier APIs
        // (any framework carrier — `.vue`, `.svelte`, …) so that
        // hover/completions/go-to-definition work on <ChildComponent> immediately.
        sync_imported_carrier_apis: !matches!(kind, crate::TypeProviderKind::None),
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
        crate::TypeProviderKind::EditorTsserver => DidOpenProviderSyncPolicy {
            await_ide_sync: false,
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
pub(super) fn collect_imported_carrier_priority_ids(
    analysis: &verter_semantic::analysis::ScriptAnalysisSnapshot,
) -> Vec<String> {
    collect_imported_carrier_priority_ids_from_imports(&analysis.imports)
}

pub(super) fn collect_imported_carrier_priority_ids_from_imports(
    imports: &[verter_semantic::analysis::AnalyzedImport],
) -> Vec<String> {
    collect_imported_carrier_priority_ids_from_imports_with_fallback(
        imports,
        None,
        |_parent, _specifier| None,
    )
}

pub(super) fn collect_imported_carrier_priority_ids_from_imports_with_fallback<F>(
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
        if carrier_language_for(canonical_id).is_none() {
            continue;
        }
        if seen.insert(canonical_id.clone()) {
            ids.push(canonical_id.clone());
        }
    }

    ids
}

pub(super) fn collect_priority_carrier_public_api_targets_from_module_references(
    snapshot: Option<&super::PublishedResolverSnapshot>,
    reader: &dyn verter_workspace::WorkspaceRead,
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
            if resolved.provider_target == crate::project_resolver::ProviderTarget::CarrierPublicApi
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

            // Projection safety limits are typed component-meta outcomes, not
            // parser diagnostics or lint rules. Resolve the owner surface and
            // map only those two operational reasons onto the macro call span
            // used by editor consumers.
            if let Some(component_meta) = host.get_component_meta(&canonical_id) {
                let macro_spans: Vec<verter_span::Span> =
                    analysis.macros.iter().map(|mac| mac.span).collect();
                diags.extend(
                    crate::features::diagnostics::map_projection_limit_diagnostics(
                        &macro_spans,
                        &component_meta.macro_expansion_diagnostics,
                        &doc.line_index,
                    ),
                );
            }

            // When lint is not explicitly configured, suppress lint diagnostics but
            // keep component usage diagnostics (type-level, not lint rules).
            if !lint_explicitly_configured {
                diags.retain(|d| match &d.code {
                    Some(NumberOrString::String(code)) => {
                        if code == "verter/unknown-prop"
                            || code == "verter/unknown-model"
                            || code == crate::features::diagnostics::TYPE_EXPANSION_BUDGET_CODE
                            || code == crate::features::diagnostics::TYPE_QUERY_DEPTH_LIMIT_CODE
                        {
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
    kind: crate::type_provider::protocol::CompletionKind,
) -> bool {
    matches!(
        kind,
        crate::type_provider::protocol::CompletionKind::Variable
            | crate::type_provider::protocol::CompletionKind::Function
            | crate::type_provider::protocol::CompletionKind::Method
            | crate::type_provider::protocol::CompletionKind::Property
            | crate::type_provider::protocol::CompletionKind::Field
            | crate::type_provider::protocol::CompletionKind::Constant
            | crate::type_provider::protocol::CompletionKind::EnumMember
    )
}

pub(super) fn is_member_access_completion_kind(
    kind: crate::type_provider::protocol::CompletionKind,
) -> bool {
    matches!(
        kind,
        crate::type_provider::protocol::CompletionKind::Property
            | crate::type_provider::protocol::CompletionKind::Field
            | crate::type_provider::protocol::CompletionKind::Method
            | crate::type_provider::protocol::CompletionKind::Constant
            | crate::type_provider::protocol::CompletionKind::EnumMember
    )
}

pub(super) fn filter_type_provider_completion_result(
    type_result: &mut crate::type_provider::protocol::CompletionResult,
    expr_context: Option<&ExpressionContext>,
    identifier_prefix: Option<&str>,
    verter_items: Option<&Vec<CompletionItem>>,
    enforce_verter_scope_allowlist: bool,
    provider_only_template_scope: Option<&std::collections::HashSet<String>>,
) {
    if !matches!(expr_context, Some(ExpressionContext::MemberAccess)) {
        if let Some(scope) = provider_only_template_scope {
            let before = type_result.items.len();
            type_result
                .items
                .retain(|item| scope.contains(item.label.as_str()));
            tracing::debug!(
                "completion: bounded provider-only template scope: {} -> {} items",
                before,
                type_result.items.len()
            );
        }
    }

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
    } else if enforce_verter_scope_allowlist
        && matches!(expr_context, Some(ExpressionContext::Unknown))
    {
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

/// Collect the template locals visible at `cursor_offset` from the semantic
/// element tree. This complements script/render-proxy names when a TypeScript
/// provider owns completion output: v-for and v-slot bindings are genuine
/// generated lexical locals, but are not script bindings and therefore cannot
/// be recovered from the outer component scope.
pub(super) fn template_lexical_scope_names(
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    cursor_offset: u32,
) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let Some(mut element_index) = template
        .elements
        .iter()
        .enumerate()
        .filter(|(_, element)| {
            element.span.start <= cursor_offset && cursor_offset <= element.span.end
        })
        .max_by_key(|(_, element)| element.nesting_depth)
        .map(|(index, _)| index)
    else {
        return names;
    };

    loop {
        let element = &template.elements[element_index];
        if let Some(v_for) = &element.v_for {
            names.insert(v_for.variable.clone());
            if let Some(index) = &v_for.index {
                names.insert(index.clone());
            }
        }
        for directive in &element.directives {
            if directive.name == "slot" {
                if let Some(pattern) = directive.expression.as_deref() {
                    names.extend(parse_template_binding_pattern_names(pattern));
                }
            }
        }

        let Some(parent_index) = element.parent_index else {
            break;
        };
        let Ok(parent_index) = usize::try_from(parent_index) else {
            break;
        };
        if parent_index >= template.elements.len() {
            break;
        }
        element_index = parent_index;
    }

    names
}

fn parse_template_binding_pattern_names(pattern: &str) -> std::collections::HashSet<String> {
    let allocator = oxc_allocator::Allocator::new();
    let wrapped = format!("({pattern}) => {{}}");
    let Ok(expression) = oxc_parser::Parser::new(&allocator, &wrapped, oxc_span::SourceType::tsx())
        .parse_expression()
    else {
        return std::collections::HashSet::new();
    };
    let oxc_ast::ast::Expression::ArrowFunctionExpression(arrow) = expression else {
        return std::collections::HashSet::new();
    };

    let mut names = std::collections::HashSet::new();
    for parameter in &arrow.params.items {
        collect_template_binding_pattern_names(&parameter.pattern, &mut names);
    }
    names
}

fn collect_template_binding_pattern_names(
    pattern: &oxc_ast::ast::BindingPattern<'_>,
    names: &mut std::collections::HashSet<String>,
) {
    use oxc_ast::ast::BindingPattern;
    match pattern {
        BindingPattern::BindingIdentifier(identifier) => {
            names.insert(identifier.name.to_string());
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_template_binding_pattern_names(&property.value, names);
            }
            if let Some(rest) = &object.rest {
                collect_template_binding_pattern_names(&rest.argument, names);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                collect_template_binding_pattern_names(element, names);
            }
            if let Some(rest) = &array.rest {
                collect_template_binding_pattern_names(&rest.argument, names);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_template_binding_pattern_names(&assignment.left, names);
        }
    }
}
