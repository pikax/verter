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
    pub insert_text: Option<String>,
    pub sort_text: Option<String>,
    /// Opaque data preserved for `completionItem/resolve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
}

/// Result of resolving a completion item (additional text edits, e.g., auto-import).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResolveResult {
    /// Additional text edits to apply (e.g., import statements).
    pub additional_text_edits: Vec<ResolvedTextEdit>,
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
