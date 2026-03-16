pub mod line_index;
pub mod position_map;
pub mod sfc_scanner;

use std::sync::Arc;

use parking_lot::RwLock;

use dashmap::DashMap;
use tower_lsp_server::ls_types::*;
use verter_host::{
    CompileProfile, FileKind, HostUpdateResult, IdeResponse, StyleOverrideEntry,
    StyleOverrideRequest, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

use crate::server::{CodeBlock, VirtualFileEntry, VirtualFilesResponse};
use crate::uri::{file_uri_to_path, percent_decode};

use line_index::LineIndex;
use position_map::PositionMapper;

/// Manages open documents and their relationship to verter_host.
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
}

/// Tracked state for an open document.
pub struct DocumentState {
    /// The canonical ID used with verter_host.
    pub canonical_id: String,
    /// Current document version (from LSP client).
    pub version: i32,
    /// Current source text.
    pub source: Arc<str>,
    /// Precomputed line index for byte-offset ↔ LSP Position conversion.
    pub line_index: LineIndex,
    /// Cached position mapper (rebuilt on each document change).
    pub position_mapper: Option<PositionMapper>,
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
                target: verter_host::CompileTarget::IDE | verter_host::CompileTarget::TEMPLATE_DATA,
                ..CompileProfile::default()
            })),
            encoding: RwLock::new(PositionEncodingKind::UTF16),
        }
    }

    /// Set the negotiated position encoding. Called once during `initialize()`,
    /// before any documents are opened.
    pub fn set_encoding(&self, encoding: PositionEncodingKind) {
        *self.encoding.write() = encoding;
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
                position_mapper: None,
                language_id: params.language_id.clone(),
                virtual_source_uri: Some(source_uri),
            };
            self.documents.insert(uri_str.clone(), state);
            return HostUpdateResult::no_change(uri_str);
        }

        let canonical_id = uri_to_canonical_id(&params.uri);
        let source: Arc<str> = Arc::from(params.text.as_str());

        let file_kind = if params.language_id == "vue" {
            FileKind::VueSfc
        } else {
            FileKind::NonSfc
        };

        let result = self.host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: source.clone(),
            file_kind,
            aliases: vec![],
        });

        // Trigger compilation to populate TSX cache (upsert only parses).
        // ensure_compiled() compiles lazily and caches TSX + source map.
        if file_kind == FileKind::VueSfc {
            let _ = self
                .host
                .ensure_compiled(&canonical_id, &self.tsx_profile.read());
        }

        // Build position mapper from TSX source map.
        // Always build on did_open — even if the host reports `changed: false`
        // (e.g., because scan_workspace already loaded the same content), we still
        // need the mapper for hover/definition/diagnostics.
        let position_mapper = if file_kind == FileKind::VueSfc {
            self.host
                .get_ide(&canonical_id, &self.tsx_profile.read())
                .and_then(|tsx| PositionMapper::from_json(&tsx.source_map?).ok())
        } else {
            None
        };

        let state = DocumentState {
            canonical_id,
            version: params.version,
            line_index: LineIndex::new(&source, self.encoding()),
            source,
            position_mapper,
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

        let file_kind = self
            .documents
            .get(&uri_str)
            .map(|d| {
                if d.language_id == "vue" {
                    FileKind::VueSfc
                } else {
                    FileKind::NonSfc
                }
            })
            .unwrap_or(FileKind::NonSfc);

        let upsert_start = std::time::Instant::now();
        let result = self.host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: source.clone(),
            file_kind,
            aliases: vec![],
        });
        tracing::info!(
            "DocumentRegistry::did_change HOST_UPSERT_DONE elapsed={:?} thread={:?}",
            upsert_start.elapsed(),
            std::thread::current().id()
        );

        // Trigger re-compilation for Vue SFCs
        if file_kind == FileKind::VueSfc {
            let compile_start = std::time::Instant::now();
            let _ = self
                .host
                .ensure_compiled(&canonical_id, &self.tsx_profile.read());
            tracing::info!(
                "DocumentRegistry::did_change ENSURE_COMPILED_DONE elapsed={:?} thread={:?}",
                compile_start.elapsed(),
                std::thread::current().id()
            );
        }

        // Rebuild position mapper.
        // When compilation fails (e.g., temporarily invalid SFC during typing),
        // preserve the old mapper so position-dependent features keep working.
        let position_mapper = match &result {
            Ok(update) if update.changed => {
                let new_mapper = self
                    .host
                    .get_ide(&canonical_id, &self.tsx_profile.read())
                    .and_then(|tsx| PositionMapper::from_json(&tsx.source_map?).ok());
                // Keep old mapper if new compilation failed (Bug 3 fix)
                new_mapper.or_else(|| {
                    self.documents
                        .get(&uri_str)
                        .and_then(|d| d.position_mapper.clone())
                })
            }
            _ => self
                .documents
                .get(&uri_str)
                .and_then(|d| d.position_mapper.clone()),
        };

        if let Some(mut entry) = self.documents.get_mut(&uri_str) {
            entry.version = version;
            entry.line_index = LineIndex::new(&source, self.encoding());
            entry.source = source;
            entry.position_mapper = position_mapper;
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
        self.documents.remove(uri.as_str());
    }

    /// Get the document state for a URI.
    pub fn get(&self, uri: &Uri) -> Option<dashmap::mapref::one::Ref<'_, String, DocumentState>> {
        self.documents.get(uri.as_str())
    }

    /// Get the position mapper for a document.
    pub fn get_position_mapper(&self, uri: &Uri) -> Option<PositionMapper> {
        self.documents.get(uri.as_str())?.position_mapper.clone()
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
                if entry.position_mapper.is_none() {
                    drop(entry);
                    if let Some(mut entry) = self.documents.get_mut(uri.as_str()) {
                        if entry.position_mapper.is_none() {
                            if let Some(mapper) = resp
                                .source_map
                                .as_ref()
                                .and_then(|sm| PositionMapper::from_json(sm).ok())
                            {
                                entry.position_mapper = Some(mapper);
                            }
                        }
                    }
                }
            }
            return Some(resp);
        }

        // Slow path: compile_slots were cleared (e.g., dependency invalidation).
        // Lazily recompile to restore IDE output.
        let is_vue = self
            .documents
            .get(uri.as_str())
            .map(|d| d.language_id == "vue")
            .unwrap_or(false);
        if !is_vue {
            return None;
        }

        self.host.ensure_compiled(&canonical_id, &profile).ok()?;
        let resp = self.host.get_ide(&canonical_id, &profile)?;

        // Rebuild position mapper since TSX output was regenerated
        if let Some(mut entry) = self.documents.get_mut(uri.as_str()) {
            if let Some(mapper) = resp
                .source_map
                .as_ref()
                .and_then(|sm| PositionMapper::from_json(sm).ok())
            {
                entry.position_mapper = Some(mapper);
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
        let is_vue = self
            .documents
            .get(uri.as_str())
            .map(|d| d.language_id == "vue")
            .unwrap_or(false);
        if !is_vue {
            return None;
        }
        let canonical_id = self.get_canonical_id(uri)?;
        let profile = self.tsx_profile.read().clone();
        self.host.ensure_compiled(&canonical_id, &profile).ok()?;
        let resp = self.host.get_ide(&canonical_id, &profile)?;
        // Always rebuild mapper from fresh source map
        if let Some(mut entry) = self.documents.get_mut(uri.as_str()) {
            entry.position_mapper = resp
                .source_map
                .as_ref()
                .and_then(|sm| PositionMapper::from_json(sm).ok());
        }
        Some(resp)
    }

    /// Check if a document's IDE output is JavaScript (JSX) rather than TypeScript (TSX).
    pub fn is_jsx(&self, uri: &Uri) -> bool {
        self.get_ide(uri).map(|r| r.is_jsx).unwrap_or(false)
    }

    /// Get the analysis snapshot for a document.
    pub fn get_analysis(&self, uri: &Uri) -> Option<verter_host::FileAnalysisSnapshot> {
        let canonical_id = self.get_canonical_id(uri)?;
        self.host.get_analysis(&canonical_id)
    }

    /// Get the diagnostics for a document.
    pub fn get_diagnostics(&self, uri: &Uri) -> Option<verter_host::DiagnosticsSnapshot> {
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

    /// Get the underlying verter_host reference.
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

/// Convert a `file://` URI string to a canonical filesystem path.
///
/// Handles percent-encoded characters (e.g., `%3A` → `:` on Windows),
/// normalises separators to `/`, and restores the leading `/` on Unix.
pub fn uri_to_canonical_id_from_str(s: &str) -> String {
    file_uri_to_path(s)
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
        let uri: Uri = "file:///C:/Users/dev/project/App.vue".parse().unwrap();
        let id = uri_to_canonical_id(&uri);
        assert_eq!(id, "C:/Users/dev/project/App.vue");
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
        let id = uri_to_canonical_id_from_str("file:///d%3A/dev/personal/verter/examples");
        assert_eq!(id, "d:/dev/personal/verter/examples");
    }

    /// @ai-generated — Windows path with lowercase drive and normal colon
    #[test]
    fn test_uri_to_canonical_id_windows_lowercase_drive() {
        let id = uri_to_canonical_id_from_str("file:///d:/dev/personal/verter");
        assert_eq!(id, "d:/dev/personal/verter");
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
        let host = Arc::new(verter_host::VerterHost::new(
            verter_host::HostConfig::default(),
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
        let host = Arc::new(verter_host::VerterHost::new(
            verter_host::HostConfig::default(),
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

        // Simulate the startup race: clear the mapper to None
        if let Some(mut entry) = registry.documents.get_mut(uri.as_str()) {
            entry.position_mapper = None;
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

    /// get_ide() fast path should not overwrite an existing position mapper.
    #[test]
    fn position_mapper_not_overwritten_when_present() {
        let host = Arc::new(verter_host::VerterHost::new(
            verter_host::HostConfig::default(),
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
