//! Template analysis types for Vue SFC templates.
//!
//! These types are populated by `verter_core` during compilation (as raw data),
//! then converted by `verter_host` into these analysis types. They enable:
//! - Cross-file render tree construction
//! - Prop constness optimization
//! - LSP features (references, rename, document highlights)
//! - Linter rules (unused components, accessibility, etc.)

use crate::types::ResolvedTypeInfo;

// =============================================================================
// Core Template Analysis Snapshot
// =============================================================================

/// Complete template analysis for an SFC.
/// Populated after compilation by converting raw template data from `verter_core`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateAnalysisSnapshot {
    /// Components used in the template.
    pub components: Vec<TemplateComponentUsage>,

    /// Script bindings actually referenced in template expressions, with positions.
    /// Each occurrence records the binding name + byte offset in the SFC source.
    /// Enables textDocument/references, rename, and documentHighlight.
    pub binding_occurrences: Vec<TemplateBindingOccurrence>,

    /// Bindings referenced in template but not found in script (unresolved).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_bindings: Vec<UnresolvedBinding>,

    /// Slots defined in this component's template (`<slot>` elements).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defined_slots: Vec<DefinedSlot>,

    /// Template refs (`ref="foo"` attributes).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub template_refs: Vec<TemplateRef>,

    /// Event handlers used (`@click`, `@input`, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_handlers: Vec<TemplateEventHandler>,

    /// Full element tree for linter traversal (all elements, not just components).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<TemplateElement>,

    /// v-if/v-else-if chain conditions for dupe detection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub if_chains: Vec<IfChain>,

    /// Template nesting depth (for max-depth rule).
    #[serde(default)]
    pub max_nesting_depth: u16,

    /// v-if + v-for conflicts (same element), stored as (span_start, span_end) pairs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub v_if_v_for_conflicts: Vec<(u32, u32)>,

    /// Prop definitions (from defineProps analysis).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prop_definitions: Vec<AnalyzedPropDefinition>,

    /// Emit definitions (from defineEmits analysis).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emit_definitions: Vec<AnalyzedEmitDefinition>,

    /// Comment directives (`@verter:disable`, `@verter:todo`, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comment_directives: Vec<CommentDirective>,

    /// TODO(type-provider): Enhanced type info populated by TSGO when connected.
    /// Contains resolved types for template expressions, slot bindings, etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_enhancements: Option<TemplateTypeEnhancements>,
}

// =============================================================================
// Component Usage
// =============================================================================

/// A component usage in a template with prop details.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateComponentUsage {
    /// Component tag name (PascalCase normalized).
    pub name: String,
    /// Import source path if resolved from script imports (None for globals/unresolved).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_source: Option<String>,
    /// Whether this is a dynamic component (`<component :is="...">`).
    pub is_dynamic: bool,
    /// Props passed to this component.
    pub props: Vec<TemplatePropUsage>,
    /// Whether `v-bind="obj"` spread was used.
    pub has_spread: bool,
    /// Slots used on this component (`<template #slotName>`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots_used: Vec<String>,
    /// Byte offset of component tag start in the SFC source.
    pub span_start: u32,
    /// Byte offset of component tag end in the SFC source.
    pub span_end: u32,
}

/// A single prop passed to a component in a template.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePropUsage {
    /// Prop name (camelCase normalized from kebab-case).
    pub name: String,
    /// Whether this is a bound prop (`:prop` vs `prop`).
    pub is_bound: bool,
    /// Constness classification of the expression.
    pub constness: PropValueConstness,
    /// Bindings referenced in the prop expression.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referenced_bindings: Vec<String>,
    /// If from v-bind spread, which object binding.
    pub from_spread: bool,
}

/// How a prop value expression is classified at a call site.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum PropValueConstness {
    /// Compile-time constant: string literal, number, boolean, const binding.
    Const,
    /// Potentially reactive: ref, computed, reactive, function call.
    Dynamic,
    /// Cannot be analyzed (expression parse error, spread, etc.).
    #[default]
    Unknown,
}

// =============================================================================
// Binding Occurrences
// =============================================================================

/// A script binding referenced at a specific position in the template.
/// Used for references, rename, and document highlights.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateBindingOccurrence {
    /// The binding name (matches a script `AnalyzedBinding.name`).
    pub name: String,
    /// Byte offset in the SFC source where this occurrence starts.
    pub span_start: u32,
    /// Byte offset in the SFC source where this occurrence ends.
    pub span_end: u32,
    /// What kind of usage: interpolation, directive value, event handler, component tag.
    pub usage_kind: BindingUsageKind,
}

/// Classification of how a binding is used in a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BindingUsageKind {
    /// `{{ binding }}` -- text interpolation.
    Interpolation,
    /// `:prop="binding"` -- directive value.
    DirectiveValue,
    /// `@click="binding"` -- event handler.
    EventHandler,
    /// `<Binding />` -- component tag name.
    ComponentTag,
    /// `ref="binding"` -- template ref (if dynamic `:ref`).
    TemplateRef,
    /// `v-for="item in binding"` -- iterator source.
    IteratorSource,
}

/// An unresolved binding with its position.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedBinding {
    /// The binding name that couldn't be resolved.
    pub name: String,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
}

// =============================================================================
// Slots
// =============================================================================

/// A slot defined in this component's template.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinedSlot {
    /// Slot name (`"default"`, `"header"`, etc.).
    pub name: String,
    /// Whether this is a scoped slot with bindings.
    pub has_bindings: bool,
}

// =============================================================================
// Template Refs
// =============================================================================

/// A template ref attribute.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateRef {
    /// Ref name: `ref="foo"` -> `"foo"`.
    pub name: String,
    /// Whether this is dynamic: `:ref="expr"`.
    pub is_dynamic: bool,
    /// The element/component tag this ref is on.
    pub target_tag: String,
}

// =============================================================================
// Event Handlers
// =============================================================================

/// An event handler in the template.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateEventHandler {
    /// Event name (`"click"`, `"input"`, etc.).
    pub event_name: String,
    /// Script binding name if simple handler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler_binding: Option<String>,
    /// Whether this is an inline expression (`@click="count++"` vs `@click="handleClick"`).
    pub is_inline: bool,
}

// =============================================================================
// Directives
// =============================================================================

/// Full directive analysis for linter rules.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateDirective {
    /// Directive name (`"if"`, `"for"`, `"bind"`, `"on"`, `"model"`, `"show"`, `"html"`, `"slot"`).
    pub name: String,
    /// Raw directive name as written (`"@click"`, `":class"`, `"v-for"`).
    pub raw_name: String,
    /// Directive argument (e.g., `"click"` in `@click`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument: Option<String>,
    /// Directive modifiers (e.g., `["prevent"]` in `@click.prevent`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
    /// Expression value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
}

/// v-for analysis.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VForDirective {
    /// Iterator variable: `"item"` from `v-for="item in items"`.
    pub variable: String,
    /// Index variable: `"i"` from `(item, i) in items`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
    /// Iterable expression: `"items"`.
    pub iterable: String,
    /// Whether `:key` is present.
    pub has_key: bool,
    /// Key expression if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_expression: Option<String>,
    /// Whether the key expression uses the index variable (common mistake).
    pub key_uses_index: bool,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
}

/// v-model analysis.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VModelDirective {
    /// Binding name (`"modelValue"` or custom argument).
    pub binding_name: String,
    /// Modifiers: `"lazy"`, `"number"`, `"trim"`, custom.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
    /// Whether the target is a component (vs native element).
    pub target_is_component: bool,
    /// The element/component tag name.
    pub target_tag: String,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
}

// =============================================================================
// Elements
// =============================================================================

/// Element-level analysis for accessibility and HTML conformance.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateElement {
    /// Tag name.
    pub tag: String,
    /// Whether this is a component (vs native HTML element).
    pub is_component: bool,
    /// Whether this is self-closing.
    pub is_self_closing: bool,
    /// Element namespace.
    pub namespace: ElementNamespace,
    /// Static and dynamic attributes.
    pub attributes: Vec<TemplateAttribute>,
    /// Directives on this element.
    pub directives: Vec<TemplateDirective>,
    /// v-for directive info (if present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v_for: Option<VForDirective>,
    /// v-model directive info (if present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v_model: Option<VModelDirective>,
    /// Whether v-if is present.
    pub has_v_if: bool,
    /// Whether v-else is present.
    pub has_v_else: bool,
    /// Whether v-else-if is present.
    pub has_v_else_if: bool,
    /// Whether v-show is present.
    pub has_v_show: bool,
    /// Whether v-html is present (security: XSS risk).
    pub has_v_html: bool,
    /// Whether v-text is present.
    pub has_v_text: bool,
    /// Nesting depth of this element in the template tree.
    pub nesting_depth: u16,
    /// Parent tag name (None for root elements).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tag: Option<String>,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
}

/// Element namespace.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum ElementNamespace {
    #[default]
    Html,
    Svg,
    MathML,
}

/// A template attribute (static or dynamic).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateAttribute {
    /// Attribute name.
    pub name: String,
    /// Attribute value (None if boolean attribute like `disabled`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Whether this is a dynamic attribute (`:attr` vs `attr`).
    pub is_dynamic: bool,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
}

// =============================================================================
// If Chains
// =============================================================================

/// A v-if/v-else-if chain for duplicate condition detection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IfChain {
    /// Condition expressions with their spans: `(expression, span_start, span_end)`.
    pub conditions: Vec<(String, u32, u32)>,
}

// =============================================================================
// Prop & Emit Definitions
// =============================================================================

/// Props analysis enriched for linter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedPropDefinition {
    /// Prop name.
    pub name: String,
    /// TypeScript type annotation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    /// Whether this prop has a default value.
    pub has_default: bool,
    /// Whether this prop is required.
    pub is_required: bool,
    /// Whether this prop is a boolean type.
    pub is_boolean: bool,
    /// Whether this prop is used in the template.
    pub used_in_template: bool,
    /// Whether this prop is used in the script.
    pub used_in_script: bool,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
}

/// Emit analysis.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedEmitDefinition {
    /// Event name.
    pub event_name: String,
    /// Whether this emit has a validator function.
    pub has_validator: bool,
    /// Whether this emit is declared in defineEmits (vs ad-hoc `emit()`).
    pub is_declared: bool,
    /// Locations where this event is actually emitted: `(span_start, span_end)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emit_locations: Vec<(u32, u32)>,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
}

// =============================================================================
// Comment Directives
// =============================================================================

/// A comment directive for linter control.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentDirective {
    /// Directive kind.
    pub kind: CommentDirectiveKind,
    /// Optional message or rule name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
    /// Whether this directive affects the next line only.
    pub affects_next_line: bool,
}

/// Comment directive kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommentDirectiveKind {
    /// `@verter:disable rule-name`
    Disable,
    /// `@verter:disable-next-line rule-name`
    DisableNextLine,
    /// `@verter:enable rule-name`
    Enable,
    /// `@verter:todo message`
    Todo,
    /// `@verter:fixme message`
    Fixme,
    /// `@verter:deprecated message`
    Deprecated,
    /// `@verter:ignore-start`
    IgnoreStart,
    /// `@verter:ignore-end`
    IgnoreEnd,
}

// =============================================================================
// Type Enhancements (populated by external type providers)
// =============================================================================

/// TODO(type-provider): Placeholder for advanced type information from external type providers.
/// Can be populated by: TypeScript language service, TSGO, or any type checker that can
/// resolve Vue template expressions. Enables type-aware linting and LSP features
/// (typed completions, hover with full signatures, generic inference, etc.).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateTypeEnhancements {
    /// Resolved types for each binding occurrence (keyed by span_start).
    pub binding_types: rustc_hash::FxHashMap<u32, ResolvedTypeInfo>,
    /// Resolved types for slot scope bindings.
    pub slot_scope_types: rustc_hash::FxHashMap<String, ResolvedTypeInfo>,
    /// Component prop type mismatches detected by type checker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prop_type_mismatches: Vec<TypeMismatch>,
    /// Event handler parameter type info.
    pub event_param_types: rustc_hash::FxHashMap<u32, ResolvedTypeInfo>,
}

/// A type mismatch detected by the type checker.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeMismatch {
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
    /// Expected type.
    pub expected: String,
    /// Actual type.
    pub actual: String,
    /// Human-readable message.
    pub message: String,
}

// =============================================================================
// Macro Usage (enriched for linter)
// =============================================================================

/// Vue macro analysis -- rich data for each macro call.
/// Tracks defineProps, defineEmits, defineModel, defineSlots, defineExpose,
/// defineOptions, withDefaults and their type-level and runtime arguments.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedMacroUsage {
    /// Which macro was called.
    pub kind: MacroKind,
    /// Whether this macro uses type-based syntax.
    pub is_type_based: bool,
    /// Type parameter content (e.g., the `{ msg: string }` in `defineProps<{ msg: string }>()`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_param: Option<String>,
    /// Runtime argument content (e.g., the `['click']` in `defineEmits(['click'])`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_arg: Option<String>,
    /// Binding name if assigned (e.g., `props` in `const props = defineProps()`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_name: Option<String>,
    /// For defineProps: extracted prop definitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<Vec<AnalyzedPropDefinition>>,
    /// For defineEmits: extracted emit definitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emits: Option<Vec<AnalyzedEmitDefinition>>,
    /// For defineModel: model name + type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    /// For defineSlots: slot definitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<Vec<DefinedSlot>>,
    /// For defineExpose: exposed bindings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposed: Option<Vec<String>>,
    /// For withDefaults: default values per prop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<rustc_hash::FxHashMap<String, String>>,
    /// Type references for cross-file resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub type_references: Vec<String>,
    /// TODO(type-provider): Enhanced type info from TSGO.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_enhancement: Option<ResolvedTypeInfo>,
    /// Byte offset start.
    pub span_start: u32,
    /// Byte offset end.
    pub span_end: u32,
}

/// Macro kind for enriched macro usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MacroKind {
    DefineProps,
    DefineEmits,
    DefineModel,
    DefineSlots,
    DefineExpose,
    DefineOptions,
    WithDefaults,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// @ai-generated - TemplateAnalysisSnapshot default is empty
    #[test]
    fn template_snapshot_default_is_empty() {
        let snapshot = TemplateAnalysisSnapshot::default();
        assert!(snapshot.components.is_empty());
        assert!(snapshot.binding_occurrences.is_empty());
        assert!(snapshot.unresolved_bindings.is_empty());
        assert!(snapshot.defined_slots.is_empty());
        assert!(snapshot.template_refs.is_empty());
        assert!(snapshot.event_handlers.is_empty());
        assert!(snapshot.elements.is_empty());
        assert!(snapshot.if_chains.is_empty());
        assert_eq!(snapshot.max_nesting_depth, 0);
        assert!(snapshot.v_if_v_for_conflicts.is_empty());
        assert!(snapshot.type_enhancements.is_none());
    }

    /// @ai-generated - Serialization round-trip for TemplateAnalysisSnapshot
    #[test]
    fn template_snapshot_serde_roundtrip() {
        let snapshot = TemplateAnalysisSnapshot {
            components: vec![TemplateComponentUsage {
                name: "MyChild".to_string(),
                import_source: Some("./MyChild.vue".to_string()),
                is_dynamic: false,
                props: vec![TemplatePropUsage {
                    name: "msg".to_string(),
                    is_bound: false,
                    constness: PropValueConstness::Const,
                    referenced_bindings: vec![],
                    from_spread: false,
                }],
                has_spread: false,
                slots_used: vec!["default".to_string()],
                span_start: 10,
                span_end: 50,
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "count".to_string(),
                span_start: 20,
                span_end: 25,
                usage_kind: BindingUsageKind::Interpolation,
            }],
            unresolved_bindings: vec![UnresolvedBinding {
                name: "unknown".to_string(),
                span_start: 30,
                span_end: 37,
            }],
            defined_slots: vec![DefinedSlot {
                name: "header".to_string(),
                has_bindings: true,
            }],
            template_refs: vec![TemplateRef {
                name: "myEl".to_string(),
                is_dynamic: false,
                target_tag: "div".to_string(),
            }],
            event_handlers: vec![TemplateEventHandler {
                event_name: "click".to_string(),
                handler_binding: Some("handleClick".to_string()),
                is_inline: false,
            }],
            ..Default::default()
        };

        let json = serde_json::to_string(&snapshot).expect("serialize");
        let roundtrip: TemplateAnalysisSnapshot = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(snapshot, roundtrip);
    }

    /// @ai-generated - PropValueConstness default is Unknown
    #[test]
    fn prop_constness_default_is_unknown() {
        assert_eq!(PropValueConstness::default(), PropValueConstness::Unknown);
    }

    /// @ai-generated - ElementNamespace default is Html
    #[test]
    fn element_namespace_default_is_html() {
        assert_eq!(ElementNamespace::default(), ElementNamespace::Html);
    }

    /// @ai-generated - TemplateElement with directives serializes correctly
    #[test]
    fn template_element_with_directives_serde() {
        let element = TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![TemplateAttribute {
                name: "class".to_string(),
                value: Some("container".to_string()),
                is_dynamic: false,
                span_start: 0,
                span_end: 20,
            }],
            directives: vec![TemplateDirective {
                name: "if".to_string(),
                raw_name: "v-if".to_string(),
                argument: None,
                modifiers: vec![],
                expression: Some("visible".to_string()),
                span_start: 21,
                span_end: 35,
            }],
            v_for: None,
            v_model: None,
            has_v_if: true,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            nesting_depth: 1,
            parent_tag: None,
            span_start: 0,
            span_end: 50,
        };

        let json = serde_json::to_string(&element).expect("serialize");
        let roundtrip: TemplateElement = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(element, roundtrip);
    }

    /// @ai-generated - VForDirective serialization
    #[test]
    fn v_for_directive_serde() {
        let v_for = VForDirective {
            variable: "item".to_string(),
            index: Some("i".to_string()),
            iterable: "items".to_string(),
            has_key: true,
            key_expression: Some("item.id".to_string()),
            key_uses_index: false,
            span_start: 0,
            span_end: 30,
        };

        let json = serde_json::to_string(&v_for).expect("serialize");
        let roundtrip: VForDirective = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(v_for, roundtrip);
    }

    /// @ai-generated - VModelDirective serialization
    #[test]
    fn v_model_directive_serde() {
        let v_model = VModelDirective {
            binding_name: "modelValue".to_string(),
            modifiers: vec!["lazy".to_string(), "trim".to_string()],
            target_is_component: false,
            target_tag: "input".to_string(),
            span_start: 0,
            span_end: 25,
        };

        let json = serde_json::to_string(&v_model).expect("serialize");
        let roundtrip: VModelDirective = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(v_model, roundtrip);
    }

    /// @ai-generated - CommentDirective serialization
    #[test]
    fn comment_directive_serde() {
        let directive = CommentDirective {
            kind: CommentDirectiveKind::Disable,
            message: Some("no-v-html".to_string()),
            span_start: 0,
            span_end: 40,
            affects_next_line: false,
        };

        let json = serde_json::to_string(&directive).expect("serialize");
        let roundtrip: CommentDirective = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(directive, roundtrip);
    }

    /// @ai-generated - AnalyzedMacroUsage for defineProps with extracted props
    #[test]
    fn macro_usage_define_props_serde() {
        let usage = AnalyzedMacroUsage {
            kind: MacroKind::DefineProps,
            is_type_based: true,
            type_param: Some("{ msg: string; count: number }".to_string()),
            runtime_arg: None,
            binding_name: Some("props".to_string()),
            props: Some(vec![AnalyzedPropDefinition {
                name: "msg".to_string(),
                type_annotation: Some("string".to_string()),
                has_default: false,
                is_required: true,
                is_boolean: false,
                used_in_template: true,
                used_in_script: false,
                span_start: 5,
                span_end: 16,
            }]),
            emits: None,
            model_name: None,
            slots: None,
            exposed: None,
            defaults: None,
            type_references: vec!["Props".to_string()],
            type_enhancement: None,
            span_start: 0,
            span_end: 50,
        };

        let json = serde_json::to_string(&usage).expect("serialize");
        let roundtrip: AnalyzedMacroUsage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(usage, roundtrip);
    }

    /// @ai-generated - All BindingUsageKind variants serialize correctly
    #[test]
    fn binding_usage_kind_all_variants_serde() {
        let variants = vec![
            BindingUsageKind::Interpolation,
            BindingUsageKind::DirectiveValue,
            BindingUsageKind::EventHandler,
            BindingUsageKind::ComponentTag,
            BindingUsageKind::TemplateRef,
            BindingUsageKind::IteratorSource,
        ];

        for kind in variants {
            let json = serde_json::to_string(&kind).expect("serialize");
            let roundtrip: BindingUsageKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(kind, roundtrip);
        }
    }

    /// @ai-generated - All CommentDirectiveKind variants serialize correctly
    #[test]
    fn comment_directive_kind_all_variants_serde() {
        let variants = vec![
            CommentDirectiveKind::Disable,
            CommentDirectiveKind::DisableNextLine,
            CommentDirectiveKind::Enable,
            CommentDirectiveKind::Todo,
            CommentDirectiveKind::Fixme,
            CommentDirectiveKind::Deprecated,
            CommentDirectiveKind::IgnoreStart,
            CommentDirectiveKind::IgnoreEnd,
        ];

        for kind in variants {
            let json = serde_json::to_string(&kind).expect("serialize");
            let roundtrip: CommentDirectiveKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(kind, roundtrip);
        }
    }

    /// @ai-generated - Dynamic component usage
    #[test]
    fn dynamic_component_usage() {
        let component = TemplateComponentUsage {
            name: "component".to_string(),
            import_source: None,
            is_dynamic: true,
            props: vec![],
            has_spread: false,
            slots_used: vec![],
            span_start: 0,
            span_end: 30,
        };

        assert!(component.is_dynamic);
        assert!(component.import_source.is_none());
    }

    /// @ai-generated - Spread prop usage
    #[test]
    fn spread_prop_usage() {
        let prop = TemplatePropUsage {
            name: "".to_string(),
            is_bound: true,
            constness: PropValueConstness::Unknown,
            referenced_bindings: vec!["obj".to_string()],
            from_spread: true,
        };

        assert!(prop.from_spread);
        assert_eq!(prop.constness, PropValueConstness::Unknown);
    }
}
