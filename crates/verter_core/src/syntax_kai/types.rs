use crate::{
    common::Span,
    cursor::ScriptLanguage,
    tokenizer::QuoteType,
    utils::{
        oxc::{
            vue::{GenericParseResult, VForWithBindings, VSlotWithBindings},
            BindingExtractionResult,
        },
        vue::PatchFlag,
    },
};

pub const NO_PARENT: u32 = u32::MAX;

pub trait SyntaxNode {
    fn get_id(&self) -> u32;
}
// ROOT

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootNodeKind {
    Script,
    Template,
    Style,
    Unknown,
    // Add more as needed
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementKind {
    // Component tags (PascalCase or kebab-case with at least one uppercase letter)
    Component = 0,
    // HTML tags (lowercase, no uppercase letters)
    Element,
    // <slot> with v-bind or # syntax (Vue 3)
    SlotOutlet,
    // <template> (when not a slot)
    Template,
    // <component is="...">
    DynamicComponent,

    // Custom component
    CustomComponent,
}

impl ElementKind {
    /// Whether this element kind represents a Vue component (as opposed to a
    /// plain HTML element, slot outlet, or template wrapper).
    #[inline]
    pub fn is_component(&self) -> bool {
        matches!(
            self,
            ElementKind::Component | ElementKind::DynamicComponent | ElementKind::CustomComponent
        )
    }
}

#[derive(Debug, Clone)]
pub enum PropKind {
    /// Static attribute: foo="foo"
    Value,

    /// Directive - excluding built-in directives and Class/Style bindings
    /// Example: v-custom:arg.modifier="expr"
    Directive,

    // Special props
    // Class attribute: class="expr"
    ClassValue,
    // :class="expr" or v-bind:class="expr"
    ClassBind,

    // Style attribute: style="expr"
    StyleValue,
    // :style="expr" or v-bind:style="expr"
    StyleBind,

    // built-in directives
    /// v-bind: :prop="expr"
    Bind,
    /// v-bind spread: v-bind="obj" (no attribute name)
    BindSpread,
    /// v-on: @event="handler"
    On,
    /// v-on spread: v-on="obj" (no attribute name)
    OnSpread,

    /// v-model
    Model,
    /// v-show: style display toggle
    Show,
    /// v-html: innerHTML binding
    Html,
    /// v-text: textContent binding
    Text,
    /// v-if: conditional rendering
    If,
    /// v-else-if: conditional rendering
    ElseIf,
    /// v-else: conditional rendering
    Else,
    /// v-for: list rendering
    For,
    /// v-slot: template slot
    Slot,
}

#[derive(Debug, Clone)]
pub struct RootNodeOpenTagStart {
    pub kind: RootNodeKind,

    // start contains <
    pub start: u32,

    // after name, before attributes, the whitespace after the tag name
    pub name_end: u32,
}
#[derive(Debug, Clone)]
pub struct RootNodeOpenTagEnd {
    pub kind: RootNodeKind,

    // start contains <
    pub start: u32,
    // end contains >
    pub end: u32,

    // after name, before attributes, the whitespace after the tag name
    pub name_end: u32,

    pub is_self_closing: bool,
}
#[derive(Debug, Clone)]
pub struct RootNodeCloseTag {
    pub kind: RootNodeKind,

    // start contains <
    pub start: u32,
    pub end: u32,

    // after name, before attributes, the whitespace after the tag name
    pub name_end: u32,
}

#[derive(Debug, Clone)]
pub struct ScriptLang {
    pub lang: ScriptLanguage,
}

// /ROOT

// Elements

// When the tag is open but before any attributes are parsed
#[derive(Debug, Clone)]
pub struct ElementOpenTagStart {
    pub kind: ElementKind,

    // start contains <
    pub start: u32,
    // after name, before attributes, the whitespace after the tag name
    pub name_end: u32,

    pub parent_id: u32,

    pub nested_level: usize,
    pub is_void_element: bool,
    pub patch_flag: PatchFlag,

    /// Prop names that are dynamic (the `arg` spans from `:prop="expr"` bindings).
    /// Only populated when PROPS flag is set (not FULL_PROPS, since FULL_PROPS
    /// implies all props are dynamic and no list is needed).
    /// Corresponds to Vue's `dynamicProps` array in codegen output.
    pub dynamic_props: Vec<Span>,

    /// Whether this element has a `ref` attribute (static or dynamic).
    /// Used for conditional NEED_PATCH at tag close.
    pub has_ref: bool,
    /// Whether this element has a `@vnode*` lifecycle hook listener.
    /// Used for conditional NEED_PATCH at tag close.
    pub has_vnode_hook: bool,
}
impl SyntaxNode for ElementOpenTagStart {
    // offset of `</`
    fn get_id(&self) -> u32 {
        self.start
    }
}

#[derive(Debug, Clone)]
pub struct ElementOpenTagEnd {
    pub kind: ElementKind,

    // start contains <
    pub start: u32,
    // end contains >
    pub end: u32,

    // after name, before attributes, the whitespace after the tag name
    pub name_end: u32,

    pub parent_id: u32,

    pub nested_level: usize,
    pub is_void_element: bool,
    pub is_self_closing: bool,

    pub patch_flag: PatchFlag,
    pub dynamic_props: Vec<Span>,

    pub has_ref: bool,
    pub has_vnode_hook: bool,
}
impl SyntaxNode for ElementOpenTagEnd {
    // offset of `<`
    fn get_id(&self) -> u32 {
        self.start
    }
}

#[derive(Debug)]
pub struct ElementCloseTag {
    pub kind: ElementKind,

    // start contains <
    pub start: u32,
    // end contains >
    pub end: u32,

    // after name, before attributes, the whitespace after the tag name
    pub name_end: u32,

    pub parent_id: u32,

    pub nested_level: usize,
    pub is_void_element: bool,
}
impl SyntaxNode for ElementCloseTag {
    // offset of `<`
    fn get_id(&self) -> u32 {
        self.start
    }
}

// /Elements

// Scopes

#[derive(Debug, Clone)]
pub struct ElementScopeConditionIf {
    // start of the element that creates the scope.
    pub element_start: u32,

    // start contains v-
    pub start: u32,
    pub end: u32,

    pub value: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct ElementScopeConditionElseIf {
    // start of the element that creates the scope.
    pub element_start: u32,

    // start contains v-
    pub start: u32,
    pub end: u32,

    pub value: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct ElementScopeConditionElse {
    // start of the element that creates the scope.
    pub element_start: u32,

    // start contains v-
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone)]
pub struct ElementScopeFor {
    // start of the element that creates the scope.
    pub element_start: u32,

    // start contains v-
    pub start: u32,
    pub end: u32,

    // full value span
    pub value: Option<Span>,

    // left of "in" or "of"
    pub iterator: Option<Span>,
    // right of "in" or "of"
    pub iterable: Option<Span>,

    pub is_of: bool,
}

// When the Slot is in the component <MyComponent v-slot:header="slotProps">, the scope is created by the component, not the <template> element. So we associate the scope with the component element instead of the <template> element.
#[derive(Debug, Clone)]
pub struct ElementScopeSlotElement {
    // start of the element that creates the scope.
    pub element_start: u32,
    // where tag ends, contains >
    pub element_content_start: u32,

    // start contains v-
    pub start: u32,
    pub end: u32,

    pub name: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct ElementScopeSlotTemplate {
    // start of <template>.
    pub element_start: u32,

    pub start: u32,
    pub end: u32,

    pub name: Option<Span>,
}

// /Scopes

// Props

#[derive(Debug, Clone)]
pub struct Prop {
    pub kind: PropKind,

    pub element_id: u32,

    pub is_directive: bool,

    // start of the prop, including directive name if it's a directive
    pub start: u32,
    pub end: u32,

    pub name_end: u32,

    pub value: Option<Span>,
    pub arg: Option<Span>,

    pub modifiers: Option<Vec<Span>>,

    pub quote: Option<QuoteType>,
    pub has_dynamic_arg: bool,
}

// /Props

// Content

#[derive(Debug, Clone)]
pub struct Text {
    pub parent_id: u32,

    pub start: u32,
    pub end: u32,

    /// Whether this text span contains an HTML entity (e.g. `&amp;`) that needs decoding.
    pub has_entity: bool,
}

#[derive(Debug, Clone)]
pub struct Interpolation {
    pub parent_id: u32,

    // start contains {{
    pub start: u32,
    // end contains }}
    pub end: u32,

    pub content: Span,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub parent_id: u32,

    // start contains <!--
    pub start: u32,
    // end contains -->
    pub end: u32,

    pub content: Span,
}

// /Content

// OXC Parsed

#[derive(Debug)]
pub struct OxcScript<'alloc> {
    pub start: u32,
    pub end: u32,

    pub tag_open_start: u32,
    pub tag_open_end: u32,

    pub tag_close_start: u32,
    pub tag_close_end: u32,

    pub content_start: u32,
    pub content_end: u32,

    pub program: oxc_ast::ast::Program<'alloc>,
    pub errors: Vec<oxc_diagnostics::OxcDiagnostic>,

    pub setup: Option<Span>,

    // lang attribute
    pub lang: Option<ScriptLanguage>,
    // generic attribute
    pub generic: Option<GenericParseResult<'alloc>>,
    // attrs attributes
    pub attrs: Option<Span>,
    // all attributes
    pub attributes: Vec<Prop>,
}

#[derive(Debug)]
pub struct OxcProp<'alloc> {
    /// The ID of the element this prop belongs to
    pub element_id: u32,
    /// The ID of this element's parent (NO_PARENT for root elements)
    pub parent_id: u32,

    // start of the prop
    pub start: u32,
    pub name_end: u32,

    pub arg: Option<OxcPropProcessed<'alloc>>,
    pub exp: Option<OxcPropProcessed<'alloc>>,
    // // note modifiers are just spans, no expressions
    pub modifiers: Option<Vec<Span>>,

    pub event: Prop,
}

#[derive(Debug)]
pub struct OxcPropProcessed<'alloc> {
    // offset to use for the expression start
    pub start: u32,
    pub end: u32,

    pub expression: Option<oxc_ast::ast::Expression<'alloc>>,
    pub errors: Option<Vec<oxc_diagnostics::OxcDiagnostic>>,

    pub bindings: Option<BindingExtractionResult<'alloc>>,
}

#[derive(Debug)]
pub struct OxcInterpolation<'a> {
    pub parent_id: u32,

    pub start: u32,
    pub end: u32,

    pub content: Span,

    pub expression: Option<oxc_ast::ast::Expression<'a>>,
    pub errors: Option<Vec<oxc_diagnostics::OxcDiagnostic>>,

    pub bindings: Option<BindingExtractionResult<'a>>,

    pub event: Interpolation,
}

#[derive(Debug)]
pub struct OxcVFor<'alloc> {
    pub element_id: u32,

    pub start: u32,
    pub end: u32,

    pub parsed: VForWithBindings<'alloc>,

    pub event: ElementScopeFor,
}

#[derive(Debug)]
pub struct OxcVSlotElement<'alloc> {
    pub element_id: u32,

    pub start: u32,
    pub end: u32,

    pub parsed: VSlotWithBindings<'alloc>,

    pub event: ElementScopeSlotElement,
}

#[derive(Debug)]
pub struct OxcVSlotTemplate<'alloc> {
    pub element_id: u32,

    pub start: u32,
    pub end: u32,

    pub parsed: VSlotWithBindings<'alloc>,

    pub event: ElementScopeSlotTemplate,
}

#[derive(Debug)]
pub struct OxcIfCondition<'alloc> {
    pub element_id: u32,

    pub start: u32,
    pub end: u32,

    pub expression: Option<oxc_ast::ast::Expression<'alloc>>,
    pub errors: Option<Vec<oxc_diagnostics::OxcDiagnostic>>,

    pub bindings: Option<BindingExtractionResult<'alloc>>,

    pub event: ElementScopeConditionIf,
}

#[derive(Debug)]
pub struct OxcElseIfCondition<'alloc> {
    pub element_id: u32,
    pub start: u32,
    pub end: u32,

    pub expression: Option<oxc_ast::ast::Expression<'alloc>>,
    pub errors: Option<Vec<oxc_diagnostics::OxcDiagnostic>>,

    pub bindings: Option<BindingExtractionResult<'alloc>>,

    pub event: ElementScopeConditionIf,
}

#[derive(Debug)]
pub struct OxcElseCondition {
    pub element_id: u32,
    pub start: u32,
    pub end: u32,
    pub event: ElementScopeConditionElse,
}

// /OXC Parsed

// OXC-Compiled Elements (emitted by oxc_parser after element_compiler)

/// OXC-parsed element start — props are parsed, scopes extracted.
/// Replaces `Event::ElementStart` after oxc_parser runs.
#[derive(Debug)]
pub struct OxcCompiledElementStart<'alloc> {
    /// Parsed props (non-structural directives).
    pub props: Vec<OxcProp<'alloc>>,
    /// Structural directives, ordered by priority: v-if > v-for > v-slot.
    pub scopes: Vec<ElementScope<'alloc>>,
    /// Owns the CompiledElementStart this replaces.
    pub event: CompiledElementStart,
}

/// OXC-parsed element closed — wraps the original for symmetry and future extension.
#[derive(Debug)]
pub struct OxcCompiledElementClosed {
    pub event: CompiledElementClosed,
}

/// Structural directive extracted from element props during oxc_parser processing.
/// Ordered by Vue priority: v-if/else-if/else > v-for > v-slot.
#[derive(Debug)]
pub enum ElementScope<'alloc> {
    If(OxcIfCondition<'alloc>),
    ElseIf(OxcElseIfCondition<'alloc>),
    Else(OxcElseCondition),
    For(OxcVFor<'alloc>),
    SlotElement(OxcVSlotElement<'alloc>),
    SlotTemplate(OxcVSlotTemplate<'alloc>),
}

// /OXC-Compiled Elements

// Compiled Elements

#[derive(Debug)]
pub struct CompiledRootScriptStart {
    pub start: u32,
    pub name_end: u32,

    pub tag_open: Span,

    pub setup: Option<Span>,

    // lang attribute
    pub lang: Option<ScriptLanguage>,
    // generic attribute
    pub generic: Option<Span>,
    // attrs attributes
    pub attrs: Option<Span>,
    // all attributes
    pub attributes: Vec<Prop>,

    pub tag_open_event: RootNodeOpenTagStart,
    pub tag_open_end_event: RootNodeOpenTagEnd,
}
#[derive(Debug)]
pub struct CompiledRootScriptEnd {
    pub start: u32,
    pub name_end: u32,
    pub end: u32,

    // None if self-closing tag
    pub tag_close: Option<Span>,

    // None is self-closing tag, otherwise content is the full content between open and close tags
    pub content: Option<Span>,

    pub tag_close_event: Option<RootNodeCloseTag>,
}

#[derive(Debug)]
pub struct CompiledRootTemplateStart {
    pub start: u32,
    pub name_end: u32,

    pub tag_open: Span,

    // vapor attribute
    pub vapor: Option<Span>,
    // lang attribute
    pub lang: Option<Span>,
    // all attributes
    pub attributes: Vec<Prop>,

    pub tag_open_event: RootNodeOpenTagStart,
    pub tag_open_end_event: RootNodeOpenTagEnd,
}

#[derive(Debug)]
pub struct CompiledRootTemplateEnd {
    pub start: u32,
    pub name_end: u32,
    pub end: u32,

    // None if self-closing tag
    pub tag_close: Option<Span>,

    // None is self-closing tag, otherwise content is the full content between open and close tags
    pub content: Option<Span>,

    pub tag_close_event: Option<RootNodeCloseTag>,
}

#[derive(Debug)]
pub struct CompiledRootStyleStart {
    pub start: u32,
    pub name_end: u32,

    pub tag_open: Span,

    // lang attribute
    pub lang: Option<StyleLang>,
    // scoped attribute
    pub scoped: bool,
    // module attribute
    pub module: Option<Span>,
    // all attributes
    pub attributes: Vec<Prop>,

    pub tag_open_event: RootNodeOpenTagStart,
    pub tag_open_end_event: RootNodeOpenTagEnd,
}
#[derive(Debug)]
pub struct CompiledRootStyleEnd {
    pub start: u32,
    pub name_end: u32,
    pub end: u32,

    // None if self-closing tag
    pub tag_close: Option<Span>,

    // None is self-closing tag, otherwise content is the full content between open and close tags
    pub content: Option<Span>,

    pub tag_close_event: Option<RootNodeCloseTag>,
}

#[derive(Debug)]
pub struct CompiledRootUnknownStart {
    pub start: u32,
    pub name_end: u32,

    pub tag_open: Span,

    pub content: Option<Span>,

    // all attributes
    pub attributes: Vec<Prop>,

    pub tag_open_event: RootNodeOpenTagStart,
    pub tag_open_end_event: RootNodeOpenTagEnd,
}
#[derive(Debug)]
pub struct CompiledRootUnknownEnd {
    pub start: u32,
    pub name_end: u32,
    pub end: u32,

    // None if self-closing tag
    pub tag_close: Option<Span>,

    // None is self-closing tag, otherwise content is the full content between open and close tags
    pub content: Option<Span>,

    pub tag_close_event: Option<RootNodeCloseTag>,
}

#[derive(Debug)]
pub struct CompiledElementStart {
    pub element_id: u32,
    pub parent_id: u32,

    pub event_open_tag: ElementOpenTagStart,
    pub event_open_tag_end: ElementOpenTagEnd,

    pub props: Vec<Prop>,
}
#[derive(Debug)]
pub struct CompiledElementClosed {
    pub element_id: u32,
    pub parent_id: u32,

    pub event_close_tag: Option<ElementCloseTag>,
}

// Compiled Elements

// CSS styles

/// Language for style preprocessing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleLang {
    Css = 0,
    Scss,
    Sass,
    Less,
    Stylus,
    Unknown,
}

/// Parsed v-bind() expression in styles, e.g. `color: v-bind(colorVar)`
#[derive(Debug, Clone)]
pub struct StyleVBind {
    // start of v-bind(
    pub start: u32,
    // end of )
    pub end: u32,

    pub name_end: u32,

    // content inside v-bind(...)
    pub content: Span,
    // todo add parsed results maybe??
}

// CSS parsed types (emitted by css_parser plugin)

/// Kind of Vue special pseudo-selector in scoped CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssParsedSpecialPseudoKind {
    /// `:deep(.inner)` — scopes the parent, descends into inner.
    Deep,
    /// `:global(.class)` — removes scoping entirely.
    Global,
    /// `:slotted(.slot)` — scopes with slot variant.
    Slotted,
}

/// A Vue special pseudo-selector found in a CSS selector.
#[derive(Debug, Clone)]
pub struct CssParsedSpecialPseudo {
    pub kind: CssParsedSpecialPseudoKind,
    /// Span of the full pseudo (e.g., `:deep(.inner)`) in SFC source.
    pub span: Span,
    /// Span of the inner content (e.g., `.inner`). None for bare `:deep`.
    pub inner: Option<Span>,
}

/// A parsed CSS selector with its span and structural info.
#[derive(Debug, Clone)]
pub struct CssParsedSelector {
    /// Full span of this selector text in SFC source.
    pub span: Span,
    /// Parsed special pseudo-selectors (:deep, :global, :slotted) with their spans.
    pub specials: Vec<CssParsedSpecialPseudo>,
}

/// A parsed v-bind() function call in CSS.
#[derive(Debug, Clone)]
pub struct CssParsedVBind {
    /// Span of full `v-bind(...)` in SFC source.
    pub full_span: Span,
    /// Span of the expression inside v-bind() in SFC source.
    pub expression: Span,
    /// Whether the expression was quoted (e.g., `v-bind('foo.bar')`).
    pub quoted: bool,
}

/// A class selector found in CSS (for CSS modules).
#[derive(Debug, Clone)]
pub struct CssParsedClass {
    /// Span of the class name (after the `.`) in SFC source.
    pub name_span: Span,
}

/// A parsed CSS style rule with selectors and declarations metadata.
#[derive(Debug, Clone)]
pub struct CssParsedRule {
    /// Span of the full selector list (before `{`) in SFC source.
    pub selector_span: Span,
    /// Individual selectors (split by `,`).
    pub selectors: Vec<CssParsedSelector>,
    /// v-bind() calls within this rule's declarations.
    pub v_binds: Vec<CssParsedVBind>,
    /// Class selectors found in this rule's selectors (for CSS modules).
    pub classes: Vec<CssParsedClass>,
}

/// Result of CSS parsing for a single `<style>` block.
/// Emitted as `Event::CssParsedStyle` by the css_parser plugin.
#[derive(Debug)]
pub struct CssParsedStyleBlock {
    /// Style language (css, scss, sass, less, stylus).
    pub lang: Option<StyleLang>,
    /// Whether this block has the `scoped` attribute.
    pub scoped: bool,
    /// Module attribute span (None if not a module block).
    pub module: Option<Span>,
    /// Content span in source (CSS content between `<style>` tags).
    pub content: Option<Span>,
    /// Parsed rules with selectors, v-binds, classes.
    pub rules: Vec<CssParsedRule>,
    /// All v-bind expressions across all rules (flattened for convenience).
    pub v_binds: Vec<CssParsedVBind>,
    /// All class selectors across all rules (flattened for convenience).
    pub classes: Vec<CssParsedClass>,
    /// Original compiled start event (preserves source positions and attributes).
    pub compiled_start: CompiledRootStyleStart,
    /// Original compiled end event (preserves content span and close tag positions).
    pub compiled_end: CompiledRootStyleEnd,
}

// /CSS parsed types

// CSS processed types (emitted by css_style plugin)

/// Processed v-bind() expression extracted from CSS.
/// `v-bind(expr)` → `var(--{scope_id}-{sanitized})`.
#[derive(Debug, Clone)]
pub struct ProcessedCssVBind {
    /// Span of the original expression inside v-bind(...) in SFC source.
    pub expression: Span,
    /// Generated CSS variable name bytes (e.g., b"--a4f2eed6-color").
    /// Owned because it is computed, not a source slice.
    pub var_name: String,
    /// Byte offset of `v-bind(` in original SFC source.
    pub css_start: u32,
    /// Byte offset of closing `)` + 1 in original SFC source.
    pub css_end: u32,
}

/// A single CSS module class mapping: original class name → hashed class name.
#[derive(Debug, Clone)]
pub struct CssModuleClassMapping {
    /// Original class name span in source (e.g., "btn").
    pub original: Span,
    /// Hashed class name (e.g., "btn_a4f2eed6_0"). Owned, computed.
    pub hashed: String,
}

/// CSS module metadata for a single `<style module>` block.
#[derive(Debug, Clone)]
pub struct CssModuleInfo {
    /// Span of custom module name in source. None for default "$style".
    pub custom_name: Option<Span>,
    /// Class name mappings (original → hashed).
    pub classes: Vec<CssModuleClassMapping>,
}

/// Result of CSS processing for a single `<style>` block.
/// Emitted as `Event::ProcessedStyle` by the css_style plugin.
#[derive(Debug)]
pub struct ProcessedStyleBlock {
    /// Style language (css, scss, sass, less, stylus).
    pub lang: Option<StyleLang>,
    /// Whether this block is scoped.
    pub scoped: bool,
    /// CSS module info (None if not a module block).
    pub module: Option<CssModuleInfo>,

    /// Transformed CSS bytes (scoped selectors applied, v-bind replaced, modules hashed).
    /// None if no transformation was needed (plain unscoped style).
    pub transformed_css: Option<Vec<u8>>,
    /// v-bind() expressions extracted from this style block.
    pub v_bind_expressions: Vec<ProcessedCssVBind>,

    /// CSS processing errors (e.g., lightningcss parse failures).
    /// Non-empty when CSS transformation was attempted but failed.
    pub errors: Vec<String>,

    /// Original compiled start event (preserves source positions and attributes).
    pub compiled_start: CompiledRootStyleStart,
    /// Original compiled end event (preserves content span and close tag positions).
    pub compiled_end: CompiledRootStyleEnd,
}

// /CSS processed types

pub enum Event<'alloc> {
    // Raw tokenizer events (emitted by Syntax)
    RootOpenStart(RootNodeOpenTagStart),
    RootOpenTagEnd(RootNodeOpenTagEnd),
    RootCloseTag(RootNodeCloseTag),
    Lang(ScriptLang),

    // Element
    OpenTag(ElementOpenTagStart),
    OpenTagEnd(ElementOpenTagEnd),
    CloseTag(ElementCloseTag),

    // Content
    Prop(Prop),
    Interpolation(Interpolation),
    Comment(Comment),
    Text(Text),

    // Compiled root open/close (emitted by element_compiler)
    CompiledScriptStart(CompiledRootScriptStart),
    CompiledScriptEnd(CompiledRootScriptEnd),
    CompiledTemplateStart(CompiledRootTemplateStart),
    CompiledTemplateEnd(CompiledRootTemplateEnd),
    CompiledStyleStart(CompiledRootStyleStart),
    CompiledStyleEnd(CompiledRootStyleEnd),
    CompiledUnknownStart(CompiledRootUnknownStart),
    CompiledUnknownEnd(CompiledRootUnknownEnd),

    // Compiled elements (emitted by element_compiler)
    ElementStart(CompiledElementStart),
    ElementClosed(CompiledElementClosed),

    // OXC-parsed events (emitted by oxc_parser)
    OxcScript(OxcScript<'alloc>),
    OxcProp(OxcProp<'alloc>),
    OxcInterpolation(OxcInterpolation<'alloc>),
    OxcVFor(OxcVFor<'alloc>),
    OxcVSlotElement(OxcVSlotElement<'alloc>),
    OxcVSlotTemplate(OxcVSlotTemplate<'alloc>),
    OxcIfCondition(OxcIfCondition<'alloc>),
    OxcElseIfCondition(OxcElseIfCondition<'alloc>),
    OxcElseCondition(OxcElseCondition),

    // OXC-compiled elements (emitted by oxc_parser after element_compiler)
    OxcCompiledElementStart(OxcCompiledElementStart<'alloc>),
    OxcCompiledElementClosed(OxcCompiledElementClosed),

    // Scopes (raw, emitted by Syntax)
    ScopeIf(ElementScopeConditionIf),
    ScopeElseIf(ElementScopeConditionElseIf),
    ScopeElse(ElementScopeConditionElse),
    ScopeFor(ElementScopeFor),
    ScopeSlotElement(ElementScopeSlotElement),
    ScopeSlotTemplate(ElementScopeSlotTemplate),

    // CSS parsed style (emitted by css_parser plugin)
    CssParsedStyle(CssParsedStyleBlock),

    // CSS processed style (emitted by css_style plugin)
    ProcessedStyle(ProcessedStyleBlock),

    // Binding metadata (emitted by code_gen_script)
    ScriptBindings(super::binding_types::BindingMetadata),
}
