#![deny(missing_docs)]
//! [`CompilePayload`] — strongly-typed payload for
//! `RequestKind::Compile`. Producer crates populate the data structure once they emit through the audit substrate.

use serde::{Deserialize, Serialize};

use crate::payloads::tags::CompileTargetTag;
use crate::record::u64_as_decimal_string;

/// Compile request payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct CompilePayload {
    /// Which codegen target ran (VDOM / Vapor / IDE).
    pub target: CompileTargetTag,
    /// Parse-phase wall-clock (ms) — `None` until producers
    /// instrument it.
    pub parse_ms: Option<f64>,
    /// Transform-phase wall-clock (ms).
    pub transform_ms: Option<f64>,
    /// Codegen-phase wall-clock (ms).
    pub codegen_ms: Option<f64>,
    /// CSS analysis wall-clock (ms).
    pub css_analysis_ms: Option<f64>,
    /// Sourcemap building wall-clock (ms).
    pub sourcemap_ms: Option<f64>,
    /// Bytes of generated output.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub output_bytes: u64,
    /// Bytes of sourcemap output.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub sourcemap_bytes: u64,
    /// Number of directives observed during compile.
    pub num_directives: u32,
    /// Number of components referenced.
    pub num_components: u32,
    /// Number of `<style>` blocks.
    pub num_style_blocks: u32,
    /// Number of `<script>` blocks (regular + setup).
    pub num_script_blocks: u32,
    /// Number of CodeTransform operations executed.
    pub code_transform_ops: u32,
}
