#![deny(missing_docs)]
//! [`McpToolPayload`] — strongly-typed payload for
//! `RequestKind::Mcp`. Producer crates populate the data structure once they emit through the audit substrate.

use serde::{Deserialize, Serialize};

/// MCP tool request payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct McpToolPayload {
    /// Tool name (matches `RequestKind::Mcp.tool`).
    pub tool_name: String,
    /// Approximate args size (bytes).
    pub args_size_bytes: u32,
    /// Approximate result size (bytes).
    pub result_size_bytes: u32,
    /// Optional error message.
    pub error: Option<String>,
}
