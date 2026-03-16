//! Rule: no-invalid-html-nesting
//!
//! Warns when HTML elements are used in invalid parent-child relationships.
//! For example, `<option>` must be a child of `<select>` or `<optgroup>`,
//! `<tr>` must be inside `<table>`/`<thead>`/`<tbody>`/`<tfoot>`, etc.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateElement};

pub struct NoInvalidHtmlNesting;

/// Returns the set of valid parent tags for elements that have strict nesting requirements.
/// Returns `None` if the element has no nesting restrictions.
fn valid_parents(tag: &str) -> Option<&'static [&'static str]> {
    match tag {
        // <option> must be inside <select>, <optgroup>, or <datalist>
        "option" => Some(&["select", "optgroup", "datalist"]),
        // <optgroup> must be inside <select>
        "optgroup" => Some(&["select"]),
        // Table structure
        "tr" => Some(&["table", "thead", "tbody", "tfoot"]),
        "td" | "th" => Some(&["tr"]),
        "thead" | "tbody" | "tfoot" => Some(&["table"]),
        "caption" | "colgroup" | "col" => Some(&["table", "colgroup"]),
        // List structure
        "li" => Some(&["ul", "ol", "menu"]),
        "dt" | "dd" => Some(&["dl"]),
        // <summary> must be inside <details>
        "summary" => Some(&["details"]),
        // <source> and <track> must be inside <video>, <audio>, or <picture>
        "source" => Some(&["video", "audio", "picture"]),
        "track" => Some(&["video", "audio"]),
        // <figcaption> must be inside <figure>
        "figcaption" => Some(&["figure"]),
        // <legend> must be inside <fieldset>
        "legend" => Some(&["fieldset"]),
        _ => None,
    }
}

/// Walks up the parent chain to find the nearest non-template ancestor tag.
/// Vue's `<template>` wrappers (used for v-if/v-for) are transparent.
fn find_nearest_html_ancestor<'a>(
    el: &TemplateElement,
    elements: &'a [TemplateElement],
) -> Option<&'a str> {
    let mut current = el;
    loop {
        match current.parent_index {
            None => return None,
            Some(idx) => {
                let parent = &elements[idx as usize];
                // Skip Vue <template> wrappers (transparent structural elements)
                if parent.tag == "template" && !parent.is_component {
                    current = parent;
                    continue;
                }
                return Some(&parent.tag);
            }
        }
    }
}

impl LintRule for NoInvalidHtmlNesting {
    fn name(&self) -> &'static str {
        "no-invalid-html-nesting"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for el in &tpl.elements {
            // Skip component elements — only validate native HTML
            if el.is_component {
                continue;
            }

            let Some(required_parents) = valid_parents(&el.tag) else {
                continue;
            };

            let ancestor_tag = find_nearest_html_ancestor(el, &tpl.elements);

            let is_valid = match ancestor_tag {
                Some(parent) => required_parents.contains(&parent),
                // Root-level element with nesting requirement — invalid
                None => false,
            };

            if !is_valid {
                let parent_list = required_parents.join(", ");
                let actual = ancestor_tag.unwrap_or("(root)");
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "`<{}>` must be a child of `<{}>`, but found inside `<{}>` instead.",
                        el.tag, parent_list, actual
                    ),
                    el.span.start,
                    el.tag_span_end,
                    self.default_severity(),
                    DiagnosticSpanKind::ElementOpenTag,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateElement};
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoInvalidHtmlNesting, template)
    }

    fn make_el(tag: &str, parent_index: Option<u32>, parent_tag: Option<&str>) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: false,
            parent_index,
            parent_tag: parent_tag.map(|s| s.to_string()),
            span: Span::new(0, 20),
            tag_span_end: 10,
            ..Default::default()
        }
    }

    #[test]
    fn option_inside_select_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("select", None, None),
                make_el("option", Some(0), Some("select")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "option inside select is valid");
    }

    #[test]
    fn option_inside_optgroup_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("select", None, None),
                make_el("optgroup", Some(0), Some("select")),
                make_el("option", Some(1), Some("optgroup")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "option inside optgroup is valid");
    }

    #[test]
    fn option_inside_div_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("div", None, None),
                make_el("option", Some(0), Some("div")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<option>"));
        assert!(diags[0].message.contains("<div>"));
        assert!(diags[0].rule == "no-invalid-html-nesting");
    }

    #[test]
    fn tr_inside_tbody_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("table", None, None),
                make_el("tbody", Some(0), Some("table")),
                make_el("tr", Some(1), Some("tbody")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "tr inside tbody is valid");
    }

    #[test]
    fn td_outside_tr_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("table", None, None),
                make_el("td", Some(0), Some("table")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<td>"));
    }

    #[test]
    fn li_inside_ul_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("ul", None, None),
                make_el("li", Some(0), Some("ul")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "li inside ul is valid");
    }

    #[test]
    fn li_inside_div_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("div", None, None),
                make_el("li", Some(0), Some("div")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<li>"));
    }

    #[test]
    fn component_element_skipped() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![{
                let mut el = make_el("MyOption", Some(0), Some("div"));
                el.is_component = true;
                el
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "component elements should be skipped");
    }

    #[test]
    fn template_wrapper_transparent() {
        // <ul> → <template v-for> → <li> should be valid
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("ul", None, None),
                make_el("template", Some(0), Some("ul")),
                make_el("li", Some(1), Some("template")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "<li> inside <template> inside <ul> should pass (template is transparent)"
        );
    }

    #[test]
    fn summary_inside_details_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("details", None, None),
                make_el("summary", Some(0), Some("details")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "summary inside details is valid");
    }

    #[test]
    fn legend_outside_fieldset_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("div", None, None),
                make_el("legend", Some(0), Some("div")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<legend>"));
    }

    #[test]
    fn no_false_positives_on_regular_elements() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_el("div", None, None),
                make_el("span", Some(0), Some("div")),
                make_el("p", Some(0), Some("div")),
                make_el("a", Some(0), Some("div")),
            ],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "regular elements should not trigger nesting validation"
        );
    }
}
