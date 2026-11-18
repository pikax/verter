use super::counters::VaporCounters;
use super::effect::{TextPart, VaporEffect};
use super::element::*;
use super::ops::{NameKind, Nav, Part, RunId, TextCreation};
use super::scope::VaporScope;
use super::state::VaporState;
use crate::assembly::emit_plan::MapMode;
use crate::ast::types::AstNodeKind;
use crate::template::code_gen::shared::helpers::VaporHelper;
use verter_span::Span;

fn open(source: &str) -> String {
    let sfc = format!("<template>{source}</template>");
    let ast = crate::framework_common::vue_bridge::parse_sfc(&sfc, None, None)
        .template_ast
        .unwrap();
    let node = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(element) = &ast.nodes[node.0].kind else {
        panic!("element fixture")
    };
    let mut html = String::new();
    build_open_tag(element, &sfc, &mut html);
    html
}
fn state() -> VaporState<'static> {
    VaporState::new(crate::emission::test_source("App.vue", ""), MapMode::NoMap)
}

#[test]
fn open_tag_simple_div() {
    assert_eq!(open("<div></div>"), "<div>");
}
#[test]
fn open_tag_with_static_attr() {
    assert_eq!(
        open("<div class=\"foo\" title=\"hello world\"/>"),
        "<div class=foo title=\"hello world\">"
    );
}
#[test]
fn open_tag_self_closing() {
    assert_eq!(open("<br/>"), "<br>");
}
#[test]
fn open_tag_skips_directives() {
    assert_eq!(open("<div :class=\"cls\" id=\"x\"/>"), "<div id=x>");
}
#[test]
fn close_tag_non_void() {
    let mut html = "<div>hello".to_owned();
    close_html_tag(&mut html, "div", false);
    assert_eq!(html, "<div>hello</div>");
}
#[test]
fn close_tag_void_noop() {
    let mut html = "<br>".to_owned();
    close_html_tag(&mut html, "br", true);
    assert_eq!(html, "<br>");
}
#[test]
fn finalize_text_parts_creates_effect() {
    let mut output = state();
    let mut scope = VaporScope {
        text_node_ref: Some(0),
        text_parts: vec![TextPart::Static("\"hello \""), TextPart::Dynamic(RunId(0))],
        ..VaporScope::default()
    };
    finalize_text_parts(&mut scope, true, &mut output);
    assert_eq!(scope.own_effects.len(), 1);
    assert!(scope.text_parts.is_empty());
    assert_eq!(
        output.effects[scope.own_effects[0].0 as usize],
        VaporEffect::Text {
            text_ref: 0,
            parts: vec![TextPart::Static("\"hello \""), TextPart::Dynamic(RunId(0))],
            generated: true
        }
    );
}
#[test]
fn finalize_text_parts_no_dynamic_noop() {
    let mut output = state();
    let mut scope = VaporScope::default();
    scope.text_parts.push(TextPart::Static("\"hello\""));
    finalize_text_parts(&mut scope, false, &mut output);
    assert!(scope.own_effects.is_empty());
    assert!(output.effects.is_empty());
}
#[test]
fn finalize_root_static_only() {
    let mut output = state();
    let scope = VaporScope {
        html: "<div>hello</div>".into(),
        ..VaporScope::default()
    };
    let root = output
        .close_scope(scope, "  ", false, false, Span::new(0, 0))
        .unwrap();
    assert_eq!(root.template_idx, Some(0));
    assert_eq!(root.node_ref, 0);
    assert!(root.is_static);
    assert_eq!(output.templates[0].html, "<div>hello");
    assert!(output.helpers().has(VaporHelper::Template));
}
#[test]
fn finalize_root_with_dynamic_text() {
    let mut output = state();
    let mut scope = VaporScope {
        html: "<div> </div>".into(),
        text_node_ref: Some(0),
        node_ref: Some(0),
        ..VaporScope::default()
    };
    scope.text_parts.push(TextPart::Static("\"msg\""));
    finalize_text_parts(&mut scope, true, &mut output);
    let root = output
        .close_scope(scope, "  ", false, false, Span::new(0, 0))
        .unwrap();
    assert!(!root.is_static);
    for helper in [
        VaporHelper::Txt,
        VaporHelper::SetText,
        VaporHelper::RenderEffect,
    ] {
        assert!(output.helpers().has(helper));
    }
    assert!(output
        .runs
        .get(root.run)
        .contains(&Part::Name(NameKind::Text, 0)));
}
#[test]
fn finalize_root_static_only_has_no_own_text_ref() {
    let mut output = state();
    let scope = VaporScope {
        html: "<p>hello".into(),
        ..VaporScope::default()
    };
    let root = output
        .close_scope(scope, "  ", false, false, Span::new(0, 0))
        .unwrap();
    assert!(!output.helpers().has(VaporHelper::Txt));
    assert!(!output.helpers().has(VaporHelper::SetText));
    assert!(!output
        .runs
        .get(root.run)
        .iter()
        .any(|part| matches!(part, Part::Name(NameKind::Text, _))));
}
#[test]
fn merge_static_child_does_not_copy_html_or_emit_nav() {
    let mut counters = VaporCounters::default();
    let mut parent = VaporScope {
        html: "<div>".into(),
        ..VaporScope::default()
    };
    let child = VaporScope {
        html: "<span>text</span>".into(),
        ..VaporScope::default()
    };
    let _ = merge_into_parent(child, &mut parent, &mut counters, 0, false);
    assert_eq!(parent.html, "<div>");
    assert!(parent.child_nav.is_empty());
}
#[test]
fn merge_dynamic_child_uses_dom_index_for_nav() {
    let mut counters = VaporCounters::default();
    let mut parent = VaporScope::default();
    let child = VaporScope {
        text_node_ref: Some(counters.next_node()),
        ..VaporScope::default()
    };
    let _ = merge_into_parent(child, &mut parent, &mut counters, 1, true);
    assert_eq!(
        parent.child_nav,
        [Nav::At {
            target: (NameKind::Path, 0),
            container: 1,
            index: 1,
            text: false,
        }]
    );
    assert_eq!(
        parent.child_text_creations,
        [TextCreation {
            text: 0,
            from: (NameKind::Path, 0)
        }]
    );
}
#[test]
fn merge_dynamic_child_bubbles_nav_text_and_effects() {
    let mut counters = VaporCounters::default();
    let mut output = state();
    let mut parent = VaporScope::default();
    let mut child = VaporScope {
        text_node_ref: Some(counters.next_node()),
        ..VaporScope::default()
    };
    child.own_effects.push(output.add_effect(VaporEffect::Text {
        text_ref: 0,
        parts: vec![TextPart::Static("\"hello\"")],
        generated: true,
    }));
    let _ = merge_into_parent(child, &mut parent, &mut counters, 0, true);
    assert_eq!(
        parent.child_nav,
        [Nav::At {
            target: (NameKind::Path, 0),
            container: 1,
            index: 0,
            text: false,
        }]
    );
    assert_eq!(
        parent.child_text_creations,
        [TextCreation {
            text: 0,
            from: (NameKind::Path, 0)
        }]
    );
    assert_eq!(parent.child_effects.len(), 1);
}
