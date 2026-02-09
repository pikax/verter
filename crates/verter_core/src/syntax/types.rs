use crate::{
    common::Span,
    cursor::ScriptLanguage,
    tokenizer::QuoteType,
    utils::oxc::{
        vue::{GenericParseResult, ScriptParseResult, VForWithBindings, VSlotWithBindings},
        BindingExtractionResult,
    },
};

pub enum SyntaxTypes {
    ROOT = 0,
    ELEMENT,
    Prop,
    TEXT,
    COMMENT,
    INTERPOLATION,

    PropArg,
    PropValue,

    Expression,
}

// events

pub trait SyntaxNode {
    fn get_id(&self) -> u32;
}

// rootSyntax
pub struct SyntaxRoot {
    pub start: u32,
    pub end: u32,
}
impl SyntaxNode for SyntaxRoot {
    // offset of <template>
    fn get_id(&self) -> u32 {
        self.start
    }
}
// /rootSyntax
// elementSyntax
// #[derive(Debug, Clone)]
// pub struct SyntaxElement {
//     pub start: u32,
//     pub end: u32,

//     pub is_self_closing: bool,
//     // if self-closing, content is None or it's still being processed
//     pub content: Option<SyntaxElementContentEnd>,
//     // None is root
//     pub parent_id: u32,

//     // pub props: Option<Vec<SyntaxProp>>,
//     pub nested_level: usize,
// }
// impl SyntaxNode for SyntaxElement {
//     // offset of `<`
//     fn get_id(&self) -> u32 {
//         self.start
//     }
// }

// #[derive(Debug, Clone)]
// pub struct SyntaxElementContentEnd {
//     // offset from `>`
//     pub start: u32,
//     // offset to `<`
//     pub end: u32,

//     pub closing_tag_start: u32,
//     pub closing_tag_end: u32,
// }

/// Sentinel value indicating no parent (root element)
pub const NO_PARENT: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub struct SyntaxCloseTag {
    /// The ID of the element being closed (its opening tag's start position)
    pub element_id: u32,
    /// The ID of this element's parent (NO_PARENT for root elements)
    pub parent_id: u32,

    pub tag_type: SyntaxTagType,

    pub start: u32,
    pub name_end: u32,
    pub end: u32,

    pub nested_level: usize,
    pub is_void_element: bool,
}
impl SyntaxNode for SyntaxCloseTag {
    // offset of `</`
    fn get_id(&self) -> u32 {
        self.start
    }
}
// /elementSyntax

// nodeProp
#[derive(Debug, Clone)]
pub struct SyntaxProp {
    /// The ID of the element this prop belongs to
    pub element_id: u32,
    /// The ID of this element's parent (NO_PARENT for root elements)
    pub parent_id: u32,

    // offset of the attribute/directive name
    pub start: u32,
    // offset of the attribute/directive end
    pub end: u32,

    pub name_end: u32,

    pub is_directive: bool,

    pub value: Option<SyntaxPropValue>,
    pub arg: Option<SyntaxPropArg>,

    pub modifiers: Option<Vec<Span>>,
    pub quote: Option<QuoteType>,
}
impl SyntaxNode for SyntaxProp {
    // offset of the prop name start
    fn get_id(&self) -> u32 {
        self.start
    }
}

#[derive(Debug, Clone)]
pub struct SyntaxPropValue {
    pub start: u32,
    pub end: u32,
}
#[derive(Debug, Clone)]
pub struct SyntaxPropArg {
    pub start: u32,
    pub end: u32,
    pub is_dynamic: bool,
}

impl SyntaxNode for SyntaxPropValue {
    // offset of the arg/value start
    fn get_id(&self) -> u32 {
        self.start
    }
}

impl SyntaxNode for SyntaxPropArg {
    // offset of the arg/value start
    fn get_id(&self) -> u32 {
        self.start
    }
}

// /nodeProp
// textSyntax
#[derive(Debug, Clone)]
pub struct SyntaxText {
    pub parent_id: u32,

    pub start: u32,
    pub end: u32,
}
impl SyntaxNode for SyntaxText {
    // offset of the text start
    fn get_id(&self) -> u32 {
        self.start
    }
}
// /textSyntax
// interpolationSyntax
#[derive(Debug, Clone)]
pub struct SyntaxInterpolation {
    pub parent_id: u32,

    pub start: u32,
    pub end: u32,

    // after {{ - may contain whitespace
    pub content_start: u32,
    // before }} - may contain whitespace
    pub content_end: u32,
}
impl SyntaxNode for SyntaxInterpolation {
    // offset of the interpolation start
    fn get_id(&self) -> u32 {
        self.start
    }
}
// /interpolationSyntax
// commentSyntax
#[derive(Debug, Clone)]
pub struct SyntaxComment {
    pub parent_id: u32,

    pub start: u32,
    pub end: u32,
}
impl SyntaxNode for SyntaxComment {
    // offset of the comment start
    fn get_id(&self) -> u32 {
        self.start
    }
}
// /commentSyntax

// Error/Warning

#[derive(Debug)]
pub enum SyntaxErrorMessages {
    OpenTagNotFound,
    UnclosedTag,
}
#[derive(Debug)]
pub enum SyntaxWarningMessages {
    UnclosedTag,
}
#[derive(Debug)]
pub struct SyntaxError {
    pub start: u32,
    pub end: u32,

    pub message: SyntaxErrorMessages,
}

#[derive(Debug)]
pub struct SyntaxWarning {
    pub start: u32,
    pub end: u32,

    pub message: SyntaxWarningMessages,
}

// /Error/Warning

// intermediaries

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxTagType {
    Element = 0,
    Component = 1,

    Slot = 2,
    Template = 3,

    CustomElement = 4,

    /// <component :is="..."> dynamic component
    DynamicComponent = 5,

    RootScript,
    RootTemplate,
    RootStyle,
    RootUnknown,
}

#[derive(Debug, Clone)]
pub struct SyntaxOpenTagStart {
    pub start: u32,
    pub name_end: u32,

    pub tag_type: SyntaxTagType,
    pub element_id: u32,

    /// Parent element ID (NO_PARENT for root elements where nested_level == 0)
    pub parent_id: u32,

    pub nested_level: usize,

    /// Some elements do not have a separate end tag
    pub is_void_element: bool,
}
impl SyntaxNode for SyntaxOpenTagStart {
    // offset of `<`
    fn get_id(&self) -> u32 {
        self.start
    }
}

#[derive(Debug, Clone)]
pub struct SyntaxOpenTagEnd {
    pub start: u32,
    pub end: u32,

    pub name_end: u32,

    pub tag_type: SyntaxTagType,

    pub element_id: u32,
    /// Parent element ID (NO_PARENT for root elements where nested_level == 0)
    pub parent_id: u32,

    pub self_closing: bool,
    // pub content: Option<SyntaxElementContentEnd>,
    pub nested_level: usize,

    /// Some elements do not have a separate end tag
    pub is_void_element: bool,
}
impl SyntaxNode for SyntaxOpenTagEnd {
    // offset of `>`
    fn get_id(&self) -> u32 {
        self.start
    }
}

// /intermediaries

// oxc parsed nodes

#[derive(Debug)]
pub struct OxcProp<'alloc> {
    /// The ID of the element this prop belongs to
    pub element_id: u32,
    /// The ID of this element's parent (NO_PARENT for root elements)
    pub parent_id: u32,

    // start of the prop
    pub start: u32,

    pub arg: Option<OxcPropProcessed<'alloc>>,
    pub exp: Option<OxcPropProcessed<'alloc>>,
    // // note modifiers are just spans, no expressions
    pub modifiers: Option<Vec<Span>>,

    pub event: SyntaxProp,
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

impl<'alloc> SyntaxNode for OxcProp<'alloc> {
    // offset of the prop name start
    fn get_id(&self) -> u32 {
        self.start
    }
}

#[derive(Debug)]
pub struct OxcScriptContent<'alloc> {
    pub element_id: u32,
    // always root
    pub parent_id: u32,

    pub tag_open_start: u32,
    pub tag_open_end: u32,

    pub tag_close_start: u32,
    pub tag_close_end: u32,

    /// Start position of the script content (after <script>)
    pub content_start: u32,
    /// End position of the script content (before </script>)
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
    pub attributes: Vec<SyntaxProp>,
}
impl<'alloc> SyntaxNode for OxcScriptContent<'alloc> {
    // offset of the prop name start
    fn get_id(&self) -> u32 {
        self.content_start
    }
}

// #[derive(Debug)]
// pub struct OxcScriptAttribute {
//     pub start: u32,
//     pub end: u32,
//     pub name_end: u32,
//     pub value_start: Option<u32>,
// }

#[derive(Debug)]
pub struct OxcInterpolation<'a> {
    pub parent_id: u32,

    pub start: u32,
    pub end: u32,

    pub expression: Option<oxc_ast::ast::Expression<'a>>,
    pub errors: Option<Vec<oxc_diagnostics::OxcDiagnostic>>,

    pub bindings: Option<BindingExtractionResult<'a>>,

    pub event: SyntaxInterpolation,
}
impl<'a> SyntaxNode for OxcInterpolation<'a> {
    // offset of the interpolation start
    fn get_id(&self) -> u32 {
        self.start
    }
}

#[derive(Debug)]
pub struct OxcVForProp<'a> {
    /// The ID of the element this prop belongs to
    pub element_id: u32,
    /// The ID of this element's parent (NO_PARENT for root elements)
    pub parent_id: u32,

    pub start: u32,

    /// The parsed v-for expression with extracted bindings
    pub parsed: VForWithBindings<'a>,

    pub event: SyntaxProp,
}

impl<'a> OxcVForProp<'a> {
    /// Returns the left side of the v-for expression (iteration variable/pattern)
    pub fn left(&self) -> Option<&oxc_ast::ast::Expression<'a>> {
        self.parsed.left()
    }

    /// Returns the right side of the v-for expression (the iterable)
    pub fn right(&self) -> Option<&oxc_ast::ast::Expression<'a>> {
        self.parsed.right()
    }

    /// Returns whether the expression uses 'of' instead of 'in'
    pub fn is_of(&self) -> bool {
        self.parsed.is_of()
    }

    /// Returns true if there are any parse errors
    pub fn has_errors(&self) -> bool {
        self.parsed.has_errors()
    }

    /// Returns the local binding spans declared by the v-for (iteration variables).
    /// Use `span.slice(source)` to get the string value.
    pub fn locals(&self) -> &[Span] {
        &self.parsed.locals
    }

    /// Returns the external reference spans used in the v-for expression.
    /// Use `span.slice(source)` to get the string value.
    pub fn references(&self) -> &[Span] {
        &self.parsed.references
    }
}

impl<'a> SyntaxNode for OxcVForProp<'a> {
    // offset of the interpolation start
    fn get_id(&self) -> u32 {
        self.start
    }
}

#[derive(Debug)]
pub struct OxcVSlotProp<'a> {
    /// The ID of the element this prop belongs to
    pub element_id: u32,
    /// The ID of this element's parent (NO_PARENT for root elements)
    pub parent_id: u32,

    pub start: u32,

    /// The parsed v-slot expression with extracted bindings
    pub parsed: VSlotWithBindings<'a>,

    pub event: SyntaxProp,
}

impl<'a> OxcVSlotProp<'a> {
    /// Returns the parsed formal parameters from the slot expression
    pub fn params(&self) -> Option<&oxc_ast::ast::FormalParameters<'a>> {
        self.parsed.params()
    }

    /// Returns true if there are any parse errors
    pub fn has_errors(&self) -> bool {
        self.parsed.has_errors()
    }

    /// Returns true if parsing was successful
    pub fn is_ok(&self) -> bool {
        self.parsed.is_ok()
    }

    /// Returns the local binding spans declared by the slot parameters.
    /// Use `span.slice(source)` to get the string value.
    pub fn locals(&self) -> &[Span] {
        &self.parsed.locals
    }

    /// Returns the external reference spans used in the slot expression.
    /// Use `span.slice(source)` to get the string value.
    pub fn references(&self) -> &[Span] {
        &self.parsed.references
    }
}

impl<'a> SyntaxNode for OxcVSlotProp<'a> {
    // offset of the interpolation start
    fn get_id(&self) -> u32 {
        self.start
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OxcVConditionType {
    If = 0,
    ElseIf = 1,
    Else = 2,
}

#[derive(Debug)]
pub struct OxcVConditional<'a> {
    /// The ID of the element this prop belongs to
    pub element_id: u32,
    /// The ID of this element's parent (NO_PARENT for root elements)
    pub parent_id: u32,

    pub start: u32,
    pub end: u32,

    pub expression: Option<oxc_ast::ast::Expression<'a>>,
    pub errors: Option<Vec<oxc_diagnostics::OxcDiagnostic>>,

    pub bindings: Option<BindingExtractionResult<'a>>,

    pub condition_type: OxcVConditionType,

    pub event: SyntaxProp,
}
impl<'a> SyntaxNode for OxcVConditional<'a> {
    // offset of the interpolation start
    fn get_id(&self) -> u32 {
        self.start
    }
}

// #[derive(Debug)]
// pub enum OxcParsedSyntax<'alloc> {
//     Prop(OxcProp<'alloc>),
//     ScriptContent(OxcScriptContent<'alloc>),
//     Interpolation(OxcInterpolation<'alloc>),
// }

// /oxc parsed nodes

// Analysis events

#[derive(Debug, Clone)]
pub enum AnalysisScopeType {
    /// Conditional scope (v-if/else-if/else) - continual only
    Conditional,
    /// Loop scope (v-for) - can provide bindings
    Loop,
    /// Slot scope (v-slot) - can provide bindings
    Slot,
    /// Directive expression - no provided bindings, but might need type narrowing
    DirectiveExp,
    /// Directive argument - no provided bindings, but might need type narrowing
    DirectiveArg,
    /// Template interpolation
    Interpolation,
}

#[derive(Debug, Clone)]
pub struct AnalysisProvidedBinding {
    pub scope_id: u32,
    pub element_id: u32,
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone)]
pub struct AnalysisScopeStart<'a> {
    pub id: u32,
    pub r#type: AnalysisScopeType,

    pub parent_id: u32,
    pub element_id: u32,
    pub parent_scope_id: u32,

    pub bindings: BindingExtractionResult<'a>,
    pub parent_bindings: BindingExtractionResult<'a>,
    pub provided_bindings: Vec<AnalysisProvidedBinding>,

    pub condition: Option<AnalysisFullScopedCondition>,
    pub parent_conditions: Vec<AnalysisFullScopedCondition>,
}

#[derive(Debug, Clone, Copy)]
pub struct AnalysisScopeCondition {
    pub condition_type: OxcVConditionType,
    pub start: u32,
    pub end: u32,
    pub expression_start: u32,
    pub expression_end: u32,
}

#[derive(Debug, Clone)]
pub struct AnalysisFullScopedCondition {
    pub value: Option<AnalysisScopeCondition>,
    pub siblings: Vec<AnalysisScopeCondition>,
}

#[derive(Debug)]
pub struct AnalysisScopeEventData<'a> {
    pub start: u32,
    pub end: u32,
    pub r#type: AnalysisScopeType,

    pub parent_id: u32,
    pub element_id: u32,
    pub parent_scope_id: u32,

    pub bindings: BindingExtractionResult<'a>,
    pub condition: Option<AnalysisFullScopedCondition>,

    pub parent_bindings: Option<BindingExtractionResult<'a>>,
    pub provided_bindings: Option<Vec<AnalysisProvidedBinding>>,
    pub parent_conditions: Vec<AnalysisFullScopedCondition>,
}

/// New analysis events
///

#[derive(Debug)]
pub struct AnalysedOxcProp<'a> {
    pub event: OxcProp<'a>,
    pub arg: Option<AnalysisScopeEventData<'a>>,
    pub exp: Option<AnalysisScopeEventData<'a>>,
}

#[derive(Debug)]
pub struct AnalysedOxcInterpolation<'a> {
    pub event: OxcInterpolation<'a>,
    pub interpolation: Option<AnalysisScopeEventData<'a>>,
}

#[derive(Debug)]
pub struct AnalysedStartScopeVConditional<'a> {
    pub event: OxcVConditional<'a>,
    pub scope: AnalysisScopeStart<'a>,
}

#[derive(Debug)]
pub struct AnalysedCloseScopes {
    pub event: SyntaxCloseTag,
    pub closed_scope_ids: Vec<u32>,
}

#[derive(Debug)]
pub struct AnalysedVFor<'a> {
    pub event: OxcVForProp<'a>,
    pub scope: AnalysisScopeStart<'a>,
    pub references: Option<AnalysisScopeEventData<'a>>,
}

#[derive(Debug)]
pub struct AnalysedVSlot<'a> {
    pub event: OxcVSlotProp<'a>,
    pub scope: AnalysisScopeStart<'a>,
    pub references: Option<AnalysisScopeEventData<'a>>,
}

#[derive(Debug)]
pub struct AnalysedScriptInfo<'a> {
    pub event: OxcScriptContent<'a>,
    pub script_info: AnalysisScriptInfo<'a>,
}

#[derive(Debug)]
pub struct AnalysisScriptInfo<'a> {
    pub event: OxcScriptContent<'a>,
    pub parsed: ScriptParseResult<'a>,
}

// /Analysis events

// CSS Style types

/// Language for style preprocessing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleLang {
    Css,
    Scss,
    Sass,
    Less,
    Stylus,
}

/// Parsed v-bind() expression in CSS
/// Uses Span to reference source bytes - no allocations
#[derive(Debug, Clone)]
pub struct CssVBindExpression {
    /// Span for generated CSS variable name (e.g., "--a4f2eed6-color")
    pub var_name_start: u32,
    pub var_name_end: u32,
    /// Span of original expression in source (e.g., "color" or "theme.color")
    pub expression: Span,
    /// Start position in CSS content
    pub css_start: u32,
    /// End position in CSS content
    pub css_end: u32,
}

/// CSS module class mapping (original name → hashed name)
#[derive(Debug, Clone)]
pub struct CssModuleClass {
    /// Original class name span in source
    pub original: Span,
    /// Hashed class name (stored in output buffer)
    pub hashed_start: u32,
    pub hashed_end: u32,
}

/// Parsed CSS style content (output of css_parser plugin)
/// All content referenced via Spans - use span.slice(source) to get &str
#[derive(Debug)]
pub struct CssStyleContent {
    pub element_id: u32,
    pub parent_id: u32,

    pub tag_open_start: u32,
    pub tag_open_end: u32,
    pub tag_close_start: u32,
    pub tag_close_end: u32,

    pub content_start: u32,
    pub content_end: u32,

    // Attributes
    pub scoped: bool,
    /// None = not module, Some(span) = module name span (default is "$style")
    pub module: Option<Span>,
    pub lang: Option<StyleLang>,
    pub attributes: Vec<SyntaxProp>,

    // Parsed CSS transformation info
    pub v_bind_expressions: Vec<CssVBindExpression>,
    pub css_module_classes: Vec<CssModuleClass>,
}

// /CSS Style types

// Event

#[derive(Debug)]
pub enum SyntaxEvent<'a> {
    Prop(SyntaxProp),
    Text(SyntaxText),
    Interpolation(SyntaxInterpolation),
    Comment(SyntaxComment),

    // Element(SyntaxElement),
    // ElementContentEnd(SyntaxElementContentEnd),
    OpenTagStart(SyntaxOpenTagStart),
    OpenTagEnd(SyntaxOpenTagEnd),

    CloseTag(SyntaxCloseTag),

    Error(SyntaxError),
    Warning(SyntaxWarning),

    // overrides SyntaxProp
    OxcProp(OxcProp<'a>),
    // overrides v-for
    OxcVFor(OxcVForProp<'a>),
    // overrides v-slot
    OxcVSlot(OxcVSlotProp<'a>),
    // overrides v-if / v-else-if / v-else
    OxcVConditional(OxcVConditional<'a>),

    // Overrides CloseTag in SyntaxElement
    OxcScriptContent(OxcScriptContent<'a>),
    // overrides SyntaxInterpolation
    OxcInterpolation(OxcInterpolation<'a>),
    // CSS style content (from css_parser plugin)
    CssStyleContent(CssStyleContent),

    // analysis events
    AnalysedScript(AnalysisScriptInfo<'a>),
    AnalysedProp(AnalysedOxcProp<'a>),
    AnalysedInterpolation(AnalysedOxcInterpolation<'a>),
    AnalysedCondition(AnalysedStartScopeVConditional<'a>),
    AnalysedCloseScopes(AnalysedCloseScopes),
    AnalysedVFor(AnalysedVFor<'a>),
    AnalysedVSlot(AnalysedVSlot<'a>),
}

// /Event
