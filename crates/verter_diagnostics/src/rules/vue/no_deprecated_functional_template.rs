//! Rule: no-deprecated-functional-template
//!
//! `<template functional>` was removed in Vue 3. Functional components are now
//! plain functions. Detect `functional` attribute on `<template>` in SFC source.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct NoDeprecatedFunctionalTemplate;

impl LintRule for NoDeprecatedFunctionalTemplate {
    fn name(&self) -> &'static str {
        "no-deprecated-functional-template"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let source = match file.source {
            Some(s) => s,
            None => return,
        };

        // Look for `<template functional` or `<template  functional` in source
        if let Some(idx) = source.find("<template") {
            let rest = &source[idx..];
            // Check if `functional` appears before the closing `>`
            if let Some(gt_pos) = rest.find('>') {
                let tag_content = &rest[..gt_pos];
                if tag_content.contains("functional") {
                    let func_offset = idx + tag_content.find("functional").unwrap();
                    ctx.report_with_tags(
                        self.name(),
                        self.category().as_str(),
                        "'<template functional>' is not supported in Vue 3. Use a plain function component instead.".to_string(),
                        func_offset as u32,
                        (func_offset + "functional".len()) as u32,
                        self.default_severity(),
                        vec![DiagnosticTag::Deprecated],
                        DiagnosticSpanKind::Attribute,
                    );
                }
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedFunctionalTemplate)];
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
    fn functional_template_reports() {
        let source = r#"<template functional><div>hello</div></template>"#;
        let diags = run_file(source);
        assert!(!diags.is_empty(), "<template functional> should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-functional-template"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn normal_template_passes() {
        let source = r#"<template><div>hello</div></template>"#;
        let diags = run_file(source);
        assert!(diags.is_empty(), "normal <template> should pass");
    }
}
