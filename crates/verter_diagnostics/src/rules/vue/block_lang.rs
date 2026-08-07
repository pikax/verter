//! Rule: block-lang
//!
//! Enforces `<script lang="ts">` for TypeScript usage. Reports `<script>` blocks
//! that do not specify `lang="ts"`, read from the ordered inventory facts
//! (parsed roles + parsed attributes) — never a raw-source delimiter scan.

// @ai-generated

use crate::block_facts::SfcBlockRole;
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

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        for block in file.blocks {
            if block.role != SfcBlockRole::Script {
                continue;
            }
            let has_lang_ts = block.attributes.iter().any(|attribute| {
                attribute.name == "lang" && attribute.value.as_deref() == Some("ts")
            });
            if !has_lang_ts {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "The `<script>` block should use `lang=\"ts\"` for TypeScript.".to_string(),
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

    fn run_blocks(blocks: &[SfcBlockFact]) -> Vec<crate::diagnostic::LintDiagnostic> {
        run_blocks_with_source(blocks, None)
    }

    fn run_blocks_with_source(
        blocks: &[SfcBlockFact],
        source: Option<&str>,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(BlockLang)];
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

    fn script_block(attributes: Vec<(&str, Option<&str>)>) -> SfcBlockFact {
        SfcBlockFact {
            role: SfcBlockRole::Script,
            attribute_insertion_anchor: 0,
            opening_span: Span::new(0, 14),
            content_span: Span::new(0, 0),
            attributes: attributes
                .into_iter()
                .map(|(name, value)| SfcBlockAttribute {
                    name: name.to_string(),
                    value: value.map(str::to_string),
                    name_span: Span::new(0, 0),
                })
                .collect(),
        }
    }

    #[test]
    fn script_without_lang_reports() {
        let diags = run_blocks(&[script_block(vec![("setup", None)])]);
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
        let diags = run_blocks(&[script_block(vec![("setup", None), ("lang", Some("ts"))])]);
        assert!(diags.is_empty(), "script with lang=\"ts\" should pass");
    }

    #[test]
    fn no_script_passes() {
        let diags = run_blocks(&[SfcBlockFact {
            role: SfcBlockRole::Template,
            attribute_insertion_anchor: 0,
            opening_span: Span::new(0, 10),
            content_span: Span::new(0, 0),
            attributes: vec![],
        }]);
        assert!(diags.is_empty(), "no script block should pass");
    }

    #[test]
    fn decoy_script_literal_inside_string_is_not_a_block() {
        // The ordered inventory facts for this source contain exactly ONE
        // script block (lang="ts"); the '<script>' STRING LITERAL inside the
        // body never becomes a block, so no diagnostic fires. The retired
        // raw-source scan reported the decoy occurrence — the source stays in
        // the context precisely so a scan regression would fire again.
        let source = "<script setup lang=\"ts\">\nconst s = '<script>'\n</script>";
        let diags = run_blocks_with_source(
            &[script_block(vec![("setup", None), ("lang", Some("ts"))])],
            Some(source),
        );
        assert!(
            diags.is_empty(),
            "a '<script>' STRING LITERAL inside the script body is not a block: {diags:?}"
        );
    }

    #[test]
    fn no_block_facts_stay_silent() {
        let diags = run_blocks(&[]);
        assert!(diags.is_empty(), "no inventory facts must mean no report");
    }
}
