//! Rule: media-has-caption
//!
//! `<video>` and `<audio>` elements must have a `<track kind="captions">` child
//! to make media accessible to deaf or hard-of-hearing users.
//!
//! This rule checks the element tree: if a media element has no element children
//! at all, or if none of its direct children are `<track>` elements, it reports.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct MediaHasCaption;

impl LintRule for MediaHasCaption {
    fn name(&self) -> &'static str {
        "media-has-caption"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for (index, el) in tpl.elements.iter().enumerate() {
            if el.tag != "video" && el.tag != "audio" {
                continue;
            }
            // If the element has no children at all, it definitely has no track.
            if !el.has_element_children {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "'<{}>' must have a '<track kind=\"captions\">' child element \
                         for accessibility.",
                        el.tag
                    ),
                    el.span.start,
                    el.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::ElementOpenTag,
                );
                continue;
            }
            // Check if any direct child is a <track> element.
            let has_track = tpl
                .elements
                .iter()
                .any(|child| child.parent_index == Some(index as u32) && child.tag == "track");
            if !has_track {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "'<{}>' must have a '<track kind=\"captions\">' child element \
                         for accessibility.",
                        el.tag
                    ),
                    el.span.start,
                    el.span.end,
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

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(MediaHasCaption, template)
    }

    fn make_video() -> TemplateElement {
        TemplateElement {
            tag: "video".to_string(),
            has_element_children: false,
            span: Span::new(0, 30),
            content_end: 0,
            ..Default::default()
        }
    }

    fn make_video_with_children() -> TemplateElement {
        TemplateElement {
            tag: "video".to_string(),
            has_element_children: true,
            span: Span::new(0, 50),
            content_end: 0,
            ..Default::default()
        }
    }

    fn make_track(parent_index: u32) -> TemplateElement {
        TemplateElement {
            tag: "track".to_string(),
            parent_index: Some(parent_index),
            attributes: vec![TemplateAttribute {
                name: "kind".to_string(),
                value: Some("captions".to_string()),
                is_dynamic: false,
                span: Span::new(40, 55),
                name_end: 0,
                value_span: None,
            }],
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn video_without_children_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_video()],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "video without track should trigger");
        assert!(diags.iter().any(|d| d.rule == "media-has-caption"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-autofocus"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn audio_without_children_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "audio".to_string(),
                has_element_children: false,
                span: Span::new(0, 30),
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "audio without track should trigger");
    }

    #[test]
    fn video_with_track_child_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_video_with_children(), make_track(0)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "video with track child should pass");
    }

    #[test]
    fn video_with_children_but_no_track_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                make_video_with_children(),
                TemplateElement {
                    tag: "source".to_string(),
                    parent_index: Some(0u32),
                    content_end: 0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "video with source but no track should trigger"
        );
    }

    #[test]
    fn div_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "div should not trigger");
    }
}
