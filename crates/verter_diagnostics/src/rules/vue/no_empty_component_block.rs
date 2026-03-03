//! Rule: no-empty-component-block
//!
//! Disallows empty `<script></script>` or `<style></style>` blocks.
//! Empty blocks should be removed to keep the SFC clean.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct NoEmptyComponentBlock;

impl LintRule for NoEmptyComponentBlock {
    fn name(&self) -> &'static str {
        "no-empty-component-block"
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

        // Check for empty script blocks
        self.check_empty_block(source, "script", ctx);
        // Check for empty style blocks
        self.check_empty_block(source, "style", ctx);
        // Check for empty template blocks
        self.check_empty_block(source, "template", ctx);
    }
}

impl NoEmptyComponentBlock {
    fn check_empty_block(&self, source: &str, tag: &str, ctx: &mut LintContext) {
        let open_prefix = format!("<{}", tag);
        let close_tag = format!("</{}>", tag);

        let mut search_start = 0;
        while let Some(pos) = source[search_start..].find(&open_prefix) {
            let abs_pos = search_start + pos;
            let after_tag = abs_pos + open_prefix.len();

            // Find the closing `>` of the opening tag
            let tag_end = match source[after_tag..].find('>') {
                Some(i) => after_tag + i + 1,
                None => break,
            };

            // Check if it's self-closing (e.g., `<script />`)
            if source[after_tag..tag_end].trim_end().ends_with('/') {
                // Self-closing tag counts as empty
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!("The `<{}>` block is empty and should be removed.", tag),
                    abs_pos as u32,
                    tag_end as u32,
                    self.default_severity(),
                    DiagnosticSpanKind::FileLevel,
                );
                search_start = tag_end;
                continue;
            }

            // Find the closing tag
            let close_pos = match source[tag_end..].find(&close_tag) {
                Some(i) => tag_end + i,
                None => break,
            };

            // Check if content between opening and closing tags is whitespace-only
            let content = &source[tag_end..close_pos];
            if content.trim().is_empty() {
                let block_end = close_pos + close_tag.len();
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!("The `<{}>` block is empty and should be removed.", tag),
                    abs_pos as u32,
                    block_end as u32,
                    self.default_severity(),
                    DiagnosticSpanKind::FileLevel,
                );
            }

            search_start = close_pos + close_tag.len();
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoEmptyComponentBlock)];
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
    fn empty_script_reports() {
        let source = "<script></script>";
        let diags = run_file(source);
        assert!(!diags.is_empty(), "empty script should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-empty-component-block"));
        assert!(
            diags[0].message.contains("<script>"),
            "message should mention script"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "block-order"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn empty_style_reports() {
        let source = "<style></style>";
        let diags = run_file(source);
        assert!(!diags.is_empty(), "empty style should trigger");
        assert!(diags[0].message.contains("<style>"));
    }

    #[test]
    fn empty_script_whitespace_reports() {
        let source = "<script>  \n  \n  </script>";
        let diags = run_file(source);
        assert!(!diags.is_empty(), "whitespace-only script should trigger");
    }

    #[test]
    fn non_empty_script_passes() {
        let source = "<script>\nconst x = 1\n</script>";
        let diags = run_file(source);
        assert!(diags.is_empty(), "non-empty script should pass");
    }

    #[test]
    fn non_empty_style_passes() {
        let source = "<style>\n.foo { color: red; }\n</style>";
        let diags = run_file(source);
        assert!(diags.is_empty(), "non-empty style should pass");
    }

    #[test]
    fn no_blocks_passes() {
        let source = "<template><div></div></template>";
        let diags = run_file(source);
        // template has content so passes
        assert!(diags.is_empty(), "template with content should pass");
    }
}
