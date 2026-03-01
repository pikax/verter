//! Rule: no-deprecated-v-on-native-modifier
//!
//! The `.native` modifier for `v-on` was removed in Vue 3.
//! Use `v-on` without `.native` on components instead, relying on the
//! component's `emits` option to distinguish native/component events.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct NoDeprecatedVOnNativeModifier;

impl LintRule for NoDeprecatedVOnNativeModifier {
    fn name(&self) -> &'static str {
        "no-deprecated-v-on-native-modifier"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "on" {
            return;
        }
        if !dir.modifiers.iter().any(|m| m == "native") {
            return;
        }
        let event = dir.argument.as_deref().unwrap_or("event");
        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "The '.native' modifier for 'v-on' is not supported in Vue 3. \
                 Use '@{event}' without '.native' and configure 'emits' in the component.",
            ),
            dir.span.start,
            dir.span.end,
            self.default_severity(),
            DiagnosticSpanKind::Directive,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedVOnNativeModifier)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_el_with_directive(dir: TemplateDirective) -> TemplateElement {
        TemplateElement {
            tag: "MyComp".to_string(),
            is_component: true,
            directives: vec![dir],
            ..Default::default()
        }
    }

    #[test]
    fn native_modifier_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_directive(TemplateDirective {
                name: "on".to_string(),
                raw_name: "@click.native".to_string(),
                argument: Some("click".to_string()),
                modifiers: vec!["native".to_string()],
                expression: Some("handler".to_string()),
                span: Span::new(10, 30),
            })],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), ".native modifier should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-v-on-native-modifier"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn without_native_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_directive(TemplateDirective {
                name: "on".to_string(),
                raw_name: "@click.stop".to_string(),
                argument: Some("click".to_string()),
                modifiers: vec!["stop".to_string()],
                expression: Some("handler".to_string()),
                span: Span::new(10, 25),
            })],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "non-native modifier should pass");
    }
}
