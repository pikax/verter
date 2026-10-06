//! Owned template analysis records: components, bindings, slots, refs, events and
//! directives of a template.

use crate::analysis::types::ResolvedTypeInfo;
use verter_span::Span;

/// Complete template analysis for an SFC.
/// Populated after compilation by converting raw template data from `verter_compiler`.
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

    /// Declared `defineSlots` members with resolved usage (outlet present or
    /// programmatic access). Populated only when usage can be statically
    /// bounded — fail-open (empty) otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_declarations: Vec<AnalyzedSlotDeclaration>,

    /// True when any template expression failed to parse — the identifier
    /// inventory is then incomplete and usage-driven diagnostics fail open.
    #[serde(default)]
    pub has_expression_errors: bool,

    /// Diagnostics produced by the template-facts extraction pass itself
    /// (template expression parse errors, e.g. `XInvalidExpression` for a
    /// malformed directive/interpolation expression). The full form of
    /// [`Self::has_expression_errors`]: every route that serves this
    /// snapshot carries the same set, so diagnostic completeness never
    /// depends on which consumer requested the facts. The compile route
    /// ALSO publishes these on its own diagnostics channel (deduplicated
    /// there) — a publisher that already surfaces that channel must not
    /// re-surface this field on the same route.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expression_diagnostics: Vec<TemplateExpressionDiagnostic>,

    /// Static member reads on identifier roots inside template expressions
    /// (`props.title`, `$slots.header`). A root consumed by a member read is
    /// NOT a whole-object escape; matched against occurrences by root span.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub member_reads: Vec<TemplateMemberRead>,

    /// A `<slot :name="expr">` dynamic outlet exists — the outlet set cannot
    /// be statically bounded (suppresses unused-slot diagnostics).
    #[serde(default)]
    pub has_dynamic_slot_outlet: bool,

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

    /// Svelte `{#snippet name(params)}` declarations in this component's
    /// template (empty for Vue). Powers `{@render |}` callee completion (D5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snippet_definitions: Vec<SnippetDefinition>,

    /// Svelte element directives (`use:x`, `transition:fn`, `bind:prop`, …) in
    /// this component's template (empty for Vue). Powers the D6
    /// directive-keyword doc hovers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub svelte_directives: Vec<SvelteDirectiveInfo>,

    /// Parsed typed value-expression records referenced by template usage
    /// locators. Internal semantic IR is rebuilt with the compiler snapshot;
    /// the public template-analysis wire remains source-analysis only.
    #[serde(skip)]
    pub expression_records: Vec<TemplateExpressionRecord>,
}

/// Severity of a [`TemplateExpressionDiagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemplateDiagnosticSeverity {
    /// A hard error.
    Error,
    /// A non-fatal warning.
    Warning,
    /// Informational.
    Info,
}

/// One diagnostic emitted by the template-facts extraction pass
/// (a template expression parse error), carried on
/// [`TemplateAnalysisSnapshot::expression_diagnostics`].
///
/// Serialized with the module's flat-span wire convention
/// (`spanStart`/`spanEnd`, no nested `span` object) via the manual impls
/// below.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateExpressionDiagnostic {
    /// Severity.
    pub severity: TemplateDiagnosticSeverity,
    /// The compiler-defined code string (e.g. `XInvalidExpression`).
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Carrier-absolute source span.
    pub span: Span,
}

/// Index into [`TemplateAnalysisSnapshot::expression_records`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TemplateExpressionLocator(pub u32);

/// One parsed typed template-expression record.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateExpressionRecord {
    pub span: Span,
    pub expression: verter_type_expr::IndexedValueExpression,
}

impl TemplateAnalysisSnapshot {
    #[must_use]
    pub fn expression(
        &self,
        locator: TemplateExpressionLocator,
    ) -> Option<&verter_type_expr::IndexedValueExpression> {
        self.expression_records
            .get(locator.0 as usize)
            .map(|record| &record.expression)
    }

    #[must_use]
    pub fn expression_at_span(
        &self,
        span: Span,
    ) -> Option<&verter_type_expr::IndexedValueExpression> {
        self.expression_records
            .iter()
            .find(|record| record.span == span)
            .map(|record| &record.expression)
    }
}

/// A Svelte `{#snippet name(params)}` declaration (typed template IR).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SnippetDefinition {
    /// Snippet name (`"row"`, `"header"`).
    pub name: String,
    /// SFC-absolute byte span of the snippet name.
    pub span: Span,
    /// The `(params)` text without the parens, when present (e.g. `"item: number"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_text: Option<String>,
}

/// A Svelte element directive attribute (`use:action`, `transition:fn`,
/// `bind:prop`, `class:name`, `style:prop`, `on:event`, `let:item`, …) as
/// typed template IR. Powers the D6 directive-keyword doc hovers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SvelteDirectiveInfo {
    /// The keyword (`use`, `transition`, `in`, `out`, `animate`, `bind`,
    /// `class`, `style`, `on`, `let`, or an unrecognised prefix verbatim).
    pub keyword: String,
    /// The local name (the part after the `:`, before any `|modifier`).
    pub local: String,
    /// The full attribute span.
    pub span: Span,
    /// Byte offset end of the keyword (before the `:`).
    pub keyword_end: u32,
    /// The local name span.
    pub local_span: Span,
    /// The value expression span, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_span: Option<Span>,
}

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
    /// Framework-neutral two-way bindings passed to this component (the Svelte
    /// `bind:` family). Empty for Vue (two-way bindings are carried in
    /// `v_models`).
    pub bindings: Vec<TemplateComponentBinding>,
    /// Framework-neutral events listened on this component (the legacy Svelte
    /// `on:` directive only — a plain `on*` attribute is a prop, never an
    /// event). Empty for Vue.
    pub events: Vec<TemplateComponentEvent>,
    /// Byte span in SFC source.
    pub span: Span,
}

/// A two-way binding passed to a child component (the Svelte `bind:` family).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateComponentBinding {
    /// The bound local member name (`value` in `bind:value`).
    pub name: String,
    /// The `|modifier` list, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
    /// Byte span in SFC source.
    #[serde(default)]
    pub span: Span,
}

/// An event listened on a child component via the legacy Svelte `on:`
/// directive. A plain `on*` attribute is a prop, never an event (the
/// props/events split is syntactic — the child component-meta, not a name
/// guess, decides which passed props are callback events).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateComponentEvent {
    /// The event name — the legacy directive local (`click` from `on:click`).
    pub name: String,
    /// The handler expression text, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler_expression: Option<String>,
    /// Whether the handler is an inline function expression.
    pub is_inline: bool,
    /// The `|modifier` list, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
    /// Byte span in SFC source.
    #[serde(default)]
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
    /// Raw expression/value text when available.
    pub expression: Option<String>,
    /// Parsed typed expression record; `None` for static/absent values.
    pub expression_locator: Option<TemplateExpressionLocator>,
    /// Constness classification of the expression.
    pub constness: PropValueConstness,
    /// Bindings referenced in the prop expression.
    pub referenced_bindings: Vec<String>,
    /// If from v-bind spread, which object binding.
    pub from_spread: bool,
    /// Byte span in SFC source.
    pub span: Span,
    /// Byte span of just the prop name in SFC source.
    pub name_span: Span,
    /// True when this is a same-name shorthand (`:bar` with no expression).
    pub is_shorthand: bool,
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

/// A slot defined in this component's template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedSlot {
    /// Slot name (`"default"`, `"header"`, etc.).
    pub name: String,
    /// Whether this is a scoped slot with bindings.
    pub has_bindings: bool,
    /// Prop names from `:prop` bindings on the `<slot>` element.
    pub binding_names: Vec<String>,
    /// Expression text for each binding (parallel to `binding_names`).
    /// E.g. for `:item="row"`, the expression is `"row"`.
    pub binding_expressions: Vec<String>,
    /// SFC-absolute spans of each binding's value expression (parallel to `binding_names`).
    pub binding_value_spans: Vec<Span>,
    /// Whether the `<slot>` element has fallback (default) content children.
    pub has_fallback_content: bool,
    /// Byte span in SFC source.
    pub span: Span,
}

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
    /// The v-if or v-else-if condition expression text (e.g., `"show"`, `"mode === 'dark'"`).
    /// `None` for v-else or when no condition directive is present.
    pub v_if_condition: Option<String>,
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
    /// Stable link to `TemplateComponentUsage` for component elements.
    /// Index into `TemplateAnalysisSnapshot.components`. `None` for native elements.
    pub component_usage_index: Option<u32>,
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

/// A resolvable class-name token in carrier markup — the typed usage fact for
/// carriers WITHOUT a template element IR (Svelte). Each token names one class
/// with the exact byte span of the authored name (a `class="a b"` value yields
/// one token per name; a `class:x` directive yields one directive token).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkupClassToken {
    /// The class name.
    pub name: String,
    /// Carrier-absolute byte span of the authored name.
    pub span: Span,
    /// `true` for a `class:x` directive token, `false` for a `class="x"` entry.
    pub from_directive: bool,
}

/// A v-if/v-else-if chain for duplicate condition detection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IfChain {
    /// Condition expressions with their spans: `(expression, span_start, span_end)`.
    pub conditions: Vec<(String, u32, u32)>,
}

/// A static member read on an identifier root inside a template expression.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMemberRead {
    /// The root identifier (`props` in `props.title`).
    pub root: String,
    /// The literal member name.
    pub member: String,
    /// File-absolute span of the root identifier (matches the corresponding
    /// binding occurrence span).
    pub root_span: verter_span::Span,
}

/// A declared `defineSlots` member with resolved usage for the linter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedSlotDeclaration {
    /// Slot name (`"default"`, `"header"`, …).
    pub name: String,
    /// SFC-absolute byte span of the slot name in the `defineSlots` declaration.
    pub span: verter_span::Span,
    /// Whether an outlet (`<slot>` / `<slot name="x">`, conditional included)
    /// or a bounded programmatic access uses this slot.
    pub used: bool,
}

/// Props analysis enriched for linter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedPropDefinition {
    /// Prop name.
    pub name: String,
    /// Typed callable role established by semantic identity.
    pub callable_role: verter_type_expr::PropCallableRole,
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

impl From<MacroKind> for crate::facts::registry::MacroKind {
    fn from(value: MacroKind) -> Self {
        use MacroKind as TemplateMacroKind;
        match value {
            TemplateMacroKind::DefineProps => Self::DefineProps,
            TemplateMacroKind::DefineEmits => Self::DefineEmits,
            TemplateMacroKind::DefineModel => Self::DefineModel,
            TemplateMacroKind::DefineSlots => Self::DefineSlots,
            TemplateMacroKind::DefineExpose => Self::DefineExpose,
            TemplateMacroKind::DefineOptions => Self::DefineOptions,
            TemplateMacroKind::WithDefaults => Self::WithDefaults,
        }
    }
}

impl serde::Serialize for TemplateComponentUsage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TemplateComponentUsage", 6)?;
        s.serialize_field("name", &self.name)?;
        if self.import_source.is_some() {
            s.serialize_field("importSource", &self.import_source)?;
        }
        s.serialize_field("isDynamic", &self.is_dynamic)?;
        if !self.props.is_empty() {
            s.serialize_field("props", &self.props)?;
        }
        s.serialize_field("hasSpread", &self.has_spread)?;
        if !self.slots_used.is_empty() {
            s.serialize_field("slotsUsed", &self.slots_used)?;
        }
        if !self.static_classes.is_empty() {
            s.serialize_field("staticClasses", &self.static_classes)?;
        }
        s.serialize_field("hasDynamicClass", &self.has_dynamic_class)?;
        if !self.dynamic_classes.is_empty() {
            s.serialize_field("dynamicClasses", &self.dynamic_classes)?;
        }
        if !self.v_models.is_empty() {
            s.serialize_field("vModels", &self.v_models)?;
        }
        if !self.bindings.is_empty() {
            s.serialize_field("bindings", &self.bindings)?;
        }
        if !self.events.is_empty() {
            s.serialize_field("events", &self.events)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
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
            bindings: Vec<TemplateComponentBinding>,
            #[serde(default)]
            events: Vec<TemplateComponentEvent>,
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
            bindings: w.bindings,
            events: w.events,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for TemplateExpressionDiagnostic {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TemplateExpressionDiagnostic", 5)?;
        s.serialize_field("severity", &self.severity)?;
        s.serialize_field("code", &self.code)?;
        s.serialize_field("message", &self.message)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for TemplateExpressionDiagnostic {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            severity: TemplateDiagnosticSeverity,
            code: String,
            message: String,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            severity: w.severity,
            code: w.code,
            message: w.message,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for TemplateComponentVModel {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TemplateComponentVModel", 3)?;
        s.serialize_field("bindingName", &self.binding_name)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TemplatePropUsage", 5)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("isBound", &self.is_bound)?;
        if self.expression.is_some() {
            s.serialize_field("expression", &self.expression)?;
        }
        s.serialize_field("constness", &self.constness)?;
        if !self.referenced_bindings.is_empty() {
            s.serialize_field("referencedBindings", &self.referenced_bindings)?;
        }
        s.serialize_field("fromSpread", &self.from_spread)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.name_span.start > 0 || self.name_span.end > 0 {
            s.serialize_field("nameSpanStart", &self.name_span.start)?;
            s.serialize_field("nameSpanEnd", &self.name_span.end)?;
        }
        if self.is_shorthand {
            s.serialize_field("isShorthand", &true)?;
        }
        s.end()
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
            #[serde(default)]
            expression: Option<String>,
            constness: PropValueConstness,
            #[serde(default)]
            referenced_bindings: Vec<String>,
            #[serde(default)]
            from_spread: bool,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            #[serde(default)]
            name_span_start: u32,
            #[serde(default)]
            name_span_end: u32,
            #[serde(default)]
            is_shorthand: bool,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            is_bound: w.is_bound,
            expression: w.expression,
            // Indexed expression records are retained compiler/session IR and
            // are intentionally absent from the public analysis wire.
            expression_locator: None,
            constness: w.constness,
            referenced_bindings: w.referenced_bindings,
            from_spread: w.from_spread,
            span: Span::new(w.span_start, w.span_end),
            name_span: Span::new(w.name_span_start, w.name_span_end),
            is_shorthand: w.is_shorthand,
        })
    }
}

impl serde::Serialize for TemplateBindingOccurrence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TemplateBindingOccurrence", 4)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.serialize_field("usageKind", &self.usage_kind)?;
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("UnresolvedBinding", 3)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("DefinedSlot", 4)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("hasBindings", &self.has_bindings)?;
        if !self.binding_names.is_empty() {
            s.serialize_field("bindingNames", &self.binding_names)?;
        }
        if !self.binding_expressions.is_empty() {
            s.serialize_field("bindingExpressions", &self.binding_expressions)?;
        }
        // binding_value_spans are SFC-absolute and not serialized (internal use only)
        if self.has_fallback_content {
            s.serialize_field("hasFallbackContent", &self.has_fallback_content)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
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
            binding_expressions: Vec<String>,
            #[serde(default)]
            has_fallback_content: bool,
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
            binding_expressions: w.binding_expressions,
            binding_value_spans: vec![],
            has_fallback_content: w.has_fallback_content,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

impl serde::Serialize for TemplateEventHandler {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TemplateEventHandler", 5)?;
        s.serialize_field("eventName", &self.event_name)?;
        if self.handler_binding.is_some() {
            s.serialize_field("handlerBinding", &self.handler_binding)?;
        }
        s.serialize_field("isInline", &self.is_inline)?;
        s.serialize_field("targetTag", &self.target_tag)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TemplateDirective", 4)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("rawName", &self.raw_name)?;
        if self.argument.is_some() {
            s.serialize_field("argument", &self.argument)?;
        }
        if !self.modifiers.is_empty() {
            s.serialize_field("modifiers", &self.modifiers)?;
        }
        if self.expression.is_some() {
            s.serialize_field("expression", &self.expression)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.name_end != 0 {
            s.serialize_field("nameEnd", &self.name_end)?;
        }
        if let Some(ref arg) = self.arg_span {
            s.serialize_field("argSpanStart", &arg.start)?;
            s.serialize_field("argSpanEnd", &arg.end)?;
        }
        if let Some(ref expr) = self.expression_span {
            s.serialize_field("expressionSpanStart", &expr.start)?;
            s.serialize_field("expressionSpanEnd", &expr.end)?;
        }
        if !self.modifier_spans.is_empty() {
            s.serialize_field("modifierSpans", &self.modifier_spans)?;
        }
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("VForDirective", 5)?;
        s.serialize_field("variable", &self.variable)?;
        if self.index.is_some() {
            s.serialize_field("index", &self.index)?;
        }
        s.serialize_field("iterable", &self.iterable)?;
        s.serialize_field("hasKey", &self.has_key)?;
        if self.key_expression.is_some() {
            s.serialize_field("keyExpression", &self.key_expression)?;
        }
        s.serialize_field("keyUsesIndex", &self.key_uses_index)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("VModelDirective", 4)?;
        s.serialize_field("bindingName", &self.binding_name)?;
        if !self.modifiers.is_empty() {
            s.serialize_field("modifiers", &self.modifiers)?;
        }
        s.serialize_field("targetIsComponent", &self.target_is_component)?;
        s.serialize_field("targetTag", &self.target_tag)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TemplateElement", 10)?;
        s.serialize_field("tag", &self.tag)?;
        s.serialize_field("isComponent", &self.is_component)?;
        s.serialize_field("isSelfClosing", &self.is_self_closing)?;
        s.serialize_field("namespace", &self.namespace)?;
        if !self.attributes.is_empty() {
            s.serialize_field("attributes", &self.attributes)?;
        }
        if !self.directives.is_empty() {
            s.serialize_field("directives", &self.directives)?;
        }
        if self.v_for.is_some() {
            s.serialize_field("vFor", &self.v_for)?;
        }
        if self.v_model.is_some() {
            s.serialize_field("vModel", &self.v_model)?;
        }
        s.serialize_field("hasVIf", &self.has_v_if)?;
        s.serialize_field("hasVElse", &self.has_v_else)?;
        s.serialize_field("hasVElseIf", &self.has_v_else_if)?;
        if self.v_if_condition.is_some() {
            s.serialize_field("vIfCondition", &self.v_if_condition)?;
        }
        s.serialize_field("hasVShow", &self.has_v_show)?;
        s.serialize_field("hasVHtml", &self.has_v_html)?;
        s.serialize_field("hasVText", &self.has_v_text)?;
        s.serialize_field("hasTextContent", &self.has_text_content)?;
        if self.has_bare_text {
            s.serialize_field("hasBareText", &self.has_bare_text)?;
        }
        if self.has_element_children {
            s.serialize_field("hasElementChildren", &self.has_element_children)?;
        }
        s.serialize_field("nestingDepth", &self.nesting_depth)?;
        if self.parent_tag.is_some() {
            s.serialize_field("parentTag", &self.parent_tag)?;
        }
        if self.parent_index.is_some() {
            s.serialize_field("parentIndex", &self.parent_index)?;
        }
        if !self.dynamic_classes.is_empty() {
            s.serialize_field("dynamicClasses", &self.dynamic_classes)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.tag_span_end != 0 {
            s.serialize_field("tagSpanEnd", &self.tag_span_end)?;
        }
        if self.content_end != 0 {
            s.serialize_field("contentEnd", &self.content_end)?;
        }
        if !self.dynamic_style_vars.is_empty() {
            s.serialize_field("dynamicStyleVars", &self.dynamic_style_vars)?;
        }
        if !self.static_style_vars.is_empty() {
            s.serialize_field("staticStyleVars", &self.static_style_vars)?;
        }
        // text_children omitted from serialization (Rust-only, not crossing FFI)
        s.end()
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
            v_if_condition: Option<String>,
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
            v_if_condition: w.v_if_condition,
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
            component_usage_index: None, // Populated later by host
        })
    }
}

impl serde::Serialize for TemplateAttribute {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TemplateAttribute", 4)?;
        s.serialize_field("name", &self.name)?;
        if self.value.is_some() {
            s.serialize_field("value", &self.value)?;
        }
        s.serialize_field("isDynamic", &self.is_dynamic)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.name_end != 0 {
            s.serialize_field("nameEnd", &self.name_end)?;
        }
        if let Some(ref vs) = self.value_span {
            s.serialize_field("valueSpanStart", &vs.start)?;
            s.serialize_field("valueSpanEnd", &vs.end)?;
        }
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AnalyzedPropDefinition", 10)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("callableRole", &self.callable_role)?;
        if self.type_annotation.is_some() {
            s.serialize_field("typeAnnotation", &self.type_annotation)?;
        }
        s.serialize_field("hasDefault", &self.has_default)?;
        s.serialize_field("isRequired", &self.is_required)?;
        s.serialize_field("isBoolean", &self.is_boolean)?;
        s.serialize_field("usedInTemplate", &self.used_in_template)?;
        s.serialize_field("usedInScript", &self.used_in_script)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedPropDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            callable_role: verter_type_expr::PropCallableRole,
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
            callable_role: w.callable_role,
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AnalyzedEmitDefinition", 5)?;
        s.serialize_field("eventName", &self.event_name)?;
        s.serialize_field("hasValidator", &self.has_validator)?;
        s.serialize_field("isDeclared", &self.is_declared)?;
        if !self.emit_locations.is_empty() {
            s.serialize_field("emitLocations", &self.emit_locations)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("CommentDirective", 4)?;
        s.serialize_field("kind", &self.kind)?;
        if self.message.is_some() {
            s.serialize_field("message", &self.message)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.serialize_field("affectsNextLine", &self.affects_next_line)?;
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("TypeMismatch", 5)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.serialize_field("expected", &self.expected)?;
        s.serialize_field("actual", &self.actual)?;
        s.serialize_field("message", &self.message)?;
        s.end()
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
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AnalyzedMacroUsage", 4)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("isTypeBased", &self.is_type_based)?;
        if self.type_param.is_some() {
            s.serialize_field("typeParam", &self.type_param)?;
        }
        if self.runtime_arg.is_some() {
            s.serialize_field("runtimeArg", &self.runtime_arg)?;
        }
        if self.binding_name.is_some() {
            s.serialize_field("bindingName", &self.binding_name)?;
        }
        if self.props.is_some() {
            s.serialize_field("props", &self.props)?;
        }
        if self.emits.is_some() {
            s.serialize_field("emits", &self.emits)?;
        }
        if self.model_name.is_some() {
            s.serialize_field("modelName", &self.model_name)?;
        }
        if self.slots.is_some() {
            s.serialize_field("slots", &self.slots)?;
        }
        if self.exposed.is_some() {
            s.serialize_field("exposed", &self.exposed)?;
        }
        if self.defaults.is_some() {
            s.serialize_field("defaults", &self.defaults)?;
        }
        if !self.type_references.is_empty() {
            s.serialize_field("typeReferences", &self.type_references)?;
        }
        if self.type_enhancement.is_some() {
            s.serialize_field("typeEnhancement", &self.type_enhancement)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
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
