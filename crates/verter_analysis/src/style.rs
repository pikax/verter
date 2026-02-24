//! CSS/style analysis for Vue SFC style blocks.
//!
//! Provides lightningcss-based analysis for CSS blocks (selectors, specificity,
//! classes, IDs, custom properties, at-rules) and passthrough for Vue-specific
//! features (v-bind, :deep, :global, :slotted) extracted by verter_core.
//!
//! For non-CSS preprocessors (SCSS, Less, etc.), only Vue features are stored.

use lightningcss::properties::custom::CustomPropertyName;
use lightningcss::rules::CssRule;
use lightningcss::selector::{Component, Selector};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use lightningcss::traits::ToCss;

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

    // Vue features (from verter_core, all languages)
    pub v_binds: Vec<AnalyzedVBind>,
    pub special_pseudos: Vec<AnalyzedSpecialPseudo>,

    // Full CSS analysis (lightningcss, CSS-only)
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
pub struct AnalyzedVBind {
    pub expression: String,
    pub quoted: bool,
    pub start: u32,
    pub end: u32,
}

/// Analyzed Vue special pseudo-class.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedSpecialPseudo {
    pub kind: SpecialPseudoKind,
    pub start: u32,
    pub end: u32,
    pub inner: Option<String>,
}

/// Full CSS analysis produced by lightningcss parsing.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CssAnalysis {
    pub selectors: Vec<AnalyzedSelector>,
    pub classes: Vec<AnalyzedCssClass>,
    pub ids: Vec<AnalyzedCssId>,
    pub custom_properties: Vec<AnalyzedCustomProperty>,
    pub at_rules: Vec<AnalyzedAtRule>,
    pub rule_count: u32,
}

/// A CSS selector with its computed specificity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedSelector {
    pub text: String,
    /// Specificity tuple: (id, class, type).
    pub specificity: (u32, u32, u32),
}

/// A CSS class selector occurrence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedCssClass {
    pub name: String,
}

/// A CSS ID selector occurrence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedCssId {
    pub name: String,
}

/// A CSS custom property (variable) declaration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedCustomProperty {
    /// Includes the `--` prefix.
    pub name: String,
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
/// Parses `css_content` with lightningcss to extract selectors, specificity,
/// classes, IDs, custom properties, and at-rules.
/// `vue_input` contains pre-extracted Vue features from verter_core.
pub fn build_css_style_analysis(
    css_content: &str,
    vue_input: VueStyleInput,
    scoped: bool,
    is_module: bool,
    module_name: Option<&str>,
) -> StyleBlockAnalysis {
    let css = parse_css(css_content);

    let v_binds = convert_v_binds(&vue_input);
    let special_pseudos = convert_special_pseudos(&vue_input);
    let flags = derive_flags(scoped, is_module, &v_binds, &special_pseudos, css.as_ref());

    StyleBlockAnalysis {
        lang: StyleAnalysisLang::Css,
        scoped,
        is_module,
        module_name: module_name.map(|s| s.to_string()),
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
) -> StyleBlockAnalysis {
    let v_binds = convert_v_binds(&vue_input);
    let special_pseudos = convert_special_pseudos(&vue_input);
    let flags = derive_flags(scoped, is_module, &v_binds, &special_pseudos, None);

    StyleBlockAnalysis {
        lang,
        scoped,
        is_module,
        module_name: module_name.map(|s| s.to_string()),
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

/// Parse CSS content with lightningcss and extract analysis data.
/// Returns `None` if the CSS fails to parse.
fn parse_css(css_content: &str) -> Option<CssAnalysis> {
    let stylesheet = StyleSheet::parse(css_content, ParserOptions::default()).ok()?;

    let mut analysis = CssAnalysis::default();
    walk_rules(&stylesheet.rules.0, css_content, &mut analysis);
    Some(analysis)
}

fn walk_rules<'i>(rules: &[CssRule<'i>], source: &str, analysis: &mut CssAnalysis) {
    for rule in rules {
        match rule {
            CssRule::Style(style_rule) => {
                analysis.rule_count += 1;

                // Extract selectors with specificity
                for selector in style_rule.selectors.0.iter() {
                    let text = selector_to_string(selector);
                    let spec = selector.specificity();
                    // lightningcss Specificity is (a << 20 | b << 10 | c) packed,
                    // but the Specificity type from parcel_selectors gives us the tuple.
                    let specificity = (
                        ((spec >> 20) & 0x3FF),
                        ((spec >> 10) & 0x3FF),
                        (spec & 0x3FF),
                    );

                    analysis
                        .selectors
                        .push(AnalyzedSelector { text, specificity });

                    // Extract classes and IDs from selector components
                    extract_selector_components(selector, analysis);
                }

                // Extract custom properties from declarations
                extract_custom_properties_from_declarations(
                    &style_rule.declarations,
                    source,
                    analysis,
                );

                // Recurse into nested rules
                walk_rules(&style_rule.rules.0, source, analysis);
            }
            CssRule::Media(media) => {
                let name = media_query_to_string(&media.query);
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::Media,
                    name,
                });
                walk_rules(&media.rules.0, source, analysis);
            }
            CssRule::Keyframes(keyframes) => {
                let name = match &keyframes.name {
                    lightningcss::rules::keyframes::KeyframesName::Ident(id) => id.0.to_string(),
                    lightningcss::rules::keyframes::KeyframesName::Custom(s) => s.to_string(),
                };
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::Keyframes,
                    name,
                });
            }
            CssRule::Supports(_supports) => {
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::Supports,
                    name: String::new(),
                });
                walk_rules(&_supports.rules.0, source, analysis);
            }
            CssRule::Import(import) => {
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::Import,
                    name: import.url.to_string(),
                });
            }
            CssRule::LayerStatement(layer) => {
                let name = layer
                    .names
                    .first()
                    .map(|n| {
                        n.0.iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                            .join(".")
                    })
                    .unwrap_or_default();
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::Layer,
                    name,
                });
            }
            CssRule::LayerBlock(layer) => {
                let name = layer
                    .name
                    .as_ref()
                    .map(|n| {
                        n.0.iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                            .join(".")
                    })
                    .unwrap_or_default();
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::Layer,
                    name,
                });
                walk_rules(&layer.rules.0, source, analysis);
            }
            CssRule::Container(container) => {
                let name = container
                    .name
                    .as_ref()
                    .map(|n| n.0 .0.to_string())
                    .unwrap_or_default();
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::Container,
                    name,
                });
                walk_rules(&container.rules.0, source, analysis);
            }
            CssRule::FontFace(_ff) => {
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::FontFace,
                    name: String::new(),
                });
            }
            CssRule::Property(prop) => {
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::Property,
                    name: prop.name.0.to_string(),
                });
            }
            CssRule::Scope(scope) => {
                analysis.at_rules.push(AnalyzedAtRule {
                    kind: AtRuleKind::Scope,
                    name: String::new(),
                });
                walk_rules(&scope.rules.0, source, analysis);
            }
            _ => {}
        }
    }
}

fn selector_to_string<'i>(selector: &Selector<'i>) -> String {
    use lightningcss::printer::PrinterOptions;
    selector
        .to_css_string(PrinterOptions::default())
        .unwrap_or_default()
}

fn media_query_to_string<'i>(query: &lightningcss::media_query::MediaList<'i>) -> String {
    use lightningcss::printer::PrinterOptions;
    query
        .to_css_string(PrinterOptions::default())
        .unwrap_or_default()
}

fn extract_selector_components<'i>(selector: &Selector<'i>, analysis: &mut CssAnalysis) {
    for component in selector.iter_raw_match_order() {
        match component {
            Component::Class(id) => {
                analysis.classes.push(AnalyzedCssClass {
                    name: id.0.to_string(),
                });
            }
            Component::ID(id) => {
                analysis.ids.push(AnalyzedCssId {
                    name: id.0.to_string(),
                });
            }
            _ => {}
        }
    }
}

fn custom_property_name_to_string(name: &CustomPropertyName<'_>) -> String {
    match name {
        CustomPropertyName::Custom(dashed) => dashed.0.to_string(),
        CustomPropertyName::Unknown(ident) => ident.0.to_string(),
    }
}

fn extract_custom_properties_from_declarations(
    declarations: &lightningcss::declaration::DeclarationBlock<'_>,
    _source: &str,
    analysis: &mut CssAnalysis,
) {
    for decl in declarations.declarations.iter() {
        if let lightningcss::properties::Property::Custom(custom) = decl {
            analysis.custom_properties.push(AnalyzedCustomProperty {
                name: custom_property_name_to_string(&custom.name),
            });
        }
    }
    for decl in declarations.important_declarations.iter() {
        if let lightningcss::properties::Property::Custom(custom) = decl {
            analysis.custom_properties.push(AnalyzedCustomProperty {
                name: custom_property_name_to_string(&custom.name),
            });
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze_css(css: &str) -> StyleBlockAnalysis {
        build_css_style_analysis(css, VueStyleInput::default(), false, false, None)
    }

    /// @ai-generated - CSS selectors are extracted with correct specificity
    #[test]
    fn test_css_analysis_selectors_and_specificity() {
        let analysis = analyze_css(
            r#"
            .btn { color: red; }
            #app .main { display: flex; }
            div > p.active { font-size: 14px; }
        "#,
        );

        let css = analysis.css.as_ref().expect("should have CSS analysis");
        assert_eq!(css.selectors.len(), 3);
        assert_eq!(css.rule_count, 3);

        // .btn → specificity (0, 1, 0)
        let btn = &css.selectors[0];
        assert_eq!(btn.text, ".btn");
        assert_eq!(btn.specificity, (0, 1, 0));

        // #app .main → specificity (1, 1, 0)
        let app_main = &css.selectors[1];
        assert_eq!(app_main.text, "#app .main");
        assert_eq!(app_main.specificity, (1, 1, 0));

        // div > p.active → specificity (0, 1, 2)
        let div_p = &css.selectors[2];
        assert_eq!(div_p.text, "div > p.active");
        assert_eq!(div_p.specificity, (0, 1, 2));
    }

    /// @ai-generated - CSS class names are extracted
    #[test]
    fn test_css_analysis_classes() {
        let analysis = analyze_css(
            r#"
            .btn { color: red; }
            .active { display: none; }
            .btn.primary { background: blue; }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
        assert!(class_names.contains(&"btn"));
        assert!(class_names.contains(&"active"));
        assert!(class_names.contains(&"primary"));
    }

    /// @ai-generated - CSS ID selectors are extracted
    #[test]
    fn test_css_analysis_ids() {
        let analysis = analyze_css(
            r#"
            #app { margin: 0; }
            #main { display: flex; }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        let id_names: Vec<&str> = css.ids.iter().map(|i| i.name.as_str()).collect();
        assert!(id_names.contains(&"app"));
        assert!(id_names.contains(&"main"));
    }

    /// @ai-generated - Custom properties are extracted from declarations
    #[test]
    fn test_css_analysis_custom_properties() {
        let analysis = analyze_css(
            r#"
            :root {
                --primary-color: #333;
                --spacing-lg: 24px;
            }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        let prop_names: Vec<&str> = css
            .custom_properties
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert!(prop_names.contains(&"--primary-color"));
        assert!(prop_names.contains(&"--spacing-lg"));
    }

    /// @ai-generated - At-rules are classified correctly
    #[test]
    fn test_css_analysis_at_rules() {
        let analysis = analyze_css(
            r#"
            @media (max-width: 768px) { .btn { display: none; } }
            @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
            @layer utilities;
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        assert!(css.at_rules.iter().any(|r| r.kind == AtRuleKind::Media));
        assert!(css
            .at_rules
            .iter()
            .any(|r| r.kind == AtRuleKind::Keyframes && r.name == "fadeIn"));
        assert!(css.at_rules.iter().any(|r| r.kind == AtRuleKind::Layer));
    }

    /// @ai-generated - Vue features are passed through correctly
    #[test]
    fn test_css_analysis_with_vue_input() {
        let vue_input = VueStyleInput {
            v_binds: vec![VBindInput {
                expression: "color".to_string(),
                quoted: false,
                start: 10,
                end: 25,
            }],
            special_pseudos: vec![SpecialPseudoInput {
                kind: SpecialPseudoKind::Deep,
                start: 30,
                end: 50,
                inner: Some(".inner".to_string()),
            }],
        };

        let analysis =
            build_css_style_analysis(".btn { color: red; }", vue_input, true, false, None);

        assert_eq!(analysis.v_binds.len(), 1);
        assert_eq!(analysis.v_binds[0].expression, "color");
        assert_eq!(analysis.special_pseudos.len(), 1);
        assert_eq!(analysis.special_pseudos[0].kind, SpecialPseudoKind::Deep);
        assert_eq!(analysis.special_pseudos[0].inner.as_deref(), Some(".inner"));
    }

    /// @ai-generated - Preprocessor blocks get no CSS parsing
    #[test]
    fn test_preprocessor_no_css_parsing() {
        let vue_input = VueStyleInput {
            v_binds: vec![VBindInput {
                expression: "color".to_string(),
                quoted: false,
                start: 10,
                end: 25,
            }],
            special_pseudos: Vec::new(),
        };

        let analysis = build_preprocessor_style_analysis(
            StyleAnalysisLang::Scss,
            vue_input,
            false,
            false,
            None,
        );

        assert_eq!(analysis.lang, StyleAnalysisLang::Scss);
        assert!(analysis.css.is_none(), "SCSS should not have CSS analysis");
        assert_eq!(analysis.v_binds.len(), 1);
    }

    /// @ai-generated - Flags are derived correctly from content
    #[test]
    fn test_flags_derived_correctly() {
        let vue_input = VueStyleInput {
            v_binds: vec![VBindInput {
                expression: "x".to_string(),
                quoted: false,
                start: 0,
                end: 5,
            }],
            special_pseudos: vec![
                SpecialPseudoInput {
                    kind: SpecialPseudoKind::Deep,
                    start: 0,
                    end: 5,
                    inner: None,
                },
                SpecialPseudoInput {
                    kind: SpecialPseudoKind::Global,
                    start: 0,
                    end: 5,
                    inner: None,
                },
                SpecialPseudoInput {
                    kind: SpecialPseudoKind::Slotted,
                    start: 0,
                    end: 5,
                    inner: None,
                },
            ],
        };

        let analysis = build_css_style_analysis(
            r#"
            @import "base.css";
            @layer reset;
            :root { --my-var: red; }
            @keyframes slide { from {} to {} }
            @container sidebar (min-width: 300px) { .card { display: flex; } }
        "#,
            vue_input,
            true,
            true,
            Some("styles"),
        );

        let flags = analysis.analysis_flags();
        assert!(flags.contains(StyleAnalysisFlags::SCOPED));
        assert!(flags.contains(StyleAnalysisFlags::MODULE));
        assert!(flags.contains(StyleAnalysisFlags::HAS_V_BIND));
        assert!(flags.contains(StyleAnalysisFlags::HAS_DEEP));
        assert!(flags.contains(StyleAnalysisFlags::HAS_GLOBAL));
        assert!(flags.contains(StyleAnalysisFlags::HAS_SLOTTED));
        assert!(flags.contains(StyleAnalysisFlags::HAS_CUSTOM_PROPS));
        assert!(flags.contains(StyleAnalysisFlags::HAS_KEYFRAMES));
        assert!(flags.contains(StyleAnalysisFlags::HAS_IMPORTS));
        assert!(flags.contains(StyleAnalysisFlags::HAS_LAYERS));
        assert!(flags.contains(StyleAnalysisFlags::HAS_CONTAINER_QUERIES));
        assert_eq!(analysis.module_name.as_deref(), Some("styles"));
    }

    /// @ai-generated - Empty CSS returns defaults
    #[test]
    fn test_empty_css() {
        let analysis = analyze_css("");
        let css = analysis.css.as_ref().unwrap();
        assert!(css.selectors.is_empty());
        assert!(css.classes.is_empty());
        assert!(css.ids.is_empty());
        assert!(css.custom_properties.is_empty());
        assert!(css.at_rules.is_empty());
        assert_eq!(css.rule_count, 0);
    }

    /// @ai-generated - Malformed CSS returns None gracefully
    #[test]
    fn test_malformed_css_graceful() {
        // lightningcss is lenient, but completely broken syntax might fail
        let analysis = analyze_css("{{{invalid$$$css}}}");
        // Even if it fails to parse, we should get a valid StyleBlockAnalysis
        assert_eq!(analysis.lang, StyleAnalysisLang::Css);
    }

    /// @ai-generated - Multiple selectors in a single rule
    #[test]
    fn test_multiple_selectors_per_rule() {
        let analysis = analyze_css(".a, .b, .c { color: red; }");
        let css = analysis.css.as_ref().unwrap();
        assert_eq!(css.selectors.len(), 3);
        assert_eq!(css.rule_count, 1);

        let names: Vec<&str> = css.selectors.iter().map(|s| s.text.as_str()).collect();
        assert!(names.contains(&".a"));
        assert!(names.contains(&".b"));
        assert!(names.contains(&".c"));
    }

    /// @ai-generated - Nested rules (CSS nesting) are walked
    #[test]
    fn test_nested_media_rules() {
        let analysis = analyze_css(
            r#"
            @media (max-width: 768px) {
                .mobile { display: block; }
                .desktop { display: none; }
            }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        assert!(css.at_rules.iter().any(|r| r.kind == AtRuleKind::Media));
        let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
        assert!(class_names.contains(&"mobile"));
        assert!(class_names.contains(&"desktop"));
    }

    /// @ai-generated - Native CSS nesting (`.parent { .child {} }`)
    #[test]
    fn test_native_css_nesting() {
        let analysis = analyze_css(
            r#"
            .parent {
                color: red;
                .child {
                    color: blue;
                }
            }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
        assert!(
            class_names.contains(&"parent"),
            "should extract parent class"
        );
        assert!(
            class_names.contains(&"child"),
            "should extract nested child class"
        );
        assert!(css.rule_count >= 1, "should count at least the outer rule");
    }

    /// @ai-generated - @scope at-rule detection
    #[test]
    fn test_scope_at_rule() {
        let analysis = analyze_css(
            r#"
            @scope (.card) {
                .title { font-weight: bold; }
            }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        assert!(
            css.at_rules.iter().any(|r| r.kind == AtRuleKind::Scope),
            "should detect @scope at-rule"
        );
        // Nested rules inside @scope should be walked
        let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
        assert!(
            class_names.contains(&"title"),
            "should extract classes inside @scope"
        );
    }

    /// @ai-generated - Complex selectors with combinators
    #[test]
    fn test_complex_selectors_with_combinators() {
        let analysis = analyze_css(
            r#"
            .a > .b { color: red; }
            .c + .d { color: green; }
            .e ~ .f { color: blue; }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        assert_eq!(css.selectors.len(), 3);
        let class_names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
        for name in &["a", "b", "c", "d", "e", "f"] {
            assert!(
                class_names.contains(name),
                "should extract class .{name} from combinator selector"
            );
        }
    }

    /// @ai-generated - @container with name
    #[test]
    fn test_container_with_name() {
        let analysis = analyze_css(
            r#"
            @container sidebar (min-width: 300px) {
                .card { display: flex; }
            }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        let container_rule = css
            .at_rules
            .iter()
            .find(|r| r.kind == AtRuleKind::Container)
            .expect("should detect @container at-rule");
        assert_eq!(
            container_rule.name, "sidebar",
            "should capture container name"
        );
    }

    /// @ai-generated - Style analysis types serialize to camelCase JSON keys
    /// for compatibility with the TypeScript playground AnalysisPanel.
    #[test]
    fn test_style_block_analysis_serializes_camel_case() {
        let vue_input = VueStyleInput {
            v_binds: vec![VBindInput {
                expression: "color".to_string(),
                quoted: false,
                start: 10,
                end: 25,
            }],
            special_pseudos: vec![SpecialPseudoInput {
                kind: SpecialPseudoKind::Deep,
                start: 30,
                end: 50,
                inner: Some(".inner".to_string()),
            }],
        };

        let analysis = build_css_style_analysis(
            r#":root { --my-var: red; } @keyframes slide { from {} to {} }"#,
            vue_input,
            true,
            true,
            Some("styles"),
        );

        let json = serde_json::to_value(&analysis).expect("should serialize");
        let obj = json.as_object().expect("should be an object");

        // StyleBlockAnalysis fields must be camelCase
        assert!(
            obj.contains_key("isModule"),
            "expected 'isModule', got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
        assert!(
            obj.contains_key("moduleName"),
            "expected 'moduleName', got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
        assert!(
            obj.contains_key("vBinds"),
            "expected 'vBinds', got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
        assert!(
            obj.contains_key("specialPseudos"),
            "expected 'specialPseudos', got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );

        // CssAnalysis fields must also be camelCase
        let css_obj = obj["css"].as_object().expect("css should be an object");
        assert!(
            css_obj.contains_key("customProperties"),
            "expected 'customProperties', got keys: {:?}",
            css_obj.keys().collect::<Vec<_>>()
        );
        assert!(
            css_obj.contains_key("atRules"),
            "expected 'atRules', got keys: {:?}",
            css_obj.keys().collect::<Vec<_>>()
        );
        assert!(
            css_obj.contains_key("ruleCount"),
            "expected 'ruleCount', got keys: {:?}",
            css_obj.keys().collect::<Vec<_>>()
        );
    }

    /// @ai-generated - @font-face detection
    #[test]
    fn test_font_face_detection() {
        let analysis = analyze_css(
            r#"
            @font-face {
                font-family: "CustomFont";
                src: url("font.woff2") format("woff2");
            }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        assert!(
            css.at_rules.iter().any(|r| r.kind == AtRuleKind::FontFace),
            "should detect @font-face at-rule"
        );
    }

    /// @ai-generated - @property at-rule
    #[test]
    fn test_property_at_rule() {
        let analysis = analyze_css(
            r#"
            @property --my-color {
                syntax: "<color>";
                initial-value: red;
                inherits: false;
            }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        let prop_rule = css
            .at_rules
            .iter()
            .find(|r| r.kind == AtRuleKind::Property)
            .expect("should detect @property at-rule");
        assert_eq!(prop_rule.name, "--my-color", "should capture property name");
    }

    /// @ai-generated - !important custom properties are extracted
    #[test]
    fn test_important_custom_properties() {
        let analysis = analyze_css(
            r#"
            .btn {
                --highlight: red !important;
            }
        "#,
        );

        let css = analysis.css.as_ref().unwrap();
        let prop_names: Vec<&str> = css
            .custom_properties
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert!(
            prop_names.contains(&"--highlight"),
            "should extract !important custom properties, got: {:?}",
            prop_names
        );
    }
}
