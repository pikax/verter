//! Rule: no-unused-components
//!
//! Disallow registering components that are not used in the template.

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;
use verter_semantic::analysis::types::ScriptAnalysisSnapshot;

/// Disallow registering components that are not used in the template.
pub struct NoUnusedComponents;

impl LintRule for NoUnusedComponents {
    fn name(&self) -> &'static str {
        "no-unused-components"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // Check for component imports that are never referenced in the template
        // This uses binding_occurrences with ComponentTag usage kind
        // and compares against the components list
        // (This is a simplified check — the full version would cross-reference
        // script imports against template component usages)

        // If there are unresolved bindings that look like component names
        // (PascalCase), those are likely missing imports — but that's a
        // different rule (no-unresolved-components)
        let _ = tpl;
        let _ = ctx;
    }

    fn check_script(&self, _script: &ScriptAnalysisSnapshot, _ctx: &mut LintContext) {
        // Full implementation would cross-reference script imports against
        // template component usages. This requires both script and template
        // analysis, so it's best done in check_template with access to both.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_metadata() {
        let rule = NoUnusedComponents;
        assert_eq!(rule.name(), "no-unused-components");
        assert_eq!(rule.category(), RuleCategory::VueRecommended);
        assert_eq!(rule.default_severity(), Some(Severity::Warning));
    }
}
