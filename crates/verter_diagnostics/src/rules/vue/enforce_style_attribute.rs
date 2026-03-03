//! Rule: enforce-style-attribute
//!
//! Requires `scoped` attribute on `<style>` blocks. Reports `<style>` tags
//! that do not include the `scoped` attribute.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct EnforceStyleAttribute;

impl LintRule for EnforceStyleAttribute {
    fn name(&self) -> &'static str {
        "enforce-style-attribute"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let source = match file.source {
            Some(s) => s,
            None => return,
        };

        // Find all `<style` tags in the source
        let mut search_start = 0;
        while let Some(pos) = source[search_start..].find("<style") {
            let abs_pos = search_start + pos;
            let after_tag = abs_pos + "<style".len();

            // Find the closing `>` of this tag
            let tag_end = match source[after_tag..].find('>') {
                Some(i) => after_tag + i,
                None => break,
            };

            // Extract the attribute region between `<style` and `>`
            let attrs = &source[after_tag..tag_end];

            // Check if `scoped` is present as an attribute
            if !attrs.contains("scoped") {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "The `<style>` block should use the `scoped` attribute.".to_string(),
                    abs_pos as u32,
                    (tag_end + 1) as u32,
                    self.default_severity(),
                    DiagnosticSpanKind::FileLevel,
                );
            }

            search_start = tag_end + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::rules::FileContext;
    use crate::visitor::LintVisitor;

    fn run_file(source: &str) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(EnforceStyleAttribute)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        let file = FileContext {
            template: None,
            script: None,
            styles: &[],
            source: Some(source),
        };
        visitor.visit_file(&file, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn style_without_scoped_reports() {
        let source = "<style>\n.foo { color: red; }\n</style>";
        let diags = run_file(source);
        assert!(!diags.is_empty(), "style without scoped should trigger");
        assert!(diags.iter().any(|d| d.rule == "enforce-style-attribute"));
        assert!(
            diags[0].message.contains("scoped"),
            "message should mention scoped"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "block-order"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn style_with_scoped_passes() {
        let source = "<style scoped>\n.foo { color: red; }\n</style>";
        let diags = run_file(source);
        assert!(diags.is_empty(), "style with scoped should pass");
    }

    #[test]
    fn no_style_passes() {
        let source = "<template><div></div></template>";
        let diags = run_file(source);
        assert!(diags.is_empty(), "no style block should pass");
    }

    #[test]
    fn multiple_styles_mixed() {
        let source = "<style scoped>\n.a {}\n</style>\n<style>\n.b {}\n</style>";
        let diags = run_file(source);
        assert_eq!(diags.len(), 1, "only the non-scoped style should trigger");
        assert!(
            diags[0].message.contains("scoped"),
            "message should mention scoped"
        );
    }

    #[test]
    fn no_source_passes() {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(EnforceStyleAttribute)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        let file = FileContext {
            template: None,
            script: None,
            styles: &[],
            source: None,
        };
        visitor.visit_file(&file, &mut ctx);
        let diags = ctx.into_diagnostics();
        assert!(diags.is_empty(), "no source should pass");
    }
}
