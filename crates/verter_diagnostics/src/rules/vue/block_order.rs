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

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        // Relative ordering comes from the ORDERED inventory facts — never a
        // raw-source substring search (decoy `<template` literals inside
        // comments or strings are not blocks).
        let script_pos = file
            .blocks
            .iter()
            .find(|block| block.role == crate::block_facts::SfcBlockRole::Script)
            .map(|block| block.opening_span.start);
        let template_pos = file
            .blocks
            .iter()
            .find(|block| block.role == crate::block_facts::SfcBlockRole::Template)
            .map(|block| block.opening_span);

        // If template appears before script, report
        if let (Some(template_span), Some(script_start)) = (template_pos, script_pos) {
            if template_span.start < script_start {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "The `<script>` block should appear before `<template>`. \
                     Recommended order: <script>, <template>, <style>."
                        .to_string(),
                    template_span.start,
                    template_span.end,
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
    use crate::block_facts::{SfcBlockFact, SfcBlockRole};
    use crate::config::LintConfig;
    use crate::rules::FileContext;
    use crate::visitor::LintVisitor;
    use verter_span::Span;

    fn run_blocks_with_source(
        blocks: &[SfcBlockFact],
        source: Option<&str>,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(BlockOrder)];
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

    fn run_blocks(blocks: &[SfcBlockFact]) -> Vec<crate::diagnostic::LintDiagnostic> {
        run_blocks_with_source(blocks, None)
    }

    fn block(role: SfcBlockRole, start: u32) -> SfcBlockFact {
        SfcBlockFact {
            role,
            attribute_insertion_anchor: 0,
            opening_span: Span::new(start, start + 10),
            content_span: Span::new(0, 0),
            attributes: vec![],
        }
    }

    #[test]
    fn decoy_template_in_root_comment_does_not_reorder_blocks() {
        // The ordered inventory facts for this source place the script block
        // FIRST; the `<template>` literal inside the ROOT COMMENT never
        // becomes a block. The retired raw-source substring search found the
        // comment occurrence first and reported a false reorder — the source
        // stays in the context precisely so a scan regression would fire
        // again.
        let source = "<!-- <template> demo -->\n<script setup>\nconst x = 1\n</script>\n<template><div></div></template>";
        let script_start = source.find("<script").unwrap() as u32;
        let template_start = source.rfind("<template>").unwrap() as u32;
        let diags = run_blocks_with_source(
            &[
                block(SfcBlockRole::Script, script_start),
                block(SfcBlockRole::Template, template_start),
            ],
            Some(source),
        );
        assert!(
            diags.is_empty(),
            "a <template> literal inside a ROOT COMMENT is not a block: {diags:?}"
        );
    }

    #[test]
    fn template_before_script_reports() {
        let diags = run_blocks(&[
            block(SfcBlockRole::Template, 0),
            block(SfcBlockRole::Script, 33),
        ]);
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
        let diags = run_blocks(&[
            block(SfcBlockRole::Script, 0),
            block(SfcBlockRole::Template, 40),
        ]);
        assert!(diags.is_empty(), "script before template should pass");
    }

    #[test]
    fn only_template_passes() {
        let diags = run_blocks(&[block(SfcBlockRole::Template, 0)]);
        assert!(diags.is_empty(), "only template should pass");
    }

    #[test]
    fn only_script_passes() {
        let diags = run_blocks(&[block(SfcBlockRole::Script, 0)]);
        assert!(diags.is_empty(), "only script should pass");
    }

    #[test]
    fn no_block_facts_stay_silent() {
        let diags = run_blocks(&[]);
        assert!(diags.is_empty(), "no inventory facts must mean no report");
    }
}
