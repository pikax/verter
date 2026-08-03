//! Rule: enforce-style-attribute
//!
//! Requires `scoped` attribute on `<style>` blocks, read from the ordered
//! inventory facts (parsed roles + parsed attributes) — never a raw-source
//! delimiter scan.

// @ai-generated

use crate::block_facts::SfcBlockRole;
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

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        for block in file.blocks {
            if block.role != SfcBlockRole::Style {
                continue;
            }
            let has_scoped = block
                .attributes
                .iter()
                .any(|attribute| attribute.name == "scoped");
            if !has_scoped {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "The `<style>` block should use the `scoped` attribute.".to_string(),
                    block.opening_span.start,
                    block.opening_span.end,
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
    use crate::block_facts::{SfcBlockAttribute, SfcBlockFact};
    use crate::config::LintConfig;
    use crate::rules::FileContext;
    use crate::visitor::LintVisitor;
    use verter_span::Span;

    fn run_blocks_with_source(
        blocks: &[SfcBlockFact],
        source: Option<&str>,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(EnforceStyleAttribute)];
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

    fn style_block(start: u32, attributes: Vec<&str>) -> SfcBlockFact {
        SfcBlockFact {
            role: SfcBlockRole::Style,
            attribute_insertion_anchor: 0,
            opening_span: Span::new(start, start + 7),
            content_span: Span::new(0, 0),
            attributes: attributes
                .into_iter()
                .map(|name| SfcBlockAttribute {
                    name: name.to_string(),
                    value: None,
                    name_span: Span::new(0, 0),
                })
                .collect(),
        }
    }

    #[test]
    fn style_without_scoped_reports() {
        let diags = run_blocks(&[style_block(0, vec![])]);
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
        let diags = run_blocks(&[style_block(0, vec!["scoped"])]);
        assert!(diags.is_empty(), "style with scoped should pass");
    }

    #[test]
    fn no_style_passes() {
        let diags = run_blocks(&[SfcBlockFact {
            role: SfcBlockRole::Template,
            attribute_insertion_anchor: 0,
            opening_span: Span::new(0, 10),
            content_span: Span::new(0, 0),
            attributes: vec![],
        }]);
        assert!(diags.is_empty(), "no style block should pass");
    }

    #[test]
    fn multiple_styles_mixed() {
        let diags = run_blocks(&[style_block(0, vec!["scoped"]), style_block(30, vec![])]);
        assert_eq!(diags.len(), 1, "only the non-scoped style should trigger");
        assert!(
            diags[0].message.contains("scoped"),
            "message should mention scoped"
        );
    }

    #[test]
    fn decoy_style_literal_inside_script_string_is_not_a_block() {
        // The ordered inventory facts for this source contain exactly ONE
        // style block (scoped); the '<style>' STRING LITERAL inside the
        // script body never becomes a block. The retired raw-source scan
        // reported the decoy occurrence — the source stays in the context
        // precisely so a scan regression would fire again.
        let source =
            "<script setup>\nconst css = '<style>'\n</script>\n<style scoped>\n.a {}\n</style>";
        let style_start = source.rfind("<style scoped>").unwrap() as u32;
        let diags =
            run_blocks_with_source(&[style_block(style_start, vec!["scoped"])], Some(source));
        assert!(
            diags.is_empty(),
            "a '<style>' STRING LITERAL inside the script body is not a block: {diags:?}"
        );
    }

    #[test]
    fn no_block_facts_stay_silent() {
        let diags = run_blocks(&[]);
        assert!(diags.is_empty(), "no inventory facts must mean no report");
    }
}
