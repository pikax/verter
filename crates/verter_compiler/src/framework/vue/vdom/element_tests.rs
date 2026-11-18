use super::super::children::text_separator;
use super::*;
use crate::ast::types::*;
use crate::framework::vue::semantic::{VueBindingTable, VueBindingView, NO_COMPONENT_TAG};
use crate::types::{NodeProp, NodeTag};
use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

fn make_tag(start: u32, end: u32, name_end: u32) -> NodeTag {
    NodeTag {
        start,
        end,
        name_end,
    }
}

fn make_element(
    tag_open: NodeTag,
    tag_close: Option<NodeTag>,
    content: Option<ElementContent>,
) -> ElementNode {
    let is_self_closing = tag_close.is_none();
    let children_flag = ChildrenFlag::empty();
    ElementNode {
        tag_open,
        tag_close,
        tag_type: TagType::Element,
        is_self_closing,
        props: Vec::new(),
        content,
        v_condition: None,
        v_for: None,
        v_slot: None,
        v_once: None,
        v_ref: None,
        prop_flag: PropFlag::empty(),
        children_flag,
        children_mode: children_flag.mode(),
        is_fully_static: false,
    }
}

fn make_resolver() -> VueBindingView<'static> {
    VueBindingView::owning(VueBindingTable::from_reported([]), false, None)
}

fn make_options() -> VdomOptions {
    VdomOptions {
        is_inline: false,
        is_production: false,
        comments: false,
        force_js: false,
        has_script: false,
        hoist_static: false,
        cache_handlers: true,
        element_vnodes: true,
        array_separator: ", ",
        trim_interpolations: false,
        branches_on_lines: false,
        scope_id: String::new(),
    }
}

fn make_ast() -> TemplateAst {
    use crate::parser::types::RootNodeTemplate;
    use crate::types::NodeTag;
    TemplateAst {
        nodes: Vec::new(),
        root: RootNodeTemplate {
            tag_open: NodeTag {
                start: 0,
                end: 0,
                name_end: 0,
            },
            tag_close: None,
            lang: None,
            attributes: Vec::new(),
            content: None,
        },
    }
}

/// Apply the recorded edits to the source and return the result string.
fn apply_output<'a>(source: &str, out: VdomOut<'a>, alloc: &'a Allocator) -> String {
    let mut ct = crate::code_transform::CodeTransform::new(source, alloc);
    out.edits.apply(&mut ct).expect("recorded edits apply");
    ct.build_string()
}

#[test]
fn self_closing_no_props() {
    let alloc = Allocator::default();
    let mut out = VdomOut::new(crate::code_transform::EditRecorder::new(&alloc));
    let source = "<br />";
    let element = make_element(make_tag(0, 6, 3), None, None);
    let options = make_options();
    let mut children = Vec::new();

    let mut buf = String::new();
    let record = process_element_leave(
        &element,
        None,
        None,
        &mut children,
        source,
        &mut out,
        &options,
        &make_resolver(),
        &mut buf,
        None,
        &make_ast(),
        false,                 // not block root (inner element test)
        false,                 // not force-open-block
        None,                  // no injected branch key
        None,                  // no hoisting in unit tests
        None,                  // no cache in unit tests
        None,                  // no resolved components in unit tests
        &NO_COMPONENT_TAG,     // no component tag in unit tests
        false,                 // not slot cached
        None,                  // no pre-reserved hoist in unit tests
        None,                  // no reserved handler caches in unit tests
        None,                  // no array-group cache reservation in unit tests
        &FxHashMap::default(), // no text-run cache reservations in unit tests
    );

    assert_eq!(record.kind, ChildKind::Element);
    assert_eq!(record.start, 0);
    assert_eq!(record.end, 6);

    let result = apply_output(source, out, &alloc);
    assert_eq!(result, "_createElementVNode(\"br\")");
}

// ==================== Empty element ====================

#[test]
fn empty_element_no_props() {
    let alloc = Allocator::default();
    let mut out = VdomOut::new(crate::code_transform::EditRecorder::new(&alloc));
    let source = "<div></div>";
    let element = make_element(
        make_tag(0, 5, 4),
        Some(make_tag(5, 11, 10)),
        Some(ElementContent {
            start: 5,
            end: 5,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    let options = make_options();
    let mut children = Vec::new();

    let mut buf = String::new();
    let record = process_element_leave(
        &element,
        None,
        None,
        &mut children,
        source,
        &mut out,
        &options,
        &make_resolver(),
        &mut buf,
        None,
        &make_ast(),
        false,                 // not block root (inner element test)
        false,                 // not force-open-block
        None,                  // no injected branch key
        None,                  // no hoisting in unit tests
        None,                  // no cache in unit tests
        None,                  // no resolved components in unit tests
        &NO_COMPONENT_TAG,     // no component tag in unit tests
        false,                 // not slot cached
        None,                  // no pre-reserved hoist in unit tests
        None,                  // no reserved handler caches in unit tests
        None,                  // no array-group cache reservation in unit tests
        &FxHashMap::default(), // no text-run cache reservations in unit tests
    );

    assert_eq!(record.kind, ChildKind::Element);
    let result = apply_output(source, out, &alloc);
    assert_eq!(result, "_createElementVNode(\"div\")");
}

// ==================== Text-only static children ====================

#[test]
fn element_with_static_text() {
    let alloc = Allocator::default();
    let mut out = VdomOut::new(crate::code_transform::EditRecorder::new(&alloc));
    let source = "<div>hello</div>";
    let mut element = make_element(
        make_tag(0, 5, 4),
        Some(make_tag(10, 16, 15)),
        Some(ElementContent {
            start: 5,
            end: 10,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    element.children_mode = ChildrenMode::TextOnlyStatic;
    let options = make_options();
    let mut children = vec![ChildRecord {
        start: 5,
        end: 10,
        kind: ChildKind::Text,
        condition: None,
        condition_prefix: None,
    }];

    let mut buf = String::new();
    process_element_leave(
        &element,
        None,
        None,
        &mut children,
        source,
        &mut out,
        &options,
        &make_resolver(),
        &mut buf,
        None,
        &make_ast(),
        false,                 // not block root (inner element test)
        false,                 // not force-open-block
        None,                  // no injected branch key
        None,                  // no hoisting in unit tests
        None,                  // no cache in unit tests
        None,                  // no resolved components in unit tests
        &NO_COMPONENT_TAG,     // no component tag in unit tests
        false,                 // not slot cached
        None,                  // no pre-reserved hoist in unit tests
        None,                  // no reserved handler caches in unit tests
        None,                  // no array-group cache reservation in unit tests
        &FxHashMap::default(), // no text-run cache reservations in unit tests
    );

    let result = apply_output(source, out, &alloc);
    assert_eq!(result, "_createElementVNode(\"div\", null, \"hello\")");
}

// ==================== Static props ====================

#[test]
fn element_with_static_class() {
    let alloc = Allocator::default();
    let mut out = VdomOut::new(crate::code_transform::EditRecorder::new(&alloc));
    // <div class="foo"></div>
    let source = "<div class=\"foo\"></div>";
    let mut element = make_element(
        make_tag(0, 17, 4),
        Some(make_tag(17, 23, 22)),
        Some(ElementContent {
            start: 17,
            end: 17,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    element.props.push(NodeProp {
        start: 5,
        name_end: 10,
        is_directive: false,
        arg_start: None,
        arg_end: None,
        is_dynamic: None,
        value_start: Some(12),
        value_end: Some(15),
        modifiers: SmallVec::new(),
    });
    element.prop_flag = PropFlag::empty().add(PropFlags::HasStaticClass);
    let options = make_options();
    let mut children = Vec::new();

    let mut buf = String::new();
    process_element_leave(
        &element,
        None,
        None,
        &mut children,
        source,
        &mut out,
        &options,
        &make_resolver(),
        &mut buf,
        None,
        &make_ast(),
        false,                 // not block root (inner element test)
        false,                 // not force-open-block
        None,                  // no injected branch key
        None,                  // no hoisting in unit tests
        None,                  // no cache in unit tests
        None,                  // no resolved components in unit tests
        &NO_COMPONENT_TAG,     // no component tag in unit tests
        false,                 // not slot cached
        None,                  // no pre-reserved hoist in unit tests
        None,                  // no reserved handler caches in unit tests
        None,                  // no array-group cache reservation in unit tests
        &FxHashMap::default(), // no text-run cache reservations in unit tests
    );

    let result = apply_output(source, out, &alloc);
    assert_eq!(result, "_createElementVNode(\"div\", { class: \"foo\" })");
}

#[test]
fn element_with_multiple_static_props() {
    let alloc = Allocator::default();
    let mut out = VdomOut::new(crate::code_transform::EditRecorder::new(&alloc));
    // <div class="foo" id="bar"></div>
    let source = "<div class=\"foo\" id=\"bar\"></div>";
    let mut element = make_element(
        make_tag(0, 26, 4),
        Some(make_tag(26, 32, 31)),
        Some(ElementContent {
            start: 26,
            end: 26,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    element.props.push(NodeProp {
        start: 5,
        name_end: 10,
        is_directive: false,
        arg_start: None,
        arg_end: None,
        is_dynamic: None,
        value_start: Some(12),
        value_end: Some(15),
        modifiers: SmallVec::new(),
    });
    element.props.push(NodeProp {
        start: 17,
        name_end: 19,
        is_directive: false,
        arg_start: None,
        arg_end: None,
        is_dynamic: None,
        value_start: Some(21),
        value_end: Some(24),
        modifiers: SmallVec::new(),
    });
    let options = make_options();
    let mut children = Vec::new();

    let mut buf = String::new();
    process_element_leave(
        &element,
        None,
        None,
        &mut children,
        source,
        &mut out,
        &options,
        &make_resolver(),
        &mut buf,
        None,
        &make_ast(),
        false,                 // not block root (inner element test)
        false,                 // not force-open-block
        None,                  // no injected branch key
        None,                  // no hoisting in unit tests
        None,                  // no cache in unit tests
        None,                  // no resolved components in unit tests
        &NO_COMPONENT_TAG,     // no component tag in unit tests
        false,                 // not slot cached
        None,                  // no pre-reserved hoist in unit tests
        None,                  // no reserved handler caches in unit tests
        None,                  // no array-group cache reservation in unit tests
        &FxHashMap::default(), // no text-run cache reservations in unit tests
    );

    let result = apply_output(source, out, &alloc);
    assert_eq!(
        result,
        "_createElementVNode(\"div\", { class: \"foo\", id: \"bar\" })"
    );
}

#[test]
fn element_with_props_and_text_child() {
    let alloc = Allocator::default();
    let mut out = VdomOut::new(crate::code_transform::EditRecorder::new(&alloc));
    // <div class="foo">hello</div>
    let source = "<div class=\"foo\">hello</div>";
    let mut element = make_element(
        make_tag(0, 17, 4),
        Some(make_tag(22, 28, 27)),
        Some(ElementContent {
            start: 17,
            end: 22,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    element.props.push(NodeProp {
        start: 5,
        name_end: 10,
        is_directive: false,
        arg_start: None,
        arg_end: None,
        is_dynamic: None,
        value_start: Some(12),
        value_end: Some(15),
        modifiers: SmallVec::new(),
    });
    element.prop_flag = PropFlag::empty().add(PropFlags::HasStaticClass);
    element.children_mode = ChildrenMode::TextOnlyStatic;
    let options = make_options();
    let mut children = vec![ChildRecord {
        start: 17,
        end: 22,
        kind: ChildKind::Text,
        condition: None,
        condition_prefix: None,
    }];

    let mut buf = String::new();
    process_element_leave(
        &element,
        None,
        None,
        &mut children,
        source,
        &mut out,
        &options,
        &make_resolver(),
        &mut buf,
        None,
        &make_ast(),
        false,                 // not block root (inner element test)
        false,                 // not force-open-block
        None,                  // no injected branch key
        None,                  // no hoisting in unit tests
        None,                  // no cache in unit tests
        None,                  // no resolved components in unit tests
        &NO_COMPONENT_TAG,     // no component tag in unit tests
        false,                 // not slot cached
        None,                  // no pre-reserved hoist in unit tests
        None,                  // no reserved handler caches in unit tests
        None,                  // no array-group cache reservation in unit tests
        &FxHashMap::default(), // no text-run cache reservations in unit tests
    );

    let result = apply_output(source, out, &alloc);
    assert_eq!(
        result,
        "_createElementVNode(\"div\", { class: \"foo\" }, \"hello\")"
    );
}

// ==================== Boolean attribute ====================

#[test]
fn element_with_boolean_attr() {
    let alloc = Allocator::default();
    let mut out = VdomOut::new(crate::code_transform::EditRecorder::new(&alloc));
    // <input disabled />
    let source = "<input disabled />";
    let mut element = make_element(make_tag(0, 18, 6), None, None);
    element.props.push(NodeProp {
        start: 7,
        name_end: 15,
        is_directive: false,
        arg_start: None,
        arg_end: None,
        is_dynamic: None,
        value_start: None,
        value_end: None,
        modifiers: SmallVec::new(),
    });
    let options = make_options();
    let mut children = Vec::new();

    let mut buf = String::new();
    process_element_leave(
        &element,
        None,
        None,
        &mut children,
        source,
        &mut out,
        &options,
        &make_resolver(),
        &mut buf,
        None,
        &make_ast(),
        false,                 // not block root (inner element test)
        false,                 // not force-open-block
        None,                  // no injected branch key
        None,                  // no hoisting in unit tests
        None,                  // no cache in unit tests
        None,                  // no resolved components in unit tests
        &NO_COMPONENT_TAG,     // no component tag in unit tests
        false,                 // not slot cached
        None,                  // no pre-reserved hoist in unit tests
        None,                  // no reserved handler caches in unit tests
        None,                  // no array-group cache reservation in unit tests
        &FxHashMap::default(), // no text-run cache reservations in unit tests
    );

    let result = apply_output(source, out, &alloc);
    assert_eq!(result, "_createElementVNode(\"input\", { disabled: \"\" })");
}

// ==================== Event handler key transformation ====================

#[test]
fn element_with_click_handler() {
    let alloc = Allocator::default();
    let mut out = VdomOut::new(crate::code_transform::EditRecorder::new(&alloc));
    // <div @click="handler"></div>
    let source = r#"<div @click="handler"></div>"#;
    let mut element = make_element(
        make_tag(0, 21, 4),
        Some(make_tag(21, 27, 26)),
        Some(ElementContent {
            start: 21,
            end: 21,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    element.props.push(NodeProp {
        start: 5,
        name_end: 6, // "@"
        is_directive: true,
        arg_start: Some(6),
        arg_end: Some(11), // "click"
        is_dynamic: None,
        value_start: Some(13),
        value_end: Some(20), // "handler"
        modifiers: SmallVec::new(),
    });
    element.prop_flag = PropFlag::empty().add(PropFlags::HasEventListener);
    let options = make_options();
    let mut children = Vec::new();

    let mut buf = String::new();
    process_element_leave(
        &element,
        None,
        None,
        &mut children,
        source,
        &mut out,
        &options,
        &make_resolver(),
        &mut buf,
        None,
        &make_ast(),
        false,                 // not block root (inner element test)
        false,                 // not force-open-block
        None,                  // no injected branch key
        None,                  // no hoisting in unit tests
        None,                  // no cache in unit tests
        None,                  // no resolved components in unit tests
        &NO_COMPONENT_TAG,     // no component tag in unit tests
        false,                 // not slot cached
        None,                  // no pre-reserved hoist in unit tests
        None,                  // no reserved handler caches in unit tests
        None,                  // no array-group cache reservation in unit tests
        &FxHashMap::default(), // no text-run cache reservations in unit tests
    );

    let result = apply_output(source, out, &alloc);
    assert!(
        result.contains("onClick"),
        "Expected onClick key, got: {}",
        result
    );
    assert!(result.contains("handler"));
}

// ==================== text_separator ====================

#[test]
fn text_sep_text_text() {
    assert_eq!(text_separator(ChildKind::Text, ChildKind::Text), "\" + \"");
}

#[test]
fn text_sep_text_interpolation() {
    assert_eq!(
        text_separator(ChildKind::Text, ChildKind::Interpolation),
        "\" + _toDisplayString"
    );
}

#[test]
fn text_sep_interpolation_text() {
    assert_eq!(
        text_separator(ChildKind::Interpolation, ChildKind::Text),
        " + \""
    );
}

#[test]
fn text_sep_interpolation_interpolation() {
    assert_eq!(
        text_separator(ChildKind::Interpolation, ChildKind::Interpolation),
        " + _toDisplayString"
    );
}
