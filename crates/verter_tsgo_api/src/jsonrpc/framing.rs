//! vscode-jsonrpc message framing: `Content-Length: N\r\n\r\n` + a UTF-8
//! JSON-RPC 2.0 body.
//!
//! This is the framing the tsgo `--api` ATTACH path speaks (and the same framing
//! the `tsgo --lsp` connection uses), distinct from the crate's STANDALONE
//! MessagePack tuple wire ([`crate::proto::frame`]). The attach client connects to
//! the server-minted pipe and exchanges JSON-RPC 2.0 envelopes over this framing;
//! see [`crate::jsonrpc`] for the connection layer that drives it.
//!
//! Mirrors the shipped rc `typescript` async client's transport
//! (`dist/api/async/client.js` → vendored `vscode-jsonrpc` v9
//! `SocketMessageReader`/`SocketMessageWriter`, `Content-Length` header,
//! `createMessageConnection`). Binary op results ride a base64 `{ data }` field
//! (`apiRequestBinary`: `Buffer.from(response.data, "base64")`).

use crate::error::{TsgoApiError, TsgoApiResult};

/// The framing header prefix, byte-for-byte as the vscode-jsonrpc writer emits it.
const CONTENT_LENGTH_PREFIX: &str = "Content-Length: ";

/// The header/body separator (`\r\n\r\n`).
const HEADER_SEPARATOR: &[u8] = b"\r\n\r\n";

/// Encode a JSON value as one vscode-jsonrpc framed message:
/// `Content-Length: <len>\r\n\r\n<json-bytes>`.
///
/// The `Content-Length` counts the UTF-8 body bytes (not the header), matching
/// the vscode-jsonrpc `WriteableStreamMessageWriter`.
#[must_use]
pub fn encode_message(value: &serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(value).expect("serde_json::Value always serializes");
    let mut out = Vec::with_capacity(body.len() + 32);
    out.extend_from_slice(CONTENT_LENGTH_PREFIX.as_bytes());
    out.extend_from_slice(body.len().to_string().as_bytes());
    out.extend_from_slice(HEADER_SEPARATOR);
    out.extend_from_slice(&body);
    out
}

/// An incremental decoder for vscode-jsonrpc framed messages.
///
/// Feed it received bytes via [`MessageFramer::push`]; call
/// [`MessageFramer::next_message`] to pull each fully-received JSON body. A
/// malformed header (no `Content-Length`, a non-numeric length) is a hard
/// [`TsgoApiError::Codec`] — fail closed, never silently resynchronise on a
/// diverged stream.
#[derive(Debug, Default)]
pub struct MessageFramer {
    buf: Vec<u8>,
}

impl MessageFramer {
    /// A fresh framer with an empty buffer.
    #[must_use]
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Append received bytes to the decode buffer.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Pull the next fully-received message body as a parsed JSON value, or
    /// `Ok(None)` when the buffer does not yet hold a complete framed message.
    ///
    /// Returns `Err` on a malformed frame (a header missing `Content-Length`, a
    /// non-numeric length, or a body that fails JSON parsing) — the stream is
    /// then unrecoverable and the caller must fail closed.
    pub fn next_message(&mut self) -> TsgoApiResult<Option<serde_json::Value>> {
        Ok(self.next_frame()?.map(|(value, _raw)| value))
    }

    /// Pull the next fully-received frame as BOTH the parsed JSON value and
    /// the RAW frame bytes (`Content-Length` header + separator + body,
    /// exactly as received), or `Ok(None)` when the buffer does not yet hold
    /// a complete framed message.
    ///
    /// The raw bytes are captured BEFORE the frame is drained, so a
    /// pass-through consumer (e.g. the [`crate::relay::LspRelay`] pumps) can
    /// forward the original bytes byte-identically — re-encoding the parsed
    /// [`serde_json::Value`] would reorder object keys and recompact
    /// whitespace.
    ///
    /// Returns `Err` on a malformed frame (a header missing `Content-Length`,
    /// a non-numeric length, or a body that fails JSON parsing) — the stream
    /// is then unrecoverable and the caller must fail closed.
    pub fn next_frame(&mut self) -> TsgoApiResult<Option<(serde_json::Value, Vec<u8>)>> {
        let Some(sep) = find_subslice(&self.buf, HEADER_SEPARATOR) else {
            // No complete header yet.
            return Ok(None);
        };
        let header = &self.buf[..sep];
        let content_len = parse_content_length(header)?;
        let body_start = sep + HEADER_SEPARATOR.len();
        let frame_end = body_start + content_len;
        if self.buf.len() < frame_end {
            // Header complete, body still incoming.
            return Ok(None);
        }
        // Capture the EXACT received frame bytes, then drain the consumed
        // frame (header + separator + body).
        let raw = self.buf[..frame_end].to_vec();
        self.buf.drain(..frame_end);
        let value = serde_json::from_slice::<serde_json::Value>(&raw[body_start..])
            .map_err(|e| TsgoApiError::Json(format!("jsonrpc body parse: {e}")))?;
        Ok(Some((value, raw)))
    }
}

/// Parse the `Content-Length` value from a framed header block. The header is the
/// raw bytes up to (not including) the `\r\n\r\n` separator; it may carry
/// additional headers (e.g. `Content-Type`) on their own CRLF-delimited lines.
fn parse_content_length(header: &[u8]) -> TsgoApiResult<usize> {
    let header_str = std::str::from_utf8(header)
        .map_err(|e| TsgoApiError::Codec(format!("jsonrpc header not UTF-8: {e}")))?;
    for line in header_str.split("\r\n") {
        // Case-insensitive match on the field name, per the vscode-jsonrpc reader.
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            let value = rest.trim();
            return value.parse::<usize>().map_err(|_| {
                TsgoApiError::Codec(format!("jsonrpc Content-Length not a number: {value:?}"))
            });
        }
    }
    Err(TsgoApiError::Codec(
        "jsonrpc frame header missing Content-Length".to_string(),
    ))
}

/// Find the first occurrence of `needle` in `haystack`, returning its start index.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Decode a base64 `data` field (the binary-op result carrier, e.g.
/// `getSourceFile` / `typeToTypeNode`). Mirrors `apiRequestBinary`'s
/// `Buffer.from(response.data, "base64")`.
pub fn decode_base64_data(value: &serde_json::Value) -> TsgoApiResult<Vec<u8>> {
    use base64::Engine as _;
    let data = value
        .get("data")
        .and_then(|d| d.as_str())
        .ok_or_else(|| TsgoApiError::Codec("binary result missing `data` field".to_string()))?;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| TsgoApiError::Codec(format!("binary result base64 decode: {e}")))
}

/// Encode bytes as a base64 `data` field (the outgoing binary payload, e.g.
/// `printNode`). Mirrors `uint8ArrayToBase64`:
/// `Buffer.from(data).toString("base64")`.
#[must_use]
pub fn encode_base64_data(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
#[path = "framing_tests.rs"]
mod tests;
