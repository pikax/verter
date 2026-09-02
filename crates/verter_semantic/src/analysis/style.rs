//! CSS/style analysis for Vue SFC style blocks.
//!
//! Projects the shared lossless style syntax authority into selectors, specificity,
//! classes, IDs, custom properties, and at-rules. Vue-specific
//! features (v-bind, :deep, :global, :slotted) are passed through from verter_compiler.

use verter_span::Span;

// =============================================================================
// Vue Feature Input Types (constructed by verter_session from verter_compiler output)
// =============================================================================

/// Pre-extracted Vue-specific CSS features from verter_compiler.
/// `verter_session` converts `CssParsed*` types into these.
#[derive(Debug, Clone, Default)]
pub struct VueStyleInput {
    pub v_binds: Vec<VBindInput>,
    pub special_pseudos: Vec<SpecialPseudoInput>,
}

/// A `v-bind()` expression found in CSS.
#[derive(Debug, Clone)]
pub struct VBindInput {
    /// The expression text (resolved from span by verter_session).
    pub expression: String,
    pub quoted: bool,
    pub start: u32,
    pub end: u32,
    /// Generated CSS variable name from the prepass (e.g. `"--a4f2eed6-color"`).
    pub generated_var_name: Option<String>,
    /// Free identifier roots of the expression (OXC-derived at the producer).
    pub expr_roots: Vec<String>,
    /// `false` when the expression failed to parse — consumers fail OPEN
    /// (treat every binding as style-used).
    pub roots_complete: bool,
}

/// A Vue special pseudo-class (`:deep`, `:global`, `:slotted`).
#[derive(Debug, Clone)]
pub struct SpecialPseudoInput {
    pub kind: SpecialPseudoKind,
    pub start: u32,
    pub end: u32,
    /// Inner selector text (resolved from span by verter_session).
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

/// Where a style block's content lives, and therefore whether this analysis
/// carries content facts at all.
///
/// The raw carrier parse never slices an external block's ignored inline span.
/// The host may later hydrate a native external CSS/SCSS/Sass/Less/Stylus file
/// from its registered VFS artifact, or hydrate a sealed supplied result for a
/// processed language. Until that selection is available, consumers fail open
/// for binding liveness (see
/// [`super::types::ScriptAnalysisSnapshot::mark_bindings_used_in_style`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockContentAvailability {
    /// Inline `<style>` content — analyzed in place.
    #[default]
    NativeAvailable,
    /// The selected bytes require an external processor and no sealed result
    /// has been admitted; content facts remain unavailable.
    ProcessedContentRequired,
    SuppliedAvailable,
    Missing,
    Conflict,
    Stale,
}

impl BlockContentAvailability {
    fn is_native_available(&self) -> bool {
        matches!(self, Self::NativeAvailable)
    }
}

/// Complete analysis of a single `<style>` block.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleBlockAnalysis {
    pub lang: StyleAnalysisLang,
    pub scoped: bool,
    pub is_module: bool,
    pub module_name: Option<String>,

    /// Sealed artifact-bound block identity, minted ONLY by the owning
    /// `CarrierBlockInventory`. The SOLE Rust association key between this
    /// analysis and a structure block: consumers full-identity-join
    /// (artifact identity + block id) and fail closed on missing/mismatch.
    #[serde(skip)]
    pub block_ref: Option<verter_language::parse_artifact::carrier_inventory::ArtifactBlockRef>,

    /// Opaque public block token for wire consumers (playground/FFI),
    /// attached at the host serve boundary only after the sealed ref
    /// revalidates against the live registered structure. Absent means
    /// "identity unavailable" — wire consumers fail closed, never ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_token: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_space_token: Option<String>,

    /// Byte offset of the style block content start within the SFC source.
    /// Stored only for span remapping; it is not block identity.
    #[serde(default)]
    pub content_offset: u32,

    // Vue features (from verter_compiler, all languages)
    pub v_binds: Vec<AnalyzedVBind>,
    pub special_pseudos: Vec<AnalyzedSpecialPseudo>,

    // Trusted structural analysis projected from the shared style syntax authority.
    pub css: Option<CssAnalysis>,

    /// Content availability: inline (analyzed) or external-`src` (deferred).
    /// Serialization is byte-identical for inline blocks (the default), so
    /// existing persisted payloads round-trip unchanged.
    #[serde(
        default,
        skip_serializing_if = "BlockContentAvailability::is_native_available"
    )]
    pub content_availability: BlockContentAvailability,

    pub flags: u16,
}

impl StyleBlockAnalysis {
    /// Get flags as `StyleAnalysisFlags`.
    pub fn analysis_flags(&self) -> StyleAnalysisFlags {
        StyleAnalysisFlags::from_bits_truncate(self.flags)
    }

    /// Whether bytes outside this block can still contribute to its surface.
    ///
    /// The shared style-syntax owner's single answer
    /// (`StyleSyntaxIr::pulls_in_unparsed_bytes`), recorded at the one parse
    /// that produced this analysis and read back here rather than re-derived.
    /// `false` is the strong claim, so every state with no parse behind it —
    /// an unrecognised language, a parse that yielded no IR, deferred external
    /// content — answers `true`.
    ///
    /// A consumer that publishes an inventory as exhaustive (`v-bind()`
    /// liveness, class lists) MUST fold this in. Reading the recorded
    /// `v-bind()` list alone answers the question by omission: a sheet whose
    /// `@import` sat inside a recovery window records no inclusion and no
    /// binding, and looks indistinguishable from a self-contained block.
    pub fn pulls_in_unparsed_bytes(&self) -> bool {
        self.analysis_flags()
            .contains(StyleAnalysisFlags::SURFACE_PULLS_UNPARSED_BYTES)
    }

    /// Whether this block is an external `<style src="...">` whose content
    /// analysis is deferred (unavailable) rather than an analyzed inline block.
    pub fn content_is_available(&self) -> bool {
        matches!(
            self.content_availability,
            BlockContentAvailability::NativeAvailable | BlockContentAvailability::SuppliedAvailable
        )
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
    /// Free identifier roots of the expression — the SOUND OXC-derived usage
    /// fact recorded once at the producer and consumed by style-liveness
    /// marking AND compile-input assembly (never re-derived by text split).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expr_roots: Vec<String>,
    /// `false` when the expression failed to parse — consumers fail OPEN
    /// (treat every binding as style-used, never a false unused diagnostic).
    #[serde(default)]
    pub roots_complete: bool,
}

/// Analyzed Vue special pseudo-class.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedSpecialPseudo {
    pub kind: SpecialPseudoKind,
    pub start: u32,
    pub end: u32,
    pub inner: Option<String>,
}

/// Trusted style analysis projected from the shared syntax authority.
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
    /// One record per complete declaration (custom-property or plain), the
    /// per-declaration counterpart to `custom_properties` (which only
    /// projects the `--`-prefixed subset). Consumers needing a declaration's
    /// name/value spans or pre-classified color-literal candidates read this
    /// instead of re-deriving them from raw bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declarations: Vec<AnalyzedDeclaration>,
}

impl CssAnalysis {
    /// Validate that all SFC-absolute spans are within `[0, sfc_source_len)`.
    ///
    /// This catches double-offset bugs where `content_offset` is accidentally
    /// applied twice, pushing spans beyond the SFC source boundary.
    /// Only runs in debug builds (`debug_assert!`).
    pub fn debug_assert_valid_spans(&self, sfc_source_len: u32) {
        fn check(span: Span, sfc_source_len: u32, label: &str) {
            verter_debug_assert!(
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
        for decl in &self.declarations {
            check(decl.name_span, sfc_source_len, "declaration name");
            check(decl.value_span, sfc_source_len, "declaration value");
            for candidate in &decl.color_candidates {
                check(candidate.span, sfc_source_len, "color candidate");
            }
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
    /// SFC-absolute byte span of the rule's declaration block, including both
    /// braces (`{ ... }`). `None` when the rule's block was never closed.
    pub rule_body_span: Option<Span>,
}

impl serde::Serialize for AnalyzedSelector {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 4
            + usize::from(self.structure.is_some())
            + 2 * usize::from(self.rule_body_span.is_some());
        let mut s = serializer.serialize_struct("AnalyzedSelector", count)?;
        s.serialize_field("text", &self.text)?;
        s.serialize_field("specificity", &self.specificity)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.structure.is_some() {
            s.serialize_field("structure", &self.structure)?;
        }
        if let Some(body) = self.rule_body_span {
            s.serialize_field("ruleBodyStart", &body.start)?;
            s.serialize_field("ruleBodyEnd", &body.end)?;
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
            #[serde(default)]
            rule_body_start: Option<u32>,
            #[serde(default)]
            rule_body_end: Option<u32>,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            text: w.text,
            specificity: w.specificity,
            span: Span::new(w.span_start, w.span_end),
            structure: w.structure,
            rule_body_span: match (w.rule_body_start, w.rule_body_end) {
                (Some(start), Some(end)) => Some(Span::new(start, end)),
                _ => None,
            },
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
    /// Index into `CssAnalysis.selectors` for the comma-part selector this
    /// class occurrence belongs to. `None` when the join was not derivable
    /// (comment-degraded selector text).
    pub selector_index: Option<u32>,
}

impl serde::Serialize for AnalyzedCssClass {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 3 + usize::from(self.selector_index.is_some());
        let mut s = serializer.serialize_struct("AnalyzedCssClass", count)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.selector_index.is_some() {
            s.serialize_field("selectorIndex", &self.selector_index)?;
        }
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
            #[serde(default)]
            selector_index: Option<u32>,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            span: Span::new(w.span_start, w.span_end),
            selector_index: w.selector_index,
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

/// A single complete CSS declaration (custom-property or plain), the
/// per-declaration counterpart `CssAnalysis.declarations` records for every
/// `StyleCompleteness::Complete` declaration — unlike `custom_properties`,
/// which only projects the `--`-prefixed subset.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedDeclaration {
    /// SFC-absolute span of the property/custom-property name.
    pub name_span: Span,
    /// SFC-absolute span of the trimmed value text.
    pub value_span: Span,
    /// Index into `CssAnalysis.selectors` for the enclosing rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_index: Option<u32>,
    /// Pre-classified color-literal candidates within the value, derived
    /// from the value's own typed `ComponentValueTree` — never a raw-byte
    /// scan or comment/string mask.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub color_candidates: Vec<AnalyzedColorCandidate>,
}

/// The syntactic shape of a color-literal candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ColorCandidateKind {
    /// A `#`-prefixed hex color token (e.g. `#fff`, `#ff0000`).
    Hex,
    /// An `rgb()`/`rgba()`/`hsl()`/`hsla()` function call.
    Function,
}

/// A color-literal candidate span found while walking a declaration's value.
/// Comment and string content structurally never produce a candidate — the
/// walk simply does not visit `ComponentValue::Comment`/`ComponentValue::String`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedColorCandidate {
    /// SFC-absolute span of the candidate (the hash token, or the whole
    /// function call including its parentheses).
    pub span: Span,
    pub kind: ColorCandidateKind,
    /// Lowercased function name (`"rgb"`/`"rgba"`/`"hsl"`/`"hsla"`).
    /// `None` for `ColorCandidateKind::Hex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    /// Numeric arguments read from the function's own `ComponentValue`
    /// argument list (skipping `Comment` entries structurally), in source
    /// order. [`NumericArg::Percentage`] preserves the `%` suffix (the
    /// producer never divides it out) so a consumer can distinguish a 0-100
    /// percentage scale from a bare 0-255/0-1 numeric scale rather than
    /// guessing from magnitude alone. Empty for `ColorCandidateKind::Hex`
    /// and — WHOLE-CANDIDATE invalidated, never a truncated partial list —
    /// for a function containing ANY component that isn't a `Number` token,
    /// a `Percentage` token, whitespace, a comma, or a comment: CSS
    /// relative-color syntax (`rgb(from red 255 0 0)`), a nested function
    /// (`calc()`, `min()`), or any other shape stay out of scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub numeric_args: Vec<NumericArg>,
}

/// One numeric argument to a color function. `Percentage` carries the bare
/// magnitude with the `%` suffix stripped but its percentage-ness preserved
/// (never divided by 100 or 255 at the producer) — a consumer normalizing
/// `rgb()`/`hsl()` channels must apply its OWN percentage rule (always
/// `/100`) rather than folding a percentage into the same 0-255-vs-0-1
/// magnitude heuristic it uses for a bare `Number`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum NumericArg {
    Number(f64),
    Percentage(f64),
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
        /// The analysed bytes are NOT the whole surface this block
        /// contributes: an inclusion names a sheet elsewhere, or the parse
        /// itself skipped input and its inclusion list is a lower bound.
        /// Set whenever the answer is unknown, because a consumer publishing
        /// "this name is absent from the block's surface" is only sound when
        /// it is clear.
        const SURFACE_PULLS_UNPARSED_BYTES = 1 << 11;
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

impl StyleAnalysisLang {
    /// Classify an authored `<style lang="…">` spelling.
    ///
    /// Delegates to the one spelling owner rather than keeping a table here.
    /// That owner is byte-exact — every preprocessor table the ecosystem hands
    /// these blocks to is keyed by exact bytes, so `lang="SCSS"` has nothing
    /// that can compile it and must not analyse as SCSS either — and it is the
    /// same authority the rewrite pipeline asks, so a spelling cannot resolve
    /// for analysis while failing closed for compilation. An unrecognised
    /// spelling is [`Self::Unknown`], never a default.
    #[must_use]
    pub fn from_lang(lang: &str) -> Self {
        match verter_css_syntax::CssDialect::from_lang(lang) {
            Some(verter_css_syntax::CssDialect::Css) => Self::Css,
            Some(verter_css_syntax::CssDialect::Scss) => Self::Scss,
            Some(verter_css_syntax::CssDialect::Sass) => Self::Sass,
            Some(verter_css_syntax::CssDialect::Less) => Self::Less,
            Some(verter_css_syntax::CssDialect::Stylus) => Self::Stylus,
            None => Self::Unknown,
        }
    }

    /// The native grammar behind this language, or `None` when there is none.
    #[must_use]
    pub const fn native_dialect(self) -> Option<verter_css_syntax::CssDialect> {
        match self {
            Self::Css => Some(verter_css_syntax::CssDialect::Css),
            Self::Scss => Some(verter_css_syntax::CssDialect::Scss),
            Self::Sass => Some(verter_css_syntax::CssDialect::Sass),
            Self::Less => Some(verter_css_syntax::CssDialect::Less),
            Self::Stylus => Some(verter_css_syntax::CssDialect::Stylus),
            Self::Unknown => None,
        }
    }

    /// Whether the shared syntax authority can parse this language's bytes as
    /// authored. `false` means the block's facts depend on an external tool.
    #[must_use]
    pub const fn is_natively_parsed(self) -> bool {
        self.native_dialect().is_some()
    }

    /// Whether bytes in this language need an external preprocessor before a
    /// plain-CSS-only stage can run over them. `Unknown` answers `false`:
    /// nothing here can claim to know what an unrecognised language needs.
    #[must_use]
    pub fn requires_external_preprocessing(self) -> bool {
        self.native_dialect()
            .is_some_and(verter_css_syntax::CssDialect::requires_external_preprocessing)
    }
}

// =============================================================================
// Builder Functions
// =============================================================================

/// Build style analysis for a CSS style block.
///
/// Parses `css_content` with the shared syntax authority to extract selectors, specificity,
/// classes, IDs, custom properties, and at-rules.
/// `vue_input` contains pre-extracted Vue features from verter_compiler.
/// All stored spans are SFC-absolute byte offsets.
pub fn build_css_style_analysis(
    css_content: &str,
    vue_input: VueStyleInput,
    scoped: bool,
    is_module: bool,
    module_name: Option<&str>,
    content_offset: u32,
) -> StyleBlockAnalysis {
    build_scanned_style_analysis(
        StyleAnalysisLang::Css,
        css_content,
        vue_input,
        scoped,
        is_module,
        module_name,
        content_offset,
    )
}

/// Build style analysis with the shared five-dialect syntax authority.
///
/// Vue special pseudos (`:deep`, `:global`, `:slotted`) discovered by the
/// parser are merged with any pseudos supplied on `vue_input`.
pub fn build_scanned_style_analysis(
    lang: StyleAnalysisLang,
    css_content: &str,
    vue_input: VueStyleInput,
    scoped: bool,
    is_module: bool,
    module_name: Option<&str>,
    content_offset: u32,
) -> StyleBlockAnalysis {
    let dialect = match lang {
        StyleAnalysisLang::Css => verter_css_syntax::CssDialect::Css,
        StyleAnalysisLang::Scss => verter_css_syntax::CssDialect::Scss,
        StyleAnalysisLang::Sass => verter_css_syntax::CssDialect::Sass,
        StyleAnalysisLang::Less => verter_css_syntax::CssDialect::Less,
        StyleAnalysisLang::Stylus => verter_css_syntax::CssDialect::Stylus,
        StyleAnalysisLang::Unknown => {
            return build_preprocessor_style_analysis(
                lang,
                vue_input,
                scoped,
                is_module,
                module_name,
                content_offset,
            );
        }
    };

    match super::style_syntax::parse_style_block(css_content, content_offset, dialect) {
        Some(ir) => build_scanned_style_analysis_from_ir(
            lang,
            &ir,
            vue_input,
            scoped,
            is_module,
            module_name,
            content_offset,
        ),
        None => build_incomplete_style_analysis(
            lang,
            vue_input,
            scoped,
            is_module,
            module_name,
            content_offset,
        ),
    }
}

/// Parse a native style block once. The caller retains the IR and projects
/// facts through [`build_scanned_style_analysis_from_ir`].
pub fn parse_style_ir_for_analysis(
    css_content: &str,
    content_offset: u32,
    lang: StyleAnalysisLang,
) -> Option<verter_css_syntax::StyleSyntaxIr> {
    let dialect = match lang {
        StyleAnalysisLang::Css => verter_css_syntax::CssDialect::Css,
        StyleAnalysisLang::Scss => verter_css_syntax::CssDialect::Scss,
        StyleAnalysisLang::Sass => verter_css_syntax::CssDialect::Sass,
        StyleAnalysisLang::Less => verter_css_syntax::CssDialect::Less,
        StyleAnalysisLang::Stylus => verter_css_syntax::CssDialect::Stylus,
        StyleAnalysisLang::Unknown => return None,
    };
    super::style_syntax::parse_style_block(css_content, content_offset, dialect)
}

/// Typed incomplete/uncertain style facts: no IR and no second parse.
/// Unknown dialects stay `ProcessedContentRequired`; a known dialect whose
/// parse produced no IR stays native with `css: None`.
pub fn build_incomplete_style_analysis(
    lang: StyleAnalysisLang,
    vue_input: VueStyleInput,
    scoped: bool,
    is_module: bool,
    module_name: Option<&str>,
    content_offset: u32,
) -> StyleBlockAnalysis {
    if lang == StyleAnalysisLang::Unknown {
        return build_preprocessor_style_analysis(
            lang,
            vue_input,
            scoped,
            is_module,
            module_name,
            content_offset,
        );
    }
    let v_binds = convert_v_binds(&vue_input);
    let special_pseudos = convert_special_pseudos(&vue_input);
    // No IR behind these facts, so nothing here can claim the analysed bytes
    // are the whole surface.
    let flags = derive_flags(scoped, is_module, &v_binds, &special_pseudos, None, true);
    StyleBlockAnalysis {
        lang,
        scoped,
        is_module,
        module_name: module_name.map(|s| s.to_string()),
        block_ref: None,
        block_token: None,
        source_space_token: None,
        content_offset,
        v_binds,
        special_pseudos,
        css: None,
        content_availability: BlockContentAvailability::NativeAvailable,
        flags: flags.bits(),
    }
}

/// Project semantic style facts from an already-parsed syntax IR.
pub fn build_scanned_style_analysis_from_ir(
    lang: StyleAnalysisLang,
    ir: &verter_css_syntax::StyleSyntaxIr,
    vue_input: VueStyleInput,
    scoped: bool,
    is_module: bool,
    module_name: Option<&str>,
    content_offset: u32,
) -> StyleBlockAnalysis {
    let (css, scanned_pseudos) = super::style_syntax::project_style_from_ir(ir);
    let css = Some(css);

    let v_binds = convert_v_binds(&vue_input);
    let mut special_pseudos = convert_special_pseudos(&vue_input);
    // Merge syntax-discovered pseudos, skipping duplicates of caller-supplied
    // entries (same kind + span).
    for scanned in scanned_pseudos {
        let duplicate = special_pseudos
            .iter()
            .any(|p| p.kind == scanned.kind && p.start == scanned.start && p.end == scanned.end);
        if !duplicate {
            special_pseudos.push(scanned);
        }
    }
    // Asked of the parse itself, not folded over the projected facts: a
    // recovered parse's at-rule list is a lower bound, so an inclusion inside
    // the range it skipped is invisible to `css.at_rules`.
    let flags = derive_flags(
        scoped,
        is_module,
        &v_binds,
        &special_pseudos,
        css.as_ref(),
        ir.pulls_in_unparsed_bytes(),
    );

    StyleBlockAnalysis {
        lang,
        scoped,
        is_module,
        module_name: module_name.map(|s| s.to_string()),
        block_ref: None,
        block_token: None,
        source_space_token: None,
        content_offset,
        v_binds,
        special_pseudos,
        css,
        content_availability: BlockContentAvailability::NativeAvailable,
        flags: flags.bits(),
    }
}

/// Build the TYPED deferred analysis for an external `<style src="...">`
/// block: declaration facts only (lang/scoped/module), NO content facts —
/// `css: None`, no v-binds, and a typed unavailable content state. The
/// external file's content stays deferred (B-23); this never fabricates an
/// empty-but-positive analysis for content the producer has not seen.
pub fn build_external_src_style_analysis(
    lang: StyleAnalysisLang,
    scoped: bool,
    is_module: bool,
    module_name: Option<&str>,
    content_offset: u32,
) -> StyleBlockAnalysis {
    // External content is deferred: this block's surface is entirely bytes
    // this analysis never saw.
    let flags = derive_flags(scoped, is_module, &[], &[], None, true);
    StyleBlockAnalysis {
        lang,
        scoped,
        is_module,
        module_name: module_name.map(|s| s.to_string()),
        block_ref: None,
        block_token: None,
        source_space_token: None,
        content_offset,
        v_binds: Vec::new(),
        special_pseudos: Vec::new(),
        css: None,
        content_availability: BlockContentAvailability::Missing,
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
    // No IR behind these facts, so nothing here can claim the analysed bytes
    // are the whole surface.
    let flags = derive_flags(scoped, is_module, &v_binds, &special_pseudos, None, true);

    StyleBlockAnalysis {
        lang,
        scoped,
        is_module,
        module_name: module_name.map(|s| s.to_string()),
        block_ref: None,
        block_token: None,
        source_space_token: None,
        content_offset,
        v_binds,
        special_pseudos,
        css: None,
        content_availability: BlockContentAvailability::ProcessedContentRequired,
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
            expr_roots: vb.expr_roots.clone(),
            roots_complete: vb.roots_complete,
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
    pulls_in_unparsed_bytes: bool,
) -> StyleAnalysisFlags {
    let mut flags = StyleAnalysisFlags::empty();

    if pulls_in_unparsed_bytes {
        flags |= StyleAnalysisFlags::SURFACE_PULLS_UNPARSED_BYTES;
    }

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
    super::style_syntax::extract_var_references_authority(value_text, offset_in_css, content_offset)
}

// =============================================================================
// Selector String Parser
// =============================================================================

/// Parse a complete static CSS selector into the legacy semantic matcher shape.
/// Dynamic, recovered, and evaluation-dependent selectors fail closed.
pub fn parse_selector(selector_text: &str) -> Option<StructuredSelector> {
    PARSE_SELECTOR_INVOCATIONS.with(|count| count.set(count.get() + 1));
    super::style_syntax::parse_selector_authority(selector_text)
}

thread_local! {
    static PARSE_SELECTOR_INVOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Per-thread count of [`parse_selector`] executions.
/// The CSS selector-match boundary must not increment this.
#[must_use]
pub fn parse_selector_thread_invocations() -> u64 {
    PARSE_SELECTOR_INVOCATIONS.with(std::cell::Cell::get)
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

#[cfg(test)]
#[path = "style_tests.rs"]
mod style_tests;
