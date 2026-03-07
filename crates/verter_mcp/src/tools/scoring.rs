//! Scoring engine: a11y, quality, and template complexity metrics.

use verter_analysis::template::TemplateAnalysisSnapshot;
use verter_analysis::types::{AnalysisFlags, ScriptAnalysisSnapshot, VueApiClassification};
use verter_analysis::StyleBlockAnalysis;
use verter_diagnostics::{LintConfig, LintPreset, Linter, Severity};

/// Template complexity metrics computed from existing analysis data.
#[derive(Debug, Default, serde::Serialize)]
pub struct TemplateComplexityMetrics {
    pub total_elements: u32,
    pub total_directives: u32,
    pub v_for_count: u32,
    pub v_if_count: u32,
    pub inline_handler_count: u32,
    pub dynamic_binding_count: u32,
    pub max_nesting_depth: u16,
    pub unique_components_used: u32,
    pub slot_usage_count: u32,
    pub template_ref_count: u32,
}

/// Compute template complexity metrics from a template snapshot.
pub fn compute_template_metrics(tpl: &TemplateAnalysisSnapshot) -> TemplateComplexityMetrics {
    let mut metrics = TemplateComplexityMetrics {
        total_elements: tpl.elements.len() as u32,
        max_nesting_depth: tpl.max_nesting_depth,
        unique_components_used: tpl.components.len() as u32,
        slot_usage_count: tpl.defined_slots.len() as u32,
        template_ref_count: tpl.template_refs.len() as u32,
        ..Default::default()
    };

    for el in &tpl.elements {
        metrics.total_directives += el.directives.len() as u32;
        if el.v_for.is_some() {
            metrics.v_for_count += 1;
        }
        if el.has_v_if {
            metrics.v_if_count += 1;
        }
    }

    for handler in &tpl.event_handlers {
        if handler.is_inline {
            metrics.inline_handler_count += 1;
        }
    }

    metrics.dynamic_binding_count = tpl.binding_occurrences.len() as u32;

    metrics
}

/// A11y score result (0-100).
#[derive(Debug, serde::Serialize)]
pub struct A11yScore {
    pub score: u32,
    pub errors: u32,
    pub warnings: u32,
    pub violations: Vec<A11yViolation>,
}

#[derive(Debug, serde::Serialize)]
pub struct A11yViolation {
    pub rule: String,
    pub severity: String,
    pub message: String,
}

/// Compute a11y score by running a11y-only lint rules.
pub fn compute_a11y_score(
    script: Option<&ScriptAnalysisSnapshot>,
    template: Option<&TemplateAnalysisSnapshot>,
    styles: &[StyleBlockAnalysis],
    source: Option<&str>,
) -> A11yScore {
    let config = LintConfig {
        preset: LintPreset::A11y,
        ..Default::default()
    };
    let linter = Linter::new(config);
    let diags = linter.lint_with_source(script, template, styles, source);
    let diag_vec = diags.into_diagnostics();

    let mut errors = 0u32;
    let mut warnings = 0u32;
    let mut violations = Vec::new();

    for d in &diag_vec {
        match d.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            _ => {}
        }
        violations.push(A11yViolation {
            rule: d.rule.clone(),
            severity: format!("{:?}", d.severity),
            message: d.message.clone(),
        });
    }

    // Score: start at 100, subtract 10 per error, 3 per warning, floor at 0
    let penalty = (errors * 10 + warnings * 3) as i32;
    let score = (100 - penalty).max(0) as u32;

    A11yScore {
        score,
        errors,
        warnings,
        violations,
    }
}

/// Component quality score with per-dimension breakdown.
#[derive(Debug, serde::Serialize)]
pub struct QualityScore {
    pub score: u32,
    pub a11y: DimensionScore,
    pub lint_health: DimensionScore,
    pub template_complexity: DimensionScore,
    pub api_surface: DimensionScore,
    pub css_health: DimensionScore,
    pub reactivity: DimensionScore,
}

#[derive(Debug, serde::Serialize)]
pub struct DimensionScore {
    pub score: u32,
    pub detail: String,
}

/// Compute composite quality score (0-100).
pub fn compute_quality_score(
    script: Option<&ScriptAnalysisSnapshot>,
    template: Option<&TemplateAnalysisSnapshot>,
    styles: &[StyleBlockAnalysis],
    source: Option<&str>,
) -> QualityScore {
    // 1. A11y dimension
    let a11y = compute_a11y_score(script, template, styles, source);
    let a11y_dim = DimensionScore {
        score: a11y.score,
        detail: format!("{} errors, {} warnings", a11y.errors, a11y.warnings),
    };

    // 2. Lint health dimension (all rules)
    let lint_config = LintConfig {
        preset: LintPreset::Recommended,
        ..Default::default()
    };
    let linter = Linter::new(lint_config);
    let lint_diags = linter.lint_with_source(script, template, styles, source);
    let lint_vec = lint_diags.into_diagnostics();
    let lint_errors = lint_vec
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count() as u32;
    let lint_warnings = lint_vec
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count() as u32;
    let lint_penalty = (lint_errors * 8 + lint_warnings * 2) as i32;
    let lint_dim = DimensionScore {
        score: (100 - lint_penalty).max(0) as u32,
        detail: format!("{} errors, {} warnings", lint_errors, lint_warnings),
    };

    // 3. Template complexity dimension
    let tpl_dim = if let Some(tpl) = template {
        let metrics = compute_template_metrics(tpl);
        let mut penalty = 0i32;
        if metrics.max_nesting_depth > 6 {
            penalty += (metrics.max_nesting_depth as i32 - 6) * 5;
        }
        if metrics.total_elements > 50 {
            penalty += ((metrics.total_elements - 50) / 10) as i32 * 3;
        }
        if metrics.inline_handler_count > 3 {
            penalty += (metrics.inline_handler_count as i32 - 3) * 4;
        }
        if metrics.v_for_count > 5 {
            penalty += (metrics.v_for_count as i32 - 5) * 3;
        }
        DimensionScore {
            score: (100 - penalty).max(0) as u32,
            detail: format!(
                "{} elements, depth {}, {} inline handlers",
                metrics.total_elements, metrics.max_nesting_depth, metrics.inline_handler_count
            ),
        }
    } else {
        DimensionScore {
            score: 100,
            detail: "No template".to_string(),
        }
    };

    // 4. API surface dimension (too many props = design smell)
    let api_dim = if let Some(s) = script {
        let mut penalty = 0i32;
        let prop_count = s
            .macros
            .iter()
            .filter(|m| m.kind == verter_analysis::types::AnalyzedMacroKind::DefineProps)
            .count() as i32;
        // Check if too many bindings (>30 is a smell)
        if s.bindings.len() > 30 {
            penalty += ((s.bindings.len() - 30) / 5) as i32 * 3;
        }
        if prop_count > 0 {
            // Penalize components with too many props (>15 is a design smell)
            for m in &s.macros {
                if m.kind == verter_analysis::types::AnalyzedMacroKind::DefineProps
                    && m.prop_fields.len() > 15
                {
                    penalty += (m.prop_fields.len() as i32 - 15) * 2;
                }
            }
        }
        DimensionScore {
            score: (100 - penalty).max(0) as u32,
            detail: format!("{} bindings", s.bindings.len()),
        }
    } else {
        DimensionScore {
            score: 100,
            detail: "No script".to_string(),
        }
    };

    // 5. CSS health dimension
    let css_dim = {
        let mut penalty = 0i32;
        let mut total_selectors = 0u32;
        let has_scoped = styles.iter().any(|s| s.scoped);
        let has_unscoped = styles.iter().any(|s| !s.scoped);

        for style_block in styles {
            if let Some(css) = &style_block.css {
                total_selectors += css.selectors.len() as u32;
            }
        }

        if has_unscoped && has_scoped {
            penalty += 5; // Mixing scoped and unscoped
        }
        if !has_scoped && !styles.is_empty() {
            penalty += 10; // No scoping at all
        }

        DimensionScore {
            score: (100 - penalty).max(0) as u32,
            detail: format!("{} selectors, scoped={}", total_selectors, has_scoped),
        }
    };

    // 6. Reactivity dimension
    let reactivity_dim = if let Some(s) = script {
        let mut penalty = 0i32;
        let watcher_count = s
            .vue_api_calls
            .iter()
            .filter(|c| {
                matches!(
                    c.api,
                    VueApiClassification::Watch
                        | VueApiClassification::WatchEffect
                        | VueApiClassification::WatchPostEffect
                        | VueApiClassification::WatchSyncEffect
                )
            })
            .count() as i32;

        if watcher_count > 5 {
            penalty += (watcher_count - 5) * 5;
        }
        if s.flags.contains(AnalysisFlags::ASYNC_SETUP) {
            penalty += 5; // Async setup adds complexity
        }

        DimensionScore {
            score: (100 - penalty).max(0) as u32,
            detail: format!("{} watchers", watcher_count),
        }
    } else {
        DimensionScore {
            score: 100,
            detail: "No script".to_string(),
        }
    };

    // Composite: weighted average
    let composite = (a11y_dim.score * 20
        + lint_dim.score * 25
        + tpl_dim.score * 20
        + api_dim.score * 10
        + css_dim.score * 10
        + reactivity_dim.score * 15)
        / 100;

    QualityScore {
        score: composite,
        a11y: a11y_dim,
        lint_health: lint_dim,
        template_complexity: tpl_dim,
        api_surface: api_dim,
        css_health: css_dim,
        reactivity: reactivity_dim,
    }
}
