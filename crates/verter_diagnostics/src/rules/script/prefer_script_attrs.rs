//! Rule: prefer-script-attrs
//!
//! Suggests using `<script setup attrs="T">` instead of `useAttrs<T>()`.
//! Only triggers when `useAttrs()` is called with a type parameter.
//! Plain `useAttrs()` without type parameter does NOT trigger this rule.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};
use verter_analysis::types::VueApiClassification;

pub struct PreferScriptAttrs;

impl LintRule for PreferScriptAttrs {
    fn name(&self) -> &'static str {
        "prefer-script-attrs"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let Some(script) = file.script else { return };
        let Some(source) = file.source else { return };
        let source_bytes = source.as_bytes();

        for call in &script.vue_api_calls {
            if call.api != VueApiClassification::UseAttrs {
                continue;
            }

            // Check source text for type parameter: useAttrs<T>()
            let start = call.span.start as usize;
            let end = call.span.end as usize;
            if end > source_bytes.len() || start >= end {
                continue;
            }

            let call_text = &source_bytes[start..end];
            // Look for `useAttrs<` pattern — indicates type parameter present
            if !has_type_parameter(call_text) {
                continue;
            }

            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Prefer `<script setup attrs=\"T\">` over `useAttrs<T>()` for typed $attrs."
                    .to_string(),
                call.span.start,
                call.span.end,
                self.default_severity(),
                DiagnosticSpanKind::ScriptCallSite,
            );
        }
    }
}

/// Check if a `useAttrs(...)` call text contains a type parameter `<T>`.
fn has_type_parameter(call_text: &[u8]) -> bool {
    // Find `useAttrs` then check for `<` before `(`
    let needle = b"useAttrs";
    let Some(pos) = call_text.windows(needle.len()).position(|w| w == needle) else {
        return false;
    };
    let after = &call_text[pos + needle.len()..];
    // Skip whitespace, then check for `<`
    let first_non_ws = after
        .iter()
        .find(|&&b| b != b' ' && b != b'\t' && b != b'\n' && b != b'\r');
    matches!(first_non_ws, Some(b'<'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(
        script: &ScriptAnalysisSnapshot,
        source: &str,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(PreferScriptAttrs)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);

        let file_ctx = FileContext {
            template: None,
            script: Some(script),
            styles: &[],
            source: Some(source),
        };
        visitor.visit_file(&file_ctx, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn use_attrs_with_type_param_reports() {
        // Source text with useAttrs<T>() at byte positions 50..95
        let source = r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string }>()
</script>"#;

        // Find the actual span of useAttrs<...>() in the source
        let call_start = source.find("useAttrs<").unwrap() as u32;
        let call_end = source[call_start as usize..].find("()").unwrap() as u32 + call_start + 2;

        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::UseAttrs,
                span: Span::new(call_start, call_end),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule(&script, source);

        // Positive: should report
        assert!(
            !diags.is_empty(),
            "useAttrs<T>() should trigger prefer-script-attrs"
        );
        assert!(diags.iter().any(|d| d.rule == "prefer-script-attrs"));
        assert!(
            diags[0].message.contains("attrs"),
            "message should mention attrs"
        );
        // Negative: should not trigger other rules
        assert!(
            !diags.iter().any(|d| d.rule != "prefer-script-attrs"),
            "should not trigger unrelated rules"
        );
    }

    #[test]
    fn use_attrs_without_type_param_passes() {
        let source = r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs()
</script>"#;

        let call_start = source.find("useAttrs()").unwrap() as u32;
        let call_end = call_start + "useAttrs()".len() as u32;

        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::UseAttrs,
                span: Span::new(call_start, call_end),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule(&script, source);
        assert!(
            diags.is_empty(),
            "useAttrs() without type param should not trigger"
        );
    }

    #[test]
    fn no_use_attrs_passes() {
        let source = "<script setup lang=\"ts\">\nconst x = 1\n</script>";
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule(&script, source);
        assert!(diags.is_empty(), "no useAttrs call should not trigger");
    }

    #[test]
    fn use_slots_with_type_param_passes() {
        // useSlots<T>() should NOT trigger this rule
        let source = r#"<script setup lang="ts">
import { useSlots } from 'vue'
const slots = useSlots<{ default: () => void }>()
</script>"#;

        let call_start = source.find("useSlots<").unwrap() as u32;
        let call_end = source[call_start as usize..].find("()").unwrap() as u32 + call_start + 2;

        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::UseSlots,
                span: Span::new(call_start, call_end),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule(&script, source);
        assert!(
            diags.is_empty(),
            "useSlots<T>() should not trigger prefer-script-attrs"
        );
    }
}
