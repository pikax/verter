//! File-level usage information for cross-file analysis.
//!
//! Provides serializable owned types for aggregating script/template usage
//! information into a single file-level summary, suitable for caching,
//! serialization, and cross-file queries.

use crate::types::AnalyzedMacroKind;

// =============================================================================
// File Usage Flags
// =============================================================================

bitflags::bitflags! {
    /// Combined bit flags for quick queries about file capabilities.
    /// Combines script and template flags into a unified set.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct FileUsageFlags: u32 {
        // Script-level flags (bits 0-7)
        const HAS_PROVIDE         = 1 << 0;
        const HAS_INJECT          = 1 << 1;
        const HAS_LIFECYCLE_HOOKS = 1 << 2;
        const HAS_REACTIVE_STATE  = 1 << 3;
        const HAS_WATCHERS        = 1 << 4;
        const HAS_EMIT_CALLS      = 1 << 5;
        const HAS_TEMPLATE_UTILS  = 1 << 6;
        const IS_ASYNC_SETUP      = 1 << 7;

        // Template-level flags (bits 8-11)
        const HAS_TEMPLATE_REFS    = 1 << 8;
        const HAS_SLOT_USAGE       = 1 << 9;
        const HAS_COMPONENT_USAGE  = 1 << 10;
        const HAS_SLOT_DEFINITIONS = 1 << 11;

        // Module-level flags (bits 12-15)
        const HAS_IMPORTS      = 1 << 12;
        const HAS_EXPORTS      = 1 << 13;
        const HAS_MACROS       = 1 << 14;
        const IS_SETUP_SCRIPT  = 1 << 15;

        // Macro-specific flags (bits 16-22)
        const HAS_DEFINE_PROPS   = 1 << 16;
        const HAS_DEFINE_EMITS   = 1 << 17;
        const HAS_DEFINE_MODEL   = 1 << 18;
        const HAS_DEFINE_EXPOSE  = 1 << 19;
        const HAS_DEFINE_OPTIONS = 1 << 20;
        const HAS_DEFINE_SLOTS   = 1 << 21;
        const HAS_WITH_DEFAULTS  = 1 << 22;

        // Style-level flags (bits 23-28)
        const HAS_SCOPED_STYLE    = 1 << 23;
        const HAS_CSS_MODULES     = 1 << 24;
        const HAS_V_BIND_CSS      = 1 << 25;
        const HAS_DEEP_PSEUDO     = 1 << 26;
        const HAS_GLOBAL_PSEUDO   = 1 << 27;
        const HAS_SLOTTED_PSEUDO  = 1 << 28;
    }
}

impl AnalyzedMacroKind {
    /// Get the corresponding `FileUsageFlags` flag for this macro kind.
    pub const fn usage_flag(&self) -> FileUsageFlags {
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

// =============================================================================
// Owned Types for Serialization / Cross-file Caching
// =============================================================================

/// Owned import information for serialization/caching.
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

/// Owned macro information for serialization/caching.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MacroInfoOwned {
    /// Kind of macro
    pub kind: AnalyzedMacroKind,
    /// Whether it uses type-based syntax
    pub is_type_based: bool,
    /// Binding name if assigned
    pub binding_name: Option<String>,
    /// Start offset in source
    pub start: u32,
    /// End offset in source
    pub end: u32,
}

/// Owned provide() usage for serialization/caching.
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

/// Owned inject() usage for serialization/caching.
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

/// Owned component usage for serialization/caching.
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

/// Owned style usage information for a single `<style>` block.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StyleUsageInfoOwned {
    /// Style language (e.g. "css", "scss")
    pub lang: Option<String>,
    /// Whether this style block is scoped
    pub scoped: bool,
    /// Whether this style block uses CSS modules
    pub is_module: bool,
    /// Custom module name (if `<style module="name">`)
    pub module_name: Option<String>,
    /// v-bind() expressions in this style block
    pub v_bind_expressions: Vec<String>,
    /// CSS class names defined in this style block
    pub class_names: Vec<String>,
    /// CSS ID selectors in this style block
    pub id_names: Vec<String>,
    /// CSS custom property names (including `--` prefix)
    pub custom_property_names: Vec<String>,
    /// Whether this block uses `:deep()`
    pub has_deep: bool,
    /// Whether this block uses `:global()`
    pub has_global: bool,
    /// Whether this block uses `:slotted()`
    pub has_slotted: bool,
}

/// Owned file usage info for serialization/caching.
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
    /// Style block usages
    pub styles: Vec<StyleUsageInfoOwned>,
    /// Raw flags for quick queries (stored as u32 for serde compatibility)
    pub flags: u32,
}

impl FileUsageInfoOwned {
    /// Check if a flag is set
    #[inline]
    pub fn has_flag(&self, flag: FileUsageFlags) -> bool {
        (self.flags & flag.bits()) != 0
    }

    /// Get flags as `FileUsageFlags`
    #[inline]
    pub fn flags(&self) -> FileUsageFlags {
        FileUsageFlags::from_bits_truncate(self.flags)
    }

    /// Set flags from `FileUsageFlags`
    #[inline]
    pub fn set_flags(&mut self, flags: FileUsageFlags) {
        self.flags = flags.bits();
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

    /// Get all CSS class names across all style blocks
    pub fn all_class_names(&self) -> impl Iterator<Item = &str> {
        self.styles
            .iter()
            .flat_map(|s| s.class_names.iter().map(|n| n.as_str()))
    }

    /// Get all v-bind CSS expressions across all style blocks
    pub fn all_v_bind_expressions(&self) -> impl Iterator<Item = &str> {
        self.styles
            .iter()
            .flat_map(|s| s.v_bind_expressions.iter().map(|n| n.as_str()))
    }

    /// Get all CSS custom property names across all style blocks
    pub fn all_custom_properties(&self) -> impl Iterator<Item = &str> {
        self.styles
            .iter()
            .flat_map(|s| s.custom_property_names.iter().map(|n| n.as_str()))
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_usage_flags_set_and_has() {
        let mut flags = FileUsageFlags::empty();
        assert!(!flags.contains(FileUsageFlags::HAS_PROVIDE));

        flags |= FileUsageFlags::HAS_PROVIDE;
        assert!(flags.contains(FileUsageFlags::HAS_PROVIDE));
        assert!(!flags.contains(FileUsageFlags::HAS_INJECT));

        flags |= FileUsageFlags::HAS_INJECT;
        assert!(flags.contains(FileUsageFlags::HAS_PROVIDE));
        assert!(flags.contains(FileUsageFlags::HAS_INJECT));
    }

    #[test]
    fn macro_kind_flags() {
        assert_eq!(
            AnalyzedMacroKind::DefineProps.usage_flag(),
            FileUsageFlags::HAS_DEFINE_PROPS
        );
        assert_eq!(
            AnalyzedMacroKind::DefineEmits.usage_flag(),
            FileUsageFlags::HAS_DEFINE_EMITS
        );
        assert_eq!(
            AnalyzedMacroKind::DefineModel.usage_flag(),
            FileUsageFlags::HAS_DEFINE_MODEL
        );
    }

    #[test]
    fn flags_merge() {
        let mut flags1 = FileUsageFlags::HAS_PROVIDE;
        let flags2 = FileUsageFlags::HAS_INJECT;

        flags1 |= flags2;
        assert!(flags1.contains(FileUsageFlags::HAS_PROVIDE));
        assert!(flags1.contains(FileUsageFlags::HAS_INJECT));
    }

    /// @ai-generated - Style usage flags are set correctly
    #[test]
    fn test_style_usage_flags() {
        let flags = FileUsageFlags::HAS_SCOPED_STYLE
            | FileUsageFlags::HAS_CSS_MODULES
            | FileUsageFlags::HAS_V_BIND_CSS;
        assert!(flags.contains(FileUsageFlags::HAS_SCOPED_STYLE));
        assert!(flags.contains(FileUsageFlags::HAS_CSS_MODULES));
        assert!(flags.contains(FileUsageFlags::HAS_V_BIND_CSS));
        assert!(!flags.contains(FileUsageFlags::HAS_DEEP_PSEUDO));
    }

    /// @ai-generated - all_class_names iterates across multiple style blocks
    #[test]
    fn test_all_class_names_across_blocks() {
        let info = FileUsageInfoOwned {
            styles: vec![
                StyleUsageInfoOwned {
                    class_names: vec!["btn".to_string(), "active".to_string()],
                    ..Default::default()
                },
                StyleUsageInfoOwned {
                    class_names: vec!["card".to_string()],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let classes: Vec<&str> = info.all_class_names().collect();
        assert_eq!(classes, vec!["btn", "active", "card"]);
    }

    /// @ai-generated - StyleUsageInfoOwned default is empty
    #[test]
    fn test_style_usage_default() {
        let style = StyleUsageInfoOwned::default();
        assert!(style.lang.is_none());
        assert!(!style.scoped);
        assert!(!style.is_module);
        assert!(style.class_names.is_empty());
        assert!(style.v_bind_expressions.is_empty());
        assert!(style.custom_property_names.is_empty());
    }

    /// @ai-generated - all_v_bind_expressions and all_custom_properties work across blocks
    #[test]
    fn test_all_v_bind_and_custom_props_across_blocks() {
        let info = FileUsageInfoOwned {
            styles: vec![
                StyleUsageInfoOwned {
                    v_bind_expressions: vec!["color".to_string()],
                    custom_property_names: vec!["--primary".to_string()],
                    ..Default::default()
                },
                StyleUsageInfoOwned {
                    v_bind_expressions: vec!["size".to_string()],
                    custom_property_names: vec!["--spacing".to_string()],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let v_binds: Vec<&str> = info.all_v_bind_expressions().collect();
        assert_eq!(v_binds, vec!["color", "size"]);

        let props: Vec<&str> = info.all_custom_properties().collect();
        assert_eq!(props, vec!["--primary", "--spacing"]);
    }

    /// @ai-generated - Integration: build FileUsageInfoOwned from actual script analysis
    #[test]
    fn integration_from_script_analysis() {
        use crate::analysis::build_script_analysis;
        use oxc_allocator::Allocator;
        use oxc_span::SourceType;

        let code = r#"
import { ref, provide, inject, onMounted } from 'vue';
const count = ref(0);
const props = defineProps<{msg: string}>();
const data = await fetchData();
"#;
        let alloc = Allocator::new();
        let snapshot = build_script_analysis(code, SourceType::ts(), &alloc);

        // Build FileUsageInfoOwned from analysis snapshot
        let mut info = FileUsageInfoOwned::default();
        let mut flags = FileUsageFlags::empty();

        // Transfer imports
        for imp in &snapshot.imports {
            info.imports.push(ImportInfoOwned {
                source: imp.source.clone(),
                bindings: imp.bindings.iter().map(|b| b.name.clone()).collect(),
                is_type_only: imp.is_type_only,
                start: 0,
                end: 0,
            });
            flags |= FileUsageFlags::HAS_IMPORTS;
        }

        // Transfer macros
        for m in &snapshot.macros {
            info.macros.push(MacroInfoOwned {
                kind: m.kind,
                is_type_based: m.is_type_based,
                binding_name: m.binding_name.clone(),
                start: 0,
                end: 0,
            });
            flags |= FileUsageFlags::HAS_MACROS;
            flags |= m.kind.usage_flag();
        }

        // Derive flags from snapshot
        if snapshot
            .flags
            .contains(crate::types::AnalysisFlags::ASYNC_SETUP)
        {
            flags |= FileUsageFlags::IS_ASYNC_SETUP;
        }
        if snapshot
            .flags
            .contains(crate::types::AnalysisFlags::HAS_PROVIDE)
        {
            flags |= FileUsageFlags::HAS_PROVIDE;
        }
        if snapshot
            .flags
            .contains(crate::types::AnalysisFlags::HAS_INJECT)
        {
            flags |= FileUsageFlags::HAS_INJECT;
        }
        if snapshot
            .flags
            .contains(crate::types::AnalysisFlags::HAS_LIFECYCLE_HOOKS)
        {
            flags |= FileUsageFlags::HAS_LIFECYCLE_HOOKS;
        }
        if snapshot
            .flags
            .contains(crate::types::AnalysisFlags::HAS_REACTIVE_STATE)
        {
            flags |= FileUsageFlags::HAS_REACTIVE_STATE;
        }

        info.set_flags(flags);

        // Verify derived data
        assert!(info.has_flag(FileUsageFlags::HAS_IMPORTS));
        assert!(info.has_flag(FileUsageFlags::HAS_MACROS));
        assert!(info.has_flag(FileUsageFlags::HAS_DEFINE_PROPS));
        assert!(info.has_flag(FileUsageFlags::IS_ASYNC_SETUP));
        assert!(info.has_flag(FileUsageFlags::HAS_PROVIDE));
        assert!(info.has_flag(FileUsageFlags::HAS_INJECT));
        assert!(info.has_flag(FileUsageFlags::HAS_LIFECYCLE_HOOKS));
        assert!(info.has_flag(FileUsageFlags::HAS_REACTIVE_STATE));
        assert!(!info.imports.is_empty());
        assert!(!info.macros.is_empty());
    }

    #[test]
    fn file_usage_info_owned_helpers() {
        let info = FileUsageInfoOwned {
            provides: vec![ProvideUsageOwned {
                key: Some("theme".to_string()),
                is_dynamic_key: false,
                start: 0,
                end: 10,
            }],
            injects: vec![InjectUsageOwned {
                key: Some("config".to_string()),
                is_dynamic_key: false,
                has_default: false,
                binding_name: None,
                start: 0,
                end: 10,
            }],
            components: vec![ComponentUsageOwned {
                name: Some("Header".to_string()),
                is_dynamic: false,
                start: 0,
                end: 10,
            }],
            flags: (FileUsageFlags::HAS_PROVIDE | FileUsageFlags::HAS_INJECT).bits(),
            ..Default::default()
        };

        assert!(info.has_flag(FileUsageFlags::HAS_PROVIDE));
        assert!(info.has_flag(FileUsageFlags::HAS_INJECT));
        assert!(!info.has_flag(FileUsageFlags::HAS_DEFINE_PROPS));

        let keys: Vec<&str> = info.provided_keys().collect();
        assert_eq!(keys, vec!["theme"]);

        let injected: Vec<&str> = info.injected_keys().collect();
        assert_eq!(injected, vec!["config"]);

        let components: Vec<&str> = info.used_components().collect();
        assert_eq!(components, vec!["Header"]);
    }
}
