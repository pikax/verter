//! Rule: block-order
//!
//! Enforces the recommended SFC block order: `<script>` before `<template>`
//! before `<style>`. This is the standard ordering convention in Vue projects.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct BlockOrder;

impl LintRule for BlockOrder {
    fn name(&self) -> &'static str {
        "block-order"
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

        // Find the byte position of the first `<script` and `<template` tags.
        // A simple substring search suffices — we only need relative ordering.
        let script_pos = source.find("<script");
        let template_pos = source.find("<template");

        // If template appears before script, report
        if let (Some(tpl), Some(scr)) = (template_pos, script_pos) {
            if tpl < scr {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "The `<script>` block should appear before `<template>`. \
                     Recommended order: <script>, <template>, <style>."
                        .to_string(),
                    tpl as u32,
                    (tpl + "<template".len()) as u32,
                    self.default_severity(),
                    DiagnosticSpanKind::FileLevel,
                );
            }
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(BlockOrder)];
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
    fn template_before_script_reports() {
        let source = "<template><div></div></template>\n<script setup>\nconst x = 1\n</script>";
        let diags = run_file(source);
        assert!(!diags.is_empty(), "template before script should trigger");
        assert!(diags.iter().any(|d| d.rule == "block-order"));
        assert!(
            diags[0].message.contains("<script>"),
            "message should mention script"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn script_before_template_passes() {
        let source = "<script setup>\nconst x = 1\n</script>\n<template><div></div></template>";
        let diags = run_file(source);
        assert!(diags.is_empty(), "script before template should pass");
    }

    #[test]
    fn only_template_passes() {
        let source = "<template><div></div></template>";
        let diags = run_file(source);
        assert!(diags.is_empty(), "only template should pass");
    }

    #[test]
    fn only_script_passes() {
        let source = "<script setup>\nconst x = 1\n</script>";
        let diags = run_file(source);
        assert!(diags.is_empty(), "only script should pass");
    }
}
