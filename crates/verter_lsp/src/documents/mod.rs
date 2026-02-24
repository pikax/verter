pub mod line_index;
pub mod position_map;
pub mod sfc_scanner;

use std::sync::Arc;

use dashmap::DashMap;
use tower_lsp_server::lsp_types::*;
use verter_host::{
    CompileProfile, FileKind, HostUpdateResult, TsxResponse, UpsertRequest, VerterHost,
};

use line_index::LineIndex;
use position_map::PositionMapper;

/// Manages open documents and their relationship to verter_host.
pub struct DocumentRegistry {
    host: VerterHost,
    /// Map from document URI to document state.
    documents: DashMap<String, DocumentState>,
    /// Default compile profile for TSX generation (LSP mode).
    tsx_profile: CompileProfile,
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
}

impl DocumentRegistry {
    pub fn new(host: VerterHost) -> Self {
        Self {
            host,
            documents: DashMap::new(),
            tsx_profile: CompileProfile {
                source_map: true,
                enable_types: true,
                ..CompileProfile::default()
            },
        }
    }

    /// Handle a document being opened in the editor.
    pub fn did_open(&self, params: &TextDocumentItem) -> HostUpdateResult {
        let uri_str = params.uri.as_str().to_string();
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
        // get_virtual_file() compiles lazily and caches TSX + source map.
        if file_kind == FileKind::VueSfc {
            let _ = self.host.get_virtual_file(verter_host::VirtualQuery {
                raw_id: None,
                canonical_id: Some(canonical_id.clone()),
                node_kind: Some(verter_host::VirtualNodeKind::Main),
                compile_profile: self.tsx_profile.clone(),
            });
        }

        // Build position mapper from TSX source map
        let position_mapper = match &result {
            Ok(update) if update.changed => self
                .host
                .get_tsx(&canonical_id, &self.tsx_profile)
                .and_then(|tsx| PositionMapper::from_json(&tsx.source_map?).ok()),
            _ => None,
        };

        let state = DocumentState {
            canonical_id,
            version: params.version,
            line_index: LineIndex::new(&source),
            source,
            position_mapper,
            language_id: params.language_id.clone(),
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

        let result = self.host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: source.clone(),
            file_kind,
            aliases: vec![],
        });

        // Trigger re-compilation for Vue SFCs
        if file_kind == FileKind::VueSfc {
            let _ = self.host.get_virtual_file(verter_host::VirtualQuery {
                raw_id: None,
                canonical_id: Some(canonical_id.clone()),
                node_kind: Some(verter_host::VirtualNodeKind::Main),
                compile_profile: self.tsx_profile.clone(),
            });
        }

        // Rebuild position mapper
        let position_mapper = match &result {
            Ok(update) if update.changed => self
                .host
                .get_tsx(&canonical_id, &self.tsx_profile)
                .and_then(|tsx| PositionMapper::from_json(&tsx.source_map?).ok()),
            _ => self
                .documents
                .get(&uri_str)
                .and_then(|d| d.position_mapper.clone()),
        };

        if let Some(mut entry) = self.documents.get_mut(&uri_str) {
            entry.version = version;
            entry.line_index = LineIndex::new(&source);
            entry.source = source;
            entry.position_mapper = position_mapper;
        }

        result.unwrap_or_else(|e| {
            tracing::error!("upsert failed for {}: {:?}", uri_str, e);
            HostUpdateResult::no_change(canonical_id)
        })
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

    /// Get the TSX output for a document.
    pub fn get_tsx(&self, uri: &Uri) -> Option<TsxResponse> {
        let canonical_id = self.get_canonical_id(uri)?;
        self.host.get_tsx(&canonical_id, &self.tsx_profile)
    }

    /// Get the analysis snapshot for a document.
    pub fn get_analysis(&self, uri: &Uri) -> Option<verter_host::FileAnalysisSnapshot> {
        let canonical_id = self.get_canonical_id(uri)?;
        self.host.get_analysis(&canonical_id)
    }

    /// Get the diagnostics for a document.
    pub fn get_diagnostics(&self, uri: &Uri) -> Option<verter_host::DiagnosticsSnapshot> {
        let canonical_id = self.get_canonical_id(uri)?;
        self.host.get_diagnostics(&canonical_id, &self.tsx_profile)
    }

    /// Get the underlying verter_host reference.
    pub fn host(&self) -> &VerterHost {
        &self.host
    }
}

/// Convert an LSP document URI to a canonical file path ID.
///
/// Extracts the path component from `file://` URIs.
/// On Windows, strips the leading `/` from paths like `/C:/Users/...`.
fn uri_to_canonical_id(uri: &Uri) -> String {
    let s = uri.as_str();

    // Handle file:// URIs by extracting the path
    if let Some(rest) = s.strip_prefix("file:///") {
        let decoded = percent_decode(rest);
        let path = decoded.replace('\\', "/");
        // On Windows, paths look like "C:/Users/..." (drive letter after file:///)
        // On Unix, paths look like "home/user/..." (need leading /)
        if path.chars().nth(1) == Some(':') {
            // Windows drive letter (e.g., "C:/Users/...")
            return path;
        }
        // Unix: restore leading /
        return format!("/{path}");
    }
    if let Some(rest) = s.strip_prefix("file://") {
        let path = percent_decode(rest);
        return path.replace('\\', "/");
    }

    // Fallback: use the full URI string
    s.to_string()
}

/// Minimal percent-decoding for URI paths (handles %20 for spaces, etc.).
fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                result.push((hi << 4 | lo) as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
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
}
