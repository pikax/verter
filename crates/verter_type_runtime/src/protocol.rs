//! Backend-runtime protocol DTOs.
//!
//! These types define the data shapes for communication with external TypeScript
//! backends (tsserver, TSGO). They are backend-neutral — both tsserver and TSGO
//! responses are parsed into these types.
//!
//! Moved from `verter_lsp::tsgo::protocol` to be shared between LSP and
//! component-meta consumers.

use serde::{Deserialize, Serialize};

/// Error from a type provider operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeProviderError {
    pub message: String,
}

impl std::fmt::Display for TypeProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TypeProviderError {}

impl TypeProviderError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Result of a completion request from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResult {
    pub items: Vec<Completion>,
    /// Whether the completion list is incomplete (more items available as the user types).
    pub is_incomplete: bool,
}

/// A completion item from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Completion {
    pub label: String,
    pub kind: Option<CompletionKind>,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    /// Byte offset of the text edit range start in the generated file.
    pub edit_range_start: Option<u32>,
    /// Byte offset of the text edit range end in the generated file.
    pub edit_range_end: Option<u32>,
    /// The provider's `textEdit.newText` — the text a SURVIVING replace-range
    /// commits. Per LSP, when a completion item carries a `textEdit` the editor
    /// applies this and IGNORES `insert_text`. It is also the preferred
    /// plain-insert fallback when the range is dropped fail-closed (the text the
    /// dropped edit would have applied). `None` when the item has no `textEdit`.
    pub text_edit_new_text: Option<String>,
    /// The provider's explicit `insertText` — the plain-insert text used only
    /// when there is NO `textEdit` (or as the fallback after
    /// [`Self::text_edit_new_text`] when a `textEdit`'s range was dropped). It is
    /// NOT the surviving-edit payload. `None` when the item supplied no explicit
    /// `insertText`.
    pub insert_text: Option<String>,
    pub sort_text: Option<String>,
    /// Typed, provider-owned resolve key preserved for `completionItem/resolve`.
    ///
    /// This is the provider's OWN lazy-resolve handle — NOT an LSP routing
    /// payload. Each provider mints the variant that lets it re-issue the
    /// resolve request (`Lsp` for the upstream-LSP `data` blob; `TsserverEntry`
    /// for a `completionEntryDetails` lookup). LSP routing fields (carrier path,
    /// provider id) live in the LSP-side envelope, never inside this key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<CompletionResolveData>,
}

/// Provider-pure lazy-resolve key carried on a [`Completion`].
///
/// A type provider attaches the variant it can later re-issue a resolve with.
/// It deliberately holds NO LSP routing information (no generated-file path, no
/// `provider_id`); the LSP layer wraps this in its own envelope when it needs to
/// route a `completionItem/resolve` back to the originating provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompletionResolveData {
    /// LSP-shaped resolve handle (TSGO and any upstream-LSP provider): the
    /// completion item's original `label` plus its opaque upstream `data`
    /// field, replayed verbatim into a `completionItem/resolve` request.
    Lsp {
        label: String,
        data: serde_json::Value,
    },
    /// tsserver-family resolve handle: the entry `name` plus the optional
    /// `source`/`data` fields a `completionEntryDetails` request keys on to
    /// recover the auto-import code actions for that specific entry, and the
    /// `offset` (byte offset into the generated file) of the completion request
    /// the entry came from — `completionEntryDetails` must be re-issued at the
    /// same position. The offset is provider-domain (a position in the
    /// provider's own generated file), not LSP routing.
    ///
    /// STALE-OFFSET FRAGILITY (review finding H3): `offset` is captured at
    /// completion-LIST time and re-converted to a tsserver `(line, offset)` via
    /// [`crate::tsserver::ipc::byte_offset_to_tsserver_pos`] against the file
    /// content the provider holds at RESOLVE time. If the open buffer changed
    /// between the list request and the accept (the editor inserted/removed text
    /// before this byte offset, advancing the generated artifact), the stored
    /// byte offset now points at a DIFFERENT line/col, and tsserver may resolve
    /// the wrong entry's `completionEntryDetails` (or none). This is acceptable
    /// in practice because `completionItem/resolve` fires immediately on accept
    /// (the user is not typing mid-accept) and a re-keyed resolve simply returns
    /// no edits (fail-closed — never a wrong import), but it is a real lazy-resolve
    /// limitation. The robust fix (re-anchoring the offset against the live
    /// version, or carrying a version stamp) is a follow-up; until it lands a
    /// drifted offset degrades to "no auto-import edit", not a corrupt one. The
    /// characterization test `stamped_offset_drifts_when_buffer_changes_before_resolve`
    /// pins this behavior.
    TsserverEntry {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
        offset: u32,
    },
}

impl CompletionResolveData {
    /// Whether this resolve handle can produce a NON-trivial `completionItem/resolve`
    /// result — i.e. an auto-import (additional text edits).
    ///
    /// Only an ACTIONABLE handle is worth minting an LSP resolve envelope for: a
    /// local member completion (a `TsserverEntry` with neither `source` nor `data`)
    /// resolves to nothing but lazy detail, so stamping it would be per-keystroke
    /// payload bloat and a no-op resolve round-trip (review finding F3).
    ///
    /// - `Lsp { data }` — the upstream-LSP provider (TSGO) only attaches `data` on
    ///   an entry that has a resolvable action; the parser drops `data` for entries
    ///   without one, so any `Lsp` handle that exists is actionable.
    /// - `TsserverEntry` — an auto-import (module-export) entry carries `source`
    ///   and/or `data` (the `completionEntryDetails` resolve key). A bare
    ///   name-only handle (a local symbol) is NOT actionable.
    ///
    /// `source`/`data` is the COMPLETE and durable actionability rail for the
    /// auto-import class — `hasAction` is deliberately NOT modeled. tsserver's
    /// `hasAction` is purely an output hint (not an input to the
    /// `getCompletionEntryDetails` lookup, which keys on `name`/`source`/`data`),
    /// and an auto-import entry ALWAYS carries `source` (the module specifier).
    /// The remaining `hasAction: true`-without-`source`/`data` shapes —
    /// class-member snippet completions, object-literal missing-comma insertion,
    /// and type-only-alias wrappers — are a DIFFERENT code-action class that this
    /// resolve path does NOT route as an import (routing them through the
    /// auto-import envelope would mis-key their resolve and produce no edit). If a
    /// future block adds support for that non-import action class it gets its OWN
    /// handle variant, not a `hasAction` flag bolted onto the auto-import rail.
    pub fn is_actionable(&self) -> bool {
        match self {
            CompletionResolveData::Lsp { .. } => true,
            CompletionResolveData::TsserverEntry { source, data, .. } => {
                source.is_some() || data.is_some()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionKind {
    Function,
    Variable,
    Property,
    Class,
    Interface,
    Module,
    Keyword,
    Snippet,
    Text,
    Method,
    Field,
    Enum,
    EnumMember,
    Constant,
    TypeParameter,
    File,
    Folder,
}

/// Result of resolving a completion item (additional text edits, e.g., auto-import).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletionResolveResult {
    /// Additional text edits to apply (e.g., import statements).
    pub additional_text_edits: Vec<ResolvedTextEdit>,
    /// Resolved detail/signature text (lazy `completionItem/resolve` enrichment),
    /// when the provider returns one. `None` leaves the item's existing detail
    /// untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Resolved documentation (markdown/plaintext) for the item, when the
    /// provider returns one. `None` leaves the item's existing docs untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
}

/// A text edit within a completion resolve result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTextEdit {
    /// Byte offset start in the generated file.
    pub start: u32,
    /// Byte offset end in the generated file.
    pub end: u32,
    /// The replacement text.
    pub new_text: String,
}

/// Hover information from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverInfo {
    /// Type signature or documentation text.
    pub contents: String,
    /// Optional byte range in the generated file.
    pub range_start: Option<u32>,
    pub range_end: Option<u32>,
}

/// A diagnostic from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDiagnostic {
    pub message: String,
    pub severity: TypeDiagnosticSeverity,
    /// Byte offset range in the generated file.
    pub start: u32,
    pub end: u32,
    pub code: Option<String>,
    /// Editor-facing diagnostic tags (e.g. unused-symbol fade, deprecation
    /// strikethrough). tsserver reports these as the `reportsUnnecessary` /
    /// `reportsDeprecated` booleans; TSGO carries the native LSP `tags` array.
    /// Both are normalized into this carrier so the LSP merge can re-emit them
    /// as `DiagnosticTag`s. Empty when the diagnostic carries no tags.
    pub tags: Vec<TypeDiagnosticTag>,
}

/// Editor-facing diagnostic tag, mirroring the LSP `DiagnosticTag` taxonomy.
///
/// `Unnecessary` fades unused code (TS6133 and friends); `Deprecated` renders a
/// strikethrough. Kept provider-neutral so both the tsserver boolean flags and
/// the TSGO native LSP `tags` array map onto the same carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeDiagnosticTag {
    Unnecessary,
    Deprecated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TypeDiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A source location from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeLocation {
    pub path: String,
    /// Byte offset range in the file.
    pub start: u32,
    pub end: u32,
}

/// A rename location from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameLocation {
    pub path: String,
    pub start: u32,
    pub end: u32,
}

/// Signature help from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureHelp {
    pub signatures: Vec<SignatureInfo>,
    pub active_signature: Option<u32>,
    pub active_parameter: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureInfo {
    pub label: String,
    pub documentation: Option<String>,
    pub parameters: Vec<ParameterInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub label: String,
    pub documentation: Option<String>,
}

/// A code action from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeCodeAction {
    pub title: String,
    pub kind: Option<String>,
    pub edits: Vec<TypeCodeEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeCodeEdit {
    pub path: String,
    pub start: u32,
    pub end: u32,
    pub new_text: String,
}

/// A diagnostic's identity carried into a code-action request.
///
/// The LSP code-action handler resolves `params.context.diagnostics` into this
/// shape before calling [`TypeProvider::get_code_actions`]: the parsed integer
/// error `code` plus the diagnostic range already mapped to TSX byte offsets in
/// the queried generated file. Backends consume it differently — the
/// tsserver-family providers feed `code` into `getCodeFixes` `errorCodes`, while
/// TSGO synthesizes an LSP `Diagnostic` (range from `start`/`end`, integer
/// `code`) for `textDocument/codeAction` `context.diagnostics`. A diagnostic
/// whose code is non-numeric or whose range does not map to TSX is dropped
/// before reaching here (fail-closed), so every context carries a real code and
/// a real TSX span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDiagnosticContext {
    /// The diagnostic's TypeScript error code (e.g. `6133` for an unused
    /// declaration), parsed to an integer from the LSP `NumberOrString` code.
    pub code: u32,
    /// Start byte offset of the diagnostic range in the queried TSX file.
    pub start: u32,
    /// End byte offset of the diagnostic range in the queried TSX file.
    pub end: u32,
}

/// A semantic token from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticToken {
    pub start: u32,
    pub length: u32,
    pub token_type: u32,
    pub token_modifiers: u32,
}

/// An inlay hint from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlayHint {
    /// Byte offset in the generated file.
    pub position: u32,
    /// The label to display.
    pub label: String,
    /// Hint kind: Type or Parameter.
    pub kind: Option<InlayHintKind>,
    pub padding_left: Option<bool>,
    pub padding_right: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum InlayHintKind {
    Type,
    Parameter,
}

/// A document highlight from the type provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDocumentHighlight {
    pub start: u32,
    pub end: u32,
    pub kind: TypeDocumentHighlightKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TypeDocumentHighlightKind {
    Text,
    Read,
    Write,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_resolve_handle_is_always_actionable() {
        // TSGO only attaches `data` on an entry with a resolvable action, so any
        // `Lsp` handle that exists is actionable.
        let handle = CompletionResolveData::Lsp {
            label: "computed".to_string(),
            data: serde_json::json!({ "exportName": "computed" }),
        };
        assert!(handle.is_actionable());
    }

    #[test]
    fn tsserver_auto_import_entry_is_actionable_local_is_not() {
        // An auto-import (module-export) entry carries source and/or data.
        let auto_import = CompletionResolveData::TsserverEntry {
            name: "computed".to_string(),
            source: Some("vue".to_string()),
            data: Some(serde_json::json!({ "exportName": "computed" })),
            offset: 7,
        };
        assert!(
            auto_import.is_actionable(),
            "an entry with source/data resolves to an auto-import edit"
        );

        // A bare name-only handle (a local symbol) resolves to nothing actionable.
        let local = CompletionResolveData::TsserverEntry {
            name: "myLocalVar".to_string(),
            source: None,
            data: None,
            offset: 7,
        };
        assert!(
            !local.is_actionable(),
            "a local member entry (no source/data) must NOT be actionable — minting an \
             envelope for it is per-keystroke payload bloat and a no-op resolve"
        );

        // Source-only (no data) is still an auto-import entry.
        let source_only = CompletionResolveData::TsserverEntry {
            name: "computed".to_string(),
            source: Some("vue".to_string()),
            data: None,
            offset: 7,
        };
        assert!(source_only.is_actionable());
    }

    /// The serialized `CompletionResolveData` wire shape (the bytes the LSP
    /// envelope embeds and a provider deserializes). Pins the `#[serde(tag =
    /// "kind", rename_all = "snake_case")]` discriminant and field spellings so a
    /// rename can't silently break the cross-process resolve round-trip.
    #[test]
    fn completion_resolve_data_wire_shape_is_pinned() {
        let lsp = CompletionResolveData::Lsp {
            label: "computed".to_string(),
            data: serde_json::json!({ "exportName": "computed" }),
        };
        let json = serde_json::to_value(&lsp).unwrap();
        assert_eq!(json["kind"], "lsp");
        assert_eq!(json["label"], "computed");
        assert_eq!(json["data"]["exportName"], "computed");

        let entry = CompletionResolveData::TsserverEntry {
            name: "computed".to_string(),
            source: Some("vue".to_string()),
            data: Some(serde_json::json!({ "exportName": "computed" })),
            offset: 7,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "tsserver_entry");
        assert_eq!(json["name"], "computed");
        assert_eq!(json["source"], "vue");
        assert_eq!(json["offset"], 7);

        // A local entry omits source/data (skip_serializing_if), keeping a local
        // completion's payload minimal.
        let local = CompletionResolveData::TsserverEntry {
            name: "x".to_string(),
            source: None,
            data: None,
            offset: 3,
        };
        let json = serde_json::to_value(&local).unwrap();
        assert_eq!(json["kind"], "tsserver_entry");
        assert!(json.get("source").is_none(), "absent source is omitted");
        assert!(json.get("data").is_none(), "absent data is omitted");

        // Round-trips back to the same value.
        let back: CompletionResolveData = serde_json::from_value(json).unwrap();
        assert_eq!(back, local);
    }
}
