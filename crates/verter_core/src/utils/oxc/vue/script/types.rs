//! Type definitions for Vue SFC script parsing.
//!
//! This module contains all the types used to represent parsed script content
//! from Vue Single File Components.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::common::Span;
use crate::syntax::binding_types::BindingType;

/// The parsing mode for a script block
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptMode {
    /// Options API script (no setup attribute)
    Options,
    /// Script setup (with setup attribute)
    Setup,
}

/// Result of parsing a script block
#[derive(Debug, Default)]
pub struct ScriptParseResult<'a> {
    /// Whether the script contains async/await at top level
    pub is_async: bool,
    /// All items found in the script
    pub items: Vec<ScriptItem<'a>>,
    /// Errors encountered during parsing
    pub errors: Vec<ScriptError>,
    /// Binding metadata extracted from `<script setup>`.
    /// Each entry is a (span, type) pair where the span references the identifier
    /// in the parsed script content (offset by `base_offset`).
    pub bindings: Vec<(Span, BindingType)>,
}

/// A parsed script item
#[derive(Debug)]
pub enum ScriptItem<'a> {
    /// Import declaration
    Import(ScriptImport<'a>),
    /// Variable/function/class declaration
    Declaration(ScriptDeclaration<'a>),
    /// TypeScript-only declaration (interface, type alias)
    /// These need to be moved outside the component definition
    TypeDeclaration(ScriptTypeDeclaration<'a>),
    /// Named export
    Export(ScriptExport<'a>),
    /// Default export
    DefaultExport(ScriptDefaultExport<'a>),
    /// Vue macro call
    Macro(ScriptMacro<'a>),
    /// Async marker (await expression or async function)
    Async(ScriptAsync),
}

/// A binding extracted from import specifiers or declarations
#[derive(Debug, Clone)]
pub struct ScriptBinding<'a> {
    /// The local name of the binding
    pub name: &'a str,
    /// Span of the binding identifier
    pub span: Span,
}

/// Import declaration item
#[derive(Debug)]
pub struct ScriptImport<'a> {
    /// Span of the entire import statement
    pub span: Span,
    /// The module specifier (e.g., "vue", "./utils")
    pub source: &'a str,
    /// Span of the source string
    pub source_span: Span,
    /// Bindings introduced by this import
    pub bindings: Vec<ScriptBinding<'a>>,
    /// Whether this is a type-only import
    pub is_type_only: bool,
}

/// Kind of declaration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationKind {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    GeneratorFunction,
    AsyncGeneratorFunction,
    Class,
}

/// Declaration item (variable, function, class)
#[derive(Debug)]
pub struct ScriptDeclaration<'a> {
    /// Span of the declaration
    pub span: Span,
    /// Name of the declared identifier (None for destructuring patterns without simple name)
    pub name: Option<&'a str>,
    /// Span of the name
    pub name_span: Option<Span>,
    /// Kind of declaration
    pub kind: DeclarationKind,
    /// Whether the initializer is a ref-creating call (ref, computed, shallowRef, etc.).
    /// When true, inline template mode will append `.value` to the identifier.
    pub is_ref_like: bool,
}

/// Kind of TypeScript-only declaration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeDeclarationKind {
    /// TypeScript interface: `interface Foo { ... }`
    Interface,
    /// TypeScript type alias: `type Foo = ...`
    TypeAlias,
    /// TypeScript enum: `enum Foo { ... }`
    Enum,
    /// TypeScript module declaration: `namespace Foo { ... }`
    Module,
}

/// TypeScript-only declaration item (interface, type alias, enum)
/// These need to be moved outside the component definition
#[derive(Debug)]
pub struct ScriptTypeDeclaration<'a> {
    /// Span of the full declaration
    pub span: Span,
    /// Name of the type/interface
    pub name: Option<&'a str>,
    /// Kind of type declaration
    pub kind: TypeDeclarationKind,
}

/// Export declaration (named or all)
#[derive(Debug)]
pub struct ScriptExport<'a> {
    /// Span of the export statement
    pub span: Span,
    /// Exported bindings (for named exports)
    pub bindings: Vec<ScriptBinding<'a>>,
    /// Source module (for re-exports like `export * from 'foo'`)
    pub source: Option<&'a str>,
    /// Whether this is a type-only export
    pub is_type_only: bool,
}

/// Type of the default export
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultExportType {
    /// Plain object: `export default { }`
    Object,
    /// defineComponent wrapper: `export default defineComponent({ })`
    DefineComponent,
    /// Function: `export default function() {}`
    Function,
    /// Arrow function: `export default () => {}`
    ArrowFunction,
    /// Class: `export default class {}`
    Class,
    /// Other expression
    Other,
}

/// Default export
#[derive(Debug)]
pub struct ScriptDefaultExport<'a> {
    /// Span of the export default statement
    pub span: Span,
    /// Type of the default export
    pub export_type: DefaultExportType,
    /// For object/defineComponent exports, the object span
    pub object_span: Option<Span>,
    /// For defineComponent, the setup function body span if present
    pub setup_body_span: Option<Span>,
    /// Phantom data to keep lifetime
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> ScriptDefaultExport<'a> {
    pub fn new(span: Span, export_type: DefaultExportType) -> Self {
        Self {
            span,
            export_type,
            object_span: None,
            setup_body_span: None,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn with_object_span(mut self, object_span: Span) -> Self {
        self.object_span = Some(object_span);
        self
    }

    pub fn with_setup_body_span(mut self, setup_body_span: Span) -> Self {
        self.setup_body_span = Some(setup_body_span);
        self
    }
}

/// Async marker
#[derive(Debug)]
pub struct ScriptAsync {
    /// Span of the await expression or async function
    pub span: Span,
}

/// Script parsing error
#[derive(Debug, Clone)]
pub struct ScriptError {
    pub span: Span,
    pub message: ScriptErrorKind,
}

/// Kinds of script parsing errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptErrorKind {
    /// export default is not allowed in script setup
    ExportDefaultInSetup,
    /// return statement is not allowed in script setup
    ReturnInSetup,
    /// Vue macro used outside of script setup
    MacroOutsideSetup,
}

// Re-export macro types from macros module
pub use super::macros::{
    MacroArrayArg, MacroDeclarator, MacroObjectArg, MacroProperty, MacroTypeParams, ScriptMacro,
    VueMacroKind,
};

// =============================================================================
// Analysis Insights System
// =============================================================================

/// Collected insights from parsing - no String allocations.
/// Tracks macro usage, potential issues, and detected patterns.
#[derive(Debug, Default)]
pub struct AnalysisInsights {
    /// Macro usage information
    pub macro_usage: Vec<MacroUsageInfo>,
    /// Potential issues detected
    pub potential_issues: Vec<PotentialIssue>,
    /// Code patterns detected (for optimization hints)
    pub patterns: Vec<DetectedPattern>,
}

impl AnalysisInsights {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a macro usage
    pub fn record_macro(&mut self, span: Span, kind: MacroUsageKind, syntax: MacroSyntax) {
        self.macro_usage.push(MacroUsageInfo { span, kind, syntax });
    }

    /// Record a potential issue
    pub fn record_issue(&mut self, span: Span, kind: IssueKind) {
        self.potential_issues.push(PotentialIssue { span, kind });
    }

    /// Record a detected pattern
    pub fn record_pattern(&mut self, span: Span, kind: PatternKind) {
        self.patterns.push(DetectedPattern { span, kind });
    }
}

/// Information about macro usage in the script
#[derive(Debug, Clone)]
pub struct MacroUsageInfo {
    pub span: Span,
    pub kind: MacroUsageKind,
    pub syntax: MacroSyntax,
}

/// Which Vue macro is being used
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroUsageKind {
    DefineProps,
    DefineEmits,
    DefineModel,
    DefineExpose,
    DefineOptions,
    DefineSlots,
    WithDefaults,
}

impl From<VueMacroKind> for MacroUsageKind {
    fn from(kind: VueMacroKind) -> Self {
        match kind {
            VueMacroKind::DefineProps => MacroUsageKind::DefineProps,
            VueMacroKind::DefineEmits => MacroUsageKind::DefineEmits,
            VueMacroKind::DefineModel => MacroUsageKind::DefineModel,
            VueMacroKind::DefineExpose => MacroUsageKind::DefineExpose,
            VueMacroKind::DefineOptions => MacroUsageKind::DefineOptions,
            VueMacroKind::DefineSlots => MacroUsageKind::DefineSlots,
            VueMacroKind::WithDefaults => MacroUsageKind::WithDefaults,
        }
    }
}

/// The syntax style used for the macro
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroSyntax {
    /// Type-based: defineProps<{ foo: string }>()
    TypeParams,
    /// Runtime object: defineProps({ foo: String })
    ObjectArg,
    /// Runtime array: defineProps(['foo', 'bar'])
    ArrayArg,
    /// No arguments: defineExpose()
    NoArgs,
}

/// A potential issue detected during parsing
#[derive(Debug, Clone)]
pub struct PotentialIssue {
    pub span: Span,
    pub kind: IssueKind,
}

/// Kinds of potential issues that can be detected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// Prop declared but never used in template
    UnusedProp,
    /// Emit declared but never called
    UnusedEmit,
    /// Type couldn't be resolved
    UnresolvedType,
    /// Duplicate prop/emit name
    DuplicateName,
    /// Complex type that falls back to Object
    ComplexTypeFallback,
    /// Missing required default in withDefaults
    MissingDefault,
}

impl IssueKind {
    /// Get static message - no allocation
    pub const fn message(&self) -> &'static str {
        match self {
            Self::UnusedProp => "Prop declared but not used in template",
            Self::UnusedEmit => "Emit declared but never called",
            Self::UnresolvedType => "Type could not be resolved",
            Self::DuplicateName => "Duplicate prop or emit name",
            Self::ComplexTypeFallback => "Complex type resolved to Object",
            Self::MissingDefault => "Optional prop without default in withDefaults",
        }
    }

    /// Get severity level for this issue
    pub const fn severity(&self) -> Severity {
        match self {
            Self::UnusedProp | Self::UnusedEmit => Severity::Warning,
            Self::UnresolvedType | Self::ComplexTypeFallback => Severity::Info,
            Self::DuplicateName => Severity::Error,
            Self::MissingDefault => Severity::Warning,
        }
    }
}

/// Severity level for issues
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A detected code pattern
#[derive(Debug, Clone)]
pub struct DetectedPattern {
    pub span: Span,
    pub kind: PatternKind,
}

/// Kinds of patterns that can be detected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    /// Multiple defineModel calls (v-model pattern)
    MultipleModels,
    /// Props + Emits + Model combination
    FullModelPattern,
    /// Generic component usage
    GenericComponent,
    /// Async setup (top-level await)
    AsyncSetup,
}
