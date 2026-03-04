//! Template analysis types for Vue SFC templates.
//!
//! These types are populated by `verter_core` during compilation (as raw data),
//! then converted by `verter_host` into these analysis types. They enable:
//! - Cross-file render tree construction
//! - Prop constness optimization
//! - LSP features (references, rename, document highlights)
//! - Linter rules (unused components, accessibility, etc.)

use crate::types::ResolvedTypeInfo;
use verter_span::Span;

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

    /// All CSS variable names set in template inline styles (static + dynamic, deduped).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub css_var_names: Vec<String>,
}

// =============================================================================
// Component Usage
// =============================================================================

/// A component usage in a template with prop details.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateComponentUsage {
    /// Component tag name (PascalCase normalized).
    pub name: String,
    /// Import source path if resolved from script imports (None for globals/unresolved).
    pub import_source: Option<String>,
    /// Whether this is a dynamic component (`<component :is="...">`).
    pub is_dynamic: bool,
    /// Props passed to this component.
    pub props: Vec<TemplatePropUsage>,
    /// Whether `v-bind="obj"` spread was used.
    pub has_spread: bool,
    /// Slots used on this component (`<template #slotName>`).
    pub slots_used: Vec<String>,
    /// Static class names from `class="foo bar"`.
    pub static_classes: Vec<String>,
    /// Whether `:class="..."` is present.
    pub has_dynamic_class: bool,
    /// Class names extracted from `:class` object syntax (e.g., `{ 'foo': cond }` → `["foo"]`).
    /// These are conditional — the component may or may not receive these classes at runtime.
    pub dynamic_classes: Vec<String>,
    /// v-model directives used on this component.
    pub v_models: Vec<TemplateComponentVModel>,
    /// Byte span in SFC source.
    pub span: Span,
}

/// A v-model directive used on a component in a template.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateComponentVModel {
    /// The model property name (e.g., `"title"` for `v-model:title`, `"modelValue"` for `v-model`).
    pub binding_name: String,
    /// Byte span in SFC source.
    pub span: Span,
}

/// A single prop passed to a component in a template.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplatePropUsage {
    /// Prop name (camelCase normalized from kebab-case).
    pub name: String,
    /// Whether this is a bound prop (`:prop` vs `prop`).
    pub is_bound: bool,
    /// Constness classification of the expression.
    pub constness: PropValueConstness,
    /// Bindings referenced in the prop expression.
    pub referenced_bindings: Vec<String>,
    /// If from v-bind spread, which object binding.
    pub from_spread: bool,
    /// Byte span in SFC source.
    pub span: Span,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateBindingOccurrence {
    /// The binding name (matches a script `AnalyzedBinding.name`).
    pub name: String,
    /// Byte span in SFC source.
    pub span: Span,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedBinding {
    /// The binding name that couldn't be resolved.
    pub name: String,
    /// Byte span in SFC source.
    pub span: Span,
}

// =============================================================================
// Slots
// =============================================================================

/// A slot defined in this component's template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedSlot {
    /// Slot name (`"default"`, `"header"`, etc.).
    pub name: String,
    /// Whether this is a scoped slot with bindings.
    pub has_bindings: bool,
    /// Prop names from `:prop` bindings on the `<slot>` element.
    pub binding_names: Vec<String>,
    /// Byte span in SFC source.
    pub span: Span,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateEventHandler {
    /// Event name (`"click"`, `"input"`, etc.).
    pub event_name: String,
    /// Script binding name if simple handler.
    pub handler_binding: Option<String>,
    /// Whether this is an inline expression (`@click="count++"` vs `@click="handleClick"`).
    pub is_inline: bool,
    /// The tag name of the element this handler is on.
    pub target_tag: String,
    /// Byte span in SFC source.
    pub span: Span,
}

// =============================================================================
// Directives
// =============================================================================

/// Full directive analysis for linter rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDirective {
    /// Directive name (`"if"`, `"for"`, `"bind"`, `"on"`, `"model"`, `"show"`, `"html"`, `"slot"`).
    pub name: String,
    /// Raw directive name as written (`"@click"`, `":class"`, `"v-for"`).
    pub raw_name: String,
    /// Directive argument (e.g., `"click"` in `@click`).
    pub argument: Option<String>,
    /// Directive modifiers (e.g., `["prevent"]` in `@click.prevent`).
    pub modifiers: Vec<String>,
    /// Expression value.
    pub expression: Option<String>,
    /// Byte span in SFC source.
    pub span: Span,
    /// Byte offset end of the directive name.
    pub name_end: u32,
    /// Argument span (e.g., `click` in `@click`).
    pub arg_span: Option<Span>,
    /// Inner expression/value span (excludes quotes).
    pub expression_span: Option<Span>,
    /// Modifier spans.
    pub modifier_spans: Vec<Span>,
}

/// v-for analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VForDirective {
    /// Iterator variable: `"item"` from `v-for="item in items"`.
    pub variable: String,
    /// Index variable: `"i"` from `(item, i) in items`.
    pub index: Option<String>,
    /// Iterable expression: `"items"`.
    pub iterable: String,
    /// Whether `:key` is present.
    pub has_key: bool,
    /// Key expression if present.
    pub key_expression: Option<String>,
    /// Whether the key expression uses the index variable (common mistake).
    pub key_uses_index: bool,
    /// Byte span in SFC source.
    pub span: Span,
}

/// v-model analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VModelDirective {
    /// Binding name (`"modelValue"` or custom argument).
    pub binding_name: String,
    /// Modifiers: `"lazy"`, `"number"`, `"trim"`, custom.
    pub modifiers: Vec<String>,
    /// Whether the target is a component (vs native element).
    pub target_is_component: bool,
    /// The element/component tag name.
    pub target_tag: String,
    /// Byte span in SFC source.
    pub span: Span,
}

// =============================================================================
// Elements
// =============================================================================

/// A text or interpolation segment within an element's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateTextSegment {
    /// Literal text (e.g., `"Count: "`).
    Text { span: Span, is_entity: bool },
    /// Interpolation expression (e.g., `{{ count }}`).
    Interpolation {
        /// Full span including `{{ }}` delimiters.
        span: Span,
        /// Inner expression span (excludes `{{ }}`).
        expression_span: Span,
    },
}

/// Element-level analysis for accessibility and HTML conformance.
#[derive(Debug, Clone, Default, PartialEq)]
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
    pub v_for: Option<VForDirective>,
    /// v-model directive info (if present).
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
    /// Whether this element has non-whitespace text or interpolation children.
    /// Used by a11y rules to detect content (e.g., `<h1>text</h1>` has text content).
    pub has_text_content: bool,
    /// Whether this element has non-whitespace literal text children (NOT interpolation).
    /// Used by `no-bare-strings-in-template` to distinguish hardcoded strings from `{{ expr }}`.
    pub has_bare_text: bool,
    /// Whether this element has direct child elements (non-text, non-comment children).
    /// Used by `no-child-content` rule to detect children alongside v-html/v-text.
    pub has_element_children: bool,
    /// Nesting depth of this element in the template tree.
    pub nesting_depth: u16,
    /// Parent tag name (None for root elements).
    pub parent_tag: Option<String>,
    /// Index of the parent element in the `elements` vec. `None` for root elements.
    pub parent_index: Option<u32>,
    /// Class names extracted from `:class` object syntax (e.g., `{ 'foo': cond }` → `["foo"]`).
    /// These are conditional — the element may or may not have these classes at runtime.
    pub dynamic_classes: Vec<String>,
    /// Byte span in SFC source.
    pub span: Span,
    /// Byte offset end of the opening tag only (`>` after attributes).
    /// Use this for diagnostic squiggles — highlights just `<div class="x">`, not the whole element.
    pub tag_span_end: u32,
    /// Byte offset of the `<` in the closing tag (or same as `tag_span_end` for self-closing).
    pub content_end: u32,
    /// Ordered text + interpolation children (excludes element/comment children).
    /// Used by code actions (extract bare text) and i18n rules.
    pub text_children: Vec<TemplateTextSegment>,
    /// CSS variables set via `:style` binding (e.g., `{ '--color': val }`).
    pub dynamic_style_vars: Vec<DynamicStyleVar>,
    /// CSS variables set via static `style` attribute (e.g., `style="--color: red"`).
    pub static_style_vars: Vec<StaticStyleVar>,
}

impl TemplateElement {
    /// Iterate over static class names from `class="foo bar"`.
    pub fn static_classes(&self) -> impl Iterator<Item = &str> {
        self.attributes
            .iter()
            .filter(|a| !a.is_dynamic && a.name == "class")
            .flat_map(|a| a.value.as_deref().unwrap_or("").split_whitespace())
    }

    /// Get the static `id` attribute value if present.
    pub fn static_id(&self) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| !a.is_dynamic && a.name == "id")
            .and_then(|a| a.value.as_deref())
    }
}

/// Extract class names from a `:class` binding expression (object syntax).
///
/// Handles common patterns:
/// - `{ 'my-class': condition }` → `["my-class"]`
/// - `{ active: isActive, 'text-bold': isBold }` → `["active", "text-bold"]`
/// - `[{ foo: bar }, 'static']` → `["foo"]` (extracts from objects in arrays)
///
/// Returns empty vec for unparseable expressions (ternary, function calls, variables).
pub fn extract_dynamic_class_names(expr: &str) -> Vec<String> {
    extract_dynamic_class_names_rich(expr)
        .into_iter()
        .filter(|dcn| !dcn.is_partial)
        .map(|dcn| dcn.name)
        .collect()
}

/// Rich dynamic class name with offset and metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicClassName {
    /// The extracted class name (or prefix if partial).
    pub name: String,
    /// Byte offset within the expression where the class name text starts.
    pub expr_offset: u32,
    /// Whether this is conditional (vs always applied).
    pub is_conditional: bool,
    /// Whether this is a partial prefix from a template literal.
    pub is_partial: bool,
}

/// Extract dynamic class names with rich metadata from a `:class` expression.
///
/// Handles:
/// - Object syntax: `{ 'my-class': cond }` → class names from keys
/// - Array syntax: `['foo', { bar: cond }]` → string literals + object keys
/// - Ternary: `cond ? 'active' : 'inactive'` → both branch values
/// - Logical: `cond && 'active'` → the string literal
/// - Template literal keys: `` { `test-${foo}`: cond } `` → partial prefix
pub fn extract_dynamic_class_names_rich(expr: &str) -> Vec<DynamicClassName> {
    let trimmed = expr.trim();
    let leading_ws = expr.len() - expr.trim_start().len();
    if trimmed.starts_with('{') {
        extract_object_class_keys_rich(trimmed, leading_ws)
    } else if trimmed.starts_with('[') {
        extract_array_class_keys_rich(trimmed, leading_ws)
    } else {
        // Ternary / logical expression at top level
        extract_string_literals_from_expr(trimmed, leading_ws)
    }
}

/// Extract rich class name info from object syntax `{ 'foo': cond, bar: cond2 }`.
fn extract_object_class_keys_rich(expr: &str, base_offset: usize) -> Vec<DynamicClassName> {
    let inner = expr.trim();
    let brace_start = inner.find('{').unwrap_or(0);
    let inner_content = &inner[brace_start + 1..];
    let inner_content = inner_content.strip_suffix('}').unwrap_or(inner_content);
    let content_offset = base_offset + brace_start + 1;

    let mut results = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = inner_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b',' if depth == 0 => {
                if let Some(dcn) =
                    extract_key_from_pair_rich(&inner_content[start..i], content_offset + start)
                {
                    results.push(dcn);
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < len {
        if let Some(dcn) =
            extract_key_from_pair_rich(&inner_content[start..], content_offset + start)
        {
            results.push(dcn);
        }
    }
    results
}

/// Extract key info from a single `key: value` pair with offset tracking.
fn extract_key_from_pair_rich(pair: &str, pair_offset: usize) -> Option<DynamicClassName> {
    let trimmed = pair.trim();
    let trim_offset = pair.len() - pair.trim_start().len();
    let abs_offset = pair_offset + trim_offset;
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    let len = bytes.len();
    let mut colon_pos = None;
    while i < len {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b':' => {
                colon_pos = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let colon_pos = colon_pos?;
    let key_part = trimmed[..colon_pos].trim();
    let key_trim_offset = trimmed[..colon_pos].len() - trimmed[..colon_pos].trim_start().len();

    if key_part.starts_with('`') && key_part.ends_with('`') {
        // Template literal key — extract static prefix
        let inner = &key_part[1..key_part.len() - 1];
        if let Some(interp_start) = inner.find("${") {
            let prefix = &inner[..interp_start];
            if !prefix.is_empty() {
                return Some(DynamicClassName {
                    name: prefix.to_string(),
                    expr_offset: (abs_offset + key_trim_offset + 1) as u32, // +1 for backtick
                    is_conditional: true,
                    is_partial: true,
                });
            }
        }
        // No interpolation — treat as regular string
        let inner = &key_part[1..key_part.len() - 1];
        return Some(DynamicClassName {
            name: inner.to_string(),
            expr_offset: (abs_offset + key_trim_offset + 1) as u32,
            is_conditional: true,
            is_partial: false,
        });
    }

    if (key_part.starts_with('\'') && key_part.ends_with('\''))
        || (key_part.starts_with('"') && key_part.ends_with('"'))
    {
        let name = key_part[1..key_part.len() - 1].to_string();
        Some(DynamicClassName {
            name,
            expr_offset: (abs_offset + key_trim_offset + 1) as u32, // +1 for opening quote
            is_conditional: true,
            is_partial: false,
        })
    } else if key_part
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'$')
        && !key_part.is_empty()
    {
        Some(DynamicClassName {
            name: key_part.to_string(),
            expr_offset: (abs_offset + key_trim_offset) as u32,
            is_conditional: true,
            is_partial: false,
        })
    } else {
        None
    }
}

/// Extract rich class names from array syntax `['foo', { bar: cond }, baz && 'qux']`.
fn extract_array_class_keys_rich(expr: &str, base_offset: usize) -> Vec<DynamicClassName> {
    let inner = expr.trim();
    let bracket_start = inner.find('[').unwrap_or(0);
    let inner_content = &inner[bracket_start + 1..];
    let inner_content = inner_content.strip_suffix(']').unwrap_or(inner_content);
    let content_offset = base_offset + bracket_start + 1;

    let mut results = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = inner_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => {
                depth -= 1;
            }
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b',' if depth == 0 => {
                results.extend(extract_array_element_rich(
                    &inner_content[start..i],
                    content_offset + start,
                ));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < len {
        results.extend(extract_array_element_rich(
            &inner_content[start..],
            content_offset + start,
        ));
    }
    results
}

/// Process a single array element: string literal, object, or expression with strings.
fn extract_array_element_rich(elem: &str, elem_offset: usize) -> Vec<DynamicClassName> {
    let trimmed = elem.trim();
    let trim_offset = elem.len() - elem.trim_start().len();
    let abs_offset = elem_offset + trim_offset;

    if trimmed.starts_with('{') {
        return extract_object_class_keys_rich(trimmed, abs_offset);
    }

    // Check for direct string literal: 'foo' or "foo"
    if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"'))
    {
        let name = trimmed[1..trimmed.len() - 1].to_string();
        if !name.is_empty() {
            return vec![DynamicClassName {
                name,
                expr_offset: (abs_offset + 1) as u32,
                is_conditional: false,
                is_partial: false,
            }];
        }
        return vec![];
    }

    // Expression containing string literals (ternary, logical, etc.)
    extract_string_literals_from_expr(trimmed, abs_offset)
}

/// Extract string literals from expressions like `cond ? 'active' : 'inactive'`
/// or `cond && 'active'`.
fn extract_string_literals_from_expr(expr: &str, base_offset: usize) -> Vec<DynamicClassName> {
    let mut results = Vec::new();
    let bytes = expr.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let str_start = i + 1;
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < len {
                let name = &expr[str_start..i];
                if !name.is_empty() {
                    results.push(DynamicClassName {
                        name: name.to_string(),
                        expr_offset: (base_offset + str_start) as u32,
                        is_conditional: true,
                        is_partial: false,
                    });
                }
            }
        }
        i += 1;
    }
    results
}

// Old extract_object_class_keys, extract_key_from_pair, extract_array_class_keys
// removed — all callers now use extract_dynamic_class_names_rich.

// =============================================================================
// CSS Variable Extraction from Template `:style` Bindings
// =============================================================================

/// A CSS variable set via a dynamic `:style` binding.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicStyleVar {
    /// CSS variable name (e.g. `"--color"`) or partial prefix for template literals.
    pub name: String,
    /// Byte offset within the expression where the variable name starts.
    pub expr_offset: u32,
    /// Value expression text (e.g. `"computedSize"`, `"val"`).
    pub value_expr: String,
    /// Whether this is a template literal key (partial/dynamic name).
    pub is_dynamic_key: bool,
    /// Whether this is inside a ternary or logical expression (conditional).
    pub is_conditional: bool,
}

/// A CSS variable set via a static `style` attribute.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticStyleVar {
    /// CSS variable name (e.g. `"--color"`).
    pub name: String,
    /// Value text (e.g. `"red"`).
    pub value: String,
    /// Byte offset within the attribute value where the name starts.
    pub name_offset: u32,
}

/// Extract CSS variable definitions from a dynamic `:style` expression.
///
/// Handles object syntax `{ '--color': val }` and extracts only keys starting with `--`.
/// Also handles array syntax `[{ '--a': x }, { '--b': y }]`.
pub fn extract_dynamic_style_vars(expr: &str) -> Vec<DynamicStyleVar> {
    let trimmed = expr.trim();
    if trimmed.starts_with('{') {
        extract_style_vars_from_object(trimmed, 0)
    } else if trimmed.starts_with('[') {
        extract_style_vars_from_array(trimmed)
    } else {
        Vec::new()
    }
}

fn extract_style_vars_from_object(expr: &str, _base_offset: usize) -> Vec<DynamicStyleVar> {
    let inner = expr.trim();
    let brace_start = inner.find('{').unwrap_or(0);
    let inner_content = &inner[brace_start + 1..];
    let inner_content = inner_content.strip_suffix('}').unwrap_or(inner_content);
    let content_offset = brace_start + 1;

    let mut results = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = inner_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b',' if depth == 0 => {
                extract_style_var_from_pair(
                    &inner_content[start..i],
                    content_offset + start,
                    &mut results,
                );
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < len {
        extract_style_var_from_pair(
            &inner_content[start..],
            content_offset + start,
            &mut results,
        );
    }

    results
}

fn extract_style_var_from_pair(pair: &str, offset: usize, out: &mut Vec<DynamicStyleVar>) {
    let pair = pair.trim();
    if pair.is_empty() {
        return;
    }

    // Find the colon separator (key: value), accounting for nested colons
    let bytes = pair.as_bytes();
    let mut depth = 0i32;
    let mut colon_pos = None;
    let mut i = 0;
    let len = bytes.len();
    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b':' if depth == 0 => {
                colon_pos = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }

    let Some(colon) = colon_pos else {
        return;
    };

    let key_text = pair[..colon].trim();
    let value_text = pair[colon + 1..].trim();

    // Extract the key — must start with --
    let (var_name, is_dynamic_key, key_offset) =
        if key_text.starts_with('\'') || key_text.starts_with('"') {
            // Quoted key
            let inner = &key_text[1..key_text.len().saturating_sub(1)];
            if inner.starts_with("--") {
                (inner.to_string(), false, offset + 1)
            } else {
                return;
            }
        } else if key_text.starts_with('`') {
            // Template literal key
            let inner = &key_text[1..key_text.len().saturating_sub(1)];
            if inner.starts_with("--") {
                // Extract the static prefix before ${
                let prefix = if let Some(dollar_pos) = inner.find("${") {
                    &inner[..dollar_pos]
                } else {
                    inner
                };
                (prefix.to_string(), inner.contains("${"), offset + 1)
            } else {
                return;
            }
        } else {
            // Unquoted identifier — not a CSS variable key (CSS vars start with --)
            return;
        };

    out.push(DynamicStyleVar {
        name: var_name,
        expr_offset: key_offset as u32,
        value_expr: value_text.to_string(),
        is_dynamic_key,
        is_conditional: false,
    });
}

fn extract_style_vars_from_array(expr: &str) -> Vec<DynamicStyleVar> {
    let inner = expr.trim();
    let bracket_start = inner.find('[').unwrap_or(0);
    let inner_content = &inner[bracket_start + 1..];
    let inner_content = inner_content.strip_suffix(']').unwrap_or(inner_content);

    let mut results = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = inner_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b',' if depth == 0 => {
                let element = inner_content[start..i].trim();
                if element.starts_with('{') {
                    results.extend(extract_style_vars_from_object(element, 0));
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < len {
        let element = inner_content[start..].trim();
        if element.starts_with('{') {
            results.extend(extract_style_vars_from_object(element, 0));
        }
    }

    results
}

/// Extract CSS variable definitions from a static `style` attribute value.
///
/// Parses `"--color: red; --size: 10px; color: blue"` and extracts only `--*` declarations.
pub fn extract_static_style_vars(style_value: &str) -> Vec<StaticStyleVar> {
    let mut results = Vec::new();

    for decl in style_value.split(';') {
        let decl = decl.trim();
        if !decl.starts_with("--") {
            continue;
        }
        if let Some(colon_pos) = decl.find(':') {
            let name = decl[..colon_pos].trim();
            let value = decl[colon_pos + 1..].trim();
            if name.starts_with("--") {
                let name_offset = (name.as_ptr() as usize - style_value.as_ptr() as usize) as u32;
                results.push(StaticStyleVar {
                    name: name.to_string(),
                    value: value.to_string(),
                    name_offset,
                });
            }
        }
    }

    results
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateAttribute {
    /// Attribute name.
    pub name: String,
    /// Attribute value (None if boolean attribute like `disabled`).
    pub value: Option<String>,
    /// Whether this is a dynamic attribute (`:attr` vs `attr`).
    pub is_dynamic: bool,
    /// Byte span in SFC source.
    pub span: Span,
    /// Byte offset end of the attribute name.
    pub name_end: u32,
    /// Inner value span (excludes quotes). `None` for boolean attributes.
    pub value_span: Option<Span>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedPropDefinition {
    /// Prop name.
    pub name: String,
    /// TypeScript type annotation.
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
    /// Byte span in SFC source.
    pub span: Span,
}

/// Emit analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedEmitDefinition {
    /// Event name.
    pub event_name: String,
    /// Whether this emit has a validator function.
    pub has_validator: bool,
    /// Whether this emit is declared in defineEmits (vs ad-hoc `emit()`).
    pub is_declared: bool,
    /// Locations where this event is actually emitted: `(span_start, span_end)`.
    pub emit_locations: Vec<(u32, u32)>,
    /// Byte span in SFC source.
    pub span: Span,
}

// =============================================================================
// Comment Directives
// =============================================================================

/// A comment directive for linter control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentDirective {
    /// Directive kind.
    pub kind: CommentDirectiveKind,
    /// Optional message or rule name.
    pub message: Option<String>,
    /// Byte span in SFC source.
    pub span: Span,
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
    /// `@verter:level(warn|error|off)` — override severity for the next line.
    /// The `message` field contains `"warn"`, `"error"`, or `"off"`.
    Level,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeMismatch {
    /// Byte span in SFC source.
    pub span: Span,
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
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzedMacroUsage {
    /// Which macro was called.
    pub kind: MacroKind,
    /// Whether this macro uses type-based syntax.
    pub is_type_based: bool,
    /// Type parameter content (e.g., the `{ msg: string }` in `defineProps<{ msg: string }>()`).
    pub type_param: Option<String>,
    /// Runtime argument content (e.g., the `['click']` in `defineEmits(['click'])`).
    pub runtime_arg: Option<String>,
    /// Binding name if assigned (e.g., `props` in `const props = defineProps()`).
    pub binding_name: Option<String>,
    /// For defineProps: extracted prop definitions.
    pub props: Option<Vec<AnalyzedPropDefinition>>,
    /// For defineEmits: extracted emit definitions.
    pub emits: Option<Vec<AnalyzedEmitDefinition>>,
    /// For defineModel: model name + type.
    pub model_name: Option<String>,
    /// For defineSlots: slot definitions.
    pub slots: Option<Vec<DefinedSlot>>,
    /// For defineExpose: exposed bindings.
    pub exposed: Option<Vec<String>>,
    /// For withDefaults: default values per prop.
    pub defaults: Option<rustc_hash::FxHashMap<String, String>>,
    /// Type references for cross-file resolution.
    pub type_references: Vec<String>,
    /// TODO(type-provider): Enhanced type info from TSGO.
    pub type_enhancement: Option<ResolvedTypeInfo>,
    /// Byte span in SFC source.
    pub span: Span,
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

// =============================================================================
// Custom Serialize/Deserialize impls (preserves spanStart/spanEnd JSON keys)
// =============================================================================

impl serde::Serialize for TemplateComponentUsage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", &self.name)?;
        if self.import_source.is_some() {
            map.serialize_entry("importSource", &self.import_source)?;
        }
        map.serialize_entry("isDynamic", &self.is_dynamic)?;
        if !self.props.is_empty() {
            map.serialize_entry("props", &self.props)?;
        }
        map.serialize_entry("hasSpread", &self.has_spread)?;
        if !self.slots_used.is_empty() {
            map.serialize_entry("slotsUsed", &self.slots_used)?;
        }
        if !self.static_classes.is_empty() {
            map.serialize_entry("staticClasses", &self.static_classes)?;
        }
        map.serialize_entry("hasDynamicClass", &self.has_dynamic_class)?;
        if !self.dynamic_classes.is_empty() {
            map.serialize_entry("dynamicClasses", &self.dynamic_classes)?;
        }
        if !self.v_models.is_empty() {
            map.serialize_entry("vModels", &self.v_models)?;
        }
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for TemplateComponentUsage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            import_source: Option<String>,
            #[serde(default)]
            is_dynamic: bool,
            #[serde(default)]
            props: Vec<TemplatePropUsage>,
            #[serde(default)]
            has_spread: bool,
            #[serde(default)]
            slots_used: Vec<String>,
            #[serde(default)]
            static_classes: Vec<String>,
            #[serde(default)]
            has_dynamic_class: bool,
            #[serde(default)]
            dynamic_classes: Vec<String>,
            #[serde(default)]
            v_models: Vec<TemplateComponentVModel>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            import_source: w.import_source,
            is_dynamic: w.is_dynamic,
            props: w.props,
            has_spread: w.has_spread,
            slots_used: w.slots_used,
            static_classes: w.static_classes,
            has_dynamic_class: w.has_dynamic_class,
            dynamic_classes: w.dynamic_classes,
            v_models: w.v_models,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for TemplateComponentVModel {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("bindingName", &self.binding_name)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for TemplateComponentVModel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            binding_name: String,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            binding_name: w.binding_name,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for TemplatePropUsage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("isBound", &self.is_bound)?;
        map.serialize_entry("constness", &self.constness)?;
        if !self.referenced_bindings.is_empty() {
            map.serialize_entry("referencedBindings", &self.referenced_bindings)?;
        }
        map.serialize_entry("fromSpread", &self.from_spread)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for TemplatePropUsage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            is_bound: bool,
            constness: PropValueConstness,
            #[serde(default)]
            referenced_bindings: Vec<String>,
            #[serde(default)]
            from_spread: bool,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            is_bound: w.is_bound,
            constness: w.constness,
            referenced_bindings: w.referenced_bindings,
            from_spread: w.from_spread,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for TemplateBindingOccurrence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.serialize_entry("usageKind", &self.usage_kind)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for TemplateBindingOccurrence {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            usage_kind: BindingUsageKind,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            span: Span::new(w.span_start, w.span_end),
            usage_kind: w.usage_kind,
        })
    }
}

impl serde::Serialize for UnresolvedBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for UnresolvedBinding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for DefinedSlot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("hasBindings", &self.has_bindings)?;
        if !self.binding_names.is_empty() {
            map.serialize_entry("bindingNames", &self.binding_names)?;
        }
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for DefinedSlot {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            has_bindings: bool,
            #[serde(default)]
            binding_names: Vec<String>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            has_bindings: w.has_bindings,
            binding_names: w.binding_names,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for TemplateEventHandler {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("eventName", &self.event_name)?;
        if self.handler_binding.is_some() {
            map.serialize_entry("handlerBinding", &self.handler_binding)?;
        }
        map.serialize_entry("isInline", &self.is_inline)?;
        map.serialize_entry("targetTag", &self.target_tag)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for TemplateEventHandler {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            event_name: String,
            #[serde(default)]
            handler_binding: Option<String>,
            #[serde(default)]
            is_inline: bool,
            target_tag: String,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            event_name: w.event_name,
            handler_binding: w.handler_binding,
            is_inline: w.is_inline,
            target_tag: w.target_tag,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for TemplateDirective {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("rawName", &self.raw_name)?;
        if self.argument.is_some() {
            map.serialize_entry("argument", &self.argument)?;
        }
        if !self.modifiers.is_empty() {
            map.serialize_entry("modifiers", &self.modifiers)?;
        }
        if self.expression.is_some() {
            map.serialize_entry("expression", &self.expression)?;
        }
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        if self.name_end != 0 {
            map.serialize_entry("nameEnd", &self.name_end)?;
        }
        if let Some(ref arg) = self.arg_span {
            map.serialize_entry("argSpanStart", &arg.start)?;
            map.serialize_entry("argSpanEnd", &arg.end)?;
        }
        if let Some(ref expr) = self.expression_span {
            map.serialize_entry("expressionSpanStart", &expr.start)?;
            map.serialize_entry("expressionSpanEnd", &expr.end)?;
        }
        if !self.modifier_spans.is_empty() {
            map.serialize_entry("modifierSpans", &self.modifier_spans)?;
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for TemplateDirective {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            raw_name: String,
            #[serde(default)]
            argument: Option<String>,
            #[serde(default)]
            modifiers: Vec<String>,
            #[serde(default)]
            expression: Option<String>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            #[serde(default)]
            name_end: u32,
            #[serde(default)]
            arg_span_start: Option<u32>,
            #[serde(default)]
            arg_span_end: Option<u32>,
            #[serde(default)]
            expression_span_start: Option<u32>,
            #[serde(default)]
            expression_span_end: Option<u32>,
            #[serde(default)]
            modifier_spans: Vec<Span>,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            raw_name: w.raw_name,
            argument: w.argument,
            modifiers: w.modifiers,
            expression: w.expression,
            span: Span::new(w.span_start, w.span_end),
            name_end: w.name_end,
            arg_span: w
                .arg_span_start
                .zip(w.arg_span_end)
                .map(|(s, e)| Span::new(s, e)),
            expression_span: w
                .expression_span_start
                .zip(w.expression_span_end)
                .map(|(s, e)| Span::new(s, e)),
            modifier_spans: w.modifier_spans,
        })
    }
}

impl serde::Serialize for VForDirective {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("variable", &self.variable)?;
        if self.index.is_some() {
            map.serialize_entry("index", &self.index)?;
        }
        map.serialize_entry("iterable", &self.iterable)?;
        map.serialize_entry("hasKey", &self.has_key)?;
        if self.key_expression.is_some() {
            map.serialize_entry("keyExpression", &self.key_expression)?;
        }
        map.serialize_entry("keyUsesIndex", &self.key_uses_index)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for VForDirective {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            variable: String,
            #[serde(default)]
            index: Option<String>,
            iterable: String,
            #[serde(default)]
            has_key: bool,
            #[serde(default)]
            key_expression: Option<String>,
            #[serde(default)]
            key_uses_index: bool,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            variable: w.variable,
            index: w.index,
            iterable: w.iterable,
            has_key: w.has_key,
            key_expression: w.key_expression,
            key_uses_index: w.key_uses_index,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for VModelDirective {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("bindingName", &self.binding_name)?;
        if !self.modifiers.is_empty() {
            map.serialize_entry("modifiers", &self.modifiers)?;
        }
        map.serialize_entry("targetIsComponent", &self.target_is_component)?;
        map.serialize_entry("targetTag", &self.target_tag)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for VModelDirective {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            binding_name: String,
            #[serde(default)]
            modifiers: Vec<String>,
            #[serde(default)]
            target_is_component: bool,
            target_tag: String,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            binding_name: w.binding_name,
            modifiers: w.modifiers,
            target_is_component: w.target_is_component,
            target_tag: w.target_tag,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for TemplateElement {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("tag", &self.tag)?;
        map.serialize_entry("isComponent", &self.is_component)?;
        map.serialize_entry("isSelfClosing", &self.is_self_closing)?;
        map.serialize_entry("namespace", &self.namespace)?;
        if !self.attributes.is_empty() {
            map.serialize_entry("attributes", &self.attributes)?;
        }
        if !self.directives.is_empty() {
            map.serialize_entry("directives", &self.directives)?;
        }
        if self.v_for.is_some() {
            map.serialize_entry("vFor", &self.v_for)?;
        }
        if self.v_model.is_some() {
            map.serialize_entry("vModel", &self.v_model)?;
        }
        map.serialize_entry("hasVIf", &self.has_v_if)?;
        map.serialize_entry("hasVElse", &self.has_v_else)?;
        map.serialize_entry("hasVElseIf", &self.has_v_else_if)?;
        map.serialize_entry("hasVShow", &self.has_v_show)?;
        map.serialize_entry("hasVHtml", &self.has_v_html)?;
        map.serialize_entry("hasVText", &self.has_v_text)?;
        map.serialize_entry("hasTextContent", &self.has_text_content)?;
        if self.has_bare_text {
            map.serialize_entry("hasBareText", &self.has_bare_text)?;
        }
        if self.has_element_children {
            map.serialize_entry("hasElementChildren", &self.has_element_children)?;
        }
        map.serialize_entry("nestingDepth", &self.nesting_depth)?;
        if self.parent_tag.is_some() {
            map.serialize_entry("parentTag", &self.parent_tag)?;
        }
        if self.parent_index.is_some() {
            map.serialize_entry("parentIndex", &self.parent_index)?;
        }
        if !self.dynamic_classes.is_empty() {
            map.serialize_entry("dynamicClasses", &self.dynamic_classes)?;
        }
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        if self.tag_span_end != 0 {
            map.serialize_entry("tagSpanEnd", &self.tag_span_end)?;
        }
        if self.content_end != 0 {
            map.serialize_entry("contentEnd", &self.content_end)?;
        }
        if !self.dynamic_style_vars.is_empty() {
            map.serialize_entry("dynamicStyleVars", &self.dynamic_style_vars)?;
        }
        if !self.static_style_vars.is_empty() {
            map.serialize_entry("staticStyleVars", &self.static_style_vars)?;
        }
        // text_children omitted from serialization (Rust-only, not crossing FFI)
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for TemplateElement {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            tag: String,
            #[serde(default)]
            is_component: bool,
            #[serde(default)]
            is_self_closing: bool,
            #[serde(default)]
            namespace: ElementNamespace,
            #[serde(default)]
            attributes: Vec<TemplateAttribute>,
            #[serde(default)]
            directives: Vec<TemplateDirective>,
            #[serde(default)]
            v_for: Option<VForDirective>,
            #[serde(default)]
            v_model: Option<VModelDirective>,
            #[serde(default)]
            has_v_if: bool,
            #[serde(default)]
            has_v_else: bool,
            #[serde(default)]
            has_v_else_if: bool,
            #[serde(default)]
            has_v_show: bool,
            #[serde(default)]
            has_v_html: bool,
            #[serde(default)]
            has_v_text: bool,
            #[serde(default)]
            has_text_content: bool,
            #[serde(default)]
            has_bare_text: bool,
            #[serde(default)]
            has_element_children: bool,
            #[serde(default)]
            nesting_depth: u16,
            #[serde(default)]
            parent_tag: Option<String>,
            #[serde(default)]
            parent_index: Option<u32>,
            #[serde(default)]
            dynamic_classes: Vec<String>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            #[serde(default)]
            tag_span_end: u32,
            #[serde(default)]
            content_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            tag: w.tag,
            is_component: w.is_component,
            is_self_closing: w.is_self_closing,
            namespace: w.namespace,
            attributes: w.attributes,
            directives: w.directives,
            v_for: w.v_for,
            v_model: w.v_model,
            has_v_if: w.has_v_if,
            has_v_else: w.has_v_else,
            has_v_else_if: w.has_v_else_if,
            has_v_show: w.has_v_show,
            has_v_html: w.has_v_html,
            has_v_text: w.has_v_text,
            has_text_content: w.has_text_content,
            has_bare_text: w.has_bare_text,
            has_element_children: w.has_element_children,
            nesting_depth: w.nesting_depth,
            parent_tag: w.parent_tag,
            parent_index: w.parent_index,
            dynamic_classes: w.dynamic_classes,
            span: Span::new(w.span_start, w.span_end),
            tag_span_end: w.tag_span_end,
            content_end: w.content_end,
            text_children: Vec::new(), // Not deserialized — Rust-only
            dynamic_style_vars: Vec::new(),
            static_style_vars: Vec::new(),
        })
    }
}

impl serde::Serialize for TemplateAttribute {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", &self.name)?;
        if self.value.is_some() {
            map.serialize_entry("value", &self.value)?;
        }
        map.serialize_entry("isDynamic", &self.is_dynamic)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        if self.name_end != 0 {
            map.serialize_entry("nameEnd", &self.name_end)?;
        }
        if let Some(ref vs) = self.value_span {
            map.serialize_entry("valueSpanStart", &vs.start)?;
            map.serialize_entry("valueSpanEnd", &vs.end)?;
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for TemplateAttribute {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            value: Option<String>,
            #[serde(default)]
            is_dynamic: bool,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            #[serde(default)]
            name_end: u32,
            #[serde(default)]
            value_span_start: Option<u32>,
            #[serde(default)]
            value_span_end: Option<u32>,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            value: w.value,
            is_dynamic: w.is_dynamic,
            span: Span::new(w.span_start, w.span_end),
            name_end: w.name_end,
            value_span: w
                .value_span_start
                .zip(w.value_span_end)
                .map(|(s, e)| Span::new(s, e)),
        })
    }
}

impl serde::Serialize for AnalyzedPropDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", &self.name)?;
        if self.type_annotation.is_some() {
            map.serialize_entry("typeAnnotation", &self.type_annotation)?;
        }
        map.serialize_entry("hasDefault", &self.has_default)?;
        map.serialize_entry("isRequired", &self.is_required)?;
        map.serialize_entry("isBoolean", &self.is_boolean)?;
        map.serialize_entry("usedInTemplate", &self.used_in_template)?;
        map.serialize_entry("usedInScript", &self.used_in_script)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedPropDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            type_annotation: Option<String>,
            #[serde(default)]
            has_default: bool,
            #[serde(default)]
            is_required: bool,
            #[serde(default)]
            is_boolean: bool,
            #[serde(default)]
            used_in_template: bool,
            #[serde(default)]
            used_in_script: bool,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            type_annotation: w.type_annotation,
            has_default: w.has_default,
            is_required: w.is_required,
            is_boolean: w.is_boolean,
            used_in_template: w.used_in_template,
            used_in_script: w.used_in_script,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for AnalyzedEmitDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("eventName", &self.event_name)?;
        map.serialize_entry("hasValidator", &self.has_validator)?;
        map.serialize_entry("isDeclared", &self.is_declared)?;
        if !self.emit_locations.is_empty() {
            map.serialize_entry("emitLocations", &self.emit_locations)?;
        }
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedEmitDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            event_name: String,
            #[serde(default)]
            has_validator: bool,
            #[serde(default)]
            is_declared: bool,
            #[serde(default)]
            emit_locations: Vec<(u32, u32)>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            event_name: w.event_name,
            has_validator: w.has_validator,
            is_declared: w.is_declared,
            emit_locations: w.emit_locations,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for CommentDirective {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("kind", &self.kind)?;
        if self.message.is_some() {
            map.serialize_entry("message", &self.message)?;
        }
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.serialize_entry("affectsNextLine", &self.affects_next_line)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for CommentDirective {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            kind: CommentDirectiveKind,
            #[serde(default)]
            message: Option<String>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            #[serde(default)]
            affects_next_line: bool,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            kind: w.kind,
            message: w.message,
            span: Span::new(w.span_start, w.span_end),
            affects_next_line: w.affects_next_line,
        })
    }
}

impl serde::Serialize for TypeMismatch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.serialize_entry("expected", &self.expected)?;
        map.serialize_entry("actual", &self.actual)?;
        map.serialize_entry("message", &self.message)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for TypeMismatch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            expected: String,
            actual: String,
            message: String,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            span: Span::new(w.span_start, w.span_end),
            expected: w.expected,
            actual: w.actual,
            message: w.message,
        })
    }
}

impl serde::Serialize for AnalyzedMacroUsage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("isTypeBased", &self.is_type_based)?;
        if self.type_param.is_some() {
            map.serialize_entry("typeParam", &self.type_param)?;
        }
        if self.runtime_arg.is_some() {
            map.serialize_entry("runtimeArg", &self.runtime_arg)?;
        }
        if self.binding_name.is_some() {
            map.serialize_entry("bindingName", &self.binding_name)?;
        }
        if self.props.is_some() {
            map.serialize_entry("props", &self.props)?;
        }
        if self.emits.is_some() {
            map.serialize_entry("emits", &self.emits)?;
        }
        if self.model_name.is_some() {
            map.serialize_entry("modelName", &self.model_name)?;
        }
        if self.slots.is_some() {
            map.serialize_entry("slots", &self.slots)?;
        }
        if self.exposed.is_some() {
            map.serialize_entry("exposed", &self.exposed)?;
        }
        if self.defaults.is_some() {
            map.serialize_entry("defaults", &self.defaults)?;
        }
        if !self.type_references.is_empty() {
            map.serialize_entry("typeReferences", &self.type_references)?;
        }
        if self.type_enhancement.is_some() {
            map.serialize_entry("typeEnhancement", &self.type_enhancement)?;
        }
        map.serialize_entry("spanStart", &self.span.start)?;
        map.serialize_entry("spanEnd", &self.span.end)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedMacroUsage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            kind: MacroKind,
            #[serde(default)]
            is_type_based: bool,
            #[serde(default)]
            type_param: Option<String>,
            #[serde(default)]
            runtime_arg: Option<String>,
            #[serde(default)]
            binding_name: Option<String>,
            #[serde(default)]
            props: Option<Vec<AnalyzedPropDefinition>>,
            #[serde(default)]
            emits: Option<Vec<AnalyzedEmitDefinition>>,
            #[serde(default)]
            model_name: Option<String>,
            #[serde(default)]
            slots: Option<Vec<DefinedSlot>>,
            #[serde(default)]
            exposed: Option<Vec<String>>,
            #[serde(default)]
            defaults: Option<rustc_hash::FxHashMap<String, String>>,
            #[serde(default)]
            type_references: Vec<String>,
            #[serde(default)]
            type_enhancement: Option<ResolvedTypeInfo>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            kind: w.kind,
            is_type_based: w.is_type_based,
            type_param: w.type_param,
            runtime_arg: w.runtime_arg,
            binding_name: w.binding_name,
            props: w.props,
            emits: w.emits,
            model_name: w.model_name,
            slots: w.slots,
            exposed: w.exposed,
            defaults: w.defaults,
            type_references: w.type_references,
            type_enhancement: w.type_enhancement,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

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
                    span: Span::new(0, 0),
                }],
                has_spread: false,
                slots_used: vec!["default".to_string()],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                span: Span::new(10, 50),
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "count".to_string(),
                span: Span::new(20, 25),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            unresolved_bindings: vec![UnresolvedBinding {
                name: "unknown".to_string(),
                span: Span::new(30, 37),
            }],
            defined_slots: vec![DefinedSlot {
                name: "header".to_string(),
                has_bindings: true,
                binding_names: vec![],
                span: Span::new(0, 0),
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
                target_tag: "div".to_string(),
                span: Span::new(0, 0),
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
                span: Span::new(0, 20),
                name_end: 0,
                value_span: None,
            }],
            directives: vec![TemplateDirective {
                name: "if".to_string(),
                raw_name: "v-if".to_string(),
                argument: None,
                modifiers: vec![],
                expression: Some("visible".to_string()),
                span: Span::new(21, 35),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            }],
            v_for: None,
            v_model: None,
            has_v_if: true,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 1,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            text_children: Vec::new(),
            dynamic_style_vars: Vec::new(),
            static_style_vars: Vec::new(),
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
            span: Span::new(0, 30),
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
            span: Span::new(0, 25),
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
            span: Span::new(0, 40),
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
                span: Span::new(5, 16),
            }]),
            emits: None,
            model_name: None,
            slots: None,
            exposed: None,
            defaults: None,
            type_references: vec!["Props".to_string()],
            type_enhancement: None,
            span: Span::new(0, 50),
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
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            span: Span::new(0, 30),
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
            span: Span::new(0, 0),
        };

        assert!(prop.from_spread);
        assert_eq!(prop.constness, PropValueConstness::Unknown);
    }

    #[test]
    fn extract_dynamic_classes_object_syntax() {
        let result = extract_dynamic_class_names("{ 'my-class': isFoo, active: isActive }");
        assert_eq!(result, vec!["my-class", "active"]);
    }

    #[test]
    fn extract_dynamic_classes_quoted_keys() {
        let result = extract_dynamic_class_names(r#"{ "text-bold": isBold, 'text-red': isRed }"#);
        assert_eq!(result, vec!["text-bold", "text-red"]);
    }

    #[test]
    fn extract_dynamic_classes_bare_identifiers() {
        let result = extract_dynamic_class_names("{ active: isActive, disabled: isDisabled }");
        assert_eq!(result, vec!["active", "disabled"]);
    }

    #[test]
    fn extract_dynamic_classes_array_with_objects() {
        let result = extract_dynamic_class_names("[{ foo: bar }, 'static']");
        assert_eq!(result, vec!["foo", "static"]);
    }

    #[test]
    fn extract_dynamic_classes_variable_returns_empty() {
        let result = extract_dynamic_class_names("myClasses");
        assert!(result.is_empty());
    }

    #[test]
    fn extract_dynamic_classes_ternary() {
        let result = extract_dynamic_class_names("isActive ? 'active' : 'inactive'");
        assert_eq!(result, vec!["active", "inactive"]);
    }

    #[test]
    fn extract_dynamic_classes_nested_value() {
        // The value expression can be complex but we only care about keys
        let result = extract_dynamic_class_names(
            "{ highlighted: items.length > 0, 'fade-in': show && ready }",
        );
        assert_eq!(result, vec!["highlighted", "fade-in"]);
    }

    #[test]
    fn extract_dynamic_classes_empty_object() {
        let result = extract_dynamic_class_names("{}");
        assert!(result.is_empty());
    }

    // =========================================================================
    // Rich dynamic class extraction tests (A0b)
    // =========================================================================

    /// @ai-generated - Array string literals extracted with offsets
    #[test]
    fn extract_rich_array_string_literals() {
        let result = extract_dynamic_class_names_rich("['foo', isLoading && 'bar']");
        let names: Vec<&str> = result.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["foo", "bar"]);
        assert!(!result[0].is_partial);
        assert!(!result[0].is_conditional); // direct string literal
        assert!(result[1].is_conditional); // conditional from &&
    }

    /// @ai-generated - Ternary expressions extract both branches
    #[test]
    fn extract_rich_ternary() {
        let result = extract_dynamic_class_names_rich("isActive ? 'active' : 'inactive'");
        let names: Vec<&str> = result.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["active", "inactive"]);
        assert!(result.iter().all(|d| d.is_conditional));
        assert!(result.iter().all(|d| !d.is_partial));
    }

    /// @ai-generated - Template literal key extracts partial prefix
    #[test]
    fn extract_rich_template_literal_prefix() {
        let result = extract_dynamic_class_names_rich("{ `test-${foo}`: cond }");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "test-");
        assert!(result[0].is_partial);
        assert!(result[0].is_conditional);
    }

    /// @ai-generated - Mixed array with objects, strings, and logical expressions
    #[test]
    fn extract_rich_mixed_array() {
        let result = extract_dynamic_class_names_rich("['foo', { bar: cond }, baz && 'qux']");
        let names: Vec<&str> = result.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["foo", "bar", "qux"]);
    }

    /// @ai-generated - Backward compat: extract_dynamic_class_names delegates to rich
    #[test]
    fn extract_dynamic_classes_backward_compat() {
        // Object syntax still works
        let result = extract_dynamic_class_names("{ active: cond }");
        assert_eq!(result, vec!["active"]);
        // Ternary now works via rich path
        let result = extract_dynamic_class_names("cond ? 'a' : 'b'");
        assert_eq!(result, vec!["a", "b"]);
        // Partial prefixes are filtered out
        let result = extract_dynamic_class_names("{ `test-${foo}`: cond }");
        assert!(result.is_empty());
    }

    /// @ai-generated - expr_offset values are correct for object keys
    #[test]
    fn extract_rich_offsets_correct() {
        // "{ 'bar': cond }" — 'bar' starts at offset 3 (after "{ '")
        let expr = "{ 'bar': cond }";
        let result = extract_dynamic_class_names_rich(expr);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "bar");
        let offset = result[0].expr_offset as usize;
        assert_eq!(
            &expr[offset..offset + 3],
            "bar",
            "offset should point to 'bar' text"
        );
    }

    // ===== CSS Variable Extraction Tests =====

    /// @ai-generated - extract_dynamic_style_vars parses object with CSS variable keys
    #[test]
    fn extract_dynamic_style_vars_object() {
        let vars = extract_dynamic_style_vars("{ '--color': val, '--size': computedSize }");
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "--color");
        assert_eq!(vars[0].value_expr, "val");
        assert!(!vars[0].is_dynamic_key);
        assert_eq!(vars[1].name, "--size");
        assert_eq!(vars[1].value_expr, "computedSize");
    }

    /// @ai-generated - extract_dynamic_style_vars ignores non-CSS-variable keys
    #[test]
    fn extract_dynamic_style_vars_filters_non_css_vars() {
        let vars = extract_dynamic_style_vars("{ 'color': 'red', '--custom': val }");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].name, "--custom");
    }

    /// @ai-generated - extract_dynamic_style_vars handles template literal keys
    #[test]
    fn extract_dynamic_style_vars_template_literal() {
        let vars = extract_dynamic_style_vars("{ `--${prefix}--color`: val }");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].name, "--");
        assert!(vars[0].is_dynamic_key);
    }

    /// @ai-generated - extract_dynamic_style_vars handles array syntax
    #[test]
    fn extract_dynamic_style_vars_array() {
        let vars = extract_dynamic_style_vars("[{ '--a': x }, { '--b': y }]");
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "--a");
        assert_eq!(vars[1].name, "--b");
    }

    /// @ai-generated - extract_dynamic_style_vars returns empty for non-object/array
    #[test]
    fn extract_dynamic_style_vars_non_object() {
        let vars = extract_dynamic_style_vars("someVariable");
        assert!(vars.is_empty());
    }

    /// @ai-generated - extract_static_style_vars parses CSS variable declarations
    #[test]
    fn extract_static_style_vars_basic() {
        let vars = extract_static_style_vars("--color: red; --size: 10px; color: blue");
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "--color");
        assert_eq!(vars[0].value, "red");
        assert_eq!(vars[1].name, "--size");
        assert_eq!(vars[1].value, "10px");
    }

    /// @ai-generated - extract_static_style_vars returns empty for no CSS vars
    #[test]
    fn extract_static_style_vars_no_vars() {
        let vars = extract_static_style_vars("color: red; font-size: 14px");
        assert!(vars.is_empty());
    }

    /// @ai-generated - extract_static_style_vars handles only CSS vars
    #[test]
    fn extract_static_style_vars_only_vars() {
        let vars = extract_static_style_vars("--x: 1; --y: 2");
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "--x");
        assert_eq!(vars[0].value, "1");
        assert_eq!(vars[1].name, "--y");
        assert_eq!(vars[1].value, "2");
    }
}
