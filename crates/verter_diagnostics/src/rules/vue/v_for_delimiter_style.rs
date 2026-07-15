//! Rule: v-for-delimiter-style
//!
//! Enforce consistent use of `in` vs `of` in `v-for` directives. This is a stub
//! rule because `VForDirective` does not currently store which delimiter was used.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::{TemplateElement, VForDirective};

pub struct VForDelimiterStyle;

impl LintRule for VForDelimiterStyle {
    fn name(&self) -> &'static str {
        "v-for-delimiter-style"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_v_for(&self, _vfor: &VForDirective, _el: &TemplateElement, _ctx: &mut LintContext) {
        // Stub: VForDirective does not currently store which delimiter
        // (`in` vs `of`) was used. This rule will be implemented when that
        // information is available in the analysis snapshot.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_metadata() {
        let rule = VForDelimiterStyle;
        assert_eq!(rule.name(), "v-for-delimiter-style");
        assert_eq!(rule.category(), RuleCategory::VueRecommended);
        assert_eq!(rule.default_severity(), Some(Severity::Warning));
    }
}
