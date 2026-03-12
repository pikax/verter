//! CSS/style analysis for Vue SFC style blocks.
//!
//! Uses a lightweight byte-level CSS scanner to extract selectors, specificity,
//! classes, IDs, custom properties, and at-rules from CSS blocks. Vue-specific
//! features (v-bind, :deep, :global, :slotted) are passed through from verter_core.
//!
//! For non-CSS preprocessors (SCSS, Less, etc.), only Vue features are stored.

use verter_span::Span;

// =============================================================================
// Vue Feature Input Types (constructed by verter_host from verter_core output)
// =============================================================================

/// Pre-extracted Vue-specific CSS features from verter_core.
/// `verter_host` converts `CssParsed*` types into these.
#[derive(Debug, Clone, Default)]
pub struct VueStyleInput {
    pub v_binds: Vec<VBindInput>,
    pub special_pseudos: Vec<SpecialPseudoInput>,
}

/// A `v-bind()` expression found in CSS.
#[derive(Debug, Clone)]
pub struct VBindInput {
    /// The expression text (resolved from span by verter_host).
    pub expression: String,
    pub quoted: bool,
    pub start: u32,
    pub end: u32,
    /// Generated CSS variable name from the prepass (e.g. `"--a4f2eed6-color"`).
    pub generated_var_name: Option<String>,
}

/// A Vue special pseudo-class (`:deep`, `:global`, `:slotted`).
#[derive(Debug, Clone)]
pub struct SpecialPseudoInput {
    pub kind: SpecialPseudoKind,
    pub start: u32,
    pub end: u32,
    /// Inner selector text (resolved from span by verter_host).
    pub inner: Option<String>,
}

/// Discriminant for Vue special pseudo-classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SpecialPseudoKind {
    Deep,
    Global,
    Slotted,
}

// =============================================================================
// Analysis Output Types (owned, serializable)
// =============================================================================

/// Complete analysis of a single `<style>` block.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleBlockAnalysis {
    pub lang: StyleAnalysisLang,
    pub scoped: bool,
    pub is_module: bool,
    pub module_name: Option<String>,

    /// Byte offset of the style block content start within the SFC source.
    /// Stored as block metadata for remapping and block identity.
    #[serde(default)]
    pub content_offset: u32,

    // Vue features (from verter_core, all languages)
    pub v_binds: Vec<AnalyzedVBind>,
    pub special_pseudos: Vec<AnalyzedSpecialPseudo>,

    // Full CSS analysis (scanner-based, CSS-only)
    pub css: Option<CssAnalysis>,

    pub flags: u16,
}

impl StyleBlockAnalysis {
    /// Get flags as `StyleAnalysisFlags`.
    pub fn analysis_flags(&self) -> StyleAnalysisFlags {
        StyleAnalysisFlags::from_bits_truncate(self.flags)
    }
}

/// Analyzed `v-bind()` expression from a style block.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedVBind {
    pub expression: String,
    pub quoted: bool,
    pub start: u32,
    pub end: u32,
    /// Generated CSS variable name from the prepass (e.g. `"--a4f2eed6-color"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_var_name: Option<String>,
}

/// Analyzed Vue special pseudo-class.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedSpecialPseudo {
    pub kind: SpecialPseudoKind,
    pub start: u32,
    pub end: u32,
    pub inner: Option<String>,
}

/// Full CSS analysis produced by the byte-level scanner.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CssAnalysis {
    pub selectors: Vec<AnalyzedSelector>,
    pub classes: Vec<AnalyzedCssClass>,
    pub ids: Vec<AnalyzedCssId>,
    pub custom_properties: Vec<AnalyzedCustomProperty>,
    pub at_rules: Vec<AnalyzedAtRule>,
    /// `var()` usages in non-custom-property declarations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub var_usages: Vec<AnalyzedVarUsage>,
    pub rule_count: u32,
}

impl CssAnalysis {
    /// Validate that all SFC-absolute spans are within `[0, sfc_source_len)`.
    ///
    /// This catches double-offset bugs where `content_offset` is accidentally
    /// applied twice, pushing spans beyond the SFC source boundary.
    /// Only runs in debug builds (`debug_assert!`).
    pub fn debug_assert_valid_spans(&self, sfc_source_len: u32) {
        fn check(span: Span, sfc_source_len: u32, label: &str) {
            debug_assert!(
                span.end <= sfc_source_len,
                "CSS span out of bounds: {label} span {start}..{end} exceeds SFC length {sfc_source_len}",
                start = span.start,
                end = span.end,
                sfc_source_len = sfc_source_len,
            );
        }

        for sel in &self.selectors {
            check(sel.span, sfc_source_len, "selector");
        }
        for cls in &self.classes {
            check(cls.span, sfc_source_len, "class");
        }
        for id in &self.ids {
            check(id.span, sfc_source_len, "id");
        }
        for prop in &self.custom_properties {
            check(prop.name_span, sfc_source_len, "custom-property name");
            check(prop.value_span, sfc_source_len, "custom-property value");
            for var_ref in &prop.var_references {
                check(var_ref.span, sfc_source_len, "var-reference");
                check(var_ref.name_span, sfc_source_len, "var-reference name");
            }
        }
        for usage in &self.var_usages {
            check(usage.reference.span, sfc_source_len, "var-usage");
            check(usage.reference.name_span, sfc_source_len, "var-usage name");
        }
    }
}

/// A CSS selector with its computed specificity and optional parsed structure.
#[derive(Debug, Clone)]
pub struct AnalyzedSelector {
    pub text: String,
    /// Specificity tuple: (id, class, type).
    pub specificity: (u32, u32, u32),
    /// SFC-absolute byte span of the selector text.
    pub span: Span,
    /// Parsed selector structure for matching. `None` for unparseable selectors.
    pub structure: Option<StructuredSelector>,
}

impl serde::Serialize for AnalyzedSelector {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 4 + usize::from(self.structure.is_some());
        let mut s = serializer.serialize_struct("AnalyzedSelector", count)?;
        s.serialize_field("text", &self.text)?;
        s.serialize_field("specificity", &self.specificity)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.structure.is_some() {
            s.serialize_field("structure", &self.structure)?;
        }
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedSelector {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            text: String,
            specificity: (u32, u32, u32),
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            #[serde(default)]
            structure: Option<StructuredSelector>,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            text: w.text,
            specificity: w.specificity,
            span: Span::new(w.span_start, w.span_end),
            structure: w.structure,
        })
    }
}

// =============================================================================
// Structured Selector Types
// =============================================================================

/// A parsed CSS selector: a chain of compound selectors joined by combinators.
///
/// For `".parent > .child.active"`:
/// - `compounds`: `[CompoundSelector { classes: ["parent"] }, CompoundSelector { classes: ["child", "active"] }]`
/// - `combinators`: `[Child]`
///
/// The compounds and combinators alternate: `compounds[0] combinator[0] compounds[1] combinator[1] ...`
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredSelector {
    pub compounds: Vec<CompoundSelector>,
    pub combinators: Vec<SelectorCombinator>,
}

/// A compound selector: a sequence of simple selectors applied to the same element.
///
/// For `"div.active#main"`: element=Some("div"), classes=["active"], id=Some("main")
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompoundSelector {
    /// Element type selector (e.g., `"div"`, `"p"`). `None` if omitted (class/id-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// Class selectors (`.foo`, `.bar`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
    /// ID selector (`#app`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Attribute selectors (`[type="text"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<AttributeSelector>,
    /// Pseudo-classes (`:hover`, `:not(...)`, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pseudo_classes: Vec<SelectorPseudoClass>,
    /// Whether a pseudo-element is present (`::before`, `::after`, etc.).
    #[serde(default)]
    pub has_pseudo_element: bool,
}

/// CSS selector combinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectorCombinator {
    /// ` ` (space) — descendant
    Descendant,
    /// `>` — child
    Child,
    /// `+` — adjacent sibling
    NextSibling,
    /// `~` — general sibling
    LaterSibling,
}

/// CSS pseudo-class classification.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectorPseudoClass {
    /// `:not(...)` — negation with inner selectors
    Not(Vec<StructuredSelector>),
    /// `:is(...)` — matches-any with inner selectors
    Is(Vec<StructuredSelector>),
    /// `:where(...)` — zero-specificity matches-any
    Where(Vec<StructuredSelector>),
    /// Runtime pseudo-class (`:hover`, `:focus`, `:first-child`, etc.)
    Runtime(String),
}

/// CSS attribute selector.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeSelector {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<AttributeOperator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// CSS attribute selector operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttributeOperator {
    /// `=` exact match
    Equal,
    /// `~=` includes word
    Includes,
    /// `|=` dash-match (e.g., lang|="en")
    DashMatch,
    /// `^=` starts with
    Prefix,
    /// `$=` ends with
    Suffix,
    /// `*=` contains substring
    Substring,
}

/// A CSS class selector occurrence.
#[derive(Debug, Clone)]
pub struct AnalyzedCssClass {
    pub name: String,
    /// SFC-absolute byte span of the class name (after `.`).
    pub span: Span,
}

impl serde::Serialize for AnalyzedCssClass {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AnalyzedCssClass", 3)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedCssClass {
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

/// A CSS ID selector occurrence.
#[derive(Debug, Clone)]
pub struct AnalyzedCssId {
    pub name: String,
    /// SFC-absolute byte span of the ID name (after `#`).
    pub span: Span,
}

impl serde::Serialize for AnalyzedCssId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AnalyzedCssId", 3)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedCssId {
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

/// A CSS custom property (variable) declaration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedCustomProperty {
    /// Includes the `--` prefix.
    pub name: String,
    /// SFC-absolute span of the `--name` portion.
    pub name_span: Span,
    /// Raw trimmed value text (e.g. `"red"`, `"var(--other) 10px"`).
    pub value: String,
    /// SFC-absolute span of the value text.
    pub value_span: Span,
    /// `var()` references within the value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub var_references: Vec<CssVarReference>,
    /// Index into `CssAnalysis.selectors` for the enclosing rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_index: Option<u32>,
}

/// A `var(--name)` or `var(--name, fallback)` reference in a CSS value.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CssVarReference {
    /// Variable name including `--` prefix.
    pub name: String,
    /// SFC-absolute span of the entire `var(...)` expression.
    pub span: Span,
    /// SFC-absolute span of the variable name within `var()`.
    pub name_span: Span,
    /// Optional fallback value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<CssVarFallback>,
}

/// The fallback portion of a `var(--name, fallback)` expression.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CssVarFallback {
    /// Raw text of the fallback value.
    pub text: String,
    /// SFC-absolute span of the fallback text.
    pub span: Span,
    /// Nested `var()` references within the fallback.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested_var_references: Vec<CssVarReference>,
}

/// A `var()` usage in a non-custom-property CSS declaration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedVarUsage {
    /// The CSS property name (e.g. `"color"`, `"background"`).
    pub property_name: String,
    /// The `var()` reference details.
    pub reference: CssVarReference,
    /// Index into `CssAnalysis.selectors` for the enclosing rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_index: Option<u32>,
}

/// A CSS at-rule occurrence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedAtRule {
    pub kind: AtRuleKind,
    pub name: String,
}

/// Discriminant for CSS at-rule types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AtRuleKind {
    Media,
    Keyframes,
    Supports,
    Import,
    Layer,
    Container,
    FontFace,
    Property,
    Scope,
    Other,
}

bitflags::bitflags! {
    /// Quick-check flags for style block features.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct StyleAnalysisFlags: u16 {
        const SCOPED                = 1 << 0;
        const MODULE                = 1 << 1;
        const HAS_V_BIND            = 1 << 2;
        const HAS_DEEP              = 1 << 3;
        const HAS_GLOBAL            = 1 << 4;
        const HAS_SLOTTED           = 1 << 5;
        const HAS_CUSTOM_PROPS      = 1 << 6;
        const HAS_KEYFRAMES         = 1 << 7;
        const HAS_IMPORTS           = 1 << 8;
        const HAS_LAYERS            = 1 << 9;
        const HAS_CONTAINER_QUERIES = 1 << 10;
    }
}

/// Style preprocessor language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum StyleAnalysisLang {
    #[default]
    Css,
    Scss,
    Sass,
    Less,
    Stylus,
    Unknown,
}

// =============================================================================
// Builder Functions
// =============================================================================

/// Build style analysis for a CSS style block.
///
/// Scans `css_content` with a byte-level scanner to extract selectors, specificity,
/// classes, IDs, custom properties, and at-rules.
/// `vue_input` contains pre-extracted Vue features from verter_core.
/// All stored spans are SFC-absolute byte offsets.
pub fn build_css_style_analysis(
    css_content: &str,
    vue_input: VueStyleInput,
    scoped: bool,
    is_module: bool,
    module_name: Option<&str>,
    content_offset: u32,
) -> StyleBlockAnalysis {
    let css = scan_css(css_content, content_offset);

    let v_binds = convert_v_binds(&vue_input);
    let special_pseudos = convert_special_pseudos(&vue_input);
    let flags = derive_flags(scoped, is_module, &v_binds, &special_pseudos, css.as_ref());

    StyleBlockAnalysis {
        lang: StyleAnalysisLang::Css,
        scoped,
        is_module,
        module_name: module_name.map(|s| s.to_string()),
        content_offset,
        v_binds,
        special_pseudos,
        css,
        flags: flags.bits(),
    }
}

/// Build style analysis for a non-CSS preprocessor block.
///
/// Only stores Vue features — no CSS parsing is performed.
pub fn build_preprocessor_style_analysis(
    lang: StyleAnalysisLang,
    vue_input: VueStyleInput,
    scoped: bool,
    is_module: bool,
    module_name: Option<&str>,
    content_offset: u32,
) -> StyleBlockAnalysis {
    let v_binds = convert_v_binds(&vue_input);
    let special_pseudos = convert_special_pseudos(&vue_input);
    let flags = derive_flags(scoped, is_module, &v_binds, &special_pseudos, None);

    StyleBlockAnalysis {
        lang,
        scoped,
        is_module,
        module_name: module_name.map(|s| s.to_string()),
        content_offset,
        v_binds,
        special_pseudos,
        css: None,
        flags: flags.bits(),
    }
}

// =============================================================================
// Internal Helpers
// =============================================================================

fn convert_v_binds(input: &VueStyleInput) -> Vec<AnalyzedVBind> {
    input
        .v_binds
        .iter()
        .map(|vb| AnalyzedVBind {
            expression: vb.expression.clone(),
            quoted: vb.quoted,
            start: vb.start,
            end: vb.end,
            generated_var_name: vb.generated_var_name.clone(),
        })
        .collect()
}

fn convert_special_pseudos(input: &VueStyleInput) -> Vec<AnalyzedSpecialPseudo> {
    input
        .special_pseudos
        .iter()
        .map(|sp| AnalyzedSpecialPseudo {
            kind: sp.kind,
            start: sp.start,
            end: sp.end,
            inner: sp.inner.clone(),
        })
        .collect()
}

fn derive_flags(
    scoped: bool,
    is_module: bool,
    v_binds: &[AnalyzedVBind],
    special_pseudos: &[AnalyzedSpecialPseudo],
    css: Option<&CssAnalysis>,
) -> StyleAnalysisFlags {
    let mut flags = StyleAnalysisFlags::empty();

    if scoped {
        flags |= StyleAnalysisFlags::SCOPED;
    }
    if is_module {
        flags |= StyleAnalysisFlags::MODULE;
    }
    if !v_binds.is_empty() {
        flags |= StyleAnalysisFlags::HAS_V_BIND;
    }

    for sp in special_pseudos {
        match sp.kind {
            SpecialPseudoKind::Deep => flags |= StyleAnalysisFlags::HAS_DEEP,
            SpecialPseudoKind::Global => flags |= StyleAnalysisFlags::HAS_GLOBAL,
            SpecialPseudoKind::Slotted => flags |= StyleAnalysisFlags::HAS_SLOTTED,
        }
    }

    if let Some(css) = css {
        if !css.custom_properties.is_empty() {
            flags |= StyleAnalysisFlags::HAS_CUSTOM_PROPS;
        }
        for at_rule in &css.at_rules {
            match at_rule.kind {
                AtRuleKind::Keyframes => flags |= StyleAnalysisFlags::HAS_KEYFRAMES,
                AtRuleKind::Import => flags |= StyleAnalysisFlags::HAS_IMPORTS,
                AtRuleKind::Layer => flags |= StyleAnalysisFlags::HAS_LAYERS,
                AtRuleKind::Container => flags |= StyleAnalysisFlags::HAS_CONTAINER_QUERIES,
                _ => {}
            }
        }
    }

    flags
}

// =============================================================================
// CSS Variable Reference Extraction
// =============================================================================

/// Maximum nesting depth for `var()` in fallbacks (prevents runaway recursion).
const MAX_VAR_NESTING_DEPTH: u8 = 8;

/// Extract all `var(--name)` and `var(--name, fallback)` references from a CSS value string.
///
/// `offset_in_css` is the byte offset of `value_text` within the CSS content.
/// `content_offset` is the SFC-absolute offset of the style block content start.
/// All returned spans are SFC-absolute.
pub fn extract_var_references(
    value_text: &str,
    offset_in_css: u32,
    content_offset: u32,
) -> Vec<CssVarReference> {
    let mut refs = Vec::new();
    extract_var_references_inner(value_text, offset_in_css, content_offset, 0, &mut refs);
    refs
}

fn extract_var_references_inner(
    text: &str,
    base_offset: u32,
    content_offset: u32,
    depth: u8,
    out: &mut Vec<CssVarReference>,
) {
    if depth >= MAX_VAR_NESTING_DEPTH {
        return;
    }

    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i + 4 <= len {
        // Look for "var("
        if bytes[i] == b'v' && bytes[i + 1] == b'a' && bytes[i + 2] == b'r' && bytes[i + 3] == b'('
        {
            let var_start = i;
            i += 4; // skip "var("

            // Skip whitespace
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }

            // Expect --
            if i + 1 >= len || bytes[i] != b'-' || bytes[i + 1] != b'-' {
                i += 1;
                continue;
            }

            let name_start = i;
            i += 2; // skip --
            while i < len && is_css_ident_char(bytes[i]) {
                i += 1;
            }
            let name_end = i;
            let var_name = &text[name_start..name_end];

            // Skip whitespace after name
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }

            // Check for fallback (comma) or close paren
            let fallback = if i < len && bytes[i] == b',' {
                i += 1; // skip comma
                        // Skip whitespace after comma
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                let fallback_start = i;

                // Find matching close paren, tracking nesting
                let mut paren_depth = 1u32;
                while i < len && paren_depth > 0 {
                    match bytes[i] {
                        b'(' => paren_depth += 1,
                        b')' => paren_depth -= 1,
                        _ => {}
                    }
                    if paren_depth > 0 {
                        i += 1;
                    }
                }
                let fallback_end = i;
                let fallback_text = text[fallback_start..fallback_end].trim();

                // Extract nested var() from fallback
                let mut nested = Vec::new();
                if !fallback_text.is_empty() {
                    let fb_offset_in_text =
                        fallback_text.as_ptr() as usize - text.as_ptr() as usize;
                    extract_var_references_inner(
                        fallback_text,
                        base_offset + fb_offset_in_text as u32,
                        content_offset,
                        depth + 1,
                        &mut nested,
                    );
                }

                let fb_abs_start = base_offset + fallback_start as u32 + content_offset;
                let fb_abs_end = base_offset + fallback_end as u32 + content_offset;

                Some(CssVarFallback {
                    text: fallback_text.to_string(),
                    span: Span::new(fb_abs_start, fb_abs_end),
                    nested_var_references: nested,
                })
            } else {
                // Find closing paren
                while i < len && bytes[i] != b')' {
                    i += 1;
                }
                None
            };

            // Skip closing paren
            let var_end = if i < len && bytes[i] == b')' {
                i += 1;
                i
            } else {
                i
            };

            let abs = |local: usize| -> u32 { base_offset + local as u32 + content_offset };

            out.push(CssVarReference {
                name: var_name.to_string(),
                span: Span::new(abs(var_start), abs(var_end)),
                name_span: Span::new(abs(name_start), abs(name_end)),
                fallback,
            });
            continue;
        }

        i += 1;
    }
}

// =============================================================================
// Selector String Parser
// =============================================================================

/// Parse a CSS selector string into a structured representation.
///
/// Returns `None` for unparseable selectors (e.g., containing `:has()`).
/// Handles: `.foo`, `.foo.bar`, `div.active`, `#app`, `.parent .child`,
/// `.parent > .child`, `.a + .b`, `.a ~ .b`, `[type="text"]`,
/// `:not(...)`, `:is(...)`, `:where(...)`, `:hover`, `::before`, `*`.
pub fn parse_selector(selector_text: &str) -> Option<StructuredSelector> {
    let trimmed = selector_text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let bytes = trimmed.as_bytes();
    let len = bytes.len();
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut current = CompoundSelector::default();
    let mut i = 0;
    let mut has_simple_selector = false;

    while i < len {
        let b = bytes[i];

        // Skip whitespace — it could be a descendant combinator
        if b.is_ascii_whitespace() {
            i += 1;
            // Skip all whitespace
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= len {
                break;
            }
            // Check if next char is a combinator
            let next = bytes[i];
            if next == b'>' || next == b'+' || next == b'~' {
                // Explicit combinator follows — whitespace was just padding
                continue;
            }
            // Whitespace IS the descendant combinator
            if has_simple_selector {
                compounds.push(std::mem::take(&mut current));
                combinators.push(SelectorCombinator::Descendant);
                has_simple_selector = false;
            }
            continue;
        }

        // Explicit combinators
        if b == b'>' || b == b'+' || b == b'~' {
            if has_simple_selector {
                compounds.push(std::mem::take(&mut current));
                has_simple_selector = false;
            }
            let combinator = match b {
                b'>' => SelectorCombinator::Child,
                b'+' => SelectorCombinator::NextSibling,
                b'~' => SelectorCombinator::LaterSibling,
                _ => unreachable!(),
            };
            combinators.push(combinator);
            i += 1;
            // Skip trailing whitespace after combinator
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            continue;
        }

        // * — universal selector
        if b == b'*' {
            // Universal selector — don't set element, just mark we have something
            has_simple_selector = true;
            i += 1;
            continue;
        }

        // # — ID selector
        if b == b'#' && i + 1 < len && is_css_ident_start(bytes[i + 1]) {
            let start = i + 1;
            let mut end = start;
            while end < len && is_css_ident_char(bytes[end]) {
                end += 1;
            }
            let name = &trimmed[start..end];
            if !is_hex_color(name) {
                current.id = Some(name.to_string());
                has_simple_selector = true;
            }
            i = end;
            continue;
        }

        // . — class selector
        if b == b'.' && i + 1 < len && is_css_ident_start(bytes[i + 1]) {
            let start = i + 1;
            let mut end = start;
            while end < len && is_css_ident_char(bytes[end]) {
                end += 1;
            }
            current.classes.push(trimmed[start..end].to_string());
            has_simple_selector = true;
            i = end;
            continue;
        }

        // [ — attribute selector
        if b == b'[' {
            if let Some((attr, consumed)) = parse_attribute_selector(&trimmed[i..]) {
                current.attributes.push(attr);
                has_simple_selector = true;
                i += consumed;
            } else {
                // Unparseable attribute selector
                return None;
            }
            continue;
        }

        // : — pseudo-class or pseudo-element
        if b == b':' {
            if i + 1 < len && bytes[i + 1] == b':' {
                // :: pseudo-element
                i += 2;
                while i < len && is_css_ident_char(bytes[i]) {
                    i += 1;
                }
                // Skip functional pseudo-element arguments
                if i < len && bytes[i] == b'(' {
                    let close = find_matching_paren(&trimmed[i..]);
                    i += close;
                }
                current.has_pseudo_element = true;
                has_simple_selector = true;
                continue;
            }

            // Single : pseudo-class
            i += 1;
            let pseudo_start = i;
            while i < len && is_css_ident_char(bytes[i]) {
                i += 1;
            }
            let pseudo_name = &trimmed[pseudo_start..i];

            match pseudo_name {
                "not" | "is" | "where" => {
                    if i < len && bytes[i] == b'(' {
                        let inner_start = i + 1;
                        let paren_consumed = find_matching_paren(&trimmed[i..]);
                        let inner_end = i + paren_consumed - 1; // before closing )
                        let inner = &trimmed[inner_start..inner_end];
                        i += paren_consumed;

                        // Parse inner selector list
                        let inner_selectors = split_selector_list(inner);
                        let mut parsed_inner = Vec::new();
                        for inner_sel in inner_selectors {
                            let inner_trimmed = inner_sel.trim();
                            if !inner_trimmed.is_empty() {
                                if let Some(parsed) = parse_selector(inner_trimmed) {
                                    parsed_inner.push(parsed);
                                }
                            }
                        }

                        let pseudo = match pseudo_name {
                            "not" => SelectorPseudoClass::Not(parsed_inner),
                            "is" => SelectorPseudoClass::Is(parsed_inner),
                            "where" => SelectorPseudoClass::Where(parsed_inner),
                            _ => unreachable!(),
                        };
                        current.pseudo_classes.push(pseudo);
                    } else {
                        current
                            .pseudo_classes
                            .push(SelectorPseudoClass::Runtime(pseudo_name.to_string()));
                    }
                    has_simple_selector = true;
                    continue;
                }
                "has" => {
                    // :has() is too complex for our matcher — bail out
                    return None;
                }
                _ => {
                    // Skip functional pseudo-class arguments
                    if i < len && bytes[i] == b'(' {
                        let paren_consumed = find_matching_paren(&trimmed[i..]);
                        i += paren_consumed;
                    }
                    current
                        .pseudo_classes
                        .push(SelectorPseudoClass::Runtime(pseudo_name.to_string()));
                    has_simple_selector = true;
                    continue;
                }
            }
        }

        // Type selector (element name)
        if is_css_ident_start(b) {
            let start = i;
            i += 1;
            while i < len && is_css_ident_char(bytes[i]) {
                i += 1;
            }
            current.element = Some(trimmed[start..i].to_string());
            has_simple_selector = true;
            continue;
        }

        // Skip unknown characters
        i += 1;
    }

    if has_simple_selector {
        compounds.push(current);
    }

    if compounds.is_empty() {
        return None;
    }

    Some(StructuredSelector {
        compounds,
        combinators,
    })
}

/// Parse an attribute selector starting at `[`.
/// Returns `(AttributeSelector, bytes_consumed)` or `None`.
fn parse_attribute_selector(s: &str) -> Option<(AttributeSelector, usize)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[0] != b'[' {
        return None;
    }

    let mut i = 1; // skip [
    let len = bytes.len();

    // Skip whitespace
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    // Attribute name
    let name_start = i;
    while i < len && is_css_ident_char(bytes[i]) {
        i += 1;
    }
    let name = s[name_start..i].to_string();

    // Skip whitespace
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    // Check for ] (presence-only selector)
    if i < len && bytes[i] == b']' {
        return Some((
            AttributeSelector {
                name,
                operator: None,
                value: None,
            },
            i + 1,
        ));
    }

    // Operator
    let operator = if i < len {
        match bytes[i] {
            b'=' => {
                i += 1;
                Some(AttributeOperator::Equal)
            }
            b'~' if i + 1 < len && bytes[i + 1] == b'=' => {
                i += 2;
                Some(AttributeOperator::Includes)
            }
            b'|' if i + 1 < len && bytes[i + 1] == b'=' => {
                i += 2;
                Some(AttributeOperator::DashMatch)
            }
            b'^' if i + 1 < len && bytes[i + 1] == b'=' => {
                i += 2;
                Some(AttributeOperator::Prefix)
            }
            b'$' if i + 1 < len && bytes[i + 1] == b'=' => {
                i += 2;
                Some(AttributeOperator::Suffix)
            }
            b'*' if i + 1 < len && bytes[i + 1] == b'=' => {
                i += 2;
                Some(AttributeOperator::Substring)
            }
            _ => None,
        }
    } else {
        None
    };

    // Skip whitespace
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    // Value
    let value = if i < len && (bytes[i] == b'"' || bytes[i] == b'\'') {
        let quote = bytes[i];
        i += 1;
        let val_start = i;
        while i < len && bytes[i] != quote {
            if bytes[i] == b'\\' && i + 1 < len {
                i += 2;
            } else {
                i += 1;
            }
        }
        let val = s[val_start..i].to_string();
        if i < len {
            i += 1; // skip closing quote
        }
        Some(val)
    } else if i < len && bytes[i] != b']' {
        // Unquoted value
        let val_start = i;
        while i < len && !bytes[i].is_ascii_whitespace() && bytes[i] != b']' {
            i += 1;
        }
        Some(s[val_start..i].to_string())
    } else {
        None
    };

    // Skip to ]
    while i < len && bytes[i] != b']' {
        i += 1;
    }
    if i < len {
        i += 1; // skip ]
    }

    Some((
        AttributeSelector {
            name,
            operator,
            value,
        },
        i,
    ))
}

/// Find the matching closing parenthesis, returning bytes consumed including both parens.
fn find_matching_paren(s: &str) -> usize {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[0] != b'(' {
        return 0;
    }

    let mut depth = 0;
    let mut in_string = false;
    let mut string_char = b'"';

    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if b == b'\\' {
                continue;
            }
            if b == string_char {
                in_string = false;
            }
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_string = true;
            string_char = b;
            continue;
        }
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                return i + 1;
            }
        }
    }

    bytes.len()
}

/// Compute specificity from a `StructuredSelector`.
pub fn compute_structured_specificity(selector: &StructuredSelector) -> (u32, u32, u32) {
    let mut a: u32 = 0; // IDs
    let mut b: u32 = 0; // classes, attributes, pseudo-classes
    let mut c: u32 = 0; // type selectors, pseudo-elements

    for compound in &selector.compounds {
        if compound.id.is_some() {
            a += 1;
        }
        b += compound.classes.len() as u32;
        b += compound.attributes.len() as u32;

        for pseudo in &compound.pseudo_classes {
            match pseudo {
                SelectorPseudoClass::Not(inner) | SelectorPseudoClass::Is(inner) => {
                    // :not() and :is() contribute the max specificity of their arguments
                    let (max_a, max_b, max_c) = inner
                        .iter()
                        .map(compute_structured_specificity)
                        .fold((0, 0, 0), |acc, s| {
                            (acc.0.max(s.0), acc.1.max(s.1), acc.2.max(s.2))
                        });
                    a += max_a;
                    b += max_b;
                    c += max_c;
                }
                SelectorPseudoClass::Where(_) => {
                    // :where() contributes zero specificity
                }
                SelectorPseudoClass::Runtime(_) => {
                    b += 1;
                }
            }
        }

        if compound.element.is_some() {
            c += 1;
        }
        if compound.has_pseudo_element {
            c += 1;
        }
    }

    (a, b, c)
}

/// Scan CSS content with a byte-level scanner and extract analysis data.
/// Returns `None` only for completely empty input.
fn scan_css(css_content: &str, content_offset: u32) -> Option<CssAnalysis> {
    let bytes = css_content.as_bytes();
    let len = bytes.len();

    if len == 0 {
        return Some(CssAnalysis::default());
    }

    let mut analysis = CssAnalysis::default();
    let mut i = 0;
    let mut in_string = false;
    let mut string_char = b'"';
    let mut in_comment = false;
    let mut brace_depth: usize = 0;
    // Track @keyframes depth to skip keyframe selectors (from, to, %)
    let mut keyframes_depth: usize = 0;
    let mut keyframes_entry_depths: Vec<usize> = Vec::new();
    // Track selector start (text between } / start-of-file and {)
    let mut selector_start: usize = 0;
    // Track declaration block start for custom property scanning
    let mut decl_block_start: usize = 0;
    // Track end of last statement (`;`) inside rule blocks, so nested selectors
    // start after the last property declaration, not from the opening `{`.
    let mut last_statement_end: usize = 0;
    // Track the selector index for the current rule block (for linking custom properties)
    let mut current_selector_index: Option<u32> = None;
    // At-rule depth stack: at_rule_entry_depths[i] = brace_depth where the at-rule block began
    // This lets us know when we're inside an at-rule block (not at selector level)
    let mut pending_at_rule: Option<(AtRuleKind, String)> = None;

    while i < len {
        let b = bytes[i];

        // Handle comments
        if in_comment {
            if b == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                in_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        // Handle strings
        if in_string {
            if b == b'\\' && i + 1 < len {
                i += 2; // skip escaped char
            } else if b == string_char {
                in_string = false;
                i += 1;
            } else {
                i += 1;
            }
            continue;
        }

        // Comment start
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            in_comment = true;
            i += 2;
            continue;
        }

        // String start
        if b == b'"' || b == b'\'' {
            in_string = true;
            string_char = b;
            i += 1;
            continue;
        }

        // Handle semicolons at top level for @import, @layer statement
        if b == b';' && brace_depth == 0 {
            if let Some((kind, name)) = pending_at_rule.take() {
                // At-rule without block (e.g., @import "x"; or @layer foo;)
                if kind == AtRuleKind::Import {
                    // Extract the URL from the at-rule text
                    let at_text = &css_content[selector_start..i];
                    let import_name = extract_import_url(at_text);
                    analysis.at_rules.push(AnalyzedAtRule {
                        kind,
                        name: import_name,
                    });
                } else {
                    analysis.at_rules.push(AnalyzedAtRule { kind, name });
                }
            }
            selector_start = i + 1;
            i += 1;
            continue;
        }

        // Track semicolons inside rule blocks for nested selector start position
        if b == b';' && brace_depth > 0 {
            last_statement_end = i + 1;
            i += 1;
            continue;
        }

        // Opening brace
        if b == b'{' {
            // For nested selectors (brace_depth > 0), use last_statement_end
            // so we don't capture property declarations in the selector text.
            let effective_start = if brace_depth > 0 && last_statement_end > selector_start {
                last_statement_end
            } else {
                selector_start
            };
            let selector_text = &css_content[effective_start..i];
            let trimmed = selector_text.trim();

            if !trimmed.is_empty() {
                if let Some(at_rule) = try_parse_at_rule(trimmed) {
                    // At-rule with block
                    match at_rule.0 {
                        AtRuleKind::Keyframes => {
                            keyframes_depth += 1;
                            keyframes_entry_depths.push(brace_depth);
                            analysis.at_rules.push(AnalyzedAtRule {
                                kind: at_rule.0,
                                name: at_rule.1,
                            });
                        }
                        AtRuleKind::FontFace | AtRuleKind::Property => {
                            // These at-rules have declaration blocks, not nested rules
                            analysis.at_rules.push(AnalyzedAtRule {
                                kind: at_rule.0,
                                name: at_rule.1,
                            });
                        }
                        _ => {
                            // Block at-rules that can contain nested rules (@media, @supports, etc.)
                            analysis.at_rules.push(AnalyzedAtRule {
                                kind: at_rule.0,
                                name: at_rule.1,
                            });
                        }
                    }
                    pending_at_rule = None;
                } else if keyframes_depth == 0 {
                    // Regular style rule — extract selectors
                    analysis.rule_count += 1;

                    // Strip CSS comments from selector text before parsing.
                    // This prevents comments like `.a /* comment */ > .b`
                    // from corrupting selector structure.
                    let clean_selector;
                    let selector_for_parse = if let Some(cleaned) = strip_css_comments(trimmed) {
                        clean_selector = cleaned;
                        clean_selector.trim()
                    } else {
                        trimmed
                    };

                    // Split comma-separated selectors
                    let css_base = css_content.as_ptr() as usize;
                    let individual_selectors = split_selector_list(selector_for_parse);
                    for sel_text in &individual_selectors {
                        let sel_trimmed = sel_text.trim();
                        if !sel_trimmed.is_empty() {
                            // If we're using the original (no comments), pointer
                            // arithmetic gives exact span. Otherwise, use the
                            // whole selector range as a fallback.
                            let sel_offset = if std::ptr::eq(
                                selector_for_parse.as_bytes().as_ptr(),
                                trimmed.as_bytes().as_ptr(),
                            ) {
                                (sel_trimmed.as_ptr() as usize - css_base) as u32
                            } else {
                                (trimmed.as_ptr() as usize - css_base) as u32
                            };
                            let structure = parse_selector(sel_trimmed);
                            let specificity = if let Some(ref s) = structure {
                                compute_structured_specificity(s)
                            } else {
                                compute_specificity_from_text(sel_trimmed)
                            };
                            analysis.selectors.push(AnalyzedSelector {
                                text: sel_trimmed.to_string(),
                                specificity,
                                span: Span::new(
                                    content_offset + sel_offset,
                                    content_offset + sel_offset + sel_trimmed.len() as u32,
                                ),
                                structure,
                            });
                        }
                    }

                    // Extract class/ID names from selector text with spans
                    let selector_offset =
                        (selector_text.as_ptr() as usize) - (css_content.as_ptr() as usize);
                    extract_classes_and_ids_from_selector(
                        selector_text,
                        selector_offset,
                        content_offset,
                        &mut analysis,
                    );

                    // Record the first selector index for this rule block
                    // (for linking custom properties back to their selector)
                    if !analysis.selectors.is_empty() {
                        current_selector_index =
                            Some((analysis.selectors.len() - individual_selectors.len()) as u32);
                    }
                }
            }

            brace_depth += 1;
            decl_block_start = i + 1;
            selector_start = i + 1;
            i += 1;
            continue;
        }

        // Closing brace
        if b == b'}' {
            // Scan for declarations in the block we're closing
            if keyframes_depth == 0 && brace_depth > 0 {
                let decl_content = &css_content[decl_block_start..i];
                scan_declarations(
                    decl_content,
                    decl_block_start as u32,
                    content_offset,
                    current_selector_index,
                    &mut analysis,
                );
            }

            brace_depth = brace_depth.saturating_sub(1);
            if keyframes_entry_depths.last() == Some(&brace_depth) {
                keyframes_entry_depths.pop();
                keyframes_depth = keyframes_depth.saturating_sub(1);
            }
            current_selector_index = None;
            selector_start = i + 1;
            decl_block_start = i + 1;
            i += 1;
            continue;
        }

        // Detect at-rule starts at the current brace depth
        if b == b'@' && (brace_depth == 0 || keyframes_depth == 0) {
            // Look ahead to get the at-rule keyword
            let at_start = i;
            i += 1; // skip '@'
            while i < len && is_css_ident_char(bytes[i]) {
                i += 1;
            }
            let keyword = &css_content[at_start + 1..i];

            match keyword {
                "import" => {
                    pending_at_rule = Some((AtRuleKind::Import, String::new()));
                }
                "layer" => {
                    // Could be @layer statement or @layer block — determined at ; or {
                    // Extract the name part
                    let name_start = i;
                    // Scan ahead for ; or { to get the name
                    let mut j = i;
                    while j < len && bytes[j] != b';' && bytes[j] != b'{' {
                        j += 1;
                    }
                    let name = css_content[name_start..j].trim().to_string();
                    pending_at_rule = Some((AtRuleKind::Layer, name));
                }
                _ => {
                    // Other at-rules parsed when we hit {
                }
            }
            continue;
        }

        i += 1;
    }

    // Handle any trailing at-rule without ; (malformed but graceful)
    if let Some((kind, name)) = pending_at_rule.take() {
        analysis.at_rules.push(AnalyzedAtRule { kind, name });
    }

    Some(analysis)
}

/// Try to parse an at-rule from trimmed text before `{`.
/// Returns `(AtRuleKind, name)` if this is an at-rule, `None` if it's a regular selector.
fn try_parse_at_rule(trimmed: &str) -> Option<(AtRuleKind, String)> {
    if !trimmed.starts_with('@') {
        return None;
    }

    let rest = &trimmed[1..];

    if let Some(after) = rest.strip_prefix("media") {
        if after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || c == '(') {
            let name = after.trim().to_string();
            return Some((AtRuleKind::Media, name));
        }
    }

    if let Some(after) = rest.strip_prefix("keyframes") {
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            let name = after.trim().to_string();
            return Some((AtRuleKind::Keyframes, name));
        }
    }

    if let Some(after) = rest.strip_prefix("-webkit-keyframes") {
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            let name = after.trim().to_string();
            return Some((AtRuleKind::Keyframes, name));
        }
    }

    if let Some(after) = rest.strip_prefix("supports") {
        if after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || c == '(') {
            return Some((AtRuleKind::Supports, String::new()));
        }
    }

    if let Some(after) = rest.strip_prefix("layer") {
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            let name = after.trim().to_string();
            return Some((AtRuleKind::Layer, name));
        }
    }

    if let Some(after) = rest.strip_prefix("container") {
        if after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || c == '(') {
            let name = extract_container_name(after.trim());
            return Some((AtRuleKind::Container, name));
        }
    }

    if let Some(after) = rest.strip_prefix("font-face") {
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            return Some((AtRuleKind::FontFace, String::new()));
        }
    }

    if let Some(after) = rest.strip_prefix("property") {
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            let name = after.trim().to_string();
            return Some((AtRuleKind::Property, name));
        }
    }

    if let Some(after) = rest.strip_prefix("scope") {
        if after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || c == '(') {
            return Some((AtRuleKind::Scope, String::new()));
        }
    }

    // Unknown at-rule
    Some((AtRuleKind::Other, String::new()))
}

/// Extract the container name from a `@container` query.
/// e.g., "sidebar (min-width: 300px)" → "sidebar"
fn extract_container_name(query: &str) -> String {
    // The name is the first ident before `(` or whitespace+`(`
    let query = query.trim();
    if query.is_empty() || query.starts_with('(') {
        return String::new();
    }
    // Take everything before the first `(`
    let before_paren = if let Some(idx) = query.find('(') {
        query[..idx].trim()
    } else {
        query
    };
    before_paren.to_string()
}

/// Extract the URL from an @import at-rule text.
/// e.g., `@import "base.css"` → `base.css`, `@import url("x.css")` → `x.css`
fn extract_import_url(at_text: &str) -> String {
    let text = at_text.trim();
    let rest = text.strip_prefix("@import").unwrap_or(text).trim_start();

    // url("...") or url('...')
    if let Some(inner) = rest.strip_prefix("url(") {
        let inner = inner.trim_start();
        if let Some(s) = extract_quoted_string(inner) {
            return s;
        }
        // url(foo.css) without quotes
        if let Some(end) = inner.find(')') {
            return inner[..end].trim().to_string();
        }
    }

    // "..." or '...'
    if let Some(s) = extract_quoted_string(rest) {
        return s;
    }

    rest.to_string()
}

/// Extract a quoted string value, stripping the quotes.
fn extract_quoted_string(s: &str) -> Option<String> {
    let s = s.trim_start();
    if (s.starts_with('"') || s.starts_with('\'')) && s.len() >= 2 {
        let quote = s.as_bytes()[0];
        if let Some(end) = s[1..].find(|c: char| c as u8 == quote) {
            return Some(s[1..1 + end].to_string());
        }
    }
    None
}

/// Strip CSS block comments (`/* ... */`) from selector text.
///
/// Returns `None` if no comments were found (original string is clean),
/// or `Some(cleaned)` with comments replaced by a single space.
fn strip_css_comments(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut has_comment = false;

    // Quick scan — avoid allocation if no comments
    while i + 1 < len {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            has_comment = true;
            break;
        }
        i += 1;
    }
    if !has_comment {
        return None;
    }

    // Build cleaned string
    let mut result = String::with_capacity(len);
    i = 0;
    while i < len {
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Skip comment contents
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip */
            }
            result.push(' '); // replace comment with single space
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    Some(result)
}

/// Split a selector list by commas, respecting parentheses nesting.
/// e.g., ".a, .b, :not(.c, .d)" → [".a", ".b", ":not(.c, .d)"]
fn split_selector_list(selector: &str) -> Vec<&str> {
    let bytes = selector.as_bytes();
    let mut result = Vec::new();
    let mut start = 0;
    let mut paren_depth: usize = 0;
    let mut bracket_depth: usize = 0;
    let mut in_string = false;
    let mut string_char = b'"';

    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if b == b'\\' {
                // Skip next char (handled by not checking next iteration)
                continue;
            }
            if b == string_char {
                in_string = false;
            }
            continue;
        }

        if b == b'"' || b == b'\'' {
            in_string = true;
            string_char = b;
            continue;
        }

        if b == b'(' {
            paren_depth += 1;
        } else if b == b')' {
            paren_depth = paren_depth.saturating_sub(1);
        } else if b == b'[' {
            bracket_depth += 1;
        } else if b == b']' {
            bracket_depth = bracket_depth.saturating_sub(1);
        } else if b == b',' && paren_depth == 0 && bracket_depth == 0 {
            result.push(&selector[start..i]);
            start = i + 1;
        }
    }

    result.push(&selector[start..]);
    result
}

/// Compute CSS specificity from selector text.
/// Returns (id_count, class_count, type_count).
fn compute_specificity_from_text(selector: &str) -> (u32, u32, u32) {
    let bytes = selector.as_bytes();
    let len = bytes.len();
    let mut ids: u32 = 0;
    let mut classes: u32 = 0;
    let mut types: u32 = 0;
    let mut i = 0;
    let mut in_string = false;
    let mut string_char = b'"';
    let mut paren_depth: usize = 0;
    // Track functional pseudo-classes that affect specificity
    let mut where_depth: Vec<usize> = Vec::new(); // :where() contributes 0 specificity

    while i < len {
        let b = bytes[i];

        if in_string {
            if b == b'\\' && i + 1 < len {
                i += 2;
                continue;
            }
            if b == string_char {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if b == b'"' || b == b'\'' {
            in_string = true;
            string_char = b;
            i += 1;
            continue;
        }

        if b == b'(' {
            paren_depth += 1;
            i += 1;
            continue;
        }

        if b == b')' {
            paren_depth = paren_depth.saturating_sub(1);
            if where_depth.last() == Some(&paren_depth) {
                where_depth.pop();
            }
            i += 1;
            continue;
        }

        // Inside :where() — skip all specificity
        if !where_depth.is_empty() {
            i += 1;
            continue;
        }

        // # → ID selector
        if b == b'#' && i + 1 < len && is_css_ident_start(bytes[i + 1]) {
            let name_start = i + 1;
            let mut end = name_start;
            while end < len && is_css_ident_char(bytes[end]) {
                end += 1;
            }
            let name = &selector[name_start..end];
            if !is_hex_color(name) {
                ids += 1;
            }
            i = end;
            continue;
        }

        // . → class selector
        if b == b'.' && i + 1 < len && is_css_ident_start(bytes[i + 1]) {
            // Skip decimal numbers
            if i > 0 && bytes[i - 1].is_ascii_digit() {
                i += 1;
                continue;
            }
            classes += 1;
            i += 1;
            while i < len && is_css_ident_char(bytes[i]) {
                i += 1;
            }
            continue;
        }

        // [ → attribute selector (contributes to class-level specificity)
        if b == b'[' {
            classes += 1;
            // Skip to matching ]
            i += 1;
            let mut bracket_string = false;
            let mut bracket_string_char = b'"';
            while i < len {
                if bracket_string {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == bracket_string_char {
                        bracket_string = false;
                    }
                } else if bytes[i] == b'"' || bytes[i] == b'\'' {
                    bracket_string = true;
                    bracket_string_char = bytes[i];
                } else if bytes[i] == b']' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // : → pseudo-class or pseudo-element
        if b == b':' {
            if i + 1 < len && bytes[i + 1] == b':' {
                // :: pseudo-element → type-level specificity
                types += 1;
                i += 2;
                while i < len && is_css_ident_char(bytes[i]) {
                    i += 1;
                }
                continue;
            }

            // Single : → pseudo-class
            i += 1;
            let pseudo_start = i;
            while i < len && is_css_ident_char(bytes[i]) {
                i += 1;
            }
            let pseudo_name = &selector[pseudo_start..i];

            match pseudo_name {
                "where" => {
                    // :where() contributes 0 specificity
                    if i < len && bytes[i] == b'(' {
                        where_depth.push(paren_depth);
                    }
                }
                "not" | "is" | "has" => {
                    // :not(), :is(), :has() — specificity of most specific argument
                    // For simplicity in this pass, we count inner selectors normally
                    // (the max-of-args rule is handled by the fact that we're processing
                    // the full text which includes the inner selectors)
                    // Don't count the pseudo-class itself
                }
                _ => {
                    // Regular pseudo-class → class-level specificity
                    classes += 1;
                }
            }
            continue;
        }

        // * → universal selector (0 specificity)
        if b == b'*' {
            i += 1;
            continue;
        }

        // Whitespace / combinators
        if b == b' ' || b == b'>' || b == b'+' || b == b'~' {
            i += 1;
            continue;
        }

        // Otherwise: could be a type selector
        if is_css_ident_start(b) {
            types += 1;
            i += 1;
            while i < len && is_css_ident_char(bytes[i]) {
                i += 1;
            }
            continue;
        }

        i += 1;
    }

    (ids, classes, types)
}

/// Extract `.class` and `#id` occurrences from selector text, recording
/// SFC-absolute byte spans.
fn extract_classes_and_ids_from_selector(
    selector_text: &str,
    offset_in_css: usize,
    content_offset: u32,
    analysis: &mut CssAnalysis,
) {
    let bytes = selector_text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_string = false;
    let mut string_char = b'"';

    while i < len {
        let b = bytes[i];

        if in_string {
            if b == b'\\' && i + 1 < len {
                i += 2;
                continue;
            }
            if b == string_char {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if b == b'"' || b == b'\'' {
            in_string = true;
            string_char = b;
            i += 1;
            continue;
        }

        if b == b'.' || b == b'#' {
            let name_start = i + 1;
            if name_start < len && is_css_ident_start(bytes[name_start]) {
                // Skip decimal numbers
                if b == b'.' && i > 0 && bytes[i - 1].is_ascii_digit() {
                    i += 1;
                    continue;
                }

                let mut end = name_start;
                while end < len && is_css_ident_char(bytes[end]) {
                    end += 1;
                }

                let name = &selector_text[name_start..end];

                // Skip hex colors for #
                if b == b'#' && is_hex_color(name) {
                    i = end;
                    continue;
                }

                let abs_start = content_offset + (offset_in_css + name_start) as u32;
                let abs_end = content_offset + (offset_in_css + end) as u32;

                if b == b'.' {
                    analysis.classes.push(AnalyzedCssClass {
                        name: name.to_string(),
                        span: Span::new(abs_start, abs_end),
                    });
                } else {
                    analysis.ids.push(AnalyzedCssId {
                        name: name.to_string(),
                        span: Span::new(abs_start, abs_end),
                    });
                }

                i = end;
                continue;
            }
        }

        i += 1;
    }
}

/// Scan a declaration block for custom property declarations and var() usages.
///
/// - Custom property declarations (`--name: value`) are added to `analysis.custom_properties`
///   with full details (value, spans, var_references, selector_index).
/// - Non-custom-property declarations containing `var()` are added to `analysis.var_usages`.
///
/// `decl_block_offset` is the byte offset of `decl_content` within `css_content`.
/// `content_offset` is the SFC-absolute offset of the style block content start.
fn scan_declarations(
    decl_content: &str,
    decl_block_offset: u32,
    content_offset: u32,
    selector_index: Option<u32>,
    analysis: &mut CssAnalysis,
) {
    let bytes = decl_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_string = false;
    let mut string_char = b'"';
    let mut in_comment = false;
    // Track whether we're at property-name position (after `;` or block start)
    let mut at_prop_start = true;
    let mut brace_depth: usize = 0;

    while i < len {
        let b = bytes[i];

        if in_comment {
            if b == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                in_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        if in_string {
            if b == b'\\' && i + 1 < len {
                i += 2;
                continue;
            }
            if b == string_char {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            in_comment = true;
            i += 2;
            continue;
        }

        if b == b'"' || b == b'\'' {
            in_string = true;
            string_char = b;
            at_prop_start = false;
            i += 1;
            continue;
        }

        if b == b'{' {
            brace_depth += 1;
            at_prop_start = true;
            i += 1;
            continue;
        }

        if b == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
            at_prop_start = true;
            i += 1;
            continue;
        }

        if b == b';' {
            at_prop_start = true;
            i += 1;
            continue;
        }

        // At property-name position, check for `--` (custom property declaration)
        if at_prop_start && b == b'-' && i + 1 < len && bytes[i + 1] == b'-' {
            // Found custom property declaration
            let name_start = i;
            i += 2; // skip `--`
            while i < len && is_css_ident_char(bytes[i]) {
                i += 1;
            }
            let name_end = i;
            let name = &decl_content[name_start..name_end];

            // Skip whitespace and colon to get to value
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < len && bytes[i] == b':' {
                i += 1; // skip ':'
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
            }

            // Extract value until `;` or `}` or end, respecting strings/comments/nested parens
            let value_start = i;
            let mut val_in_string = false;
            let mut val_string_char = b'"';
            let mut val_paren_depth = 0u32;
            while i < len {
                let vb = bytes[i];
                if val_in_string {
                    if vb == b'\\' && i + 1 < len {
                        i += 2;
                        continue;
                    }
                    if vb == val_string_char {
                        val_in_string = false;
                    }
                    i += 1;
                    continue;
                }
                if vb == b'"' || vb == b'\'' {
                    val_in_string = true;
                    val_string_char = vb;
                    i += 1;
                    continue;
                }
                if vb == b'(' {
                    val_paren_depth += 1;
                    i += 1;
                    continue;
                }
                if vb == b')' {
                    val_paren_depth = val_paren_depth.saturating_sub(1);
                    i += 1;
                    continue;
                }
                if val_paren_depth == 0 && (vb == b';' || vb == b'}') {
                    break;
                }
                i += 1;
            }
            let value_end = i;
            let value_raw = decl_content[value_start..value_end].trim();

            let abs_name_start = decl_block_offset + name_start as u32 + content_offset;
            let abs_name_end = decl_block_offset + name_end as u32 + content_offset;
            let abs_value_start = decl_block_offset
                + (value_raw.as_ptr() as usize - decl_content.as_ptr() as usize) as u32
                + content_offset;
            let abs_value_end = abs_value_start + value_raw.len() as u32;

            // Extract var() references from the value
            let value_offset_in_css = decl_block_offset
                + (value_raw.as_ptr() as usize - decl_content.as_ptr() as usize) as u32;
            let var_refs = extract_var_references(value_raw, value_offset_in_css, content_offset);

            analysis.custom_properties.push(AnalyzedCustomProperty {
                name: name.to_string(),
                name_span: Span::new(abs_name_start, abs_name_end),
                value: value_raw.to_string(),
                value_span: Span::new(abs_value_start, abs_value_end),
                var_references: var_refs,
                selector_index,
            });

            at_prop_start = false;
            continue;
        }

        // At property-name position, scan for a regular property that may contain var()
        if at_prop_start && b.is_ascii_alphabetic() && brace_depth == 0 {
            let prop_name_start = i;
            // Scan property name (letters, hyphens, digits)
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
            {
                i += 1;
            }
            let prop_name = decl_content[prop_name_start..i].trim();

            // Skip whitespace and colon
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < len && bytes[i] == b':' {
                i += 1;
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }

                // Scan value for var()
                let value_start = i;
                let mut val_in_string = false;
                let mut val_string_char = b'"';
                let mut val_paren_depth = 0u32;
                while i < len {
                    let vb = bytes[i];
                    if val_in_string {
                        if vb == b'\\' && i + 1 < len {
                            i += 2;
                            continue;
                        }
                        if vb == val_string_char {
                            val_in_string = false;
                        }
                        i += 1;
                        continue;
                    }
                    if vb == b'"' || vb == b'\'' {
                        val_in_string = true;
                        val_string_char = vb;
                        i += 1;
                        continue;
                    }
                    if vb == b'(' {
                        val_paren_depth += 1;
                        i += 1;
                        continue;
                    }
                    if vb == b')' {
                        val_paren_depth = val_paren_depth.saturating_sub(1);
                        i += 1;
                        continue;
                    }
                    if val_paren_depth == 0 && (vb == b';' || vb == b'}') {
                        break;
                    }
                    i += 1;
                }
                let value_end = i;
                let value_raw = decl_content[value_start..value_end].trim();

                // Check if value contains var()
                if value_raw.contains("var(") {
                    let value_offset_in_css = decl_block_offset
                        + (value_raw.as_ptr() as usize - decl_content.as_ptr() as usize) as u32;
                    let var_refs =
                        extract_var_references(value_raw, value_offset_in_css, content_offset);
                    for var_ref in var_refs {
                        analysis.var_usages.push(AnalyzedVarUsage {
                            property_name: prop_name.to_string(),
                            reference: var_ref,
                            selector_index,
                        });
                    }
                }
            }

            at_prop_start = false;
            continue;
        }

        if b == b':' {
            at_prop_start = false;
            i += 1;
            continue;
        }

        if b.is_ascii_whitespace() {
            // Whitespace doesn't change prop_start state
            i += 1;
            continue;
        }

        at_prop_start = false;
        i += 1;
    }
}

/// Check if a string looks like a hex color (3, 4, 6, or 8 hex digits).
fn is_hex_color(s: &str) -> bool {
    let len = s.len();
    (len == 3 || len == 4 || len == 6 || len == 8) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Check if a byte is a valid CSS identifier start character.
fn is_css_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'-' || b > 0x7F
}

/// Check if a byte is a valid CSS identifier continuation character.
fn is_css_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b > 0x7F
}

#[cfg(test)]
#[path = "style_tests.rs"]
mod style_tests;
