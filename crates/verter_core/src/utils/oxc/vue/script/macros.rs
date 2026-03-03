//! Vue macro detection and types.
//!
//! This module provides byte-based detection of Vue compiler macros like
//! defineProps, defineEmits, etc., and the types to represent them.

#![allow(dead_code)]

use crate::common::Span;

use super::resolve_type::{ResolvedElements, RuntimeType};

/// Type parameters info: defineProps<Props>() or defineEmits<{ (e: 'change'): void }>()
#[derive(Debug)]
pub struct MacroTypeParams {
    /// Span of the `<` token
    pub lt_span: Span,
    /// Span of the type name/body (between < and >)
    pub type_span: Span,
    /// Span of the `>` token
    pub gt_span: Span,
    /// Resolved type information from the type parameter (for type literals)
    pub resolved: ResolvedElements,
    /// Inferred runtime types for the root type parameter (for simple types like `string`)
    pub runtime_types: Vec<RuntimeType>,
    /// Whether the type parameter was a type reference (e.g., `Props` in `defineProps<Props>()`)
    /// that could not be resolved. Used to emit "Unresolvable type reference" diagnostics.
    pub unresolved_type_ref: bool,
}

/// Object argument info: defineProps({ foo: String })
#[derive(Debug)]
pub struct MacroObjectArg<'a> {
    /// Span of the entire object `{ ... }`
    pub span: Span,
    /// Property spans (name spans only)
    pub properties: Vec<MacroProperty<'a>>,
}

/// A property in a macro object argument.
///
/// For `defineProps` object syntax, the AST-extracted fields provide all type
/// information needed for codegen — no string parsing of prop values is needed.
#[derive(Debug)]
pub struct MacroProperty<'a> {
    /// The property name
    pub name: &'a str,
    /// Span of the property name
    pub name_span: Span,
    /// Span of the value (Some for { foo: String }, None for shorthand)
    pub value_span: Option<Span>,
    /// Whether this property uses method shorthand (e.g., `foo() { ... }`)
    pub is_method: bool,
    /// Whether the prop has `required: true` in object form.
    /// Extracted from the OXC AST (not string parsing).
    pub required: bool,
    /// Whether the prop has a `default:` key in object form.
    /// Extracted from the OXC AST (not string parsing).
    pub has_default: bool,
    /// Runtime constructor types extracted from the AST.
    /// - `title: String` → `[RuntimeType::String]`
    /// - `value: [String, Number]` → `[RuntimeType::String, RuntimeType::Number]`
    /// - `{ type: Number }` → `[RuntimeType::Number]`
    /// - `{ type: [String, Number] }` → `[RuntimeType::String, RuntimeType::Number]`
    pub runtime_types: Vec<RuntimeType>,
    /// Span of the TypeScript type annotation from `as PropType<T>`, if present.
    /// Points to `T` inside `PropType<T>`.
    /// - `Array as PropType<string[]>` → span of `string[]`
    /// - `{ type: Object as PropType<{name: string}> }` → span of `{name: string}`
    pub prop_type_annotation: Option<Span>,
}

/// Array argument info: defineEmits(['change', 'update'])
#[derive(Debug)]
pub struct MacroArrayArg {
    /// Span of the entire array
    pub span: Span,
    /// Spans of individual elements
    pub element_spans: Vec<Span>,
}

/// Declarator info for macro assignments: `const props = defineProps()`
#[derive(Debug)]
pub struct MacroDeclarator<'a> {
    /// The binding name (e.g., "props" in `const props = defineProps()`)
    /// None for destructuring patterns like `const { foo } = defineProps()`
    pub name: Option<&'a str>,
    /// Span of the binding identifier or pattern
    pub binding_span: Span,
    /// Span of the entire variable declaration statement (includes `const`/`let`/`var`)
    pub statement_span: Span,
}

/// Vue macro kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VueMacroKind {
    DefineProps = 0,
    DefineEmits = 1,
    DefineExpose = 2,
    DefineOptions = 3,
    DefineModel = 4,
    DefineSlots = 5,
    WithDefaults = 6,
}

/// Vue macro call with full span information
#[derive(Debug)]
pub enum ScriptMacro<'a> {
    /// defineProps<T>() or defineProps({ ... }) or defineProps([...])
    DefineProps {
        span: Span,
        declarator: Option<MacroDeclarator<'a>>,
        type_params: Option<MacroTypeParams>,
        object_arg: Option<MacroObjectArg<'a>>,
        array_arg: Option<MacroArrayArg>,
    },

    /// defineEmits<T>() or defineEmits([...]) or defineEmits({ event: (payload) => true })
    DefineEmits {
        span: Span,
        declarator: Option<MacroDeclarator<'a>>,
        type_params: Option<MacroTypeParams>,
        object_arg: Option<MacroObjectArg<'a>>,
        array_arg: Option<MacroArrayArg>,
    },

    /// defineExpose({ ... })
    DefineExpose {
        span: Span,
        declarator: Option<MacroDeclarator<'a>>,
        object_arg: Option<MacroObjectArg<'a>>,
    },

    /// defineOptions({ ... })
    DefineOptions {
        span: Span,
        declarator: Option<MacroDeclarator<'a>>,
        object_arg: Option<MacroObjectArg<'a>>,
        /// Whether `inheritAttrs: false` is present in the options object.
        has_inherit_attrs_false: bool,
    },

    /// defineModel<T>(name?, options?)
    DefineModel {
        span: Span,
        declarator: Option<MacroDeclarator<'a>>,
        type_params: Option<MacroTypeParams>,
        /// First string arg if present (model name)
        name_span: Option<Span>,
        /// Second object arg if present (options)
        options_span: Option<Span>,
    },

    /// defineSlots<T>()
    DefineSlots {
        span: Span,
        declarator: Option<MacroDeclarator<'a>>,
        type_params: Option<MacroTypeParams>,
    },

    /// withDefaults(defineProps<T>(), { ... })
    WithDefaults {
        span: Span,
        declarator: Option<MacroDeclarator<'a>>,
        /// The inner defineProps call span (may be missing in invalid scripts)
        define_props_span: Option<Span>,
        /// Type parameters from the inner defineProps
        define_props_type_params: Option<MacroTypeParams>,
        /// The defaults object (only populated when the second arg is an object literal)
        defaults: Option<MacroObjectArg<'a>>,
        /// Raw span of the second argument expression (any expression type:
        /// object literal, variable reference, function call, etc.)
        /// Used for `_mergeDefaults` wrapping when the type can't be resolved.
        defaults_arg_span: Option<Span>,
    },
}

impl ScriptMacro<'_> {
    /// Get the kind of this macro
    pub fn kind(&self) -> VueMacroKind {
        match self {
            Self::DefineProps { .. } => VueMacroKind::DefineProps,
            Self::DefineEmits { .. } => VueMacroKind::DefineEmits,
            Self::DefineExpose { .. } => VueMacroKind::DefineExpose,
            Self::DefineOptions { .. } => VueMacroKind::DefineOptions,
            Self::DefineModel { .. } => VueMacroKind::DefineModel,
            Self::DefineSlots { .. } => VueMacroKind::DefineSlots,
            Self::WithDefaults { .. } => VueMacroKind::WithDefaults,
        }
    }

    /// Get the span of this macro call
    pub fn span(&self) -> Span {
        match self {
            Self::DefineProps { span, .. }
            | Self::DefineEmits { span, .. }
            | Self::DefineExpose { span, .. }
            | Self::DefineOptions { span, .. }
            | Self::DefineModel { span, .. }
            | Self::DefineSlots { span, .. }
            | Self::WithDefaults { span, .. } => *span,
        }
    }
}

/// Check if a byte slice is a Vue macro name and return the kind.
///
/// Uses first-byte dispatch for O(1) rejection of non-macros.
#[inline]
pub fn detect_macro_kind(name: &[u8]) -> Option<VueMacroKind> {
    let len = name.len();

    // Macros range from 11-14 characters
    // defineProps (11), defineEmits (11), defineSlots (11), defineModel (11)
    // defineExpose (12), withDefaults (12)
    // defineOptions (13)
    if !(11..=13).contains(&len) {
        return None;
    }

    // All Vue macros start with 'd' or 'w'
    match name.first()? {
        b'd' => match len {
            11 => {
                if name == b"defineProps" {
                    Some(VueMacroKind::DefineProps)
                } else if name == b"defineEmits" {
                    Some(VueMacroKind::DefineEmits)
                } else if name == b"defineSlots" {
                    Some(VueMacroKind::DefineSlots)
                } else if name == b"defineModel" {
                    Some(VueMacroKind::DefineModel)
                } else {
                    None
                }
            }
            12 => {
                if name == b"defineExpose" {
                    Some(VueMacroKind::DefineExpose)
                } else {
                    None
                }
            }
            13 => {
                if name == b"defineOptions" {
                    Some(VueMacroKind::DefineOptions)
                } else {
                    None
                }
            }
            _ => None,
        },
        b'w' => {
            if len == 12 && name == b"withDefaults" {
                Some(VueMacroKind::WithDefaults)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if a byte slice is "defineComponent"
#[inline]
pub fn is_define_component(name: &[u8]) -> bool {
    name.len() == 15 && name == b"defineComponent"
}

/// Macro name byte literals for comparison
pub mod names {
    pub const DEFINE_PROPS: &[u8] = b"defineProps";
    pub const DEFINE_EMITS: &[u8] = b"defineEmits";
    pub const DEFINE_EXPOSE: &[u8] = b"defineExpose";
    pub const DEFINE_OPTIONS: &[u8] = b"defineOptions";
    pub const DEFINE_MODEL: &[u8] = b"defineModel";
    pub const DEFINE_SLOTS: &[u8] = b"defineSlots";
    pub const WITH_DEFAULTS: &[u8] = b"withDefaults";
    pub const DEFINE_COMPONENT: &[u8] = b"defineComponent";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_macros() {
        assert_eq!(
            detect_macro_kind(b"defineProps"),
            Some(VueMacroKind::DefineProps)
        );
        assert_eq!(
            detect_macro_kind(b"defineEmits"),
            Some(VueMacroKind::DefineEmits)
        );
        assert_eq!(
            detect_macro_kind(b"defineExpose"),
            Some(VueMacroKind::DefineExpose)
        );
        assert_eq!(
            detect_macro_kind(b"defineOptions"),
            Some(VueMacroKind::DefineOptions)
        );
        assert_eq!(
            detect_macro_kind(b"defineModel"),
            Some(VueMacroKind::DefineModel)
        );
        assert_eq!(
            detect_macro_kind(b"defineSlots"),
            Some(VueMacroKind::DefineSlots)
        );
        assert_eq!(
            detect_macro_kind(b"withDefaults"),
            Some(VueMacroKind::WithDefaults)
        );
    }

    #[test]
    fn test_non_macros() {
        assert_eq!(detect_macro_kind(b"ref"), None);
        assert_eq!(detect_macro_kind(b"computed"), None);
        assert_eq!(detect_macro_kind(b"defineComponent"), None); // Not a macro
        assert_eq!(detect_macro_kind(b"define"), None);
        assert_eq!(detect_macro_kind(b"definePropsExtra"), None); // Too long
    }

    #[test]
    fn test_is_define_component() {
        assert!(is_define_component(b"defineComponent"));
        assert!(!is_define_component(b"defineProps"));
        assert!(!is_define_component(b"define"));
    }
}
