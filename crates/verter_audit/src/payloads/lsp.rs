#![deny(missing_docs)]
//! [`LspRequestPayload`] — strongly-typed payload for
//! `RequestKind::Lsp`. Producer crates populate the data structure once they emit through the audit substrate.

use serde::{Deserialize, Serialize};

use crate::payloads::tags::LspMethodTag;

/// LSP request payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct LspRequestPayload {
    /// LSP method name.
    pub method: LspMethodTag,
    /// Position info for position-bound methods (hover, goto-def,
    /// completion, …).
    pub position: Option<PositionInfo>,
    /// Approximate response payload size (bytes) — populated by
    /// producers when the response is serialised.
    pub response_size_bytes: u32,
    /// Approximate request payload size (bytes).
    pub request_size_bytes: u32,
    /// Number of diagnostics (for `publishDiagnostics`).
    pub num_diagnostics: Option<u32>,
    /// Number of completion items (for `completion`).
    pub num_completion_items: Option<u32>,
    /// Number of document symbols (for `documentSymbol`).
    pub num_symbols: Option<u32>,
    /// Number of references (for `references`).
    pub num_references: Option<u32>,
    /// Optional error / cancellation marker. `Some("cancelled")`
    /// signals a cancellation per the LSP cancellation contract.
    pub error: Option<String>,
}

/// Editor position carried by LSP-method payloads. Producers
/// populate from the LSP request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct PositionInfo {
    /// Canonical id of the file.
    pub canonical_id: String,
    /// Zero-based line.
    pub line: u32,
    /// Zero-based character offset within the line.
    pub character: u32,
}
