//! Verter-specific inlay hints for Vue SFCs.
//!
//! Generates inlay hints from Verter's own analysis (no TSGO required):
//! - DOM query calls (`document.querySelector('.btn')`) → matched template element
//! - `useTemplateRef('foo')` calls → matched template `ref="foo"` element
//!
//! These hints are merged with TSGO type hints (when available) in `server.rs`.

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
use tower_lsp_server::ls_types::{InlayHint, InlayHintLabel};
use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateElement};
use verter_analysis::types::{
    DomQueryCallSite, DomQueryKind, VueApiCallSite, VueApiClassification,
};
use verter_analysis::{match_selector, MatchResult};
use verter_host::FileAnalysisSnapshot;

/// Generate Verter-specific inlay hints for a Vue SFC.
///
/// Returns hints for:
/// - DOM query calls showing matched template elements
/// - `useTemplateRef()` calls showing matched template refs
pub fn verter_inlay_hints(
    source: &str,
    blocks: &[SfcBlock],
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    let template = analysis.template.as_ref();

    // Find the script block offset to convert script-relative spans to SFC-absolute
    let script_offset = blocks
        .iter()
        .find(|b| b.tag_name == "script" && b.is_setup())
        .or_else(|| blocks.iter().find(|b| b.tag_name == "script"))
        .map(|b| b.open_tag_end)
        .unwrap_or(0);

    // DOM query inlay hints
    for call in &analysis.dom_query_calls {
        if let Some(hint) = dom_query_hint(call, script_offset, template, line_index) {
            hints.push(hint);
        }
    }

    // useTemplateRef inlay hints
    for call in &analysis.vue_api_calls {
        if call.api == VueApiClassification::UseTemplateRef {
            if let Some(hint) = template_ref_hint(call, script_offset, source, template, line_index)
            {
                hints.push(hint);
            }
        }
    }

    hints
}

/// Generate an inlay hint for a DOM query call.
///
/// Shows the matched template element(s) after the call expression:
/// - `querySelector('.btn')` → `<button class="btn"> (line 12)`
/// - `querySelectorAll('.item')` → `3 matches`
/// - `getElementById('app')` → `<div id="app"> (line 3)`
fn dom_query_hint(
    call: &DomQueryCallSite,
    script_offset: u32,
    template: Option<&TemplateAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<InlayHint> {
    let template = template?;
    let elements = &template.elements;
    if elements.is_empty() {
        return None;
    }

    let hint_text = if let Some(parsed) = &call.parsed {
        // Use structured matching
        let mut matches = Vec::new();
        let mut maybe_count = 0u32;
        for (i, _el) in elements.iter().enumerate() {
            match match_selector(parsed, i, elements) {
                MatchResult::Matches => matches.push(i),
                MatchResult::MaybeMatches => maybe_count += 1,
                MatchResult::NoMatch => {}
            }
        }

        if call.kind == DomQueryKind::QuerySelector || call.kind == DomQueryKind::GetElementById {
            // Single-element queries: show first match
            if let Some(&idx) = matches.first() {
                format_element_hint(&elements[idx], idx, line_index, script_offset)
            } else if maybe_count > 0 {
                format!(
                    "{maybe_count} possible match{}",
                    if maybe_count == 1 { "" } else { "es" }
                )
            } else {
                "no match".to_string()
            }
        } else {
            // Multi-element queries: show count
            if matches.is_empty() && maybe_count == 0 {
                "no matches".to_string()
            } else {
                let mut parts = Vec::new();
                if !matches.is_empty() {
                    parts.push(format!(
                        "{} match{}",
                        matches.len(),
                        if matches.len() == 1 { "" } else { "es" }
                    ));
                }
                if maybe_count > 0 {
                    parts.push(format!("{maybe_count} possible"));
                }
                parts.join(", ")
            }
        }
    } else {
        // No parsed selector — can't match
        return None;
    };

    // Position the hint at the end of the call expression (absolute SFC offset)
    let absolute_offset = script_offset + call.span.end;
    let position = line_index.offset_to_position(absolute_offset)?;

    Some(InlayHint {
        position,
        label: InlayHintLabel::String(format!(" // → {hint_text}")),
        kind: None,
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: None,
        data: None,
    })
}

/// Generate an inlay hint for a `useTemplateRef('foo')` call.
fn template_ref_hint(
    call: &VueApiCallSite,
    script_offset: u32,
    _source: &str,
    template: Option<&TemplateAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<InlayHint> {
    let ref_name = call.arg_value.as_deref()?;
    let template = template?;

    // Find matching template ref
    let matched = template
        .template_refs
        .iter()
        .find(|r| !r.is_dynamic && r.name == ref_name);

    let hint_text = if let Some(tref) = matched {
        format!("<{} ref=\"{}\">", tref.target_tag, ref_name)
    } else {
        format!("ref \"{ref_name}\" not found in template")
    };

    let absolute_offset = script_offset + call.span.end;
    let position = line_index.offset_to_position(absolute_offset)?;

    Some(InlayHint {
        position,
        label: InlayHintLabel::String(format!(" // → {hint_text}")),
        kind: None,
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: None,
        data: None,
    })
}

/// Format a single matched element for an inlay hint.
fn format_element_hint(
    el: &TemplateElement,
    _index: usize,
    line_index: &LineIndex,
    _script_offset: u32,
) -> String {
    // Build a short element representation
    let mut desc = format!("<{}", el.tag);

    // Add id if present
    if let Some(id) = el.static_id() {
        desc.push_str(&format!(" id=\"{id}\""));
    }

    // Add classes (static)
    let classes: Vec<&str> = el.static_classes().collect();
    if !classes.is_empty() {
        desc.push_str(&format!(" class=\"{}\"", classes.join(" ")));
    }

    // Show extracted dynamic class names
    if !el.dynamic_classes.is_empty() {
        desc.push_str(&format!(
            " :class=\"{{ {} }}\"",
            el.dynamic_classes
                .iter()
                .map(|c| format!("{c}?"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    } else {
        // Check for unparseable dynamic class
        let has_dynamic_class = el
            .attributes
            .iter()
            .any(|a| a.is_dynamic && a.name == "class");
        if has_dynamic_class {
            desc.push_str(" :class=\"...\"");
        }
    }

    desc.push('>');

    // Add line number
    if let Some(pos) = line_index.offset_to_position(el.span.start) {
        desc.push_str(&format!(" (line {})", pos.line + 1));
    }

    desc
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_analysis::style::parse_selector;
    use verter_analysis::template::{TemplateAttribute, TemplateRef};

    fn make_line_index(source: &str) -> LineIndex {
        LineIndex::new(
            source,
            tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
        )
    }

    fn make_element(tag: &str, classes: &[&str], id: Option<&str>) -> TemplateElement {
        let mut attrs = Vec::new();
        if !classes.is_empty() {
            attrs.push(TemplateAttribute {
                name: "class".to_string(),
                value: Some(classes.join(" ")),
                is_dynamic: false,
                span: verter_span::Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
        }
        if let Some(id_val) = id {
            attrs.push(TemplateAttribute {
                name: "id".to_string(),
                value: Some(id_val.to_string()),
                is_dynamic: false,
                span: verter_span::Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
        }
        TemplateElement {
            tag: tag.to_string(),
            attributes: attrs,
            content_end: 0,
            ..Default::default()
        }
    }

    fn make_element_with_dynamic_class(tag: &str, static_classes: &[&str]) -> TemplateElement {
        let mut attrs = Vec::new();
        if !static_classes.is_empty() {
            attrs.push(TemplateAttribute {
                name: "class".to_string(),
                value: Some(static_classes.join(" ")),
                is_dynamic: false,
                span: verter_span::Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
        }
        // Dynamic :class binding
        attrs.push(TemplateAttribute {
            name: "class".to_string(),
            value: Some("{ 'active': isActive }".to_string()),
            is_dynamic: true,
            span: verter_span::Span::new(0, 0),
            name_end: 0,
            value_span: None,
        });
        TemplateElement {
            tag: tag.to_string(),
            attributes: attrs,
            content_end: 0,
            ..Default::default()
        }
    }

    fn make_analysis(
        elements: Vec<TemplateElement>,
        template_refs: Vec<TemplateRef>,
        dom_query_calls: Vec<DomQueryCallSite>,
        vue_api_calls: Vec<VueApiCallSite>,
    ) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                template_refs,
                elements,
                ..Default::default()
            }),
            vue_api_calls,
            dom_query_calls,
            ..Default::default()
        }
    }

    fn script_block(content_start: u32) -> SfcBlock {
        SfcBlock {
            tag_name: "script".to_string(),
            open_tag_start: 0,
            open_tag_end: content_start,
            close_tag_start: content_start + 200,
            close_tag_end: content_start + 210,
            attrs_raw: " setup lang=\"ts\"".to_string(),
        }
    }

    #[test]
    fn dom_query_query_selector_matches_element() {
        // <button class="btn"> at line 1 of a simple source
        let source = "<script setup>\ndocument.querySelector('.btn')\n</script>\n<template>\n<button class=\"btn\">Click</button>\n</template>";
        let li = make_line_index(source);

        let mut el = make_element("button", &["btn"], None);
        // Set span_start to point inside template
        let template_start = source.find("<button").unwrap() as u32;
        el.span.start = template_start;

        let parsed = parse_selector(".btn").unwrap();
        let analysis = make_analysis(
            vec![el],
            vec![],
            vec![DomQueryCallSite {
                kind: DomQueryKind::QuerySelector,
                selector_text: ".btn".to_string(),
                parsed: Some(parsed),
                span: verter_span::Span::new(15, 44),
                arg_span: verter_span::Span::new(40, 44),
            }],
            vec![],
        );

        let blocks = vec![script_block(15)];
        let hints = verter_inlay_hints(source, &blocks, &analysis, &li);

        assert_eq!(hints.len(), 1);
        let label = match &hints[0].label {
            InlayHintLabel::String(s) => s.clone(),
            _ => panic!("expected string label"),
        };
        assert!(label.contains("<button"), "label={label}");
        assert!(label.contains("class=\"btn\""), "label={label}");
        assert!(label.contains("line"), "label={label}");
    }

    #[test]
    fn dom_query_no_match() {
        let source = "<script setup>\ndocument.querySelector('.missing')\n</script>\n<template>\n<div>Hello</div>\n</template>";
        let li = make_line_index(source);

        let parsed = parse_selector(".missing").unwrap();
        let analysis = make_analysis(
            vec![make_element("div", &[], None)],
            vec![],
            vec![DomQueryCallSite {
                kind: DomQueryKind::QuerySelector,
                selector_text: ".missing".to_string(),
                parsed: Some(parsed),
                span: verter_span::Span::new(15, 50),
                arg_span: verter_span::Span::new(40, 49),
            }],
            vec![],
        );

        let blocks = vec![script_block(15)];
        let hints = verter_inlay_hints(source, &blocks, &analysis, &li);

        assert_eq!(hints.len(), 1);
        let label = match &hints[0].label {
            InlayHintLabel::String(s) => s.clone(),
            _ => panic!("expected string label"),
        };
        assert!(label.contains("no match"), "label={label}");
    }

    #[test]
    fn dom_query_query_selector_all_shows_count() {
        let source = "<script setup>\ndocument.querySelectorAll('.item')\n</script>\n<template>\n<div class=\"item\">1</div>\n<div class=\"item\">2</div>\n<div class=\"item\">3</div>\n</template>";
        let li = make_line_index(source);

        let parsed = parse_selector(".item").unwrap();
        let analysis = make_analysis(
            vec![
                make_element("div", &["item"], None),
                make_element("div", &["item"], None),
                make_element("div", &["item"], None),
            ],
            vec![],
            vec![DomQueryCallSite {
                kind: DomQueryKind::QuerySelectorAll,
                selector_text: ".item".to_string(),
                parsed: Some(parsed),
                span: verter_span::Span::new(15, 49),
                arg_span: verter_span::Span::new(42, 48),
            }],
            vec![],
        );

        let blocks = vec![script_block(15)];
        let hints = verter_inlay_hints(source, &blocks, &analysis, &li);

        assert_eq!(hints.len(), 1);
        let label = match &hints[0].label {
            InlayHintLabel::String(s) => s.clone(),
            _ => panic!("expected string label"),
        };
        assert!(label.contains("3 matches"), "label={label}");
    }

    #[test]
    fn use_template_ref_hint() {
        let source = "<script setup>\nconst form = useTemplateRef('myForm')\n</script>\n<template>\n<form ref=\"myForm\">...</form>\n</template>";
        let li = make_line_index(source);

        let analysis = make_analysis(
            vec![],
            vec![TemplateRef {
                name: "myForm".to_string(),
                is_dynamic: false,
                target_tag: "form".to_string(),
            }],
            vec![],
            vec![VueApiCallSite {
                api: VueApiClassification::UseTemplateRef,
                span: verter_span::Span::new(14, 51),
                arg_value: Some("myForm".to_string()),
                is_async_callback: false,
                callback_params: vec![],
            }],
        );

        let blocks = vec![script_block(15)];
        let hints = verter_inlay_hints(source, &blocks, &analysis, &li);

        assert_eq!(hints.len(), 1);
        let label = match &hints[0].label {
            InlayHintLabel::String(s) => s.clone(),
            _ => panic!("expected string label"),
        };
        assert!(label.contains("<form"), "label={label}");
        assert!(label.contains("ref=\"myForm\""), "label={label}");
    }

    #[test]
    fn use_template_ref_not_found() {
        let source = "<script setup>\nconst el = useTemplateRef('missing')\n</script>\n<template>\n<div>Hello</div>\n</template>";
        let li = make_line_index(source);

        let analysis = make_analysis(
            vec![],
            vec![],
            vec![],
            vec![VueApiCallSite {
                api: VueApiClassification::UseTemplateRef,
                span: verter_span::Span::new(11, 50),
                arg_value: Some("missing".to_string()),
                is_async_callback: false,
                callback_params: vec![],
            }],
        );

        let blocks = vec![script_block(15)];
        let hints = verter_inlay_hints(source, &blocks, &analysis, &li);

        assert_eq!(hints.len(), 1);
        let label = match &hints[0].label {
            InlayHintLabel::String(s) => s.clone(),
            _ => panic!("expected string label"),
        };
        assert!(label.contains("not found"), "label={label}");
    }

    #[test]
    fn dynamic_class_shows_possible_match() {
        let source = "<script setup>\ndocument.querySelector('.active')\n</script>\n<template>\n<div :class=\"{ active: isActive }\">Hello</div>\n</template>";
        let li = make_line_index(source);

        let parsed = parse_selector(".active").unwrap();
        // Element with dynamic class → MaybeMatches
        let el = make_element_with_dynamic_class("div", &[]);

        let analysis = make_analysis(
            vec![el],
            vec![],
            vec![DomQueryCallSite {
                kind: DomQueryKind::QuerySelector,
                selector_text: ".active".to_string(),
                parsed: Some(parsed),
                span: verter_span::Span::new(15, 48),
                arg_span: verter_span::Span::new(40, 47),
            }],
            vec![],
        );

        let blocks = vec![script_block(15)];
        let hints = verter_inlay_hints(source, &blocks, &analysis, &li);

        assert_eq!(hints.len(), 1);
        let label = match &hints[0].label {
            InlayHintLabel::String(s) => s.clone(),
            _ => panic!("expected string label"),
        };
        assert!(label.contains("possible match"), "label={label}");
    }

    #[test]
    fn no_hints_without_template() {
        let source = "<script setup>\ndocument.querySelector('.btn')\n</script>";
        let li = make_line_index(source);

        let parsed = parse_selector(".btn").unwrap();
        // Analysis with no template
        let analysis = FileAnalysisSnapshot {
            dom_query_calls: vec![DomQueryCallSite {
                kind: DomQueryKind::QuerySelector,
                selector_text: ".btn".to_string(),
                parsed: Some(parsed),
                span: verter_span::Span::new(15, 44),
                arg_span: verter_span::Span::new(40, 44),
            }],
            ..Default::default()
        };

        let blocks = vec![script_block(15)];
        let hints = verter_inlay_hints(source, &blocks, &analysis, &li);

        assert!(hints.is_empty());
    }
}
