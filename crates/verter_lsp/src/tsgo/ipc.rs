//! TSGO `TypeProvider` implementation via LSP JSON-RPC over stdio.
//!
//! Spawns `tsgo --lsp --stdio` as a child process and communicates using
//! the Language Server Protocol over stdin/stdout with JSON-RPC framing.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{oneshot, Mutex};

use crate::tsgo::protocol::*;
use crate::tsgo::traits::{ProviderFuture, TypeProvider};

/// LSP JSON-RPC transport over a child process's stdio.
struct LspTransport {
    stdin: Arc<Mutex<ChildStdin>>,
    /// Pending request senders, keyed by request ID. Shared with the read loop.
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>,
    next_id: AtomicI64,
}

impl LspTransport {
    /// Send an LSP request and wait for the response.
    async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, TypeProviderError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let body = serde_json::to_string(&msg)
            .map_err(|e| TypeProviderError::new(format!("serialize error: {e}")))?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| TypeProviderError::new(format!("write error: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| TypeProviderError::new(format!("flush error: {e}")))?;
        drop(stdin);

        let result = tokio::time::timeout(std::time::Duration::from_secs(10), rx)
            .await
            .map_err(|_| TypeProviderError::new(format!("request '{method}' timed out after 10s")))?
            .map_err(|_| TypeProviderError::new("response channel closed"))?;

        // Check for JSON-RPC error
        if let Some(err) = result.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(TypeProviderError::new(msg));
        }

        Ok(result
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    /// Send an LSP notification (no response expected).
    async fn notify(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), TypeProviderError> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let body = serde_json::to_string(&msg)
            .map_err(|e| TypeProviderError::new(format!("serialize error: {e}")))?;

        let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| TypeProviderError::new(format!("write error: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| TypeProviderError::new(format!("flush error: {e}")))?;

        Ok(())
    }
}

/// Read loop that processes JSON-RPC messages from the child's stdout
/// and dispatches responses to pending request channels.
/// Also handles `textDocument/publishDiagnostics` notifications and
/// auto-responds to server→client requests (e.g., `client/registerCapability`).
async fn read_loop(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>,
    diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>>,
    stdin_for_replies: Arc<Mutex<ChildStdin>>,
) {
    let mut reader = BufReader::new(stdout);
    let mut header_buf = String::new();

    loop {
        // Read headers until we find Content-Length
        let mut content_length: Option<usize> = None;
        header_buf.clear();

        loop {
            header_buf.clear();
            match reader.read_line(&mut header_buf).await {
                Ok(0) => return, // EOF
                Ok(_) => {
                    let line = header_buf.trim();
                    if line.is_empty() {
                        break; // End of headers
                    }
                    if let Some(len_str) = line.strip_prefix("Content-Length:") {
                        if let Ok(len) = len_str.trim().parse::<usize>() {
                            content_length = Some(len);
                        }
                    }
                }
                Err(_) => return, // I/O error
            }
        }

        let content_length = match content_length {
            Some(len) => len,
            None => continue, // Malformed message
        };

        // Read the body
        let mut body = vec![0u8; content_length];
        if tokio::io::AsyncReadExt::read_exact(&mut reader, &mut body)
            .await
            .is_err()
        {
            return;
        }

        let msg: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Determine if this is a server→client request (has "method" + "id"),
        // a notification (has "method" but no "id"), or a response (has "id" but no "method").
        let has_method = msg.get("method").is_some();
        let msg_id = msg.get("id").cloned();

        if let (true, Some(id)) = (has_method, &msg_id) {
            // Server→client request: auto-respond with empty result to unblock TSGO.
            // Common examples: client/registerCapability, workspace/configuration
            let reply = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": null,
            });
            let body = serde_json::to_string(&reply).unwrap_or_default();
            let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
            let mut stdin = stdin_for_replies.lock().await;
            let _ = stdin.write_all(frame.as_bytes()).await;
            let _ = stdin.flush().await;
            continue;
        }

        if has_method {
            // Notification (no id): handle known types
            if let Some(method) = msg.get("method").and_then(|v| v.as_str()) {
                if method == "textDocument/publishDiagnostics" {
                    if let Some(params) = msg.get("params") {
                        let uri = params
                            .get("uri")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let diags = params
                            .get("diagnostics")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(parse_lsp_diagnostic)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        diagnostics_cache.lock().await.insert(uri, diags);
                    }
                }
            }
            continue;
        }

        // Response to our request: route by id
        if let Some(id_val) = msg_id {
            if let Some(id) = id_val.as_i64() {
                if let Some(tx) = pending.lock().await.remove(&id) {
                    let _ = tx.send(msg);
                }
            }
        }
    }
}

/// Parse a single LSP Diagnostic JSON value into a `TypeDiagnostic`.
fn parse_lsp_diagnostic(d: &serde_json::Value) -> Option<TypeDiagnostic> {
    let range = d.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    let message = d.get("message")?.as_str()?.to_string();
    let severity = match d.get("severity").and_then(|v| v.as_u64()) {
        Some(1) => TypeDiagnosticSeverity::Error,
        Some(2) => TypeDiagnosticSeverity::Warning,
        Some(3) => TypeDiagnosticSeverity::Info,
        Some(4) => TypeDiagnosticSeverity::Hint,
        _ => TypeDiagnosticSeverity::Error,
    };
    let code = d.get("code").and_then(|v| {
        v.as_u64()
            .map(|n| n.to_string())
            .or_else(|| v.as_str().map(String::from))
    });

    // Store line/character as packed (line << 16 | character) for later resolution.
    // The actual byte offset conversion happens when the caller has the content.
    let start_line = start.get("line")?.as_u64()? as u32;
    let start_char = start.get("character")?.as_u64()? as u32;
    let end_line = end.get("line")?.as_u64()? as u32;
    let end_char = end.get("character")?.as_u64()? as u32;

    Some(TypeDiagnostic {
        message,
        severity,
        start: pack_position(start_line, start_char),
        end: pack_position(end_line, end_char),
        code,
    })
}

/// Pack an LSP line/character into a u32 for storage.
fn pack_position(line: u32, character: u32) -> u32 {
    // This encoding works for files up to 65535 lines with columns up to 65535.
    (line << 16) | (character & 0xFFFF)
}

/// Convert an LSP `(line, character)` position to a byte offset in content.
fn position_to_offset(content: &str, line: u32, character: u32) -> u32 {
    let mut current_line = 0u32;
    let mut byte_offset = 0usize;
    let bytes = content.as_bytes();

    // Find the start of the target line
    while current_line < line && byte_offset < bytes.len() {
        if bytes[byte_offset] == b'\n' {
            current_line += 1;
        }
        byte_offset += 1;
    }

    // Add character offset within the line
    (byte_offset + character as usize).min(bytes.len()) as u32
}

/// Parse an LSP Location JSON value into a `TypeLocation`, using content for offset resolution.
fn parse_lsp_location(loc: &serde_json::Value, content: Option<&str>) -> Option<TypeLocation> {
    let uri = loc.get("uri")?.as_str()?;
    let range = loc.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    let start_line = start.get("line")?.as_u64()? as u32;
    let start_char = start.get("character")?.as_u64()? as u32;
    let end_line = end.get("line")?.as_u64()? as u32;
    let end_char = end.get("character")?.as_u64()? as u32;

    let (start_offset, end_offset) = if let Some(c) = content {
        (
            position_to_offset(c, start_line, start_char),
            position_to_offset(c, end_line, end_char),
        )
    } else {
        // Fallback: store packed positions
        (
            pack_position(start_line, start_char),
            pack_position(end_line, end_char),
        )
    };

    Some(TypeLocation {
        path: uri.to_string(),
        start: start_offset,
        end: end_offset,
    })
}

/// Parse an LSP CompletionItem JSON value into a `Completion`.
fn parse_completion_item(item: &serde_json::Value, content: Option<&str>) -> Option<Completion> {
    let label = item.get("label")?.as_str()?.to_string();
    let kind = item.get("kind").and_then(|v| v.as_u64()).map(|k| match k {
        1 => CompletionKind::Text,
        2 => CompletionKind::Method,
        3 => CompletionKind::Function,
        5 => CompletionKind::Field,
        6 => CompletionKind::Variable,
        7 => CompletionKind::Class,
        8 => CompletionKind::Interface,
        9 => CompletionKind::Module,
        13 => CompletionKind::Enum,
        14 => CompletionKind::Keyword,
        15 => CompletionKind::Snippet,
        16 => CompletionKind::Property,
        20 => CompletionKind::EnumMember,
        21 => CompletionKind::Constant,
        25 => CompletionKind::TypeParameter,
        _ => CompletionKind::Text,
    });
    let detail = item
        .get("detail")
        .and_then(|v| v.as_str())
        .map(String::from);
    let documentation = item.get("documentation").and_then(|v| {
        v.as_str()
            .map(String::from)
            .or_else(|| v.get("value").and_then(|v2| v2.as_str()).map(String::from))
    });
    let insert_text = item
        .get("insertText")
        .and_then(|v| v.as_str())
        .map(String::from);
    let sort_text = item
        .get("sortText")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Parse textEdit range for edit_range_start/end
    let (edit_range_start, edit_range_end) = item
        .get("textEdit")
        .and_then(|te| {
            let range = te.get("range")?;
            let start = range.get("start")?;
            let end = range.get("end")?;
            let sl = start.get("line")?.as_u64()? as u32;
            let sc = start.get("character")?.as_u64()? as u32;
            let el = end.get("line")?.as_u64()? as u32;
            let ec = end.get("character")?.as_u64()? as u32;
            if let Some(c) = content {
                Some((
                    Some(position_to_offset(c, sl, sc)),
                    Some(position_to_offset(c, el, ec)),
                ))
            } else {
                Some((Some(pack_position(sl, sc)), Some(pack_position(el, ec))))
            }
        })
        .unwrap_or((None, None));

    Some(Completion {
        label,
        kind,
        detail,
        documentation,
        edit_range_start,
        edit_range_end,
        insert_text,
        sort_text,
    })
}

/// Convert a byte offset into an LSP `(line, character)` position.
///
/// The LSP protocol uses UTF-16 code units for `character`, but for ASCII/BMP
/// content the byte offset within the line is equivalent. This is sufficient
/// for generated TSX which is predominantly ASCII.
fn offset_to_position(content: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let bytes = content.as_bytes();
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if i == offset {
            return (line, (offset - line_start) as u32);
        }
        if b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    // offset == content.len() or beyond
    (line, (offset.min(bytes.len()) - line_start) as u32)
}

/// A `TypeProvider` backed by a real TSGO process (`tsgo --lsp --stdio`).
///
/// Spawns the process, initializes the LSP connection, and translates
/// `TypeProvider` method calls into LSP requests.
pub struct TsgoTypeProvider {
    transport: Arc<LspTransport>,
    /// Keep the child alive for the provider's lifetime.
    _child: Child,
    /// Document version counter per path.
    versions: Arc<Mutex<HashMap<String, i32>>>,
    /// Cached file contents for byte-offset → LSP position conversion.
    contents: Arc<Mutex<HashMap<String, String>>>,
    /// Cached diagnostics received from textDocument/publishDiagnostics notifications.
    diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>>,
}

impl TsgoTypeProvider {
    /// Spawn a TSGO process and initialize the LSP connection.
    ///
    /// `tsgo_bin` is the path to the tsgo binary (or just "tsgo" to find it on PATH).
    /// `root_uri` is the workspace root URI (e.g., `file:///tmp/my-project`).
    pub async fn spawn(tsgo_bin: &str, root_uri: &str) -> Result<Self, TypeProviderError> {
        let mut child = tokio::process::Command::new(tsgo_bin)
            .arg("--lsp")
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| TypeProviderError::new(format!("failed to spawn tsgo: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TypeProviderError::new("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TypeProviderError::new("no stdout"))?;

        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let stdin = Arc::new(Mutex::new(stdin));

        let transport = Arc::new(LspTransport {
            stdin: Arc::clone(&stdin),
            pending: Arc::clone(&pending),
            next_id: AtomicI64::new(1),
        });

        let diagnostics_cache: Arc<Mutex<HashMap<String, Vec<TypeDiagnostic>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Start the read loop in a background task (needs stdin for replying to server requests)
        tokio::spawn(read_loop(
            stdout,
            pending,
            Arc::clone(&diagnostics_cache),
            Arc::clone(&stdin),
        ));

        // Send initialize request
        let init_result = transport
            .request(
                "initialize",
                serde_json::json!({
                    "processId": std::process::id(),
                    "capabilities": {},
                    "rootUri": root_uri,
                    "workspaceFolders": [{
                        "uri": root_uri,
                        "name": "workspace"
                    }]
                }),
            )
            .await?;

        tracing::debug!("TSGO initialized: {:?}", init_result);

        // Send initialized notification
        transport
            .notify("initialized", serde_json::json!({}))
            .await?;

        Ok(Self {
            transport,
            _child: child,
            versions: Arc::new(Mutex::new(HashMap::new())),
            contents: Arc::new(Mutex::new(HashMap::new())),
            diagnostics_cache,
        })
    }

    /// Convert a file path to a `file://` URI.
    fn path_to_uri(path: &str) -> String {
        let normalized = path.replace('\\', "/");
        if normalized.starts_with('/') {
            format!("file://{normalized}")
        } else {
            format!("file:///{normalized}")
        }
    }
}

impl TypeProvider for TsgoTypeProvider {
    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let uri = Self::path_to_uri(path);
        let lang_id = if path.ends_with(".tsx") {
            "typescriptreact"
        } else {
            "typescript"
        };
        let content = content.to_string();
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            contents_cache
                .lock()
                .await
                .insert(path_owned, content.clone());
            transport
                .notify(
                    "textDocument/didOpen",
                    serde_json::json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": lang_id,
                            "version": 1,
                            "text": content,
                        }
                    }),
                )
                .await
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let uri = Self::path_to_uri(path);
        let content = content.to_string();
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let versions = Arc::clone(&self.versions);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            contents_cache
                .lock()
                .await
                .insert(path_owned.clone(), content.clone());
            let version = {
                let mut vers = versions.lock().await;
                let v = vers.entry(path_owned).or_insert(0);
                *v += 1;
                *v
            };
            transport
                .notify(
                    "textDocument/didChange",
                    serde_json::json!({
                        "textDocument": {
                            "uri": uri,
                            "version": version,
                        },
                        "contentChanges": [{
                            "text": content,
                        }]
                    }),
                )
                .await
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            contents_cache.lock().await.remove(&path_owned);
            transport
                .notify(
                    "textDocument/didClose",
                    serde_json::json!({
                        "textDocument": { "uri": uri }
                    }),
                )
                .await
        })
    }

    fn get_completions(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<Completion>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => (0, offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/completion",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                    }),
                )
                .await?;

            // Parse: result can be CompletionList { items: [] } or CompletionItem[]
            let items = if let Some(arr) = result.as_array() {
                arr.as_slice()
            } else if let Some(arr) = result.get("items").and_then(|v| v.as_array()) {
                arr.as_slice()
            } else {
                return Ok(vec![]);
            };

            Ok(items
                .iter()
                .filter_map(|item| parse_completion_item(item, content_snapshot.as_deref()))
                .collect())
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => offset_to_position(c, offset),
                    None => (0, offset),
                }
            };
            let result = transport
                .request(
                    "textDocument/hover",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                    }),
                )
                .await?;

            if result.is_null() {
                return Ok(None);
            }

            // Parse hover result: { contents: { kind, value } | string }
            let contents = if let Some(c) = result.get("contents") {
                if let Some(value) = c.get("value").and_then(|v| v.as_str()) {
                    value.to_string()
                } else if let Some(s) = c.as_str() {
                    s.to_string()
                } else {
                    format!("{c}")
                }
            } else {
                return Ok(None);
            };

            Ok(Some(HoverInfo {
                contents,
                range_start: None,
                range_end: None,
            }))
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let uri = Self::path_to_uri(path);
        let diagnostics_cache = Arc::clone(&self.diagnostics_cache);
        Box::pin(async move {
            let cache = diagnostics_cache.lock().await;
            Ok(cache.get(&uri).cloned().unwrap_or_default())
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => (0, offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/definition",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                    }),
                )
                .await?;

            let locations = if result.is_array() {
                result.as_array().cloned().unwrap_or_default()
            } else if result.is_object() {
                vec![result]
            } else {
                return Ok(vec![]);
            };

            Ok(locations
                .iter()
                .filter_map(|loc| parse_lsp_location(loc, content_snapshot.as_deref()))
                .collect())
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => (0, offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/references",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                        "context": { "includeDeclaration": true }
                    }),
                )
                .await?;

            let locations = result.as_array().cloned().unwrap_or_default();
            Ok(locations
                .iter()
                .filter_map(|loc| parse_lsp_location(loc, content_snapshot.as_deref()))
                .collect())
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => (0, offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/rename",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                        "newName": "__verter_rename_probe__",
                    }),
                )
                .await?;

            if result.is_null() {
                return Ok(vec![]);
            }

            let mut locations = Vec::new();
            parse_workspace_edit_locations(&result, content_snapshot.as_deref(), &mut locations);
            Ok(locations)
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => offset_to_position(c, offset),
                    None => (0, offset),
                }
            };
            let result = transport
                .request(
                    "textDocument/signatureHelp",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                    }),
                )
                .await?;

            if result.is_null() {
                return Ok(None);
            }

            Ok(Some(parse_signature_help(&result)))
        })
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (start_line, start_char, end_line, end_char, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (sl, sc) = offset_to_position(c, start_offset);
                        let (el, ec) = offset_to_position(c, end_offset);
                        (sl, sc, el, ec, Some(c.clone()))
                    }
                    None => (0, start_offset, 0, end_offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/codeAction",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "range": {
                            "start": { "line": start_line, "character": start_char },
                            "end": { "line": end_line, "character": end_char },
                        },
                        "context": { "diagnostics": [] },
                    }),
                )
                .await?;

            let items = result.as_array().cloned().unwrap_or_default();
            Ok(items
                .iter()
                .filter_map(|item| parse_code_action(item, content_snapshot.as_deref()))
                .collect())
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let content_snapshot = {
                let cache = contents_cache.lock().await;
                cache.get(&path_owned).cloned()
            };
            let result = transport
                .request(
                    "textDocument/semanticTokens/full",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                    }),
                )
                .await?;

            let data = result
                .get("data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            Ok(decode_semantic_tokens(&data, content_snapshot.as_deref()))
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let uri = Self::path_to_uri(path);
        let path_owned = path.to_string();
        let transport = Arc::clone(&self.transport);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, character, content_snapshot) = {
                let cache = contents_cache.lock().await;
                match cache.get(&path_owned) {
                    Some(c) => {
                        let (l, ch) = offset_to_position(c, offset);
                        (l, ch, Some(c.clone()))
                    }
                    None => (0, offset, None),
                }
            };
            let result = transport
                .request(
                    "textDocument/documentHighlight",
                    serde_json::json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                    }),
                )
                .await?;

            let items = result.as_array().cloned().unwrap_or_default();
            Ok(items
                .iter()
                .filter_map(|item| parse_document_highlight(item, content_snapshot.as_deref()))
                .collect())
        })
    }
}

// ── Parsing helpers ─────────────────────────────────────────────────

/// Extract rename locations from a WorkspaceEdit JSON response.
fn parse_workspace_edit_locations(
    result: &serde_json::Value,
    content: Option<&str>,
    locations: &mut Vec<RenameLocation>,
) {
    // Handle `changes: { [uri]: TextEdit[] }` format
    if let Some(changes) = result.get("changes").and_then(|v| v.as_object()) {
        for (change_uri, edits) in changes {
            if let Some(arr) = edits.as_array() {
                for edit in arr {
                    if let Some(loc) = parse_rename_edit(change_uri, edit, content) {
                        locations.push(loc);
                    }
                }
            }
        }
    }
    // Handle `documentChanges: TextDocumentEdit[]` format
    if let Some(doc_changes) = result.get("documentChanges").and_then(|v| v.as_array()) {
        for dc in doc_changes {
            let dc_uri = dc
                .get("textDocument")
                .and_then(|td| td.get("uri"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if let Some(edits) = dc.get("edits").and_then(|v| v.as_array()) {
                for edit in edits {
                    if let Some(loc) = parse_rename_edit(dc_uri, edit, content) {
                        locations.push(loc);
                    }
                }
            }
        }
    }
}

fn parse_rename_edit(
    uri: &str,
    edit: &serde_json::Value,
    content: Option<&str>,
) -> Option<RenameLocation> {
    let range = edit.get("range")?;
    let (start, end) = parse_range_to_offsets(range, content)?;
    Some(RenameLocation {
        path: uri.to_string(),
        start,
        end,
    })
}

/// Parse a SignatureHelp from a JSON response.
fn parse_signature_help(result: &serde_json::Value) -> SignatureHelp {
    let signatures = result
        .get("signatures")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|sig| {
                    let label = sig.get("label")?.as_str()?.to_string();
                    let documentation = sig.get("documentation").and_then(extract_markup_string);
                    let parameters = sig
                        .get("parameters")
                        .and_then(|v| v.as_array())
                        .map(|params| {
                            params
                                .iter()
                                .filter_map(|p| {
                                    let plabel = p.get("label")?.as_str()?.to_string();
                                    let pdoc =
                                        p.get("documentation").and_then(extract_markup_string);
                                    Some(ParameterInfo {
                                        label: plabel,
                                        documentation: pdoc,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(SignatureInfo {
                        label,
                        documentation,
                        parameters,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    SignatureHelp {
        signatures,
        active_signature: result
            .get("activeSignature")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        active_parameter: result
            .get("activeParameter")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
    }
}

/// Parse a CodeAction from a JSON response.
fn parse_code_action(item: &serde_json::Value, content: Option<&str>) -> Option<TypeCodeAction> {
    let title = item.get("title")?.as_str()?.to_string();
    let kind = item.get("kind").and_then(|v| v.as_str()).map(String::from);

    let mut edits = Vec::new();
    if let Some(edit) = item.get("edit") {
        if let Some(changes) = edit.get("changes").and_then(|v| v.as_object()) {
            for (change_uri, text_edits) in changes {
                if let Some(arr) = text_edits.as_array() {
                    for te in arr {
                        if let Some(ce) = parse_text_edit_to_code_edit(change_uri, te, content) {
                            edits.push(ce);
                        }
                    }
                }
            }
        }
        if let Some(doc_changes) = edit.get("documentChanges").and_then(|v| v.as_array()) {
            for dc in doc_changes {
                let dc_uri = dc
                    .get("textDocument")
                    .and_then(|td| td.get("uri"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if let Some(arr) = dc.get("edits").and_then(|v| v.as_array()) {
                    for te in arr {
                        if let Some(ce) = parse_text_edit_to_code_edit(dc_uri, te, content) {
                            edits.push(ce);
                        }
                    }
                }
            }
        }
    }

    Some(TypeCodeAction { title, kind, edits })
}

fn parse_text_edit_to_code_edit(
    uri: &str,
    te: &serde_json::Value,
    content: Option<&str>,
) -> Option<TypeCodeEdit> {
    let range = te.get("range")?;
    let new_text = te.get("newText")?.as_str()?.to_string();
    let (start, end) = parse_range_to_offsets(range, content)?;
    Some(TypeCodeEdit {
        path: uri.to_string(),
        start,
        end,
        new_text,
    })
}

/// Decode delta-encoded semantic tokens into absolute-offset tokens.
fn decode_semantic_tokens(data: &[serde_json::Value], content: Option<&str>) -> Vec<SemanticToken> {
    if data.len() < 5 {
        return vec![];
    }
    let mut tokens = Vec::new();
    let mut current_line = 0u32;
    let mut current_start = 0u32;

    for chunk in data.chunks_exact(5) {
        let delta_line = chunk[0].as_u64().unwrap_or(0) as u32;
        let delta_start = chunk[1].as_u64().unwrap_or(0) as u32;
        let length = chunk[2].as_u64().unwrap_or(0) as u32;
        let token_type = chunk[3].as_u64().unwrap_or(0) as u32;
        let token_modifiers = chunk[4].as_u64().unwrap_or(0) as u32;

        if delta_line > 0 {
            current_line += delta_line;
            current_start = delta_start;
        } else {
            current_start += delta_start;
        }

        let start = if let Some(c) = content {
            position_to_offset(c, current_line, current_start)
        } else {
            pack_position(current_line, current_start)
        };

        tokens.push(SemanticToken {
            start,
            length,
            token_type,
            token_modifiers,
        });
    }

    tokens
}

/// Parse a DocumentHighlight from a JSON value.
fn parse_document_highlight(
    item: &serde_json::Value,
    content: Option<&str>,
) -> Option<TypeDocumentHighlight> {
    let range = item.get("range")?;
    let (start, end) = parse_range_to_offsets(range, content)?;
    let kind = match item.get("kind").and_then(|v| v.as_u64()) {
        Some(2) => TypeDocumentHighlightKind::Read,
        Some(3) => TypeDocumentHighlightKind::Write,
        _ => TypeDocumentHighlightKind::Text,
    };
    Some(TypeDocumentHighlight { start, end, kind })
}

/// Parse a JSON range `{ start: { line, character }, end: { line, character } }` to byte offsets.
fn parse_range_to_offsets(range: &serde_json::Value, content: Option<&str>) -> Option<(u32, u32)> {
    let start = range.get("start")?;
    let end = range.get("end")?;
    let sl = start.get("line")?.as_u64()? as u32;
    let sc = start.get("character")?.as_u64()? as u32;
    let el = end.get("line")?.as_u64()? as u32;
    let ec = end.get("character")?.as_u64()? as u32;

    if let Some(c) = content {
        Some((position_to_offset(c, sl, sc), position_to_offset(c, el, ec)))
    } else {
        Some((pack_position(sl, sc), pack_position(el, ec)))
    }
}

/// Extract a string from a MarkupContent or plain string JSON value.
fn extract_markup_string(v: &serde_json::Value) -> Option<String> {
    v.as_str()
        .map(String::from)
        .or_else(|| v.get("value").and_then(|v2| v2.as_str()).map(String::from))
}

/// Find the tsgo binary on the system.
///
/// Checks (in order):
/// 1. `tsgo` on PATH
/// 2. Native binary from npx cache (`@typescript/native-preview-{platform}/lib/tsgo`)
/// 3. npx shim in cache
pub fn find_tsgo_binary() -> Option<String> {
    // Check if tsgo is on PATH
    if let Some(path) = which_cmd("tsgo") {
        return Some(path);
    }

    // Search npx cache for the native binary
    if let Some(cache_dir) = npm_cache_npx_dir() {
        // Walk npx cache dirs looking for the native binary
        if let Ok(entries) = std::fs::read_dir(&cache_dir) {
            for entry in entries.flatten() {
                let native_bin = entry
                    .path()
                    .join("node_modules/@typescript/native-preview-win32-x64/lib/tsgo.exe");
                if native_bin.exists() {
                    return Some(native_bin.to_string_lossy().to_string());
                }
                // Also check unix paths
                for platform in &["linux-x64", "darwin-x64", "darwin-arm64"] {
                    let bin = entry.path().join(format!(
                        "node_modules/@typescript/native-preview-{platform}/lib/tsgo"
                    ));
                    if bin.exists() {
                        return Some(bin.to_string_lossy().to_string());
                    }
                }
                // Check the shim (.bin/tsgo)
                let shim = entry.path().join("node_modules/.bin/tsgo");
                if shim.exists() {
                    return Some(shim.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

fn which_cmd(cmd: &str) -> Option<String> {
    let which = if cfg!(windows) { "where" } else { "which" };
    std::process::Command::new(which)
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        // `where` on Windows may return multiple lines; take the first.
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
}

fn npm_cache_npx_dir() -> Option<std::path::PathBuf> {
    // On Windows: %LOCALAPPDATA%/npm-cache/_npx/
    // On Unix: ~/.npm/_npx/
    if cfg!(windows) {
        std::env::var("LOCALAPPDATA")
            .ok()
            .map(|d| std::path::PathBuf::from(d).join("npm-cache/_npx"))
    } else {
        dirs_or_home().map(|d| d.join(".npm/_npx"))
    }
}

fn dirs_or_home() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}

/// Create a temporary project directory with tsconfig.json for testing.
pub fn create_test_project(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "jsx": "preserve",
    "jsxImportSource": "vue",
    "paths": {}
  },
  "include": ["**/*.ts", "**/*.tsx"]
}"#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tsgo_bin_or_skip() -> Option<String> {
        match find_tsgo_binary() {
            Some(bin) => Some(bin),
            None => {
                if std::env::var("VERTER_REQUIRE_TSGO")
                    .map(|v| v == "1")
                    .unwrap_or(false)
                {
                    panic!(
                        "tsgo not found, but VERTER_REQUIRE_TSGO=1 is set; install tsgo or prewarm npx cache",
                    );
                }
                eprintln!("skipping: tsgo not found");
                None
            }
        }
    }

    /// @ai-generated — path_to_uri produces correct file URIs
    #[test]
    fn test_path_to_uri() {
        assert_eq!(
            TsgoTypeProvider::path_to_uri("/home/user/App.vue.tsx"),
            "file:///home/user/App.vue.tsx"
        );
        assert_eq!(
            TsgoTypeProvider::path_to_uri("C:/Users/dev/App.vue.tsx"),
            "file:///C:/Users/dev/App.vue.tsx"
        );
    }

    /// @ai-generated — offset_to_position handles single-line and multi-line content
    #[test]
    fn test_offset_to_position() {
        assert_eq!(offset_to_position("hello world", 0), (0, 0));
        assert_eq!(offset_to_position("hello world", 5), (0, 5));
        assert_eq!(offset_to_position("line1\nline2\nline3", 0), (0, 0));
        assert_eq!(offset_to_position("line1\nline2\nline3", 6), (1, 0));
        assert_eq!(offset_to_position("line1\nline2\nline3", 8), (1, 2));
        assert_eq!(offset_to_position("line1\nline2\nline3", 12), (2, 0));
        assert_eq!(offset_to_position("line1\nline2\nline3", 16), (2, 4));
        // offset at content length
        assert_eq!(offset_to_position("ab\ncd", 5), (1, 2));
    }

    /// @ai-generated — TSGO process spawns and initializes successfully
    #[tokio::test]
    async fn test_tsgo_spawn_and_initialize() {
        let Some(tsgo_bin) = tsgo_bin_or_skip() else {
            return;
        };

        let tmp = std::env::temp_dir().join("verter_tsgo_test_init");
        let _ = std::fs::remove_dir_all(&tmp);
        create_test_project(&tmp).unwrap();

        let root_uri = TsgoTypeProvider::path_to_uri(tmp.to_str().unwrap());
        let provider = TsgoTypeProvider::spawn(&tsgo_bin, &root_uri).await;

        // Clean up
        let _ = std::fs::remove_dir_all(&tmp);

        assert!(
            provider.is_ok(),
            "TSGO should initialize: {:?}",
            provider.err()
        );
    }

    /// @ai-generated — TSGO processes open_file and hover for a .ts file
    #[tokio::test]
    async fn test_tsgo_hover_on_ts_file() {
        let Some(tsgo_bin) = tsgo_bin_or_skip() else {
            return;
        };

        let tmp = std::env::temp_dir().join("verter_tsgo_test_hover");
        let _ = std::fs::remove_dir_all(&tmp);
        create_test_project(&tmp).unwrap();

        // Write a simple TS file
        let ts_path = tmp.join("test.ts");
        std::fs::write(&ts_path, "const msg: string = \"hello\";\n").unwrap();

        let root_uri = TsgoTypeProvider::path_to_uri(tmp.to_str().unwrap());
        let provider = TsgoTypeProvider::spawn(&tsgo_bin, &root_uri).await.unwrap();

        // Open the file
        let file_path = ts_path.to_str().unwrap().replace('\\', "/");
        provider
            .open_file(&file_path, "const msg: string = \"hello\";\n")
            .await
            .unwrap();

        // Give TSGO a moment to process
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Hover on "msg" (offset 6 on line 0)
        let hover = provider.get_hover(&file_path, 6).await.unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&tmp);

        // TSGO should return hover info with the type
        assert!(hover.is_some(), "TSGO should return hover info for 'msg'");
        if let Some(info) = &hover {
            eprintln!("TSGO hover result: {}", info.contents);
            assert!(
                info.contents.contains("string") || info.contents.contains("msg"),
                "hover should mention the type or identifier, got: {}",
                info.contents
            );
        }
    }

    /// @ai-generated — Full E2E: Vue → verter TSX → TSGO hover
    #[tokio::test]
    async fn test_e2e_vue_to_tsgo_hover() {
        let Some(tsgo_bin) = tsgo_bin_or_skip() else {
            return;
        };

        let tmp = std::env::temp_dir().join("verter_tsgo_e2e_hover");
        let _ = std::fs::remove_dir_all(&tmp);
        create_test_project(&tmp).unwrap();

        // Create a Vue SFC
        let vue_source = r#"<script setup lang="ts">
const msg: string = "hello";
const count: number = 42;
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#;

        // Generate TSX using verter_host — upsert then trigger compilation via get_virtual_file
        let host = verter_host::VerterHost::new(verter_host::HostConfig::default());
        let _ = host.upsert(verter_host::UpsertRequest {
            canonical_id: Some("App.vue".to_string()),
            input_id: "App.vue".to_string(),
            source: std::sync::Arc::from(vue_source),
            file_kind: verter_host::FileKind::VueSfc,
            aliases: vec![],
        });

        // Trigger compilation (upsert only parses; get_virtual_file compiles lazily)
        let profile = verter_host::CompileProfile {
            source_map: true,
            enable_types: true,
            ..Default::default()
        };
        let _compiled = host
            .get_virtual_file(verter_host::VirtualQuery {
                raw_id: None,
                canonical_id: Some("App.vue".to_string()),
                node_kind: Some(verter_host::VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("compilation should succeed");

        let tsx = host
            .get_tsx("App.vue", &profile)
            .expect("should have cached TSX after compilation");

        eprintln!(
            "Generated TSX ({} bytes):\n{}",
            tsx.code.len(),
            &tsx.code[..200.min(tsx.code.len())]
        );

        // Write TSX to disk so TSGO can find it
        let tsx_path = tmp.join("App.vue.tsx");
        std::fs::write(&tsx_path, &*tsx.code).unwrap();

        let root_uri = TsgoTypeProvider::path_to_uri(tmp.to_str().unwrap());
        let provider = TsgoTypeProvider::spawn(&tsgo_bin, &root_uri).await.unwrap();

        // Open the TSX file in TSGO
        let tsx_file_path = tsx_path.to_str().unwrap().replace('\\', "/");
        provider.open_file(&tsx_file_path, &tsx.code).await.unwrap();

        // Wait for TSGO to process
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Hover on "msg" — find its byte offset in the TSX
        let msg_offset = tsx.code.find("const msg").map(|i| i as u32 + 6);
        assert!(msg_offset.is_some(), "TSX should contain 'const msg'");
        let offset = msg_offset.unwrap();
        let (line, character) = offset_to_position(&tsx.code, offset);
        eprintln!("Hovering at byte offset {offset} → line {line}, character {character}");

        let hover = provider.get_hover(&tsx_file_path, offset).await.unwrap();

        if let Some(info) = &hover {
            eprintln!("E2E TSGO hover on msg: {}", info.contents);
        }

        assert!(
            hover.is_some(),
            "TSGO should return hover info for 'msg' in TSX at offset {} (line {}, char {})",
            offset,
            line,
            character
        );

        // Clean up
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── Parsing helper unit tests ─────────────────────────────────────

    /// @ai-generated — position_to_offset converts line/char to byte offset
    #[test]
    fn test_position_to_offset_fn() {
        let content = "line1\nline2\nline3";
        assert_eq!(position_to_offset(content, 0, 0), 0);
        assert_eq!(position_to_offset(content, 0, 3), 3);
        assert_eq!(position_to_offset(content, 1, 0), 6);
        assert_eq!(position_to_offset(content, 1, 2), 8);
        assert_eq!(position_to_offset(content, 2, 0), 12);
    }

    /// @ai-generated — parse_completion_item parses a JSON completion item
    #[test]
    fn test_parse_completion_item() {
        let json = serde_json::json!({
            "label": "myVar",
            "kind": 6,
            "detail": "const myVar: string",
            "insertText": "myVar",
            "sortText": "0myVar"
        });
        let item = parse_completion_item(&json, None).unwrap();
        assert_eq!(item.label, "myVar");
        assert!(matches!(item.kind, Some(CompletionKind::Variable)));
        assert_eq!(item.detail.as_deref(), Some("const myVar: string"));
        assert_eq!(item.insert_text.as_deref(), Some("myVar"));
    }

    /// @ai-generated — parse_lsp_location parses an LSP Location with content
    #[test]
    fn test_parse_lsp_location() {
        let json = serde_json::json!({
            "uri": "file:///test.ts",
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 5 }
            }
        });
        let content = "line1\nline2\n";
        let loc = parse_lsp_location(&json, Some(content)).unwrap();
        assert_eq!(loc.path, "file:///test.ts");
        assert_eq!(loc.start, 6);
        assert_eq!(loc.end, 11);
    }

    /// @ai-generated — parse_lsp_diagnostic extracts diagnostics from JSON
    #[test]
    fn test_parse_lsp_diagnostic() {
        let json = serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 5 },
                "end": { "line": 0, "character": 10 }
            },
            "severity": 1,
            "code": 2322,
            "message": "Type error"
        });
        let diag = parse_lsp_diagnostic(&json).unwrap();
        assert_eq!(diag.message, "Type error");
        assert!(matches!(diag.severity, TypeDiagnosticSeverity::Error));
        assert_eq!(diag.code.as_deref(), Some("2322"));
    }

    /// @ai-generated — parse_signature_help parses a SignatureHelp response
    #[test]
    fn test_parse_signature_help_fn() {
        let json = serde_json::json!({
            "signatures": [{
                "label": "fn(x: number): void",
                "documentation": "A test function",
                "parameters": [{ "label": "x", "documentation": "The number param" }]
            }],
            "activeSignature": 0,
            "activeParameter": 0
        });
        let sig = parse_signature_help(&json);
        assert_eq!(sig.signatures.len(), 1);
        assert_eq!(sig.signatures[0].label, "fn(x: number): void");
        assert_eq!(sig.signatures[0].parameters.len(), 1);
        assert_eq!(sig.active_signature, Some(0));
    }

    /// @ai-generated — decode_semantic_tokens decodes delta-encoded tokens
    #[test]
    fn test_decode_semantic_tokens() {
        let content = "const msg = 'hello';\nconst count = 42;\n";
        let data: Vec<serde_json::Value> = vec![
            0.into(),
            0.into(),
            5.into(),
            15.into(),
            0.into(),
            0.into(),
            6.into(),
            3.into(),
            8.into(),
            0.into(),
        ];
        let tokens = decode_semantic_tokens(&data, Some(content));
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].start, 0);
        assert_eq!(tokens[0].length, 5);
        assert_eq!(tokens[1].start, 6);
        assert_eq!(tokens[1].length, 3);
    }

    /// @ai-generated — parse_document_highlight parses highlight JSON
    #[test]
    fn test_parse_document_highlight() {
        let json = serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 6 },
                "end": { "line": 0, "character": 9 }
            },
            "kind": 2
        });
        let content = "const msg = 'hello';\n";
        let hl = parse_document_highlight(&json, Some(content)).unwrap();
        assert_eq!(hl.start, 6);
        assert_eq!(hl.end, 9);
        assert!(matches!(hl.kind, TypeDocumentHighlightKind::Read));
    }

    /// @ai-generated — parse_code_action extracts edits from code action JSON
    #[test]
    fn test_parse_code_action() {
        let json = serde_json::json!({
            "title": "Add import",
            "kind": "quickfix",
            "edit": {
                "changes": {
                    "file:///test.ts": [{
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 0 }
                        },
                        "newText": "import { ref } from 'vue';\n"
                    }]
                }
            }
        });
        let action = parse_code_action(&json, None).unwrap();
        assert_eq!(action.title, "Add import");
        assert_eq!(action.kind.as_deref(), Some("quickfix"));
        assert_eq!(action.edits.len(), 1);
        assert_eq!(action.edits[0].new_text, "import { ref } from 'vue';\n");
    }
}
