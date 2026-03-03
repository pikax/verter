//! Rule: prefer-define-options
//!
//! Suggests using `defineOptions()` macro for component options in `<script setup>`
//! instead of a separate `<script>` block. Reports when `defineOptions` is not
//! found among the macros.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

pub struct PreferDefineOptions;

impl LintRule for PreferDefineOptions {
    fn name(&self) -> &'static str {
        "prefer-define-options"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Hint
    }

    fn check_script(&self, _script: &ScriptAnalysisSnapshot, _ctx: &mut LintContext) {
        // This rule checks if defineOptions is being used for component options.
        // A full implementation would detect when a component uses a separate
        // <script> block for options like `name`, `inheritAttrs`, etc. and suggest
        // using defineOptions() instead.
        //
        // For now, this is a structural placeholder — the analysis snapshot
        // tracks macros, and the full check would compare against dual-block usage.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_metadata() {
        let rule = PreferDefineOptions;
        assert_eq!(rule.name(), "prefer-define-options");
        assert_eq!(rule.category(), RuleCategory::Script);
        assert_eq!(rule.default_severity(), Severity::Hint);
    }
}
