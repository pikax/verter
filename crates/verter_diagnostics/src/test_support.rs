use crate::config::LintConfig;
use crate::context::LintContext;
use crate::cross_file::CrossFileSnapshot;
use crate::diagnostic::LintDiagnostic;
use crate::rules::{FileContext, LintRule};
use crate::visitor::LintVisitor;
use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateElement};
use verter_analysis::types::ScriptAnalysisSnapshot;
use verter_analysis::StyleBlockAnalysis;

fn run_rule_with<R, F>(rule: R, visit: F) -> Vec<LintDiagnostic>
where
    R: LintRule + 'static,
    F: FnOnce(&LintVisitor, &mut LintContext),
{
    let rules: Vec<Box<dyn LintRule>> = vec![Box::new(rule)];
    let visitor = LintVisitor::new(&rules);
    let config = LintConfig::default();
    let mut ctx = LintContext::new(&config);
    visit(&visitor, &mut ctx);
    ctx.into_diagnostics()
}

pub(crate) fn run_template_rule<R>(
    rule: R,
    template: &TemplateAnalysisSnapshot,
) -> Vec<LintDiagnostic>
where
    R: LintRule + 'static,
{
    run_rule_with(rule, |visitor, ctx| visitor.visit_template(template, ctx))
}

pub(crate) fn run_template_elements_rule<R>(
    rule: R,
    elements: Vec<TemplateElement>,
) -> Vec<LintDiagnostic>
where
    R: LintRule + 'static,
{
    run_template_rule(
        rule,
        &TemplateAnalysisSnapshot {
            elements,
            ..Default::default()
        },
    )
}

pub(crate) fn run_script_rule<R>(rule: R, script: &ScriptAnalysisSnapshot) -> Vec<LintDiagnostic>
where
    R: LintRule + 'static,
{
    run_rule_with(rule, |visitor, ctx| visitor.visit_script(script, ctx))
}

#[allow(dead_code)]
pub(crate) fn run_style_rule<R>(rule: R, styles: &[StyleBlockAnalysis]) -> Vec<LintDiagnostic>
where
    R: LintRule + 'static,
{
    run_rule_with(rule, |visitor, ctx| visitor.visit_styles(styles, ctx))
}

pub(crate) fn run_file_rule<R>(rule: R, file: &FileContext<'_>) -> Vec<LintDiagnostic>
where
    R: LintRule + 'static,
{
    run_rule_with(rule, |visitor, ctx| visitor.visit_file(file, ctx))
}

pub(crate) fn run_cross_file_rule<R>(rule: R, snapshot: &CrossFileSnapshot) -> Vec<LintDiagnostic>
where
    R: LintRule + 'static,
{
    run_rule_with(rule, |visitor, ctx| visitor.visit_cross_file(snapshot, ctx))
}
