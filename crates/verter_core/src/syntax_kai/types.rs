use lightningcss::traits::Op;

use crate::{
    common::Span,
    cursor::ScriptLanguage,
    tokenizer::QuoteType,
    utils::{
        oxc::{
            vue::{GenericParseResult, VForWithBindings, VSlotWithBindings},
            BindingExtractionResult,
        },
        vue::{PatchFlag, PatchFlags},
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

// Compiled Elements

#[derive(Debug)]
pub enum CompiledProp<'alloc> {
    Prop(Prop),
    Oxc(OxcProp<'alloc>),
}

#[derive(Debug)]
pub struct CompiledRootScriptStart<'alloc> {
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
    pub attributes: Vec<CompiledProp<'alloc>>,

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
pub struct CompiledRootTemplateStart<'alloc> {
    pub start: u32,
    pub name_end: u32,

    pub tag_open: Span,

    // vapor attribute
    pub vapor: Option<Span>,
    // lang attribute
    pub lang: Option<Span>,
    // all attributes
    pub attributes: Vec<CompiledProp<'alloc>>,

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
pub struct CompiledRootStyleStart<'alloc> {
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
    pub attributes: Vec<CompiledProp<'alloc>>,

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
pub struct CompiledRootUnknownStart<'alloc> {
    pub start: u32,
    pub name_end: u32,

    pub tag_open: Span,

    pub content: Option<Span>,

    // all attributes
    pub attributes: Vec<CompiledProp<'alloc>>,

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
pub struct CompiledElementStart<'alloc> {
    pub element_id: u32,
    pub parent_id: u32,

    pub event_open_tag: ElementOpenTagStart,
    pub event_open_tag_end: ElementOpenTagEnd,

    pub props: Vec<CompiledProp<'alloc>>,
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

// /CSS styles

pub enum Event<'alloc> {
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

    // Compiled
    RootScript(CompiledRootScript),
    RootTemplate(CompiledRootTemplate),
    RootStyle(CompiledRootStyle),
    RootUnknown(CompiledRootUnknown),

    // Oxc Parsed
    OxcScript(OxcScript<'alloc>),
    OxcProp(OxcProp<'alloc>),
    OxcInterpolation(OxcInterpolation<'alloc>),
    OxcVFor(OxcVFor<'alloc>),
    OxcVSlotElement(OxcVSlotElement<'alloc>),
    OxcVSlotTemplate(OxcVSlotTemplate<'alloc>),
    OxcIfCondition(OxcIfCondition<'alloc>),
    OxcElseIfCondition(OxcElseIfCondition<'alloc>),
    OxcElseCondition(OxcElseCondition),

    // Compiled
    ElementStart(CompiledElementStart<'alloc>),
    ElementClosed(CompiledElementClosed),

    // Scopes
    ScopeIf(ElementScopeConditionIf),
    ScopeElseIf(ElementScopeConditionElseIf),
    ScopeElse(ElementScopeConditionElse),
    ScopeFor(ElementScopeFor),
    ScopeSlotElement(ElementScopeSlotElement),
    ScopeSlotTemplate(ElementScopeSlotTemplate),
}
