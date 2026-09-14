//! Linter engine: runs rules against analysis data.

use crate::block_facts::SfcBlockFact;
use crate::comment_directives::parse_comment_directives;
use crate::config::LintConfig;
use crate::context::LintContext;
use crate::cross_file::CrossFileSnapshot;
use crate::diagnostic_set::DiagnosticSet;
use crate::rules::{FileContext, RuleRegistry};
use crate::visitor::LintVisitor;
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;
use verter_semantic::analysis::types::ScriptAnalysisSnapshot;
use verter_semantic::analysis::StyleBlockAnalysis;

/// Main linter engine. Holds the rule registry and configuration.
pub struct Linter {
    registry: RuleRegistry,
    config: LintConfig,
}

impl Linter {
    /// Create a new linter with the given configuration.
    pub fn new(config: LintConfig) -> Self {
        Self {
            registry: RuleRegistry::builtin(),
            config,
        }
    }

    /// Create a linter with a custom rule registry.
    pub fn with_registry(config: LintConfig, registry: RuleRegistry) -> Self {
        Self { registry, config }
    }

    /// Lint a file given its analysis data.
    ///
    /// Accepts optional script, template, and style analysis snapshots, the
    /// ordered block facts projected from the registered carrier inventory,
    /// plus an optional source string for rules that need byte-level access.
    /// Returns a [`DiagnosticSet`] that can be enriched before consumption.
    pub fn lint(
        &self,
        script: Option<&ScriptAnalysisSnapshot>,
        template: Option<&TemplateAnalysisSnapshot>,
        styles: &[StyleBlockAnalysis],
        blocks: &[SfcBlockFact],
    ) -> DiagnosticSet {
        self.lint_inner(script, template, styles, None, blocks, None)
    }

    /// Lint a file with the full SFC source available.
    ///
    /// Same as [`lint`](Self::lint) but provides source text for rules that
    /// need byte-level access (e.g., CSS class extraction).
    pub fn lint_with_source(
        &self,
        script: Option<&ScriptAnalysisSnapshot>,
        template: Option<&TemplateAnalysisSnapshot>,
        styles: &[StyleBlockAnalysis],
        source: Option<&str>,
        blocks: &[SfcBlockFact],
    ) -> DiagnosticSet {
        self.lint_inner(script, template, styles, source, blocks, None)
    }

    /// Lint a file with cross-file analysis data.
    ///
    /// Same as [`lint`](Self::lint) but also runs cross-file rules using the
    /// pre-computed [`CrossFileSnapshot`].
    pub fn lint_with_cross_file(
        &self,
        script: Option<&ScriptAnalysisSnapshot>,
        template: Option<&TemplateAnalysisSnapshot>,
        styles: &[StyleBlockAnalysis],
        blocks: &[SfcBlockFact],
        cross_file: Option<&CrossFileSnapshot>,
    ) -> DiagnosticSet {
        self.lint_inner(script, template, styles, None, blocks, cross_file)
    }

    /// Full lint pipeline.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn lint_inner(
        &self,
        script: Option<&ScriptAnalysisSnapshot>,
        template: Option<&TemplateAnalysisSnapshot>,
        styles: &[StyleBlockAnalysis],
        source: Option<&str>,
        blocks: &[SfcBlockFact],
        cross_file: Option<&CrossFileSnapshot>,
    ) -> DiagnosticSet {
        let rules = self.registry.rules();
        let mut ctx = LintContext::new(&self.config);
        let visitor = LintVisitor::new(rules);

        // Process comment directives from template first
        if let Some(tpl) = template {
            parse_comment_directives(&tpl.comment_directives, &mut ctx, source);
        }

        // Visit all analysis data
        if let Some(tpl) = template {
            visitor.visit_template(tpl, &mut ctx);
        }
        if let Some(s) = script {
            visitor.visit_script(s, &mut ctx);
        }
        visitor.visit_styles(styles, &mut ctx);

        // Visit file-level context (for rules that need cross-block reasoning)
        let file_ctx = FileContext {
            template,
            script,
            styles,
            source,
            blocks,
        };
        visitor.visit_file(&file_ctx, &mut ctx);

        // Visit cross-file data
        if let Some(cf) = cross_file {
            visitor.visit_cross_file(cf, &mut ctx);
        }

        ctx.into_diagnostic_set()
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &LintConfig {
        &self.config
    }

    /// Get a mutable reference to the configuration.
    pub fn config_mut(&mut self) -> &mut LintConfig {
        &mut self.config
    }
}

impl Default for Linter {
    fn default() -> Self {
        Self::new(LintConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::block_facts::{SfcBlockFact, SfcBlockRole};
    use crate::rules::{RuleApplicability, RuleRegistry};
    use verter_language::FrameworkAdapterId;
    use verter_span::Span;

    fn script_block() -> SfcBlockFact {
        SfcBlockFact {
            role: SfcBlockRole::Script,
            opening_span: Span::new(0, 8),
            content_span: Span::new(8, 8),
            attribute_insertion_anchor: 7,
            attributes: Vec::new(),
        }
    }

    fn rule_names(set: crate::diagnostic_set::DiagnosticSet) -> Vec<String> {
        set.into_diagnostics().into_iter().map(|d| d.rule).collect()
    }

    /// A Svelte component has no `<template>` host, so an empty element list
    /// is its normal shape — reporting Vue's root-element requirement there
    /// is a fabricated diagnostic. Selection happens by declared rule
    /// applicability against the registered carrier identity.
    #[test]
    fn svelte_carrier_selection_drops_vue_only_template_root_rule() {
        let svelte = FrameworkAdapterId::svelte();
        let linter = Linter::with_registry(
            LintConfig::default(),
            RuleRegistry::for_carrier(Some(&svelte)),
        );
        let template = TemplateAnalysisSnapshot::default();
        let names = rule_names(linter.lint(None, Some(&template), &[], &[]));
        assert!(
            !names.iter().any(|n| n == "valid-template-root"),
            "Vue-only valid-template-root must not run on a Svelte carrier, got {names:?}"
        );
    }

    /// The positive control for the same rule: an empty Vue template still
    /// reports, so the Svelte result above is selection and not suppression.
    #[test]
    fn vue_carrier_selection_keeps_vue_only_template_root_rule() {
        let vue = FrameworkAdapterId::vue();
        let linter =
            Linter::with_registry(LintConfig::default(), RuleRegistry::for_carrier(Some(&vue)));
        let template = TemplateAnalysisSnapshot::default();
        let names = rule_names(linter.lint(None, Some(&template), &[], &[]));
        assert!(
            names.iter().any(|n| n == "valid-template-root"),
            "empty Vue template must still report valid-template-root, got {names:?}"
        );
    }

    /// Applicable work survives selection: `block-lang` reads a `<script>`
    /// section's `lang` attribute, which means the same thing on every
    /// carrier, so it runs on Svelte as well as Vue.
    #[test]
    fn carrier_neutral_rule_runs_on_both_carriers() {
        let blocks = [script_block()];
        for adapter in [FrameworkAdapterId::vue(), FrameworkAdapterId::svelte()] {
            let linter = Linter::with_registry(
                LintConfig::default(),
                RuleRegistry::for_carrier(Some(&adapter)),
            );
            let names = rule_names(linter.lint(None, None, &[], &blocks));
            assert!(
                names.iter().any(|n| n == "block-lang"),
                "block-lang must run on the {adapter} carrier, got {names:?}"
            );
        }
    }

    /// `enforce-style-attribute` demands Vue's `scoped` attribute, which
    /// Svelte does not have — Svelte styles are component-scoped already.
    /// It is Vue-only and must not select onto a Svelte carrier.
    #[test]
    fn svelte_carrier_selection_drops_vue_only_scoped_style_rule() {
        let style = SfcBlockFact {
            role: SfcBlockRole::Style,
            opening_span: Span::new(0, 7),
            content_span: Span::new(7, 7),
            attribute_insertion_anchor: 6,
            attributes: Vec::new(),
        };
        let svelte = FrameworkAdapterId::svelte();
        let vue = FrameworkAdapterId::vue();

        let svelte_names = rule_names(
            Linter::with_registry(
                LintConfig::default(),
                RuleRegistry::for_carrier(Some(&svelte)),
            )
            .lint(None, None, &[], std::slice::from_ref(&style)),
        );
        assert!(
            !svelte_names.iter().any(|n| n == "enforce-style-attribute"),
            "Vue-only enforce-style-attribute must not run on Svelte, got {svelte_names:?}"
        );

        let vue_names = rule_names(
            Linter::with_registry(LintConfig::default(), RuleRegistry::for_carrier(Some(&vue)))
                .lint(None, None, &[], std::slice::from_ref(&style)),
        );
        assert!(
            vue_names.iter().any(|n| n == "enforce-style-attribute"),
            "the Vue control must still report enforce-style-attribute, got {vue_names:?}"
        );
    }

    /// Without a registered carrier identity no framework's component model
    /// can be claimed, so only carrier-neutral rules select.
    #[test]
    fn unregistered_carrier_selects_only_carrier_neutral_rules() {
        let registry = RuleRegistry::for_carrier(None);
        assert!(
            !registry.rules().is_empty(),
            "carrier-neutral rules must survive selection"
        );
        assert!(
            registry
                .rules()
                .iter()
                .all(|rule| rule.applicability() == RuleApplicability::CarrierNeutral),
            "an adapter-owned rule must not select without a registered carrier"
        );
        assert!(
            registry
                .rules()
                .iter()
                .any(|rule| rule.name() == "block-lang"),
            "block-lang is carrier-neutral and must survive"
        );
    }

    /// Selection is a real narrowing, not a no-op that leaves the Vue set
    /// intact under a different name.
    #[test]
    fn svelte_selection_is_strictly_narrower_than_vue_selection() {
        let vue = RuleRegistry::for_carrier(Some(&FrameworkAdapterId::vue()));
        let svelte = RuleRegistry::for_carrier(Some(&FrameworkAdapterId::svelte()));
        assert_eq!(vue.rules().len(), RuleRegistry::builtin().rules().len());
        assert!(
            svelte.rules().len() < vue.rules().len(),
            "Svelte selection must drop the Vue-only rules"
        );
    }

    #[test]
    fn linter_with_no_data_returns_empty() {
        let linter = Linter::new(LintConfig::default());
        let set = linter.lint(None, None, &[], &[]);
        assert!(set.is_empty());
    }

    #[test]
    fn linter_with_empty_analysis_returns_empty() {
        let mut config = LintConfig::default();
        // Disable valid-template-root since an empty snapshot has no elements
        config.rules.insert("valid-template-root".to_string(), None);
        let linter = Linter::new(config);
        let template = TemplateAnalysisSnapshot::default();
        let set = linter.lint(None, Some(&template), &[], &[]);
        assert!(set.is_empty());
    }

    #[test]
    fn linter_returns_diagnostic_set_with_enrichment_api() {
        let linter = Linter::new(LintConfig::default());
        let mut set = linter.lint(None, None, &[], &[]);
        assert_eq!(set.len(), 0);
        // DiagnosticSet supports enrichment: add + enhance
        set.add(crate::diagnostic::LintDiagnostic {
            rule: "test".to_string(),
            category: "test".to_string(),
            severity: crate::diagnostic::Severity::Warning,
            message: "test".to_string(),
            span: verter_span::Span::new(0, 10),
            tags: vec![],
            span_kind: crate::diagnostic::DiagnosticSpanKind::ElementOpenTag,
            certainty: crate::diagnostic::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        });
        assert_eq!(set.len(), 1);
        set.enhance(0, |d| d.message = "enriched".to_string());
        let diags = set.into_diagnostics();
        assert_eq!(diags[0].message, "enriched");
    }
}
