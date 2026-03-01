//! Rule: `undefined-css-class`
//!
//! Detects CSS class names used in the template but not defined in any `<style>` block.

use std::collections::HashSet;

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct UndefinedCssClass;

impl LintRule for UndefinedCssClass {
    fn name(&self) -> &'static str {
        "undefined-css-class"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Css
    }

    fn default_severity(&self) -> Severity {
        Severity::Hint
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let template = match file.template {
            Some(t) => t,
            None => return,
        };

        let source = match file.source {
            Some(s) => s,
            None => return,
        };

        // Collect all defined class names from all style blocks
        let has_styles = file
            .styles
            .iter()
            .any(|s| s.css.as_ref().is_some_and(|c| !c.classes.is_empty()));

        if !has_styles {
            return; // No CSS classes defined → no diagnostics to give
        }

        let mut defined_classes: HashSet<&str> = HashSet::default();
        for style in file.styles {
            if let Some(css) = &style.css {
                for class in &css.classes {
                    defined_classes.insert(&class.name);
                }
            }
        }

        for element in &template.elements {
            // Check static class attributes
            for attr in &element.attributes {
                if attr.name != "class" || attr.is_dynamic {
                    continue;
                }
                let value = match attr.value.as_ref() {
                    Some(v) => v,
                    None => continue,
                };

                // Safely extract attribute text from source
                let attr_start = attr.span.start as usize;
                let attr_end = attr.span.end as usize;
                if attr_end > source.len() {
                    continue;
                }
                let attr_text = &source[attr_start..attr_end];
                let val_offset = match attr_text.find(value.as_str()) {
                    Some(v) => v,
                    None => continue,
                };
                let val_start = attr_start + val_offset;

                let mut pos = 0;
                for class_name in value.split_whitespace() {
                    if !defined_classes.contains(class_name) {
                        if let Some(name_start) = value[pos..].find(class_name) {
                            let abs_start = (val_start + pos + name_start) as u32;
                            let abs_end = abs_start + class_name.len() as u32;

                            ctx.report_with_tags(
                                self.name(),
                                self.category().as_str(),
                                format!(
                                    "Class `{class_name}` is used in template but not defined in any `<style>` block",
                                ),
                                abs_start,
                                abs_end,
                                self.default_severity(),
                                vec![DiagnosticTag::Unnecessary],
                                DiagnosticSpanKind::CssClassName,
                            );
                        }
                    }
                    if let Some(found) = value[pos..].find(class_name) {
                        pos += found + class_name.len();
                    }
                }
            }

            // Check dynamic class names
            for dcn in &element.dynamic_classes {
                if !defined_classes.contains(dcn.as_str()) {
                    ctx.report_with_tags(
                        self.name(),
                        self.category().as_str(),
                        format!(
                            "Class `{dcn}` is used in dynamic `:class` but not defined in any `<style>` block",
                        ),
                        element.span.start,
                        element.span.end,
                        self.default_severity(),
                        vec![DiagnosticTag::Unnecessary],
                        DiagnosticSpanKind::FullElement,
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

    fn build_style(css_content: &str, scoped: bool, content_offset: u32) -> StyleBlockAnalysis {
        let mut analysis = style::build_css_style_analysis(
            css_content,
            style::VueStyleInput {
                v_binds: vec![],
                special_pseudos: vec![],
            },
            scoped,
            false,
            None,
            content_offset,
        );
        if let Some(ref mut css) = analysis.css {
            for sel in &mut css.selectors {
                sel.span.start += content_offset;
                sel.span.end += content_offset;
            }
            for cls in &mut css.classes {
                cls.span.start += content_offset;
                cls.span.end += content_offset;
            }
            for id in &mut css.ids {
                id.span.start += content_offset;
                id.span.end += content_offset;
            }
        }
        analysis
    }

    fn run_rule(file: &FileContext<'_>) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(UndefinedCssClass)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_file(file, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn detects_undefined_class() {
        let source = r#"<template><div class="foo"></div></template>
<style scoped>
.bar { color: red; }
</style>"#;
        let class_attr = r#"class="foo""#;
        let class_start = source.find(class_attr).unwrap() as u32;
        let class_end = class_start + class_attr.len() as u32;

        let style_start = source.find(".bar").unwrap() as u32 - 1; // content offset
        let css_content = ".bar { color: red; }";
        let style = build_style(css_content, true, style_start);

        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".into(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".into(),
                    value: Some("foo".into()),
                    is_dynamic: false,
                    span: Span::new(class_start, class_end),
                }],
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

                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: Span::new(0, 10),
                tag_span_end: 10,
            }],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: Some(source),
        };

        let diags = run_rule(&file);
        assert_eq!(diags.len(), 1, "should detect undefined class");
        assert!(diags[0].message.contains("foo"));
        assert_eq!(diags[0].severity, Severity::Hint);
        assert_eq!(diags[0].tags, vec![DiagnosticTag::Unnecessary]);
        assert_eq!(diags[0].rule, "undefined-css-class");
    }

    #[test]
    fn no_diagnostic_when_defined() {
        let source = r#"<template><div class="foo"></div></template>
<style scoped>
.foo { color: red; }
</style>"#;
        let class_attr = r#"class="foo""#;
        let class_start = source.find(class_attr).unwrap() as u32;
        let class_end = class_start + class_attr.len() as u32;

        let style_start = source.find(".foo").unwrap() as u32 - 1;
        let css_content = ".foo { color: red; }";
        let style = build_style(css_content, true, style_start);

        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".into(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".into(),
                    value: Some("foo".into()),
                    is_dynamic: false,
                    span: Span::new(class_start, class_end),
                }],
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

                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: Span::new(0, 10),
                tag_span_end: 10,
            }],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: Some(source),
        };

        let diags = run_rule(&file);
        assert!(
            diags.is_empty(),
            "should have no undefined class diagnostic"
        );
    }

    #[test]
    fn no_diagnostic_without_style() {
        let source = r#"<template><div class="foo"></div></template>"#;

        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".into(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".into(),
                    value: Some("foo".into()),
                    is_dynamic: false,
                    span: Span::new(15, 26),
                }],
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

                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: Span::new(0, 10),
                tag_span_end: 10,
            }],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[],
            source: Some(source),
        };

        let diags = run_rule(&file);
        assert!(diags.is_empty(), "should not diagnose without style block");
    }

    #[test]
    fn no_diagnostic_without_source() {
        let css_content = ".bar { color: red; }";
        let style = build_style(css_content, true, 100);

        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".into(),
                is_component: false,
                is_self_closing: false,
                namespace: ElementNamespace::Html,
                attributes: vec![TemplateAttribute {
                    name: "class".into(),
                    value: Some("foo".into()),
                    is_dynamic: false,
                    span: Span::new(15, 26),
                }],
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

                has_element_children: false,
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                span: Span::new(0, 10),
                tag_span_end: 10,
            }],
            ..Default::default()
        };

        let file = FileContext {
            template: Some(&template),
            script: None,
            styles: &[style],
            source: None, // No source available
        };

        let diags = run_rule(&file);
        assert!(diags.is_empty(), "should not diagnose without source");
    }
}
