//! Rule: `unused-css-selector`
//!
//! Detects scoped CSS selectors that don't match any template element.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};
use verter_analysis::{match_selector, MatchResult};

pub struct UnusedCssSelector;

impl LintRule for UnusedCssSelector {
    fn name(&self) -> &'static str {
        "unused-css-selector"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Css
    }

    fn default_severity(&self) -> Severity {
        Severity::Hint
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let template = match file.template {
            Some(t) if !t.elements.is_empty() => t,
            _ => return,
        };

        for style in file.styles {
            if !style.scoped {
                continue;
            }

            let css = match &style.css {
                Some(css) => css,
                None => continue,
            };

            for selector in &css.selectors {
                let structure = match &selector.structure {
                    Some(s) => s,
                    None => continue,
                };

                // Skip :deep() and :global() selectors — they target external elements
                if selector.text.contains(":deep(") || selector.text.contains(":global(") {
                    continue;
                }

                // Check if any template element matches
                let mut any_match = false;
                for (idx, _) in template.elements.iter().enumerate() {
                    match match_selector(structure, idx, &template.elements) {
                        MatchResult::Matches | MatchResult::MaybeMatches => {
                            any_match = true;
                            break;
                        }
                        MatchResult::NoMatch => {}
                    }
                }

                if !any_match {
                    let abs_start = selector.span.start + style.content_offset;
                    let abs_end = selector.span.end + style.content_offset;

                    ctx.report_with_tags(
                        self.name(),
                        self.category().as_str(),
                        format!(
                            "Unused CSS selector `{}` — no matching template elements",
                            selector.text,
                        ),
                        abs_start,
                        abs_end,
                        self.default_severity(),
                        vec![DiagnosticTag::Unnecessary],
                        DiagnosticSpanKind::CssSelector,
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
    use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateElement};
    use verter_analysis::{style, ElementNamespace, StyleBlockAnalysis, TemplateAttribute};
    use verter_span::Span;

    fn make_element(tag: &str, classes: &[&str], id: Option<&str>) -> TemplateElement {
        let mut attrs = Vec::new();
        if !classes.is_empty() {
            attrs.push(TemplateAttribute {
                name: "class".into(),
                value: Some(classes.join(" ")),
                is_dynamic: false,
                span: Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
        }
        if let Some(id_val) = id {
            attrs.push(TemplateAttribute {
                name: "id".into(),
                value: Some(id_val.into()),
                is_dynamic: false,
                span: Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
        }
        TemplateElement {
            tag: tag.into(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: attrs,
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,

            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 0),
            tag_span_end: 0,
            content_end: 0,
            ..Default::default()
        }
    }

    fn build_style(css_content: &str, scoped: bool, content_offset: u32) -> StyleBlockAnalysis {
        // Note: CSS spans from the scanner are content-relative.
        // The rule adds style.content_offset when reporting diagnostics.
        style::build_css_style_analysis(
            css_content,
            style::VueStyleInput {
                v_binds: vec![],
                special_pseudos: vec![],
            },
            scoped,
            false,
            None,
            content_offset,
        )
    }

    fn run_rule(file: &FileContext<'_>) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(UnusedCssSelector)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_file(file, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn detects_unused_selector() {
        let css = ".used { color: red; }\n.unused { color: blue; }";
        let style = build_style(css, true, 100);

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("div", &["used"], None)],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: None,
        };

        let diags = run_rule(&file);
        assert_eq!(diags.len(), 1, "should detect 1 unused selector");
        assert!(diags[0].message.contains(".unused"));
        assert_eq!(diags[0].severity, Severity::Hint);
        assert_eq!(diags[0].tags, vec![DiagnosticTag::Unnecessary]);
        assert_eq!(diags[0].rule, "unused-css-selector");
    }

    #[test]
    fn no_diagnostics_when_all_match() {
        let css = ".foo { color: red; }";
        let style = build_style(css, true, 100);

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("div", &["foo"], None)],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: None,
        };

        let diags = run_rule(&file);
        assert!(diags.is_empty(), "should have no unused selectors");
    }

    #[test]
    fn skips_non_scoped_blocks() {
        let css = ".unused { color: red; }";
        let style = build_style(css, false, 100);

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("div", &[], None)],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: None,
        };

        let diags = run_rule(&file);
        assert!(diags.is_empty(), "should skip non-scoped blocks");
    }

    #[test]
    fn skips_deep_selectors() {
        let css = ".foo :deep(.bar) { color: red; }";
        let style = build_style(css, true, 100);

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("div", &["foo"], None)],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: None,
        };

        let diags = run_rule(&file);
        assert!(diags.is_empty(), "should skip :deep() selectors");
    }

    #[test]
    fn skips_when_no_template() {
        let css = ".unused { color: red; }";
        let style = build_style(css, true, 100);

        let file = FileContext {
            template: None,
            script: None,
            styles: &[style],
            source: None,
        };

        let diags = run_rule(&file);
        assert!(diags.is_empty(), "should skip when no template");
    }
}
