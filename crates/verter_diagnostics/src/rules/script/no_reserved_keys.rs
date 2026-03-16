//! Rule: no-reserved-keys
//!
//! Disallows using reserved Vue instance property names as binding names.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

const RESERVED_KEYS: &[&str] = &[
    "$data",
    "$props",
    "$el",
    "$options",
    "$parent",
    "$root",
    "$children",
    "$slots",
    "$scopedSlots",
    "$refs",
    "$isServer",
    "$attrs",
    "$listeners",
    "$watch",
    "$on",
    "$once",
    "$off",
    "$emit",
    "$mount",
    "$forceUpdate",
    "$nextTick",
    "$destroy",
    "_uid",
    "_isVue",
    "_data",
    "_props",
    "_watchers",
    "_setupProxy",
];

pub struct NoReservedKeys;

impl LintRule for NoReservedKeys {
    fn name(&self) -> &'static str {
        "no-reserved-keys"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for binding in &script.bindings {
            if RESERVED_KEYS.contains(&binding.name.as_str()) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "'{}' is a reserved Vue instance property and cannot be used as a binding name.",
                        binding.name
                    ),
                    binding.span.start,
                    binding.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::ScriptCallSite,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(NoReservedKeys, script)
    }

    fn make_binding(name: &str) -> AnalyzedBinding {
        AnalyzedBinding {
            name: name.to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: Span::new(10, 15),
            used_in_script: false,
            used_in_style: false,
        }
    }

    #[test]
    fn reserved_key_reports() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![make_binding("$data")],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "$data as binding should trigger");
        assert!(diags[0].rule == "no-reserved-keys");
        assert!(
            !diags.iter().any(|d| d.rule == "no-reserved-props"),
            "must not trigger props rule"
        );
    }

    #[test]
    fn normal_key_passes() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![make_binding("count")],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "normal binding should pass");
    }
}
