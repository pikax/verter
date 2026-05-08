//! FFI / WASM transport types for the typeinfo host substrate.
//!
//! Mirror the wire-form messages defined in
//! `proto/verter/v1/typeinfo.proto`. Consumers (NAPI, WASM) decode
//! `Buffer` / `Uint8Array` payloads into these structs and pass them
//! through the typeinfo API; the host-side adapter in `verter_ffi`
//! lowers them into `verter_session::typeinfo::types::*` before
//! dispatching.

use serde::{Deserialize, Serialize};

/// Stringly-tagged projection mode. Canonical values are
/// `"identity" | "navigate" | "shallow" | "expanded" | "skeleton"`.
/// Other values surface as `Unknown(String)` at the FFI boundary.
pub const MODE_IDENTITY: &str = "identity";
/// Navigation mode tag.
pub const MODE_NAVIGATE: &str = "navigate";
/// Shallow mode tag.
pub const MODE_SHALLOW: &str = "shallow";
/// Expanded mode tag.
pub const MODE_EXPANDED: &str = "expanded";
/// Skeleton mode tag.
pub const MODE_SKELETON: &str = "skeleton";

/// FFI-boundary mirror of `EvaluateTypeExpressionRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiEvaluateTypeExpressionRequest {
    /// Canonical id of the file the expression evaluates against.
    pub scope: String,
    /// The TypeScript type expression body.
    pub expression: String,
    /// Optional extra imports to inject into the synthesised scratch.
    #[serde(default)]
    pub extra_imports: Vec<FfiImportSpec>,
    /// Stringly-tagged projection mode (see the `MODE_*` constants).
    pub mode: String,
    /// Whether the request publishes to the host's scratch cache.
    pub cacheable: bool,
}

/// FFI-boundary mirror of `ImportSpec`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiImportSpec {
    /// Raw import specifier.
    pub specifier: String,
    /// Per-binding payloads.
    pub bindings: Vec<FfiNamedImport>,
}

/// FFI-boundary mirror of `NamedImport`. The `kind` discriminator
/// names the variant; the union fields are populated per-variant.
/// Consumers branch on `kind` to decode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiNamedImport {
    /// Variant tag — `"default" | "named" | "namespace"`.
    pub kind: String,
    /// `default`: local binding name. `namespace`: local namespace name.
    /// `named`: ignored.
    #[serde(default)]
    pub local_name: String,
    /// `named`: original exported name. Ignored for other variants.
    #[serde(default)]
    pub exported_name: String,
    /// `named`: optional rename. Empty string means "no alias".
    #[serde(default)]
    pub local_alias: String,
    /// `named`: `true` for `import { type X }`. Ignored for other variants.
    #[serde(default)]
    pub type_only: bool,
}

/// FFI-boundary mirror of `SymbolEntry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiSymbolEntry {
    /// Local declaration name.
    pub name: String,
    /// Stringly-tagged kind — `"typeAlias" | "interface" | "class" |
    /// "const" | "let" | "var" | "function" | "asyncFunction" |
    /// "classValue" | "enum"`.
    pub kind: String,
    /// SFC-absolute span start. Set when `hasSpan = true`.
    #[serde(default)]
    pub span_start: u32,
    /// SFC-absolute span end. Set when `hasSpan = true`.
    #[serde(default)]
    pub span_end: u32,
    /// Discriminator — `false` when no analysis-snapshot span was
    /// available for the symbol.
    pub has_span: bool,
    /// `true` when the symbol is exported.
    pub is_exported: bool,
}
