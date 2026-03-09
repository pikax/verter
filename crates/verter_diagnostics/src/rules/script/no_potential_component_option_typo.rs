//! Rule: no-potential-component-option-typo
//!
//! Detect common typos in Options API option names. Checks script bindings for
//! near-matches to known Vue option names (e.g., "compued" instead of "computed").

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

/// Known Vue Options API option names.
const OPTION_NAMES: &[&str] = &[
    "data",
    "computed",
    "methods",
    "watch",
    "props",
    "emits",
    "components",
    "directives",
    "mixins",
    "extends",
    "setup",
    "render",
    "beforeCreate",
    "created",
    "beforeMount",
    "mounted",
    "beforeUpdate",
    "updated",
    "beforeUnmount",
    "unmounted",
    "inject",
    "provide",
    "template",
    "name",
];

pub struct NoPotentialComponentOptionTypo;

/// Simple edit distance check (Levenshtein distance <= 2).
fn is_near_match(a: &str, b: &str) -> bool {
    if a == b {
        return false; // Exact match is not a typo
    }
    let len_diff = (a.len() as isize - b.len() as isize).unsigned_abs();
    if len_diff > 2 {
        return false;
    }

    // Simple Levenshtein for short strings
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let m = a_bytes.len();
    let n = b_bytes.len();

    if m == 0 || n == 0 {
        return false;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    let dist = prev[n];
    // Only flag if distance is 1 or 2 and the string is long enough to avoid false positives
    dist <= 2 && a.len() >= 4
}

impl LintRule for NoPotentialComponentOptionTypo {
    fn name(&self) -> &'static str {
        "no-potential-component-option-typo"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        // Skip if Composition API macros present (Options API is not expected)
        if !script.macros.is_empty() {
            return;
        }

        for binding in &script.bindings {
            // Skip if it's an exact match to a known option (not a typo)
            if OPTION_NAMES.contains(&binding.name.as_str()) {
                continue;
            }

            // Check if it's a near-match to any known option
            for &option in OPTION_NAMES {
                if is_near_match(&binding.name, option) {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        format!(
                            "Possible typo: '{}' is similar to Options API key '{}'. Did you mean '{}'?",
                            binding.name, option, option
                        ),
                        binding.span.start,
                        binding.span.end,
                        self.default_severity(),
                        DiagnosticSpanKind::ScriptCallSite,
                    );
                    break; // Report only the closest match
                }
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
        crate::test_support::run_script_rule(NoPotentialComponentOptionTypo, script)
    }

    fn make_binding(name: &str) -> AnalyzedBinding {
        AnalyzedBinding {
            name: name.to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: Span::new(10, 30),
            used_in_script: false,
            used_in_style: false,
        }
    }

    #[test]
    fn typo_detected() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![make_binding("compued")], // typo for "computed"
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "typo 'compued' should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-potential-component-option-typo"));
        assert!(
            diags[0].message.contains("computed"),
            "message should suggest correct option"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn exact_match_passes() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![make_binding("computed")],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "exact option name should not trigger typo rule"
        );
    }

    #[test]
    fn unrelated_binding_passes() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![make_binding("myCounter"), make_binding("isVisible")],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "unrelated bindings should not trigger typo detection"
        );
    }

    #[test]
    fn near_match_helper() {
        assert!(is_near_match("compued", "computed"));
        assert!(is_near_match("metods", "methods"));
        assert!(!is_near_match("computed", "computed")); // exact = no typo
        assert!(!is_near_match("xyz", "computed")); // too different
        assert!(!is_near_match("abc", "data")); // short + different
    }
}
