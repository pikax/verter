//! Rule: block-lang
//!
//! Enforces `<script lang="ts">` for TypeScript usage. Reports `<script>` blocks
//! that do not specify `lang="ts"`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct BlockLang;

impl LintRule for BlockLang {
    fn name(&self) -> &'static str {
        "block-lang"
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

        // Find all `<script` tags in the source
        let mut search_start = 0;
        while let Some(pos) = source[search_start..].find("<script") {
            let abs_pos = search_start + pos;
            let after_tag = abs_pos + "<script".len();

            // Find the closing `>` of this tag
            let tag_end = match source[after_tag..].find('>') {
                Some(i) => after_tag + i,
                None => break,
            };

            // Extract the attribute region between `<script` and `>`
            let attrs = &source[after_tag..tag_end];

            // Check if lang="ts" or lang='ts' is present
            let has_lang_ts = attrs.contains("lang=\"ts\"") || attrs.contains("lang='ts'");

            if !has_lang_ts {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "The `<script>` block should use `lang=\"ts\"` for TypeScript.".to_string(),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(BlockLang)];
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
    fn script_without_lang_reports() {
        let source = "<script setup>\nconst x = 1\n</script>";
        let diags = run_file(source);
        assert!(
            !diags.is_empty(),
            "script without lang=\"ts\" should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "block-lang"));
        assert!(
            diags[0].message.contains("lang=\"ts\""),
            "message should mention lang=\"ts\""
        );
        assert!(
            !diags.iter().any(|d| d.rule == "block-order"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn script_with_lang_ts_passes() {
        let source = "<script setup lang=\"ts\">\nconst x = 1\n</script>";
        let diags = run_file(source);
        assert!(diags.is_empty(), "script with lang=\"ts\" should pass");
    }

    #[test]
    fn script_with_lang_ts_single_quotes_passes() {
        let source = "<script setup lang='ts'>\nconst x = 1\n</script>";
        let diags = run_file(source);
        assert!(
            diags.is_empty(),
            "script with lang='ts' (single quotes) should pass"
        );
    }

    #[test]
    fn no_script_passes() {
        let source = "<template><div></div></template>";
        let diags = run_file(source);
        assert!(diags.is_empty(), "no script block should pass");
    }

    #[test]
    fn no_source_passes() {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(BlockLang)];
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
