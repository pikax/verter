//! Rule: match-component-file-name
//!
//! Component name should match its filename. This is a stub rule that will be
//! fully implemented when filename metadata is available in the file context.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct MatchComponentFileName;

impl LintRule for MatchComponentFileName {
    fn name(&self) -> &'static str {
        "match-component-file-name"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_file(&self, _file: &FileContext<'_>, _ctx: &mut LintContext) {
        // Stub: requires filename metadata not currently in FileContext.
        // Will be implemented when FileContext includes the file path.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_metadata() {
        let rule = MatchComponentFileName;
        assert_eq!(rule.name(), "match-component-file-name");
        assert_eq!(rule.category(), RuleCategory::VueRecommended);
        assert_eq!(rule.default_severity(), Some(Severity::Warning));
    }
}
