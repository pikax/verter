//! File-level usage information for cross-file analysis.
//!
//! Aggregates script and template usage into a single structure
//! suitable for caching, serialization, and cross-file queries.

#![allow(dead_code)]

use crate::common::Span;
use crate::utils::oxc::vue::{
    // Usage types re-exported from script/usage
    ComponentUsageInfo,
    // Script types re-exported from script module
    DeclarationKind,
    EmitCallUsage,
    InjectUsage,
    LifecycleUsage,
    ProvideUsage,
    ReactiveStateUsage,
    ScriptImport,
    ScriptItem,
    ScriptMacro,
    ScriptParseResult,
    SlotDefinitionInfo,
    SlotUsageInfo,
    TemplateRefAttrUsage,
    TemplateUsageCollector,
    UsageCollector,
    VueMacroKind,
    WatcherUsage,
};

// =============================================================================
// File Usage Flags
// =============================================================================

/// Combined bit flags for quick queries about file capabilities.
/// Combines script and template flags into a unified set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileUsageFlags {
    bits: u32,
}

impl FileUsageFlags {
    // Script-level flags (bits 0-7)
    pub const HAS_PROVIDE: u32 = 1 << 0;
    pub const HAS_INJECT: u32 = 1 << 1;
    pub const HAS_LIFECYCLE_HOOKS: u32 = 1 << 2;
    pub const HAS_REACTIVE_STATE: u32 = 1 << 3;
    pub const HAS_WATCHERS: u32 = 1 << 4;
    pub const HAS_EMIT_CALLS: u32 = 1 << 5;
    pub const HAS_TEMPLATE_UTILS: u32 = 1 << 6;
    pub const IS_ASYNC_SETUP: u32 = 1 << 7;

    // Template-level flags (bits 8-11)
    pub const HAS_TEMPLATE_REFS: u32 = 1 << 8;
    pub const HAS_SLOT_USAGE: u32 = 1 << 9;
    pub const HAS_COMPONENT_USAGE: u32 = 1 << 10;
    pub const HAS_SLOT_DEFINITIONS: u32 = 1 << 11;

    // Module-level flags (bits 12-15)
    pub const HAS_IMPORTS: u32 = 1 << 12;
    pub const HAS_EXPORTS: u32 = 1 << 13;
    pub const HAS_MACROS: u32 = 1 << 14;
    pub const IS_SETUP_SCRIPT: u32 = 1 << 15;

    // Macro-specific flags (bits 16-22)
    pub const HAS_DEFINE_PROPS: u32 = 1 << 16;
    pub const HAS_DEFINE_EMITS: u32 = 1 << 17;
    pub const HAS_DEFINE_MODEL: u32 = 1 << 18;
    pub const HAS_DEFINE_EXPOSE: u32 = 1 << 19;
    pub const HAS_DEFINE_OPTIONS: u32 = 1 << 20;
    pub const HAS_DEFINE_SLOTS: u32 = 1 << 21;
    pub const HAS_WITH_DEFAULTS: u32 = 1 << 22;

    /// Create new empty flags
    #[inline]
    pub const fn new() -> Self {
        Self { bits: 0 }
    }

    /// Set a flag
    #[inline]
    pub fn set(&mut self, flag: u32) {
        self.bits |= flag;
    }

    /// Check if a flag is set
    #[inline]
    pub const fn has(&self, flag: u32) -> bool {
        (self.bits & flag) != 0
    }

    /// Get raw bits
    #[inline]
    pub const fn bits(&self) -> u32 {
        self.bits
    }

    /// Merge another flags set into this one
    #[inline]
    pub fn merge(&mut self, other: &Self) {
        self.bits |= other.bits;
    }

    /// Create from raw bits
    #[inline]
    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }
}

// =============================================================================
// Import Info
// =============================================================================

/// Simplified import information for file-level analysis.
/// Stores spans only - actual source text extracted on demand.
#[derive(Debug, Clone)]
pub struct ImportInfo {
    /// Span of the entire import statement
    pub span: Span,
    /// Span of the module specifier string
    pub source_span: Span,
    /// Spans of binding names introduced by this import
    pub binding_spans: Vec<Span>,
    /// Whether this is a type-only import
    pub is_type_only: bool,
}

impl ImportInfo {
    /// Create from ScriptImport
    pub fn from_script_import(import: &ScriptImport<'_>) -> Self {
        Self {
            span: import.span,
            source_span: import.source_span,
            binding_spans: import.bindings.iter().map(|b| b.span).collect(),
            is_type_only: import.is_type_only,
        }
    }
}

// =============================================================================
// Macro Info
// =============================================================================

/// Simplified macro information for file-level analysis.
#[derive(Debug, Clone)]
pub struct MacroInfo {
    /// Span of the macro call
    pub span: Span,
    /// Kind of Vue macro
    pub kind: MacroKind,
    /// Whether it uses type-based syntax
    pub is_type_based: bool,
    /// Binding span if assigned (const props = defineProps(...))
    pub binding_span: Option<Span>,
}

/// Simplified macro kind for aggregation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum MacroKind {
    DefineProps,
    DefineEmits,
    DefineModel,
    DefineExpose,
    DefineOptions,
    DefineSlots,
    WithDefaults,
}

impl From<VueMacroKind> for MacroKind {
    fn from(kind: VueMacroKind) -> Self {
        match kind {
            VueMacroKind::DefineProps => Self::DefineProps,
            VueMacroKind::DefineEmits => Self::DefineEmits,
            VueMacroKind::DefineModel => Self::DefineModel,
            VueMacroKind::DefineExpose => Self::DefineExpose,
            VueMacroKind::DefineOptions => Self::DefineOptions,
            VueMacroKind::DefineSlots => Self::DefineSlots,
            VueMacroKind::WithDefaults => Self::WithDefaults,
        }
    }
}

impl MacroKind {
    /// Get the flag bit for this macro kind
    pub const fn flag(&self) -> u32 {
        match self {
            Self::DefineProps => FileUsageFlags::HAS_DEFINE_PROPS,
            Self::DefineEmits => FileUsageFlags::HAS_DEFINE_EMITS,
            Self::DefineModel => FileUsageFlags::HAS_DEFINE_MODEL,
            Self::DefineExpose => FileUsageFlags::HAS_DEFINE_EXPOSE,
            Self::DefineOptions => FileUsageFlags::HAS_DEFINE_OPTIONS,
            Self::DefineSlots => FileUsageFlags::HAS_DEFINE_SLOTS,
            Self::WithDefaults => FileUsageFlags::HAS_WITH_DEFAULTS,
        }
    }
}

impl MacroInfo {
    /// Create from ScriptMacro
    pub fn from_script_macro(macro_item: &ScriptMacro<'_>) -> Self {
        // Extract common fields based on variant
        let (is_type_based, binding_span) = match macro_item {
            ScriptMacro::DefineProps {
                type_params,
                declarator,
                ..
            } => (
                type_params.is_some(),
                declarator.as_ref().map(|d| d.binding_span),
            ),
            ScriptMacro::DefineEmits {
                type_params,
                declarator,
                ..
            } => (
                type_params.is_some(),
                declarator.as_ref().map(|d| d.binding_span),
            ),
            ScriptMacro::DefineExpose { declarator, .. } => {
                (false, declarator.as_ref().map(|d| d.binding_span))
            }
            ScriptMacro::DefineOptions { declarator, .. } => {
                (false, declarator.as_ref().map(|d| d.binding_span))
            }
            ScriptMacro::DefineModel {
                type_params,
                declarator,
                ..
            } => (
                type_params.is_some(),
                declarator.as_ref().map(|d| d.binding_span),
            ),
            ScriptMacro::DefineSlots {
                type_params,
                declarator,
                ..
            } => (
                type_params.is_some(),
                declarator.as_ref().map(|d| d.binding_span),
            ),
            ScriptMacro::WithDefaults {
                define_props_type_params,
                declarator,
                ..
            } => (
                define_props_type_params.is_some(),
                declarator.as_ref().map(|d| d.binding_span),
            ),
        };

        Self {
            span: macro_item.span(),
            kind: macro_item.kind().into(),
            is_type_based,
            binding_span,
        }
    }
}

// =============================================================================
// Declaration Info
// =============================================================================

/// Simplified declaration information
#[derive(Debug, Clone)]
pub struct DeclarationInfo {
    /// Span of the declaration
    pub span: Span,
    /// Span of the binding name
    pub name_span: Option<Span>,
    /// Kind of declaration
    pub kind: DeclarationKind,
}

// =============================================================================
// File Usage Info (Span-based)
// =============================================================================

/// Aggregated usage information for a single Vue SFC file.
/// Uses spans for efficient single-file analysis.
#[derive(Debug, Default)]
pub struct FileUsageInfo {
    // Module-level information
    /// Imports in the file
    pub imports: Vec<ImportInfo>,
    /// Top-level declarations
    pub declarations: Vec<DeclarationInfo>,
    /// Vue macros used
    pub macros: Vec<MacroInfo>,

    // Script API usage (from UsageCollector)
    /// provide() calls
    pub provides: Vec<ProvideUsage>,
    /// inject() calls
    pub injects: Vec<InjectUsage>,
    /// Lifecycle hooks
    pub lifecycle: Vec<LifecycleUsage>,
    /// Reactive state definitions (ref, reactive, computed)
    pub reactive: Vec<ReactiveStateUsage>,
    /// Watcher definitions
    pub watchers: Vec<WatcherUsage>,
    /// emit() calls
    pub emit_calls: Vec<EmitCallUsage>,

    // Template usage (from TemplateUsageCollector)
    /// Template ref attributes
    pub template_refs: Vec<TemplateRefAttrUsage>,
    /// Slot usages (v-slot)
    pub slot_usages: Vec<SlotUsageInfo>,
    /// Slot definitions (<slot>)
    pub slot_definitions: Vec<SlotDefinitionInfo>,
    /// Component usages
    pub component_usages: Vec<ComponentUsageInfo>,

    /// Quick lookup flags
    pub flags: FileUsageFlags,
}

impl FileUsageInfo {
    /// Create a new empty FileUsageInfo
    pub fn new() -> Self {
        Self::default()
    }

    /// Populate from ScriptParseResult
    pub fn from_script_result(result: &ScriptParseResult<'_>) -> Self {
        let mut info = Self::new();

        if result.is_async {
            info.flags.set(FileUsageFlags::IS_ASYNC_SETUP);
        }

        for item in &result.items {
            match item {
                ScriptItem::Import(import) => {
                    info.flags.set(FileUsageFlags::HAS_IMPORTS);
                    info.imports.push(ImportInfo::from_script_import(import));
                }
                ScriptItem::Declaration(decl) => {
                    info.declarations.push(DeclarationInfo {
                        span: decl.span,
                        name_span: decl.name_span,
                        kind: decl.kind,
                    });
                }
                ScriptItem::Macro(macro_item) => {
                    info.flags.set(FileUsageFlags::HAS_MACROS);
                    let macro_info = MacroInfo::from_script_macro(macro_item);
                    info.flags.set(macro_info.kind.flag());
                    info.macros.push(macro_info);
                }
                ScriptItem::Export(_) | ScriptItem::DefaultExport(_) => {
                    info.flags.set(FileUsageFlags::HAS_EXPORTS);
                }
                ScriptItem::Async(_) => {
                    info.flags.set(FileUsageFlags::IS_ASYNC_SETUP);
                }
                ScriptItem::TypeDeclaration(_) => {
                    // TypeScript-only declarations don't affect usage flags
                    // They are moved outside the component during codegen
                }
            }
        }

        info
    }

    /// Merge script usage from UsageCollector
    pub fn merge_script_usage(&mut self, collector: UsageCollector<'_>) {
        // Transfer flags
        self.flags.bits |= collector.flags.bits();

        // Transfer collections
        self.provides = collector.provides;
        self.injects = collector.injects;
        self.lifecycle = collector.lifecycle;
        self.reactive = collector.reactive;
        self.watchers = collector.watchers;
        self.emit_calls = collector.emit_calls;
    }

    /// Merge template usage from TemplateUsageCollector
    pub fn merge_template_usage(&mut self, collector: TemplateUsageCollector) {
        // Transfer flags
        self.flags.bits |= collector.flags.bits();

        // Transfer collections
        self.template_refs = collector.ref_attrs;
        self.slot_usages = collector.slot_usages;
        self.slot_definitions = collector.slot_definitions;
        self.component_usages = collector.component_usages;
    }

    /// Check if file provides any dependency injection keys
    #[inline]
    pub fn has_provides(&self) -> bool {
        self.flags.has(FileUsageFlags::HAS_PROVIDE)
    }

    /// Check if file injects any dependency injection keys
    #[inline]
    pub fn has_injects(&self) -> bool {
        self.flags.has(FileUsageFlags::HAS_INJECT)
    }

    /// Check if file uses async setup (top-level await)
    #[inline]
    pub fn is_async_setup(&self) -> bool {
        self.flags.has(FileUsageFlags::IS_ASYNC_SETUP)
    }

    /// Check if file defines props
    #[inline]
    pub fn has_define_props(&self) -> bool {
        self.flags.has(FileUsageFlags::HAS_DEFINE_PROPS)
    }

    /// Check if file defines emits
    #[inline]
    pub fn has_define_emits(&self) -> bool {
        self.flags.has(FileUsageFlags::HAS_DEFINE_EMITS)
    }
}

// =============================================================================
// Owned Types for Serialization
// =============================================================================

/// Owned version of ImportInfo for serialization/caching.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportInfoOwned {
    /// The module specifier (e.g., "vue", "./utils")
    pub source: String,
    /// Binding names introduced by this import
    pub bindings: Vec<String>,
    /// Whether this is a type-only import
    pub is_type_only: bool,
    /// Start offset in source
    pub start: u32,
    /// End offset in source
    pub end: u32,
}

/// Owned version of MacroInfo for serialization/caching.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MacroInfoOwned {
    /// Kind of macro
    pub kind: MacroKind,
    /// Whether it uses type-based syntax
    pub is_type_based: bool,
    /// Binding name if assigned
    pub binding_name: Option<String>,
    /// Start offset in source
    pub start: u32,
    /// End offset in source
    pub end: u32,
}

/// Owned version of ProvideUsage for serialization/caching.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProvideUsageOwned {
    /// The injection key (if string literal or known symbol)
    pub key: Option<String>,
    /// Whether key is dynamic/unknown
    pub is_dynamic_key: bool,
    /// Start offset in source
    pub start: u32,
    /// End offset in source
    pub end: u32,
}

/// Owned version of InjectUsage for serialization/caching.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InjectUsageOwned {
    /// The injection key (if string literal or known symbol)
    pub key: Option<String>,
    /// Whether key is dynamic/unknown
    pub is_dynamic_key: bool,
    /// Whether a default value was provided
    pub has_default: bool,
    /// The binding name if assigned
    pub binding_name: Option<String>,
    /// Start offset in source
    pub start: u32,
    /// End offset in source
    pub end: u32,
}

/// Owned version of ComponentUsageInfo for serialization/caching.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ComponentUsageOwned {
    /// Component name (if static)
    pub name: Option<String>,
    /// Whether this is a dynamic component
    pub is_dynamic: bool,
    /// Start offset in source
    pub start: u32,
    /// End offset in source
    pub end: u32,
}

/// Owned version of FileUsageInfo for serialization/caching.
/// Resolves spans to actual string values.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct FileUsageInfoOwned {
    /// Imports in the file
    pub imports: Vec<ImportInfoOwned>,
    /// Macros used
    pub macros: Vec<MacroInfoOwned>,
    /// provide() calls
    pub provides: Vec<ProvideUsageOwned>,
    /// inject() calls
    pub injects: Vec<InjectUsageOwned>,
    /// Component usages
    pub components: Vec<ComponentUsageOwned>,
    /// Raw flags for quick queries
    pub flags: u32,
}

impl FileUsageInfoOwned {
    /// Create from FileUsageInfo by extracting text from source
    pub fn from_span_based(info: &FileUsageInfo, source: &[u8]) -> Self {
        let extract = |span: Span| -> Option<String> {
            let start = span.start as usize;
            let end = span.end as usize;
            if end <= source.len() {
                std::str::from_utf8(&source[start..end])
                    .ok()
                    .map(|s| s.to_string())
            } else {
                None
            }
        };

        let imports = info
            .imports
            .iter()
            .map(|imp| ImportInfoOwned {
                source: extract(imp.source_span).unwrap_or_default(),
                bindings: imp
                    .binding_spans
                    .iter()
                    .filter_map(|&s| extract(s))
                    .collect(),
                is_type_only: imp.is_type_only,
                start: imp.span.start,
                end: imp.span.end,
            })
            .collect();

        let macros = info
            .macros
            .iter()
            .map(|m| MacroInfoOwned {
                kind: m.kind,
                is_type_based: m.is_type_based,
                binding_name: m.binding_span.and_then(&extract),
                start: m.span.start,
                end: m.span.end,
            })
            .collect();

        let provides = info
            .provides
            .iter()
            .map(|p| {
                use crate::utils::oxc::vue::ProvideKeyKind;
                let (key, is_dynamic) = match p.key.kind {
                    ProvideKeyKind::StringLiteral => (extract(p.key.span), false),
                    ProvideKeyKind::Symbol => (extract(p.key.span), false),
                    ProvideKeyKind::Dynamic => (None, true),
                };
                ProvideUsageOwned {
                    key,
                    is_dynamic_key: is_dynamic,
                    start: p.span.start,
                    end: p.span.end,
                }
            })
            .collect();

        let injects = info
            .injects
            .iter()
            .map(|i| {
                use crate::utils::oxc::vue::ProvideKeyKind;
                let (key, is_dynamic) = match i.key.kind {
                    ProvideKeyKind::StringLiteral => (extract(i.key.span), false),
                    ProvideKeyKind::Symbol => (extract(i.key.span), false),
                    ProvideKeyKind::Dynamic => (None, true),
                };
                InjectUsageOwned {
                    key,
                    is_dynamic_key: is_dynamic,
                    has_default: i.has_default,
                    binding_name: i.binding_span.and_then(&extract),
                    start: i.span.start,
                    end: i.span.end,
                }
            })
            .collect();

        let components = info
            .component_usages
            .iter()
            .map(|c| ComponentUsageOwned {
                name: if c.is_dynamic {
                    None
                } else {
                    extract(c.name_span)
                },
                is_dynamic: c.is_dynamic,
                start: c.span.start,
                end: c.span.end,
            })
            .collect();

        Self {
            imports,
            macros,
            provides,
            injects,
            components,
            flags: info.flags.bits(),
        }
    }

    /// Check if a flag is set
    #[inline]
    pub const fn has_flag(&self, flag: u32) -> bool {
        (self.flags & flag) != 0
    }

    /// Get the injection keys provided by this file
    pub fn provided_keys(&self) -> impl Iterator<Item = &str> {
        self.provides.iter().filter_map(|p| p.key.as_deref())
    }

    /// Get the injection keys required by this file
    pub fn injected_keys(&self) -> impl Iterator<Item = &str> {
        self.injects.iter().filter_map(|i| i.key.as_deref())
    }

    /// Get component names used by this file
    pub fn used_components(&self) -> impl Iterator<Item = &str> {
        self.components.iter().filter_map(|c| c.name.as_deref())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_usage_flags() {
        let mut flags = FileUsageFlags::new();
        assert!(!flags.has(FileUsageFlags::HAS_PROVIDE));

        flags.set(FileUsageFlags::HAS_PROVIDE);
        assert!(flags.has(FileUsageFlags::HAS_PROVIDE));
        assert!(!flags.has(FileUsageFlags::HAS_INJECT));

        flags.set(FileUsageFlags::HAS_INJECT);
        assert!(flags.has(FileUsageFlags::HAS_PROVIDE));
        assert!(flags.has(FileUsageFlags::HAS_INJECT));
    }

    #[test]
    fn test_macro_kind_flags() {
        assert_eq!(
            MacroKind::DefineProps.flag(),
            FileUsageFlags::HAS_DEFINE_PROPS
        );
        assert_eq!(
            MacroKind::DefineEmits.flag(),
            FileUsageFlags::HAS_DEFINE_EMITS
        );
        assert_eq!(
            MacroKind::DefineModel.flag(),
            FileUsageFlags::HAS_DEFINE_MODEL
        );
    }

    #[test]
    fn test_flags_merge() {
        let mut flags1 = FileUsageFlags::new();
        flags1.set(FileUsageFlags::HAS_PROVIDE);

        let mut flags2 = FileUsageFlags::new();
        flags2.set(FileUsageFlags::HAS_INJECT);

        flags1.merge(&flags2);
        assert!(flags1.has(FileUsageFlags::HAS_PROVIDE));
        assert!(flags1.has(FileUsageFlags::HAS_INJECT));
    }

    #[test]
    fn test_file_usage_info_helpers() {
        let mut info = FileUsageInfo::new();
        assert!(!info.has_provides());
        assert!(!info.has_injects());

        info.flags.set(FileUsageFlags::HAS_PROVIDE);
        assert!(info.has_provides());
        assert!(!info.has_injects());

        info.flags.set(FileUsageFlags::HAS_DEFINE_PROPS);
        assert!(info.has_define_props());
    }
}
