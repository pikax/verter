//! Rule: `scoped-css-cascade`
//!
//! Detects scoped CSS class names that cascade to child components via class attributes.

use std::collections::HashSet;

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct ScopedCssCascade;

impl LintRule for ScopedCssCascade {
    fn name(&self) -> &'static str {
        "scoped-css-cascade"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Css
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Hint)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let template = match file.template {
            Some(t) => t,
            None => return,
        };

        // Collect all class names from scoped CSS selectors
        let mut scoped_classes: HashSet<&str> = HashSet::default();

        for style in file.styles {
            if !style.scoped {
                continue;
            }
            if let Some(css) = &style.css {
                for class in &css.classes {
                    scoped_classes.insert(&class.name);
                }
            }
        }

        if scoped_classes.is_empty() {
            return;
        }

        for comp in &template.components {
            // Check static classes that match scoped selectors
            let static_cascading: Vec<&String> = comp
                .static_classes
                .iter()
                .filter(|c| scoped_classes.contains(c.as_str()))
                .collect();

            // Check dynamic (conditional) classes that match scoped selectors
            let dynamic_cascading: Vec<&String> = comp
                .dynamic_classes
                .iter()
                .filter(|c| scoped_classes.contains(c.as_str()))
                .collect();

            if static_cascading.is_empty() && dynamic_cascading.is_empty() {
                continue;
            }

            if !static_cascading.is_empty() {
                let class_list = static_cascading
                    .iter()
                    .map(|c| format!("`.{c}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                ctx.report_hint(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Scoped style {class_list} cascades to `<{}>` via class attribute",
                        comp.name,
                    ),
                    comp.span.start,
                    comp.span.end,
                    DiagnosticSpanKind::ElementOpenTag,
                );
            }
            if !dynamic_cascading.is_empty() {
                let class_list = dynamic_cascading
                    .iter()
                    .map(|c| format!("`.{c}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                ctx.report_hint(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Scoped style {class_list} may cascade to `<{}>` via conditional :class binding",
                        comp.name,
                    ),
                    comp.span.start,
                    comp.span.end,
                    DiagnosticSpanKind::ElementOpenTag,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::rules::FileContext;

    use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateComponentUsage};
    use verter_analysis::{style, StyleBlockAnalysis};
    use verter_span::Span;

    fn build_style(css_content: &str, scoped: bool, content_offset: u32) -> StyleBlockAnalysis {
        let mut analysis = style::build_css_style_analysis(
            css_content,
            style::VueStyleInput {
                v_binds: vec![],
                special_pseudos: vec![],
            },
            scoped,
            false,
            None,
            content_offset,
        );
        if let Some(ref mut css) = analysis.css {
            for sel in &mut css.selectors {
                sel.span.start += content_offset;
                sel.span.end += content_offset;
            }
            for cls in &mut css.classes {
                cls.span.start += content_offset;
                cls.span.end += content_offset;
            }
            for id in &mut css.ids {
                id.span.start += content_offset;
                id.span.end += content_offset;
            }
        }
        analysis
    }

    fn run_rule(file: &FileContext<'_>) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_file_rule(ScopedCssCascade, file)
    }

    #[test]
    fn detects_cascade_to_child() {
        let css = ".foo { color: red; }";
        let style = build_style(css, true, 100);

        let template = TemplateAnalysisSnapshot {
            components: vec![TemplateComponentUsage {
                name: "Child".into(),
                import_source: Some("./Child.vue".into()),
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec!["foo".into()],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                span: Span::new(10, 20),
            }],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: None,
        };

        let diags = run_rule(&file);
        assert_eq!(diags.len(), 1, "should detect cascade");
        assert!(diags[0].message.contains(".foo"));
        assert!(diags[0].message.contains("<Child>"));
        assert_eq!(diags[0].severity, Severity::Hint);
        assert_eq!(diags[0].rule, "scoped-css-cascade");
    }

    #[test]
    fn no_cascade_without_matching_classes() {
        let css = ".foo { color: red; }";
        let style = build_style(css, true, 100);

        let template = TemplateAnalysisSnapshot {
            components: vec![TemplateComponentUsage {
                name: "Child".into(),
                import_source: Some("./Child.vue".into()),
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec!["bar".into()],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                span: Span::new(10, 20),
            }],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: None,
        };

        let diags = run_rule(&file);
        assert!(
            diags.is_empty(),
            "should not detect cascade for non-matching classes"
        );
    }

    #[test]
    fn no_cascade_non_scoped() {
        let css = ".foo { color: red; }";
        let style = build_style(css, false, 100);

        let template = TemplateAnalysisSnapshot {
            components: vec![TemplateComponentUsage {
                name: "Child".into(),
                import_source: None,
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec!["foo".into()],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                span: Span::new(10, 20),
            }],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: None,
        };

        let diags = run_rule(&file);
        assert!(diags.is_empty(), "should not detect cascade for non-scoped");
    }

    #[test]
    fn detects_dynamic_cascade() {
        let css = ".foo { color: red; }";
        let style = build_style(css, true, 100);

        let template = TemplateAnalysisSnapshot {
            components: vec![TemplateComponentUsage {
                name: "Child".into(),
                import_source: None,
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: true,
                dynamic_classes: vec!["foo".into()],
                v_models: vec![],
                span: Span::new(10, 20),
            }],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: None,
        };

        let diags = run_rule(&file);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("conditional :class"));
    }
}
