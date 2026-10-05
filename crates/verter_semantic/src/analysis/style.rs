//! CSS/style analysis for Vue SFC style blocks.
//!
//! Projects the shared lossless style syntax authority into selectors, specificity,
//! classes, IDs, custom properties, and at-rules. Vue-specific
//! features (v-bind, :deep, :global, :slotted) are passed through from verter_compiler.
use verter_session_query::analysis::style::{
    AnalyzedSpecialPseudo, AnalyzedVBind, AtRuleKind, BlockContentAvailability, CssAnalysis,
    CssVarReference, SelectorPseudoClass, SpecialPseudoKind, StructuredSelector,
    StyleAnalysisFlags, StyleAnalysisLang, StyleBlockAnalysis,
};

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

// =============================================================================
// Analysis Output Types (owned, serializable)
// =============================================================================

// =============================================================================
// Structured Selector Types
// =============================================================================

/// Dialect classification of a style language through the shared CSS syntax
/// authority. The language enum is plain data; this owner maps it to and from
/// the grammar that parses it.
pub trait StyleLangDialect: Sized {
    /// Classify an authored `<style lang="…">` spelling.
    fn from_lang(lang: &str) -> Self;
    /// The native grammar behind this language, or `None` when there is none.
    fn native_dialect(self) -> Option<verter_css_syntax::CssDialect>;
    /// Whether the shared syntax authority can parse this language's bytes as
    /// authored.
    fn is_natively_parsed(&self) -> bool;
    /// Whether bytes in this language need an external preprocessor before a
    /// plain-CSS-only stage can run over them.
    fn requires_external_preprocessing(self) -> bool;
}

impl StyleLangDialect for StyleAnalysisLang {
    /// Classify an authored `<style lang="…">` spelling.
    ///
    /// Delegates to the one spelling owner rather than keeping a table here.
    /// That owner is byte-exact, so `lang="SCSS"` does not become SCSS merely
    /// through case folding. This classification does not decide what a
    /// framework compiler does after its processor lookup misses; Vue's
    /// reference compiler falls through to plain CSS in that case. An
    /// unrecognised spelling is [`Self::Unknown`], never an implicit dialect.
    fn from_lang(lang: &str) -> Self {
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
    fn native_dialect(self) -> Option<verter_css_syntax::CssDialect> {
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
    fn is_natively_parsed(&self) -> bool {
        self.native_dialect().is_some()
    }

    /// Whether bytes in this language need an external preprocessor before a
    /// plain-CSS-only stage can run over them. `Unknown` answers `false`:
    /// nothing here can claim to know what an unrecognised language needs.
    fn requires_external_preprocessing(self) -> bool {
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
    let Some(dialect) = lang.native_dialect() else {
        return build_preprocessor_style_analysis(
            lang,
            vue_input,
            scoped,
            is_module,
            module_name,
            content_offset,
        );
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
    let dialect = lang.native_dialect()?;
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
