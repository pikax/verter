//! Lint rule trait and rule registry.
//!
//! Each rule implements [`LintRule`] with optional hooks. The visitor calls
//! active rules during a single-pass DFS traversal.

pub mod a11y;
pub mod cross_file;
pub mod css;
pub mod html_conformance;
pub mod performance;
pub mod reactivity;
pub mod script;
pub mod security;
pub mod vapor;
pub mod vue;

use crate::context::LintContext;
use crate::cross_file::CrossFileSnapshot;
use crate::diagnostic::Severity;
use verter_analysis::template::{
    TemplateAnalysisSnapshot, TemplateBindingOccurrence, TemplateDirective, TemplateElement,
    VForDirective,
};
use verter_analysis::types::ScriptAnalysisSnapshot;
use verter_analysis::StyleBlockAnalysis;

/// File-level context passed to `check_file`.
///
/// Provides all analysis data for a single SFC, enabling rules that need
/// cross-block reasoning (e.g., CSS selectors vs template elements).
pub struct FileContext<'a> {
    pub template: Option<&'a TemplateAnalysisSnapshot>,
    pub script: Option<&'a ScriptAnalysisSnapshot>,
    pub styles: &'a [StyleBlockAnalysis],
    /// Full SFC source text (for byte-level extraction).
    pub source: Option<&'a str>,
}

/// Category for lint rule classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleCategory {
    /// Error prevention (Vue essential).
    VueEssential,
    /// Readability (Vue recommended).
    VueRecommended,
    /// Accessibility / WCAG.
    Accessibility,
    /// HTML spec conformance.
    HtmlConformance,
    /// CSS rules.
    Css,
    /// Script conventions.
    Script,
    /// Vue reactivity pitfalls.
    Reactivity,
    /// Performance optimization.
    Performance,
    /// Security (XSS, unsafe URLs).
    Security,
    /// Vapor mode compatibility.
    Vapor,
    /// Cross-file validation.
    CrossFile,
}

impl RuleCategory {
    /// String representation for diagnostic category field.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::VueEssential => "vue-essential",
            Self::VueRecommended => "vue-recommended",
            Self::Accessibility => "accessibility",
            Self::HtmlConformance => "html-conformance",
            Self::Css => "css",
            Self::Script => "script",
            Self::Reactivity => "reactivity",
            Self::Performance => "performance",
            Self::Security => "security",
            Self::Vapor => "vapor",
            Self::CrossFile => "cross-file",
        }
    }
}

/// Trait for lint rules. Implement only the hooks relevant to your rule.
/// All hook methods are no-ops by default.
pub trait LintRule: Send + Sync {
    /// Rule name (e.g., `"require-v-for-key"`).
    fn name(&self) -> &'static str;
    /// Rule category.
    fn category(&self) -> RuleCategory;
    /// Default severity.
    fn default_severity(&self) -> Severity;

    // ── Template hooks ──

    /// Called once with the full template analysis snapshot.
    fn check_template(&self, _tpl: &TemplateAnalysisSnapshot, _ctx: &mut LintContext) {}
    /// Called for each element in the template.
    fn check_element(&self, _el: &TemplateElement, _ctx: &mut LintContext) {}
    /// Called for each directive on an element.
    fn check_directive(
        &self,
        _dir: &TemplateDirective,
        _el: &TemplateElement,
        _ctx: &mut LintContext,
    ) {
    }
    /// Called for each interpolation binding occurrence.
    fn check_interpolation(&self, _occ: &TemplateBindingOccurrence, _ctx: &mut LintContext) {}
    /// Called for each v-for directive.
    fn check_v_for(&self, _vfor: &VForDirective, _el: &TemplateElement, _ctx: &mut LintContext) {}

    // ── Script hooks ──

    /// Called once with the full script analysis snapshot.
    fn check_script(&self, _script: &ScriptAnalysisSnapshot, _ctx: &mut LintContext) {}

    // ── Style hooks ──

    /// Called for each style block.
    fn check_style(&self, _style: &StyleBlockAnalysis, _ctx: &mut LintContext) {}

    // ── File-level hooks ──

    /// Called once with the full file context (all blocks + source).
    /// Use for rules that need cross-block reasoning (e.g., CSS vs template).
    fn check_file(&self, _file: &FileContext<'_>, _ctx: &mut LintContext) {}

    // ── Cross-file hooks ──

    /// Called once with cross-file analysis data (provide/inject validation, composable chains).
    fn check_cross_file(&self, _snapshot: &CrossFileSnapshot, _ctx: &mut LintContext) {}
}

/// Registry of all available lint rules.
pub struct RuleRegistry {
    rules: Vec<Box<dyn LintRule>>,
}

impl RuleRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Register a rule.
    pub fn register(&mut self, rule: Box<dyn LintRule>) {
        self.rules.push(rule);
    }

    /// Get all registered rules.
    pub fn rules(&self) -> &[Box<dyn LintRule>] {
        &self.rules
    }

    /// Create a registry with all built-in rules.
    pub fn builtin() -> Self {
        let mut registry = Self::new();
        // Rules will be registered here as they're implemented in Phase 10
        register_builtin_rules(&mut registry);
        registry
    }
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

/// Register all built-in rules. Called by `RuleRegistry::builtin()`.
fn register_builtin_rules(registry: &mut RuleRegistry) {
    // Vue Essential
    registry.register(Box::new(vue::RequireVForKey));
    registry.register(Box::new(vue::ValidVFor));
    registry.register(Box::new(vue::NoDuplicateAttributes));
    registry.register(Box::new(vue::NoTemplateKey));
    registry.register(Box::new(vue::NoTextareaMustache));
    registry.register(Box::new(vue::NoDupeVElseIf));
    registry.register(Box::new(vue::NoUseVIfWithVFor));
    registry.register(Box::new(vue::ValidVIf));
    registry.register(Box::new(vue::ValidVElse));
    registry.register(Box::new(vue::ValidVShow));
    registry.register(Box::new(vue::ValidVOn));
    registry.register(Box::new(vue::ValidVBind));
    registry.register(Box::new(vue::ValidVModel));
    registry.register(Box::new(vue::NoVTextVHtmlOnComponent));
    registry.register(Box::new(vue::NoReservedComponentNames));
    registry.register(Box::new(vue::RequireComponentIs));
    registry.register(Box::new(vue::NoUselessTemplateAttrs));
    registry.register(Box::new(vue::NoChildContent));
    registry.register(Box::new(vue::ValidVOnce));
    registry.register(Box::new(vue::ValidVPre));
    registry.register(Box::new(vue::ValidVHtml));
    registry.register(Box::new(vue::ValidVText));
    registry.register(Box::new(vue::ValidVCloak));
    registry.register(Box::new(vue::ValidVMemo));
    registry.register(Box::new(vue::ValidVSlot));
    registry.register(Box::new(vue::ValidVIs));
    registry.register(Box::new(vue::NoDeprecatedVOnNativeModifier));
    registry.register(Box::new(vue::NoDeprecatedScopeAttribute));
    registry.register(Box::new(vue::NoDeprecatedSlotAttribute));
    registry.register(Box::new(vue::NoDeprecatedVBindSync));
    registry.register(Box::new(vue::NoDeprecatedVOnNumberModifiers));
    registry.register(Box::new(vue::NoDeprecatedDataObjectDeclaration));
    registry.register(Box::new(vue::NoDeprecatedDeleteSet));
    registry.register(Box::new(vue::NoDeprecatedDestroyedLifecycle));
    registry.register(Box::new(vue::NoDeprecatedDollarListenersApi));
    registry.register(Box::new(vue::NoDeprecatedDollarScopedslotsApi));
    registry.register(Box::new(vue::NoDeprecatedEventsApi));
    registry.register(Box::new(vue::NoDeprecatedFilter));
    registry.register(Box::new(vue::NoDeprecatedFunctionalTemplate));
    registry.register(Box::new(vue::NoDeprecatedHtmlElementIs));
    registry.register(Box::new(vue::NoDeprecatedInlineTemplate));
    registry.register(Box::new(vue::NoDeprecatedModelDefinition));
    registry.register(Box::new(vue::NoDeprecatedPropsDefaultThis));
    registry.register(Box::new(vue::NoDeprecatedRouterLinkTagProp));
    registry.register(Box::new(vue::NoDeprecatedVIs));
    registry.register(Box::new(vue::NoDeprecatedVueConfigKeycodes));
    registry.register(Box::new(vue::ValidTemplateRoot));
    registry.register(Box::new(vue::NoRootVIf));
    registry.register(Box::new(vue::ConditionalRootComplex));
    registry.register(Box::new(vue::RequireToggleInsideTransition));
    registry.register(Box::new(vue::UseVOnExact));
    registry.register(Box::new(vue::NoMultipleSlotArgs));
    registry.register(Box::new(vue::NoMutatingProps));
    registry.register(Box::new(vue::NoUnusedVars));
    registry.register(Box::new(vue::NoUndefComponents));
    // Vue Recommended
    registry.register(Box::new(vue::NoUnusedComponents));
    registry.register(Box::new(vue::NoUnusedProps));
    registry.register(Box::new(vue::MultiWordComponentNames));
    registry.register(Box::new(vue::HtmlSelfClosing));
    registry.register(Box::new(vue::AttributeOrder));
    registry.register(Box::new(vue::VBindStyle));
    registry.register(Box::new(vue::VOnStyle));
    registry.register(Box::new(vue::VSlotStyle));
    registry.register(Box::new(vue::PreferTrueAttributeShorthand));
    registry.register(Box::new(vue::PreferVBindShorthand));
    registry.register(Box::new(vue::NoTemplateTargetBlank));
    registry.register(Box::new(vue::NoVForIndexAsKey));
    registry.register(Box::new(vue::NoConstantCondition));
    registry.register(Box::new(vue::NoLoneTemplate));
    registry.register(Box::new(vue::NoUselessMustaches));
    registry.register(Box::new(vue::NoUselessVBind));
    registry.register(Box::new(vue::ThisInTemplate));
    registry.register(Box::new(vue::NoStaticInlineStyles));
    registry.register(Box::new(vue::NoVForTemplateKeyOnChild));
    registry.register(Box::new(vue::NoTemplateShadow));
    registry.register(Box::new(vue::NoNegatedVIfCondition));
    registry.register(Box::new(vue::AttributeHyphenation));
    registry.register(Box::new(vue::VOnEventHyphenation));
    registry.register(Box::new(vue::BlockOrder));
    registry.register(Box::new(vue::MatchComponentFileName));
    registry.register(Box::new(vue::NoBareStringsInTemplate));
    registry.register(Box::new(vue::NoDuplicateAttrInheritance));
    registry.register(Box::new(vue::NoMultipleObjectsInClass));
    registry.register(Box::new(vue::NoUndefProperties));
    registry.register(Box::new(vue::PreferSeparateStaticClass));
    registry.register(Box::new(vue::SlotNameCasing));
    registry.register(Box::new(vue::VForDelimiterStyle));
    registry.register(Box::new(vue::VOnHandlerStyle));
    registry.register(Box::new(vue::BlockLang));
    registry.register(Box::new(vue::ComponentNameInTemplateCasing));
    registry.register(Box::new(vue::CustomEventNameCasing));
    registry.register(Box::new(vue::EnforceStyleAttribute));
    registry.register(Box::new(vue::HtmlButtonHasType));
    registry.register(Box::new(vue::NoEmptyComponentBlock));
    registry.register(Box::new(vue::NoUnusedRefs));
    registry.register(Box::new(vue::NoVTextDirective));
    // Accessibility
    registry.register(Box::new(a11y::AltText));
    registry.register(Box::new(a11y::AnchorHasContent));
    registry.register(Box::new(a11y::AriaProps));
    registry.register(Box::new(a11y::AriaRole));
    registry.register(Box::new(a11y::ClickEventsHaveKeyEvents));
    registry.register(Box::new(a11y::FormControlHasLabel));
    registry.register(Box::new(a11y::HeadingHasContent));
    registry.register(Box::new(a11y::IframeHasTitle));
    registry.register(Box::new(a11y::InteractiveSupportsFocus));
    registry.register(Box::new(a11y::MediaHasCaption));
    registry.register(Box::new(a11y::NoAriaHiddenOnFocusable));
    registry.register(Box::new(a11y::NoAutofocus));
    registry.register(Box::new(a11y::NoDistractingElements));
    registry.register(Box::new(a11y::RoleHasRequiredAriaProps));
    registry.register(Box::new(a11y::TabindexNoPositive));
    // Security
    registry.register(Box::new(security::NoVHtml));
    registry.register(Box::new(security::NoUnsafeUrl));
    // HTML Conformance
    registry.register(Box::new(html_conformance::NoDeprecatedElement));
    registry.register(Box::new(html_conformance::NoVoidElementContent));
    // Reactivity
    registry.register(Box::new(reactivity::NoRefAsOperand));
    registry.register(Box::new(reactivity::NoSetupPropsReactivityLoss));
    // Performance
    registry.register(Box::new(performance::MaxTemplateDepth::default()));
    registry.register(Box::new(performance::PreferStaticClass));
    // Script
    registry.register(Box::new(script::NoLifecycleAfterAwait));
    registry.register(Box::new(script::NoInlineLifecycle));
    registry.register(Box::new(script::NoAsyncInComputed));
    registry.register(Box::new(script::RequireSymbolProvide));
    registry.register(Box::new(script::PreferUseTemplateRef));
    registry.register(Box::new(script::NoWatchAfterAwait));
    registry.register(Box::new(script::DefineMacrosOrder));
    registry.register(Box::new(script::RequireDefaultProp));
    registry.register(Box::new(script::NoUnusedEmitDeclarations));
    registry.register(Box::new(script::ValidDefineEmits));
    registry.register(Box::new(script::ValidDefineProps));
    registry.register(Box::new(script::NoExportInScriptSetup));
    registry.register(Box::new(script::NoExposeAfterAwait));
    registry.register(Box::new(script::NoReservedKeys));
    registry.register(Box::new(script::NoReservedProps));
    registry.register(Box::new(script::PreferImportFromVue));
    registry.register(Box::new(script::NoArrowFunctionsInWatch));
    registry.register(Box::new(script::RequireExplicitEmits));
    registry.register(Box::new(script::RequirePropTypes));
    registry.register(Box::new(script::NoRequiredPropWithDefault));
    registry.register(Box::new(script::DefineEmitsDeclaration));
    registry.register(Box::new(script::DefinePropsDeclaration));
    registry.register(Box::new(script::RequireEmitValidator));
    registry.register(Box::new(script::ComponentDefinitionNameCasing));
    registry.register(Box::new(script::PropNameCasing));
    registry.register(Box::new(script::OneComponentPerFile));
    registry.register(Box::new(script::OrderInComponents));
    registry.register(Box::new(script::NoSideEffectsInComputed));
    registry.register(Box::new(script::ComponentApiStyle));
    registry.register(Box::new(script::NextTickStyle));
    registry.register(Box::new(script::NoPotentialComponentOptionTypo));
    registry.register(Box::new(script::NoBooleanDefault));
    registry.register(Box::new(script::NoImportCompilerMacros));
    registry.register(Box::new(script::PreferDefineOptions));
    registry.register(Box::new(script::RequireTypedRef));
    registry.register(Box::new(script::PreferScriptAttrs));
    // Vapor (only active when vapor_mode is enabled in config)
    registry.register(Box::new(vapor::NoSuspense));
    registry.register(Box::new(vapor::NoVueLifecycleEvents));
    registry.register(Box::new(vapor::NoInlineTemplate));
    registry.register(Box::new(vapor::NoNonVaporComponents));
    // CSS
    registry.register(Box::new(css::UnusedCssSelector));
    registry.register(Box::new(css::ScopedCssCascade));
    registry.register(Box::new(css::UndefinedCssClass));
    // Cross-file
    registry.register(Box::new(cross_file::ProvideInjectValidation));
    registry.register(Box::new(cross_file::DeepComposableTracking));
    registry.register(Box::new(cross_file::NoDuplicateVue));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_category_as_str() {
        assert_eq!(RuleCategory::VueEssential.as_str(), "vue-essential");
        assert_eq!(RuleCategory::Accessibility.as_str(), "accessibility");
        assert_eq!(RuleCategory::CrossFile.as_str(), "cross-file");
    }

    #[test]
    fn empty_registry() {
        let registry = RuleRegistry::new();
        assert!(registry.rules().is_empty());
    }

    #[test]
    fn builtin_registry_has_rules() {
        let registry = RuleRegistry::builtin();
        assert!(registry.rules().len() >= 138);
    }
}
