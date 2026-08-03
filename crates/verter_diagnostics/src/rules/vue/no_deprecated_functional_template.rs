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

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        // The ordered inventory facts (parsed roles + parsed attributes) are
        // the sole authority — never a raw-source `<template` scan, so a
        // decoy literal inside a script string can never fabricate the block.
        for block in file.blocks {
            if block.role != crate::block_facts::SfcBlockRole::Template {
                continue;
            }
            let Some(attribute) = block
                .attributes
                .iter()
                .find(|attribute| attribute.name == "functional")
            else {
                continue;
            };
            ctx.report_with_tags(
                self.name(),
                self.category().as_str(),
                "'<template functional>' is not supported in Vue 3. Use a plain function component instead.".to_string(),
                attribute.name_span.start,
                attribute.name_span.end,
                self.default_severity(),
                vec![DiagnosticTag::Deprecated],
                DiagnosticSpanKind::Attribute,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_facts::{SfcBlockAttribute, SfcBlockFact, SfcBlockRole};
    use crate::config::LintConfig;
    use crate::rules::FileContext;
    use crate::visitor::LintVisitor;
    use verter_span::Span;

    fn template_block(attributes: Vec<(&str, u32)>) -> SfcBlockFact {
        SfcBlockFact {
            role: SfcBlockRole::Template,
            opening_span: Span::new(0, 21),
            content_span: Span::new(21, 40),
            attribute_insertion_anchor: 20,
            attributes: attributes
                .into_iter()
                .map(|(name, start)| SfcBlockAttribute {
                    name: name.to_string(),
                    value: None,
                    name_span: Span::new(start, start + name.len() as u32),
                })
                .collect(),
        }
    }

    fn run_blocks_with_source(
        blocks: &[SfcBlockFact],
        source: Option<&str>,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedFunctionalTemplate)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        let file = FileContext {
            template: None,
            script: None,
            styles: &[],
            source,
            blocks,
        };
        visitor.visit_file(&file, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn decoy_functional_template_literal_inside_script_string_is_not_a_block() {
        // The ordered inventory facts for this source contain no functional
        // template attribute; the '<template functional>' STRING LITERAL in
        // the script body never becomes a block. The retired raw-source scan
        // reported the decoy — the source stays in the context precisely so a
        // scan regression would fire again.
        let source = "<script setup>\nconst s = '<template functional>'\n</script>\n<template><div /></template>";
        let template_start = source.rfind("<template>").unwrap() as u32;
        let diags = run_blocks_with_source(
            &[SfcBlockFact {
                role: SfcBlockRole::Template,
                opening_span: Span::new(template_start, template_start + 10),
                content_span: Span::new(template_start + 10, template_start + 19),
                attribute_insertion_anchor: template_start + 9,
                attributes: vec![],
            }],
            Some(source),
        );
        assert!(
            diags.is_empty(),
            "a '<template functional>' STRING LITERAL inside the script body is not a block: {diags:?}"
        );
    }

    #[test]
    fn functional_template_reports() {
        let diags = run_blocks_with_source(&[template_block(vec![("functional", 10)])], None);
        assert!(!diags.is_empty(), "<template functional> should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-functional-template"));
        assert_eq!(
            diags[0].span,
            Span::new(10, 20),
            "reports the attribute name span"
        );
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
        let diags = run_blocks_with_source(&[template_block(vec![])], None);
        assert!(diags.is_empty(), "normal <template> should pass");
    }
}
