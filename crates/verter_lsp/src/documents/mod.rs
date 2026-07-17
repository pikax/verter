pub mod line_index;
pub mod position_map;
pub mod provider_projection;
pub mod sfc_scanner;

use std::sync::Arc;

use parking_lot::RwLock;

use dashmap::DashMap;
use tower_lsp_server::ls_types::*;
use verter_session::{
    CompileProfile, FileLanguage, HostUpdateResult, IdeResponse, StyleOverrideEntry,
    StyleOverrideRequest, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

use crate::server::{CodeBlock, VirtualFileEntry, VirtualFilesResponse};
use crate::uri::{file_uri_to_path, percent_decode};

use line_index::LineIndex;
use position_map::PositionMapper;
use provider_projection::{
    DocumentProviderProjection, ProviderPositionMapper, SelfFileProviderMapper,
};

/// Manages open documents and their relationship to verter_session.
pub struct DocumentRegistry {
    pub(crate) host: Arc<VerterHost>,
    /// Map from document URI to document state.
    documents: DashMap<String, DocumentState>,
    /// Default compile profile for TSX generation (LSP mode).
    /// Wrapped in `Arc<RwLock>` so `set_embed_ambient_types()` can update it
    /// after `initialized()` determines whether `@verter/types` is installed.
    /// Arc-wrapped so the background init task can share the same profile instance
    /// and see `embed_ambient_types` toggled without cloning.
    pub(crate) tsx_profile: Arc<RwLock<CompileProfile>>,
    /// Negotiated position encoding from the client (LSP 3.17).
    /// Set once during `initialize()`, before any documents are opened.
    encoding: RwLock<PositionEncodingKind>,
    /// The authoritative, generation-stamped store of provider surfaces synced
    /// to the type provider. Owned here — the single shared document/host facade
    /// reached by the server, the sync coordinator, AND the background-drain free
    /// functions (which already receive `&DocumentRegistry`) — so EVERY sync/close
    /// path records/forgets a generation through one owner. A cross-file rename
    /// captures the current snapshot set under a fence and maps a returned offset
    /// only against the exact generation it captured.
    provider_surfaces: crate::provider_surface_store::ProviderSurfaceStore,
}

/// Tracked state for an open document.
pub struct DocumentState {
    /// The canonical ID used with verter_session.
    pub canonical_id: String,
    /// Current document version (from LSP client).
    pub version: i32,
    /// Current source text.
    pub source: Arc<str>,
    /// Precomputed line index for byte-offset ↔ LSP Position conversion.
    pub line_index: LineIndex,
    /// The document's provider projection (rebuilt on each document change):
    /// the source↔provider position mapper plus the discriminant of which
    /// provider buffer this document projects into (`CarrierIde` for a Vue /
    /// Svelte carrier, `SelfFile` for a `.svelte.ts` / `.svelte.js` rune
    /// module). `None` for a plain script / virtual file (no provider
    /// projection). The `provider_path` is NOT stored here — it is DERIVED from
    /// the committed provider-sync state (carrier IDE path) or IS the canonical
    /// id (self-file).
    pub projection: Option<DocumentProviderProjection>,
    /// Language ID (e.g., "vue", "typescript").
    pub language_id: String,
    /// For virtual files (`verter-virtual://`): the source .vue file URI.
    /// When set, this document is a virtual file and feature requests should
    /// be routed through the source file's TSX for TSGO queries.
    pub virtual_source_uri: Option<String>,
}

impl DocumentRegistry {
    pub fn new(host: Arc<VerterHost>) -> Self {
        Self {
            host,
            documents: DashMap::new(),
            tsx_profile: Arc::new(RwLock::new(CompileProfile {
                source_map: true,
                target: verter_session::CompileTarget::IDE
                    | verter_session::CompileTarget::TEMPLATE_DATA,
                ..CompileProfile::default()
            })),
            encoding: RwLock::new(PositionEncodingKind::UTF16),
            provider_surfaces: crate::provider_surface_store::ProviderSurfaceStore::new(),
        }
    }

    /// The generation-stamped provider-surface store (the authority behind
    /// fail-closed cross-file rename mapping).
    pub fn provider_surfaces(&self) -> &crate::provider_surface_store::ProviderSurfaceStore {
        &self.provider_surfaces
    }

    /// Set the negotiated position encoding. Called once during `initialize()`,
    /// before any documents are opened.
    pub fn set_encoding(&self, encoding: PositionEncodingKind) {
        *self.encoding.write() = encoding;
    }

    /// Build the self-file provider projection for an own-path provider
    /// document: a Svelte rune module (`.svelte.ts` / `.svelte.js`) gets the
    /// rune-prelude line count plus the import-specifier rewrite segments; a
    /// plain TS-family script gets a zero-prelude mapper over the same
    /// rewrite segments (its provider buffer is the source verbatim).
    /// `None` for framework carriers and for unknown extensions (a `.md` /
    /// extensionless document is never served to the TypeScript provider).
    ///
    /// `replacements` are the `(byte_start, byte_end, replacement)` import-
    /// specifier rewrites computed against `source` (the resolver-backed
    /// `compute_specifier_replacements`). They may be EMPTY before resolver
    /// ownership is ready — the prelude offset alone is a correct uniform line
    /// shift for every position; the rewrite column shifts refine import lines
    /// once the resolver lands.
    fn build_self_file_projection(
        canonical_id: &str,
        source: &str,
        replacements: &[(usize, usize, String)],
        line_index: &LineIndex,
    ) -> Option<DocumentProviderProjection> {
        // Path-gated: the registry extension table is the authority, so an
        // unknown extension (which the host classifier's catch-all would
        // report as a TS script) builds NO projection.
        let file_language = crate::server::self_file_language_for(canonical_id)?;
        let built = verter_session::framework::self_file_provider_content(&file_language, source)?;
        let mapper =
            SelfFileProviderMapper::new(built.prelude_line_count, replacements, line_index);
        Some(DocumentProviderProjection::SelfFile { mapper })
    }

    /// Enable embedding ambient `declare module "@verter/types"` in generated TSX.
    /// Called when `@verter/types` is not installed in the workspace.
    pub fn set_embed_ambient_types(&self, embed: bool) {
        self.tsx_profile.write().embed_ambient_types = embed;
    }

    /// Get the negotiated encoding.
    pub fn encoding(&self) -> PositionEncodingKind {
        self.encoding.read().clone()
    }

    /// Resolve the [`FileLanguage`] row for an editor document.
    ///
    /// The client's `language_id` is authoritative for a framework CARRIER
    /// (an in-memory carrier document may not carry its `.vue` / `.svelte`
    /// path); every other document classifies by canonical path through the
    /// host's language classifier — the same authority the workspace-scan
    /// ingress uses, so one file resolves one `FileLanguage` row regardless of
    /// which ingress loaded it. The carrier mapping is REGISTRY-driven
    /// (`carrier_for_editor_language_id`), not a hardcoded `== "vue"` branch:
    /// any registered carrier (`vue`, `svelte`, …) resolves to its framework
    /// row here and the host upsert parses it through the registered carrier —
    /// never silently as a plain script.
    fn document_file_language(&self, language_id: &str, canonical_id: &str) -> FileLanguage {
        verter_session::LanguageRegistry::global()
            .carrier_for_editor_language_id(language_id)
            .unwrap_or_else(|| self.host.language_classifier().classify(canonical_id))
    }

    /// Handle a document being opened in the editor.
    pub fn did_open(&self, params: &TextDocumentItem) -> HostUpdateResult {
        let uri_str = params.uri.as_str().to_string();

        // Handle virtual files (verter-virtual:// scheme) — lightweight state only
        if let Some(source_uri) = parse_virtual_uri(&uri_str) {
            let source: Arc<str> = Arc::from(params.text.as_str());
            let state = DocumentState {
                canonical_id: uri_str.clone(),
                version: params.version,
                line_index: LineIndex::new(&source, self.encoding()),
                source,
                projection: None,
                language_id: params.language_id.clone(),
                virtual_source_uri: Some(source_uri),
            };
            self.documents.insert(uri_str.clone(), state);
            return HostUpdateResult::no_change(uri_str);
        }

        let canonical_id = uri_to_canonical_id(&params.uri);
        let source: Arc<str> = Arc::from(params.text.as_str());

        let file_language = self.document_file_language(&params.language_id, &canonical_id);
        // Every framework CARRIER (Vue OR Svelte) projects an IDE TSX file +
        // source map — the compile + position-mapper paths are carrier-general.
        let is_carrier = file_language.is_framework_carrier();

        let result = self.host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: source.clone(),
            file_language: file_language.clone(),
            aliases: vec![],
        });

        // Trigger compilation to populate the TSX cache (upsert only parses).
        // IDE-sync: drive the IDE/TSX surface, NOT the runtime `Main` node — a
        // Main-less carrier (Svelte) projects only `CachedTsx`, so
        // `ensure_ide_compiled` populates it where `ensure_compiled` (which
        // demands `Main`) would not. `get_ide` below then reads the source map.
        if is_carrier {
            let _ = self
                .host
                .ensure_ide_compiled(&canonical_id, &self.tsx_profile.read());
        }

        let line_index = LineIndex::new(&source, self.encoding());

        // Build the document's provider projection.
        // Always build on did_open — even if the host reports `changed: false`
        // (e.g., because scan_workspace already loaded the same content), we still
        // need the mapper for hover/definition/diagnostics.
        //  - CARRIER (`.vue` / `.svelte`): the IDE TSX source-map projection.
        //  - SELF-FILE (rune module OR plain TS-family script): the line-only
        //    rewrite-aware projection (prelude-offset-only at did_open; the
        //    server refines it with resolver-backed rewrite segments once
        //    ownership is ready). Plain scripts carry a zero-line prelude —
        //    their provider buffer is the source verbatim.
        let projection = if is_carrier {
            self.host
                .get_ide(&canonical_id, &self.tsx_profile.read())
                .and_then(|tsx| PositionMapper::from_json(&tsx.source_map?).ok())
                .map(DocumentProviderProjection::carrier_ide)
        } else {
            Self::build_self_file_projection(&canonical_id, &source, &[], &line_index)
        };

        let state = DocumentState {
            canonical_id,
            version: params.version,
            line_index,
            source,
            projection,
            language_id: params.language_id.clone(),
            virtual_source_uri: None,
        };

        self.documents.insert(uri_str.clone(), state);

        result.unwrap_or_else(|e| {
            tracing::error!("upsert failed for {}: {:?}", uri_str, e);
            HostUpdateResult::no_change(uri_to_canonical_id(&params.uri))
        })
    }

    /// Handle a document being changed.
    pub fn did_change(&self, uri: &Uri, version: i32, text: &str) -> HostUpdateResult {
        let uri_str = uri.as_str().to_string();
        tracing::info!(
            "DocumentRegistry::did_change UPSERT_START v{version} thread={:?}",
            std::thread::current().id()
        );

        // Virtual files: just update text + line index, skip host upsert
        if let Some(mut entry) = self.documents.get_mut(&uri_str) {
            if entry.virtual_source_uri.is_some() {
                let source: Arc<str> = Arc::from(text);
                entry.version = version;
                entry.line_index = LineIndex::new(&source, self.encoding());
                entry.source = source;
                return HostUpdateResult::no_change(entry.canonical_id.clone());
            }
        }

        let canonical_id = uri_to_canonical_id(uri);
        let source: Arc<str> = Arc::from(text);

        let stored_language_id = self
            .documents
            .get(&uri_str)
            .map(|d| d.language_id.clone())
            .unwrap_or_default();
        let file_language = self.document_file_language(&stored_language_id, &canonical_id);
        // Carrier-general: every framework carrier (Vue OR Svelte) re-compiles
        // its IDE TSX on change.
        let is_carrier = file_language.is_framework_carrier();

        let upsert_start = std::time::Instant::now();
        let result = self.host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: source.clone(),
            file_language,
            aliases: vec![],
        });
        tracing::info!(
            "DocumentRegistry::did_change HOST_UPSERT_DONE elapsed={:?} thread={:?}",
            upsert_start.elapsed(),
            std::thread::current().id()
        );

        // Update VFS overlay with latest buffer content. sub-plan
        // §6b.D2b — route through `host.notify_upsert` so the route-only
        // shallow cache is evicted alongside the workspace overlay write.
        #[cfg(not(target_arch = "wasm32"))]
        self.host.notify_upsert(&canonical_id, source.clone());

        // Trigger re-compilation for framework carriers (Vue / Svelte SFCs).
        // IDE-sync: drive the IDE/TSX surface (not the runtime `Main`) so a
        // Main-less carrier (Svelte) re-populates its `CachedTsx`; `get_ide`
        // below rebuilds the position mapper from the fresh source map.
        if is_carrier {
            let compile_start = std::time::Instant::now();
            let _ = self
                .host
                .ensure_ide_compiled(&canonical_id, &self.tsx_profile.read());
            tracing::info!(
                "DocumentRegistry::did_change ENSURE_COMPILED_DONE elapsed={:?} thread={:?}",
                compile_start.elapsed(),
                std::thread::current().id()
            );
        }

        let new_line_index = LineIndex::new(&source, self.encoding());

        // Rebuild the document's provider projection.
        // When compilation fails (e.g., temporarily invalid SFC during typing),
        // preserve the old projection so position-dependent features keep working.
        let rebuild_carrier = |this: &Self| -> Option<DocumentProviderProjection> {
            this.host
                .get_ide(&canonical_id, &this.tsx_profile.read())
                .and_then(|tsx| PositionMapper::from_json(&tsx.source_map?).ok())
                .map(DocumentProviderProjection::carrier_ide)
        };
        let prior_projection = || {
            self.documents
                .get(&uri_str)
                .and_then(|d| d.projection.clone())
        };
        let projection = if is_carrier {
            match &result {
                Ok(update) if update.changed => {
                    // Keep old projection if new compilation failed (Bug 3 fix).
                    rebuild_carrier(self).or_else(prior_projection)
                }
                _ => prior_projection(),
            }
        } else {
            // Self-file (rune module or plain TS-family script; unknown
            // extension → `None`). The prelude offset is content-independent;
            // rebuild it whole-line (rewrite segments get refined by the
            // server once the resolver is ready).
            Self::build_self_file_projection(&canonical_id, &source, &[], &new_line_index)
        };

        if let Some(mut entry) = self.documents.get_mut(&uri_str) {
            entry.version = version;
            entry.line_index = new_line_index;
            entry.source = source;
            entry.projection = projection;
        }

        result.unwrap_or_else(|e| {
            tracing::error!("upsert failed for {}: {:?}", uri_str, e);
            HostUpdateResult::no_change(canonical_id)
        })
    }

    /// Handle incremental document changes.
    ///
    /// Applies each content change event to the stored source text:
    /// - If a change has a `range`, it replaces that range in the source.
    /// - If a change has no `range`, it replaces the entire document (full sync fallback).
    ///
    /// After applying all changes, delegates to `did_change` with the final text.
    pub fn did_change_incremental(
        &self,
        uri: &Uri,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> HostUpdateResult {
        let uri_str = uri.as_str().to_string();

        // Get current source text
        let mut text = match self.documents.get(&uri_str) {
            Some(doc) => doc.source.to_string(),
            None => {
                // No document tracked — use last change's full text as fallback
                if let Some(change) = changes.last() {
                    return self.did_change(uri, version, &change.text);
                }
                return HostUpdateResult::no_change(uri_to_canonical_id(uri));
            }
        };

        // Apply each change in order
        for change in &changes {
            if let Some(range) = change.range {
                // Incremental change: apply to specific range
                let line_index = LineIndex::new(&text, self.encoding());
                let start_offset =
                    line_index.position_to_offset(&range.start).unwrap_or(0) as usize;
                let end_offset = line_index
                    .position_to_offset(&range.end)
                    .unwrap_or(text.len() as u32) as usize;

                let start_offset = start_offset.min(text.len());
                let end_offset = end_offset.min(text.len());

                text.replace_range(start_offset..end_offset, &change.text);
            } else {
                // Full replacement (no range = full document)
                text = change.text.clone();
            }
        }

        self.did_change(uri, version, &text)
    }

    /// Handle a document being closed.
    pub fn did_close(&self, uri: &Uri) {
        // Clear the VFS overlay so resolution falls back to snapshot/disk.
        let canonical_id = uri_to_canonical_id(uri);
        // route through `host.notify_close`
        // so the route-only shallow cache is evicted alongside the
        // workspace overlay clear.
        self.host.notify_close(&canonical_id);

        self.documents.remove(uri.as_str());
    }

    /// Get the document state for a URI.
    pub fn get(&self, uri: &Uri) -> Option<dashmap::mapref::one::Ref<'_, String, DocumentState>> {
        self.documents.get(uri.as_str())
    }

    /// Get the document's provider projection (the source↔provider mapper +
    /// the projection discriminant).
    pub fn get_projection(&self, uri: &Uri) -> Option<DocumentProviderProjection> {
        self.documents.get(uri.as_str())?.projection.clone()
    }

    /// Get the unified source↔provider position mapper for a document, ready
    /// for the feature layer (projection-agnostic).
    pub fn get_position_mapper(&self, uri: &Uri) -> Option<ProviderPositionMapper> {
        Some(
            self.documents
                .get(uri.as_str())?
                .projection
                .as_ref()?
                .mapper(),
        )
    }

    /// Get the canonical ID for a document URI.
    pub fn get_canonical_id(&self, uri: &Uri) -> Option<String> {
        self.documents
            .get(uri.as_str())
            .map(|d| d.canonical_id.clone())
    }

    /// Reverse lookup: find the URI for a given canonical ID.
    ///
    /// Iterates all open documents to find a match. Used by completion resolve
    /// to map TSX paths back to Vue URIs.
    pub fn canonical_id_to_uri(&self, canonical_id: &str) -> Option<Uri> {
        for entry in self.documents.iter() {
            if entry.value().canonical_id == canonical_id {
                return entry.key().parse().ok();
            }
        }
        None
    }

    /// Get the IDE output (TSX or JSX) for a document.
    ///
    /// If compile_slots were cleared (e.g., by dependency invalidation via `did_open`
    /// on a `.ts` file), lazily recompiles the Vue file to restore IDE output.
    /// This prevents "no IDE context" failures after peek definition or go-to-definition
    /// opens a dependency file.
    pub fn get_ide(&self, uri: &Uri) -> Option<IdeResponse> {
        let canonical_id = self.get_canonical_id(uri)?;
        let profile = self.tsx_profile.read().clone();

        // Fast path: cache hit
        if let Some(resp) = self.host.get_ide(&canonical_id, &profile) {
            // Lazily rebuild position mapper if it was None (startup race:
            // did_open runs before background_init completes, so the mapper
            // may not have been built, but the workspace scanner later compiles
            // the file and caches TSX in the host).
            if let Some(entry) = self.documents.get(uri.as_str()) {
                if entry.projection.is_none() {
                    drop(entry);
                    if let Some(mut entry) = self.documents.get_mut(uri.as_str()) {
                        if entry.projection.is_none() {
                            if let Some(mapper) = resp
                                .source_map
                                .as_ref()
                                .and_then(|sm| PositionMapper::from_json(sm).ok())
                            {
                                entry.projection =
                                    Some(DocumentProviderProjection::carrier_ide(mapper));
                            }
                        }
                    }
                }
            }
            return Some(resp);
        }

        // Slow path: compile_slots were cleared (e.g., dependency invalidation).
        // Lazily recompile to restore IDE output. Carrier-general (Vue / Svelte).
        let is_carrier = self
            .documents
            .get(uri.as_str())
            .map(|d| {
                self.document_file_language(&d.language_id, &canonical_id)
                    .is_framework_carrier()
            })
            .unwrap_or(false);
        if !is_carrier {
            return None;
        }

        // IDE-sync: drive the IDE/TSX surface, NOT the runtime `Main` node. A
        // Main-less carrier (Svelte) has a `CachedTsx` but no `Main`, so
        // `ensure_compiled` (which demands `Main`) would return
        // `MissingVirtualNode` and abort here even though the IDE TSX exists.
        // `ensure_ide_compiled` resolves through the `Ide` demand; `Ok(false)`
        // (a genuine no-IDE surface) skips, `Ok(true)` proceeds to `get_ide`.
        if !self
            .host
            .ensure_ide_compiled(&canonical_id, &profile)
            .ok()?
        {
            return None;
        }
        let resp = self.host.get_ide(&canonical_id, &profile)?;

        // Rebuild position mapper since TSX output was regenerated
        if let Some(mut entry) = self.documents.get_mut(uri.as_str()) {
            if let Some(mapper) = resp
                .source_map
                .as_ref()
                .and_then(|sm| PositionMapper::from_json(sm).ok())
            {
                entry.projection = Some(DocumentProviderProjection::carrier_ide(mapper));
            }
        }

        Some(resp)
    }

    /// Force-recompile a file and rebuild its PositionMapper from the fresh source map.
    ///
    /// Used after blocker hydration or snapshot reconciliation triggers recompilation
    /// that may have changed the TSX output. Without this, hover would query correct
    /// TSX with stale position offsets.
    pub fn recompile_and_refresh_mapper(&self, uri: &Uri) -> Option<IdeResponse> {
        let canonical_id = self.get_canonical_id(uri)?;
        // Carrier-general (Vue / Svelte) — every carrier projects an IDE TSX.
        let is_carrier = self
            .documents
            .get(uri.as_str())
            .map(|d| {
                self.document_file_language(&d.language_id, &canonical_id)
                    .is_framework_carrier()
            })
            .unwrap_or(false);
        if !is_carrier {
            return None;
        }
        let profile = self.tsx_profile.read().clone();
        // IDE-sync: drive the IDE/TSX surface (not the runtime `Main`) so a
        // Main-less carrier (Svelte) refreshes its mapper. `Ok(false)` (no IDE
        // surface) returns None; `Ok(true)` proceeds to `get_ide`.
        if !self
            .host
            .ensure_ide_compiled(&canonical_id, &profile)
            .ok()?
        {
            return None;
        }
        let resp = self.host.get_ide(&canonical_id, &profile)?;
        // Always rebuild mapper from fresh source map
        if let Some(mut entry) = self.documents.get_mut(uri.as_str()) {
            entry.projection = resp
                .source_map
                .as_ref()
                .and_then(|sm| PositionMapper::from_json(sm).ok())
                .map(DocumentProviderProjection::carrier_ide);
        }
        Some(resp)
    }

    /// Refine an OPEN self-file document's projection with resolver-backed
    /// import-specifier rewrite segments.
    ///
    /// At did_open / did_change the projection carries the prelude offset with
    /// NO rewrite segments (the resolver may not be ready). Once the server has
    /// a published resolver it computes the rewrite replacements and calls this
    /// to refine the rewrite-aware column mapping. A no-op when the document is
    /// not a `SelfFile` projection (e.g. a carrier or an unknown extension).
    pub fn refresh_self_file_rewrites(&self, uri: &Uri, replacements: &[(usize, usize, String)]) {
        let Some(mut entry) = self.documents.get_mut(uri.as_str()) else {
            return;
        };
        if !matches!(
            entry.projection,
            Some(DocumentProviderProjection::SelfFile { .. })
        ) {
            return;
        }
        let canonical_id = entry.canonical_id.clone();
        let source = entry.source.clone();
        let line_index = entry.line_index.clone();
        if let Some(projection) =
            Self::build_self_file_projection(&canonical_id, &source, replacements, &line_index)
        {
            entry.projection = Some(projection);
        }
    }

    /// Check if a document's IDE output is JavaScript (JSX) rather than TypeScript (TSX).
    ///
    /// The IDE compile is the authority, but a cold caller (no compiled IDE
    /// output yet) must not GUESS `.tsx`: the parse-level script dialect is
    /// available after upsert and names the companion correctly from the
    /// start. Without the fallback a JS carrier first opens `{carrier}.tsx`,
    /// then flips to `{carrier}.jsx` once the compile lands — and tsserver's
    /// output-file membership check excludes the `.jsx` while the stale
    /// `.tsx` is still in the Program.
    pub fn is_jsx(&self, uri: &Uri) -> bool {
        if let Some(ide) = self.get_ide(uri) {
            return ide.is_jsx;
        }
        self.get_analysis(uri)
            .map(|analysis| !analysis.is_typescript)
            .unwrap_or(false)
    }

    /// Canonical-id variant of [`Self::is_jsx`], host-level (no open-document
    /// requirement): the IDE compile is authoritative and the parse-level
    /// script dialect is the cold fallback. Gateway/publish call sites that
    /// hold an `Option<IdeResponse>` must route through this rather than
    /// `map(is_jsx).unwrap_or(false)` — the `unwrap_or(false)` guesses `.tsx`
    /// for a JS carrier whose compile is momentarily unavailable (the
    /// `.tsx` → `.jsx` companion flip tsserver's output-file check rejects).
    pub fn is_jsx_for_canonical(&self, canonical_id: &str) -> bool {
        let profile = self.tsx_profile.read().clone();
        if let Some(ide) = self.host.get_ide(canonical_id, &profile) {
            return ide.is_jsx;
        }
        self.host
            .get_analysis(canonical_id)
            .map(|analysis| !analysis.is_typescript)
            .unwrap_or(false)
    }

    /// Get the analysis snapshot for a document.
    pub fn get_analysis(&self, uri: &Uri) -> Option<verter_session::FileAnalysisSnapshot> {
        let canonical_id = self.get_canonical_id(uri)?;
        self.host.get_analysis(&canonical_id)
    }

    /// Get the diagnostics for a document.
    pub fn get_diagnostics(&self, uri: &Uri) -> Option<verter_session::DiagnosticsSnapshot> {
        let canonical_id = self.get_canonical_id(uri)?;
        self.host
            .get_diagnostics(&canonical_id, &self.tsx_profile.read())
    }

    /// Get all virtual files for a document, including TSX output.
    pub fn get_virtual_files(&self, uri: &Uri) -> Option<VirtualFilesResponse> {
        let canonical_id = self.get_canonical_id(uri)?;

        // Get IDE output (TSX/JSX for template type checking)
        let ide = self
            .host
            .get_ide(&canonical_id, &self.tsx_profile.read())
            .map(|t| CodeBlock {
                code: t.code.to_string(),
                source_map: t.source_map.map(|m| m.to_string()),
                is_js: t.is_jsx,
            });

        // Get API output (declaration for cross-file type resolution)
        let is_js = ide.as_ref().is_some_and(|b| b.is_js);
        let api = self.host.get_public_api(&canonical_id).map(|t| CodeBlock {
            code: t.code.to_string(),
            source_map: t.source_map.map(|m| m.to_string()),
            is_js,
        });

        // Get all virtual node kinds
        let node_kinds = self.host.list_virtual_nodes(&canonical_id);

        // Fetch each virtual file
        let mut virtual_files = Vec::with_capacity(node_kinds.len());
        for kind in &node_kinds {
            let kind_str = match kind {
                VirtualNodeKind::Main => "main".to_string(),
                VirtualNodeKind::Script => "script".to_string(),
                VirtualNodeKind::Template => "template".to_string(),
                VirtualNodeKind::Style { index } => format!("style:{index}"),
                VirtualNodeKind::Custom { index } => format!("custom:{index}"),
            };

            let vf = self.host.get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(canonical_id.clone()),
                node_kind: Some(kind.clone()),
                compile_profile: self.tsx_profile.read().clone(),
            });

            match vf {
                Ok(response) => {
                    virtual_files.push(VirtualFileEntry {
                        kind: kind_str,
                        code: response.code.to_string(),
                        lang: response.lang.unwrap_or_else(|| "js".to_string()),
                        source_map: response.source_map.map(|m| m.to_string()),
                        stale: response.stale,
                    });
                }
                Err(_) => {
                    // Skip virtual files that fail to compile
                    virtual_files.push(VirtualFileEntry {
                        kind: kind_str,
                        code: String::new(),
                        lang: "js".to_string(),
                        source_map: None,
                        stale: true,
                    });
                }
            }
        }

        Some(VirtualFilesResponse {
            ide,
            api,
            virtual_files,
        })
    }

    /// Get the analysis snapshot as a JSON value.
    ///
    /// When the negotiated encoding is not UTF-8, all `spanStart`/`spanEnd` byte
    /// offsets in the analysis are converted to the negotiated encoding (UTF-16 or UTF-32).
    pub fn get_analysis_json(&self, uri: &Uri) -> Option<serde_json::Value> {
        let canonical_id = self.get_canonical_id(uri)?;
        let analysis = self.host.get_analysis(&canonical_id)?;
        let mut json = serde_json::to_value(&analysis).ok()?;
        let encoding = self.encoding();
        if encoding != PositionEncodingKind::UTF8 {
            // UTF-8 byte offsets are native — no conversion needed.
            // For UTF-16 or UTF-32, convert all span offsets.
            let source = self.documents.get(uri.as_str())?.source.clone();
            convert_analysis_spans_json(&mut json, &source, &encoding);
        }
        Some(json)
    }

    /// Get the virtual source URI for a document (if it's a virtual file).
    pub fn get_virtual_source_uri(&self, uri: &Uri) -> Option<String> {
        self.documents.get(uri.as_str())?.virtual_source_uri.clone()
    }

    /// Apply preprocessor-compiled style overrides for a document.
    ///
    /// Returns `true` if the overrides were applied successfully.
    pub fn apply_style_overrides(
        &self,
        canonical_id: &str,
        overrides: Vec<StyleOverrideEntry>,
    ) -> bool {
        let req = StyleOverrideRequest {
            canonical_id: canonical_id.to_string(),
            compile_profile: self.tsx_profile.read().clone(),
            overrides,
        };
        match self.host.apply_style_overrides(req) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!("apply_style_overrides failed: {e:?}");
                false
            }
        }
    }

    /// Return the URI strings of all currently open documents.
    pub fn open_uris(&self) -> Vec<String> {
        self.documents.iter().map(|e| e.key().clone()).collect()
    }

    /// Get the underlying verter_session reference.
    pub fn host(&self) -> &VerterHost {
        &self.host
    }

    /// Get a shared reference to the host (for MCP embedding).
    pub fn host_arc(&self) -> Arc<VerterHost> {
        Arc::clone(&self.host)
    }
}

// ── Analysis span conversion ─────────────────────────────────────────

/// Recursively walk a JSON value and convert all `spanStart`/`spanEnd` fields
/// from UTF-8 byte offsets to the negotiated encoding.
fn convert_analysis_spans_json(
    value: &mut serde_json::Value,
    source: &str,
    encoding: &PositionEncodingKind,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if let Some(byte_offset) = val
                    .as_u64()
                    .filter(|_| key == "spanStart" || key == "spanEnd")
                {
                    let byte_offset = byte_offset as u32;
                    let converted = convert_byte_offset(source, byte_offset, encoding);
                    *val = serde_json::Value::Number(serde_json::Number::from(converted));
                } else {
                    convert_analysis_spans_json(val, source, encoding);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                convert_analysis_spans_json(item, source, encoding);
            }
        }
        _ => {}
    }
}

/// Convert a UTF-8 byte offset to the target encoding's offset.
fn convert_byte_offset(source: &str, byte_offset: u32, encoding: &PositionEncodingKind) -> u32 {
    if *encoding == PositionEncodingKind::UTF16 {
        byte_offset_to_utf16(source, byte_offset)
    } else if *encoding == PositionEncodingKind::UTF32 {
        byte_offset_to_utf32(source, byte_offset)
    } else {
        byte_offset // UTF-8 passthrough
    }
}

/// Convert a UTF-8 byte offset to a UTF-16 code unit offset.
fn byte_offset_to_utf16(source: &str, byte_offset: u32) -> u32 {
    let clamped = clamp_to_char_boundary(source, byte_offset as usize);
    source[..clamped].encode_utf16().count() as u32
}

/// Convert a UTF-8 byte offset to a UTF-32 (Unicode code point) offset.
fn byte_offset_to_utf32(source: &str, byte_offset: u32) -> u32 {
    let clamped = clamp_to_char_boundary(source, byte_offset as usize);
    source[..clamped].chars().count() as u32
}

/// Clamp an offset to the nearest valid char boundary (at or before the offset).
fn clamp_to_char_boundary(source: &str, offset: usize) -> usize {
    let clamped = offset.min(source.len());
    // Walk backwards to find a valid char boundary
    let mut pos = clamped;
    while pos > 0 && !source.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Convert an LSP document URI to a canonical file path ID.
///
/// Extracts the path component from `file://` URIs.
/// On Windows, strips the leading `/` from paths like `/C:/Users/...`.
pub fn uri_to_canonical_id(uri: &Uri) -> String {
    uri_to_canonical_id_from_str(uri.as_str())
}

/// Convert a `file://` URI string to a canonical filesystem path ID.
///
/// Handles percent-encoded characters (e.g., `%3A` → `:` on Windows) and
/// restores the leading `/` on Unix, then routes the result through the single
/// canonical-path owner (`verter_span::path`) so the produced ID is byte-equal
/// to every other producer (type providers, VFS ingestion, scheduler). On
/// Windows this lowercases the drive letter (`C:/…` → `c:/…`) — URI-derived IDs
/// previously stayed uppercase and split file identity against the owner-
/// normalized IDs the VFS keys on. On Unix it is a no-op (paths already
/// canonical).
pub fn uri_to_canonical_id_from_str(s: &str) -> String {
    verter_span::path::canonicalize_path(&file_uri_to_path(s))
}
/// Parse a `verter-virtual://` URI and extract the source .vue file URI.
///
/// URI format: `verter-virtual:///kind.lang?sourceUri=<encoded-source-uri>`
/// Returns the decoded source URI if the input is a virtual file URI.
fn parse_virtual_uri(uri: &str) -> Option<String> {
    if !uri.starts_with("verter-virtual://") {
        return None;
    }
    let query_start = uri.find("?sourceUri=")?;
    let encoded = &uri[query_start + "?sourceUri=".len()..];
    Some(percent_decode(encoded))
}

#[cfg(test)]
#[allow(unused_must_use)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_to_canonical_id_unix_file_uri() {
        let uri: Uri = "file:///home/user/project/App.vue".parse().unwrap();
        let id = uri_to_canonical_id(&uri);
        assert_eq!(id, "/home/user/project/App.vue");
    }

    #[test]
    fn test_uri_to_canonical_id_windows_file_uri() {
        // The canonical-ID owner lowercases the Windows drive letter so a
        // URI-derived ID is byte-equal to every other producer (type providers,
        // VFS ingestion) — otherwise `did_open` would key the VFS under
        // `C:/...` while lookups resolve `c:/...`, splitting file identity.
        // Pre-fix this returned the uppercase `C:/...`; this assertion is
        // discriminating.
        let uri: Uri = "file:///C:/Users/dev/project/App.vue".parse().unwrap();
        let id = uri_to_canonical_id(&uri);
        assert_eq!(id, "c:/Users/dev/project/App.vue");
        assert_ne!(id, "C:/Users/dev/project/App.vue");
    }

    #[test]
    fn test_uri_to_canonical_id_with_spaces() {
        let uri: Uri = "file:///home/user/my%20project/App.vue".parse().unwrap();
        let id = uri_to_canonical_id(&uri);
        assert_eq!(id, "/home/user/my project/App.vue");
    }

    #[test]
    fn test_uri_to_canonical_id_non_file_uri() {
        let uri: Uri = "untitled:Untitled-1".parse().unwrap();
        let id = uri_to_canonical_id(&uri);
        assert_eq!(id, "untitled:Untitled-1");
    }

    /// @ai-generated — Windows drive letter with percent-encoded colon (%3A)
    /// VS Code sometimes sends `file:///d%3A/path` instead of `file:///d:/path`.
    #[test]
    fn test_uri_to_canonical_id_windows_encoded_drive() {
        let id = uri_to_canonical_id_from_str("file:///d%3A/dev/example/examples");
        assert_eq!(id, "d:/dev/example/examples");
    }

    /// @ai-generated — Windows path with lowercase drive and normal colon
    #[test]
    fn test_uri_to_canonical_id_windows_lowercase_drive() {
        let id = uri_to_canonical_id_from_str("file:///d:/dev/example");
        assert_eq!(id, "d:/dev/example");
    }

    /// @ai-generated — Verify the from_str variant matches the Uri variant
    #[test]
    fn test_uri_to_canonical_id_from_str_matches_uri() {
        let uri: Uri = "file:///C:/Users/dev/project/App.vue".parse().unwrap();
        assert_eq!(
            uri_to_canonical_id(&uri),
            uri_to_canonical_id_from_str("file:///C:/Users/dev/project/App.vue")
        );
    }

    #[test]
    fn test_parse_virtual_uri_tsx() {
        let uri = "verter-virtual:///tsx.tsx?sourceUri=file%3A%2F%2F%2Fhome%2Fuser%2FApp.vue";
        let source = parse_virtual_uri(uri);
        assert_eq!(source, Some("file:///home/user/App.vue".to_string()));
    }

    #[test]
    fn test_parse_virtual_uri_non_virtual() {
        let uri = "file:///home/user/App.vue";
        assert_eq!(parse_virtual_uri(uri), None);
    }

    #[test]
    fn open_uris_returns_open_documents() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);

        // Open two documents
        registry.did_open(&TextDocumentItem {
            uri: "file:///home/user/App.vue".parse().unwrap(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><div>hello</div></template>".to_string(),
        });
        registry.did_open(&TextDocumentItem {
            uri: "file:///home/user/Main.vue".parse().unwrap(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><span>world</span></template>".to_string(),
        });

        let uris = registry.open_uris();
        assert_eq!(uris.len(), 2, "should have 2 open documents");
        assert!(
            uris.contains(&"file:///home/user/App.vue".to_string()),
            "should contain App.vue URI"
        );
        assert!(
            uris.contains(&"file:///home/user/Main.vue".to_string()),
            "should contain Main.vue URI"
        );

        // Close one
        registry.did_close(&"file:///home/user/App.vue".parse().unwrap());
        let uris = registry.open_uris();
        assert_eq!(uris.len(), 1, "should have 1 open document after close");
        assert!(
            !uris.contains(&"file:///home/user/App.vue".to_string()),
            "should not contain closed App.vue URI"
        );
        assert!(
            uris.contains(&"file:///home/user/Main.vue".to_string()),
            "should still contain Main.vue URI"
        );
    }

    #[test]
    fn test_parse_virtual_uri_windows() {
        let uri =
            "verter-virtual:///tsx.tsx?sourceUri=file%3A%2F%2F%2FC%3A%2FUsers%2Fdev%2FApp.vue";
        let source = parse_virtual_uri(uri);
        assert_eq!(source, Some("file:///C:/Users/dev/App.vue".to_string()));
    }

    /// Bug 1 regression: `get_ide()` fast path should lazily rebuild a missing
    /// position mapper when the host already has cached TSX output.
    #[test]
    fn position_mapper_lazily_rebuilt_on_fast_path() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/App.vue".parse().unwrap();

        // Open a Vue file — this compiles and builds the mapper
        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><div>hello</div></template><script setup lang=\"ts\">\nconst x = 1;\n</script>".to_string(),
        });

        // Verify mapper was built during did_open
        assert!(
            registry.get_position_mapper(&uri).is_some(),
            "mapper should be built during did_open"
        );

        // Simulate the startup race: clear the projection to None
        if let Some(mut entry) = registry.documents.get_mut(uri.as_str()) {
            entry.projection = None;
        }
        assert!(
            registry.get_position_mapper(&uri).is_none(),
            "mapper should be None after clearing"
        );

        // Call get_ide() — fast path should lazily rebuild the mapper
        let ide = registry.get_ide(&uri);
        assert!(ide.is_some(), "get_ide should return cached TSX");

        // The mapper should now be rebuilt
        assert!(
            registry.get_position_mapper(&uri).is_some(),
            "mapper should be lazily rebuilt on fast path cache hit"
        );
    }

    /// A `.svelte` framework carrier opened in the editor routes through the
    /// language classifier to the REGISTERED Svelte carrier: the upsert
    /// succeeds and the host holds a Svelte carrier source snapshot — it is NOT
    /// silently misparsed as a plain TypeScript script, and NOT rejected as a
    /// known-but-unsupported file.
    #[test]
    fn did_open_svelte_parses_through_the_registered_carrier() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/App.svelte".parse().unwrap();

        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "svelte".to_string(),
            version: 1,
            text:
                "<script lang=\"ts\">let { x }: { x: number } = $props();</script>\n<div>{x}</div>"
                    .to_string(),
        });

        // The editor document stays tracked…
        assert!(
            registry.open_uris().contains(&uri.as_str().to_string()),
            "the opened document must stay tracked in the registry"
        );
        // …AND the host holds a Svelte carrier source snapshot: the registered
        // carrier parses it (positive routing), never a misparsed script.
        assert!(
            registry
                .host()
                .get_source("/home/user/App.svelte")
                .is_some(),
            "the registered Svelte carrier must parse the .svelte document"
        );
    }

    /// End-to-end: a Main-less `.svelte` carrier's IDE TSX reaches the
    /// LSP IDE-sync path. `DocumentRegistry::get_ide`'s SLOW path (compile slots
    /// cleared) drove `host.ensure_compiled(canonical, profile).ok()?` before
    /// the migration — which DEMANDS `VirtualNodeKind::Main`. A Svelte carrier
    /// projects ONLY an IDE `CachedTsx` (no runtime `Main`), so the old
    /// `ensure_compiled` returned `MissingVirtualNode` → `.ok()?` short-circuited
    /// to `None` and the Svelte IDE surface NEVER reached the LSP, even though
    /// the TSX existed. After the migration to `ensure_ide_compiled` (which
    /// resolves through the `Ide` demand, never `Main`), the slow path restores
    /// the IDE TSX.
    ///
    /// DISCRIMINATING: this FAILS against the pre-migration `ensure_compiled`-
    /// gated slow path (it returns `None` for the Main-less carrier) and PASSES
    /// after the migration. A Vue file would not discriminate — Vue has a `Main`
    /// node, so its old gate succeeded; only a Main-less carrier exposes the bug.
    #[test]
    fn main_less_svelte_ide_tsx_reaches_lsp_get_ide_slow_path() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));
        let uri: Uri = "file:///home/user/Counter.svelte".parse().unwrap();
        let canonical = "/home/user/Counter.svelte";

        // Open a Main-less Svelte carrier (no runtime Main; IDE TSX only).
        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "svelte".to_string(),
            version: 1,
            text: "<script lang=\"ts\">let count = 0;</script>\n<button onclick={() => count++}>{count}</button>\n".to_string(),
        });

        // Force `get_ide`'s SLOW path: clear the compile slots so the host's
        // fast-path `get_ide` peek misses and the registry must recompile. This
        // is exactly the dependency-invalidation scenario the slow path exists
        // for — and the one that gated on `ensure_compiled` before the fix.
        registry.host().invalidate_compile_slots(canonical);
        assert!(
            registry
                .host()
                .get_ide(canonical, &registry.tsx_profile.read())
                .is_none(),
            "precondition: the fast-path peek must miss after invalidating the compile slots, \
             so `get_ide` takes the slow recompile path under test"
        );

        // The slow path now drives `ensure_ide_compiled` (Ide demand), so the
        // Main-less Svelte carrier's IDE TSX reaches the LSP.
        let ide = registry.get_ide(&uri);
        assert!(
            ide.is_some(),
            "the Main-less Svelte carrier's IDE TSX must reach the LSP `get_ide` slow path — \
             pre-migration this returned None because `ensure_compiled` demanded a runtime Main \
             the Svelte carrier never produces"
        );
        let ide = ide.unwrap();
        // Svelte-specific IDE output (the @verter/svelte-jsx pragma) — NOT Vue
        // TSX. This confirms the carrier-correct surface reached the LSP.
        assert!(
            ide.code.contains("@jsxImportSource @verter/svelte-jsx"),
            "the LSP must receive the Svelte-specific IDE TSX, got:\n{}",
            ide.code
        );
        assert!(
            !ide.code.contains("_sfc_main"),
            "the LSP must NOT receive Vue SFC TSX residue for a .svelte carrier"
        );
    }

    /// Plain-script documents keep parsing exactly as before: the
    /// classifier resolves the script row for the canonical path and
    /// the upsert succeeds.
    #[test]
    fn did_open_script_document_still_parses() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/util.ts".parse().unwrap();

        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "typescript".to_string(),
            version: 1,
            text: "export const x = 1;".to_string(),
        });

        assert!(
            registry.host().get_source("/home/user/util.ts").is_some(),
            "a plain script document must parse through the script path"
        );
    }

    /// The editor ingress and the workspace-scan ingress resolve the
    /// SAME `FileLanguage` row for the same path: both route through the
    /// host's language classifier. `language_id == "vue"` stays
    /// authoritative for the Vue carrier (an in-memory Vue document may
    /// not carry a `.vue` path).
    #[test]
    fn document_language_agrees_with_host_classifier() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));

        assert_eq!(
            registry.document_file_language("vue", "/x/App.vue"),
            FileLanguage::vue()
        );
        for path in ["/x/a.ts", "/x/a.js", "/x/a.svelte", "/x/notes.md"] {
            assert_eq!(
                registry.document_file_language("plaintext", path),
                host.language_classifier().classify(path),
                "editor ingress must agree with the scan ingress for {path}"
            );
        }
        // The classifier (not a hard-coded TypeScript fallback) decides:
        // a `.js` path resolves the JS script row.
        assert_eq!(
            registry.document_file_language("javascript", "/x/a.js"),
            FileLanguage::script(verter_session::ScriptSourceType::js())
        );
        // A `.svelte` path resolves its framework carrier row — never
        // the Vue row, never a plain script.
        let svelte = registry.document_file_language("svelte", "/x/a.svelte");
        assert!(svelte.is_framework_carrier());
        assert!(!svelte.is_vue());
        assert_ne!(svelte, FileLanguage::script_ts());
    }

    #[test]
    fn did_open_svelte_builds_an_ide_position_mapper() {
        // The carrier-general compile gate (is_framework_carrier, not is_vue)
        // must reach the Svelte IDE projection on did_open — the source map
        // yields a position mapper. A `.svelte` file with the Vue-only gate
        // would build NO mapper (the bug this pins).
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));

        let item = TextDocumentItem {
            uri: "file:///x/Comp.svelte".parse().expect("uri"),
            language_id: "svelte".to_string(),
            version: 1,
            text: "<script lang=\"ts\">let count = 0;</script>\n<div>{count}</div>".to_string(),
        };
        let _ = registry.did_open(&item);

        let state = registry
            .documents
            .get("file:///x/Comp.svelte")
            .expect("the .svelte document is registered");
        assert!(
            matches!(
                state.projection,
                Some(DocumentProviderProjection::CarrierIde { .. })
            ),
            "did_open on a .svelte carrier must build an IDE (carrier) projection \
             (the carrier-general compile gate reaches the Svelte projection)"
        );
    }

    /// The hot read path hands out a SHARED source-map mapper: two
    /// `get_position_mapper` results for the SAME carrier document must point at
    /// ONE `PositionMapper` allocation, not two deep copies of its
    /// `OwnedSourceMap` + precomputed lookup tables.
    ///
    /// Discriminating: with the mapper owned behind a `Box`, `.mapper()`
    /// deep-clones the whole `PositionMapper` pointee on every call, so the two
    /// results point at DISTINCT allocations and the address comparison FAILS.
    /// With the mapper owned behind an `Arc`, `.mapper()` clones the handle and
    /// both results share ONE allocation, so the comparison PASSES.
    #[test]
    fn get_position_mapper_shares_one_allocation() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///x/Comp.svelte".parse().expect("uri");

        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "svelte".to_string(),
            version: 1,
            text: "<script lang=\"ts\">let count = 0;</script>\n<div>{count}</div>".to_string(),
        });

        // Two independent reads of the unified mapper for the same document.
        let first = registry
            .get_position_mapper(&uri)
            .expect("a .svelte carrier must expose a source-map mapper");
        let second = registry
            .get_position_mapper(&uri)
            .expect("a .svelte carrier must expose a source-map mapper");

        // Both reads project the same carrier into its IDE TSX, so both are the
        // SourceMap arm. Compare the address of the underlying `PositionMapper`
        // pointee: a shared handle yields one allocation, a per-call deep clone
        // yields two.
        let (ProviderPositionMapper::SourceMap(a), ProviderPositionMapper::SourceMap(b)) =
            (&first, &second)
        else {
            panic!("a .svelte carrier mapper must be the SourceMap projection");
        };
        assert!(
            std::ptr::eq(a.as_ref(), b.as_ref()),
            "two get_position_mapper reads must share ONE PositionMapper allocation \
             (the read path hands out a cheap handle clone, not a deep copy of the \
             source map + lookup tables)"
        );
    }

    /// A document EDIT REBUILDS the position-mapper `Arc` rather than mutating
    /// the existing pointee: after `did_change` recompiles a carrier whose TSX
    /// changed, `get_position_mapper` hands out an `Arc` that is a FRESH
    /// allocation — NOT `Arc::ptr_eq` to the pre-edit handle.
    ///
    /// This is the companion to the read-path share test: within one document
    /// version the `Arc` is shared (cheap handle clones), but ACROSS an edit the
    /// projection is REPLACED whole (`rebuild_carrier` wraps a fresh
    /// `PositionMapper` in a new `Arc::new`, then overwrites `entry.projection`).
    ///
    /// Discriminating: the pre-edit and post-edit handles are captured and
    /// compared by allocation address. If a future refactor mutated the mapper
    /// in place behind the SAME `Arc` (e.g. `Arc::get_mut` to overwrite the
    /// source map + lookup tables) instead of installing a fresh `Arc`, the two
    /// handles would alias and `Arc::ptr_eq` would be TRUE — failing this
    /// assertion. The edit changes the `<script>` body so the recompiled TSX
    /// genuinely differs (`update.changed`), forcing the rebuild branch rather
    /// than the keep-prior-projection fallback.
    #[test]
    fn did_change_installs_a_fresh_position_mapper_arc() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///x/Comp.svelte".parse().expect("uri");

        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "svelte".to_string(),
            version: 1,
            text: "<script lang=\"ts\">let count = 0;</script>\n<div>{count}</div>".to_string(),
        });

        // Capture the pre-edit mapper handle's allocation.
        let before = registry
            .get_position_mapper(&uri)
            .expect("a .svelte carrier must expose a source-map mapper before the edit");
        let ProviderPositionMapper::SourceMap(before_arc) = &before else {
            panic!("a .svelte carrier mapper must be the SourceMap projection");
        };

        // Edit the document: change the script body so the recompiled IDE TSX
        // genuinely differs (drives `update.changed` → the rebuild branch).
        let _ = registry.did_change(
            &uri,
            2,
            "<script lang=\"ts\">let total = 100; let extra = 1;</script>\n<div>{total}</div>",
        );

        // The post-edit read must hand out a DIFFERENT allocation: the rebuild
        // installed a fresh `Arc`, it did not mutate the old pointee in place.
        let after = registry
            .get_position_mapper(&uri)
            .expect("a .svelte carrier must expose a source-map mapper after the edit");
        let ProviderPositionMapper::SourceMap(after_arc) = &after else {
            panic!("a .svelte carrier mapper must remain the SourceMap projection");
        };

        assert!(
            !std::ptr::eq(before_arc.as_ref(), after_arc.as_ref()),
            "a doc edit must REPLACE the position-mapper Arc with a fresh \
             allocation (rebuild installs a new Arc; it must not mutate the \
             pre-edit pointee in place)"
        );
    }

    /// A `.svelte.ts` rune module is NOT a carrier; did_open must build a
    /// SELF-FILE projection whose mapper offsets the user-source line by the
    /// rune prelude line count. Without the offset wiring, a source position
    /// would map to the same provider line (off by `prelude_line_count`) — the
    /// discriminating assertion.
    #[test]
    fn did_open_rune_module_builds_self_file_projection_with_prelude_offset() {
        use provider_projection::DocumentProviderProjection;
        use verter_span::{LspPosition, TsPosition};

        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));

        let uri: Uri = "file:///x/store.svelte.ts".parse().expect("uri");
        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "typescript".to_string(),
            version: 1,
            text: "export const s = $state(0);\n".to_string(),
        });

        let projection = registry
            .get_projection(&uri)
            .expect("a .svelte.ts rune module must build a provider projection");
        let mapper = match &projection {
            DocumentProviderProjection::SelfFile { mapper } => mapper.clone(),
            DocumentProviderProjection::CarrierIde { .. } => {
                panic!("a .svelte.ts rune module is NOT a carrier — must be a SelfFile projection")
            }
        };
        let prelude = mapper.prelude_line_count();
        assert!(
            prelude > 0,
            "the rune prelude must occupy at least one line (the offset to wire)"
        );

        // Source line 0 maps to provider line `prelude` — NOT line 0.
        let prov = registry
            .get_position_mapper(&uri)
            .expect("unified mapper")
            .carrier_to_tsx(LspPosition::new(0, 13))
            .expect("source maps to provider");
        assert_eq!(
            prov.pos,
            TsPosition::new(prelude, 13),
            "the source line must shift DOWN by the prelude line count (off-by-prelude if unwired)"
        );
        // The provider position maps back to source line 0.
        let back = registry
            .get_position_mapper(&uri)
            .expect("unified mapper")
            .tsx_to_carrier(TsPosition::new(prelude, 13))
            .expect("provider maps back");
        assert_eq!(back.pos, LspPosition::new(0, 13));
        // A provider position inside the prelude region drops (never clamps).
        assert!(
            registry
                .get_position_mapper(&uri)
                .expect("unified mapper")
                .tsx_to_carrier(TsPosition::new(0, 0))
                .is_none(),
            "a provider position in the prelude region must drop, not surface a fake source line"
        );
    }

    /// A plain TS-family script is NOT a carrier either: did_open must build a
    /// SELF-FILE projection whose zero-prelude mapper is the identity (its
    /// provider buffer is the source bytes verbatim). Without the projection,
    /// every provider-backed feature (hover/definition/completion/diagnostics)
    /// fails closed for plain scripts.
    #[test]
    fn did_open_plain_script_builds_self_file_projection_with_identity_mapping() {
        use provider_projection::DocumentProviderProjection;
        use verter_span::{LspPosition, TsPosition};

        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));

        let uri: Uri = "file:///x/plain-control.ts".parse().expect("uri");
        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "typescript".to_string(),
            version: 1,
            text: "export const plainControlNumber = 1;\nplainControlNumber.toFixed(0);\n"
                .to_string(),
        });

        let projection = registry
            .get_projection(&uri)
            .expect("a plain .ts script must build a self-file provider projection");
        let mapper = match &projection {
            DocumentProviderProjection::SelfFile { mapper } => mapper.clone(),
            DocumentProviderProjection::CarrierIde { .. } => {
                panic!("a plain .ts script is NOT a carrier — must be a SelfFile projection")
            }
        };
        assert_eq!(
            mapper.prelude_line_count(),
            0,
            "a plain script's provider buffer is verbatim — no prelude offset"
        );

        // Identity mapping in both directions (zero prelude, no rewrites).
        let prov = registry
            .get_position_mapper(&uri)
            .expect("unified mapper")
            .carrier_to_tsx(LspPosition::new(1, 3))
            .expect("source maps to provider");
        assert_eq!(prov.pos, TsPosition::new(1, 3));
        let back = registry
            .get_position_mapper(&uri)
            .expect("unified mapper")
            .tsx_to_carrier(TsPosition::new(1, 3))
            .expect("provider maps back");
        assert_eq!(back.pos, LspPosition::new(1, 3));
    }

    /// An unknown extension (no registered language row) must NOT build a
    /// provider projection: never serve a non-script document to the
    /// TypeScript provider.
    #[test]
    fn did_open_unknown_extension_builds_no_projection() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));

        let uri: Uri = "file:///x/notes.md".parse().expect("uri");
        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "markdown".to_string(),
            version: 1,
            text: "# notes\n".to_string(),
        });

        assert!(
            registry.get_projection(&uri).is_none(),
            "an unknown-extension document must not get a provider projection"
        );
    }

    /// With no compiled IDE output (a cold caller), `is_jsx_for_canonical`
    /// must fall back to the parse-level script dialect — a JS carrier is
    /// `.jsx` from the start, never a `.tsx` guess that flips later.
    #[test]
    fn is_jsx_for_canonical_falls_back_to_parse_dialect_without_ide_compile() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));

        // A no-lang Svelte component: the parse reports the JS dialect.
        let js_svelte = "<script>\nlet msg = 'hi';\n</script>\n<p>{msg}</p>";
        let _ = host.upsert(verter_session::UpsertRequest {
            canonical_id: Some("/x/JsComp.svelte".to_string()),
            input_id: "/x/JsComp.svelte".to_string(),
            source: Arc::from(js_svelte),
            file_language: verter_session::FileLanguage::svelte(),
            aliases: vec![],
        });
        let ts_vue = "<script setup lang=\"ts\">\nconst msg: string = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>";
        let _ = host.upsert(verter_session::UpsertRequest {
            canonical_id: Some("/x/TsComp.vue".to_string()),
            input_id: "/x/TsComp.vue".to_string(),
            source: Arc::from(ts_vue),
            file_language: verter_session::FileLanguage::vue(),
            aliases: vec![],
        });

        // No IDE compile ran — the parse-level dialect decides.
        let analysis = host.get_analysis("/x/JsComp.svelte");
        assert!(
            analysis.is_some(),
            "analysis must exist for the fallback to consult"
        );
        assert!(
            !analysis.unwrap().is_typescript,
            "a no-lang Svelte script is not TypeScript"
        );
        assert!(
            registry.is_jsx_for_canonical("/x/JsComp.svelte"),
            "a no-lang (JS) Svelte carrier is .jsx without an IDE compile"
        );
        assert!(
            !registry.is_jsx_for_canonical("/x/TsComp.vue"),
            "a lang=ts Vue carrier is .tsx without an IDE compile"
        );
    }

    /// get_ide() fast path should not overwrite an existing position mapper.
    #[test]
    fn position_mapper_not_overwritten_when_present() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/App.vue".parse().unwrap();

        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><div>hello</div></template><script setup lang=\"ts\">\nconst x = 1;\n</script>".to_string(),
        });

        // Grab the original mapper
        let original = registry.get_position_mapper(&uri);
        assert!(original.is_some(), "mapper should exist after did_open");

        // Call get_ide() — should not replace the mapper
        let ide = registry.get_ide(&uri);
        assert!(ide.is_some());

        // Mapper should still be the same instance
        let after = registry.get_position_mapper(&uri);
        assert!(after.is_some(), "mapper should still exist after get_ide");
    }
}
