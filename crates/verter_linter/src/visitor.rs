//! Single-pass DFS visitor that calls all active lint rules.

use crate::context::LintContext;
use crate::rules::LintRule;
use verter_analysis::template::{BindingUsageKind, TemplateAnalysisSnapshot};
use verter_analysis::types::ScriptAnalysisSnapshot;
use verter_analysis::StyleBlockAnalysis;

/// Visitor that runs all rules against analysis data in a single pass.
pub struct LintVisitor<'a> {
    rules: &'a [Box<dyn LintRule>],
}

impl<'a> LintVisitor<'a> {
    /// Create a new visitor with the given rules.
    pub fn new(rules: &'a [Box<dyn LintRule>]) -> Self {
        Self { rules }
    }

    /// Visit template analysis data.
    pub fn visit_template(&self, template: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // 1. Call check_template on all rules
        for rule in self.rules {
            rule.check_template(template, ctx);
        }

        // 2. Walk elements
        for element in &template.elements {
            for rule in self.rules {
                rule.check_element(element, ctx);
            }

            // Walk directives on this element
            for directive in &element.directives {
                for rule in self.rules {
                    rule.check_directive(directive, element, ctx);
                }
            }

            // Walk v-for if present
            if let Some(v_for) = &element.v_for {
                for rule in self.rules {
                    rule.check_v_for(v_for, element, ctx);
                }
            }
        }

        // 3. Walk binding occurrences (interpolations)
        for occurrence in &template.binding_occurrences {
            if occurrence.usage_kind == BindingUsageKind::Interpolation {
                for rule in self.rules {
                    rule.check_interpolation(occurrence, ctx);
                }
            }
        }
    }

    /// Visit script analysis data.
    pub fn visit_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for rule in self.rules {
            rule.check_script(script, ctx);
        }
    }

    /// Visit style analysis data.
    pub fn visit_styles(&self, styles: &[StyleBlockAnalysis], ctx: &mut LintContext) {
        for style in styles {
            for rule in self.rules {
                rule.check_style(style, ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::diagnostic::Severity;
    use crate::rules::RuleCategory;

    /// A test rule that counts elements.
    struct CountElementsRule;

    impl LintRule for CountElementsRule {
        fn name(&self) -> &'static str {
            "count-elements"
        }
        fn category(&self) -> RuleCategory {
            RuleCategory::VueEssential
        }
        fn default_severity(&self) -> Severity {
            Severity::Warning
        }
        fn check_element(
            &self,
            el: &verter_analysis::template::TemplateElement,
            ctx: &mut LintContext,
        ) {
            if el.tag == "div" {
                ctx.report(
                    self.name(),
                    self.category().as_str(),
                    "Found a div".to_string(),
                    el.span_start,
                    el.span_end,
                );
            }
        }
    }

    #[test]
    fn visitor_calls_element_rules() {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(CountElementsRule)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);

        let template = TemplateAnalysisSnapshot {
            elements: vec![
                verter_analysis::template::TemplateElement {
                    tag: "div".to_string(),
                    is_component: false,
                    is_self_closing: false,
                    namespace: verter_analysis::template::ElementNamespace::Html,
                    attributes: vec![],
                    directives: vec![],
                    v_for: None,
                    v_model: None,
                    has_v_if: false,
                    has_v_else: false,
                    has_v_else_if: false,
                    has_v_show: false,
                    has_v_html: false,
                    has_v_text: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    span_start: 0,
                    span_end: 10,
                },
                verter_analysis::template::TemplateElement {
                    tag: "span".to_string(),
                    is_component: false,
                    is_self_closing: false,
                    namespace: verter_analysis::template::ElementNamespace::Html,
                    attributes: vec![],
                    directives: vec![],
                    v_for: None,
                    v_model: None,
                    has_v_if: false,
                    has_v_else: false,
                    has_v_else_if: false,
                    has_v_show: false,
                    has_v_html: false,
                    has_v_text: false,
                    nesting_depth: 1,
                    parent_tag: Some("div".to_string()),
                    span_start: 10,
                    span_end: 20,
                },
            ],
            ..Default::default()
        };

        visitor.visit_template(&template, &mut ctx);
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1); // Only the div
        assert_eq!(diags[0].message, "Found a div");
    }

    #[test]
    fn visitor_empty_rules_no_diagnostics() {
        let rules: Vec<Box<dyn LintRule>> = vec![];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);

        let template = TemplateAnalysisSnapshot::default();
        visitor.visit_template(&template, &mut ctx);
        assert!(ctx.into_diagnostics().is_empty());
    }
}
