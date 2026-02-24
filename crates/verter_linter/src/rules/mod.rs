//! Lint rule trait and rule registry.
//!
//! Each rule implements [`LintRule`] with optional hooks. The visitor calls
//! active rules during a single-pass DFS traversal.

pub mod a11y;
pub mod performance;
pub mod reactivity;
pub mod security;
pub mod vue;

use crate::context::LintContext;
use crate::diagnostic::Severity;
use verter_analysis::template::{
    TemplateAnalysisSnapshot, TemplateBindingOccurrence, TemplateDirective, TemplateElement,
    VForDirective,
};
use verter_analysis::types::ScriptAnalysisSnapshot;
use verter_analysis::StyleBlockAnalysis;

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
    // Vue Recommended
    registry.register(Box::new(vue::NoUnusedComponents));
    // Accessibility
    registry.register(Box::new(a11y::AltText));
    registry.register(Box::new(a11y::AnchorHasContent));
    registry.register(Box::new(a11y::AriaRole));
    registry.register(Box::new(a11y::ClickEventsHaveKeyEvents));
    registry.register(Box::new(a11y::FormControlHasLabel));
    registry.register(Box::new(a11y::HeadingHasContent));
    registry.register(Box::new(a11y::IframeHasTitle));
    registry.register(Box::new(a11y::NoAutofocus));
    registry.register(Box::new(a11y::NoDistractingElements));
    registry.register(Box::new(a11y::TabindexNoPositive));
    // Security
    registry.register(Box::new(security::NoVHtml));
    // Reactivity
    registry.register(Box::new(reactivity::NoRefAsOperand));
    registry.register(Box::new(reactivity::NoSetupPropsReactivityLoss));
    // Performance
    registry.register(Box::new(performance::MaxTemplateDepth::default()));
    registry.register(Box::new(performance::PreferStaticClass));
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
        assert!(registry.rules().len() >= 23);
    }
}
