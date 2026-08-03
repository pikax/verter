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
    /// How the editor must interpret the inserted text — plain text or an LSP
    /// snippet (with `$0`/`${1:…}` tab-stop placeholders). Mirrors LSP
    /// `InsertTextFormat`. Provider-neutral: each provider populates it from its
    /// OWN snippet signal (TSGO's `insertTextFormat`; tsserver's `isSnippet`),
    /// never from string-sniffing the label. `None` ⇒ the provider gave no
    /// snippet signal and the consumer treats the item as plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_text_format: Option<CompletionInsertTextFormat>,
    /// Characters that, when typed, commit this completion. Mirrors LSP
    /// `CompletionItem.commitCharacters`. `None` when the provider's wire carries
    /// none for the item (fail-closed — never fabricated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_characters: Option<Vec<String>>,
    /// The text used to filter this item against the typed prefix, when it
    /// differs from `label`. Mirrors LSP `CompletionItem.filterText`. `None`
    /// leaves the editor filtering on the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_text: Option<String>,
    /// Whether the editor should pre-select this item in the list. Mirrors LSP
    /// `CompletionItem.preselect`. `None` ⇒ no preselect signal from the
    /// provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preselect: Option<bool>,
    /// Additional inline label annotations (a trailing signature `detail` and a
    /// right-aligned `description`, e.g. the import source module). Mirrors LSP
    /// `CompletionItemLabelDetails`. `None` when the provider supplied none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_details: Option<CompletionLabelDetails>,
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

/// How a completion item's inserted text must be interpreted by the editor.
///
/// Backend-neutral mirror of LSP `InsertTextFormat`: TSGO carries the numeric
/// `insertTextFormat` (1 = plain, 2 = snippet) on the wire; tsserver signals a
/// snippet via the `isSnippet` boolean on a completion entry. Both normalize
/// onto this two-variant carrier so the LSP merge re-emits the correct
/// `InsertTextFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionInsertTextFormat {
    PlainText,
    Snippet,
}

/// Inline label annotations for a completion item.
///
/// Backend-neutral mirror of LSP `CompletionItemLabelDetails`: `detail` renders
/// immediately after the label (typically a signature/type), `description`
/// renders right-aligned (typically the import source). Either may be `None`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CompletionLabelDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Parse an LSP `CompletionItem.commitCharacters`-shaped JSON value into the
/// neutral [`Completion::commit_characters`] carrier — the SINGLE shared parse
/// used by BOTH provider families (TSGO and tsserver) so the fail-closed
/// semantics cannot drift between them.
///
/// Strict and fail-closed:
/// * the value is absent or not an array → `None` (no signal);
/// * the array exists but ANY element is not a string → `None` (a malformed
///   `commitCharacters` carries no trustworthy signal, so it is dropped whole
///   rather than silently keeping only the well-formed elements);
/// * the array is empty (or yields an empty vec) → `None` (an explicit empty
///   set is "no commit characters", which is the same observable signal as the
///   field being absent — never `Some(vec![])`).
///
/// Otherwise returns `Some(chars)` with the strings in wire order. Allocates the
/// result vector only for a genuine non-empty, all-string array.
pub fn parse_commit_characters(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let arr = value?.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut chars = Vec::with_capacity(arr.len());
    for element in arr {
        // Any non-string element makes the whole array untrustworthy: bail to
        // `None` rather than dropping the malformed element and keeping the rest.
        let s = element.as_str()?;
        chars.push(s.to_string());
    }
    // Defensive: a non-empty array can only yield an empty vec via the
    // early-return above, but normalize the empty result to `None` regardless so
    // the carrier never surfaces `Some(vec![])`.
    if chars.is_empty() {
        None
    } else {
        Some(chars)
    }
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
    /// Resolved label annotations (e.g. an import-source `description` recovered
    /// only at `completionItem/resolve` / `completionEntryDetails` time). `None`
    /// leaves the item's existing label details untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_details: Option<CompletionLabelDetails>,
    /// A command the editor runs after inserting the item (e.g. an upstream-LSP
    /// "trigger signature help" / format-on-accept command carried on the
    /// `completionItem/resolve` response). `None` when the provider supplied
    /// none. Provider-neutral: tsserver completion details carry no `command`,
    /// so the tsserver path always leaves this `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CompletionCommand>,
}

/// A command attached to a completion item, mirroring LSP `Command`.
///
/// Carried on a [`CompletionResolveResult`] when the provider's
/// `completionItem/resolve` response attaches one. `arguments` is an opaque
/// JSON array forwarded verbatim to the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionCommand {
    pub title: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<serde_json::Value>>,
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

/// Witness that a [`DisplaySignature`] is being minted at a provider wire
/// boundary.
///
/// Obtainable ONLY through a [`TypeProvider`] impl
/// ([`crate::traits::TypeProvider::provider_wire_witness`]) — per-engine
/// normalization INTO the neutral protocol is the producer's job, once, at the
/// IPC boundary, and this token is what marks that boundary. The field is
/// non-public (the `CarrierAccessToken { _private: () }` precedent), so an
/// out-of-crate struct literal does not compile.
pub struct DisplaySignatureWireWitness {
    _private: (),
}

impl DisplaySignatureWireWitness {
    pub(crate) fn mint() -> Self {
        Self { _private: () }
    }
}

/// The provider's quick-info display string, VERBATIM (e.g.
/// `const count: Ref<number>`) — a branded DISPLAY value, not a type.
///
/// The brand seals the display domain: consumers render it as-is and must not
/// split, trim to a right-hand side, or otherwise recover structure from it,
/// in Rust or in TypeScript. Structurally enforced (the primary rail is this
/// type's ordinary compile):
///
/// - the inner `String` is private — no out-of-crate struct literal;
/// - no `Deserialize` — the value cannot be conjured off a wire;
/// - no `Deref`/`AsRef<str>`/`Into<String>`/`Display` — no implicit coercion
///   into string-typed (or semantic) APIs;
/// - construction requires a [`DisplaySignatureWireWitness`], obtainable only
///   through a provider impl;
/// - the sole reader is the deliberately-labelled [`Self::as_display_str`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct DisplaySignature(String);

impl DisplaySignature {
    /// Mint the branded display value from the provider's OWN wire response.
    ///
    /// The witness confines minting to provider impls — the only place an
    /// engine's wire shape is known (authority order rule 1: normalization
    /// into the neutral protocol happens once, at the IPC boundary).
    pub fn from_provider_wire(
        _witness: DisplaySignatureWireWitness,
        value: impl Into<String>,
    ) -> Self {
        Self(value.into())
    }

    /// The display string, for RENDERING only.
    ///
    /// Deliberately labelled: a call site reading this into a semantic
    /// decision (type resolution, cache keys, role classification) is a
    /// Typed-IR-Only violation in plain sight.
    pub fn as_display_str(&self) -> &str {
        &self.0
    }

    /// Derive a new branded value by rewriting the display text (display-domain
    /// normalization only, e.g. replacing an implementation-only facade name
    /// with the authored identifier). Requires an existing legitimately-minted
    /// brand — this is not a construction route.
    pub fn with_display_rewrite(&self, rewrite: impl Fn(&str) -> String) -> Self {
        Self(rewrite(&self.0))
    }
}

/// The provider's quick-info symbol kind — a closed, backend-neutral mirror of
/// tsserver's `ScriptElementKind` vocabulary (cf. [`CompletionKind`]).
///
/// tsgo's LSP `textDocument/hover` carries NO kind field, and fabricating one
/// (or sniffing it from display-string prefixes) is forbidden — on that engine
/// the field stays `None` (accepted asymmetry). An unknown wire kind also
/// parses to `None` (fail closed, never fabricated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuickInfoKind {
    Warning,
    Keyword,
    Script,
    Module,
    Class,
    LocalClass,
    Interface,
    Type,
    Enum,
    EnumMember,
    Var,
    LocalVar,
    Using,
    AwaitUsing,
    Function,
    LocalFunction,
    Method,
    Getter,
    Setter,
    Property,
    Accessor,
    Constructor,
    Call,
    Index,
    Construct,
    Parameter,
    TypeParameter,
    PrimitiveType,
    Label,
    Alias,
    Const,
    Let,
    Directory,
    ExternalModuleName,
    JsxAttribute,
    String,
    Link,
    LinkName,
    LinkText,
}

impl QuickInfoKind {
    /// Parse a tsserver `ScriptElementKind` wire string. Unknown or empty
    /// kinds are `None` — fail closed, never fabricated.
    pub fn from_tsserver_wire(kind: &str) -> Option<Self> {
        Some(match kind {
            "warning" => Self::Warning,
            "keyword" => Self::Keyword,
            "script" => Self::Script,
            "module" => Self::Module,
            "class" => Self::Class,
            "local class" => Self::LocalClass,
            "interface" => Self::Interface,
            "type" => Self::Type,
            "enum" => Self::Enum,
            "enum member" => Self::EnumMember,
            "var" => Self::Var,
            "local var" => Self::LocalVar,
            "using" => Self::Using,
            "await using" => Self::AwaitUsing,
            "function" => Self::Function,
            "local function" => Self::LocalFunction,
            "method" => Self::Method,
            "getter" => Self::Getter,
            "setter" => Self::Setter,
            "property" => Self::Property,
            "accessor" => Self::Accessor,
            "constructor" => Self::Constructor,
            "call" => Self::Call,
            "index" => Self::Index,
            "construct" => Self::Construct,
            "parameter" => Self::Parameter,
            "type parameter" => Self::TypeParameter,
            "primitive type" => Self::PrimitiveType,
            "label" => Self::Label,
            "alias" => Self::Alias,
            "const" => Self::Const,
            "let" => Self::Let,
            "directory" => Self::Directory,
            "external module name" => Self::ExternalModuleName,
            "JSX attribute" => Self::JsxAttribute,
            "string" => Self::String,
            "link" => Self::Link,
            "link name" => Self::LinkName,
            "link text" => Self::LinkText,
            _ => return None,
        })
    }

    /// The exact tsserver wire spelling, for DISPLAY labels (`(kind) …`).
    pub fn wire_label(&self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Keyword => "keyword",
            Self::Script => "script",
            Self::Module => "module",
            Self::Class => "class",
            Self::LocalClass => "local class",
            Self::Interface => "interface",
            Self::Type => "type",
            Self::Enum => "enum",
            Self::EnumMember => "enum member",
            Self::Var => "var",
            Self::LocalVar => "local var",
            Self::Using => "using",
            Self::AwaitUsing => "await using",
            Self::Function => "function",
            Self::LocalFunction => "local function",
            Self::Method => "method",
            Self::Getter => "getter",
            Self::Setter => "setter",
            Self::Property => "property",
            Self::Accessor => "accessor",
            Self::Constructor => "constructor",
            Self::Call => "call",
            Self::Index => "index",
            Self::Construct => "construct",
            Self::Parameter => "parameter",
            Self::TypeParameter => "type parameter",
            Self::PrimitiveType => "primitive type",
            Self::Label => "label",
            Self::Alias => "alias",
            Self::Const => "const",
            Self::Let => "let",
            Self::Directory => "directory",
            Self::ExternalModuleName => "external module name",
            Self::JsxAttribute => "JSX attribute",
            Self::String => "string",
            Self::Link => "link",
            Self::LinkName => "link name",
            Self::LinkText => "link text",
        }
    }
}

/// Compose the `(kind) display` signature line, idempotently.
///
/// The shared boundary-formatter half both producers and the LSP hover/detail
/// boundary render through: tsserver's `displayString` may already carry the
/// exact `({kind}) ` prefix for certain symbol kinds (e.g. `(alias) const
/// Foo`), and it is never double-applied. Display-domain composition only.
pub fn kind_prefixed_display(kind: &str, display: &str) -> String {
    if kind.is_empty() {
        return display.to_string();
    }
    let prefix = format!("({kind}) ");
    if display.starts_with(&prefix) {
        display.to_string()
    } else {
        format!("{prefix}{display}")
    }
}

/// Hover information from the type provider.
///
/// `contents` is the RENDERED blob (markdown on tsserver-family producers,
/// normalized plaintext on tsgo) for whole-blob consumers; the structured
/// fields are the semantic-display carriers. Recovering structure by parsing
/// `contents` is forbidden — consumers read the structured fields, and
/// markdown is rendered FROM them once, at the LSP hover boundary.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HoverInfo {
    /// Rendered display text (type signature plus documentation).
    pub contents: String,
    /// Optional byte range in the generated file.
    pub range_start: Option<u32>,
    pub range_end: Option<u32>,
    /// The engine's quick-info display string, verbatim, branded display-only.
    /// `None` when the engine supplied no display data (never fabricated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_signature: Option<DisplaySignature>,
    /// The engine's symbol kind. tsserver populates it from the `quickinfo`
    /// wire; tsgo has no kind on LSP hover and leaves it `None` (accepted
    /// asymmetry — never fabricated, never prefix-sniffed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<QuickInfoKind>,
    /// The engine's documentation block, separated from the signature at the
    /// producer. `None` when the engine supplied none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
}

impl HoverInfo {
    /// The `(kind) display` signature line — the SAME shared boundary
    /// formatter the tsserver-family producers render `contents` through
    /// ([`kind_prefixed_display`]). `None` when the hover carries no
    /// structured signature (fail closed — never scraped from `contents`).
    pub fn kind_labeled_signature(&self) -> Option<String> {
        let display = self.display_signature.as_ref()?.as_display_str();
        Some(match &self.kind {
            Some(kind) => kind_prefixed_display(kind.wire_label(), display),
            None => display.to_string(),
        })
    }
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
    /// Secondary "see declaration here" spans the editor renders as clickable
    /// links under the primary diagnostic (e.g. the duplicate-identifier "also
    /// declared here" span, or "the expected type comes from property 'X'").
    /// tsserver carries these as a `relatedInformation` array (each span has its
    /// own `file`); TSGO carries the native LSP `relatedInformation`
    /// (`location.uri` + `location.range`). Both are normalized onto this carrier
    /// so the LSP merge can map each related span back to the carrier source.
    /// Empty when the diagnostic carries no related information; the
    /// `skip_serializing_if` keeps the no-related case zero-cost on the wire.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_information: Vec<DiagnosticRelatedInfo>,
}

/// A single secondary "related information" span attached to a [`TypeDiagnostic`]
/// — the provider-neutral mirror of LSP `DiagnosticRelatedInformation`.
///
/// `path` / `start` / `end` are in the PROVIDER domain: `path` is the related
/// span's own file (the generated file, a carrier `.tsx`/`.ts`, or a real
/// `.ts`/other file) and `start`/`end` are byte offsets in THAT file (a packed
/// `(line<<16)|col` fallback when the parser had no content for the file, mirroring
/// the primary span's no-content behaviour). The LSP merge maps `(path, start,
/// end)` back to a carrier-source `Location` through the SAME cross-file/carrier
/// mappers references/rename/code-actions use, and drops the entry fail-closed
/// when it cannot be mapped (never a line-0 link).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticRelatedInfo {
    /// Target file path of the related span (provider-domain path).
    pub path: String,
    /// Byte offset of the related span's start in `path`.
    pub start: u32,
    /// Byte offset of the related span's end in `path`.
    pub end: u32,
    /// The related-span message (e.g. "'x' was also declared here.").
    pub message: String,
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
    /// Per-signature active-parameter index, mirroring LSP
    /// `SignatureInformation.activeParameter`. When present it takes precedence
    /// over the top-level [`SignatureHelp::active_parameter`] in LSP clients, so
    /// it carries the active param for THIS overload specifically. `None` when
    /// the provider does not supply a per-signature value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_parameter: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub label: ParameterLabelKind,
    pub documentation: Option<String>,
}

/// A signature-help parameter's label, mirroring the two forms of LSP
/// `ParameterInformation.label`.
///
/// LSP allows a parameter label to be either a literal string OR a
/// `[start, end)` offset pair indexing INTO the enclosing signature `label`.
/// The offset form lets the client bold the exact parameter span within the
/// rendered signature; it is strictly richer than the literal form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum ParameterLabelKind {
    /// A literal label string (rendered as-is).
    Simple(String),
    /// `[start, end)` offset pair INTO the enclosing signature `label`, per the
    /// LSP `ParameterInformation.label` offset form. Offsets are UTF-16 code
    /// units (LSP wire encoding) — preserve exactly what the provider sent / was
    /// computed against the UTF-16 label; do not recompute encoding downstream.
    Offsets(u32, u32),
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
///
/// `token_type` / `token_modifiers` are indices/bits into VERTER's published
/// semantic-token legend (`crate::semantic_tokens::VERTER_TOKEN_TYPES` /
/// `VERTER_TOKEN_MODIFIERS` — the exact legend the LSP advertises), NEVER a
/// provider's own token space. Each provider decodes its backend's encoding
/// (tsserver-family `"2020"` classifications; tsgo's server-advertised legend)
/// and remaps by name through `crate::semantic_tokens::SemanticTokenLegendMap`
/// before constructing one of these; a token the map cannot express is dropped
/// at the provider (fail closed), so consumers may forward these indices to
/// the wire unchanged.
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

    /// The serialized `ParameterLabelKind` wire shape (the cross-process bytes a
    /// signature-help carrier embeds). Pins the adjacently-tagged
    /// `#[serde(tag = "kind", content = "value")]` representation and round-trips
    /// both variants, so a future serde-attr change can't silently break the
    /// signature-help parameter-label shape between processes.
    #[test]
    fn parameter_label_kind_wire_shape_is_pinned() {
        // Simple → {"kind":"Simple","value":"x: number"}
        let simple = ParameterLabelKind::Simple("x: number".to_string());
        let json = serde_json::to_value(&simple).unwrap();
        assert_eq!(json["kind"], "Simple");
        assert_eq!(json["value"], "x: number");
        let back: ParameterLabelKind = serde_json::from_value(json).unwrap();
        assert_eq!(back, simple);

        // Offsets → {"kind":"Offsets","value":[6,11]}
        let offsets = ParameterLabelKind::Offsets(6, 11);
        let json = serde_json::to_value(&offsets).unwrap();
        assert_eq!(json["kind"], "Offsets");
        assert_eq!(json["value"], serde_json::json!([6, 11]));
        assert_eq!(json["value"][0], 6);
        assert_eq!(json["value"][1], 11);
        let back: ParameterLabelKind = serde_json::from_value(json).unwrap();
        assert_eq!(back, offsets);
    }

    /// The shared `parse_commit_characters` helper is strict and fail-closed for
    /// BOTH provider families. Discriminating against the pre-fix per-provider
    /// `filter_map(...).collect()`: that yielded `Some(vec![])` on an empty array
    /// and silently dropped non-string elements (returning the survivors); this
    /// helper returns `None` for every no-trustworthy-signal case.
    #[test]
    fn parse_commit_characters_is_strict_and_fail_closed() {
        // A genuine non-empty all-string array → the strings in wire order.
        let ok = serde_json::json!([".", ";", ","]);
        assert_eq!(
            parse_commit_characters(Some(&ok)),
            Some(vec![".".to_string(), ";".to_string(), ",".to_string()]),
            "a non-empty all-string array parses to the strings in order"
        );

        // Absent → None.
        assert_eq!(
            parse_commit_characters(None),
            None,
            "absent commitCharacters carries no signal"
        );

        // Present but not an array → None (fail-closed, not a panic).
        let not_array = serde_json::json!("oops");
        assert_eq!(
            parse_commit_characters(Some(&not_array)),
            None,
            "a non-array commitCharacters is malformed → None"
        );

        // Empty array → None (NOT Some(vec![])). This is the core regression: the
        // pre-fix code returned Some(empty) here.
        let empty = serde_json::json!([]);
        assert_eq!(
            parse_commit_characters(Some(&empty)),
            None,
            "an empty array is 'no commit characters' → None, never Some(vec![])"
        );

        // Any non-string element makes the whole array untrustworthy → None. The
        // pre-fix filter_map would have silently kept ["."] and dropped the 42.
        let malformed = serde_json::json!([".", 42, ";"]);
        assert_eq!(
            parse_commit_characters(Some(&malformed)),
            None,
            "a non-string element drops the WHOLE array → None (no partial signal)"
        );
    }
}
